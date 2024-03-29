mod bind;
mod compiler;
mod config;
mod db;
mod engine;
mod fetch;
mod handler;
mod resolver;

use std::{
    net::{IpAddr, Ipv4Addr, SocketAddr},
    sync::Arc,
    time::Duration,
};

use clap::Parser;
use hickory_server::{proto::udp::UdpSocket, ServerFuture};
use itertools::Itertools;
use tokio::{
    net::TcpListener,
    signal::unix::{signal, SignalKind},
};
use tokio_util::{sync::CancellationToken, task::TaskTracker};

use crate::{
    bind::spawn_bind,
    config::{Config, FileOrUrl},
    engine::AdblockEngine,
    handler::Handler,
    resolver::Resolver,
};

const TCP_TIMEOUT: Duration = Duration::from_secs(10);
const UPDATE_INTERVAL: Duration = Duration::from_secs(86400); // 1 day
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
    } = Args::parse();

    tracing::info!("config_url: {config_url}");
    tracing::info!("port: {port}");
    tracing::info!("forwarders: [{}]", forwarders.iter().join(", "));
    tracing::info!("forwarders_port: {forwarders_port}");

    tracing::info!("Validating adblock config. config_url: {config_url}");
    Config::load(&config_url).await?;
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
                tracing::info!("engine-update running db update. ERROR: {err}");
                cloned_token.cancel();
                return;
            }
            tracing::info!("engine-update running db update. DONE");

            tracing::info!("engine-update sleeping for 1 day");
            tokio::select! {
                _ = tokio::time::sleep(UPDATE_INTERVAL) => {
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

    tracker.close();

    let handler = Handler::new(engine, resolver);

    tracing::info!("Starting dns server");
    let mut server = ServerFuture::new(handler);
    let socket_addr = SocketAddr::from(([0, 0, 0, 0, 0, 0, 0, 0], port));
    server.register_listener(TcpListener::bind(&socket_addr).await?, TCP_TIMEOUT);
    server.register_socket(UdpSocket::bind(socket_addr).await?);
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
