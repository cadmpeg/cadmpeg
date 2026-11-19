// SPDX-License-Identifier: Apache-2.0
//! Sketches (§6.1): each section the decoder reads, with its entities and
//! its constraints.
//!
//! A curve's stored end is the point entity of its endpoint record, with a
//! `Start` or `End` role; a centre a constraint names is a point entity
//! with a `Center` role. The decoder reads no section's support references,
//! so a sketch's plane states the stored frame where the decoder reads one,
//! and otherwise one input with no role and an `Unread` selection of the
//! support (§5.9). A relation whose v10 reading has no v11 relation of
//! the same meaning keeps its operands and values as a `Native` relation.

use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::text::NonBlankString;
use cadmpeg_core::CodecError;
use cadmpeg_ir::design::construction::{
    kinds, Construction, ConstructionInput, FrameValue, InputReference, ScalarDimension,
};
use cadmpeg_ir::design::dimension::Dimension;
use cadmpeg_ir::design::evaluation::EvaluationStatus;
use cadmpeg_ir::design::ops::sketch::{
    AngleOperands, Contact, DefiningRole, DistanceAxis, EntityDefinition, EntityForm, EntityRole,
    Operand, Planar, PlanarCurve, RadialMeasure, Sketch, SketchConstraint, SketchEntity,
    SketchPlane, SketchRelation, SketchSolve,
};
use cadmpeg_ir::design::selection::{Anchor, Picks, PrincipalEntity, Selection};
use cadmpeg_ir::design::units::{Quantity, ValueKind};
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::features::{
    FeatureDefinition as LegacyDefinition, FeatureId, FeatureOperation as LegacyOperation,
    ParameterId, SketchFeatureBinding,
};
use cadmpeg_ir::ids::IdentityNamespace;
use cadmpeg_ir::math::portable::Portable;
use cadmpeg_ir::math::{Point2, Vector3};
use cadmpeg_ir::scalar::Angle;
use cadmpeg_ir::sketches::{
    Sketch as LegacySketch, SketchAxis, SketchConstraintDefinitionInput as Input,
    SketchCoordinateAxis, SketchEntity as LegacyEntity, SketchEntityId, SketchGeometry,
    SketchGeometryDefinition as Geometry, SketchId, SketchLocus,
};
use cadmpeg_ir::units::{FinitePoint2, UnitVector2, UnitVector3};

use crate::container::ContainerScan;

const SKETCH_POINT: IdentityNamespace =
    cadmpeg_ir::identity_namespace!("creo", "model", "sketch_point");
const SKETCH_CENTER: IdentityNamespace =
    cadmpeg_ir::identity_namespace!("creo", "model", "sketch_center");

/// A point entity agrees with a curve end it is joined to within this
/// distance, in millimetres.
const END_AGREEMENT: f64 = 1e-6;

/// States every sketch whose owning feature the decoder reads, and returns
/// the sketches a sketch feature of their own owns.
pub(super) fn project(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan,
    ir: &mut CadIr,
) -> Result<BTreeSet<SketchId>, CodecError> {
    let (owners, standalone) = owners(scan, ir);
    let kinds = ir
        .design
        .parameters
        .iter()
        .map(|parameter| (parameter.id.clone(), parameter.kind.clone()))
        .collect::<BTreeMap<_, _>>();
    let mut sketches = Vec::new();
    let mut entities = Vec::new();
    let mut constraints = Vec::new();
    let mut roles = BTreeMap::new();
    for sketch in &ir.model.sketches {
        let Some(owner) = owners.get(&sketch.id) else {
            continue;
        };
        ctx.charge_entities(1, "project Creo v11 sketch")?;
        sketches.push(project_sketch(sketch, owner));
        let record = |native_ref: &Option<String>| record(sketch, native_ref.as_ref());
        let mut points = Points::new(
            sketch,
            ir.model
                .sketch_entities
                .iter()
                .filter(|entity| entity.sketch == sketch.id),
        )?;
        for entity in ir
            .model
            .sketch_entities
            .iter()
            .filter(|entity| entity.sketch == sketch.id)
        {
            ctx.charge_entities(1, "project Creo v11 sketch entity")?;
            let mut projected = project_entity(entity)?;
            projected.native_ref = record(&entity.native_ref);
            entities.push(projected);
        }
        for constraint in ir
            .model
            .sketch_constraints
            .iter()
            .filter(|constraint| constraint.sketch == sketch.id)
        {
            let Some(relation) = relation(constraint.definition.kind(), &mut points, &kinds)?
            else {
                continue;
            };
            ctx.charge_entities(1, "project Creo v11 sketch constraint")?;
            constraints.push(SketchConstraint {
                id: constraint.id.clone(),
                sketch: sketch.id.clone(),
                relation,
                name: constraint.name.clone(),
                active: constraint.active.map(Dimension::Literal),
                native_options: BTreeMap::new(),
                source_settings: BTreeMap::new(),
                native_ref: record(&constraint.native_ref),
            });
        }
        ctx.charge_entities(
            cadmpeg_core::decode::u64_from_index(points.minted.len()),
            "project Creo v11 sketch point",
        )?;
        roles.append(&mut points.roles);
        entities.extend(points.minted.into_values());
    }
    for entity in &mut entities {
        if let Some(roles) = roles.remove(&entity.id) {
            entity.roles = roles;
        }
    }
    sketches.sort_by(|a, b| a.id.cmp(&b.id));
    entities.sort_by(|a, b| a.id.cmp(&b.id));
    constraints.sort_by(|a, b| a.id.cmp(&b.id));
    ir.design.sketches.extend(sketches);
    ir.design.sketch_entities.extend(entities);
    ir.design.sketch_constraints.extend(constraints);
    Ok(standalone
        .into_iter()
        .filter(|sketch| owners.contains_key(sketch))
        .collect())
}

/// The planar sketch a v10 sketch feature names.
pub(super) fn sketch_of(operation: &LegacyOperation) -> Option<&SketchId> {
    match operation {
        LegacyOperation::Sketch {
            sketch: SketchFeatureBinding::Planar(Some(sketch)),
            ..
        } => Some(sketch),
        _ => None,
    }
}

/// The native record a sketch record names: a saved entity is a part of its
/// sketch's record, which is the record a v11 record names.
fn record(sketch: &LegacySketch, native_ref: Option<&String>) -> Option<String> {
    let native_ref = native_ref?;
    match &sketch.native_ref {
        Some(parent)
            if native_ref
                .strip_prefix(parent.as_str())
                .is_some_and(|rest| rest.starts_with(':')) =>
        {
            Some(parent.clone())
        }
        _ => Some(native_ref.clone()),
    }
}

/// The feature that owns each sketch: its own sketch feature, else the
/// feature the decoder joins its section to. The second set holds the
/// sketches a sketch feature owns.
fn owners(scan: &ContainerScan, ir: &CadIr) -> (BTreeMap<SketchId, FeatureId>, BTreeSet<SketchId>) {
    let mut owners = BTreeMap::new();
    let mut standalone = BTreeSet::new();
    for feature in &ir.model.features {
        let (LegacyDefinition::Operation(operation)
        | LegacyDefinition::PostProcess { operation, .. }) = feature.evaluation.definition();
        if let Some(sketch) = sketch_of(operation) {
            owners.insert(sketch.clone(), feature.id.clone());
            standalone.insert(sketch.clone());
        }
    }
    let features = ir
        .model
        .features
        .iter()
        .map(|feature| &feature.id)
        .collect::<BTreeSet<_>>();
    for definition in &scan.features.definitions {
        if !crate::decode::sketch_ids::feature_definition_has_sketch_design(definition) {
            continue;
        }
        let Some(sketch) = crate::decode::sketch_ids::model_sketch_id(scan, definition) else {
            continue;
        };
        let Some(owner) = crate::decode::sketch_ids::section_owner_feature_id(
            scan,
            definition.identity.id(),
            &sketch,
        ) else {
            continue;
        };
        if features.contains(&owner) {
            owners.entry(sketch).or_insert(owner);
        }
    }
    (owners, standalone)
}

fn project_sketch(sketch: &LegacySketch, owner: &FeatureId) -> Sketch {
    let resolved = sketch
        .placement
        .resolved()
        .and_then(|(origin, normal, u_axis)| {
            let z_axis = UnitVector3::normalized(normal.get())?;
            let x_axis = UnitVector3::normalized(u_axis.get())?;
            let y_axis =
                UnitVector3::normalized(Vector3::from(z_axis).cross(Vector3::from(x_axis)))?;
            Some(FrameValue {
                origin,
                x_axis,
                y_axis,
                z_axis,
            })
        });
    let inputs = if resolved.is_some() {
        Vec::new()
    } else {
        vec![ConstructionInput {
            role: None,
            reference: Some(InputReference::Picks(Selection {
                at: Anchor::Input,
                picks: Picks::Unread,
                native: None,
            })),
            value: None,
            reversed: None,
        }]
    };
    Sketch {
        id: sketch.id.clone(),
        owner: owner.clone(),
        name: sketch.name.clone(),
        plane: SketchPlane::Placed(Box::new(Construction::<kinds::Frame> {
            inputs,
            resolved,
            ..Construction::default()
        })),
        solve: SketchSolve {
            state: None,
            status: EvaluationStatus::Current,
        },
        native_options: BTreeMap::new(),
        source_settings: BTreeMap::new(),
        native_ref: sketch.native_ref.clone(),
    }
}

/// A native kind in the Creo namespace; a kind the decoder already names
/// there keeps its name.
fn creo_kind(kind: &str) -> String {
    if kind.starts_with("creo:") {
        kind.to_owned()
    } else {
        format!("creo:{kind}")
    }
}

fn native_form(kind: &str) -> Result<EntityDefinition<Planar>, CodecError> {
    Ok(EntityDefinition::Defined {
        form: Box::new(EntityForm::Native(
            NonBlankString::new(creo_kind(kind))
                .ok_or_else(|| CodecError::malformed("Creo v11 sketch entity kind is blank"))?,
        )),
        solved: None,
    })
}

fn direction(angle: Angle) -> Option<UnitVector2> {
    UnitVector2::new([angle.get().portable_cos(), angle.get().portable_sin()])
}

/// The solved curve of an entity whose v10 geometry has a v11 curve.
fn curve(geometry: &SketchGeometry) -> Option<PlanarCurve> {
    Some(match geometry.definition() {
        Geometry::Point { position } => PlanarCurve::Point(*position),
        Geometry::Line { start, end } => PlanarCurve::Line {
            start: *start,
            end: *end,
        },
        Geometry::ReferenceLine { origin, direction } => {
            let length = direction.u.portable_hypot(direction.v);
            PlanarCurve::InfiniteLine {
                through: *origin,
                direction: UnitVector2::new([direction.u / length, direction.v / length])?,
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
        } => PlanarCurve::Ellipse {
            center: *center,
            major: direction(*major_angle)?,
            radii: [radii.major(), radii.minor()],
            range: *bounds,
        },
        Geometry::Nurbs { curve } => PlanarCurve::Nurbs(curve.clone()),
        Geometry::Hyperbola { .. }
        | Geometry::Parabola { .. }
        | Geometry::Text { .. }
        | Geometry::ExternalReference { .. }
        | Geometry::Native { .. } => return None,
    })
}

fn project_entity(entity: &LegacyEntity) -> Result<SketchEntity<Planar>, CodecError> {
    let definition = match curve(&entity.geometry) {
        Some(curve) => EntityDefinition::Free(curve),
        None => match entity.geometry.definition() {
            Geometry::Hyperbola { .. } => native_form("hyperbola")?,
            Geometry::Parabola { .. } => native_form("parabola")?,
            Geometry::Text { .. } => native_form("text")?,
            Geometry::ExternalReference { .. } => native_form("external_reference")?,
            Geometry::Native { native_kind } => native_form(native_kind.as_str())?,
            _ => native_form("curve")?,
        },
    };
    Ok(SketchEntity {
        id: entity.id().clone(),
        sketch: entity.sketch.clone(),
        definition,
        roles: Vec::new(),
        construction: Some(entity.construction),
        centerline: None,
        native: None,
        native_options: BTreeMap::new(),
        source_settings: BTreeMap::new(),
        native_ref: entity.native_ref.clone(),
    })
}

/// The stored ends of a line or an arc, in the order of its endpoint
/// records.
fn ends(geometry: &PlanarCurve) -> Option<[FinitePoint2; 2]> {
    match geometry {
        PlanarCurve::Line { start, end } => Some([*start, *end]),
        PlanarCurve::Arc {
            center,
            radius,
            start,
            end,
        } => {
            let at = |angle: Angle| {
                FinitePoint2::new(Point2 {
                    u: center.get().u + radius.get() * angle.get().portable_cos(),
                    v: center.get().v + radius.get() * angle.get().portable_sin(),
                })
            };
            Some([at(*start)?, at(*end)?])
        }
        _ => None,
    }
}

fn agree(a: FinitePoint2, b: FinitePoint2) -> bool {
    (a.get().u - b.get().u).portable_hypot(a.get().v - b.get().v) <= END_AGREEMENT
}

/// The point entities of one sketch: its point records, the curve ends
/// joined to them, and the centres its constraints name. A point of a curve
/// whose geometry the decoder does not read has no position.
struct Points {
    sketch: SketchId,
    native_ref: Option<String>,
    unread_point: NonBlankString,
    curves: BTreeMap<SketchEntityId, Option<PlanarCurve>>,
    unread: BTreeMap<SketchEntityId, String>,
    point_entities: BTreeSet<SketchEntityId>,
    ends: BTreeMap<(SketchEntityId, usize), SketchEntityId>,
    centers: BTreeMap<SketchEntityId, SketchEntityId>,
    minted: BTreeMap<SketchEntityId, SketchEntity<Planar>>,
    roles: BTreeMap<SketchEntityId, Vec<EntityRole<Planar>>>,
}

impl Points {
    fn new<'a>(
        sketch: &LegacySketch,
        entities: impl Iterator<Item = &'a LegacyEntity> + Clone,
    ) -> Result<Self, CodecError> {
        let mut points = Self {
            sketch: sketch.id.clone(),
            native_ref: sketch.native_ref.clone(),
            unread_point: NonBlankString::new("creo:point")
                .ok_or_else(|| CodecError::malformed("Creo v11 point kind is blank"))?,
            curves: BTreeMap::new(),
            unread: BTreeMap::new(),
            point_entities: BTreeSet::new(),
            ends: BTreeMap::new(),
            centers: BTreeMap::new(),
            minted: BTreeMap::new(),
            roles: BTreeMap::new(),
        };
        let mut records = BTreeMap::new();
        for entity in entities.clone() {
            let geometry = curve(&entity.geometry);
            if let (Some(PlanarCurve::Point(position)), [record]) =
                (&geometry, entity.endpoint_refs.as_slice())
            {
                records.insert(record.clone(), (entity.id().clone(), Some(*position)));
                points.point_entities.insert(entity.id().clone());
            }
            if let Geometry::Native { native_kind } = entity.geometry.definition() {
                points
                    .unread
                    .insert(entity.id().clone(), native_kind.as_str().to_owned());
            }
            points.curves.insert(entity.id().clone(), geometry);
        }
        for entity in entities {
            let Some(geometry) = points.curves.get(entity.id()).cloned() else {
                continue;
            };
            let [first, second] = entity.endpoint_refs.as_slice() else {
                continue;
            };
            let positions = match &geometry {
                Some(geometry) => match ends(geometry) {
                    Some(positions) => Some(positions),
                    None => continue,
                },
                None if points.unread.contains_key(entity.id()) => None,
                None => continue,
            };
            for (index, record) in [first, second].into_iter().enumerate() {
                let position = positions.map(|positions| positions[index]);
                let point = match records.get(record) {
                    Some((point, stored)) => {
                        if let (Some(stored), Some(position)) = (stored, position) {
                            if !agree(*stored, position) {
                                continue;
                            }
                        }
                        point.clone()
                    }
                    None => {
                        let Some(point) = minted_point(record) else {
                            continue;
                        };
                        records.insert(record.clone(), (point.clone(), position));
                        points.mint(point.clone(), position);
                        point
                    }
                };
                points.role(
                    &point,
                    entity.id(),
                    if index == 0 {
                        DefiningRole::Start
                    } else {
                        DefiningRole::End
                    },
                );
                points.ends.insert((entity.id().clone(), index), point);
            }
        }
        Ok(points)
    }

    fn mint(&mut self, id: SketchEntityId, position: Option<FinitePoint2>) {
        let definition = match position {
            Some(position) => EntityDefinition::Free(PlanarCurve::Point(position)),
            None => EntityDefinition::Defined {
                form: Box::new(EntityForm::Native(self.unread_point.clone())),
                solved: None,
            },
        };
        self.point_entities.insert(id.clone());
        self.minted.insert(
            id.clone(),
            SketchEntity {
                id,
                sketch: self.sketch.clone(),
                definition,
                roles: Vec::new(),
                construction: None,
                centerline: None,
                native: None,
                native_options: BTreeMap::new(),
                source_settings: BTreeMap::new(),
                native_ref: self.native_ref.clone(),
            },
        );
    }

    fn role(&mut self, point: &SketchEntityId, of: &SketchEntityId, role: DefiningRole) {
        self.roles
            .entry(point.clone())
            .or_default()
            .push(EntityRole {
                of: of.clone(),
                role,
            });
    }

    fn center(&mut self, of: &SketchEntityId) -> Option<SketchEntityId> {
        if let Some(center) = self.centers.get(of) {
            return Some(center.clone());
        }
        let position = match self.curves.get(of)? {
            Some(
                PlanarCurve::Circle { center, .. }
                | PlanarCurve::Arc { center, .. }
                | PlanarCurve::Ellipse { center, .. },
            ) => Some(*center),
            None if matches!(
                self.unread.get(of).map(String::as_str),
                Some("circle" | "arc")
            ) =>
            {
                None
            }
            _ => return None,
        };
        let key = of.as_str().split_once('#')?.1;
        let id = crate::identity::compose_checked::<SketchEntityId>(&SKETCH_CENTER, key).ok()?;
        self.mint(id.clone(), position);
        self.role(&id, of, DefiningRole::Center);
        self.centers.insert(of.clone(), id.clone());
        Some(id)
    }

    /// The point entity a locus names.
    fn point(&mut self, locus: &SketchLocus) -> Option<SketchEntityId> {
        match locus {
            SketchLocus::Entity(entity) => {
                self.point_entities.contains(entity).then(|| entity.clone())
            }
            SketchLocus::Start(entity) => self.ends.get(&(entity.clone(), 0)).cloned(),
            SketchLocus::End(entity) => self.ends.get(&(entity.clone(), 1)).cloned(),
            SketchLocus::Center(entity) => self.center(entity),
        }
    }

    /// The operand a locus names: an entity, or the point entity of a curve's
    /// end or centre.
    fn operand(&mut self, locus: &SketchLocus) -> Option<Operand<Planar>> {
        match locus {
            SketchLocus::Entity(entity) => self.entity(entity),
            _ => self.point(locus).map(Operand::Entity),
        }
    }

    fn entity(&self, entity: &SketchEntityId) -> Option<Operand<Planar>> {
        self.curves
            .contains_key(entity)
            .then(|| Operand::Entity(entity.clone()))
    }

    fn position(&self, point: &SketchEntityId) -> Option<FinitePoint2> {
        match self.curves.get(point) {
            Some(Some(PlanarCurve::Point(position))) => Some(*position),
            _ => match &self.minted.get(point)?.definition {
                EntityDefinition::Free(PlanarCurve::Point(position)) => Some(*position),
                _ => None,
            },
        }
    }
}

/// The point entity id of an endpoint record the sketch stores no point
/// entity for.
fn minted_point(record: &str) -> Option<SketchEntityId> {
    let key = record
        .strip_prefix("creo:featdefs:sketch#")?
        .replacen(":point#", "/", 1);
    crate::identity::compose_checked::<SketchEntityId>(&SKETCH_POINT, key).ok()
}

fn length(kinds: &BTreeMap<ParameterId, ValueKind>, parameter: &ParameterId) -> bool {
    kinds.get(parameter) == Some(&ValueKind::Quantity(Quantity::LENGTH))
}

fn angle(kinds: &BTreeMap<ParameterId, ValueKind>, parameter: &ParameterId) -> bool {
    kinds.get(parameter) == Some(&ValueKind::Quantity(Quantity::ANGLE))
}

fn value(kinds: &BTreeMap<ParameterId, ValueKind>, parameter: &ParameterId) -> ScalarDimension {
    match kinds.get(parameter) {
        Some(ValueKind::Quantity(quantity)) if *quantity == Quantity::LENGTH => {
            ScalarDimension::Length(Dimension::Parameter(parameter.clone()))
        }
        Some(ValueKind::Quantity(quantity)) if *quantity == Quantity::ANGLE => {
            ScalarDimension::Angle(Dimension::Parameter(parameter.clone()))
        }
        _ => ScalarDimension::Ratio(Dimension::Parameter(parameter.clone())),
    }
}

fn native(
    kind: &str,
    operands: Vec<Option<Operand<Planar>>>,
    values: Vec<ScalarDimension>,
) -> Result<SketchRelation<Planar>, CodecError> {
    Ok(SketchRelation::Native {
        kind: NonBlankString::new(creo_kind(kind))
            .ok_or_else(|| CodecError::malformed("Creo v11 sketch relation kind is blank"))?,
        operands: operands.into_iter().flatten().collect(),
        values,
    })
}

fn pair(
    points: &mut Points,
    first: &SketchLocus,
    second: &SketchLocus,
) -> Option<(Operand<Planar>, Operand<Planar>)> {
    Some((points.operand(first)?, points.operand(second)?))
}

/// Orders two points so that `b - a` along `axis` is not negative, as a
/// stored magnitude requires.
fn ordered_along(
    points: &mut Points,
    first: &SketchLocus,
    second: &SketchLocus,
    axis: SketchCoordinateAxis,
) -> Option<(Operand<Planar>, Operand<Planar>)> {
    let a = points.point(first)?;
    let b = points.point(second)?;
    let (pa, pb) = (points.position(&a)?, points.position(&b)?);
    let coordinate = |point: FinitePoint2| match axis {
        SketchCoordinateAxis::U => point.get().u,
        SketchCoordinateAxis::V => point.get().v,
    };
    Some(if coordinate(pb) >= coordinate(pa) {
        (Operand::Entity(a), Operand::Entity(b))
    } else {
        (Operand::Entity(b), Operand::Entity(a))
    })
}

/// The v11 relation of one v10 constraint; `None` for a disabled slot.
fn relation(
    input: &Input,
    points: &mut Points,
    kinds: &BTreeMap<ParameterId, ValueKind>,
) -> Result<Option<SketchRelation<Planar>>, CodecError> {
    if matches!(input, Input::Disabled {}) {
        return Ok(None);
    }
    if let Some(relation) =
        geometric_relation(input, points).or_else(|| dimensional_relation(input, points, kinds))
    {
        return Ok(Some(relation));
    }
    native_relation(input, points, kinds).map(Some)
}

/// The v11 relation of a constraint that holds no value, where its operands
/// resolve.
fn geometric_relation(input: &Input, points: &mut Points) -> Option<SketchRelation<Planar>> {
    let entity = |points: &Points, entity: &SketchEntityId| points.entity(entity);
    match input {
        Input::Coincident { entities } => match entities.as_slice() {
            [a, b] => entity(points, a)
                .zip(entity(points, b))
                .map(|(a, b)| SketchRelation::Coincident { a, b }),
            _ => None,
        },
        Input::CoincidentLoci { loci } => match loci.as_slice() {
            [a, b] => pair(points, a, b).map(|(a, b)| SketchRelation::Coincident { a, b }),
            _ => None,
        },
        Input::SameCoordinate { relation } => {
            let (a, b) = (relation.first().clone(), relation.second().clone());
            points
                .point(&a)
                .zip(points.point(&b))
                .map(|(a, b)| match relation.axis() {
                    SketchCoordinateAxis::U => SketchRelation::Vertical {
                        a: Operand::Entity(a),
                        b: Some(Operand::Entity(b)),
                    },
                    SketchCoordinateAxis::V => SketchRelation::Horizontal {
                        a: Operand::Entity(a),
                        b: Some(Operand::Entity(b)),
                    },
                })
        }
        Input::PointOnObject { point, entity: of } => points
            .point(point)
            .zip(entity(points, of))
            .map(|(point, b)| SketchRelation::Coincident {
                a: Operand::Entity(point),
                b,
            }),
        Input::Midpoint { point, entity: of } => points
            .point(point)
            .zip(entity(points, of))
            .map(|(point, of)| SketchRelation::Midpoint { point, of }),
        Input::Concentric { first, second } => entity(points, first)
            .zip(entity(points, second))
            .map(|(a, b)| SketchRelation::Concentric { a, b }),
        Input::Collinear { first, second } => entity(points, first)
            .zip(entity(points, second))
            .map(|(a, b)| SketchRelation::Collinear { a, b }),
        Input::Symmetric {
            first,
            second,
            axis,
        } => pair(points, first, second)
            .zip(entity(points, axis))
            .map(|((a, b), about)| SketchRelation::Symmetric { a, b, about }),
        Input::PointSymmetric {
            first,
            second,
            center,
        } => {
            let about = points.point(center).map(Operand::Entity);
            pair(points, first, second)
                .zip(about)
                .map(|((a, b), about)| SketchRelation::Symmetric { a, b, about })
        }
        Input::Horizontal { entity: of } => {
            entity(points, of).map(|a| SketchRelation::Horizontal { a, b: None })
        }
        Input::Vertical { entity: of } => {
            entity(points, of).map(|a| SketchRelation::Vertical { a, b: None })
        }
        Input::Parallel { first, second } => entity(points, first)
            .zip(entity(points, second))
            .map(|(a, b)| SketchRelation::Parallel { a, b }),
        Input::Perpendicular { first, second } => entity(points, first)
            .zip(entity(points, second))
            .map(|(a, b)| SketchRelation::Perpendicular { a, b }),
        Input::Tangent { first, second } => {
            entity(points, first)
                .zip(entity(points, second))
                .map(|(a, b)| SketchRelation::Tangent {
                    a,
                    b,
                    contact: None,
                })
        }
        Input::TangentLoci { first, second } => match (first, second) {
            (
                SketchLocus::Start(a) | SketchLocus::End(a),
                SketchLocus::Start(b) | SketchLocus::End(b),
            ) => {
                let ends = points.point(first).zip(points.point(second));
                entity(points, a)
                    .zip(entity(points, b))
                    .zip(ends)
                    .map(|((a, b), (p, q))| SketchRelation::Tangent {
                        a,
                        b,
                        contact: Some(Contact::Ends([p, q])),
                    })
            }
            _ => None,
        },
        Input::Curvature { first, second } => entity(points, first)
            .zip(entity(points, second))
            .map(|(a, b)| SketchRelation::Curvature {
                a,
                b,
                contact: None,
            }),
        Input::Equal { first, second } => {
            entity(points, first)
                .zip(entity(points, second))
                .map(|(a, b)| SketchRelation::Equal {
                    members: vec![a, b],
                })
        }
        Input::Fixed { entity: of } => entity(points, of).map(|of| SketchRelation::Fixed { of }),
        _ => None,
    }
}

/// The v11 relation of a constraint that holds a value of the quantity the
/// relation measures, where its operands resolve.
fn dimensional_relation(
    input: &Input,
    points: &mut Points,
    kinds: &BTreeMap<ParameterId, ValueKind>,
) -> Option<SketchRelation<Planar>> {
    let entity = |points: &Points, entity: &SketchEntityId| points.entity(entity);
    match input {
        Input::ArcAngle { entity: of, angle } => entity(points, of).and_then(|_| {
            Some(SketchRelation::Angle {
                between: AngleOperands::Sweep(of.clone()),
                sector: None,
                value: Dimension::Literal(Angle::new(angle.get())?),
            })
        }),
        Input::DistanceLoci {
            first,
            second,
            parameter,
        } if length(kinds, parameter) => {
            pair(points, first, second).map(|(a, b)| SketchRelation::Distance {
                a,
                b,
                along: Some(DistanceAxis::Aligned),
                doubled: None,
                value: Dimension::Parameter(parameter.clone()),
            })
        }
        Input::DistanceLociValue {
            first,
            second,
            distance,
            parameter,
        } if parameter
            .as_ref()
            .is_none_or(|parameter| length(kinds, parameter)) =>
        {
            pair(points, first, second).map(|(a, b)| SketchRelation::Distance {
                a,
                b,
                along: Some(DistanceAxis::Aligned),
                doubled: None,
                value: parameter
                    .clone()
                    .map_or(Dimension::Literal(*distance), Dimension::Parameter),
            })
        }
        Input::HorizontalDistance {
            first,
            second,
            parameter,
        } if length(kinds, parameter) => {
            ordered_along(points, first, second, SketchCoordinateAxis::U).map(|(a, b)| {
                SketchRelation::Distance {
                    a,
                    b,
                    along: Some(DistanceAxis::Horizontal),
                    doubled: None,
                    value: Dimension::Parameter(parameter.clone()),
                }
            })
        }
        Input::VerticalDistance {
            first,
            second,
            parameter,
        } if length(kinds, parameter) => {
            ordered_along(points, first, second, SketchCoordinateAxis::V).map(|(a, b)| {
                SketchRelation::Distance {
                    a,
                    b,
                    along: Some(DistanceAxis::Vertical),
                    doubled: None,
                    value: Dimension::Parameter(parameter.clone()),
                }
            })
        }
        Input::EqualDistance { first, second } => {
            let first = pair(points, &first.first, &first.second);
            let second = pair(points, &second.first, &second.second);
            first
                .zip(second)
                .map(|((a, b), (c, d))| SketchRelation::EqualDistance {
                    first: [a, b],
                    second: [c, d],
                })
        }
        Input::Angle {
            first,
            second,
            parameter,
        } if angle(kinds, parameter) => {
            entity(points, first)
                .zip(entity(points, second))
                .map(|(a, b)| SketchRelation::Angle {
                    between: AngleOperands::Between([a, b]),
                    sector: None,
                    value: Dimension::Parameter(parameter.clone()),
                })
        }
        Input::AngleToAxis {
            entity: of,
            axis,
            parameter,
        } if angle(kinds, parameter) => entity(points, of).map(|line| SketchRelation::Angle {
            between: AngleOperands::Between([
                Operand::Frame(match axis {
                    SketchAxis::Horizontal => PrincipalEntity::XAxis,
                    SketchAxis::Vertical => PrincipalEntity::YAxis,
                }),
                line,
            ]),
            sector: None,
            value: Dimension::Parameter(parameter.clone()),
        }),
        Input::Radius {
            entity: of,
            parameter,
        } if length(kinds, parameter) && circular(points, of) => Some(SketchRelation::Radial {
            of: of.clone(),
            measure: RadialMeasure::Radius,
            value: Dimension::Parameter(parameter.clone()),
        }),
        Input::Diameter {
            entity: of,
            parameter,
        } if length(kinds, parameter) && circular(points, of) => Some(SketchRelation::Radial {
            of: of.clone(),
            measure: RadialMeasure::Diameter,
            value: Dimension::Parameter(parameter.clone()),
        }),
        _ => None,
    }
}

fn circular(points: &Points, entity: &SketchEntityId) -> bool {
    matches!(
        points.curves.get(entity),
        Some(Some(PlanarCurve::Circle { .. } | PlanarCurve::Arc { .. }))
    )
}

/// A constraint that has no v11 relation of the same meaning, or whose
/// operands do not all resolve, as a native relation of its v10 kind.
fn native_relation(
    input: &Input,
    points: &mut Points,
    kinds: &BTreeMap<ParameterId, ValueKind>,
) -> Result<SketchRelation<Planar>, CodecError> {
    let entity = |points: &Points, entity: &SketchEntityId| points.entity(entity);
    let mut values = Vec::new();
    let (kind, operands) = match input {
        Input::Native {
            native_kind,
            entities,
            parameter,
            ..
        } => {
            values.extend(parameter.iter().map(|parameter| value(kinds, parameter)));
            return native(
                native_kind.as_str(),
                entities.iter().map(|e| entity(points, e)).collect(),
                values,
            );
        }
        Input::Coincident { entities } => (
            "coincident",
            entities
                .iter()
                .map(|e| entity(points, e))
                .collect::<Vec<_>>(),
        ),
        Input::CoincidentLoci { loci } => (
            "coincident_loci",
            loci.iter().map(|locus| points.operand(locus)).collect(),
        ),
        Input::Distance {
            entities,
            parameter,
        } => {
            values.push(value(kinds, parameter));
            (
                "distance",
                entities.iter().map(|e| entity(points, e)).collect(),
            )
        }
        Input::DistanceLoci {
            first,
            second,
            parameter,
        }
        | Input::HorizontalDistance {
            first,
            second,
            parameter,
        }
        | Input::VerticalDistance {
            first,
            second,
            parameter,
        } => {
            values.push(value(kinds, parameter));
            (
                input_kind(input),
                vec![points.operand(first), points.operand(second)],
            )
        }
        Input::DistanceLociValue {
            first,
            second,
            distance,
            parameter,
        } => {
            values.push(match parameter {
                Some(parameter) => value(kinds, parameter),
                None => ScalarDimension::Length(Dimension::Literal(*distance)),
            });
            (
                "distance_loci_value",
                vec![points.operand(first), points.operand(second)],
            )
        }
        Input::PolarDistance {
            first,
            second,
            distance,
            angle,
            distance_parameter,
        } => {
            values.push(match distance_parameter {
                Some(parameter) => value(kinds, parameter),
                None => ScalarDimension::Length(Dimension::Literal(*distance)),
            });
            values.extend(angle.map(|angle| ScalarDimension::Angle(Dimension::Literal(angle))));
            (
                "polar_distance",
                vec![points.operand(first), points.operand(second)],
            )
        }
        Input::PointCoordinateValues { point, values: uv } => {
            values.extend(
                uv.iter()
                    .map(|value| ScalarDimension::Length(Dimension::Literal(*value))),
            );
            ("point_coordinate_values", vec![points.operand(point)])
        }
        Input::MidpointCoordinate {
            first,
            second,
            value: mean,
            ..
        } => {
            values.push(ScalarDimension::Length(Dimension::Literal(*mean)));
            (
                input_kind(input),
                vec![points.operand(first), points.operand(second)],
            )
        }
        Input::AtIntersection {
            point,
            first,
            second,
        } => (
            "at_intersection",
            vec![
                points.operand(point),
                entity(points, first),
                entity(points, second),
            ],
        ),
        Input::Coradial { first, second }
        | Input::Concentric { first, second }
        | Input::Collinear { first, second }
        | Input::Parallel { first, second }
        | Input::Perpendicular { first, second }
        | Input::Tangent { first, second }
        | Input::Curvature { first, second }
        | Input::Equal { first, second } => (
            input_kind(input),
            vec![entity(points, first), entity(points, second)],
        ),
        Input::ProjectedCopy { source, result } => (
            "projected_copy",
            vec![entity(points, source), entity(points, result)],
        ),
        Input::Symmetric {
            first,
            second,
            axis,
        } => (
            "symmetric",
            vec![
                points.operand(first),
                points.operand(second),
                entity(points, axis),
            ],
        ),
        Input::PointSymmetric {
            first,
            second,
            center,
        } => (
            "point_symmetric",
            vec![
                points.operand(first),
                points.operand(second),
                points.operand(center),
            ],
        ),
        Input::TangentLoci { first, second } => (
            "tangent_loci",
            vec![points.operand(first), points.operand(second)],
        ),
        Input::SameCoordinate { relation } => (
            "same_coordinate",
            vec![
                points.operand(relation.first()),
                points.operand(relation.second()),
            ],
        ),
        Input::PointOnObject { point, entity: of } | Input::Midpoint { point, entity: of } => (
            input_kind(input),
            vec![points.operand(point), entity(points, of)],
        ),
        Input::Horizontal { entity: of }
        | Input::Vertical { entity: of }
        | Input::Fixed { entity: of } => (input_kind(input), vec![entity(points, of)]),
        Input::ArcAngle { entity: of, angle } | Input::EllipseAngle { entity: of, angle } => {
            values.push(ScalarDimension::Angle(Dimension::Literal(
                Angle::new(angle.get())
                    .ok_or_else(|| CodecError::malformed("Creo sketch angle is not finite"))?,
            )));
            (input_kind(input), vec![entity(points, of)])
        }
        Input::Angle {
            first,
            second,
            parameter,
        } => {
            values.push(value(kinds, parameter));
            ("angle", vec![entity(points, first), entity(points, second)])
        }
        Input::AngleToAxis {
            entity: of,
            axis,
            parameter,
        } => {
            values.push(value(kinds, parameter));
            (
                "angle_to_axis",
                vec![
                    Some(Operand::Frame(match axis {
                        SketchAxis::Horizontal => PrincipalEntity::XAxis,
                        SketchAxis::Vertical => PrincipalEntity::YAxis,
                    })),
                    entity(points, of),
                ],
            )
        }
        Input::Radius {
            entity: of,
            parameter,
        }
        | Input::Diameter {
            entity: of,
            parameter,
        }
        | Input::Weight {
            entity: of,
            parameter,
        } => {
            values.push(value(kinds, parameter));
            (input_kind(input), vec![entity(points, of)])
        }
        Input::RepeatedLength {
            entities,
            parameter,
        }
        | Input::RepeatedRadius {
            entities,
            parameter,
        }
        | Input::RepeatedDiameter {
            entities,
            parameter,
        } => {
            values.push(value(kinds, parameter));
            (
                input_kind(input),
                entities.iter().map(|e| entity(points, e)).collect(),
            )
        }
        Input::SnellsLaw {
            incident,
            refracted,
            interface,
            parameter,
        } => {
            values.push(value(kinds, parameter));
            (
                "snells_law",
                vec![
                    points.operand(incident),
                    points.operand(refracted),
                    entity(points, interface),
                ],
            )
        }
        Input::Group { elements } | Input::Text { elements, .. } => (
            input_kind(input),
            elements.iter().map(|locus| points.operand(locus)).collect(),
        ),
        Input::InternalAlignment { helper, parent, .. } => (
            "internal_alignment",
            vec![entity(points, helper), entity(points, parent)],
        ),
        _ => (input_kind(input), Vec::new()),
    };
    native(kind, operands, values)
}

/// The v10 wire name of a constraint kind.
fn input_kind(input: &Input) -> &'static str {
    match input {
        Input::Disabled {} => "disabled",
        Input::Coincident { .. } => "coincident",
        Input::Polygon { .. } => "polygon",
        Input::SplineGroup { .. } => "spline_group",
        Input::RectangularPattern { .. } => "rectangular_pattern",
        Input::CircularPattern { .. } => "circular_pattern",
        Input::TextFrame { .. } => "text_frame",
        Input::TextPath { .. } => "text_path",
        Input::CoincidentLoci { .. } => "coincident_loci",
        Input::SameCoordinate { .. } => "same_coordinate",
        Input::PointOnObject { .. } => "point_on_object",
        Input::Midpoint { .. } => "midpoint",
        Input::PointCoordinateValues { .. } => "point_coordinate_values",
        Input::MidpointCoordinate { .. } => "midpoint_coordinate",
        Input::Offset { .. } => "offset",
        Input::ProjectedCopy { .. } => "projected_copy",
        Input::AtIntersection { .. } => "at_intersection",
        Input::Concentric { .. } => "concentric",
        Input::Coradial { .. } => "coradial",
        Input::Collinear { .. } => "collinear",
        Input::Symmetric { .. } => "symmetric",
        Input::PointSymmetric { .. } => "point_symmetric",
        Input::Horizontal { .. } => "horizontal",
        Input::Vertical { .. } => "vertical",
        Input::Parallel { .. } => "parallel",
        Input::Perpendicular { .. } => "perpendicular",
        Input::Tangent { .. } => "tangent",
        Input::TangentLoci { .. } => "tangent_loci",
        Input::Curvature { .. } => "curvature",
        Input::Equal { .. } => "equal",
        Input::Fixed { .. } => "fixed",
        Input::ArcAngle { .. } => "arc_angle",
        Input::EllipseAngle { .. } => "ellipse_angle",
        Input::Distance { .. } => "distance",
        Input::DistanceLoci { .. } => "distance_loci",
        Input::DistanceLociValue { .. } => "distance_loci_value",
        Input::PolarDistance { .. } => "polar_distance",
        Input::AngleDifference { .. } => "angle_difference",
        Input::ScalarEquality { .. } => "scalar_equality",
        Input::EqualDistance { .. } => "equal_distance",
        Input::HorizontalDistance { .. } => "horizontal_distance",
        Input::VerticalDistance { .. } => "vertical_distance",
        Input::RepeatedDistance { .. } => "repeated_distance",
        Input::RepeatedLength { .. } => "repeated_length",
        Input::ParallelLineSetDistance { .. } => "parallel_line_set_distance",
        Input::Angle { .. } => "angle",
        Input::AngleToAxis { .. } => "angle_to_axis",
        Input::Radius { .. } => "radius",
        Input::RepeatedRadius { .. } => "repeated_radius",
        Input::Diameter { .. } => "diameter",
        Input::RepeatedDiameter { .. } => "repeated_diameter",
        Input::SnellsLaw { .. } => "snells_law",
        Input::Weight { .. } => "weight",
        Input::InternalAlignment { .. } => "internal_alignment",
        Input::Group { .. } => "group",
        Input::Text { .. } => "text",
        Input::Native { .. } => "native",
    }
}

#[cfg(test)]
mod tests;
