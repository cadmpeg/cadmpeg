// SPDX-License-Identifier: Apache-2.0
//! The planar sketches of a Fusion design (§6.1), restated from the
//! decoder's sketch records. Each sketch is the operation of the feature
//! that owns it. A sketch whose owner has no item of a listing timeline is
//! the operation of a member of one unordered `Group` of its document's
//! scope (§5.2); in a scope with a timeline, that group is interim.
//!
//! An entity with no planar curve states its native kind, and a dimension
//! that binds no parameter of its scope states the value that its parameter
//! record stores; a loss names each.

use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::design::construction::{Construction, ConstructionInput, InputReference};
use cadmpeg_ir::design::container::{Container, UnorderedKind};
use cadmpeg_ir::design::dimension::Dimension;
use cadmpeg_ir::design::evaluation::EvaluationStatus;
use cadmpeg_ir::design::feature::{Feature, FeatureDefinition, FeatureOperation, OperationFamily};
use cadmpeg_ir::design::ids::ContainerId;
use cadmpeg_ir::design::ops::sketch::{
    EntityDefinition, EntityForm, Operand, Planar, Sketch, SketchConstraint, SketchEntity,
    SketchPlane, SketchRelation, SketchSolve,
};
use cadmpeg_ir::design::options::NativeOptions;
use cadmpeg_ir::design::parameter::DesignParameter;
use cadmpeg_ir::design::projection::sketch::{
    self as relation, native_entity_kind, native_relation, planar_curve, stored_frame,
    stored_value, DimensionValue, Loci,
};
use cadmpeg_ir::design::refs::Member;
use cadmpeg_ir::design::selection::{Anchor, Picks, Selection};
use cadmpeg_ir::design::units::ValueKind;
use cadmpeg_ir::document::Model;
use cadmpeg_ir::features::{FeatureId, FeatureOperation as LegacyOperation};
use cadmpeg_ir::ids::IdentityKeyTail;
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::sketches::{SketchGeometryDefinition as Geometry, SketchId, SketchLocus};

use super::timeline;
use crate::loss::F3dLossCode;

/// The restated sketches of a design.
#[derive(Default)]
pub(super) struct Sketches {
    pub(super) records: Vec<Sketch>,
    pub(super) entities: Vec<SketchEntity<Planar>>,
    pub(super) constraints: Vec<SketchConstraint<Planar>>,
}

/// The owners of the sketches of `model` that no feature of `features`
/// owns, keyed by the document prefix of each: the v10 feature whose
/// operation names the sketch, where no feature of `features` has its id,
/// else a feature minted for the sketch. Each owner holds the sketch as its
/// operation.
pub(super) fn outside(
    model: &Model,
    features: &BTreeSet<FeatureId>,
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
        let native = source
            .and_then(|feature| feature.native_ref.as_deref())
            .or(record.native_ref.as_deref());
        let prefix = native
            .and_then(crate::ids::native_stream)
            .map_or("", timeline::document_prefix);
        let id = match source
            .map(|feature| feature.id.clone())
            .filter(|id| !features.contains(id))
        {
            Some(id) => id,
            None => minted(&record.id)?,
        };
        out.entry(prefix.to_owned()).or_default().push(Feature {
            id,
            name: source
                .and_then(|feature| feature.name.clone())
                .or_else(|| record.name.clone()),
            suppressed: source
                .and_then(|feature| feature.suppressed)
                .map(Dimension::Literal),
            definition: FeatureDefinition {
                operation: FeatureOperation::Sketch(record.id.clone()),
                assembly: None,
                refine: None,
                fuzzy_tolerance: None,
            },
            placement: None,
            component: None,
            driven_by: None,
            native_ref: source.and_then(|feature| feature.native_ref.clone()),
        });
    }
    Ok(out)
}

/// Hold `owners`, the owners of the sketches of the document at `prefix`
/// that no timeline item lists, in one root unordered group of `into`. The
/// spec places such a group only in a scope with no timeline, so a group
/// beside a timeline is named as interim.
pub(super) fn hold(
    ctx: &DecodeContext<'_>,
    prefix: &str,
    owners: Vec<Feature>,
    into: &mut timeline::Projected,
    losses: &mut Vec<LossNote>,
) -> Result<(), CodecError> {
    let group = group_id(prefix)?;
    if into
        .containers
        .iter()
        .any(|container| matches!(container, Container::Ordered { .. }))
    {
        losses.push(F3dLossCode::DesignSketchOutsideTimeline.note(format!(
            "container {group}: sketches whose owner has no timeline item are members of one unordered group"
        )));
    }
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

/// The id of the feature minted to own the sketch `sketch`, in the scope of
/// the archive member that the sketch was read from.
fn minted(sketch: &SketchId) -> Result<FeatureId, CodecError> {
    let (key, alone) = timeline::member_scope(sketch.as_str());
    let id = FeatureId::compose(
        &cadmpeg_ir::identity_namespace!("f3d", "design", "sketch-owner"),
        keyed(&alone),
    );
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

/// Restate the sketch of each feature of `features` that owns one: a feature
/// whose operation is a sketch, or an unresolved sketch whose v10 record names
/// a planar sketch. `unordered` holds the features that are members of an
/// unordered container, and `parameters` the design parameters of their
/// scope.
pub(super) fn project(
    model: &Model,
    features: &mut [Feature],
    unordered: &BTreeSet<FeatureId>,
    parameters: &[&DesignParameter],
    out: &mut Sketches,
    losses: &mut Vec<LossNote>,
) {
    let mut restated = out
        .records
        .iter()
        .map(|sketch| sketch.id.clone())
        .collect::<BTreeSet<_>>();
    for feature in features {
        let id = match &feature.definition.operation {
            FeatureOperation::Sketch(id) => Some(id),
            FeatureOperation::Unresolved {
                family: OperationFamily::Sketch,
            } => model
                .features
                .iter()
                .find(|source| source.id == feature.id)
                .and_then(|source| planar_sketch(source.evaluation.definition().operation())),
            _ => None,
        };
        let Some(record) = id
            .filter(|id| !restated.contains(*id))
            .and_then(|id| model.sketches.iter().find(|record| &record.id == id))
        else {
            continue;
        };
        restated.insert(record.id.clone());
        let anchor = if unordered.contains(&feature.id) {
            Anchor::Current
        } else {
            Anchor::Input
        };
        restate(&feature.id, record, model, anchor, parameters, out, losses);
        feature.definition.operation = FeatureOperation::Sketch(record.id.clone());
    }
}

/// Restate `record` as the sketch of the feature `owner`, its plane an
/// unread input anchored at `anchor`.
fn restate(
    owner: &FeatureId,
    record: &cadmpeg_ir::sketches::Sketch,
    model: &Model,
    anchor: Anchor,
    parameters: &[&DesignParameter],
    out: &mut Sketches,
    losses: &mut Vec<LossNote>,
) {
    let entities = model
        .sketch_entities
        .iter()
        .filter(|entity| entity.sketch == record.id)
        .collect::<Vec<_>>();
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
                        "f3d",
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
            roles: Vec::new(),
            construction: entity.construction.then_some(true),
            centerline: None,
            native: None,
            native_options: NativeOptions::new(),
            source_settings: NativeOptions::new(),
            native_ref: entity.native_ref.clone(),
        });
    }
    let points = Points(&entities);
    for constraint in model
        .sketch_constraints
        .iter()
        .filter(|constraint| constraint.sketch == record.id)
    {
        let kind = constraint.definition.kind();
        let (relation, unbound) = relation_of(kind, model, parameters, &points);
        if matches!(&relation, SketchRelation::Coincident { a, b } if a == b) {
            continue;
        }
        if unbound {
            losses.push(F3dLossCode::DesignSketchDimensionUnbound.note(format!(
                "sketch dimension {} binds no design parameter of its scope; it states the value its parameter record stores, or no value",
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
}

/// The relation that states `kind`, and whether it is a dimension that binds
/// no design parameter of `parameters` of its quantity.
fn relation_of(
    kind: &cadmpeg_ir::sketches::SketchConstraintDefinitionInput,
    model: &Model,
    parameters: &[&DesignParameter],
    points: &Points<'_>,
) -> (SketchRelation<Planar>, bool) {
    if let Some(relation) = relation::geometric(kind, points) {
        return (relation, false);
    }
    if !relation::is_dimension(kind) {
        return (native_relation("f3d", kind, Vec::new()), false);
    }
    let Some((parameter, quantity)) = relation::dimension_parameter(kind) else {
        return (native_relation("f3d", kind, Vec::new()), true);
    };
    let bound = parameters.iter().any(|candidate| {
        &candidate.id == parameter && candidate.kind == ValueKind::Quantity(quantity)
    });
    let value = if bound {
        Some(DimensionValue::Parameter(parameter.clone()))
    } else {
        stored_value(&model.parameters, parameter, quantity)
    };
    let relation = value
        .as_ref()
        .and_then(|value| relation::dimension(kind, value, points))
        .unwrap_or_else(|| {
            native_relation(
                "f3d",
                kind,
                value
                    .iter()
                    .filter_map(|value| value.scalar(quantity))
                    .collect(),
            )
        });
    (relation, !bound)
}

/// The entities of one sketch. The decoder states each point a relation
/// names as an entity of its own.
struct Points<'a>(&'a [&'a cadmpeg_ir::sketches::SketchEntity]);

impl Loci for Points<'_> {
    fn operand(&self, locus: &SketchLocus) -> Option<Operand<Planar>> {
        match locus {
            SketchLocus::Entity(id) => Some(Operand::Entity(id.clone())),
            SketchLocus::Start(_) | SketchLocus::End(_) | SketchLocus::Center(_) => None,
        }
    }

    fn position(&self, locus: &SketchLocus) -> Option<[f64; 2]> {
        let SketchLocus::Entity(id) = locus else {
            return None;
        };
        let entity = self.0.iter().find(|entity| entity.id() == id)?;
        match entity.geometry.definition() {
            Geometry::Point { position } => Some([position.u, position.v]),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests;
