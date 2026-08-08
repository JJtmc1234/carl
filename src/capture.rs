//! Taking a picture of the screen.
//!
//! Wayland does not let a program simply grab the framebuffer, which is the right default and
//! the reason `org.gnome.Shell.Screenshot` answers `AccessDenied` to an ordinary caller.
//! `gnome-screenshot` is on the sanctioned side of that boundary, so Carl asks it rather than
//! trying to go around the restriction.
//!
//! Nothing here is game specific. A screenshot is a screenshot.

use std::path::{Path, PathBuf};
use std::process::Command;

use crate::{Error, Result};

/// How much of the screen to take.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Area {
    /// Everything. The right choice for a fullscreen game.
    Screen,
    /// Just the focused window.
    Window,
}

pub struct Camera {
    program: PathBuf,
}

impl Default for Camera {
    fn default() -> Self {
        Self {
            program: PathBuf::from("gnome-screenshot"),
        }
    }
}

impl Camera {
    pub fn at(program: impl Into<PathBuf>) -> Self {
        Self {
            program: program.into(),
        }
    }

    pub fn args_for(&self, area: Area, to: &Path) -> Vec<String> {
        let mut args = Vec::new();
        if area == Area::Window {
            args.push("--window".into());
        }
        // The pointer is included because where you are pointing is usually the thing you
        // are asking about.
        //
        // There is no way to suppress the white flash. gnome-screenshot has no flag for it,
        // there is no gsetting, and the flash comes from GNOME Shell itself. The Shell's own
        // D-Bus method does take a flash boolean, but it answers AccessDenied to any caller
        // it has not sanctioned, so that route is closed. Carl flashes the screen every time
        // he looks, and during a game that is genuinely unpleasant.
        args.push("--include-pointer".into());
        args.push("--file".into());
        args.push(to.to_string_lossy().into_owned());
        args
    }

    /// Captures to `to` and returns the path.
    pub fn capture(&self, area: Area, to: &Path) -> Result<PathBuf> {
        if let Some(parent) = to.parent() {
            std::fs::create_dir_all(parent)?;
        }
        // A stale file from a previous run would otherwise look like a fresh capture, and
        // Carl would confidently describe a screen from ten minutes ago.
        let _ = std::fs::remove_file(to);

        let mut cmd = Command::new(&self.program);
        cmd.args(self.args_for(area, to));

        // Strip the snap environment before spawning.
        //
        // Carl may be started from inside a snap, and VS Code is one. A snap sets
        // LD_LIBRARY_PATH at an old bundled glibc, which a normally installed
        // gnome-screenshot then loads instead of the system one and dies with
        // "undefined symbol: __libc_pthread_init". The failure names a symbol rather than a
        // cause, so it is worth removing rather than debugging twice.
        for leaked in [
            "LD_LIBRARY_PATH",
            "LD_PRELOAD",
            "SNAP",
            "SNAP_NAME",
            "SNAP_REVISION",
            "GTK_PATH",
            "GIO_MODULE_DIR",
            "GSETTINGS_SCHEMA_DIR",
        ] {
            cmd.env_remove(leaked);
        }

        let out = cmd.output().map_err(|e| {
            Error::Refused(format!(
                "cannot run {} ({e}). Install it with: sudo apt install gnome-screenshot",
                self.program.display()
            ))
        })?;

        if !out.status.success() {
            return Err(Error::Refused(format!(
                "screenshot failed: {}",
                String::from_utf8_lossy(&out.stderr).trim()
            )));
        }

        // gnome-screenshot can exit zero and still write nothing, for example when the
        // compositor refuses. Checking the file is the only honest confirmation.
        let size = std::fs::metadata(to).map(|m| m.len()).unwrap_or(0);
        if size == 0 {
            return Err(Error::Refused(format!(
                "{} reported success but wrote no image to {}",
                self.program.display(),
                to.display()
            )));
        }
        Ok(to.to_path_buf())
    }
}

/// Width and height straight out of the PNG header.
///
/// Read here rather than pulling in an image crate. Every PNG starts with an 8 byte signature
/// then an IHDR chunk whose first two fields are the dimensions, big endian.
pub fn png_size(path: &Path) -> Result<(u32, u32)> {
    let bytes = std::fs::read(path)?;
    if bytes.len() < 24 || &bytes[..8] != b"\x89PNG\r\n\x1a\n" {
        return Err(Error::Refused(format!("{} is not a PNG", path.display())));
    }
    let w = u32::from_be_bytes([bytes[16], bytes[17], bytes[18], bytes[19]]);
    let h = u32::from_be_bytes([bytes[20], bytes[21], bytes[22], bytes[23]]);
    Ok((w, h))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_full_screen_grab_takes_no_window_flag() {
        let args = Camera::default().args_for(Area::Screen, Path::new("/tmp/a.png"));
        assert!(!args.contains(&"--window".to_string()), "{args:?}");
        assert!(args.contains(&"/tmp/a.png".to_string()), "{args:?}");
    }

    #[test]
    fn a_window_grab_asks_for_the_window() {
        let args = Camera::default().args_for(Area::Window, Path::new("/tmp/a.png"));
        assert!(args.contains(&"--window".to_string()), "{args:?}");
    }

    /// The error has to name the fix. "No such file or directory" sends someone hunting.
    #[test]
    fn a_missing_screenshot_tool_says_how_to_install_it() {
        let err = Camera::at("/nonexistent/gnome-screenshot")
            .capture(Area::Screen, Path::new("/tmp/carl-never.png"))
            .unwrap_err();
        assert!(err.to_string().contains("apt install"), "{err}");
    }

    /// Exiting zero is not proof. The compositor can refuse and leave nothing behind.
    #[test]
    fn success_with_no_image_is_still_a_failure() {
        let dir = tempfile::tempdir().unwrap();
        let stub = dir.path().join("liar");
        std::fs::write(&stub, "#!/bin/sh\nexit 0\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&stub, std::fs::Permissions::from_mode(0o755)).unwrap();
        }

        let err = Camera::at(&stub)
            .capture(Area::Screen, &dir.path().join("shot.png"))
            .unwrap_err();
        assert!(err.to_string().contains("wrote no image"), "{err}");
    }

    /// A leftover from last time would be described as if it were current.
    #[test]
    fn a_stale_file_is_cleared_before_capturing() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("shot.png");
        std::fs::write(&target, "an old screenshot").unwrap();

        let stub = dir.path().join("noop");
        std::fs::write(&stub, "#!/bin/sh\nexit 0\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&stub, std::fs::Permissions::from_mode(0o755)).unwrap();
        }

        assert!(Camera::at(&stub).capture(Area::Screen, &target).is_err());
        assert!(!target.exists(), "the stale image must be gone, not reused");
    }

    #[test]
    fn png_dimensions_are_read_from_the_header() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("t.png");

        // A minimal valid header claiming 1920 by 1080.
        let mut bytes = b"\x89PNG\r\n\x1a\n".to_vec();
        bytes.extend_from_slice(&13u32.to_be_bytes());
        bytes.extend_from_slice(b"IHDR");
        bytes.extend_from_slice(&1920u32.to_be_bytes());
        bytes.extend_from_slice(&1080u32.to_be_bytes());
        bytes.extend_from_slice(&[8, 6, 0, 0, 0]);
        std::fs::write(&path, bytes).unwrap();

        assert_eq!(png_size(&path).unwrap(), (1920, 1080));
    }

    #[test]
    fn something_that_is_not_a_png_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nope.png");
        std::fs::write(&path, "just some text pretending").unwrap();
        assert!(png_size(&path).is_err());
    }
}
