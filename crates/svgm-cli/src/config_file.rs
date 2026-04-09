use std::collections::HashMap;
use std::error::Error;
use std::path::{Path, PathBuf};
use std::{fmt, fs};

use svgm_core::{Config, Preset};

const CONFIG_FILENAME: &str = "svgm.config.toml";

#[derive(Debug)]
pub struct ConfigError(String);

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl Error for ConfigError {}

#[derive(serde::Deserialize, Default)]
struct RawConfig {
    preset: Option<String>,
    precision: Option<u32>,
    #[serde(default)]
    passes: HashMap<String, bool>,
}

/// Find a config file. If `explicit` is given, return it directly.
/// Otherwise walk up from `start_dir` looking for `svgm.config.toml`.
pub fn find_config(explicit: Option<&Path>, start_dir: &Path) -> Option<PathBuf> {
    if let Some(p) = explicit {
        return Some(p.to_path_buf());
    }

    let mut dir = start_dir.to_path_buf();
    loop {
        let candidate = dir.join(CONFIG_FILENAME);
        if candidate.is_file() {
            return Some(candidate);
        }
        if !dir.pop() {
            return None;
        }
    }
}

/// Load and validate a config file, returning a core `Config`.
pub fn load_config(path: &Path) -> Result<Config, Box<dyn Error>> {
    let contents = fs::read_to_string(path)?;
    let raw: RawConfig = toml::from_str(&contents)?;

    let preset = match raw.preset.as_deref() {
        Some("safe") => Preset::Safe,
        Some("default") | Some("balanced") | Some("aggressive") => Preset::Default,
        Some(other) => {
            return Err(Box::new(ConfigError(format!(
                "unknown preset \"{other}\" — expected safe or default"
            ))));
        }
        None => Preset::default(),
    };

    let known = svgm_core::config::all_pass_names();
    for name in raw.passes.keys() {
        if !known.contains(&name.as_str()) {
            return Err(Box::new(ConfigError(format!("unknown pass \"{name}\""))));
        }
    }

    Ok(Config {
        preset,
        precision: raw.precision,
        pass_overrides: raw.passes,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn parse_full_config() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(CONFIG_FILENAME);
        fs::write(
            &path,
            r#"
preset = "safe"
precision = 2

[passes]
removeDesc = true
removeComments = false
"#,
        )
        .unwrap();

        let config = load_config(&path).unwrap();
        assert_eq!(config.preset, Preset::Safe);
        assert_eq!(config.precision, Some(2));
        assert_eq!(config.pass_overrides.get("removeDesc"), Some(&true));
        assert_eq!(config.pass_overrides.get("removeComments"), Some(&false));
    }

    #[test]
    fn parse_minimal_config() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(CONFIG_FILENAME);
        fs::write(&path, "").unwrap();

        let config = load_config(&path).unwrap();
        assert_eq!(config.preset, Preset::Default);
        assert_eq!(config.precision, None);
        assert!(config.pass_overrides.is_empty());
    }

    #[test]
    fn unknown_preset_errors() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(CONFIG_FILENAME);
        fs::write(&path, "preset = \"turbo\"").unwrap();

        let err = load_config(&path).unwrap_err();
        assert!(err.to_string().contains("unknown preset"));
    }

    #[test]
    fn unknown_pass_errors() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(CONFIG_FILENAME);
        fs::write(&path, "[passes]\nfakeName = true").unwrap();

        let err = load_config(&path).unwrap_err();
        assert!(err.to_string().contains("unknown pass"));
    }

    #[test]
    fn find_config_walks_up() {
        let dir = tempfile::tempdir().unwrap();
        let nested = dir.path().join("a").join("b").join("c");
        fs::create_dir_all(&nested).unwrap();
        let config_path = dir.path().join(CONFIG_FILENAME);
        fs::write(&config_path, "preset = \"safe\"").unwrap();

        let found = find_config(None, &nested).unwrap();
        assert_eq!(found, config_path);
    }

    #[test]
    fn find_config_explicit_path() {
        let p = Path::new("/tmp/custom.toml");
        assert_eq!(
            find_config(Some(p), Path::new("/somewhere")),
            Some(p.to_path_buf())
        );
    }

    #[test]
    fn find_config_returns_none_when_missing() {
        let dir = tempfile::tempdir().unwrap();
        assert!(find_config(None, dir.path()).is_none());
    }
}
