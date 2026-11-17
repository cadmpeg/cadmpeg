// SPDX-License-Identifier: Apache-2.0

use crate::history::face_support::historical_face_support_contexts;
use crate::history::{face_operand_support_candidates, TopologyQueryCache};
use crate::history_records::{AsmHistoricalCarrierBinding, AsmHistoricalTopology};
use crate::records::dimensions::DesignRecipeReference;
use cadmpeg_ir::ids::FaceId;
use std::collections::HashSet;

fn face(slot: i64) -> FaceId {
    FaceId::mint(format!("f3d:brep:entity#{slot}")).unwrap()
}

fn reference() -> DesignRecipeReference {
    DesignRecipeReference {
        selector: 2,
        selector_offset: 0,
        token: "37".into(),
        token_integer: 0,
        token_offset: 4,
        design_reference: -73,
        design_reference_offset: 8,
        candidate_faces: vec![face(10)],
        candidate_edges: Vec::new(),
        alternate_selector_faces: Vec::new(),
        alternate_selector_edges: Vec::new(),
        operand_offset: None,
    }
}

#[test]
fn extrude_support_keeps_the_first_reference_after_context_subtraction() {
    let topology = AsmHistoricalTopology {
        faces: vec![10, 11, 12, 13],
        face_surfaces: (10..14)
            .map(|entity| AsmHistoricalCarrierBinding {
                entity,
                carrier: entity + 100,
            })
            .collect(),
        ..Default::default()
    };
    let history =
        crate::test_support::history::history("f3d:history".into(), 0, None, false, Vec::new());
    let mut reference = reference();
    let preceding_candidates = vec![face(11), face(12)];
    for (keys, expected) in [
        (vec![face(10)], vec![10]),
        (vec![face(10), face(13)], vec![10, 13]),
        (Vec::new(), Vec::new()),
    ] {
        reference.candidate_faces = keys;
        let candidates =
            face_operand_support_candidates(&preceding_candidates, Some(&reference), Some(-73));
        let contexts = historical_face_support_contexts(
            &candidates,
            &history,
            &topology,
            &HashSet::new(),
            &mut TopologyQueryCache::default(),
        );
        let target_contexts = contexts
            .iter()
            .filter(|context| matches!(context.active_face_slot, 10 | 13))
            .collect::<Vec<_>>();
        assert_eq!(
            target_contexts
                .iter()
                .map(|context| context.active_face_slot)
                .collect::<Vec<_>>(),
            expected
        );
        for context in target_contexts {
            assert_eq!(context.preceding_face_slots, [context.active_face_slot]);
            assert_eq!(context.surface_slot, context.active_face_slot + 100);
        }
    }
}

#[test]
fn extrude_support_deduplicates_the_first_reference_and_uses_alternate_selectors() {
    let mut reference = reference();
    reference.alternate_selector_faces = std::mem::take(&mut reference.candidate_faces);
    let candidates = vec![face(10), face(11)];
    assert_eq!(
        face_operand_support_candidates(&candidates, Some(&reference), Some(-73)).as_ref(),
        candidates
    );
}

#[test]
fn extrude_support_requires_the_recipe_design_reference() {
    let reference = reference();
    let candidates = vec![face(11)];
    for recipe_reference in [None, Some(-74)] {
        assert_eq!(
            face_operand_support_candidates(&candidates, Some(&reference), recipe_reference)
                .as_ref(),
            candidates
        );
    }
}

#[test]
fn a_support_context_states_the_unique_preceding_plane_or_cylinder() {
    use crate::history_records::{AsmHistoricalCylinder, AsmHistoricalPlane};
    use crate::records::topology::historical_context::DesignHistoricalSurfaceGeometry;
    use cadmpeg_ir::math::{Point3, Vector3};

    let history =
        crate::test_support::history::history("f3d:history".into(), 0, None, false, Vec::new());
    let cylinder = AsmHistoricalCylinder {
        surface: 20,
        origin: Point3::new(1.0, 2.0, 3.0),
        axis: Vector3::new(1.0, 0.0, 0.0),
        radius: 3.25,
    };
    let plane = AsmHistoricalPlane {
        surface: 20,
        origin: Point3::new(0.0, 0.0, 2.5),
        normal: Vector3::new(0.0, 0.0, 1.0),
    };
    let surface = |cylinders: Vec<AsmHistoricalCylinder>, planes: Vec<AsmHistoricalPlane>| {
        let topology = AsmHistoricalTopology {
            faces: vec![4],
            face_surfaces: vec![AsmHistoricalCarrierBinding {
                entity: 4,
                carrier: 20,
            }],
            surface_cylinders: cylinders,
            surface_planes: planes,
            ..Default::default()
        };
        let contexts = historical_face_support_contexts(
            &[face(4)],
            &history,
            &topology,
            &HashSet::new(),
            &mut TopologyQueryCache::default(),
        );
        assert_eq!(contexts.len(), 1);
        contexts[0].preceding_surface.clone()
    };
    assert_eq!(
        surface(vec![cylinder.clone()], Vec::new()),
        Some(DesignHistoricalSurfaceGeometry::Cylinder {
            origin: cylinder.origin,
            axis: cylinder.axis,
            radius: 3.25,
        })
    );
    assert_eq!(
        surface(Vec::new(), vec![plane.clone()]),
        Some(DesignHistoricalSurfaceGeometry::Plane {
            origin: plane.origin,
            normal: plane.normal,
        })
    );
    assert_eq!(surface(vec![cylinder.clone()], vec![plane]), None);
    assert_eq!(surface(vec![cylinder.clone(), cylinder], Vec::new()), None);
    assert_eq!(surface(Vec::new(), Vec::new()), None);
}
