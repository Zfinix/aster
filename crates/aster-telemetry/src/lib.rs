//! OTLP span export, off unless an endpoint is configured, so a default run
//! pays nothing and no collector is needed to use aster.

#![forbid(unsafe_code)]

use anyhow::{Context, Result};
use opentelemetry::trace::TracerProvider as _;
use opentelemetry_otlp::{SpanExporter, WithExportConfig};
use opentelemetry_sdk::Resource;
use opentelemetry_sdk::trace::SdkTracerProvider;
use tracing::Subscriber;
use tracing_subscriber::Layer;
use tracing_subscriber::registry::LookupSpan;

/// The standard OTLP variable. Its presence is what turns export on.
pub const ENDPOINT_VAR: &str = "OTEL_EXPORTER_OTLP_ENDPOINT";

/// The per-signal override, which the spec says is already a full URL.
pub const TRACES_ENDPOINT_VAR: &str = "OTEL_EXPORTER_OTLP_TRACES_ENDPOINT";

const TRACES_PATH: &str = "/v1/traces";

/// Holds the provider open. Spans are batched, so dropping this without
/// calling [`Telemetry::shutdown`] loses whatever has not been flushed.
pub struct Telemetry {
    provider: SdkTracerProvider,
}

impl Telemetry {
    /// Flush and stop. Safe to call more than once.
    pub fn shutdown(&self) {
        if let Err(e) = self.provider.shutdown() {
            tracing::debug!("could not flush telemetry: {e}");
        }
    }
}

pub fn endpoint_from_env() -> Option<String> {
    var(TRACES_ENDPOINT_VAR).or_else(|| var(ENDPOINT_VAR))
}

fn var(name: &str) -> Option<String> {
    std::env::var(name)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

/// An endpoint passed programmatically is used verbatim by the exporter, so
/// the signal path has to be appended here. Idempotent, so an endpoint that
/// already names it is left alone.
fn traces_url(endpoint: &str) -> String {
    let endpoint = endpoint.trim_end_matches('/');
    match endpoint.ends_with(TRACES_PATH) {
        true => endpoint.to_string(),
        false => format!("{endpoint}{TRACES_PATH}"),
    }
}

/// Boxed so callers compose it without naming the exporter's types, which are
/// long and change between opentelemetry releases.
pub type SpanLayer<S> = Box<dyn Layer<S> + Send + Sync>;

/// A layer exporting to `endpoint`, which is a collector's base URL such as
/// `http://localhost:4318`. The exporter connects lazily, so this succeeds
/// whether or not a collector is listening yet.
pub fn layer<S>(service: &str, endpoint: &str) -> Result<(SpanLayer<S>, Telemetry)>
where
    S: Subscriber + for<'a> LookupSpan<'a> + Send + Sync + 'static,
{
    let url = traces_url(endpoint);
    let exporter = SpanExporter::builder()
        .with_http()
        .with_endpoint(&url)
        .build()
        .with_context(|| format!("building an OTLP exporter for {url}"))?;
    let provider = SdkTracerProvider::builder()
        .with_batch_exporter(exporter)
        .with_resource(
            Resource::builder()
                .with_service_name(service.to_string())
                .build(),
        )
        .build();
    let tracer = provider.tracer("aster");
    Ok((
        Box::new(tracing_opentelemetry::layer().with_tracer(tracer)),
        Telemetry { provider },
    ))
}

/// [`layer`] against the endpoint in the environment, or `None` when there is
/// none, which is the default and costs nothing.
pub fn from_env<S>(service: &str) -> Result<Option<(SpanLayer<S>, Telemetry)>>
where
    S: Subscriber + for<'a> LookupSpan<'a> + Send + Sync + 'static,
{
    match endpoint_from_env() {
        Some(endpoint) => Ok(Some(layer(service, &endpoint)?)),
        None => Ok(None),
    }
}

#[cfg(test)]
#[path = "lib_tests.rs"]
mod tests;
