// SPDX-License-Identifier: Apache-2.0
//! Compare chain reconstruction with complete decode snapshots.

use crate::history_records::AsmHistory;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

fn compare(histories: &[AsmHistory]) {
    for history in histories {
        for state in &history.states {
            assert_eq!(
                history.topology(state),
                state.topology(),
                "{} state {}",
                history.id,
                state.state_id
            );
            assert_eq!(
                history.entity_versions(state),
                state.entity_versions,
                "{} state {}",
                history.id,
                state.state_id
            );
            assert_eq!(
                history.check_transitions(),
                Ok(()),
                "{} state {}",
                history.id,
                state.state_id
            );
        }
    }
}

fn sizes(histories: &[AsmHistory]) -> (usize, usize) {
    let transitions: Vec<_> = histories
        .iter()
        .flat_map(|h| h.states.iter().map(|s| &s.transition))
        .collect();
    let snapshots: Vec<_> = histories
        .iter()
        .flat_map(|h| h.states.iter().map(|s| (s.topology(), &s.entity_versions)))
        .collect();
    (
        serde_json::to_vec(&transitions).unwrap().len(),
        serde_json::to_vec(&snapshots).unwrap().len(),
    )
}

#[test]
#[ignore = "requires CADMPEG_F3D_HISTORY_ORACLE design paths"]
fn history_deltas_match_snapshots() {
    let Ok(paths) = std::env::var("CADMPEG_F3D_HISTORY_ORACLE") else {
        return;
    };
    for path in paths.split(':') {
        let bytes = std::fs::read(path).unwrap();
        let arena = DecodeArena::new();
        let policy = DecodePolicy::desktop();
        let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).unwrap();
        let scan = crate::container::scan(&ctx, root).unwrap();
        let mut allowance = crate::history::history_binding_allowance(&policy);
        let mut histories = Vec::new();
        for brep in crate::container::history_breps(&scan) {
            let width = brep
                .kernel
                .as_ref()
                .and_then(crate::container::KernelFraming::asm_header)
                .map_or(cadmpeg_asm::kernel_header::RefWidth::Eight, |header| {
                    header.width
                });
            let bytes = scan.entry_bytes(&brep.name).unwrap();
            if let Some(history) =
                crate::history::decode(&ctx, bytes, &brep.name, width, &mut allowance).unwrap()
            {
                assert!(
                    history
                        .states
                        .iter()
                        .all(|state| state.topology().is_some()),
                    "{path}: unbound history {}",
                    history.id
                );
                histories.push(history);
            }
        }
        assert!(!histories.is_empty(), "{path}: no histories");
        let states: usize = histories.iter().map(|history| history.states.len()).sum();
        let entries: usize = histories
            .iter()
            .flat_map(|history| &history.states)
            .map(|state| {
                let transition = state.transition.as_ref().unwrap();
                transition.records.inserted.len()
            })
            .sum();
        // Count complete state-record maps independently of retained topology maps.
        let mut state_records = 0;
        for history in &histories {
            for state in &history.states {
                let mut current = Some(state);
                let mut count: i64 = 0;
                while let Some(node) = current {
                    let records = &node.transition.as_ref().unwrap().records;
                    count += i64::try_from(records.inserted.len()).unwrap()
                        - i64::try_from(records.deleted.len()).unwrap();
                    current = node.next_ref.map(|next| {
                        history
                            .states
                            .iter()
                            .find(|state| state.node_index == next)
                            .unwrap()
                    });
                }
                state_records += usize::try_from(count).unwrap();
            }
        }
        assert!(entries > 0);
        compare(&histories);
        let before = sizes(&histories);
        crate::history::discard_projection_caches(&mut histories);
        compare(&histories);
        let after = sizes(&histories);
        println!("{path}\thistories={}\tstates={states}\tstate_records={state_records}\tcomplete_transitions_bytes={}\tcomplete_snapshots_bytes={}\tretained_transitions_bytes={}\tretained_snapshots_bytes={}", histories.len(), before.0, before.1, after.0, after.1);
    }
}
