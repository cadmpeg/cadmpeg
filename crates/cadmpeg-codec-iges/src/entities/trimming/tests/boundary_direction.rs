// SPDX-License-Identifier: Apache-2.0
//! Traversal direction of Type 144 boundaries about the support normal.

use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::eval::admission::EvaluationAdmission;
use cadmpeg_ir::geometry::pcurve::{PcurveGeometry, PcurveNurbs, PcurveNurbsPoles};
use cadmpeg_ir::math::{Point2, Point3, Vector3};
use cadmpeg_ir::topology::{LoopBoundaryRole, Sense};
use cadmpeg_ir::units::FinitePoint2;
use cadmpeg_ir::CadIr;

use super::super::{polygon_signed_area_twice, reversed_pcurve};
use crate::test_support::test_drawing_and_trimming::{
    trimmed_plane_with_boundaries, trimmed_plane_with_boundaries_and_inner,
    trimmed_plane_with_inner_loop_file,
};
use crate::IgesCodec;

const EPS_REVERSED_PCURVE_POINT: f64 = 1.0e-12;
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

/// The parameter-space area of the single pcurve of each coedge of
/// `loop_id`, traversed in the pcurve's own direction.
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
            let [start, end] = pcurve.parameter_range().unwrap().get();
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
/// parameter curve of each coedge runs with its coedge.
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

#[test]
fn counterclockwise_inner_boundary_runs_clockwise_about_the_support_normal() {
    let ir = decoded(trimmed_plane_with_inner_loop_file());
    assert_neutral_directions(&ir);
    let inner = ir
        .model
        .coedges
        .iter()
        .find(|coedge| coedge.owner_loop.as_str() == "iges:model:loop#D15:1")
        .unwrap();
    assert_eq!(inner.sense, Sense::Reversed);
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
fn reversed_pcurve_traverses_the_interval_backward() {
    let nurbs = PcurveNurbs::new(
        2,
        vec![0.0, 0.0, 0.0, 1.0, 2.0, 2.0, 2.0],
        PcurveNurbsPoles::Polynomial {
            points: [[0.0, 0.0], [1.0, 2.0], [3.0, 1.0], [4.0, 4.0]]
                .into_iter()
                .map(|[u, v]| FinitePoint2::new(Point2::new(u, v)).unwrap())
                .collect(),
        },
        false,
    )
    .unwrap();
    let source = (PcurveGeometry::Nurbs { nurbs }, [0.5, 1.75]);
    let reversed = reversed_pcurve(&source).unwrap();
    assert_eq!(reversed.1, source.1);
    for step in 0..=SAMPLES_PER_COEDGE {
        let t = 0.5 + 1.25 * f64::from(step) / f64::from(SAMPLES_PER_COEDGE);
        let forward = cadmpeg_ir::eval::decode::pcurve_uv(
            EvaluationAdmission::Standard,
            &source.0,
            0.5 + 1.75 - t,
        )
        .unwrap()
        .get();
        let backward =
            cadmpeg_ir::eval::decode::pcurve_uv(EvaluationAdmission::Standard, &reversed.0, t)
                .unwrap()
                .get();
        assert!(
            (forward.u - backward.u).hypot(forward.v - backward.v) <= EPS_REVERSED_PCURVE_POINT,
            "sample {step} agrees"
        );
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
