use std::{
    cell::{Cell, RefCell},
    rc::Rc,
    time::Instant,
};

use futures::{
    future::LocalBoxFuture,
    stream::{FuturesUnordered, StreamExt},
};
use rquickjs::{Ctx, Exception, Function, Promise};
use serde::Deserialize;
use serde_json::json;

use crate::{Field, HttpRequest, Method, RequestExecutor};

const REQUEST_LIMIT: usize = 32;
const REQUEST_BYTES: usize = 1024 * 1024;
const RESPONSE_BYTES: u64 = 8 * 1024 * 1024;
const TOTAL_RESPONSE_BYTES: usize = 16 * 1024 * 1024;
const CONCURRENT_REQUESTS: usize = 4;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RequestInput {
    url: String,
    method: Method,
    headers: Vec<(String, String)>,
    body: Option<String>,
}

type Completion<'js> = (Function<'js>, Function<'js>, Result<String, String>);

/// Owned by one script phase; dropping it cancels all in-flight subrequests.
#[derive(Clone, Default)]
pub(super) struct Network<'js> {
    pending: Rc<RefCell<FuturesUnordered<LocalBoxFuture<'js, Completion<'js>>>>>,
    count: Rc<Cell<usize>>,
    response_bytes: Rc<Cell<usize>>,
}

impl<'js> Network<'js> {
    pub fn binding(
        &self,
        cx: Ctx<'js>,
        executor: &RequestExecutor,
    ) -> rquickjs::Result<Function<'js>> {
        let state = self.clone();
        let http = executor.http.with_response_limit(RESPONSE_BYTES);
        let timeout = executor.timeout;
        Function::new(cx, move |cx: Ctx<'js>, source: String| {
            if state.count.get() >= REQUEST_LIMIT {
                return Err(Exception::throw_range(
                    &cx,
                    "Scripts may send at most 32 HTTP requests per phase",
                ));
            }
            if state.pending.borrow().len() >= CONCURRENT_REQUESTS {
                return Err(Exception::throw_range(
                    &cx,
                    "Scripts may have at most 4 HTTP requests in flight; await them before sending more",
                ));
            }
            if source.len() > REQUEST_BYTES {
                return Err(Exception::throw_range(
                    &cx,
                    "Script HTTP request exceeds the 1 MiB limit",
                ));
            }
            let input: RequestInput = serde_json::from_str(&source).map_err(|error| {
                Exception::throw_type(&cx, &format!("Invalid script HTTP request: {error}"))
            })?;
            let (promise, resolve, reject) = Promise::new(&cx)?;
            let http = http.clone();
            let response_bytes = state.response_bytes.clone();
            state.count.set(state.count.get() + 1);

            state.pending.borrow_mut().push(Box::pin(async move {
                let request = HttpRequest {
                    path: input.url,
                    method: input.method,
                    headers: input.headers.into_iter().map(Field::from).collect(),
                    ..Default::default()
                };
                let body = input
                    .body
                    .filter(|_| !matches!(request.method, Method::Get | Method::Head))
                    .map(bytes::Bytes::from);
                let started = Instant::now();
                let send = async {
                    http.execute(&request, body, None).await.map(|(response, _)| response).map_err(|error| error.to_string())
                };
                let response = match timeout {
                    Some(timeout) => smol::future::or(send, async {
                        smol::Timer::after(timeout).await;
                        Err(format!("Script HTTP request timed out after {timeout:?}"))
                    }).await,
                    None => send.await,
                };
                let result = response.and_then(|response| {
                    let total = response_bytes.get().saturating_add(response.body.len());
                    response_bytes.set(total);
                    if total > TOTAL_RESPONSE_BYTES {
                        return Err("Script HTTP responses exceed the 16 MiB phase limit".into());
                    }
                    Ok(json!({
                    "code": response.status.as_u16(),
                    "status": response.status.canonical_reason().unwrap_or(""),
                    "responseTime": started.elapsed().as_secs_f64() * 1000.,
                    "headers": response.headers.iter().map(|(key, value)| (key.as_str(), String::from_utf8_lossy(value.as_bytes()))).collect::<Vec<_>>(),
                    "body": String::from_utf8_lossy(&response.body),
                    }).to_string())
                });
                (resolve, reject, result)
            }));
            Ok(promise)
        })
    }

    pub fn is_empty(&self) -> bool {
        self.pending.borrow().is_empty()
    }

    pub async fn next(&self) -> Option<Completion<'js>> {
        std::future::poll_fn(|cx| self.pending.borrow_mut().poll_next_unpin(cx)).await
    }

    pub fn clear(&self) {
        self.pending.borrow_mut().clear();
    }
}
