// SPDX-License-Identifier: Apache-2.0
//! Neutral evaluation of Siemens NX feature-history effects.

use crate::decode::feature_completeness;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_core::CodecError;

use std::collections::BTreeSet;

use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::features::{
    patterns::PatternSeed, BodyRetentionMode, BodySelection, BooleanOp, FeatureDefinition,
    FeatureId, FeatureOperation, UnresolvedFamily,
};
use cadmpeg_ir::ids::BodyId;

use crate::decode::feature_completeness::{
    output_free_local_body_construction, output_free_native_snapshot,
};
use serde::{Deserialize, Serialize};

/// Why a saved-body census cannot yet be evaluated exactly.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UnsupportedBodyCensusReason {
    /// A feature's active or suppressed state is unresolved.
    UnresolvedSuppression,
    /// The neutral feature family has no admitted body-effect evaluator.
    UnsupportedFeatureDefinition,
    /// Required construction semantics remain incomplete.
    IncompleteFeatureDefinition,
    /// Feature outputs do not form a coherent body-identity transition.
    InvalidOutputLineage,
    /// Feature ordinals or dependency directions do not form a replay order.
    InvalidHistoryOrder,
}

impl UnsupportedBodyCensusReason {
    /// Stable snake-case code for this boundary.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::UnresolvedSuppression => "unresolved_suppression",
            Self::UnsupportedFeatureDefinition => "unsupported_feature_definition",
            Self::IncompleteFeatureDefinition => "incomplete_feature_definition",
            Self::InvalidOutputLineage => "invalid_output_lineage",
            Self::InvalidHistoryOrder => "invalid_history_order",
        }
    }
}

/// Feature at an unsupported saved-body census boundary.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FeatureBoundary {
    /// Feature identity at the boundary.
    pub id: FeatureId,
    /// Feature display name, if any.
    pub name: Option<String>,
    /// Feature definition family, if any.
    pub family: Option<String>,
    /// Feature history ordinal.
    pub ordinal: u64,
}

/// Body identities in strictly increasing canonical order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CanonicalBodyCensus(Vec<BodyId>);

impl CanonicalBodyCensus {
    fn new(bodies: Vec<BodyId>) -> Result<Self, &'static str> {
        if bodies.windows(2).any(|pair| pair[0] >= pair[1]) {
            return Err("body census: identities must be unique and in canonical order");
        }
        Ok(Self(bodies))
    }

    /// Ordered body identities.
    pub fn as_slice(&self) -> &[BodyId] {
        &self.0
    }
}

/// Two canonical body censuses that differ.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BodyCensusDifference {
    rederived: CanonicalBodyCensus,
    saved: CanonicalBodyCensus,
}

impl BodyCensusDifference {
    /// Re-derived canonical body identities.
    pub fn rederived(&self) -> &[BodyId] {
        self.rederived.as_slice()
    }

    /// Saved canonical body identities.
    pub fn saved(&self) -> &[BodyId] {
        self.saved.as_slice()
    }
}

/// Result of evaluating neutral history against the saved current-body census.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    try_from = "BodyCensusEvaluationWire",
    into = "BodyCensusEvaluationWire"
)]
pub enum BodyCensusEvaluation {
    /// Neutral evaluation produced exactly the saved body identities.
    Verified {
        /// Re-derived body identities in canonical order.
        bodies: CanonicalBodyCensus,
    },
    /// Exact evaluation stopped at an unsupported semantic boundary.
    Unsupported {
        /// Feature at the boundary.
        feature: FeatureBoundary,
        /// Semantic boundary that prevented exact evaluation.
        reason: UnsupportedBodyCensusReason,
    },
    /// Active configuration requires configuration-local evaluation.
    ConfigurationEvaluation,
    /// Evaluation completed, but its body identities differ from the saved model.
    Mismatch {
        /// Distinct re-derived and saved canonical censuses.
        evidence: BodyCensusDifference,
    },
}

impl BodyCensusEvaluation {
    /// Admit a verified census with unique identities in canonical order.
    pub fn verified(bodies: Vec<BodyId>) -> Result<Self, &'static str> {
        Ok(Self::Verified {
            bodies: CanonicalBodyCensus::new(bodies)?,
        })
    }

    /// Admit unequal canonical censuses as mismatch evidence.
    pub fn mismatch(rederived: Vec<BodyId>, saved: Vec<BodyId>) -> Result<Self, &'static str> {
        let rederived = CanonicalBodyCensus::new(rederived)?;
        let saved = CanonicalBodyCensus::new(saved)?;
        if rederived == saved {
            return Err("body census: mismatch requires different censuses");
        }
        Ok(Self::Mismatch {
            evidence: BodyCensusDifference { rederived, saved },
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum BodyCensusEvaluationWire {
    Verified {
        bodies: Vec<BodyId>,
    },
    Unsupported {
        feature: Option<FeatureBoundary>,
        reason: String,
    },
    Mismatch {
        rederived: Vec<BodyId>,
        saved: Vec<BodyId>,
    },
}

impl TryFrom<BodyCensusEvaluationWire> for BodyCensusEvaluation {
    type Error = String;

    fn try_from(wire: BodyCensusEvaluationWire) -> Result<Self, Self::Error> {
        Ok(match wire {
            BodyCensusEvaluationWire::Verified { bodies } => {
                Self::verified(bodies).map_err(str::to_owned)?
            }
            BodyCensusEvaluationWire::Mismatch { rederived, saved } => {
                Self::mismatch(rederived, saved).map_err(str::to_owned)?
            }
            BodyCensusEvaluationWire::Unsupported { feature, reason } => {
                if reason == "configuration_evaluation" {
                    if feature.is_some() {
                        return Err(
                            "feature: configuration evaluation cannot name a feature".into()
                        );
                    }
                    Self::ConfigurationEvaluation
                } else {
                    let reason = UnsupportedBodyCensusReason::deserialize(
                        serde::de::value::StringDeserializer::<serde::de::value::Error>::new(
                            reason,
                        ),
                    )
                    .map_err(|error| format!("reason: {error}"))?;
                    Self::Unsupported {
                        feature: feature.ok_or("feature: required for feature evaluation")?,
                        reason,
                    }
                }
            }
        })
    }
}

impl From<BodyCensusEvaluation> for BodyCensusEvaluationWire {
    fn from(value: BodyCensusEvaluation) -> Self {
        match value {
            BodyCensusEvaluation::Verified { bodies } => Self::Verified { bodies: bodies.0 },
            BodyCensusEvaluation::Mismatch { evidence } => Self::Mismatch {
                rederived: evidence.rederived.0,
                saved: evidence.saved.0,
            },
            BodyCensusEvaluation::Unsupported { feature, reason } => Self::Unsupported {
                feature: Some(feature),
                reason: reason.as_str().into(),
            },
            BodyCensusEvaluation::ConfigurationEvaluation => Self::Unsupported {
                feature: None,
                reason: "configuration_evaluation".into(),
            },
        }
    }
}

/// Evaluate admitted neutral feature effects and compare their body identities
/// with the saved current model.
///
/// The caller must validate the IR first. This evaluator checks replay order,
/// operation completeness, and body lineage; it does not repeat topology or
/// selection-target validation.
pub(crate) fn evaluate_saved_body_census(
    ctx: &DecodeContext<'_>,
    ir: &CadIr,
) -> Result<BodyCensusEvaluation, CodecError> {
    let mut feature_storage = ctx.reserve_scoped(0, "NX body census feature order")?;
    let mut features = feature_storage.with_storage(|| {
        ctx.collect_vec(ir.model.features.iter(), "NX body census feature order")
    })?;
    ctx.stable_sort_by(
        &mut features,
        |value| &value.ordinal,
        Ord::cmp,
        "nx saved body census features sort",
    )?;
    let mut saved_storage = ctx.reserve_scoped(0, "NX saved body identity index")?;
    let mut saved = BTreeSet::new();
    for body in ctx.admit_iter(&ir.model.bodies, "NX saved body identity traversal")? {
        saved_storage.with_storage(|| {
            ctx.insert_btree_set(&mut saved, &body.id, "NX saved body identity index")
        })?;
    }
    let mut replay_storage = ctx.reserve_scoped(0, "NX replay body identity index")?;
    let rederived =
        match replay_storage.with_storage(|| rederived_body_census(ctx, &features, &saved)) {
            Ok(bodies) => bodies,
            Err(CensusError::Unsupported(feature, reason)) => {
                return Ok(BodyCensusEvaluation::Unsupported {
                    feature: feature_boundary(ctx, feature)?,
                    reason,
                })
            }
            Err(CensusError::Resource(error)) => return Err(error),
        };
    drop(features);
    drop(feature_storage);
    let same = rederived.len() == saved.len()
        && ctx.all_by(
            rederived.iter().zip(&saved),
            |(left, right)| ctx.equal(*left, *right, "NX body census identity comparison"),
            "NX body census comparison traversal",
        )?;
    if !same {
        return Ok(BodyCensusEvaluation::Mismatch {
            evidence: BodyCensusDifference {
                rederived: CanonicalBodyCensus(ctx.try_collect_vec(
                    rederived.into_iter().map(|body| {
                        body.try_clone_for_decode(ctx, "NX rederived census body identity")
                    }),
                    "NX rederived body census",
                )?),
                saved: CanonicalBodyCensus(ctx.try_collect_vec(
                    saved.into_iter().map(|body| {
                        body.try_clone_for_decode(ctx, "NX saved census body identity")
                    }),
                    "NX saved body census",
                )?),
            },
        });
    }
    if !active_configuration_is_admitted(ctx, ir, &saved)? {
        return Ok(BodyCensusEvaluation::ConfigurationEvaluation);
    }
    Ok(BodyCensusEvaluation::Verified {
        bodies: CanonicalBodyCensus(
            ctx.try_collect_vec(
                rederived
                    .into_iter()
                    .map(|body| body.try_clone_for_decode(ctx, "NX verified census body identity")),
                "NX verified body census",
            )?,
        ),
    })
}

fn feature_boundary(
    ctx: &DecodeContext<'_>,
    feature: &cadmpeg_ir::features::Feature,
) -> Result<FeatureBoundary, CodecError> {
    let family = match feature.evaluation.definition() {
        FeatureDefinition::PostProcess { .. } => "post_process",
        FeatureDefinition::Operation(operation) => match operation {
            FeatureOperation::TreeNode { .. } => "tree_node",
            FeatureOperation::BaseFeature { .. } => "base_feature",
            FeatureOperation::MeshImport { .. } => "mesh_import",
            FeatureOperation::InsertBodies { .. } => "insert_bodies",
            FeatureOperation::InsertComponent { .. } => "insert_component",
            FeatureOperation::AssemblyJoint { .. } => "assembly_joint",
            FeatureOperation::Form { .. } => "form",
            FeatureOperation::CosmeticThread { .. } => "cosmetic_thread",
            FeatureOperation::ReferenceImage { .. } => "reference_image",
            FeatureOperation::Decal { .. } => "decal",
            FeatureOperation::DatumPrincipalPlane { .. } => "datum_principal_plane",
            FeatureOperation::DatumPlane { .. } => "datum_plane",
            FeatureOperation::DatumThreePointPlane { .. } => "datum_three_point_plane",
            FeatureOperation::DatumOffsetPlane { .. } => "datum_offset_plane",
            FeatureOperation::DatumAxis { .. } => "datum_axis",
            FeatureOperation::DatumPoint { .. } => "datum_point",
            FeatureOperation::PointGeometry { .. } => "point_geometry",
            FeatureOperation::LineSegment { .. } => "line_segment",
            FeatureOperation::CircularArc { .. } => "circular_arc",
            FeatureOperation::EllipticArc { .. } => "elliptic_arc",
            FeatureOperation::Polyline { .. } => "polyline",
            FeatureOperation::RegularPolygonCurve { .. } => "regular_polygon_curve",
            FeatureOperation::PlanarPatch { .. } => "planar_patch",
            FeatureOperation::FaceFromShapes { .. } => "face_from_shapes",
            FeatureOperation::DatumCoordinateSystem { .. } => "datum_coordinate_system",
            FeatureOperation::Block { .. } => "block",
            FeatureOperation::EquationCurve { .. } => "equation_curve",
            FeatureOperation::ProjectedCurve { .. } => "projected_curve",
            FeatureOperation::ProjectOnSurface { .. } => "project_on_surface",
            FeatureOperation::CompositeCurve { .. } => "composite_curve",
            FeatureOperation::Helix { .. } => "helix",
            FeatureOperation::HelixNativeAxis { .. } => "helix_native_axis",
            FeatureOperation::Coil { .. } => "coil",
            FeatureOperation::Sphere { .. } => "sphere",
            FeatureOperation::Torus { .. } => "torus",
            FeatureOperation::Wrap { .. } => "wrap",
            FeatureOperation::Sketch { .. } => "sketch",
            FeatureOperation::SpatialSketch { .. } => "spatial_sketch",
            FeatureOperation::SketchBlockDefinition { .. } => "sketch_block_definition",
            FeatureOperation::SketchBlockInstance { .. } => "sketch_block_instance",
            FeatureOperation::StoredGeometry { .. } => "stored_geometry",
            FeatureOperation::ExtractBody { .. } => "extract_body",
            FeatureOperation::DerivedGeometry { .. } => "derived_geometry",
            FeatureOperation::ImportedGeometry { .. } => "imported_geometry",
            FeatureOperation::Primitive { .. } => "primitive",
            FeatureOperation::Revolve { .. } => "revolve",
            FeatureOperation::Sweep { .. } => "sweep",
            FeatureOperation::HelicalSweep { .. } => "helical_sweep",
            FeatureOperation::Binder { .. } => "binder",
            FeatureOperation::Rib { .. } => "rib",
            FeatureOperation::SheetMetalBaseFlange { .. } => "sheet_metal_base_flange",
            FeatureOperation::SheetMetalEdgeFlange { .. } => "sheet_metal_edge_flange",
            FeatureOperation::SheetMetalHem { .. } => "sheet_metal_hem",
            FeatureOperation::Fillet { .. } => "fillet",
            FeatureOperation::FullRoundFillet { .. } => "full_round_fillet",
            FeatureOperation::FaceBlend { .. } => "face_blend",
            FeatureOperation::Chamfer { .. } => "chamfer",
            FeatureOperation::Shell { .. } => "shell",
            FeatureOperation::OffsetShape { .. } => "offset_shape",
            FeatureOperation::Compound { .. } => "compound",
            FeatureOperation::RefineShape { .. } => "refine_shape",
            FeatureOperation::ReverseShape { .. } => "reverse_shape",
            FeatureOperation::RuledBetweenCurves { .. } => "ruled_between_curves",
            FeatureOperation::SectionShape { .. } => "section_shape",
            FeatureOperation::MirrorShape { .. } => "mirror_shape",
            FeatureOperation::Thicken { .. } => "thicken",
            FeatureOperation::OffsetSurface { .. } => "offset_surface",
            FeatureOperation::KnitSurface { .. } => "knit_surface",
            FeatureOperation::SewBodies { .. } => "sew_bodies",
            FeatureOperation::FilledSurface { .. } => "filled_surface",
            FeatureOperation::TrimSurface { .. } => "trim_surface",
            FeatureOperation::ExtendSurface { .. } => "extend_surface",
            FeatureOperation::RuledSurface { .. } => "ruled_surface",
            FeatureOperation::Draft { .. } => "draft",
            FeatureOperation::Combine { .. } => "combine",
            FeatureOperation::BoundaryFill { .. } => "boundary_fill",
            FeatureOperation::CutWithSurface { .. } => "cut_with_surface",
            FeatureOperation::TrimBodies { .. } => "trim_bodies",
            FeatureOperation::SplitBody { .. } => "split_body",
            FeatureOperation::SplitFace { .. } => "split_face",
            FeatureOperation::DeleteBody { .. } => "delete_body",
            FeatureOperation::DeleteFace { .. } => "delete_face",
            FeatureOperation::ReplaceFace { .. } => "replace_face",
            FeatureOperation::MoveFace { .. } => "move_face",
            FeatureOperation::MoveBody { .. } => "move_body",
            FeatureOperation::Dome { .. } => "dome",
            FeatureOperation::Flex { .. } => "flex",
            FeatureOperation::Scale { .. } => "scale",
            FeatureOperation::Hole { .. } => "hole",
            FeatureOperation::Pattern { .. } => "pattern",
            FeatureOperation::Unresolved { .. } => "unresolved",
            FeatureOperation::Native { .. } => "native",
            FeatureOperation::Extrude { .. } => "extrude",
            FeatureOperation::Loft { .. } => "loft",
        },
    };
    Ok(FeatureBoundary {
        id: feature
            .id
            .try_clone_for_decode(ctx, "NX census boundary feature identity")?,
        name: feature
            .name
            .as_deref()
            .map(|name| ctx.copy_retained_text(name, "NX census boundary feature name"))
            .transpose()?,
        family: Some(ctx.copy_retained_text(family, "NX census boundary feature family")?),
        ordinal: feature.ordinal,
    })
}

#[derive(Debug)]
enum CensusError<'ir> {
    Unsupported(
        &'ir cadmpeg_ir::features::Feature,
        UnsupportedBodyCensusReason,
    ),
    Resource(CodecError),
}

impl From<CodecError> for CensusError<'_> {
    fn from(error: CodecError) -> Self {
        Self::Resource(error)
    }
}

impl From<cadmpeg_core::decode::ResourceLimit> for CensusError<'_> {
    fn from(error: cadmpeg_core::decode::ResourceLimit) -> Self {
        Self::Resource(CodecError::ResourceLimit(error))
    }
}

/// Saved-body census evidence for the profile harness.
#[doc(hidden)]
pub fn saved_body_census_evidence(
    ir: &CadIr,
    policy: &DecodePolicy,
) -> Result<BodyCensusEvaluation, CodecError> {
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, policy)?;
    evaluate_saved_body_census(&ctx, ir)
}

fn active_configuration_is_admitted(
    ctx: &DecodeContext<'_>,
    ir: &CadIr,
    saved: &BTreeSet<&BodyId>,
) -> Result<bool, CodecError> {
    if ir.model.configurations.is_empty() {
        return Ok(true);
    }
    if saved.is_empty()
        && ctx.all_by(
            &ir.model.features,
            |feature| is_body_neutral_feature(ctx, feature),
            "NX empty census neutral features",
        )?
    {
        return Ok(true);
    }
    let Some((index, configuration)) = ctx.find_by(
        ir.model.configurations.iter().enumerate(),
        |(_, configuration)| Ok(configuration.active),
        "NX active configuration selection",
    )?
    else {
        return Ok(false);
    };
    let Some(configuration_bodies) = configuration.bodies.as_deref() else {
        return Ok(false);
    };
    if ctx.any_by(
        &ir.model.configurations[index + 1..],
        |configuration| Ok(configuration.active),
        "NX extra active configuration selection",
    )? {
        return Ok(false);
    }
    if configuration_bodies.len() != saved.len()
        || !ctx.all_by(
            configuration_bodies,
            |body| ctx.contains_btree_set(saved, &body, "NX active configuration body lookup"),
            "NX active configuration bodies",
        )?
    {
        return Ok(false);
    }
    Ok(ctx.all_by(
        &ir.model.features,
        |feature| is_body_neutral_feature(ctx, feature),
        "NX active configuration neutral features",
    )? || !feature_completeness::active_configuration_state_is_incomplete(
        ctx,
        ir,
        configuration,
    )?)
}

fn rederived_body_census<'ir>(
    ctx: &DecodeContext<'_>,
    features: &[&'ir cadmpeg_ir::features::Feature],
    saved_bodies: &BTreeSet<&BodyId>,
) -> Result<BTreeSet<&'ir BodyId>, CensusError<'ir>> {
    let mut bodies = BTreeSet::new();
    let mut seen_storage = ctx.reserve_scoped(0, "NX replay feature identity index")?;
    let mut seen_features = BTreeSet::new();
    let mut previous_ordinal = None;
    let mut features = features.iter();
    while let Some(&feature) = ctx.next_charged(&mut features, "NX body census replay traversal")? {
        if ctx.contains_btree_set(
            &seen_features,
            &(&feature.id),
            "NX repeated replay feature lookup",
        )? || previous_ordinal.is_some_and(|ordinal| feature.ordinal <= ordinal)
            || ctx.any_by(
                feature.dependencies.as_slice(),
                |dependency| {
                    Ok(!ctx.contains_btree_set(
                        &seen_features,
                        &dependency,
                        "NX replay dependency lookup",
                    )?)
                },
                "NX replay dependency traversal",
            )?
        {
            return Err(CensusError::Unsupported(
                feature,
                UnsupportedBodyCensusReason::InvalidHistoryOrder,
            ));
        }
        previous_ordinal = Some(feature.ordinal);
        seen_storage.with_storage(|| {
            ctx.insert_btree_set(
                &mut seen_features,
                &feature.id,
                "NX replay feature identity index",
            )
        })?;
        match feature.suppressed {
            None if !is_body_neutral_feature(ctx, feature)?
                && !suppression_is_body_census_invariant(ctx, feature, &bodies)? =>
            {
                return Err(CensusError::Unsupported(
                    feature,
                    UnsupportedBodyCensusReason::UnresolvedSuppression,
                ));
            }
            None => {}
            Some(true) => continue,
            Some(false) => {}
        }
        match feature.evaluation.definition() {
            FeatureDefinition::Operation(
                FeatureOperation::TreeNode { .. }
                | FeatureOperation::DatumPrincipalPlane { .. }
                | FeatureOperation::DatumPlane { .. }
                | FeatureOperation::Unresolved {
                    family:
                        UnresolvedFamily::DatumPlane
                        | UnresolvedFamily::DatumAxis
                        | UnresolvedFamily::DatumPoint
                        | UnresolvedFamily::DatumCoordinateSystem
                        | UnresolvedFamily::BridgeCurve,
                }
                | FeatureOperation::DatumOffsetPlane { .. }
                | FeatureOperation::DatumAxis { .. }
                | FeatureOperation::DatumPoint { .. }
                | FeatureOperation::DatumCoordinateSystem { .. }
                | FeatureOperation::Sketch { .. }
                | FeatureOperation::ProjectedCurve { .. }
                | FeatureOperation::SectionShape { .. },
            ) => {
                if !feature.evaluation.outputs().is_empty() {
                    return Err(CensusError::Unsupported(
                        feature,
                        UnsupportedBodyCensusReason::InvalidOutputLineage,
                    ));
                }
            }
            FeatureDefinition::Operation(FeatureOperation::Native { kind, .. })
                if kind.as_str() == "DELETE"
                    && !ctx.contains_key_btree_map(
                        &feature.source_properties,
                        "primary_body_object_index",
                        "NX replay delete body property",
                    )? => {}
            FeatureDefinition::Operation(FeatureOperation::Native { kind, .. })
                if kind.as_str() == "FSET" && feature.evaluation.outputs().is_empty() => {}
            FeatureDefinition::Operation(FeatureOperation::Block {
                dimensions: Some(_),
                placement: Some(_),
                op: BooleanOp::NewBody,
            }) => {
                let [output] = feature.evaluation.outputs().as_slice() else {
                    return Err(CensusError::Unsupported(
                        feature,
                        UnsupportedBodyCensusReason::InvalidOutputLineage,
                    ));
                };
                if !ctx.insert_btree_set(&mut bodies, output, "NX new body replay identity")? {
                    return Err(CensusError::Unsupported(
                        feature,
                        UnsupportedBodyCensusReason::InvalidOutputLineage,
                    ));
                }
            }
            FeatureDefinition::Operation(FeatureOperation::Block {
                dimensions: Some(_),
                placement: Some(_),
                op: BooleanOp::Join | BooleanOp::Cut | BooleanOp::Intersect,
            }) => {
                preserve_in_place_single_output(ctx, feature, &bodies, saved_bodies)?;
            }
            FeatureDefinition::Operation(FeatureOperation::Block {
                dimensions: Some(_),
                placement: Some(_),
                op: BooleanOp::Unresolved,
            }) if matches!(feature.evaluation.outputs().as_slice(), [output] if ctx.contains_btree_set(&bodies, &output, "NX replay output body lookup")?) =>
            {
                preserve_in_place_single_output(ctx, feature, &bodies, saved_bodies)?;
            }
            FeatureDefinition::Operation(FeatureOperation::Block { .. }) => {
                return Err(CensusError::Unsupported(
                    feature,
                    UnsupportedBodyCensusReason::IncompleteFeatureDefinition,
                ));
            }
            FeatureDefinition::Operation(FeatureOperation::Sphere { op, .. }) => {
                apply_complete_boolean_outputs(
                    ctx,
                    feature,
                    &mut bodies,
                    *op,
                    feature_completeness::sphere_definition_is_incomplete(feature),
                )?;
            }
            FeatureDefinition::Operation(FeatureOperation::Unresolved {
                family: UnresolvedFamily::Loft | UnresolvedFamily::FreeformSurface,
            }) if feature.evaluation.outputs().is_empty() => {}
            FeatureDefinition::Operation(FeatureOperation::Unresolved {
                family: UnresolvedFamily::Brep,
            }) if feature.evaluation.outputs().is_empty() => {}
            FeatureDefinition::Operation(FeatureOperation::Unresolved {
                family: UnresolvedFamily::Brep,
            }) => {
                return Err(CensusError::Unsupported(
                    feature,
                    UnsupportedBodyCensusReason::IncompleteFeatureDefinition,
                ));
            }
            FeatureDefinition::Operation(FeatureOperation::Unresolved {
                family: UnresolvedFamily::DeleteFace,
            }) if feature.evaluation.outputs().is_empty() => {}
            FeatureDefinition::Operation(FeatureOperation::Unresolved {
                family: UnresolvedFamily::DeleteFace,
            }) => {
                preserve_in_place_single_output(ctx, feature, &bodies, saved_bodies)?;
            }
            FeatureDefinition::Operation(FeatureOperation::Unresolved {
                family: UnresolvedFamily::MirrorFace,
            }) if feature.evaluation.outputs().is_empty() => {}
            FeatureDefinition::Operation(FeatureOperation::Unresolved {
                family: UnresolvedFamily::MirrorFace,
            }) => {
                preserve_in_place_single_output(ctx, feature, &bodies, saved_bodies)?;
            }
            FeatureDefinition::Operation(FeatureOperation::Unresolved {
                family: UnresolvedFamily::SubdivisionBody,
            }) if feature.evaluation.outputs().is_empty() => {}
            FeatureDefinition::Operation(FeatureOperation::Unresolved {
                family: UnresolvedFamily::SubdivisionBody,
            }) => {
                return Err(CensusError::Unsupported(
                    feature,
                    UnsupportedBodyCensusReason::IncompleteFeatureDefinition,
                ));
            }
            FeatureDefinition::Operation(FeatureOperation::Unresolved {
                family: UnresolvedFamily::TopologyOptimization,
            }) if feature.evaluation.outputs().is_empty() => {}
            FeatureDefinition::Operation(FeatureOperation::Unresolved {
                family: UnresolvedFamily::TopologyOptimization,
            }) => {
                return Err(CensusError::Unsupported(
                    feature,
                    UnsupportedBodyCensusReason::IncompleteFeatureDefinition,
                ));
            }
            FeatureDefinition::Operation(FeatureOperation::Loft { op, .. }) => {
                apply_complete_boolean_outputs(
                    ctx,
                    feature,
                    &mut bodies,
                    *op,
                    feature_completeness::loft_definition_is_incomplete(ctx, feature)?,
                )?;
            }
            FeatureDefinition::Operation(FeatureOperation::Extrude {
                op: BooleanOp::Unresolved | BooleanOp::Join | BooleanOp::Cut | BooleanOp::Intersect,
                ..
            }) if matches!(feature.evaluation.outputs().as_slice(), [output] if ctx.contains_btree_set(&bodies, &output, "NX replay output body lookup")?) =>
            {
                preserve_in_place_single_output(ctx, feature, &bodies, saved_bodies)?;
            }
            FeatureDefinition::Operation(FeatureOperation::Extrude { .. })
                if output_free_local_body_construction(ctx, feature)? => {}
            FeatureDefinition::Operation(FeatureOperation::Extrude { op, .. }) => {
                apply_complete_boolean_outputs(
                    ctx,
                    feature,
                    &mut bodies,
                    *op,
                    feature_completeness::extrude_definition_is_incomplete(ctx, feature)?,
                )?;
            }
            FeatureDefinition::Operation(FeatureOperation::Revolve { .. })
                if output_free_local_body_construction(ctx, feature)? => {}
            FeatureDefinition::Operation(FeatureOperation::Revolve { op, .. }) => {
                apply_complete_boolean_outputs(
                    ctx,
                    feature,
                    &mut bodies,
                    *op,
                    feature_completeness::revolve_definition_is_incomplete(ctx, feature)?,
                )?;
            }
            FeatureDefinition::Operation(FeatureOperation::Rib { .. })
                if output_free_local_body_construction(ctx, feature)? => {}
            FeatureDefinition::Operation(FeatureOperation::Rib { op, .. }) => {
                apply_complete_boolean_outputs(
                    ctx,
                    feature,
                    &mut bodies,
                    *op,
                    feature_completeness::rib_definition_is_incomplete(ctx, feature)?,
                )?;
            }
            FeatureDefinition::Operation(FeatureOperation::Sweep { .. })
                if output_free_local_body_construction(ctx, feature)? => {}
            FeatureDefinition::Operation(FeatureOperation::Sweep { shape, .. }) => {
                let mode = shape.mode();
                let op = match mode {
                    cadmpeg_ir::features::SweepMode::Solid { op } => op.into(),
                    cadmpeg_ir::features::SweepMode::Surface {} => BooleanOp::NewBody,
                    cadmpeg_ir::features::SweepMode::Unresolved {} => BooleanOp::Unresolved,
                };
                apply_complete_boolean_outputs(
                    ctx,
                    feature,
                    &mut bodies,
                    op,
                    feature_completeness::sweep_definition_is_incomplete(ctx, feature)?,
                )?;
            }
            FeatureDefinition::Operation(FeatureOperation::BaseFeature { .. })
                if output_free_native_snapshot(ctx, feature)? => {}
            FeatureDefinition::Operation(FeatureOperation::BaseFeature {
                bodies: BodySelection::Resolved { bodies, .. },
            }) if bodies.is_empty() && feature.evaluation.outputs().is_empty() => {}
            FeatureDefinition::Operation(FeatureOperation::BaseFeature { bodies: selection }) => {
                let Some(selected) = explicit_body_selection(selection) else {
                    return Err(CensusError::Unsupported(
                        feature,
                        UnsupportedBodyCensusReason::IncompleteFeatureDefinition,
                    ));
                };
                if !selected.matches(ctx, feature.evaluation.outputs())?
                    || ctx.any_by(
                        selected.iter(),
                        |body| {
                            ctx.contains_btree_set(
                                &bodies,
                                &body,
                                "NX selected existing body lookup",
                            )
                        },
                        "NX selected existing body traversal",
                    )?
                {
                    return Err(CensusError::Unsupported(
                        feature,
                        UnsupportedBodyCensusReason::InvalidOutputLineage,
                    ));
                }
                let mut selected = selected.iter();
                while let Some(body) =
                    ctx.next_charged(&mut selected, "NX selected body insertion traversal")?
                {
                    ctx.insert_btree_set(&mut bodies, body, "NX selected body replay identity")?;
                }
            }
            FeatureDefinition::Operation(FeatureOperation::InsertBodies { bodies: selection }) => {
                let selected = feature.evaluation.outputs();
                if !selection.is_resolved() || selected.is_empty() {
                    return Err(CensusError::Unsupported(
                        feature,
                        UnsupportedBodyCensusReason::IncompleteFeatureDefinition,
                    ));
                }
                if ctx.any_by(
                    selected.iter(),
                    |body| {
                        ctx.contains_btree_set(&bodies, &body, "NX selected existing body lookup")
                    },
                    "NX selected existing body traversal",
                )? {
                    return Err(CensusError::Unsupported(
                        feature,
                        UnsupportedBodyCensusReason::InvalidOutputLineage,
                    ));
                }
                let mut selected = selected.iter();
                while let Some(body) =
                    ctx.next_charged(&mut selected, "NX selected body insertion traversal")?
                {
                    ctx.insert_btree_set(&mut bodies, body, "NX selected body replay identity")?;
                }
            }
            FeatureDefinition::Operation(FeatureOperation::ExtractBody { source }) => {
                if feature.evaluation.outputs().is_empty() && complete_local_body_selection(source)
                {
                    continue;
                }
                let Some(sources) = explicit_body_selection(source) else {
                    return Err(CensusError::Unsupported(
                        feature,
                        UnsupportedBodyCensusReason::IncompleteFeatureDefinition,
                    ));
                };
                if sources.len() != feature.evaluation.outputs().len()
                    || ctx.any_by(
                        sources.iter(),
                        |body| {
                            Ok(!ctx.contains_btree_set(
                                &bodies,
                                &body,
                                "NX extraction source body lookup",
                            )?)
                        },
                        "NX extraction source body traversal",
                    )?
                    || ctx.any_by(
                        feature.evaluation.outputs(),
                        |body| {
                            ctx.contains_btree_set(
                                &bodies,
                                &body,
                                "NX extraction existing output body lookup",
                            )
                        },
                        "NX extraction output body traversal",
                    )?
                {
                    return Err(CensusError::Unsupported(
                        feature,
                        UnsupportedBodyCensusReason::InvalidOutputLineage,
                    ));
                }
                for output in ctx.admit_iter(
                    feature.evaluation.outputs(),
                    "NX replay output insertion traversal",
                )? {
                    ctx.insert_btree_set(&mut bodies, output, "NX replay output identity")?;
                }
            }
            FeatureDefinition::Operation(FeatureOperation::TrimSurface { .. }) => {
                preserve_in_place_outputs(ctx, feature, &bodies, saved_bodies)?;
            }
            FeatureDefinition::Operation(FeatureOperation::ExtendSurface { .. }) => {
                preserve_in_place_outputs(ctx, feature, &bodies, saved_bodies)?;
            }
            FeatureDefinition::Operation(FeatureOperation::Hole { .. }) => {
                preserve_in_place_single_output(ctx, feature, &bodies, saved_bodies)?;
            }
            FeatureDefinition::Operation(FeatureOperation::Chamfer { .. }) => {
                preserve_in_place_single_output(ctx, feature, &bodies, saved_bodies)?;
            }
            FeatureDefinition::Operation(FeatureOperation::Fillet { .. }) => {
                preserve_in_place_single_output(ctx, feature, &bodies, saved_bodies)?;
            }
            FeatureDefinition::Operation(FeatureOperation::FaceBlend { .. }) => {
                preserve_in_place_single_output(ctx, feature, &bodies, saved_bodies)?;
            }
            FeatureDefinition::Operation(FeatureOperation::OffsetSurface { .. }) => {
                preserve_in_place_single_output(ctx, feature, &bodies, saved_bodies)?;
            }
            FeatureDefinition::Operation(FeatureOperation::Thicken { .. }) => {
                preserve_in_place_single_output(ctx, feature, &bodies, saved_bodies)?;
            }
            FeatureDefinition::Operation(FeatureOperation::Draft { .. }) => {
                preserve_in_place_single_output(ctx, feature, &bodies, saved_bodies)?;
            }
            FeatureDefinition::Operation(FeatureOperation::Unresolved {
                family: UnresolvedFamily::Draft,
            }) if output_free_local_body_construction(ctx, feature)? => {}
            FeatureDefinition::Operation(FeatureOperation::ReplaceFace { .. }) => {
                preserve_in_place_single_output(ctx, feature, &bodies, saved_bodies)?;
            }
            FeatureDefinition::Operation(FeatureOperation::Combine { operands, .. })
                if feature.evaluation.outputs().is_empty()
                    && complete_local_or_native_body_selection(ctx, operands.target())?
                    && complete_local_or_native_body_selection(ctx, operands.tools())? => {}
            FeatureDefinition::Operation(FeatureOperation::Combine { operands, .. })
                if local_tool_combine_is_census_invariant(
                    ctx,
                    feature,
                    operands.target(),
                    operands.tools(),
                    &bodies,
                )? => {}
            FeatureDefinition::Operation(FeatureOperation::Combine {
                operands,

                keep_tools,
                ..
            }) => {
                let target = operands.target();
                let tools = operands.tools();
                apply_complete_body_combine(
                    ctx,
                    feature,
                    &mut bodies,
                    target,
                    tools,
                    if *keep_tools {
                        ToolRetention::Keep
                    } else {
                        ToolRetention::Delete
                    },
                    feature_completeness::combine_definition_is_incomplete(feature),
                )?;
            }
            FeatureDefinition::Operation(FeatureOperation::SewBodies {
                bodies: selection, ..
            }) => {
                if local_body_replacement_is_census_invariant(ctx, feature, selection, &bodies)? {
                    continue;
                }
                apply_complete_body_replacement(
                    ctx,
                    feature,
                    &mut bodies,
                    selection,
                    feature_completeness::sew_bodies_definition_is_incomplete(feature),
                )?;
            }
            FeatureDefinition::Operation(FeatureOperation::TrimBodies { operands, .. }) => {
                let targets = operands.targets();
                let tools = operands.tools();
                if feature.evaluation.outputs().is_empty() {
                    continue;
                }
                preserve_complete_body_targets(
                    ctx,
                    feature,
                    &bodies,
                    targets,
                    tools,
                    feature_completeness::trim_bodies_definition_is_incomplete(feature),
                )?;
            }
            FeatureDefinition::Operation(FeatureOperation::DeleteBody {
                bodies: selection,
                mode,
            }) => {
                apply_complete_body_retention(
                    ctx,
                    feature,
                    &mut bodies,
                    selection,
                    ResolvedBodyRetentionMode::try_from(*mode)
                        .map_err(|reason| CensusError::Unsupported(feature, reason))?,
                    feature_completeness::operands::body_selection_is_incomplete(selection),
                )?;
            }
            FeatureDefinition::Operation(FeatureOperation::Pattern { seeds, pattern }) => {
                if feature.evaluation.outputs().is_empty() {
                    continue;
                }
                apply_complete_body_pattern(
                    ctx,
                    feature,
                    &mut bodies,
                    seeds,
                    feature_completeness::operands::pattern_occurrence_count(ctx, pattern)?,
                    feature_completeness::operands::pattern_feature_is_incomplete(
                        ctx,
                        seeds,
                        pattern,
                        &feature.dependencies,
                    )?,
                )?;
            }
            _ => {
                return Err(CensusError::Unsupported(
                    feature,
                    UnsupportedBodyCensusReason::UnsupportedFeatureDefinition,
                ));
            }
        }
    }
    Ok(bodies)
}

fn is_body_neutral_feature(
    ctx: &DecodeContext<'_>,
    feature: &cadmpeg_ir::features::Feature,
) -> Result<bool, CodecError> {
    if !feature.evaluation.outputs().is_empty() {
        return Ok(false);
    }
    Ok(match feature.evaluation.definition() {
        FeatureDefinition::Operation(
            FeatureOperation::TreeNode { .. }
            | FeatureOperation::DatumPrincipalPlane { .. }
            | FeatureOperation::DatumPlane { .. }
            | FeatureOperation::Unresolved {
                family:
                    UnresolvedFamily::DatumPlane
                    | UnresolvedFamily::DatumAxis
                    | UnresolvedFamily::DatumPoint
                    | UnresolvedFamily::DatumCoordinateSystem
                    | UnresolvedFamily::BridgeCurve,
            }
            | FeatureOperation::DatumOffsetPlane { .. }
            | FeatureOperation::DatumAxis { .. }
            | FeatureOperation::DatumPoint { .. }
            | FeatureOperation::DatumCoordinateSystem { .. }
            | FeatureOperation::Sketch { .. }
            | FeatureOperation::ProjectedCurve { .. }
            | FeatureOperation::SectionShape { .. },
        ) => true,
        FeatureDefinition::Operation(FeatureOperation::Native { kind, .. }) => {
            (kind.as_str() == "DELETE"
                && !ctx.contains_key_btree_map(
                    &feature.source_properties,
                    "primary_body_object_index",
                    "NX neutral delete body property",
                )?)
                || kind.as_str() == "FSET"
        }
        _ => false,
    })
}

fn suppression_is_body_census_invariant(
    ctx: &DecodeContext<'_>,
    feature: &cadmpeg_ir::features::Feature,
    bodies: &BTreeSet<&BodyId>,
) -> Result<bool, CodecError> {
    let output_free = feature.evaluation.outputs().is_empty();
    let in_place = || -> Result<bool, CodecError> {
        Ok(output_free
            || (feature.evaluation.outputs().len() == 1
                && ctx.contains_btree_set(
                    bodies,
                    &(&feature.evaluation.outputs()[0]),
                    "NX suppression output body lookup",
                )?))
    };
    Ok(match feature.evaluation.definition() {
        FeatureDefinition::Operation(FeatureOperation::DeleteBody {
            bodies: BodySelection::Local { .. },
            mode: BodyRetentionMode::DeleteSelected,
        }) => true,
        FeatureDefinition::Operation(FeatureOperation::ExtractBody { source }) => {
            output_free && complete_local_body_selection(source)
        }
        FeatureDefinition::Operation(FeatureOperation::SewBodies {
            bodies: selection, ..
        }) => local_body_replacement_is_census_invariant(ctx, feature, selection, bodies)?,
        FeatureDefinition::Operation(FeatureOperation::TrimBodies { .. }) => output_free,
        FeatureDefinition::Operation(FeatureOperation::Pattern { .. }) => {
            feature_completeness::output_free_pattern_construction(ctx, feature)?
                || output_free_local_body_construction(ctx, feature)?
        }
        FeatureDefinition::Operation(FeatureOperation::Combine { operands, .. }) => {
            (output_free
                && complete_local_or_native_body_selection(ctx, operands.target())?
                && complete_local_or_native_body_selection(ctx, operands.tools())?)
                || local_tool_combine_is_census_invariant(
                    ctx,
                    feature,
                    operands.target(),
                    operands.tools(),
                    bodies,
                )?
        }
        FeatureDefinition::Operation(FeatureOperation::Extrude { op, .. }) => {
            output_free_local_body_construction(ctx, feature)?
                || (matches!(
                    op,
                    BooleanOp::Unresolved | BooleanOp::Join | BooleanOp::Cut | BooleanOp::Intersect
                ) && feature.evaluation.outputs().len() == 1
                    && ctx.contains_btree_set(
                        bodies,
                        &(&feature.evaluation.outputs()[0]),
                        "NX suppression extrude body lookup",
                    )?)
        }
        FeatureDefinition::Operation(
            FeatureOperation::Revolve { .. }
            | FeatureOperation::Rib { .. }
            | FeatureOperation::Sweep { .. },
        ) => output_free_local_body_construction(ctx, feature)?,
        FeatureDefinition::Operation(FeatureOperation::BaseFeature { .. }) => {
            output_free_native_snapshot(ctx, feature)?
        }
        FeatureDefinition::Operation(FeatureOperation::Unresolved {
            family: UnresolvedFamily::Brep,
        }) => output_free,
        FeatureDefinition::Operation(
            FeatureOperation::TrimSurface { .. }
            | FeatureOperation::ExtendSurface { .. }
            | FeatureOperation::Hole { .. }
            | FeatureOperation::Chamfer { .. }
            | FeatureOperation::Fillet { .. }
            | FeatureOperation::FaceBlend { .. }
            | FeatureOperation::OffsetSurface { .. }
            | FeatureOperation::Thicken { .. }
            | FeatureOperation::Draft { .. }
            | FeatureOperation::ReplaceFace { .. }
            | FeatureOperation::Unresolved {
                family:
                    UnresolvedFamily::Loft
                    | UnresolvedFamily::FreeformSurface
                    | UnresolvedFamily::DeleteFace
                    | UnresolvedFamily::MirrorFace
                    | UnresolvedFamily::SubdivisionBody
                    | UnresolvedFamily::TopologyOptimization,
            },
        ) => in_place()?,
        FeatureDefinition::Operation(FeatureOperation::Unresolved {
            family: UnresolvedFamily::Draft,
        }) => output_free_local_body_construction(ctx, feature)? || in_place()?,
        _ => false,
    })
}

fn complete_local_or_native_body_selection(
    ctx: &DecodeContext<'_>,
    selection: &BodySelection,
) -> Result<bool, CodecError> {
    Ok(match selection {
        BodySelection::Local { .. } | BodySelection::NativeSet(_) => true,
        BodySelection::Native(native) => !ctx
            .trim_text(native, "NX native body selection text")?
            .is_empty(),
        _ => false,
    })
}

fn local_tool_combine_is_census_invariant(
    ctx: &DecodeContext<'_>,
    feature: &cadmpeg_ir::features::Feature,
    target: &BodySelection,
    tools: &BodySelection,
    bodies: &BTreeSet<&BodyId>,
) -> Result<bool, CodecError> {
    let Some(selected_target) = explicit_body_selection(target) else {
        return Ok(false);
    };
    if selected_target.len() != 1 || !matches!(tools, BodySelection::Local { .. }) {
        return Ok(false);
    }
    let Some(target) = selected_target.iter().next() else {
        return Ok(false);
    };
    Ok(selected_target.matches(ctx, feature.evaluation.outputs())?
        && ctx.contains_btree_set(bodies, &target, "NX local-tool combine target lookup")?)
}

/// Validate the exact body-identity effect of an in-place edit independently
/// of construction semantics that cannot alter that identity transition.
fn preserve_in_place_single_output<'ir>(
    ctx: &DecodeContext<'_>,
    feature: &'ir cadmpeg_ir::features::Feature,
    bodies: &BTreeSet<&BodyId>,
    saved_bodies: &BTreeSet<&BodyId>,
) -> Result<(), CensusError<'ir>> {
    preserve_in_place_outputs(ctx, feature, bodies, saved_bodies)?;
    if feature.evaluation.outputs().len() > 1 {
        return Err(CensusError::Unsupported(
            feature,
            UnsupportedBodyCensusReason::InvalidOutputLineage,
        ));
    }
    Ok(())
}

fn preserve_in_place_outputs<'ir>(
    ctx: &DecodeContext<'_>,
    feature: &'ir cadmpeg_ir::features::Feature,
    bodies: &BTreeSet<&BodyId>,
    saved_bodies: &BTreeSet<&BodyId>,
) -> Result<(), CensusError<'ir>> {
    // A retained body image can be the final saved output of an in-place edit
    // even when no replay writer has established it yet. In-place operations
    // never create a body, so accept that terminal identity without inserting
    // it into the replay set. An identity absent from both sets remains an
    // invalid output lineage.
    if ctx.any_by(
        feature.evaluation.outputs(),
        |output| {
            Ok(
                !ctx.contains_btree_set(bodies, &output, "NX in-place replay body lookup")?
                    && !ctx.contains_btree_set(
                        saved_bodies,
                        &output,
                        "NX in-place saved body lookup",
                    )?,
            )
        },
        "NX in-place output traversal",
    )? {
        return Err(CensusError::Unsupported(
            feature,
            UnsupportedBodyCensusReason::InvalidOutputLineage,
        ));
    }
    Ok(())
}

fn apply_complete_boolean_outputs<'ir>(
    ctx: &DecodeContext<'_>,
    feature: &'ir cadmpeg_ir::features::Feature,
    bodies: &mut BTreeSet<&'ir BodyId>,
    op: BooleanOp,
    incomplete: bool,
) -> Result<(), CensusError<'ir>> {
    if incomplete || matches!(op, BooleanOp::Unresolved) {
        return Err(CensusError::Unsupported(
            feature,
            UnsupportedBodyCensusReason::IncompleteFeatureDefinition,
        ));
    }
    if feature.evaluation.outputs().is_empty() {
        return Err(CensusError::Unsupported(
            feature,
            UnsupportedBodyCensusReason::InvalidOutputLineage,
        ));
    }
    match op {
        BooleanOp::NewBody
            if ctx.all_by(
                feature.evaluation.outputs(),
                |output| {
                    Ok(!ctx.contains_btree_set(
                        bodies,
                        &output,
                        "NX new boolean output body lookup",
                    )?)
                },
                "NX new boolean output traversal",
            )? =>
        {
            for output in ctx.admit_iter(
                feature.evaluation.outputs(),
                "NX replay output insertion traversal",
            )? {
                ctx.insert_btree_set(bodies, output, "NX replay output identity")?;
            }
        }
        BooleanOp::Join | BooleanOp::Cut | BooleanOp::Intersect
            if ctx.all_by(
                feature.evaluation.outputs(),
                |output| {
                    ctx.contains_btree_set(
                        bodies,
                        &output,
                        "NX in-place boolean output body lookup",
                    )
                },
                "NX in-place boolean output traversal",
            )? => {}
        _ => {
            return Err(CensusError::Unsupported(
                feature,
                UnsupportedBodyCensusReason::InvalidOutputLineage,
            ));
        }
    }
    Ok(())
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ToolRetention {
    Keep,
    Delete,
}

fn apply_complete_body_combine<'ir>(
    ctx: &DecodeContext<'_>,
    feature: &'ir cadmpeg_ir::features::Feature,
    bodies: &mut BTreeSet<&'ir BodyId>,
    target: &'ir BodySelection,
    tools: &'ir BodySelection,
    tool_retention: ToolRetention,
    incomplete: bool,
) -> Result<(), CensusError<'ir>> {
    if incomplete {
        return Err(CensusError::Unsupported(
            feature,
            UnsupportedBodyCensusReason::IncompleteFeatureDefinition,
        ));
    }
    let (Some(targets), Some(tools)) = (
        explicit_body_selection(target),
        explicit_body_selection(tools),
    ) else {
        return Err(CensusError::Unsupported(
            feature,
            UnsupportedBodyCensusReason::IncompleteFeatureDefinition,
        ));
    };
    if targets.len() != 1 {
        return Err(CensusError::Unsupported(
            feature,
            UnsupportedBodyCensusReason::InvalidOutputLineage,
        ));
    };
    let Some(target) = targets.iter().next() else {
        return Err(CensusError::Unsupported(
            feature,
            UnsupportedBodyCensusReason::InvalidOutputLineage,
        ));
    };
    if !targets.matches(ctx, feature.evaluation.outputs())?
        || !ctx.contains_btree_set(bodies, &target, "NX combine target body lookup")?
        || ctx.any_by(
            tools.iter(),
            |tool| Ok(!ctx.contains_btree_set(bodies, &tool, "NX tool body lookup")?),
            "NX tool body traversal",
        )?
    {
        return Err(CensusError::Unsupported(
            feature,
            UnsupportedBodyCensusReason::InvalidOutputLineage,
        ));
    }
    if tool_retention == ToolRetention::Delete {
        let mut selected = tools.iter();
        while let Some(tool) =
            ctx.next_charged(&mut selected, "NX replay body removal traversal")?
        {
            ctx.remove_btree_set(bodies, &tool, "NX replay body removal lookup")?;
        }
    }
    Ok(())
}

fn apply_complete_body_replacement<'ir>(
    ctx: &DecodeContext<'_>,
    feature: &'ir cadmpeg_ir::features::Feature,
    bodies: &mut BTreeSet<&'ir BodyId>,
    inputs: &'ir BodySelection,
    incomplete: bool,
) -> Result<(), CensusError<'ir>> {
    if incomplete {
        return Err(CensusError::Unsupported(
            feature,
            UnsupportedBodyCensusReason::IncompleteFeatureDefinition,
        ));
    }
    let Some(inputs) = explicit_body_selection(inputs) else {
        return Err(CensusError::Unsupported(
            feature,
            UnsupportedBodyCensusReason::IncompleteFeatureDefinition,
        ));
    };
    let mut input_storage = ctx.reserve_scoped(0, "NX replacement input identity index")?;
    let mut input_set = BTreeSet::new();
    let mut input_ids = inputs.iter();
    while let Some(input) =
        ctx.next_charged(&mut input_ids, "NX replacement input identity traversal")?
    {
        input_storage.with_storage(|| {
            ctx.insert_btree_set(&mut input_set, input, "NX replacement input identity index")
        })?;
    }
    if ctx.any_by(
        inputs.iter(),
        |input| Ok(!ctx.contains_btree_set(bodies, &input, "NX replacement input body lookup")?),
        "NX replacement input body traversal",
    )? || feature.evaluation.outputs().is_empty()
        || ctx.any_by(
            feature.evaluation.outputs(),
            |output| {
                Ok(ctx.contains_btree_set(
                    bodies,
                    &output,
                    "NX replacement existing output lookup",
                )? && !ctx.contains_btree_set(
                    &input_set,
                    &output,
                    "NX replacement input identity lookup",
                )?)
            },
            "NX replacement output traversal",
        )?
    {
        return Err(CensusError::Unsupported(
            feature,
            UnsupportedBodyCensusReason::InvalidOutputLineage,
        ));
    }
    let mut selected = inputs.iter();
    while let Some(input) = ctx.next_charged(&mut selected, "NX replay body removal traversal")? {
        ctx.remove_btree_set(bodies, &input, "NX replay body removal lookup")?;
    }
    for output in ctx.admit_iter(
        feature.evaluation.outputs(),
        "NX replay output insertion traversal",
    )? {
        ctx.insert_btree_set(bodies, output, "NX replay output identity")?;
    }
    Ok(())
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ResolvedBodyRetentionMode {
    DeleteSelected,
    KeepSelected,
}

impl TryFrom<BodyRetentionMode> for ResolvedBodyRetentionMode {
    type Error = UnsupportedBodyCensusReason;

    fn try_from(mode: BodyRetentionMode) -> Result<Self, Self::Error> {
        match mode {
            BodyRetentionMode::DeleteSelected => Ok(Self::DeleteSelected),
            BodyRetentionMode::KeepSelected => Ok(Self::KeepSelected),
            BodyRetentionMode::Unresolved => {
                Err(UnsupportedBodyCensusReason::IncompleteFeatureDefinition)
            }
        }
    }
}

fn apply_complete_body_retention<'ir>(
    ctx: &DecodeContext<'_>,
    feature: &'ir cadmpeg_ir::features::Feature,
    bodies: &mut BTreeSet<&'ir BodyId>,
    selection: &'ir BodySelection,
    mode: ResolvedBodyRetentionMode,
    incomplete: bool,
) -> Result<(), CensusError<'ir>> {
    if incomplete {
        return Err(CensusError::Unsupported(
            feature,
            UnsupportedBodyCensusReason::IncompleteFeatureDefinition,
        ));
    }
    if mode == ResolvedBodyRetentionMode::DeleteSelected
        && matches!(selection, BodySelection::Local { .. })
        && feature.evaluation.outputs().is_empty()
    {
        return Ok(());
    }
    let Some(selected) = explicit_body_selection(selection) else {
        return Err(CensusError::Unsupported(
            feature,
            UnsupportedBodyCensusReason::IncompleteFeatureDefinition,
        ));
    };
    if !feature.evaluation.outputs().is_empty()
        || ctx.any_by(
            selected.iter(),
            |body| Ok(!ctx.contains_btree_set(bodies, &body, "NX selected body lookup")?),
            "NX selected body traversal",
        )?
    {
        return Err(CensusError::Unsupported(
            feature,
            UnsupportedBodyCensusReason::InvalidOutputLineage,
        ));
    }
    match mode {
        ResolvedBodyRetentionMode::DeleteSelected => {
            let mut selected = selected.iter();
            while let Some(body) = ctx.next_charged(&mut selected, "NX deleted body traversal")? {
                ctx.remove_btree_set(bodies, &body, "NX deleted body lookup")?;
            }
        }
        ResolvedBodyRetentionMode::KeepSelected => {
            let mut storage = ctx.reserve_scoped(0, "NX kept body identity index")?;
            let mut kept = BTreeSet::new();
            let mut selected = selected.iter();
            while let Some(body) =
                ctx.next_charged(&mut selected, "NX kept body identity traversal")?
            {
                storage.with_storage(|| {
                    ctx.insert_btree_set(&mut kept, body, "NX kept body identity index")
                })?;
            }
            ctx.retain_btree_set(
                bodies,
                |body| ctx.contains_btree_set(&kept, body, "NX retained body identity lookup"),
                "NX body retention traversal",
            )?;
        }
    }
    Ok(())
}

fn preserve_complete_body_targets<'ir>(
    ctx: &DecodeContext<'_>,
    feature: &'ir cadmpeg_ir::features::Feature,
    bodies: &BTreeSet<&BodyId>,
    targets: &'ir BodySelection,
    tools: &'ir BodySelection,
    incomplete: bool,
) -> Result<(), CensusError<'ir>> {
    if incomplete {
        return Err(CensusError::Unsupported(
            feature,
            UnsupportedBodyCensusReason::IncompleteFeatureDefinition,
        ));
    }
    let (Some(targets), Some(tools)) = (
        explicit_body_selection(targets),
        explicit_body_selection(tools),
    ) else {
        return Err(CensusError::Unsupported(
            feature,
            UnsupportedBodyCensusReason::IncompleteFeatureDefinition,
        ));
    };
    if !targets.matches(ctx, feature.evaluation.outputs())?
        || ctx.any_by(
            targets.iter(),
            |target| Ok(!ctx.contains_btree_set(bodies, &target, "NX target body lookup")?),
            "NX target body traversal",
        )?
        || ctx.any_by(
            tools.iter(),
            |tool| Ok(!ctx.contains_btree_set(bodies, &tool, "NX tool body lookup")?),
            "NX tool body traversal",
        )?
    {
        return Err(CensusError::Unsupported(
            feature,
            UnsupportedBodyCensusReason::InvalidOutputLineage,
        ));
    }
    Ok(())
}

fn apply_complete_body_pattern<'ir>(
    ctx: &DecodeContext<'_>,
    feature: &'ir cadmpeg_ir::features::Feature,
    bodies: &mut BTreeSet<&'ir BodyId>,
    seeds: &'ir [PatternSeed],
    occurrence_count: Option<usize>,
    incomplete: bool,
) -> Result<(), CensusError<'ir>> {
    if incomplete {
        return Err(CensusError::Unsupported(
            feature,
            UnsupportedBodyCensusReason::IncompleteFeatureDefinition,
        ));
    }
    let Some(occurrence_count) = occurrence_count else {
        return Err(CensusError::Unsupported(
            feature,
            UnsupportedBodyCensusReason::IncompleteFeatureDefinition,
        ));
    };
    if ctx.any_by(
        seeds,
        |seed| Ok(!matches!(seed, PatternSeed::Bodies(_))),
        "NX pattern seed kind traversal",
    )? {
        return Err(CensusError::Unsupported(
            feature,
            UnsupportedBodyCensusReason::UnsupportedFeatureDefinition,
        ));
    }
    let mut seed_storage = ctx.reserve_scoped(0, "NX pattern seed body index")?;
    let mut seed_bodies = BTreeSet::new();
    let mut seed_count = 0usize;
    let mut duplicate_seeds = false;
    let mut seeds = seeds.iter();
    while let Some(seed) = ctx.next_charged(&mut seeds, "NX pattern body seed traversal")? {
        let selection = match seed {
            PatternSeed::Bodies(selection) => explicit_body_selection(selection),
            _ => None,
        };
        let Some(selection) = selection else {
            return Err(CensusError::Unsupported(
                feature,
                UnsupportedBodyCensusReason::IncompleteFeatureDefinition,
            ));
        };
        seed_count = seed_count
            .checked_add(selection.len())
            .ok_or_else(|| ctx.refuse_codec_limit("NX pattern seed body count", 0, u64::MAX))?;
        let mut selected = selection.iter();
        while let Some(body) =
            ctx.next_charged(&mut selected, "NX pattern seed identity traversal")?
        {
            duplicate_seeds |= !seed_storage.with_storage(|| {
                ctx.insert_btree_set(&mut seed_bodies, body, "NX pattern seed body index")
            })?;
        }
    }
    let Some(copies) = occurrence_count.checked_sub(1) else {
        return Err(CensusError::Unsupported(
            feature,
            UnsupportedBodyCensusReason::InvalidOutputLineage,
        ));
    };
    if seed_count.checked_mul(copies) != Some(feature.evaluation.outputs().len())
        || duplicate_seeds
        || ctx.any_by(
            &seed_bodies,
            |body| {
                Ok(!ctx.contains_btree_set(bodies, body, "NX pattern seed replay body lookup")?)
            },
            "NX pattern seed replay body traversal",
        )?
        || ctx.any_by(
            feature.evaluation.outputs(),
            |output| {
                ctx.contains_btree_set(bodies, &output, "NX pattern existing output body lookup")
            },
            "NX pattern output body traversal",
        )?
    {
        return Err(CensusError::Unsupported(
            feature,
            UnsupportedBodyCensusReason::InvalidOutputLineage,
        ));
    }
    for output in ctx.admit_iter(
        feature.evaluation.outputs(),
        "NX pattern output insertion traversal",
    )? {
        ctx.insert_btree_set(bodies, output, "NX pattern output body index")?;
    }
    Ok(())
}

#[derive(Clone, Copy)]
enum ExplicitBodies<'ir> {
    Ids(&'ir [BodyId]),
    Members(&'ir cadmpeg_ir::features::BodyMembers<BodyId>),
}

impl<'ir> ExplicitBodies<'ir> {
    fn len(self) -> usize {
        match self {
            Self::Ids(ids) => ids.len(),
            Self::Members(members) => members.count(),
        }
    }
    fn iter(self) -> impl Iterator<Item = &'ir BodyId> + Clone {
        let (ids, rows) = match self {
            Self::Ids(ids) => (ids, [].iter()),
            Self::Members(members) => (&[][..], members.iter()),
        };
        ids.iter()
            .chain(rows.map(cadmpeg_ir::features::BodyMember::body))
    }
    fn matches(self, ctx: &DecodeContext<'_>, outputs: &[BodyId]) -> Result<bool, CodecError> {
        Ok(self.len() == outputs.len()
            && ctx.all_by(
                self.iter().zip(outputs),
                |(selected, output)| {
                    ctx.equal(selected, output, "NX selected body identity comparison")
                },
                "NX selected body comparison traversal",
            )?)
    }
}

fn explicit_body_selection(selection: &BodySelection) -> Option<ExplicitBodies<'_>> {
    match selection {
        BodySelection::Bodies(bodies) | BodySelection::Resolved { bodies, .. } => {
            (!bodies.is_empty()).then_some(ExplicitBodies::Ids(bodies.as_slice()))
        }
        BodySelection::ResolvedSet { members } => Some(ExplicitBodies::Members(members)),
        _ => None,
    }
}

fn complete_local_body_selection(selection: &BodySelection) -> bool {
    // Local members and their enclosing reference are immutable checked values.
    matches!(selection, BodySelection::Local { .. })
}

fn local_body_replacement_is_census_invariant(
    ctx: &DecodeContext<'_>,
    feature: &cadmpeg_ir::features::Feature,
    selection: &BodySelection,
    bodies: &BTreeSet<&BodyId>,
) -> Result<bool, CodecError> {
    Ok(complete_local_body_selection(selection)
        && ctx.all_by(
            feature.evaluation.outputs(),
            |output| {
                ctx.contains_btree_set(bodies, &output, "NX local body replacement output lookup")
            },
            "NX local body replacement output traversal",
        )?)
}

#[cfg(test)]
mod tests;
