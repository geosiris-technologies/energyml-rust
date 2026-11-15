//! `well-sheet`: a well's drilling sheet - a depth column, bands of stratigraphy or lithology, property columns.
//!
//! Nothing is computed here. [`energyml_sheet::read::board_of_frame`] reads the frame into a board (the MD column,
//! one track per stratigraphic rank, one per interval property: the colours are its own, from the colour
//! cascade of `energyml_domain::well_strata`), [`energyml_sheet::layout::compute`] places it in millimetres, and
//! [`energyml_sheet::picture::of_layout`] translates the layout into the list. This module only declares the
//! settings, hands them over, and says what the reading and the layout had to say.
//!
//! The page is in millimetres, the sheet's own unit ([`energyml_sheet::draw::PageUnit::Millimetre`]): its size comes
//! from the layout (the page width and the scale asked for), not from the request's pixel size, which this view
//! does not read.

use energyml_domain::package_index::{NodeState, PackageIndex};
use energyml_sheet::layout::{self, HeightGoal, Params, Track};
use energyml_sheet::picture::{PictureStyle, of_layout};
use energyml_sheet::read::{self, Options, PropertyLabel, TrackSelection};
use energyml_storage::ObjectSummary;
use serde_json::Value;

use crate::kind::{Drawn, ObjectSlot, Offer, Request, ViewContext, ViewKind, ViewNote, local_type};
use crate::refusal::ViewRefusal;
use crate::setting::ViewSetting;
use crate::style;

/// The id of the view.
pub const ID: &str = "well-sheet";

/// The types it shows.
pub const TYPES: &[&str] = &["WellboreFrameRepresentation"];

/// The proportion modes it offers, the first the default: the ids of `energyml_sheet::layout::ModeRegistry::with_builtins`.
pub const PROPORTIONS: &[&str] = &["readable", "to-scale"];

/// How a property column is headed: the ids of [`PropertyLabel`], the first the default.
pub const LABELS: &[&str] = &["kind", "title"];

/// The view.
pub struct WellSheet;

impl ViewKind for WellSheet {
    fn id(&self) -> &'static str {
        ID
    }

    fn about(&self) -> &'static str {
        "a well's drilling sheet: the measured depth column, the stratigraphy and the interval properties of a wellbore frame as bands, on a page in millimetres"
    }

    fn objects(&self) -> Vec<ObjectSlot> {
        vec![ObjectSlot {
            role: "frame",
            types: TYPES,
            least: 1,
            most: 1,
            about: "a wellbore frame, whose intervals are the sheet's sections",
        }]
    }

    fn settings(&self) -> Vec<ViewSetting> {
        vec![
            ViewSetting::choice(
                "proportion",
                PROPORTIONS,
                "how depth becomes height: `readable` gives every section the room its text needs, `to-scale` keeps true proportions",
            )
            .default(Value::from(PROPORTIONS[0])),
            ViewSetting::number("scale", "the height of the sheet, in millimetres of paper per metre of MD")
                .uom("mm/m")
                .bounds(Some(0.01), None)
                .default(Value::from(4.0)),
            ViewSetting::number("page_width", "the width of the page, in millimetres: it widens when the columns need more")
                .uom("mm")
                .bounds(Some(50.0), None)
                .default(Value::from(180.0)),
            ViewSetting::number("text_size", "the size of the text, in points")
                .uom("pt")
                .bounds(Some(4.0), Some(24.0))
                .default(Value::from(8.0)),
            ViewSetting::text("md_header", "what is read above the depth column").default(Value::from("MD")),
            ViewSetting::choice(
                "label",
                LABELS,
                "how a property column is headed: by the title of its property kind, or by its own title",
            )
            .default(Value::from(LABELS[0])),
            ViewSetting::text(
                "tracks",
                "the property uuids to draw as columns, comma-separated, in this order; none: every interval property",
            ),
        ]
    }

    fn size(&self) -> [f64; 2] {
        // The page is in millimetres and sized by its layout: this is only what a face may show it in.
        [680.0, 900.0]
    }

    fn offers(&self, index: &PackageIndex) -> Vec<Offer> {
        index
            .nodes()
            .filter(|node| {
                node.state == NodeState::Present && TYPES.contains(&local_type(&node.qualified_type))
            })
            .map(|node| Offer {
                view: ID,
                objects: vec![node.uri.to_string()],
                title: node.title.as_deref().unwrap_or(&node.uuid).to_string(),
                missing: Vec::new(),
            })
            .collect()
    }

    fn draw(&self, request: &Request, cx: &ViewContext<'_>) -> Result<Drawn, ViewRefusal> {
        let refuse = |why: String| ViewRefusal::computing(ID, why);
        let uri = request
            .object("frame")
            .ok_or_else(|| refuse("no frame given".to_string()))?;
        let frame = cx.source.read_object_at(uri).map_err(refuse)?;
        let settings = &request.settings;

        // The package's own listing, from the index: the identity of every object it holds, nothing read again.
        let resources: Vec<ObjectSummary> = cx
            .index
            .nodes()
            .filter(|node| node.state == NodeState::Present)
            .map(|node| {
                ObjectSummary::new(
                    node.uri.to_string(),
                    node.uuid.to_string(),
                    node.qualified_type.to_string(),
                )
            })
            .collect();

        let mut options = Options::new(settings.text("md_header").unwrap_or("MD")).with_label(match settings
            .text("label")
        {
            Some("title") => PropertyLabel::Title,
            _ => PropertyLabel::Kind,
        });
        if let Some(wanted) = settings.text("tracks") {
            let uuids: Vec<String> = wanted
                .split(',')
                .map(|uuid| uuid.trim().to_string())
                .filter(|uuid| !uuid.is_empty())
                .collect();
            if !uuids.is_empty() {
                options = options.with_tracks(TrackSelection::Only(uuids));
            }
        }

        let mut read = read::board_of_frame(frame.as_reflect(), &resources, cx.source, &options)
            .map_err(|why| refuse(why.to_string()))?;
        // The title of the sheet is the frame's own: the reading names none, and never invents one.
        read.board.title = cx
            .index
            .node(uri)
            .and_then(|node| node.title.as_deref())
            .map(str::to_string);

        // Refuse rather than draw: only an interval column cuts the sheet into sections, and a frame with none has
        // nothing to show but its depth column.
        let has_section = read.board.tracks.iter().any(|track| match track {
            Track::Intervals(column) => !column.intervals.is_empty(),
            Track::Md(_) => false,
        });
        if !has_section {
            let said = if read.notes.is_empty() {
                String::new()
            } else {
                format!(" ({})", read.notes.join("; "))
            };
            return Err(refuse(format!(
                "the frame carries no stratigraphy and no interval property to draw, so the sheet has no section{said}"
            )));
        }

        let params = Params {
            proportion: settings.text("proportion").unwrap_or(PROPORTIONS[0]).to_string(),
            height: HeightGoal::Scale(settings.number("scale").unwrap_or(4.0)),
            page_width_mm: settings.number("page_width").unwrap_or(180.0),
            text_size_pt: settings.number("text_size").unwrap_or(8.0),
        };
        let laid = layout::compute(&read.board, &params).map_err(|why| refuse(why.to_string()))?;

        let list = of_layout(
            &laid,
            &PictureStyle {
                ink: style::INK,
                guide: style::NEUTRAL,
                background: Some(style::PAPER),
                object: Some(uri.to_string()),
                ..PictureStyle::default()
            },
        );
        // Refuse rather than draw: a sheet with no section has no height, and says so by name.
        list.validate().map_err(|why| refuse(why.to_string()))?;

        let mut notes: Vec<ViewNote> = read
            .notes
            .into_iter()
            .map(|says| ViewNote {
                key: "sheet-reading",
                says,
            })
            .collect();
        notes.extend(laid.notes.iter().map(|says| ViewNote {
            key: "sheet-layout",
            says: says.clone(),
        }));
        Ok(Drawn { list, notes })
    }
}
