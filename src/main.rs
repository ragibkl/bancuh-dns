mod compiler;
mod config;
mod db;
mod engine;
mod fetch;
mod handler;
mod resolver;

use std::{net::SocketAddr, time::Duration};

use clap::Parser;
use config::ConfigUrl;
use engine::AdblockEngine;
use hickory_server::{proto::udp::UdpSocket, ServerFuture};
use resolver::create_resolver;
use tokio::net::TcpListener;

use crate::handler::Handler;

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
    config_url: ConfigUrl,

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

    let args = Args::parse();
    tracing::info!("config_url: {}", &args.config_url);
    tracing::info!("port: {}", &args.port);
    tracing::info!("forwarders: [{}]", &args.forwarders.to_vec().join(", "));

    let mut engine = AdblockEngine::new(args.config_url);
    engine.start_update().await;

    let resolver = create_resolver(&args.forwarders);
    let handler = Handler::new(engine, resolver);

    let mut server = ServerFuture::new(handler);
    let socket_addr: SocketAddr = format!("0.0.0.0:{}", args.port).parse()?;
    server.register_listener(TcpListener::bind(&socket_addr).await?, TCP_TIMEOUT);
    server.register_socket(UdpSocket::bind(socket_addr).await?);

    server.block_until_done().await?;

    Ok(())
}
