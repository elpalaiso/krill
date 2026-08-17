//! M2a: `krill serve` — read-only web UI (session cards + live preview).
//! M2b: interactive — WebSocket terminal (xterm.js, read + input),
//! vendored assets, `--bind tailscale`. Web view is fixed at 80×24 and
//! input is meant to be short (§13 — tmux resizes to the smallest
//! client, so the web stays a viewer first).
//! Design doc §8.2. The page is a single embedded HTML file (vanilla JS,
//! no CDN — a tailnet can be offline); state is rebuilt per request from
//! tmux + meta files, so the server holds nothing and can die freely.
//!
//! Security (design §7): binding a non-loopback address without a token
//! refuses to start. With a token set, every request must carry it
//! (`?token=` or `Authorization: Bearer`).

use crate::msg as m;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Path, Query, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{Html, IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use krill_core::error::{Context, Result};
use krill_core::session::{self, Health};
use krill_core::{bail, git, tmux};
use serde::Serialize;
use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

const INDEX_HTML: &str = include_str!("../web/index.html");
// Vendored from the @xterm/xterm npm package (MIT) — no CDN (§8).
const XTERM_JS: &str = include_str!("../web/assets/xterm.js");
const XTERM_CSS: &str = include_str!("../web/assets/xterm.css");

#[derive(Clone)]
struct Srv {
    token: Option<String>,
}

#[derive(Serialize)]
struct SessionInfo {
    name: String,
    repo: String,
    agent: String,
    state: &'static str,
    age: String,
    diff: String,
}

fn state_str(h: Health) -> &'static str {
    match h {
        Health::Active => "active",
        Health::Quiet => "quiet",
        Health::Dead => "dead",
    }
}

/// Design §7: only loopback may run without a token.
fn requires_token(ip: IpAddr) -> bool {
    !ip.is_loopback()
}

/// Token check, pure for tests: expected vs what the request carried.
fn token_ok(expected: Option<&str>, query: Option<&str>, bearer: Option<&str>) -> bool {
    match expected {
        None => true,
        Some(t) => query == Some(t) || bearer == Some(t),
    }
}

fn authed(srv: &Srv, q: &HashMap<String, String>, headers: &HeaderMap) -> bool {
    let bearer = headers
        .get("authorization")
        .and_then(|h| h.to_str().ok())
        .and_then(|h| h.strip_prefix("Bearer "));
    token_ok(srv.token.as_deref(), q.get("token").map(String::as_str), bearer)
}

pub fn run(bind: &str, port: u16, token: Option<String>) -> Result<()> {
    let Ok(ip) = bind.parse::<IpAddr>() else {
        bail!(m::serve_bad_bind(bind));
    };
    if requires_token(ip) && token.is_none() {
        bail!(m::serve_token_required());
    }
    let addr = SocketAddr::new(ip, port);
    let app = Router::new()
        .route("/", get(index))
        .route("/assets/xterm.js", get(asset_js))
        .route("/assets/xterm.css", get(asset_css))
        .route("/api/sessions", get(api_sessions))
        .route("/api/preview/{repo}/{name}", get(api_preview))
        .route("/ws/{repo}/{name}", get(ws_upgrade))
        .with_state(Arc::new(Srv { token }));

    let rt = tokio::runtime::Runtime::new().context(m::serve_start_failed())?;
    rt.block_on(async move {
        let listener = tokio::net::TcpListener::bind(addr)
            .await
            .with_context(|| m::serve_bind_failed(&addr.to_string()))?;
        println!("{}", m::serve_listening(&addr.to_string()));
        axum::serve(listener, app).await.context(m::serve_start_failed())?;
        Ok(())
    })
}

async fn index(
    State(srv): State<Arc<Srv>>,
    Query(q): Query<HashMap<String, String>>,
    headers: HeaderMap,
) -> std::result::Result<Html<&'static str>, StatusCode> {
    if !authed(&srv, &q, &headers) {
        return Err(StatusCode::UNAUTHORIZED);
    }
    Ok(Html(INDEX_HTML))
}

// Library assets carry no session data — served without auth so the
// page's <script>/<link> tags don't need the token appended.
async fn asset_js() -> impl IntoResponse {
    ([(header::CONTENT_TYPE, "application/javascript")], XTERM_JS)
}

async fn asset_css() -> impl IntoResponse {
    ([(header::CONTENT_TYPE, "text/css")], XTERM_CSS)
}

/// capture-pane text uses bare \n; a terminal needs \r\n.
fn crlf(s: &str) -> String {
    s.replace('\n', "\r\n")
}

/// Read the pipe-pane log from `offset`, capped per tick. Raw bytes —
/// a chunk may split a UTF-8 sequence, hence binary WS frames.
fn read_from(path: &std::path::Path, offset: u64) -> Vec<u8> {
    use std::io::{Read, Seek, SeekFrom};
    let Ok(mut f) = std::fs::File::open(path) else {
        return Vec::new();
    };
    if f.seek(SeekFrom::Start(offset)).is_err() {
        return Vec::new();
    }
    let mut buf = Vec::new();
    let _ = f.take(256 * 1024).read_to_end(&mut buf);
    buf
}

async fn ws_upgrade(
    State(srv): State<Arc<Srv>>,
    Path((repo, name)): Path<(String, String)>,
    Query(q): Query<HashMap<String, String>>,
    headers: HeaderMap,
    ws: WebSocketUpgrade,
) -> std::result::Result<Response, StatusCode> {
    if !authed(&srv, &q, &headers) {
        return Err(StatusCode::UNAUTHORIZED);
    }
    let (tmux_name, log) = tokio::task::spawn_blocking(move || {
        let meta = session::find(&name, Some(&repo)).map_err(|_| StatusCode::NOT_FOUND)?;
        if !tmux::has(&meta.tmux) {
            return Err(StatusCode::GONE);
        }
        let log = meta.log_path().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        Ok((meta.tmux, log))
    })
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)??;
    Ok(ws.on_upgrade(move |socket| ws_session(socket, tmux_name, log)))
}

/// One live terminal: current screen first, then follow the pipe-pane
/// log for new output; browser keystrokes go back via send-keys -l.
async fn ws_session(mut socket: WebSocket, tmux_name: String, log: PathBuf) {
    let tn = tmux_name.clone();
    if let Ok(Ok(snap)) = tokio::task::spawn_blocking(move || tmux::capture_pane_ansi(&tn)).await {
        if socket.send(Message::Text((crlf(&snap) + "\r\n").into())).await.is_err() {
            return;
        }
    }
    let mut offset = std::fs::metadata(&log).map(|m| m.len()).unwrap_or(0);
    let mut poll = tokio::time::interval(Duration::from_millis(250));
    let mut ticks: u32 = 0;
    loop {
        tokio::select! {
            _ = poll.tick() => {
                let l = log.clone();
                let chunk = tokio::task::spawn_blocking(move || read_from(&l, offset))
                    .await
                    .unwrap_or_default();
                if !chunk.is_empty() {
                    offset += chunk.len() as u64;
                    if socket.send(Message::Binary(chunk.into())).await.is_err() {
                        break;
                    }
                }
                ticks += 1;
                if ticks % 8 == 0 { // ~2s: still alive?
                    let tn = tmux_name.clone();
                    let alive = tokio::task::spawn_blocking(move || tmux::has(&tn))
                        .await
                        .unwrap_or(false);
                    if !alive {
                        let _ = socket.send(Message::Text("\r\n[dead]\r\n".into())).await;
                        break;
                    }
                }
            }
            msg = socket.recv() => match msg {
                Some(Ok(Message::Text(d))) => {
                    let tn = tmux_name.clone();
                    let _ = tokio::task::spawn_blocking(move || tmux::send_raw(&tn, d.as_str()))
                        .await;
                }
                Some(Ok(Message::Close(_))) | Some(Err(_)) | None => break,
                _ => {}
            }
        }
    }
}

async fn api_sessions(
    State(srv): State<Arc<Srv>>,
    Query(q): Query<HashMap<String, String>>,
    headers: HeaderMap,
) -> std::result::Result<Json<Vec<SessionInfo>>, StatusCode> {
    if !authed(&srv, &q, &headers) {
        return Err(StatusCode::UNAUTHORIZED);
    }
    // Session listing shells out (tmux, git) — keep it off the executor.
    let infos = tokio::task::spawn_blocking(snapshot)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(infos))
}

fn snapshot() -> Result<Vec<SessionInfo>> {
    let metas = session::load_all()?;
    let live = tmux::server_sessions();
    Ok(metas
        .into_iter()
        .map(|meta| {
            let (h, age) = session::health(&meta, &live);
            let diff = if h == Health::Dead {
                "-".into()
            } else {
                git::shortstat(&meta.worktree, &meta.base)
            };
            SessionInfo {
                name: meta.name,
                repo: meta.repo_name,
                agent: meta.agent,
                state: state_str(h),
                age: age.map(krill_core::fmt_age).unwrap_or_else(|| "-".into()),
                diff,
            }
        })
        .collect())
}

async fn api_preview(
    State(srv): State<Arc<Srv>>,
    Path((repo, name)): Path<(String, String)>,
    Query(q): Query<HashMap<String, String>>,
    headers: HeaderMap,
) -> std::result::Result<String, StatusCode> {
    if !authed(&srv, &q, &headers) {
        return Err(StatusCode::UNAUTHORIZED);
    }
    tokio::task::spawn_blocking(move || {
        let meta = session::find(&name, Some(&repo)).map_err(|_| StatusCode::NOT_FOUND)?;
        if !tmux::has(&meta.tmux) {
            return Err(StatusCode::GONE);
        }
        tmux::capture_pane(&meta.tmux).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
    })
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_loopback_may_skip_the_token() {
        assert!(!requires_token("127.0.0.1".parse().unwrap()));
        assert!(!requires_token("::1".parse().unwrap()));
        assert!(requires_token("0.0.0.0".parse().unwrap()));
        assert!(requires_token("100.64.1.2".parse().unwrap())); // tailnet range
        assert!(requires_token("192.168.0.10".parse().unwrap()));
    }

    #[test]
    fn token_check_accepts_query_or_bearer() {
        assert!(token_ok(None, None, None)); // no token configured
        assert!(token_ok(Some("t"), Some("t"), None));
        assert!(token_ok(Some("t"), None, Some("t")));
        assert!(!token_ok(Some("t"), None, None));
        assert!(!token_ok(Some("t"), Some("wrong"), None));
        assert!(!token_ok(Some("t"), None, Some("wrong")));
    }

    #[test]
    fn states_map_to_the_ls_vocabulary() {
        assert_eq!(state_str(Health::Active), "active");
        assert_eq!(state_str(Health::Quiet), "quiet");
        assert_eq!(state_str(Health::Dead), "dead");
    }

    #[test]
    fn index_html_is_self_contained() {
        // Design §8: no CDN dependencies — a tailnet can be offline.
        assert!(INDEX_HTML.contains("<html"));
        assert!(INDEX_HTML.contains("/assets/xterm.js")); // vendored, not CDN
        assert!(!INDEX_HTML.contains("http://cdn"));
        assert!(!INDEX_HTML.contains("https://"));
        assert!(XTERM_JS.len() > 100_000); // the real library, not a stub
        assert!(XTERM_CSS.contains("xterm"));
    }

    #[test]
    fn crlf_converts_bare_newlines() {
        assert_eq!(crlf("a\nb\n"), "a\r\nb\r\n");
        assert_eq!(crlf("no-newline"), "no-newline");
    }

    #[test]
    fn read_from_respects_offset_and_missing_files() {
        let dir = std::env::temp_dir().join(format!("krill-ws-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let f = dir.join("log");
        std::fs::write(&f, b"hello world").unwrap();
        assert_eq!(read_from(&f, 0), b"hello world");
        assert_eq!(read_from(&f, 6), b"world");
        assert_eq!(read_from(&f, 100), b"");
        assert_eq!(read_from(&dir.join("missing"), 0), b"");
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
