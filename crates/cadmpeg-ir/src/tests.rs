// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]
//! Unit tests for the IR: the worked cube validates clean, JSON round-trips,
//! and each validation check actually fires when its invariant is broken.

use crate::annotations::{ExactnessNote, Provenance};
use crate::document::Model;
use crate::examples::unit_cube;
use crate::geometry::{
    Curve, CurveGeometry, ProceduralSurface, ProceduralSurfaceDefinition, SurfaceGeometry,
};
use crate::ids::{CoedgeId, CurveId, EdgeId, ProceduralSurfaceId, SubdId, UnknownId};
use crate::math::{Point3, Vector3};
use crate::native::NativeRecord;
use crate::provenance::{Exactness, SourceObjectAssociation};
use crate::report::{Check, LossCategory, LossNote, Severity};
use crate::subd::{
    SubdEdge, SubdEdgeTag, SubdEdgeUse, SubdFace, SubdScheme, SubdSurface, SubdVertex,
    SubdVertexTag,
};
use crate::tessellation::TessellationChannel;
use crate::topology::Color;
use crate::unknown::UnknownRecord;
use crate::validate::validate;
use crate::{diff, CadIr, LossProvenance};
use serde::{de::DeserializeOwned, Serialize};
use std::fmt::Debug;

fn assert_base64_round_trip_and_rejection<T>(value: &T, field: &str)
where
    T: Serialize + DeserializeOwned + PartialEq + Debug,
{
    let mut json = serde_json::to_value(value).unwrap();
    assert_eq!(json[field], "AQID");
    assert_eq!(serde_json::from_value::<T>(json.clone()).unwrap(), *value);
    json[field] = serde_json::Value::String("%%%".into());
    assert!(serde_json::from_value::<T>(json).is_err());
}

/// Replace the surface of the cube's first face with an unknown surface,
/// optionally linking a preserved record, and return the face id and its
/// surface id. Leaves every loop/coedge/edge of the face intact.
fn make_first_face_surface_unknown(ir: &mut crate::CadIr, record: Option<UnknownId>) -> String {
    let face = &ir.model.faces[0];
    let surface_id = face.surface.0.clone();
    for s in &mut ir.model.surfaces {
        if s.id.0 == surface_id {
            s.geometry = SurfaceGeometry::Unknown { record };
            break;
        }
    }
    surface_id
}

#[test]
fn face_on_unknown_surface_validates_clean() {
    let mut ir = unit_cube();
    // Preserve a raw record and point the unknown surface at it.
    let rec = UnknownId("synthetic:cube:unknown#0".into());
    ir.push_native_unknown(
        "synthetic",
        UnknownRecord {
            id: rec.clone(),
            offset: 0,
            byte_len: 16,
            sha256: "0".repeat(64),
            data: None,
            links: Vec::new(),
        },
    )
    .unwrap();
    make_first_face_surface_unknown(&mut ir, Some(rec));

    let report = validate(&ir, Vec::new());
    assert!(
        report.is_ok(),
        "a face on an unknown surface is legal, got: {:?}",
        report.findings
    );
    // The face and its topology stay in the graph.
    assert_eq!(ir.model.faces.len(), 6);
    // The situation is surfaced as a count.
    assert_eq!(
        report.entity_counts.get("surfaces_unknown_geometry"),
        Some(&1)
    );
}

#[test]
fn unknown_surface_without_record_is_legal() {
    let mut ir = unit_cube();
    make_first_face_surface_unknown(&mut ir, None);
    let report = validate(&ir, Vec::new());
    assert!(
        report.is_ok(),
        "an unknown surface need not preserve bytes, got: {:?}",
        report.findings
    );
    assert_eq!(
        report.entity_counts.get("surfaces_unknown_geometry"),
        Some(&1)
    );
}

#[test]
fn unknown_surface_dangling_record_is_flagged() {
    let mut ir = unit_cube();
    // Link a record id that is not in the unknowns arena.
    make_first_face_surface_unknown(&mut ir, Some(UnknownId("missing".into())));
    let report = validate(&ir, Vec::new());
    assert!(
        report
            .findings
            .iter()
            .any(|f| f.check == Check::ReferentialIntegrity),
        "expected a referential-integrity finding for the dangling record, got: {:?}",
        report.findings
    );
    assert!(!report.is_ok());
}

#[test]
fn unknown_surface_json_round_trips() {
    let mut ir = unit_cube();
    let rec = UnknownId("synthetic:cube:unknown#0".into());
    ir.push_native_unknown(
        "synthetic",
        UnknownRecord {
            id: rec.clone(),
            offset: 0,
            byte_len: 16,
            sha256: "0".repeat(64),
            data: None,
            links: Vec::new(),
        },
    )
    .unwrap();
    make_first_face_surface_unknown(&mut ir, Some(rec));

    let json = ir.to_canonical_json().unwrap();
    let parsed = crate::CadIr::from_json(&json).unwrap();
    assert_eq!(parsed, ir, "round-trip must preserve the unknown surface");
}

#[test]
fn unit_cube_has_expected_census() {
    let ir = unit_cube();
    assert_eq!(ir.model.bodies.len(), 1);
    assert_eq!(ir.model.regions.len(), 1);
    assert_eq!(ir.model.shells.len(), 1);
    assert_eq!(ir.model.faces.len(), 6);
    assert_eq!(ir.model.loops.len(), 6);
    assert_eq!(ir.model.coedges.len(), 24);
    assert_eq!(ir.model.edges.len(), 12);
    assert_eq!(ir.model.vertices.len(), 8);
    assert_eq!(ir.model.points.len(), 8);
    assert_eq!(ir.model.surfaces.len(), 6);
    assert_eq!(ir.model.curves.len(), 12);
}

#[test]
fn unit_cube_validates_clean() {
    let ir = unit_cube();
    let report = validate(&ir, Vec::new());
    assert!(
        report.is_ok(),
        "cube should have no error findings, got: {:?}",
        report.findings
    );
    assert_eq!(report.error_count(), 0);
    assert_eq!(report.warning_count(), 0);
    assert_eq!(report.entity_counts.get("coedges"), Some(&24));
}

#[test]
fn arena_registry_drives_counts_and_diff_dispatch() {
    let ir = unit_cube();
    let report = validate(&ir, Vec::new());
    let diff_kinds = diff(&ir, &ir)
        .per_arena
        .into_iter()
        .map(|arena| arena.kind)
        .collect::<Vec<_>>();

    assert_eq!(
        &diff_kinds[..Model::arena_names().len()],
        Model::arena_names()
    );
    for name in Model::arena_names() {
        assert!(
            report.entity_counts.contains_key(*name),
            "entity counts omitted registered arena {name}"
        );
    }
}

#[test]
fn current_json_without_configurations_defaults_to_empty() {
    let ir = unit_cube();
    let mut value = serde_json::to_value(&ir).unwrap();
    value
        .get_mut("model")
        .and_then(serde_json::Value::as_object_mut)
        .unwrap()
        .remove("configurations");

    let decoded: CadIr = serde_json::from_value(value).unwrap();
    assert!(decoded.model.configurations.is_empty());
}

#[test]
fn current_json_without_parameters_defaults_to_empty() {
    let ir = unit_cube();
    let mut value = serde_json::to_value(&ir).unwrap();
    value
        .get_mut("model")
        .and_then(serde_json::Value::as_object_mut)
        .unwrap()
        .remove("parameters");

    let decoded: CadIr = serde_json::from_value(value).unwrap();
    assert!(decoded.model.parameters.is_empty());
}

#[test]
fn current_json_without_sketch_arenas_defaults_to_empty() {
    let ir = unit_cube();
    let mut value = serde_json::to_value(&ir).unwrap();
    let model = value
        .get_mut("model")
        .and_then(serde_json::Value::as_object_mut)
        .unwrap();
    model.remove("sketches");
    model.remove("sketch_entities");
    model.remove("sketch_constraints");

    let decoded: CadIr = serde_json::from_value(value).unwrap();
    assert!(decoded.model.sketches.is_empty());
    assert!(decoded.model.sketch_entities.is_empty());
    assert!(decoded.model.sketch_constraints.is_empty());
}

#[test]
fn malformed_sketch_geometry_and_constraints_are_rejected() {
    use crate::features::Length;
    use crate::math::{Point2, Point3, Vector3};
    use crate::sketches::{
        Sketch, SketchConstraint, SketchConstraintDefinition, SketchConstraintId, SketchEntity,
        SketchEntityId, SketchEntityUse, SketchGeometry, SketchId,
    };

    let mut ir = unit_cube();
    let sketch_id = SketchId("synthetic:test:sketch#0".into());
    let circle_id = SketchEntityId("synthetic:test:sketch-entity#0".into());
    let nurbs_id = SketchEntityId("synthetic:test:sketch-entity#1".into());
    ir.model.sketches.push(Sketch {
        id: sketch_id.clone(),
        name: None,
        configuration: None,
        origin: Point3::new(0.0, 0.0, 0.0),
        normal: Vector3::new(0.0, 0.0, 1.0),
        u_axis: Vector3::new(1.0, 0.0, 1.0),
        profiles: vec![vec![SketchEntityUse {
            entity: circle_id.clone(),
            reversed: false,
        }]],
        native_ref: None,
    });
    ir.model.sketch_entities.extend([
        SketchEntity {
            id: circle_id.clone(),
            sketch: sketch_id.clone(),
            construction: false,
            native_ref: None,
            geometry_ref: None,
            endpoint_refs: Vec::new(),
            geometry: SketchGeometry::Circle {
                center: Point2::new(0.0, 0.0),
                radius: Length(-1.0),
            },
        },
        SketchEntity {
            id: nurbs_id,
            sketch: sketch_id.clone(),
            construction: false,
            native_ref: None,
            geometry_ref: None,
            endpoint_refs: Vec::new(),
            geometry: SketchGeometry::Nurbs {
                degree: 3,
                knots: vec![0.0, 1.0],
                control_points: vec![Point2::new(0.0, 0.0)],
                weights: Some(vec![0.0]),
                periodic: false,
            },
        },
    ]);
    ir.model.sketch_constraints.push(SketchConstraint {
        id: SketchConstraintId("synthetic:test:sketch-constraint#0".into()),
        sketch: sketch_id,
        definition: SketchConstraintDefinition::Coincident {
            entities: vec![circle_id],
        },
    });
    ir.finalize();

    let report = validate(&ir, Vec::new());
    assert!(report.findings.iter().any(|finding| {
        finding.check == Check::GeometricConsistency
            && finding.entity.as_deref() == Some("synthetic:test:sketch#0")
    }));
    assert!(report.findings.iter().any(|finding| {
        finding.check == Check::Bounds
            && finding.entity.as_deref() == Some("synthetic:test:sketch-entity#0")
    }));
    assert!(report.findings.iter().any(|finding| {
        finding.check == Check::ParameterDomain
            && finding.entity.as_deref() == Some("synthetic:test:sketch-entity#1")
    }));
    assert!(report.findings.iter().any(|finding| {
        finding.check == Check::Counts
            && finding.entity.as_deref() == Some("synthetic:test:sketch-constraint#0")
    }));
}

#[test]
fn locus_aware_sketch_constraints_round_trip_and_validate_geometry() {
    use crate::features::ParameterId;
    use crate::math::{Point2, Point3, Vector3};
    use crate::sketches::{
        Sketch, SketchConstraint, SketchConstraintDefinition, SketchConstraintId, SketchEntity,
        SketchEntityId, SketchGeometry, SketchId, SketchLocus,
    };

    let entity = SketchEntityId("synthetic:test:entity#0".into());
    let parameter = ParameterId("synthetic:test:parameter#0".into());
    let definitions = vec![
        SketchConstraintDefinition::CoincidentLoci {
            loci: vec![
                SketchLocus::Start(entity.clone()),
                SketchLocus::Center(entity.clone()),
            ],
        },
        SketchConstraintDefinition::Midpoint {
            point: SketchLocus::End(entity.clone()),
            entity: entity.clone(),
        },
        SketchConstraintDefinition::Concentric {
            first: entity.clone(),
            second: entity.clone(),
        },
        SketchConstraintDefinition::Collinear {
            first: entity.clone(),
            second: entity.clone(),
        },
        SketchConstraintDefinition::Symmetric {
            first: SketchLocus::Start(entity.clone()),
            second: SketchLocus::End(entity.clone()),
            axis: entity.clone(),
        },
        SketchConstraintDefinition::Radius {
            entity: entity.clone(),
            parameter: parameter.clone(),
        },
        SketchConstraintDefinition::Diameter {
            entity: entity.clone(),
            parameter: parameter.clone(),
        },
        SketchConstraintDefinition::DistanceLoci {
            first: SketchLocus::Start(entity.clone()),
            second: SketchLocus::End(entity.clone()),
            parameter: parameter.clone(),
        },
        SketchConstraintDefinition::HorizontalDistance {
            first: SketchLocus::Start(entity.clone()),
            second: SketchLocus::End(entity.clone()),
            parameter: parameter.clone(),
        },
        SketchConstraintDefinition::VerticalDistance {
            first: SketchLocus::Start(entity.clone()),
            second: SketchLocus::End(entity.clone()),
            parameter,
        },
    ];
    let json = serde_json::to_string(&definitions).unwrap();
    assert_eq!(
        serde_json::from_str::<Vec<SketchConstraintDefinition>>(&json).unwrap(),
        definitions
    );

    let mut ir = unit_cube();
    let sketch = SketchId("synthetic:test:sketch#locus".into());
    ir.model.sketches.push(Sketch {
        id: sketch.clone(),
        name: None,
        configuration: None,
        origin: Point3::new(0.0, 0.0, 0.0),
        normal: Vector3::new(0.0, 0.0, 1.0),
        u_axis: Vector3::new(1.0, 0.0, 0.0),
        profiles: Vec::new(),
        native_ref: None,
    });
    ir.model.sketch_entities.push(SketchEntity {
        id: entity.clone(),
        sketch: sketch.clone(),
        construction: false,
        native_ref: None,
        geometry_ref: None,
        endpoint_refs: Vec::new(),
        geometry: SketchGeometry::Line {
            start: Point2::new(0.0, 0.0),
            end: Point2::new(1.0, 0.0),
        },
    });
    let constraint_id = SketchConstraintId("synthetic:test:constraint#locus".into());
    ir.model.sketch_constraints.push(SketchConstraint {
        id: constraint_id.clone(),
        sketch,
        definition: SketchConstraintDefinition::CoincidentLoci {
            loci: vec![
                SketchLocus::Center(entity.clone()),
                SketchLocus::Start(entity),
            ],
        },
    });
    ir.finalize();
    let report = validate(&ir, Vec::new());
    assert!(report.findings.iter().any(|finding| {
        finding.entity.as_deref() == Some(constraint_id.0.as_str())
            && finding.check == Check::GeometricConsistency
    }));
}

#[test]
fn sketch_profiles_and_constraints_enforce_local_connectivity() {
    use crate::math::{Point2, Point3, Vector3};
    use crate::sketches::{
        Sketch, SketchConstraint, SketchConstraintDefinition, SketchConstraintId, SketchEntity,
        SketchEntityId, SketchEntityUse, SketchGeometry, SketchId,
    };

    let mut ir = unit_cube();
    let first_sketch = SketchId("synthetic:test:sketch#first".into());
    let second_sketch = SketchId("synthetic:test:sketch#second".into());
    let first = SketchEntityId("synthetic:test:entity#first".into());
    let disconnected = SketchEntityId("synthetic:test:entity#disconnected".into());
    let foreign = SketchEntityId("synthetic:test:entity#foreign".into());
    let plane = |id: SketchId, profiles| Sketch {
        id,
        name: None,
        configuration: None,
        origin: Point3::new(0.0, 0.0, 0.0),
        normal: Vector3::new(0.0, 0.0, 1.0),
        u_axis: Vector3::new(1.0, 0.0, 0.0),
        profiles,
        native_ref: None,
    };
    ir.model.sketches.extend([
        plane(
            first_sketch.clone(),
            vec![vec![
                SketchEntityUse {
                    entity: first.clone(),
                    reversed: false,
                },
                SketchEntityUse {
                    entity: disconnected.clone(),
                    reversed: false,
                },
            ]],
        ),
        plane(second_sketch.clone(), Vec::new()),
    ]);
    let line = |id, sketch, start, end| SketchEntity {
        id,
        sketch,
        construction: false,
        native_ref: None,
        geometry_ref: None,
        endpoint_refs: Vec::new(),
        geometry: SketchGeometry::Line { start, end },
    };
    ir.model.sketch_entities.extend([
        line(
            first.clone(),
            first_sketch.clone(),
            Point2::new(0.0, 0.0),
            Point2::new(1.0, 0.0),
        ),
        line(
            disconnected,
            first_sketch.clone(),
            Point2::new(2.0, 0.0),
            Point2::new(3.0, 0.0),
        ),
        line(
            foreign.clone(),
            second_sketch,
            Point2::new(0.0, 0.0),
            Point2::new(1.0, 0.0),
        ),
    ]);
    let constraint = SketchConstraintId("synthetic:test:constraint#foreign".into());
    ir.model.sketch_constraints.push(SketchConstraint {
        id: constraint.clone(),
        sketch: first_sketch.clone(),
        definition: SketchConstraintDefinition::Parallel {
            first,
            second: foreign,
        },
    });
    ir.finalize();
    let report = validate(&ir, Vec::new());
    assert!(report.findings.iter().any(|finding| {
        finding.entity.as_deref() == Some(first_sketch.0.as_str())
            && finding.message.contains("disconnected consecutive")
    }));
    assert!(report.findings.iter().any(|finding| {
        finding.entity.as_deref() == Some(constraint.0.as_str())
            && finding.message.contains("different sketch")
    }));
}

#[test]
fn neutral_features_resolve_sketch_profile_and_path_operands() {
    use crate::features::{
        BooleanOp, Extent, Feature, FeatureDefinition, FeatureId, Length, PathRef, ProfileRef,
    };
    use crate::sketches::SketchId;

    let sketch = SketchId("synthetic:test:sketch#missing".into());
    let definitions = [
        FeatureDefinition::Extrude {
            profile: ProfileRef::Sketch(sketch.clone()),
            direction: None,
            extent: Extent::Blind {
                length: Length(10.0),
            },
            op: BooleanOp::NewBody,
            draft: None,
        },
        FeatureDefinition::Sweep {
            profile: ProfileRef::Sketch(sketch.clone()),
            path: PathRef::Sketch(sketch.clone()),
            op: BooleanOp::NewBody,
            twist: None,
            scale: None,
        },
    ];
    let json = serde_json::to_string(&definitions).unwrap();
    assert_eq!(
        serde_json::from_str::<[FeatureDefinition; 2]>(&json).unwrap(),
        definitions
    );

    let mut ir = unit_cube();
    ir.model.features.push(Feature {
        id: FeatureId("synthetic:test:feature#sketch-ref".into()),
        ordinal: 0,
        name: None,
        suppressed: false,
        parent: None,
        outputs: Vec::new(),
        definition: definitions[1].clone(),
        native_ref: None,
    });
    ir.finalize();
    let report = validate(&ir, Vec::new());
    assert_eq!(
        report
            .findings
            .iter()
            .filter(|finding| finding.message.contains("missing sketch"))
            .count(),
        2
    );
}

#[test]
fn feature_history_rejects_dangling_and_forward_dependencies() {
    use crate::features::{BooleanOp, Extent, Feature, FeatureDefinition, FeatureId, ProfileRef};
    use crate::ids::{BodyId, FaceId};

    let mut ir = unit_cube();
    let feature_id = FeatureId("synthetic:test:feature#invalid".into());
    ir.model.features.push(Feature {
        id: feature_id.clone(),
        ordinal: 0,
        name: None,
        suppressed: false,
        parent: Some(feature_id.clone()),
        outputs: vec![BodyId("synthetic:test:body#missing".into())],
        definition: FeatureDefinition::Extrude {
            profile: ProfileRef::Faces(vec![FaceId("synthetic:test:face#profile-missing".into())]),
            direction: None,
            extent: Extent::ToFace {
                face: FaceId("synthetic:test:face#termination-missing".into()),
            },
            op: BooleanOp::NewBody,
            draft: None,
        },
        native_ref: None,
    });
    ir.finalize();
    let report = validate(&ir, Vec::new());
    for fragment in [
        "does not precede",
        "missing output body",
        "missing profile face",
        "missing termination face",
    ] {
        assert!(
            report.findings.iter().any(|finding| {
                finding.entity.as_deref() == Some(feature_id.0.as_str())
                    && finding.message.contains(fragment)
            }),
            "missing finding containing {fragment:?}"
        );
    }
}

#[test]
fn configuration_body_membership_round_trips_and_validates() {
    use crate::features::{ConfigurationId, DesignConfiguration};
    use crate::ids::BodyId;
    use std::collections::BTreeMap;

    let mut ir = unit_cube();
    let configuration_id = ConfigurationId("synthetic:test:configuration#0".into());
    let body = ir.model.bodies[0].id.clone();
    ir.model.configurations.push(DesignConfiguration {
        id: configuration_id.clone(),
        name: "Default".into(),
        material: None,
        properties: BTreeMap::new(),
        bodies: vec![body.clone()],
        native_ref: None,
    });
    ir.finalize();
    assert!(validate(&ir, Vec::new()).is_ok());
    let round_trip = CadIr::from_json(&serde_json::to_string(&ir).unwrap()).unwrap();
    assert_eq!(round_trip.model.configurations[0].bodies, vec![body]);

    ir.model.configurations[0].bodies = vec![
        BodyId("synthetic:test:body#missing".into()),
        BodyId("synthetic:test:body#missing".into()),
    ];
    let report = validate(&ir, Vec::new());
    assert!(report.findings.iter().any(|finding| {
        finding.entity.as_deref() == Some(configuration_id.0.as_str())
            && finding.message.contains("missing configuration body")
    }));
    assert!(report.findings.iter().any(|finding| {
        finding.entity.as_deref() == Some(configuration_id.0.as_str())
            && finding.message.contains("repeats body")
    }));
}

#[test]
fn native_records_use_own_ids_for_counts_diff_and_validation() {
    let left = unit_cube();
    let mut right = left.clone();
    right.native.namespace_mut("f3d").arenas.insert(
        "act_guids".into(),
        vec![NativeRecord {
            id: "f3d:test:act-guid#0".into(),
            fields: serde_json::Map::new(),
        }],
    );
    right.native.namespace_mut("sldprt").arenas.insert(
        "configurations".into(),
        vec![NativeRecord {
            id: "sldprt:test:configuration#0".into(),
            fields: serde_json::Map::new(),
        }],
    );
    right.native.finalize();

    let result = diff(&left, &right);
    assert_eq!(
        result
            .per_arena
            .iter()
            .find(|arena| arena.kind == "native.f3d.act_guids")
            .unwrap()
            .added,
        ["f3d:test:act-guid#0"]
    );
    assert_eq!(
        result
            .per_arena
            .iter()
            .find(|arena| arena.kind == "native.sldprt.configurations")
            .unwrap()
            .added,
        ["sldprt:test:configuration#0"]
    );
    let report = validate(&right, Vec::new());
    assert_eq!(report.entity_counts["native.f3d.act_guids"], 1);
    assert_eq!(report.entity_counts["native.sldprt.configurations"], 1);
    assert!(report.is_ok(), "{:?}", report.findings);

    right
        .native
        .namespace_mut("sldprt")
        .arenas
        .get_mut("configurations")
        .unwrap()[0]
        .id = "f3d:test:act-guid#0".into();
    right.native.finalize();
    assert!(validate(&right, Vec::new())
        .findings
        .iter()
        .any(|finding| finding.message == "entity id is not globally unique"));
}

#[test]
fn every_cube_edge_has_two_opposite_sense_coedges() {
    let ir = unit_cube();
    for edge in &ir.model.edges {
        let coedges: Vec<_> = ir
            .model
            .coedges
            .iter()
            .filter(|c| c.edge == edge.id)
            .collect();
        assert_eq!(coedges.len(), 2, "edge {} should have 2 coedges", edge.id);
        assert_ne!(
            coedges[0].sense, coedges[1].sense,
            "edge {} coedges should have opposite sense",
            edge.id
        );
        // Partners point at each other.
        assert_eq!(coedges[0].radial_next, coedges[1].id);
        assert_eq!(coedges[1].radial_next, coedges[0].id);
    }
}

#[test]
fn json_round_trips_and_is_deterministic() {
    let ir = unit_cube();
    let json1 = ir.to_canonical_json().unwrap();
    let json2 = ir.to_canonical_json().unwrap();
    assert_eq!(json1, json2, "serialization must be deterministic");

    let parsed = crate::CadIr::from_json(&json1).unwrap();
    assert_eq!(parsed, ir, "round-trip must preserve the document");
    assert_eq!(parsed.to_canonical_json().unwrap(), json1);
}

#[test]
fn appearance_asset_and_binding_round_trip() {
    use crate::appearance::{Appearance, AppearanceBinding, AppearanceTarget};
    use crate::ids::AppearanceId;

    let mut ir = unit_cube();
    let body = ir.model.bodies[0].id.clone();
    ir.model.appearances.push(Appearance {
        id: AppearanceId("synthetic:test:appearance#prism-001".into()),
        name: Some("Prism-001".into()),
        asset_guid: Some("visual-guid".into()),
        visual_guid: Some("visual-guid".into()),
        physical_token: Some("physical-token".into()),
        schema: Some("GenericSchema".into()),
        category: None,
        base_color: Some(crate::topology::Color {
            r: 0.1,
            g: 0.2,
            b: 0.3,
            a: 1.0,
        }),
        properties: std::collections::BTreeMap::new(),
    });
    ir.model.appearance_bindings.push(AppearanceBinding {
        id: "synthetic:test:appearance-binding#0".into(),
        target: AppearanceTarget::Body(body),
        appearance: AppearanceId("synthetic:test:appearance#prism-001".into()),
        source_entity_id: Some("0_1".into()),
        object_type: Some("Body".into()),
        channels: std::collections::BTreeMap::new(),
    });

    let json = ir.to_canonical_json().unwrap();
    let decoded = CadIr::from_json(&json).unwrap();
    assert_eq!(decoded.model.appearances, ir.model.appearances);
    assert_eq!(
        decoded.model.appearance_bindings,
        ir.model.appearance_bindings
    );
}

#[test]
fn dangling_reference_is_flagged() {
    let mut ir = unit_cube();
    // Point a coedge's edge at something that does not exist.
    ir.model.coedges[0].edge = EdgeId("does-not-exist".into());
    let report = validate(&ir, Vec::new());
    assert!(report
        .findings
        .iter()
        .any(|f| f.check == Check::ReferentialIntegrity));
    assert!(!report.is_ok());
}

#[test]
fn broken_loop_ring_is_flagged() {
    let mut ir = unit_cube();
    // Redirect a coedge's `next` to a valid coedge in a different loop, so the
    // referenced id resolves but the ring no longer closes.
    let foreign = ir.model.coedges[20].id.clone();
    ir.model.coedges[0].next = foreign;
    let report = validate(&ir, Vec::new());
    assert!(
        report
            .findings
            .iter()
            .any(|f| f.check == Check::LoopClosure),
        "expected a loop-closure finding, got: {:?}",
        report.findings
    );
}

#[test]
fn mismatched_partner_edge_is_flagged() {
    let mut ir = unit_cube();
    // Force a coedge's partner to reference a coedge on a different edge by
    // repointing the partner's edge. Find coedge[0]'s partner and change it.
    let partner_id: CoedgeId = ir.model.coedges[0].radial_next.clone();
    let other_edge = ir
        .model
        .coedges
        .iter()
        .find(|c| c.edge != ir.model.coedges[0].edge)
        .unwrap()
        .edge
        .clone();
    for c in &mut ir.model.coedges {
        if c.id == partner_id {
            c.edge = other_edge.clone();
        }
    }
    let report = validate(&ir, Vec::new());
    assert!(
        report
            .findings
            .iter()
            .any(|f| f.check == Check::CoedgePairing),
        "expected a coedge-pairing finding, got: {:?}",
        report.findings
    );
}

#[test]
fn signed_sphere_radius_is_valid() {
    let mut ir = unit_cube();
    ir.model.surfaces[0].geometry = SurfaceGeometry::Sphere {
        center: Point3::new(0.0, 0.0, 0.0),
        axis: Vector3::new(0.0, 0.0, 1.0),
        ref_direction: Vector3::new(1.0, 0.0, 0.0),
        radius: -1.0,
    };
    let report = validate(&ir, Vec::new());
    assert!(report.is_ok(), "findings: {:?}", report.findings);
}

#[test]
fn degenerate_plane_normal_is_flagged() {
    let mut ir = unit_cube();
    if let SurfaceGeometry::Plane { normal, .. } = &mut ir.model.surfaces[0].geometry {
        *normal = Vector3::new(0.0, 0.0, 0.0);
    }
    let report = validate(&ir, Vec::new());
    assert!(report.findings.iter().any(|f| f.check == Check::Bounds));
}

#[test]
fn new_topology_references_are_validated() {
    let mut ir = unit_cube();
    ir.model.shells[0]
        .wire_edges
        .push(EdgeId("missing-wire".into()));
    ir.model.shells[0]
        .free_vertices
        .push(crate::ids::VertexId("missing-free".into()));
    ir.model.coedges[0].radial_next = CoedgeId("missing-radial".into());

    let report = validate(&ir, Vec::new());
    let messages = report
        .findings
        .iter()
        .map(|finding| finding.message.as_str())
        .collect::<Vec<_>>();
    assert!(messages.iter().any(|message| message.contains("wire edge")));
    assert!(messages
        .iter()
        .any(|message| message.contains("free vertex")));
    assert!(messages
        .iter()
        .any(|message| message.contains("coedge(radial_next)")));
}

#[test]
fn topology_tolerance_and_new_conics_are_bounds_checked() {
    let mut ir = unit_cube();
    let edge_id = ir.model.edges[0].id.0.clone();
    ir.model.edges[0].tolerance = Some(-1.0);
    ir.model.curves.push(Curve {
        id: CurveId("synthetic:test:curve#bad-parabola".into()),
        geometry: CurveGeometry::Parabola {
            vertex: Point3::new(0.0, 0.0, 0.0),
            axis: Vector3::new(0.0, 0.0, 1.0),
            major_direction: Vector3::new(1.0, 0.0, 0.0),
            focal_distance: 0.0,
        },
        source_object: None,
    });
    ir.model.curves.push(Curve {
        id: CurveId("synthetic:test:curve#bad-hyperbola".into()),
        geometry: CurveGeometry::Hyperbola {
            center: Point3::new(0.0, 0.0, 0.0),
            axis: Vector3::new(0.0, 0.0, 1.0),
            major_direction: Vector3::new(1.0, 0.0, 0.0),
            major_radius: -1.0,
            minor_radius: 1.0,
        },
        source_object: None,
    });

    let report = validate(&ir, Vec::new());
    for entity in [
        edge_id.as_str(),
        "synthetic:test:curve#bad-parabola",
        "synthetic:test:curve#bad-hyperbola",
    ] {
        assert!(report
            .findings
            .iter()
            .any(
                |finding| (finding.check == Check::Bounds || finding.check == Check::Tolerances)
                    && finding.entity.as_deref() == Some(entity)
            ));
    }
}

#[test]
fn wrong_document_version_is_flagged() {
    let mut ir = unit_cube();
    ir.ir_version = "1".into();
    assert!(validate(&ir, Vec::new())
        .findings
        .iter()
        .any(|finding| finding.check == Check::Version));
}

#[test]
fn parser_rejects_unsupported_missing_and_non_string_versions() {
    let canonical = serde_json::to_value(unit_cube()).unwrap();
    for version in [
        Some(serde_json::Value::String("0".into())),
        None,
        Some(serde_json::Value::Number(1.into())),
    ] {
        let mut value = canonical.clone();
        let object = value.as_object_mut().unwrap();
        match version {
            Some(version) => {
                object.insert("ir_version".into(), version);
            }
            None => {
                object.remove("ir_version");
            }
        }
        let json = serde_json::to_string(&value).unwrap();
        let error = CadIr::from_json(&json).unwrap_err();
        assert!(!error.is_syntax());
        assert!(error.to_string().contains("unsupported ir_version"));
        assert!(serde_json::from_str::<CadIr>(&json).is_err());
    }
}

#[test]
fn direct_deserialization_accepts_current_version_and_canonical_round_trip() {
    let ir = unit_cube();
    let json = ir.to_canonical_json().unwrap();
    let parsed = serde_json::from_str::<CadIr>(&json).unwrap();
    assert_eq!(parsed, ir);
    assert_eq!(parsed.to_canonical_json().unwrap(), json);
}

#[test]
fn schema_constrains_version_and_requires_subd_arena() {
    let schema = serde_json::to_value(crate::cadir_json_schema()).unwrap();
    assert_eq!(
        schema.pointer("/properties/ir_version/const"),
        Some(&serde_json::json!(crate::IR_VERSION))
    );
    assert!(schema
        .pointer("/properties/model/$ref")
        .and_then(serde_json::Value::as_str)
        .is_some());

    let model_schema = schema.pointer("/$defs/Model").unwrap();
    assert!(model_schema
        .pointer("/required")
        .and_then(serde_json::Value::as_array)
        .unwrap()
        .contains(&serde_json::json!("subds")));

    let mut value = serde_json::to_value(unit_cube()).unwrap();
    value
        .pointer_mut("/model")
        .unwrap()
        .as_object_mut()
        .unwrap()
        .remove("subds");
    assert!(serde_json::from_value::<CadIr>(value).is_err());
}

#[test]
fn subd_round_trip_and_directed_ring_validation() {
    let mut ir = CadIr::empty(crate::units::Units::default());
    ir.model.subds.push(SubdSurface {
        id: SubdId("synthetic:subd:surface#0".into()),
        scheme: SubdScheme::CatmullClark,
        vertices: vec![
            SubdVertex {
                point: Point3::new(0.0, 0.0, 0.0),
                tag: SubdVertexTag::Smooth,
            },
            SubdVertex {
                point: Point3::new(1.0, 0.0, 0.0),
                tag: SubdVertexTag::Smooth,
            },
            SubdVertex {
                point: Point3::new(0.0, 1.0, 0.0),
                tag: SubdVertexTag::Smooth,
            },
        ],
        edges: vec![
            SubdEdge {
                vertices: [0, 1],
                sharpness: [0.0, 0.25],
                tag: SubdEdgeTag::Smooth,
                sector_coefficients: [1.0, 1.0],
            },
            SubdEdge {
                vertices: [1, 2],
                sharpness: [0.25, 0.0],
                tag: SubdEdgeTag::SmoothX,
                sector_coefficients: [1.0, 1.0],
            },
            SubdEdge {
                vertices: [2, 0],
                sharpness: [0.0, 0.0],
                tag: SubdEdgeTag::Smooth,
                sector_coefficients: [1.0, 1.0],
            },
        ],
        faces: vec![SubdFace {
            edges: vec![
                SubdEdgeUse {
                    edge: 0,
                    reversed: false,
                },
                SubdEdgeUse {
                    edge: 1,
                    reversed: false,
                },
                SubdEdgeUse {
                    edge: 2,
                    reversed: false,
                },
            ],
        }],
        source_object: None,
    });
    assert!(validate(&ir, Vec::new()).is_ok());
    let parsed = CadIr::from_json(&ir.to_canonical_json().unwrap()).unwrap();
    assert_eq!(parsed, ir);
    assert_eq!(
        serde_json::to_value(SubdEdgeTag::SmoothX).unwrap(),
        serde_json::json!("smooth_x")
    );
    ir.model.subds[0].faces[0].edges[1].reversed = true;
    assert!(!validate(&ir, Vec::new()).is_ok());
}

#[test]
fn subd_rejects_short_rings_and_negative_sharpness() {
    let mut ir = CadIr::empty(crate::units::Units::default());
    ir.model.subds.push(SubdSurface {
        id: SubdId("synthetic:subd:surface#short".into()),
        scheme: SubdScheme::CatmullClark,
        vertices: vec![
            SubdVertex {
                point: Point3::new(0.0, 0.0, 0.0),
                tag: SubdVertexTag::Smooth,
            },
            SubdVertex {
                point: Point3::new(1.0, 0.0, 0.0),
                tag: SubdVertexTag::Smooth,
            },
        ],
        edges: vec![SubdEdge {
            vertices: [0, 1],
            sharpness: [-0.1, 0.0],
            tag: SubdEdgeTag::Smooth,
            sector_coefficients: [0.0, 0.0],
        }],
        faces: vec![SubdFace {
            edges: vec![
                SubdEdgeUse {
                    edge: 0,
                    reversed: false,
                },
                SubdEdgeUse {
                    edge: 0,
                    reversed: true,
                },
            ],
        }],
        source_object: None,
    });
    let findings = validate(&ir, Vec::new()).findings;
    assert!(findings
        .iter()
        .any(|finding| finding.message.contains("fewer than three")));
    assert!(findings
        .iter()
        .any(|finding| finding.message.contains("edge 0 is invalid")));
}

#[test]
fn revolution_rejects_equal_intervals() {
    let mut ir = unit_cube();
    ir.model.procedural_surfaces.push(ProceduralSurface {
        id: ProceduralSurfaceId("synthetic:test:procedural-surface#equal".into()),
        surface: ir.model.surfaces[0].id.clone(),
        definition: ProceduralSurfaceDefinition::Revolution {
            directrix: ir.model.curves[0].id.clone(),
            axis_origin: Point3::new(0.0, 0.0, 0.0),
            axis_direction: Vector3::new(0.0, 0.0, 1.0),
            angular_interval: [1.0, 1.0],
            parameter_interval: [0.0, 1.0],
            transposed: false,
        },
        cache_fit_tolerance: None,
    });
    assert!(validate(&ir, Vec::new())
        .findings
        .iter()
        .any(|finding| finding.message.contains("revolution interval")));
}

#[test]
fn source_association_is_a_free_carrier_root() {
    let mut ir = CadIr::empty(crate::units::Units::default());
    ir.model.curves.push(Curve {
        id: CurveId("synthetic:source:curve#0".into()),
        geometry: CurveGeometry::Unknown { record: None },
        source_object: Some(SourceObjectAssociation {
            format: "rhino".into(),
            object_id: "00000000-0000-0000-0000-000000000000".into(),
            name: Some("curve".into()),
            color: None,
            visible: Some(true),
            layer: Some("layer-0".into()),
            instance_path: Vec::new(),
        }),
    });
    let report = validate(&ir, Vec::new());
    assert!(report.is_ok(), "{:?}", report.findings);
    let parsed = CadIr::from_json(&ir.to_canonical_json().unwrap()).unwrap();
    assert_eq!(parsed, ir);
}

#[test]
fn source_association_rejects_out_of_range_color() {
    let mut ir = CadIr::empty(crate::units::Units::default());
    ir.model.curves.push(Curve {
        id: CurveId("synthetic:source:curve#color".into()),
        geometry: CurveGeometry::Unknown { record: None },
        source_object: Some(SourceObjectAssociation {
            format: "rhino".into(),
            object_id: "object".into(),
            name: None,
            color: Some(Color {
                r: 1.1,
                g: 0.0,
                b: 0.0,
                a: 1.0,
            }),
            visible: None,
            layer: None,
            instance_path: Vec::new(),
        }),
    });
    assert!(validate(&ir, Vec::new())
        .findings
        .iter()
        .any(|finding| finding.message.contains("outside [0, 1]")));
}

#[test]
fn parser_distinguishes_malformed_json_from_version_rejection() {
    let error = CadIr::from_json("{\"ir_version\":\"1\"").unwrap_err();
    assert!(error.is_syntax() || error.is_eof());
    assert!(!error.to_string().contains("unsupported ir_version"));
}

#[test]
fn entity_ids_follow_canonical_grammar() {
    let mut ir = unit_cube();
    ir.model.points[0].id.0 = "synthetic:scope:point".into();
    let findings = validate(&ir, Vec::new()).findings;
    assert!(findings.iter().any(|finding| {
        finding.check == Check::Identity
            && finding.entity.as_deref() == Some("synthetic:scope:point")
            && finding.message.contains("<format>:<scope>:<kind>#<key>")
    }));
}

#[test]
fn ids_are_globally_unique_across_arenas() {
    let mut ir = unit_cube();
    ir.model.points[0].id.0 = ir.model.vertices[0].id.0.clone();
    assert!(validate(&ir, Vec::new())
        .findings
        .iter()
        .any(|finding| finding.check == Check::Identity));
}

#[test]
fn arena_ids_must_be_sorted() {
    let mut ir = unit_cube();
    ir.model.points.swap(0, 1);
    assert!(validate(&ir, Vec::new())
        .findings
        .iter()
        .any(|finding| finding.check == Check::ArenaOrder));
}

#[test]
fn two_member_radial_ring_with_equal_senses_warns() {
    let mut ir = unit_cube();
    let other_id = ir.model.coedges[0].radial_next.clone();
    let sense = ir.model.coedges[0].sense;
    ir.model
        .coedges
        .iter_mut()
        .find(|coedge| coedge.id == other_id)
        .unwrap()
        .sense = sense;
    assert!(validate(&ir, Vec::new()).findings.iter().any(|finding| {
        finding.check == Check::CoedgePairing
            && finding.severity == crate::report::Severity::Warning
    }));
}

#[test]
fn coedge_backed_edge_cannot_be_a_wire_edge() {
    let mut ir = unit_cube();
    ir.model.shells[0]
        .wire_edges
        .push(ir.model.coedges[0].edge.clone());
    assert!(validate(&ir, Vec::new())
        .findings
        .iter()
        .any(|finding| finding.check == Check::WireTopology));
}

#[test]
fn wire_and_free_topology_negative_cases_are_reported() {
    let mut ir = unit_cube();

    let mut unowned_edge = ir.model.edges[0].clone();
    unowned_edge.id.0 = "synthetic:test:edge#unowned".into();
    ir.model.edges.push(unowned_edge);

    let mut duplicate_edge = ir.model.edges[1].clone();
    duplicate_edge.id.0 = "synthetic:test:edge#duplicate".into();
    ir.model.shells[0]
        .wire_edges
        .extend([duplicate_edge.id.clone(), duplicate_edge.id.clone()]);
    ir.model.edges.push(duplicate_edge);

    let mut unowned_vertex = ir.model.vertices[0].clone();
    unowned_vertex.id.0 = "synthetic:test:vertex#unowned".into();
    ir.model.vertices.push(unowned_vertex);

    ir.model.shells[0]
        .free_vertices
        .push(ir.model.edges[0].start.clone());
    ir.model.bodies[0].kind = crate::topology::BodyKind::Wire;
    ir.finalize();

    let findings = validate(&ir, Vec::new()).findings;
    for message in [
        "wire edge must belong to exactly one shell",
        "free vertex must belong to exactly one shell",
        "free vertex is also referenced by an edge",
        "wire body contains faces",
    ] {
        assert!(
            findings.iter().any(|finding| {
                finding.check == Check::WireTopology && finding.message == message
            }),
            "missing `{message}` in {findings:?}"
        );
    }
}

#[test]
fn empty_shell_is_reported() {
    let mut ir = unit_cube();
    ir.model.shells[0].faces.clear();
    let findings = validate(&ir, Vec::new()).findings;
    assert!(findings.iter().any(|finding| {
        finding.check == Check::WireTopology && finding.message == "shell owns no topology"
    }));
}

#[test]
fn orphan_carrier_is_flagged() {
    let mut ir = unit_cube();
    let mut orphan = ir.model.curves[0].clone();
    orphan.id = CurveId("zz:orphan".into());
    ir.model.curves.push(orphan);
    assert!(validate(&ir, Vec::new())
        .findings
        .iter()
        .any(|finding| finding.check == Check::CarrierReachability));
}

#[test]
fn annotation_keys_streams_and_field_paths_are_checked() {
    let mut ir = unit_cube();
    ir.annotations.provenance.insert(
        "missing".into(),
        Provenance {
            stream: u32::MAX,
            offset: 0,
            tag: None,
        },
    );
    ir.annotations.exactness.insert(
        ir.model.edges[0].id.0.clone(),
        ExactnessNote {
            entity: Exactness::Derived,
            fields: std::collections::BTreeMap::from([(
                "not_a_serialized_field".into(),
                Exactness::Derived,
            )]),
        },
    );
    let findings = validate(&ir, Vec::new()).findings;
    assert!(findings.iter().any(|finding| {
        finding.check == Check::Annotations && finding.severity == crate::report::Severity::Error
    }));
    assert!(findings.iter().any(|finding| {
        finding.check == Check::Annotations && finding.severity == crate::report::Severity::Warning
    }));
}

#[test]
fn native_topology_link_must_resolve() {
    let mut ir = unit_cube();
    ir.native.namespace_mut("f3d").arenas.insert(
        "sketch_curve_links".into(),
        vec![NativeRecord {
            id: "native:link#0".into(),
            fields: serde_json::from_value(serde_json::json!({"links": ["missing"]})).unwrap(),
        }],
    );
    ir.native.finalize();
    assert!(validate(&ir, Vec::new())
        .findings
        .iter()
        .any(|finding| finding.check == Check::NativeLinks));
}

#[test]
fn periodic_curve_parameter_domain_is_checked() {
    let mut ir = unit_cube();
    let curve_id = ir.model.edges[0].curve.clone().unwrap();
    ir.model
        .curves
        .iter_mut()
        .find(|curve| curve.id == curve_id)
        .unwrap()
        .geometry = CurveGeometry::Circle {
        center: Point3::new(0.0, 0.0, 0.0),
        axis: Vector3::new(0.0, 0.0, 1.0),
        ref_direction: Vector3::new(1.0, 0.0, 0.0),
        radius: 1.0,
    };
    ir.model.edges[0].param_range = Some([0.0, 7.0]);
    assert!(validate(&ir, Vec::new())
        .findings
        .iter()
        .any(|finding| finding.check == Check::ParameterDomain));
}

#[test]
fn document_and_entity_tolerances_are_checked() {
    let mut ir = unit_cube();
    ir.tolerances.angular = f64::NAN;
    ir.model.faces[0].tolerance = Some(0.0);
    assert!(validate(&ir, Vec::new())
        .findings
        .iter()
        .any(|finding| finding.check == Check::Tolerances));
}

#[test]
fn preserved_payload_digest_is_checked() {
    let mut ir = unit_cube();
    ir.push_native_unknown(
        "zz",
        UnknownRecord {
            id: UnknownId("zz:payload".into()),
            offset: 0,
            byte_len: 3,
            sha256: "0".repeat(64),
            data: Some(vec![1, 2, 3]),
            links: Vec::new(),
        },
    )
    .unwrap();
    assert!(validate(&ir, Vec::new())
        .findings
        .iter()
        .any(|finding| finding.check == Check::PayloadIntegrity));
}

#[test]
fn byte_payloads_use_nonempty_base64_and_reject_invalid_text() {
    assert_base64_round_trip_and_rejection(
        &UnknownRecord {
            id: UnknownId("synthetic:test:unknown#0".into()),
            offset: 0,
            byte_len: 3,
            sha256: "00".repeat(32),
            data: Some(vec![1, 2, 3]),
            links: Vec::new(),
        },
        "data",
    );
    assert_base64_round_trip_and_rejection(
        &TessellationChannel {
            item_size: 3,
            kind: 0,
            flags: 0,
            count: 1,
            data: vec![1, 2, 3],
        },
        "data",
    );
}

#[test]
fn schema_generation_produces_definitions() {
    let schema = crate::cadir_json_schema();
    // The schema must reference the entity types, not just be an empty object.
    let json = serde_json::to_string(&schema).unwrap();
    assert!(json.contains("Body"));
    assert!(json.contains("Coedge"));
    assert!(json.contains("SurfaceGeometry"));
    let defs = schema
        .get("$defs")
        .and_then(serde_json::Value::as_object)
        .expect("schema has a $defs object");
    assert!(!defs.is_empty());
}

#[test]
fn loss_provenance_root_alias_constructs_and_serializes() {
    let note = LossNote {
        category: LossCategory::Geometry,
        severity: Severity::Warning,
        message: "geometry was retained as metadata".into(),
        provenance: Some(LossProvenance {
            format: "rhino".into(),
            stream: String::new(),
            offset: 42,
            tag: Some(
                "OBJECT_RECORD/class=00000000-0000-0000-0000-000000000000/type=0x00000020".into(),
            ),
        }),
    };
    let json = serde_json::to_value(&note).unwrap();
    assert_eq!(json["provenance"]["format"], "rhino");
    assert_eq!(json["provenance"]["stream"], "");
    assert_eq!(json["provenance"]["offset"], 42);
    assert_eq!(
        json["provenance"]["tag"],
        "OBJECT_RECORD/class=00000000-0000-0000-0000-000000000000/type=0x00000020"
    );
}

#[test]
fn rational_quadratic_arc_evaluates_on_the_circle() {
    // Quarter circle of radius 5 as a rational quadratic Bezier.
    let weight = 0.5_f64.sqrt();
    let point = crate::eval::nurbs_curve_point(
        2,
        &[0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
        &[
            Point3::new(5.0, 0.0, 0.0),
            Point3::new(5.0, 5.0, 0.0),
            Point3::new(0.0, 5.0, 0.0),
        ],
        Some(&[1.0, weight, 1.0]),
        0.5,
    )
    .unwrap();
    let radius = (point.x * point.x + point.y * point.y).sqrt();
    assert!((radius - 5.0).abs() < 1e-12, "mid-span radius {radius}");
}

#[test]
fn edge_endpoint_mismatch_is_flagged() {
    let mut ir = unit_cube();
    let report = validate(&ir, Vec::new());
    assert!(
        !report
            .findings
            .iter()
            .any(|f| f.check == Check::GeometricConsistency),
        "worked cube must be geometrically consistent, got: {:?}",
        report.findings
    );

    // Displace one corner: the point no longer lies on its edges' curves at
    // the stored parameter values.
    ir.model.points[0].position.z += 1.0;
    let report = validate(&ir, Vec::new());
    assert!(
        report
            .findings
            .iter()
            .any(|f| f.check == Check::GeometricConsistency
                && f.severity == Severity::Error
                && f.entity.as_deref().is_some_and(|e| e.contains("edge"))),
        "displaced vertex must fail edge endpoint consistency, got: {:?}",
        report.findings
    );
}

#[test]
fn pcurve_surface_mismatch_is_flagged() {
    // The bottom face's plane is `origin (0,0,0), normal (0,0,-1)`, whose
    // derived u/v frame maps `(u, v) -> (u, -v, 0)`. Edge #0 runs from
    // `(0,0,0)` to `(10,0,0)`, so its parameter image is the line
    // `(0,0) -> (10,0)`.
    let good = |u_end: f64, v_end: f64| {
        let mut ir = unit_cube();
        ir.model.pcurves.push(crate::geometry::Pcurve {
            id: crate::ids::PcurveId("synthetic:cube:pcurve#0".into()),
            geometry: crate::geometry::PcurveGeometry::Nurbs {
                degree: 1,
                knots: vec![0.0, 0.0, 1.0, 1.0],
                control_points: vec![
                    crate::math::Point2::new(0.0, 0.0),
                    crate::math::Point2::new(u_end, v_end),
                ],
                weights: None,
                periodic: false,
            },
            wrapper_reversed: None,
            parameter_range: None,
            fit_tolerance: None,
        });
        let coedge = ir
            .model
            .coedges
            .iter_mut()
            .find(|coedge| {
                coedge.id.0.contains("bottom") && coedge.edge.0 == "synthetic:cube:edge#0"
            })
            .expect("bottom face uses edge #0");
        coedge.pcurve = Some(crate::ids::PcurveId("synthetic:cube:pcurve#0".into()));
        validate(&ir, Vec::new())
    };

    let consistent = good(10.0, 0.0);
    assert!(
        !consistent
            .findings
            .iter()
            .any(|f| f.check == Check::GeometricConsistency),
        "matching pcurve must validate, got: {:?}",
        consistent.findings
    );

    let inconsistent = good(10.0, 5.0);
    assert!(
        inconsistent
            .findings
            .iter()
            .any(|f| f.check == Check::GeometricConsistency
                && f.entity.as_deref().is_some_and(|e| e.contains("coedge"))),
        "off-surface-image pcurve must be flagged, got: {:?}",
        inconsistent.findings
    );
}

#[test]
fn feature_extents_round_trip_through_json() {
    use crate::features::{Angle, Extent, Length};
    use crate::ids::FaceId;

    let extents = vec![
        Extent::Blind {
            length: Length(12.5),
        },
        Extent::Symmetric {
            length: Length(25.0),
        },
        Extent::TwoSided {
            first: Length(10.0),
            second: Length(20.0),
        },
        Extent::ThroughAll,
        Extent::ToFace {
            face: FaceId("synthetic:test:face#0".into()),
        },
        Extent::Angle {
            angle: Angle(std::f64::consts::PI),
        },
    ];

    let json = serde_json::to_string(&extents).unwrap();
    assert_eq!(serde_json::from_str::<Vec<Extent>>(&json).unwrap(), extents);
}

#[test]
fn edge_selections_round_trip_through_json() {
    use crate::features::EdgeSelection;
    use crate::ids::EdgeId;

    let selections = vec![
        EdgeSelection::Edges(vec![EdgeId("synthetic:test:edge#0".into())]),
        EdgeSelection::Native("sldprt:history:feature#10:0".into()),
    ];
    let json = serde_json::to_string(&selections).unwrap();
    assert_eq!(
        serde_json::from_str::<Vec<EdgeSelection>>(&json).unwrap(),
        selections
    );
}

#[test]
fn face_selections_round_trip_through_json() {
    use crate::features::FaceSelection;
    use crate::ids::FaceId;

    let selections = vec![
        FaceSelection::Faces(vec![FaceId("synthetic:test:face#0".into())]),
        FaceSelection::Native("sldprt:history:feature#14:0".into()),
    ];
    let json = serde_json::to_string(&selections).unwrap();
    assert_eq!(
        serde_json::from_str::<Vec<FaceSelection>>(&json).unwrap(),
        selections
    );
}
