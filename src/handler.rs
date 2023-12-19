use hickory_resolver::{error::ResolveErrorKind, TokioAsyncResolver};
use hickory_server::{
    authority::MessageResponseBuilder,
    proto::op::{Header, MessageType, OpCode, ResponseCode},
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
    null_store: NullStore,
    resolver: TokioAsyncResolver,
}

impl Handler {
    pub async fn init(resolver: TokioAsyncResolver) -> Self {
        let mut null_store = NullStore::new();
        null_store.fetch().await;

        Self {
            null_store,
            resolver,
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

        // check null store for blocked domain
        let name = request.query().name();
        if self.null_store.is_blocked(&name.to_string()).await {
            return Err(HandlerError::nx_domain(name.to_string()));
        }

        // fetch record from forward resolver
        let records = match self
            .resolver
            .lookup(name, request.query().query_type())
            .await
        {
            Ok(lookup) => lookup.records().to_owned(),
            Err(err) => match err.kind() {
                ResolveErrorKind::NoRecordsFound {
                    response_code: ResponseCode::NoError,
                    ..
                } => Vec::new(),
                _ => return Err(err.into()),
            },
        };

        // build header and return response
        let header = Header::response_from_request(request.header());
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
