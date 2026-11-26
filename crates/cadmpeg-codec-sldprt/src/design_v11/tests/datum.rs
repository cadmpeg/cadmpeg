// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, HashSet};

use cadmpeg_ir::design::construction::{
    AxisValue, ConstructionInput, FrameValue, InputReference, PlaneValue, Relation, ScalarDimension,
};
use cadmpeg_ir::design::dimension::Dimension;
use cadmpeg_ir::design::feature::{FeatureOperation, OperationFamily};
use cadmpeg_ir::design::ops::datum::DatumElement;
use cadmpeg_ir::design::selection::{Anchor, PickTarget, Picks, Selection};
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::features::datums::{DatumAxis, DatumPlaneReference};
use cadmpeg_ir::features::geometry::{
    FeatureCoordinateFrame, FeatureDatumPlaneFrame, FeatureDirection3, FinitePoint3,
};
use cadmpeg_ir::features::parameters::{
    ConfigurationEvaluation, ConfigurationFeatureState, DesignConfiguration,
};
use cadmpeg_ir::features::selections::DistinctMembers;
use cadmpeg_ir::features::{ConfigurationId, FeatureDefinition, FeatureOperation as V10Operation};
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::scalar::Length;
use cadmpeg_ir::units::UnitVector3;

use super::{assert_no_design_findings, part_with_history, timeline_feature, typed_timeline};
use crate::loss::SldprtLossCode;
use crate::native::SldprtNative;
use crate::records::{
    FeatureInputClass, CONSTRUCTED_MID_PLANE_U_AXIS_SOURCE, REFERENCE_PLANE_U_AXIS_SOURCE_PROPERTY,
};

/// The decoded plane through (0, 0, `z`) mm with normal 2·+Z and u axis +X.
fn plane_at(z: f64) -> FeatureDefinition {
    FeatureDefinition::Operation(V10Operation::DatumPlane {
        frame: FeatureDatumPlaneFrame::new(
            Point3::new(0.0, 0.0, z),
            Vector3::new(0.0, 0.0, 2.0),
            Vector3::new(1.0, 0.0, 0.0),
        )
        .unwrap(),
    })
}

/// A part whose history holds these user datums:
/// - `Plane1`, decoded through (0, 0, 5) mm with normal 2·+Z and u axis +X;
/// - `Axis1`, decoded through (1, 2, 3) mm along 3·+Y;
/// - `Plane2`, with no decoded geometry;
/// - `Plane3`, decoded as `Plane1` offset by 10 mm;
/// - `Point1`, decoded at (1, 1, 1) mm;
/// - `Csys1`, decoded with origin (4, 5, 6) mm and axes +Y, −X, +Z;
/// - `Plane4`, decoded as `Plane1` with a constructed u axis;
/// - `Plane5`, decoded as `Plane1`, whose record states `D1` as 30 degrees.
fn part() -> (CadIr, SldprtNative) {
    let keywords = br#"<Keywords Name="Datums">
        <Feature Name="Front" id="2"/>
        <Feature Name="Top" id="3"/>
        <Feature Name="Right" id="4"/>
        <Sketch Name="Origin" Type="OriginProfileFeature" id="106"/>
        <Feature Name="Plane1" Type="RefPlane" id="30"/>
        <Feature Name="Axis1" Type="RefAxis" id="31"/>
        <Feature Name="Plane2" Type="RefPlane" id="32"/>
        <Feature Name="Plane3" Type="RefPlane" id="33"/>
        <Feature Name="Point1" Type="RefPoint" id="34"/>
        <Feature Name="Csys1" Type="CoordSys" id="35"/>
        <Feature Name="Plane4" Type="RefPlane" id="36"/>
        <Feature Name="Plane5" Type="RefPlane" id="37">
            <Dimension Name="D1">30deg</Dimension>
        </Feature>
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
            ("moRefPlane_c", "Plane3", 33),
            ("moRefPoint_c", "Point1", 34),
            ("moCoordSys_c", "Csys1", 35),
            ("moRefPlane_c", "Plane4", 36),
            ("moRefPlane_c", "Plane5", 37),
        ],
    );
    let native =
        SldprtNative::load(ir.native.namespace("sldprt").unwrap(), ir.tolerances.linear).unwrap();
    let plane1 = ir
        .model
        .features
        .iter()
        .find(|feature| feature.name.as_deref() == Some("Plane1"))
        .unwrap()
        .id
        .clone();
    for feature in &mut ir.model.features {
        let definition = match feature.name.as_deref() {
            Some("Plane1" | "Plane4" | "Plane5") => plane_at(5.0),
            Some("Axis1") => FeatureDefinition::Operation(V10Operation::DatumAxis {
                axis: DatumAxis::complete(
                    FinitePoint3::new(Point3::new(1.0, 2.0, 3.0)).unwrap(),
                    FeatureDirection3::new(Vector3::new(0.0, 3.0, 0.0)).unwrap(),
                ),
            }),
            Some("Plane3") => FeatureDefinition::Operation(V10Operation::DatumOffsetPlane {
                reference: Some(DatumPlaneReference::Feature {
                    feature: plane1.clone(),
                }),
                distance: Length::new(10.0).unwrap(),
            }),
            Some("Point1") => FeatureDefinition::Operation(V10Operation::DatumPoint {
                position: FinitePoint3::new(Point3::new(1.0, 1.0, 1.0)).unwrap(),
                construction: None,
            }),
            Some("Csys1") => FeatureDefinition::Operation(V10Operation::DatumCoordinateSystem {
                frame: FeatureCoordinateFrame::new(
                    Point3::new(4.0, 5.0, 6.0),
                    Vector3::new(0.0, 1.0, 0.0),
                    Vector3::new(-1.0, 0.0, 0.0),
                    Vector3::new(0.0, 0.0, 1.0),
                )
                .unwrap(),
            }),
            _ => continue,
        };
        if feature.name.as_deref() == Some("Plane4") {
            feature.source_properties.insert(
                cadmpeg_core::nonblank_const!(REFERENCE_PLANE_U_AXIS_SOURCE_PROPERTY),
                CONSTRUCTED_MID_PLANE_U_AXIS_SOURCE.to_owned(),
            );
        }
        feature.evaluation.edit(|current, _| *current = definition);
    }
    (ir, native)
}

/// The datum element of the typed timeline feature `name`.
fn element<'a>(timeline: &'a super::super::tree::Timeline, name: &str) -> &'a DatumElement {
    let FeatureOperation::Datum(datum) = &timeline_feature(timeline, name).definition.operation
    else {
        panic!("{name} is a typed datum");
    };
    &datum.element
}

/// Asserts that `inputs` are one input whose role is unread and whose
/// reference is an unread selection.
fn assert_unread(inputs: &[ConstructionInput]) {
    let [input] = inputs else {
        panic!("one input: {inputs:?}");
    };
    assert_eq!(input.role, None, "{input:?}");
    assert!(
        matches!(
            &input.reference,
            Some(InputReference::Picks(Selection {
                at: Anchor::Input,
                picks: Picks::Unread,
                native: None,
            }))
        ),
        "{input:?}"
    );
}

/// Declares the plane-data class `class` in the object of the reference
/// plane `plane` in the feature-input lane that names it.
fn declare(native: &mut SldprtNative, plane: &str, class: &str) {
    let lane = native
        .feature_input_lanes
        .iter_mut()
        .find(|lane| lane.names.iter().any(|name| name.value == plane))
        .unwrap();
    let offset = lane
        .names
        .iter()
        .find(|name| name.value == plane)
        .unwrap()
        .offset
        + 1;
    lane.classes.push(FeatureInputClass {
        id: format!("test:class:{offset}"),
        parent: lane.id.clone(),
        ordinal: 0,
        offset,
        name: class.to_owned(),
    });
}

/// The timeline and typed records of `ir`, typed with the feature-input
/// lanes of `native`.
fn typed_with_lanes(
    ir: &CadIr,
    native: &SldprtNative,
) -> (super::super::tree::Timeline, super::super::typed::Typed) {
    let mut timeline = super::super::tree::timeline(&native.feature_histories).unwrap();
    let entries =
        super::super::parameters::Entries::new(&native.feature_histories, &timeline).unwrap();
    let typed = super::super::typed::project(
        &ir.model,
        &native.feature_histories,
        &native.feature_input_lanes,
        &mut timeline,
        &entries,
        &super::super::typed::Scope {
            definition: &ir.design.scopes[0].definition,
            outside_profile: &HashSet::new(),
            surfaces: &ir.geometry.surfaces,
            angular: ir.tolerances.angular.get(),
        },
    );
    (timeline, typed)
}

/// Gives `name` the definition `definition` in a configuration that is not
/// active.
fn configure(ir: &mut CadIr, name: &str, definition: FeatureDefinition) {
    let feature = ir
        .model
        .features
        .iter()
        .find(|feature| feature.name.as_deref() == Some(name))
        .unwrap()
        .id
        .clone();
    ir.model.configurations.push(DesignConfiguration {
        id: ConfigurationId::mint("sldprt:model:configuration#9:1").unwrap(),
        ordinal: 1,
        active: false,
        source_index: Some(1),
        name: None,
        material: None,
        properties: BTreeMap::default(),
        parameter_overrides: BTreeMap::default(),
        bodies: None,
        parameter_values: BTreeMap::default(),
        feature_states: [(
            feature,
            ConfigurationFeatureState {
                evaluation: ConfigurationEvaluation::Active {
                    outputs: DistinctMembers::default(),
                },
                dependencies: DistinctMembers::default(),
                definition,
            },
        )]
        .into_iter()
        .collect(),
        native_ref: None,
    });
}

/// The note of `code` among the typed datum notes.
fn note(typed: &super::super::typed::Typed, code: SldprtLossCode) -> String {
    typed
        .datums
        .iter()
        .find(|note| note.code == code.kind())
        .unwrap_or_else(|| panic!("a {} note", code.kind()))
        .message
        .clone()
}

#[test]
fn a_decoded_reference_plane_and_axis_are_typed_datums() {
    let (mut ir, native) = part();
    let (timeline, entries, typed) = typed_timeline(&ir, &native);
    let DatumElement::Plane(plane) = element(&timeline, "Plane1") else {
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
    let DatumElement::Axis(axis) = element(&timeline, "Axis1") else {
        panic!("an axis datum");
    };
    assert_eq!(
        axis.resolved,
        Some(AxisValue {
            origin: FinitePoint3::new(Point3::new(1.0, 2.0, 3.0)).unwrap(),
            direction: UnitVector3::normalized(Vector3::new(0.0, 1.0, 0.0)).unwrap(),
        })
    );
    assert_unread(&plane.inputs);
    assert_unread(&axis.inputs);
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
fn an_offset_plane_is_offset_from_the_datum_it_names_by_its_distance() {
    let (ir, native) = part();
    let (timeline, _, _) = typed_timeline(&ir, &native);
    let DatumElement::Plane(plane) = element(&timeline, "Plane3") else {
        panic!("a plane datum");
    };
    let [input] = plane.inputs.as_slice() else {
        panic!("one input: {:?}", plane.inputs);
    };
    assert_eq!(input.role.as_ref().unwrap().relation, Relation::Offset);
    assert_eq!(
        input.value,
        Some(ScalarDimension::Length(Dimension::Literal(
            Length::new(10.0).unwrap()
        )))
    );
    let Some(InputReference::Picks(selection)) = &input.reference else {
        panic!("a picked reference");
    };
    let Picks::Named(picks) = &selection.picks else {
        panic!("named picks");
    };
    let plane1 = &timeline_feature(&timeline, "Plane1").id;
    assert!(
        matches!(&picks.as_slice()[0].target, PickTarget::Datum { feature, part: None } if feature == plane1),
        "{picks:?}"
    );
}

#[test]
fn a_decoded_point_and_coordinate_system_are_typed_datums() {
    let (ir, native) = part();
    let (timeline, _, _) = typed_timeline(&ir, &native);
    let DatumElement::Points(points) = element(&timeline, "Point1") else {
        panic!("a point datum");
    };
    assert_eq!(
        points
            .iter()
            .map(|point| point.resolved)
            .collect::<Vec<_>>(),
        vec![Some(FinitePoint3::new(Point3::new(1.0, 1.0, 1.0)).unwrap())]
    );
    let DatumElement::Frame(frame) = element(&timeline, "Csys1") else {
        panic!("a frame datum");
    };
    assert_eq!(
        frame.resolved,
        Some(FrameValue {
            origin: FinitePoint3::new(Point3::new(4.0, 5.0, 6.0)).unwrap(),
            x_axis: UnitVector3::normalized(Vector3::new(0.0, 1.0, 0.0)).unwrap(),
            y_axis: UnitVector3::normalized(Vector3::new(-1.0, 0.0, 0.0)).unwrap(),
            z_axis: UnitVector3::normalized(Vector3::new(0.0, 0.0, 1.0)).unwrap(),
        })
    );
    assert_unread(&points[0].inputs);
    assert_unread(&frame.inputs);
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
    let message = note(&typed, SldprtLossCode::DesignDatumUnresolved);
    assert!(message.contains(plane2.id.as_str()), "{message}");
    assert!(!message.contains("\"Plane1\""), "{message}");
}

#[test]
fn a_plane_with_a_constructed_axis_is_typed_and_named() {
    let (ir, native) = part();
    let (timeline, _, typed) = typed_timeline(&ir, &native);
    assert!(matches!(
        element(&timeline, "Plane4"),
        DatumElement::Plane(_)
    ));
    let message = note(&typed, SldprtLossCode::DesignDatumPlaneAxisConstructed);
    assert!(message.contains("\"Plane4\""), "{message}");
    assert!(!message.contains("\"Plane1\""), "{message}");
}

#[test]
fn a_plane_with_an_angular_d1_is_typed_and_named() {
    let (ir, native) = part();
    let (timeline, _, typed) = typed_timeline(&ir, &native);
    assert!(matches!(
        element(&timeline, "Plane5"),
        DatumElement::Plane(_)
    ));
    let message = note(&typed, SldprtLossCode::DesignDatumPlaneAxisAngled);
    assert!(message.contains("\"Plane5\""), "{message}");
    assert!(!message.contains("\"Plane1\""), "{message}");
}

#[test]
fn a_datum_whose_geometry_differs_in_a_configuration_is_named() {
    let (mut ir, native) = part();
    let (_, _, typed) = typed_timeline(&ir, &native);
    assert!(typed
        .datums
        .iter()
        .all(|note| note.code != SldprtLossCode::DesignDatumConfigurationDropped.kind()));
    configure(&mut ir, "Plane1", plane_at(10.0));
    let (_, _, typed) = typed_timeline(&ir, &native);
    let message = note(&typed, SldprtLossCode::DesignDatumConfigurationDropped);
    assert!(message.contains("\"Plane1\""), "{message}");
    assert!(!message.contains("\"Plane5\""), "{message}");
}

#[test]
fn a_datum_with_no_typed_geometry_in_a_configuration_is_named() {
    let (mut ir, native) = part();
    let undecoded = ir
        .model
        .features
        .iter()
        .find(|feature| feature.name.as_deref() == Some("Plane2"))
        .unwrap()
        .evaluation
        .definition()
        .clone();
    configure(&mut ir, "Plane1", undecoded);
    let (_, _, typed) = typed_timeline(&ir, &native);
    let message = note(&typed, SldprtLossCode::DesignDatumConfigurationDropped);
    assert!(message.contains("\"Plane1\""), "{message}");
    assert!(!message.contains("\"Plane5\""), "{message}");
}

#[test]
fn a_plane_whose_plane_data_is_fixed_states_its_frame_with_no_inputs() {
    let (ir, mut native) = part();
    declare(&mut native, "Plane1", "moFixedRefPlnData_c");
    declare(&mut native, "Plane5", "moAbsoluteRefPlnData_c");
    declare(&mut native, "Plane4", "moFixedRefPlnData_c");
    declare(&mut native, "Plane4", "moConstraintMidPlaneRefplaneData_c");
    let (timeline, _) = typed_with_lanes(&ir, &native);
    for name in ["Plane1", "Plane5"] {
        let DatumElement::Plane(plane) = element(&timeline, name) else {
            panic!("{name} is a plane datum");
        };
        assert!(plane.inputs.is_empty(), "{name}: {:?}", plane.inputs);
        assert!(plane.resolved.is_some(), "{name}");
    }
    let DatumElement::Plane(plane) = element(&timeline, "Plane4") else {
        panic!("Plane4 is a plane datum");
    };
    assert_unread(&plane.inputs);
}

#[test]
fn a_plane_whose_plane_data_follows_references_states_an_unread_input() {
    let (ir, mut native) = part();
    declare(
        &mut native,
        "Plane1",
        "moConstraintCoincPlaneRefplaneData_c",
    );
    let (timeline, _) = typed_with_lanes(&ir, &native);
    for name in ["Plane1", "Plane5"] {
        let DatumElement::Plane(plane) = element(&timeline, name) else {
            panic!("{name} is a plane datum");
        };
        assert_unread(&plane.inputs);
        assert!(plane.resolved.is_some(), "{name}");
    }
}

#[test]
fn a_plane_or_frame_off_square_beyond_the_angular_tolerance_stays_unresolved() {
    let (mut ir, native) = part();
    let skew = 5e-10;
    assert!(ir.tolerances.angular.get() < skew);
    for feature in &mut ir.model.features {
        let definition = match feature.name.as_deref() {
            Some("Plane1") => FeatureDefinition::Operation(V10Operation::DatumPlane {
                frame: FeatureDatumPlaneFrame::new(
                    Point3::new(0.0, 0.0, 5.0),
                    Vector3::new(0.0, 0.0, 1.0),
                    Vector3::new(1.0, 0.0, skew),
                )
                .unwrap(),
            }),
            Some("Csys1") => FeatureDefinition::Operation(V10Operation::DatumCoordinateSystem {
                frame: FeatureCoordinateFrame::new(
                    Point3::new(4.0, 5.0, 6.0),
                    Vector3::new(1.0, 0.0, 0.0),
                    Vector3::new(skew, 1.0, 0.0),
                    Vector3::new(0.0, 0.0, 1.0),
                )
                .unwrap(),
            }),
            _ => continue,
        };
        feature.evaluation.edit(|current, _| *current = definition);
    }
    let (timeline, _, typed) = typed_timeline(&ir, &native);
    let message = note(&typed, SldprtLossCode::DesignDatumUnresolved);
    for name in ["Plane1", "Csys1"] {
        assert_eq!(
            timeline_feature(&timeline, name).definition.operation,
            FeatureOperation::Unresolved {
                family: OperationFamily::Datum
            },
            "{name}"
        );
        assert!(message.contains(&format!("\"{name}\"")), "{message}");
    }
}
