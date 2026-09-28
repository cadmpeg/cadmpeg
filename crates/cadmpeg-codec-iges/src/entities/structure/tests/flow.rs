// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use std::io::Cursor;

use cadmpeg_ir::codec::Codec;
use cadmpeg_ir::codec::DecodeOptions;

use crate::loss::IgesLossCode;
use crate::test_support::test_drawing_and_trimming::bounded_associativity_forms_file_with_global;
use crate::test_support::test_drawing_and_trimming::flow_associativity_forms_file;
use crate::test_support::test_drawing_and_trimming::label_display_without_leader_file;
use crate::test_support::test_drawing_and_trimming::legacy_associativity_forms_file;
use crate::test_support::test_drawing_and_trimming::legacy_associativity_forms_file_with_global;
use crate::test_support::test_drawing_and_trimming::legacy_generic_single_parent_file;
use crate::test_support::test_owned::owned_test_file;
use crate::test_support::test_owned::OwnedTestEntity;

use crate::IgesCodec;
const LEGACY_TEXT_ANGLE_TOLERANCE: f64 = 1.0e-4;

#[test]
fn decode_keeps_nonplane_single_parent_relations_native_and_transfers_the_bounded_parent() {
    let global_v4 = b"1H,,1H;,7Hproduct,8Hpart.igs,7Hcadmpeg,3H0.1,32,38,6,308,15,0H,1.0,2,2HMM,1,1.0,13H260714.000000,0.001,1000.0,6Hauthor,3Horg,6,0;";
    let global_v5 = b"1H,,1H;,7Hproduct,8Hpart.igs,7Hcadmpeg,3H0.1,32,38,6,308,15,0H,1.0,2,2HMM,1,1.0,13H260714.000000,0.001,1000.0,6Hauthor,3Horg,8,0,0H;";
    for (version, global) in [("4.0", &global_v4[..]), ("5.0", &global_v5[..])] {
        let result = IgesCodec
            .decode(
                &mut Cursor::new(legacy_generic_single_parent_file(global)),
                &DecodeOptions::default(),
            )
            .unwrap();
        assert_eq!(result.ir().model.faces.len(), 1, "IGES {version}");
        assert_eq!(
            result.ir().model.faces[0].surface,
            "iges:model:surface#D1".try_into().expect("valid identity"),
            "IGES {version}"
        );
        assert!(result.report().losses.iter().all(|loss| {
            loss.code != IgesLossCode::EntityNotProjected.kind()
                || !loss.message.contains("IGES entity type 402 form 9")
        }));
        let association = result.ir().native.namespace("iges").unwrap().arenas()["associativities"]
            .iter()
            .find(|value| value.fields()["kind"] == "single_parent")
            .expect("generic single-parent association");
        assert_eq!(association.fields()["parent"], "iges:entity:directory#1");
        assert_eq!(
            association.fields()["children"][0],
            "iges:entity:directory#5"
        );
    }
}

#[test]
fn decode_preserves_legacy_dimensioned_geometry_roles_in_v4_and_v5_profiles() {
    let global_v4 = b"1H,,1H;,7Hproduct,8Hpart.igs,7Hcadmpeg,3H0.1,32,38,6,308,15,0H,1.0,2,2HMM,1,1.0,13H260714.000000,0.001,1000.0,6Hauthor,3Horg,6,0;";
    let global_v5 = b"1H,,1H;,7Hproduct,8Hpart.igs,7Hcadmpeg,3H0.1,32,38,6,308,15,0H,1.0,2,2HMM,1,1.0,13H260714.000000,0.001,1000.0,6Hauthor,3Horg,8,0,0H;";
    for (version, global) in [("4.0", &global_v4[..]), ("5.0", &global_v5[..])] {
        let result = IgesCodec
            .decode(
                &mut Cursor::new(bounded_associativity_forms_file_with_global(global)),
                &DecodeOptions::default(),
            )
            .unwrap();
        let association = result.ir().native.namespace("iges").unwrap().arenas()["associativities"]
            .iter()
            .find(|value| value.fields()["kind"] == "dimensioned_geometry")
            .expect("legacy dimensioned-geometry association");
        assert_eq!(
            association.fields()["dimension"],
            "iges:entity:directory#21"
        );
        assert_eq!(
            association.fields()["geometry"][0],
            "iges:entity:directory#9"
        );
        assert!(
            result.report().losses.iter().all(|loss| {
                loss.code != IgesLossCode::EntityNotProjected.kind()
                    || !loss.message.contains("IGES entity type 402 form 13")
            }),
            "IGES {version}: {:#?}",
            result.report().losses
        );
    }
}

#[test]
fn decode_rejects_label_display_without_leader() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(label_display_without_leader_file()),
            &DecodeOptions::default(),
        )
        .unwrap();
    assert!(result
        .report()
        .losses
        .iter()
        .any(|loss| loss.code == IgesLossCode::EntityNotProjected.kind()));
    let loss = result
        .report()
        .losses
        .iter()
        .find(|loss| loss.code == IgesLossCode::EntityNotProjected.kind())
        .unwrap();
    assert_eq!(
        loss.provenance
            .as_ref()
            .and_then(|provenance| provenance.tag.as_deref()),
        Some("directory_entry:D5")
    );
    let label_display = result.ir().native.namespace("iges").unwrap().arenas()["associativities"]
        .iter()
        .find(|associativity| associativity.fields()["kind"] == "label_display")
        .unwrap();
    assert!(label_display.fields()["placements"][0]["leader"].is_null());
}

#[test]
fn decode_preserves_signal_and_piping_flow_class_order() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(flow_associativity_forms_file()),
            &DecodeOptions::default(),
        )
        .unwrap();
    let associativities =
        &result.ir().native.namespace("iges").unwrap().arenas()["associativities"];
    let signal = associativities
        .iter()
        .find(|value| {
            value.fields()["kind"] == "flow"
                && value.fields()["form"] == 18
                && value.fields()["connections"].as_array().unwrap().len() == 1
        })
        .unwrap();
    assert_eq!(signal.fields()["type_flag"], 1);
    assert_eq!(signal.fields()["declared_associated_flow_count"], 0);
    assert_eq!(signal.fields()["declared_connection_count"], 1);
    assert_eq!(signal.fields()["declared_join_count"], 1);
    assert_eq!(signal.fields()["declared_name_count"], 1);
    assert_eq!(signal.fields()["declared_name_display_count"], 1);
    assert_eq!(signal.fields()["declared_continuation_count"], 1);
    assert_eq!(signal.fields()["function_flag"], 2);
    assert_eq!(signal.fields()["connections"][0], "iges:entity:directory#1");
    assert_eq!(signal.fields()["joins"][0], "iges:entity:directory#3");
    assert_eq!(signal.fields()["names"][0][0], 70);
    assert_eq!(
        signal.fields()["name_displays"][0],
        "iges:entity:directory#5"
    );
    assert_eq!(
        signal.fields()["continuations"][0],
        "iges:entity:directory#9"
    );
    let pipe = associativities
        .iter()
        .find(|value| {
            value.fields()["kind"] == "flow"
                && value.fields()["form"] == 20
                && value.fields()["connections"].as_array().unwrap().len() == 1
        })
        .unwrap();
    assert_eq!(pipe.fields()["type_flag"], 2);
    assert_eq!(pipe.fields()["declared_associated_flow_count"], 0);
    assert_eq!(pipe.fields()["declared_connection_count"], 1);
    assert_eq!(pipe.fields()["declared_join_count"], 1);
    assert_eq!(pipe.fields()["declared_name_count"], 1);
    assert_eq!(pipe.fields()["declared_name_display_count"], 0);
    assert_eq!(pipe.fields()["declared_continuation_count"], 1);
    assert!(pipe.fields()["function_flag"].is_null());
    assert_eq!(pipe.fields()["connections"][0], "iges:entity:directory#11");
    assert_eq!(
        pipe.fields()["continuations"][0],
        "iges:entity:directory#17"
    );
    assert!(
        result.report().losses.is_empty(),
        "{:#?}",
        result.report().losses
    );
}

#[test]
fn decode_rejects_a_non_geometry_flow_join_target() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(owned_test_file(&[
                OwnedTestEntity {
                    entity_type: 132,
                    form: 0,
                    label: "PIPEPT".into(),
                    status: "00000400",
                    parameters: "132,0,0,0,0,101,1,2HP1,0,4HPIPE,0,1,1,0,0;".into(),
                },
                OwnedTestEntity {
                    entity_type: 410,
                    form: 0,
                    label: "VIEW".into(),
                    status: "00000100",
                    parameters: "410,1,1,0,0,0,0,0,0;".into(),
                },
                OwnedTestEntity {
                    entity_type: 402,
                    form: 20,
                    label: "PIPEFLOW".into(),
                    status: "00000200",
                    parameters: "402,1,0,1,1,0,0,2,1,3,4HPIPE;".into(),
                },
            ])),
            &DecodeOptions::default(),
        )
        .unwrap();

    assert!(result
        .report()
        .losses
        .iter()
        .any(|loss| loss.code == IgesLossCode::EntityNotProjected.kind()));
    let flow = result.ir().native.namespace("iges").unwrap().arenas()["associativities"]
        .iter()
        .find(|value| value.fields()["kind"] == "flow")
        .unwrap();
    assert!(flow.fields()["joins"][0].is_null());
}

#[test]
fn decode_preserves_legacy_signal_text_and_connect_associativities() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(legacy_associativity_forms_file()),
            &DecodeOptions::default(),
        )
        .unwrap();
    let associativities =
        &result.ir().native.namespace("iges").unwrap().arenas()["associativities"];

    let signal = associativities
        .iter()
        .find(|value| value.fields()["kind"] == "legacy_signal_string")
        .unwrap();
    assert_eq!(signal.fields()["declared_signal_name_count"], 1);
    assert_eq!(signal.fields()["declared_connection_count"], 1);
    assert_eq!(signal.fields()["declared_schematic_count"], 1);
    assert_eq!(signal.fields()["declared_physical_count"], 1);
    assert_eq!(
        signal.fields()["signal_names"][0],
        serde_json::json!([78, 69, 84])
    );
    assert_eq!(signal.fields()["connections"][0], "iges:entity:directory#3");
    assert_eq!(
        signal.fields()["schematic_entities"][0],
        "iges:entity:directory#11"
    );
    assert_eq!(
        signal.fields()["physical_entities"][0],
        "iges:entity:directory#11"
    );

    let text = associativities
        .iter()
        .find(|value| value.fields()["kind"] == "legacy_text_node")
        .unwrap();
    assert_eq!(text.fields()["declared_geometry_count"], 1);
    assert_eq!(text.fields()["declared_text_description_count"], 1);
    assert_eq!(text.fields()["geometry"][0], "iges:entity:directory#5");
    assert_eq!(text.fields()["box_width"], 1.0);
    assert_eq!(text.fields()["box_height"], 2.0);
    assert_eq!(text.fields()["font_characteristic"], 1);
    assert!(
        (text.fields()["slant_angle"].as_f64().unwrap() - std::f64::consts::FRAC_PI_2).abs()
            <= LEGACY_TEXT_ANGLE_TOLERANCE
    );
    assert_eq!(text.fields()["rotation_angle"], 0.0);
    assert_eq!(text.fields()["mirror_flag"], 0);
    assert_eq!(text.fields()["rotate_internal_flag"], 0);

    let connect = associativities
        .iter()
        .find(|value| value.fields()["kind"] == "legacy_connect_node")
        .unwrap();
    assert_eq!(connect.fields()["declared_point_count"], 1);
    assert_eq!(connect.fields()["declared_data_count"], 2);
    assert_eq!(connect.fields()["points"][0], "iges:entity:directory#1");
    assert_eq!(connect.fields()["data"][0]["kind"], "string");
    assert_eq!(
        connect.fields()["data"][0]["value"],
        serde_json::json!([67, 79, 78, 83, 84, 82])
    );
    assert_eq!(connect.fields()["data"][1]["kind"], "integer");
    assert_eq!(connect.fields()["data"][1]["value"], 42);
    assert!(
        result.report().losses.is_empty(),
        "{:#?}",
        result.report().losses
    );
}

#[test]
fn decode_preserves_v4_legacy_signal_text_and_connect_associativities() {
    const GLOBAL_V4: &[u8] = b"1H,,1H;,7Hproduct,8Hpart.igs,7Hcadmpeg,3H0.1,32,38,6,308,15,7Hproduct,1.0,2,2HMM,1,1.0,13H260714.000000,0.001,1000.0,6Hauthor,3Horg,6,0;";
    let result = IgesCodec
        .decode(
            &mut Cursor::new(legacy_associativity_forms_file_with_global(GLOBAL_V4)),
            &DecodeOptions::default(),
        )
        .unwrap();
    let associativities =
        &result.ir().native.namespace("iges").unwrap().arenas()["associativities"];
    for kind in [
        "legacy_signal_string",
        "legacy_text_node",
        "legacy_connect_node",
    ] {
        assert!(
            associativities
                .iter()
                .any(|value| value.fields()["kind"] == kind),
            "missing V4 associativity kind {kind}"
        );
    }
    assert!(result.report().losses.is_empty(), "{:#?}", result.report());
}
