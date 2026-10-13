use crate::om::registry::RegistryTokenForm;
use crate::om::FieldDefinition;
use crate::om::TypeDefinition;

fn append_declaration(bytes: &mut Vec<u8>, name: &[u8], tail: &[u8]) {
    bytes.push(u8::try_from(name.len() + 1).expect("synthetic declaration fits in one byte"));
    bytes.extend_from_slice(name);
    bytes.extend_from_slice(tail);
}

#[test]
fn decodes_direct_class_registry_tail() {
    let fingerprint = [0x10, 0x20, 0x30, 0x40, 0x50, 0x60, 0x70, 0x80];
    let definition = TypeDefinition {
        offset: 0,
        name: "UGS::OM::RootObject",
        registry_tail: &[
            0x38, 0x05, 0x10, 0x20, 0x30, 0x40, 0x50, 0x60, 0x70, 0x80, 0x02,
        ],
    };

    let layout = crate::om::registry::class_registry_layout(definition.registry_tail).unwrap();

    assert_eq!(layout.storage_code.value(), 0x38);
    assert_eq!(layout.storage_code.form(), RegistryTokenForm::Direct);
    assert_eq!(layout.storage_code.width(), 1);
    assert_eq!(layout.base_class.map_or(0, std::num::NonZeroU32::get), 0x05);
    assert_eq!(layout.schema_fingerprint, fingerprint);
    assert_eq!(layout.reference.get(), 0x02);
}

#[test]
fn decodes_compact_and_wide_class_registry_tokens() {
    let fingerprint = [0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0xff, 0x01, 0x23];
    let compact = TypeDefinition {
        offset: 0,
        name: "UGS::OM::SaveAuditTrail",
        registry_tail: &[
            0x80, 0xc9, 0x09, 0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0xff, 0x01, 0x23, 0x01,
        ],
    };

    let compact_layout = crate::om::registry::class_registry_layout(compact.registry_tail).unwrap();
    assert_eq!(compact_layout.storage_code.value(), 0xc9 + 1);
    assert_eq!(
        compact_layout.storage_code.form(),
        RegistryTokenForm::Compact
    );
    assert_eq!(compact_layout.storage_code.width(), 2);
    assert_eq!(
        compact_layout
            .base_class
            .map_or(0, std::num::NonZeroU32::get),
        0x09
    );
    assert_eq!(compact_layout.schema_fingerprint, fingerprint);
    assert_eq!(compact_layout.reference.get(), 0x01);

    let wide = TypeDefinition {
        offset: 0,
        name: "UGS::Part::Unit::ProxySystemMeasure",
        registry_tail: &[
            0xa0, 0x27, 0x10, 0x82, 0x3c, 0x12, 0x34, 0x56, 0x78, 0x9a, 0xbc, 0xde, 0xf0, 0x80,
            0xd3,
        ],
    };

    let wide_layout = crate::om::registry::class_registry_layout(wide.registry_tail).unwrap();
    assert_eq!(wide_layout.storage_code.value(), 0x27_10 + 1);
    assert_eq!(wide_layout.storage_code.form(), RegistryTokenForm::Wide);
    assert_eq!(wide_layout.storage_code.width(), 3);
    assert_eq!(
        wide_layout.base_class.map_or(0, std::num::NonZeroU32::get),
        0x02_3c + 1
    );
    assert_eq!(
        wide_layout.schema_fingerprint,
        [0x12, 0x34, 0x56, 0x78, 0x9a, 0xbc, 0xde, 0xf0]
    );
    assert_eq!(wide_layout.reference.get(), 0xd3 + 1);
}

#[test]
fn decodes_each_extended_registry_prefix_family() {
    let compact = crate::om::registry::registry_token_at(&[0x8f, 0xfe], 0).unwrap();
    assert_eq!(compact.value(), 0x0f00 + 0xfe + 1);
    assert_eq!(compact.form(), RegistryTokenForm::Compact);
    assert_eq!(compact.width(), 2);

    let wide_90 = crate::om::registry::registry_token_at(&[0x90, 0x12, 0x34], 0).unwrap();
    assert_eq!(wide_90.value(), 0x1234 + 1);
    assert_eq!(wide_90.form(), RegistryTokenForm::Wide);
    assert_eq!(wide_90.width(), 3);

    let wide_a1 = crate::om::registry::registry_token_at(&[0xa1, 0x12, 0x34], 0).unwrap();
    assert_eq!(wide_a1.value(), 0x1_1234 + 1);
    assert_eq!(wide_a1.form(), RegistryTokenForm::Wide);

    let wide_f1 = crate::om::registry::registry_token_at(&[0xf1, 0x12, 0x34], 0).unwrap();
    assert_eq!(wide_f1.value(), 0x1_1234 + 1);
    assert_eq!(wide_f1.form(), RegistryTokenForm::Wide);
}

#[test]
fn decodes_direct_and_compact_field_registry_heads() {
    let direct = FieldDefinition {
        offset: 0,
        name: "m_objectStateCollection",
        registry_tail: &[0x78, 0x28, 0x12, 0x34],
    };
    let direct_layout = crate::om::registry::field_registry_layout(direct.registry_tail).unwrap();
    assert_eq!(direct_layout.storage_code.value(), 0x78);
    assert_eq!(direct_layout.storage_code.form(), RegistryTokenForm::Direct);
    assert_eq!(direct_layout.owner_class.get(), 0x28);

    let compact = FieldDefinition {
        offset: 0,
        name: "first_record_area",
        registry_tail: &[0x80, 0xcf, 0x28],
    };
    let compact_layout = crate::om::registry::field_registry_layout(compact.registry_tail).unwrap();
    assert_eq!(compact_layout.storage_code.value(), 0xcf + 1);
    assert_eq!(
        compact_layout.storage_code.form(),
        RegistryTokenForm::Compact
    );
    assert_eq!(compact_layout.owner_class.get(), 0x28);
}

#[test]
fn rejects_incomplete_or_null_registry_tails() {
    let truncated = TypeDefinition {
        offset: 0,
        name: "UGS::OM::Truncated",
        registry_tail: &[0x38, 0x05, 0x00, 0x01],
    };
    assert!(crate::om::registry::class_registry_layout(truncated.registry_tail).is_none());

    let null_storage = FieldDefinition {
        offset: 0,
        name: "m_null",
        registry_tail: &[0xff],
    };
    assert!(crate::om::registry::field_registry_layout(null_storage.registry_tail).is_none());

    let null_owner = FieldDefinition {
        offset: 0,
        name: "m_null_owner",
        registry_tail: &[0x78, 0x00],
    };
    assert!(crate::om::registry::field_registry_layout(null_owner.registry_tail).is_none());
}

#[test]
fn separates_complete_reference_class_and_member_regions() {
    let mut bytes = Vec::new();
    append_declaration(&mut bytes, b"UGS::OM::Meta", &[0x02]);
    append_declaration(&mut bytes, b"UGS::Solid::Topol", &[0x19]);
    bytes.push(0x01);

    append_declaration(
        &mut bytes,
        b"UGS::OM::RootObject",
        &[
            0x38, 0x00, 0x10, 0x20, 0x30, 0x40, 0x50, 0x60, 0x70, 0x80, 0x01,
        ],
    );
    append_declaration(
        &mut bytes,
        b"UGS::OM::ChildObject",
        &[
            0x80, 0xc9, 0x01, 0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0xff, 0x01, 0x23, 0x02,
        ],
    );
    bytes.push(0x02);

    let member_start = bytes.len();
    append_declaration(&mut bytes, b"first_record_area", &[0x80, 0xcf, 0x01]);
    let first_member_start = bytes.len();
    append_declaration(&mut bytes, b"m_first", &[0x78, 0x01]);
    append_declaration(&mut bytes, b"m_second", &[0x10, 0x02]);
    append_declaration(&mut bytes, b"m_tail", &[0x11, 0x03]);

    let registry = crate::test_support::with_decode_context(|ctx| {
        crate::om::registry::type_registry(ctx, &bytes, 0, bytes.len())
    })
    .unwrap();
    assert_eq!(registry.definitions.len(), 2);
    assert_eq!(registry.field_owner_count, std::num::NonZeroUsize::new(2));
    assert_eq!(registry.definitions[0].name, "UGS::OM::RootObject");
    assert_eq!(registry.definitions[1].name, "UGS::OM::ChildObject");
    assert_eq!(
        crate::om::registry::class_registry_layout(registry.definitions[0].registry_tail)
            .unwrap()
            .base_class
            .map_or(0, std::num::NonZeroU32::get),
        0
    );
    assert_eq!(
        crate::om::registry::class_registry_layout(registry.definitions[1].registry_tail)
            .unwrap()
            .storage_code
            .value(),
        0xc9 + 1
    );
    assert_eq!(registry.field_start, member_start);

    let fields = crate::test_support::with_decode_context(|ctx| {
        crate::om::registry::all_field_definitions(
            ctx,
            &bytes,
            registry.field_start,
            bytes.len(),
            registry.field_owner_count,
        )
    })
    .unwrap();
    assert_eq!(fields.len(), 3);
    assert_eq!(fields[0].name, "m_first");
    assert_eq!(
        crate::om::registry::field_registry_layout(fields[0].registry_tail)
            .unwrap()
            .owner_class
            .get(),
        1
    );
    assert_eq!(fields[1].name, "m_second");
    assert_eq!(
        crate::om::registry::field_registry_layout(fields[1].registry_tail)
            .unwrap()
            .owner_class
            .get(),
        2
    );
    assert_eq!(fields[2].name, "m_tail");

    let mut one_byte_gap = bytes;
    one_byte_gap[member_start - 1] = 0x28;
    let variant = crate::test_support::with_decode_context(|ctx| {
        crate::om::registry::type_registry(ctx, &one_byte_gap, 0, one_byte_gap.len())
    })
    .unwrap();
    assert_eq!(variant.definitions.len(), 2);
    assert_eq!(variant.field_start, first_member_start);
}

#[test]
fn field_search_offset_overflow_refuses_record() {
    crate::test_support::with_decode_context(|ctx| {
        assert!(matches!(
            super::field_definitions(ctx, &[], usize::MAX, 0, None),
            Err(cadmpeg_core::CodecError::Malformed(_))
        ));
    });
}

#[test]
fn splits_bounded_plain_declaration_with_a_different_owner() {
    // The first two declarations reproduce the source frame at bison-chuck offset 25771.
    let mut bytes = Vec::new();
    append_declaration(&mut bytes, b"m_displayMarker", &[0x83, 0x87, 0x80, 0xa5]);
    let plain_start = bytes.len();
    append_declaration(&mut bytes, b"first_record_area", &[0x80, 0xcf, 0x16]);
    let next_start = bytes.len();
    append_declaration(&mut bytes, b"m_next", &[0x01, 0x01]);
    let fields = crate::test_support::with_decode_context(|ctx| {
        super::all_field_definitions(
            ctx,
            &bytes,
            0,
            bytes.len(),
            std::num::NonZeroUsize::new(206),
        )
    })
    .unwrap();
    assert_eq!(
        fields.iter().map(|field| field.name).collect::<Vec<_>>(),
        ["m_displayMarker", "first_record_area", "m_next"]
    );
    assert_eq!(fields[0].registry_tail, &[0x83, 0x87, 0x80, 0xa5]);
    assert_eq!(fields[1].offset, plain_start);
    assert_eq!(fields[1].registry_tail, &[0x80, 0xcf, 0x16]);
    assert_eq!(
        super::field_registry_layout(fields[0].registry_tail)
            .unwrap()
            .owner_class
            .get(),
        166
    );
    assert_eq!(
        super::field_registry_layout(fields[1].registry_tail)
            .unwrap()
            .owner_class
            .get(),
        22
    );
    let unproven = crate::test_support::with_decode_context(|ctx| {
        super::all_field_definitions(ctx, &bytes, 0, bytes.len(), None)
    })
    .unwrap();
    assert_eq!(unproven.len(), 2);
    assert_eq!(
        unproven[0].registry_tail,
        &bytes[1 + b"m_displayMarker".len()..next_start]
    );
    assert_eq!(fields[2].offset, next_start);
    assert_eq!(fields[2].registry_tail, &[0x01]);
}

#[test]
fn plain_registry_lane_requires_complete_frames_and_owner_joins() {
    let mut bytes = Vec::new();
    append_declaration(&mut bytes, b"arbitrary_plain_name", &[0x80, 0xcf, 0x16]);
    append_declaration(&mut bytes, b"another_name", &[0xa0, 0x27, 0x10, 0x80, 0xa5]);
    crate::test_support::with_decode_context(|ctx| {
        assert!(
            super::following_field_definitions(ctx, &[1, 1, 1], 0, 3, 206)
                .unwrap()
                .is_none()
        );
        let fields = super::following_field_definitions(ctx, &bytes, 0, bytes.len(), 206)
            .unwrap()
            .unwrap();
        assert_eq!(fields.len(), 2);
        assert_eq!(fields[0].name, "arbitrary_plain_name");
        assert_eq!(fields[1].name, "another_name");
        assert!(
            super::following_field_definitions(ctx, &bytes, 0, bytes.len(), 165)
                .unwrap()
                .is_none()
        );
        assert!(
            super::following_field_definitions(ctx, &bytes, 0, bytes.len() - 1, 206)
                .unwrap()
                .is_none()
        );
        let mut invalid = bytes.clone();
        invalid[0] -= 1;
        assert!(
            super::following_field_definitions(ctx, &invalid, 0, invalid.len(), 206)
                .unwrap()
                .is_none()
        );
        invalid = bytes.clone();
        let first_owner = 1 + b"arbitrary_plain_name".len() + 2;
        invalid[first_owner] = 0;
        assert!(
            super::following_field_definitions(ctx, &invalid, 0, invalid.len(), 206)
                .unwrap()
                .is_none()
        );
    });
}

#[test]
fn failed_plain_split_preserves_the_original_bounded_bytes() {
    let mut bytes = Vec::new();
    append_declaration(&mut bytes, b"m_first", &[0x01, 0x01]);
    append_declaration(&mut bytes, b"unowned_name", &[0x80, 0xcf, 0x03]);
    let next = bytes.len();
    append_declaration(&mut bytes, b"m_next", &[0x01, 0x01]);
    let fields = crate::test_support::with_decode_context(|ctx| {
        super::all_field_definitions(ctx, &bytes, 0, bytes.len(), std::num::NonZeroUsize::new(2))
    })
    .unwrap();
    assert_eq!(fields.len(), 2);
    assert_eq!(fields[0].registry_tail, &bytes[1 + b"m_first".len()..next]);
    assert_eq!(fields[1].name, "m_next");
}

#[test]
fn reference_run_keeps_raw_codes_separate_from_ordinals() {
    let mut bytes = vec![0x10, 0x20, 0x30];
    append_declaration(&mut bytes, b"UGS::OM::Meta", &[0x02]);
    append_declaration(&mut bytes, b"UGS::MATRIX", &[0x07]);
    append_declaration(
        &mut bytes,
        b"UGS::Attr::DAttributeDescriptionDepository",
        &[0x05],
    );
    bytes.push(0x01);
    crate::test_support::with_decode_context(|ctx| {
        let references = super::reference_declarations(ctx, &bytes).unwrap().unwrap();
        assert_eq!(references.len(), 3);
        assert_eq!(references[0].offset, 3);
        assert_eq!(references[0].name, "UGS::OM::Meta");
        assert_eq!(references[0].code, 2);
        assert_eq!(references[1].name, "UGS::MATRIX");
        assert_eq!(references[1].code, 7);
        assert_eq!(references[2].code, 5);
        bytes.pop();
        assert!(super::reference_declarations(ctx, &bytes)
            .unwrap()
            .is_none());
        assert!(super::reference_declarations(ctx, &[]).unwrap().is_none());
        assert!(super::reference_declarations(ctx, &[1]).unwrap().is_none());
    });
}
