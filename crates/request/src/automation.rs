use crate::{HttpRequest, RequestScripts};
use request_eagle_automation::{Body, RequestInput};

impl From<RequestInput> for HttpRequest {
    fn from(input: RequestInput) -> Self {
        Self {
            method: serde_json::from_value(serde_json::to_value(input.method).unwrap()).unwrap(),
            path: input.url,
            headers: input.headers,
            query: (!input.query.is_empty()).then_some(input.query),
            body: input.body.map(|body| match body {
                Body::Text(text) => text.into_bytes(),
                Body::Bytes(bytes) => bytes,
            }),
            scripts: RequestScripts {
                pre_request: input.pre_request,
                post_response: input.post_response,
            },
        }
    }
}

impl From<&HttpRequest> for RequestInput {
    fn from(request: &HttpRequest) -> Self {
        Self {
            method: serde_json::from_value(serde_json::to_value(request.method).unwrap()).unwrap(),
            url: request.path.clone(),
            headers: request.headers.clone(),
            query: request.query.clone().unwrap_or_default(),
            body: request
                .body
                .as_ref()
                .map(|bytes| match String::from_utf8(bytes.clone()) {
                    Ok(text) => Body::Text(text),
                    Err(_) => Body::Bytes(bytes.clone()),
                }),
            pre_request: request.scripts.pre_request.clone(),
            post_response: request.scripts.post_response.clone(),
        }
    }
}
