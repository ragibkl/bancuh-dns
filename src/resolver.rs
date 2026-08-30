use std::net::IpAddr;

use hickory_resolver::{
    TokioResolver,
    config::{ConnectionConfig, NameServerConfig, ProtocolConfig, ResolverConfig, ResolverOpts},
    net::{DnsError, NetError, NoRecords, runtime::TokioRuntimeProvider},
    proto::{
        op::ResponseCode,
        rr::{Record, RecordType},
    },
};
use itertools::Itertools;

pub fn create_resolver(forwarders: &[IpAddr], port: &u16) -> Result<TokioResolver, NetError> {
    tracing::info!(
        "Setting up forwarders: [{}] on port: {port}",
        forwarders.iter().join(", ")
    );

    let mut config = ResolverConfig::default();
    forwarders.iter().for_each(|f| {
        tracing::info!("Setting up forwarder: {f}:{port}");
        let mut connection = ConnectionConfig::new(ProtocolConfig::Udp);
        connection.port = *port;
        let name_server = NameServerConfig::new(*f, true, vec![connection]);
        config.add_name_server(name_server);
    });

    let options = ResolverOpts::default();

    TokioResolver::builder_with_config(config, TokioRuntimeProvider::default())
        .with_options(options)
        .build()
}

#[derive(Debug)]
pub struct Resolver {
    resolver: TokioResolver,
}

impl Resolver {
    pub fn new(forwarders: &[IpAddr], port: &u16) -> Result<Self, NetError> {
        let resolver = create_resolver(forwarders, port)?;
        Ok(Self { resolver })
    }

    /// Lookup records from forward resolver
    ///
    /// A genuine NODATA answer (the name exists, but holds no records of this type) is
    /// reported as `NoRecordsFound` carrying a `NoError` response code, and is returned
    /// here as an empty Vec.
    ///
    /// Match on the response code rather than `is_no_records_found()`: that predicate is
    /// still true for NXDOMAIN, and answering an upstream failure with an empty success
    /// would hide it from clients, which is what it used to do before hickory 0.26 split
    /// real failures out into `DnsError::ResponseCode`.
    pub async fn lookup(
        &self,
        name: &str,
        query_type: RecordType,
    ) -> Result<Vec<Record>, NetError> {
        match self.resolver.lookup(name, query_type).await {
            Ok(lookup) => Ok(lookup.answers().to_vec()),
            Err(NetError::Dns(DnsError::NoRecordsFound(NoRecords {
                response_code: ResponseCode::NoError,
                ..
            }))) => Ok(Vec::new()),
            Err(err) => Err(err),
        }
    }
}
