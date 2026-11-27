// SPDX-License-Identifier: Apache-2.0
//! A state held in full is stated as a delta over the state after it, and a
//! chain-back chain bases each state on the one stated before it (§5.4).

use std::collections::BTreeMap;

use super::{ChainBack, HeldState, StateBase, StateContent, StateDelta};
use crate::design::ids::{ContainerId, EvaluationId};
use crate::design::names::{NameEntry, NativeName, NativeNameValue};
use crate::design::refs::{EntityRef, Position, RecordRef};
use crate::document::Topology;
use crate::features::geometry::FinitePoint3;
use crate::ids::FaceId;
use crate::math::Point3;

fn cube() -> Topology {
    crate::examples::unit_cube().unwrap().geometry
}

fn held(records: Topology) -> HeldState {
    HeldState {
        records,
        names: BTreeMap::new(),
    }
}

fn entry(value: i64) -> NameEntry {
    NameEntry {
        name: None,
        native: Some(NativeName {
            codec: crate::codec_id!("test"),
            value: NativeNameValue::Integers(vec![value]),
        }),
    }
}

fn face(key: &str) -> EntityRef {
    EntityRef::Face(FaceId::try_from(format!("test:design:face#{key}")).unwrap())
}

fn evaluation(key: u32) -> EvaluationId {
    EvaluationId::try_from(format!("test:design:evaluation#{key}")).unwrap()
}

/// `base` with its first point moved and its last vertex dropped.
fn edited(base: &Topology) -> Topology {
    let mut state = base.clone();
    state.points[0].set_position(FinitePoint3::new(Point3::new(-1.0, -2.0, -3.0)).unwrap());
    state.vertices.pop();
    state
}

#[test]
fn between_equal_states_is_empty() {
    let state = held(cube());
    assert_eq!(StateDelta::between(&state, &state), StateDelta::default());
}

#[test]
fn between_holds_changed_records_and_removes_absent_ones() {
    let base = cube();
    let state = edited(&base);
    let delta = StateDelta::between(&held(state.clone()), &held(base.clone()));
    let mut expected = Topology::default();
    expected.points.push(state.points[0].clone());
    assert_eq!(delta.records, expected);
    let dropped = base.vertices.last().unwrap().id.clone();
    assert_eq!(delta.removed, vec![RecordRef::Vertex(dropped)]);
}

#[test]
fn between_sets_differing_names_and_clears_absent_ones() {
    let base = HeldState {
        records: Topology::default(),
        names: BTreeMap::from([(face("a"), entry(1)), (face("b"), entry(2))]),
    };
    let state = HeldState {
        records: Topology::default(),
        names: BTreeMap::from([(face("a"), entry(1)), (face("c"), entry(3))]),
    };
    assert_eq!(
        StateDelta::between(&state, &base).names,
        BTreeMap::from([(face("b"), None), (face("c"), Some(entry(3)))])
    );
}

#[test]
fn chain_back_bases_each_state_on_the_one_stated_before_it() {
    let end = cube();
    let later = edited(&end);
    let mut earlier = later.clone();
    earlier.points.remove(1);
    let at = Position::Start(ContainerId::try_from("test:design:container#1".to_owned()).unwrap());
    let mut chain = ChainBack::new(StateBase::Current(at.clone()), held(end.clone()));
    assert_eq!(
        chain.push(evaluation(1), held(later.clone())),
        StateContent {
            base: StateBase::Current(at),
            delta: StateDelta::between(&held(later.clone()), &held(end)),
        }
    );
    let second = chain.push(evaluation(2), held(earlier));
    assert_eq!(second.base, StateBase::Evaluation(evaluation(1)));
    assert_eq!(second.delta.records, Topology::default());
    assert_eq!(
        second.delta.removed,
        vec![RecordRef::Point(later.points[1].id.clone())]
    );
}
