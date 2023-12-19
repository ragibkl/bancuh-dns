use std::time::Duration;

use lazy_static::lazy_static;
use thiserror::Error;
use url::Url;

#[derive(Debug)]
pub struct FetchHttp {
    pub url: Url,
}

#[derive(Error, Debug)]
pub enum FetchHTTPError {
    #[error("HTTPError: {0}")]
    HTTPError(#[from] reqwest::Error),

    #[error("Unknown")]
    Unknown,
}

impl FetchHttp {
    pub async fn fetch(&self) -> Result<String, FetchHTTPError> {
        lazy_static! {
            static ref CLIENT: reqwest::Client = reqwest::Client::builder()
                .connect_timeout(Duration::from_secs(5))
                .timeout(Duration::from_secs(60))
                .build()
                .unwrap();
        }

        let mut last_error: FetchHTTPError = FetchHTTPError::Unknown;
        for i in 1..=5 {
            match CLIENT.get(self.url.to_string()).send().await {
                Ok(response) => {
                    let text = response.text().await?;
                    println!("Fetch ok: {}, attempt: {}", &self.url, i);
                    return Ok(text);
                }
                Err(e) => {
                    println!("Fetch err: {}, attempt: {}", &self.url, i);
                    last_error = e.into();
                }
            }
        }

        Err(last_error)
    }
}
