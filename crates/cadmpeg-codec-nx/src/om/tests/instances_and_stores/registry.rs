// SPDX-License-Identifier: Apache-2.0

use super::*;

#[test]
fn om_color_table_requires_complete_names_indices_and_rgb_atoms() {
    let mut bytes = vec![0x02, 0x80, 0xd9, 0x01];
    for ordinal in 0..=216 {
        let name = if ordinal == 0 {
            "Background".to_string()
        } else {
            format!("Color {ordinal}")
        };
        bytes.push(u8::try_from(name.len() + 2).unwrap());
        bytes.extend_from_slice(name.as_bytes());
        bytes.push(0);
    }
    bytes.extend_from_slice(&[
        0x02, 0x14, 0xff, 0x06, 0x00, 0xf0, 0x02, 0x80, 0x9d, 0x80, 0xc7, 0x00, 0xc0, 0x13, 0x0a,
        0xc6, 0x01, 0x80, 0xd9, 0x80, 0xc8, 0x01, 0x01, 0x01,
    ]);
    for color_index in 1u16..=216 {
        bytes.push(0x05);
        if color_index < 128 {
            bytes.push(color_index as u8);
        } else {
            bytes.extend_from_slice(&[0x80, (color_index - 1) as u8]);
        }
        bytes.extend_from_slice(&[0x01, 0x80, 0xc8]);
        if color_index == 2 {
            bytes.extend_from_slice(&shifted_f64_bytes(2.0));
            let mut binary32 = 1.0_f32.to_be_bytes();
            binary32[0] += 0x10;
            bytes.extend_from_slice(&binary32);
            bytes.push(0x00);
        } else {
            bytes.extend_from_slice(&[0x01, 0x01, 0x01]);
        }
    }

    let tables = super::super::color_tables(&bytes);
    assert_eq!(tables.len(), 1);
    assert_eq!(
        tables[0].background.map(|(component, _)| component.value()),
        [1.0, 1.0, 1.0]
    );
    assert_eq!(tables[0].definitions.len(), 216);
    assert_eq!(tables[0].definitions[0].name, "Color 1");
    assert_eq!(
        tables[0].definitions[0]
            .components
            .map(|(component, _)| component.value()),
        [1.0, 1.0, 1.0]
    );
    assert_eq!(
        tables[0].definitions[1]
            .components
            .map(|(component, _)| component.value()),
        [0.5, 0.25, 0.0]
    );
    let index_offset = tables[0].definitions[127].offset + 1;
    assert_eq!(bytes[index_offset..index_offset + 2], [0x80, 0x7f]);

    let mut wrong_background = bytes.clone();
    wrong_background[5] = b'b';
    assert!(super::super::color_tables(&wrong_background).is_empty());

    let mut malformed = bytes.clone();
    *malformed.last_mut().unwrap() = 0x02;
    assert!(super::super::color_tables(&malformed).is_empty());
    let truncated = &bytes[..bytes.len() - 1];
    assert!(super::super::color_tables(truncated).is_empty());
}

#[test]
fn om_registry_uses_length_framing_and_stays_outside_entity_payloads() {
    let mut bytes = indexed_om_section();
    bytes.extend_from_slice(b"\x10UGS::PayloadText");
    let sections = super::super::indexed_sections(&bytes);
    assert_eq!(sections.len(), 1);
    assert_eq!(sections[0].types.len(), 1);
    assert_eq!(sections[0].types[0].name, "UGS::EXP_expression");
    assert_eq!(sections[0].types[0].registry_tail[0], 0x81);
    assert_eq!(sections[0].types[0].offset, 8);
}

#[test]
fn om_numeric_expression_retains_identity_name_unit_and_value() {
    let bytes = indexed_om_section();
    let section = super::super::indexed_sections(&bytes).remove(0);
    let expression_records = section.numeric_expression_records();
    assert_eq!(expression_records[0].0, 1);
    let expressions = expression_records
        .iter()
        .map(|(_, expression)| expression)
        .collect::<Vec<_>>();
    assert_eq!(expressions.len(), 1);
    assert_eq!(expressions[0].object_id, Some(0x102));
    assert_eq!(
        expressions[0].name.as_str(),
        "p8_CircularPattern_pattern_Circular_Dir_offset_angle"
    );
    assert_eq!(expressions[0].name.index(), Some(8));
    assert_eq!(
        expressions[0].name.qualifier(),
        Some("CircularPattern_pattern_Circular_Dir_offset_angle")
    );
    assert_eq!(expressions[0].unit, super::super::ExpressionUnit::Degree);
    assert_eq!(expressions[0].expression, "120");
    assert_eq!(expressions[0].value, Some(120.0));
    let declaration = super::super::expression_declaration_name(
        section.as_fixed().expect("fixed store")[1].bytes,
    )
    .unwrap();
    assert_eq!(
        declaration.name.as_str(),
        "p8_CircularPattern_pattern_Circular_Dir_offset_angle"
    );
    assert_eq!(declaration.name.index(), 8);
    assert_eq!(
        declaration.name.qualifier(),
        Some("CircularPattern_pattern_Circular_Dir_offset_angle")
    );
    assert_eq!(declaration.literal, Some("120"));
    let declaration =
        super::super::expression_declaration_name(b"\x04\x04p1\0\x04\x0a-5.1 * 2\0").unwrap();
    assert_eq!(declaration.name.as_str(), "p1");
    assert_eq!(declaration.literal, Some("-5.1 * 2"));
    let declaration =
        super::super::expression_declaration_name(b"\x04\x04p1\0\x04\x055.1\0\x04\x05120\0")
            .unwrap();
    assert_eq!(declaration.literal, None);
    assert!(super::super::expression_declaration_name(b"\x04\x04p1\0\x04\x04p2\0").is_none());
    assert!(super::super::expression_declaration_name(b"\x04\x05p1-\0").is_none());
}
