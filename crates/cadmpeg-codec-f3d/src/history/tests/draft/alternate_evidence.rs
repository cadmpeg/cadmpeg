// SPDX-License-Identifier: Apache-2.0
//! Repeated carrier evidence invalidates the complete alternate lane.

use crate::history::{
    draft_surface_geometry, resolve_draft_face_by_surface_transition, DraftSurfaceEvidence,
};
use crate::history_records::{
    AsmHistoricalCarrierBinding, AsmHistoricalCylinder, AsmHistoricalPlane,
    AsmHistoricalSurfaceAxis, AsmHistoricalSurfaceRadius, AsmHistoricalTopology,
};
use crate::records::{
    dimensions::DesignRecipeReference, recipes::ConstructionRecipeKind,
    topology::face::DesignFaceOperand,
};
use cadmpeg_core::decode::{DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use cadmpeg_ir::{
    ids::FaceId,
    math::{Point3, Vector3},
};

fn operand() -> DesignFaceOperand {
    let face = |slot| FaceId::mint(format!("f3d:brep:entity#{slot}")).expect("identity grammar");
    let reference = |candidate_faces, alternate_selector_faces| DesignRecipeReference {
        selector: 2,
        selector_offset: 0,
        token: "1".into(),
        token_offset: 0,
        design_reference: 303,
        design_reference_offset: 0,
        candidate_faces,
        candidate_edges: Vec::new(),
        alternate_selector_faces,
        alternate_selector_edges: Vec::new(),
    };
    let mut operand: DesignFaceOperand = serde_json::from_value(serde_json::json!({
        "id": "f3d:test:face-operand#1",
        "scope_record_index": 10,
        "scope_reference_ordinal": 0,
        "record_index": 1,
        "byte_offset": 0,
        "class_tag": "414",
        "paired_byte_offset": 100,
        "paired_class_tag": "258",
        "recipe_record_index": 4,
        "recipe_record_byte_offset": 116,
        "recipe_id": "f3d:test:recipe#4",
        "recipe_prefix_offset": 127,
        "recipe_prefix_bytes": "",
        "recipe_references": [],
        "recipe_kind": "bounded_face",
        "recipe_program_offset": 0,
        "recipe_program": [0, -1, 1],
        "recipe_node_offsets": [0],
        "recipe_nodes": [{
            "byte_offset": 0,
            "end_byte_offset": 12,
            "program": [0, -1, 1],
            "recipe_structure": null
        }],
        "candidate_faces": ["f3d:brep:entity#10", "f3d:brep:entity#11"],
        "preceding_candidate_faces": [],
        "changed_candidate_faces": [],
        "historical_support_contexts": [],
        "resolved_face_slots": [],
        "next_record_index": 5,
        "next_byte_offset": 200
    }))
    .expect("Draft face operand");
    operand.recipe_kind = ConstructionRecipeKind::BoundedFace;
    operand.recipe_references = vec![reference(Vec::new(), vec![face(10), face(11)])];

    operand
}

fn topology(changed: bool) -> AsmHistoricalTopology {
    AsmHistoricalTopology {
        faces: vec![10, 11],
        face_surfaces: vec![
            AsmHistoricalCarrierBinding {
                entity: 10,
                carrier: 100,
            },
            AsmHistoricalCarrierBinding {
                entity: 11,
                carrier: 101,
            },
        ],
        surface_planes: [100, 101]
            .into_iter()
            .map(|surface| AsmHistoricalPlane {
                surface,
                origin: Point3::new(0.0, 0.0, 0.0),
                normal: if changed {
                    Vector3::new(1.0, 0.0, 0.0)
                } else {
                    Vector3::new(0.0, 0.0, 1.0)
                },
            })
            .collect(),
        ..Default::default()
    }
}

#[derive(Clone, Copy)]
enum Carrier {
    Binding,
    Plane,
    Cylinder,
    Axis,
    Radius,
}

fn conflicting(topology: &mut AsmHistoricalTopology, face: i64, kind: Carrier, duplicate: bool) {
    let surface = face + 90;
    if matches!(kind, Carrier::Binding) {
        if duplicate {
            topology.face_surfaces.push(AsmHistoricalCarrierBinding {
                entity: face,
                carrier: surface + 200,
            });
        }
        return;
    }
    topology
        .surface_planes
        .retain(|plane| plane.surface != surface);
    let axis = Vector3::new(0.0, 0.0, 1.0);
    let origin = Point3::new(0.0, 0.0, 0.0);
    match kind {
        Carrier::Binding => unreachable!("binding handled above"),
        Carrier::Plane => {
            topology.surface_planes.push(AsmHistoricalPlane {
                surface,
                origin,
                normal: axis,
            });
            if duplicate {
                topology.surface_planes.push(AsmHistoricalPlane {
                    surface,
                    origin,
                    normal: Vector3::new(0.0, 1.0, 0.0),
                });
            }
        }
        Carrier::Cylinder => {
            topology.surface_cylinders.push(AsmHistoricalCylinder {
                surface,
                origin,
                axis,
                radius: 1.0,
            });
            if duplicate {
                topology.surface_cylinders.push(AsmHistoricalCylinder {
                    surface,
                    origin,
                    axis,
                    radius: 2.0,
                });
            }
        }
        Carrier::Axis | Carrier::Radius => {
            topology.surface_axes.push(AsmHistoricalSurfaceAxis {
                surface,
                origin,
                direction: axis,
            });
            if matches!(kind, Carrier::Axis) && duplicate {
                topology.surface_axes.push(AsmHistoricalSurfaceAxis {
                    surface,
                    origin,
                    direction: Vector3::new(0.0, 1.0, 0.0),
                });
            }
            if matches!(kind, Carrier::Radius) {
                topology.surface_radii.push(AsmHistoricalSurfaceRadius {
                    surface,
                    radius: 1.0,
                });
                if duplicate {
                    topology.surface_radii.push(AsmHistoricalSurfaceRadius {
                        surface,
                        radius: 2.0,
                    });
                }
            }
        }
    }
}

fn rejects_complete_lane(kind: Carrier) {
    let operand = operand();
    for duplicate_before in [true, false] {
        for face in [10, 11] {
            let mut before = topology(false);
            let mut after = topology(true);
            conflicting(&mut before, face, kind, duplicate_before);
            conflicting(&mut after, face, kind, !duplicate_before);
            crate::test_support::with_decode_context(|ctx| {
                let repeated = if duplicate_before { &before } else { &after };
                assert!(matches!(
                    draft_surface_geometry(ctx, repeated, face).expect("repeated carrier"),
                    DraftSurfaceEvidence::Repeated
                ));
                assert_eq!(
                    resolve_draft_face_by_surface_transition(ctx, &operand, &before, &after)
                        .expect("alternate proof"),
                    None
                );
                assert_eq!(ctx.resource_refusal(), None);
            });
        }
    }
}

#[test]
fn draft_alternate_duplicate_binding_rejects_other_changed_candidate() {
    rejects_complete_lane(Carrier::Binding);
}
#[test]
fn draft_alternate_duplicate_plane_rejects_other_changed_candidate() {
    rejects_complete_lane(Carrier::Plane);
}
#[test]
fn draft_alternate_duplicate_cylinder_rejects_other_changed_candidate() {
    rejects_complete_lane(Carrier::Cylinder);
}
#[test]
fn draft_alternate_duplicate_axis_rejects_other_changed_candidate() {
    rejects_complete_lane(Carrier::Axis);
}
#[test]
fn draft_alternate_duplicate_radius_rejects_other_changed_candidate() {
    rejects_complete_lane(Carrier::Radius);
}

#[test]
fn draft_alternate_missing_carrier_allows_other_changed_candidate() {
    let operand = operand();
    for missing_before in [true, false] {
        for face in [10, 11] {
            let mut before = topology(false);
            let mut after = topology(true);
            let missing = if missing_before {
                &mut before
            } else {
                &mut after
            };
            missing
                .face_surfaces
                .retain(|binding| binding.entity != face);
            crate::test_support::with_decode_context(|ctx| {
                assert_eq!(
                    resolve_draft_face_by_surface_transition(ctx, &operand, &before, &after)
                        .expect("alternate proof"),
                    Some(if face == 10 { 11 } else { 10 })
                );
                assert_eq!(ctx.resource_refusal(), None);
            });
        }
    }
}

#[test]
fn draft_alternate_missing_geometry_allows_other_changed_candidate() {
    let operand = operand();
    for missing_before in [true, false] {
        for face in [10, 11] {
            let mut before = topology(false);
            let mut after = topology(true);
            let missing = if missing_before {
                &mut before
            } else {
                &mut after
            };
            missing
                .surface_planes
                .retain(|plane| plane.surface != face + 90);
            let absent = if missing_before { &before } else { &after };
            crate::test_support::with_decode_context(|ctx| {
                assert!(matches!(
                    draft_surface_geometry(ctx, absent, face).expect("absent geometry"),
                    DraftSurfaceEvidence::Absent
                ));
                assert_eq!(
                    resolve_draft_face_by_surface_transition(ctx, &operand, &before, &after)
                        .expect("alternate proof"),
                    Some(if face == 10 { 11 } else { 10 })
                );
                assert_eq!(ctx.resource_refusal(), None);
            });
        }
    }
}

#[test]
fn draft_exact_leaf_skips_duplicate_carrier_geometry() {
    let mut operand = operand();
    operand.recipe_references[0]
        .alternate_selector_faces
        .clear();
    operand.recipe_references[0].candidate_faces = vec![crate::ids::brep_face_id(10)];
    let mut topology = topology(false);
    conflicting(&mut topology, 10, Carrier::Plane, true);
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = u64::MAX;
    crate::test_support::with_decode_policy(&policy, |ctx| {
        let _probe = cadmpeg_core::decode::refusal_probe::RefusalProbe::arm(
            ResourceDimension::WorkUnits,
            "find F3D draft surface geometry",
            Some(1),
        );
        assert_eq!(
            resolve_draft_face_by_surface_transition(ctx, &operand, &topology, &topology)
                .expect("exact leaf"),
            Some(10)
        );
        assert_eq!(ctx.resource_refusal(), None);
    });
}

#[test]
fn draft_duplicate_binding_stops_before_unread_carriers() {
    let mut topology = topology(false);
    topology.face_surfaces = vec![
        AsmHistoricalCarrierBinding {
            entity: 10,
            carrier: 100,
        },
        AsmHistoricalCarrierBinding {
            entity: 10,
            carrier: 200,
        },
    ];
    topology
        .face_surfaces
        .extend((12..4110).map(|entity| AsmHistoricalCarrierBinding {
            entity,
            carrier: entity + 100,
        }));
    let mut policy = DecodePolicy::service();
    // Two matching raw binding records; neither the suffix nor any geometry is read.
    policy.limits.max_work_units = 2;
    policy.limits.max_collection_items = 0;
    policy.limits.max_materialized_bytes = 0;
    policy.limits.max_retained_bytes = 0;
    crate::test_support::with_decode_policy(&policy, |ctx| {
        assert!(matches!(
            draft_surface_geometry(ctx, &topology, 10).expect("binding proof"),
            DraftSurfaceEvidence::Repeated
        ));
        assert_eq!(ctx.resource_refusal(), None);
        let Err(CodecError::ResourceLimit(original)) = ctx.charge_work(1, "after repeated binding")
        else {
            panic!("exact two-record work boundary")
        };
        assert_eq!(original.dimension, ResourceDimension::WorkUnits);
        assert_eq!(original.used, 2);
        assert_eq!(original.additional, 1);
        let Err(CodecError::ResourceLimit(sticky)) =
            draft_surface_geometry(ctx, &AsmHistoricalTopology::default(), 10)
        else {
            panic!("original binding refusal")
        };
        assert_eq!(sticky, original);
    });
}

#[test]
fn draft_alternate_each_reached_phase_keeps_original_refusal() {
    let operand = operand();
    let before = topology(false);
    let after = topology(true);
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = u64::MAX;
    for (operation, additional) in [
        ("scan F3D draft face candidates", 2),
        ("collect F3D draft alternate faces", 1),
        ("scan F3D draft surface candidates", 1),
        ("find F3D draft face in both states", 1),
        ("find F3D draft surface geometry", 1),
    ] {
        crate::test_support::with_decode_policy(&policy, |ctx| {
            let _probe = cadmpeg_core::decode::refusal_probe::RefusalProbe::arm(
                ResourceDimension::WorkUnits,
                operation,
                Some(additional),
            );
            let Err(CodecError::ResourceLimit(original)) =
                resolve_draft_face_by_surface_transition(ctx, &operand, &before, &after)
            else {
                panic!("reached draft phase {operation}")
            };
            assert_eq!(original.operation, operation);
            assert_eq!(original.dimension, ResourceDimension::WorkUnits);
            assert_eq!(original.additional, additional);
            assert_eq!(ctx.resource_refusal(), Some(original.clone()));
            let Err(CodecError::ResourceLimit(sticky)) =
                resolve_draft_face_by_surface_transition(ctx, &operand, &before, &after)
            else {
                panic!("sticky draft phase")
            };
            assert_eq!(sticky, original);
        });
    }
}
