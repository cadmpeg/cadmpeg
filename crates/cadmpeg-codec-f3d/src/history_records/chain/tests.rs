// SPDX-License-Identifier: Apache-2.0
use super::*;
use crate::history_records::*;
use cadmpeg_ir::math::{Point3, Vector3};

fn topology(keys: &[i64], changed: bool) -> AsmHistoricalTopology {
    let value = if changed { 5.0 } else { 2.0 };
    let origin = Point3::new(value, 0.0, 0.0);
    let axis = Vector3::new(0.0, 0.0, 1.0);
    let mut topology = AsmHistoricalTopology {
        bodies: keys.to_vec(),
        regions: keys.to_vec(),
        shells: keys.to_vec(),
        faces: keys.to_vec(),
        loops: keys.to_vec(),
        coedges: keys.to_vec(),
        edges: keys.to_vec(),
        vertices: keys.to_vec(),
        points: keys.to_vec(),
        surfaces: keys.iter().flat_map(|&key| [key, key]).collect(),
        curves: keys.iter().flat_map(|&key| [key, key, key]).collect(),
        pcurves: keys.to_vec(),
        surface_radii: keys
            .iter()
            .map(|&key| AsmHistoricalSurfaceRadius {
                surface: key,
                radius: value,
            })
            .collect(),
        surface_cylinders: keys
            .iter()
            .map(|&key| AsmHistoricalCylinder {
                surface: key,
                origin,
                axis,
                radius: value,
            })
            .collect(),
        surface_planes: keys
            .iter()
            .map(|&key| AsmHistoricalPlane {
                surface: key,
                origin,
                normal: axis,
            })
            .collect(),
        surface_axes: keys
            .iter()
            .map(|&key| AsmHistoricalSurfaceAxis {
                surface: key,
                origin,
                direction: axis,
            })
            .collect(),
        curve_axes: keys
            .iter()
            .map(|&key| AsmHistoricalCurveAxis {
                curve: key,
                origin,
                direction: axis,
            })
            .collect(),
        body_regions: keys
            .iter()
            .map(|&key| AsmHistoricalRelation {
                owner_ref: key,
                member_refs: vec![if changed { 9 } else { 8 }],
            })
            .collect(),
        region_shells: keys
            .iter()
            .map(|&key| AsmHistoricalRelation {
                owner_ref: key,
                member_refs: vec![if changed { 9 } else { 8 }],
            })
            .collect(),
        shell_faces: keys
            .iter()
            .map(|&key| AsmHistoricalRelation {
                owner_ref: key,
                member_refs: vec![if changed { 9 } else { 8 }],
            })
            .collect(),
        shell_wire_edges: keys
            .iter()
            .map(|&key| AsmHistoricalRelation {
                owner_ref: key,
                member_refs: vec![if changed { 9 } else { 8 }],
            })
            .collect(),
        shell_free_vertices: keys
            .iter()
            .map(|&key| AsmHistoricalRelation {
                owner_ref: key,
                member_refs: vec![if changed { 9 } else { 8 }],
            })
            .collect(),
        face_loops: keys
            .iter()
            .map(|&key| AsmHistoricalRelation {
                owner_ref: key,
                member_refs: vec![if changed { 9 } else { 8 }],
            })
            .collect(),
        loop_coedges: keys
            .iter()
            .map(|&key| AsmHistoricalRelation {
                owner_ref: key,
                member_refs: vec![if changed { 9 } else { 8 }],
            })
            .collect(),
        coedge_topology: keys
            .iter()
            .map(|&key| AsmHistoricalCoedge {
                coedge: key,
                owner_loop: 8,
                edge: 8,
                next: 8,
                previous: 8,
                radial_next: if changed { 9 } else { 8 },
            })
            .collect(),
        edge_vertices: keys
            .iter()
            .map(|&key| AsmHistoricalEdge {
                edge: key,
                start_vertex: 8,
                end_vertex: if changed { 9 } else { 8 },
            })
            .collect(),
        face_surfaces: keys
            .iter()
            .map(|&key| AsmHistoricalCarrierBinding {
                entity: key,
                carrier: if changed { 9 } else { 8 },
            })
            .collect(),
        vertex_points: keys
            .iter()
            .map(|&key| AsmHistoricalCarrierBinding {
                entity: key,
                carrier: if changed { 9 } else { 8 },
            })
            .collect(),
        edge_curves: keys
            .iter()
            .map(|&key| AsmHistoricalOptionalCarrierBinding {
                entity: key,
                carrier: if changed { None } else { Some(8) },
            })
            .collect(),
        coedge_pcurves: keys
            .iter()
            .map(|&key| AsmHistoricalOptionalCarrierBinding {
                entity: key,
                carrier: if changed { None } else { Some(8) },
            })
            .collect(),
        point_positions: keys
            .iter()
            .map(|&key| AsmHistoricalPoint {
                point: key,
                position: origin,
            })
            .collect(),
        ..AsmHistoricalTopology::default()
    };
    for attribute in [10, 20] {
        for ordinal in 0..2 {
            topology
                .persistent_subentity_tags
                .push(AsmHistoricalPersistentSubentityTag {
                    entity_kind:
                        crate::records::topology::body_recipe::AsmHistoricalEntityKind::Face,
                    entity_ref: attribute,
                    selector: attribute,
                    token: if changed { "changed" } else { "tag" }.into(),
                    design_references: vec![30],
                    ordinal,
                });
        }
    }
    topology
}

fn chain() -> AsmHistory {
    let topologies = [
        AsmHistoricalTopology {
            bodies: vec![1],
            ..Default::default()
        },
        topology(&[1, 2], false),
        topology(&[2, 3], false),
        topology(&[2, 3], true),
    ];
    // The tail has membership but no retained relations.
    let versions: Vec<BTreeMap<_, _>> = [
        vec![(0, 0), (1, 1)],
        vec![(0, 0), (1, 1), (2, 2), (10, 10), (20, 20)],
        vec![(0, 0), (2, 2), (3, 3), (10, 10), (20, 20)],
        vec![(0, 0), (2, 22), (3, 33), (10, 110), (20, 120)],
    ]
    .into_iter()
    .map(|rows| rows.into_iter().collect())
    .collect();
    let mut states = Vec::new();
    for index in 0usize..4 {
        let snapshot = |index: usize| AsmStateSnapshot {
            state_id: i64::try_from(index).unwrap(),
            topology: &topologies[index],
            versions: &versions[index],
        };
        let transition =
            AsmHistoricalTransition::between(index.checked_sub(1).map(snapshot), snapshot(index))
                .unwrap();
        states.push(AsmDeltaState {
            id: format!("state-{index}"),
            parent: "history".into(),
            byte_offset: 0,
            state_id: i64::try_from(index).unwrap(),
            version_flag: 1,
            state_flag: 0,
            previous_ref: (index < 3).then(|| i64::try_from(index + 1).unwrap()),
            next_ref: index
                .checked_sub(1)
                .map(|index| i64::try_from(index).unwrap()),
            node_index: i64::try_from(index).unwrap(),
            partner_ref: None,
            owner_ref: 0,
            bulletin_boards: Vec::new(),
            records: Vec::new(),
            transition: Some(transition),
        });
    }
    // Storage order must not determine chain order.
    states.swap(0, 2);
    AsmHistory {
        id: "history".into(),
        byte_offset: 0,
        preamble: None,
        binding_budget_exceeded: false,
        states,
        topology_scope: AsmTopologyScope::Complete,
        materialized: crate::history_records::MaterializedTopologies::default(),
    }
}

fn expected_versions(state_id: i64) -> BTreeMap<i64, i64> {
    match state_id {
        0 => vec![(0, 0), (1, 1)],
        1 => vec![(0, 0), (1, 1), (2, 2), (10, 10), (20, 20)],
        2 => vec![(0, 0), (2, 2), (3, 3), (10, 10), (20, 20)],
        3 => vec![(0, 0), (2, 22), (3, 33), (10, 110), (20, 120)],
        _ => unreachable!(),
    }
    .into_iter()
    .collect()
}

fn expected_topology(state_id: i64, scope: AsmTopologyScope) -> Option<AsmHistoricalTopology> {
    let mut expected = match state_id {
        0 => AsmHistoricalTopology {
            bodies: vec![1],
            ..Default::default()
        },
        1 => topology(&[1, 2], false),
        2 => topology(&[2, 3], false),
        3 => topology(&[2, 3], true),
        _ => unreachable!(),
    };
    if scope == AsmTopologyScope::Retained {
        expected.surface_radii.clear();
        expected.surface_cylinders.clear();
        expected.surface_axes.clear();
        expected.persistent_subentity_tags.clear();
        expected.body_regions.clear();
        expected.region_shells.clear();
        expected.shell_faces.clear();
        expected.shell_wire_edges.clear();
        expected.shell_free_vertices.clear();
        expected.edge_vertices.clear();
        expected.vertex_points.clear();
        expected.point_positions.clear();
        if !expected.has_retained_rows() {
            return None;
        }
    }
    Some(expected)
}

#[test]
fn reconstructs_every_family_in_both_scopes() {
    let mut history = chain();
    assert_eq!(history.check_transitions(), Ok(()));
    for scope in [AsmTopologyScope::Complete, AsmTopologyScope::Retained] {
        for state in history.states() {
            assert_eq!(
                history.topology(state),
                expected_topology(state.state_id, scope).as_ref(),
                "{scope:?} state {}",
                state.state_id
            );
            let expected = expected_topology(state.state_id, scope);
            let slots = expected
                .as_ref()
                .map(AsmHistoricalTopology::slots)
                .unwrap_or_default();
            let versions: Vec<_> = expected_versions(state.state_id)
                .into_iter()
                .filter(|(slot, _)| slots.contains(slot))
                .map(|(entity_ref, record_ref)| AsmEntityVersion {
                    entity_ref,
                    record_ref,
                })
                .collect();
            assert_eq!(history.entity_versions(state), versions);
        }
        crate::history::discard_projection_caches(std::slice::from_mut(&mut history));
    }
    assert!(history.topology(&history.states()[2]).is_none());
}

#[test]
fn cache_only_materializes_requested_members_and_resets() {
    let mut history = chain();
    assert!(history.materialized.states.get().is_none());
    assert!(history.topology(&history.states()[0]).is_some());
    let cache = history.materialized.states.get().unwrap();
    assert_eq!(
        cache.iter().filter(|entry| entry.get().is_some()).count(),
        1
    );
    let outsider = history.states()[0].clone();
    assert!(history.topology(&outsider).is_none());
    assert!(history.entity_versions(&outsider).is_empty());
    assert!(history.clone().materialized.states.get().is_none());
    history.release_topologies();
    assert!(history.materialized.states.get().is_none());
}

#[test]
fn transition_and_history_serde_preserve_data_and_clear_cache() {
    let history = chain();
    for state in history.states() {
        assert!(history.topology(state).is_some());
        let transition = state.transition.as_ref().unwrap();
        let bytes = serde_json::to_vec(transition).unwrap();
        assert_eq!(*transition, serde_json::from_slice(&bytes).unwrap());
    }
    let bytes = serde_json::to_vec(&history).unwrap();
    let decoded: AsmHistory = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(decoded, history);
    assert!(decoded.materialized.states.get().is_none());
    for state in decoded.states() {
        assert_eq!(
            decoded.topology(state),
            expected_topology(state.state_id, AsmTopologyScope::Complete).as_ref()
        );
    }
}

fn transition_mut(history: &mut AsmHistory, id: i64) -> &mut AsmHistoricalTransition {
    history
        .states_mut()
        .iter_mut()
        .find(|state| state.state_id == id)
        .unwrap()
        .transition
        .as_mut()
        .unwrap()
}

#[test]
fn transition_invariant_errors() {
    use AsmTransitionError::*;
    type BrokenChain = (AsmTransitionError, fn(&mut AsmHistory));
    let cases: Vec<BrokenChain> = vec![
        (MissingTransition, |h| h.states_mut()[0].transition = None),
        (Chain, |h| h.states_mut()[0].next_ref = Some(999)),
        (Chain, |h| h.states_mut()[2].next_ref = Some(3)),
        (Chain, |h| {
            h.states_mut()[0].node_index = h.states()[1].node_index;
        }),
        (PreviousState, |h| {
            transition_mut(h, 1).previous_state_id = Some(99);
        }),
        (PreviousState, |h| {
            transition_mut(h, 0).previous_state_id = Some(99);
        }),
        (InsertedPresent, |h| {
            let t = transition_mut(h, 1);
            t.records.inserted.insert(0, 1);
            t.record_revisions.insert(
                0,
                AsmEntityVersion {
                    entity_ref: 1,
                    record_ref: 1,
                },
            );
        }),
        (DeletedAbsent, |h| {
            transition_mut(h, 1).records.deleted.push(99);
        }),
        (InvalidUpdate, |h| {
            let t = transition_mut(h, 3);
            t.record_revisions[0].record_ref = 2;
        }),
        (InvalidUpdate, |h| {
            let t = transition_mut(h, 0);
            t.records.updated.push(99);
            t.record_revisions.push(AsmEntityVersion {
                entity_ref: 99,
                record_ref: 100,
            });
        }),
        (RevisionCoverage, |h| {
            transition_mut(h, 0).record_revisions.pop();
        }),
        (RevisionCoverage, |h| {
            transition_mut(h, 0)
                .record_revisions
                .push(AsmEntityVersion {
                    entity_ref: 99,
                    record_ref: 99,
                });
        }),
        (RemovedRowAbsent, |h| {
            transition_mut(h, 0).rows.face_surfaces.removed.push(99);
        }),
        (UnsortedFamily, |h| {
            transition_mut(h, 1).topology.faces.inserted.reverse();
        }),
        (UnsortedFamily, |h| {
            transition_mut(h, 1).rows.face_surfaces.set.reverse();
        }),
        (InsertedPresent, |h| {
            transition_mut(h, 2).topology.faces.inserted.insert(0, 2);
        }),
        (DeletedAbsent, |h| {
            transition_mut(h, 2).topology.faces.deleted.push(99);
        }),
        (InvalidUpdate, |h| {
            transition_mut(h, 0).topology.faces.updated.push(99);
        }),
        (InvalidUpdate, |h| {
            transition_mut(h, 3).topology.faces.updated = vec![2];
        }),
    ];
    for (case, (expected, break_chain)) in cases.into_iter().enumerate() {
        let mut history = chain();
        break_chain(&mut history);
        assert_eq!(history.check_transitions(), Err(expected), "case {case}");
        let head = history
            .states()
            .iter()
            .find(|state| state.state_id == 3)
            .unwrap();
        assert!(history.topology(head).is_none(), "case {case}");
        assert!(history.entity_versions(head).is_empty(), "case {case}");
    }
    let mut unbound = chain();
    for state in unbound.states_mut() {
        state.transition = None;
    }
    assert_eq!(unbound.check_transitions(), Ok(()));
    for state in unbound.states() {
        assert!(unbound.topology(state).is_none());
    }
}

#[test]
fn between_rejects_unsorted_families_without_reordering() {
    let versions = BTreeMap::new();
    let check = |topology: &AsmHistoricalTopology| {
        let snapshot = || AsmStateSnapshot {
            state_id: 1,
            topology,
            versions: &versions,
        };
        assert!(AsmHistoricalTransition::between(None, snapshot()).is_none());
        assert!(AsmHistoricalTransition::between(
            Some(snapshot()),
            AsmStateSnapshot {
                state_id: 2,
                topology: &AsmHistoricalTopology::default(),
                versions: &versions
            }
        )
        .is_none());
    };
    {
        let mut bad = topology(&[1, 2], false);
        bad.bodies.reverse();
        check(&bad);
    }
    {
        let mut bad = topology(&[1, 2], false);
        bad.regions.reverse();
        check(&bad);
    }
    {
        let mut bad = topology(&[1, 2], false);
        bad.shells.reverse();
        check(&bad);
    }
    {
        let mut bad = topology(&[1, 2], false);
        bad.faces.reverse();
        check(&bad);
    }
    {
        let mut bad = topology(&[1, 2], false);
        bad.loops.reverse();
        check(&bad);
    }
    {
        let mut bad = topology(&[1, 2], false);
        bad.coedges.reverse();
        check(&bad);
    }
    {
        let mut bad = topology(&[1, 2], false);
        bad.edges.reverse();
        check(&bad);
    }
    {
        let mut bad = topology(&[1, 2], false);
        bad.vertices.reverse();
        check(&bad);
    }
    {
        let mut bad = topology(&[1, 2], false);
        bad.points.reverse();
        check(&bad);
    }
    {
        let mut bad = topology(&[1, 2], false);
        bad.surfaces.reverse();
        check(&bad);
    }
    {
        let mut bad = topology(&[1, 2], false);
        bad.curves.reverse();
        check(&bad);
    }
    {
        let mut bad = topology(&[1, 2], false);
        bad.pcurves.reverse();
        check(&bad);
    }
    {
        let mut bad = topology(&[1, 2], false);
        bad.surface_radii.reverse();
        check(&bad);
    }
    {
        let mut bad = topology(&[1, 2], false);
        bad.surface_cylinders.reverse();
        check(&bad);
    }
    {
        let mut bad = topology(&[1, 2], false);
        bad.surface_planes.reverse();
        check(&bad);
    }
    {
        let mut bad = topology(&[1, 2], false);
        bad.surface_axes.reverse();
        check(&bad);
    }
    {
        let mut bad = topology(&[1, 2], false);
        bad.curve_axes.reverse();
        check(&bad);
    }
    {
        let mut bad = topology(&[1, 2], false);
        bad.body_regions.reverse();
        check(&bad);
    }
    {
        let mut bad = topology(&[1, 2], false);
        bad.region_shells.reverse();
        check(&bad);
    }
    {
        let mut bad = topology(&[1, 2], false);
        bad.shell_faces.reverse();
        check(&bad);
    }
    {
        let mut bad = topology(&[1, 2], false);
        bad.shell_wire_edges.reverse();
        check(&bad);
    }
    {
        let mut bad = topology(&[1, 2], false);
        bad.shell_free_vertices.reverse();
        check(&bad);
    }
    {
        let mut bad = topology(&[1, 2], false);
        bad.face_loops.reverse();
        check(&bad);
    }
    {
        let mut bad = topology(&[1, 2], false);
        bad.loop_coedges.reverse();
        check(&bad);
    }
    {
        let mut bad = topology(&[1, 2], false);
        bad.coedge_topology.reverse();
        check(&bad);
    }
    {
        let mut bad = topology(&[1, 2], false);
        bad.edge_vertices.reverse();
        check(&bad);
    }
    {
        let mut bad = topology(&[1, 2], false);
        bad.face_surfaces.reverse();
        check(&bad);
    }
    {
        let mut bad = topology(&[1, 2], false);
        bad.vertex_points.reverse();
        check(&bad);
    }
    {
        let mut bad = topology(&[1, 2], false);
        bad.edge_curves.reverse();
        check(&bad);
    }
    {
        let mut bad = topology(&[1, 2], false);
        bad.coedge_pcurves.reverse();
        check(&bad);
    }
    {
        let mut bad = topology(&[1, 2], false);
        bad.point_positions.reverse();
        check(&bad);
    }
    let mut bad = topology(&[1, 2], false);
    bad.persistent_subentity_tags.reverse();
    check(&bad);
}

#[test]
fn history_remains_sync() {
    fn assert_sync<T: Sync>() {}
    assert_sync::<AsmHistory>();
}

#[test]
fn older_materialization_does_not_validate_newer_deltas() {
    let history = &mut chain();
    transition_mut(history, 3)
        .rows
        .face_surfaces
        .removed
        .push(99);
    let older = history
        .states()
        .iter()
        .find(|state| state.state_id == 2)
        .unwrap();
    assert_eq!(
        history.topology(older),
        expected_topology(older.state_id, AsmTopologyScope::Complete).as_ref()
    );
    assert_eq!(
        history.entity_versions(older),
        expected_versions(older.state_id)
            .into_iter()
            .filter(
                |(slot, _)| expected_topology(older.state_id, AsmTopologyScope::Complete)
                    .unwrap()
                    .slots()
                    .contains(slot)
            )
            .map(|(entity_ref, record_ref)| AsmEntityVersion {
                entity_ref,
                record_ref
            })
            .collect::<Vec<_>>()
    );
    assert_eq!(
        history.check_transitions(),
        Err(AsmTransitionError::RemovedRowAbsent)
    );
    let newer = history
        .states()
        .iter()
        .find(|state| state.state_id == 3)
        .unwrap();
    assert!(history.topology(newer).is_none());
}

#[test]
fn cached_older_state_keeps_record_revision_validation() {
    let history = &mut chain();
    transition_mut(history, 3).record_revisions[0].record_ref = 2;
    let older = history
        .states()
        .iter()
        .find(|state| state.state_id == 2)
        .unwrap();
    assert_eq!(
        history.topology(older),
        expected_topology(older.state_id, AsmTopologyScope::Complete).as_ref()
    );
    let newer = history
        .states()
        .iter()
        .find(|state| state.state_id == 3)
        .unwrap();
    assert!(history.topology(newer).is_none());
}

#[test]
fn empty_deltas_leave_family_storage_untouched() {
    let mut family = topology(&[1, 2], false);
    let before_rows = family.face_surfaces.as_ptr();
    let before_members = family.faces.as_ptr();
    let before_tags = family.persistent_subentity_tags.as_ptr();
    AsmHistoricalTransition {
        previous_state_id: None,
        records: AsmHistoricalEntityDelta::default(),
        record_revisions: Vec::new(),
        topology: AsmHistoricalTopologyDelta::default(),
        rows: AsmHistoricalRowsDelta::default(),
    }
    .apply_topology(&mut family)
    .unwrap();
    assert_eq!(family.face_surfaces.as_ptr(), before_rows);
    assert_eq!(family.faces.as_ptr(), before_members);
    assert_eq!(family.persistent_subentity_tags.as_ptr(), before_tags);
}

#[test]
fn linear_merges_reconstruct_all_small_membership_and_row_changes() {
    for old_mask in 0..8 {
        for new_mask in 0..8 {
            let keys = |mask| {
                (1..=3)
                    .filter(|key| mask & (1 << (key - 1)) != 0)
                    .collect::<Vec<_>>()
            };
            let old_keys = keys(old_mask);
            let new_keys = keys(new_mask);
            let old = topology(&old_keys, false);
            let new = topology(&new_keys, true);
            let old_versions = old_keys
                .iter()
                .map(|&key| (key, key))
                .collect::<BTreeMap<_, _>>();
            let new_versions = new_keys
                .iter()
                .map(|&key| (key, key + 10))
                .collect::<BTreeMap<_, _>>();
            let transition = AsmHistoricalTransition::between(
                Some(AsmStateSnapshot {
                    state_id: 1,
                    topology: &old,
                    versions: &old_versions,
                }),
                AsmStateSnapshot {
                    state_id: 2,
                    topology: &new,
                    versions: &new_versions,
                },
            )
            .unwrap();
            let mut actual = old;
            transition.apply_topology(&mut actual).unwrap();
            assert_eq!(actual, new, "{old_mask} -> {new_mask}");
            let mut actual_versions = old_versions
                .into_iter()
                .map(|(entity_ref, record_ref)| AsmEntityVersion {
                    entity_ref,
                    record_ref,
                })
                .collect();
            transition.apply_versions(&mut actual_versions).unwrap();
            assert_eq!(
                actual_versions,
                new_versions
                    .into_iter()
                    .map(|(entity_ref, record_ref)| AsmEntityVersion {
                        entity_ref,
                        record_ref
                    })
                    .collect::<Vec<_>>()
            );
        }
    }
}

#[test]
fn tag_groups_keep_decode_order_within_each_entity() {
    let original = topology(&[1, 2], false);
    let groups = entity_tags(&original);
    assert_eq!(
        groups.iter().map(AsmHistoricalRow::key).collect::<Vec<_>>(),
        [10, 20]
    );
    assert_eq!(
        groups
            .iter()
            .flat_map(|group| &group.tags)
            .cloned()
            .collect::<Vec<_>>(),
        original.persistent_subentity_tags
    );
    for group in groups {
        assert_eq!(
            group.tags.iter().map(|tag| tag.ordinal).collect::<Vec<_>>(),
            [0, 1]
        );
    }
}

#[test]
fn carrier_groups_preserve_membership_multiplicity_changes() {
    let topologies = [
        AsmHistoricalTopology::default(),
        AsmHistoricalTopology {
            surfaces: vec![2, 2, 4],
            curves: vec![3, 3, 3],
            ..Default::default()
        },
        AsmHistoricalTopology {
            surfaces: vec![2, 4, 4, 4],
            curves: vec![3, 3, 3, 5, 5],
            ..Default::default()
        },
        AsmHistoricalTopology {
            surfaces: vec![4, 4],
            ..Default::default()
        },
        AsmHistoricalTopology::default(),
    ];
    let versions: Vec<BTreeMap<_, _>> = topologies
        .iter()
        .map(|topology| topology.slots().into_iter().map(|key| (key, key)).collect())
        .collect();
    let mut materialized = AsmHistoricalTopology::default();
    let mut retained = AsmHistoricalTopology::default();
    let mut materialized_versions = Vec::new();
    for index in 0usize..topologies.len() {
        let snapshot = |index: usize| AsmStateSnapshot {
            state_id: i64::try_from(index).unwrap(),
            topology: &topologies[index],
            versions: &versions[index],
        };
        let transition =
            AsmHistoricalTransition::between(index.checked_sub(1).map(snapshot), snapshot(index))
                .unwrap();
        if index == 1 {
            assert_eq!(transition.topology.surfaces.inserted, [2, 4]);
            assert_eq!(transition.topology.curves.inserted, [3]);
            assert_eq!(
                transition.rows.surface_repeats.set[0],
                AsmHistoricalSlotRepeat { slot: 2, count: 2 }
            );
            assert_eq!(
                transition.rows.curve_repeats.set[0],
                AsmHistoricalSlotRepeat { slot: 3, count: 3 }
            );
        }
        if index == 2 {
            assert_eq!(
                transition.topology.surfaces,
                AsmHistoricalEntityDelta::default()
            );
            assert_eq!(
                transition.rows.surface_repeats.set,
                [AsmHistoricalSlotRepeat { slot: 4, count: 3 }]
            );
            assert_eq!(transition.rows.surface_repeats.removed, [2]);
        }
        let mut decoded: AsmHistoricalTransition =
            serde_json::from_slice(&serde_json::to_vec(&transition).unwrap()).unwrap();
        decoded.apply_versions(&mut materialized_versions).unwrap();
        decoded.apply_topology(&mut materialized).unwrap();
        assert_eq!(materialized, topologies[index]);
        decoded.rows.retain_projection_rows();
        decoded.apply_topology(&mut retained).unwrap();
        assert_eq!(retained, topologies[index]);
    }
}

#[test]
fn carrier_groups_reject_inconsistent_memberships() {
    let cases: Vec<fn(&mut AsmHistory)> = vec![
        |history| {
            transition_mut(history, 1).rows.surface_repeats.set[0].count = 1;
        },
        |history| {
            transition_mut(history, 1)
                .rows
                .curve_repeats
                .set
                .last_mut()
                .unwrap()
                .slot = 99;
        },
        |history| {
            transition_mut(history, 1).rows.surface_repeats.set[0].count = 0;
        },
        |history| {
            transition_mut(history, 1).topology.curves.inserted.pop();
        },
    ];
    for (case, corrupt) in cases.into_iter().enumerate() {
        let mut history = chain();
        corrupt(&mut history);
        assert_eq!(
            history.check_transitions(),
            Err(AsmTransitionError::MembershipCoverage),
            "case {case}"
        );
        let head = history
            .states()
            .iter()
            .find(|state| state.state_id == 3)
            .unwrap();
        assert!(history.topology(head).is_none(), "case {case}");
    }
}

#[test]
fn states_mut_invalidates_materializations_before_mutation() {
    let mut history = chain();
    let index = history
        .states()
        .iter()
        .position(|s| s.state_id == 3)
        .unwrap();
    assert!(history.topology(&history.states()[index]).is_some());
    history.states_mut()[index]
        .transition
        .as_mut()
        .unwrap()
        .previous_state_id = Some(-1);
    assert!(history.materialized.states.get().is_none());
    assert!(history.topology(&history.states()[index]).is_none());
    assert_eq!(
        history.check_transitions(),
        Err(AsmTransitionError::PreviousState)
    );
}
