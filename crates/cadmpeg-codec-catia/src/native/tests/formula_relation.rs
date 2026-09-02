// SPDX-License-Identifier: Apache-2.0
//! Native-namespace tests over synthetic CATPart streams.

#![allow(clippy::doc_markdown, clippy::unwrap_used)]

use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeOptions};

use crate::test_support::*;
use crate::CatiaCodec;

#[test]
fn native_namespace_types_and_validates_formula_relations() {
    let native =
        crate::native::CatiaNative::decode(&standard_catpart_with_formula_relation(0x63, false));
    let formula = native.entity_records[0]
        .formula_relation
        .as_ref()
        .expect("complete formula relation");
    assert_eq!(formula.expression_entity.payload_offset, 4);
    assert_eq!(formula.output_entity.payload_offset, 6);
    assert_eq!(formula.expression_entity.reference.entity_id, 2);
    assert_eq!(
        formula.expression_entity.reference.entity.as_deref(),
        Some(native.entity_records[1].id.as_str())
    );
    assert_eq!(
        formula.expression_entity.reference.class_name,
        native
            .object_graphs
            .iter()
            .flat_map(|graph| &graph.records)
            .find(|record| record.entity_id == Some(2))
            .and_then(|record| record.class_name.clone())
    );
    assert_eq!(formula.output_entity.reference.entity_id, 99);
    assert_eq!(formula.output_entity.reference.entity, None);
    let parameter_entity = &native.entity_records[2];
    assert_eq!(
        formula.parameter_dependencies,
        [crate::native::CatiaRelationParameterDependency {
            source_offset: 0,
            symbol: "#1_ /2".to_string(),
            candidates: vec![crate::native::CatiaEntityReference {
                entity_id: parameter_entity.entity_id,
                is_null: false,
                entity: Some(parameter_entity.id.clone()),
                class_name: native
                    .object_graphs
                    .iter()
                    .flat_map(|graph| &graph.records)
                    .find(|record| record.entity_id == Some(parameter_entity.entity_id))
                    .and_then(|record| record.class_name.clone()),
            }],
        }]
    );
    let expected_formula = formula.clone();

    let mut version_235_namespace = cadmpeg_ir::NativeNamespace::default();
    native
        .store(&mut version_235_namespace)
        .expect("store current formula output reference");
    let mut stored_fields = version_235_namespace
        .arenas
        .get_mut("entity_records")
        .expect("stored entity records")[0]
        .fields_mut();
    let formula_fields = stored_fields
        .get_mut("formula_relation")
        .expect("stored formula relation")
        .as_object_mut()
        .expect("stored formula-relation object");
    let expression = formula_fields
        .remove("expression_entity")
        .expect("stored expression entity");
    let expression = expression.as_object().expect("stored expression incidence")["reference"]
        .as_object()
        .expect("stored expression-entity object");
    formula_fields.insert("expression".to_string(), expression["entity"].clone());
    let output = formula_fields
        .remove("output_entity")
        .expect("stored output entity");
    let output = output.as_object().expect("stored output incidence")["reference"]
        .as_object()
        .expect("stored output-entity object");
    formula_fields.insert(
        "parameter_entity_id".to_string(),
        output["entity_id"].clone(),
    );
    formula_fields.insert(
        "parameter_is_null".to_string(),
        output.get("is_null").cloned().unwrap_or_default(),
    );
    formula_fields.insert(
        "parameter".to_string(),
        output.get("entity").cloned().unwrap_or_default(),
    );
    drop(stored_fields);

    version_235_namespace.set_version(
        std::num::NonZeroU32::new(crate::native::CATIA_FORMULA_OUTPUT_REFERENCE_VERSION - 1)
            .unwrap(),
    );
    let migrated = crate::native::CatiaNative::load(&version_235_namespace)
        .expect("migrate formula output reference");
    assert_eq!(
        migrated.entity_records[0].formula_relation,
        Some(expected_formula.clone())
    );

    let mut version_236_namespace = cadmpeg_ir::NativeNamespace::default();
    native
        .store(&mut version_236_namespace)
        .expect("store current formula expression reference");
    let mut stored_fields = version_236_namespace
        .arenas
        .get_mut("entity_records")
        .expect("stored entity records")[0]
        .fields_mut();
    let formula_fields = stored_fields
        .get_mut("formula_relation")
        .expect("stored formula relation")
        .as_object_mut()
        .expect("stored formula-relation object");
    let expression = formula_fields
        .remove("expression_entity")
        .expect("stored expression entity");
    formula_fields.insert(
        "expression".to_string(),
        expression.as_object().expect("stored expression incidence")["reference"]
            .as_object()
            .expect("stored expression-entity object")["entity"]
            .clone(),
    );
    drop(stored_fields);

    version_236_namespace.set_version(
        std::num::NonZeroU32::new(crate::native::CATIA_FORMULA_EXPRESSION_REFERENCE_VERSION - 1)
            .unwrap(),
    );
    let migrated = crate::native::CatiaNative::load(&version_236_namespace)
        .expect("migrate formula expression reference");
    assert_eq!(
        migrated.entity_records[0].formula_relation,
        Some(expected_formula.clone())
    );

    let mut version_249_namespace = cadmpeg_ir::NativeNamespace::default();
    native
        .store(&mut version_249_namespace)
        .expect("store current formula reference offsets");
    let mut stored_fields = version_249_namespace
        .arenas
        .get_mut("entity_records")
        .expect("stored entity records")[0]
        .fields_mut();
    let formula_fields = stored_fields
        .get_mut("formula_relation")
        .expect("stored formula relation")
        .as_object_mut()
        .expect("stored formula-relation object");
    for field in ["expression_entity", "output_entity"] {
        let reference = formula_fields[field]
            .as_object()
            .expect("stored formula incidence")["reference"]
            .clone();
        formula_fields.insert(field.to_string(), reference);
    }
    drop(stored_fields);

    version_249_namespace.set_version(
        std::num::NonZeroU32::new(crate::native::CATIA_FORMULA_REFERENCE_OFFSET_VERSION - 1)
            .unwrap(),
    );
    let migrated = crate::native::CatiaNative::load(&version_249_namespace)
        .expect("migrate formula reference offsets");
    assert_eq!(
        migrated.entity_records[0].formula_relation,
        Some(expected_formula.clone())
    );

    let mut version_237_namespace = cadmpeg_ir::NativeNamespace::default();
    native
        .store(&mut version_237_namespace)
        .expect("store current formula dependency references");
    let mut stored_fields = version_237_namespace
        .arenas
        .get_mut("entity_records")
        .expect("stored entity records")[0]
        .fields_mut();
    let candidates = stored_fields
        .get_mut("formula_relation")
        .expect("stored formula relation")
        .as_object_mut()
        .expect("stored formula-relation object")
        .get_mut("parameter_dependencies")
        .expect("stored parameter dependencies")
        .as_array_mut()
        .expect("stored parameter-dependency array")[0]
        .as_object_mut()
        .expect("stored parameter dependency")
        .get_mut("candidates")
        .expect("stored dependency candidates")
        .as_array_mut()
        .expect("stored candidate array");
    for candidate in candidates {
        *candidate = candidate.as_object().expect("stored candidate reference")["entity"].clone();
    }
    drop(stored_fields);

    version_237_namespace.set_version(
        std::num::NonZeroU32::new(crate::native::CATIA_FORMULA_DEPENDENCY_REFERENCE_VERSION - 1)
            .unwrap(),
    );
    let migrated = crate::native::CatiaNative::load(&version_237_namespace)
        .expect("migrate formula dependency references");
    assert_eq!(
        migrated.entity_records[0].formula_relation,
        Some(expected_formula.clone())
    );

    let mut version_245_namespace = cadmpeg_ir::NativeNamespace::default();
    native
        .store(&mut version_245_namespace)
        .expect("store current formula dependency offsets");
    version_245_namespace
        .arenas
        .get_mut("entity_records")
        .expect("stored entity records")[0]
        .fields()
        .get_mut("formula_relation")
        .expect("stored formula relation")
        .as_object_mut()
        .expect("stored formula-relation object")
        .get_mut("parameter_dependencies")
        .expect("stored parameter dependencies")
        .as_array_mut()
        .expect("stored parameter-dependency array")[0]
        .as_object_mut()
        .expect("stored parameter dependency")
        .remove("source_offset");
    version_245_namespace.set_version(
        std::num::NonZeroU32::new(crate::native::CATIA_RELATION_DEPENDENCY_OFFSET_VERSION - 1)
            .unwrap(),
    );
    let migrated = crate::native::CatiaNative::load(&version_245_namespace)
        .expect("migrate formula dependency offsets");
    assert_eq!(
        migrated.entity_records[0].formula_relation,
        Some(expected_formula.clone())
    );

    let mut version_205_namespace = cadmpeg_ir::NativeNamespace::default();
    native
        .store(&mut version_205_namespace)
        .expect("store current formula dependency candidates");
    let mut version_205_entities: Vec<crate::native::CatiaEntityRecord> = version_205_namespace
        .arena_as("entity_records")
        .expect("load version 205 entity records");
    version_205_entities[0]
        .formula_relation
        .as_mut()
        .expect("complete formula relation")
        .parameter_dependencies[0]
        .candidates
        .clear();
    version_205_namespace
        .set_arena("entity_records", &version_205_entities)
        .expect("store version 205 entity records");
    version_205_namespace.set_version(std::num::NonZeroU32::new(205).unwrap());
    let migrated = crate::native::CatiaNative::load(&version_205_namespace)
        .expect("migrate version 205 formula dependency candidates");
    assert_eq!(
        migrated.entity_records[0].formula_relation,
        Some(expected_formula)
    );

    let mut malformed = native;
    malformed.entity_records[0]
        .formula_relation
        .as_mut()
        .expect("complete formula relation")
        .output_entity
        .reference
        .entity_id = 98;
    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    malformed
        .store(&mut namespace)
        .expect("store malformed formula relation");
    assert!(matches!(
        crate::native::CatiaNative::load(&namespace),
        Err(cadmpeg_ir::NativeConvertError::InvalidOwner(_))
    ));

    let mut malformed_offset =
        crate::native::CatiaNative::decode(&standard_catpart_with_formula_relation(0x63, false));
    malformed_offset.entity_records[0]
        .formula_relation
        .as_mut()
        .expect("complete formula relation")
        .expression_entity
        .payload_offset = u64::MAX;
    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    malformed_offset
        .store(&mut namespace)
        .expect("store malformed formula incidence offset");
    assert!(matches!(
        crate::native::CatiaNative::load(&namespace),
        Err(cadmpeg_ir::NativeConvertError::InvalidOwner(_))
    ));
}

#[test]
fn formula_relation_requires_a_complete_relation_expression_target() {
    let mut file = standard_catpart_with_formula_relation(0x63, false);
    let role = file
        .windows("param".len())
        .position(|bytes| bytes == b"param")
        .expect("formula parameter role");
    file[role..role + "param".len()].copy_from_slice(b"other");

    let native = crate::native::CatiaNative::decode(&file);
    assert!(native.entity_records[0].formula_relation.is_none());
}

#[test]
fn formula_parameter_dependency_requires_a_unique_binding() {
    let native =
        crate::native::CatiaNative::decode(&standard_catpart_with_formula_relation(0x63, true));
    let dependency = &native.entity_records[0]
        .formula_relation
        .as_ref()
        .expect("complete formula relation")
        .parameter_dependencies[0];

    assert_eq!(dependency.symbol, "#1_ /2");
    assert_eq!(dependency.candidates.len(), 2);
}

#[test]
fn formula_parameter_dependency_retains_an_unmatched_symbol() {
    let native = crate::native::CatiaNative::decode(&standard_catpart_with_typed_formula_inputs(
        4,
        false,
        &[("#1_", "LENGTH", "Thickness", "#2_ /2", 35.0)],
        "LENGTH",
        Some(33.0),
        "µ+#1_ /2-2mm",
    ));
    let dependency = &native.entity_records[0]
        .formula_relation
        .as_ref()
        .expect("complete formula relation")
        .parameter_dependencies[0];

    assert_eq!(dependency.symbol, "#1_ /2");
    assert_eq!(dependency.source_offset, 3);
    assert!(dependency.candidates.is_empty());
}

#[test]
fn formula_parameter_dependencies_exclude_string_literal_contents() {
    let native = crate::native::CatiaNative::decode(&standard_catpart_with_typed_formula_inputs(
        4,
        false,
        &[("#1_", "Integer", "Count", "#1_ /2", 35.0)],
        "String",
        None,
        "\"literal #1_ /2\"+ToString(#1_ /2)",
    ));
    let dependencies = &native.entity_records[0]
        .formula_relation
        .as_ref()
        .expect("complete formula relation")
        .parameter_dependencies;

    assert_eq!(dependencies.len(), 1);
    assert_eq!(dependencies[0].symbol, "#1_ /2");
    assert_eq!(dependencies[0].source_offset, 26);
    assert_eq!(dependencies[0].candidates.len(), 1);

    let expected_formula = native.entity_records[0]
        .formula_relation
        .clone()
        .expect("complete formula relation");
    let mut old_namespace = cadmpeg_ir::NativeNamespace::default();
    native
        .store(&mut old_namespace)
        .expect("store relation dependencies");
    let mut stored_fields = old_namespace
        .arenas
        .get_mut("entity_records")
        .expect("stored entity records")[0]
        .fields_mut();
    let dependencies = stored_fields
        .get_mut("formula_relation")
        .expect("stored formula relation")
        .as_object_mut()
        .expect("stored formula relation")
        .get_mut("parameter_dependencies")
        .expect("stored parameter dependencies")
        .as_array_mut()
        .expect("stored parameter dependencies");
    let mut literal_dependency = dependencies[0].clone();
    literal_dependency
        .as_object_mut()
        .expect("stored parameter dependency")
        .insert("source_offset".to_string(), 9_u64.into());
    dependencies.insert(0, literal_dependency);
    drop(stored_fields);

    old_namespace.set_version(
        std::num::NonZeroU32::new(
            crate::native::CATIA_RELATION_STRING_LITERAL_DEPENDENCY_VERSION - 1,
        )
        .unwrap(),
    );
    let migrated = crate::native::CatiaNative::load(&old_namespace)
        .expect("migrate string-literal relation dependencies");
    assert_eq!(
        migrated.entity_records[0].formula_relation,
        Some(expected_formula)
    );

    let unterminated =
        crate::native::CatiaNative::decode(&standard_catpart_with_typed_formula_inputs(
            4,
            false,
            &[("#1_", "Integer", "Count", "#1_ /2", 35.0)],
            "String",
            None,
            "\"unterminated #1_ /2",
        ));
    assert!(unterminated.entity_records[0]
        .formula_relation
        .as_ref()
        .expect("complete formula relation")
        .parameter_dependencies
        .is_empty());
}

#[test]
fn formula_relation_resolves_bare_expression_symbols() {
    let native = crate::native::CatiaNative::decode(&standard_catpart_with_typed_formula_inputs(
        4,
        false,
        &[("#1_", "LENGTH", "Thickness", "#1_", 35.0)],
        "LENGTH",
        Some(33.0),
        "#1_-2mm",
    ));

    assert_eq!(
        native.entity_records[0]
            .formula_relation
            .as_ref()
            .expect("complete formula relation")
            .parameter_dependencies,
        [crate::native::CatiaRelationParameterDependency {
            source_offset: 0,
            symbol: "#1_".to_string(),
            candidates: vec![crate::native::CatiaEntityReference {
                entity_id: native.entity_records[2].entity_id,
                is_null: false,
                entity: Some(native.entity_records[2].id.clone()),
                class_name: native.object_graphs[0]
                    .records
                    .iter()
                    .find(|record| record.entity_id == Some(native.entity_records[2].entity_id))
                    .and_then(|record| record.class_name.clone()),
            }],
        }]
    );
}

#[test]
fn terminal_entity_identity_is_a_null_formula_output() {
    let bytes = standard_catpart_with_formula_relation(5, false);
    let native = crate::native::CatiaNative::decode(&bytes);
    let formula = native.entity_records[0]
        .formula_relation
        .as_ref()
        .expect("complete formula relation");
    assert_eq!(formula.output_entity.reference.entity_id, 5);
    assert!(formula.output_entity.reference.is_null);
    assert_eq!(formula.output_entity.reference.entity, None);
    let formula_record = native.object_graphs[0]
        .records
        .iter()
        .find(|record| record.id == native.entity_records[0].object_record)
        .expect("formula object record");
    assert!(formula_record.references[2].is_null);
    assert_eq!(formula_record.references[2].target, None);

    let mut version_210_namespace = cadmpeg_ir::NativeNamespace::default();
    native
        .store(&mut version_210_namespace)
        .expect("store terminal null references");
    let mut version_210_records: Vec<crate::native::CatiaObjectRecord> = version_210_namespace
        .arena_as("object_graph_records")
        .expect("load version 210 object records");
    for record in &mut version_210_records {
        for reference in &mut record.references {
            reference.is_null = false;
        }
    }
    version_210_namespace
        .set_arena("object_graph_records", &version_210_records)
        .expect("store version 210 object records");
    let mut version_210_entities: Vec<crate::native::CatiaEntityRecord> = version_210_namespace
        .arena_as("entity_records")
        .expect("load version 210 entity records");
    version_210_entities[0]
        .formula_relation
        .as_mut()
        .expect("complete formula relation")
        .output_entity
        .reference
        .is_null = false;
    version_210_namespace
        .set_arena("entity_records", &version_210_entities)
        .expect("store version 210 entity records");
    version_210_namespace.set_version(std::num::NonZeroU32::new(210).unwrap());
    let migrated = crate::native::CatiaNative::load(&version_210_namespace)
        .expect("migrate terminal null references");
    assert!(migrated.object_graphs[0].records[0].references[2].is_null);
    assert!(
        migrated.entity_records[0]
            .formula_relation
            .as_ref()
            .expect("migrated formula relation")
            .output_entity
            .reference
            .is_null
    );

    let decoded = CatiaCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("decode formula with null output");
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_NULL_FORMULA_OUTPUT_COUNT),
        1
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_CLASSIFIED_FORMULA_OUTPUT_ENTITY_COUNT),
        0
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::UNCLASSIFIED_FORMULA_OUTPUT_ENTITY_COUNT),
        1
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::UNRESOLVED_FORMULA_OUTPUT_COUNT),
        0
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_NULL_OBJECT_RECORD_REFERENCE_COUNT),
        1
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::UNRESOLVED_OBJECT_RECORD_REFERENCE_COUNT),
        0
    );
}
