// SPDX-License-Identifier: Apache-2.0
//! The source's planar sketches as Fusion `Sketch` timeline items on the
//! origin planes (cadir-v11.md §3.4; f3d.md §3.1, "Records of a Sketch
//! item"): which sketches are written, each entity and relation in the
//! fixed frame Fusion stores for its origin plane, and the losses of what is
//! not written. A tangency at the ends of its curves is written with its
//! endpoint branch and the coincidence of those ends.

use std::collections::{BTreeMap, HashMap};

use cadmpeg_ir::design::construction::{
    kinds, Construction, ConstructionInput, FrameValue, InputReference, Relation, Subject,
};
use cadmpeg_ir::design::dimension::Dimension;
use cadmpeg_ir::design::feature::FeatureOperation;
use cadmpeg_ir::design::ops::datum::DatumElement;
use cadmpeg_ir::design::ops::sketch::{
    Contact, DefiningRole, EntityDefinition, Operand, Planar, PlanarCurve, Sketch,
    SketchConstraint, SketchEntity, SketchPlane, SketchRelation,
};
use cadmpeg_ir::design::selection::{PickTarget, Picks, PrincipalEntity};
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::features::FeatureId;
use cadmpeg_ir::math::portable::Portable;
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::sketches::SketchEntityId;

use crate::loss::F3dLossCode;

/// The largest departure of a sketch normal from its origin plane's axis.
const NORMAL_TOLERANCE: f64 = 1e-9;
/// The largest component of a mapped source sketch axis across the Fusion
/// sketch axis it lies along.
const AXIS_TOLERANCE: f64 = 1e-9;
/// The smallest linear tolerance a sketch frame is checked with, in mm: the
/// decoder's linear resolution.
const MIN_LINEAR_TOLERANCE: f64 = 1e-5;
/// Millimetres per Fusion length unit (cm).
const MM_PER_CM: f64 = 10.0;
const FULL_TURN: f64 = std::f64::consts::TAU;

/// An origin plane of the design, with the frame Fusion stores for a sketch
/// on it (f3d.md §3.1, "Sketch placement on an origin plane").
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum OriginPlane {
    Xy,
    Xz,
    Yz,
}

impl OriginPlane {
    fn of(entity: PrincipalEntity) -> Option<Self> {
        match entity {
            PrincipalEntity::XyPlane => Some(Self::Xy),
            PrincipalEntity::XzPlane => Some(Self::Xz),
            PrincipalEntity::YzPlane => Some(Self::Yz),
            _ => None,
        }
    }

    /// The sketch's u axis, v axis and normal in model coordinates.
    pub(super) const fn axes(self) -> [[f64; 3]; 3] {
        match self {
            Self::Xy => [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
            Self::Xz => [[1.0, 0.0, 0.0], [0.0, 0.0, -1.0], [0.0, 1.0, 0.0]],
            Self::Yz => [[0.0, 1.0, 0.0], [0.0, 0.0, 1.0], [1.0, 0.0, 0.0]],
        }
    }

    /// The position of the plane's origin record among the three origin
    /// planes `XY`, `XZ`, `YZ`.
    pub(super) const fn ordinal(self) -> u32 {
        match self {
            Self::Xy => 0,
            Self::Xz => 1,
            Self::Yz => 2,
        }
    }

    /// The plane whose normal is the principal axis `normal` lies along.
    fn along(normal: [f64; 3]) -> Option<Self> {
        [Self::Xy, Self::Xz, Self::Yz]
            .into_iter()
            .find(|plane| (dot(plane.axes()[2], normal).abs() - 1.0).abs() <= NORMAL_TOLERANCE)
    }
}

/// A written entity of a sketch.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum Entity {
    /// A point, by its position in [`PlannedSketch::points`].
    Point(usize),
    /// A curve, by its position in [`PlannedSketch::curves`].
    Curve(usize),
}

/// A curve of a sketch in Fusion's sketch coordinates, in cm; its points
/// are positions in [`PlannedSketch::points`].
#[derive(Clone, Debug, PartialEq)]
pub(super) enum CurveGeometry {
    Line {
        start: usize,
        end: usize,
    },
    /// A circular arc counterclockwise from `angles[0]` to `angles[1]`.
    Arc {
        center: usize,
        start: usize,
        end: usize,
        radius: f64,
        angles: [f64; 2],
    },
    Circle {
        center: usize,
        radius: f64,
    },
}

impl CurveGeometry {
    /// The points the curve record names, in its stored order.
    pub(super) fn points(&self) -> Vec<usize> {
        match *self {
            Self::Line { start, end } => vec![end, start],
            Self::Arc {
                center, start, end, ..
            } => vec![center, end, start],
            Self::Circle { center, .. } => vec![center],
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(super) struct PlannedCurve {
    pub(super) geometry: CurveGeometry,
    pub(super) construction: bool,
}

/// The kind of a written relation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum RelationKind {
    Coincident,
    Collinear,
    Concentric,
    EqualLength,
    EqualRadius,
    Parallel,
    Perpendicular,
    Horizontal,
    Vertical,
    /// Written in the tangency class, with its endpoint branch where the
    /// source states the contact at the curves' ends.
    Tangent(Option<TangencyEnds>),
    Curvature,
    Symmetric,
    Midpoint,
}

/// The endpoint branch the tangency class stores (f3d.md §3.1, "Sketch
/// relations"): for each curve in member order, whether the contact is at its
/// stored start, and whether the two curves' parameter directions agree
/// there.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct TangencyEnds {
    pub(super) starts: [bool; 2],
    pub(super) aligned: bool,
}

impl TangencyEnds {
    /// The class's three flag bytes.
    pub(super) fn bytes(self) -> [u8; 3] {
        [
            u8::from(self.starts[0]),
            u8::from(self.starts[1]),
            u8::from(self.aligned),
        ]
    }
}

/// Where a written tangency touches, from the contact its source states.
struct Touch {
    ends: Option<TangencyEnds>,
    /// The coincidences the contact makes that the written curves do not
    /// already hold.
    joins: Vec<[Entity; 2]>,
}

/// A written relation and its members in semantic order.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct PlannedRelation {
    pub(super) kind: RelationKind,
    pub(super) members: Vec<Entity>,
}

/// One source sketch as a Fusion `Sketch` item.
#[derive(Debug)]
pub(super) struct PlannedSketch<'a> {
    pub(super) feature: &'a FeatureId,
    pub(super) plane: OriginPlane,
    /// Whether the source normal is opposite the plane's, so that the
    /// sketch is written as its reflection.
    pub(super) mirrored: bool,
    /// The points in sketch coordinates in cm, without the origin point.
    pub(super) points: Vec<[f64; 2]>,
    pub(super) curves: Vec<PlannedCurve>,
    pub(super) relations: Vec<PlannedRelation>,
}

/// The sketches the design writes, in timeline order, and what it does not
/// write of them.
#[derive(Debug, Default)]
pub(super) struct SketchPlan<'a> {
    pub(super) items: Vec<PlannedSketch<'a>>,
    /// The typed sketches not written, with the reason.
    pub(super) unwritten: BTreeMap<&'a FeatureId, String>,
    losses: Vec<LossNote>,
}

impl SketchPlan<'_> {
    /// Write none of the planned sketches, for `reason`: a failure of the
    /// design with them.
    pub(super) fn drop_items(&mut self, reason: &str) {
        for item in self.items.drain(..) {
            self.unwritten.insert(
                item.feature,
                format!(
                    "is dropped because the writer failed with the design's sketches ({reason})"
                ),
            );
        }
        self.losses.clear();
    }

    /// Whether `feature` is written as a `Sketch` item.
    pub(super) fn writes(&self, feature: &FeatureId) -> bool {
        self.items.iter().any(|item| item.feature == feature)
    }

    pub(super) fn into_losses(self) -> Vec<LossNote> {
        self.losses
    }
}

/// The plan of the typed sketches among `features`, the root scope's
/// features in timeline order.
pub(super) fn plan<'a>(target: &'a CadIr, features: &[&'a FeatureId]) -> SketchPlan<'a> {
    let tolerance = target.tolerances.linear.get().max(MIN_LINEAR_TOLERANCE);
    let mut out = SketchPlan::default();
    for &id in features {
        let Some(feature) = target.design.features.iter().find(|f| &f.id == id) else {
            continue;
        };
        let FeatureOperation::Sketch(sketch_id) = &feature.definition.operation else {
            continue;
        };
        let Some(sketch) = target.design.sketches.iter().find(|s| &s.id == sketch_id) else {
            out.unwritten.insert(id, "has no sketch record".into());
            continue;
        };
        if matches!(feature.suppressed, Some(Dimension::Literal(true))) {
            out.unwritten.insert(id, "is suppressed".into());
            continue;
        }
        let frame = match placement(target, sketch, tolerance) {
            Ok(frame) => frame,
            Err(reason) => {
                out.unwritten.insert(id, reason.into());
                continue;
            }
        };
        let mut notes = Notes::default();
        let planned = PlannedSketch::new(target, id, sketch, frame, &mut notes);
        out.losses.extend(notes.into_losses(sketch_id.as_str()));
        out.items.push(planned);
    }
    out
}

/// The origin plane of `sketch` and the map from its sketch coordinates to
/// Fusion's.
fn placement(target: &CadIr, sketch: &Sketch, tolerance: f64) -> Result<Frame, &'static str> {
    let construction = match &sketch.plane {
        SketchPlane::Placed(construction) => construction,
        SketchPlane::Block => return Err("is a block definition"),
    };
    let frame = construction.resolved.as_ref().ok_or("states no frame")?;
    let normal = vector(frame.z_axis.as_raw());
    let named = match picked_plane(target, construction) {
        Some(Pick::Plane(plane)) => plane,
        Some(Pick::Unread) => OriginPlane::along(normal).ok_or("lies on no origin plane")?,
        None => return Err("lies on no origin plane"),
    };
    let axes = named.axes();
    let origin = frame.origin.get();
    let origin = [origin.x, origin.y, origin.z];
    if dot(origin, axes[2]).abs() > tolerance
        || (dot(normal, axes[2]).abs() - 1.0).abs() > NORMAL_TOLERANCE
    {
        return Err("lies on no origin plane");
    }
    Ok(Frame::new(named, frame, origin))
}

enum Pick {
    Plane(OriginPlane),
    Unread,
}

/// The origin plane the sketch's plane construction picks: one `Whole`
/// `Coincident` pick of a principal plane, or of a datum that is one, or an
/// unread selection.
fn picked_plane(target: &CadIr, construction: &Construction<kinds::Frame>) -> Option<Pick> {
    let [input] = construction.inputs.as_slice() else {
        return None;
    };
    match picks(input)? {
        Picks::Unread => Some(Pick::Unread),
        Picks::Named(members) => match members.as_slice() {
            [picked] => match &picked.target {
                PickTarget::Principal { entity, .. } => OriginPlane::of(*entity).map(Pick::Plane),
                PickTarget::Datum {
                    feature,
                    part: None,
                } => datum_plane(target, feature).map(Pick::Plane),
                _ => None,
            },
            _ => None,
        },
        Picks::Rule { .. } => None,
    }
}

fn picks(input: &ConstructionInput) -> Option<&Picks<cadmpeg_ir::design::selection::Ordered>> {
    let coincident = input
        .role
        .as_ref()
        .is_none_or(|role| role.subject == Subject::Whole && role.relation == Relation::Coincident);
    match (&input.reference, coincident) {
        (Some(InputReference::Picks(selection)), true) => Some(&selection.picks),
        _ => None,
    }
}

/// The origin plane a datum plane feature states as one `Whole` `Coincident`
/// pick of a principal plane.
fn datum_plane(target: &CadIr, feature: &FeatureId) -> Option<OriginPlane> {
    let datum = target.design.features.iter().find(|f| &f.id == feature)?;
    let FeatureOperation::Datum(datum) = &datum.definition.operation else {
        return None;
    };
    let DatumElement::Plane(plane) = &datum.element else {
        return None;
    };
    let [input] = plane.inputs.as_slice() else {
        return None;
    };
    let Picks::Named(members) = picks(input)? else {
        return None;
    };
    match members.as_slice() {
        [picked] => match &picked.target {
            PickTarget::Principal { entity, .. } => OriginPlane::of(*entity),
            _ => None,
        },
        _ => None,
    }
}

/// The map from a source sketch's coordinates (mm) to Fusion's sketch
/// coordinates (cm) on its origin plane: a rotation, or a reflection where
/// the source normal is opposite the plane's.
#[derive(Clone, Copy, Debug)]
struct Frame {
    plane: OriginPlane,
    /// The source origin in Fusion's sketch coordinates, in mm.
    origin: [f64; 2],
    /// The source x and y axes in Fusion's sketch coordinates.
    x: [f64; 2],
    y: [f64; 2],
    /// The angle of the source x axis in Fusion's sketch coordinates.
    theta: f64,
    /// `1` where the source normal is the plane's, `-1` where it is opposite.
    sense: f64,
}

impl Frame {
    fn new(plane: OriginPlane, frame: &FrameValue, origin: [f64; 3]) -> Self {
        let [u_axis, v_axis, normal] = plane.axes();
        let in_plane = |direction: [f64; 3]| [dot(direction, u_axis), dot(direction, v_axis)];
        let x = in_plane(vector(frame.x_axis.as_raw()));
        let y = in_plane(vector(frame.y_axis.as_raw()));
        let sense = if dot(vector(frame.z_axis.as_raw()), normal) < 0.0 {
            -1.0
        } else {
            1.0
        };
        Self {
            plane,
            origin: in_plane(origin),
            x,
            y,
            theta: x[1].portable_atan2(x[0]),
            sense,
        }
    }

    /// The source sketch point `(u, v)` in mm as a Fusion sketch point in cm.
    fn point(&self, u: f64, v: f64) -> [f64; 2] {
        [
            (self.origin[0] + u * self.x[0] + v * self.y[0]) / MM_PER_CM,
            (self.origin[1] + u * self.x[1] + v * self.y[1]) / MM_PER_CM,
        ]
    }

    /// The source angle `angle` as a Fusion sketch angle.
    fn angle(&self, angle: f64) -> f64 {
        self.theta + self.sense * angle
    }
}

/// The omitted content of one sketch, by reason.
#[derive(Default)]
struct Notes {
    omitted: BTreeMap<&'static str, Vec<String>>,
}

impl Notes {
    fn omit(&mut self, item: String, reason: &'static str) {
        self.omitted.entry(reason).or_default().push(item);
    }

    fn into_losses(self, sketch: &str) -> Vec<LossNote> {
        self.omitted
            .into_iter()
            .map(|(reason, items)| {
                F3dLossCode::WriterSketchContentOmitted.note(format!(
                    "F3D writes sketch {sketch} without {}: {reason}",
                    items.join(" ")
                ))
            })
            .collect()
    }
}

impl<'a> PlannedSketch<'a> {
    fn new(
        target: &'a CadIr,
        feature: &'a FeatureId,
        sketch: &Sketch,
        frame: Frame,
        notes: &mut Notes,
    ) -> Self {
        let entities = target
            .design
            .sketch_entities
            .iter()
            .filter(|entity| entity.sketch == sketch.id)
            .collect::<Vec<_>>();
        let mut planned = Self {
            feature,
            plane: frame.plane,
            mirrored: frame.sense < 0.0,
            points: Vec::new(),
            curves: Vec::new(),
            relations: Vec::new(),
        };
        if let Some(name) = sketch
            .name
            .as_deref()
            .filter(|name| !name.trim().is_empty())
        {
            notes.omit(
                format!("its name {name}"),
                "the writer stores Fusion's default sketch name",
            );
        }
        let mut written = HashMap::<&SketchEntityId, Entity>::new();
        for entity in &entities {
            if let EntityDefinition::Free(PlanarCurve::Point(point)) = &entity.definition {
                let point = point.get();
                written.insert(&entity.id, Entity::Point(planned.points.len()));
                planned.points.push(frame.point(point.u, point.v));
            }
        }
        for entity in &entities {
            match &entity.definition {
                EntityDefinition::Free(PlanarCurve::Point(_)) => {}
                EntityDefinition::Free(curve) => {
                    match planned.curve(curve, &entity.id, &entities, &written, frame) {
                        Ok(geometry) => {
                            written.insert(&entity.id, Entity::Curve(planned.curves.len()));
                            planned.curves.push(PlannedCurve {
                                geometry,
                                construction: entity.construction == Some(true),
                            });
                        }
                        Err(reason) => notes.omit(entity.id.to_string(), reason),
                    }
                }
                EntityDefinition::Defined { .. } => {
                    notes.omit(
                        entity.id.to_string(),
                        "Fusion stores no defining form of a sketch entity",
                    );
                }
            }
        }
        let mut joins = Vec::new();
        for constraint in target
            .design
            .sketch_constraints
            .iter()
            .filter(|constraint| constraint.sketch == sketch.id)
        {
            let mut relations = match relations(constraint, &written, &planned.curves, frame) {
                Ok(relations) => relations,
                Err(reason) => {
                    notes.omit(constraint.id.to_string(), reason);
                    continue;
                }
            };
            if let (Some(contact), [relation]) =
                (contact(&constraint.relation), relations.as_mut_slice())
            {
                match planned.touch(contact, &relation.members, &written) {
                    Ok(touch) => {
                        if let RelationKind::Tangent(ends) = &mut relation.kind {
                            *ends = touch.ends;
                        }
                        joins.extend(touch.joins);
                    }
                    Err(reason) => notes.omit(format!("the contact of {}", constraint.id), reason),
                }
            }
            planned.relations.extend(relations);
        }
        for join in joins {
            let stated = planned.relations.iter().any(|relation| {
                relation.kind == RelationKind::Coincident
                    && (relation.members == join || relation.members == [join[1], join[0]])
            });
            if !stated {
                planned.relations.push(PlannedRelation {
                    kind: RelationKind::Coincident,
                    members: join.to_vec(),
                });
            }
        }
        planned
    }

    /// Where the written tangency of `members` touches, from its source
    /// `contact`: a contact at the ends of both curves gives the endpoint
    /// branch and joins the two end points; a contact through a point puts
    /// that point on each curve it does not bound.
    fn touch(
        &self,
        contact: &Contact<Planar>,
        members: &[Entity],
        written: &HashMap<&SketchEntityId, Entity>,
    ) -> Result<Touch, &'static str> {
        let &[Entity::Curve(a), Entity::Curve(b)] = members else {
            return Err("its members are not two written curves");
        };
        let point = |id: &SketchEntityId| match written.get(id) {
            Some(Entity::Point(point)) => Ok(*point),
            Some(Entity::Curve(_)) => Err("its contact names a curve, not a point"),
            None => Err("its contact names an entity the writer does not write"),
        };
        match contact {
            Contact::Via(p) => {
                let p = point(p)?;
                Ok(Touch {
                    ends: None,
                    joins: [a, b]
                        .into_iter()
                        .filter(|curve| self.end(*curve, p).is_none())
                        .map(|curve| [Entity::Point(p), Entity::Curve(curve)])
                        .collect(),
                })
            }
            Contact::Ends([p, q]) => {
                let (p, q) = (point(p)?, point(q)?);
                let ((p, at_a), (q, at_b)) = match (self.end(a, p), self.end(b, q)) {
                    (Some(at_a), Some(at_b)) => ((p, at_a), (q, at_b)),
                    _ => match (self.end(a, q), self.end(b, p)) {
                        (Some(at_a), Some(at_b)) => ((q, at_a), (p, at_b)),
                        _ => {
                            return Err(
                                "its contact points are not ends of its curves as the writer bounds them",
                            )
                        }
                    },
                };
                let ((start_a, [ax, ay]), (start_b, [bx, by])) = (at_a, at_b);
                Ok(Touch {
                    ends: Some(TangencyEnds {
                        starts: [start_a, start_b],
                        aligned: ax * bx + ay * by > 0.0,
                    }),
                    joins: (p != q)
                        .then_some([Entity::Point(p), Entity::Point(q)])
                        .into_iter()
                        .collect(),
                })
            }
        }
    }

    /// Whether `point` bounds `curve` at its stored start (`true`) or end
    /// (`false`), with the curve's parameter direction there; `None` where
    /// `point` bounds neither end.
    fn end(&self, curve: usize, point: usize) -> Option<(bool, [f64; 2])> {
        let at_start = |start: usize, end: usize| {
            if point == start {
                Some(true)
            } else if point == end {
                Some(false)
            } else {
                None
            }
        };
        match self.curves.get(curve)?.geometry {
            CurveGeometry::Line { start, end } => {
                let at_start = at_start(start, end)?;
                let ([x0, y0], [x1, y1]) = (*self.points.get(start)?, *self.points.get(end)?);
                Some((at_start, [x1 - x0, y1 - y0]))
            }
            CurveGeometry::Arc {
                start, end, angles, ..
            } => {
                let at_start = at_start(start, end)?;
                let angle = if at_start { angles[0] } else { angles[1] };
                let (sin, cos) = angle.portable_sin_cos();
                Some((at_start, [-sin, cos]))
            }
            CurveGeometry::Circle { .. } => None,
        }
    }

    /// The geometry of `curve`, with the points that bound it: the written
    /// points whose roles name it, else points added at its ends.
    fn curve(
        &mut self,
        curve: &PlanarCurve,
        id: &SketchEntityId,
        entities: &[&SketchEntity<Planar>],
        written: &HashMap<&SketchEntityId, Entity>,
        frame: Frame,
    ) -> Result<CurveGeometry, &'static str> {
        let role_point = |role: DefiningRole| {
            entities.iter().find_map(|entity| {
                entity
                    .roles
                    .iter()
                    .any(|r| &r.of == id && r.role == role)
                    .then(|| written.get(&entity.id))
                    .flatten()
                    .and_then(|entity| match entity {
                        Entity::Point(point) => Some(*point),
                        Entity::Curve(_) => None,
                    })
            })
        };
        let mut point = |role: DefiningRole, at: [f64; 2]| {
            role_point(role).unwrap_or_else(|| {
                self.points.push(at);
                self.points.len() - 1
            })
        };
        match curve {
            PlanarCurve::Line { start, end } => {
                let (start, end) = (start.get(), end.get());
                Ok(CurveGeometry::Line {
                    start: point(DefiningRole::Start, frame.point(start.u, start.v)),
                    end: point(DefiningRole::End, frame.point(end.u, end.v)),
                })
            }
            PlanarCurve::Circle { center, radius } => {
                let center = center.get();
                Ok(CurveGeometry::Circle {
                    center: point(DefiningRole::Center, frame.point(center.u, center.v)),
                    radius: radius.get() / MM_PER_CM,
                })
            }
            PlanarCurve::Arc {
                center,
                radius,
                start,
                end,
            } => {
                let (center, radius) = (center.get(), radius.get());
                let (start, end) = (start.get(), end.get());
                let at = |angle: f64| {
                    let (sin, cos) = angle.portable_sin_cos();
                    frame.point(center.u + radius * cos, center.v + radius * sin)
                };
                let mut span = (end - start).rem_euclid(FULL_TURN);
                if span == 0.0 {
                    span = FULL_TURN;
                }
                let center = point(DefiningRole::Center, frame.point(center.u, center.v));
                let source_start = point(DefiningRole::Start, at(start));
                let source_end = point(DefiningRole::End, at(end));
                // A reflection reverses the arc: its source end is Fusion's
                // counterclockwise start.
                let (first, start, end) = if frame.sense > 0.0 {
                    (frame.angle(start), source_start, source_end)
                } else {
                    (frame.angle(end), source_end, source_start)
                };
                // Both stored angles lie within one turn of zero.
                let mut first = first.rem_euclid(FULL_TURN);
                if first + span > FULL_TURN {
                    first -= FULL_TURN;
                }
                Ok(CurveGeometry::Arc {
                    center,
                    start,
                    end,
                    radius: radius / MM_PER_CM,
                    angles: [first, first + span],
                })
            }
            PlanarCurve::InfiniteLine { .. } => Err("Fusion stores no unbounded sketch line"),
            PlanarCurve::Ellipse { .. }
            | PlanarCurve::Parabola { .. }
            | PlanarCurve::Hyperbola { .. }
            | PlanarCurve::Nurbs(_)
            | PlanarCurve::Spatial(_) => {
                Err("the writer writes points, lines, arcs and circles of a sketch")
            }
            PlanarCurve::Point(_) => Err("a point is no curve"),
        }
    }
}

/// The written relations of `constraint` in Fusion's sketch frame, or the
/// reason it is not written.
fn relations(
    constraint: &SketchConstraint<Planar>,
    written: &HashMap<&SketchEntityId, Entity>,
    curves: &[PlannedCurve],
    frame: Frame,
) -> Result<Vec<PlannedRelation>, &'static str> {
    if matches!(constraint.active, Some(Dimension::Literal(false))) {
        return Err("Fusion stores no inactive sketch relation");
    }
    let member = |operand: &Operand<Planar>| match operand {
        Operand::Entity(id) => written
            .get(id)
            .copied()
            .ok_or("it names an entity the writer does not write"),
        Operand::Frame(_) | Operand::Model(_) => {
            Err("the writer writes relations between the sketch's own entities")
        }
    };
    let two = |kind, a: &Operand<Planar>, b: &Operand<Planar>| {
        Ok(vec![PlannedRelation {
            kind,
            members: vec![member(a)?, member(b)?],
        }])
    };
    let one_or_two = |kind, a: &Operand<Planar>, b: &Option<Operand<Planar>>| {
        let mut members = vec![member(a)?];
        if let Some(b) = b {
            members.push(member(b)?);
        }
        Ok(vec![PlannedRelation { kind, members }])
    };
    match &constraint.relation {
        SketchRelation::Coincident { a, b } => two(RelationKind::Coincident, a, b),
        SketchRelation::Concentric { a, b } => two(RelationKind::Concentric, a, b),
        SketchRelation::Collinear { a, b } => two(RelationKind::Collinear, a, b),
        SketchRelation::Parallel { a, b } => two(RelationKind::Parallel, a, b),
        SketchRelation::Perpendicular { a, b } => two(RelationKind::Perpendicular, a, b),
        SketchRelation::Tangent { a, b, .. } => two(RelationKind::Tangent(None), a, b),
        SketchRelation::Curvature { a, b, .. } => two(RelationKind::Curvature, a, b),
        SketchRelation::Horizontal { a, b } => one_or_two(orientation(frame.x)?, a, b),
        SketchRelation::Vertical { a, b } => one_or_two(orientation(frame.y)?, a, b),
        SketchRelation::Midpoint { point, of } => Ok(vec![PlannedRelation {
            kind: RelationKind::Midpoint,
            members: vec![member(&Operand::Entity(point.clone()))?, member(of)?],
        }]),
        SketchRelation::Symmetric { a, b, about } => Ok(vec![PlannedRelation {
            kind: RelationKind::Symmetric,
            members: vec![member(a)?, member(b)?, member(about)?],
        }]),
        SketchRelation::Equal { members } => {
            let members = members.iter().map(member).collect::<Result<Vec<_>, _>>()?;
            let [first, rest @ ..] = members.as_slice() else {
                return Err("an equality names no entity");
            };
            let round = |entity: &Entity| match entity {
                Entity::Curve(curve) => curves
                    .get(*curve)
                    .map(|curve| !matches!(curve.geometry, CurveGeometry::Line { .. })),
                Entity::Point(_) => None,
            };
            let kind = match members.iter().map(round).collect::<Option<Vec<_>>>() {
                Some(round) if round.iter().all(|r| !r) => RelationKind::EqualLength,
                Some(round) if round.iter().all(|r| *r) => RelationKind::EqualRadius,
                _ => return Err("an equality names a point, or a line with an arc or circle"),
            };
            Ok(rest
                .iter()
                .map(|other| PlannedRelation {
                    kind,
                    members: vec![*first, *other],
                })
                .collect())
        }
        SketchRelation::Distance { .. }
        | SketchRelation::Angle { .. }
        | SketchRelation::Radial { .. }
        | SketchRelation::Length { .. } => {
            Err("the writer does not write sketch dimensions; their parameters are user parameters")
        }
        SketchRelation::Fixed { .. } => Err("Fusion stores no fix relation"),
        SketchRelation::EqualDistance { .. }
        | SketchRelation::Offset(_)
        | SketchRelation::Pattern(_)
        | SketchRelation::Polygon(_)
        | SketchRelation::Slot(_)
        | SketchRelation::Pierce { .. }
        | SketchRelation::Weight { .. }
        | SketchRelation::FixedLength { .. }
        | SketchRelation::FixedAngle { .. }
        | SketchRelation::Native { .. } => Err("the writer writes no relation of this kind"),
    }
}

/// The contact a tangency or curvature relation states.
fn contact(relation: &SketchRelation<Planar>) -> Option<&Contact<Planar>> {
    match relation {
        SketchRelation::Tangent { contact, .. } | SketchRelation::Curvature { contact, .. } => {
            contact.as_ref()
        }
        _ => None,
    }
}

/// The Fusion relation that holds an entity along `axis`, a source sketch
/// axis in Fusion's sketch coordinates: horizontal along Fusion's u axis,
/// vertical along its v axis.
fn orientation(axis: [f64; 2]) -> Result<RelationKind, &'static str> {
    if axis[1].abs() <= AXIS_TOLERANCE {
        Ok(RelationKind::Horizontal)
    } else if axis[0].abs() <= AXIS_TOLERANCE {
        Ok(RelationKind::Vertical)
    } else {
        Err("its source axis lies along neither axis of Fusion's sketch frame")
    }
}

fn vector(vector: &cadmpeg_ir::math::Vector3) -> [f64; 3] {
    [vector.x, vector.y, vector.z]
}

fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
