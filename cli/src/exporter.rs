use std::path::PathBuf;

use anyhow::Result;
use joule_profiler_core::profiler::JouleProfiler;
use joule_profiler_exporter_csv::CsvExporter;
use joule_profiler_exporter_json::{JsonExporter, Shape};
use joule_profiler_exporter_terminal::TerminalExporter;
use serde::Deserialize;

/// The `[exporter]` table: `format` picks the exporter, the other keys are its own.
#[derive(Debug, Deserialize)]
#[serde(tag = "format", rename_all = "lowercase")]
pub enum ExporterConfig {
    Terminal(TerminalConfig),
    Json(JsonConfig),
    Csv(CsvConfig),
}

impl Default for ExporterConfig {
    fn default() -> Self {
        ExporterConfig::Terminal(TerminalConfig::default())
    }
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct TerminalConfig {}

#[derive(Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct JsonConfig {
    /// Default: `data<timestamp>.json`, or `.jsonl` for lines.
    pub file: Option<PathBuf>,

    /// One object per line, written as the run goes, instead of one document at the end.
    pub lines: bool,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct CsvConfig {
    /// Default: `data<timestamp>.csv`.
    pub file: Option<PathBuf>,
}

/// Sets the configured exporter.
pub fn set_exporter(profiler: &mut JouleProfiler, config: &ExporterConfig) -> Result<()> {
    match config {
        ExporterConfig::Terminal(TerminalConfig {}) => {
            profiler.set_exporter(TerminalExporter::default());
        }

        ExporterConfig::Json(JsonConfig { file, lines }) => {
            let shape = if *lines {
                Shape::Lines
            } else {
                Shape::Document
            };

            let exporter = match file {
                Some(path) => JsonExporter::to_file(path, shape)?,
                None => JsonExporter::new(shape)?,
            };
            profiler.set_exporter(exporter);
        }

        ExporterConfig::Csv(CsvConfig { file }) => {
            let exporter = match file {
                Some(path) => CsvExporter::to_file(path)?,
                None => CsvExporter::new()?,
            };
            profiler.set_exporter(exporter);
        }
    }

    Ok(())
}
