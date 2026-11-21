// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use cadmpeg_core::CodecError;
use cadmpeg_ir::topology::Sense;
use cadmpeg_ir::CadIr;

use super::reversed_edges;

/// Every edge of the cube has one forward and one reversed coedge.
fn cube() -> CadIr {
    cadmpeg_ir::examples::unit_cube().unwrap()
}

/// Make every coedge of edge `index` run against it.
fn reverse_all_coedges(ir: &mut CadIr, index: usize) {
    let edge = ir.geometry.edges[index].id.clone();
    for coedge in &mut ir.geometry.coedges {
        if coedge.edge == edge {
            coedge.sense = Sense::Reversed;
        }
    }
}

#[test]
fn an_edge_with_a_forward_coedge_is_stored_along_its_curve() {
    let reversed = reversed_edges(&cube()).unwrap();
    assert!(reversed.edges.is_empty());
    assert!(reversed.curves.is_empty());
}

#[test]
fn an_edge_whose_coedges_all_run_against_it_is_stored_reversed() {
    let mut ir = cube();
    reverse_all_coedges(&mut ir, 0);
    let reversed = reversed_edges(&ir).unwrap();
    let edge = &ir.geometry.edges[0];
    assert_eq!(reversed.edges.len(), 1);
    assert!(reversed.edges.contains(&edge.id));
    assert_eq!(reversed.curves.len(), 1);
    assert!(reversed.curves.contains(edge.curve().unwrap()));
}

#[test]
fn stored_sense_flips_only_the_coedges_of_reversed_edges() {
    let mut ir = cube();
    reverse_all_coedges(&mut ir, 0);
    let reversed = reversed_edges(&ir).unwrap();
    let flipped = &ir.geometry.edges[0].id;
    let kept = &ir.geometry.edges[1].id;
    assert_eq!(
        reversed.stored_sense(flipped, Sense::Reversed),
        Sense::Forward
    );
    assert_eq!(
        reversed.stored_sense(flipped, Sense::Forward),
        Sense::Reversed
    );
    assert_eq!(reversed.stored_sense(kept, Sense::Forward), Sense::Forward);
    assert_eq!(
        reversed.stored_sense(kept, Sense::Reversed),
        Sense::Reversed
    );
}

#[test]
fn a_curve_with_edges_stored_in_both_directions_is_refused() {
    let mut ir = cube();
    reverse_all_coedges(&mut ir, 0);
    let shared = ir.geometry.edges[0].curve().cloned();
    ir.geometry.edges[1].set_curve(shared).unwrap();
    let error = reversed_edges(&ir).err().unwrap();
    assert!(
        matches!(&error, CodecError::NotImplemented(message)
            if message.ends_with("carries edges stored in both directions")),
        "{error:?}"
    );
}

/// Put edge `index` on a NURBS curve.
fn on_nurbs(ir: &mut CadIr, index: usize) {
    use cadmpeg_ir::geometry::nurbs::{NurbsCurve, NurbsPoles3};
    use cadmpeg_ir::geometry::{CurveGeometry, SolvedCurveGeometry};
    use cadmpeg_ir::math::Point3;
    let curve = ir.geometry.edges[index].curve().cloned().unwrap();
    let nurbs = NurbsCurve::new(
        1,
        vec![0.0, 0.0, 1.0, 1.0],
        NurbsPoles3::Polynomial {
            points: vec![Point3::new(0.0, 0.0, 0.0), Point3::new(1.0, 0.0, 0.0)],
        },
        false,
    )
    .unwrap();
    for row in &mut ir.geometry.curves {
        if row.id == curve {
            row.geometry = CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(nurbs.clone()));
        }
    }
}

#[test]
fn a_nurbs_edge_whose_coedges_all_run_against_it_is_stored_reversed_without_a_curve_marker() {
    let mut ir = cube();
    on_nurbs(&mut ir, 0);
    reverse_all_coedges(&mut ir, 0);
    let reversed = reversed_edges(&ir).unwrap();
    assert!(reversed.edges.contains(&ir.geometry.edges[0].id));
    assert!(reversed.curves.is_empty());
}

#[test]
fn a_closed_nurbs_edge_whose_coedges_all_run_against_it_is_refused_by_name() {
    let mut ir = cube();
    on_nurbs(&mut ir, 0);
    reverse_all_coedges(&mut ir, 0);
    let edge = &mut ir.geometry.edges[0];
    let start = edge.ends.as_ref().unwrap().start.clone();
    edge.ends = Some(cadmpeg_ir::topology::EdgeEnds::new(start.clone(), start));
    let id = edge.id.clone();
    let error = reversed_edges(&ir).err().unwrap();
    assert!(
        matches!(&error, CodecError::NotImplemented(message) if message.contains(id.as_str())),
        "{error:?}"
    );
}
