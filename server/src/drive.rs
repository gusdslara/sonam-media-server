//! Network drive (WebDAV) and the watched library folder.
//!
//! SMB needs port 445, which is privileged on Linux/Android and already owned by
//! Windows itself, so the drive speaks WebDAV: Windows "Map network drive",
//! macOS Finder, GNOME/KDE file managers, davfs2, rclone and most mobile file
//! managers mount it natively.
use crate::USER_COLS;
use crate::{authenticate, digest, error, id, now, sniff, throttle, user_row, AppState, User};
use argon2::{Argon2, PasswordHash, PasswordVerifier};
use axum::{
    body::Body,
    extract::{ConnectInfo, Query, Request, State},
    http::{header, HeaderMap, Method, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use base64::Engine;
use rusqlite::params;
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    io::Read,
    net::SocketAddr,
    path::{Path, PathBuf},
};

/// Watched folders: photos, music and videos placed here (or in sub-folders)
/// show up in the media library. Uploads through the app land here too, so the
/// network drive and the library always show the same files.
pub const PHOTOS_DIR: &str = "Fotos";
pub const MUSIC_DIR: &str = "Músicas";
pub const VIDEOS_DIR: &str = "Vídeos";
/// Plain NAS storage: any file type, never indexed.
pub const FILES_DIR: &str = "Arquivos";
/// Watched folder used by version 0.1 drives; still indexed when present.
pub const LEGACY_LIBRARY_DIR: &str = "Biblioteca";
pub const WATCHED: [&str; 4] = [PHOTOS_DIR, MUSIC_DIR, VIDEOS_DIR, LEGACY_LIBRARY_DIR];

pub fn folder_for_kind(kind: &str) -> &'static str {
    match kind {
        "photo" => PHOTOS_DIR,
        "audio" => MUSIC_DIR,
        _ => VIDEOS_DIR,
    }
}

pub fn watched(rel: &str) -> bool {
    let top = rel.split('/').next().unwrap_or("");
    WATCHED.contains(&top)
}
const CREDENTIAL_TTL: i64 = 600;
const MAX_DEPTH: usize = 12;

pub fn root(s: &AppState) -> PathBuf {
    s.root.join("drive")
}

pub(crate) fn prepare(root: &Path) -> std::io::Result<()> {
    for dir in [PHOTOS_DIR, MUSIC_DIR, VIDEOS_DIR, FILES_DIR] {
        std::fs::create_dir_all(root.join("drive").join(dir))?;
    }
    Ok(())
}

/// A file name that is valid on Windows, macOS, Linux and Android storage.
pub fn safe_name(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|c| {
            if c.is_control() || matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|') {
                '_'
            } else {
                c
            }
        })
        .take(200)
        .collect();
    let cleaned = cleaned
        .trim()
        .trim_end_matches(['.', ' '])
        .trim_start_matches('.');
    if cleaned.is_empty() {
        "Arquivo".into()
    } else {
        cleaned.to_string()
    }
}

/// `dir/name`, or `dir/name (2).ext`, ... when it already exists.
pub fn unique_target(dir: &Path, name: &str) -> PathBuf {
    let candidate = dir.join(name);
    if !candidate.exists() {
        return candidate;
    }
    let (stem, ext) = match name.rsplit_once('.') {
        Some((s, e)) if !s.is_empty() => (s.to_string(), format!(".{e}")),
        _ => (name.to_string(), String::new()),
    };
    (2..)
        .map(|n| dir.join(format!("{stem} ({n}){ext}")))
        .find(|p| !p.exists())
        .unwrap()
}

/// A path is taken when a file is there or a library item that lives only in
/// Google Drive keeps that place (it comes back there when downloaded).
pub(crate) fn path_taken(s: &AppState, full: &Path) -> bool {
    full.exists()
        || relative(s, full).is_some_and(|rel| {
            s.db.lock()
                .unwrap()
                .query_row("SELECT 1 FROM media WHERE path=?", [rel], |_| Ok(()))
                .is_ok()
        })
}

/// Like `unique_target`, also skipping places reserved by Drive-only items.
pub(crate) fn unique_free(s: &AppState, dir: &Path, name: &str) -> PathBuf {
    let first = unique_target(dir, name);
    if !path_taken(s, &first) {
        return first;
    }
    let (stem, ext) = match name.rsplit_once('.') {
        Some((st, e)) if !st.is_empty() => (st.to_string(), format!(".{e}")),
        _ => (name.to_string(), String::new()),
    };
    (2..)
        .map(|n| dir.join(format!("{stem} ({n}){ext}")))
        .find(|p| !path_taken(s, p))
        .unwrap()
}

pub(crate) fn mtime_of(path: &Path) -> i64 {
    std::fs::metadata(path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

pub(crate) fn relative(s: &AppState, full: &Path) -> Option<String> {
    let rel = full.strip_prefix(root(s)).ok()?;
    let parts: Vec<String> = rel
        .components()
        .map(|c| c.as_os_str().to_string_lossy().to_string())
        .collect();
    (!parts.is_empty()).then(|| parts.join("/"))
}

/// Move a finished upload into its type folder and record it with `insert`,
/// holding the scan lock so the scanner never indexes the file twice.
pub(crate) fn place_upload<T>(
    s: &AppState,
    partial: &Path,
    name: &str,
    kind: &str,
    insert: impl FnOnce(&str, i64) -> rusqlite::Result<T>,
) -> std::io::Result<T> {
    let _guard = s.scanning.lock().unwrap_or_else(|e| e.into_inner());
    let dir = root(s).join(folder_for_kind(kind));
    std::fs::create_dir_all(&dir)?;
    let target = unique_free(s, &dir, &safe_name(name));
    std::fs::rename(partial, &target)?;
    let rel = relative(s, &target).ok_or_else(|| std::io::Error::other("caminho inválido"))?;
    match insert(&rel, mtime_of(&target)) {
        Ok(v) => Ok(v),
        Err(e) => {
            let _ = std::fs::remove_file(&target);
            Err(std::io::Error::other(e))
        }
    }
}

/// Version 0.1 kept app uploads as `media/<id>`. Move them into the type
/// folders so the drive shows everything; rows keep their id, progress and
/// collections. Files that cannot be moved simply stay where they are.
pub(crate) fn adopt_uploads(s: &AppState) {
    let rows: Vec<(String, String, String)> = {
        let db = s.db.lock().unwrap();
        let Ok(mut q) = db.prepare("SELECT id,name,kind FROM media WHERE path IS NULL") else {
            return;
        };
        let rows = q
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
            .map(|r| r.flatten().collect())
            .unwrap_or_default();
        rows
    };
    for (mid, name, kind) in rows {
        let from = s.root.join("media").join(&mid);
        if !from.is_file() {
            continue;
        }
        let dir = root(s).join(folder_for_kind(&kind));
        if std::fs::create_dir_all(&dir).is_err() {
            continue;
        }
        let target = unique_target(&dir, &safe_name(&name));
        if std::fs::rename(&from, &target).is_err() {
            continue;
        }
        let Some(rel) = relative(s, &target) else {
            let _ = std::fs::rename(&target, &from);
            continue;
        };
        let moved = s.db.lock().unwrap().execute(
            "UPDATE media SET path=?,mtime=? WHERE id=?",
            params![rel, mtime_of(&target), mid],
        );
        if moved.is_err() {
            let _ = std::fs::rename(&target, &from);
        }
    }
}

pub(crate) fn handler(root: &Path) -> dav_server::DavHandler {
    dav_server::DavHandler::builder()
        .filesystem(dav_server::localfs::LocalFs::new(
            root.join("drive"),
            false,
            false,
            true,
        ))
        .locksystem(dav_server::memls::MemLs::new())
        .strip_prefix("/dav")
        .hide_symlinks(true)
        .autoindex(true)
        .build_handler()
}

fn unauthorized(message: &str) -> Response {
    (
        StatusCode::UNAUTHORIZED,
        [(
            header::WWW_AUTHENTICATE,
            "Basic realm=\"Sonam\", charset=\"UTF-8\"",
        )],
        message.to_string(),
    )
        .into_response()
}

fn user_by_id(s: &AppState, uid: &str) -> Option<User> {
    s.db.lock()
        .unwrap()
        .query_row(
            &format!("SELECT {USER_COLS} FROM users u WHERE u.id=?"),
            [uid],
            user_row,
        )
        .ok()
}

/// WebDAV clients cannot show our login page, so they use HTTP Basic with the
/// normal account name and password. Successful credentials are cached briefly
/// (by hash) because clients repeat them on every request and Argon2 is slow on
/// purpose.
async fn dav_user(s: &AppState, addr: SocketAddr, h: &HeaderMap) -> Result<User, Response> {
    if let Ok(u) = authenticate(s, h) {
        return Ok(u);
    }
    let Some((name, password)) = h
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Basic "))
        .and_then(|v| {
            base64::engine::general_purpose::STANDARD
                .decode(v.trim())
                .ok()
        })
        .and_then(|v| String::from_utf8(v).ok())
        .and_then(|v| {
            v.split_once(':')
                .map(|(a, b)| (a.to_string(), b.to_string()))
        })
    else {
        return Err(unauthorized("Entre com o nome e a senha do Sonam"));
    };
    let key = digest(&format!("{}\0{password}", name.trim().to_lowercase()));
    let cached = {
        let mut cache = s.dav_auth.lock().unwrap();
        cache.retain(|_, v| v.1 > now());
        cache.get(&key).map(|v| v.0.clone())
    };
    if let Some(uid) = cached {
        if let Some(u) = user_by_id(s, &uid) {
            return Ok(u);
        }
    }
    throttle(s, format!("dav:{}", addr.ip())).map_err(IntoResponse::into_response)?;
    let found =
        s.db.lock()
            .unwrap()
            .query_row(
                "SELECT id,password FROM users WHERE name=?",
                [name.trim()],
                |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)),
            )
            .ok();
    let Some((uid, hash)) = found else {
        return Err(unauthorized("Nome ou senha incorretos"));
    };
    let valid = tokio::task::spawn_blocking(move || {
        PasswordHash::new(&hash).is_ok_and(|hash| {
            Argon2::default()
                .verify_password(password.as_bytes(), &hash)
                .is_ok()
        })
    })
    .await
    .unwrap_or(false);
    if !valid {
        return Err(unauthorized("Nome ou senha incorretos"));
    }
    s.dav_auth
        .lock()
        .unwrap()
        .insert(key, (uid.clone(), now() + CREDENTIAL_TTL));
    user_by_id(s, &uid).ok_or_else(|| unauthorized("Perfil removido"))
}

pub(crate) async fn dav(
    State(s): State<AppState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    req: Request,
) -> Response {
    let user = match dav_user(&s, addr, req.headers()).await {
        Ok(u) => u,
        Err(r) => return r,
    };
    // The drive is raw storage, outside per-profile genre/collection rules,
    // so only trusted profiles (admin or allowed to upload) can mount it.
    if !user.admin && !user.can_upload {
        return error(
            StatusCode::FORBIDDEN,
            "Este perfil não tem acesso à unidade de rede. Peça ao administrador para liberar envios.",
        )
        .into_response();
    }
    let writes = !matches!(*req.method(), Method::GET | Method::HEAD | Method::OPTIONS)
        && req.method().as_str() != "PROPFIND";
    // The four top folders are the drive's structure; keep them in place.
    if writes && matches!(req.method().as_str(), "DELETE" | "MOVE") {
        let path = percent_decode(req.uri().path());
        let rel = path.trim_start_matches("/dav").trim_matches('/');
        if [PHOTOS_DIR, MUSIC_DIR, VIDEOS_DIR, FILES_DIR].contains(&rel) {
            return error(
                StatusCode::FORBIDDEN,
                "As pastas Fotos, Músicas, Vídeos e Arquivos fazem parte da unidade e não podem ser removidas ou renomeadas",
            )
            .into_response();
        }
    }
    let response = s.dav.handle(req).await;
    if writes && response.status().is_success() {
        s.scan.notify_one();
    }
    response.map(Body::new)
}

/// Answer the OPTIONS probe some clients (Windows) send to `/` before `/dav`.
pub(crate) fn root_options() -> Response {
    (
        StatusCode::OK,
        [
            ("dav", "1, 2"),
            ("ms-author-via", "DAV"),
            ("allow", "OPTIONS, GET, HEAD"),
        ],
    )
        .into_response()
}

#[derive(Debug, Default, PartialEq)]
pub struct ScanReport {
    pub added: usize,
    pub moved: usize,
    pub updated: usize,
    pub removed: usize,
}

fn hidden(name: &str) -> bool {
    let lower = name.to_lowercase();
    name.starts_with('.')
        || name.starts_with("~$")
        || lower.ends_with(".partial")
        || lower.ends_with(".tmp")
        || lower == "thumbs.db"
        || lower == "desktop.ini"
}

pub(crate) fn walk(dir: &Path, rel: &str, depth: usize, out: &mut HashMap<String, (i64, i64)>) {
    if depth > MAX_DEPTH {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        if hidden(&name) {
            continue;
        }
        // Never follow links out of the drive.
        let Ok(meta) = std::fs::symlink_metadata(entry.path()) else {
            continue;
        };
        let path = format!("{rel}/{name}");
        if meta.is_dir() {
            walk(&entry.path(), &path, depth + 1, out);
        } else if meta.is_file() && meta.len() > 0 {
            let mtime = meta
                .modified()
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_secs() as i64)
                .unwrap_or(0);
            out.insert(path, (meta.len() as i64, mtime));
        }
    }
}

/// Resolve a stored relative drive path. Paths are produced by the scanner
/// from directory listings, never from user input, but stay defensive.
pub fn resolve(s: &AppState, rel: &str) -> Option<PathBuf> {
    let mut p = root(s);
    for part in rel.split('/') {
        if part.is_empty() || part == "." || part == ".." || part.contains('\\') {
            return None;
        }
        p.push(part);
    }
    Some(p)
}

fn probe(path: &Path) -> Option<(&'static str, &'static str)> {
    let mut f = std::fs::File::open(path).ok()?;
    let mut bytes = [0u8; 64];
    let n = f.read(&mut bytes).ok()?;
    sniff(&bytes[..n])
}

fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            let hex = |c: u8| (c as char).to_digit(16);
            if let (Some(h), Some(l)) = (hex(b[i + 1]), hex(b[i + 2])) {
                out.push((h * 16 + l) as u8);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn genre_for(rel: &str) -> String {
    // "Fotos/Desenhos/x.jpg" -> "Desenhos". Files at the top of a watched
    // folder get no genre.
    let parts: Vec<&str> = rel.split('/').collect();
    if parts.len() > 2 {
        parts[parts.len() - 2].chars().take(80).collect()
    } else {
        String::new()
    }
}

/// Synchronise `drive/Biblioteca` with the media table. Files keep their place
/// on disk; the library only references them. Moves/renames are detected by
/// size + modification time so progress and collections survive.
pub fn scan(s: &AppState) -> Result<ScanReport, rusqlite::Error> {
    let _guard = s.scanning.lock().unwrap_or_else(|e| e.into_inner());
    let mut report = ScanReport::default();
    let mut found = HashMap::new();
    for dir in WATCHED {
        let base = root(s).join(dir);
        if base.is_dir() {
            walk(&base, dir, 0, &mut found);
        }
    }
    let owner: Option<String> =
        s.db.lock()
            .unwrap()
            .query_row(
                "SELECT id FROM users WHERE admin=1 ORDER BY rowid LIMIT 1",
                [],
                |r| r.get(0),
            )
            .ok();
    let Some(owner) = owner else {
        return Ok(report); // Nothing to attribute files to before setup.
    };
    let known: Vec<(String, String, i64, i64, bool)> = {
        let db = s.db.lock().unwrap();
        let mut q =
            db.prepare("SELECT id,path,size,mtime,cloud_only FROM media WHERE path IS NOT NULL")?;
        let rows = q
            .query_map([], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?))
            })?
            .collect::<Result<_, _>>()?;
        rows
    };
    let mut missing = vec![];
    for (mid, path, size, mtime, cloud_only) in known {
        match found.remove(&path) {
            // The file is back where a Drive-only item lived: local again.
            Some((new_size, new_mtime)) if cloud_only => {
                s.db.lock().unwrap().execute(
                    "UPDATE media SET cloud_only=0,size=?,mtime=? WHERE id=?",
                    params![new_size, new_mtime, mid],
                )?;
                report.updated += 1;
            }
            Some((new_size, new_mtime)) if (new_size, new_mtime) != (size, mtime) => {
                let full = resolve(s, &path);
                let sniffed = full.as_deref().and_then(probe);
                let db = s.db.lock().unwrap();
                match sniffed {
                    Some((kind, mime)) => {
                        db.execute(
                            "UPDATE media SET size=?,mtime=?,kind=?,mime=? WHERE id=?",
                            params![new_size, new_mtime, kind, mime, mid],
                        )?;
                        report.updated += 1;
                    }
                    None => {
                        db.execute("DELETE FROM media WHERE id=?", [&mid])?;
                        report.removed += 1;
                    }
                }
                let _ = std::fs::remove_file(crate::thumb_path(&s.root, &mid));
            }
            Some(_) => {}
            // Freed here, kept in Google Drive.
            None if cloud_only => {}
            None => missing.push((mid, size, mtime)),
        }
    }
    let mut fresh: Vec<_> = found.into_iter().collect();
    fresh.sort();
    for (path, (size, mtime)) in fresh {
        let name: String = path
            .rsplit('/')
            .next()
            .unwrap_or("Arquivo")
            .chars()
            .take(240)
            .collect();
        if let Some(i) = missing.iter().position(|m| m.1 == size && m.2 == mtime) {
            let (mid, _, _) = missing.remove(i);
            s.db.lock().unwrap().execute(
                "UPDATE media SET path=?,name=? WHERE id=?",
                params![path, name, mid],
            )?;
            report.moved += 1;
            continue;
        }
        let Some(full) = resolve(s, &path) else {
            continue;
        };
        let Some((kind, mut mime)) = probe(&full) else {
            continue; // Documents and other files simply stay on the drive.
        };
        if mime == "video/webm" && name.to_lowercase().ends_with(".mkv") {
            mime = "video/x-matroska";
        }
        s.db.lock().unwrap().execute(
            "INSERT INTO media(id,name,kind,mime,size,genre,created,owner,path,mtime) VALUES(?,?,?,?,?,?,?,?,?,?)",
            params![id(), name, kind, mime, size, genre_for(&path), now(), owner, path, mtime],
        )?;
        report.added += 1;
    }
    for (mid, _, _) in missing {
        s.db.lock()
            .unwrap()
            .execute("DELETE FROM media WHERE id=?", [&mid])?;
        let _ = std::fs::remove_file(crate::thumb_path(&s.root, &mid));
        report.removed += 1;
    }
    Ok(report)
}

/// Background loop: initial scan, then after drive writes (debounced) and
/// every two minutes for files copied straight into the folder on the server.
pub(crate) async fn scanner(s: AppState) {
    loop {
        let st = s.clone();
        match tokio::task::spawn_blocking(move || scan(&st)).await {
            Ok(Ok(r)) if r != ScanReport::default() => println!(
                "Biblioteca da unidade: {} novo(s), {} movido(s), {} atualizado(s), {} removido(s)",
                r.added, r.moved, r.updated, r.removed
            ),
            Ok(Err(e)) => eprintln!("drive scan: {e}"),
            _ => {}
        }
        tokio::select! {
            _ = s.scan.notified() => tokio::time::sleep(std::time::Duration::from_secs(2)).await,
            _ = tokio::time::sleep(std::time::Duration::from_secs(120)) => {}
        }
    }
}

#[derive(Deserialize)]
pub(crate) struct ListQuery {
    #[serde(default)]
    path: String,
}

#[derive(Serialize)]
struct Entry {
    name: String,
    path: String,
    dir: bool,
    size: u64,
    modified: i64,
    /// Library item for this file (watched folders), for thumbnails/playback.
    media: Option<serde_json::Value>,
}

/// JSON listing of a drive folder for the app's file browser. Changes are
/// made through WebDAV (`/dav`) with the session cookie.
pub(crate) async fn list(
    State(s): State<AppState>,
    h: HeaderMap,
    Query(q): Query<ListQuery>,
) -> Result<Json<serde_json::Value>, crate::ApiError> {
    let u = authenticate(&s, &h)?;
    if !u.admin && !u.can_upload {
        return Err(error(
            StatusCode::FORBIDDEN,
            "Este perfil não tem acesso aos arquivos do servidor",
        ));
    }
    let rel = q.path.trim_matches('/').to_string();
    let dir = if rel.is_empty() {
        root(&s)
    } else {
        resolve(&s, &rel).ok_or(error(StatusCode::BAD_REQUEST, "Pasta inválida"))?
    };
    let st = s.clone();
    let rel2 = rel.clone();
    let entries = tokio::task::spawn_blocking(move || -> std::io::Result<Vec<Entry>> {
        let mut out = vec![];
        for entry in std::fs::read_dir(&dir)?.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if hidden(&name) {
                continue;
            }
            let Ok(meta) = std::fs::symlink_metadata(entry.path()) else {
                continue;
            };
            if !meta.is_dir() && !meta.is_file() {
                continue;
            }
            let path = if rel2.is_empty() {
                name.clone()
            } else {
                format!("{rel2}/{name}")
            };
            out.push(Entry {
                modified: meta
                    .modified()
                    .ok()
                    .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                    .map(|d| d.as_secs() as i64)
                    .unwrap_or(0),
                size: if meta.is_file() { meta.len() } else { 0 },
                dir: meta.is_dir(),
                name,
                path,
                media: None,
            });
        }
        // Folders first, then by name (case-insensitive).
        out.sort_by(|a, b| {
            b.dir
                .cmp(&a.dir)
                .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
        });
        let db = st.db.lock().unwrap();
        let mut q = db
            .prepare("SELECT id,kind,mime FROM media WHERE path=?")
            .map_err(std::io::Error::other)?;
        for e in out.iter_mut().filter(|e| !e.dir) {
            e.media = q
                .query_row([&e.path], |r| {
                    Ok(serde_json::json!({"id": r.get::<_, String>(0)?, "kind": r.get::<_, String>(1)?, "mime": r.get::<_, String>(2)?}))
                })
                .ok();
        }
        Ok(out)
    })
    .await
    .map_err(|_| error(StatusCode::INTERNAL_SERVER_ERROR, "Falha ao ler a pasta"))?
    .map_err(|_| error(StatusCode::NOT_FOUND, "Pasta não encontrada"))?;
    let (free, total) = free_space(&root(&s));
    Ok(Json(serde_json::json!({
        "path": rel,
        "watched": watched(&rel),
        "entries": entries,
        "free": free,
        "total": total,
    })))
}

/// Free / total bytes of the volume holding the drive (best effort).
fn free_space(path: &Path) -> (Option<u64>, Option<u64>) {
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        let Ok(c) = std::ffi::CString::new(path.as_os_str().as_bytes()) else {
            return (None, None);
        };
        let mut st: libc::statvfs = unsafe { std::mem::zeroed() };
        if unsafe { libc::statvfs(c.as_ptr(), &mut st) } == 0 {
            let unit = st.f_frsize as u64;
            return (
                Some(st.f_bavail as u64 * unit),
                Some(st.f_blocks as u64 * unit),
            );
        }
        (None, None)
    }
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        #[link(name = "kernel32")]
        extern "system" {
            fn GetDiskFreeSpaceExW(
                dir: *const u16,
                free_to_caller: *mut u64,
                total: *mut u64,
                free: *mut u64,
            ) -> i32;
        }
        let wide: Vec<u16> = path.as_os_str().encode_wide().chain([0]).collect();
        let (mut avail, mut total, mut free) = (0u64, 0u64, 0u64);
        if unsafe { GetDiskFreeSpaceExW(wide.as_ptr(), &mut avail, &mut total, &mut free) } != 0 {
            return (Some(avail), Some(total));
        }
        (None, None)
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = path;
        (None, None)
    }
}

/// Windows refuses Basic authentication over plain HTTP and limits WebDAV
/// downloads to 50 MB by default. This script (run by the owner of the PC)
/// adjusts both WebClient settings once, elevated, and then maps the drive in
/// the normal user session so Explorer can see it.
pub(crate) fn windows_script(url: &str, user: &str) -> String {
    let user = user.replace(['"', '%', '^', '&', '|', '<', '>'], "");
    format!(
        r#"@echo off
chcp 65001 >nul
title Sonam - unidade de rede
setlocal
set "URL={url}"
set "HSUSER={user}"
if /I "%~1"=="--configurar" goto configure

echo Sonam: conectar a unidade de rede
echo Endereco: %URL%
echo.
reg query "HKLM\SYSTEM\CurrentControlSet\Services\WebClient\Parameters" /v BasicAuthLevel 2>nul | find "0x2" >nul
if errorlevel 1 (
  echo O Windows precisa liberar WebDAV em HTTP e arquivos grandes.
  echo Confirme a janela de administrador que vai abrir.
  powershell -NoProfile -Command "Start-Process -FilePath '%~f0' -ArgumentList '--configurar' -Verb RunAs -Wait"
)
echo.
echo Digite a senha do perfil "%HSUSER%" no Sonam quando pedir.
net use * "%URL%" /user:"%HSUSER%" * /persistent:yes
if errorlevel 1 (
  echo.
  echo Nao foi possivel conectar. Confira o endereco, a senha e se o servidor esta ligado.
) else (
  echo.
  echo Pronto. A unidade aparece em "Este Computador".
)
pause
exit /b

:configure
reg add "HKLM\SYSTEM\CurrentControlSet\Services\WebClient\Parameters" /v BasicAuthLevel /t REG_DWORD /d 2 /f >nul
reg add "HKLM\SYSTEM\CurrentControlSet\Services\WebClient\Parameters" /v FileSizeLimitInBytes /t REG_DWORD /d 4294967295 /f >nul
sc config WebClient start= auto >nul
net stop WebClient >nul 2>&1
net start WebClient >nul
exit /b
"#
    )
    .replace('\n', "\r\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn genre_from_parent_folder() {
        assert_eq!(genre_for("Biblioteca/x.mp4"), "");
        assert_eq!(genre_for("Vídeos/x.mp4"), "");
        assert_eq!(genre_for("Vídeos/Desenhos/x.mp4"), "Desenhos");
        assert_eq!(genre_for("Vídeos/Filmes/Terror/x.mp4"), "Terror");
    }

    #[test]
    fn names_are_portable_and_unique() {
        assert_eq!(safe_name("a/b:c?.jpg"), "a_b_c_.jpg");
        assert_eq!(safe_name("  ..  "), "Arquivo");
        assert_eq!(safe_name(".oculto.jpg"), "oculto.jpg");
        let d = tempfile::tempdir().unwrap();
        std::fs::write(d.path().join("x.jpg"), b"1").unwrap();
        std::fs::write(d.path().join("x (2).jpg"), b"1").unwrap();
        assert_eq!(unique_target(d.path(), "x.jpg"), d.path().join("x (3).jpg"));
        assert_eq!(unique_target(d.path(), "y"), d.path().join("y"));
        assert!(watched("Fotos/a.jpg") && watched("Biblioteca") && !watched("Arquivos/a.jpg"));
        assert_eq!(percent_decode("/dav/M%C3%BAsicas"), "/dav/Músicas");
    }

    #[test]
    fn old_uploads_move_into_type_folders() {
        let d = tempfile::tempdir().unwrap();
        let s = crate::state(d.path().into()).unwrap();
        {
            let db = s.db.lock().unwrap();
            db.execute(
                "INSERT INTO users(id,name,password,admin) VALUES('a','Admin','x',1)",
                [],
            )
            .unwrap();
            db.execute("INSERT INTO media(id,name,kind,mime,size,created,owner) VALUES('m1','Praia.jpg','photo','image/jpeg',3,1,'a')", []).unwrap();
        }
        std::fs::write(d.path().join("media/m1"), b"\xff\xd8\xff").unwrap();
        adopt_uploads(&s);
        let path: String =
            s.db.lock()
                .unwrap()
                .query_row("SELECT path FROM media WHERE id='m1'", [], |r| r.get(0))
                .unwrap();
        assert_eq!(path, "Fotos/Praia.jpg");
        assert!(root(&s).join("Fotos/Praia.jpg").is_file());
        assert!(!d.path().join("media/m1").exists());
        assert_eq!(scan(&s).unwrap(), ScanReport::default());
    }

    #[test]
    fn resolve_rejects_traversal() {
        let d = tempfile::tempdir().unwrap();
        let s = crate::state(d.path().into()).unwrap();
        assert!(resolve(&s, "Biblioteca/../../secret").is_none());
        assert!(resolve(&s, "Biblioteca/a\\..\\b").is_none());
        assert!(resolve(&s, "Biblioteca/ok.mp4").is_some());
    }

    #[test]
    fn scan_adds_moves_and_removes() {
        let d = tempfile::tempdir().unwrap();
        let s = crate::state(d.path().into()).unwrap();
        s.db.lock()
            .unwrap()
            .execute(
                "INSERT INTO users(id,name,password,admin) VALUES('a','Admin','x',1)",
                [],
            )
            .unwrap();
        let lib = root(&s).join(LEGACY_LIBRARY_DIR);
        std::fs::create_dir_all(lib.join("Viagens")).unwrap();
        std::fs::write(lib.join("Viagens/praia.jpg"), b"\xff\xd8\xffjpeg-data").unwrap();
        std::fs::write(lib.join("notas.txt"), b"not media").unwrap();
        std::fs::write(lib.join(".oculto.jpg"), b"\xff\xd8\xff").unwrap();
        let r = scan(&s).unwrap();
        assert_eq!(r.added, 1);
        let (mid, genre): (String, String) =
            s.db.lock()
                .unwrap()
                .query_row("SELECT id,genre FROM media", [], |r| {
                    Ok((r.get(0)?, r.get(1)?))
                })
                .unwrap();
        assert_eq!(genre, "Viagens");
        // Rename keeps the same library entry.
        std::fs::rename(lib.join("Viagens/praia.jpg"), lib.join("praia-2026.jpg")).unwrap();
        let r = scan(&s).unwrap();
        assert_eq!(r.moved, 1);
        let (same, path): (String, String) =
            s.db.lock()
                .unwrap()
                .query_row("SELECT id,path FROM media", [], |r| {
                    Ok((r.get(0)?, r.get(1)?))
                })
                .unwrap();
        assert_eq!(same, mid);
        assert_eq!(path, "Biblioteca/praia-2026.jpg");
        std::fs::remove_file(lib.join("praia-2026.jpg")).unwrap();
        assert_eq!(scan(&s).unwrap().removed, 1);
        assert_eq!(scan(&s).unwrap(), ScanReport::default());
    }

    #[test]
    fn windows_script_is_crlf_and_sanitized() {
        let script = windows_script("http://192.168.0.2:8096/dav/", "Ana\"&calc");
        assert!(script.contains("\r\n"));
        assert!(script.contains("set \"HSUSER=Anacalc\""));
    }
}
