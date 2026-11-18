// SPDX-License-Identifier: Apache-2.0
//! The v11 design layer of a Creo model: its scope, its regeneration timeline,
//! one feature per model feature, its parameters, and its sketches.
//!
//! The projection reads the records the decoder has already built and leaves
//! them unchanged. A model whose feature list the decoder does not read
//! states no design records: whether its bodies belong to members minted for
//! them is under review (interim, errata A1).

use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::text::NonBlankString;
use cadmpeg_core::CodecError;
use cadmpeg_ir::design::container::{Container, OrderedKind, Rollback};
use cadmpeg_ir::design::evaluation::{CurrentState, EvaluationStatus};
use cadmpeg_ir::design::feature::{Feature, FeatureDefinition, FeatureOperation, OperationFamily};
use cadmpeg_ir::design::ids::ContainerId;
use cadmpeg_ir::design::refs::{Member, Position};
use cadmpeg_ir::design::scope::DesignScope;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::features::{
    Feature as LegacyFeature, FeatureDefinition as LegacyDefinition,
    FeatureOperation as LegacyOperation, UnresolvedFamily,
};
use cadmpeg_ir::ids::{BodyId, IdentityNamespace, ProductDefinitionId};
use cadmpeg_ir::products::{ExternalDocument, ProductDefinition, ProductDefinitionKind};
use cadmpeg_ir::sketches::SketchId;

use crate::container::ContainerScan;

mod parameters;
mod sketches;

const TIMELINE: IdentityNamespace = cadmpeg_ir::identity_namespace!("creo", "model", "timeline");

/// States the design scope of the model's root definition, its timeline,
/// its features, its parameters, and its sketches, where the decoder reads a
/// feature list.
pub(super) fn project(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan,
    ir: &mut CadIr,
    definition: &ProductDefinitionId,
) -> Result<(), CodecError> {
    if ir.model.features.is_empty() {
        return Ok(());
    }
    let definition = definition.clone();
    parameters::project(ctx, ir, &definition)?;
    let sketches = sketches::project(ctx, scan, ir)?;
    parameters::omit_measured_display(ir);
    project_timeline(ctx, ir, &definition, &sketches)?;
    ir.design.features.sort_by(|a, b| a.id.cmp(&b.id));
    ir.design.containers.sort_by(|a, b| a.id().cmp(b.id()));
    Ok(())
}

/// The kind of the model the archive's version line declares.
pub(super) fn model_kind(scan: &ContainerScan) -> ProductDefinitionKind {
    if crate::decode::assembly::declares_assembly(&scan.framing.version_line) {
        ProductDefinitionKind::Assembly
    } else {
        ProductDefinitionKind::Part
    }
}

/// The model's root definition. A model whose archive header names no model
/// has none in the product layer; it is stated here, owning every body.
pub(super) fn root_definition(
    ctx: &DecodeContext<'_>,
    ir: &mut CadIr,
    kind: ProductDefinitionKind,
) -> Result<ProductDefinitionId, CodecError> {
    let id = crate::identity::compose_checked::<ProductDefinitionId>(
        &crate::identity::MODEL_PRODUCT_DEFINITION,
        "root",
    )?;
    if ir
        .model
        .product_definitions
        .iter()
        .any(|definition| definition.id == id)
    {
        return Ok(id);
    }
    ctx.charge_entities(1, "admit Creo model product_definitions")?;
    let mut definition = ProductDefinition::root(id.clone(), ExternalDocument::default());
    definition.kind = Some(kind);
    definition.bodies = ir
        .geometry
        .bodies
        .iter()
        .map(|body| body.id.clone())
        .collect();
    ir.model.product_definitions.push(definition);
    ir.model.product_definitions.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(id)
}

fn definition_bodies(ir: &CadIr, definition: &ProductDefinitionId) -> Vec<BodyId> {
    let owned = ir
        .model
        .product_definitions
        .iter()
        .filter(|candidate| candidate.id == *definition)
        .flat_map(|candidate| &candidate.bodies)
        .collect::<BTreeSet<_>>();
    ir.geometry
        .bodies
        .iter()
        .filter(|body| owned.contains(&body.id))
        .map(|body| body.id.clone())
        .collect()
}

/// One ordered timeline holding every model feature in regeneration order.
/// The decoder reads no insert position, so the timeline ends at its last
/// member, which holds the current geometry.
fn project_timeline(
    ctx: &DecodeContext<'_>,
    ir: &mut CadIr,
    definition: &ProductDefinitionId,
    sketches: &BTreeSet<SketchId>,
) -> Result<(), CodecError> {
    let mut ordered = ir.model.features.iter().collect::<Vec<_>>();
    ordered.sort_by(|a, b| (a.ordinal, &a.id).cmp(&(b.ordinal, &b.id)));
    let timeline = crate::identity::compose_checked::<ContainerId>(&TIMELINE, "root")?;
    let members = ordered
        .iter()
        .map(|feature| Member::Feature(feature.id.clone()))
        .collect::<Vec<_>>();
    let at = members.last().map_or_else(
        || Position::Start(timeline.clone()),
        |last| Position::After(last.clone()),
    );
    let mut features = Vec::new();
    for feature in &ordered {
        ctx.charge_entities(1, "project Creo v11 feature")?;
        features.push(project_feature(feature, sketches)?);
    }
    ctx.charge_entities(1, "project Creo v11 timeline")?;
    ctx.charge_entities(1, "project Creo v11 scope")?;
    let current = CurrentState {
        at,
        bodies: definition_bodies(ir, definition),
        poses: BTreeMap::new(),
        status: EvaluationStatus::Current,
        native_ref: None,
    };
    ir.design.features.extend(features);
    ir.design.containers.push(Container::Ordered {
        id: timeline.clone(),
        kind: OrderedKind::Timeline,
        members,
        folders: Vec::new(),
        rollback: Rollback::End,
        placement: None,
    });
    ir.design.scopes.push(DesignScope {
        definition: definition.clone(),
        roots: vec![timeline],
        active_configuration: None,
        active_arrangements: BTreeSet::new(),
        occurrences: BTreeMap::new(),
        current: vec![current],
    });
    Ok(())
}

/// The v11 feature of one model feature. The decoder does not read Creo's
/// suppression state, so `suppressed` is unstated.
fn project_feature(
    feature: &LegacyFeature,
    sketches: &BTreeSet<SketchId>,
) -> Result<Feature, CodecError> {
    let (operation, refine) = match feature.evaluation.definition() {
        LegacyDefinition::Operation(operation) => (operation, None),
        LegacyDefinition::PostProcess {
            operation, refine, ..
        } => (operation, Some(*refine)),
    };
    Ok(Feature {
        id: feature.id.clone(),
        name: feature.name.clone(),
        suppressed: None,
        definition: FeatureDefinition {
            operation: self::operation(operation, sketches)?,
            refine,
            fuzzy_tolerance: None,
        },
        placement: None,
        component: None,
        driven_by: None,
        native_ref: feature.native_ref.clone(),
    })
}

fn native(kind: &str) -> Result<FeatureOperation, CodecError> {
    Ok(FeatureOperation::Native {
        kind: NonBlankString::new(format!("creo:{kind}"))
            .ok_or_else(|| CodecError::malformed("Creo v11 operation kind is blank"))?,
        native_options: BTreeMap::new(),
        source_settings: BTreeMap::new(),
    })
}

/// The family of a model operation. A sketch feature whose sketch is stated
/// names it; every other operation keeps its family and no operands in this
/// projection, and an operation the decoder keeps native stays native under
/// its Creo kind.
fn operation(
    operation: &LegacyOperation,
    sketches: &BTreeSet<SketchId>,
) -> Result<FeatureOperation, CodecError> {
    if let Some(sketch) =
        self::sketches::sketch_of(operation).filter(|sketch| sketches.contains(*sketch))
    {
        return Ok(FeatureOperation::Sketch(sketch.clone()));
    }
    let family = match operation {
        LegacyOperation::TreeNode { .. } => return native("tree_node"),
        LegacyOperation::BaseFeature { .. } => return native("base_feature"),
        LegacyOperation::MeshImport { .. }
        | LegacyOperation::MeshGenerateFaceGroups { .. }
        | LegacyOperation::MeshCombineFaceGroups { .. }
        | LegacyOperation::MeshSection { .. }
        | LegacyOperation::MeshModify { .. }
        | LegacyOperation::MeshConvert(..) => OperationFamily::Mesh,
        LegacyOperation::InsertBodies { .. } => return native("insert_bodies"),
        LegacyOperation::InsertComponent { .. } => OperationFamily::Place,
        LegacyOperation::InsertComponents { .. } => OperationFamily::PlaceSet,
        LegacyOperation::ComponentTransfer { .. } => return native("component_transfer"),
        LegacyOperation::CapturedComponentPositions { .. } => {
            return native("captured_component_positions")
        }
        LegacyOperation::AssemblyJoint { .. } => OperationFamily::Joint,
        LegacyOperation::Form(..) => OperationFamily::Form,
        LegacyOperation::CosmeticThread { .. } => OperationFamily::Thread,
        LegacyOperation::ReferenceImage { .. } => return native("reference_image"),
        LegacyOperation::Decal { .. } => OperationFamily::Decal,
        LegacyOperation::DatumPrincipalPlane { .. }
        | LegacyOperation::DatumPlane { .. }
        | LegacyOperation::DatumThreePointPlane { .. }
        | LegacyOperation::DatumOffsetPlane { .. }
        | LegacyOperation::DatumAxis { .. }
        | LegacyOperation::DatumPoint { .. }
        | LegacyOperation::PointGeometry { .. }
        | LegacyOperation::DatumCoordinateSystem { .. } => OperationFamily::Datum,
        LegacyOperation::LineSegment { .. }
        | LegacyOperation::CircularArc { .. }
        | LegacyOperation::EllipticArc { .. }
        | LegacyOperation::Polyline { .. }
        | LegacyOperation::RegularPolygonCurve { .. }
        | LegacyOperation::EquationCurve { .. }
        | LegacyOperation::ProjectedCurve { .. }
        | LegacyOperation::ProjectOnSurface { .. }
        | LegacyOperation::CompositeCurve { .. }
        | LegacyOperation::Helix { .. }
        | LegacyOperation::HelixNativeAxis { .. }
        | LegacyOperation::SectionShape { .. } => OperationFamily::Curve,
        LegacyOperation::PlanarPatch { .. }
        | LegacyOperation::FaceFromShapes { .. }
        | LegacyOperation::FilledSurface { .. } => OperationFamily::Patch,
        LegacyOperation::Block { .. }
        | LegacyOperation::Sphere { .. }
        | LegacyOperation::Torus { .. }
        | LegacyOperation::Primitive { .. } => OperationFamily::Primitive,
        LegacyOperation::Coil { .. } | LegacyOperation::HelicalSweep { .. } => {
            OperationFamily::HelicalSweep
        }
        LegacyOperation::Wrap { .. } => return native("wrap"),
        LegacyOperation::Sketch { .. }
        | LegacyOperation::SketchBlockDefinition { .. }
        | LegacyOperation::SketchBlockInstance { .. } => OperationFamily::Sketch,
        LegacyOperation::SpatialSketch { .. } => OperationFamily::SpatialSketch,
        LegacyOperation::StoredGeometry { .. } => return native("stored_geometry"),
        LegacyOperation::ExtractBody { .. } => OperationFamily::Extract,
        LegacyOperation::DerivedGeometry { .. } => return native("derived_geometry"),
        LegacyOperation::ImportedGeometry { .. } => return native("imported_geometry"),
        LegacyOperation::Extrude { .. } => OperationFamily::Extrude,
        LegacyOperation::Revolve { .. } => OperationFamily::Revolve,
        LegacyOperation::Sweep { .. } => OperationFamily::Sweep,
        LegacyOperation::Loft { .. } => OperationFamily::Loft,
        LegacyOperation::Binder { .. } => return native("binder"),
        LegacyOperation::Rib { .. } => OperationFamily::Rib,
        LegacyOperation::SheetMetalBaseFlange { .. }
        | LegacyOperation::SheetMetalAttachedPlate { .. }
        | LegacyOperation::SheetMetalBend { .. }
        | LegacyOperation::SheetMetalContourFlange { .. }
        | LegacyOperation::SheetMetalUnfold { .. }
        | LegacyOperation::SheetMetalRefold { .. }
        | LegacyOperation::SheetMetalBendLines { .. }
        | LegacyOperation::SheetMetalCorner { .. }
        | LegacyOperation::SheetMetalEdgeFlange(..)
        | LegacyOperation::SheetMetalHem { .. } => OperationFamily::SheetMetal,
        LegacyOperation::Fillet { .. }
        | LegacyOperation::FullRoundFillet { .. }
        | LegacyOperation::FaceBlend { .. } => OperationFamily::Fillet,
        LegacyOperation::Chamfer { .. } => OperationFamily::Chamfer,
        LegacyOperation::Shell { .. } => OperationFamily::Shell,
        LegacyOperation::OffsetShape { .. } | LegacyOperation::OffsetSurface { .. } => {
            OperationFamily::Offset
        }
        LegacyOperation::Compound { .. } => return native("compound"),
        LegacyOperation::RefineShape { .. } => return native("refine_shape"),
        LegacyOperation::ReverseShape { .. } => return native("reverse_shape"),
        LegacyOperation::RuledBetweenCurves { .. } | LegacyOperation::RuledSurface { .. } => {
            OperationFamily::ImplicitSweep
        }
        LegacyOperation::MirrorShape { .. } | LegacyOperation::Pattern { .. } => {
            OperationFamily::Pattern
        }
        LegacyOperation::Thicken { .. } => OperationFamily::Thicken,
        LegacyOperation::KnitSurface { .. } | LegacyOperation::SewBodies { .. } => {
            OperationFamily::Knit
        }
        LegacyOperation::UnstitchFaces { .. } => OperationFamily::UnsewFaces,
        LegacyOperation::TrimSurface { .. }
        | LegacyOperation::CutWithSurface { .. }
        | LegacyOperation::TrimBodies { .. }
        | LegacyOperation::TrimBodiesAtPlane { .. }
        | LegacyOperation::SplitBody { .. }
        | LegacyOperation::SilhouetteSplit { .. }
        | LegacyOperation::SplitFace { .. } => OperationFamily::Split,
        LegacyOperation::ExtendSurface { .. } | LegacyOperation::UntrimSurface { .. } => {
            OperationFamily::Extend
        }
        LegacyOperation::Draft { .. } => OperationFamily::Draft,
        LegacyOperation::Combine { .. } => OperationFamily::Combine,
        LegacyOperation::BoundaryFill { .. } => OperationFamily::BoundaryFill,
        LegacyOperation::DeleteBody { .. } => OperationFamily::DeleteBodies,
        LegacyOperation::DeleteFace { .. } => OperationFamily::DeleteFaces,
        LegacyOperation::ReplaceFace { .. } => OperationFamily::ReplaceFaces,
        LegacyOperation::MoveFace { .. } => OperationFamily::MoveFaces,
        LegacyOperation::MoveBody { .. } | LegacyOperation::Scale { .. } => {
            OperationFamily::TransformBodies
        }
        LegacyOperation::Dome { .. } => OperationFamily::Dome,
        LegacyOperation::Flex { .. } => OperationFamily::Flex,
        LegacyOperation::Hole { .. } => OperationFamily::Hole,
        LegacyOperation::Unresolved { family } => return unresolved_family(*family),
        LegacyOperation::Native { kind, .. } => return native(kind.as_str()),
    };
    Ok(FeatureOperation::Unresolved { family })
}

fn unresolved_family(family: UnresolvedFamily) -> Result<FeatureOperation, CodecError> {
    let family = match family {
        UnresolvedFamily::DatumPlane
        | UnresolvedFamily::DatumAxis
        | UnresolvedFamily::DatumPoint
        | UnresolvedFamily::DatumCoordinateSystem => OperationFamily::Datum,
        UnresolvedFamily::BridgeCurve => OperationFamily::Curve,
        UnresolvedFamily::Brep => return native("brep"),
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
        UnresolvedFamily::TopologyOptimization => return native("topology_optimization"),
        UnresolvedFamily::MoveObject => OperationFamily::TransformBodies,
    };
    Ok(FeatureOperation::Unresolved { family })
}

#[cfg(test)]
mod tests;
