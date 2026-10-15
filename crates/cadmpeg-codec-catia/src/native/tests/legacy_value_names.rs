// SPDX-License-Identifier: Apache-2.0
//! Evaluated legacy value names keyed by entity and value offset.

use crate::legacy_entity::LegacyRoleName;
use crate::native::{
    CatiaLegacyRoleSelector, CatiaLegacyRoleSelectorEncoding, CatiaLegacyTextEncoding,
    CatiaLegacyTextField, LegacyEvaluatedValueNames,
};

fn role(byte_offset: u64, entity_id: u32, field_code: u16) -> CatiaLegacyRoleSelector {
    CatiaLegacyRoleSelector {
        byte_offset,
        entity_id,
        name: LegacyRoleName::Selector(1),
        encoding: CatiaLegacyRoleSelectorEncoding::Paged,
        selector: 1,
        field_code: Some(field_code),
    }
}

fn field(byte_offset: u64, entity_id: u32, value: &str) -> CatiaLegacyTextField {
    CatiaLegacyTextField {
        byte_offset,
        entity_id,
        encoding: CatiaLegacyTextEncoding::U8InclusiveLength,
        role: Some(role(byte_offset, entity_id, 0x1200)),
        value: value.to_owned(),
    }
}

// A paged role with a selector name ends three bytes after its offset; the
// evaluated value starts six bytes after that.
const VALUE_AFTER_ROLE: u64 = 9;

#[test]
fn evaluated_value_name_is_the_only_identifier_before_the_evaluation_role() {
    let roles = [role(100, 7, 0x17c4), role(200, 8, 0x17c4)];
    let fields = [
        field(150, 7, "After"),
        field(50, 7, "Width"),
        field(40, 8, "Height"),
        field(60, 8, "Depth"),
        field(70, 9, "Other"),
        field(80, 7, "not an identifier"),
    ];
    let names = LegacyEvaluatedValueNames::new(&roles, &fields);

    let name = names.get(7, 100 + VALUE_AFTER_ROLE).unwrap();
    assert_eq!((name.byte_offset, name.value.as_str()), (50, "Width"));
    assert!(names.get(7, 100 + VALUE_AFTER_ROLE + 1).is_none());
    assert!(names.get(8, 200 + VALUE_AFTER_ROLE).is_none());
    assert!(names.get(9, 100 + VALUE_AFTER_ROLE).is_none());
}

#[test]
fn evaluated_value_name_requires_one_evaluation_role() {
    let roles = [
        role(100, 7, 0x17c4),
        role(100, 7, 0x17c4),
        role(300, 7, 0x1200),
    ];
    let fields = [field(50, 7, "Width")];
    let names = LegacyEvaluatedValueNames::new(&roles, &fields);

    assert!(names.get(7, 100 + VALUE_AFTER_ROLE).is_none());
    assert!(names.get(7, 300 + VALUE_AFTER_ROLE).is_none());
}
