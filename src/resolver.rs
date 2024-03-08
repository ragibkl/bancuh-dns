use std::net::{IpAddr, SocketAddr};

use hickory_resolver::{
    config::{NameServerConfig, Protocol, ResolverConfig, ResolverOpts},
    error::{ResolveError, ResolveErrorKind},
    proto::{
        op::ResponseCode,
        rr::{Record, RecordType},
    },
    TokioAsyncResolver,
};
use itertools::Itertools;

pub fn create_resolver(forwarders: &[IpAddr], port: &u16) -> TokioAsyncResolver {
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

    TokioAsyncResolver::tokio(config, options)
}

#[derive(Debug)]
pub struct Resolver {
    resolver: TokioAsyncResolver,
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
            Err(err) => match err.kind() {
                ResolveErrorKind::NoRecordsFound {
                    response_code: ResponseCode::NoError,
                    ..
                } => Ok(Vec::new()),
                _ => Err(err),
            },
        }
    }
}
