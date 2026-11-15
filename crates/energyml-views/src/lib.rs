//! The views of an EnergyML package, as pictures.
//!
//! One registry of views ([`ViewKinds`]): each view ([`ViewKind`]) says what it can show in a package
//! ([`ViewKind::offers`], from the package index alone), declares the objects and the settings it takes, and draws
//! a [`DisplayList`](energyml_sheet::draw::DisplayList) from the computing the other crates already do - it lays
//! out, it does not compute. One door ([`draw_svg`]) checks a request ([`ViewAsk`]) against those declarations,
//! draws, and writes the SVG with the request in its metadata, so that [`redraw`] makes the same bytes again from
//! the drawing alone. A PNG is always that SVG rasterised ([`png`]), never a second drawing.
//!
//! A face (the command line, a desktop window, a web page) asks the same door: it holds no logic of its own.
//!
//! # Rules
//!
//! - **Refuse rather than draw.** A view that cannot show an object returns a named [`ViewRefusal`], never an
//!   empty picture. A part it leaves out, the rest being drawn, is a [`ViewNote`], written in the picture too.
//! - **The full uri inside.** Objects are named by their full ETP uri; a bare uuid is an adapter's, at the edge.
//! - **Colours by one door** ([`colour`]): the map the file gives, else a palette of
//!   [`energyml_domain::palettes`], said so in the legend; never an invented colour.
//!
//! # The views
//!
//! | Id | Shows |
//! |---|---|
//! | [`along_well`] | the geosteering section: a vertical section along a well, with the curtain of a property, the boundaries, the horizons and the tops |
//! | [`image_track`] | a property with several values per element on a wellbore frame as an image: depth down, a plane of the matrix across, the value as a colour |
//! | [`md_coverage`] | which objects cover which measured depths of a well: a band per MD reference, a bar per object, hatching for an extent inherited through a link, a cross for a reversed interval |
//! | [`property_insight`] | what a property holds: its form, its distribution, its relation with a sibling, its variation along each axis |
//! | [`provenance_graph`] | where each object comes from: activities, files, carriers; the whole package or the graph around one object |
//! | [`survey_qc`] | the control of a directional survey: plan and section, inclination, azimuth and dogleg against MD, the gap with a trajectory; rejected stations and contradicted stated values marked |
//! | [`table_curve`] | the curves of a table (KrPc, PVT): a curve per pair of numeric columns, a legend per column with its unit, a second ordinate axis when two families of units cross |
//! | [`table_schema`] | how the tables hold together: a box per table, an arrow per link |
//! | [`target`] | the hole seen end on at one measured depth: a property in rings and sectors under a declared angle convention |
//! | [`well_locator`] | a well laid flat (MD bar, MD against TVD, plan, vertical section) and a mark at a measured depth |
//! | [`well_sheet`] | a well's drilling sheet: the depth column, the stratigraphy and the interval properties of a wellbore frame as bands, on a page in millimetres |

pub mod along_well;
pub mod ask;
pub mod colour;
pub mod graph;
pub mod image_track;
pub mod kind;
pub mod md_coverage;
pub mod pick;
pub mod picture;
pub mod property_insight;
pub mod provenance_graph;
pub mod refusal;
pub mod setting;
pub mod style;
pub mod survey_qc;
pub mod table_curve;
pub mod table_schema;
pub mod target;
pub mod well_locator;
pub mod well_sheet;

pub use ask::ViewAsk;
pub use colour::{ColourDoor, ColourSource, NoColourMaps, PackageColours, ValueColours, value_colours};
pub use kind::{Drawn, ObjectSlot, Offer, Request, ViewContext, ViewKind, ViewKinds, ViewNote, local_type};
pub use picture::{Picture, draw_list, draw_svg, png, read_ask, redraw};
pub use refusal::ViewRefusal;
pub use setting::{SettingKind, Settled, ViewSetting};
