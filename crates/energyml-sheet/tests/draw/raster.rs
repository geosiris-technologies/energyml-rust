//! `draw::raster`: value images. Each test states the rule it checks and what
//! would have to break for it to turn red. The expected values come from the
//! rule written in the design (samples cover half the way to their
//! neighbours, a gap stays a gap, a pixel belongs to a quad when its centre is
//! inside), never from a file.

use energyml_sheet::draw::raster::{
    ColumnMap, GridView, MAX_RASTER_PIXELS, MaxReduce, MeanReduce, MinReduce, NearestReduce, PaintCounts,
    Raster, RowMap, RowPick, RowReduce, RowReduces, columns_for, interval_covers, node_covers, paint_grid,
    paint_quads, rows_for,
};
use energyml_sheet::draw::svg::DrawError;

fn cover_of(positions: &[f64], cap: Option<f64>) -> Vec<Option<energyml_sheet::draw::raster::Cover>> {
    node_covers(positions, cap).unwrap()
}

/// Casser : prendre le pas médian partout au lieu de la moitié de l'écart à
/// chaque voisin.
#[test]
fn node_covers_reach_half_way_to_each_neighbour() {
    let covers = cover_of(&[0.0, 10.0, 30.0], None);
    let spans: Vec<(f64, f64, f64)> = covers
        .iter()
        .map(|c| {
            let c = c.unwrap();
            (c.from, c.at, c.to)
        })
        .collect();
    assert_eq!(
        spans,
        vec![(-5.0, 0.0, 5.0), (5.0, 10.0, 20.0), (20.0, 30.0, 40.0)]
    );
}

/// S7: a gap longer than the cap is never bridged. Casser : retirer le
/// plafond (seule la dernière ligne resterait vide).
#[test]
fn a_gap_stays_a_gap() {
    let covers = cover_of(&[0.0, 10.0, 20.0, 200.0, 210.0], Some(15.0));
    let map = rows_for(&covers, [0.0, 220.0], 44).unwrap();
    assert!(
        map.picks[7..37].iter().all(|p| *p == RowPick::Blank),
        "{:?}",
        &map.picks[7..37]
    );
    assert_eq!(map.picks[43], RowPick::Blank);
    assert_eq!(map.blank, 31);
    assert_eq!(map.picks[37], RowPick::Sample(3));
}

/// Casser : ne garder que le premier échantillon du seau.
#[test]
fn several_samples_in_one_row_make_a_bucket() {
    let positions: Vec<f64> = (0..100).map(f64::from).collect();
    let covers = cover_of(&positions, None);
    let map = rows_for(&covers, [0.0, 100.0], 10).unwrap();
    for (r, pick) in map.picks.iter().enumerate() {
        assert_eq!(
            *pick,
            RowPick::Bucket {
                from: 10 * r,
                to: 10 * r + 10,
                nearest: 10 * r + 5
            }
        );
    }
    assert_eq!(map.most_per_pixel, 10);
    assert_eq!(map.blank, 0);
}

/// RESQML TRG 2.2 §12.13 p. 310: a property on intervals between MD points.
/// Casser : choisir l'échantillon au milieu le plus proche (la ligne 1, de
/// centre 15, est à égale distance de 5 et de 25 et prendrait 0).
#[test]
fn an_interval_row_covers_its_interval() {
    let covers = interval_covers(&[0.0, 10.0], &[10.0, 40.0]).unwrap();
    let map = rows_for(&covers, [0.0, 40.0], 4).unwrap();
    assert_eq!(
        map.picks,
        vec![
            RowPick::Sample(0),
            RowPick::Sample(1),
            RowPick::Sample(1),
            RowPick::Sample(1)
        ]
    );
}

/// Casser : retirer la troisième règle (échantillon posé dans la ligne).
#[test]
fn a_point_sample_narrower_than_a_pixel_is_still_drawn() {
    let covers = cover_of(&[0.0, 100.0, 200.0], Some(0.1));
    let map = rows_for(&covers, [0.0, 200.0], 4).unwrap();
    assert_eq!(map.picks[2], RowPick::Sample(1));
}

/// Casser : ignorer la période.
#[test]
fn columns_wrap_on_a_cyclic_axis() {
    let covers = cover_of(&[0.0, 90.0, 180.0, 270.0], None);
    let cyclic = columns_for(&covers, [0.0, 360.0], 8, Some(360.0)).unwrap();
    assert_eq!(cyclic.picks[7], Some(0));
    let flat = columns_for(&covers, [0.0, 360.0], 8, None).unwrap();
    assert_eq!(flat.picks[7], None);
    assert_eq!(flat.blank, 1);
}

fn identity_rows(n: usize) -> RowMap {
    RowMap {
        picks: (0..n).map(RowPick::Sample).collect(),
        blank: 0,
        most_per_pixel: 1,
    }
}

fn identity_columns(n: usize) -> ColumnMap {
    ColumnMap {
        picks: (0..n).map(Some).collect(),
        blank: 0,
    }
}

/// Casser : décaler la colonne d'un pixel.
#[test]
fn paint_uses_the_given_colour_for_every_cell() {
    let values: Vec<f64> = (0..6).map(f64::from).collect();
    let grid = GridView {
        rows: 2,
        cols: 3,
        values: &values,
    };
    let raster = paint_grid(
        grid,
        &identity_rows(2),
        &identity_columns(3),
        &NearestReduce,
        &|v| Some([(v as u8) * 10, 0, 0, 255]),
    )
    .unwrap();
    let expected: Vec<u8> = (0..6u8).flat_map(|v| [v * 10, 0, 0, 255]).collect();
    assert_eq!(raster.rgba(), expected.as_slice());
    assert_eq!((raster.width(), raster.height()), (3, 2));
}

/// ADR-0005: a value nobody coloured is counted, not painted. Casser : peindre
/// le NaN en noir.
#[test]
fn a_value_the_colour_refuses_is_counted_not_painted() {
    let values = [0.0, 1.0, 2.0, f64::NAN, 4.0, 5.0];
    let grid = GridView {
        rows: 2,
        cols: 3,
        values: &values,
    };
    let raster = paint_grid(
        grid,
        &identity_rows(2),
        &identity_columns(3),
        &NearestReduce,
        &|v| v.is_finite().then_some([200, 0, 0, 255]),
    )
    .unwrap();
    let counts = raster.counts();
    assert_eq!(counts.not_coloured, 1);
    assert_eq!(raster.pixel(0, 1), Some([0, 0, 0, 0]));
    assert_eq!(counts.painted + counts.blank + counts.not_coloured, 6);
}

/// ADR-0006: a caller's addition is tried first. Casser : `register` par
/// `push` au lieu d'`insert(0, ..)`.
#[test]
fn bucket_reductions_are_registered_and_the_callers_come_first() {
    struct Mine;
    impl RowReduce for Mine {
        fn id(&self) -> &'static str {
            "mean"
        }
        fn reduce(&self, _values: &[f64], _nearest: usize) -> f64 {
            42.0
        }
    }
    let mut reduces = RowReduces::with_builtins();
    for id in ["mean", "nearest", "min", "max"] {
        assert!(reduces.find(id).is_some(), "{id}");
    }
    assert_eq!(reduces.find("mean").unwrap().reduce(&[1.0, 3.0], 0), 2.0);
    reduces.register(Box::new(Mine));
    assert_eq!(reduces.find("mean").unwrap().reduce(&[1.0, 3.0], 0), 42.0);
    assert!(reduces.find("median").is_none());
}

/// Casser : inclure le NaN dans la somme.
#[test]
fn mean_min_max_skip_non_finite_values() {
    let values = [1.0, f64::NAN, 3.0];
    assert_eq!(MeanReduce.reduce(&values, 0), 2.0);
    assert_eq!(MinReduce.reduce(&values, 0), 1.0);
    assert_eq!(MaxReduce.reduce(&values, 0), 3.0);
    let only_nan = [f64::NAN];
    assert!(MeanReduce.reduce(&only_nan, 0).is_nan());
    assert!(MinReduce.reduce(&only_nan, 0).is_nan());
    assert!(MaxReduce.reduce(&only_nan, 0).is_nan());
}

const RED: [u8; 4] = [255, 0, 0, 255];
const GREEN: [u8; 4] = [0, 255, 0, 255];

/// Casser : tester le coin du pixel au lieu de son centre.
#[test]
fn a_pixel_whose_centre_is_inside_a_quad_is_painted() {
    let mut raster = Raster::transparent(5, 3).unwrap();
    let quad = [[0.0, 0.0], [4.0, 0.0], [4.0, 2.0], [0.0, 2.0]];
    let counts = paint_quads(&mut raster, &[quad], &|_| Some(RED));
    for y in 0..3 {
        for x in 0..5 {
            let inside = x < 4 && y < 2;
            assert_eq!(
                raster.pixel(x, y),
                Some(if inside { RED } else { [0, 0, 0, 0] }),
                "({x}, {y})"
            );
        }
    }
    assert_eq!(
        counts,
        PaintCounts {
            painted: 8,
            blank: 7,
            not_coloured: 0
        }
    );
    assert_eq!(raster.counts(), counts);
}

/// Casser : peindre dans l'ordre inverse.
#[test]
fn the_later_quad_wins_where_two_overlap() {
    let mut raster = Raster::transparent(6, 2).unwrap();
    let first = [[0.0, 0.0], [4.0, 0.0], [4.0, 2.0], [0.0, 2.0]];
    let second = [[2.0, 0.0], [6.0, 0.0], [6.0, 2.0], [2.0, 2.0]];
    paint_quads(&mut raster, &[first, second], &|k| {
        Some(if k == 0 { RED } else { GREEN })
    });
    assert_eq!(raster.pixel(1, 0), Some(RED));
    assert_eq!(raster.pixel(2, 0), Some(GREEN));
    assert_eq!(raster.pixel(3, 1), Some(GREEN));
    assert_eq!(raster.pixel(5, 1), Some(GREEN));
}

/// Casser : dilater les quads d'un pixel.
#[test]
fn a_pixel_between_two_quads_stays_transparent() {
    let mut raster = Raster::transparent(5, 1).unwrap();
    let left = [[0.0, 0.0], [2.0, 0.0], [2.0, 1.0], [0.0, 1.0]];
    let right = [[3.0, 0.0], [5.0, 0.0], [5.0, 1.0], [3.0, 1.0]];
    let counts = paint_quads(&mut raster, &[left, right], &|_| Some(RED));
    assert_eq!(raster.pixel(2, 0), Some([0, 0, 0, 0]));
    assert_eq!(counts.blank, 1);
    assert_eq!(counts.painted, 4);
}

/// Casser : sauter un quad sans couleur.
#[test]
fn a_nearer_cell_without_colour_clears_what_is_behind() {
    let mut raster = Raster::transparent(6, 1).unwrap();
    let behind = [[0.0, 0.0], [4.0, 0.0], [4.0, 1.0], [0.0, 1.0]];
    let nearer = [[2.0, 0.0], [6.0, 0.0], [6.0, 1.0], [2.0, 1.0]];
    let counts = paint_quads(&mut raster, &[behind, nearer], &|k| (k == 0).then_some(RED));
    assert_eq!(raster.pixel(1, 0), Some(RED));
    assert_eq!(raster.pixel(2, 0), Some([0, 0, 0, 0]));
    assert_eq!(raster.pixel(3, 0), Some([0, 0, 0, 0]));
    assert_eq!(counts.painted, 2);
    assert_eq!(counts.not_coloured, 4);
    assert_eq!(counts.painted + counts.blank + counts.not_coloured, 6);
}

/// Casser : retirer la borne de `MAX_RASTER_PIXELS`.
#[test]
fn a_raster_too_large_is_refused() {
    assert_eq!(
        Raster::transparent(4097, 4096),
        Err(DrawError::Size {
            width: 4097,
            height: 4096,
            limit: MAX_RASTER_PIXELS
        })
    );
    assert!(matches!(Raster::transparent(0, 1), Err(DrawError::Size { .. })));
    assert!(Raster::transparent(4096, 4096).is_ok());
}

/// Casser : ne pas comparer la longueur des octets à `w x h x 4`.
#[test]
fn bytes_of_the_wrong_length_are_refused() {
    assert!(matches!(
        Raster::from_rgba(2, 2, vec![0; 15]),
        Err(DrawError::RasterBytes {
            expected: 16,
            found: 15,
            ..
        })
    ));
    assert!(Raster::from_rgba(2, 2, vec![0; 16]).is_ok());
}

/// Casser : accepter des positions non croissantes.
#[test]
fn positions_that_do_not_increase_are_refused_by_name() {
    assert_eq!(
        node_covers(&[0.0, 5.0, 5.0], None),
        Err(DrawError::NotIncreasing { at: 2 })
    );
    assert_eq!(node_covers(&[3.0], None), Err(DrawError::SingleSampleNeedsWidth));
    assert!(matches!(
        node_covers(&[0.0, 1.0], Some(0.0)),
        Err(DrawError::BadGap { .. })
    ));
    assert!(matches!(
        rows_for(&[], [1.0, 1.0], 4),
        Err(DrawError::BadWindow { .. })
    ));
    assert_eq!(
        rows_for(&[], [0.0, 1.0], 0),
        Err(DrawError::NoPixels { pixels: 0 })
    );
}

/// Casser : ne pas vérifier la longueur des valeurs de la grille.
#[test]
fn a_grid_of_the_wrong_size_or_a_pick_out_of_it_is_refused() {
    let values = [0.0; 5];
    let bad = GridView {
        rows: 2,
        cols: 3,
        values: &values,
    };
    let refused = paint_grid(
        bad,
        &identity_rows(2),
        &identity_columns(3),
        &NearestReduce,
        &|_| None,
    );
    assert!(matches!(
        refused,
        Err(DrawError::GridShape {
            expected: 6,
            found: 5,
            ..
        })
    ));
    let values = [0.0; 6];
    let grid = GridView {
        rows: 2,
        cols: 3,
        values: &values,
    };
    let refused = paint_grid(
        grid,
        &identity_rows(3),
        &identity_columns(3),
        &NearestReduce,
        &|_| None,
    );
    assert!(matches!(
        refused,
        Err(DrawError::PickOutOfRange { axis: "rows", .. })
    ));
}

/// W3C PNG Specification (Second Edition), signature and IHDR chunk. Break it: write the height in place of the
/// width.
#[test]
fn encoded_png_starts_with_the_signature_and_says_its_size() {
    use energyml_sheet::draw::raster::encode_png;
    let raster = Raster::from_rgba(3, 2, vec![255; 3 * 2 * 4]).unwrap();
    let bytes = encode_png(&raster).unwrap();
    assert_eq!(&bytes[..8], b"\x89PNG\r\n\x1a\n");
    assert_eq!(u32::from_be_bytes(bytes[16..20].try_into().unwrap()), 3);
    assert_eq!(u32::from_be_bytes(bytes[20..24].try_into().unwrap()), 2);
}

/// The pixels are written exactly, straight alpha, in every build: the image data inflated gives each row, after
/// its filter byte 0, the raster's bytes. Break it: premultiply the pixels before writing them.
#[test]
fn a_raster_is_written_exactly_whatever_the_build() {
    use energyml_sheet::draw::raster::encode_png;
    use std::io::Read as _;
    let rgba = vec![255, 0, 0, 255, 0, 0, 0, 0, 10, 20, 30, 128, 0, 255, 0, 255];
    let raster = Raster::from_rgba(2, 2, rgba.clone()).unwrap();
    let bytes = encode_png(&raster).unwrap();
    // IHDR is 8 + 4 + 4 + 13 + 4 bytes from the start; IDAT follows.
    let at = 8 + 25;
    let length = u32::from_be_bytes(bytes[at..at + 4].try_into().unwrap()) as usize;
    assert_eq!(&bytes[at + 4..at + 8], b"IDAT");
    let mut inflated = Vec::new();
    flate2::read::ZlibDecoder::new(&bytes[at + 8..at + 8 + length])
        .read_to_end(&mut inflated)
        .unwrap();
    assert_eq!(inflated, [&[0][..], &rgba[..8], &[0][..], &rgba[8..]].concat());
}

/// A decoder that is not this crate's reads the same pixels back. Break it: write the rows bottom up.
#[cfg(feature = "png")]
#[test]
fn encode_then_decode_gives_the_same_opaque_pixels() {
    use energyml_sheet::draw::raster::encode_png;
    let rgba = vec![255, 0, 0, 255, 0, 0, 0, 0, 10, 20, 30, 255, 0, 255, 0, 255];
    let raster = Raster::from_rgba(2, 2, rgba).unwrap();
    let pixmap = resvg::tiny_skia::Pixmap::decode_png(&encode_png(&raster).unwrap()).unwrap();
    for (x, y) in [(0u32, 0u32), (1, 0), (0, 1), (1, 1)] {
        let p = pixmap.pixel(x, y).unwrap();
        let got = [p.red(), p.green(), p.blue(), p.alpha()];
        assert_eq!(Some(got), raster.pixel(x, y), "({x}, {y})");
    }
}
