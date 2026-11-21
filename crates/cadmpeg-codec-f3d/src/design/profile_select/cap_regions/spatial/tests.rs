// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::sketches::{
    SpatialSketch, SpatialSketchEntity, SpatialSketchEntityUse, SpatialSketchGeometry,
    SpatialSketchGeometryDefinition, SpatialSketchId, SpatialSketchProfile,
};

use super::profiles_agree;
use crate::history_records::{
    AsmDeltaState, AsmHistoricalTopology, AsmHistoricalTransition, AsmHistory, AsmStateSnapshot,
};
use crate::ids::neutral_spatial_sketch_curve_id;

use super::super::tests::{plane_faces, PlaneFace};

const TOLERANCE: f64 = 1e-6;

/// Two squares on the XY plane: profile 0 spans 0..10 and profile 1 spans
/// 20..30 along x.
fn two_squares() -> (SpatialSketch, Vec<SpatialSketchEntity>) {
    let id = SpatialSketchId::mint("f3d:model:spatial-sketch#squares").unwrap();
    let mut entities = Vec::new();
    let mut profiles = Vec::new();
    for (square, x) in [(0_u64, 0.0), (1, 20.0)] {
        let corners = [(x, 0.0), (x + 10.0, 0.0), (x + 10.0, 10.0), (x, 10.0)];
        let mut uses = Vec::new();
        for k in 0..4 {
            let primary = square * 10 + k + 1;
            let start = corners[usize::try_from(k).unwrap()];
            let end = corners[usize::try_from((k + 1) % 4).unwrap()];
            let entity = neutral_spatial_sketch_curve_id(&id, primary, 0);
            // The last side is stored backwards and used reversed.
            let (stored_start, stored_end, reversed) = if k == 3 {
                (end, start, true)
            } else {
                (start, end, false)
            };
            entities.push(SpatialSketchEntity::new(
                entity.clone(),
                id.clone(),
                SpatialSketchGeometry::try_from(SpatialSketchGeometryDefinition::Line {
                    start: Point3::new(stored_start.0, stored_start.1, 0.0),
                    end: Point3::new(stored_end.0, stored_end.1, 0.0),
                })
                .unwrap(),
            ));
            uses.push(SpatialSketchEntityUse { entity, reversed });
        }
        profiles.push(
            SpatialSketchProfile::try_new(
                Point3::new(x, 0.0, 0.0),
                Vector3::new(0.0, 0.0, 1.0),
                Vector3::new(1.0, 0.0, 0.0),
                uses,
            )
            .unwrap(),
        );
    }
    (
        SpatialSketch {
            id,
            name: None,
            configuration: None,
            visible: None,
            native_ref: None,
            profiles,
        },
        entities,
    )
}

fn square(x: f64) -> Vec<(f64, f64)> {
    vec![(x, 0.0), (x + 10.0, 0.0), (x + 10.0, 10.0), (x, 10.0)]
}

/// A history whose state 1 follows the empty state 0 and holds `faces`.
fn extrusion(faces: &[PlaneFace]) -> AsmHistory {
    let topologies = [AsmHistoricalTopology::default(), plane_faces(faces)];
    let versions = BTreeMap::new();
    let mut states = Vec::new();
    for index in 0..topologies.len() {
        let snapshot = |i: usize| AsmStateSnapshot {
            state_id: i64::try_from(i).unwrap(),
            topology: &topologies[i],
            versions: &versions,
        };
        let transition =
            AsmHistoricalTransition::between(index.checked_sub(1).map(snapshot), snapshot(index))
                .unwrap();
        states.push(AsmDeltaState {
            id: format!("synthetic:test:state#{index}"),
            parent: "synthetic:test:history".into(),
            byte_offset: 0,
            state_id: i64::try_from(index).unwrap(),
            version_flag: 1,
            state_flag: 0,
            previous_ref: (index + 1 < topologies.len()).then(|| i64::try_from(index + 1).unwrap()),
            next_ref: index.checked_sub(1).map(|i| i64::try_from(i).unwrap()),
            node_index: i64::try_from(index).unwrap(),
            partner_ref: None,
            owner_ref: 0,
            bulletin_boards: Vec::new(),
            records: Vec::new(),
            transition: Some(transition),
        });
    }
    AsmHistory::new("synthetic:test:history".into(), 0, None, false, states)
}

#[test]
fn far_caps_that_trace_the_selected_profiles_certify_them() {
    let (sketch, entities) = two_squares();
    let history = extrusion(&[(1, 0.0, vec![square(0.0)]), (2, 3.0, vec![square(0.0)])]);
    let agree = |selected: &[u32]| {
        profiles_agree(
            &sketch,
            &entities,
            selected,
            std::slice::from_ref(&history),
            (1, 0),
            TOLERANCE,
        )
    };
    assert!(agree(&[0]));
    // Every profile is more than the far cap holds.
    assert!(!agree(&[0, 1]));
    assert!(!agree(&[1]));
}

#[test]
fn far_caps_of_both_profiles_certify_both_and_neither_alone() {
    let (sketch, entities) = two_squares();
    let history = extrusion(&[(1, -4.0, vec![square(0.0)]), (2, -4.0, vec![square(20.0)])]);
    let agree = |selected: &[u32]| {
        profiles_agree(
            &sketch,
            &entities,
            selected,
            std::slice::from_ref(&history),
            (1, 0),
            TOLERANCE,
        )
    };
    assert!(agree(&[0, 1]));
    assert!(!agree(&[0]));
    assert!(!agree(&[1]));
}

#[test]
fn caps_on_the_profile_plane_alone_certify_nothing() {
    let (sketch, entities) = two_squares();
    let history = extrusion(&[(1, 0.0, vec![square(0.0)])]);
    assert!(!profiles_agree(
        &sketch,
        &entities,
        &[0],
        std::slice::from_ref(&history),
        (1, 0),
        TOLERANCE,
    ));
}

#[test]
fn a_far_cap_with_a_hole_does_not_certify_the_filled_profile() {
    let (sketch, entities) = two_squares();
    let hole = vec![(2.0, 2.0), (2.0, 4.0), (4.0, 4.0), (4.0, 2.0)];
    let history = extrusion(&[(2, 3.0, vec![square(0.0), hole])]);
    assert!(!profiles_agree(
        &sketch,
        &entities,
        &[0],
        std::slice::from_ref(&history),
        (1, 0),
        TOLERANCE,
    ));
}

#[test]
fn a_transition_from_another_state_certifies_nothing() {
    let (sketch, entities) = two_squares();
    let history = extrusion(&[(2, 3.0, vec![square(0.0)])]);
    assert!(!profiles_agree(
        &sketch,
        &entities,
        &[0],
        std::slice::from_ref(&history),
        (1, 7),
        TOLERANCE,
    ));
}
