// SPDX-License-Identifier: Apache-2.0
use super::*;

#[test]
fn an_unresolved_coordinate_frame_keeps_the_next_timeline_item() {
    let mut scope = crate::records::feature::scope::DesignParameterScope::empty(
        &format!("{STREAM}:design-parameter-scope#200"),
        crate::records::feature::scope::DesignFeatureKind::JointOrigin,
        20,
    );
    scope.with_joint_origin_transform(
        [
            [0.0, -1.0, 0.0, 1.0],
            [1.0, 0.0, 0.0, -2.0],
            [0.0, 0.0, 1.0, 3.0],
            [0.0, 0.0, 0.0, 1.0],
        ]
        .try_into()
        .unwrap(),
    );
    let frame = crate::design::feature_project::joint_origin_frame(&scope).unwrap();
    let features = [
        legacy(
            "joint-origin:10",
            0,
            None,
            LegacyOperation::Native {
                kind: "JointOrigin".into(),
                parameters: BTreeMap::default(),
            },
        ),
        legacy(
            "joint-origin:20",
            1,
            Some(false),
            LegacyOperation::DatumCoordinateSystem { frame },
        ),
    ];
    let mut losses = Vec::new();
    let documents = crate::test_support::with_decode_context(|ctx| {
        crate::design_v11::timeline::project(
            ctx,
            &features,
            &[timeline(STREAM, 0, &[10, 20])],
            &[],
            &crate::design_v11::timeline::ScopeInputs {
                records: &[],
                tail_flags: &crate::design_v11::timeline::TailFlags::new(),
            },
            &BTreeSet::new(),
            &mut losses,
        )
    })
    .unwrap();
    let projected = &documents[""];
    assert_eq!(projected.features.len(), 2);
    assert!(matches!(
        projected.features[0].definition.operation,
        FeatureOperation::Native { .. }
    ));
    let FeatureOperation::Datum(datum) = &projected.features[1].definition.operation else {
        panic!("datum frame")
    };
    let cadmpeg_ir::design::ops::datum::DatumElement::Frame(construction) = &datum.element else {
        panic!("coordinate frame")
    };
    let resolved = construction.resolved.as_ref().unwrap();
    assert_eq!(
        resolved.origin.get(),
        cadmpeg_ir::math::Point3::new(10.0, -20.0, 30.0)
    );
    assert_eq!(
        *resolved.x_axis.as_raw(),
        cadmpeg_ir::math::Vector3::new(0.0, 1.0, 0.0)
    );
    assert_eq!(
        *resolved.y_axis.as_raw(),
        cadmpeg_ir::math::Vector3::new(-1.0, 0.0, 0.0)
    );
    assert_eq!(
        *resolved.z_axis.as_raw(),
        cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0)
    );
    assert_eq!(projected.features[1].native_ref, features[1].native_ref);
    assert_eq!(
        projected.features[1].suppressed,
        Some(Dimension::Literal(false))
    );
    assert_eq!(
        losses
            .iter()
            .map(|loss| loss.code.local_code())
            .collect::<Vec<_>>(),
        [
            "feature.coordinate-system-frame-unresolved",
            "feature.coordinate-system-construction-dropped",
            "history.rollback-unread",
        ]
    );
}

#[test]
fn each_item_of_a_listing_timeline_is_one_feature_in_item_order() {
    let features = [
        legacy(
            "extrude:20",
            1,
            Some(true),
            LegacyOperation::Unresolved {
                family: UnresolvedFamily::Extrude,
            },
        ),
        legacy(
            "probe:10",
            0,
            Some(false),
            LegacyOperation::Native {
                kind: "Probe".into(),
                parameters: BTreeMap::default(),
            },
        ),
    ];
    let timelines = [timeline(STREAM, 1, &[]), timeline(STREAM, 0, &[10, 20, 30])];
    let types = [record_type(GROUND, vec![30])];
    let mut losses = Vec::new();
    let documents = crate::test_support::with_decode_context(|ctx| {
        crate::design_v11::timeline::project(
            ctx,
            &features,
            &timelines,
            &types,
            &crate::design_v11::timeline::ScopeInputs {
                records: &[],
                tail_flags: &crate::design_v11::timeline::TailFlags::new(),
            },
            &BTreeSet::new(),
            &mut losses,
        )
    })
    .unwrap();
    assert_eq!(documents.keys().collect::<Vec<_>>(), [""]);
    let projected = &documents[""];
    assert_eq!(projected.containers.len(), 1);
    let container = &projected.containers[0];
    let Container::Ordered {
        kind,
        folders,
        rollback,
        ..
    } = container
    else {
        panic!("ordered container")
    };
    assert_eq!(*kind, OrderedKind::Timeline);
    assert!(folders.is_empty());
    assert_eq!(*rollback, Some(Rollback::End));
    let [loss] = losses.as_slice() else {
        panic!("one loss: {losses:?}")
    };
    assert_eq!(
        loss.code,
        crate::loss::F3dLossCode::HistoryRollbackUnread.kind()
    );
    assert_eq!(loss.severity, cadmpeg_ir::report::Severity::Warning);
    assert!(
        loss.message.contains(container.id().as_str()),
        "{}",
        loss.message
    );
    let ids = projected
        .features
        .iter()
        .map(|feature| Member::Feature(feature.id.clone()))
        .collect::<Vec<_>>();
    assert_eq!(members(container), ids.as_slice());
    let [probe, extrude, ground] = projected.features.as_slice() else {
        panic!("three features")
    };
    assert_eq!(probe.id.as_str(), "f3d:model:feature#probe:10");
    assert_eq!(probe.suppressed, Some(Dimension::Literal(false)));
    assert!(matches!(
        &probe.definition.operation,
        FeatureOperation::Native { kind, .. } if kind.as_str() == "f3d:Probe"
    ));
    assert_eq!(extrude.suppressed, Some(Dimension::Literal(true)));
    assert_eq!(
        extrude.definition.operation,
        FeatureOperation::Unresolved {
            family: OperationFamily::Extrude
        }
    );
    assert_eq!(extrude.native_ref, features[0].native_ref);
    assert!(matches!(
        &ground.definition.operation,
        FeatureOperation::Native { kind, .. } if kind.as_str() == "f3d:Ground"
    ));
    assert_eq!(ground.suppressed, None);
    assert_eq!(
        ground.native_ref.as_deref(),
        Some(timelines[1].id().as_str())
    );
}

#[test]
fn a_stream_whose_timelines_list_no_item_states_no_container() {
    let mut losses = Vec::new();
    let documents = crate::test_support::with_decode_context(|ctx| {
        crate::design_v11::timeline::project(
            ctx,
            &[],
            &[timeline(STREAM, 0, &[])],
            &[],
            &crate::design_v11::timeline::ScopeInputs {
                records: &[],
                tail_flags: &crate::design_v11::timeline::TailFlags::new(),
            },
            &BTreeSet::new(),
            &mut losses,
        )
    })
    .unwrap();
    assert!(documents.is_empty());
    assert!(losses.is_empty(), "{losses:?}");
}

#[test]
fn an_item_of_an_unregistered_type_is_a_native_timeline_item() {
    let documents = crate::test_support::with_decode_context(|ctx| {
        crate::design_v11::timeline::project(
            ctx,
            &[],
            &[timeline(STREAM, 0, &[7])],
            &[],
            &crate::design_v11::timeline::ScopeInputs {
                records: &[],
                tail_flags: &crate::design_v11::timeline::TailFlags::new(),
            },
            &BTreeSet::new(),
            &mut Vec::new(),
        )
    })
    .unwrap();
    let [feature] = documents[""].features.as_slice() else {
        panic!("one feature")
    };
    assert!(matches!(
        &feature.definition.operation,
        FeatureOperation::Native { kind, .. } if kind.as_str() == "f3d:timeline-item"
    ));
}

/// Tail flags with state bytes 0 and 1 `suppression`, the no-result flag
/// `no_result` and the rolled-back flag `rolled_back`.
#[test]
fn the_rollback_marker_stands_before_the_first_of_the_trailing_rolled_back_items() {
    let (rollback, members, losses) = rollback_of(
        &[10, 20, 30, 40],
        &[(10, false), (20, false), (30, true), (40, true)],
    );
    assert_eq!(rollback, Rollback::Before(members[2].clone()));
    assert!(losses.is_empty(), "{losses:?}");
    let (rollback, members, losses) = rollback_of(&[10, 20], &[(10, true), (20, true)]);
    assert_eq!(rollback, Rollback::Before(members[0].clone()));
    assert!(losses.is_empty(), "{losses:?}");
}

#[test]
fn an_unread_item_before_the_rolled_back_items_charges_the_marker_bound_loss() {
    let (rollback, members, losses) = rollback_of(&[10, 20, 30], &[(10, false), (30, true)]);
    assert_eq!(rollback, Rollback::Before(members[2].clone()));
    assert_eq!(
        losses,
        [crate::loss::F3dLossCode::HistoryRollbackBoundUnread.kind()]
    );
}

#[test]
fn an_unread_item_after_the_first_rolled_back_item_charges_the_rollback_loss() {
    let (rollback, members, losses) = rollback_of(&[10, 20], &[(10, true)]);
    assert_eq!(rollback, Rollback::Before(members[0].clone()));
    assert_eq!(
        losses,
        [crate::loss::F3dLossCode::HistoryRollbackUnread.kind()]
    );
}

#[test]
fn a_timeline_whose_items_are_all_read_and_built_ends_after_its_last_item() {
    let (rollback, _, losses) = rollback_of(&[10, 20], &[(10, false), (20, false)]);
    assert_eq!(rollback, Rollback::End);
    assert!(losses.is_empty(), "{losses:?}");
}

#[test]
fn a_built_item_after_a_rolled_back_item_leaves_the_marker_unread() {
    let (rollback, _, losses) = rollback_of(&[10, 20], &[(10, true), (20, false)]);
    assert_eq!(rollback, Rollback::End);
    assert_eq!(
        losses,
        [crate::loss::F3dLossCode::HistoryRollbackUnread.kind()]
    );
}

#[test]
fn the_current_state_stands_at_the_rollback_marker() {
    let id =
        cadmpeg_ir::design::ids::ContainerId::mint("f3d:design:timeline#0".to_owned()).unwrap();
    let members = [10, 20, 30]
        .map(|key| Member::Feature(FeatureId::mint(format!("f3d:model:feature#{key}")).unwrap()));
    assert_eq!(
        crate::design_v11::rollback_position(&id, &members, Some(&Rollback::End)),
        Position::After(members[2].clone())
    );
    assert_eq!(
        crate::design_v11::rollback_position(
            &id,
            &members,
            Some(&Rollback::Before(members[1].clone()))
        ),
        Position::After(members[0].clone())
    );
    assert_eq!(
        crate::design_v11::rollback_position(
            &id,
            &members,
            Some(&Rollback::Before(members[0].clone()))
        ),
        Position::Start(id.clone())
    );
}

/// An unprojected item is suppressed where state byte 0 or 1 of its scope
/// tail is set. With the no-result flag and neither byte set, it is active
/// with no current result and charges a loss.
#[test]
fn an_unprojected_item_takes_its_suppression_from_its_scope_tail_state_bytes() {
    for (suppression, no_result, suppressed, assumed) in [
        ([true, false], true, true, false),
        ([false, true], true, true, false),
        ([false, false], true, false, true),
        ([false, false], false, false, false),
    ] {
        let flags: crate::design_v11::timeline::TailFlags<'_> =
            [((STREAM, 7), tail_flags(suppression, no_result, false))]
                .into_iter()
                .collect();
        let mut losses = Vec::new();
        let documents = crate::test_support::with_decode_context(|ctx| {
            crate::design_v11::timeline::project(
                ctx,
                &[],
                &[timeline(STREAM, 0, &[7, 8])],
                &[],
                &crate::design_v11::timeline::ScopeInputs {
                    records: &[],
                    tail_flags: &flags,
                },
                &BTreeSet::new(),
                &mut losses,
            )
        })
        .unwrap();
        let [flagged, unflagged] = documents[""].features.as_slice() else {
            panic!("two features")
        };
        let case = format!("{suppression:?} {no_result}");
        assert_eq!(
            flagged.suppressed,
            Some(Dimension::Literal(suppressed)),
            "{case}"
        );
        assert_eq!(unflagged.suppressed, None, "{case}");
        assert_eq!(
            losses.iter().any(|loss| {
                loss.code == crate::loss::F3dLossCode::FeatureNoResultActiveAssumed.kind()
            }),
            assumed,
            "{case}"
        );
    }
}

/// A rolled-back item that holds no state is active with either no-result
/// flag (f3d.md §3.1, "In every form, the state bytes"), and charges no
/// assumption.
#[test]
fn a_rolled_back_item_with_either_no_result_flag_is_active_without_a_loss() {
    for no_result in [false, true] {
        let flags: crate::design_v11::timeline::TailFlags<'_> =
            [((STREAM, 8), tail_flags([false; 2], no_result, true))]
                .into_iter()
                .collect();
        let mut losses = Vec::new();
        let documents = crate::test_support::with_decode_context(|ctx| {
            crate::design_v11::timeline::project(
                ctx,
                &[],
                &[timeline(STREAM, 0, &[7, 8])],
                &[],
                &crate::design_v11::timeline::ScopeInputs {
                    records: &[],
                    tail_flags: &flags,
                },
                &BTreeSet::new(),
                &mut losses,
            )
        })
        .unwrap();
        let [_, rolled] = documents[""].features.as_slice() else {
            panic!("two features")
        };
        assert_eq!(
            rolled.suppressed,
            Some(Dimension::Literal(false)),
            "no-result {no_result}"
        );
        assert!(
            losses.iter().all(|loss| {
                loss.code != crate::loss::F3dLossCode::FeatureNoResultActiveAssumed.kind()
            }),
            "no-result {no_result}: {losses:#?}"
        );
    }
}

/// An unprojected Thread item whose scope stores the companion tail takes
/// its suppression from the companion suppression flag.
#[test]
fn an_unprojected_item_takes_its_suppression_from_its_thread_companion_flag() {
    for suppressed in [false, true] {
        let mut scope = crate::records::feature::scope::DesignParameterScope::empty(
            &format!("{STREAM}:design-parameter-scope#100"),
            crate::records::feature::scope::DesignFeatureKind::Thread,
            7,
        );
        scope.companion_suppressed = Some(Located {
            value: suppressed,
            offset: scope.feature_ordinal_offset() + 62,
        });
        let mut losses = Vec::new();
        let documents = crate::test_support::with_decode_context(|ctx| {
            crate::design_v11::timeline::project(
                ctx,
                &[],
                &[timeline(STREAM, 0, &[7, 8])],
                &[],
                &crate::design_v11::timeline::ScopeInputs {
                    records: std::slice::from_ref(&scope),
                    tail_flags: &crate::design_v11::timeline::TailFlags::new(),
                },
                &BTreeSet::new(),
                &mut losses,
            )
        })
        .unwrap();
        let [thread, other] = documents[""].features.as_slice() else {
            panic!("two features")
        };
        assert_eq!(
            thread.suppressed,
            Some(Dimension::Literal(suppressed)),
            "suppressed {suppressed}"
        );
        assert_eq!(other.suppressed, None, "suppressed {suppressed}");
    }
}
