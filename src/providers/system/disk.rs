//! How much room is left where it matters.
//!
//! `statvfs` rather than shelling out to `df`, because a panel that samples every couple of
//! seconds should not fork a process to do it, and because `df` output is a formatting contract
//! that changes between distributions.
//!
//! Only the paths that matter are asked about. A full listing of every mount is htop's job, and
//! the question this answers is narrower: can Carl still write down what just happened.

use std::path::Path;

/// What one filesystem has left.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Space {
    pub total_bytes: u64,
    pub available_bytes: u64,
}

impl Space {
    pub fn used_bytes(&self) -> u64 {
        self.total_bytes.saturating_sub(self.available_bytes)
    }

    /// How full, between zero and one. `None` for a filesystem of no size, which is a thing
    /// pseudo filesystems really do report.
    pub fn used_fraction(&self) -> Option<f64> {
        if self.total_bytes == 0 {
            return None;
        }
        Some(self.used_bytes() as f64 / self.total_bytes as f64)
    }
}

/// Asks the kernel how much room is left on the filesystem holding `path`.
///
/// Returns `None` when the path does not exist or cannot be reached, rather than zero. A
/// missing directory and a full disk are different problems and must not render the same.
pub fn space_for(path: &Path) -> Option<Space> {
    let raw = std::ffi::CString::new(path.as_os_str().as_encoded_bytes()).ok()?;
    let mut buf: libc::statvfs = unsafe { std::mem::zeroed() };

    // The one unsafe call in this module. `statvfs` fills a struct we own and reports failure
    // through its return value, which is checked before anything in the struct is read.
    let rc = unsafe { libc::statvfs(raw.as_ptr(), &mut buf) };
    if rc != 0 {
        return None;
    }

    // f_frsize is the fragment size, which is the one the block counts are in. f_bsize is the
    // preferred IO size and is a different number on some filesystems.
    let unit = if buf.f_frsize > 0 {
        buf.f_frsize as u64
    } else {
        buf.f_bsize as u64
    };

    Some(Space {
        total_bytes: (buf.f_blocks as u64).saturating_mul(unit),
        // f_bavail rather than f_bfree, because the difference is space reserved for root and
        // JJ's agents are not root.
        available_bytes: (buf.f_bavail as u64).saturating_mul(unit),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Read against the real machine, because the point of this module is that it works here.
    #[test]
    fn the_root_filesystem_reports_a_believable_size() {
        let s = space_for(Path::new("/")).expect("root should be measurable");
        assert!(
            s.total_bytes > 1 << 30,
            "a root filesystem under a gibibyte is not real"
        );
        assert!(s.available_bytes <= s.total_bytes);
        let used = s.used_fraction().expect("root has a size");
        assert!((0.0..=1.0).contains(&used), "fraction out of range: {used}");
    }

    #[test]
    fn a_temporary_directory_is_measurable_too() {
        let d = tempfile::tempdir().unwrap();
        assert!(space_for(d.path()).is_some());
    }

    /// The missing metric rule. A path that is not there must not read as a full disk.
    #[test]
    fn a_path_that_does_not_exist_is_unknown_rather_than_zero() {
        assert_eq!(
            space_for(Path::new("/nonexistent/definitely/not/here")),
            None
        );
    }

    #[test]
    fn a_path_with_an_interior_nul_is_refused_rather_than_panicking() {
        assert_eq!(space_for(Path::new("/tmp/a\0b")), None);
    }

    #[test]
    fn a_filesystem_of_no_size_has_no_fraction() {
        let empty = Space {
            total_bytes: 0,
            available_bytes: 0,
        };
        assert_eq!(empty.used_fraction(), None);
        assert_eq!(empty.used_bytes(), 0);
    }

    #[test]
    fn used_is_total_minus_available() {
        let s = Space {
            total_bytes: 100,
            available_bytes: 25,
        };
        assert_eq!(s.used_bytes(), 75);
        assert_eq!(s.used_fraction(), Some(0.75));
    }
}
