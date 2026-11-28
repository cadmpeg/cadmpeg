// SPDX-License-Identifier: Apache-2.0

use super::*;

#[test]
fn rollback_marker_handles_unread_runs_and_contradictions() {
    let (marker, members, losses) = rollback_of(&[10, 20, 30], &[(10, true), (30, true)]);
    assert_eq!(marker, Rollback::Before(members[0].clone()));
    assert!(losses.is_empty());
    let (marker, _, losses) = rollback_of(&[10, 20, 30], &[(10, true), (30, false)]);
    assert_eq!(marker, Rollback::End);
    assert_eq!(
        losses,
        [crate::loss::F3dLossCode::HistoryRollbackUnread.kind()]
    );
    let (marker, members, losses) = rollback_of(&[10, 20, 30, 40, 50], &[(10, false), (50, true)]);
    assert_eq!(marker, Rollback::Before(members[4].clone()));
    assert_eq!(
        losses,
        [crate::loss::F3dLossCode::HistoryRollbackBoundUnread.kind()]
    );
}

#[test]
fn rollback_marker_resets_per_timeline_and_keeps_suppression() {
    let flags = [
        ((STREAM, 10), tail_flags([true, false], true, false)),
        ((STREAM, 20), tail_flags([false, true], true, true)),
        ((STREAM, 30), tail_flags([false; 2], false, false)),
    ]
    .into_iter()
    .collect();
    let mut losses = Vec::new();
    let mut documents = crate::test_support::with_decode_context(|ctx| {
        super::super::timeline::project(
            ctx,
            &[],
            &[timeline(STREAM, 0, &[10, 20]), timeline(STREAM, 1, &[30])],
            &[],
            &super::super::timeline::ScopeInputs {
                records: &[],
                tail_flags: &flags,
            },
            &BTreeSet::new(),
            &mut losses,
        )
    })
    .unwrap();
    let document = documents.remove("").unwrap();
    let Container::Ordered {
        members, rollback, ..
    } = &document.containers[0]
    else {
        panic!("ordered")
    };
    assert_eq!(*rollback, Some(Rollback::Before(members[1].clone())));
    let Container::Ordered { rollback, .. } = &document.containers[1] else {
        panic!("ordered")
    };
    assert_eq!(*rollback, Some(Rollback::End));
    assert_eq!(
        document.features[0].suppressed,
        Some(Dimension::Literal(true))
    );
    assert_eq!(
        document.features[1].suppressed,
        Some(Dimension::Literal(true))
    );
    assert!(losses.is_empty());
}
