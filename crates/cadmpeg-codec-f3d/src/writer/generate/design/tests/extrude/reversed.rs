// SPDX-License-Identifier: Apache-2.0
//! An extrude that runs against its sketch's normal: a negative
//! `AlongDistance` whose side tokens run against the ring (f3d.md §3.1,
//! "Side-face walk"); and an extrude whose sketch frame is offset from the
//! model origin.

use cadmpeg_ir::design::dimension::Dimension;
use cadmpeg_ir::design::feature::FeatureOperation;
use cadmpeg_ir::design::ops::shared::{Limit, Span};
use cadmpeg_ir::design::ops::sketch::SketchPlane;
use cadmpeg_ir::design::ops::swept::ExtrudeDirection;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::features::{
    ExtrudeDirection as DecodedDirection, ExtrudeExtent, FeatureDefinition,
    FeatureOperation as DecodedOperation, LinearTermination, ParameterValue,
};
use cadmpeg_ir::scalar::Length;

use super::super::sketch::{frame, sketch_id};
use super::super::{decoded as decoded_parameter, loss, write};
use super::arcs::{extruded_between, slot_loop, slot_strokes, Side, Stroke};
use super::faces::{corners, flat, lines, sides_follow_curves};
use super::{decoded_extrude, kinds, EVALUATION_MISSING, SIDE, TOLERANCE};

/// The square of the prism's section, counterclockwise.
const SQUARE: [[f64; 2]; 4] = [[0.0, 0.0], [SIDE, 0.0], [SIDE, SIDE], [0.0, SIDE]];

/// The square drawn as `c0` to `c3` over a prism from `z = levels[0]` to
/// `z = levels[1]`, extruded `D1 = 10 mm` from the sketch on `XY`.
fn square_prism(levels: [f64; 2]) -> CadIr {
    extruded_between(&lines(&SQUARE), &[flat(&SQUARE)], levels)
}

/// The source extrude.
fn source_extrude(ir: &mut CadIr) -> &mut cadmpeg_ir::design::ops::swept::Extrude {
    ir.design
        .features
        .iter_mut()
        .find_map(|feature| match &mut feature.definition.operation {
            FeatureOperation::Extrude(extrude) => Some(extrude.as_mut()),
            _ => None,
        })
        .expect("the source extrude")
}

fn reverse(ir: &mut CadIr) {
    source_extrude(ir).direction = Some(ExtrudeDirection::ProfileNormal {
        reversed: Some(Dimension::Literal(true)),
    });
}

/// The decoded extrude's direction and one-sided blind length.
fn travel(decoded: &CadIr) -> (DecodedDirection, f64) {
    let FeatureDefinition::Operation(DecodedOperation::Extrude {
        direction, extent, ..
    }) = decoded_extrude(decoded).evaluation.definition()
    else {
        panic!("an Extrude");
    };
    let ExtrudeExtent::OneSided { side } = extent else {
        panic!("one side: {extent:?}");
    };
    let LinearTermination::Blind { length } = &side.termination else {
        panic!("blind: {side:?}");
    };
    (direction.clone(), length.get())
}

/// The expression and length in mm of the decoded extrude's owned length
/// parameter.
fn owned_length(decoded: &CadIr) -> (String, f64) {
    let extrude = decoded_extrude(decoded).id.clone();
    let lengths = decoded
        .model
        .parameters
        .iter()
        .filter(|parameter| parameter.owner.as_ref() == Some(&extrude))
        .filter_map(|parameter| match &parameter.value {
            Some(ParameterValue::Length(length)) => {
                Some((parameter.expression.clone(), length.get()))
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    let [length] = lengths.as_slice() else {
        panic!("one owned length: {lengths:?}");
    };
    length.clone()
}

/// A prism below its sketch plane, extruded against the sketch normal, is
/// a reversed one-sided blind Extrude of the source depth. Its distance
/// parameter is `-D1`, `D1` stays a user parameter, and the side tokens
/// run against the ring from the same token `3`.
#[test]
fn a_prism_against_its_sketch_normal_is_a_reversed_extrude() {
    let mut ir = square_prism([-SIDE, 0.0]);
    reverse(&mut ir);
    let (report, decoded) = write(&ir);
    assert_eq!(
        kinds(&decoded),
        ["sketch", "extrude"],
        "{:#?}",
        report.losses
    );
    let (direction, length) = travel(&decoded);
    assert_eq!(direction, DecodedDirection::ReversedProfileNormal {});
    assert!((length - SIDE).abs() <= TOLERANCE, "{length}");
    let (expression, along) = owned_length(&decoded);
    assert_eq!(expression, "-D1");
    assert!((along + SIDE).abs() <= TOLERANCE, "{along}");
    let (_, depth) = decoded_parameter(&decoded, "D1").expect("D1 decodes");
    assert!((depth - SIDE).abs() <= TOLERANCE, "{depth}");
    assert!(decoded
        .model
        .parameters
        .iter()
        .any(|parameter| parameter.name == "D1" && parameter.owner.is_none()));
    // The walk starts on c3, the curve written last, as for travel along
    // the normal, and then runs clockwise.
    sides_follow_curves(
        &decoded,
        &[
            (3, [0.0, SIDE], [0.0, 0.0]),
            (4, [SIDE, SIDE], [0.0, SIDE]),
            (5, [SIDE, 0.0], [SIDE, SIDE]),
            (6, [0.0, 0.0], [SIDE, 0.0]),
        ],
    );
    let end = corners(&decoded, "2");
    assert!(!end.is_empty());
    assert!(
        end.iter()
            .all(|corner| (corner[2] + SIDE).abs() <= TOLERANCE),
        "{end:?}"
    );
}

/// A literal depth against the normal is a negative literal distance.
#[test]
fn a_literal_depth_against_the_normal_is_a_negative_distance() {
    let mut ir = square_prism([-SIDE, 0.0]);
    reverse(&mut ir);
    source_extrude(&mut ir).span = Some(Span::Between {
        start: Box::new(Limit::AtOrigin),
        end: Box::new(Limit::Amount(Dimension::Literal(
            Length::new(SIDE).expect("a length"),
        ))),
    });
    let (report, decoded) = write(&ir);
    assert_eq!(
        kinds(&decoded),
        ["sketch", "extrude"],
        "{:#?}",
        report.losses
    );
    let (direction, length) = travel(&decoded);
    assert_eq!(direction, DecodedDirection::ReversedProfileNormal {});
    assert!((length - SIDE).abs() <= TOLERANCE, "{length}");
    let (expression, along) = owned_length(&decoded);
    assert_eq!(expression, "-10 mm");
    assert!((along + SIDE).abs() <= TOLERANCE, "{along}");
}

/// A reversed extrude whose solid lies along the normal has no start cap
/// facing the normal on its sketch plane.
#[test]
fn a_reversed_extrude_over_a_solid_along_the_normal_stays_in_the_base_feature() {
    let mut ir = square_prism([0.0, SIDE]);
    reverse(&mut ir);
    let (report, decoded) = write(&ir);
    assert_eq!(kinds(&decoded), ["sketch", "base"], "{:#?}", report.losses);
    let missing = loss(&report, EVALUATION_MISSING).join("\n");
    assert!(
        missing.contains("its faces do not match its profile"),
        "{missing}"
    );
}

/// Sets the source sketch's frame to the `XY` frame at `origin`.
fn place(ir: &mut CadIr, origin: [f64; 3]) {
    let sketch = ir
        .design
        .sketches
        .iter_mut()
        .find(|sketch| sketch.id == sketch_id("profile"))
        .expect("the profile sketch");
    let SketchPlane::Placed(construction) = &mut sketch.plane else {
        panic!("a placed sketch");
    };
    construction.resolved = Some(frame(origin, [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]));
}

/// A sketch frame whose origin lies on `XY` away from the model origin
/// writes its points moved by that origin, and the faces match the curves
/// at their model positions, along and against the normal.
#[test]
fn a_sketch_origin_off_the_model_origin_moves_the_profile() {
    let offset = [5.0, 3.0];
    let moved = SQUARE.map(|[u, v]| [u + offset[0], v + offset[1]]);
    for (levels, reversed) in [([0.0, SIDE], false), ([-SIDE, 0.0], true)] {
        let mut ir = extruded_between(&lines(&SQUARE), &[flat(&moved)], levels);
        place(&mut ir, [offset[0], offset[1], 0.0]);
        if reversed {
            reverse(&mut ir);
        }
        let (report, decoded) = write(&ir);
        assert_eq!(
            kinds(&decoded),
            ["sketch", "extrude"],
            "{:#?}",
            report.losses
        );
        let [a, b, c, d] = moved;
        let expected = if reversed {
            [(3, d, a), (4, c, d), (5, b, c), (6, a, b)]
        } else {
            [(3, d, a), (4, a, b), (5, b, c), (6, c, d)]
        };
        sides_follow_curves(&decoded, &expected);
    }
}

/// A sketch frame off its origin plane is not written, and its extrude stays
/// in the Base Feature.
#[test]
fn a_sketch_frame_off_its_origin_plane_keeps_the_base_feature() {
    let mut ir = square_prism([0.0, SIDE]);
    place(&mut ir, [0.0, 0.0, 5.0]);
    let (report, decoded) = write(&ir);
    assert_eq!(kinds(&decoded), ["base"], "{:#?}", report.losses);
    let missing = loss(&report, EVALUATION_MISSING).join("\n");
    assert!(missing.contains("its sketch is not written"), "{missing}");
}

/// The residual note of an Extrude whose side tokens under a negative
/// `AlongDistance` follow the walk inferred for hole loops and loops with
/// arcs.
const REVERSED_RESIDUAL: &str = "hole loops and loops with arcs under its negative AlongDistance";

/// A slot below its sketch plane, extruded against the sketch normal, is a
/// reversed Extrude of the source depth; its side tokens follow the walk
/// inferred for a loop with arcs, which the residual note names.
#[test]
fn a_slot_against_its_sketch_normal_is_a_reversed_extrude_with_the_residual_note() {
    let mut ir = extruded_between(
        &slot_strokes([0.0, 0.0], 3.0),
        &[slot_loop([0.0, 0.0], 3.0, false)],
        [-SIDE, 0.0],
    );
    reverse(&mut ir);
    let (report, decoded) = write(&ir);
    assert_eq!(
        kinds(&decoded),
        ["sketch", "extrude"],
        "{:#?}",
        report.losses
    );
    let (direction, length) = travel(&decoded);
    assert_eq!(direction, DecodedDirection::ReversedProfileNormal {});
    assert!((length - SIDE).abs() <= TOLERANCE, "{length}");
    assert_eq!(owned_length(&decoded).0, "-D1");
    let residual = loss(&report, "export/record.residual-unavailable").join("\n");
    assert!(
        residual.contains(REVERSED_RESIDUAL) && residual.contains("DR-883"),
        "{residual}"
    );
}

/// A square with a square hole below its sketch plane, extruded against
/// the sketch normal, is a reversed Extrude; its hole's side tokens follow
/// the inferred walk, which the residual note names.
#[test]
fn a_square_with_a_hole_against_its_sketch_normal_is_a_reversed_extrude_with_the_residual_note() {
    let square = |corners: [[f64; 2]; 4]| -> Vec<Stroke> {
        (0..4)
            .map(|i| Stroke::Line(corners[i], corners[(i + 1) % 4]))
            .collect()
    };
    let outer = [[0.0, 0.0], [20.0, 0.0], [20.0, 20.0], [0.0, 20.0]];
    let inner = [[5.0, 5.0], [15.0, 5.0], [15.0, 15.0], [5.0, 15.0]];
    let strokes = [square(outer), square(inner)].concat();
    let lines = |corners: &[[f64; 2]]| {
        corners
            .iter()
            .map(|corner| (*corner, Side::Line))
            .collect::<Vec<_>>()
    };
    let mut ir = extruded_between(
        &strokes,
        &[
            lines(&outer),
            lines(&[inner[0], inner[3], inner[2], inner[1]]),
        ],
        [-SIDE, 0.0],
    );
    reverse(&mut ir);
    let (report, decoded) = write(&ir);
    assert_eq!(
        kinds(&decoded),
        ["sketch", "extrude"],
        "{:#?}",
        report.losses
    );
    let (direction, length) = travel(&decoded);
    assert_eq!(direction, DecodedDirection::ReversedProfileNormal {});
    assert!((length - SIDE).abs() <= TOLERANCE, "{length}");
    let residual = loss(&report, "export/record.residual-unavailable").join("\n");
    assert!(
        residual.contains(REVERSED_RESIDUAL) && residual.contains("DR-883"),
        "{residual}"
    );
}
