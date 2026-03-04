use std::sync::Arc;
use std::net::SocketAddr;

use axum::Router;
use futures::StreamExt;
use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use rustls::DigitallySignedStruct;
use rustls_acme::{caches::DirCache, AcmeConfig, AcmeState, ResolvesServerCertAcme, UseChallenge};
use tokio::net::TcpListener;
use tokio_util::{sync::CancellationToken, task::TaskTracker};

/// Skips all TLS certificate verification. Only for use with local test ACME servers (Pebble).
#[derive(Debug)]
struct NoVerifier;

impl ServerCertVerifier for NoVerifier {
    fn verify_server_cert(
        &self,
        _end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp: &[u8],
        _now: UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        Ok(ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        _message: &[u8],
        _cert: &CertificateDer<'_>,
        _dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        Ok(HandshakeSignatureValid::assertion())
    }

    fn verify_tls13_signature(
        &self,
        _message: &[u8],
        _cert: &CertificateDer<'_>,
        _dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        Ok(HandshakeSignatureValid::assertion())
    }

    fn supported_verify_schemes(&self) -> Vec<rustls::SignatureScheme> {
        rustls::crypto::aws_lc_rs::default_provider()
            .signature_verification_algorithms
            .supported_schemes()
    }
}

pub async fn setup_tls(
    domain: String,
    email: String,
    acme_url: Option<String>,
    acme_cache_dir: String,
    acme_insecure: bool,
    tracker: &TaskTracker,
    token: CancellationToken,
) -> Arc<ResolvesServerCertAcme> {
    let base_config = if acme_insecure {
        tracing::warn!("ACME_INSECURE=true: TLS certificate verification disabled (Pebble mode)");
        let client_config = rustls::ClientConfig::builder_with_provider(Arc::new(
            rustls::crypto::aws_lc_rs::default_provider(),
        ))
        .with_safe_default_protocol_versions()
        .unwrap()
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(NoVerifier))
        .with_no_client_auth();

        AcmeConfig::new_with_client_config([domain.clone()], Arc::new(client_config))
    } else {
        AcmeConfig::new([domain.clone()])
    };

    let mut config = base_config
        .contact_push(format!("mailto:{email}"))
        .cache(DirCache::new(acme_cache_dir))
        .challenge_type(UseChallenge::Http01);

    if let Some(url) = acme_url {
        config = config.directory(url);
    } else {
        config = config.directory_lets_encrypt(true);
    }

    let mut acme_state = AcmeState::new(config);
    let resolver = acme_state.resolver();
    let challenge_service = acme_state.http01_challenge_tower_service();

    // Spawn ACME polling task — drives cert issuance and renewal
    let cloned_token = token.clone();
    tracker.spawn(async move {
        loop {
            tokio::select! {
                event = acme_state.next() => {
                    match event {
                        Some(Ok(ok)) => tracing::info!("acme event: {ok:?}"),
                        Some(Err(err)) => tracing::warn!("acme error: {err}"),
                        None => {
                            tracing::error!("acme state stream ended unexpectedly");
                            cloned_token.cancel();
                            return;
                        }
                    }
                }
                _ = cloned_token.cancelled() => {
                    tracing::info!("acme task received cancel signal");
                    return;
                }
            }
        }
    });

    // Spawn HTTP-01 challenge server on port 80
    let cloned_token = token.clone();
    tracker.spawn(async move {
        let app = Router::new().route_service(
            "/.well-known/acme-challenge/{token}",
            challenge_service,
        );

        let addr = SocketAddr::from(([0, 0, 0, 0, 0, 0, 0, 0], 80));
        let listener = match TcpListener::bind(&addr).await {
            Ok(l) => l,
            Err(err) => {
                tracing::error!("acme http challenge server bind failed: {err}");
                cloned_token.cancel();
                return;
            }
        };
        tracing::info!("acme http challenge server listening on {addr}");

        let shutdown = cloned_token.clone();
        if let Err(err) = axum::serve(listener, app)
            .with_graceful_shutdown(async move { shutdown.cancelled().await })
            .await
        {
            tracing::error!("acme http challenge server error: {err}");
            cloned_token.cancel();
        }
    });

    resolver
}
