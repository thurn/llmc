use std::path::{Path, PathBuf};
use std::{env, fs};

use anyhow::{Context, Result};
use serde::Deserialize;

/// User-configurable settings loaded from config.toml.
///
/// Settings are loaded from (in order of precedence):
/// 1. CLI flags (highest priority)
/// 2. .llmc/config.toml in the repository
/// 3. ~/.config/llmc/config.toml (or $XDG_CONFIG_HOME/llmc/config.toml)
/// 4. Built-in defaults (lowest priority)
#[derive(Debug, Clone, Default)]
pub struct Settings {
    /// Default target path for `llmc setup` when none is provided.
    pub default_target: Option<PathBuf>,

    /// Commands to run for validation (e.g., ["just fmt", "just check"]).
    pub validation_commands: Option<Vec<String>>,

    /// Custom preamble template for prompts. If None, uses the built-in default.
    pub preamble_template: Option<String>,

    /// Whether to strip attribution lines from prompts (default: true).
    pub strip_attribution: bool,
}

/// Raw config file structure matching config.toml format.
#[derive(Debug, Deserialize, Default)]
struct ConfigFile {
    #[serde(default)]
    setup: SetupConfig,
    #[serde(default)]
    validation: ValidationConfig,
    #[serde(default)]
    prompts: PromptsConfig,
}

#[derive(Debug, Deserialize, Default)]
struct SetupConfig {
    default_target: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
struct ValidationConfig {
    commands: Option<Vec<String>>,
}

#[derive(Debug, Deserialize, Default)]
struct PromptsConfig {
    strip_attribution: Option<bool>,
    preamble_template: Option<String>,
}

impl Settings {
    /// Load settings from config files, with CLI overrides applied.
    ///
    /// Config file locations (checked in order, first found wins):
    /// 1. .llmc/config.toml in the provided repo root
    /// 2. $XDG_CONFIG_HOME/llmc/config.toml (or ~/.config/llmc/config.toml)
    pub fn load(repo_root: Option<&Path>) -> Result<Self> {
        let config_file = Self::find_config_file(repo_root);
        let mut settings = Self::default();

        if let Some(path) = config_file {
            let config = Self::parse_config_file(&path)?;
            settings.apply_config_file(config);
        }

        // Default strip_attribution is set in apply_config_file

        Ok(settings)
    }

    /// Find the config file path, checking repo-local then global locations.
    fn find_config_file(repo_root: Option<&Path>) -> Option<PathBuf> {
        // First check repo-local config
        if let Some(root) = repo_root {
            let local_config = root.join(".llmc").join("config.toml");
            if local_config.exists() {
                return Some(local_config);
            }
        }

        // Then check XDG config location
        let xdg_config = Self::xdg_config_path();
        if xdg_config.exists() {
            return Some(xdg_config);
        }

        None
    }

    /// Get the XDG config file path.
    fn xdg_config_path() -> PathBuf {
        let config_home = env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                env::var_os("HOME")
                    .map(|h| PathBuf::from(h).join(".config"))
                    .unwrap_or_else(|| PathBuf::from(".config"))
            });

        config_home.join("llmc").join("config.toml")
    }

    /// Parse a config file from the given path.
    fn parse_config_file(path: &Path) -> Result<ConfigFile> {
        let contents = fs::read_to_string(path)
            .with_context(|| format!("Failed to read config file {path:?}"))?;

        toml::from_str(&contents).with_context(|| format!("Failed to parse config file {path:?}"))
    }

    /// Apply values from a parsed config file.
    fn apply_config_file(&mut self, config: ConfigFile) {
        if let Some(target) = config.setup.default_target {
            // Expand ~ to home directory
            let expanded = if let Some(stripped) = target.strip_prefix("~/") {
                if let Some(home) = env::var_os("HOME") {
                    PathBuf::from(home).join(stripped)
                } else {
                    PathBuf::from(target)
                }
            } else {
                PathBuf::from(target)
            };
            self.default_target = Some(expanded);
        }

        if let Some(commands) = config.validation.commands {
            self.validation_commands = Some(commands);
        }

        if let Some(strip) = config.prompts.strip_attribution {
            self.strip_attribution = strip;
        } else {
            // Default to true if not specified
            self.strip_attribution = true;
        }

        if let Some(template) = config.prompts.preamble_template {
            self.preamble_template = Some(template);
        }
    }

    /// Apply CLI overrides to settings.
    #[allow(dead_code)]
    pub fn with_default_target(mut self, target: Option<PathBuf>) -> Self {
        if target.is_some() {
            self.default_target = target;
        }
        self
    }

    /// Get the default target path, falling back to ~/Documents/llmc.
    pub fn default_target_path(&self) -> Result<PathBuf> {
        if let Some(ref target) = self.default_target {
            return Ok(target.clone());
        }

        let Some(home) = env::var_os("HOME") else {
            return Err(anyhow::anyhow!("HOME is not set"));
        };

        Ok(PathBuf::from(home).join("Documents").join("llmc"))
    }

    /// Get the validation commands, falling back to defaults.
    pub fn validation_commands(&self) -> Vec<String> {
        self.validation_commands.clone().unwrap_or_else(|| {
            vec![
                "just fmt".to_string(),
                "just check".to_string(),
                "just clippy".to_string(),
                "just review".to_string(),
            ]
        })
    }

    /// Format validation commands for display in prompts.
    pub fn validation_commands_display(&self) -> String {
        self.validation_commands().join(", ")
    }

    /// Get the preamble template, or None to use the default.
    pub fn preamble_template(&self) -> Option<&str> {
        self.preamble_template.as_deref()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_settings() {
        let settings = Settings::default();
        assert!(settings.default_target.is_none());
        assert!(settings.validation_commands.is_none());
        assert!(!settings.strip_attribution); // Default struct value
    }

    #[test]
    fn test_validation_commands_default() {
        let settings = Settings {
            strip_attribution: true,
            ..Default::default()
        };
        let commands = settings.validation_commands();
        assert_eq!(commands.len(), 4);
        assert!(commands.contains(&"just fmt".to_string()));
    }
}
