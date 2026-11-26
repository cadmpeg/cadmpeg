// SPDX-License-Identifier: Apache-2.0
//! The delta between two whole record sets of one id space.

use super::*;
use cadmpeg_core::decode::ResourceLimits;

use crate::design::refs::RecordRef;

fn named(key: u32, name: &str) -> Body {
    Body {
        name: Some(name.to_owned()),
        ..body(key)
    }
}

fn bodies(records: Vec<Body>) -> crate::document::Topology {
    crate::document::Topology {
        bodies: records,
        ..crate::document::Topology::default()
    }
}

#[test]
fn a_delta_holds_the_new_and_changed_records_and_removes_the_missing_ones() {
    let base = bodies(vec![body(1), body(2), body(3)]);
    let state = bodies(vec![body(1), named(2, "changed"), body(4)]);
    let delta = StateDelta::between(&base, &state);
    assert_eq!(delta.records.bodies, vec![named(2, "changed"), body(4)]);
    assert_eq!(delta.removed, vec![RecordRef::Body(id("body", 3))]);
    assert!(delta.names.is_empty() && delta.owners.is_empty() && delta.members.is_empty());
    assert_eq!(StateDelta::between(&state, &state), StateDelta::default());
}

#[test]
fn a_delta_over_its_base_resolves_to_the_state() {
    let base = bodies(vec![body(1), body(2), body(3)]);
    let state = bodies(vec![body(1), named(2, "changed"), body(4)]);
    let mut ir = CadIr::empty();
    ir.design.evaluations = vec![
        evaluation(
            1,
            Evaluated::State(after(1)),
            None,
            Some(StateContent {
                base: StateBase::Empty,
                delta: StateDelta {
                    records: base.clone(),
                    ..StateDelta::default()
                },
            }),
        ),
        evaluation(
            2,
            Evaluated::State(after(2)),
            None,
            Some(StateContent {
                base: StateBase::Evaluation(id("evaluation", 1)),
                delta: StateDelta::between(&base, &state),
            }),
        ),
    ];
    let resolved = ir
        .resolved_evaluation_state(&id("evaluation", 2), &ResourceLimits::default())
        .expect("the chain resolves");
    let mut held = resolved.records().bodies.clone();
    held.sort_by(|a, b| a.id.cmp(&b.id));
    assert_eq!(held, state.bodies);
}
