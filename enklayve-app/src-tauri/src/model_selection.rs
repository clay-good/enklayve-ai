use serde::{Deserialize, Serialize};
use crate::hardware::HardwareProfile;
use crate::models::ModelInfo;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BestModelSelection {
    pub model: ModelInfo,
    pub explanation: String,
}

pub fn get_best_model_for_hardware(hardware: &HardwareProfile) -> BestModelSelection {
    let ram_gb = hardware.ram_total_gb;
    let storage_gb = hardware.storage_available_gb;

    // Models from most to least capable.
    let ordered = [
        "Qwen3 32B (Q4)",
        "Qwen3 14B (Q4)",
        "Qwen3 8B (Q4)",
        "Qwen3 4B (Q4)",
        "Qwen3 1.7B (Q4)",
    ];

    // The best tier this machine's RAM supports.
    let ram_tier = if ram_gb >= 64.0 {
        0
    } else if ram_gb >= 32.0 {
        1
    } else if ram_gb >= 16.0 {
        2
    } else if ram_gb >= 8.0 {
        3
    } else {
        4
    };

    // Leave headroom so we never fill the disk completely.
    const DISK_HEADROOM_GB: f64 = 2.0;

    // From the RAM tier downward, pick the first model that also fits on disk.
    let mut chosen: Option<ModelInfo> = None;
    let mut disk_constrained = false;
    for name in ordered.iter().skip(ram_tier) {
        if let Some(m) = find_model_by_name(name) {
            if (m.size_gb as f64 + DISK_HEADROOM_GB) <= storage_gb {
                chosen = Some(m);
                break;
            }
            // RAM could run it, but the disk can't hold it: step down.
            disk_constrained = true;
        }
    }

    let model = chosen.unwrap_or_else(|| {
        find_model_by_name("Qwen3 1.7B (Q4)").unwrap_or_else(get_fallback_model)
    });

    let reason = match model.name.as_str() {
        "Qwen3 32B (Q4)" => "maximum intelligence for your high-end system",
        "Qwen3 14B (Q4)" => "a very smart model for your system",
        "Qwen3 8B (Q4)" => "balanced intelligence and speed - recommended for most users",
        "Qwen3 4B (Q4)" => "fast and efficient for your system",
        _ => "a lightweight model that fits your system",
    };

    let hardware_summary = hardware.get_hardware_summary();
    let explanation = if disk_constrained {
        format!(
            "Best model that fits your {:.0} GB of free disk space: {} for your {}",
            storage_gb, reason, hardware_summary
        )
    } else {
        format!("Best {} for your {}", reason, hardware_summary)
    };

    BestModelSelection { model, explanation }
}

fn find_model_by_name(name: &str) -> Option<ModelInfo> {
    let all_models = crate::models::get_available_models();
    all_models.into_iter().find(|m| m.name == name)
}

fn get_fallback_model() -> ModelInfo {
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
    }
}

/// The model the app will actually run, chosen automatically from what's
/// already downloaded and what this machine can handle. The user never picks.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActiveModelSelection {
    pub path: String,
    pub name: String,
    pub size_gb: f64,
    pub performance_tier: String,
    pub context_length: u32,
    /// True when more than one model was available and we chose among them.
    pub auto_selected: bool,
    /// Short, friendly explanation for the About panel.
    pub note: String,
}

/// Pick the model to run from the set that's downloaded: the most capable one
/// this machine's memory can handle, stepping down automatically when needed so
/// the user never has to think about it. Falls back to the lightest download if
/// none comfortably fit.
pub fn pick_active_model(
    downloaded: &[crate::downloads::DownloadedModelInfo],
    hardware: &HardwareProfile,
) -> Option<ActiveModelSelection> {
    if downloaded.is_empty() {
        return None;
    }
    let known = crate::models::get_available_models();
    let ram = hardware.ram_total_gb;

    // Pair each download with what we know about it (by file name). We clone the
    // download info so every comparison below works with plain single references.
    let mut candidates: Vec<(crate::downloads::DownloadedModelInfo, Option<ModelInfo>)> = downloaded
        .iter()
        .map(|d| (d.clone(), known.iter().find(|m| m.file_name == d.name).cloned()))
        .collect();

    // Prefer models whose minimum RAM this machine meets; among those, the
    // largest (most capable). If none fit, take the smallest download so
    // something still runs.
    let fits = |m: &Option<ModelInfo>, d: &crate::downloads::DownloadedModelInfo| -> bool {
        match m {
            Some(mi) => ram >= mi.min_ram_gb as f64,
            // Unknown file: estimate it needs roughly its own size in RAM.
            None => ram >= d.size_gb + 1.0,
        }
    };

    let size_of = |m: &Option<ModelInfo>, d: &crate::downloads::DownloadedModelInfo| -> f64 {
        m.as_ref().map(|mi| mi.size_gb as f64).unwrap_or(d.size_gb)
    };

    let any_fit = candidates.iter().any(|(d, m)| fits(m, d));
    let auto_selected = candidates.len() > 1;

    let chosen = if any_fit {
        candidates.retain(|(d, m)| fits(m, d));
        candidates
            .into_iter()
            .max_by(|(da, ma), (db, mb)| {
                size_of(ma, da)
                    .partial_cmp(&size_of(mb, db))
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
    } else {
        candidates.into_iter().min_by(|(da, ma), (db, mb)| {
            size_of(ma, da)
                .partial_cmp(&size_of(mb, db))
                .unwrap_or(std::cmp::Ordering::Equal)
        })
    }?;

    let (d, m) = chosen;
    let name = m.as_ref().map(|mi| mi.name.clone()).unwrap_or_else(|| d.name.clone());
    let size_gb = size_of(&m, &d);
    let performance_tier = m
        .as_ref()
        .map(|mi| mi.performance_tier.clone())
        .unwrap_or_else(|| "Local".to_string());
    let context_length = m.as_ref().map(|mi| mi.context_length).unwrap_or(0);

    let note = if !any_fit {
        format!("Using the lightest model that fits your {:.0} GB of memory.", ram)
    } else if auto_selected {
        format!("Auto-selected for your {}.", hardware.get_hardware_summary())
    } else {
        format!("Running on your {}.", hardware.get_hardware_summary())
    };

    Some(ActiveModelSelection {
        path: d.path.clone(),
        name,
        size_gb,
        performance_tier,
        context_length,
        auto_selected,
        note,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hardware::{PerformanceTier, CpuVendor, Platform};

    fn create_test_hardware(ram_gb: f64) -> HardwareProfile {
        HardwareProfile {
            cpu_vendor: CpuVendor::AppleSilicon,
            cpu_brand: "Apple M2".to_string(),
            cpu_cores: 8,
            cpu_threads: 8,
            ram_total_gb: ram_gb,
            ram_available_gb: ram_gb * 0.7,
            has_gpu: true,
            gpu_vendor: Some("Apple".to_string()),
            gpu_name: Some("Apple GPU".to_string()),
            platform: Platform::MacOS,
            is_apple_silicon: true,
            storage_available_gb: 200.0,
            performance_tier: PerformanceTier::Good,
        }
    }

    #[test]
    fn test_ultra_high_end_selection() {
        let hardware = create_test_hardware(64.0);
        let selection = get_best_model_for_hardware(&hardware);
        assert!(selection.model.name.contains("32B"));
        assert!(!selection.explanation.is_empty());
    }

    #[test]
    fn test_high_end_selection() {
        let hardware = create_test_hardware(32.0);
        let selection = get_best_model_for_hardware(&hardware);
        assert!(selection.model.name.contains("14B"));
    }

    #[test]
    fn test_mid_range_selection() {
        let hardware = create_test_hardware(16.0);
        let selection = get_best_model_for_hardware(&hardware);
        assert!(selection.model.name.contains("8B"));
    }

    #[test]
    fn test_low_end_selection() {
        let hardware = create_test_hardware(8.0);
        let selection = get_best_model_for_hardware(&hardware);
        assert!(selection.model.name.contains("4B"));
        assert!(selection.model.size_gb <= 2.5);
    }

    fn dl(file_name: &str, size_gb: f64) -> crate::downloads::DownloadedModelInfo {
        crate::downloads::DownloadedModelInfo {
            name: file_name.to_string(),
            path: format!("/models/{}", file_name),
            size_bytes: (size_gb * 1_073_741_824.0) as u64,
            size_gb,
        }
    }

    #[test]
    fn test_pick_active_prefers_most_capable_that_fits() {
        let hardware = create_test_hardware(16.0);
        let downloaded = vec![
            dl("Qwen3-4B-Q4_K_M.gguf", 2.3),
            dl("Qwen3-8B-Q4_K_M.gguf", 4.7),
            dl("Qwen3-14B-Q4_K_M.gguf", 8.4), // needs 16GB min -> fits exactly
        ];
        let sel = pick_active_model(&downloaded, &hardware).unwrap();
        assert!(sel.name.contains("14B"));
        assert!(sel.auto_selected);
    }

    #[test]
    fn test_pick_active_steps_down_when_nothing_fits() {
        let hardware = create_test_hardware(8.0);
        // Only large models present; none meet their min RAM on an 8GB machine.
        let downloaded = vec![
            dl("Qwen3-14B-Q4_K_M.gguf", 8.4),
            dl("Qwen3-32B-Q4_K_M.gguf", 18.4),
        ];
        let sel = pick_active_model(&downloaded, &hardware).unwrap();
        assert!(sel.name.contains("14B"), "should fall back to the lightest download");
    }

    #[test]
    fn test_pick_active_none_when_empty() {
        let hardware = create_test_hardware(16.0);
        assert!(pick_active_model(&[], &hardware).is_none());
    }

    #[test]
    fn test_minimal_selection() {
        let hardware = create_test_hardware(4.0);
        let selection = get_best_model_for_hardware(&hardware);
        assert!(selection.model.name.contains("1.7B"));
        assert!(selection.model.size_gb <= 1.5);
    }

    #[test]
    fn test_disk_constrained_steps_down() {
        // Plenty of RAM for the 8B pick, but only 6 GB free disk.
        let mut hardware = create_test_hardware(16.0);
        hardware.storage_available_gb = 6.0;
        let selection = get_best_model_for_hardware(&hardware);
        // 8B (4.7) + 2 GB headroom = 6.7 > 6, so it must step down to 4B (2.3).
        assert!(selection.model.name.contains("4B"), "got {}", selection.model.name);
        assert!((selection.model.size_gb as f64 + 2.0) <= 6.0);
    }

    #[test]
    fn test_never_exceeds_disk() {
        for ram in [8.0, 16.0, 32.0, 64.0] {
            let mut hardware = create_test_hardware(ram);
            hardware.storage_available_gb = 5.0;
            let selection = get_best_model_for_hardware(&hardware);
            assert!(
                selection.model.size_gb <= 5.0,
                "{} GB model chosen for 5 GB disk",
                selection.model.size_gb
            );
        }
    }
}
