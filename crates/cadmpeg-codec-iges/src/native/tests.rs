// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]
#![allow(unused_imports)]

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::io::{self, Cursor, Read, Seek, SeekFrom};

use cadmpeg_core::decode::DecodeMode;
use cadmpeg_core::decode::ResourceDimension;
use cadmpeg_core::CodecError;
use cadmpeg_ir::codec::{Codec, CodecBackend, Confidence, DecodeOptions, EncodeInput, Encoder};
use cadmpeg_ir::geometry::{
    Curve, CurveGeometry, NurbsCurve, NurbsSurface, Pcurve, PcurveGeometry, Surface,
    SurfaceGeometry,
};
use cadmpeg_ir::ids::{
    BodyId, CoedgeId, CurveId, EdgeId, FaceId, LoopId, PcurveId, PointId, RegionId, ShellId,
    SurfaceId, VertexId,
};
use cadmpeg_ir::math::{Point2, Point3, Vector3};
use cadmpeg_ir::report::WritePath;
use cadmpeg_ir::topology::{
    Body, BodyKind, Coedge, Edge, Face, Loop, LoopBoundaryRole, Point, Region, Sense, Shell, Vertex,
};
use cadmpeg_ir::units::Units;
use cadmpeg_ir::CadIr;

use crate::test_support::*;
use crate::{IgesCodec, IgesEncoder, IgesVersion, IgesWriteOptions};

#[test]
fn every_admitted_entity_form_routes_to_a_typed_decoder_or_native_retention_loss() {
    let matrix_path =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../corpus/iges-envelope-a.toml");
    let source = std::fs::read_to_string(matrix_path).unwrap();
    let matrix = toml::from_str::<toml::Value>(&source).unwrap();
    let entities = matrix["entity"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|entity| {
            let entity_type = entity["type"].as_integer().unwrap();
            let forms = entity["forms"].as_array().map_or_else(
                || vec![5001, 9999],
                |forms| {
                    forms
                        .iter()
                        .map(|form| form.as_integer().unwrap())
                        .collect()
                },
            );
            forms.into_iter().map(move |form| OwnedTestEntity {
                entity_type,
                form,
                label: format!("E{entity_type}"),
                status: "00000000",
                parameters: format!("{entity_type};"),
            })
        })
        .collect::<Vec<_>>();
    let bytes = owned_test_file(&entities);

    let result = IgesCodec
        .decode(
            &mut Cursor::new(bytes.as_slice()),
            &DecodeOptions::default(),
        )
        .unwrap();

    let generic_fallthroughs = result
        .report()
        .losses
        .iter()
        .filter(|loss| {
            loss.message
                .ends_with("retained without neutral projection")
        })
        .map(|loss| loss.message.as_str())
        .collect::<Vec<_>>();
    assert_eq!(
        generic_fallthroughs,
        vec![
            "IGES entity type 124 form 0 retained without neutral projection",
            "IGES entity type 124 form 1 retained without neutral projection",
            "IGES entity type 124 form 10 retained without neutral projection",
            "IGES entity type 124 form 11 retained without neutral projection",
            "IGES entity type 124 form 12 retained without neutral projection",
        ]
    );
}

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
            parameters: "106,1,2,0.5,1,2,3;".into(),
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
    assert_eq!(copious.fields()["declared_tuple_count"], 2);
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
fn decode_label_display_truncated_count_rejects_trailing_property_group() {
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
    assert!(associativity.fields()["placements"]
        .as_array()
        .unwrap()
        .is_empty());
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
fn decode_general_note_truncated_count_rejects_trailing_property_group() {
    let bytes = owned_test_file(&[
        OwnedTestEntity {
            entity_type: 212,
            form: 0,
            label: "NOTE".into(),
            status: "00000200",
            parameters: "212,2,1,1,1,1,1,1.5707963267948966,0,0,0,0,0,0,1HA,0,1,3;".into(),
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
    let annotation = &native.arenas["annotations"][0];

    assert!(entity.fields()["property_links"][0].is_null());
    assert_eq!(annotation.fields()["declared_string_count"], 2);
    assert!(annotation.fields()["strings"]
        .as_array()
        .unwrap()
        .is_empty());
}

#[test]
fn decode_new_general_note_partial_final_block_rejects_extra_string() {
    let bytes = owned_test_file(&[OwnedTestEntity {
        entity_type: 213,
        form: 0,
        label: "NOTE".into(),
        status: "00000200",
        parameters: "213,1,1,0,0,0,0,0,0,0,0,0,2,1,1,1,1,0,1,0,0H,1,1,1,1,1,1.5707963267948966,0,0,0,0,0,1HA,1,1,1,1,0,1,0,0H,2;".into(),
    }]);
    let result = IgesCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    let annotation = &result.ir().native.namespace("iges").unwrap().arenas["annotations"][0];

    assert_eq!(annotation.fields()["declared_string_count"], 2);
    assert!(annotation.fields()["strings"]
        .as_array()
        .unwrap()
        .is_empty());
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
fn decode_preserves_native_entities_and_graph() {
    let bytes = point_file();

    let result = IgesCodec
        .decode(
            &mut Cursor::new(bytes.as_slice()),
            &DecodeOptions::default(),
        )
        .unwrap();

    assert_eq!(result.ir().source.as_ref().unwrap().format, "iges");
    assert_eq!(
        result.ir().source.as_ref().unwrap().attributes["document_local_sha256"],
        crate::document_digest(result.ir())
    );
    assert_eq!(
        result
            .source_fidelity()
            .retained_record(crate::SOURCE_IMAGE_ID)
            .unwrap()
            .data
            .as_deref(),
        Some(bytes.as_slice())
    );
    let native = result.ir().native.namespace("iges").unwrap();
    assert_eq!(native.version, 3);
    assert_eq!(native.arenas["cards"].len(), 7);
    assert_eq!(native.arenas["entities"].len(), 1);
    assert!(native.arenas["colors"].is_empty());
    assert_eq!(native.arenas["display_attributes"].len(), 1);
    assert!(!native.arenas.contains_key("opaque_bytes"));
    assert_eq!(native.arenas["entities"][0].id(), "iges:entity:directory#1");
    assert_eq!(result.ir().model.points.len(), 1);
    assert_eq!(result.ir().model.points[0].position.x, 1.0);
    assert_eq!(result.ir().model.points[0].position.y, 2.0);
    assert_eq!(result.ir().model.points[0].position.z, 3.0);
    assert_eq!(result.ir().model.vertices.len(), 1);
    assert!(result.report().geometry_transferred);
    assert!(!result.report().losses.iter().any(|loss| {
        loss.message == "IGES entity type 116 form 0 retained without neutral projection"
    }));
    let validation = cadmpeg_ir::validate_neutral(result.ir(), Vec::new());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
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
