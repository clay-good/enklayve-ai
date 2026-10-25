use serde::{Deserialize, Serialize};
use crate::hardware::{HardwareProfile, PerformanceTier};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelInfo {
    pub name: String,
    pub description: String,
    pub size_gb: f32,
    pub min_ram_gb: u32,
    pub recommended_ram_gb: u32,
    pub repo_url: String,
    pub file_name: String,
    pub checksum: String,
    pub recommended_use: String,
    pub performance_tier: String,
    pub estimated_speed_tokens_per_sec: u32,
    pub context_length: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum CompatibilityLevel {
    Excellent,
    Good,
    Acceptable,
    Poor,
    Incompatible,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelRecommendation {
    pub model: ModelInfo,
    pub compatibility: CompatibilityLevel,
    pub is_recommended: bool,
    pub estimated_speed: String,
    pub warnings: Vec<String>,
    pub benefits: Vec<String>,
}

pub fn get_available_models() -> Vec<ModelInfo> {
    vec![
        ModelInfo {
            name: "Qwen3 1.7B (Q4)".to_string(),
            description: "Ultra-efficient reasoning model for minimal hardware - perfect for basic Q&A".to_string(),
            size_gb: 1.0,
            min_ram_gb: 4,
            recommended_ram_gb: 6,
            repo_url: "https://huggingface.co/unsloth/Qwen3-1.7B-GGUF".to_string(),
            file_name: "Qwen3-1.7B-Q4_K_M.gguf".to_string(),
            checksum: "".to_string(),
            recommended_use: "Fast responses, simple document Q&A, basic summarization".to_string(),
            performance_tier: "Fast".to_string(),
            estimated_speed_tokens_per_sec: 85,
            context_length: 32768,
        },
        ModelInfo {
            name: "Qwen3 4B (Q4)".to_string(),
            description: "Fast and efficient reasoning model with strong performance for most tasks".to_string(),
            size_gb: 2.3,
            min_ram_gb: 6,
            recommended_ram_gb: 8,
            repo_url: "https://huggingface.co/unsloth/Qwen3-4B-GGUF".to_string(),
            file_name: "Qwen3-4B-Q4_K_M.gguf".to_string(),
            checksum: "".to_string(),
            recommended_use: "Document analysis, coding help, technical content, multilingual support".to_string(),
            performance_tier: "Fast".to_string(),
            estimated_speed_tokens_per_sec: 60,
            context_length: 32768,
        },
        ModelInfo {
            name: "Qwen3 8B (Q4)".to_string(),
            description: "Balanced intelligence and speed - recommended for most users".to_string(),
            size_gb: 4.7,
            min_ram_gb: 8,
            recommended_ram_gb: 16,
            repo_url: "https://huggingface.co/unsloth/Qwen3-8B-GGUF".to_string(),
            file_name: "Qwen3-8B-Q4_K_M.gguf".to_string(),
            checksum: "".to_string(),
            recommended_use: "Complex reasoning, technical docs, code analysis, mathematics, research".to_string(),
            performance_tier: "Balanced".to_string(),
            estimated_speed_tokens_per_sec: 45,
            context_length: 32768,
        },
        ModelInfo {
            name: "Qwen3 14B (Q4)".to_string(),
            description: "Very smart reasoning model for advanced analysis and complex tasks".to_string(),
            size_gb: 8.4,
            min_ram_gb: 16,
            recommended_ram_gb: 32,
            repo_url: "https://huggingface.co/unsloth/Qwen3-14B-GGUF".to_string(),
            file_name: "Qwen3-14B-Q4_K_M.gguf".to_string(),
            checksum: "".to_string(),
            recommended_use: "Advanced reasoning, professional writing, research, complex technical analysis".to_string(),
            performance_tier: "Smart".to_string(),
            estimated_speed_tokens_per_sec: 30,
            context_length: 32768,
        },
        ModelInfo {
            name: "Qwen3 32B (Q4)".to_string(),
            description: "Maximum intelligence for the most demanding tasks and research".to_string(),
            size_gb: 18.4,
            min_ram_gb: 32,
            recommended_ram_gb: 64,
            repo_url: "https://huggingface.co/unsloth/Qwen3-32B-GGUF".to_string(),
            file_name: "Qwen3-32B-Q4_K_M.gguf".to_string(),
            checksum: "".to_string(),
            recommended_use: "Expert-level analysis, complex research, advanced coding, scientific work".to_string(),
            performance_tier: "Maximum".to_string(),
            estimated_speed_tokens_per_sec: 15,
            context_length: 32768,
        },
    ]
}

pub fn get_recommended_models(hardware: &HardwareProfile) -> Vec<ModelRecommendation> {
    let all_models = get_available_models();
    let mut recommendations: Vec<ModelRecommendation> = all_models
        .into_iter()
        .map(|model| evaluate_model_compatibility(&model, hardware))
        .collect();

    recommendations.sort_by(|a, b| {
        let compat_order = |c: &CompatibilityLevel| -> i32 {
            match c {
                CompatibilityLevel::Excellent => 0,
                CompatibilityLevel::Good => 1,
                CompatibilityLevel::Acceptable => 2,
                CompatibilityLevel::Poor => 3,
                CompatibilityLevel::Incompatible => 4,
            }
        };

        let a_order = compat_order(&a.compatibility);
        let b_order = compat_order(&b.compatibility);

        if a_order != b_order {
            a_order.cmp(&b_order)
        } else {
            // Handle NaN safely - treat NaN as smallest value
            b.model.size_gb.partial_cmp(&a.model.size_gb)
                .unwrap_or(std::cmp::Ordering::Equal)
        }
    });

    recommendations
}

fn evaluate_model_compatibility(
    model: &ModelInfo,
    hardware: &HardwareProfile,
) -> ModelRecommendation {
    let available_ram = hardware.ram_total_gb;
    let mut warnings = Vec::new();
    let mut benefits = Vec::new();

    let compatibility = if available_ram < model.min_ram_gb as f64 {
        warnings.push(format!(
            "Requires {} GB RAM but only {:.1} GB available",
            model.min_ram_gb, available_ram
        ));
        CompatibilityLevel::Incompatible
    } else if available_ram >= model.recommended_ram_gb as f64 {
        benefits.push("Plenty of RAM for smooth performance".to_string());
        if hardware.is_apple_silicon {
            benefits.push("Optimized for Apple Silicon unified memory".to_string());
        }
        CompatibilityLevel::Excellent
    } else if available_ram >= (model.min_ram_gb as f64 + 2.0) {
        benefits.push("Sufficient RAM for good performance".to_string());
        CompatibilityLevel::Good
    } else if available_ram >= model.min_ram_gb as f64 {
        warnings.push("Meets minimum RAM but may be slower".to_string());
        warnings.push("Consider closing other applications".to_string());
        CompatibilityLevel::Acceptable
    } else {
        warnings.push("Below recommended specifications".to_string());
        CompatibilityLevel::Poor
    };

    if hardware.storage_available_gb < (model.size_gb as f64 + 5.0) {
        warnings.push(format!(
            "Needs {:.1} GB storage, only {:.1} GB available",
            model.size_gb + 5.0,
            hardware.storage_available_gb
        ));
    }

    let is_recommended = match (&hardware.performance_tier, model.performance_tier.as_str()) {
        (PerformanceTier::Excellent, "Maximum" | "Smart") => {
            benefits.push("Perfect match for your high-end hardware".to_string());
            true
        }
        (PerformanceTier::Good, "Balanced" | "Smart") => {
            benefits.push("Ideal for your system".to_string());
            true
        }
        (PerformanceTier::Fair, "Balanced" | "Fast") => {
            benefits.push("Well-matched to your hardware".to_string());
            true
        }
        (PerformanceTier::Poor | PerformanceTier::Minimal, "Fast") => {
            benefits.push("Best option for your system".to_string());
            true
        }
        (PerformanceTier::Excellent, "Balanced" | "Fast") => {
            benefits.push("Will run very fast on your hardware".to_string());
            false
        }
        (PerformanceTier::Good, "Fast") => {
            benefits.push("Fast performance expected".to_string());
            false
        }
        (PerformanceTier::Fair | PerformanceTier::Good, "Maximum") => {
            warnings.push("May be slower than optimal".to_string());
            false
        }
        _ => false,
    };

    let estimated_speed = if hardware.is_apple_silicon {
        match &hardware.performance_tier {
            PerformanceTier::Excellent => "Very Fast (50-100+ tokens/sec)".to_string(),
            PerformanceTier::Good => "Fast (30-50 tokens/sec)".to_string(),
            PerformanceTier::Fair => "Moderate (15-30 tokens/sec)".to_string(),
            PerformanceTier::Poor => "Slow (5-15 tokens/sec)".to_string(),
            PerformanceTier::Minimal => "Very Slow (<5 tokens/sec)".to_string(),
        }
    } else {
        match &hardware.performance_tier {
            PerformanceTier::Excellent => "Fast (30-60 tokens/sec)".to_string(),
            PerformanceTier::Good => "Moderate (20-40 tokens/sec)".to_string(),
            PerformanceTier::Fair => "Slow (10-20 tokens/sec)".to_string(),
            PerformanceTier::Poor => "Very Slow (3-10 tokens/sec)".to_string(),
            PerformanceTier::Minimal => "Extremely Slow (<3 tokens/sec)".to_string(),
        }
    };

    if hardware.is_apple_silicon && hardware.cpu_cores >= 8 {
        benefits.push("High-performance cores will accelerate inference".to_string());
    }

    if model.context_length >= 32768 {
        benefits.push(format!(
            "Large context window ({} tokens) for extensive documents",
            model.context_length
        ));
    }

    ModelRecommendation {
        model: model.clone(),
        compatibility,
        is_recommended,
        estimated_speed,
        warnings,
        benefits,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hardware::{HardwareProfile, PerformanceTier, CpuVendor, Platform};

    fn create_test_hardware(ram_gb: f64, tier: PerformanceTier) -> HardwareProfile {
        HardwareProfile {
            cpu_vendor: CpuVendor::AppleSilicon,
            cpu_brand: "Apple M2 Pro".to_string(),
            cpu_cores: 10,
            cpu_threads: 10,
            ram_total_gb: ram_gb,
            ram_available_gb: ram_gb * 0.7,
            has_gpu: true,
            gpu_vendor: Some("Apple".to_string()),
            gpu_name: Some("Apple GPU".to_string()),
            platform: Platform::MacOS,
            is_apple_silicon: true,
            storage_available_gb: 200.0,
            performance_tier: tier,
        }
    }

    #[test]
    fn test_model_count() {
        let models = get_available_models();
        assert_eq!(models.len(), 5, "Should have exactly 5 Qwen3 models");

        for model in &models {
            assert!(model.name.starts_with("Qwen3"), "All models should be Qwen3 series");
        }
    }

    #[test]
    fn test_recommendation_for_excellent_hardware() {
        let hardware = create_test_hardware(64.0, PerformanceTier::Excellent);
        let recommendations = get_recommended_models(&hardware);

        let has_recommended = recommendations.iter().any(|r| r.is_recommended);
        assert!(has_recommended, "Should have at least one recommended model");

        assert!(
            !matches!(recommendations[0].compatibility, CompatibilityLevel::Incompatible),
            "Top recommendation should be compatible"
        );
    }

    #[test]
    fn test_recommendation_for_good_hardware() {
        let hardware = create_test_hardware(16.0, PerformanceTier::Good);
        let recommendations = get_recommended_models(&hardware);

        let compatible_count = recommendations
            .iter()
            .filter(|r| !matches!(r.compatibility, CompatibilityLevel::Incompatible))
            .count();

        assert!(compatible_count >= 3, "Should have at least 3 compatible models");
    }

    #[test]
    fn test_recommendation_for_fair_hardware() {
        let hardware = create_test_hardware(8.0, PerformanceTier::Fair);
        let recommendations = get_recommended_models(&hardware);

        let compatible_count = recommendations
            .iter()
            .filter(|r| !matches!(r.compatibility, CompatibilityLevel::Incompatible))
            .count();

        assert!(compatible_count >= 2, "Should have at least 2 compatible models");
    }

    #[test]
    fn test_incompatible_model_detection() {
        let hardware = create_test_hardware(4.0, PerformanceTier::Poor);
        let recommendations = get_recommended_models(&hardware);

        let large_models: Vec<_> = recommendations
            .iter()
            .filter(|r| r.model.size_gb > 10.0)
            .collect();

        for model in large_models {
            assert!(
                matches!(model.compatibility, CompatibilityLevel::Incompatible),
                "Large models ({}GB) should be incompatible with 4GB RAM",
                model.model.size_gb
            );
            assert!(!model.warnings.is_empty(), "Should have warnings for large models");
        }

        let compatible_count = recommendations
            .iter()
            .filter(|r| !matches!(r.compatibility, CompatibilityLevel::Incompatible))
            .count();

        assert!(compatible_count >= 1, "Should have at least 1 compatible model for 4GB RAM");
    }
}
