// SPDX-License-Identifier: Apache-2.0
//! Traversal direction of Type 144 boundaries about the support normal.

use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::eval::admission::EvaluationAdmission;
use cadmpeg_ir::geometry::pcurve::PcurveGeometry;
use cadmpeg_ir::geometry::DirectedParameterRange;
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::topology::{Coedge, LoopBoundaryRole, Sense};
use cadmpeg_ir::CadIr;

use super::super::winding::polygon_signed_area_twice;
use crate::loss::IgesLossCode;
use crate::test_support::test_drawing_and_trimming::test_boundary_direction::{
    trimmed_nurbs_patch_file, trimmed_plane_with_outer_and_inner,
};
use crate::test_support::test_drawing_and_trimming::{
    trimmed_plane_with_boundaries, trimmed_plane_with_boundaries_and_inner,
    trimmed_plane_with_inner_loop_file,
};
use crate::IgesCodec;

/// A clockwise square about `+z` from `(0.2, 0.2)` over `[0, 4]`.
const CLOCKWISE_SQUARE: &str = "106,1,5,0,0.2,0.2,0.2,0.8,0.8,0.8,0.8,0.2,0.2,0.2;";

const SAMPLES_PER_COEDGE: u32 = 16;

/// The model-space area of the loop `loop_id` about `normal`, traversed with
/// each coedge sense applied: positive when the loop runs counterclockwise
/// about `normal`.
fn loop_area_about(ir: &CadIr, loop_id: &str, normal: Vector3) -> f64 {
    let loop_ = ir
        .model
        .loops
        .iter()
        .find(|loop_| loop_.id.as_str() == loop_id)
        .unwrap();
    let mut points = Vec::new();
    for coedge_id in loop_.coedges() {
        let coedge = ir
            .model
            .coedges
            .iter()
            .find(|coedge| coedge.id == *coedge_id)
            .unwrap();
        let edge = ir
            .model
            .edges
            .iter()
            .find(|edge| edge.id == coedge.edge)
            .unwrap();
        let curve = ir
            .model
            .curves
            .iter()
            .find(|curve| Some(&curve.id) == edge.curve())
            .unwrap();
        let [start, end] = edge.param_range().unwrap().get();
        let mut coedge_points = (0..=SAMPLES_PER_COEDGE)
            .map(|step| {
                let t = start + (end - start) * f64::from(step) / f64::from(SAMPLES_PER_COEDGE);
                cadmpeg_ir::eval::decode::curve_point(
                    EvaluationAdmission::Standard,
                    &curve.geometry,
                    t,
                )
                .unwrap()
                .get()
            })
            .collect::<Vec<Point3>>();
        if coedge.sense == Sense::Reversed {
            coedge_points.reverse();
        }
        points.extend(coedge_points);
    }
    let origin = points[0];
    let mut area = 0.0;
    for (index, point) in points.iter().enumerate() {
        let next = points[(index + 1) % points.len()];
        area += point
            .vector_from(origin)
            .cross(next.vector_from(origin))
            .dot(normal);
    }
    area
}

/// The parameter-space area of the pcurves of each coedge of `loop_id`, in
/// use order, each traversed over the interval its use states or else over
/// its own range.
fn pcurve_area(ir: &CadIr, loop_id: &str) -> f64 {
    let loop_ = ir
        .model
        .loops
        .iter()
        .find(|loop_| loop_.id.as_str() == loop_id)
        .unwrap();
    let mut points = Vec::new();
    for coedge_id in loop_.coedges() {
        let coedge = ir
            .model
            .coedges
            .iter()
            .find(|coedge| coedge.id == *coedge_id)
            .unwrap();
        for pcurve_use in &coedge.pcurves {
            let pcurve = ir
                .model
                .pcurves
                .iter()
                .find(|pcurve| pcurve.id == pcurve_use.pcurve)
                .unwrap();
            let [start, end] = pcurve_use.parameter_range.map_or_else(
                || pcurve.parameter_range().unwrap().get(),
                cadmpeg_ir::geometry::DirectedParameterRange::endpoints,
            );
            for step in 0..=SAMPLES_PER_COEDGE {
                let t = start + (end - start) * f64::from(step) / f64::from(SAMPLES_PER_COEDGE);
                let uv = cadmpeg_ir::eval::decode::pcurve_uv(
                    EvaluationAdmission::Standard,
                    &pcurve.geometry,
                    t,
                )
                .unwrap()
                .get();
                points.push([uv.u, uv.v]);
            }
        }
    }
    polygon_signed_area_twice(&points)
}

/// The loops of the one face of `ir` with their roles.
fn face_loops(ir: &CadIr) -> Vec<(String, LoopBoundaryRole)> {
    let [face] = ir.model.faces.as_slice() else {
        panic!("the model has one face");
    };
    assert_eq!(face.sense, Sense::Forward);
    face.loops
        .iter()
        .map(|id| (id.as_str().to_owned(), face.loop_role(id)))
        .collect()
}

fn decoded(bytes: Vec<u8>) -> CadIr {
    let result = IgesCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    assert!(result.report().losses.is_empty());
    let validation =
        cadmpeg_ir::validate_neutral(result.ir(), Vec::new()).expect("validate the neutral model");
    assert!(validation.is_ok());
    result.ir().clone()
}

/// Each loop keeps the face on its left about the +z support normal: the
/// outer loop runs counterclockwise, the inner loop clockwise, and the
/// parameter-curve uses of each coedge run with its coedge.
fn assert_neutral_directions(ir: &CadIr) {
    let normal = Vector3::new(0.0, 0.0, 1.0);
    for (loop_id, role) in face_loops(ir) {
        let area = loop_area_about(ir, &loop_id, normal);
        let parameter_area = pcurve_area(ir, &loop_id);
        match role {
            LoopBoundaryRole::Outer => {
                assert!(area > 0.0, "the outer loop runs counterclockwise");
                assert!(
                    parameter_area > 0.0,
                    "the outer pcurve runs counterclockwise"
                );
            }
            LoopBoundaryRole::Inner => {
                assert!(area < 0.0, "the inner loop runs clockwise");
                assert!(parameter_area < 0.0, "the inner pcurve runs clockwise");
            }
            LoopBoundaryRole::Unspecified => panic!("a loop has no role"),
        }
    }
}

/// The one coedge of the loop `loop_id`.
fn only_coedge<'ir>(ir: &'ir CadIr, loop_id: &str) -> &'ir Coedge {
    let mut coedges = ir
        .model
        .coedges
        .iter()
        .filter(|coedge| coedge.owner_loop.as_str() == loop_id);
    let coedge = coedges.next().expect("the loop has a coedge");
    assert!(coedges.next().is_none(), "the loop has one coedge");
    coedge
}

/// The coedge runs backward over its source boundary: its sense is reversed
/// and its one pcurve use states the reversed interval `[4, 0]` of a pcurve
/// whose poles keep the source order, starting at `first` toward `second`.
fn assert_reversed_use(ir: &CadIr, coedge: &Coedge, first: [f64; 2], second: [f64; 2]) {
    assert_eq!(coedge.sense, Sense::Reversed);
    let [pcurve_use] = coedge.pcurves.as_slice() else {
        panic!("the coedge has one pcurve use");
    };
    assert_eq!(
        pcurve_use.parameter_range,
        Some(DirectedParameterRange::new([4.0, 0.0]).unwrap())
    );
    let pcurve = ir
        .model
        .pcurves
        .iter()
        .find(|pcurve| pcurve.id == pcurve_use.pcurve)
        .unwrap();
    assert_eq!(pcurve.parameter_range().unwrap().get(), [0.0, 4.0]);
    let PcurveGeometry::Nurbs { nurbs } = &pcurve.geometry else {
        panic!("the pcurve is a NURBS curve");
    };
    let poles = nurbs.control_points();
    assert_eq!([poles[0].u, poles[0].v], first);
    assert_eq!([poles[1].u, poles[1].v], second);
}

#[test]
fn counterclockwise_inner_boundary_runs_clockwise_about_the_support_normal() {
    let ir = decoded(trimmed_plane_with_inner_loop_file());
    assert_neutral_directions(&ir);
    let inner = only_coedge(&ir, "iges:model:loop#D15:1");
    assert_reversed_use(&ir, inner, [0.25, 0.25], [0.75, 0.25]);
}

#[test]
fn clockwise_outer_boundary_runs_counterclockwise_about_the_support_normal() {
    let ir = decoded(trimmed_plane_with_outer_and_inner(
        CLOCKWISE_SQUARE,
        CLOCKWISE_SQUARE,
        "106,1,5,0,0.4,0.4,0.4,0.6,0.6,0.6,0.6,0.4,0.4,0.4;",
        "144,1,1,1,7,13;",
    ));
    assert_neutral_directions(&ir);
    let outer = only_coedge(&ir, "iges:model:loop#D15:0");
    assert_reversed_use(&ir, outer, [0.2, 0.2], [0.2, 0.8]);
    assert_eq!(
        only_coedge(&ir, "iges:model:loop#D15:1").sense,
        Sense::Forward
    );
}

/// On the NURBS patch `S(u, v) = (u, v, 0)` the support normal is `+z`, and
/// the winding comes from the parameter-space image.
#[test]
fn clockwise_outer_boundary_on_a_nurbs_support_runs_counterclockwise() {
    let ir = decoded(trimmed_nurbs_patch_file(CLOCKWISE_SQUARE, true));
    assert_neutral_directions(&ir);
    let outer = only_coedge(&ir, "iges:model:loop#D9:0");
    assert_reversed_use(&ir, outer, [0.2, 0.2], [0.2, 0.8]);
}

#[test]
fn counterclockwise_outer_boundary_on_a_nurbs_support_keeps_its_source_direction() {
    let ir = decoded(trimmed_nurbs_patch_file(
        "106,1,5,0,0.2,0.2,0.8,0.2,0.8,0.8,0.2,0.8,0.2,0.2;",
        true,
    ));
    assert_neutral_directions(&ir);
    let outer = only_coedge(&ir, "iges:model:loop#D9:0");
    assert_eq!(outer.sense, Sense::Forward);
    assert!(outer
        .pcurves
        .iter()
        .all(|pcurve_use| pcurve_use.parameter_range.is_none()));
}

/// A model-space boundary on a non-plane support has no parameter-space image
/// and no planar model-space image, so it keeps its source traversal and
/// reports the unresolved winding.
#[test]
fn model_space_boundary_on_a_nurbs_support_keeps_its_source_traversal_with_a_loss() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(trimmed_nurbs_patch_file(CLOCKWISE_SQUARE, false)),
            &DecodeOptions::default(),
        )
        .unwrap();
    assert!(result
        .report()
        .losses
        .iter()
        .any(|loss| loss.code == IgesLossCode::BoundaryWindingUnresolved.kind()));
    let [coedge] = result.ir().model.coedges.as_slice() else {
        panic!("the face has one coedge");
    };
    assert_eq!(coedge.sense, Sense::Forward);
}

#[test]
fn clockwise_inner_boundary_keeps_its_source_direction() {
    let clockwise = "106,1,5,0,0.25,0.25,0.25,0.75,0.75,0.75,0.75,0.25,0.25,0.25;";
    let ir = decoded(trimmed_plane_with_boundaries_and_inner(
        "106,1,5,0,0,0,1,0,1,1,0,1,0,0;",
        clockwise,
        "144,1,1,1,7,13;",
    ));
    assert_neutral_directions(&ir);
    assert!(ir
        .model
        .coedges
        .iter()
        .all(|coedge| coedge.sense == Sense::Forward));
}

#[test]
fn inner_boundary_of_the_parameter_domain_runs_clockwise() {
    let ir = decoded(trimmed_plane_with_boundaries(
        "106,1,5,0,0,0,1,0,1,1,0,1,0,0;",
        "144,1,0,1,,13;",
    ));
    let normal = Vector3::new(0.0, 0.0, 1.0);
    let loops = face_loops(&ir);
    assert_eq!(loops.len(), 1);
    for (loop_id, _) in loops {
        assert!(loop_area_about(&ir, &loop_id, normal) < 0.0);
        assert!(pcurve_area(&ir, &loop_id) < 0.0);
    }
}

#[test]
fn polygon_area_is_positive_counterclockwise() {
    let square = [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]];
    assert_eq!(polygon_signed_area_twice(&square), 2.0);
    let mut clockwise = square;
    clockwise.reverse();
    assert_eq!(polygon_signed_area_twice(&clockwise), -2.0);
}
