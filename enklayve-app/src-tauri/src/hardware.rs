use anyhow::Result;
use serde::{Deserialize, Serialize};
use sysinfo::System;
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Platform {
    Windows,
    MacOS,
    Linux,
    Unknown,
}

impl Platform {
    pub fn detect() -> Self {
        #[cfg(target_os = "windows")]
        return Platform::Windows;

        #[cfg(target_os = "macos")]
        return Platform::MacOS;

        #[cfg(target_os = "linux")]
        return Platform::Linux;

        #[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
        return Platform::Unknown;
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum CpuVendor {
    Intel,
    AMD,
    AppleSilicon,
    ARM,
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum PerformanceTier {
    Excellent,  // Can run 70B+ models
    Good,       // Can run 8-13B models well
    Fair,       // Can run 3-8B models
    Poor,       // Can run 1-3B models only
    Minimal,    // Very limited, basic functionality only
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HardwareProfile {
    // CPU information
    pub cpu_vendor: CpuVendor,
    pub cpu_brand: String,
    pub cpu_cores: usize,
    pub cpu_threads: usize,

    // Memory information
    pub ram_total_gb: f64,
    pub ram_available_gb: f64,

    // GPU information (if available)
    pub has_gpu: bool,
    pub gpu_vendor: Option<String>,
    pub gpu_name: Option<String>,

    // Platform information
    pub platform: Platform,
    pub is_apple_silicon: bool,

    // Storage information
    pub storage_available_gb: f64,

    // Performance tier
    pub performance_tier: PerformanceTier,
}

impl HardwareProfile {
    pub fn detect() -> Result<Self> {
        let mut sys = System::new();
        sys.refresh_all();

        // CPU detection
        let cpu_brand = if let Some(cpu) = sys.cpus().first() {
            cpu.brand().to_string()
        } else {
            "Unknown CPU".to_string()
        };
        let cpu_cores = sys.physical_core_count().unwrap_or(1);
        let cpu_threads = sys.cpus().len();

        // Determine CPU vendor
        let cpu_vendor = if cpu_brand.contains("Apple") {
            CpuVendor::AppleSilicon
        } else if cpu_brand.contains("Intel") {
            CpuVendor::Intel
        } else if cpu_brand.contains("AMD") {
            CpuVendor::AMD
        } else if cpu_brand.contains("ARM") || cpu_brand.contains("Cortex") {
            CpuVendor::ARM
        } else {
            CpuVendor::Unknown
        };

        // Check if Apple Silicon
        let is_apple_silicon = matches!(cpu_vendor, CpuVendor::AppleSilicon);

        // Memory detection
        let ram_total_bytes = sys.total_memory();
        let ram_available_bytes = sys.available_memory();
        let ram_total_gb = ram_total_bytes as f64 / (1024.0 * 1024.0 * 1024.0);
        let ram_available_gb = ram_available_bytes as f64 / (1024.0 * 1024.0 * 1024.0);

        // Storage detection
        let storage_available_gb = Self::detect_storage_space()?;

        // GPU detection (basic for now)
        let (has_gpu, gpu_vendor, gpu_name) = Self::detect_gpu(&cpu_vendor, &cpu_brand);

        // Platform detection
        let platform = Platform::detect();

        // Determine performance tier
        let performance_tier = Self::calculate_performance_tier(
            ram_total_gb,
            cpu_cores,
            has_gpu,
            is_apple_silicon,
        );

        Ok(HardwareProfile {
            cpu_vendor,
            cpu_brand,
            cpu_cores,
            cpu_threads,
            ram_total_gb,
            ram_available_gb,
            has_gpu,
            gpu_vendor,
            gpu_name,
            platform,
            is_apple_silicon,
            storage_available_gb,
            performance_tier,
        })
    }

    fn detect_gpu(cpu_vendor: &CpuVendor, cpu_brand: &str) -> (bool, Option<String>, Option<String>) {
        // For Apple Silicon, the GPU is integrated
        if matches!(cpu_vendor, CpuVendor::AppleSilicon) {
            let gpu_cores = if cpu_brand.contains("M1 Pro") || cpu_brand.contains("M2 Pro") {
                "14-19 cores"
            } else if cpu_brand.contains("M1 Max") || cpu_brand.contains("M2 Max") {
                "24-38 cores"
            } else if cpu_brand.contains("M1 Ultra") || cpu_brand.contains("M2 Ultra") {
                "48-76 cores"
            } else if cpu_brand.contains("M3 Pro") {
                "14-18 cores"
            } else if cpu_brand.contains("M3 Max") {
                "30-40 cores"
            } else if cpu_brand.contains("M4") {
                "10+ cores"
            } else {
                "7-10 cores"
            };

            return (
                true,
                Some("Apple".to_string()),
                Some(format!("Apple GPU ({})", gpu_cores)),
            );
        }

        // Windows: Use DXGI to enumerate GPUs
        #[cfg(target_os = "windows")]
        {
            if let Some((vendor, name)) = Self::detect_windows_gpu() {
                return (true, Some(vendor), Some(name));
            } else {
                return (false, None, None);
            }
        }

        // macOS: Intel Macs might have AMD or Intel integrated graphics
        #[cfg(target_os = "macos")]
        {
            // Intel Macs might have AMD or Intel integrated graphics
            return (true, Some("Integrated".to_string()), Some("Graphics".to_string()));
        }

        // Linux and other platforms: placeholder for now
        #[cfg(not(any(target_os = "macos", target_os = "windows")))]
        {
            // On Linux, we could use lspci, vulkan, or other methods
            // For now, return no GPU
            (false, None, None)
        }
    }

    /// Windows-specific GPU detection using DXGI
    #[cfg(target_os = "windows")]
    fn detect_windows_gpu() -> Option<(String, String)> {
        use windows::Win32::Graphics::Dxgi::{
            CreateDXGIFactory1, IDXGIFactory1, DXGI_ADAPTER_DESC,
        };
        use windows::core::Interface;

        unsafe {
            // Create DXGI factory
            let factory: IDXGIFactory1 = match CreateDXGIFactory1() {
                Ok(f) => f,
                Err(_) => return None,
            };

            // Try to get the first adapter (primary GPU)
            let adapter = match factory.EnumAdapters(0) {
                Ok(a) => a,
                Err(_) => return None,
            };

            // Get adapter description
            let mut desc = DXGI_ADAPTER_DESC::default();
            if adapter.GetDesc(&mut desc).is_err() {
                return None;
            }

            // Convert wide string description to Rust string
            let description = String::from_utf16_lossy(&desc.Description);
            let gpu_name = description.trim_end_matches('\0').to_string();

            // Determine vendor based on vendor ID
            let vendor = match desc.VendorId {
                0x10DE => "NVIDIA",  // NVIDIA
                0x1002 => "AMD",     // AMD
                0x8086 => "Intel",   // Intel
                _ => "Unknown",
            };

            // Check if it's likely a dedicated GPU (has dedicated video memory > 1GB)
            let dedicated_memory_gb = desc.DedicatedVideoMemory as f64 / (1024.0 * 1024.0 * 1024.0);

            // Only report as GPU if it has significant dedicated memory (> 1GB)
            // or if it's NVIDIA/AMD (likely discrete GPU)
            if dedicated_memory_gb > 1.0 || vendor == "NVIDIA" || vendor == "AMD" {
                Some((vendor.to_string(), gpu_name))
            } else {
                // Integrated graphics with low memory - don't report as GPU for CUDA purposes
                None
            }
        }
    }

    fn detect_storage_space() -> Result<f64> {
        // Get home directory
        let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("/"));

        // For now, estimate 100GB available (placeholder)
        // In production, use platform-specific APIs to get actual disk space
        // Unix: statvfs, Windows: GetDiskFreeSpaceEx

        #[cfg(unix)]
        {
            if let Ok(_metadata) = std::fs::metadata(&home) {
                // This is a simplified version - actual implementation would use statvfs
                return Ok(100.0); // Placeholder
            }
        }

        Ok(100.0) // Default estimate
    }

    fn calculate_performance_tier(
        ram_gb: f64,
        cpu_cores: usize,
        has_gpu: bool,
        is_apple_silicon: bool,
    ) -> PerformanceTier {
        // Apple Silicon gets special treatment due to unified memory and efficiency
        if is_apple_silicon {
            if ram_gb >= 32.0 {
                return PerformanceTier::Excellent;
            } else if ram_gb >= 16.0 {
                return PerformanceTier::Good;
            } else {
                return PerformanceTier::Fair;
            }
        }

        // For other platforms
        if ram_gb >= 32.0 && (has_gpu || cpu_cores >= 8) {
            PerformanceTier::Excellent
        } else if ram_gb >= 16.0 && cpu_cores >= 4 {
            PerformanceTier::Good
        } else if ram_gb >= 8.0 && cpu_cores >= 2 {
            PerformanceTier::Fair
        } else if ram_gb >= 4.0 {
            PerformanceTier::Poor
        } else {
            PerformanceTier::Minimal
        }
    }

    /// Calculate optimal number of GPU layers based on hardware and model
    pub fn get_optimal_gpu_layers(&self, model_path: Option<&str>) -> u32 {
        // Determine max layers based on model architecture
        let max_layers = if let Some(path) = model_path {
            Self::get_model_layer_count(path)
        } else {
            // Conservative default if model unknown
            999 // Use percentage allocation below
        };

        // Apple Silicon with unified memory architecture: the GPU and CPU share
        // the same RAM, so there is no separate VRAM budget to stay under.
        // Offloading every layer is almost always fastest — any layer left on the
        // CPU forces a per-token round trip that drags generation down. The model
        // selection already right-sizes the model to this machine's RAM, so if it
        // loaded, it fits. Only fall back to CPU on very low-memory machines.
        if self.is_apple_silicon {
            if self.ram_total_gb >= 8.0 {
                return max_layers; // 100% on GPU
            } else {
                return 0; // Keep on CPU if very low RAM
            }
        }

        // NVIDIA GPU (CUDA) - Windows/Linux with discrete GPU
        if self.has_gpu {
            // Aggressive GPU offloading for CUDA (separate VRAM pool)
            if self.ram_total_gb >= 64.0 {
                return max_layers; // 100% on GPU (ultra high-end)
            } else if self.ram_total_gb >= 32.0 {
                return std::cmp::min(max_layers, (max_layers as f32 * 0.90) as u32); // 90% on GPU
            } else if self.ram_total_gb >= 16.0 {
                return std::cmp::min(max_layers, (max_layers as f32 * 0.85) as u32); // 85% on GPU
            } else if self.ram_total_gb >= 8.0 {
                return std::cmp::min(max_layers, (max_layers as f32 * 0.60) as u32); // 60% on GPU
            } else {
                return std::cmp::min(max_layers, (max_layers as f32 * 0.40) as u32); // 40% on GPU
            }
        }

        // CPU-only fallback (Intel/AMD without GPU)
        0
    }

    /// Determine model layer count from filename
    fn get_model_layer_count(path: &str) -> u32 {
        let path_lower = path.to_lowercase();

        // Extract model family and size from path
        // Llama family
        if path_lower.contains("llama") || path_lower.contains("meta-llama") {
            if path_lower.contains("70b") || path_lower.contains("65b") {
                return 80; // Llama 3.1/3 70B, Llama 2 65B
            } else if path_lower.contains("45b") {
                return 80; // Llama 3.1 45B (custom dense variant)
            } else if path_lower.contains("34b") {
                return 48; // CodeLlama 34B
            } else if path_lower.contains("13b") {
                return 40; // Llama 2/3 13B
            } else if path_lower.contains("8b") || path_lower.contains("7b") {
                return 32; // Llama 3.1/3.2 8B, Llama 2 7B
            } else if path_lower.contains("3b") {
                return 26; // Llama 3.2 3B
            }
        }

        // Qwen family (Alibaba)
        if path_lower.contains("qwen") {
            if path_lower.contains("72b") {
                return 80; // Qwen2.5 72B
            } else if path_lower.contains("32b") {
                return 64; // Qwen2.5 32B
            } else if path_lower.contains("14b") {
                return 40; // Qwen2.5 14B
            } else if path_lower.contains("7b") {
                return 32; // Qwen2.5 7B
            }
        }

        // Mistral family
        if path_lower.contains("mistral") {
            if path_lower.contains("nemo") || path_lower.contains("12b") {
                return 40; // Mistral Nemo 12B
            } else if path_lower.contains("7b") {
                return 32; // Mistral 7B
            }
        }

        // Mixtral (Mixture of Experts)
        if path_lower.contains("mixtral") {
            if path_lower.contains("8x7b") || path_lower.contains("8x22b") {
                return 32; // Mixtral uses 32 layers (8 experts per layer)
            }
        }

        // Gemma family (Google)
        if path_lower.contains("gemma") {
            if path_lower.contains("27b") {
                return 46; // Gemma 2 27B
            } else if path_lower.contains("9b") {
                return 42; // Gemma 2 9B
            } else if path_lower.contains("7b") {
                return 28; // Gemma 1 7B
            } else if path_lower.contains("2b") {
                return 18; // Gemma 1/2 2B
            }
        }

        // DeepSeek family
        if path_lower.contains("deepseek") {
            if path_lower.contains("33b") {
                return 60; // DeepSeek Coder 33B
            } else if path_lower.contains("67b") {
                return 95; // DeepSeek 67B
            }
        }

        // CodeLlama
        if path_lower.contains("codellama") {
            if path_lower.contains("34b") {
                return 48; // CodeLlama 34B
            } else if path_lower.contains("13b") {
                return 40; // CodeLlama 13B
            } else if path_lower.contains("7b") {
                return 32; // CodeLlama 7B
            }
        }

        // Conservative fallback: assume mid-range model
        // 40 layers is common for 12-13B models
        40
    }

    pub fn get_summary(&self) -> String {
        format!(
            "Platform: {:?}\nCPU: {} ({} cores, {} threads)\nRAM: {:.1} GB total ({:.1} GB available)\nGPU: {}\nPerformance: {:?}",
            self.platform,
            self.cpu_brand,
            self.cpu_cores,
            self.cpu_threads,
            self.ram_total_gb,
            self.ram_available_gb,
            if self.has_gpu {
                format!("{} - {}",
                    self.gpu_vendor.as_ref().unwrap_or(&"Unknown".to_string()),
                    self.gpu_name.as_ref().unwrap_or(&"GPU".to_string())
                )
            } else {
                "No dedicated GPU detected".to_string()
            },
            self.performance_tier
        )
    }

    pub fn get_hardware_summary(&self) -> String {
        let ram_display = format!("{:.0}GB RAM", self.ram_total_gb);

        let cpu_display = if self.is_apple_silicon {
            if self.cpu_brand.contains("M1") || self.cpu_brand.contains("M2") ||
               self.cpu_brand.contains("M3") || self.cpu_brand.contains("M4") {
                self.cpu_brand.split_whitespace().take(2).collect::<Vec<_>>().join(" ")
            } else {
                "Apple Silicon".to_string()
            }
        } else {
            match self.cpu_vendor {
                CpuVendor::Intel => format!("Intel {} cores", self.cpu_cores),
                CpuVendor::AMD => format!("AMD {} cores", self.cpu_cores),
                _ => format!("{} cores", self.cpu_cores),
            }
        };

        format!("{}, {}", ram_display, cpu_display)
    }
}

// For dirs crate (to get home directory)
mod dirs {
    use std::path::PathBuf;

    pub fn home_dir() -> Option<PathBuf> {
        std::env::var_os("HOME")
            .or_else(|| std::env::var_os("USERPROFILE"))
            .map(PathBuf::from)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hardware_detection() {
        let profile = HardwareProfile::detect().unwrap();
        println!("Hardware Profile:\n{}", profile.get_summary());

        assert!(profile.cpu_cores > 0);
        assert!(profile.ram_total_gb > 0.0);
        assert!(!profile.cpu_brand.is_empty());
    }

    #[test]
    fn test_platform_detection() {
        let platform = Platform::detect();
        println!("Platform: {:?}", platform);
        assert!(!matches!(platform, Platform::Unknown));
    }
}
