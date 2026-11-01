//! I-MR Chart Builder engine: load QC data from Excel workbooks or CSV files, compute Individual &
//! Moving Range control limits (JMP's defaults), color points by highlight rules, and export the
//! statistics workbook (with its proof sheets) and the interactive HTML chart.

pub mod cell;
pub mod colors;
pub mod dates;
pub mod error;
pub mod export_html;
pub mod export_xlsx;
pub mod loader;
pub mod plot;
pub mod proof;
pub mod pyfmt;
pub mod report;
pub mod rules;
pub mod stats;

pub use cell::{Cell, Column, DType, Frame};
pub use error::{Error, Result};
pub use report::{ChartJob, Settings, SourceTable, build_multi_job, build_sources_job};
pub use rules::Rule;
pub use stats::{ImrResult, compute_imr, parse_limit};

/// The program version, shown in reports and `--version`.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
