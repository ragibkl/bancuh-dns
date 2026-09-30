use std::time::Duration;

use lazy_static::lazy_static;
use reqwest::StatusCode;
use thiserror::Error;
use url::Url;

/// Attempts per fetch, including the first.
const ATTEMPTS: u32 = 5;

/// Pause before the second attempt, doubling after each failure: 1 + 2 + 4 + 8 = 15 s in
/// all. Retrying back to back gave up within a second, so a brief network blip could
/// drop a whole source from a compile.
#[cfg(not(test))]
const FIRST_RETRY_DELAY: Duration = Duration::from_secs(1);
#[cfg(test)]
const FIRST_RETRY_DELAY: Duration = Duration::from_millis(10);

#[derive(Debug)]
pub struct FetchHttp {
    pub url: Url,
}

#[derive(Error, Debug)]
pub enum FetchHTTPError {
    #[error("HTTPError: {0}")]
    HTTPError(#[from] reqwest::Error),

    #[error("HTTP status {0}")]
    Status(StatusCode),

    #[error("Unknown")]
    Unknown,
}

impl FetchHTTPError {
    /// Whether another attempt could succeed. A 404 or 403 will not change within
    /// seconds; a connection error, a timeout, a 5xx or a 429 may.
    fn is_transient(&self) -> bool {
        match self {
            Self::Status(status) => {
                status.is_server_error() || *status == StatusCode::TOO_MANY_REQUESTS
            }
            Self::HTTPError(_) | Self::Unknown => true,
        }
    }
}

impl FetchHttp {
    async fn fetch_once(&self, client: &reqwest::Client) -> Result<String, FetchHTTPError> {
        let response = client.get(self.url.to_string()).send().await?;

        // Without this check an error page counts as the source: a 404, a 500 or a
        // challenge page parses to almost no domains, and the source silently drops out.
        let status = response.status();
        if !status.is_success() {
            return Err(FetchHTTPError::Status(status));
        }

        Ok(response.text().await?)
    }

    pub async fn fetch(&self) -> Result<String, FetchHTTPError> {
        lazy_static! {
            static ref CLIENT: reqwest::Client = reqwest::Client::builder()
                .connect_timeout(Duration::from_secs(5))
                .timeout(Duration::from_secs(60))
                .build()
                .expect("Could not construct http client");
        }

        let mut last_error: FetchHTTPError = FetchHTTPError::Unknown;
        let mut delay = FIRST_RETRY_DELAY;
        for i in 1..=ATTEMPTS {
            match self.fetch_once(&CLIENT).await {
                Ok(text) => {
                    println!("Fetch ok: {}, attempt: {}", self.url, i);
                    return Ok(text);
                }
                Err(e) => {
                    println!("Fetch err: {}, attempt: {}: {}", self.url, i, e);
                    let transient = e.is_transient();
                    last_error = e;
                    if !transient || i == ATTEMPTS {
                        break;
                    }
                    tokio::time::sleep(delay).await;
                    delay *= 2;
                }
            }
        }

        Err(last_error)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };

    use axum::{Router, extract::State, http::StatusCode as AxumStatus, routing::get};

    use super::*;

    /// Serve `/` with the given statuses in turn (the last one repeats), and count hits.
    async fn serve(statuses: Vec<u16>) -> (Url, Arc<AtomicUsize>) {
        let hits = Arc::new(AtomicUsize::new(0));
        let state = (Arc::new(statuses), hits.clone());
        let app = Router::new()
            .route(
                "/",
                get(
                    |State((statuses, hits)): State<(Arc<Vec<u16>>, Arc<AtomicUsize>)>| async move {
                        let n = hits.fetch_add(1, Ordering::SeqCst);
                        let code = statuses[n.min(statuses.len() - 1)];
                        (
                            AxumStatus::from_u16(code).unwrap(),
                            "0.0.0.0 blocked.example\n",
                        )
                    },
                ),
            )
            .with_state(state);

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

        (format!("http://{addr}/").parse().unwrap(), hits)
    }

    #[tokio::test]
    async fn ok_is_returned() {
        let (url, hits) = serve(vec![200]).await;
        let text = FetchHttp { url }.fetch().await.unwrap();

        assert_eq!(text, "0.0.0.0 blocked.example\n");
        assert_eq!(hits.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn not_found_fails_without_retrying() {
        let (url, hits) = serve(vec![404]).await;
        let err = FetchHttp { url }.fetch().await.unwrap_err();

        assert!(matches!(err, FetchHTTPError::Status(StatusCode::NOT_FOUND)));
        assert_eq!(hits.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn server_error_is_retried_until_it_succeeds() {
        let (url, hits) = serve(vec![503, 500, 200]).await;
        let text = FetchHttp { url }.fetch().await.unwrap();

        assert_eq!(text, "0.0.0.0 blocked.example\n");
        assert_eq!(hits.load(Ordering::SeqCst), 3);
    }

    #[tokio::test]
    async fn persistent_server_error_gives_up_after_all_attempts() {
        let (url, hits) = serve(vec![502]).await;
        let err = FetchHttp { url }.fetch().await.unwrap_err();

        assert!(matches!(
            err,
            FetchHTTPError::Status(StatusCode::BAD_GATEWAY)
        ));
        assert_eq!(hits.load(Ordering::SeqCst), ATTEMPTS as usize);
    }

    #[tokio::test]
    async fn connection_error_is_retried() {
        // bind then drop, so nothing listens on the port
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        drop(listener);

        let url: Url = format!("http://{addr}/").parse().unwrap();
        let start = std::time::Instant::now();
        let err = FetchHttp { url }.fetch().await.unwrap_err();

        assert!(matches!(err, FetchHTTPError::HTTPError(_)));
        // four pauses of 10, 20, 40 and 80 ms in tests
        assert!(start.elapsed() >= Duration::from_millis(150));
    }
}
