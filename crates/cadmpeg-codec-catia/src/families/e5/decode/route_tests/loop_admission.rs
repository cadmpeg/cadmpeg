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
    let plan = E5LoopPlan::admit(&source).expect("complete permutation");
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
        assert!(
            E5LoopPlan::admit(&loop_record(&indices)).is_none(),
            "{indices:?}"
        );
    }
    let mut unresolved = loop_record(&[0, 1]);
    unresolved.oriented_members = None;
    assert!(E5LoopPlan::admit(&unresolved).is_none());
    let mut empty = loop_record(&[]);
    empty.members.clear();
    assert!(E5LoopPlan::admit(&empty).is_none());
}
