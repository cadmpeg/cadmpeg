// SPDX-License-Identifier: Apache-2.0
//! The dimensions of a written sketch as Fusion sketch dimensions (f3d.md
//! §3.1, "Sketch dimensions"), decoded back and read from the archive's
//! Design segment.

use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_ir::design::container::Container;
use cadmpeg_ir::design::dimension::Dimension;
use cadmpeg_ir::design::expr::{BinaryOp, Node};
use cadmpeg_ir::design::ids::ContainerId;
use cadmpeg_ir::design::ops::sketch::{
    AngleOperands, DefiningRole, DistanceAxis, Planar, PlanarCurve, RadialMeasure, SketchRelation,
};
use cadmpeg_ir::design::parameter::{
    DesignParameter, DimensionDisplay, ParameterDefinition, ParameterOwner,
};
use cadmpeg_ir::design::refs::Member;
use cadmpeg_ir::design::selection::PrincipalEntity;
use cadmpeg_ir::design::units::{Quantity, UnitSymbol, ValueKind};
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::features::parameters::ParameterValue;
use cadmpeg_ir::report::export::ExportReport;
use cadmpeg_ir::scalar::{Angle, PositiveLength};
use cadmpeg_ir::sketches::SketchConstraintDefinitionInput as Kind;

use super::roster::{segment, Segment};
use super::sketch::{constraint_id, fixed_clock_tables, frame, picked, point2, principal, Source};
use super::{
    loss, neutral, number, parameter, parameter_id, reference, scoped_cube, u32_at, u64_at, write,
    write_bytes,
};
use crate::loss::F3dLossCode;

const TOLERANCE: f64 = 1e-9;
const SKETCH: &str = "dims";
/// The tilt of line `m` from the sketch's x axis.
const TILT: f64 = std::f64::consts::FRAC_PI_6;
/// The rectangle's diagonal, in mm.
fn diagonal() -> f64 {
    40.0_f64.hypot(20.0)
}
/// The dimension constraints and the parameters they bind.
const DIMENSIONS: [(&str, &str); 7] = [
    ("side", "Side"),
    ("gap", "Gap"),
    ("across", "Across"),
    ("up", "Up"),
    ("hole", "Hole"),
    ("bend", "Bend"),
    ("tilt", "Tilt"),
];

/// Every `writer.sketch-content-omitted` note of `report`.
fn omitted(report: &ExportReport) -> String {
    loss(
        report,
        &F3dLossCode::WriterSketchContentOmitted.kind().to_string(),
    )
    .join("\n")
}

fn length(key: &str, name: &str, millimetres: f64) -> DesignParameter {
    parameter(
        key,
        name,
        neutral(number(millimetres, Some(UnitSymbol::Mm))),
        Some(millimetres),
    )
}

fn bound<T>(key: &str) -> Dimension<T> {
    Dimension::Parameter(parameter_id(key))
}

/// The unit cube with one sketch on `plane`, whose x and y axes are `x` and
/// `y`: a 40 by 20 rectangle `p1`..`p4`, `l1`..`l4`; a circle of diameter 8
/// about `cc` (15, 10); an arc of radius 3 about `ac` (30, 10); and a line
/// `m` at 30° to `l1`. Its seven dimensions bind `Side` (the diagonal `p1`
/// to `p3`), `Gap = 20 mm` (`l1` to `l3`), `Across = 15 mm` (horizontal, `p1`
/// to `cc`), `Up = 10 mm` (vertical, `p1` to `cc`), `Hole = 8 mm`
/// (diameter), `Bend = 3 mm` (radius) and `Tilt = 30 deg` (`l1` to `m`).
fn dimensioned(plane: PrincipalEntity, x: [f64; 3], y: [f64; 3]) -> CadIr {
    let mut ir = scoped_cube();
    let mut source = Source::new(SKETCH, picked(principal(plane)), frame([0.0; 3], x, y));
    let corners = [(0.0, 0.0), (40.0, 0.0), (40.0, 20.0), (0.0, 20.0)];
    for (index, (u, v)) in corners.iter().enumerate() {
        let start_of = format!("l{}", index + 1);
        let end_of = format!("l{}", (index + 3) % 4 + 1);
        source.entity(
            &format!("p{}", index + 1),
            PlanarCurve::Point(point2(*u, *v)),
            &[
                (start_of.as_str(), DefiningRole::Start),
                (end_of.as_str(), DefiningRole::End),
            ],
        );
    }
    for (index, start) in corners.iter().enumerate() {
        let end = corners[(index + 1) % 4];
        source.entity(
            &format!("l{}", index + 1),
            PlanarCurve::Line {
                start: point2(start.0, start.1),
                end: point2(end.0, end.1),
            },
            &[],
        );
    }
    let (sin, cos) = TILT.sin_cos();
    let tip = (50.0 + 20.0 * cos, 20.0 * sin);
    source
        .entity(
            "cc",
            PlanarCurve::Point(point2(15.0, 10.0)),
            &[("c", DefiningRole::Center)],
        )
        .entity(
            "c",
            PlanarCurve::Circle {
                center: point2(15.0, 10.0),
                radius: PositiveLength::new(4.0).expect("a radius"),
            },
            &[],
        )
        .entity(
            "ac",
            PlanarCurve::Point(point2(30.0, 10.0)),
            &[("a", DefiningRole::Center)],
        )
        .entity(
            "a",
            PlanarCurve::Arc {
                center: point2(30.0, 10.0),
                radius: PositiveLength::new(3.0).expect("a radius"),
                start: Angle::ZERO,
                end: Angle::QUARTER_TURN,
            },
            &[],
        )
        .entity(
            "q1",
            PlanarCurve::Point(point2(50.0, 0.0)),
            &[("m", DefiningRole::Start)],
        )
        .entity(
            "q2",
            PlanarCurve::Point(point2(tip.0, tip.1)),
            &[("m", DefiningRole::End)],
        )
        .entity(
            "m",
            PlanarCurve::Line {
                start: point2(50.0, 0.0),
                end: point2(tip.0, tip.1),
            },
            &[],
        );
    let distance = |a: &str, b: &str, along, key: &str| SketchRelation::<Planar>::Distance {
        a: source.operand(a),
        b: source.operand(b),
        along: Some(along),
        doubled: None,
        value: bound(key),
    };
    let relations = [
        ("side", distance("p1", "p3", DistanceAxis::Aligned, "side")),
        ("gap", distance("l1", "l3", DistanceAxis::Aligned, "gap")),
        (
            "across",
            distance("p1", "cc", DistanceAxis::Horizontal, "across"),
        ),
        ("up", distance("p1", "cc", DistanceAxis::Vertical, "up")),
        (
            "hole",
            SketchRelation::Radial {
                of: super::sketch::entity_id(SKETCH, "c"),
                measure: RadialMeasure::Diameter,
                value: bound("hole"),
            },
        ),
        (
            "bend",
            SketchRelation::Radial {
                of: super::sketch::entity_id(SKETCH, "a"),
                measure: RadialMeasure::Radius,
                value: bound("bend"),
            },
        ),
        (
            "tilt",
            SketchRelation::Angle {
                between: AngleOperands::Between([source.operand("l1"), source.operand("m")]),
                sector: None,
                value: bound("tilt"),
            },
        ),
    ];
    for (name, relation) in relations {
        source.constraint(name, relation);
    }
    let id = source.add_to(&mut ir);
    let Some(Container::Ordered { members, .. }) = ir.design.containers.first_mut() else {
        panic!("the cube's timeline");
    };
    members.insert(0, Member::Feature(id));
    let mut tilt = parameter(
        "tilt",
        "Tilt",
        neutral(number(30.0, Some(UnitSymbol::Deg))),
        Some(TILT),
    );
    tilt.kind = ValueKind::Quantity(Quantity::ANGLE);
    ir.design.parameters.extend([
        length("side", "Side", diagonal()),
        length("gap", "Gap", 20.0),
        length("across", "Across", 15.0),
        length("up", "Up", 10.0),
        length("hole", "Hole", 8.0),
        length("bend", "Bend", 3.0),
        tilt,
    ]);
    ir
}

/// The fixture on `XY`.
fn on_xy() -> CadIr {
    dimensioned(PrincipalEntity::XyPlane, [1.0, 0.0, 0.0], [0.0, 1.0, 0.0])
}

/// The kind of a decoded constraint and the parameter it binds.
fn binding(kind: &Kind) -> Option<(&'static str, &cadmpeg_ir::features::ParameterId)> {
    Some(match kind {
        Kind::Distance { parameter, .. } => ("distance", parameter),
        Kind::DistanceLoci { parameter, .. } => ("distance_loci", parameter),
        Kind::HorizontalDistance { parameter, .. } => ("horizontal_distance", parameter),
        Kind::VerticalDistance { parameter, .. } => ("vertical_distance", parameter),
        Kind::Radius { parameter, .. } => ("radius", parameter),
        Kind::Diameter { parameter, .. } => ("diameter", parameter),
        Kind::Angle { parameter, .. } => ("angle", parameter),
        Kind::Native {
            parameter: Some(parameter),
            ..
        } => ("native", parameter),
        _ => return None,
    })
}

/// The kinds of the decoded constraints bound to each parameter, by the
/// parameter's name.
fn bound_kinds(decoded: &CadIr) -> BTreeMap<String, BTreeSet<&'static str>> {
    let mut out = BTreeMap::<String, BTreeSet<&'static str>>::new();
    for constraint in &decoded.model.sketch_constraints {
        let Some((kind, id)) = binding(constraint.definition.kind()) else {
            continue;
        };
        let name = decoded
            .model
            .parameters
            .iter()
            .find(|parameter| &parameter.id == id)
            .map_or_else(|| id.to_string(), |parameter| parameter.name.clone());
        out.entry(name).or_default().insert(kind);
    }
    out
}

#[test]
fn dimensions_decode_back_by_kind() {
    // Each plane with whether the source x axis lies along Fusion's u axis.
    let planes = [
        (
            "XY",
            PrincipalEntity::XyPlane,
            [1.0, 0.0, 0.0],
            [0.0, 1.0, 0.0],
            true,
        ),
        (
            "XZ",
            PrincipalEntity::XzPlane,
            [0.0, 0.0, 1.0],
            [1.0, 0.0, 0.0],
            false,
        ),
        (
            "YZ",
            PrincipalEntity::YzPlane,
            [0.0, 1.0, 0.0],
            [0.0, 0.0, 1.0],
            true,
        ),
        (
            "-Y",
            PrincipalEntity::XzPlane,
            [1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0],
            true,
        ),
    ];
    for (name, plane, x, y, x_along_u) in planes {
        let (report, decoded) = write(&dimensioned(plane, x, y));
        let kinds = bound_kinds(&decoded);
        let (across, up) = if x_along_u {
            ("horizontal_distance", "vertical_distance")
        } else {
            ("vertical_distance", "horizontal_distance")
        };
        for (parameter, kind) in [
            ("Side", "distance_loci"),
            ("Gap", "distance"),
            ("Across", across),
            ("Up", up),
            ("Hole", "diameter"),
            ("Bend", "radius"),
            ("Tilt", "angle"),
        ] {
            assert_eq!(
                kinds.get(parameter),
                Some(&BTreeSet::from([kind])),
                "{name} {parameter}: {kinds:#?}"
            );
        }
        let omitted = omitted(&report);
        for (constraint, _) in DIMENSIONS {
            assert!(
                !omitted.contains(constraint_id(SKETCH, constraint).as_str()),
                "{name}: {omitted}"
            );
        }
    }
}

#[test]
fn a_dimension_parameter_is_owned_by_its_sketch() {
    let (_, decoded) = write(&on_xy());
    let sketch = decoded
        .model
        .features
        .iter()
        .find(|feature| {
            matches!(
                feature.evaluation.definition(),
                cadmpeg_ir::features::FeatureDefinition::Operation(
                    cadmpeg_ir::features::FeatureOperation::Sketch { .. }
                )
            )
        })
        .expect("the decoded sketch");
    for (_, name) in DIMENSIONS {
        let parameter = decoded
            .model
            .parameters
            .iter()
            .find(|parameter| parameter.name == name)
            .unwrap_or_else(|| panic!("{name}: {:#?}", decoded.model.parameters));
        assert_eq!(parameter.owner.as_ref(), Some(&sketch.id), "{name}");
    }
    let tilt = decoded
        .model
        .parameters
        .iter()
        .find(|parameter| parameter.name == "Tilt")
        .expect("Tilt");
    assert!(
        matches!(&tilt.value, Some(ParameterValue::Angle(angle)) if (angle.get() - TILT).abs() <= TOLERANCE),
        "{tilt:#?}"
    );
    assert!(decoded
        .model
        .parameters
        .iter()
        .any(|parameter| parameter.name == "Width" && parameter.owner.is_none()));
}

/// One dimension frame read from the Design segment.
struct WrittenFrame {
    mask: u64,
    prefix: String,
    text: String,
    measurement: Vec<u8>,
    /// The text frame's style byte and four style values.
    style: (u8, [f32; 4]),
}

fn utf16(bytes: &[u8], at: &mut usize) -> String {
    let units = usize::try_from(u32_at(bytes, *at)).expect("length");
    let text = bytes[*at + 4..*at + 4 + 2 * units]
        .chunks_exact(2)
        .map(|unit| u16::from_le_bytes([unit[0], unit[1]]))
        .collect::<Vec<_>>();
    *at += 4 + 2 * units;
    String::from_utf16(&text).expect("UTF-16")
}

fn f64_at(bytes: &[u8], at: usize) -> f64 {
    f64::from_le_bytes(bytes[at..at + 8].try_into().expect("eight bytes"))
}

/// The records of the types whose GUID starts with `prefix`.
fn records_of(design: &Segment, prefix: &str) -> Vec<u64> {
    design
        .primary
        .iter()
        .map(|(index, _)| *index)
        .filter(|index| design.type_of(*index).0.starts_with(prefix))
        .collect()
}

/// Every dimension frame: the measurement of its type (f3d.md §3.1: 129
/// bytes linear, 153 radial or diameter, 146 angular), the 91-byte text
/// frame, the prefix, the text, and the kind block (52 bytes linear, else
/// 32) whose last eight bytes are the mask.
fn written_frames(design: &Segment) -> Vec<WrittenFrame> {
    let mut frames = Vec::new();
    for (prefix, measurement, kind) in [
        ("8C780195", 129, 52),
        ("6EEAFDE5", 146, 32),
        ("664AD49F", 153, 32),
        ("8EDAC24D", 153, 32),
    ] {
        for index in records_of(design, prefix) {
            let bytes = design.record(index);
            let operands = usize::try_from(u32_at(bytes, 20)).expect("count");
            let mut at = 24 + 15 * operands;
            assert_eq!(bytes[at], 0, "record {index}: no property block");
            at += 1;
            let measured = bytes[at..at + measurement].to_vec();
            at += measurement;
            let style = (
                bytes[at],
                [0, 1, 2, 3].map(|value| {
                    let from = at + 1 + 4 * value;
                    f32::from_le_bytes(bytes[from..from + 4].try_into().expect("four bytes"))
                }),
            );
            at += 91;
            let prefix = utf16(bytes, &mut at);
            let text = utf16(bytes, &mut at);
            frames.push(WrittenFrame {
                mask: u64_at(bytes, at + kind - 8),
                prefix,
                text,
                measurement: measured,
                style,
            });
        }
    }
    frames
}

#[test]
fn a_frame_stores_its_text_and_prefix() {
    let (_, decoded, bytes) = write_bytes(&on_xy());
    let design = segment(&bytes, "FusionDesignSegmentType1");
    let frames = written_frames(&design);
    let masks = frames
        .iter()
        .map(|frame| frame.mask)
        .collect::<BTreeSet<_>>();
    assert_eq!(
        masks,
        BTreeSet::from([0x4000, 0x40_0000, 0x80_0000, 0x1_0000, 0x2_0000, 0x8000])
    );
    let expressions = decoded
        .model
        .parameters
        .iter()
        .map(|parameter| parameter.expression.as_str())
        .collect::<BTreeSet<_>>();
    for frame in &frames {
        assert!(
            expressions.contains(frame.text.as_str()),
            "{} among {expressions:?}",
            frame.text
        );
        let expected = match frame.mask {
            0x1_0000 => "\u{d8}",
            0x2_0000 => "R",
            _ => "",
        };
        assert_eq!(frame.prefix, expected, "mask {:#x}", frame.mask);
    }
    let aligned = frames
        .iter()
        .filter(|frame| frame.mask == 0x4000)
        .map(|frame| f64_at(&frame.measurement, 97))
        .collect::<Vec<_>>();
    assert_eq!(aligned.len(), 2);
    for expected in [diagonal() / 10.0, 2.0] {
        assert!(
            aligned
                .iter()
                .any(|value| (value - expected).abs() <= TOLERANCE),
            "{aligned:?}"
        );
    }
    let radius = |mask| {
        frames
            .iter()
            .find(|frame| frame.mask == mask)
            .map(|frame| f64_at(&frame.measurement, 145))
            .expect("a radial frame")
    };
    assert!((radius(0x1_0000) - 0.4).abs() <= TOLERANCE);
    assert!((radius(0x2_0000) - 0.3).abs() <= TOLERANCE);
    let angle = frames
        .iter()
        .find(|frame| frame.mask == 0x8000)
        .map(|frame| f64_at(&frame.measurement, 0))
        .expect("the angular frame");
    assert!((angle - TILT).abs() <= TOLERANCE, "{angle}");
}

#[test]
fn a_referenced_parameter_stays_a_user_parameter() {
    let mut ir = on_xy();
    ir.design.parameters.push(parameter(
        "twice",
        "Twice",
        neutral(Node::Binary {
            op: BinaryOp::Multiply,
            left: Box::new(reference("gap")),
            right: Box::new(number(2.0, None)),
        }),
        Some(40.0),
    ));
    let operand = |name: &str| {
        cadmpeg_ir::design::ops::sketch::Operand::Entity(super::sketch::entity_id(SKETCH, name))
    };
    ir.design
        .sketch_constraints
        .push(cadmpeg_ir::design::ops::sketch::SketchConstraint {
            id: constraint_id(SKETCH, "twice"),
            sketch: super::sketch::sketch_id(SKETCH),
            relation: SketchRelation::Distance {
                a: operand("l2"),
                b: operand("l4"),
                along: Some(DistanceAxis::Aligned),
                doubled: None,
                value: bound("twice"),
            },
            name: None,
            active: None,
            native_options: cadmpeg_ir::design::options::NativeOptions::new(),
            source_settings: cadmpeg_ir::design::options::NativeOptions::new(),
            native_ref: None,
        });
    let (_, decoded, bytes) = write_bytes(&ir);
    let gap = decoded
        .model
        .parameters
        .iter()
        .find(|parameter| parameter.name == "Gap")
        .expect("Gap");
    assert!(gap.owner.is_none(), "{gap:#?}");
    let kinds = bound_kinds(&decoded);
    for name in ["d2", "Twice"] {
        assert_eq!(
            kinds.get(name),
            Some(&BTreeSet::from(["distance"])),
            "{name}: {kinds:#?}"
        );
    }
    let owned = decoded
        .model
        .parameters
        .iter()
        .find(|parameter| parameter.name == "d2")
        .expect("d2");
    assert_eq!(owned.expression, "Gap");
    let frames = written_frames(&segment(&bytes, "FusionDesignSegmentType1"));
    let formula = frames
        .iter()
        .filter(|frame| frame.prefix == "fx: ")
        .map(|frame| frame.text.as_str())
        .collect::<BTreeSet<_>>();
    assert!(formula.contains("Gap"), "{formula:?}");
    assert_eq!(formula.len(), 2, "{formula:?}");
}

#[test]
fn a_dimension_that_disagrees_with_its_geometry_is_omitted() {
    let mut ir = on_xy();
    let side = ir
        .design
        .parameters
        .iter_mut()
        .find(|parameter| parameter.id == parameter_id("side"))
        .expect("Side");
    *side = length("side", "Side", diagonal() + 1.0);
    let (report, decoded) = write(&ir);
    let omitted = omitted(&report);
    assert!(
        omitted.contains(&format!(
            "{}: its value disagrees with its written geometry",
            constraint_id(SKETCH, "side")
        )),
        "{omitted}"
    );
    let side = decoded
        .model
        .parameters
        .iter()
        .find(|parameter| parameter.name == "Side")
        .expect("Side");
    assert!(side.owner.is_none());
    assert_eq!(decoded.model.sketches.len(), 1);
    assert!(bound_kinds(&decoded).contains_key("Gap"));
}

#[test]
fn a_rotated_horizontal_distance_is_omitted() {
    let (sin, cos) = TILT.sin_cos();
    let (report, decoded) = write(&dimensioned(
        PrincipalEntity::XyPlane,
        [cos, sin, 0.0],
        [-sin, cos, 0.0],
    ));
    let omitted = omitted(&report);
    for constraint in ["across", "up"] {
        assert!(
            omitted.contains(constraint_id(SKETCH, constraint).as_str()),
            "{omitted}"
        );
    }
    assert!(
        omitted.contains("its source axis lies along neither axis of Fusion's sketch frame"),
        "{omitted}"
    );
    assert!(bound_kinds(&decoded).contains_key("Side"));
}

#[test]
fn the_container_scope_and_collections_list_the_dimensions() {
    let (_, decoded, bytes) = write_bytes(&on_xy());
    let design = segment(&bytes, "FusionDesignSegmentType1");
    let [scope] = records_of(&design, "8DA771B7")[..] else {
        panic!("one Sketch scope");
    };
    let frame = design.record(scope);
    assert_eq!(u32_at(frame, 22), 7);
    let owners = (0..7)
        .map(|owner| u64_at(frame, 26 + 11 * owner + 1))
        .collect::<Vec<_>>();
    for owner in &owners {
        assert!(design.type_of(*owner).0.starts_with("D91D429C"), "{owner}");
    }
    let table = 144 + 11 * 7;
    assert_eq!(u32_at(frame, table), 2 + 7);
    assert_eq!(
        (0..7)
            .map(|owner| u64_at(frame, table + 4 + 22 + 11 * owner + 1))
            .collect::<Vec<_>>(),
        owners
    );

    let collection = design.record(199);
    let members = (0..usize::try_from(u32_at(collection, 40)).expect("count"))
        .map(|member| u64_at(collection, 44 + 11 * member + 1))
        .collect::<Vec<_>>();
    let at = members
        .iter()
        .position(|member| *member == scope)
        .expect("the scope");
    assert_eq!(members[at + 3..at + 10], owners[..]);

    let parameters = records_of(&design, "B0556F58");
    let [collection] = parameters[..] else {
        panic!("one parameter collection");
    };
    assert_eq!(
        usize::try_from(u32_at(design.record(collection), 30)).expect("count"),
        decoded.model.parameters.len()
    );

    let context = design.record(198);
    let mut at = 21 + 4 + 11 * usize::try_from(u32_at(context, 21)).expect("count");
    let counters = u32_at(context, at);
    at += 4;
    for _ in 0..counters {
        at += 4 + usize::try_from(u32_at(context, at)).expect("length") + 8;
    }
    assert_eq!(u64_at(context, at), 7);
}

#[test]
fn the_fixed_clock_note_names_each_dimension_table() {
    let (report, _, bytes) = write_bytes(&on_xy());
    let frames = written_frames(&segment(&bytes, "FusionDesignSegmentType1")).len();
    // The component and Base Feature tables, three of the sketch and one
    // of each dimension's parameter.
    assert_eq!(fixed_clock_tables(&report, &bytes).len(), 3 + 3 + frames);
}

#[test]
fn a_failure_in_the_dimension_records_keeps_the_sketches() {
    super::super::FAIL_NEXT_DIMENSIONS.with(|fail| fail.set(true));
    let (report, decoded) = write(&on_xy());
    assert_eq!(decoded.model.sketches.len(), 1);
    assert!(
        bound_kinds(&decoded).is_empty(),
        "{:#?}",
        bound_kinds(&decoded)
    );
    let omitted = omitted(&report);
    assert!(
        omitted.contains("the writer failed with the sketch's dimensions (not implemented yet: a test failure of the dimension records)"),
        "{omitted}"
    );
    for (constraint, _) in DIMENSIONS {
        assert!(
            omitted.contains(constraint_id(SKETCH, constraint).as_str()),
            "{omitted}"
        );
    }
    assert!(decoded
        .model
        .parameters
        .iter()
        .any(|parameter| parameter.name == "Side" && parameter.owner.is_none()));
}

#[test]
fn the_residual_note_names_the_ordinals_suffixes_and_angular_point_order() {
    let (report, _) = write(&on_xy());
    let notes = loss(&report, "export/record.residual-unavailable")
        .into_iter()
        .filter(|message| message.contains("DR-917"))
        .collect::<Vec<_>>();
    let [note] = notes.as_slice() else {
        panic!("one sketch dimension note: {:#?}", report.losses);
    };
    for named in [
        "each angular line's point positions, end then start",
        "relation ordinals",
        "the dimension suffix 2 for a sketch's first dimension of a kind",
    ] {
        assert!(note.contains(named), "{named}: {note}");
    }
}

/// The driven state of each decoded constraint bound to a parameter, by the
/// parameter's name.
fn driving(decoded: &CadIr) -> BTreeMap<String, Option<bool>> {
    decoded
        .model
        .sketch_constraints
        .iter()
        .filter_map(|constraint| {
            let (_, id) = binding(constraint.definition.kind())?;
            let name = decoded
                .model
                .parameters
                .iter()
                .find(|parameter| &parameter.id == id)?
                .name
                .clone();
            Some((name, constraint.driving))
        })
        .collect()
}

/// The ten-byte expression trailer of each written dimension parameter, by
/// the parameter's name, read in the eleven-byte discriminated form of the
/// parameter type's version 8 (f3d.md §3.1).
fn dimension_trailers(design: &Segment) -> BTreeMap<String, Vec<u8>> {
    let mut trailers = BTreeMap::new();
    for index in records_of(design, "7A6A3D31") {
        let bytes = design.record(index);
        let mut at = 27;
        utf16(bytes, &mut at);
        at += 4;
        at += if bytes[at] == 1 { 11 } else { 1 };
        utf16(bytes, &mut at);
        let trailer = bytes[at..at + 10].to_vec();
        at += 10;
        if !utf16(bytes, &mut at).contains("Dimension") {
            continue;
        }
        at += 4;
        utf16(bytes, &mut at);
        trailers.insert(utf16(bytes, &mut at), trailer);
    }
    trailers
}

/// The design parameter `key` of `ir`.
fn parameter_mut<'a>(ir: &'a mut CadIr, key: &str) -> &'a mut DesignParameter {
    ir.design
        .parameters
        .iter_mut()
        .find(|parameter| parameter.id == parameter_id(key))
        .expect("the parameter")
}

/// A second aligned distance `key` from `l1` to `l3`, bound to the
/// parameter `bound_to`.
fn add_gap_dimension(ir: &mut CadIr, key: &str, bound_to: &str) {
    let operand = |name: &str| {
        cadmpeg_ir::design::ops::sketch::Operand::Entity(super::sketch::entity_id(SKETCH, name))
    };
    ir.design
        .sketch_constraints
        .push(cadmpeg_ir::design::ops::sketch::SketchConstraint {
            id: constraint_id(SKETCH, key),
            sketch: super::sketch::sketch_id(SKETCH),
            relation: SketchRelation::Distance {
                a: operand("l1"),
                b: operand("l3"),
                along: Some(DistanceAxis::Aligned),
                doubled: None,
                value: bound(bound_to),
            },
            name: None,
            active: None,
            native_options: cadmpeg_ir::design::options::NativeOptions::new(),
            source_settings: cadmpeg_ir::design::options::NativeOptions::new(),
            native_ref: None,
        });
}

fn drive(ir: &mut CadIr, key: &str) {
    ir.design
        .parameters
        .iter_mut()
        .find(|parameter| parameter.id == parameter_id(key))
        .expect("the parameter")
        .definition = ParameterDefinition::Driven;
}

#[test]
fn driven_and_driving_dimensions_read_back_with_their_state() {
    // `Gap` (an aligned distance of 20 mm) and `Hole` (a diameter of 8 mm)
    // are driven; the other five dimensions are driving.
    let mut ir = on_xy();
    drive(&mut ir, "gap");
    drive(&mut ir, "hole");
    let (report, decoded, bytes) = write_bytes(&ir);
    let omitted = omitted(&report);
    for (constraint, _) in DIMENSIONS {
        assert!(
            !omitted.contains(constraint_id(SKETCH, constraint).as_str()),
            "{omitted}"
        );
    }
    assert!(
        loss(
            &report,
            &F3dLossCode::WriterParameterOmitted.kind().to_string()
        )
        .is_empty(),
        "{:#?}",
        report.losses
    );
    let states = driving(&decoded);
    for (_, name) in DIMENSIONS {
        let expected = matches!(name, "Gap" | "Hole").then_some(false);
        assert_eq!(states.get(name), Some(&expected), "{name}: {states:#?}");
    }
    for (name, millimetres) in [("Gap", 20.0), ("Hole", 8.0)] {
        let parameter = decoded
            .model
            .parameters
            .iter()
            .find(|parameter| parameter.name == name)
            .unwrap_or_else(|| panic!("{name}"));
        assert_eq!(
            Some(parameter.expression.as_str()),
            super::super::expression::quantity_literal(millimetres, Some(UnitSymbol::Mm))
                .as_deref(),
            "{name}"
        );
    }
    let frames = written_frames(&segment(&bytes, "FusionDesignSegmentType1"));
    let driven = frames
        .iter()
        .filter(|frame| frame.style.0 == 1)
        .map(|frame| (frame.mask, frame.prefix.as_str(), frame.text.as_str()))
        .collect::<BTreeSet<_>>();
    assert_eq!(
        driven,
        BTreeSet::from([(0x4000, "", "20.00 mm"), (0x1_0000, "\u{d8}", "8.00 mm")])
    );
    for frame in &frames {
        let values = if frame.style.0 == 1 {
            [0.25, 0.25, 0.25, 1.0]
        } else {
            [0.0, 0.0, 0.0, 1.0]
        };
        assert_eq!(frame.style.1, values, "mask {:#x}", frame.mask);
    }
    assert_eq!(frames.len(), DIMENSIONS.len());
    let trailers = dimension_trailers(&segment(&bytes, "FusionDesignSegmentType1"));
    for (_, name) in DIMENSIONS {
        let mut expected = vec![0; 10];
        expected[6] = u8::from(matches!(name, "Gap" | "Hole"));
        assert_eq!(trailers.get(name), Some(&expected), "{name}");
    }
}

#[test]
fn a_dimension_whose_parameter_is_not_in_the_design_is_omitted() {
    let mut ir = on_xy();
    ir.design
        .parameters
        .retain(|parameter| parameter.id != parameter_id("up"));
    let (report, decoded) = write(&ir);
    let omitted = omitted(&report);
    assert!(
        omitted.contains(&format!(
            "{}: its parameter is not written as a length",
            constraint_id(SKETCH, "up")
        )),
        "{omitted}"
    );
    assert!(driving(&decoded).contains_key("Gap"));
}

#[test]
fn a_driven_parameter_shared_by_two_dimensions_is_written_with_the_first() {
    let mut ir = on_xy();
    drive(&mut ir, "gap");
    add_gap_dimension(&mut ir, "gap_again", "gap");
    let (report, decoded) = write(&ir);
    let omitted = omitted(&report);
    assert!(
        omitted.contains(&format!(
            "{}: an earlier dimension owns its driven parameter",
            constraint_id(SKETCH, "gap_again")
        )),
        "{omitted}"
    );
    assert!(
        !omitted.contains(&format!("{}:", constraint_id(SKETCH, "gap"))),
        "{omitted}"
    );
    assert_eq!(driving(&decoded).get("Gap"), Some(&Some(false)));
}

#[test]
fn a_driven_parameter_without_a_cached_value_states_the_measurement() {
    let mut ir = on_xy();
    drive(&mut ir, "gap");
    parameter_mut(&mut ir, "gap").value = None;
    let (_, decoded) = write(&ir);
    let gap = decoded
        .model
        .parameters
        .iter()
        .find(|parameter| parameter.name == "Gap")
        .expect("Gap");
    assert_eq!(
        Some(gap.expression.as_str()),
        super::super::expression::quantity_literal(20.0, Some(UnitSymbol::Mm)).as_deref()
    );
    assert_eq!(driving(&decoded).get("Gap"), Some(&Some(false)));
}

/// The record of a driven dimension's parameter keeps the literal its source
/// record stores (the source setting `f3d:literal`, 21 mm), which differs
/// from the measured value (20 mm, the parameter's value) that its frame
/// states. Read back, the parameter is `Driven` with the measured value, and
/// keeps the literal.
#[test]
fn a_driven_parameter_keeps_the_literal_its_source_record_stores() {
    use cadmpeg_ir::design::options::NativeOption;
    let literal = || {
        NativeOption::Text(Dimension::Literal(
            cadmpeg_core::text::NonBlankString::new("21 mm".to_owned()).expect("nonblank"),
        ))
    };
    let mut ir = on_xy();
    drive(&mut ir, "gap");
    parameter_mut(&mut ir, "gap")
        .source_settings
        .insert(cadmpeg_core::nonblank_literal!("f3d:literal"), literal());
    let (_, decoded, bytes) = write_bytes(&ir);
    let gap = decoded
        .model
        .parameters
        .iter()
        .find(|parameter| parameter.name == "Gap")
        .expect("Gap");
    assert_eq!(gap.expression, "21 mm");
    assert!(
        matches!(gap.value, Some(ParameterValue::Length(length))
            if (length.get() - 21.0).abs() < TOLERANCE),
        "{:?}",
        gap.value
    );
    let frames = written_frames(&segment(&bytes, "FusionDesignSegmentType1"));
    let driven = frames
        .iter()
        .filter(|frame| frame.style.0 == 1)
        .collect::<Vec<_>>();
    let [frame] = driven.as_slice() else {
        panic!("one driven frame")
    };
    assert_eq!(frame.text, "20.00 mm");
    assert!((f64_at(&frame.measurement, 97) - 2.0).abs() < TOLERANCE);
    let gap = decoded
        .design
        .parameters
        .iter()
        .find(|parameter| {
            parameter
                .name
                .as_ref()
                .map(cadmpeg_core::text::NonBlankString::as_str)
                == Some("Gap")
        })
        .expect("Gap in the design");
    assert!(matches!(gap.definition, ParameterDefinition::Driven));
    assert!(
        matches!(gap.value, Some(cadmpeg_ir::design::parameter::ParameterValue::Number(value))
            if (value.get() - 20.0).abs() < TOLERANCE),
        "{:?}",
        gap.value
    );
    assert_eq!(
        gap.source_settings
            .get(crate::design_v11::parameters::LITERAL)
            .cloned(),
        Some(literal())
    );
}

#[test]
fn a_driven_parameter_of_another_quantity_is_named_with_its_dimension() {
    let mut ir = on_xy();
    drive(&mut ir, "gap");
    parameter_mut(&mut ir, "gap").kind = ValueKind::Quantity(Quantity::ANGLE);
    let (report, decoded) = write(&ir);
    let omitted = omitted(&report);
    assert!(
        omitted.contains(&format!(
            "{}: its driven parameter is of another quantity",
            constraint_id(SKETCH, "gap")
        )),
        "{omitted}"
    );
    let untaken = loss(
        &report,
        &F3dLossCode::WriterParameterOmitted.kind().to_string(),
    )
    .join("\n");
    assert!(
        untaken.contains("no written dimension binds these")
            && untaken.contains(parameter_id("gap").as_str()),
        "{untaken}"
    );
    assert!(!driving(&decoded).contains_key("Gap"));
}

#[test]
fn a_driven_parameter_keeps_its_comment_and_its_rename_is_reported() {
    let mut ir = on_xy();
    drive(&mut ir, "gap");
    let gap = parameter_mut(&mut ir, "gap");
    gap.name = cadmpeg_core::text::NonBlankString::new("Gap width".to_owned());
    gap.comment = Some("measured".into());
    let (report, decoded) = write(&ir);
    let renamed = loss(&report, "export/parameter.renamed").join("\n");
    assert!(
        renamed.contains(&format!(
            "{} (`Gap width` -> `Gap_width`)",
            parameter_id("gap")
        )),
        "{renamed}"
    );
    assert!(crate::test_support::native_test::f3d_native(&decoded)
        .design_parameters
        .iter()
        .any(
            |parameter| parameter.name() == "Gap_width" && parameter.comment() == Some("measured")
        ));
    assert_eq!(driving(&decoded).get("Gap_width"), Some(&Some(false)));
}

#[test]
fn a_claimed_driving_parameter_keeps_its_comment() {
    let mut ir = on_xy();
    parameter_mut(&mut ir, "up").comment = Some("rise".into());
    let (_, decoded) = write(&ir);
    assert!(crate::test_support::native_test::f3d_native(&decoded)
        .design_parameters
        .iter()
        .any(|parameter| parameter.name() == "Up" && parameter.comment() == Some("rise")));
    assert_eq!(driving(&decoded).get("Up"), Some(&None));
}

#[test]
fn a_driven_parameter_names_its_owner_and_its_display_with_its_dimension() {
    let mut ir = on_xy();
    drive(&mut ir, "gap");
    let timeline = ContainerId::mint("test:design:container#timeline").expect("identity grammar");
    let gap = parameter_mut(&mut ir, "gap");
    gap.owner = Some(ParameterOwner::Container(timeline.clone()));
    gap.display = Some(DimensionDisplay::Radius);
    let (report, decoded) = write(&ir);
    let unwritable = loss(&report, "export/value.unwritable");
    let named = |reason: &str, entry: String| {
        assert!(
            unwritable
                .iter()
                .any(|message| message.contains(reason) && message.contains(&entry)),
            "{reason}: {unwritable:#?}"
        );
    };
    named(
        "driven parameters as parameters of their dimensions without their owners",
        format!("{} (owner container {timeline})", parameter_id("gap")),
    );
    named(
        "driven parameters without their diameter or radius display presentation",
        format!("{} (Radius)", parameter_id("gap")),
    );
    assert_eq!(driving(&decoded).get("Gap"), Some(&Some(false)));
}
