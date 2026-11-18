// SPDX-License-Identifier: Apache-2.0
//! The v11 design layer of an NX part, projected from the decoded history.
//!
//! The scope is the design of the part's root definition. The part navigator
//! history is one ordered timeline of the decoded features in construction
//! order; a part with no decoded history is one unordered group whose members
//! are one stored-geometry feature per current body.

use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::text::NonBlankString;
use cadmpeg_core::CodecError;
use cadmpeg_ir::design::configuration::{Arrangement, ArrangementKind, Overrides};
use cadmpeg_ir::design::container::{Container, OrderedKind, Rollback, UnorderedKind};
use cadmpeg_ir::design::dimension::Dimension;
use cadmpeg_ir::design::evaluation::{CurrentState, EvaluationStatus};
use cadmpeg_ir::design::feature::{
    Feature, FeatureDefinition, FeatureOperation, OperationFamily, StoredOrigin,
};
use cadmpeg_ir::design::ids::{ArrangementId, ContainerId};
use cadmpeg_ir::design::refs::{Member, Position};
use cadmpeg_ir::design::scope::DesignScope;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::features::{
    BodySelection, Feature as LegacyFeature, FeatureId, FeatureOperation as LegacyOperation,
    FeatureTreeNodeRole, UnresolvedFamily,
};
use cadmpeg_ir::ids::{BodyId, ProductDefinitionId};
use cadmpeg_ir::products::{ProductDefinition, ProductDefinitionKind};

mod parameters;

/// The root definition of an NX assembly, as the placement attachment states it.
const ASSEMBLY_ROOT: &str = "nx:assembly:product-definition#root";
/// The root definition of an NX part that states no assembly root.
const PART_ROOT: &str = "nx:document:product-definition#root";
const TIMELINE: &str = "nx:feature-history:timeline#root";
const GROUP: &str = "nx:document:group#root";

/// Project the decoded NX history into `ir.design` as the design of
/// `definition`. Returns the identities the projection mints, which no source
/// record supplies.
pub(super) fn project(
    ctx: &DecodeContext<'_>,
    ir: &mut CadIr,
    definition: ProductDefinitionId,
) -> Result<Vec<String>, CodecError> {
    let mut minted = Vec::new();
    let mut history = ir
        .model
        .features
        .iter()
        .filter(|feature| is_history(feature))
        .collect::<Vec<_>>();
    history.sort_by(|a, b| (a.ordinal, &a.id).cmp(&(b.ordinal, &b.id)));
    let bodies = ir
        .geometry
        .bodies
        .iter()
        .map(|body| body.id.clone())
        .collect::<Vec<_>>();
    let mut features = Vec::with_capacity(history.len());
    for feature in &history {
        ctx.charge_entities(1, "project NX v11 feature")?;
        features.push(Feature {
            id: feature.id.clone(),
            name: None,
            suppressed: feature.suppressed.map(Dimension::Literal),
            definition: FeatureDefinition {
                operation: operation(feature)?,
                refine: None,
                fuzzy_tolerance: None,
            },
            placement: None,
            component: None,
            driven_by: None,
            native_ref: feature.native_ref.clone(),
        });
    }
    let (container, current) = if features.is_empty() {
        no_timeline(ctx, &bodies, &mut features, &mut minted)?
    } else {
        timeline(ctx, &features, bodies, &mut minted)?
    };
    let (arrangements, active_arrangements) = arrangements(ctx, ir, &definition, &mut minted)?;
    parameters::project(ctx, ir, &definition)?;
    ctx.charge_entities(1, "project NX v11 scope")?;
    let design = &mut ir.design;
    design.scopes.push(DesignScope {
        definition,
        roots: vec![container.id().clone()],
        active_configuration: None,
        active_arrangements,
        occurrences: BTreeMap::default(),
        current,
    });
    design.containers.push(container);
    design.features.extend(features);
    design.arrangements.extend(arrangements);
    design
        .scopes
        .sort_by(|a, b| a.definition.cmp(&b.definition));
    design.containers.sort_by(|a, b| a.id().cmp(b.id()));
    design.features.sort_by(|a, b| a.id.cmp(&b.id));
    design.arrangements.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(minted)
}

/// The root definition of the document: the assembly root where the
/// placement attachment states one, else a part root that owns every
/// current body, which this pushes to `minted`.
pub(super) fn root_definition(
    ctx: &DecodeContext<'_>,
    ir: &mut CadIr,
    minted: &mut Vec<String>,
) -> Result<ProductDefinitionId, CodecError> {
    if let Some(root) = ir
        .model
        .product_definitions
        .iter()
        .find(|definition| definition.id.as_str() == ASSEMBLY_ROOT)
    {
        return Ok(root.id.clone());
    }
    ctx.charge_entities(1, "project NX v11 root definition")?;
    let id = ProductDefinitionId::mint(PART_ROOT).map_err(CodecError::malformed)?;
    let mut bodies = ir
        .geometry
        .bodies
        .iter()
        .map(|body| body.id.clone())
        .collect::<Vec<_>>();
    bodies.sort();
    ir.model.product_definitions.push(ProductDefinition {
        id: id.clone(),
        kind: ProductDefinitionKind::Part,
        source_name: None,
        label: None,
        description: None,
        part_number: None,
        bom_properties: BTreeMap::new(),
        bodies,
        sketches: Vec::new(),
        tessellations: Vec::new(),
        native_ref: None,
    });
    ir.model.product_definitions.sort_by(|a, b| a.id.cmp(&b.id));
    minted.push(id.as_str().to_owned());
    Ok(id)
}

/// Whether a decoded feature is a member of the part navigator history. An
/// expression table is listed as a tree node of its own and is no feature.
fn is_history(feature: &LegacyFeature) -> bool {
    !matches!(
        feature.evaluation.definition().operation(),
        LegacyOperation::TreeNode {
            role: FeatureTreeNodeRole::Equations,
            ..
        }
    )
}

/// One ordered timeline of the history in construction order, with its one
/// current state at the end, which holds every current body.
fn timeline(
    ctx: &DecodeContext<'_>,
    features: &[Feature],
    bodies: Vec<BodyId>,
    minted: &mut Vec<String>,
) -> Result<(Container, Vec<CurrentState>), CodecError> {
    ctx.charge_entities(1, "project NX v11 timeline")?;
    let id = ContainerId::mint(TIMELINE).map_err(CodecError::malformed)?;
    minted.push(id.as_str().to_owned());
    let members = features
        .iter()
        .map(|feature| Member::Feature(feature.id.clone()))
        .collect::<Vec<_>>();
    let at = members.last().map_or_else(
        || Position::Start(id.clone()),
        |last| Position::After(last.clone()),
    );
    let container = Container::Ordered {
        id,
        kind: OrderedKind::Timeline,
        members,
        folders: Vec::new(),
        rollback: Rollback::End,
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

/// The no-timeline rule: one root unordered group whose members are one
/// stored-geometry feature per current body, each body the current state of
/// its feature.
fn no_timeline(
    ctx: &DecodeContext<'_>,
    bodies: &[BodyId],
    features: &mut Vec<Feature>,
    minted: &mut Vec<String>,
) -> Result<(Container, Vec<CurrentState>), CodecError> {
    ctx.charge_entities(1, "project NX v11 group")?;
    let id = ContainerId::mint(GROUP).map_err(CodecError::malformed)?;
    minted.push(id.as_str().to_owned());
    let mut members = BTreeSet::new();
    let mut current = Vec::with_capacity(bodies.len());
    for body in bodies {
        ctx.charge_entities(1, "project NX v11 stored body")?;
        let feature = stored_body_feature(body)?;
        minted.push(feature.as_str().to_owned());
        members.insert(Member::Feature(feature.clone()));
        current.push(CurrentState {
            at: Position::After(Member::Feature(feature.clone())),
            bodies: vec![body.clone()],
            poses: BTreeMap::default(),
            status: EvaluationStatus::Current,
            native_ref: None,
        });
        features.push(Feature {
            id: feature,
            name: None,
            suppressed: None,
            definition: FeatureDefinition {
                operation: FeatureOperation::Stored {
                    bodies: Some(vec![body.clone()]),
                    tessellations: Vec::new(),
                    origin: StoredOrigin::Base {},
                },
                refine: None,
                fuzzy_tolerance: None,
            },
            placement: None,
            component: None,
            driven_by: None,
            native_ref: None,
        });
    }
    let container = Container::Unordered {
        id,
        kind: UnorderedKind::Group,
        members,
    };
    Ok((container, current))
}

/// The stored-geometry feature of one body under the no-timeline rule, keyed
/// by the body's identity.
fn stored_body_feature(body: &BodyId) -> Result<FeatureId, CodecError> {
    FeatureId::mint(format!(
        "nx:document:feature#stored/{}",
        body.as_str().replace('#', "/")
    ))
    .map_err(CodecError::malformed)
}

/// The NX arrangements, each a positional arrangement that states no
/// overrides, and the one the part names as in use.
fn arrangements(
    ctx: &DecodeContext<'_>,
    ir: &CadIr,
    scope: &ProductDefinitionId,
    minted: &mut Vec<String>,
) -> Result<(Vec<Arrangement>, BTreeSet<ArrangementId>), CodecError> {
    let mut arrangements = Vec::with_capacity(ir.model.configurations.len());
    let mut active = BTreeSet::new();
    for configuration in &ir.model.configurations {
        let Some(name) = configuration.name.clone().and_then(NonBlankString::new) else {
            continue;
        };
        ctx.charge_entities(1, "project NX v11 arrangement")?;
        let id = ArrangementId::mint(
            configuration
                .id
                .as_str()
                .replace(":configuration#", ":arrangement#"),
        )
        .map_err(CodecError::malformed)?;
        minted.push(id.as_str().to_owned());
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
        kind: NonBlankString::new(format!("nx:{kind}"))
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
        LegacyOperation::StoredGeometry {} => return stored_outputs(feature),
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

/// A BREP operation whose result bodies the decoder resolves holds them as
/// its own content.
fn stored_outputs(feature: &LegacyFeature) -> Result<FeatureOperation, CodecError> {
    let outputs = feature.evaluation.outputs();
    if outputs.is_empty() {
        return native(feature.source_tag.as_deref().unwrap_or("stored_geometry"));
    }
    Ok(FeatureOperation::Stored {
        bodies: Some(Vec::clone(outputs)),
        tessellations: Vec::new(),
        origin: StoredOrigin::Base {},
    })
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
        UnresolvedFamily::ExtractFace
        | UnresolvedFamily::CopyFace
        | UnresolvedFamily::LinkedFace => OperationFamily::Extract,
        UnresolvedFamily::Draft => OperationFamily::Draft,
        UnresolvedFamily::DeleteFace => OperationFamily::DeleteFaces,
        UnresolvedFamily::MoveFace => OperationFamily::MoveFaces,
        UnresolvedFamily::MirrorFace => OperationFamily::Pattern,
        UnresolvedFamily::SubdivisionBody => OperationFamily::Form,
        UnresolvedFamily::MoveObject => OperationFamily::TransformBodies,
        UnresolvedFamily::Brep | UnresolvedFamily::TopologyOptimization => return None,
    })
}

#[cfg(test)]
mod tests;
