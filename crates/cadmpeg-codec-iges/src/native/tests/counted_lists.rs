// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeOptions};

use super::{assert_overdeclared_contract, code_count};
use crate::loss::IgesLossCode;
use crate::test_support::*;
use crate::IgesCodec;

#[test]
fn decode_bounds_declared_attribute_counts_by_record_tokens() {
    let bytes = owned_test_file(&[OwnedTestEntity {
        entity_type: 322,
        form: 0,
        label: "BADCOUNT".into(),
        status: "00000200",
        parameters: "322,,0,9223372036854775807;".into(),
    }]);
    let result = IgesCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();

    let definitions =
        &result.ir().native.namespace("iges").unwrap().arenas["attribute_table_definitions"];
    assert_eq!(definitions.len(), 1);
    assert!(definitions[0].fields()["attributes"]
        .as_array()
        .unwrap()
        .is_empty());
    assert!(result
        .report()
        .losses
        .iter()
        .any(|loss| loss.message.contains("attribute-table definition")));
}

#[test]
fn decode_stops_cursor_records_after_an_overlong_nested_count() {
    let bytes = owned_test_file(&[
        OwnedTestEntity {
            entity_type: 310,
            form: 0,
            label: "FONTCNT".into(),
            status: "00000200",
            parameters: "310,1,1HA,0,1,2,65,0,0,99,66,0,0,0;".into(),
        },
        OwnedTestEntity {
            entity_type: 302,
            form: 0,
            label: "CLASSCNT".into(),
            status: "00000200",
            parameters: "302,2,0,0,99,1,1,0;".into(),
        },
        OwnedTestEntity {
            entity_type: 322,
            form: 1,
            label: "ATTRCNT".into(),
            status: "00000200",
            parameters: "322,4HATTR,0,2,1,1,99,2,3,1,42;".into(),
        },
        OwnedTestEntity {
            entity_type: 322,
            form: 2,
            label: "ATTRPAIR".into(),
            status: "00000200",
            parameters: "322,4HPAIR,0,1,1,1,2,10,0,20;".into(),
        },
    ]);
    let result = IgesCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    let native = result.ir().native.namespace("iges").unwrap();

    let characters = native.arenas["text_fonts"][0].fields()["characters"]
        .as_array()
        .unwrap()
        .clone();
    assert_eq!(characters.len(), 0);
    assert_eq!(
        native.arenas["text_fonts"][0].fields()["declared_character_count"],
        2
    );

    let classes = native.arenas["associativities"][0].fields()["classes"]
        .as_array()
        .unwrap()
        .clone();
    assert_eq!(classes.len(), 0);
    assert_eq!(
        native.arenas["associativities"][0].fields()["declared_class_count"],
        2
    );

    let definitions = &native.arenas["attribute_table_definitions"];
    for definition in definitions {
        let attributes = definition.fields()["attributes"]
            .as_array()
            .unwrap()
            .clone();
        assert!(attributes.is_empty());
    }
    assert_eq!(definitions[0].fields()["declared_attribute_count"], 2);
    assert_eq!(definitions[1].fields()["declared_attribute_count"], 1);
}

#[test]
fn decode_native_counted_lists_do_not_expose_partial_prefixes() {
    let bytes = owned_test_file(&[
        OwnedTestEntity {
            entity_type: 184,
            form: 0,
            label: "ASSEMBLY".into(),
            status: "00000200",
            parameters: "184,2,1,3;".into(),
        },
        OwnedTestEntity {
            entity_type: 320,
            form: 0,
            label: "NETWORK".into(),
            status: "00000200",
            parameters: "320,0,3HNET,99,1,2HR1,0,0;".into(),
        },
        OwnedTestEntity {
            entity_type: 106,
            form: 1,
            label: "COPIOUS".into(),
            status: "00000000",
            parameters: "106,1,3,0.5,1,2,3;".into(),
        },
        OwnedTestEntity {
            entity_type: 406,
            form: 11,
            label: "TABULAR".into(),
            status: "00000200",
            parameters: "406,7,5,1,1,3,10,20;".into(),
        },
        OwnedTestEntity {
            entity_type: 406,
            form: 24,
            label: "LAYERS".into(),
            status: "00000200",
            parameters: "406,6,2,1,1HA,1,1HB;".into(),
        },
    ]);
    let result = IgesCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    let native = result.ir().native.namespace("iges").unwrap();

    let assembly = &native.arenas["solid_assemblies"][0];
    assert_eq!(assembly.fields()["declared_count"], 2);
    assert!(assembly.fields()["items"].as_array().unwrap().is_empty());

    let network = &native.arenas["network_definitions"][0];
    assert_eq!(network.fields()["declared_member_count"], 99);
    assert!(network.fields()["members"].as_array().unwrap().is_empty());
    assert!(network.fields()["type_flag"].is_null());
    assert!(network.fields()["primary_reference_designator"].is_null());
    assert!(network.fields()["display_template"].is_null());
    assert!(network.fields()["declared_connect_point_count"].is_null());
    assert!(network.fields()["connect_points"]
        .as_array()
        .unwrap()
        .is_empty());

    let copious = &native.arenas["copious_data"][0];
    assert_eq!(copious.fields()["declared_tuple_count"], 3);
    assert!(copious.fields()["common_z"].is_number());
    assert!(copious.fields()["tuples"].as_array().unwrap().is_empty());

    let property = |form| {
        native.arenas["properties"]
            .iter()
            .find(|property| property.fields()["form"] == form)
            .unwrap()
    };
    assert!(property(11).fields()["independent_variables"]
        .as_array()
        .unwrap()
        .is_empty());
    assert!(property(11).fields()["dependent_values"]
        .as_array()
        .unwrap()
        .is_empty());
    assert!(property(24).fields()["definitions"]
        .as_array()
        .unwrap()
        .is_empty());
}

#[test]
fn decode_type_406_form_11_does_not_expose_a_partial_independent_prefix() {
    let bytes = owned_test_file(&[OwnedTestEntity {
        entity_type: 406,
        form: 11,
        label: "TABLATE".into(),
        status: "00000200",
        parameters: "406,8,5,1,2,1,2,1,99,10;".into(),
    }]);
    let result = IgesCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    let native = result.ir().native.namespace("iges").unwrap();
    let entity = &native.arenas["entities"][0];
    let property = &native.arenas["properties"][0];

    assert_eq!(entity.fields()["parameters"].as_array().unwrap().len(), 10);
    assert_eq!(property.fields()["declared_dependent_count"], 1);
    assert!(property.fields()["independent_variables"]
        .as_array()
        .unwrap()
        .is_empty());
    assert!(property.fields()["dependent_values"]
        .as_array()
        .unwrap()
        .is_empty());
}

#[test]
fn decode_type_406_form_11_does_not_expose_zero_count_independent_values() {
    let bytes = owned_test_file(&[OwnedTestEntity {
        entity_type: 406,
        form: 11,
        label: "ZCOUNT".into(),
        status: "00000200",
        parameters: "406,5,5,1,1,1,0,1,1,1,3;".into(),
    }]);
    let result = IgesCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    let native = result.ir().native.namespace("iges").unwrap();
    let property = &native.arenas["properties"][0];

    assert_eq!(property.fields()["declared_dependent_count"], 1);
    assert!(property.fields()["independent_variables"]
        .as_array()
        .unwrap()
        .is_empty());
    assert!(property.fields()["dependent_values"]
        .as_array()
        .unwrap()
        .is_empty());
}

#[test]
fn decode_definition_levels_stop_before_trailing_property_group() {
    let bytes = owned_test_file(&[
        OwnedTestEntity {
            entity_type: 406,
            form: 1,
            label: "LEVELS".into(),
            status: "00000200",
            parameters: "406,3,7,0,1,0,1,3;".into(),
        },
        OwnedTestEntity {
            entity_type: 406,
            form: 7,
            label: "PROP".into(),
            status: "00000200",
            parameters: "406,1,1HX;".into(),
        },
    ]);
    let result = IgesCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    let native = result.ir().native.namespace("iges").unwrap();
    let entity = &native.arenas["entities"][0];
    let levels = &native.arenas["definition_levels"][0];

    assert_eq!(
        entity.fields()["property_links"][0],
        "iges:entity:directory#3"
    );
    assert_eq!(levels.fields()["declared_count"], 3);
    assert_eq!(levels.fields()["levels"], serde_json::json!([7, 0, 1]));
}

#[test]
fn decode_label_display_defaulted_final_placement_rejects_trailing_property_group() {
    let bytes = owned_test_file(&[
        OwnedTestEntity {
            entity_type: 402,
            form: 5,
            label: "LABELS".into(),
            status: "00000200",
            parameters: "402,2,1,0,0,0,0,0,1,0,1,3;".into(),
        },
        OwnedTestEntity {
            entity_type: 406,
            form: 7,
            label: "PROP".into(),
            status: "00000200",
            parameters: "406,1,1HX;".into(),
        },
    ]);
    let result = IgesCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    let native = result.ir().native.namespace("iges").unwrap();
    let entity = &native.arenas["entities"][0];
    let associativity = &native.arenas["associativities"][0];

    assert!(entity.fields()["property_links"][0].is_null());
    assert_eq!(associativity.fields()["declared_count"], 2);
    let fields = associativity.fields();
    let placements = fields["placements"].as_array().unwrap();
    assert_eq!(placements.len(), 2);
    assert!(placements[1]["label_level"].is_null());
    assert_eq!(
        code_count(result.report(), IgesLossCode::ParameterCountOverdeclared),
        0
    );
}

#[test]
fn decode_view_list_uses_form6_class_entry_and_visible_count() {
    let bytes = owned_test_file(&[
        OwnedTestEntity {
            entity_type: 410,
            form: 0,
            label: "VIEW".into(),
            status: "00000200",
            parameters: "410,1,1,0,0,0,0,0,0,1,3,0;".into(),
        },
        OwnedTestEntity {
            entity_type: 402,
            form: 6,
            label: "VIEWLST".into(),
            status: "00000200",
            parameters: "402,1,1,1,5,1,7,0;".into(),
        },
        OwnedTestEntity {
            entity_type: 116,
            form: 0,
            label: "VISIBLE".into(),
            status: "00010100",
            parameters: "116,1,2,3,0,1,3,0;".into(),
        },
        OwnedTestEntity {
            entity_type: 212,
            form: 0,
            label: "ASSOC".into(),
            status: "00010100",
            parameters: "212,1,1,1,1,1,1.5707963267948966,0,0,0,0,0,0,1HA;".into(),
        },
    ]);
    let result = IgesCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    let native = result.ir().native.namespace("iges").unwrap();
    let associativity = native.arenas["associativities"]
        .iter()
        .find(|record| record.id() == "iges:structure:associativity#D3")
        .unwrap();
    let fields = associativity.fields();
    assert_eq!(fields["declared_visible_count"], 1);
    assert_eq!(fields["view"], "iges:entity:directory#1");
    assert_eq!(fields["visible_entities"][0], "iges:entity:directory#5");

    let source = native.arenas["entities"]
        .iter()
        .find(|record| record.id() == "iges:entity:directory#3")
        .unwrap();
    assert_eq!(
        source.fields()["association_links"][0],
        "iges:entity:directory#7"
    );
}

#[test]
fn decode_external_reference_index_uses_counted_name_pointer_pairs() {
    let bytes = owned_test_file(&[
        OwnedTestEntity {
            entity_type: 116,
            form: 0,
            label: "TARGET".into(),
            status: "00010100",
            parameters: "116,1,2,3,0;".into(),
        },
        OwnedTestEntity {
            entity_type: 402,
            form: 12,
            label: "XREF".into(),
            status: "00000200",
            parameters: "402,1,6HREF001,1,1,5,0;".into(),
        },
        OwnedTestEntity {
            entity_type: 212,
            form: 0,
            label: "ASSOC".into(),
            status: "00010100",
            parameters: "212,1,1,1,1,1,1.5707963267948966,0,0,0,0,0,0,1HA;".into(),
        },
    ]);
    let result = IgesCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    let native = result.ir().native.namespace("iges").unwrap();
    let index = native.arenas["associativities"]
        .iter()
        .find(|record| record.id() == "iges:structure:associativity#D3")
        .unwrap();
    let fields = index.fields();
    assert_eq!(fields["declared_count"], 1);
    assert_eq!(
        fields["entries"][0]["symbolic_name"],
        serde_json::json!([82, 69, 70, 48, 48, 49])
    );
    assert_eq!(fields["entries"][0]["entity"], "iges:entity:directory#1");
    assert!(
        result.report().losses.is_empty(),
        "{:#?}",
        result.report().losses
    );
}

#[test]
fn decode_dimensioned_geometry_uses_counted_geometry_pointers() {
    let bytes = owned_test_file(&[
        OwnedTestEntity {
            entity_type: 212,
            form: 0,
            label: "NOTE".into(),
            status: "00010100",
            parameters: "212,1,1,1,1,1,1.5707963267948966,0,0,0,0,0,0,1HA;".into(),
        },
        OwnedTestEntity {
            entity_type: 214,
            form: 1,
            label: "LEAD1".into(),
            status: "00010100",
            parameters: "214,1,2,1,0,0,0,2,0;".into(),
        },
        OwnedTestEntity {
            entity_type: 214,
            form: 1,
            label: "LEAD2".into(),
            status: "00010100",
            parameters: "214,1,2,1,0,0,0,2,0;".into(),
        },
        OwnedTestEntity {
            entity_type: 216,
            form: 0,
            label: "DIM".into(),
            status: "00000100",
            parameters: "216,1,3,5,0,0,1,11,0;".into(),
        },
        OwnedTestEntity {
            entity_type: 116,
            form: 0,
            label: "GEOM".into(),
            status: "00010100",
            parameters: "116,4,5,6,0;".into(),
        },
        OwnedTestEntity {
            entity_type: 402,
            form: 13,
            label: "DIMGEOM".into(),
            status: "00000200",
            parameters: "402,1,1,7,9,1,13,0;".into(),
        },
        OwnedTestEntity {
            entity_type: 212,
            form: 0,
            label: "ASSOC".into(),
            status: "00010100",
            parameters: "212,1,1,1,1,1,1.5707963267948966,0,0,0,0,0,0,1HA;".into(),
        },
    ]);
    let result = IgesCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    let native = result.ir().native.namespace("iges").unwrap();
    let dimensioned = native.arenas["associativities"]
        .iter()
        .find(|record| record.id() == "iges:structure:associativity#D11")
        .unwrap();
    let fields = dimensioned.fields();
    assert_eq!(fields["declared_geometry_count"], 1);
    assert_eq!(fields["dimension"], "iges:entity:directory#7");
    assert_eq!(fields["geometry"][0], "iges:entity:directory#9");
    assert!(
        result.report().losses.is_empty(),
        "{:#?}",
        result.report().losses
    );
}

#[test]
fn decode_view_visibility_counts_stop_at_the_next_list_boundary() {
    let bytes = owned_test_file(&[
        OwnedTestEntity {
            entity_type: 402,
            form: 3,
            label: "VIEWS".into(),
            status: "00000200",
            parameters: "402,2,3,1,5,0,1,7;".into(),
        },
        OwnedTestEntity {
            entity_type: 410,
            form: 0,
            label: "VIEW".into(),
            status: "00000200",
            parameters: "410,1;".into(),
        },
        OwnedTestEntity {
            entity_type: 116,
            form: 0,
            label: "POINT".into(),
            status: "00000200",
            parameters: "116,1,2,3;".into(),
        },
        OwnedTestEntity {
            entity_type: 406,
            form: 7,
            label: "PROP".into(),
            status: "00000200",
            parameters: "406,1,1HX;".into(),
        },
    ]);
    let result = IgesCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    let native = result.ir().native.namespace("iges").unwrap();
    let entity = &native.arenas["entities"][0];
    let visibility = &native.arenas["view_visibility"][0];

    assert_eq!(
        entity.fields()["property_links"][0],
        "iges:entity:directory#7"
    );
    assert!(visibility.fields()["displays"]
        .as_array()
        .unwrap()
        .is_empty());
    assert!(visibility.fields()["entities"]
        .as_array()
        .unwrap()
        .is_empty());
}

#[test]
fn decode_flow_counts_do_not_recover_incomplete_class_lists() {
    let bytes = owned_test_file(&[
        OwnedTestEntity {
            entity_type: 402,
            form: 18,
            label: "FLOW".into(),
            status: "00000200",
            parameters: "402,2,2,2,2,2,2,2,0,0,0,0,0,0,0,1,3;".into(),
        },
        OwnedTestEntity {
            entity_type: 406,
            form: 7,
            label: "PROP".into(),
            status: "00000200",
            parameters: "406,1,1HX;".into(),
        },
    ]);
    let result = IgesCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    let native = result.ir().native.namespace("iges").unwrap();
    let entity = &native.arenas["entities"][0];
    let flow = &native.arenas["associativities"][0];

    assert!(entity.fields()["property_links"]
        .as_array()
        .unwrap()
        .is_empty());
    assert_eq!(flow.fields()["declared_associated_flow_count"], 2);
    assert!(flow.fields()["associated_flows"]
        .as_array()
        .unwrap()
        .is_empty());
    assert!(flow.fields()["connections"].as_array().unwrap().is_empty());
    assert!(flow.fields()["joins"].as_array().unwrap().is_empty());
    assert!(flow.fields()["names"].as_array().unwrap().is_empty());
    assert!(flow.fields()["name_displays"]
        .as_array()
        .unwrap()
        .is_empty());
    assert!(flow.fields()["continuations"]
        .as_array()
        .unwrap()
        .is_empty());
}

#[test]
fn decode_flow_form18_keeps_zero_class_lists_before_trailing_groups() {
    let bytes = owned_test_file(&[
        OwnedTestEntity {
            entity_type: 212,
            form: 0,
            label: "NOTE".into(),
            status: "00010100",
            parameters: "212,1,1,1,1,1,1.5707963267948966,0,0,0,0,0,0,1HA;".into(),
        },
        OwnedTestEntity {
            entity_type: 402,
            form: 18,
            label: "FLOW".into(),
            status: "00000200",
            parameters: "402,2,0,0,0,0,0,0,1,2,1,1,0;".into(),
        },
    ]);
    let result = IgesCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    let native = result.ir().native.namespace("iges").unwrap();
    let source = native.arenas["entities"]
        .iter()
        .find(|record| record.id() == "iges:entity:directory#3")
        .unwrap();
    assert_eq!(
        source.fields()["association_links"].as_array().unwrap(),
        &[serde_json::json!("iges:entity:directory#1")]
    );
    let flow = native.arenas["associativities"]
        .iter()
        .find(|record| record.id() == "iges:structure:associativity#D3")
        .unwrap();
    assert_eq!(flow.fields()["declared_associated_flow_count"], 0);
    assert_eq!(flow.fields()["declared_connection_count"], 0);
    assert_eq!(flow.fields()["declared_join_count"], 0);
    assert_eq!(flow.fields()["declared_name_count"], 0);
    assert_eq!(flow.fields()["declared_name_display_count"], 0);
    assert_eq!(flow.fields()["declared_continuation_count"], 0);
}

#[test]
fn decode_native_type_106_does_not_invent_tuples_for_invalid_interpretation() {
    let bytes = owned_test_file(&[
        OwnedTestEntity {
            entity_type: 106,
            form: 11,
            label: "VALID".into(),
            status: "00000000",
            parameters: "106,1,2,0,0,0,1,1,0;".into(),
        },
        OwnedTestEntity {
            entity_type: 106,
            form: 11,
            label: "ABSENT".into(),
            status: "00000000",
            parameters: "106,,2,0,0,1,0,0;".into(),
        },
        OwnedTestEntity {
            entity_type: 106,
            form: 11,
            label: "OUTRANGE".into(),
            status: "00000000",
            parameters: "106,4,2,0,0,1,0,0;".into(),
        },
        OwnedTestEntity {
            entity_type: 106,
            form: 11,
            label: "MISMATCH".into(),
            status: "00000000",
            parameters: "106,2,2,0,0,0,1,0,0;".into(),
        },
    ]);
    let result = IgesCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    let native = result.ir().native.namespace("iges").unwrap();
    let record = |sequence| {
        native.arenas["copious_data"]
            .iter()
            .find(|record| record.id() == format!("iges:native:copious-data#D{sequence}"))
            .unwrap()
    };

    let valid = record(1).fields();
    assert_eq!(valid["tuples"].as_array().unwrap().len(), 2);
    assert_eq!(valid["tuples"][0].as_array().unwrap().len(), 2);
    for (sequence, interpretation) in [(3, None), (5, Some(4)), (7, Some(2))] {
        let fields = record(sequence).fields();
        assert_eq!(fields["declared_tuple_count"], 2);
        assert!(fields["common_z"].is_null());
        assert!(fields["tuples"].as_array().unwrap().is_empty());
        match interpretation {
            Some(interpretation) => assert_eq!(fields["interpretation"], interpretation),
            None => assert!(fields["interpretation"].is_null()),
        }
    }
}

#[test]
fn decode_bounds_declared_brep_counts_by_record_tokens() {
    let bytes = owned_test_file(&[OwnedTestEntity {
        entity_type: 502,
        form: 1,
        label: "BADCOUNT".into(),
        status: "00010000",
        parameters: "502,9223372036854775807;".into(),
    }]);
    let result = IgesCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();

    assert!(result
        .report()
        .losses
        .iter()
        .any(|loss| loss.message.contains("vertex-list count")));
    assert!(result.ir().model.vertices.is_empty());
}

#[test]
fn decode_bounds_declared_trimming_counts_by_record_tokens() {
    let bytes = owned_test_file(&[
        OwnedTestEntity {
            entity_type: 108,
            form: 0,
            label: "PLANE".into(),
            status: "00010000",
            parameters: "108,0,0,1,0,0,0,0,0,0;".into(),
        },
        OwnedTestEntity {
            entity_type: 141,
            form: 0,
            label: "BADCOUNT".into(),
            status: "00010000",
            parameters: "141,0,1,1,9223372036854775807;".into(),
        },
    ]);
    let result = IgesCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();

    assert!(result
        .report()
        .losses
        .iter()
        .any(|loss| loss.message.contains("boundary segment count")));
}

#[test]
fn decode_bounds_declared_presentation_counts_by_record_tokens() {
    let bytes = owned_test_file(&[OwnedTestEntity {
        entity_type: 310,
        form: 0,
        label: "BADCOUNT".into(),
        status: "00000200",
        parameters: "310,1,1HA,,1,9223372036854775807;".into(),
    }]);
    let result = IgesCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();

    assert!(result
        .report()
        .losses
        .iter()
        .any(|loss| loss.message.contains("font header")));
    let fonts = &result.ir().native.namespace("iges").unwrap().arenas["text_fonts"];
    assert!(fonts[0].fields()["characters"]
        .as_array()
        .unwrap()
        .is_empty());
}

#[test]
fn decode_bounds_declared_annotation_counts_by_record_tokens() {
    let bytes = owned_test_file(&[OwnedTestEntity {
        entity_type: 212,
        form: 0,
        label: "BADCOUNT".into(),
        status: "00010100",
        parameters: "212,9223372036854775807;".into(),
    }]);
    let result = IgesCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();

    assert!(result
        .report()
        .losses
        .iter()
        .any(|loss| loss.message.contains("text count")));
    let annotations = &result.ir().native.namespace("iges").unwrap().arenas["annotations"];
    assert!(annotations[0].fields()["strings"]
        .as_array()
        .unwrap()
        .is_empty());
}

#[test]
fn decode_bounds_declared_drawing_counts_by_record_tokens() {
    let bytes = owned_test_file(&[OwnedTestEntity {
        entity_type: 404,
        form: 0,
        label: "BADCOUNT".into(),
        status: "00000000",
        parameters: "404,9223372036854775807;".into(),
    }]);
    let result = IgesCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();

    assert!(result
        .report()
        .losses
        .iter()
        .any(|loss| loss.message.contains("drawing view placements")));
    let drawings = &result.ir().native.namespace("iges").unwrap().arenas["drawings"];
    assert!(drawings[0].fields()["views"].as_array().unwrap().is_empty());
}

#[test]
fn decode_bounds_declared_solid_counts_by_record_tokens() {
    let bytes = owned_test_file(&[OwnedTestEntity {
        entity_type: 180,
        form: 0,
        label: "BADCOUNT".into(),
        status: "00000000",
        parameters: "180,9223372036854775807;".into(),
    }]);
    let result = IgesCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();

    assert!(result
        .report()
        .losses
        .iter()
        .any(|loss| loss.message.contains("Boolean postfix length")));
    let trees = &result.ir().native.namespace("iges").unwrap().arenas["boolean_trees"];
    assert!(trees[0].fields()["terms"].as_array().unwrap().is_empty());
}

#[test]
fn decode_text_score_forms_uses_counted_ranges_before_trailing_associations() {
    let bytes = owned_test_file(&[
        OwnedTestEntity {
            entity_type: 212,
            form: 0,
            label: "NOTE".into(),
            status: "00010100",
            parameters: "212,1,1,1,1,1,1.5707963267948966,0,0,0,0,0,0,1HA,0,2,3,5;".into(),
        },
        OwnedTestEntity {
            entity_type: 406,
            form: 34,
            label: "UNDER".into(),
            status: "00010000",
            parameters: "406,4,1,1,2,4,1,1,0;".into(),
        },
        OwnedTestEntity {
            entity_type: 406,
            form: 35,
            label: "OVER".into(),
            status: "00010000",
            parameters: "406,7,2,1,2,4,2,1,3,1,1,0;".into(),
        },
    ]);
    let result = IgesCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    let native = result.ir().native.namespace("iges").unwrap();
    let properties = &native.arenas["properties"];
    for (form, kind, ranges) in [(34, "underscore", 1), (35, "overscore", 2)] {
        let property = properties
            .iter()
            .find(|property| property.fields()["form"] == form)
            .expect("text-score property")
            .fields();
        assert_eq!(property["property_kind"], kind);
        assert_eq!(property["ranges"].as_array().unwrap().len(), ranges);
    }
    for sequence in [3, 5] {
        let entity = native.arenas["entities"]
            .iter()
            .find(|entity| entity.id() == format!("iges:entity:directory#{sequence}"))
            .unwrap();
        assert_eq!(
            entity.fields()["association_links"],
            serde_json::json!(["iges:entity:directory#1"])
        );
    }
    assert!(
        result.report().losses.is_empty(),
        "{:#?}",
        result.report().losses
    );
}

#[test]
fn decode_leader_segment_count_does_not_invent_tails_past_the_arrowhead_block() {
    let bytes = owned_test_file(&[OwnedTestEntity {
        entity_type: 214,
        form: 1,
        label: "LEADER".into(),
        status: "00000200",
        parameters: "214,3,1.0,1.0,0.0,0.0,0.0,5.0,5.0;".into(),
    }]);
    let result = IgesCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    let native = result.ir().native.namespace("iges").unwrap();
    let leader = &native.arenas["annotations"][0];

    assert_eq!(leader.fields()["declared_segment_count"], 3);
    assert!(leader.fields()["segment_tails"]
        .as_array()
        .unwrap()
        .is_empty());
}

#[test]
fn decode_view_list_visible_entities_stop_at_the_view_pointer() {
    let bytes = owned_test_file(&[
        OwnedTestEntity {
            entity_type: 410,
            form: 0,
            label: "VIEW".into(),
            status: "00000200",
            parameters: "410,1,1,0,0,0,0,0,0,1,3,0;".into(),
        },
        OwnedTestEntity {
            entity_type: 402,
            form: 6,
            label: "VIEWLST".into(),
            status: "00000200",
            parameters: "402,1,2,1,5;".into(),
        },
        OwnedTestEntity {
            entity_type: 116,
            form: 0,
            label: "VISIBLE".into(),
            status: "00010100",
            parameters: "116,1,2,3;".into(),
        },
    ]);
    let result = IgesCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    let native = result.ir().native.namespace("iges").unwrap();
    let associativity = &native.arenas["associativities"][0];

    assert_eq!(associativity.fields()["declared_visible_count"], 2);
    assert!(associativity.fields()["visible_entities"]
        .as_array()
        .unwrap()
        .is_empty());
}

#[test]
fn decode_single_parent_children_stop_at_the_parent_pointer() {
    let bytes = owned_test_file(&[
        OwnedTestEntity {
            entity_type: 116,
            form: 0,
            label: "CHILD".into(),
            status: "00010100",
            parameters: "116,1,2,3;".into(),
        },
        OwnedTestEntity {
            entity_type: 402,
            form: 9,
            label: "PARENT".into(),
            status: "00000200",
            parameters: "402,1,2,1,1;".into(),
        },
    ]);
    let result = IgesCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    let native = result.ir().native.namespace("iges").unwrap();
    let associativity = &native.arenas["associativities"][0];

    assert_eq!(associativity.fields()["declared_child_count"], 2);
    assert!(associativity.fields()["children"]
        .as_array()
        .unwrap()
        .is_empty());
}

#[test]
fn decode_dimensioned_geometry_stops_at_the_dimension_pointer() {
    let bytes = owned_test_file(&[
        OwnedTestEntity {
            entity_type: 116,
            form: 0,
            label: "GEOM".into(),
            status: "00010100",
            parameters: "116,1,2,3;".into(),
        },
        OwnedTestEntity {
            entity_type: 202,
            form: 0,
            label: "DIM".into(),
            status: "00000200",
            parameters: "202,0,0,0,0,0,0,0,0,0;".into(),
        },
        OwnedTestEntity {
            entity_type: 402,
            form: 13,
            label: "DIMGEOM".into(),
            status: "00000200",
            parameters: "402,1,2,3,1;".into(),
        },
    ]);
    let result = IgesCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    let native = result.ir().native.namespace("iges").unwrap();
    let associativity = &native.arenas["associativities"][0];

    assert_eq!(associativity.fields()["declared_geometry_count"], 2);
    assert!(associativity.fields()["geometry"]
        .as_array()
        .unwrap()
        .is_empty());
}

#[test]
fn decode_planar_entities_stop_at_the_transform_pointer() {
    let bytes = owned_test_file(&[
        OwnedTestEntity {
            entity_type: 116,
            form: 0,
            label: "MEMBER".into(),
            status: "00010100",
            parameters: "116,1,2,3;".into(),
        },
        OwnedTestEntity {
            entity_type: 402,
            form: 16,
            label: "PLANAR".into(),
            status: "00000200",
            parameters: "402,1,2,0,1;".into(),
        },
    ]);
    let result = IgesCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    let native = result.ir().native.namespace("iges").unwrap();
    let associativity = &native.arenas["associativities"][0];

    assert_eq!(associativity.fields()["declared_entity_count"], 2);
    assert!(associativity.fields()["entities"]
        .as_array()
        .unwrap()
        .is_empty());
}

#[test]
fn decode_recalculable_dimension_reads_a_defaulted_final_tuple() {
    let bytes = owned_test_file(&[
        OwnedTestEntity {
            entity_type: 202,
            form: 0,
            label: "DIM".into(),
            status: "00000200",
            parameters: "202,0,0,0,0,0,0,0,0,0;".into(),
        },
        OwnedTestEntity {
            entity_type: 402,
            form: 21,
            label: "RECALC".into(),
            status: "00000200",
            parameters: "402,1,1,1,0,0.0,1,0,0.0;".into(),
        },
    ]);
    let result = IgesCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    let native = result.ir().native.namespace("iges").unwrap();
    let associativity = &native.arenas["associativities"][0];

    assert_eq!(associativity.fields()["declared_geometry_count"], 1);
    let fields = associativity.fields();
    let geometry = fields["geometry"].as_array().unwrap();
    assert_eq!(geometry.len(), 1);
    assert_eq!(geometry[0]["point"][0], 0.0);
    assert!(geometry[0]["point"][1].is_null());
    assert_eq!(
        code_count(result.report(), IgesLossCode::ParameterCountOverdeclared),
        0
    );
}

#[test]
fn decode_array_positions_stop_at_the_do_dont_flag() {
    let bytes = owned_test_file(&[
        OwnedTestEntity {
            entity_type: 116,
            form: 0,
            label: "BASE".into(),
            status: "00000200",
            parameters: "116,0,0,0;".into(),
        },
        OwnedTestEntity {
            entity_type: 412,
            form: 0,
            label: "RECT".into(),
            status: "00000200",
            parameters: "412,1,1.0,0,0,0,2,2,1.0,1.0,0.0,2,0,1;".into(),
        },
        OwnedTestEntity {
            entity_type: 414,
            form: 0,
            label: "CIRC".into(),
            status: "00000200",
            parameters: "414,1,4,0,0,0,1.0,0.0,90.0,2,0,1;".into(),
        },
    ]);
    let result = IgesCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();

    let native = result.ir().native.namespace("iges").unwrap();
    let rectangular = &native.arenas["rectangular_arrays"][0];
    assert!(rectangular.fields()["positions"]
        .as_array()
        .unwrap()
        .is_empty());

    let circular = &native.arenas["circular_arrays"][0];
    assert!(circular.fields()["positions"]
        .as_array()
        .unwrap()
        .is_empty());
}

#[test]
fn decode_complete_leader_and_island_lists_keep_every_declared_item() {
    let bytes = owned_test_file(&[
        OwnedTestEntity {
            entity_type: 100,
            form: 0,
            label: "BOUND".into(),
            status: "00010100",
            parameters: "100,0.0,0.0,0.0,1.0,0.0,1.0,0.0;".into(),
        },
        OwnedTestEntity {
            entity_type: 100,
            form: 0,
            label: "ISLAND".into(),
            status: "00010100",
            parameters: "100,0.0,0.0,0.0,0.5,0.0,0.5,0.0;".into(),
        },
        OwnedTestEntity {
            entity_type: 230,
            form: 0,
            label: "SECTION".into(),
            status: "00000200",
            parameters: "230,1,0,0.0,0.0,0.0,0.0,0.0,1,3;".into(),
        },
        OwnedTestEntity {
            entity_type: 212,
            form: 0,
            label: "NOTE".into(),
            status: "00010100",
            parameters: "212,1,1,1,1,1,1.5707963267948966,0,0,0,0,0,0,1HA;".into(),
        },
        OwnedTestEntity {
            entity_type: 214,
            form: 1,
            label: "LEADER".into(),
            status: "00010100",
            parameters: "214,1,1.0,1.0,0.0,0.0,0.0,5.0,5.0;".into(),
        },
        OwnedTestEntity {
            entity_type: 210,
            form: 0,
            label: "LABEL".into(),
            status: "00000200",
            parameters: "210,7,1,9;".into(),
        },
    ]);
    let result = IgesCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    let native = result.ir().native.namespace("iges").unwrap();
    let annotations = &native.arenas["annotations"];
    let annotation = |kind: &str| {
        annotations
            .iter()
            .find(|record| record.fields()["kind"] == kind)
            .expect("annotation")
            .fields()
    };

    assert_eq!(annotation("sectioned_area")["declared_island_count"], 1);
    assert_eq!(
        annotation("sectioned_area")["islands"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        annotation("general_label")["leaders"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        annotation("leader")["segment_tails"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
}

fn salvage(bytes: &[u8]) -> cadmpeg_ir::codec::DecodeResult {
    IgesCodec
        .decode(&mut Cursor::new(bytes.to_vec()), &DecodeOptions::default())
        .unwrap()
}

fn overdeclared_site(
    entities: &[OwnedTestEntity],
    sequence: u32,
    arena: &str,
    declared_field: &str,
    declared: i64,
    list_field: &str,
) {
    let bytes = owned_test_file(entities);
    assert_overdeclared_contract(&bytes, sequence);

    let result = salvage(&bytes);
    let native = result.ir().native.namespace("iges").unwrap();
    let record = native.arenas[arena]
        .iter()
        .find(|record| {
            record.fields()["source_entity"] == format!("iges:entity:directory#{sequence}")
        })
        .expect("native record");
    let fields = record.fields();
    assert_eq!(fields[declared_field], declared, "{arena} declared count");
    assert!(
        fields[list_field].as_array().unwrap().is_empty(),
        "{arena} {list_field} must not be read"
    );
}

fn only(entity_type: i64, form: i64, label: &str, parameters: &str) -> Vec<OwnedTestEntity> {
    vec![OwnedTestEntity {
        entity_type,
        form,
        label: label.into(),
        status: "00000200",
        parameters: parameters.into(),
    }]
}

fn point(label: &str) -> OwnedTestEntity {
    OwnedTestEntity {
        entity_type: 116,
        form: 0,
        label: label.into(),
        status: "00010100",
        parameters: "116,1,2,3;".into(),
    }
}

fn entity(entity_type: i64, form: i64, label: &str, parameters: &str) -> OwnedTestEntity {
    OwnedTestEntity {
        entity_type,
        form,
        label: label.into(),
        status: "00000200",
        parameters: parameters.into(),
    }
}

#[test]
fn decode_overdeclared_definition_levels_charge_the_loss_and_read_no_level() {
    overdeclared_site(
        &only(406, 1, "LEVELS", "406,3,10,20;"),
        1,
        "definition_levels",
        "declared_count",
        3,
        "levels",
    );
}

#[test]
fn decode_overdeclared_boolean_tree_charges_the_loss_and_reads_no_term() {
    overdeclared_site(
        &only(180, 0, "TREE", "180,5,-1,-3,1;"),
        1,
        "boolean_trees",
        "declared_length",
        5,
        "terms",
    );
}

#[test]
fn decode_overdeclared_subfigure_definition_charges_the_loss_and_reads_no_member() {
    overdeclared_site(
        &[point("MEMBER"), entity(308, 0, "SUB", "308,0,3HSUB,3,1;")],
        3,
        "subfigure_definitions",
        "declared_member_count",
        3,
        "members",
    );
}

#[test]
fn decode_overdeclared_group_charges_the_loss_and_reads_no_member() {
    overdeclared_site(
        &[point("MEMBER"), entity(402, 1, "GROUP", "402,3,1;")],
        3,
        "groups",
        "declared_member_count",
        3,
        "members",
    );
}

#[test]
fn decode_overdeclared_network_instance_charges_the_loss_and_reads_no_connect_point() {
    overdeclared_site(
        &only(420, 0, "NETINST", "420,0,0,0,0,1,1,1,0,2HR1,0,3,0;"),
        1,
        "network_instances",
        "declared_connect_point_count",
        3,
        "connect_points",
    );
}

#[test]
fn decode_overdeclared_solid_assembly_charges_the_loss_and_reads_no_item() {
    overdeclared_site(
        &only(184, 0, "ASSEMBLY", "184,2,1,3;"),
        1,
        "solid_assemblies",
        "declared_count",
        2,
        "items",
    );
}

fn manifold_solid_entities(
    parameters: impl Fn(u32, u32) -> String,
) -> (Vec<OwnedTestEntity>, u32, u32, u32) {
    let mut entities = Vec::new();
    let outer = append_tetrahedral_shell(&mut entities, "OUT", [0.0, 0.0, 0.0], 4.0);
    let void = append_tetrahedral_shell(&mut entities, "VOID", [0.5, 0.5, 0.5], 0.5);
    let solid = u32::try_from(entities.len() * 2 + 1).unwrap();
    let parameters = parameters(outer, void);
    entities.push(OwnedTestEntity {
        entity_type: 186,
        form: 0,
        label: "MSBO".into(),
        status: "00000000",
        parameters,
    });
    (entities, solid, outer, void)
}

#[test]
fn decode_overdeclared_manifold_solid_charges_the_loss_and_reads_no_void_shell() {
    let (entities, solid, _, _) =
        manifold_solid_entities(|outer, void| format!("186,{outer},1,2,{void},0;"));
    overdeclared_site(
        &entities,
        solid,
        "manifold_solids",
        "declared_void_count",
        2,
        "voids",
    );
}

#[test]
fn decode_overdeclared_units_data_charges_the_loss_and_reads_no_unit() {
    overdeclared_site(
        &only(316, 0, "UNITS", "316,3,2HIN,4HINCH,25.4,2HFT;"),
        1,
        "units_data",
        "declared_count",
        3,
        "units",
    );
}

#[test]
fn decode_overdeclared_leader_charges_the_loss_and_reads_no_segment() {
    overdeclared_site(
        &only(214, 1, "LEADER", "214,3,1.0,1.0,0.0,0.0,0.0,5.0,5.0;"),
        1,
        "annotations",
        "declared_segment_count",
        3,
        "segment_tails",
    );
}

#[test]
fn decode_overdeclared_line_font_pattern_reserves_the_hexadecimal_suffix() {
    let bytes = owned_test_file(&only(304, 2, "PATTERN", "304,3,1.0,2.0,2H0F;"));
    assert_overdeclared_contract(&bytes, 1);

    let result = salvage(&bytes);
    let native = result.ir().native.namespace("iges").unwrap();
    let font = &native.arenas["line_fonts"][0];

    assert_eq!(font.fields()["segment_count"], 3);
    assert!(font.fields()["lengths"].as_array().unwrap().is_empty());
    assert!(font.fields()["hexadecimal_pattern"].is_null());
}

#[test]
fn decode_overdeclared_line_font_pattern_claims_no_length_as_its_suffix() {
    let bytes = owned_test_file(&only(304, 2, "PATTERN", "304,5,2HAB;"));
    assert_overdeclared_contract(&bytes, 1);

    let result = salvage(&bytes);
    let native = result.ir().native.namespace("iges").unwrap();
    let font = &native.arenas["line_fonts"][0];

    assert_eq!(font.fields()["segment_count"], 5);
    assert!(font.fields()["lengths"].as_array().unwrap().is_empty());
    assert!(font.fields()["hexadecimal_pattern"].is_null());
}

#[test]
fn decode_overdeclared_copious_data_charges_the_loss_and_reads_no_tuple() {
    overdeclared_site(
        &[OwnedTestEntity {
            entity_type: 106,
            form: 1,
            label: "COPIOUS".into(),
            status: "00000000",
            parameters: "106,1,3,0.5,1,2,3;".into(),
        }],
        1,
        "copious_data",
        "declared_tuple_count",
        3,
        "tuples",
    );
}

#[test]
fn decode_overdeclared_external_reference_index_charges_the_loss_and_reads_no_entry() {
    for form in [2, 12] {
        overdeclared_site(
            &[
                point("TARGET"),
                entity(402, form, "INDEX", "402,3,1HA,1,1HB,1;"),
            ],
            3,
            "associativities",
            "declared_count",
            3,
            "entries",
        );
    }
}

#[test]
fn decode_overdeclared_segmented_visibility_charges_the_loss_and_reads_no_block() {
    overdeclared_site(
        &[
            entity(410, 0, "VIEW", "410,1;"),
            entity(402, 19, "SEGVIS", "402,2,1,0.5,1,0,0,0;"),
        ],
        3,
        "segmented_visibility",
        "declared_block_count",
        2,
        "blocks",
    );
}

#[test]
fn decode_overdeclared_view_list_charges_the_loss_and_reads_no_entity() {
    overdeclared_site(
        &[
            entity(410, 0, "VIEW", "410,1,1,0,0,0,0,0,0,1,3,0;"),
            entity(402, 6, "VIEWLST", "402,1,2,1,5;"),
            point("VISIBLE"),
        ],
        3,
        "associativities",
        "declared_visible_count",
        2,
        "visible_entities",
    );
}

#[test]
fn decode_overdeclared_single_parent_charges_the_loss_and_reads_no_child() {
    overdeclared_site(
        &[point("CHILD"), entity(402, 9, "PARENT", "402,1,2,1,1;")],
        3,
        "associativities",
        "declared_child_count",
        2,
        "children",
    );
}

#[test]
fn decode_overdeclared_dimensioned_geometry_charges_the_loss_and_reads_no_geometry() {
    overdeclared_site(
        &[
            point("GEOM"),
            entity(202, 0, "DIM", "202,0,0,0,0,0,0,0,0,0;"),
            entity(402, 13, "DIMGEOM", "402,1,2,3,1;"),
        ],
        5,
        "associativities",
        "declared_geometry_count",
        2,
        "geometry",
    );
}

#[test]
fn decode_overdeclared_planar_charges_the_loss_and_reads_no_entity() {
    overdeclared_site(
        &[point("MEMBER"), entity(402, 16, "PLANAR", "402,1,2,0,1;")],
        3,
        "associativities",
        "declared_entity_count",
        2,
        "entities",
    );
}

#[test]
fn decode_overdeclared_recalculable_dimension_charges_the_loss_and_reads_no_tuple() {
    overdeclared_site(
        &[
            entity(202, 0, "DIM", "202,0,0,0,0,0,0,0,0,0;"),
            entity(402, 21, "RECALC", "402,1,2,1,0,0.0,1,0,0.0,0.0,0.0;"),
        ],
        3,
        "associativities",
        "declared_geometry_count",
        2,
        "geometry",
    );
}

#[test]
fn decode_overdeclared_attribute_definition_charges_the_loss_and_reads_no_attribute() {
    overdeclared_site(
        &only(322, 0, "ATTRDEF", "322,4HATTR,0,3,1,1,0,2,1,0;"),
        1,
        "attribute_table_definitions",
        "declared_attribute_count",
        3,
        "attributes",
    );
}

#[test]
fn decode_overdeclared_property_lists_charge_the_loss_and_read_no_value() {
    for (form, parameters, list) in [
        (12, "406,3,1HA,1HB;", "names"),
        (14, "406,3,1HA,1HB;", "values"),
        (24, "406,6,2,1,1HA,1,1HB;", "definitions"),
        (25, "406,5,1HX,3,1,2;", "levels"),
        (27, "406,6,1HN,3,1,5,1;", "values"),
        (34, "406,7,3,1,1,2,1;", "ranges"),
        (35, "406,7,3,1,1,2,1;", "ranges"),
    ] {
        let bytes = owned_test_file(&only(406, form, "PROP", parameters));
        assert_overdeclared_contract(&bytes, 1);

        let result = salvage(&bytes);
        let native = result.ir().native.namespace("iges").unwrap();
        let property = &native.arenas["properties"][0];
        assert!(
            property.fields()[list].as_array().unwrap().is_empty(),
            "form {form} {list} must not be read"
        );
    }
}

#[test]
fn decode_overdeclared_dimension_display_notes_charge_the_loss_and_read_no_note() {
    overdeclared_site(
        &only(
            406,
            30,
            "DIMDISP",
            "406,14,1,0,1,1HL,0,1.5707963267948966,0,0,0,0,0.0,2,1,1,2;",
        ),
        1,
        "properties",
        "declared_value_count",
        14,
        "supplemental_notes",
    );
}

#[test]
fn decode_overdeclared_flag_note_and_general_label_charge_the_loss_and_read_no_leader() {
    for (entity_type, parameters, sequence) in [
        (208, "208,0.0,0.0,0.0,0.0,1,2,3;", 5_u32),
        (210, "210,1,2,3;", 5),
    ] {
        let bytes = owned_test_file(&[
            entity(
                212,
                0,
                "NOTE",
                "212,1,1,1,1,1,1.5707963267948966,0,0,0,0,0,0,1HA;",
            ),
            entity(214, 1, "LEADER", "214,1,1.0,1.0,0.0,0.0,0.0,5.0,5.0;"),
            entity(entity_type, 0, "ANNO", parameters),
        ]);
        assert_overdeclared_contract(&bytes, sequence);

        let result = salvage(&bytes);
        let native = result.ir().native.namespace("iges").unwrap();
        let annotation = native.arenas["annotations"]
            .iter()
            .find(|record| {
                record.fields()["source_entity"] == format!("iges:entity:directory#{sequence}")
            })
            .expect("annotation");
        assert!(
            annotation.fields()["leaders"]
                .as_array()
                .unwrap()
                .is_empty(),
            "type {entity_type} leaders must not be read"
        );
    }
}

#[test]
fn decode_overdeclared_sectioned_area_charges_the_loss_and_reads_no_island() {
    overdeclared_site(
        &[
            entity(100, 0, "BOUND", "100,0.0,0.0,0.0,1.0,0.0,1.0,0.0;"),
            entity(230, 0, "SECTION", "230,1,0,0.0,0.0,0.0,0.0,0.0,2,1;"),
        ],
        3,
        "annotations",
        "declared_island_count",
        2,
        "islands",
    );
}

#[test]
fn decode_sectioned_area_retains_a_negative_declared_island_count() {
    let bytes = owned_test_file(&[
        entity(100, 0, "BOUND", "100,0.0,0.0,0.0,1.0,0.0,1.0,0.0;"),
        entity(230, 0, "SECTION", "230,1,0,0.0,0.0,0.0,0.0,0.0,-1,1;"),
    ]);
    let result = salvage(&bytes);
    let native = result.ir().native.namespace("iges").unwrap();
    let section = &native.arenas["annotations"][0];
    let fields = section.fields();

    assert_eq!(fields["declared_island_count"], -1);
    assert!(fields["islands"].as_array().unwrap().is_empty());
    assert_no_count_loss(&result);
}

fn assert_no_count_loss(result: &cadmpeg_ir::codec::DecodeResult) {
    assert_eq!(
        code_count(result.report(), IgesLossCode::ParameterCountOverdeclared),
        0,
        "{:#?}",
        result.report().losses
    );
}

#[test]
fn decode_units_data_reads_a_final_unit_present_in_part() {
    let bytes = owned_test_file(&only(316, 0, "UNITS", "316,2,2HIN,4HINCH,25.4,2HFT;"));
    let result = salvage(&bytes);
    let native = result.ir().native.namespace("iges").unwrap();
    let units = &native.arenas["units_data"][0];
    let fields = units.fields();
    let list = fields["units"].as_array().unwrap();

    assert_eq!(fields["declared_count"], 2);
    assert_eq!(list.len(), 2);
    assert!(list[1]["unit_value"].is_null());
    assert!(list[1]["scale_factor"].is_null());
    assert_no_count_loss(&result);
}

#[test]
fn decode_external_reference_index_reads_a_final_pair_present_in_part() {
    for form in [2, 12] {
        let bytes = owned_test_file(&[
            point("TARGET"),
            entity(402, form, "INDEX", "402,2,1HA,1,1HB;"),
        ]);
        let result = salvage(&bytes);
        let native = result.ir().native.namespace("iges").unwrap();
        let associativity = &native.arenas["associativities"][0];
        let fields = associativity.fields();
        let entries = fields["entries"].as_array().unwrap();

        assert_eq!(fields["declared_count"], 2);
        assert_eq!(entries.len(), 2);
        assert!(entries[1]["entity"].is_null());
        assert_no_count_loss(&result);
    }
}

#[test]
fn decode_segmented_visibility_reads_a_final_block_present_in_part() {
    let bytes = owned_test_file(&[
        entity(410, 0, "VIEW", "410,1;"),
        entity(402, 19, "SEGVIS", "402,2,1,0.5,1,0,0,0,1,0.75;"),
    ]);
    let result = salvage(&bytes);
    let native = result.ir().native.namespace("iges").unwrap();
    let visibility = &native.arenas["segmented_visibility"][0];
    let fields = visibility.fields();
    let blocks = fields["blocks"].as_array().unwrap();

    assert_eq!(fields["declared_block_count"], 2);
    assert_eq!(blocks.len(), 2);
    assert!(blocks[1]["display_flag"].is_null());
    assert_no_count_loss(&result);
}

// A negative declared count fails `usize::try_from`, so the counted tail is
// `Unreadable` and charges no overdeclaration loss. The retained
// `declared_*_count` field is then the only witness that the file declared
// anything at all. The sectioned-area twin of this test exercises the same
// path through `counted_tail_at`.
#[test]
fn decode_segmented_visibility_retains_a_negative_declared_block_count() {
    let bytes = owned_test_file(&[
        entity(410, 0, "VIEW", "410,1;"),
        entity(402, 19, "SEGVIS", "402,-1,1,0.5,1,0,0,0;"),
    ]);
    let result = salvage(&bytes);
    let native = result.ir().native.namespace("iges").unwrap();
    let visibility = &native.arenas["segmented_visibility"][0];
    let fields = visibility.fields();

    assert_eq!(fields["declared_block_count"], -1);
    assert!(fields["blocks"].as_array().unwrap().is_empty());
    assert_no_count_loss(&result);
}

#[test]
fn decode_view_visibility_reads_both_lists_and_retains_both_declared_counts() {
    let bytes = owned_test_file(&[
        entity(410, 0, "VIEW", "410,1;"),
        entity(402, 3, "VISIBLE", "402,1,1,1,5,0,0;"),
        point("SHOWN"),
    ]);
    let result = salvage(&bytes);
    let native = result.ir().native.namespace("iges").unwrap();
    let fields = native.arenas["view_visibility"][0].fields();
    let displays = fields["displays"].as_array().unwrap();
    let entities = fields["entities"].as_array().unwrap();

    assert_eq!(fields["declared_view_count"], 1);
    assert_eq!(fields["declared_entity_count"], 1);
    assert_eq!(displays.len(), 1);
    assert_eq!(displays[0]["view"], "iges:presentation:view#D1");
    assert_eq!(entities.len(), 1);
    assert_eq!(entities[0], "iges:entity:directory#5");
    assert_no_count_loss(&result);
}

// A chained two-list layout is refused jointly, so neither count has a
// defensible `present` figure and no `parameter.count-overdeclared` verdict
// can be charged for either list. The retained declared counts are the only
// witnesses, which is why the overrun tests below assert
// `assert_no_count_loss` instead of going through `overdeclared_site`.
#[test]
fn decode_view_visibility_retains_both_declared_counts_when_the_entity_list_overruns() {
    let bytes = owned_test_file(&[
        entity(410, 0, "VIEW", "410,1;"),
        entity(402, 3, "VISIBLE", "402,1,2,1,5;"),
        point("SHOWN"),
    ]);
    let result = salvage(&bytes);
    let native = result.ir().native.namespace("iges").unwrap();
    let fields = native.arenas["view_visibility"][0].fields();

    assert_eq!(fields["declared_view_count"], 1);
    assert_eq!(fields["declared_entity_count"], 2);
    assert!(fields["displays"].as_array().unwrap().is_empty());
    assert!(fields["entities"].as_array().unwrap().is_empty());
    assert_no_count_loss(&result);
}

#[test]
fn decode_view_visibility_retains_a_negative_declared_view_count() {
    let bytes = owned_test_file(&[entity(402, 4, "DISPLAY", "402,-1,0,0,0;")]);
    let result = salvage(&bytes);
    let native = result.ir().native.namespace("iges").unwrap();
    let fields = native.arenas["view_visibility"][0].fields();

    assert_eq!(fields["declared_view_count"], -1);
    assert_eq!(fields["declared_entity_count"], 0);
    assert!(fields["displays"].as_array().unwrap().is_empty());
    assert!(fields["entities"].as_array().unwrap().is_empty());
    assert_no_count_loss(&result);
}

#[test]
fn decode_drawing_reads_both_lists_and_retains_both_declared_counts() {
    let bytes = owned_test_file(&[
        entity(410, 0, "VIEW", "410,1;"),
        entity(404, 1, "DRAWING", "404,1,1,10,20,0.5,1,5,0,0;"),
        point("ANNOT"),
    ]);
    let result = salvage(&bytes);
    let native = result.ir().native.namespace("iges").unwrap();
    let fields = native.arenas["drawings"][0].fields();
    let views = fields["views"].as_array().unwrap();
    let annotations = fields["annotations"].as_array().unwrap();

    assert_eq!(fields["declared_view_count"], 1);
    assert_eq!(fields["declared_annotation_count"], 1);
    assert_eq!(views.len(), 1);
    assert_eq!(views[0]["view"], "iges:presentation:view#D1");
    assert!((views[0]["rotation"].as_f64().unwrap() - 0.5).abs() < 1e-12);
    assert_eq!(annotations.len(), 1);
    assert_eq!(annotations[0], "iges:entity:directory#5");
    assert_no_count_loss(&result);
}

#[test]
fn decode_drawing_retains_both_declared_counts_when_the_annotation_list_overruns() {
    let bytes = owned_test_file(&[
        entity(410, 0, "VIEW", "410,1;"),
        entity(404, 0, "DRAWING", "404,1,1,10,20,3,5;"),
        point("ANNOT"),
    ]);
    let result = salvage(&bytes);
    let native = result.ir().native.namespace("iges").unwrap();
    let fields = native.arenas["drawings"][0].fields();

    assert_eq!(fields["declared_view_count"], 1);
    assert_eq!(fields["declared_annotation_count"], 3);
    assert!(fields["views"].as_array().unwrap().is_empty());
    assert!(fields["annotations"].as_array().unwrap().is_empty());
    assert_no_count_loss(&result);
}

#[test]
fn decode_drawing_with_a_negative_declared_view_count_locates_no_annotation_count() {
    let bytes = owned_test_file(&[
        entity(410, 0, "VIEW", "410,1;"),
        entity(404, 0, "DRAWING", "404,-1,1,10,20,1,5,0,0;"),
        point("ANNOT"),
    ]);
    let result = salvage(&bytes);
    let native = result.ir().native.namespace("iges").unwrap();
    let fields = native.arenas["drawings"][0].fields();

    assert_eq!(fields["declared_view_count"], -1);
    assert!(fields["declared_annotation_count"].is_null());
    assert!(fields["views"].as_array().unwrap().is_empty());
    assert!(fields["annotations"].as_array().unwrap().is_empty());
    assert_no_count_loss(&result);
}

#[test]
fn decode_general_symbol_retains_both_declared_counts_when_the_leader_list_overruns() {
    let bytes = owned_test_file(&[
        entity(
            212,
            0,
            "NOTE",
            "212,1,1,1,1,1,1.5707963267948966,0,0,0,0,0,0,1HS;",
        ),
        entity(100, 0, "GEOMETRY", "100,0,0,0,1,0,1,0;"),
        entity(228, 0, "SYMBOL", "228,1,1,3,2,5;"),
    ]);
    let result = salvage(&bytes);
    let native = result.ir().native.namespace("iges").unwrap();
    let symbol = native.arenas["annotations"]
        .iter()
        .find(|record| record.fields()["kind"] == "general_symbol")
        .expect("general symbol");
    let fields = symbol.fields();

    assert_eq!(fields["declared_geometry_count"], 1);
    assert_eq!(fields["declared_leader_count"], 2);
    assert!(fields["geometry"].as_array().unwrap().is_empty());
    assert!(fields["leaders"].as_array().unwrap().is_empty());
    assert_no_count_loss(&result);
}

#[test]
fn decode_manifold_solid_reads_its_shell_uses_and_resolves_both_closed_shells() {
    let (bytes, solid, outer, void) = explicit_void_solid_file();
    let result = salvage(&bytes);
    let native = result.ir().native.namespace("iges").unwrap();
    let solids = &native.arenas["manifold_solids"];
    assert_eq!(solids.len(), 1);
    let fields = solids[0].fields();
    let voids = fields["voids"].as_array().unwrap();

    assert_eq!(solids[0].id(), format!("iges:solid:manifold-brep#D{solid}"));
    assert_eq!(
        fields["source_entity"],
        format!("iges:entity:directory#{solid}")
    );
    assert_eq!(fields["shell"], format!("iges:entity:directory#{outer}"));
    assert_eq!(fields["shell_orientation"], 1);
    assert_eq!(fields["declared_void_count"], 1);
    assert!(fields["transformation"].is_null());
    assert_eq!(voids.len(), 1);
    assert_eq!(voids[0]["shell"], format!("iges:entity:directory#{void}"));
    assert_eq!(voids[0]["orientation"], 0);
    assert_no_count_loss(&result);
}

// §4.49 gives VOF no default, so a final pair the record delimiter cuts
// short keeps a null orientation instead of an invented flag. The trailing
// partial pair is admitted by the `div_ceil` branch of
// `items_before_default_tail_at`, the same path the segmented-visibility
// twin exercises.
#[test]
fn decode_manifold_solid_reads_a_final_void_shell_use_present_in_part() {
    let (entities, solid, _, void) =
        manifold_solid_entities(|outer, void| format!("186,{outer},1,1,{void};"));
    let result = salvage(&owned_test_file(&entities));
    let native = result.ir().native.namespace("iges").unwrap();
    let fields = native.arenas["manifold_solids"][0].fields();
    let voids = fields["voids"].as_array().unwrap();

    assert_eq!(
        fields["source_entity"],
        format!("iges:entity:directory#{solid}")
    );
    assert_eq!(fields["declared_void_count"], 1);
    assert_eq!(voids.len(), 1);
    assert_eq!(voids[0]["shell"], format!("iges:entity:directory#{void}"));
    assert!(voids[0]["orientation"].is_null());
    assert_no_count_loss(&result);
}

#[test]
fn decode_manifold_solid_retains_a_negative_declared_void_count() {
    let (entities, _, outer, _) = manifold_solid_entities(|outer, _| format!("186,{outer},1,-1;"));
    let result = salvage(&owned_test_file(&entities));
    let native = result.ir().native.namespace("iges").unwrap();
    let fields = native.arenas["manifold_solids"][0].fields();

    assert_eq!(fields["declared_void_count"], -1);
    assert!(fields["voids"].as_array().unwrap().is_empty());
    assert_eq!(fields["shell"], format!("iges:entity:directory#{outer}"));
    assert_eq!(fields["shell_orientation"], 1);
    assert_no_count_loss(&result);
}

#[test]
fn decode_manifold_solid_leaves_an_open_shell_pointer_unresolved() {
    let (mut entities, closed_solid, outer, _) =
        manifold_solid_entities(|outer, _| format!("186,{outer},1,0;"));
    entities.push(entity(514, 2, "OPEN", "514,0;"));
    let open_shell = u32::try_from(entities.len() * 2 - 1).unwrap();
    let open_solid = u32::try_from(entities.len() * 2 + 1).unwrap();
    entities.push(entity(186, 0, "OPENSLD", &format!("186,{open_shell},1,0;")));
    let result = salvage(&owned_test_file(&entities));
    let native = result.ir().native.namespace("iges").unwrap();
    let solids = &native.arenas["manifold_solids"];
    assert_eq!(solids.len(), 2);
    let closed = solids[0].fields();
    let rejected = solids[1].fields();

    assert_eq!(
        closed["source_entity"],
        format!("iges:entity:directory#{closed_solid}")
    );
    assert_eq!(closed["shell"], format!("iges:entity:directory#{outer}"));
    assert_eq!(
        rejected["source_entity"],
        format!("iges:entity:directory#{open_solid}")
    );
    assert!(rejected["shell"].is_null());
    assert_eq!(rejected["declared_void_count"], 0);
    assert!(rejected["voids"].as_array().unwrap().is_empty());
    assert_eq!(
        code_count(result.report(), IgesLossCode::PointerUnresolved),
        1
    );
    assert_no_count_loss(&result);
}

#[test]
fn decode_text_score_reads_a_final_range_present_in_part() {
    let bytes = owned_test_file(&only(406, 34, "UNDER", "406,7,2,1,1,2,1,1;"));
    let result = salvage(&bytes);
    let native = result.ir().native.namespace("iges").unwrap();
    let property = &native.arenas["properties"][0];
    let fields = property.fields();
    let ranges = fields["ranges"].as_array().unwrap();

    assert_eq!(ranges.len(), 2);
    assert!(ranges[1]["last_character"].is_null());
    assert_no_count_loss(&result);
}

#[test]
fn decode_leader_reads_a_final_segment_present_in_part() {
    let bytes = owned_test_file(&only(
        214,
        1,
        "LEADER",
        "214,2,1.0,1.0,0.0,0.0,0.0,5.0,5.0,7.0;",
    ));
    let result = salvage(&bytes);
    let native = result.ir().native.namespace("iges").unwrap();
    let leader = &native.arenas["annotations"][0];
    let fields = leader.fields();
    let tails = fields["segment_tails"].as_array().unwrap();

    assert_eq!(fields["declared_segment_count"], 2);
    assert_eq!(tails.len(), 2);
    assert_eq!(tails[1][0], 7.0);
    assert!(tails[1][1].is_null());
    assert_no_count_loss(&result);
}

#[test]
fn decode_copious_data_reads_a_final_tuple_present_in_part() {
    let bytes = owned_test_file(&[OwnedTestEntity {
        entity_type: 106,
        form: 1,
        label: "COPIOUS".into(),
        status: "00000000",
        parameters: "106,1,2,0.5,1,2,3;".into(),
    }]);
    let result = salvage(&bytes);
    let native = result.ir().native.namespace("iges").unwrap();
    let copious = &native.arenas["copious_data"][0];
    let fields = copious.fields();
    let tuples = fields["tuples"].as_array().unwrap();

    assert_eq!(fields["declared_tuple_count"], 2);
    assert_eq!(tuples.len(), 2);
    assert_eq!(tuples[1][0], 3.0);
    assert!(tuples[1][1].is_null());
    assert_no_count_loss(&result);
}

#[test]
fn decode_line_font_pattern_holds_only_complete_lengths_before_its_suffix() {
    let bytes = owned_test_file(&only(304, 2, "PATTERN", "304,2,1.0,2.0,2H0F;"));
    let result = salvage(&bytes);
    let native = result.ir().native.namespace("iges").unwrap();
    let font = &native.arenas["line_fonts"][0];
    let fields = font.fields();

    assert_eq!(fields["segment_count"], 2);
    assert_eq!(fields["lengths"].as_array().unwrap().len(), 2);
    assert!(fields["hexadecimal_pattern"].is_array());
    assert_no_count_loss(&result);
}

#[test]
fn decode_charges_one_count_loss_per_entry_in_directory_sequence_order() {
    let bytes = owned_test_file(&[
        entity(406, 1, "LEVELS", "406,3,10,20;"),
        entity(316, 0, "UNITS", "316,3,2HIN,4HINCH,25.4,2HFT;"),
        entity(
            212,
            0,
            "NOTE",
            "212,2,1,1,1,1,1.5707963267948966,0,0,0,0,0,0,1HA;",
        ),
    ]);
    let result = salvage(&bytes);
    let charged = result
        .report()
        .losses
        .iter()
        .filter(|loss| loss.code == IgesLossCode::ParameterCountOverdeclared.kind())
        .map(|loss| {
            loss.provenance
                .as_ref()
                .and_then(|source| source.tag.clone())
                .unwrap_or_default()
        })
        .collect::<Vec<_>>();

    assert_eq!(
        charged,
        [
            "directory_entry:D1".to_owned(),
            "directory_entry:D3".to_owned(),
            "directory_entry:D5".to_owned()
        ]
    );
}

#[test]
fn decode_charges_the_count_loss_once_for_an_entry_with_two_counted_lists() {
    let bytes = owned_test_file(&only(
        406,
        30,
        "DIMDISP",
        "406,14,1,0,1,1HL,0,1.5707963267948966,0,0,0,0,0.0,2,1,1,2;",
    ));
    assert_overdeclared_contract(&bytes, 1);
}

#[test]
fn decode_attribute_definition_holds_its_count_while_the_nested_triple_stays_empty() {
    let bytes = owned_test_file(&only(322, 0, "ATTRDEF", "322,4HATTR,0,1,1,1;"));
    let result = salvage(&bytes);
    let native = result.ir().native.namespace("iges").unwrap();
    let definition = &native.arenas["attribute_table_definitions"][0];

    assert_eq!(definition.fields()["declared_attribute_count"], 1);
    assert!(definition.fields()["attributes"]
        .as_array()
        .unwrap()
        .is_empty());
    assert_no_count_loss(&result);
}

#[test]
fn decode_attribute_instance_rows_clamp_to_the_values_the_record_holds() {
    for (parameters, rows) in [
        ("422,2,8,4HIRON,9,5HBRASS;", 2),
        ("422,1,8,4HIRON,9,5HBRASS;", 1),
        ("422,0,8,4HIRON;", 0),
        ("422,3,8,4HIRON,9,5HBRASS;", 0),
        ("422,2,8,4HIRON,9;", 0),
        ("422,,8,4HIRON;", 0),
        ("422,-3,8,4HIRON;", 0),
        ("422,9223372036854775807,8,4HIRON;", 0),
    ] {
        let entities = [
            OwnedTestEntity {
                entity_type: 322,
                form: 0,
                label: "ATTRDEF".into(),
                status: "00000000",
                parameters: "322,4HMETA,1,2,10,1,1,11,3,1;".into(),
            },
            OwnedTestEntity {
                entity_type: 422,
                form: 1,
                label: "ATTRTAB".into(),
                status: "00000000",
                parameters: parameters.into(),
            },
        ];
        let bytes = owned_test_file_with_structures(&entities, &[(3, -1)]);
        let result = salvage(&bytes);
        let native = result.ir().native.namespace("iges").unwrap();
        let instance = &native.arenas["attribute_table_instances"][0];
        let fields = instance.fields();

        assert_eq!(
            fields["definition"], "iges:product:attribute-definition#D1",
            "{parameters}"
        );
        let read = fields["rows"].as_array().unwrap();
        assert_eq!(read.len(), rows, "{parameters}");
        assert!(
            read.iter().all(|row| row.as_array().unwrap().len() == 2),
            "{parameters}"
        );
    }
}
