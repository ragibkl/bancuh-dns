mod compiler;
mod config;
mod db;
mod fetch;
mod handler;
mod null_store;
mod resolver;

use std::{net::SocketAddr, time::Duration};

use hickory_server::{proto::udp::UdpSocket, ServerFuture};
use resolver::create_resolver;
use tokio::net::TcpListener;

use crate::handler::Handler;

const TCP_TIMEOUT: Duration = Duration::from_secs(10);

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();

    let resolver = create_resolver().await;
    let handler = Handler::init(resolver).await;
    let mut server = ServerFuture::new(handler);

    let socket_addr: SocketAddr = "0.0.0.0:1153".parse()?;
    server.register_listener(TcpListener::bind(&socket_addr).await?, TCP_TIMEOUT);
    server.register_socket(UdpSocket::bind(socket_addr).await?);

    server.block_until_done().await?;

    Ok(())
}
