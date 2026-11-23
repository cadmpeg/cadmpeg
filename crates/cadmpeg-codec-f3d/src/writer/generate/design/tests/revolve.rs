// SPDX-License-Identifier: Apache-2.0
//! The sketch-driven `Revolve` a design writes in place of its Base Feature
//! (f3d.md §3.1, "Records of a sketch-driven Revolve"), decoded back and read
//! from the archive's Design segment.

use std::collections::{BTreeMap, BTreeSet};
use std::f64::consts::TAU;

use cadmpeg_ir::design::construction::{
    AxisValue, Construction, ConstructionInput, InputReference, InputRole, Relation, Subject,
};
use cadmpeg_ir::design::container::{Container, OrderedKind, Rollback};
use cadmpeg_ir::design::dimension::Dimension;
use cadmpeg_ir::design::evaluation::{CurrentState, EvaluationStatus};
use cadmpeg_ir::design::feature::FeatureOperation;
use cadmpeg_ir::design::ids::ContainerId;
use cadmpeg_ir::design::ops::shared::{Limit, Span};
use cadmpeg_ir::design::ops::sketch::{DefiningRole, PlanarCurve};
use cadmpeg_ir::design::ops::swept::Revolve;
use cadmpeg_ir::design::options::{BodyForm, Boolean, BooleanOp, NativeOptions, SolidOutput};
use cadmpeg_ir::design::refs::{Member, Position};
use cadmpeg_ir::design::scope::DesignScope;
use cadmpeg_ir::design::selection::{
    Anchor, Members, OrderedSelection, PickTarget, Picked, Picks, PrincipalEntity, Selection,
};
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::features::geometry::FinitePoint3;
use cadmpeg_ir::features::revolve::{RevolveConstruction, RevolveExtent};
use cadmpeg_ir::geometry::analytic::{SphereSurface, TorusSurface};
use cadmpeg_ir::geometry::SolvedSurfaceGeometry;
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::scalar::Angle;
use cadmpeg_ir::units::UnitVector3;

use super::super::item::{Decision, Item};
use super::super::{plan, Fallback};
use super::roster::segment;
use super::sketch::{entity_id, feature, feature_id, frame, picked, point2, principal, Source};
use super::{loss, root, write_bytes};

mod body;

use body::{faces_only, revolved};

const TOLERANCE: f64 = 1e-9;
/// The loss code of a feature written without its state.
const EVALUATION_MISSING: &str = "export/evaluation.missing";
/// The tube's inner and outer radius and its height, in mm.
const INNER: f64 = 2.0;
const OUTER: f64 = 4.0;
const HEIGHT: f64 = 6.0;
/// The offset of the item tail in the written scope frame.
const TAIL: usize = 287;

/// The tube's half-plane rectangle, counterclockwise in `(r, z)`.
fn tube() -> Vec<(f64, f64)> {
    vec![(INNER, 0.0), (OUTER, 0.0), (OUTER, HEIGHT), (INNER, HEIGHT)]
}

/// The solid cylinder's half-plane rectangle, its first side... its last side
/// on the axis.
fn cylinder() -> Vec<(f64, f64)> {
    vec![(0.0, 0.0), (OUTER, 0.0), (OUTER, HEIGHT), (0.0, HEIGHT)]
}

/// A sketch on `XZ`, whose point `(u, v)` lies at `(u, 0, -v)`, of the
/// polygon `corners` of `(r, z)` points: line `l<i>` from corner `i` to the
/// next. `centreline` adds the construction line `axis` parallel to the `z`
/// axis at that radius.
fn profile(corners: &[(f64, f64)], centreline: Option<f64>) -> Source {
    let mut sketch = Source::new(
        "profile",
        picked(principal(PrincipalEntity::XzPlane)),
        frame([0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 0.0, -1.0]),
    );
    let count = corners.len();
    for (index, (r, z)) in corners.iter().enumerate() {
        let before = (index + count - 1) % count;
        let start_of = format!("l{index}");
        let end_of = format!("l{before}");
        sketch.entity(
            &format!("p{index}"),
            PlanarCurve::Point(point2(*r, -z)),
            &[
                (start_of.as_str(), DefiningRole::Start),
                (end_of.as_str(), DefiningRole::End),
            ],
        );
    }
    for (index, (r, z)) in corners.iter().enumerate() {
        let (rn, zn) = corners[(index + 1) % count];
        sketch.entity(
            &format!("l{index}"),
            PlanarCurve::Line {
                start: point2(*r, -z),
                end: point2(rn, -zn),
            },
            &[],
        );
    }
    if let Some(r) = centreline {
        sketch.entity(
            "axis",
            PlanarCurve::Line {
                start: point2(r, 1.0),
                end: point2(r, -HEIGHT - 1.0),
            },
            &[],
        );
        sketch.construction("axis");
    }
    sketch
}

fn selection(target: PickTarget) -> OrderedSelection {
    Selection {
        at: Anchor::Input,
        picks: Picks::Named(
            Members::new(vec![Picked {
                target,
                context: None,
                native: None,
                domain: None,
                witness: None,
                resolved: None,
            }])
            .expect("one pick"),
        ),
        native: None,
    }
}

/// The `+z` axis through the origin.
fn z_axis() -> AxisValue {
    AxisValue {
        origin: FinitePoint3::new(Point3::new(0.0, 0.0, 0.0)).expect("finite"),
        direction: UnitVector3::Z_AXIS,
    }
}

/// A `Coincident` pick of the whole sketch entity `line` of the profile
/// sketch, valued `z_axis`.
fn axis(line: &str) -> Construction<cadmpeg_ir::design::construction::kinds::Axis> {
    Construction {
        inputs: vec![ConstructionInput {
            role: Some(InputRole {
                subject: Subject::Whole,
                relation: Relation::Coincident,
            }),
            reference: Some(InputReference::Picks(selection(PickTarget::SketchEntity(
                entity_id("profile", line),
            )))),
            value: None,
            reversed: None,
        }],
        resolved: Some(z_axis()),
        ..Construction::default()
    }
}

fn turn(angle: f64) -> Span<Angle> {
    Span::Between {
        start: Box::new(Limit::AtOrigin),
        end: Box::new(Limit::Amount(Dimension::Literal(
            Angle::new(angle).expect("finite angle"),
        ))),
    }
}

fn revolve(axis: Option<Construction<cadmpeg_ir::design::construction::kinds::Axis>>) -> Revolve {
    Revolve {
        profile: selection(PickTarget::Sketch(super::sketch::sketch_id("profile"))),
        loops: None,
        axis,
        span: Some(turn(TAU)),
        fit: None,
        output: SolidOutput {
            body: Some(BodyForm::Solid),
            boolean: Some(Boolean::NewBody),
        },
        native_options: NativeOptions::new(),
        source_settings: NativeOptions::new(),
    }
}

/// `body` with a timeline of the `profile` sketch and `revolve`, rolled to
/// its end, and the current state after the revolve.
fn design(mut ir: CadIr, profile: Source, revolve: Revolve) -> CadIr {
    let sketch = profile.add_to(&mut ir);
    ir.design.features.push(feature(
        "revolve",
        FeatureOperation::Revolve(Box::new(revolve)),
    ));
    let revolve = feature_id("revolve");
    let timeline = ContainerId::mint("test:design:container#timeline").expect("identity grammar");
    ir.design.containers.push(Container::Ordered {
        id: timeline.clone(),
        kind: OrderedKind::Timeline,
        members: vec![Member::Feature(sketch), Member::Feature(revolve.clone())],
        folders: Vec::new(),
        rollback: Rollback::End,
        placement: None,
    });
    let body = ir.geometry.bodies[0].id.clone();
    ir.design.scopes.push(DesignScope {
        definition: root(),
        roots: vec![timeline],
        active_configuration: None,
        active_arrangements: BTreeSet::new(),
        occurrences: BTreeMap::new(),
        current: vec![CurrentState {
            at: Position::After(Member::Feature(revolve)),
            bodies: vec![body],
            poses: BTreeMap::new(),
            status: EvaluationStatus::Current,
            native_ref: None,
        }],
        unproduced: BTreeSet::default(),
    });
    ir
}

/// The tube about a construction centreline.
fn tube_design() -> CadIr {
    design(
        revolved(&tube()),
        profile(&tube(), Some(0.0)),
        revolve(Some(axis("axis"))),
    )
}

/// The item the design writes for `ir`'s revolve, or the reason the Base
/// Feature holds it.
fn decided(ir: &CadIr) -> Result<usize, String> {
    let design = plan(ir, &Fallback::default())
        .expect("a plan")
        .expect("a design scope");
    match design.item {
        Decision::Item(Item::Revolve(plan)) => Ok(plan.sides.len()),
        Decision::Item(Item::Extrude(_)) => Err("an Extrude".to_owned()),
        Decision::BaseFeature { reason, .. } => Err(reason),
        Decision::None => Err("no item".to_owned()),
    }
}

fn u32_at(bytes: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(bytes[at..at + 4].try_into().expect("four bytes"))
}

fn u64_at(bytes: &[u8], at: usize) -> u64 {
    u64::from_le_bytes(bytes[at..at + 8].try_into().expect("eight bytes"))
}

fn f64_at(bytes: &[u8], at: usize) -> f64 {
    f64::from_le_bytes(bytes[at..at + 8].try_into().expect("eight bytes"))
}

/// The target of the marked reference at `at`.
fn reference_at(bytes: &[u8], at: usize) -> u64 {
    assert_eq!(bytes[at], 1, "a marked reference at {at}");
    u64_at(bytes, at + 1)
}

/// The decoded v10 `Revolve`: its construction and its operation.
fn decoded_revolve(
    decoded: &CadIr,
) -> (
    &RevolveConstruction,
    &cadmpeg_ir::features::operation_settings::BooleanOp,
) {
    decoded
        .model
        .features
        .iter()
        .find_map(|feature| match feature.evaluation.definition() {
            cadmpeg_ir::features::FeatureDefinition::Operation(
                cadmpeg_ir::features::FeatureOperation::Revolve { construction, op },
            ) => Some((construction, op)),
            _ => None,
        })
        .expect("a decoded Revolve")
}

#[test]
fn a_rectangle_beside_a_centreline_is_a_revolve_of_four_sides() {
    assert_eq!(decided(&tube_design()), Ok(4));
    let (report, decoded, bytes) = write_bytes(&tube_design());
    let missing = loss(&report, EVALUATION_MISSING).join("\n");
    assert!(!missing.contains("feature#revolve"), "{missing}");
    let (construction, op) = decoded_revolve(&decoded);
    assert_eq!(
        *op,
        cadmpeg_ir::features::operation_settings::BooleanOp::NewBody
    );
    let RevolveConstruction::Resolved { extent, axis, .. } = construction else {
        panic!("a resolved revolve: {construction:?}");
    };
    let RevolveExtent::OneSided { termination } = extent else {
        panic!("one side: {extent:?}");
    };
    let cadmpeg_ir::features::extents::AngularTermination::Angle { angle } = termination else {
        panic!("an angle: {termination:?}");
    };
    assert!((angle.get() - TAU).abs() <= TOLERANCE, "{angle:?}");
    assert!(axis.reference.is_some(), "{axis:?}");
    let direction = cadmpeg_ir::features::geometry::FiniteVector3::from(axis.direction).get();
    assert!((direction.z.abs() - 1.0).abs() <= TOLERANCE, "{axis:?}");

    let design = segment(&bytes, "FusionDesignSegmentType1");
    let [(scope, version)] = design.records_of("E3849A15-2FC6-42A0-AF3A-2F1D7273B406")[..] else {
        panic!("one Revolve scope");
    };
    assert_eq!(version, 2);
    let frame = design.record(scope);
    let tracked = design.record(reference_at(frame, TAIL + 19));
    assert_eq!(u32_at(tracked, 21), 5, "four side entries and the body");
    let keys = (0..5)
        .map(|entry| u64_at(tracked, 25 + 19 * entry + 11))
        .collect::<Vec<_>>();
    assert_eq!(keys, [3, 4, 5, 6, 7]);
}

#[test]
fn the_scope_frame_stores_the_stated_fields() {
    let (_, _, bytes) = write_bytes(&tube_design());
    let design = segment(&bytes, "FusionDesignSegmentType1");
    let [(scope, _)] = design.records_of("E3849A15-2FC6-42A0-AF3A-2F1D7273B406")[..] else {
        panic!("one Revolve scope");
    };
    let frame = design.record(scope);
    assert_eq!(frame[11..20], [0; 9]);
    assert_eq!(frame[20], 1);
    assert_eq!(u32_at(frame, 21), 0);
    assert_eq!(u32_at(frame, 25), 4, "new body");
    assert_eq!(u32_at(frame, 29), 2, "fixed angle");
    assert_eq!(frame[33..39], [0, 1, 1, 0, 0, 0]);
    let owner = reference_at(frame, 39);
    let axis_group = reference_at(frame, 50);
    assert_eq!(frame[61], 0);
    let start = reference_at(frame, 62);
    assert_eq!(frame[73], 0);
    assert_eq!(u32_at(frame, 74), 1);
    let profile_group = reference_at(frame, 78);
    assert_eq!(u32_at(frame, 89), 0);
    assert_eq!(u32_at(frame, 93), 1);
    assert_eq!(
        design.type_of(reference_at(frame, 97)).0,
        "D26351F0-5940-4D23-AA20-2C35475A6D9E"
    );
    assert_eq!((u32_at(frame, 108), u32_at(frame, 112)), (0, 0));
    assert_eq!(u32_at(frame, 116), 36);
    assert_eq!(frame[192..195], [0; 3]);
    assert_eq!(u32_at(frame, 195), 6);
    let ordered = (0..6)
        .map(|at| reference_at(frame, 199 + 11 * at))
        .collect::<Vec<_>>();
    assert!(
        ordered.windows(2).all(|pair| pair[0] < pair[1]),
        "{ordered:?}"
    );
    for named in [owner, axis_group, start, profile_group] {
        assert!(ordered.contains(&named), "{named} in {ordered:?}");
    }
    assert_eq!(u32_at(frame, 269), 7);
    assert_eq!(
        String::from_utf16(
            &frame[273..287]
                .chunks(2)
                .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
                .collect::<Vec<_>>()
        )
        .expect("UTF-16"),
        "Revolve"
    );
    assert_eq!(
        design.type_of(owner).0,
        "D91D429C-1BB5-4640-B49F-69AF14F463E9"
    );
    assert_eq!(
        design.type_of(start).0,
        "63D2920D-7EC2-49C4-9F08-315197F9E657"
    );
    let role = |group: u64| u64_at(design.record(group), 53);
    assert_eq!(role(profile_group), 0x0000_0041_0000_0000);
    assert_eq!(role(axis_group), 0x0000_0021_0000_0000);
}

#[test]
fn the_axis_selection_names_the_centreline_and_its_carrier() {
    let (_, _, bytes) = write_bytes(&tube_design());
    let design = segment(&bytes, "FusionDesignSegmentType1");
    let [(scope, _)] = design.records_of("E3849A15-2FC6-42A0-AF3A-2F1D7273B406")[..] else {
        panic!("one Revolve scope");
    };
    let axis_group = design.record(reference_at(design.record(scope), 50));
    let selection = reference_at(axis_group, 25);
    assert_eq!(
        design.type_of(selection),
        ("5A1BF548-241F-46FD-9FB5-E4B05126EB9D", 1)
    );
    let axis = design.record(selection);
    let identity = reference_at(axis, 21);
    assert_eq!(identity, selection + 3);
    assert_eq!(
        design.type_of(identity).0,
        "E2CEFD18-D755-4E09-8E7F-953A2F6D43F8"
    );
    assert_eq!(u64_at(axis, 211), 0x0000_0021_0000_0000);
    assert_eq!(axis[256..259], [1, 0, 0]);
    assert_eq!(reference_at(axis, 271), scope);
    let curve = design.record(identity);
    assert_eq!(curve.len(), 45);
    let carrier = reference_at(axis, 200);
    let record = design.record(carrier);
    assert_eq!(
        design.type_of(carrier).0,
        "50AEE8B4-9456-4DD7-847D-6F6C909BBB9D"
    );
    // The centreline runs from (0, 1) to (0, -7) mm on XZ: from model
    // (0, 0, -1) towards -z, in cm through the XZ frame matrix.
    let at = |offset: usize| [0, 1, 2].map(|axis| f64_at(record, offset + 8 * axis));
    let near = |a: [f64; 3], b: [f64; 3]| a.iter().zip(b).all(|(x, y)| (x - y).abs() <= 1e-12);
    assert!(near(at(21), [0.0, 0.0, -0.1]), "{:?}", at(21));
    assert!(near(at(45), [0.0, 0.0, 0.8]), "{:?}", at(45));
    assert_eq!(record[92], 0, "the XZ frame stores its matrix");
    assert_eq!(record.len(), 232);
    let curve_link = reference_at(record, 221);
    assert_eq!(curve_link, carrier + 1);
    assert_eq!(reference_at(design.record(curve_link), 21), carrier + 2);
    assert_eq!(design.record(carrier + 2)[21..], curve[21..]);
    for (link, offset) in [(carrier + 3, 81), (carrier + 5, 69)] {
        assert_eq!(reference_at(record, offset), link);
        assert_eq!(reference_at(design.record(link), 21), link + 1);
        assert_eq!(design.record(link + 1).len(), 37);
    }
}

#[test]
fn each_group_selection_and_start_record_has_its_own_records_after_it() {
    let (_, _, bytes) = write_bytes(&tube_design());
    let design = segment(&bytes, "FusionDesignSegmentType1");
    let [(scope, _)] = design.records_of("E3849A15-2FC6-42A0-AF3A-2F1D7273B406")[..] else {
        panic!("one Revolve scope");
    };
    let frame = design.record(scope);
    let (profile_group, axis_group, start) = (
        reference_at(frame, 78),
        reference_at(frame, 50),
        reference_at(frame, 62),
    );
    let stamps = "6CCF41D5-40BE-48ED-A834-18F3EAED6C57";
    let companion = "5FBE538E-139B-46D4-BCAF-CC796D1E1F78";
    for record in [profile_group, axis_group, start] {
        assert_eq!(design.type_of(record + 1).0, stamps, "{record}");
        assert_eq!(design.type_of(record + 2).0, companion, "{record}");
    }
    let profile_selection = reference_at(design.record(profile_group), 25);
    assert_eq!(profile_selection, profile_group + 3);
    assert_eq!(axis_group, profile_selection + 4);
    let axis_selection = reference_at(design.record(axis_group), 25);
    assert_eq!(axis_selection, axis_group + 3);
    let owner = reference_at(frame, 39);
    assert_eq!(owner, axis_selection + 4);
    assert_eq!(start, owner + 3);
    // The scope's record-timestamp table, companion and tracked results
    // follow the start record's own records.
    assert_eq!(design.type_of(start + 3).0, stamps);
    assert_eq!(design.type_of(start + 4).0, companion);
    assert_eq!(reference_at(frame, TAIL + 19), start + 5);
}

#[test]
fn a_profile_line_on_the_axis_takes_no_side_entry() {
    let ir = design(
        revolved(&cylinder()),
        profile(&cylinder(), None),
        revolve(Some(axis("l3"))),
    );
    assert_eq!(decided(&ir), Ok(3));
    let (_, decoded, bytes) = write_bytes(&ir);
    decoded_revolve(&decoded);
    let design = segment(&bytes, "FusionDesignSegmentType1");
    let [(scope, _)] = design.records_of("E3849A15-2FC6-42A0-AF3A-2F1D7273B406")[..] else {
        panic!("one Revolve scope");
    };
    let tracked = design.record(reference_at(design.record(scope), TAIL + 19));
    assert_eq!(u32_at(tracked, 21), 4, "three side entries and the body");
}

#[test]
fn an_axis_against_its_picked_line_is_the_same_full_turn() {
    let mut construction = axis("axis");
    construction.inputs[0].reversed = Some(true);
    let ir = design(
        revolved(&tube()),
        profile(&tube(), Some(0.0)),
        revolve(Some(construction)),
    );
    assert_eq!(decided(&ir), Ok(4));
}

#[test]
fn a_resolved_axis_finds_the_construction_line() {
    let mut construction = axis("axis");
    construction.inputs.clear();
    let ir = design(
        revolved(&tube()),
        profile(&tube(), Some(0.0)),
        revolve(Some(construction)),
    );
    assert_eq!(decided(&ir), Ok(4));
}

#[test]
fn an_arc_about_the_axis_sweeps_a_sphere_and_one_off_it_a_torus() {
    // A half disc of radius 3 mm: the arc from (0, -3) to (0, 3) through
    // (3, 0) on XZ, closed by its line on the axis.
    let mut half = Source::new(
        "profile",
        picked(principal(PrincipalEntity::XzPlane)),
        frame([0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 0.0, -1.0]),
    );
    half.entity(
        "arc",
        PlanarCurve::Arc {
            center: point2(0.0, 0.0),
            radius: cadmpeg_ir::scalar::PositiveLength::new(3.0).expect("radius"),
            start: Angle::new(-TAU / 4.0).expect("angle"),
            end: Angle::new(TAU / 4.0).expect("angle"),
        },
        &[],
    )
    .entity(
        "line",
        PlanarCurve::Line {
            start: point2(0.0, 3.0),
            end: point2(0.0, -3.0),
        },
        &[],
    );
    let sphere = SolvedSurfaceGeometry::Sphere(
        SphereSurface::try_new(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
            3.0,
        )
        .expect("sphere"),
    );
    let ir = design(faces_only(vec![sphere]), half, revolve(Some(axis("line"))));
    assert_eq!(decided(&ir), Ok(1));

    // A circle of radius 1 mm about (3, 0) on XZ.
    let mut ring = Source::new(
        "profile",
        picked(principal(PrincipalEntity::XzPlane)),
        frame([0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 0.0, -1.0]),
    );
    ring.entity(
        "circle",
        PlanarCurve::Circle {
            center: point2(3.0, 0.0),
            radius: cadmpeg_ir::scalar::PositiveLength::new(1.0).expect("radius"),
        },
        &[],
    )
    .entity(
        "axis",
        PlanarCurve::Line {
            start: point2(0.0, 2.0),
            end: point2(0.0, -2.0),
        },
        &[],
    )
    .construction("axis");
    let torus = SolvedSurfaceGeometry::Torus(
        TorusSurface::try_new(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
            3.0,
            1.0,
        )
        .expect("torus"),
    );
    let ir = design(faces_only(vec![torus]), ring, revolve(Some(axis("axis"))));
    assert_eq!(decided(&ir), Ok(1));
}

#[test]
fn each_row_outside_the_slice_names_its_reason() {
    let with = |change: fn(&mut Revolve)| {
        let mut operation = revolve(Some(axis("axis")));
        change(&mut operation);
        decided(&design(
            revolved(&tube()),
            profile(&tube(), Some(0.0)),
            operation,
        ))
    };
    let reason = |text: &str| Err(text.to_owned());
    assert_eq!(
        with(|revolve| revolve.span = Some(turn(TAU / 2.0))),
        reason("its extent is not one-sided full turn")
    );
    assert_eq!(
        with(|revolve| revolve.span = None),
        reason("its extent is not one-sided full turn")
    );
    assert_eq!(
        with(|revolve| revolve.axis = None),
        reason("its axis is not established")
    );
    assert_eq!(
        with(|revolve| {
            revolve.output.boolean = Some(Boolean::Apply {
                op: BooleanOp::Cut,
                targets: None,
            });
        }),
        reason("it does not make one new solid")
    );
    assert_eq!(
        with(|revolve| {
            if let Some(axis) = &mut revolve.axis {
                axis.resolved = Some(AxisValue {
                    origin: FinitePoint3::new(Point3::new(1.0, 0.0, 0.0)).expect("finite"),
                    direction: UnitVector3::Z_AXIS,
                });
            }
        }),
        reason("its axis value does not lie on its picked line")
    );
    // A construction line through the middle of the profile.
    let mut crossing = revolve(Some(axis("axis")));
    if let Some(axis) = &mut crossing.axis {
        axis.resolved = None;
    }
    assert_eq!(
        decided(&design(
            revolved(&tube()),
            profile(&tube(), Some(f64::midpoint(INNER, OUTER))),
            crossing
        )),
        reason("its profile crosses its axis")
    );
    // The solid cylinder's faces for the tube's profile.
    assert_eq!(
        decided(&design(
            revolved(&cylinder()),
            profile(&tube(), Some(0.0)),
            revolve(Some(axis("axis")))
        )),
        reason("its faces do not match its profile")
    );
}

#[test]
fn a_revolve_outside_the_slice_stays_in_the_base_feature_with_its_reason() {
    let mut operation = revolve(Some(axis("axis")));
    operation.span = Some(turn(TAU / 2.0));
    let ir = design(revolved(&tube()), profile(&tube(), Some(0.0)), operation);
    let (report, _, bytes) = write_bytes(&ir);
    let missing = loss(&report, EVALUATION_MISSING).join("\n");
    assert!(
        missing.contains(
            "test:design:feature#revolve (the Base Feature holds it: its extent is not one-sided full turn)"
        ),
        "{missing}"
    );
    let design = segment(&bytes, "FusionDesignSegmentType1");
    assert!(design
        .records_of("E3849A15-2FC6-42A0-AF3A-2F1D7273B406")
        .is_empty());
}
