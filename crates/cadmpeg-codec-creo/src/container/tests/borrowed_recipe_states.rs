// SPDX-License-Identifier: Apache-2.0

use super::super::{depdb_recipe_rows, Section};
use crate::feature::rows::FeatureRow;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

const MATERIALIZED_BASE: u64 = 16 * 1024 * 1024;
const DISPLAY: &[u8] = b"\xe3Extrude id 8053\0";
const BINDING: &[u8] = b"\xe3\xf7\x50\x9f\x75\x83\x95\xf6\x9f\x73Profile 1\0\xf6\0protextrude\0";

fn row_fixture() -> (Vec<u8>, usize) {
    let mut bytes = DISPLAY.to_vec();
    bytes.extend_from_slice(BINDING);
    let body_len = bytes.len();
    bytes.extend_from_slice(b"unrelated tail");
    (bytes, body_len)
}

fn row_storage_bytes(body_len: usize) -> u64 {
    // First amortized growth admits four slots for this concrete row type.
    let slot_size = std::mem::size_of::<FeatureRow>();
    assert!((2..=1024).contains(&slot_size));
    u64::try_from(4 * slot_size + body_len).expect("one row allocation bound")
}

#[test]
fn recipe_row_selection_does_not_copy_unused_display_names() {
    let section = Section::scan_for_test("DEPDB_DATA".into(), 0, DISPLAY.len(), None, DISPLAY)
        .expect("bounded section");
    for nested in [false, true] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = 0;
        policy.limits.max_materialized_bytes = MATERIALIZED_BASE;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
        let mut parent = ctx.reserve_scoped(0, "recipe row parent").expect("parent");
        let rows = if nested {
            parent.with_storage(|| depdb_recipe_rows(&ctx, std::slice::from_ref(&section)))
        } else {
            depdb_recipe_rows(&ctx, std::slice::from_ref(&section))
        }.expect("display state has no recipe row");
        assert!(rows.is_empty());
        let probe = ctx.reserve_scoped(MATERIALIZED_BASE, "after recipe row selection")
            .expect("all borrowed state storage has ended");
        drop(probe);
        drop((rows, parent));
        let original = ctx.charge_retained_limit(1, "after unused recipe display")
            .expect_err("no retained output allowance");
        assert_eq!((original.dimension, original.used, original.additional),
            (ResourceDimension::RetainedBytes, 0, 1));
        assert!(matches!(depdb_recipe_rows(&ctx, &[]),
            Err(CodecError::ResourceLimit(actual)) if actual == original));
    }
}

#[test]
fn recipe_rows_retain_only_body_and_row_backing() {
    let (bytes, body_len) = row_fixture();
    let section = Section::scan_for_test("DEPDB_DATA".into(), 0, bytes.len(), None, &bytes)
        .expect("bounded section");
    let retained = row_storage_bytes(body_len);
    for cap in [retained - 1, retained] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = cap;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
        let result = depdb_recipe_rows(&ctx, std::slice::from_ref(&section));
        if cap == retained {
            let rows = result.expect("exact surviving body and row backing");
            let [row] = rows.as_slice() else { panic!("one recipe row"); };
            assert_eq!(row.feature_id, 8053);
            assert_eq!(row.root_schema_class.map(crate::feature::schema::SchemaClass::code), Some(917));
            assert_eq!((row.stream_offset, row.body_offset, row.offset), (0, 0, 1));
            assert_eq!(&*row.body, &bytes[..body_len]);
            let original = ctx.charge_retained_limit(1, "after retained recipe row")
                .expect_err("exact retained cap");
            assert_eq!((original.used, original.additional), (retained, 1));
        } else {
            let Err(CodecError::ResourceLimit(original)) = result else {
                panic!("last actual row allocation needs its full backing");
            };
            assert_eq!(original.dimension, ResourceDimension::RetainedBytes);
            assert_eq!(original.operation, "creo DEPDB recipe rows");
            assert_eq!((original.used, original.additional),
                (u64::try_from(body_len).expect("body bytes"), retained - u64::try_from(body_len).expect("body bytes")));
        }
        let original = ctx.resource_refusal().expect("original allocation refusal");
        assert!(matches!(depdb_recipe_rows(&ctx, &[]),
            Err(CodecError::ResourceLimit(actual)) if actual == original));
    }
}

#[test]
fn recipe_rows_keep_only_output_storage_in_the_ambient_parent() {
    let (bytes, body_len) = row_fixture();
    let section = Section::scan_for_test("DEPDB_DATA".into(), 0, bytes.len(), None, &bytes)
        .expect("bounded section");
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_materialized_bytes = MATERIALIZED_BASE;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
    let mut parent = ctx.reserve_scoped(0, "recipe row parent").expect("parent");
    let rows = parent.with_storage(|| depdb_recipe_rows(&ctx, std::slice::from_ref(&section)))
        .expect("row output belongs to ambient parent");
    assert_eq!(rows.len(), 1);
    let probe = ctx.reserve_scoped(MATERIALIZED_BASE - row_storage_bytes(body_len), "after ambient recipe rows")
        .expect("only actual output backing remains live");
    drop(probe);
    drop((rows, parent));
    let probe = ctx.reserve_scoped(MATERIALIZED_BASE, "after ambient recipe row drop")
        .expect("actual output and receipt have ended");
    drop(probe);
    let original = ctx.charge_retained_limit(1, "after ambient recipe row drop")
        .expect_err("zero retained cap");
    assert_eq!((original.dimension, original.used, original.additional),
        (ResourceDimension::RetainedBytes, 0, 1));
    assert!(matches!(depdb_recipe_rows(&ctx, &[]),
        Err(CodecError::ResourceLimit(actual)) if actual == original));
}
