use std::net::SocketAddr;

use hickory_resolver::{
    config::{NameServerConfig, Protocol, ResolverConfig, ResolverOpts},
    TokioAsyncResolver,
};

pub fn create_resolver(forwarders: &[String]) -> TokioAsyncResolver {
    tracing::info!("Setting up forwarders: {}", forwarders.to_vec().join(", "));

    let mut config = ResolverConfig::new();
    forwarders.iter().for_each(|f| {
        tracing::info!("Setting up forwarder: {f}");
        let addr: SocketAddr = format!("{f}:53").parse().unwrap();
        let name_server = NameServerConfig::new(addr, Protocol::Udp);
        config.add_name_server(name_server);
    });

    let options = ResolverOpts::default();

    TokioAsyncResolver::tokio(config, options)
}
