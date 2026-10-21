// SPDX-License-Identifier: Apache-2.0
//! Canonical Sketch parameters drive source-less output and retain their owner.

use std::io::Cursor;

use cadmpeg_ir::codec::write::{target::TargetRequest, EncodeInput, Encoder};
use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::features::{Expression, ParameterValue};
use cadmpeg_ir::scalar::Length;

use crate::F3dCodec;

#[test]
fn owned_sketch_parameter_synthesis_uses_the_canonical_cache_without_source_bytes() {
    let bytes = include_bytes!("../../../tests/golden/fixtures/design_sketch_constraints.f3d");
    let decoded = F3dCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    let mut ir: cadmpeg_ir::document::CadIr =
        serde_json::from_slice(&serde_json::to_vec(decoded.ir()).unwrap()).unwrap();
    let parameter = ir
        .model
        .parameters
        .iter()
        .find(|parameter| parameter.name == "d1")
        .unwrap();
    let parameter_id = parameter.id.clone();
    let owner_id = parameter.owner.clone().unwrap();
    let owner_name = ir
        .model
        .features
        .iter()
        .find(|feature| feature.id == owner_id)
        .unwrap()
        .name
        .clone();
    let value = ParameterValue::Length(Length::new(15.0).unwrap());
    ir.set_parameter_expression(&parameter_id, Expression::literal(&value), Some(value))
        .unwrap();
    let native = crate::test_support::native_test::f3d_native(&ir);
    assert_eq!(native.design_parameters.len(), 2);
    let recipe = native
        .design_dimension_recipe_records
        .iter()
        .find(|record| record.record_index == 1305)
        .unwrap();
    assert_eq!(recipe.companion_record_index, 1202);
    assert_eq!(recipe.program, [0, 0]);
    assert!(recipe.references.is_empty());
    assert!(
        recipe.prefix_bytes.data().is_none(),
        "the CADIR document has no source recipe prefix bytes"
    );
    assert!(native
        .design_parameters
        .iter()
        .all(|parameter| parameter.source_ordinal == 9));
    assert_eq!(
        native
            .design_parameters
            .iter()
            .find(|parameter| parameter.name() == "d1")
            .unwrap()
            .evaluated_value()
            .get(),
        1.0
    );
    let auxiliary = native
        .sketch_curve_identities
        .iter()
        .find(|curve| curve.record_index == 800)
        .unwrap();
    assert!(auxiliary.owner_reference.is_none());
    let auxiliary_id = auxiliary.id.clone();
    let plan = F3dCodec
        .plan(EncodeInput::new(&ir, None), TargetRequest::Inherit)
        .unwrap();
    assert!(plan
        .report()
        .losses
        .iter()
        .any(|loss| loss.code.local_code() == "writer.design-records-omitted"));
    assert!(plan
        .report()
        .losses
        .iter()
        .any(|loss| loss.code.local_code() == "evaluation.stale"));
    assert_eq!(
        plan.report().native_dispositions.record_disposition(
            "f3d",
            "sketch_curve_identities",
            &auxiliary_id
        ),
        cadmpeg_ir::codec::write::NativeDisposition::Dropped
    );
    assert!(plan
        .report()
        .losses
        .iter()
        .any(|loss| loss.message.contains(&auxiliary_id)));
    assert!(crate::test_support::native_test::f3d_native(&ir)
        .sketch_curve_identities
        .iter()
        .any(|curve| curve.id == auxiliary_id && curve.owner_reference.is_none()));
    let original_constraint = ir
        .model
        .sketch_constraints
        .iter()
        .find(|constraint| {
            serde_json::to_value(&constraint.definition).unwrap()["parameter"]
                == serde_json::json!(parameter_id)
        })
        .unwrap();
    let cadmpeg_ir::sketches::SketchConstraintDefinitionInput::Native { entities, .. } =
        original_constraint.definition.kind()
    else {
        panic!("the source fixture dimension has a Native definition")
    };
    assert_eq!(entities.len(), 6);
    let omitted_ids = entities
        .iter()
        .map(cadmpeg_ir::SketchEntityId::as_str)
        .collect::<Vec<_>>()
        .join(", ");
    let reduction = format!(
        "Sketch constraint {}: ordered entity links reduce from 6 to 0; omitted links [{omitted_ids}]",
        original_constraint.id
    );
    let notes = plan
        .report()
        .losses
        .iter()
        .filter(|loss| loss.message == reduction)
        .collect::<Vec<_>>();
    assert_eq!(
        notes.len(),
        1,
        "the exact ordered six-link reduction is named once"
    );
    assert_eq!(notes[0].code.local_code(), "writer.design-records-omitted");
    assert_eq!(notes[0].severity, cadmpeg_ir::report::Severity::Error);
    let cadmpeg_ir::codec::write::ArenaCoverage::Declared(coverage) = plan.report().coverage else {
        panic!("reduced constraint fields need explicit arena coverage")
    };
    assert_eq!(
        coverage.sketch_constraints,
        cadmpeg_ir::codec::write::ArenaDisposition::Reported
    );
    let expected_constraint = owned_dimension_definition(&ir, "d1");
    assert_eq!(expected_constraint["kind"], "native");
    assert_eq!(expected_constraint["entities"].as_array().unwrap().len(), 6);
    let mut output = Vec::new();
    plan.write_to(&mut output).unwrap();
    let written = F3dCodec
        .decode(&mut Cursor::new(output), &DecodeOptions::default())
        .unwrap();
    let findings = crate::test_support::with_decode_context(|ctx| {
        crate::validate::validate_native(ctx, written.ir()).unwrap()
    });
    assert!(findings.is_empty(), "native findings: {findings:?}");
    let validation = cadmpeg_ir::validate::validate_neutral(written.ir(), Vec::new()).unwrap();
    assert!(
        validation.is_ok(),
        "neutral findings: {:?}",
        validation.findings
    );
    assert_eq!(written.ir().model.features.len(), 1);
    assert_eq!(written.ir().model.sketches.len(), 1);
    assert_eq!(
        written.ir().model.sketch_entities.len(),
        ir.model.sketch_entities.len()
    );
    assert_eq!(written.ir().model.parameters.len(), 2);
    assert_eq!(written.ir().model.sketch_constraints.len(), 4);
    let mut reduced_constraint = expected_constraint.clone();
    reduced_constraint["entities"] = serde_json::json!([]);
    assert_eq!(owned_dimension_definition(written.ir(), "d1"), reduced_constraint,
        "only the six reported links reduce; kind, state, flags, properties, canonical parameter and the actual recipe operand remain exact");
    assert_eq!(
        owned_dimension_definition(&ir, "d1"),
        expected_constraint,
        "preparing the reduction does not change the input document"
    );
    for name in ["d1", "d2"] {
        let parameter = written
            .ir()
            .model
            .parameters
            .iter()
            .find(|parameter| parameter.name == name)
            .unwrap();
        assert_eq!(
            written
                .ir()
                .model
                .sketch_constraints
                .iter()
                .filter(|constraint| {
                    let definition = serde_json::to_value(&constraint.definition).unwrap();
                    definition["parameter"] == serde_json::json!(parameter.id)
                        || definition["parameter"]
                            == serde_json::json!({"kind":"parameter", "value":parameter.id})
                })
                .count(),
            1,
            "{name} retains precisely one canonical constraint binding"
        );
    }
    let owner = written
        .ir()
        .model
        .features
        .iter()
        .find(|feature| feature.name == owner_name)
        .expect("the Sketch owner survives synthesis");
    let parameter = written
        .ir()
        .model
        .parameters
        .iter()
        .find(|parameter| parameter.name == "d1" && parameter.owner.as_ref() == Some(&owner.id))
        .expect("the canonical parameter retains its owner");
    assert_eq!(
        parameter.value,
        Some(ParameterValue::Length(Length::new(15.0).unwrap()))
    );
    let native = crate::test_support::native_test::f3d_native(written.ir());
    let recipe = native
        .design_dimension_recipe_records
        .iter()
        .find(|record| record.record_index == 1305)
        .expect("the companion-owned typed recipe survives source-free synthesis");
    assert_eq!(recipe.companion_record_index, 1202);
    assert_eq!(recipe.program, [0, 0]);
    assert!(recipe.references.is_empty());
    assert_eq!(
        recipe.recipe_kind,
        crate::records::recipes::ConstructionRecipeKind::Body
    );
    assert!(!native
        .sketch_curve_identities
        .iter()
        .any(|curve| curve.record_index == 800));
    assert!(
        !native
            .design_entity_headers
            .iter()
            .any(|header| header.entity_id.suffix() == 800),
        "an omitted auxiliary curve cannot leave its own orphan entity header"
    );
    assert!(
        !native
            .design_types
            .iter()
            .any(|record| record.entities.values().any(|entity| *entity == 800)),
        "an omitted auxiliary curve must not leave a dangling type registration"
    );
    assert_eq!(native.design_parameters.len(), 2);
    assert!(native
        .design_parameters
        .iter()
        .all(|parameter| parameter.source_ordinal == 9));
    let record = native
        .design_parameters
        .iter()
        .find(|parameter| parameter.name() == "d1")
        .unwrap();
    assert_eq!(record.evaluated_value().get(), 1.5);
    let owner = native
        .design_parameter_owners
        .iter()
        .find(|owner| Some(owner.record_index()) == record.owner_record_index())
        .unwrap();
    assert_eq!(owner.evaluated_value().get(), 1.5);
    assert!(written
        .ir()
        .model
        .sketch_constraints
        .iter()
        .any(|constraint| {
            serde_json::to_value(&constraint.definition).unwrap()["parameter"]
                == serde_json::json!(parameter.id)
        }));
}

#[test]
fn an_unsolved_directional_dimension_is_omitted_with_its_exact_definition_reported() {
    let bytes = include_bytes!("../../../tests/golden/fixtures/design_sketch_constraints.f3d");
    let decoded = F3dCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    let mut ir: cadmpeg_ir::CadIr =
        serde_json::from_slice(&serde_json::to_vec(decoded.ir()).unwrap()).unwrap();
    let parameter = ir
        .model
        .parameters
        .iter()
        .find(|parameter| parameter.name == "d2")
        .unwrap();
    let parameter_id = parameter.id.clone();
    let owner_id = parameter.owner.clone().unwrap();
    let constraint = ir
        .model
        .sketch_constraints
        .iter()
        .find(|constraint| {
            let value = serde_json::to_value(&constraint.definition).unwrap();
            value["kind"] == "horizontal_distance"
                && value["parameter"]
                    == serde_json::json!({"kind":"parameter", "value":parameter_id})
        })
        .unwrap();
    let constraint_id = constraint.id.clone();
    let definition = serde_json::to_string(&constraint.definition).unwrap();
    let expected_loss = format!("Sketch constraint {constraint_id} is omitted: no exact regenerated encoding for canonical definition {definition}");
    let expected_d1 = owned_dimension_definition(&ir, "d1");
    let expected_relations =
        ["parallel", "concentric"].map(|kind| native_relation_content(&ir, kind));
    let original_entities = ir
        .model
        .sketch_entities
        .iter()
        .map(|entity| (entity.construction, entity.geometry.clone()))
        .collect::<Vec<_>>();
    let original_native = crate::test_support::native_test::f3d_native(&ir);
    assert_eq!(original_native.persistent_references.len(), 12);
    assert_eq!(original_native.lost_edge_references.len(), 1);
    let expected_references = original_native
        .persistent_references
        .iter()
        .map(|reference| (reference.kind, reference.value))
        .collect::<Vec<_>>();
    let expected_lost_edges = original_native
        .lost_edge_references
        .iter()
        .map(|reference| {
            (
                reference.class_tag.clone(),
                reference.record_index,
                reference.next_class_tag.clone(),
                reference.next_record_index,
            )
        })
        .collect::<Vec<_>>();
    let pair_id = original_native.design_dimension_locus_pairs[0].id.clone();
    let value = ParameterValue::Length(Length::new(15.0).unwrap());
    ir.set_parameter_expression(&parameter_id, Expression::literal(&value), Some(value))
        .unwrap();
    assert!(
        ir.model
            .features
            .iter()
            .find(|feature| feature.id == owner_id)
            .unwrap()
            .needs_recompute
    );
    let plan = F3dCodec
        .plan(EncodeInput::new(&ir, None), TargetRequest::Inherit)
        .unwrap();
    let losses = plan
        .report()
        .losses
        .iter()
        .filter(|loss| loss.message == expected_loss)
        .collect::<Vec<_>>();
    assert_eq!(losses.len(), 1);
    assert_eq!(losses[0].code.local_code(), "writer.design-records-omitted");
    assert_eq!(losses[0].severity, cadmpeg_ir::report::Severity::Error);
    let cadmpeg_ir::codec::write::ArenaCoverage::Declared(coverage) = plan.report().coverage else {
        panic!("per-constraint reduction needs declared coverage")
    };
    assert_eq!(
        coverage.sketch_constraints,
        cadmpeg_ir::codec::write::ArenaDisposition::Reported
    );
    assert_eq!(
        plan.report().native_dispositions.record_disposition(
            "f3d",
            "design_dimension_locus_pairs",
            &pair_id
        ),
        cadmpeg_ir::codec::write::NativeDisposition::Dropped
    );
    let mut output = Vec::new();
    let report = plan.write_to(&mut output).unwrap();
    let written = F3dCodec
        .decode(&mut Cursor::new(output), &DecodeOptions::default())
        .unwrap();
    assert_eq!(ir.model.sketch_constraints.len(), 4);
    assert_eq!(
        serde_json::to_string(
            &ir.model
                .sketch_constraints
                .iter()
                .find(|constraint| constraint.id == constraint_id)
                .unwrap()
                .definition
        )
        .unwrap(),
        definition
    );
    assert_eq!(
        written.ir().model.sketch_constraints.len(),
        3,
        "actual constraints: {:#?}; actual companions: {:#?}",
        written.ir().model.sketch_constraints,
        crate::test_support::native_test::f3d_native(written.ir()).design_parameter_companions,
    );
    assert_eq!(owned_dimension_definition(written.ir(), "d1"), expected_d1);
    assert_eq!(
        ["parallel", "concentric"].map(|kind| native_relation_content(written.ir(), kind)),
        expected_relations
    );
    assert_eq!(
        written
            .ir()
            .model
            .sketch_entities
            .iter()
            .map(|entity| (entity.construction, entity.geometry.clone()))
            .collect::<Vec<_>>(),
        original_entities
    );
    assert_eq!(written.ir().model.features.len(), 1);
    assert_eq!(written.ir().model.sketches.len(), 1);
    assert_eq!(written.ir().model.parameters.len(), 2);
    let parameter = written
        .ir()
        .model
        .parameters
        .iter()
        .find(|parameter| parameter.name == "d2")
        .unwrap();
    assert!(parameter.owner.is_some());
    assert_eq!(
        parameter.value,
        Some(ParameterValue::Length(Length::new(15.0).unwrap()))
    );
    assert!(written
        .ir()
        .model
        .sketch_constraints
        .iter()
        .all(|constraint| {
            !crate::design::dimensions::constraint_parameters(constraint.definition.kind())
                .any(|id| id == &parameter.id)
        }));
    let native = crate::test_support::native_test::f3d_native(written.ir());
    let parameter = native
        .design_parameters
        .iter()
        .find(|parameter| parameter.name() == "d2")
        .unwrap();
    assert_eq!(parameter.evaluated_value().get(), 1.5);
    let owner = native
        .design_parameter_owners
        .iter()
        .find(|owner| Some(owner.record_index()) == parameter.owner_record_index())
        .unwrap();
    assert_eq!(owner.evaluated_value().get(), 1.5);
    assert_eq!(
        native
            .persistent_references
            .iter()
            .map(|reference| (reference.kind, reference.value))
            .collect::<Vec<_>>(),
        expected_references
    );
    assert_eq!(
        native
            .lost_edge_references
            .iter()
            .map(|reference| (
                reference.class_tag.clone(),
                reference.record_index,
                reference.next_class_tag.clone(),
                reference.next_record_index,
            ))
            .collect::<Vec<_>>(),
        expected_lost_edges
    );
    let terminal_companion = native
        .design_parameter_companions
        .iter()
        .find(|companion| companion.record_index() == owner.companion_record_index())
        .unwrap();
    assert!(terminal_companion
        .payload()
        .unwrap()
        .owned_recipe_ids()
        .is_empty());
    let fidelity = written.source_fidelity();
    assert_eq!(
        fidelity.metadata[&format!("{}.payload_byte_length", terminal_companion.id())],
        "0"
    );
    let terminal_offset: u64 = fidelity.metadata
        [&format!("{}.byte_offset", terminal_companion.id())]
        .parse()
        .unwrap();
    for reference in &native.persistent_references {
        let offset: u64 = fidelity.metadata[&format!("{}.byte_offset", reference.id)]
            .parse()
            .unwrap();
        assert!(offset < terminal_offset);
    }
    for reference in &native.lost_edge_references {
        let offset: u64 = fidelity.metadata[&format!("{}.record_byte_offset", reference.id)]
            .parse()
            .unwrap();
        assert!(offset < terminal_offset);
    }

    assert_eq!(native.sketch_relations.len(), 2);
    assert_eq!(native.design_dimension_recipe_records.len(), 1);
    assert!(native.design_dimension_locus_pairs.is_empty());
    assert!(report
        .losses
        .iter()
        .any(|loss| loss.code.local_code() == "evaluation.stale"));
    let findings = crate::test_support::with_decode_context(|ctx| {
        crate::validate::validate_native(ctx, written.ir()).unwrap()
    });
    assert!(findings.is_empty(), "native findings: {findings:?}");
    let validation = cadmpeg_ir::validate::validate_neutral(written.ir(), Vec::new()).unwrap();
    assert!(
        validation.is_ok(),
        "neutral findings: {:?}",
        validation.findings
    );
}

#[test]
fn unchanged_owned_dimension_retains_all_six_entity_links_without_source_bytes() {
    let bytes = include_bytes!("../../../tests/golden/fixtures/design_sketch_constraints.f3d");
    let decoded = F3dCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    let ir: cadmpeg_ir::CadIr =
        serde_json::from_slice(&serde_json::to_vec(decoded.ir()).unwrap()).unwrap();
    let expected = owned_dimension_definition(&ir, "d1");
    assert_eq!(expected["entities"].as_array().unwrap().len(), 6);
    let expected_d2 = owned_dimension_definition(&ir, "d2");
    let native = crate::test_support::native_test::f3d_native(&ir);
    assert!(native
        .design_dimension_recipe_records
        .iter()
        .all(|recipe| recipe.prefix_bytes.data().is_none()));
    let plan = F3dCodec
        .plan(EncodeInput::new(&ir, None), TargetRequest::Inherit)
        .unwrap();
    assert!(!plan
        .report()
        .losses
        .iter()
        .any(|loss| loss.message.contains("ordered entity links reduce")));
    assert!(
        !plan.report().losses.iter().any(|loss| {
            ir.model.sketch_constraints.iter().any(|constraint| {
                loss.message
                    .starts_with(&format!("Sketch constraint {} is omitted:", constraint.id))
            })
        }),
        "all unchanged canonical constraints have an exact supported encoding"
    );
    let mut output = Vec::new();
    plan.write_to(&mut output).unwrap();
    let written = F3dCodec
        .decode(&mut Cursor::new(output), &DecodeOptions::default())
        .unwrap();
    let findings = crate::test_support::with_decode_context(|ctx| {
        crate::validate::validate_native(ctx, written.ir()).unwrap()
    });
    assert!(findings.is_empty(), "native findings: {findings:?}");
    let validation = cadmpeg_ir::validate::validate_neutral(written.ir(), Vec::new()).unwrap();
    assert!(
        validation.is_ok(),
        "neutral findings: {:?}",
        validation.findings
    );
    assert_eq!(written.ir().model.features.len(), 1);
    assert_eq!(written.ir().model.sketches.len(), 1);
    assert_eq!(written.ir().model.sketch_entities.len(), 8);
    assert_eq!(written.ir().model.sketch_constraints.len(), 4);
    assert_eq!(written.ir().model.parameters.len(), 2);
    assert_eq!(owned_dimension_definition(written.ir(), "d1"), expected,
        "unchanged canonical cache retains the complete kind, ordered six entities, parameter and actual recipe");
    assert_eq!(owned_dimension_definition(written.ir(), "d2"), expected_d2);
    for name in ["d1", "d2"] {
        let before = ir
            .model
            .parameters
            .iter()
            .find(|parameter| parameter.name == name)
            .unwrap();
        let after = written
            .ir()
            .model
            .parameters
            .iter()
            .find(|parameter| parameter.name == name)
            .unwrap();
        assert_eq!(after.value, before.value);
        assert_eq!(after.expression, before.expression);
    }
}

#[test]
fn a_changed_neutral_sketch_entity_is_not_replaced_by_the_native_graph() {
    let bytes = include_bytes!("../../../tests/golden/fixtures/design_sketch_constraints.f3d");
    let decoded = F3dCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    let mut ir = decoded.ir().clone();
    ir.model.sketch_entities[0].construction = true;
    let plan = F3dCodec
        .plan(EncodeInput::new(&ir, None), TargetRequest::Inherit)
        .unwrap();
    let cadmpeg_ir::codec::write::ArenaCoverage::Declared(coverage) = plan.report().coverage else {
        panic!("omitted Sketch content must have explicit arena dispositions")
    };
    assert_eq!(
        coverage.features,
        cadmpeg_ir::codec::write::ArenaDisposition::Dropped
    );
    assert_eq!(
        coverage.sketch_entities,
        cadmpeg_ir::codec::write::ArenaDisposition::Dropped
    );
    for arena in ["model.features", "model.sketch_entities"] {
        assert!(
            plan.report().losses.iter().any(|loss| {
                loss.code.local_code() == "writer.design-records-omitted"
                    && loss.message.contains(arena)
            }),
            "the {arena} omission is reported by name"
        );
    }
    let mut output = Vec::new();
    plan.write_to(&mut output).unwrap();
    let written = F3dCodec
        .decode(&mut Cursor::new(output), &DecodeOptions::default())
        .unwrap();
    assert!(written.ir().model.sketch_entities.is_empty());
    assert!(written.ir().model.features.is_empty());
}

#[test]
fn deleted_neutral_sketch_design_is_not_recreated_from_stale_native_records() {
    let bytes = include_bytes!("../../../tests/golden/fixtures/design_sketch_constraints.f3d");
    let decoded = F3dCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    let mut ir = decoded.ir().clone();
    ir.model.features.clear();
    ir.model.parameters.clear();
    ir.model.sketches.clear();
    ir.model.sketch_entities.clear();
    ir.model.sketch_constraints.clear();
    let plan = F3dCodec
        .plan(EncodeInput::new(&ir, None), TargetRequest::Inherit)
        .unwrap();
    for arena in [
        "design_parameter_scopes",
        "design_parameter_owners",
        "design_parameters",
        "design_sketch_placements",
    ] {
        assert_eq!(
            plan.report().native_dispositions.disposition("f3d", arena),
            cadmpeg_ir::codec::write::NativeDisposition::Dropped
        );
    }
    let mut output = Vec::new();
    plan.write_to(&mut output).unwrap();
    let written = F3dCodec
        .decode(&mut Cursor::new(output), &DecodeOptions::default())
        .unwrap();
    assert!(written.ir().model.features.is_empty());
    assert!(written.ir().model.parameters.is_empty());
    assert!(written.ir().model.sketches.is_empty());
    assert!(written.ir().model.sketch_entities.is_empty());
    assert!(written.ir().model.sketch_constraints.is_empty());
    let native = crate::test_support::native_test::f3d_native(written.ir());
    assert!(native.design_parameter_scopes.is_empty());
    assert!(native.design_parameter_owners.is_empty());
    assert!(native.design_parameters.is_empty());
    assert!(native.design_sketch_placements.is_empty());
}

#[test]
fn canonical_owned_parameter_order_updates_its_writable_owner_lane() {
    let bytes = include_bytes!("../../../tests/golden/fixtures/design_sketch_constraints.f3d");
    let decoded = F3dCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    let mut ir = decoded.ir().clone();
    for parameter in &mut ir.model.parameters {
        parameter.ordinal = match parameter.name.as_str() {
            "d1" => 1,
            "d2" => 0,
            _ => panic!("the fixture has precisely its two stated dimension parameters"),
        };
    }
    let mut output = Vec::new();
    F3dCodec
        .plan(EncodeInput::new(&ir, None), TargetRequest::Inherit)
        .and_then(|plan| plan.write_to(&mut output))
        .unwrap();
    let written = F3dCodec
        .decode(&mut Cursor::new(output), &DecodeOptions::default())
        .unwrap();
    let native = crate::test_support::native_test::f3d_native(written.ir());
    for (name, expected) in [("d1", 1), ("d2", 0)] {
        assert_eq!(
            written
                .ir()
                .model
                .parameters
                .iter()
                .find(|parameter| parameter.name == name)
                .unwrap()
                .ordinal,
            expected
        );
        let record = native
            .design_parameters
            .iter()
            .find(|parameter| parameter.name() == name)
            .unwrap();
        assert_eq!(record.source_ordinal, 9);
        let owner = native
            .design_parameter_owners
            .iter()
            .find(|owner| Some(owner.record_index()) == record.owner_record_index())
            .unwrap();
        assert_eq!(owner.local_ordinal(), Some(expected));
        assert_eq!(owner.owned_ordinal(), 9);
    }
}

/// Compare content through proved typed targets; source offsets in native IDs
/// may change while the graph's record identities and geometry remain exact.
fn owned_dimension_definition(ir: &cadmpeg_ir::CadIr, name: &str) -> serde_json::Value {
    let parameter = ir
        .model
        .parameters
        .iter()
        .find(|parameter| parameter.name == name)
        .unwrap();
    let mut definitions = ir.model.sketch_constraints.iter().filter_map(|constraint| {
        let value = serde_json::to_value(&constraint.definition).unwrap();
        (value["parameter"] == serde_json::json!(parameter.id)
            || value["parameter"] == serde_json::json!({"kind":"parameter", "value":parameter.id}))
        .then_some(value)
    });
    let mut definition = definitions.next().expect("one owned dimension");
    assert!(
        definitions.next().is_none(),
        "the parameter binds one dimension"
    );
    definition["parameter"] = serde_json::json!(name);
    let native = crate::test_support::native_test::f3d_native(ir);
    if let Some(entities) = definition
        .get_mut("entities")
        .and_then(serde_json::Value::as_array_mut)
    {
        for identity in entities {
            let entity = ir
                .model
                .sketch_entities
                .iter()
                .find(|entity| identity.as_str() == Some(entity.id().as_str()))
                .expect("every dimension entity exists");
            let native_ref = entity
                .native_ref
                .as_deref()
                .expect("the source fixture entity has a typed native carrier");
            let carrier = native
                .sketch_points
                .iter()
                .find(|point| point.id == native_ref)
                .map(|point| ("point", point.record_index))
                .or_else(|| {
                    native
                        .sketch_curve_identities
                        .iter()
                        .find(|curve| curve.id == native_ref)
                        .map(|curve| ("curve", curve.record_index))
                })
                .expect("the native carrier is a real point or curve");
            *identity = serde_json::json!({"carrier_kind":carrier.0,"record_index":carrier.1,"construction":entity.construction,"geometry":entity.geometry});
        }
    }
    if let Some(operands) = definition
        .get_mut("operands")
        .and_then(serde_json::Value::as_array_mut)
    {
        for operand in operands {
            let native_ref = operand["native_ref"]
                .as_str()
                .expect("the recipe operand has a typed target");
            let record = native
                .design_dimension_recipe_records
                .iter()
                .find(|record| record.id == native_ref)
                .expect("the operand's typed recipe exists");
            assert_eq!(operand["object_index"], record.record_index);
            operand.as_object_mut().unwrap().remove("native_ref");
            operand["recipe_kind"] = serde_json::to_value(record.recipe_kind).unwrap();
            operand["recipe_program"] = serde_json::json!(record.program);
            operand["recipe_references"] = serde_json::to_value(&record.references).unwrap();
        }
    }
    definition
}

/// Resolve relation identities through their actual typed point carriers before
/// comparing all remaining fields, including ordered and repeated operands.
fn native_relation_content(ir: &cadmpeg_ir::CadIr, kind: &str) -> serde_json::Value {
    let native = crate::test_support::native_test::f3d_native(ir);
    let mut matches = ir.model.sketch_constraints.iter().filter(|constraint| {
        serde_json::to_value(&constraint.definition).unwrap()["native_kind"] == kind
    });
    let constraint = matches.next().unwrap();
    assert!(matches.next().is_none());
    let relation = native
        .sketch_relations
        .iter()
        .find(|relation| Some(&relation.id) == constraint.native_ref.as_ref())
        .unwrap();
    let mut value = serde_json::to_value(constraint).unwrap();
    for field in ["id", "sketch", "native_ref"] {
        value.as_object_mut().unwrap().remove(field);
    }
    value["relation_record_index"] = serde_json::json!(relation.record_index);
    for identity in value["definition"]["entities"].as_array_mut().unwrap() {
        let entity = ir
            .model
            .sketch_entities
            .iter()
            .find(|entity| identity.as_str() == Some(entity.id().as_str()))
            .unwrap();
        let point = native
            .sketch_points
            .iter()
            .find(|point| Some(&point.id) == entity.native_ref.as_ref())
            .unwrap();
        *identity = serde_json::json!({"point_record_index":point.record_index, "construction":entity.construction, "geometry":entity.geometry});
    }
    value
}
