//! Rotating file logging via `tracing`.
//!
//! One file per day, seven days retained. The returned guard must stay alive for the
//! lifetime of the process - dropping it flushes and stops the background writer.

use anyhow::{Context, Result};
use std::path::Path;
use tracing_appender::non_blocking::WorkerGuard;
use tracing_appender::rolling::{RollingFileAppender, Rotation};
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::EnvFilter;

const DEFAULT_FILTER: &str = "frigate_popup_lib=debug,frigate_popup=debug,warn";

pub fn init(dir: &Path) -> Result<WorkerGuard> {
    std::fs::create_dir_all(dir)
        .with_context(|| format!("creating log directory {}", dir.display()))?;

    let appender = RollingFileAppender::builder()
        .rotation(Rotation::DAILY)
        .filename_prefix("frigate-popup")
        .filename_suffix("log")
        .max_log_files(7)
        .build(dir)
        .with_context(|| format!("opening log file in {}", dir.display()))?;

    let (writer, guard) = tracing_appender::non_blocking(appender);

    let filter = EnvFilter::try_from_default_env()
        .or_else(|_| EnvFilter::try_new(DEFAULT_FILTER))
        .context("building the log filter")?;

    let file_layer = tracing_subscriber::fmt::layer()
        .with_writer(writer)
        .with_ansi(false)
        .with_target(true);

    let registry = tracing_subscriber::registry().with(filter).with(file_layer);

    // Debug builds keep a console attached, so mirror everything there too.
    #[cfg(debug_assertions)]
    let registry = registry.with(tracing_subscriber::fmt::layer().with_target(true));

    registry
        .try_init()
        .context("installing the tracing subscriber")?;

    Ok(guard)
}
