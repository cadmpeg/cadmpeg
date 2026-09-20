// SPDX-License-Identifier: Apache-2.0
//! Unit tests for the writable brep model the encoder builds from CADIR.

use super::{adjacent_quad_sheet, polygon_sheet};
use crate::writer::model::{WritableFaceSurface, WritableModel};
use cadmpeg_ir::math::Point3;

#[test]
fn single_face_resolves_arena_permutations_in_traversal_order() {
    let mut ir = polygon_sheet(&[
        Point3::new(0.0, 0.0, 0.0),
        Point3::new(2.0, 0.0, 0.0),
        Point3::new(0.0, 2.0, 0.0),
    ]);
    let expected = super::super::brep_payload(
        &WritableModel::try_new(&ir).expect("writable triangle"),
        crate::RhinoArchiveVersion::V8,
    )
    .expect("triangle payload");
    ir.model.vertices.reverse();
    ir.model.edges.reverse();
    ir.model.coedges.reverse();
    let model =
        WritableModel::try_new(&ir).expect("references resolve independently of arena order");
    let actual = super::super::brep_payload(&model, crate::RhinoArchiveVersion::V8)
        .expect("permuted triangle payload");
    assert_eq!(actual.body, expected.body);
    assert_eq!(actual.direct, expected.direct);
    assert_eq!(model.loops[0].coedges, vec![0, 1, 2]);
    for (position, edge) in model.edges.iter().enumerate() {
        assert_eq!(edge.start, position);
        assert_eq!(edge.end, (position + 1) % 3);
        assert_eq!(edge.uses, vec![position]);
    }
}

#[test]
fn multi_face_resolves_domains_and_incidence_in_arena_order() {
    let ir = adjacent_quad_sheet();
    let model = WritableModel::try_new(&ir).expect("writable adjacent faces");
    for (position, edge) in model.edges.iter().enumerate() {
        assert_eq!(edge.source.id, ir.model.edges[position].id);
        assert_eq!(Some(edge.domain), ir.model.edges[position].param_range());
        for coedge in &edge.uses {
            assert_eq!(model.coedges[*coedge].edge, position);
        }
    }
    for face in &model.faces {
        assert!(matches!(
            model.surfaces[face.surface],
            WritableFaceSurface::Plane { .. }
        ));
    }
}
