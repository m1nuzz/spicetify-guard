use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

#[derive(Default, Serialize, Deserialize)]
pub struct Cache {
    pub last_ok: Option<std::time::SystemTime>,
    pub spotify_ver: Option<String>,
    pub backup_ver: Option<String>,
}

pub fn get_versions_from_config() -> (Option<String>, Option<String>) {
    let appdata = std::env::var("APPDATA").unwrap_or_default();
    let config_path = Path::new(&appdata).join("spicetify").join("config-xpui.ini");
    if let Ok(content) = fs::read_to_string(config_path) {
        get_versions_from_config_from_string(&content)
    } else {
        (None, None)
    }
}

pub fn get_versions_from_config_from_string(content: &str) -> (Option<String>, Option<String>) {
    let mut spotify_version = None;
    let mut backup_version = None;
    let mut in_backup_section = false;

    for line in content.lines() {
        if line.trim() == "[Backup]" {
            in_backup_section = true;
            continue;
        }

        if in_backup_section {
            if line.trim().starts_with("version") {
                let parts: Vec<&str> = line.split('=').collect();
                if parts.len() == 2 {
                    spotify_version = Some(parts[1].trim().to_string());
                }
            } else if line.trim().starts_with("with") {
                let parts: Vec<&str> = line.split('=').collect();
                if parts.len() == 2 {
                    backup_version = Some(parts[1].trim().to_string());
                }
            } else if line.trim().starts_with('[') {
                break;
            }
        }
    }

    (spotify_version, backup_version)
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    proptest! {
        #[test]
        fn test_get_versions_from_config_prop(
            version in prop::option::of("[0-9]{1,2}\\.[0-9]{1,2}(\\.[0-9]{1,3}){0,2}"),
            with in prop::option::of("[0-9]{1,2}\\.[0-9]{1,2}(\\.[0-9]{1,3}){0,2}")
        ) {
            let mut content = String::from("[Backup]\n");
            if let Some(v) = &version {
                content.push_str(&format!("version = {}\n", v));
            }
            if let Some(w) = &with {
                content.push_str(&format!("with = {}\n", w));
            }
            let (pv, pw) = get_versions_from_config_from_string(&content);
            assert_eq!(pv, version);
            assert_eq!(pw, with);
        }
    }
}