//! `subrepresentation` — a `SubRepresentation` of one supporting
//! representation: *“select a range of nodes, edges, faces, or volumes (cell)
//! indices from the topological support of an existing representation to
//! define a sub-representation”* (RESQML TRG 2.2 §8.9 p. 246).
//!
//! Step 8 of the build plan of Lutetia, the sub-representations:
//! the productive reservoir and the drainage of one well, as cell regions, with
//! `SupportingRepresentation` pointing at the grid and every index
//! below the number of cells.
//!
//! # The elements, and how they are chosen
//!
//! Two ways, a union (`select`):
//!
//! - **`indices`** — given, by their index in the support, with the element
//!   they index: `nodes`, `triangles` or `cells`. **Ordered**: *“An ordered
//!   list of indexable elements”* (TRG §8.9), so a property on the
//!   sub-representation lines up with the order written;
//! - **`ijk-box`** — the rule the reference-model description names for a region of cells: every
//!   cell whose I, J and K are in three ranges (both ends included, from 0,
//!   an axis left out whole), in RESQML's cell order — `i` fastest, then `j`,
//!   then `k`. The productive reservoir is the Chalk of the grid: the
//!   layers 4 to 10 of the reference model, `kFirst = 3`, `kLast = 9`. The arithmetic is
//!   `energyml_analysis::cells::ijk_box`.
//!
//! # What is counted, and refused
//!
//! The support's count of the element comes from **its shape**, never from a
//! second reading of its XML: the nodes it holds (a 2D grid its declared
//! `fastest × slowest`), its triangles across patches, the cells of an IJK grid
//! `Ni × Nj × Nk` — *“NI x NJ x NK”*, the cells a grid declares, defined or
//! not (Technical Usage Guide 2.2 Table 7, as `energyml_domain::property` quotes
//! it). **An index at or past that count is refused by name**, with the count
//!, and so is an index given twice: two elements of the
//! sub-representation would be one of the support, and a property would give
//! it two values.
//!
//! **Not offered, and why**: `edges` and `faces` have no enumeration a shape
//! carries — an IJK grid's *“"faces" indexing […] depends on the grid faulting
//! and column edge enumeration”* (usage guide §9.12.3 p. 159), a triangulated
//! surface's edges are numbered by no rule this build reads; the cells of an
//! unstructured grid are refused by name until something here draws them. A
//! cell selected that the support leaves **undefined** is legal — its index is
//! in range — and it is counted and said: it has no geometry to draw.
//!
//! # What is written
//!
//! RESQML 2.2 puts `IndexableElement` on the object and
//! `SupportingRepresentation` on each patch (`crates/energyml-geometry/ARCHITECTURE.md`,
//! the section on sub-representations): one patch, the indices in the external file
//! (`Plan::datasets`), the support by reference. The support must be an
//! object of the package: an artefact of the scene has no uuid a patch could
//! name.
//!
//! # Identity
//!
//! From what the sub-representation is: the support, the element, the indices
//! and the title. Another selection is another object.

use std::collections::BTreeMap;

use serde_json::json;

use energyml_analysis::cells;
use energyml_domain::provenance::Parameter;
use energyml_geometry::geometry::{Patch, Topology};

use crate::declare::{Choice, Inputs, Params, Shape};
use crate::operation::{About, ActivityDraft, Category, Context, Dataset, Need, NewObject, Note, Numbers, Operation, Plan, Refusal, Slot};

use super::support;
use super::written::{NamingParams, RepresentedParams};

/// The operation.
pub struct Subrepresentation;

/// What the given indices index.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Choice)]
pub enum Element {
    /// The support's nodes.
    Nodes,
    /// The triangles of a triangulated support.
    Triangles,
    /// The cells of an IJK grid.
    Cells,
}

/// How the elements are chosen.
#[derive(Debug, Params)]
#[params(tag = "select")]
pub enum Select {
    /// The indices, given.
    #[case(default)]
    Indices(Given),
    /// The cells of an IJK box.
    IjkBox(IjkBox),
}

/// The indices, given.
#[derive(Debug, Params)]
pub struct Given {
    /// indices: what they index in the support — its nodes, its triangles, the cells of an IJK grid
    pub element: Element,
    /// indices: the elements, by their index in the support from 0, in the order the sub-representation lists them
    #[param(least = 1, ordered)]
    pub indices: Vec<usize>,
}

/// An IJK box: three ranges, each end included.
#[derive(Debug, Params)]
pub struct IjkBox {
    /// ijk-box: the first I, from 0 — left out, 0
    pub i_first: Option<usize>,
    /// ijk-box: the last I, included — left out, the last of the grid
    pub i_last: Option<usize>,
    /// ijk-box: the first J, from 0 — left out, 0
    pub j_first: Option<usize>,
    /// ijk-box: the last J, included — left out, the last of the grid
    pub j_last: Option<usize>,
    /// ijk-box: the first K (a layer, gaps not counted), from 0 — left out, 0
    pub k_first: Option<usize>,
    /// ijk-box: the last K, included — left out, the last of the grid
    pub k_last: Option<usize>,
}

/// What `subrepresentation` is told.
#[derive(Debug, Params)]
pub struct SubrepresentationParams {
    /// How the elements are chosen.
    #[param(flatten)]
    pub select: Select,
    /// What the sub-representation represents, when named — it may be another interpretation than the support's (TRG 2.2 §8.9).
    #[param(flatten)]
    pub represented: RepresentedParams,
    /// The title and the file.
    #[param(flatten)]
    pub naming: NamingParams,
}

/// What `subrepresentation` takes.
#[derive(Debug, Inputs)]
pub struct SubrepresentationInputs {
    /// the supporting representation — an object of the package, whose elements are chosen
    #[input(types = "resqml22.IjkGridRepresentation, resqml22.TriangulatedSetRepresentation, resqml22.Grid2dRepresentation, resqml22.PointSetRepresentation, resqml22.PolylineSetRepresentation")]
    pub support: Shape,
}

/// The title the object behind a shape carries, or the shape's name.
fn title_of(shape: &Shape) -> String {
    shape.known.as_ref().and_then(|known| known.reference.as_ref()).map(|r| r.title.clone()).unwrap_or_else(|| shape.name.clone())
}

/// The one IJK patch of a support — its cells, `[ni, nj, nk]`, the RESQML
/// index of each cell drawn, and its points.
type IjkPatch<'a> = (&'a [[u32; 8]], [usize; 3], &'a [u32], &'a [[f64; 3]]);

/// The [`IjkPatch`] of a support, or why it has none.
fn ijk_of(shape: &Shape) -> Result<IjkPatch<'_>, Refusal> {
    match shape.geometry.patches.as_slice() {
        [Patch { topology: Topology::Hexahedra { cells, dimensions, index }, points, .. }] => {
            Ok((cells, *dimensions, index, points))
        }
        [Patch { topology: Topology::Polyhedra(_), .. }] => Err(Refusal::on(
            "support",
            format!(
                "`{}` is an unstructured grid: its cells are not selected here yet — nothing in this build draws a sub-representation of them",
                title_of(shape)
            ),
        )),
        _ => Err(Refusal::on(
            "support",
            format!("`{}` holds no IJK grid, and cells are chosen in one", title_of(shape)),
        )),
    }
}

/// How many nodes a support holds: a 2D grid its declared lattice, anything
/// else the points it places.
fn node_count(shape: &Shape) -> Result<usize, Refusal> {
    let mut total = 0usize;
    for patch in &shape.geometry.patches {
        if let Topology::Grid { fastest, slowest } = &patch.topology {
            if patch.points.len() != fastest * slowest {
                return Err(Refusal::on(
                    "support",
                    format!(
                        "`{}` declares {fastest} × {slowest} nodes and holds {}: its node numbering is not the file's",
                        title_of(shape),
                        patch.points.len()
                    ),
                ));
            }
        }
        total += patch.points.len();
    }
    Ok(total)
}

/// The triangles of a support across its patches, each as its three corners.
fn triangles_of(shape: &Shape) -> Vec<[[f64; 3]; 3]> {
    let mut out = Vec::new();
    for patch in &shape.geometry.patches {
        if let Topology::Triangles(triangles) = &patch.topology {
            out.extend(triangles.iter().map(|t| t.map(|n| patch.points[n as usize])));
        }
    }
    out
}

impl Operation for Subrepresentation {
    type Params = SubrepresentationParams;
    type Inputs = SubrepresentationInputs;

    fn about(&self) -> About {
        About {
            name: "subrepresentation",
            describe: "a sub-representation of one representation of the package — its nodes, triangles or cells, given by index or as an IJK box",
            category: Category::Create,
            makes: &["resqml22.SubRepresentation"],
            recipe: "subrepresentation",
            represents: None,
        }
    }

    fn check(&self, inputs: &SubrepresentationInputs) -> Result<(), Refusal> {
        match inputs.support.known.as_ref().and_then(|known| known.reference.as_ref()) {
            Some(_) => Ok(()),
            None => Err(Refusal::on(
                "support",
                format!(
                    "`{}` is no object of the package: a sub-representation names its support by uuid, and an artefact has none",
                    inputs.support.name
                ),
            )),
        }
    }

    fn plan(&self, inputs: SubrepresentationInputs, params: SubrepresentationParams, cx: &Context<'_>) -> Result<Plan, Refusal> {
        self.check(&inputs)?;
        let SubrepresentationInputs { support: shape } = inputs;
        let Some(reference) = shape.known.as_ref().and_then(|known| known.reference.clone()) else {
            return Err(Refusal::on("support", "names no object of the package"));
        };

        // -- the elements ---------------------------------------------------------------
        let (element, indices, rule) = match &params.select {
            Select::Indices(given) => {
                let indices: Vec<u64> = given.indices.iter().map(|one| *one as u64).collect();
                (given.element, indices, "given".to_string())
            }
            Select::IjkBox(ijk) => {
                let (_, dimensions, _, _) = ijk_of(&shape)?;
                let range = |first: Option<usize>, last: Option<usize>, axis: usize| match (first, last) {
                    (None, None) => None,
                    (first, last) => Some([first.unwrap_or(0), last.unwrap_or(dimensions[axis].saturating_sub(1))]),
                };
                let ranges = [range(ijk.i_first, ijk.i_last, 0), range(ijk.j_first, ijk.j_last, 1), range(ijk.k_first, ijk.k_last, 2)];
                let indices = cells::ijk_box(dimensions, ranges).map_err(|why| {
                    let slot = match &why {
                        cells::CellsError::Reversed { axis, .. } | cells::CellsError::Beyond { axis, .. } => format!("{axis}Last"),
                        _ => "select".to_string(),
                    };
                    Refusal::on(&slot, format!("{why} — `{}` is {} × {} × {} cells", title_of(&shape), dimensions[0], dimensions[1], dimensions[2]))
                })?;
                let said = |axis: usize, r: Option<[usize; 2]>| match r {
                    Some([a, b]) => format!("{} {a}–{b}", ['I', 'J', 'K'][axis]),
                    None => format!("every {}", ['I', 'J', 'K'][axis]),
                };
                let rule = format!("the IJK box {}, {}, {}", said(0, ranges[0]), said(1, ranges[1]), said(2, ranges[2]));
                (Element::Cells, indices, rule)
            }
        };

        let (available, name) = match element {
            Element::Nodes => (node_count(&shape)?, "nodes"),
            Element::Triangles => {
                let count = triangles_of(&shape).len();
                if count == 0 {
                    return Err(Refusal::on("element", format!("`{}` has no triangle to choose", title_of(&shape))));
                }
                (count, "triangles")
            }
            Element::Cells => {
                let (_, [ni, nj, nk], _, _) = ijk_of(&shape)?;
                (ni * nj * nk, "cells")
            }
        };
        let slot = match params.select {
            Select::Indices(_) => "indices",
            Select::IjkBox(_) => "select",
        };
        if let Some(bad) = indices.iter().find(|one| **one as usize >= available) {
            return Err(Refusal::on(
                slot,
                format!(
                    "{bad} is no index of `{}`: it has {available} {name}, from 0 to {}",
                    title_of(&shape),
                    available.saturating_sub(1)
                ),
            ));
        }
        let mut seen: BTreeMap<u64, usize> = BTreeMap::new();
        for (at, one) in indices.iter().enumerate() {
            if let Some(first) = seen.insert(*one, at) {
                return Err(Refusal::on(
                    slot,
                    format!(
                        "{one} is given at places {} and {}: two elements of the sub-representation would be one of `{}`, and a property on it would give that one two values",
                        first + 1,
                        at + 1,
                        title_of(&shape)
                    ),
                ));
            }
        }

        // -- the shape: what is chosen, drawn ---------------------------------------------
        let mut undefined = 0usize;
        let geometry = match element {
            Element::Nodes => {
                let points: Vec<[f64; 3]> = shape.geometry.patches.iter().flat_map(|patch| patch.points.iter().copied()).collect();
                support::one_patch("SubRepresentation", indices.iter().map(|one| points[*one as usize]).collect(), Topology::Points)
            }
            Element::Triangles => {
                let all = triangles_of(&shape);
                let mut points = Vec::with_capacity(indices.len() * 3);
                let mut chosen = Vec::with_capacity(indices.len());
                for one in &indices {
                    let base = points.len() as u32;
                    points.extend(all[*one as usize]);
                    chosen.push([base, base + 1, base + 2]);
                }
                support::one_patch("SubRepresentation", points, Topology::Triangles(chosen))
            }
            Element::Cells => {
                let (cells_drawn, _, numbering, points) = ijk_of(&shape)?;
                let mut kept_points = Vec::new();
                let mut kept = Vec::new();
                for one in &indices {
                    let position = if numbering.is_empty() {
                        Some(*one as usize)
                    } else {
                        numbering.binary_search(&(*one as u32)).ok()
                    };
                    let Some(cell) = position.and_then(|at| cells_drawn.get(at)) else {
                        undefined += 1;
                        continue;
                    };
                    let base = kept_points.len() as u32;
                    kept_points.extend(cell.iter().map(|n| points[*n as usize]));
                    kept.push(std::array::from_fn(|corner| base + corner as u32));
                }
                let count = kept.len();
                support::one_patch("SubRepresentation", kept_points, Topology::Hexahedra { cells: kept, dimensions: [count, 1, 1], index: Vec::new() })
            }
        };

        // -- the object ---------------------------------------------------------------------
        let title = params.naming.title();
        let file = params.naming.data_file.clone();
        let seed = format!(
            "subrepresentation:{}:{name}:{}:{title}",
            reference.uuid.to_ascii_lowercase(),
            indices.iter().map(|one| one.to_string()).collect::<Vec<_>>().join(",")
        );
        let mut rank = 0usize;
        let uuid = support::next_uuid(cx.identity, &seed, &mut rank)?;
        let represented = support::represented_in(&params.represented, None, None, cx.identity, cx.when, &seed, &title, &mut rank)?;
        let path = format!("/resqml22/{uuid}/indices");
        let dimensions = vec![indices.len() as i64];
        let mut object = json!({
            "$type": "resqml22.SubRepresentation",
            "uuid": uuid,
            "schemaVersion": "2.2",
            "Citation": support::citation(cx.when, &title),
            "IndexableElement": name,
            "SubRepresentationPatch": [{
                "$type": "resqml22.SubRepresentationPatch",
                "Indices": {
                    "$type": "eml23.IntegerExternalArray",
                    "ArrayIntegerType": "arrayOfInt64LE",
                    "NullValue": -1,
                    "CountPerValue": 1,
                    "Values": support::external(&file, &path, &dimensions),
                },
                "SupportingRepresentation": reference.dor(),
            }],
        });
        if let Some(reference) = represented.reference {
            object["RepresentedObject"] = reference;
        }

        let mut says = vec![Note {
            key: "selected",
            detail: format!(
                "{} of the {available} {name} of `{}`, {rule}, in the order written",
                indices.len(),
                reference.title
            ),
        }];
        if undefined > 0 {
            says.push(Note {
                key: "undefined-cells",
                detail: format!(
                    "{undefined} chosen cell(s) have no geometry — `CellGeometryIsDefined` false on the support: in range, written, and nothing to draw"
                ),
            });
        }

        let mut objects = represented.objects;
        objects.push(NewObject {
            uuid: uuid.clone(),
            qualified_type: "resqml22.SubRepresentation".to_string(),
            role: "representation".to_string(),
            title: title.clone(),
            json: object,
        });
        Ok(Plan {
            media: Vec::new(),
            blocked: None,
            objects,
            datasets: vec![Dataset {
                file,
                path,
                dimensions,
                values: Numbers::I64(indices.iter().map(|one| *one as i64).collect()),
            }],
            activity: ActivityDraft {
                recipe: "subrepresentation",
                title: format!("subrepresentation → {title}"),
                seed,
                parameters: vec![
                    Parameter::Text { title: "indexableElement".to_string(), value: name.to_string() },
                    Parameter::Double { title: "elementCount".to_string(), value: indices.len() as f64, uom: None },
                    Parameter::Double { title: "supportCount".to_string(), value: available as f64, uom: None },
                    Parameter::Double { title: "undefinedCellCount".to_string(), value: undefined as f64, uom: None },
                ],
            },
            says,
            dropped: Vec::new(),
            shapes: vec![support::made_root(&title, &uuid, geometry)],
        })
    }

    fn reports(&self) -> Vec<Slot> {
        let count = |name: &'static str, about: &'static str| Slot::required(name, Need::Number { uom: None, least: None, most: None }, about);
        vec![
            Slot::required("indexableElement", Need::Text, "the element the indices index — nodes, triangles, cells"),
            count("elementCount", "the elements chosen"),
            count("supportCount", "the elements of that kind the support has"),
            count("undefinedCellCount", "chosen cells the support leaves without geometry"),
        ]
    }
}
