//! `energyml profile` - the wells that fall in the band around a drawn track.
//!
//! Argument parsing and display only: the heads, the distances, the sides and
//! the band are `energyml_analysis::wells_profile`'s, so this command and the
//! front end answer the same thing.

use std::path::PathBuf;

use anyhow::{Context, Result, anyhow, bail};
use clap::Parser;
use energyml_analysis::profile::{Reason, Side};
use energyml_analysis::wells_profile;
use energyml_geometry::{Placement, heads};
use serde_json::{Value, json};

use crate::{Modes, open};

/// `energyml profile` arguments.
#[derive(Parser)]
pub struct ProfileArgs {
    /// The EPC file - the section's first package, `s1`.
    file: PathBuf,
    /// With `--svg`: another package, `s2`, `s3`… in the order given. Its wells
    /// and surfaces are cut along the same sections and drawn in the same
    /// drawing, in the first package's system.
    #[arg(long = "also", value_name = "FILE.epc")]
    also: Vec<PathBuf>,
    /// The track: a GeoJSON file (a LineString, a Feature or a collection whose
    /// first line is used) or the vertices inline, `x,y;x,y;…`. In the
    /// placement's unit.
    #[arg(
        long,
        value_name = "FILE.geojson|x,y;x,y",
        allow_hyphen_values = true,
        required_unless_present = "from_svg"
    )]
    track: Option<String>,
    /// Width kept on the left of the track, read in the direction it was drawn.
    #[arg(long, value_name = "METRES", required_unless_present = "from_svg")]
    left: Option<f64>,
    /// Width kept on the right of the track.
    #[arg(long, value_name = "METRES", required_unless_present = "from_svg")]
    right: Option<f64>,
    /// Draw again the section view this SVG holds, from what its metadata says
    /// it was made of, over the packages given (`FILE` is `s1`, `--also` the
    /// next) - into `--svg`, and `--png`. On the same packages, the same bytes.
    #[arg(long, value_name = "FILE.svg", requires = "svg")]
    from_svg: Option<PathBuf>,
    /// The coordinates of the track: `projected` (the CRS as it stands),
    /// `metres` (its horizontal unit taken to metres) or `local`.
    #[arg(long, default_value = "projected")]
    placement: String,
    /// Which wells, by uri or uuid. Left out, every trajectory of the package -
    /// with `--svg`, every trajectory and frame of every package; given, each
    /// goes to the package that holds it.
    #[arg(long = "well", value_name = "URI|UUID")]
    wells: Vec<String>,
    /// The angle, in degrees, a rounded outer turn of the band is cut at.
    #[arg(long, default_value_t = 10.0)]
    arc_step: f64,
    /// Print the answer as JSON, with the band.
    #[arg(long)]
    json: bool,
    /// Draw the section view as SVG into this file - wells and surfaces, the
    /// same drawing the front end shows.
    #[arg(long, value_name = "FILE.svg")]
    svg: Option<PathBuf>,
    /// With `--svg`: a surface to cut, by uri, in whichever package holds it.
    /// Left out, every surface and grid of every package a section cuts
    /// (triangulated, 2D grids, IJK grids).
    #[arg(long = "surface", value_name = "URI")]
    surfaces: Vec<String>,
    /// With `--svg`: a cross section every this many metres along the track.
    #[arg(long, value_name = "METRES", conflicts_with = "cross_count")]
    cross_every: Option<f64>,
    /// With `--svg`: this many cross sections, spread along the track.
    #[arg(long, value_name = "N")]
    cross_count: Option<u32>,
    /// With `--svg`: a cross section's length on each side of the track.
    #[arg(long, value_name = "METRES", default_value_t = 200.0)]
    cross_length: f64,
    /// With `--svg`: a cross section's corridor on each side, for its wells.
    #[arg(long, value_name = "METRES", default_value_t = 50.0)]
    cross_width: f64,
    /// With `--svg`: what to draw - `long`, `plan`, or `cross:N`.
    #[arg(long, default_value = "long")]
    draw: String,
    /// With `--svg`: the drawing's size in pixels, `WIDTHxHEIGHT`.
    #[arg(long, default_value = "1200x600")]
    size: String,
    /// With `--svg`: metres of depth per unit of time, for objects in time.
    #[arg(long, value_name = "FACTOR")]
    time_factor: Option<f64>,
    /// With `--svg`: the same drawing as a PNG too (needs a build with
    /// `--features png`; refused by name otherwise).
    #[arg(long, value_name = "FILE.png", requires = "svg")]
    png: Option<PathBuf>,
    /// With `--png`: how many pixels per pixel of the SVG (2: sharp on a phone).
    #[arg(long, default_value_t = 2.0)]
    png_scale: f32,
    /// With `--svg`: how objects are named - `full` (their titles) or `codes`
    /// (`W0`, `H1`, `F2`… and a legend beside the drawing).
    #[arg(long, default_value = "full")]
    labels: String,
    /// With `--svg`: write a plain, static SVG. By default the file carries a
    /// small script that makes it zoom (wheel), pan (drag) and reset (double
    /// click) when opened alone in a browser, with lines that keep their pixel
    /// width; without the script the drawing is the same. A redraw
    /// (`--from-svg`) keeps what its own metadata says.
    #[arg(long = "static", requires = "svg", conflicts_with = "from_svg")]
    static_svg: bool,
    /// With `--svg`: a legend of the stratigraphic units the drawn wells show,
    /// on the left of the drawing, in a group of its own (`strata-legend`, an
    /// Inkscape layer) that is easy to move. Said in a note, never drawn empty,
    /// when no well shows a unit. A redraw (`--from-svg`) keeps what its own
    /// metadata says.
    #[arg(long, requires = "svg", conflicts_with = "from_svg")]
    strata_legend: bool,
    /// With `--svg`: a short black tick across each well, wherever one unit
    /// gives way to the next (wells coloured by strata), in a group of its own
    /// inside the well's. A redraw (`--from-svg`) keeps what its own metadata
    /// says.
    #[arg(long, requires = "svg", conflicts_with = "from_svg")]
    strata_ticks: bool,
    /// With `--svg`: do not draw the portions of a well that carry no strata
    /// information (wells coloured by strata): a well is drawn only where a
    /// unit is known, and a note counts the wells that lost portions. By
    /// default the whole depth is drawn.
    #[arg(long, requires = "svg", conflicts_with = "from_svg")]
    hide_unstratified: bool,
    /// With `--svg`: how the portions of a well with no strata information are
    /// drawn (wells coloured by strata) - `solid`, `dashed`, `hatched` or
    /// `hidden`, with parameters, e.g. `dashed:colour=#999999,dash=6,gap=4` or
    /// `hatched:spacing=5,angle=45`. A parameter left out takes its default; the
    /// default style is the solid grey a drawing always had. With a style of
    /// its own and `--strata-legend`, the legend says what it is. `--hide-unstratified`
    /// is `hidden`, and wins.
    #[arg(
        long,
        value_name = "STYLE[:PARAM=VALUE,…]",
        requires = "svg",
        conflicts_with = "from_svg"
    )]
    unstratified: Option<String>,
    /// With `--svg`: a named marker on the objects of the section whose
    /// qualified type contains `type` (`A|B`: any) and whose title contains
    /// `title`, case ignored - `NAME:shape=diamond,colour=#1565c0,at=bottom,type=WellboreTrajectory,title=BSS`.
    /// `shape` is a RESQML node symbol or `contour`; `at` is `head`, `bottom` or
    /// `md` with `md=VALUE`; `size` is in pixels. Repeatable; listed in the
    /// legend of markers. A redraw (`--from-svg`) keeps what its own metadata says.
    #[arg(
        long = "marker",
        value_name = "NAME:PARAM=VALUE,…",
        requires = "svg",
        conflicts_with = "from_svg"
    )]
    markers: Vec<String>,
}

/// Runs the command.
pub fn run(args: ProfileArgs, modes: Modes) -> Result<()> {
    if args.svg.is_some() {
        let mut stores = vec![open(&args.file, &modes)?];
        for path in &args.also {
            stores.push(open(path, &modes)?);
        }
        return draw_svg(&stores, &args);
    }
    if !args.also.is_empty() {
        bail!("`--also` draws several packages in one section view: give `--svg`");
    }
    let store = open(&args.file, &modes)?;
    let (Some(track), Some(left), Some(right)) = (&args.track, args.left, args.right) else {
        bail!("give `--track`, `--left` and `--right`");
    };
    let track = read_track(track)?;
    let placement = match args.placement.as_str() {
        "projected" => Placement::Projected,
        "metres" => Placement::Metres,
        "local" => Placement::Local,
        other => bail!("placement `{other}`: one of projected, metres, local"),
    };
    let asked: Vec<String> = if args.wells.is_empty() {
        heads::trajectories_of(&store.resources())
    } else {
        args.wells.clone()
    };
    let answer = wells_profile::of_wells(
        &store,
        &asked,
        &track,
        left,
        right,
        args.arc_step.to_radians(),
        placement,
    )
    .map_err(|e| anyhow!("{e}"))?;

    if args.json {
        println!("{}", serde_json::to_string_pretty(&to_json(&answer))?);
        return Ok(());
    }
    let system = answer
        .system
        .map(|c| format!("EPSG:{c}"))
        .unwrap_or_else(|| "no EPSG code".into());
    println!(
        "track {:.1} long, left {} right {}; {} head(s) read, system {system}",
        answer.length, left, right, answer.heads
    );
    println!("kept {} (d: + left, - right):", answer.profile.kept.len());
    for one in &answer.profile.kept {
        let signed = if one.side == Side::Left { one.d } else { -one.d };
        let side = if one.side == Side::Left { "left" } else { "right" };
        println!(
            "  {:<28} s {:>9.2}  d {:>8.2}  {side}",
            answer.title(&one.id),
            one.s,
            signed
        );
    }
    println!("beyond an end (ignored): {}", answer.profile.beyond_an_end());
    println!(
        "too far on their side: {}",
        answer.profile.count(Reason::TooFarOnItsSide)
    );
    if answer.profile.count(Reason::NoPosition) > 0 {
        println!("no position: {}", answer.profile.count(Reason::NoPosition));
    }
    for (id, why) in &answer.refused {
        println!("refused {id}: {why}");
    }
    Ok(())
}

/// `--svg`: the section view of these packages along the track, drawn - or,
/// with `--from-svg`, a drawing made again from its own metadata.
///
/// Parsing and writing only: what a package gives (its colour sets, its index,
/// its default selection) is `energyml_section::read::package`, the
/// calculation `compute_packages`, the drawing `draw` - and `redraw` both.
fn draw_svg(stores: &[energyml_storage::EpcStore], args: &ProfileArgs) -> Result<()> {
    use energyml_section::Opened;
    use energyml_section::model::{Asked, CrossWidths, Crosses, Inputs, SectionKey, Target, View};
    use energyml_section::read::{frame, package};

    let out = args.svg.as_ref().expect("`draw_svg` is called with `--svg`");
    if args.placement != "projected" {
        bail!("`--svg` draws in the projected system: leave `--placement` at `projected`");
    }
    let listed = stores
        .iter()
        .map(package::Listed::of)
        .collect::<std::result::Result<Vec<_>, String>>()
        .map_err(|e| anyhow!("{e}"))?;
    let mut opened: Vec<Opened<'_>> = stores
        .iter()
        .zip(&listed)
        .enumerate()
        .map(|(at, (store, listed))| Opened {
            store: format!("s{}", at + 1),
            source: store,
            context: listed.context(),
            cache: None,
        })
        .collect();
    let backend = energyml_proj::Proj4::new();

    let (svg, catalogue) = if let Some(from) = &args.from_svg {
        let text = std::fs::read_to_string(from).with_context(|| format!("reading {}", from.display()))?;
        (
            energyml_section::redraw(&text, &mut opened, &backend)
                .map_err(|e| anyhow!("{}: {e}", from.display()))?,
            None,
        )
    } else {
        let (Some(track), Some(left), Some(right)) = (&args.track, args.left, args.right) else {
            bail!("give `--track`, `--left` and `--right`, or `--from-svg`");
        };
        // Each package its selection: what was named goes to the package that
        // holds it; a kind nobody named takes every object of every package.
        let mut objects: Vec<Asked> = Vec::new();
        for (at, store) in stores.iter().enumerate() {
            let (wells, surfaces) = package::selection(store);
            let held = |uri: &String| store.resolve(uri).ok().map(|one| one.uri.clone());
            objects.push(Asked {
                store: format!("s{}", at + 1),
                revision: 0,
                wells: if args.wells.is_empty() {
                    wells
                } else {
                    args.wells.iter().filter_map(held).collect()
                },
                surfaces: if args.surfaces.is_empty() {
                    surfaces
                } else {
                    args.surfaces.iter().filter_map(held).collect()
                },
            });
        }
        for uri in args.wells.iter().chain(&args.surfaces) {
            // The store's gate: a uri names its entry, and a name that fits
            // several is refused with its candidates rather than served as "one".
            let mut refusal = None;
            let mut held = false;
            for store in stores {
                match store.resolve(uri) {
                    Ok(_) => held = true,
                    Err(energyml_storage::Error::NotFound(_)) => {}
                    Err(other) => refusal = Some(other),
                }
            }
            if let Some(refusal) = refusal.filter(|_| !held) {
                bail!("`{uri}`: {refusal}");
            }
            if !held {
                bail!("`{uri}` is in none of the packages given");
            }
        }
        // The scene's system: the first a package's objects state.
        let system = stores.iter().zip(&objects).find_map(|(store, asked)| {
            let all: Vec<String> = asked.wells.iter().chain(&asked.surfaces).cloned().collect();
            frame::first_system(store, &all)
        });
        let widths = CrossWidths {
            before: args.cross_length,
            after: args.cross_length,
            left: args.cross_width,
            right: args.cross_width,
        };
        let crosses = match (args.cross_every, args.cross_count) {
            (Some(spacing), _) => Crosses::Every {
                spacing,
                common: widths,
                own: Vec::new(),
            },
            (None, Some(count)) => Crosses::Count {
                count,
                common: widths,
                own: Vec::new(),
            },
            (None, None) => Crosses::None,
        };
        let inputs = Inputs {
            trace: read_track(track)?,
            system,
            left,
            right,
            crosses,
            objects,
            time_factor: args.time_factor,
        };
        let catalogue =
            energyml_section::compute_packages(&inputs, &mut opened, &backend).map_err(|e| anyhow!("{e}"))?;

        let target = match args.draw.as_str() {
            "long" => Target::Section {
                key: SectionKey::Long,
            },
            "plan" => Target::Plan,
            other => match other.strip_prefix("cross:").and_then(|n| n.parse().ok()) {
                Some(index) => Target::Section {
                    key: SectionKey::Cross { index },
                },
                None => bail!("`--draw {other}`: one of long, plan, cross:N"),
            },
        };
        let (width, height) = args
            .size
            .split_once('x')
            .and_then(|(w, h)| Some((w.trim().parse::<f64>().ok()?, h.trim().parse::<f64>().ok()?)))
            .ok_or_else(|| anyhow!("`--size {}`: expected WIDTHxHEIGHT", args.size))?;
        let mut view = View::new(target, [width, height]);
        view.zoom_script = !args.static_svg;
        view.strata_legend = args.strata_legend;
        view.strata_ticks = args.strata_ticks;
        view.draw_unstratified = !args.hide_unstratified;
        if let Some(text) = &args.unstratified {
            view.unstratified = energyml_section::style::parse(text)
                .map_err(|why| anyhow!("`--unstratified {text}`: {why}"))?;
        }
        view.labels = match args.labels.as_str() {
            "full" => energyml_section::model::LabelMode::Full,
            "codes" => energyml_section::model::LabelMode::Codes,
            other => bail!("`--labels {other}`: one of full, codes"),
        };
        let shown_in = match &view.target {
            Target::Section { key } => Some(*key),
            Target::Plan => None,
        };
        for spec in &args.markers {
            let marker = energyml_section::markers::from_spec(spec, &catalogue.objects, shown_in)
                .map_err(|why| anyhow!("`--marker {spec}`: {why}"))?;
            view.markers.push(marker);
        }
        (energyml_section::draw(&catalogue, &view), Some(catalogue))
    };

    std::fs::write(out, &svg).with_context(|| format!("writing {}", out.display()))?;
    if let Some(png) = &args.png {
        let bytes = energyml_section::png::render(&svg, args.png_scale).map_err(|e| anyhow!("{e}"))?;
        std::fs::write(png, bytes).with_context(|| format!("writing {}", png.display()))?;
        println!("{}", png.display());
    }
    let Some(catalogue) = catalogue else {
        println!("{}: drawn again from its metadata", out.display());
        return Ok(());
    };
    let drawn = catalogue
        .objects
        .iter()
        .filter(|row| matches!(row.outcome, energyml_section::model::Outcome::Drawn { .. }))
        .count();
    println!(
        "{}: {} of {} object(s) drawn, {} cross section(s), {} note(s)",
        out.display(),
        drawn,
        catalogue.objects.len(),
        catalogue.crosses.len(),
        catalogue.notes.len()
    );
    for row in &catalogue.objects {
        if let energyml_section::model::Outcome::Refused { reason } = &row.outcome {
            // The reason names the object itself (`assemble` writes it so).
            println!("  refused: {reason}");
        }
    }
    for note in &catalogue.notes {
        println!("  note: {}", note.said);
    }
    Ok(())
}

fn to_json(answer: &wells_profile::WellsProfile) -> Value {
    let reason = |r: Reason| match r {
        Reason::TooFarOnItsSide => "too-far-on-its-side",
        Reason::BeyondAnEnd => "beyond-an-end",
        Reason::NoPosition => "no-position",
    };
    json!({
        "kept": answer.profile.kept.iter().map(|one| json!({
            "id": one.id, "title": answer.title(&one.id), "s": one.s, "d": one.d,
            "side": if one.side == Side::Left { "left" } else { "right" }, "foot": one.foot,
        })).collect::<Vec<_>>(),
        "rejected": answer.profile.rejected.iter().map(|one| json!({
            "id": one.id, "title": answer.title(&one.id), "reason": reason(one.reason),
        })).collect::<Vec<_>>(),
        "beyondAnEnd": answer.profile.beyond_an_end(),
        "length": answer.length,
        "band": answer.band,
        "refused": answer.refused.iter().map(|(id, why)| json!({ "id": id, "why": why })).collect::<Vec<_>>(),
        "system": answer.system,
        "heads": answer.heads,
    })
}

/// The track from the argument: a file when one exists by that name, else the
/// vertices written inline.
fn read_track(spec: &str) -> Result<Vec<[f64; 2]>> {
    let path = PathBuf::from(spec);
    if path.is_file() {
        let text = std::fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
        let json: Value =
            serde_json::from_str(&text).with_context(|| format!("{} is not JSON", path.display()))?;
        return track_of_geojson(&json);
    }
    track_of_inline(spec)
}

/// `x,y;x,y;…`.
fn track_of_inline(spec: &str) -> Result<Vec<[f64; 2]>> {
    spec.split(';')
        .filter(|vertex| !vertex.trim().is_empty())
        .map(|vertex| {
            let (x, y) = vertex
                .split_once(',')
                .ok_or_else(|| anyhow!("vertex `{vertex}`: expected `x,y`"))?;
            Ok([
                x.trim()
                    .parse()
                    .with_context(|| format!("vertex `{vertex}`: x"))?,
                y.trim()
                    .parse()
                    .with_context(|| format!("vertex `{vertex}`: y"))?,
            ])
        })
        .collect()
}

/// The first LineString of a GeoJSON geometry, Feature or collection.
fn track_of_geojson(json: &Value) -> Result<Vec<[f64; 2]>> {
    match json["type"].as_str() {
        Some("LineString") => json["coordinates"]
            .as_array()
            .ok_or_else(|| anyhow!("a LineString without coordinates"))?
            .iter()
            .map(|position| {
                let xy = position.as_array().filter(|p| p.len() >= 2);
                match xy.and_then(|p| Some([p[0].as_f64()?, p[1].as_f64()?])) {
                    Some(xy) => Ok(xy),
                    None => bail!("a position is not `[x, y]`: {position}"),
                }
            })
            .collect(),
        Some("Feature") => track_of_geojson(&json["geometry"]),
        Some("FeatureCollection") => {
            let line = json["features"]
                .as_array()
                .into_iter()
                .flatten()
                .find(|f| f["geometry"]["type"] == "LineString")
                .ok_or_else(|| anyhow!("the collection holds no LineString"))?;
            track_of_geojson(line)
        }
        Some(other) => bail!("a track is a LineString, not a {other}"),
        None => bail!("not GeoJSON: no `type`"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_inline_track_is_pairs() {
        assert_eq!(
            track_of_inline("0,0; 10,5.5").unwrap(),
            vec![[0.0, 0.0], [10.0, 5.5]]
        );
        assert!(track_of_inline("0,0;3").is_err());
    }

    #[test]
    fn a_feature_collection_gives_its_first_line_and_a_polygon_is_refused() {
        let collection = json!({"type":"FeatureCollection","features":[
            {"type":"Feature","geometry":{"type":"Point","coordinates":[1,1]}},
            {"type":"Feature","geometry":{"type":"LineString","coordinates":[[0,0,5],[3,4]]}}]});
        assert_eq!(
            track_of_geojson(&collection).unwrap(),
            vec![[0.0, 0.0], [3.0, 4.0]]
        );
        let polygon = json!({"type":"Polygon","coordinates":[]});
        assert!(track_of_geojson(&polygon).is_err());
    }
}
