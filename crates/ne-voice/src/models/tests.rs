use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use super::*;

/// Serves `body` over loopback HTTP. Honours `Range: bytes=N-` unless
/// `ignore_range` is set. Records each Range header it sees.
struct TestServer {
    url: String,
    ranges: Arc<Mutex<Vec<String>>>,
    requests: Arc<AtomicUsize>,
}

fn serve(body: Vec<u8>, ignore_range: bool) -> TestServer {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind loopback");
    let url = format!("http://{}/model.bin", listener.local_addr().expect("addr"));
    let ranges = Arc::new(Mutex::new(Vec::new()));
    let requests = Arc::new(AtomicUsize::new(0));
    let (seen, count) = (Arc::clone(&ranges), Arc::clone(&requests));
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { return };
            let mut reader = BufReader::new(stream.try_clone().expect("clone"));
            let mut range_start = None;
            loop {
                let mut line = String::new();
                if reader.read_line(&mut line).unwrap_or(0) == 0 || line == "\r\n" {
                    break;
                }
                if let Some(value) = line.to_ascii_lowercase().strip_prefix("range: bytes=") {
                    seen.lock().expect("lock").push(value.trim().to_owned());
                    range_start = value.trim().trim_end_matches('-').parse::<usize>().ok();
                }
            }
            count.fetch_add(1, Ordering::SeqCst);
            let (status, payload, extra) = match range_start.filter(|_| !ignore_range) {
                Some(start) if start < body.len() => (
                    "206 Partial Content",
                    &body[start..],
                    format!(
                        "Content-Range: bytes {start}-{}/{}\r\n",
                        body.len() - 1,
                        body.len()
                    ),
                ),
                _ => ("200 OK", &body[..], String::new()),
            };
            let header = format!(
                "HTTP/1.1 {status}\r\nContent-Length: {}\r\n{extra}Connection: close\r\n\r\n",
                payload.len()
            );
            let _ = stream.write_all(header.as_bytes());
            let _ = stream.write_all(payload);
        }
    });
    TestServer {
        url,
        ranges,
        requests,
    }
}

fn payload(len: usize) -> Vec<u8> {
    (0..len).map(|i| (i * 31 % 251) as u8).collect()
}

fn hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("ne-voice-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("create temp dir");
    dir
}

fn source<'a>(server: &'a TestServer, body: &'a [u8], sha: &'a str) -> Source<'a> {
    Source {
        name: "test",
        url: &server.url,
        size_bytes: body.len() as u64,
        sha256: sha,
    }
}

#[test]
fn catalogue_entries_are_well_formed() {
    let mut ids: Vec<_> = CATALOG.iter().map(|s| s.id).collect();
    ids.sort_unstable();
    ids.dedup();
    assert_eq!(ids.len(), CATALOG.len());
    for spec in CATALOG {
        assert_eq!(spec.sha256.len(), 64, "{}", spec.id);
        assert!(spec.sha256.bytes().all(|b| b.is_ascii_hexdigit()));
        assert!(spec.url.starts_with("https://"));
        assert!(spec.url.ends_with(spec.file_name));
        assert!(spec.size_bytes > 0);
    }
    assert!(find_model(DEFAULT_MODEL_ID).is_some());
    assert!(find_model("nope").is_none());
}

#[test]
fn downloads_and_verifies_a_file() {
    let body = payload(300_000);
    let server = serve(body.clone(), false);
    let dir = temp_dir("full");
    let dest = dir.join("m.bin");
    let sha = hex(&body);

    let mut last = None;
    download_verified(
        &source(&server, &body, &sha),
        &dest,
        |p| last = Some(p),
        &CancelToken::new(),
    )
    .expect("download");

    assert_eq!(fs::read(&dest).expect("read"), body);
    assert!(!part_path(&dest).exists());
    assert_eq!(
        last,
        Some(Progress {
            downloaded: 300_000,
            total: 300_000
        })
    );
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn resumes_a_partial_download_with_a_range_request() {
    let body = payload(300_000);
    let server = serve(body.clone(), false);
    let dir = temp_dir("resume");
    let dest = dir.join("m.bin");
    fs::write(part_path(&dest), &body[..100_000]).expect("seed partial");

    let mut first = None;
    download_verified(
        &source(&server, &body, &hex(&body)),
        &dest,
        |p| {
            first.get_or_insert(p);
        },
        &CancelToken::new(),
    )
    .expect("resume");

    assert_eq!(fs::read(&dest).expect("read"), body);
    assert_eq!(server.ranges.lock().expect("lock").as_slice(), ["100000-"]);
    assert_eq!(first.map(|p| p.downloaded), Some(100_000));
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn restarts_when_the_server_ignores_the_range() {
    let body = payload(200_000);
    let server = serve(body.clone(), true);
    let dir = temp_dir("norange");
    let dest = dir.join("m.bin");
    fs::write(part_path(&dest), &body[..50_000]).expect("seed partial");

    download_verified(
        &source(&server, &body, &hex(&body)),
        &dest,
        |_| {},
        &CancelToken::new(),
    )
    .expect("download");

    assert_eq!(fs::read(&dest).expect("read"), body);
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn checksum_mismatch_is_rejected_and_leaves_nothing() {
    let body = payload(50_000);
    let server = serve(body.clone(), false);
    let dir = temp_dir("badsum");
    let dest = dir.join("m.bin");
    let wrong = "0".repeat(64);

    let error = download_verified(
        &source(&server, &body, &wrong),
        &dest,
        |_| {},
        &CancelToken::new(),
    )
    .expect_err("must fail");

    assert!(
        matches!(error, VoiceError::ChecksumMismatch { .. }),
        "{error}"
    );
    assert!(!dest.exists());
    assert!(!part_path(&dest).exists());
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn cancel_stops_the_download_and_keeps_the_partial_file() {
    let body = payload(400_000);
    let server = serve(body.clone(), false);
    let dir = temp_dir("cancel");
    let dest = dir.join("m.bin");
    let cancel = CancelToken::new();
    let trigger = cancel.clone();

    let error = download_verified(
        &source(&server, &body, &hex(&body)),
        &dest,
        |p| {
            if p.downloaded >= 64 * 1024 {
                trigger.cancel();
            }
        },
        &cancel,
    )
    .expect_err("must cancel");

    assert!(matches!(error, VoiceError::Cancelled));
    assert!(!dest.exists());
    let partial = fs::metadata(part_path(&dest)).expect("partial kept").len();
    assert!(partial > 0 && partial < 400_000);

    // A later attempt resumes from where it stopped.
    download_verified(
        &source(&server, &body, &hex(&body)),
        &dest,
        |_| {},
        &CancelToken::new(),
    )
    .expect("resume after cancel");
    assert_eq!(fs::read(&dest).expect("read"), body);
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn http_errors_are_reported() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let url = format!("http://{}/x", listener.local_addr().expect("addr"));
    std::thread::spawn(move || {
        if let Ok((mut s, _)) = listener.accept() {
            let mut buf = [0u8; 1024];
            let _ = std::io::Read::read(&mut s, &mut buf);
            let _ = s.write_all(
                b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
            );
        }
    });
    let dir = temp_dir("http404");
    let src = Source {
        name: "test",
        url: &url,
        size_bytes: 10,
        sha256: &"0".repeat(64),
    };
    let error = download_verified(&src, &dir.join("m.bin"), |_| {}, &CancelToken::new())
        .expect_err("must fail");
    assert!(
        matches!(&error, VoiceError::Download(m) if m.contains("404")),
        "{error}"
    );
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn unknown_model_ids_are_rejected() {
    let store = ModelStore::new(temp_dir("unknown"));
    assert!(matches!(
        store.download("nope", |_| {}, &CancelToken::new()),
        Err(VoiceError::UnknownModel(_))
    ));
    assert!(matches!(
        store.delete("nope"),
        Err(VoiceError::UnknownModel(_))
    ));
}

#[test]
fn lists_and_deletes_installed_models() {
    let dir = temp_dir("store");
    let store = ModelStore::new(&dir);
    let spec = &CATALOG[1];
    assert!(store.installed().is_empty());

    // A file of the wrong size is not an installed model.
    fs::write(store.path_for(spec), b"truncated").expect("write");
    assert!(!store.is_installed(spec));
    assert!(store.installed().is_empty());

    let file = File::create(store.path_for(spec)).expect("create");
    file.set_len(spec.size_bytes).expect("sparse file");
    assert!(store.is_installed(spec));
    let installed = store.installed();
    assert_eq!(installed.len(), 1);
    assert_eq!(installed[0].spec.id, spec.id);
    assert_eq!(installed[0].path, store.path_for(spec));

    // Installed models are returned without touching the network.
    let path = store
        .download(spec.id, |_| {}, &CancelToken::new())
        .expect("already installed");
    assert_eq!(path, store.path_for(spec));

    fs::write(part_path(&path), b"x").expect("write partial");
    store.delete(spec.id).expect("delete");
    assert!(!path.exists());
    assert!(!part_path(&path).exists());
    assert!(matches!(
        store.delete(spec.id),
        Err(VoiceError::ModelNotInstalled(_))
    ));
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn sha256_file_matches_a_known_digest() {
    let dir = temp_dir("sha");
    let path = dir.join("abc.txt");
    fs::write(&path, b"abc").expect("write");
    assert_eq!(
        sha256_file(&path).expect("hash"),
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn complete_partial_file_is_verified_without_a_request() {
    // A .part that is already complete must be verified without any request.
    let body = payload(10_000);
    let server = serve(body.clone(), false);
    let dir = temp_dir("complete");
    let dest = dir.join("m.bin");
    fs::write(part_path(&dest), &body).expect("seed");

    download_verified(
        &source(&server, &body, &hex(&body)),
        &dest,
        |_| {},
        &CancelToken::new(),
    )
    .expect("verify only");

    assert_eq!(server.requests.load(Ordering::SeqCst), 0);
    assert_eq!(fs::read(&dest).expect("read"), body);
    let _ = fs::remove_dir_all(dir);
}
