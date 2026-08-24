// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use std::collections::BTreeMap;
use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeOptions};

use crate::global::Dialect;
use crate::loss::IgesLossCode;
use crate::test_support::*;
use crate::IgesCodec;

use super::{
    general_note_font_valid_for_dialect, mirror_flag_valid, standard_color,
    vertical_text_flag_valid,
};

const GLOBAL_V4: &[u8] =
    b"1H,,1H;,7Hproduct,8Hpart.igs,7Hcadmpeg,3H0.1,32,38,6,308,15,7Hproduct,1.0,2,2HMM,1,1.0,13H260714.000000,0.001,1000.0,6Hauthor,3Horg,6,0;";
const GLOBAL_V5_0: &[u8] =
    b"1H,,1H;,7Hproduct,8Hpart.igs,7Hcadmpeg,3H0.1,32,38,6,308,15,0H,1.0,2,2HMM,1,1.0,13H260714.000000,0.001,1000.0,6Hauthor,3Horg,8,0,0H;";
const GLOBAL_V5_3: &[u8] =
    b"1H,,1H;,7Hproduct,8Hpart.igs,7Hcadmpeg,3H0.1,32,38,6,308,15,0H,1.0,2,2HMM,1,1.0,15H20260714.000000,0.001,1000.0,6Hauthor,3Horg,11,0,0H,0H;";

fn text_template_entity(status: &'static str) -> OwnedTestEntity {
    OwnedTestEntity {
        entity_type: 312,
        form: 0,
        label: "TEXTTPL".into(),
        status,
        parameters: "312,4,2,1,1.5707963267948966,0,0,0,10,20,0;".into(),
    }
}

#[test]
fn text_template_directory_rules_follow_legacy_and_later_dialects() {
    let legacy_fields = |global| {
        owned_test_file_with_global_and_directory_fields(
            &[text_template_entity("00030101")],
            global,
            &[],
            &[(1, 1)],
            &[(1, 200)],
            &[(1, 1)],
            &[(1, 1)],
        )
    };
    for (global, dialect) in [(GLOBAL_V4, Dialect::V4_0), (GLOBAL_V5_0, Dialect::V5_0)] {
        let result = IgesCodec
            .decode(
                &mut Cursor::new(legacy_fields(global)),
                &DecodeOptions::default(),
            )
            .unwrap();
        assert_eq!(
            result.ir().native.namespace("iges").unwrap().arenas["text_templates"].len(),
            1
        );
        assert!(
            !result
                .report()
                .losses
                .iter()
                .any(|loss| { loss.code == IgesLossCode::DisplayDataNotProjected.kind() }),
            "legacy {dialect:?}: {:#?}",
            result.report().losses
        );
    }

    let later = IgesCodec
        .decode(
            &mut Cursor::new(owned_test_file_with_global(
                &[text_template_entity("00000200")],
                GLOBAL_V5_3,
            )),
            &DecodeOptions::default(),
        )
        .unwrap();
    assert!(!later
        .report()
        .losses
        .iter()
        .any(|loss| loss.code == IgesLossCode::DisplayDataNotProjected.kind()));

    let rejected = IgesCodec
        .decode(
            &mut Cursor::new(legacy_fields(GLOBAL_V5_3)),
            &DecodeOptions::default(),
        )
        .unwrap();
    assert!(rejected
        .report()
        .losses
        .iter()
        .any(|loss| loss.code == IgesLossCode::DisplayDataNotProjected.kind()));

    for status in ["00010200", "00000201"] {
        let rejected = IgesCodec
            .decode(
                &mut Cursor::new(owned_test_file_with_global(
                    &[text_template_entity(status)],
                    GLOBAL_V5_3,
                )),
                &DecodeOptions::default(),
            )
            .unwrap();
        assert!(
            rejected
                .report()
                .losses
                .iter()
                .any(|loss| loss.code == IgesLossCode::DisplayDataNotProjected.kind()),
            "invalid later Type 312 status {status} was projected"
        );
    }
}

#[test]
fn presentation_enumerations_match_the_iges_tables() {
    let entries = BTreeMap::new();
    for value in [
        0, 1, 2, 3, 6, 12, 13, 14, 17, 18, 19, 1001, 1002, 1003, 2001, 3001,
    ] {
        assert!(
            general_note_font_valid_for_dialect(value, &entries, Dialect::V5_3),
            "font code {value}"
        );
    }
    for value in [-1, 4, 5, 7, 1000, 3002] {
        assert!(
            !general_note_font_valid_for_dialect(value, &entries, Dialect::V5_3),
            "font code {value}"
        );
    }

    for value in 0..=2 {
        assert!(mirror_flag_valid(value));
    }
    assert!(!mirror_flag_valid(-1));
    assert!(!mirror_flag_valid(3));

    for value in 0..=1 {
        assert!(vertical_text_flag_valid(value));
    }
    assert!(!vertical_text_flag_valid(-1));
    assert!(!vertical_text_flag_valid(2));

    assert!(standard_color(1).is_some());
    assert!(standard_color(8).is_some());
    assert!(standard_color(0).is_none());
    assert!(standard_color(9).is_none());
}

#[test]
fn general_note_font_codes_follow_the_declared_dialect() {
    let entries = BTreeMap::new();

    assert!(!general_note_font_valid_for_dialect(
        2001,
        &entries,
        Dialect::V4_0
    ));
    assert!(general_note_font_valid_for_dialect(
        2001,
        &entries,
        Dialect::V5_0
    ));
    assert!(!general_note_font_valid_for_dialect(
        3001,
        &entries,
        Dialect::V5_0
    ));
    assert!(general_note_font_valid_for_dialect(
        3001,
        &entries,
        Dialect::V5_1
    ));
}

#[test]
fn color_name_placekeeper_is_4_0_only() {
    let entity = || OwnedTestEntity {
        entity_type: 314,
        form: 0,
        label: "COLOR".into(),
        status: "00000200",
        parameters: "314,20,40,60,0,0;".into(),
    };
    let global_v4 = b"1H,,1H;,7Hproduct,8Hpart.igs,7Hcadmpeg,3H0.1,32,38,6,308,15,0H,1.0,2,2HMM,1,1.0,13H260714.000000,0.001,1000.0,6Hauthor,3Horg,6,0;";
    let global_v5 = b"1H,,1H;,7Hproduct,8Hpart.igs,7Hcadmpeg,3H0.1,32,38,6,308,15,0H,1.0,2,2HMM,1,1.0,13H260714.000000,0.001,1000.0,6Hauthor,3Horg,8,0,0H;";

    let v4 = IgesCodec
        .decode(
            &mut Cursor::new(owned_test_file_with_global(&[entity()], global_v4)),
            &DecodeOptions::default(),
        )
        .unwrap();
    assert!(v4
        .ir()
        .model
        .appearances
        .iter()
        .any(|appearance| appearance.id.0 == "iges:appearance:color#D1"));
    assert!(!v4
        .report()
        .losses
        .iter()
        .any(|loss| { loss.message.contains("optional color name is not a string") }));

    let v5 = IgesCodec
        .decode(
            &mut Cursor::new(owned_test_file_with_global(&[entity()], global_v5)),
            &DecodeOptions::default(),
        )
        .unwrap();
    assert!(!v5
        .ir()
        .model
        .appearances
        .iter()
        .any(|appearance| appearance.id.0 == "iges:appearance:color#D1"));
    assert!(v5
        .report()
        .losses
        .iter()
        .any(|loss| { loss.message.contains("optional color name is not a string") }));
}

#[test]
fn color_definition_requires_definition_directory_fields() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(owned_test_file(&[OwnedTestEntity {
                entity_type: 314,
                form: 0,
                label: "COLOR".into(),
                status: "00010200",
                parameters: "314,20,40,60,6Hcustom;".into(),
            }])),
            &DecodeOptions::default(),
        )
        .unwrap();

    assert!(result.report().losses.iter().any(|loss| {
        loss.code == IgesLossCode::DisplayDataNotProjected.kind()
            && loss.message.contains("color definition Directory fields")
    }));
    assert!(!result
        .ir()
        .model
        .appearances
        .iter()
        .any(|appearance| appearance.id.0 == "iges:appearance:color#D1"));
}

#[test]
fn color_definition_ignores_nonsemantic_directory_fields() {
    let entity = || OwnedTestEntity {
        entity_type: 314,
        form: 0,
        label: "COLOR".into(),
        status: "00000200",
        parameters: "314,20,40,60,6Hcustom;".into(),
    };
    for global in [GLOBAL_V4, GLOBAL_V5_0] {
        let result = IgesCodec
            .decode(
                &mut Cursor::new(owned_test_file_with_global_and_directory_fields(
                    &[entity()],
                    global,
                    &[],
                    &[(1, 1)],
                    &[],
                    &[(1, 1)],
                    &[(1, 1)],
                )),
                &DecodeOptions::default(),
            )
            .unwrap();

        assert!(
            result
                .ir()
                .model
                .appearances
                .iter()
                .any(|appearance| appearance.id.0 == "iges:appearance:color#D1"),
            "color definition was not projected: {:#?}",
            result.report().losses
        );
        assert!(!result.report().losses.iter().any(|loss| {
            loss.code == IgesLossCode::DisplayDataNotProjected.kind()
                && loss.message.contains("color definition Directory fields")
        }));
    }
}

#[test]
fn color_definition_requires_a_standard_fallback_color() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(owned_test_file_with_directory_fields(
                &[OwnedTestEntity {
                    entity_type: 314,
                    form: 0,
                    label: "COLOR".into(),
                    status: "00000200",
                    parameters: "314,20,40,60,6Hcustom;".into(),
                }],
                &[(1, 9)],
                &[],
                &[],
                &[],
                &[],
            )),
            &DecodeOptions::default(),
        )
        .unwrap();

    assert!(result.report().losses.iter().any(|loss| {
        loss.code == IgesLossCode::DisplayDataNotProjected.kind()
            && loss.message.contains("color definition Directory fields")
    }));
    assert!(!result
        .ir()
        .model
        .appearances
        .iter()
        .any(|appearance| appearance.id.0 == "iges:appearance:color#D1"));
}

#[test]
fn text_font_definition_requires_an_independent_directory_entry() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(owned_test_file(&[OwnedTestEntity {
                entity_type: 310,
                form: 0,
                label: "FONT".into(),
                status: "00010200",
                parameters: "310,101,4HBASE,,10,1,65,8,0,0;".into(),
            }])),
            &DecodeOptions::default(),
        )
        .unwrap();

    assert!(result.report().losses.iter().any(|loss| {
        loss.code == IgesLossCode::DisplayDataNotProjected.kind()
            && loss.message.contains("font header")
    }));
}

#[test]
fn decode_applies_standard_body_color_and_face_color_override() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(colored_explicit_vertex_loop_file()),
            &DecodeOptions::default(),
        )
        .unwrap();
    let body = result
        .ir()
        .model
        .bodies
        .iter()
        .find(|body| body.id.0 == "iges:model:body#D11")
        .unwrap_or_else(|| panic!("losses={:#?}", result.report().losses));
    assert_eq!(
        body.color,
        Some(cadmpeg_ir::topology::Color {
            r: 1.0,
            g: 0.0,
            b: 0.0,
            a: 1.0,
        })
    );
    assert_eq!(body.visible, Some(true));
    let face = result
        .ir()
        .model
        .faces
        .iter()
        .find(|face| face.id.0 == "iges:model:face#D11:D9")
        .unwrap();
    assert_eq!(
        face.color,
        Some(cadmpeg_ir::topology::Color {
            r: 0.2,
            g: 0.4,
            b: 0.6,
            a: 1.0,
        })
    );
    assert!(result
        .ir()
        .model
        .appearances
        .iter()
        .any(|appearance| appearance.id.0 == "iges:appearance:color#D13"
            && appearance.name.as_deref() == Some("custom")));
    assert_eq!(result.ir().model.appearance_bindings.len(), 2);
    let native = result.ir().native.namespace("iges").unwrap();
    assert_eq!(native.version, 6);
    assert_eq!(native.arenas["colors"].len(), 1);
    assert_eq!(
        native.arenas["colors"][0].id(),
        "iges:presentation:color#D13"
    );
    assert_eq!(native.arenas["colors"][0].fields()["red_percent"], 20.0);
    assert_eq!(native.arenas["display_attributes"].len(), 7);
    assert!(
        result.report().losses.is_empty(),
        "{:#?}",
        result.report().losses
    );
    let validation = cadmpeg_ir::validate_neutral(result.ir(), Vec::new());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn decode_keeps_raw_display_pointers_when_definition_targets_do_not_resolve() {
    let bytes = owned_test_file_with_directory_fields(
        &[
            OwnedTestEntity {
                entity_type: 116,
                form: 0,
                label: "SOURCE".into(),
                status: "00000000",
                parameters: "116,1,2,3;".into(),
            },
            OwnedTestEntity {
                entity_type: 304,
                form: 99,
                label: "BADFONT".into(),
                status: "00000000",
                parameters: "304;".into(),
            },
            OwnedTestEntity {
                entity_type: 116,
                form: 0,
                label: "FILLER".into(),
                status: "00000000",
                parameters: "116,0,0,0;".into(),
            },
            OwnedTestEntity {
                entity_type: 314,
                form: 9,
                label: "BADCOLOR".into(),
                status: "00000000",
                parameters: "314,0,0,0;".into(),
            },
        ],
        &[(1, -7)],
        &[(1, -3)],
        &[(1, -99)],
        &[],
        &[],
    );
    let result = IgesCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    let display = result.ir().native.namespace("iges").unwrap().arenas["display_attributes"]
        .iter()
        .find(|record| record.id() == "iges:presentation:display-attributes#D1")
        .unwrap();
    assert_eq!(display.fields()["line_font_number"], -3);
    assert!(display.fields()["line_font_definition"].is_null());
    assert_eq!(display.fields()["level_number"], -99);
    assert!(display.fields()["level_definition"].is_null());
    assert_eq!(display.fields()["color_number"], -7);
    assert!(display.fields()["color_definition"].is_null());

    let pointer_losses = result
        .report()
        .losses
        .iter()
        .filter(|loss| loss.code == crate::loss::IgesLossCode::PointerUnresolved.kind())
        .collect::<Vec<_>>();
    assert_eq!(pointer_losses.len(), 3);
    assert!(pointer_losses.iter().all(|loss| {
        loss.provenance
            .as_ref()
            .and_then(|provenance| provenance.tag.as_deref())
            == Some("D1")
    }));
}

#[test]
fn decode_types_template_and_visible_blank_line_fonts() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(line_font_definitions_file()),
            &DecodeOptions::default(),
        )
        .unwrap();
    let native = result.ir().native.namespace("iges").unwrap();
    let line_fonts = &native.arenas["line_fonts"];
    assert_eq!(line_fonts.len(), 2);
    assert_eq!(line_fonts[0].id(), "iges:presentation:line-font#D3");
    assert_eq!(line_fonts[0].fields()["kind"], "template");
    assert_eq!(line_fonts[0].fields()["tangent_oriented"], true);
    assert_eq!(
        line_fonts[0].fields()["template"],
        "iges:entity:directory#1"
    );
    assert_eq!(line_fonts[1].fields()["kind"], "visible_blank_pattern");
    assert_eq!(line_fonts[1].fields()["segment_count"], 5);
    assert_eq!(
        line_fonts[1].fields()["hexadecimal_pattern"]
            .as_array()
            .unwrap()
            .iter()
            .map(|value| value.as_u64().unwrap())
            .collect::<Vec<_>>(),
        vec![49, 54]
    );
    let line_display = native.arenas["display_attributes"]
        .iter()
        .find(|record| record.id() == "iges:presentation:display-attributes#D7")
        .unwrap();
    assert_eq!(line_display.fields()["line_font_number"], -5);
    assert_eq!(
        line_display.fields()["line_font_definition"],
        "iges:presentation:line-font#D5"
    );
    assert!(
        result.report().losses.is_empty(),
        "{:#?}",
        result.report().losses
    );
}

#[test]
fn line_font_definition_requires_its_definition_directory_fields() {
    let cases = [
        ("subordinate", "00010200", &[][..], &[][..], &[][..]),
        ("structure", "00000200", &[(1, 7)][..], &[][..], &[][..]),
        ("line weight", "00000200", &[][..], &[(1, 1)][..], &[][..]),
        ("color", "00000200", &[][..], &[][..], &[(1, 1)][..]),
    ];

    for (name, status, structures, line_weights, colors) in cases {
        let result = IgesCodec
            .decode(
                &mut Cursor::new(owned_test_file_with_directory_fields(
                    &[OwnedTestEntity {
                        entity_type: 304,
                        form: 2,
                        label: "PATTERN".into(),
                        status,
                        parameters: "304,2,1,2,1H3;".into(),
                    }],
                    colors,
                    &[(1, 1)],
                    &[],
                    line_weights,
                    structures,
                )),
                &DecodeOptions::default(),
            )
            .unwrap();

        assert!(
            result.report().losses.iter().any(|loss| {
                loss.code == IgesLossCode::DisplayDataNotProjected.kind()
                    && loss.message.contains("line-font definition")
            }),
            "invalid {name} was projected: {:#?}",
            result.report().losses
        );
    }
}

#[test]
fn decode_rejects_out_of_table_text_template_font() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(out_of_table_text_template_font_file()),
            &DecodeOptions::default(),
        )
        .unwrap();

    assert!(result
        .report()
        .losses
        .iter()
        .any(|loss| loss.code == crate::loss::IgesLossCode::DisplayDataNotProjected.kind()));
}

#[test]
fn decode_types_definition_levels_and_directory_level_links() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(definition_levels_file()),
            &DecodeOptions::default(),
        )
        .unwrap();
    let native = result.ir().native.namespace("iges").unwrap();
    let levels = &native.arenas["definition_levels"];
    assert_eq!(levels.len(), 1);
    assert_eq!(levels[0].id(), "iges:presentation:definition-levels#D1");
    assert_eq!(levels[0].fields()["declared_count"], 3);
    assert_eq!(
        levels[0].fields()["levels"]
            .as_array()
            .unwrap()
            .iter()
            .map(|value| value.as_i64().unwrap())
            .collect::<Vec<_>>(),
        vec![2, 7, 11]
    );
    let line = native.arenas["display_attributes"]
        .iter()
        .find(|record| record.id() == "iges:presentation:display-attributes#D3")
        .unwrap();
    assert_eq!(line.fields()["level_number"], -1);
    assert_eq!(
        line.fields()["level_definition"],
        "iges:presentation:definition-levels#D1"
    );
    assert!(
        result.report().losses.is_empty(),
        "{:#?}",
        result.report().losses
    );
}

#[test]
fn decode_resolves_directory_line_weight_to_millimetres() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(weighted_line_file()),
            &DecodeOptions::default(),
        )
        .unwrap();
    let display = &result.ir().native.namespace("iges").unwrap().arenas["display_attributes"][0];
    assert_eq!(display.fields()["line_weight_number"], 1);
    assert_eq!(display.fields()["line_weight_mm"], 1.0);
    assert!(
        result.report().losses.is_empty(),
        "{:#?}",
        result.report().losses
    );
}

#[test]
fn decode_accepts_v5_relative_directory_line_weights() {
    let entities = [OwnedTestEntity {
        entity_type: 110,
        form: 0,
        label: "LINE".into(),
        status: "00000000",
        parameters: "110,0,0,0,1,0,0;".into(),
    }];
    let global = b"1H,,1H;,7Hproduct,8Hpart.igs,7Hcadmpeg,3H0.1,32,38,6,308,15,0H,1.0,2,2HMM,3,0,13H260714.000000,0.001,1000.0,6Hauthor,3Horg,8,0,0H;";
    let result = IgesCodec
        .decode(
            &mut Cursor::new(owned_test_file_with_global_and_line_weights(
                &entities,
                global,
                &[(1, 3)],
            )),
            &DecodeOptions::default(),
        )
        .unwrap();

    let display = &result.ir().native.namespace("iges").unwrap().arenas["display_attributes"][0];
    assert_eq!(display.fields()["line_weight_number"], 3);
    assert_eq!(display.fields()["line_weight_mm"], serde_json::Value::Null);
    assert!(!result.report().losses.iter().any(|loss| {
        loss.code == crate::loss::IgesLossCode::LineWeightScaleUnavailable.kind()
            || loss.code == crate::loss::IgesLossCode::DisplayDataNotProjected.kind()
    }));
}

#[test]
fn decode_distinguishes_absolute_and_incremental_text_templates() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(text_display_template_forms_file()),
            &DecodeOptions::default(),
        )
        .unwrap();
    let templates = &result.ir().native.namespace("iges").unwrap().arenas["text_templates"];
    assert_eq!(templates.len(), 2);
    let absolute = templates
        .iter()
        .find(|template| template.fields()["form"] == 0)
        .unwrap();
    assert_eq!(absolute.fields()["origin_or_increment"][0], 10.0);
    assert_eq!(absolute.fields()["origin_or_increment"][1], 20.0);
    let incremental = templates
        .iter()
        .find(|template| template.fields()["form"] == 1)
        .unwrap();
    assert_eq!(incremental.fields()["font_code"], 18);
    assert_eq!(incremental.fields()["mirror"], 1);
    assert_eq!(incremental.fields()["vertical"], 1);
    assert_eq!(incremental.fields()["origin_or_increment"][0], 2.0);
    assert_eq!(incremental.fields()["origin_or_increment"][1], -1.0);
    assert!(
        result.report().losses.is_empty(),
        "{:#?}",
        result.report().losses
    );
}

#[test]
fn decode_preserves_text_font_glyphs_and_supersession() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(text_font_definition_file()),
            &DecodeOptions::default(),
        )
        .unwrap();
    let fonts = &result.ir().native.namespace("iges").unwrap().arenas["text_fonts"];
    assert_eq!(fonts.len(), 2);
    let base = fonts
        .iter()
        .find(|font| font.fields()["font_code"] == 101)
        .unwrap();
    assert_eq!(base.fields()["characters"].as_array().unwrap().len(), 2);
    assert_eq!(base.fields()["characters"][0]["character_code"], 65);
    assert_eq!(
        base.fields()["characters"][0]["motions"]
            .as_array()
            .unwrap()
            .len(),
        3
    );
    assert!(base.fields()["characters"][0]["motions"][0]["pen_up"].is_null());
    assert_eq!(
        base.fields()["characters"][0]["motions"][1]["pen_up"],
        false
    );
    assert_eq!(base.fields()["characters"][1]["declared_motion_count"], 0);
    let modification = fonts
        .iter()
        .find(|font| font.fields()["font_code"] == 102)
        .unwrap();
    assert_eq!(
        modification.fields()["supersedes_definition"],
        "iges:presentation:text-font#D1"
    );
    assert_eq!(
        modification.fields()["characters"][0]["motions"][0]["pen_up"],
        true
    );
    let template = &result.ir().native.namespace("iges").unwrap().arenas["text_templates"][0];
    assert_eq!(
        template.fields()["font_definition"],
        "iges:presentation:text-font#D3"
    );
    assert!(
        result.report().losses.is_empty(),
        "{:#?}",
        result.report().losses
    );
}
