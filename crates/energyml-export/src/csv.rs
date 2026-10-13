//! Long CSV — one row per (object, element, property), in metres.
//!
//! The tabular export: what a geologist pastes into a
//! spreadsheet or loads into pandas. **Long format, never wide** — one column
//! per property would mean aligning the elements of properties that do not
//! sample in the same places, hence choosing a common grid; refusing to align
//! and writing long is the honest option, and pivoting is a spreadsheet's job.
//!
//! Three decisions, stated here:
//!
//! - **A `NaN` value is an empty cell.** CSV has no convention for "no data";
//!   an empty cell is what every reader understands, where `NaN` becomes a
//!   string half of them refuse to sum. *Nodes* with no position are removed
//!   upstream by [`registry::prepare`], as in every format.
//! - **A cell value has empty coordinates.** A cell has not one position but
//!   eight; writing its centroid would be a computed value presented as data
//!   from the file. The `element` column says `cell`, the index says which one,
//!   and x/y/z stay empty.
//! - **An object with no property is written all the same**: one row per node,
//!   with empty property columns. Geometry alone is an answer, and an export
//!   that yields an empty file for a well with no log looks like a broken
//!   export.
//!
//! A property's header is its title, its kind and its unit in separate columns
//! — not `title (uom)` run together, which would force every consumer to
//! re-parse what this writer already knew apart.

use std::io::Write;

use energyml_geometry::{AttributeScope, Placement, SceneObject};

use crate::error::{Error, Result};
use crate::registry::{Exporter, Options};

/// Writes long-format CSV.
pub struct CsvExporter;

impl Exporter for CsvExporter {
    fn name(&self) -> &'static str {
        "csv"
    }

    fn extension(&self) -> &'static str {
        "csv"
    }

    fn mime(&self) -> &'static str {
        "text/csv"
    }

    fn describe(&self) -> &'static str {
        "long-format CSV: one row per element and attribute, placed and in metres"
    }

    fn placement(&self) -> Placement {
        Placement::Metres
    }

    fn handles(&self, object: &SceneObject) -> bool {
        !object.geometry.is_empty()
    }

    fn write(&self, object: &SceneObject, out: &mut dyn Write, options: &Options<'_>) -> Result<()> {
        if object.geometry.is_empty() {
            return Err(Error::unsuitable(
                "csv",
                format!("`{}`, which has no points", object.title),
            ));
        }
        self.write_all(std::slice::from_ref(object), out, options)
    }

    fn write_all(&self, objects: &[SceneObject], out: &mut dyn Write, options: &Options<'_>) -> Result<()> {
        // One header for the whole file — that is what makes a group export
        // pasteable as it stands: the well is a column, not a block.
        writeln!(out, "object,uuid,element,index,x,y,z,property,kind,uom,value")?;
        for object in objects {
            if !self.handles(object) {
                continue;
            }
            let object = crate::registry::prepare(object, options)?;
            let object = object.as_ref();
            let placement = options.placement.unwrap_or_else(|| self.placement());
            let points = options.points(object, placement)?;
            let name = field(&object.title);
            let uuid = field(&object.uuid);

            let of_scope = |scope: AttributeScope| {
                object
                    .attributes
                    .iter()
                    .filter(move |attribute| attribute.scope == scope)
            };

            // The nodes: the position always, then one row per value attached
            // to it — or a bare row when nothing is attached.
            for (index, point) in points.iter().enumerate() {
                let mut wrote = false;
                for attribute in of_scope(AttributeScope::Points) {
                    for component in 0..attribute.components as usize {
                        let value = attribute
                            .values
                            .get(index * attribute.components as usize + component)
                            .copied();
                        writeln!(
                            out,
                            "{name},{uuid},node,{index},{},{},{},{},{},{},{}",
                            point[0],
                            point[1],
                            point[2],
                            field(&component_name(&attribute.name, attribute.components, component)),
                            field(attribute.kind.as_deref().unwrap_or("")),
                            field(attribute.uom.as_deref().unwrap_or("")),
                            cell(value),
                        )?;
                        wrote = true;
                    }
                }
                if !wrote {
                    writeln!(
                        out,
                        "{name},{uuid},node,{index},{},{},{},,,,",
                        point[0], point[1], point[2]
                    )?;
                }
            }

            // The cells: no position — see the module — but every value, with
            // the index that places it back in the grid.
            for attribute in of_scope(AttributeScope::Cells) {
                let stride = attribute.components as usize;
                for index in 0..attribute.values.len() / stride.max(1) {
                    for component in 0..stride {
                        let value = attribute.values.get(index * stride + component).copied();
                        writeln!(
                            out,
                            "{name},{uuid},cell,{index},,,,{},{},{},{}",
                            field(&component_name(&attribute.name, attribute.components, component)),
                            field(attribute.kind.as_deref().unwrap_or("")),
                            field(attribute.uom.as_deref().unwrap_or("")),
                            cell(value),
                        )?;
                    }
                }
            }
        }
        Ok(())
    }
}

/// `Velocity[1]` for a vector's component, the bare name for a scalar.
fn component_name(name: &str, components: u64, component: usize) -> String {
    if components > 1 {
        format!("{name}[{component}]")
    } else {
        name.to_owned()
    }
}

/// A value: the number, or the empty cell for `NaN` and for the absent.
fn cell(value: Option<f64>) -> String {
    match value {
        Some(v) if v.is_finite() => format!("{v}"),
        // ±inf is a value the file states (xsd:double has them); losing it to
        // an empty cell would confuse it with "no data".
        Some(v) if v.is_infinite() => format!("{v}"),
        _ => String::new(),
    }
}

/// The CSV field: quoted as soon as it holds anything that would break the row.
fn field(raw: &str) -> String {
    if raw.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", raw.replace('"', "\"\""))
    } else {
        raw.to_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_field_with_a_comma_is_quoted_and_quotes_are_doubled() {
        assert_eq!(field("plain"), "plain");
        assert_eq!(field("a,b"), "\"a,b\"");
        assert_eq!(field("say \"hi\""), "\"say \"\"hi\"\"\"");
    }

    #[test]
    fn nan_is_an_empty_cell_and_infinity_is_not() {
        assert_eq!(cell(Some(1.5)), "1.5");
        assert_eq!(cell(Some(f64::NAN)), "");
        assert_eq!(cell(None), "");
        assert_eq!(cell(Some(f64::INFINITY)), "inf");
    }

    #[test]
    fn a_vector_component_is_named_and_a_scalar_is_not() {
        assert_eq!(component_name("Velocity", 3, 1), "Velocity[1]");
        assert_eq!(component_name("Porosity", 1, 0), "Porosity");
    }
}
