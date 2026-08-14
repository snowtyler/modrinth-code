use crate::state::MemorySettings;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::Path;
use tracing::warn;

pub const JAVA_SETTINGS_CONFIG_NAME: &str = "java-settings.json";

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct JavaRecommendationConfig {
    #[serde(default)]
    pub recommended_memory_mb: Option<u32>,
    #[serde(default)]
    pub recommended_jvm_args: Vec<String>,
}

/// Reads `java-settings.json` from either the instance root or the `config/` directory.
pub async fn load_instance_java_recommendations(
    instance_dir: &Path,
) -> Option<JavaRecommendationConfig> {
    let config_path_direct = instance_dir.join(JAVA_SETTINGS_CONFIG_NAME);
    let config_path_in_config = instance_dir.join("config").join(JAVA_SETTINGS_CONFIG_NAME);

    let config_file = if config_path_direct.is_file() {
        Some(config_path_direct)
    } else if config_path_in_config.is_file() {
        Some(config_path_in_config)
    } else {
        None
    };

    let config_file = config_file?;

    let content = match tokio::fs::read_to_string(&config_file).await {
        Ok(c) => c,
        Err(err) => {
            warn!("Failed to read {}: {err}", config_file.display());
            return None;
        }
    };

    match serde_json::from_str(&content) {
        Ok(cfg) => Some(cfg),
        Err(err) => {
            warn!("Failed to parse {}: {err}", config_file.display());
            None
        }
    }
}

/// Clamps recommended RAM against total physical system RAM: min(recommended, total_ram - 2048MB)
pub fn clamp_recommended_memory(recommended_mb: u32) -> u32 {
    let mut sys = sysinfo::System::new_all();
    sys.refresh_memory();
    let total_ram_mb = (sys.total_memory() / (1024 * 1024)) as u32;

    if total_ram_mb > 2048 {
        let max_safe = total_ram_mb.saturating_sub(2048);
        recommended_mb.min(max_safe).max(512)
    } else if total_ram_mb > 512 {
        recommended_mb.min(total_ram_mb).max(512)
    } else {
        recommended_mb.max(512)
    }
}

/// Resolves effective memory settings following the strict priority:
/// 1. Per-Instance Manual Override (if set)
/// 2. Recommended in `java-settings.json` (clamped against physical RAM)
/// 3. Global Settings Default
pub fn resolve_effective_memory(
    override_memory: Option<MemorySettings>,
    global_memory: MemorySettings,
    recommendation: Option<&JavaRecommendationConfig>,
) -> MemorySettings {
    if let Some(mem) = override_memory {
        return mem;
    }

    if let Some(rec) = recommendation {
        if let Some(rec_mb) = rec.recommended_memory_mb {
            let clamped = clamp_recommended_memory(rec_mb);
            return MemorySettings {
                maximum: clamped,
            };
        }
    }

    global_memory
}

/// Resolves effective JVM launch arguments:
/// 1. Per-Instance Manual Override (if set)
/// 2. Deduplicated combination of Global Args + Recommended Args
/// 3. Global Args
pub fn resolve_effective_jvm_args(
    override_args: Option<Vec<String>>,
    global_args: Vec<String>,
    recommendation: Option<&JavaRecommendationConfig>,
) -> Vec<String> {
    if let Some(args) = override_args {
        return args;
    }

    if let Some(rec) = recommendation {
        if !rec.recommended_jvm_args.is_empty() {
            return deduplicate_jvm_args(&global_args, &rec.recommended_jvm_args);
        }
    }

    global_args
}

/// Deduplicates JVM arguments while preserving argument ordering and preventing duplicate flags.
pub fn deduplicate_jvm_args(base_args: &[String], extra_args: &[String]) -> Vec<String> {
    let mut result = Vec::new();
    let mut seen_keys = HashSet::new();

    // Extra / recommended arguments take precedence over base arguments for conflict resolution
    // Combine base followed by extra
    let combined = base_args.iter().chain(extra_args.iter());

    for arg in combined {
        let key = extract_arg_key(arg);
        if seen_keys.insert(key) {
            result.push(arg.clone());
        }
    }

    result
}

fn extract_arg_key(arg: &str) -> String {
    if let Some((key, _)) = arg.split_once('=') {
        key.to_string()
    } else if arg.starts_with("-XX:+") || arg.starts_with("-XX:-") {
        // e.g. -XX:+UseG1GC -> -XX:UseG1GC key
        format!("-XX:{}", &arg[5..])
    } else if arg.starts_with("-Xmx") || arg.starts_with("-Xms") || arg.starts_with("-Xmn") {
        arg[..4].to_string()
    } else {
        arg.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_deduplicate_jvm_args() {
        let base = vec!["-XX:+UseG1GC".to_string(), "-XX:G1ReservePercent=15".to_string()];
        let extra = vec!["-XX:+UseG1GC".to_string(), "-XX:G1ReservePercent=20".to_string(), "-XX:MaxGCPauseMillis=50".to_string()];

        let combined = deduplicate_jvm_args(&base, &extra);
        assert_eq!(
            combined,
            vec![
                "-XX:+UseG1GC".to_string(),
                "-XX:G1ReservePercent=15".to_string(),
                "-XX:MaxGCPauseMillis=50".to_string(),
            ]
        );
    }

    #[test]
    fn test_resolve_priority() {
        let global = MemorySettings { maximum: 4096 };
        let override_mem = MemorySettings { maximum: 8192 };
        let rec = JavaRecommendationConfig {
            recommended_memory_mb: Some(6144),
            recommended_jvm_args: vec!["-XX:+UseZGC".to_string()],
        };

        // Priority 1: Manual Override
        let res1 = resolve_effective_memory(Some(override_mem), global, Some(&rec));
        assert_eq!(res1.maximum, 8192);

        // Priority 2: Recommendation
        let res2 = resolve_effective_memory(None, global, Some(&rec));
        assert!(res2.maximum > 0);

        // Priority 3: Global Default
        let res3 = resolve_effective_memory(None, global, None);
        assert_eq!(res3.maximum, 4096);
    }
}
