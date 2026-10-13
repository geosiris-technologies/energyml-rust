//! The IGN Géoplateforme altimetry — the elevation at a point, or along a line.
//!
//! This came out of a survey prompted by the *"TopoExport — Terrain 3D"* reuse:
//! that reuse rebuilds a digital terrain model from IGN's **RGE ALTI®** and is
//! migrating to **LiDAR HD**. The public service that carries both datasets is
//! measured here; the other sources that were ruled out are covered in the
//! terrain sources survey.
//!
//! Everything below was **measured on the service itself**, for lack of better
//! than a summary of the documentation (`cartes.gouv.fr`, since the exact pages
//! of `geoservices.ign.fr` redirect elsewhere). The figures are dated because
//! they will age, exactly as in `crate::brgm`.
//!
//! - **entry point**: `https://data.geopf.fr/altimetrie/1.0/calcul/alti/rest`,
//!   two routes, `elevation.{format}` and `elevationLine.{format}`;
//! - **no key**: a bare `GET` answers 200, and `access-control-allow-origin: *`
//!   is set on the answer *and* on the preflight — the front end could call it
//!   directly, like the BRGM WFS;
//! - **Licence Ouverte / Etalab 2.0**, announced by `cartes.gouv.fr`;
//! - **`GET /altimetrie/resources`** lists nine resources, two of which are used
//!   here: [`RGE_ALTI`] (`ign_rge_alti_wld`, what TopoExport consumes today) and
//!   [`LIDAR_HD_MNT`] (`ign_lidar_hd_mnt_mono_wld`, the announced migration);
//! - **`resource` is mandatory**: its absence returns a `400` (a refusal of the
//!   `"'None' is not an accepted resource"` kind), never a silent default;
//! - **several points are separated by `|`**, never by `,` — a comma returns
//!   `BAD_PARAMETER` (*"cannot be parsed as a valid longitude"*), and so does a
//!   `lon` list shorter than the `lat` one (*"The number of [lon] values must
//!   match the number of [lat] values"*);
//! - **`zonly=false` makes `lon`, `lat`, `z` and `acc` come back** in every
//!   element; `zonly=true` (or its absence) returns only a bare number per
//!   point — this module only ever asks for the complete form, so that a point
//!   can be rebuilt without asking the caller again;
//! - **`z = -99999` is an absence of coverage**, never a depth below sea level —
//!   documented by `cartes.gouv.fr`, confirmed by querying a point off the
//!   Canary Islands;
//! - **refusals are JSON, with a `400`** — unlike the BRGM WFS, which answers in
//!   XML. `{"error":{"code":"...","description":"..."}}`;
//! - **`5 requests per second`** per IP, documented;
//! - **at most `5000` points per request**, documented — not verified at the
//!   limit: refusing here avoids a request known to be doomed.
//!
//! # Why this is not a [`crate::Provider`]
//!
//! A [`crate::Provider`] of this crate offers collections that a
//! [`crate::query::Area`] **filters** — it is the `Intersects` search of a WFS:
//! a region is designated, the service returns what lies in it. This service
//! filters nothing: it is given **explicit** points, or an explicit line to
//! resample, and answers one value per point. Forcing it into `Query`/`Area`
//! would make a drawn area claim to be a profile trace — exactly what the
//! sources design refused by name (a polyline is not accepted as an area for
//! now), and what the map view cannot export today.
//!
//! This module therefore only holds what the crate's rule asks for in every
//! case — **build a request, read an answer, both pure** — for the day a
//! consumer wants a ground elevation: a scatter of points (a `feeds` in the
//! `crate::wfs` style towards `PointSetRepresentation`), or a profile along a
//! well trajectory. Neither `Providers::builtin()` nor the front end names it; adding
//! it to the registry would pretend that an *area + filters* form describes this
//! service, which it does not.

use crate::error::{Error, Result};
use crate::transport::{Answer, Request};
use crate::wfs::encode;

/// The entry point common to both routes.
pub const ENDPOINT: &str = "https://data.geopf.fr/altimetrie/1.0/calcul/alti/rest";

/// What the service documents as a ceiling, per request.
pub const MAX_POINTS: usize = 5000;

/// RGE ALTI®, for mainland France and overseas — what TopoExport consumes
/// today, measured through `GET /altimetrie/resources`.
pub const RGE_ALTI: &str = "ign_rge_alti_wld";

/// LiDAR HD, the interpolated single-echo digital terrain model — the migration
/// TopoExport announces, same list of resources.
pub const LIDAR_HD_MNT: &str = "ign_lidar_hd_mnt_mono_wld";

/// An elevation read for a point.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Elevation {
    /// The longitude of the point, in WGS 84 degrees — the one the service
    /// **returns**, not the one sent: it is what allows a point to be rebuilt
    /// without keeping the request beside it.
    pub lon: f64,
    /// The latitude, likewise.
    pub lat: f64,
    /// The elevation, in metres. `None` where the service answers `-99999`:
    /// **no data here**, never a measured depth.
    pub z: Option<f64>,
}

/// The elevation at each of the given points, in WGS 84 degrees.
///
/// `resource` is one of the resources declared by the service — [`RGE_ALTI`] or
/// [`LIDAR_HD_MNT`] here, but nothing prevents passing another: this module does
/// not guess the list, it documents it.
pub fn elevation_request(points: &[(f64, f64)], resource: &str) -> Result<Request> {
    check(points, 1)?;
    let (lon, lat) = coordinates(points);
    let url = format!(
        "{ENDPOINT}/elevation.json?lon={}&lat={}&resource={}&zonly=false",
        encode(&lon),
        encode(&lat),
        encode(resource)
    );
    Ok(Request {
        url,
        accept: "application/json".to_string(),
        about: format!("elevation at {} point(s) from `{resource}`", points.len()),
    })
}

/// The elevation profile along a line, resampled every `sampling` metres.
///
/// The line is a sequence of vertices, **open**: it is not an area, it is a
/// trace — see the module doc for why this does not go through
/// [`crate::query::Area`].
pub fn profile_request(points: &[(f64, f64)], resource: &str, sampling: u32) -> Result<Request> {
    check(points, 2)?;
    let (lon, lat) = coordinates(points);
    let url = format!(
        "{ENDPOINT}/elevationLine.json?lon={}&lat={}&resource={}&sampling={sampling}",
        encode(&lon),
        encode(&lat),
        encode(resource)
    );
    Ok(Request {
        url,
        accept: "application/json".to_string(),
        about: format!(
            "elevation profile over {} point(s) from `{resource}`",
            points.len()
        ),
    })
}

fn check(points: &[(f64, f64)], least: usize) -> Result<()> {
    if points.len() < least {
        return Err(Error::Refused(format!(
            "{} point(s) given, at least {least} are needed",
            points.len()
        )));
    }
    if points.len() > MAX_POINTS {
        return Err(Error::Refused(format!(
            "{} points asked, the service documents at most {MAX_POINTS} per request",
            points.len()
        )));
    }
    Ok(())
}

fn coordinates(points: &[(f64, f64)]) -> (String, String) {
    let lon = points
        .iter()
        .map(|(lon, _)| lon.to_string())
        .collect::<Vec<_>>()
        .join("|");
    let lat = points
        .iter()
        .map(|(_, lat)| lat.to_string())
        .collect::<Vec<_>>()
        .join("|");
    (lon, lat)
}

/// What a quote or a refusal is allowed to weigh — see `crate::wfs::SMALL`, same
/// reason: beyond that, it is no longer what one thinks one is reading.
const SMALL: usize = 64 * 1024;

/// Read an `elevation.json` or `elevationLine.json` answer — both carry the same
/// `"elevations"` key, the second adds `"height_differences"`, which is ignored
/// here: this module returns points, not a cumulated height difference.
///
/// **Pure**, and read whole rather than streamed: unlike a WFS harvest, an
/// answer from here is bounded by [`MAX_POINTS`], so it is never large.
pub fn read_elevations(answer: Answer) -> Result<Vec<Elevation>> {
    let status = answer.status;
    let text = answer.text(SMALL)?;
    let value: serde_json::Value = serde_json::from_str(&text).map_err(|why| {
        Error::Unreadable(format!(
            "not JSON: {why}: {}",
            text.chars().take(200).collect::<String>()
        ))
    })?;

    // **The refusal is JSON here, not XML** — unlike the BRGM WFS. Reading it
    // before looking for `elevations` avoids saying "no elevations here" of an
    // answer that explains perfectly well why.
    if let Some(error) = value.get("error") {
        let code = error.get("code").and_then(|v| v.as_str()).map(str::to_string);
        let says = error
            .get("description")
            .and_then(|v| v.as_str())
            .unwrap_or("the server refused, without a description")
            .to_string();
        return Err(Error::Server { status, code, says });
    }

    let elevations = value
        .get("elevations")
        .and_then(|v| v.as_array())
        .ok_or_else(|| {
            Error::Unreadable(format!(
                "this answer names no `elevations`: {}",
                text.chars().take(200).collect::<String>()
            ))
        })?;

    elevations.iter().map(read_one).collect()
}

fn read_one(value: &serde_json::Value) -> Result<Elevation> {
    let field = |name: &str| {
        value.get(name).and_then(|v| v.as_f64()).ok_or_else(|| {
            Error::Unreadable(format!("an element of `elevations` names no `{name}`: {value}"))
        })
    };
    let lon = field("lon")?;
    let lat = field("lat")?;
    let z = field("z")?;
    // `-99999` is what the service documents as an absence of coverage;
    // confirmed by querying a point off the Canary Islands.
    let z = if z <= -99998.0 { None } else { Some(z) };
    Ok(Elevation { lon, lat, z })
}
