// SPDX-License-Identifier: Apache-2.0

use crate::document::CadIr;
use crate::math::Point3;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

#[test]
fn geometric_checks_reuse_the_prepared_index_without_copying_native_identities() {
    let mut ir = CadIr::empty();
    ir.native.namespace_mut("test").arenas_mut().insert(
        "records".into(),
        (0..1_000)
            .map(|ordinal| {
                crate::NativeRecord::new(
                    crate::ids::Identity::new(format!("test:native:record#{ordinal}")).unwrap(),
                    serde_json::Map::new(),
                )
                .unwrap()
            })
            .collect(),
    );
    let index = crate::index::ModelIndex::build(&ir);
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut findings = Vec::new();
    super::super::check_pcurve_surface_consistency(&ctx, &index, &mut findings).unwrap();
    super::super::check_procedural_support_consistency(&ctx, &index, &mut findings).unwrap();
    assert!(findings.is_empty());
    ctx.finish_session().unwrap();
}

#[test]
fn geometric_vertex_indexes_keep_last_duplicate_and_skip_missing_points() {
    let mut ir = crate::examples::unit_cube().unwrap();
    let vertex = ir.model.vertices[0].clone();
    let mut point = ir
        .model
        .points
        .iter()
        .find(|point| point.id == vertex.point)
        .unwrap()
        .clone();
    point.set_position(crate::features::FinitePoint3::new(Point3::new(7.0, 8.0, 9.0)).unwrap());
    ir.model.points.push(point);
    let mut missing = vertex.clone();
    missing.point = "test:model:point#missing".try_into().unwrap();
    ir.model.vertices.push(missing);
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    {
        let positions = super::super::vertex_positions(&ir).unwrap();
        assert_eq!(
            positions.get(vertex.id.as_str()),
            Some(&(
                Point3::new(7.0, 8.0, 9.0),
                vertex.tolerance.map(crate::scalar::PositiveReal::get)
            ))
        );
    }
    ctx.finish_session().unwrap();
}
