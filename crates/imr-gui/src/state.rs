//! Everything the window shows and does, without the window: the open files and their choices,
//! the chart settings, the highlight rules table, the current chart, and messages. The window
//! (`app.rs`) only draws this and calls its methods, so it can be tested without a screen.

use std::path::{Path, PathBuf};

use imr_charts::Figure;
use imr_core::pyfmt::py_format_g;
use imr_core::report::{DEFAULT_RECENT_DAYS, Settings, SourceTable};
use imr_core::rules::{DEFAULT_COLOR, Rule};
use imr_core::{
    ChartJob, Error, Frame, build_sources_job, export_html, export_xlsx, loader, parse_limit, rules,
};

pub const APP: &str = "I-MR Chart Builder";
pub const NO_DATE: &str = "(row order)";
pub const ANY_SHEET: &str = "(any sheet)";

/// One input file's section: which sheet, value column and date column to chart from it.
#[derive(Debug, Clone)]
pub struct Source {
    pub path: PathBuf,
    pub sheets: Vec<String>,
    pub sheet: String,
    pub frame: Option<Frame>,
    /// Value columns offered (numeric ones, plus any copied over from the first file).
    pub columns: Vec<String>,
    /// Date columns offered, `(row order)` last.
    pub dates: Vec<String>,
    pub column: String,
    pub date: String,
}

impl Source {
    fn open(path: &Path) -> Result<Source, Error> {
        let sheets = loader::list_sheets(path)?;
        let Some(first) = sheets.first().cloned() else {
            return Err(Error::msg("This workbook has no data sheets."));
        };
        let mut s = Source {
            path: path.to_path_buf(),
            sheets,
            sheet: first.clone(),
            frame: None,
            columns: Vec::new(),
            dates: Vec::new(),
            column: String::new(),
            date: String::new(),
        };
        s.load_sheet(&first)?;
        Ok(s)
    }

    pub fn name(&self) -> String {
        self.path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
    }

    pub fn is_csv(&self) -> bool {
        loader::is_csv(&self.path)
    }

    /// Read `sheet` and offer its columns, keeping the choices that are still there.
    pub fn load_sheet(&mut self, sheet: &str) -> Result<(), Error> {
        self.frame = None;
        self.sheet = sheet.to_string();
        let frame = loader::load_table(&self.path, if self.is_csv() { None } else { Some(sheet) })?;
        let (prev_col, prev_date) = (self.column.clone(), self.date.clone());
        self.columns = loader::numeric_columns(&frame, 0.8);
        self.dates = loader::date_columns(&frame);
        self.dates.push(NO_DATE.into());
        self.column = if self.columns.contains(&prev_col) {
            prev_col
        } else {
            self.columns.first().cloned().unwrap_or_default()
        };
        self.date = if self.dates.contains(&prev_date) { prev_date } else { self.dates[0].clone() };
        self.frame = Some(frame);
        Ok(())
    }

    /// Choose `name` if this sheet has it, adding it to the list when it wasn't offered (e.g. a
    /// column that is numeric in another file but mostly junk in this one).
    fn select(&mut self, name: &str, date: bool) -> bool {
        let Some(frame) = &self.frame else { return false };
        if name.is_empty() {
            return false;
        }
        if date && name == NO_DATE {
            self.date = NO_DATE.into();
            return true;
        }
        let list = if date { &mut self.dates } else { &mut self.columns };
        if !list.iter().any(|c| c == name) {
            if frame.column(name).is_none() {
                return false;
            }
            let at = if date { list.len() - 1 } else { list.len() }; // dates: keep (row order) last
            list.insert(at, name.to_string());
        }
        if date {
            self.date = name.into()
        } else {
            self.column = name.into()
        }
        true
    }

    pub fn date_column(&self) -> Option<String> {
        (!self.date.is_empty() && self.date != NO_DATE).then(|| self.date.clone())
    }
}

/// One row of the highlight rules table, as typed (checked into a [`Rule`] when charting).
#[derive(Debug, Clone, PartialEq)]
pub struct RuleRow {
    pub enabled: bool,
    pub name: String,
    /// "" or `(any sheet)`: rows from any sheet.
    pub sheet: String,
    pub column: String,
    pub op: String,
    pub value: String,
    pub color: String,
}

impl Default for RuleRow {
    fn default() -> RuleRow {
        RuleRow {
            enabled: true,
            name: String::new(),
            sheet: ANY_SHEET.into(),
            column: String::new(),
            op: "=".into(),
            value: String::new(),
            color: DEFAULT_COLOR.into(),
        }
    }
}

impl From<&Rule> for RuleRow {
    fn from(r: &Rule) -> RuleRow {
        RuleRow {
            enabled: r.enabled,
            name: r.name.clone(),
            sheet: if r.sheet.is_empty() { ANY_SHEET.into() } else { r.sheet.clone() },
            column: r.column.clone(),
            op: r.op.clone(),
            value: r.value.clone(),
            color: r.color.clone(),
        }
    }
}

impl RuleRow {
    pub fn rule(&self) -> Result<Rule, Error> {
        let sheet = self.sheet.trim();
        Rule::new(
            self.name.trim(),
            self.column.trim(),
            self.op.as_str(),
            self.value.as_str(),
            &self.color,
            self.enabled,
            if sheet == ANY_SHEET { "" } else { sheet },
        )
    }
}

/// A saved output format.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Png,
    Html,
    Xlsx,
}

impl Format {
    pub fn extension(self) -> &'static str {
        match self {
            Format::Png => "png",
            Format::Html => "html",
            Format::Xlsx => "xlsx",
        }
    }

    pub fn filter_name(self) -> &'static str {
        match self {
            Format::Png => "PNG image",
            Format::Html => "Interactive HTML",
            Format::Xlsx => "Excel workbook",
        }
    }
}

#[derive(Debug, Clone)]
pub struct AppState {
    pub sources: Vec<Source>,
    /// The LSL as typed (`2e7`, `50%`; blank for none).
    pub lsl: String,
    pub sigma: f64,
    /// 0 turns the recent-days highlight off.
    pub recent_days: i64,
    pub rules: Vec<RuleRow>,
    pub selected_rule: Option<usize>,
    pub job: Option<ChartJob>,
    pub figure: Option<Figure>,
    /// The chart's numbers in one line.
    pub stats: String,
    /// Problems to show the user, oldest first (the window shows and clears them).
    pub errors: Vec<String>,
    /// The last thing that went right (e.g. where a file was saved).
    pub status: String,
    /// The settings and rules panel is open (it can be folded away to give the chart room).
    pub settings_open: bool,
}

impl Default for AppState {
    fn default() -> AppState {
        AppState {
            sources: Vec::new(),
            lsl: String::new(),
            sigma: 3.0,
            recent_days: DEFAULT_RECENT_DAYS,
            rules: Vec::new(),
            selected_rule: None,
            job: None,
            figure: None,
            stats: String::new(),
            errors: Vec::new(),
            status: String::new(),
            settings_open: true,
        }
    }
}

impl AppState {
    fn error(&mut self, msg: impl Into<String>) {
        self.errors.push(msg.into());
    }

    /// Forget the chart (its settings changed).
    pub fn invalidate(&mut self) {
        self.job = None;
        self.figure = None;
    }

    /// The files line: the path of one file, or how many and the first names.
    pub fn files_label(&self) -> String {
        match self.sources.as_slice() {
            [] => "No file loaded".into(),
            [one] => one.path.display().to_string(),
            many => {
                let names: Vec<String> = many.iter().take(3).map(Source::name).collect();
                format!(
                    "{} files: {}{}",
                    many.len(),
                    names.join(", "),
                    if many.len() > 3 { ", …" } else { "" }
                )
            }
        }
    }

    /// Open one or more files; each gets its own sheet and column choices, and their rows are
    /// charted together. On a problem nothing changes and the error names the file.
    pub fn open_paths(&mut self, paths: &[PathBuf]) -> bool {
        if paths.is_empty() {
            return false;
        }
        let mut sources = Vec::new();
        for path in paths {
            match Source::open(path) {
                Ok(s) => sources.push(s),
                Err(e) => {
                    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                    self.error(if paths.len() > 1 { format!("{name}: {e}") } else { e.to_string() });
                    return false;
                }
            }
        }
        self.sources = sources;
        // Choosing a column in the first workbook picks it in the others too, where they have it.
        let (column, date) = (self.sources[0].column.clone(), self.sources[0].date.clone());
        self.copy_choice(&column, false);
        self.copy_choice(&date, true);
        self.invalidate();
        true
    }

    /// Select value (or date) column `name` in every workbook after the first that has it.
    pub fn copy_choice(&mut self, name: &str, date: bool) {
        for s in self.sources.iter_mut().skip(1) {
            s.select(name, date);
        }
    }

    /// The user chose value column `name` in workbook `i` (the first one's choice is copied).
    pub fn choose_column(&mut self, i: usize, name: &str) {
        if let Some(s) = self.sources.get_mut(i) {
            s.column = name.to_string();
        }
        if i == 0 {
            self.copy_choice(name, false);
        }
        self.invalidate();
    }

    /// The user chose date column `name` (or `(row order)`) in workbook `i`.
    pub fn choose_date(&mut self, i: usize, name: &str) {
        if let Some(s) = self.sources.get_mut(i) {
            s.date = name.to_string();
        }
        if i == 0 {
            self.copy_choice(name, true);
        }
        self.invalidate();
    }

    /// Read `sheet` of workbook `i`.
    pub fn load_sheet(&mut self, i: usize, sheet: &str) -> bool {
        let many = self.sources.len() > 1;
        let Some(s) = self.sources.get_mut(i) else { return false };
        let ok = match s.load_sheet(sheet) {
            Ok(()) => true,
            Err(e) => {
                let msg = if many { format!("{}: {e}", s.name()) } else { e.to_string() };
                self.error(msg);
                false
            }
        };
        self.invalidate();
        ok
    }

    /// Each loaded sheet's columns (sheets of the same name across files merged), for the rules table.
    pub fn columns_by_sheet(&self) -> Vec<(String, Vec<String>)> {
        let mut out: Vec<(String, Vec<String>)> = Vec::new();
        for s in &self.sources {
            let Some(frame) = &s.frame else { continue };
            let entry = match out.iter().position(|(n, _)| *n == s.sheet) {
                Some(i) => &mut out[i].1,
                None => {
                    out.push((s.sheet.clone(), Vec::new()));
                    &mut out.last_mut().expect("just pushed").1
                }
            };
            for c in frame.names() {
                if !entry.contains(&c) {
                    entry.push(c);
                }
            }
        }
        out
    }

    /// The sheets a rule can name: `(any sheet)` and the loaded ones.
    pub fn rule_sheets(&self) -> Vec<String> {
        std::iter::once(ANY_SHEET.to_string())
            .chain(self.columns_by_sheet().into_iter().map(|(s, _)| s))
            .collect()
    }

    /// Columns offered for a rule on `sheet`: that sheet's, or every sheet's for any sheet.
    pub fn columns_for(&self, sheet: &str) -> Vec<String> {
        let by_sheet = self.columns_by_sheet();
        let sheet = sheet.trim();
        if sheet.is_empty() || sheet == ANY_SHEET {
            let mut all: Vec<String> = Vec::new();
            for (_, cols) in by_sheet {
                for c in cols {
                    if !all.contains(&c) {
                        all.push(c);
                    }
                }
            }
            return all;
        }
        by_sheet.into_iter().find(|(s, _)| s == sheet).map(|(_, c)| c).unwrap_or_default()
    }

    // --- the rules table ---------------------------------------------------------

    /// Add a rule row (after the last) and select it.
    pub fn add_rule(&mut self, row: RuleRow) {
        self.rules.push(row);
        self.selected_rule = Some(self.rules.len() - 1);
        self.invalidate();
    }

    pub fn remove_rule(&mut self) {
        if let Some(i) = self.selected_rule.filter(|i| *i < self.rules.len()) {
            self.rules.remove(i);
            self.selected_rule = if self.rules.is_empty() { None } else { Some(i.min(self.rules.len() - 1)) };
            self.invalidate();
        }
    }

    /// Move the selected rule up (-1) or down (+1).
    pub fn move_rule(&mut self, step: i64) {
        let Some(i) = self.selected_rule else { return };
        let target = i as i64 + step;
        if target < 0 || target as usize >= self.rules.len() {
            return;
        }
        self.rules.swap(i, target as usize);
        self.selected_rule = Some(target as usize);
        self.invalidate();
    }

    /// The table's rules, in order; the error names the first incomplete row.
    pub fn checked_rules(&self) -> Result<Vec<Rule>, Error> {
        self.rules
            .iter()
            .enumerate()
            .map(|(i, r)| r.rule().map_err(|e| Error::msg(format!("Highlight rule {}: {e}", i + 1))))
            .collect()
    }

    pub fn set_rules(&mut self, rules: &[Rule]) {
        self.rules = rules.iter().map(RuleRow::from).collect();
        self.selected_rule = None;
        self.invalidate();
    }

    pub fn load_rules(&mut self, path: &Path) -> bool {
        match rules::load_rules(path) {
            Ok(r) => {
                self.set_rules(&r);
                true
            }
            Err(e) => {
                self.error(format!("Could not load rules: {e}"));
                false
            }
        }
    }

    /// Save the rules (adding `.json` when the name has no such extension).
    pub fn save_rules(&mut self, path: &Path) -> Option<PathBuf> {
        let rules = match self.checked_rules() {
            Ok(r) => r,
            Err(e) => {
                self.error(format!("Could not save rules: {e}"));
                return None;
            }
        };
        let path = with_extension(path, "json");
        match rules::save_rules(&rules, &path) {
            Ok(()) => {
                self.status = format!("Saved {}", path.display());
                Some(path)
            }
            Err(e) => {
                self.error(format!("Could not save rules: {e}"));
                None
            }
        }
    }

    // --- the chart ---------------------------------------------------------------

    pub fn can_generate(&self) -> bool {
        !self.sources.is_empty()
    }

    /// Build the chart from the current choices; on a problem, say what.
    pub fn generate(&mut self) -> bool {
        if self.sources.is_empty() {
            return false;
        }
        let many = self.sources.len() > 1;
        for s in &self.sources {
            let place = if many { format!("{}: ", s.name()) } else { String::new() };
            if s.frame.is_none() {
                let msg = format!("{place}Could not read sheet '{}'.", s.sheet);
                self.invalidate();
                self.error(msg);
                return false;
            }
            if s.column.is_empty() {
                self.invalidate();
                self.error(format!("{place}This sheet has no numeric columns to chart."));
                return false;
            }
        }
        let built = parse_limit(&self.lsl).and_then(|lsl| {
            let settings =
                Settings { lsl, k: self.sigma, recent_days: self.recent_days, rules: self.checked_rules()? };
            let sources = self
                .sources
                .iter()
                .map(|s| SourceTable {
                    path: s.path.clone(),
                    frame: s.frame.clone().expect("checked above"),
                    column: s.column.clone(),
                    date_column: s.date_column(),
                    sheet: Some(s.sheet.clone()),
                    label_column: None,
                })
                .collect();
            build_sources_job(sources, &settings)
        });
        match built {
            Ok(job) => {
                self.stats = stats_line(&job);
                self.figure = Some(imr_charts::figure(&job));
                self.job = Some(job);
                true
            }
            Err(e) => {
                self.invalidate();
                self.error(format!("Could not build chart: {e}"));
                false
            }
        }
    }

    /// Where a save dialog starts: the first file's folder, `IMR_<column>.<ext>`.
    pub fn default_save_path(&self, format: Format) -> Option<PathBuf> {
        let job = self.job.as_ref()?;
        let dir =
            self.sources.first().and_then(|s| s.path.parent()).map(Path::to_path_buf).unwrap_or_default();
        Some(dir.join(format!("{}.{}", job.default_stem(), format.extension())))
    }

    /// Save the chart (adding the format's extension when missing).
    pub fn save(&mut self, format: Format, path: &Path) -> Option<PathBuf> {
        let job = self.job.as_ref()?;
        let path = with_extension(path, format.extension());
        let result = match format {
            Format::Png => imr_charts::save_png(job, &path),
            Format::Html => export_html::save_html(job, &path),
            Format::Xlsx => export_xlsx::save_xlsx(job, &path),
        };
        match result {
            Ok(()) => {
                self.status = format!("Saved {}", path.display());
                Some(path)
            }
            Err(e) => {
                self.error(format!("Could not save: {e}"));
                None
            }
        }
    }
}

fn with_extension(path: &Path, ext: &str) -> PathBuf {
    let has = path.extension().is_some_and(|e| e.to_string_lossy().eq_ignore_ascii_case(ext));
    if has { path.to_path_buf() } else { PathBuf::from(format!("{}.{ext}", path.display())) }
}

/// The chart's numbers in one line, as the Python version showed them.
pub fn stats_line(job: &ChartJob) -> String {
    let r = &job.result;
    let g = |x: f64| py_format_g(x, 4);
    let count = |f: &[bool]| f.iter().filter(|b| **b).count();
    let beyond = (0..r.n()).filter(|&i| r.above_ucl[i] || r.below_lcl[i]).count();
    let mut s = String::new();
    if job.sources.len() > 1 {
        s += &format!("files={}  ", job.sources.len());
    }
    s += &format!(
        "n={}  excluded={}  mean={}  σ={}  UCL={}  LCL={}  MR̄={}  MR UCL={}  beyond limits={beyond}",
        r.n(),
        job.excluded,
        g(r.mean),
        g(r.sigma),
        g(r.ucl),
        g(r.lcl),
        g(r.mr_bar),
        g(r.mr_ucl)
    );
    if r.lsl.is_some() {
        s += &format!("  below LSL={}", count(&r.below_lsl));
    }
    if !job.rules.is_empty() {
        s += &format!("  highlighted={}", job.highlighted());
    }
    let dupes = job.duplicates();
    if dupes > 0 {
        s += &format!("  ⚠ {dupes} points repeated across files");
    }
    s
}
