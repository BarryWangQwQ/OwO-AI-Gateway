use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LayoutMode {
    /// `~/.owo/config.toml` with state under `~/.owo/` (or `$OWO_HOME`).
    Home,
    /// `config.toml` next to the executable with state under `./data/`.
    Portable,
}

/// Resolved on-disk layout (spec §6).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OwoPaths {
    pub mode: LayoutMode,
    pub root: PathBuf,
    pub config: PathBuf,
    pub state: PathBuf,
    pub backups: PathBuf,
    pub logs: PathBuf,
}

impl OwoPaths {
    pub fn home() -> Option<Self> {
        let root = match std::env::var_os("OWO_HOME") {
            Some(dir) if !dir.is_empty() => PathBuf::from(dir),
            _ => user_home()?.join(".owo"),
        };
        Some(Self::from_home_root(root))
    }

    pub fn from_home_root(root: PathBuf) -> Self {
        Self {
            mode: LayoutMode::Home,
            config: root.join("config.toml"),
            state: root.join("state"),
            backups: root.join("backups"),
            logs: root.join("logs"),
            root,
        }
    }

    pub fn portable(dir: &Path) -> Self {
        let data = dir.join("data");
        Self {
            mode: LayoutMode::Portable,
            root: dir.to_path_buf(),
            config: dir.join("config.toml"),
            state: data.join("state"),
            backups: data.join("backups"),
            logs: data.join("logs"),
        }
    }

    /// Portable layout rooted at the directory of the running executable.
    pub fn portable_from_exe() -> std::io::Result<Self> {
        let exe = std::env::current_exe()?;
        let dir = exe.parent().ok_or_else(|| std::io::Error::other("executable has no parent directory"))?;
        Ok(Self::portable(dir))
    }

    /// Overrides the config file location while keeping state next to it.
    pub fn with_config_file(mut self, config: PathBuf) -> Self {
        self.config = config;
        self
    }

    pub fn ensure_dirs(&self) -> std::io::Result<()> {
        for dir in [&self.state, &self.backups, &self.logs] {
            std::fs::create_dir_all(dir)?;
        }
        Ok(())
    }
}

fn user_home() -> Option<PathBuf> {
    dirs::home_dir()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn portable_layout() {
        let p = OwoPaths::portable(Path::new("/opt/owo"));
        assert_eq!(p.config, Path::new("/opt/owo/config.toml"));
        assert_eq!(p.state, Path::new("/opt/owo/data/state"));
        assert_eq!(p.backups, Path::new("/opt/owo/data/backups"));
    }

    #[test]
    fn home_layout() {
        let p = OwoPaths::from_home_root(PathBuf::from("/home/u/.owo"));
        assert_eq!(p.config, Path::new("/home/u/.owo/config.toml"));
        assert_eq!(p.logs, Path::new("/home/u/.owo/logs"));
    }
}
