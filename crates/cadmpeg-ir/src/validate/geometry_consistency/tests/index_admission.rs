// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use crate::document::CadIr;
use crate::math::Point3;
use crate::report::check::Finding;

fn refuses(ir: &CadIr, checker: impl Fn(&DecodeContext<'_>, &CadIr, &mut Vec<Finding>) -> Result<(), CodecError>) {
    for dimension in [ResourceDimension::MaterializedBytes, ResourceDimension::CollectionItems, ResourceDimension::WorkUnits] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        match dimension {
            ResourceDimension::MaterializedBytes => policy.limits.max_materialized_bytes = 0,
            ResourceDimension::CollectionItems => policy.limits.max_collection_items = 0,
            ResourceDimension::WorkUnits => policy.limits.max_work_units = 0,
            _ => panic!("test dimension"),
        }
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let mut findings = Vec::new();
        let Err(CodecError::ResourceLimit(limit)) = checker(&ctx, ir, &mut findings) else { panic!("index must refuse"); };
        assert_eq!(limit.dimension, dimension);
        assert!(findings.is_empty());
        assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(original)) if original == limit));
    }
}

#[test]
fn geometric_endpoint_indexes_preserve_original_refusals() {
    refuses(&crate::examples::unit_cube().unwrap(), super::super::check_edge_endpoint_consistency);
}

#[test]
fn geometric_pcurve_indexes_preserve_original_refusals() {
    refuses(&super::untrimmed_surface_curve(), super::super::check_pcurve_surface_consistency);
}

#[test]
fn geometric_procedural_indexes_preserve_original_refusals() {
    refuses(&super::mapped_surface_curve([1.0, 0.0]), super::super::check_procedural_support_consistency);
}

#[test]
fn geometric_vertex_indexes_keep_last_duplicate_and_skip_missing_points() {
    let mut ir = crate::examples::unit_cube().unwrap();
    let vertex = ir.model.vertices[0].clone();
    let mut point = ir.model.points.iter().find(|point| point.id == vertex.point).unwrap().clone();
    point.set_position(crate::features::FinitePoint3::new(Point3::new(7.0, 8.0, 9.0)).unwrap());
    ir.model.points.push(point);
    let mut missing = vertex.clone();
    missing.point = "test:model:point#missing".try_into().unwrap();
    ir.model.vertices.push(missing);
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_materialized_bytes = 65536;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    {
        let positions = super::super::vertex_positions(&ctx, &ir).unwrap();
        assert_eq!(positions.get(&ctx, vertex.id.as_str()).unwrap(), Some(&(Point3::new(7.0, 8.0, 9.0), vertex.tolerance.map(crate::scalar::PositiveReal::get))));
    }
    drop(ctx.reserve_scoped(65536, "geometric vertex scopes released").unwrap());
    ctx.finish_session().unwrap();
}

#[test]
fn geometric_endpoint_indexes_release_all_storage_with_zero_retained_budget() {
    let ir = crate::examples::unit_cube().unwrap();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_materialized_bytes = 65536;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut findings = Vec::new();
    super::super::check_edge_endpoint_consistency(&ctx, &ir, &mut findings).unwrap();
    assert!(findings.is_empty());
    drop(ctx.reserve_scoped(65536, "geometric endpoint scopes released").unwrap());
    ctx.finish_session().unwrap();
}
