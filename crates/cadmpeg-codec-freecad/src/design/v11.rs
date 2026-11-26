// SPDX-License-Identifier: Apache-2.0
//! The v11 design layer of a `FreeCAD` document.
//!
//! The document is one scope at its root definition. Each `PartDesign::Body`
//! is an ordered `Body` container, a root of the scope, whose `Tip` is its
//! rollback, and each of its members is one feature, except the stages a
//! `MultiTransform` lists, which are its layout (§6.5). Each design object
//! outside every Body is one feature, a member of the scope's root unordered
//! `Group` (§5.2), whose own result is its current state (§5.3). A document
//! with no design object and no parameter has no scope (§5.1).

use std::collections::{BTreeMap, BTreeSet, HashMap};

use cadmpeg_ir::design::construction::{kinds, Construction, FrameValue, PostStep};
use cadmpeg_ir::design::container::{Container, OrderedKind, Rollback, UnorderedKind};
use cadmpeg_ir::design::dimension::Dimension;
use cadmpeg_ir::design::evaluation::{CurrentState, EvaluationStatus};
use cadmpeg_ir::design::feature::{
    Feature, FeatureDefinition, FeatureOperation, FuzzyTolerance, OperationFamily, StoredOrigin,
};
use cadmpeg_ir::design::ids::ContainerId;
use cadmpeg_ir::design::names::CodecName;
use cadmpeg_ir::design::refs::{Member, Position};
use cadmpeg_ir::design::scope::DesignScope;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::features::geometry::FinitePoint3;
use cadmpeg_ir::features::operation_settings::FuzzyTolerance as LegacyFuzzyTolerance;
use cadmpeg_ir::features::tree::FeatureTreeNodeRole;
use cadmpeg_ir::features::{
    Feature as LegacyFeature, FeatureDefinition as LegacyDefinition, FeatureId,
    FeatureOperation as LegacyOperation, UnresolvedFamily,
};
use cadmpeg_ir::ids::{BodyId, Identity, ProductDefinitionId};
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::products::{ProductDefinition, Storage};
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::transform::Transform;
use cadmpeg_ir::units::UnitVector3;

use crate::native::frame::FiniteFrame;
use crate::native::{ObjectRecord, PropertyRecord};

mod parameters;
pub(crate) mod states;

/// Project the decoded design of `ir` into its v11 design layer, a scope of
/// the root definition `definition`. A record the projection cannot state in
/// full is stated as far as it is read, with a loss in `losses` (§3.3). The
/// scope's `unproduced` bodies are stated once the root owns its bodies
/// ([`state_unproduced`]).
pub(crate) fn project(
    ir: &mut CadIr,
    definition: ProductDefinitionId,
    objects: &[ObjectRecord],
    properties: &[PropertyRecord],
    losses: &mut Vec<LossNote>,
) {
    let source = Source::of(objects, properties);
    let history = History::of(&ir.model.features, &source);
    let mut features = history
        .features
        .values()
        .map(|legacy| feature(legacy, &source))
        .collect::<Vec<_>>();
    let grouped = history
        .features
        .keys()
        .filter(|id| !history.contained.contains(**id))
        .map(|id| Member::Feature((*id).clone()))
        .collect::<BTreeSet<_>>();
    let mut containers = Vec::new();
    let mut body_containers = BTreeMap::new();
    let mut roots = Vec::new();
    let mut current = Vec::new();
    for body in &history.bodies {
        let id = ContainerId::from(
            Identity::from(body.feature.clone())
                .with_kind(&cadmpeg_ir::identity_component!("container")),
        );
        let members = body
            .members
            .iter()
            .cloned()
            .map(Member::Feature)
            .collect::<Vec<_>>();
        // A stored empty `Tip` builds no member; a `Tip` that names no member
        // of the Body fixes no rollback (cadir-v11.md §5.2), and the state at
        // it is spelled `After(Container(body))`.
        let (rollback, at) = match &body.tip {
            None => (
                Some(
                    members
                        .first()
                        .map_or(Rollback::End, |first| Rollback::Before(first.clone())),
                ),
                Position::Start(id.clone()),
            ),
            Some(tip) => match body.members.iter().position(|member| member == tip) {
                Some(index) => (
                    Some(
                        members
                            .get(index + 1)
                            .map_or(Rollback::End, |next| Rollback::Before(next.clone())),
                    ),
                    Position::After(members[index].clone()),
                ),
                None => (None, Position::After(Member::Container(id.clone()))),
            },
        };
        current.push(CurrentState {
            at,
            bodies: body.outputs.clone(),
            poses: BTreeMap::default(),
            status: body
                .native_ref
                .as_deref()
                .map_or(EvaluationStatus::Current, |object| source.status(object)),
            native_ref: body.native_ref.clone(),
        });
        roots.push(id.clone());
        body_containers.insert(body.feature.clone(), id.clone());
        containers.push(Container::Ordered {
            id,
            kind: OrderedKind::Body,
            members,
            folders: Vec::new(),
            rollback,
            placement: body
                .native_ref
                .as_deref()
                .and_then(|object| source.placements.get(object))
                .and_then(|property| crate::placement::placement_matrix_unreported(property))
                .and_then(placement)
                .map(Box::new),
        });
    }
    if !grouped.is_empty() {
        let id = ContainerId::from(Identity::compose(
            &cadmpeg_ir::identity_namespace!("fcstd", "design", "container"),
            cadmpeg_ir::identity_key!("document:group"),
        ));
        roots.push(id.clone());
        containers.push(Container::Unordered {
            id,
            kind: UnorderedKind::Group,
            members: grouped.clone(),
        });
    }
    containers.sort_by(|a, b| a.id().cmp(b.id()));
    features.sort_by(|a, b| a.id.cmp(&b.id));
    let carriers = properties
        .iter()
        .map(|property| (property.id.as_str(), property.type_name.as_str()))
        .collect::<HashMap<_, _>>();
    let parameters = parameters::project(
        ir,
        &definition,
        &body_containers,
        &features,
        &carriers,
        losses,
    );
    if features.is_empty() && parameters.is_empty() {
        return;
    }
    ir.design.containers = containers;
    ir.design.features = features;
    ir.design.parameters = parameters;
    // A member of the Group that changes state has its own result, the
    // bodies of its shape (§5.3).
    for member in grouped {
        let Member::Feature(id) = &member else {
            continue;
        };
        let legacy = history.features[id];
        let bodies = legacy.evaluation.outputs();
        if bodies.is_empty() || ir.is_stateless(id, None).unwrap_or(true) {
            continue;
        }
        current.push(CurrentState {
            at: Position::After(member.clone()),
            bodies: bodies.clone(),
            poses: BTreeMap::default(),
            status: legacy
                .native_ref
                .as_deref()
                .map_or(EvaluationStatus::Current, |object| source.status(object)),
            native_ref: legacy.native_ref.clone(),
        });
    }
    ir.design.scopes.push(DesignScope {
        definition,
        roots,
        active_configuration: None,
        active_arrangements: BTreeSet::default(),
        occurrences: BTreeMap::default(),
        current,
        unproduced: BTreeSet::new(),
    });
}

/// List as unproduced each body that a definition the scope spans owns and
/// no current state holds (§5.5): the shape of an object outside every Body,
/// whose producing member the document does not hold, and of a Body that
/// states no container.
pub(crate) fn state_unproduced(ir: &mut CadIr) {
    let Some(scope) = ir.design.scopes.first() else {
        return;
    };
    let spanned = spanned_definitions(
        &ir.model.product_definitions,
        &ir.model.occurrences,
        &scope.definition,
    );
    let current = scope
        .current
        .iter()
        .flat_map(|state| &state.bodies)
        .collect::<BTreeSet<_>>();
    let bodies = ir
        .geometry
        .bodies
        .iter()
        .map(|body| &body.id)
        .collect::<BTreeSet<_>>();
    let unproduced = ir
        .model
        .product_definitions
        .iter()
        .filter(|definition| spanned.contains(&definition.id))
        .flat_map(|definition| {
            definition.bodies.iter().chain(
                definition
                    .instance_bodies
                    .iter()
                    .flat_map(|instance| &instance.bodies),
            )
        })
        .filter(|body| bodies.contains(body) && !current.contains(body))
        .cloned()
        .collect::<BTreeSet<_>>();
    ir.design.scopes[0].unproduced = unproduced;
}

/// The root definition `root` and the definitions its scope spans (§5.1):
/// each `Embedded` definition that no occurrence places, and each `Embedded`
/// definition an occurrence of a spanned definition places.
fn spanned_definitions<'a>(
    definitions: &'a [ProductDefinition],
    occurrences: &[cadmpeg_ir::products::Occurrence],
    root: &'a ProductDefinitionId,
) -> BTreeSet<&'a ProductDefinitionId> {
    let embedded = |definition: &ProductDefinition| matches!(definition.storage, Storage::Embedded);
    let placed = occurrences
        .iter()
        .map(|occurrence| &occurrence.prototype)
        .collect::<BTreeSet<_>>();
    let prototypes = occurrences
        .iter()
        .map(|occurrence| (&occurrence.id, &occurrence.prototype))
        .collect::<BTreeMap<_, _>>();
    let mut spanned = BTreeSet::from([root]);
    let mut pending = vec![root];
    for definition in definitions {
        if embedded(definition)
            && !placed.contains(&definition.id)
            && spanned.insert(&definition.id)
        {
            pending.push(&definition.id);
        }
    }
    while let Some(id) = pending.pop() {
        let Some(definition) = definitions.iter().find(|definition| &definition.id == id) else {
            continue;
        };
        for occurrence in &definition.occurrences {
            let Some(prototype) = prototypes.get(occurrence).and_then(|prototype| {
                definitions
                    .iter()
                    .find(|definition| &definition.id == *prototype)
            }) else {
                continue;
            };
            if embedded(prototype) && spanned.insert(&prototype.id) {
                pending.push(&prototype.id);
            }
        }
    }
    spanned
}

/// The placement of a container whose source stores frame `frame` (§5.2):
/// the frame as one fixed step. A frame equal to the identity is no
/// placement.
fn placement(frame: FiniteFrame) -> Option<Construction<kinds::Frame>> {
    let transform = frame.transform();
    if transform.rows() == Transform::identity().rows() {
        return None;
    }
    let rows = frame.rows();
    let axis = |column: usize| {
        UnitVector3::new(Vector3::new(
            rows[0][column],
            rows[1][column],
            rows[2][column],
        ))
    };
    Some(Construction {
        inputs: Vec::new(),
        after: vec![PostStep::Fixed(transform)],
        solution: None,
        mode: None,
        resolved: Some(FrameValue {
            origin: FinitePoint3::new(Point3::new(rows[0][3], rows[1][3], rows[2][3]))?,
            x_axis: axis(0)?,
            y_axis: axis(1)?,
            z_axis: axis(2)?,
        }),
        resolved_in: BTreeMap::new(),
    })
}

/// What the projection reads from the document's object records beside the
/// decoded features.
struct Source<'a> {
    /// The objects `FreeCAD` marks `Touched`, out of date.
    touched: BTreeSet<&'a str>,
    /// The `Placement` property of each object that stores one.
    placements: HashMap<&'a str, &'a PropertyRecord>,
    /// The properties each object stores, by object and name.
    stored: BTreeSet<(&'a str, &'a str)>,
    /// The objects a `MultiTransform` lists in `Transformations`.
    stages: BTreeSet<&'a str>,
}

impl<'a> Source<'a> {
    fn of(objects: &'a [ObjectRecord], properties: &'a [PropertyRecord]) -> Self {
        let touched = objects
            .iter()
            .filter(|object| object.attributes.get("Touched").map(String::as_str) == Some("1"))
            .map(|object| object.id().as_str())
            .collect();
        let stored = properties
            .iter()
            .map(|property| (property.owner.as_str(), property.name.as_str()))
            .collect();
        let placements = properties
            .iter()
            .filter(|property| property.name == "Placement")
            .map(|property| (property.owner.as_str(), property))
            .collect();
        Self {
            touched,
            placements,
            stored,
            stages: multi_transform_stages(objects, properties),
        }
    }

    /// Whether the object `legacy` was read from stores property `name`.
    fn stores(&self, legacy: &LegacyFeature, name: &str) -> bool {
        legacy
            .native_ref
            .as_deref()
            .is_some_and(|object| self.stored.contains(&(object, name)))
    }

    /// The status of a state object `object` stores: `Stale` where `FreeCAD`
    /// marks the object `Touched` (§5.4).
    fn status(&self, object: &str) -> EvaluationStatus {
        if self.touched.contains(object) {
            EvaluationStatus::Stale
        } else {
            EvaluationStatus::Current
        }
    }

    fn is_stage(&self, legacy: &LegacyFeature) -> bool {
        legacy
            .native_ref
            .as_deref()
            .is_some_and(|object| self.stages.contains(object))
    }
}

/// The objects a `PartDesign::MultiTransform` lists in `Transformations`: the
/// stages of its layout, which are no features of their own (§6.5).
fn multi_transform_stages<'a>(
    objects: &'a [ObjectRecord],
    properties: &'a [PropertyRecord],
) -> BTreeSet<&'a str> {
    let transforms = objects
        .iter()
        .filter(|object| object.type_name.ends_with("MultiTransform"))
        .map(|object| object.id().as_str())
        .collect::<BTreeSet<_>>();
    properties
        .iter()
        .filter(|property| {
            property.name == "Transformations" && transforms.contains(property.owner.as_str())
        })
        .flat_map(PropertyRecord::links)
        .filter_map(|link| link.as_ref()?.object())
        .collect()
}

/// The design objects of the document and the `PartDesign::Body` objects among
/// them.
struct History<'a> {
    features: BTreeMap<&'a FeatureId, &'a LegacyFeature>,
    bodies: Vec<BodyHistory<'a>>,
    contained: BTreeSet<FeatureId>,
}

/// A `PartDesign::Body`: its members in `Group` order and its `Tip`.
struct BodyHistory<'a> {
    feature: &'a FeatureId,
    members: Vec<FeatureId>,
    tip: Option<FeatureId>,
    outputs: Vec<BodyId>,
    native_ref: Option<String>,
}

impl<'a> History<'a> {
    fn of(legacy: &'a [LegacyFeature], source: &Source<'_>) -> Self {
        let mut ordered = legacy.iter().collect::<Vec<_>>();
        ordered.sort_by_key(|feature| (feature.ordinal, &feature.id));
        let mut features = BTreeMap::new();
        let mut bodies = Vec::new();
        let mut contained = BTreeSet::new();
        let containers = ordered
            .iter()
            .filter_map(
                |feature| match feature.evaluation.definition().operation() {
                    LegacyOperation::TreeNode {
                        role: FeatureTreeNodeRole::SolidBodies,
                        children,
                    } => Some((*feature, children)),
                    _ => None,
                },
            )
            .collect::<Vec<_>>();
        let container_ids = containers
            .iter()
            .map(|(feature, _)| &feature.id)
            .collect::<BTreeSet<_>>();
        for feature in &ordered {
            if !container_ids.contains(&feature.id) && !source.is_stage(feature) {
                features.insert(&feature.id, *feature);
            }
        }
        for (feature, children) in containers {
            let mut members = Vec::new();
            for child in &**children {
                if features.contains_key(child) && contained.insert(child.clone()) {
                    members.push(child.clone());
                }
            }
            let tip = children
                .active_child()
                .as_ref()
                .filter(|tip| members.contains(tip))
                .cloned();
            bodies.push(BodyHistory {
                feature: &feature.id,
                members,
                tip,
                outputs: feature.evaluation.outputs().clone(),
                native_ref: feature.native_ref.clone(),
            });
        }
        Self {
            features,
            bodies,
            contained,
        }
    }
}

/// The feature of `legacy`. Its `refine` and `fuzzy_tolerance` are stated only
/// where the object stores `Refine` and `FuzzyTolerance` (§2.4): the decoded
/// definition fills the one the object lacks.
fn feature(legacy: &LegacyFeature, source: &Source<'_>) -> Feature {
    let (operation, refine, fuzzy_tolerance) = match legacy.evaluation.definition() {
        LegacyDefinition::PostProcess {
            operation,
            refine,
            fuzzy_tolerance,
        } => (
            operation,
            source.stores(legacy, "Refine").then_some(*refine),
            source
                .stores(legacy, "FuzzyTolerance")
                .then_some(match fuzzy_tolerance {
                    LegacyFuzzyTolerance::KernelDefault => FuzzyTolerance::KernelDefault,
                    LegacyFuzzyTolerance::Automatic => FuzzyTolerance::Automatic,
                    LegacyFuzzyTolerance::Explicit(value) => {
                        FuzzyTolerance::Explicit(Dimension::Literal(*value))
                    }
                }),
        ),
        LegacyDefinition::Operation(operation) => (operation, None, None),
    };
    Feature {
        id: legacy.id.clone(),
        name: legacy.name.clone(),
        suppressed: legacy.suppressed.map(Dimension::Literal),
        definition: FeatureDefinition {
            operation: self::operation(legacy, operation),
            assembly: None,
            refine,
            fuzzy_tolerance,
        },
        placement: None,
        component: None,
        driven_by: None,
        native_ref: legacy.native_ref.clone(),
    }
}

fn native(kind: &str) -> FeatureOperation {
    FeatureOperation::Native {
        kind: CodecName::new(&cadmpeg_ir::codec_id!("fcstd"), kind)
            .unwrap_or_else(|| cadmpeg_ir::codec_name!("fcstd:feature")),
        native_options: BTreeMap::default(),
        source_settings: BTreeMap::default(),
    }
}

/// The feature's operation: its family where the decoder reads the operation,
/// and the source's kind where it does not.
fn operation(legacy: &LegacyFeature, operation: &LegacyOperation) -> FeatureOperation {
    let kind = legacy.source_tag.as_deref().unwrap_or("feature");
    let family = match operation {
        LegacyOperation::StoredGeometry {} => {
            return FeatureOperation::Stored {
                bodies: Some(legacy.evaluation.outputs().clone()),
                tessellations: Vec::new(),
                origin: StoredOrigin::Base {},
            }
        }
        LegacyOperation::Sketch { .. } => OperationFamily::Sketch,
        LegacyOperation::TreeNode {
            role: FeatureTreeNodeRole::Equations,
            ..
        } => OperationFamily::Spreadsheet,
        LegacyOperation::SpatialSketch { .. } => OperationFamily::SpatialSketch,
        LegacyOperation::DatumPrincipalPlane { .. }
        | LegacyOperation::DatumPlane { .. }
        | LegacyOperation::DatumThreePointPlane { .. }
        | LegacyOperation::DatumOffsetPlane { .. }
        | LegacyOperation::DatumAxis { .. }
        | LegacyOperation::DatumPoint { .. }
        | LegacyOperation::DatumCoordinateSystem { .. }
        | LegacyOperation::Unresolved {
            family:
                UnresolvedFamily::DatumPlane
                | UnresolvedFamily::DatumAxis
                | UnresolvedFamily::DatumPoint
                | UnresolvedFamily::DatumCoordinateSystem,
        } => OperationFamily::Datum,
        LegacyOperation::Extrude { .. }
        | LegacyOperation::Unresolved {
            family: UnresolvedFamily::Extrude,
        } => OperationFamily::Extrude,
        LegacyOperation::Revolve { .. }
        | LegacyOperation::Unresolved {
            family: UnresolvedFamily::Revolve,
        } => OperationFamily::Revolve,
        LegacyOperation::Sweep { .. } => OperationFamily::Sweep,
        LegacyOperation::HelicalSweep { .. } | LegacyOperation::Coil { .. } => {
            OperationFamily::HelicalSweep
        }
        LegacyOperation::Loft { .. }
        | LegacyOperation::RuledSurface { .. }
        | LegacyOperation::RuledBetweenCurves { .. }
        | LegacyOperation::Unresolved {
            family: UnresolvedFamily::Loft,
        } => OperationFamily::Loft,
        LegacyOperation::Primitive { .. }
        | LegacyOperation::PlanarPatch { .. }
        | LegacyOperation::Block { .. }
        | LegacyOperation::Sphere { .. }
        | LegacyOperation::Torus { .. } => OperationFamily::Primitive,
        LegacyOperation::Fillet { .. }
        | LegacyOperation::Unresolved {
            family: UnresolvedFamily::Fillet,
        } => OperationFamily::Fillet,
        LegacyOperation::Chamfer { .. } => OperationFamily::Chamfer,
        LegacyOperation::Shell { .. } => OperationFamily::Shell,
        LegacyOperation::Thicken { .. } => OperationFamily::Thicken,
        LegacyOperation::OffsetShape { .. } | LegacyOperation::OffsetSurface { .. } => {
            OperationFamily::Offset
        }
        LegacyOperation::Draft { .. }
        | LegacyOperation::Unresolved {
            family: UnresolvedFamily::Draft,
        } => OperationFamily::Draft,
        LegacyOperation::Hole { .. } => OperationFamily::Hole,
        LegacyOperation::Pattern { .. } | LegacyOperation::MirrorShape { .. } => {
            OperationFamily::Pattern
        }
        LegacyOperation::Combine { .. }
        | LegacyOperation::Unresolved {
            family: UnresolvedFamily::Combine,
        } => OperationFamily::Combine,
        LegacyOperation::Scale { .. } | LegacyOperation::MoveBody { .. } => {
            OperationFamily::TransformBodies
        }
        LegacyOperation::Helix { .. }
        | LegacyOperation::HelixNativeAxis { .. }
        | LegacyOperation::SectionShape { .. }
        | LegacyOperation::ProjectOnSurface { .. }
        | LegacyOperation::ProjectedCurve { .. }
        | LegacyOperation::LineSegment { .. }
        | LegacyOperation::CircularArc { .. }
        | LegacyOperation::EllipticArc { .. }
        | LegacyOperation::Polyline { .. }
        | LegacyOperation::RegularPolygonCurve { .. }
        | LegacyOperation::EquationCurve { .. }
        | LegacyOperation::CompositeCurve { .. }
        | LegacyOperation::PointGeometry { .. } => OperationFamily::Curve,
        LegacyOperation::FaceFromShapes { .. } | LegacyOperation::FilledSurface { .. } => {
            OperationFamily::Patch
        }
        _ => return native(kind),
    };
    FeatureOperation::Unresolved { family }
}

#[cfg(test)]
mod tests;
