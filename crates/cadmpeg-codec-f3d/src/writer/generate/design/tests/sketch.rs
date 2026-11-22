// SPDX-License-Identifier: Apache-2.0
//! The `Sketch` items a Base Feature design writes before its `Base Feature`
//! (f3d.md §3.1, "Records of a Sketch item"), decoded back and read from the
//! archive's Design segment.

use cadmpeg_ir::design::construction::{
    kinds, Construction, ConstructionInput, FrameValue, InputReference, InputRole, Relation,
    Subject,
};
use cadmpeg_ir::design::container::Container;
use cadmpeg_ir::design::dimension::Dimension;
use cadmpeg_ir::design::evaluation::EvaluationStatus;
use cadmpeg_ir::design::feature::{Feature, FeatureDefinition, FeatureOperation};
use cadmpeg_ir::design::ops::datum::{Datum, DatumElement};
use cadmpeg_ir::design::ops::sketch::{
    Contact, DefiningRole, EntityDefinition, EntityRole, Operand, Planar, PlanarCurve, Sketch,
    SketchConstraint, SketchEntity, SketchPlane, SketchRelation, SketchSolve,
};
use cadmpeg_ir::design::options::NativeOptions;
use cadmpeg_ir::design::refs::Member;
use cadmpeg_ir::design::selection::{
    Anchor, Members, PickTarget, Picked, Picks, PrincipalEntity, Selection,
};
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::features::geometry::FinitePoint3;
use cadmpeg_ir::features::FeatureId;
use cadmpeg_ir::math::{Point2, Point3, Vector3};
use cadmpeg_ir::scalar::{Angle, Length, PositiveLength};
use cadmpeg_ir::sketches::{SketchConstraintId, SketchEntityId, SketchId};
use cadmpeg_ir::units::{FinitePoint2, UnitVector3};

use super::roster::segment;
use super::{loss, scoped_cube, write, write_bytes, EXTRUDE};
use crate::loss::F3dLossCode;

const TOLERANCE: f64 = 1e-9;

fn feature_id(name: &str) -> FeatureId {
    FeatureId::mint(format!("test:design:feature#{name}")).expect("identity grammar")
}

fn sketch_id(name: &str) -> SketchId {
    SketchId::mint(format!("test:design:sketch#{name}")).expect("identity grammar")
}

fn entity_id(sketch: &str, name: &str) -> SketchEntityId {
    SketchEntityId::mint(format!("test:design:entity#{sketch}-{name}")).expect("identity grammar")
}

fn constraint_id(sketch: &str, name: &str) -> SketchConstraintId {
    SketchConstraintId::mint(format!("test:design:constraint#{sketch}-{name}"))
        .expect("identity grammar")
}

fn feature(name: &str, operation: FeatureOperation) -> Feature {
    Feature {
        id: feature_id(name),
        name: None,
        suppressed: None,
        definition: FeatureDefinition {
            operation,
            refine: None,
            fuzzy_tolerance: None,
        },
        placement: None,
        component: None,
        driven_by: None,
        native_ref: None,
    }
}

fn unit(x: f64, y: f64, z: f64) -> UnitVector3 {
    UnitVector3::new(Vector3::new(x, y, z)).expect("a unit vector")
}

fn point2(u: f64, v: f64) -> FinitePoint2 {
    FinitePoint2::new(Point2 { u, v }).expect("a finite point")
}

/// A frame from its origin and its x and y axes.
fn frame(origin: [f64; 3], x: [f64; 3], y: [f64; 3]) -> FrameValue {
    let z = Vector3::from(x).cross(Vector3::from(y));
    FrameValue {
        origin: FinitePoint3::new(Point3 {
            x: origin[0],
            y: origin[1],
            z: origin[2],
        })
        .expect("a finite point"),
        x_axis: unit(x[0], x[1], x[2]),
        y_axis: unit(y[0], y[1], y[2]),
        z_axis: unit(z.x, z.y, z.z),
    }
}

fn picked(target: PickTarget) -> ConstructionInput {
    ConstructionInput {
        role: Some(InputRole {
            subject: Subject::Whole,
            relation: Relation::Coincident,
        }),
        reference: Some(InputReference::Picks(Selection {
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
        })),
        value: None,
        reversed: None,
    }
}

fn unread() -> ConstructionInput {
    ConstructionInput {
        role: None,
        reference: Some(InputReference::Picks(Selection {
            at: Anchor::Input,
            picks: Picks::Unread,
            native: None,
        })),
        value: None,
        reversed: None,
    }
}

fn principal(entity: PrincipalEntity) -> PickTarget {
    PickTarget::Principal {
        entity,
        container: None,
    }
}

fn datum(name: &str, plane: PrincipalEntity) -> Feature {
    feature(
        name,
        FeatureOperation::Datum(Box::new(Datum {
            element: DatumElement::Plane(Construction::<kinds::Plane> {
                inputs: vec![picked(principal(plane))],
                ..Construction::default()
            }),
            link: None,
            native_options: NativeOptions::new(),
            source_settings: NativeOptions::new(),
        })),
    )
}

/// One source sketch: its feature, record, entities and constraints.
struct Source {
    name: &'static str,
    input: ConstructionInput,
    frame: FrameValue,
    entities: Vec<SketchEntity<Planar>>,
    constraints: Vec<SketchConstraint<Planar>>,
}

impl Source {
    fn new(name: &'static str, input: ConstructionInput, frame: FrameValue) -> Self {
        Self {
            name,
            input,
            frame,
            entities: Vec::new(),
            constraints: Vec::new(),
        }
    }

    fn entity(
        &mut self,
        name: &str,
        curve: PlanarCurve,
        roles: &[(&str, DefiningRole)],
    ) -> &mut Self {
        self.entities.push(SketchEntity {
            id: entity_id(self.name, name),
            sketch: sketch_id(self.name),
            definition: EntityDefinition::Free(curve),
            roles: roles
                .iter()
                .map(|(of, role)| EntityRole {
                    of: entity_id(self.name, of),
                    role: role.clone(),
                })
                .collect(),
            construction: None,
            centerline: None,
            native: None,
            native_options: NativeOptions::new(),
            source_settings: NativeOptions::new(),
            native_ref: None,
        });
        self
    }

    fn constraint(&mut self, name: &str, relation: SketchRelation<Planar>) -> &mut Self {
        self.constraints.push(SketchConstraint {
            id: constraint_id(self.name, name),
            sketch: sketch_id(self.name),
            relation,
            name: None,
            active: None,
            native_options: NativeOptions::new(),
            source_settings: NativeOptions::new(),
            native_ref: None,
        });
        self
    }

    fn operand(&self, name: &str) -> Operand<Planar> {
        Operand::Entity(entity_id(self.name, name))
    }

    fn add_to(self, ir: &mut CadIr) -> FeatureId {
        let id = feature_id(self.name);
        ir.design.features.push(feature(
            self.name,
            FeatureOperation::Sketch(sketch_id(self.name)),
        ));
        ir.design.sketches.push(Sketch {
            id: sketch_id(self.name),
            owner: id.clone(),
            name: None,
            plane: SketchPlane::Placed(Box::new(Construction {
                inputs: vec![self.input],
                resolved: Some(self.frame),
                ..Construction::default()
            })),
            solve: SketchSolve {
                state: None,
                status: EvaluationStatus::Current,
            },
            native_options: NativeOptions::new(),
            source_settings: NativeOptions::new(),
            native_ref: None,
        });
        ir.design.sketch_entities.extend(self.entities);
        ir.design.sketch_constraints.extend(self.constraints);
        id
    }
}

/// The world position of the source sketch point `(u, v)`.
fn world(frame: &FrameValue, u: f64, v: f64) -> [f64; 3] {
    let origin = frame.origin.get();
    let (x_axis, y_axis) = (frame.x_axis.as_raw(), frame.y_axis.as_raw());
    [
        origin.x + u * x_axis.x + v * y_axis.x,
        origin.y + u * x_axis.y + v * y_axis.y,
        origin.z + u * x_axis.z + v * y_axis.z,
    ]
}

/// A rectangle of four lines whose corners are four points with two roles
/// each, on Top (`XZ`) with its x axis along +Z; a circle on Front (`XY`);
/// an arc and a chord on Right (`YZ`); an offset sketch; and a line on `XZ`
/// whose normal is −Y.
fn scoped_sketches() -> (CadIr, Vec<(&'static str, FrameValue)>) {
    let mut ir = scoped_cube();
    let mut members = vec![
        datum("front", PrincipalEntity::XyPlane),
        datum("top", PrincipalEntity::XzPlane),
        datum("right", PrincipalEntity::YzPlane),
    ];
    let datums = members
        .iter()
        .map(|feature| feature.id.clone())
        .collect::<Vec<_>>();
    ir.design.features.append(&mut members);
    let mut timeline = datums
        .iter()
        .cloned()
        .map(Member::Feature)
        .collect::<Vec<_>>();
    let mut frames = Vec::new();

    let top = frame([0.0, 0.0, 0.0], [0.0, 0.0, 1.0], [1.0, 0.0, 0.0]);
    let mut sketch1 = Source::new(
        "sketch1",
        picked(PickTarget::Datum {
            feature: datums[1].clone(),
            part: None,
        }),
        top.clone(),
    );
    let corners = [(0.0, 0.0), (13.0, 0.0), (13.0, 95.0), (0.0, 95.0)];
    for (index, (u, v)) in corners.iter().enumerate() {
        let start_of = format!("l{}", index + 1);
        let end_of = format!("l{}", (index + 3) % 4 + 1);
        sketch1.entity(
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
        sketch1.entity(
            &format!("l{}", index + 1),
            PlanarCurve::Line {
                start: point2(start.0, start.1),
                end: point2(end.0, end.1),
            },
            &[],
        );
    }
    let (w2, w4) = (sketch1.operand("l2"), sketch1.operand("l4"));
    let (l1, l2, l3, l4, p1) = (
        sketch1.operand("l1"),
        sketch1.operand("l2"),
        sketch1.operand("l3"),
        sketch1.operand("l4"),
        sketch1.operand("p1"),
    );
    sketch1
        .constraint(
            "h1",
            SketchRelation::Horizontal {
                a: l1.clone(),
                b: None,
            },
        )
        .constraint("h3", SketchRelation::Horizontal { a: l3, b: None })
        .constraint(
            "v2",
            SketchRelation::Vertical {
                a: l2.clone(),
                b: None,
            },
        )
        .constraint("v4", SketchRelation::Vertical { a: l4, b: None })
        .constraint(
            "perpendicular",
            SketchRelation::Perpendicular { a: l1, b: l2 },
        )
        .constraint("fixed", SketchRelation::Fixed { of: p1 })
        .constraint(
            "width",
            SketchRelation::Distance {
                a: w2,
                b: w4,
                along: None,
                doubled: None,
                value: Dimension::Literal(Length::new(13.0).expect("a length")),
            },
        );
    frames.push(("sketch1", top));
    timeline.push(Member::Feature(sketch1.add_to(&mut ir)));

    let front = frame([0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]);
    let mut sketch2 = Source::new(
        "sketch2",
        picked(principal(PrincipalEntity::XyPlane)),
        front.clone(),
    );
    sketch2.entity(
        "c",
        PlanarCurve::Circle {
            center: point2(5.0, 5.0),
            radius: PositiveLength::new(20.0).expect("a radius"),
        },
        &[],
    );
    frames.push(("sketch2", front));
    timeline.push(Member::Feature(sketch2.add_to(&mut ir)));

    let right = frame([0.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]);
    let mut sketch3 = Source::new(
        "sketch3",
        picked(PickTarget::Datum {
            feature: datums[2].clone(),
            part: None,
        }),
        right.clone(),
    );
    sketch3
        .entity(
            "a",
            PlanarCurve::Arc {
                center: point2(0.0, 0.0),
                radius: PositiveLength::new(10.0).expect("a radius"),
                start: Angle::ZERO,
                end: Angle::QUARTER_TURN,
            },
            &[],
        )
        .entity(
            "q1",
            PlanarCurve::Point(point2(10.0, 0.0)),
            &[("chord", DefiningRole::Start)],
        )
        .entity(
            "q2",
            PlanarCurve::Point(point2(0.0, 10.0)),
            &[("chord", DefiningRole::End)],
        )
        .entity(
            "chord",
            PlanarCurve::Line {
                start: point2(10.0, 0.0),
                end: point2(0.0, 10.0),
            },
            &[],
        );
    let (q1, chord) = (sketch3.operand("q1"), sketch3.operand("chord"));
    let arc = sketch3.operand("a");
    sketch3.constraint("on-arc", SketchRelation::Coincident { a: q1, b: arc });
    let arc_again = sketch3.operand("a");
    sketch3.constraint(
        "tangent",
        SketchRelation::Tangent {
            a: chord,
            b: arc_again,
            contact: None,
        },
    );
    frames.push(("sketch3", right));
    timeline.push(Member::Feature(sketch3.add_to(&mut ir)));

    let offset = frame([0.0, 0.0, 5.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]);
    let mut sketch4 = Source::new("sketch4", unread(), offset);
    sketch4.entity("p", PlanarCurve::Point(point2(1.0, 1.0)), &[]);
    timeline.push(Member::Feature(sketch4.add_to(&mut ir)));

    let flipped = frame([0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 0.0, 1.0]);
    let mut sketch5 = Source::new("sketch5", unread(), flipped.clone());
    sketch5.entity(
        "line",
        PlanarCurve::Line {
            start: point2(1.0, 2.0),
            end: point2(4.0, 6.0),
        },
        &[],
    );
    frames.push(("sketch5", flipped));
    timeline.push(Member::Feature(sketch5.add_to(&mut ir)));

    let Some(Container::Ordered { members, .. }) = ir.design.containers.first_mut() else {
        panic!("the cube's timeline");
    };
    timeline.append(members);
    *members = timeline;
    (ir, frames)
}

/// The model-space position of every decoded sketch point and line end.
fn decoded_points(decoded: &CadIr) -> Vec<[f64; 3]> {
    let mut points = Vec::new();
    for sketch in &decoded.model.sketches {
        let (origin, normal, u) = sketch.resolved_placement().expect("a decoded frame");
        let (origin, normal, u) = (origin.get(), normal.get(), u.get());
        let v = normal.cross(u);
        for entity in decoded
            .model
            .sketch_entities
            .iter()
            .filter(|entity| entity.sketch == sketch.id)
        {
            let at = |p: &FinitePoint2| {
                let p = p.get();
                [
                    origin.x + p.u * u.x + p.v * v.x,
                    origin.y + p.u * u.y + p.v * v.y,
                    origin.z + p.u * u.z + p.v * v.z,
                ]
            };
            match entity.geometry.definition() {
                cadmpeg_ir::sketches::SketchGeometryDefinition::Point { position } => {
                    points.push(at(position));
                }
                cadmpeg_ir::sketches::SketchGeometryDefinition::Line { start, end } => {
                    points.push(at(start));
                    points.push(at(end));
                }
                _ => {}
            }
        }
    }
    points
}

fn near(a: [f64; 3], b: [f64; 3]) -> bool {
    (0..3).all(|axis| (a[axis] - b[axis]).abs() <= TOLERANCE)
}

#[test]
fn sketches_on_origin_planes_decode_back() {
    let (ir, frames) = scoped_sketches();
    let (_, decoded) = write(&ir);
    assert_eq!(
        decoded.model.sketches.len(),
        4,
        "{:#?}",
        decoded.model.sketches
    );
    let points = decoded_points(&decoded);
    for (name, frame) in &frames {
        for entity in ir
            .design
            .sketch_entities
            .iter()
            .filter(|entity| entity.sketch == sketch_id(name))
        {
            let expected = match &entity.definition {
                EntityDefinition::Free(PlanarCurve::Point(p)) => vec![p.get()],
                EntityDefinition::Free(PlanarCurve::Line { start, end }) => {
                    vec![start.get(), end.get()]
                }
                _ => Vec::new(),
            };
            for p in expected {
                let at = world(frame, p.u, p.v);
                assert!(
                    points.iter().any(|point| near(*point, at)),
                    "{name}: {at:?} among {points:?}"
                );
            }
        }
    }
    let curves = decoded
        .model
        .sketch_entities
        .iter()
        .map(|entity| entity.geometry.definition())
        .collect::<Vec<_>>();
    assert!(curves.iter().any(|curve| matches!(
        curve,
        cadmpeg_ir::sketches::SketchGeometryDefinition::Circle { radius, .. }
            if (radius.get() - 20.0).abs() <= TOLERANCE
    )));
    assert!(curves.iter().any(|curve| matches!(
        curve,
        cadmpeg_ir::sketches::SketchGeometryDefinition::Arc { radius, .. }
            if (radius.get() - 10.0).abs() <= TOLERANCE
    )));
    let top = decoded
        .model
        .sketches
        .iter()
        .filter_map(cadmpeg_ir::sketches::Sketch::resolved_placement)
        .filter(|(_, normal, _)| (normal.get().y - 1.0).abs() <= TOLERANCE)
        .count();
    assert_eq!(
        top, 2,
        "Sketch1 and Sketch5 lie on XZ with Fusion's normal +Y"
    );
}

#[test]
fn sketch_items_precede_the_base_feature() {
    let (ir, _) = scoped_sketches();
    let (_, decoded, bytes) = write_bytes(&ir);
    let mut features = decoded.model.features.iter().collect::<Vec<_>>();
    features.sort_by_key(|feature| feature.ordinal);
    let kinds = features
        .iter()
        .filter_map(|feature| match feature.evaluation.definition() {
            cadmpeg_ir::features::FeatureDefinition::Operation(
                cadmpeg_ir::features::FeatureOperation::Sketch { .. },
            ) => Some("sketch"),
            cadmpeg_ir::features::FeatureDefinition::Operation(
                cadmpeg_ir::features::FeatureOperation::BaseFeature { .. },
            ) => Some("base"),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(kinds, ["sketch", "sketch", "sketch", "sketch", "base"]);
    let (_, _, cube) = write_bytes(&scoped_cube());
    let (written, roster) = (
        segment(&bytes, "FusionDesignSegmentType1"),
        segment(&cube, "FusionDesignSegmentType1"),
    );
    for (index, _) in roster.primary.iter().filter(|(index, _)| *index < 225) {
        if *index == 10 || *index == 198 || (199..=203).contains(index) || *index == 207 {
            continue;
        }
        assert_eq!(
            written.type_of(*index),
            roster.type_of(*index),
            "record {index}"
        );
        assert_eq!(
            written.record(*index).len(),
            roster.record(*index).len(),
            "record {index}"
        );
    }
}

#[test]
fn sketch_relations_decode_by_kind() {
    let (ir, _) = scoped_sketches();
    let (report, decoded) = write(&ir);
    let kinds = decoded
        .model
        .sketch_constraints
        .iter()
        .map(|constraint| format!("{:?}", constraint.definition.kind()))
        .collect::<Vec<_>>();
    let count = |prefix: &str| kinds.iter().filter(|kind| kind.starts_with(prefix)).count();
    assert_eq!(count("Horizontal"), 2, "{kinds:?}");
    assert_eq!(count("Vertical"), 2, "{kinds:?}");
    assert_eq!(count("Perpendicular"), 1, "{kinds:?}");
    assert_eq!(count("Tangent"), 1, "{kinds:?}");
    assert!(count("Coincident") >= 1, "{kinds:?}");
    let omitted = loss(
        &report,
        &F3dLossCode::WriterSketchContentOmitted.kind().to_string(),
    );
    let all = omitted.join("\n");
    assert!(
        all.contains(constraint_id("sketch1", "fixed").as_str()),
        "{all}"
    );
    assert!(
        all.contains(constraint_id("sketch1", "width").as_str()),
        "{all}"
    );
}

#[test]
fn written_sketches_leave_evaluation_missing() {
    let (ir, _) = scoped_sketches();
    let (report, _) = write(&ir);
    let missing = loss(&report, "export/evaluation.missing");
    assert_eq!(missing.len(), 1, "{:#?}", report.losses);
    let note = missing[0];
    assert!(
        note.contains(&format!(
            "{} (its sketch lies on no origin plane)",
            feature_id("sketch4")
        )),
        "{note}"
    );
    assert!(note.contains(EXTRUDE), "{note}");
    for written in ["sketch1", "sketch2", "sketch3", "sketch5"] {
        assert!(!note.contains(feature_id(written).as_str()), "{note}");
    }
}

#[test]
fn the_sketch_note_names_the_point_and_curve_identities() {
    let (ir, _) = scoped_sketches();
    let (report, _) = write(&ir);
    let notes = loss(&report, "export/record.residual-unavailable")
        .into_iter()
        .filter(|message| message.contains("DR-901"))
        .collect::<Vec<_>>();
    let [note] = notes.as_slice() else {
        panic!("one DR-901 note: {:#?}", report.losses);
    };
    for named in [
        "point and curve identities (`pt_tag`, `crv_primary_id`): one sequence from 100",
        "record 10 (sketch collection: `crv_tag_id_idx`",
    ] {
        assert!(note.contains(named), "{named}: {note}");
    }
}

#[test]
fn the_fixed_clock_note_names_every_record_timestamp_table() {
    let (ir, _) = scoped_sketches();
    let (report, _, bytes) = write_bytes(&ir);
    let design = segment(&bytes, "FusionDesignSegmentType1");
    let tables = design
        .primary
        .iter()
        .map(|(index, _)| *index)
        .filter(|index| {
            design
                .type_of(*index)
                .0
                .eq_ignore_ascii_case(super::super::types::RECORD_TIMESTAMPS.guid)
        })
        .collect::<Vec<_>>();
    // The component, Base Feature scope and result tables, and three for
    // each of the four sketches.
    assert_eq!(tables.len(), 3 + 3 * 4);
    let notes = loss(&report, "export/record.residual-unavailable")
        .into_iter()
        .filter(|message| message.contains("fixed-clock"))
        .collect::<Vec<_>>();
    let [note] = notes.as_slice() else {
        panic!("one fixed-clock note: {:#?}", report.losses);
    };
    for table in tables {
        assert!(
            note.contains(&format!("record {table} (")),
            "{table}: {note}"
        );
    }
}

#[test]
fn the_id_table_lists_sketches_then_the_base_feature() {
    let (ir, _) = scoped_sketches();
    let (_, decoded, bytes) = write_bytes(&ir);
    let design = segment(&bytes, "FusionDesignSegmentType1");
    let timeline = design.record(207);
    let count = super::u32_at(timeline, 32);
    assert_eq!(count, 5);
    let scopes = (0..5)
        .map(|item| super::u64_at(timeline, 37 + 11 * item))
        .collect::<Vec<_>>();
    assert_eq!(scopes[4], 209);
    assert!(scopes[..4].iter().all(|scope| *scope > 224));
    let table = design.record(203);
    let forward = super::u32_at(table, 21);
    assert_eq!(forward, 5);
    let pairs_at = 25 + 20 * 5;
    assert_eq!(super::u32_at(table, pairs_at), 5);
    let pairs = (0..5)
        .map(|pair| {
            let at = pairs_at + 4 + 16 * pair;
            (super::u64_at(table, at), super::u64_at(table, at + 8))
        })
        .collect::<Vec<_>>();
    assert_eq!(
        pairs,
        (300..305).zip(scopes.iter().copied()).collect::<Vec<_>>()
    );
    let native = crate::test_support::native_test::f3d_native(&decoded);
    assert!(!native.persistent_subentity_tags.is_empty());
    assert!(native
        .persistent_subentity_tags
        .iter()
        .all(|tag| tag.design_references == [-304]));
}

#[test]
fn a_sketch_of_lines_only_is_written_with_its_base_feature() {
    let mut ir = scoped_cube();
    let front = frame([0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]);
    let mut sketch = Source::new(
        "lines",
        picked(principal(PrincipalEntity::XyPlane)),
        front.clone(),
    );
    sketch.entity(
        "line",
        PlanarCurve::Line {
            start: point2(0.0, 0.0),
            end: point2(3.0, 4.0),
        },
        &[],
    );
    let id = sketch.add_to(&mut ir);
    let Some(Container::Ordered { members, .. }) = ir.design.containers.first_mut() else {
        panic!("the cube's timeline");
    };
    members.insert(0, Member::Feature(id));
    let (report, decoded) = write(&ir);
    assert_eq!(decoded.model.sketches.len(), 1, "{:#?}", report.losses);
    let points = decoded_points(&decoded);
    for at in [world(&front, 0.0, 0.0), world(&front, 3.0, 4.0)] {
        assert!(
            points.iter().any(|point| near(*point, at)),
            "{at:?} among {points:?}"
        );
    }
    assert!(decoded.model.features.iter().any(|feature| matches!(
        feature.evaluation.definition(),
        cadmpeg_ir::features::FeatureDefinition::Operation(
            cadmpeg_ir::features::FeatureOperation::BaseFeature { .. }
        )
    )));
}

/// A sketch on `plane` with source axes `x` and `y`: a line along each axis
/// and a point pair along each, with a horizontal and a vertical relation on
/// each line and pair.
fn oriented(
    name: &'static str,
    plane: PrincipalEntity,
    x_axis: [f64; 3],
    y_axis: [f64; 3],
) -> Source {
    let origin = [0.0, 0.0, 0.0];
    let mut sketch = Source::new(
        name,
        picked(principal(plane)),
        frame(origin, x_axis, y_axis),
    );
    sketch
        .entity(
            "h",
            PlanarCurve::Line {
                start: point2(1.0, 2.0),
                end: point2(11.0, 2.0),
            },
            &[],
        )
        .entity(
            "v",
            PlanarCurve::Line {
                start: point2(1.0, 2.0),
                end: point2(1.0, 9.0),
            },
            &[],
        )
        .entity("p", PlanarCurve::Point(point2(3.0, 5.0)), &[])
        .entity("q", PlanarCurve::Point(point2(8.0, 5.0)), &[])
        .entity("r", PlanarCurve::Point(point2(3.0, 12.0)), &[]);
    let (along_x, along_y) = (sketch.operand("h"), sketch.operand("v"));
    let (point, beside, above) = (
        sketch.operand("p"),
        sketch.operand("q"),
        sketch.operand("r"),
    );
    sketch
        .constraint(
            "h",
            SketchRelation::Horizontal {
                a: along_x,
                b: None,
            },
        )
        .constraint(
            "v",
            SketchRelation::Vertical {
                a: along_y,
                b: None,
            },
        )
        .constraint(
            "pq",
            SketchRelation::Horizontal {
                a: point.clone(),
                b: Some(beside),
            },
        )
        .constraint(
            "pr",
            SketchRelation::Vertical {
                a: point,
                b: Some(above),
            },
        );
    sketch
}

/// Whether the decoded orientation relation `definition` holds on the
/// decoded geometry: a horizontal line has no v extent, a vertical line no
/// u extent, and a point pair shares its stated coordinate. `None` for
/// another kind.
fn orientation_holds(
    decoded: &CadIr,
    definition: &cadmpeg_ir::sketches::SketchConstraintDefinitionInput,
) -> Option<bool> {
    use cadmpeg_ir::sketches::{
        SketchConstraintDefinitionInput as Definition, SketchCoordinateAxis,
        SketchGeometryDefinition as Geometry, SketchLocus,
    };
    let geometry = |id: &SketchEntityId| {
        decoded
            .model
            .sketch_entities
            .iter()
            .find(|entity| entity.id() == id)
            .map(|entity| entity.geometry.definition().clone())
    };
    let line = |id: &SketchEntityId| match geometry(id) {
        Some(Geometry::Line { start, end }) => {
            let (start, end) = (start.get(), end.get());
            Some((end.u - start.u, end.v - start.v))
        }
        _ => None,
    };
    let point = |locus: &SketchLocus| match locus {
        SketchLocus::Entity(id) => match geometry(id) {
            Some(Geometry::Point { position }) => Some(position.get()),
            _ => None,
        },
        _ => None,
    };
    Some(match definition {
        Definition::Horizontal { entity } => {
            line(entity).is_some_and(|(du, dv)| dv.abs() <= TOLERANCE * du.hypot(dv))
        }
        Definition::Vertical { entity } => {
            line(entity).is_some_and(|(du, dv)| du.abs() <= TOLERANCE * du.hypot(dv))
        }
        Definition::SameCoordinate { relation } => {
            match (point(relation.first()), point(relation.second())) {
                (Some(a), Some(b)) => match relation.axis() {
                    SketchCoordinateAxis::U => (a.u - b.u).abs() <= TOLERANCE,
                    SketchCoordinateAxis::V => (a.v - b.v).abs() <= TOLERANCE,
                },
                _ => false,
            }
        }
        _ => return None,
    })
}

#[test]
fn orientation_relations_follow_each_plane_frame() {
    let mut ir = scoped_cube();
    let (cos, sin) = (3f64.sqrt() / 2.0, 0.5);
    let sources = [
        // Quarter turns from Fusion's frame on each plane.
        oriented(
            "xy",
            PrincipalEntity::XyPlane,
            [0.0, 1.0, 0.0],
            [-1.0, 0.0, 0.0],
        ),
        oriented(
            "xz",
            PrincipalEntity::XzPlane,
            [0.0, 0.0, 1.0],
            [1.0, 0.0, 0.0],
        ),
        oriented(
            "yz",
            PrincipalEntity::YzPlane,
            [0.0, 0.0, 1.0],
            [0.0, -1.0, 0.0],
        ),
        // Normals opposite the plane's: a reflection, and a reflection with
        // a quarter turn.
        oriented(
            "mirror",
            PrincipalEntity::XzPlane,
            [1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0],
        ),
        oriented(
            "mirror-turn",
            PrincipalEntity::XyPlane,
            [0.0, 1.0, 0.0],
            [1.0, 0.0, 0.0],
        ),
        oriented(
            "oblique",
            PrincipalEntity::XyPlane,
            [cos, sin, 0.0],
            [-sin, cos, 0.0],
        ),
    ];
    let mut ids = Vec::new();
    for source in sources {
        ids.push(source.add_to(&mut ir));
    }
    let Some(Container::Ordered { members, .. }) = ir.design.containers.first_mut() else {
        panic!("the cube's timeline");
    };
    for (at, id) in ids.into_iter().enumerate() {
        members.insert(at, Member::Feature(id));
    }
    let (report, decoded) = write(&ir);
    assert_eq!(decoded.model.sketches.len(), 6, "{:#?}", report.losses);
    let checked = decoded
        .model
        .sketch_constraints
        .iter()
        .filter_map(|constraint| {
            orientation_holds(&decoded, constraint.definition.kind())
                .map(|holds| (holds, constraint.definition.kind()))
        })
        .collect::<Vec<_>>();
    assert_eq!(checked.len(), 20, "{checked:#?}");
    assert!(checked.iter().all(|(holds, _)| *holds), "{checked:#?}");
    let omitted = loss(
        &report,
        &F3dLossCode::WriterSketchContentOmitted.kind().to_string(),
    )
    .join("\n");
    for name in ["h", "v", "pq", "pr"] {
        assert!(
            omitted.contains(constraint_id("oblique", name).as_str()),
            "{omitted}"
        );
        for written in ["xy", "xz", "yz", "mirror", "mirror-turn"] {
            assert!(
                !omitted.contains(constraint_id(written, name).as_str()),
                "{omitted}"
            );
        }
    }
}

#[test]
fn a_failure_in_the_sketch_records_keeps_the_design() {
    super::super::FAIL_NEXT_SKETCHES.with(|fail| fail.set(true));
    let (ir, _) = scoped_sketches();
    let (report, decoded) = write(&ir);
    assert!(decoded.model.sketches.is_empty(), "{:#?}", report.losses);
    assert!(decoded.model.features.iter().any(|feature| matches!(
        feature.evaluation.definition(),
        cadmpeg_ir::features::FeatureDefinition::Operation(
            cadmpeg_ir::features::FeatureOperation::BaseFeature { .. }
        )
    )));
    assert!(
        !report
            .losses
            .iter()
            .any(|loss| loss.message.contains("stores no design")
                || loss.message.contains("writes no design")),
        "{:#?}",
        report.losses
    );
    let missing = loss(&report, "export/evaluation.missing").join("\n");
    for dropped in ["sketch1", "sketch2", "sketch3", "sketch5"] {
        assert!(
            missing.contains(&format!(
                "{} (its sketch is dropped because the writer failed with the design's sketches (not implemented yet: a test failure of the sketch records))",
                feature_id(dropped)
            )),
            "{missing}"
        );
    }
    assert!(
        missing.contains(&format!(
            "{} (its sketch lies on no origin plane)",
            feature_id("sketch4")
        )),
        "{missing}"
    );
    assert!(loss(
        &report,
        &F3dLossCode::WriterSketchContentOmitted.kind().to_string()
    )
    .is_empty());
}

/// The model-space position of the decoded sketch point `id`.
fn decoded_point(decoded: &CadIr, id: &SketchEntityId) -> Option<[f64; 3]> {
    let entity = decoded
        .model
        .sketch_entities
        .iter()
        .find(|entity| entity.id() == id)?;
    let cadmpeg_ir::sketches::SketchGeometryDefinition::Point { position } =
        entity.geometry.definition()
    else {
        return None;
    };
    let sketch = decoded
        .model
        .sketches
        .iter()
        .find(|sketch| sketch.id == entity.sketch)?;
    let (origin, normal, u) = sketch.resolved_placement()?;
    let (origin, normal, u, p) = (origin.get(), normal.get(), u.get(), position.get());
    let v = normal.cross(u);
    Some([
        origin.x + p.u * u.x + p.v * v.x,
        origin.y + p.u * u.y + p.v * v.y,
        origin.z + p.u * u.z + p.v * v.z,
    ])
}

/// A line along +x to `(10, 0)` and an arc from there turning up to
/// `(15, 5)`, each with its own end points: a tangent at the shared ends
/// (`joint`), and a tangent of the arc to a rising line at a point that ends
/// no curve as the writer bounds it (`rise`).
fn tangent_sketch() -> Source {
    let mut sketch = Source::new(
        "tangent",
        picked(principal(PrincipalEntity::XyPlane)),
        frame([0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]),
    );
    sketch
        .entity(
            "l0",
            PlanarCurve::Point(point2(0.0, 0.0)),
            &[("line", DefiningRole::Start)],
        )
        .entity(
            "line-end",
            PlanarCurve::Point(point2(10.0, 0.0)),
            &[("line", DefiningRole::End)],
        )
        .entity(
            "arc-start",
            PlanarCurve::Point(point2(10.0, 0.0)),
            &[("arc", DefiningRole::Start)],
        )
        .entity(
            "arc-end",
            PlanarCurve::Point(point2(15.0, 5.0)),
            &[("arc", DefiningRole::End)],
        )
        .entity("stray", PlanarCurve::Point(point2(15.0, 5.0)), &[])
        .entity(
            "line",
            PlanarCurve::Line {
                start: point2(0.0, 0.0),
                end: point2(10.0, 0.0),
            },
            &[],
        )
        .entity(
            "arc",
            PlanarCurve::Arc {
                center: point2(10.0, 5.0),
                radius: PositiveLength::new(5.0).expect("a radius"),
                start: Angle::THREE_QUARTER_TURN,
                end: Angle::ZERO,
            },
            &[],
        )
        .entity(
            "rise",
            PlanarCurve::Line {
                start: point2(15.0, 5.0),
                end: point2(15.0, 15.0),
            },
            &[],
        );
    let (line, arc, rise) = (
        sketch.operand("line"),
        sketch.operand("arc"),
        sketch.operand("rise"),
    );
    let contact = |p: &str, q: &str| {
        Some(Contact::Ends([
            entity_id("tangent", p),
            entity_id("tangent", q),
        ]))
    };
    sketch
        .constraint(
            "joint",
            SketchRelation::Tangent {
                a: line,
                b: arc.clone(),
                contact: contact("line-end", "arc-start"),
            },
        )
        .constraint(
            "rise",
            SketchRelation::Tangent {
                a: arc,
                b: rise,
                contact: contact("arc-end", "stray"),
            },
        );
    sketch
}

#[test]
fn a_tangent_contact_at_the_ends_is_written_as_a_coincidence_or_named() {
    let mut ir = scoped_cube();
    let id = tangent_sketch().add_to(&mut ir);
    let Some(Container::Ordered { members, .. }) = ir.design.containers.first_mut() else {
        panic!("the cube's timeline");
    };
    members.insert(0, Member::Feature(id));
    let (report, decoded) = write(&ir);
    assert_eq!(decoded.model.sketches.len(), 1, "{:#?}", report.losses);
    let kinds = decoded
        .model
        .sketch_constraints
        .iter()
        .map(|constraint| constraint.definition.kind())
        .collect::<Vec<_>>();
    let tangents = kinds
        .iter()
        .filter(|kind| format!("{kind:?}").starts_with("Tangent"))
        .count();
    assert_eq!(tangents, 2, "{kinds:#?}");
    let joint = [10.0, 0.0, 0.0];
    let joined = kinds.iter().any(|kind| match kind {
        cadmpeg_ir::sketches::SketchConstraintDefinitionInput::CoincidentLoci { loci } => {
            loci.len() == 2
                && loci.iter().all(|locus| match locus {
                    cadmpeg_ir::sketches::SketchLocus::Entity(point) => {
                        decoded_point(&decoded, point).is_some_and(|at| near(at, joint))
                    }
                    _ => false,
                })
        }
        _ => false,
    });
    assert!(joined, "{kinds:#?}");
    let omitted = loss(
        &report,
        &F3dLossCode::WriterSketchContentOmitted.kind().to_string(),
    )
    .join("\n");
    assert!(
        omitted.contains(&format!(
            "the contact of {}",
            constraint_id("tangent", "rise")
        )),
        "{omitted}"
    );
    assert!(
        !omitted.contains(constraint_id("tangent", "joint").as_str()),
        "{omitted}"
    );
}

#[test]
fn a_tangent_contact_at_the_ends_writes_its_endpoint_branch() {
    use super::super::sketch::{plan, RelationKind, TangencyEnds};
    let mut ir = scoped_cube();
    let id = tangent_sketch().add_to(&mut ir);
    let plan = plan(&ir, &[&id]);
    let [item] = plan.items.as_slice() else {
        panic!("one written sketch: {:?}", plan.unwritten);
    };
    let tangents = item
        .relations
        .iter()
        .filter_map(|relation| match relation.kind {
            RelationKind::Tangent(ends) => Some(ends),
            _ => None,
        })
        .collect::<Vec<_>>();
    // The line's stored end meets the arc's stored start, where both run
    // along +x.
    assert_eq!(
        tangents,
        [
            Some(TangencyEnds {
                starts: [false, true],
                aligned: true,
            }),
            None,
        ]
    );
    let joins = item
        .relations
        .iter()
        .filter(|relation| relation.kind == RelationKind::Coincident)
        .count();
    assert_eq!(joins, 1);
}

#[test]
fn a_tangent_via_a_shared_endpoint_writes_its_branch_and_branchless_uses_shared_class() {
    use super::super::sketch::{plan, RelationKind, TangencyEnds};
    use super::super::types;
    let mut ir = scoped_cube();
    // A line along +x to `joint`, and an arc from `joint` turning up to
    // `(20, 10)`: `joint` bounds both curves.
    let mut sketch = Source::new(
        "tangent-via",
        picked(principal(PrincipalEntity::XyPlane)),
        frame([0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]),
    );
    sketch
        .entity(
            "p0",
            PlanarCurve::Point(point2(0.0, 0.0)),
            &[("line", DefiningRole::Start)],
        )
        .entity(
            "joint",
            PlanarCurve::Point(point2(10.0, 0.0)),
            &[("line", DefiningRole::End), ("arc", DefiningRole::Start)],
        )
        .entity(
            "arc-end",
            PlanarCurve::Point(point2(20.0, 10.0)),
            &[("arc", DefiningRole::End)],
        )
        .entity(
            "line",
            PlanarCurve::Line {
                start: point2(0.0, 0.0),
                end: point2(10.0, 0.0),
            },
            &[],
        )
        .entity(
            "arc",
            PlanarCurve::Arc {
                center: point2(10.0, 10.0),
                radius: PositiveLength::new(10.0).expect("a radius"),
                start: Angle::THREE_QUARTER_TURN,
                end: Angle::ZERO,
            },
            &[],
        );
    let (line, arc) = (sketch.operand("line"), sketch.operand("arc"));
    sketch
        .constraint(
            "via-end",
            SketchRelation::Tangent {
                a: line.clone(),
                b: arc.clone(),
                contact: Some(Contact::Via(entity_id("tangent-via", "joint"))),
            },
        )
        .constraint(
            "no-contact",
            SketchRelation::Tangent {
                a: line,
                b: arc,
                contact: None,
            },
        );
    let id = sketch.add_to(&mut ir);
    let planned = plan(&ir, &[&id]);
    let [item] = planned.items.as_slice() else {
        panic!("one written sketch: {:?}", planned.unwritten);
    };
    let tangents = item
        .relations
        .iter()
        .filter_map(|relation| match relation.kind {
            RelationKind::Tangent(ends) => Some(ends),
            _ => None,
        })
        .collect::<Vec<_>>();
    // The line's stored end meets the arc's stored start, where both run
    // along +x.
    assert_eq!(
        tangents,
        [
            Some(TangencyEnds {
                starts: [false, true],
                aligned: true,
            }),
            None,
        ]
    );
    let shared = item
        .relations
        .iter()
        .filter(|relation| !matches!(relation.kind, RelationKind::Tangent(Some(_))))
        .count();
    let Some(Container::Ordered { members, .. }) = ir.design.containers.first_mut() else {
        panic!("the cube's timeline");
    };
    members.insert(0, Member::Feature(id));
    let (report, decoded, bytes) = write_bytes(&ir);
    let design = segment(&bytes, "FusionDesignSegmentType1");
    let records_of = |row: &crate::writer::generate::presentation::TypeRow| {
        design
            .primary
            .iter()
            .filter(|(index, _)| design.type_of(*index).0.eq_ignore_ascii_case(row.guid))
            .count()
    };
    // Only the tangency with an endpoint branch takes the tangency class;
    // every other relation, the branchless tangency among them, takes the
    // shared relation class.
    assert_eq!(records_of(&types::SKETCH_TANGENCY), 1);
    assert_eq!(records_of(&types::SKETCH_RELATION), shared);
    let decoded_tangents = decoded
        .model
        .sketch_constraints
        .iter()
        .filter(|constraint| format!("{:?}", constraint.definition.kind()).starts_with("Tangent"))
        .count();
    assert_eq!(decoded_tangents, 2, "{:#?}", report.losses);
}
