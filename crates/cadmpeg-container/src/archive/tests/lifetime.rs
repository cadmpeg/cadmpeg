use super::{archive_bytes, ArchiveSnapshot, DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_core::CodecError;

const CALLER_BYTES: usize = 1_000_000;

#[test]
fn snapshot_drop_releases_metadata_inside_caller_storage_scope() {
    let bytes = archive_bytes();
    for capture in [false, true] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = 0;
        policy.limits.max_materialized_bytes = cadmpeg_core::decode::u64_from_index(CALLER_BYTES);
        let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).expect("root");
        let build = || -> Result<Vec<u8>, CodecError> {
            let snapshot = ArchiveSnapshot::new(&ctx, root)?;
            assert_eq!(snapshot.entries().len(), 3);
            assert_eq!(snapshot.entries()[0].name, "stored.bin");
            drop(snapshot);
            // The replacement needs every byte of the allowance. Any entry,
            // name, or index storage left in the caller scope makes it refuse.
            ctx.vector_storage(CALLER_BYTES, "replace dropped ZIP snapshot")
        };
        let (replacement, storage) = if capture {
            ctx.with_scoped_storage("ZIP snapshot caller", build)
                .expect("released snapshot bytes")
        } else {
            let mut storage = ctx
                .reserve_scoped(0, "ZIP snapshot caller")
                .expect("caller scope");
            let replacement = storage
                .with_storage(build)
                .expect("released snapshot bytes");
            (replacement, storage)
        };
        assert_eq!(replacement.capacity(), CALLER_BYTES);
        drop((replacement, storage));
        ctx.finish_session().expect("no retained snapshot storage");
    }
}

#[test]
fn ledger_drop_releases_spans_and_role_names_inside_caller_storage_scope() {
    let bytes = archive_bytes();
    let setup_arena = DecodeArena::new();
    let (setup, root) =
        DecodeContext::from_root_bytes(&bytes, &setup_arena, &DecodePolicy::service())
            .expect("root");
    let snapshot = ArchiveSnapshot::new(&setup, root).expect("snapshot");
    for capture in [false, true] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = 0;
        policy.limits.max_materialized_bytes = cadmpeg_core::decode::u64_from_index(CALLER_BYTES);
        let (ctx, _) =
            DecodeContext::from_root_bytes(&bytes, &arena, &policy).expect("caller root");
        let build = || -> Result<Vec<u8>, CodecError> {
            let (spans, storage) = snapshot.physical_ledger(&ctx)?;
            assert_eq!(spans.first().expect("first span").start, 0);
            assert_eq!(
                spans.last().expect("last span").end,
                cadmpeg_core::decode::u64_from_index(bytes.len())
            );
            assert!(spans.windows(2).all(|pair| pair[0].end == pair[1].start));
            drop((spans, storage));
            // All region, partition-index, span, and role-name storage must
            // be gone while the enclosing caller reservation remains live.
            ctx.vector_storage(CALLER_BYTES, "replace dropped ZIP ledger")
        };
        let (replacement, storage) = if capture {
            ctx.with_scoped_storage("ZIP ledger caller", build)
                .expect("released ledger bytes")
        } else {
            let mut storage = ctx
                .reserve_scoped(0, "ZIP ledger caller")
                .expect("caller scope");
            let replacement = storage.with_storage(build).expect("released ledger bytes");
            (replacement, storage)
        };
        assert_eq!(replacement.capacity(), CALLER_BYTES);
        drop((replacement, storage));
        ctx.finish_session().expect("no retained ledger storage");
    }
}
