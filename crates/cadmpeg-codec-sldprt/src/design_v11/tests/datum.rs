// SPDX-License-Identifier: Apache-2.0

use cadmpeg_ir::design::construction::{AxisValue, PlaneValue};
use cadmpeg_ir::design::feature::{FeatureOperation, OperationFamily};
use cadmpeg_ir::design::ops::datum::DatumElement;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::features::datums::DatumAxis;
use cadmpeg_ir::features::geometry::{FeatureDatumPlaneFrame, FeatureDirection3, FinitePoint3};
use cadmpeg_ir::features::{FeatureDefinition, FeatureOperation as V10Operation};
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::units::UnitVector3;

use super::{assert_no_design_findings, part_with_history, timeline_feature, typed_timeline};
use crate::loss::SldprtLossCode;
use crate::native::SldprtNative;

/// A part whose history holds the user datums `Plane1`, `Axis1` and
/// `Plane2`: `Plane1` decoded through (0, 0, 5) mm with normal 2·+Z and u
/// axis +X, `Axis1` decoded through (1, 2, 3) mm along 3·+Y, and `Plane2`
/// with no decoded geometry.
fn part() -> (CadIr, SldprtNative) {
    let keywords = br#"<Keywords Name="Datums">
        <Feature Name="Front" id="2"/>
        <Feature Name="Top" id="3"/>
        <Feature Name="Right" id="4"/>
        <Sketch Name="Origin" Type="OriginProfileFeature" id="106"/>
        <Feature Name="Plane1" Type="RefPlane" id="30"/>
        <Feature Name="Axis1" Type="RefAxis" id="31"/>
        <Feature Name="Plane2" Type="RefPlane" id="32"/>
    </Keywords>"#;
    let mut ir = part_with_history(
        keywords,
        &[
            ("moRefPlane_c", "Front", 2),
            ("moRefPlane_c", "Top", 3),
            ("moRefPlane_c", "Right", 4),
            ("moOriginProfileFeature_c", "Origin", 106),
            ("moRefPlane_c", "Plane1", 30),
            ("moRefAxis_c", "Axis1", 31),
            ("moRefPlane_c", "Plane2", 32),
        ],
    );
    let native =
        SldprtNative::load(ir.native.namespace("sldprt").unwrap(), ir.tolerances.linear).unwrap();
    let plane = FeatureDefinition::Operation(V10Operation::DatumPlane {
        frame: FeatureDatumPlaneFrame::new(
            Point3::new(0.0, 0.0, 5.0),
            Vector3::new(0.0, 0.0, 2.0),
            Vector3::new(1.0, 0.0, 0.0),
        )
        .unwrap(),
    });
    let axis = FeatureDefinition::Operation(V10Operation::DatumAxis {
        axis: DatumAxis::complete(
            FinitePoint3::new(Point3::new(1.0, 2.0, 3.0)).unwrap(),
            FeatureDirection3::new(Vector3::new(0.0, 3.0, 0.0)).unwrap(),
        ),
    });
    for feature in &mut ir.model.features {
        let definition = match feature.name.as_deref() {
            Some("Plane1") => plane.clone(),
            Some("Axis1") => axis.clone(),
            _ => continue,
        };
        feature.evaluation.edit(|current, _| *current = definition);
    }
    (ir, native)
}

#[test]
fn a_decoded_reference_plane_and_axis_are_typed_datums() {
    let (mut ir, native) = part();
    let (timeline, entries, typed) = typed_timeline(&ir, &native);
    let FeatureOperation::Datum(plane) =
        &timeline_feature(&timeline, "Plane1").definition.operation
    else {
        panic!("Plane1 is a typed datum");
    };
    let DatumElement::Plane(plane) = &plane.element else {
        panic!("a plane datum");
    };
    assert_eq!(
        plane.resolved,
        Some(PlaneValue {
            origin: FinitePoint3::new(Point3::new(0.0, 0.0, 5.0)).unwrap(),
            normal: UnitVector3::normalized(Vector3::new(0.0, 0.0, 1.0)).unwrap(),
            x_axis: UnitVector3::normalized(Vector3::new(1.0, 0.0, 0.0)).unwrap(),
        })
    );
    let FeatureOperation::Datum(axis) = &timeline_feature(&timeline, "Axis1").definition.operation
    else {
        panic!("Axis1 is a typed datum");
    };
    let DatumElement::Axis(axis) = &axis.element else {
        panic!("an axis datum");
    };
    assert_eq!(
        axis.resolved,
        Some(AxisValue {
            origin: FinitePoint3::new(Point3::new(1.0, 2.0, 3.0)).unwrap(),
            direction: UnitVector3::normalized(Vector3::new(0.0, 1.0, 0.0)).unwrap(),
        })
    );
    let definition = ir.design.scopes[0].definition.clone();
    let (parameters, _, _) = entries.parameters(
        &definition,
        &super::super::parameters::Readings {
            decoded: super::super::parameters::Decoded::new(
                &ir.model.parameters,
                &[],
                &crate::resolved_features::assembly::ConfigurationMaps::new(),
            ),
            length_unit: Some(cadmpeg_ir::design::units::UnitSymbol::Mm),
            length_places: None,
            bound: typed.bound,
            measured: typed.measured,
            driven: typed.references,
            manager: std::collections::HashMap::new(),
            confirmed: std::collections::HashSet::new(),
        },
    );
    ir.design.features = timeline.features;
    ir.design.parameters = parameters;
    ir.design.sketches = typed.sketches.records;
    ir.design.sketch_entities = typed.sketches.entities;
    ir.design.sketch_constraints = typed.sketches.constraints;
    assert_no_design_findings(&ir);
}

#[test]
fn a_datum_with_no_decoded_geometry_stays_unresolved_and_is_named() {
    let (ir, native) = part();
    let (timeline, _, typed) = typed_timeline(&ir, &native);
    let plane2 = timeline_feature(&timeline, "Plane2");
    assert_eq!(
        plane2.definition.operation,
        FeatureOperation::Unresolved {
            family: OperationFamily::Datum
        }
    );
    let note = typed
        .datum_unresolved
        .expect("the undecoded datum is named");
    assert_eq!(note.code, SldprtLossCode::DesignDatumUnresolved.kind());
    assert!(
        note.message.contains(plane2.id.as_str()),
        "{}",
        note.message
    );
    assert!(!note.message.contains("\"Plane1\""), "{}", note.message);
}
