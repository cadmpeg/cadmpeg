// SPDX-License-Identifier: Apache-2.0
//! Sketches (§6.1): ownership, the space of each entity and constraint,
//! defining forms and roles, relation operands, patterns, regions, and the
//! parameters sketch dimensions bind.

use crate::report::check::Ruled;
use std::collections::{BTreeMap, BTreeSet};

use serde_json::Value;

use super::body::{self, RunKind};
use super::{error, Site};
use crate::design::dimension::Dimension;
use crate::design::feature::{Feature, FeatureOperation};
use crate::design::ops::sketch::TextAlign;
use crate::design::ops::sketch::{
    AngleOperands, AngleSector, Contact, DefiningRole, DerivationMode, DistanceAxis,
    EntityDefinition, EntityForm, Operand, Planar, PlanarCurve, RadialMeasure, SketchConstraint,
    SketchEntity, SketchPatternDirection, SketchPatternLayout, SketchPlane, SketchRelation,
    SketchSpace, Spatial, SpatialCurve, TextLayout, TextVerticalBox,
};
use crate::design::selection::{PickTarget, PrincipalEntity};
use crate::geometry::InlineCurve;
use crate::math::Vector3;
use crate::report::check::Finding;

use super::super::records::{DesignRecords, MemberKey};

/// The shape of an entity's curve, as far as the rules read it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Shape {
    Point,
    Line,
    InfiniteLine,
    Circle,
    Arc,
    /// An ellipse, with whether it is bounded.
    Ellipse(bool),
    Parabola(bool),
    Hyperbola(bool),
    /// A NURBS curve, with whether it is rational, and its pole and knot
    /// counts.
    Nurbs {
        rational: bool,
        poles: usize,
        knots: usize,
    },
    /// A non-planar curve in a planar sketch.
    Spatial,
    Other,
}

impl Shape {
    const fn bounded(self) -> bool {
        matches!(
            self,
            Self::Line
                | Self::Arc
                | Self::Ellipse(true)
                | Self::Parabola(true)
                | Self::Hyperbola(true)
                | Self::Nurbs { .. }
                | Self::Spatial
                | Self::Other
        )
    }

    const fn circular(self) -> bool {
        matches!(self, Self::Circle | Self::Arc)
    }
}

/// The ids and curves of one sketch space.
trait Space: SketchSpace + Sized {
    const PLANAR: bool;
    const NAME: &'static str;
    fn sketch(id: &Self::Sketch) -> &str;
    fn entity(id: &Self::Entity) -> &str;
    fn constraint(id: &Self::Constraint) -> &str;
    fn shape(curve: &Self::Curve) -> Shape;
    fn entities<'a>(index: &DesignRecords<'a>) -> &'a [SketchEntity<Self>];
    fn constraints<'a>(index: &DesignRecords<'a>) -> &'a [SketchConstraint<Self>];
}

impl Space for Planar {
    const PLANAR: bool = true;
    const NAME: &'static str = "planar";

    fn sketch(id: &Self::Sketch) -> &str {
        id.as_str()
    }

    fn entity(id: &Self::Entity) -> &str {
        id.as_str()
    }

    fn constraint(id: &Self::Constraint) -> &str {
        id.as_str()
    }

    fn shape(curve: &PlanarCurve) -> Shape {
        match curve {
            PlanarCurve::Point(_) => Shape::Point,
            PlanarCurve::Line { .. } => Shape::Line,
            PlanarCurve::InfiniteLine { .. } => Shape::InfiniteLine,
            PlanarCurve::Circle { .. } => Shape::Circle,
            PlanarCurve::Arc { .. } => Shape::Arc,
            PlanarCurve::Ellipse { range, .. } => Shape::Ellipse(range.is_some()),
            PlanarCurve::Parabola { range, .. } => Shape::Parabola(range.is_some()),
            PlanarCurve::Hyperbola { range, .. } => Shape::Hyperbola(range.is_some()),
            PlanarCurve::Nurbs(nurbs) => Shape::Nurbs {
                rational: nurbs.weights().is_some(),
                poles: nurbs.control_points().len(),
                knots: nurbs.knots().len(),
            },
            PlanarCurve::Spatial(_) => Shape::Spatial,
        }
    }

    fn entities<'a>(index: &DesignRecords<'a>) -> &'a [SketchEntity<Self>] {
        &index.ir.design.sketch_entities
    }

    fn constraints<'a>(index: &DesignRecords<'a>) -> &'a [SketchConstraint<Self>] {
        &index.ir.design.sketch_constraints
    }
}

impl Space for Spatial {
    const PLANAR: bool = false;
    const NAME: &'static str = "spatial";

    fn sketch(id: &Self::Sketch) -> &str {
        id.as_str()
    }

    fn entity(id: &Self::Entity) -> &str {
        id.as_str()
    }

    fn constraint(id: &Self::Constraint) -> &str {
        id.as_str()
    }

    fn shape(curve: &SpatialCurve) -> Shape {
        let SpatialCurve::Bounded(curve) = curve else {
            return Shape::Point;
        };
        let bounded = curve.range.is_some();
        match &curve.carrier {
            InlineCurve::Line(_) if bounded => Shape::Line,
            InlineCurve::Line(_) => Shape::InfiniteLine,
            InlineCurve::Circle(_) if bounded => Shape::Arc,
            InlineCurve::Circle(_) => Shape::Circle,
            InlineCurve::Ellipse(_) => Shape::Ellipse(bounded),
            InlineCurve::Parabola(_) => Shape::Parabola(bounded),
            InlineCurve::Hyperbola(_) => Shape::Hyperbola(bounded),
            InlineCurve::Nurbs(nurbs) => Shape::Nurbs {
                rational: nurbs.weights().is_some(),
                poles: nurbs.control_points().len(),
                knots: nurbs.knots().len(),
            },
            InlineCurve::Polyline(_) | InlineCurve::Transformed { .. } => Shape::Other,
        }
    }

    fn entities<'a>(index: &DesignRecords<'a>) -> &'a [SketchEntity<Self>] {
        &index.ir.design.spatial_sketch_entities
    }

    fn constraints<'a>(index: &DesignRecords<'a>) -> &'a [SketchConstraint<Self>] {
        &index.ir.design.spatial_sketch_constraints
    }
}

/// The entities of one space, by id.
struct Entities<'a, S: Space> {
    by_id: BTreeMap<&'a str, &'a SketchEntity<S>>,
    /// Each (of, role) pair, by the role's spelling, with the role and the
    /// entities that hold it.
    roles: BTreeMap<(&'a str, String), (&'a DefiningRole, Vec<&'a str>)>,
}

impl<'a, S: Space> Entities<'a, S> {
    fn new(entities: &'a [SketchEntity<S>]) -> Self {
        let mut by_id = BTreeMap::new();
        let mut roles: BTreeMap<_, (&DefiningRole, Vec<&str>)> = BTreeMap::new();
        for entity in entities {
            by_id.entry(S::entity(&entity.id)).or_insert(entity);
            for role in &entity.roles {
                roles
                    .entry((S::entity(&role.of), role_key(&role.role)))
                    .or_insert((&role.role, Vec::new()))
                    .1
                    .push(S::entity(&entity.id));
            }
        }
        Self { by_id, roles }
    }

    /// The shape of an entity's curve: its free curve or its solved one.
    fn shape(&self, id: &str) -> Option<Shape> {
        curve(self.by_id.get(id)?).map(S::shape)
    }

    fn form(&self, id: &str) -> Option<&'a EntityForm<S>> {
        match &self.by_id.get(id)?.definition {
            EntityDefinition::Defined { form, .. } => Some(form),
            EntityDefinition::Free(_) => None,
        }
    }

    /// Whether the entity is a point entity: its curve is a point, or it is
    /// derived with no solved curve. An entity whose curve is not stated
    /// otherwise counts as one.
    fn point(&self, id: &str) -> bool {
        self.shape(id).is_none_or(|shape| shape == Shape::Point)
    }

    /// Whether the shape is known and `test` refuses it.
    fn refuses(&self, id: &str, test: impl Fn(Shape) -> bool) -> bool {
        self.shape(id).is_some_and(|shape| !test(shape))
    }

    fn bounds(&self, point: &str, curve: &str) -> bool {
        [DefiningRole::Start, DefiningRole::End].iter().any(|role| {
            self.roles
                .get(&(curve, role_key(role)))
                .is_some_and(|(_, holders)| holders.contains(&point))
        })
    }

    fn holds(&self, of: &str, role: &DefiningRole) -> bool {
        self.roles.contains_key(&(of, role_key(role)))
    }

    fn spline(&self, id: &str) -> bool {
        matches!(self.form(id), Some(EntityForm::Interpolated(_)))
            || matches!(self.shape(id), Some(Shape::Nurbs { .. }))
    }
}

fn role_key(role: &DefiningRole) -> String {
    format!("{role:?}")
}

fn curve<S: SketchSpace>(entity: &SketchEntity<S>) -> Option<&S::Curve> {
    match &entity.definition {
        EntityDefinition::Free(curve) => Some(curve),
        EntityDefinition::Defined { solved, .. } => solved.as_ref(),
    }
}

fn shape_in<S: Space>(index: &DesignRecords<'_>, id: &str) -> Option<Shape> {
    S::entities(index)
        .iter()
        .find(|entity| S::entity(&entity.id) == id)
        .and_then(curve)
        .map(S::shape)
}

/// The stated shape of the sketch entity a pick targets. `None` where the
/// pick targets no sketch entity, or the entity states no curve.
fn picked_shape(index: &DesignRecords<'_>, target: &PickTarget) -> Option<Shape> {
    match target {
        PickTarget::SketchEntity(id) => shape_in::<Planar>(index, id.as_str()),
        PickTarget::SpatialSketchEntity(id) => shape_in::<Spatial>(index, id.as_str()),
        _ => None,
    }
}

/// Whether a pick targets a sketch entity whose stated curve is no point.
pub(super) fn picks_no_point(index: &DesignRecords<'_>, target: &PickTarget) -> bool {
    picked_shape(index, target).is_some_and(|shape| shape != Shape::Point)
}

/// Whether a pick targets a sketch entity whose stated curve is no circle
/// or arc.
pub(super) fn picks_no_circle(index: &DesignRecords<'_>, target: &PickTarget) -> bool {
    picked_shape(index, target).is_some_and(|shape| !shape.circular())
}

/// The sketches of the design, both spaces.
struct Sketches<'a> {
    planar: BTreeMap<&'a str, &'a crate::design::ops::sketch::Sketch>,
    spatial: BTreeMap<&'a str, &'a crate::design::ops::sketch::SpatialSketch>,
}

impl<'a> Sketches<'a> {
    fn new(index: &DesignRecords<'a>) -> Self {
        let design = &index.ir.design;
        Self {
            planar: design.sketches.iter().map(|s| (s.id.as_str(), s)).collect(),
            spatial: design
                .spatial_sketches
                .iter()
                .map(|s| (s.id.as_str(), s))
                .collect(),
        }
    }

    fn contains<S: Space>(&self, sketch: &str) -> bool {
        if S::PLANAR {
            self.planar.contains_key(sketch)
        } else {
            self.spatial.contains_key(sketch)
        }
    }

    fn contains_other<S: Space>(&self, sketch: &str) -> bool {
        if S::PLANAR {
            self.spatial.contains_key(sketch)
        } else {
            self.planar.contains_key(sketch)
        }
    }

    /// The scope of a sketch's owner.
    fn owner_scope<'b>(&self, index: &DesignRecords<'b>, sketch: &str) -> Option<&'b str> {
        let owner = self
            .planar
            .get(sketch)
            .map(|s| s.owner.as_str())
            .or_else(|| self.spatial.get(sketch).map(|s| s.owner.as_str()))?;
        let (&owner, _) = index.features.get_key_value(owner)?;
        index.scope_of(MemberKey::Feature(owner))
    }
}

pub(super) fn check_sketches(index: &DesignRecords<'_>, wire: &Value, findings: &mut Vec<Finding>) {
    let sketches = Sketches::new(index);
    check_ownership(index, findings);
    for sketch in &index.ir.design.sketches {
        if let SketchPlane::Placed(plane) = &sketch.plane {
            if let Some(frame) = &plane.resolved {
                let axes = [frame.x_axis, frame.y_axis, frame.z_axis].map(Vector3::from);
                let tolerance = index.ir.tolerances.angular.get();
                let orthogonal =
                    (0..3).all(|i| (i + 1..3).all(|j| axes[i].dot(axes[j]).abs() <= tolerance));
                if !orthogonal {
                    error(
                        findings,
                        sketch.id.as_str(),
                        crate::rule!(
                            "design.sketch-resolved-frame-axes-not-orthonormal",
                            "the sketch's resolved frame axes are not orthonormal"
                        ),
                    );
                }
            }
        }
    }
    check_space::<Planar>(index, &sketches, findings);
    check_space::<Spatial>(index, &sketches, findings);
    check_block_cycles(index, findings);
    check_regions(index, wire, findings);
}

/// Each sketch's owner exists and owns it by its operation, its overrides,
/// or its definition; a `Block` sketch is owned by a `Sketch` feature.
fn check_ownership(index: &DesignRecords<'_>, findings: &mut Vec<Finding>) {
    let design = &index.ir.design;
    let mut overridden: BTreeMap<&str, BTreeSet<&str>> = BTreeMap::new();
    let overrides = design
        .configurations
        .iter()
        .map(|c| &c.overrides)
        .chain(design.arrangements.iter().map(|a| &a.overrides));
    for overrides in overrides {
        for (feature, state) in &overrides.features {
            if let Some(definition) = &state.definition {
                let named = match &definition.operation {
                    FeatureOperation::Sketch(sketch) => Some(sketch.as_str()),
                    FeatureOperation::SpatialSketch(sketch) => Some(sketch.as_str()),
                    _ => None,
                };
                overridden
                    .entry(feature.as_str())
                    .or_default()
                    .extend(named);
            }
        }
    }
    let owners = design
        .sketches
        .iter()
        .map(|s| {
            (
                s.id.as_str(),
                s.owner.as_str(),
                matches!(s.plane, SketchPlane::Block),
            )
        })
        .chain(
            design
                .spatial_sketches
                .iter()
                .map(|s| (s.id.as_str(), s.owner.as_str(), false)),
        );
    for (sketch, owner, block) in owners {
        let Some(feature) = index.features.get(owner) else {
            error(
                findings,
                sketch,
                crate::rule!(
                    "design.sketch-owner-no-feature-design",
                    "the sketch's owner {owner} is no feature of the design"
                ),
            );
            continue;
        };
        let own = match &feature.definition.operation {
            FeatureOperation::Sketch(own) => Some(own.as_str()),
            FeatureOperation::SpatialSketch(own) => Some(own.as_str()),
            _ => None,
        };
        let sketch_feature = matches!(feature.definition.operation, FeatureOperation::Sketch(_));
        if block && !sketch_feature {
            error(
                findings,
                sketch,
                crate::rule!(
                    "design.block-sketch-owned-no-sketch-operation",
                    "a Block sketch is owned by {owner}, which has no Sketch operation"
                ),
            );
        }
        let owns = match own {
            Some(own) => {
                own == sketch
                    || overridden
                        .get(owner)
                        .is_some_and(|named| named.contains(sketch))
            }
            None => names_sketch(index, feature, sketch),
        };
        if !owns {
            error(
                findings,
                sketch,
                crate::rule!("design.sketch-owner-neither-holds-operation-nor", "the sketch's owner {owner} neither holds it as its operation nor names it in its definition"),
            );
        }
    }
    for feature in &design.features {
        let (own, planar) = match &feature.definition.operation {
            FeatureOperation::Sketch(own) => (own.as_str(), true),
            FeatureOperation::SpatialSketch(own) => (own.as_str(), false),
            _ => continue,
        };
        let owner = if planar {
            design
                .sketches
                .iter()
                .find(|s| s.id.as_str() == own)
                .map(|s| s.owner.as_str())
        } else {
            design
                .spatial_sketches
                .iter()
                .find(|s| s.id.as_str() == own)
                .map(|s| s.owner.as_str())
        };
        if owner != Some(feature.id.as_str()) {
            error(
                findings,
                feature.id.as_str(),
                crate::rule!(
                    "design.feature-sketch-operation-names-no-sketch",
                    "the feature's sketch operation names {own}, which is no sketch it owns"
                ),
            );
        }
    }
}

/// Whether a feature's definition, or a definition override of it, names
/// the sketch.
fn names_sketch(index: &DesignRecords<'_>, feature: &Feature, sketch: &str) -> bool {
    let design = &index.ir.design;
    let overrides = design
        .configurations
        .iter()
        .map(|c| &c.overrides)
        .chain(design.arrangements.iter().map(|a| &a.overrides))
        .filter_map(|overrides| overrides.features.get(&feature.id)?.definition.as_ref());
    std::iter::once(&feature.definition)
        .chain(overrides)
        .any(|definition| super::wire_strings(definition).contains(sketch))
}

fn check_space<S: Space>(
    index: &DesignRecords<'_>,
    sketches: &Sketches<'_>,
    findings: &mut Vec<Finding>,
) {
    let records = S::entities(index);
    let entities = Entities::<S>::new(records);
    for entity in records {
        check_entity(index, sketches, &entities, entity, findings);
    }
    for ((of, _), (role, holders)) in &entities.roles {
        if holders.len() > 1 {
            error(
                findings,
                of,
                crate::rule!(
                    "design.entities-hold-one-role-entity",
                    "{} entities hold one role {role:?} of the entity",
                    holders.len()
                ),
            );
        }
    }
    check_fit_indices(&entities, findings);
    for constraint in S::constraints(index) {
        check_constraint(index, sketches, &entities, constraint, findings);
    }
}

fn check_entity<S: Space>(
    index: &DesignRecords<'_>,
    sketches: &Sketches<'_>,
    entities: &Entities<'_, S>,
    entity: &SketchEntity<S>,
    findings: &mut Vec<Finding>,
) {
    let id = S::entity(&entity.id);
    let sketch = S::sketch(&entity.sketch);
    check_sketch_space::<S>(sketches, id, sketch, findings);
    let same = |other: &S::Entity| {
        entities
            .by_id
            .get(S::entity(other))
            .is_some_and(|other| S::sketch(&other.sketch) == sketch)
    };
    let mut refuse = |message: Ruled<'_>| error(findings, id, message);
    match &entity.definition {
        EntityDefinition::Free(curve) => {
            if S::shape(curve) == Shape::Spatial {
                refuse(crate::rule!(
                    "design.free-entity-planar-sketch-spatial-curve",
                    "a Free entity of a planar sketch has a Spatial curve"
                ));
            }
        }
        EntityDefinition::Defined { form, solved } => {
            let shape = solved.as_ref().map(S::shape);
            let derived = matches!(**form, EntityForm::Derived(_));
            if solved.is_none()
                && !matches!(
                    **form,
                    EntityForm::Derived(_)
                        | EntityForm::Text(_)
                        | EntityForm::BlockInstance(_)
                        | EntityForm::Native(_)
                )
            {
                refuse(crate::rule!("design.entity-solved-curve-none-only-derived", 
                    "the entity's solved curve is None; only Derived, Text, BlockInstance, and Native forms omit it"
                ));
            }
            if shape == Some(Shape::Spatial) && !derived {
                refuse(crate::rule!(
                    "design.planarcurve-spatial-occurs-solved-curve-form",
                    "PlanarCurve::Spatial occurs as the solved curve of a form other than Derived"
                ));
            }
            match &**form {
                EntityForm::Interpolated(_) => {
                    if shape.is_some_and(|shape| !matches!(shape, Shape::Nurbs { .. })) {
                        refuse(crate::rule!(
                            "design.interpolated-entity-solved-curve-not-nurbs",
                            "an Interpolated entity's solved curve is not a NURBS"
                        ));
                    }
                }
                EntityForm::Conic { .. } => {
                    let conic = matches!(
                        shape,
                        None | Some(
                            Shape::Ellipse(true)
                                | Shape::Parabola(true)
                                | Shape::Hyperbola(true)
                                | Shape::Nurbs { rational: true, .. }
                        )
                    );
                    if !conic {
                        refuse(crate::rule!("design.conic-entity-solved-curve-no-ellipse", 
                            "a Conic entity's solved curve is no ellipse, parabola, or hyperbola arc, nor a rational NURBS"
                        ));
                    }
                }
                EntityForm::Derived(derivation) => {
                    if solved.is_none()
                        && matches!(
                            derivation.source.picks,
                            crate::design::selection::Picks::Unread
                        )
                    {
                        refuse(crate::rule!(
                            "design.derived-entity-no-solved-curve-unread",
                            "a Derived entity with no solved curve has an Unread source"
                        ));
                    }
                    if let DerivationMode::OntoFaces { faces, .. } = &derivation.mode {
                        if let Some(class) = super::picks::refused(faces, super::picks::is_face) {
                            refuse(crate::rule!(
                                "design.derivationmode-ontofaces-faces-picks-not-face",
                                "DerivationMode::OntoFaces.faces picks {class}, not a face"
                            ));
                        }
                    }
                }
                EntityForm::Text(text) => {
                    let layout: Vec<&S::Entity> = match &text.layout {
                        Some(TextLayout::Frame(frame)) => frame.iter().collect(),
                        Some(TextLayout::Path { curve, .. }) => vec![curve],
                        None => Vec::new(),
                    };
                    if !layout.into_iter().all(same) {
                        refuse(crate::rule!(
                            "design.text-layout-names-entity-another-sketch",
                            "a text layout names an entity of another sketch"
                        ));
                    }
                    if text.text.vertical_box == Some(TextVerticalBox::CapHeight)
                        && text.text.vertical == Some(TextAlign::Baseline)
                    {
                        refuse(crate::rule!(
                            "design.textcontent-aligned-capheight-states-vertical-baseline",
                            "a TextContent aligned to CapHeight states vertical Baseline"
                        ));
                    }
                }
                EntityForm::Instanced { instance, of } => {
                    let block = match entities.form(S::entity(instance)) {
                        Some(EntityForm::BlockInstance(block)) if same(instance) => {
                            Some(block.block.as_str())
                        }
                        _ => None,
                    };
                    match block {
                        None => refuse(crate::rule!(
                            "design.instanced-instance-no-blockinstance-entity-same",
                            "Instanced.instance is no BlockInstance entity of the same sketch"
                        )),
                        Some(block) => {
                            let of_block = index
                                .ir
                                .design
                                .sketch_entities
                                .iter()
                                .find(|e| e.id == *of)
                                .is_some_and(|e| e.sketch.as_str() == block);
                            if !of_block {
                                refuse(crate::rule!(
                                    "design.instanced-no-entity-instance-block-definition",
                                    "Instanced.of is no entity of the instance's block definition"
                                ));
                            }
                        }
                    }
                }
                EntityForm::Frame => {
                    if shape.is_some_and(|shape| shape != Shape::Point) {
                        refuse(crate::rule!(
                            "design.frame-entity-solved-curve-not-point",
                            "a Frame entity's solved curve is not a Point"
                        ));
                    }
                }
                EntityForm::BlockInstance(_) | EntityForm::Native(_) => {}
            }
        }
    }
    for role in &entity.roles {
        check_role(
            entities,
            entity,
            &role.of,
            &role.role,
            same(&role.of),
            findings,
        );
    }
}

fn check_sketch_space<S: Space>(
    sketches: &Sketches<'_>,
    record: &str,
    sketch: &str,
    findings: &mut Vec<Finding>,
) {
    if sketches.contains::<S>(sketch) {
        return;
    }
    if sketches.contains_other::<S>(sketch) {
        error(
            findings,
            record,
            crate::rule!(
                "design.record-sketch-names-sketch-other-space",
                "the record of a {} sketch names {sketch}, a sketch of the other space",
                S::NAME
            ),
        );
    } else {
        error(
            findings,
            record,
            crate::rule!(
                "design.record-names-sketch-no-sketch",
                "the record names sketch {sketch}, which is no {} sketch",
                S::NAME
            ),
        );
    }
}

fn check_role<S: Space>(
    entities: &Entities<'_, S>,
    holder: &SketchEntity<S>,
    of: &S::Entity,
    role: &DefiningRole,
    same_sketch: bool,
    findings: &mut Vec<Finding>,
) {
    let id = S::entity(&holder.id);
    let of = S::entity(of);
    if !same_sketch {
        error(
            findings,
            id,
            crate::rule!(
                "design.entity-holds-role-no-entity-sketch",
                "the entity holds a role of {of}, which is no entity of its sketch"
            ),
        );
        return;
    }
    let holder_shape = curve(holder).map(S::shape);
    let derived_unsolved = matches!(
        &holder.definition,
        EntityDefinition::Defined { form, solved: None } if matches!(**form, EntityForm::Derived(_))
    );
    let point_role = matches!(
        role,
        DefiningRole::Start
            | DefiningRole::End
            | DefiningRole::Center
            | DefiningRole::Apex
            | DefiningRole::Focus(_)
            | DefiningRole::Fit(_)
            | DefiningRole::Anchor
            | DefiningRole::Insertion
    );
    let mut refuse = |message: Ruled<'_>| error(findings, id, message);
    if point_role && !derived_unsolved && holder_shape.is_some_and(|shape| shape != Shape::Point) {
        refuse(crate::rule!(
            "design.entity-holds-point-role-no-point",
            "the entity holds the point role {role:?} but is no point"
        ));
    }
    let target = entities.shape(of);
    let form = entities.form(of);
    match role {
        DefiningRole::Start | DefiningRole::End => {
            if target.is_some_and(|shape| !shape.bounded()) {
                refuse(crate::rule!(
                    "design.targets-no-bounded-curve",
                    "{role:?} targets {of}, which is no bounded curve"
                ));
            }
        }
        DefiningRole::Center => {
            if target.is_some_and(|shape| {
                !matches!(
                    shape,
                    Shape::Circle | Shape::Arc | Shape::Ellipse(_) | Shape::Hyperbola(_)
                )
            }) {
                refuse(crate::rule!(
                    "design.center-targets-no-circle-arc-ellipse",
                    "Center targets {of}, which is no circle, arc, ellipse, or hyperbola"
                ));
            }
        }
        DefiningRole::Apex if !matches!(form, Some(EntityForm::Conic { .. })) => {
            refuse(crate::rule!(
                "design.apex-targets-no-conic",
                "Apex targets {of}, which is no Conic"
            ));
        }
        DefiningRole::Anchor if !matches!(form, Some(EntityForm::Text(_))) => {
            refuse(crate::rule!(
                "design.anchor-targets-no-text",
                "Anchor targets {of}, which is no Text"
            ));
        }
        DefiningRole::Insertion if !matches!(form, Some(EntityForm::BlockInstance(_))) => {
            refuse(crate::rule!(
                "design.insertion-targets-no-blockinstance",
                "Insertion targets {of}, which is no BlockInstance"
            ));
        }
        DefiningRole::Pole(i) | DefiningRole::Knot(i) => {
            if let Some(Shape::Nurbs { poles, knots, .. }) = target {
                let count = if matches!(role, DefiningRole::Pole(_)) {
                    poles
                } else {
                    knots
                };
                if usize::try_from(*i).is_ok_and(|i| i >= count) {
                    refuse(crate::rule!(
                        "design.lies-past",
                        "{role:?} lies past the {count} of {of}"
                    ));
                }
            }
        }
        DefiningRole::Handle(i) => {
            let line = holder.construction == Some(true) && holder_shape == Some(Shape::Line);
            let spline = match form {
                Some(EntityForm::Interpolated(spline)) => Some(spline),
                _ => None,
            };
            let fit = entities.holds(of, &DefiningRole::Fit(*i));
            let free = spline.is_some_and(|spline| {
                spline
                    .handles
                    .get(i)
                    .is_none_or(|handle| handle.direction.is_none())
            });
            if !line || spline.is_none() || !fit || !free {
                refuse(crate::rule!("design.handle-held-other-construction-line-interpolated", 
                    "Handle({i}) is held other than by a construction Line of an Interpolated spline with fit point {i} and an undirected handle there"
                ));
            }
        }
        DefiningRole::XAxis | DefiningRole::YAxis => {
            let frame = matches!(form, Some(EntityForm::Frame));
            let axis =
                holder.construction == Some(true) && holder_shape == Some(Shape::InfiniteLine);
            if !frame || !axis {
                refuse(crate::rule!(
                    "design.held-other-construction-infiniteline-frame-entity",
                    "{role:?} is held other than by a construction InfiniteLine of a Frame entity"
                ));
            }
        }
        _ => {}
    }
}

/// The `Fit` indices of one spline are 0 to n−1 without gaps.
fn check_fit_indices<S: Space>(entities: &Entities<'_, S>, findings: &mut Vec<Finding>) {
    let mut fits: BTreeMap<&str, BTreeSet<u32>> = BTreeMap::new();
    for ((of, _), (role, _)) in &entities.roles {
        if let DefiningRole::Fit(i) = role {
            fits.entry(of).or_default().insert(*i);
        }
    }
    for (spline, indices) in fits {
        let gapless = indices
            .iter()
            .enumerate()
            .all(|(at, &index)| usize::try_from(index).is_ok_and(|index| index == at));
        if !gapless {
            error(
                findings,
                spline,
                crate::rule!(
                    "design.spline-fit-indices-not-0-n",
                    "the spline's Fit indices are not 0 to n−1 without gaps"
                ),
            );
        }
    }
}

/// Constraint operands of one sketch, and the classes of entity they name.
struct Operands<'a, 'b, S: Space> {
    entities: &'b Entities<'a, S>,
    sketch: &'b str,
    id: &'b str,
}

impl<S: Space> Operands<'_, '_, S> {
    fn entity(&self, entity: &S::Entity, findings: &mut Vec<Finding>) {
        let name = S::entity(entity);
        let same = self
            .entities
            .by_id
            .get(name)
            .is_some_and(|e| S::sketch(&e.sketch) == self.sketch);
        if !same {
            error(
                findings,
                self.id,
                crate::rule!(
                    "design.constraint-names-no-entity-sketch",
                    "the constraint names {name}, which is no entity of its sketch"
                ),
            );
        }
    }

    fn operand(&self, operand: &Operand<S>, findings: &mut Vec<Finding>) {
        match operand {
            Operand::Entity(entity) => self.entity(entity, findings),
            Operand::Frame(principal) => {
                if !S::PLANAR {
                    error(
                        findings,
                        self.id,
                        crate::rule!(
                            "design.operand-frame-occurs-spatial-sketch",
                            "Operand::Frame occurs in a spatial sketch"
                        ),
                    );
                } else if !matches!(
                    principal,
                    PrincipalEntity::XAxis | PrincipalEntity::YAxis | PrincipalEntity::Origin
                ) {
                    error(
                        findings,
                        self.id,
                        crate::rule!(
                            "design.operand-frame-names-other-xaxis-yaxis",
                            "Operand::Frame names other than XAxis, YAxis, or Origin"
                        ),
                    );
                }
            }
            Operand::Model(selection) => {
                let own = super::picks::named(selection).is_some_and(|picks| {
                    picks.iter().any(|pick| {
                        matches!(&pick.target, PickTarget::SketchFrame { sketch, .. } if sketch.as_str() == self.sketch)
                    })
                });
                if own {
                    error(
                        findings,
                        self.id,
                        crate::rule!(
                            "design.model-operand-picks-own-sketch-sketchframe",
                            "a Model operand picks its own sketch's SketchFrame"
                        ),
                    );
                }
            }
        }
    }

    /// The operand's shape where it names an entity whose curve is stated;
    /// a frame axis is a line and the frame origin a point.
    fn shape(&self, operand: &Operand<S>) -> Option<Shape> {
        match operand {
            Operand::Entity(entity) => self.entities.shape(S::entity(entity)),
            Operand::Frame(PrincipalEntity::Origin) => Some(Shape::Point),
            Operand::Frame(_) => Some(Shape::InfiniteLine),
            Operand::Model(_) => None,
        }
    }

    fn refuse(&self, findings: &mut Vec<Finding>, message: Ruled<'_>) {
        error(findings, self.id, message);
    }

    fn planar_only(&self, findings: &mut Vec<Finding>, what: &str) {
        if !S::PLANAR {
            self.refuse(
                findings,
                crate::rule!(
                    "design.occurs-spatial-sketch",
                    "{what} occurs in a spatial sketch"
                ),
            );
        }
    }

    fn point(&self, entity: &S::Entity) -> bool {
        self.entities.point(S::entity(entity))
    }
}

fn line(shape: Shape) -> bool {
    matches!(shape, Shape::Line | Shape::InfiniteLine)
}

fn check_constraint<S: Space>(
    index: &DesignRecords<'_>,
    sketches: &Sketches<'_>,
    entities: &Entities<'_, S>,
    constraint: &SketchConstraint<S>,
    findings: &mut Vec<Finding>,
) {
    let id = S::constraint(&constraint.id);
    let sketch = S::sketch(&constraint.sketch);
    check_sketch_space::<S>(sketches, id, sketch, findings);
    let ops = Operands {
        entities,
        sketch,
        id,
    };
    check_relation(&ops, &constraint.relation, findings);
    let value_binds = match &constraint.relation {
        SketchRelation::Distance { value, .. }
        | SketchRelation::Radial { value, .. }
        | SketchRelation::Length { value, .. } => !matches!(value, Dimension::Literal(_)),
        SketchRelation::Angle { value, .. } => !matches!(value, Dimension::Literal(_)),
        SketchRelation::Offset(offset) => !matches!(offset.distance, Dimension::Literal(_)),
        SketchRelation::Weight { value, .. } => !matches!(value, Dimension::Literal(_)),
        _ => false,
    };
    if value_binds && constraint.name.is_some() {
        error(
            findings,
            id,
            crate::rule!("design.dimension-constraint-binds-parameter-states-name", "a dimension constraint that binds a parameter states a name; the parameter's name is its name"),
        );
    }
    check_bound_scope(index, sketches, id, sketch, &constraint.relation, findings);
}

/// A sketch dimension binds only design parameters of the sketch owner's
/// scope.
fn check_bound_scope<T: serde::Serialize>(
    index: &DesignRecords<'_>,
    sketches: &Sketches<'_>,
    id: &str,
    sketch: &str,
    value: &T,
    findings: &mut Vec<Finding>,
) {
    let Some(scope) = sketches.owner_scope(index, sketch) else {
        return;
    };
    let Ok(wire) = serde_json::to_value(value) else {
        return;
    };
    let bound = super::tagged(&wire, "parameter")
        .into_iter()
        .chain(super::tagged(&wire, "negated"))
        .filter_map(|found| found.get("value")?.as_str());
    for parameter in bound {
        if let Some(record) = index.parameters.get(parameter) {
            if record.scope.as_str() != scope {
                error(
                    findings,
                    id,
                    crate::rule!("design.sketch-dimension-binds-parameter-another-scope", "the sketch dimension binds {parameter}, a parameter of another scope than the sketch owner's"),
                );
            }
        }
    }
}

fn check_relation<S: Space>(
    ops: &Operands<'_, '_, S>,
    relation: &SketchRelation<S>,
    findings: &mut Vec<Finding>,
) {
    match relation {
        SketchRelation::Coincident { a, b }
        | SketchRelation::Collinear { a, b }
        | SketchRelation::Parallel { a, b }
        | SketchRelation::Perpendicular { a, b } => {
            ops.operand(a, findings);
            ops.operand(b, findings);
        }
        SketchRelation::Concentric { a, b } => {
            for operand in [a, b] {
                ops.operand(operand, findings);
                if ops
                    .shape(operand)
                    .is_some_and(|shape| !shape.circular() && !matches!(shape, Shape::Ellipse(_)))
                {
                    ops.refuse(
                        findings,
                        crate::rule!(
                            "design.concentric-operand-not-circular-elliptic",
                            "a Concentric operand is not circular or elliptic"
                        ),
                    );
                }
            }
        }
        SketchRelation::Tangent { a, b, contact } | SketchRelation::Curvature { a, b, contact } => {
            ops.operand(a, findings);
            ops.operand(b, findings);
            match contact {
                Some(Contact::Ends([p, q])) => {
                    ops.entity(p, findings);
                    ops.entity(q, findings);
                    let bounds = |point: &S::Entity, operand: &Operand<S>| match operand {
                        Operand::Entity(curve) => {
                            ops.entities.bounds(S::entity(point), S::entity(curve))
                        }
                        _ => true,
                    };
                    if !bounds(p, a) || !bounds(q, b) {
                        ops.refuse(
                            findings,
                            crate::rule!(
                                "design.contact-ends-names-point-does-not",
                                "Contact::Ends names a point that does not bound its curve"
                            ),
                        );
                    }
                }
                Some(Contact::Via(point)) => {
                    ops.entity(point, findings);
                    if !ops.point(point) {
                        ops.refuse(
                            findings,
                            crate::rule!(
                                "design.contact-via-names-no-point-entity",
                                "Contact::Via names no point entity"
                            ),
                        );
                    }
                }
                None => {}
            }
        }
        SketchRelation::Horizontal { a, b } | SketchRelation::Vertical { a, b } => {
            ops.planar_only(findings, "Horizontal or Vertical");
            ops.operand(a, findings);
            let fits = match b {
                None => ops.shape(a).is_none_or(line),
                Some(b) => {
                    ops.operand(b, findings);
                    [a, b]
                        .iter()
                        .all(|operand| ops.shape(operand).is_none_or(|shape| shape == Shape::Point))
                }
            };
            if !fits {
                ops.refuse(
                    findings,
                    crate::rule!(
                        "design.horizontal-vertical-takes-one-line-two",
                        "Horizontal or Vertical takes one line or two points"
                    ),
                );
            }
        }
        SketchRelation::Midpoint { point, of } => {
            ops.entity(point, findings);
            ops.operand(of, findings);
            if !ops.point(point) || ops.shape(of).is_some_and(|shape| !shape.bounded()) {
                ops.refuse(
                    findings,
                    crate::rule!(
                        "design.midpoint-takes-point-entity-bounded-curve",
                        "Midpoint takes a point entity and a bounded curve"
                    ),
                );
            }
        }
        SketchRelation::Symmetric { a, b, about } => {
            for operand in [a, b, about] {
                ops.operand(operand, findings);
            }
        }
        SketchRelation::Equal { members } => {
            for member in members {
                ops.operand(member, findings);
            }
            let shapes: Vec<Shape> = members.iter().filter_map(|m| ops.shape(m)).collect();
            let lines = shapes.iter().all(|&shape| line(shape));
            let circles = shapes.iter().all(|shape| shape.circular());
            if members.len() < 2 || !(lines || circles) {
                ops.refuse(findings, crate::rule!("design.equal-takes-least-two-members-one", "Equal takes at least two members of one kind class, lines or circular entities"));
            }
        }
        SketchRelation::EqualDistance { first, second } => {
            for operand in first.iter().chain(second) {
                ops.operand(operand, findings);
            }
        }
        SketchRelation::Fixed { of } => ops.operand(of, findings),
        SketchRelation::Distance {
            a, b, along, value, ..
        } => {
            ops.operand(a, findings);
            ops.operand(b, findings);
            match along {
                Some(DistanceAxis::Horizontal | DistanceAxis::Vertical) => {
                    ops.planar_only(findings, "DistanceAxis::Horizontal or Vertical");
                }
                Some(DistanceAxis::Along(operand)) => ops.operand(operand, findings),
                Some(DistanceAxis::Aligned) => {
                    if matches!(value, Dimension::Literal(value) if value.get() < 0.0) {
                        ops.refuse(
                            findings,
                            crate::rule!(
                                "design.aligned-distance-negative-value",
                                "an Aligned Distance has a negative value"
                            ),
                        );
                    }
                }
                None => {}
            }
        }
        SketchRelation::Angle {
            between, sector, ..
        } => {
            if matches!(sector, Some(AngleSector::Directed { .. })) {
                ops.planar_only(findings, "AngleSector::Directed");
            }
            match between {
                AngleOperands::Between([a, b]) => {
                    ops.operand(a, findings);
                    ops.operand(b, findings);
                }
                AngleOperands::Sweep(arc) => {
                    ops.entity(arc, findings);
                    if ops.entities.refuses(S::entity(arc), |shape| {
                        matches!(shape, Shape::Arc | Shape::Ellipse(true))
                    }) {
                        ops.refuse(
                            findings,
                            crate::rule!(
                                "design.angleoperands-sweep-names-no-arc-elliptic",
                                "AngleOperands::Sweep names no arc or elliptic arc"
                            ),
                        );
                    }
                }
                AngleOperands::Points(points) => {
                    for point in points {
                        ops.entity(point, findings);
                    }
                    if !points.iter().all(|point| ops.point(point)) {
                        ops.refuse(
                            findings,
                            crate::rule!(
                                "design.angleoperands-points-names-entity-no-point",
                                "AngleOperands::Points names an entity that is no point"
                            ),
                        );
                    }
                }
            }
        }
        SketchRelation::Radial { of, measure, .. } => {
            ops.entity(of, findings);
            let ellipse = matches!(
                measure,
                RadialMeasure::MajorRadius | RadialMeasure::MinorRadius
            );
            let refused = ops.entities.refuses(S::entity(of), |shape| {
                if ellipse {
                    matches!(shape, Shape::Ellipse(_))
                } else {
                    shape.circular() || matches!(shape, Shape::Ellipse(_))
                }
            });
            if refused {
                ops.refuse(
                    findings,
                    crate::rule!(
                        "design.radial-no-circle-arc-ellipse-measure",
                        "Radial.of is no circle, arc, or ellipse its measure applies to"
                    ),
                );
            }
        }
        SketchRelation::Length { of, .. } => {
            if of.is_empty() {
                ops.refuse(
                    findings,
                    crate::rule!("design.length-empty", "Length.of is empty"),
                );
            }
            for entity in of {
                ops.entity(entity, findings);
            }
        }
        SketchRelation::Offset(offset) => {
            if offset.pairs.is_empty() {
                ops.refuse(
                    findings,
                    crate::rule!(
                        "design.sketchoffset-pairs-empty",
                        "SketchOffset.pairs is empty"
                    ),
                );
            }
            let mut results = BTreeSet::new();
            let sources: BTreeSet<&str> = offset.pairs.iter().map(|[s, _]| S::entity(s)).collect();
            for [source, result] in &offset.pairs {
                ops.entity(source, findings);
                ops.entity(result, findings);
                let result = S::entity(result);
                if !results.insert(result) || sources.contains(result) {
                    ops.refuse(findings, crate::rule!("design.offset-result-result-two-pairs-source", "an offset result is a result in two pairs or a source of the same offset"));
                }
            }
        }
        SketchRelation::Pattern(pattern) => check_pattern(ops, pattern, findings),
        SketchRelation::Polygon(polygon) => {
            ops.entity(&polygon.center, findings);
            for edge in &polygon.edges {
                ops.entity(edge, findings);
            }
            let lines = polygon.edges.iter().all(|edge| {
                !ops.entities
                    .refuses(S::entity(edge), |shape| shape == Shape::Line)
            });
            if polygon.edges.len() < 3 || !lines {
                ops.refuse(
                    findings,
                    crate::rule!(
                        "design.sketchpolygon-edges-holds-fewer-three-lines",
                        "SketchPolygon.edges holds fewer than three lines"
                    ),
                );
            }
            match &polygon.circle {
                Some(circle) => {
                    ops.entity(circle, findings);
                    if ops
                        .entities
                        .refuses(S::entity(circle), |shape| shape == Shape::Circle)
                    {
                        ops.refuse(
                            findings,
                            crate::rule!(
                                "design.sketchpolygon-circle-no-circle-entity",
                                "SketchPolygon.circle is no circle entity"
                            ),
                        );
                    }
                }
                None if polygon.inscribed.is_some() => {
                    ops.refuse(
                        findings,
                        crate::rule!(
                            "design.sketchpolygon-inscribed-occurs-without-circle",
                            "SketchPolygon.inscribed occurs without a circle"
                        ),
                    );
                }
                None => {}
            }
        }
        SketchRelation::Slot(slot) => {
            for entity in slot.centers.iter().chain(&slot.edges) {
                ops.entity(entity, findings);
            }
            if !slot.centers.iter().all(|center| ops.point(center)) {
                ops.refuse(
                    findings,
                    crate::rule!(
                        "design.sketchslot-centers-names-entity-no-point",
                        "SketchSlot.centers names an entity that is no point"
                    ),
                );
            }
        }
        SketchRelation::Pierce { point, curve } => {
            ops.entity(point, findings);
            ops.operand(curve, findings);
            if !ops.point(point) {
                ops.refuse(
                    findings,
                    crate::rule!(
                        "design.pierce-point-no-point-entity",
                        "Pierce.point is no point entity"
                    ),
                );
            }
        }
        SketchRelation::Weight { pole, .. } => {
            ops.entity(pole, findings);
            let holds = ops
                .entities
                .by_id
                .get(S::entity(pole))
                .is_some_and(|entity| {
                    entity.roles.iter().any(|role| {
                        matches!(role.role, DefiningRole::Pole(_))
                            && ops.entities.spline(S::entity(&role.of))
                    })
                });
            if !holds {
                ops.refuse(
                    findings,
                    crate::rule!(
                        "design.weight-pole-holds-no-pole-role",
                        "Weight.pole holds no Pole role of a spline entity"
                    ),
                );
            }
        }
        SketchRelation::FixedLength { of } | SketchRelation::FixedAngle { of } => {
            for entity in of {
                ops.entity(entity, findings);
            }
            let lines = of
                .iter()
                .all(|entity| !ops.entities.refuses(S::entity(entity), line));
            if of.is_empty() || !lines {
                ops.refuse(
                    findings,
                    crate::rule!(
                        "design.fixedlength-fixedangle-names-no-lines-entity",
                        "FixedLength or FixedAngle names no lines, or an entity that is no line"
                    ),
                );
            }
        }
        SketchRelation::Native { operands, .. } => {
            for operand in operands {
                ops.operand(operand, findings);
            }
        }
    }
}

fn check_pattern<S: Space>(
    ops: &Operands<'_, '_, S>,
    pattern: &crate::design::ops::sketch::SketchPattern<S>,
    findings: &mut Vec<Finding>,
) {
    if pattern.seeds.is_empty() {
        ops.refuse(
            findings,
            crate::rule!(
                "design.sketchpattern-seeds-empty",
                "SketchPattern.seeds is empty"
            ),
        );
    }
    let site = Site {
        entity: ops.id,
        feature: ops.id,
        scope: None,
        assembly: false,
    };
    let seeds: BTreeSet<&str> = pattern.seeds.iter().map(|s| S::entity(s)).collect();
    for seed in &pattern.seeds {
        ops.entity(seed, findings);
    }
    match &pattern.layout {
        SketchPatternLayout::Linear { directions } => {
            if !(1..=2).contains(&directions.len()) {
                ops.refuse(
                    findings,
                    crate::rule!(
                        "design.linear-sketch-pattern-directions-one-two",
                        "a Linear sketch pattern has {} directions; it has one or two",
                        directions.len()
                    ),
                );
            }
            for axis in directions {
                body::check_spacing(
                    site,
                    "a sketch pattern direction",
                    &axis.spacing,
                    RunKind {
                        fit: false,
                        up_to: false,
                    },
                    findings,
                );
                match &axis.direction {
                    Some(SketchPatternDirection::Angle(_)) => {
                        ops.planar_only(findings, "SketchPatternDirection::Angle");
                    }
                    Some(SketchPatternDirection::Along { line, .. }) => ops.operand(line, findings),
                    None => {}
                }
            }
        }
        SketchPatternLayout::Circular {
            center, spacing, ..
        } => {
            ops.operand(center, findings);
            body::check_spacing(
                site,
                "a circular sketch pattern",
                spacing,
                RunKind {
                    fit: true,
                    up_to: false,
                },
                findings,
            );
        }
        SketchPatternLayout::Native(_) => {}
    }
    let mut placed = BTreeSet::new();
    let mut indices = Vec::new();
    for instance in &pattern.instances {
        if indices.contains(&&instance.index) {
            ops.refuse(
                findings,
                crate::rule!(
                    "design.sketch-pattern-instance-index-repeats",
                    "sketch pattern instance index {:?} repeats",
                    instance.index.0
                ),
            );
        }
        indices.push(&instance.index);
        let suppressed = instance.suppressed == Some(true);
        if !suppressed && instance.entities.len() != pattern.seeds.len() {
            ops.refuse(
                findings,
                crate::rule!(
                    "design.sketch-pattern-instance-entities-seeds",
                    "a sketch pattern instance has {} entities for {} seeds",
                    instance.entities.len(),
                    pattern.seeds.len()
                ),
            );
        }
        for entity in &instance.entities {
            ops.entity(entity, findings);
            let entity = S::entity(entity);
            if !placed.insert(entity) || seeds.contains(entity) {
                ops.refuse(
                    findings,
                    crate::rule!(
                        "design.sketch-entity-two-instances-both-seed",
                        "sketch entity {entity} is in two instances or both seed and instance"
                    ),
                );
            }
        }
    }
}

/// Block instancing forms no cycle.
fn check_block_cycles(index: &DesignRecords<'_>, findings: &mut Vec<Finding>) {
    let mut places: BTreeMap<&str, BTreeSet<&str>> = BTreeMap::new();
    for entity in &index.ir.design.sketch_entities {
        if let EntityDefinition::Defined { form, .. } = &entity.definition {
            if let EntityForm::BlockInstance(block) = &**form {
                places
                    .entry(entity.sketch.as_str())
                    .or_default()
                    .insert(block.block.as_str());
            }
        }
    }
    for &start in places.keys() {
        let mut seen = BTreeSet::new();
        let mut pending: Vec<&str> = places[start].iter().copied().collect();
        while let Some(sketch) = pending.pop() {
            if sketch == start {
                error(
                    findings,
                    start,
                    crate::rule!(
                        "design.block-instancing-places-sketch-inside-itself",
                        "block instancing places the sketch inside itself"
                    ),
                );
                break;
            }
            if seen.insert(sketch) {
                pending.extend(places.get(sketch).into_iter().flatten().copied());
            }
        }
    }
}

/// Every `SketchRegion` pick: a nonempty outer boundary of non-construction
/// curves of its sketch, and fragments below their counts.
fn check_regions(index: &DesignRecords<'_>, wire: &Value, findings: &mut Vec<Finding>) {
    let entities: BTreeMap<&str, &SketchEntity<Planar>> = index
        .ir
        .design
        .sketch_entities
        .iter()
        .map(|e| (e.id.as_str(), e))
        .collect();
    for found in super::tagged(wire, "sketch_region") {
        let Ok(PickTarget::SketchRegion { sketch, region }) =
            serde_json::from_value::<PickTarget>(found.clone())
        else {
            continue;
        };
        let at = sketch.as_str();
        if region.outer.is_empty() {
            error(
                findings,
                at,
                crate::rule!(
                    "design.sketchregion-outer-boundary-empty",
                    "a SketchRegion's outer boundary is empty"
                ),
            );
        }
        for boundary in region.outer.iter().chain(region.holes.iter().flatten()) {
            let entity = entities.get(boundary.entity.as_str());
            let curve = entity.is_some_and(|entity| {
                entity.sketch == sketch
                    && entity.construction != Some(true)
                    && curve(entity).is_none_or(|c| Planar::shape(c) != Shape::Point)
            });
            if !curve {
                error(
                    findings,
                    at,
                    crate::rule!("design.sketchregion-boundary-use-names-no-non", "a SketchRegion boundary use names {}, which is no non-construction curve of the sketch", boundary.entity.as_str()),
                );
            }
            if boundary
                .fragment
                .as_ref()
                .is_some_and(|f| f.index >= f.count)
            {
                error(
                    findings,
                    at,
                    crate::rule!(
                        "design.sketchregion-boundary-fragment-index-not-below",
                        "a SketchRegion boundary fragment's index is not below its count"
                    ),
                );
            }
        }
    }
}
