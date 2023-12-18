use std::net::IpAddr;

use hickory_server::{
    authority::MessageResponseBuilder,
    proto::{
        op::{Header, MessageType, OpCode, ResponseCode},
        rr::{RData, Record},
    },
    server::{Request, RequestHandler, ResponseHandler, ResponseInfo},
};

use crate::null_store::NullStore;

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

/// DNS Request Handler
#[derive(Clone, Debug)]
pub struct Handler {
    null_store: NullStore,
}

impl Handler {
    pub fn new() -> Self {
        Self {
            null_store: NullStore,
        }
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
        if self.null_store.is_blocked(name.to_string().as_str()).await {
            return Err(HandlerError::nx_domain(name.to_string()));
        }

        let mut header = Header::response_from_request(request.header());
        header.set_authoritative(true);

        let rdata = match request.src().ip() {
            IpAddr::V4(ipv4) => RData::A(ipv4.into()),
            IpAddr::V6(ipv6) => RData::AAAA(ipv6.into()),
        };
        let records = vec![Record::from_rdata(request.query().name().into(), 60, rdata)];

        let response = MessageResponseBuilder::from_message_request(request).build(
            header,
            records.iter(),
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
                let response = MessageResponseBuilder::from_message_request(request)
                    .error_msg(request.header(), err.0);

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
