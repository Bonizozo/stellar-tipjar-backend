//! Helpers for propagating W3C trace context on outbound `reqwest` calls.
//!
//! Usage:
//! ```ignore
//! use crate::telemetry::http_client::inject_trace_headers;
//!
//! let mut headers = reqwest::header::HeaderMap::new();
//! inject_trace_headers(&mut headers);
//! let resp = client.get(url).headers(headers).send().await?;
//! ```

use opentelemetry::global;
use opentelemetry::propagation::Injector;
use reqwest::header::{HeaderMap, HeaderName, HeaderValue};

struct ReqwestHeaderInjector<'a>(&'a mut HeaderMap);

impl<'a> Injector for ReqwestHeaderInjector<'a> {
    fn set(&mut self, key: &str, value: String) {
        if let (Ok(name), Ok(val)) = (
            HeaderName::from_bytes(key.as_bytes()),
            HeaderValue::from_str(&value),
        ) {
            self.0.insert(name, val);
        }
    }
}

/// Inject the current W3C `traceparent` / `tracestate` context into a
/// `reqwest::header::HeaderMap`.  Call before sending an outbound request so
/// downstream services can continue the distributed trace.
pub fn inject_trace_headers(headers: &mut HeaderMap) {
    let cx = opentelemetry::Context::current();
    global::get_text_map_propagator(|p| {
        p.inject_context(&cx, &mut ReqwestHeaderInjector(headers));
    });
}
