// SPDX-License-Identifier: Apache-2.0
//! The source's planar sketches as Fusion `Sketch` timeline items on the
//! origin planes or on offset construction planes (cadir-v11.md §3.4; f3d.md
//! §3.1, "Records of a Sketch item" and "Records of a construction-plane
//! item"): which sketches are written, each entity and relation in the frame
//! Fusion stores for its plane, and the losses of what is not written. A
//! tangency at the ends of its curves is written with its endpoint branch and
//! the coincidence of those ends, and a dimension as a Fusion sketch
//! dimension that owns its parameter.

pub(super) mod dimensions;

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
use cadmpeg_ir::design::selection::{PickTarget, Picked, Picks, PrincipalEntity};
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::features::FeatureId;
use cadmpeg_ir::math::portable::Portable;
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::sketches::{SketchEntityId, SketchId};

use crate::loss::F3dLossCode;
use dimensions::{Candidate, PlannedDimension, Tolerances};

use super::expression;
use super::parameters::{OwnedParameter, ParameterPlan};
use cadmpeg_ir::design::units::UnitSymbol;

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
/// Why a sketch whose plane is parallel to no origin plane is not written.
const TILTED: &str = "lies on a plane tilted from every origin plane";
/// The expression of a construction plane's `ExtraOffset` parameter (f3d.md
/// §3.1, "Offset parameters").
const EXTRA_OFFSET: &str = "0.00 mm";

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

    /// The origin, u direction and v direction of the plane's origin record,
    /// as the placement transform of a sketch on it stores them (f3d.md §3.1,
    /// "Placement path").
    pub(super) const fn entity_frame(self) -> [[f64; 3]; 3] {
        match self {
            Self::Xy => [[0.0, 0.0, 0.0], [1.0, 0.0, -0.0], [-0.0, 1.0, 0.0]],
            Self::Xz => [[0.0, 0.0, 0.0], [0.0, 0.0, 1.0], [1.0, 0.0, 0.0]],
            Self::Yz => [[0.0, 0.0, 0.0], [-0.0, 1.0, 0.0], [0.0, -0.0, 1.0]],
        }
    }

    /// The u direction, v direction and normal `u × v` of the plane's origin
    /// record: the axes of a construction plane offset from it.
    pub(super) fn entity_axes(self) -> [[f64; 3]; 3] {
        let [_, u, v] = self.entity_frame();
        [u, v, cross(u, v)]
    }

    pub(super) const fn name(self) -> &'static str {
        match self {
            Self::Xy => "XY",
            Self::Xz => "XZ",
            Self::Yz => "YZ",
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

/// The plane a written sketch lies on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Support {
    Origin(OriginPlane),
    /// A construction plane, by its position in [`SketchPlan::planes`].
    Construction(usize),
}

/// A Fusion `WorkPlane` item that offsets a construction plane from an origin
/// plane, for the sketches that lie on it.
#[derive(Debug)]
pub(super) struct PlannedPlane<'a> {
    pub(super) base: OriginPlane,
    /// The offset along the normal of `base`'s origin record, in cm.
    pub(super) offset: f64,
    /// The `AlongDistance` expression.
    expression: String,
    /// The support the source names, which a plane is shared by.
    key: Option<&'a [Picked]>,
    /// The position in [`SketchPlan::items`] of the first sketch on it, which
    /// the item precedes in the timeline.
    pub(super) first: usize,
    sketches: Vec<&'a SketchId>,
    /// The `AlongDistance` and `ExtraOffset` parameters.
    pub(super) parameters: Option<[OwnedParameter; 2]>,
}

impl PlannedPlane<'_> {
    /// The u direction, v direction and normal of the plane.
    pub(super) fn axes(&self) -> [[f64; 3]; 3] {
        self.base.entity_axes()
    }

    /// The plane's origin in cm: the base plane's origin moved `offset`
    /// along its normal.
    pub(super) fn origin(&self) -> [f64; 3] {
        let ([origin, ..], [.., normal]) = (self.base.entity_frame(), self.axes());
        [
            origin[0] + self.offset * normal[0],
            origin[1] + self.offset * normal[1],
            origin[2] + self.offset * normal[2],
        ]
    }

    /// The row-major matrix whose columns are the u direction, the v
    /// direction, the normal and the origin.
    pub(super) fn matrix(&self) -> [f64; 16] {
        let ([u, v, n], o) = (self.axes(), self.origin());
        [
            u[0], v[0], n[0], o[0], u[1], v[1], n[1], o[1], u[2], v[2], n[2], o[2], 0.0, 0.0, 0.0,
            1.0,
        ]
    }

    /// The note that names the sketches on the plane and the source support
    /// the plane stands for.
    fn note(&self, index: usize) -> LossNote {
        let sketches = self
            .sketches
            .iter()
            .map(|sketch| sketch.as_str())
            .collect::<Vec<_>>()
            .join(" ");
        let parameter = self
            .parameters
            .as_ref()
            .map_or_else(String::new, |[along, _]| {
                format!(", with offset parameter {}", along.name)
            });
        let support = self.key.map_or_else(
            || "the source states the sketch frame and no support the writer reads".to_owned(),
            |picks| {
                let named = picks
                    .iter()
                    .map(|pick| match &pick.target {
                        PickTarget::Datum { feature, .. }
                        | PickTarget::Feature(feature)
                        | PickTarget::FeatureResult(feature) => feature.to_string(),
                        PickTarget::Entity(_) => "a topology selection".to_owned(),
                        _ => "a selection".to_owned(),
                    })
                    .collect::<Vec<_>>()
                    .join(" ");
                format!(
                    "the source support {named} is not written; its name and parameter are lost"
                )
            },
        );
        F3dLossCode::WriterSketchSupportSubstituted.note(format!(
            "F3D places sketches {sketches} on construction plane {}, offset {} from {}{parameter}: {support}",
            index + 1,
            self.expression,
            self.base.name(),
        ))
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
    pub(super) sketch: &'a SketchId,
    pub(super) support: Support,
    /// Whether the source normal is opposite the plane's, so that the
    /// sketch is written as its reflection.
    pub(super) mirrored: bool,
    /// The points in sketch coordinates in cm, without the origin point.
    pub(super) points: Vec<[f64; 2]>,
    pub(super) curves: Vec<PlannedCurve>,
    pub(super) relations: Vec<PlannedRelation>,
    /// The dimensions measured on the written geometry, before their
    /// parameters are planned.
    pub(super) candidates: Vec<Candidate<'a>>,
    /// The written dimensions, in constraint order.
    pub(super) dimensions: Vec<PlannedDimension>,
    /// The content of the sketch the writer does not write.
    pub(super) losses: Vec<LossNote>,
}

impl PlannedSketch<'_> {
    /// The origin plane the sketch lies on, `None` on a construction plane.
    pub(super) fn origin_plane(&self) -> Option<OriginPlane> {
        match self.support {
            Support::Origin(plane) => Some(plane),
            Support::Construction(_) => None,
        }
    }
}

/// The sketches the design writes, in timeline order, and what it does not
/// write of them.
#[derive(Debug, Default)]
pub(super) struct SketchPlan<'a> {
    pub(super) items: Vec<PlannedSketch<'a>>,
    /// The construction planes, in timeline order.
    pub(super) planes: Vec<PlannedPlane<'a>>,
    /// The typed sketches not written, with the reason.
    pub(super) unwritten: BTreeMap<&'a FeatureId, String>,
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
        self.planes.clear();
    }

    /// Write none of the sketches' dimensions, for `reason`: a failure of
    /// the design with them. Their parameters stay user parameters.
    pub(super) fn drop_dimensions(&mut self, reason: &str) {
        for item in &mut self.items {
            let dropped = item
                .candidates
                .drain(..)
                .map(|candidate| candidate.constraint.to_string())
                .collect::<Vec<_>>();
            if !dropped.is_empty() {
                item.losses
                    .push(F3dLossCode::WriterSketchContentOmitted.note(format!(
                        "F3D writes sketch {} without {}: the writer failed with the sketch's dimensions ({reason})",
                        item.sketch,
                        dropped.join(" ")
                    )));
            }
        }
    }

    /// Plan the parameters of each construction plane and of each measured
    /// dimension in timeline order, with source ordinals from `1`: a plane
    /// before the dimensions of its first sketch. A dimension the writer
    /// cannot write is charged. Returns the next source ordinal.
    pub(super) fn claim_parameters(
        &mut self,
        parameters: &mut ParameterPlan,
        tolerances: Tolerances,
    ) -> u32 {
        let mut ordinal = 1u32;
        for (position, item) in self.items.iter_mut().enumerate() {
            if let Some(plane) = self.planes.iter_mut().find(|plane| plane.first == position) {
                if let Some(next) = ordinal.checked_add(2) {
                    plane.parameters = Some([
                        parameters.owned(
                            ordinal,
                            plane.expression.clone(),
                            UnitSymbol::Mm,
                            plane.offset,
                            false,
                        ),
                        parameters.owned(
                            ordinal + 1,
                            EXTRA_OFFSET.to_owned(),
                            UnitSymbol::Mm,
                            0.0,
                            false,
                        ),
                    ]);
                    ordinal = next;
                }
            }
            let mut notes = Notes::default();
            for candidate in std::mem::take(&mut item.candidates) {
                let constraint = candidate.constraint.to_string();
                let planned = ordinal
                    .checked_add(1)
                    .ok_or("its source ordinal exceeds u32")
                    .and_then(|next| {
                        let planned = candidate.plan(parameters, ordinal, tolerances)?;
                        ordinal = next;
                        Ok(planned)
                    });
                match planned {
                    Ok(planned) => item.dimensions.push(planned),
                    Err(reason) => notes.omit(constraint, reason),
                }
            }
            item.losses.extend(notes.into_losses(item.sketch.as_str()));
        }
        ordinal
    }

    /// Whether `feature` is written as a `Sketch` item.
    pub(super) fn writes(&self, feature: &FeatureId) -> bool {
        self.items.iter().any(|item| item.feature == feature)
    }

    pub(super) fn into_losses(self) -> Vec<LossNote> {
        let planes = self
            .planes
            .iter()
            .enumerate()
            .map(|(index, plane)| plane.note(index))
            .collect::<Vec<_>>();
        self.items
            .into_iter()
            .flat_map(|item| item.losses)
            .chain(planes)
            .collect()
    }
}

/// The plan of the typed sketches among `features`, the root scope's
/// features in timeline order. `planes` is the failure that dropped the
/// construction planes, if one did: the sketches off the origin planes are
/// then not written.
pub(super) fn plan<'a>(
    target: &'a CadIr,
    features: &[&'a FeatureId],
    planes: Option<&str>,
) -> SketchPlan<'a> {
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
        let (placement, frame) = match placement(target, sketch, tolerance) {
            Ok(placed) => placed,
            Err(reason) => {
                out.unwritten.insert(id, reason.into());
                continue;
            }
        };
        let support = match placement {
            Placement::Origin(plane) => Support::Origin(plane),
            Placement::Construction { base, offset, key } => {
                if let Some(reason) = planes {
                    out.unwritten.insert(
                        id,
                        format!("is dropped because the writer failed with the design's construction planes ({reason})"),
                    );
                    continue;
                }
                let shared = out.planes.iter().position(|plane| {
                    plane.base == base
                        && key.is_some()
                        && plane.key == key
                        && (plane.offset * MM_PER_CM - offset).abs() <= tolerance
                });
                let index = match shared {
                    Some(index) => index,
                    None => {
                        let Some(expression) =
                            expression::quantity_literal(offset, Some(UnitSymbol::Mm))
                        else {
                            out.unwritten
                                .insert(id, "states a plane offset with no Fusion literal".into());
                            continue;
                        };
                        out.planes.push(PlannedPlane {
                            base,
                            offset: offset / MM_PER_CM,
                            expression,
                            key,
                            first: out.items.len(),
                            sketches: Vec::new(),
                            parameters: None,
                        });
                        out.planes.len() - 1
                    }
                };
                out.planes[index].sketches.push(sketch_id);
                Support::Construction(index)
            }
        };
        let mut notes = Notes::default();
        let mut planned = PlannedSketch::new(target, id, sketch, support, frame, &mut notes);
        planned.losses = notes.into_losses(sketch_id.as_str());
        out.items.push(planned);
    }
    out
}

/// Where a sketch is placed.
enum Placement<'a> {
    Origin(OriginPlane),
    /// A plane parallel to `base` at `offset` mm along the normal of
    /// `base`'s origin record, with the support the source names.
    Construction {
        base: OriginPlane,
        offset: f64,
        key: Option<&'a [Picked]>,
    },
}

/// The plane of `sketch` and the map from its sketch coordinates to
/// Fusion's: the origin plane it names or lies on, else a plane parallel to
/// an origin plane.
fn placement<'a>(
    target: &CadIr,
    sketch: &'a Sketch,
    tolerance: f64,
) -> Result<(Placement<'a>, Frame), &'static str> {
    let construction = match &sketch.plane {
        SketchPlane::Placed(construction) => construction,
        SketchPlane::Block => return Err("is a block definition"),
    };
    let frame = construction.resolved.as_ref().ok_or("states no frame")?;
    let normal = vector(frame.z_axis.as_raw());
    let origin = frame.origin.get();
    let origin = [origin.x, origin.y, origin.z];
    let picked = picked_plane(target, construction);
    if let Some(Pick::Plane(named)) = picked {
        let axes = named.axes();
        if dot(origin, axes[2]).abs() > tolerance
            || (dot(normal, axes[2]).abs() - 1.0).abs() > NORMAL_TOLERANCE
        {
            return Err("lies on no origin plane");
        }
        return Ok((
            Placement::Origin(named),
            Frame::new(named.axes(), frame, origin),
        ));
    }
    let base = OriginPlane::along(normal).ok_or(TILTED)?;
    let offset = dot(origin, base.axes()[2]);
    if matches!(picked, Some(Pick::Unread)) && offset.abs() <= tolerance {
        return Ok((
            Placement::Origin(base),
            Frame::new(base.axes(), frame, origin),
        ));
    }
    let key = match construction.inputs.as_slice() {
        [input] => match picks(input) {
            Some(Picks::Named(members)) => Some(members.as_slice()),
            _ => None,
        },
        _ => None,
    };
    Ok((
        Placement::Construction { base, offset, key },
        Frame::new(base.entity_axes(), frame, origin),
    ))
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
/// coordinates (cm) on its plane: a rotation, or a reflection where the
/// source normal is opposite the plane's.
#[derive(Clone, Copy, Debug)]
struct Frame {
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
    /// The map onto the Fusion sketch plane whose u axis, v axis and normal
    /// are `axes` and whose origin is the foot of the model origin.
    fn new(axes: [[f64; 3]; 3], frame: &FrameValue, origin: [f64; 3]) -> Self {
        let [u_axis, v_axis, normal] = axes;
        let in_plane = |direction: [f64; 3]| [dot(direction, u_axis), dot(direction, v_axis)];
        let x = in_plane(vector(frame.x_axis.as_raw()));
        let y = in_plane(vector(frame.y_axis.as_raw()));
        let sense = if dot(vector(frame.z_axis.as_raw()), normal) < 0.0 {
            -1.0
        } else {
            1.0
        };
        Self {
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
        sketch: &'a Sketch,
        support: Support,
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
            sketch: &sketch.id,
            support,
            mirrored: frame.sense < 0.0,
            points: Vec::new(),
            curves: Vec::new(),
            relations: Vec::new(),
            candidates: Vec::new(),
            dimensions: Vec::new(),
            losses: Vec::new(),
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
            let geometry = dimensions::Geometry {
                points: &planned.points,
                curves: &planned.curves,
                frame,
                angular: target.tolerances.angular.get(),
            };
            let mut relations = match relations(constraint, &written, &geometry) {
                Ok(Written::Relations(relations)) => relations,
                Ok(Written::Dimension(candidate)) => {
                    planned.candidates.push(candidate);
                    continue;
                }
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

/// What a constraint of a written sketch is written as.
enum Written<'a> {
    Relations(Vec<PlannedRelation>),
    Dimension(Candidate<'a>),
}

/// The written relations of `constraint` in Fusion's sketch frame, or its
/// dimension measured on `geometry`; or the reason it is not written.
fn relations<'a>(
    constraint: &'a SketchConstraint<Planar>,
    written: &HashMap<&SketchEntityId, Entity>,
    geometry: &dimensions::Geometry<'_>,
) -> Result<Written<'a>, &'static str> {
    let (curves, frame) = (geometry.curves, geometry.frame);
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
    let relations = match &constraint.relation {
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
            return dimensions::candidate(constraint, &member, geometry).map(Written::Dimension)
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
    };
    relations.map(Written::Relations)
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

fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}
