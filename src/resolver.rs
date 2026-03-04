use std::net::{IpAddr, SocketAddr};

use hickory_resolver::{
    config::{NameServerConfig, ResolverConfig, ResolverOpts},
    name_server::TokioConnectionProvider,
    proto::{
        rr::{Record, RecordType},
        xfer::Protocol,
    },
    ResolveError, Resolver as HickoryResolver,
};
use itertools::Itertools;

pub fn create_resolver(forwarders: &[IpAddr], port: &u16) -> HickoryResolver<TokioConnectionProvider> {
    tracing::info!(
        "Setting up forwarders: [{}] on port: {port}",
        forwarders.iter().join(", ")
    );

    let mut config = ResolverConfig::new();
    forwarders.iter().for_each(|f| {
        let addr = SocketAddr::new(*f, *port);
        tracing::info!("Setting up forwarder: {addr}");
        let name_server = NameServerConfig::new(addr, Protocol::Udp);
        config.add_name_server(name_server);
    });

    let options = ResolverOpts::default();

    HickoryResolver::builder_with_config(config, TokioConnectionProvider::default())
        .with_options(options)
        .build()
}

#[derive(Debug)]
pub struct Resolver {
    resolver: HickoryResolver<TokioConnectionProvider>,
}

impl Resolver {
    pub fn new(forwarders: &[IpAddr], port: &u16) -> Self {
        let resolver = create_resolver(forwarders, port);
        Self { resolver }
    }

    /// Lookup records from forward resolver
    /// If the call errors with NoRecordsFound and NoError response_code, we simply return Ok with an empty Vec
    pub async fn lookup(
        &self,
        name: &str,
        query_type: RecordType,
    ) -> Result<Vec<Record>, ResolveError> {
        match self.resolver.lookup(name, query_type).await {
            Ok(lookup) => Ok(lookup.records().to_owned()),
            Err(err) if err.is_no_records_found() && !err.is_nx_domain() => Ok(Vec::new()),
            Err(err) => Err(err),
        }
    }
}
