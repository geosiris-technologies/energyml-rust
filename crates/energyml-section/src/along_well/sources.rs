//! The sources of a curtain: a registry, the caller's entries tried first
//! (ADR-0006). A source that is declared and not built refuses by name, never
//! an empty curtain (ADR-0005).

use energyml_domain::ArraySource;
use energyml_domain::component_axes::{AxisLink, AxisSources, AxisSubject, AxisValues, ComponentAxis};
use energyml_domain::property::shape::{ComponentPick, ElementShape};
use energyml_domain::series::matrix::{Plane, read_plane};
use energyml_domain::{property, units, values};
use energyml_geometry::around_well::transverse::{GridAxis, GridRefusal, PlaneAxis, TransverseGrid};

use super::model::{AlongWellRefusal, AxisSignAsk, CurtainAsk, Note, PlaneAxisAsk};

/// The columns of a curtain, one per frame node.
#[derive(Clone, Debug, PartialEq)]
pub struct CurtainColumns {
    /// The property (full uri).
    pub property: String,
    /// `values[node * rows + row]`, node slowest; NaN where absent.
    pub values: Vec<f64>,
    /// The offsets of the rows along the high side, metres, positive up.
    pub rows_up: Vec<f64>,
    /// The offset of the plane along `right`, metres.
    pub lateral: f64,
    /// The unit of the values, as the file writes it (never converted).
    pub unit: Option<String>,
    /// What the source has to say.
    pub notes: Vec<Note>,
}

/// One way to get the columns of a curtain.
pub trait CurtainSource: Send + Sync {
    /// `cube-on-frame`, `grid-property`, `lwd-image`.
    fn id(&self) -> &'static str;

    /// The columns of the curtain `ask` describes, on the nodes of `frame_uri`.
    ///
    /// # Errors
    ///
    /// An [`AlongWellRefusal`], by name.
    fn columns(
        &self,
        ask: &CurtainAsk,
        frame_uri: &str,
        node_count: u64,
        source: &dyn ArraySource,
    ) -> Result<CurtainColumns, AlongWellRefusal>;
}

/// The sources, the caller's first (ADR-0006).
pub struct CurtainSources {
    entries: Vec<Box<dyn CurtainSource>>,
}

impl CurtainSources {
    /// `cube-on-frame`, then `grid-property` and `lwd-image`, declared and refused.
    pub fn builtin() -> CurtainSources {
        CurtainSources {
            entries: vec![
                Box::new(CubeOnFrame),
                Box::new(NotBuilt("grid-property")),
                Box::new(NotBuilt("lwd-image")),
            ],
        }
    }

    /// Puts an entry before every entry already there.
    pub fn with(mut self, extra: Box<dyn CurtainSource>) -> CurtainSources {
        self.entries.insert(0, extra);
        self
    }

    /// The first entry of that id.
    pub fn get(&self, id: &str) -> Option<&dyn CurtainSource> {
        self.entries
            .iter()
            .find(|entry| entry.id() == id)
            .map(|entry| entry.as_ref())
    }

    /// The ids, in the order they are tried.
    pub fn ids(&self) -> Vec<&'static str> {
        self.entries.iter().map(|entry| entry.id()).collect()
    }
}

/// A source declared and not built: refused by name, never an empty curtain (ADR-0005).
pub struct NotBuilt(pub &'static str);

impl CurtainSource for NotBuilt {
    fn id(&self) -> &'static str {
        self.0
    }

    fn columns(
        &self,
        _ask: &CurtainAsk,
        _frame_uri: &str,
        _node_count: u64,
        _source: &dyn ArraySource,
    ) -> Result<CurtainColumns, AlongWellRefusal> {
        Err(AlongWellRefusal::SourceNotBuilt { id: self.0 })
    }
}

/// A cube on the nodes of a frame (UDAR's `Rh`, `Rv`).
pub struct CubeOnFrame;

/// The offsets of one axis in metres: listed or regular values in a length
/// unit. Ranks and labels are not offsets.
fn offsets_in_metres(axis: &ComponentAxis) -> Result<Vec<f64>, AlongWellRefusal> {
    let values: Vec<f64> = match &axis.values {
        AxisValues::Regular { start, step, count } => (0..*count).map(|i| start + i as f64 * step).collect(),
        AxisValues::Listed(listed) => listed.clone(),
        AxisValues::Labels(_) | AxisValues::Ranks(_) => {
            return Err(AlongWellRefusal::AxesUnknown { dims: vec![axis.dim] });
        }
    };
    let uom = axis.uom.clone().unwrap_or_default();
    let factor = units::metres(1.0, &uom).map_err(|_| AlongWellRefusal::AxisUnit { dim: axis.dim, uom })?;
    Ok(values.into_iter().map(|value| value * factor).collect())
}

fn sign_factor(sign: AxisSignAsk) -> f64 {
    match sign {
        AxisSignAsk::Positive => 1.0,
        AxisSignAsk::Negative => -1.0,
    }
}

/// Which dimension runs along the high side, which is lateral, and the pick of
/// the plane: every value along the high-side dimension, the lateral one fixed
/// at `lateral_index`. The roles are the caller's declaration (S2), never read
/// from the file.
///
/// # Errors
///
/// [`GridRefusal::SameAxisTwice`] (as [`AlongWellRefusal::Grid`]) when both
/// dimensions are declared along the same axis, [`AlongWellRefusal::LateralIndex`].
pub fn plane_of_roles(
    roles: &[(PlaneAxisAsk, AxisSignAsk); 2],
    lateral_index: usize,
    dims: [usize; 2],
) -> Result<(usize, usize, ComponentPick), AlongWellRefusal> {
    if roles[0].0 == roles[1].0 {
        return Err(GridRefusal::SameAxisTwice.into());
    }
    let h = if roles[0].0 == PlaneAxisAsk::HighSide {
        0
    } else {
        1
    };
    let l = 1 - h;
    if lateral_index >= dims[l] {
        return Err(AlongWellRefusal::LateralIndex {
            index: lateral_index,
            len: dims[l],
        });
    }
    let mut at = vec![0usize; 2];
    at[l] = lateral_index;
    Ok((h, l, ComponentPick::Along { axis: h, at }))
}

/// The columns of a plane read along the high-side dimension: the rows'
/// offsets are those of the high-side dimension, signed as declared; the
/// lateral offset is that of the lateral dimension at `lateral_index`.
pub fn place_plane(
    ask: &CurtainAsk,
    offsets: &[Vec<f64>; 2],
    h: usize,
    l: usize,
    plane: Plane,
    unit: Option<String>,
) -> CurtainColumns {
    let rows_up: Vec<f64> = offsets[h]
        .iter()
        .map(|offset| offset * sign_factor(ask.roles[h].1))
        .collect();
    let lateral = offsets[l][ask.lateral_index] * sign_factor(ask.roles[l].1);
    CurtainColumns {
        property: ask.property.clone(),
        values: plane.values,
        rows_up,
        lateral,
        unit,
        notes: vec![Note::RolesDeclared],
    }
}

impl CurtainSource for CubeOnFrame {
    fn id(&self) -> &'static str {
        "cube-on-frame"
    }

    fn columns(
        &self,
        ask: &CurtainAsk,
        frame_uri: &str,
        node_count: u64,
        source: &dyn ArraySource,
    ) -> Result<CurtainColumns, AlongWellRefusal> {
        let object = source
            .object_at(&ask.property)
            .ok_or_else(|| AlongWellRefusal::NotInPackage {
                uri: ask.property.clone(),
            })?;
        let read = |why: String| AlongWellRefusal::Read {
            uri: ask.property.clone(),
            why,
        };
        let property = property::read(object.as_reflect()).map_err(|why| read(why.to_string()))?;

        // 1. The support must be the frame.
        let frame_uuid = energyml_core::identity::parse_address(frame_uri).0;
        let support = property.supporting_representation.clone().unwrap_or_default();
        if !support.eq_ignore_ascii_case(&frame_uuid) {
            return Err(AlongWellRefusal::NotOnFrame { support });
        }

        // 2. Two dimensions per node, each at least two; never a factorised flat list.
        let counts = property.value_count_per_element.clone();
        if counts.len() == 1 && counts[0] > 1 {
            return Err(AlongWellRefusal::FlatComponents { count: counts[0] });
        }
        let shape = ElementShape::of_property(&property)?;
        if shape.rank() != 2 || shape.dims.iter().any(|&dim| dim < 2) {
            return Err(AlongWellRefusal::NotTwoDimensions { counts });
        }

        // 3. The axes: declared first, then the file's convention B; ranks are not offsets.
        let links: Vec<AxisLink> = ask.links.iter().cloned().map(AxisLink::from).collect();
        let resolved = AxisSources::with_builtins().resolve(
            &AxisSubject {
                uri: &ask.property,
                shape: &shape,
                facets: &property.facets,
                links: &links,
                rows: Some(node_count),
            },
            source,
            source,
        )?;
        let unknown: Vec<usize> = resolved
            .axes
            .iter()
            .filter(|axis| matches!(axis.values, AxisValues::Ranks(_) | AxisValues::Labels(_)))
            .map(|axis| axis.dim)
            .collect();
        if !unknown.is_empty() {
            return Err(AlongWellRefusal::AxesUnknown { dims: unknown });
        }
        let offsets = [
            offsets_in_metres(&resolved.axes[0])?,
            offsets_in_metres(&resolved.axes[1])?,
        ];

        // 4. The grid: the roles and signs are the caller's.
        let grid_axis = |d: usize| GridAxis {
            along: PlaneAxis::from(ask.roles[d].0),
            sign: ask.roles[d].1.into(),
            offsets: offsets[d].clone(),
        };
        TransverseGrid::new(grid_axis(0), grid_axis(1))?;

        // 5. The plane.
        let dims = [shape.dims[0], shape.dims[1]];
        let (h, l, pick) = plane_of_roles(&ask.roles, ask.lateral_index, dims)?;
        let mut plane = read_plane(&ask.property, &property, node_count, &pick, source)?;

        // 6. The columns; a `NullValue` the file declares is an absence.
        if let Some(patch) = property.patches.first()
            && let Some(null) = values::null_value(patch.array)
        {
            let null = null as f64;
            for value in &mut plane.values {
                if *value == null {
                    *value = f64::NAN;
                }
            }
        }
        // TODO(verify): a `ColorInformation` that names one component
        // (`ValueVectorIndex`) is not read here: no public door reads that entry
        // for one property outside the colour cascade, so the face puts the note
        // `Note::ColourOfOneComponent` itself.
        Ok(place_plane(ask, &offsets, h, l, plane, property.uom.clone()))
    }
}
