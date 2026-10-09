// SPDX-License-Identifier: Apache-2.0
use crate::feature::operations::FeatureReferenceName;
use crate::feature::rows::FeatureRow;
use crate::feature::schema::SchemaClass;
use cadmpeg_core::decode::ResourceDimension;

#[test]
fn release_words_preserve_first_selected_value() {
    for (banner, expected) in [
        (b"Version\t2020 ignored".as_slice(), Some("2020")),
        (b"prefix Release2020".as_slice(), Some("2020")),
        (b"Release\xff Release2020".as_slice(), Some("2020")),
        (b"Release \xff Release2020".as_slice(), None),
        (b"Version Release2020".as_slice(), Some("Release2020")),
        (b"\xff Release 2020".as_slice(), Some("2020")),
        (b"Release".as_slice(), None),
    ] {
        let actual = crate::decode::with_test_decode_ctx(|ctx| {
            super::super::legacy_product_release(ctx, banner)
        })
        .expect("release selection");
        assert_eq!(actual.as_deref(), expected);
    }
}

#[test]
fn release_selection_does_not_visit_trailing_words() {
    let short = b"Release 2020 ";
    let mut long = short.to_vec();
    long.extend_from_slice(&[b'x'; 1024]);
    let cap = |banner: &[u8]| {
        let error = crate::test_support::last_refusal_at(
            &[],
            ResourceDimension::WorkUnits,
            "creo legacy product release",
            |ctx| super::super::legacy_product_release(ctx, banner),
        );
        let cadmpeg_core::CodecError::ResourceLimit(refusal) = error else {
            panic!("resource refusal");
        };
        refusal
            .used
            .checked_add(refusal.additional)
            .expect("work need")
    };
    assert_eq!(cap(short), cap(&long));
}

#[test]
fn identity_index_preserves_reference_classes_and_invalid_names() {
    let row = |class| FeatureRow {
        feature_id: 7,
        root_schema_class: class,
        stream_offset: 0,
        body: vec![0; 2].try_into().expect("row body"),
        body_offset: 0,
        offset: 0,
    };
    let names = [
        b"Datum Plane id 7".as_slice(),
        b"DTM0007",
        b"PRT_CSYS_DEF",
        b"arbitrary",
        b"\xff",
    ];
    for (name, datum, coordinate, section) in [
        (names[0], true, false, true),
        (names[1], true, false, true),
        (names[2], false, true, true),
        (names[3], false, false, true),
        (names[4], false, false, false),
    ] {
        let reference = FeatureReferenceName {
            feature_id: 7,
            name_bytes: name.to_vec(),
            own_reference_id: 0,
            reference_type: 0,
            offset: 0,
        };
        for (class, expected) in [
            (Some(SchemaClass::DatumPlane), datum),
            (Some(SchemaClass::CoordinateSystem), coordinate),
            (Some(SchemaClass::Section), section),
            (None, false),
            (Some(SchemaClass::Hole), false),
        ] {
            let actual = crate::decode::with_test_decode_ctx(|ctx| {
                super::super::feature_row_has_model_identity(
                    ctx,
                    &row(class),
                    &std::collections::BTreeSet::new(),
                    &[],
                    std::slice::from_ref(&reference),
                )
            })
            .expect("identity selection");
            assert_eq!(actual, expected, "name {name:?}, class {class:?}");
        }
    }
}

#[test]
fn cmnm_fixed_length_field_rejects_invalid_text() {
    for bytes in [
        b"#- CMNM 00\xffx".as_slice(),
        b"#- CMNM xyzx".as_slice(),
        b"#- CMNM 000x".as_slice(),
    ] {
        let actual =
            crate::decode::with_test_decode_ctx(|ctx| super::super::cmnm_model_name(ctx, bytes))
                .expect("bounded field inspection");
        assert_eq!(actual, None);
    }
}


#[test]
fn identity_owner_selection_drops_before_reference_index_growth() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
    use cadmpeg_core::CodecError;
    use super::super::FeatureIdentityIndex;

    // One entry uses the core growth minimum of four buckets. The bound
    // includes entry bytes, alignment padding, and the control-byte group.
    fn one_entry_bytes<T>() -> u64 {
        u64::try_from(4 * std::mem::size_of::<T>()
            + std::mem::align_of::<T>().max(16) - 1 + 4 + 16)
            .expect("one-entry table storage")
    }
    let owner_bytes = one_entry_bytes::<u32>();
    let selection_bytes = one_entry_bytes::<(u32, u8)>();
    // Selection growth overlaps owners. Reference growth overlaps only the
    // still-needed selection, then retains its own equal-sized table.
    let cap = (owner_bytes + selection_bytes).max(2 * selection_bytes);
    let row = FeatureRow {
        feature_id: 7,
        root_schema_class: Some(SchemaClass::DatumPlane),
        stream_offset: 0,
        body: vec![0; 2].try_into().expect("row body"),
        body_offset: 0,
        offset: 0,
    };
    let reference = FeatureReferenceName {
        feature_id: 7,
        name_bytes: b"Datum Plane".to_vec(),
        own_reference_id: 0,
        reference_type: 0,
        offset: 0,
    };
    let structural = std::collections::BTreeSet::new();
    for (allowed, probe_while_live) in [(cap - 1, false), (cap, true), (cap, false)] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_materialized_bytes = allowed;
        policy.limits.max_retained_bytes = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
        let result = ctx.with_scoped_storage("identity selection parent", ||
            FeatureIdentityIndex::new(&ctx, std::slice::from_ref(&row), &structural,
                &[], std::slice::from_ref(&reference)));
        let original = if allowed < cap {
            let Err(CodecError::ResourceLimit(refusal)) = result else {
                panic!("one byte below reference growth must refuse");
            };
            assert_eq!(refusal.dimension, ResourceDimension::MaterializedBytes);
            assert_eq!(refusal.operation, "creo reference identity kinds");
            assert_eq!((refusal.used, refusal.additional),
                (selection_bytes, selection_bytes));
            refusal
        } else {
            let (index, storage) = result.expect("owner last-use refund admits reference growth");
            assert!(index.contains(&ctx, &row, &structural).expect("datum identity"));
            let mut index = Some(index);
            let retained_bytes = if probe_while_live {
                selection_bytes
            } else {
                drop(index.take());
                0
            };
            let refusal = ctx.reserve_scoped_limit(cap + 1, "after identity owner selection")
                .expect_err("probe actual live index storage");
            assert_eq!(refusal.dimension, ResourceDimension::MaterializedBytes);
            assert_eq!(refusal.used, retained_bytes);
            drop(index);
            drop(storage);
            refusal
        };
        assert!(matches!(FeatureIdentityIndex::new(&ctx, std::slice::from_ref(&row),
            &structural, &[], std::slice::from_ref(&reference)),
            Err(CodecError::ResourceLimit(refusal)) if refusal == original));
        assert_eq!(ctx.resource_refusal(), Some(original));
    }
}
