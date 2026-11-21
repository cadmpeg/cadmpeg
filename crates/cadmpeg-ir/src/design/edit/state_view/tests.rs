// SPDX-License-Identifier: Apache-2.0

use std::fmt::Debug;

use cadmpeg_core::text::NonBlankString;

use super::*;
use crate::design::evaluation::{
    CurrentState, Evaluation, EvaluationStatus, StateContent, StateDelta,
};
use crate::design::names::{NativeName, NativeNameValue};
use crate::design::refs::{Evaluated, RecordRef};
use crate::design::scope::DesignScope;
use crate::design::selection::{PathStep, StepRef};
use crate::products::{ProductDefinition, Storage};
use crate::topology::{Body, BodyKind, Region};

fn id<T>(kind: &str, key: u32) -> T
where
    T: TryFrom<String>,
    T::Error: Debug,
{
    T::try_from(format!("test:state-view:{kind}#{key}")).unwrap()
}

fn position(key: u32) -> Position {
    Position::Start(id("container", key))
}

fn path() -> OccurrencePath {
    OccurrencePath(vec![PathStep {
        occurrence: StepRef::Occurrence(id("occurrence", 1)),
        configuration: None,
    }])
}

fn name(text: &str) -> NameEntry {
    NameEntry {
        name: None,
        native: Some(NativeName {
            codec: NonBlankString::new("test").unwrap().into(),
            value: NativeNameValue::Text(NonBlankString::new(text).unwrap()),
        }),
    }
}

fn body(key: u32) -> Body {
    Body {
        id: id("body", key),
        kind: BodyKind::Solid,
        transform: None,
        name: None,
        color: None,
        visible: None,
        native_ref: None,
    }
}

fn evaluation(key: u32, base: StateBase) -> Evaluation {
    Evaluation {
        id: id("evaluation", key),
        scope: id("definition", 1),
        configuration: None,
        of: Evaluated::State(position(key + 1)),
        status: EvaluationStatus::Current,
        content: Some(StateContent {
            base,
            delta: StateDelta::default(),
        }),
        native_ref: None,
    }
}

fn current_document() -> CadIr {
    let mut ir = CadIr::empty();
    ir.geometry.bodies = vec![body(1), body(9)];
    ir.geometry.regions.push(Region {
        id: id("region", 1),
        body: id("body", 1),
        shells: Vec::new(),
        native_ref: None,
    });
    ir.design.names = BTreeMap::from([
        (EntityRef::Body(id("body", 1)), name("one")),
        (EntityRef::Body(id("body", 9)), name("nine")),
    ])
    .into();
    let mut owner = ProductDefinition::new(id("definition", 2), Storage::Embedded);
    owner.bodies = vec![id("body", 1)];
    ir.model.product_definitions.push(owner);
    let mut scope = ProductDefinition::new(id("definition", 1), Storage::Embedded);
    scope.occurrences = vec![id("occurrence", 1)];
    ir.model.product_definitions.push(scope);
    ir.design.scopes.push(DesignScope {
        definition: id("definition", 1),
        roots: Vec::new(),
        active_configuration: None,
        active_arrangements: BTreeSet::new(),
        occurrences: BTreeMap::new(),
        current: vec![CurrentState {
            at: position(1),
            bodies: vec![id("body", 1)],
            poses: BTreeMap::from([(path(), Transform::identity())]),
            status: EvaluationStatus::Current,
            native_ref: None,
        }],
    });
    ir
}

#[test]
fn current_state_selects_reached_records_names_owners_and_poses() {
    let ir = current_document();
    let before = ir.clone();
    let state = ir
        .resolved_current_state(&id("definition", 1), &position(1))
        .unwrap();
    assert_eq!(state.records().bodies, vec![body(1)]);
    assert_eq!(state.records().regions, ir.geometry.regions);
    assert_eq!(
        state.names(),
        &BTreeMap::from([(EntityRef::Body(id("body", 1)), name("one"))])
    );
    assert_eq!(
        state.owners(),
        &BTreeMap::from([(id("body", 1), id("definition", 2))])
    );
    assert_eq!(state.poses(), &ir.design.scopes[0].current[0].poses);
    assert_eq!(
        state.members(),
        &BTreeMap::from([(id("occurrence", 1), id("definition", 1))])
    );
    assert!(state.instance_bodies().is_empty());
    assert_eq!(ir, before);
}

#[test]
fn current_state_and_current_based_evaluation_include_instance_bodies() {
    let mut ir = current_document();
    ir.geometry.bodies.push(body(2));
    ir.model.product_definitions[1]
        .instance_bodies
        .push(crate::products::InstanceBodies {
            occurrence: path(),
            bodies: vec![id("body", 2)],
        });
    ir.design.scopes[0].current[0].bodies.push(id("body", 2));
    let current = ir
        .resolved_current_state(&id("definition", 1), &position(1))
        .unwrap();
    assert_eq!(
        current.instance_bodies(),
        &BTreeMap::from([(path(), vec![id("body", 2)])])
    );

    ir.design.evaluations = vec![evaluation(1, StateBase::Current(position(1)))];
    let evaluated = ir
        .resolved_evaluation_state(&id("evaluation", 1), &ResourceLimits::default())
        .unwrap();
    assert_eq!(
        evaluated.instance_bodies(),
        &BTreeMap::from([(path(), vec![id("body", 2)])])
    );
}

#[test]
fn evaluation_composes_current_base_and_all_delta_maps() {
    let mut ir = current_document();
    let mut first = evaluation(1, StateBase::Current(position(1)));
    let delta = &mut first.content.as_mut().unwrap().delta;
    delta.records.bodies = vec![body(2)];
    delta.removed = vec![RecordRef::Region(id("region", 1))];
    delta.names = BTreeMap::from([
        (EntityRef::Body(id("body", 1)), None),
        (EntityRef::Body(id("body", 2)), Some(name("two"))),
    ]);
    delta.owners = BTreeMap::from([
        (id("body", 1), None),
        (id("body", 2), Some(id("definition", 3))),
    ]);
    delta.members = BTreeMap::from([(id("occurrence", 1), Some(id("definition", 2)))]);
    delta.instance_bodies = BTreeMap::from([(path(), Some(vec![id("body", 2)]))]);
    let mut second = evaluation(2, StateBase::Evaluation(id("evaluation", 1)));
    let delta = &mut second.content.as_mut().unwrap().delta;
    delta.records.bodies = vec![Body {
        name: Some("replacement".into()),
        ..body(1)
    }];
    delta.members = BTreeMap::from([(id("occurrence", 1), Some(id("definition", 3)))]);
    delta.poses = BTreeMap::from([(path(), None)]);
    ir.design.evaluations = vec![second, first];
    let before = ir.clone();
    let state = ir
        .resolved_evaluation_state(&id("evaluation", 2), &ResourceLimits::default())
        .unwrap();
    assert_eq!(
        state.records().bodies,
        vec![
            Body {
                name: Some("replacement".into()),
                ..body(1)
            },
            body(2)
        ]
    );
    assert!(state.records().regions.is_empty());
    assert_eq!(
        state.names(),
        &BTreeMap::from([(EntityRef::Body(id("body", 2)), name("two"))])
    );
    assert_eq!(
        state.owners(),
        &BTreeMap::from([
            (id("body", 1), id("definition", 2)),
            (id("body", 2), id("definition", 3)),
        ])
    );
    assert_eq!(
        state.members(),
        &BTreeMap::from([(id("occurrence", 1), id("definition", 3))])
    );
    assert_eq!(
        state.instance_bodies(),
        &BTreeMap::from([(path(), vec![id("body", 2)])])
    );
    assert!(state.poses().is_empty());
    assert_eq!(ir, before);
}

#[test]
fn empty_base_and_map_removals_resolve() {
    let mut ir = CadIr::empty();
    let mut first = evaluation(1, StateBase::Empty);
    let delta = &mut first.content.as_mut().unwrap().delta;
    delta.records.bodies = vec![body(1)];
    delta.members = BTreeMap::from([(id("occurrence", 1), Some(id("definition", 1)))]);
    delta.instance_bodies = BTreeMap::from([(path(), Some(vec![id("body", 1)]))]);
    delta.poses = BTreeMap::from([(path(), Some(Transform::identity()))]);
    let mut second = evaluation(2, StateBase::Evaluation(id("evaluation", 1)));
    let delta = &mut second.content.as_mut().unwrap().delta;
    delta.removed = vec![RecordRef::Body(id("body", 1))];
    delta.members = BTreeMap::from([(id("occurrence", 1), None)]);
    delta.instance_bodies = BTreeMap::from([(path(), None)]);
    delta.poses = BTreeMap::from([(path(), None)]);
    ir.design.evaluations = vec![first, second];
    let state = ir
        .resolved_evaluation_state(&id("evaluation", 2), &ResourceLimits::default())
        .unwrap();
    assert_eq!(state.records(), &Topology::default());
    assert!(state.names().is_empty());
    assert!(state.owners().is_empty());
    assert!(state.members().is_empty());
    assert!(state.instance_bodies().is_empty());
    assert!(state.poses().is_empty());
}

#[test]
fn absent_current_state_and_current_base_are_refused() {
    let mut ir = current_document();
    for (scope, at) in [
        (id("definition", 9), position(1)),
        (id("definition", 1), position(9)),
    ] {
        assert_eq!(
            ir.resolved_current_state(&scope, &at),
            Err(StateResolutionError::MissingCurrentState { scope, at })
        );
    }
    ir.design.evaluations = vec![evaluation(1, StateBase::Current(position(9)))];
    assert_eq!(
        ir.resolved_evaluation_state(&id("evaluation", 1), &ResourceLimits::default()),
        Err(StateResolutionError::MissingCurrentState {
            scope: id("definition", 1),
            at: position(9),
        })
    );
}

#[test]
fn absent_evaluation_and_base_are_refused() {
    let mut ir = CadIr::empty();
    let missing = id("evaluation", 9);
    assert_eq!(
        ir.resolved_evaluation_state(&missing, &ResourceLimits::default()),
        Err(StateResolutionError::MissingEvaluation(missing.clone()))
    );
    ir.design.evaluations = vec![evaluation(1, StateBase::Evaluation(missing.clone()))];
    assert_eq!(
        ir.resolved_evaluation_state(&id("evaluation", 1), &ResourceLimits::default()),
        Err(StateResolutionError::MissingEvaluation(missing))
    );
}

#[test]
fn content_absent_from_the_evaluation_or_base_is_refused() {
    let mut ir = CadIr::empty();
    let mut unstored = evaluation(1, StateBase::Empty);
    unstored.content = None;
    ir.design.evaluations = vec![
        unstored,
        evaluation(2, StateBase::Evaluation(id("evaluation", 1))),
    ];
    for key in [1, 2] {
        assert_eq!(
            ir.resolved_evaluation_state(&id("evaluation", key), &ResourceLimits::default()),
            Err(StateResolutionError::MissingContent(id("evaluation", 1)))
        );
    }
}

#[test]
fn self_cycle_and_cycle_through_a_base_are_refused() {
    let mut ir = CadIr::empty();
    for base in [1, 2] {
        ir.design.evaluations = vec![
            evaluation(1, StateBase::Evaluation(id("evaluation", base))),
            evaluation(2, StateBase::Evaluation(id("evaluation", 1))),
        ];
        assert_eq!(
            ir.resolved_evaluation_state(&id("evaluation", 1), &ResourceLimits::default()),
            Err(StateResolutionError::Cycle(id("evaluation", 1)))
        );
    }
}

#[test]
fn chain_at_the_bound_resolves_and_one_past_is_refused() {
    let mut ir = CadIr::empty();
    ir.design.evaluations = vec![
        evaluation(1, StateBase::Empty),
        evaluation(2, StateBase::Evaluation(id("evaluation", 1))),
        evaluation(3, StateBase::Evaluation(id("evaluation", 2))),
    ];
    let limits = ResourceLimits {
        max_state_chain_depth: 2,
        ..ResourceLimits::default()
    };
    assert!(ir
        .resolved_evaluation_state(&id("evaluation", 2), &limits)
        .is_ok());
    assert_eq!(
        ir.resolved_evaluation_state(&id("evaluation", 3), &limits),
        Err(StateResolutionError::ChainDepthExceeded { limit: 2 })
    );
    let zero = ResourceLimits {
        max_state_chain_depth: 0,
        ..limits
    };
    assert_eq!(
        ir.resolved_evaluation_state(&id("evaluation", 1), &zero),
        Err(StateResolutionError::ChainDepthExceeded { limit: 0 })
    );
    ir.design.evaluations[0].content.as_mut().unwrap().base = StateBase::Current(position(1));
    assert_eq!(
        ir.resolved_evaluation_state(&id("evaluation", 3), &limits),
        Err(StateResolutionError::ChainDepthExceeded { limit: 2 })
    );
}

#[test]
fn current_state_resolves_scope_owned_bodies_and_listed_occurrences() {
    let mut ir = current_document();
    ir.model.product_definitions[0].bodies.clear();
    ir.model.product_definitions[1].bodies = vec![id("body", 1)];
    let state = ir
        .resolved_current_state(&id("definition", 1), &position(1))
        .unwrap();
    assert_eq!(
        state.owners(),
        &BTreeMap::from([(id("body", 1), id("definition", 1))])
    );
    assert_eq!(
        state.members(),
        &BTreeMap::from([(id("occurrence", 1), id("definition", 1))])
    );
}

#[test]
fn evaluation_fallback_uses_its_scope_and_preserves_explicit_entries() {
    let mut ir = current_document();
    let mut base = evaluation(1, StateBase::Empty);
    base.scope = id("definition", 9);
    let delta = &mut base.content.as_mut().unwrap().delta;
    delta.records.bodies = vec![body(1), body(2), body(3)];
    delta.owners = BTreeMap::from([(id("body", 3), Some(id("definition", 3)))]);
    delta.members = BTreeMap::from([(id("occurrence", 9), Some(id("definition", 3)))]);
    ir.design.evaluations = vec![
        base,
        evaluation(2, StateBase::Evaluation(id("evaluation", 1))),
    ];
    let state = ir
        .resolved_evaluation_state(&id("evaluation", 2), &ResourceLimits::default())
        .unwrap();
    assert_eq!(
        state.owners(),
        &BTreeMap::from([
            (id("body", 1), id("definition", 2)),
            (id("body", 2), id("definition", 1)),
            (id("body", 3), id("definition", 3)),
        ])
    );
    assert_eq!(
        state.members(),
        &BTreeMap::from([
            (id("occurrence", 1), id("definition", 1)),
            (id("occurrence", 9), id("definition", 3)),
        ])
    );
}
