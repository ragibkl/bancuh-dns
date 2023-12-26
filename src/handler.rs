use std::net::{Ipv4Addr, Ipv6Addr};

use hickory_resolver::{
    error::ResolveErrorKind,
    proto::rr::{
        rdata::{A, AAAA, CNAME},
        RData, Record,
    },
    Name,
};
use hickory_server::{
    authority::MessageResponseBuilder,
    proto::op::{Header, MessageType, OpCode, ResponseCode},
    server::{Request, RequestHandler, ResponseHandler, ResponseInfo},
};

use crate::{engine::AdblockEngine, resolver::Resolver};

#[derive(Debug, thiserror::Error)]
#[error("HandlerError: {1}")]
pub struct HandlerError(ResponseCode, String);

impl HandlerError {
    pub fn refused(msg: impl ToString) -> Self {
        Self(ResponseCode::Refused, msg.to_string())
    }

    pub fn serv_fail(err: impl ToString) -> Self {
        Self(ResponseCode::ServFail, err.to_string())
    }

    pub fn nx_domain(domain: impl ToString) -> Self {
        Self(
            ResponseCode::NXDomain,
            format!("No record found for {}", domain.to_string()),
        )
    }
}

impl From<std::io::Error> for HandlerError {
    fn from(err: std::io::Error) -> Self {
        Self::serv_fail(err)
    }
}

impl From<hickory_resolver::error::ResolveError> for HandlerError {
    fn from(value: hickory_resolver::error::ResolveError) -> Self {
        match value.kind() {
            ResolveErrorKind::NoRecordsFound { query, .. } => Self::nx_domain(query.name()),
            _ => Self::serv_fail(value),
        }
    }
}

/// DNS Request Handler
#[derive(Debug)]
pub struct Handler {
    engine: AdblockEngine,
    resolver: Resolver,
}

impl Handler {
    pub fn new(engine: AdblockEngine, resolver: Resolver) -> Self {
        Self { engine, resolver }
    }
}

impl Handler {
    async fn do_handle_request<R: ResponseHandler>(
        &self,
        request: &Request,
        responder: &mut R,
    ) -> Result<ResponseInfo, HandlerError> {
        // make sure the request is a query
        if request.op_code() != OpCode::Query {
            return Err(HandlerError::refused("Unsupported OpCode"));
        }

        // make sure the message type is a query
        if request.message_type() != MessageType::Query {
            return Err(HandlerError::refused("Unsupported MessageType"));
        }

        let name = request.query().name();

        // check engine for domain override redirection
        if let Some(alias) = self.engine.get_redirect(&name.to_string()).await {
            let mut records = Vec::new();

            // include a cname record in the response
            let cname = Name::from_utf8(&alias).unwrap();
            let cname_rdata = RData::CNAME(CNAME(cname));
            let cname_record = Record::from_rdata(request.query().name().into(), 60, cname_rdata);
            records.push(cname_record);

            // fetch records from forward resolver using the alias and return them
            let alias_records = self
                .resolver
                .lookup(&alias, request.query().query_type())
                .await?;
            records.extend(alias_records);

            return self.send_response(request, responder, &records).await;
        }

        // check engine if domain is blocked
        if self.engine.is_blocked(&name.to_string()).await {
            match request.query().query_type() {
                hickory_resolver::proto::rr::RecordType::A => {
                    let a = Ipv4Addr::new(0, 0, 0, 0);
                    let a_rdata = RData::A(A(a));
                    let a_record = Record::from_rdata(request.query().name().into(), 60, a_rdata);
                    let records = vec![a_record];

                    return self.send_response(request, responder, &records).await;
                }
                hickory_resolver::proto::rr::RecordType::AAAA => {
                    let aaaa = Ipv6Addr::new(0, 0, 0, 0, 0, 0, 0, 0);
                    let aaaa_rdata = RData::AAAA(AAAA(aaaa));
                    let aaaa_record =
                        Record::from_rdata(request.query().name().into(), 60, aaaa_rdata);
                    let records = vec![aaaa_record];

                    return self.send_response(request, responder, &records).await;
                }
                _ => return Err(HandlerError::nx_domain(name.to_string())),
            }
        }

        // fetch records from forward resolver and return them
        let records = self
            .resolver
            .lookup(&name.to_string(), request.query().query_type())
            .await?;
        self.send_response(request, responder, &records).await
    }

    /// build header and return response
    async fn send_response<R: ResponseHandler>(
        &self,
        request: &Request,
        responder: &mut R,
        records: &[Record],
    ) -> Result<ResponseInfo, HandlerError> {
        let header = Header::response_from_request(request.header());
        let response = MessageResponseBuilder::from_message_request(request).build(
            header,
            records,
            &[],
            &[],
            &[],
        );

        Ok(responder.send_response(response).await?)
    }
}

#[async_trait::async_trait]
impl RequestHandler for Handler {
    async fn handle_request<R: ResponseHandler>(
        &self,
        request: &Request,
        mut responder: R,
    ) -> ResponseInfo {
        match self.do_handle_request(request, &mut responder).await {
            Ok(info) => info,
            Err(err) => {
                let header = Header::response_from_request(request.header());
                let response =
                    MessageResponseBuilder::from_message_request(request).error_msg(&header, err.0);

                match responder.send_response(response).await {
                    Ok(ok) => ok,
                    Err(_) => {
                        let mut header = Header::new();
                        header.set_response_code(ResponseCode::ServFail);
                        header.into()
                    }
                }
            }
        }
    }
}
