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
    use super::super::FeatureIdentityIndex;
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
    use cadmpeg_core::CodecError;

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
    let run = |allowed| {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_materialized_bytes = allowed;
        policy.limits.max_retained_bytes = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
        FeatureIdentityIndex::new(
            &ctx,
            std::slice::from_ref(&row),
            &structural,
            &[],
            std::slice::from_ref(&reference),
        )
        .map(drop)
    };
    let cap =
        crate::test_support::allocation_limit_at(ResourceDimension::MaterializedBytes, None, run);
    let below = crate::test_support::allocation_limit_at(
        ResourceDimension::MaterializedBytes,
        Some("creo reference identity kinds"),
        run,
    );
    let expected_live = {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_materialized_bytes = cap;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
        let mut storage = ctx
            .reserve_scoped(0, "test one identity table")
            .expect("storage");
        let mut map = std::collections::HashMap::<u32, u8>::new();
        storage
            .with_storage(|| {
                ctx.insert_hash_map(
                    &mut map,
                    7,
                    FeatureIdentityIndex::DATUM,
                    "test one identity table",
                )
            })
            .expect("one live reference table");
        let resource = ctx
            .reserve_scoped_limit(cap + 1, "probe one identity table")
            .expect_err("read actual admitted storage");
        resource.used
    };
    for (allowed, probe_while_live) in [(below, false), (cap, true), (cap, false)] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_materialized_bytes = allowed;
        policy.limits.max_retained_bytes = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
        let result = ctx.with_scoped_storage("identity selection parent", || {
            FeatureIdentityIndex::new(
                &ctx,
                std::slice::from_ref(&row),
                &structural,
                &[],
                std::slice::from_ref(&reference),
            )
        });
        if allowed == below {
            assert!(matches!(result, Err(CodecError::ResourceLimit(resource))
                if resource.dimension == ResourceDimension::MaterializedBytes
                    && resource.operation == "creo reference identity kinds"
                    && resource.used == expected_live));
            continue;
        }
        let (index, storage) = result.expect("owner last-use refund admits reference growth");
        assert!(index
            .contains(&ctx, &row, &structural)
            .expect("datum identity"));
        let mut index = Some(index);
        let live = if probe_while_live {
            expected_live
        } else {
            drop(index.take());
            0
        };
        let resource = ctx
            .reserve_scoped_limit(cap + 1, "after identity owner selection")
            .expect_err("read actual live index storage");
        assert_eq!(resource.dimension, ResourceDimension::MaterializedBytes);
        assert_eq!(resource.used, live);
        drop(index);
        drop(storage);
    }
}

#[test]
fn datum_identity_index_omits_zero_reference_masks() {
    for count in [1, 256] {
        let rows = (1..=count)
            .map(|feature_id| FeatureRow {
                feature_id,
                root_schema_class: Some(SchemaClass::DatumPlane),
                stream_offset: 0,
                body: vec![0; 2].try_into().expect("body"),
                body_offset: 0,
                offset: 0,
            })
            .collect::<Vec<_>>();
        let references = (1..=count)
            .map(|feature_id| FeatureReferenceName {
                feature_id,
                name_bytes: b"arbitrary".to_vec(),
                own_reference_id: 0,
                reference_type: 0,
                offset: 0,
            })
            .collect::<Vec<_>>();
        crate::decode::with_test_decode_ctx(|ctx| {
            let structural = std::collections::BTreeSet::new();
            let index =
                super::super::FeatureIdentityIndex::new(ctx, &rows, &structural, &[], &references)?;
            assert!(index.reference_kinds.is_empty());
            for row in &rows {
                assert!(!index.contains(ctx, row, &structural)?);
            }
            let resource = ctx
                .reserve_scoped_limit(u64::MAX, "zero-mask index storage")
                .expect_err("read live index storage");
            assert_eq!(resource.used, 0);
            Ok::<_, cadmpeg_core::CodecError>(())
        })
        .expect("zero-mask identities need no surviving index storage");
    }
}
