// SPDX-License-Identifier: Apache-2.0
//! The v11 design layer of a `FreeCAD` document.
//!
//! The document is one scope at its root definition. Each `PartDesign::Body`
//! is an ordered `Body` container, a root of the scope, whose `Tip` is its
//! rollback, and each of its members is one feature, except the stages a
//! `MultiTransform` lists, which are its layout (§6.5). An object outside
//! every Body has no design record, and a document whose Bodies hold no
//! member and no parameter has no scope (§5.1).

use std::collections::{BTreeMap, BTreeSet, HashMap};

use cadmpeg_core::text::NonBlankString;
use cadmpeg_core::CodecError;
use cadmpeg_ir::design::container::{Container, OrderedKind, Rollback};
use cadmpeg_ir::design::dimension::Dimension;
use cadmpeg_ir::design::evaluation::{CurrentState, EvaluationStatus};
use cadmpeg_ir::design::feature::{
    Feature, FeatureDefinition, FeatureOperation, FuzzyTolerance, OperationFamily, StoredOrigin,
};
use cadmpeg_ir::design::ids::ContainerId;
use cadmpeg_ir::design::refs::{Member, Position};
use cadmpeg_ir::design::scope::DesignScope;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::features::{
    Feature as LegacyFeature, FeatureDefinition as LegacyDefinition, FeatureId,
    FeatureOperation as LegacyOperation, FeatureTreeNodeRole,
    FuzzyTolerance as LegacyFuzzyTolerance, UnresolvedFamily,
};
use cadmpeg_ir::ids::BodyId;

use crate::native::{ObjectRecord, PropertyRecord};

mod parameters;

/// Project the decoded design of `ir` into its v11 design layer, and say
/// whether it states a scope.
pub(crate) fn project(
    ir: &mut CadIr,
    objects: &[ObjectRecord],
    properties: &[PropertyRecord],
) -> Result<bool, CodecError> {
    let Some(definition) = ir
        .model
        .product_definitions
        .iter()
        .find(|definition| definition.is_root())
        .map(|definition| definition.id.clone())
    else {
        return Ok(false);
    };
    let source = Source::of(objects, properties);
    let history = History::of(&ir.model.features, &source)?;
    if history.bodies.is_empty() {
        return Ok(false);
    }
    let mut features = Vec::new();
    for member in &history.contained {
        if let Some(legacy) = history.features.get(member) {
            features.push(feature(legacy, &source)?);
        }
    }
    let mut containers = Vec::new();
    let mut body_containers = BTreeMap::new();
    let mut roots = Vec::new();
    let mut current = Vec::new();
    for body in &history.bodies {
        let id = ContainerId::mint(format!("fcstd:design:container#{}", body.key))
            .map_err(CodecError::malformed)?;
        let members = body
            .members
            .iter()
            .cloned()
            .map(Member::Feature)
            .collect::<Vec<_>>();
        let tip = body
            .tip
            .as_ref()
            .and_then(|tip| body.members.iter().position(|member| member == tip));
        let rollback = match tip {
            Some(index) => members
                .get(index + 1)
                .map_or(Rollback::End, |next| Rollback::Before(next.clone())),
            None => members
                .first()
                .map_or(Rollback::End, |first| Rollback::Before(first.clone())),
        };
        let at = tip.map_or_else(
            || Position::Start(id.clone()),
            |index| Position::After(members[index].clone()),
        );
        let touched = body
            .native_ref
            .as_deref()
            .is_some_and(|object| source.touched.contains(object));
        current.push(CurrentState {
            at,
            bodies: body.outputs.clone(),
            poses: BTreeMap::default(),
            status: if touched {
                EvaluationStatus::Stale
            } else {
                EvaluationStatus::Current
            },
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
            placement: None,
        });
    }
    containers.sort_by(|a, b| a.id().cmp(b.id()));
    features.sort_by(|a, b| a.id.cmp(&b.id));
    let carriers = properties
        .iter()
        .map(|property| (property.id.as_str(), property.type_name.as_str()))
        .collect::<HashMap<_, _>>();
    let parameters = parameters::project(ir, &definition, &body_containers, &features, &carriers)?;
    if features.is_empty() && parameters.is_empty() {
        return Ok(false);
    }
    ir.design.containers = containers;
    ir.design.features = features;
    ir.design.parameters = parameters;
    ir.design.scopes.push(DesignScope {
        definition,
        roots,
        active_configuration: None,
        active_arrangements: BTreeSet::default(),
        occurrences: BTreeMap::default(),
        current,
    });
    Ok(true)
}

/// What the projection reads from the document's object records beside the
/// decoded features.
struct Source<'a> {
    /// The objects FreeCAD marks `Touched`, out of date.
    touched: BTreeSet<&'a str>,
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
        Self {
            touched,
            stored,
            stages: super::multi_transform_stages(objects, properties),
        }
    }

    /// Whether the object `legacy` was read from stores property `name`.
    fn stores(&self, legacy: &LegacyFeature, name: &str) -> bool {
        legacy
            .native_ref
            .as_deref()
            .is_some_and(|object| self.stored.contains(&(object, name)))
    }

    fn is_stage(&self, legacy: &LegacyFeature) -> bool {
        legacy
            .native_ref
            .as_deref()
            .is_some_and(|object| self.stages.contains(object))
    }
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
    key: &'a str,
    members: Vec<FeatureId>,
    tip: Option<FeatureId>,
    outputs: Vec<BodyId>,
    native_ref: Option<String>,
}

impl<'a> History<'a> {
    fn of(legacy: &'a [LegacyFeature], source: &Source<'_>) -> Result<Self, CodecError> {
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
            let key = feature
                .id
                .as_str()
                .split_once('#')
                .map(|(_, key)| key)
                .ok_or_else(|| CodecError::malformed("FCStd Body feature id has no key"))?;
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
                key,
                members,
                tip,
                outputs: feature.evaluation.outputs().clone(),
                native_ref: feature.native_ref.clone(),
            });
        }
        Ok(Self {
            features,
            bodies,
            contained,
        })
    }
}

/// The feature of `legacy`. Its `refine` and `fuzzy_tolerance` are stated only
/// where the object stores `Refine` and `FuzzyTolerance` (§2.4): the decoded
/// definition fills the one the object lacks.
fn feature(legacy: &LegacyFeature, source: &Source<'_>) -> Result<Feature, CodecError> {
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
                .then(|| match fuzzy_tolerance {
                    LegacyFuzzyTolerance::KernelDefault => FuzzyTolerance::KernelDefault,
                    LegacyFuzzyTolerance::Automatic => FuzzyTolerance::Automatic,
                    LegacyFuzzyTolerance::Explicit(value) => {
                        FuzzyTolerance::Explicit(Dimension::Literal(*value))
                    }
                }),
        ),
        LegacyDefinition::Operation(operation) => (operation, None, None),
    };
    Ok(Feature {
        id: legacy.id.clone(),
        name: legacy.name.clone(),
        suppressed: legacy.suppressed.map(Dimension::Literal),
        definition: FeatureDefinition {
            operation: self::operation(legacy, operation)?,
            refine,
            fuzzy_tolerance,
        },
        placement: None,
        component: None,
        driven_by: None,
        native_ref: legacy.native_ref.clone(),
    })
}

fn native(kind: &str) -> Result<FeatureOperation, CodecError> {
    Ok(FeatureOperation::Native {
        kind: NonBlankString::new(format!("fcstd:{kind}"))
            .ok_or_else(|| CodecError::malformed("FCStd v11 operation kind is blank"))?,
        native_options: BTreeMap::default(),
        source_settings: BTreeMap::default(),
    })
}

/// The feature's operation: its family where the decoder reads the operation,
/// and the source's kind where it does not.
fn operation(
    legacy: &LegacyFeature,
    operation: &LegacyOperation,
) -> Result<FeatureOperation, CodecError> {
    let kind = legacy.source_tag.as_deref().unwrap_or("feature");
    let family = match operation {
        LegacyOperation::StoredGeometry {} => {
            return Ok(FeatureOperation::Stored {
                bodies: Some(legacy.evaluation.outputs().clone()),
                tessellations: Vec::new(),
                origin: StoredOrigin::Base {},
            })
        }
        LegacyOperation::Sketch { .. } => OperationFamily::Sketch,
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
        | LegacyOperation::Unresolved {
            family: UnresolvedFamily::Loft,
        } => OperationFamily::Loft,
        LegacyOperation::Primitive { .. }
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
        LegacyOperation::RuledSurface { .. } | LegacyOperation::RuledBetweenCurves { .. } => {
            OperationFamily::ImplicitSweep
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
        LegacyOperation::PlanarPatch { .. }
        | LegacyOperation::FaceFromShapes { .. }
        | LegacyOperation::FilledSurface { .. } => OperationFamily::Patch,
        _ => return native(kind),
    };
    Ok(FeatureOperation::Unresolved { family })
}

#[cfg(test)]
mod tests;
