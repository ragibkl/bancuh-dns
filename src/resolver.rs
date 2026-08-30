use std::net::{IpAddr, SocketAddr};

use hickory_resolver::{
    config::{NameServerConfig, ResolverConfig, ResolverOpts},
    name_server::TokioConnectionProvider,
    proto::{
        op::ResponseCode,
        rr::{Record, RecordType},
        xfer::Protocol,
        ProtoErrorKind,
    },
    ResolveError, Resolver as HickoryResolver,
};
use itertools::Itertools;

pub fn create_resolver(
    forwarders: &[IpAddr],
    port: &u16,
) -> HickoryResolver<TokioConnectionProvider> {
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
    ///
    /// A genuine NODATA answer (the name exists, but holds no records of this type) is
    /// reported by hickory as `NoRecordsFound` carrying a `NoError` response code, and is
    /// returned here as an empty Vec.
    ///
    /// Note that `is_no_records_found()` is not usable for that test: hickory folds
    /// ServFail, Refused, FormErr, NotImp and the rest of the failure codes into the same
    /// `NoRecordsFound` kind, so matching on it would silently turn upstream failures into
    /// successful empty answers. Match on the response code instead, so that NXDOMAIN and
    /// real failures both propagate to the caller.
    pub async fn lookup(
        &self,
        name: &str,
        query_type: RecordType,
    ) -> Result<Vec<Record>, ResolveError> {
        match self.resolver.lookup(name, query_type).await {
            Ok(lookup) => Ok(lookup.records().to_owned()),
            Err(err)
                if matches!(
                    err.proto().map(|proto| proto.kind()),
                    Some(ProtoErrorKind::NoRecordsFound {
                        response_code: ResponseCode::NoError,
                        ..
                    })
                ) =>
            {
                Ok(Vec::new())
            }
            Err(err) => Err(err),
        }
    }
}
