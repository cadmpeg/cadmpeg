// SPDX-License-Identifier: Apache-2.0
//! The v11 design layer of an NX part, projected from the decoded history.
//!
//! The scope is the design of the part's root definition. The part navigator
//! history is one ordered timeline of the decoded features in construction
//! order. A part with no decoded history states a scope only where it holds
//! expressions or arrangements; its bodies are then unproduced (§5.5).

use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_core::text::NonBlankString;
use cadmpeg_core::CodecError;
use cadmpeg_ir::design::configuration::{Arrangement, ArrangementKind, Overrides};
use cadmpeg_ir::design::container::{Container, OrderedKind, Rollback};
use cadmpeg_ir::design::dimension::Dimension;
use cadmpeg_ir::design::evaluation::{CurrentState, EvaluationStatus};
use cadmpeg_ir::design::feature::{
    Feature, FeatureDefinition, FeatureOperation, Link, OperationFamily, StoredOrigin,
};
use cadmpeg_ir::design::ids::{ArrangementId, ContainerId};
use cadmpeg_ir::design::names::CodecName;
use cadmpeg_ir::design::projection::fields::{
    ArrangementField, FeatureField, ModelConfigurationField, ModelFeatureField,
};
use cadmpeg_ir::design::projection::Restatement;
use cadmpeg_ir::design::refs::{Member, Position};
use cadmpeg_ir::design::scope::DesignScope;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::features::selections::BodySelection;
use cadmpeg_ir::features::tree::FeatureTreeNodeRole;
use cadmpeg_ir::features::{
    Feature as LegacyFeature, FeatureOperation as LegacyOperation, UnresolvedFamily,
};
use cadmpeg_ir::ids::{BodyId, ProductDefinitionId};
use cadmpeg_ir::products::{ExternalDocument, ProductDefinition, ProductDefinitionKind};
use cadmpeg_ir::report::loss::LossNote;

use crate::loss::NxLossCode;

mod parameters;
mod sketches;

/// The root definition of an NX part that states no assembly root.
pub(super) const PART_ROOT: &str = "nx:document:product-definition#root";
const TIMELINE: &str = "nx:feature-history:timeline#root";

/// Project the decoded NX history, sketches, expressions and arrangements
/// into `ir.design` as the design of `definition`.
pub(super) fn project(
    ir: &mut CadIr,
    definition: ProductDefinitionId,
    losses: &mut Vec<LossNote>,
) -> Result<(), CodecError> {
    let mut history = ir
        .model
        .features
        .iter()
        .filter(|feature| is_history(feature))
        .collect::<Vec<_>>();
    history.sort_by(|a, b| (a.ordinal, &a.id).cmp(&(b.ordinal, &b.id)));
    let owners = sketches::owners(ir, &history);
    let mut features = Vec::with_capacity(history.len());
    for feature in &history {
        let operation = match sketches::sketch_of(feature) {
            Some(sketch) if owners.get(sketch) == Some(&feature.id) => {
                FeatureOperation::Sketch(sketch.clone())
            }
            _ => operation(feature)?,
        };
        features.push(Feature {
            id: feature.id.clone(),
            name: None,
            suppressed: feature.suppressed.map(Dimension::Literal),
            definition: FeatureDefinition {
                operation,
                assembly: None,
                refine: None,
                fuzzy_tolerance: None,
            },
            placement: None,
            component: None,
            driven_by: None,
            native_ref: feature.native_ref.clone(),
        });
    }
    let bodies = ir
        .geometry
        .bodies
        .iter()
        .map(|body| body.id.clone())
        .collect::<Vec<_>>();
    let (arrangements, active_arrangements) = arrangements(ir, &definition, losses)?;
    sketches::project(ir, &owners, losses);
    parameters::project(ir, &definition, losses);
    if features.is_empty() && arrangements.is_empty() && ir.design.parameters.is_empty() {
        return Ok(());
    }
    let mut scope = DesignScope {
        definition,
        roots: Vec::new(),
        active_configuration: None,
        active_arrangements,
        occurrences: BTreeMap::default(),
        current: Vec::new(),
        unproduced: BTreeSet::default(),
    };
    let design = &mut ir.design;
    if features.is_empty() {
        scope.unproduced = bodies.into_iter().collect();
    } else {
        let (container, current) = timeline(&features, bodies)?;
        scope.roots.push(container.id().clone());
        scope.current = current;
        design.containers.push(container);
    }
    design.scopes.push(scope);
    design.features.extend(features);
    design.arrangements.extend(arrangements);
    design
        .scopes
        .sort_by(|a, b| a.definition.cmp(&b.definition));
    design.containers.sort_by(|a, b| a.id().cmp(b.id()));
    design.features.sort_by(|a, b| a.id.cmp(&b.id));
    design.arrangements.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(())
}

/// The model fields [`project`] reads to build each design field.
pub(super) const RESTATEMENT: Restatement = Restatement {
    feature: feature_inputs,
    parameter: parameters::inputs,
    arrangement: arrangement_inputs,
};

/// The model feature fields [`project`] reads to build `field`: the
/// operation reads the definition, the stored outputs and the source tag.
#[deny(clippy::wildcard_enum_match_arm)]
fn feature_inputs(field: FeatureField) -> &'static [ModelFeatureField] {
    match field {
        FeatureField::Suppressed => &[ModelFeatureField::Suppressed],
        FeatureField::Definition => &[
            ModelFeatureField::Definition,
            ModelFeatureField::Outputs,
            ModelFeatureField::SourceTag,
        ],
        FeatureField::NativeRef => &[ModelFeatureField::NativeRef],
        FeatureField::Name
        | FeatureField::Placement
        | FeatureField::Component
        | FeatureField::DrivenBy => &[],
    }
}

/// The model configuration fields [`project`] reads to build each field of
/// the arrangement that restates the configuration.
#[deny(clippy::wildcard_enum_match_arm)]
fn arrangement_inputs(field: ArrangementField) -> &'static [ModelConfigurationField] {
    match field {
        ArrangementField::Name => &[ModelConfigurationField::Name],
        ArrangementField::NativeRef => &[ModelConfigurationField::NativeRef],
        ArrangementField::Scope
        | ArrangementField::Kind
        | ArrangementField::Configuration
        | ArrangementField::Overrides
        | ArrangementField::Appearances
        | ArrangementField::Moves
        | ArrangementField::Nested => &[],
    }
}

/// The root definition of the document: the one the placement attachment
/// states for an assembly, else a part definition stored in the root file
/// that owns every current body. The flag is true where this states it.
pub(super) fn root_definition(ir: &mut CadIr) -> Result<(ProductDefinitionId, bool), CodecError> {
    if let Some(root) = ir
        .model
        .product_definitions
        .iter()
        .find(|definition| definition.is_root())
    {
        return Ok((root.id.clone(), false));
    }
    let id = ProductDefinitionId::mint(PART_ROOT).map_err(CodecError::malformed)?;
    let mut root = ProductDefinition::root(id.clone(), ExternalDocument::default());
    root.kind = Some(ProductDefinitionKind::Part);
    root.bodies = ir
        .geometry
        .bodies
        .iter()
        .map(|body| body.id.clone())
        .collect();
    root.bodies.sort();
    ir.model.product_definitions.push(root);
    ir.model.product_definitions.sort_by(|a, b| a.id.cmp(&b.id));
    Ok((id, true))
}

/// Whether a decoded feature is a member of the part navigator history. An
/// expression table is listed as a tree node of its own and is no feature.
/// The retained-history input holds the bodies present when the retained
/// history begins, which no source record lists as a feature.
fn is_history(feature: &LegacyFeature) -> bool {
    match feature.evaluation.definition().operation() {
        LegacyOperation::TreeNode {
            role: FeatureTreeNodeRole::Equations,
            ..
        } => false,
        LegacyOperation::BaseFeature { .. } => feature.native_ref.is_some(),
        _ => true,
    }
}

/// One ordered timeline of the history in construction order. The decoder
/// reads no current feature (part 11, entry B.96), so the rollback of a
/// timeline with members is unstated and its one current state, at
/// `After(Container(timeline))` (§5.3), holds every current body. A timeline
/// with no member has no member to stand before: it is built through its end,
/// and its current state is at its start.
fn timeline(
    features: &[Feature],
    bodies: Vec<BodyId>,
) -> Result<(Container, Vec<CurrentState>), CodecError> {
    let id = ContainerId::mint(TIMELINE).map_err(CodecError::malformed)?;
    let members = features
        .iter()
        .map(|feature| Member::Feature(feature.id.clone()))
        .collect::<Vec<_>>();
    let (rollback, at) = if members.is_empty() {
        (Some(Rollback::End), Position::Start(id.clone()))
    } else {
        (None, Position::After(Member::Container(id.clone())))
    };
    let container = Container::Ordered {
        id,
        kind: OrderedKind::Timeline,
        members,
        folders: Vec::new(),
        rollback,
        placement: None,
    };
    let current = vec![CurrentState {
        at,
        bodies,
        poses: BTreeMap::default(),
        status: EvaluationStatus::Current,
        native_ref: None,
    }];
    Ok((container, current))
}

/// The NX arrangements, each a positional arrangement that states no
/// overrides and restates the model configuration of its id, and the ones
/// the part names as in use. An arrangement with no name has no design
/// record.
fn arrangements(
    ir: &CadIr,
    scope: &ProductDefinitionId,
    losses: &mut Vec<LossNote>,
) -> Result<(Vec<Arrangement>, BTreeSet<ArrangementId>), CodecError> {
    let mut arrangements = Vec::with_capacity(ir.model.configurations.len());
    let mut active = BTreeSet::new();
    for configuration in &ir.model.configurations {
        let Some(name) = configuration.name.clone().and_then(NonBlankString::new) else {
            losses.push(NxLossCode::ArrangementUnnamed.note(format!(
                "NX arrangement {} stores no name and has no design record.",
                configuration.id
            )));
            continue;
        };
        let id = ArrangementId::mint(configuration.id.as_str()).map_err(CodecError::malformed)?;
        if configuration.active {
            active.insert(id.clone());
        }
        arrangements.push(Arrangement {
            id,
            scope: scope.clone(),
            name,
            kind: ArrangementKind::Positional,
            configuration: None,
            overrides: Overrides::default(),
            appearances: Vec::new(),
            moves: Vec::new(),
            nested: Vec::new(),
            native_ref: configuration.native_ref.clone(),
        });
    }
    Ok((arrangements, active))
}

fn native(kind: &str) -> Result<FeatureOperation, CodecError> {
    Ok(FeatureOperation::Native {
        kind: CodecName::new(&cadmpeg_ir::codec_id!("nx"), kind)
            .ok_or_else(|| CodecError::malformed("NX v11 operation kind is blank"))?,
        native_options: BTreeMap::default(),
        source_settings: BTreeMap::default(),
    })
}

/// The v11 operation of a decoded feature. The decoder resolves no operand
/// of an NX feature into a neutral selection yet, so a feature of a known
/// family is `Unresolved`; a feature whose family is not established keeps
/// its NX kind.
fn operation(feature: &LegacyFeature) -> Result<FeatureOperation, CodecError> {
    let kind = feature.source_tag.as_deref();
    let family = match feature.evaluation.definition().operation() {
        LegacyOperation::BaseFeature {
            bodies: BodySelection::Resolved { bodies, .. },
        } if !bodies.is_empty() => {
            return Ok(FeatureOperation::Stored {
                bodies: Some(bodies.iter().cloned().collect()),
                tessellations: Vec::new(),
                origin: StoredOrigin::Base {},
            })
        }
        LegacyOperation::StoredGeometry {} => {
            return stored_outputs(feature, StoredOrigin::Base {}, "stored_geometry")
        }
        LegacyOperation::Unresolved {
            family: UnresolvedFamily::LinkedFace,
        } => return stored_outputs(feature, wave_link(), "LINKED_FACE"),
        LegacyOperation::Native { kind, .. } => return native(kind.as_str()),
        LegacyOperation::TreeNode { .. } | LegacyOperation::BaseFeature { .. } => {
            return native(kind.unwrap_or("base_feature"))
        }
        LegacyOperation::Sketch { .. } => OperationFamily::Sketch,
        LegacyOperation::Block { .. } | LegacyOperation::Sphere { .. } => {
            OperationFamily::Primitive
        }
        LegacyOperation::Extrude { .. } => OperationFamily::Extrude,
        LegacyOperation::Revolve { .. } => OperationFamily::Revolve,
        LegacyOperation::Sweep { .. } => OperationFamily::Sweep,
        LegacyOperation::Loft { .. } => OperationFamily::Loft,
        LegacyOperation::Rib { .. } => OperationFamily::Rib,
        LegacyOperation::Fillet { .. } | LegacyOperation::FaceBlend { .. } => {
            OperationFamily::Fillet
        }
        LegacyOperation::Chamfer { .. } => OperationFamily::Chamfer,
        LegacyOperation::Shell { .. } => OperationFamily::Shell,
        LegacyOperation::Draft { .. } => OperationFamily::Draft,
        LegacyOperation::Thicken { .. } => OperationFamily::Thicken,
        LegacyOperation::OffsetSurface { .. } => OperationFamily::Offset,
        LegacyOperation::Hole { .. } => OperationFamily::Hole,
        LegacyOperation::CosmeticThread { .. } => OperationFamily::Thread,
        LegacyOperation::Pattern { .. } => OperationFamily::Pattern,
        LegacyOperation::Combine { .. } => OperationFamily::Combine,
        LegacyOperation::TrimBodies { .. } | LegacyOperation::TrimSurface { .. } => {
            OperationFamily::Split
        }
        LegacyOperation::DeleteBody { .. } => OperationFamily::DeleteBodies,
        LegacyOperation::ExtractBody { .. } => OperationFamily::Extract,
        LegacyOperation::ReplaceFace { .. } => OperationFamily::ReplaceFaces,
        LegacyOperation::SewBodies { .. } => OperationFamily::Knit,
        LegacyOperation::ExtendSurface { .. } => OperationFamily::Extend,
        LegacyOperation::SectionShape { .. } | LegacyOperation::ProjectedCurve { .. } => {
            OperationFamily::Curve
        }
        LegacyOperation::DatumOffsetPlane { .. }
        | LegacyOperation::DatumCoordinateSystem { .. } => OperationFamily::Datum,
        LegacyOperation::Unresolved { family } => match unresolved_family(*family) {
            Some(family) => family,
            None => return native(kind.unwrap_or("unresolved")),
        },
        _ => return native(kind.unwrap_or("operation")),
    };
    Ok(FeatureOperation::Unresolved { family })
}

/// A feature defined by stored geometry whose result bodies the decoder
/// resolves holds them as its own content. Its bodies are a value of the
/// design, so where they are not resolved the feature keeps its NX kind.
fn stored_outputs(
    feature: &LegacyFeature,
    origin: StoredOrigin,
    kind: &str,
) -> Result<FeatureOperation, CodecError> {
    let outputs = feature.evaluation.outputs();
    if outputs.is_empty() {
        return native(feature.source_tag.as_deref().unwrap_or(kind));
    }
    Ok(FeatureOperation::Stored {
        bodies: Some(Vec::clone(outputs)),
        tessellations: Vec::new(),
        origin,
    })
}

/// A WAVE link is a frozen copy of geometry in another part that keeps its
/// link. The decoder reads none of the link's fields.
fn wave_link() -> StoredOrigin {
    StoredOrigin::Linked(Box::new(Link {
        source: None,
        document: None,
        transform: None,
        scale: None,
        associative: None,
        state: None,
        native_ref: None,
    }))
}

const fn unresolved_family(family: UnresolvedFamily) -> Option<OperationFamily> {
    Some(match family {
        UnresolvedFamily::DatumPlane
        | UnresolvedFamily::DatumAxis
        | UnresolvedFamily::DatumPoint
        | UnresolvedFamily::DatumCoordinateSystem => OperationFamily::Datum,
        UnresolvedFamily::BridgeCurve => OperationFamily::Curve,
        UnresolvedFamily::Cylinder | UnresolvedFamily::Cone | UnresolvedFamily::Sphere => {
            OperationFamily::Primitive
        }
        UnresolvedFamily::Thread | UnresolvedFamily::DetailedThread => OperationFamily::Thread,
        UnresolvedFamily::Loft | UnresolvedFamily::ThroughCurveMesh => OperationFamily::Loft,
        UnresolvedFamily::FreeformSurface
        | UnresolvedFamily::FillHole
        | UnresolvedFamily::BoundarySurface => OperationFamily::Patch,
        UnresolvedFamily::Extrude => OperationFamily::Extrude,
        UnresolvedFamily::Revolve => OperationFamily::Revolve,
        UnresolvedFamily::Combine => OperationFamily::Combine,
        UnresolvedFamily::Fillet => OperationFamily::Fillet,
        UnresolvedFamily::ExtractFace | UnresolvedFamily::CopyFace => OperationFamily::Extract,
        UnresolvedFamily::Draft => OperationFamily::Draft,
        UnresolvedFamily::DeleteFace => OperationFamily::DeleteFaces,
        UnresolvedFamily::MoveFace => OperationFamily::MoveFaces,
        UnresolvedFamily::MirrorFace => OperationFamily::Pattern,
        UnresolvedFamily::SubdivisionBody => OperationFamily::Form,
        UnresolvedFamily::MoveObject => OperationFamily::TransformBodies,
        UnresolvedFamily::LinkedFace
        | UnresolvedFamily::Brep
        | UnresolvedFamily::TopologyOptimization => return None,
    })
}

#[cfg(test)]
mod tests;
