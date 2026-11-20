//! Adding attachments, their URLs, and moving them to and from the server.

use ne_store::AttachmentInfo;
use ne_sync::BlobRef;

use super::blob_index::{self, BlobEntry};
use super::protocol::{MAX_ATTACHMENT_BYTES, extension_for, is_plausible_mime, mime_for};
use super::types::AddedAttachment;
use super::{AppCore, AppError, AppResult};

const IMAGE_EXTENSIONS: &[&str] = &["png", "jpg", "jpeg", "gif", "webp", "bmp", "avif", "svg"];

/// Names Windows reserves for devices, with or without an extension.
const RESERVED_NAMES: [&str; 22] = [
    "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
    "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
];

/// A file name without any directory part, safe to use as a path component on
/// every platform. Both `/` and `\` separate components wherever the app runs,
/// so the same input always gives the same output; `std::path` is not used
/// because it only treats `\` as a separator on Windows.
fn safe_name(name: &str) -> String {
    let base = name
        .rsplit(['/', '\\'])
        .find(|part| !part.is_empty())
        .unwrap_or("");
    if matches!(base, "" | "." | "..") {
        return "attachment".to_owned();
    }
    let replaced: String = base
        .chars()
        .map(|c| {
            if c.is_control() || matches!(c, '<' | '>' | ':' | '"' | '|' | '?' | '*') {
                '_'
            } else {
                c
            }
        })
        .collect();
    // Windows drops trailing dots and spaces, which would change the name.
    let cleaned = replaced.trim_end_matches(['.', ' ']);
    if cleaned.is_empty() {
        return "attachment".to_owned();
    }
    let stem = cleaned.split('.').next().unwrap_or("").trim_end();
    if RESERVED_NAMES
        .iter()
        .any(|reserved| stem.eq_ignore_ascii_case(reserved))
    {
        format!("_{cleaned}")
    } else {
        cleaned.to_owned()
    }
}

impl AppCore {
    pub(crate) fn attachment_url_for(&self, hash: &str) -> String {
        format!("{}/{hash}", self.inner.cfg.attachment_url_base)
    }

    fn describe(
        &self,
        info: &AttachmentInfo,
        provided_mime: Option<&str>,
    ) -> AppResult<AddedAttachment> {
        let head = self.store(|ws| {
            let path = ws.attachment_path(&info.hash)?;
            let mut head = vec![0u8; 16];
            let read = std::fs::File::open(&path)
                .and_then(|mut f| std::io::Read::read(&mut f, &mut head))
                .map_err(|e| ne_store::Error::Io {
                    path,
                    message: e.to_string(),
                })?;
            head.truncate(read);
            Ok(head)
        })?;
        let mut mime = mime_for(&info.file_name, &head).to_owned();
        if mime == "application/octet-stream"
            && let Some(provided) = provided_mime.filter(|m| is_plausible_mime(m))
        {
            provided.clone_into(&mut mime);
        }
        Ok(AddedAttachment {
            hash: info.hash.clone(),
            mime,
            byte_length: info.size,
            url: self.attachment_url_for(&info.hash),
        })
    }

    pub async fn attachment_add_bytes(
        &self,
        bytes: Vec<u8>,
        mime: &str,
        file_name: Option<&str>,
    ) -> AppResult<AddedAttachment> {
        if bytes.is_empty() {
            return Err(AppError::invalid("The file is empty."));
        }
        if bytes.len() as u64 > MAX_ATTACHMENT_BYTES {
            return Err(AppError::new(
                "too_large",
                "That file is larger than 64 MiB.",
            ));
        }
        let name = match file_name {
            Some(name) => safe_name(name),
            None => format!("attachment.{}", extension_for(mime)),
        };
        let info = self.store(|ws| ws.add_attachment(&name, &bytes))?;
        self.describe(&info, Some(mime))
    }

    pub async fn attachment_pick(&self, images_only: bool) -> AppResult<Option<AddedAttachment>> {
        let dialogs = self.inner.deps.dialogs.clone();
        let picked = self
            .blocking(move |_| {
                let extensions: &[&str] = if images_only { IMAGE_EXTENSIONS } else { &[] };
                Ok(dialogs.pick_file("Choose a file", extensions))
            })
            .await?;
        let Some(path) = picked else {
            return Ok(None);
        };
        let info = self
            .blocking(move |core| {
                let size = std::fs::metadata(&path)
                    .map_err(|_| AppError::new("io_error", "The file could not be read."))?
                    .len();
                if size > MAX_ATTACHMENT_BYTES {
                    return Err(AppError::new(
                        "too_large",
                        "That file is larger than 64 MiB.",
                    ));
                }
                core.store(|ws| ws.add_attachment_from_path(&path))
            })
            .await?;
        self.describe(&info, None).map(Some)
    }

    pub async fn attachment_url(&self, hash: &str) -> AppResult<String> {
        if !ne_store::is_valid_hash(hash) {
            return Err(AppError::invalid("That is not an attachment hash."));
        }
        Ok(self.attachment_url_for(hash))
    }

    /// Downloads an attachment this device lacks, if the account has it.
    pub(crate) async fn fetch_missing(&self, hash: &str) -> AppResult<()> {
        let runtime = self
            .runtime()
            .ok_or_else(|| AppError::not_found("That attachment"))?;
        let entry = self
            .with_ws(|ws| blob_index::lookup(ws, hash))?
            .ok_or_else(|| AppError::not_found("That attachment"))?;
        let dir = tempfile::tempdir()
            .map_err(|_| AppError::new("io_error", "A temporary file could not be created."))?;
        let target = dir.path().join(safe_name(&entry.file_name));
        let blob = BlobRef {
            hash: entry.cipher_hash,
            size: entry.size,
        };
        runtime
            .client
            .download_attachment(&runtime.key, hash, &blob, &target)
            .await?;
        let wanted = hash.to_owned();
        self.blocking(move |core| {
            let info = core.store(|ws| ws.add_attachment_from_path(&target))?;
            if info.hash == wanted {
                Ok(())
            } else {
                Err(AppError::new(
                    "tampered",
                    "The downloaded file does not match its address.",
                ))
            }
        })
        .await
    }

    /// Uploads attachments that pages show and the server does not have yet,
    /// then records where they went so other devices can find them.
    pub(crate) async fn upload_attachments(&self) -> AppResult<()> {
        let Some(runtime) = self.runtime() else {
            return Ok(());
        };
        let pending = self.with_ws(|ws| {
            let known = blob_index::known(ws)?;
            let mut pending = Vec::new();
            for info in ws.attachments()? {
                if info.ref_count > 0
                    && !known.contains(&info.hash)
                    && let Ok(path) = ws.attachment_path(&info.hash)
                {
                    pending.push((info, path));
                }
            }
            Ok(pending)
        })?;
        let mut entries = Vec::new();
        for (info, path) in pending {
            let file = std::fs::File::open(&path)
                .map_err(|_| AppError::new("io_error", "The file could not be read."))?;
            match runtime
                .client
                .upload_attachment(&runtime.key, &info.hash, file)
                .await
            {
                Ok(blob) => entries.push((
                    info.hash.clone(),
                    BlobEntry {
                        cipher_hash: blob.hash,
                        size: blob.size,
                        file_name: info.file_name,
                    },
                )),
                // Offline or session trouble: stop and retry on the next cycle.
                Err(err)
                    if err.is_retryable() || matches!(err, ne_sync::SyncError::Unauthorized) =>
                {
                    self.with_ws(|ws| blob_index::record(ws, &entries))?;
                    return Err(err.into());
                }
                // This one cannot be uploaded (too large, quota): skip it.
                Err(_) => {}
            }
        }
        self.with_ws(|ws| blob_index::record(ws, &entries))?;
        self.after_local_change();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::safe_name;

    #[test]
    fn names_lose_their_directories() {
        assert_eq!(safe_name("../../etc/passwd"), "passwd");
        assert_eq!(safe_name("a:b.png"), "a_b.png");
        assert_eq!(safe_name(".."), "attachment");
        assert_eq!(safe_name(""), "attachment");
    }

    #[test]
    fn backslash_paths_are_handled_on_every_platform() {
        assert_eq!(safe_name("..\\..\\secret.txt"), "secret.txt");
        assert_eq!(safe_name("C:\\Users\\x\\a.png"), "a.png");
        assert_eq!(safe_name("dir/sub\\file.png"), "file.png");
        assert_eq!(safe_name("folder/"), "folder");
        assert_eq!(safe_name("a\\.."), "attachment");
        assert_eq!(safe_name("a<b>|c?*.png"), "a_b__c__.png");
    }

    #[test]
    fn windows_reserved_names_and_trailing_dots_are_neutralised() {
        assert_eq!(safe_name("con"), "_con");
        assert_eq!(safe_name("NUL.txt"), "_NUL.txt");
        assert_eq!(safe_name("dir\\Com1.png"), "_Com1.png");
        assert_eq!(safe_name("lpt9"), "_lpt9");
        assert_eq!(safe_name("console.txt"), "console.txt");
        assert_eq!(safe_name("photo."), "photo");
        assert_eq!(safe_name("..."), "attachment");
        assert_eq!(safe_name("report .  "), "report");
    }
}
