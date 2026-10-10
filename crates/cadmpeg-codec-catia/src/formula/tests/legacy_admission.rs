// SPDX-License-Identifier: Apache-2.0
//! Entity admission for the three legacy parameter packet forms.

use cadmpeg_ir::document::CadIr;

pub(super) fn legacy_values() -> crate::native::CatiaNative {
    let mut bytes = Vec::new();
    for entity_id in [1_u32, 4, 9, 12, 13] {
        bytes.push(0xea);
        bytes.extend(entity_id.to_le_bytes());
        bytes.extend([0x81, 0xfd, 0x8c]);
        if entity_id == 4 {
            for (role, selector, value) in [
                ("body", vec![0x80, 4, 0, 0, 0], "#1_ + 2"),
                ("param", vec![0xd1, 8], "(#1_ : #In Real) : Real\n"),
            ] {
                bytes.push(u8::try_from(role.len() + 1).expect("short role"));
                bytes.extend(role.as_bytes());
                bytes.extend(selector);
                bytes.extend(b"\xe8\x00\x12\x01");
                bytes.push(u8::try_from(value.len() + 1).expect("short text"));
                bytes.extend(value.as_bytes());
                bytes.push(0xfe);
            }
        } else if entity_id == 9 {
            bytes.extend([8, b'p', b'a', b'r', b'a', b'm', b'i', b'n', 0x80]);
            bytes.extend(4134_u32.to_le_bytes());
            bytes.extend([0xe8, 0xe4, 0x0b, 0x01]);
            bytes.extend(b"\xfe\x84\x92\x82\x05Real\x83");
            bytes.extend([5, b'n', b'a', b'm', b'e', 0xd1, 9]);
            bytes.extend(b"\xe8\x00\x12\x01\x07Result\xfe");
            bytes.extend(b"\xfe\x84\x88\x82\xfe\xe6");
            bytes.extend(3.5_f64.to_bits().to_le_bytes());
        } else if entity_id == 12 {
            bytes.extend([5, b'n', b'a', b'm', b'e', 0xd1, 11]);
            bytes.extend(b"\xe8\x00\x12\x01\x0cResponsible\xfe");
            bytes.extend(b"\xfe\x84\x92\x82\x07String\x83");
            bytes.extend(b"\xfe\x85\x93\x82\xfe\x0cCilas Evans");
        } else if entity_id == 13 {
            bytes.extend([5, b'n', b'a', b'm', b'e', 0xd1, 12]);
            bytes.extend(b"\xe8\x00\x12\x01\x06Count\xfe");
            bytes.extend(b"\xfe\x84\x92\x82\x08Integer\x83");
            bytes.extend(b"\xfe\x85\x9d\x82\xfe\x8c");
        }
    }
    bytes.extend(b"\xde\x04\xfe\xfe\x12CATCatalogManager");
    bytes.extend(b"\xfe\xfe\xfe");
    bytes.extend([0x81, 0x04, b'F', b'o', b'o', 0x84, 0xfe]);
    bytes.extend(b"\x4e\x11\x00\x00\x00DASSAULT-SYSTEMES\x05\x00\x00\x00CATIA");
    let native = crate::native::CatiaNative::decode(&bytes);
    let [run] = native.legacy_entity_runs.as_slice() else {
        panic!("one complete legacy entity run");
    };
    assert_eq!(run.scalar_values.len(), 1);
    assert_eq!(run.string_values.len(), 1);
    assert_eq!(run.integer_values.len(), 1);
    native
}

fn assert_first_candidate_refused(native: &crate::native::CatiaNative) {
    crate::test_support::with_entity_limit(0, |ctx| {
        let mut ir = CadIr::empty();
        let Err(cadmpeg_core::CodecError::ResourceLimit(limit)) =
            crate::formula::transfer_parameters(
                ctx,
                &mut ir,
                native,
                &mut cadmpeg_ir::Annotations::default(),
                &crate::decode::ModelingGraphScope::Unscoped,
            )
        else {
            panic!("legacy candidate must exceed the zero entity limit");
        };
        assert_eq!(
            limit.dimension,
            cadmpeg_core::decode::ResourceDimension::Entities
        );
        assert_eq!(limit.used, 0);
        assert_eq!(limit.operation, "admit CATIA formula candidate");
        assert!(ir.model.parameters.is_empty());
    });
}

#[test]
fn formula_legacy_scalar_limit_refuses_before_candidate_creation() {
    let mut native = legacy_values();
    let run = &mut native.legacy_entity_runs[0];
    run.string_values.clear();
    run.integer_values.clear();
    assert_first_candidate_refused(&native);
}

#[test]
fn formula_legacy_string_limit_refuses_before_candidate_creation() {
    let mut native = legacy_values();
    let run = &mut native.legacy_entity_runs[0];
    run.scalar_values.clear();
    run.integer_values.clear();
    assert_first_candidate_refused(&native);
}

#[test]
fn formula_legacy_integer_limit_refuses_before_candidate_creation() {
    let mut native = legacy_values();
    let run = &mut native.legacy_entity_runs[0];
    run.scalar_values.clear();
    run.string_values.clear();
    assert_first_candidate_refused(&native);
}

#[test]
fn formula_legacy_candidates_transfer_under_service_profile() {
    let native = legacy_values();
    let mut ir = CadIr::empty();
    crate::test_support::with_service_context(|ctx| {
        crate::formula::transfer_parameters(
            ctx,
            &mut ir,
            &native,
            &mut cadmpeg_ir::Annotations::default(),
            &crate::decode::ModelingGraphScope::Unscoped,
        )
        .map(crate::formula::FormulaTransfer::detached)
    })
    .expect("service profile admits three legacy candidates");
    assert_eq!(ir.model.parameters.len(), 3);
}

#[test]
fn formula_legacy_indexes_refuse_collection_limits_before_growth() {
    let native = legacy_values();
    let mut refused = std::collections::HashSet::new();
    for cap in 0..=256 {
        let result = crate::test_support::with_collection_limit(cap, |ctx| {
            crate::formula::transfer_parameters(
                ctx,
                &mut CadIr::empty(),
                &native,
                &mut cadmpeg_ir::Annotations::default(),
                &crate::decode::ModelingGraphScope::Unscoped,
            )
            .map(crate::formula::FormulaTransfer::detached)
        });
        match result {
            Err(cadmpeg_core::CodecError::ResourceLimit(limit)) => {
                refused.insert(limit.operation);
            }
            Ok(_) => {}
            Err(error) => panic!("unexpected legacy transfer error: {error}"),
        }
    }
    for operation in [
        "catia_legacy_parameter_candidates",
        "catia_legacy_parameter_entity_index",
        "catia_legacy_parameter_name_index",
        "catia_legacy_relation_index",
    ] {
        assert!(
            refused.contains(operation),
            "no low-limit refusal at {operation}"
        );
    }
}

#[test]
fn formula_legacy_parameter_identity_refuses_work_limit() {
    let native = legacy_values();
    let refused = crate::test_support::with_work_refusal("catia_legacy_parameter_id", |ctx| {
        crate::formula::transfer_parameters(
            ctx,
            &mut CadIr::empty(),
            &native,
            &mut cadmpeg_ir::Annotations::default(),
            &crate::decode::ModelingGraphScope::Unscoped,
        )
        .map(crate::formula::FormulaTransfer::detached)
    });
    assert!(
        matches!(refused, Err(cadmpeg_core::CodecError::ResourceLimit(limit))
        if limit.operation == "catia_legacy_parameter_id")
    );
}

#[test]
fn formula_legacy_selector_chain_refuses_work_limit() {
    let native = legacy_values();
    // The allowance includes all copied identifiers and index insertions before the selector visit.
    let refused =
        crate::test_support::with_work_refusal("catia_legacy_type_selector_chain", |ctx| {
            crate::formula::transfer_parameters(
                ctx,
                &mut CadIr::empty(),
                &native,
                &mut cadmpeg_ir::Annotations::default(),
                &crate::decode::ModelingGraphScope::Unscoped,
            )
            .map(crate::formula::FormulaTransfer::detached)
        });
    assert!(
        matches!(refused, Err(cadmpeg_core::CodecError::ResourceLimit(limit))
        if limit.operation == "catia_legacy_type_selector_chain")
    );
}

fn relation_fixture(
    count: usize,
) -> (
    crate::native::CatiaLegacyRelation,
    std::collections::BTreeMap<
        cadmpeg_ir::features::ParameterId,
        super::super::FormulaParameterCandidate<'static>,
    >,
    Vec<cadmpeg_ir::features::ParameterId>,
) {
    use cadmpeg_ir::features::{DesignParameter, ParameterId, ParameterValue};
    let native = legacy_values();
    let mut relation = native.legacy_entity_runs[0].relations[0].clone();
    relation.inputs = (1..=count)
        .map(|index| crate::native::CatiaLegacyRelationParameter {
            parameter: format!("#{index}_"),
            value_type: "Real".to_owned(),
        })
        .collect();
    relation.expression = relation
        .inputs
        .iter()
        .map(|input| input.parameter.as_str())
        .collect::<Vec<_>>()
        .join(" + ");
    let mut candidates = std::collections::BTreeMap::new();
    let mut ids = Vec::new();
    for (index, input) in relation.inputs.iter().enumerate() {
        let id = ParameterId::mint(format!("catia:test:parameter#{index}")).expect("identity");
        ids.push(id.clone());
        candidates.insert(
            id.clone(),
            super::super::FormulaParameterCandidate {
                parameter: DesignParameter {
                    id,
                    owner: None,
                    ordinal: 0,
                    name: input.parameter.clone(),
                    expression: "1".to_owned(),
                    display: None,
                    value: Some(ParameterValue::Real(
                        cadmpeg_ir::scalar::FiniteReal::new(1.0).expect("finite"),
                    )),
                    dependencies: cadmpeg_ir::features::DistinctMembers::default(),
                    properties: std::collections::BTreeMap::new(),
                    pmi: None,
                    native_ref: None,
                },
                parameter_type: super::super::FormulaParameterType::Real,
                output: None,
                source_order: u64::try_from(index).expect("fixture index"),
                storage: None,
            },
        );
    }
    (relation, candidates, ids)
}

#[test]
fn formula_legacy_symbol_join_indexes_once_for_many_distinct_inputs() {
    use std::collections::HashMap;
    const COUNT: usize = 4096;
    let (relation, candidates, ids) = relation_fixture(COUNT);
    let names = relation
        .inputs
        .iter()
        .zip(&ids)
        .map(|(input, id)| (input.parameter.as_str(), Some(id.clone())))
        .collect::<HashMap<_, _>>();
    // Same-width symbols require at least two byte-pair comparisons per old join visit.
    // Candidate lookups admit 55 comparisons for this tree. Every binding insertion
    // admits at least one node shift, apart from splits and key comparisons.
    let same_width_pairs = [9_u64, 90, 900, 3097]
        .into_iter()
        .map(|count| count * (count + 1) / 2)
        .sum::<u64>();
    let count = u64::try_from(COUNT).expect("fixture count");
    let alignment = std::mem::align_of::<super::super::EvaluatedFormulaValue>()
        .max(std::mem::align_of::<&str>())
        .max(std::mem::align_of::<usize>());
    let node_bytes = 11
        * (std::mem::size_of::<&str>()
            + std::mem::size_of::<super::super::EvaluatedFormulaValue>())
        + 16 * std::mem::size_of::<usize>()
        + 2 * alignment;
    let old_lower_bound = count * (count + 1) / 2
        + 4 * same_width_pairs
        + count * 22 * 55
        + count * u64::try_from(node_bytes).expect("node bound");
    // This allowance covers key bounds, node shifts, hash growth and evaluation.
    let allowance = count * 8192;
    assert!(allowance < old_lower_bound);
    let result = crate::test_support::with_work_limit(allowance, |ctx| {
        super::super::legacy_relation_evaluation(ctx, &relation, &names, &candidates)
    })
    .expect("indexed join fits a linear allowance")
    .expect("complete relation");
    assert_eq!(result.dependencies, ids);
    assert_eq!(result.source_type, "Real");
    assert_eq!(result.expression, relation.expression);
    assert_eq!(
        result.evaluated.scalar().expect("scalar result").value(),
        4096.0
    );
}

#[test]
fn formula_legacy_input_rejection_does_not_admit_unread_rows() {
    use std::collections::HashMap;
    for failure in [
        "missing-symbol",
        "missing-name",
        "ambiguous-name",
        "duplicate-binding",
        "incompatible-type",
    ] {
        let (mut relation, candidates, ids) = relation_fixture(2);
        match failure {
            "missing-symbol" => relation.inputs[0].parameter = "#0_".to_owned(),
            "incompatible-type" => relation.inputs[0].value_type = "Boolean".to_owned(),
            _ => {}
        }
        let mut names = HashMap::from([
            (relation.inputs[0].parameter.clone(), Some(ids[0].clone())),
            (relation.inputs[1].parameter.clone(), Some(ids[1].clone())),
        ]);
        match failure {
            "missing-name" => {
                names.remove(&relation.inputs[0].parameter);
            }
            "ambiguous-name" => {
                names.insert(relation.inputs[0].parameter.clone(), None);
            }
            "duplicate-binding" => {
                names.insert(relation.inputs[1].parameter.clone(), Some(ids[0].clone()));
            }
            _ => {}
        }
        relation.inputs.extend(std::iter::repeat_n(
            crate::native::CatiaLegacyRelationParameter {
                parameter: "#unread_".to_owned(),
                value_type: "Real".to_owned(),
            },
            4096,
        ));
        let names = names
            .iter()
            .map(|(name, id)| (name.as_str(), id.clone()))
            .collect::<HashMap<_, _>>();
        // The prefix has at most one binding insertion (three initial node passes)
        // and two candidate comparisons. Full-list admission alone needs 4098 units.
        assert!(
            crate::test_support::with_work_limit(4096, |ctx| {
                super::super::legacy_relation_evaluation(ctx, &relation, &names, &candidates)
            })
            .expect("first rejected binding leaves the large input tail unread")
            .is_none(),
            "{failure}"
        );
    }
}

#[test]
fn formula_legacy_symbol_index_preserves_bare_and_complete_ordinal_names() {
    use std::collections::HashMap;
    for (symbol, input) in [("#1_", "#1_"), ("#1_/12", "#1_"), ("#1_ /12", "#1_")] {
        let (mut relation, candidates, ids) = relation_fixture(1);
        relation.expression = symbol.to_owned();
        relation.inputs[0].parameter = input.to_owned();
        let names = HashMap::from([(input, Some(ids[0].clone()))]);
        let result = crate::test_support::with_service_context(|ctx| {
            super::super::legacy_relation_evaluation(ctx, &relation, &names, &candidates)
        })
        .expect("ordinal join")
        .expect("complete binding");
        assert_eq!(result.dependencies, ids);
        assert_eq!(
            result.evaluated.scalar().expect("scalar result").value(),
            1.0
        );
    }
    for input in ["#1_/12", "#1_ /12"] {
        let (mut relation, candidates, ids) = relation_fixture(1);
        relation.expression = input.to_owned();
        relation.inputs[0].parameter = input.to_owned();
        let names = HashMap::from([(input, Some(ids[0].clone()))]);
        let error =
            crate::test_support::with_work_refusal("catia_legacy_parameter_name_index", |ctx| {
                super::super::legacy_relation_evaluation(ctx, &relation, &names, &candidates)
                    .map(|result| result.is_some())
            })
            .expect_err("complete ordinal names still pass the symbol join");
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.operation == "catia_legacy_parameter_name_index")
        );
        assert!(crate::test_support::with_service_context(|ctx| {
            super::super::legacy_relation_evaluation(ctx, &relation, &names, &candidates)
        })
        .expect("complete ordinal binding")
        .is_none());
    }
    let (relation, candidates, ids) = relation_fixture(2);
    let names = HashMap::from([("#1_", Some(ids[0].clone())), ("#2_", Some(ids[1].clone()))]);
    for operation in [
        "catia_legacy_formula_symbol_index",
        "catia_formula_legacy_input_visits",
    ] {
        let error = crate::test_support::with_work_refusal(operation, |ctx| {
            super::super::legacy_relation_evaluation(ctx, &relation, &names, &candidates)
                .map(|result| result.is_some())
        })
        .expect_err("indexed membership and lazy visits retain refusal coverage");
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit) if limit.operation == operation)
        );
    }
}

#[test]
fn formula_legacy_empty_symbol_index_does_not_admit_unread_names() {
    let (mut relation, candidates, _) = relation_fixture(1);
    relation.expression = "1".to_owned();
    relation.inputs[0].parameter = "x".repeat(4096);
    relation
        .inputs
        .extend(std::iter::repeat_n(relation.inputs[0].clone(), 4096));
    let names = std::collections::HashMap::new();
    // One expression byte. Empty membership rejects before input rows or keys.
    assert!(crate::test_support::with_work_limit(1, |ctx| {
        super::super::legacy_relation_evaluation(ctx, &relation, &names, &candidates)
    })
    .expect("empty membership leaves long names and the input tail unread")
    .is_none());
}

#[test]
fn formula_legacy_empty_inputs_skip_row_admission() {
    use cadmpeg_core::decode::{refusal_probe::RefusalProbe, ResourceDimension};
    let (mut relation, candidates, _) = relation_fixture(0);
    relation.expression = "1".to_owned();
    let names = std::collections::HashMap::new();
    let _probe = RefusalProbe::arm(
        ResourceDimension::WorkUnits,
        "catia_formula_legacy_input_visits",
        None,
    );
    let result = crate::test_support::with_work_limit(u64::MAX, |ctx| {
        let result = super::super::legacy_relation_evaluation(ctx, &relation, &names, &candidates)?;
        assert_eq!(ctx.resource_refusal(), None);
        Ok::<_, cadmpeg_core::CodecError>(result)
    })
    .expect("empty input list has no row admission")
    .expect("constant relation");
    assert!(result.dependencies.is_empty());
    assert_eq!(result.source_type, "Real");
    assert_eq!(result.expression, "1");
    assert_eq!(
        result.evaluated.scalar().expect("scalar result").value(),
        1.0
    );
}
