// SPDX-License-Identifier: Apache-2.0
//! STEP geometric validation-property tests.

#![allow(clippy::unwrap_used)]
#![allow(clippy::default_trait_access)]
#![allow(unused_imports)]

use std::fmt::Write as _;
use std::io::Cursor;

use cadmpeg_core::decode::{DecodeMode, InspectOptions};
use cadmpeg_ir::codec::{Codec, CodecBackend, Confidence, DecodeOptions};
use cadmpeg_ir::eval::{
    model_curve_point_by_id, model_surface_partials_by_id, model_surface_point_by_id, pcurve_uv,
};
use cadmpeg_ir::examples::unit_cube;
use cadmpeg_ir::geometry::{
    Curve, CurveGeometry, NurbsCurve, NurbsSurface, PcurveGeometry, Surface, SurfaceGeometry,
};
use cadmpeg_ir::ids::{CurveId, ProceduralCurveId, SurfaceId};
use cadmpeg_ir::index::ModelIndex;
use cadmpeg_ir::math::{Point2, Point3, Vector3};
use cadmpeg_ir::transform::Transform;
use cadmpeg_ir::units::{LengthUnit, Units};
use cadmpeg_ir::CadIr;
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipWriter};

use crate::ids::StepIdentity;
use crate::test_support::{decode_inline, export};
use crate::{
    write_step, StepCodec, StepError, StepSchema, StepUnsupportedPolicy, StepWriteOptions,
};

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
                .any(|record| record.id.0.ends_with(&format!("#{id}"))),
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
