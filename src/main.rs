mod admin;
mod bind;
mod compiler;
mod config;
mod db;
mod engine;
mod fetch;
mod handler;
mod net;
mod query_log;
mod rate_limiter;
mod resolver;
mod tls;

use std::{
    net::{IpAddr, Ipv4Addr, SocketAddr},
    sync::Arc,
    time::Duration,
};

use clap::Parser;
use hickory_server::ServerFuture;
use itertools::Itertools;
use tokio::signal::unix::{signal, SignalKind};
use tokio_util::{sync::CancellationToken, task::TaskTracker};

use crate::{
    bind::spawn_bind,
    config::{Config, FileOrUrl},
    engine::AdblockEngine,
    handler::Handler,
    query_log::QueryLogStore,
    rate_limiter::new_rate_limiter,
    resolver::Resolver,
    tls::setup_tls,
};

const TCP_TIMEOUT: Duration = Duration::from_secs(10);
const BIND_IP: IpAddr = IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1));
const BIND_PORT: u16 = 5353;

#[derive(Parser, Debug)]
#[command(name = "Bancuh DNS")]
#[command(version)]
#[command(about)]
struct Args {
    /// Sets a custom config file
    #[arg(
        short,
        long,
        env,
        value_name = "CONFIG_URL",
        default_value = "https://raw.githubusercontent.com/ragibkl/adblock-dns-server/master/data/configuration.yaml"
    )]
    config_url: FileOrUrl,

    /// Sets a custom listener port
    #[arg(short, long, env, value_name = "PORT", default_value = "53")]
    port: u16,

    /// Sets custom forward resolvers
    #[arg(short, long, env, value_name = "FORWARDERS", value_delimiter = ',')]
    forwarders: Vec<IpAddr>,

    /// Sets a custom forward resolvers port, useful for local custom port
    #[arg(long, env, value_name = "FORWARDERS_PORT", default_value = "53")]
    forwarders_port: u16,

    /// Sets the blocklist update interval in seconds
    #[arg(long, env, value_name = "UPDATE_INTERVAL", default_value = "86400")]
    update_interval: u64,

    /// Enable DoT (port 853) and DoH (port 443) via ACME/Let's Encrypt
    #[arg(long, env, value_name = "TLS_ENABLED")]
    tls_enabled: bool,

    /// Email address for ACME/Let's Encrypt registration (required when TLS_ENABLED=true)
    #[arg(long, env, value_name = "TLS_EMAIL")]
    tls_email: Option<String>,

    /// Domain name for the TLS certificate (required when TLS_ENABLED=true)
    #[arg(long, env, value_name = "TLS_DOMAIN")]
    tls_domain: Option<String>,

    /// Custom ACME directory URL (defaults to Let's Encrypt production)
    #[arg(long, env, value_name = "ACME_URL")]
    acme_url: Option<String>,

    /// Directory for caching ACME account key and certificates
    #[arg(
        long,
        env,
        value_name = "ACME_CACHE_DIR",
        default_value = "/var/cache/bancuh-dns/certs"
    )]
    acme_cache_dir: String,

    /// Disable TLS certificate verification for the ACME server (for testing with Pebble)
    #[arg(long, env, value_name = "ACME_INSECURE")]
    acme_insecure: bool,

    /// Port for the admin HTTP server (query logs UI)
    #[arg(long, env, value_name = "ADMIN_PORT", default_value = "8080")]
    admin_port: u16,

    /// Maximum DNS requests per second per IP (0 = unlimited)
    #[arg(long, env, value_name = "RATE_LIMIT", default_value = "100")]
    rate_limit: u32,

    /// IPv4 prefix length for rate limiting (e.g. 32 = per-IP, 24 = per /24 subnet)
    #[arg(long, env, value_name = "RATE_LIMIT_IPV4_PREFIX", default_value = "32")]
    rate_limit_ipv4_prefix: u8,

    /// IPv6 prefix length for rate limiting (e.g. 48 = per /48 block, 128 = per-IP)
    #[arg(long, env, value_name = "RATE_LIMIT_IPV6_PREFIX", default_value = "48")]
    rate_limit_ipv6_prefix: u8,
}

async fn sigint() -> std::io::Result<()> {
    signal(SignalKind::interrupt())?.recv().await;
    Ok(())
}

async fn sigterm() -> std::io::Result<()> {
    signal(SignalKind::terminate())?.recv().await;
    Ok(())
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();

    let Args {
        config_url,
        port,
        forwarders,
        forwarders_port,
        update_interval,
        tls_enabled,
        tls_email,
        tls_domain,
        acme_url,
        acme_cache_dir,
        acme_insecure,
        admin_port,
        rate_limit,
        rate_limit_ipv4_prefix,
        rate_limit_ipv6_prefix,
    } = Args::parse();

    let update_interval = Duration::from_secs(update_interval);

    tracing::info!("config_url: {config_url}");
    tracing::info!("port: {port}");
    tracing::info!("forwarders: [{}]", forwarders.iter().join(", "));
    tracing::info!("forwarders_port: {forwarders_port}");
    tracing::info!("update_interval: {update_interval:?}");

    tracing::info!("Validating adblock config. config_url: {config_url}");
    let mut delay = Duration::from_secs(5);
    for attempt in 1u32.. {
        match Config::load(&config_url).await {
            Ok(_) => break,
            Err(err) if attempt >= 5 => {
                tracing::error!("Validating adblock config failed after {attempt} attempts: {err}");
                return Err(err.into());
            }
            Err(err) => {
                tracing::warn!(
                    "Validating adblock config failed (attempt {attempt}): {err}. Retrying in {delay:?}"
                );
                tokio::time::sleep(delay).await;
                delay = (delay * 2).min(Duration::from_secs(60));
            }
        }
    }
    tracing::info!("Validating adblock config. config_url: {config_url}. DONE");

    let engine = Arc::new(AdblockEngine::new(config_url)?);

    let tracker = TaskTracker::new();
    let token = CancellationToken::new();

    tracing::info!("Starting engine-update task");
    let cloned_engine = engine.clone();
    let cloned_token = token.clone();
    tracker.spawn(async move {
        loop {
            tracing::info!("engine-update running db update");
            if let Err(err) = cloned_engine.run_update().await {
                tracing::warn!(
                    "engine-update running db update. ERROR: {err}. Keeping existing db, will retry next interval."
                );
            } else {
                tracing::info!("engine-update running db update. DONE");
            }

            tracing::info!("engine-update sleeping for {update_interval:?}");
            tokio::select! {
                _ = tokio::time::sleep(update_interval) => {
                    tracing::info!("engine-update waking up");
                }
                _ = cloned_token.cancelled() => {
                    tracing::info!("engine-update received cancel signal");
                    return;
                }
            }
        }
    });
    tracing::info!("Starting engine-update task. DONE");

    let resolver = if forwarders.is_empty() {
        tracing::info!("Starting bind");
        let cloned_token = token.clone();
        tracker.spawn(async move {
            let mut child = match spawn_bind() {
                Ok(child) => child,
                Err(err) => {
                    tracing::error!("Starting bind. ERROR: {err}");
                    cloned_token.cancel();
                    return;
                }
            };
            tracing::info!("Starting bind. DONE");

            tokio::select! {
                _ = cloned_token.cancelled() => {
                    tracing::info!("bind received cancel signal");
                    let _ = child.kill().await;
                },
                _ = child.wait() => {
                    tracing::info!("bind ended prematurely");
                    cloned_token.cancel();
                },
            }
        });

        Resolver::new(&[BIND_IP], &BIND_PORT)
    } else {
        Resolver::new(&forwarders, &forwarders_port)
    };

    let query_log = Arc::new(QueryLogStore::new());
    let rate_limiter = new_rate_limiter(rate_limit).map(Arc::new);
    let handler = Handler::new(
        engine,
        resolver,
        query_log.clone(),
        rate_limiter,
        rate_limit_ipv4_prefix,
        rate_limit_ipv6_prefix,
    );

    tracing::info!("Starting dns server");
    let mut server = ServerFuture::new(handler);
    let v4_addr = SocketAddr::from(([0, 0, 0, 0], port));
    let v6_addr = SocketAddr::from(([0, 0, 0, 0, 0, 0, 0, 0], port));
    server.register_listener(net::bind_tcp(v4_addr)?, TCP_TIMEOUT);
    server.register_listener(net::bind_tcp(v6_addr)?, TCP_TIMEOUT);
    server.register_socket(net::bind_udp(v4_addr)?);
    server.register_socket(net::bind_udp(v6_addr)?);

    let tls_resolver = if tls_enabled {
        let domain = tls_domain.expect("TLS_DOMAIN is required when TLS_ENABLED=true");
        let email = tls_email.expect("TLS_EMAIL is required when TLS_ENABLED=true");

        tracing::info!("Setting up TLS/ACME for domain: {domain}");
        let resolver = setup_tls(
            domain.clone(),
            email,
            acme_url,
            acme_cache_dir,
            acme_insecure,
            &tracker,
            token.clone(),
        )
        .await;

        tracing::info!("Registering DoT listener on port 853");
        let dot_v4 = SocketAddr::from(([0, 0, 0, 0], 853));
        let dot_v6 = SocketAddr::from(([0, 0, 0, 0, 0, 0, 0, 0], 853));
        server.register_tls_listener(net::bind_tcp(dot_v4)?, TCP_TIMEOUT, resolver.clone())?;
        server.register_tls_listener(net::bind_tcp(dot_v6)?, TCP_TIMEOUT, resolver.clone())?;

        tracing::info!("Registering DoH listener on port 443");
        let doh_v4 = SocketAddr::from(([0, 0, 0, 0], 443));
        let doh_v6 = SocketAddr::from(([0, 0, 0, 0, 0, 0, 0, 0], 443));
        server.register_https_listener(
            net::bind_tcp(doh_v4)?,
            TCP_TIMEOUT,
            resolver.clone(),
            Some(domain.clone()),
            "/dns-query".to_string(),
        )?;
        server.register_https_listener(
            net::bind_tcp(doh_v6)?,
            TCP_TIMEOUT,
            resolver.clone(),
            Some(domain),
            "/dns-query".to_string(),
        )?;
        tracing::info!("TLS/ACME setup done");

        Some(resolver)
    } else {
        None
    };

    tracing::info!("Starting admin HTTP server on port {admin_port}");
    let cloned_query_log = query_log.clone();
    let cloned_token = token.clone();
    tracker.spawn(admin::serve(
        admin_port,
        cloned_query_log,
        tls_resolver,
        cloned_token,
    ));

    tracker.close();

    tracing::info!("Starting dns server. DONE");

    tokio::select! {
        res = sigint() => match res {
            Ok(()) => {
                tracing::info!("Received sigint signal");
            }
            Err(err) => {
                tracing::info!("Unable to listen for sigint signal: {err}");
            }
        },
        res = sigterm() => match res {
            Ok(()) => {
                tracing::info!("Received sigterm signal");
            }
            Err(err) => {
                tracing::info!("Unable to listen for sigterm signal: {err}");
            }
        },
        _ = tracker.wait() => {
            tracing::info!("Tasks ended prematurely");
        },
    }

    tracing::info!("Shutting down tasks");
    token.cancel();
    tracing::info!("Waiting for tasks to end");
    tracker.wait().await;
    tracing::info!("Stopping server");
    server.shutdown_gracefully().await?;
    tracing::info!("Stopping server. DONE");

    Ok(())
}
