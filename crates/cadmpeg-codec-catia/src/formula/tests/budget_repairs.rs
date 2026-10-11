// SPDX-License-Identifier: Apache-2.0
//! Formula candidate and scope admission.

use crate::test_support::{with_materialized_limit, with_retained_limit, with_work_refusal};
use cadmpeg_core::CodecError;

#[test]
fn formula_entity_index_waits_for_a_lookup() {
    use cadmpeg_core::decode::{refusal_probe::RefusalProbe, ResourceDimension};

    let mut native = crate::native::CatiaNative::decode(
        &crate::test_support::test_formula::standard_catpart_with_definition_chain_type(
            "Boolean",
            &[0x84, 0x88, 0x82, 0x32, 4, 0, 0, 0, 0x81],
        ),
    );
    let mut record = native.entity_records[0].clone();
    record.value_production = None;
    record.object_production = None;
    native.legacy_entity_runs.clear();
    native.entity_records = (0..4096)
        .map(|ordinal| {
            let mut entity = record.clone();
            entity.id = format!("synthetic:test:entity#{ordinal}");
            entity.ordinal = ordinal;
            entity
        })
        .collect();
    let _probe = RefusalProbe::arm(
        ResourceDimension::WorkUnits,
        "catia_formula_entity_index",
        None,
    );
    crate::test_support::with_work_limit(u64::MAX, |ctx| {
        let mut ir = cadmpeg_ir::document::CadIr::empty();
        let transfer = super::super::transfer_parameters(
            ctx,
            &mut ir,
            &native,
            &mut cadmpeg_ir::Annotations::default(),
            &crate::decode::ModelingGraphScope::Unscoped,
        )?;
        assert!(ir.model.parameters.is_empty());
        assert_eq!(transfer.typed_parameter_count, 0);
        assert_eq!(transfer.definition_chain_parameter_count, 0);
        assert_eq!(transfer.relation_program_parameter_count, 0);
        assert_eq!(transfer.legacy_parameter_count, 0);
        assert_eq!(transfer.legacy_formula_count, 0);
        assert!(transfer.consumed_object_records.is_empty());
        assert_eq!(ctx.resource_refusal(), None);
        Ok::<_, CodecError>(())
    })
    .expect("unqueried entity records have no lookup-index work");
}

#[test]
fn formula_entity_index_admits_the_first_lookup() {
    let native = crate::native::CatiaNative::decode(
        &crate::test_support::test_formula::standard_catpart_with_formula_relation(4, false),
    );
    let refused = with_work_refusal("catia_formula_entity_index", |ctx| {
        let mut ir = cadmpeg_ir::document::CadIr::empty();
        super::super::transfer_parameters(
            ctx,
            &mut ir,
            &native,
            &mut cadmpeg_ir::Annotations::default(),
            &crate::decode::ModelingGraphScope::Unscoped,
        )
        .map(|transfer| transfer.typed_parameter_count)
    });
    assert!(matches!(refused, Err(CodecError::ResourceLimit(limit))
        if limit.operation == "catia_formula_entity_index"));
    let (count, parameters) = crate::test_support::with_service_context(|ctx| {
        let mut ir = cadmpeg_ir::document::CadIr::empty();
        let transfer = super::super::transfer_parameters(
            ctx,
            &mut ir,
            &native,
            &mut cadmpeg_ir::Annotations::default(),
            &crate::decode::ModelingGraphScope::Unscoped,
        )?;
        Ok::<_, CodecError>((transfer.typed_parameter_count, ir.model.parameters))
    })
    .expect("queried entity index preserves the formula transfer");
    assert_eq!(count, 2);
    let [input, output] = parameters.as_slice() else {
        panic!("one input and one formula output");
    };
    assert_eq!(input.name, "Thickness");
    assert_eq!(input.expression, "35 mm");
    assert_eq!(output.name, "Result");
    assert_eq!(output.expression, "#1_ /2-2mm");
    assert_eq!(
        output.dependencies.as_slice(),
        std::slice::from_ref(&input.id)
    );
}

#[test]
fn formula_transfer_moves_the_surviving_candidate_without_copying() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

    let bytes = crate::test_support::test_formula::standard_catpart_with_definition_chain_type(
        "Boolean",
        &[0x84, 0x88, 0x82, 0x32, 4, 0, 0, 0, 0x81],
    );
    let native = crate::native::CatiaNative::decode(&bytes);
    let entity = native
        .entity_records
        .iter()
        .find(|entity| entity.definition_chain_value().is_some())
        .expect("definition chain");
    let _probe = cadmpeg_core::decode::refusal_probe::RefusalProbe::arm(
        ResourceDimension::WorkUnits,
        "catia_formula_parameter_copy",
        None,
    );
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = u64::MAX;
    let (ctx, _) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).expect("root");
    let mut ir = cadmpeg_ir::document::CadIr::empty();
    let transfer = super::super::transfer_parameters(
        &ctx,
        &mut ir,
        &native,
        &mut cadmpeg_ir::Annotations::default(),
        &crate::decode::ModelingGraphScope::Unscoped,
    )
    .expect("transfer does not copy the surviving candidate");
    assert_eq!(transfer.definition_chain_parameter_count, 1);
    let [parameter] = ir.model.parameters.as_slice() else {
        panic!("one definition-chain parameter");
    };
    assert_eq!(
        parameter.name,
        entity
            .definition_chain_value()
            .expect("definition chain")
            .selector
            .value
    );
    assert_eq!(parameter.expression, "true");
    assert_eq!(
        parameter.value,
        Some(cadmpeg_ir::features::ParameterValue::Boolean(true))
    );
    assert_eq!(parameter.native_ref.as_deref(), Some(entity.id.as_str()));
    assert_eq!(parameter.ordinal, 0);
    assert_eq!(ctx.resource_refusal(), None);
}

#[test]
fn formula_transfer_preserves_the_candidate_promotion_refusal() {
    let bytes = crate::test_support::test_formula::standard_catpart_with_definition_chain_type(
        "Boolean",
        &[0x84, 0x88, 0x82, 0x32, 4, 0, 0, 0, 0x81],
    );
    let native = crate::native::CatiaNative::decode(&bytes);
    let refused = crate::test_support::with_retained_refusal(
        &bytes,
        "catia_formula_candidate_scratch",
        |ctx| {
            let mut ir = cadmpeg_ir::document::CadIr::empty();
            let result = super::super::transfer_parameters(
                ctx,
                &mut ir,
                &native,
                &mut cadmpeg_ir::Annotations::default(),
                &crate::decode::ModelingGraphScope::Unscoped,
            );
            let Err(CodecError::ResourceLimit(limit)) = result else {
                panic!("candidate promotion must refuse");
            };
            assert_eq!(ctx.resource_refusal(), Some(limit));
            assert!(ir.model.parameters.is_empty());
            assert!(matches!(ctx.charge_work(0, "after candidate promotion"),
                Err(CodecError::ResourceLimit(original)) if original == limit));
            Err::<(), _>(CodecError::ResourceLimit(limit))
        },
    );
    assert!(matches!(refused, Err(CodecError::ResourceLimit(limit))
        if limit.operation == "catia_formula_candidate_scratch"));
}

#[test]
fn formula_conflicting_input_admits_its_surviving_index_key() {
    let native = crate::native::CatiaNative::decode(
        &crate::test_support::test_formula::standard_catpart_with_formula_relation(4, false),
    );
    let entity = native
        .entity_records
        .iter()
        .find(|entity| entity.parameter_value().is_some())
        .expect("typed input");
    let value = entity.parameter_value().expect("typed input");
    let run = |ctx: &cadmpeg_core::decode::DecodeContext<'_>| -> Result<_, CodecError> {
        let existing =
            super::super::typed_entity_parameter_candidate(ctx, entity, value, "LENGTH")?
                .expect("candidate");
        let mut incoming =
            super::super::typed_entity_parameter_candidate(ctx, entity, value, "LENGTH")?
                .expect("candidate");
        incoming.parameter.value = None;
        let expected = existing.parameter.id.clone();
        let mut scratch = ctx.reserve_scoped(0, "test conflict index")?;
        let mut candidates = std::collections::BTreeMap::from([(expected.clone(), existing)]);
        let mut conflicts = std::collections::BTreeSet::new();
        super::super::merge_formula_parameter_candidate(
            ctx,
            &mut scratch,
            &mut candidates,
            &mut conflicts,
            incoming,
        )?;
        drop(candidates);
        assert_eq!(conflicts.into_iter().collect::<Vec<_>>(), [expected]);
        Ok(())
    };
    let refusal = with_work_refusal("catia_formula_conflict_id", |ctx| {
        let result = run(ctx);
        if let Err(CodecError::ResourceLimit(limit)) = &result {
            assert_eq!(ctx.resource_refusal().as_ref(), Some(limit));
        }
        result
    });
    assert!(matches!(refusal, Err(CodecError::ResourceLimit(limit))
        if limit.operation == "catia_formula_conflict_id"));
    with_retained_limit(0, run).expect("the surviving conflict key remains scoped");
}

#[test]
fn formula_speculative_candidates_release_scoped_storage() {
    let native = crate::native::CatiaNative::decode(
        &crate::test_support::test_formula::standard_catpart_with_formula_relation(4, false),
    );
    let entity = native
        .entity_records
        .iter()
        .find(|entity| entity.parameter_value().is_some())
        .expect("typed input");
    let value = entity.parameter_value().expect("typed input");
    with_materialized_limit(65536, |ctx| -> Result<_, CodecError> {
        for _ in 0..64 {
            let candidate =
                super::super::typed_entity_parameter_candidate(ctx, entity, value, "LENGTH")?
                    .expect("candidate");
            assert_eq!(candidate.parameter.name, value.name.value);
            drop(candidate);
            let storage = ctx.reserve_scoped(65536, "test released candidate")?;
            drop(storage);
        }
        Ok(())
    })
    .expect("candidate storage tracks live candidates");
    with_retained_limit(0, |ctx| -> Result<_, CodecError> {
        let candidate =
            super::super::typed_entity_parameter_candidate(ctx, entity, value, "LENGTH")?
                .expect("candidate");
        assert_eq!(
            candidate.parameter.native_ref.as_deref(),
            Some(entity.id.as_str())
        );
        Ok(())
    })
    .expect("speculative candidates consume no retained storage");
}

#[test]
fn formula_container_scope_refuses_variable_length_equality() {
    let binding = super::binding(&"stream".repeat(1024));
    let refused = with_work_refusal("catia_legacy_container_scope_match", |ctx| {
        super::super::outer_container_in_scope(
            ctx,
            Some(&binding),
            super::LegacyModelingScope::Container(&binding),
        )
    });
    assert!(
        matches!(refused, Err(CodecError::ResourceLimit(limit)) if limit.operation == "catia_legacy_container_scope_match")
    );
    let mut other = binding.clone();
    other.ordinal += 1;
    assert!(!crate::test_support::with_work_limit(
        0,
        |ctx| super::super::outer_container_in_scope(
            ctx,
            Some(&other),
            super::LegacyModelingScope::Container(&binding)
        )
    )
    .expect("scalar mismatch runs no string comparison"));
}

#[test]
fn formula_ordered_parameter_small_sorts_skip_constant_work() {
    use cadmpeg_core::decode::{refusal_probe::RefusalProbe, ResourceDimension};
    let operation = "catia_formula_ordered_parameters_sort";
    let native = |count: usize| {
        let mut native = super::legacy_admission::legacy_values();
        if count < 3 {
            native.legacy_entity_runs[0].integer_values.clear();
        }
        if count < 2 {
            native.legacy_entity_runs[0].string_values.clear();
        }
        if count < 1 {
            native.legacy_entity_runs[0].scalar_values.clear();
        }
        native
    };
    let transfer = |ctx: &cadmpeg_core::decode::DecodeContext<'_>,
                    native: &crate::native::CatiaNative| {
        let mut ir = cadmpeg_ir::document::CadIr::empty();
        let transferred = super::super::transfer_parameters(
            ctx,
            &mut ir,
            native,
            &mut cadmpeg_ir::Annotations::default(),
            &crate::decode::ModelingGraphScope::Unscoped,
        )?;
        assert_eq!(
            transferred.legacy_parameter_count,
            ir.model.parameters.len()
        );
        Ok::<_, CodecError>(ir.model.parameters)
    };
    for count in 0..=2 {
        let native = native(count);
        let _probe = RefusalProbe::arm(ResourceDimension::WorkUnits, operation, None);
        let parameters = crate::test_support::with_work_limit(u64::MAX, |ctx| {
            let parameters = transfer(ctx, &native)?;
            assert_eq!(ctx.resource_refusal(), None);
            Ok::<_, CodecError>(parameters)
        })
        .expect("small numeric source orders need no general sort");
        assert_eq!(parameters.len(), count);
        for (ordinal, parameter) in parameters.iter().enumerate() {
            assert_eq!(
                parameter.ordinal,
                u32::try_from(ordinal).expect("fixture ordinal")
            );
            assert_eq!(parameter.name, ["Result", "Responsible"][ordinal]);
        }
    }
    let native = native(3);
    let parameters = crate::test_support::with_service_context(|ctx| transfer(ctx, &native))
        .expect("larger source ordering");
    assert_eq!(
        parameters
            .iter()
            .map(|parameter| parameter.name.as_str())
            .collect::<Vec<_>>(),
        ["Result", "Responsible", "Count"]
    );
    let error = with_work_refusal(operation, |ctx| transfer(ctx, &native))
        .expect_err("larger source orders retain sorting admission");
    assert!(matches!(error, CodecError::ResourceLimit(limit) if limit.operation == operation));
}
