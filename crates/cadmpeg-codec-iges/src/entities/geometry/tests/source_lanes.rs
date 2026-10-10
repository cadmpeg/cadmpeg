// SPDX-License-Identifier: Apache-2.0

use crate::parameter::{ParameterRecord, Token, TokenValue};
use cadmpeg_core::decode::{u64_from_index, DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use cadmpeg_ir::features::FinitePoint3;
use cadmpeg_ir::geometry::nurbs::WeightedPole3;
use cadmpeg_ir::scalar::{FiniteReal, PositiveReal};
use std::mem::{align_of, size_of};

#[test]
fn rational_nurbs_source_lanes_release_consumed_buffers_at_their_phase_boundary() {
    const CONTROLS: usize = 512;
    let mut entry = crate::test_support::directory_target(1, 126);
    entry.form = 0;
    let directory = [entry];
    let mut values = vec![126, i64::try_from(CONTROLS - 1).unwrap(), 1, 1, 0, 0, 0];
    // Degree-one clamped knots: 0, 0, 1, ..., 511, 511.
    values.extend([0, 0]);
    values.extend((1..CONTROLS).map(|index| i64::try_from(index).unwrap()));
    values.push(i64::try_from(CONTROLS - 1).unwrap());
    values.extend((0..CONTROLS).map(|index| i64::try_from(1 + index % 2).unwrap()));
    for index in 0..CONTROLS {
        values.extend([i64::try_from(index).unwrap(), 0, 0]);
    }
    values.extend([0, i64::try_from(CONTROLS - 1).unwrap(), 0, 0, 1]);
    let record = ParameterRecord::from_test_tokens(
        1, 1..2, Vec::new(), values.len(),
        values.into_iter().map(|value| Token {
            value: TokenValue::Integer(value), span: 0..0,
        }).collect(), Vec::new(),
    );
    let bytes = crate::test_support::test_owned::owned_test_file(&[]);
    let global = crate::test_support::with_service_context(&bytes, |setup| {
        let scan = crate::card::scan_with_context(&bytes, setup).unwrap();
        let (global, _, _) = crate::global::parse(&scan, setup).unwrap();
        global.length_context().unwrap()
    });
    // Two u32 -> borrowed-record tree nodes precede the lane allocations.
    let node = 11 * (size_of::<u32>() + size_of::<&ParameterRecord>())
        + 16 * size_of::<usize>()
        + 2 * align_of::<u32>().max(align_of::<&ParameterRecord>()).max(align_of::<usize>());
    let lookup = 2 * node;
    // Core amortized growth doubles from four slots. The 1,536 scalar
    // coordinates occupy 2,048 slots. At the final control-vector growth,
    // its new 512-slot buffer overlaps its old 256-slot buffer. Consumed
    // source knot and finite-weight vectors are already dead at this phase.
    // Admitted output knots stay live until the curve is accepted.
    let scalar_capacity = (3 * CONTROLS).next_power_of_two();
    let poles = scalar_capacity * size_of::<FiniteReal>();
    let weights = CONTROLS * size_of::<PositiveReal>();
    let controls = CONTROLS * size_of::<FinitePoint3>();
    let overlap = (CONTROLS / 2) * size_of::<FinitePoint3>();
    let knots = (CONTROLS + 2) * size_of::<f64>();
    let used = u64_from_index(lookup + knots + poles + weights + controls);
    let peak = used + u64_from_index(overlap);
    let paired = CONTROLS * size_of::<WeightedPole3<FinitePoint3>>();
    let paired_overlap = (CONTROLS / 2) * size_of::<WeightedPole3<FinitePoint3>>();
    let success_peak = peak.max(u64_from_index(
        lookup + knots + controls + weights + paired + paired_overlap,
    ));
    for cap in [peak - 1, success_peak] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_materialized_bytes = cap;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let mut ir = cadmpeg_ir::CadIr::empty();
        let result = super::super::project_geometry(
            &mut ir, &directory, std::slice::from_ref(&record),
            &std::collections::BTreeMap::new(), &global, &ctx,
        );
        if cap < peak {
            let first = match result.as_ref() {
                Err(CodecError::ResourceLimit(first)) => *first,
                _ => panic!("expected actual control-buffer overlap refusal"),
            };
            drop(result);
            assert_eq!(first.dimension, ResourceDimension::MaterializedBytes);
            assert_eq!(first.operation, "iges NURBS placed controls");
            assert_eq!((first.limit, first.used, first.additional), (cap, used, u64_from_index(overlap)));
            assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(last)) if last == first));
        } else {
            let projection = result.unwrap();
            assert!(projection.losses.is_empty());
            assert!(projection.decoded.contains(&1));
            assert_eq!(ir.model.curves.len(), 1);
            let cadmpeg_ir::geometry::CurveGeometry::Solved(cadmpeg_ir::geometry::SolvedCurveGeometry::Nurbs(curve)) = &ir.model.curves[0].geometry else {
                panic!("expected the rational spline");
            };
            assert_eq!(curve.pole_count(), CONTROLS);
            assert_eq!(curve.pole_rows().weights().unwrap(),
                (0..CONTROLS).map(|index| f64::from(u32::try_from(1 + index % 2).unwrap())).collect::<Vec<_>>());
            assert_eq!(ir.model.curves[0].id.as_str(), "iges:model:curve#D1");
            drop(projection);
            ctx.finish_session().unwrap();
        }
    }
}
