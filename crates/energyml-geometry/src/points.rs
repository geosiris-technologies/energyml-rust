//! Any `AbstractPoint3dArray`, resolved to coordinates.
//!
//! Six forms, and only two have the numbers in a file - see the table in the
//! crate documentation. The others compute them: from a lattice, from a
//! supporting surface's z, from lines, or from another representation's nodes
//! entirely. Anything that reaches the end unhandled is refused **by name**:
//! empty geometry looks exactly like a working program with nothing to draw,
//! and that is the bug this module refuses to ship.

use energyml_core::value::{FieldValue, Reflect};

use crate::error::{Error, Result};
use crate::parametric::Vertical;
use crate::source::ValueSource;
use crate::values::{self, scalar_f64, scalar_i64};

/// One point, in the representation's own coordinate system.
///
/// A plain `[f64; 3]`, so a slice of them is already the layout a graphics API
/// wants. Putting it in the local system rather than a projected one is
/// deliberate: reprojection is the caller's decision and needs the CRS, which
/// [`crate::crs`] hands over separately.
pub type Point = [f64; 3];

/// Every point of a point array.
///
/// `vertical` says which way the local z counts, and it is asked for rather
/// than assumed because **one form of array cannot be read without it**: a
/// vertical parametric line stores a depth where a coordinate goes, and the
/// standard makes the conversion depend on the CRS (`ParametricLineArray`,
/// TRG §3.8). Every other form here holds coordinates already and ignores it.
///
/// Required rather than optional on purpose. A default would be right for the
/// depth systems and silently mirror every well and every pillar of an
/// elevation system - which is exactly the defect this argument was added to
/// end, and it went unnoticed because each individual number looked reasonable.
pub fn read(
    array: &dyn Reflect,
    source: &dyn ValueSource,
    at: &str,
    vertical: Vertical,
) -> Result<Vec<Point>> {
    let name = array.type_info().name;

    // **Two** doubles per point, and it has to be asked before the branch
    // below: `Point2dExternalArray` ends in `ExternalArray` like everything
    // else, and read three at a time it does not fail - it produces two thirds
    // as many points, each made of one point's x and y and the next one's x,
    // with the following point's y taken for a depth. Every coordinate is a
    // real number from the file, in the right units, in the right order. What
    // comes out is a scatter of plausible points that are not where anything
    // is.
    //
    // The schema is explicit that this is a *2D* dataset - *"the 2 coordinates
    // are stored sequentially in HDF5, i.e. a multi-dimensional array of points
    // is stored as a 2 × … HDF5 array"* - and the corpus agrees: the geobody
    // boundary point set holds a (6, 2) dataset that was being read as 4 points.
    // z is 0: the array says nothing about depth, and 0 in the local system is
    // the only answer that does not invent one.
    if name.starts_with("Point2d") && (name.ends_with("ExternalArray") || name.ends_with("Hdf5Array")) {
        let coordinates = values::read_doubles(array, source, at)?;
        if coordinates.len() % 2 != 0 {
            return Err(Error::malformed(
                at,
                format!(
                    "{} coordinate(s) is not a whole number of 2D points",
                    coordinates.len()
                ),
            ));
        }
        return Ok(coordinates
            .as_chunks::<2>()
            .0
            .iter()
            .map(|c| [c[0], c[1], 0.0])
            .collect());
    }

    // The common case: three doubles per point in an external file.
    if name.ends_with("ExternalArray") || name.ends_with("Hdf5Array") {
        let coordinates = values::read_doubles(array, source, at)?;
        if coordinates.len() % 3 != 0 {
            return Err(Error::malformed(
                at,
                format!(
                    "{} coordinate(s) is not a whole number of points",
                    coordinates.len()
                ),
            ));
        }
        return Ok(coordinates
            .as_chunks::<3>()
            .0
            .iter()
            .map(|c| [c[0], c[1], c[2]])
            .collect());
    }

    // `Point3dFromRepresentationLatticeArray` also ends in `LatticeArray` and
    // is a different thing entirely - nodes picked out of *another*
    // representation. Letting it fall in here reported "a lattice with no
    // `Origin`", which blames the file for this module's mistake.
    if name.starts_with("Point3d") && name.ends_with("LatticeArray") && !name.contains("FromRepresentation") {
        return lattice(array, at);
    }

    if name.ends_with("ZValueArray") {
        return z_values(array, source, at, vertical);
    }

    // Points that live on lines rather than in a file.
    if name.ends_with("ParametricArray") {
        return crate::parametric::read(array, source, at, vertical);
    }

    // Points that live in **another representation**: a selection of its nodes,
    // named rather than copied. See [`crate::derived`] for why the selection
    // reads two ways and what settles which.
    if name == "Point3dFromRepresentationLatticeArray" {
        return crate::derived::read(array, source, at);
    }

    // Every form the corpus contains is handled above. What reaches here is a
    // type this build does not know, and it is named rather than guessed at.
    Err(Error::unsupported(name, at))
}

/// How many points, when it can be told without evaluating them.
pub fn count(array: &dyn Reflect, source: &dyn ValueSource, at: &str) -> Result<usize> {
    let name = array.type_info().name;
    if name.ends_with("ExternalArray") || name.ends_with("Hdf5Array") {
        // Same divisor as [`read`], for the same reason, and it has to move
        // with it: a count that disagrees with the points is worse than either
        // being wrong on its own, because the check that would have caught the
        // other one passes.
        let per_point = if name.starts_with("Point2d") { 2 } else { 3 };
        return Ok(values::count(array, source, at)? / per_point);
    }
    // The selection alone answers this, without resolving the support or
    // reading a coordinate - which for a horizon over a million-node survey is
    // the difference between a number and a minute.
    if name == "Point3dFromRepresentationLatticeArray" {
        return crate::derived::count(array, at);
    }
    // The last resort: evaluate and count. Which way z counts cannot change
    // how many points there are, so the reading passed here is immaterial -
    // said out loud rather than left as a puzzle at the call site.
    Ok(read(array, source, at, Vertical::Down)?.len())
}

/// `Point3dLatticeArray`: an origin, and one direction per dimension.
///
/// The schema spells out the rule and it is easy to get wrong: *"A lattice of N
/// offset points is described by a spacing array of size N−1. […] the first
/// offset is 0. The second offset is the first spacing × offset. The third
/// offset is (first spacing + second spacing) × offset"*. So the spacings are
/// the gaps **between** points, the running sum is what multiplies the
/// direction vector, and a dimension with n spacings yields n+1 points.
///
/// # The order of the points
///
/// The dimension list runs **slowest axis first**, and the points come out in
/// the order the axes imply - the last one varying fastest, matching the
/// `FastestAxisCount` × `SlowestAxisCount` declared next to it. Both versions
/// in the corpus say so plainly: a 50 × 100 grid lists the 99-spacing dimension
/// before the 49-spacing one. Getting this backwards costs nothing in the point
/// count and transposes the surface, which no assertion downstream catches.
///
/// # Two spellings
///
/// RESQML 2.0.1 calls a dimension `Offset` and its vector `Offset` again; 2.2
/// renamed them `Dimension` and `Direction`. Same structure, so both are
/// accepted and the rest of the module never learns which version it has.
fn lattice(array: &dyn Reflect, at: &str) -> Result<Vec<Point>> {
    let origin = point_of(array, "Origin")
        .ok_or_else(|| Error::malformed(at, "a lattice with no `Origin` describes nothing"))?;
    let dimensions = match array.field("Offset").or_else(|| array.field("Dimension")) {
        Some(FieldValue::List(list)) => list,
        // A single dimension may arrive unwrapped.
        Some(FieldValue::Object(one)) => return lattice_along(origin, &[one], at),
        _ => return Ok(vec![origin]),
    };
    let dimensions: Vec<&dyn Reflect> = dimensions.iter().filter_map(|item| item.as_object()).collect();
    lattice_along(origin, &dimensions, at)
}

fn lattice_along(origin: Point, dimensions: &[&dyn Reflect], at: &str) -> Result<Vec<Point>> {
    let mut points = vec![origin];
    for offset in dimensions {
        let offset = *offset;
        let mut direction = point_of(offset, "Offset")
            .or_else(|| point_of(offset, "Direction"))
            .ok_or_else(|| Error::malformed(at, "a lattice dimension with no direction vector"))?;
        let spacings = spacings_of(offset, at)?;

        // Running sum, starting at zero for the point already there.
        let mut distances = vec![0.0];
        let mut total = 0.0;
        for spacing in &spacings {
            total += spacing;
            distances.push(total);
        }

        // The schema's rule is `spacing × offset vector`, with no word about
        // normalisation, and fesapi writes it that way: a **unit** direction
        // and metric spacings. SISMAGE writes something else - the direction is
        // the **whole axis extent** and the spacings are metres along it.
        // Applied literally, a 10.6 km axis steps 132 km at a time and the
        // surface lands nowhere, every coordinate looking like a coordinate.
        //
        // The two conventions are told apart without guessing, because the
        // SISMAGE form carries its own checksum: the summed spacings equal the
        // direction's length (2e-12 relative, measured on `ALWYN-RESQML.epc`
        // 8d3c3099). A conformant file cannot satisfy that identity by
        // accident - its direction has length 1 while its spacings sum to the
        // axis extent in metres. A direction far from unit whose length equals
        // the summed spacings is the writer stating "metric spacings along a
        // direction", and is read as such.
        let length =
            (direction[0] * direction[0] + direction[1] * direction[1] + direction[2] * direction[2]).sqrt();
        if (length - 1.0).abs() > 1e-3 && total > 0.0 && (total / length - 1.0).abs() < 1e-6 {
            direction = [
                direction[0] / length,
                direction[1] / length,
                direction[2] / length,
            ];
        }

        // Each dimension multiplies the set - that is what makes it a lattice -
        // and the dimension being added becomes the innermost one, so the last
        // offset in the file ends up varying fastest.
        let outer = std::mem::take(&mut points);
        points.reserve(outer.len() * distances.len());
        for point in &outer {
            for distance in &distances {
                points.push([
                    point[0] + direction[0] * distance,
                    point[1] + direction[1] * distance,
                    point[2] + direction[2] * distance,
                ]);
            }
        }
    }
    Ok(points)
}

/// The spacings of one lattice dimension.
///
/// `Spacing` is itself an `AbstractDoubleArray`, so it can be a constant array -
/// which is how a regular grid says "n steps of the same size" - or a real
/// array for an irregular one.
fn spacings_of(offset: &dyn Reflect, at: &str) -> Result<Vec<f64>> {
    let Some(FieldValue::Object(spacing)) = offset.field("Spacing") else {
        return Ok(Vec::new());
    };
    let name = spacing.type_info().name;
    if name.ends_with("ConstantArray") {
        let value =
            scalar_f64(spacing, "Value").ok_or_else(|| Error::malformed(at, "a spacing with no `Value`"))?;
        let count =
            scalar_i64(spacing, "Count").ok_or_else(|| Error::malformed(at, "a spacing with no `Count`"))?;
        return Ok(vec![value; count.max(0) as usize]);
    }
    if name.ends_with("XmlArray") || name.ends_with("LatticeArray") {
        // No external read needed, and none available here: a spacing that
        // lives in a file is refused below rather than half-evaluated.
        return values::read_doubles(spacing, &energyml_domain::source::NoObjects, at);
    }
    Err(Error::unsupported(name, format!("{at}.Spacing")))
}

/// `Point3dZValueArray`: x and y from a supporting geometry, z from an array.
///
/// The supporting geometry is itself a point array - in practice a lattice -
/// so this recurses, and a 2D grid whose z comes from HDF5 works without the
/// caller knowing the shape of any of it.
fn z_values(
    array: &dyn Reflect,
    source: &dyn ValueSource,
    at: &str,
    vertical: Vertical,
) -> Result<Vec<Point>> {
    let Some(FieldValue::Object(supporting)) = array.field("SupportingGeometry") else {
        return Err(Error::malformed(
            at,
            "a z-value array with no supporting geometry",
        ));
    };
    // The support is part of the same geometry, so it is in the same system.
    let mut points = read(supporting, source, &format!("{at}.SupportingGeometry"), vertical)?;

    let Some(FieldValue::Object(z)) = array.field("ZValues") else {
        return Err(Error::malformed(at, "a z-value array with no `ZValues`"));
    };
    let z_shape = shape_of(z, source);
    let z = values::read_doubles(z, source, &format!("{at}.ZValues"))?;

    // Which lattice axis is the outer one? fesapi and SISMAGE list the slowest
    // axis first; SKUA-GOCAD lists the **fastest** first. The z dataset is the
    // ground truth - its own shape says which order the values were written
    // in - and when it contradicts the lattice's order unambiguously (two
    // distinct axis lengths that match crosswise and not straight), the
    // support is transposed to the dataset's order **before** the z values
    // are laid on. Transposing afterwards would keep every z on the wrong
    // point. `Volve_…_originEQN.epc` d9d77341 is the reference case: a
    // (399, 736) dataset under a lattice written 736-axis first.
    let mut dims = lattice_dims(supporting);
    if let (Some(d), Some(shape)) = (&mut dims, &z_shape)
        && d.len() == 2
        && shape.len() == 2
        && d.iter().product::<usize>() == points.len()
    {
        // The lattice may also carry Gocad's off-by-one (handled below), so an
        // axis matches a dataset dimension at either n or n + 1.
        let fits = |axis: usize, extent: usize| axis == extent || axis == extent + 1;
        let straight = fits(d[0], shape[0]) && fits(d[1], shape[1]);
        let crossed = fits(d[0], shape[1]) && fits(d[1], shape[0]);
        if crossed && !straight {
            points = transpose(points, d);
            d.swap(0, 1);
        }
    }

    if z.len() != points.len() {
        // SKUA-GOCAD writes `Spacing.Count` as the number of **points** on the
        // axis where the schema demands the number of intervals - "A lattice
        // of N offset points is described by a spacing array of size N−1" -
        // so its lattices come out one point too large per axis, and the z
        // array, sized for the true grid, no longer fits. The mistake is
        // provable, not guessed: it holds exactly when the z count equals the
        // product of (points − 1) over every axis. Same reference case:
        // 736 × 399 z values under a 737 × 400 lattice.
        match &dims {
            Some(dims)
                if dims.iter().product::<usize>() == points.len()
                    && dims.iter().all(|d| *d > 1)
                    && dims.iter().map(|d| d - 1).product::<usize>() == z.len() =>
            {
                points = drop_last_per_axis(points, dims);
            }
            _ => {
                return Err(Error::malformed(
                    at,
                    format!(
                        "{} z value(s) for {} point(s) of supporting geometry",
                        z.len(),
                        points.len()
                    ),
                ));
            }
        }
    }
    for (point, z) in points.iter_mut().zip(z) {
        point[2] = z;
    }
    Ok(points)
}

/// The stored shape of an array's external data, when it has one.
fn shape_of(array: &dyn Reflect, source: &dyn ValueSource) -> Option<Vec<usize>> {
    let locations = source.locate(array);
    let [location] = locations.as_slice() else {
        return None;
    };
    let metadata = source.metadata(location).ok()?;
    Some(metadata.dimensions.iter().map(|d| (*d).max(0) as usize).collect())
}

/// Reorder a row-major 2-D lattice so the inner axis becomes the outer one.
pub(crate) fn transpose(points: Vec<Point>, dims: &[usize]) -> Vec<Point> {
    let (outer, inner) = (dims[0], dims[1]);
    let mut out = Vec::with_capacity(points.len());
    for j in 0..inner {
        for i in 0..outer {
            out.push(points[i * inner + j]);
        }
    }
    out
}

/// The per-axis point counts a lattice array would evaluate to, outermost
/// (slowest) first. `None` when the array is not a lattice.
///
/// This is `spacings + 1` per dimension - the counts do not depend on the
/// direction-vector convention, so no evaluation is needed to know them.
pub(crate) fn lattice_dims(array: &dyn Reflect) -> Option<Vec<usize>> {
    let name = array.type_info().name;
    // A borrowed lattice is laid out on a grid too - its selection has one axis
    // per dimension, exactly like a stored one - and the caller's reason for
    // asking is the same: to notice when the z dataset was written with the
    // axes the other way round. Leaving it out meant the transpose check was
    // silently off for every horizon picked on a survey, which is the shape
    // most likely to meet a producer that disagrees about axis order.
    if name == "Point3dFromRepresentationLatticeArray" {
        return crate::derived::selection_dims(array).ok();
    }
    if !(name.starts_with("Point3d") && name.ends_with("LatticeArray")) || name.contains("FromRepresentation")
    {
        return None;
    }
    let dimensions = match array.field("Offset").or_else(|| array.field("Dimension"))? {
        FieldValue::List(list) => list
            .iter()
            .filter_map(|item| item.as_object())
            .collect::<Vec<_>>(),
        FieldValue::Object(one) => vec![one],
        _ => return None,
    };
    let mut dims = Vec::with_capacity(dimensions.len());
    for offset in dimensions {
        dims.push(spacings_of(offset, "lattice").ok()?.len() + 1);
    }
    Some(dims)
}

/// Drop the last point of every axis of a row-major lattice.
///
/// The reconciliation for the off-by-one above: `dims` are the counts the
/// points were generated with, outermost first, and what comes back is the
/// same lattice at `dims − 1` per axis, order preserved.
pub(crate) fn drop_last_per_axis(points: Vec<Point>, dims: &[usize]) -> Vec<Point> {
    let mut keep = Vec::with_capacity(dims.iter().map(|d| d - 1).product());
    // The index in each dimension, innermost varying fastest.
    let mut index = vec![0usize; dims.len()];
    for point in points {
        if index.iter().zip(dims).all(|(i, d)| i + 1 < *d) {
            keep.push(point);
        }
        // Increment the multi-dimensional counter.
        for axis in (0..index.len()).rev() {
            index[axis] += 1;
            if index[axis] < dims[axis] {
                break;
            }
            index[axis] = 0;
        }
    }
    keep
}

/// A `Point3d` field, as three coordinates.
fn point_of(object: &dyn Reflect, field: &str) -> Option<Point> {
    let FieldValue::Object(point) = object.field(field)? else {
        return None;
    };
    Some([
        scalar_f64(point, "Coordinate1")?,
        scalar_f64(point, "Coordinate2")?,
        scalar_f64(point, "Coordinate3")?,
    ])
}
