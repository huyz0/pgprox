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
//! # A broken edit does not take the node down
//!
//! Validate, then swap. A document that does not parse, or parses into a
//! configuration that does not validate, leaves watchers exactly where they
//! were and is reported.
//!
//! The alternative is worse than it sounds. A running node has clients on it,
//! and a typo in a `ConfigMap` is a routine event; taking the node down for one
//! would turn every config edit into a deploy. Serving nothing is not an option
//! either, because "no limits configured" reads to the rest of the process as
//! "defaults", which silently discards every cap the operator set.
//!
//! So the last good configuration keeps serving, the error is surfaced through
//! [`FileSource::last_error`], and the node keeps working while somebody fixes
//! the file.
//!
//! # What "changed" means
//!
//! The content, not the timestamp. A `ConfigMap` update rewrites the file even
//! when the data is identical, and republishing an unchanged configuration
//! would wake every watcher in the process for nothing. Comparing what was read
//! against what is held is cheap and exact.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};
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
    /// The last poll that failed, cleared by the next that succeeds.
    ///
    /// A node serving a stale configuration and a node serving a current one
    /// look identical from outside, which is exactly when an operator needs to
    /// be told which they have.
    last_error: Mutex<Option<ConfigError>>,
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
        Ok(Arc::new(Self {
            config,
            tx,
            last_error: Mutex::new(None),
        }))
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
    /// The read runs on Tokio's blocking pool, not inline on the calling task.
    /// `Self::run` calls this once a second for the life of the process, on
    /// the same runtime that also serves client connections; a synchronous
    /// `std::fs::read_to_string` here would stall every other task on that
    /// worker thread for the duration of the read. See
    /// `docs/internal/standards/async-concurrency.md`'s Blocking rule.
    /// `FileSource::new` and `ConfigSource::load` do the same read inline
    /// deliberately: both run once, before the node has bound a listener or
    /// taken a connection, which is the accepted exception the same rule
    /// describes.
    ///
    /// # Errors
    ///
    /// Whatever the read or the parse produced. Watchers keep the last good
    /// configuration either way: a typo in a `ConfigMap` is routine, and taking a
    /// node with clients on it down for one would make every config edit a
    /// deploy.
    ///
    /// # Panics
    ///
    /// If the blocking task panics. `read` never panics on its own, so this
    /// would mean a bug elsewhere; propagating it is better than a poll loop
    /// that silently stops updating.
    pub async fn poll(&self) -> Result<bool, ConfigError> {
        let path = self.config.path();
        let outcome = tokio::task::spawn_blocking(move || read(&path))
            .await
            .unwrap_or_else(|err| std::panic::resume_unwind(err.into_panic()));
        match outcome {
            Ok(next) => {
                self.set_error(None);
                Ok(self.publish_if_changed(next))
            }
            Err(err) => {
                self.set_error(Some(err.clone()));
                Err(err)
            }
        }
    }

    /// The last poll error, or [`None`] if the last poll succeeded.
    ///
    /// What `/readyz` and the admin API report. A node serving a stale
    /// configuration and one serving a current configuration look identical
    /// from outside, so this is how an operator tells them apart.
    #[must_use]
    pub fn last_error(&self) -> Option<ConfigError> {
        self.last_error
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    /// Whether the last poll succeeded.
    #[must_use]
    pub fn is_healthy(&self) -> bool {
        self.last_error().is_none()
    }

    fn set_error(&self, err: Option<ConfigError>) {
        *self
            .last_error
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = err;
    }

    /// Polls until cancelled, on the configured interval.
    ///
    /// A failing poll is logged by the caller through [`Self::last_error`] and
    /// the loop keeps going, because the file becoming readable again is the
    /// expected outcome and stopping would mean never noticing that it did.
    pub async fn run(self: Arc<Self>) {
        let mut ticker = tokio::time::interval(self.config.poll_interval);
        // The first tick fires immediately and the constructor has already
        // read the file, so skip it rather than doing the same work twice.
        ticker.tick().await;
        loop {
            ticker.tick().await;
            let _ = self.poll().await;
        }
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

    fn is_healthy(&self) -> bool {
        // The inherent method of the same name would resolve to this one, so
        // this asks the thing that answers it instead.
        self.last_error().is_none()
    }

    async fn run_loop(self: Arc<Self>) {
        // The poll loop below, reached through the trait so the composition
        // root can start it without knowing which source it holds.
        Self::run(self).await;
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
    use std::sync::atomic::{AtomicUsize, Ordering};
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

    /// `M14.42`. `is_healthy` survived being replaced by `true` *and* by
    /// `false`, which is only possible if nothing asks it. `M14.34` found the
    /// same of the `Arc` forwarding impl in `pgprox-core`, so between them the
    /// config staleness signal had no test anywhere: not on the trait default,
    /// not on the forwarding, and not on the one implementation that overrides
    /// it because it can actually go stale.
    ///
    /// What it is for, from its own doc: a node serving a stale document looks
    /// exactly like one serving the current one, which is when an operator most
    /// needs to be told which they have.
    #[tokio::test(start_paused = true)]
    async fn the_trait_loop_polls_the_file() {
        // `run_loop` could be replaced with `()`. It is the override that makes
        // the composition root able to start the watch without knowing which
        // source it holds, and its own doc records what happened when nothing
        // started it: a ConfigMap edit never reached a running node.
        //
        // A no-op body is indistinguishable from a running loop unless the test
        // waits for the loop to do something, which is what this does.
        let (dir, mut config) = mounted(MINIMAL);
        config.poll_interval = Duration::from_millis(10);
        let source = FileSource::new(config).unwrap();
        let mut watcher = ConfigSource::watch(&*source);

        let running = tokio::spawn(ConfigSource::run_loop(source.clone()));

        fs::write(dir.path().join("pgprox.yaml"), "max_client_conns: 250\n").unwrap();
        tokio::time::timeout(Duration::from_secs(5), watcher.changed())
            .await
            .expect("the trait loop never published a change, so it never polled")
            .unwrap();
        assert_eq!(watcher.borrow_and_update().max_client_conns, 250);

        running.abort();
    }

    #[tokio::test]
    async fn health_follows_the_last_poll_rather_than_a_constant() {
        let (dir, config) = mounted(MINIMAL);
        let source = FileSource::new(config).unwrap();

        // Called three ways on purpose, because they are three different
        // functions and the obvious one is not the one you would expect.
        //
        // `FileSource::new` returns `Arc<Self>`, and `pgprox-core` implements
        // `ConfigSource for Arc<T>`, so `source.is_healthy()` resolves to the
        // trait method on the `Arc` with no deref rather than to the inherent
        // method on `FileSource` with one. The first version of this test
        // asserted only the first two and both mutants of the inherent method
        // survived: it was never called.
        assert!(source.is_healthy(), "a good load left the source unhealthy");
        assert!(ConfigSource::is_healthy(&*source));
        assert!(FileSource::is_healthy(&source), "the inherent method");
        assert!(source.last_error().is_none());

        // Replace it with something unparseable and poll again.
        fs::write(
            dir.path().join("pgprox.yaml"),
            "max_client_conns: not-a-number\n",
        )
        .unwrap();
        assert!(source.poll().await.is_err());
        assert!(
            !source.is_healthy(),
            "a source that failed its last poll still reported itself healthy"
        );
        assert!(!ConfigSource::is_healthy(&*source));
        assert!(!FileSource::is_healthy(&source));
        assert!(source.last_error().is_some());

        // And it recovers, so health tracks the latest poll rather than
        // latching once it has been either value.
        fs::write(dir.path().join("pgprox.yaml"), MINIMAL).unwrap();
        assert!(source.poll().await.is_ok());
        assert!(source.is_healthy(), "a recovered source stayed unhealthy");
        assert!(FileSource::is_healthy(&source));
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
        assert!(source.poll().await.unwrap(), "nothing was published");

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
            source.poll().await.unwrap(),
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

    #[tokio::test]
    async fn an_unchanged_file_publishes_nothing() {
        // A ConfigMap update rewrites the file even when the data is identical,
        // and republishing would wake every watcher in the process for nothing.
        let (dir, config) = mounted(MINIMAL);
        let source = FileSource::new(config).unwrap();

        assert!(
            !source.poll().await.unwrap(),
            "an unchanged file was republished"
        );

        // Rewritten with the same content, as kubelet does.
        fs::write(dir.path().join("pgprox.yaml"), MINIMAL).unwrap();
        assert!(
            !source.poll().await.unwrap(),
            "identical content was republished"
        );
    }

    #[tokio::test]
    async fn a_broken_edit_leaves_the_last_good_configuration_serving() {
        // A typo in a ConfigMap is routine. Taking a node with clients on it
        // down for one would turn every config edit into a deploy, and serving
        // nothing is worse still: "no limits configured" reads to the rest of
        // the process as "defaults", silently discarding every cap the operator
        // set.
        let (dir, config) = mounted(MINIMAL);
        let source = FileSource::new(config).unwrap();
        let mut rx = source.watch();
        rx.borrow_and_update();
        assert!(source.is_healthy());

        fs::write(
            dir.path().join("pgprox.yaml"),
            "max_client_conns: [broken
",
        )
        .unwrap();
        let err = source.poll().await.unwrap_err();
        assert!(matches!(err, ConfigError::Invalid { .. }), "{err:?}");

        assert!(
            !rx.has_changed().unwrap(),
            "a broken document reached watchers"
        );
        assert_eq!(
            rx.borrow().max_client_conns,
            100,
            "the last good configuration stopped serving"
        );
    }

    #[tokio::test]
    async fn a_document_that_parses_but_does_not_validate_is_also_refused() {
        // Validation is part of the swap, not a separate step a caller might
        // forget, so a valid YAML document with a nonsense value is treated
        // exactly like a broken one.
        let (dir, config) = mounted(MINIMAL);
        let source = FileSource::new(config).unwrap();
        let mut rx = source.watch();
        rx.borrow_and_update();

        fs::write(
            dir.path().join("pgprox.yaml"),
            "max_client_conns: 0
",
        )
        .unwrap();
        assert!(source.poll().await.is_err());
        assert_eq!(rx.borrow().max_client_conns, 100);
    }

    #[tokio::test]
    async fn a_failing_poll_is_reported_and_a_later_good_one_clears_it() {
        // A node serving a stale configuration and one serving a current
        // configuration look identical from outside, so this is how an operator
        // tells them apart.
        let (dir, config) = mounted(MINIMAL);
        let source = FileSource::new(config).unwrap();
        assert!(source.last_error().is_none());

        fs::write(
            dir.path().join("pgprox.yaml"),
            "servers: [
",
        )
        .unwrap();
        assert!(source.poll().await.is_err());
        assert!(
            source.last_error().is_some(),
            "the failure was not reported"
        );
        assert!(!source.is_healthy());

        fs::write(
            dir.path().join("pgprox.yaml"),
            "max_client_conns: 300
",
        )
        .unwrap();
        assert!(source.poll().await.unwrap());
        assert!(
            source.is_healthy(),
            "a good poll did not clear the previous failure"
        );
    }

    #[tokio::test]
    async fn a_file_that_disappears_does_not_take_the_node_down() {
        // Mid-swap a ConfigMap directory can be momentarily inconsistent, and
        // a node that fell over for that would fall over on every update.
        let (dir, config) = mounted(MINIMAL);
        let source = FileSource::new(config).unwrap();

        fs::remove_file(dir.path().join("pgprox.yaml")).unwrap();
        let err = source.poll().await.unwrap_err();
        assert!(matches!(err, ConfigError::Unreadable { .. }), "{err:?}");
        assert_eq!(
            source.watch().borrow().max_client_conns,
            100,
            "a vanished file stopped the last good configuration serving"
        );

        // And it recovers when the swap completes.
        fs::write(
            dir.path().join("pgprox.yaml"),
            "max_client_conns: 400
",
        )
        .unwrap();
        assert!(source.poll().await.unwrap());
        assert_eq!(source.watch().borrow().max_client_conns, 400);
    }

    #[tokio::test(start_paused = true)]
    async fn the_poll_loop_keeps_going_after_a_failure() {
        // The file becoming readable again is the expected outcome, and
        // stopping would mean never noticing that it did.
        let (dir, config) = mounted(MINIMAL);
        let interval = config.poll_interval;
        let source = FileSource::new(config).unwrap();
        let mut rx = source.watch();
        rx.borrow_and_update();

        let running = tokio::spawn(Arc::clone(&source).run());

        fs::write(
            dir.path().join("pgprox.yaml"),
            "broken: [
",
        )
        .unwrap();
        tokio::time::sleep(interval * 2).await;
        assert!(!source.is_healthy(), "the loop did not see the bad file");

        fs::write(
            dir.path().join("pgprox.yaml"),
            "max_client_conns: 777
",
        )
        .unwrap();
        tokio::time::sleep(interval * 2).await;

        assert!(source.is_healthy(), "the loop stopped after a failure");
        assert_eq!(rx.borrow_and_update().max_client_conns, 777);
        running.abort();
    }

    /// `M88.8`. `poll` used to call `std::fs::read_to_string` straight from
    /// the calling task. On the single-threaded runtime this test uses, that
    /// means no other task can run until the read returns: the executor has
    /// exactly one worker thread, and a synchronous call never gives it back
    /// until it is done.
    ///
    /// `tokio::task::spawn_blocking` moves the read to a different thread and
    /// hands the caller a `JoinHandle`, which cannot resolve inside the same
    /// poll of the awaiting future — the executor has to suspend and come
    /// back for it, and in between it is free to run whatever else is ready.
    /// So a task spawned just before `poll().await` gets to make progress
    /// while the fix is in place, and gets none at all while it is not: this
    /// is not a timing measurement, it is presence or absence of a
    /// suspension point.
    #[tokio::test]
    async fn poll_yields_to_the_runtime_instead_of_blocking_it() {
        let (_dir, config) = mounted(MINIMAL);
        let source = FileSource::new(config).unwrap();

        let progressed = Arc::new(AtomicUsize::new(0));
        let counter = tokio::spawn({
            let progressed = Arc::clone(&progressed);
            async move {
                loop {
                    progressed.fetch_add(1, Ordering::Relaxed);
                    tokio::task::yield_now().await;
                }
            }
        });

        source.poll().await.unwrap();
        counter.abort();

        assert!(
            progressed.load(Ordering::Relaxed) > 0,
            "the counter task never ran while poll() was in flight: poll() \
             blocked the runtime's only worker thread instead of handing the \
             read to spawn_blocking"
        );
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
