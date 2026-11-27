// SPDX-License-Identifier: Apache-2.0
//! The planar and spatial sketches of a Fusion design (§6.1), restated from
//! the decoder's sketch records. Each sketch is the operation of the feature
//! that owns it. A sketch whose owner has no item of a listing timeline is
//! the operation of a member of one unordered `Group` of its document's
//! scope (§5.2), and so is each construction plane, axis, and point of a
//! document whose streams list no timeline item.
//!
//! A point entity that a curve's record names as its start, end, or center,
//! and that lies there, holds that role on the curve. A relation on such a
//! point of a curve names that point entity, else a point entity minted
//! there. A dimension that binds no parameter of its scope states the value
//! that its parameter record stores, and a reference dimension binds a
//! `Driven` parameter minted for it. An entity with no curve states its
//! native kind; a loss names each.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::text::NonBlankString;
use cadmpeg_core::CodecError;
use cadmpeg_ir::design::construction::{Construction, ConstructionInput, InputReference};
use cadmpeg_ir::design::container::{Container, UnorderedKind};
use cadmpeg_ir::design::dimension::Dimension;
use cadmpeg_ir::design::evaluation::EvaluationStatus;
use cadmpeg_ir::design::feature::{Feature, FeatureDefinition, FeatureOperation, OperationFamily};
use cadmpeg_ir::design::ids::ContainerId;
use cadmpeg_ir::design::ops::sketch::{
    EntityDefinition, EntityForm, Operand, Planar, Sketch, SketchConstraint, SketchEntity,
    SketchPlane, SketchRelation, SketchSolve, Spatial, SpatialSketch,
};
use cadmpeg_ir::design::options::NativeOptions;
use cadmpeg_ir::design::parameter::DesignParameter;
use cadmpeg_ir::design::projection::sketch::{
    self as relation, native_entity_kind, native_relation, planar_curve, stored_frame,
    stored_quantity, stored_value, DimensionValue, Loci, LocusPoints, StoredPoints,
};
use cadmpeg_ir::design::projection::spatial_sketch;
use cadmpeg_ir::design::refs::Member;
use cadmpeg_ir::design::selection::{Anchor, Picks, Selection};
use cadmpeg_ir::design::units::{Quantity, ValueKind};
use cadmpeg_ir::document::Model;
use cadmpeg_ir::features::{
    Feature as LegacyFeature, FeatureId, FeatureOperation as LegacyOperation, ParameterId,
};
use cadmpeg_ir::ids::{IdentityKeyTail, ProductDefinitionId};
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::sketches::{
    SketchGeometryDefinition as Geometry, SketchId, SketchLocus, SpatialSketchId,
};

use super::timeline;
use crate::loss::F3dLossCode;

/// The restated sketches of a design.
#[derive(Default)]
pub(super) struct Sketches {
    pub(super) records: Vec<Sketch>,
    pub(super) entities: Vec<SketchEntity<Planar>>,
    pub(super) constraints: Vec<SketchConstraint<Planar>>,
    pub(super) spatial: Vec<SpatialSketch>,
    pub(super) spatial_entities: Vec<SketchEntity<Spatial>>,
    pub(super) spatial_constraints: Vec<SketchConstraint<Spatial>>,
    /// The `Driven` parameters minted for reference dimensions.
    pub(super) parameters: Vec<DesignParameter>,
}

/// The members of `model` that no item of a listing timeline lists, keyed
/// by the document prefix of each: the owners of the planar and spatial
/// sketches that no feature of `features` owns, and, in a document that is
/// not `listing`, one of the prefixes of the documents whose streams list an
/// item, its construction planes, axes, and points. A sketch's
/// owner is the v10 feature whose operation names the sketch, where no
/// feature of `features` has its id, else a feature minted for the sketch,
/// and holds the sketch as its operation.
pub(super) fn outside(
    model: &Model,
    features: &BTreeSet<FeatureId>,
    listing: &BTreeSet<String>,
    losses: &mut Vec<LossNote>,
) -> Result<BTreeMap<String, Vec<Feature>>, CodecError> {
    let owned = model
        .features
        .iter()
        .filter(|feature| features.contains(&feature.id))
        .filter_map(|feature| planar_sketch(feature.evaluation.definition().operation()))
        .collect::<BTreeSet<_>>();
    let mut out = BTreeMap::<String, Vec<Feature>>::new();
    for record in &model.sketches {
        if owned.contains(&record.id) {
            continue;
        }
        let source = model.features.iter().find(|feature| {
            planar_sketch(feature.evaluation.definition().operation()) == Some(&record.id)
        });
        let id = match source
            .map(|feature| feature.id.clone())
            .filter(|id| !features.contains(id))
        {
            Some(id) => id,
            None => minted(record.id.as_str(), false)?,
        };
        out.entry(prefix(source, record.native_ref.as_deref()))
            .or_default()
            .push(owner(
                id,
                source,
                record.name.as_ref(),
                FeatureOperation::Sketch(record.id.clone()),
            ));
    }
    let owned = model
        .features
        .iter()
        .filter(|feature| features.contains(&feature.id))
        .filter_map(|feature| spatial_sketch(feature.evaluation.definition().operation()))
        .collect::<BTreeSet<_>>();
    for record in &model.spatial_sketches {
        if owned.contains(&record.id) {
            continue;
        }
        let source = model.features.iter().find(|feature| {
            spatial_sketch(feature.evaluation.definition().operation()) == Some(&record.id)
        });
        let id = match source
            .map(|feature| feature.id.clone())
            .filter(|id| !features.contains(id))
        {
            Some(id) => id,
            None => minted(record.id.as_str(), true)?,
        };
        out.entry(prefix(source, record.native_ref.as_deref()))
            .or_default()
            .push(owner(
                id,
                source,
                record.name.as_ref(),
                FeatureOperation::SpatialSketch(record.id.clone()),
            ));
    }
    for feature in model.features.iter().filter(|feature| {
        !features.contains(&feature.id) && construction(feature.evaluation.definition().operation())
    }) {
        let document = prefix(Some(feature), None);
        if listing.contains(&document) {
            continue;
        }
        let operation =
            super::datum::project(feature, losses).unwrap_or(FeatureOperation::Unresolved {
                family: OperationFamily::Datum,
            });
        out.entry(document).or_default().push(owner(
            feature.id.clone(),
            Some(feature),
            None,
            operation,
        ));
    }
    Ok(out)
}

/// The document prefix of the member that `source`, else the record at
/// `native`, was read from.
fn prefix(source: Option<&LegacyFeature>, native: Option<&str>) -> String {
    source
        .and_then(|feature| feature.native_ref.as_deref())
        .or(native)
        .and_then(crate::ids::native_stream)
        .map_or("", timeline::document_prefix)
        .to_owned()
}

/// The member `id` with `operation`, named and suppressed as the v10
/// feature `source` is, else named `name`.
fn owner(
    id: FeatureId,
    source: Option<&LegacyFeature>,
    name: Option<&String>,
    operation: FeatureOperation,
) -> Feature {
    Feature {
        id,
        name: source
            .and_then(|feature| feature.name.clone())
            .or_else(|| name.cloned()),
        suppressed: source
            .and_then(|feature| feature.suppressed)
            .map(Dimension::Literal),
        definition: FeatureDefinition {
            operation,
            assembly: None,
            refine: None,
            fuzzy_tolerance: None,
        },
        placement: None,
        component: None,
        driven_by: None,
        native_ref: source.and_then(|feature| feature.native_ref.clone()),
    }
}

/// Whether a v10 operation is a construction plane, axis, point, or
/// coordinate system other than an origin plane.
pub(super) fn construction(operation: &LegacyOperation) -> bool {
    !matches!(operation, LegacyOperation::DatumPrincipalPlane { .. })
        && matches!(
            super::operation::family(operation),
            super::operation::Family::Typed(OperationFamily::Datum)
        )
}

/// Hold `owners`, the members of the document at `prefix` that no timeline
/// item lists, in one root unordered group of `into` (§5.2).
pub(super) fn hold(
    ctx: &DecodeContext<'_>,
    prefix: &str,
    owners: Vec<Feature>,
    into: &mut timeline::Projected,
) -> Result<(), CodecError> {
    let group = group_id(prefix)?;
    ctx.charge_entities(1, "project F3D v11 sketch group")?;
    into.containers.push(Container::Unordered {
        id: group,
        kind: UnorderedKind::Group,
        members: owners
            .iter()
            .map(|owner| Member::Feature(owner.id.clone()))
            .collect(),
    });
    into.features.extend(owners);
    Ok(())
}

/// The container of the sketches outside the timelines of the document at
/// `prefix`.
fn group_id(prefix: &str) -> Result<ContainerId, CodecError> {
    let id = ContainerId::from(cadmpeg_ir::ids::Identity::compose(
        &cadmpeg_ir::identity_namespace!("f3d", "design", "container"),
        cadmpeg_ir::identity_key!("sketches"),
    ));
    match prefix.strip_suffix('@') {
        Some(key) => {
            ContainerId::mint(timeline::qualified(key, id.as_str())?).map_err(CodecError::malformed)
        }
        None => Ok(id),
    }
}

/// The id of the feature minted to own the sketch `sketch`, a spatial
/// sketch where `spatial`, in the scope of the archive member that the
/// sketch was read from.
fn minted(sketch: &str, spatial: bool) -> Result<FeatureId, CodecError> {
    let (key, alone) = timeline::member_scope(sketch);
    let namespace = if spatial {
        cadmpeg_ir::identity_namespace!("f3d", "design", "spatial-sketch-owner")
    } else {
        cadmpeg_ir::identity_namespace!("f3d", "design", "sketch-owner")
    };
    let id = FeatureId::compose(&namespace, keyed(&alone));
    match key {
        Some(key) => {
            FeatureId::mint(timeline::qualified(key, id.as_str())?).map_err(CodecError::malformed)
        }
        None => Ok(id),
    }
}

/// The identity key of `text`: its length and its percent-encoded form.
fn keyed(text: &str) -> cadmpeg_ir::ids::IdentityKey {
    cadmpeg_ir::ids::IdentityKey::from(text.len())
        .then(cadmpeg_ir::identity_key!(":"))
        .with_tail(&IdentityKeyTail::percent_encode(text))
}

/// The planar sketch that a v10 operation names.
fn planar_sketch(operation: &LegacyOperation) -> Option<&SketchId> {
    match operation {
        LegacyOperation::Sketch { sketch } => sketch.id(),
        _ => None,
    }
}

/// The spatial sketch that a v10 operation names.
fn spatial_sketch(operation: &LegacyOperation) -> Option<&SpatialSketchId> {
    match operation {
        LegacyOperation::SpatialSketch { sketch } => sketch.as_ref(),
        _ => None,
    }
}

/// What a sketch's relations read of its scope.
pub(super) struct Scope<'a> {
    pub(super) ctx: &'a DecodeContext<'a>,
    pub(super) model: &'a Model,
    /// The point records each curve record names.
    pub(super) incidence: &'a Incidence,
    /// The document's linear tolerance, in millimetres.
    pub(super) linear: f64,
    /// The definition whose design holds the scope.
    pub(super) definition: &'a ProductDefinitionId,
    /// The features that are members of an unordered container.
    pub(super) unordered: &'a BTreeSet<FeatureId>,
    /// The design parameters of the scope.
    pub(super) parameters: &'a [&'a DesignParameter],
}

/// The identity and record index of a sketch curve record.
#[derive(serde::Deserialize)]
pub(super) struct CurveRecord {
    id: String,
    record_index: u32,
}

/// A sketch point record and the curve records that name it.
#[derive(serde::Deserialize)]
pub(super) struct PointRecord {
    id: String,
    #[serde(default)]
    companion: PointCompanion,
}

#[derive(Default, serde::Deserialize)]
struct PointCompanion {
    #[serde(default)]
    incident_curves: Vec<u32>,
}

/// The point records that each curve record names as its start, end, or
/// center, read from the reverse incidence each point record stores.
#[derive(Default)]
pub(super) struct Incidence {
    points: HashMap<String, Vec<String>>,
}

impl Incidence {
    pub(super) fn new(curves: &[CurveRecord], points: &[PointRecord]) -> Self {
        let curve_ids = curves
            .iter()
            .filter_map(|curve| {
                Some((
                    (crate::ids::native_stream(&curve.id)?, curve.record_index),
                    curve.id.as_str(),
                ))
            })
            .collect::<HashMap<_, _>>();
        let mut out = Self::default();
        for point in points {
            let Some(stream) = crate::ids::native_stream(&point.id) else {
                continue;
            };
            for record in &point.companion.incident_curves {
                if let Some(curve) = curve_ids.get(&(stream, *record)) {
                    out.points
                        .entry((*curve).to_owned())
                        .or_default()
                        .push(point.id.clone());
                }
            }
        }
        out
    }

    /// Whether `curve` names `point`: the curve's record names the point's
    /// record, or no point record names the curve's record.
    fn names(
        &self,
        curve: &cadmpeg_ir::sketches::SketchEntity,
        point: &cadmpeg_ir::sketches::SketchEntity,
    ) -> bool {
        match curve
            .native_ref
            .as_deref()
            .and_then(|curve| self.points.get(curve))
        {
            Some(points) => point
                .native_ref
                .as_ref()
                .is_some_and(|point| points.contains(point)),
            None => true,
        }
    }
}

/// Restate the sketch of each feature of `features` that owns one: a feature
/// whose operation is a sketch, or an unresolved sketch whose v10 record names
/// a planar or a spatial sketch.
pub(super) fn project(
    scope: &Scope<'_>,
    features: &mut [Feature],
    out: &mut Sketches,
    losses: &mut Vec<LossNote>,
) -> Result<(), cadmpeg_core::decode::ResourceLimit> {
    let model = scope.model;
    let mut restated = out
        .records
        .iter()
        .map(|sketch| sketch.id.clone())
        .collect::<BTreeSet<_>>();
    let mut spatial = out
        .spatial
        .iter()
        .map(|sketch| sketch.id.clone())
        .collect::<BTreeSet<_>>();
    for feature in features {
        let source = || {
            model
                .features
                .iter()
                .find(|source| source.id == feature.id)
                .map(|source| source.evaluation.definition().operation())
        };
        let (planar, space) = match &feature.definition.operation {
            FeatureOperation::Sketch(id) => (Some(id.clone()), None),
            FeatureOperation::SpatialSketch(id) => (None, Some(id.clone())),
            FeatureOperation::Unresolved {
                family: OperationFamily::Sketch,
            } => (source().and_then(planar_sketch).cloned(), None),
            FeatureOperation::Unresolved {
                family: OperationFamily::SpatialSketch,
            } => (None, source().and_then(spatial_sketch).cloned()),
            _ => (None, None),
        };
        if let Some(record) = space
            .filter(|id| !spatial.contains(id))
            .and_then(|id| model.spatial_sketches.iter().find(|record| record.id == id))
        {
            spatial.insert(record.id.clone());
            restate_spatial(&feature.id, record, scope, out, losses);
            feature.definition.operation = FeatureOperation::SpatialSketch(record.id.clone());
            continue;
        }
        let Some(record) = planar
            .filter(|id| !restated.contains(id))
            .and_then(|id| model.sketches.iter().find(|record| record.id == id))
        else {
            continue;
        };
        restated.insert(record.id.clone());
        let anchor = if scope.unordered.contains(&feature.id) {
            Anchor::Current
        } else {
            Anchor::Input
        };
        restate(&feature.id, record, scope, anchor, out, losses)?;
        feature.definition.operation = FeatureOperation::Sketch(record.id.clone());
    }
    Ok(())
}

/// Restate the spatial sketch `record` as the sketch of `owner`. A dimension
/// binds the design parameter of the scope that has its id and quantity.
fn restate_spatial(
    owner: &FeatureId,
    record: &cadmpeg_ir::sketches::SpatialSketch,
    scope: &Scope<'_>,
    out: &mut Sketches,
    losses: &mut Vec<LossNote>,
) {
    let restated = spatial_sketch::restate(
        &cadmpeg_core::nonblank_literal!("f3d"),
        owner,
        record,
        scope.model,
        |parameter, quantity| {
            scope.parameters.iter().any(|candidate| {
                &candidate.id == parameter && candidate.kind == ValueKind::Quantity(quantity)
            })
        },
    );
    for entity in &restated.native_entities {
        losses.push(F3dLossCode::DesignSketchEntityNative.note(format!(
            "spatial sketch entity {entity} has no spatial curve; it states its native kind"
        )));
    }
    for constraint in &restated.unbound {
        losses.push(F3dLossCode::DesignSketchDimensionUnbound.note(format!(
            "spatial sketch dimension {constraint} binds no design parameter of its scope; it states the value its parameter record stores, or no value"
        )));
    }
    for constraint in &restated.native_relations {
        losses.push(F3dLossCode::DesignSketchRelationNative.note(format!(
            "spatial sketch relation {constraint} has no typed relation of its operands and value; it states its native kind"
        )));
    }
    out.spatial.push(restated.sketch);
    out.spatial_entities.extend(restated.entities);
    out.spatial_constraints.extend(restated.constraints);
}

/// Restate `record` as the sketch of the feature `owner`, its plane an
/// unread input anchored at `anchor`.
fn restate(
    owner: &FeatureId,
    record: &cadmpeg_ir::sketches::Sketch,
    scope: &Scope<'_>,
    anchor: Anchor,
    out: &mut Sketches,
    losses: &mut Vec<LossNote>,
) -> Result<(), cadmpeg_core::decode::ResourceLimit> {
    let model = scope.model;
    let entities = model
        .sketch_entities
        .iter()
        .filter(|entity| entity.sketch == record.id)
        .collect::<Vec<_>>();
    let stored = StoredPoints::new(
        scope.ctx,
        &entities,
        |curve, _, point| scope.incidence.names(curve, point),
        |a, b| (a[0] - b[0]).hypot(a[1] - b[1]) <= scope.linear,
    )?;
    for entity in &entities {
        let definition = match planar_curve(&entity.geometry) {
            Some(form) => EntityDefinition::Free(form),
            None => {
                losses.push(F3dLossCode::DesignSketchEntityNative.note(format!(
                    "sketch entity {} has no planar curve; it states its native kind",
                    entity.id()
                )));
                EntityDefinition::Defined {
                    form: Box::new(EntityForm::Native(native_entity_kind(
                        &cadmpeg_core::nonblank_literal!("f3d"),
                        &entity.geometry,
                    ))),
                    solved: None,
                }
            }
        };
        out.entities.push(SketchEntity {
            id: entity.id().clone(),
            sketch: record.id.clone(),
            definition,
            roles: stored.roles(entity.id()),
            construction: entity.construction.then_some(true),
            centerline: None,
            native: None,
            native_options: NativeOptions::new(),
            source_settings: NativeOptions::new(),
            native_ref: entity.native_ref.clone(),
        });
    }
    let constraints = model
        .sketch_constraints
        .iter()
        .filter(|constraint| constraint.sketch == record.id)
        .collect::<Vec<_>>();
    let loci = LocusPoints::new(
        scope.ctx,
        constraints
            .iter()
            .map(|constraint| constraint.definition.kind()),
        &entities,
        &stored,
    )?;
    out.entities.extend(loci.entities(&record.id));
    let points = Points {
        entities: &entities,
        loci: &loci,
    };
    for constraint in constraints {
        let kind = constraint.definition.kind();
        let (relation, unbound) = relation_of(constraint, owner, scope, &points, out);
        if matches!(&relation, SketchRelation::Coincident { a, b } if a == b) {
            continue;
        }
        if unbound {
            losses.push(F3dLossCode::DesignSketchDimensionUnbound.note(format!(
                "sketch dimension {} binds no design parameter of its scope; it states the value its parameter record stores, or no value",
                constraint.id
            )));
        }
        if loci.unplaced(kind) {
            losses.push(F3dLossCode::DesignSketchLocusUnplaced.note(format!(
                "sketch relation {} names an end of a closed curve, the center of a curve that has none, or a point whose minted id another entity holds; it states the curve",
                constraint.id
            )));
        }
        out.constraints.push(SketchConstraint {
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
    out.records.push(Sketch {
        id: record.id.clone(),
        owner: owner.clone(),
        name: record.name.clone(),
        plane: SketchPlane::Placed(Box::new(Construction {
            inputs: vec![ConstructionInput {
                role: None,
                reference: Some(InputReference::Picks(Selection {
                    at: anchor,
                    picks: Picks::Unread,
                    native: None,
                })),
                value: None,
                reversed: None,
            }],
            resolved: stored_frame(record.placement),
            after: Vec::new(),
            solution: None,
            mode: None,
            resolved_in: BTreeMap::new(),
        })),
        solve: SketchSolve {
            state: None,
            status: EvaluationStatus::Current,
        },
        native_options: NativeOptions::new(),
        source_settings: NativeOptions::new(),
        native_ref: record.native_ref.clone(),
    });
    Ok(())
}

/// The relation that states `constraint` of the sketch of `owner`, and
/// whether it is a dimension that binds no design parameter of the scope.
/// A native relation that names a parameter measures the quantity of that
/// design parameter, else of the value its parameter record stores. A
/// reference dimension whose parameter is no design parameter of the scope
/// binds a `Driven` parameter minted for it, with the value its parameter
/// record stores; one that cannot states no value.
fn relation_of(
    constraint: &cadmpeg_ir::sketches::SketchConstraint,
    owner: &FeatureId,
    scope: &Scope<'_>,
    points: &Points<'_>,
    out: &mut Sketches,
) -> (SketchRelation<Planar>, bool) {
    let model = scope.model;
    let kind = constraint.definition.kind();
    let codec = cadmpeg_core::nonblank_literal!("f3d");
    if let Some(relation) = relation::geometric(kind, points) {
        return (relation, false);
    }
    if !relation::is_dimension(kind) {
        return (native_relation(&codec, kind, Vec::new(), points), false);
    }
    let Some((parameter, quantity)) = relation::dimension_parameter(kind, |_, parameter| {
        scope
            .parameters
            .iter()
            .find(|candidate| &candidate.id == parameter)
            .and_then(|candidate| match candidate.kind {
                ValueKind::Quantity(quantity) => Some(quantity),
                _ => None,
            })
            .or_else(|| stored_quantity(&model.parameters, parameter))
    }) else {
        return (native_relation(&codec, kind, Vec::new(), points), true);
    };
    let reference = constraint.driving == Some(false);
    let bound = scope.parameters.iter().any(|candidate| {
        &candidate.id == parameter && candidate.kind == ValueKind::Quantity(quantity)
    }) || out.parameters.iter().any(|minted| &minted.id == parameter)
        || (reference && mint(parameter, quantity, owner, scope, out));
    let value = if bound {
        Some(DimensionValue::Parameter(parameter.clone()))
    } else if reference {
        None
    } else {
        stored_value(&model.parameters, parameter, quantity)
    };
    let relation = value
        .as_ref()
        .and_then(|value| relation::dimension(kind, value, points))
        .unwrap_or_else(|| {
            native_relation(
                &codec,
                kind,
                value
                    .iter()
                    .filter_map(|value| value.scalar(quantity))
                    .collect(),
                points,
            )
        });
    (relation, !bound)
}

/// Mint the `Driven` parameter `parameter` of `quantity` for a reference
/// dimension of the sketch of `owner`, where no design parameter of the
/// scope has its id. It has the name of its parameter record where no
/// parameter of the scope has that name, and the value the record stores
/// where it is one of `quantity`.
fn mint(
    parameter: &ParameterId,
    quantity: Quantity,
    owner: &FeatureId,
    scope: &Scope<'_>,
    out: &mut Sketches,
) -> bool {
    if scope
        .parameters
        .iter()
        .any(|candidate| &candidate.id == parameter)
    {
        return false;
    }
    let source = scope
        .model
        .parameters
        .iter()
        .find(|candidate| &candidate.id == parameter);
    let name = source
        .and_then(|source| NonBlankString::new(source.name.clone()))
        .filter(|name| {
            scope
                .parameters
                .iter()
                .copied()
                .chain(&out.parameters)
                .all(|known| known.name.as_ref() != Some(name))
        });
    out.parameters.push(relation::driven_parameter(
        &scope.model.parameters,
        parameter,
        quantity,
        scope.definition,
        owner,
        name,
    ));
    true
}

/// The entities of one sketch and the points its relations name at a
/// curve's start, end, or center.
struct Points<'a> {
    entities: &'a [&'a cadmpeg_ir::sketches::SketchEntity],
    loci: &'a LocusPoints,
}

impl Loci for Points<'_> {
    fn operand(&self, locus: &SketchLocus) -> Option<Operand<Planar>> {
        match locus {
            SketchLocus::Entity(id) => Some(Operand::Entity(id.clone())),
            SketchLocus::Start(_) | SketchLocus::End(_) | SketchLocus::Center(_) => {
                self.loci.operand(locus)
            }
        }
    }

    fn position(&self, locus: &SketchLocus) -> Option<[f64; 2]> {
        let SketchLocus::Entity(id) = locus else {
            return self.loci.position(locus);
        };
        let entity = self.entities.iter().find(|entity| entity.id() == id)?;
        match entity.geometry.definition() {
            Geometry::Point { position } => Some([position.u, position.v]),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests;
