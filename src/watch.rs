use crate::UserEvent;
use anyhow::Result;
use notify::RecursiveMode;
use notify_debouncer_full::{DebounceEventResult, new_debouncer};
use std::path::{Path, PathBuf};
use std::time::Duration;

const DEBOUNCE: Duration = Duration::from_millis(150);

/// Watches the *directory* containing `target`, never the file itself: editors
/// save by writing a temp file and renaming over the original, which orphans an
/// inotify watch held on the original inode and silently stops all reloads.
///
/// `notify` is called on the watcher's own thread for every change that gets
/// past the debounce. It is a closure rather than a `tao::EventLoopProxy` so
/// that watching does not require a window, which is the only way the rename
/// case below can be tested at all: an event loop needs a display, and the
/// trap it guards against is invisible from a headless run otherwise.
///
/// The returned debouncer must be kept alive for watching to continue.
pub fn spawn<F>(target: PathBuf, notify: F) -> Result<impl Sized + Send + 'static>
where
    F: Fn(UserEvent) + Send + 'static,
{
    let dir = target
        .parent()
        .map_or_else(|| PathBuf::from("."), Path::to_path_buf);
    let watched = target;

    let mut debouncer = new_debouncer(DEBOUNCE, None, move |result: DebounceEventResult| {
        let Ok(events) = result else { return };

        let touched = events
            .iter()
            .flat_map(|e| e.paths.iter())
            .any(|p| is_target(p, &watched));

        if touched {
            let event = if watched.exists() {
                UserEvent::Changed
            } else {
                UserEvent::Vanished
            };
            notify(event);
        }
    })?;

    debouncer.watch(&dir, RecursiveMode::NonRecursive)?;
    Ok(debouncer)
}

/// Events report the path as the kernel saw it. The watcher registers the
/// canonical parent directory, so inotify reports paths under what was
/// registered and a plain comparison answers. `Path` compares by components,
/// so `/docs/./notes.md` and `/docs//notes.md` compare equal to
/// `/docs/notes.md` already.
fn is_target(candidate: &Path, target: &Path) -> bool {
    candidate == target
}

#[cfg(test)]
mod tests {
    use super::{is_target, spawn};
    use crate::UserEvent;
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::mpsc::{Receiver, RecvTimeoutError, channel};
    use std::time::Duration;

    /// Long enough that a slow machine is not mistaken for a broken watcher,
    /// short enough that a genuinely broken one fails the run rather than
    /// hanging it. The debounce itself is 150ms.
    const SETTLE: Duration = Duration::from_secs(5);

    /// A directory that removes itself, so the suite needs no `tempfile`
    /// dependency to test a file watcher.
    struct TempDir(PathBuf);

    impl TempDir {
        fn new() -> Self {
            static COUNTER: AtomicUsize = AtomicUsize::new(0);
            let name = format!(
                "mhr-watch-{}-{}",
                std::process::id(),
                COUNTER.fetch_add(1, Ordering::Relaxed)
            );

            let path = std::env::temp_dir().join(name);
            std::fs::create_dir_all(&path).expect("temp dir is creatable");
            // Watch events report resolved paths, and /tmp is a symlink on some
            // platforms, so an uncanonicalized path here would fail the parent
            // comparison in is_target for reasons unrelated to the test.
            let path = std::fs::canonicalize(&path).expect("temp dir is canonical");
            Self(path)
        }

        fn join(&self, name: &str) -> PathBuf {
            self.0.join(name)
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn watching(target: &Path) -> (impl Sized, Receiver<UserEvent>) {
        let (sender, receiver) = channel();
        let debouncer = spawn(target.to_path_buf(), move |event| {
            let _ = sender.send(event);
        })
        .expect("watcher starts");
        (debouncer, receiver)
    }

    /// The trap the module exists to avoid. Editors do not write a file in
    /// place, they write a new one beside it and rename over the top, which
    /// leaves a watch held on the original inode pointing at a file nothing
    /// can reach any more.
    ///
    /// It has to be two saves, and that is the whole point of the test. The
    /// first rename still reports something even when the watch is on the file,
    /// because the orphaned inode is what the rename happened to. Only the
    /// second save is silent, which is exactly why the bug survives a casual
    /// check: the viewer looks like it works, once. Watching the parent
    /// directory is what survives it, and this is the only thing in the suite
    /// that would notice if that changed.
    #[test]
    fn redraws_on_every_save_that_arrives_by_rename() {
        let dir = TempDir::new();
        let target = dir.join("notes.md");
        std::fs::write(&target, "before").expect("fixture is writable");

        let (_debouncer, events) = watching(&target);

        for pass in 1..=2 {
            let staged = dir.join("notes.md.tmp");
            std::fs::write(&staged, format!("after {pass}")).expect("temp file is writable");
            std::fs::rename(&staged, &target).expect("rename over the target");

            assert!(
                matches!(events.recv_timeout(SETTLE), Ok(UserEvent::Changed)),
                "save {pass} by rename produced no redraw"
            );
        }
    }

    #[test]
    fn redraws_when_the_file_is_written_in_place() {
        let dir = TempDir::new();
        let target = dir.join("notes.md");
        std::fs::write(&target, "before").expect("fixture is writable");

        let (_debouncer, events) = watching(&target);
        std::fs::write(&target, "after").expect("target is writable");

        assert!(matches!(
            events.recv_timeout(SETTLE),
            Ok(UserEvent::Changed)
        ));
    }

    #[test]
    fn reports_the_file_going_away() {
        let dir = TempDir::new();
        let target = dir.join("notes.md");
        std::fs::write(&target, "here").expect("fixture is writable");

        let (_debouncer, events) = watching(&target);
        std::fs::remove_file(&target).expect("target is removable");

        assert!(matches!(
            events.recv_timeout(SETTLE),
            Ok(UserEvent::Vanished)
        ));
    }

    /// The whole directory is watched, so everything in it that is not the
    /// target has to be filtered back out.
    #[test]
    fn ignores_its_neighbors_in_the_watched_directory() {
        let dir = TempDir::new();
        let target = dir.join("notes.md");
        std::fs::write(&target, "here").expect("fixture is writable");

        let (_debouncer, events) = watching(&target);
        std::fs::write(dir.join("other.md"), "unrelated").expect("neighbor is writable");

        assert!(
            matches!(
                events.recv_timeout(Duration::from_millis(600)),
                Err(RecvTimeoutError::Timeout)
            ),
            "a change to another file caused a redraw"
        );
    }

    #[test]
    fn matches_the_target_by_its_own_path() {
        let target = Path::new("/docs/notes.md");
        assert!(is_target(target, target));
    }

    /// The comparison is exact, so what it accepts is worth pinning: `Path`
    /// compares by components, which is what lets a plain `==` stand in for a
    /// normalizing match.
    #[test]
    fn matches_a_spelling_that_differs_only_in_separators() {
        let target = Path::new("/docs/notes.md");
        for spelling in ["/docs/./notes.md", "/docs//notes.md", "/docs/notes.md/"] {
            assert!(is_target(Path::new(spelling), target), "{spelling}");
        }
    }

    /// A path that reaches the same file through a symlinked parent is not the
    /// target. This never comes up in practice, because the watcher registers
    /// the canonical directory and inotify reports paths under what was
    /// registered, but it pins the comparison as an exact one.
    #[test]
    fn does_not_match_across_a_symlinked_parent() {
        assert!(!is_target(
            Path::new("/link/notes.md"),
            Path::new("/real/notes.md")
        ));
    }

    #[test]
    fn does_not_match_the_same_name_in_another_directory() {
        assert!(!is_target(
            Path::new("/elsewhere/notes.md"),
            Path::new("/docs/notes.md")
        ));
    }

    #[test]
    fn does_not_match_a_neighbor() {
        assert!(!is_target(
            Path::new("/docs/other.md"),
            Path::new("/docs/notes.md")
        ));
    }
}
