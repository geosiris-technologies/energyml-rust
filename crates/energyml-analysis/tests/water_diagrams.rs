//! Piper and Stiff diagrams.
//!
//! The textbook points: a water of one salt falls on the corners its ions
//! name (Piper 1944 — a ternary share of 100 % is a corner by the definition
//! of a ternary diagram). The corners are computed here from the layout the
//! module doc states — two unit equilateral triangles `GAP` apart on one base
//! line — not read from `Piper::frame`, which is checked against them.
//!
//! The milliequivalents are recomputed by hand from the IUPAC standard atomic
//! weights (abridged, CIAAW 2021) written in this file: a second source for
//! the table `ion_balance` holds.

use energyml_analysis::ion_balance::BalanceError;
use energyml_analysis::water_diagrams::{Piper, WaterError, major_ions, piper, stiff};

const GAP: f64 = 0.2;
const H: f64 = 0.866_025_403_784_438_6; // √3 / 2

// Molar masses, g/mol, from the atomic weights: Na 22.990, K 39.098,
// Ca 40.078, Mg 24.305, Cl 35.45, S 32.06, O 15.999, C 12.011, H 1.008.
const NA: f64 = 22.990;
const K: f64 = 39.098;
const CA: f64 = 40.078;
const MG: f64 = 24.305;
const CL: f64 = 35.45;
const SO4: f64 = 32.06 + 4.0 * 15.999;
const HCO3: f64 = 1.008 + 12.011 + 3.0 * 15.999;

fn near(a: [f64; 2], b: [f64; 2]) -> bool {
    (a[0] - b[0]).abs() < 1e-9 && (a[1] - b[1]).abs() < 1e-9
}

/// Where the line from `a` in direction `u` meets the line from `b` in
/// direction `v` — to find the diamond's corners from the triangles'.
fn meet(a: [f64; 2], u: [f64; 2], b: [f64; 2], v: [f64; 2]) -> [f64; 2] {
    // a + s·u = b + t·v, solved for s by Cramer's rule.
    let det = u[0] * (-v[1]) - (-v[0]) * u[1];
    let s = ((b[0] - a[0]) * (-v[1]) - (-v[0]) * (b[1] - a[1])) / det;
    [a[0] + s * u[0], a[1] + s * u[1]]
}

/// The layout, by its corners.
struct Corners {
    ca: [f64; 2],
    na_k: [f64; 2],
    mg: [f64; 2],
    hco3: [f64; 2],
    cl: [f64; 2],
    so4: [f64; 2],
    /// Lines of constant Na+K climb parallel to the Ca–Mg side; lines of
    /// constant HCO3+CO3 parallel to the Cl–SO4 side.
    up_right: [f64; 2],
    up_left: [f64; 2],
}

fn corners() -> Corners {
    Corners {
        ca: [0.0, 0.0],
        na_k: [1.0, 0.0],
        mg: [0.5, H],
        hco3: [1.0 + GAP, 0.0],
        cl: [2.0 + GAP, 0.0],
        so4: [1.5 + GAP, H],
        up_right: [0.5, H],
        up_left: [-0.5, H],
    }
}

#[test]
fn a_sodium_chloride_water_falls_on_the_na_k_and_cl_corners() {
    // One millimole of NaCl per litre; the other axis ions measured at 0.
    // Diamond: Na+K = 100 % and HCO3+CO3 = 0 % — the line of Na+K = 100 %
    // from the Na+K corner meets the line of HCO3+CO3 = 0 % from the Cl
    // corner: the diamond's right corner, the sodium chloride facies.
    // To break: swap Na+K and Mg in the cation point, or the two terms of
    // the diamond's x — the corners stop matching.
    let water = [
        ("Na+", NA),
        ("K+", 0.0),
        ("Ca+2", 0.0),
        ("Mg+2", 0.0),
        ("Cl-", CL),
        ("SO4-2", 0.0),
        ("HCO3-", 0.0),
        ("CO3-2", 0.0),
    ];
    let found = piper(&water, 1.0, &[]).unwrap();
    assert!(
        (found.cations[2] - 100.0).abs() < 1e-9 && found.cations[0] == 0.0 && found.cations[1] == 0.0,
        "{:?}",
        found.cations
    );
    assert!(
        (found.anions[0] - 100.0).abs() < 1e-9 && found.anions[1] == 0.0 && found.anions[2] == 0.0,
        "{:?}",
        found.anions
    );
    let c = corners();
    let points = found.points(GAP).unwrap();
    assert!(near(points.cation, c.na_k), "{:?}", points.cation);
    assert!(near(points.anion, c.cl), "{:?}", points.anion);
    let right = meet(c.na_k, c.up_right, c.cl, c.up_left);
    assert!(
        near(points.diamond, right),
        "{:?} against {right:?}",
        points.diamond
    );
}

#[test]
fn a_calcium_bicarbonate_water_falls_on_the_ca_and_hco3_corners() {
    // Ca(HCO3)2, one millimole: one Ca+2, two HCO3-. The other axis ions are
    // declared absent — and the declaration is carried with the result.
    // Diamond: Na+K = 0 %, HCO3+CO3 = 100 % — the line from the Ca corner
    // along the Ca–Mg side meets the one from the HCO3 corner: the left
    // corner, the calcium bicarbonate facies.
    let water = [("Ca+2", CA), ("HCO3-", 2.0 * HCO3)];
    let absent = ["Mg+2", "Na+", "K+", "Cl-", "SO4-2", "CO3-2"];
    let found = piper(&water, 1.0, &absent).unwrap();
    assert!((found.cations[0] - 100.0).abs() < 1e-9, "{:?}", found.cations);
    assert!((found.anions[2] - 100.0).abs() < 1e-9, "{:?}", found.anions);
    assert_eq!(found.ions.declared_absent, absent);
    let c = corners();
    let points = found.points(GAP).unwrap();
    assert!(near(points.cation, c.ca), "{:?}", points.cation);
    assert!(near(points.anion, c.hco3), "{:?}", points.anion);
    let left = meet(c.ca, c.up_right, c.hco3, c.up_left);
    assert!(
        near(points.diamond, left),
        "{:?} against {left:?}",
        points.diamond
    );
}

#[test]
fn the_frame_is_the_layout_the_module_states() {
    let c = corners();
    let frame = Piper::frame(GAP).unwrap();
    assert_eq!(frame.cation, [c.ca, c.na_k, c.mg]);
    assert_eq!(frame.anion, [c.hco3, c.cl, c.so4]);
    // Bottom, right, top, left.
    let bottom = meet(c.na_k, c.up_right, c.hco3, c.up_left);
    let right = meet(c.na_k, c.up_right, c.cl, c.up_left);
    let top = meet(c.ca, c.up_right, c.cl, c.up_left);
    let left = meet(c.ca, c.up_right, c.hco3, c.up_left);
    for (found, expected) in frame.diamond.iter().zip([bottom, right, top, left]) {
        assert!(near(*found, expected), "{found:?} against {expected:?}");
    }
    assert!(matches!(Piper::frame(-0.1), Err(WaterError::BadGap { .. })));
}

/// Lutetia's water (the reference model's recipe, step `echantillon-gth-1`) —
/// values of a file, used for the arithmetic only: no carbonate is stated.
const CHALK: [(&str, f64); 8] = [
    ("Ca+2", 150.0),
    ("Mg+2", 15.0),
    ("Na+", 25.0),
    ("K+", 4.0),
    ("Ba+2", 0.05),
    ("HCO3-", 330.0),
    ("SO4-2", 150.0),
    ("Cl-", 40.0),
];

#[test]
fn each_ternary_adds_up_to_one_hundred_and_the_shares_are_those_of_the_milliequivalents() {
    let found = piper(&CHALK, 5.0, &["CO3-2"]).unwrap();
    assert!(
        (found.cations.iter().sum::<f64>() - 100.0).abs() < 1e-9,
        "{:?}",
        found.cations
    );
    assert!(
        (found.anions.iter().sum::<f64>() - 100.0).abs() < 1e-9,
        "{:?}",
        found.anions
    );
    // By hand: meq = mg/L ÷ M × |z|. Barium enters the balance, no axis.
    let (ca, mg, na_k) = (150.0 / CA * 2.0, 15.0 / MG * 2.0, 25.0 / NA + 4.0 / K);
    let total = ca + mg + na_k;
    for (found, expected) in found.cations.iter().zip([ca, mg, na_k]) {
        assert!(
            (found - 100.0 * expected / total).abs() < 1e-6,
            "{found} against {}",
            100.0 * expected / total
        );
    }
    let (cl, so4, hco3) = (40.0 / CL, 150.0 / SO4 * 2.0, 330.0 / HCO3);
    let total = cl + so4 + hco3;
    for (found, expected) in found.anions.iter().zip([cl, so4, hco3]) {
        assert!(
            (found - 100.0 * expected / total).abs() < 1e-6,
            "{found} against {}",
            100.0 * expected / total
        );
    }
}

#[test]
fn the_stiff_axes_are_na_k_cl_then_ca_hco3_then_mg_so4_in_milliequivalents() {
    // To break: swap two rows, or plot percentages instead of meq/L.
    let found = stiff(&CHALK, 5.0, &["CO3-2"]).unwrap();
    let labels: Vec<(&str, &str)> = found.axes.iter().map(|one| (one.cation, one.anion)).collect();
    assert_eq!(labels, [("Na+K", "Cl"), ("Ca", "HCO3+CO3"), ("Mg", "SO4")]);
    let expected = [
        (25.0 / NA + 4.0 / K, 40.0 / CL),
        (150.0 / CA * 2.0, 330.0 / HCO3),
        (15.0 / MG * 2.0, 150.0 / SO4 * 2.0),
    ];
    for (axis, (cation, anion)) in found.axes.iter().zip(expected) {
        assert!(
            (axis.cation_meq - cation).abs() < 1e-6,
            "{axis:?} against {cation}"
        );
        assert!((axis.anion_meq - anion).abs() < 1e-6, "{axis:?} against {anion}");
    }
}

#[test]
fn a_missing_ion_is_refused_never_drawn_as_zero() {
    // Lutetia states no carbonate: without the declaration, refused.
    // To break: let an absent ion count as 0 — both diagrams would draw.
    assert_eq!(
        piper(&CHALK, 5.0, &[]),
        Err(WaterError::Missing {
            formulas: vec!["CO3-2"]
        })
    );
    assert_eq!(
        stiff(&CHALK, 5.0, &[]),
        Err(WaterError::Missing {
            formulas: vec!["CO3-2"]
        })
    );
    // No magnesium stated (the water is rebalanced without it).
    let water = [
        ("Ca+2", CA),
        ("Na+", NA),
        ("K+", 0.0),
        ("Cl-", CL),
        ("HCO3-", 2.0 * HCO3),
        ("SO4-2", 0.0),
    ];
    assert_eq!(
        piper(&water, 1.0, &[]),
        Err(WaterError::Missing {
            formulas: vec!["Mg+2", "CO3-2"]
        })
    );
    assert!(piper(&water, 1.0, &["Mg+2", "CO3-2"]).is_ok());
}

#[test]
fn a_declaration_that_contradicts_the_analysis_is_refused() {
    assert!(matches!(
        piper(&CHALK, 5.0, &["CO3-2", "K+"]),
        Err(WaterError::DeclaredAndStated { given: 4.0, .. })
    ));
    assert!(matches!(
        piper(&CHALK, 5.0, &["CO3-2", "Ba+2"]),
        Err(WaterError::NotAnAxisIon { .. })
    ));
}

#[test]
fn an_analysis_the_balance_refuses_draws_nothing() {
    // 3 meq/L of Na+ against 1 of Cl-: +50 %, beyond 5 %.
    // To break: skip the tolerance — the diagrams would draw a non-water.
    let water = [
        ("Na+", 3.0 * NA),
        ("K+", 0.0),
        ("Ca+2", 0.0),
        ("Mg+2", 0.0),
        ("Cl-", CL),
        ("SO4-2", 0.0),
        ("HCO3-", 0.0),
        ("CO3-2", 0.0),
    ];
    match piper(&water, 5.0, &[]) {
        Err(WaterError::Unbalanced { error, tolerance, .. }) => {
            assert!((error - 50.0).abs() < 1e-9 && tolerance == 5.0)
        }
        other => panic!("refused as unbalanced, not {other:?}"),
    }
    assert!(
        piper(&water, 51.0, &[]).is_ok(),
        "within a tolerance the caller declared"
    );
    // What `ion_balance` refuses, the diagrams refuse with its reason.
    assert!(matches!(
        piper(&[("NO3-2", 10.0), ("Na+", 5.0)], 5.0, &[]),
        Err(WaterError::Balance(BalanceError::Unknown { .. }))
    ));
    assert!(matches!(
        stiff(&[("Na+", -1.0), ("Cl-", 5.0)], 5.0, &[]),
        Err(WaterError::Balance(BalanceError::BadConcentration { .. }))
    ));
    assert!(matches!(
        major_ions(&CHALK, f64::NAN, &["CO3-2"]),
        Err(WaterError::BadTolerance { .. })
    ));
}

#[test]
fn a_ternary_of_nothing_is_refused() {
    // Barium alone carries the cations: balanced, but no Piper cation axis
    // holds anything.
    let water = [
        ("Ba+2", 137.33),
        ("Ca+2", 0.0),
        ("Mg+2", 0.0),
        ("Na+", 0.0),
        ("K+", 0.0),
        ("Cl-", 2.0 * CL),
        ("SO4-2", 0.0),
        ("HCO3-", 0.0),
        ("CO3-2", 0.0),
    ];
    assert_eq!(
        piper(&water, 1.0, &[]),
        Err(WaterError::EmptyTernary { which: "cation" })
    );
}
