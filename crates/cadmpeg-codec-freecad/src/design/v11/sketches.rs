// SPDX-License-Identifier: Apache-2.0
//! The sketches of a `FreeCAD` document as v11 planar sketches (§6.1).
//!
//! A `Sketcher::SketchObject` stores each constraint by its type code and
//! its operands as `(GeoId, PointPos)` pairs, which `Sketch.cpp` reads by
//! position: which operands are set and which name a point decide the form.
//! The decoded constraint records keep fewer of these facts, so the decoder
//! keeps each constraint's stored form ([`StoredSketches`]) and the
//! projection states the relations from it. Each design record keeps the id
//! of the decoded record it restates.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use cadmpeg_core::decode::{DecodeContext, ResourceLimit};
use cadmpeg_core::text::NonBlankString;
use cadmpeg_ir::design::construction::{
    kinds, Construction, ConstructionInput, FrameAxis, InputReference, InputRole, PostStep,
    Relation, ScalarDimension, SourceMode, Subject,
};
use cadmpeg_ir::design::dimension::Dimension;
use cadmpeg_ir::design::evaluation::EvaluationStatus;
use cadmpeg_ir::design::feature::{Feature, FeatureOperation, LinkState, OperationFamily};
use cadmpeg_ir::design::ids::ContainerId;
use cadmpeg_ir::design::names::{CodecName, NativeName, NativeNameValue};
use cadmpeg_ir::design::ops::sketch::{
    AngleOperands, AngleSector, Contact, DefiningRole, Derivation, DerivationMode, DistanceAxis,
    EntityDefinition, EntityForm, EntityRole, Operand, Planar, RadialMeasure, Sketch,
    SketchConstraint, SketchEntity, SketchPlane, SketchRelation, SketchSolve, SolveState,
};
use cadmpeg_ir::design::options::{NativeOption, NativeOptions};
use cadmpeg_ir::design::parameter::DesignParameter;
use cadmpeg_ir::design::projection::sketch::{
    native_entity_kind, planar_curve, stored_frame, stored_value, DimensionValue, LocusPoints,
    StoredPoints,
};
use cadmpeg_ir::design::refs::Member;
use cadmpeg_ir::design::selection::{
    Anchor, Members, NativeOwner, OrderedSelection, PickTarget, Picked, Picks, PrincipalEntity,
    Selection,
};
use cadmpeg_ir::design::units::{Quantity, ValueKind};
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::features::parameters::ParameterValue as StoredValue;
use cadmpeg_ir::features::{
    FeatureDefinition as LegacyDefinition, FeatureId, FeatureOperation as LegacyOperation,
    ParameterId,
};
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::scalar::{Angle, FiniteReal, Length, PositiveReal};
use cadmpeg_ir::sketches::{
    Sketch as LegacySketch, SketchConstraint as LegacyConstraint,
    SketchConstraintDefinitionInput as Kind, SketchConstraintId, SketchEntity as LegacyEntity,
    SketchEntityId, SketchGeometry, SketchGeometryDefinition as Geometry, SketchId,
    SketchInternalAlignment, SketchLocus,
};
use cadmpeg_ir::transform::Transform;

use super::Source;
use crate::design::{
    EXTERNAL_GEOMETRY_DEFINING_FLAG, EXTERNAL_GEOMETRY_FROZEN_FLAG, EXTERNAL_GEOMETRY_MISSING_FLAG,
};
use crate::loss::FreecadLossCode;
use crate::native::{LinkTarget, ObjectRecord, PropertyRecord};

/// The tolerance `FreeCAD`'s `Rotation::getYawPitchRoll` uses to detect the
/// poles of the pitch angle of a rotation.
const EPS_PITCH_POLE: f64 = 16.0 * f64::EPSILON;

/// The facts of a document's sketches that the decoded sketch records do
/// not keep.
#[derive(Debug, Default)]
pub(crate) struct StoredSketches {
    /// Each constraint's stored form, by the id of its decoded record.
    pub(crate) constraints: BTreeMap<SketchConstraintId, StoredConstraint>,
    /// Each external geometry's link and flags, by the id of its decoded
    /// entity.
    pub(crate) externals: BTreeMap<SketchEntityId, StoredExternal>,
}

/// A constraint as the sketch stores it.
#[derive(Debug)]
pub(crate) struct StoredConstraint {
    /// The `Type` code.
    pub(crate) code: Option<i64>,
    /// The set operands, in stored order.
    pub(crate) operands: Vec<StoredOperand>,
    /// The parameter that holds the constraint's `Value`.
    pub(crate) parameter: Option<ParameterId>,
    /// The stored `Value`.
    pub(crate) value: Option<f64>,
}

/// One `(GeoId, PointPos)` operand of a constraint.
#[derive(Debug)]
pub(crate) struct StoredOperand {
    /// The `PointPos`: 0 the curve, 1 its start, 2 its end, 3 its center.
    pub(crate) position: i64,
    /// What the operand names.
    pub(crate) target: OperandTarget,
}

/// What a constraint operand names.
#[derive(Debug)]
pub(crate) enum OperandTarget {
    /// One of the sketch's own axes or its origin.
    Frame(PrincipalEntity),
    /// A sketch entity or a point on one.
    Locus(SketchLocus),
    /// A `GeoId` that names no entity of the sketch.
    Unresolved,
}

/// The link and flags of one external geometry.
#[derive(Debug)]
pub(crate) struct StoredExternal {
    /// The `ExternalGeometryExtension` flags, where the geometry is cached.
    pub(crate) flags: Option<u64>,
    /// The linked object and its subelements.
    pub(crate) link: Option<(String, Vec<String>)>,
}

/// The design records of the document's sketches.
#[derive(Debug, Default)]
pub(super) struct Projected {
    pub(super) sketches: Vec<Sketch>,
    pub(super) entities: Vec<SketchEntity<Planar>>,
    pub(super) constraints: Vec<SketchConstraint<Planar>>,
}

/// What the projection of one document's sketches reads.
pub(super) struct Scope<'a> {
    pub(super) ctx: &'a DecodeContext<'a>,
    pub(super) ir: &'a CadIr,
    pub(super) stored: &'a StoredSketches,
    pub(super) source: &'a Source<'a>,
    pub(super) objects: &'a [ObjectRecord],
    /// The container of each `PartDesign::Body`, by the Body's object.
    pub(super) bodies: HashMap<&'a str, ContainerId>,
    /// The members of the root `Group`.
    pub(super) grouped: &'a BTreeSet<Member>,
    pub(super) parameters: &'a [DesignParameter],
}

/// What a link to an object of the document picks.
struct Targets {
    /// The design features by the object they were read from, and whether
    /// each is a datum.
    features: HashMap<String, (FeatureId, bool)>,
    /// The origin planes, axes, and points by their object.
    origins: HashMap<String, PickTarget>,
    /// The sketches by the object they were read from.
    sketches: HashMap<String, SketchId>,
}

/// The sketch of each sketch feature among `features` and its records. Each
/// feature whose sketch the projection states operates on it.
pub(super) fn project(
    scope: &Scope<'_>,
    features: &mut [Feature],
    losses: &mut Vec<LossNote>,
) -> Result<Projected, ResourceLimit> {
    let sketches = scope
        .ir
        .model
        .features
        .iter()
        .filter_map(|legacy| {
            let operation = match legacy.evaluation.definition() {
                LegacyDefinition::Operation(operation)
                | LegacyDefinition::PostProcess { operation, .. } => operation,
            };
            let LegacyOperation::Sketch { sketch } = operation else {
                return None;
            };
            let record = scope
                .ir
                .model
                .sketches
                .iter()
                .find(|record| Some(&record.id) == sketch.id())?;
            Some((&legacy.id, record))
        })
        .collect::<BTreeMap<_, _>>();
    let projected = |feature: &Feature| {
        matches!(
            feature.definition.operation,
            FeatureOperation::Unresolved {
                family: OperationFamily::Sketch
            }
        )
        .then(|| sketches.get(&feature.id).copied())
        .flatten()
    };
    let targets = Targets {
        features: features
            .iter()
            .filter_map(|feature| {
                let datum = matches!(
                    feature.definition.operation,
                    FeatureOperation::Unresolved {
                        family: OperationFamily::Datum
                    }
                );
                Some((feature.native_ref.clone()?, (feature.id.clone(), datum)))
            })
            .collect(),
        origins: origins(scope),
        sketches: features
            .iter()
            .filter_map(|feature| {
                Some((feature.native_ref.clone()?, projected(feature)?.id.clone()))
            })
            .collect(),
    };
    let mut out = Projected::default();
    for feature in features.iter_mut() {
        let Some(record) = projected(feature) else {
            continue;
        };
        sketch(scope, &targets, feature, record, &mut out, losses)?;
        feature.definition.operation = FeatureOperation::Sketch(record.id.clone());
    }
    Ok(out)
}

fn sketch(
    scope: &Scope<'_>,
    targets: &Targets,
    feature: &Feature,
    record: &LegacySketch,
    out: &mut Projected,
    losses: &mut Vec<LossNote>,
) -> Result<(), ResourceLimit> {
    let model = &scope.ir.model;
    let anchor = if scope.grouped.contains(&Member::Feature(feature.id.clone())) {
        Anchor::Current
    } else {
        Anchor::Input
    };
    let entities = model
        .sketch_entities
        .iter()
        .filter(|entity| entity.sketch == record.id && !builtin(entity.id()))
        .collect::<Vec<_>>();
    let constraints = model
        .sketch_constraints
        .iter()
        .filter(|constraint| constraint.sketch == record.id)
        .collect::<Vec<_>>();
    let roles = roles(&constraints);
    let mut loci = Vec::new();
    for constraint in &constraints {
        let Some(stored) = scope.stored.constraints.get(&constraint.id) else {
            continue;
        };
        loci.extend(
            stored
                .operands
                .iter()
                .filter_map(|operand| match &operand.target {
                    OperandTarget::Locus(locus) => Some(locus.clone()),
                    _ => None,
                }),
        );
        loci.extend(ends_measured(stored, &entities));
    }
    // A `FreeCAD` sketch stores the points of its curves in the curves.
    let points = LocusPoints::of_loci(scope.ctx, &loci, &entities, &StoredPoints::default())?;
    let minted = points.entities(&record.id);
    let view = View {
        entities: entities
            .iter()
            .map(|entity| (entity.id(), *entity))
            .collect(),
        minted: minted.iter().map(|entity| entity.id.clone()).collect(),
        points: &points,
        roles: &roles,
    };
    let sources = sources(scope, targets, &entities, &anchor, losses);
    for entity in &entities {
        let (definition, construction) = definition(scope, entity, &anchor, &sources, losses);
        out.entities.push(SketchEntity {
            id: entity.id().clone(),
            sketch: record.id.clone(),
            definition,
            roles: roles.get(entity.id()).cloned().unwrap_or_default(),
            construction,
            centerline: None,
            native: None,
            native_options: NativeOptions::new(),
            source_settings: NativeOptions::new(),
            native_ref: entity.native_ref.clone(),
        });
    }
    out.entities.extend(minted);
    for constraint in constraints {
        if matches!(constraint.definition.kind(), Kind::InternalAlignment { .. }) {
            continue;
        }
        let Some(stored) = scope.stored.constraints.get(&constraint.id) else {
            continue;
        };
        let stated = relation(scope, &view, stored);
        if stated.native {
            losses.push(FreecadLossCode::DesignSketchRelationNative.note(format!(
                "sketch constraint {} has no typed relation of its stored operands; it states its native kind",
                constraint.id
            )));
        }
        if stated.binding == Some(Binding::Stored) {
            losses.push(FreecadLossCode::DesignSketchDimensionUnbound.note(format!(
                "sketch dimension {} binds no design parameter of its scope; it states the value it stores",
                constraint.id
            )));
        }
        if stated.unplaced {
            losses.push(FreecadLossCode::DesignSketchLocusUnplaced.note(format!(
                "sketch constraint {} names the start, end, or center of a curve that stores no position there; it states the curve",
                constraint.id
            )));
        }
        let mut native_options = NativeOptions::new();
        if let Some(orientation) = constraint.orientation.filter(|value| *value != 0) {
            option(
                &mut native_options,
                "orientation",
                NativeOption::Integer(Dimension::Literal(i64::from(orientation))),
            );
        }
        if let Some(value) = stated.angle_at_point {
            option(
                &mut native_options,
                "value",
                NativeOption::Angle(Dimension::Literal(value)),
            );
        }
        if let Some(metadata) = constraint.metadata.clone().and_then(NonBlankString::new) {
            option(
                &mut native_options,
                "metadata",
                NativeOption::Text(Dimension::Literal(metadata)),
            );
        }
        let mut source_settings = NativeOptions::new();
        for (key, value) in [
            ("visible", constraint.visible),
            ("virtual_space", constraint.virtual_space),
        ] {
            if let Some(value) = value {
                option(
                    &mut source_settings,
                    key,
                    NativeOption::Bool(Dimension::Literal(value)),
                );
            }
        }
        for (key, value) in [
            ("label_distance", constraint.label_distance),
            ("label_position", constraint.label_position),
        ] {
            if let Some(value) = value.and_then(|value| FiniteReal::new(value.get())) {
                option(
                    &mut source_settings,
                    key,
                    NativeOption::Real(Dimension::Literal(value)),
                );
            }
        }
        out.constraints.push(SketchConstraint {
            id: constraint.id.clone(),
            sketch: record.id.clone(),
            relation: stated.relation,
            name: if stated.binding == Some(Binding::Parameter) {
                None
            } else {
                constraint.name.clone()
            },
            active: constraint.active.map(Dimension::Literal),
            native_options,
            source_settings,
            native_ref: constraint.native_ref.clone(),
        });
    }
    let object = record.native_ref.as_deref();
    out.sketches.push(Sketch {
        id: record.id.clone(),
        owner: feature.id.clone(),
        name: record.name.clone(),
        plane: SketchPlane::Placed(Box::new(plane(
            scope, targets, object, record, anchor, losses,
        ))),
        solve: SketchSolve {
            state: object
                .and_then(|object| scope.source.property(object, "FullyConstrained"))
                .and_then(stored_bool)
                .map(|full| {
                    if full {
                        SolveState::Full
                    } else {
                        SolveState::NotFull
                    }
                }),
            status: object.map_or(EvaluationStatus::Current, |object| {
                scope.source.status(object)
            }),
        },
        native_options: NativeOptions::new(),
        source_settings: NativeOptions::new(),
        native_ref: record.native_ref.clone(),
    });
    Ok(())
}

/// Whether `id` is one of the entities the decoder adds for the sketch's
/// own axes and origin, which are `Frame` operands (§6.1).
fn builtin(id: &SketchEntityId) -> bool {
    [
        ":reference-horizontal-axis",
        ":reference-vertical-axis",
        ":reference-root-point",
    ]
    .iter()
    .any(|suffix| id.as_str().ends_with(suffix))
}

/// The roles that the internal alignments of a sketch give their helpers.
fn roles(constraints: &[&LegacyConstraint]) -> BTreeMap<SketchEntityId, Vec<EntityRole<Planar>>> {
    let mut roles = BTreeMap::<_, Vec<_>>::new();
    for constraint in constraints {
        let Kind::InternalAlignment {
            helper,
            parent,
            alignment,
        } = constraint.definition.kind()
        else {
            continue;
        };
        let role = match alignment {
            SketchInternalAlignment::EllipseMajorDiameter
            | SketchInternalAlignment::HyperbolaMajor => DefiningRole::MajorAxis,
            SketchInternalAlignment::EllipseMinorDiameter
            | SketchInternalAlignment::HyperbolaMinor => DefiningRole::MinorAxis,
            SketchInternalAlignment::EllipseFocus1
            | SketchInternalAlignment::HyperbolaFocus
            | SketchInternalAlignment::ParabolaFocus => DefiningRole::Focus(1),
            SketchInternalAlignment::EllipseFocus2 => DefiningRole::Focus(2),
            SketchInternalAlignment::BsplineControlPoint(index) => DefiningRole::Pole(*index),
            SketchInternalAlignment::BsplineKnotPoint(index) => DefiningRole::Knot(*index),
            SketchInternalAlignment::ParabolaFocalAxis => {
                DefiningRole::Native(cadmpeg_core::nonblank_literal!("fcstd:parabola_focal_axis"))
            }
        };
        roles.entry(helper.clone()).or_default().push(EntityRole {
            of: parent.clone(),
            role,
        });
    }
    roles
}

/// The end points of the line whose length or coordinate extent `stored`
/// measures: `FreeCAD` measures between them (`Sketch.cpp`).
fn ends_measured(stored: &StoredConstraint, entities: &[&LegacyEntity]) -> Vec<SketchLocus> {
    let [operand] = stored.operands.as_slice() else {
        return Vec::new();
    };
    match (&stored.code, &operand.target) {
        (Some(6..=8), OperandTarget::Locus(SketchLocus::Entity(id)))
            if entities.iter().any(|entity| {
                entity.id() == id && matches!(entity.geometry.definition(), Geometry::Line { .. })
            }) =>
        {
            vec![SketchLocus::Start(id.clone()), SketchLocus::End(id.clone())]
        }
        _ => Vec::new(),
    }
}

/// The entities of one sketch and the points its constraints name.
struct View<'a> {
    entities: BTreeMap<&'a SketchEntityId, &'a LegacyEntity>,
    minted: BTreeSet<SketchEntityId>,
    points: &'a LocusPoints,
    roles: &'a BTreeMap<SketchEntityId, Vec<EntityRole<Planar>>>,
}

impl View<'_> {
    fn geometry(&self, id: &SketchEntityId) -> Option<&SketchGeometry> {
        self.entities.get(id).map(|entity| &entity.geometry)
    }

    fn is_line(&self, id: &SketchEntityId) -> bool {
        self.geometry(id)
            .is_some_and(|geometry| matches!(geometry.definition(), Geometry::Line { .. }))
    }

    fn is_arc(&self, id: &SketchEntityId) -> bool {
        self.geometry(id)
            .is_some_and(|geometry| matches!(geometry.definition(), Geometry::Arc { .. }))
    }

    fn is_point(&self, id: &SketchEntityId) -> bool {
        self.minted.contains(id)
            || self
                .geometry(id)
                .is_some_and(|geometry| matches!(geometry.definition(), Geometry::Point { .. }))
    }

    fn has_role(&self, id: &SketchEntityId, role: impl Fn(&DefiningRole) -> bool) -> bool {
        self.roles
            .get(id)
            .is_some_and(|roles| roles.iter().any(|held| role(&held.role)))
    }

    /// The operand that `operand` names: an axis or the origin, an entity,
    /// or the point entity at a curve's start, end, or center.
    fn operand(&self, operand: &StoredOperand) -> Option<Operand<Planar>> {
        match &operand.target {
            OperandTarget::Frame(entity) => Some(Operand::Frame(*entity)),
            OperandTarget::Locus(SketchLocus::Entity(id)) => Some(Operand::Entity(id.clone())),
            OperandTarget::Locus(locus) => self.points.operand(locus),
            OperandTarget::Unresolved => None,
        }
    }

    /// The point entity that `operand` names.
    fn point(&self, operand: &StoredOperand) -> Option<SketchEntityId> {
        match self.operand(operand)? {
            Operand::Entity(id) if self.is_point(&id) => Some(id),
            _ => None,
        }
    }

    /// The point entity at the start or the end of the line `line`.
    fn end(&self, line: &SketchEntityId, end: bool) -> Option<Operand<Planar>> {
        self.points.operand(&if end {
            SketchLocus::End(line.clone())
        } else {
            SketchLocus::Start(line.clone())
        })
    }

    /// Whether `operand` names a point on a curve that stores no position
    /// there.
    fn unplaced(&self, operand: &StoredOperand) -> bool {
        match &operand.target {
            OperandTarget::Locus(
                locus @ (SketchLocus::Start(_) | SketchLocus::End(_) | SketchLocus::Center(_)),
            ) => self.points.operand(locus).is_none(),
            _ => false,
        }
    }
}

/// The curve that `operand` names or lies on.
fn curve_of(operand: &StoredOperand) -> Option<Operand<Planar>> {
    match &operand.target {
        OperandTarget::Frame(entity) => Some(Operand::Frame(*entity)),
        OperandTarget::Locus(
            SketchLocus::Entity(id)
            | SketchLocus::Start(id)
            | SketchLocus::End(id)
            | SketchLocus::Center(id),
        ) => Some(Operand::Entity(id.clone())),
        OperandTarget::Unresolved => None,
    }
}

/// A constraint's relation and what stating it cost.
struct Stated {
    relation: SketchRelation<Planar>,
    /// The relation is `Native` where §6.1 types the constraint's kind.
    native: bool,
    /// What the relation's dimension states.
    binding: Option<Binding>,
    /// The relation states a curve for a point whose position is not
    /// stored.
    unplaced: bool,
    /// The angle a pointwise tangency stores.
    angle_at_point: Option<Angle>,
}

/// What a constraint's dimension states.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Binding {
    /// The design parameter that holds the stored value.
    Parameter,
    /// The stored value, where no design parameter of the scope holds it.
    Stored,
}

/// The quantity of the value of a constraint of type `code`.
fn quantity(code: i64) -> Option<Quantity> {
    match code {
        6..=8 | 11 | 18 => Some(Quantity::LENGTH),
        9 => Some(Quantity::ANGLE),
        16 | 19 => Some(Quantity::REAL),
        _ => None,
    }
}

/// The relation that states the stored constraint `stored` (§6.1).
fn relation(scope: &Scope<'_>, view: &View<'_>, stored: &StoredConstraint) -> Stated {
    let mut binding = None;
    let value = stored
        .code
        .and_then(quantity)
        .zip(stored.parameter.as_ref())
        .and_then(|(quantity, parameter)| {
            if scope.parameters.iter().any(|candidate| {
                &candidate.id == parameter && candidate.kind == ValueKind::Quantity(quantity)
            }) {
                binding = Some(Binding::Parameter);
                Some((quantity, DimensionValue::Parameter(parameter.clone())))
            } else {
                binding = Some(Binding::Stored);
                stored_value(&scope.ir.model.parameters, parameter, quantity)
                    .map(|value| (quantity, value))
            }
        });
    // A pointwise tangency or perpendicularity stores the angle at its
    // point, which the solver keeps.
    let at_point = stored
        .value
        .and_then(Angle::new)
        .filter(|angle| angle.get() != 0.0);
    let typed = stored.code.and_then(|code| {
        typed(
            view,
            code,
            stored,
            value.as_ref().map(|(_, value)| value),
            at_point,
        )
    });
    if let Some((relation, angle_at_point)) = typed {
        return Stated {
            relation,
            native: false,
            binding,
            unplaced: false,
            angle_at_point,
        };
    }
    let kind = stored
        .code
        .map_or("missing_type", crate::design::constraint_kind);
    let mut values = Vec::new();
    match &value {
        Some((quantity, value)) => values.extend(value.scalar(*quantity)),
        None if matches!(stored.code, Some(5 | 10)) => {
            values.extend(at_point.map(|angle| ScalarDimension::Angle(Dimension::Literal(angle))));
        }
        None => {}
    }
    Stated {
        relation: SketchRelation::Native {
            kind: cadmpeg_core::nonblank_literal!("fcstd:{}", kind),
            operands: stored
                .operands
                .iter()
                .filter_map(|operand| view.operand(operand).or_else(|| curve_of(operand)))
                .collect(),
            values,
        },
        native: !matches!(stored.code, Some(0 | 16 | 20 | 21)),
        binding,
        unplaced: stored.operands.iter().any(|operand| view.unplaced(operand)),
        angle_at_point: None,
    }
}

/// The typed relation of a constraint of type `code`, and the angle a
/// pointwise form stores, where §6.1 types the form `Sketch.cpp` reads.
fn typed(
    view: &View<'_>,
    code: i64,
    stored: &StoredConstraint,
    value: Option<&DimensionValue>,
    at_point: Option<Angle>,
) -> Option<(SketchRelation<Planar>, Option<Angle>)> {
    let operands = stored.operands.as_slice();
    let at = |index: usize| {
        operands
            .get(index)
            .and_then(|operand| view.operand(operand))
    };
    let curve = |index: usize| operands.get(index).and_then(curve_of);
    let entity = |index: usize| match curve(index)? {
        Operand::Entity(id) => Some(id),
        Operand::Frame(_) | Operand::Model(_) => None,
    };
    let positions = operands
        .iter()
        .map(|operand| operand.position)
        .collect::<Vec<_>>();
    let relation = match (code, positions.as_slice()) {
        (1, [_, _]) => {
            for (middle, point) in [(0, 1), (1, 0)] {
                if let OperandTarget::Locus(SketchLocus::Center(line)) = &operands[middle].target {
                    if view.is_line(line) {
                        return Some((
                            SketchRelation::Midpoint {
                                point: view.point(&operands[point])?,
                                of: Operand::Entity(line.clone()),
                            },
                            None,
                        ));
                    }
                }
            }
            SketchRelation::Coincident {
                a: at(0)?,
                b: at(1)?,
            }
        }
        (2, [_]) => SketchRelation::Horizontal { a: at(0)?, b: None },
        (2, [_, _]) => SketchRelation::Horizontal {
            a: at(0)?,
            b: Some(at(1)?),
        },
        (3, [_]) => SketchRelation::Vertical { a: at(0)?, b: None },
        (3, [_, _]) => SketchRelation::Vertical {
            a: at(0)?,
            b: Some(at(1)?),
        },
        (4, [0, 0]) => SketchRelation::Parallel {
            a: at(0)?,
            b: at(1)?,
        },
        (10, [0, 0]) => SketchRelation::Perpendicular {
            a: at(0)?,
            b: at(1)?,
        },
        (5, [0, 0]) => SketchRelation::Tangent {
            a: at(0)?,
            b: at(1)?,
            contact: None,
        },
        (5, [first, second, ..]) => {
            if operands
                .iter()
                .filter_map(|operand| view.point(operand))
                .any(|point| view.has_role(&point, |role| matches!(role, DefiningRole::Knot(_))))
            {
                return None;
            }
            let contact = match operands {
                [_, _, via] => Contact::Via(view.point(via)?),
                [a, b] if *first != 0 && *second != 0 => {
                    Contact::Ends([view.point(a)?, view.point(b)?])
                }
                [a, _] if *first != 0 => Contact::Via(view.point(a)?),
                [_, b] => Contact::Via(view.point(b)?),
                _ => return None,
            };
            return Some((
                SketchRelation::Tangent {
                    a: curve(0)?,
                    b: curve(1)?,
                    contact: Some(contact),
                },
                at_point,
            ));
        }
        (6, [0]) => {
            let id = entity(0)?;
            let length = value?.length()?;
            if view.is_line(&id) {
                SketchRelation::Distance {
                    a: view.end(&id, false)?,
                    b: view.end(&id, true)?,
                    along: Some(DistanceAxis::Aligned),
                    doubled: None,
                    value: length,
                }
            } else if view.is_arc(&id) {
                SketchRelation::Length {
                    of: vec![id],
                    value: length,
                }
            } else {
                return None;
            }
        }
        (6, [_, second]) => SketchRelation::Distance {
            a: at(0)?,
            b: at(1)?,
            along: (*second != 0).then_some(DistanceAxis::Aligned),
            doubled: None,
            value: value?.length()?,
        },
        (7 | 8, [_] | [_, _]) => {
            let (a, b) = match operands {
                [only] => match view.operand(only)? {
                    Operand::Entity(id) if view.is_line(&id) => {
                        (view.end(&id, false)?, view.end(&id, true)?)
                    }
                    Operand::Entity(id) if view.is_point(&id) => {
                        (Operand::Frame(PrincipalEntity::Origin), Operand::Entity(id))
                    }
                    _ => return None,
                },
                _ => (at(0)?, at(1)?),
            };
            SketchRelation::Distance {
                a,
                b,
                along: Some(if code == 7 {
                    DistanceAxis::Horizontal
                } else {
                    DistanceAxis::Vertical
                }),
                doubled: None,
                value: value?.length()?,
            }
        }
        (9, [_]) => {
            let id = entity(0)?;
            let angle = value?.angle()?;
            if view.is_line(&id) {
                SketchRelation::Angle {
                    between: AngleOperands::Between([
                        Operand::Frame(PrincipalEntity::XAxis),
                        Operand::Entity(id),
                    ]),
                    sector: Some(directed(false)),
                    value: angle,
                }
            } else if view.is_arc(&id) {
                SketchRelation::Angle {
                    between: AngleOperands::Sweep(id),
                    sector: None,
                    value: angle,
                }
            } else {
                return None;
            }
        }
        (9, [first, second]) => SketchRelation::Angle {
            between: AngleOperands::Between([curve(0)?, curve(1)?]),
            sector: Some(directed(*second != 0 && ((*first == 2) != (*second == 2)))),
            value: value?.angle()?,
        },
        (11 | 18, [0]) => SketchRelation::Radial {
            of: entity(0)?,
            measure: if code == 11 {
                RadialMeasure::Radius
            } else {
                RadialMeasure::Diameter
            },
            value: value?.length()?,
        },
        (12, [_, _]) => SketchRelation::Equal {
            members: vec![at(0)?, at(1)?],
        },
        (13, [_, _]) => SketchRelation::Coincident {
            a: at(0)?,
            b: at(1)?,
        },
        (14, [_, _, _]) => SketchRelation::Symmetric {
            a: at(0)?,
            b: at(1)?,
            about: at(2)?,
        },
        (17, [_]) => SketchRelation::Fixed { of: at(0)? },
        (19, [0]) => {
            let pole = entity(0)?;
            if !view.has_role(&pole, |role| matches!(role, DefiningRole::Pole(_))) {
                return None;
            }
            SketchRelation::Weight {
                pole,
                value: match value? {
                    DimensionValue::Parameter(parameter) => Dimension::Parameter(parameter.clone()),
                    DimensionValue::Stored(StoredValue::Real(real)) => {
                        Dimension::Literal(PositiveReal::new(real.get())?)
                    }
                    DimensionValue::Stored(_) => return None,
                },
            }
        }
        _ => return None,
    };
    if matches!(&relation, SketchRelation::Coincident { a, b } if a == b) {
        return None;
    }
    Some((relation, None))
}

/// The directed angle `FreeCAD` measures between two directions:
/// counterclockwise from the first to the second, the second reversed where
/// `reversed` is true.
fn directed(reversed: bool) -> AngleSector {
    AngleSector::Directed {
        second_reversed: reversed,
        clockwise: false,
    }
}

/// The definition and construction flag of the decoded entity `entity`.
fn definition(
    scope: &Scope<'_>,
    entity: &LegacyEntity,
    anchor: &Anchor,
    sources: &BTreeMap<&SketchEntityId, (OrderedSelection, Option<u32>)>,
    losses: &mut Vec<LossNote>,
) -> (EntityDefinition<Planar>, Option<bool>) {
    let curve = planar_curve(&entity.geometry);
    if let Some(stored) = scope.stored.externals.get(entity.id()) {
        let (source, piece) = sources.get(entity.id()).cloned().unwrap_or_else(|| {
            (
                Selection {
                    at: anchor.clone(),
                    picks: Picks::Unread,
                    native: None,
                },
                None,
            )
        });
        let link = stored.flags.map(|flags| {
            if flags & EXTERNAL_GEOMETRY_MISSING_FLAG != 0 {
                LinkState::Broken
            } else if flags & EXTERNAL_GEOMETRY_FROZEN_FLAG != 0 {
                LinkState::Locked
            } else {
                LinkState::Current
            }
        });
        let construction = stored
            .flags
            .is_none_or(|flags| flags & EXTERNAL_GEOMETRY_DEFINING_FLAG == 0);
        return (
            EntityDefinition::Defined {
                form: Box::new(EntityForm::Derived(Box::new(Derivation {
                    mode: DerivationMode::Project,
                    source,
                    piece,
                    link,
                }))),
                solved: curve,
            },
            Some(construction),
        );
    }
    let definition = curve.map_or_else(
        || {
            losses.push(FreecadLossCode::DesignSketchEntityNative.note(format!(
                "sketch entity {} has no planar curve; it states its native kind",
                entity.id()
            )));
            EntityDefinition::Defined {
                form: Box::new(EntityForm::Native(native_entity_kind(
                    &cadmpeg_core::nonblank_literal!("fcstd"),
                    &entity.geometry,
                ))),
                solved: None,
            }
        },
        EntityDefinition::Free,
    );
    (definition, Some(entity.construction))
}

/// The source selection of each external geometry of a sketch, and its
/// `piece` among the external geometries that link one source (§6.1,
/// Derived entities).
fn sources<'e>(
    scope: &Scope<'_>,
    targets: &Targets,
    entities: &[&'e LegacyEntity],
    anchor: &Anchor,
    losses: &mut Vec<LossNote>,
) -> BTreeMap<&'e SketchEntityId, (OrderedSelection, Option<u32>)> {
    let mut by_link = BTreeMap::<&(String, Vec<String>), Vec<&SketchEntityId>>::new();
    for entity in entities {
        if let Some(link) = scope
            .stored
            .externals
            .get(entity.id())
            .and_then(|stored| stored.link.as_ref())
        {
            by_link.entry(link).or_default().push(entity.id());
        }
    }
    let mut out = BTreeMap::new();
    for (link, members) in by_link {
        let Some(selection) = picks(targets, std::slice::from_ref(link), anchor, None) else {
            losses.push(FreecadLossCode::DesignSketchSourceUnread.note(format!(
                "external geometry {} links an object that no design record restates; its source is unread",
                members
                    .iter()
                    .map(|member| member.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            )));
            continue;
        };
        let shared = members.len() > 1;
        for (index, member) in members.into_iter().enumerate() {
            let piece = shared.then(|| u32::try_from(index).ok()).flatten();
            out.insert(member, (selection.clone(), piece));
        }
    }
    out
}

/// The selection of the objects and subelements `links` name, in order: an
/// origin entity as `Principal`, a sketch as the principal entity `plane` of
/// its frame, else as the sketch, a datum feature as `Datum`, another feature
/// as its result, and a subelement of a feature by its native name. `None`
/// where a link names an object that no design record restates.
fn picks(
    targets: &Targets,
    links: &[(String, Vec<String>)],
    at: &Anchor,
    plane: Option<PrincipalEntity>,
) -> Option<OrderedSelection> {
    let mut picked = Vec::new();
    for (object, subelements) in links {
        let subelements = subelements
            .iter()
            .filter(|sub| !sub.is_empty())
            .collect::<Vec<_>>();
        if let Some(origin) = targets.origins.get(object) {
            if !subelements.is_empty() {
                return None;
            }
            picked.push(pick(origin.clone(), None));
            continue;
        }
        let (feature, datum) = targets.features.get(object)?;
        if subelements.is_empty() {
            picked.push(pick(
                if let Some(sketch) = targets.sketches.get(object) {
                    plane.map_or_else(
                        || PickTarget::Sketch(sketch.clone()),
                        |entity| PickTarget::SketchFrame {
                            sketch: sketch.clone(),
                            entity,
                        },
                    )
                } else if *datum {
                    PickTarget::Datum {
                        feature: feature.clone(),
                        part: None,
                    }
                } else {
                    PickTarget::FeatureResult(feature.clone())
                },
                None,
            ));
        }
        for sub in subelements {
            picked.push(pick(
                PickTarget::Native {
                    within: Some(NativeOwner::Feature(feature.clone())),
                },
                Some(NativeName {
                    codec: cadmpeg_core::codec_id!("fcstd"),
                    value: NativeNameValue::Text(NonBlankString::new(sub.clone())?),
                }),
            ));
        }
    }
    Some(Selection {
        at: at.clone(),
        picks: Picks::Named(Members::new(picked).ok()?),
        native: None,
    })
}

fn pick(target: PickTarget, native: Option<NativeName>) -> Picked {
    Picked {
        target,
        context: None,
        native,
        domain: None,
        witness: None,
        resolved: None,
    }
}

/// The origin planes, axes, and point of each `App::Origin` of the
/// document, as `Principal` picks (§6.2, Origin features): of the Body
/// whose `Origin` it is, else of the definition.
fn origins(scope: &Scope<'_>) -> HashMap<String, PickTarget> {
    let mut owners = HashMap::new();
    for (body, container) in &scope.bodies {
        if let Some(origin) = scope.source.property(body, "Origin").and_then(|property| {
            property
                .links()
                .iter()
                .flatten()
                .find_map(LinkTarget::object)
        }) {
            owners.insert(origin, container);
        }
    }
    let mut out = HashMap::new();
    for origin in scope
        .objects
        .iter()
        .filter(|object| object.type_name == "App::Origin")
    {
        let container = owners.get(origin.id().as_str()).copied().cloned();
        let Some(features) = scope.source.property(origin.id(), "OriginFeatures") else {
            continue;
        };
        for object in features
            .links()
            .iter()
            .flatten()
            .filter_map(LinkTarget::object)
        {
            let role = scope
                .source
                .property(object, "Role")
                .and_then(stored_text)
                .unwrap_or(object);
            let entity = [
                ("XY_Plane", PrincipalEntity::XyPlane),
                ("XZ_Plane", PrincipalEntity::XzPlane),
                ("YZ_Plane", PrincipalEntity::YzPlane),
                ("X_Axis", PrincipalEntity::XAxis),
                ("Y_Axis", PrincipalEntity::YAxis),
                ("Z_Axis", PrincipalEntity::ZAxis),
                ("Origin", PrincipalEntity::Origin),
            ]
            .into_iter()
            .find_map(|(name, entity)| role.starts_with(name).then_some(entity));
            if let Some(entity) = entity {
                out.insert(
                    object.to_owned(),
                    PickTarget::Principal {
                        entity,
                        container: container.clone(),
                    },
                );
            }
        }
    }
    out
}

/// The plane of the sketch object `object` (§6.1 Placement): no input where
/// its attachment is off, else the support as one `Whole` `Coincident`
/// input in the attachment mode, then `MapReversed` and the
/// `AttachmentOffset` steps (§5.9).
fn plane(
    scope: &Scope<'_>,
    targets: &Targets,
    object: Option<&str>,
    record: &LegacySketch,
    anchor: Anchor,
    losses: &mut Vec<LossNote>,
) -> Construction<kinds::Frame> {
    let mut construction = Construction {
        inputs: Vec::new(),
        after: Vec::new(),
        solution: None,
        mode: None,
        resolved: stored_frame(record.placement),
        resolved_in: BTreeMap::new(),
    };
    let Some(object) = object else {
        return construction;
    };
    let property = |name| scope.source.property(object, name);
    let links = property("AttachmentSupport")
        .or_else(|| property("Support"))
        .map(|support| {
            support
                .links()
                .iter()
                .flatten()
                .filter_map(|link| Some((link.object()?.to_owned(), link.subelements().to_vec())))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let mode = property("MapMode").and_then(crate::attachment::map_mode_name);
    let Some(mode) = mode.filter(|mode| !links.is_empty() && *mode != "Deactivated") else {
        return construction;
    };
    // The `Object` modes align the sketch with one plane of the support's
    // own frame; every other mode lies it on the support's plane.
    let plane = match mode {
        "ObjectXZ" => PrincipalEntity::XzPlane,
        "ObjectYZ" => PrincipalEntity::YzPlane,
        _ => PrincipalEntity::XyPlane,
    };
    let selection = picks(targets, &links, &anchor, Some(plane));
    if selection.is_none() {
        losses.push(FreecadLossCode::DesignSketchSupportUnread.note(format!(
            "sketch {} is attached to an object that no design record restates; its support is unread",
            record.id
        )));
    }
    construction.inputs.push(ConstructionInput {
        role: selection.is_some().then_some(InputRole {
            subject: Subject::Whole,
            relation: Relation::Coincident,
        }),
        reference: Some(InputReference::Picks(selection.unwrap_or(Selection {
            at: anchor,
            picks: Picks::Unread,
            native: None,
        }))),
        value: None,
        reversed: None,
    });
    let reversed = property("MapReversed").and_then(stored_bool);
    let mut source_settings = NativeOptions::new();
    match (mode, reversed) {
        ("MidPoint", Some(reversed)) => option(
            &mut source_settings,
            "MapReversed",
            NativeOption::Bool(Dimension::Literal(reversed)),
        ),
        ("MidPoint" | "Translate", _) => {}
        (_, Some(true)) => construction
            .after
            .push(PostStep::Flip(Dimension::Literal(true))),
        _ => {}
    }
    match property("AttachmentOffset").map(crate::placement::placement_matrix_unreported) {
        Some(Some(offset)) if mode == "Translate" => {
            if offset.transform() != Transform::identity() {
                losses.push(FreecadLossCode::DesignSketchOffsetUnstated.note(format!(
                    "sketch {} offsets its Translate attachment along axes the decoder does not state; its resolved frame holds the offset",
                    record.id
                )));
            }
        }
        Some(Some(offset)) => construction.after.extend(offset_steps(offset.rows())),
        Some(None) => losses.push(FreecadLossCode::DesignSketchOffsetUnstated.note(format!(
            "sketch {} stores an AttachmentOffset the decoder cannot read; its resolved frame holds the offset",
            record.id
        ))),
        None => {}
    }
    construction.mode = NonBlankString::new(mode.to_owned()).map(|method| SourceMode {
        codec: cadmpeg_core::codec_id!("fcstd"),
        method: NativeNameValue::Text(method),
        native_options: NativeOptions::new(),
        source_settings,
    });
    construction
}

/// The steps of a `FreeCAD` `AttachmentOffset` whose matrix rows are
/// `rows`: `Translate` along X, Y, and Z, then `Rotate` about Z (yaw), Y
/// (pitch), and X (roll), each left out where its literal is zero (§5.9).
fn offset_steps(rows: [[f64; 4]; 4]) -> Vec<PostStep> {
    let mut steps = Vec::new();
    for (axis, distance) in [
        (FrameAxis::X, rows[0][3]),
        (FrameAxis::Y, rows[1][3]),
        (FrameAxis::Z, rows[2][3]),
    ] {
        if let Some(distance) = Length::new(distance).filter(|_| distance != 0.0) {
            steps.push(PostStep::Translate {
                axis,
                distance: Dimension::Literal(distance),
            });
        }
    }
    let (yaw, pitch, roll) = yaw_pitch_roll(rows);
    for (axis, angle) in [
        (FrameAxis::Z, yaw),
        (FrameAxis::Y, pitch),
        (FrameAxis::X, roll),
    ] {
        if let Some(angle) = Angle::new(angle).filter(|_| angle != 0.0) {
            steps.push(PostStep::Rotate {
                axis,
                angle: Dimension::Literal(angle),
            });
        }
    }
    steps
}

/// The intrinsic Z, Y, X angles of the rotation in `rows`, as `FreeCAD`'s
/// `Rotation::getYawPitchRoll` gives them: at a pole of the pitch the yaw is zero.
fn yaw_pitch_roll(rows: [[f64; 4]; 4]) -> (f64, f64, f64) {
    let sine = -rows[2][0];
    if (sine - 1.0).abs() <= EPS_PITCH_POLE {
        (
            0.0,
            std::f64::consts::FRAC_PI_2,
            rows[0][1].atan2(rows[1][1]),
        )
    } else if (sine + 1.0).abs() <= EPS_PITCH_POLE {
        (
            0.0,
            -std::f64::consts::FRAC_PI_2,
            (-rows[0][1]).atan2(rows[1][1]),
        )
    } else {
        (
            rows[1][0].atan2(rows[0][0]),
            sine.clamp(-1.0, 1.0).asin(),
            rows[2][1].atan2(rows[2][2]),
        )
    }
}

/// The value of an `App::PropertyBool`.
fn stored_bool(property: &PropertyRecord) -> Option<bool> {
    match property.values() {
        [value] if value.tag == "Bool" => crate::native::parse_bool(value.attributes.get("value")?),
        _ => None,
    }
}

/// The value of an `App::PropertyString`.
fn stored_text(property: &PropertyRecord) -> Option<&str> {
    match property.values() {
        [value] if value.tag == "String" => value.attributes.get("value").map(String::as_str),
        _ => None,
    }
}

/// Insert `value` under the `fcstd` key `key`.
fn option(options: &mut NativeOptions, key: &str, value: NativeOption) {
    if let Some(key) = CodecName::new(&cadmpeg_core::codec_id!("fcstd"), key) {
        options.insert(key, value);
    }
}

#[cfg(test)]
mod tests;
