// SPDX-License-Identifier: Apache-2.0
//! Revolution construction writer tests.
#![allow(clippy::unwrap_used)]

use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_test_support::EditableDecodeResult;
use std::io::Cursor;

use crate::test_support::container::{make_block, sldprt_with_body};
use crate::test_support::native::sldprt_native;
use crate::test_support::parasolid::triangle_body;
use crate::SldprtCodec;

const EPS_PARTIAL_REVOLUTION_ANGLE: f64 = 1.0e-12;
const EPS_REVERSED_REVOLUTION_ANGLE: f64 = 1.0e-12;
const EPS_SYMMETRIC_REVOLUTION_ANGLE: f64 = 1.0e-12;
const EPS_TWO_SIDED_REVOLUTION_FIRST_ANGLE: f64 = 1.0e-12;
const EPS_TWO_SIDED_REVOLUTION_SECOND_ANGLE: f64 = 1.0e-12;

#[test]
fn semantic_writer_round_trips_typed_revolution() {
    use cadmpeg_ir::features::{
        AngularTermination, BooleanOp, FeatureDefinition, FeatureOperation, RevolveExtent,
    };
    use cadmpeg_ir::math::{Point3, Vector3};

    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Revolve Name="Turn" Type="Revolve" id="17" AxisOrigin="10mm,20mm,30mm" AxisDirection="0,1,0" Operation="Join"><Dimension Name="Angle">180deg</Dimension></Revolve></Keywords>"#,
    ));
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let mut decoded = EditableDecodeResult::from(decoded);
    assert!(matches!(
        decoded.ir().model.features[0].evaluation.definition(),
        FeatureDefinition::Operation(FeatureOperation::Revolve {
            construction,
            op: BooleanOp::Join,
        }) if construction.profile().is_none()
            && construction.axis().is_some_and(|axis| matches!(axis,
                cadmpeg_ir::features::RevolutionAxis {
                    origin: geometry_1,
                    direction: geometry_2,
                    ..
                } if matches!(geometry_1.get(), Point3 { x: 10.0, y: 20.0, z: 30.0 }) && matches!(geometry_2.get(), Vector3 { x: 0.0, y: 1.0, z: 0.0 })))
            && matches!(construction.extent(), Some(RevolveExtent::OneSided {
                    termination: AngularTermination::Angle { angle: value },
                }) if (decoded.ir().dimension_value(value).unwrap().get() - std::f64::consts::PI).abs() < EPS_PARTIAL_REVOLUTION_ANGLE)
    ));

    {
        let mut ir_edit = decoded.ir_mut();
        let id = ir_edit.model.features[0].id.clone();
        ir_edit
            .set_dimension(
                &id,
                "/construction/extent/termination/angle",
                cadmpeg_ir::scalar::PositiveAngle::new(std::f64::consts::FRAC_PI_2).unwrap(),
            )
            .unwrap();
        let updated_ir_edit_evaluation = &mut ir_edit.model.features[0].evaluation;
        let mut updated_ir_edit_definition = updated_ir_edit_evaluation.definition().clone();
        let FeatureDefinition::Operation(FeatureOperation::Revolve { construction, op }) =
            &mut updated_ir_edit_definition
        else {
            panic!("typed revolution feature");
        };
        let Some(axis) = construction.axis_mut() else {
            panic!("resolved revolution axis");
        };
        axis.origin = cadmpeg_ir::features::FinitePoint3::new(Point3::new(1.0, 2.0, 3.0)).unwrap();
        axis.direction =
            cadmpeg_ir::features::FeatureDirection3::new(Vector3::new(0.0, 0.0, 1.0)).unwrap();
        *op = BooleanOp::Cut;
        updated_ir_edit_evaluation.set_definition(updated_ir_edit_definition);
    }

    let mut encoded = Vec::new();
    crate::test_support::serialize_edited_history(
        decoded.ir(),
        decoded.source_fidelity(),
        &mut encoded,
    )
    .unwrap();
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();
    let feature = &sldprt_native(regenerated.ir()).feature_histories[0].features[0];
    assert_eq!(feature.properties["AxisOrigin"], "1mm,2mm,3mm");
    assert_eq!(feature.properties["AxisDirection"], "0,0,1");
    assert_eq!(feature.properties["Operation"], "Cut");
    assert_eq!(
        feature.parameters["Angle"],
        format!("{}rad", std::f64::consts::FRAC_PI_2)
    );
}

#[test]
fn semantic_writer_retains_partial_native_revolution_construction() {
    use cadmpeg_ir::features::{BooleanOp, FeatureDefinition, FeatureOperation};
    use cadmpeg_ir::math::{Point3, Vector3};

    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Revolve Name="Unknown turn" Type="Revolve" id="17" AxisOrigin="1mm,2mm,3mm" AxisDirection="0,0,1"/></Keywords>"#,
    ));

    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let mut decoded = EditableDecodeResult::from(decoded);
    assert!(matches!(
        decoded.ir().model.features[0].evaluation.definition(),
        FeatureDefinition::Operation(FeatureOperation::Revolve {
            construction,
            op: BooleanOp::Unresolved,
        }) if construction.profile().is_none()
            && construction.axis().is_some_and(|axis| matches!(axis,
                cadmpeg_ir::features::RevolutionAxis {
                    origin: geometry_1,
                    direction: geometry_2,
                    ..
                } if matches!(geometry_1.get(), Point3 {
                        x: 1.0,
                        y: 2.0,
                        z: 3.0
                    }) && matches!(geometry_2.get(), Vector3 {
                        x: 0.0,
                        y: 0.0,
                        z: 1.0
                    })))
            && construction.extent().is_none()
    ));
    let mut detached = decoded.ir().clone();
    detached.model.features[0].native_ref = None;
    crate::test_support::assert_synthesis_fallback(&detached, decoded.source_fidelity());
    decoded.ir_mut().model.features[0].name = Some("Renamed turn".into());

    let mut encoded = Vec::new();
    crate::test_support::plan_inherited_write(
        decoded.ir(),
        decoded.source_fidelity(),
        &mut encoded,
    )
    .unwrap();
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();
    let native = &sldprt_native(regenerated.ir()).feature_histories[0].features[0];
    assert_eq!(native.name, "Renamed turn");
    assert_eq!(native.properties["AxisOrigin"], "1mm,2mm,3mm");
    assert_eq!(native.properties["AxisDirection"], "0,0,1");
    assert!(!native.properties.contains_key("Profile"));
    assert!(!native.properties.contains_key("Operation"));
    assert!(!native.parameters.contains_key("Angle"));
    assert!(matches!(
        regenerated.ir().model.features[0].evaluation.definition(),
        FeatureDefinition::Operation(FeatureOperation::Revolve {
            ref construction,
            op: BooleanOp::Unresolved,
        }) if construction.axis().is_some()
            && construction.profile().is_none()
            && construction.extent().is_none()
    ));
}

#[test]
fn semantic_writer_round_trips_all_revolution_extents() {
    use cadmpeg_ir::features::{
        AngularTermination, BooleanOp, FeatureDefinition, FeatureOperation, RevolveExtent,
    };

    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Sketch Name="TurnProfile" Type="Sketch" id="40"/><Revolve Name="One" Type="Revolve" id="41" Profile="40" AxisOrigin="0mm,0mm,0mm" AxisDirection="0,0,1" EndCondition="OneSided" Operation="Join"><Dimension Name="Angle">90deg</Dimension></Revolve><Revolve Name="Sym" Type="Revolve" id="42" Profile="40" AxisOrigin="0mm,0mm,0mm" AxisDirection="0,1,0" EndCondition="Symmetric" Operation="NewBody"><Dimension Name="Angle">180deg</Dimension></Revolve><Revolve Name="Two" Type="Revolve" id="43" Profile="40" AxisOrigin="0mm,0mm,0mm" AxisDirection="1,0,0" EndCondition="TwoSided" Operation="Cut"><Dimension Name="Angle">30deg</Dimension><Dimension Name="Angle2">60deg</Dimension></Revolve></Keywords>"#,
    ));
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let mut decoded = EditableDecodeResult::from(decoded);
    let profile_feature = decoded.ir().model.features[0].id.clone();
    assert!(matches!(
        decoded.ir().model.features[1].evaluation.definition(),
        FeatureDefinition::Operation(FeatureOperation::Revolve {
            construction,
            op: BooleanOp::Join,
        }) if matches!(construction.profile(), Some(cadmpeg_ir::features::PlanarProfileRef::Feature(profile)) if profile == &profile_feature)
            && matches!(construction.extent(), Some(RevolveExtent::OneSided {
                    termination: AngularTermination::Angle { angle: value },
                }) if (decoded.ir().dimension_value(value).unwrap().get() - 90f64.to_radians()).abs() < EPS_REVERSED_REVOLUTION_ANGLE)
    ));
    assert!(matches!(
        decoded.ir().model.features[2].evaluation.definition(),
        FeatureDefinition::Operation(FeatureOperation::Revolve {
            ref construction,
            op: BooleanOp::NewBody,
        }) if matches!(construction.extent(), Some(RevolveExtent::Symmetric {
                    termination: AngularTermination::Angle { angle: value },
                }) if (decoded.ir().dimension_value(value).unwrap().get() - std::f64::consts::PI).abs() < EPS_SYMMETRIC_REVOLUTION_ANGLE)
    ));
    assert!(matches!(
        decoded.ir().model.features[3].evaluation.definition(),
        FeatureDefinition::Operation(FeatureOperation::Revolve {
            ref construction,
            op: BooleanOp::Cut,
        }) if matches!(construction.extent(), Some(RevolveExtent::TwoSided {
                    first: AngularTermination::Angle { angle: first },
                    second: AngularTermination::Angle { angle: second },
                }) if (decoded.ir().dimension_value(first).unwrap().get() - 30f64.to_radians()).abs()
                    < EPS_TWO_SIDED_REVOLUTION_FIRST_ANGLE
                    && (decoded.ir().dimension_value(second).unwrap().get() - 60f64.to_radians()).abs()
                        < EPS_TWO_SIDED_REVOLUTION_SECOND_ANGLE)
    ));

    let angle2_id = decoded
        .ir()
        .model
        .parameters
        .iter()
        .find(|parameter| parameter.name == "Angle2")
        .unwrap()
        .id
        .clone();
    {
        let mut ir_edit = decoded.ir_mut();
        let owner = ir_edit.model.features[3].id.clone();
        ir_edit
            .set_dimension(
                &owner,
                "/construction/extent/first/angle",
                cadmpeg_ir::scalar::PositiveAngle::new(0.75).unwrap(),
            )
            .unwrap();
        let updated_ir_edit_evaluation = &mut ir_edit.model.features[3].evaluation;
        let mut updated_ir_edit_definition = updated_ir_edit_evaluation.definition().clone();
        let FeatureDefinition::Operation(FeatureOperation::Revolve { construction, op }) =
            &mut updated_ir_edit_definition
        else {
            panic!("typed revolution");
        };
        let RevolveExtent::TwoSided { first, .. } =
            construction.extent_mut().expect("fixture extent")
        else {
            panic!("original two-sided revolution");
        };
        let termination = first.clone();
        *construction.extent_mut().expect("fixture extent") =
            RevolveExtent::OneSided { termination };
        *op = BooleanOp::Intersect;
        updated_ir_edit_evaluation.set_definition(updated_ir_edit_definition);
    }

    let mut encoded = Vec::new();
    crate::test_support::serialize_edited_history(
        decoded.ir(),
        decoded.source_fidelity(),
        &mut encoded,
    )
    .unwrap();
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();
    let native = &sldprt_native(regenerated.ir()).feature_histories[0].features;
    assert_eq!(native[3].properties["EndCondition"], "OneSided");
    assert_eq!(native[3].properties["Operation"], "Intersect");
    assert_eq!(native[3].properties["Profile"], "40");
    assert_eq!(native[3].parameters["Angle"], "0.75rad");
    // D1: explicit OneSided removes its use, not the independent authored angle.
    assert!(native[3].parameters.contains_key("Angle2"));
    let angle2 = regenerated
        .ir()
        .model
        .parameters
        .iter()
        .find(|parameter| parameter.id == angle2_id)
        .expect("same independent second-angle parameter");
    assert_eq!(angle2.name, "Angle2");
    assert!(
        matches!(&angle2.value, Some(cadmpeg_ir::features::ParameterValue::Angle(value))
        if (value.get() - 60.0_f64.to_radians()).abs() < EPS_TWO_SIDED_REVOLUTION_SECOND_ANGLE)
    );
    assert!(
        matches!(regenerated.ir().model.features[3].definition().operation(),
        FeatureOperation::Revolve { construction, op: BooleanOp::Intersect }
        if matches!(construction.extent(), Some(RevolveExtent::OneSided {
            termination: AngularTermination::Angle { angle }
        }) if regenerated.ir().dimension_value(angle).unwrap().get() == 0.75))
    );
}
