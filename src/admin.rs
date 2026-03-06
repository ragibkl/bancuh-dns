use std::{net::SocketAddr, sync::Arc};

use axum::{
    extract::{ConnectInfo, State},
    response::Html,
    routing::get,
    Json, Router,
};
use axum_server::{tls_rustls::RustlsConfig, Handle};
use rustls_acme::ResolvesServerCertAcme;
use tokio_util::sync::CancellationToken;

use crate::query_log::{QueryLog, QueryLogStore};

#[derive(Clone)]
struct AppState {
    query_log: Arc<QueryLogStore>,
}

#[derive(serde::Serialize)]
struct LogsApiOutput {
    ip: String,
    queries: Vec<QueryLog>,
}

fn normalize_ip(addr: SocketAddr) -> String {
    let ip = addr.ip().to_string();
    // Strip IPv4-mapped IPv6 prefix
    ip.strip_prefix("::ffff:").unwrap_or(&ip).to_string()
}

async fn get_logs_api(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    State(state): State<AppState>,
) -> Json<LogsApiOutput> {
    let ip_str = normalize_ip(addr);
    let ip = ip_str.parse().unwrap_or(addr.ip());
    let queries = state.query_log.get_logs(&ip);

    Json(LogsApiOutput {
        ip: ip_str,
        queries,
    })
}

async fn get_logs_html(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    State(state): State<AppState>,
) -> Html<String> {
    let ip_str = normalize_ip(addr);
    let ip = ip_str.parse().unwrap_or(addr.ip());
    let queries = state.query_log.get_logs(&ip);
    let active_ips = state.query_log.active_ips();

    let mut rows = String::new();
    for q in &queries {
        rows.push_str(&format!(
            "<tr><td>{}</td><td>{}</td><td>{}</td></tr>\n",
            q.query_time.format("%Y-%m-%d %H:%M:%S"),
            html_escape(&q.question),
            html_escape(&q.answer),
        ));
    }

    let html = format!(
        r#"<!DOCTYPE html>
<html>
<head>
  <title>Bancuh DNS - Query Logs</title>
  <style>
    body {{ font-family: sans-serif; margin: 20px; }}
    table, th, td {{ border: 1px solid #ccc; border-collapse: collapse; }}
    th, td {{ padding: 8px 12px; text-align: left; }}
    th {{ background: #f5f5f5; }}
  </style>
</head>
<body>
  <h2>Bancuh DNS - Query Logs</h2>
  <p>Your IP: <strong>{ip_str}</strong></p>
  <p>Active IPs (10 min): <strong>{active_ips}</strong></p>
  <p>Showing {count} queries</p>
  <table>
    <tr><th>Timestamp</th><th>Query</th><th>Answer</th></tr>
    {rows}
  </table>
</body>
</html>"#,
        count = queries.len(),
    );

    Html(html)
}

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

fn make_app(query_log: Arc<QueryLogStore>) -> Router {
    let state = AppState { query_log };
    Router::new()
        .route("/logs", get(get_logs_html))
        .route("/api/logs", get(get_logs_api))
        .with_state(state)
}

pub async fn serve(
    port: u16,
    query_log: Arc<QueryLogStore>,
    tls_resolver: Option<Arc<ResolvesServerCertAcme>>,
    token: CancellationToken,
) {
    let app = make_app(query_log);

    // HTTP on port (default 8080)
    let http_app = app.clone();
    let http_token = token.clone();
    let http_handle = tokio::spawn(async move {
        let v4_addr = SocketAddr::from(([0, 0, 0, 0], port));
        let v6_addr = SocketAddr::from(([0, 0, 0, 0, 0, 0, 0, 0], port));
        tracing::info!("Starting admin HTTP server on port {port}");

        let v4_listener = match crate::net::bind_tcp(v4_addr) {
            Ok(l) => l,
            Err(err) => {
                tracing::error!("Failed to bind admin server on {v4_addr}: {err}");
                return;
            }
        };

        let v6_listener = match crate::net::bind_tcp(v6_addr) {
            Ok(l) => l,
            Err(err) => {
                tracing::error!("Failed to bind admin server on {v6_addr}: {err}");
                return;
            }
        };

        let v4_app = http_app.clone();
        let v4_token = http_token.clone();
        let v4 = tokio::spawn(async move {
            if let Err(err) = axum::serve(
                v4_listener,
                v4_app.into_make_service_with_connect_info::<SocketAddr>(),
            )
            .with_graceful_shutdown(async move { v4_token.cancelled().await })
            .await
            {
                tracing::error!("Admin HTTP server (v4) error: {err}");
            }
        });

        let v6_token = http_token.clone();
        let v6 = tokio::spawn(async move {
            if let Err(err) = axum::serve(
                v6_listener,
                http_app.into_make_service_with_connect_info::<SocketAddr>(),
            )
            .with_graceful_shutdown(async move { v6_token.cancelled().await })
            .await
            {
                tracing::error!("Admin HTTP server (v6) error: {err}");
            }
        });

        let _ = tokio::join!(v4, v6);
    });

    // HTTPS on port 8443 (only when TLS is enabled)
    let https_handle = if let Some(resolver) = tls_resolver {
        let https_app = app;
        let https_token = token.clone();
        Some(tokio::spawn(async move {
            let mut tls_config = rustls::ServerConfig::builder()
                .with_no_client_auth()
                .with_cert_resolver(resolver);
            tls_config.alpn_protocols = vec![b"h2".to_vec(), b"http/1.1".to_vec()];

            let rustls_config = RustlsConfig::from_config(Arc::new(tls_config));
            let addr = SocketAddr::from(([0, 0, 0, 0], 8443));
            let handle = Handle::new();

            tracing::info!("Starting admin HTTPS server on port 8443");

            let cloned_token = https_token.clone();
            let cloned_handle = handle.clone();
            tokio::spawn(async move {
                cloned_token.cancelled().await;
                tracing::info!("admin HTTPS server received cancel signal");
                cloned_handle.shutdown();
            });

            if let Err(err) = axum_server::bind_rustls(addr, rustls_config)
                .handle(handle)
                .serve(https_app.into_make_service_with_connect_info::<SocketAddr>())
                .await
            {
                tracing::error!("Admin HTTPS server error: {err}");
            }
        }))
    } else {
        None
    };

    let _ = http_handle.await;
    if let Some(h) = https_handle {
        let _ = h.await;
    }
}
