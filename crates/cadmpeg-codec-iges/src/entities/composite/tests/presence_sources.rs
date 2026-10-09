// SPDX-License-Identifier: Apache-2.0

use super::*;
use crate::directory::DirectoryEntry;
use crate::entities::geometry::SourceSequences;
use crate::global::ProjectedGlobal;
use crate::test_support::sequence_index::parameter_inputs;
use std::collections::BTreeMap;

fn run<'ctx>(ir: &mut CadIr, directory: &[DirectoryEntry], global: &ProjectedGlobal,
    ctx: &'ctx DecodeContext<'_>, sequences: &mut SourceSequences<'ctx>, type_130: bool,
) -> Result<(), CodecError> {
    let entries = BTreeMap::new();
    let records = BTreeMap::new();
    if type_130 {
        super::super::project_type_130_children(ir, directory, (&entries, &records),
            global, ctx, sequences).map(|_| ())
    } else {
        super::super::project(ir, directory, (&entries, &records),
            global, ctx, sequences).map(|_| ())
    }
}

#[test]
fn composite_presence_searches_refuse_first_and_last_actual_directory_visits() {
    let (_, global) = parameter_inputs(0);
    for type_130 in [false, true] {
        for count in [1, 64] {
            let mut directory: Vec<_> = (0..count).map(|index| {
                crate::test_support::directory_target(u32::try_from(2 * index + 1).unwrap(), 116)
            }).collect();
            // The Type130 pass first needs one matching Type102 presence visit.
            if type_130 { directory[0].entity_type = 102; }
            let prelude = u64::from(type_130);
            let operation = if type_130 { "iges composite Type130 presence search" }
                else { "iges composite presence search" };
            for visited in [0, count - 1] {
                let arena = DecodeArena::new();
                let mut policy = DecodePolicy::service();
                let cap = prelude + u64::try_from(visited).unwrap();
                policy.limits.max_work_units = cap;
                let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
                let mut sequences = SourceSequences::new(&ctx).unwrap();
                let mut ir = CadIr::empty();
                let first = match run(&mut ir, &directory, &global, &ctx, &mut sequences, type_130) {
                    Err(CodecError::ResourceLimit(first)) => first,
                    _ => panic!("expected actual Directory presence visit refusal"),
                };
                assert_eq!(first.dimension, ResourceDimension::WorkUnits);
                assert_eq!(first.operation, operation);
                assert_eq!((first.used, first.additional, first.limit), (cap, 1, cap));
                assert_eq!(ir.model, CadIr::empty().model);
                for source in [directory.as_slice(), &[]] {
                    assert!(matches!(run(&mut ir, source, &global, &ctx, &mut sequences, type_130),
                        Err(CodecError::ResourceLimit(last)) if last == first));
                }
                drop(sequences);
                assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(last)) if last == first));
            }
        }
    }
}

#[test]
fn composite_presence_exhaustion_accepts_exact_work_and_leaves_the_model_empty() {
    let (_, global) = parameter_inputs(0);
    for type_130 in [false, true] {
        for count in [0, 1, 64] {
            let mut directory: Vec<_> = (0..count).map(|index| {
                crate::test_support::directory_target(u32::try_from(2 * index + 1).unwrap(), 116)
            }).collect();
            if type_130 && count != 0 { directory[0].entity_type = 102; }
            let prelude = u64::from(type_130 && count != 0);
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_work_units = prelude + u64::try_from(count).unwrap();
            policy.limits.max_collection_items = 0;
            policy.limits.max_materialized_bytes = 0;
            policy.limits.max_retained_bytes = 0;
            policy.limits.max_entities = 0;
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
            let mut sequences = SourceSequences::new(&ctx).unwrap();
            let mut ir = CadIr::empty();
            run(&mut ir, &directory, &global, &ctx, &mut sequences, type_130).unwrap();
            assert_eq!(ir.model, CadIr::empty().model);
            drop(sequences);
            ctx.finish_session().unwrap();
        }
    }
}
