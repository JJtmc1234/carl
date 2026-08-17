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

use std::io::Read;
use std::os::unix::fs::{FileTypeExt, PermissionsExt};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicPtr, Ordering};

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

/// A bound socket that tidies up after itself.
///
/// The socket file outlives the process that made it unless somebody unlinks it. Leftovers are
/// already survivable, because `bind` tests a socket that is in the way and clears it when nothing
/// answers. But survivable is not the same as tidy: debris makes `ls` lie about what is running,
/// and the recovery path should be for crashes rather than for every ordinary stop.
///
/// Drop covers returning normally and unwinding on a panic. It does not cover a signal, which is
/// why `on_signal` exists, and it does not cover SIGKILL or a power cut, which is what the stale
/// socket recovery in `bind` is still there for.
pub struct Bound {
    listener: UnixListener,
    path: PathBuf,
}

impl Bound {
    pub fn listener(&self) -> &UnixListener {
        &self.listener
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for Bound {
    fn drop(&mut self) {
        if self.path.as_os_str().is_empty() {
            return;
        }
        // Only if it is still ours. A restart that rebound the same path must not have its socket
        // deleted by the previous process finally getting round to its Drop.
        if let Ok(meta) = std::fs::metadata(&self.path)
            && meta.file_type().is_socket()
        {
            let _ = std::fs::remove_file(&self.path);
        }
    }
}

/// What, if anything, is behind a socket file that is in the way.
enum Behind {
    /// Something is serving. Do not touch it.
    Backend,
    /// Debris from a process that died. Safe to clear.
    Nothing,
    /// Could not find out, which is not the same as nothing.
    CannotTell(String),
}

/// What a failed connect proves, which is much less than it looks.
///
/// `ECONNREFUSED` is the one error that means what it says: the address is there and nothing is
/// listening on it. Every other error is an admission of ignorance. `EAGAIN` in particular comes
/// from a live listener whose backlog is full, which is what a loaded machine produces, and
/// `EMFILE` comes from this process being out of descriptors. Reading either as debris would
/// delete a running backend's socket.
///
/// Split out as a pure function so the table can be checked without arranging for the kernel to
/// produce each error, which mostly means exhausting a real resource and disturbing whatever else
/// is running.
fn failed_connect(kind: std::io::ErrorKind, why: &str) -> Behind {
    match kind {
        std::io::ErrorKind::ConnectionRefused => Behind::Nothing,
        _ => Behind::CannotTell(why.to_string()),
    }
}

/// Asks what is behind a socket, three ways rather than two.
///
/// Two mistakes were made here and both were found by a flaky test rather than by reasoning,
/// which is why the shape is now this careful.
///
/// A successful `connect` is not proof of life. The kernel can complete a connection into a
/// listener's backlog and that listener can then close, so the connect succeeds against a server
/// that is already gone. Reading settles it: a peer that has gone gives end of file at once, and
/// a live one sends nothing on a fresh connection because this protocol speaks only when spoken
/// to, so the read times out with the connection still open. Silence is the liveness signal,
/// which is unusual enough to be worth saying.
///
/// A *failed* connect is not proof of death either, and believing it was is the worse of the two
/// mistakes. Under load, connecting to a live listener whose backlog is full returns `EAGAIN`,
/// and treating that as debris would delete a running backend's socket and strand it. Only
/// `ECONNREFUSED` actually means nothing is there. Everything else is an admission of ignorance,
/// and the answer to ignorance is to refuse rather than to guess in the destructive direction.
fn behind(path: &Path) -> Behind {
    let stream = match UnixStream::connect(path) {
        Ok(s) => s,
        Err(e) => return failed_connect(e.kind(), &e.to_string()),
    };

    if let Err(e) = stream.set_read_timeout(Some(PROBE)) {
        return Behind::CannotTell(e.to_string());
    }

    // Retried on `EINTR`, which means the wait was interrupted by a signal and says nothing at
    // all about the peer. Rust does not retry it, and under a loaded machine it happens often
    // enough that without this a second backend reports "could not find out" instead of the true
    // and much more useful "already listening". Bounded, so a storm of signals cannot spin here.
    let deadline = std::time::Instant::now() + PROBE * 4;
    loop {
        match (&stream).read(&mut [0u8; 1]) {
            Ok(0) => return Behind::Nothing,
            Ok(_) => return Behind::Backend,
            Err(e) => match e.kind() {
                // Waited the full time with the connection still open, which is exactly what a
                // live backend looks like: it speaks only when spoken to.
                std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut => {
                    return Behind::Backend;
                }
                std::io::ErrorKind::Interrupted if std::time::Instant::now() < deadline => continue,
                _ => return Behind::CannotTell(e.to_string()),
            },
        }
    }
}

/// How long to wait for a socket in the way to prove it is alive.
///
/// Paid only at startup and only when a socket file is already there.
const PROBE: std::time::Duration = std::time::Duration::from_millis(150);

/// Binds and returns something that will tidy up.
pub fn hold(path: &Path) -> Result<Bound> {
    Ok(Bound {
        listener: bind(path)?,
        path: path.to_path_buf(),
    })
}

/// Removes the socket on SIGTERM or SIGINT, then dies the way it was asked to.
///
/// Reaches past the standard library, which has no signal API at all, and is the only place in
/// Carl that does. Kept to the two calls POSIX guarantees are safe to make from a handler:
/// `unlink` and `_exit`. Anything that allocates, locks or formats would be a real bug here rather
/// than a style question, because a handler runs in the middle of whatever the program was doing.
///
/// systemd stops a service with SIGTERM, so without this every ordinary stop would leave a socket
/// behind. That makes it the common case rather than the rare one.
pub fn on_signal(path: &Path) -> Result<()> {
    // Allocated and leaked here, on purpose, before any handler can run. It has to outlive
    // everything and a handler may not touch the allocator, so the process owning one string for
    // its whole life is the cheap correct answer.
    let doomed = std::ffi::CString::new(path.as_os_str().as_encoded_bytes())
        .map_err(|_| Error::Refused(format!("{} contains a zero byte", path.display())))?;
    DOOMED.store(doomed.into_raw(), Ordering::SeqCst);

    // Safety: installing a handler that only makes async signal safe calls. Called once, from
    // `main`, before anything else is listening.
    unsafe {
        libc::signal(libc::SIGTERM, handler as *const () as libc::sighandler_t);
        libc::signal(libc::SIGINT, handler as *const () as libc::sighandler_t);
    }
    Ok(())
}

/// The socket to unlink when a signal arrives.
///
/// An atomic pointer rather than a mutable static, because loading one is a single instruction
/// with no aliasing question, and a handler is exactly where that question would be hard to
/// answer.
static DOOMED: AtomicPtr<std::ffi::c_char> = AtomicPtr::new(std::ptr::null_mut());

extern "C" fn handler(signal: libc::c_int) {
    let path = DOOMED.load(Ordering::SeqCst);
    // Safety: `unlink` and `_exit` are both async signal safe, which is the whole constraint on
    // what may happen in here. The pointer was set before any handler could run and is never
    // freed, so it is either null or valid.
    unsafe {
        if !path.is_null() {
            libc::unlink(path);
        }
        // 128 plus the signal is what a shell reports for a process stopped this way, so
        // `carl panel` exiting looks like anything else that took a SIGTERM.
        libc::_exit(128 + signal);
    }
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
        match behind(path) {
            Behind::Backend => {
                return Err(Error::Refused(format!(
                    "a panel backend is already listening on {}",
                    path.display()
                )));
            }
            Behind::Nothing => std::fs::remove_file(path)?,
            Behind::CannotTell(why) => {
                return Err(Error::Refused(format!(
                    "there is a socket at {} and this could not find out whether anything is \
                     behind it: {why}. Refusing rather than guessing, because deleting a live \
                     backend's socket would strand it. Try again, or remove the file if you are \
                     sure nothing is running.",
                    path.display()
                )));
            }
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

    /// The mistake worth a test of its own, because it is the destructive one.
    ///
    /// A failed connect is not proof that nothing is there. `EAGAIN` comes from a live listener
    /// whose backlog is full, which is what a loaded machine produces, and `EMFILE` from running
    /// out of descriptors. Reading either as debris would delete a running backend's socket and
    /// strand it, so only a refused connection may ever lead to a delete.
    #[test]
    fn only_a_refused_connection_means_nothing_is_there() {
        use std::io::ErrorKind;

        assert!(matches!(
            failed_connect(ErrorKind::ConnectionRefused, "refused"),
            Behind::Nothing
        ));

        for uncertain in [
            ErrorKind::WouldBlock,
            ErrorKind::Interrupted,
            ErrorKind::PermissionDenied,
            ErrorKind::TimedOut,
            ErrorKind::Other,
        ] {
            assert!(
                matches!(failed_connect(uncertain, "x"), Behind::CannotTell(_)),
                "{uncertain:?} must never lead to deleting a socket"
            );
        }
    }

    /// And the message says what to do, because refusing without a way forward is a dead end.
    #[test]
    fn refusing_to_guess_explains_itself() {
        match failed_connect(
            std::io::ErrorKind::WouldBlock,
            "resource temporarily unavailable",
        ) {
            Behind::CannotTell(why) => assert!(why.contains("resource"), "{why}"),
            other => panic!("wrong answer: {}", matches!(other, Behind::Nothing)),
        }
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
