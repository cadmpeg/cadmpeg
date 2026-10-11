// SPDX-License-Identifier: Apache-2.0
//! Shallow drawing joins share their descendant graph.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;

use cadmpeg_core::decode::DecodePolicy;

#[test]
fn shallow_wrapper_joins_do_not_materialize_each_parent_closure() {
    for (parents, leaves) in [(1, 64), (128, 1), (128, 64)] {
        let mut source = String::from("ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=MAPPED_ITEM('',#2,$);#2=REPRESENTATION_MAP($,#3);#3=REPRESENTATION('',(");
        for index in 0..parents {
            if index != 0 {
                source.push(',');
            }
            write!(source, "#{}", 1000 + index * 3).expect("parent reference");
        }
        source.push_str(
            "),$);#4=MAPPED_ITEM('',#5,$);#5=REPRESENTATION_MAP($,#6);#6=REPRESENTATION('',(",
        );
        for index in 0..leaves {
            if index != 0 {
                source.push(',');
            }
            write!(source, "#{}", 100 + index).expect("leaf reference");
        }
        source.push_str("),$);#7=ITEM();");
        for index in 0..parents {
            let id = 1000 + index * 3;
            write!(source, "#{id}=MAPPED_ITEM('',#{},$);#{}=REPRESENTATION_MAP($,#{});#{}=REPRESENTATION('',(#4,#7),$);", id + 1, id + 1, id + 2, id + 2).expect("parent");
        }
        let identities: BTreeMap<_, _> = (0..leaves)
            .map(|index| {
                let id = 100 + index;
                write!(source, "#{id}=ITEM();").expect("leaf");
                (id, BTreeSet::from([format!("target-{index:03}")]))
            })
            .collect();
        source.push_str("ENDSEC;END-ISO-10303-21;");
        let (exchange, _) =
            crate::test_support::with_service_context(source.as_bytes(), crate::parse::parse_inner)
                .expect("join exchange");
        let mut policy = DecodePolicy::service();
        // 128 parents of 64 leaves would need 8,192 copied closure entries.
        // Shared nodes, edges and one result set fit below this cap.
        policy.limits.max_collection_items = 4096;
        policy.limits.max_retained_bytes = 32;
        crate::test_support::with_policy_context(b"", &policy, |_, ctx| {
            let cache = super::super::WrapperCache::new(ctx).expect("cache");
            let resolved = cache
                .resolve(1, &identities, &exchange)
                .expect("linear join resolution")
                .expect("nonempty targets");
            if leaves == 1 {
                let super::super::WrapperTargetResolution::Singleton(target) = resolved else {
                    panic!("singleton")
                };
                assert_eq!(target, "target-000");
            } else {
                let super::super::WrapperTargetResolution::Ambiguous((targets, _storage)) =
                    resolved
                else {
                    panic!("ambiguity")
                };
                assert_eq!(
                    targets,
                    identities
                        .values()
                        .flat_map(|values| values.iter().cloned())
                        .collect()
                );
            }
            assert_eq!(ctx.resource_refusal(), None);
        });
    }
}

#[test]
fn wrapper_joins_compare_shared_long_identity_classes_once() {
    for (parents, name_len) in [(1, 8), (128, 8), (1, 16384), (128, 16384)] {
        let name = "A".repeat(name_len);
        let mut source = String::from("ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=MAPPED_ITEM('',#2,$);#2=REPRESENTATION_MAP($,#3);#3=REPRESENTATION('',(");
        for index in 0..parents {
            if index != 0 {
                source.push(',');
            }
            write!(source, "#{}", 1000 + index * 3).expect("parent reference");
        }
        source.push_str("),$);#100=ITEM();#101=ITEM();");
        for index in 0..parents {
            let id = 1000 + index * 3;
            write!(source, "#{id}=MAPPED_ITEM('',#{},$);#{}=REPRESENTATION_MAP($,#{});#{}=REPRESENTATION('',(#100,#101),$);", id + 1, id + 1, id + 2, id + 2).expect("parent");
        }
        source.push_str("ENDSEC;END-ISO-10303-21;");
        let identities = BTreeMap::from([
            (100, BTreeSet::from([name.clone()])),
            (101, BTreeSet::from([name.clone()])),
        ]);
        let (exchange, _) =
            crate::test_support::with_service_context(source.as_bytes(), crate::parse::parse_inner)
                .expect("join exchange");
        let mut policy = DecodePolicy::service();
        // Repeating a 16-KiB comparison for 128 parents needs over four
        // million work units. Interning the two source leaves fits this cap.
        policy.limits.max_work_units = 2_000_000;
        policy.limits.max_collection_items = 4096;
        policy.limits.max_retained_bytes = u64::try_from(name_len).expect("name length");
        crate::test_support::with_policy_context(b"", &policy, |_, ctx| {
            let cache = super::super::WrapperCache::new(ctx).expect("cache");
            let Some(super::super::WrapperTargetResolution::Singleton(target)) = cache
                .resolve(1, &identities, &exchange)
                .expect("shared identity class")
            else {
                panic!("same identity is a singleton")
            };
            assert_eq!(target, name);
            assert_eq!(ctx.resource_refusal(), None);
        });
    }
}
