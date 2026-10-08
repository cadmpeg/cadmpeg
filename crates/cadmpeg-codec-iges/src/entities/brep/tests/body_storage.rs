// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::decode::{u64_from_index, DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use cadmpeg_ir::topology::{BodyKind, Sense};

#[test]
fn body_shell_storage_follows_each_body_instead_of_the_definition_tables() {
    let mut sheet = crate::test_support::directory_target(1, 514);
    sheet.form = 2;
    let solid = crate::test_support::directory_target(3, 186);
    let directory = [sheet, solid];
    let shell_bytes = u64_from_index(std::mem::size_of::<(u32, Sense)>());
    for refuse_overlap in [false, true] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_materialized_bytes = 2 * shell_bytes;
        policy.limits.max_retained_bytes = 0;
        policy.limits.max_collection_items = 2;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let body = |index: usize| {
            let mut storage = ctx.reserve_scoped(0, "test body shell storage").unwrap();
            let mut shells = storage.with_storage(|| {
                ctx.collection_vec(1, "test body shell uses")
            }).unwrap();
            shells.push((5 + 2 * u32::try_from(index).unwrap(), Sense::Forward));
            super::super::BodyDefinition {
                entry: &directory[index],
                kind: if index == 0 { BodyKind::Sheet } else { BodyKind::Solid },
                shells,
                closed: index == 1,
                transform: None,
                _shell_storage: storage,
            }
        };
        // Fixed stack slots isolate the owning record's shell backing.
        let mut bodies = [body(0), body(1)].into_iter();
        let first = bodies.next().unwrap();
        assert_eq!(first.shells, [(5, Sense::Forward)]);
        drop(first);
        let available = ctx.reserve_scoped(shell_bytes, "after first body drop").unwrap();
        drop(available);
        if refuse_overlap {
            let CodecError::ResourceLimit(first) = ctx
                .reserve_scoped(shell_bytes + 1, "live second body overlap")
                .err()
                .expect("the second body must still own its shell storage")
            else {
                panic!("expected materialized storage refusal");
            };
            assert_eq!(first.dimension, ResourceDimension::MaterializedBytes);
            assert_eq!((first.limit, first.used, first.additional), (2 * shell_bytes, shell_bytes, shell_bytes + 1));
            assert_eq!(first.operation, "live second body overlap");
            drop(bodies);
            assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(last)) if last == first));
        } else {
            let second = bodies.next().unwrap();
            assert_eq!(second.shells, [(7, Sense::Forward)]);
            drop(second);
            drop(bodies);
            let all = ctx.reserve_scoped(2 * shell_bytes, "all body shell storage released").unwrap();
            drop(all);
            ctx.finish_session().unwrap();
        }
    }
}
