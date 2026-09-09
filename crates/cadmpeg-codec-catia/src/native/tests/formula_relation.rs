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
        .formula_relation()
        .expect("complete formula relation");
    assert_eq!(formula.expression_entity.payload_offset, 4);
    assert_eq!(formula.output_entity.payload_offset, 6);
    assert_eq!(formula.expression_entity.reference.entity_id(), 2);
    assert_eq!(
        formula.expression_entity.reference.entity(),
        Some(native.entity_records[1].id.as_str())
    );
    assert_eq!(
        formula
            .expression_entity
            .reference
            .class_name()
            .map(str::to_owned),
        native
            .object_graphs
            .iter()
            .flat_map(|graph| &graph.records)
            .find(|record| record.entity_id() == Some(2))
            .and_then(|record| record.class_name().map(str::to_owned))
    );
    assert_eq!(formula.output_entity.reference.entity_id(), 99);
    assert_eq!(formula.output_entity.reference.entity(), None);
    let parameter_entity = &native.entity_records[2];
    assert_eq!(
        formula.parameter_dependencies,
        [crate::native::CatiaRelationParameterDependency {
            source_offset: 0,
            symbol: "#1_ /2".to_string(),
            candidates: vec![crate::native::CatiaEntityReference::resolved_or_unresolved(
                parameter_entity.entity_id,
                Some(parameter_entity.id.clone()),
                native
                    .object_graphs
                    .iter()
                    .flat_map(|graph| &graph.records)
                    .find(|record| record.entity_id() == Some(parameter_entity.entity_id))
                    .and_then(|record| record.class_name().map(str::to_owned)),
            )],
        }]
    );

    let mut malformed = native;
    let malformed_output = malformed.entity_records[0]
        .formula_relation()
        .expect("complete formula relation")
        .output_entity
        .reference
        .clone()
        .with_entity_id(98);
    malformed.entity_records[0]
        .formula_relation_mut()
        .expect("complete formula relation")
        .output_entity
        .reference = malformed_output;
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
        .formula_relation_mut()
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
    assert!(native.entity_records[0].formula_relation().is_none());
}

#[test]
fn formula_parameter_dependency_requires_a_unique_binding() {
    let native =
        crate::native::CatiaNative::decode(&standard_catpart_with_formula_relation(0x63, true));
    let dependency = &native.entity_records[0]
        .formula_relation()
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
        .formula_relation()
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
        .formula_relation()
        .expect("complete formula relation")
        .parameter_dependencies;

    assert_eq!(dependencies.len(), 1);
    assert_eq!(dependencies[0].symbol, "#1_ /2");
    assert_eq!(dependencies[0].source_offset, 26);
    assert_eq!(dependencies[0].candidates.len(), 1);

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
        .formula_relation()
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
            .formula_relation()
            .expect("complete formula relation")
            .parameter_dependencies,
        [crate::native::CatiaRelationParameterDependency {
            source_offset: 0,
            symbol: "#1_".to_string(),
            candidates: vec![crate::native::CatiaEntityReference::resolved_or_unresolved(
                native.entity_records[2].entity_id,
                Some(native.entity_records[2].id.clone()),
                native.object_graphs[0]
                    .records
                    .iter()
                    .find(|record| record.entity_id() == Some(native.entity_records[2].entity_id))
                    .and_then(|record| record.class_name().map(str::to_owned)),
            )],
        }]
    );
}

#[test]
fn terminal_entity_identity_is_a_null_formula_output() {
    let bytes = standard_catpart_with_formula_relation(5, false);
    let native = crate::native::CatiaNative::decode(&bytes);
    let formula = native.entity_records[0]
        .formula_relation()
        .expect("complete formula relation");
    assert_eq!(formula.output_entity.reference.entity_id(), 5);
    assert!(formula.output_entity.reference.is_null());
    assert_eq!(formula.output_entity.reference.entity(), None);
    let formula_record = native.object_graphs[0]
        .records
        .iter()
        .find(|record| record.id == native.entity_records[0].object_record)
        .expect("formula object record");
    assert!(formula_record.references[2].is_null());
    assert_eq!(formula_record.references[2].target(), None);

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
