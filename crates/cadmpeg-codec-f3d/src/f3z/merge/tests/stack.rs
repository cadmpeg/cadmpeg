// SPDX-License-Identifier: Apache-2.0
//! Recursive stack cleanup keeps child paths covered through return and unwind.

use super::super::MergeStackFrame;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_core::CodecError;

const CHILD: &str = "child.f3d";

fn policy() -> DecodePolicy {
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_materialized_bytes =
        u64::try_from(2 * std::mem::size_of::<String>() + CHILD.len()).unwrap();
    policy
}

#[test]
fn merge_stack_frame_releases_child_on_return_and_error() {
    for fails in [false, true] {
        let arena = DecodeArena::new();
        let policy = policy();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let (mut stack_storage, mut stack) = {
            let (stack, storage) = ctx
                .scoped_vector_storage::<String>(2, "test stack")
                .unwrap();
            (storage, stack)
        };
        stack.push("root.f3d".into());
        let result = (|| -> Result<(), CodecError> {
            let (path_storage, path) = {
                let (path, storage) = ctx.with_scoped_storage("test child path", || {
                    ctx.copy_retained_text(CHILD, "copy test child path")
                })?;
                (storage, path)
            };
            ctx.push_scoped_vec(&mut stack_storage, &mut stack, path, "test stack push")?;
            let frame = MergeStackFrame {
                stack: &mut stack,
                _path_storage: path_storage,
            };
            assert_eq!(frame.stack.as_slice(), ["root.f3d", CHILD]);
            if fails {
                return Err(CodecError::malformed("child failed"));
            }
            Ok(())
        })();
        assert_eq!(result.is_err(), fails);
        assert_eq!(stack, ["root.f3d"]);
        let released = ctx
            .reserve_scoped(u64::try_from(CHILD.len()).unwrap(), "child released")
            .unwrap();
        drop(released);
        drop(stack);
        drop(stack_storage);
        ctx.finish_session().unwrap();
    }
}

#[test]
fn merge_stack_frame_releases_child_on_panic() {
    let arena = DecodeArena::new();
    let policy = policy();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let (mut stack_storage, mut stack) = {
        let (stack, storage) = ctx
            .scoped_vector_storage::<String>(2, "test stack")
            .unwrap();
        (storage, stack)
    };
    stack.push("root.f3d".into());
    let unwind = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let (path_storage, path) = {
            let (path, storage) = ctx
                .with_scoped_storage("test child path", || {
                    ctx.copy_retained_text(CHILD, "copy test child path")
                })
                .unwrap();
            (storage, path)
        };
        ctx.push_scoped_vec(&mut stack_storage, &mut stack, path, "test stack push")
            .unwrap();
        let frame = MergeStackFrame {
            stack: &mut stack,
            _path_storage: path_storage,
        };
        assert_eq!(frame.stack.as_slice(), ["root.f3d", CHILD]);
        panic!("child panic");
    }));
    assert!(unwind.is_err());
    assert_eq!(stack, ["root.f3d"]);
    let released = ctx
        .reserve_scoped(u64::try_from(CHILD.len()).unwrap(), "child released")
        .unwrap();
    drop(released);
    drop(stack);
    drop(stack_storage);
    ctx.finish_session().unwrap();
}

#[test]
fn merge_stack_frame_preserves_original_refusal() {
    let arena = DecodeArena::new();
    let policy = policy();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let (mut stack_storage, mut stack) = {
        let (stack, storage) = ctx
            .scoped_vector_storage::<String>(2, "test stack")
            .unwrap();
        (storage, stack)
    };
    stack.push("root.f3d".into());
    let error = (|| -> Result<(), CodecError> {
        let (path_storage, path) = {
            let (path, storage) = ctx.with_scoped_storage("test child path", || {
                ctx.copy_retained_text(CHILD, "copy test child path")
            })?;
            (storage, path)
        };
        ctx.push_scoped_vec(&mut stack_storage, &mut stack, path, "test stack push")?;
        let _frame = MergeStackFrame {
            stack: &mut stack,
            _path_storage: path_storage,
        };
        ctx.charge_work(u64::MAX, "original child refusal")?;
        Ok(())
    })()
    .unwrap_err();
    let CodecError::ResourceLimit(first) = error else {
        panic!("child must refuse")
    };
    assert_eq!(first.operation, "original child refusal");
    assert_eq!(ctx.resource_refusal(), Some(first));
    assert_eq!(stack, ["root.f3d"]);
    drop(stack);
    drop(stack_storage);
    assert!(
        matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(limit)) if limit == first)
    );
}

#[test]
fn merge_stack_child_copy_and_push_refusals_preserve_parent() {
    for copy_refuses in [false, true] {
        let arena = DecodeArena::new();
        let mut policy = policy();
        if copy_refuses {
            policy.limits.max_materialized_bytes -= 1;
        } else {
            policy.limits.max_collection_items = 0;
        }
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let (mut stack_storage, mut stack) = {
            let (stack, storage) = ctx
                .scoped_vector_storage::<String>(2, "test stack")
                .unwrap();
            (storage, stack)
        };
        stack.push("root.f3d".into());
        let error = (|| -> Result<(), CodecError> {
            let (path_storage, path) = {
                let (path, storage) = ctx.with_scoped_storage("test child path", || {
                    ctx.copy_retained_text(CHILD, "copy test child path")
                })?;
                (storage, path)
            };
            ctx.push_scoped_vec(&mut stack_storage, &mut stack, path, "test stack push")?;
            let _frame = MergeStackFrame {
                stack: &mut stack,
                _path_storage: path_storage,
            };
            Ok(())
        })()
        .unwrap_err();
        let CodecError::ResourceLimit(first) = error else {
            panic!("child must refuse")
        };
        assert_eq!(
            first.operation,
            if copy_refuses {
                "copy test child path"
            } else {
                "test stack push"
            }
        );
        assert_eq!(ctx.resource_refusal(), Some(first));
        assert_eq!(stack, ["root.f3d"]);
        drop(stack);
        drop(stack_storage);
        assert!(
            matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(limit)) if limit == first)
        );
    }
}

#[test]
fn missing_member_does_not_materialize_an_unused_occurrence_key() {
    use super::super::MergeSession;
    use crate::f3z::archive::ArchiveSession;
    use crate::records::xref::XrefReference;
    let bytes =
        crate::test_support::assembly_test::f3z_archive("root.f3d", &[("root.f3d", b"root")]);
    crate::test_support::with_decode_context(|scan_ctx| {
        let scan =
            crate::container::scan(scan_ctx, cadmpeg_core::decode::View::over_retained(&bytes))
                .unwrap();
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_materialized_bytes = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let archive = ArchiveSession {
            members: std::collections::BTreeMap::new(),
            layers: cadmpeg_core::dialect::DialectLayers::of(scan.kind.dialect().clone()),
            losses: Vec::new(),
            _members_storage: ctx.reserve_scoped(0, "test empty member index").unwrap(),
        };
        let table = crate::xref::XrefTable {
            references: vec![XrefReference {
                id: "f3d:xref:reference#1".into(),
                ordinal: 1,
                occurrence_ordinal: 0,
                from: "root.f3d".to_owned().try_into().unwrap(),
                relative_path: "missing.f3d".to_owned().try_into().unwrap(),
                neutron_role: "x".repeat(4096).try_into().unwrap(),
                neutron_data: String::new(),
                transform: None,
            }],
            ..Default::default()
        };
        let mut report = cadmpeg_ir::codec::DecodeBody::new(
            cadmpeg_ir::report::decode::DecodeTransfer::full(false),
        );
        let mut ir = cadmpeg_ir::CadIr::empty();
        let mut fidelity = cadmpeg_ir::SourceFidelity::default();
        let mut stack_storage = ctx.reserve_scoped(0, "test empty stack").unwrap();
        let count = MergeSession {
            ctx: &ctx,
            scan: &scan,
            archive: &archive,
        }
        .merge(
            &mut Vec::new(),
            &mut stack_storage,
            &mut ir,
            &mut report,
            &mut fidelity,
            &table,
        )
        .unwrap();
        assert_eq!(count, 0);
        assert_eq!(report.losses.len(), 1);
        assert_eq!(
            report.losses[0].code,
            crate::loss::F3dLossCode::XrefMemberMissing.kind()
        );
        drop((stack_storage, archive));
        ctx.finish_session().unwrap();
    });
}
