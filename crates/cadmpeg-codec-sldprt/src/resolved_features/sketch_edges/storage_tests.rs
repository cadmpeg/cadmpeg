// SPDX-License-Identifier: Apache-2.0
//! Storage boundaries for projected NURBS publication.

use super::{project_edge, SketchPlaneFrame};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use cadmpeg_ir::geometry::nurbs::NurbsCurve;
use cadmpeg_ir::geometry::pcurve::WeightedPole2;
use cadmpeg_ir::geometry::{CurveGeometry, SolvedCurveGeometry};
use cadmpeg_ir::ids::{CurveId, EdgeId, PointId, VertexId};
use cadmpeg_ir::math::{Point2, Point3, Vector3};
use cadmpeg_ir::scalar::NonZeroReal;
use cadmpeg_ir::sketches::SketchGeometryDefinition;
use cadmpeg_ir::topology::{Edge, EdgeCarrier};
use cadmpeg_ir::units::FinitePoint2;
use std::collections::HashMap;
use std::mem::size_of;

#[test]
fn projected_nurbs_value_handoff_preserves_exact_storage_and_refusal() {
    let curve_id = CurveId::mint("test:model:curve#projection").unwrap();
    let start_vertex = VertexId::mint("test:model:vertex#start").unwrap();
    let end_vertex = VertexId::mint("test:model:vertex#end").unwrap();
    let start_point = PointId::mint("test:model:point#start").unwrap();
    let end_point = PointId::mint("test:model:point#end").unwrap();
    let edge = Edge {
        id: EdgeId::mint("test:model:edge#projection").unwrap(),
        carrier: EdgeCarrier::unbounded(Some(curve_id.clone())),
        start: start_vertex.clone(),
        end: end_vertex.clone(),
        tolerance: None,
    };
    let source_points = [Point3::new(0.0, 0.0, 0.0), Point3::new(1.0, 2.0, 0.0)];
    let knots = [0.0, 0.0, 1.0, 1.0];
    let vertices = HashMap::from([(&start_vertex, &start_point), (&end_vertex, &end_point)]);
    let points = HashMap::from([
        (&start_point, source_points[0]),
        (&end_point, source_points[1]),
    ]);
    for weights in [None, Some(vec![2.0, 3.0])] {
        let rational = weights.is_some();
        let pole_size = if rational {
            size_of::<WeightedPole2<FinitePoint2>>()
        } else {
            size_of::<FinitePoint2>()
        };
        let knot_bytes = knots.len() * size_of::<f64>();
        let output_pole_bytes = source_points.len() * pole_size;
        let retained_bytes = knot_bytes + output_pole_bytes;
        // Indexed raw lanes and converted output use exact reservations. Raw rational
        // pairing starts with four amortized slots; its lease lasts through conversion.
        let raw_bytes = source_points.len() * size_of::<Point2>()
            + if rational { source_points.len() * size_of::<NonZeroReal>() } else { 0 };
        let paired_bytes = if rational { 4 * size_of::<WeightedPole2<Point2>>() } else { 0 };
        let materialized_bytes = knot_bytes + raw_bytes + paired_bytes + output_pole_bytes;
        let geometry = CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(
            NurbsCurve::from_lanes(
                &cadmpeg_test_support::service_decode_context(),
                1,
                knots.to_vec(),
                source_points.to_vec(),
                weights.clone(),
                false,
            )
            .unwrap()
            .unwrap(),
        ));
        let curves = HashMap::from([(&curve_id, &geometry)]);
        for boundary in [None, Some(ResourceDimension::RetainedBytes), Some(ResourceDimension::MaterializedBytes)] {
            let mut policy = DecodePolicy::service();
            policy.limits.max_retained_bytes = u64::try_from(retained_bytes).unwrap()
                - u64::from(boundary == Some(ResourceDimension::RetainedBytes));
            policy.limits.max_materialized_bytes = u64::try_from(materialized_bytes).unwrap()
                - u64::from(boundary == Some(ResourceDimension::MaterializedBytes));
            let arena = DecodeArena::new();
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
            let mut refusals = crate::lane_refusal::LaneRefusals::new();
            let output = project_edge(
                &ctx,
                &edge,
                &vertices,
                &points,
                &curves,
                SketchPlaneFrame {
                    origin: Point3::new(0.0, 0.0, 0.0),
                    u_axis: Vector3::new(1.0, 0.0, 0.0),
                    v_axis: Vector3::new(0.0, 1.0, 0.0),
                },
                &mut refusals,
            );
            if let Some(dimension) = boundary {
                let CodecError::ResourceLimit(limit) = output.unwrap_err() else {
                    panic!("one byte below the source-derived boundary must refuse");
                };
                assert_eq!(limit.dimension, dimension);
                if dimension == ResourceDimension::RetainedBytes {
                    assert_eq!(limit.operation, "project SLDPRT sketch NURBS edge");
                    assert_eq!(limit.used, 0);
                    assert_eq!(limit.additional, u64::try_from(retained_bytes).unwrap());
                } else {
                    assert_eq!(limit.operation, "IR pcurve admitted poles");
                    assert_eq!(limit.used, u64::try_from(materialized_bytes - output_pole_bytes).unwrap());
                    assert_eq!(limit.additional, u64::try_from(output_pole_bytes).unwrap());
                }
                assert_eq!(ctx.resource_refusal().as_ref(), Some(&limit));
                let CodecError::ResourceLimit(replayed) =
                    ctx.reserve_scoped(0, "replay projected NURBS refusal").unwrap_err()
                else {
                    panic!("the original resource refusal remains sticky");
                };
                assert_eq!(replayed, limit);
            } else {
                let output = output.unwrap().unwrap();
                let SketchGeometryDefinition::Nurbs { curve } = output.definition() else {
                    panic!("projection preserves the NURBS carrier");
                };
                assert_eq!(curve.knots().as_slice(), knots);
                assert_eq!(curve.pole_rows().raw_points(), [Point2::new(0.0, 0.0), Point2::new(1.0, 2.0)]);
                assert_eq!(curve.pole_rows().weights(), weights);
                assert!(refusals.take_records().is_empty());
                let storage = ctx.reserve_scoped(u64::try_from(materialized_bytes).unwrap(), "reuse projected NURBS scratch").unwrap();
                drop(storage);
                let CodecError::ResourceLimit(limit) = ctx.copy_retained_text("x", "projected NURBS output remains retained").unwrap_err() else {
                    panic!("published output exhausts its exact retained allowance");
                };
                assert_eq!(limit.dimension, ResourceDimension::RetainedBytes);
                assert_eq!(limit.used, u64::try_from(retained_bytes).unwrap());
                assert_eq!(limit.additional, 1);
            }
        }
    }
}
