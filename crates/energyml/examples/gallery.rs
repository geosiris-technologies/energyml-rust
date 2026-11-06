//! The gallery images that no command writes: a well sheet, iso-lines, a
//! constrained triangulation and a kriged surface, drawn from the reference
//! model Lutetia.
//!
//! The three section views are `energyml profile` (see `docs/gallery.md`);
//! this program draws the others with the libraries themselves, so the page
//! says what each call is:
//!
//! - `well-sheet.svg`: `energyml_sheet` on the wellbore frame that carries the
//!   most columns;
//! - `isocontours.svg`: `energyml_geostat::isocontour` on a depth surface;
//! - `triangulation.svg`: `energyml_geostat::triangulate` on the borehole
//!   collars, with the fault sticks as constraints;
//! - `kriging.svg`: `energyml_geostat::kriging` on the same collars, the estimate
//!   and its variance side by side.
//!
//! ```sh
//! cargo run -p energyml --example gallery --features all,hdf5 -- \
//!     path/to/package/Lutetia.epc --out docs/images
//! ```
//!
//! `--check` writes nothing and fails when a file under `--out` differs from
//! what is drawn now. The picture is a pure function of the package: no clock,
//! no random number, every number written with a fixed width.

use std::fmt::Write as _;
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;

use energyml::geometry::Topology;
use energyml::geometry::scene::{self, Request, SceneObject};
use energyml::geostat::isocontour;
use energyml::geostat::kriging::Ordinary;
use energyml::geostat::triangulate;
use energyml::geostat::variogram::{self, Model, ModelKind};
use energyml::sheet::layout::{self, HeightGoal, Params};
use energyml::sheet::read::{self, Options};
use energyml::sheet::svg;
use energyml::storage::EpcStore;

type Failure = Box<dyn std::error::Error>;

fn main() -> ExitCode {
    match run() {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::FAILURE,
        Err(error) => {
            eprintln!("gallery: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<bool, Failure> {
    let mut package: Option<PathBuf> = None;
    let mut out = PathBuf::from("docs/images");
    let mut check = false;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--out" => out = PathBuf::from(args.next().ok_or("--out needs a folder")?),
            "--check" => check = true,
            other if other.starts_with("--") => return Err(format!("unknown option {other}").into()),
            other => package = Some(PathBuf::from(other)),
        }
    }
    let package = package.ok_or("usage: gallery <Lutetia.epc> [--out <folder>] [--check]")?;
    let store = EpcStore::open(&package, Arc::new(energyml::schemas::all()))?;

    let images = [
        ("well-sheet.svg", well_sheet(&store)?),
        ("isocontours.svg", isocontours(&store)?),
        ("triangulation.svg", triangulation(&store)?),
        ("kriging.svg", kriging(&store)?),
    ];

    let mut same = true;
    for (name, drawing) in &images {
        let path = out.join(name);
        if check {
            let held = std::fs::read_to_string(&path).unwrap_or_default();
            if held.replace("\r\n", "\n") != *drawing {
                eprintln!("gallery: {} is not what the code draws now", path.display());
                same = false;
            }
        } else {
            std::fs::create_dir_all(&out)?;
            std::fs::write(&path, drawing)?;
            println!("wrote {}", path.display());
        }
    }
    Ok(same)
}

// ---------------------------------------------------------------------------
// The package
// ---------------------------------------------------------------------------

/// The scene objects of one qualified-type filter, in the package's order.
fn objects_of(store: &EpcStore, of_type: &str) -> Vec<SceneObject> {
    let resources = store.resources();
    let request = Request {
        of_type: Some(of_type),
        ..Request::default()
    };
    scene::of_package(&resources, store, &request).objects
}

/// The first object of a type whose title is exactly `title`.
fn object_titled(store: &EpcStore, of_type: &str, title: &str) -> Result<SceneObject, Failure> {
    objects_of(store, of_type)
        .into_iter()
        .find(|object| object.title == title)
        .ok_or_else(|| format!("the package holds no {of_type} titled \"{title}\"").into())
}

fn xyz(object: &SceneObject) -> Vec<[f64; 3]> {
    object
        .geometry
        .patches
        .iter()
        .flat_map(|patch| patch.points.iter().copied())
        .collect()
}

/// The triples `x, y, z` that the triangulation takes (it reads x and y, and carries z).
fn flat_xyz(points: &[[f64; 3]]) -> Vec<f64> {
    points.iter().flat_map(|p| [p[0], p[1], p[2]]).collect()
}

// ---------------------------------------------------------------------------
// A small SVG canvas
// ---------------------------------------------------------------------------

/// Data coordinates to a pixel box, with y going up.
struct Canvas {
    min: [f64; 2],
    max: [f64; 2],
    left: f64,
    top: f64,
    width: f64,
    height: f64,
    body: String,
}

const INK: &str = "#263238";
const GRID: &str = "#cfd8dc";

impl Canvas {
    /// A plot box of `width` x `height` pixels at (`left`, `top`) that shows `min..max`
    /// in both axes at the same scale: the box is shrunk on the longer side.
    fn square_scale(min: [f64; 2], max: [f64; 2], left: f64, top: f64, width: f64, height: f64) -> Canvas {
        let span = [max[0] - min[0], max[1] - min[1]];
        let scale = (width / span[0]).min(height / span[1]);
        let (w, h) = (span[0] * scale, span[1] * scale);
        Canvas {
            min,
            max,
            left,
            top,
            width: w,
            height: h,
            body: String::new(),
        }
    }

    fn x(&self, x: f64) -> f64 {
        self.left + (x - self.min[0]) / (self.max[0] - self.min[0]) * self.width
    }

    fn y(&self, y: f64) -> f64 {
        self.top + self.height - (y - self.min[1]) / (self.max[1] - self.min[1]) * self.height
    }

    fn frame(&mut self) {
        let _ = writeln!(
            self.body,
            r#"<rect x="{:.1}" y="{:.1}" width="{:.1}" height="{:.1}" fill="none" stroke="{INK}" stroke-width="1"/>"#,
            self.left, self.top, self.width, self.height
        );
    }

    fn polyline(&mut self, points: &[[f64; 3]], colour: &str, width: f64) {
        if points.len() < 2 {
            return;
        }
        let mut d = String::new();
        for (i, p) in points.iter().enumerate() {
            let _ = write!(
                d,
                "{}{:.1},{:.1}",
                if i == 0 { "M" } else { " L" },
                self.x(p[0]),
                self.y(p[1])
            );
        }
        let _ = writeln!(
            self.body,
            r#"<path d="{d}" fill="none" stroke="{colour}" stroke-width="{width}" stroke-linejoin="round" stroke-linecap="round"/>"#
        );
    }

    fn segment(&mut self, a: [f64; 2], b: [f64; 2], colour: &str, width: f64) {
        let _ = writeln!(
            self.body,
            r#"<line x1="{:.1}" y1="{:.1}" x2="{:.1}" y2="{:.1}" stroke="{colour}" stroke-width="{width}"/>"#,
            self.x(a[0]),
            self.y(a[1]),
            self.x(b[0]),
            self.y(b[1])
        );
    }

    fn dot(&mut self, at: [f64; 2], radius: f64, fill: &str) {
        let _ = writeln!(
            self.body,
            r##"<circle cx="{:.1}" cy="{:.1}" r="{radius}" fill="{fill}" stroke="#ffffff" stroke-width="1"/>"##,
            self.x(at[0]),
            self.y(at[1])
        );
    }

    fn cell(&mut self, centre: [f64; 2], size: [f64; 2], fill: &str) {
        let _ = writeln!(
            self.body,
            r#"<rect x="{:.1}" y="{:.1}" width="{:.1}" height="{:.1}" fill="{fill}"/>"#,
            self.x(centre[0] - size[0] / 2.0),
            self.y(centre[1] + size[1] / 2.0),
            size[0] / (self.max[0] - self.min[0]) * self.width + 0.6,
            size[1] / (self.max[1] - self.min[1]) * self.height + 0.6
        );
    }

    fn text(&mut self, x: f64, y: f64, size: f64, anchor: &str, content: &str) {
        let _ = writeln!(
            self.body,
            r#"<text x="{x:.1}" y="{y:.1}" font-size="{size}" text-anchor="{anchor}" fill="{INK}">{}</text>"#,
            escape(content)
        );
    }

    /// Ticks and labels along the bottom and left edges, every `step` data units.
    fn axes(&mut self, step: f64) {
        let mut tick = self.min[0];
        while tick <= self.max[0] {
            let x = self.x(tick);
            let _ = writeln!(
                self.body,
                r#"<line x1="{x:.1}" y1="{:.1}" x2="{x:.1}" y2="{:.1}" stroke="{GRID}" stroke-width="0.5"/>"#,
                self.top,
                self.top + self.height
            );
            self.text(
                x,
                self.top + self.height + 14.0,
                10.0,
                "middle",
                &format!("{:.0}", tick - self.min[0]),
            );
            tick += step;
        }
        let mut tick = self.min[1];
        while tick <= self.max[1] {
            let y = self.y(tick);
            let _ = writeln!(
                self.body,
                r#"<line x1="{:.1}" y1="{y:.1}" x2="{:.1}" y2="{y:.1}" stroke="{GRID}" stroke-width="0.5"/>"#,
                self.left,
                self.left + self.width
            );
            self.text(
                self.left - 6.0,
                y + 3.5,
                10.0,
                "end",
                &format!("{:.0}", tick - self.min[1]),
            );
            tick += step;
        }
    }
}

fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// The whole document: a title, the canvases' bodies and a footer line.
fn document(width: f64, height: f64, title: &str, bodies: &[&str], footer: &str) -> String {
    let mut s = String::new();
    let _ = writeln!(
        s,
        r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {width} {height}" width="{width}" height="{height}" font-family="Liberation Sans, Arial, sans-serif">"#
    );
    let _ = writeln!(s, r##"<rect width="{width}" height="{height}" fill="#ffffff"/>"##);
    let _ = writeln!(
        s,
        r#"<text x="16" y="24" font-size="15" font-weight="bold" fill="{INK}">{}</text>"#,
        escape(title)
    );
    for body in bodies {
        s.push_str(body);
    }
    let _ = writeln!(
        s,
        r##"<text x="16" y="{:.0}" font-size="10" fill="#546e7a">{}</text>"##,
        height - 10.0,
        escape(footer)
    );
    s.push_str("</svg>\n");
    s
}

/// A colour on a blue-to-orange ramp, `t` in 0..=1. Fixed stops, no library colour table.
fn ramp(t: f64) -> String {
    let stops: [[f64; 3]; 4] = [
        [33.0, 102.0, 172.0],
        [103.0, 169.0, 207.0],
        [239.0, 138.0, 98.0],
        [178.0, 24.0, 43.0],
    ];
    let t = t.clamp(0.0, 1.0) * 3.0;
    let i = (t.floor() as usize).min(2);
    let f = t - i as f64;
    let mix = |c: usize| (stops[i][c] + (stops[i + 1][c] - stops[i][c]) * f).round() as u8;
    format!("#{:02x}{:02x}{:02x}", mix(0), mix(1), mix(2))
}

// ---------------------------------------------------------------------------
// The well sheet
// ---------------------------------------------------------------------------

/// How many intervals of a board carry a colour: what makes a sheet worth drawing.
fn coloured(board: &layout::Board) -> usize {
    board
        .tracks
        .iter()
        .map(|track| match track {
            layout::Track::Intervals(column) => {
                column.intervals.iter().filter(|i| i.colour.is_some()).count()
            }
            _ => 0,
        })
        .sum()
}

/// The sheet of the wellbore frame whose columns colour the most intervals
/// (ties: the first in the package's order, so the choice is the same every
/// run). Kept: the depth column, every coloured column, and the first column
/// of text, which is what a reader needs to read the colours.
fn well_sheet(store: &EpcStore) -> Result<String, Failure> {
    let resources = store.resources();
    let options = Options::new("MD (m)");
    let mut best: Option<(usize, String, read::Read)> = None;
    for summary in resources
        .iter()
        .filter(|s| s.qualified_type.ends_with("WellboreFrameRepresentation"))
    {
        let object = store.get_object(&summary.uri)?;
        let Ok(read) = read::board_of_frame(&*object, &resources, store, &options) else {
            continue;
        };
        let score = coloured(&read.board);
        if best.as_ref().is_none_or(|(held, _, _)| score > *held) {
            best = Some((score, summary.uri.clone(), read));
        }
    }
    let (score, uri, mut read) = best.ok_or("no wellbore frame of the package can be drawn as a sheet")?;
    eprintln!(
        "well sheet: {uri} ({score} coloured intervals, {} columns)",
        read.board.tracks.len()
    );
    for note in &read.notes {
        eprintln!("  note: {note}");
    }
    let mut text_seen = false;
    read.board.tracks.retain(|track| match track {
        layout::Track::Intervals(column) => {
            let has_colour = column.intervals.iter().any(|i| i.colour.is_some());
            let has_text = column.intervals.iter().any(|i| i.text.is_some());
            if has_colour {
                true
            } else if has_text && !text_seen {
                text_seen = true;
                true
            } else {
                false
            }
        }
        _ => true,
    });
    let params = Params {
        proportion: "readable".to_string(),
        height: HeightGoal::Scale(4.0),
        page_width_mm: 140.0,
        text_size_pt: 8.0,
    };
    let laid = layout::compute(&read.board, &params)?;
    Ok(svg::write(&laid))
}

// ---------------------------------------------------------------------------
// Iso-lines
// ---------------------------------------------------------------------------

fn isocontours(store: &EpcStore) -> Result<String, Failure> {
    let surface = object_titled(store, "Grid2dRepresentation", "H6 - top of the Chalk, in depth")?;
    let points = xyz(&surface);
    let triangles = triangulate::triangulate_xy(&flat_xyz(&points))?;
    let flat = flat_xyz(&points);
    let values: Vec<f64> = points.iter().map(|p| p[2]).collect();
    let (low, high) = isocontour::range(&values).ok_or("the surface holds no depth")?;
    let levels = isocontour::levels_spaced(low.ceil(), high, 1.0)?;
    let drawn = isocontour::contours(&flat, &triangles, &values, &levels)?;

    let (min, max) = bounds(&points);
    let mut canvas = Canvas::square_scale(min, max, 60.0, 50.0, 700.0, 520.0);
    canvas.frame();
    canvas.axes(100.0);
    for contour in &drawn {
        let strength = (contour.level - low) / (high - low).max(f64::EPSILON);
        let colour = ramp(strength);
        let major = (contour.level / 5.0 - (contour.level / 5.0).round()).abs() < 1e-9;
        for line in isocontour::chain(&contour.segments) {
            let mut points = line.points.clone();
            // A closed line does not repeat its first point: close it here.
            if line.closed
                && let Some(first) = points.first().copied()
            {
                points.push(first);
            }
            canvas.polyline(&points, &colour, if major { 2.0 } else { 0.9 });
        }
    }
    let footer = format!(
        "{}: {} nodes, iso-lines every 1 m, heavier every 5 m, over {:.1} to {:.1} m. Axes in metres from the south-west corner.",
        surface.title,
        points.len(),
        low,
        high
    );
    Ok(document(
        820.0,
        640.0,
        "Iso-lines of a depth surface",
        &[&canvas.body],
        &footer,
    ))
}

fn bounds(points: &[[f64; 3]]) -> ([f64; 2], [f64; 2]) {
    let mut min = [f64::MAX; 2];
    let mut max = [f64::MIN; 2];
    for p in points {
        for axis in 0..2 {
            min[axis] = min[axis].min(p[axis]);
            max[axis] = max[axis].max(p[axis]);
        }
    }
    (min, max)
}

// ---------------------------------------------------------------------------
// Triangulation
// ---------------------------------------------------------------------------

const COLLARS: &str = "Lutetia - DTM control points, 14 BSS collars";

/// One trace per fault: the highest point of each stick, in the order the
/// file lists the sticks. A stick is a short polyline across the fault; the
/// line through their tops is what a map shows as the fault's trace.
fn fault_traces(store: &EpcStore) -> Vec<(String, Vec<[f64; 3]>)> {
    let mut traces = Vec::new();
    for object in objects_of(store, "PolylineSetRepresentation") {
        let Some(name) = object.title.strip_suffix(" - sticks") else {
            continue;
        };
        let mut trace = Vec::new();
        for patch in &object.geometry.patches {
            let Topology::Polylines(runs) = &patch.topology else {
                continue;
            };
            let mut start = 0usize;
            for run in runs {
                let end = start + *run as usize;
                if let Some(top) = patch.points[start..end]
                    .iter()
                    .copied()
                    .max_by(|a, b| a[2].total_cmp(&b[2]))
                {
                    trace.push(top);
                }
                start = end;
            }
        }
        traces.push((name.to_string(), trace));
    }
    traces
}

fn triangulation(store: &EpcStore) -> Result<String, Failure> {
    let collars = xyz(&object_titled(store, "PointSetRepresentation", COLLARS)?);
    let traces = fault_traces(store);
    let lines: Vec<Vec<[f64; 3]>> = traces.iter().map(|(_, line)| line.clone()).collect();
    let mut all = collars.clone();
    for line in &lines {
        all.extend(line.iter().copied());
    }
    let result = triangulate::triangulate_xy_constrained(&flat_xyz(&collars), &lines)?;

    let (min, max) = bounds(&all);
    let margin = 60.0;
    let (min, max) = (
        [min[0] - margin, min[1] - margin],
        [max[0] + margin, max[1] + margin],
    );
    let mut canvas = Canvas::square_scale(min, max, 60.0, 50.0, 700.0, 520.0);
    canvas.frame();
    canvas.axes(100.0);
    let nodes = &result.points;
    for triangle in result.triangles.chunks(3) {
        let corner = |i: usize| nodes[triangle[i] as usize];
        for (a, b) in [(0, 1), (1, 2), (2, 0)] {
            canvas.segment(
                [corner(a)[0], corner(a)[1]],
                [corner(b)[0], corner(b)[1]],
                "#90a4ae",
                0.8,
            );
        }
    }
    for line in &lines {
        canvas.polyline(line, "#c62828", 2.4);
    }
    for collar in &collars {
        canvas.dot([collar[0], collar[1]], 4.0, INK);
    }
    let footer = format!(
        "{} boreholes (dots), {} fault traces (red, constraints: {}), {} triangles. Axes in metres from the south-west corner.",
        collars.len(),
        lines.len(),
        traces
            .iter()
            .map(|(name, _)| name.as_str())
            .collect::<Vec<_>>()
            .join(", "),
        result.triangles.len() / 3
    );
    Ok(document(
        820.0,
        640.0,
        "Constrained Delaunay triangulation of the borehole collars",
        &[&canvas.body],
        &footer,
    ))
}

// ---------------------------------------------------------------------------
// Kriging
// ---------------------------------------------------------------------------

fn kriging(store: &EpcStore) -> Result<String, Failure> {
    let collars = xyz(&object_titled(store, "PointSetRepresentation", COLLARS)?);
    let samples: Vec<[f64; 2]> = collars.iter().map(|p| [p[0], p[1]]).collect();
    let values: Vec<f64> = collars.iter().map(|p| p[2]).collect();

    let observed = variogram::empirical(&samples, &values, None, 6)?;
    let model: Model = Model::fit(&observed, ModelKind::Spherical)?;

    let (min, max) = bounds(&collars);
    let margin = 60.0;
    let (min, max) = (
        [min[0] - margin, min[1] - margin],
        [max[0] + margin, max[1] + margin],
    );
    let step = 20.0;
    let columns = ((max[0] - min[0]) / step).ceil() as usize;
    let rows = ((max[1] - min[1]) / step).ceil() as usize;
    let mut queries = Vec::with_capacity(columns * rows);
    for row in 0..rows {
        for column in 0..columns {
            queries.push([
                min[0] + (column as f64 + 0.5) * step,
                min[1] + (row as f64 + 0.5) * step,
            ]);
        }
    }
    let answer = Ordinary::new(model.clone()).interpolate(&samples, &values, &queries)?;

    let range = |v: &[f64]| {
        v.iter()
            .copied()
            .filter(|x| x.is_finite())
            .fold((f64::MAX, f64::MIN), |(lo, hi), x| (lo.min(x), hi.max(x)))
    };
    let (vlo, vhi) = range(&answer.values);
    let (slo, shi) = range(&answer.variances);

    let mut bodies = Vec::new();
    for (side, (title, data, lo, hi)) in [
        ("estimate (m)", &answer.values, vlo, vhi),
        ("kriging variance (m2)", &answer.variances, slo, shi),
    ]
    .into_iter()
    .enumerate()
    {
        let mut canvas = Canvas::square_scale(min, max, 50.0 + side as f64 * 420.0, 60.0, 360.0, 400.0);
        for (query, value) in queries.iter().zip(data.iter()) {
            if value.is_finite() {
                canvas.cell(
                    *query,
                    [step, step],
                    &ramp((value - lo) / (hi - lo).max(f64::EPSILON)),
                );
            }
        }
        canvas.frame();
        for sample in &samples {
            canvas.dot(*sample, 3.5, INK);
        }
        let x = canvas.left;
        canvas.text(x, 52.0, 12.0, "start", &format!("{title}: {lo:.2} to {hi:.2}"));
        bodies.push(canvas.body);
    }
    let refs: Vec<&str> = bodies.iter().map(String::as_str).collect();
    let footer = format!(
        "Ordinary kriging of the elevation of {} collars on a {step} m grid. Spherical model fitted to {} classes: nugget {:.3}, sill {:.3}, range {:.0} m.",
        samples.len(),
        observed.bins.len(),
        model.nugget,
        model.sill,
        model.range
    );
    Ok(document(
        860.0,
        520.0,
        "Kriged surface and its variance",
        &refs,
        &footer,
    ))
}
