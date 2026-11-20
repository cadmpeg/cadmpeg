// SPDX-License-Identifier: Apache-2.0
//! The v11 history of an Inventor part.

use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::text::NonBlankString;
use cadmpeg_core::CodecError;
use cadmpeg_ir::design::container::{Container, OrderedKind, Rollback};
use cadmpeg_ir::design::dimension::Dimension;
use cadmpeg_ir::design::evaluation::{CurrentState, EvaluationStatus};
use cadmpeg_ir::design::feature::{Feature, FeatureDefinition, FeatureOperation, OperationFamily};
use cadmpeg_ir::design::ids::ContainerId;
use cadmpeg_ir::design::refs::{Member, Position};
use cadmpeg_ir::design::scope::DesignScope;
use cadmpeg_ir::features::{
    Feature as LegacyFeature, FeatureOperation as LegacyOperation, UnresolvedFamily,
};
use cadmpeg_ir::CadIr;

mod datums;
mod operations;
mod parameters;
mod sheet_metal;
mod sketches;

pub(crate) fn project(
    ctx: &DecodeContext<'_>,
    ir: &mut CadIr,
    features: &crate::feature::FeatureInventory,
    sketches: &crate::sketch::SketchInventory,
    parameters: &[crate::design::PmDcParameter],
    units: &[crate::design::PmDcUnit],
    losses: &mut Vec<cadmpeg_ir::report::loss::LossNote>,
) -> Result<(), CodecError> {
    let flags = features
        .features
        .iter()
        .map(|record| (record.id(), record.header.flags))
        .chain(
            features
                .pattern_features
                .iter()
                .map(|record| (record.id(), record.header.flags)),
        )
        .chain(
            features
                .work_planes
                .iter()
                .map(|record| (record.id(), record.header.flags)),
        )
        .chain(
            sketches
                .sketches
                .iter()
                .map(|record| (record.id(), record.header.flags)),
        )
        .collect();
    project_history(ctx, ir, &flags)?;
    parameters::project(ctx, ir, parameters, units)?;
    sketches::project(ctx, ir)?;
    datums::project(ctx, ir)?;
    sheet_metal::project(ctx, ir, features, parameters, losses)?;
    operations::project(ctx, ir, losses)
}

pub(crate) fn validate_operations(
    ir: &CadIr,
) -> Result<Vec<cadmpeg_ir::report::check::Finding>, CodecError> {
    operations::validate(ir)
}

fn project_history(
    ctx: &DecodeContext<'_>,
    ir: &mut CadIr,
    flags: &BTreeMap<String, u32>,
) -> Result<(), CodecError> {
    let Some(part) = ir.model.product_definitions.first() else {
        return Ok(());
    };
    let definition = part.id.clone();
    let owned_bodies = part.bodies.iter().collect::<BTreeSet<_>>();
    let mut ordered = ir.model.features.iter().collect::<Vec<_>>();
    ordered.sort_by_key(|feature| (feature.ordinal, &feature.id));
    let mut roots = Vec::new();
    let mut current = Vec::new();
    for (flat, key) in [(false, "root"), (true, "flat-pattern")] {
        let history = ordered
            .iter()
            .copied()
            .filter(|feature| is_flat_pattern(feature) == flat)
            .collect::<Vec<_>>();
        if flat && history.is_empty() {
            continue;
        }
        ctx.charge_entities(1, "project Inventor v11 timeline")?;
        let id = ContainerId::mint(format!("inventor:document:timeline#{key}"))
            .map_err(CodecError::malformed)?;
        let members = history
            .iter()
            .map(|feature| Member::Feature(feature.id.clone()))
            .collect::<Vec<_>>();
        let before = history
            .iter()
            .position(|feature| property(feature, "end_of_part", "below"));
        let rollback = before.map_or(Rollback::End, |index| {
            Rollback::Before(members[index].clone())
        });
        if !flat {
            let count = before.unwrap_or(members.len());
            let at = count.checked_sub(1).map_or_else(
                || Position::Start(id.clone()),
                |index| Position::After(members[index].clone()),
            );
            current.push(CurrentState {
                at,
                bodies: ir
                    .geometry
                    .bodies
                    .iter()
                    .filter(|body| owned_bodies.contains(&body.id))
                    .map(|body| body.id.clone())
                    .collect(),
                poses: BTreeMap::default(),
                status: EvaluationStatus::Current,
                native_ref: None,
            });
        }
        roots.push(id.clone());
        ir.design.containers.push(Container::Ordered {
            id,
            kind: OrderedKind::Timeline,
            members,
            folders: Vec::new(),
            rollback,
            placement: None,
        });
    }
    for feature in ordered {
        ctx.charge_entities(1, "project Inventor v11 feature")?;
        ir.design.features.push(Feature {
            id: feature.id.clone(),
            name: feature.name.clone(),
            suppressed: feature
                .native_ref
                .as_ref()
                .and_then(|id| flags.get(id))
                .map(|flags| {
                    Dimension::Literal(flags & crate::feature::history::SUPPRESSED_FLAG != 0)
                }),
            definition: FeatureDefinition {
                operation: operation(feature.evaluation.definition().operation())?,
                refine: None,
                fuzzy_tolerance: None,
            },
            placement: None,
            component: None,
            driven_by: None,
            native_ref: feature.native_ref.clone(),
        });
    }
    ctx.charge_entities(1, "project Inventor v11 scope")?;
    ir.design.features.sort_by(|a, b| a.id.cmp(&b.id));
    ir.design.containers.sort_by(|a, b| a.id().cmp(b.id()));
    ir.design.scopes.push(DesignScope {
        definition,
        roots,
        active_configuration: None,
        active_arrangements: BTreeSet::default(),
        occurrences: BTreeMap::default(),
        current,
    });
    Ok(())
}

fn is_flat_pattern(feature: &LegacyFeature) -> bool {
    property(feature, "history", "flat_pattern")
        || feature.source_tag.as_deref() == Some("flat_pattern")
}

fn property(feature: &LegacyFeature, key: &str, value: &str) -> bool {
    feature
        .source_properties
        .iter()
        .any(|(name, text)| name.as_str() == key && text == value)
}

fn native(kind: &str) -> Result<FeatureOperation, CodecError> {
    Ok(FeatureOperation::Native {
        kind: NonBlankString::new(format!("inventor:{kind}"))
            .ok_or_else(|| CodecError::malformed("Inventor v11 operation kind is blank"))?,
        native_options: BTreeMap::default(),
        source_settings: BTreeMap::default(),
    })
}

fn operation(operation: &LegacyOperation) -> Result<FeatureOperation, CodecError> {
    let family = match operation {
        LegacyOperation::TreeNode { .. } => return native("tree_node"),
        LegacyOperation::BaseFeature { .. } => return native("base_feature"),
        LegacyOperation::MeshImport { .. } => OperationFamily::Mesh,
        LegacyOperation::MeshGenerateFaceGroups { .. } => OperationFamily::Mesh,
        LegacyOperation::MeshCombineFaceGroups { .. } => OperationFamily::Mesh,
        LegacyOperation::MeshSection { .. } => OperationFamily::Mesh,
        LegacyOperation::MeshModify { .. } => OperationFamily::Mesh,
        LegacyOperation::MeshConvert(..) => OperationFamily::Mesh,
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
        LegacyOperation::DatumPrincipalPlane { .. } => OperationFamily::Datum,
        LegacyOperation::DatumPlane { .. } => OperationFamily::Datum,
        LegacyOperation::DatumThreePointPlane { .. } => OperationFamily::Datum,
        LegacyOperation::DatumOffsetPlane { .. } => OperationFamily::Datum,
        LegacyOperation::DatumAxis { .. } => OperationFamily::Datum,
        LegacyOperation::DatumPoint { .. } => OperationFamily::Datum,
        LegacyOperation::PointGeometry { .. } => OperationFamily::Datum,
        LegacyOperation::LineSegment { .. } => OperationFamily::Curve,
        LegacyOperation::CircularArc { .. } => OperationFamily::Curve,
        LegacyOperation::EllipticArc { .. } => OperationFamily::Curve,
        LegacyOperation::Polyline { .. } => OperationFamily::Curve,
        LegacyOperation::RegularPolygonCurve { .. } => OperationFamily::Curve,
        LegacyOperation::PlanarPatch { .. } => OperationFamily::Patch,
        LegacyOperation::FaceFromShapes { .. } => OperationFamily::Patch,
        LegacyOperation::DatumCoordinateSystem { .. } => OperationFamily::Datum,
        LegacyOperation::Block { .. } => OperationFamily::Primitive,
        LegacyOperation::EquationCurve { .. } => OperationFamily::Curve,
        LegacyOperation::ProjectedCurve { .. } => OperationFamily::Curve,
        LegacyOperation::ProjectOnSurface { .. } => OperationFamily::Curve,
        LegacyOperation::CompositeCurve { .. } => OperationFamily::Curve,
        LegacyOperation::Helix { .. } => OperationFamily::Curve,
        LegacyOperation::HelixNativeAxis { .. } => OperationFamily::Curve,
        LegacyOperation::Coil { .. } => OperationFamily::HelicalSweep,
        LegacyOperation::Sphere { .. } => OperationFamily::Primitive,
        LegacyOperation::Torus { .. } => OperationFamily::Primitive,
        LegacyOperation::Wrap { .. } => return native("wrap"),
        LegacyOperation::Sketch { .. } => OperationFamily::Sketch,
        LegacyOperation::SpatialSketch { .. } => OperationFamily::SpatialSketch,
        LegacyOperation::SketchBlockDefinition { .. } => OperationFamily::Sketch,
        LegacyOperation::SketchBlockInstance { .. } => OperationFamily::Sketch,
        LegacyOperation::StoredGeometry { .. } => return native("stored_geometry"),
        LegacyOperation::ExtractBody { .. } => OperationFamily::Extract,
        LegacyOperation::DerivedGeometry { .. } => return native("derived_geometry"),
        LegacyOperation::ImportedGeometry { .. } => return native("imported_geometry"),
        LegacyOperation::Primitive { .. } => OperationFamily::Primitive,
        LegacyOperation::Revolve { .. } => OperationFamily::Revolve,
        LegacyOperation::Sweep { .. } => OperationFamily::Sweep,
        LegacyOperation::HelicalSweep { .. } => OperationFamily::HelicalSweep,
        LegacyOperation::Binder { .. } => return native("binder"),
        LegacyOperation::Rib { .. } => OperationFamily::Rib,
        LegacyOperation::SheetMetalBaseFlange { .. } => OperationFamily::SheetMetal,
        LegacyOperation::SheetMetalAttachedPlate { .. } => OperationFamily::SheetMetal,
        LegacyOperation::SheetMetalBend { .. } => OperationFamily::SheetMetal,
        LegacyOperation::SheetMetalContourFlange { .. } => OperationFamily::SheetMetal,
        LegacyOperation::SheetMetalUnfold { .. } => OperationFamily::SheetMetal,
        LegacyOperation::SheetMetalRefold { .. } => OperationFamily::SheetMetal,
        LegacyOperation::SheetMetalBendLines { .. } => OperationFamily::SheetMetal,
        LegacyOperation::SheetMetalCorner { .. } => OperationFamily::SheetMetal,
        LegacyOperation::SheetMetalEdgeFlange(..) => OperationFamily::SheetMetal,
        LegacyOperation::SheetMetalHem { .. } => OperationFamily::SheetMetal,
        LegacyOperation::Fillet { .. } => OperationFamily::Fillet,
        LegacyOperation::FullRoundFillet { .. } => OperationFamily::Fillet,
        LegacyOperation::FaceBlend { .. } => OperationFamily::Fillet,
        LegacyOperation::Chamfer { .. } => OperationFamily::Chamfer,
        LegacyOperation::Shell { .. } => OperationFamily::Shell,
        LegacyOperation::OffsetShape { .. } => OperationFamily::Offset,
        LegacyOperation::Compound { .. } => return native("compound"),
        LegacyOperation::RefineShape { .. } => return native("refine_shape"),
        LegacyOperation::ReverseShape { .. } => return native("reverse_shape"),
        LegacyOperation::RuledBetweenCurves { .. } => OperationFamily::ImplicitSweep,
        LegacyOperation::SectionShape { .. } => OperationFamily::Curve,
        LegacyOperation::MirrorShape { .. } => OperationFamily::Pattern,
        LegacyOperation::Thicken { .. } => OperationFamily::Thicken,
        LegacyOperation::OffsetSurface { .. } => OperationFamily::Offset,
        LegacyOperation::KnitSurface { .. } => OperationFamily::Knit,
        LegacyOperation::SewBodies { .. } => OperationFamily::Knit,
        LegacyOperation::UnstitchFaces { .. } => OperationFamily::UnsewFaces,
        LegacyOperation::FilledSurface { .. } => OperationFamily::Patch,
        LegacyOperation::TrimSurface { .. } => OperationFamily::Split,
        LegacyOperation::ExtendSurface { .. } => OperationFamily::Extend,
        LegacyOperation::UntrimSurface { .. } => OperationFamily::Extend,
        LegacyOperation::RuledSurface { .. } => OperationFamily::ImplicitSweep,
        LegacyOperation::Draft { .. } => OperationFamily::Draft,
        LegacyOperation::Combine { .. } => OperationFamily::Combine,
        LegacyOperation::BoundaryFill { .. } => OperationFamily::BoundaryFill,
        LegacyOperation::CutWithSurface { .. } => OperationFamily::Split,
        LegacyOperation::TrimBodies { .. } => OperationFamily::Split,
        LegacyOperation::TrimBodiesAtPlane { .. } => OperationFamily::Split,
        LegacyOperation::SplitBody { .. } => OperationFamily::Split,
        LegacyOperation::SilhouetteSplit { .. } => OperationFamily::Split,
        LegacyOperation::SplitFace { .. } => OperationFamily::Split,
        LegacyOperation::DeleteBody { .. } => OperationFamily::DeleteBodies,
        LegacyOperation::DeleteFace { .. } => OperationFamily::DeleteFaces,
        LegacyOperation::ReplaceFace { .. } => OperationFamily::ReplaceFaces,
        LegacyOperation::MoveFace { .. } => OperationFamily::MoveFaces,
        LegacyOperation::MoveBody { .. } => OperationFamily::TransformBodies,
        LegacyOperation::Dome { .. } => OperationFamily::Dome,
        LegacyOperation::Flex { .. } => OperationFamily::Flex,
        LegacyOperation::Scale { .. } => OperationFamily::TransformBodies,
        LegacyOperation::Hole { .. } => OperationFamily::Hole,
        LegacyOperation::Pattern { .. } => OperationFamily::Pattern,
        LegacyOperation::Unresolved { family } => match family {
            UnresolvedFamily::DatumPlane => OperationFamily::Datum,
            UnresolvedFamily::DatumAxis => OperationFamily::Datum,
            UnresolvedFamily::DatumPoint => OperationFamily::Datum,
            UnresolvedFamily::DatumCoordinateSystem => OperationFamily::Datum,
            UnresolvedFamily::BridgeCurve => OperationFamily::Curve,
            UnresolvedFamily::Brep => return native("brep"),
            UnresolvedFamily::Cylinder => OperationFamily::Primitive,
            UnresolvedFamily::Cone => OperationFamily::Primitive,
            UnresolvedFamily::Sphere => OperationFamily::Primitive,
            UnresolvedFamily::Thread => OperationFamily::Thread,
            UnresolvedFamily::DetailedThread => OperationFamily::Thread,
            UnresolvedFamily::Loft => OperationFamily::Loft,
            UnresolvedFamily::ThroughCurveMesh => OperationFamily::Loft,
            UnresolvedFamily::FreeformSurface => OperationFamily::Patch,
            UnresolvedFamily::Extrude => OperationFamily::Extrude,
            UnresolvedFamily::Revolve => OperationFamily::Revolve,
            UnresolvedFamily::Combine => OperationFamily::Combine,
            UnresolvedFamily::Fillet => OperationFamily::Fillet,
            UnresolvedFamily::ExtractFace => OperationFamily::Extract,
            UnresolvedFamily::CopyFace => OperationFamily::Extract,
            UnresolvedFamily::LinkedFace => OperationFamily::Extract,
            UnresolvedFamily::FillHole => OperationFamily::Patch,
            UnresolvedFamily::BoundarySurface => OperationFamily::Patch,
            UnresolvedFamily::Draft => OperationFamily::Draft,
            UnresolvedFamily::DeleteFace => OperationFamily::DeleteFaces,
            UnresolvedFamily::MoveFace => OperationFamily::MoveFaces,
            UnresolvedFamily::MirrorFace => OperationFamily::Pattern,
            UnresolvedFamily::SubdivisionBody => OperationFamily::Form,
            UnresolvedFamily::TopologyOptimization => return native("topology_optimization"),
            UnresolvedFamily::MoveObject => OperationFamily::TransformBodies,
        },
        LegacyOperation::Native { kind, .. } => return native(kind.as_str()),
        LegacyOperation::Extrude { .. } => OperationFamily::Extrude,
        LegacyOperation::Loft { .. } => OperationFamily::Loft,
    };
    Ok(FeatureOperation::Unresolved { family })
}

#[cfg(test)]
mod tests;
