use std::{net::SocketAddr, sync::Arc};

use axum::{
    extract::{ConnectInfo, State},
    response::Html,
    routing::get,
    Json, Router,
};
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

pub async fn serve(port: u16, query_log: Arc<QueryLogStore>, token: CancellationToken) {
    let state = AppState { query_log };

    let app = Router::new()
        .route("/logs", get(get_logs_html))
        .route("/api/logs", get(get_logs_api))
        .with_state(state);

    let addr = SocketAddr::from(([0, 0, 0, 0, 0, 0, 0, 0], port));
    tracing::info!("Starting admin HTTP server on port {port}");

    let listener = match tokio::net::TcpListener::bind(&addr).await {
        Ok(l) => l,
        Err(err) => {
            tracing::error!("Failed to bind admin server on port {port}: {err}");
            return;
        }
    };

    if let Err(err) = axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .with_graceful_shutdown(async move { token.cancelled().await })
    .await
    {
        tracing::error!("Admin HTTP server error: {err}");
    }
}
