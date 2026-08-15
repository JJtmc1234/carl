//! Binding the panel socket, safely.
//!
//! The socket is the panel backend's whole attack surface, and it is a bigger one than it looks:
//! anything that can talk to it is treated as JJ, with authority over every agent. So the
//! permissions are not a precaution here, they are the authentication.
//!
//! **This is deliberately the same shape as `aosd`'s control socket**, which solved the same
//! problem in the same house. It is not copied for the sake of it. Two bug fixes are baked into
//! the order of operations below and both were found the hard way there, so writing a fresh
//! version would mean finding them again.
//!
//! Carl does not depend on the `aos` crates, and adding a cross repository dependency for forty
//! lines would couple two projects that otherwise have separate lifetimes. The pattern is shared
//! and the code is not, which is written down here so the next person knows the duplication was
//! considered rather than missed.

use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};

use crate::{Error, Result};

/// The kernel's cap on a socket path, from `sun_path` in `sockaddr_un`.
///
/// 108 on Linux. Written as a constant rather than a magic number because the failure it prevents
/// is otherwise reported as "path must be shorter than SUN_LEN", which names the limit without
/// saying what it is.
const SUN_LEN: usize = 108;

/// Where the socket lives, under Carl's home.
///
/// Inside a directory of its own so the directory can be locked down without changing the
/// permissions of everything else in `~/.carl`.
pub fn socket_path(home: &Path) -> PathBuf {
    home.join("panel/panel.sock")
}

/// Binds the panel socket, owner only.
///
/// A socket file that is already there is either a live backend or debris from one that crashed,
/// and guessing is not acceptable in either direction. Deleting a live backend's socket would
/// strand it; refusing to start because of a dead one's leftovers would need a manual cleanup
/// after every crash. So it asks: if something answers, refuse. If nothing answers, clear it.
pub fn bind(path: &Path) -> Result<UnixListener> {
    // A unix socket path is capped by `sun_path` in the kernel, near 108 bytes, and the error it
    // gives back names a constant rather than the path. Checked here so a long `--home` says what
    // is actually wrong, and checked before the directory is created so a failure leaves nothing
    // behind to clean up.
    let len = path.as_os_str().len();
    if len >= SUN_LEN {
        return Err(Error::Refused(format!(
            "the socket path is {len} bytes and a unix socket allows under {SUN_LEN}. Carl's home \
             is too deep for a socket inside it: {}",
            path.display()
        )));
    }

    if path.exists() {
        match UnixStream::connect(path) {
            Ok(_) => {
                return Err(Error::Refused(format!(
                    "a panel backend is already listening on {}",
                    path.display()
                )));
            }
            Err(_) => std::fs::remove_file(path)?,
        }
    }

    let dir = path
        .parent()
        .ok_or_else(|| Error::Refused(format!("{} has no parent directory", path.display())))?;
    std::fs::create_dir_all(dir)?;

    // The directory is the outer lock, and it is the part that actually matters. Without the
    // execute bit nobody else can reach a name inside it at all, whatever that name's own
    // permissions say.
    std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))?;

    // Bind at the real path, then tighten. Narrowing the umask across the bind would be wrong
    // because umask is per process rather than per thread, so it changes what every other thread
    // creates at that moment. Binding under a staging name and renaming would be wrong because a
    // socket path is capped near 108 bytes and a staging name is longer than the real one. The
    // 0700 directory above means there is nobody who can reach the socket in the gap anyway.
    let listener = UnixListener::bind(path)?;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;

    // Assert rather than trust. A filesystem that quietly ignored the chmod is one where the
    // authentication does not exist, and serving on it would be serving to anybody.
    let mode = std::fs::metadata(path)?.permissions().mode() & 0o777;
    if mode != 0o600 {
        let _ = std::fs::remove_file(path);
        return Err(Error::Refused(format!(
            "{} came out as {mode:o} rather than 600, refusing to serve",
            path.display()
        )));
    }

    Ok(listener)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_socket_and_its_directory_are_owner_only() {
        let dir = tempfile::tempdir().unwrap();
        let at = socket_path(dir.path());
        let _l = bind(&at).unwrap();

        let mode = |p: &Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode(&at), 0o600, "the socket");
        assert_eq!(mode(at.parent().unwrap()), 0o700, "the directory above it");
    }

    /// Two backends on one socket would each see half the panel's requests.
    #[test]
    fn a_second_backend_refuses_rather_than_stealing_the_socket() {
        let dir = tempfile::tempdir().unwrap();
        let at = socket_path(dir.path());
        let _first = bind(&at).unwrap();

        let e = bind(&at).unwrap_err().to_string();
        assert!(e.contains("already listening"), "{e}");
    }

    /// Found by running it rather than by testing it. A deep home gave "path must be shorter
    /// than SUN_LEN", which names the limit without saying what it is or which path was too long.
    #[test]
    fn a_home_too_deep_for_a_socket_says_so_and_leaves_nothing_behind() {
        let dir = tempfile::tempdir().unwrap();
        let deep = dir.path().join("d".repeat(120));
        let at = socket_path(&deep);

        let e = bind(&at).unwrap_err().to_string();
        assert!(e.contains("108"), "says what the limit is: {e}");
        assert!(e.contains(&at.display().to_string()), "and which path: {e}");
        assert!(!deep.exists(), "and made no directory on the way out");
    }

    /// And the other half: a crash must not need a manual cleanup before Carl will start.
    #[test]
    fn debris_from_a_crash_is_cleared_rather_than_fatal() {
        let dir = tempfile::tempdir().unwrap();
        let at = socket_path(dir.path());
        {
            let _dead = bind(&at).unwrap();
        }
        assert!(at.exists(), "the file outlives the listener");
        assert!(bind(&at).is_ok(), "and is recognised as debris");
    }
}
