// SPDX-License-Identifier: Apache-2.0

use super::split_plane::{split_body_test_feature, two_solid_boxes_with_planes};
use super::{scope_between, state};
use crate::history::bind_feature_outputs;
use crate::history_records::AsmHistoricalTopology;
use cadmpeg_ir::math::Point3;

fn deleted_bodies(after: AsmHistoricalTopology, before: AsmHistoricalTopology) -> Option<String> {
    let scope = scope_between(1, 0);
    let history = crate::test_support::history::history(
        "f3d:asm-history#deleted".into(),
        0,
        None,
        false,
        vec![state(1, 1, Some(0), after), state(0, 0, None, before)],
    );
    let mut feature = split_body_test_feature(&scope.id);
    bind_feature_outputs(
        std::slice::from_mut(&mut feature),
        std::slice::from_ref(&scope),
        &mut [history],
        &[],
        &Default::default(),
        None,
    )
    .unwrap();
    feature
        .source_properties
        .get(&cadmpeg_core::nonblank_literal!("deleted_bodies"))
        .cloned()
}

#[test]
fn a_body_the_feature_state_still_holds_is_not_deleted() {
    let before = two_solid_boxes_with_planes(
        1,
        Point3::new(0.0, 0.0, 0.0),
        Point3::new(10.0, 10.0, 10.0),
        2,
        Point3::new(12.0, 0.0, 0.0),
        Point3::new(15.0, 5.0, 5.0),
        vec![],
    );
    // Body 2 stays in the state, but one coedge of its edge is gone, so its
    // shell is open.
    let mut open = before.clone();
    open.loop_coedges
        .iter_mut()
        .find(|relation| relation.owner_ref == 105)
        .unwrap()
        .member_refs = vec![106];
    open.coedge_topology.retain(|coedge| coedge.coedge != 107);
    let coedge = open
        .coedge_topology
        .iter_mut()
        .find(|coedge| coedge.coedge == 106)
        .unwrap();
    (coedge.next, coedge.previous, coedge.radial_next) = (106, 106, 106);
    assert_eq!(deleted_bodies(open, before.clone()), None);

    let mut removed = before.clone();
    removed.bodies.retain(|&body| body != 2);
    removed
        .body_regions
        .retain(|relation| relation.owner_ref != 2);
    assert_eq!(
        deleted_bodies(removed, before.clone()),
        Some("2".to_owned())
    );

    assert_eq!(deleted_bodies(before.clone(), before), None);
}
