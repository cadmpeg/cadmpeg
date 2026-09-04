// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use std::collections::BTreeSet;
use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::Exactness;

use crate::container::{self};
use crate::loss::CreoLossCode;
use crate::test_support::*;
use crate::CreoCodec;

#[test]
fn decode_identifies_variable_round_form_from_differing_complete_envelopes() {
    let geometry = |type_bytes: [u8; 2]| {
        let mut geometry = b"srf_array\0\xf8\x02".to_vec();
        for ((surface_id, next_surface, diameter, extent), type_byte) in
            [(7, 8, 1.0, [1.0, 2.0, 2.0]), (8, 0, 2.0, [2.0, 1.0, 1.0])]
                .into_iter()
                .zip(type_bytes)
        {
            geometry.extend_from_slice(&[surface_id, type_byte, 4, 0x01, 0, next_surface]);
            geometry.push(0x15);
            for value in [0.0, 0.0, diameter, 0.0, 0.0, 0.0]
                .into_iter()
                .chain(extent)
            {
                push_generated_scalar(&mut geometry, value);
            }
            geometry.push(0xe3);
        }
        geometry.extend_from_slice(b"crv_array\0\xf3\xf8\0");
        geometry
    };
    let allfeatur = b"\x04\xeb\x04\x00\x10\x01\x00\xe5\xe3\xf6\x83\x91\xe1".to_vec();
    let data = build_prt(
        "c",
        &[
            ("VisibGeom", geometry([0x24, 0x24])),
            ("AllFeatur", allfeatur.clone()),
            ("MdlStatus", b"Round id 4\0".to_vec()),
        ],
    );

    let result = CreoCodec
        .decode(&mut Cursor::new(data), &DecodeOptions::default())
        .expect("decode");
    let feature = result
        .ir()
        .model
        .features
        .iter()
        .find(|feature| feature.id.as_str() == "creo:model:feature#4")
        .expect("round feature");
    assert!(matches!(
        feature.definition,
        cadmpeg_ir::features::FeatureDefinition::Fillet {
            ref groups,
        } if matches!(groups.as_slice(), [cadmpeg_ir::features::FilletGroup {
            radius: cadmpeg_ir::features::RadiusSpec::Unresolved {
                form: Some(cadmpeg_ir::features::RadiusForm::Variable)
            }, ..
        }])
    ));

    let mixed = build_prt(
        "c",
        &[
            ("VisibGeom", geometry([0x24, 0x26])),
            ("AllFeatur", allfeatur),
            ("MdlStatus", b"Round id 4\0".to_vec()),
        ],
    );
    let mixed = CreoCodec
        .decode(&mut Cursor::new(mixed), &DecodeOptions::default())
        .expect("decode");
    assert!(matches!(
        mixed.ir().model.features[0].definition,
        cadmpeg_ir::features::FeatureDefinition::Fillet {
            ref groups,
        } if matches!(groups.as_slice(), [cadmpeg_ir::features::FilletGroup {
            radius: cadmpeg_ir::features::RadiusSpec::Unresolved { form: None }, ..
        }])
    ));
}

#[test]
fn decode_transfers_strong_parents_as_ordered_dependencies() {
    let mut datum = b"srf_array\0\xf8\x01".to_vec();
    datum.extend([4, 0x22, 1, 1, 1, 0]);
    datum.extend([0x0f; 4]);
    datum.extend([0x46, 0, 0, 0, 0, 0, 0, 0]);
    datum.push(0x0f);
    datum.extend([0x46, 0x08, 0, 0, 0, 0, 0, 0]);
    datum.extend([0x2d, 0, 0, 0, 0, 0, 0, 0]);
    datum.push(0x0f);
    datum.extend([0x2d, 0x08, 0, 0, 0, 0, 0, 0]);
    let mut geometry = visibgeom_payload(1, 0);
    geometry.extend_from_slice(&[7, 0x22, 4, 0x01, 0, 0]);
    let allfeatur = allfeatur_row(
        4,
        [0xeb, 0x04],
        917,
        b"\xe0\x01parent_table\0\xf8\x01\x01\
        \xe0\x21strong_parents\0\xf8\x02\x02\x01",
    );
    let data = build_prt(
        "c",
        &[
            ("ActDatums", datum),
            ("VisibGeom", geometry),
            ("AllFeatur", allfeatur),
            ("MdlStatus", b"Datum Plane id 2\0Protrusion id 4\0".to_vec()),
        ],
    );
    let result = CreoCodec
        .decode(&mut Cursor::new(data), &DecodeOptions::default())
        .expect("decode");
    let feature = result
        .ir()
        .model
        .features
        .iter()
        .find(|feature| feature.id.as_str() == "creo:model:feature#4")
        .expect("feature 4");
    assert!(result.ir().model.feature_parent(&feature.id).is_none());
    assert_eq!(
        feature
            .dependencies
            .iter()
            .map(cadmpeg_ir::FeatureId::as_str)
            .collect::<Vec<_>>(),
        vec!["creo:model:feature#1", "creo:model:feature#2"]
    );
    let validation = cadmpeg_ir::validate_neutral(result.ir(), result.report().losses.clone());
    assert!(validation.is_ok(), "{validation:#?}");
}

#[test]
fn decode_resolves_feature_dependencies_independently_of_storage_order() {
    let mut datum = b"srf_array\0\xf8\x01".to_vec();
    datum.extend([4, 0x22, 1, 1, 1, 0]);
    datum.extend([0x0f; 4]);
    datum.extend([0x46, 0, 0, 0, 0, 0, 0, 0]);
    datum.push(0x0f);
    datum.extend([0x46, 0x08, 0, 0, 0, 0, 0, 0]);
    datum.extend([0x2d, 0, 0, 0, 0, 0, 0, 0]);
    datum.push(0x0f);
    datum.extend([0x2d, 0x08, 0, 0, 0, 0, 0, 0]);
    let mut geometry = visibgeom_payload(1, 0);
    geometry.extend_from_slice(&[7, 0x22, 4, 0x01, 0, 0]);
    let allfeatur = allfeatur_row(
        4,
        [0xeb, 0x04],
        917,
        b"\xe0\x01parent_table\0\xf8\x01\x01\
        \xe0\x21strong_parents\0\xf8\x02\x02\x01",
    );
    let data = build_prt(
        "c",
        &[
            ("ActDatums", datum),
            ("VisibGeom", geometry),
            ("AllFeatur", allfeatur),
            ("MdlStatus", b"Protrusion id 4\0Datum Plane id 2\0".to_vec()),
        ],
    );
    let result = CreoCodec
        .decode(&mut Cursor::new(data), &DecodeOptions::default())
        .expect("decode");
    let feature = result
        .ir()
        .model
        .features
        .iter()
        .find(|feature| feature.id.as_str() == "creo:model:feature#4")
        .expect("feature 4");
    assert_eq!(
        feature
            .dependencies
            .iter()
            .map(cadmpeg_ir::FeatureId::as_str)
            .collect::<Vec<_>>(),
        vec!["creo:model:feature#1", "creo:model:feature#2"]
    );
    let validation = cadmpeg_ir::validate_neutral(result.ir(), result.report().losses.clone());
    assert!(validation.is_ok(), "{validation:#?}");
}

#[test]
fn decode_retains_recipe_proven_revolution_with_unresolved_operands() {
    let mdlstatus = b"\xe3icon\0cutrevolve\0K\xc3\xb6rper id 40\0".to_vec();
    let data = build_prt("c", &[("MdlStatus", mdlstatus)]);
    let result = CreoCodec
        .decode(&mut Cursor::new(data), &DecodeOptions::default())
        .expect("decode");
    let feature = result
        .ir()
        .model
        .features
        .iter()
        .find(|feature| feature.id.0 == "creo:model:feature#40")
        .expect("revolution feature");

    assert!(matches!(
        &feature.definition,
        cadmpeg_ir::features::FeatureDefinition::Revolve {
            construction: cadmpeg_ir::features::RevolutionConstruction {
                profile: None,
                axis: None,
                extent: None,
                ..
            },
            op: cadmpeg_ir::features::BooleanOp::Cut,
        }
    ));
}

#[test]
fn decode_retains_recipe_proven_extrusion_with_unresolved_operands() {
    let mdlstatus = b"\xe3icon\0cutextrude\0K\xc3\xb6rper id 40\0".to_vec();
    let data = build_prt("c", &[("MdlStatus", mdlstatus)]);
    let result = CreoCodec
        .decode(&mut Cursor::new(data), &DecodeOptions::default())
        .expect("decode");
    let feature = result
        .ir()
        .model
        .features
        .iter()
        .find(|feature| feature.id.0 == "creo:model:feature#40")
        .expect("extrusion feature");

    assert!(matches!(
        &feature.definition,
        cadmpeg_ir::features::FeatureDefinition::Extrude {
            profile: cadmpeg_ir::features::ProfileRef::Unresolved(_),
            direction: cadmpeg_ir::features::ExtrudeDirection::ProfileNormal,
            extent: cadmpeg_ir::features::ExtrudeExtent::OneSided {
                side: cadmpeg_ir::features::ExtrudeSide {
                    termination: cadmpeg_ir::features::LinearTermination::Unresolved,
                    ..
                }
            },
            op: cadmpeg_ir::features::BooleanOp::Cut,
            ..
        }
    ));
}

#[test]
fn decode_recipe_supplies_reference_backed_extrusion_boolean_effect() {
    let mdlstatus = b"\xe3icon\0cutextrude\0Extrude 1 id 40\0".to_vec();
    let data = build_prt("c", &[("MdlStatus", mdlstatus)]);
    let result = CreoCodec
        .decode(&mut Cursor::new(data), &DecodeOptions::default())
        .expect("decode");
    let feature = result
        .ir()
        .model
        .features
        .iter()
        .find(|feature| feature.id.0 == "creo:model:feature#40")
        .expect("reference-backed extrusion feature");

    assert_eq!(feature.name.as_deref(), Some("Extrude 1 id 40"));
    assert!(matches!(
        feature.definition,
        cadmpeg_ir::features::FeatureDefinition::Extrude {
            profile: cadmpeg_ir::features::ProfileRef::Unresolved(_),
            direction: cadmpeg_ir::features::ExtrudeDirection::ProfileNormal,
            extent: cadmpeg_ir::features::ExtrudeExtent::OneSided {
                side: cadmpeg_ir::features::ExtrudeSide {
                    termination: cadmpeg_ir::features::LinearTermination::Unresolved,
                    ..
                }
            },
            op: cadmpeg_ir::features::BooleanOp::Cut,
            ..
        }
    ));
}

#[test]
fn decode_transfers_featdefs_sketch_variables_as_native_design_data() {
    let mut payload =
        b"feat_defs_40\0var_arr\0\xf8\x03\xf7\x01\xfb\xe2schema\xf1\xf7\x01\xe2".to_vec();
    payload.extend_from_slice(&[1, 7, 0xe4, 0x0f, 1, 0, 3, 0xe2]);
    payload.extend_from_slice(&[2, 7, 0x46, 0x08, 0, 0, 0, 0, 0, 0, 0x0f, 1, 0, 4, 0xe2]);
    payload.extend_from_slice(&[3, 6, 0x46, 0x10, 0, 0, 0, 0, 0, 0, 0x0f, 1, 0, 6, 0xe2]);
    let definition_length = payload.len();
    let data = build_prt("c", &[("FeatDefs", payload)]);
    let scan = container::scan_bytes(data.clone());
    let offset = scan.features.definitions[0].offset as u64;
    let variable_offset = scan.features.definitions[0]
        .variables
        .as_ref()
        .unwrap()
        .rows[0]
        .offset;
    let result = CreoCodec
        .decode(&mut Cursor::new(data), &DecodeOptions::default())
        .expect("decode");

    let namespace = result
        .ir()
        .native
        .namespace("creo")
        .expect("creo namespace");
    assert_eq!(namespace.version(), 1);
    let definitions = &namespace.arenas["feature_definitions"];
    assert_eq!(definitions.len(), 1);
    assert_eq!(definitions[0].id(), "creo:featdefs:feature_definition#40");
    assert_eq!(definitions[0].fields()["definition_id"], 40);
    assert_eq!(
        definitions[0].fields()["body"].as_array().unwrap().len(),
        definition_length
    );
    let sketches = &namespace.arenas["sketches"];
    assert_eq!(sketches.len(), 1);
    assert_eq!(sketches[0].id(), "creo:featdefs:sketch#40");
    assert_eq!(sketches[0].fields()["definition_id"], 40);
    assert!(sketches[0].fields()["owner_feature_id"].is_null());
    let sketch_fields = sketches[0].fields();
    let headers = sketch_fields["table_headers"]
        .as_array()
        .expect("table headers");
    assert_eq!(headers.len(), 1);
    assert_eq!(headers[0]["kind"], "variables");
    assert_eq!(headers[0]["declared_count"], 3);
    assert_eq!(headers[0]["entity_ref"], 1);
    assert_eq!(headers[0]["row_count"], 3);
    let points = sketch_fields["section_points"]
        .as_array()
        .expect("section points");
    assert_eq!(points.len(), 1);
    assert_eq!(points[0]["point_id"], 7);
    assert_eq!(points[0]["u"], 1.0);
    assert_eq!(points[0]["v"], 3.0);
    assert_eq!(points[0]["state"], "resolved");
    let variables = sketch_fields["variables"]
        .as_array()
        .expect("variables array");
    assert_eq!(variables.len(), 3);
    assert_eq!(variables[0]["key"], 7);
    assert_eq!(variables[0]["value"], 1.0);
    assert_eq!(
        variables[0]["value_body"].as_array().expect("value body"),
        &[228]
    );
    assert_eq!(
        variables[0]["guess_body"].as_array().expect("guess body"),
        &[15]
    );
    assert_eq!(variables[0]["guess_dimension_driven"], false);
    assert_eq!(variables[0]["resolved_value"], 1.0);
    assert_eq!(variables[0]["known"], 1);
    assert_eq!(variables[0]["homogeneity"], 0);
    assert_eq!(variables[0]["uvar_id"], 3);
    assert_eq!(variables[0]["offset"], variable_offset);
    assert_eq!(variables[1]["value"], 3.0);
    assert_eq!(
        variables[1]["value_body"].as_array().expect("value body"),
        &[70, 8, 0, 0, 0, 0, 0, 0]
    );
    assert_eq!(
        variables[1]["guess_body"].as_array().expect("guess body"),
        &[15]
    );
    assert_eq!(variables[1]["resolved_value"], 3.0);
    assert_eq!(variables[1]["known"], 1);
    assert_eq!(variables[1]["homogeneity"], 0);
    assert_eq!(variables[1]["uvar_id"], 4);
    assert_eq!(variables[2]["variable_type"], 3);
    assert_eq!(variables[2]["key"], 6);
    assert_eq!(variables[2]["value"], 4.0);
    assert_eq!(variables[2]["resolved_value"], 4.0);
    assert_eq!(variables[2]["known"], 1);
    assert_eq!(variables[2]["homogeneity"], 0);
    assert_eq!(variables[2]["uvar_id"], 6);
    assert_annotation(
        &result.source_fidelity().annotations,
        "creo:featdefs:sketch#40",
        "creo:FeatDefs",
        offset,
        "feature_sketch",
        Exactness::Derived,
    );
}

#[test]
fn decode_transfers_feature_dimensions_as_owned_parameters() {
    let payload = b"feat_defs_917\0\xe0\x01feat_id\0\x28\xe0\x00gsec2d_ptr\0\
        dimtab_ptr\0\xf8\x02\xf7\x81\x02\xfb\xe2\
        \xe0\x01type\0\x0a\xe0\x01value\0\xe4\
        \xe0\x01direct\0\x01\xe0\x01aux_value\0\x0f\
        \xe0\x01ext_id\0\x2a\xf3\xf7\x81\x02\xe2\
        \x0a\xe4\x01\x18\x2a\xe0\x00relat_ptr\0"
        .to_vec();
    let data = build_prt(
        "c",
        &[
            ("FeatDefs", payload),
            ("MdlStatus", b"Extrude id 40\0".to_vec()),
            (
                "DEPDB_DATA",
                b"\xe0\x00entity(crv_fr_eqn)\0\xe3\xe0\x01id\0\x07\
                    \xe0\x0aexpression\0\xf8\x01result=d42+1[deg]\0"
                    .to_vec(),
            ),
        ],
    );
    let scan = container::scan_bytes(data.clone());
    assert_eq!(scan.features.definitions[0].id, 917);
    assert_eq!(scan.features.definitions[0].owner_feature_id, Some(40));
    let result = CreoCodec
        .decode(&mut Cursor::new(data), &DecodeOptions::default())
        .expect("decode");

    assert_eq!(result.ir().model.parameters.len(), 3);
    let parameter = result
        .ir()
        .model
        .parameters
        .iter()
        .find(|parameter| parameter.name == "d917_42_1")
        .expect("first repeated dimension");
    let repeated = result
        .ir()
        .model
        .parameters
        .iter()
        .find(|parameter| parameter.name == "d917_42_2")
        .expect("second repeated dimension");
    let relation = result
        .ir()
        .model
        .parameters
        .iter()
        .find(|parameter| parameter.name == "result")
        .expect("relation parameter");
    assert_eq!(
        parameter.owner.as_ref().unwrap().as_str(),
        "creo:model:sketch_feature#917"
    );
    assert_eq!(parameter.name, "d917_42_1");
    assert_eq!(repeated.name, "d917_42_2");
    assert_ne!(parameter.id, repeated.id);
    assert_eq!(parameter.expression, "1");
    assert_eq!(
        parameter.value,
        Some(cadmpeg_ir::features::ParameterValue::Angle(
            cadmpeg_ir::features::Angle(1.0)
        ))
    );
    assert!(relation.dependencies.is_empty());
    assert_eq!(relation.properties["external_dependencies"], "d42");
    assert_eq!(
        relation.value,
        Some(cadmpeg_ir::features::ParameterValue::Angle(
            cadmpeg_ir::features::Angle(1.0 + 1.0f64.to_radians())
        ))
    );
    let model_feature = result
        .ir()
        .model
        .features
        .iter()
        .find(|feature| feature.id.as_str() == "creo:model:feature#40")
        .expect("model feature");
    assert!(matches!(
        &model_feature.definition,
        cadmpeg_ir::features::FeatureDefinition::Extrude {
            profile: cadmpeg_ir::features::ProfileRef::Native(profile),
            extent: cadmpeg_ir::features::ExtrudeExtent::OneSided {
                side: cadmpeg_ir::features::ExtrudeSide {
                    termination: cadmpeg_ir::features::LinearTermination::Unresolved,
                    ..
                }
            },
            op: cadmpeg_ir::features::BooleanOp::Unresolved,
            ..
        } if profile == "creo:featdefs:sketch#917"
    ));
    assert_eq!(
        model_feature.source_properties["native_parameter.dimension_count"],
        "2"
    );
    let sketch_feature = result
        .ir()
        .model
        .features
        .iter()
        .find(|feature| feature.id.as_str() == "creo:model:sketch_feature#917")
        .expect("sketch feature");
    assert_eq!(
        sketch_feature.source_content,
        [
            cadmpeg_ir::features::FeatureSourceContent::Parameter(parameter.id.clone()),
            cadmpeg_ir::features::FeatureSourceContent::Parameter(repeated.id.clone()),
        ]
    );
    let validation = cadmpeg_ir::validate_neutral(result.ir(), result.report().losses.clone());
    assert!(validation.is_ok(), "{validation:#?}");
}

#[test]
fn decode_transfers_decoded_dimensions_from_an_incomplete_table() {
    let payload = b"feat_defs_917\0\xe0\x01feat_id\0\x28\xe0\x00gsec2d_ptr\0\
        dimtab_ptr\0\xf8\x03\xf7\x81\x02\xfb\xe2\
        \xe0\x01type\0\x0a\xe0\x01value\0\xe4\
        \xe0\x01direct\0\x01\xe0\x01aux_value\0\x0f\
        \xe0\x01ext_id\0\x2a\xf3\xf7\x81\x02\xe2\
        \x02\x46\x08\x00\x00\x00\x00\x00\x00\x00\x00\x18\x2b\xe0\x00relat_ptr\0"
        .to_vec();
    let data = build_prt(
        "c",
        &[
            ("FeatDefs", payload),
            ("MdlStatus", b"Extrude id 40\0".to_vec()),
        ],
    );
    let scan = container::scan_bytes(data.clone());
    let dimensions = scan.features.definitions[0]
        .dimensions
        .as_ref()
        .expect("dimension table");
    assert_eq!(dimensions.declared_count, 3);
    assert_eq!(dimensions.rows.len(), 2);
    let result = CreoCodec
        .decode(&mut Cursor::new(data), &DecodeOptions::default())
        .expect("decode incomplete dimension table");

    assert_eq!(result.ir().model.parameters.len(), 2);
    assert!(result
        .ir()
        .model
        .parameters
        .iter()
        .all(|parameter| parameter.owner.as_ref().unwrap().as_str()
            == "creo:model:sketch_feature#917"));
    let coverage = result.report();
    assert_eq!(
        coverage.coverage_count(crate::coverage::DECODED_FEATURE_DIMENSION_COUNT),
        2
    );
    assert_eq!(
        coverage.coverage_count(crate::coverage::TRANSFERRED_FEATURE_DIMENSION_PARAMETER_COUNT),
        2
    );
    assert_eq!(
        coverage.coverage_count(crate::coverage::RESOLVED_FEATURE_DIMENSION_VALUE_COUNT),
        2
    );
    assert_eq!(
        coverage.coverage_count(crate::coverage::UNRESOLVED_FEATURE_DIMENSION_VALUE_COUNT),
        0
    );
    assert_eq!(
        coverage.coverage_count(crate::coverage::DECODED_FEATURE_SOLVER_VARIABLE_COUNT),
        0
    );
    assert_eq!(
        coverage.coverage_count(crate::coverage::DECODED_FEATURE_DIMENSION_DRIVEN_VARIABLE_COUNT),
        0
    );
    assert_eq!(
        coverage.coverage_count(crate::coverage::DECODED_FEATURE_DIMENSION_DRIVEN_GUESS_COUNT),
        0
    );
    assert_eq!(
        coverage.coverage_count(crate::coverage::RESOLVED_FEATURE_DIMENSION_DRIVEN_VARIABLE_COUNT),
        0
    );
    assert_eq!(
        coverage.coverage_count(
            crate::coverage::RESOLVED_FEATURE_DIMENSION_DRIVEN_COORDINATE_VARIABLE_COUNT
        ),
        0
    );
    assert_eq!(
        coverage.coverage_count(
            crate::coverage::RESOLVED_FEATURE_DIMENSION_DRIVEN_OTHER_VARIABLE_COUNT
        ),
        0
    );
    assert_eq!(
        coverage
            .coverage_count(crate::coverage::UNRESOLVED_FEATURE_DIMENSION_DRIVEN_VARIABLE_COUNT),
        0
    );
}

#[test]
fn decode_reports_unresolved_dimension_driven_solver_variables() {
    let mut payload =
        b"feat_defs_40\0var_arr\0\xf8\x02\xf7\x01\xfb\xe2schema\xf1\xf7\x01\xe2".to_vec();
    payload.extend_from_slice(&[
        1, 7, 0xed, 0, 0, 0, 0, 0, 0, 0, 0, 0xed, 1, 2, 3, 4, 5, 6, 7, 8, 0, 1, 3, 0xe2,
    ]);
    payload.extend_from_slice(&[7, 8, 0xed, 0, 0, 0, 0, 0, 0, 0, 0, 0x0f, 0, 1, 4, 0xe2]);
    let data = build_prt("c", &[("FeatDefs", payload)]);

    let result = CreoCodec
        .decode(&mut Cursor::new(data), &DecodeOptions::default())
        .expect("decode dimension-driven variable");
    let coverage = result.report();

    assert_eq!(
        coverage.coverage_count(crate::coverage::DECODED_FEATURE_DIMENSION_DRIVEN_VARIABLE_COUNT),
        2
    );
    assert_eq!(
        coverage.coverage_count(
            crate::coverage::DECODED_FEATURE_DIMENSION_DRIVEN_COORDINATE_VARIABLE_COUNT
        ),
        1
    );
    assert_eq!(
        coverage
            .coverage_count(crate::coverage::DECODED_FEATURE_DIMENSION_DRIVEN_OTHER_VARIABLE_COUNT),
        1
    );
    assert_eq!(
        coverage.coverage_count(crate::coverage::DECODED_FEATURE_DIMENSION_DRIVEN_GUESS_COUNT),
        1
    );
    assert_eq!(
        coverage.coverage_count(crate::coverage::RESOLVED_FEATURE_DIMENSION_DRIVEN_VARIABLE_COUNT),
        0
    );
    assert_eq!(
        coverage.coverage_count(
            crate::coverage::RESOLVED_FEATURE_DIMENSION_DRIVEN_COORDINATE_VARIABLE_COUNT
        ),
        0
    );
    assert_eq!(
        coverage.coverage_count(
            crate::coverage::RESOLVED_FEATURE_DIMENSION_DRIVEN_OTHER_VARIABLE_COUNT
        ),
        0
    );
    assert_eq!(
        coverage
            .coverage_count(crate::coverage::UNRESOLVED_FEATURE_DIMENSION_DRIVEN_VARIABLE_COUNT),
        2
    );
    assert_eq!(
        coverage.coverage_count(
            crate::coverage::UNRESOLVED_FEATURE_DIMENSION_DRIVEN_COORDINATE_VARIABLE_COUNT
        ),
        1
    );
    assert_eq!(
        coverage.coverage_count(
            crate::coverage::UNRESOLVED_FEATURE_DIMENSION_DRIVEN_OTHER_VARIABLE_COUNT
        ),
        1
    );
    assert_eq!(
        coverage.coverage_count(crate::coverage::UNRESOLVED_FEATURE_DIMENSION_DRIVEN_GUESS_COUNT),
        1
    );
    assert!(result.report().losses.iter().any(|loss| {
        loss.code.category() == cadmpeg_ir::LossCategory::DesignIntent
            && loss.severity == cadmpeg_ir::Severity::Warning
            && loss.message.contains(
                "2 dimension-driven section solver variable(s) retain unresolved exact values: 1 \
                 coordinate variable(s) lack a complete dimension equation and 1 variable(s) \
                 have a non-coordinate family",
            )
    }));
    assert!(result.report().losses.iter().any(|loss| {
        loss.code.category() == cadmpeg_ir::LossCategory::DesignIntent
            && loss.severity == cadmpeg_ir::Severity::Warning
            && loss.message.contains(
                "1 section solver variable pre-solve estimate(s) use a dimension-driven sentinel",
            )
    }));
}

#[test]
fn decode_retains_bounded_unresolved_dimension_value_tokens() {
    let payload = b"feat_defs_917\0\xe0\x01feat_id\0\x28\xe0\x00gsec2d_ptr\0\
        dimtab_ptr\0\xf8\x03\xf7\x81\x02\xfb\xe2\
        \xe0\x01type\0\x01\xe0\x01value\0\xe4\
        \xe0\x01direct\0\x00\xe0\x01aux_value\0\x18\
        \xe0\x01ext_id\0\x2a\xf3\xf7\x81\x02\xe2\
        \x01\x00\x04\xa6\x00\x18\x2b\xf3\xf7\x81\x02\xe2\
        \x01\x01\x04\xfe\xf2\x00\x18\x2c\xe0\x00relat_ptr\0"
        .to_vec();
    let data = build_prt(
        "c",
        &[
            ("FeatDefs", payload),
            ("MdlStatus", b"Extrude id 40\0".to_vec()),
        ],
    );
    let result = CreoCodec
        .decode(&mut Cursor::new(data), &DecodeOptions::default())
        .expect("decode dimensions");

    let parameters = &result.ir().model.parameters;
    assert_eq!(parameters.len(), 3);
    assert_eq!(parameters[1].properties["value_state"], "unresolved");
    assert_eq!(
        parameters[1].properties["value_encoding"],
        "three_byte_placeholder"
    );
    assert_eq!(parameters[1].properties["value_token"], "0004a6");
    assert_eq!(
        parameters[2].properties["value_encoding"],
        "four_byte_placeholder"
    );
    assert_eq!(parameters[2].properties["value_token"], "0104fef2");

    let sketches = &result.ir().native.namespace("creo").unwrap().arenas["sketches"];
    let sketch_fields = sketches[0].fields();
    let dimensions = sketch_fields["dimensions"]
        .as_array()
        .expect("native dimensions");
    assert_eq!(dimensions[1]["unresolved_value_token"][0], 0);
    assert_eq!(dimensions[1]["unresolved_value_token"][1], 4);
    assert_eq!(dimensions[1]["unresolved_value_token"][2], 166);
    assert_eq!(dimensions[1]["value_body"][0], 0);
    assert_eq!(dimensions[1]["value_body"][1], 4);
    assert_eq!(dimensions[1]["value_body"][2], 166);
    assert_eq!(dimensions[1]["auxiliary_body"][0], 24);
    assert_eq!(dimensions[2]["unresolved_value_token"][0], 1);
    assert_eq!(dimensions[2]["unresolved_value_token"][1], 4);
    assert_eq!(dimensions[2]["unresolved_value_token"][2], 254);
    assert_eq!(dimensions[2]["unresolved_value_token"][3], 242);
    let coverage = result.report();
    assert_eq!(
        coverage.coverage_count(crate::coverage::DECODED_FEATURE_DIMENSION_COUNT),
        3
    );
    assert_eq!(
        coverage.coverage_count(crate::coverage::TRANSFERRED_FEATURE_DIMENSION_PARAMETER_COUNT),
        3
    );
    assert_eq!(
        coverage.coverage_count(crate::coverage::RESOLVED_FEATURE_DIMENSION_VALUE_COUNT),
        1
    );
    assert_eq!(
        coverage.coverage_count(crate::coverage::UNRESOLVED_FEATURE_DIMENSION_VALUE_COUNT),
        2
    );
    assert!(result.report().losses.iter().any(|loss| {
        loss.code == CreoLossCode::SectionDimensionValueUnresolved.kind()
            && loss.code.category() == cadmpeg_ir::LossCategory::DesignIntent
            && loss.severity == cadmpeg_ir::Severity::Warning
            && loss.message.contains(
                "2 section dimension(s) retain source-native value tokens because their exact \
                 scalar encodings remain unresolved",
            )
    }));
    let validation = cadmpeg_ir::validate_neutral(result.ir(), result.report().losses.clone());
    assert!(validation.is_ok(), "{validation:#?}");
}

#[test]
fn decode_retains_dimensions_from_repeated_feature_definition_ids() {
    let definition = b"feat_defs_917\0\xe0\x01feat_id\0\x28\xe0\x00gsec2d_ptr\0\
        dimtab_ptr\0\xf8\x01\xf7\x58\xfb\xe2\
        \xe0\x01type\0\x02\xe0\x01value\0\xe4\
        \xe0\x01direct\0\x00\xe0\x01aux_value\0\x0f\
        \xe0\x01ext_id\0\x2a\xe0\x00relat_ptr\0";
    let mut payload = definition.to_vec();
    payload.extend_from_slice(definition);
    let data = build_prt(
        "c",
        &[
            ("FeatDefs", payload),
            ("MdlStatus", b"Extrude id 40\0".to_vec()),
        ],
    );
    let scan = container::scan_bytes(data.clone());
    assert_eq!(scan.features.definitions.len(), 2);
    assert!(scan
        .features
        .definitions
        .iter()
        .all(|definition| definition.id == 917));

    let result = CreoCodec
        .decode(&mut Cursor::new(data), &DecodeOptions::default())
        .expect("decode");
    let namespace = result
        .ir()
        .native
        .namespace("creo")
        .expect("creo namespace");
    let definition_ids = namespace.arenas["feature_definitions"]
        .iter()
        .map(cadmpeg_ir::NativeRecord::id)
        .collect::<BTreeSet<_>>();
    let sketch_ids = namespace.arenas["sketches"]
        .iter()
        .map(cadmpeg_ir::NativeRecord::id)
        .collect::<BTreeSet<_>>();
    assert_eq!(definition_ids.len(), 2);
    assert_eq!(sketch_ids.len(), 2);
    assert!(definition_ids
        .iter()
        .all(|id| id.starts_with("creo:featdefs:feature_definition#offset:")));
    assert!(sketch_ids
        .iter()
        .all(|id| id.starts_with("creo:featdefs:sketch#offset:")));

    assert_eq!(result.ir().model.parameters.len(), 2);
    assert_ne!(
        result.ir().model.parameters[0].id,
        result.ir().model.parameters[1].id
    );
    assert_ne!(
        result.ir().model.parameters[0].native_ref,
        result.ir().model.parameters[1].native_ref
    );
    assert!(result.ir().model.parameters.iter().all(|parameter| {
        parameter.value
            == Some(cadmpeg_ir::features::ParameterValue::Length(
                cadmpeg_ir::features::Length(1.0),
            ))
    }));
    let validation = cadmpeg_ir::validate_neutral(result.ir(), result.report().losses.clone());
    assert!(validation.is_ok(), "{validation:#?}");
}

#[test]
fn decode_reports_missing_declared_constraint_table_rows() {
    let mut payload = b"feat_defs_40\0relat_ptr\0\xf4\x04\xf8\x05\xf7\x6a\xfb\xe2\
        \xe0\x01id\0\xe0\x01used\0\xe0\x01type\0\xf1\xf7\x6a\xe2\
        \x34\x00\x05\x01\xf6\xe4\x00\xe6\x0f\x10\x0f\xe4\x00\x00\x00\xe2\
        \x35\x01\x07\x29\x32\xf6\x00\xe6\x0f\x10\x0f\xe4\x01\x2a\x03\xe2"
        .to_vec();
    payload.extend_from_slice(
        b"skamp_ptr\0\xf3\xf8\x02\xf7\x6b\xfb\xe2\
          \xe0\x01id\0\x05\xe0\x01type\0\x02\xe0\x01flags\0\x03\
          \xe0\x01status\0\x04\xe0\x00items\0\xf8\x01\xf7\x6c\xfb\xe2\
          \xe0\x01ent_id\0\x2a\xe0\x01sense\0\x01\xf1\xf7\x6c\xe2\
          \xf3\xf7\x6b\xe2\
          triples_ptr\0\xf4\x04\xf8\x03\xf7\x6d\xfb\xe2\
          \xe0\x01rel_id\0\x07\xe0\x01eqn_id\0\x08\xe0\x01skamp_id\0\x05\
          \xf1\xf7\x6d\xe2\xf6\x09\x05\xe2",
    );
    let data = build_prt("c", &[("FeatDefs", payload)]);

    let result = CreoCodec
        .decode(&mut Cursor::new(data), &DecodeOptions::default())
        .expect("decode incomplete constraint tables");
    let coverage = result.report();

    assert_eq!(
        coverage.coverage_count(crate::coverage::DECODED_FEATURE_RELATION_COUNT),
        2
    );
    assert_eq!(
        coverage.coverage_count(crate::coverage::MISSING_FEATURE_RELATION_ROW_COUNT),
        1
    );
    assert_eq!(
        coverage.coverage_count(crate::coverage::DECODED_FEATURE_SKAMP_COUNT),
        1
    );
    assert_eq!(
        coverage.coverage_count(crate::coverage::MISSING_FEATURE_SKAMP_ROW_COUNT),
        1
    );
    assert_eq!(
        coverage.coverage_count(crate::coverage::DECODED_FEATURE_RELATION_TRIPLE_COUNT),
        2
    );
    assert_eq!(
        coverage.coverage_count(crate::coverage::MISSING_FEATURE_RELATION_TRIPLE_ROW_COUNT),
        1
    );
    for (code, message) in [
        (
            CreoLossCode::SectionRelationMissing,
            "1 declared section relation row(s) did not decode",
        ),
        (
            CreoLossCode::SectionIncidenceMissing,
            "1 declared section incidence row(s) did not decode",
        ),
        (
            CreoLossCode::SectionRelationJoinMissing,
            "1 declared section relation-incidence join row(s) did not decode",
        ),
    ] {
        assert!(result.report().losses.iter().any(|loss| {
            loss.code == code.kind()
                && loss.code.category() == cadmpeg_ir::LossCategory::DesignIntent
                && loss.severity == cadmpeg_ir::Severity::Warning
                && loss.message.contains(message)
        }));
    }
}

#[test]
fn decode_reports_malformed_relation_table_allocation_count() {
    let payload =
        b"feat_defs_40\0relat_ptr\0\xf4\x04\xf8\x00\xf7\x6a\xfb\xe2schema\xf1\xf7\x6a\xe2".to_vec();
    let data = build_prt("c", &[("FeatDefs", payload)]);

    let result = CreoCodec
        .decode(&mut Cursor::new(data), &DecodeOptions::default())
        .expect("decode malformed relation table");

    assert_eq!(
        result
            .report()
            .coverage_count(crate::coverage::MALFORMED_FEATURE_RELATION_TABLE_COUNT),
        1
    );
    assert!(result.report().losses.iter().any(|loss| {
        loss.code == CreoLossCode::SectionRelationTableMalformed.kind()
            && loss.code.category() == cadmpeg_ir::LossCategory::DesignIntent
            && loss.severity == cadmpeg_ir::Severity::Warning
            && loss
                .message
                .contains("use the invalid zero allocation count")
    }));
}

#[test]
fn decode_accepts_the_count_one_empty_relation_table() {
    let payload =
        b"feat_defs_40\0relat_ptr\0\xf4\x04\xf8\x01\xf7\x6a\xfb\xe2schema\xf1\xf7\x6a\xe2".to_vec();
    let data = build_prt("c", &[("FeatDefs", payload)]);

    let result = CreoCodec
        .decode(&mut Cursor::new(data), &DecodeOptions::default())
        .expect("decode empty relation table");

    assert_eq!(
        result
            .report()
            .coverage_count(crate::coverage::DECODED_FEATURE_RELATION_COUNT),
        0
    );
    assert_eq!(
        result
            .report()
            .coverage_count(crate::coverage::MISSING_FEATURE_RELATION_ROW_COUNT),
        0
    );
    assert_eq!(
        result
            .report()
            .coverage_count(crate::coverage::MALFORMED_FEATURE_RELATION_TABLE_COUNT),
        0
    );
    assert!(!result
        .report()
        .losses
        .iter()
        .any(|loss| loss.message.contains("section relation table")));
}

#[test]
fn decode_promotes_unnamed_depdb_recipe_into_feature_history() {
    let depdb = b"\xe3K\xc3\xb6rper ID 8051\0\xe3\
        \xf7\x50\x9f\x75\x83\x95\xf6\x9f\x73Profile 1\0\xf6\0protextrude\0"
        .to_vec();
    let data = build_prt("c", &[("DEPDB_DATA", depdb)]);
    let scan = container::scan_bytes(data.clone());
    assert_eq!(scan.features.operations.len(), 2);
    assert_eq!(scan.features.depdb_recipe_rows.len(), 1);
    assert_eq!(scan.features.depdb_recipe_rows[0].feature_id, 8053);
    let operation = scan
        .features
        .operations
        .iter()
        .find(|operation| operation.feature_id == 8053)
        .expect("recipe operation");

    let result = CreoCodec
        .decode(&mut Cursor::new(data), &DecodeOptions::default())
        .expect("decode");
    let feature = result
        .ir()
        .model
        .features
        .iter()
        .find(|feature| feature.id.as_str() == "creo:model:feature#8053")
        .expect("recipe feature");
    let rows = &result.ir().native.namespace("creo").unwrap().arenas["depdb_recipe_rows"];
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].fields()["owner_feature_id"], 8053);
    assert_eq!(rows[0].fields()["header"][0], 0);
    assert_eq!(
        rows[0].fields()["body"].as_array().map(Vec::len),
        Some(scan.features.depdb_recipe_rows[0].body.len())
    );
    assert_eq!(feature.name, None);
    assert_eq!(
        result
            .ir()
            .model
            .feature_parent(&feature.id)
            .map(cadmpeg_ir::features::FeatureId::as_str),
        Some("creo:model:feature#8051")
    );
    assert_eq!(
        feature
            .dependencies
            .iter()
            .map(cadmpeg_ir::features::FeatureId::as_str)
            .collect::<Vec<_>>(),
        ["creo:model:feature#8051"]
    );
    assert_eq!(feature.source_tag.as_deref(), Some("protextrude"));
    assert_eq!(
        feature.source_properties.get("recipe").map(String::as_str),
        Some("protextrude")
    );
    assert_annotation(
        &result.source_fidelity().annotations,
        "creo:model:feature#8053",
        "creo:DEPDB_DATA",
        operation.offset as u64,
        "feature_recipe",
        Exactness::ByteExact,
    );
}

#[test]
fn decode_retains_conflicting_recipe_candidates_without_projecting_one() {
    let depdb = b"\xf7\x50\x9f\x75\x83\x95\xf6\x9f\x73Profile 1\0\xf6\0protextrude\0\
        \xf7\x50\x9f\x75\x83\x95\xf6\x9f\x73Profile 2\0\xf6\0protrevolve\0"
        .to_vec();
    let data = build_prt("c", &[("DEPDB_DATA", depdb)]);
    let scan = container::scan_bytes(data.clone());

    assert_eq!(scan.features.operation_states.len(), 2);
    assert_eq!(scan.features.operations.len(), 1);
    assert_eq!(scan.features.operations[0].recipe, None);
    assert!(scan.features.operations[0].recipe_conflict);
    assert_eq!(scan.features.depdb_recipe_rows.len(), 2);
    assert!(scan
        .features
        .depdb_recipe_rows
        .iter()
        .all(|row| row.feature_id == 8053));
    assert_eq!(
        scan.features
            .depdb_recipe_rows
            .iter()
            .filter_map(|row| row.root_schema_class)
            .collect::<Vec<_>>(),
        [917, 917]
    );

    let result = CreoCodec
        .decode(&mut Cursor::new(data), &DecodeOptions::default())
        .expect("decode");
    let feature = result
        .ir()
        .model
        .features
        .iter()
        .find(|feature| feature.id.as_str() == "creo:model:feature#8053")
        .expect("native feature");
    let operation_states =
        &result.ir().native.namespace("creo").unwrap().arenas["feature_operation_states"];
    assert_eq!(operation_states.len(), 2);
    assert!(operation_states
        .iter()
        .all(|state| state.fields()["recipe_conflict"] == true));
    assert!(matches!(
        &feature.definition,
        cadmpeg_ir::features::FeatureDefinition::Native { kind, .. }
            if kind == "Native Feature"
    ));
    assert_eq!(
        feature
            .source_properties
            .get("featdefs_row_schema_classes")
            .map(String::as_str),
        Some("917")
    );
    assert!(!feature.source_properties.contains_key("recipe"));
    assert_eq!(feature.source_tag, None);
}

#[test]
fn decode_preserves_unowned_depdb_section_instances_with_unique_native_ids() {
    let depdb = b"feat_defs_917\0template\xe3S2D0004\0first\xe3S2D0004\0second".to_vec();
    let data = build_prt("c", &[("DEPDB_DATA", depdb)]);
    let scan = container::scan_bytes(data.clone());
    let positional = scan
        .features
        .definitions
        .iter()
        .filter(|definition| definition.body.starts_with(b"\xe3S2D"))
        .collect::<Vec<_>>();

    assert_eq!(positional.len(), 2);
    assert!(positional
        .iter()
        .all(|definition| definition.owner_feature_id.is_none()));
    let expected_positional_ids = positional
        .iter()
        .map(|definition| {
            format!(
                "creo:featdefs:feature_definition#offset:{}",
                definition.offset
            )
        })
        .collect::<BTreeSet<_>>();

    let result = CreoCodec
        .decode(&mut Cursor::new(data), &DecodeOptions::default())
        .expect("decode");
    let records = &result.ir().native.namespace("creo").unwrap().arenas["feature_definitions"];
    let positional_ids = records
        .iter()
        .filter(|record| expected_positional_ids.contains(record.id()))
        .map(|record| record.id().to_string())
        .collect::<BTreeSet<_>>();

    assert_eq!(positional_ids, expected_positional_ids);
    assert!(positional_ids
        .iter()
        .all(|id| id.starts_with("creo:featdefs:feature_definition#offset:")));
    assert!(result.ir().model.features.is_empty());
}
