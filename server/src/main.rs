#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let data = sonam_server::env_var("DATA").unwrap_or_else(|_| "data".into());
    let bind = sonam_server::env_var("BIND").unwrap_or_else(|_| "0.0.0.0:8096".into());
    sonam_server::serve(data.into(), &bind).await
}
