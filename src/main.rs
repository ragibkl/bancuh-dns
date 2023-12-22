mod compiler;
mod config;
mod db;
mod engine;
mod fetch;
mod handler;
mod resolver;

use std::{net::SocketAddr, time::Duration};

use clap::Parser;
use config::FileOrUrl;
use engine::AdblockEngine;
use hickory_server::{proto::udp::UdpSocket, ServerFuture};
use resolver::create_resolver;
use tokio::{net::TcpListener, signal};

use crate::{config::Config, handler::Handler};

const TCP_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Parser, Debug)]
#[command(name = "Bancuh DNS")]
#[command(version)]
#[command(about)]
struct Args {
    /// Sets a custom config file
    #[arg(
        short,
        long,
        value_name = "CONFIG_URL",
        default_value = "https://raw.githubusercontent.com/ragibkl/adblock-dns-server/master/data/configuration.yaml"
    )]
    config_url: FileOrUrl,

    /// Sets a custom listener port
    #[arg(short, long, value_name = "PORT", default_value = "53")]
    port: u16,

    /// Sets a custom forward resolvers
    #[arg(
        short,
        long,
        value_name = "FORWARDERS",
        value_delimiter = ',',
        default_value = "8.8.8.8,8.8.4.4"
    )]
    forwarders: Vec<String>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();

    let Args {
        config_url,
        port,
        forwarders,
    } = Args::parse();

    tracing::info!("config_url: {config_url}");
    tracing::info!("port: {port}");
    tracing::info!("forwarders: [{}]", forwarders.join(", "));

    tracing::info!("Validating adblock config. config_url: {config_url}");
    Config::load(&config_url).await?;
    tracing::info!("Validating adblock config. config_url: {config_url}. DONE");

    let engine = AdblockEngine::new(config_url);
    engine.start_update();

    let resolver = create_resolver(&forwarders);
    let handler = Handler::new(engine, resolver);

    tracing::info!("Starting server");
    let mut server = ServerFuture::new(handler);
    let socket_addr: SocketAddr = format!("0.0.0.0:{port}").parse()?;
    server.register_listener(TcpListener::bind(&socket_addr).await?, TCP_TIMEOUT);
    server.register_socket(UdpSocket::bind(socket_addr).await?);
    tracing::info!("Starting server. DONE");

    match signal::ctrl_c().await {
        Ok(()) => {
            tracing::info!("Received shutdown signal");
        }
        Err(err) => {
            tracing::info!("Unable to listen for shutdown signal: {err}");
        }
    }

    tracing::info!("Stopping server");
    server.shutdown_gracefully().await?;
    tracing::info!("Stopping server. DONE");

    Ok(())
}
