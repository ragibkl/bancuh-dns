use std::net::IpAddr;

use hickory_server::{
    authority::MessageResponseBuilder,
    proto::{
        op::{Header, MessageType, OpCode, ResponseCode},
        rr::{RData, Record},
    },
    server::{Request, RequestHandler, ResponseHandler, ResponseInfo},
};

#[derive(Debug, thiserror::Error)]
#[error("HandlerError: {0}")]
pub struct HandlerError(String);

impl HandlerError {
    pub fn from_str(err: impl ToString) -> Self {
        Self(err.to_string())
    }
}

impl From<std::io::Error> for HandlerError {
    fn from(err: std::io::Error) -> Self {
        Self(err.to_string())
    }
}

/// DNS Request Handler
#[derive(Clone, Debug)]
pub struct Handler;

impl Handler {
    async fn do_handle_request<R: ResponseHandler>(
        &self,
        request: &Request,
        mut responder: R,
    ) -> Result<ResponseInfo, HandlerError> {
        // make sure the request is a query
        if request.op_code() != OpCode::Query {
            return Err(HandlerError::from_str("Invalid OpCode"));
        }

        // make sure the message type is a query
        if request.message_type() != MessageType::Query {
            return Err(HandlerError::from_str("Invalid MessageType"));
        }

        // let name = request.query().name();

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
        responder: R,
    ) -> ResponseInfo {
        dbg!(request);

        match self.do_handle_request(request, responder).await {
            Ok(info) => info,
            Err(_err) => {
                let mut header = Header::new();
                header.set_response_code(ResponseCode::ServFail);
                return header.into();
            }
        }
    }
}
