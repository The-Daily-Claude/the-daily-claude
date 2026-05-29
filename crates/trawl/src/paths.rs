use anyhow::{Result, anyhow};
use std::path::{Path, PathBuf};

const APP_DIR: &str = "com.the-daily-claude.trawl";
const STASH_DIR: &str = "the-stash";
const ENTRIES_DIR: &str = "entries";
const STATE_FILE: &str = "trawl-state.json";
const REGISTRY_FILE: &str = "pii-registry.json";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TargetOs {
    Linux,
    MacOs,
    Other,
}

impl TargetOs {
    fn current() -> Self {
        if cfg!(target_os = "macos") {
            Self::MacOs
        } else if cfg!(target_os = "linux") {
            Self::Linux
        } else {
            Self::Other
        }
    }
}

pub fn data_dir() -> Result<PathBuf> {
    let home = dirs::home_dir().ok_or_else(|| anyhow!("could not determine home directory"))?;
    let xdg_data_home = std::env::var_os("XDG_DATA_HOME").map(PathBuf::from);
    Ok(resolve_data_dir_for(
        TargetOs::current(),
        &home,
        xdg_data_home.as_deref(),
    ))
}

pub fn default_entries_dir() -> Result<PathBuf> {
    Ok(data_dir()?.join(STASH_DIR).join(ENTRIES_DIR))
}

pub fn default_state_path() -> Result<PathBuf> {
    Ok(data_dir()?.join(STATE_FILE))
}

pub fn default_registry_path() -> Result<PathBuf> {
    Ok(data_dir()?.join(REGISTRY_FILE))
}

pub fn resolve_data_dir_for(
    target_os: TargetOs,
    home_dir: &Path,
    xdg_data_home: Option<&Path>,
) -> PathBuf {
    let fallback = || home_dir.join(".local/share");
    let base = match target_os {
        TargetOs::MacOs => fallback(),
        TargetOs::Linux => match xdg_data_home {
            Some(p) if !p.as_os_str().is_empty() && p.is_absolute() => p.to_path_buf(),
            _ => fallback(),
        },
        TargetOs::Other => fallback(),
    };
    base.join(APP_DIR)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn linux_uses_absolute_xdg_data_home() {
        let home = Path::new("/home/alice");
        let xdg = Path::new("/tmp/xdg-data");
        assert_eq!(
            resolve_data_dir_for(TargetOs::Linux, home, Some(xdg)),
            PathBuf::from("/tmp/xdg-data/com.the-daily-claude.trawl")
        );
    }

    #[test]
    fn linux_falls_back_to_local_share_without_xdg() {
        let home = Path::new("/home/alice");
        assert_eq!(
            resolve_data_dir_for(TargetOs::Linux, home, None),
            PathBuf::from("/home/alice/.local/share/com.the-daily-claude.trawl")
        );
    }

    #[test]
    fn linux_ignores_empty_xdg_data_home() {
        let home = Path::new("/home/alice");
        assert_eq!(
            resolve_data_dir_for(TargetOs::Linux, home, Some(Path::new(""))),
            PathBuf::from("/home/alice/.local/share/com.the-daily-claude.trawl")
        );
    }

    #[test]
    fn linux_ignores_relative_xdg_data_home() {
        let home = Path::new("/home/alice");
        assert_eq!(
            resolve_data_dir_for(TargetOs::Linux, home, Some(Path::new("relative/path"))),
            PathBuf::from("/home/alice/.local/share/com.the-daily-claude.trawl")
        );
    }

    #[test]
    fn macos_always_uses_literal_local_share() {
        let home = Path::new("/Users/alice");
        let xdg = Path::new("/tmp/xdg-data");
        assert_eq!(
            resolve_data_dir_for(TargetOs::MacOs, home, Some(xdg)),
            PathBuf::from("/Users/alice/.local/share/com.the-daily-claude.trawl")
        );
    }

    #[test]
    fn derived_paths_use_target_filenames_without_dot_prefix() {
        let home = Path::new("/home/alice");
        let data_dir = resolve_data_dir_for(TargetOs::Linux, home, None);
        assert_eq!(
            data_dir.join(STASH_DIR).join(ENTRIES_DIR),
            PathBuf::from("/home/alice/.local/share/com.the-daily-claude.trawl/the-stash/entries")
        );
        assert_eq!(
            data_dir.join(STATE_FILE),
            PathBuf::from("/home/alice/.local/share/com.the-daily-claude.trawl/trawl-state.json")
        );
        assert_eq!(
            data_dir.join(REGISTRY_FILE),
            PathBuf::from("/home/alice/.local/share/com.the-daily-claude.trawl/pii-registry.json")
        );
    }
}
