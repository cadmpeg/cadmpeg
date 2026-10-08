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
