//! `energyml view`: the views of a package, as pictures - what each view can show, what it takes, its SVG, and its
//! PNG on demand.
//!
//! Every decision is `energyml_views`'s: this file reads the command line, opens the package, asks the one door
//! (`draw_svg`, `redraw`) and writes the files. The PNG is the SVG just written, rasterised (`energyml_views::png`),
//! never a second drawing.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow, bail};
use clap::{Args, Subcommand};
use energyml_domain::package_index::PackageIndex;
use energyml_domain::palettes::Palettes;
use energyml_storage::EpcStore;
use energyml_views::{PackageColours, Picture, ViewAsk, ViewContext, ViewKinds};

use crate::{Modes, layers, open};

/// `energyml view`.
#[derive(Args, Debug)]
pub struct ViewArgs {
    #[command(subcommand)]
    command: ViewCommand,
}

#[derive(Subcommand, Debug)]
enum ViewCommand {
    /// What each view can show in a package, from its index alone (no array is read).
    List(ListArgs),
    /// What a view takes: its objects and its settings, with their defaults.
    About(AboutArgs),
    /// Draw one view as SVG - and as PNG, the SVG rasterised, with `--png`.
    Draw(DrawArgs),
    /// Draw a view again from the request its SVG keeps, over a package: on the same package, the same bytes.
    Redraw(RedrawArgs),
}

#[derive(Args, Debug)]
struct ListArgs {
    /// The EPC file.
    file: PathBuf,
    /// Only the offers that show this object (a uri, or a bare uuid).
    #[arg(long, value_name = "URI|UUID")]
    object: Option<String>,
    /// Print the offers as JSON.
    #[arg(long)]
    json: bool,
}

#[derive(Args, Debug)]
struct AboutArgs {
    /// The view (`energyml view list` names them).
    view: String,
}

#[derive(Args, Debug)]
struct DrawArgs {
    /// The view.
    view: String,
    /// The EPC file.
    file: PathBuf,
    /// An object the view shows, by uri or bare uuid; repeated, in the order of the view's slots.
    #[arg(long = "object", value_name = "URI|UUID")]
    objects: Vec<String>,
    /// A setting, `NAME=VALUE`, read by the nature the view declares (`energyml view about VIEW`); repeatable.
    #[arg(long = "set", value_name = "NAME=VALUE")]
    settings: Vec<String>,
    /// The size of the picture in pixels, `WIDTHxHEIGHT`; left out, the view's own.
    #[arg(long, value_name = "WxH")]
    size: Option<String>,
    #[command(flatten)]
    out: Output,
}

#[derive(Args, Debug)]
struct RedrawArgs {
    /// The EPC file the drawing is made again over.
    file: PathBuf,
    /// The drawing whose request is made again.
    #[arg(long = "from-svg", value_name = "FILE.svg")]
    from_svg: PathBuf,
    #[command(flatten)]
    out: Output,
}

#[derive(Args, Debug)]
struct Output {
    /// Where the SVG is written.
    #[arg(long, value_name = "FILE.svg")]
    svg: PathBuf,
    /// Also write the PNG: the SVG rasterised (built with `--features png`; refused by name otherwise).
    #[arg(long, value_name = "FILE.png")]
    png: Option<PathBuf>,
    /// With `--png`: how many pixels per pixel of the SVG (2: sharp on a phone).
    #[arg(long, default_value_t = 2.0)]
    png_scale: f32,
}

/// Run `energyml view`.
pub fn run(args: ViewArgs, modes: Modes) -> Result<()> {
    let kinds = ViewKinds::with_builtins();
    match args.command {
        ViewCommand::List(args) => list(&kinds, args, &modes),
        ViewCommand::About(args) => about(&kinds, &args.view),
        ViewCommand::Draw(args) => draw(&kinds, args, &modes),
        ViewCommand::Redraw(args) => redraw(&kinds, args, &modes),
    }
}

fn list(kinds: &ViewKinds, args: ListArgs, modes: &Modes) -> Result<()> {
    let store = open(&args.file, modes)?;
    let index = PackageIndex::build(&store, 0);
    let wanted = match &args.object {
        None => None,
        Some(name) => Some(
            index
                .node(name.trim())
                .map(|node| node.uri.to_string())
                .ok_or_else(|| anyhow!("the package holds no `{name}`"))?,
        ),
    };
    let offers: Vec<_> = kinds
        .offers(&index)
        .into_iter()
        .filter(|offer| wanted.as_ref().is_none_or(|uri| offer.objects.contains(uri)))
        .collect();
    if args.json {
        println!("{}", serde_json::to_string_pretty(&offers)?);
        return Ok(());
    }
    if offers.is_empty() {
        println!("no view can show anything of this package");
    }
    for offer in &offers {
        println!("{:<18} {}  {}", offer.view, offer.title, offer.objects.join(" "));
        for missing in &offer.missing {
            println!("{:<18}   needs {missing}", "");
        }
    }
    Ok(())
}

fn about(kinds: &ViewKinds, id: &str) -> Result<()> {
    let Some(kind) = kinds.find(id) else {
        bail!("no view `{id}`: the views are {}", kinds.ids().join(", "));
    };
    println!("{}: {}", kind.id(), kind.about());
    let size = kind.size();
    println!("size: {} x {} pixels unless `--size` says", size[0], size[1]);
    println!("objects:");
    for slot in kind.objects() {
        println!(
            "  {} ({} to {}): {} - {}",
            slot.role,
            slot.least,
            slot.most,
            slot.types.join(", "),
            slot.about
        );
    }
    println!("settings:");
    for setting in kind.settings() {
        let default = match (&setting.default, setting.required) {
            (Some(value), _) => format!(", default {value}"),
            (None, true) => ", required".to_string(),
            (None, false) => String::new(),
        };
        println!(
            "  {}: {}{default} - {}",
            setting.name,
            setting.nature(),
            setting.about
        );
    }
    Ok(())
}

fn draw(kinds: &ViewKinds, args: DrawArgs, modes: &Modes) -> Result<()> {
    refuse_png_without_the_feature(&args.out)?;
    let Some(kind) = kinds.find(&args.view) else {
        bail!(
            "no view `{}`: the views are {}",
            args.view,
            kinds.ids().join(", ")
        );
    };
    let mut ask = ViewAsk::new(kind.id());
    for object in &args.objects {
        ask = ask.object(object.trim());
    }
    let declared = kind.settings();
    for pair in &args.settings {
        let Some((name, text)) = pair.split_once('=') else {
            bail!("`--set {pair}`: write NAME=VALUE");
        };
        let name = name.trim();
        // A name the view does not declare goes through as text: the door refuses it by name, with the known ones.
        let value = match declared.iter().find(|setting| setting.name == name) {
            Some(setting) => setting
                .parse(text)
                .map_err(|why| anyhow!("setting `{name}`: {why}"))?,
            None => serde_json::Value::from(text),
        };
        ask = ask.set(name, value);
    }
    if let Some(size) = &args.size {
        let (width, height) = parse_size(size)?;
        ask = ask.sized(width, height);
    }
    let store = open(&args.file, modes)?;
    let picture = with_context(&store, |cx| energyml_views::draw_svg(kinds, &ask, cx))?;
    write(&picture, &args.out)
}

fn redraw(kinds: &ViewKinds, args: RedrawArgs, modes: &Modes) -> Result<()> {
    refuse_png_without_the_feature(&args.out)?;
    let svg = std::fs::read_to_string(&args.from_svg)
        .with_context(|| format!("reading {}", args.from_svg.display()))?;
    let store = open(&args.file, modes)?;
    let picture = with_context(&store, |cx| energyml_views::redraw(kinds, &svg, cx))?;
    write(&picture, &args.out)
}

/// The context of a view over an open package: its index, its colours by the cache's walk, the palettes.
fn with_context(
    store: &EpcStore,
    then: impl FnOnce(&ViewContext<'_>) -> Result<Picture, energyml_views::ViewRefusal>,
) -> Result<Picture> {
    let index = PackageIndex::build(store, 0);
    let cache = layers::cache_for("view");
    let colours = PackageColours { cache: &cache, store };
    let palettes = Palettes::new();
    let cx = ViewContext {
        source: store,
        index: &index,
        colours: &colours,
        palettes: &palettes,
    };
    then(&cx).map_err(|why| anyhow!("{why}"))
}

fn write(picture: &Picture, out: &Output) -> Result<()> {
    for note in &picture.notes {
        eprintln!("note: {}", note.says);
    }
    write_file(&out.svg, picture.svg.as_bytes())?;
    if let Some(png) = &out.png {
        let bytes = energyml_views::png(&picture.svg, out.png_scale).map_err(|why| anyhow!("{why}"))?;
        write_file(png, &bytes)?;
    }
    Ok(())
}

fn write_file(path: &Path, bytes: &[u8]) -> Result<()> {
    std::fs::write(path, bytes).with_context(|| format!("writing {}", path.display()))?;
    println!("{}", path.display());
    Ok(())
}

/// `--png` from a build without the PNG export: refused before anything is read or drawn.
fn refuse_png_without_the_feature(out: &Output) -> Result<()> {
    if out.png.is_some() && !cfg!(feature = "png") {
        bail!(
            "this build cannot write a PNG: it was built without the `png` feature (rebuild the CLI with `--features png`)"
        );
    }
    Ok(())
}

/// Read `--size WIDTHxHEIGHT`, in pixels: two finite numbers above zero.
///
/// The one reading for `view` and `profile`: `NaN`, `inf`, zero and a negative
/// size draw nothing and say nothing, so they are refused by name.
pub(crate) fn parse_size(text: &str) -> Result<(f64, f64)> {
    let (width, height) = text
        .split_once(['x', 'X'])
        .ok_or_else(|| anyhow!("`--size {text}`: write WIDTHxHEIGHT, in pixels"))?;
    let number = |word: &str| -> Result<f64> {
        let value = word
            .trim()
            .parse::<f64>()
            .map_err(|_| anyhow!("`--size {text}`: `{word}` is not a number"))?;
        if !value.is_finite() || value <= 0.0 {
            bail!("`--size {text}`: `{word}` must be a finite number above zero");
        }
        Ok(value)
    };
    Ok((number(width)?, number(height)?))
}

#[cfg(test)]
mod size_tests {
    use super::parse_size;

    #[test]
    fn a_size_is_two_finite_positive_numbers() {
        assert_eq!(parse_size("800x600").expect("parses"), (800.0, 600.0));
        assert!(parse_size("NaNx600").is_err());
        assert!(parse_size("0x600").is_err());
        assert!(parse_size("800x-1").is_err());
        assert!(parse_size("infx3").is_err());
        // Break it: remove the `is_finite`/`<= 0.0` check in `parse_size`.
    }
}
