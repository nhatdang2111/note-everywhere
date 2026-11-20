//! Speech model catalogue, download and local storage.
//!
//! The caller decides where models live (`ModelStore::new(dir)`); this crate
//! never picks a path. Downloads are resumable, report progress, can be
//! cancelled, and are only kept after the SHA-256 matches the catalogue.

use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

use sha2::{Digest, Sha256};
use ureq::tls::{RootCerts, TlsConfig, TlsProvider};

use crate::cancel::CancelToken;
use crate::error::VoiceError;

/// A downloadable ggml whisper model.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ModelSpec {
    pub id: &'static str,
    pub label: &'static str,
    pub file_name: &'static str,
    pub url: &'static str,
    pub size_bytes: u64,
    /// Lowercase hex SHA-256 of the file.
    pub sha256: &'static str,
}

/// Quantised multilingual models from the official whisper.cpp repository.
pub const CATALOG: [ModelSpec; 2] = [
    ModelSpec {
        id: "large-v3-turbo-q5_0",
        label: "Whisper large-v3-turbo (q5_0), best accuracy",
        file_name: "ggml-large-v3-turbo-q5_0.bin",
        url: "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-large-v3-turbo-q5_0.bin",
        size_bytes: 574_041_195,
        sha256: "394221709cd5ad1f40c46e6031ca61bce88931e6e088c188294c6d5a55ffa7e2",
    },
    ModelSpec {
        id: "small-q5_1",
        label: "Whisper small (q5_1), fast but less accurate in Vietnamese",
        file_name: "ggml-small-q5_1.bin",
        url: "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-small-q5_1.bin",
        size_bytes: 190_085_487,
        sha256: "ae85e4a935d7a567bd102fe55afc16bb595bdb618e11b2fc7591bc08120411bb",
    },
];

/// Model offered first when the feature is switched on.
pub const DEFAULT_MODEL_ID: &str = "large-v3-turbo-q5_0";

pub fn find_model(id: &str) -> Option<&'static ModelSpec> {
    CATALOG.iter().find(|spec| spec.id == id)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Progress {
    pub downloaded: u64,
    pub total: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstalledModel {
    pub spec: ModelSpec,
    pub path: PathBuf,
}

/// Models stored in one directory chosen by the caller.
#[derive(Debug, Clone)]
pub struct ModelStore {
    dir: PathBuf,
}

impl ModelStore {
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self { dir: dir.into() }
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    pub fn path_for(&self, spec: &ModelSpec) -> PathBuf {
        self.dir.join(spec.file_name)
    }

    /// A model counts as installed when its file exists with the exact size;
    /// the checksum was verified when it was downloaded.
    pub fn is_installed(&self, spec: &ModelSpec) -> bool {
        fs::metadata(self.path_for(spec)).is_ok_and(|m| m.len() == spec.size_bytes)
    }

    pub fn installed(&self) -> Vec<InstalledModel> {
        CATALOG
            .iter()
            .filter(|spec| self.is_installed(spec))
            .map(|spec| InstalledModel {
                spec: *spec,
                path: self.path_for(spec),
            })
            .collect()
    }

    /// Removes a model and any unfinished download of it.
    pub fn delete(&self, id: &str) -> Result<(), VoiceError> {
        let spec = find_model(id).ok_or_else(|| VoiceError::UnknownModel(id.to_owned()))?;
        let path = self.path_for(spec);
        let mut removed = false;
        for target in [path.clone(), part_path(&path)] {
            match fs::remove_file(&target) {
                Ok(()) => removed = true,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => return Err(VoiceError::io(target, e)),
            }
        }
        if removed {
            Ok(())
        } else {
            Err(VoiceError::ModelNotInstalled(id.to_owned()))
        }
    }

    /// Downloads a catalogue model (resuming a previous attempt) and returns
    /// its path. Already-installed models return immediately.
    pub fn download(
        &self,
        id: &str,
        on_progress: impl FnMut(Progress),
        cancel: &CancelToken,
    ) -> Result<PathBuf, VoiceError> {
        let spec = find_model(id).ok_or_else(|| VoiceError::UnknownModel(id.to_owned()))?;
        let path = self.path_for(spec);
        if self.is_installed(spec) {
            return Ok(path);
        }
        fs::create_dir_all(&self.dir).map_err(|e| VoiceError::io(&self.dir, e))?;
        download_verified(
            &Source {
                name: spec.id,
                url: spec.url,
                size_bytes: spec.size_bytes,
                sha256: spec.sha256,
            },
            &path,
            on_progress,
            cancel,
        )?;
        Ok(path)
    }
}

struct Source<'a> {
    name: &'a str,
    url: &'a str,
    size_bytes: u64,
    sha256: &'a str,
}

fn part_path(path: &Path) -> PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push(".part");
    PathBuf::from(name)
}

fn agent() -> ureq::Agent {
    let tls = TlsConfig::builder()
        .provider(TlsProvider::NativeTls)
        .root_certs(RootCerts::PlatformVerifier)
        .build();
    ureq::Agent::new_with_config(
        ureq::Agent::config_builder()
            .tls_config(tls)
            .http_status_as_error(false)
            .timeout_connect(Some(Duration::from_secs(20)))
            .timeout_recv_response(Some(Duration::from_secs(30)))
            .build(),
    )
}

fn download_verified(
    source: &Source<'_>,
    dest: &Path,
    mut on_progress: impl FnMut(Progress),
    cancel: &CancelToken,
) -> Result<(), VoiceError> {
    let part = part_path(dest);
    let mut resume_from = fs::metadata(&part).map_or(0, |m| m.len());
    if resume_from > source.size_bytes {
        discard(&part)?;
        resume_from = 0;
    }

    if resume_from < source.size_bytes {
        stream_into(source, &part, resume_from, &mut on_progress, cancel)?;
    }

    let actual = sha256_file(&part)?;
    if actual != source.sha256 {
        discard(&part)?;
        return Err(VoiceError::ChecksumMismatch {
            name: source.name.to_owned(),
            expected: source.sha256.to_owned(),
            actual,
        });
    }
    fs::rename(&part, dest).map_err(|e| VoiceError::io(dest, e))
}

/// Appends the remainder of the file to `part`, restarting from zero when the
/// server does not honour the range request.
fn stream_into(
    source: &Source<'_>,
    part: &Path,
    resume_from: u64,
    on_progress: &mut impl FnMut(Progress),
    cancel: &CancelToken,
) -> Result<(), VoiceError> {
    let mut request = agent().get(source.url);
    if resume_from > 0 {
        request = request.header("Range", format!("bytes={resume_from}-"));
    }
    let mut response = request
        .call()
        .map_err(|e| VoiceError::Download(e.to_string()))?;

    let status = response.status().as_u16();
    let mut downloaded = match status {
        206 => resume_from,
        200 => 0,
        416 => {
            // The partial file does not fit the remote file; start over.
            discard(part)?;
            return stream_into(source, part, 0, on_progress, cancel);
        }
        other => {
            return Err(VoiceError::Download(format!(
                "server answered HTTP {other}"
            )));
        }
    };

    let mut file = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(downloaded == 0)
        .append(downloaded > 0)
        .open(part)
        .map_err(|e| VoiceError::io(part, e))?;

    let total = source.size_bytes;
    on_progress(Progress { downloaded, total });
    let mut reader = response.body_mut().as_reader();
    let mut chunk = vec![0u8; 64 * 1024];
    loop {
        if cancel.is_cancelled() {
            return Err(VoiceError::Cancelled);
        }
        let read = reader
            .read(&mut chunk)
            .map_err(|e| VoiceError::Download(e.to_string()))?;
        if read == 0 {
            break;
        }
        file.write_all(&chunk[..read])
            .map_err(|e| VoiceError::io(part, e))?;
        downloaded += read as u64;
        if downloaded > total {
            drop(file);
            discard(part)?;
            return Err(VoiceError::Download(
                "server sent more data than the catalogue size".into(),
            ));
        }
        on_progress(Progress { downloaded, total });
    }
    file.flush().map_err(|e| VoiceError::io(part, e))?;
    if downloaded != total {
        // Keep the partial file so the next attempt resumes.
        return Err(VoiceError::Download(format!(
            "connection ended after {downloaded} of {total} bytes"
        )));
    }
    Ok(())
}

fn discard(path: &Path) -> Result<(), VoiceError> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(VoiceError::io(path, e)),
    }
}

/// Lowercase hex SHA-256 of a file, read in chunks.
pub fn sha256_file(path: &Path) -> Result<String, VoiceError> {
    let mut file = File::open(path).map_err(|e| VoiceError::io(path, e))?;
    let mut hasher = Sha256::new();
    let mut chunk = vec![0u8; 1024 * 1024];
    loop {
        let read = file.read(&mut chunk).map_err(|e| VoiceError::io(path, e))?;
        if read == 0 {
            break;
        }
        hasher.update(&chunk[..read]);
    }
    Ok(hasher
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect())
}

#[cfg(test)]
mod tests;
