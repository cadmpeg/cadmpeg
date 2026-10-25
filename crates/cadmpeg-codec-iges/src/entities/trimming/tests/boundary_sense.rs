// SPDX-License-Identifier: Apache-2.0
//! Parameter-curve collections of Type 141 items used with sense 2.

use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeOptions, DecodeResult};
use cadmpeg_ir::eval::admission::EvaluationAdmission;
use cadmpeg_ir::geometry::DirectedParameterRange;
use cadmpeg_ir::topology::{Coedge, Sense};
use cadmpeg_ir::CadIr;

use super::super::winding::polygon_signed_area_twice;
use crate::loss::IgesLossCode;
use crate::test_support::test_drawing_and_trimming::test_mixed_sense::{
    bounded_square_file, disagreeing_parameter_curve_square_file,
    edge_direction_parameter_curve_square_file,
};
use crate::IgesCodec;

const SAMPLES_PER_USE: u32 = 8;

/// Twice the area of the unit square.
const UNIT_SQUARE_AREA_TWICE: f64 = 2.0;

const EPS_PARAMETER_POINT: f64 = 1.0e-12;

fn decode(bytes: Vec<u8>) -> DecodeResult {
    IgesCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("decode the fixture")
}

/// The coedges of the one loop of `ir` in traversal order.
fn loop_coedges(ir: &CadIr) -> Vec<&Coedge> {
    let [loop_] = ir.model.loops.as_slice() else {
        panic!("the model has one loop");
    };
    loop_
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

/// The parameter-space points of the uses of `coedge`, in use order, each use
/// sampled over its stated interval.
fn use_points(ir: &CadIr, coedge: &Coedge) -> Vec<[f64; 2]> {
    let mut points = Vec::new();
    for pcurve_use in &coedge.pcurves {
        let pcurve = ir
            .model
            .pcurves
            .iter()
            .find(|pcurve| pcurve.id == pcurve_use.pcurve)
            .expect("the used pcurve");
        let [start, end] = pcurve_use.parameter_range.map_or_else(
            || pcurve.parameter_range().expect("a pcurve range").get(),
            DirectedParameterRange::endpoints,
        );
        for step in 0..=SAMPLES_PER_USE {
            let t = start + (end - start) * f64::from(step) / f64::from(SAMPLES_PER_USE);
            let uv = cadmpeg_ir::eval::decode::pcurve_uv(
                EvaluationAdmission::Standard,
                &pcurve.geometry,
                t,
            )
            .expect("the pcurve evaluates")
            .get();
            points.push([uv.u, uv.v]);
        }
    }
    points
}

fn same_parameter_point(left: [f64; 2], right: [f64; 2]) -> bool {
    (left[0] - right[0]).abs() <= EPS_PARAMETER_POINT
        && (left[1] - right[1]).abs() <= EPS_PARAMETER_POINT
}

/// The face traces the counterclockwise unit square in parameter space: each
/// coedge's uses form one chain from the end of the previous coedge, and the
/// reversed third coedge runs from `(1, 1)` to `(0, 1)`.
fn assert_unit_square_chain(ir: &CadIr) {
    let coedges = loop_coedges(ir);
    assert_eq!(coedges.len(), 4);
    assert_eq!(coedges[2].sense, Sense::Reversed);
    let mut ring = Vec::new();
    for coedge in &coedges {
        let points = use_points(ir, coedge);
        let (Some(first), Some(_)) = (points.first(), points.last()) else {
            panic!("every coedge has a parameter curve use");
        };
        if let Some(previous) = ring.last() {
            assert!(same_parameter_point(*previous, *first));
        }
        ring.extend(points);
    }
    let third = use_points(ir, coedges[2]);
    assert!(same_parameter_point(third[0], [1.0, 1.0]));
    assert!(same_parameter_point(third[third.len() - 1], [0.0, 1.0]));
    assert!(
        (polygon_signed_area_twice(&ring) - UNIT_SQUARE_AREA_TWICE).abs() <= EPS_PARAMETER_POINT
    );
}

/// IGES 5.3 §4.31 D12 orients a collection in the boundary traversal. A
/// sense-2 collection stated in the model-curve direction composes the same
/// model-space curve, so the coedge keeps it and uses it backward.
#[test]
fn edge_direction_parameter_curve_of_a_sense_2_item_is_used_backward() {
    for preference in 0..=3 {
        let result = decode(edge_direction_parameter_curve_square_file(preference));
        assert!(result.report().losses.is_empty());
        let ir = result.ir();
        assert_eq!(ir.model.faces.len(), 1);
        let validation =
            cadmpeg_ir::validate_neutral(ir, Vec::new()).expect("validate the neutral model");
        assert!(validation.is_ok());
        let [pcurve_use] = loop_coedges(ir)[2].pcurves.as_slice() else {
            panic!("the reversed coedge has one parameter curve use");
        };
        assert_eq!(
            pcurve_use.parameter_range,
            Some(DirectedParameterRange::new([1.0, 0.0]).expect("a directed range"))
        );
        assert_unit_square_chain(ir);
    }
}

/// A sense-2 collection in the boundary traversal keeps its source interval.
#[test]
fn traversal_parameter_curve_of_a_sense_2_item_keeps_its_interval() {
    let result = decode(bounded_square_file());
    assert!(result.report().losses.is_empty());
    let ir = result.ir();
    let [pcurve_use] = loop_coedges(ir)[2].pcurves.as_slice() else {
        panic!("the reversed coedge has one parameter curve use");
    };
    assert_eq!(pcurve_use.parameter_range, None);
    assert_unit_square_chain(ir);
}

/// A model-preferred boundary omits an alternate collection whose image is not
/// its model-space curve, and records the omission.
#[test]
fn disagreeing_alternate_parameter_curve_is_omitted_with_a_loss() {
    let result = decode(disagreeing_parameter_curve_square_file(1));
    assert!(result
        .report()
        .losses
        .iter()
        .any(|loss| loss.code == IgesLossCode::BoundaryPcurveDisagrees.kind()));
    let ir = result.ir();
    assert_eq!(ir.model.faces.len(), 1);
    let coedges = loop_coedges(ir);
    assert!(coedges[2].pcurves.is_empty());
    assert!(coedges
        .iter()
        .enumerate()
        .all(|(position, coedge)| position == 2 || coedge.pcurves.len() == 1));
}

/// A parameter-preferred boundary whose collection does not compose its
/// model-space curve projects no face.
#[test]
fn disagreeing_preferred_parameter_curve_projects_no_face() {
    let result = decode(disagreeing_parameter_curve_square_file(2));
    assert!(result.ir().model.faces.is_empty());
    assert!(!result.report().losses.is_empty());
    assert!(result
        .report()
        .losses
        .iter()
        .all(|loss| loss.code != IgesLossCode::BoundaryPcurveDisagrees.kind()));
}
