//! Optional backup of the library to the owner's Google Drive.
//!
//! * Sign-in uses Google's device flow ("TVs and limited input devices"): the
//!   server shows a code, the owner confirms it on any phone or computer. No
//!   redirect URL is needed, so it works on a LAN IP, a `.local` name or a TV.
//! * Scope `drive.file`: the app only sees files it created itself, never the
//!   rest of the owner's Drive.
//! * Files land in `Meu Drive/Sonam/<same folders as the network drive>`,
//!   so they can be found by hand (`Vídeos/Filmes/x.mp4` → `Sonam/Vídeos/Filmes/x.mp4`).
//! * Sync is manual. "Free space" removes the local copy only after the Drive
//!   copy was verified by MD5; the library row stays (`media.cloud_only`) and is
//!   played straight from Drive through this server.
use crate::{
    admin, authenticate, bad, drive, error, id, now, set_setting, setting, visible, ApiResult,
    AppState, User,
};
use axum::{
    body::Body,
    extract::{Path, Query, State},
    http::{header, HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use md5::{Digest, Md5};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, HashSet},
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Duration,
};
use tokio::io::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt};

const SCOPE: &str = "https://www.googleapis.com/auth/drive.file";
const API: &str = "https://www.googleapis.com/drive/v3";
const UPLOAD: &str = "https://www.googleapis.com/upload/drive/v3";
const OAUTH: &str = "https://oauth2.googleapis.com";
const FOLDER_MIME: &str = "application/vnd.google-apps.folder";
pub const ROOT_NAME: &str = "Sonam";
/// Resumable upload chunk; must be a multiple of 256 KiB.
const CHUNK: usize = 16 * 1024 * 1024;
const TMP_DIR: &str = "cloud-tmp";

type CloudResult<T> = Result<T, String>;

#[derive(Default)]
pub struct CloudState {
    token: Option<(String, i64)>,
    device: Option<DeviceAuth>,
    job: Job,
    cancel: Arc<AtomicBool>,
}

#[derive(Clone, Serialize)]
struct DeviceAuth {
    user_code: String,
    verification_url: String,
    expires: i64,
    error: Option<String>,
    #[serde(skip)]
    device_code: String,
}

#[derive(Clone, Serialize, Default)]
struct Job {
    running: bool,
    /// "sync", "free" or "restore".
    kind: String,
    done: usize,
    total: usize,
    bytes_done: u64,
    bytes_total: u64,
    current: String,
    errors: Vec<String>,
    finished: Option<i64>,
    summary: String,
    cancelled: bool,
}

pub fn migrate(db: &Connection) -> rusqlite::Result<()> {
    let has = db
        .prepare("SELECT 1 FROM pragma_table_info('media') WHERE name='cloud_only'")?
        .exists([])?;
    if !has {
        db.execute_batch("ALTER TABLE media ADD COLUMN cloud_only INTEGER NOT NULL DEFAULT 0;")?;
    }
    db.execute_batch(
        "CREATE TABLE IF NOT EXISTS cloud(
            id TEXT PRIMARY KEY,
            key TEXT NOT NULL UNIQUE,
            media_id TEXT,
            path TEXT NOT NULL,
            name TEXT NOT NULL,
            kind TEXT NOT NULL,
            mime TEXT NOT NULL,
            size INTEGER NOT NULL,
            mtime INTEGER NOT NULL,
            genre TEXT NOT NULL DEFAULT '',
            created INTEGER NOT NULL,
            md5 TEXT NOT NULL,
            drive_id TEXT NOT NULL,
            uploaded INTEGER NOT NULL,
            lost INTEGER NOT NULL DEFAULT 0
        );
        CREATE INDEX IF NOT EXISTS cloud_media ON cloud(media_id);
        CREATE TABLE IF NOT EXISTS cloud_folders(path TEXT PRIMARY KEY, drive_id TEXT NOT NULL);",
    )
}

/// Leftovers of interrupted downloads.
pub fn prepare(root: &std::path::Path) -> std::io::Result<()> {
    let tmp = root.join(TMP_DIR);
    if tmp.is_dir() {
        for e in std::fs::read_dir(&tmp)?.flatten() {
            let _ = std::fs::remove_file(e.path());
        }
    }
    std::fs::create_dir_all(tmp)
}

pub fn client() -> reqwest::Client {
    reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(20))
        .user_agent(concat!("Sonam/", env!("CARGO_PKG_VERSION")))
        .build()
        .expect("cliente HTTPS")
}

fn credentials(s: &AppState) -> Option<(String, String)> {
    let id = crate::env_var("GOOGLE_CLIENT_ID")
        .ok()
        .or_else(|| setting(s, "gdrive_client_id"))
        .filter(|v| !v.is_empty())?;
    let secret = crate::env_var("GOOGLE_CLIENT_SECRET")
        .ok()
        .or_else(|| setting(s, "gdrive_client_secret"))
        .unwrap_or_default();
    Some((id, secret))
}

fn connected(s: &AppState) -> bool {
    setting(s, "gdrive_refresh").is_some_and(|v| !v.is_empty())
}

fn esc(v: &str) -> String {
    v.replace('\\', "\\\\").replace('\'', "\\'")
}

fn google_error(v: &serde_json::Value) -> String {
    v.pointer("/error/message")
        .or_else(|| v.get("error_description"))
        .or_else(|| v.get("error"))
        .and_then(|e| e.as_str())
        .unwrap_or("erro desconhecido")
        .to_string()
}

fn rate_limited(v: &serde_json::Value) -> bool {
    matches!(
        v.pointer("/error/errors/0/reason").and_then(|r| r.as_str()),
        Some("rateLimitExceeded" | "userRateLimitExceeded")
    )
}

fn net_error(e: reqwest::Error) -> String {
    eprintln!("google: {e}");
    if e.is_timeout() {
        "O Google Drive demorou demais para responder".into()
    } else {
        "Sem conexão com o Google. Verifique a internet do servidor".into()
    }
}

// ---------------------------------------------------------------- tokens ---

/// One refresh at a time: a grid of Drive thumbnails would otherwise ask
/// Google for a new token once per image.
static REFRESH: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

async fn access_token(s: &AppState, force: bool) -> CloudResult<String> {
    let cached = || {
        s.cloud
            .lock()
            .unwrap()
            .token
            .clone()
            .filter(|(_, exp)| *exp > now() + 60)
            .map(|(t, _)| t)
    };
    if !force {
        if let Some(t) = cached() {
            return Ok(t);
        }
    }
    let _one = REFRESH.lock().await;
    if !force {
        if let Some(t) = cached() {
            return Ok(t);
        }
    }
    let refresh = setting(s, "gdrive_refresh")
        .filter(|v| !v.is_empty())
        .ok_or("Conecte uma conta Google primeiro")?;
    let (cid, secret) = credentials(s).ok_or("Configure o acesso ao Google primeiro")?;
    let r = s
        .http
        .post(format!("{OAUTH}/token"))
        .timeout(Duration::from_secs(30))
        .form(&[
            ("client_id", cid.as_str()),
            ("client_secret", secret.as_str()),
            ("refresh_token", refresh.as_str()),
            ("grant_type", "refresh_token"),
        ])
        .send()
        .await
        .map_err(net_error)?;
    let ok = r.status().is_success();
    let v: serde_json::Value = r.json().await.unwrap_or_default();
    if !ok {
        if v.get("error").and_then(|e| e.as_str()) == Some("invalid_grant") {
            return Err(
                "O acesso ao Google Drive foi revogado ou expirou. Conecte a conta novamente"
                    .into(),
            );
        }
        return Err(format!("Google recusou o acesso: {}", google_error(&v)));
    }
    let t = v["access_token"]
        .as_str()
        .ok_or("Resposta inválida do Google")?
        .to_string();
    let exp = now() + v["expires_in"].as_i64().unwrap_or(3600);
    s.cloud.lock().unwrap().token = Some((t.clone(), exp));
    Ok(t)
}

/// JSON call with auth, one token refresh on 401 and backoff on 429/5xx.
async fn call(
    s: &AppState,
    method: reqwest::Method,
    url: &str,
    query: &[(&str, &str)],
    body: Option<&serde_json::Value>,
) -> CloudResult<serde_json::Value> {
    let mut refreshed = false;
    for attempt in 0..5u32 {
        let token = access_token(s, false).await?;
        let mut req = s
            .http
            .request(method.clone(), url)
            .bearer_auth(&token)
            .timeout(Duration::from_secs(60))
            .query(query);
        if let Some(b) = body {
            req = req.json(b);
        }
        let r = match req.send().await {
            Ok(r) => r,
            Err(e) if attempt < 3 => {
                eprintln!("google: {e}");
                tokio::time::sleep(Duration::from_secs(2u64 << attempt)).await;
                continue;
            }
            Err(e) => return Err(net_error(e)),
        };
        let status = r.status();
        if status == reqwest::StatusCode::UNAUTHORIZED && !refreshed {
            refreshed = true;
            access_token(s, true).await?;
            continue;
        }
        if (status.as_u16() == 429 || status.is_server_error()) && attempt < 4 {
            tokio::time::sleep(Duration::from_secs(2u64 << attempt)).await;
            continue;
        }
        if status == reqwest::StatusCode::NO_CONTENT {
            return Ok(serde_json::Value::Null);
        }
        let v: serde_json::Value = r.json().await.unwrap_or_default();
        // Drive reports per-user rate limits as 403.
        if status == reqwest::StatusCode::FORBIDDEN && rate_limited(&v) && attempt < 4 {
            tokio::time::sleep(Duration::from_secs(2u64 << attempt)).await;
            continue;
        }
        if !status.is_success() {
            return Err(if status == reqwest::StatusCode::NOT_FOUND {
                "not_found".into()
            } else if v.pointer("/error/errors/0/reason").and_then(|r| r.as_str())
                == Some("storageQuotaExceeded")
            {
                "Seu Google Drive está sem espaço livre".into()
            } else {
                format!("Google Drive: {}", google_error(&v))
            });
        }
        return Ok(v);
    }
    Err("O Google Drive está ocupado. Tente de novo em alguns minutos".into())
}

// ----------------------------------------------------------- device flow ---

#[derive(Deserialize)]
pub(crate) struct Config {
    client_id: String,
    client_secret: String,
}

pub(crate) async fn save_config(
    State(s): State<AppState>,
    h: HeaderMap,
    Json(c): Json<Config>,
) -> ApiResult<Json<serde_json::Value>> {
    admin(&s, &h)?;
    let cid = c.client_id.trim();
    let secret = c.client_secret.trim();
    if !cid.ends_with(".apps.googleusercontent.com") || cid.len() > 200 {
        return Err(bad(
            "O ID do cliente termina com .apps.googleusercontent.com. Copie do Google Cloud Console",
        ));
    }
    if secret.is_empty() || secret.len() > 200 || secret.chars().any(char::is_whitespace) {
        return Err(bad("Cole a chave secreta do cliente OAuth"));
    }
    set_setting(&s, "gdrive_client_id", cid)?;
    set_setting(&s, "gdrive_client_secret", secret)?;
    Ok(Json(serde_json::json!({"ok":true})))
}

pub(crate) async fn connect_start(
    State(s): State<AppState>,
    h: HeaderMap,
) -> ApiResult<Json<serde_json::Value>> {
    admin(&s, &h)?;
    let (cid, _) = credentials(&s).ok_or(bad("Configure o acesso ao Google primeiro"))?;
    let r = s
        .http
        .post(format!("{OAUTH}/device/code"))
        .timeout(Duration::from_secs(30))
        .form(&[("client_id", cid.as_str()), ("scope", SCOPE)])
        .send()
        .await
        .map_err(|e| bad(&net_error(e)))?;
    let ok = r.status().is_success();
    let v: serde_json::Value = r.json().await.unwrap_or_default();
    if !ok {
        let reason = google_error(&v);
        let code = v["error"].as_str().unwrap_or_default();
        return Err(bad(
            &if matches!(code, "invalid_client" | "unauthorized_client")
                || reason.to_lowercase().contains("client type")
            {
                "O Google não aceitou este cliente. Ele precisa ser do tipo \"TVs e dispositivos de entrada limitada\"".to_string()
            } else {
                format!("Google recusou: {reason}")
            },
        ));
    }
    let device = DeviceAuth {
        user_code: v["user_code"].as_str().unwrap_or_default().into(),
        verification_url: v["verification_url"]
            .as_str()
            .unwrap_or("https://www.google.com/device")
            .into(),
        expires: now() + v["expires_in"].as_i64().unwrap_or(1800),
        error: None,
        device_code: v["device_code"].as_str().unwrap_or_default().into(),
    };
    let interval = v["interval"].as_u64().unwrap_or(5).max(2);
    s.cloud.lock().unwrap().device = Some(device.clone());
    tokio::spawn(poll_device(s.clone(), device.device_code.clone(), interval));
    Ok(Json(serde_json::to_value(device).unwrap()))
}

fn device_failed(s: &AppState, code: &str, msg: &str) {
    if let Some(d) = s.cloud.lock().unwrap().device.as_mut() {
        if d.device_code == code {
            d.error = Some(msg.into());
        }
    }
}

async fn poll_device(s: AppState, code: String, mut interval: u64) {
    loop {
        tokio::time::sleep(Duration::from_secs(interval)).await;
        let current = s.cloud.lock().unwrap().device.clone();
        let Some(d) = current.filter(|d| d.device_code == code && d.error.is_none()) else {
            return; // cancelled or replaced by a newer code
        };
        if d.expires < now() {
            device_failed(&s, &code, "O código expirou. Gere outro");
            return;
        }
        let Some((cid, secret)) = credentials(&s) else {
            return;
        };
        let r = s
            .http
            .post(format!("{OAUTH}/token"))
            .timeout(Duration::from_secs(30))
            .form(&[
                ("client_id", cid.as_str()),
                ("client_secret", secret.as_str()),
                ("device_code", code.as_str()),
                ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
            ])
            .send()
            .await;
        let Ok(r) = r else { continue };
        let v: serde_json::Value = r.json().await.unwrap_or_default();
        match v.get("error").and_then(|e| e.as_str()) {
            Some("authorization_pending") => continue,
            Some("slow_down") => {
                interval += 5;
                continue;
            }
            Some("access_denied") => {
                device_failed(&s, &code, "O acesso foi negado na conta Google");
                return;
            }
            Some("expired_token") => {
                device_failed(&s, &code, "O código expirou. Gere outro");
                return;
            }
            Some(_) => {
                device_failed(&s, &code, &format!("Google recusou: {}", google_error(&v)));
                return;
            }
            None => {}
        }
        let (Some(access), Some(refresh)) =
            (v["access_token"].as_str(), v["refresh_token"].as_str())
        else {
            device_failed(&s, &code, "Resposta inválida do Google");
            return;
        };
        let exp = now() + v["expires_in"].as_i64().unwrap_or(3600);
        let email = s
            .http
            .get(format!("{API}/about"))
            .query(&[("fields", "user(emailAddress)")])
            .bearer_auth(access)
            .timeout(Duration::from_secs(30))
            .send()
            .await
            .ok();
        let email = match email {
            Some(r) => r
                .json::<serde_json::Value>()
                .await
                .ok()
                .and_then(|v| v.pointer("/user/emailAddress")?.as_str().map(String::from))
                .unwrap_or_default(),
            None => String::new(),
        };
        if let Err(msg) = adopt_account(&s, &email) {
            device_failed(&s, &code, &msg);
            let _ = s
                .http
                .post(format!("{OAUTH}/revoke"))
                .form(&[("token", refresh)])
                .send()
                .await;
            return;
        }
        let _ = set_setting(&s, "gdrive_refresh", refresh);
        let _ = set_setting(&s, "gdrive_email", &email);
        let mut c = s.cloud.lock().unwrap();
        c.token = Some((access.into(), exp));
        c.device = None;
        return;
    }
}

/// A different account would make the existing Drive copies unreachable.
/// Allowed only when nothing lives exclusively in the old account.
fn adopt_account(s: &AppState, email: &str) -> CloudResult<()> {
    let old = setting(s, "gdrive_email").unwrap_or_default();
    if old.is_empty() || old.eq_ignore_ascii_case(email) {
        return Ok(());
    }
    if email.is_empty() {
        return Err("Não foi possível confirmar qual conta Google foi usada. Tente de novo".into());
    }
    let only = inventory(s).map_err(|e| e.to_string())?.only_cloud.count;
    if only > 0 {
        return Err(format!(
            "Esta é outra conta. {only} item(ns) estão só em {old}: conecte essa conta, ou baixe-os de volta antes de trocar"
        ));
    }
    let db = s.db.lock().unwrap();
    db.execute_batch("DELETE FROM cloud; DELETE FROM cloud_folders; DELETE FROM settings WHERE key='gdrive_root';")
        .map_err(|e| e.to_string())
}

pub(crate) async fn connect_cancel(
    State(s): State<AppState>,
    h: HeaderMap,
) -> ApiResult<Json<serde_json::Value>> {
    admin(&s, &h)?;
    s.cloud.lock().unwrap().device = None;
    Ok(Json(serde_json::json!({"ok":true})))
}

pub(crate) async fn disconnect(
    State(s): State<AppState>,
    h: HeaderMap,
) -> ApiResult<Json<serde_json::Value>> {
    admin(&s, &h)?;
    if s.cloud.lock().unwrap().job.running {
        return Err(bad("Espere a tarefa atual terminar ou cancele-a"));
    }
    if let Some(refresh) = setting(&s, "gdrive_refresh").filter(|v| !v.is_empty()) {
        let _ = s
            .http
            .post(format!("{OAUTH}/revoke"))
            .timeout(Duration::from_secs(15))
            .form(&[("token", refresh.as_str())])
            .send()
            .await;
    }
    // Keep the e-mail, folder ids and the list of copies: reconnecting the
    // same account gives the app access to the same files again.
    set_setting(&s, "gdrive_refresh", "")?;
    s.cloud.lock().unwrap().token = None;
    Ok(Json(serde_json::json!({"ok":true})))
}

// ------------------------------------------------------------- inventory ---

#[derive(Clone, Debug)]
struct LocalItem {
    key: String,
    media_id: Option<String>,
    /// Path inside the network drive; mirrored under `Sonam/` in Drive.
    path: String,
    name: String,
    kind: String,
    mime: String,
    size: i64,
    mtime: i64,
    genre: String,
    created: i64,
    full: PathBuf,
}

#[derive(Clone, Debug)]
struct CloudRow {
    id: String,
    key: String,
    media_id: Option<String>,
    path: String,
    name: String,
    kind: String,
    mime: String,
    size: i64,
    mtime: i64,
    md5: String,
    drive_id: String,
    lost: bool,
}

#[derive(Serialize, Default, Clone, Copy)]
struct Tally {
    count: usize,
    bytes: i64,
}
impl Tally {
    fn add(&mut self, bytes: i64) {
        self.count += 1;
        self.bytes += bytes;
    }
}

#[derive(Default)]
struct Inventory {
    locals: Vec<LocalItem>,
    rows: Vec<CloudRow>,
    here: Tally,
    backed: Tally,
    pending: Tally,
    only_cloud: Tally,
    lost: Tally,
    /// Backed-up local copies per kind: what "free space" can remove.
    freeable: HashMap<String, Tally>,
}

fn row(r: &rusqlite::Row) -> rusqlite::Result<CloudRow> {
    Ok(CloudRow {
        id: r.get(0)?,
        key: r.get(1)?,
        media_id: r.get(2)?,
        path: r.get(3)?,
        name: r.get(4)?,
        kind: r.get(5)?,
        mime: r.get(6)?,
        size: r.get(7)?,
        mtime: r.get(8)?,
        md5: r.get(9)?,
        drive_id: r.get(10)?,
        lost: r.get(11)?,
    })
}
const ROW_COLS: &str = "id,key,media_id,path,name,kind,mime,size,mtime,md5,drive_id,lost";

fn local_items(s: &AppState) -> rusqlite::Result<Vec<LocalItem>> {
    let mut out = vec![];
    {
        let db = s.db.lock().unwrap();
        let mut q = db.prepare(
            "SELECT id,path,name,kind,mime,size,mtime,genre,created FROM media WHERE cloud_only=0",
        )?;
        let rows = q.query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, Option<String>>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, String>(4)?,
                r.get::<_, i64>(5)?,
                r.get::<_, i64>(6)?,
                r.get::<_, String>(7)?,
                r.get::<_, i64>(8)?,
            ))
        })?;
        for r in rows {
            let (mid, path, name, kind, mime, size, mtime, genre, created) = r?;
            let (path, full) = match path {
                Some(p) => match drive::resolve(s, &p) {
                    Some(full) => (p, full),
                    None => continue,
                },
                None => (
                    format!(
                        "{}/{}",
                        drive::folder_for_kind(&kind),
                        drive::safe_name(&name)
                    ),
                    s.root.join("media").join(&mid),
                ),
            };
            out.push(LocalItem {
                key: format!("m:{mid}"),
                media_id: Some(mid),
                path,
                name,
                kind,
                mime,
                size,
                mtime,
                genre,
                created,
                full,
            });
        }
    }
    let mut files = HashMap::new();
    let base = drive::root(s).join(drive::FILES_DIR);
    if base.is_dir() {
        drive::walk(&base, drive::FILES_DIR, 0, &mut files);
    }
    for (path, (size, mtime)) in files {
        let Some(full) = drive::resolve(s, &path) else {
            continue;
        };
        let name = path.rsplit('/').next().unwrap_or("Arquivo").to_string();
        out.push(LocalItem {
            key: format!("f:{path}"),
            media_id: None,
            mime: mime_guess::from_path(&name)
                .first_or_octet_stream()
                .to_string(),
            name,
            kind: "file".into(),
            size,
            mtime,
            genre: String::new(),
            created: mtime,
            full,
            path,
        });
    }
    Ok(out)
}

fn inventory(s: &AppState) -> rusqlite::Result<Inventory> {
    let locals = local_items(s)?;
    let rows: Vec<CloudRow> = {
        let db = s.db.lock().unwrap();
        let mut q = db.prepare(&format!("SELECT {ROW_COLS} FROM cloud"))?;
        let r = q.query_map([], row)?.collect::<Result<_, _>>()?;
        r
    };
    let by_key: HashMap<&str, &CloudRow> = rows.iter().map(|r| (r.key.as_str(), r)).collect();
    let mut inv = Inventory::default();
    let mut present = HashSet::new();
    for l in &locals {
        inv.here.add(l.size);
        match by_key.get(l.key.as_str()) {
            Some(r) => {
                present.insert(r.id.clone());
                if !r.lost && r.size == l.size && r.mtime == l.mtime {
                    inv.backed.add(l.size);
                    inv.freeable.entry(l.kind.clone()).or_default().add(l.size);
                } else {
                    inv.pending.add(l.size);
                }
            }
            None => inv.pending.add(l.size),
        }
    }
    for r in &rows {
        if present.contains(&r.id) {
            continue;
        }
        if r.lost {
            inv.lost.add(r.size);
        } else {
            inv.only_cloud.add(r.size);
        }
    }
    inv.locals = locals;
    inv.rows = rows;
    Ok(inv)
}

// ---------------------------------------------------------------- status ---

/// For every signed-in profile: is there a Drive copy to browse?
pub(crate) async fn summary(
    State(s): State<AppState>,
    h: HeaderMap,
) -> ApiResult<Json<serde_json::Value>> {
    authenticate(&s, &h)?;
    let items: i64 =
        s.db.lock()
            .unwrap()
            .query_row("SELECT count(*) FROM cloud WHERE lost=0", [], |r| r.get(0))?;
    Ok(Json(serde_json::json!({
        "connected": connected(&s),
        "items": items,
    })))
}

pub(crate) async fn status(
    State(s): State<AppState>,
    h: HeaderMap,
) -> ApiResult<Json<serde_json::Value>> {
    admin(&s, &h)?;
    let st = s.clone();
    let inv = tokio::task::spawn_blocking(move || inventory(&st))
        .await
        .map_err(|_| bad("Falha ao contar os arquivos"))??;
    let is_connected = connected(&s);
    let quota = if is_connected {
        call(
            &s,
            reqwest::Method::GET,
            &format!("{API}/about"),
            &[("fields", "storageQuota(limit,usage)")],
            None,
        )
        .await
    } else {
        Err(String::new())
    };
    // A revoked or broken sign-in must show up here, not only when syncing.
    let problem = quota.as_ref().err().filter(|e| !e.is_empty()).cloned();
    let quota = quota.ok().map(|v| {
        serde_json::json!({
            "used": v.pointer("/storageQuota/usage").and_then(|x| x.as_str()).and_then(|x| x.parse::<i64>().ok()),
            "limit": v.pointer("/storageQuota/limit").and_then(|x| x.as_str()).and_then(|x| x.parse::<i64>().ok()),
        })
    });
    let (device, job) = {
        let c = s.cloud.lock().unwrap();
        (c.device.clone(), c.job.clone())
    };
    let env = crate::env_var("GOOGLE_CLIENT_ID").is_ok();
    Ok(Json(serde_json::json!({
        "configured": credentials(&s).is_some(),
        "managed": env,
        "client_id": credentials(&s).map(|c| c.0),
        "connected": is_connected,
        "email": setting(&s, "gdrive_email").filter(|v| !v.is_empty()),
        "folder": setting(&s, "gdrive_root").filter(|v| !v.is_empty()),
        "quota": quota,
        // Sign-in rejected (vs. network trouble): offer to connect again.
        "reauth": problem.as_deref().is_some_and(|p| p.contains("revogado") || p.starts_with("Google recusou")),
        "problem": problem,
        "device": device,
        "job": job,
        "here": inv.here,
        "backed": inv.backed,
        "pending": inv.pending,
        "only_cloud": inv.only_cloud,
        "lost": inv.lost,
        "freeable": inv.freeable,
        "last_sync": setting(&s, "gdrive_last_sync").and_then(|v| v.parse::<i64>().ok()),
    })))
}

// ------------------------------------------------------------------ jobs ---

fn begin(s: &AppState, kind: &str, total: usize, bytes: u64) -> ApiResult<Arc<AtomicBool>> {
    let mut c = s.cloud.lock().unwrap();
    if c.job.running {
        return Err(error(
            StatusCode::CONFLICT,
            "Já há uma tarefa do Google Drive em andamento",
        ));
    }
    c.cancel = Arc::new(AtomicBool::new(false));
    c.job = Job {
        running: true,
        kind: kind.into(),
        total,
        bytes_total: bytes,
        ..Default::default()
    };
    Ok(c.cancel.clone())
}

fn progress(s: &AppState, f: impl FnOnce(&mut Job)) {
    f(&mut s.cloud.lock().unwrap().job);
}

fn finish(s: &AppState, summary: String, cancelled: bool) {
    progress(s, |j| {
        j.running = false;
        j.current.clear();
        j.finished = Some(now());
        j.summary = summary;
        j.cancelled = cancelled;
    });
}

fn fail(s: &AppState, name: &str, e: &str) {
    progress(s, |j| {
        if j.errors.len() < 50 {
            j.errors.push(format!("{name}: {e}"));
        }
    });
}

pub(crate) async fn cancel(
    State(s): State<AppState>,
    h: HeaderMap,
) -> ApiResult<Json<serde_json::Value>> {
    admin(&s, &h)?;
    s.cloud.lock().unwrap().cancel.store(true, Ordering::SeqCst);
    Ok(Json(serde_json::json!({"ok":true})))
}

// ----------------------------------------------------------- Drive tree ---

async fn ensure_root(s: &AppState) -> CloudResult<String> {
    if let Some(rid) = setting(s, "gdrive_root").filter(|v| !v.is_empty()) {
        match call(
            s,
            reqwest::Method::GET,
            &format!("{API}/files/{rid}"),
            &[("fields", "id,trashed")],
            None,
        )
        .await
        {
            Ok(v) if v["trashed"].as_bool() != Some(true) => return Ok(rid),
            Ok(_) => {}
            Err(e) if e != "not_found" => return Err(e),
            Err(_) => {}
        }
        // Removed by hand in Drive: start a fresh tree; copies are re-checked.
        let db = s.db.lock().unwrap();
        let _ = db.execute("DELETE FROM cloud_folders", []);
    }
    let v = call(
        s,
        reqwest::Method::POST,
        &format!("{API}/files"),
        &[("fields", "id")],
        Some(&serde_json::json!({
            "name": ROOT_NAME,
            "mimeType": FOLDER_MIME,
            "description": "Cópia de segurança feita pelo Sonam. As pastas são as mesmas da unidade de rede.",
            "appProperties": {"hs": "root"},
        })),
    )
    .await?;
    let rid = v["id"]
        .as_str()
        .ok_or("Resposta inválida do Google")?
        .to_string();
    set_setting(s, "gdrive_root", &rid).map_err(|e| e.1)?;
    let _ = simple_upload(
        s,
        &rid,
        "LEIA-ME.txt",
        "text/plain",
        include_str!("cloud_readme.txt").as_bytes().to_vec(),
    )
    .await;
    Ok(rid)
}

async fn simple_upload(
    s: &AppState,
    parent: &str,
    name: &str,
    mime: &str,
    data: Vec<u8>,
) -> CloudResult<()> {
    let boundary = format!("hs{}", id().replace('-', ""));
    let meta = serde_json::json!({"name": name, "parents": [parent], "mimeType": mime});
    let mut body = format!(
        "--{boundary}\r\nContent-Type: application/json; charset=UTF-8\r\n\r\n{meta}\r\n--{boundary}\r\nContent-Type: {mime}\r\n\r\n"
    )
    .into_bytes();
    body.extend(data);
    body.extend(format!("\r\n--{boundary}--\r\n").into_bytes());
    let token = access_token(s, false).await?;
    let r = s
        .http
        .post(format!("{UPLOAD}/files?uploadType=multipart"))
        .bearer_auth(token)
        .header(
            header::CONTENT_TYPE,
            format!("multipart/related; boundary={boundary}"),
        )
        .timeout(Duration::from_secs(60))
        .body(body)
        .send()
        .await
        .map_err(net_error)?;
    if !r.status().is_success() {
        return Err("Falha ao criar o arquivo de instruções".into());
    }
    Ok(())
}

/// Drive folder id for a drive-relative folder ("Vídeos/Filmes"), creating the
/// chain under `Sonam/` as needed. Ids are cached in `cloud_folders`.
async fn folder(s: &AppState, root_id: &str, path: &str) -> CloudResult<String> {
    let mut parent = root_id.to_string();
    let mut acc = String::new();
    for part in path.split('/').filter(|p| !p.is_empty()) {
        if !acc.is_empty() {
            acc.push('/');
        }
        acc.push_str(part);
        let cached: Option<String> =
            s.db.lock()
                .unwrap()
                .query_row(
                    "SELECT drive_id FROM cloud_folders WHERE path=?",
                    [&acc],
                    |r| r.get(0),
                )
                .optional()
                .map_err(|e| e.to_string())?;
        if let Some(fid) = cached {
            parent = fid;
            continue;
        }
        let q = format!(
            "name='{}' and '{}' in parents and mimeType='{FOLDER_MIME}' and trashed=false",
            esc(part),
            esc(&parent)
        );
        let found = call(
            s,
            reqwest::Method::GET,
            &format!("{API}/files"),
            &[("q", &q), ("fields", "files(id)"), ("pageSize", "1")],
            None,
        )
        .await?;
        let fid = match found.pointer("/files/0/id").and_then(|v| v.as_str()) {
            Some(fid) => fid.to_string(),
            None => call(
                s,
                reqwest::Method::POST,
                &format!("{API}/files"),
                &[("fields", "id")],
                Some(&serde_json::json!({"name": part, "mimeType": FOLDER_MIME, "parents": [parent]})),
            )
            .await?["id"]
                .as_str()
                .ok_or("Resposta inválida do Google")?
                .to_string(),
        };
        s.db.lock()
            .unwrap()
            .execute(
                "INSERT OR REPLACE INTO cloud_folders VALUES(?,?)",
                params![acc, fid],
            )
            .map_err(|e| e.to_string())?;
        parent = fid;
    }
    Ok(parent)
}

fn parent_of(path: &str) -> &str {
    path.rsplit_once('/').map(|p| p.0).unwrap_or("")
}

/// Every file this app created that still exists (not trashed), with MD5.
async fn remote_files(s: &AppState) -> CloudResult<HashMap<String, String>> {
    let mut out = HashMap::new();
    let mut page: Option<String> = None;
    loop {
        let mut q: Vec<(&str, &str)> = vec![
            (
                "q",
                "trashed=false and appProperties has { key='hs' and value='1' }",
            ),
            ("fields", "nextPageToken,files(id,md5Checksum)"),
            ("pageSize", "1000"),
            ("spaces", "drive"),
        ];
        if let Some(p) = page.as_deref() {
            q.push(("pageToken", p));
        }
        let v = call(s, reqwest::Method::GET, &format!("{API}/files"), &q, None).await?;
        for f in v["files"].as_array().into_iter().flatten() {
            if let Some(fid) = f["id"].as_str() {
                out.insert(
                    fid.to_string(),
                    f["md5Checksum"].as_str().unwrap_or_default().to_string(),
                );
            }
        }
        match v["nextPageToken"].as_str() {
            Some(p) => page = Some(p.to_string()),
            None => return Ok(out),
        }
    }
}

/// Drive search is eventually consistent: a copy uploaded moments ago can be
/// missing from `remote_files`. Ask for each missing id directly before
/// treating it as deleted (which would upload it twice or mark it lost).
async fn confirm_missing<'a>(
    s: &AppState,
    remote: &mut HashMap<String, String>,
    ids: impl Iterator<Item = &'a str>,
) -> CloudResult<()> {
    let missing: Vec<String> = ids
        .filter(|id| !remote.contains_key(*id))
        .map(String::from)
        .collect();
    for fid in missing {
        match call(
            s,
            reqwest::Method::GET,
            &format!("{API}/files/{fid}"),
            &[("fields", "id,trashed,md5Checksum")],
            None,
        )
        .await
        {
            Ok(v) if v["trashed"].as_bool() != Some(true) => {
                let md5 = v["md5Checksum"].as_str().unwrap_or_default().to_string();
                remote.insert(fid, md5);
            }
            Ok(_) => {}
            Err(e) if e == "not_found" => {}
            Err(e) => return Err(e),
        }
    }
    Ok(())
}

// ---------------------------------------------------------------- upload ---

/// Resumable upload in 16 MiB chunks with resume after network errors.
/// Returns (drive id, md5 computed locally); fails if Google's MD5 differs.
async fn upload_file(
    s: &AppState,
    item: &LocalItem,
    parent: &str,
    existing: Option<&str>,
    cancel: &AtomicBool,
) -> CloudResult<(String, String)> {
    let size = item.size as u64;
    let meta = match existing {
        Some(_) => serde_json::json!({"mimeType": item.mime}),
        None => serde_json::json!({
            "name": item.path.rsplit('/').next().unwrap_or(&item.name),
            "parents": [parent],
            "mimeType": item.mime,
            "appProperties": {"hs": "1", "hs_kind": item.kind},
        }),
    };
    let (method, url) = match existing {
        Some(fid) => (
            reqwest::Method::PATCH,
            format!("{UPLOAD}/files/{fid}?uploadType=resumable&fields=id,md5Checksum,size"),
        ),
        None => (
            reqwest::Method::POST,
            format!("{UPLOAD}/files?uploadType=resumable&fields=id,md5Checksum,size"),
        ),
    };
    let mut session: Option<String> = None;
    let mut file = tokio::fs::File::open(&item.full)
        .await
        .map_err(|_| "Não foi possível ler o arquivo no servidor".to_string())?;
    let mut hasher = Md5::new();
    let mut hashed = 0u64;
    let mut offset = 0u64;
    let mut failures = 0u32;
    let mut buf = vec![0u8; CHUNK];
    loop {
        if cancel.load(Ordering::SeqCst) {
            return Err("cancelado".into());
        }
        let token = access_token(s, false).await?;
        let url_session = match &session {
            Some(u) => u.clone(),
            None => {
                let r = s
                    .http
                    .request(method.clone(), &url)
                    .bearer_auth(&token)
                    .header("X-Upload-Content-Type", &item.mime)
                    .header("X-Upload-Content-Length", size)
                    .timeout(Duration::from_secs(60))
                    .json(&meta)
                    .send()
                    .await
                    .map_err(net_error)?;
                if r.status() == reqwest::StatusCode::UNAUTHORIZED && failures == 0 {
                    failures += 1;
                    access_token(s, true).await?;
                    continue;
                }
                if r.status() == reqwest::StatusCode::NOT_FOUND {
                    return Err("not_found".into());
                }
                if !r.status().is_success() {
                    let code = r.status().as_u16();
                    let v: serde_json::Value = r.json().await.unwrap_or_default();
                    if (code == 429 || code >= 500 || rate_limited(&v)) && failures < 5 {
                        failures += 1;
                        tokio::time::sleep(Duration::from_secs(2u64 << failures)).await;
                        continue;
                    }
                    return Err(
                        if v.pointer("/error/errors/0/reason").and_then(|r| r.as_str())
                            == Some("storageQuotaExceeded")
                        {
                            "Seu Google Drive está sem espaço livre".into()
                        } else {
                            format!("Google Drive: {}", google_error(&v))
                        },
                    );
                }
                let loc = r
                    .headers()
                    .get(header::LOCATION)
                    .and_then(|v| v.to_str().ok())
                    .ok_or("O Google não abriu a sessão de envio")?
                    .to_string();
                session = Some(loc.clone());
                loc
            }
        };
        // Read the next chunk from `offset`.
        file.seek(std::io::SeekFrom::Start(offset))
            .await
            .map_err(|e| e.to_string())?;
        let want = ((size - offset) as usize).min(CHUNK);
        let mut n = 0;
        while n < want {
            let k = file
                .read(&mut buf[n..want])
                .await
                .map_err(|e| e.to_string())?;
            if k == 0 {
                return Err("O arquivo mudou durante o envio".into());
            }
            n += k;
        }
        if offset + n as u64 > hashed {
            let skip = (hashed - offset) as usize;
            hasher.update(&buf[skip..n]);
            hashed = offset + n as u64;
        }
        let mut req = s
            .http
            .put(&url_session)
            .timeout(Duration::from_secs(600))
            .header(header::CONTENT_LENGTH, n);
        if size > 0 {
            req = req.header(
                header::CONTENT_RANGE,
                format!("bytes {}-{}/{}", offset, offset + n as u64 - 1, size),
            );
        }
        let sent = req.body(buf[..n].to_vec()).send().await;
        let r = match sent {
            Ok(r) => r,
            Err(e) => {
                failures += 1;
                if failures > 6 {
                    return Err(net_error(e));
                }
                tokio::time::sleep(Duration::from_secs(2u64 << failures.min(5))).await;
                offset = resume_offset(s, &url_session, size).await.unwrap_or(offset);
                continue;
            }
        };
        let status = r.status().as_u16();
        match status {
            200 | 201 => {
                let v: serde_json::Value = r.json().await.unwrap_or_default();
                let fid = v["id"]
                    .as_str()
                    .ok_or("Resposta inválida do Google")?
                    .to_string();
                let local = hex::encode(hasher.finalize());
                let remote = v["md5Checksum"].as_str().unwrap_or_default();
                if hashed != size || (!remote.is_empty() && remote != local) {
                    return Err(
                        "A cópia no Drive não confere com o original. Tente sincronizar de novo"
                            .into(),
                    );
                }
                progress(s, |j| j.bytes_done += n as u64);
                return Ok((fid, local));
            }
            308 => {
                let next = range_end(&r).map(|e| e + 1).unwrap_or(0);
                let advanced = next.saturating_sub(offset);
                progress(s, |j| j.bytes_done += advanced);
                offset = next;
                failures = 0;
            }
            404 | 410 => {
                // Session expired: start over.
                progress(s, |j| j.bytes_done = j.bytes_done.saturating_sub(offset));
                session = None;
                offset = 0;
                failures += 1;
                if failures > 3 {
                    return Err("O Google encerrou a sessão de envio".into());
                }
            }
            429 | 500..=599 => {
                failures += 1;
                if failures > 6 {
                    return Err("O Google Drive está ocupado. Tente de novo mais tarde".into());
                }
                tokio::time::sleep(Duration::from_secs(2u64 << failures.min(5))).await;
                offset = resume_offset(s, &url_session, size).await.unwrap_or(offset);
            }
            _ => {
                let v: serde_json::Value = r.json().await.unwrap_or_default();
                return Err(format!("Google Drive: {}", google_error(&v)));
            }
        }
    }
}

fn range_end(r: &reqwest::Response) -> Option<u64> {
    r.headers()
        .get(header::RANGE)?
        .to_str()
        .ok()?
        .strip_prefix("bytes=")?
        .split('-')
        .nth(1)?
        .parse()
        .ok()
}

async fn resume_offset(s: &AppState, session: &str, size: u64) -> Option<u64> {
    let r = s
        .http
        .put(session)
        .header(header::CONTENT_RANGE, format!("bytes */{size}"))
        .header(header::CONTENT_LENGTH, 0)
        .timeout(Duration::from_secs(30))
        .send()
        .await
        .ok()?;
    (r.status().as_u16() == 308).then(|| range_end(&r).map(|e| e + 1).unwrap_or(0))
}

// ------------------------------------------------------------------ sync ---

pub(crate) async fn sync(
    State(s): State<AppState>,
    h: HeaderMap,
) -> ApiResult<Json<serde_json::Value>> {
    admin(&s, &h)?;
    if !connected(&s) {
        return Err(bad("Conecte uma conta Google primeiro"));
    }
    let cancel = begin(&s, "sync", 0, 0)?;
    progress(&s, |j| j.current = "Conferindo o Google Drive…".into());
    tokio::spawn(async move {
        let result = run_sync(&s, &cancel).await;
        let cancelled = cancel.load(Ordering::SeqCst);
        let summary = match result {
            Ok(msg) => {
                let _ = set_setting(&s, "gdrive_last_sync", &now().to_string());
                msg
            }
            Err(e) => {
                fail(&s, "Sincronização", &e);
                "A sincronização parou".into()
            }
        };
        finish(&s, summary, cancelled);
    });
    Ok(Json(serde_json::json!({"ok":true})))
}

enum Step {
    New,
    Content(String),
    Move,
}

async fn run_sync(s: &AppState, cancel: &AtomicBool) -> CloudResult<String> {
    let root_id = ensure_root(s).await?;
    // Copies deleted by hand in Drive: upload again when the original is
    // still here, otherwise flag them as lost.
    let mut remote = remote_files(s).await?;
    let st = s.clone();
    let inv = tokio::task::spawn_blocking(move || inventory(&st))
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())?;
    confirm_missing(s, &mut remote, inv.rows.iter().map(|r| r.drive_id.as_str())).await?;
    let local_keys: HashSet<&str> = inv.locals.iter().map(|l| l.key.as_str()).collect();
    {
        let db = s.db.lock().unwrap();
        for r in &inv.rows {
            let exists = remote.contains_key(&r.drive_id);
            if !exists && local_keys.contains(r.key.as_str()) {
                let _ = db.execute("DELETE FROM cloud WHERE id=?", [&r.id]);
            } else if exists == r.lost {
                let _ = db.execute("UPDATE cloud SET lost=? WHERE id=?", params![!exists, r.id]);
            }
        }
    }
    let rows: HashMap<String, CloudRow> = {
        let db = s.db.lock().unwrap();
        let mut q = db
            .prepare(&format!("SELECT {ROW_COLS} FROM cloud"))
            .map_err(|e| e.to_string())?;
        let v: Vec<CloudRow> = q
            .query_map([], row)
            .map_err(|e| e.to_string())?
            .flatten()
            .collect();
        v.into_iter().map(|r| (r.key.clone(), r)).collect()
    };
    let mut plan: Vec<(LocalItem, Step)> = vec![];
    for l in inv.locals {
        match rows.get(&l.key) {
            None => plan.push((l, Step::New)),
            Some(r) if r.lost => {}
            Some(r) if r.size != l.size || r.mtime != l.mtime => {
                let fid = r.drive_id.clone();
                plan.push((l, Step::Content(fid)))
            }
            Some(r) if r.path != l.path => plan.push((l, Step::Move)),
            Some(_) => {}
        }
    }
    // Small files first: photos and music are safe quickly, films after.
    plan.sort_by_key(|(l, _)| l.size);
    let bytes: u64 = plan
        .iter()
        .filter(|(_, st)| !matches!(st, Step::Move))
        .map(|(l, _)| l.size as u64)
        .sum();
    progress(s, |j| {
        j.total = plan.len();
        j.bytes_total = bytes;
    });
    let (mut sent, mut moved, mut failed) = (0usize, 0usize, 0usize);
    for (l, step) in plan {
        if cancel.load(Ordering::SeqCst) {
            break;
        }
        progress(s, |j| j.current = l.name.clone());
        let r = sync_one(s, &root_id, &l, step, rows.get(&l.key), cancel).await;
        match r {
            Ok(true) => sent += 1,
            Ok(false) => moved += 1,
            Err(e) if e == "cancelado" => break,
            Err(e) => {
                failed += 1;
                fail(s, &l.name, &e);
                if e.contains("sem espaço") || e.contains("revogado") {
                    break;
                }
            }
        }
        progress(s, |j| j.done += 1);
    }
    Ok(match (sent, moved, failed) {
        (0, 0, 0) => "Tudo já estava no Google Drive".into(),
        _ => {
            let mut parts = vec![];
            if sent > 0 {
                parts.push(format!("{sent} enviado(s)"));
            }
            if moved > 0 {
                parts.push(format!("{moved} reorganizado(s)"));
            }
            if failed > 0 {
                parts.push(format!("{failed} com erro"));
            }
            parts.join(", ")
        }
    })
}

fn file_name(l: &LocalItem) -> &str {
    l.path.rsplit('/').next().unwrap_or(&l.name)
}

/// Put a Drive file in `parent` with `name`, wherever it is now (the user
/// may also have moved it by hand in Drive).
async fn move_file(s: &AppState, fid: &str, parent: &str, name: &str) -> CloudResult<()> {
    let v = call(
        s,
        reqwest::Method::GET,
        &format!("{API}/files/{fid}"),
        &[("fields", "parents")],
        None,
    )
    .await?;
    let old: Vec<&str> = v["parents"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|p| p.as_str())
        .filter(|p| *p != parent)
        .collect();
    let remove = old.join(",");
    let already = v["parents"]
        .as_array()
        .is_some_and(|a| a.iter().any(|p| p.as_str() == Some(parent)));
    let mut q: Vec<(&str, &str)> = vec![("fields", "id")];
    if !already {
        q.push(("addParents", parent));
    }
    if !remove.is_empty() {
        q.push(("removeParents", &remove));
    }
    call(
        s,
        reqwest::Method::PATCH,
        &format!("{API}/files/{fid}"),
        &q,
        Some(&serde_json::json!({ "name": name })),
    )
    .await
    .map(|_| ())
}

/// Ok(true) = bytes uploaded, Ok(false) = only moved/renamed in Drive.
async fn sync_one(
    s: &AppState,
    root_id: &str,
    l: &LocalItem,
    step: Step,
    old: Option<&CloudRow>,
    cancel: &AtomicBool,
) -> CloudResult<bool> {
    let mut retried = false;
    loop {
        let parent = folder(s, root_id, parent_of(&l.path)).await?;
        let res = match &step {
            Step::Move => {
                let old = old.ok_or("cópia não encontrada")?;
                move_file(s, &old.drive_id, &parent, file_name(l))
                    .await
                    .map(|_| (old.drive_id.clone(), old.md5.clone()))
            }
            Step::Content(fid) => {
                let moved = match old {
                    Some(o) if o.path != l.path => move_file(s, fid, &parent, file_name(l)).await,
                    _ => Ok(()),
                };
                match moved {
                    Ok(()) => upload_file(s, l, &parent, Some(fid), cancel).await,
                    Err(e) => Err(e),
                }
            }
            Step::New => upload_file(s, l, &parent, None, cancel).await,
        };
        match res {
            Ok((fid, md5)) => {
                let db = s.db.lock().unwrap();
                db.execute(
                    "INSERT INTO cloud(id,key,media_id,path,name,kind,mime,size,mtime,genre,created,md5,drive_id,uploaded,lost)
                     VALUES(?,?,?,?,?,?,?,?,?,?,?,?,?,?,0)
                     ON CONFLICT(key) DO UPDATE SET path=excluded.path,name=excluded.name,kind=excluded.kind,mime=excluded.mime,
                       size=excluded.size,mtime=excluded.mtime,genre=excluded.genre,md5=excluded.md5,drive_id=excluded.drive_id,
                       uploaded=excluded.uploaded,lost=0",
                    params![
                        id(), l.key, l.media_id, l.path, l.name, l.kind, l.mime, l.size, l.mtime,
                        l.genre, l.created, md5, fid, now()
                    ],
                )
                .map_err(|e| e.to_string())?;
                return Ok(!matches!(step, Step::Move));
            }
            // A cached folder was deleted by hand in Drive: rebuild once.
            Err(e) if e == "not_found" && !retried => {
                retried = true;
                let _ =
                    s.db.lock()
                        .unwrap()
                        .execute("DELETE FROM cloud_folders", []);
                if !matches!(step, Step::New) {
                    // The copy itself is gone too; upload as new.
                    let _ =
                        s.db.lock()
                            .unwrap()
                            .execute("DELETE FROM cloud WHERE key=?", [&l.key]);
                    return Box::pin(sync_one(s, root_id, l, Step::New, None, cancel)).await;
                }
            }
            Err(e) if e == "not_found" => return Err("Pasta do Drive não encontrada".into()),
            Err(e) => return Err(e),
        }
    }
}

// ------------------------------------------------------------ free space ---

#[derive(Deserialize)]
pub(crate) struct FreeRequest {
    /// "video", "audio", "photo", "file".
    #[serde(default)]
    kinds: Vec<String>,
    /// Library items (media ids) to free individually.
    #[serde(default)]
    media: Vec<String>,
}

pub(crate) async fn free(
    State(s): State<AppState>,
    h: HeaderMap,
    Json(f): Json<FreeRequest>,
) -> ApiResult<Json<serde_json::Value>> {
    admin(&s, &h)?;
    if !connected(&s) {
        return Err(bad("Conecte a conta Google para liberar espaço"));
    }
    if f.kinds.is_empty() && f.media.is_empty() {
        return Err(bad("Escolha o que liberar"));
    }
    let cancel = begin(&s, "free", 0, 0)?;
    progress(&s, |j| {
        j.current = "Conferindo as cópias no Google Drive…".into()
    });
    let st = s.clone();
    let single = f.media.len() == 1 && f.kinds.is_empty();
    let job = async move {
        let r = run_free(&st, f, &cancel).await;
        let cancelled = cancel.load(Ordering::SeqCst);
        match r {
            Ok((n, bytes)) => {
                finish(
                    &st,
                    format!(
                        "{n} cópia(s) local(is) removida(s), {} liberados",
                        human(bytes)
                    ),
                    cancelled,
                );
                Ok(n)
            }
            Err(e) => {
                fail(&st, "Liberar espaço", &e);
                finish(&st, "Nada foi removido".into(), cancelled);
                Err(e)
            }
        }
    };
    if single {
        // One item from the library screen: answer when done.
        let n = job.await.map_err(|e| bad(&e))?;
        if n == 0 {
            return Err(bad(
                "Este arquivo ainda não tem cópia conferida no Google Drive. Sincronize primeiro",
            ));
        }
        return Ok(Json(serde_json::json!({"ok":true})));
    }
    tokio::spawn(job);
    Ok(Json(serde_json::json!({"ok":true})))
}

fn human(bytes: i64) -> String {
    let b = bytes as f64;
    if b >= 1024f64.powi(3) {
        format!("{:.1} GB", b / 1024f64.powi(3)).replace('.', ",")
    } else if b >= 1024f64.powi(2) {
        format!("{:.1} MB", b / 1024f64.powi(2)).replace('.', ",")
    } else {
        format!("{} KB", (b / 1024.0).ceil() as i64)
    }
}

async fn run_free(s: &AppState, f: FreeRequest, cancel: &AtomicBool) -> CloudResult<(usize, i64)> {
    let mut remote = remote_files(s).await?;
    let st = s.clone();
    let inv = tokio::task::spawn_blocking(move || inventory(&st))
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())?;
    let rows: HashMap<&str, &CloudRow> = inv.rows.iter().map(|r| (r.key.as_str(), r)).collect();
    let wanted: HashSet<&str> = f.media.iter().map(String::as_str).collect();
    let candidates = inv
        .locals
        .iter()
        .filter(|l| {
            f.kinds.contains(&l.kind) || l.media_id.as_deref().is_some_and(|m| wanted.contains(m))
        })
        .filter_map(|l| rows.get(l.key.as_str()).map(|r| r.drive_id.as_str()));
    confirm_missing(s, &mut remote, candidates).await?;
    let picked: Vec<(LocalItem, CloudRow)> = inv
        .locals
        .iter()
        .filter(|l| {
            f.kinds.contains(&l.kind) || l.media_id.as_deref().is_some_and(|m| wanted.contains(m))
        })
        .filter_map(|l| {
            let r = rows.get(l.key.as_str())?;
            // Only copies that match the original and still exist in Drive
            // with the MD5 verified at upload.
            (!r.lost
                && r.size == l.size
                && r.mtime == l.mtime
                && remote
                    .get(&r.drive_id)
                    .is_some_and(|m| m.is_empty() || *m == r.md5))
            .then(|| (l.clone(), (*r).clone()))
        })
        .collect();
    progress(s, |j| {
        j.total = picked.len();
        j.bytes_total = picked.iter().map(|p| p.0.size as u64).sum();
    });
    let mut freed = (0usize, 0i64);
    for (l, _) in picked {
        if cancel.load(Ordering::SeqCst) {
            break;
        }
        progress(s, |j| j.current = l.name.clone());
        let st = s.clone();
        let item = l.clone();
        let r = tokio::task::spawn_blocking(move || free_one(&st, &item))
            .await
            .map_err(|e| e.to_string())?;
        match r {
            Ok(()) => {
                freed.0 += 1;
                freed.1 += l.size;
            }
            Err(e) => fail(s, &l.name, &e),
        }
        progress(s, |j| {
            j.done += 1;
            j.bytes_done += l.size as u64;
        });
    }
    Ok(freed)
}

fn free_one(s: &AppState, l: &LocalItem) -> CloudResult<()> {
    // Keep a thumbnail so the Drive view still shows the photo quickly.
    if l.kind == "photo" {
        let thumb = crate::thumb_path(&s.root, l.media_id.as_deref().unwrap_or_default());
        if !thumb.exists() {
            crate::render_thumbnail(l.full.clone(), thumb);
        }
    }
    let _guard = s.scanning.lock().unwrap_or_else(|e| e.into_inner());
    let meta =
        std::fs::metadata(&l.full).map_err(|_| "o arquivo não está mais aqui".to_string())?;
    if meta.len() as i64 != l.size || drive::mtime_of(&l.full) != l.mtime {
        return Err("o arquivo mudou desde a cópia; sincronize de novo".into());
    }
    if let Some(mid) = &l.media_id {
        s.db.lock()
            .unwrap()
            .execute("UPDATE media SET cloud_only=1 WHERE id=?", [mid])
            .map_err(|e| e.to_string())?;
    }
    if let Err(e) = std::fs::remove_file(&l.full) {
        if let Some(mid) = &l.media_id {
            let _ =
                s.db.lock()
                    .unwrap()
                    .execute("UPDATE media SET cloud_only=0 WHERE id=?", [mid]);
        }
        return Err(format!("não foi possível apagar a cópia local ({e})"));
    }
    Ok(())
}

// --------------------------------------------------------------- restore ---

#[derive(Deserialize)]
pub(crate) struct RestoreRequest {
    #[serde(default)]
    ids: Vec<String>,
    #[serde(default)]
    all: bool,
}

pub(crate) async fn restore(
    State(s): State<AppState>,
    h: HeaderMap,
    Json(r): Json<RestoreRequest>,
) -> ApiResult<Json<serde_json::Value>> {
    admin(&s, &h)?;
    if !connected(&s) {
        return Err(bad("Conecte a conta Google para baixar"));
    }
    let st = s.clone();
    let inv = tokio::task::spawn_blocking(move || inventory(&st))
        .await
        .map_err(|_| bad("Falha ao contar os arquivos"))??;
    let local: HashSet<&str> = inv.locals.iter().map(|l| l.key.as_str()).collect();
    let wanted: HashSet<&str> = r.ids.iter().map(String::as_str).collect();
    let picked: Vec<CloudRow> = inv
        .rows
        .iter()
        .filter(|c| !c.lost && !local.contains(c.key.as_str()))
        .filter(|c| r.all || wanted.contains(c.id.as_str()))
        .cloned()
        .collect();
    if picked.is_empty() {
        return Err(bad("Nada para baixar: já está tudo neste servidor"));
    }
    let bytes = picked.iter().map(|c| c.size as u64).sum();
    let cancel = begin(&s, "restore", picked.len(), bytes)?;
    tokio::spawn(async move {
        let mut ok = 0;
        for c in picked {
            if cancel.load(Ordering::SeqCst) {
                break;
            }
            progress(&s, |j| j.current = c.name.clone());
            match restore_one(&s, &c, &cancel).await {
                Ok(()) => ok += 1,
                Err(e) if e == "cancelado" => break,
                Err(e) => fail(&s, &c.name, &e),
            }
            progress(&s, |j| j.done += 1);
        }
        s.scan.notify_one();
        let cancelled = cancel.load(Ordering::SeqCst);
        finish(
            &s,
            format!("{ok} item(ns) de volta neste servidor"),
            cancelled,
        );
    });
    Ok(Json(serde_json::json!({"ok":true})))
}

async fn restore_one(s: &AppState, c: &CloudRow, cancel: &AtomicBool) -> CloudResult<()> {
    // The library row (if any) knows the current place of the file.
    let media_path: Option<Option<String>> = match &c.media_id {
        Some(mid) => {
            s.db.lock()
                .unwrap()
                .query_row("SELECT path FROM media WHERE id=?", [mid], |r| r.get(0))
                .optional()
                .map_err(|e| e.to_string())?
        }
        None => None,
    };
    let rel = media_path
        .clone()
        .flatten()
        .unwrap_or_else(|| c.path.clone());
    if drive::resolve(s, &rel).is_none() {
        return Err("caminho inválido".into());
    }
    let tmp = s.root.join(TMP_DIR).join(id());
    struct Tmp(PathBuf);
    impl Drop for Tmp {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.0);
        }
    }
    let _tmp = Tmp(tmp.clone());
    let mut out = tokio::fs::File::create(&tmp)
        .await
        .map_err(|_| "sem espaço ou sem permissão no servidor".to_string())?;
    let mut hasher = Md5::new();
    let mut got = 0u64;
    let mut attempt = 0u32;
    // Download with resume (Range) after network errors.
    'outer: loop {
        let token = access_token(s, attempt > 0).await?;
        let mut req = s
            .http
            .get(format!("{API}/files/{}", c.drive_id))
            .query(&[("alt", "media")])
            .bearer_auth(token);
        if got > 0 {
            req = req.header(header::RANGE, format!("bytes={got}-"));
        }
        let r = match req.send().await {
            Ok(r) => r,
            Err(e) => {
                attempt += 1;
                if attempt > 5 {
                    return Err(net_error(e));
                }
                tokio::time::sleep(Duration::from_secs(2u64 << attempt)).await;
                continue;
            }
        };
        match r.status().as_u16() {
            200 if got > 0 => {
                // Server ignored Range: start over.
                out.set_len(0).await.map_err(|e| e.to_string())?;
                out.seek(std::io::SeekFrom::Start(0))
                    .await
                    .map_err(|e| e.to_string())?;
                progress(s, |j| j.bytes_done = j.bytes_done.saturating_sub(got));
                got = 0;
                hasher = Md5::new();
            }
            200 | 206 => {}
            404 => {
                let _ =
                    s.db.lock()
                        .unwrap()
                        .execute("UPDATE cloud SET lost=1 WHERE id=?", [&c.id]);
                return Err("o arquivo não existe mais no Google Drive".into());
            }
            401 if attempt == 0 => {
                attempt += 1;
                continue;
            }
            code => {
                attempt += 1;
                if attempt > 5 {
                    return Err(format!("o Google Drive respondeu {code}"));
                }
                tokio::time::sleep(Duration::from_secs(2u64 << attempt)).await;
                continue;
            }
        }
        let mut r = r;
        loop {
            if cancel.load(Ordering::SeqCst) {
                return Err("cancelado".into());
            }
            match r.chunk().await {
                Ok(Some(bytes)) => {
                    hasher.update(&bytes);
                    out.write_all(&bytes)
                        .await
                        .map_err(|_| "sem espaço no servidor".to_string())?;
                    got += bytes.len() as u64;
                    progress(s, |j| j.bytes_done += bytes.len() as u64);
                }
                Ok(None) => break 'outer,
                Err(e) => {
                    attempt += 1;
                    if attempt > 5 {
                        return Err(net_error(e));
                    }
                    tokio::time::sleep(Duration::from_secs(2u64 << attempt.min(5))).await;
                    continue 'outer;
                }
            }
        }
    }
    out.flush().await.map_err(|e| e.to_string())?;
    out.sync_all().await.map_err(|e| e.to_string())?;
    drop(out);
    if got as i64 != c.size || hex::encode(hasher.finalize()) != c.md5 {
        return Err("o arquivo baixado não confere com a cópia original".into());
    }
    let st = s.clone();
    let c2 = c.clone();
    let tmp2 = tmp.clone();
    let has_row = media_path.is_some();
    tokio::task::spawn_blocking(move || place_restored(&st, &c2, &tmp2, &rel, has_row))
        .await
        .map_err(|e| e.to_string())?
}

fn place_restored(
    s: &AppState,
    c: &CloudRow,
    tmp: &std::path::Path,
    rel: &str,
    has_row: bool,
) -> CloudResult<()> {
    {
        let f = std::fs::File::options()
            .write(true)
            .open(tmp)
            .map_err(|e| e.to_string())?;
        let when = std::time::UNIX_EPOCH + Duration::from_secs(c.mtime.max(0) as u64);
        let _ = f.set_modified(when);
    }
    let guard = s.scanning.lock().unwrap_or_else(|e| e.into_inner());
    let wanted = drive::resolve(s, rel).ok_or("caminho inválido")?;
    let dir = wanted.parent().ok_or("caminho inválido")?.to_path_buf();
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let target = if wanted.exists() {
        let name = wanted
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();
        drive::unique_target(&dir, &name)
    } else {
        wanted
    };
    std::fs::rename(tmp, &target).map_err(|e| e.to_string())?;
    let new_rel = drive::relative(s, &target).ok_or("caminho inválido")?;
    let mtime = drive::mtime_of(&target);
    let db = s.db.lock().unwrap();
    if has_row {
        let mid = c.media_id.as_deref().unwrap_or_default();
        db.execute(
            "UPDATE media SET cloud_only=0,path=?,mtime=?,size=? WHERE id=?",
            params![new_rel, mtime, c.size, mid],
        )
        .map_err(|e| e.to_string())?;
        db.execute(
            "UPDATE cloud SET mtime=?,path=? WHERE id=?",
            params![mtime, new_rel, c.id],
        )
        .map_err(|e| e.to_string())?;
    } else if c.kind == "file" {
        db.execute(
            "UPDATE cloud SET key=?,path=?,mtime=? WHERE id=?",
            params![format!("f:{new_rel}"), new_rel, mtime, c.id],
        )
        .map_err(|e| e.to_string())?;
    } else {
        // The library entry was deleted; the scanner re-creates it. Link
        // the copy to the new entry so the next sync does not upload again.
        drop(db);
        drop(guard);
        let _ = drive::scan(s);
        let db = s.db.lock().unwrap();
        let mid: Option<String> = db
            .query_row("SELECT id FROM media WHERE path=?", [&new_rel], |r| {
                r.get(0)
            })
            .optional()
            .map_err(|e| e.to_string())?;
        if let Some(mid) = mid {
            db.execute(
                "UPDATE cloud SET key=?,media_id=?,path=?,mtime=? WHERE id=?",
                params![format!("m:{mid}"), mid, new_rel, mtime, c.id],
            )
            .map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

// ------------------------------------------------------- remove from Drive ---

pub(crate) async fn remove(
    State(s): State<AppState>,
    h: HeaderMap,
    Path(cid): Path<String>,
) -> ApiResult<Json<serde_json::Value>> {
    admin(&s, &h)?;
    if s.cloud.lock().unwrap().job.running {
        return Err(bad("Espere a tarefa atual terminar"));
    }
    let c =
        s.db.lock()
            .unwrap()
            .query_row(
                &format!("SELECT {ROW_COLS} FROM cloud WHERE id=?"),
                [&cid],
                row,
            )
            .map_err(|_| error(StatusCode::NOT_FOUND, "Cópia não encontrada"))?;
    // To the Drive trash: recoverable there for 30 days.
    match call(
        &s,
        reqwest::Method::PATCH,
        &format!("{API}/files/{}", c.drive_id),
        &[("fields", "id")],
        Some(&serde_json::json!({"trashed": true})),
    )
    .await
    {
        Ok(_) => {}
        Err(e) if e == "not_found" => {}
        Err(e) => return Err(bad(&e)),
    }
    let db = s.db.lock().unwrap();
    db.execute("DELETE FROM cloud WHERE id=?", [&cid])?;
    // A library entry that existed only in Drive is gone now.
    if let Some(mid) = &c.media_id {
        db.execute("DELETE FROM media WHERE id=? AND cloud_only=1", [mid])?;
        let _ = std::fs::remove_file(crate::thumb_path(&s.root, mid));
    }
    Ok(Json(serde_json::json!({"ok":true})))
}

// ------------------------------------------------------------- browsing ---

#[derive(Serialize)]
struct CloudItem {
    id: String,
    media_id: Option<String>,
    name: String,
    kind: String,
    mime: String,
    size: i64,
    genre: String,
    path: String,
    created: i64,
    uploaded: i64,
    /// A local copy exists on this server.
    here: bool,
    lost: bool,
    collections: Vec<String>,
}

/// What lives in Drive, filtered by the same per-profile rules as the
/// library. Plain files (Arquivos) only for profiles with drive access.
pub(crate) async fn items(
    State(s): State<AppState>,
    h: HeaderMap,
) -> ApiResult<Json<Vec<serde_json::Value>>> {
    let u = authenticate(&s, &h)?;
    let st = s.clone();
    let out = tokio::task::spawn_blocking(move || list_items(&st, &u))
        .await
        .map_err(|_| bad("Falha ao listar"))??;
    Ok(Json(out))
}

fn list_items(s: &AppState, u: &User) -> rusqlite::Result<Vec<serde_json::Value>> {
    let db = s.db.lock().unwrap();
    let mut q = db.prepare(
        "SELECT c.id,c.media_id,coalesce(m.name,c.name),c.kind,c.mime,c.size,coalesce(m.genre,c.genre),
                coalesce(m.path,c.path),c.created,c.uploaded,c.lost,
                m.id IS NOT NULL AND m.cloud_only=0 AND m.size=c.size AND m.mtime=c.mtime,
                c.path,c.mtime,m.id IS NULL
         FROM cloud c LEFT JOIN media m ON m.id=c.media_id
         ORDER BY c.created DESC,c.id",
    )?;
    let rows = q
        .query_map([], |r| {
            Ok((
                CloudItem {
                    id: r.get(0)?,
                    media_id: r.get(1)?,
                    name: r.get(2)?,
                    kind: r.get(3)?,
                    mime: r.get(4)?,
                    size: r.get(5)?,
                    genre: r.get(6)?,
                    path: r.get(7)?,
                    created: r.get(8)?,
                    uploaded: r.get(9)?,
                    lost: r.get(10)?,
                    here: r.get(11)?,
                    collections: vec![],
                },
                r.get::<_, String>(12)?,
                r.get::<_, i64>(13)?,
                r.get::<_, bool>(14)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    let files = u.admin || u.can_upload;
    let mut members = db.prepare("SELECT collection_id FROM collection_items WHERE media_id=?")?;
    let mut out = vec![];
    for (mut item, path, mtime, orphan) in rows {
        // Deleted from the library: its collection rules are gone too.
        if orphan && item.kind != "file" && !u.admin {
            continue;
        }
        if item.kind == "file" {
            if !files {
                continue;
            }
            item.here = drive::resolve(s, &path)
                .and_then(|p| std::fs::metadata(&p).ok().map(|m| (m, p)))
                .is_some_and(|(m, p)| m.len() as i64 == item.size && drive::mtime_of(&p) == mtime);
        }
        if let Some(mid) = &item.media_id {
            item.collections = members
                .query_map([mid], |r| r.get(0))?
                .collect::<Result<_, _>>()?;
        }
        if visible(u, &item.genre, &item.collections) {
            out.push(serde_json::to_value(item).unwrap());
        }
    }
    Ok(out)
}

fn visible_row(s: &AppState, u: &User, cid: &str) -> ApiResult<CloudRow> {
    let c =
        s.db.lock()
            .unwrap()
            .query_row(
                &format!("SELECT {ROW_COLS} FROM cloud WHERE id=?"),
                [cid],
                row,
            )
            .map_err(|_| error(StatusCode::NOT_FOUND, "Arquivo não encontrado"))?;
    if c.kind == "file" {
        if !u.admin && !u.can_upload {
            return Err(error(StatusCode::NOT_FOUND, "Arquivo não encontrado"));
        }
        return Ok(c);
    }
    let (genre, collections) = match &c.media_id {
        Some(mid) => {
            let db = s.db.lock().unwrap();
            let genre: Option<String> = db
                .query_row("SELECT genre FROM media WHERE id=?", [mid], |r| r.get(0))
                .optional()?;
            // Deleted from the library: its collections (and so its
            // restrictions) are gone, so only the administrator sees it.
            let Some(genre) = genre else {
                if u.admin {
                    return Ok(c);
                }
                return Err(error(StatusCode::NOT_FOUND, "Arquivo não encontrado"));
            };
            let cols = db
                .prepare("SELECT collection_id FROM collection_items WHERE media_id=?")?
                .query_map([mid], |r| r.get(0))?
                .collect::<Result<Vec<String>, _>>()?;
            (genre, cols)
        }
        None if u.admin => return Ok(c),
        None => return Err(error(StatusCode::NOT_FOUND, "Arquivo não encontrado")),
    };
    if !visible(u, &genre, &collections) {
        return Err(error(StatusCode::NOT_FOUND, "Arquivo não encontrado"));
    }
    Ok(c)
}

#[derive(Deserialize)]
pub(crate) struct StreamQuery {
    download: Option<bool>,
}

pub(crate) async fn item_stream(
    State(s): State<AppState>,
    h: HeaderMap,
    Path(cid): Path<String>,
    Query(q): Query<StreamQuery>,
) -> ApiResult<Response> {
    let u = authenticate(&s, &h)?;
    let c = visible_row(&s, &u, &cid)?;
    let name = c.path.rsplit('/').next().unwrap_or(&c.name).to_string();
    // Plain files can be anything (HTML, scripts): never render them inside
    // the app's origin, always hand them over as a download.
    let file = c.kind == "file";
    proxy(
        &s,
        &c.drive_id,
        if file {
            "application/octet-stream"
        } else {
            &c.mime
        },
        &h,
        (file || q.download.unwrap_or(false)).then_some(name.as_str()),
    )
    .await
}

pub(crate) async fn item_thumbnail(
    State(s): State<AppState>,
    h: HeaderMap,
    Path(cid): Path<String>,
) -> ApiResult<Response> {
    let u = authenticate(&s, &h)?;
    let c = visible_row(&s, &u, &cid)?;
    if let Some(mid) = &c.media_id {
        let thumb = crate::thumb_path(&s.root, mid);
        if thumb.exists() {
            let bytes = tokio::fs::read(thumb).await?;
            return Ok(([(header::CONTENT_TYPE, "image/jpeg")], bytes).into_response());
        }
        // Original still on this server: build the thumbnail from it instead
        // of downloading the full photo from Drive.
        let local: bool =
            s.db.lock()
                .unwrap()
                .query_row("SELECT cloud_only=0 FROM media WHERE id=?", [mid], |r| {
                    r.get(0)
                })
                .unwrap_or(false);
        if local {
            return Ok(
                axum::response::Redirect::temporary(&format!("/api/media/{mid}/thumbnail"))
                    .into_response(),
            );
        }
    }
    Ok(
        axum::response::Redirect::temporary(&format!("/api/cloud/items/{cid}/stream"))
            .into_response(),
    )
}

pub(crate) async fn item_details(
    State(s): State<AppState>,
    h: HeaderMap,
    Path(cid): Path<String>,
) -> ApiResult<Json<serde_json::Value>> {
    let u = authenticate(&s, &h)?;
    let c = visible_row(&s, &u, &cid)?;
    Ok(Json(serde_json::json!({
        "id": c.id, "name": c.name, "kind": c.kind, "mime": c.mime, "size": c.size,
        "genre": "", "created": 0, "path": format!("{ROOT_NAME}/{}", c.path),
        "collections": [], "photo": null,
    })))
}

/// Stream a Drive file through this server, passing Range through so video
/// seeking works. Used for items whose only copy is in Drive.
pub(crate) async fn proxy(
    s: &AppState,
    drive_id: &str,
    mime: &str,
    h: &HeaderMap,
    download: Option<&str>,
) -> ApiResult<Response> {
    let mut refreshed = false;
    loop {
        let token = access_token(s, refreshed)
            .await
            .map_err(|e| error(StatusCode::SERVICE_UNAVAILABLE, &e))?;
        let mut req = s
            .http
            .get(format!("{API}/files/{drive_id}"))
            .query(&[("alt", "media")])
            .bearer_auth(token);
        if let Some(r) = h.get(header::RANGE) {
            req = req.header(header::RANGE, r.as_bytes());
        }
        let r = req
            .send()
            .await
            .map_err(|e| error(StatusCode::BAD_GATEWAY, &net_error(e)))?;
        let status = r.status().as_u16();
        if status == 401 && !refreshed {
            refreshed = true;
            continue;
        }
        if status == 404 {
            return Err(error(
                StatusCode::NOT_FOUND,
                "O arquivo não existe mais no Google Drive",
            ));
        }
        if !(200..300).contains(&status) && status != 416 {
            return Err(error(
                StatusCode::BAD_GATEWAY,
                "O Google Drive não entregou o arquivo. Tente de novo",
            ));
        }
        let mut res = Response::builder()
            .status(status)
            .header(header::CONTENT_TYPE, mime)
            .header(header::ACCEPT_RANGES, "bytes");
        for k in [header::CONTENT_LENGTH, header::CONTENT_RANGE] {
            if let Some(v) = r.headers().get(&k) {
                res = res.header(k, v.as_bytes());
            }
        }
        let disposition = match download {
            Some(name) => format!(
                "attachment; filename=\"download\"; filename*=UTF-8''{}",
                percent_encode(name)
            ),
            None => "inline".into(),
        };
        res = res.header(header::CONTENT_DISPOSITION, disposition);
        return res
            .body(Body::from_stream(r.bytes_stream()))
            .map_err(|_| bad("Resposta inválida"));
    }
}

fn percent_encode(v: &str) -> String {
    v.bytes()
        .map(|b| {
            if b.is_ascii_alphanumeric() || b"-._~".contains(&b) {
                (b as char).to_string()
            } else {
                format!("%{b:02X}")
            }
        })
        .collect()
}

/// For library rows that live only in Drive (`media.cloud_only`).
pub(crate) async fn media_proxy(
    s: &AppState,
    media_id: &str,
    mime: &str,
    h: &HeaderMap,
) -> ApiResult<Response> {
    let drive_id: String =
        s.db.lock()
            .unwrap()
            .query_row(
                "SELECT drive_id FROM cloud WHERE key=? AND lost=0",
                [format!("m:{media_id}")],
                |r| r.get(0),
            )
            .map_err(|_| {
                error(
                    StatusCode::NOT_FOUND,
                    "A cópia no Google Drive não foi encontrada",
                )
            })?;
    proxy(s, &drive_id, mime, h, None).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn query_values_are_escaped() {
        assert_eq!(esc("Pai d'Égua\\x"), "Pai d\\'Égua\\\\x");
        assert_eq!(parent_of("Vídeos/Filmes/a.mp4"), "Vídeos/Filmes");
        assert_eq!(parent_of("a.mp4"), "");
        assert_eq!(percent_encode("Fé 1.mp4"), "F%C3%A9%201.mp4");
    }

    #[test]
    fn deleted_library_items_only_for_admin() {
        let d = tempfile::tempdir().unwrap();
        let s = crate::state(d.path().into()).unwrap();
        s.db.lock()
            .unwrap()
            .execute(
                "INSERT INTO cloud(id,key,media_id,path,name,kind,mime,size,mtime,created,md5,drive_id,uploaded) VALUES('c1','m:gone','gone','Vídeos/x.mp4','x.mp4','video','video/mp4',1,1,0,'x','d1',0)",
                [],
            )
            .unwrap();
        let mut u = User {
            id: "u".into(),
            name: "Ana".into(),
            admin: false,
            can_upload: true,
            blocked_genres: vec![],
            blocked_collections: vec!["private".into()],
        };
        assert!(list_items(&s, &u).unwrap().is_empty());
        assert!(visible_row(&s, &u, "c1").is_err());
        u.admin = true;
        assert_eq!(list_items(&s, &u).unwrap().len(), 1);
        assert!(visible_row(&s, &u, "c1").is_ok());
    }

    #[test]
    fn inventory_tracks_backup_state_and_free_space() {
        let d = tempfile::tempdir().unwrap();
        let s = crate::state(d.path().into()).unwrap();
        {
            let db = s.db.lock().unwrap();
            db.execute(
                "INSERT INTO users(id,name,password,admin) VALUES('a','Admin','x',1)",
                [],
            )
            .unwrap();
        }
        let photo = drive::root(&s).join("Fotos/praia.jpg");
        std::fs::write(&photo, b"\xff\xd8\xffjpeg").unwrap();
        std::fs::write(drive::root(&s).join("Arquivos/notas.txt"), b"texto").unwrap();
        drive::scan(&s).unwrap();
        let inv = inventory(&s).unwrap();
        assert_eq!(inv.here.count, 2);
        assert_eq!(inv.pending.count, 2);
        assert_eq!(inv.backed.count, 0);
        // Pretend the photo was uploaded.
        let l = inv
            .locals
            .iter()
            .find(|l| l.kind == "photo")
            .unwrap()
            .clone();
        s.db.lock()
            .unwrap()
            .execute(
                "INSERT INTO cloud(id,key,media_id,path,name,kind,mime,size,mtime,created,md5,drive_id,uploaded) VALUES('c1',?,?,?,?,?,?,?,?,0,'x','d1',0)",
                params![l.key, l.media_id, l.path, l.name, l.kind, l.mime, l.size, l.mtime],
            )
            .unwrap();
        let inv = inventory(&s).unwrap();
        assert_eq!(inv.backed.count, 1);
        assert_eq!(inv.freeable["photo"].count, 1);
        // Free the local copy: the library row stays, hidden from the
        // local view, and the scanner must not delete it.
        free_one(&s, &l).unwrap();
        assert!(!photo.exists());
        drive::scan(&s).unwrap();
        let (n, only): (i64, bool) =
            s.db.lock()
                .unwrap()
                .query_row("SELECT count(*),max(cloud_only) FROM media", [], |r| {
                    Ok((r.get(0)?, r.get(1)?))
                })
                .unwrap();
        assert_eq!((n, only), (1, true));
        let inv = inventory(&s).unwrap();
        assert_eq!(inv.only_cloud.count, 1);
        assert_eq!(inv.here.count, 1);
        // Putting the file back by hand turns it into a local item again.
        std::fs::write(&photo, b"\xff\xd8\xffjpeg").unwrap();
        drive::scan(&s).unwrap();
        let only: bool =
            s.db.lock()
                .unwrap()
                .query_row("SELECT cloud_only FROM media", [], |r| r.get(0))
                .unwrap();
        assert!(!only);
        // A new upload never takes the name of an item that lives only in Drive.
        s.db.lock()
            .unwrap()
            .execute("UPDATE media SET cloud_only=1", [])
            .unwrap();
        std::fs::remove_file(&photo).unwrap();
        assert!(drive::path_taken(&s, &photo));
    }
}
