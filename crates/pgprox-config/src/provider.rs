//! Where the configuration comes from.
//!
//! # The symlink
//!
//! A `ConfigMap` mounted into a pod is not a file. Kubernetes writes each
//! version into a hidden timestamped directory and points a symlink at it, then
//! swaps the symlink atomically when the data changes. The path an operator
//! writes in the pod spec is a symlink to a symlink to the real file.
//!
//! So a watcher registered on the file watches an inode that is never modified
//! and never will be. It works perfectly in a test that writes to the file in
//! place, and never fires once in the cluster. That is the bug this module
//! exists to not have, and it is why the provider re-resolves the path from the
//! directory every time rather than holding on to anything.
//!
//! # Polled, not evented
//!
//! Re-reading the directory is what makes the symlink case correct by
//! construction: there is no registration to point at the wrong inode.
//!
//! It also costs nothing worth measuring. Kubelet propagates a `ConfigMap`
//! change on its own sync period, which is tens of seconds by default, so a
//! change has already been late by a minute before this could possibly see it.
//! Reacting to it in microseconds rather than a second buys nothing, and the
//! read is one `stat` and, only when something moved, one small file.
//!
//! # What "changed" means
//!
//! The content, not the timestamp. A `ConfigMap` update rewrites the file even
//! when the data is identical, and republishing an unchanged configuration
//! would wake every watcher in the process for nothing. Comparing what was read
//! against what is held is cheap and exact.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use pgprox_core::config::{Config, ConfigError, ConfigSource};
use tokio::sync::watch;

use crate::document;

/// How the file provider is tuned.
#[derive(Clone, Debug)]
pub struct FileConfig {
    /// The directory the configuration is mounted in.
    pub directory: PathBuf,
    /// The file within it.
    pub file_name: String,
    /// How often to look.
    ///
    /// A second, against a kubelet sync period measured in tens of seconds. See
    /// the module docs for why this is not an event watcher.
    pub poll_interval: Duration,
}

impl FileConfig {
    /// A provider reading `path`, splitting it into the directory to watch and
    /// the file within it.
    ///
    /// Splitting rather than storing the path whole is the point: the directory
    /// is what gets re-read, so the symlink is resolved afresh every time.
    #[must_use]
    pub fn at(path: impl AsRef<Path>) -> Self {
        let path = path.as_ref();
        // `Path::parent` of a bare file name is `Some("")`, not `None`, and an
        // empty directory joins to a relative path that reads differently from
        // the one the caller wrote.
        let directory = match path.parent() {
            Some(parent) if !parent.as_os_str().is_empty() => parent.to_path_buf(),
            _ => PathBuf::from("."),
        };
        Self {
            directory,
            file_name: path.file_name().map_or_else(
                || "pgprox.yaml".to_owned(),
                |name| name.to_string_lossy().into_owned(),
            ),
            poll_interval: Duration::from_secs(1),
        }
    }

    /// The path to read, resolved from the directory each time.
    #[must_use]
    pub fn path(&self) -> PathBuf {
        self.directory.join(&self.file_name)
    }
}

/// A [`ConfigSource`] reading a file from a mounted directory.
#[derive(Debug)]
pub struct FileSource {
    config: FileConfig,
    tx: watch::Sender<Arc<Config>>,
}

impl FileSource {
    /// Reads the file once and serves what it found.
    ///
    /// Fails if the initial read fails. A node that cannot read its
    /// configuration at startup should not start: it has no last-good value to
    /// fall back to, and starting with defaults would silently ignore every
    /// limit the operator set.
    ///
    /// # Errors
    ///
    /// [`ConfigError::Unreadable`] if the file is missing or unreadable, or
    /// [`ConfigError::Invalid`] if it does not parse or does not validate.
    pub fn new(config: FileConfig) -> Result<Arc<Self>, ConfigError> {
        let initial = read(&config.path())?;
        let (tx, _) = watch::channel(Arc::new(initial));
        Ok(Arc::new(Self { config, tx }))
    }

    /// How this provider is configured.
    #[must_use]
    pub const fn settings(&self) -> &FileConfig {
        &self.config
    }

    /// Re-reads the directory and publishes the result if it changed.
    ///
    /// Returns whether anything was published. This is one tick of the poll
    /// loop, exposed so the behaviour is testable without waiting for a timer.
    ///
    /// # Errors
    ///
    /// Whatever the read or the parse produced. The previous configuration
    /// stays published either way: a broken edit must not take a running node
    /// down. See `M4.4`.
    pub fn poll(&self) -> Result<bool, ConfigError> {
        let next = read(&self.config.path())?;
        Ok(self.publish_if_changed(next))
    }

    /// Publishes only if the content differs from what watchers hold.
    ///
    /// A `ConfigMap` update rewrites the file even when the data is identical,
    /// so comparing content rather than timestamps is what stops every watcher
    /// in the process waking for nothing.
    fn publish_if_changed(&self, next: Config) -> bool {
        if *self.tx.borrow().as_ref() == next {
            return false;
        }
        self.tx.send_replace(Arc::new(next));
        true
    }
}

#[async_trait::async_trait]
impl ConfigSource for FileSource {
    async fn load(&self) -> Result<Config, ConfigError> {
        read(&self.config.path())
    }

    fn watch(&self) -> watch::Receiver<Arc<Config>> {
        self.tx.subscribe()
    }
}

/// Reads and parses one file.
///
/// The path is resolved by the operating system on every call, which is what
/// follows a swapped symlink to the new target.
fn read(path: &Path) -> Result<Config, ConfigError> {
    let text = std::fs::read_to_string(path).map_err(|err| ConfigError::Unreadable {
        // The path is in the message because "no such file" without one is a
        // support ticket rather than a fix.
        reason: format!("{}: {err}", path.display()),
    })?;
    document::parse(&text)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    const MINIMAL: &str = "max_client_conns: 100\n";

    /// A directory with a config file in it.
    fn mounted(text: &str) -> (TempDir, FileConfig) {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("pgprox.yaml");
        fs::write(&path, text).unwrap();
        let config = FileConfig::at(&path);
        (dir, config)
    }

    /// Lays out a directory the way Kubernetes lays out a `ConfigMap`:
    /// the data in a timestamped directory, `..data` pointing at it, and the
    /// visible file pointing through that.
    fn mounted_as_configmap(dir: &Path, version: &str, text: &str) {
        let data = dir.join(format!("..{version}"));
        fs::create_dir_all(&data).unwrap();
        fs::write(data.join("pgprox.yaml"), text).unwrap();

        // The atomic swap: build the new link beside the old one and rename
        // over it, which is exactly what kubelet does.
        let staging = dir.join("..data_tmp");
        let _ = fs::remove_file(&staging);
        std::os::unix::fs::symlink(&data, &staging).unwrap();
        fs::rename(&staging, dir.join("..data")).unwrap();

        let visible = dir.join("pgprox.yaml");
        if !visible.exists() {
            std::os::unix::fs::symlink(Path::new("..data").join("pgprox.yaml"), &visible).unwrap();
        }
    }

    #[tokio::test]
    async fn a_provider_serves_what_the_file_says() {
        let (_dir, config) = mounted(MINIMAL);
        let source = FileSource::new(config).unwrap();

        assert_eq!(source.watch().borrow().max_client_conns, 100);
        assert_eq!(source.load().await.unwrap().max_client_conns, 100);
    }

    #[test]
    fn a_missing_file_stops_the_node_starting() {
        // There is no last-good value to fall back to, and starting with
        // defaults would silently ignore every limit the operator set.
        let dir = TempDir::new().unwrap();
        let config = FileConfig::at(dir.path().join("absent.yaml"));

        let err = FileSource::new(config).unwrap_err();
        assert!(matches!(err, ConfigError::Unreadable { .. }), "{err:?}");
        assert!(
            err.to_string().contains("absent.yaml"),
            "the path belongs in the message, got {err}"
        );
    }

    #[test]
    fn an_invalid_file_stops_the_node_starting() {
        let (_dir, config) = mounted("max_client_conns: 0\n");
        let err = FileSource::new(config).unwrap_err();
        assert!(matches!(err, ConfigError::Invalid { .. }), "{err:?}");
    }

    #[tokio::test]
    async fn an_edit_in_place_is_picked_up() {
        let (dir, config) = mounted(MINIMAL);
        let source = FileSource::new(config).unwrap();
        let mut rx = source.watch();
        rx.borrow_and_update();

        fs::write(dir.path().join("pgprox.yaml"), "max_client_conns: 250\n").unwrap();
        assert!(source.poll().unwrap(), "nothing was published");

        rx.changed().await.unwrap();
        assert_eq!(rx.borrow().max_client_conns, 250);
    }

    #[tokio::test]
    async fn a_configmap_symlink_swap_is_picked_up() {
        // The case this module exists for. A watcher registered on the file
        // watches an inode that is never modified: kubelet writes a new
        // directory and swaps a symlink, so the old inode stays exactly as it
        // was forever. This test fails against any implementation that resolves
        // the path once and holds on to it.
        let dir = TempDir::new().unwrap();
        mounted_as_configmap(dir.path(), "2026_01_01", MINIMAL);

        let source = FileSource::new(FileConfig::at(dir.path().join("pgprox.yaml"))).unwrap();
        let mut rx = source.watch();
        rx.borrow_and_update();
        assert_eq!(rx.borrow().max_client_conns, 100);

        // A new version arrives: new directory, symlink swapped over it. The
        // file the provider was originally shown is untouched.
        mounted_as_configmap(dir.path(), "2026_01_02", "max_client_conns: 999\n");

        assert!(
            source.poll().unwrap(),
            "a symlink swap went unnoticed, which is the ConfigMap bug"
        );
        rx.changed().await.unwrap();
        assert_eq!(rx.borrow().max_client_conns, 999);
    }

    #[tokio::test]
    async fn the_old_version_is_genuinely_untouched_by_the_swap() {
        // Proves the test above is testing what it claims: the original data
        // file still holds the original content, so anything that noticed the
        // change must have re-resolved the path.
        let dir = TempDir::new().unwrap();
        mounted_as_configmap(dir.path(), "2026_01_01", MINIMAL);
        let first = dir.path().join("..2026_01_01").join("pgprox.yaml");
        let before = fs::read_to_string(&first).unwrap();

        mounted_as_configmap(dir.path(), "2026_01_02", "max_client_conns: 999\n");

        assert_eq!(
            fs::read_to_string(&first).unwrap(),
            before,
            "the swap modified the old file, so this fixture is not a ConfigMap"
        );
        assert_eq!(
            fs::read_to_string(dir.path().join("pgprox.yaml")).unwrap(),
            "max_client_conns: 999\n",
            "the visible path did not follow the swap"
        );
    }

    #[test]
    fn an_unchanged_file_publishes_nothing() {
        // A ConfigMap update rewrites the file even when the data is identical,
        // and republishing would wake every watcher in the process for nothing.
        let (dir, config) = mounted(MINIMAL);
        let source = FileSource::new(config).unwrap();

        assert!(!source.poll().unwrap(), "an unchanged file was republished");

        // Rewritten with the same content, as kubelet does.
        fs::write(dir.path().join("pgprox.yaml"), MINIMAL).unwrap();
        assert!(!source.poll().unwrap(), "identical content was republished");
    }

    #[test]
    fn a_provider_splits_the_path_into_a_directory_and_a_file() {
        // Storing the path whole would be the same bug as watching the file.
        let config = FileConfig::at("/etc/pgprox/pgprox.yaml");
        assert_eq!(config.directory, Path::new("/etc/pgprox"));
        assert_eq!(config.file_name, "pgprox.yaml");
        assert_eq!(config.path(), Path::new("/etc/pgprox/pgprox.yaml"));
    }

    #[test]
    fn a_bare_file_name_reads_from_the_current_directory() {
        let config = FileConfig::at("pgprox.yaml");
        assert_eq!(config.directory, Path::new("."));
        assert_eq!(config.path(), Path::new("./pgprox.yaml"));
    }

    #[test]
    fn a_path_with_no_file_name_falls_back_to_a_default() {
        // `FileConfig::at("/")` is a mistake, and reporting it as a missing
        // file at a nameable path beats panicking on an operator's typo.
        let config = FileConfig::at("/");
        assert_eq!(config.file_name, "pgprox.yaml");
    }

    #[tokio::test]
    async fn the_provider_works_through_an_arc_dyn() {
        let (_dir, config) = mounted(MINIMAL);
        let source: Arc<dyn ConfigSource> = FileSource::new(config).unwrap();

        assert_eq!(source.load().await.unwrap().max_client_conns, 100);
        assert_eq!(source.watch().borrow().max_client_conns, 100);
    }

    #[test]
    fn the_poll_interval_is_short_against_kubelet_but_not_a_busy_loop() {
        let config = FileConfig::at("/etc/pgprox/pgprox.yaml");
        assert!(config.poll_interval >= Duration::from_millis(100));
        assert!(config.poll_interval <= Duration::from_secs(5));
    }
}
