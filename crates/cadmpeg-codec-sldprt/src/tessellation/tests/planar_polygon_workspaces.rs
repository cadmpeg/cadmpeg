// SPDX-License-Identifier: Apache-2.0
//! A rejected planar destination prefix expires before the next face attempt.

use crate::tessellation::{analytic_trim, TrimTopology};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_ir::math::Point2;

#[test]
fn repeated_rejected_planar_prefixes_release_direct_and_parent_scoped_storage() {
    let mut model = super::model_with_body();
    let id = super::add_square_face(&mut model, "rejected-prefix", 0.0);
    // Three connected line coedges build a prefix. The fourth starts at the
    // first vertex rather than the preceding coedge's end.
    let first = model.edges[0].start.clone();
    model.edges[3].start = first;
    let face = model.faces.iter().find(|face| face.id == id).unwrap();
    let surface = &model
        .surfaces
        .iter()
        .find(|surface| surface.id == face.surface)
        .unwrap()
        .geometry;
    let loops = model.loops.iter().map(|value| (&value.id, value)).collect();
    let coedges = model
        .coedges
        .iter()
        .map(|value| (&value.id, value))
        .collect();
    let edges = model.edges.iter().map(|value| (&value.id, value)).collect();
    let vertices = model
        .vertices
        .iter()
        .map(|value| (&value.id, value))
        .collect();
    let points = model
        .points
        .iter()
        .map(|value| (&value.id, value.position().get()))
        .collect();
    let curves = model
        .curves
        .iter()
        .map(|value| (&value.id, &value.geometry))
        .collect();
    let topology = TrimTopology {
        loops: &loops,
        coedges: &coedges,
        edges: &edges,
        vertices: &vertices,
        points: &points,
        curves: &curves,
        coordinate_scale: 1.0,
    };
    for parent_scoped in [false, true] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = 0;
        // Four destination slots coexist with one exact source sample slot.
        let peak = u64::try_from(5 * std::mem::size_of::<Point2>()).unwrap();
        policy.limits.max_materialized_bytes = peak;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let mut parent = ctx
            .reserve_scoped(0, "test planar candidate parent")
            .unwrap();
        for _ in 0..128 {
            let trim = if parent_scoped {
                parent.with_storage(|| analytic_trim(&ctx, face, surface, &topology))
            } else {
                analytic_trim(&ctx, face, surface, &topology)
            }
            .unwrap();
            assert!(trim.is_none());
            drop(
                ctx.reserve_scoped(peak, "test rejected planar prefix expiry")
                    .unwrap(),
            );
        }
        drop(parent);
        ctx.finish_session().unwrap();
    }
}
