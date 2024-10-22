use std::fs;
use std::io;
use std::path::Path;
use std::str::FromStr;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;

use tracing::level_filters::LevelFilter;
use tracing::Level;
use tracing_chrome::FlushGuard;
use tracing_subscriber::fmt::writer::BoxMakeWriter;
use tracing_subscriber::{fmt::format::FmtSpan, layer::Filter};
use tracing_subscriber::{EnvFilter, Layer};

pub mod otel {
  pub use opentelemetry;
  pub use opentelemetry_sdk as sdk;
  pub use tracing_opentelemetry as tracing;
}

pub mod chrome {
  pub use tracing_chrome::FlushGuard;
}

static IS_TRACING_ENABLED: AtomicBool = AtomicBool::new(false);

// skip event because it's not useful for performance analysis
struct FilterEvent;

impl<S> Filter<S> for FilterEvent {
  fn enabled(
    &self,
    meta: &tracing::Metadata<'_>,
    _cx: &tracing_subscriber::layer::Context<'_, S>,
  ) -> bool {
    !meta.is_event()
  }
}

pub struct OtelGuard {
  trace_provider: opentelemetry_sdk::trace::TracerProvider,
}

impl Drop for OtelGuard {
  fn drop(&mut self) {
    let _ = self.trace_provider.shutdown();
    opentelemetry::global::shutdown_tracer_provider();
  }
}

pub fn enable_tracing_by_env_with_otel(filter: &str) -> Option<OtelGuard> {
  if !IS_TRACING_ENABLED.swap(true, Ordering::Relaxed) {
    use opentelemetry::{global, trace::TracerProvider as _, KeyValue};
    use opentelemetry_sdk::propagation::TraceContextPropagator;
    use opentelemetry_sdk::{runtime, Resource};
    use tracing_opentelemetry::OpenTelemetryLayer;
    use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

    global::set_text_map_propagator(TraceContextPropagator::new());

    let provider =
      opentelemetry_otlp::new_pipeline()
        .tracing()
        .with_exporter(opentelemetry_otlp::new_exporter().tonic())
        .with_trace_config(opentelemetry_sdk::trace::Config::default().with_resource(
          Resource::new(vec![KeyValue::new("service.name", "rspack-app")]),
        ))
        .install_batch(runtime::Tokio)
        .unwrap();
    global::set_tracer_provider(provider.clone());

    let trace = provider.tracer("rspack-app");
    dbg!(filter);

    tracing_subscriber::registry()
      .with(generate_common_layers(filter))
      .with(OpenTelemetryLayer::new(trace))
      .init();
    return Some(OtelGuard {
      trace_provider: provider,
    });
  }
  None
}

pub fn enable_tracing_by_env(filter: &str, output: &str) {
  if !IS_TRACING_ENABLED.swap(true, Ordering::Relaxed) {
    use tracing_subscriber::{fmt, prelude::*};
    let layers = generate_common_layers(filter);
    let trace_writer = TraceWriter::from(output);

    tracing_subscriber::registry()
      .with(layers)
      .with(
        fmt::layer()
          .pretty()
          .with_file(true)
          // To keep track of the closing point of spans
          .with_span_events(FmtSpan::CLOSE)
          .with_writer(trace_writer.make_writer()),
      )
      .init();
    tracing::trace!("enable_tracing_by_env");
  }
}

fn generate_common_layers(
  filter: &str,
) -> Vec<Box<dyn Layer<tracing_subscriber::Registry> + Send + Sync>> {
  let default_level = Level::from_str(filter).ok();

  let mut layers = vec![];
  if let Some(default_level) = default_level {
    layers.push(
      tracing_subscriber::filter::Targets::new()
        .with_target("rspack_core", default_level)
        .with_target("node_binding", default_level)
        .with_target("rspack_loader_swc", default_level)
        .with_target("rspack_loader_runner", default_level)
        .with_target("rspack_plugin_javascript", default_level)
        .with_target("rspack_resolver", Level::WARN)
        .boxed(),
    );
  } else {
    // SAFETY: we know that trace_var is `Ok(StrinG)` now,
    // for the second unwrap, if we can't parse the directive, then the tracing result would be
    // unexpected, then panic is reasonable
    let env_layer = EnvFilter::builder()
      .with_regex(true)
      .parse(filter)
      .expect("Parse tracing directive syntax failed,for details about the directive syntax you could refer https://docs.rs/tracing-subscriber/latest/tracing_subscriber/filter/struct.EnvFilter.html#directives");

    layers.push(env_layer.boxed());
  }
  layers
}
// register layer for tokio_console, and tokio_console use network to send trace info so no need to pass result by string back
pub fn enable_tracing_by_env_with_tokio_console() {
  if !IS_TRACING_ENABLED.swap(true, Ordering::Relaxed) {
    console_subscriber::init();
  }
}
pub fn enable_tracing_by_env_with_chrome_layer(filter: &str, output: &str) -> Option<FlushGuard> {
  if !IS_TRACING_ENABLED.swap(true, Ordering::Relaxed) {
    use tracing_chrome::ChromeLayerBuilder;
    use tracing_subscriber::prelude::*;
    let console_layer = console_subscriber::ConsoleLayer::builder().spawn();
    let trace_writer = TraceWriter::from(output);
    let (chrome_layer, guard) = ChromeLayerBuilder::new()
      .include_args(true)
      .writer(trace_writer.writer())
      .build();
    let layers = generate_common_layers(filter);
    // If we don't do this. chrome_layer will collect nothing.
    // std::mem::forget(guard);
    tracing_subscriber::registry()
      .with(layers)
      .with(chrome_layer.with_filter(FilterEvent {}))
      .with(console_layer)
      .init();
    Some(guard)
  } else {
    None
  }
}

enum TraceWriter<'a> {
  Stdout,
  Stderr,
  File { path: &'a Path },
}

impl<'a> From<&'a str> for TraceWriter<'a> {
  fn from(s: &'a str) -> Self {
    match s {
      "stdout" => Self::Stdout,
      "stderr" => Self::Stderr,
      path => Self::File {
        path: Path::new(path),
      },
    }
  }
}

impl TraceWriter<'_> {
  pub fn make_writer(&self) -> BoxMakeWriter {
    match self {
      TraceWriter::Stdout => BoxMakeWriter::new(io::stdout),
      TraceWriter::Stderr => BoxMakeWriter::new(io::stderr),
      TraceWriter::File { path } => {
        BoxMakeWriter::new(fs::File::create(path).expect("Failed to create trace file"))
      }
    }
  }

  pub fn writer(&self) -> Box<dyn io::Write + Send + 'static> {
    match self {
      TraceWriter::Stdout => Box::new(io::stdout()),
      TraceWriter::Stderr => Box::new(io::stderr()),
      TraceWriter::File { path } => {
        Box::new(fs::File::create(path).expect("Failed to create trace file"))
      }
    }
  }
}
