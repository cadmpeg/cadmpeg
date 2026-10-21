// SPDX-License-Identifier: Apache-2.0

use super::*;
use cadmpeg_ir::ids::Identity;
use serde_json::json;
use std::collections::BTreeMap;

fn install(ir: &mut CadIr, arena: &str, id: &str, fields: Value) {
    ir.native
        .namespace_mut("iges")
        .arenas_mut()
        .entry(arena.into())
        .or_default()
        .push(
            NativeRecord::new(
                Identity::new(id).unwrap(),
                fields.as_object().unwrap().clone(),
            )
            .unwrap(),
        );
}

fn owner() -> Entity {
    let mut owner = entity(
        110,
        0,
        vec![
            "0".into(),
            "0".into(),
            "0".into(),
            "1".into(),
            "0".into(),
            "0".into(),
        ],
    );
    owner.source_targets.push("iges:entity:directory#1".into());
    owner
}

#[test]
fn directory_pattern_number_is_written_from_typed_display() {
    let mut ir = CadIr::empty();
    install(
        &mut ir,
        "display_attributes",
        "iges:presentation:display#D1",
        json!({
            "source_entity": "iges:entity:directory#1", "line_font_number": 3,
        }),
    );
    let mut entities = vec![owner()];
    let exported = append(&ir, &mut entities, crate::IgesVersion::V5_3);
    assert!(exported.losses.is_empty());
    assert!(matches!(
        entities[0].line_font,
        Some(DirectoryLineFont::Number(3))
    ));
    let bytes = encode_file(
        &entities,
        &BTreeMap::new(),
        crate::IgesVersion::V5_3,
        generated_minimum_resolution(&ir).unwrap(),
    )
    .unwrap();
    let directory = bytes
        .split(|byte| *byte == b'\n')
        .find(|line| line.get(72) == Some(&b'D'))
        .unwrap();
    assert_eq!(&directory[24..32], b"       3");
}

#[test]
fn reemitted_font_definition_uses_current_directory_pointer() {
    let mut ir = CadIr::empty();
    let font = "iges:presentation:line-font#D99";
    install(
        &mut ir,
        "line_fonts",
        font,
        json!({
            "source_entity":"iges:entity:directory#99", "kind":"visible_blank_pattern",
            "fallback_line_font_number": 1, "segment_count": 2, "lengths": [2.0, 1.0],
            "hexadecimal_pattern": "A",
        }),
    );
    install(
        &mut ir,
        "display_attributes",
        "iges:presentation:display#D1",
        json!({
            "source_entity":"iges:entity:directory#1", "line_font_number": -99,
            "line_font_definition": font,
        }),
    );
    let mut entities = vec![owner()];
    let exported = append(&ir, &mut entities, crate::IgesVersion::V5_3);
    assert!(exported.losses.is_empty());
    assert!(exported.regenerated.contains(font));
    assert_eq!(entities.len(), 2);
    assert_eq!(entities[1].type_code, 304);
    assert!(matches!(
        entities[1].line_font,
        Some(DirectoryLineFont::Number(1))
    ));
    assert!(matches!(
        entities[0].line_font,
        Some(DirectoryLineFont::Definition(1))
    ));
    let bytes = encode_file(
        &entities,
        &BTreeMap::new(),
        crate::IgesVersion::V5_3,
        generated_minimum_resolution(&ir).unwrap(),
    )
    .unwrap();
    let directory = bytes
        .split(|byte| *byte == b'\n')
        .find(|line| line.get(72) == Some(&b'D'))
        .unwrap();
    assert_eq!(&directory[24..32], b"      -3");
}
