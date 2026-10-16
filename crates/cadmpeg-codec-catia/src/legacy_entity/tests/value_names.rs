// SPDX-License-Identifier: Apache-2.0
//! Uniqueness and byte framing of legacy value names.

use crate::legacy_entity::{
    LegacyRoleName, LegacyRoleSelector, LegacyRoleSelectorEncoding, LegacyTextEncoding,
    LegacyTextField, LegacyValueNames,
};

fn role(offset: usize, entity_id: u32, field_code: u16) -> LegacyRoleSelector {
    LegacyRoleSelector {
        offset,
        entity_id,
        name: LegacyRoleName::Selector(1),
        encoding: LegacyRoleSelectorEncoding::Paged,
        selector: 1,
        field_code: Some(field_code),
    }
}

fn field(offset: usize, entity_id: u32, value: &str) -> LegacyTextField {
    LegacyTextField {
        offset,
        entity_id,
        encoding: LegacyTextEncoding::U8InclusiveLength,
        role: Some(role(offset, entity_id, 0x1200)),
        value: value.to_owned(),
    }
}

#[test]
fn evaluated_names_require_matching_bytes_and_unique_roles_and_identifiers() {
    let mut data = [0; 250];
    data[103..109].copy_from_slice(b"\xe8\xc4\x17\x01\xfe\xfe");
    let roles = [role(100, 7, 0x17c4), role(200, 8, 0x17c4)];
    let fields = [
        field(150, 7, "After"),
        field(100, 7, "AtRole"),
        field(50, 7, "Width"),
        field(40, 8, "OtherEntity"),
        field(30, 7, "not an identifier"),
    ];
    let names = LegacyValueNames::new(&data, &roles, &fields);
    assert_eq!(names.get(7, 109), Some(&fields[2]));
    assert_eq!(names.get(8, 209), None);
    assert_eq!(names.get(7, 110), None);
    assert_eq!(names.get(8, 109), None);

    let duplicates = [roles[0].clone(), roles[0].clone()];
    assert_eq!(
        LegacyValueNames::new(&data, &duplicates, &fields).get(7, 109),
        None
    );
    let two_names = [fields[2].clone(), field(60, 7, "Depth")];
    assert_eq!(
        LegacyValueNames::new(&data, &roles, &two_names).get(7, 109),
        None
    );
    assert_eq!(LegacyValueNames::new(&data, &roles, &[]).get(7, 109), None);
}

#[test]
fn literal_name_precedes_evaluated_name_and_duplicates_prevent_fallback() {
    let mut data = [0; 109];
    data[103..109].copy_from_slice(b"\xe8\xc4\x17\x01\xfe\xfe");
    let roles = [role(100, 7, 0x17c4)];
    let mut fields = [field(50, 7, "Width"), field(150, 7, "Length.")];
    fields[1].role.as_mut().unwrap().name = LegacyRoleName::Literal("name".to_owned());
    let names = LegacyValueNames::new(&data, &roles, &fields);
    assert_eq!(names.get(7, 109), Some(&fields[1]));
    assert_eq!(names.get(7, 999), Some(&fields[1]));
    let duplicate = fields[1].clone();
    let fields = [fields[0].clone(), fields[1].clone(), duplicate];
    assert_eq!(
        LegacyValueNames::new(&data, &roles, &fields).get(7, 109),
        None
    );
}
