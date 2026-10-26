//! Getting an image into the editor: a file path the terminal pasted after a
//! drag-and-drop, or the system clipboard (Ctrl+V).

use crate::image_attach::{self, PastedImage};
use std::path::PathBuf;

const IMAGE_EXTS: [&str; 5] = ["png", "jpg", "jpeg", "gif", "webp"];

/// Backslash-escaped characters (`My\ Shot.png`) -> literal characters.
fn unescape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            if let Some(next) = chars.next() {
                out.push(next);
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// A terminal drag-and-drop arrives as a bracketed paste of the file path (quoted or
/// backslash-escaped). `Some` only if the whole paste is ONE existing image file.
pub fn parse_dropped_path(raw: &str) -> Option<PathBuf> {
    let t = raw.trim();
    if t.is_empty() || t.contains('\n') {
        return None;
    }
    let quoted = t.len() >= 2
        && ((t.starts_with('\'') && t.ends_with('\'')) || (t.starts_with('"') && t.ends_with('"')));
    let path = if quoted {
        PathBuf::from(&t[1..t.len() - 1])
    } else {
        // Raw first: a Windows path (`C:\Users\me\shot.png`) must not be unescaped.
        let raw = PathBuf::from(t);
        if raw.is_file() {
            raw
        } else {
            PathBuf::from(unescape(t))
        }
    };
    let ext = path.extension()?.to_str()?.to_ascii_lowercase();
    (IMAGE_EXTS.contains(&ext.as_str()) && path.is_file()).then_some(path)
}

/// Dropped path -> prepared image. `None` on any failure: the caller pastes the text as-is.
pub fn load_dropped(raw: &str) -> Option<PastedImage> {
    let path = parse_dropped_path(raw)?;
    image_attach::prepare(std::fs::read(path).ok()?).ok()
}

/// What the system clipboard held.
pub enum Clip {
    Image(PastedImage),
    Text(String),
    Empty,
}

/// Read the system clipboard: an image if there is one, else non-empty text.
pub fn read_clipboard() -> Clip {
    let Ok(mut cb) = arboard::Clipboard::new() else {
        return Clip::Empty;
    };
    if let Ok(img) = cb.get_image() {
        if let Ok(p) =
            image_attach::from_rgba(img.width as u32, img.height as u32, img.bytes.into_owned())
        {
            return Clip::Image(p);
        }
    }
    match cb.get_text() {
        Ok(t) if !t.is_empty() => Clip::Text(t),
        _ => Clip::Empty,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    /// Fresh scratch dir whose name contains a space (what Finder drags produce).
    fn scratch(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("usta img {tag} {}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn write_png(path: &Path) {
        image::RgbaImage::from_pixel(8, 8, image::Rgba([0, 0, 255, 255]))
            .save(path)
            .unwrap();
    }

    #[test]
    fn plain_path_to_an_existing_png() {
        let d = scratch("plain");
        let p = d.join("shot.png");
        write_png(&p);
        assert_eq!(parse_dropped_path(p.to_str().unwrap()), Some(p));
    }

    #[test]
    fn existing_raw_path_wins_over_unescaping() {
        // A literal backslash in the file name (a Windows-style path on Unix).
        let d = scratch("raw");
        let p = d.join("a\\b.png");
        write_png(&p);
        assert_eq!(parse_dropped_path(p.to_str().unwrap()), Some(p));
    }

    #[test]
    fn quoted_paths_are_unwrapped() {
        let d = scratch("quoted");
        let p = d.join("a b.png");
        write_png(&p);
        let s = p.to_str().unwrap();
        assert_eq!(parse_dropped_path(&format!("'{s}'")), Some(p.clone()));
        assert_eq!(parse_dropped_path(&format!("\"{s}\"")), Some(p));
    }

    #[test]
    fn backslash_escaped_spaces_are_unescaped() {
        let d = scratch("escaped");
        let p = d.join("My Shot.png");
        write_png(&p);
        let escaped = p.to_str().unwrap().replace(' ', "\\ ");
        assert_eq!(parse_dropped_path(&escaped), Some(p));
    }

    #[test]
    fn surrounding_whitespace_and_trailing_newline_are_trimmed() {
        let d = scratch("trim");
        let p = d.join("t.png");
        write_png(&p);
        assert_eq!(
            parse_dropped_path(&format!("  {}\n", p.to_str().unwrap())),
            Some(p)
        );
    }

    #[test]
    fn extension_match_is_case_insensitive() {
        let d = scratch("case");
        let p = d.join("T.PNG");
        write_png(&p);
        assert_eq!(parse_dropped_path(p.to_str().unwrap()), Some(p));
    }

    #[test]
    fn nonexistent_file_is_not_a_drop() {
        assert_eq!(parse_dropped_path("/definitely/not/here.png"), None);
    }

    #[test]
    fn non_image_extension_is_not_a_drop() {
        let d = scratch("txt");
        let p = d.join("notes.txt");
        std::fs::write(&p, "hi").unwrap();
        assert_eq!(parse_dropped_path(p.to_str().unwrap()), None);
    }

    #[test]
    fn multiline_or_prose_paste_is_not_a_drop() {
        assert_eq!(parse_dropped_path("line one\nline two.png"), None);
        assert_eq!(parse_dropped_path("see the screenshot please"), None);
        assert_eq!(parse_dropped_path(""), None);
    }

    #[test]
    fn load_dropped_returns_a_prepared_image() {
        let d = scratch("load");
        let p = d.join("x.png");
        write_png(&p);
        let img = load_dropped(p.to_str().unwrap()).unwrap();
        assert_eq!((img.media_type, img.width, img.height), ("image/png", 8, 8));
    }

    #[test]
    fn load_dropped_is_none_for_a_corrupt_image() {
        let d = scratch("corrupt");
        let p = d.join("bad.png");
        std::fs::write(&p, b"not a png").unwrap();
        assert!(load_dropped(p.to_str().unwrap()).is_none());
    }
}
