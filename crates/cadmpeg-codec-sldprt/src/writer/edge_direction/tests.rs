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
fn a_nurbs_curve_whose_edges_all_run_against_their_coedges_is_written_reversed() {
    let mut ir = cube();
    on_nurbs(&mut ir, 0);
    reverse_all_coedges(&mut ir, 0);
    let reversed = reversed_edges(&ir).unwrap();
    let edge = &ir.geometry.edges[0];
    assert!(reversed.edges.contains(&edge.id));
    assert!(reversed.curves.contains(edge.curve().unwrap()));
}

#[test]
fn a_nurbs_curve_that_also_carries_an_edge_stored_along_it_keeps_its_direction() {
    let mut ir = cube();
    on_nurbs(&mut ir, 0);
    reverse_all_coedges(&mut ir, 0);
    let shared = ir.geometry.edges[0].curve().cloned();
    ir.geometry.edges[1].set_curve(shared).unwrap();
    let reversed = reversed_edges(&ir).unwrap();
    assert!(reversed.edges.contains(&ir.geometry.edges[0].id));
    assert!(!reversed.edges.contains(&ir.geometry.edges[1].id));
    assert!(reversed.curves.is_empty());
}

/// Make edge `index` closed at its start vertex.
fn close(ir: &mut CadIr, index: usize) -> cadmpeg_ir::ids::EdgeId {
    let edge = &mut ir.geometry.edges[index];
    let start = edge.ends.as_ref().unwrap().start.clone();
    edge.ends = Some(cadmpeg_ir::topology::EdgeEnds::new(start.clone(), start));
    edge.id.clone()
}

#[test]
fn a_closed_nurbs_edge_whose_coedges_all_run_against_it_is_written_on_its_reversed_curve() {
    let mut ir = cube();
    on_nurbs(&mut ir, 0);
    reverse_all_coedges(&mut ir, 0);
    close(&mut ir, 0);
    let reversed = reversed_edges(&ir).unwrap();
    assert!(reversed
        .curves
        .contains(ir.geometry.edges[0].curve().unwrap()));
}

#[test]
fn a_closed_nurbs_edge_against_its_coedges_on_a_curve_that_keeps_its_direction_is_refused_by_name()
{
    let mut ir = cube();
    on_nurbs(&mut ir, 0);
    reverse_all_coedges(&mut ir, 0);
    let shared = ir.geometry.edges[0].curve().cloned();
    ir.geometry.edges[1].set_curve(shared).unwrap();
    let id = close(&mut ir, 0);
    let error = reversed_edges(&ir).err().unwrap();
    assert!(
        matches!(&error, CodecError::NotImplemented(message) if message.contains(id.as_str())),
        "{error:?}"
    );
}

/// A triangle on a NURBS surface, bounded by one NURBS edge and two edges
/// without a curve, each the boundary of the one face and so used by one
/// coedge; decoded from its native record image.
fn nurbs_triangle() -> CadIr {
    let native = crate::test_support::container::sldprt_with_body(
        &crate::test_support::parasolid::nurbs_triangle_body(),
    );
    <crate::SldprtCodec as cadmpeg_ir::codec::Codec>::decode(
        &crate::SldprtCodec,
        &mut std::io::Cursor::new(native),
        &cadmpeg_ir::codec::DecodeOptions::default(),
    )
    .unwrap()
    .ir()
    .clone()
}

/// The triangle with its NURBS edge and curve both turned around, so that
/// the edge's one coedge runs against it while the loop is unchanged, and
/// each vertex of that edge moved `gap` mm off the curve's end.
fn reversed_nurbs_edge(gap: f64) -> CadIr {
    use cadmpeg_ir::geometry::{CurveGeometry, SolvedCurveGeometry};
    use cadmpeg_ir::topology::EdgeEnds;
    let mut ir = nurbs_triangle();
    let index = ir
        .geometry
        .edges
        .iter()
        .position(|edge| edge.curve().is_some())
        .unwrap();
    let edge = &mut ir.geometry.edges[index];
    let ends = edge.ends.clone().unwrap();
    edge.ends = Some(EdgeEnds::new(ends.end.clone(), ends.start.clone()));
    let (id, curve) = (edge.id.clone(), edge.curve().cloned().unwrap());
    for coedge in &mut ir.geometry.coedges {
        if coedge.edge == id {
            coedge.sense = match coedge.sense {
                Sense::Forward => Sense::Reversed,
                Sense::Reversed => Sense::Forward,
            };
        }
    }
    for row in &mut ir.geometry.curves {
        if row.id == curve {
            let Some(SolvedCurveGeometry::Nurbs(nurbs)) = row.geometry.solved() else {
                panic!("the triangle's curve is a NURBS curve");
            };
            let mut turned = nurbs.clone();
            turned.reverse_parameterization();
            row.geometry = CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(turned));
        }
    }
    let moved = [ends.start, ends.end]
        .iter()
        .map(|vertex| {
            ir.geometry
                .vertices
                .iter()
                .find(|row| &row.id == vertex)
                .unwrap()
                .point
                .clone()
        })
        .collect::<Vec<_>>();
    for point in &mut ir.geometry.points {
        if moved.contains(&point.id) {
            let p = point.position().get();
            let off = cadmpeg_ir::math::Point3::new(p.x + 0.6 * gap, p.y, p.z + 0.8 * gap);
            point.set_position(cadmpeg_ir::features::geometry::FinitePoint3::new(off).unwrap());
        }
    }
    ir
}

/// The reader orders a NURBS edge stored against its curve by its vertices'
/// places on the curve, within the stream's linear resolution. Vertices
/// `1e-3` mm off the curve's ends are within the document's coincidence
/// allowance but not that resolution, so only a curve that runs along the
/// stored edge keeps the edge's direction.
#[test]
fn a_nurbs_edge_against_its_coedges_reads_back_along_its_curve() {
    let ir = reversed_nurbs_edge(1.0e-3);
    let edge = ir
        .geometry
        .edges
        .iter()
        .find(|edge| edge.curve().is_some())
        .unwrap();
    assert!(reversed_edges(&ir).unwrap().edges.contains(&edge.id));
    let source = cadmpeg_ir::validate_neutral(&ir, Vec::new()).unwrap();
    assert!(
        !format!("{:?}", source.findings).contains("runs against its curve"),
        "{:?}",
        source.findings
    );
    let body = crate::writer::brep_body(&ir, 0.001, false).unwrap();
    let native = crate::test_support::container::sldprt_with_body(&body);
    let decoded = <crate::SldprtCodec as cadmpeg_ir::codec::Codec>::decode(
        &crate::SldprtCodec,
        &mut std::io::Cursor::new(native),
        &cadmpeg_ir::codec::DecodeOptions::default(),
    )
    .unwrap()
    .ir()
    .clone();
    let kinds = decoded
        .geometry
        .curves
        .iter()
        .map(|curve| {
            format!("{:?}", curve.geometry)
                .chars()
                .take(40)
                .collect::<String>()
        })
        .collect::<Vec<_>>();
    assert!(
        kinds.iter().any(|kind| kind.contains("Nurbs")),
        "{} edges, curves {kinds:#?}",
        decoded.geometry.edges.len()
    );
    let report = cadmpeg_ir::validate_neutral(&decoded, Vec::new()).unwrap();
    let findings = format!("{:?}", report.findings);
    assert!(!findings.contains("runs against its curve"), "{findings}");
}
