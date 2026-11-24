// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use std::f64::consts::{FRAC_PI_2, FRAC_PI_3, FRAC_PI_4};
use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeOptions, DecodeResult};
use cadmpeg_ir::design::dimension::Dimension;
use cadmpeg_ir::design::feature::{FeatureOperation, OperationFamily};
use cadmpeg_ir::design::ops::dressup::{
    CenterSource, Chamfer, ChamferSize, Fillet, FilletSet, FilletSize, Hole, HoleHead,
    HolePositions, HoleShape,
};
use cadmpeg_ir::design::ops::shared::Limit;
use cadmpeg_ir::design::ops::sketch::{EntityDefinition, PlanarCurve};
use cadmpeg_ir::design::selection::{PickTarget, Picks};
use cadmpeg_ir::math::Vector3;

use crate::test_support::{dressup_document, Dress, HoleProps, Member2d, ProfileFrame, Slot};
use crate::SolidEdgeCodec;

/// Agreement of a computed millimetre or radian value with the hand-computed
/// one.
const EPS_VALUE: f64 = 1e-9;

const HOLE: &str = "JMHoleFunction";
const ROUND: &str = "JMRoundFunction";
const CHAMFER: &str = "JMChamferFunction";

/// A profile on the XY plane with normal +Z.
const XY: ProfileFrame = ProfileFrame {
    origin: [0.0, 0.0, 0.0],
    normal: [0.0, 0.0, 1.0],
    x_axis: [1.0, 0.0, 0.0],
};

fn decode(bytes: Vec<u8>) -> DecodeResult {
    SolidEdgeCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap()
}

fn codes(result: &DecodeResult) -> Vec<String> {
    result
        .report()
        .losses
        .iter()
        .map(|loss| loss.code.local_code().to_owned())
        .collect()
}

fn operation(result: &DecodeResult) -> &FeatureOperation {
    &result.ir().design.features[0].definition.operation
}

fn unresolved(result: &DecodeResult, family: OperationFamily) -> bool {
    operation(result) == &FeatureOperation::Unresolved { family }
}

fn props(kind: u32, diameter: f64) -> HoleProps {
    HoleProps {
        kind,
        diameter,
        head_diameter: 0.0,
        head_depth: 0.0,
        sink_degrees: 0.0,
        depth: 0.0,
    }
}

/// A hole feature with `positions`, stored extent `kind` and `side`, and the
/// further slots `extra`.
fn hole_with(
    props: HoleProps,
    kind: f64,
    side: f64,
    positions: &[[f64; 2]],
    extra: &[(usize, Dress)],
) -> DecodeResult {
    let members: Vec<Member2d> = positions.iter().copied().map(Member2d::Hole).collect();
    let mut slots = vec![
        (96, Dress::Props(props)),
        (208, Dress::Value(Slot::Double(kind))),
        (236, Dress::Value(Slot::Double(side))),
    ];
    slots.extend_from_slice(extra);
    decode(dressup_document(HOLE, Some((&XY, &members)), &slots))
}

fn hole(result: &DecodeResult) -> &Hole {
    match operation(result) {
        FeatureOperation::Hole(hole) => hole,
        other => panic!("expected a hole, found {other:?}"),
    }
}

fn bore(hole: &Hole) -> &cadmpeg_ir::design::ops::dressup::Bore {
    match &hole.shape {
        HoleShape::Bore(bore) => bore,
        other => panic!("expected a bore, found {other:?}"),
    }
}

fn mm(dimension: Option<&Dimension<cadmpeg_ir::scalar::PositiveLength>>) -> f64 {
    match dimension {
        Some(Dimension::Literal(value)) => value.get(),
        other => panic!("expected a literal length, found {other:?}"),
    }
}

fn radians(dimension: Option<&Dimension<cadmpeg_ir::scalar::InteriorAngle>>) -> f64 {
    match dimension {
        Some(Dimension::Literal(value)) => value.get(),
        other => panic!("expected a literal angle, found {other:?}"),
    }
}

fn blind_mm(hole: &Hole) -> f64 {
    match &hole.extent {
        Some(Limit::Amount(Dimension::Literal(length))) => length.get(),
        other => panic!("expected a blind extent, found {other:?}"),
    }
}

/// The positions of the hole: the points of the sketch that the hole names as
/// a whole, on the XY profile plane, as `[u, v, 0]` in millimetres.
fn points(result: &DecodeResult, hole: &Hole) -> Vec<[f64; 3]> {
    let HolePositions::Points { points, centers } = &hole.positions else {
        panic!("expected sketch positions, found {:?}", hole.positions);
    };
    assert_eq!(
        centers.as_ref().unwrap().iter().collect::<Vec<_>>(),
        [&CenterSource::Points]
    );
    let Picks::Named(picks) = &points.picks else {
        panic!("expected a named pick");
    };
    let [pick] = picks.as_slice() else {
        panic!("expected one pick");
    };
    let PickTarget::Sketch(sketch) = &pick.target else {
        panic!("expected a whole-sketch pick");
    };
    result
        .ir()
        .design
        .sketch_entities
        .iter()
        .filter(|entity| &entity.sketch == sketch)
        .map(|entity| match &entity.definition {
            EntityDefinition::Free(PlanarCurve::Point(point)) => {
                let at = point.get();
                [at.u, at.v, 0.0]
            }
            other => panic!("expected a point, found {other:?}"),
        })
        .collect()
}

fn near(a: f64, b: f64) -> bool {
    (a - b).abs() <= EPS_VALUE
}

#[test]
fn simple_hole_four_millimetres_through_all() {
    let result = hole_with(props(0, 0.004), 16.0, 1.0, &[[0.01, 0.02]], &[]);
    let hole = hole(&result);
    assert!(near(mm(bore(hole).diameter.as_ref()), 4.0));
    assert_eq!(bore(hole).head, Some(HoleHead::Plain));
    assert_eq!(hole.extent, Some(Limit::ThroughAll));
    assert_eq!(hole.thread, None);
    let at = points(&result, hole);
    assert_eq!(at.len(), 1);
    assert!(near(at[0][0], 10.0) && near(at[0][1], 20.0) && near(at[0][2], 0.0));
    let found = codes(&result);
    assert!(!found.contains(&"feature.dressup-unread".to_owned()));
    assert!(!found.contains(&"feature.operands-pending".to_owned()));
    assert!(!found.contains(&"feature.selection-unread".to_owned()));
}

#[test]
fn six_millimetre_hole() {
    let result = hole_with(props(0, 0.006), 16.0, 1.0, &[[0.0, 0.0]], &[]);
    assert!(near(mm(bore(hole(&result)).diameter.as_ref()), 6.0));
}

#[test]
fn side_one_drills_against_the_profile_normal_and_side_two_along_it() {
    let against = hole_with(props(0, 0.004), 16.0, 1.0, &[[0.0, 0.0]], &[]);
    let along = hole_with(props(0, 0.004), 16.0, 2.0, &[[0.0, 0.0]], &[]);
    let direction = |result: &DecodeResult| {
        let unit = hole(result).direction.as_ref().unwrap().resolved.unwrap();
        Vector3::from(unit)
    };
    assert!(near(direction(&against).z, -1.0));
    assert!(near(direction(&along).z, 1.0));
}

#[test]
fn blind_hole_takes_its_depth_from_the_dimension() {
    let depth = (264, Dress::Value(Slot::Dim(0.008)));
    let result = hole_with(props(0, 0.004), 13.0, 1.0, &[[0.0, 0.0]], &[depth]);
    assert!(near(blind_mm(hole(&result)), 8.0));
}

#[test]
fn blind_hole_falls_back_to_the_stored_depth() {
    let mut stored = props(0, 0.004);
    stored.depth = 0.012;
    let result = hole_with(stored, 13.0, 1.0, &[[0.0, 0.0]], &[]);
    assert!(near(blind_mm(hole(&result)), 12.0));
}

#[test]
fn counterbore_eight_by_three() {
    let mut stored = props(2, 0.004);
    stored.head_diameter = 0.008;
    stored.head_depth = 0.003;
    let result = hole_with(stored, 16.0, 1.0, &[[0.0, 0.0]], &[]);
    match &bore(hole(&result)).head {
        Some(HoleHead::Counterbore { diameter, depth }) => {
            assert!(near(mm(diameter.as_ref()), 8.0));
            assert!(near(mm(depth.as_ref()), 3.0));
        }
        other => panic!("expected a counterbore, found {other:?}"),
    }
}

#[test]
fn counterbore_depth_prefers_its_dimension() {
    let mut stored = props(2, 0.004);
    stored.head_diameter = 0.008;
    stored.head_depth = 0.001;
    let depth = (124, Dress::Value(Slot::Dim(0.003)));
    let result = hole_with(stored, 16.0, 1.0, &[[0.0, 0.0]], &[depth]);
    match &bore(hole(&result)).head {
        Some(HoleHead::Counterbore { depth, .. }) => assert!(near(mm(depth.as_ref()), 3.0)),
        other => panic!("expected a counterbore, found {other:?}"),
    }
}

#[test]
fn countersink_eight_millimetres_ninety_degrees() {
    let mut stored = props(1, 0.004);
    stored.head_diameter = 0.008;
    stored.sink_degrees = 90.0;
    let result = hole_with(stored, 16.0, 1.0, &[[0.0, 0.0]], &[]);
    match &bore(hole(&result)).head {
        Some(HoleHead::Countersink(cone)) => {
            assert!(near(mm(cone.diameter.as_ref()), 8.0));
            assert!(near(radians(cone.angle.as_ref()), FRAC_PI_2));
        }
        other => panic!("expected a countersink, found {other:?}"),
    }
}

#[test]
fn countersink_angle_prefers_its_dimension_in_radians() {
    let mut stored = props(1, 0.004);
    stored.head_diameter = 0.008;
    stored.sink_degrees = 90.0;
    let angle = (152, Dress::Value(Slot::Dim(FRAC_PI_3)));
    let result = hole_with(stored, 16.0, 1.0, &[[0.0, 0.0]], &[angle]);
    match &bore(hole(&result)).head {
        Some(HoleHead::Countersink(cone)) => assert!(near(radians(cone.angle.as_ref()), FRAC_PI_3)),
        other => panic!("expected a countersink, found {other:?}"),
    }
}

#[test]
fn tapped_hole_with_counterbore_states_bore_head_and_a_thread_loss() {
    let mut stored = props(3, 0.005);
    stored.head_diameter = 0.010;
    stored.head_depth = 0.004;
    let result = hole_with(stored, 16.0, 1.0, &[[0.0, 0.0]], &[]);
    let hole = hole(&result);
    assert!(near(mm(bore(hole).diameter.as_ref()), 5.0));
    match &bore(hole).head {
        Some(HoleHead::Counterbore { diameter, depth }) => {
            assert!(near(mm(diameter.as_ref()), 10.0));
            assert!(near(mm(depth.as_ref()), 4.0));
        }
        other => panic!("expected a counterbore, found {other:?}"),
    }
    assert_eq!(hole.thread, None);
    let found = codes(&result);
    assert_eq!(
        found
            .iter()
            .filter(|code| *code == "feature.thread-unread")
            .count(),
        1
    );
}

#[test]
fn tapped_hole_without_a_head_is_a_plain_bore() {
    let result = hole_with(props(3, 0.005), 16.0, 1.0, &[[0.0, 0.0]], &[]);
    assert_eq!(bore(hole(&result)).head, Some(HoleHead::Plain));
}

#[test]
fn two_positions_state_two_points_in_profile_order() {
    let result = hole_with(
        props(0, 0.004),
        16.0,
        1.0,
        &[[0.01, 0.02], [0.03, 0.04]],
        &[],
    );
    let at = points(&result, hole(&result));
    assert_eq!(at.len(), 2);
    assert!(near(at[0][0], 10.0) && near(at[0][1], 20.0));
    assert!(near(at[1][0], 30.0) && near(at[1][1], 40.0));
}

#[test]
fn profile_without_hole_positions_stays_unresolved() {
    let result = hole_with(props(0, 0.004), 16.0, 1.0, &[], &[]);
    assert!(unresolved(&result, OperationFamily::Hole));
    assert!(codes(&result).contains(&"feature.dressup-unread".to_owned()));
}

#[test]
fn unknown_hole_type_stays_unresolved_and_names_the_feature() {
    let result = hole_with(props(9, 0.004), 16.0, 1.0, &[[0.0, 0.0]], &[]);
    assert!(unresolved(&result, OperationFamily::Hole));
    let notes: Vec<_> = result
        .report()
        .losses
        .iter()
        .filter(|loss| loss.code.local_code() == "feature.dressup-unread")
        .collect();
    assert_eq!(notes.len(), 1);
    assert!(notes[0].message.contains("0xa"), "{}", notes[0].message);
}

#[test]
fn dimension_with_p_144_reads() {
    let depth = (264, Dress::Value(Slot::DimCompact(0.008)));
    let result = hole_with(props(0, 0.004), 13.0, 1.0, &[[0.0, 0.0]], &[depth]);
    assert!(near(blind_mm(hole(&result)), 8.0));
}

#[test]
fn dimension_with_p_148_reads() {
    let depth = (264, Dress::Value(Slot::Dim(0.009)));
    let result = hole_with(props(0, 0.004), 13.0, 1.0, &[[0.0, 0.0]], &[depth]);
    assert!(near(blind_mm(hole(&result)), 9.0));
}

#[test]
fn dimension_whose_repeated_target_differs_does_not_read() {
    let depth = (264, Dress::Value(Slot::DimBroken(0.008)));
    let result = hole_with(props(0, 0.004), 13.0, 1.0, &[[0.0, 0.0]], &[depth]);
    assert!(unresolved(&result, OperationFamily::Hole));
    assert!(codes(&result).contains(&"feature.dressup-unread".to_owned()));
}

/// A round with one set per radius in `radii` (metres), each of kind
/// `kinds[k]`.
fn round_with(radii: &[Slot], kinds: &[f64]) -> DecodeResult {
    let n = radii.len();
    let mut slots = Vec::new();
    for k in 0..n {
        slots.push((72 + 20 * k, Dress::Proxy));
        slots.push((84 + 20 * n + 20 * k, Dress::Value(Slot::Double(kinds[k]))));
        slots.push((96 + 40 * n + 20 * k, Dress::Value(radii[k])));
    }
    decode(dressup_document(ROUND, None, &slots))
}

fn fillet(result: &DecodeResult) -> &Fillet {
    match operation(result) {
        FeatureOperation::Fillet(fillet) => fillet,
        other => panic!("expected a fillet, found {other:?}"),
    }
}

fn radius(set: &FilletSet) -> f64 {
    match set {
        FilletSet::Edges { edges, section, .. } => {
            assert_eq!(edges.picks, Picks::Unread);
            match &section.size {
                Some(FilletSize::Radius(Dimension::Literal(radius))) => radius.get(),
                other => panic!("expected a constant radius, found {other:?}"),
            }
        }
        other => panic!("expected an edge set, found {other:?}"),
    }
}

#[test]
fn round_with_one_set_of_two_millimetres() {
    let result = round_with(&[Slot::Dim(0.002)], &[1.0]);
    let fillet = fillet(&result);
    assert_eq!(fillet.sets.len(), 1);
    assert!(near(radius(&fillet.sets[0]), 2.0));
    let found = codes(&result);
    assert_eq!(
        found
            .iter()
            .filter(|code| *code == "feature.selection-unread")
            .count(),
        1
    );
    assert!(!found.contains(&"feature.operands-pending".to_owned()));
}

#[test]
fn round_with_two_sets() {
    let result = round_with(&[Slot::Dim(0.002), Slot::Dim(0.003)], &[1.0, 1.0]);
    let fillet = fillet(&result);
    assert_eq!(fillet.sets.len(), 2);
    assert!(near(radius(&fillet.sets[0]), 2.0));
    assert!(near(radius(&fillet.sets[1]), 3.0));
}

#[test]
fn round_set_of_another_kind_stays_unresolved() {
    let result = round_with(&[Slot::Dim(0.002), Slot::Dim(0.003)], &[1.0, 2.0]);
    assert!(unresolved(&result, OperationFamily::Fillet));
    let found = codes(&result);
    assert!(found.contains(&"feature.dressup-unread".to_owned()));
    assert!(!found.contains(&"feature.selection-unread".to_owned()));
    assert!(found.contains(&"feature.operands-pending".to_owned()));
}

#[test]
fn round_set_without_a_radius_stays_unresolved() {
    let result = round_with(&[Slot::Empty], &[1.0]);
    assert!(unresolved(&result, OperationFamily::Fillet));
    assert!(codes(&result).contains(&"feature.dressup-unread".to_owned()));
}

/// A chamfer of `kind` with the given setbacks.
fn chamfer_with(kind: f64, first: Slot, second: Slot) -> DecodeResult {
    decode(dressup_document(
        CHAMFER,
        None,
        &[
            (68, Dress::Value(Slot::Double(kind))),
            (96, Dress::Proxy),
            (128, Dress::Proxy),
            (156, Dress::Value(first)),
            (184, Dress::Value(second)),
        ],
    ))
}

fn chamfer(result: &DecodeResult) -> &Chamfer {
    match operation(result) {
        FeatureOperation::Chamfer(chamfer) => chamfer,
        other => panic!("expected a chamfer, found {other:?}"),
    }
}

fn length(dimension: &Dimension<cadmpeg_ir::scalar::PositiveLength>) -> f64 {
    match dimension {
        Dimension::Literal(value) => value.get(),
        other => panic!("expected a literal length, found {other:?}"),
    }
}

#[test]
fn equal_distance_chamfer() {
    let result = chamfer_with(41.0, Slot::Dim(0.003), Slot::Empty);
    let set = &chamfer(&result).sets[0];
    assert_eq!(set.edges.picks, Picks::Unread);
    assert!(set.reference.is_none());
    match set.size.as_ref().unwrap() {
        ChamferSize::Distance(distance) => assert!(near(length(distance), 3.0)),
        other => panic!("expected one distance, found {other:?}"),
    }
    assert!(codes(&result).contains(&"feature.selection-unread".to_owned()));
}

#[test]
fn two_distance_chamfer() {
    let result = chamfer_with(43.0, Slot::Dim(0.003), Slot::Dim(0.005));
    let set = &chamfer(&result).sets[0];
    assert_eq!(set.reference.as_ref().unwrap().picks, Picks::Unread);
    match set.size.as_ref().unwrap() {
        ChamferSize::Distances { first, second } => {
            assert!(near(length(first), 3.0));
            assert!(near(length(second), 5.0));
        }
        other => panic!("expected two distances, found {other:?}"),
    }
}

#[test]
fn distance_and_angle_chamfer() {
    let result = chamfer_with(42.0, Slot::Dim(0.003), Slot::Dim(FRAC_PI_4));
    let set = &chamfer(&result).sets[0];
    assert_eq!(set.reference.as_ref().unwrap().picks, Picks::Unread);
    match set.size.as_ref().unwrap() {
        ChamferSize::DistanceAngle { distance, angle } => {
            assert!(near(length(distance), 3.0));
            assert!(near(radians(Some(angle)), FRAC_PI_4));
        }
        other => panic!("expected a distance and angle, found {other:?}"),
    }
}

#[test]
fn unknown_chamfer_type_stays_unresolved() {
    let result = chamfer_with(44.0, Slot::Dim(0.003), Slot::Empty);
    assert!(unresolved(&result, OperationFamily::Chamfer));
    assert!(codes(&result).contains(&"feature.dressup-unread".to_owned()));
}
