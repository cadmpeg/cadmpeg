// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use crate::directory::{DirectoryEntry, SourceStatus};
use std::collections::BTreeMap;
use std::io::Cursor;

use cadmpeg_core::decode::DecodeMode;
use cadmpeg_core::decode::ResourceDimension;
use cadmpeg_ir::codec::{Codec, DecodeOptions};

use super::{
    flow_join_target_valid, functional_level_identifier_valid, line_font_property_code_valid,
    network_connectivity_valid, signal_string_geometry_target,
    subfigure_definition_directory_fields_valid,
};
use crate::global::GlobalTable;
use crate::loss::IgesLossCode;
use crate::test_support::*;
use crate::IgesCodec;
mod bounded_planes;
mod definitions;
mod dialect;
mod network;
const LEGACY_TEXT_ANGLE_TOLERANCE: f64 = 1.0e-4;

#[test]
fn signal_string_geometry_accepts_composite_constituents_and_copious_forms() {
    for (entity_type, form) in [
        (100, 0),
        (102, 0),
        (104, 3),
        (106, 11),
        (110, 2),
        (112, 0),
        (116, 0),
        (126, 5),
        (130, 0),
        (132, 0),
    ] {
        assert!(signal_string_geometry_target(entity_type, form));
    }
    for (entity_type, form) in [(106, 10), (130, 1), (134, 0), (402, 11)] {
        assert!(!signal_string_geometry_target(entity_type, form));
    }
}

#[test]
fn flow_join_targets_use_geometry_classification_or_subfigure_instance_type() {
    let target = |entity_type, use_flag| DirectoryEntry {
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
        status: SourceStatus::from_codes([0, 0, use_flag, 0]),
        line_weight: 0,
        color: 0,
        parameter_line_count: 0,
        form: 0,
        reserved: [[b' '; 8]; 2],
        label: [b' '; 8],
        subscript: 0,
    };

    assert!(flow_join_target_valid(
        &target(110, 0),
        crate::global::GlobalTable::V5Later
    ));
    assert!(flow_join_target_valid(
        &target(408, 2),
        crate::global::GlobalTable::V5Later
    ));
    assert!(!flow_join_target_valid(
        &target(410, 1),
        crate::global::GlobalTable::V5Later
    ));
    assert!(!flow_join_target_valid(
        &target(406, 0),
        crate::global::GlobalTable::V5Later
    ));
}

#[test]
fn type406_form19_accepts_only_predefined_line_font_pattern_codes() {
    for code in [
        12, 14, 16, 18, 22, 42, 44, 46, 48, 52, 54, 152, 154, 156, 162, 164, 166, 172, 174, 176,
        178, 192, 194, 198, 200, 203, 206, 223, 227, 230, 232, 237, 239, 240, 253, 270, 330, 355,
        360, 380, 385, 390, 395, 400, 405, 410, 415, 420, 425, 430, 445, 485,
    ] {
        assert!(line_font_property_code_valid(code), "{code}");
    }
    for code in [0, 1, 13, 19, 55, 151, 207, 222, 486, 5001] {
        assert!(!line_font_property_code_valid(code), "{code}");
    }
}

#[test]
fn type406_form24_accepts_only_predefined_functional_level_identifiers() {
    for value in [
        "Annotation",
        "Drilled Holes",
        "Errors",
        "Panel_Outline",
        "Placement_Keepin",
        "Placement_Keepout",
        "PRD_ID",
        "Routing_Keepin",
        "Routing_Keepout",
        "Signal_Guide",
        "Substrate_Outline",
        "Thermal_Outline",
        "Trace_Keepin",
        "Trace_Keepout",
        "Undefined",
        "Unplaced_Components",
        "Via_Keepin",
        "Via_Keepout",
        "Via_Placement",
        "Bond_Pad",
        "Breakout",
        "Chip_Pad",
        "Component_Outline",
        "Component_Placement",
        "Crossover",
        "Deposition_Components",
        "Dielectric",
        "Glue_Mask",
        "Ground",
        "Hole_Fill",
        "Laser-Trim-Path",
        "Pad",
        "Pin_ID",
        "Pin_Placement",
        "Power",
        "Sheet_Dielectric",
        "Signal",
        "Signal_ID",
        "Silkscreen",
        "Solder_Mask",
        "Solder_Paste-Mask",
        "Wire-Bond",
        "signal_t",
        "component_placement_2",
        "thermal_outline_t",
        "wire-bond_17",
    ] {
        assert!(
            functional_level_identifier_valid(value.as_bytes()),
            "{value}"
        );
    }
    for value in [
        "Signal_C",
        "Signal_1",
        "Signal_0",
        "Signal_T_2",
        "Bond_Pad_1",
        "Drilled Holes_T",
        "Unknown",
        "",
    ] {
        assert!(
            !functional_level_identifier_valid(value.as_bytes()),
            "{value}"
        );
    }
}

#[test]
fn single_target_cycle_detection_handles_long_file_controlled_chains_iteratively() {
    let targets = (1..=100_000_u32)
        .map(|sequence| (sequence, sequence + 1))
        .collect::<BTreeMap<_, _>>();
    let mut visited = std::collections::BTreeSet::new();

    assert!(!crate::entities::structure::single_target_cycle(
        1,
        &targets,
        &mut visited
    ));
    assert_eq!(visited.len(), 100_000);

    let mut cyclic = targets;
    cyclic.insert(100_001, 50_000);
    assert!(crate::entities::structure::single_target_cycle(
        1,
        &cyclic,
        &mut std::collections::BTreeSet::new()
    ));
}

#[test]
fn decode_preserves_solid_definition_and_instance_identities() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(solid_instance_file()),
            &DecodeOptions::default(),
        )
        .unwrap();
    let instances = &result.ir().native.namespace("iges").unwrap().arenas()["solid_instances"];
    assert_eq!(instances.len(), 1);
    assert_eq!(instances[0].id(), "iges:product:solid-instance#D3");
    assert_eq!(
        instances[0].fields().unwrap()["solid"],
        "iges:entity:directory#1"
    );
    assert!(
        result.report().losses.is_empty(),
        "{:#?}",
        result.report().losses
    );
}

#[test]
fn decode_preserves_rectangular_and_circular_pattern_order() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(patterned_instance_file()),
            &DecodeOptions::default(),
        )
        .unwrap();
    let native = result.ir().native.namespace("iges").unwrap();
    let rectangular = &native.arenas()["rectangular_arrays"][0];
    assert_eq!(
        rectangular.fields().unwrap()["base"],
        "iges:entity:directory#1"
    );
    assert_eq!(rectangular.fields().unwrap()["columns"], 2);
    assert_eq!(rectangular.fields().unwrap()["rows"], 3);
    assert_eq!(rectangular.fields().unwrap()["positions"][0], 2);
    let circular = &native.arenas()["circular_arrays"][0];
    assert_eq!(
        circular.fields().unwrap()["base"],
        "iges:entity:directory#3"
    );
    assert_eq!(circular.fields().unwrap()["location_count"], 4);
    assert_eq!(circular.fields().unwrap()["positions"][0], 1);
    assert_eq!(circular.fields().unwrap()["positions"][1], 3);
    assert!(
        result.report().losses.is_empty(),
        "{:#?}",
        result.report().losses
    );
}

#[test]
fn decode_distinguishes_all_external_reference_forms_without_resolution() {
    let bytes = external_reference_forms_file();
    let summary = IgesCodec
        .inspect(
            &mut Cursor::new(&bytes),
            &cadmpeg_core::decode::InspectOptions::default(),
        )
        .unwrap();
    assert!(summary
        .notes
        .iter()
        .any(|note| note == "external_references=5"));
    let result = IgesCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    let references = &result.ir().native.namespace("iges").unwrap().arenas()["external_references"];
    assert_eq!(references.len(), 5);
    assert_eq!(
        references[0].fields().unwrap()["reference_kind"],
        "external_definition"
    );
    assert_eq!(
        references[1].fields().unwrap()["reference_kind"],
        "external_file_definition"
    );
    assert!(references[1].fields().unwrap()["symbolic_name"].is_null());
    assert_eq!(
        references[2].fields().unwrap()["reference_kind"],
        "external_logical"
    );
    assert_eq!(
        references[3].fields().unwrap()["reference_kind"],
        "native_definition"
    );
    assert_eq!(
        references[4].fields().unwrap()["reference_kind"],
        "native_library_definition"
    );
    assert_eq!(references[4].fields().unwrap()["library_name"][0], 68);
    assert!(references
        .iter()
        .all(|reference| reference.fields().unwrap()["resolution_state"] == "not_attempted"));
    assert!(
        result.report().losses.is_empty(),
        "{:#?}",
        result.report().losses
    );
}

#[test]
fn decode_preserves_group_order_and_back_pointer_policy() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(group_forms_file()),
            &DecodeOptions::default(),
        )
        .unwrap();
    let groups = &result.ir().native.namespace("iges").unwrap().arenas()["groups"];
    assert_eq!(groups.len(), 2);
    assert_eq!(groups[0].fields().unwrap()["ordered"], true);
    assert_eq!(groups[0].fields().unwrap()["back_pointers_required"], true);
    assert_eq!(
        groups[0].fields().unwrap()["members"][0],
        "iges:entity:directory#1"
    );
    assert_eq!(groups[1].fields().unwrap()["ordered"], false);
    assert_eq!(groups[1].fields().unwrap()["back_pointers_required"], false);
    let entities = &result.ir().native.namespace("iges").unwrap().arenas()["entities"];
    assert_eq!(
        entities[0].fields().unwrap()["association_links"][0],
        "iges:entity:directory#3"
    );
    assert!(entities[0].fields().unwrap()["property_links"]
        .as_array()
        .unwrap()
        .is_empty());
    assert!(
        result.report().losses.is_empty(),
        "{:#?}",
        result.report().losses
    );
}

#[test]
fn decode_reports_an_unresolvable_required_trailing_back_pointer() {
    let bytes = owned_test_file(&[
        OwnedTestEntity {
            entity_type: 402,
            form: 1,
            label: "GROUP".into(),
            status: "00000200",
            parameters: "402,1,3;".into(),
        },
        OwnedTestEntity {
            entity_type: 116,
            form: 0,
            label: "MEMBER".into(),
            status: "00000000",
            parameters: "116,0,0,0,0,1,99,0;".into(),
        },
    ]);
    let pointer_offset = bytes
        .windows(6)
        .position(|window| window == b",1,99,")
        .unwrap()
        + 3;
    let result = IgesCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    let native = result.ir().native.namespace("iges").unwrap();
    let member = native.arenas()["entities"]
        .iter()
        .find(|entity| entity.id() == "iges:entity:directory#3")
        .unwrap();

    assert!(member.fields().unwrap()["association_links"]
        .as_array()
        .unwrap()
        .is_empty());
    let reference = &member.fields().unwrap()["references"][0];
    assert_eq!(reference["kind"], "parameter");
    assert_eq!(reference["parameter_index"], 6);
    assert_eq!(reference["raw_pointer"], 99);
    assert_eq!(reference["resolution"], "dangling");
    let loss = result
        .report()
        .losses
        .iter()
        .find(|loss| loss.message.contains("D3 Parameter pointer 99"))
        .unwrap();
    assert_eq!(loss.code, IgesLossCode::PointerUnresolved.kind());
    assert_eq!(
        loss.provenance.as_ref().unwrap().offset,
        pointer_offset as u64
    );
}

#[test]
fn strict_decode_refuses_an_unresolved_pointer_loss() {
    let bytes = owned_test_file(&[
        OwnedTestEntity {
            entity_type: 402,
            form: 1,
            label: "GROUP".into(),
            status: "00000200",
            parameters: "402,1,3;".into(),
        },
        OwnedTestEntity {
            entity_type: 116,
            form: 0,
            label: "MEMBER".into(),
            status: "00000000",
            parameters: "116,0,0,0,0,1,99,0;".into(),
        },
    ]);
    let mut options = DecodeOptions::default();
    options.policy.mode = DecodeMode::Strict;

    let error = IgesCodec
        .decode(&mut Cursor::new(bytes), &options)
        .unwrap_err();

    match error {
        cadmpeg_ir::codec::DecodeFailure::StrictRejected { rejection } => {
            assert_eq!(
                rejection.loss().code.to_string(),
                IgesLossCode::PointerUnresolved.kind().to_string()
            );
        }
        other => panic!("expected a shared-gate strict refusal, got {other:?}"),
    }
}

#[test]
fn decode_reports_an_ambiguous_required_trailing_back_pointer_boundary() {
    let bytes = owned_test_file(&[
        OwnedTestEntity {
            entity_type: 402,
            form: 1,
            label: "GROUP".into(),
            status: "00000200",
            parameters: "402,1,3;".into(),
        },
        OwnedTestEntity {
            entity_type: 999,
            form: 0,
            label: "MEMBER".into(),
            status: "00000000",
            parameters: "999,0,0,0,0,1,0,2,7,9;".into(),
        },
    ]);
    let result = IgesCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();

    let loss = result
        .report()
        .losses
        .iter()
        .find(|loss| loss.code == IgesLossCode::ParameterBoundaryAmbiguous.kind())
        .expect("ambiguous required group boundary loss");
    assert!(loss.message.contains("2 structural"));
    assert_eq!(
        loss.provenance
            .as_ref()
            .and_then(|provenance| provenance.tag.as_deref()),
        Some("directory_entry:D3")
    );
    let member = result.ir().native.namespace("iges").unwrap().arenas()["entities"]
        .iter()
        .find(|entity| entity.id() == "iges:entity:directory#3")
        .unwrap();
    assert_eq!(
        member.fields().unwrap()["parameters"]
            .as_array()
            .unwrap()
            .len(),
        10
    );
}

#[test]
fn decode_types_all_attribute_table_definition_forms() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(attribute_definition_forms_file()),
            &DecodeOptions::default(),
        )
        .unwrap();
    let definitions =
        &result.ir().native.namespace("iges").unwrap().arenas()["attribute_table_definitions"];
    assert_eq!(definitions.len(), 3);
    assert_eq!(definitions[0].fields().unwrap()["form"], 0);
    assert_eq!(
        definitions[0].fields().unwrap()["attributes"][0]["declared_value_count"],
        1
    );
    assert_eq!(
        definitions[1].fields().unwrap()["attributes"][0]["values"][0]["value"]["kind"],
        "integer"
    );
    assert_eq!(
        definitions[1].fields().unwrap()["attributes"][1]["values"][0]["value"]["kind"],
        "string"
    );
    assert_eq!(
        definitions[2].fields().unwrap()["attributes"][0]["values"][0]["value"]["kind"],
        "real"
    );
    assert!(
        definitions[2].fields().unwrap()["attributes"][0]["values"][0]["display_template"]
            .is_null()
    );
    assert!(
        result.report().losses.is_empty(),
        "{:#?}",
        result.report().losses
    );
}

#[test]
fn type322_attribute_list_value_follows_the_declared_dialect() {
    let global_v4 = b"1H,,1H;,7Hproduct,8Hpart.igs,7Hcadmpeg,3H0.1,32,38,6,308,15,0H,1.0,2,2HMM,1,1.0,13H260714.000000,0.001,1000.0,6Hauthor,3Horg,6,0;";
    let global_v5 = b"1H,,1H;,7Hproduct,8Hpart.igs,7Hcadmpeg,3H0.1,32,38,6,308,15,0H,1.0,2,2HMM,1,1.0,13H260714.000000,0.001,1000.0,6Hauthor,3Horg,8,0,0H;";

    for (global, expected_version) in [(&global_v4[..], "4.0"), (&global_v5[..], "5.0")] {
        let result = IgesCodec
            .decode(
                &mut Cursor::new(owned_test_file_with_global(
                    &[OwnedTestEntity {
                        entity_type: 322,
                        form: 0,
                        label: "ATTRDEF".into(),
                        status: "00000000",
                        parameters: "322,4HMETA,5,1,10,1,1;".into(),
                    }],
                    global,
                )),
                &DecodeOptions::default(),
            )
            .unwrap();
        assert_eq!(
            result.report().dialects().unwrap().primary().declared()["effective_version"],
            expected_version
        );
        let definition = &result.ir().native.namespace("iges").unwrap().arenas()
            ["attribute_table_definitions"][0];
        assert_eq!(definition.fields().unwrap()["attribute_list_type"], 5);
        assert!(!result
            .report()
            .losses
            .iter()
            .any(|loss| loss.code == IgesLossCode::EntityNotProjected.kind()));
        assert_eq!(
            result
                .report()
                .losses
                .iter()
                .any(|loss| loss.code == IgesLossCode::EntityOutsideEnvelope.kind()),
            expected_version == "4.0"
        );
    }
}

#[test]
fn type322_accepts_no_value_and_not_used_data_types() {
    let global_v4 = b"1H,,1H;,7Hproduct,8Hpart.igs,7Hcadmpeg,3H0.1,32,38,6,308,15,0H,1.0,2,2HMM,1,1.0,13H260714.000000,0.001,1000.0,6Hauthor,3Horg,6,0;";
    let global_v5 = b"1H,,1H;,7Hproduct,8Hpart.igs,7Hcadmpeg,3H0.1,32,38,6,308,15,0H,1.0,2,2HMM,1,1.0,13H260714.000000,0.001,1000.0,6Hauthor,3Horg,8,0,0H;";
    let parameters = "322,3HROW,0,2,10,0,1,,11,5,1,,;";

    for global in [&global_v4[..], &global_v5[..]] {
        let result = IgesCodec
            .decode(
                &mut Cursor::new(owned_test_file_with_global(
                    &[OwnedTestEntity {
                        entity_type: 322,
                        form: 1,
                        label: "ATTRROW".into(),
                        status: "00000200",
                        parameters: parameters.into(),
                    }],
                    global,
                )),
                &DecodeOptions::default(),
            )
            .unwrap();
        let definition = &result.ir().native.namespace("iges").unwrap().arenas()
            ["attribute_table_definitions"][0];
        assert_eq!(
            definition.fields().unwrap()["attributes"][0]["value_data_type"],
            0
        );
        assert_eq!(
            definition.fields().unwrap()["attributes"][1]["value_data_type"],
            5
        );
        assert_eq!(
            definition.fields().unwrap()["attributes"][0]["values"][0]["value"]["kind"],
            "omitted"
        );
        assert_eq!(
            definition.fields().unwrap()["attributes"][1]["values"][0]["value"]["kind"],
            "omitted"
        );
        assert!(
            !result
                .report()
                .losses
                .iter()
                .any(|loss| loss.code == IgesLossCode::EntityNotProjected.kind()),
            "{:#?}",
            result.report().losses
        );
        assert_eq!(
            result
                .report()
                .losses
                .iter()
                .any(|loss| loss.code == IgesLossCode::EntityOutsideEnvelope.kind()),
            global == &global_v4[..]
        );
    }
}

#[test]
fn type322_requires_unique_bounded_attribute_types_in_v5() {
    let global_v5 = b"1H,,1H;,7Hproduct,8Hpart.igs,7Hcadmpeg,3H0.1,32,38,6,308,15,0H,1.0,2,2HMM,1,1.0,13H260714.000000,0.001,1000.0,6Hauthor,3Horg,8,0,0H;";

    for parameters in [
        "322,4HATTR,1,1,-1,1,1;",
        "322,4HATTR,1,1,10000,1,1;",
        "322,4HATTR,1,2,10,1,1,10,1,1;",
    ] {
        let result = IgesCodec
            .decode(
                &mut Cursor::new(owned_test_file_with_global(
                    &[OwnedTestEntity {
                        entity_type: 322,
                        form: 0,
                        label: "ATTRDEF".into(),
                        status: "00000200",
                        parameters: parameters.into(),
                    }],
                    global_v5,
                )),
                &DecodeOptions::default(),
            )
            .unwrap();
        assert!(
            result
                .report()
                .losses
                .iter()
                .any(|loss| loss.code == IgesLossCode::EntityNotProjected.kind()),
            "invalid Type 322 Attribute Type was admitted: {parameters}"
        );
    }
}

#[test]
fn decode_types_attribute_table_tuple_and_row_major_instances() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(attribute_instance_forms_file()),
            &DecodeOptions::default(),
        )
        .unwrap();
    let instances =
        &result.ir().native.namespace("iges").unwrap().arenas()["attribute_table_instances"];
    assert_eq!(instances.len(), 2);
    assert_eq!(
        instances[0].fields().unwrap()["definition"],
        "iges:product:attribute-definition#D1"
    );
    assert_eq!(
        instances[0].fields().unwrap()["rows"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(instances[1].fields().unwrap()["declared_row_count"], 2);
    assert_eq!(
        instances[1].fields().unwrap()["rows"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    assert_eq!(
        instances[1].fields().unwrap()["rows"][1][0]["kind"],
        "integer"
    );
    assert_eq!(
        instances[1].fields().unwrap()["rows"][1][1]["kind"],
        "string"
    );
    assert!(
        result.report().losses.is_empty(),
        "{:#?}",
        result.report().losses
    );
}

#[test]
fn decode_ignores_nonnegative_attribute_instance_structure_values() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(attribute_instance_ignored_structures_file()),
            &DecodeOptions::default(),
        )
        .unwrap();
    let native = result.ir().native.namespace("iges").unwrap();
    let instances = &native.arenas()["attribute_table_instances"];

    assert_eq!(instances.len(), 2);
    for instance in instances {
        assert!(instance.fields().unwrap()["definition"].is_null());
        assert!(instance.fields().unwrap()["rows"]
            .as_array()
            .unwrap()
            .is_empty());
    }
    for sequence in [3, 5] {
        let entity = native.arenas()["entities"]
            .iter()
            .find(|entity| entity.id() == format!("iges:entity:directory#{sequence}"))
            .unwrap();
        assert!(entity.fields().unwrap()["references"]
            .as_array()
            .unwrap()
            .is_empty());
    }
}

#[test]
fn decode_validates_structure_targets_by_source_entity() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(structure_target_rules_file()),
            &DecodeOptions::default(),
        )
        .unwrap();
    let entities = &result.ir().native.namespace("iges").unwrap().arenas()["entities"];
    let reference = |sequence: u32| {
        let entity = entities
            .iter()
            .find(|entity| entity.id() == format!("iges:entity:directory#{sequence}"))
            .unwrap();
        entity.fields().unwrap()["references"][0].clone()
    };

    let attribute = reference(3);
    assert_eq!(attribute["resolution"], "wrong_type");
    assert_eq!(attribute["expected"], "type-322-form-0");
    let associativity = reference(7);
    assert_eq!(associativity["resolution"], "resolved");
    assert_eq!(associativity["expected"], "type-302-matching-form");
    let wrong_associativity = reference(11);
    assert_eq!(wrong_associativity["resolution"], "wrong_type");
    let macro_instance = reference(15);
    assert_eq!(macro_instance["resolution"], "resolved");
    assert_eq!(macro_instance["expected"], "type-306-or-type-416");
    let wrong_owner = reference(17);
    assert_eq!(wrong_owner["resolution"], "wrong_type");
    assert_eq!(wrong_owner["expected"], "structure-not-permitted");

    let reference_losses = result
        .report()
        .losses
        .iter()
        .filter(|loss| loss.code == IgesLossCode::PointerUnresolved.kind())
        .collect::<Vec<_>>();
    assert_eq!(reference_losses.len(), 3);
    assert!(reference_losses.iter().all(|loss| {
        loss.provenance.as_ref().is_some_and(|provenance| {
            provenance
                .tag
                .as_deref()
                .is_some_and(|tag| tag.starts_with('D'))
        })
    }));

    let attribute_instance = result.ir().native.namespace("iges").unwrap().arenas()
        ["attribute_table_instances"]
        .first()
        .unwrap();
    assert!(attribute_instance.fields().unwrap()["definition"].is_null());
}

#[test]
fn decode_links_product_names_and_reference_designators_to_owners() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(product_property_file()),
            &DecodeOptions::default(),
        )
        .unwrap();
    let properties = &result.ir().native.namespace("iges").unwrap().arenas()["product_properties"];
    assert_eq!(properties.len(), 2);
    assert_eq!(
        properties[0].fields().unwrap()["property_kind"],
        "reference_designator"
    );
    assert_eq!(
        properties[0].fields().unwrap()["owners"][0],
        "iges:entity:directory#1"
    );
    assert_eq!(properties[1].fields().unwrap()["property_kind"], "name");
    assert_eq!(properties[1].fields().unwrap()["value"][0], 66);
    assert_eq!(
        properties[1].fields().unwrap()["owners"][0],
        "iges:entity:directory#1"
    );
    let owner = &result.ir().native.namespace("iges").unwrap().arenas()["entities"][0];
    assert!(owner.fields().unwrap()["association_links"]
        .as_array()
        .unwrap()
        .is_empty());
    assert_eq!(
        owner.fields().unwrap()["property_links"][0],
        "iges:entity:directory#3"
    );
    assert_eq!(
        owner.fields().unwrap()["property_links"][1],
        "iges:entity:directory#5"
    );
    assert!(
        result.report().losses.is_empty(),
        "{:#?}",
        result.report().losses
    );
}

#[test]
fn decode_types_scalar_and_string_property_forms() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(scalar_property_forms_file()),
            &DecodeOptions::default(),
        )
        .unwrap();
    let properties = &result.ir().native.namespace("iges").unwrap().arenas()["properties"];
    assert_eq!(properties.len(), 15);
    assert!(properties
        .iter()
        .all(|property| property.id().starts_with("iges:application:property#D")));
    let property = |form| {
        properties
            .iter()
            .find(|property| property.fields().unwrap()["form"] == form)
            .unwrap()
    };
    assert_eq!(
        property(2).fields().unwrap()["property_kind"],
        "region_restriction"
    );
    assert_eq!(property(2).fields().unwrap()["electrical_circuitry"], 2);
    assert_eq!(
        property(4).fields().unwrap()["property_kind"],
        "region_fill"
    );
    assert_eq!(property(4).fields().unwrap()["fill_code"], 1);
    assert_eq!(property(4).fields().unwrap()["obsolete_pointer"], 0);
    assert_eq!(property(5).fields().unwrap()["extension_flag"], 2);
    assert_eq!(property(6).fields().unwrap()["lower_layer"], 2);
    assert_eq!(property(6).fields().unwrap()["upper_layer"], 8);
    assert_eq!(
        property(12).fields().unwrap()["names"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    assert_eq!(property(13).fields().unwrap()["standard"][0], 65);
    assert_eq!(property(18).fields().unwrap()["percent"], 12.5);
    assert_eq!(property(20).fields().unwrap()["highlighted"], true);
    assert_eq!(property(21).fields().unwrap()["pickable"], true);
    assert_eq!(property(10).fields().unwrap()["line_font"], 1);
    assert_eq!(property(10).fields().unwrap()["view"], 0);
    assert_eq!(property(10).fields().unwrap()["level"], 1);
    assert_eq!(property(10).fields().unwrap()["blank"], 0);
    assert_eq!(property(10).fields().unwrap()["line_weight"], 1);
    assert_eq!(property(10).fields().unwrap()["color"], 0);
    assert!(
        result.report().losses.is_empty(),
        "{:#?}",
        result.report().losses
    );
}

#[test]
fn decode_admits_v4_region_fill_property() {
    const GLOBAL_V4: &[u8] = b"1H,,1H;,7Hproduct,8Hpart.igs,7Hcadmpeg,3H0.1,32,38,6,308,15,7Hproduct,1.0,2,2HMM,1,1.0,13H260714.000000,0.001,1000.0,6Hauthor,3Horg,6,0;";
    let result = IgesCodec
        .decode(
            &mut Cursor::new(owned_test_file_with_global(
                &[OwnedTestEntity {
                    entity_type: 406,
                    form: 4,
                    label: "REGFILL".into(),
                    status: "00000000",
                    parameters: "406,2,1,0;".into(),
                }],
                GLOBAL_V4,
            )),
            &DecodeOptions::default(),
        )
        .unwrap();
    let property = &result.ir().native.namespace("iges").unwrap().arenas()["properties"][0];
    assert_eq!(property.fields().unwrap()["form"], 4);
    assert_eq!(property.fields().unwrap()["property_kind"], "region_fill");
    assert_eq!(property.fields().unwrap()["fill_code"], 1);
    assert_eq!(property.fields().unwrap()["obsolete_pointer"], 0);
    assert!(result.report().losses.is_empty(), "{:#?}", result.report());
}

#[test]
fn decode_rejects_descending_drilled_hole_layer_range() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(invalid_drilled_hole_layer_order_file()),
            &DecodeOptions::default(),
        )
        .unwrap();
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
        Some("directory_entry:D1")
    );
}

#[test]
fn decode_rejects_nonstandard_type406_form19_pattern_code() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(owned_test_file(&[OwnedTestEntity {
                entity_type: 406,
                form: 19,
                label: "LFPC".into(),
                status: "00000000",
                parameters: "406,1,13;".into(),
            }])),
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
fn decode_rejects_unknown_type406_form24_functional_level_identifier() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(owned_test_file(&[OwnedTestEntity {
                entity_type: 406,
                form: 24,
                label: "LAYERMAP".into(),
                status: "00000000",
                parameters: "406,5,1,1,3HTOP,0,8HSIGNAL_C;".into(),
            }])),
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
fn decode_accepts_equal_drilled_hole_layer_range() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(equal_drilled_hole_layer_range_file()),
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
fn decode_types_grid_group_and_lep_property_forms() {
    let decode = |bytes| {
        IgesCodec
            .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
            .unwrap()
    };
    let grid = decode(grid_property_file());
    let property = &grid.ir().native.namespace("iges").unwrap().arenas()["properties"][0];
    assert_eq!(
        property.fields().unwrap()["property_kind"],
        "uniform_rectangular_grid"
    );
    assert_eq!(
        property.fields().unwrap()["owners"][0],
        "iges:entity:directory#5"
    );
    assert!(
        grid.report().losses.is_empty(),
        "{:#?}",
        grid.report().losses
    );

    let group = decode(group_type_property_file());
    let property = &group.ir().native.namespace("iges").unwrap().arenas()["properties"][0];
    assert_eq!(property.fields().unwrap()["associativity_type"], 5);
    assert_eq!(
        property.fields().unwrap()["owners"][0],
        "iges:entity:directory#3"
    );
    assert!(
        group.report().losses.is_empty(),
        "{:#?}",
        group.report().losses
    );

    let lep = decode(lep_property_forms_file());
    let properties = &lep.ir().native.namespace("iges").unwrap().arenas()["properties"];
    let property = |form| {
        properties
            .iter()
            .find(|value| value.fields().unwrap()["form"] == form)
            .unwrap()
    };
    assert_eq!(
        property(24).fields().unwrap()["definitions"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    assert_eq!(
        property(25).fields().unwrap()["levels"]
            .as_array()
            .unwrap()
            .len(),
        3
    );
    assert_eq!(property(26).fields().unwrap()["function_code"], 5);
    assert_eq!(
        property(26).fields().unwrap()["owners"][0],
        "iges:entity:directory#5"
    );
    assert!(lep.report().losses.is_empty(), "{:#?}", lep.report().losses);
}

#[test]
fn decode_types_tabular_and_generic_data_properties() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(variable_schema_property_forms_file()),
            &DecodeOptions::default(),
        )
        .unwrap();
    let properties = &result.ir().native.namespace("iges").unwrap().arenas()["properties"];
    let property = |form| {
        properties
            .iter()
            .find(|value| value.fields().unwrap()["form"] == form)
            .unwrap()
    };
    assert_eq!(
        property(11).fields().unwrap()["property_kind"],
        "tabular_data"
    );
    assert_eq!(
        property(11).fields().unwrap()["independent_variables"][0]["values"][1],
        25.0
    );
    assert_eq!(property(11).fields().unwrap()["dependent_values"][1], 46.0);
    assert_eq!(
        property(27).fields().unwrap()["values"]
            .as_array()
            .unwrap()
            .len(),
        6
    );
    assert_eq!(
        property(27).fields().unwrap()["values"][4]["value"]["kind"],
        "integer"
    );
    assert_eq!(
        property(27).fields().unwrap()["owners"][0],
        "iges:entity:directory#1"
    );
    assert!(
        result.report().losses.is_empty(),
        "{:#?}",
        result.report().losses
    );
}

#[test]
fn decode_types_dimension_drawing_text_and_closure_properties() {
    let decode = |bytes| {
        IgesCodec
            .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
            .unwrap()
    };
    let dimensions = decode(dimension_property_forms_file());
    let properties = &dimensions.ir().native.namespace("iges").unwrap().arenas()["properties"];
    let property = |form| {
        properties
            .iter()
            .find(|property| property.fields().unwrap()["form"] == form)
            .expect("dimension property form exists")
            .fields()
            .unwrap()
    };
    let units = property(28);
    assert_eq!(units["property_kind"], "dimension_units");
    assert_eq!(units["secondary_position"], 0);
    assert_eq!(units["units_indicator"], 2);
    assert_eq!(units["character_set"], 1);
    assert_eq!(units["suffix"], serde_json::json!([77, 77]));
    assert_eq!(units["fraction_flag"], 0);
    assert_eq!(units["precision"], 3);
    let tolerance = property(29);
    assert_eq!(tolerance["property_kind"], "dimension_tolerance");
    assert_eq!(tolerance["secondary_flag"], 0);
    assert_eq!(tolerance["tolerance_type"], 2);
    assert_eq!(tolerance["placement"], 2);
    assert_eq!(tolerance["upper"], 0.1);
    assert_eq!(tolerance["lower"], -0.1);
    assert_eq!(tolerance["suppress_plus"], false);
    assert_eq!(tolerance["fraction_flag"], 0);
    assert_eq!(tolerance["precision"], 3);
    let display = property(30);
    assert_eq!(display["property_kind"], "dimension_display_data");
    assert_eq!(display["dimension_type"], 2);
    assert_eq!(display["label_position"], 1);
    assert_eq!(display["declared_character_set"], 1);
    assert_eq!(display["character_set"], 1);
    assert_eq!(display["label"], serde_json::json!([68, 73, 65]));
    assert_eq!(display["decimal_symbol"], 0);
    assert_eq!(
        display["declared_witness_line_angle"],
        std::f64::consts::FRAC_PI_2
    );
    assert_eq!(display["witness_line_angle"], std::f64::consts::FRAC_PI_2);
    assert_eq!(display["text_alignment"], 1);
    assert_eq!(display["text_level"], 0);
    assert_eq!(display["text_placement"], 0);
    assert_eq!(display["arrow_orientation"], 0);
    assert_eq!(display["initial_value"], 12.5);
    assert_eq!(display["supplemental_notes"][0]["position"], 1);
    assert_eq!(display["supplemental_notes"][0]["first_text"], 1);
    assert_eq!(display["supplemental_notes"][0]["last_text"], 1);
    let basic = property(31);
    assert_eq!(basic["property_kind"], "basic_dimension");
    assert_eq!(
        basic["corners"],
        serde_json::json!([[0.0, 0.0], [2.0, 0.0], [2.0, 1.0], [0.0, 1.0]])
    );
    assert!(
        dimensions.report().losses.is_empty(),
        "{:#?}",
        dimensions.report().losses
    );

    let drawing = decode(drawing_metadata_property_forms_file());
    let properties = &drawing.ir().native.namespace("iges").unwrap().arenas()["properties"];
    let approval = properties
        .iter()
        .find(|property| property.fields().unwrap()["form"] == 32)
        .expect("approval property")
        .fields()
        .unwrap();
    assert_eq!(approval["property_kind"], "drawing_sheet_approval");
    assert_eq!(approval["name"], serde_json::json!([74, 65, 78, 69]));
    assert_eq!(approval["organization"], serde_json::json!([69, 78, 71]));
    assert_eq!(approval["date"], serde_json::json!(b"20260714.123456"));
    let sheet = properties
        .iter()
        .find(|property| property.fields().unwrap()["form"] == 33)
        .expect("sheet-id property")
        .fields()
        .unwrap();
    assert_eq!(sheet["property_kind"], "drawing_sheet_id");
    assert_eq!(sheet["sheet_number"], 2);
    assert_eq!(sheet["revision"], serde_json::json!([67]));
    assert!(
        drawing.report().losses.is_empty(),
        "{:#?}",
        drawing.report().losses
    );

    let scores = decode(text_score_property_forms_file());
    let properties = &scores.ir().native.namespace("iges").unwrap().arenas()["properties"];
    for (form, kind, first, last) in [(34, "underscore", 2, 4), (35, "overscore", 3, 5)] {
        let property = properties
            .iter()
            .find(|property| property.fields().unwrap()["form"] == form)
            .expect("text-score property")
            .fields()
            .unwrap();
        assert_eq!(property["property_kind"], kind);
        assert_eq!(property["ranges"][0]["text_index"], 1);
        assert_eq!(property["ranges"][0]["first_character"], first);
        assert_eq!(property["ranges"][0]["last_character"], last);
    }
    assert!(
        scores.report().losses.is_empty(),
        "{:#?}",
        scores.report().losses
    );

    let closure = decode(closure_property_file());
    let property = closure.ir().native.namespace("iges").unwrap().arenas()["properties"][0]
        .fields()
        .unwrap();
    assert_eq!(property["property_kind"], "closure");
    assert_eq!(property["u"], 0);
    assert_eq!(property["v"], 1);
    assert!(
        closure.report().losses.is_empty(),
        "{:#?}",
        closure.report().losses
    );
}

#[test]
fn decode_preserves_property_defaults_without_coercing_non_boolean_flags() {
    let bytes = owned_test_file(&[
        OwnedTestEntity {
            entity_type: 406,
            form: 20,
            label: "HILITE".into(),
            status: "00000000",
            parameters: "406,1,2;".into(),
        },
        OwnedTestEntity {
            entity_type: 406,
            form: 21,
            label: "PICK".into(),
            status: "00000000",
            parameters: "406,1,-1;".into(),
        },
        OwnedTestEntity {
            entity_type: 406,
            form: 22,
            label: "GRID".into(),
            status: "00000000",
            parameters: "406,9,2,2,2,0,0,1,1,1,1;".into(),
        },
        OwnedTestEntity {
            entity_type: 406,
            form: 29,
            label: "TOL".into(),
            status: "00000000",
            parameters: "406,8,0,2,2,0.1,-0.1,2,0,3;".into(),
        },
        OwnedTestEntity {
            entity_type: 406,
            form: 30,
            label: "DISPLAY".into(),
            status: "00000000",
            parameters: "406,14,2,1,,3HDIA,0,,1,0,0,0,12.5,1,1,1,1;".into(),
        },
    ]);
    let result = IgesCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    let properties = &result.ir().native.namespace("iges").unwrap().arenas()["properties"];
    let property = |form| {
        properties
            .iter()
            .find(|property| property.fields().unwrap()["form"] == form)
            .unwrap()
    };

    assert!(property(20).fields().unwrap()["highlighted"].is_null());
    assert!(property(21).fields().unwrap()["pickable"].is_null());
    assert!(property(22).fields().unwrap()["finite"].is_null());
    assert!(property(22).fields().unwrap()["lines"].is_null());
    assert!(property(22).fields().unwrap()["weighted"].is_null());
    assert!(property(29).fields().unwrap()["suppress_plus"].is_null());
    let display = property(30).fields().unwrap();
    assert!(display["declared_character_set"].is_null());
    assert_eq!(display["character_set"], 1);
    assert!(display["declared_witness_line_angle"].is_null());
    assert_eq!(display["witness_line_angle"], std::f64::consts::FRAC_PI_2);
}

#[test]
fn decode_preserves_implementor_associativity_class_grammar() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(associativity_definition_file()),
            &DecodeOptions::default(),
        )
        .unwrap();
    let definition = &result.ir().native.namespace("iges").unwrap().arenas()["associativities"][0];
    assert_eq!(definition.fields().unwrap()["kind"], "definition");
    assert_eq!(definition.fields().unwrap()["associativity_form"], 5001);
    assert_eq!(
        definition.fields().unwrap()["classes"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    assert_eq!(
        definition.fields().unwrap()["classes"][0]["back_pointers_required"],
        true
    );
    assert_eq!(definition.fields().unwrap()["classes"][0]["ordered"], true);
    assert_eq!(
        definition.fields().unwrap()["classes"][0]["item_types"][0],
        1
    );
    assert_eq!(
        definition.fields().unwrap()["classes"][0]["item_types"][1],
        2
    );
    assert_eq!(
        definition.fields().unwrap()["classes"][1]["back_pointers_required"],
        false
    );
    assert_eq!(definition.fields().unwrap()["classes"][1]["ordered"], false);
    assert_eq!(
        definition.fields().unwrap()["classes"][1]["item_types"][0],
        3
    );
    assert!(
        result.report().losses.is_empty(),
        "{:#?}",
        result.report().losses
    );
}

#[test]
fn decode_types_bounded_predefined_associativity_roles() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(bounded_associativity_forms_file()),
            &DecodeOptions::default(),
        )
        .unwrap();
    let associativities =
        &result.ir().native.namespace("iges").unwrap().arenas()["associativities"];
    assert_eq!(associativities.len(), 6);
    let parent = associativities
        .iter()
        .find(|value| value.fields().unwrap()["kind"] == "single_parent")
        .unwrap();
    assert_eq!(
        parent.fields().unwrap()["parent"],
        "iges:entity:directory#9"
    );
    assert_eq!(parent.fields().unwrap()["declared_child_count"], 1);
    assert_eq!(
        parent.fields().unwrap()["children"][0],
        "iges:entity:directory#11"
    );
    let labels = associativities
        .iter()
        .find(|value| value.fields().unwrap()["kind"] == "label_display")
        .unwrap();
    assert_eq!(
        labels.fields().unwrap()["placements"][0]["view"],
        "iges:entity:directory#1"
    );
    assert_eq!(
        labels.fields().unwrap()["placements"][0]["text_location"][2],
        3.0
    );
    assert_eq!(
        labels.fields().unwrap()["placements"][0]["leader"],
        "iges:entity:directory#3"
    );
    let dimension = associativities
        .iter()
        .find(|value| value.fields().unwrap()["kind"] == "dimensioned_geometry")
        .unwrap();
    assert_eq!(
        dimension.fields().unwrap()["dimension"],
        "iges:entity:directory#21"
    );
    assert_eq!(dimension.fields().unwrap()["declared_geometry_count"], 1);
    assert_eq!(
        dimension.fields().unwrap()["geometry"][0],
        "iges:entity:directory#9"
    );
    let planar = associativities
        .iter()
        .find(|value| value.fields().unwrap()["kind"] == "planar")
        .unwrap();
    assert!(planar.fields().unwrap()["plane_transform"].is_null());
    assert_eq!(planar.fields().unwrap()["declared_entity_count"], 2);
    assert_eq!(
        planar.fields().unwrap()["entities"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    let external_index = associativities
        .iter()
        .find(|value| value.fields().unwrap()["kind"] == "external_reference_index")
        .unwrap();
    assert_eq!(external_index.fields().unwrap()["declared_count"], 1);
    assert_eq!(
        external_index.fields().unwrap()["entries"][0]["symbolic_name"],
        serde_json::json!([78, 65, 77, 69])
    );
    assert_eq!(
        external_index.fields().unwrap()["entries"][0]["entity"],
        "iges:entity:directory#9"
    );
    assert!(
        result.report().losses.is_empty(),
        "{:#?}",
        result.report().losses
    );
}

#[test]
fn decode_preserves_external_logical_reference_index_in_v4_and_v5_profiles() {
    let global_v4 = b"1H,,1H;,7Hproduct,8Hpart.igs,7Hcadmpeg,3H0.1,32,38,6,308,15,0H,1.0,2,2HMM,1,1.0,13H260714.000000,0.001,1000.0,6Hauthor,3Horg,6,0;";
    let global_v5 = b"1H,,1H;,7Hproduct,8Hpart.igs,7Hcadmpeg,3H0.1,32,38,6,308,15,0H,1.0,2,2HMM,1,1.0,13H260714.000000,0.001,1000.0,6Hauthor,3Horg,8,0,0H;";
    for (version, global) in [("4.0", &global_v4[..]), ("5.0", &global_v5[..])] {
        let result = IgesCodec
            .decode(
                &mut Cursor::new(bounded_associativity_forms_file_with_global(global)),
                &DecodeOptions::default(),
            )
            .unwrap();
        let external_index = result.ir().native.namespace("iges").unwrap().arenas()
            ["associativities"]
            .iter()
            .find(|value| value.fields().unwrap()["kind"] == "external_reference_index")
            .unwrap_or_else(|| panic!("missing Type 402 Form 2 in IGES {version}"));
        assert_eq!(
            external_index.fields().unwrap()["declared_count"],
            1,
            "IGES {version}"
        );
        assert_eq!(
            external_index.fields().unwrap()["entries"][0]["entity"],
            "iges:entity:directory#9",
            "IGES {version}"
        );
        assert!(
            result
                .report()
                .losses
                .iter()
                .all(|loss| { !loss.message.contains("IGES entity type 402 form 2") }),
            "IGES {version}: {:#?}",
            result.report().losses
        );
    }
}

#[test]
fn decode_projects_legacy_single_parent_plane_holes_in_v4_and_v5_profiles() {
    let global_v4 = b"1H,,1H;,7Hproduct,8Hpart.igs,7Hcadmpeg,3H0.1,32,38,6,308,15,0H,1.0,2,2HMM,1,1.0,13H260714.000000,0.001,1000.0,6Hauthor,3Horg,6,0;";
    let global_v5 = b"1H,,1H;,7Hproduct,8Hpart.igs,7Hcadmpeg,3H0.1,32,38,6,308,15,0H,1.0,2,2HMM,1,1.0,13H260714.000000,0.001,1000.0,6Hauthor,3Horg,8,0,0H;";
    for (version, global) in [("4.0", &global_v4[..]), ("5.0", &global_v5[..])] {
        let result = IgesCodec
            .decode(
                &mut Cursor::new(legacy_perforated_plane_file(global)),
                &DecodeOptions::default(),
            )
            .unwrap();
        assert_eq!(
            result.ir().model.faces.len(),
            1,
            "IGES {version}: {:#?}",
            result.report().losses
        );
        let face = &result.ir().model.faces[0];
        assert_eq!(
            face.surface,
            "iges:model:surface#D1".try_into().expect("valid identity")
        );
        assert_eq!(face.loops.len(), 2, "IGES {version}");
        let loop_roles = face
            .loops
            .iter()
            .map(|loop_id| face.loop_role(loop_id))
            .collect::<Vec<_>>();
        assert_eq!(
            loop_roles,
            vec![
                cadmpeg_ir::topology::LoopBoundaryRole::Outer,
                cadmpeg_ir::topology::LoopBoundaryRole::Inner,
            ],
            "IGES {version}"
        );
        assert!(result.report().losses.iter().all(|loss| {
            loss.code != IgesLossCode::EntityNotProjected.kind()
                || !loss.message.contains("IGES entity type 402 form 9")
        }));
        assert!(
            cadmpeg_ir::validate_neutral(result.ir(), Vec::new()).is_ok(),
            "IGES {version} legacy hole topology is invalid"
        );
    }
}

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
            .find(|value| value.fields().unwrap()["kind"] == "single_parent")
            .expect("generic single-parent association");
        assert_eq!(
            association.fields().unwrap()["parent"],
            "iges:entity:directory#1"
        );
        assert_eq!(
            association.fields().unwrap()["children"][0],
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
            .find(|value| value.fields().unwrap()["kind"] == "dimensioned_geometry")
            .expect("legacy dimensioned-geometry association");
        assert_eq!(
            association.fields().unwrap()["dimension"],
            "iges:entity:directory#21"
        );
        assert_eq!(
            association.fields().unwrap()["geometry"][0],
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
        .find(|associativity| associativity.fields().unwrap()["kind"] == "label_display")
        .unwrap();
    assert!(label_display.fields().unwrap()["placements"][0]["leader"].is_null());
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
            value.fields().unwrap()["kind"] == "flow"
                && value.fields().unwrap()["form"] == 18
                && value.fields().unwrap()["connections"]
                    .as_array()
                    .unwrap()
                    .len()
                    == 1
        })
        .unwrap();
    assert_eq!(signal.fields().unwrap()["type_flag"], 1);
    assert_eq!(
        signal.fields().unwrap()["declared_associated_flow_count"],
        0
    );
    assert_eq!(signal.fields().unwrap()["declared_connection_count"], 1);
    assert_eq!(signal.fields().unwrap()["declared_join_count"], 1);
    assert_eq!(signal.fields().unwrap()["declared_name_count"], 1);
    assert_eq!(signal.fields().unwrap()["declared_name_display_count"], 1);
    assert_eq!(signal.fields().unwrap()["declared_continuation_count"], 1);
    assert_eq!(signal.fields().unwrap()["function_flag"], 2);
    assert_eq!(
        signal.fields().unwrap()["connections"][0],
        "iges:entity:directory#1"
    );
    assert_eq!(
        signal.fields().unwrap()["joins"][0],
        "iges:entity:directory#3"
    );
    assert_eq!(signal.fields().unwrap()["names"][0][0], 70);
    assert_eq!(
        signal.fields().unwrap()["name_displays"][0],
        "iges:entity:directory#5"
    );
    assert_eq!(
        signal.fields().unwrap()["continuations"][0],
        "iges:entity:directory#9"
    );
    let pipe = associativities
        .iter()
        .find(|value| {
            value.fields().unwrap()["kind"] == "flow"
                && value.fields().unwrap()["form"] == 20
                && value.fields().unwrap()["connections"]
                    .as_array()
                    .unwrap()
                    .len()
                    == 1
        })
        .unwrap();
    assert_eq!(pipe.fields().unwrap()["type_flag"], 2);
    assert_eq!(pipe.fields().unwrap()["declared_associated_flow_count"], 0);
    assert_eq!(pipe.fields().unwrap()["declared_connection_count"], 1);
    assert_eq!(pipe.fields().unwrap()["declared_join_count"], 1);
    assert_eq!(pipe.fields().unwrap()["declared_name_count"], 1);
    assert_eq!(pipe.fields().unwrap()["declared_name_display_count"], 0);
    assert_eq!(pipe.fields().unwrap()["declared_continuation_count"], 1);
    assert!(pipe.fields().unwrap()["function_flag"].is_null());
    assert_eq!(
        pipe.fields().unwrap()["connections"][0],
        "iges:entity:directory#11"
    );
    assert_eq!(
        pipe.fields().unwrap()["continuations"][0],
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
        .find(|value| value.fields().unwrap()["kind"] == "flow")
        .unwrap();
    assert!(flow.fields().unwrap()["joins"][0].is_null());
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
        .find(|value| value.fields().unwrap()["kind"] == "legacy_signal_string")
        .unwrap();
    assert_eq!(signal.fields().unwrap()["declared_signal_name_count"], 1);
    assert_eq!(signal.fields().unwrap()["declared_connection_count"], 1);
    assert_eq!(signal.fields().unwrap()["declared_schematic_count"], 1);
    assert_eq!(signal.fields().unwrap()["declared_physical_count"], 1);
    assert_eq!(
        signal.fields().unwrap()["signal_names"][0],
        serde_json::json!([78, 69, 84])
    );
    assert_eq!(
        signal.fields().unwrap()["connections"][0],
        "iges:entity:directory#3"
    );
    assert_eq!(
        signal.fields().unwrap()["schematic_entities"][0],
        "iges:entity:directory#11"
    );
    assert_eq!(
        signal.fields().unwrap()["physical_entities"][0],
        "iges:entity:directory#11"
    );

    let text = associativities
        .iter()
        .find(|value| value.fields().unwrap()["kind"] == "legacy_text_node")
        .unwrap();
    assert_eq!(text.fields().unwrap()["declared_geometry_count"], 1);
    assert_eq!(text.fields().unwrap()["declared_text_description_count"], 1);
    assert_eq!(
        text.fields().unwrap()["geometry"][0],
        "iges:entity:directory#5"
    );
    assert_eq!(text.fields().unwrap()["box_width"], 1.0);
    assert_eq!(text.fields().unwrap()["box_height"], 2.0);
    assert_eq!(text.fields().unwrap()["font_characteristic"], 1);
    assert!(
        (text.fields().unwrap()["slant_angle"].as_f64().unwrap() - std::f64::consts::FRAC_PI_2)
            .abs()
            <= LEGACY_TEXT_ANGLE_TOLERANCE
    );
    assert_eq!(text.fields().unwrap()["rotation_angle"], 0.0);
    assert_eq!(text.fields().unwrap()["mirror_flag"], 0);
    assert_eq!(text.fields().unwrap()["rotate_internal_flag"], 0);

    let connect = associativities
        .iter()
        .find(|value| value.fields().unwrap()["kind"] == "legacy_connect_node")
        .unwrap();
    assert_eq!(connect.fields().unwrap()["declared_point_count"], 1);
    assert_eq!(connect.fields().unwrap()["declared_data_count"], 2);
    assert_eq!(
        connect.fields().unwrap()["points"][0],
        "iges:entity:directory#1"
    );
    assert_eq!(connect.fields().unwrap()["data"][0]["kind"], "string");
    assert_eq!(
        connect.fields().unwrap()["data"][0]["value"],
        serde_json::json!([67, 79, 78, 83, 84, 82])
    );
    assert_eq!(connect.fields().unwrap()["data"][1]["kind"], "integer");
    assert_eq!(connect.fields().unwrap()["data"][1]["value"], 42);
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
                .any(|value| value.fields().unwrap()["kind"] == kind),
            "missing V4 associativity kind {kind}"
        );
    }
    assert!(result.report().losses.is_empty(), "{:#?}", result.report());
}

mod legacy_and_network;
