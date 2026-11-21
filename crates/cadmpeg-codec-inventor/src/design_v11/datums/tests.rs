// SPDX-License-Identifier: Apache-2.0
use super::*;
use crate::test_support::root_part_ir;
use cadmpeg_core::decode::{DecodeArena, DecodePolicy};
use cadmpeg_ir::design::names::NativeNameValue;
use cadmpeg_ir::design::selection::{Anchor, OrderedSelection, Picks};
use cadmpeg_ir::features::{
    DatumPointConstruction, DistinctMembers, EdgeSelection, Feature, FeatureContent,
    FeatureCoordinateFrame, FeatureDefinition, FeatureEvaluation, FeatureId, FinitePoint3,
    SelectionReference, SketchFeatureBinding, SketchPointSelection, UnresolvedFamily,
    VertexSelection,
};
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::scalar::Length;

fn project_operation(operation: LegacyOperation) -> FeatureOperation {
    let mut ir = root_part_ir();
    ir.model.features.push(Feature {
        product_definition: None,
        id: FeatureId::mint("inventor:design:feature#datum").expect("id"),
        ordinal: 0,
        name: Some("Datum".into()),
        suppressed: None,
        dependencies: DistinctMembers::default(),
        source_properties: BTreeMap::default(),
        source_tag: None,
        source_text: None,
        source_content: FeatureContent::default(),
        evaluation: FeatureEvaluation::from_definition(FeatureDefinition::Operation(operation)),
        native_ref: Some("inventor:pmdc:datum#test".into()),
    });
    let original = ir.model.clone();
    let arena = DecodeArena::new();
    let policy = DecodePolicy::service();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
    crate::design_v11::project_history(&ctx, &mut ir, &BTreeMap::default()).expect("history");
    project(&ctx, &mut ir).expect("datums");
    assert_eq!(ir.model, original);
    assert_eq!(ir.design.features[0].id, ir.model.features[0].id);
    assert_eq!(
        ir.design.features[0].native_ref,
        ir.model.features[0].native_ref
    );
    assert_eq!(ir.design.features[0].name, ir.model.features[0].name);
    ir.design.features.remove(0).definition.operation
}

fn projected(operation: LegacyOperation) -> DatumElement {
    let FeatureOperation::Datum(datum) = project_operation(operation) else {
        panic!("datum operation")
    };
    assert!(datum.link.is_none());
    assert!(datum.native_options.is_empty());
    assert!(datum.source_settings.is_empty());
    datum.element
}

fn plane() -> FeatureDatumPlaneFrame {
    FeatureDatumPlaneFrame::new(
        Point3::new(2.0, 3.0, 4.0),
        Vector3::new(0.0, 0.0, 1.0),
        Vector3::new(1.0, 0.0, 0.0),
    )
    .expect("frame")
}

fn picks(input: &ConstructionInput) -> &OrderedSelection {
    let Some(InputReference::Picks(selection)) = &input.reference else {
        panic!("selection")
    };
    assert_eq!(selection.at, Anchor::Input);
    selection
}

fn picked(input: &ConstructionInput) -> &cadmpeg_ir::design::selection::Picked {
    let Picks::Named(members) = &picks(input).picks else {
        panic!("named picks")
    };
    assert_eq!(members.as_slice().len(), 1);
    &members.as_slice()[0]
}

fn native_vertex(key: &str) -> VertexSelection {
    VertexSelection::Native(
        SelectionReference::new(format!("inventor:vertex#{key}")).expect("reference"),
    )
}

#[test]
fn work_plane_keeps_the_read_frame() {
    let frame = plane();
    let DatumElement::Plane(value) = projected(LegacyOperation::DatumPlane { frame }) else {
        panic!("plane")
    };
    assert_eq!(
        value.resolved,
        Some(PlaneValue {
            origin: frame.origin(),
            normal: UnitVector3::Z_AXIS,
            x_axis: UnitVector3::X_AXIS
        })
    );
    assert!(value.inputs.is_empty());
    assert!(value.after.is_empty());
    assert!(value.mode.is_none());
    assert!(value.solution.is_none());
    assert!(value.resolved_in.is_empty());
}

#[test]
fn offset_plane_keeps_the_support_and_signed_distance() {
    let feature = FeatureId::mint("inventor:design:feature#base-plane").expect("id");
    let distance = Length::new(-12.0).expect("distance");
    let DatumElement::Plane(value) = projected(LegacyOperation::DatumOffsetPlane {
        reference: Some(DatumPlaneReference::Feature {
            feature: feature.clone(),
        }),
        distance,
    }) else {
        panic!("plane")
    };
    assert_eq!(value.inputs.len(), 1);
    let offset = &value.inputs[0];
    assert_eq!(
        offset.role,
        Some(InputRole {
            subject: Subject::Whole,
            relation: Relation::Offset
        })
    );
    assert_eq!(
        offset.value,
        Some(ScalarDimension::Length(Dimension::Literal(distance)))
    );
    assert_eq!(
        picked(offset).target,
        PickTarget::Datum {
            feature,
            part: None
        }
    );
    assert!(offset.reversed.is_none());
    assert!(value.resolved.is_none());
}

#[test]
fn offset_plane_keeps_a_read_face_selection() {
    let DatumElement::Plane(value) = projected(LegacyOperation::DatumOffsetPlane {
        reference: Some(DatumPlaneReference::Face {
            face: cadmpeg_ir::features::FaceSelection::Native("inventor:face#support".into()),
        }),
        distance: Length::new(8.0).expect("distance"),
    }) else {
        panic!("plane")
    };
    assert_eq!(
        picked(&value.inputs[0])
            .native
            .as_ref()
            .expect("native")
            .value,
        NativeNameValue::Text(cadmpeg_core::nonblank_literal!("inventor:face#support"))
    );
}

#[test]
fn offset_plane_keeps_a_read_support_frame() {
    let support = cadmpeg_ir::features::FeatureSupportPlaneFrame::new(
        Point3::new(2.0, 3.0, 4.0),
        Vector3::new(0.0, 0.0, 1.0),
        Vector3::new(1.0, 0.0, 0.0),
    )
    .expect("frame");
    let DatumElement::Plane(value) = projected(LegacyOperation::DatumOffsetPlane {
        reference: Some(DatumPlaneReference::ResolvedPlane { frame: support }),
        distance: Length::new(8.0).expect("distance"),
    }) else {
        panic!("plane")
    };
    let Some(InputReference::Built(reference)) = &value.inputs[0].reference else {
        panic!("built")
    };
    let AnyConstruction::Plane(reference) = reference.as_ref() else {
        panic!("plane")
    };
    assert_eq!(
        reference.resolved.as_ref().expect("resolved").origin,
        support.origin()
    );
    assert!(value.resolved.is_none());
}

#[test]
fn three_point_plane_keeps_the_operand_order_and_solved_frame() {
    let frame = plane();
    let points = Box::new([native_vertex("a"), native_vertex("b"), native_vertex("c")])
        .try_into()
        .expect("points");
    let DatumElement::Plane(value) =
        projected(LegacyOperation::DatumThreePointPlane { frame, points })
    else {
        panic!("plane")
    };
    assert_eq!(
        value.resolved.as_ref().expect("resolved").origin,
        frame.origin()
    );
    assert_eq!(value.inputs.len(), 3);
    for (input, key) in value.inputs.iter().zip(["a", "b", "c"]) {
        assert_eq!(
            input.role,
            Some(InputRole {
                subject: Subject::Whole,
                relation: Relation::Coincident
            })
        );
        assert_eq!(
            picked(input).native.as_ref().expect("native").value,
            NativeNameValue::Text(
                cadmpeg_core::text::NonBlankString::new(format!("inventor:vertex#{key}"))
                    .expect("name")
            )
        );
    }
}

#[test]
fn principal_planes_pick_the_definition_planes() {
    for (plane, entity) in [
        (PrincipalPlane::Front, PrincipalEntity::XyPlane),
        (PrincipalPlane::Top, PrincipalEntity::XzPlane),
        (PrincipalPlane::Right, PrincipalEntity::YzPlane),
    ] {
        let DatumElement::Plane(value) = projected(LegacyOperation::DatumPrincipalPlane { plane })
        else {
            panic!("plane")
        };
        assert_eq!(value.inputs.len(), 1);
        assert_eq!(
            value.inputs[0].role,
            Some(InputRole {
                subject: Subject::Whole,
                relation: Relation::Coincident
            })
        );
        assert_eq!(
            picked(&value.inputs[0]).target,
            PickTarget::Principal {
                entity,
                container: None
            }
        );
        assert!(value.after.is_empty());
        assert!(value.resolved.is_none());
    }
}

#[test]
fn axis_keeps_its_origin_and_normalized_direction() {
    let origin = FinitePoint3::new(Point3::new(1.0, 2.0, 3.0)).expect("point");
    let direction = cadmpeg_ir::features::FeatureDirection3::new(Vector3::new(0.0, 5.0, 0.0))
        .expect("direction");
    let DatumElement::Axis(value) = projected(LegacyOperation::datum_axis(origin, direction))
    else {
        panic!("axis")
    };
    assert_eq!(
        value.resolved,
        Some(AxisValue {
            origin,
            direction: UnitVector3::Y_AXIS
        })
    );
    assert!(value.inputs.is_empty());
}

#[test]
fn fixed_points_and_origin_keep_the_read_position() {
    for raw in [Point3::new(1.0, 2.0, 3.0), Point3::new(0.0, 0.0, 0.0)] {
        let position = FinitePoint3::new(raw).expect("position");
        let DatumElement::Points(values) = projected(LegacyOperation::DatumPoint {
            position,
            construction: None,
        }) else {
            panic!("points")
        };
        assert_eq!(values.len(), 1);
        assert_eq!(values[0].resolved, Some(position));
        assert!(values[0].inputs.is_empty());
    }
}

#[test]
fn coordinate_system_keeps_all_four_read_vectors() {
    let frame = FeatureCoordinateFrame::new(
        Point3::new(3.0, 4.0, 5.0),
        Vector3::new(0.0, 1.0, 0.0),
        Vector3::new(-1.0, 0.0, 0.0),
        Vector3::new(0.0, 0.0, 1.0),
    )
    .expect("frame");
    let DatumElement::Frame(value) = projected(LegacyOperation::DatumCoordinateSystem { frame })
    else {
        panic!("frame")
    };
    assert_eq!(
        value.resolved,
        Some(FrameValue {
            origin: frame.origin(),
            x_axis: frame.x_axis(),
            y_axis: frame.y_axis(),
            z_axis: frame.z_axis()
        })
    );
    assert!(value.inputs.is_empty());
}

#[test]
fn point_rules_keep_their_read_relations() {
    let edge = EdgeSelection::Native("inventor:edge#first".into());
    let second = EdgeSelection::Native("inventor:edge#second".into());
    let base = DatumPlaneReference::Feature {
        feature: FeatureId::mint("inventor:design:feature#base").expect("id"),
    };
    for (rule, relations) in [
        (
            DatumPointConstruction::CircleCenter { edge: edge.clone() },
            vec![Relation::Center],
        ),
        (
            DatumPointConstruction::TwoEdgeIntersection {
                edges: [edge.clone(), second],
            },
            vec![Relation::Coincident, Relation::Coincident],
        ),
        (
            DatumPointConstruction::ThreePlaneIntersection {
                planes: Box::new([base.clone(), base.clone(), base.clone()]),
            },
            vec![
                Relation::Coincident,
                Relation::Coincident,
                Relation::Coincident,
            ],
        ),
        (
            DatumPointConstruction::Vertex {
                vertex: native_vertex("point"),
            },
            vec![Relation::Coincident],
        ),
        (
            DatumPointConstruction::EdgePlaneIntersection {
                edge: edge.clone(),
                plane: base,
            },
            vec![Relation::Coincident, Relation::Coincident],
        ),
        (
            DatumPointConstruction::DistanceOnEdge {
                edge,
                fraction: cadmpeg_ir::scalar::Fraction::new(0.25).expect("fraction"),
            },
            vec![Relation::AtParameter],
        ),
    ] {
        let position = FinitePoint3::new(Point3::new(1.0, 2.0, 3.0)).expect("position");
        let DatumElement::Points(values) = projected(LegacyOperation::DatumPoint {
            position,
            construction: Some(Box::new(rule)),
        }) else {
            panic!("points")
        };
        let value = &values[0];
        assert_eq!(value.resolved, Some(position));
        assert_eq!(
            value
                .inputs
                .iter()
                .map(|input| input.role.as_ref().expect("role").relation.clone())
                .collect::<Vec<_>>(),
            relations
        );
        if value.inputs[0].role.as_ref().expect("role").relation == Relation::AtParameter {
            assert_eq!(
                value.inputs[0].value,
                Some(ScalarDimension::Ratio(Dimension::Literal(
                    cadmpeg_ir::scalar::FiniteReal::new(0.25).expect("ratio")
                )))
            );
        }
    }
}

#[test]
fn unread_and_sketch_operands_keep_datums_unresolved() {
    let position = FinitePoint3::new(Point3::new(1.0, 2.0, 3.0)).expect("position");
    let point = cadmpeg_ir::sketches::SketchEntityId::mint("inventor:design:sketch-entity#point")
        .expect("id");
    let sketch = cadmpeg_ir::sketches::SketchId::mint("inventor:design:sketch#plane").expect("id");
    let incomplete = [
        LegacyOperation::DatumOffsetPlane {
            reference: None,
            distance: Length::new(4.0).expect("distance"),
        },
        LegacyOperation::DatumThreePointPlane {
            frame: plane(),
            points: Box::new([
                native_vertex("a"),
                native_vertex("b"),
                VertexSelection::Unresolved,
            ])
            .try_into()
            .expect("points"),
        },
        LegacyOperation::DatumPoint {
            position,
            construction: Some(Box::new(DatumPointConstruction::Vertex {
                vertex: VertexSelection::Unresolved,
            })),
        },
        LegacyOperation::DatumPoint {
            position,
            construction: Some(Box::new(DatumPointConstruction::SketchPoint {
                point: SketchPointSelection::Planar {
                    sketch,
                    point,
                    native: "inventor:point#native".into(),
                },
            })),
        },
        LegacyOperation::Unresolved {
            family: UnresolvedFamily::DatumAxis,
        },
    ];
    for operation in incomplete {
        assert_eq!(
            project_operation(operation),
            FeatureOperation::Unresolved {
                family: OperationFamily::Datum
            }
        );
    }
}

#[test]
fn sketch_features_keep_the_skeleton_operation() {
    assert_eq!(
        project_operation(LegacyOperation::Sketch {
            sketch: SketchFeatureBinding::Unresolved
        }),
        FeatureOperation::Unresolved {
            family: OperationFamily::Sketch
        }
    );
}
