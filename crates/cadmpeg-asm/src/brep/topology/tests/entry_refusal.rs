// SPDX-License-Identifier: Apache-2.0

#[test]
fn asm_absent_ring_head_preserves_original_refusal() {
    let record = crate::sab::Record {
        index: 0, name: "loop".into(), tokens: Vec::new().into(), offset: 0, len: 0,
    };
    let by_index = std::collections::HashMap::new();
    let kept = std::collections::HashSet::new();
    crate::test_support::with_entry_context(|ctx, original| {
        let result = super::super::ring_coedges(ctx, &record, &by_index, &kept, crate::asm_format!("sat"));
        match original {
            Some(first) => assert!(matches!(result, Err(cadmpeg_core::CodecError::ResourceLimit(last)) if last == first)),
            None => assert!(result.expect("absent ring is free").is_empty()),
        }
    });
}

#[test]
fn asm_absent_wire_edge_preserves_original_refusal() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;
    for dimension in [None, Some(ResourceDimension::WorkUnits), Some(ResourceDimension::CollectionItems),
        Some(ResourceDimension::MaterializedBytes), Some(ResourceDimension::RetainedBytes),
        Some(ResourceDimension::Entities), Some(ResourceDimension::RecursionDepth)] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = 0;
        policy.limits.max_collection_items = 0;
        policy.limits.max_materialized_bytes = 0;
        policy.limits.max_retained_bytes = 0;
        policy.limits.max_entities = 0;
        policy.limits.max_recursion_depth = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty input");
        let table = crate::nurbs::toks::SubtypeTable::from_records(&ctx, &[]).expect("empty subtype owner");
        let mut scratch = ctx.reserve_scoped(0, "test actual wire scratch").expect("scratch before refusal");
        let original = dimension.map(|dimension| {
            let result = match dimension {
                ResourceDimension::WorkUnits => ctx.charge_work(1, "test original wire refusal"),
                ResourceDimension::CollectionItems => ctx.charge_collection_items(1, "test original wire refusal"),
                ResourceDimension::MaterializedBytes => ctx.reserve_scoped(1, "test original wire refusal").map(|_| ()),
                ResourceDimension::RetainedBytes => ctx.charge_retained(1, "test original wire refusal"),
                ResourceDimension::Entities => ctx.charge_entities(1, "test original wire refusal"),
                ResourceDimension::RecursionDepth => ctx.enter_nested("test original wire refusal").map(|_| ()),
                _ => panic!("wire refusal dimension"),
            };
            let Err(CodecError::ResourceLimit(first)) = result else { panic!("original wire refusal"); };
            assert_eq!(first.dimension, dimension);
            first
        });
        let by_index = std::collections::HashMap::new();
        for _ in 0..64 {
            let mut out = crate::brep::AsmBrep::default();
            let mut carriers = crate::brep::Carriers::default();
            let mut reach = crate::brep::Reachable::default();
            let inputs = super::super::TopologyContext { ctx: &ctx, by_index: &by_index,
                token_table: &table, purpose: crate::brep::DecodePurpose::Model, format: crate::asm_format!("sat") };
            let result = super::super::keep_wire_edge(inputs, &mut out, 7, &mut carriers, &mut reach, &mut scratch);
            match original {
                Some(first) => assert!(matches!(result, Err(CodecError::ResourceLimit(last)) if last == first)),
                None => result.expect("absent wire edge is free"),
            }
            assert!(out.edges.is_empty() && reach.edges.is_empty() && carriers.curve_geo.is_empty());
        }
        drop(scratch);
        match original {
            Some(first) => assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(last)) if last == first)),
            None => ctx.finish_session().expect("fresh absent wire route finishes"),
        }
    }
}
