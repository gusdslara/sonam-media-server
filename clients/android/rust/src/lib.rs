//! JNI bridge: runs the same Sonam server library inside the Android app
//! process, so an old phone can host the library while a foreground service
//! keeps the process alive.
use jni::{
    objects::{JClass, JString},
    sys::{jboolean, jint, jstring, JNI_FALSE, JNI_TRUE},
    JNIEnv,
};
use std::{
    path::PathBuf,
    sync::Mutex,
    thread::JoinHandle,
    time::{Duration, Instant},
};

struct Running {
    stop: tokio::sync::oneshot::Sender<()>,
    thread: JoinHandle<()>,
}

static SERVER: Mutex<Option<Running>> = Mutex::new(None);

fn start(data: PathBuf, port: u16) -> Result<(), String> {
    let mut slot = SERVER.lock().map_err(|_| "estado interno inválido")?;
    if slot.as_ref().is_some_and(|r| !r.thread.is_finished()) {
        return Ok(());
    }
    let state = sonam_server::state(data).map_err(|e| e.to_string())?;
    // Bind on the caller's thread so "port in use" is reported to the UI.
    let listener = std::net::TcpListener::bind(("0.0.0.0", port))
        .map_err(|e| format!("Porta {port} indisponível: {e}"))?;
    listener.set_nonblocking(true).map_err(|e| e.to_string())?;
    let (stop, stopped) = tokio::sync::oneshot::channel::<()>();
    let thread = std::thread::Builder::new()
        .name("sonam-server".into())
        .spawn(move || {
            let runtime = match tokio::runtime::Builder::new_multi_thread()
                .worker_threads(2)
                .enable_all()
                .thread_name("sonam-worker")
                .build()
            {
                Ok(r) => r,
                Err(_) => return,
            };
            let shutdown_state = state.clone();
            runtime.block_on(async move {
                let Ok(listener) = tokio::net::TcpListener::from_std(listener) else {
                    return;
                };
                tokio::select! {
                    _ = sonam_server::run(state, listener, std::future::pending()) => {}
                    _ = stopped => {}
                }
                sonam_server::net::stop_tunnel(&shutdown_state).await;
                sonam_server::net::stop_mdns(&shutdown_state);
            });
            // Streams still open (a movie playing) must not keep us alive.
            runtime.shutdown_timeout(Duration::from_secs(2));
        })
        .map_err(|e| e.to_string())?;
    *slot = Some(Running { stop, thread });
    Ok(())
}

fn stop() {
    let running = SERVER.lock().ok().and_then(|mut s| s.take());
    if let Some(r) = running {
        let _ = r.stop.send(());
        let deadline = Instant::now() + Duration::from_secs(5);
        while !r.thread.is_finished() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(50));
        }
    }
}

fn message(env: &mut JNIEnv, text: Option<String>) -> jstring {
    match text {
        None => std::ptr::null_mut(),
        Some(t) => env
            .new_string(t)
            .map(|s| s.into_raw())
            .unwrap_or(std::ptr::null_mut()),
    }
}

/// Returns `null` on success or a human readable error.
#[no_mangle]
pub extern "system" fn Java_org_sonam_server_ServerBridge_start(
    mut env: JNIEnv,
    _class: JClass,
    data: JString,
    port: jint,
) -> jstring {
    let data: String = match env.get_string(&data) {
        Ok(s) => s.into(),
        Err(_) => return message(&mut env, Some("Pasta de dados inválida".into())),
    };
    let port = u16::try_from(port).unwrap_or(8096);
    let result = std::panic::catch_unwind(|| start(PathBuf::from(data), port))
        .unwrap_or_else(|_| Err("Falha inesperada ao iniciar o servidor".into()));
    message(&mut env, result.err())
}

#[no_mangle]
pub extern "system" fn Java_org_sonam_server_ServerBridge_stop(_env: JNIEnv, _class: JClass) {
    let _ = std::panic::catch_unwind(stop);
}

#[no_mangle]
pub extern "system" fn Java_org_sonam_server_ServerBridge_isRunning(
    _env: JNIEnv,
    _class: JClass,
) -> jboolean {
    let running = SERVER
        .lock()
        .map(|s| s.as_ref().is_some_and(|r| !r.thread.is_finished()))
        .unwrap_or(false);
    if running {
        JNI_TRUE
    } else {
        JNI_FALSE
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};

    fn get_status(port: u16) -> Option<String> {
        let mut s = std::net::TcpStream::connect(("127.0.0.1", port)).ok()?;
        s.set_read_timeout(Some(Duration::from_secs(3))).ok()?;
        s.write_all(b"GET /api/status HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n")
            .ok()?;
        let mut out = String::new();
        s.read_to_string(&mut out).ok()?;
        Some(out)
    }

    #[test]
    fn start_serve_stop_restart() {
        let dir = std::env::temp_dir().join(format!("hs-android-{}", std::process::id()));
        let port = 18099;
        for round in 0..2 {
            assert_eq!(start(dir.clone(), port), Ok(()), "round {round}");
            assert_eq!(start(dir.clone(), port), Ok(()), "second start is a no-op");
            let deadline = Instant::now() + Duration::from_secs(10);
            let mut body = None;
            while body.is_none() && Instant::now() < deadline {
                body = get_status(port).filter(|b| b.contains("\"setup\":true"));
                std::thread::sleep(Duration::from_millis(100));
            }
            assert!(body.is_some(), "server answered in round {round}");
            stop();
            assert!(get_status(port).is_none(), "port released after stop");
        }
        let _ = std::fs::remove_dir_all(dir);
    }
}
