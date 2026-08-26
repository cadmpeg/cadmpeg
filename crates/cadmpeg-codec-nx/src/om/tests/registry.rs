use super::{FieldDefinition, TypeDefinition};
use crate::om::registry::RegistryTokenForm;

#[test]
fn decodes_direct_class_registry_tail() {
    let fingerprint = [0x10, 0x20, 0x30, 0x40, 0x50, 0x60, 0x70, 0x80];
    let definition = TypeDefinition {
        offset: 0,
        name: "UGS::OM::RootObject",
        trailing_code: 0x38,
        registry_suffix: &[0x05, 0x10, 0x20, 0x30, 0x40, 0x50, 0x60, 0x70, 0x80, 0x02],
    };

    let layout = definition.class_registry_layout().unwrap();

    assert_eq!(layout.storage_code.value, 0x38);
    assert_eq!(layout.storage_code.form, RegistryTokenForm::Direct);
    assert_eq!(layout.storage_code.width, 1);
    assert_eq!(layout.base_class, 0x05);
    assert_eq!(layout.schema_fingerprint, fingerprint);
    assert_eq!(layout.reference, 0x02);
}

#[test]
fn decodes_compact_and_wide_class_registry_tokens() {
    let fingerprint = [0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0xff, 0x01, 0x23];
    let compact = TypeDefinition {
        offset: 0,
        name: "UGS::OM::SaveAuditTrail",
        trailing_code: 0x80,
        registry_suffix: &[
            0xc9, 0x09, 0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0xff, 0x01, 0x23, 0x01,
        ],
    };

    let compact_layout = compact.class_registry_layout().unwrap();
    assert_eq!(compact_layout.storage_code.value, 0xc9 + 1);
    assert_eq!(compact_layout.storage_code.form, RegistryTokenForm::Compact);
    assert_eq!(compact_layout.storage_code.width, 2);
    assert_eq!(compact_layout.base_class, 0x09);
    assert_eq!(compact_layout.schema_fingerprint, fingerprint);
    assert_eq!(compact_layout.reference, 0x01);

    let wide = TypeDefinition {
        offset: 0,
        name: "UGS::Part::Unit::ProxySystemMeasure",
        trailing_code: 0xa0,
        registry_suffix: &[
            0x27, 0x10, 0x82, 0x3c, 0x12, 0x34, 0x56, 0x78, 0x9a, 0xbc, 0xde, 0xf0, 0x80, 0xd3,
        ],
    };

    let wide_layout = wide.class_registry_layout().unwrap();
    assert_eq!(wide_layout.storage_code.value, 0x27_10 + 1);
    assert_eq!(wide_layout.storage_code.form, RegistryTokenForm::Wide);
    assert_eq!(wide_layout.storage_code.width, 3);
    assert_eq!(wide_layout.base_class, 0x02_3c + 1);
    assert_eq!(
        wide_layout.schema_fingerprint,
        [0x12, 0x34, 0x56, 0x78, 0x9a, 0xbc, 0xde, 0xf0]
    );
    assert_eq!(wide_layout.reference, 0xd3 + 1);
}

#[test]
fn decodes_each_extended_registry_prefix_family() {
    let compact = crate::om::registry::registry_token_at(0x8f, &[0xfe], 0).unwrap();
    assert_eq!(compact.value, 0x0f00 + 0xfe + 1);
    assert_eq!(compact.form, RegistryTokenForm::Compact);
    assert_eq!(compact.width, 2);

    let wide_90 = crate::om::registry::registry_token_at(0x90, &[0x12, 0x34], 0).unwrap();
    assert_eq!(wide_90.value, 0x1234 + 1);
    assert_eq!(wide_90.form, RegistryTokenForm::Wide);
    assert_eq!(wide_90.width, 3);

    let wide_a1 = crate::om::registry::registry_token_at(0xa1, &[0x12, 0x34], 0).unwrap();
    assert_eq!(wide_a1.value, 0x1_1234 + 1);
    assert_eq!(wide_a1.form, RegistryTokenForm::Wide);

    let wide_f1 = crate::om::registry::registry_token_at(0xf1, &[0x12, 0x34], 0).unwrap();
    assert_eq!(wide_f1.value, 0x1_1234 + 1);
    assert_eq!(wide_f1.form, RegistryTokenForm::Wide);
}

#[test]
fn decodes_direct_and_compact_field_registry_heads() {
    let direct = FieldDefinition {
        offset: 0,
        name: "m_objectStateCollection",
        trailing_code: 0x78,
        registry_suffix: &[0x28, 0x12, 0x34],
    };
    let direct_layout = direct.field_registry_layout().unwrap();
    assert_eq!(direct_layout.storage_code.value, 0x78);
    assert_eq!(direct_layout.storage_code.form, RegistryTokenForm::Direct);
    assert_eq!(direct_layout.owner_class, 0x28);

    let compact = FieldDefinition {
        offset: 0,
        name: "first_record_area",
        trailing_code: 0x80,
        registry_suffix: &[0xcf, 0x28],
    };
    let compact_layout = compact.field_registry_layout().unwrap();
    assert_eq!(compact_layout.storage_code.value, 0xcf + 1);
    assert_eq!(compact_layout.storage_code.form, RegistryTokenForm::Compact);
    assert_eq!(compact_layout.owner_class, 0x28);
}

#[test]
fn rejects_incomplete_or_null_registry_tails() {
    let truncated = TypeDefinition {
        offset: 0,
        name: "UGS::OM::Truncated",
        trailing_code: 0x38,
        registry_suffix: &[0x05, 0x00, 0x01],
    };
    assert!(truncated.class_registry_layout().is_none());

    let null_storage = FieldDefinition {
        offset: 0,
        name: "m_null",
        trailing_code: 0xff,
        registry_suffix: &[],
    };
    assert!(null_storage.field_registry_layout().is_none());

    let null_owner = FieldDefinition {
        offset: 0,
        name: "m_null_owner",
        trailing_code: 0x78,
        registry_suffix: &[0x00],
    };
    assert!(null_owner.field_registry_layout().is_none());
}
