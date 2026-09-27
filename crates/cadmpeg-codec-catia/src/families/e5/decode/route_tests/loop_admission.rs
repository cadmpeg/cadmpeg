// SPDX-License-Identifier: Apache-2.0
use crate::families::e5::decode::E5LoopPlan;
use crate::families::e5::graph::{E5Loop, E5LoopMember, E5OrientedMember};

fn loop_record(indices: &[usize]) -> E5Loop {
    E5Loop {
        record_id: 7,
        surface: 10,
        members: vec![
            E5LoopMember {
                pcurve: 11,
                edge_use: 20,
                reversed: false,
            },
            E5LoopMember {
                pcurve: 12,
                edge_use: 21,
                reversed: true,
            },
        ],
        oriented_members: Some(
            indices
                .iter()
                .map(|&serialized_index| E5OrientedMember {
                    serialized_index,
                    reversed: true,
                })
                .collect(),
        ),
        outer: Some(true),
        orientation_hint: None,
    }
}

#[test]
fn loop_admission_retains_the_oriented_source_occurrences() {
    let source = loop_record(&[1, 0]);
    let plan = crate::test_support::with_service_context(|ctx| E5LoopPlan::admit(ctx, &source))
        .expect("service resource budget").expect("complete permutation");
    assert!(std::ptr::eq(plan.source, &raw const source));
    assert_eq!(plan.members.len(), 2);
    for (planned, index) in plan.members.iter().zip([1, 0]) {
        assert!(std::ptr::eq(
            planned.source,
            &raw const source.members[index]
        ));
        assert_eq!(planned.orientation.serialized_index, index);
        assert!(planned.orientation.reversed);
        assert_eq!(planned.id.as_str(), format!("catia:e5:coedge#7-{index}"));
    }
}

#[test]
fn loop_admission_refuses_incomplete_duplicate_and_out_of_range_orders() {
    for indices in [
        vec![],
        vec![0],
        vec![0, 0],
        vec![1, 1],
        vec![0, 2],
        vec![0, 1, 0],
    ] {
        let record = loop_record(&indices);
        assert!(
            crate::test_support::with_service_context(|ctx| E5LoopPlan::admit(ctx, &record))
                .expect("service resource budget").is_none(),
            "{indices:?}"
        );
    }
    let mut unresolved = loop_record(&[0, 1]);
    unresolved.oriented_members = None;
    assert!(crate::test_support::with_service_context(|ctx| E5LoopPlan::admit(ctx, &unresolved))
        .expect("service resource budget").is_none());
    let mut empty = loop_record(&[]);
    empty.members.clear();
    assert!(crate::test_support::with_service_context(|ctx| E5LoopPlan::admit(ctx, &empty))
        .expect("service resource budget").is_none());
}

#[test]
fn loop_admission_refuses_before_seen_set_and_member_plan_growth() {
    let source = loop_record(&[1, 0]);
    for (cap, operation) in [(0, "catia_e5_loop_plan_seen"), (1, "catia_e5_loop_plan_members")] {
        assert!(matches!(
            crate::test_support::with_collection_limit(cap, |ctx| E5LoopPlan::admit(ctx, &source)),
            Err(cadmpeg_core::CodecError::ResourceLimit(limit)) if limit.operation == operation
        ));
    }
    assert!(crate::test_support::with_service_context(|ctx| E5LoopPlan::admit(ctx, &source))
        .expect("service resource budget").is_some());
}
