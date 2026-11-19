// SPDX-License-Identifier: Apache-2.0
//! Edits of an ordered container whose rollback is unstated, and removal of
//! a container member (§4, §5.3).

use std::collections::BTreeMap;

use super::{after, feature, id, owning, status, timeline, unresolved, with_bodies};
use crate::design::container::{Container, OrderedKind, Rollback};
use crate::design::evaluation::{CurrentState, EvaluationStatus};
use crate::design::refs::{Evaluated, Member, Position};
use crate::CadIr;

/// Leaves the timeline's rollback unstated, with its current state at
/// `After(Container(1))`, and every evaluation current with content.
fn unstated(document: &mut CadIr) {
    let Container::Ordered { rollback, .. } = &mut document.design.containers[0] else {
        panic!("ordered");
    };
    *rollback = None;
    document.design.scopes[0].current[0].at =
        Position::After(Member::Container(id("container", 1)));
    for evaluation in &mut document.design.evaluations {
        evaluation.status = EvaluationStatus::Current;
        evaluation.content.get_or_insert_with(|| with_bodies(&[]));
    }
}

fn timeline_rollback(document: &CadIr) -> Option<Rollback> {
    let Container::Ordered { rollback, .. } = &document.design.containers[0] else {
        panic!("ordered");
    };
    rollback.clone()
}

#[test]
fn the_current_state_at_an_unstated_rollback_is_after_every_member() {
    let mut document = timeline();
    unstated(&mut document);
    document
        .edit()
        .unwrap()
        .insert_member(&id("container", 1), 3, owning(4, 40))
        .unwrap();
    assert_eq!(
        timeline_rollback(&document),
        None,
        "an unstated rollback stays unstated"
    );
    assert_eq!(
        document.design.scopes[0].current[0].status,
        EvaluationStatus::Stale
    );
    assert_eq!(status(&document, 2), EvaluationStatus::Current);

    let mut document = timeline();
    unstated(&mut document);
    document
        .edit()
        .unwrap()
        .remove_member(&Member::Feature(feature(3)))
        .unwrap();
    assert_eq!(timeline_rollback(&document), None);
    let [current] = document.design.scopes[0].current.as_slice() else {
        panic!("one current state");
    };
    assert_eq!(
        current.at,
        Position::After(Member::Container(id("container", 1)))
    );
    assert_eq!(current.status, EvaluationStatus::Stale);
    assert_eq!(status(&document, 2), EvaluationStatus::Current);
}

fn current_at(at: Position) -> CurrentState {
    CurrentState {
        at,
        bodies: Vec::new(),
        poses: BTreeMap::new(),
        status: EvaluationStatus::Current,
        native_ref: None,
    }
}

#[test]
fn removing_a_container_deletes_the_current_states_at_its_unstated_rollbacks() {
    let mut document = timeline();
    let Container::Ordered { members, .. } = &mut document.design.containers[0] else {
        panic!("ordered");
    };
    members.push(Member::Container(id("container", 2)));
    document.design.containers.extend([
        Container::Ordered {
            id: id("container", 2),
            kind: OrderedKind::Body,
            members: vec![
                Member::Feature(feature(4)),
                Member::Container(id("container", 3)),
            ],
            folders: Vec::new(),
            rollback: None,
            placement: None,
        },
        Container::Ordered {
            id: id("container", 3),
            kind: OrderedKind::GeometricalSet,
            members: vec![Member::Feature(feature(5))],
            folders: Vec::new(),
            rollback: None,
            placement: None,
        },
    ]);
    document
        .design
        .features
        .extend([unresolved(4), unresolved(5)]);
    document.design.scopes[0].current.extend([
        current_at(Position::After(Member::Container(id("container", 2)))),
        current_at(Position::After(Member::Container(id("container", 3)))),
    ]);
    document.design.finalize();
    document
        .edit()
        .unwrap()
        .remove_member(&Member::Container(id("container", 2)))
        .unwrap();
    let positions: Vec<_> = document.design.scopes[0]
        .current
        .iter()
        .map(|current| current.at.clone())
        .collect();
    assert_eq!(positions, [after(3)]);
    assert_eq!(document.design.containers.len(), 1);
}

#[test]
fn removal_retargets_to_the_feature_before_a_container_member() {
    let mut document = timeline();
    let Container::Ordered { members, .. } = &mut document.design.containers[0] else {
        panic!("ordered");
    };
    members.insert(2, Member::Container(id("container", 2)));
    document.design.containers.push(Container::Ordered {
        id: id("container", 2),
        kind: OrderedKind::Body,
        members: vec![Member::Feature(feature(4))],
        folders: Vec::new(),
        rollback: Some(Rollback::End),
        placement: None,
    });
    document.design.features.push(unresolved(4));
    document
        .design
        .evaluations
        .retain(|evaluation| evaluation.of != Evaluated::State(after(2)));
    document.design.finalize();
    document
        .edit()
        .unwrap()
        .remove_member(&Member::Feature(feature(3)))
        .unwrap();
    assert_eq!(document.design.scopes[0].current[0].at, after(2));
}
