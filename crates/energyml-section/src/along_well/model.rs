//! The model of a geosteering section: what is asked ([`Request`]), what is
//! read once ([`Model`]), and every refusal by name ([`AlongWellRefusal`]).

use energyml_analysis::tops::{Tops, TopsError};
use energyml_core::identifiers::EtpUri;
use energyml_core::value::{FieldValue, Reflect};
use energyml_domain::ArraySource;
use energyml_domain::along_md::{InterpRefusal, median_step};
use energyml_domain::component_axes::{AxisLink, AxisRefusal};
use energyml_domain::package_index::CandidateNeed;
use energyml_domain::property::shape::{ElementShape, ShapeRefusal};
use energyml_domain::series::matrix::MatrixRefusal;
use energyml_domain::{property, strata_by_md, units, values};
use energyml_geometry::around_well::projection::{
    ProjectionRefusal, VerticalSection, WellAxis, WellProjection, WellProjections,
};
use energyml_geometry::around_well::transverse::{AxisSign, GridRefusal, PlaneAxis};
use energyml_geometry::well::{PathRefusal, PlacedPath};
use energyml_sheet::draw::{DrawError, MetadataError};

use super::boundaries::{Boundaries, BoundaryMode, BoundaryRefusal, boundaries};
use super::cells::{CurtainGeometry, CurtainLayout, CurtainRefusal, curtain_cells};
use super::horizons::{HorizonTrace, PlacedMesh, computed_tops, horizon_in_curtain};
use super::markers::{MarkerRefusal, MarkerTick};
use super::sources::{CurtainColumns, CurtainSources};

/// The format of the body of the drawing's metadata; a new one is a new number.
pub const FORMAT: u32 = 1;
/// The element of the drawing's `<metadata>` (`draw::svg::Metadata::element`).
pub const METADATA_ELEMENT: &str = "along-well-view";

// -- serialisable mirrors ------------------------------------------------------------

/// Mirror of `energyml_domain::component_axes::AxisLink`, which takes no `serde`.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AxisLinkAsk {
    /// A column of a table, by its title, for one dimension.
    Table {
        /// The table's uri.
        table: String,
        /// The column's title, exactly.
        column: String,
        /// The dimension it gives.
        dim: usize,
    },
    /// A property of shape `[dims[dim]]` on the same support.
    PairedProperty {
        /// The paired property's uri.
        property: String,
        /// The dimension it gives.
        dim: usize,
    },
    /// A WITSML channel whose `AxisDefinition` gives the axis.
    WitsmlChannel {
        /// The channel's uri.
        channel: String,
    },
}

impl From<AxisLinkAsk> for AxisLink {
    fn from(ask: AxisLinkAsk) -> AxisLink {
        match ask {
            AxisLinkAsk::Table { table, column, dim } => AxisLink::Table { table, column, dim },
            AxisLinkAsk::PairedProperty { property, dim } => AxisLink::PairedProperty { property, dim },
            AxisLinkAsk::WitsmlChannel { channel } => AxisLink::WitsmlChannel { channel },
        }
    }
}

impl From<AxisLink> for AxisLinkAsk {
    fn from(link: AxisLink) -> AxisLinkAsk {
        match link {
            AxisLink::Table { table, column, dim } => AxisLinkAsk::Table { table, column, dim },
            AxisLink::PairedProperty { property, dim } => AxisLinkAsk::PairedProperty { property, dim },
            AxisLink::WitsmlChannel { channel } => AxisLinkAsk::WitsmlChannel { channel },
        }
    }
}

/// Mirror of `energyml_geometry::around_well::transverse::PlaneAxis`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PlaneAxisAsk {
    /// Along the high side of the hole.
    HighSide,
    /// Along the right of an observer looking down the hole.
    Right,
}

impl From<PlaneAxisAsk> for PlaneAxis {
    fn from(ask: PlaneAxisAsk) -> PlaneAxis {
        match ask {
            PlaneAxisAsk::HighSide => PlaneAxis::HighSide,
            PlaneAxisAsk::Right => PlaneAxis::Right,
        }
    }
}

impl From<PlaneAxis> for PlaneAxisAsk {
    fn from(axis: PlaneAxis) -> PlaneAxisAsk {
        match axis {
            PlaneAxis::HighSide => PlaneAxisAsk::HighSide,
            PlaneAxis::Right => PlaneAxisAsk::Right,
        }
    }
}

/// Mirror of `energyml_geometry::around_well::transverse::AxisSign`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AxisSignAsk {
    /// Offsets count along the axis.
    Positive,
    /// Offsets count against the axis.
    Negative,
}

impl From<AxisSignAsk> for AxisSign {
    fn from(ask: AxisSignAsk) -> AxisSign {
        match ask {
            AxisSignAsk::Positive => AxisSign::Positive,
            AxisSignAsk::Negative => AxisSign::Negative,
        }
    }
}

impl From<AxisSign> for AxisSignAsk {
    fn from(sign: AxisSign) -> AxisSignAsk {
        match sign {
            AxisSign::Positive => AxisSignAsk::Positive,
            AxisSign::Negative => AxisSignAsk::Negative,
        }
    }
}

/// Mirror of `energyml_domain::package_index::CandidateNeed`.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CandidateNeedAsk {
    /// No axis source in the file: axes to declare.
    Axes,
    /// Angle origin and sense: never in the file.
    AngleConvention,
    /// Several angle siblings fit: the caller chooses.
    ChooseAngles {
        /// Their uris.
        candidates: Vec<String>,
    },
}

impl From<CandidateNeed> for CandidateNeedAsk {
    fn from(need: CandidateNeed) -> CandidateNeedAsk {
        match need {
            CandidateNeed::Axes => CandidateNeedAsk::Axes,
            CandidateNeed::AngleConvention => CandidateNeedAsk::AngleConvention,
            CandidateNeed::ChooseAngles { candidates } => CandidateNeedAsk::ChooseAngles { candidates },
        }
    }
}

impl From<CandidateNeedAsk> for CandidateNeed {
    fn from(ask: CandidateNeedAsk) -> CandidateNeed {
        match ask {
            CandidateNeedAsk::Axes => CandidateNeed::Axes,
            CandidateNeedAsk::AngleConvention => CandidateNeed::AngleConvention,
            CandidateNeedAsk::ChooseAngles { candidates } => CandidateNeed::ChooseAngles { candidates },
        }
    }
}

// -- the request ---------------------------------------------------------------------

/// What a geosteering section is asked - full uris only (a bare uuid is turned
/// into a uri at the edge, never here), every fact the file does not say
/// declared by the caller (S2).
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Request {
    /// The frame (full uri).
    pub frame: String,
    /// `None`: the trajectory the frame references.
    pub trajectory: Option<String>,
    /// The curtain, when one is asked.
    pub curtain: Option<CurtainAsk>,
    /// The boundaries, when asked.
    pub boundaries: Option<BoundaryAsk>,
    /// The surfaces whose horizons are cut (full uris).
    pub surfaces: Vec<String>,
    /// Whether the tops of the hole itself are computed.
    pub computed_tops: bool,
    /// Marker objects: refused by name today.
    pub markers: Vec<String>,
    /// `None`: the frame's extent.
    pub md_range: Option<[f64; 2]>,
    /// `None`: the curtain's rows, else the boundaries' largest radius, else no horizon is looked for (a note says so).
    pub dn_range: Option<[f64; 2]>,
    /// An id of `WellProjections`: `md_tvd`, `developed`, `vertical_section`.
    pub projection: String,
    /// For `vertical_section` only: what the caller builds the entry with.
    pub section: Option<SectionAsk>,
    /// Metres; `None`: the median node step, clamped to [0.1, 5].
    pub step: Option<f64>,
}

/// The vertical section the caller asks for.
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SectionAsk {
    /// Radians, clockwise from grid north (the caller converts degrees).
    pub azimuth: f64,
    /// `[east, north]`, metres, absolute.
    pub origin: [f64; 2],
}

/// The curtain a request asks for.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CurtainAsk {
    /// The property (full uri).
    pub property: String,
    /// An id of `CurtainSources`.
    pub source: String,
    /// The axes the caller declared (mirror of `AxisLink`), tried before the file's convention B.
    pub links: Vec<AxisLinkAsk>,
    /// For the slow then the fast dimension: which axis of the plane it runs along, and its sign (S2).
    pub roles: [(PlaneAxisAsk, AxisSignAsk); 2],
    /// The row shown along the lateral dimension.
    pub lateral_index: usize,
}

/// The boundaries a request asks for.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BoundaryAsk {
    /// The radii (full uri).
    pub radius: String,
    /// The angles (full uri).
    pub angle: String,
    /// How the boundaries are taken from the rings.
    pub mode: BoundaryMode,
}

/// What the drawing says beside the picture: typed; [`Note::english`] is the
/// sentence the exported SVG carries (a face translates for its screen).
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum Note {
    /// The roles and signs of the two dimensions are the caller's declaration.
    RolesDeclared,
    /// Cells with no value are left transparent.
    NoValue {
        /// How many.
        cells: usize,
    },
    /// The curtain folds over its own centre line.
    Folded {
        /// How many stretches of the hole.
        spans: usize,
    },
    /// The section runs backwards.
    Reversed {
        /// How many stretches of the hole.
        spans: usize,
    },
    /// Gaps in the data are not bridged.
    Gaps {
        /// How many.
        spans: usize,
    },
    /// Nodes with no high side are not drawn.
    NodesWithoutFrame {
        /// How many.
        count: usize,
    },
    /// The curtain's plane is off the hole and is drawn in the plane of the hole.
    LateralOffPlane {
        /// The lateral offset, metres.
        lateral: f64,
    },
    /// Marker objects were asked and not read.
    MarkersNotRead {
        /// How many.
        count: usize,
    },
    /// The file colours one component of the curtain's property only.
    ColourOfOneComponent {
        /// The component.
        index: usize,
    },
    /// No offset range is known: no horizon is looked for.
    NoDnRange,
}

impl Note {
    /// The sentence the SVG carries.
    pub fn english(&self) -> String {
        match self {
            Note::RolesDeclared => {
                "the roles and signs of the curtain's two dimensions are the caller's declaration: the file does not state them".to_string()
            }
            Note::NoValue { cells } => format!("{cells} cell(s) have no value and are left transparent"),
            Note::Folded { spans } => format!(
                "the curtain folds over itself on {spans} stretch(es) of the hole; the cells are drawn where they fall"
            ),
            Note::Reversed { spans } => {
                format!("the section runs backwards on {spans} stretch(es) of the hole")
            }
            Note::Gaps { spans } => format!("{spans} gap(s) in the data are not bridged"),
            Note::NodesWithoutFrame { count } => format!(
                "{count} node(s) have no high side (the hole is too close to vertical) and are not drawn"
            ),
            Note::LateralOffPlane { lateral } => format!(
                "the plane of the curtain is {lateral} m off the hole; it is drawn in the plane of the hole"
            ),
            Note::MarkersNotRead { count } => {
                format!("{count} marker object(s) were not read: this build has no reader of markers")
            }
            Note::ColourOfOneComponent { index } => {
                format!("the file colours component {index} of the property only")
            }
            Note::NoDnRange => "no offset range is known: no horizon was looked for".to_string(),
        }
    }
}

// -- the refusals --------------------------------------------------------------------

/// What a geosteering section refuses, by name.
#[derive(Debug, thiserror::Error)]
pub enum AlongWellRefusal {
    /// A field of the request is not an object uri.
    #[error("`{field}` is not the uri of an object: `{given}`")]
    NotAUri {
        /// The field of the request.
        field: &'static str,
        /// What it holds.
        given: String,
    },
    /// The object is not in the package, or a surface was asked and not given.
    #[error("`{uri}` is not in the package")]
    NotInPackage {
        /// The uri asked.
        uri: String,
    },
    /// The frame is not a wellbore frame, or the property is not on it.
    #[error("`{support}` is not the wellbore frame the curtain hangs on")]
    NotOnFrame {
        /// What the property is on, or what the frame is.
        support: String,
    },
    /// The trajectory cannot be placed.
    #[error(transparent)]
    Path(#[from] PathRefusal),
    /// A projection refuses.
    #[error(transparent)]
    Projection(#[from] ProjectionRefusal),
    /// The frame's depths cannot be read.
    #[error("the frame's nodes cannot be read: {0}")]
    Frame(String),
    /// A projection that is not in the registry.
    #[error("no projection `{id}` (and `vertical_section` needs a `section`); the registry holds {known:?}")]
    UnknownProjection {
        /// The id asked.
        id: String,
        /// The ids of the registry.
        known: Vec<&'static str>,
    },
    /// A source that is not in the registry.
    #[error("no curtain source `{id}`; the registry holds {known:?}")]
    UnknownSource {
        /// The id asked.
        id: String,
        /// The ids of the registry.
        known: Vec<&'static str>,
    },
    /// A source declared and not built.
    #[error("the curtain source `{id}` is declared and not built in this version")]
    SourceNotBuilt {
        /// The id of the source (named `id`, not `source`: thiserror reads a field called `source` as the error's cause).
        id: &'static str,
    },
    /// A property that is not a matrix of two dimensions per node.
    #[error(
        "a curtain needs a matrix of two dimensions per node, each at least 2; the property declares {counts:?}"
    )]
    NotTwoDimensions {
        /// The counts per element it declares.
        counts: Vec<u64>,
    },
    /// A flat list of components: never factorised.
    #[error(
        "the property declares {count} components in one dimension: they are never factorised into a matrix"
    )]
    FlatComponents {
        /// The count.
        count: u64,
    },
    /// The shape of a property is refused.
    #[error(transparent)]
    Shape(#[from] ShapeRefusal),
    /// A plane of the matrix cannot be read.
    #[error(transparent)]
    Matrix(#[from] MatrixRefusal),
    /// Only ranks describe an axis.
    #[error("the axes of dimension(s) {dims:?} are not stated: declare them (ranks are not offsets)")]
    AxesUnknown {
        /// The dimensions with no axis.
        dims: Vec<usize>,
    },
    /// An axis is refused.
    #[error(transparent)]
    Axes(#[from] AxisRefusal),
    /// An axis whose unit is not a length.
    #[error("the axis of dimension {dim} is in `{uom}`, which is not a length this build converts")]
    AxisUnit {
        /// The dimension.
        dim: usize,
        /// The unit, as the file spells it (empty: none).
        uom: String,
    },
    /// The grid of the matrix is refused.
    #[error(transparent)]
    Grid(#[from] GridRefusal),
    /// The lateral index is outside the lateral dimension.
    #[error("lateral index {index} in a dimension of {len}")]
    LateralIndex {
        /// The index asked.
        index: usize,
        /// The length of the dimension.
        len: usize,
    },
    /// The curtain is refused.
    #[error(transparent)]
    Curtain(#[from] CurtainRefusal),
    /// The boundaries are refused.
    #[error(transparent)]
    Boundaries(#[from] BoundaryRefusal),
    /// The tops are refused.
    #[error(transparent)]
    Tops(#[from] TopsError),
    /// A boundary property whose unit is not the right quantity.
    #[error("`{uri}` is in `{uom}`: a radius is a length and an angle an angle")]
    BoundaryUnit {
        /// The property.
        uri: String,
        /// The unit, as the file spells it (empty: none).
        uom: String,
    },
    /// A surface with no face.
    #[error("`{uri}` has no triangle and no quadrilateral: no horizon can be cut")]
    NoFaces {
        /// The surface.
        uri: String,
    },
    /// An interpolation that is not in the registry.
    #[error("no interpolation `{id}`; the registry holds {known:?}")]
    UnknownInterpolation {
        /// The id asked.
        id: String,
        /// The ids of the registry.
        known: Vec<&'static str>,
    },
    /// An interpolation refuses.
    #[error(transparent)]
    Interpolation(#[from] InterpRefusal),
    /// The drawing is refused.
    #[error(transparent)]
    Draw(#[from] DrawError),
    /// The metadata of a drawing does not read.
    #[error(transparent)]
    Metadata(#[from] MetadataError),
    /// The body of the metadata is not this version's.
    #[error("the drawing's metadata is not a geosteering section this build reads: {0}")]
    BadMetadata(String),
    /// An object cannot be read.
    #[error("`{uri}` cannot be read: {why}")]
    Read {
        /// The object.
        uri: String,
        /// Why.
        why: String,
    },
    /// A surface that cannot be indexed for heights.
    #[error(transparent)]
    Heights(#[from] energyml_analysis::Error),
    /// A legend that was asked and cannot be drawn.
    #[error("the legend cannot be drawn: {0}")]
    Legend(String),
}

// -- the model -----------------------------------------------------------------------

/// A surface's tops along the hole.
#[derive(Clone, Debug)]
pub struct SurfaceTops {
    /// The uri of the surface.
    pub surface: String,
    /// What `tops::tops` found.
    pub tops: Tops,
    /// Each crossing placed by the projection, `[abscissa, tvd]`, in the order
    /// of `tops.crossings`; `None` where the projection refused it.
    pub placed: Vec<Option<[f64; 2]>>,
}

/// The curtain: its layout, its placed cells, its values.
#[derive(Clone, Debug)]
pub struct Curtain {
    /// Where the cells begin and end.
    pub layout: CurtainLayout,
    /// The cells, placed.
    pub geometry: CurtainGeometry,
    /// The values.
    pub columns: CurtainColumns,
}

/// Everything a section is drawn from, read once (the expensive stage).
#[derive(Clone, Debug)]
pub struct Model {
    /// What was asked.
    pub request: Request,
    /// The hole, placed.
    pub path: PlacedPath,
    /// The hole sampled once.
    pub axis: WellAxis,
    /// The hole placed at each sample of `axis.md`, `[abscissa, tvd]`; `None`
    /// where the projection refused it (a hole stays a hole).
    pub trace: Vec<Option<[f64; 2]>>,
    /// The measured depths of the frame's nodes.
    pub node_md: Vec<f64>,
    /// The curtain, when asked.
    pub curtain: Option<Curtain>,
    /// The boundaries, when asked.
    pub boundaries: Option<Boundaries>,
    /// The horizons cut.
    pub horizons: Vec<HorizonTrace>,
    /// The tops of the hole.
    pub tops: Vec<SurfaceTops>,
    /// The markers placed (a caller that holds markers pushes them).
    pub markers: Vec<MarkerTick>,
    /// The markers refused by name.
    pub markers_refused: Vec<MarkerRefusal>,
    /// What the drawing says beside the picture.
    pub notes: Vec<Note>,
}

/// The projection a request names: `vertical_section` is built from the
/// request's own section, every other id comes from the registry.
pub(crate) struct ProjectionChoice {
    section: Option<VerticalSection>,
}

impl ProjectionChoice {
    pub(crate) fn of(request: &Request) -> ProjectionChoice {
        ProjectionChoice {
            section: request.section.map(|section| VerticalSection {
                azimuth: section.azimuth,
                origin: section.origin,
            }),
        }
    }

    pub(crate) fn get<'a>(
        &'a self,
        id: &str,
        projections: &'a WellProjections,
    ) -> Result<&'a dyn WellProjection, AlongWellRefusal> {
        let found: Option<&'a dyn WellProjection> = if id == "vertical_section" {
            self.section
                .as_ref()
                .map(|section| section as &dyn WellProjection)
        } else {
            projections.get(id)
        };
        found.ok_or_else(|| AlongWellRefusal::UnknownProjection {
            id: id.to_string(),
            known: projections.ids(),
        })
    }
}

fn check_uri(field: &'static str, given: &str) -> Result<(), AlongWellRefusal> {
    match EtpUri::parse(given) {
        Ok(uri) if uri.uuid.is_some() => Ok(()),
        _ => Err(AlongWellRefusal::NotAUri {
            field,
            given: given.to_string(),
        }),
    }
}

fn validate(request: &Request) -> Result<(), AlongWellRefusal> {
    check_uri("frame", &request.frame)?;
    if let Some(uri) = &request.trajectory {
        check_uri("trajectory", uri)?;
    }
    if let Some(ask) = &request.curtain {
        check_uri("curtain.property", &ask.property)?;
    }
    if let Some(ask) = &request.boundaries {
        check_uri("boundaries.radius", &ask.radius)?;
        check_uri("boundaries.angle", &ask.angle)?;
    }
    for uri in &request.surfaces {
        check_uri("surfaces", uri)?;
    }
    for uri in &request.markers {
        check_uri("markers", uri)?;
    }
    Ok(())
}

/// The numbers of a one-patch property, its unit and its values per node;
/// each `NullValue` the file declares becomes NaN.
fn read_numbers(
    uri: &str,
    source: &dyn ArraySource,
) -> Result<(Vec<f64>, Option<String>, usize), AlongWellRefusal> {
    let read = |why: String| AlongWellRefusal::Read {
        uri: uri.to_string(),
        why,
    };
    let object = source
        .object_at(uri)
        .ok_or_else(|| AlongWellRefusal::NotInPackage { uri: uri.to_string() })?;
    let property = property::read(object.as_reflect()).map_err(|why| read(why.to_string()))?;
    if property.patches.len() != 1 {
        return Err(read(format!(
            "{} patch(es): a boundary property has exactly one",
            property.patches.len()
        )));
    }
    let patch = &property.patches[0];
    let read_values = patch.read(source).map_err(|why| read(why.to_string()))?;
    let mut numbers = read_values
        .to_f64()
        .ok_or_else(|| read("the values are not numbers".to_string()))?;
    if let Some(null) = values::null_value(patch.array) {
        let null = null as f64;
        for value in &mut numbers {
            if *value == null {
                *value = f64::NAN;
            }
        }
    }
    let per_node = ElementShape::of_property(&property)?.len();
    Ok((numbers, property.uom.clone(), per_node))
}

/// The nodes' depths of the frame, the frame's trajectory and the path.
fn frame_and_path(
    request: &Request,
    source: &dyn ArraySource,
) -> Result<(Vec<f64>, PlacedPath), AlongWellRefusal> {
    let frame = source
        .object_at(&request.frame)
        .ok_or_else(|| AlongWellRefusal::NotInPackage {
            uri: request.frame.clone(),
        })?;
    let frame_reflect: &dyn Reflect = frame.as_reflect();
    let type_key = frame_reflect.type_info().type_key;
    if type_key != "WellboreFrameRepresentation" {
        return Err(AlongWellRefusal::NotOnFrame {
            support: type_key.to_string(),
        });
    }
    let node_md = strata_by_md::node_mds(frame_reflect, source)
        .map_err(|why| AlongWellRefusal::Frame(why.to_string()))?;
    if node_md.is_empty() {
        return Err(AlongWellRefusal::Frame("the frame has no node".to_string()));
    }
    let trajectory = match &request.trajectory {
        Some(uri) => source
            .object_at(uri)
            .ok_or_else(|| AlongWellRefusal::NotInPackage { uri: uri.clone() })?,
        None => {
            let Some(FieldValue::Object(reference)) = frame_reflect.field("Trajectory") else {
                return Err(AlongWellRefusal::Frame(
                    "the frame references no trajectory and none was given".to_string(),
                ));
            };
            source
                .object_of(reference)
                .ok_or_else(|| AlongWellRefusal::NotInPackage {
                    uri: "the frame's trajectory".to_string(),
                })?
        }
    };
    let path = PlacedPath::of(trajectory.as_reflect(), source)?;
    Ok((node_md, path))
}

/// Everything a section is drawn from, read once.
///
/// The steps: the request is validated (full uris, a projection the registry
/// holds), the frame and the trajectory are read and the hole placed, the
/// curtain's columns come from the source asked for, the cells are placed by
/// the projection, the boundaries are read and placed, the horizons are cut
/// and the tops of the hole computed, and every marker uri is refused by
/// name. A node with no high side (S4) is counted, never a refusal of the whole.
///
/// # Errors
///
/// An [`AlongWellRefusal`], by name.
pub fn build(
    request: &Request,
    source: &dyn ArraySource,
    surfaces: &[PlacedMesh],
    sources: &CurtainSources,
    projections: &WellProjections,
) -> Result<Model, AlongWellRefusal> {
    validate(request)?;
    let choice = ProjectionChoice::of(request);
    let projection = choice.get(&request.projection, projections)?;

    let (node_md, path) = frame_and_path(request, source)?;
    let md_range = request
        .md_range
        .unwrap_or([node_md[0], node_md[node_md.len() - 1]]);
    let axis = WellAxis::new(&path, md_range, 0.0)?;
    let trace: Vec<Option<[f64; 2]>> = axis
        .md
        .iter()
        .map(|&md| projection.place(&axis, md, 0.0).ok())
        .collect();

    let mut notes: Vec<Note> = Vec::new();

    // The curtain.
    let mut curtain = None;
    if let Some(ask) = &request.curtain {
        let curtain_source = sources
            .get(&ask.source)
            .ok_or_else(|| AlongWellRefusal::UnknownSource {
                id: ask.source.clone(),
                known: sources.ids(),
            })?;
        let columns = curtain_source.columns(ask, &request.frame, node_md.len() as u64, source)?;
        let layout = CurtainLayout::new(&node_md, &columns.rows_up, columns.lateral)?;
        let geometry = curtain_cells(&axis, &layout, projection)?;
        notes.extend(columns.notes.iter().cloned());
        let no_value = columns.values.iter().filter(|value| value.is_nan()).count();
        if no_value > 0 {
            notes.push(Note::NoValue { cells: no_value });
        }
        if !geometry.folded.is_empty() {
            notes.push(Note::Folded {
                spans: geometry.folded.len(),
            });
        }
        if !geometry.reversed.is_empty() {
            notes.push(Note::Reversed {
                spans: geometry.reversed.len(),
            });
        }
        if !geometry.gaps.is_empty() {
            notes.push(Note::Gaps {
                spans: geometry.gaps.len(),
            });
        }
        if !geometry.missing.is_empty() {
            notes.push(Note::NodesWithoutFrame {
                count: geometry.missing.len(),
            });
        }
        if columns.lateral != 0.0 {
            notes.push(Note::LateralOffPlane {
                lateral: columns.lateral,
            });
        }
        curtain = Some(Curtain {
            layout,
            geometry,
            columns,
        });
    }

    // The boundaries.
    let mut limits = None;
    if let Some(ask) = &request.boundaries {
        let (mut radius, radius_unit, per_node) = read_numbers(&ask.radius, source)?;
        let (mut angle, angle_unit, _) = read_numbers(&ask.angle, source)?;
        let radius_unit = radius_unit.unwrap_or_default();
        let angle_unit = angle_unit.unwrap_or_default();
        let metres = units::metres(1.0, &radius_unit).map_err(|_| AlongWellRefusal::BoundaryUnit {
            uri: ask.radius.clone(),
            uom: radius_unit.clone(),
        })?;
        let radians = units::radians(1.0, &angle_unit).map_err(|_| AlongWellRefusal::BoundaryUnit {
            uri: ask.angle.clone(),
            uom: angle_unit.clone(),
        })?;
        radius.iter_mut().for_each(|value| *value *= metres);
        angle.iter_mut().for_each(|value| *value *= radians);
        let found = boundaries(&axis, &node_md, &radius, &angle, per_node, ask.mode, projection)?;
        if curtain.is_none() && found.refused > 0 {
            notes.push(Note::NodesWithoutFrame { count: found.refused });
        }
        limits = Some(found);
    }

    // The range of offsets the horizons are looked for in.
    let step = request
        .step
        .unwrap_or_else(|| median_step(&node_md).map_or(1.0, |median| median.clamp(0.1, 5.0)));
    let dn_range = request.dn_range.or_else(|| {
        if let Some(curtain) = &curtain {
            let bounds = &curtain.layout.row_bounds;
            let low = bounds.iter().map(|b| b[0]).fold(f64::INFINITY, f64::min);
            let high = bounds.iter().map(|b| b[1]).fold(f64::NEG_INFINITY, f64::max);
            return Some([low, high]);
        }
        let found = limits.as_ref()?;
        let mut largest = 0.0_f64;
        let points = found
            .above
            .iter()
            .chain(found.below.iter())
            .flatten()
            .chain(found.projected.iter().flatten());
        for point in points {
            largest = largest.max(point.dn.abs());
        }
        (largest > 0.0).then_some([-largest, largest])
    });
    if dn_range.is_none() && !request.surfaces.is_empty() {
        notes.push(Note::NoDnRange);
    }

    // The horizons, and the tops of the hole.
    let mut horizons = Vec::new();
    let mut tops = Vec::new();
    for uri in &request.surfaces {
        let mesh = surfaces
            .iter()
            .find(|mesh| &mesh.uri == uri)
            .ok_or_else(|| AlongWellRefusal::NotInPackage { uri: uri.clone() })?;
        let heights = mesh.heights()?;
        if let Some(range) = dn_range {
            horizons.push(horizon_in_curtain(
                &path, &axis, &node_md, range, step, &heights, uri, projection,
            )?);
        }
        if request.computed_tops {
            let found = computed_tops(&path, md_range, step, &heights)?;
            let placed = found
                .crossings
                .iter()
                .map(|crossing| projection.place(&axis, crossing.md, 0.0).ok())
                .collect();
            tops.push(SurfaceTops {
                surface: uri.clone(),
                tops: found,
                placed,
            });
        }
    }

    // The markers: refused by name, the section still draws.
    let mut markers_refused = Vec::new();
    for uri in &request.markers {
        let qualified_type = source
            .object_at(uri)
            .map(|object| energyml_core::json::qualified_type_of(object.as_reflect().type_info()))
            .unwrap_or_else(|| "object the package does not hold".to_string());
        markers_refused.push(MarkerRefusal::NoReader {
            uri: uri.clone(),
            qualified_type,
        });
    }
    if !markers_refused.is_empty() {
        notes.push(Note::MarkersNotRead {
            count: markers_refused.len(),
        });
    }

    Ok(Model {
        request: request.clone(),
        path,
        axis,
        trace,
        node_md,
        curtain,
        boundaries: limits,
        horizons,
        tops,
        markers: Vec::new(),
        markers_refused,
        notes,
    })
}
