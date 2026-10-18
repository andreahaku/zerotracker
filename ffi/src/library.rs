//! C ABI and safe filesystem/network operations for downloadable content packs.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashSet};
use std::ffi::{CStr, c_char};
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread::{self, JoinHandle};
use std::time::UNIX_EPOCH;
use zip::ZipArchive;

const PART_BUFFER_SIZE: usize = 32 * 1024;
const MAX_CATALOG_BYTES: u64 = 4 * 1024 * 1024;
const MAX_PACK_FILES: usize = 32_768;
const DOWNLOAD_RETRY_DELAYS_SECS: [u64; 3] = [2, 4, 8];
const RETRY_SLEEP_SLICE_MS: u64 = 100;

#[repr(C)]
#[derive(Clone, Copy)]
pub struct PackInfo {
    pub id: [c_char; 64],
    pub name: [c_char; 96],
    pub kind: [c_char; 16],
    pub authors: [c_char; 128],
    pub licence: [c_char; 32],
    pub licence_url: [c_char; 256],
    pub description: [c_char; 256],
    pub bytes: u64,
    pub songs: u32,
    pub instruments: u32,
    pub samples: u32,
    pub installed: u8,
}

impl Default for PackInfo {
    fn default() -> Self {
        Self {
            id: [0; 64],
            name: [0; 96],
            kind: [0; 16],
            authors: [0; 128],
            licence: [0; 32],
            licence_url: [0; 256],
            description: [0; 256],
            bytes: 0,
            songs: 0,
            instruments: 0,
            samples: 0,
            installed: 0,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct Catalog {
    version: u32,
    packs: Vec<Pack>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct Pack {
    id: String,
    name: String,
    kind: String,
    description: String,
    authors: String,
    licence: String,
    licence_url: String,
    #[serde(default)]
    source: String,
    #[serde(default)]
    prefix: Option<String>,
    #[serde(default)]
    files: Vec<PackFile>,
    #[serde(default)]
    url: String,
    #[serde(default)]
    bytes: u64,
    #[serde(default)]
    unpacked_bytes: u64,
    #[serde(default)]
    sha256: String,
    #[serde(default)]
    songs: u32,
    #[serde(default)]
    instruments: u32,
    #[serde(default)]
    samples: u32,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct PackFile {
    url: String,
    path: String,
    bytes: u64,
    sha256: String,
}

impl Pack {
    fn source(&self) -> &str {
        if self.source.is_empty() {
            "zip"
        } else {
            &self.source
        }
    }

    fn download_bytes(&self) -> u64 {
        if self.source() == "files" {
            self.files
                .iter()
                .fold(0_u64, |total, file| total.saturating_add(file.bytes))
        } else {
            self.bytes
        }
    }

    fn install_bytes(&self) -> u64 {
        if self.source() == "files" {
            self.download_bytes()
        } else {
            self.unpacked_bytes
        }
    }
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
struct InstallDb {
    #[serde(default)]
    packs: BTreeMap<String, InstalledPack>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
struct InstalledPack {
    files: Vec<InstalledFile>,
    #[serde(default)]
    directories: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct InstalledFile {
    path: String,
    size: u64,
    modified_ns: u128,
}

#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Status {
    Idle = 0,
    Fetching = 1,
    Downloading = 2,
    Installing = 3,
    Done = 4,
    Failed = 5,
    Cancelled = 6,
}

struct Shared {
    catalog: Catalog,
    installed: InstallDb,
    status: Status,
    permille: u32,
    message: String,
}

#[repr(C)]
pub struct Library {
    root: PathBuf,
    catalog_url: String,
    agent: ureq::Agent,
    shared: Arc<Mutex<Shared>>,
    worker: Mutex<Option<JoinHandle<()>>>,
    cancel: Arc<AtomicBool>,
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn packs_dir(root: &Path) -> PathBuf {
    root.join(".packs")
}

fn cache_catalog_path(root: &Path) -> PathBuf {
    packs_dir(root).join("catalog.json")
}

fn installed_db_path(root: &Path) -> PathBuf {
    packs_dir(root).join("installed.json")
}

fn safe_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 63
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_'))
}

fn valid_url(url: &str) -> bool {
    url.starts_with("https://") || url.starts_with("http://")
}

fn validate_catalog(catalog: &Catalog) -> Result<(), String> {
    if catalog.version != 1 {
        return Err(format!("Unsupported catalog version {}", catalog.version));
    }
    let mut ids = HashSet::new();
    for pack in &catalog.packs {
        if !safe_id(&pack.id) || !ids.insert(pack.id.as_str()) {
            return Err("Catalog contains an invalid or duplicate pack id".into());
        }
        if !valid_url(&pack.licence_url) {
            return Err(format!("Invalid licence URL in pack {}", pack.id));
        }
        match pack.source() {
            "zip" => {
                if !pack.files.is_empty() || !valid_url(&pack.url) {
                    return Err(format!("Invalid ZIP source in pack {}", pack.id));
                }
                if pack.bytes == 0 || pack.unpacked_bytes == 0 {
                    return Err(format!("Invalid size in pack {}", pack.id));
                }
                if !valid_sha256(&pack.sha256) {
                    return Err(format!("Invalid SHA-256 in pack {}", pack.id));
                }
                if let Some(prefix) = &pack.prefix {
                    checked_relative_path(prefix)?;
                }
            }
            "files" => {
                if pack.files.is_empty()
                    || pack.files.len() > MAX_PACK_FILES
                    || pack.prefix.is_some()
                {
                    return Err(format!("Invalid files source in pack {}", pack.id));
                }
                let mut paths = HashSet::new();
                let mut total = 0_u64;
                for file in &pack.files {
                    if !valid_url(&file.url) || file.bytes == 0 || !valid_sha256(&file.sha256) {
                        return Err(format!("Invalid file metadata in pack {}", pack.id));
                    }
                    let path = checked_relative_path(&file.path)?;
                    if !paths.insert(path) {
                        return Err(format!("Duplicate file path in pack {}", pack.id));
                    }
                    total = total
                        .checked_add(file.bytes)
                        .ok_or_else(|| format!("File sizes overflow in pack {}", pack.id))?;
                }
                if (pack.bytes != 0 && pack.bytes != total)
                    || (pack.unpacked_bytes != 0 && pack.unpacked_bytes != total)
                {
                    return Err(format!("Invalid aggregate size in pack {}", pack.id));
                }
            }
            _ => return Err(format!("Unknown source type in pack {}", pack.id)),
        }
        if pack.id.len() >= 64
            || pack.name.len() >= 96
            || pack.kind.len() >= 16
            || pack.authors.len() >= 128
            || pack.licence.len() >= 32
            || pack.licence_url.len() >= 256
            || pack.description.len() >= 256
            || [
                pack.id.as_str(),
                pack.name.as_str(),
                pack.kind.as_str(),
                pack.authors.as_str(),
                pack.licence.as_str(),
                pack.licence_url.as_str(),
                pack.description.as_str(),
            ]
            .iter()
            .any(|value| value.contains('\0'))
        {
            return Err(format!("Pack metadata is too long for {}", pack.id));
        }
    }
    Ok(())
}

fn valid_sha256(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|b| b.is_ascii_hexdigit())
}

fn load_catalog(path: &Path) -> Option<Catalog> {
    let bytes = fs::read(path).ok()?;
    if bytes.len() as u64 > MAX_CATALOG_BYTES {
        return None;
    }
    let catalog: Catalog = serde_json::from_slice(&bytes).ok()?;
    validate_catalog(&catalog).ok()?;
    Some(catalog)
}

fn load_install_db(root: &Path) -> InstallDb {
    fs::read(installed_db_path(root))
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default()
}

fn atomic_write(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let parent = path.parent().ok_or("Missing parent directory")?;
    fs::create_dir_all(parent).map_err(|e| format!("Create directory: {e}"))?;
    let temp = path.with_extension(format!("tmp-{}", std::process::id()));
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temp)
        .or_else(|_| {
            let _ = fs::remove_file(&temp);
            OpenOptions::new().write(true).create_new(true).open(&temp)
        })
        .map_err(|e| format!("Create temporary file: {e}"))?;
    let result = file
        .write_all(bytes)
        .and_then(|()| file.sync_all())
        .map_err(|e| format!("Write temporary file: {e}"));
    drop(file);
    result?;
    fs::rename(&temp, path).map_err(|e| {
        let _ = fs::remove_file(&temp);
        format!("Replace {}: {e}", path.display())
    })
}

fn save_install_db(root: &Path, db: &InstallDb) -> Result<(), String> {
    let bytes = serde_json::to_vec_pretty(db).map_err(|e| format!("Encode install list: {e}"))?;
    atomic_write(&installed_db_path(root), &bytes)
}

fn set_state(
    shared: &Arc<Mutex<Shared>>,
    status: Status,
    progress: u32,
    message: impl Into<String>,
) {
    let mut state = lock(shared);
    state.status = status;
    state.permille = progress.min(1000);
    state.message = message.into();
}

fn finish_worker(shared: &Arc<Mutex<Shared>>, result: Result<String, String>, cancel: &AtomicBool) {
    if cancel.load(Ordering::Relaxed) && result.is_err() {
        set_state(shared, Status::Cancelled, 0, "Cancelled");
        return;
    }
    match result {
        Ok(message) => set_state(shared, Status::Done, 1000, message),
        Err(message) => set_state(shared, Status::Failed, 0, message),
    }
}

fn start_job<F>(lib: &Library, initial: Status, task: F) -> bool
where
    F: FnOnce(Arc<Mutex<Shared>>, Arc<AtomicBool>, PathBuf) + Send + 'static,
{
    let mut worker = lock(&lib.worker);
    if worker.as_ref().is_some_and(|handle| !handle.is_finished()) {
        return false;
    }
    if let Some(handle) = worker.take() {
        let _ = handle.join();
    }
    lib.cancel.store(false, Ordering::Relaxed);
    set_state(&lib.shared, initial, 0, "Starting");
    let shared = Arc::clone(&lib.shared);
    let cancel = Arc::clone(&lib.cancel);
    let root = lib.root.clone();
    *worker = Some(thread::spawn(move || task(shared, cancel, root)));
    true
}

fn update_progress(
    shared: &Arc<Mutex<Shared>>,
    status: Status,
    done: u64,
    total: u64,
    message: &str,
) {
    let progress = if total == 0 {
        0
    } else {
        ((done.min(total) as u128 * 1000) / total as u128) as u32
    };
    set_state(shared, status, progress, message);
}

fn fetch_catalog(
    root: &Path,
    agent: &ureq::Agent,
    url: &str,
    cancel: &AtomicBool,
) -> Result<Catalog, String> {
    if cancel.load(Ordering::Relaxed) {
        return Err("Cancelled".into());
    }
    let response = agent
        .get(url)
        .call()
        .map_err(|e| format!("Catalog request failed: {e}"))?;
    if !(200..300).contains(&response.status()) {
        return Err(format!(
            "Catalog request returned HTTP {}",
            response.status()
        ));
    }
    let mut bytes = Vec::new();
    response
        .into_reader()
        .take(MAX_CATALOG_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| format!("Read catalog: {e}"))?;
    if bytes.len() as u64 > MAX_CATALOG_BYTES {
        return Err("Catalog is too large".into());
    }
    let catalog: Catalog =
        serde_json::from_slice(&bytes).map_err(|e| format!("Invalid catalog: {e}"))?;
    validate_catalog(&catalog)?;
    atomic_write(&cache_catalog_path(root), &bytes)?;
    Ok(catalog)
}

fn hash_file(path: &Path, cancel: &AtomicBool) -> Result<String, String> {
    let mut file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW)
        .open(path)
        .map_err(|e| format!("Open downloaded pack: {e}"))?;
    let mut hash = Sha256::new();
    let mut buffer = [0; PART_BUFFER_SIZE];
    loop {
        if cancel.load(Ordering::Relaxed) {
            return Err("Cancelled".into());
        }
        let read = file
            .read(&mut buffer)
            .map_err(|e| format!("Read downloaded pack: {e}"))?;
        if read == 0 {
            break;
        }
        hash.update(&buffer[..read]);
    }
    Ok(format!("{:x}", hash.finalize()))
}

fn partial_file_len(path: &Path) -> Result<Option<u64>, String> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_file() => Err(format!(
            "Refusing unsafe partial download {}",
            path.display()
        )),
        Ok(metadata) => Ok(Some(metadata.len())),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(format!("Inspect partial download: {error}")),
    }
}

fn download_label_from_url(url: &str) -> &str {
    url.split(['?', '#'])
        .next()
        .unwrap_or(url)
        .rsplit('/')
        .next()
        .filter(|name| !name.is_empty())
        .unwrap_or("pack archive")
}

fn download_pack(
    root: &Path,
    agent: &ureq::Agent,
    pack: &Pack,
    shared: &Arc<Mutex<Shared>>,
    cancel: &AtomicBool,
) -> Result<PathBuf, String> {
    let label = download_label_from_url(&pack.url);
    retry_download(label, cancel, || {
        download_pack_attempt(root, agent, pack, shared, cancel)
    })
}

fn download_pack_attempt(
    root: &Path,
    agent: &ureq::Agent,
    pack: &Pack,
    shared: &Arc<Mutex<Shared>>,
    cancel: &AtomicBool,
) -> Result<PathBuf, DownloadError> {
    if cancel.load(Ordering::Relaxed) {
        return Err("Cancelled".into());
    }
    if !safe_id(&pack.id) || !valid_url(&pack.url) {
        return Err("Invalid pack id or URL".into());
    }
    let dir = packs_dir(root).join("download");
    fs::create_dir_all(&dir).map_err(|e| format!("Create download directory: {e}"))?;
    let part = dir.join(format!("{}.zip.part", pack.id));

    let mut offset = partial_file_len(&part)?.unwrap_or(0);
    if offset > pack.bytes {
        fs::remove_file(&part).map_err(|e| format!("Discard oversized partial: {e}"))?;
        offset = 0;
    }
    ensure_download_space(root, pack, offset)?;
    if offset == pack.bytes && offset != 0 {
        let hash = hash_file(&part, cancel)?;
        if hash.eq_ignore_ascii_case(&pack.sha256) {
            return Ok(part);
        }
        fs::remove_file(&part).map_err(|e| format!("Discard invalid partial: {e}"))?;
        offset = 0;
        ensure_download_space(root, pack, offset)?;
    }

    let mut request = agent.get(&pack.url);
    if offset > 0 {
        request = request.set("Range", &format!("bytes={offset}-"));
    }
    let response = match call_download_request(request) {
        Ok(response) => response,
        Err(DownloadError::HttpStatus(416)) if offset > 0 => {
            fs::remove_file(&part).map_err(|e| format!("Restart partial download: {e}"))?;
            offset = 0;
            ensure_download_space(root, pack, offset)?;
            call_download_request(agent.get(&pack.url))?
        }
        Err(error) => return Err(error),
    };
    let status = response.status();
    let append = offset > 0 && status == 206;
    if append {
        let content_range = response.header("Content-Range").unwrap_or_default();
        if !content_range.starts_with(&format!("bytes {offset}-")) {
            return Err("Server returned an invalid Content-Range".into());
        }
    } else if status != 200 {
        return Err(DownloadError::HttpStatus(status));
    }
    if !append && offset > 0 {
        let required = pack.bytes.saturating_add(pack.unpacked_bytes);
        if available_bytes(root).saturating_add(offset) < required {
            return Err("Not enough free space to restart this pack download".into());
        }
    }
    let mut file = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(!append)
        .append(append)
        .custom_flags(libc::O_NOFOLLOW)
        .open(&part)
        .map_err(|e| format!("Open partial download: {e}"))?;
    let mut reader = response.into_reader();
    let mut buffer = [0; PART_BUFFER_SIZE];
    let mut done = if append { offset } else { 0 };
    loop {
        if cancel.load(Ordering::Relaxed) {
            let _ = file.flush();
            return Err("Cancelled".into());
        }
        let read = reader
            .read(&mut buffer)
            .map_err(|_| DownloadError::Retryable("Network interruption during download".into()))?;
        if read == 0 {
            break;
        }
        done = done.saturating_add(read as u64);
        if done > pack.bytes {
            return Err("Downloaded more bytes than the catalog declared".into());
        }
        file.write_all(&buffer[..read])
            .map_err(|e| format!("Write partial download: {e}"))?;
        update_progress(shared, Status::Downloading, done, pack.bytes, "Downloading");
    }
    file.flush()
        .map_err(|e| format!("Flush partial download: {e}"))?;
    file.sync_all()
        .map_err(|e| format!("Sync partial download: {e}"))?;
    drop(file);
    let actual = fs::metadata(&part)
        .map_err(|e| format!("Check download size: {e}"))?
        .len();
    if actual != pack.bytes {
        return Err(DownloadError::Retryable(format!(
            "Incomplete download ({actual}/{} bytes)",
            pack.bytes
        )));
    }
    update_progress(
        shared,
        Status::Installing,
        0,
        pack.unpacked_bytes,
        "Checking SHA-256",
    );
    let hash = hash_file(&part, cancel)?;
    if !hash.eq_ignore_ascii_case(&pack.sha256) {
        let _ = fs::remove_file(&part);
        return Err("SHA-256 mismatch; partial download discarded".into());
    }
    Ok(part)
}

fn file_part_path(root: &Path, pack_id: &str, file_path: &str) -> Result<PathBuf, String> {
    if !safe_id(pack_id) {
        return Err("Invalid pack id".into());
    }
    let relative = checked_relative_path(file_path)?;
    let mut partial = PathBuf::from(".packs/download");
    partial.push(pack_id);
    partial.push(format!("{}.part", relative.to_string_lossy()));
    let partial = checked_relative_path(&partial.to_string_lossy())?;
    ensure_no_symlink_ancestors(root, &partial)?;
    Ok(root.join(partial))
}

fn ensure_files_download_space(
    root: &Path,
    pack: &Pack,
    staged_bytes: u64,
    staged_files: &HashSet<String>,
) -> Result<(), String> {
    let mut partial_bytes = 0_u64;
    for file in &pack.files {
        if staged_files.contains(&file.path) {
            continue;
        }
        let path = file_part_path(root, &pack.id, &file.path)?;
        match partial_file_len(&path)? {
            Some(size) if size <= file.bytes => {
                partial_bytes = partial_bytes.saturating_add(size);
            }
            Some(_) => {
                fs::remove_file(&path).map_err(|e| format!("Discard oversized partial: {e}"))?;
            }
            None => {}
        }
    }
    let required = pack
        .download_bytes()
        .saturating_sub(staged_bytes.min(pack.download_bytes()))
        .saturating_sub(partial_bytes.min(pack.download_bytes()));
    if available_bytes(root) < required {
        return Err("Not enough free space to download and install this pack".into());
    }
    Ok(())
}

#[derive(Clone, Copy)]
struct FileDownloadContext<'a> {
    root: &'a Path,
    agent: &'a ureq::Agent,
    pack: &'a Pack,
    staged_bytes: u64,
    staged_files: &'a HashSet<String>,
    progress_before: u64,
    shared: &'a Arc<Mutex<Shared>>,
    cancel: &'a AtomicBool,
}

enum DownloadError {
    Retryable(String),
    Fatal(String),
    HttpStatus(u16),
}

impl From<String> for DownloadError {
    fn from(error: String) -> Self {
        Self::Fatal(error)
    }
}

impl From<&str> for DownloadError {
    fn from(error: &str) -> Self {
        Self::Fatal(error.to_owned())
    }
}

impl DownloadError {
    fn retryable(&self) -> bool {
        matches!(self, Self::Retryable(_) | Self::HttpStatus(429 | 500..=599))
    }

    fn message(self, label: &str) -> String {
        match self {
            Self::HttpStatus(status) => format!("HTTP {status} on {label}"),
            Self::Retryable(message) => format!("{message} on {label}"),
            Self::Fatal(message) => message,
        }
    }
}

fn retry_download<T>(
    label: &str,
    cancel: &AtomicBool,
    mut attempt: impl FnMut() -> Result<T, DownloadError>,
) -> Result<T, String> {
    for delay_secs in DOWNLOAD_RETRY_DELAYS_SECS
        .iter()
        .copied()
        .chain(std::iter::once(0))
    {
        if cancel.load(Ordering::Relaxed) {
            return Err("Cancelled".into());
        }
        match attempt() {
            Ok(value) => return Ok(value),
            Err(error) if error.retryable() && delay_secs > 0 => {
                let delay_ms = delay_secs * 1000;
                let mut waited_ms = 0;
                while waited_ms < delay_ms {
                    if cancel.load(Ordering::Relaxed) {
                        return Err("Cancelled".into());
                    }
                    let slice = RETRY_SLEEP_SLICE_MS.min(delay_ms - waited_ms);
                    thread::sleep(std::time::Duration::from_millis(slice));
                    waited_ms += slice;
                }
                let _ = error;
            }
            Err(error) => return Err(error.message(label)),
        }
    }
    unreachable!("retry loop always returns")
}

fn call_download_request(request: ureq::Request) -> Result<ureq::Response, DownloadError> {
    request.call().map_err(|error| match error {
        ureq::Error::Status(status, _) => DownloadError::HttpStatus(status),
        ureq::Error::Transport(_) => DownloadError::Retryable("Network error".into()),
    })
}

fn download_file(context: FileDownloadContext<'_>, file: &PackFile) -> Result<PathBuf, String> {
    retry_download(&file.path, context.cancel, || {
        download_file_attempt(
            FileDownloadContext {
                root: context.root,
                agent: context.agent,
                pack: context.pack,
                staged_bytes: context.staged_bytes,
                staged_files: context.staged_files,
                progress_before: context.progress_before,
                shared: context.shared,
                cancel: context.cancel,
            },
            file,
        )
    })
}

fn download_file_attempt(
    context: FileDownloadContext<'_>,
    file: &PackFile,
) -> Result<PathBuf, DownloadError> {
    let FileDownloadContext {
        root,
        agent,
        pack,
        staged_bytes,
        staged_files,
        progress_before,
        shared,
        cancel,
    } = context;
    if cancel.load(Ordering::Relaxed) {
        return Err("Cancelled".into());
    }
    if !valid_url(&file.url) || !valid_sha256(&file.sha256) || file.bytes == 0 {
        return Err("Invalid file URL, size, or SHA-256".into());
    }
    let part = file_part_path(root, &pack.id, &file.path)?;
    let parent = part.parent().ok_or("Missing partial download parent")?;
    fs::create_dir_all(parent).map_err(|e| format!("Create partial download directory: {e}"))?;

    let mut offset = partial_file_len(&part)?.unwrap_or(0);
    if offset > file.bytes {
        fs::remove_file(&part).map_err(|e| format!("Discard oversized partial: {e}"))?;
        offset = 0;
    }
    ensure_files_download_space(root, pack, staged_bytes, staged_files)?;
    if offset == file.bytes && offset != 0 {
        let hash = hash_file(&part, cancel)?;
        if hash.eq_ignore_ascii_case(&file.sha256) {
            return Ok(part);
        }
        fs::remove_file(&part).map_err(|e| format!("Discard invalid partial: {e}"))?;
        offset = 0;
        ensure_files_download_space(root, pack, staged_bytes, staged_files)?;
    }

    let mut request = agent.get(&file.url);
    if offset > 0 {
        request = request.set("Range", &format!("bytes={offset}-"));
    }
    let response = match call_download_request(request) {
        Ok(response) => response,
        Err(DownloadError::HttpStatus(416)) if offset > 0 => {
            fs::remove_file(&part).map_err(|e| format!("Restart partial download: {e}"))?;
            offset = 0;
            ensure_files_download_space(root, pack, staged_bytes, staged_files)?;
            call_download_request(agent.get(&file.url))?
        }
        Err(error) => return Err(error),
    };
    let append = offset > 0 && response.status() == 206;
    if append {
        if !response
            .header("Content-Range")
            .unwrap_or_default()
            .starts_with(&format!("bytes {offset}-"))
        {
            return Err("Server returned an invalid Content-Range".into());
        }
    } else if response.status() != 200 {
        return Err(DownloadError::HttpStatus(response.status()));
    }
    if !append && offset > 0 {
        fs::remove_file(&part).map_err(|e| format!("Restart partial download: {e}"))?;
        offset = 0;
        ensure_files_download_space(root, pack, staged_bytes, staged_files)?;
    }

    let mut output = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(!append)
        .append(append)
        .custom_flags(libc::O_NOFOLLOW)
        .open(&part)
        .map_err(|e| format!("Open file partial: {e}"))?;
    let mut input = response.into_reader();
    let mut done = if append { offset } else { 0 };
    let mut buffer = [0; PART_BUFFER_SIZE];
    loop {
        if cancel.load(Ordering::Relaxed) {
            let _ = output.flush();
            return Err("Cancelled".into());
        }
        let read = input
            .read(&mut buffer)
            .map_err(|_| DownloadError::Retryable("Network interruption during download".into()))?;
        if read == 0 {
            break;
        }
        done = done.saturating_add(read as u64);
        if done > file.bytes {
            return Err("Downloaded more bytes than the catalog declared".into());
        }
        output
            .write_all(&buffer[..read])
            .map_err(|e| format!("Write file partial: {e}"))?;
        update_progress(
            shared,
            Status::Downloading,
            progress_before.saturating_add(done),
            pack.download_bytes(),
            &format!("Downloading {}", file.path),
        );
    }
    output
        .flush()
        .map_err(|e| format!("Flush file partial: {e}"))?;
    output
        .sync_all()
        .map_err(|e| format!("Sync file partial: {e}"))?;
    drop(output);
    let actual = fs::metadata(&part)
        .map_err(|e| format!("Check file download size: {e}"))?
        .len();
    if actual != file.bytes {
        return Err(DownloadError::Retryable(format!(
            "Incomplete file download ({actual}/{} bytes)",
            file.bytes
        )));
    }
    let hash = hash_file(&part, cancel)?;
    if !hash.eq_ignore_ascii_case(&file.sha256) {
        let _ = fs::remove_file(&part);
        return Err(format!("SHA-256 mismatch for {}; partial discarded", file.path).into());
    }
    Ok(part)
}

fn checked_relative_path(name: &str) -> Result<PathBuf, String> {
    if name.is_empty() || name.contains('\\') || name.as_bytes().contains(&0) {
        return Err("Unsafe archive path".into());
    }
    let bytes = name.as_bytes();
    if bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' {
        return Err(format!("Unsafe archive path: {name}"));
    }
    let path = Path::new(name);
    if path.is_absolute()
        || path
            .components()
            .any(|part| !matches!(part, Component::Normal(_)))
    {
        return Err(format!("Unsafe archive path: {name}"));
    }
    Ok(path.to_path_buf())
}

fn prefixed_path(prefix: Option<&str>, relative: PathBuf) -> Result<PathBuf, String> {
    let Some(prefix) = prefix else {
        return Ok(relative);
    };
    let mut path = checked_relative_path(prefix.trim_end_matches('/'))?;
    path.push(relative);
    if path
        .components()
        .any(|part| !matches!(part, Component::Normal(_)))
    {
        return Err("Unsafe prefixed archive path".into());
    }
    Ok(path)
}

fn modified_ns(metadata: &fs::Metadata) -> Result<u128, String> {
    metadata
        .modified()
        .map_err(|e| format!("Read file modification time: {e}"))?
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .map_err(|_| "File modification time predates Unix epoch".into())
}

fn ensure_no_symlink_ancestors(root: &Path, relative: &Path) -> Result<(), String> {
    let mut current = root.to_path_buf();
    for component in relative.parent().into_iter().flat_map(Path::components) {
        let Component::Normal(part) = component else {
            return Err("Unsafe destination path".into());
        };
        current.push(part);
        match fs::symlink_metadata(&current) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                return Err(format!("Refusing symlink directory {}", current.display()));
            }
            Ok(metadata) if !metadata.is_dir() => {
                return Err(format!(
                    "Destination parent is not a directory: {}",
                    current.display()
                ));
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(format!("Inspect destination: {error}")),
        }
    }
    Ok(())
}

fn available_bytes(path: &Path) -> u64 {
    use std::os::unix::ffi::OsStrExt;
    let Ok(path) = std::ffi::CString::new(path.as_os_str().as_bytes()) else {
        return 0;
    };
    // SAFETY: `path` is NUL-terminated and `info` is writable.
    unsafe {
        let mut info: libc::statvfs = std::mem::zeroed();
        if libc::statvfs(path.as_ptr(), &mut info) != 0 {
            return 0;
        }
        (info.f_bavail as u64).saturating_mul(info.f_frsize as u64)
    }
}

fn ensure_download_space(root: &Path, pack: &Pack, partial_bytes: u64) -> Result<(), String> {
    let required = pack
        .bytes
        .saturating_add(pack.install_bytes())
        .saturating_sub(partial_bytes.min(pack.bytes));
    if available_bytes(root) < required {
        return Err("Not enough free space to download and install this pack".into());
    }
    Ok(())
}

fn install_archive(
    root: &Path,
    pack: &Pack,
    archive_path: &Path,
    shared: &Arc<Mutex<Shared>>,
    cancel: &AtomicBool,
) -> Result<String, String> {
    if !safe_id(&pack.id) {
        return Err("Invalid pack id".into());
    }
    if available_bytes(root) < pack.unpacked_bytes {
        return Err("Not enough free space for this pack".into());
    }
    let packs = packs_dir(root);
    fs::create_dir_all(packs.join("tmp"))
        .map_err(|e| format!("Create temporary directory: {e}"))?;
    let temp_root = packs.join("tmp").join(&pack.id);
    if fs::symlink_metadata(&temp_root).is_ok() {
        if fs::symlink_metadata(&temp_root)
            .map_err(|e| format!("Inspect temporary directory: {e}"))?
            .file_type()
            .is_symlink()
        {
            return Err("Refusing symlink temporary directory".into());
        }
        fs::remove_dir_all(&temp_root).map_err(|e| format!("Clear previous extraction: {e}"))?;
    }
    fs::create_dir(&temp_root).map_err(|e| format!("Create pack extraction directory: {e}"))?;

    let extract_result = (|| -> Result<(Vec<PathBuf>, Vec<PathBuf>), String> {
        let file = File::open(archive_path).map_err(|e| format!("Open pack archive: {e}"))?;
        let mut archive = ZipArchive::new(file).map_err(|e| format!("Invalid ZIP archive: {e}"))?;
        if archive.len() > MAX_PACK_FILES {
            return Err("Pack contains too many entries".into());
        }
        let mut files = Vec::new();
        let mut dirs = HashSet::new();
        let mut seen = HashSet::new();
        let mut total = 0_u64;
        let entry_count = archive.len();
        for index in 0..entry_count {
            if cancel.load(Ordering::Relaxed) {
                return Err("Cancelled".into());
            }
            let mut entry = archive
                .by_index(index)
                .map_err(|e| format!("Read ZIP entry: {e}"))?;
            if entry
                .unix_mode()
                .is_some_and(|mode| mode & 0o170000 == 0o120000)
            {
                return Err(format!("Symbolic links are not allowed: {}", entry.name()));
            }
            if entry.is_dir() {
                let rel = checked_relative_path(entry.name().trim_end_matches('/'))?;
                let rel = prefixed_path(pack.prefix.as_deref(), rel)?;
                let path = temp_root.join(rel);
                fs::create_dir_all(&path)
                    .map_err(|e| format!("Create extracted directory: {e}"))?;
                dirs.insert(path);
                continue;
            }
            if !entry.is_file() {
                return Err(format!("Unsupported ZIP entry: {}", entry.name()));
            }
            let safe = entry
                .enclosed_name()
                .ok_or_else(|| format!("Unsafe archive path: {}", entry.name()))?;
            let archive_rel = checked_relative_path(entry.name())?;
            if safe != archive_rel {
                return Err(format!(
                    "Invalid or duplicate archive path: {}",
                    entry.name()
                ));
            }
            let rel = prefixed_path(pack.prefix.as_deref(), archive_rel)?;
            if !seen.insert(rel.clone()) {
                return Err(format!("Duplicate archive path: {}", entry.name()));
            }
            total = total
                .checked_add(entry.size())
                .ok_or("Pack size overflow")?;
            if total > pack.unpacked_bytes {
                return Err("Archive exceeds its declared unpacked size".into());
            }
            let target = temp_root.join(&rel);
            let parent = target.parent().ok_or("Missing extracted file parent")?;
            fs::create_dir_all(parent).map_err(|e| format!("Create extraction parent: {e}"))?;
            dirs.insert(parent.to_path_buf());
            let mut out = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&target)
                .map_err(|e| format!("Create extracted file: {e}"))?;
            let mut copied = 0_u64;
            let mut buffer = [0; PART_BUFFER_SIZE];
            loop {
                if cancel.load(Ordering::Relaxed) {
                    return Err("Cancelled".into());
                }
                let read = entry
                    .read(&mut buffer)
                    .map_err(|e| format!("Extract ZIP entry: {e}"))?;
                if read == 0 {
                    break;
                }
                copied = copied
                    .checked_add(read as u64)
                    .ok_or("Extracted file size overflow")?;
                if copied > entry.size() || copied > pack.unpacked_bytes {
                    return Err(format!("ZIP entry exceeds declared size: {}", entry.name()));
                }
                out.write_all(&buffer[..read])
                    .map_err(|e| format!("Write extracted file: {e}"))?;
            }
            if copied != entry.size() {
                return Err(format!("Truncated ZIP entry: {}", entry.name()));
            }
            files.push(rel);
            update_progress(
                shared,
                Status::Installing,
                total,
                pack.unpacked_bytes,
                "Extracting",
            );
        }
        if total != pack.unpacked_bytes {
            return Err(format!(
                "Unpacked size mismatch ({total}/{} bytes)",
                pack.unpacked_bytes
            ));
        }
        Ok((files, dirs.into_iter().collect()))
    })();
    let (files, temp_dirs) = match extract_result {
        Ok(result) => result,
        Err(error) => {
            let _ = fs::remove_dir_all(&temp_root);
            return Err(error);
        }
    };
    let _ = temp_dirs;

    install_staged_files(root, pack, &temp_root, files, shared, cancel)
}

fn install_staged_files(
    root: &Path,
    pack: &Pack,
    temp_root: &Path,
    files: Vec<PathBuf>,
    shared: &Arc<Mutex<Shared>>,
    cancel: &AtomicBool,
) -> Result<String, String> {
    let mut db = load_install_db(root);
    if db.packs.contains_key(&pack.id) {
        let _ = fs::remove_dir_all(temp_root);
        return Err("Pack is already installed".into());
    }
    let mut created_dirs = Vec::new();
    let mut installed_paths = Vec::new();
    let install_result = (|| -> Result<Vec<InstalledFile>, String> {
        let mut records = Vec::new();
        for relative in &files {
            if cancel.load(Ordering::Relaxed) {
                return Err("Cancelled".into());
            }
            ensure_no_symlink_ancestors(root, relative)?;
            let destination = root.join(relative);
            if fs::symlink_metadata(&destination).is_ok() {
                return Err(format!("Refusing to overwrite {}", relative.display()));
            }
            let mut missing = Vec::new();
            let mut parent = destination
                .parent()
                .ok_or("Missing destination parent")?
                .to_path_buf();
            while parent != root && !parent.exists() {
                missing.push(parent.clone());
                parent = parent
                    .parent()
                    .ok_or("Invalid destination parent")?
                    .to_path_buf();
            }
            fs::create_dir_all(destination.parent().ok_or("Missing destination parent")?)
                .map_err(|e| format!("Create install directory: {e}"))?;
            for path in missing {
                if !created_dirs.contains(&path) {
                    created_dirs.push(path);
                }
            }

            let source = temp_root.join(relative);
            // `hard_link` creates the destination atomically and fails if it exists, unlike rename.
            match fs::hard_link(&source, &destination) {
                Ok(()) => {}
                Err(_) => {
                    let mut out = OpenOptions::new()
                        .write(true)
                        .create_new(true)
                        .open(&destination)
                        .map_err(|e| {
                            format!("Create {} without overwrite: {e}", relative.display())
                        })?;
                    let mut input =
                        File::open(&source).map_err(|e| format!("Open extracted file: {e}"))?;
                    if let Err(error) = std::io::copy(&mut input, &mut out) {
                        let _ = fs::remove_file(&destination);
                        return Err(format!("Copy installed file: {error}"));
                    }
                    out.sync_all()
                        .map_err(|e| format!("Sync installed file: {e}"))?;
                }
            }
            installed_paths.push(destination.clone());
            let metadata =
                fs::metadata(&destination).map_err(|e| format!("Inspect installed file: {e}"))?;
            records.push(InstalledFile {
                path: relative.to_string_lossy().into_owned(),
                size: metadata.len(),
                modified_ns: modified_ns(&metadata)?,
            });
        }
        Ok(records)
    })();

    let records = match install_result {
        Ok(records) => records,
        Err(error) => {
            for path in installed_paths.iter().rev() {
                let _ = fs::remove_file(path);
            }
            for path in created_dirs.iter().rev() {
                let _ = fs::remove_dir(path);
            }
            let _ = fs::remove_dir_all(temp_root);
            return Err(error);
        }
    };
    let directories = created_dirs
        .iter()
        .filter_map(|path| {
            path.strip_prefix(root)
                .ok()
                .map(|p| p.to_string_lossy().into_owned())
        })
        .collect();
    db.packs.insert(
        pack.id.clone(),
        InstalledPack {
            files: records,
            directories,
        },
    );
    if let Err(error) = save_install_db(root, &db) {
        for path in installed_paths.iter().rev() {
            let _ = fs::remove_file(path);
        }
        for path in created_dirs.iter().rev() {
            let _ = fs::remove_dir(path);
        }
        let _ = fs::remove_dir_all(temp_root);
        return Err(error);
    }
    {
        let mut state = lock(shared);
        state.installed = db;
    }
    let _ = fs::remove_dir_all(temp_root);
    Ok(format!("Installed {}", pack.name))
}

fn install_files(
    root: &Path,
    agent: &ureq::Agent,
    pack: &Pack,
    shared: &Arc<Mutex<Shared>>,
    cancel: &AtomicBool,
) -> Result<String, String> {
    if pack.source() != "files" || !safe_id(&pack.id) {
        return Err("Invalid files pack".into());
    }
    ensure_files_download_space(root, pack, 0, &HashSet::new())?;
    let packs = packs_dir(root);
    fs::create_dir_all(packs.join("tmp"))
        .map_err(|e| format!("Create temporary directory: {e}"))?;
    let temp_root = packs.join("tmp").join(&pack.id);
    if fs::symlink_metadata(&temp_root).is_ok() {
        let metadata = fs::symlink_metadata(&temp_root)
            .map_err(|e| format!("Inspect temporary directory: {e}"))?;
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            return Err("Refusing unsafe temporary path".into());
        }
        fs::remove_dir_all(&temp_root).map_err(|e| format!("Clear previous staging: {e}"))?;
    }
    fs::create_dir(&temp_root).map_err(|e| format!("Create staging directory: {e}"))?;

    let stage_result = (|| -> Result<Vec<PathBuf>, String> {
        let mut staged_paths = Vec::with_capacity(pack.files.len());
        let mut staged_path_keys = HashSet::new();
        let mut staged_bytes = 0_u64;
        let mut progress_before = 0_u64;
        for file in &pack.files {
            if cancel.load(Ordering::Relaxed) {
                return Err("Cancelled".into());
            }
            let relative = checked_relative_path(&file.path)?;
            let partial = download_file(
                FileDownloadContext {
                    root,
                    agent,
                    pack,
                    staged_bytes,
                    staged_files: &staged_path_keys,
                    progress_before,
                    shared,
                    cancel,
                },
                file,
            )?;
            let staged = temp_root.join(&relative);
            let parent = staged.parent().ok_or("Missing staging parent")?;
            fs::create_dir_all(parent).map_err(|e| format!("Create staging parent: {e}"))?;
            match fs::hard_link(&partial, &staged) {
                Ok(()) => {}
                Err(_) => {
                    let mut output = OpenOptions::new()
                        .write(true)
                        .create_new(true)
                        .open(&staged)
                        .map_err(|e| format!("Stage {}: {e}", file.path))?;
                    let mut input =
                        File::open(&partial).map_err(|e| format!("Open verified download: {e}"))?;
                    if let Err(error) = std::io::copy(&mut input, &mut output) {
                        let _ = fs::remove_file(&staged);
                        return Err(format!("Stage {}: {error}", file.path));
                    }
                    output
                        .sync_all()
                        .map_err(|e| format!("Sync staged file: {e}"))?;
                }
            }
            staged_paths.push(relative);
            staged_path_keys.insert(file.path.clone());
            staged_bytes = staged_bytes.saturating_add(file.bytes);
            progress_before = progress_before.saturating_add(file.bytes);
        }
        if staged_bytes != pack.install_bytes() {
            return Err("Staged size does not match catalog".into());
        }
        Ok(staged_paths)
    })();

    let files = match stage_result {
        Ok(files) => files,
        Err(error) => {
            let _ = fs::remove_dir_all(&temp_root);
            return Err(error);
        }
    };
    let result = install_staged_files(root, pack, &temp_root, files, shared, cancel);
    if result.is_ok() {
        let _ = fs::remove_dir_all(packs.join("download").join(&pack.id));
    }
    result
}

fn remove_pack(root: &Path, pack_id: &str) -> Result<String, String> {
    let mut db = load_install_db(root);
    let Some(pack) = db.packs.remove(pack_id) else {
        return Err("Pack is not recorded as installed".into());
    };
    let mut removed = 0;
    let mut preserved = 0;
    for file in &pack.files {
        let relative = checked_relative_path(&file.path)?;
        ensure_no_symlink_ancestors(root, &relative)?;
        let path = root.join(relative);
        let metadata = match fs::symlink_metadata(&path) {
            Ok(metadata) if metadata.file_type().is_file() => metadata,
            _ => {
                preserved += 1;
                continue;
            }
        };
        if metadata.len() != file.size || modified_ns(&metadata)? != file.modified_ns {
            preserved += 1;
            continue;
        }
        fs::remove_file(&path).map_err(|e| format!("Remove {}: {e}", file.path))?;
        removed += 1;
    }
    let mut directories = pack.directories.iter().collect::<Vec<_>>();
    directories.sort_by_key(|dir| std::cmp::Reverse(Path::new(dir).components().count()));
    for dir in directories {
        let relative = checked_relative_path(dir)?;
        let path = root.join(relative);
        if fs::remove_dir(&path).is_err() {
            // Non-empty or already absent directories may belong to the user.
        }
    }
    save_install_db(root, &db)?;
    Ok(format!(
        "Removed {removed} files; preserved {preserved} changed or missing files"
    ))
}

fn fill_c_string<const N: usize>(dst: &mut [c_char; N], text: &str) {
    let bytes = text.as_bytes();
    let count = bytes.len().min(N.saturating_sub(1));
    for (out, input) in dst.iter_mut().zip(bytes.iter()).take(count) {
        *out = *input as c_char;
    }
    dst[count] = 0;
}

/// Creates a library handle and loads the most recent cached catalog, if any.
///
/// # Safety
/// `root` and `catalog_url` must point to valid NUL-terminated UTF-8 strings.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zt_library_open(
    root: *const c_char,
    catalog_url: *const c_char,
) -> *mut Library {
    if root.is_null() || catalog_url.is_null() {
        return std::ptr::null_mut();
    }
    // SAFETY: required by this function's contract.
    let root = PathBuf::from(
        unsafe { CStr::from_ptr(root) }
            .to_string_lossy()
            .into_owned(),
    );
    // SAFETY: required by this function's contract.
    let catalog_url = unsafe { CStr::from_ptr(catalog_url) }
        .to_string_lossy()
        .into_owned();
    if !valid_url(&catalog_url) {
        return std::ptr::null_mut();
    }
    let _ = fs::create_dir_all(packs_dir(&root));
    let agent = ureq::AgentBuilder::new()
        .timeout_connect(std::time::Duration::from_secs(20))
        .timeout_read(std::time::Duration::from_secs(45))
        .build();
    let catalog = load_catalog(&cache_catalog_path(&root)).unwrap_or(Catalog {
        version: 1,
        packs: Vec::new(),
    });
    let installed = load_install_db(&root);
    Box::into_raw(Box::new(Library {
        root,
        catalog_url,
        agent,
        shared: Arc::new(Mutex::new(Shared {
            catalog,
            installed,
            status: Status::Idle,
            permille: 0,
            message: String::new(),
        })),
        worker: Mutex::new(None),
        cancel: Arc::new(AtomicBool::new(false)),
    }))
}

/// Starts an asynchronous catalog refresh. A failed fetch keeps the last good cache.
///
/// # Safety
/// `lib` must be null or a live handle returned by `zt_library_open`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zt_library_refresh(lib: *mut Library) {
    if lib.is_null() {
        return;
    }
    // SAFETY: caller owns a live handle.
    let lib = unsafe { &*lib };
    let url = lib.catalog_url.clone();
    let agent = lib.agent.clone();
    let started = start_job(lib, Status::Fetching, move |shared, cancel, root| {
        let result = fetch_catalog(&root, &agent, &url, &cancel);
        match result {
            Ok(catalog) => {
                lock(&shared).catalog = catalog;
                finish_worker(&shared, Ok("Catalog updated".into()), &cancel);
            }
            Err(error) => {
                let cached = load_catalog(&cache_catalog_path(&root));
                if let Some(catalog) = cached {
                    lock(&shared).catalog = catalog;
                }
                finish_worker(&shared, Err(error), &cancel);
            }
        }
    });
    if !started {
        set_state(
            &lib.shared,
            Status::Failed,
            0,
            "Another library operation is active",
        );
    }
}

/// Returns the number of catalog entries.
///
/// # Safety
/// `lib` must be null or a live handle returned by `zt_library_open`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zt_library_count(lib: *const Library) -> u32 {
    if lib.is_null() {
        return 0;
    }
    // SAFETY: caller provides a live handle.
    lock(&unsafe { &*lib }.shared)
        .catalog
        .packs
        .len()
        .min(u32::MAX as usize) as u32
}

/// Copies metadata for one catalog entry. Returns 1 on success.
///
/// # Safety
/// `lib` must be a live handle and `out` must be writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zt_library_pack(
    lib: *const Library,
    index: u32,
    out: *mut PackInfo,
) -> i32 {
    if lib.is_null() || out.is_null() {
        return 0;
    }
    // SAFETY: caller provides a live handle and writable out pointer.
    let lib = unsafe { &*lib };
    let state = lock(&lib.shared);
    let Some(pack) = state.catalog.packs.get(index as usize) else {
        return 0;
    };
    let mut info = PackInfo::default();
    fill_c_string(&mut info.id, &pack.id);
    fill_c_string(&mut info.name, &pack.name);
    fill_c_string(&mut info.kind, &pack.kind);
    fill_c_string(&mut info.authors, &pack.authors);
    fill_c_string(&mut info.licence, &pack.licence);
    fill_c_string(&mut info.licence_url, &pack.licence_url);
    fill_c_string(&mut info.description, &pack.description);
    info.bytes = pack.download_bytes();
    info.songs = pack.songs;
    info.instruments = pack.instruments;
    info.samples = pack.samples;
    info.installed = u8::from(state.installed.packs.contains_key(&pack.id));
    unsafe {
        *out = info;
    }
    1
}

/// Starts a background download, verification, and installation.
///
/// # Safety
/// `lib` must be null or a live handle returned by `zt_library_open`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zt_library_install(lib: *mut Library, index: u32) -> i32 {
    if lib.is_null() {
        return 0;
    }
    // SAFETY: caller owns a live handle.
    let lib = unsafe { &*lib };
    let pack = {
        let state = lock(&lib.shared);
        let Some(pack) = state.catalog.packs.get(index as usize).cloned() else {
            return 0;
        };
        if state.installed.packs.contains_key(&pack.id) {
            drop(state);
            set_state(&lib.shared, Status::Failed, 0, "Pack is already installed");
            return 0;
        }
        pack
    };
    let agent = lib.agent.clone();
    let started = start_job(lib, Status::Downloading, move |shared, cancel, root| {
        let result = (|| {
            let install_result = match pack.source() {
                "zip" => {
                    let archive = download_pack(&root, &agent, &pack, &shared, &cancel)?;
                    let result = install_archive(&root, &pack, &archive, &shared, &cancel);
                    if result.is_ok() {
                        let _ = fs::remove_file(archive);
                    }
                    result
                }
                "files" => install_files(&root, &agent, &pack, &shared, &cancel),
                _ => Err("Unsupported pack source".into()),
            };
            if install_result.is_err() {
                set_state(&shared, Status::Installing, 0, "Installation failed");
            }
            install_result
        })();
        finish_worker(&shared, result, &cancel);
    });
    if !started {
        set_state(
            &lib.shared,
            Status::Failed,
            0,
            "Another library operation is active",
        );
        return 0;
    }
    1
}

/// Starts asynchronous removal of the selected installed pack.
///
/// # Safety
/// `lib` must be null or a live handle returned by `zt_library_open`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zt_library_remove(lib: *mut Library, index: u32) -> i32 {
    if lib.is_null() {
        return 0;
    }
    // SAFETY: caller owns a live handle.
    let lib = unsafe { &*lib };
    let pack_id = {
        let state = lock(&lib.shared);
        let Some(pack) = state.catalog.packs.get(index as usize) else {
            return 0;
        };
        if !state.installed.packs.contains_key(&pack.id) {
            drop(state);
            set_state(&lib.shared, Status::Failed, 0, "Pack is not installed");
            return 0;
        }
        pack.id.clone()
    };
    let shared_for_task = Arc::clone(&lib.shared);
    let started = start_job(lib, Status::Installing, move |shared, cancel, root| {
        let result = if cancel.load(Ordering::Relaxed) {
            Err("Cancelled".into())
        } else {
            remove_pack(&root, &pack_id)
        };
        if let Ok(db) = fs::read(installed_db_path(&root))
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .ok_or(())
        {
            lock(&shared).installed = db;
        }
        finish_worker(&shared, result, &cancel);
    });
    if !started {
        set_state(
            &shared_for_task,
            Status::Failed,
            0,
            "Another library operation is active",
        );
        return 0;
    }
    1
}

/// Requests cancellation of a running network operation.
///
/// # Safety
/// `lib` must be null or a live handle returned by `zt_library_open`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zt_library_cancel(lib: *mut Library) {
    if !lib.is_null() {
        // SAFETY: caller owns a live handle.
        unsafe { &*lib }.cancel.store(true, Ordering::Relaxed);
    }
}

/// Returns the operation state and copies a short status message.
///
/// # Safety
/// `lib` must be null or live. Non-null output pointers must be writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zt_library_status(
    lib: *const Library,
    permille: *mut u32,
    msg: *mut c_char,
    len: usize,
) -> u8 {
    if lib.is_null() {
        return Status::Failed as u8;
    }
    // SAFETY: caller provides a live handle.
    let state = lock(&unsafe { &*lib }.shared);
    if !permille.is_null() {
        // SAFETY: documented writable out pointer.
        unsafe {
            *permille = state.permille;
        }
    }
    if !msg.is_null() && len > 0 {
        let bytes = state.message.as_bytes();
        let count = bytes.len().min(len - 1);
        for (i, byte) in bytes.iter().take(count).enumerate() {
            // SAFETY: caller provides `len` writable bytes.
            unsafe {
                *msg.add(i) = *byte as c_char;
            }
        }
        // SAFETY: caller provides `len` writable bytes.
        unsafe {
            *msg.add(count) = 0;
        }
    }
    state.status as u8
}

/// Returns free bytes on the filesystem containing the songs root.
///
/// # Safety
/// `lib` must be null or a live handle returned by `zt_library_open`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zt_library_free_bytes(lib: *const Library) -> u64 {
    if lib.is_null() {
        return 0;
    }
    // SAFETY: caller provides a live handle.
    available_bytes(&unsafe { &*lib }.root)
}

/// Frees a library handle and joins its worker, if any.
///
/// # Safety
/// `lib` must be null or a live handle returned by `zt_library_open`, freed once.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zt_library_free(lib: *mut Library) {
    if lib.is_null() {
        return;
    }
    // SAFETY: caller transfers ownership of this handle exactly once.
    let lib = unsafe { Box::from_raw(lib) };
    lib.cancel.store(true, Ordering::Relaxed);
    if let Some(worker) = lock(&lib.worker).take() {
        let _ = worker.join();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;
    use std::thread;
    use std::time::SystemTime;
    use zip::write::{SimpleFileOptions, ZipWriter};

    fn temp_dir(label: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "zt-library-{label}-{}-{unique}",
            std::process::id()
        ));
        fs::create_dir_all(&path).unwrap();
        path
    }

    fn zip_with_entry(path: &Path, name: &str, bytes: &[u8], mode: Option<u32>) {
        let file = File::create(path).unwrap();
        let mut zip = ZipWriter::new(file);
        let mut options = SimpleFileOptions::default();
        if let Some(mode) = mode {
            options = options.unix_permissions(mode);
        }
        zip.start_file(name, options).unwrap();
        zip.write_all(bytes).unwrap();
        zip.finish().unwrap();
    }

    #[test]
    fn rejects_zip_slip_and_symlink_entries() {
        let root = temp_dir("zip-slip");
        let archive = root.join("bad.zip");
        zip_with_entry(&archive, "../outside.txt", b"bad", None);
        let pack = Pack {
            id: "test".into(),
            source: "zip".into(),
            prefix: None,
            files: vec![],
            name: "Test".into(),
            kind: "samples".into(),
            description: "".into(),
            authors: "".into(),
            licence: "CC0-1.0".into(),
            licence_url: "https://example.org".into(),
            url: "https://example.org/test.zip".into(),
            bytes: 1,
            unpacked_bytes: 3,
            sha256: "0".repeat(64),
            songs: 0,
            instruments: 0,
            samples: 0,
        };
        let shared = Arc::new(Mutex::new(Shared {
            catalog: Catalog {
                version: 1,
                packs: vec![],
            },
            installed: InstallDb::default(),
            status: Status::Idle,
            permille: 0,
            message: String::new(),
        }));
        assert!(install_archive(&root, &pack, &archive, &shared, &AtomicBool::new(false)).is_err());

        zip_with_entry(&archive, "Samples/link", b"target", Some(0o120777));
        assert!(install_archive(&root, &pack, &archive, &shared, &AtomicBool::new(false)).is_err());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn rejects_absolute_zip_path() {
        let root = temp_dir("absolute");
        let archive = root.join("bad.zip");
        zip_with_entry(&archive, "/outside.txt", b"bad", None);
        let pack = test_pack("test", 3);
        assert!(
            install_archive(
                &root,
                &pack,
                &archive,
                &test_shared(),
                &AtomicBool::new(false)
            )
            .is_err()
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn zip_prefix_installs_under_destination_prefix() {
        let root = temp_dir("zip-prefix");
        let archive = root.join("avrilc.zip");
        zip_with_entry(&archive, "AvrilC sidpresets/Keys/test.m8i", b"preset", None);
        let shared = test_shared();
        let mut pack = test_pack("avrilc", 6);
        pack.bytes = fs::metadata(&archive).unwrap().len();
        pack.prefix = Some("Instruments/AvrilC/".into());

        install_archive(&root, &pack, &archive, &shared, &AtomicBool::new(false)).unwrap();
        assert_eq!(
            fs::read(root.join("Instruments/AvrilC/AvrilC sidpresets/Keys/test.m8i")).unwrap(),
            b"preset"
        );
        assert!(!root.join("AvrilC sidpresets/Keys/test.m8i").exists());
        remove_pack(&root, "avrilc").unwrap();
        assert!(
            !root
                .join("Instruments/AvrilC/AvrilC sidpresets/Keys/test.m8i")
                .exists()
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn rejects_unsafe_zip_prefix() {
        let root = temp_dir("zip-prefix-slip");
        let archive = root.join("pack.zip");
        zip_with_entry(&archive, "preset.m8i", b"preset", None);
        let mut pack = test_pack("unsafe", 6);
        pack.prefix = Some("../outside".into());
        assert!(
            install_archive(
                &root,
                &pack,
                &archive,
                &test_shared(),
                &AtomicBool::new(false)
            )
            .is_err()
        );
        assert!(!root.join("outside/preset.m8i").exists());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn refuses_low_space_and_existing_files() {
        let root = temp_dir("no-overwrite");
        let archive = root.join("pack.zip");
        zip_with_entry(&archive, "Samples/Test/a.wav", b"pack", None);
        let shared = test_shared();
        let mut pack = test_pack("test", 4);
        pack.unpacked_bytes = u64::MAX;
        assert!(install_archive(&root, &pack, &archive, &shared, &AtomicBool::new(false)).is_err());
        pack.unpacked_bytes = 4;
        fs::create_dir_all(root.join("Samples/Test")).unwrap();
        fs::write(root.join("Samples/Test/a.wav"), b"user").unwrap();
        assert!(install_archive(&root, &pack, &archive, &shared, &AtomicBool::new(false)).is_err());
        assert_eq!(fs::read(root.join("Samples/Test/a.wav")).unwrap(), b"user");
        assert!(load_install_db(&root).packs.is_empty());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn download_checks_archive_and_extraction_space_before_request() {
        let root = temp_dir("download-space");
        let mut pack = test_pack("test", u64::MAX);
        pack.bytes = 4;
        assert_eq!(
            download_pack(
                &root,
                &test_agent(),
                &pack,
                &test_shared(),
                &AtomicBool::new(false)
            ),
            Err("Not enough free space to download and install this pack".into())
        );
        assert!(!packs_dir(&root).join("download/test.zip.part").exists());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn remove_pack_cleans_created_directories_deepest_first() {
        let root = temp_dir("remove-dirs");
        let archive = root.join("pack.zip");
        zip_with_entry(&archive, "Samples/Test/Nested/a.wav", b"pack", None);
        let shared = test_shared();
        let mut pack = test_pack("test", 4);
        pack.bytes = fs::metadata(&archive).unwrap().len();
        let cancel = AtomicBool::new(false);

        install_archive(&root, &pack, &archive, &shared, &cancel).unwrap();
        assert!(root.join("Samples/Test/Nested/a.wav").is_file());
        assert!(root.join("Samples/Test/Nested").is_dir());
        assert!(root.join("Samples/Test").is_dir());
        assert!(root.join("Samples").is_dir());

        remove_pack(&root, "test").unwrap();
        assert!(!root.join("Samples/Test/Nested/a.wav").exists());
        assert!(!root.join("Samples/Test/Nested").exists());
        assert!(!root.join("Samples/Test").exists());
        assert!(!root.join("Samples").exists());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn cancelled_download_stops_before_network_access() {
        let root = temp_dir("cancel");
        let pack = test_pack("test", 4);
        assert_eq!(
            download_pack(
                &root,
                &test_agent(),
                &pack,
                &test_shared(),
                &AtomicBool::new(true)
            ),
            Err("Cancelled".into())
        );
        assert!(!packs_dir(&root).join("download/test.zip.part").exists());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn retry_backoff_can_be_cancelled_promptly() {
        let cancel = Arc::new(AtomicBool::new(false));
        let cancel_after_delay = Arc::clone(&cancel);
        let canceller = thread::spawn(move || {
            thread::sleep(std::time::Duration::from_millis(50));
            cancel_after_delay.store(true, Ordering::Relaxed);
        });
        let result: Result<(), String> = retry_download("sample.wav", &cancel, || {
            Err(DownloadError::Retryable("Network error".into()))
        });
        canceller.join().unwrap();
        assert_eq!(result, Err("Cancelled".into()));
    }

    #[test]
    fn files_source_resumes_each_file_and_installs_verified_path() {
        let root = temp_dir("files-resume");
        let body = b"0123456789ab";
        let existing = b"0123";
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let request_line = Arc::new(Mutex::new(String::new()));
        let captured = Arc::clone(&request_line);
        let expected_body = body.to_vec();
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = [0; 2048];
            let n = stream.read(&mut request).unwrap();
            let request = String::from_utf8_lossy(&request[..n]);
            *lock(&captured) = request.lines().next().unwrap_or_default().to_owned();
            let range = request
                .lines()
                .find(|line| line.to_ascii_lowercase().starts_with("range:"))
                .unwrap_or_default()
                .to_owned();
            assert!(range.contains("bytes=4-"));
            let rest = &expected_body[existing.len()..];
            write!(
                stream,
                "HTTP/1.1 206 Partial Content\r\nContent-Length: {}\r\nContent-Range: bytes {}-{}/{}\r\nConnection: close\r\n\r\n",
                rest.len(),
                existing.len(),
                expected_body.len() - 1,
                expected_body.len()
            )
            .unwrap();
            stream.write_all(rest).unwrap();
        });

        let file = PackFile {
            url: format!("http://{address}/archive%20file%2Bname%3Dsample.wav"),
            path: "Samples/Floor Baba/test sample.wav".into(),
            bytes: body.len() as u64,
            sha256: format!("{:x}", Sha256::digest(body)),
        };
        let pack = Pack {
            source: "files".into(),
            files: vec![file.clone()],
            url: String::new(),
            bytes: 0,
            unpacked_bytes: 0,
            sha256: String::new(),
            ..test_pack("floor-baba", 0)
        };
        validate_catalog(&Catalog {
            version: 1,
            packs: vec![pack.clone()],
        })
        .unwrap();
        let part = file_part_path(&root, &pack.id, &file.path).unwrap();
        fs::create_dir_all(part.parent().unwrap()).unwrap();
        fs::write(&part, existing).unwrap();

        install_files(
            &root,
            &test_agent(),
            &pack,
            &test_shared(),
            &AtomicBool::new(false),
        )
        .unwrap();
        server.join().unwrap();
        assert_eq!(
            *lock(&request_line),
            "GET /archive%20file%2Bname%3Dsample.wav HTTP/1.1"
        );
        assert_eq!(fs::read(root.join(&file.path)).unwrap(), body.as_slice());
        assert!(!part.exists());
        remove_pack(&root, &pack.id).unwrap();
        assert!(!root.join(&file.path).exists());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn files_source_retries_server_errors_then_installs() {
        let root = temp_dir("files-retry");
        let body = b"retried remote sample";
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let server = thread::spawn(move || {
            for attempt in 0..3 {
                let (mut stream, _) = listener.accept().unwrap();
                let mut request = [0; 1024];
                let _ = stream.read(&mut request).unwrap();
                if attempt < 2 {
                    stream
                        .write_all(
                            b"HTTP/1.1 500 Temporary Error\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                        )
                        .unwrap();
                } else {
                    write!(
                        stream,
                        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                        body.len()
                    )
                    .unwrap();
                    stream.write_all(body).unwrap();
                }
            }
        });

        let file = PackFile {
            url: format!("http://{address}/BASS%201.wav"),
            path: "Samples/Floor Baba/BASS 1.wav".into(),
            bytes: body.len() as u64,
            sha256: format!("{:x}", Sha256::digest(body)),
        };
        let pack = Pack {
            source: "files".into(),
            files: vec![file.clone()],
            url: String::new(),
            bytes: 0,
            unpacked_bytes: 0,
            sha256: String::new(),
            ..test_pack("retry-files", 0)
        };

        install_files(
            &root,
            &test_agent(),
            &pack,
            &test_shared(),
            &AtomicBool::new(false),
        )
        .unwrap();
        server.join().unwrap();
        assert_eq!(fs::read(root.join(&file.path)).unwrap(), body);
        remove_pack(&root, &pack.id).unwrap();
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn files_source_installs_cached_sha256_verified_file_without_network() {
        let root = temp_dir("files-cached");
        let content = b"verified remote file";
        let file = PackFile {
            url: "https://example.org/already-cached.wav".into(),
            path: "Samples/Pack/already-cached.wav".into(),
            bytes: content.len() as u64,
            sha256: format!("{:x}", Sha256::digest(content)),
        };
        let pack = Pack {
            source: "files".into(),
            files: vec![file.clone()],
            url: String::new(),
            bytes: 0,
            unpacked_bytes: 0,
            sha256: String::new(),
            ..test_pack("cached-files", 0)
        };
        let part = file_part_path(&root, &pack.id, &file.path).unwrap();
        fs::create_dir_all(part.parent().unwrap()).unwrap();
        fs::write(&part, content).unwrap();

        install_files(
            &root,
            &test_agent(),
            &pack,
            &test_shared(),
            &AtomicBool::new(false),
        )
        .unwrap();
        assert_eq!(fs::read(root.join(&file.path)).unwrap(), content);
        assert!(!part.exists());
        remove_pack(&root, &pack.id).unwrap();
        assert!(!root.join(&file.path).exists());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn files_source_rejects_unsafe_and_duplicate_paths() {
        let mut pack = test_pack("files", 0);
        pack.source = "files".into();
        pack.url.clear();
        pack.sha256.clear();
        pack.files = vec![PackFile {
            url: "https://example.org/a.wav".into(),
            path: "../escape.wav".into(),
            bytes: 1,
            sha256: "a".repeat(64),
        }];
        let catalog = Catalog {
            version: 1,
            packs: vec![pack.clone()],
        };
        assert!(validate_catalog(&catalog).is_err());

        pack.files[0].path = "Samples/a.wav".into();
        pack.files.push(pack.files[0].clone());
        assert!(
            validate_catalog(&Catalog {
                version: 1,
                packs: vec![pack],
            })
            .is_err()
        );
    }

    #[test]
    fn wrong_sha256_discards_download() {
        let root = temp_dir("bad-sha");
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = [0; 512];
            let _ = stream.read(&mut request).unwrap();
            stream
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 3\r\nConnection: close\r\n\r\nbad")
                .unwrap();
        });
        let mut pack = test_pack("bad", 3);
        pack.url = format!("http://{address}/bad.zip");
        pack.bytes = 3;
        let result = download_pack(
            &root,
            &test_agent(),
            &pack,
            &test_shared(),
            &AtomicBool::new(false),
        );
        server.join().unwrap();
        assert!(result.unwrap_err().contains("SHA-256 mismatch"));
        assert!(!packs_dir(&root).join("download/bad.zip.part").exists());
        let _ = fs::remove_dir_all(root);
    }

    fn test_pack(id: &str, unpacked_bytes: u64) -> Pack {
        Pack {
            id: id.into(),
            source: "zip".into(),
            prefix: None,
            files: vec![],
            name: "Test".into(),
            kind: "samples".into(),
            description: "".into(),
            authors: "".into(),
            licence: "CC0-1.0".into(),
            licence_url: "https://example.org".into(),
            url: "https://example.org/test.zip".into(),
            bytes: 1,
            unpacked_bytes,
            sha256: "0".repeat(64),
            songs: 0,
            instruments: 0,
            samples: 0,
        }
    }

    fn test_shared() -> Arc<Mutex<Shared>> {
        Arc::new(Mutex::new(Shared {
            catalog: Catalog {
                version: 1,
                packs: vec![],
            },
            installed: InstallDb::default(),
            status: Status::Idle,
            permille: 0,
            message: String::new(),
        }))
    }

    fn test_agent() -> ureq::Agent {
        ureq::AgentBuilder::new()
            .timeout_connect(std::time::Duration::from_secs(2))
            .timeout_read(std::time::Duration::from_secs(2))
            .build()
    }

    #[test]
    fn resume_download_uses_range_and_verifies_hash() {
        let root = temp_dir("resume");
        let body = b"the rest of this test archive";
        let full = b"previous-";
        let pack_bytes = [full.as_slice(), body].concat();
        let digest = format!("{:x}", Sha256::digest(&pack_bytes));
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let received_range = Arc::new(Mutex::new(String::new()));
        let range_capture = Arc::clone(&received_range);
        let body_vec = body.to_vec();
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = [0; 1024];
            let n = stream.read(&mut request).unwrap();
            let request = String::from_utf8_lossy(&request[..n]);
            for line in request.lines() {
                if line.to_ascii_lowercase().starts_with("range:") {
                    *lock(&range_capture) = line.to_string();
                }
            }
            write!(stream, "HTTP/1.1 206 Partial Content\r\nContent-Length: {}\r\nContent-Range: bytes {}-{}/{}\r\nConnection: close\r\n\r\n", body_vec.len(), full.len(), pack_bytes_len(full.len(), body_vec.len()) - 1, full.len() + body_vec.len()).unwrap();
            stream.write_all(&body_vec).unwrap();
        });
        let pack_dir = packs_dir(&root).join("download");
        fs::create_dir_all(&pack_dir).unwrap();
        fs::write(pack_dir.join("sample.zip.part"), full).unwrap();
        let pack = Pack {
            id: "sample".into(),
            source: "zip".into(),
            prefix: None,
            files: vec![],
            name: "Sample".into(),
            kind: "samples".into(),
            description: "".into(),
            authors: "".into(),
            licence: "CC0-1.0".into(),
            licence_url: "https://example.org".into(),
            url: format!("http://{addr}/sample.zip"),
            bytes: (full.len() + body.len()) as u64,
            unpacked_bytes: 1,
            sha256: digest,
            songs: 0,
            instruments: 0,
            samples: 1,
        };
        let shared = Arc::new(Mutex::new(Shared {
            catalog: Catalog {
                version: 1,
                packs: vec![],
            },
            installed: InstallDb::default(),
            status: Status::Downloading,
            permille: 0,
            message: String::new(),
        }));
        let path = download_pack(
            &root,
            &test_agent(),
            &pack,
            &shared,
            &AtomicBool::new(false),
        )
        .unwrap();
        server.join().unwrap();
        assert_eq!(
            &*lock(&received_range),
            &format!("Range: bytes={}-", full.len())
        );
        assert_eq!(fs::read(path).unwrap(), pack_bytes);
        let _ = fs::remove_dir_all(root);
    }

    fn pack_bytes_len(offset: usize, body: usize) -> usize {
        offset + body
    }

    #[test]
    fn remove_preserves_modified_files_and_does_not_overwrite() {
        let root = temp_dir("remove");
        let relative = PathBuf::from("Samples/Test/a.wav");
        fs::create_dir_all(root.join("Samples/Test")).unwrap();
        fs::write(root.join(&relative), b"original").unwrap();
        let metadata = fs::metadata(root.join(&relative)).unwrap();
        let db = InstallDb {
            packs: BTreeMap::from([(
                "test".into(),
                InstalledPack {
                    files: vec![InstalledFile {
                        path: relative.to_string_lossy().into_owned(),
                        size: metadata.len(),
                        modified_ns: modified_ns(&metadata).unwrap(),
                    }],
                    directories: vec!["Samples/Test".into()],
                },
            )]),
        };
        save_install_db(&root, &db).unwrap();
        fs::write(root.join(&relative), b"user edit").unwrap();
        let result = remove_pack(&root, "test").unwrap();
        assert!(result.contains("preserved 1"));
        assert_eq!(fs::read(root.join(&relative)).unwrap(), b"user edit");
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn offline_catalog_cache_loads_only_valid_catalogs() {
        let root = temp_dir("offline");
        let valid = Catalog {
            version: 1,
            packs: vec![Pack {
                id: "starter".into(),
                source: "zip".into(),
                prefix: None,
                files: vec![],
                name: "Starter".into(),
                kind: "samples".into(),
                description: "".into(),
                authors: "A".into(),
                licence: "CC0-1.0".into(),
                licence_url: "https://example.org/licence".into(),
                url: "https://example.org/starter.zip".into(),
                bytes: 5,
                unpacked_bytes: 8,
                sha256: "a".repeat(64),
                songs: 2,
                instruments: 12,
                samples: 7,
            }],
        };
        atomic_write(
            &cache_catalog_path(&root),
            &serde_json::to_vec(&valid).unwrap(),
        )
        .unwrap();
        let catalog = load_catalog(&cache_catalog_path(&root)).unwrap();
        assert_eq!(catalog.packs[0].id, "starter");
        assert_eq!(catalog.packs[0].songs, 2);
        assert_eq!(catalog.packs[0].instruments, 12);
        assert_eq!(catalog.packs[0].samples, 7);

        let legacy = br#"{"version":1,"packs":[{"id":"legacy","name":"Legacy","kind":"samples","description":"","authors":"A","licence":"CC0-1.0","licence_url":"https://example.org/licence","url":"https://example.org/legacy.zip","bytes":5,"unpacked_bytes":8,"sha256":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"}]}"#;
        let legacy: Catalog = serde_json::from_slice(legacy).unwrap();
        validate_catalog(&legacy).unwrap();
        assert_eq!(legacy.packs[0].source(), "zip");
        assert!(legacy.packs[0].files.is_empty());
        let _ = fs::remove_dir_all(root);
    }
}
