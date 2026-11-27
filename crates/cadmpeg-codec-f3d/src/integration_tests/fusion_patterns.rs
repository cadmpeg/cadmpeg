// SPDX-License-Identifier: Apache-2.0
//! CC0 Fusion API saves of rectangular patterns. Fusion reported each copy's
//! translation.

use std::io::Cursor;

use cadmpeg_core::json;
use cadmpeg_ir::{Codec, DecodeOptions};

use crate::F3dCodec;

const EPS_HOLE_POSITION_MM: f64 = 1.0e-8;

/// The decoded IR of a corpus file as JSON, after checking that the neutral
/// model validates.
fn decoded(bytes: &[u8]) -> serde_json::Value {
    let result = F3dCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("decode the corpus file");
    let validation =
        cadmpeg_ir::validate_neutral(result.ir(), result.report().losses.clone()).unwrap();
    assert!(validation.is_ok(), "{validation:#?}");
    cadmpeg_test_support::wire::in_memory_json(result.ir())
}

/// The definition of the feature named `name`.
fn definition<'a>(ir: &'a serde_json::Value, name: &str) -> &'a serde_json::Value {
    let mut features = ir["model"]["features"]
        .as_array()
        .expect("feature list")
        .iter()
        .filter(|feature| feature["name"] == name);
    let feature = features.next().unwrap_or_else(|| panic!("feature {name}"));
    assert!(features.next().is_none(), "one feature named {name}");
    &feature["definition"]
}

/// The symmetric byte of every rectangular-pattern distribution block.
fn distribution_symmetry(ir: &serde_json::Value) -> Vec<bool> {
    ir["native"]["f3d"]["design_parameter_scopes"]
        .as_array()
        .expect("scopes")
        .iter()
        .filter_map(|scope| scope["rectangular_pattern_construction"]["distribution"].as_object())
        .map(|distribution| distribution["symmetric"]["value"] == true)
        .collect()
}

#[test]
fn a_symmetric_pattern_with_a_count_one_second_direction_reads_its_spacing() {
    // Fusion places the four copies at x = -5, 0, +5 and +10 mm. Direction two
    // has quantity 1 and keeps its 5 mm distance.
    let files: [&[u8]; 2] = [
        include_bytes!("../../../../corpus/f3d/symmetric_pattern.f3d"),
        include_bytes!("../../../../corpus/f3d/symmetric_pattern_body.f3d"),
    ];
    for bytes in files {
        let ir = decoded(bytes);
        let pattern = &definition(&ir, "R-Pattern 1")["value"]["value"]["pattern"];
        assert_eq!(pattern["kind"], "linear", "{pattern}");
        assert_eq!(pattern["direction"], json!({"x": 1.0, "y": 0.0, "z": 0.0}));
        assert_eq!(pattern["spacing"], 5.0);
        assert_eq!(pattern["count"], 4);
        assert_eq!(pattern["symmetric"], true);
        assert_eq!(distribution_symmetry(&ir), [true]);
    }
}

/// The Compute option of every rectangular-pattern distribution block.
fn distribution_compute(ir: &serde_json::Value) -> Vec<u64> {
    ir["native"]["f3d"]["design_parameter_scopes"]
        .as_array()
        .expect("scopes")
        .iter()
        .filter_map(|scope| scope["rectangular_pattern_construction"]["distribution"].as_object())
        .filter_map(|distribution| distribution["compute_option"]["value"].as_u64())
        .collect()
}

#[test]
fn the_compute_option_leaves_a_five_copy_pattern_unchanged() {
    // Fusion translates the copies by 0, 2, 4, 6 and 8 mm in all three saves.
    let files: [(&[u8], u64); 3] = [
        (
            include_bytes!("../../../../corpus/f3d/compute_adjust.f3d"),
            2,
        ),
        (
            include_bytes!("../../../../corpus/f3d/compute_optimized.f3d"),
            0,
        ),
        (
            include_bytes!("../../../../corpus/f3d/compute_identical.f3d"),
            1,
        ),
    ];
    for (bytes, compute) in files {
        let ir = decoded(bytes);
        let pattern = &definition(&ir, "Groove pattern")["value"]["value"]["pattern"];
        assert_eq!(pattern["kind"], "linear", "{pattern}");
        assert_eq!(pattern["spacing"], 2.0);
        assert_eq!(pattern["count"], 5);
        assert_eq!(distribution_compute(&ir), [compute]);
    }
}

#[test]
fn an_optimized_pattern_reads_its_extent_and_symmetric_distribution() {
    // Fusion translates the Extent copies by 0, 10 and 20 mm, and the
    // symmetric copies by -10, 0 and +10 mm.
    let extent = decoded(include_bytes!(
        "../../../../corpus/f3d/optimized_extent_pattern.f3d"
    ));
    let pattern = &definition(&extent, "R-Pattern 1")["value"]["value"]["pattern"];
    assert_eq!(pattern["kind"], "linear", "{pattern}");
    assert_eq!(pattern["direction"], json!({"x": 1.0, "y": 0.0, "z": 0.0}));
    assert_eq!(pattern["spacing"], 10.0);
    assert_eq!(pattern["count"], 3);
    assert_eq!(pattern["symmetric"], false);
    assert_eq!(distribution_compute(&extent), [0]);

    let symmetric = decoded(include_bytes!(
        "../../../../corpus/f3d/optimized_symmetric_pattern.f3d"
    ));
    let pattern = &definition(&symmetric, "R-Pattern 1")["value"]["value"]["pattern"];
    assert_eq!(pattern["spacing"], 10.0);
    assert_eq!(pattern["count"], 3);
    assert_eq!(pattern["symmetric"], true);
    assert_eq!(distribution_symmetry(&symmetric), [true]);
    assert_eq!(distribution_compute(&symmetric), [0]);
}

#[test]
fn a_symmetric_extent_direction_spaces_its_copies_by_the_extent_on_each_side() {
    // Fusion translates the symmetric Extent copies by -20, 0 and +20 mm.
    let ir = decoded(include_bytes!(
        "../../../../corpus/f3d/optimized_symmetric_extent_pattern.f3d"
    ));
    let pattern = &definition(&ir, "R-Pattern 1")["value"]["value"]["pattern"];
    assert_eq!(pattern["kind"], "linear", "{pattern}");
    assert_eq!(pattern["direction"], json!({"x": 1.0, "y": 0.0, "z": 0.0}));
    assert_eq!(pattern["spacing"], 20.0);
    assert_eq!(pattern["count"], 3);
    assert_eq!(pattern["symmetric"], true);
    assert_eq!(distribution_symmetry(&ir), [true]);
}

#[test]
fn a_pattern_of_a_cut_through_two_bodies_reads_its_distribution() {
    // Fusion translates the copies by 0, 10 and 20 mm along Y.
    let ir = decoded(include_bytes!(
        "../../../../corpus/f3d/two_target_pattern.f3d"
    ));
    let pattern = &definition(&ir, "R-Pattern 1")["value"]["value"]["pattern"];
    assert_eq!(pattern["kind"], "linear", "{pattern}");
    assert_eq!(pattern["direction"], json!({"x": 0.0, "y": 1.0, "z": 0.0}));
    assert_eq!(pattern["spacing"], 10.0);
    assert_eq!(pattern["count"], 3);
    assert_eq!(distribution_compute(&ir), [2]);
}

#[test]
fn each_direction_of_a_rectangular_pattern_reads_its_own_symmetric_byte() {
    // Fusion repeats one slot along +x at 5 mm and along +y at 10 mm. A
    // symmetric direction of count 3 places one copy on each side of the seed;
    // count 4 places two copies along +y and one against it.
    let files: [(&[u8], u64, u64, bool, bool); 4] = [
        (
            include_bytes!("../../../../corpus/f3d/sym_q3_dir1only.f3d"),
            3,
            3,
            true,
            false,
        ),
        (
            include_bytes!("../../../../corpus/f3d/sym_q3_dir2only.f3d"),
            3,
            3,
            false,
            true,
        ),
        (
            include_bytes!("../../../../corpus/f3d/sym_q3_both.f3d"),
            3,
            3,
            true,
            true,
        ),
        (
            include_bytes!("../../../../corpus/f3d/sym_q4_dir2only.f3d"),
            2,
            4,
            false,
            true,
        ),
    ];
    for (bytes, count, second_count, symmetric, second_symmetric) in files {
        let ir = decoded(bytes);
        let pattern = &definition(&ir, "R-Pattern 1")["value"]["value"]["pattern"];
        assert_eq!(pattern["kind"], "linear", "{pattern}");
        assert_eq!(pattern["direction"], json!({"x": 1.0, "y": 0.0, "z": 0.0}));
        assert_eq!(pattern["spacing"], 5.0);
        assert_eq!(pattern["count"], count);
        assert_eq!(pattern["symmetric"], symmetric, "{pattern}");
        let second = &pattern["second"];
        assert_eq!(second["direction"], json!({"x": 0.0, "y": 1.0, "z": 0.0}));
        assert_eq!(second["spacing"], 10.0);
        assert_eq!(second["count"], second_count);
        assert_eq!(second["symmetric"], second_symmetric, "{pattern}");
    }
}

/// The Design `BulkStream.dat` member of a Fusion archive, which Fusion
/// stores with Zstandard.
fn design_bulk_stream(bytes: &[u8]) -> Vec<u8> {
    const NAME: &str = "FusionAssetName[Active]/FusionDesignSegmentType1/BulkStream.dat";
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes)).expect("archive");
    let index = archive.index_for_name(NAME).expect("Design BulkStream");
    assert!(crate::zip_write::is_zstd_member(&mut archive, index).expect("method"));
    let mut member = archive.by_index_raw(index).expect("raw member");
    let session = cadmpeg_core::decode::test_support::service_decode_context();
    session
        .charge_input(
            cadmpeg_core::decode::u64_from_index(bytes.len()),
            "the archive",
        )
        .expect("the archive fits the session");
    crate::zip_write::expand_member(&session, &mut member, NAME, true).expect("expand BulkStream")
}

#[test]
fn the_payload_stores_the_symmetric_byte_of_direction_two_after_that_of_direction_one() {
    use crate::layout::rectangular_pattern_payload_prefix as prefix;
    let files: [(&[u8], [u8; 2]); 4] = [
        (
            include_bytes!("../../../../corpus/f3d/sym_q3_dir1only.f3d"),
            [1, 0],
        ),
        (
            include_bytes!("../../../../corpus/f3d/sym_q3_dir2only.f3d"),
            [0, 1],
        ),
        (
            include_bytes!("../../../../corpus/f3d/sym_q3_both.f3d"),
            [1, 1],
        ),
        (
            include_bytes!("../../../../corpus/f3d/sym_q4_dir2only.f3d"),
            [0, 1],
        ),
    ];
    for (bytes, symmetric) in files {
        let ir = decoded(bytes);
        let stream = design_bulk_stream(bytes);
        let [distribution] = ir["native"]["f3d"]["design_parameter_scopes"]
            .as_array()
            .expect("scopes")
            .iter()
            .filter_map(|scope| {
                scope["rectangular_pattern_construction"]["distribution"].as_object()
            })
            .collect::<Vec<_>>()[..]
        else {
            panic!("one distribution block");
        };
        let offset = |field: &str| {
            usize::try_from(distribution[field]["offset"].as_u64().expect("offset")).unwrap()
        };
        let start = offset("symmetric") - prefix::SYMMETRIC;
        assert_eq!(offset("symmetric_two"), start + prefix::SYMMETRIC_TWO);
        assert_eq!(
            stream[start + prefix::SENTINEL..start + prefix::SENTINEL + 4],
            [0xff; 4]
        );
        assert_eq!(
            [
                stream[start + prefix::SYMMETRIC],
                stream[start + prefix::SYMMETRIC_TWO]
            ],
            symmetric
        );
        // Distance type 0: each distance owner is the adjacent spacing.
        assert_eq!(stream[offset("total_extent")], 0);
    }
}

/// The offsets along one direction of a linear pattern, in grid order: the
/// original, then for a symmetric direction an odd coordinate `k` at
/// `(k + 1) / 2` spacings along the direction and an even `k` at `k / 2`
/// spacings against it.
fn direction_offsets(direction: &serde_json::Value) -> Vec<[f64; 3]> {
    let spacing = direction["spacing"].as_f64().expect("spacing");
    let unit = ["x", "y", "z"].map(|axis| direction["direction"][axis].as_f64().expect("axis"));
    let count = u32::try_from(direction["count"].as_u64().expect("count")).expect("u32 count");
    let symmetric = direction["symmetric"] == true;
    (0..count)
        .map(|k| {
            let steps = match (symmetric, k % 2) {
                (false, _) => f64::from(k),
                (true, 1) => f64::from(k.div_ceil(2)),
                (true, _) => -f64::from(k / 2),
            };
            unit.map(|component| component * spacing * steps)
        })
        .collect()
}

#[test]
fn a_symmetric_extent_direction_places_its_copies_where_fusion_does() {
    // Fusion API readback of each copy's translation, in mm. The plate slot is
    // patterned with distance type Extent 20 mm along +x, +y, or both, or
    // with Extent -20 mm along x.
    const EPS_POSITION_MM: f64 = 1.0e-9;
    let third = 20.0 / 3.0;
    let files: [(&[u8], Vec<f64>, Vec<f64>); 8] = [
        (
            include_bytes!("../../../../corpus/f3d/sym_extent_q2.f3d"),
            vec![0.0, 20.0],
            vec![0.0],
        ),
        (
            include_bytes!("../../../../corpus/f3d/sym_extent_q4.f3d"),
            vec![-10.0, 0.0, 10.0, 20.0],
            vec![0.0],
        ),
        (
            include_bytes!("../../../../corpus/f3d/sym_extent_q6.f3d"),
            vec![-2.0 * third, -third, 0.0, third, 2.0 * third, 20.0],
            vec![0.0],
        ),
        (
            include_bytes!("../../../../corpus/f3d/sym_extent_q7.f3d"),
            vec![-20.0, -2.0 * third, -third, 0.0, third, 2.0 * third, 20.0],
            vec![0.0],
        ),
        (
            include_bytes!("../../../../corpus/f3d/sym_extent_q8.f3d"),
            vec![-15.0, -10.0, -5.0, 0.0, 5.0, 10.0, 15.0, 20.0],
            vec![0.0],
        ),
        (
            include_bytes!("../../../../corpus/f3d/sym_extent_q4_negative.f3d"),
            vec![10.0, 0.0, -10.0, -20.0],
            vec![0.0],
        ),
        (
            include_bytes!("../../../../corpus/f3d/sym_extent_q5_dir2only.f3d"),
            vec![0.0],
            vec![-20.0, -10.0, 0.0, 10.0, 20.0],
        ),
        (
            include_bytes!("../../../../corpus/f3d/sym_extent_q4x5.f3d"),
            vec![-10.0, 0.0, 10.0, 20.0],
            vec![-20.0, -10.0, 0.0, 10.0, 20.0],
        ),
    ];
    for (bytes, xs, ys) in files {
        let ir = decoded(bytes);
        let pattern = &definition(&ir, "R-Pattern 1")["value"]["value"]["pattern"];
        assert_eq!(pattern["kind"], "linear", "{pattern}");
        let first = direction_offsets(pattern);
        let second = if pattern["second"].is_null() {
            vec![[0.0; 3]]
        } else {
            direction_offsets(&pattern["second"])
        };
        let mut positions = first
            .iter()
            .flat_map(|u| second.iter().map(move |v| [u[0] + v[0], u[1] + v[1]]))
            .collect::<Vec<_>>();
        let mut expected = xs
            .iter()
            .flat_map(|x| ys.iter().map(move |y| [*x, *y]))
            .collect::<Vec<_>>();
        for list in [&mut positions, &mut expected] {
            list.sort_by(|a, b| a[0].total_cmp(&b[0]).then(a[1].total_cmp(&b[1])));
        }
        assert_eq!(positions.len(), expected.len(), "{pattern}");
        for (position, expected) in positions.iter().zip(&expected) {
            assert!(
                (position[0] - expected[0]).abs() < EPS_POSITION_MM
                    && (position[1] - expected[1]).abs() < EPS_POSITION_MM,
                "{position:?} {expected:?} {pattern}"
            );
        }
    }
}

#[test]
fn a_negative_symmetric_extent_reverses_the_direction_and_its_extra_instance() {
    // Fusion places the four copies at x = 10, 0, -10 and -20 mm: the
    // signed-distance side holds the extra instance.
    let ir = decoded(include_bytes!(
        "../../../../corpus/f3d/sym_extent_q4_negative.f3d"
    ));
    let pattern = &definition(&ir, "R-Pattern 1")["value"]["value"]["pattern"];
    assert_eq!(pattern["kind"], "linear", "{pattern}");
    assert_eq!(pattern["direction"], json!({"x": -1.0, "y": 0.0, "z": 0.0}));
    assert_eq!(pattern["spacing"], 10.0);
    assert_eq!(pattern["count"], 4);
    assert_eq!(pattern["symmetric"], true);
}

#[test]
fn a_rectangular_feature_pattern_keeps_its_direction_in_the_v11_record() {
    let ir = decoded(include_bytes!(
        "../../../../corpus/f3d/optimized_extent_pattern.f3d"
    ));
    let features = ir["design"]["features"].as_array().expect("v11 features");
    let feature = features
        .iter()
        .find(|feature| feature["name"] == "R-Pattern 1")
        .expect("rectangular pattern");
    let operation = &feature["definition"]["operation"];
    assert_eq!(operation["kind"], "pattern");
    let layout = &operation["value"]["layout"];
    assert_eq!(layout["kind"], "linear");
    let run = &layout["value"]["first"];
    assert_eq!(
        run["direction"]["resolved"],
        json!({"x": 1.0, "y": 0.0, "z": 0.0})
    );
    assert_eq!(run["run"]["spacing"]["kind"], "span");
    assert_eq!(
        run["run"]["spacing"]["value"]["count"],
        json!({"kind": "literal", "value": 3})
    );
    assert_eq!(
        run["run"]["spacing"]["value"]["span"],
        json!({"kind": "literal", "value": 20.0})
    );
    let mut centers = ir["geometry"]["surfaces"]
        .as_array()
        .expect("final surfaces")
        .iter()
        .filter_map(|surface| {
            let geometry = &surface["geometry"]["value"];
            (geometry["kind"] == "cylinder").then(|| {
                let origin = &geometry["origin"];
                [origin["x"].as_f64().unwrap(), origin["y"].as_f64().unwrap()]
            })
        })
        .collect::<Vec<_>>();
    centers.sort_by(|a, b| a[0].total_cmp(&b[0]));
    assert_eq!(centers.len(), 3);
    for (center, expected_x) in centers.iter().zip([10.0, 20.0, 30.0]) {
        assert!((center[0] - expected_x).abs() < EPS_HOLE_POSITION_MM);
        assert!((center[1] - 20.0).abs() < EPS_HOLE_POSITION_MM);
    }
}

#[test]
fn an_unproved_body_seed_keeps_its_native_record_and_final_geometry() {
    let result = F3dCodec
        .decode(
            &mut Cursor::new(include_bytes!(
                "../../../../corpus/f3d/symmetric_pattern_body.f3d"
            )),
            &DecodeOptions::default(),
        )
        .expect("decode the body pattern");
    assert_eq!(result.ir().geometry.bodies.len(), 5);
    let feature = result
        .ir()
        .design
        .features
        .iter()
        .find(|feature| feature.name.as_deref() == Some("R-Pattern 1"))
        .expect("body pattern feature");
    let cadmpeg_ir::design::feature::FeatureOperation::Pattern(pattern) =
        &feature.definition.operation
    else {
        panic!("typed body pattern");
    };
    let cadmpeg_ir::design::ops::body::PatternSeeds::Bodies { bodies, join } = &pattern.seeds
    else {
        panic!("body seeds");
    };
    assert_eq!(*join, None);
    let cadmpeg_ir::design::selection::Picks::Named(picks) = &bodies.picks else {
        panic!("native body seed");
    };
    assert!(picks.as_slice()[0].native.is_some());
    assert!(feature.native_ref.is_some());
    assert!(result.report().losses.iter().any(|loss| {
        loss.code == crate::loss::F3dLossCode::FeatureRectangularPatternDesignNative.kind()
    }));
}

#[test]
fn suppressed_instances_are_omitted_from_the_pattern() {
    // Positions 1 and 3 of five along X, and one position of a 3 by 3 grid,
    // are suppressed.
    let files: [(&[u8], u64, Option<u64>, serde_json::Value); 3] = [
        (
            include_bytes!("../../../../corpus/f3d/suppressed_two_instances.f3d"),
            5,
            None,
            json!([[1, 0], [3, 0]]),
        ),
        (
            include_bytes!("../../../../corpus/f3d/suppressed_grid_middle.f3d"),
            3,
            Some(3),
            json!([[1, 1]]),
        ),
        (
            include_bytes!("../../../../corpus/f3d/suppressed_grid_corner.f3d"),
            3,
            Some(3),
            json!([[2, 2]]),
        ),
    ];
    for (bytes, count, second, omitted) in files {
        let ir = decoded(bytes);
        let pattern = &definition(&ir, "R-Pattern 1")["value"]["value"]["pattern"];
        assert_eq!(pattern["kind"], "linear", "{pattern}");
        let spacing = pattern["spacing"].as_f64().expect("spacing");
        assert!((spacing - 6.0).abs() < 1e-9, "{pattern}");
        assert_eq!(pattern["count"], count);
        assert_eq!(pattern["second"]["count"].as_u64(), second);
        assert_eq!(pattern["omitted_instances"], omitted);
    }
}

/// A save, its first direction, its counts, its symmetric bytes and its
/// omitted grid positions.
type SymmetricSuppression = (
    &'static [u8],
    serde_json::Value,
    u64,
    Option<u64>,
    bool,
    Option<bool>,
    serde_json::Value,
);

#[test]
fn suppressed_instances_of_symmetric_directions_keep_the_neutral_numbering() {
    // A symmetric direction's odd positions lie along it and its even ones
    // against it. The pattern with direction one at count one projects as one
    // direction along Y.
    let files: [SymmetricSuppression; 4] = [
        (
            include_bytes!("../../../../corpus/f3d/suppressed_symmetric_even.f3d"),
            json!({"x": 1.0, "y": 0.0, "z": 0.0}),
            4,
            None,
            true,
            None,
            json!([[1, 0], [2, 0]]),
        ),
        (
            include_bytes!("../../../../corpus/f3d/suppressed_symmetric_direction_two.f3d"),
            json!({"x": 1.0, "y": 0.0, "z": 0.0}),
            3,
            Some(3),
            false,
            Some(true),
            json!([[0, 1], [1, 2]]),
        ),
        (
            include_bytes!("../../../../corpus/f3d/suppressed_symmetric_both.f3d"),
            json!({"x": 1.0, "y": 0.0, "z": 0.0}),
            3,
            Some(3),
            true,
            Some(true),
            json!([[1, 1], [2, 2]]),
        ),
        (
            include_bytes!("../../../../corpus/f3d/suppressed_direction_two_only.f3d"),
            json!({"x": 0.0, "y": 1.0, "z": 0.0}),
            4,
            None,
            false,
            None,
            json!([[1, 0], [3, 0]]),
        ),
    ];
    for (bytes, direction, count, second, symmetric, second_symmetric, omitted) in files {
        let ir = decoded(bytes);
        let pattern = &definition(&ir, "R-Pattern 1")["value"]["value"]["pattern"];
        assert_eq!(pattern["kind"], "linear", "{pattern}");
        assert_eq!(pattern["direction"], direction, "{pattern}");
        if second.is_some() {
            assert_eq!(
                pattern["second"]["direction"],
                json!({"x": 0.0, "y": 1.0, "z": 0.0}),
                "{pattern}"
            );
        }
        assert_eq!(pattern["count"], count, "{pattern}");
        assert_eq!(pattern["second"]["count"].as_u64(), second, "{pattern}");
        assert_eq!(pattern["symmetric"], symmetric, "{pattern}");
        assert_eq!(
            pattern["second"]["symmetric"].as_bool(),
            second_symmetric,
            "{pattern}"
        );
        assert_eq!(pattern["omitted_instances"], omitted, "{pattern}");
    }
}
