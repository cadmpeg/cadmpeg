// SPDX-License-Identifier: Apache-2.0

use super::*;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_ir::attributes::AttributeTarget;

#[test]
fn inherited_attributes_cache_shared_and_cyclic_owner_paths() {
    let count = 4096;
    for cyclic in [false, true] {
        let records: Vec<_> = (1..=count)
            .map(|index| {
                let owner = if index == count {
                    usize::from(cyclic)
                } else {
                    index + 1
                };
                crate::test_support::sab::record(
                    index,
                    "custom-attrib".into(),
                    vec![
                        Token::Ref(-1),
                        Token::Ref(-1),
                        Token::Ref(-1),
                        Token::Ref(i64::try_from(owner).unwrap()),
                    ]
                    .into(),
                    0,
                    0,
                )
            })
            .collect();
        let by_index = records
            .iter()
            .map(|record| (i64::try_from(record.index).unwrap(), record))
            .collect();
        let expected = AttributeTarget::Edge(EdgeId::mint("test:model:edge#root").unwrap());
        let targets = HashMap::from([(0, expected.clone())]);
        let mut cache = HashMap::new();
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = 256 * u64::try_from(count).unwrap();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let mut storage = ctx.reserve_scoped(0, "test inherited targets").unwrap();
        for index in 1..=count {
            assert_eq!(
                inherited_attribute_target(
                    &ctx,
                    i64::try_from(index).unwrap(),
                    &by_index,
                    &targets,
                    &mut cache,
                    &mut storage
                )
                .unwrap(),
                (!cyclic).then(|| expected.clone())
            );
        }
        assert_eq!(cache.len(), count);
        drop(cache);
        drop(storage);
        ctx.finish_session().unwrap();
    }
}
