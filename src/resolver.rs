use std::net::SocketAddr;

use hickory_resolver::{
    config::{NameServerConfig, Protocol, ResolverConfig, ResolverOpts},
    TokioAsyncResolver,
};

pub async fn create_resolver() -> TokioAsyncResolver {
    let ns1_addr: SocketAddr = "8.8.8.8:53".parse().unwrap();
    let ns2_addr: SocketAddr = "8.8.4.4:53".parse().unwrap();

    let mut config = ResolverConfig::new();
    config.add_name_server(NameServerConfig::new(ns1_addr, Protocol::Udp));
    config.add_name_server(NameServerConfig::new(ns2_addr, Protocol::Udp));

    let options = ResolverOpts::default();

    TokioAsyncResolver::tokio(config, options)
}
