use super::*;

#[test]
fn an_item_whose_state_is_not_placed_has_no_evaluation_and_the_walk_goes_on() {
    let design = design(&["c", "a", "u", "b", "d"], "d", Vec::new());
    let stored = [Stored {
        name: "BREP.x",
        states: &[1, 2, 3, 4],
    }];
    let (entries, inferred) = walk(
        &design,
        vec![
            bound(0, 1, None),
            bound(0, 2, Some(1)),
            Kind::Unresolved("the file stores no state 9".to_owned()),
            bound(0, 3, Some(2)),
            bound(0, 4, Some(3)),
        ],
        &stored,
    );
    assert!(inferred.is_empty());
    let Plan { planned, stop, .. } = plan_of(&entries, &stored);
    assert_eq!(stop, None);
    assert_eq!(
        planned
            .iter()
            .map(|planned| (planned.entry, planned.targets.clone()))
            .collect::<Vec<_>>(),
        [(3, vec![Some(3)]), (1, vec![Some(2)]), (0, vec![Some(1)])]
    );
    let [loss] = unresolved(&design.scope.definition, &entries)
        .try_into()
        .expect("one loss");
    assert_eq!(loss.code, F3dLossCode::HistoryStateUnread.kind());
    assert!(
        loss.message.contains(&format!(
            "the state after feature {} is not stated: the file stores no state 9",
            feature_id("u")
        )) && !loss.message.contains(feature_id("a").as_str()),
        "{}",
        loss.message
    );
}

#[test]
fn an_unplaced_item_that_steps_a_history_keeps_the_states_before_it() {
    let design = design(&["c", "a", "u", "b", "d"], "d", Vec::new());
    let stored = [Stored {
        name: "BREP.x",
        states: &[1, 2, 4, 5],
    }];
    let (entries, inferred) = walk(
        &design,
        vec![
            bound(0, 1, None),
            bound(0, 2, Some(1)),
            Kind::Unresolved("the file stores no state 3".to_owned()),
            bound(0, 4, Some(3)),
            bound(0, 5, Some(4)),
        ],
        &stored,
    );
    assert!(inferred.is_empty());
    let Plan {
        planned,
        gaps,
        stop,
    } = plan_of(&entries, &stored);
    assert_eq!(stop, None);
    assert_eq!(
        planned
            .iter()
            .map(|planned| (planned.entry, planned.targets.clone()))
            .collect::<Vec<_>>(),
        [(3, vec![Some(4)]), (1, vec![Some(2)]), (0, vec![Some(1)])]
    );
    let [gap] = gaps.as_slice() else {
        panic!("one gap: {gaps:#?}");
    };
    assert!(gap.features.is_empty());
    assert!(stopped(&design.scope.definition, gap).is_none());
    let [loss] = unresolved(&design.scope.definition, &entries)
        .try_into()
        .expect("one loss");
    assert!(
        loss.message.contains(feature_id("u").as_str()),
        "{}",
        loss.message
    );
}

#[test]
fn an_item_whose_state_several_histories_store_lies_in_the_one_the_walk_stands_at_that_state_in() {
    let design = design(&["c", "a", "u", "b", "d"], "d", Vec::new());
    let stored = [
        Stored {
            name: "BREP.x",
            states: &[1, 3, 5],
        },
        Stored {
            name: "BREP.y",
            states: &[2, 3, 4],
        },
    ];
    let (entries, inferred) = walk(
        &design,
        vec![
            bound(0, 1, None),
            bound(1, 2, None),
            Kind::Ambiguous {
                state: 3,
                stored: vec![0, 1],
                previous: None,
            },
            bound(1, 4, Some(2)),
            bound(0, 5, Some(3)),
        ],
        &stored,
    );
    assert_eq!(entries[2].kind, bound(0, 3, Some(1)));
    let Plan { planned, stop, .. } = plan_of(&entries, &stored);
    assert_eq!(stop, None);
    assert_eq!(
        planned,
        [
            Planned {
                entry: 3,
                targets: vec![Some(3), Some(4)],
            },
            Planned {
                entry: 2,
                targets: vec![Some(3), Some(2)],
            },
            Planned {
                entry: 1,
                targets: vec![Some(1), Some(2)],
            },
            Planned {
                entry: 0,
                targets: vec![Some(1), None],
            },
        ]
    );
    let histories = histories(&stored);
    let [loss] = placed(&design.scope.definition, &entries, &histories, &inferred)
        .try_into()
        .expect("one loss");
    assert_eq!(loss.code, F3dLossCode::HistoryStateInferred.kind());
    assert!(
        loss.message.contains(&format!(
            "the state after feature {} is taken to be state 3 of BREP.x",
            feature_id("u")
        )) && loss.message.contains("leave BREP.x at that state"),
        "{}",
        loss.message
    );
    assert!(unresolved(&design.scope.definition, &entries).is_empty());
}

#[test]
fn an_item_whose_state_several_histories_stand_at_commits_no_state_and_the_walk_goes_on() {
    let design = design(&["c", "e", "u", "b", "d"], "d", Vec::new());
    let stored = [
        Stored {
            name: "BREP.x",
            states: &[3, 5],
        },
        Stored {
            name: "BREP.y",
            states: &[3, 4],
        },
    ];
    let (entries, inferred) = walk(
        &design,
        vec![
            bound(0, 3, None),
            bound(1, 3, None),
            Kind::Ambiguous {
                state: 3,
                stored: vec![0, 1],
                previous: None,
            },
            bound(1, 4, Some(3)),
            bound(0, 5, Some(3)),
        ],
        &stored,
    );
    assert!(inferred.is_empty());
    let Plan { planned, stop, .. } = plan_of(&entries, &stored);
    assert_eq!(stop, None);
    assert_eq!(
        planned,
        [
            Planned {
                entry: 3,
                targets: vec![Some(3), Some(4)],
            },
            Planned {
                entry: 1,
                targets: vec![Some(3), Some(3)],
            },
            Planned {
                entry: 0,
                targets: vec![Some(3), None],
            },
        ]
    );
    let [loss] = unresolved(&design.scope.definition, &entries)
        .try_into()
        .expect("one loss");
    assert!(
        loss.message.contains(feature_id("u").as_str())
            && loss
                .message
                .contains("leave none or several of them at that state")
            && loss.message.contains("taken to commit no history state"),
        "{}",
        loss.message
    );
}

#[test]
fn the_items_of_other_histories_between_two_items_whose_states_do_not_meet_have_no_evaluation() {
    let design = design(&["a", "b", "c", "d"], "d", Vec::new());
    let stored = [
        Stored {
            name: "BREP.x",
            states: &[1, 2, 3, 4],
        },
        Stored {
            name: "BREP.y",
            states: &[5],
        },
    ];
    let (entries, inferred) = walk(
        &design,
        vec![
            bound(0, 1, None),
            bound(1, 5, None),
            bound(0, 3, Some(2)),
            bound(0, 4, Some(3)),
        ],
        &stored,
    );
    assert!(inferred.is_empty());
    let Plan {
        planned,
        gaps,
        stop,
    } = plan_of(&entries, &stored);
    assert_eq!(stop, None);
    assert_eq!(
        planned,
        [
            Planned {
                entry: 2,
                targets: vec![Some(3), Some(5)],
            },
            Planned {
                entry: 0,
                targets: vec![Some(1), None],
            },
        ]
    );
    let [gap] = gaps.as_slice() else {
        panic!("one gap: {gaps:#?}");
    };
    assert_eq!(gap.features, [feature_id("b")]);
    let loss = stopped(&design.scope.definition, gap).expect("a loss");
    assert_eq!(loss.code, F3dLossCode::HistoryStateUnread.kind());
    assert!(
        loss.message.contains(&format!(
            "the states after features {} are not stated: no timeline item joins the states of BREP.x after state 1 of feature {} up to state 2, which feature {} stores as the state before it",
            feature_id("b"),
            feature_id("a"),
            feature_id("c")
        )),
        "{}",
        loss.message
    );
}

#[test]
fn a_later_item_whose_state_before_it_is_below_an_earlier_items_state_ends_the_states_there() {
    let design = design(&["a", "b", "c"], "c", Vec::new());
    let stored = [Stored {
        name: "BREP.x",
        states: &[1, 2, 3, 4],
    }];
    let (entries, inferred) = walk(
        &design,
        vec![
            bound(0, 3, Some(2)),
            bound(0, 2, Some(1)),
            bound(0, 4, Some(2)),
        ],
        &stored,
    );
    assert!(inferred.is_empty());
    let Plan {
        planned,
        gaps,
        stop,
    } = plan_of(&entries, &stored);
    assert!(gaps.is_empty());
    assert_eq!(
        planned,
        [Planned {
            entry: 1,
            targets: vec![Some(2)],
        }]
    );
    assert_eq!(stop.expect("a stop").features, [feature_id("a")]);
}

#[test]
fn two_gaps_of_two_histories_give_up_only_the_items_between_their_own_items() {
    let design = design(&["a", "group", "c", "d"], "d", Vec::new());
    let stored = [
        Stored {
            name: "BREP.x",
            states: &[1, 2, 3],
        },
        Stored {
            name: "BREP.y",
            states: &[1, 2, 3],
        },
    ];
    let (entries, _) = walk(
        &design,
        vec![
            bound(1, 1, None),
            bound(0, 1, None),
            bound(1, 3, Some(2)),
            bound(0, 3, Some(2)),
        ],
        &stored,
    );
    assert!(entries[1].stateless && entries[3].current && !entries[2].current);
    let Plan {
        planned,
        gaps,
        stop,
    } = plan_of(&entries, &stored);
    assert_eq!(stop, None);
    assert_eq!(
        planned,
        [Planned {
            entry: 0,
            targets: vec![None, Some(1)],
        }]
    );
    assert_eq!(
        gaps.iter()
            .map(|gap| gap.features.clone())
            .collect::<Vec<_>>(),
        [vec![feature_id("c")], Vec::new()]
    );
}

#[test]
fn the_state_after_an_item_before_a_gap_of_its_history_is_based_on_the_state_after_the_gap() {
    let design = design(&["a", "b", "c", "d"], "d", Vec::new());
    let x = |at| points(vec![point("p", at)]);
    let y = |at| points(vec![point("q", at)]);
    let outcome = run(
        &design,
        vec![
            bound(0, 1, None),
            bound(1, 5, None),
            bound(0, 3, Some(2)),
            bound(1, 6, Some(5)),
        ],
        &[
            Stored {
                name: "BREP.x",
                states: &[1, 2, 3],
            },
            Stored {
                name: "BREP.y",
                states: &[5, 6],
            },
        ],
        &[
            Ok(read(3, x(3.0), vec![(want(1, &[]), Ok(records(x(1.0))))])),
            Ok(read(6, y(6.0), vec![(want(5, &[]), Ok(records(y(5.0))))])),
        ],
        &[Consumed::default(), Consumed::default()],
        &no_fidelity(),
    );
    let [c, a] = outcome.evaluations.as_slice() else {
        panic!("two evaluations: {:#?}", outcome.evaluations);
    };
    assert_eq!([&c.of, &a.of], [&after("c"), &after("a")]);
    assert_eq!(content(c).1.records.points, [point("q", 5.0)]);
    assert_eq!(content(a).0, &StateBase::Evaluation(c.id.clone()));
    assert_eq!(content(a).1.records.points, [point("p", 1.0)]);
    assert_eq!(content(a).1.removed, [RecordRef::Point(point("q", 0.0).id)]);
    let [loss] = outcome.losses.as_slice() else {
        panic!("one loss: {:#?}", outcome.losses);
    };
    assert!(
        loss.message.contains(&format!(
            "the states after features {} are not stated",
            feature_id("b")
        )),
        "{}",
        loss.message
    );
}

#[test]
fn an_ambiguous_item_whose_state_before_it_is_below_its_historys_earlier_item_is_not_bound() {
    let design = design(&["a", "u"], "u", Vec::new());
    let stored = [
        Stored {
            name: "BREP.x",
            states: &[1, 2, 3],
        },
        Stored {
            name: "BREP.y",
            states: &[3, 4],
        },
    ];
    let (entries, inferred) = walk(
        &design,
        vec![
            bound(0, 2, Some(1)),
            Kind::Ambiguous {
                state: 3,
                stored: vec![0, 1],
                previous: Some(1),
            },
        ],
        &stored,
    );
    assert!(inferred.is_empty());
    assert!(matches!(entries[1].kind, Kind::Ambiguous { .. }));
    let Plan { planned, stop, .. } = plan_of(&entries, &stored);
    assert_eq!(stop, None);
    assert_eq!(
        planned,
        [Planned {
            entry: 0,
            targets: vec![Some(2), Some(4)],
        }]
    );
}

#[test]
fn an_evaluation_over_the_entity_limit_ends_the_states_there_and_keeps_the_later_ones() {
    let design = design(&["a", "b", "c"], "c", Vec::new());
    let x = |at| points(vec![point("p", at)]);
    let mut policy = DecodePolicy::default();
    policy.limits.max_entities = 3;
    let outcome = run_under(
        &policy,
        &design,
        vec![
            bound(0, 1, None),
            bound(0, 2, Some(1)),
            bound(0, 3, Some(2)),
        ],
        &[Stored {
            name: "BREP.x",
            states: &[1, 2, 3],
        }],
        &[Ok(read(
            3,
            x(3.0),
            vec![
                (want(2, &[]), Ok(records(x(2.0)))),
                (want(1, &[]), Ok(records(x(1.0)))),
            ],
        ))],
        &[Consumed::default()],
        &no_fidelity(),
    );
    assert!(!outcome.refused);
    let [b] = outcome.evaluations.as_slice() else {
        panic!("one evaluation: {:#?}", outcome.evaluations);
    };
    assert_eq!(b.of, after("b"));
    let [loss] = outcome.losses.as_slice() else {
        panic!("one loss: {:#?}", outcome.losses);
    };
    assert!(
        loss.message.contains(&format!(
            "the states after features {} are not stated: the evaluation does not fit the decode's resource limits left",
            feature_id("a")
        )),
        "{}",
        loss.message
    );
}

#[test]
fn a_stopped_chain_names_every_dropped_feature_in_order() {
    let design = design(&["c", "a", "b", "d"], "d", Vec::new());
    let outcome = run(
        &design,
        vec![
            bound(0, 1, None),
            bound(0, 2, Some(1)),
            bound(0, 3, Some(2)),
            bound(0, 4, Some(3)),
        ],
        &[Stored {
            name: "BREP.x",
            states: &[1, 2, 3, 4],
        }],
        &[Ok(read(
            4,
            points(vec![point("p", 4.0)]),
            vec![(
                want(3, &[]),
                Err("its record table is incomplete".to_owned()),
            )],
        ))],
        &[Consumed::default()],
        &no_fidelity(),
    );
    assert!(outcome.evaluations.is_empty());
    let [loss] = outcome.losses.as_slice() else {
        panic!("one loss: {:#?}", outcome.losses);
    };
    assert_eq!(loss.code, F3dLossCode::HistoryStateUnread.kind());
    assert!(
        loss.message.contains(&format!(
            "features {}, {}, {} are not stated",
            feature_id("a"),
            feature_id("b"),
            feature_id("c")
        )),
        "{}",
        loss.message
    );
}
