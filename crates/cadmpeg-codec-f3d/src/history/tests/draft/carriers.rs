// SPDX-License-Identifier: Apache-2.0
//! Duplicate carrier geometry and reached proof admission.

use crate::history::{draft_surface_geometry, DraftSurfaceGeometry};
use crate::history_records::{
    AsmHistoricalCarrierBinding, AsmHistoricalCylinder, AsmHistoricalPlane,
    AsmHistoricalSurfaceAxis, AsmHistoricalSurfaceRadius, AsmHistoricalTopology,
};
use cadmpeg_core::decode::{DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use cadmpeg_ir::math::{Point3, Vector3};

fn topology() -> AsmHistoricalTopology {
    AsmHistoricalTopology {
        faces: vec![10],
        face_surfaces: vec![AsmHistoricalCarrierBinding { entity: 10, carrier: 100 }],
        ..Default::default()
    }
}

fn plane(normal: Vector3) -> AsmHistoricalPlane {
    AsmHistoricalPlane { surface: 100, origin: Point3::new(0.0, 0.0, 0.0), normal }
}

fn cylinder(radius: f64) -> AsmHistoricalCylinder {
    AsmHistoricalCylinder {
        surface: 100, origin: Point3::new(0.0, 0.0, 0.0),
        axis: Vector3::new(0.0, 0.0, 1.0), radius,
    }
}

fn axis(direction: Vector3) -> AsmHistoricalSurfaceAxis {
    AsmHistoricalSurfaceAxis { surface: 100, origin: Point3::new(0.0, 0.0, 0.0), direction }
}

fn assert_ambiguous(topology: &AsmHistoricalTopology) {
    crate::test_support::with_decode_context(|ctx| {
        assert!(draft_surface_geometry(ctx, topology, 10).unwrap().is_none());
        assert_eq!(ctx.resource_refusal(), None);
    });
}

#[test]
fn draft_plane_carrier_rejects_conflicting_records() {
    let mut topology = topology();
    topology.surface_planes = vec![plane(Vector3::new(0.0, 0.0, 1.0)), plane(Vector3::new(1.0, 0.0, 0.0))];
    assert_ambiguous(&topology);
}

#[test]
fn draft_cylinder_carrier_rejects_conflicting_records() {
    let mut topology = topology();
    topology.surface_cylinders = vec![cylinder(1.0), cylinder(2.0)];
    assert_ambiguous(&topology);
}

#[test]
fn draft_axis_carrier_rejects_conflicting_records() {
    let mut topology = topology();
    topology.surface_axes = vec![axis(Vector3::new(0.0, 0.0, 1.0)), axis(Vector3::new(1.0, 0.0, 0.0))];
    assert_ambiguous(&topology);
}

#[test]
fn draft_axis_radius_rejects_conflicting_records() {
    let mut topology = topology();
    topology.surface_axes = vec![axis(Vector3::new(0.0, 0.0, 1.0))];
    topology.surface_radii = vec![AsmHistoricalSurfaceRadius { surface: 100, radius: 1.0 }, AsmHistoricalSurfaceRadius { surface: 100, radius: 2.0 }];
    assert_ambiguous(&topology);
}

#[test]
fn draft_cylinder_keeps_its_axis_and_radius_carriers() {
    let mut topology = topology();
    topology.surface_cylinders = vec![cylinder(2.0)];
    topology.surface_axes = vec![axis(Vector3::new(0.0, 0.0, 1.0))];
    topology.surface_radii = vec![AsmHistoricalSurfaceRadius { surface: 100, radius: 2.0 }];
    crate::test_support::with_decode_context(|ctx| {
        assert!(matches!(draft_surface_geometry(ctx, &topology, 10).unwrap(),
            Some(DraftSurfaceGeometry::Cylinder { radius: 2.0, .. })));
    });
}

#[test]
fn draft_duplicate_carrier_stops_before_unread_geometry() {
    let mut topology = topology();
    topology.surface_planes = vec![plane(Vector3::new(0.0, 0.0, 1.0)), plane(Vector3::new(1.0, 0.0, 0.0))];
    topology.surface_planes.extend((101..4200).map(|surface| {
        let mut plane = plane(Vector3::new(0.0, 1.0, 0.0));
        plane.surface = surface;
        plane
    }));
    topology.surface_cylinders = vec![cylinder(2.0)];
    let mut policy = DecodePolicy::service();
    // One binding plus the two matching plane records; no suffix is queried.
    policy.limits.max_work_units = 3;
    policy.limits.max_collection_items = 0;
    policy.limits.max_materialized_bytes = 0;
    policy.limits.max_retained_bytes = 0;
    crate::test_support::with_decode_policy(&policy, |ctx| {
        assert!(draft_surface_geometry(ctx, &topology, 10).unwrap().is_none());
        assert_eq!(ctx.resource_refusal(), None);
        assert!(matches!(ctx.charge_work(1, "after duplicate carrier proof"),
            Err(CodecError::ResourceLimit(limit)) if limit.dimension == ResourceDimension::WorkUnits
                && limit.used == 3 && limit.additional == 1));
    });
}

#[test]
fn draft_alternate_transition_rejects_conflicting_carrier_proof() {
    use crate::history::resolve_draft_face_by_surface_transition;
    use crate::records::{
        dimensions::DesignRecipeReference, recipes::ConstructionRecipeKind,
        topology::face::DesignFaceOperand,
    };
    use cadmpeg_ir::ids::FaceId;
    let face = |slot| FaceId::mint(format!("f3d:brep:entity#{slot}")).expect("identity grammar");
    let reference = |candidate_faces, alternate_selector_faces| DesignRecipeReference {
        selector: 2, selector_offset: 0, token: "1".into(), token_offset: 0,
        design_reference: 303, design_reference_offset: 0, candidate_faces,
        candidate_edges: Vec::new(), alternate_selector_faces,
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
    operand.recipe_references = vec![reference(Vec::new(), vec![face(10)])];


    let mut before = topology();
    before.surface_planes = vec![plane(Vector3::new(0.0, 0.0, 1.0))];
    let mut after = topology();
    after.surface_planes = vec![plane(Vector3::new(1.0, 0.0, 0.0))];
    crate::test_support::with_decode_context(|ctx| {
        assert_eq!(resolve_draft_face_by_surface_transition(ctx, &operand, &before, &after).unwrap(), Some(10));
    });
    for duplicate_before in [false, true] {
        let mut conflicting_before = before.clone();
        let mut conflicting_after = after.clone();
        if duplicate_before {
            conflicting_before.surface_planes.push(plane(Vector3::new(0.0, 1.0, 0.0)));
        } else {
            conflicting_after.surface_planes.push(plane(Vector3::new(0.0, 1.0, 0.0)));
        }
        crate::test_support::with_decode_context(|ctx| {
            assert_eq!(resolve_draft_face_by_surface_transition(ctx, &operand, &conflicting_before, &conflicting_after).unwrap(), None);
            assert_eq!(ctx.resource_refusal(), None);
        });
    }
}

#[test]
fn draft_carrier_proof_preserves_original_work_refusal() {
    let mut topology = topology();
    topology.surface_planes = vec![plane(Vector3::new(0.0, 0.0, 1.0))];
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = u64::MAX;
    crate::test_support::with_decode_policy(&policy, |ctx| {
        assert!(draft_surface_geometry(ctx, &topology, 10).unwrap().is_some());
        let _probe = cadmpeg_core::decode::refusal_probe::RefusalProbe::arm(
            ResourceDimension::WorkUnits, "find F3D draft surface geometry", Some(1),
        );
        let Err(CodecError::ResourceLimit(original)) = draft_surface_geometry(ctx, &topology, 10) else {
            panic!("Draft surface proof refusal");
        };
        assert_eq!(original.operation, "find F3D draft surface geometry");
        assert_eq!(original.additional, 1);
        let Err(CodecError::ResourceLimit(sticky)) = draft_surface_geometry(ctx, &AsmHistoricalTopology::default(), 10) else {
            panic!("sticky Draft surface proof refusal");
        };
        assert_eq!(sticky, original);
    });
}
