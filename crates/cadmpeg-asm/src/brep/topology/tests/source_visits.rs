// SPDX-License-Identifier: Apache-2.0

use super::super::*;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

#[derive(Clone, Copy)]
enum SourcePass {
    Analytic,
    Faces,
    Reachable,
    SavedEdges,
    CurveSenses,
}

impl SourcePass {
    fn operation(self) -> &'static str {
        match self {
            Self::Analytic | Self::Faces | Self::CurveSenses => "ASM topology record pass",
            Self::Reachable => "ASM reachable face walk",
            Self::SavedEdges => "ASM saved edge record pass",
        }
    }

    fn complete_visits(self, count: usize) -> u64 {
        // Saved edges also complete the separate shell-record filter pass.
        u64::try_from(count).unwrap() * match self {
            Self::SavedEdges => 2,
            _ => 1,
        }
    }

    fn run(self, ctx: &DecodeContext<'_>, records: &[Record],
        token_table: &nurbs::toks::SubtypeTable,
        scratch: &mut cadmpeg_core::decode::ScopedReservation<'_>) -> Result<(), CodecError> {
        let by_index = HashMap::new();
        let inputs = TopologyContext {
            ctx, by_index: &by_index, token_table,
            purpose: DecodePurpose::Model, format: crate::asm_format!("f3d"),
        };
        let mut out = AsmBrep::default();
        let mut carriers = Carriers::default();
        let mut reach = Reachable::default();
        match self {
            Self::Analytic => {
                let (decoded, inward) = decode_analytic_carriers(ctx, records, scratch)?;
                assert!(decoded.curve_geo.is_empty() && decoded.surface_geo.is_empty());
                assert!(inward.is_empty());
            }
            Self::Faces => keep_faces_and_carriers(
                inputs, &mut out, records, &mut carriers, &mut reach, scratch,
            )?,
            Self::Reachable => walk_reachable_topology(
                inputs, &mut out, records, &mut carriers, &mut reach, scratch,
            )?,
            Self::SavedEdges => {
                let wire = collect_wire_topology(inputs, &mut out, records, Some(3),
                    &mut carriers, &mut reach, scratch)?;
                assert!(wire.wire_edges_by_shell.is_empty());
                assert!(wire.free_vertices_by_shell.is_empty() && wire.saved_free_edges.is_empty());
            }
            Self::CurveSenses => {
                let (reversed, forward) = classify_edge_curve_senses(ctx, records, &reach)?;
                assert!(reversed.is_empty() && forward.is_empty());
            }
        }
        assert!(carriers.curve_geo.is_empty() && carriers.surface_geo.is_empty());
        assert!(reach.faces.is_empty() && reach.edges.is_empty() && reach.curves.is_empty());
        assert!(out.curves.is_empty() && out.surfaces.is_empty() && out.faces.is_empty());
        assert!(out.edges.is_empty() && out.wire_topologies.is_empty());
        Ok(())
    }
}

fn empty_table() -> nurbs::toks::SubtypeTable {
    let arena = DecodeArena::new();
    let policy = DecodePolicy::service();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    nurbs::toks::SubtypeTable::from_records(&ctx, &[]).unwrap()
}

fn records() -> [Record; 3] {
    std::array::from_fn(|index| crate::test_support::sab::record(
index,
"unrelated".into(),
Vec::<Token>::new().into(),
0,
0
))
}

fn source_refusal(pass: SourcePass) {
    let source = records();
    let token_table = empty_table();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut scratch = ctx.reserve_scoped(0, "test topology scratch").unwrap();
    let Err(CodecError::ResourceLimit(first)) = pass.run(&ctx, &source, &token_table, &mut scratch) else {
        panic!("expected the first topology source visit to refuse");
    };
    assert_eq!(first.dimension, ResourceDimension::WorkUnits);
    assert_eq!(first.operation, pass.operation());
    assert_eq!((first.limit, first.used, first.additional), (0, 0, 1));
    for replay in [source.as_slice(), &[]] {
        assert!(matches!(pass.run(&ctx, replay, &token_table, &mut scratch),
            Err(CodecError::ResourceLimit(last)) if last == first));
    }
    drop(scratch);
    assert!(matches!(ctx.finish_session(),
        Err(CodecError::ResourceLimit(last)) if last == first));
}

fn source_acceptance(pass: SourcePass) {
    let source = records();
    let token_table = empty_table();
    for input in [source.as_slice(), &[]] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = pass.complete_visits(input.len());
        policy.limits.max_collection_items = 0;
        policy.limits.max_materialized_bytes = 0;
        policy.limits.max_retained_bytes = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let mut scratch = ctx.reserve_scoped(0, "test topology scratch").unwrap();
        pass.run(&ctx, input, &token_table, &mut scratch).unwrap();
        drop(scratch);
        ctx.finish_session().unwrap();
    }
}

macro_rules! source_controls {
    ($refusal:ident, $acceptance:ident, $pass:ident) => {
        #[test]
        fn $refusal() { source_refusal(SourcePass::$pass); }
        #[test]
        fn $acceptance() { source_acceptance(SourcePass::$pass); }
    };
}

source_controls!(analytic_record_source_refuses_one_visit,
    analytic_record_source_accepts_exact_visits_and_empty_input, Analytic);
source_controls!(face_record_source_refuses_one_visit,
    face_record_source_accepts_exact_visits_and_empty_input, Faces);
source_controls!(reachable_record_source_refuses_one_visit,
    reachable_record_source_accepts_exact_visits_and_empty_input, Reachable);
source_controls!(saved_edge_record_source_refuses_one_visit,
    saved_edge_record_source_accepts_exact_saved_and_shell_pass_visits, SavedEdges);
source_controls!(edge_sense_record_source_refuses_one_visit,
    edge_sense_record_source_accepts_exact_visits_and_empty_input, CurveSenses);

#[test]
fn analytic_carrier_allocation_refuses_after_one_record_without_visiting_the_tail() {
    let mut source = records();
    let token_table = empty_table();
    source[0] = crate::test_support::sab::record(0, "plane-surface".into(),
        vec![Token::Position([0.0, 0.0, 0.0])].into(), 0, 0);
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    // One record visit, then one analytic carrier token visit.
    policy.limits.max_work_units = 2;
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut scratch = ctx.reserve_scoped(0, "test topology scratch").unwrap();
    let Err(CodecError::ResourceLimit(first)) = SourcePass::Analytic.run(&ctx, &source, &token_table, &mut scratch) else {
        panic!("expected the first actual carrier allocation to refuse");
    };
    assert_eq!(first.dimension, ResourceDimension::CollectionItems);
    assert_eq!(first.operation, "ASM carrier positions");
    assert_eq!((first.limit, first.used, first.additional), (0, 0, 1));
    for replay in [source.as_slice(), &[]] {
        assert!(matches!(SourcePass::Analytic.run(&ctx, replay, &token_table, &mut scratch),
            Err(CodecError::ResourceLimit(last)) if last == first));
    }
    drop(scratch);
    assert!(matches!(ctx.finish_session(),
        Err(CodecError::ResourceLimit(last)) if last == first));
}

#[cfg(target_pointer_width = "64")]
fn first_index_refusal(pass: SourcePass) {
    let mut source = records();
    let token_table = empty_table();
    let limit = 9_223_372_036_854_775_807_u64;
    source[0].index = usize::try_from(limit + 1).unwrap();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 1;
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut scratch = ctx.reserve_scoped(0, "test topology scratch").unwrap();
    let Err(CodecError::ResourceLimit(first)) = pass.run(&ctx, &source, &token_table, &mut scratch) else {
        panic!("expected the first record index conversion to refuse");
    };
    assert_eq!(first.dimension, ResourceDimension::Codec("ASM record index"));
    assert_eq!(first.operation, "ASM record index");
    assert_eq!((first.limit, first.used, first.additional), (limit, limit, 1));
    for replay in [source.as_slice(), &[]] {
        assert!(matches!(pass.run(&ctx, replay, &token_table, &mut scratch),
            Err(CodecError::ResourceLimit(last)) if last == first));
    }
    drop(scratch);
    assert!(matches!(ctx.finish_session(),
        Err(CodecError::ResourceLimit(last)) if last == first));
}

#[cfg(target_pointer_width = "64")]
#[test]
fn reachable_record_index_refuses_after_one_visit_without_admitting_the_tail() {
    first_index_refusal(SourcePass::Reachable);
}

#[cfg(target_pointer_width = "64")]
#[test]
fn saved_edge_record_index_refuses_after_one_visit_without_admitting_the_tail() {
    first_index_refusal(SourcePass::SavedEdges);
}
