//! Where the app keeps its files, following each operating system's convention.
//!
//! | Directory | macOS | Windows |
//! |---|---|---|
//! | `config` | `~/Library/Application Support/dev.noteeverywhere.app` | `%APPDATA%\noteeverywhere\app\config` |
//! | `data` | `~/Library/Application Support/dev.noteeverywhere.app` | `%LOCALAPPDATA%\noteeverywhere\app\data` |
//! | `cache` | `~/Library/Caches/dev.noteeverywhere.app` | `%LOCALAPPDATA%\noteeverywhere\app\cache` |
//! | `models` | `<data>/models` | `<data>\models` |
//!
//! `data` uses the machine-local location on Windows because it holds the
//! local database; roaming profiles must not copy it between computers. On
//! macOS the folder name equals the desktop app's bundle identifier.
//!
//! Nothing is created here; callers create directories when they first write.

use std::path::{Path, PathBuf};

use directories::ProjectDirs;

const QUALIFIER: &str = "dev";
const ORGANIZATION: &str = "noteeverywhere";
const APPLICATION: &str = "app";

/// The home directory (or the Windows known folders) could not be determined.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("cannot determine the user's home directory")]
pub struct NoHomeDirectory;

/// The app's directories.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppDirs {
    config: PathBuf,
    data: PathBuf,
    cache: PathBuf,
    models: PathBuf,
}

impl AppDirs {
    /// The directories for the current user and operating system.
    pub fn discover() -> Result<Self, NoHomeDirectory> {
        let dirs =
            ProjectDirs::from(QUALIFIER, ORGANIZATION, APPLICATION).ok_or(NoHomeDirectory)?;
        Ok(Self::from_roots(
            dirs.config_dir().to_owned(),
            dirs.data_local_dir().to_owned(),
            dirs.cache_dir().to_owned(),
        ))
    }

    /// Builds the set from explicit roots (for tests and portable installs).
    /// Models always live under the data directory.
    pub fn from_roots(config: PathBuf, data: PathBuf, cache: PathBuf) -> Self {
        let models = data.join("models");
        Self {
            config,
            data,
            cache,
            models,
        }
    }

    /// User settings.
    pub fn config(&self) -> &Path {
        &self.config
    }

    /// Notes database, attachments and other durable local data.
    pub fn data(&self) -> &Path {
        &self.data
    }

    /// Files that can be rebuilt (thumbnails, indexes).
    pub fn cache(&self) -> &Path {
        &self.cache
    }

    /// Downloaded speech and language models: large, durable, not user data.
    pub fn models(&self) -> &Path {
        &self.models
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn models_live_under_data() {
        let dirs = AppDirs::from_roots("/c".into(), "/d".into(), "/k".into());
        assert_eq!(dirs.config(), Path::new("/c"));
        assert_eq!(dirs.data(), Path::new("/d"));
        assert_eq!(dirs.cache(), Path::new("/k"));
        assert_eq!(dirs.models(), Path::new("/d/models"));
    }

    #[test]
    fn discovered_paths_follow_the_os_convention() {
        let dirs = AppDirs::discover().expect("home directory");
        for path in [dirs.config(), dirs.data(), dirs.cache(), dirs.models()] {
            assert!(path.is_absolute(), "{path:?}");
        }
        assert_ne!(dirs.cache(), dirs.data());
        assert_eq!(
            dirs.models().file_name().and_then(|n| n.to_str()),
            Some("models")
        );
        #[cfg(target_os = "macos")]
        {
            assert!(
                dirs.data()
                    .ends_with("Library/Application Support/dev.noteeverywhere.app")
            );
            assert!(
                dirs.config()
                    .ends_with("Library/Application Support/dev.noteeverywhere.app")
            );
            assert!(
                dirs.cache()
                    .ends_with("Library/Caches/dev.noteeverywhere.app")
            );
        }
        #[cfg(target_os = "windows")]
        {
            assert!(dirs.data().ends_with(r"noteeverywhere\app\data"));
            assert!(dirs.config().ends_with(r"noteeverywhere\app\config"));
            assert!(dirs.cache().ends_with(r"noteeverywhere\app\cache"));
        }
    }
}
