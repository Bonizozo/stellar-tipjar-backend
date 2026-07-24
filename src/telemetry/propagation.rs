use axum::http::HeaderMap;
use opentelemetry::global;
use opentelemetry::propagation::{Extractor, Injector};
use opentelemetry::Context;

struct HeaderExtractor<'a>(&'a HeaderMap);

impl<'a> Extractor for HeaderExtractor<'a> {
    fn get(&self, key: &str) -> Option<&str> {
        self.0.get(key).and_then(|v| v.to_str().ok())
    }

    fn keys(&self) -> Vec<&str> {
        self.0.keys().map(|k| k.as_str()).collect()
    }
}

/// A simple HashMap-based injector / extractor for use with AMQP headers
/// and any other key-value carrier.
pub struct HashMapCarrier(pub std::collections::HashMap<String, String>);

impl Injector for HashMapCarrier {
    fn set(&mut self, key: &str, value: String) {
        self.0.insert(key.to_owned(), value);
    }
}

impl Extractor for HashMapCarrier {
    fn get(&self, key: &str) -> Option<&str> {
        self.0.get(key).map(|s| s.as_str())
    }

    fn keys(&self) -> Vec<&str> {
        self.0.keys().map(|s| s.as_str()).collect()
    }
}

/// Extract a parent `Context` from incoming HTTP headers (W3C TraceContext / Baggage).
pub fn extract_context(headers: &HeaderMap) -> Context {
    global::get_text_map_propagator(|p| p.extract(&HeaderExtractor(headers)))
}

/// Inject the current OTel context into a `HeaderMap` for outbound HTTP requests.
///
/// Call this on the `reqwest::RequestBuilder` headers before sending to propagate
/// the W3C `traceparent` / `tracestate` headers downstream.
pub fn inject_context(headers: &mut HeaderMap) {
    let cx = opentelemetry::Context::current();
    global::get_text_map_propagator(|p| {
        p.inject_context(&cx, &mut HeaderMapInjector(headers));
    });
}

struct HeaderMapInjector<'a>(&'a mut HeaderMap);

impl<'a> Injector for HeaderMapInjector<'a> {
    fn set(&mut self, key: &str, value: String) {
        if let (Ok(name), Ok(val)) = (
            axum::http::HeaderName::from_bytes(key.as_bytes()),
            axum::http::HeaderValue::from_str(&value),
        ) {
            self.0.insert(name, val);
        }
    }
}
