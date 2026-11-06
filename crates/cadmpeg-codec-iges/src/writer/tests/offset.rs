// SPDX-License-Identifier: Apache-2.0

use cadmpeg_ir::codec::write::{target::TargetRequest, EncodeInput, Encoder};
use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::geometry::{CurveGeometry, ProceduralCurveDefinition, SolvedCurveGeometry};
use std::io::Cursor;

use crate::{IgesCodec, IgesVersion};

#[test]
fn uniform_circle_offset_keeps_its_source_and_construction() {
    let bytes = crate::test_support::test_curves_and_surfaces::uniform_offset_circle_file();
    let input = IgesCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("offset circle decodes");
    let plan = IgesCodec
        .plan(
            EncodeInput::new(input.ir(), None),
            TargetRequest::Explicit(IgesVersion::V5_3.descriptor().id.as_str()),
        )
        .expect("offset circle synthesizes");
    assert!(
        plan.report()
            .losses
            .iter()
            .all(|loss| loss.code != crate::loss::IgesLossCode::ProceduralReduced.kind()),
        "losses: {:?}",
        plan.report().losses
    );
    let mut output = Vec::new();
    plan.write_to(&mut output).expect("offset circle writes");
    let output = IgesCodec
        .decode(&mut Cursor::new(output), &DecodeOptions::default())
        .expect("written offset circle decodes");
    assert_eq!(output.ir().model.curves.len(), 2);
    assert_eq!(output.ir().model.procedural_curves.len(), 1);
    let construction = &output.ir().model.procedural_curves[0];
    let ProceduralCurveDefinition::Offset(offset) = construction.definition() else {
        panic!("written construction is an offset");
    };
    let distance = serde_json::to_value(offset).expect("offset serializes")["distance"]
        .as_f64()
        .expect("offset distance");
    assert!((distance - 0.5).abs() < 1.0e-12);
    let source = output
        .ir()
        .model
        .curves
        .iter()
        .find(|curve| curve.id == *offset.source())
        .expect("offset source survives");
    let Some(SolvedCurveGeometry::Circle(circle)) = source.geometry.solved() else {
        panic!("offset source is a circle");
    };
    assert!((circle.radius().get() - 2.0).abs() < 1.0e-12);
    for name in ["ARC", "OFFSET"] {
        assert!(
            output.ir().model.attributes.iter().any(|attribute| {
                attribute.name == "name"
                    && matches!(
                        attribute.target,
                        cadmpeg_ir::attributes::AttributeTarget::Curve(_)
                    )
                    && attribute.values
                        == [Some(cadmpeg_ir::attributes::AttributeValue::String(
                            name.into(),
                        ))]
            }),
            "curve name {name} survives"
        );
    }
}

#[test]
fn inconsistent_offset_cache_is_not_written_as_a_construction() {
    let bytes = crate::test_support::test_curves_and_surfaces::uniform_offset_circle_file();
    let input = IgesCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("offset circle decodes");
    let mut ir = input.ir().clone();
    let curve = ir
        .model
        .curves
        .iter_mut()
        .find(|curve| matches!(curve.geometry, CurveGeometry::Procedural { .. }))
        .expect("offset carrier");
    let CurveGeometry::Procedural {
        cache: Some(SolvedCurveGeometry::Circle(circle)),
        ..
    } = &mut curve.geometry
    else {
        panic!("offset has a circle cache");
    };
    *circle = cadmpeg_ir::geometry::analytic::CircleCurve::new(
        circle.center(),
        *circle.frame(),
        cadmpeg_ir::scalar::PositiveLength::new(1.25).expect("positive radius"),
    );
    let plan = IgesCodec
        .plan(
            EncodeInput::new(&ir, None),
            TargetRequest::Explicit(IgesVersion::V5_3.descriptor().id.as_str()),
        )
        .expect("inconsistent cache degrades");
    assert!(plan
        .report()
        .losses
        .iter()
        .any(|loss| loss.code == crate::loss::IgesLossCode::ProceduralReduced.kind()));
    let mut output = Vec::new();
    plan.write_to(&mut output).expect("reduced curve writes");
    let output = IgesCodec
        .decode(&mut Cursor::new(output), &DecodeOptions::default())
        .expect("reduced curve decodes");
    assert!(output.ir().model.procedural_curves.is_empty());
    assert!(output.ir().model.curves.iter().any(|curve| {
        matches!(curve.geometry.solved(), Some(SolvedCurveGeometry::Circle(circle))
            if (circle.radius().get() - 1.25).abs() < 1.0e-12)
    }));
}
