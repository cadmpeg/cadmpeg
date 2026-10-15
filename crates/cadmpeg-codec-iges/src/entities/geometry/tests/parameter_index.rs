// SPDX-License-Identifier: Apache-2.0

use crate::global::ProjectedGlobal;
use crate::parameter::ParameterRecord;
use crate::test_support::sequence_index::{parameter_inputs, work};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use cadmpeg_ir::CadIr;

fn run(
    ir: &mut CadIr,
    parameters: &[ParameterRecord],
    global: &ProjectedGlobal,
    ctx: &DecodeContext<'_>,
) -> Result<(), CodecError> {
    super::super::project_geometry(
        ir,
        &[],
        parameters,
        &std::collections::BTreeMap::new(),
        global,
        ctx,
    )
    .map(|_| ())
}

#[test]
fn geometry_parameter_index_first_and_last_visits_refuse_without_model_changes() {
    for count in [1, 64] {
        let (parameters, global) = parameter_inputs(count);
        for visited in [0, count - 1] {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_work_units = work(visited);
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
            let mut ir = CadIr::empty();
            let Err(CodecError::ResourceLimit(first)) = run(&mut ir, &parameters, &global, &ctx)
            else {
                panic!("expected actual owning Parameter source refusal")
            };
            assert_eq!(first.dimension, ResourceDimension::WorkUnits);
            assert_eq!(first.operation, "iges geometry parameter traversal");
            assert_eq!(
                (first.used, first.additional, first.limit),
                (work(visited), 1, work(visited))
            );
            assert_eq!(ir.model, CadIr::empty().model);
            assert!(matches!(run(&mut ir, &parameters, &global, &ctx),
            Err(CodecError::ResourceLimit(last)) if last == first));
            assert!(matches!(run(&mut ir, &[], &global, &ctx),
            Err(CodecError::ResourceLimit(last)) if last == first));
            assert!(
                matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(last)) if last == first)
            );
        }
    }
}

#[test]
fn geometry_parameter_index_completion_has_no_terminal_source_step() {
    for count in [1, 64] {
        let (parameters, global) = parameter_inputs(count);
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = work(count);
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let mut ir = CadIr::empty();
        // The completed owning map goes directly to the first real child map.
        let Err(CodecError::ResourceLimit(first)) = run(&mut ir, &parameters, &global, &ctx) else {
            panic!("expected next projection's actual source visit")
        };
        assert_eq!(first.dimension, ResourceDimension::WorkUnits);
        assert_eq!(first.operation, "iges splines parameter index traversal");
        assert_eq!(
            (first.used, first.additional, first.limit),
            (work(count), 1, work(count))
        );
        assert_eq!(ir.model, CadIr::empty().model);
        assert!(
            matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(last)) if last == first)
        );
        let arena = DecodeArena::new();
        let policy = DecodePolicy::service();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        run(&mut ir, &parameters, &global, &ctx).unwrap();
        assert_eq!(ir.model, CadIr::empty().model);
        ctx.finish_session().unwrap();
    }
}

#[test]
fn empty_geometry_parameter_index_is_free_with_zero_budgets() {
    let (parameters, global) = parameter_inputs(0);
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    policy.limits.max_collection_items = 0;
    policy.limits.max_materialized_bytes = 0;
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_entities = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut ir = CadIr::empty();
    run(&mut ir, &parameters, &global, &ctx).unwrap();
    assert_eq!(ir.model, CadIr::empty().model);
    ctx.finish_session().unwrap();
}
