// SPDX-License-Identifier: Apache-2.0
//! Decoded face models whose coedges traverse their edge curves and
//! parameter curves in mixed directions.

use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::geometry::pcurve::PcurveGeometry;
use cadmpeg_ir::geometry::{
    CompositeCurveSegment, CompositeCurveTransition, Curve, CurveGeometry, SolvedCurveGeometry,
};
use cadmpeg_ir::ids::CurveId;
use cadmpeg_ir::topology::{EdgeCarrier, FaceLoops, IncreasingParameterInterval, Sense};
use cadmpeg_ir::CadIr;

use super::{explicit_multi_pcurve_loop_file, trimmed_plane_with_inner_loop_file};
use crate::test_support::test_owned::{owned_test_file, OwnedTestEntity};
use crate::IgesCodec;

/// The surface `S(u, v) = (u, v, 0)` over `[0, 1]²` as a Type 128 entity.
const UNIT_PATCH: &str =
    "128,1,1,1,1,0,0,1,0,0,0,0,1,1,0,0,1,1,1,1,1,1,0,0,0,1,0,0,0,1,0,1,1,0,0,1,0,1;";

/// The coedges of the first loop of `ir` in loop order.
fn loop_coedges(ir: &CadIr) -> Vec<&cadmpeg_ir::topology::Coedge> {
    ir.model.loops[0]
        .coedges()
        .iter()
        .map(|id| {
            ir.model
                .coedges
                .iter()
                .find(|coedge| coedge.id == *id)
                .expect("the loop coedge")
        })
        .collect()
}

fn decoded(bytes: Vec<u8>) -> CadIr {
    IgesCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("decode the fixture")
        .ir()
        .clone()
}

/// A Type 143 face on the unit patch whose Type 141 boundary runs
/// counterclockwise around the unit square. Its third model curve is stated
/// from `(0, 1, 0)` to `(1, 1, 0)` and used with sense 2; every parameter
/// curve runs in the traversal.
pub(crate) fn bounded_square_file() -> Vec<u8> {
    bounded_square_file_with("110,1,1,0,0,1,0;", 1)
}

/// The bounded unit square with `third` as the parameter curve of the
/// reversed third model curve and `preference` as the Type 141 `PREF`.
fn bounded_square_file_with(third: &str, preference: u8) -> Vec<u8> {
    let entity = |entity_type, status, parameters: &str| OwnedTestEntity {
        entity_type,
        form: 0,
        label: "SQUARE".into(),
        status,
        parameters: parameters.into(),
    };
    owned_test_file(&[
        entity(128, "00010000", UNIT_PATCH),
        entity(110, "00010000", "110,0,0,0,1,0,0;"),
        entity(110, "00010000", "110,1,0,0,1,1,0;"),
        entity(110, "00010000", "110,0,1,0,1,1,0;"),
        entity(110, "00010000", "110,0,1,0,0,0,0;"),
        entity(110, "00010500", "110,0,0,0,1,0,0;"),
        entity(110, "00010500", "110,1,0,0,1,1,0;"),
        entity(110, "00010500", third),
        entity(110, "00010500", "110,0,1,0,0,0,0;"),
        entity(
            141,
            "00010000",
            &format!("141,1,{preference},1,4,3,1,1,11,5,1,1,13,7,2,1,15,9,1,1,17;"),
        ),
        entity(143, "00000000", "143,1,1,1,19;"),
    ])
}

/// The bounded unit square whose reversed third model curve has its
/// parameter curve stated in the direction of that model curve, from
/// `(0, 1)` to `(1, 1)`, with `preference` as the Type 141 `PREF`.
pub(crate) fn edge_direction_parameter_curve_square_file(preference: u8) -> Vec<u8> {
    bounded_square_file_with("110,0,1,0,1,1,0;", preference)
}

/// The bounded unit square whose reversed third model curve has a parameter
/// curve from `(0, 0.5)` to `(1, 0.5)`, whose image is not that model curve,
/// with `preference` as the Type 141 `PREF`.
pub(crate) fn disagreeing_parameter_curve_square_file(preference: u8) -> Vec<u8> {
    bounded_square_file_with("110,0,0.5,0,1,0.5,0;", preference)
}

/// The bounded unit square with its one loop classified as the outer loop,
/// which the writer states as a Type 142 boundary. The loop uses its third
/// edge reversed and the other three forward.
pub(crate) fn mixed_sense_outer_loop_model() -> CadIr {
    let mut ir = decoded(bounded_square_file());
    let [face] = ir.model.faces.as_mut_slice() else {
        panic!("the fixture decodes one face");
    };
    let FaceLoops::Unspecified { loops } = &face.loops else {
        panic!("a Type 143 face does not classify its loops");
    };
    let [outer] = loops.as_slice() else {
        panic!("the fixture face has one loop");
    };
    let outer = outer.clone();
    face.loops = FaceLoops::Classified {
        outer,
        inner: Vec::new(),
    };
    let senses = loop_coedges(&ir)
        .into_iter()
        .map(|coedge| coedge.sense)
        .collect::<Vec<_>>();
    assert_eq!(
        senses,
        [
            Sense::Forward,
            Sense::Forward,
            Sense::Reversed,
            Sense::Forward
        ]
    );
    ir
}

/// The mixed-sense outer loop with the curves of its first edge (forward)
/// and its third edge (reversed) each stated as a Type 102 composite of one
/// segment.
pub(crate) fn composite_edge_outer_loop_model() -> CadIr {
    let mut ir = mixed_sense_outer_loop_model();
    for position in [0, 2] {
        let edge_id = loop_coedges(&ir)[position].edge.clone();
        let edge = ir
            .model
            .edges
            .iter_mut()
            .find(|edge| edge.id == edge_id)
            .expect("the coedge edge");
        let child = edge.curve().expect("an edge curve").clone();
        let composite: CurveId = format!("{child}:composite")
            .try_into()
            .expect("a composite curve id");
        let range = edge.param_range().map(cadmpeg_ir::units::FiniteVector::get);
        edge.carrier =
            EdgeCarrier::new(Some(composite.clone()), range).expect("the child edge range");
        let segments = vec![CompositeCurveSegment {
            curve: child,
            same_sense: true,
            transition: CompositeCurveTransition::Continuous,
        }];
        ir.model.curves.push(Curve {
            id: composite,
            geometry: CurveGeometry::Solved(SolvedCurveGeometry::Composite {
                segments: segments.try_into().expect("one segment"),
                self_intersect: None,
            }),
            parameter_range: range.and_then(IncreasingParameterInterval::new),
            source_object: None,
        });
    }
    ir.model
        .curves
        .sort_by(|left, right| left.id.as_str().cmp(right.id.as_str()));
    ir
}

/// The trimmed plane with an inner loop whose closed outer edge is reversed:
/// its vertices swap, its NURBS curve, edge range and carrier range reverse,
/// and its coedge uses it reversed. The parameter curve still runs in the
/// traversal, so the face traces the same boundary.
pub(crate) fn reversed_closed_outer_edge_model() -> CadIr {
    let mut ir = decoded(trimmed_plane_with_inner_loop_file());
    let outer = ir.model.loops[0].coedges()[0].clone();
    let edge = ir
        .model
        .coedges
        .iter()
        .find(|coedge| coedge.id == outer)
        .expect("the outer coedge")
        .edge
        .clone();
    reverse_edge(&mut ir, &edge);
    ir
}

/// The bounded unit square whose reversed third coedge states its parameter
/// curve in the direction of the edge curve, from `(0, 1)` to `(1, 1)`.
pub(crate) fn edge_direction_parameter_curve_boundary_model() -> CadIr {
    let mut ir = decoded(bounded_square_file());
    let coedge = loop_coedges(&ir)[2];
    assert_eq!(coedge.sense, Sense::Reversed);
    let [pcurve_use] = coedge.pcurves.as_slice() else {
        panic!("the reversed coedge has one parameter curve");
    };
    let pcurve = pcurve_use.pcurve.clone();
    reverse_pcurve(&mut ir, &pcurve);
    ir
}

/// The explicit multi-pcurve loop face whose first edge is reversed, with a
/// reversed coedge that states its two parameter curves in the direction of
/// the edge curve: each parameter curve reverses, and the uses list them in
/// edge-curve order.
pub(crate) fn edge_direction_parameter_curve_brep_model() -> CadIr {
    let mut ir = decoded(explicit_multi_pcurve_loop_file());
    let position = ir
        .model
        .coedges
        .iter()
        .position(|coedge| coedge.pcurves.len() == 2)
        .expect("the coedge with two parameter curves");
    let edge = ir.model.coedges[position].edge.clone();
    reverse_edge(&mut ir, &edge);
    let coedge = &mut ir.model.coedges[position];
    coedge.pcurves.reverse();
    let pcurves = coedge
        .pcurves
        .iter()
        .map(|pcurve_use| pcurve_use.pcurve.clone())
        .collect::<Vec<_>>();
    for pcurve in &pcurves {
        reverse_pcurve(&mut ir, pcurve);
    }
    ir
}

/// Reverse edge `edge_id` and its curve: the curve traces the same points
/// backward, the edge swaps its vertices and reflects its range, and each
/// coedge of the edge inverts its sense.
fn reverse_edge(ir: &mut CadIr, edge_id: &cadmpeg_ir::ids::EdgeId) {
    let edge = ir
        .model
        .edges
        .iter_mut()
        .find(|edge| edge.id == *edge_id)
        .expect("the edge");
    let curve_id = edge.curve().expect("an edge curve").clone();
    let curve = ir
        .model
        .curves
        .iter_mut()
        .find(|curve| curve.id == curve_id)
        .expect("the edge curve");
    let reflect: Box<dyn Fn(f64) -> f64> = match &mut curve.geometry {
        CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(nurbs)) => {
            let degree = usize::try_from(nurbs.degree()).expect("a NURBS degree");
            let knots = nurbs.knots();
            let (start, end) = (knots[degree], knots[nurbs.control_points().len()]);
            nurbs
                .reverse_parameterization_in_range(
                    cadmpeg_ir::scalar::FiniteReal::new(start).expect("a finite knot"),
                    cadmpeg_ir::scalar::FiniteReal::new(end).expect("a finite knot"),
                )
                .expect("a reversed knot vector");
            Box::new(move |parameter| start + end - parameter)
        }
        CurveGeometry::Solved(SolvedCurveGeometry::Line(line)) => {
            line.reverse_parameterization();
            Box::new(|parameter: f64| -parameter)
        }
        _ => panic!("the fixture edge curve is a line or a NURBS curve"),
    };
    let reversed = |range: [f64; 2]| [reflect(range[1]), reflect(range[0])];
    curve.parameter_range = curve
        .parameter_range
        .map(|range| IncreasingParameterInterval::new(reversed(range.into())).expect("a range"));
    let range = edge.param_range().map(|range| reversed(range.get()));
    edge.carrier = EdgeCarrier::new(Some(curve_id), range).expect("the reversed edge range");
    std::mem::swap(&mut edge.start, &mut edge.end);
    for coedge in &mut ir.model.coedges {
        if coedge.edge == *edge_id {
            coedge.sense = match coedge.sense {
                Sense::Forward => Sense::Reversed,
                Sense::Reversed => Sense::Forward,
            };
        }
    }
}

/// Reverse parameter curve `pcurve_id` in place. The fixture parameter
/// curves are polynomial with a knot vector symmetric about the middle of
/// its domain, so the reversed pole order traces the same points backward
/// over the same domain.
fn reverse_pcurve(ir: &mut CadIr, pcurve_id: &cadmpeg_ir::ids::PcurveId) {
    let pcurve = ir
        .model
        .pcurves
        .iter_mut()
        .find(|pcurve| pcurve.id == *pcurve_id)
        .expect("the parameter curve");
    let PcurveGeometry::Nurbs { nurbs } = &mut pcurve.geometry else {
        panic!("the fixture parameter curve is a NURBS curve");
    };
    assert!(nurbs.weights().is_none());
    let knots = nurbs.knots().iter().copied().collect::<Vec<_>>();
    let (first, last) = (knots[0], knots[knots.len() - 1]);
    assert!(knots
        .iter()
        .zip(knots.iter().rev())
        .all(|(knot, mirror)| knot + mirror == first + last));
    let mut poles = nurbs.control_points();
    poles.reverse();
    assert!(nurbs.replace_admitted_control_points(&poles));
}
