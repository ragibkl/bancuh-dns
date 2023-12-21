use thiserror::Error;

use super::{file_or_url::ParseFileOrUrlError, source_config::Source, FileOrUrl};

#[derive(Error, Debug)]
pub enum FromSourceError {
    #[error("InvalidUrl: {0}")]
    InvalidUrl(#[from] url::ParseError),

    #[error("InvalidPath")]
    InvalidPath,

    #[error("FileNotExists")]
    FileNotExists,

    #[error("Infallible")]
    Infallible(#[from] core::convert::Infallible),

    #[error("InvalidFileOrUrl")]
    InvalidFileOrUrl(#[from] ParseFileOrUrlError),
}

#[derive(Debug, Clone)]
pub struct DomainSource<T: Clone> {
    pub format: T,
    pub file_or_url: FileOrUrl,
}

impl<T: Clone> DomainSource<T> {
    pub fn try_from_source(
        config_url: &FileOrUrl,
        source: &Source<T>,
    ) -> Result<Self, FromSourceError> {
        let file_or_url = if source.path.starts_with("./") {
            match config_url {
                FileOrUrl::Url(u) => FileOrUrl::Url(u.join(&source.path)?),
                FileOrUrl::File(p) => {
                    let path = p
                        .parent()
                        .ok_or(FromSourceError::InvalidPath)?
                        .join(&source.path);

                    if path.exists() {
                        FileOrUrl::File(path)
                    } else {
                        return Err(FromSourceError::FileNotExists);
                    }
                }
            }
        } else {
            source.path.parse()?
        };

        Ok(Self {
            format: source.format.clone(),
            file_or_url,
        })
    }
}
