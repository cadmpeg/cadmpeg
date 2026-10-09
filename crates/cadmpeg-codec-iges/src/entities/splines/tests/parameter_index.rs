// SPDX-License-Identifier: Apache-2.0

use crate::global::ProjectedGlobal;
use crate::parameter::ParameterRecord;
use crate::test_support::sequence_index::{parameter_inputs, work};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use cadmpeg_ir::CadIr;
use crate::entities::geometry::SourceSequences;

fn run<'ctx>(ir: &mut CadIr, parameters: &[ParameterRecord], global: &ProjectedGlobal,
    ctx: &'ctx DecodeContext<'_>, sequences: &mut SourceSequences<'ctx>) -> Result<(), CodecError> {
    super::super::project(ir, &[], parameters, global, ctx, sequences).map(|_| ())
}

#[test]
fn splines_parameter_index_first_and_last_visits_refuse_without_model_changes() {
    for count in [1, 64] {
        let (parameters, global) = parameter_inputs(count);
        for visited in [0, count - 1] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = work(visited);
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let mut sequences = SourceSequences::new(&ctx).unwrap();
        let mut ir = CadIr::empty();
        let first = match run(&mut ir, &parameters, &global, &ctx, &mut sequences) {
            Err(CodecError::ResourceLimit(first)) => first,
            _ => panic!("expected actual owning Parameter source refusal"),
        };
        assert_eq!(first.dimension, ResourceDimension::WorkUnits);
        assert_eq!(first.operation, "iges splines parameter index traversal");
        assert_eq!((first.used, first.additional, first.limit), (work(visited), 1, work(visited)));
        assert_eq!(ir.model, CadIr::empty().model);
        assert!(matches!(run(&mut ir, &parameters, &global, &ctx, &mut sequences),
            Err(CodecError::ResourceLimit(last)) if last == first));
        assert!(matches!(run(&mut ir, &[], &global, &ctx, &mut sequences),
            Err(CodecError::ResourceLimit(last)) if last == first));
        drop(sequences);
        assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(last)) if last == first));
        }
    }
}

#[test]
fn splines_parameter_index_completion_has_no_terminal_source_step() {
    for count in [1, 64] {
        let (parameters, global) = parameter_inputs(count);
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = work(count);
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let mut sequences = SourceSequences::new(&ctx).unwrap();
        let mut ir = CadIr::empty();
        run(&mut ir, &parameters, &global, &ctx, &mut sequences).unwrap();
        assert_eq!(ir.model, CadIr::empty().model);
        drop(sequences);
        ctx.finish_session().unwrap();
    }
}

#[test]
fn empty_splines_parameter_index_is_free_with_zero_budgets() {
    let (parameters, global) = parameter_inputs(0);
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    policy.limits.max_collection_items = 0;
    policy.limits.max_materialized_bytes = 0;
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_entities = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut sequences = SourceSequences::new(&ctx).unwrap();
    let mut ir = CadIr::empty();
    run(&mut ir, &parameters, &global, &ctx, &mut sequences).unwrap();
    assert_eq!(ir.model, CadIr::empty().model);
    drop(sequences);
    ctx.finish_session().unwrap();
}
