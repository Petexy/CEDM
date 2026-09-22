//! Opening a file that somebody else is allowed to write.
//!
//! A login screen reads before anybody has proved who they are, and two of the
//! things it reads are files an ordinary account can put there: the look each
//! account publishes on its way into a session, and the picture
//! `accounts-daemon` copies out of a home directory. Neither directory can be
//! taken away — a greeter that cannot read them is a greeter that draws every
//! account in the same purple with the same initial — so what has to be bounded
//! instead is what opening one can cost.
//!
//! `std::fs::File::open` is the wrong tool for that, and not because of what it
//! checks. It is that the open itself is a blocking call, and the check comes
//! after it. A name in a directory is not a file: it can be a directory, a
//! socket, a symbolic link somewhere else, or a named pipe — and opening a
//! named pipe for reading waits, by specification, until somebody opens the
//! other end. Nobody ever does. The greeter stops inside `open`, before the
//! owner check that would have refused the file has run, and the login screen
//! is a black screen for as long as the account that planted it cares to leave
//! it there. Any account can create one: the directory is writable by all of
//! them, which is the whole design of publishing.
//!
//! So this opens with `O_NONBLOCK` as well as `O_NOFOLLOW`, which turns that
//! wait into a descriptor that is handed straight back, and then refuses
//! anything `fstat` says is not a regular file. The flag is cleared again
//! afterwards: a regular file on Linux ignores it, but nothing here wants to
//! depend on that, and past the check the descriptor is an ordinary file being
//! read in the ordinary way.
//!
//! Every check is made against the descriptor rather than the path. The
//! difference matters here more than it usually does, because the directory
//! this reads from is one another account may be writing in at the same moment:
//! a check on a path and a read of that path are two different files whenever
//! somebody wants them to be.

use std::fs::File;
use std::os::unix::fs::MetadataExt;
use std::os::unix::fs::OpenOptionsExt;
use std::os::unix::io::AsRawFd;
use std::path::Path;

/// Who a file has to belong to before anything in it is believed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Owner {
    /// Whoever the system publishes as. `/var/lib/AccountsService/icons` is
    /// `accounts-daemon`'s directory and not an account's, so a file in it is
    /// the system's answer about that account however little it is worth —
    /// there is no second opinion to prefer it over.
    Anyone,
    /// This account and no other. A directory accounts write their own files
    /// into is a directory where a name proves nothing, so the owner is what
    /// decides whether a file found under an account's name is that account's.
    /// What is left to somebody who squats on a name is denying its owner a
    /// colour, which is where every login screen was before any of this
    /// existed.
    Uid(u32),
}

/// Open `path` for reading, or `None` for anything that is not a plain file
/// `owner` may speak through.
///
/// `None` is deliberately the only failure. Every caller is drawing a login
/// screen and every one of them has an answer for an account that has
/// published nothing, so there is no diagnosis here worth the distinction: a
/// pipe, a directory, a symbolic link, a file belonging to somebody else and a
/// file that is simply not there all mean the same thing to the screen.
pub fn open(path: &Path, owner: Owner) -> Option<File> {
    let file = std::fs::OpenOptions::new()
        .read(true)
        // `O_NOFOLLOW` so a link planted under somebody else's name leads
        // nowhere; `O_NONBLOCK` so a named pipe planted there cannot hold the
        // login screen; `O_NOCTTY` because a process that has just opened a
        // terminal by accident has acquired a controlling one; `O_CLOEXEC`
        // because this greeter does start other programs and none of them has
        // any business inheriting this.
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_NOCTTY | libc::O_CLOEXEC)
        .open(path)
        .ok()?;
    let metadata = file.metadata().ok()?;
    if !metadata.is_file() {
        tracing::debug!(
            path = %path.display(),
            "ignoring something that is not a plain file"
        );
        return None;
    }
    if let Owner::Uid(uid) = owner {
        if metadata.uid() != uid {
            tracing::debug!(
                path = %path.display(),
                "ignoring a file the account it is named for does not own"
            );
            return None;
        }
    }
    // Past the check this is an ordinary file. Linux ignores `O_NONBLOCK` on
    // one, so clearing it changes nothing on the machines this runs on; it is
    // cleared anyway, so that no reader downstream has to know the flag was
    // ever set or wonder whether a short read meant the end of the file.
    clear_nonblocking(&file);
    Some(file)
}

fn clear_nonblocking(file: &File) {
    let fd = file.as_raw_fd();
    // SAFETY: `fd` is owned by `file` and outlives both calls, and `F_GETFL`
    // and `F_SETFL` read and write only the descriptor's own flags.
    unsafe {
        let flags = libc::fcntl(fd, libc::F_GETFL);
        if flags >= 0 {
            libc::fcntl(fd, libc::F_SETFL, flags & !libc::O_NONBLOCK);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::sync::mpsc;
    use std::time::Duration;

    fn scratch(tag: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!("cedm-reading-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).unwrap();
        path
    }

    fn fifo(path: &Path) {
        let name = std::ffi::CString::new(path.to_str().unwrap()).unwrap();
        // SAFETY: `name` is a NUL-terminated path that outlives the call.
        assert_eq!(unsafe { libc::mkfifo(name.as_ptr(), 0o644) }, 0);
    }

    /// The one this module exists for.
    ///
    /// A named pipe with nobody at the far end is what a `File::open` never
    /// comes back from. Run on a thread with a deadline, because a regression
    /// here does not fail a test — it hangs the suite, exactly as it would hang
    /// the login screen.
    #[test]
    fn a_named_pipe_is_refused_rather_than_waited_on() {
        let root = scratch("fifo");
        let path = root.join("alex.toml");
        fifo(&path);

        let (answer, answered) = mpsc::channel();
        let asked = path.clone();
        std::thread::spawn(move || {
            let _ = answer.send(open(&asked, Owner::Anyone).is_some());
        });
        assert_eq!(
            answered.recv_timeout(Duration::from_secs(5)),
            Ok(false),
            "opening a pipe must come back, and must come back refusing"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_directory_a_link_and_somebody_elses_file_are_all_refused() {
        let root = scratch("kinds");
        let real = root.join("real.toml");
        std::fs::write(&real, "accent = \"Blue\"\n").unwrap();
        std::fs::create_dir(root.join("directory.toml")).unwrap();
        std::os::unix::fs::symlink(&real, root.join("link.toml")).unwrap();

        assert!(open(&real, Owner::Anyone).is_some());
        assert!(open(&root.join("directory.toml"), Owner::Anyone).is_none());
        assert!(
            open(&root.join("link.toml"), Owner::Anyone).is_none(),
            "a symbolic link is refused even when what it points at is fine"
        );
        assert!(open(&root.join("missing.toml"), Owner::Anyone).is_none());

        // SAFETY: `getuid` cannot fail and touches no memory this owns.
        let mine = unsafe { libc::getuid() };
        assert!(open(&real, Owner::Uid(mine)).is_some());
        assert!(
            open(&real, Owner::Uid(mine.wrapping_add(1))).is_none(),
            "a file is only believed for the account that owns it"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    /// The descriptor that comes back reads like any other.
    #[test]
    fn what_is_opened_reads_to_the_end() {
        use std::io::Read;

        let root = scratch("reads");
        let path = root.join("long.toml");
        let body = "x".repeat(512 * 1024);
        std::fs::write(&path, &body).unwrap();

        let mut raw = String::new();
        open(&path, Owner::Anyone)
            .unwrap()
            .read_to_string(&mut raw)
            .unwrap();
        assert_eq!(raw.len(), body.len());
        let _ = std::fs::remove_dir_all(&root);
    }
}
