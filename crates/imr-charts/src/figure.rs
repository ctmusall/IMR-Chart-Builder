//! The chart as drawn, independent of how: the PNG writer and the app's interactive view both
//! draw from this. Positions are matplotlib's: figure fractions for the panels, data or axes
//! fractions for marks, points for sizes and offsets.

/// Where an annotation's x or y is measured.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Coord {
    /// A data value.
    Data(f64),
    /// A fraction of the panel (0 = left / bottom, 1 = right / top).
    Axes(f64),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HAlign {
    Left,
    Center,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VAlign {
    Top,
    Center,
    Bottom,
}

/// Text placed at a point, shifted by an offset in points.
#[derive(Debug, Clone, PartialEq)]
pub struct Annotation {
    pub text: String,
    pub x: Coord,
    pub y: Coord,
    /// Offset in points (right, up).
    pub offset: (f64, f64),
    pub ha: HAlign,
    pub va: VAlign,
    pub color: String,
    /// Font size in points.
    pub size: f64,
    pub bold: bool,
    /// Drawn on a rounded, mostly opaque white box (the LSL label).
    pub boxed: bool,
    pub zorder: f64,
}

/// A polyline through the points; NaN breaks it.
#[derive(Debug, Clone, PartialEq)]
pub struct Line {
    pub x: Vec<f64>,
    pub y: Vec<f64>,
    pub color: String,
    /// Width in points.
    pub width: f64,
    pub zorder: f64,
}

/// A horizontal line across the panel.
#[derive(Debug, Clone, PartialEq)]
pub struct HLine {
    pub y: f64,
    pub color: String,
    pub width: f64,
    pub zorder: f64,
}

/// Round markers; NaN points are not drawn.
#[derive(Debug, Clone, PartialEq)]
pub struct Scatter {
    pub x: Vec<f64>,
    pub y: Vec<f64>,
    /// Fill color; None for rings.
    pub face: Option<String>,
    /// Outline color (the fill color when the outline width is 0).
    pub edge: String,
    /// Marker area in points².
    pub size: f64,
    /// Outline width in points.
    pub width: f64,
    pub zorder: f64,
    /// The legend entry, when the points are in the legend.
    pub label: Option<String>,
}

/// A shaded band from `x0` to `x1`, the full height of the panel.
#[derive(Debug, Clone, PartialEq)]
pub struct Band {
    pub x0: f64,
    pub x1: f64,
    pub color: String,
    pub zorder: f64,
}

/// The legend: entries (label, marker color), and where it sits as fractions of the panel
/// (left, bottom, right, top).
#[derive(Debug, Clone, PartialEq)]
pub struct Legend {
    pub entries: Vec<(String, String)>,
    pub bbox: [f64; 4],
    /// Font size in points.
    pub size: f64,
}

/// One plot area with its axes.
#[derive(Debug, Clone, PartialEq)]
pub struct Panel {
    /// (left, bottom, width, height) as fractions of the figure.
    pub position: [f64; 4],
    pub xlim: (f64, f64),
    pub ylim: (f64, f64),
    pub xticks: Vec<(f64, String)>,
    pub yticks: Vec<(f64, String)>,
    pub xlabel: String,
    pub ylabel: String,
    /// Tick labels on the x axis (the upper panel shares the lower one's axis and hides them).
    pub show_xtick_labels: bool,
    /// Rotation of the x tick labels in degrees (90: reading upward).
    pub xrotation: f64,
    /// The x axis is dates (matplotlib date numbers).
    pub dates: bool,
    pub lines: Vec<Line>,
    pub hlines: Vec<HLine>,
    pub scatters: Vec<Scatter>,
    pub bands: Vec<Band>,
    pub texts: Vec<Annotation>,
    pub legend: Option<Legend>,
}

/// Text placed on the figure (title and note).
#[derive(Debug, Clone, PartialEq)]
pub struct FigureText {
    pub text: String,
    /// Position as fractions of the figure.
    pub x: f64,
    pub y: f64,
    pub ha: HAlign,
    pub va: VAlign,
    pub size: f64,
    pub bold: bool,
    pub color: String,
}

/// The whole chart.
#[derive(Debug, Clone, PartialEq)]
pub struct Figure {
    /// Size in inches.
    pub size: (f64, f64),
    pub title: FigureText,
    pub note: FigureText,
    pub panels: Vec<Panel>,
}
