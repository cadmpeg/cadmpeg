// SPDX-License-Identifier: Apache-2.0

#![allow(clippy::unwrap_used)]

use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeOptions};

use crate::directory::{DirectoryEntry, Status};
use crate::global::Dialect;
use crate::loss::IgesLossCode;
use crate::test_support::*;
use crate::IgesCodec;

use super::{
    clipping_plane_valid, depth_clipping_valid, display_flag_valid, drawing_directory_valid,
    has_in_plane_component, standard_color_valid, standard_line_font_valid, view_directory_valid,
    views_visible_directory_valid,
};

fn directory_entry(entity_type: i64, form: i64) -> DirectoryEntry {
    DirectoryEntry {
        source_offset: 0,
        sequence: 1,
        entity_type,
        parameter_start: 0,
        structure: 0,
        line_font: 0,
        level: 0,
        view: 0,
        transform: 0,
        label_display: 0,
        status: Status {
            blank: 0,
            subordinate: 0,
            use_flag: 1,
            hierarchy: 0,
        },
        line_weight: 0,
        color: 0,
        parameter_line_count: 0,
        form,
        reserved: [[b' '; 8]; 2],
        label: [b' '; 8],
        subscript: 0,
    }
}

#[test]
fn drawing_presentation_directory_rules_match_the_iges_tables() {
    let mut drawing = directory_entry(404, 0);
    assert!(drawing_directory_valid(&drawing));
    drawing.status.subordinate = 1;
    assert!(!drawing_directory_valid(&drawing));
    drawing.status.subordinate = 0;
    drawing.status.use_flag = 2;
    assert!(!drawing_directory_valid(&drawing));
    drawing.status.use_flag = 1;
    drawing.status.blank = 1;
    drawing.status.hierarchy = 3;
    assert!(drawing_directory_valid(&drawing));

    for field in 0..4 {
        let mut candidate = directory_entry(404, 1);
        match field {
            0 => candidate.structure = 1,
            1 => candidate.line_font = 1,
            2 => candidate.line_weight = 1,
            _ => candidate.color = 1,
        }
        assert!(!drawing_directory_valid(&candidate));
    }

    let mut view = directory_entry(410, 0);
    assert!(view_directory_valid(&view, Dialect::V4_0));
    view.status.subordinate = 2;
    assert!(view_directory_valid(&view, Dialect::V4_0));
    view.status.subordinate = 0;
    view.status.use_flag = 2;
    assert!(view_directory_valid(&view, Dialect::V4_0));
    assert!(!view_directory_valid(&view, Dialect::V5_0));
    assert!(!view_directory_valid(&view, Dialect::V5_3));
    view.status.use_flag = 1;
    view.status.blank = 1;
    view.status.hierarchy = 3;
    assert!(view_directory_valid(&view, Dialect::V4_0));
    assert!(view_directory_valid(&view, Dialect::V5_0));
    assert!(view_directory_valid(&view, Dialect::V5_3));
    for field in 0..4 {
        let mut candidate = directory_entry(410, 1);
        match field {
            0 => candidate.structure = 1,
            1 => candidate.line_font = 1,
            2 => candidate.line_weight = 1,
            _ => candidate.color = 1,
        }
        assert!(view_directory_valid(&candidate, Dialect::V4_0));
        assert!(view_directory_valid(&candidate, Dialect::V5_3));
    }
    view.level = 2;
    view.view = 3;
    view.label_display = 5;
    assert!(view_directory_valid(&view, Dialect::V4_0));
    assert!(view_directory_valid(&view, Dialect::V5_3));

    for form in [3, 4, 19] {
        let mut visible = directory_entry(402, form);
        assert!(views_visible_directory_valid(&visible, Dialect::V4_0));
        assert!(views_visible_directory_valid(&visible, Dialect::V5_0));
        visible.status.subordinate = 1;
        assert!(!views_visible_directory_valid(&visible, Dialect::V4_0));
        assert!(!views_visible_directory_valid(&visible, Dialect::V5_0));
        visible.status.subordinate = 0;
        visible.status.use_flag = 0;
        assert!(views_visible_directory_valid(&visible, Dialect::V4_0));
        assert!(!views_visible_directory_valid(&visible, Dialect::V5_0));
        visible.status.use_flag = 2;
        assert!(views_visible_directory_valid(&visible, Dialect::V4_0));
        assert!(!views_visible_directory_valid(&visible, Dialect::V5_0));
        visible.status.use_flag = 1;
        visible.status.blank = 1;
        visible.status.hierarchy = 3;
        assert!(views_visible_directory_valid(&visible, Dialect::V4_0));
        assert!(views_visible_directory_valid(&visible, Dialect::V5_0));
        for field in 0..4 {
            let mut candidate = directory_entry(402, form);
            match field {
                0 => candidate.structure = 1,
                1 => candidate.line_font = 1,
                2 => candidate.line_weight = 1,
                _ => candidate.color = 1,
            }
            assert!(views_visible_directory_valid(&candidate, Dialect::V4_0));
            assert!(views_visible_directory_valid(&candidate, Dialect::V5_0));
        }
        visible.level = 2;
        visible.view = 3;
        visible.transform = 5;
        visible.label_display = 7;
        assert!(views_visible_directory_valid(&visible, Dialect::V4_0));
        assert!(views_visible_directory_valid(&visible, Dialect::V5_0));
    }
}

#[test]
fn decode_view_visibility_use_flag_follows_v4_and_v5_rules() {
    const GLOBAL_V4: &[u8] = b"1H,,1H;,7Hproduct,8Hpart.igs,7Hcadmpeg,3H0.1,32,38,6,308,15,7Hproduct,1.0,2,2HMM,1,1.0,13H260714.000000,0.001,1000.0,6Hauthor,3Horg,6,0;";
    const GLOBAL_V5_0: &[u8] = b"1H,,1H;,7Hproduct,8Hpart.igs,7Hcadmpeg,3H0.1,32,38,6,308,15,0H,1.0,2,2HMM,1,1.0,13H260714.000000,0.001,1000.0,6Hauthor,3Horg,8,0,0H;";
    let decode = |global: &[u8], status: &'static str| {
        IgesCodec
            .decode(
                &mut Cursor::new(owned_test_file_with_global(
                    &[
                        OwnedTestEntity {
                            entity_type: 410,
                            form: 0,
                            label: "VIEW".into(),
                            status: "00000100",
                            parameters: "410,1,1,0,0,0,0,0,0,1,3,0;".into(),
                        },
                        OwnedTestEntity {
                            entity_type: 402,
                            form: 3,
                            label: "VISIBLE".into(),
                            status,
                            parameters: "402,1,0,1;".into(),
                        },
                    ],
                    global,
                )),
                &DecodeOptions::default(),
            )
            .unwrap()
    };

    let v4 = decode(GLOBAL_V4, "00000200");
    assert!(v4.report().losses.iter().all(|loss| {
        loss.code != IgesLossCode::EntityNotProjected.kind()
            || loss
                .provenance
                .as_ref()
                .and_then(|provenance| provenance.tag.as_deref())
                != Some("directory_entry:D3")
    }));
    assert_eq!(
        v4.ir().native.namespace("iges").unwrap().arenas["view_visibility"].len(),
        1
    );

    let v5 = decode(GLOBAL_V5_0, "00000200");
    assert!(
        v5.report().losses.iter().any(|loss| {
            loss.code == IgesLossCode::EntityNotProjected.kind()
                && loss
                    .provenance
                    .as_ref()
                    .and_then(|provenance| provenance.tag.as_deref())
                    == Some("directory_entry:D3")
        }),
        "{:#?}",
        v5.report().losses
    );
}

#[test]
fn decode_quarantines_presentation_entities_with_invalid_directory_contracts() {
    let cases = [
        (
            "drawing",
            vec![OwnedTestEntity {
                entity_type: 404,
                form: 1,
                label: "DRAWING".into(),
                status: "00000000",
                parameters: "404,1,1,0,0,0;".into(),
            }],
            "directory_entry:D1",
        ),
        (
            "view",
            vec![OwnedTestEntity {
                entity_type: 410,
                form: 0,
                label: "VIEW".into(),
                status: "00000000",
                parameters: "410,1,1,0,0,0,0,0,0;".into(),
            }],
            "directory_entry:D1",
        ),
        (
            "segmented visibility",
            vec![
                OwnedTestEntity {
                    entity_type: 410,
                    form: 0,
                    label: "VIEW".into(),
                    status: "00000100",
                    parameters: "410,1,1,0,0,0,0,0,0;".into(),
                },
                OwnedTestEntity {
                    entity_type: 402,
                    form: 19,
                    label: "SEGMENTS".into(),
                    status: "00010000",
                    parameters: "402,1,1,0.5,0,,,1;".into(),
                },
            ],
            "directory_entry:D3",
        ),
        (
            "view visibility",
            vec![
                OwnedTestEntity {
                    entity_type: 410,
                    form: 0,
                    label: "VIEW".into(),
                    status: "00000100",
                    parameters: "410,1,1,0,0,0,0,0,0,1,3,0;".into(),
                },
                OwnedTestEntity {
                    entity_type: 402,
                    form: 3,
                    label: "VISIBLE".into(),
                    status: "00010000",
                    parameters: "402,1,0,1;".into(),
                },
            ],
            "directory_entry:D3",
        ),
    ];

    for (kind, entities, expected_tag) in cases {
        let result = IgesCodec
            .decode(
                &mut Cursor::new(owned_test_file(&entities)),
                &DecodeOptions::default(),
            )
            .unwrap();
        let losses = result
            .report()
            .losses
            .iter()
            .filter(|loss| loss.code == IgesLossCode::EntityNotProjected.kind())
            .collect::<Vec<_>>();
        assert_eq!(losses.len(), 1, "{kind}: {:#?}", result.report().losses);
        assert_eq!(
            losses[0]
                .provenance
                .as_ref()
                .and_then(|provenance| provenance.tag.as_deref()),
            Some(expected_tag),
            "{kind}: {:#?}",
            result.report().losses
        );
    }
}

#[test]
fn view_up_component_test_is_scale_invariant() {
    assert!(has_in_plane_component(
        [0.0, 0.0, 1.0e-200],
        [0.0, 1.0e-200, 0.0]
    ));
    assert!(has_in_plane_component(
        [1.0e200, 0.0, 0.0],
        [1.0e200, 1.0e184, 0.0]
    ));
    assert!(!has_in_plane_component(
        [1.0e200, 0.0, 0.0],
        [1.0e200, 0.0, 0.0]
    ));
}

#[test]
fn drawing_enumerations_match_the_iges_tables() {
    for value in 0..=3 {
        assert!(depth_clipping_valid(value));
    }
    assert!(!depth_clipping_valid(-1));
    assert!(!depth_clipping_valid(4));

    for value in 0..=1 {
        assert!(display_flag_valid(value));
    }
    assert!(!display_flag_valid(-1));
    assert!(!display_flag_valid(2));

    for value in 1..=5 {
        assert!(standard_line_font_valid(value));
    }
    assert!(!standard_line_font_valid(0));
    assert!(!standard_line_font_valid(6));

    for value in 0..=8 {
        assert!(standard_color_valid(value));
    }
    assert!(!standard_color_valid(-1));
    assert!(!standard_color_valid(9));
}

#[test]
fn clipping_plane_use_flag_follows_the_declared_dialect() {
    let mut target = DirectoryEntry {
        source_offset: 0,
        sequence: 1,
        entity_type: 108,
        parameter_start: 0,
        structure: 0,
        line_font: 0,
        level: 0,
        view: 0,
        transform: 0,
        label_display: 0,
        status: Status {
            blank: 0,
            subordinate: 0,
            use_flag: 0,
            hierarchy: 0,
        },
        line_weight: 0,
        color: 0,
        parameter_line_count: 0,
        form: 0,
        reserved: [[b' '; 8]; 2],
        label: [b' '; 8],
        subscript: 0,
    };
    for use_flag in [0, 1, 2, 5] {
        target.status.use_flag = use_flag;
        assert!(clipping_plane_valid(&target, Dialect::V4_0), "{use_flag}");
    }
    for use_flag in [3, 4] {
        target.status.use_flag = use_flag;
        assert!(!clipping_plane_valid(&target, Dialect::V4_0), "{use_flag}");
    }
    assert!(!clipping_plane_valid(&target, Dialect::V5_0));
    target.status.use_flag = 1;
    assert!(clipping_plane_valid(&target, Dialect::V5_0));
}

#[test]
fn decode_rejects_file_duplicate_drawing_sheet_ids() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(duplicate_drawing_sheet_ids_file()),
            &DecodeOptions::default(),
        )
        .unwrap();
    let losses = result
        .report()
        .losses
        .iter()
        .filter(|loss| loss.code == IgesLossCode::EntityNotProjected.kind())
        .collect::<Vec<_>>();
    assert_eq!(losses.len(), 2, "{:#?}", result.report().losses);
    let tags = losses
        .iter()
        .map(|loss| {
            loss.provenance
                .as_ref()
                .and_then(|provenance| provenance.tag.as_deref())
                .unwrap()
        })
        .collect::<Vec<_>>();
    assert_eq!(tags, ["directory_entry:D3", "directory_entry:D9"]);
}

#[test]
fn decode_accepts_distinct_drawing_sheet_id_pairs() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(distinct_drawing_sheet_ids_file()),
            &DecodeOptions::default(),
        )
        .unwrap();
    assert!(
        result.report().losses.is_empty(),
        "{:#?}",
        result.report().losses
    );
}

#[test]
fn decode_rejects_drawing_sheet_id_referenced_by_two_drawings() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(shared_drawing_sheet_id_file()),
            &DecodeOptions::default(),
        )
        .unwrap();
    let losses = result
        .report()
        .losses
        .iter()
        .filter(|loss| loss.code == IgesLossCode::EntityNotProjected.kind())
        .collect::<Vec<_>>();
    assert_eq!(losses.len(), 1, "{:#?}", result.report().losses);
    assert_eq!(
        losses[0]
            .provenance
            .as_ref()
            .and_then(|provenance| provenance.tag.as_deref()),
        Some("directory_entry:D3")
    );
}

#[test]
fn decode_types_orthographic_and_perspective_views() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(view_forms_file()),
            &DecodeOptions::default(),
        )
        .unwrap();
    let views = &result.ir().native.namespace("iges").unwrap().arenas["views"];
    assert_eq!(views.len(), 3);
    assert_eq!(views[0].fields()["projection"], "orthographic_parallel");
    assert!(views[0].fields()["scale"].is_null());
    assert_eq!(
        views[0].fields()["clipping_planes"]
            .as_array()
            .unwrap()
            .len(),
        6
    );
    assert_eq!(views[1].fields()["projection"], "perspective");
    assert_eq!(views[1].fields()["view_plane_normal"][2], 1.0);
    assert_eq!(views[1].fields()["center_of_projection"][2], 10.0);
    assert_eq!(views[1].fields()["clipping_window"][0], -2.0);
    assert_eq!(views[1].fields()["depth_clipping"], 3);
    assert_eq!(views[2].fields()["view_plane_normal"][2], 1.0e-200);
    assert_eq!(views[2].fields()["view_up"][1], 1.0e-200);
    assert!(
        result.report().losses.is_empty(),
        "{:#?}",
        result.report().losses
    );
}

#[test]
fn decode_rejects_out_of_table_depth_clipping_indicator() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(out_of_table_depth_clipping_view_file()),
            &DecodeOptions::default(),
        )
        .unwrap();

    assert!(result
        .report()
        .losses
        .iter()
        .any(|loss| loss.code == IgesLossCode::EntityNotProjected.kind()));
}

#[test]
fn decode_rejects_out_of_table_segmented_display_flag() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(out_of_table_segmented_display_file()),
            &DecodeOptions::default(),
        )
        .unwrap();

    assert!(result
        .report()
        .losses
        .iter()
        .any(|loss| loss.code == IgesLossCode::EntityNotProjected.kind()));
}

#[test]
fn decode_applies_defaults_and_accepts_zero_text_box_dimensions() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(defaulted_text_and_view_fields_file()),
            &DecodeOptions::default(),
        )
        .unwrap();
    assert!(
        result.report().losses.is_empty(),
        "{:#?}",
        result.report().losses
    );
}

#[test]
fn decode_does_not_default_wrong_typed_view_fields() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(malformed_view_parameter_type_file()),
            &DecodeOptions::default(),
        )
        .unwrap();
    let losses = result
        .report()
        .losses
        .iter()
        .filter(|loss| loss.code == IgesLossCode::EntityNotProjected.kind())
        .count();
    assert_eq!(losses, 1, "{:#?}", result.report().losses);
}

#[test]
fn decode_types_view_visibility_and_display_overrides() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(view_visibility_forms_file()),
            &DecodeOptions::default(),
        )
        .unwrap();
    let visibility = &result.ir().native.namespace("iges").unwrap().arenas["view_visibility"];
    assert_eq!(visibility.len(), 2);
    assert_eq!(visibility[0].fields()["form"], 3);
    assert_eq!(
        visibility[0].fields()["displays"][0]["view"],
        "iges:presentation:view#D1"
    );
    assert!(visibility[0].fields()["displays"][0]["line_font"].is_null());
    assert_eq!(visibility[1].fields()["form"], 4);
    assert_eq!(visibility[1].fields()["displays"][0]["line_font"], 1);
    assert_eq!(visibility[1].fields()["displays"][0]["color"], 2);
    assert_eq!(visibility[1].fields()["displays"][0]["line_weight"], 3);
    assert!(
        result.report().losses.is_empty(),
        "{:#?}",
        result.report().losses
    );
}

#[test]
fn decode_view_visibility_defaults_omitted_entity_count_and_color() {
    for (form, parameters) in [(3, "402,1,,1;"), (4, "402,1,,1,1,0,,1;")] {
        let bytes = owned_test_file(&[
            OwnedTestEntity {
                entity_type: 410,
                form: 0,
                label: "VIEW".into(),
                status: "00000100",
                parameters: "410,1,1,0,0,0,0,0,0,1,3,0;".into(),
            },
            OwnedTestEntity {
                entity_type: 402,
                form,
                label: "VISIBLE".into(),
                status: "00000100",
                parameters: parameters.into(),
            },
        ]);
        let result = IgesCodec
            .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
            .unwrap();
        let visibility = &result.ir().native.namespace("iges").unwrap().arenas["view_visibility"];
        let fields = visibility[0].fields();
        assert_eq!(fields["declared_view_count"], 1, "form={form}");
        assert!(fields["declared_entity_count"].is_null(), "form={form}");
        assert_eq!(
            fields["displays"].as_array().unwrap().len(),
            1,
            "form={form}"
        );
        assert!(
            fields["entities"].as_array().unwrap().is_empty(),
            "form={form}"
        );
        assert!(
            result.report().losses.is_empty(),
            "form={form}: {:#?}",
            result.report().losses
        );
    }
}

#[test]
fn decode_view_visibility_entity_count_requirement_follows_dialect() {
    const GLOBAL_V4: &[u8] = b"1H,,1H;,7Hproduct,8Hpart.igs,7Hcadmpeg,3H0.1,32,38,6,308,15,7Hproduct,1.0,2,2HMM,1,1.0,13H260714.000000,0.001,1000.0,6Hauthor,3Horg,6,0;";
    const GLOBAL_V5_0: &[u8] = b"1H,,1H;,7Hproduct,8Hpart.igs,7Hcadmpeg,3H0.1,32,38,6,308,15,0H,1.0,2,2HMM,1,1.0,13H260714.000000,0.001,1000.0,6Hauthor,3Horg,8,0,0H;";
    let decode = |global: &[u8], visible_parameters: &str| {
        IgesCodec
            .decode(
                &mut Cursor::new(owned_test_file_with_global(
                    &[
                        OwnedTestEntity {
                            entity_type: 410,
                            form: 0,
                            label: "VIEW".into(),
                            status: "00000100",
                            parameters: "410,1,1,0,0,0,0,0,0,1,3,0;".into(),
                        },
                        OwnedTestEntity {
                            entity_type: 402,
                            form: 3,
                            label: "VISIBLE".into(),
                            status: "00000100",
                            parameters: visible_parameters.into(),
                        },
                    ],
                    global,
                )),
                &DecodeOptions::default(),
            )
            .unwrap()
    };

    let v4 = decode(GLOBAL_V4, "402,1,,1;");
    let v4_losses = v4
        .report()
        .losses
        .iter()
        .filter(|loss| loss.code == IgesLossCode::EntityNotProjected.kind())
        .collect::<Vec<_>>();
    assert_eq!(v4_losses.len(), 1, "{:#?}", v4.report().losses);
    assert_eq!(
        v4_losses[0]
            .provenance
            .as_ref()
            .and_then(|provenance| provenance.tag.as_deref()),
        Some("directory_entry:D3")
    );

    let v5 = decode(GLOBAL_V5_0, "402,1,,1;");
    assert!(
        v5.report()
            .losses
            .iter()
            .all(|loss| loss.code != IgesLossCode::EntityNotProjected.kind()),
        "{:#?}",
        v5.report().losses
    );
    let visibility = &v5.ir().native.namespace("iges").unwrap().arenas["view_visibility"];
    assert_eq!(visibility.len(), 1);
    assert_eq!(
        visibility[0].fields()["displays"].as_array().unwrap().len(),
        1
    );
    assert!(visibility[0].fields()["entities"]
        .as_array()
        .unwrap()
        .is_empty());
}

#[test]
fn decode_preserves_ordered_segmented_view_display() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(segmented_view_visibility_file()),
            &DecodeOptions::default(),
        )
        .unwrap();
    let segmented =
        &result.ir().native.namespace("iges").unwrap().arenas["segmented_visibility"][0];
    assert_eq!(segmented.fields()["blocks"].as_array().unwrap().len(), 2);
    assert_eq!(segmented.fields()["blocks"][0]["breakpoint"], 0.5);
    assert_eq!(segmented.fields()["blocks"][0]["color"]["kind"], "omitted");
    assert_eq!(segmented.fields()["blocks"][1]["breakpoint"], 1.0);
    assert_eq!(segmented.fields()["blocks"][1]["color"]["value"], 2);
    assert_eq!(segmented.fields()["blocks"][1]["line_font"]["value"], 3);
    assert!(
        result.report().losses.is_empty(),
        "{:#?}",
        result.report().losses
    );
}

#[test]
fn decode_types_drawing_view_placement_annotations_and_sheet_properties() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(drawing_with_properties_file()),
            &DecodeOptions::default(),
        )
        .unwrap();
    let drawing = &result.ir().native.namespace("iges").unwrap().arenas["drawings"][0];
    assert_eq!(drawing.fields()["form"], 1);
    assert_eq!(
        drawing.fields()["views"][0]["view"],
        "iges:presentation:view#D1"
    );
    assert_eq!(drawing.fields()["views"][0]["origin"][0], 10.0);
    assert_eq!(drawing.fields()["views"][0]["rotation"], 0.5);
    assert_eq!(
        drawing.fields()["annotations"][0],
        "iges:entity:directory#3"
    );
    assert_eq!(drawing.fields()["size"][0], 210.0);
    assert_eq!(drawing.fields()["size"][1], 297.0);
    assert_eq!(drawing.fields()["units_flag"], 2);
    assert_eq!(drawing.fields()["units_name"][0], 77);
    assert_eq!(drawing.fields()["name"][0], 68);
    assert!(
        result.report().losses.is_empty(),
        "{:#?}",
        result.report().losses
    );
}

#[test]
fn decode_reports_conflicting_drawing_property_values() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(drawing_with_conflicting_size_properties_file()),
            &DecodeOptions::default(),
        )
        .unwrap();
    let drawing = &result.ir().native.namespace("iges").unwrap().arenas["drawings"][0];

    assert!(drawing.fields()["size"].is_null());
    assert_eq!(drawing.fields()["ambiguous_property_forms"][0], 16);
    let loss = result
        .report()
        .losses
        .iter()
        .find(|loss| loss.code == IgesLossCode::DrawingPropertyAmbiguous.kind())
        .expect("ambiguous drawing property loss");
    assert_eq!(
        loss.provenance
            .as_ref()
            .and_then(|provenance| provenance.tag.as_deref()),
        Some("directory_entry:D13")
    );
}

#[test]
fn decode_types_view_list_with_required_back_pointers() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(view_list_associativity_file(true)),
            &DecodeOptions::default(),
        )
        .unwrap();
    let view_list = result.ir().native.namespace("iges").unwrap().arenas["associativities"]
        .iter()
        .find(|value| value.fields()["kind"] == "view_list")
        .unwrap();
    assert_eq!(view_list.fields()["declared_visible_count"], 1);
    assert_eq!(view_list.fields()["view"], "iges:entity:directory#1");
    assert_eq!(
        view_list.fields()["visible_entities"][0],
        "iges:entity:directory#5"
    );
    assert!(
        result.report().losses.is_empty(),
        "{:#?}",
        result.report().losses
    );

    let missing = IgesCodec
        .decode(
            &mut Cursor::new(view_list_associativity_file(false)),
            &DecodeOptions::default(),
        )
        .unwrap();
    assert!(missing.report().losses.iter().any(|loss| {
        loss.message.contains("entity type 402 form 6")
            && loss.message.contains("predefined associativity")
    }));
}

#[test]
fn decode_types_v4_view_list_with_required_back_pointers() {
    const GLOBAL_V4: &[u8] = b"1H,,1H;,7Hproduct,8Hpart.igs,7Hcadmpeg,3H0.1,32,38,6,308,15,7Hproduct,1.0,2,2HMM,1,1.0,13H260714.000000,0.001,1000.0,6Hauthor,3Horg,6,0;";
    let result = IgesCodec
        .decode(
            &mut Cursor::new(view_list_associativity_file_with_global(true, GLOBAL_V4)),
            &DecodeOptions::default(),
        )
        .unwrap();
    let view_list = result.ir().native.namespace("iges").unwrap().arenas["associativities"]
        .iter()
        .find(|value| value.fields()["kind"] == "view_list")
        .unwrap();
    assert_eq!(view_list.fields()["declared_visible_count"], 1);
    assert_eq!(view_list.fields()["view"], "iges:entity:directory#1");
    assert_eq!(
        view_list.fields()["visible_entities"][0],
        "iges:entity:directory#5"
    );
    assert_eq!(
        result
            .report()
            .losses
            .iter()
            .filter(|loss| loss.code == IgesLossCode::SourceDialectUnverified.kind())
            .count(),
        1
    );
    assert!(result
        .report()
        .losses
        .iter()
        .all(|loss| loss.code == IgesLossCode::SourceDialectUnverified.kind()));
}
