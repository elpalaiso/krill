//! M2a: `krill serve` — read-only web UI (session cards + live preview).
//! Design doc §8.2. The page is a single embedded HTML file (vanilla JS,
//! no CDN — a tailnet can be offline); state is rebuilt per request from
//! tmux + meta files, so the server holds nothing and can die freely.
//!
//! Security (design §7): binding a non-loopback address without a token
//! refuses to start. With a token set, every request must carry it
//! (`?token=` or `Authorization: Bearer`).

use crate::msg as m;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::Html;
use axum::routing::get;
use axum::{Json, Router};
use krill_core::error::{Context, Result};
use krill_core::session::{self, Health};
use krill_core::{bail, git, tmux};
use serde::Serialize;
use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;

const INDEX_HTML: &str = include_str!("../web/index.html");

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
        .route("/api/sessions", get(api_sessions))
        .route("/api/preview/{repo}/{name}", get(api_preview))
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
        assert!(!INDEX_HTML.contains("http://cdn"));
        assert!(!INDEX_HTML.contains("https://"));
    }
}
