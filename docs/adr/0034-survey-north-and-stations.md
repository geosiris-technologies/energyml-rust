# 0034 - The north of a survey and its stations: what the file states, what the caller declares

Status: accepted

## Context

A directional survey is a list of stations (measured depth, inclination,
azimuth), joined by minimum curvature and placed from a datum. Three facts it
needs are left open by the standards, and a drawing made on a guess looks as
plausible as a drawing made on the truth:

- **The north of the azimuths.** WITSML 2.1 makes `AziRef` optional "because of
  legacy data" while strongly recommending it. Grid and true north differ by the
  convergence, several degrees in places: the bottom of a 3 000 m hole moves by
  tens of metres.
- **The corrections.** A survey in true or magnetic north states, in the file,
  the convergence and the declination its producer used. Whether to apply them,
  and in which order, is left to the reader.
- **The stations the producer rejected.** The guide says a rejected station does
  not influence the stations below it, but a drawing that silently drops measured
  stations shows a well the file does not quite describe.

RESQML 2.0.1 has the same question for its `DeviationSurveyRepresentation`, which
the library did not read at all.

Until now the library refused a survey it could not place, by name, and the
check of the north came after the placement of the datum, so a survey with no
north was reported as something else when its datum was also wrong.

## Decision

- **The north is checked before the datum is placed** (L1). The check needs only
  the stations read; its second half, "does this north sit in the frame of the
  datum", needs the placement and comes after. "The hole leaves the vertical" is
  read on the kept stations (L2): a station whose inclination has a sine above
  `min_curvature::VERTICAL`.
- **A north the file does not state is declared by the caller, never guessed**
  (S11). `read_declaring` takes a grid or plant north; the result carries it
  (`NorthSource::Declared`), nothing is written into the package, and a
  declaration never replaces a north the file states (`NorthAlreadyStated`).
  Without a declaration, a survey that leaves the vertical is refused
  (`NorthNotStated`). Registered as `house/survey/declared-north`.
- **The corrections the file states are applied** (S12, L3). True north to grid
  north: the convergence is subtracted. Magnetic north to grid north: the
  declination is added, then the convergence subtracted. A station's own value
  (`StnGridConUsed`, `StnMagDeclUsed`) comes before the trajectory's
  (`GridConUsed`, `MagDeclUsed`). The result is a grid-north azimuth, placed as a
  survey stated in grid north. Quote, WITSML TRG 2.1 section 16.7 p. 352: "To
  convert a true north referenced azimuth to a grid north azimuth, the
  convergence angle must be subtracted from true north"; p. 353: "To convert a
  Magnetic azimuth to a True North azimuth, the magnetic declination should be
  added". A correction the file does not state is refused by name
  (`NorthNotApplied`); the library takes none from elsewhere (a geodetic library,
  a magnetic model) because that would be a second source the file does not name.
  Registered as `house/survey/north-corrections-applied`.
- **Astronomic and compass north, or a value outside the list, stay refused**
  (L4): the file has no field that corrects them.
- **A rejected station is left out only when the caller declares so** (S13, L7).
  By default the survey is refused (`RejectedStation`). Left out, the positions
  run over the other stations and the result lists the stations kept
  (WITSML TRG 2.1 section 16.12 p. 363: a rejected station "does not influence
  position computation for stations below it"). A refusal names a station by its
  **number in the file**, from 1, even when stations were left out (L8).
  Registered as `house/survey/rejected-station-left-out-on-declaration`.
- **A RESQML 2.0.1 deviation survey is read** (S14, L5, L6). Its azimuths are
  relative to the projected CRS north by its schema, even when the local CRS is
  rotated; its inclinations are measured from the vertical; no method is stated,
  so the stations are joined by minimum curvature. `FirstStationLocation` is read
  and shown, never used to place, because the schema does not say in which CRS it
  is: the survey is placed from its `MdDatum`, and refused by name
  (`GridWithoutProjected`) where that datum names no projected CRS to turn from.
  Registered as `house/survey/deviation-survey-grid-north`. The survey is entered
  in the registry of readers, so that `geometry::of`, the scene and the exports
  say the named refusal instead of "unsupported".
- **Values the survey declares never make the reading fail** (L9). Station
  values (`Tvd`, `DispNs`, `DispEw`, `VertSect`, `Dls`, `RateBuild`, `RateTurn`)
  are read for comparison; a missing or unknown unit is a fault on that field
  alone. `RateBuild` and `RateTurn` are read and never compared: the schema ties
  them to the radius of curvature computation, another method than the one
  recomputed (L10). `DispNs` and `DispEw` are not compared when
  `GridScaleFactorUsed` is present, since the table of units has no length per
  length family (L11).
- **The points a survey is placed with are in the frame and the units of the CRS
  that `crs::of` gives it** (L12, Q12). `crs.to_metres(points[k])` equals the
  placement of the local position of the kept station `k`; without a CRS, the
  points are in metres. The conversions go through the existing system
  (`units::metres`, `Crs::to_metres`, `Crs::from_projected`); there is no private
  table and no factor written by hand. A survey in feet, or a datum that is a
  `ReferencePointInACrs`, is no longer drawn with its units mixed.

## Consequences

- A survey that states no north draws nothing until the caller says which north
  to use, and the drawing says it was declared. A file that states its north is
  never overruled.
- A true or magnetic north survey lands where the same survey in grid north
  does. A test can compare the two: the same fact from two sources.
- The rejected stations are a choice the caller makes and the result reports; the
  default is the refusal of today.
- The four rules are declared, not checked: `stations` applies them, and the
  breach is a named refusal of the reading, which `diagnose` lists. No entry in
  `integrity::CHECKS`.
- Not decided here: the stations whose status is `open`, which are treated as
  before.
