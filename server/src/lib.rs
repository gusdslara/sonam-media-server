use argon2::{
    password_hash::{rand_core::OsRng, SaltString},
    Argon2, PasswordHash, PasswordHasher, PasswordVerifier,
};
use axum::{
    body::Body,
    extract::{ConnectInfo, DefaultBodyLimit, Multipart, Path, Query, Request, State},
    http::{header, HeaderMap, StatusCode},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{any, get, post, put},
    Json, Router,
};
use rand::Rng;
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::HashMap,
    net::SocketAddr,
    path::PathBuf,
    sync::{Arc, Mutex},
    time::{SystemTime, UNIX_EPOCH},
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tower::ServiceExt;

pub mod cloud;
pub mod drive;
pub mod net;
pub mod photo;

/// Cached thumbnail. The "-o" suffix marks thumbnails generated with EXIF
/// orientation applied; older unrotated caches are simply ignored.
pub(crate) fn thumb_path(root: &std::path::Path, id: &str) -> PathBuf {
    root.join("thumbnails").join(format!("{id}-o.jpg"))
}

#[derive(rust_embed::RustEmbed)]
#[folder = "../dist/"]
struct Assets;
#[derive(Clone)]
pub struct AppState {
    db: Arc<Mutex<Connection>>,
    root: PathBuf,
    setup_token: String,
    attempts: Arc<Mutex<HashMap<String, (i64, u32)>>>,
    uploads: Arc<tokio::sync::Semaphore>,
    converters: Arc<tokio::sync::Semaphore>,
    encoder: Arc<tokio::sync::OnceCell<Option<String>>>,
    thumbnails: Arc<tokio::sync::Semaphore>,
    dav: Arc<dav_server::DavHandler>,
    dav_auth: Arc<Mutex<HashMap<String, (String, i64)>>>,
    scan: Arc<tokio::sync::Notify>,
    scanning: Arc<Mutex<()>>,
    net: Arc<Mutex<net::NetState>>,
    tunnel: Arc<tokio::sync::Mutex<Option<net::TunnelHandle>>>,
    cloud: Arc<Mutex<cloud::CloudState>>,
    http: reqwest::Client,
}
type ApiResult<T> = Result<T, ApiError>;
#[derive(Debug)]
pub struct ApiError(StatusCode, String);
impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.0, Json(serde_json::json!({"error":self.1}))).into_response()
    }
}
impl From<rusqlite::Error> for ApiError {
    fn from(e: rusqlite::Error) -> Self {
        eprintln!("database: {e}");
        error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "Não foi possível acessar a biblioteca",
        )
    }
}
impl From<std::io::Error> for ApiError {
    fn from(e: std::io::Error) -> Self {
        eprintln!("storage: {e}");
        error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "Não foi possível acessar o armazenamento",
        )
    }
}
fn error(s: StatusCode, m: &str) -> ApiError {
    ApiError(s, m.into())
}
fn bad(m: &str) -> ApiError {
    error(StatusCode::BAD_REQUEST, m)
}
fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64
}
fn id() -> String {
    uuid::Uuid::new_v4().to_string()
}
fn token() -> String {
    format!("{}{}", id(), id())
}
fn digest(s: &str) -> String {
    hex::encode(Sha256::digest(s.as_bytes()))
}
fn clean(s: &str, max: usize) -> ApiResult<String> {
    let s = s.trim();
    if s.is_empty() || s.len() > max || s.chars().any(char::is_control) {
        Err(bad("Texto vazio, muito longo ou inválido"))
    } else {
        Ok(s.into())
    }
}

#[derive(Clone, Serialize, Deserialize, Debug)]
struct User {
    id: String,
    name: String,
    admin: bool,
    can_upload: bool,
    blocked_genres: Vec<String>,
    blocked_collections: Vec<String>,
}
fn user_row(r: &rusqlite::Row) -> rusqlite::Result<User> {
    Ok(User {
        id: r.get(0)?,
        name: r.get(1)?,
        admin: r.get(2)?,
        can_upload: r.get(3)?,
        blocked_genres: serde_json::from_str(&r.get::<_, String>(4)?).unwrap_or_default(),
        blocked_collections: serde_json::from_str(&r.get::<_, String>(5)?).unwrap_or_default(),
    })
}
const USER_COLS: &str = "u.id,u.name,u.admin,u.can_upload,u.blocked_genres,u.blocked_collections";
fn authenticate(s: &AppState, h: &HeaderMap) -> ApiResult<User> {
    let bearer = h
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "));
    let cookie = h
        .get(header::COOKIE)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| {
            v.split(';')
                .find_map(|p| p.trim().strip_prefix("hs_session="))
        });
    let t = bearer
        .or(cookie)
        .ok_or(error(StatusCode::UNAUTHORIZED, "Entre na sua biblioteca"))?;
    s.db.lock().unwrap().query_row(&format!("SELECT {USER_COLS} FROM users u JOIN sessions s ON s.user_id=u.id WHERE s.token=? AND s.expires>?"), params![digest(t),now()], user_row).map_err(|_|error(StatusCode::UNAUTHORIZED,"Sessão expirada. Entre novamente"))
}
fn admin(s: &AppState, h: &HeaderMap) -> ApiResult<User> {
    let u = authenticate(s, h)?;
    if !u.admin {
        return Err(error(
            StatusCode::FORBIDDEN,
            "Apenas o administrador pode fazer isso",
        ));
    }
    Ok(u)
}
fn throttle(s: &AppState, key: String) -> ApiResult<()> {
    let mut map = s.attempts.lock().unwrap();
    map.retain(|_, v| now() - v.0 < 60);
    let v = map.entry(key).or_insert((now(), 0));
    v.1 += 1;
    if v.1 > 8 {
        return Err(error(
            StatusCode::TOO_MANY_REQUESTS,
            "Muitas tentativas. Aguarde um minuto",
        ));
    }
    Ok(())
}
fn session(s: &AppState, uid: &str, h: &HeaderMap) -> ApiResult<(HeaderMap, String)> {
    let t = token();
    let db = s.db.lock().unwrap();
    db.execute("DELETE FROM sessions WHERE expires<?", [now()])?;
    db.execute(
        "INSERT INTO sessions VALUES(?,?,?)",
        params![digest(&t), uid, now() + 86400 * 30],
    )?;
    let secure = h
        .get(header::ORIGIN)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.starts_with("https://"));
    let mut headers = HeaderMap::new();
    headers.insert(
        header::SET_COOKIE,
        format!(
            "hs_session={t}; HttpOnly; SameSite=Strict; Path=/; Max-Age=2592000{}",
            if secure { "; Secure" } else { "" }
        )
        .parse()
        .unwrap(),
    );
    Ok((headers, t))
}

pub(crate) fn setting(s: &AppState, key: &str) -> Option<String> {
    s.db.lock()
        .unwrap()
        .query_row("SELECT value FROM settings WHERE key=?", [key], |r| {
            r.get(0)
        })
        .ok()
}
fn set_setting(s: &AppState, key: &str, value: &str) -> ApiResult<()> {
    s.db.lock().unwrap().execute(
        "INSERT INTO settings VALUES(?,?) ON CONFLICT(key) DO UPDATE SET value=excluded.value",
        params![key, value],
    )?;
    Ok(())
}
fn migrate(db: &Connection) -> rusqlite::Result<()> {
    let has_path = db
        .prepare("SELECT 1 FROM pragma_table_info('media') WHERE name='path'")?
        .exists([])?;
    if !has_path {
        db.execute_batch(
            "ALTER TABLE media ADD COLUMN path TEXT;
             ALTER TABLE media ADD COLUMN mtime INTEGER NOT NULL DEFAULT 0;",
        )?;
    }
    db.execute_batch(
        "CREATE UNIQUE INDEX IF NOT EXISTS media_path ON media(path) WHERE path IS NOT NULL;",
    )?;
    cloud::migrate(db)
}

/// `SONAM_<key>`, falling back to the pre-rename `HOMESTREAM_<key>` so
/// existing installs (service files, compose, scripts) keep working.
pub fn env_var(key: &str) -> Result<String, std::env::VarError> {
    std::env::var(format!("SONAM_{key}")).or_else(|_| std::env::var(format!("HOMESTREAM_{key}")))
}

/// [`env_var`] for values that may not be UTF-8 (paths).
pub fn env_var_os(key: &str) -> Option<std::ffi::OsString> {
    std::env::var_os(format!("SONAM_{key}"))
        .or_else(|| std::env::var_os(format!("HOMESTREAM_{key}")))
}


pub fn state(root: PathBuf) -> Result<AppState, Box<dyn std::error::Error + Send + Sync>> {
    std::fs::create_dir_all(root.join("media"))?;
    std::fs::create_dir_all(root.join("thumbnails"))?;
    drive::prepare(&root)?;
    cloud::prepare(&root)?;
    // Partial uploads are never exposed; remove abandoned files on restart.
    for entry in std::fs::read_dir(root.join("media"))?.flatten() {
        if entry.path().extension().is_some_and(|x| x == "partial") {
            let _ = std::fs::remove_file(entry.path());
        }
    }
    let db = Connection::open(root.join("library.sqlite3"))?;
    db.execute_batch(include_str!("schema.sql"))?;
    migrate(&db)?;
    let setup_token = crate::env_var("SETUP_TOKEN").unwrap_or_else(|_| token());
    if db.query_row("SELECT count(*) FROM users", [], |r| r.get::<_, u32>(0))? == 0 {
        println!("Chave para configurar pela rede: {setup_token}");
    }
    let s = AppState {
        db: Arc::new(Mutex::new(db)),
        setup_token,
        attempts: Default::default(),
        uploads: Arc::new(tokio::sync::Semaphore::new(2)),
        converters: Arc::new(tokio::sync::Semaphore::new(1)),
        encoder: Default::default(),
        thumbnails: Arc::new(tokio::sync::Semaphore::new(2)),
        dav: Arc::new(drive::handler(&root)),
        dav_auth: Default::default(),
        scan: Default::default(),
        scanning: Default::default(),
        net: Default::default(),
        tunnel: Default::default(),
        cloud: Default::default(),
        http: cloud::client(),
        root,
    };
    drive::adopt_uploads(&s);
    Ok(s)
}
pub async fn serve(
    root: PathBuf,
    bind: &str,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let s = state(root)?;
    let listener = tokio::net::TcpListener::bind(bind).await?;
    run(s, listener, async {
        let _ = tokio::signal::ctrl_c().await;
    })
    .await
}
/// Serve on an already bound listener, with the background services
/// (watched folder, `.local` name, optional public link), until `shutdown`.
pub async fn run(
    s: AppState,
    listener: tokio::net::TcpListener,
    shutdown: impl std::future::Future<Output = ()> + Send + 'static,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let port = listener.local_addr()?.port();
    s.net.lock().unwrap().port = port;
    println!(
        "Sonam em http://localhost:{port} — dados em {}",
        s.root.display()
    );
    let scanner = tokio::spawn(drive::scanner(s.clone()));
    // Short address: also answer on port 80 when it is free and allowed, so
    // `http://sonam` and `\\sonam\dav` work without ":8096".
    // Phones and non-root Linux cannot open it; then only the main port is used.
    let short = if port != 80 && crate::env_var("PORT80").as_deref() != Ok("off") {
        match tokio::net::TcpListener::bind("0.0.0.0:80").await {
            Ok(l) => {
                s.net.lock().unwrap().short = true;
                let app = router(s.clone()).into_make_service_with_connect_info::<SocketAddr>();
                Some(tokio::spawn(async move {
                    let _ = axum::serve(l, app).await;
                }))
            }
            Err(_) => None,
        }
    } else {
        None
    };
    let st = s.clone();
    let _ = tokio::task::spawn_blocking(move || net::start_mdns(&st)).await;
    if setting(&s, "tunnel").as_deref() == Some("true") {
        tokio::spawn(net::start_tunnel(s.clone()));
    }
    for a in net::addresses(&s) {
        println!("  {}", a.url);
    }
    let result = axum::serve(
        listener,
        router(s.clone()).into_make_service_with_connect_info::<SocketAddr>(),
    )
    .with_graceful_shutdown(shutdown)
    .await;
    scanner.abort();
    if let Some(task) = short {
        task.abort();
    }
    net::stop_tunnel(&s).await;
    net::stop_mdns(&s);
    result?;
    Ok(())
}
pub fn router(s: AppState) -> Router {
    Router::new()
        .route("/api/status", get(status))
        .route("/api/setup", post(setup))
        .route("/api/login", post(login))
        .route("/api/logout", post(logout))
        .route("/api/me", get(me))
        .route("/api/users", get(users).post(create_user))
        .route("/api/users/{id}", put(update_user))
        .route(
            "/api/media",
            get(list_media)
                .post(upload)
                // 64 GiB; 32-bit phones (armv7) cap at the largest usize (~4 GiB).
                .layer(DefaultBodyLimit::max(
                    usize::try_from(64u64 << 30).unwrap_or(usize::MAX),
                )),
        )
        .route("/api/media/{id}", put(update_media).delete(delete_media))
        .route("/api/media/{id}/stream", get(stream))
        .route("/api/media/{id}/compatible", get(compatible))
        .route("/api/media/{id}/thumbnail", get(thumbnail))
        .route("/api/media/{id}/details", get(details))
        .route("/api/media/{id}/progress", put(progress))
        .route("/api/collections", get(collections).post(create_collection))
        .route(
            "/api/collections/{id}/items/{media}",
            put(add_item).delete(remove_item),
        )
        .route("/api/pair", post(pair_start))
        .route("/api/pair/approve", post(pair_approve))
        .route("/api/pair/{id}", post(pair_poll))
        .route("/api/settings", get(settings).put(save_settings))
        .route("/api/connect", get(connect))
        .route("/api/drive/windows", get(drive_windows))
        .route("/api/drive/scan", post(drive_scan))
        .route("/api/drive/list", get(drive::list))
        .route("/api/cloud", get(cloud::status))
        .route("/api/cloud/summary", get(cloud::summary))
        .route("/api/cloud/config", put(cloud::save_config))
        .route(
            "/api/cloud/connect",
            post(cloud::connect_start).delete(cloud::connect_cancel),
        )
        .route("/api/cloud/disconnect", post(cloud::disconnect))
        .route("/api/cloud/sync", post(cloud::sync))
        .route("/api/cloud/cancel", post(cloud::cancel))
        .route("/api/cloud/free", post(cloud::free))
        .route("/api/cloud/restore", post(cloud::restore))
        .route("/api/cloud/items", get(cloud::items))
        .route(
            "/api/cloud/items/{id}",
            axum::routing::delete(cloud::remove),
        )
        .route("/api/cloud/items/{id}/stream", get(cloud::item_stream))
        .route(
            "/api/cloud/items/{id}/thumbnail",
            get(cloud::item_thumbnail),
        )
        .route("/api/cloud/items/{id}/details", get(cloud::item_details))
        .route("/dav", any(drive::dav))
        .route("/dav/", any(drive::dav))
        .route("/dav/{*path}", any(drive::dav))
        .fallback(static_asset)
        .layer(DefaultBodyLimit::max(32 * 1024))
        .layer(middleware::from_fn_with_state(s.clone(), security))
        .with_state(s)
}
async fn security(State(s): State<AppState>, req: Request, next: Next) -> Response {
    let host = req
        .headers()
        .get(header::HOST)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();
    if !net::host_allowed(&s, &host) {
        return error(StatusCode::FORBIDDEN, "Host não autorizado").into_response();
    }
    let dav = req.uri().path() == "/dav" || req.uri().path().starts_with("/dav/");
    if let Some(origin) = req
        .headers()
        .get(header::ORIGIN)
        .and_then(|v| v.to_str().ok())
    {
        if origin != format!("http://{host}") && origin != format!("https://{host}") {
            return error(StatusCode::FORBIDDEN, "Origem não autorizada").into_response();
        }
    }
    if req
        .headers()
        .get("sec-fetch-site")
        .is_some_and(|v| v == "cross-site")
    {
        return error(StatusCode::FORBIDDEN, "Origem não autorizada").into_response();
    }
    let mut res = next.run(req).await;
    let h = res.headers_mut();
    h.insert("x-content-type-options", "nosniff".parse().unwrap());
    h.insert("referrer-policy", "no-referrer".parse().unwrap());
    // Drive files can be anything (HTML included): never let them run script
    // or share the app's origin when opened in a browser.
    let csp = if dav {
        "sandbox; default-src 'none'; img-src 'self'; media-src 'self'; style-src 'unsafe-inline'; frame-ancestors 'none'"
    } else {
        "default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'; img-src 'self' blob: data:; media-src 'self' blob:; connect-src 'self'; frame-ancestors 'none'; base-uri 'none'; form-action 'self'"
    };
    h.insert("content-security-policy", csp.parse().unwrap());
    h.insert(header::CACHE_CONTROL, "no-store".parse().unwrap());
    res
}
async fn static_asset(req: Request) -> Response {
    if req.method() == axum::http::Method::OPTIONS && req.uri().path() == "/" {
        return drive::root_options();
    }
    if req.method() != axum::http::Method::GET {
        return StatusCode::NOT_FOUND.into_response();
    }
    let p = req.uri().path().trim_start_matches('/');
    if p.starts_with("api/") {
        return StatusCode::NOT_FOUND.into_response();
    }
    let p = if p.is_empty() { "index.html" } else { p };
    match Assets::get(p) {
        Some(a) => (
            [(
                header::CONTENT_TYPE,
                mime_guess::from_path(p).first_or_octet_stream().to_string(),
            )],
            a.data.to_vec(),
        )
            .into_response(),
        None => StatusCode::NOT_FOUND.into_response(),
    }
}
async fn status(
    State(s): State<AppState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
) -> ApiResult<Json<serde_json::Value>> {
    let setup =
        s.db.lock()
            .unwrap()
            .query_row("SELECT count(*)=0 FROM users", [], |r| r.get::<_, bool>(0))?;
    Ok(Json(
        // `local`: this request comes from the server machine itself, where the
        // first account can be created without the setup key.
        serde_json::json!({"setup":setup,"version":env!("CARGO_PKG_VERSION"),"name":net::name(&s),"local":addr.ip().is_loopback(),"port":s.net.lock().unwrap().port}),
    ))
}
#[derive(Deserialize)]
struct Credentials {
    name: String,
    password: String,
    setup_token: Option<String>,
}
fn valid_password(p: &str) -> ApiResult<()> {
    if p.len() < 10 || p.len() > 256 {
        Err(bad("A senha deve ter entre 10 e 256 caracteres"))
    } else {
        Ok(())
    }
}
async fn hash_password(p: String) -> ApiResult<String> {
    tokio::task::spawn_blocking(move || {
        Argon2::default()
            .hash_password(p.as_bytes(), &SaltString::generate(&mut OsRng))
            .map(|h| h.to_string())
            .map_err(|_| bad("Senha inválida"))
    })
    .await
    .map_err(|_| bad("Erro ao processar senha"))?
}
async fn setup(
    State(s): State<AppState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    h: HeaderMap,
    Json(c): Json<Credentials>,
) -> ApiResult<impl IntoResponse> {
    throttle(&s, addr.ip().to_string())?;
    if !addr.ip().is_loopback() && c.setup_token.as_deref() != Some(&s.setup_token) {
        return Err(error(
            StatusCode::FORBIDDEN,
            "Para configurar pela rede, informe a chave exibida no terminal do servidor",
        ));
    }
    let name = clean(&c.name, 80)?;
    valid_password(&c.password)?;
    let hash = hash_password(c.password).await?;
    let uid = id();
    {
        let mut db = s.db.lock().unwrap();
        let tx = db.transaction()?;
        if tx.query_row("SELECT count(*) FROM users", [], |r| r.get::<_, u32>(0))? > 0 {
            return Err(error(StatusCode::CONFLICT, "Servidor já configurado"));
        }
        tx.execute(
            "INSERT INTO users(id,name,password,admin,can_upload) VALUES(?,?,?,1,1)",
            params![uid, name, hash],
        )?;
        tx.commit()?;
    }
    let (headers, _) = session(&s, &uid, &h)?;
    Ok((headers, Json(serde_json::json!({"ok":true}))))
}
async fn login(
    State(s): State<AppState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    h: HeaderMap,
    Json(c): Json<Credentials>,
) -> ApiResult<impl IntoResponse> {
    throttle(&s, addr.ip().to_string())?;
    let found =
        s.db.lock()
            .unwrap()
            .query_row(
                "SELECT id,password FROM users WHERE name=?",
                [c.name.trim()],
                |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)),
            )
            .ok();
    let (uid,hash)=found.unwrap_or_else(||(String::new(),"$argon2id$v=19$m=19456,t=2,p=1$c29tZXNhbHRzb21lc2FsdA$AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA".into()));
    let valid = tokio::task::spawn_blocking(move || {
        PasswordHash::new(&hash).is_ok_and(|hash| {
            Argon2::default()
                .verify_password(c.password.as_bytes(), &hash)
                .is_ok()
        })
    })
    .await
    .unwrap_or(false);
    if !valid || uid.is_empty() {
        return Err(error(StatusCode::UNAUTHORIZED, "Nome ou senha incorretos"));
    }
    let (headers, _) = session(&s, &uid, &h)?;
    Ok((headers, Json(serde_json::json!({"ok":true}))))
}
async fn logout(State(s): State<AppState>, h: HeaderMap) -> ApiResult<impl IntoResponse> {
    authenticate(&s, &h)?;
    for raw in h
        .get(header::COOKIE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .split(';')
        .filter_map(|v| v.trim().strip_prefix("hs_session="))
    {
        s.db.lock()
            .unwrap()
            .execute("DELETE FROM sessions WHERE token=?", [digest(raw)])?;
    }
    if let Some(t) = h
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
    {
        s.db.lock()
            .unwrap()
            .execute("DELETE FROM sessions WHERE token=?", [digest(t)])?;
    }
    Ok((
        [(
            header::SET_COOKIE,
            "hs_session=; HttpOnly; SameSite=Strict; Path=/; Max-Age=0",
        )],
        Json(serde_json::json!({"ok":true})),
    ))
}
async fn me(State(s): State<AppState>, h: HeaderMap) -> ApiResult<Json<User>> {
    Ok(Json(authenticate(&s, &h)?))
}
async fn users(State(s): State<AppState>, h: HeaderMap) -> ApiResult<Json<Vec<User>>> {
    admin(&s, &h)?;
    let db = s.db.lock().unwrap();
    let mut q = db.prepare(&format!("SELECT {USER_COLS} FROM users u ORDER BY u.name"))?;
    let rows = q.query_map([], user_row)?.collect::<Result<Vec<_>, _>>()?;
    Ok(Json(rows))
}
#[derive(Deserialize)]
struct NewUser {
    name: String,
    password: String,
    can_upload: bool,
    blocked_genres: Vec<String>,
    blocked_collections: Vec<String>,
}
async fn create_user(
    State(s): State<AppState>,
    h: HeaderMap,
    Json(c): Json<NewUser>,
) -> ApiResult<Json<serde_json::Value>> {
    admin(&s, &h)?;
    let name = clean(&c.name, 80)?;
    valid_password(&c.password)?;
    let hash = hash_password(c.password).await?;
    s.db.lock()
        .unwrap()
        .execute(
            "INSERT INTO users VALUES(?,?,?,0,?,?,?)",
            params![
                id(),
                name,
                hash,
                c.can_upload,
                serde_json::to_string(&c.blocked_genres).unwrap(),
                serde_json::to_string(&c.blocked_collections).unwrap()
            ],
        )
        .map_err(|_| bad("Esse nome já está em uso"))?;
    Ok(Json(serde_json::json!({"ok":true})))
}
#[derive(Deserialize)]
struct UserPolicy {
    can_upload: bool,
    blocked_genres: Vec<String>,
    blocked_collections: Vec<String>,
}
async fn update_user(
    State(s): State<AppState>,
    h: HeaderMap,
    Path(uid): Path<String>,
    Json(p): Json<UserPolicy>,
) -> ApiResult<Json<serde_json::Value>> {
    admin(&s, &h)?;
    let n=s.db.lock().unwrap().execute("UPDATE users SET can_upload=?,blocked_genres=?,blocked_collections=? WHERE id=? AND admin=0",params![p.can_upload,serde_json::to_string(&p.blocked_genres).unwrap(),serde_json::to_string(&p.blocked_collections).unwrap(),uid])?;
    if n == 0 {
        return Err(bad("Perfil inexistente ou administrador"));
    }
    Ok(Json(serde_json::json!({"ok":true})))
}
#[derive(Serialize, Deserialize, Clone, Debug)]
struct Media {
    id: String,
    name: String,
    kind: String,
    mime: String,
    size: i64,
    genre: String,
    created: i64,
    seconds: f64,
    collections: Vec<String>,
    /// Relative path inside the network drive when the file came from the
    /// watched folder; `None` for files uploaded through the app.
    path: Option<String>,
    /// Local copy removed after backup; bytes come from Google Drive.
    cloud_only: bool,
    /// Google Drive copy: "ok" (matches), "outdated" (changed since), none.
    backup: Option<String>,
}
/// Columns read by `media_row`; `?1` is the user id for progress.
const MEDIA_SELECT: &str = "SELECT m.id,m.name,m.kind,m.mime,m.size,m.genre,m.created,coalesce(p.seconds,0),m.path,m.cloud_only,CASE WHEN c.id IS NULL THEN NULL WHEN c.lost=0 AND c.size=m.size AND c.mtime=m.mtime THEN 'ok' ELSE 'outdated' END FROM media m LEFT JOIN progress p ON p.media_id=m.id AND p.user_id=?1 LEFT JOIN cloud c ON c.key='m:'||m.id";
fn visible(u: &User, genre: &str, collections: &[String]) -> bool {
    u.admin
        || (!u
            .blocked_genres
            .iter()
            .any(|g| g.to_lowercase() == genre.to_lowercase())
            && !collections
                .iter()
                .any(|c| u.blocked_collections.contains(c)))
}
fn media_one(s: &AppState, u: &User, mid: &str) -> ApiResult<Media> {
    let db = s.db.lock().unwrap();
    let mut m = db
        .query_row(
            &format!("{MEDIA_SELECT} WHERE m.id=?2"),
            params![u.id, mid],
            media_row,
        )
        .map_err(|_| error(StatusCode::NOT_FOUND, "Arquivo não encontrado"))?;
    m.collections = db
        .prepare("SELECT collection_id FROM collection_items WHERE media_id=?")?
        .query_map([mid], |r| r.get(0))?
        .collect::<Result<Vec<String>, _>>()?;
    if !visible(u, &m.genre, &m.collections) {
        return Err(error(StatusCode::NOT_FOUND, "Arquivo não encontrado"));
    }
    Ok(m)
}
/// On-disk location of a media item's bytes.
fn media_file(s: &AppState, m: &Media) -> ApiResult<PathBuf> {
    match &m.path {
        Some(p) => {
            drive::resolve(s, p).ok_or(error(StatusCode::NOT_FOUND, "Arquivo não encontrado"))
        }
        None => Ok(s.root.join("media").join(&m.id)),
    }
}
fn media_row(r: &rusqlite::Row) -> rusqlite::Result<Media> {
    Ok(Media {
        id: r.get(0)?,
        name: r.get(1)?,
        kind: r.get(2)?,
        mime: r.get(3)?,
        size: r.get(4)?,
        genre: r.get(5)?,
        created: r.get(6)?,
        seconds: r.get(7)?,
        collections: vec![],
        path: r.get(8)?,
        cloud_only: r.get(9)?,
        backup: r.get(10)?,
    })
}
#[derive(Deserialize)]
struct Filter {
    q: Option<String>,
    kind: Option<String>,
    collection: Option<String>,
    sort: Option<String>,
}
async fn list_media(
    State(s): State<AppState>,
    h: HeaderMap,
    Query(f): Query<Filter>,
) -> ApiResult<Json<Vec<Media>>> {
    let u = authenticate(&s, &h)?;
    let db = s.db.lock().unwrap();
    // The library shows what is on this server; Drive-only items are listed
    // by the Google Drive view.
    let mut stmt = db.prepare(&format!(
        "{MEDIA_SELECT} WHERE m.cloud_only=0 ORDER BY m.created DESC,m.id"
    ))?;
    let rows = stmt
        .query_map([&u.id], media_row)?
        .collect::<Result<Vec<_>, _>>()?;
    let mut out = Vec::new();
    let search = f.q.unwrap_or_default().to_lowercase();
    for mut m in rows {
        m.collections = db
            .prepare("SELECT collection_id FROM collection_items WHERE media_id=?")?
            .query_map([&m.id], |r| r.get(0))?
            .collect::<Result<Vec<String>, _>>()?;
        if visible(&u, &m.genre, &m.collections)
            && f.kind.as_ref().is_none_or(|k| k == &m.kind)
            && f.collection
                .as_ref()
                .is_none_or(|c| m.collections.contains(c))
            && format!("{} {}", m.name, m.genre)
                .to_lowercase()
                .contains(&search)
        {
            out.push(m);
        }
    }
    if f.sort.as_deref() == Some("name") {
        out.sort_by_key(|m| m.name.to_lowercase());
    }
    Ok(Json(out))
}
// Allowlist of passive media containers. Never accept HTML, SVG or executable files.
fn sniff(bytes: &[u8]) -> Option<(&'static str, &'static str)> {
    if bytes.starts_with(b"\xff\xd8\xff") {
        Some(("photo", "image/jpeg"))
    } else if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some(("photo", "image/png"))
    } else if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        Some(("photo", "image/gif"))
    } else if bytes.starts_with(b"RIFF") && bytes.get(8..12) == Some(b"WEBP") {
        Some(("photo", "image/webp"))
    } else if bytes.starts_with(b"RIFF") && bytes.get(8..12) == Some(b"WAVE") {
        Some(("audio", "audio/wav"))
    } else if bytes.starts_with(b"fLaC") {
        Some(("audio", "audio/flac"))
    } else if bytes.starts_with(b"OggS") {
        Some(("audio", "audio/ogg"))
    } else if bytes.starts_with(b"ID3")
        || bytes.first() == Some(&255) && bytes.get(1).is_some_and(|b| b & 0xe0 == 0xe0)
    {
        Some(("audio", "audio/mpeg"))
    } else if bytes.starts_with(b"\x1aE\xdf\xa3") {
        Some(("video", "video/webm"))
    } else if bytes.get(4..8) == Some(b"ftyp") {
        if bytes
            .get(8..12)
            .is_some_and(|b| b == b"avif" || b == b"avis")
        {
            Some(("photo", "image/avif"))
        } else if bytes.get(8..12) == Some(b"M4A ") {
            Some(("audio", "audio/mp4"))
        } else {
            Some(("video", "video/mp4"))
        }
    } else {
        None
    }
}
struct PartialFile(PathBuf);
impl Drop for PartialFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}
async fn upload(
    State(s): State<AppState>,
    h: HeaderMap,
    mut multipart: Multipart,
) -> ApiResult<Json<Vec<Media>>> {
    let u = authenticate(&s, &h)?;
    if !u.admin && !u.can_upload {
        return Err(error(
            StatusCode::FORBIDDEN,
            "Este perfil não pode enviar arquivos",
        ));
    }
    let _permit = s.uploads.clone().try_acquire_owned().map_err(|_| {
        error(
            StatusCode::TOO_MANY_REQUESTS,
            "Dois envios em andamento. Tente novamente em instantes",
        )
    })?;
    let mut out = vec![];
    while let Some(mut field) = multipart
        .next_field()
        .await
        .map_err(|_| bad("Upload incompleto ou maior que 64 GiB"))?
    {
        if out.len() >= 100 {
            return Err(bad("Envie até 100 arquivos por vez"));
        }
        let name = clean(field.file_name().unwrap_or("Arquivo"), 240)?;
        let mid = id();
        let partial = PartialFile(s.root.join("media").join(format!("{mid}.partial")));
        let mut file = tokio::fs::File::create(&partial.0).await?;
        let mut size = 0i64;
        while let Some(chunk) = field
            .chunk()
            .await
            .map_err(|_| bad("Upload interrompido ou maior que 64 GiB"))?
        {
            size += chunk.len() as i64;
            file.write_all(&chunk).await?;
        }
        file.sync_all().await?;
        drop(file);
        let mut probe = tokio::fs::File::open(&partial.0).await?;
        let mut bytes = [0u8; 64];
        let n = probe.read(&mut bytes).await?;
        drop(probe);
        let (kind, mut mime) = sniff(&bytes[..n]).ok_or(bad(
            "Formato não permitido. Envie uma foto, áudio ou vídeo compatível",
        ))?;
        if mime == "video/webm" && name.to_lowercase().ends_with(".mkv") {
            mime = "video/x-matroska";
        }
        // Uploads live in the drive's type folder (Fotos/Músicas/Vídeos), so the
        // network drive and the library show the same files.
        drive::place_upload(&s, &partial.0, &name, kind, |path, mtime| {
            s.db.lock().unwrap().execute(
                "INSERT INTO media(id,name,kind,mime,size,created,owner,path,mtime) VALUES(?,?,?,?,?,?,?,?,?)",
                params![mid, name, kind, mime, size, now(), u.id, path, mtime],
            )
        })?;
        out.push(media_one(&s, &u, &mid)?);
    }
    Ok(Json(out))
}
async fn stream(
    State(s): State<AppState>,
    h: HeaderMap,
    Path(mid): Path<String>,
    req: Request,
) -> ApiResult<Response> {
    let u = authenticate(&s, &h)?;
    let m = media_one(&s, &u, &mid)?;
    if m.cloud_only {
        return cloud::media_proxy(&s, &m.id, &m.mime, &h).await;
    }
    let mut response = tower_http::services::ServeFile::new(media_file(&s, &m)?)
        .oneshot(req)
        .await
        .unwrap()
        .into_response();
    response
        .headers_mut()
        .insert(header::CONTENT_TYPE, m.mime.parse().unwrap());
    response
        .headers_mut()
        .insert(header::CONTENT_DISPOSITION, "inline".parse().unwrap());
    Ok(response)
}
async fn thumbnail(
    State(s): State<AppState>,
    h: HeaderMap,
    Path(mid): Path<String>,
    req: Request,
) -> ApiResult<Response> {
    let u = authenticate(&s, &h)?;
    let m = media_one(&s, &u, &mid)?;
    if m.kind != "photo" {
        return Err(bad("Este arquivo não é uma imagem"));
    }
    let path = thumb_path(&s.root, &m.id);
    if !path.exists() && m.cloud_only {
        return Ok(
            axum::response::Redirect::temporary(&format!("/api/media/{}/stream", m.id))
                .into_response(),
        );
    }
    if !path.exists() {
        let permit = s
            .thumbnails
            .clone()
            .acquire_owned()
            .await
            .map_err(|_| bad("Servidor encerrando"))?;
        let original = media_file(&s, &m)?;
        let output = path.clone();
        let generated = tokio::task::spawn_blocking(move || {
            let _permit = permit;
            render_thumbnail(original, output)
        })
        .await
        .unwrap_or(false);
        if !generated {
            return Ok(
                axum::response::Redirect::temporary(&format!("/api/media/{}/stream", m.id))
                    .into_response(),
            );
        }
    }
    Ok(tower_http::services::ServeFile::new(path)
        .oneshot(req)
        .await
        .unwrap()
        .into_response())
}
/// Decode a photo and write a 640 px JPEG thumbnail (EXIF rotation applied).
pub(crate) fn render_thumbnail(original: PathBuf, output: PathBuf) -> bool {
    if output.exists() {
        return true;
    }
    let Ok(reader) = image::ImageReader::open(original) else {
        return false;
    };
    let Ok(mut reader) = reader.with_guessed_format() else {
        return false;
    };
    let mut limits = image::Limits::default();
    limits.max_alloc = Some(128 * 1024 * 1024);
    limits.max_image_width = Some(20000);
    limits.max_image_height = Some(20000);
    reader.limits(limits);
    // Phones store portrait photos sideways plus an EXIF rotation tag.
    let Ok(mut decoder) = reader.into_decoder() else {
        return false;
    };
    let orientation = image::ImageDecoder::orientation(&mut decoder).ok();
    let Ok(mut image) = image::DynamicImage::from_decoder(decoder) else {
        return false;
    };
    if let Some(o) = orientation {
        image.apply_orientation(o);
    }
    let thumb = image.thumbnail(640, 640).to_rgb8();
    let temp = output.with_extension(format!("{}.tmp", id()));
    if image::DynamicImage::ImageRgb8(thumb)
        .save_with_format(&temp, image::ImageFormat::Jpeg)
        .is_err()
    {
        let _ = std::fs::remove_file(temp);
        return false;
    }
    let ok = std::fs::rename(&temp, &output).is_ok();
    if !ok {
        let _ = std::fs::remove_file(temp);
    }
    ok
}
/// File facts plus, for photos, EXIF (capture date, camera, GPS). Subject to
/// the same per-profile visibility rules as the file itself.
async fn details(
    State(s): State<AppState>,
    h: HeaderMap,
    Path(mid): Path<String>,
) -> ApiResult<Json<serde_json::Value>> {
    let u = authenticate(&s, &h)?;
    let m = media_one(&s, &u, &mid)?;
    let file = media_file(&s, &m)?;
    let kind = m.kind.clone();
    let local = !m.cloud_only;
    let photo = tokio::task::spawn_blocking(move || {
        (kind == "photo" && local).then(|| photo::details(&file))
    })
    .await
    .map_err(|_| bad("Não foi possível ler os detalhes"))?;
    let collections: Vec<String> = {
        let db = s.db.lock().unwrap();
        let mut q = db.prepare(
            "SELECT c.name FROM collections c JOIN collection_items i ON i.collection_id=c.id WHERE i.media_id=? ORDER BY c.name",
        )?;
        let rows = q
            .query_map([&m.id], |r| r.get(0))?
            .collect::<Result<_, _>>()?;
        rows
    };
    Ok(Json(serde_json::json!({
        "id": m.id,
        "name": m.name,
        "kind": m.kind,
        "mime": m.mime,
        "size": m.size,
        "genre": m.genre,
        "created": m.created,
        "path": m.path,
        "cloud_only": m.cloud_only,
        "backup": m.backup,
        "collections": collections,
        "photo": photo,
    })))
}
#[derive(Deserialize)]
struct EditMedia {
    name: String,
    genre: String,
}
async fn update_media(
    State(s): State<AppState>,
    h: HeaderMap,
    Path(mid): Path<String>,
    Json(m): Json<EditMedia>,
) -> ApiResult<Json<serde_json::Value>> {
    admin(&s, &h)?;
    let name = clean(&m.name, 240)?;
    if m.genre.len() > 80 {
        return Err(bad("Gênero muito longo"));
    }
    s.db.lock().unwrap().execute(
        "UPDATE media SET name=?,genre=? WHERE id=?",
        params![name, m.genre.trim(), mid],
    )?;
    Ok(Json(serde_json::json!({"ok":true})))
}
async fn delete_media(
    State(s): State<AppState>,
    h: HeaderMap,
    Path(mid): Path<String>,
) -> ApiResult<Json<serde_json::Value>> {
    let u = admin(&s, &h)?;
    let m = media_one(&s, &u, &mid)?;
    match tokio::fs::remove_file(media_file(&s, &m)?).await {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(e.into()),
        _ => {}
    }
    let _ = tokio::fs::remove_file(thumb_path(&s.root, &m.id)).await;
    s.db.lock()
        .unwrap()
        .execute("DELETE FROM media WHERE id=?", [&m.id])?;
    Ok(Json(serde_json::json!({"ok":true})))
}
#[derive(Deserialize)]
struct Progress {
    seconds: f64,
}
async fn progress(
    State(s): State<AppState>,
    h: HeaderMap,
    Path(mid): Path<String>,
    Json(p): Json<Progress>,
) -> ApiResult<Json<serde_json::Value>> {
    let u = authenticate(&s, &h)?;
    media_one(&s, &u, &mid)?;
    if !p.seconds.is_finite() || p.seconds < 0.0 {
        return Err(bad("Posição inválida"));
    }
    s.db.lock().unwrap().execute("INSERT INTO progress VALUES(?,?,?,?) ON CONFLICT(user_id,media_id) DO UPDATE SET seconds=excluded.seconds,updated=excluded.updated",params![u.id,mid,p.seconds,now()])?;
    Ok(Json(serde_json::json!({"ok":true})))
}
#[derive(Serialize, Deserialize)]
struct Collection {
    id: String,
    name: String,
    kind: String,
}
async fn collections(State(s): State<AppState>, h: HeaderMap) -> ApiResult<Json<Vec<Collection>>> {
    let u = authenticate(&s, &h)?;
    let db = s.db.lock().unwrap();
    let mut q = db.prepare("SELECT id,name,kind FROM collections ORDER BY created DESC")?;
    let rows = q
        .query_map([], |r| {
            Ok(Collection {
                id: r.get(0)?,
                name: r.get(1)?,
                kind: r.get(2)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(Json(
        rows.into_iter()
            .filter(|c| u.admin || !u.blocked_collections.contains(&c.id))
            .collect(),
    ))
}
#[derive(Deserialize)]
struct NewCollection {
    name: String,
    kind: String,
}
async fn create_collection(
    State(s): State<AppState>,
    h: HeaderMap,
    Json(c): Json<NewCollection>,
) -> ApiResult<Json<Collection>> {
    admin(&s, &h)?;
    let name = clean(&c.name, 100)?;
    if !["video", "audio", "photo"].contains(&c.kind.as_str()) {
        return Err(bad("Tipo de coleção inválido"));
    }
    let cid = id();
    s.db.lock().unwrap().execute(
        "INSERT INTO collections VALUES(?,?,?,?)",
        params![cid, name, c.kind, now()],
    )?;
    Ok(Json(Collection {
        id: cid,
        name,
        kind: c.kind,
    }))
}
async fn add_item(
    State(s): State<AppState>,
    h: HeaderMap,
    Path((cid, mid)): Path<(String, String)>,
) -> ApiResult<Json<serde_json::Value>> {
    let u = admin(&s, &h)?;
    let m = media_one(&s, &u, &mid)?;
    let db = s.db.lock().unwrap();
    let kind = db
        .query_row("SELECT kind FROM collections WHERE id=?", [&cid], |r| {
            r.get::<_, String>(0)
        })
        .map_err(|_| bad("Coleção não encontrada"))?;
    if kind != m.kind {
        return Err(bad("A coleção precisa ser do mesmo tipo do arquivo"));
    }
    db.execute("INSERT OR IGNORE INTO collection_items VALUES(?,?,(SELECT coalesce(max(position),0)+1 FROM collection_items WHERE collection_id=?))",params![cid,mid,cid])?;
    Ok(Json(serde_json::json!({"ok":true})))
}
async fn remove_item(
    State(s): State<AppState>,
    h: HeaderMap,
    Path((cid, mid)): Path<(String, String)>,
) -> ApiResult<Json<serde_json::Value>> {
    admin(&s, &h)?;
    s.db.lock().unwrap().execute(
        "DELETE FROM collection_items WHERE collection_id=? AND media_id=?",
        params![cid, mid],
    )?;
    Ok(Json(serde_json::json!({"ok":true})))
}
async fn pair_start(
    State(s): State<AppState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
) -> ApiResult<Json<serde_json::Value>> {
    throttle(&s, format!("pair:{}", addr.ip()))?;
    let pid = token();
    let pin = format!("{:06}", rand::thread_rng().gen_range(0..1_000_000));
    let db = s.db.lock().unwrap();
    db.execute("DELETE FROM pairing WHERE expires<?", [now()])?;
    db.execute(
        "INSERT INTO pairing VALUES(?,?,NULL,?)",
        params![digest(&pid), pin, now() + 300],
    )
    .map_err(|_| bad("Tente gerar outro código"))?;
    Ok(Json(
        serde_json::json!({"id":pid,"pin":pin,"expires_in":300}),
    ))
}
#[derive(Deserialize)]
struct Pin {
    pin: String,
}
async fn pair_approve(
    State(s): State<AppState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    h: HeaderMap,
    Json(p): Json<Pin>,
) -> ApiResult<Json<serde_json::Value>> {
    let u = authenticate(&s, &h)?;
    throttle(&s, format!("approve:{}", addr.ip()))?;
    let n = s.db.lock().unwrap().execute(
        "UPDATE pairing SET user_id=? WHERE pin=? AND expires>? AND user_id IS NULL",
        params![u.id, p.pin.trim(), now()],
    )?;
    if n != 1 {
        return Err(bad("Código inválido, usado ou expirado"));
    }
    Ok(Json(serde_json::json!({"ok":true})))
}
async fn pair_poll(
    State(s): State<AppState>,
    h: HeaderMap,
    Path(pid): Path<String>,
) -> ApiResult<impl IntoResponse> {
    let uid = {
        let mut db = s.db.lock().unwrap();
        let tx = db.transaction()?;
        let uid = tx
            .query_row(
                "SELECT user_id FROM pairing WHERE id=? AND expires>?",
                params![digest(&pid), now()],
                |r| r.get::<_, Option<String>>(0),
            )
            .map_err(|_| error(StatusCode::GONE, "Código expirado"))?;
        if uid.is_some() {
            tx.execute("DELETE FROM pairing WHERE id=?", [digest(&pid)])?;
        }
        tx.commit()?;
        uid
    };
    if let Some(uid) = uid {
        let (headers, t) = session(&s, &uid, &h)?;
        Ok((
            headers,
            Json(serde_json::json!({"approved":true,"token":t})),
        ))
    } else {
        Ok((
            HeaderMap::new(),
            Json(serde_json::json!({"approved":false})),
        ))
    }
}
#[derive(Deserialize)]
struct Settings {
    gpu: Option<bool>,
    name: Option<String>,
    tunnel: Option<bool>,
    /// Own domain / public address, e.g. "https://casa.duckdns.org" ("" clears).
    public_url: Option<String>,
}
async fn settings(State(s): State<AppState>, h: HeaderMap) -> ApiResult<Json<serde_json::Value>> {
    admin(&s, &h)?;
    let gpu =
        s.db.lock()
            .unwrap()
            .query_row("SELECT value FROM settings WHERE key='gpu'", [], |r| {
                r.get::<_, String>(0)
            })?
            == "true";
    let available = ffmpeg_available().await;
    let used = s.db.lock().unwrap().query_row(
        "SELECT coalesce(sum(size),0) FROM media WHERE cloud_only=0",
        [],
        |r| r.get::<_, i64>(0),
    )?;
    let encoder = if available && gpu {
        detect_encoder(&s).await
    } else {
        None
    };
    Ok(Json(serde_json::json!({
        "gpu": gpu,
        "playback": "direct",
        "transcoding_available": available,
        "encoder": encoder,
        "port": s.net.lock().unwrap().port,
        "used_bytes": used,
        "data_path": s.root.to_string_lossy(),
        "name": net::name(&s),
        "name_error": s.net.lock().unwrap().mdns_error.clone(),
        "public_url": setting(&s, "public_url").unwrap_or_default(),
        "short": s.net.lock().unwrap().short,
        "addresses": net::addresses(&s),
        "tunnel": {
            "available": net::cloudflared_available().await,
            "enabled": setting(&s, "tunnel").as_deref() == Some("true"),
            "status": s.net.lock().unwrap().tunnel.clone(),
        },
        "drive": {
            "path": drive::root(&s).to_string_lossy(),
            "folders": [drive::PHOTOS_DIR, drive::MUSIC_DIR, drive::VIDEOS_DIR],
            "files_folder": drive::FILES_DIR,
        },
    })))
}
async fn save_settings(
    State(s): State<AppState>,
    h: HeaderMap,
    Json(c): Json<Settings>,
) -> ApiResult<Json<serde_json::Value>> {
    admin(&s, &h)?;
    if let Some(gpu) = c.gpu {
        set_setting(&s, "gpu", &gpu.to_string())?;
    }
    if let Some(name) = c.name {
        let name = name.trim().to_lowercase();
        if !net::valid_name(&name) {
            return Err(bad(
                "Use de 1 a 40 letras minúsculas, números ou hífen, sem espaços",
            ));
        }
        if name != net::name(&s) {
            set_setting(&s, "mdns_name", &name)?;
            let st = s.clone();
            tokio::task::spawn_blocking(move || net::start_mdns(&st))
                .await
                .map_err(|_| bad("Não foi possível anunciar o novo nome"))?;
        }
    }
    if let Some(url) = c.public_url {
        let url = url.trim().trim_end_matches('/').to_string();
        if !url.is_empty() && net::parse_public_url(&url).is_none() {
            return Err(bad(
                "Use um endereço como https://minhacasa.duckdns.org ou http://203.0.113.7:8096",
            ));
        }
        set_setting(&s, "public_url", &url)?;
    }
    if let Some(on) = c.tunnel {
        set_setting(&s, "tunnel", &on.to_string())?;
        if on {
            tokio::spawn(net::start_tunnel(s.clone()));
        } else {
            net::stop_tunnel(&s).await;
        }
    }
    Ok(Json(serde_json::json!({"ok":true})))
}
/// Addresses and drive access for any signed-in profile (not only admin).
async fn connect(State(s): State<AppState>, h: HeaderMap) -> ApiResult<Json<serde_json::Value>> {
    let u = authenticate(&s, &h)?;
    Ok(Json(serde_json::json!({
        "name": net::name(&s),
        "port": s.net.lock().unwrap().port,
        "addresses": net::addresses(&s),
        "drive": {
            "allowed": u.admin || u.can_upload,
            "user": u.name,
            "folders": [drive::PHOTOS_DIR, drive::MUSIC_DIR, drive::VIDEOS_DIR],
            "files_folder": drive::FILES_DIR,
        },
    })))
}
#[derive(Deserialize)]
struct DriveScriptQuery {
    url: String,
}
/// Windows helper that maps the drive. Only addresses this server actually
/// announces are accepted, so the download cannot point anywhere else.
async fn drive_windows(
    State(s): State<AppState>,
    h: HeaderMap,
    Query(q): Query<DriveScriptQuery>,
) -> ApiResult<Response> {
    let u = authenticate(&s, &h)?;
    if !u.admin && !u.can_upload {
        return Err(error(
            StatusCode::FORBIDDEN,
            "Este perfil não tem acesso à unidade de rede",
        ));
    }
    let base = q.url.trim_end_matches('/');
    if !net::addresses(&s).iter().any(|a| a.url == base) {
        return Err(bad("Endereço desconhecido"));
    }
    Ok((
        [
            (header::CONTENT_TYPE, "application/octet-stream"),
            (
                header::CONTENT_DISPOSITION,
                "attachment; filename=\"conectar-sonam.cmd\"",
            ),
        ],
        drive::windows_script(&format!("{base}/dav/"), &u.name),
    )
        .into_response())
}
async fn drive_scan(State(s): State<AppState>, h: HeaderMap) -> ApiResult<Json<serde_json::Value>> {
    let u = authenticate(&s, &h)?;
    if !u.admin && !u.can_upload {
        return Err(error(
            StatusCode::FORBIDDEN,
            "Sem acesso aos arquivos do servidor",
        ));
    }
    let st = s.clone();
    let r = tokio::task::spawn_blocking(move || drive::scan(&st))
        .await
        .map_err(|_| bad("Falha ao verificar a pasta"))??;
    Ok(Json(serde_json::json!({
        "added": r.added, "moved": r.moved, "updated": r.updated, "removed": r.removed
    })))
}

fn ffmpeg_command() -> tokio::process::Command {
    let mut c = tokio::process::Command::new(
        crate::env_var_os("FFMPEG").unwrap_or_else(|| "ffmpeg".into()),
    );
    c.kill_on_drop(true);
    #[cfg(windows)]
    c.creation_flags(0x08000000);
    c
}
async fn ffmpeg_available() -> bool {
    tokio::time::timeout(
        std::time::Duration::from_secs(3),
        ffmpeg_command()
            .arg("-version")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status(),
    )
    .await
    .is_ok_and(|r| r.is_ok_and(|s| s.success()))
}
async fn detect_encoder(s: &AppState) -> Option<String> {
    s.encoder
        .get_or_init(|| async {
            // Check actual device usability, not just whether FFmpeg lists an encoder.
            for encoder in [
                "h264_nvenc",
                "h264_qsv",
                "h264_amf",
                "h264_vaapi",
                "h264_videotoolbox",
            ] {
                let mut cmd = ffmpeg_command();
                cmd.args(["-hide_banner", "-loglevel", "error", "-nostdin"]);
                if encoder == "h264_vaapi" {
                    cmd.args(["-vaapi_device", "/dev/dri/renderD128"]);
                }
                cmd.args([
                    "-f",
                    "lavfi",
                    "-i",
                    "color=size=128x128:rate=1",
                    "-frames:v",
                    "1",
                ]);
                if encoder == "h264_vaapi" {
                    cmd.args(["-vf", "format=nv12,hwupload"]);
                }
                cmd.args(["-c:v", encoder, "-f", "null", "-"])
                    .stdout(std::process::Stdio::null())
                    .stderr(std::process::Stdio::null());
                if tokio::time::timeout(std::time::Duration::from_secs(5), cmd.status())
                    .await
                    .is_ok_and(|r| r.is_ok_and(|s| s.success()))
                {
                    return Some(encoder.into());
                }
            }
            None
        })
        .await
        .clone()
}
struct ProcessStream {
    reader: tokio_util::io::ReaderStream<tokio::process::ChildStdout>,
    _child: tokio::process::Child,
    _permit: tokio::sync::OwnedSemaphorePermit,
}
impl futures_core::Stream for ProcessStream {
    type Item = Result<axum::body::Bytes, std::io::Error>;
    fn poll_next(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Self::Item>> {
        std::pin::Pin::new(&mut self.reader).poll_next(cx)
    }
}
#[derive(Deserialize)]
struct CompatibleQuery {
    start: Option<f64>,
    cpu: Option<bool>,
}
async fn compatible(
    State(s): State<AppState>,
    h: HeaderMap,
    Path(mid): Path<String>,
    Query(q): Query<CompatibleQuery>,
) -> ApiResult<Response> {
    let u = authenticate(&s, &h)?;
    let m = media_one(&s, &u, &mid)?;
    if m.kind == "photo" {
        return Err(bad("Imagens não usam conversão de vídeo"));
    }
    if m.cloud_only {
        return Err(bad(
            "Este arquivo está só no Google Drive. Baixe de volta para o servidor para usar o modo compatível",
        ));
    }
    let start = q.start.unwrap_or(0.0);
    if !start.is_finite() || !(0.0..=604800.0).contains(&start) {
        return Err(bad("Posição de reprodução inválida"));
    }
    let permit = s.converters.clone().try_acquire_owned().map_err(|_| {
        error(
            StatusCode::SERVICE_UNAVAILABLE,
            "Já há uma conversão em andamento. Use o original ou tente mais tarde",
        )
    })?;
    if !ffmpeg_available().await {
        return Err(error(
            StatusCode::NOT_IMPLEMENTED,
            "Instale FFmpeg no servidor para ativar a reprodução compatível",
        ));
    }
    let gpu =
        s.db.lock()
            .unwrap()
            .query_row("SELECT value FROM settings WHERE key='gpu'", [], |r| {
                r.get::<_, String>(0)
            })?
            == "true";
    let encoder = if gpu && q.cpu != Some(true) {
        detect_encoder(&s).await
    } else {
        None
    };
    let encoder = encoder.as_deref().unwrap_or("libx264");
    let mut cmd = ffmpeg_command();
    cmd.args([
        "-hide_banner",
        "-loglevel",
        "error",
        "-nostdin",
        "-threads",
        "2",
    ]);
    if encoder == "h264_vaapi" {
        cmd.args(["-vaapi_device", "/dev/dri/renderD128"]);
    }
    cmd.args(["-ss", &start.to_string(), "-i"])
        .arg(media_file(&s, &m)?);
    cmd.args([
        "-map", "0:v:0?", "-map", "0:a:0?", "-sn", "-dn", "-c:v", encoder,
    ]);
    if encoder == "libx264" {
        cmd.args(["-preset", "veryfast", "-crf", "23"]);
    }
    if encoder == "h264_vaapi" {
        cmd.args(["-vf", "format=nv12,hwupload"]);
    } else {
        cmd.args(["-pix_fmt", "yuv420p"]);
    }
    cmd.args([
        "-threads",
        "2",
        "-c:a",
        "aac",
        "-b:a",
        "160k",
        "-ac",
        "2",
        "-movflags",
        "frag_keyframe+empty_moov+default_base_moof",
        "-f",
        "mp4",
        "pipe:1",
    ]);
    let mut child = cmd
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()?;
    let stdout = child
        .stdout
        .take()
        .ok_or(bad("Não foi possível iniciar a conversão"))?;
    let stream = ProcessStream {
        reader: tokio_util::io::ReaderStream::new(stdout),
        _child: child,
        _permit: permit,
    };
    Ok((
        [(
            header::CONTENT_TYPE,
            if m.kind == "audio" {
                "audio/mp4"
            } else {
                "video/mp4"
            },
        )],
        Body::from_stream(stream),
    )
        .into_response())
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::Request;
    fn fixture() -> (tempfile::TempDir, AppState) {
        let d = tempfile::tempdir().unwrap();
        let s = state(d.path().into()).unwrap();
        (d, s)
    }
    fn user() -> User {
        User {
            id: "u".into(),
            name: "Ana".into(),
            admin: false,
            can_upload: false,
            blocked_genres: vec!["Terror".into()],
            blocked_collections: vec!["private".into()],
        }
    }
    #[test]
    fn deny_takes_precedence() {
        let u = user();
        assert!(!visible(&u, "terror", &[]));
        assert!(!visible(&u, "", &["public".into(), "private".into()]));
        assert!(visible(&u, "Jazz", &[]));
        let mut accented = u.clone();
        accented.blocked_genres = vec!["AÇÃO".into()];
        assert!(!visible(&accented, "ação", &[]));
    }
    #[test]
    fn reject_active_files() {
        assert!(sniff(b"<svg onload='alert(1)'>").is_none());
        assert!(sniff(b"<html>video.mp4</html>").is_none());
        assert!(sniff(b"MZ executable").is_none());
        assert_eq!(sniff(b"\xff\xd8\xffabc"), Some(("photo", "image/jpeg")));
    }
    #[tokio::test]
    async fn unauthorized_and_rebinding() {
        let (_d, s) = fixture();
        let app = router(s);
        for (host, path, code) in [
            ("localhost:8096", "/api/media", 401),
            ("attacker.test:8096", "/api/status", 403),
        ] {
            let r = app
                .clone()
                .oneshot(
                    Request::builder()
                        .uri(path)
                        .header("host", host)
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(r.status().as_u16(), code);
        }
    }
    #[test]
    fn raw_tokens_never_persist() {
        let (_d, s) = fixture();
        s.db.lock()
            .unwrap()
            .execute(
                "INSERT INTO users(id,name,password) VALUES('u','Ana','hash')",
                [],
            )
            .unwrap();
        let (_, raw) = session(&s, "u", &HeaderMap::new()).unwrap();
        let stored: String =
            s.db.lock()
                .unwrap()
                .query_row("SELECT token FROM sessions", [], |r| r.get(0))
                .unwrap();
        assert_ne!(stored, raw);
        assert_eq!(stored, digest(&raw));
    }
}
