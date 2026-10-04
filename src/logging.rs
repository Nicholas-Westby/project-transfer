//! The log file in the data home, plus stderr while developing.

use anyhow::Context;
use std::path::Path;
use tracing_appender::non_blocking::WorkerGuard;
use tracing_appender::rolling::{RollingFileAppender, Rotation};
use tracing_subscriber::{EnvFilter, fmt, layer::SubscriberExt, util::SubscriberInitExt};

/// Keep the log folder from growing without bound.
const KEEP_DAYS: usize = 14;

/// Starts logging to `<logs_dir>/project-transfer.<date>.log` (a new file each
/// day) and to stderr, at `info` unless `RUST_LOG` says otherwise. Keep the
/// guard alive for as long as the app runs: dropping it flushes the file.
pub fn init(logs_dir: &Path) -> anyhow::Result<WorkerGuard> {
    let file = RollingFileAppender::builder()
        .rotation(Rotation::DAILY)
        .filename_prefix("project-transfer")
        .filename_suffix("log")
        .max_log_files(KEEP_DAYS)
        .build(logs_dir)
        .with_context(|| format!("could not open a log file in {}", logs_dir.display()))?;
    let (writer, guard) = tracing_appender::non_blocking(file);
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    // A second init (another test, say) keeps the first subscriber; that is
    // harmless, so it is not an error.
    let _ = tracing_subscriber::registry()
        .with(filter)
        .with(fmt::layer().with_ansi(false).with_writer(writer))
        .with(fmt::layer().with_writer(std::io::stderr))
        .try_init();
    Ok(guard)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_events_to_a_dated_file_in_the_logs_folder() {
        let dir = tempfile::tempdir().unwrap();
        let guard = init(dir.path()).unwrap();
        tracing::info!("logging test marker");
        drop(guard);
        let files: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .map(|e| e.unwrap().path())
            .collect();
        assert_eq!(files.len(), 1, "{files:?}");
        let name = files[0].file_name().unwrap().to_string_lossy().into_owned();
        assert!(
            name.starts_with("project-transfer.") && name.ends_with(".log"),
            "{name}"
        );
        let body = std::fs::read_to_string(&files[0]).unwrap();
        assert!(body.contains("logging test marker"), "{body}");
    }
}
