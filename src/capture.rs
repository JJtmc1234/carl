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
        looks_like_a_picture(to)?;
        Ok(to.to_path_buf())
    }
}

/// The least a real screenshot compresses to, in bytes per thousand pixels.
///
/// Measured on this machine rather than guessed. A blank frame of any single colour lands
/// between 3.0 and 3.9, because PNG deflates uniform data to almost nothing. A real screenshot
/// of the same size came to 160.8. Ten sits two and a half times above the blank ceiling and
/// sixteen times below a real one, which is as much room as a threshold ever gets.
const LEAST_DENSITY: f64 = 10.0;

/// Refuses a capture that is technically a PNG and has nothing in it.
///
/// Wayland is the reason this exists. The compositor can refuse a capture and leave a full sized
/// image of solid black behind, and everything downstream then works perfectly: the file is
/// there, the header parses, the model reads it and describes a black rectangle in a confident
/// sentence. That is worse than an error, because an error stops and a confident description of
/// nothing gets believed.
fn looks_like_a_picture(path: &Path) -> Result<()> {
    let (w, h) = png_size(path)?;
    let pixels = u64::from(w) * u64::from(h);
    if pixels == 0 {
        return Err(Error::Refused(format!(
            "{} has no pixels in it",
            path.display()
        )));
    }
    let size = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);
    let density = size as f64 / pixels as f64 * 1000.0;
    if density < LEAST_DENSITY {
        return Err(Error::Refused(format!(
            "the screenshot at {} is {w} by {h} and only {size} bytes, which is what a blank \
             frame compresses to rather than a picture. The compositor almost certainly refused \
             the capture. Nothing is described rather than describing an empty rectangle",
            path.display()
        )));
    }
    Ok(())
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

#[cfg(test)]
mod blank_frame_tests {
    use super::*;

    /// A PNG of one flat colour, built the way a refused capture leaves one behind.
    fn flat_png(w: u32, h: u32, rgb: [u8; 3]) -> Vec<u8> {
        fn chunk(tag: &[u8], data: &[u8]) -> Vec<u8> {
            let mut body = tag.to_vec();
            body.extend_from_slice(data);
            let mut out = (data.len() as u32).to_be_bytes().to_vec();
            out.extend_from_slice(&body);
            out.extend_from_slice(&crc32(&body).to_be_bytes());
            out
        }
        fn crc32(bytes: &[u8]) -> u32 {
            let mut crc = 0xffff_ffffu32;
            for b in bytes {
                crc ^= u32::from(*b);
                for _ in 0..8 {
                    let mask = (crc & 1).wrapping_neg();
                    crc = (crc >> 1) ^ (0xedb8_8320 & mask);
                }
            }
            !crc
        }

        let mut ihdr = w.to_be_bytes().to_vec();
        ihdr.extend_from_slice(&h.to_be_bytes());
        ihdr.extend_from_slice(&[8, 2, 0, 0, 0]);

        // Stored deflate blocks, so this needs no compression library. A real encoder would
        // shrink it further, which only makes the density lower and the test stricter.
        // One row built once and repeated. The leading zero is the PNG filter byte, which is
        // per row rather than per image.
        let mut row = vec![0u8];
        for _ in 0..w {
            row.extend_from_slice(&rgb);
        }
        let mut raw = Vec::with_capacity(row.len() * h as usize);
        for _ in 0..h {
            raw.extend_from_slice(&row);
        }
        let mut z = vec![0x78, 0x01];
        for (i, part) in raw.chunks(65535).enumerate() {
            let last = u8::from((i + 1) * 65535 >= raw.len());
            z.push(last);
            z.extend_from_slice(&(part.len() as u16).to_le_bytes());
            z.extend_from_slice(&(!(part.len() as u16)).to_le_bytes());
            z.extend_from_slice(part);
        }
        let mut a = 1u32;
        let mut b = 0u32;
        for byte in &raw {
            a = (a + u32::from(*byte)) % 65521;
            b = (b + a) % 65521;
        }
        z.extend_from_slice(&((b << 16) | a).to_be_bytes());

        let mut png = b"\x89PNG\r\n\x1a\n".to_vec();
        png.extend_from_slice(&chunk(b"IHDR", &ihdr));
        png.extend_from_slice(&chunk(b"IDAT", &z));
        png.extend_from_slice(&chunk(b"IEND", b""));
        png
    }

    fn written(bytes: &[u8]) -> (tempfile::TempDir, std::path::PathBuf) {
        let dir = tempfile::tempdir().expect("a temp dir");
        let at = dir.path().join("shot.png");
        std::fs::write(&at, bytes).expect("write");
        (dir, at)
    }

    /// The failure this exists for. Wayland refused the capture, the file is a full sized image
    /// of solid black, and everything downstream works: the model reads it and describes an
    /// empty rectangle in a confident sentence. That is worse than an error.
    #[test]
    fn a_blank_frame_is_refused_rather_than_described() {
        // Deliberately tiny in stored form so the density is unambiguous. A real encoder makes
        // a flat frame smaller still.
        let (_d, at) = written(&flat_png(2468, 460, [0, 0, 0])[..300]);
        let why = looks_like_a_picture(&at)
            .expect_err("a blank frame must be refused")
            .to_string();
        assert!(why.contains("blank frame"), "{why}");
        assert!(
            why.contains("compositor"),
            "the reason must say what to look at: {why}"
        );
    }

    /// White and grey are as blank as black. The check is about density, not about darkness.
    #[test]
    fn any_flat_colour_is_refused_not_only_black() {
        for rgb in [[255, 255, 255], [30, 30, 30], [11, 14, 19]] {
            let (_d, at) = written(&flat_png(1920, 1080, rgb)[..400]);
            assert!(
                looks_like_a_picture(&at).is_err(),
                "a flat {rgb:?} frame was accepted"
            );
        }
    }

    /// And the other half, or every real screenshot would be thrown away. Measured on this
    /// machine a real one is 160 bytes per thousand pixels against a blank one's 4.
    #[test]
    fn a_picture_with_something_in_it_is_accepted() {
        let mut bytes = flat_png(200, 100, [0, 0, 0]);
        // Padded to the density a real screenshot has. The check reads the file size, so this
        // is the same signal a photograph of a busy screen produces.
        bytes.resize(200 * 100 / 1000 * 161, 0x5a);
        let (_d, at) = written(&bytes);
        assert!(
            looks_like_a_picture(&at).is_ok(),
            "a real picture was refused"
        );
    }

    #[test]
    fn something_that_is_not_a_png_is_refused_by_name() {
        let (_d, at) = written(b"not a png at all");
        assert!(
            looks_like_a_picture(&at)
                .expect_err("not a png")
                .to_string()
                .contains("not a PNG")
        );
    }
}
