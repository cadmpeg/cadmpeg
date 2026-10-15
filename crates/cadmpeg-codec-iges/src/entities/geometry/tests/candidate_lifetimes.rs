// SPDX-License-Identifier: Apache-2.0

use crate::parameter::{ParameterRecord, Token, TokenValue};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use cadmpeg_ir::features::FinitePoint3;
use cadmpeg_ir::geometry::nurbs::WeightedPole3;
use std::collections::BTreeMap;
use std::mem::size_of;

fn record(count: usize, polynomial: bool, closed: bool) -> ParameterRecord {
    let mut values = vec![
        126,
        i64::try_from(count - 1).unwrap(),
        1,
        1,
        i64::from(closed),
        i64::from(polynomial),
        0,
    ];
    values.extend([0, 0]);
    values.extend((1..count).map(|index| i64::try_from(index).unwrap()));
    values.push(i64::try_from(count - 1).unwrap());
    values.extend((0..count).map(|index| {
        if polynomial {
            1
        } else {
            i64::try_from(1 + index % 2).unwrap()
        }
    }));
    for index in 0..count {
        values.extend([i64::try_from(index).unwrap(), 0, 0]);
    }
    values.extend([0, i64::try_from(count - 1).unwrap(), 0, 0, 1]);
    ParameterRecord::from_test_tokens(
        1,
        1..2,
        Vec::new(),
        values.len(),
        values
            .into_iter()
            .map(|value| Token {
                value: TokenValue::Integer(value),
                span: 0..0,
            })
            .collect(),
        Vec::new(),
    )
}

#[test]
fn rejected_nurbs_candidates_retain_no_knot_or_pole_buffers() {
    let (_, global) = crate::test_support::sequence_index::parameter_inputs(0);
    let mut entry = crate::test_support::directory_target(1, 126);
    entry.form = 0;
    let mut retained = Vec::new();
    for polynomial in [true, false] {
        for count in [2, 512] {
            let record = record(count, polynomial, true);
            let error = cadmpeg_test_support::refusal::resource_limit_at(
                ResourceDimension::RetainedBytes,
                "test retained NURBS output total",
                |cap| {
                    let arena = DecodeArena::new();
                    let mut policy = DecodePolicy::service();
                    policy.limits.max_materialized_bytes = 1024 * 1024;
                    policy.limits.max_retained_bytes = cap;
                    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
                    let mut ir = cadmpeg_ir::CadIr::empty();
                    let projection = super::super::project_geometry(
                        &mut ir,
                        std::slice::from_ref(&entry),
                        std::slice::from_ref(&record),
                        &BTreeMap::new(),
                        &global,
                        &ctx,
                    )
                    .unwrap();
                    assert!(projection.decoded.is_empty());
                    assert!(ir.model.curves.is_empty());
                    assert_eq!(projection.losses.len(), 1);
                    assert_eq!(projection.losses[0].message,
                    "IGES entity type 126 form 0 was not projected: closed spline flag disagrees with evaluated endpoints");
                    drop(projection);
                    let released = ctx
                        .reserve_scoped(
                            policy.limits.max_materialized_bytes,
                            "test rejected NURBS scratch released",
                        )
                        .unwrap();
                    drop(released);
                    let limit = match ctx.charge_retained(1, "test retained NURBS output total") {
                        Err(CodecError::ResourceLimit(limit)) => limit,
                        Err(error) => return Err(error),
                        Ok(()) => return ctx.finish_session(),
                    };
                    assert_eq!(limit.dimension, ResourceDimension::RetainedBytes);
                    assert_eq!(limit.operation, "test retained NURBS output total");
                    assert_eq!(limit.additional, 1);
                    assert!(
                        matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(last)) if last == limit)
                    );
                    Err::<(), _>(CodecError::ResourceLimit(limit))
                },
            );
            let CodecError::ResourceLimit(limit) = error else {
                panic!("expected retained accounting refusal");
            };
            retained.push(limit.used);
        }
    }
    assert!(
        retained.iter().all(|bytes| *bytes == retained[0]),
        "only the fixed loss text and slots survive: {retained:?}"
    );
}

#[test]
fn accepted_nurbs_candidate_promotes_exact_output_knot_and_pole_storage() {
    let (_, global) = crate::test_support::sequence_index::parameter_inputs(0);
    let mut entry = crate::test_support::directory_target(1, 126);
    entry.form = 0;
    for polynomial in [true, false] {
        let count = 64_usize;
        let record = record(count, polynomial, false);
        let expected = u64::try_from(
            (count + 2) * size_of::<f64>()
                + count
                    * if polynomial {
                        size_of::<FinitePoint3>()
                    } else {
                        size_of::<WeightedPole3<FinitePoint3>>()
                    },
        )
        .unwrap();
        let error = cadmpeg_test_support::refusal::resource_limit_at(
            ResourceDimension::RetainedBytes,
            "iges NURBS candidate storage",
            |cap| {
                let arena = DecodeArena::new();
                let mut policy = DecodePolicy::service();
                policy.limits.max_retained_bytes = cap;
                let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
                let mut ir = cadmpeg_ir::CadIr::empty();
                let projection = super::super::project_geometry(
                    &mut ir,
                    std::slice::from_ref(&entry),
                    std::slice::from_ref(&record),
                    &BTreeMap::new(),
                    &global,
                    &ctx,
                )?;
                drop(projection);
                ctx.finish_session()
            },
        );
        assert!(matches!(error, CodecError::ResourceLimit(limit)
            if limit.operation == "iges NURBS candidate storage" && limit.additional == expected));
        crate::test_support::with_service_context(&[], |ctx| {
            let mut ir = cadmpeg_ir::CadIr::empty();
            let projection = super::super::project_geometry(
                &mut ir,
                std::slice::from_ref(&entry),
                std::slice::from_ref(&record),
                &BTreeMap::new(),
                &global,
                ctx,
            )
            .unwrap();
            assert!(projection.losses.is_empty());
            assert!(projection.decoded.contains(&1));
            assert_eq!(ir.model.curves.len(), 1);
            let Some(cadmpeg_ir::geometry::SolvedCurveGeometry::Nurbs(curve)) =
                ir.model.curves[0].geometry.solved()
            else {
                panic!("expected admitted NURBS carrier");
            };
            assert_eq!(curve.pole_count(), count);
            assert_eq!(curve.knots().len(), count + 2);
        });
    }
}
