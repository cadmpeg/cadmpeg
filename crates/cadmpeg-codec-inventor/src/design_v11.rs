// SPDX-License-Identifier: Apache-2.0
//! The v11 history of an Inventor part.

use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::text::NonBlankString;
use cadmpeg_core::CodecError;
use cadmpeg_ir::design::container::{Container, OrderedKind, Rollback, UnorderedKind};
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

pub(crate) mod assembly;
mod datums;
pub(crate) mod evaluations;
mod operations;
mod parameters;
pub(crate) mod sheet_metal;
mod sketches;
mod topologies;

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
    project_history(ctx, ir, &flags, !features.sheet_metal_styles.is_empty())?;
    parameters::project(ctx, ir, parameters, units)?;
    losses.extend(sketches::project(ctx, ir)?);
    datums::project(ctx, ir)?;
    sheet_metal::project(ctx, ir, features, parameters, losses)?;
    operations::project(ctx, ir, losses)?;
    sheet_metal::attachments::project(ir, features)?;
    losses.extend(sheet_metal::feature_reliefs(ir, features, parameters));
    place_current_bodies(ir)?;
    topologies::restate(ctx, ir)?;
    Ok(())
}

pub(crate) fn unproduced_bodies(ir: &CadIr) -> usize {
    ir.design
        .scopes
        .iter()
        .map(|scope| scope.unproduced.len())
        .sum()
}

/// Whether a member before the rollback of `timeline` can change a state
/// (§5.4), so that the member produces the timeline's current bodies (§5.5).
pub(crate) fn timeline_produces(ir: &CadIr, timeline: &Container) -> Result<bool, CodecError> {
    let Container::Ordered {
        members, rollback, ..
    } = timeline
    else {
        return Ok(false);
    };
    let end = match rollback {
        None | Some(Rollback::End) => members.len(),
        Some(Rollback::Before(member)) => members
            .iter()
            .position(|candidate| candidate == member)
            .ok_or_else(|| CodecError::malformed("Inventor v11 rollback names no member"))?,
    };
    for member in &members[..end] {
        let Member::Feature(id) = member else {
            continue;
        };
        if !ir
            .is_stateless(id, None)
            .map_err(|error| CodecError::malformed(error.to_string()))?
        {
            return Ok(true);
        }
    }
    Ok(false)
}

/// Checks each part scope's states through the IR's checked state resolver:
/// every current state and evaluation resolves, and the part owns every
/// body of a current state's extent.
pub(crate) fn validate_states(
    ir: &CadIr,
    limits: &cadmpeg_core::decode::ResourceLimits,
) -> Vec<cadmpeg_ir::report::check::Finding> {
    let mut findings = Vec::new();
    let mut fail = |message: String, entity: Option<String>| {
        findings.push(cadmpeg_ir::report::check::Finding {
            check: cadmpeg_ir::report::check::Check::NativeLinks,
            rule: cadmpeg_ir::rule_name!("inventor/design_v11.state-admission"),
            severity: cadmpeg_ir::report::Severity::Error,
            message,
            entity,
            related: Vec::new(),
        });
    };
    for scope in &ir.design.scopes {
        for current in &scope.current {
            let state = match ir.resolved_current_state(&scope.definition, &current.at) {
                Ok(state) => state,
                Err(error) => {
                    fail(
                        format!("Inventor v11 current state does not resolve: {error}"),
                        Some(scope.definition.as_str().to_owned()),
                    );
                    continue;
                }
            };
            if state
                .owners()
                .values()
                .any(|owner| owner != &scope.definition)
            {
                fail(
                    "Inventor v11 current state holds a body the part does not own".into(),
                    Some(scope.definition.as_str().to_owned()),
                );
            }
        }
        for evaluation in ir
            .design
            .evaluations
            .iter()
            .filter(|evaluation| evaluation.scope == scope.definition)
        {
            if let Err(error) = ir.resolved_evaluation_state(&evaluation.id, limits) {
                fail(
                    format!("Inventor v11 evaluation does not resolve: {error}"),
                    Some(evaluation.id.as_str().to_owned()),
                );
            }
        }
    }
    findings
}

pub(crate) fn validate_operations(
    ctx: &DecodeContext<'_>,
    ir: &CadIr,
    features: &crate::feature::FeatureInventory,
    sketches: &crate::sketch::SketchInventory,
    parameters: &[crate::design::PmDcParameter],
    units: &[crate::design::PmDcUnit],
) -> Result<Vec<cadmpeg_ir::report::check::Finding>, CodecError> {
    // The projection reads the arenas the CADIR wire omits.
    if !ir.model.omitted_arenas_held() {
        return Ok(Vec::new());
    }
    let mut expected = CadIr::empty();
    expected
        .model
        .product_definitions
        .clone_from(&ir.model.product_definitions);
    expected.model.features.clone_from(&ir.model.features);
    expected.model.parameters.clone_from(&ir.model.parameters);
    expected.model.sketches.clone_from(&ir.model.sketches);
    expected
        .model
        .sketch_entities
        .clone_from(&ir.model.sketch_entities);
    expected
        .model
        .sketch_constraints
        .clone_from(&ir.model.sketch_constraints);
    expected
        .model
        .feature_input_topologies
        .clone_from(&ir.model.feature_input_topologies);
    expected.geometry.bodies.clone_from(&ir.geometry.bodies);
    project(
        ctx,
        &mut expected,
        features,
        sketches,
        parameters,
        units,
        &mut Vec::new(),
    )?;
    // The evaluations come from the decoded history states, which this
    // check does not rebuild; the state check resolves each one.
    expected
        .design
        .evaluations
        .clone_from(&ir.design.evaluations);
    if ir.design == expected.design {
        return Ok(Vec::new());
    }
    let actual = cadmpeg_core::serde_erase::to_json_value(&ir.design)
        .map_err(|error| CodecError::malformed(error.to_string()))?;
    let expected = cadmpeg_core::serde_erase::to_json_value(&expected.design)
        .map_err(|error| CodecError::malformed(error.to_string()))?;
    let detail = cadmpeg_ir::compare::values_agree(&actual, &expected)
        .err()
        .unwrap_or_else(|| "exact scalar values differ".to_owned());
    Ok(vec![cadmpeg_ir::report::check::Finding {
        rule: cadmpeg_ir::rule_name!(
            "inventor/design_v11.inventor-v11-design-differs-admitted-source"
        ),
        check: cadmpeg_ir::report::check::Check::NativeLinks,
        severity: cadmpeg_ir::report::Severity::Error,
        message: format!("Inventor v11 design differs from its admitted source operands: {detail}"),
        entity: None,
        related: Vec::new(),
    }])
}

fn project_history(
    ctx: &DecodeContext<'_>,
    ir: &mut CadIr,
    flags: &BTreeMap<String, u32>,
    has_rules: bool,
) -> Result<(), CodecError> {
    // A part without features, parameters, or sheet-metal rules has no
    // design history, so it gets no scope or timeline.
    if ir.model.features.is_empty() && ir.model.parameters.is_empty() && !has_rules {
        return Ok(());
    }
    let Some(part) = ir.model.product_definitions.first() else {
        return Ok(());
    };
    let definition = part.id.clone();
    let owned_bodies = part.bodies.iter().collect::<BTreeSet<_>>();
    let mut ordered = ir.model.features.iter().collect::<Vec<_>>();
    ordered.sort_by_key(|feature| (feature.ordinal, &feature.id));
    let mut owned = ir
        .geometry
        .bodies
        .iter()
        .filter(|body| owned_bodies.contains(&body.id))
        .map(|body| body.id.clone())
        .collect::<Vec<_>>();
    owned.sort();
    let mut roots = Vec::new();
    let mut current = Vec::new();
    for (flat, key) in [(false, "root"), (true, "flat-pattern")] {
        let history = ordered
            .iter()
            .copied()
            .filter(|feature| is_flat_pattern(feature) == flat && !is_block_definition(feature))
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
                bodies: Vec::new(),
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
            rollback: Some(rollback),
            placement: None,
        });
    }
    let blocks = ordered
        .iter()
        .filter(|feature| is_block_definition(feature))
        .map(|feature| Member::Feature(feature.id.clone()))
        .collect::<BTreeSet<_>>();
    if !blocks.is_empty() {
        ctx.charge_entities(1, "project Inventor Blocks group")?;
        let id =
            ContainerId::mint("inventor:document:group#blocks").map_err(CodecError::malformed)?;
        roots.push(id.clone());
        ir.design.containers.push(Container::Unordered {
            id,
            kind: UnorderedKind::Group,
            members: blocks,
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
    ctx.charge_entities(1, "project Inventor v11 scope")?;
    ir.design.features.sort_by(|a, b| a.id.cmp(&b.id));
    ir.design.containers.sort_by(|a, b| a.id().cmp(b.id()));
    let unproduced = owned.into_iter().collect();
    ir.design.scopes.push(DesignScope {
        definition,
        roots,
        active_configuration: None,
        active_arrangements: BTreeSet::default(),
        occurrences: BTreeMap::default(),
        current,
        unproduced,
    });
    Ok(())
}

/// Moves each part scope's bodies from `unproduced` to its root current
/// state when a member of the root timeline before its rollback can change a
/// state (§5.5). Runs after the last operation stage, because the predicate
/// reads the final operations.
fn place_current_bodies(ir: &mut CadIr) -> Result<(), CodecError> {
    for index in 0..ir.design.scopes.len() {
        let Some(at) = ir.design.scopes[index]
            .current
            .first()
            .map(|state| state.at.clone())
        else {
            continue;
        };
        let produces = match timeline_of(ir, &at) {
            Some(timeline) => timeline_produces(ir, timeline)?,
            None => false,
        };
        if produces {
            let scope = &mut ir.design.scopes[index];
            let bodies = std::mem::take(&mut scope.unproduced);
            if let Some(state) = scope.current.first_mut() {
                state.bodies = bodies.into_iter().collect();
            }
        }
    }
    Ok(())
}

/// The ordered container that holds the position `at`.
fn timeline_of<'ir>(ir: &'ir CadIr, at: &Position) -> Option<&'ir Container> {
    match at {
        Position::Start(id) => ir.design.containers.iter().find(|c| c.id() == id),
        Position::After(member) => {
            ir.design.containers.iter().find(
                |c| matches!(c, Container::Ordered { members, .. } if members.contains(member)),
            )
        }
    }
}

fn is_block_definition(feature: &LegacyFeature) -> bool {
    matches!(
        feature.evaluation.definition().operation(),
        LegacyOperation::SketchBlockDefinition { .. }
    )
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
    native_with_options(kind, BTreeMap::default())
}

fn native_with_options(
    kind: &str,
    native_options: cadmpeg_ir::design::options::NativeOptions,
) -> Result<FeatureOperation, CodecError> {
    Ok(FeatureOperation::Native {
        kind: cadmpeg_ir::design::names::CodecName::new(&cadmpeg_ir::codec_id!("inventor"), kind)
            .ok_or_else(|| CodecError::malformed("Inventor v11 operation kind is blank"))?,
        native_options,
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
        LegacyOperation::Native { kind, parameters } => {
            if parameters.is_empty() {
                return native(kind.as_str());
            }
            let text = cadmpeg_ir::hash::finite_json::to_canonical_json_string(parameters)
                .map_err(|error| CodecError::malformed(error.to_string()))?;
            let text = NonBlankString::new(text)
                .ok_or_else(|| CodecError::malformed("Inventor native parameters are blank"))?;
            return native_with_options(
                kind.as_str(),
                BTreeMap::from([(
                    cadmpeg_ir::codec_name!("inventor:parameters"),
                    cadmpeg_ir::design::options::NativeOption::Text(Dimension::Literal(text)),
                )]),
            );
        }
        LegacyOperation::Extrude { .. } => OperationFamily::Extrude,
        LegacyOperation::Loft { .. } => OperationFamily::Loft,
    };
    Ok(FeatureOperation::Unresolved { family })
}

#[cfg(test)]
mod tests;
