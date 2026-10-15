// SPDX-License-Identifier: Apache-2.0
//! Ordered support membership and temporary index admission.

use super::super::{push_distinct_support, B5SupportIndex, B5SupportPlan};
use crate::test_support::test_b5::finite_pair;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_core::CodecError;

#[test]
fn b5_support_index_preserves_numeric_equality_and_first_occurrence_order() {
    crate::test_support::with_service_context(|ctx| {
        let mut storage = ctx.reserve_scoped(0, "test_support_plan")?;
        let mut index_storage = ctx.reserve_scoped(0, "test_support_index")?;
        let mut index = B5SupportIndex::new();
        let mut supports = B5SupportPlan::new();
        let first = (10, 20, finite_pair([-0.0, 1.0]));
        let inputs = [
            (9, first),
            (9, (10, 20, finite_pair([0.0, 1.0]))),
            (9, (10, 21, finite_pair([0.0, 1.0]))),
            (9, (11, 20, finite_pair([0.0, 1.0]))),
            (9, (10, 20, finite_pair([1.0, 0.0]))),
            (7, first),
            (9, first),
        ];
        for (edge, support) in inputs {
            storage.with_storage(|| {
                push_distinct_support(
                    ctx,
                    &mut supports,
                    (&mut index, &mut index_storage),
                    edge,
                    support,
                )
            })?;
        }
        assert_eq!(supports[&9], [first, inputs[2].1, inputs[3].1, inputs[4].1]);
        assert_eq!(supports[&7], [first]);
        assert_eq!(supports[&9][0].2[0].get().to_bits(), (-0.0_f64).to_bits());
        Ok::<_, CodecError>(())
    })
    .expect("support membership budget");
}

#[test]
fn b5_support_index_admits_a_large_population_under_a_linear_work_limit() {
    const COUNT: u32 = 8192;
    // This limit gives the index, geometric backing growth and output appends
    // 1,024 units per distinct support, or 8,388,608 units in total.
    crate::test_support::with_work_limit(u64::from(COUNT) * 1024, |ctx| {
        let mut storage = ctx.reserve_scoped(0, "test_support_plan")?;
        let mut index_storage = ctx.reserve_scoped(0, "test_support_index")?;
        let mut index = B5SupportIndex::new();
        let mut supports = B5SupportPlan::new();
        for id in 0..COUNT {
            storage.with_storage(|| {
                push_distinct_support(
                    ctx,
                    &mut supports,
                    (&mut index, &mut index_storage),
                    9,
                    (id, id + 1, finite_pair([0.0, 1.0])),
                )
            })?;
        }
        let stored = &supports[&9];
        assert_eq!(
            stored.len(),
            usize::try_from(COUNT).expect("bounded fixture count")
        );
        for (support, id) in stored.iter().zip(0..COUNT) {
            assert_eq!(*support, (id, id + 1, finite_pair([0.0, 1.0])));
        }
        Ok::<_, CodecError>(())
    })
    .expect("linear support membership budget");
}

#[test]
fn b5_support_index_refuses_before_membership_lookup_and_insertion() {
    for operation in [
        "catia_b5_edge_support_index",
        "catia_b5_edge_support_membership",
    ] {
        let refused = crate::test_support::with_work_refusal(operation, |ctx| {
            let mut storage = ctx.reserve_scoped(0, "test_support_plan")?;
            let mut index_storage = ctx.reserve_scoped(0, "test_support_index")?;
            let mut index = B5SupportIndex::new();
            let mut supports = B5SupportPlan::new();
            storage.with_storage(|| {
                push_distinct_support(
                    ctx,
                    &mut supports,
                    (&mut index, &mut index_storage),
                    9,
                    (10, 20, finite_pair([0.0, 1.0])),
                )
            })
        });
        assert!(
            matches!(refused, Err(CodecError::ResourceLimit(limit)) if limit.operation == operation)
        );
    }
}

#[test]
fn b5_support_index_storage_is_temporary_and_released_after_its_readers() {
    const MATERIALIZED_LIMIT: u64 = 1 << 20;
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_materialized_bytes = MATERIALIZED_LIMIT;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
    let mut storage = ctx
        .reserve_scoped(0, "test_support_plan")
        .expect("output lease");
    let mut index_storage = ctx
        .reserve_scoped(0, "test_support_index")
        .expect("index lease");
    let mut index = B5SupportIndex::new();
    let mut supports = B5SupportPlan::new();
    for id in 0..1024 {
        storage
            .with_storage(|| {
                push_distinct_support(
                    &ctx,
                    &mut supports,
                    (&mut index, &mut index_storage),
                    9,
                    (id, id + 1, finite_pair([0.0, 1.0])),
                )
            })
            .expect("zero retained budget admits scratch");
    }
    drop(index);
    drop(index_storage);
    assert_eq!(supports[&9].len(), 1024);
    let vector_bytes = cadmpeg_core::decode::u64_from_index(supports[&9].capacity())
        * cadmpeg_core::decode::u64_from_index(std::mem::size_of::<super::super::B5Support>());
    // One output tree node fits within 1 KiB; the vector backing stays live.
    let released = ctx
        .reserve_scoped(
            MATERIALIZED_LIMIT - vector_bytes - 1024,
            "test_index_released_with_supports_alive",
        )
        .expect("index backing is released before the ordered supports");
    drop(released);
    drop(supports);
    drop(storage);
    ctx.reserve_scoped(MATERIALIZED_LIMIT, "test_released_support_storage")
        .expect("all scratch storage is released");
}

#[test]
fn b5_support_index_preserves_general_body_support_populations() {
    const FACE_COUNT: u32 = 64;
    let bytes = crate::test_support::test_b5::b5_closed_triangle_stream();
    let mut graph = crate::test_support::with_service_context(|ctx| {
        crate::families::b5::graph::parse(ctx, &bytes, &mut crate::nurbs::LaneRefusals::new())
    })
    .expect("graph budget")
    .expect("closed triangle");
    let source_face = graph.faces[0].clone();
    let source_loop = graph.loops[&source_face.loops[0]].clone();
    let surface = graph.surfaces[&source_face.surface].clone();
    let mut expected_surfaces = vec![source_face.surface];
    for ordinal in 1..FACE_COUNT {
        let base = 1000 + 10 * ordinal;
        let mut loop_ = source_loop.clone();
        loop_.object_id = base + 4;
        loop_.surface = base;
        for (offset, member) in loop_.members.iter_mut().enumerate() {
            let mut pcurve = graph.pcurves[&member.pcurve].clone();
            pcurve.object_id = base + 1 + u32::try_from(offset).expect("three members");
            pcurve.surface = base;
            member.pcurve = pcurve.object_id;
            graph.pcurves.insert(pcurve.object_id, pcurve);
        }
        let mut face = source_face.clone();
        face.object_id = base + 5;
        face.surface = base;
        face.loops = vec![loop_.object_id];
        graph.faces.push(face);
        graph.loops.insert(loop_.object_id, loop_);
        graph.surfaces.insert(base, surface.clone());
        expected_surfaces.push(base);
    }
    let payload =
        cadmpeg_ir::ids::UnknownId::mint("catia:test:unknown#general-supports".to_owned())
            .expect("identity grammar");
    crate::test_support::with_service_context(|ctx| {
        let plan = super::build_plan(
            ctx,
            &graph,
            &payload,
            &mut crate::nurbs::LaneRefusals::new(),
        )?
        .expect("General body remains admitted");
        assert_eq!(
            plan.ownership.body_kind,
            cadmpeg_ir::topology::BodyKind::General
        );
        assert_eq!(plan.edge_support_plan.len(), 3);
        for supports in plan.edge_support_plan.values() {
            assert_eq!(
                supports.len(),
                usize::try_from(FACE_COUNT).expect("bounded fixture count")
            );
            assert_eq!(
                supports.iter().map(|support| support.0).collect::<Vec<_>>(),
                expected_surfaces
            );
        }
        Ok::<_, CodecError>(())
    })
    .expect("General support planning budget");
}
