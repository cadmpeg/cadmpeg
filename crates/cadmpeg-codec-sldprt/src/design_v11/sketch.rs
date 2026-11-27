// SPDX-License-Identifier: Apache-2.0
//! Planar sketches (§6.1), from the decoder's typed sketches.
//!
//! A sketch feature whose sketch the decoder typed names a sketch record that
//! holds its entities and relations. The plane is a construction: one
//! `Coincident` input on the built-in plane the sketch's profile names where
//! that plane precedes the sketch and the stored frame lies in it, else the
//! `Unread` input of §5.9, with the stored frame as its value. A point record
//! that several curves name is one point entity with a role for each, so a
//! coincidence of two of its ends is that entity's. An entity is
//! construction where the decoder marks it construction or reads it from a
//! curve record of state `0`, which no profile uses, except in a sketch
//! where another curve closes a loop only with the curves of state `0`;
//! another entity states no flag. A circle the decoder built from a
//! dimension record (open item DI-83) is left out where it repeats a stored
//! circle or arc of its sketch, or where no cylindrical face of the solid
//! lies on it and its center lies on a stored curve of its radius; the
//! dimensions that named it then name that curve. Only radial dimensions
//! move to a curve the center lies on, where the center is a point of that
//! curve; a native dimension also moves to a curve the circle repeats, which
//! has its center. Where another relation names it, it is construction. A
//! built circle no relation names is left out, and any other is
//! construction unless a cylindrical face of the solid lies on it. A
//! dimension binds the parameter that holds its value, and states that
//! parameter's quantity; a reference
//! dimension makes the tree parameter it binds `Driven`, and one whose
//! parameter no tree node lists binds a `Driven` parameter minted for it. A
//! sketch with a dimension that binds no parameter, or with an entity that
//! has no planar curve, stays `Unresolved`; its dimensions still state the
//! quantities of the parameters they bind.

use std::collections::{HashMap, HashSet};

use cadmpeg_core::text::NonBlankString;
use cadmpeg_ir::design::construction::{
    kinds, Construction, ConstructionInput, FrameValue, InputReference, InputRole, Relation,
    ScalarDimension, Subject,
};
use cadmpeg_ir::design::dimension::Dimension;
use cadmpeg_ir::design::evaluation::EvaluationStatus;
use cadmpeg_ir::design::feature::{Feature, FeatureOperation, OperationFamily};
use cadmpeg_ir::design::ops::sketch::{
    AngleOperands, DefiningRole, DistanceAxis, EntityDefinition, EntityRole, Operand, Planar,
    PlanarCurve, RadialMeasure, Sketch, SketchConstraint, SketchEntity, SketchPlane,
    SketchRelation, SketchSolve,
};
use cadmpeg_ir::design::options::NativeOptions;
use cadmpeg_ir::design::parameter::{
    DesignParameter, DimensionDisplay, ParameterDefinition, ParameterOwner, ParameterValue,
};
use cadmpeg_ir::design::selection::{
    Anchor, Members, OrderedSelection, PickTarget, Picked, Picks, PrincipalEntity, Selection,
};
use cadmpeg_ir::design::units::{Quantity, ValueKind};
use cadmpeg_ir::features::{FeatureDefinition, FeatureId, ParameterId};
use cadmpeg_ir::ids::ProductDefinitionId;
use cadmpeg_ir::scalar::FiniteReal;
use cadmpeg_ir::sketches::{
    SketchConstraintDefinitionInput as Kind, SketchEntityId, SketchGeometryDefinition as Geometry,
    SketchLocus,
};
use cadmpeg_ir::units::{UnitVector2, UnitVector3};

use super::parameters::Entries;
use super::profile_check;
use crate::brep::graph::LINEAR_RESOLUTION_MM;
use crate::resolved_features::RELATION_INSTANCE_ID_PREFIX;

/// Distance within which a stored point lies on a plane or at a curve's end,
/// in millimetres per millimetre of the larger coordinate.
const EPS_SKETCH_INCIDENCE: f64 = 1.0e-9;

/// How far from one the cosine between a cylinder's axis and a sketch normal
/// may be for the axis to lie along the normal.
const EPS_AXIS_ALONG_NORMAL: f64 = 1.0e-8;

/// The sketch records of the part, and what their dimensions state of the
/// parameters they bind.
#[derive(Default)]
pub(super) struct Sketches {
    pub(super) records: Vec<Sketch>,
    pub(super) entities: Vec<SketchEntity<Planar>>,
    pub(super) constraints: Vec<SketchConstraint<Planar>>,
    /// The `Driven` parameters minted for reference dimensions.
    pub(super) driven: Vec<DesignParameter>,
    /// The tree parameters a reference dimension binds.
    pub(super) references: HashSet<ParameterId>,
    /// The quantity of each parameter a dimension binds.
    pub(super) bound: HashMap<ParameterId, Quantity>,
    /// The parameters a radius or diameter dimension binds, by what it
    /// measures.
    pub(super) measured: HashMap<ParameterId, DimensionDisplay>,
    /// The sketch features left `Unresolved`, with the reason.
    pub(super) unprojected: Vec<(FeatureId, &'static str)>,
    /// The entities stated construction because their curve record has
    /// state `0`, whose construction the decoder does not mark.
    pub(super) outside_profile: Vec<SketchEntityId>,
    /// The circles built from a dimension record that the design leaves
    /// out, or states construction although the decoder does not mark them.
    pub(super) dimension_circles: Vec<DimensionCircle>,
}

/// A circle built from a dimension record that the design does not keep as
/// decoded: what its sketch and the solid say of it, and what the design
/// does with it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct DimensionCircle {
    entity: SketchEntityId,
    reading: Reading,
    outcome: Outcome,
}

/// What the sketch and the solid say of a built circle.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Reading {
    /// It has the center and radius of this stored circle or arc.
    Repeats(Candidate),
    /// Its center lies on this stored circle or arc, which has its radius,
    /// and no cylindrical face of the solid is found on it.
    OnCurve(Candidate),
    /// It has the center and radius of this earlier built circle.
    RepeatsBuilt(SketchEntityId),
    /// No stored curve of its sketch has its radius, and no cylindrical face
    /// of the solid lies on it.
    NoCurveOfRadius,
    /// A stored curve of its radius lies elsewhere in its sketch, and no
    /// cylindrical face of the solid lies on it.
    Unwitnessed,
    /// Its sketch has no resolved frame to compare with the solid.
    Unplaced,
}

/// The stored curve a built circle is read on: the first of `among` stored
/// curves on one carrier, in record order. The file does not say which of
/// several the dimension measured.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Candidate {
    curve: SketchEntityId,
    among: usize,
}

/// What the design does with a built circle.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Outcome {
    /// Left out; these dimensions, which named it, name the stored curve.
    Redirected(Vec<String>),
    /// Left out; no relation of the sketch names it.
    Unnamed,
    /// Stated construction although it is read on a stored curve: these
    /// relations name it and do not move to that curve.
    Kept(Vec<String>),
    /// Stated construction.
    Construction,
}

impl Outcome {
    fn left_out(&self) -> bool {
        matches!(self, Self::Redirected(_) | Self::Unnamed)
    }
}

impl DimensionCircle {
    /// The circle, the reading and the outcome, for a loss note.
    pub(super) fn text(&self) -> String {
        let first = |among: usize| match among {
            1 => String::new(),
            _ => format!(", the first of {among} such curves on one carrier"),
        };
        let reading = match &self.reading {
            Reading::Repeats(Candidate { curve, among }) => {
                format!("it repeats {}{}", curve.as_str(), first(*among))
            }
            Reading::OnCurve(Candidate { curve, among }) => format!(
                "its center lies on {}, of its radius{}",
                curve.as_str(),
                first(*among)
            ),
            Reading::RepeatsBuilt(circle) => format!("it repeats {}", circle.as_str()),
            Reading::NoCurveOfRadius if self.outcome == Outcome::Unnamed => {
                "no stored curve of its sketch has its radius and no face of the solid lies \
                 on it"
                    .to_owned()
            }
            Reading::NoCurveOfRadius => "no stored curve of its sketch has its radius and no \
                                         face of the solid lies on it, so the profile lacks the \
                                         circle its dimension measures"
                .to_owned(),
            Reading::Unwitnessed => "no face of the solid lies on it".to_owned(),
            Reading::Unplaced => {
                "its sketch has no resolved frame to compare with the solid".to_owned()
            }
        };
        let outcome = match &self.outcome {
            Outcome::Redirected(dimensions) => match dimensions.as_slice() {
                [dimension] => format!("left out; the dimension {dimension} names that curve"),
                _ => format!(
                    "left out; the dimensions {} name that curve",
                    dimensions.join(", ")
                ),
            },
            Outcome::Unnamed => "left out; no relation names it".to_owned(),
            Outcome::Kept(relations) => match relations.as_slice() {
                [relation] => format!(
                    "construction; the relation {relation} names it and does not move to that \
                     curve"
                ),
                _ => format!(
                    "construction; the relations {} name it and do not move to that curve",
                    relations.join(", ")
                ),
            },
            Outcome::Construction => "construction".to_owned(),
        };
        format!("{} ({reading}; {outcome})", self.entity.as_str())
    }
}

/// What the projection reads besides the v10 sketch records.
pub(super) struct Context<'a> {
    pub(super) model: &'a cadmpeg_ir::document::Model,
    pub(super) entries: &'a Entries<'a>,
    pub(super) scope: &'a ProductDefinitionId,
    /// The datum feature of the built-in plane each sketch feature's profile
    /// names, with that plane's principal entity.
    pub(super) supports: &'a HashMap<FeatureId, (FeatureId, PrincipalEntity)>,
    /// The entities read from a curve record of state `0`.
    pub(super) outside_profile: &'a HashSet<SketchEntityId>,
    /// The surfaces of the current geometry.
    pub(super) surfaces: &'a [cadmpeg_ir::geometry::Surface],
}

/// Type each `Unresolved` sketch feature of `features` whose sketch the
/// decoder typed.
pub(super) fn project(features: &mut [Feature], context: &Context<'_>) -> Sketches {
    let mut out = Sketches::default();
    let mut preceding = HashSet::new();
    for feature in features.iter_mut() {
        if matches!(
            feature.definition.operation,
            FeatureOperation::Unresolved {
                family: OperationFamily::Sketch
            }
        ) {
            let support = context
                .supports
                .get(&feature.id)
                .filter(|(datum, _)| preceding.contains(datum));
            match sketch(feature, context, support, &mut out) {
                Ok(id) => feature.definition.operation = FeatureOperation::Sketch(id),
                Err(reason) => out.unprojected.push((feature.id.clone(), reason)),
            }
        }
        preceding.insert(feature.id.clone());
    }
    out
}

fn sketch(
    feature: &Feature,
    context: &Context<'_>,
    support: Option<&(FeatureId, PrincipalEntity)>,
    out: &mut Sketches,
) -> Result<SketchId, &'static str> {
    let model = context.model;
    let source = model
        .features
        .iter()
        .find(|candidate| candidate.id == feature.id)
        .ok_or("the sketch feature has no decoded record")?;
    let FeatureDefinition::Operation(cadmpeg_ir::features::FeatureOperation::Sketch { sketch }) =
        source.evaluation.definition()
    else {
        return Err("the sketch feature has no decoded sketch");
    };
    let id = sketch
        .id()
        .ok_or("the sketch feature names no planar sketch")?;
    let record = model
        .sketches
        .iter()
        .find(|candidate| &candidate.id == id)
        .ok_or("the sketch record is missing")?;
    let entities = model
        .sketch_entities
        .iter()
        .filter(|entity| entity.sketch == record.id)
        .collect::<Vec<_>>();
    let roles = Roles::new(&entities);
    let mut failure = None;
    let mut sketch_entities = Vec::with_capacity(entities.len());
    let mut outside_profile = Vec::new();
    let forms = entities
        .iter()
        .map(|entity| curve(&entity.geometry))
        .collect::<Vec<_>>();
    let mut readings = Vec::with_capacity(entities.len());
    for index in 0..entities.len() {
        let reading = reading(index, &entities, &readings, record, context.surfaces);
        readings.push(reading);
    }
    let needed = needed_state_zero(&entities, &forms, &readings, context.outside_profile);
    for ((entity, form), reading) in entities
        .iter()
        .zip(forms.iter().cloned())
        .zip(readings.iter())
    {
        let Some(form) = form else {
            failure.get_or_insert("a sketch entity has no planar curve");
            continue;
        };
        let outside =
            context.outside_profile.contains(entity.id()) && !needed.contains(entity.id());
        if outside && !entity.construction {
            outside_profile.push(entity.id().clone());
        }
        sketch_entities.push(SketchEntity {
            id: entity.id().clone(),
            sketch: record.id.clone(),
            definition: EntityDefinition::Free(form),
            roles: roles.of(entity.id()),
            construction: (entity.construction || outside || reading.is_some()).then_some(true),
            centerline: None,
            native: None,
            native_options: NativeOptions::new(),
            source_settings: NativeOptions::new(),
            native_ref: entity.native_ref.clone(),
        });
    }
    let mut constraints = Vec::new();
    let mut driven = Vec::new();
    let mut references = Vec::new();
    let mut bound = Vec::new();
    for constraint in model
        .sketch_constraints
        .iter()
        .filter(|constraint| constraint.sketch == record.id)
    {
        let relations = Relations {
            context,
            owner: &feature.id,
            roles: &roles,
            entities: &entities,
        };
        let (relation, binding) = match relations.relation(constraint, &driven) {
            Ok(relation) => relation,
            Err(reason) => {
                failure.get_or_insert(reason);
                continue;
            }
        };
        if matches!(&relation, SketchRelation::Coincident { a, b } if a == b) {
            continue;
        }
        if let Some((parameter, quantity, minted)) = binding {
            if constraint.driving == Some(false) && context.entries.contains(&parameter) {
                references.push(parameter.clone());
            }
            bound.push((parameter, quantity));
            driven.extend(minted);
        }
        constraints.push(SketchConstraint {
            id: constraint.id.clone(),
            sketch: record.id.clone(),
            relation,
            name: constraint.name.clone(),
            active: constraint.active.map(Dimension::Literal),
            native_options: NativeOptions::new(),
            source_settings: NativeOptions::new(),
            native_ref: constraint.native_ref.clone(),
        });
    }
    out.bound.extend(bound);
    out.driven.extend(driven);
    out.references.extend(references);
    if let Some(reason) = failure {
        return Err(reason);
    }
    let dimension_circles = settle(&entities, readings, &mut constraints);
    sketch_entities.retain(|kept| {
        dimension_circles
            .iter()
            .all(|(_, circle)| !circle.outcome.left_out() || circle.entity != kept.id)
    });
    out.records.push(Sketch {
        id: record.id.clone(),
        owner: feature.id.clone(),
        name: record.name.clone(),
        plane: SketchPlane::Placed(Box::new(placement(record.placement, support)?)),
        solve: SketchSolve {
            state: None,
            status: EvaluationStatus::Current,
        },
        native_options: NativeOptions::new(),
        source_settings: NativeOptions::new(),
        native_ref: record.native_ref.clone(),
    });
    out.entities.extend(sketch_entities);
    out.outside_profile.extend(outside_profile);
    out.dimension_circles.extend(
        dimension_circles
            .into_iter()
            .filter(|(construction, circle)| !construction || circle.outcome.left_out())
            .map(|(_, circle)| circle),
    );
    out.measured.extend(
        constraints
            .iter()
            .filter_map(|constraint| match &constraint.relation {
                SketchRelation::Radial {
                    measure,
                    value: Dimension::Parameter(parameter),
                    ..
                } => Some((
                    parameter.clone(),
                    match measure {
                        RadialMeasure::Diameter => DimensionDisplay::Diameter,
                        _ => DimensionDisplay::Radius,
                    },
                )),
                _ => None,
            }),
    );
    out.constraints.extend(constraints);
    Ok(record.id.clone())
}

type SketchId = cadmpeg_ir::sketches::SketchId;

/// The plane of a sketch whose stored placement is `placement`, on `support`
/// where the stored frame lies in that plane.
fn placement(
    placement: cadmpeg_ir::sketches::SketchPlacement,
    support: Option<&(FeatureId, PrincipalEntity)>,
) -> Result<Construction<kinds::Frame>, &'static str> {
    let frame = placement.resolved();
    let resolved = frame
        .map(|(origin, normal, u_axis)| {
            let z_axis = UnitVector3::normalized_nonzero(normal)?;
            let x_axis = UnitVector3::normalized_nonzero(u_axis)?;
            let y_axis = UnitVector3::normalized(z_axis.as_raw().cross(*x_axis.as_raw()))?;
            Some(FrameValue {
                origin,
                x_axis,
                y_axis,
                z_axis,
            })
        })
        .map(|frame| frame.ok_or("the stored sketch frame is degenerate"))
        .transpose()?;
    let on_support = support.filter(|(_, entity)| {
        resolved
            .as_ref()
            .is_some_and(|frame| in_principal_plane(frame, *entity))
    });
    let input = match on_support {
        Some((datum, _)) => ConstructionInput {
            role: Some(InputRole {
                subject: Subject::Whole,
                relation: Relation::Coincident,
            }),
            reference: Some(InputReference::Picks(selection(vec![pick(
                PickTarget::Datum {
                    feature: datum.clone(),
                    part: None,
                },
            )])?)),
            value: None,
            reversed: None,
        },
        None => ConstructionInput {
            role: None,
            reference: Some(InputReference::Picks(Selection {
                at: Anchor::Input,
                picks: Picks::Unread,
                native: None,
            })),
            value: None,
            reversed: None,
        },
    };
    Ok(Construction {
        inputs: vec![input],
        resolved,
        after: Vec::new(),
        solution: None,
        mode: None,
        resolved_in: std::collections::BTreeMap::new(),
    })
}

/// Whether `frame`'s plane is the principal plane `entity`: its normal is the
/// plane's axis, either way, and its origin lies on it.
fn in_principal_plane(frame: &FrameValue, entity: PrincipalEntity) -> bool {
    let axis = match entity {
        PrincipalEntity::XyPlane => 2,
        PrincipalEntity::XzPlane => 1,
        PrincipalEntity::YzPlane => 0,
        _ => return false,
    };
    let normal = frame.z_axis.as_raw();
    let normal = [normal.x, normal.y, normal.z];
    let origin = frame.origin.get();
    let origin = [origin.x, origin.y, origin.z];
    let scale = origin
        .iter()
        .fold(1.0_f64, |scale, value| scale.max(value.abs()));
    (normal[axis].abs() - 1.0).abs() <= EPS_SKETCH_INCIDENCE
        && origin[axis].abs() <= EPS_SKETCH_INCIDENCE * scale
}

/// One pick of `target`, by its target alone.
pub(super) fn pick(target: PickTarget) -> Picked {
    Picked {
        target,
        context: None,
        native: None,
        domain: None,
        witness: None,
        resolved: None,
    }
}

/// An ordered selection of `picks`, anchored in the feature's input state.
pub(super) fn selection(picks: Vec<Picked>) -> Result<OrderedSelection, &'static str> {
    Ok(Selection {
        at: Anchor::Input,
        picks: Picks::Named(Members::new(picks)?),
        native: None,
    })
}

/// The point entity at each end of the sketch's lines and arcs: the point
/// whose native record the curve names as that end and whose position is the
/// end's.
struct Roles {
    ends: HashMap<(SketchEntityId, bool), SketchEntityId>,
}

impl Roles {
    fn new(entities: &[&cadmpeg_ir::sketches::SketchEntity]) -> Self {
        let mut points = HashMap::<&str, Option<&cadmpeg_ir::sketches::SketchEntity>>::new();
        for entity in entities {
            if let (Some(native), Geometry::Point { .. }) =
                (entity.native_ref.as_deref(), entity.geometry.definition())
            {
                points
                    .entry(native)
                    .and_modify(|found| *found = None)
                    .or_insert(Some(entity));
            }
        }
        let mut ends = HashMap::new();
        for curve in entities {
            let Some([start, end]) = curve_ends(&curve.geometry) else {
                continue;
            };
            let [first, second] = curve.endpoint_refs.as_slice() else {
                continue;
            };
            for (native, at, is_start) in [(first, start, true), (second, end, false)] {
                let Some(Some(point)) = points.get(native.as_str()) else {
                    continue;
                };
                let Geometry::Point { position } = point.geometry.definition() else {
                    continue;
                };
                if coincident([position.u, position.v], at) {
                    ends.insert((curve.id().clone(), is_start), point.id().clone());
                }
            }
        }
        Self { ends }
    }

    /// The roles of the point entity `point`.
    fn of(&self, point: &SketchEntityId) -> Vec<EntityRole<Planar>> {
        let mut roles = self
            .ends
            .iter()
            .filter(|(_, at)| *at == point)
            .map(|((curve, is_start), _)| EntityRole {
                of: curve.clone(),
                role: if *is_start {
                    DefiningRole::Start
                } else {
                    DefiningRole::End
                },
            })
            .collect::<Vec<_>>();
        roles.sort_by(|a: &EntityRole<Planar>, b: &EntityRole<Planar>| {
            (a.of.as_str(), matches!(a.role, DefiningRole::End))
                .cmp(&(b.of.as_str(), matches!(b.role, DefiningRole::End)))
        });
        roles
    }

    /// The point entity at the start (`true`) or end of `curve`.
    fn end(&self, curve: &SketchEntityId, start: bool) -> Option<&SketchEntityId> {
        self.ends.get(&(curve.clone(), start))
    }
}

/// What the sketch and the solid say of `entities[index]` where it is a
/// circle built from a dimension record that the design does not keep as
/// decoded; `None` for another entity and for a built circle a cylindrical
/// face of the solid witnesses. A stored circle or arc the circle repeats
/// comes first, then an earlier built circle it repeats, of those `earlier`
/// does not read on a stored curve, then the faces of the solid, then a
/// stored circle or arc its center lies on; the stored candidates count
/// where they share one carrier.
fn reading(
    index: usize,
    entities: &[&cadmpeg_ir::sketches::SketchEntity],
    earlier: &[Option<Reading>],
    record: &cadmpeg_ir::sketches::Sketch,
    surfaces: &[cadmpeg_ir::geometry::Surface],
) -> Option<Reading> {
    let circle = dimension_circle(entities.get(index)?)?;
    let (center, radius) = circle;
    let stored = entities
        .iter()
        .filter(|candidate| !built(candidate))
        .filter_map(|candidate| Some((*candidate, carrier(&candidate.geometry)?)))
        .collect::<Vec<_>>();
    let repeated = stored
        .iter()
        .filter(|(_, at)| same_carrier(*at, circle))
        .copied()
        .collect::<Vec<_>>();
    if let Some(candidate) = one_carrier(&repeated) {
        return Some(Reading::Repeats(candidate));
    }
    let repeats_built = entities[..index]
        .iter()
        .zip(earlier)
        .find(|(candidate, reading)| {
            !matches!(reading, Some(Reading::Repeats(_) | Reading::OnCurve(_)))
                && dimension_circle(candidate).is_some_and(|at| same_carrier(at, circle))
        });
    if let Some((circle, _)) = repeats_built {
        return Some(Reading::RepeatsBuilt(circle.id().clone()));
    }
    let placement = record.resolved_placement();
    if let Some((origin, normal, u_axis)) = placement {
        let v_axis = [
            normal.y * u_axis.z - normal.z * u_axis.y,
            normal.z * u_axis.x - normal.x * u_axis.z,
            normal.x * u_axis.y - normal.y * u_axis.x,
        ];
        let at = [
            origin.x + center[0] * u_axis.x + center[1] * v_axis[0],
            origin.y + center[0] * u_axis.y + center[1] * v_axis[1],
            origin.z + center[0] * u_axis.z + center[1] * v_axis[2],
        ];
        if surfaces
            .iter()
            .any(|surface| on_cylinder(surface, [normal.x, normal.y, normal.z], at, radius))
        {
            return None;
        }
    }
    let of_radius = stored
        .iter()
        .filter(|(_, (_, size))| (size - radius).abs() <= LINEAR_RESOLUTION_MM)
        .copied()
        .collect::<Vec<_>>();
    let through = of_radius
        .iter()
        .filter(|(candidate, _)| passes_through(&candidate.geometry, center))
        .copied()
        .collect::<Vec<_>>();
    if let Some(candidate) = one_carrier(&through) {
        return Some(Reading::OnCurve(candidate));
    }
    Some(if placement.is_none() {
        Reading::Unplaced
    } else if of_radius.is_empty() {
        Reading::NoCurveOfRadius
    } else {
        Reading::Unwitnessed
    })
}

/// The first curve of `candidates`, each with its carrier, and how many
/// there are, where all lie on one carrier; `None` where there are none or
/// their carriers differ.
fn one_carrier(
    candidates: &[(&cadmpeg_ir::sketches::SketchEntity, ([f64; 2], f64))],
) -> Option<Candidate> {
    let (first, at) = candidates.first()?;
    candidates
        .iter()
        .all(|(_, other)| same_carrier(*other, *at))
        .then(|| Candidate {
            curve: first.id().clone(),
            among: candidates.len(),
        })
}

/// Whether two circles or arcs, each as its center and radius, lie on one
/// carrier.
fn same_carrier((a, a_radius): ([f64; 2], f64), (b, b_radius): ([f64; 2], f64)) -> bool {
    near(a, b) && (a_radius - b_radius).abs() <= LINEAR_RESOLUTION_MM
}

/// Whether two points of a sketch are one within the linear resolution.
fn near(a: [f64; 2], b: [f64; 2]) -> bool {
    (a[0] - b[0]).hypot(a[1] - b[1]) <= LINEAR_RESOLUTION_MM
}

/// Whether `point` lies on the circle or arc `geometry`.
fn passes_through(geometry: &cadmpeg_ir::sketches::SketchGeometry, point: [f64; 2]) -> bool {
    use cadmpeg_ir::math::portable::Portable;
    let Some((center, radius)) = carrier(geometry) else {
        return false;
    };
    let offset = [point[0] - center[0], point[1] - center[1]];
    if (offset[0].hypot(offset[1]) - radius).abs() > LINEAR_RESOLUTION_MM {
        return false;
    }
    let Geometry::Arc {
        start_angle,
        end_angle,
        ..
    } = geometry.definition()
    else {
        return true;
    };
    let at_end = curve_ends(geometry).is_some_and(|ends| ends.iter().any(|end| near(*end, point)));
    let tau = std::f64::consts::TAU;
    let turn = (offset[1].portable_atan2(offset[0]) - start_angle.get()).rem_euclid(tau);
    at_end || turn <= (end_angle.get() - start_angle.get()).rem_euclid(tau)
}

/// The built circles of `entities` with a reading, each with whether the
/// decoder marks it construction and what the design does with it: the
/// dimensions that name a circle read on a stored curve name that curve
/// instead, where every relation that names the circle `moves` there.
fn settle(
    entities: &[&cadmpeg_ir::sketches::SketchEntity],
    readings: Vec<Option<Reading>>,
    constraints: &mut [SketchConstraint<Planar>],
) -> Vec<(bool, DimensionCircle)> {
    let mut circles = Vec::new();
    for (entity, reading) in entities.iter().zip(readings) {
        let Some(reading) = reading else {
            continue;
        };
        let id = entity.id();
        let naming = (0..constraints.len())
            .filter(|at| names(&constraints[*at].relation, id))
            .collect::<Vec<_>>();
        let listed = |at: &[usize]| {
            at.iter()
                .map(|at| constraints[*at].id.as_str().to_owned())
                .collect::<Vec<_>>()
        };
        let outcome = match &reading {
            _ if naming.is_empty() => Outcome::Unnamed,
            Reading::Repeats(Candidate { curve, .. })
            | Reading::OnCurve(Candidate { curve, .. }) => {
                let kept = naming
                    .iter()
                    .copied()
                    .filter(|at| !moves(&reading, &constraints[*at].relation, id))
                    .collect::<Vec<_>>();
                if kept.is_empty() {
                    let redirected = listed(&naming);
                    for at in &naming {
                        redirect(&mut constraints[*at].relation, id, curve);
                    }
                    Outcome::Redirected(redirected)
                } else {
                    Outcome::Kept(listed(&kept))
                }
            }
            _ => Outcome::Construction,
        };
        circles.push((
            entity.construction,
            DimensionCircle {
                entity: id.clone(),
                reading,
                outcome,
            },
        ));
    }
    circles
}

/// Whether `relation` names `entity` anywhere among its operands; `true`
/// where the relation does not serialize, so no circle a relation may name
/// is left out.
fn names(relation: &SketchRelation<Planar>, entity: &SketchEntityId) -> bool {
    fn mentions(value: &serde_json::Value, id: &str) -> bool {
        match value {
            serde_json::Value::String(text) => text == id,
            serde_json::Value::Array(items) => items.iter().any(|item| mentions(item, id)),
            serde_json::Value::Object(fields) => fields.values().any(|item| mentions(item, id)),
            _ => false,
        }
    }
    match cadmpeg_core::serde_erase::to_json_value(relation) {
        Ok(value) => mentions(&value, entity.as_str()),
        Err(_) => true,
    }
}

/// Whether `relation`, which names the built circle `entity`, moves to the
/// stored curve `reading` gives: a radial dimension of the circle moves to
/// either curve; a native relation with a value moves only to a curve the
/// circle repeats. Such a relation measures from the circle's center, which
/// a curve the center lies on does not share.
fn moves(reading: &Reading, relation: &SketchRelation<Planar>, entity: &SketchEntityId) -> bool {
    match relation {
        SketchRelation::Radial { of, .. } => of == entity,
        SketchRelation::Native {
            operands, values, ..
        } => {
            matches!(reading, Reading::Repeats(_))
                && !values.is_empty()
                && operands.contains(&Operand::Entity(entity.clone()))
        }
        _ => false,
    }
}

/// Make the dimension `relation`, which `moves` from `from`, name `to`.
fn redirect(relation: &mut SketchRelation<Planar>, from: &SketchEntityId, to: &SketchEntityId) {
    match relation {
        SketchRelation::Radial { of, .. } => *of = to.clone(),
        SketchRelation::Native { operands, .. } => {
            let mut renamed = Vec::with_capacity(operands.len());
            for operand in operands.drain(..) {
                let operand = match operand {
                    Operand::Entity(id) if &id == from => Operand::Entity(to.clone()),
                    other => other,
                };
                if !renamed.contains(&operand) {
                    renamed.push(operand);
                }
            }
            *operands = renamed;
        }
        _ => {}
    }
}

/// Whether `surface` is a cylinder of `radius` whose axis lies along
/// `normal` through `at`.
fn on_cylinder(
    surface: &cadmpeg_ir::geometry::Surface,
    normal: [f64; 3],
    at: [f64; 3],
    radius: f64,
) -> bool {
    let Some(cadmpeg_ir::geometry::SolvedSurfaceGeometry::Cylinder(cylinder)) =
        surface.geometry.solved()
    else {
        return false;
    };
    let axis = cylinder.frame().axis().as_raw();
    let alignment = axis.x * normal[0] + axis.y * normal[1] + axis.z * normal[2];
    if !((alignment.abs() - 1.0).abs() <= EPS_AXIS_ALONG_NORMAL
        && (cylinder.radius().get() - radius).abs() <= LINEAR_RESOLUTION_MM)
    {
        return false;
    }
    let origin = cylinder.origin().get();
    let offset = [at[0] - origin.x, at[1] - origin.y, at[2] - origin.z];
    let along = offset[0] * axis.x + offset[1] * axis.y + offset[2] * axis.z;
    let off_axis = [
        offset[0] - along * axis.x,
        offset[1] - along * axis.y,
        offset[2] - along * axis.z,
    ];
    off_axis[0].hypot(off_axis[1]).hypot(off_axis[2]) <= LINEAR_RESOLUTION_MM
}

/// Whether the decoder built `entity` from a relation record: its geometry
/// carrier is the relation instance.
fn built(entity: &cadmpeg_ir::sketches::SketchEntity) -> bool {
    entity
        .geometry_ref
        .as_deref()
        .is_some_and(|carrier| carrier.starts_with(RELATION_INSTANCE_ID_PREFIX))
}

/// The center and radius of a circle the decoder built from a dimension
/// record.
fn dimension_circle(entity: &cadmpeg_ir::sketches::SketchEntity) -> Option<([f64; 2], f64)> {
    match entity.geometry.definition() {
        Geometry::Circle { center, radius } if built(entity) => {
            Some(([center.u, center.v], radius.get()))
        }
        _ => None,
    }
}

/// The center and radius of a circle or an arc.
fn carrier(geometry: &cadmpeg_ir::sketches::SketchGeometry) -> Option<([f64; 2], f64)> {
    match geometry.definition() {
        Geometry::Circle { center, radius } | Geometry::Arc { center, radius, .. } => {
            Some(([center.u, center.v], radius.get()))
        }
        _ => None,
    }
}

/// The start and end of a line or an arc.
fn curve_ends(geometry: &cadmpeg_ir::sketches::SketchGeometry) -> Option<[[f64; 2]; 2]> {
    use cadmpeg_ir::math::portable::Portable;
    match geometry.definition() {
        Geometry::Line { start, end } => Some([[start.u, start.v], [end.u, end.v]]),
        Geometry::Arc {
            center,
            radius,
            start_angle,
            end_angle,
        } => {
            let at = |angle: f64| {
                let (sin, cos) = angle.portable_sin_cos();
                [center.u + radius.get() * cos, center.v + radius.get() * sin]
            };
            Some([at(start_angle.get()), at(end_angle.get())])
        }
        _ => None,
    }
}

fn coincident(a: [f64; 2], b: [f64; 2]) -> bool {
    let scale = [a[0], a[1], b[0], b[1]]
        .iter()
        .fold(1.0_f64, |scale, value| scale.max(value.abs()));
    (a[0] - b[0]).abs() <= EPS_SKETCH_INCIDENCE * scale
        && (a[1] - b[1]).abs() <= EPS_SKETCH_INCIDENCE * scale
}

/// The curves of `entities` read from a curve record of state `0`, with
/// their construction bit clear, where the sketch's other non-construction
/// curves close only through them (`profile_check::needed`). A built circle
/// the design leaves out or states construction (`reading`) is none of the
/// other curves.
fn needed_state_zero<'a>(
    entities: &[&'a cadmpeg_ir::sketches::SketchEntity],
    forms: &'a [Option<PlanarCurve>],
    readings: &[Option<Reading>],
    outside_profile: &HashSet<SketchEntityId>,
) -> HashSet<&'a SketchEntityId> {
    let mut live = HashMap::new();
    let mut state_zero = HashMap::new();
    for ((entity, form), reading) in entities.iter().zip(forms).zip(readings) {
        let Some(form) = form
            .as_ref()
            .filter(|form| !matches!(form, PlanarCurve::Point(_)))
        else {
            continue;
        };
        if entity.construction || reading.is_some() {
            continue;
        }
        if outside_profile.contains(entity.id()) {
            state_zero.insert(entity.id(), form);
        } else {
            live.insert(entity.id(), form);
        }
    }
    profile_check::needed(&live, &state_zero)
}

/// The v11 curve of a decoded sketch entity, or `None` for a geometry the
/// planar curve vocabulary does not hold as decoded.
fn curve(geometry: &cadmpeg_ir::sketches::SketchGeometry) -> Option<PlanarCurve> {
    use cadmpeg_ir::math::portable::Portable;
    Some(match geometry.definition() {
        Geometry::Point { position } => PlanarCurve::Point(*position),
        Geometry::Line { start, end } => PlanarCurve::Line {
            start: *start,
            end: *end,
        },
        Geometry::ReferenceLine { origin, direction } => {
            let [u, v] = [direction.get().u, direction.get().v];
            let length = u.portable_hypot(v);
            PlanarCurve::InfiniteLine {
                through: *origin,
                direction: UnitVector2::new([u / length, v / length])?,
            }
        }
        Geometry::Circle { center, radius } => PlanarCurve::Circle {
            center: *center,
            radius: *radius,
        },
        Geometry::Arc {
            center,
            radius,
            start_angle,
            end_angle,
        } => PlanarCurve::Arc {
            center: *center,
            radius: *radius,
            start: *start_angle,
            end: *end_angle,
        },
        Geometry::Ellipse {
            center,
            major_angle,
            radii,
            bounds,
        } => {
            let (sin, cos) = major_angle.get().portable_sin_cos();
            PlanarCurve::Ellipse {
                center: *center,
                major: UnitVector2::new([cos, sin])?,
                radii: [radii.major(), radii.minor()],
                range: *bounds,
            }
        }
        Geometry::Nurbs { curve } => PlanarCurve::Nurbs(curve.clone()),
        _ => return None,
    })
}

/// A parameter a dimension binds, its quantity, and the parameter minted
/// for it where no tree node lists it.
type Binding = (ParameterId, Quantity, Option<DesignParameter>);

/// The relations of one sketch.
struct Relations<'a> {
    context: &'a Context<'a>,
    owner: &'a FeatureId,
    roles: &'a Roles,
    entities: &'a [&'a cadmpeg_ir::sketches::SketchEntity],
}

impl Relations<'_> {
    /// The relation of `constraint`, and the parameter its value binds. A
    /// dimension that binds no parameter has no relation.
    fn relation(
        &self,
        constraint: &cadmpeg_ir::sketches::SketchConstraint,
        minted: &[DesignParameter],
    ) -> Result<(SketchRelation<Planar>, Option<Binding>), &'static str> {
        const UNBOUND: &str = "a sketch dimension binds no parameter";
        let kind = constraint.definition.kind();
        if let Some(relation) = self.geometric(kind) {
            return Ok((relation, None));
        }
        if !is_dimension(kind) {
            return Ok((native(kind, operands(kind), Vec::new()), None));
        }
        let (parameter, quantity) = dimension_parameter(kind).ok_or(UNBOUND)?;
        let binding = self
            .binding(parameter, quantity, constraint.driving, minted)
            .ok_or(UNBOUND)?;
        let relation = self
            .dimension(kind, binding.0.clone())
            .unwrap_or_else(|| native(kind, operands(kind), vec![scalar(quantity, &binding.0)]));
        Ok((relation, Some(binding)))
    }

    /// The parameter `parameter` as a dimension of `quantity` binds it: a
    /// parameter of the tree or one already `minted`, or a `Driven`
    /// parameter minted for a reference dimension the decoder read, whose
    /// name no other parameter of the sketch's feature has.
    fn binding(
        &self,
        parameter: &ParameterId,
        quantity: Quantity,
        driving: Option<bool>,
        minted: &[DesignParameter],
    ) -> Option<Binding> {
        if self.context.entries.contains(parameter)
            || minted.iter().any(|known| &known.id == parameter)
        {
            return Some((parameter.clone(), quantity, None));
        }
        if driving != Some(false) {
            return None;
        }
        let source = self
            .context
            .model
            .parameters
            .iter()
            .find(|candidate| &candidate.id == parameter)?;
        if self
            .context
            .entries
            .owned(self.owner, &source.name)
            .is_some()
            || minted.iter().any(|known| {
                known.name.as_ref().map(NonBlankString::as_str) == Some(source.name.as_str())
            })
        {
            return None;
        }
        let value = match (quantity, source.value.as_ref()?) {
            (
                Quantity::LENGTH,
                cadmpeg_ir::features::parameters::ParameterValue::Length(length),
            ) => FiniteReal::new(length.get())?,
            (Quantity::ANGLE, cadmpeg_ir::features::parameters::ParameterValue::Angle(angle)) => {
                FiniteReal::new(angle.get())?
            }
            _ => return None,
        };
        Some((
            parameter.clone(),
            quantity,
            Some(DesignParameter {
                id: parameter.clone(),
                scope: self.context.scope.clone(),
                owner: Some(ParameterOwner::Feature(self.owner.clone())),
                name: NonBlankString::new(source.name.clone()),
                kind: ValueKind::Quantity(quantity),
                unit: None,
                definition: ParameterDefinition::Driven,
                value: Some(ParameterValue::Number(value)),
                display: None,
                tolerance: None,
                decimals: None,
                comment: None,
                native_options: NativeOptions::new(),
                source_settings: NativeOptions::new(),
                native_ref: source.native_ref.clone(),
            }),
        ))
    }

    /// The point entity or curve a locus names.
    fn locus(&self, locus: &SketchLocus) -> Option<Operand<Planar>> {
        Some(Operand::Entity(match locus {
            SketchLocus::Entity(id) => id.clone(),
            SketchLocus::Start(curve) => self.roles.end(curve, true)?.clone(),
            SketchLocus::End(curve) => self.roles.end(curve, false)?.clone(),
            SketchLocus::Center(_) => return None,
        }))
    }

    /// The position of a point locus.
    fn position(&self, locus: &SketchLocus) -> Option<[f64; 2]> {
        let Some(Operand::Entity(id)) = self.locus(locus) else {
            return None;
        };
        let entity = self.entities.iter().find(|entity| entity.id() == &id)?;
        match entity.geometry.definition() {
            Geometry::Point { position } => Some([position.u, position.v]),
            _ => None,
        }
    }

    fn geometric(&self, kind: &Kind) -> Option<SketchRelation<Planar>> {
        let entity = |id: &SketchEntityId| Operand::Entity(id.clone());
        Some(match kind {
            Kind::Coincident { entities } if entities.len() == 2 => SketchRelation::Coincident {
                a: entity(&entities[0]),
                b: entity(&entities[1]),
            },
            Kind::CoincidentLoci { loci } if loci.len() == 2 => SketchRelation::Coincident {
                a: self.locus(&loci[0])?,
                b: self.locus(&loci[1])?,
            },
            Kind::PointOnObject { point, entity: id } => SketchRelation::Coincident {
                a: self.locus(point)?,
                b: entity(id),
            },
            Kind::Midpoint { point, entity: id } => match self.locus(point)? {
                Operand::Entity(point) => SketchRelation::Midpoint {
                    point,
                    of: entity(id),
                },
                _ => return None,
            },
            Kind::Concentric { first, second } => SketchRelation::Concentric {
                a: entity(first),
                b: entity(second),
            },
            Kind::Collinear { first, second } => SketchRelation::Collinear {
                a: entity(first),
                b: entity(second),
            },
            Kind::Parallel { first, second } => SketchRelation::Parallel {
                a: entity(first),
                b: entity(second),
            },
            Kind::Perpendicular { first, second } => SketchRelation::Perpendicular {
                a: entity(first),
                b: entity(second),
            },
            Kind::Tangent { first, second } => SketchRelation::Tangent {
                a: entity(first),
                b: entity(second),
                contact: None,
            },
            Kind::Horizontal { entity: id } => SketchRelation::Horizontal {
                a: entity(id),
                b: None,
            },
            Kind::Vertical { entity: id } => SketchRelation::Vertical {
                a: entity(id),
                b: None,
            },
            Kind::Equal { first, second } => SketchRelation::Equal {
                members: vec![entity(first), entity(second)],
            },
            Kind::Fixed { entity: id } => SketchRelation::Fixed { of: entity(id) },
            Kind::Symmetric {
                first,
                second,
                axis,
            } => SketchRelation::Symmetric {
                a: self.locus(first)?,
                b: self.locus(second)?,
                about: entity(axis),
            },
            _ => return None,
        })
    }

    /// The dimension relation of `kind`, bound to `value`. A horizontal or
    /// vertical distance orders its points so the stated value is the
    /// non-negative difference (§6.1).
    fn dimension(&self, kind: &Kind, value: ParameterId) -> Option<SketchRelation<Planar>> {
        let along = |first: &SketchLocus, second: &SketchLocus, axis: usize, along| {
            let (a, b) = (self.position(first)?, self.position(second)?);
            let (a_operand, b_operand) = (self.locus(first)?, self.locus(second)?);
            let (a_operand, b_operand) = if b[axis] >= a[axis] {
                (a_operand, b_operand)
            } else {
                (b_operand, a_operand)
            };
            Some(SketchRelation::Distance {
                a: a_operand,
                b: b_operand,
                along: Some(along),
                doubled: None,
                value: Dimension::Parameter(value.clone()),
            })
        };
        Some(match kind {
            Kind::DistanceLoci { first, second, .. } => SketchRelation::Distance {
                a: self.locus(first)?,
                b: self.locus(second)?,
                along: Some(DistanceAxis::Aligned),
                doubled: None,
                value: Dimension::Parameter(value),
            },
            Kind::Distance { entities, .. } if entities.len() == 2 => SketchRelation::Distance {
                a: Operand::Entity(entities[0].clone()),
                b: Operand::Entity(entities[1].clone()),
                along: Some(DistanceAxis::Aligned),
                doubled: None,
                value: Dimension::Parameter(value),
            },
            Kind::HorizontalDistance { first, second, .. } => {
                along(first, second, 0, DistanceAxis::Horizontal)?
            }
            Kind::VerticalDistance { first, second, .. } => {
                along(first, second, 1, DistanceAxis::Vertical)?
            }
            Kind::Radius { entity, .. } => SketchRelation::Radial {
                of: entity.clone(),
                measure: RadialMeasure::Radius,
                value: Dimension::Parameter(value),
            },
            Kind::Diameter { entity, .. } => SketchRelation::Radial {
                of: entity.clone(),
                measure: RadialMeasure::Diameter,
                value: Dimension::Parameter(value),
            },
            Kind::Angle { first, second, .. } => SketchRelation::Angle {
                between: AngleOperands::Between([
                    Operand::Entity(first.clone()),
                    Operand::Entity(second.clone()),
                ]),
                sector: None,
                value: Dimension::Parameter(value),
            },
            _ => return None,
        })
    }
}

/// The entities a relation the vocabulary does not type names.
fn operands(kind: &Kind) -> Vec<Operand<Planar>> {
    let entities = match kind {
        Kind::Native { entities, .. }
        | Kind::Coincident { entities }
        | Kind::Distance { entities, .. } => entities.clone(),
        Kind::CoincidentLoci { loci } => loci.iter().map(locus_entity).cloned().collect(),
        Kind::PointOnObject { point, entity } | Kind::Midpoint { point, entity } => {
            vec![locus_entity(point).clone(), entity.clone()]
        }
        Kind::DistanceLoci { first, second, .. }
        | Kind::HorizontalDistance { first, second, .. }
        | Kind::VerticalDistance { first, second, .. } => {
            vec![locus_entity(first).clone(), locus_entity(second).clone()]
        }
        Kind::Radius { entity, .. } | Kind::Diameter { entity, .. } => vec![entity.clone()],
        Kind::Angle { first, second, .. } => vec![first.clone(), second.clone()],
        _ => Vec::new(),
    };
    let mut operands = Vec::with_capacity(entities.len());
    for entity in entities {
        if !operands.contains(&entity) {
            operands.push(entity);
        }
    }
    operands.into_iter().map(Operand::Entity).collect()
}

fn locus_entity(locus: &SketchLocus) -> &SketchEntityId {
    match locus {
        SketchLocus::Entity(id)
        | SketchLocus::Start(id)
        | SketchLocus::End(id)
        | SketchLocus::Center(id) => id,
    }
}

/// The native relation families of a dimension whose quantity their class
/// states: a distance or a circle's size is a length, an angle dimension an
/// angle (`docs/formats/sldprt.md`).
const NATIVE_LENGTH_DIMENSIONS: [&str; 6] = [
    "sgPntPntDist",
    "sgPntLineDist",
    "sgLLDist",
    "sgPntPntHorDist",
    "sgPntPntVertDist",
    "sgCircleDim",
];
const NATIVE_ANGLE_DIMENSIONS: [&str; 1] = ["sgAnglDim"];

/// Whether `kind` is a dimension: a relation that states a value.
fn is_dimension(kind: &Kind) -> bool {
    match kind {
        Kind::Native { native_kind, .. } => {
            NATIVE_LENGTH_DIMENSIONS.contains(&native_kind.as_str())
                || NATIVE_ANGLE_DIMENSIONS.contains(&native_kind.as_str())
        }
        Kind::PointCoordinateValues { .. }
        | Kind::MidpointCoordinate { .. }
        | Kind::Offset { .. }
        | Kind::ArcAngle { .. }
        | Kind::EllipseAngle { .. }
        | Kind::Distance { .. }
        | Kind::DistanceLoci { .. }
        | Kind::DistanceLociValue { .. }
        | Kind::PolarDistance { .. }
        | Kind::AngleDifference { .. }
        | Kind::HorizontalDistance { .. }
        | Kind::VerticalDistance { .. }
        | Kind::RepeatedDistance { .. }
        | Kind::RepeatedLength { .. }
        | Kind::ParallelLineSetDistance { .. }
        | Kind::Angle { .. }
        | Kind::AngleToAxis { .. }
        | Kind::Radius { .. }
        | Kind::RepeatedRadius { .. }
        | Kind::Diameter { .. }
        | Kind::RepeatedDiameter { .. }
        | Kind::SnellsLaw { .. }
        | Kind::Weight { .. } => true,
        Kind::Disabled {}
        | Kind::Coincident { .. }
        | Kind::Polygon { .. }
        | Kind::SplineGroup { .. }
        | Kind::RectangularPattern { .. }
        | Kind::CircularPattern { .. }
        | Kind::TextFrame { .. }
        | Kind::TextPath { .. }
        | Kind::CoincidentLoci { .. }
        | Kind::SameCoordinate { .. }
        | Kind::PointOnObject { .. }
        | Kind::Midpoint { .. }
        | Kind::ProjectedCopy { .. }
        | Kind::AtIntersection { .. }
        | Kind::Concentric { .. }
        | Kind::Coradial { .. }
        | Kind::Collinear { .. }
        | Kind::Symmetric { .. }
        | Kind::PointSymmetric { .. }
        | Kind::Horizontal { .. }
        | Kind::Vertical { .. }
        | Kind::Parallel { .. }
        | Kind::Perpendicular { .. }
        | Kind::Tangent { .. }
        | Kind::TangentLoci { .. }
        | Kind::Curvature { .. }
        | Kind::Equal { .. }
        | Kind::Fixed { .. }
        | Kind::ScalarEquality { .. }
        | Kind::EqualDistance { .. }
        | Kind::InternalAlignment { .. }
        | Kind::Group { .. }
        | Kind::Text { .. } => false,
    }
}

/// The parameter a dimension relation binds and the quantity it measures.
fn dimension_parameter(kind: &Kind) -> Option<(&ParameterId, Quantity)> {
    Some(match kind {
        Kind::Distance { parameter, .. }
        | Kind::DistanceLoci { parameter, .. }
        | Kind::HorizontalDistance { parameter, .. }
        | Kind::VerticalDistance { parameter, .. }
        | Kind::Radius { parameter, .. }
        | Kind::Diameter { parameter, .. } => (parameter, Quantity::LENGTH),
        Kind::Angle { parameter, .. } => (parameter, Quantity::ANGLE),
        Kind::Native {
            native_kind,
            parameter: Some(parameter),
            ..
        } => {
            if NATIVE_LENGTH_DIMENSIONS.contains(&native_kind.as_str()) {
                (parameter, Quantity::LENGTH)
            } else if NATIVE_ANGLE_DIMENSIONS.contains(&native_kind.as_str()) {
                (parameter, Quantity::ANGLE)
            } else {
                return None;
            }
        }
        _ => return None,
    })
}

fn scalar(quantity: Quantity, parameter: &ParameterId) -> ScalarDimension {
    if quantity == Quantity::ANGLE {
        ScalarDimension::Angle(Dimension::Parameter(parameter.clone()))
    } else {
        ScalarDimension::Length(Dimension::Parameter(parameter.clone()))
    }
}

/// A relation whose meaning the decoder did not type, named by its decoded
/// form: `sldprt:<native class>` for a native relation, else
/// `sldprt:<decoded kind>`.
fn native(
    kind: &Kind,
    operands: Vec<Operand<Planar>>,
    values: Vec<ScalarDimension>,
) -> SketchRelation<Planar> {
    let name = match kind {
        Kind::Native { native_kind, .. } if native_kind.as_str().starts_with("sldprt:") => {
            native_kind.as_str().to_owned()
        }
        Kind::Native { native_kind, .. } => format!("sldprt:{}", native_kind.as_str()),
        _ => format!("sldprt:{}", kind_name(kind)),
    };
    SketchRelation::Native {
        kind: NonBlankString::new(name)
            .unwrap_or_else(|| cadmpeg_core::nonblank_literal!("sldprt:sketch_relation")),
        operands,
        values,
    }
}

/// The serialized tag of a decoded relation kind.
fn kind_name(kind: &Kind) -> String {
    cadmpeg_core::serde_erase::to_json_value(kind)
        .ok()
        .and_then(|value| value.get("kind")?.as_str().map(str::to_owned))
        .unwrap_or_else(|| "sketch_relation".to_owned())
}
