// SPDX-License-Identifier: Apache-2.0
//! Shared bound parameter indexing.

use super::*;

#[test]
fn e5_bound_parameter_queries_do_not_replay_shared_entries() {
    let count = 4_096_u32;
    let bounds = BTreeMap::from([(
        1,
        E5Bounds {
            entries: (0..count)
                .map(|representation| E5BoundEntry {
                    representation,
                    parameter: FiniteReal::new(f64::from(representation))
                        .expect("finite parameter"),
                    code: representation,
                })
                .collect(),
        },
    )]);
    crate::test_support::with_service_context(|ctx| {
        let (parameters, _storage) = index_bound_parameters(ctx, &bounds).expect("bound index");
        crate::test_support::with_work_limit(u64::from(count) * 64, |query_ctx| {
            for representation in 0..count {
                assert_eq!(
                    bound_representation_parameter(query_ctx, &parameters, 1, representation)?
                        .map(FiniteReal::get),
                    Some(f64::from(representation))
                );
            }
            Ok::<_, CodecError>(())
        })
        .expect("one indexed query per representation");
    });
}

#[test]
fn e5_bound_parameter_index_keeps_duplicate_ambiguity() {
    let entry = E5BoundEntry {
        representation: 7,
        parameter: FiniteReal::new(0.25).expect("finite"),
        code: 1,
    };
    let bounds = BTreeMap::from([(
        1,
        E5Bounds {
            entries: vec![entry, entry],
        },
    )]);
    crate::test_support::with_retained_limit(0, |ctx| {
        let (parameters, _storage) =
            index_bound_parameters(ctx, &bounds).expect("scoped bound index");
        assert_eq!(
            bound_representation_parameter(ctx, &parameters, 1, 7).expect("parameter query"),
            None
        );
    });
}
