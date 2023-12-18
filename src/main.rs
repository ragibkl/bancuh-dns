mod handler;
mod null_store;

use std::{net::SocketAddr, time::Duration};

use hickory_server::{proto::udp::UdpSocket, ServerFuture};
use tokio::net::TcpListener;

use crate::handler::Handler;

const TCP_TIMEOUT: Duration = Duration::from_secs(10);

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();

    let mut server = ServerFuture::new(Handler::new());

    let socket_addr: SocketAddr = "0.0.0.0:1153".parse()?;
    server.register_listener(TcpListener::bind(&socket_addr).await?, TCP_TIMEOUT);
    server.register_socket(UdpSocket::bind(socket_addr).await?);

    server.block_until_done().await?;

    Ok(())
}
