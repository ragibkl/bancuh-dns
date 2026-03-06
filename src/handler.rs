use std::{
    net::{IpAddr, Ipv4Addr, Ipv6Addr},
    sync::Arc,
};

use chrono::Utc;

use hickory_resolver::{
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

use crate::{
    engine::AdblockEngine,
    query_log::{QueryLog, QueryLogStore},
    rate_limiter::{mask_ip, RateLimiter},
    resolver::Resolver,
};

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

impl From<hickory_resolver::ResolveError> for HandlerError {
    fn from(err: hickory_resolver::ResolveError) -> Self {
        if err.is_nx_domain() || err.is_no_records_found() {
            Self::nx_domain(err.to_string())
        } else {
            Self::serv_fail(err)
        }
    }
}

impl From<hickory_resolver::proto::ProtoError> for HandlerError {
    fn from(err: hickory_resolver::proto::ProtoError) -> Self {
        Self::serv_fail(err)
    }
}

impl From<crate::engine::EngineError> for HandlerError {
    fn from(err: crate::engine::EngineError) -> Self {
        Self::serv_fail(err)
    }
}

/// DNS Request Handler
pub struct Handler {
    engine: Arc<AdblockEngine>,
    resolver: Resolver,
    query_log: Arc<QueryLogStore>,
    rate_limiter: Option<Arc<RateLimiter>>,
    rate_limit_ipv4_prefix: u8,
    rate_limit_ipv6_prefix: u8,
}

impl Handler {
    pub fn new(
        engine: Arc<AdblockEngine>,
        resolver: Resolver,
        query_log: Arc<QueryLogStore>,
        rate_limiter: Option<Arc<RateLimiter>>,
        rate_limit_ipv4_prefix: u8,
        rate_limit_ipv6_prefix: u8,
    ) -> Self {
        Self {
            engine,
            resolver,
            query_log,
            rate_limiter,
            rate_limit_ipv4_prefix,
            rate_limit_ipv6_prefix,
        }
    }
}

impl Handler {
    /// Returns (ResponseInfo, question_string, answer_classification)
    async fn do_handle_request<R: ResponseHandler>(
        &self,
        request: &Request,
        responder: &mut R,
    ) -> Result<(ResponseInfo, String, String), HandlerError> {
        // make sure the request is a query
        if request.op_code() != OpCode::Query {
            return Err(HandlerError::refused("Unsupported OpCode"));
        }

        // make sure the message type is a query
        if request.message_type() != MessageType::Query {
            return Err(HandlerError::refused("Unsupported MessageType"));
        }

        let request_info = request.request_info().map_err(HandlerError::serv_fail)?;
        let name = request_info.query.name();
        let question = format!("{} {}", name, request_info.query.query_type());

        // check engine for domain override redirection
        if let Some(alias) = self.engine.get_redirect(&name.to_string()).await? {
            let mut records = Vec::new();

            // include a cname record in the response
            let cname = Name::from_utf8(&alias)?;
            let rdata = RData::CNAME(CNAME(cname));
            let record = Record::from_rdata(request_info.query.name().into(), 60, rdata);
            records.push(record);

            // fetch records from forward resolver using the alias and return them
            let alias_records = self
                .resolver
                .lookup(&alias, request_info.query.query_type())
                .await?;
            records.extend(alias_records);

            let info = self.send_response(request, responder, &records).await?;
            return Ok((info, question, format!("rewritten: {alias}")));
        }

        // check engine if domain is blocked
        if self.engine.is_blocked(&name.to_string()).await? {
            match request_info.query.query_type() {
                hickory_resolver::proto::rr::RecordType::A => {
                    let ipv4_null_addr = Ipv4Addr::new(0, 0, 0, 0);
                    let rdata = RData::A(A(ipv4_null_addr));
                    let record = Record::from_rdata(request_info.query.name().into(), 60, rdata);
                    let records = vec![record];

                    let info = self.send_response(request, responder, &records).await?;
                    return Ok((info, question, "blocked".to_string()));
                }
                hickory_resolver::proto::rr::RecordType::AAAA => {
                    let ipv6_null_addr = Ipv6Addr::new(0, 0, 0, 0, 0, 0, 0, 0);
                    let rdata = RData::AAAA(AAAA(ipv6_null_addr));
                    let record = Record::from_rdata(request_info.query.name().into(), 60, rdata);
                    let records = vec![record];

                    let info = self.send_response(request, responder, &records).await?;
                    return Ok((info, question, "blocked".to_string()));
                }
                _ => return Err(HandlerError::nx_domain(name.to_string())),
            }
        }

        // fetch records from forward resolver and return them
        let records = self
            .resolver
            .lookup(&name.to_string(), request_info.query.query_type())
            .await?;
        let info = self.send_response(request, responder, &records).await?;
        Ok((info, question, "forwarded".to_string()))
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

fn normalize_ip(addr: std::net::SocketAddr) -> IpAddr {
    match addr.ip() {
        IpAddr::V6(v6) => {
            // Convert IPv4-mapped IPv6 (::ffff:x.x.x.x) back to IPv4
            if let Some(v4) = v6.to_ipv4_mapped() {
                IpAddr::V4(v4)
            } else {
                IpAddr::V6(v6)
            }
        }
        ip => ip,
    }
}

#[async_trait::async_trait]
impl RequestHandler for Handler {
    async fn handle_request<R: ResponseHandler>(
        &self,
        request: &Request,
        mut responder: R,
    ) -> ResponseInfo {
        let src_ip = normalize_ip(request.src());

        // Rate limiting check — silently drop to avoid backscatter from spoofed IPs
        let rate_key = mask_ip(
            src_ip,
            self.rate_limit_ipv4_prefix,
            self.rate_limit_ipv6_prefix,
        );
        if self
            .rate_limiter
            .as_ref()
            .is_some_and(|rl| rl.check_key(&rate_key).is_err())
        {
            tracing::warn!("rate limited (dropped): {src_ip}");
            let mut header = Header::new();
            header.set_response_code(ResponseCode::Refused);
            return header.into();
        }

        match self.do_handle_request(request, &mut responder).await {
            Ok((info, question, answer)) => {
                self.query_log.insert(
                    src_ip,
                    QueryLog {
                        query_time: Utc::now(),
                        question,
                        answer,
                    },
                );
                info
            }
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
