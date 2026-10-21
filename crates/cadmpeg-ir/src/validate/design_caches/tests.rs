// SPDX-License-Identifier: Apache-2.0
use crate::features::{
    BooleanOp, DatumPointConstruction, Dimension, ExtrudeDirection, ExtrudeExtent, ExtrudeSide,
    ExtrudeStart, ExtrusionDirectionSource, Feature, FeatureDefinition, FeatureDirection3,
    FeatureEvaluation, FeatureId, FeatureOperation, FinitePoint3, LinearTermination,
    PlanarProfileRef, ProfileRef, SketchPointSelection,
};
use crate::math::{Point2, Point3, Vector3};
use crate::report::{check::Check, Severity};
use crate::scalar::NonZeroLength;
use crate::sketches::{
    Sketch, SketchEntity, SketchEntityId, SketchGeometry, SketchGeometryDefinition, SketchId,
    SketchPlacement,
};
use crate::CadIr;

fn sketch_document() -> (CadIr, SketchId, SketchEntityId) {
    let mut ir = CadIr::empty();
    let sketch = SketchId::mint("test:model:sketch#source").unwrap();
    let point = SketchEntityId::mint("test:model:entity#point").unwrap();
    ir.model.sketches.push(Sketch {
        id: sketch.clone(),
        name: None,
        configuration: None,
        visible: None,
        placement: SketchPlacement::try_resolved(
            Point3::new(10.0, 20.0, 30.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
        )
        .unwrap(),
        profiles: crate::sketches::SketchProfiles::default(),
        native_ref: None,
    });
    ir.model.sketch_entities.push(SketchEntity::new(
        point.clone(),
        sketch.clone(),
        SketchGeometry::try_from(SketchGeometryDefinition::Point {
            position: Point2::new(1.0, 2.0),
        })
        .unwrap(),
    ));
    (ir, sketch, point)
}
fn feature(operation: FeatureOperation) -> Feature {
    Feature {
        id: FeatureId::mint("test:model:feature#resolved").unwrap(),
        ordinal: 0,
        name: None,
        suppressed: None,
        needs_recompute: false,
        dependencies: crate::features::DistinctMembers::default(),
        source_properties: std::collections::BTreeMap::default(),
        source_tag: None,
        source_text: None,
        source_content: crate::features::FeatureContent::default(),
        evaluation: FeatureEvaluation::from_definition(FeatureDefinition::Operation(operation)),
        native_ref: None,
    }
}

#[test]
fn datum_point_cache_uses_the_neutral_sketch_placement_and_explicit_stale_state() {
    let (mut ir, sketch, point) = sketch_document();
    ir.model
        .features
        .push(feature(FeatureOperation::DatumPoint {
            position: FinitePoint3::new(Point3::new(11.0, 23.0, 30.0)).unwrap(),
            construction: Some(Box::new(DatumPointConstruction::SketchPoint {
                point: SketchPointSelection::Planar {
                    sketch,
                    point,
                    native: "source point".into(),
                },
            })),
        }));
    let report = crate::validate::validate_neutral(&ir, vec![]).unwrap();
    assert!(report
        .findings
        .iter()
        .any(|finding| finding.check == Check::GeometricConsistency
            && finding.severity == Severity::Error
            && finding.message.contains("datum-point position")));
    ir.model.features[0].needs_recompute = true;
    assert!(!crate::validate::validate_neutral(&ir, vec![])
        .unwrap()
        .findings
        .iter()
        .any(|finding| finding.message.contains("datum-point position")));
    ir.model.features[0].needs_recompute = false;
    ir.model.features[0].evaluation.edit(|definition, _| {
        let FeatureDefinition::Operation(FeatureOperation::DatumPoint { position, .. }) =
            definition
        else {
            panic!("datum fixture")
        };
        *position = FinitePoint3::new(Point3::new(11.0, 22.0, 30.0)).unwrap();
    });
    assert!(crate::validate::validate_neutral(&ir, vec![])
        .unwrap()
        .is_ok());
}

#[test]
fn explicit_profile_normal_cache_is_oriented_and_stale_exceptions_belong_to_its_feature() {
    let (mut ir, sketch, _) = sketch_document();
    ir.model.features.push(feature(FeatureOperation::Extrude {
        profile: ProfileRef::Planar(PlanarProfileRef::Sketch(sketch)),
        direction: ExtrudeDirection::Explicit {
            vector: FeatureDirection3::new(Vector3::new(0.0, 0.0, -1.0)).unwrap(),
            source: Some(ExtrusionDirectionSource::ProfileNormal {}),
        },
        start: ExtrudeStart::default(),
        extent: ExtrudeExtent::OneSided {
            side: ExtrudeSide {
                termination: LinearTermination::Blind {
                    length: Dimension::Literal(NonZeroLength::new(5.0).unwrap()),
                },
                draft: None,
            },
        },
        op: BooleanOp::NewBody,
        solid: None,
        face_maker: None,
        inner_wire_taper: None,
        length_along_profile_normal: None,
        allow_multi_profile_faces: None,
    }));
    assert!(crate::validate::validate_neutral(&ir, vec![])
        .unwrap()
        .findings
        .iter()
        .any(|finding| finding.message.contains("extrusion direction")));
    ir.model.features[0].needs_recompute = true;
    assert!(!crate::validate::validate_neutral(&ir, vec![])
        .unwrap()
        .findings
        .iter()
        .any(|finding| finding.message.contains("extrusion direction")));
}
