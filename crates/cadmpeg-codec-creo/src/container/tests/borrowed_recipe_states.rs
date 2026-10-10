// SPDX-License-Identifier: Apache-2.0

use super::super::{depdb_recipe_rows, Section};
use crate::feature::rows::FeatureRow;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

const DISPLAY: &[u8] = b"\xe3Extrude id 8053\0";
const BINDING: &[u8] = b"\xe3\xf7\x50\x9f\x75\x83\x95\xf6\x9f\x73Profile 1\0\xf6\0protextrude\0";

fn row_fixture() -> (Vec<u8>, usize) {
    let mut bytes = DISPLAY.to_vec();
    bytes.extend_from_slice(BINDING);
    let body_len = bytes.len();
    bytes.extend_from_slice(b"unrelated tail");
    (bytes, body_len)
}

fn row_storage_bytes(rows: &Vec<FeatureRow>) -> u64 {
    u64::try_from(
        rows.capacity() * std::mem::size_of::<FeatureRow>()
            + rows.iter().map(|row| row.body.len()).sum::<usize>(),
    )
    .expect("actual row storage")
}

#[test]
fn recipe_row_selection_does_not_copy_unused_display_names() {
    let section = Section::scan_for_test("DEPDB_DATA".into(), 0, DISPLAY.len(), None, DISPLAY)
        .expect("bounded section");
    let cap = crate::test_support::allocation_limit_at(
        ResourceDimension::MaterializedBytes,
        None,
        |allowed| {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_retained_bytes = 0;
            policy.limits.max_materialized_bytes = allowed;
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
            depdb_recipe_rows(&ctx, std::slice::from_ref(&section))
        },
    );
    for nested in [false, true] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = 0;
        policy.limits.max_materialized_bytes = cap;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
        let mut parent = ctx.reserve_scoped(0, "recipe row parent").expect("parent");
        let rows = if nested {
            parent.with_storage(|| depdb_recipe_rows(&ctx, std::slice::from_ref(&section)))
        } else {
            depdb_recipe_rows(&ctx, std::slice::from_ref(&section))
        }
        .expect("display state has no recipe row");
        assert!(rows.is_empty());
        ctx.reserve_scoped(cap, "after recipe row selection")
            .expect("all borrowed state storage has ended");
        drop((rows, parent));
        let resource = ctx
            .reserve_scoped_limit(u64::MAX, "after recipe output drop")
            .expect_err("read released backing");
        assert_eq!(resource.used, 0);
    }
}

#[test]
fn recipe_rows_retain_only_body_and_row_backing() {
    let (bytes, body_len) = row_fixture();
    let section = Section::scan_for_test("DEPDB_DATA".into(), 0, bytes.len(), None, &bytes)
        .expect("bounded section");
    let cap = crate::test_support::allocation_limit_at(
        ResourceDimension::RetainedBytes,
        None,
        |allowed| {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_retained_bytes = allowed;
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
            depdb_recipe_rows(&ctx, std::slice::from_ref(&section))
        },
    );
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = cap;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
    let rows = depdb_recipe_rows(&ctx, std::slice::from_ref(&section))
        .expect("only surviving body and row backing");
    let [row] = rows.as_slice() else {
        panic!("one recipe row");
    };
    assert_eq!(row.feature_id, 8053);
    assert_eq!(
        row.root_schema_class
            .map(crate::feature::schema::SchemaClass::code),
        Some(917)
    );
    assert_eq!((row.stream_offset, row.body_offset, row.offset), (0, 0, 1));
    assert_eq!(&*row.body, &bytes[..body_len]);
    let resource = ctx
        .charge_retained_limit(1, "after retained recipe row")
        .expect_err("exact retained cap");
    assert_eq!(
        (resource.used, resource.additional),
        (row_storage_bytes(&rows), 1)
    );
    let error = crate::test_support::last_refusal_at(
        &[],
        ResourceDimension::RetainedBytes,
        "creo DEPDB recipe rows",
        |ctx| depdb_recipe_rows(ctx, std::slice::from_ref(&section)),
    );
    assert!(matches!(error, CodecError::ResourceLimit(resource)
        if resource.operation == "creo DEPDB recipe rows"
            && resource.used == u64::try_from(body_len).expect("body bytes")));
}

#[test]
fn recipe_rows_keep_only_output_storage_in_the_ambient_parent() {
    let (bytes, body_len) = row_fixture();
    let section = Section::scan_for_test("DEPDB_DATA".into(), 0, bytes.len(), None, &bytes)
        .expect("bounded section");
    let cap = crate::test_support::allocation_limit_at(
        ResourceDimension::MaterializedBytes,
        None,
        |allowed| {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_retained_bytes = 0;
            policy.limits.max_materialized_bytes = allowed;
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
            ctx.with_scoped_storage("recipe row parent", || {
                depdb_recipe_rows(&ctx, std::slice::from_ref(&section))
            })
            .map(drop)
        },
    );
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_materialized_bytes = cap;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
    let mut parent = ctx.reserve_scoped(0, "recipe row parent").expect("parent");
    let rows = parent
        .with_storage(|| depdb_recipe_rows(&ctx, std::slice::from_ref(&section)))
        .expect("row output belongs to ambient parent");
    assert_eq!(rows.len(), 1);
    assert_eq!(&*rows[0].body, &bytes[..body_len]);
    let probe = ctx
        .reserve_scoped(
            cap.checked_sub(row_storage_bytes(&rows))
                .expect("peak includes output"),
            "after ambient recipe rows",
        )
        .expect("only actual output backing remains live");
    drop(probe);
    drop((rows, parent));
    ctx.reserve_scoped(cap, "after ambient recipe row drop")
        .expect("actual output and receipt have ended");
    let resource = ctx
        .reserve_scoped_limit(u64::MAX, "after ambient recipe release")
        .expect_err("read released backing");
    assert_eq!(resource.used, 0);
}
