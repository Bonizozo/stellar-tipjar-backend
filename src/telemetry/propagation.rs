use axum::http::HeaderMap;
use opentelemetry::Context;
use opentelemetry::propagation::Extractor;
use opentelemetry::global;

struct HeaderExtractor<'a>(&'a HeaderMap);

impl<'a> Extractor for HeaderExtractor<'a> {
    fn get(&self, key: &str) -> Option<&str> {
        self.0.get(key).and_then(|v| v.to_str().ok())
    }

    fn keys(&self) -> Vec<&str> {
        self.0.keys().map(|k| k.as_str()).collect()
    }
}

/// Extract a parent `Context` from incoming HTTP headers (W3C TraceContext / Baggage).
pub fn extract_context(headers: &HeaderMap) -> Context {
    global::get_text_map_propagator(|p| p.extract(&HeaderExtractor(headers)))
}
