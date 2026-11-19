use tauri::Manager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            let data = app.path().app_data_dir()?;
            // Installs from before the rename kept their library under the old
            // identifier; move it once so nothing is lost.
            if !data.exists() {
                if let Some(old) = data.parent().map(|p| p.join("org.homestream.app")) {
                    if old.is_dir() {
                        std::fs::rename(&old, &data)?;
                    }
                }
            }
            let state =
                sonam_server::state(data).map_err(|e| std::io::Error::other(e.to_string()))?;
            // Bind before opening the window. Never silently attach to another process.
            let listener = std::net::TcpListener::bind("0.0.0.0:8096")?;
            listener.set_nonblocking(true)?;
            std::thread::Builder::new()
                .name("sonam-server".into())
                .spawn(move || {
                    let runtime = tokio::runtime::Runtime::new().expect("runtime");
                    runtime.block_on(async move {
                        let listener =
                            tokio::net::TcpListener::from_std(listener).expect("listener");
                        // Same lifecycle as the standalone server: watched drive
                        // folder, `.local` name and the optional public link.
                        if let Err(e) =
                            sonam_server::run(state, listener, std::future::pending()).await
                        {
                            eprintln!("Servidor encerrado: {e}");
                        }
                    });
                })?;
            tauri::WebviewWindowBuilder::new(
                app,
                "main",
                tauri::WebviewUrl::External("http://127.0.0.1:8096".parse()?),
            )
            .title("Sonam")
            .inner_size(1320.0, 880.0)
            .min_inner_size(360.0, 640.0)
            .build()?;
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("Não foi possível iniciar o Sonam. Confira se a porta 8096 está livre.");
}
