use std::net::SocketAddr;

use socket2::{Domain, Protocol, Socket, Type};

/// Bind a TCP listener with `IPV6_V6ONLY` set for IPv6 addresses.
///
/// This prevents IPv6 sockets from capturing IPv4 traffic via dual-stack,
/// allowing separate IPv4 and IPv6 bindings on the same port.
pub fn bind_tcp(addr: SocketAddr) -> std::io::Result<tokio::net::TcpListener> {
    let domain = if addr.is_ipv4() {
        Domain::IPV4
    } else {
        Domain::IPV6
    };
    let socket = Socket::new(domain, Type::STREAM, Some(Protocol::TCP))?;
    socket.set_reuse_address(true)?;
    if addr.is_ipv6() {
        socket.set_only_v6(true)?;
    }
    socket.bind(&addr.into())?;
    socket.listen(1024)?;
    socket.set_nonblocking(true)?;
    tokio::net::TcpListener::from_std(socket.into())
}

/// Bind a UDP socket with `IPV6_V6ONLY` set for IPv6 addresses.
pub fn bind_udp(addr: SocketAddr) -> std::io::Result<tokio::net::UdpSocket> {
    let domain = if addr.is_ipv4() {
        Domain::IPV4
    } else {
        Domain::IPV6
    };
    let socket = Socket::new(domain, Type::DGRAM, Some(Protocol::UDP))?;
    socket.set_reuse_address(true)?;
    if addr.is_ipv6() {
        socket.set_only_v6(true)?;
    }
    socket.bind(&addr.into())?;
    socket.set_nonblocking(true)?;
    tokio::net::UdpSocket::from_std(socket.into())
}
