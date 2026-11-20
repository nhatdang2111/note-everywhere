//! Serving attachments to the webview through a custom URI scheme.
//!
//! The scheme only resolves lowercase 64-hex content hashes that the store
//! knows, so there is no path to traverse. Content is served with a type
//! derived from its bytes and file name, never from the request.

use std::io::{Read, Seek, SeekFrom};

use super::{AppCore, AppError, AppResult};

pub const SCHEME: &str = "ne-attachment";

/// Custom protocols are `scheme://localhost/...` on macOS and Linux and
/// `http://scheme.localhost/...` on Windows.
pub fn default_url_base() -> String {
    if cfg!(windows) {
        format!("http://{SCHEME}.localhost")
    } else {
        format!("{SCHEME}://localhost")
    }
}

/// Largest attachment this app stores; the server's cap is lower still.
pub const MAX_ATTACHMENT_BYTES: u64 = 64 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AttachmentResponse {
    pub status: u16,
    pub headers: Vec<(&'static str, String)>,
    pub body: Vec<u8>,
}

impl AttachmentResponse {
    /// Answer while the core is still starting.
    pub fn unavailable() -> Self {
        Self::status_only(503)
    }

    fn status_only(status: u16) -> Self {
        Self {
            status,
            headers: vec![("Content-Type", "text/plain".into())],
            body: Vec::new(),
        }
    }
}

const SAFE_TYPES: &[(&str, &str)] = &[
    ("png", "image/png"),
    ("jpg", "image/jpeg"),
    ("jpeg", "image/jpeg"),
    ("gif", "image/gif"),
    ("webp", "image/webp"),
    ("bmp", "image/bmp"),
    ("avif", "image/avif"),
    ("svg", "image/svg+xml"),
    ("pdf", "application/pdf"),
    ("mp3", "audio/mpeg"),
    ("wav", "audio/wav"),
    ("m4a", "audio/mp4"),
    ("mp4", "video/mp4"),
    ("webm", "video/webm"),
    ("txt", "text/plain"),
];

fn sniff(head: &[u8]) -> Option<&'static str> {
    let starts = |magic: &[u8]| head.starts_with(magic);
    if starts(b"\x89PNG\r\n\x1a\n") {
        Some("image/png")
    } else if starts(&[0xFF, 0xD8, 0xFF]) {
        Some("image/jpeg")
    } else if starts(b"GIF87a") || starts(b"GIF89a") {
        Some("image/gif")
    } else if head.len() >= 12 && &head[..4] == b"RIFF" && &head[8..12] == b"WEBP" {
        Some("image/webp")
    } else if head.len() >= 12 && &head[..4] == b"RIFF" && &head[8..12] == b"WAVE" {
        Some("audio/wav")
    } else if starts(b"%PDF-") {
        Some("application/pdf")
    } else {
        None
    }
}

/// The type to serve: what the bytes look like, else what the file name says,
/// else a download-only type. Only a short list of inert types is ever served
/// as anything but `application/octet-stream`.
pub fn mime_for(file_name: &str, head: &[u8]) -> &'static str {
    if let Some(mime) = sniff(head) {
        return mime;
    }
    let extension = file_name
        .rsplit_once('.')
        .map(|(_, e)| e.to_ascii_lowercase());
    extension
        .and_then(|e| {
            SAFE_TYPES
                .iter()
                .find(|(known, _)| *known == e)
                .map(|(_, m)| *m)
        })
        .unwrap_or("application/octet-stream")
}

/// A file extension for a type the UI reports, used when a pasted file has no name.
pub fn extension_for(mime: &str) -> &'static str {
    SAFE_TYPES
        .iter()
        .find(|(_, m)| *m == mime)
        .map_or("bin", |(e, _)| e)
}

pub fn is_plausible_mime(mime: &str) -> bool {
    mime.len() <= 127
        && mime.split_once('/').is_some_and(|(kind, sub)| {
            let ok = |s: &str| {
                !s.is_empty()
                    && s.bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b"+-.".contains(&b))
            };
            ok(kind) && ok(sub)
        })
}

#[derive(Debug, PartialEq, Eq)]
enum ByteRange {
    Whole,
    Part(u64, u64),
    Unsatisfiable,
}

/// Parses a single-range `Range: bytes=...` header against a length.
fn parse_range(header: Option<&str>, len: u64) -> ByteRange {
    let Some(spec) = header.and_then(|h| h.trim().strip_prefix("bytes=")) else {
        return ByteRange::Whole;
    };
    if spec.contains(',') || len == 0 {
        return ByteRange::Whole;
    }
    let Some((first, last)) = spec.split_once('-') else {
        return ByteRange::Whole;
    };
    let (start, end) = match (first.trim(), last.trim()) {
        ("", suffix) => match suffix.parse::<u64>() {
            Ok(n) if n > 0 => (len.saturating_sub(n), len - 1),
            _ => return ByteRange::Unsatisfiable,
        },
        (from, "") => match from.parse::<u64>() {
            Ok(s) => (s, len - 1),
            Err(_) => return ByteRange::Whole,
        },
        (from, to) => match (from.parse::<u64>(), to.parse::<u64>()) {
            (Ok(s), Ok(e)) => (s, e.min(len - 1)),
            _ => return ByteRange::Whole,
        },
    };
    if start >= len || start > end {
        ByteRange::Unsatisfiable
    } else {
        ByteRange::Part(start, end)
    }
}

fn read_range(path: &std::path::Path, start: u64, len: u64) -> std::io::Result<Vec<u8>> {
    let mut file = std::fs::File::open(path)?;
    file.seek(SeekFrom::Start(start))?;
    let mut body = Vec::with_capacity(usize::try_from(len).unwrap_or(0));
    file.take(len).read_to_end(&mut body)?;
    Ok(body)
}

impl AppCore {
    /// Resolves one request of the attachment scheme. `path` is the URL path.
    pub async fn serve_attachment(&self, path: &str, range: Option<&str>) -> AttachmentResponse {
        let hash = path.trim_start_matches('/');
        if !ne_store::is_valid_hash(hash) {
            return AttachmentResponse::status_only(404);
        }
        let mut located = self.locate_attachment(hash);
        if located.is_none() && self.fetch_missing(hash).await.is_ok() {
            located = self.locate_attachment(hash);
        }
        let Some((file, file_name)) = located else {
            return AttachmentResponse::status_only(404);
        };
        let range = range.map(str::to_owned);
        let served = tokio::task::spawn_blocking(move || -> AppResult<AttachmentResponse> {
            let io = |_: std::io::Error| AppError::new("io_error", "The file could not be read.");
            let total = std::fs::metadata(&file).map_err(io)?.len();
            let head = read_range(&file, 0, total.min(16)).map_err(io)?;
            let mime = mime_for(&file_name, &head);
            let (status, start, length, content_range) = match parse_range(range.as_deref(), total)
            {
                ByteRange::Whole => (200, 0, total, None),
                ByteRange::Part(s, e) => {
                    (206, s, e - s + 1, Some(format!("bytes {s}-{e}/{total}")))
                }
                ByteRange::Unsatisfiable => {
                    let mut response = AttachmentResponse::status_only(416);
                    response
                        .headers
                        .push(("Content-Range", format!("bytes */{total}")));
                    return Ok(response);
                }
            };
            let body = read_range(&file, start, length).map_err(io)?;
            let mut headers = vec![
                ("Content-Type", mime.to_owned()),
                ("Content-Length", body.len().to_string()),
                ("Accept-Ranges", "bytes".to_owned()),
                // Content-addressed: the bytes behind a URL never change.
                (
                    "Cache-Control",
                    "private, max-age=31536000, immutable".to_owned(),
                ),
                ("X-Content-Type-Options", "nosniff".to_owned()),
                (
                    "Content-Security-Policy",
                    "default-src 'none'; style-src 'unsafe-inline'; sandbox".to_owned(),
                ),
            ];
            if mime == "application/octet-stream" {
                headers.push(("Content-Disposition", "attachment".to_owned()));
            }
            if let Some(value) = content_range {
                headers.push(("Content-Range", value));
            }
            Ok(AttachmentResponse {
                status,
                headers,
                body,
            })
        })
        .await;
        match served {
            Ok(Ok(response)) => response,
            _ => AttachmentResponse::status_only(500),
        }
    }

    fn locate_attachment(&self, hash: &str) -> Option<(std::path::PathBuf, String)> {
        self.with_ws(|ws| {
            let info = ws.attachment(hash)?;
            let path = ws.attachment_path(hash);
            Ok(info.zip(path.ok()).map(|(i, p)| (p, i.file_name)))
        })
        .ok()
        .flatten()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ranges() {
        assert_eq!(parse_range(None, 10), ByteRange::Whole);
        assert_eq!(parse_range(Some("bytes=0-3"), 10), ByteRange::Part(0, 3));
        assert_eq!(parse_range(Some("bytes=4-"), 10), ByteRange::Part(4, 9));
        assert_eq!(parse_range(Some("bytes=-3"), 10), ByteRange::Part(7, 9));
        assert_eq!(parse_range(Some("bytes=8-99"), 10), ByteRange::Part(8, 9));
        assert_eq!(
            parse_range(Some("bytes=10-12"), 10),
            ByteRange::Unsatisfiable
        );
        assert_eq!(parse_range(Some("bytes=5-2"), 10), ByteRange::Unsatisfiable);
        assert_eq!(parse_range(Some("bytes=0-1,3-4"), 10), ByteRange::Whole);
        assert_eq!(parse_range(Some("items=0-1"), 10), ByteRange::Whole);
    }

    #[test]
    fn types_come_from_content_then_name() {
        assert_eq!(mime_for("x.bin", b"\x89PNG\r\n\x1a\nrest"), "image/png");
        assert_eq!(mime_for("photo.JPG", b"nothing"), "image/jpeg");
        assert_eq!(mime_for("run.exe", b"MZ"), "application/octet-stream");
        assert_eq!(
            mime_for("page.html", b"<script>"),
            "application/octet-stream"
        );
    }

    #[test]
    fn mime_syntax() {
        assert!(is_plausible_mime("image/png"));
        assert!(!is_plausible_mime("image"));
        assert!(!is_plausible_mime("a/b c"));
        assert_eq!(extension_for("image/png"), "png");
        assert_eq!(extension_for("weird/type"), "bin");
    }
}
