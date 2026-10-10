// SPDX-License-Identifier: Apache-2.0

use crate::parameter::{ParameterRecord, Token, TokenValue};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use cadmpeg_ir::geometry::analytic::PlaneSurface;
use cadmpeg_ir::geometry::{SolvedSurfaceGeometry, SurfaceGeometry};
use cadmpeg_ir::ids::SurfaceId;
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::CadIr;
use std::collections::BTreeMap;

fn definition_boundary(
    entity_type: i64,
    values: &[i64],
    operation: &'static str,
    preceding_items: u64,
    reason: &str,
) {
    let mut entry = crate::test_support::directory_target(1, entity_type);
    entry.form = 1;
    let directory = [entry];
    let record = ParameterRecord::from_test_tokens(
        1,
        1..2,
        Vec::new(),
        values.len(),
        values
            .iter()
            .map(|value| Token {
                value: TokenValue::Integer(*value),
                span: 0..0,
            })
            .collect(),
        Vec::new(),
    );
    let entries = BTreeMap::from([(1, &directory[0])]);
    let records = BTreeMap::from([(1, &record)]);
    let bytes = crate::test_support::test_owned::owned_test_file(&[]);
    let global = crate::test_support::with_service_context(&bytes, |setup| {
        let scan = crate::card::scan_with_context(&bytes, setup).unwrap();
        let (global, _, _) = crate::global::parse(&scan, setup).unwrap();
        global.length_context().unwrap()
    });
    // The selected production vector has two slots. Its malformed first tuple
    // then requires one diagnostic slot, without any definition-map insertion.
    // Two following tokens admit the count even when the first tuple is invalid.
    let vector_need = preceding_items + 2;
    for cap in [vector_need - 1, vector_need, vector_need + 1] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = cap;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let mut ir = CadIr::empty();
        let mut sequences = super::super::super::geometry::SourceSequences::default();
        let result = super::super::project(
            &mut ir,
            &directory,
            (&entries, &records),
            &global,
            &ctx,
            &mut sequences,
        );
        if cap <= vector_need {
            let first = match result.as_ref() {
                Err(CodecError::ResourceLimit(first)) => *first,
                _ => panic!("expected source vector or diagnostic-slot refusal"),
            };
            drop(result);
            assert_eq!(first.dimension, ResourceDimension::CollectionItems);
            assert_eq!(first.limit, cap);
            if cap < vector_need {
                assert_eq!(first.operation, operation);
                assert_eq!((first.used, first.additional), (preceding_items, 2));
            } else {
                assert_eq!(first.operation, "iges entity loss slots");
                assert_eq!((first.used, first.additional), (vector_need, 1));
            }
            let replay = super::super::project(
                &mut ir,
                &directory,
                (&entries, &records),
                &global,
                &ctx,
                &mut sequences,
            );
            assert!(
                matches!(replay.as_ref(), Err(CodecError::ResourceLimit(last)) if *last == first)
            );
            drop(replay);
            assert!(
                matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(last)) if last == first)
            );
        } else {
            let outcome = result.unwrap();
            assert!(outcome.decoded.is_empty());
            assert_eq!(outcome.losses.len(), 1);
            assert_eq!(
                outcome.losses[0].message,
                format!("IGES entity type {entity_type} form 1 was not projected: {reason}")
            );
            drop(outcome);
            ctx.finish_session().unwrap();
        }
        assert!(ir.model.points.is_empty());
        assert!(ir.model.edges.is_empty());
        assert!(ir.model.bodies.is_empty());
    }
}

#[test]
fn vertex_list_source_count_refuses_before_coordinates() {
    definition_boundary(
        502,
        &[502, 2, 0, 0],
        "iges B-rep vertex-list points",
        0,
        "vertex-list coordinates are truncated or non-finite",
    );
}

#[test]
fn edge_list_source_count_refuses_before_edge_tuples() {
    definition_boundary(
        504,
        &[504, 2, 0, 0],
        "iges B-rep edge-list edges",
        0,
        "edge-list tuple is invalid or names a missing vertex",
    );
}

#[test]
fn loop_source_count_refuses_before_uses() {
    definition_boundary(
        508,
        &[508, 2, 0, 0],
        "iges B-rep loop uses",
        0,
        "loop edge-use tuple is invalid",
    );
}

#[test]
fn nested_pcurve_source_count_refuses_after_its_loop_slot() {
    definition_boundary(
        508,
        &[508, 1, 1, 1, 1, 0, 2, 2, 0],
        "iges B-rep use pcurves",
        1,
        "loop edge-use tuple is invalid",
    );
}

#[test]
fn shell_source_count_refuses_before_face_uses() {
    definition_boundary(
        514,
        &[514, 2, 0, 0],
        "iges B-rep shell face uses",
        0,
        "shell face-use tuple is invalid",
    );
}

fn resolution_boundary(operation: &'static str, preceding_items: u64) {
    let source = CadIr::empty();
    let id = SurfaceId::mint("iges:model:surface#D1").unwrap();
    let geometry = SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
        PlaneSurface::try_new(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
        )
        .unwrap(),
    ));
    let support = super::super::SurfaceSupport {
        id: &id,
        geometry: &geometry,
        factor: 1.0,
    };
    let endpoints = super::super::PcurveEndpointCheck {
        start: Point3::new(0.0, 0.0, 0.0),
        end: Point3::new(0.0, 0.0, 0.0),
        tolerance: 0.0,
    };
    let uses = [(false, 3), (false, 5)];
    for cap in [preceding_items + 1, 4] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = cap;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let mut composite_storage = ctx
            .reserve_scoped(0, "test actual composite index")
            .unwrap();
        let mut composite_index = None;
        let mut model_index = None;
        let result = super::super::resolve_pcurve_uses(
            &source,
            &uses,
            &support,
            endpoints,
            &ctx,
            (
                &mut model_index,
                (&mut composite_index, &mut composite_storage),
            ),
        )
        .map(|resolved| resolved.map(drop));
        let refusal = if cap < 4 {
            let first = match result {
                Err(super::super::super::composite::CompositeCurveError::Budget(
                    CodecError::ResourceLimit(first),
                )) => first,
                _ => panic!("expected actual resolved or mapped pcurve allocation refusal"),
            };
            assert_eq!(first.dimension, ResourceDimension::CollectionItems);
            assert_eq!(first.operation, operation);
            assert_eq!(
                (first.limit, first.used, first.additional),
                (cap, preceding_items, 2)
            );
            for replay_uses in [&uses[..], &[][..]] {
                let replay = super::super::resolve_pcurve_uses(
                    &source,
                    replay_uses,
                    &support,
                    endpoints,
                    &ctx,
                    (
                        &mut model_index,
                        (&mut composite_index, &mut composite_storage),
                    ),
                );
                assert!(
                    matches!(replay, Err(super::super::super::composite::CompositeCurveError::Budget(
                    CodecError::ResourceLimit(last))) if last == first)
                );
            }
            Some(first)
        } else {
            // Both real two-slot vectors fit. The first absent source curve
            // returns unavailable support rather than fabricating geometry.
            assert!(result.unwrap().is_none());
            let empty = super::super::resolve_pcurve_uses(
                &source,
                &[],
                &support,
                endpoints,
                &ctx,
                (
                    &mut model_index,
                    (&mut composite_index, &mut composite_storage),
                ),
            )
            .unwrap()
            .unwrap();
            assert!(empty.0.is_empty());
            drop(empty);
            None
        };
        drop(model_index);
        drop(composite_index);
        drop(composite_storage);
        if let Some(first) = refusal {
            assert!(
                matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(last)) if last == first)
            );
        } else {
            ctx.finish_session().unwrap();
        }
    }
}

#[test]
fn resolved_pcurve_source_count_refuses_before_resolution() {
    resolution_boundary("iges B-rep resolved pcurves", 0);
}

#[test]
fn mapped_pcurve_source_count_refuses_after_resolved_slots() {
    resolution_boundary("iges B-rep mapped pcurves", 2);
}
