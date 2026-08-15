//! Pictures somebody pasted into Slack.
//!
//! Pasting a screenshot into a thread is the most natural thing a person does when they want
//! help with something on screen, and until now it was the least useful: Carl saw the message
//! text, which for a bare screenshot is nothing at all, and answered as though nothing had
//! been sent.
//!
//! He can already read images. `capture.rs` takes one and hands Claude a path, and Claude
//! reads it with its own file tools. A Slack image only needs to become a file in the same
//! place, so nothing downstream has to learn anything new.
//!
//! Downloading needs the bot token, which is the part that is easy to get wrong. A Slack file
//! url is not public, and fetching it without the token returns an HTML sign in page with a
//! 200 status. So a download that "works" and gives back a page of markup is the expected
//! failure, and it is checked for rather than trusted.

use std::path::{Path, PathBuf};

use crate::{Error, Result};

/// The most a picture may be before Carl declines to fetch it.
///
/// Screenshots are well under this. Anything larger is a video, a zip, or somebody testing
/// what happens, and none of those are things Carl can read anyway.
pub const MAX_BYTES: u64 = 20 * 1024 * 1024;

/// A picture attached to a message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Shared {
    pub id: String,
    pub name: String,
    /// The private download url, which needs the bot token.
    pub url: String,
    pub mimetype: String,
    pub size: u64,
}

/// Pictures on this message, if any.
///
/// Only images. Carl can read a picture and cannot read a spreadsheet, and pretending
/// otherwise means downloading something to fail on it a second later.
pub fn images_in(event: &serde_json::Value) -> Vec<Shared> {
    let Some(files) = event.get("files").and_then(|f| f.as_array()) else {
        return Vec::new();
    };

    files
        .iter()
        .filter_map(|f| {
            let mimetype = f.get("mimetype")?.as_str()?.to_string();
            if !mimetype.starts_with("image/") {
                return None;
            }
            Some(Shared {
                id: f.get("id")?.as_str()?.to_string(),
                name: f
                    .get("name")
                    .and_then(|n| n.as_str())
                    .unwrap_or("shared image")
                    .to_string(),
                // The private url, not permalink. permalink is a page for a person to look at
                // and url_private is the bytes.
                url: f
                    .get("url_private_download")
                    .or_else(|| f.get("url_private"))?
                    .as_str()?
                    .to_string(),
                mimetype,
                size: f.get("size").and_then(|s| s.as_u64()).unwrap_or(0),
            })
        })
        .collect()
}

/// A filename for a shared picture, safe to join onto a directory.
///
/// Built from the Slack id and the mimetype rather than the name somebody gave the file. A
/// name arrives from another person and can be anything, including something path shaped, and
/// this becomes a real path on JJ's machine.
pub fn filename_for(file: &Shared) -> String {
    let ext = match file.mimetype.as_str() {
        "image/png" => "png",
        "image/jpeg" => "jpg",
        "image/gif" => "gif",
        "image/webp" => "webp",
        _ => "img",
    };
    let id: String = file
        .id
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .take(32)
        .collect();
    format!("slack-{}.{ext}", if id.is_empty() { "shared" } else { &id })
}

/// Downloads one picture into `into`, returning where it landed.
pub fn fetch(file: &Shared, bot_token: &str, into: &Path) -> Result<PathBuf> {
    if file.size > MAX_BYTES {
        return Err(Error::Refused(format!(
            "{} is {} bytes, which is more than Carl will fetch",
            file.name, file.size
        )));
    }
    std::fs::create_dir_all(into)?;
    let path = into.join(filename_for(file));

    let mut res = ureq::get(&file.url)
        .header("Authorization", &format!("Bearer {bot_token}"))
        .call()
        .map_err(|e| Error::Refused(format!("could not fetch {}: {e}", file.name)))?;

    let bytes = res
        .body_mut()
        .with_config()
        .limit(MAX_BYTES)
        .read_to_vec()
        .map_err(|e| Error::Refused(format!("could not read {}: {e}", file.name)))?;

    // A Slack file url without a usable token returns a sign in page, with a 200 and a
    // perfectly ordinary looking body. Writing that to screen.png and asking Claude to read it
    // wastes a turn and produces a confident description of nothing.
    if looks_like_a_web_page(&bytes) {
        return Err(Error::Refused(format!(
            "Slack sent a web page instead of {}, which means the token could not read it. \
             The app needs the files:read scope and a reinstall.",
            file.name
        )));
    }

    std::fs::write(&path, bytes)?;
    Ok(path)
}

/// Whether these bytes are markup rather than a picture.
fn looks_like_a_web_page(bytes: &[u8]) -> bool {
    let head = &bytes[..bytes.len().min(200)];
    let text = String::from_utf8_lossy(head).to_lowercase();
    text.contains("<!doctype html") || text.contains("<html")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn event_with(files: serde_json::Value) -> serde_json::Value {
        json!({ "type": "message", "text": "look at this", "files": files })
    }

    #[test]
    fn a_shared_png_is_found() {
        let e = event_with(json!([{
            "id": "F123ABC",
            "name": "Screenshot.png",
            "mimetype": "image/png",
            "url_private_download": "https://files.slack.com/x/F123ABC/download",
            "size": 51200
        }]));

        let found = images_in(&e);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].id, "F123ABC");
        assert_eq!(found[0].size, 51200);
    }

    /// Carl can read a picture and cannot read a spreadsheet. Downloading one to fail on it a
    /// second later helps nobody.
    #[test]
    fn only_pictures_count() {
        let e = event_with(json!([
            { "id": "F1", "mimetype": "application/pdf", "url_private": "https://x/1" },
            { "id": "F2", "mimetype": "text/csv", "url_private": "https://x/2" },
            { "id": "F3", "mimetype": "image/jpeg", "url_private": "https://x/3" }
        ]));
        let found = images_in(&e);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].id, "F3");
    }

    #[test]
    fn a_message_with_no_files_finds_none() {
        assert!(images_in(&json!({ "type": "message", "text": "hello" })).is_empty());
    }

    /// The name arrives from another person and can be anything, and it becomes a real path on
    /// JJ's machine.
    #[test]
    fn a_filename_cannot_be_path_shaped() {
        let file = Shared {
            id: "../../etc/passwd".into(),
            name: "../../../evil.png".into(),
            url: "https://x".into(),
            mimetype: "image/png".into(),
            size: 10,
        };
        let n = filename_for(&file);
        assert!(!n.contains('/'), "{n}");
        assert!(!n.contains(".."), "{n}");
        assert!(n.ends_with(".png"), "{n}");
    }

    #[test]
    fn an_unknown_picture_type_still_gets_a_name() {
        let file = Shared {
            id: "F9".into(),
            name: "x".into(),
            url: "https://x".into(),
            mimetype: "image/heic".into(),
            size: 10,
        };
        assert_eq!(filename_for(&file), "slack-F9.img");
    }

    /// The expected failure. Slack answers an unauthorised file request with a sign in page
    /// and a 200, so a download that looks like it worked is the normal way this breaks.
    #[test]
    fn a_sign_in_page_is_not_a_picture() {
        assert!(looks_like_a_web_page(b"<!DOCTYPE html><html><head>"));
        assert!(looks_like_a_web_page(b"\n  <html lang=\"en\">"));
        assert!(!looks_like_a_web_page(&[
            0x89, b'P', b'N', b'G', 0x0d, 0x0a
        ]));
        assert!(!looks_like_a_web_page(&[0xff, 0xd8, 0xff, 0xe0]));
    }

    #[test]
    fn something_far_too_big_is_refused_without_fetching_it() {
        let file = Shared {
            id: "F1".into(),
            name: "huge.png".into(),
            url: "https://files.slack.com/never-reached".into(),
            mimetype: "image/png".into(),
            size: MAX_BYTES + 1,
        };
        let d = tempfile::tempdir().unwrap();
        let err = fetch(&file, "xoxb-fake", d.path()).unwrap_err().to_string();
        assert!(err.contains("more than Carl will fetch"), "{err}");
    }
}
