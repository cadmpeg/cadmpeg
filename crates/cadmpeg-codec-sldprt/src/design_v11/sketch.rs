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
//! dimension makes the tree parameter it binds `Driven`. A driving dimension
//! that binds no parameter states the value its parameter record stores, or
//! no value, and an entity that has no planar curve states its native kind.
//!
//! A relation on the start, end, or center of a curve where no point record
//! lies names a point entity minted there, with that role on the curve, and
//! so does a relation on a center where two point records lie and the curve
//! names no center record; a loss names each such center. A native
//! relation states each operand its record holds.
//! A reference dimension whose parameter is no parameter of the design binds
//! a `Driven` parameter minted for it, with the value its parameter record
//! stores where the record stores one of its quantity.
//!
//! A block definition is a sketch with no plane, the operation of its
//! feature. A sketch that no feature of the timeline restates is the
//! operation of a member of the scope's root `Group` (§5.2): the v10 feature
//! whose operation names it, else one minted for it.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};

use cadmpeg_core::decode::ResourceLimit;
use cadmpeg_core::text::NonBlankString;
use cadmpeg_ir::design::construction::{
    kinds, Construction, ConstructionInput, FrameValue, InputReference, InputRole, Relation,
    Subject,
};
use cadmpeg_ir::design::dimension::Dimension;
use cadmpeg_ir::design::evaluation::EvaluationStatus;
use cadmpeg_ir::design::feature::{
    Feature, FeatureDefinition as DesignDefinition, FeatureOperation, OperationFamily,
};
use cadmpeg_ir::design::ops::sketch::{
    EntityDefinition, EntityForm, Operand, Planar, PlanarCurve, RadialMeasure, Sketch,
    SketchConstraint, SketchEntity, SketchPlane, SketchRelation, SketchSolve,
};
use cadmpeg_ir::design::options::NativeOptions;
use cadmpeg_ir::design::parameter::{DesignParameter, DimensionDisplay};
use cadmpeg_ir::design::projection::sketch::{
    self as relation, driven_parameter, native_entity_kind, native_relation, planar_curve,
    stored_frame, stored_quantity, stored_value, DimensionValue, Loci, LocusPoints, StoredPoints,
};
use cadmpeg_ir::design::projection::spatial_sketch;
use cadmpeg_ir::design::selection::{
    Anchor, Members, OrderedSelection, PickTarget, Picked, Picks, PrincipalEntity, Selection,
};
use cadmpeg_ir::design::units::Quantity;
use cadmpeg_ir::features::{
    FeatureDefinition, FeatureId, FeatureOperation as LegacyOperation, ParameterId,
};
use cadmpeg_ir::ids::ProductDefinitionId;
use cadmpeg_ir::sketches::{
    SketchConstraintDefinitionInput as Kind, SketchConstraintId, SketchEntityId,
    SketchGeometryDefinition as Geometry, SketchLocus, SketchNativeOperand, SpatialSketchId,
};

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
    /// The features, members of no timeline, that own the sketches no
    /// timeline feature restates.
    pub(super) outside: Vec<Feature>,
    /// The entities with no planar curve, stated by their native kind.
    pub(super) native_entities: Vec<SketchEntityId>,
    /// The dimensions that bind no parameter of the design.
    pub(super) unbound: Vec<SketchConstraintId>,
    /// The relations on a curve's start, end, or center that state the curve
    /// itself, where no position of that point is stored.
    pub(super) unplaced: Vec<SketchConstraintId>,
    /// The points of curves where two or more point entities lie that the
    /// curve records name alike, each named with its curve and those point
    /// entities.
    pub(super) ambiguous: Vec<String>,
    /// The relations that name one point entity for both operands of a
    /// coincidence, which the point states.
    pub(super) single_point: Vec<SketchConstraintId>,
    /// The relations stated by their native kind.
    pub(super) native_relations: Vec<SketchConstraintId>,
    /// The spatial sketches, each the operation of its owner.
    pub(super) spatial: Vec<spatial_sketch::Restated>,
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
    pub(super) ctx: &'a cadmpeg_core::decode::DecodeContext<'a>,
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
pub(super) fn project(
    features: &mut [Feature],
    context: &Context<'_>,
) -> Result<Sketches, ResourceLimit> {
    let mut out = Sketches::default();
    let mut preceding = HashSet::new();
    for feature in features.iter_mut() {
        let sketch_feature = match &feature.definition.operation {
            FeatureOperation::Unresolved {
                family: OperationFamily::Sketch,
            } => true,
            FeatureOperation::Native { .. } => source(context.model, &feature.id)
                .and_then(owned_sketch)
                .is_some_and(|(_, block)| block),
            _ => false,
        };
        if matches!(
            feature.definition.operation,
            FeatureOperation::Unresolved {
                family: OperationFamily::SpatialSketch
            }
        ) {
            if let Some(record) = source(context.model, &feature.id)
                .and_then(owned_spatial_sketch)
                .and_then(|id| spatial_record(context.model, id))
            {
                spatial(&feature.id, record, context, &Anchor::Input, &mut out);
                feature.definition.operation = FeatureOperation::SpatialSketch(record.id.clone());
            }
        } else if sketch_feature {
            let support = context
                .supports
                .get(&feature.id)
                .filter(|(datum, _)| preceding.contains(datum));
            match sketch(feature, context.model, support) {
                Ok((record, holder)) => {
                    restate(&feature.id, record, context, holder, &mut out)?;
                    feature.definition.operation = FeatureOperation::Sketch(record.id.clone());
                }
                Err(reason) => out.unprojected.push((feature.id.clone(), reason)),
            }
        }
        preceding.insert(feature.id.clone());
    }
    out.outside = outside(features, context, &mut out)?;
    Ok(out)
}

/// The v10 features outside `features`, the timeline's, that [`outside`]
/// makes members of the root `Group`: of each sketch and spatial sketch
/// record of `model` that no feature of `features` owns, the first feature
/// whose operation names it.
pub(super) fn owners_outside(
    model: &cadmpeg_ir::document::Model,
    features: &[Feature],
) -> HashSet<FeatureId> {
    let timeline = features
        .iter()
        .map(|feature| &feature.id)
        .collect::<HashSet<_>>();
    let mut owners = HashSet::new();
    let mut add = |owning: Vec<&cadmpeg_ir::features::Feature>| {
        if let Some(first) = owning.first() {
            if owning.iter().all(|feature| !timeline.contains(&feature.id)) {
                owners.insert(first.id.clone());
            }
        }
    };
    for record in &model.sketches {
        add(model
            .features
            .iter()
            .filter(|feature| owned_sketch(feature).is_some_and(|(sketch, _)| sketch == &record.id))
            .collect());
    }
    for record in &model.spatial_sketches {
        add(model
            .features
            .iter()
            .filter(|feature| owned_spatial_sketch(feature) == Some(&record.id))
            .collect());
    }
    owners
}

/// The owners of the sketches of `context` that no feature of `features`
/// restates, each sketch restated as its owner's operation. The owner is the
/// v10 feature whose operation names the sketch, where no feature of
/// `features` has its id, else a feature minted for the sketch.
fn outside(
    features: &[Feature],
    context: &Context<'_>,
    out: &mut Sketches,
) -> Result<Vec<Feature>, ResourceLimit> {
    let restated = out
        .records
        .iter()
        .map(|sketch| sketch.id.clone())
        .collect::<HashSet<_>>();
    let mut owners = Vec::new();
    for record in &context.model.sketches {
        if restated.contains(&record.id) {
            continue;
        }
        let (source, block) = context
            .model
            .features
            .iter()
            .find_map(|feature| match owned_sketch(feature) {
                Some((sketch, block)) if sketch == &record.id => Some((Some(feature), block)),
                _ => None,
            })
            .unwrap_or((None, false));
        let id = source
            .map(|feature| feature.id.clone())
            .filter(|id| features.iter().all(|feature| &feature.id != id))
            .unwrap_or_else(|| {
                minted_owner(
                    &cadmpeg_ir::identity_namespace!("sldprt", "design", "sketch-owner"),
                    record.id.as_str(),
                )
            });
        let holder = if block {
            Holder::Block
        } else {
            Holder::Unordered
        };
        restate(&id, record, context, holder, out)?;
        owners.push(Feature {
            name: source
                .and_then(|feature| feature.name.clone())
                .or_else(|| record.name.clone()),
            suppressed: source
                .and_then(|feature| feature.suppressed)
                .map(Dimension::Literal),
            definition: DesignDefinition {
                operation: FeatureOperation::Sketch(record.id.clone()),
                assembly: None,
                refine: None,
                fuzzy_tolerance: None,
            },
            placement: None,
            component: None,
            driven_by: None,
            native_ref: source.and_then(|feature| feature.native_ref.clone()),
            id,
        });
    }
    let restated = out
        .spatial
        .iter()
        .map(|restated| restated.sketch.id.clone())
        .collect::<HashSet<_>>();
    for record in &context.model.spatial_sketches {
        if restated.contains(&record.id) {
            continue;
        }
        let source = context
            .model
            .features
            .iter()
            .find(|feature| owned_spatial_sketch(feature) == Some(&record.id));
        let id = source
            .map(|feature| feature.id.clone())
            .filter(|id| features.iter().all(|feature| &feature.id != id))
            .unwrap_or_else(|| {
                minted_owner(
                    &cadmpeg_ir::identity_namespace!("sldprt", "design", "spatial-sketch-owner"),
                    record.id.as_str(),
                )
            });
        spatial(&id, record, context, &Anchor::Current, out);
        owners.push(Feature {
            name: source
                .and_then(|feature| feature.name.clone())
                .or_else(|| record.name.clone()),
            suppressed: source
                .and_then(|feature| feature.suppressed)
                .map(Dimension::Literal),
            definition: DesignDefinition {
                operation: FeatureOperation::SpatialSketch(record.id.clone()),
                assembly: None,
                refine: None,
                fuzzy_tolerance: None,
            },
            placement: None,
            component: None,
            driven_by: None,
            native_ref: source.and_then(|feature| feature.native_ref.clone()),
            id,
        });
    }
    Ok(owners)
}

/// Restate the spatial sketch `record` as the sketch of `owner`, a selection
/// of its records anchored at `at`. Its dimensions bind the parameters of
/// the tree.
fn spatial(
    owner: &FeatureId,
    record: &cadmpeg_ir::sketches::SpatialSketch,
    context: &Context<'_>,
    at: &Anchor,
    out: &mut Sketches,
) {
    let bound = RefCell::new(Vec::new());
    let restated = spatial_sketch::restate(
        &cadmpeg_core::nonblank_literal!("sldprt"),
        owner,
        record,
        context.model,
        at,
        |parameter, quantity| {
            let known = context.entries.contains(parameter);
            if known {
                bound.borrow_mut().push((parameter.clone(), quantity));
            }
            known
        },
    );
    out.bound.extend(bound.into_inner());
    out.spatial.push(restated);
}

/// The spatial sketch that the v10 feature `feature` owns.
fn owned_spatial_sketch(feature: &cadmpeg_ir::features::Feature) -> Option<&SpatialSketchId> {
    match feature.evaluation.definition() {
        FeatureDefinition::Operation(LegacyOperation::SpatialSketch { sketch }) => sketch.as_ref(),
        _ => None,
    }
}

/// The spatial sketch record `id` of `model`.
fn spatial_record<'a>(
    model: &'a cadmpeg_ir::document::Model,
    id: &SpatialSketchId,
) -> Option<&'a cadmpeg_ir::sketches::SpatialSketch> {
    model
        .spatial_sketches
        .iter()
        .find(|record| &record.id == id)
}

/// The id of the feature minted in `namespace` to own the sketch `sketch`.
fn minted_owner(namespace: &cadmpeg_ir::ids::IdentityNamespace, sketch: &str) -> FeatureId {
    FeatureId::compose(
        namespace,
        cadmpeg_ir::ids::IdentityKey::from(sketch.len())
            .then(cadmpeg_ir::identity_key!(":"))
            .with_tail(&cadmpeg_ir::ids::IdentityKeyTail::percent_encode(sketch)),
    )
}

/// The sketch record of the sketch feature `feature` of `model`, and its
/// holder, the feature that `support` places.
fn sketch<'a>(
    feature: &Feature,
    model: &'a cadmpeg_ir::document::Model,
    support: Option<&'a (FeatureId, PrincipalEntity)>,
) -> Result<(&'a cadmpeg_ir::sketches::Sketch, Holder<'a>), &'static str> {
    let source = source(model, &feature.id).ok_or("the sketch feature has no decoded record")?;
    let (id, block) = match source.evaluation.definition() {
        FeatureDefinition::Operation(LegacyOperation::Sketch { sketch }) => (
            sketch
                .id()
                .ok_or("the sketch feature names no planar sketch")?,
            false,
        ),
        FeatureDefinition::Operation(LegacyOperation::SketchBlockDefinition {
            sketch: Some(sketch),
        }) => (sketch, true),
        _ => return Err("the sketch feature has no decoded sketch"),
    };
    let record = model
        .sketches
        .iter()
        .find(|candidate| &candidate.id == id)
        .ok_or("the sketch record is missing")?;
    let holder = if block {
        Holder::Block
    } else {
        Holder::Ordered(support)
    };
    Ok((record, holder))
}

/// The v10 feature `id` of `model`.
fn source<'a>(
    model: &'a cadmpeg_ir::document::Model,
    id: &FeatureId,
) -> Option<&'a cadmpeg_ir::features::Feature> {
    model.features.iter().find(|candidate| &candidate.id == id)
}

/// The planar sketch that the v10 feature `feature` owns, and whether it is
/// a block definition.
fn owned_sketch(feature: &cadmpeg_ir::features::Feature) -> Option<(&SketchId, bool)> {
    match feature.evaluation.definition() {
        FeatureDefinition::Operation(LegacyOperation::Sketch { sketch }) => {
            Some((sketch.id()?, false))
        }
        FeatureDefinition::Operation(LegacyOperation::SketchBlockDefinition {
            sketch: Some(sketch),
        }) => Some((sketch, true)),
        _ => None,
    }
}

/// The container member that owns a sketch: a feature of the timeline, with
/// the principal plane its sketch can lie on, a member of an unordered
/// container (§5.8, Anchors), or the feature of a block definition, which
/// has no plane.
#[derive(Clone, Copy)]
enum Holder<'a> {
    Ordered(Option<&'a (FeatureId, PrincipalEntity)>),
    Unordered,
    Block,
}

/// Restate `record` as the sketch of the feature `owner`, which `holder`
/// places.
fn restate(
    owner: &FeatureId,
    record: &cadmpeg_ir::sketches::Sketch,
    context: &Context<'_>,
    holder: Holder<'_>,
    out: &mut Sketches,
) -> Result<(), ResourceLimit> {
    let model = context.model;
    let entities = model
        .sketch_entities
        .iter()
        .filter(|entity| entity.sketch == record.id)
        .collect::<Vec<_>>();
    let mut sketch_entities = Vec::with_capacity(entities.len());
    let mut outside_profile = Vec::new();
    let forms = entities
        .iter()
        .map(|entity| planar_curve(&entity.geometry))
        .collect::<Vec<_>>();
    let mut readings = Vec::with_capacity(entities.len());
    for index in 0..entities.len() {
        let reading = reading(index, &entities, &readings, record, context.surfaces);
        readings.push(reading);
    }
    let needed = needed_state_zero(&entities, &forms, &readings, context.outside_profile);
    let stored = entities
        .iter()
        .zip(&readings)
        .filter(|(_, reading)| reading.is_none())
        .map(|(entity, _)| *entity)
        .collect::<Vec<_>>();
    let points_of = StoredPoints::new(context.ctx, &stored, standing, coincident)?;
    out.ambiguous
        .extend(points_of.ambiguous().map(|(curve, point, candidates)| {
            format!(
                "the {point} of {curve} ({})",
                candidates
                    .iter()
                    .map(SketchEntityId::as_str)
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        }));
    for ((entity, form), reading) in entities
        .iter()
        .zip(forms.iter().cloned())
        .zip(readings.iter())
    {
        let definition = match form {
            Some(form) => EntityDefinition::Free(form),
            None => {
                out.native_entities.push(entity.id().clone());
                EntityDefinition::Defined {
                    form: Box::new(EntityForm::Native(native_entity_kind(
                        &cadmpeg_core::nonblank_literal!("sldprt"),
                        &entity.geometry,
                    ))),
                    solved: None,
                }
            }
        };
        let outside =
            context.outside_profile.contains(entity.id()) && !needed.contains(entity.id());
        if outside && !entity.construction {
            outside_profile.push(entity.id().clone());
        }
        sketch_entities.push(SketchEntity {
            id: entity.id().clone(),
            sketch: record.id.clone(),
            definition,
            roles: points_of.roles(entity.id()),
            construction: (entity.construction || outside || reading.is_some()).then_some(true),
            centerline: None,
            native: None,
            native_options: NativeOptions::new(),
            source_settings: NativeOptions::new(),
            native_ref: entity.native_ref.clone(),
        });
    }
    let points = LocusPoints::new(
        context.ctx,
        model
            .sketch_constraints
            .iter()
            .filter(|constraint| constraint.sketch == record.id)
            .map(|constraint| constraint.definition.kind()),
        &stored,
        &points_of,
    )?;
    sketch_entities.extend(points.entities(&record.id));
    let mut constraints = Vec::new();
    let mut driven = Vec::new();
    let mut references = Vec::new();
    let mut bound = Vec::new();
    let at = match holder {
        Holder::Ordered(_) => Anchor::Input,
        Holder::Unordered | Holder::Block => Anchor::Current,
    };
    for constraint in model
        .sketch_constraints
        .iter()
        .filter(|constraint| constraint.sketch == record.id)
    {
        let relations = Relations {
            context,
            owner,
            points: &points,
            entities: &entities,
            at: &at,
        };
        let (relation, binding, unbound) = relations.relation(constraint, &driven);
        if matches!(
            &relation,
            SketchRelation::Coincident {
                a: Operand::Entity(a),
                b: Operand::Entity(b),
            } if a == b
        ) {
            out.single_point.push(constraint.id.clone());
            continue;
        }
        if unbound {
            out.unbound.push(constraint.id.clone());
        }
        if points.unplaced(constraint.definition.kind()) {
            out.unplaced.push(constraint.id.clone());
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
            native_options: relation::native_options(
                &cadmpeg_core::nonblank_literal!("sldprt"),
                constraint.definition.kind(),
            ),
            source_settings: NativeOptions::new(),
            native_ref: constraint.native_ref.clone(),
        });
    }
    out.bound.extend(bound);
    out.driven.extend(driven);
    out.references.extend(references);
    let dimension_circles = settle(&entities, readings, &mut constraints);
    sketch_entities.retain(|kept| {
        dimension_circles
            .iter()
            .all(|(_, circle)| !circle.outcome.left_out() || circle.entity != kept.id)
    });
    out.records.push(Sketch {
        id: record.id.clone(),
        owner: owner.clone(),
        name: record.name.clone(),
        plane: match holder {
            Holder::Block => SketchPlane::Block,
            Holder::Ordered(_) | Holder::Unordered => {
                SketchPlane::Placed(Box::new(placement(record.placement, holder)))
            }
        },
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
    out.native_relations.extend(
        constraints
            .iter()
            .filter(|constraint| matches!(constraint.relation, SketchRelation::Native { .. }))
            .map(|constraint| constraint.id.clone()),
    );
    out.constraints.extend(constraints);
    Ok(())
}

type SketchId = cadmpeg_ir::sketches::SketchId;

/// The plane of a sketch whose stored placement is `placement`, on the
/// support of `holder` where the stored frame lies in that plane.
fn placement(
    placement: cadmpeg_ir::sketches::SketchPlacement,
    holder: Holder<'_>,
) -> Construction<kinds::Frame> {
    let resolved = stored_frame(placement);
    let (support, anchor) = match holder {
        Holder::Ordered(support) => (support, Anchor::Input),
        Holder::Unordered | Holder::Block => (None, Anchor::Current),
    };
    let on_support = support
        .filter(|(_, entity)| {
            resolved
                .as_ref()
                .is_some_and(|frame| in_principal_plane(frame, *entity))
        })
        .and_then(|(datum, _)| {
            selection(vec![pick(PickTarget::Datum {
                feature: datum.clone(),
                part: None,
            })])
            .ok()
        });
    let input = match on_support {
        Some(selection) => ConstructionInput {
            role: Some(InputRole {
                subject: Subject::Whole,
                relation: Relation::Coincident,
            }),
            reference: Some(InputReference::Picks(selection)),
            value: None,
            reversed: None,
        },
        None => ConstructionInput {
            role: None,
            reference: Some(InputReference::Picks(Selection {
                at: anchor,
                picks: Picks::Unread,
                native: None,
            })),
            value: None,
            reversed: None,
        },
    };
    Construction {
        inputs: vec![input],
        resolved,
        after: Vec::new(),
        solution: None,
        mode: None,
        resolved_in: std::collections::BTreeMap::new(),
    }
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

/// The standing of `point` as the point `locus` names on `curve`: first for
/// the native record the curve names as that end or as its center, and for
/// any point at the center of a curve that names no center record.
fn standing(
    curve: &cadmpeg_ir::sketches::SketchEntity,
    locus: &SketchLocus,
    point: &cadmpeg_ir::sketches::SketchEntity,
) -> Option<u8> {
    let named = match (locus, curve.endpoint_refs.as_slice(), &curve.center_ref) {
        (SketchLocus::Start(_), [first, _], _) => first,
        (SketchLocus::End(_), [_, second], _) => second,
        (SketchLocus::Center(_), _, Some(center)) => center,
        (SketchLocus::Center(_), _, None) => return Some(0),
        _ => return None,
    };
    (point.native_ref.as_deref() == Some(named.as_str())).then_some(0)
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

/// A parameter a dimension binds, its quantity, and the parameter minted
/// for it where no tree node lists it.
type Binding = (ParameterId, Quantity, Option<DesignParameter>);

/// The relations of one sketch, and the anchor of a native operand that
/// names no entity of it.
struct Relations<'a> {
    context: &'a Context<'a>,
    owner: &'a FeatureId,
    points: &'a LocusPoints,
    entities: &'a [&'a cadmpeg_ir::sketches::SketchEntity],
    at: &'a Anchor,
}

impl Relations<'_> {
    /// The relation of `constraint`, the parameter its value binds, and
    /// whether it is a dimension that binds no parameter of the design. Such
    /// a dimension states the value its parameter record stores, where the
    /// record stores one of the dimension's quantity.
    fn relation(
        &self,
        constraint: &cadmpeg_ir::sketches::SketchConstraint,
        minted: &[DesignParameter],
    ) -> (SketchRelation<Planar>, Option<Binding>, bool) {
        let kind = constraint.definition.kind();
        let codec = cadmpeg_core::nonblank_literal!("sldprt");
        if let Some(relation) = relation::geometric(kind, self) {
            return (relation, None, false);
        }
        if !is_dimension(kind) {
            return (
                native_relation(&codec, kind, Vec::new(), self, self.at),
                None,
                false,
            );
        }
        let Some((parameter, quantity)) = dimension_parameter(kind, &self.context.model.parameters)
        else {
            return (
                native_relation(&codec, kind, Vec::new(), self, self.at),
                None,
                true,
            );
        };
        let binding = self.binding(parameter, quantity, constraint.driving, minted);
        let value = match &binding {
            Some((bound, _, _)) => Some(DimensionValue::Parameter(bound.clone())),
            None => stored_value(&self.context.model.parameters, parameter, quantity),
        };
        let relation = value
            .as_ref()
            .and_then(|value| relation::dimension(kind, value, self))
            .unwrap_or_else(|| {
                native_relation(
                    &codec,
                    kind,
                    value
                        .iter()
                        .filter_map(|value| value.scalar(quantity))
                        .collect(),
                    self,
                    self.at,
                )
            });
        let unbound = binding.is_none();
        (relation, binding, unbound)
    }

    /// The parameter `parameter` as a dimension of `quantity` binds it: a
    /// parameter of the tree or one already `minted`, or, for a reference
    /// dimension, a `Driven` parameter minted for it. The minted parameter
    /// has the name of its parameter record where no other parameter of the
    /// sketch's feature has it, and the value the record stores where it is
    /// one of `quantity`.
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
        let model = self.context.model;
        let source = model
            .parameters
            .iter()
            .find(|candidate| &candidate.id == parameter);
        let name = source
            .and_then(|source| NonBlankString::new(source.name.clone()))
            .filter(|name| {
                self.context
                    .entries
                    .owned(self.owner, name.as_str())
                    .is_none()
                    && minted.iter().all(|known| known.name.as_ref() != Some(name))
            });
        Some((
            parameter.clone(),
            quantity,
            Some(driven_parameter(
                &model.parameters,
                parameter,
                quantity,
                self.context.scope,
                self.owner,
                name,
            )),
        ))
    }
}

impl Loci for Relations<'_> {
    /// The point entity or curve a locus names: the point record at a
    /// curve's end or center, else the point minted there.
    fn operand(&self, locus: &SketchLocus) -> Option<Operand<Planar>> {
        match locus {
            SketchLocus::Entity(id) => Some(Operand::Entity(id.clone())),
            _ => self.points.operand(locus),
        }
    }

    /// The position of a point locus.
    fn position(&self, locus: &SketchLocus) -> Option<[f64; 2]> {
        let SketchLocus::Entity(id) = locus else {
            return self.points.position(locus);
        };
        let entity = self.entities.iter().find(|entity| entity.id() == id)?;
        match entity.geometry.definition() {
            Geometry::Point { position } => Some([position.u, position.v]),
            _ => None,
        }
    }

    /// The one entity that holds the marker a resolved operand names.
    fn record(&self, operand: &SketchNativeOperand) -> Option<Operand<Planar>> {
        if operand.native_kind.as_str().ends_with(":unresolved") {
            return None;
        }
        let native = operand.native_ref.as_deref()?;
        let mut named = self
            .entities
            .iter()
            .filter(|entity| entity.native_ref.as_deref() == Some(native));
        match (named.next(), named.next()) {
            (Some(entity), None) => Some(Operand::Entity(entity.id().clone())),
            _ => None,
        }
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
        Kind::Native { native_kind, .. } if native_quantity(native_kind.as_str()).is_some() => true,
        _ => relation::is_dimension(kind),
    }
}

/// The parameter a dimension relation binds and the quantity it measures:
/// the quantity its native class states, else that of the value its
/// parameter record stores.
fn dimension_parameter<'k>(
    kind: &'k Kind,
    parameters: &[cadmpeg_ir::features::parameters::DesignParameter],
) -> Option<(&'k ParameterId, Quantity)> {
    relation::dimension_parameter(kind, |native_kind, parameter| {
        native_quantity(native_kind).or_else(|| stored_quantity(parameters, parameter))
    })
}

/// The quantity a native dimension class states.
fn native_quantity(native_kind: &str) -> Option<Quantity> {
    if NATIVE_LENGTH_DIMENSIONS.contains(&native_kind) {
        Some(Quantity::LENGTH)
    } else if NATIVE_ANGLE_DIMENSIONS.contains(&native_kind) {
        Some(Quantity::ANGLE)
    } else {
        None
    }
}
