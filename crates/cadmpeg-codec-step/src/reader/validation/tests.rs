// SPDX-License-Identifier: Apache-2.0
//! STEP geometric validation-property tests.

#![allow(clippy::unwrap_used)]
#![allow(clippy::default_trait_access)]

use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeOptions};

use crate::StepCodec;

#[test]
fn complex_validation_measure_carrier_is_decoded() {
    let source = String::from_utf8(
        include_bytes!("../../../tests/fixtures/ap242_tessellation.p21").to_vec(),
    )
    .expect("fixture is UTF-8")
    .replace(
        "#42=REPRESENTATION('surface area',(#43),#2);",
        "#42=(REPRESENTATION('surface area',(#43),#2) SHAPE_REPRESENTATION());",
    )
    .replace(
        "#43=MEASURE_REPRESENTATION_ITEM('surface area measure',AREA_MEASURE(50.),#44);",
        "#43=(MEASURE_REPRESENTATION_ITEM() MEASURE_WITH_UNIT(AREA_MEASURE(50.),#44) REPRESENTATION_ITEM('surface area measure'));",
    );
    let result = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode complex validation measure");

    assert!(result.report().notes.iter().any(|note| {
        note == "geometric validation surface area triangle sheet: expected 50, tessellation approximation 50"
    }));
    assert!(!result.report().losses.iter().any(|loss| {
        loss.message
            .contains("geometric validation property #41 has an unsupported value")
    }));
    let validation = cadmpeg_ir::validate_neutral(result.ir(), result.report().losses.clone());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn direct_area_and_volume_unit_subtypes_scale_validation_measures() {
    let source = String::from_utf8(
        include_bytes!("../../../tests/fixtures/ap242_tessellation.p21").to_vec(),
    )
    .expect("fixture is UTF-8")
    .replace("#44=DERIVED_UNIT((#55));", "#44=AREA_UNIT((#55));")
    .replace("#53=DERIVED_UNIT((#56));", "#53=VOLUME_UNIT((#56));");
    let result = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode direct validation unit subtypes");

    assert!(result.report().notes.iter().any(|note| {
        note == "geometric validation surface area triangle sheet: expected 50, tessellation approximation 50"
    }));
    assert!(result.report().notes.iter().any(|note| {
        note == "geometric validation volume open sheet volume: expected 0, tessellation approximation 0"
    }));
    assert!(!result
        .report()
        .losses
        .iter()
        .any(|loss| { loss.message.contains("unit scale did not resolve") }));
    let unknowns = result
        .ir()
        .native_unknowns("step")
        .expect("STEP unknown records");
    for id in [44, 53, 55, 56] {
        assert!(
            !unknowns
                .iter()
                .any(|record| record.id.as_str().ends_with(&format!("#{id}"))),
            "validation unit carrier #{id} was not typed"
        );
    }
}

#[test]
fn validation_representation_decodes_all_measure_items() {
    let source = String::from_utf8(
        include_bytes!("../../../tests/fixtures/ap242_tessellation.p21").to_vec(),
    )
    .expect("fixture is UTF-8")
    .replace(
        "#42=REPRESENTATION('surface area',(#43),#2);",
        "#42=REPRESENTATION('surface area',(#43,#52),#2);",
    );
    let result = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode validation representation with multiple items");

    assert!(result.report().notes.iter().any(|note| {
        note == "geometric validation surface area triangle sheet: expected 50, tessellation approximation 50"
    }));
    assert!(result.report().notes.iter().any(|note| {
        note == "geometric validation volume triangle sheet: expected 0, tessellation approximation 0"
    }));
    assert!(!result.report().losses.iter().any(|loss| {
        loss.message
            .contains("geometric validation property #41 has unsupported item")
    }));
}

#[test]
fn ps06_combined_validation_items_ignore_set_order_and_keep_siblings() {
    let decode_fixture = |bytes: &[u8]| {
        StepCodec::default()
            .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
            .expect("decode PS-06 fixture")
    };
    let validation_notes = |result: &cadmpeg_ir::codec::DecodeResult| {
        let mut notes = result
            .report()
            .notes
            .iter()
            .filter(|note| note.starts_with("geometric validation "))
            .cloned()
            .collect::<Vec<_>>();
        notes.sort();
        notes
    };
    let unsupported_items = |result: &cadmpeg_ir::codec::DecodeResult| {
        result
            .report()
            .losses
            .iter()
            .filter(|loss| {
                loss.message
                    .contains("geometric validation property #41 has unsupported item")
            })
            .map(|loss| loss.message.clone())
            .collect::<Vec<_>>()
    };

    let source_order = decode_fixture(include_bytes!(
        "tests/data/ps06_combined_validation_properties_source_order.p21"
    ));
    let reordered = decode_fixture(include_bytes!(
        "tests/data/ps06_combined_validation_properties_reordered.p21"
    ));

    let expected_notes = vec![
        "geometric validation centroid combined validation: expected (1,2,3)".to_owned(),
        "geometric validation surface area combined validation: expected 50".to_owned(),
        "geometric validation volume combined validation: expected 8".to_owned(),
    ];
    assert_eq!(validation_notes(&source_order), expected_notes);
    assert_eq!(validation_notes(&reordered), expected_notes);
    assert_eq!(
        unsupported_items(&source_order),
        ["geometric validation property #41 has unsupported item #60"]
    );
    assert_eq!(
        unsupported_items(&source_order),
        unsupported_items(&reordered)
    );
}

#[test]
fn validation_shape_representation_with_parameters_uses_inherited_items() {
    let source = String::from_utf8(
        include_bytes!("../../../tests/fixtures/ap242_tessellation.p21").to_vec(),
    )
    .expect("fixture is UTF-8")
    .replace(
        "#42=REPRESENTATION('surface area',(#43),#2);",
        "#42=SHAPE_REPRESENTATION_WITH_PARAMETERS('surface area',(#43),#2);",
    );
    let result = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode parameterized validation representation");

    assert!(result.report().notes.iter().any(|note| {
        note == "geometric validation surface area triangle sheet: expected 50, tessellation approximation 50"
    }));
    assert!(!result.report().losses.iter().any(|loss| {
        loss.message
            .contains("geometric validation property #41 has unsupported item")
    }));
}

#[test]
fn numerical_followup_mesh_volume_and_centroid_are_translation_invariant() {
    use cadmpeg_ir::{
        math::Point3,
        tessellation::{Tessellation, TessellationMesh},
    };
    let source = include_bytes!("../../../tests/fixtures/ap242_tessellation.p21");
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let mut ir = decoded.ir().clone();
    assert_eq!(ir.model.bodies.len(), 1);
    for offset in [0.0, 1e6, 1e9] {
        let points = [[0., 0., 0.], [2., 0., 0.], [0., 1., 0.], [0., 0., 1.]]
            .map(|p| Point3::new(p[0] + offset, p[1] + offset, p[2] + offset))
            .to_vec();
        let mesh = TessellationMesh::from_list_lanes(
            points,
            vec![[0, 2, 1], [0, 1, 3], [0, 3, 2], [1, 2, 3]],
            None,
        )
        .unwrap();
        let mut tessellation = Tessellation::new(
            "test:step:tessellation#numerical-followup",
            mesh,
            Vec::new(),
        )
        .unwrap();
        tessellation.body = Some(ir.model.bodies[0].id.clone());
        ir.model.tessellations = vec![tessellation];
        let properties = super::mesh_properties(&ir).unwrap();
        assert!((properties.volume - 1.0 / 3.0).abs() <= f64::EPSILON);
        assert_eq!(
            properties.centroid,
            Point3::new(offset + 0.5, offset + 0.25, offset + 0.25)
        );
    }
}

#[test]
fn numerical_seventh_mesh_mass_properties_preserve_uniform_scale() {
    use cadmpeg_ir::{
        math::Point3,
        tessellation::{Tessellation, TessellationMesh},
    };
    let source = include_bytes!("../../../tests/fixtures/ap242_tessellation.p21");
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let mut ir = decoded.ir().clone();
    assert_eq!(ir.model.bodies.len(), 1);
    for scale in [1.0e-90, 1.0, 1.0e90] {
        let points = [[0., 0., 0.], [2., 0., 0.], [0., 1., 0.], [0., 0., 1.]]
            .map(|p| Point3::new(p[0] * scale, p[1] * scale, p[2] * scale))
            .to_vec();
        let mesh = TessellationMesh::from_list_lanes(
            points,
            vec![[0, 2, 1], [0, 1, 3], [0, 3, 2], [1, 2, 3]],
            None,
        )
        .unwrap();
        let mut tessellation = Tessellation::new(
            "test:step:tessellation#numerical-followup",
            mesh,
            Vec::new(),
        )
        .unwrap();
        tessellation.body = Some(ir.model.bodies[0].id.clone());
        ir.model.tessellations = vec![tessellation];
        let properties = super::mesh_properties(&ir).unwrap();
        assert!(
            (properties.volume / scale / scale / scale - 1.0 / 3.0).abs() <= 8.0 * f64::EPSILON
        );
        for (actual, expected) in [
            (properties.centroid.x / scale, 0.5),
            (properties.centroid.y / scale, 0.25),
            (properties.centroid.z / scale, 0.25),
        ] {
            assert!((actual - expected).abs() <= 8.0 * f64::EPSILON);
        }
    }
}
