//! Friendly addresses: a `.local` name on the home network (mDNS) and an
//! optional public HTTPS link through a Cloudflare quick tunnel.
use crate::AppState;
use axum::{
    extract::{ConnectInfo, Request},
    middleware::{self, Next},
    response::Response,
};
use serde::Serialize;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use tokio::io::AsyncBufReadExt;

#[derive(Default)]
pub struct NetState {
    pub port: u16,
    pub mdns: Option<mdns_sd::ServiceDaemon>,
    pub mdns_error: Option<String>,
    /// Also listening on port 80, so `http://sonam` works without ":8096".
    pub short: bool,
    pub tunnel: TunnelStatus,
    /// Incremented on every start/stop so stale tunnel tasks can tell they
    /// were replaced.
    pub generation: u64,
}

#[derive(Clone, Serialize)]
pub struct TunnelStatus {
    pub state: &'static str, // off | starting | on | error
    pub url: Option<String>,
    pub error: Option<String>,
}

impl Default for TunnelStatus {
    fn default() -> Self {
        Self {
            state: "off",
            url: None,
            error: None,
        }
    }
}

pub(crate) struct TunnelHandle {
    child: tokio::process::Child,
    stop: tokio::sync::oneshot::Sender<()>,
}

pub fn valid_name(name: &str) -> bool {
    (1..=40).contains(&name.len())
        && name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
        && !name.starts_with('-')
        && !name.ends_with('-')
}

pub fn name(s: &AppState) -> String {
    crate::setting(s, "mdns_name").unwrap_or_else(|| "sonam".into())
}

/// "https://casa.duckdns.org" / "http://203.0.113.7:8096" -> (scheme, host, port).
pub fn parse_public_url(url: &str) -> Option<(&str, String, Option<u16>)> {
    let (scheme, rest) = url.split_once("://")?;
    if !matches!(scheme, "http" | "https") || rest.is_empty() || rest.contains(['/', '@', '?', '#'])
    {
        return None;
    }
    let (host, port) = match rest.rsplit_once(':') {
        Some((h, p)) if !h.contains(']') || h.ends_with(']') => (h, Some(p.parse::<u16>().ok()?)),
        _ => (rest, None),
    };
    let host = host.to_lowercase();
    let valid = host.parse::<IpAddr>().is_ok()
        || (!host.is_empty()
            && host.len() <= 253
            && host.split('.').all(|l| {
                !l.is_empty()
                    && l.len() <= 63
                    && l.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
                    && !l.starts_with('-')
                    && !l.ends_with('-')
            }));
    valid.then_some((scheme, host, port))
}

fn public_host(s: &AppState) -> Option<String> {
    let url = crate::setting(s, "public_url")?;
    parse_public_url(&url).map(|(_, h, _)| h)
}

/// Hosts accepted in the Host header besides IP literals and localhost. Keep
/// this list tight: it is the DNS-rebinding defence. Names an attacker's DNS
/// cannot point at this server: the bare LAN name (LLMNR/mDNS on Windows),
/// `<name>.local` (mDNS), Tailscale MagicDNS `<name>.<tailnet>.ts.net`, the
/// owner's own domain and the active tunnel.
pub(crate) fn host_allowed(s: &AppState, host: &str) -> bool {
    let bare = host
        .parse::<SocketAddr>()
        .map(|a| a.ip().to_string())
        .unwrap_or_else(|_| host.rsplit_once(':').map_or(host, |h| h.0).to_string())
        .trim_end_matches('.')
        .to_lowercase();
    if bare == "localhost" || bare.parse::<IpAddr>().is_ok() {
        return true;
    }
    if crate::env_var("PUBLIC_HOST").is_ok_and(|p| !p.is_empty() && p == host) {
        return true;
    }
    let me = name(s);
    if bare == me || bare == format!("{me}.local") {
        return true;
    }
    if bare.starts_with(&format!("{me}."))
        && bare.ends_with(".ts.net")
        && bare.matches('.').count() == 3
    {
        return true;
    }
    if public_host(s).is_some_and(|h| h == bare) {
        return true;
    }
    let net = s.net.lock().unwrap();
    net.tunnel
        .url
        .as_deref()
        .and_then(|u| u.strip_prefix("https://"))
        .is_some_and(|t| t == bare)
}

fn private_v4(ip: Ipv4Addr) -> bool {
    ip.is_private() || (ip.octets()[0] == 100 && (64..128).contains(&ip.octets()[1]))
}

/// Hypervisor, container and WSL bridges are not reachable from other devices.
fn virtual_adapter(name: &str) -> bool {
    let n = name.to_lowercase();
    [
        "vethernet",
        "wsl",
        "hyper-v",
        "docker",
        "veth",
        "virbr",
        "br-",
        "vmnet",
        "vmware",
        "virtualbox",
        "vboxnet",
        "lxc",
        "cni",
        "podman",
    ]
    .iter()
    .any(|k| n.contains(k))
}

/// LAN (and Tailscale/CGNAT) IPv4 addresses of this machine, best first.
pub fn lan_ips() -> Vec<Ipv4Addr> {
    let mut ips: Vec<Ipv4Addr> = if_addrs::get_if_addrs()
        .unwrap_or_default()
        .into_iter()
        .filter(|i| !i.is_loopback())
        .filter_map(|i| match i.ip() {
            IpAddr::V4(v4)
                if private_v4(v4) && (v4.octets()[0] == 100 || !virtual_adapter(&i.name)) =>
            {
                Some(v4)
            }
            _ => None,
        })
        .collect();
    ips.sort_by_key(|ip| match ip.octets() {
        [192, 168, ..] => 0,
        [10, ..] => 1,
        [172, ..] => 2,
        _ => 3,
    });
    ips.dedup();
    ips
}

#[derive(Serialize)]
pub struct Address {
    pub kind: &'static str, // domain | public | short | name | lan | vpn
    pub url: String,
}

pub fn addresses(s: &AppState) -> Vec<Address> {
    let (port, mdns_ok, tunnel, short) = {
        let n = s.net.lock().unwrap();
        (n.port, n.mdns.is_some(), n.tunnel.url.clone(), n.short)
    };
    let mut out = vec![];
    if let Some(url) = crate::setting(s, "public_url").filter(|u| !u.is_empty()) {
        out.push(Address {
            kind: "domain",
            url,
        });
    }
    if let Some(url) = tunnel {
        out.push(Address {
            kind: "public",
            url,
        });
    }
    if mdns_ok {
        let me = name(s);
        let suffix = if short {
            String::new()
        } else {
            format!(":{port}")
        };
        out.push(Address {
            kind: "short",
            url: format!("http://{me}{suffix}"),
        });
        out.push(Address {
            kind: "name",
            url: format!("http://{me}.local{suffix}"),
        });
    }
    for ip in lan_ips() {
        out.push(Address {
            kind: if ip.octets()[0] == 100 { "vpn" } else { "lan" },
            url: format!("http://{ip}:{port}"),
        });
    }
    out
}

/// (Re)announce `<name>.local` and an `_http._tcp` service on every interface.
pub fn start_mdns(s: &AppState) {
    let host = name(s);
    let port = s.net.lock().unwrap().port;
    if let Some(old) = s.net.lock().unwrap().mdns.take() {
        let _ = old.shutdown();
    }
    let result = (|| -> Result<mdns_sd::ServiceDaemon, mdns_sd::Error> {
        let daemon = mdns_sd::ServiceDaemon::new()?;
        let info = mdns_sd::ServiceInfo::new(
            "_http._tcp.local.",
            &format!("Sonam {host}"),
            &format!("{host}.local."),
            "",
            port,
            &[("path", "/"), ("app", "sonam")][..],
        )?
        .enable_addr_auto();
        daemon.register(info)?;
        Ok(daemon)
    })();
    let mut net = s.net.lock().unwrap();
    match result {
        Ok(d) => {
            net.mdns = Some(d);
            net.mdns_error = None;
        }
        Err(e) => {
            eprintln!("mDNS: {e}");
            net.mdns_error = Some(e.to_string());
        }
    }
}

pub fn stop_mdns(s: &AppState) {
    if let Some(d) = s.net.lock().unwrap().mdns.take() {
        let _ = d.shutdown();
    }
}

fn cloudflared() -> std::ffi::OsString {
    crate::env_var_os("CLOUDFLARED").unwrap_or_else(|| "cloudflared".into())
}

pub async fn cloudflared_available() -> bool {
    let mut cmd = tokio::process::Command::new(cloudflared());
    cmd.arg("--version")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .kill_on_drop(true);
    #[cfg(windows)]
    cmd.creation_flags(0x08000000);
    tokio::time::timeout(std::time::Duration::from_secs(5), cmd.status())
        .await
        .is_ok_and(|r| r.is_ok_and(|s| s.success()))
}

/// Requests arriving through the tunnel come from cloudflared on loopback.
/// Replace that address with the visitor's IP so loopback-only privileges
/// (setup without key) never apply and rate limits stay per visitor.
async fn tunneled(mut req: Request, next: Next) -> Response {
    let ip = req
        .headers()
        .get("cf-connecting-ip")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.trim().parse::<IpAddr>().ok())
        .filter(|ip| !ip.is_loopback() && !ip.is_unspecified())
        .unwrap_or(IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)));
    req.extensions_mut()
        .insert(ConnectInfo(SocketAddr::new(ip, 0)));
    next.run(req).await
}

fn find_url(line: &str) -> Option<String> {
    let start = line.find("https://")?;
    let url: String = line[start..]
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric() || matches!(c, ':' | '/' | '.' | '-'))
        .collect();
    let host = url.strip_prefix("https://")?;
    (host.ends_with(".trycloudflare.com") && host != "api.trycloudflare.com").then_some(url)
}

fn set_status(s: &AppState, generation: u64, status: TunnelStatus) {
    let mut net = s.net.lock().unwrap();
    if net.generation == generation {
        net.tunnel = status;
    }
}

fn tunnel_error(s: &AppState, generation: u64, msg: String) {
    set_status(
        s,
        generation,
        TunnelStatus {
            state: "error",
            url: None,
            error: Some(msg),
        },
    )
}

pub async fn start_tunnel(s: AppState) {
    stop_tunnel(&s).await;
    let generation = {
        let mut net = s.net.lock().unwrap();
        net.generation += 1;
        net.tunnel = TunnelStatus {
            state: "starting",
            url: None,
            error: None,
        };
        net.generation
    };
    let fail = |s: &AppState, msg: String| tunnel_error(s, generation, msg);
    // Dedicated loopback listener: only cloudflared talks to it.
    let listener = match tokio::net::TcpListener::bind("127.0.0.1:0").await {
        Ok(l) => l,
        Err(e) => return fail(&s, format!("Porta interna indisponível: {e}")),
    };
    let port = listener.local_addr().map(|a| a.port()).unwrap_or(0);
    let (stop, stopped) = tokio::sync::oneshot::channel::<()>();
    let app = crate::router(s.clone()).layer(middleware::from_fn(tunneled));
    tokio::spawn(async move {
        let _ = axum::serve(
            listener,
            app.into_make_service_with_connect_info::<SocketAddr>(),
        )
        .with_graceful_shutdown(async {
            let _ = stopped.await;
        })
        .await;
    });
    let mut cmd = tokio::process::Command::new(cloudflared());
    cmd.args([
        "tunnel",
        "--no-autoupdate",
        "--url",
        &format!("http://127.0.0.1:{port}"),
    ])
    .stdin(std::process::Stdio::null())
    .stdout(std::process::Stdio::null())
    .stderr(std::process::Stdio::piped())
    .kill_on_drop(true);
    #[cfg(windows)]
    cmd.creation_flags(0x08000000);
    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(_) => {
            let _ = stop.send(());
            return fail(
                &s,
                "cloudflared não encontrado. Instale-o neste computador para gerar o link.".into(),
            );
        }
    };
    let stderr = child.stderr.take();
    *s.tunnel.lock().await = Some(TunnelHandle { child, stop });
    let Some(stderr) = stderr else { return };
    let st = s.clone();
    tokio::spawn(async move {
        let mut lines = tokio::io::BufReader::new(stderr).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            if let Some(url) = find_url(&line) {
                println!("Link público: {url}");
                set_status(
                    &st,
                    generation,
                    TunnelStatus {
                        state: "on",
                        url: Some(url),
                        error: None,
                    },
                );
            }
        }
        // stderr closed: cloudflared exited.
        let mut net = st.net.lock().unwrap();
        if net.generation == generation && net.tunnel.state != "off" {
            net.tunnel = TunnelStatus {
                state: "error",
                url: None,
                error: Some("O túnel foi encerrado. Verifique a internet e tente de novo.".into()),
            };
        }
    });
    let st = s.clone();
    tokio::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_secs(60)).await;
        let net = st.net.lock().unwrap();
        if net.generation == generation && net.tunnel.state == "starting" {
            drop(net);
            tunnel_error(
                &st,
                generation,
                "O link não ficou pronto em 1 minuto. Verifique a internet e tente de novo.".into(),
            );
        }
    });
}

pub async fn stop_tunnel(s: &AppState) {
    {
        let mut net = s.net.lock().unwrap();
        net.generation += 1;
        net.tunnel = TunnelStatus {
            state: "off",
            url: None,
            error: None,
        };
    }
    if let Some(mut h) = s.tunnel.lock().await.take() {
        let _ = h.child.kill().await;
        let _ = h.stop.send(());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_and_urls() {
        assert!(valid_name("sonam"));
        assert!(valid_name("casa-2"));
        assert!(!valid_name("Casa"));
        assert!(!valid_name("-x"));
        assert!(!valid_name("a.b"));
        assert_eq!(
            find_url("INF |  https://quiet-river-12.trycloudflare.com   |"),
            Some("https://quiet-river-12.trycloudflare.com".into())
        );
        assert_eq!(find_url("https://api.trycloudflare.com/tunnel"), None);
        assert_eq!(find_url("https://evil.example.com"), None);
    }

    #[tokio::test]
    async fn tunnel_never_counts_as_local() {
        use axum::{body::Body, http::Request};
        use tower::ServiceExt;
        let d = tempfile::tempdir().unwrap();
        let s = crate::state(d.path().into()).unwrap();
        let app = crate::router(s).layer(middleware::from_fn(tunneled));
        let mut req = Request::post("/api/setup")
            .header("host", "localhost")
            .header("content-type", "application/json")
            .header("cf-connecting-ip", "127.0.0.1")
            .body(Body::from(r#"{"name":"Ana","password":"0123456789ab"}"#))
            .unwrap();
        req.extensions_mut()
            .insert(ConnectInfo(SocketAddr::from(([127, 0, 0, 1], 5000))));
        let r = app.oneshot(req).await.unwrap();
        assert_eq!(r.status().as_u16(), 403);
    }

    #[test]
    fn public_urls() {
        assert_eq!(
            parse_public_url("https://casa.duckdns.org"),
            Some(("https", "casa.duckdns.org".into(), None))
        );
        assert_eq!(
            parse_public_url("http://203.0.113.7:8096"),
            Some(("http", "203.0.113.7".into(), Some(8096)))
        );
        assert!(parse_public_url("ftp://x").is_none());
        assert!(parse_public_url("https://x.com/path").is_none());
        assert!(parse_public_url("https://user@x.com").is_none());
        assert!(parse_public_url("https://bad_host").is_none());
    }

    #[test]
    fn hosts() {
        let d = tempfile::tempdir().unwrap();
        let s = crate::state(d.path().into()).unwrap();
        assert!(host_allowed(&s, "localhost:8096"));
        assert!(host_allowed(&s, "192.168.0.5:8096"));
        assert!(host_allowed(&s, "[::1]:8096"));
        assert!(host_allowed(&s, "sonam.local:8096"));
        assert!(host_allowed(&s, "Sonam.local"));
        assert!(host_allowed(&s, "sonam"));
        assert!(host_allowed(&s, "sonam:8096"));
        assert!(host_allowed(&s, "sonam.tail1234.ts.net"));
        assert!(!host_allowed(&s, "sonam.evil.com"));
        assert!(!host_allowed(&s, "sonam.a.b.ts.net"));
        assert!(!host_allowed(&s, "casa.duckdns.org"));
        crate::set_setting(&s, "public_url", "https://casa.duckdns.org").unwrap();
        assert!(host_allowed(&s, "casa.duckdns.org"));
        assert!(!host_allowed(&s, "attacker.test:8096"));
        assert!(!host_allowed(&s, "x.trycloudflare.com"));
        s.net.lock().unwrap().tunnel.url = Some("https://x.trycloudflare.com".into());
        assert!(host_allowed(&s, "x.trycloudflare.com"));
        assert!(!host_allowed(&s, "y.trycloudflare.com"));
    }
}
