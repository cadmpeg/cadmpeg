// SPDX-License-Identifier: Apache-2.0
//! Compare chain reconstruction with independent full record-table decodes.

use crate::history_records::{AsmEntityVersion, AsmHistoricalTopology, AsmHistory};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use std::collections::BTreeMap;

type Snapshot = (usize, AsmHistoricalTopology, BTreeMap<i64, i64>);

fn full_decode(
    ctx: &DecodeContext<'_>,
    history: &AsmHistory,
    bytes: &[u8],
    width: cadmpeg_asm::kernel_header::RefWidth,
) -> Vec<Snapshot> {
    let states = history.states();
    let archive = crate::history::decode_record_archive(ctx, states, bytes, width)
        .unwrap()
        .unwrap();
    let active_count = crate::history::archived_active_record_count(states)
        .unwrap()
        .or_else(|| crate::history::insert_only_active_record_count(states))
        .unwrap();
    let mut archived_ids: Vec<_> = states
        .iter()
        .flat_map(|s| &s.records)
        .filter_map(|r| r.revision_id)
        .collect();
    archived_ids.sort_unstable();
    let mut snapshots = Vec::new();
    assert!(crate::history::walk_entity_versions(
        states,
        i64::try_from(active_count).unwrap(),
        &archived_ids,
        |ordinal, versions| {
            let records = crate::history::materialize_record_table(versions, &archive).unwrap();
            let decoded =
                crate::brep::decode_history_topology(ctx, &records, bytes, crate::ids::ID_FORMAT)
                    .unwrap();
            let topology = crate::history::historical_topology_with_tags(&decoded)
                .unwrap()
                .unwrap();
            snapshots.push((ordinal, topology, versions.clone()));
        }
    ));
    snapshots
}

fn retained(topology: &AsmHistoricalTopology) -> Option<AsmHistoricalTopology> {
    let retained = AsmHistoricalTopology {
        bodies: topology.bodies.clone(),
        regions: topology.regions.clone(),
        shells: topology.shells.clone(),
        faces: topology.faces.clone(),
        loops: topology.loops.clone(),
        coedges: topology.coedges.clone(),
        edges: topology.edges.clone(),
        vertices: topology.vertices.clone(),
        points: topology.points.clone(),
        surfaces: topology.surfaces.clone(),
        curves: topology.curves.clone(),
        pcurves: topology.pcurves.clone(),
        face_surfaces: topology.face_surfaces.clone(),
        surface_planes: topology.surface_planes.clone(),
        loop_coedges: topology.loop_coedges.clone(),
        coedge_topology: topology.coedge_topology.clone(),
        face_loops: topology.face_loops.clone(),
        edge_curves: topology.edge_curves.clone(),
        coedge_pcurves: topology.coedge_pcurves.clone(),
        curve_axes: topology.curve_axes.clone(),
        ..Default::default()
    };
    (!retained.face_surfaces.is_empty()
        || !retained.surface_planes.is_empty()
        || !retained.loop_coedges.is_empty()
        || !retained.coedge_topology.is_empty()
        || !retained.face_loops.is_empty()
        || !retained.edge_curves.is_empty()
        || !retained.coedge_pcurves.is_empty()
        || !retained.curve_axes.is_empty())
    .then_some(retained)
}

fn compare(history: &AsmHistory, snapshots: &[Snapshot], is_retained: bool) {
    assert_eq!(history.check_transitions(), Ok(()), "{}", history.id);
    for (ordinal, topology, versions) in snapshots {
        let state = &history.states()[*ordinal];
        let retained = is_retained.then(|| retained(topology));
        let expected = retained.as_ref().map_or(Some(topology), Option::as_ref);
        assert_eq!(
            history.topology(state),
            expected,
            "{} state {}",
            history.id,
            state.state_id
        );
        let expected_versions: Vec<_> = expected.map_or_else(Vec::new, |topology| {
            let slots = topology.slots();
            versions
                .iter()
                .filter(|(slot, _)| slots.contains(slot))
                .map(|(&entity_ref, &record_ref)| AsmEntityVersion {
                    entity_ref,
                    record_ref,
                })
                .collect()
        });
        assert_eq!(
            history.entity_versions(state),
            expected_versions,
            "{} state {}",
            history.id,
            state.state_id
        );
    }
}

fn transition_bytes(histories: &[AsmHistory]) -> usize {
    let transitions: Vec<_> = histories
        .iter()
        .flat_map(|h| h.states().iter().map(|s| &s.transition))
        .collect();
    serde_json::to_vec(&transitions).unwrap().len()
}

#[test]
#[ignore = "requires CADMPEG_F3D_HISTORY_ORACLE design paths"]
fn history_deltas_match_snapshots() {
    let paths = std::env::var("CADMPEG_F3D_HISTORY_ORACLE").unwrap();
    for path in paths.split(':') {
        let bytes = std::fs::read(path).unwrap();
        let arena = DecodeArena::new();
        let policy = DecodePolicy::desktop();
        let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).unwrap();
        let scan = crate::container::scan(&ctx, root).unwrap();
        let mut allowance = crate::history::history_binding_allowance(&policy);
        let mut histories = Vec::new();
        let mut snapshots = Vec::new();
        for brep in crate::container::history_breps(&scan) {
            let width = brep
                .kernel
                .as_ref()
                .and_then(crate::container::KernelFraming::asm_header)
                .map_or(cadmpeg_asm::kernel_header::RefWidth::Eight, |header| {
                    header.width
                });
            let bytes = scan.entry_bytes(&brep.name).unwrap();
            if let Some(mut history) =
                crate::history::decode(&ctx, bytes, &brep.name, width, &mut allowance).unwrap()
            {
                assert!(
                    history
                        .states()
                        .iter()
                        .all(|state| history.topology(state).is_some()),
                    "{path}: unbound history {}",
                    history.id
                );
                let stats = crate::history::binding::bind(&ctx, history.states_mut(), bytes, width)
                    .unwrap();
                history.release_topologies();
                let full_started = std::time::Instant::now();
                let decoded = full_decode(&ctx, &history, bytes, width);
                let full_elapsed = full_started.elapsed();
                compare(&history, &decoded, false);
                println!(
                    "{path} history={} match steps={} fallbacks={:?} decoded_records={} full_records={} binding_seconds={:.3} full_decode_seconds={:.3}",
                    history.id, stats.steps, stats.fallbacks, stats.decoded_records,
                    stats.full_records, stats.elapsed.as_secs_f64(), full_elapsed.as_secs_f64(),
                );
                snapshots.push(decoded);
                histories.push(history);
            }
        }
        assert!(!histories.is_empty(), "{path}: no histories");
        let states: usize = histories.iter().map(|h| h.states().len()).sum();
        let state_records: usize = snapshots
            .iter()
            .flatten()
            .map(|(_, _, versions)| versions.len())
            .sum();
        let before = transition_bytes(&histories);
        crate::history::discard_projection_caches(&mut histories);
        for (history, snapshots) in histories.iter().zip(&snapshots) {
            compare(history, snapshots, true);
        }
        let after = transition_bytes(&histories);
        println!("{path}\thistories={}\tstates={states}\tstate_records={state_records}\tcomplete_transitions_bytes={before}\tretained_transitions_bytes={after}", histories.len());
    }
}
