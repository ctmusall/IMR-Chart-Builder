//! Command line: `imr-chart-cli FILE [FILE ...] --column COL [--lsl 2e7] ...`.
//!
//! Used by the `imr-chart-cli` binary, and by the `imr-chart` app when it is started with options.

use std::ffi::OsString;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::Parser;
use imr_core::report::{ChartJob, DEFAULT_RECENT_DAYS, FORMATS, Settings, SourceTable};
use imr_core::{Error, Frame, build_sources_job, export_html, export_xlsx, loader, parse_limit, rules};

/// Saves a chart as a PNG image (provided by the charts crate once it exists).
pub type PngSaver = fn(&ChartJob, &Path) -> imr_core::Result<()>;

#[derive(Parser, Debug)]
#[command(
    name = "imr-chart",
    version,
    about = "Generate Individual & Moving Range (I-MR) control charts from Excel or CSV files.",
    allow_negative_numbers = true
)]
struct Args {
    /// Input .xlsx/.xlsm/.xls or .csv file; give several files with the same layout to chart all
    /// their rows together.
    #[arg(value_name = "FILE", required = true)]
    files: Vec<PathBuf>,

    /// Worksheet name, the same in every file (default: 'Summary' if present, else the first data sheet).
    #[arg(long)]
    sheet: Option<String>,

    /// Numeric column to chart; repeat for several charts.
    #[arg(short = 'c', long = "column", value_name = "COLUMN")]
    columns: Vec<String>,

    /// Date column for the x-axis (default: 'Date Processed' if present).
    #[arg(long)]
    date_column: Option<String>,

    /// Plot in row order instead of by date.
    #[arg(long)]
    no_date: bool,

    /// Column used to identify points (default: 'QC Lot' if present).
    #[arg(long)]
    label_column: Option<String>,

    /// Lower spec limit, e.g. 2e7 or 50%; repeat once per --column (or give one for all).
    #[arg(long = "lsl", value_name = "VALUE")]
    lsls: Vec<String>,

    /// Sigma multiplier for control limits.
    #[arg(long, default_value_t = 3.0)]
    sigma: f64,

    /// Highlight the last N days of data (0 disables).
    #[arg(long, value_name = "N", default_value_t = DEFAULT_RECENT_DAYS)]
    recent_days: i64,

    /// JSON file of highlight rules: points whose row matches a rule are drawn in its color (save
    /// one from the app, or see the README).
    #[arg(long, value_name = "FILE")]
    rules: Option<PathBuf>,

    /// Output directory.
    #[arg(short, long, default_value = ".")]
    out: PathBuf,

    /// Comma-separated output formats from png, html, xlsx.
    #[arg(long, default_value = "png,html,xlsx")]
    formats: String,

    /// List sheets and numeric columns, then exit.
    #[arg(long)]
    list_columns: bool,
}

/// Whether the app should open its window: no options, only file paths (e.g. files dropped onto
/// the program).
pub fn wants_gui(args: &[OsString]) -> bool {
    !args.iter().any(|a| a.to_string_lossy().starts_with('-'))
}

/// Run the command line with `args` (the first is the program name). PNG output needs `png`.
pub fn main_with(args: impl IntoIterator<Item = OsString>, png: Option<PngSaver>) -> ExitCode {
    let args = match Args::try_parse_from(args) {
        Ok(a) => a,
        Err(e) => {
            let _ = e.print();
            return ExitCode::from(if e.use_stderr() { 2 } else { 0 });
        }
    };
    match run(&args, png) {
        Ok(code) => ExitCode::from(code),
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::from(1)
        }
    }
}

/// Load one input; a problem with its contents names the file.
fn load(path: &Path, sheet: Option<&str>) -> Result<Frame, Error> {
    loader::load_table(path, sheet).map_err(|e| match e {
        Error::Value(m) => Error::Value(format!("{}: {m}", path.display())),
        other => other,
    })
}

/// `out_dir / name`, written as Python's pathlib writes it (`./x` is `x`).
fn out_path(out_dir: &Path, name: &str) -> PathBuf {
    if out_dir == Path::new(".") { PathBuf::from(name) } else { out_dir.join(name) }
}

fn usage_error(msg: &str) -> Result<u8, Error> {
    eprintln!("error: {msg}");
    Ok(2)
}

fn run(args: &Args, png: Option<PngSaver>) -> Result<u8, Error> {
    let tables: Vec<(PathBuf, Frame)> = args
        .files
        .iter()
        .map(|p| Ok((p.clone(), load(p, args.sheet.as_deref())?)))
        .collect::<Result<_, Error>>()?;
    let frames: Vec<&Frame> = tables.iter().map(|(_, f)| f).collect();
    let mut out = std::io::stdout().lock();

    if args.list_columns {
        let sheets = args.files.iter().map(loader::list_sheets).collect::<Result<Vec<_>, _>>()?;
        let dates: Vec<Vec<String>> = frames.iter().map(|f| loader::date_columns(f)).collect();
        let dates = loader::common_columns(&dates);
        let _ = writeln!(out, "Sheets: {}", loader::common_columns(&sheets).join(", "));
        let _ = writeln!(
            out,
            "Date columns: {}",
            if dates.is_empty() { "(none)".into() } else { dates.join(", ") }
        );
        let _ = writeln!(out, "Numeric columns:");
        for c in loader::pooled_numeric_columns(&frames) {
            let _ = writeln!(out, "  {c}");
        }
        return Ok(0);
    }

    if args.columns.is_empty() {
        return usage_error("at least one --column is required (use --list-columns to see options)");
    }
    let formats: Vec<String> =
        args.formats.split(',').map(|f| f.trim().to_lowercase()).filter(|f| !f.is_empty()).collect();
    let bad: Vec<&str> = formats.iter().map(String::as_str).filter(|f| !FORMATS.contains(f)).collect();
    if !bad.is_empty() {
        return usage_error(&format!("unknown format(s): {}", bad.join(", ")));
    }
    if args.recent_days < 0 {
        return usage_error("--recent-days must be 0 or more");
    }
    let mut lsls = args.lsls.iter().map(|v| parse_limit(v)).collect::<Result<Vec<_>, _>>()?;
    if lsls.len() > 1 && lsls.len() != args.columns.len() {
        return usage_error("give one --lsl for all columns, or one per --column");
    }
    if lsls.len() <= 1 {
        lsls = vec![lsls.first().copied().flatten(); args.columns.len()];
    }
    let highlight_rules = match &args.rules {
        Some(path) => rules::load_rules(path)?,
        None => Vec::new(),
    };
    // Each file's sheet name (a CSV's is its file name), so rules for one sheet skip the others.
    let sheets: Vec<String> = args
        .files
        .iter()
        .map(|f| {
            if loader::is_csv(f) {
                Ok(loader::stem(f))
            } else if let Some(s) = &args.sheet {
                Ok(s.clone())
            } else {
                Ok(loader::list_sheets(f)?.into_iter().next().unwrap_or_default())
            }
        })
        .collect::<Result<_, Error>>()?;
    let date_col = if args.no_date {
        None
    } else {
        args.date_column.clone().or_else(|| loader::pick_date_column(frames[0]))
    };
    let label_col = args.label_column.clone().or_else(|| loader::pick_label_column(frames[0]));
    for (column, lsl) in args.columns.iter().zip(lsls) {
        let sources = tables
            .iter()
            .zip(&sheets)
            .map(|((path, frame), sheet)| SourceTable {
                path: path.clone(),
                frame: frame.clone(),
                column: column.clone(),
                date_column: date_col.clone(),
                sheet: Some(sheet.clone()),
                label_column: label_col.clone(),
            })
            .collect();
        let settings =
            Settings { lsl, k: args.sigma, recent_days: args.recent_days, rules: highlight_rules.clone() };
        let job = build_sources_job(sources, &settings)?;
        let r = &job.result;
        let files =
            if job.sources.len() > 1 { format!("files={} ", job.sources.len()) } else { String::new() };
        let g = |x: f64| imr_core::pyfmt::py_format_g(x, 6);
        let highlighted = if highlight_rules.is_empty() {
            String::new()
        } else {
            format!(" highlighted={}", job.highlighted())
        };
        let _ = writeln!(
            out,
            "{column}: {files}n={} excluded={} mean={} UCL={} LCL={} MR-UCL={}{highlighted}",
            r.n(),
            job.excluded,
            g(r.mean),
            g(r.ucl),
            g(r.lcl),
            g(r.mr_ucl)
        );
        let duplicates = job.duplicates();
        if duplicates > 0 {
            let _ = out.flush();
            eprintln!(
                "  warning: {duplicates} points have the same lot, date and value in more than one file \
                 (overlapping exports?); all are charted"
            );
        }
        for f in &formats {
            let path = out_path(&args.out, &format!("{}.{f}", job.default_stem()));
            match f.as_str() {
                "png" => match png {
                    Some(save) => save(&job, &path)?,
                    None => {
                        return Err(Error::msg(
                            "PNG output is not available in this build yet; use --formats html,xlsx",
                        ));
                    }
                },
                "html" => export_html::save_html(&job, &path)?,
                _ => export_xlsx::save_xlsx(&job, &path)?,
            }
            let _ = writeln!(out, "  wrote {}", path.display());
        }
    }
    Ok(0)
}
