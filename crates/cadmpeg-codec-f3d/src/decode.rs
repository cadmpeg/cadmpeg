// SPDX-License-Identifier: Apache-2.0
#![cfg_attr(
    test,
    allow(clippy::default_trait_access, clippy::field_reassign_with_default)
)]
//! Assemble a `.f3d` archive into a [`CadIr`] document and [`DecodeBody`].
//!
//! [`crate::container`] scans the ZIP, reads ASM headers, finds the history
//! boundary. This module resolves Design body-to-blob bindings, frames every
//! referenced B-rep with [`cadmpeg_asm::sab`], builds topology and geometry through
//! [`crate::brep`], then
//! adds design, sketch, history, ACT, and appearance data.
//!
//! A framing failure or a stream without decoded geometry produces a
//! metadata-only document. The report marks geometry and topology as blocking,
//! and retained source data remains available for native replay.
use cadmpeg_core::decode::u64_from_index;

use cadmpeg_ir::annotations::StreamHandle;
use cadmpeg_ir::features::{PlanarProfileRef, ProfileRef};

use cadmpeg_core::container::ContainerRole;

use crate::native::F3dNative;
use cadmpeg_asm::brep::transfer::{transfer_into_ir, AsmTransferRemainder};
use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::CodecError;
use cadmpeg_ir::annotations::AnnotationBuilder;
use cadmpeg_ir::codec::{DecodeBody, Decoded};
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::ids::UnknownId;
use cadmpeg_ir::report::{
    loss::{LossCategory, LossTaxonomy},
    Severity,
};
use cadmpeg_ir::units::{Tolerances, UnitVector3};
use cadmpeg_ir::unknown::UnknownRecord;

use crate::brep::{self, Brep};
use crate::container::{self, BrepFacts, ContainerScan};
use crate::loss::F3dLossCode;
use crate::materials;
use cadmpeg_asm::{asm_header, sab};

fn index_selected_body_key(
    ctx: &DecodeContext<'_>,
    index: &mut std::collections::HashMap<String, std::collections::BTreeSet<u64>>,
    blob_name: &str,
    body_key: u64,
) -> Result<(), CodecError> {
    if let Some(keys) = ctx.get_mut_hash_map(index, blob_name, "find F3D selected body keys")? {
        return ctx
            .insert_btree_set(keys, body_key, "index F3D selected body keys")
            .map(|_| ());
    }

    let name = ctx.copy_retained_text(blob_name, "retain F3D selected body blob name")?;
    let entry = ctx.entry_hash_map(index, name, "index F3D selected body blobs")?;
    let mut keys = std::collections::BTreeSet::new();
    ctx.insert_btree_set(&mut keys, body_key, "index F3D selected body keys")
        .map(|_| ())?;
    entry.or_insert(keys);
    Ok(())
}

fn body_visibility_for<'m>(
    ctx: &DecodeContext<'_>,
    index: &'m std::collections::HashMap<
        (String, u64),
        crate::design::decode::body::DecodedBodyVisibility,
    >,
    blob_name: &str,
    body_key: u64,
) -> Result<Option<&'m crate::design::decode::body::DecodedBodyVisibility>, CodecError> {
    let operation = "look up F3D body visibility";
    let (name, _reservation) = ctx.format_scoped(format_args!("{blob_name}"), operation)?;
    ctx.get_hash_map(index, &(name, body_key), operation)
}

fn count_result_items<T>(
    ctx: &DecodeContext<'_>,
    mut values: impl Iterator<Item = Result<T, CodecError>>,
    operation: &'static str,
) -> Result<usize, CodecError> {
    values.try_fold(0usize, |count, value| {
        value?;
        count
            .checked_add(1)
            .ok_or_else(|| ctx.refuse_codec_limit(operation, 0, u64::MAX))
    })
}

fn join_text_brep_names(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan<'_>,
) -> Result<String, CodecError> {
    let count = count_result_items(
        ctx,
        container::text_brep_names(ctx, scan)?,
        "count F3D text BREP carriers",
    )?;
    let length = container::text_brep_names(ctx, scan)?.try_fold(0usize, |length, name| {
        let name = name?;
        length
            .checked_add(name.len())
            .ok_or_else(|| ctx.refuse_codec_limit("join F3D text B-rep names", 0, u64::MAX))
    })?;
    let separators = (if count == 0 { 0 } else { count - 1 })
        .checked_mul("`, `".len())
        .ok_or_else(|| ctx.refuse_codec_limit("join F3D text B-rep names", 0, u64::MAX))?;
    let length = length
        .checked_add(separators)
        .ok_or_else(|| ctx.refuse_codec_limit("join F3D text B-rep names", 0, u64::MAX))?;
    let mut joined = ctx.retained_string(length, "join F3D text B-rep names")?;
    for (index, name) in container::text_brep_names(ctx, scan)?.enumerate() {
        let name = name?;
        if index != 0 {
            ctx.append_retained(&mut joined, "`, `", "copy F3D text BREP name separator")?;
        }
        ctx.append_retained(&mut joined, name, "copy F3D text BREP name")?;
    }
    Ok(joined)
}

fn container_only_dimension_parameters(
    ctx: &DecodeContext<'_>,
    native: &F3dNative,
) -> Result<std::collections::HashSet<cadmpeg_ir::features::ParameterId>, CodecError> {
    let container_only = crate::design::dimensions::container_only_dimension_companions(
        ctx,
        &native.design_dimension_locus_pairs,
        &native.design_dimension_null_locus_pairs,
        &native.design_dimension_annotation_frames,
        &native.design_dimension_locus_groups,
        &native.design_dimension_recipe_records,
    )?;
    let mut parameters_by_id = std::collections::HashSet::new();
    for owner in ctx.admit_iter(
        &native.design_parameter_owners,
        "scan F3D native design parameter owners",
    )? {
        let stream = crate::ids::native_stream(owner.id()).unwrap_or(crate::ids::DEFAULT_STREAM);
        if !ctx.contains_hash_set(
            &container_only,
            &(stream, owner.companion_record_index()),
            "find F3D container-only dimension companion",
        )? {
            continue;
        }
        let mut matches_parameter = |parameter: &crate::records::parameters::DesignParameter| {
            Ok(ctx.equal(
                crate::ids::native_stream(&parameter.id).unwrap_or(crate::ids::DEFAULT_STREAM),
                stream,
                "compare F3D dimension parameter streams",
            )? && parameter.record_index == owner.parameter_record_index()
                && parameter.kind() == crate::records::parameters::DesignParameterKind::Dimension)
        };
        let Some(parameter_index) = ctx.position_by(
            &native.design_parameters,
            &mut matches_parameter,
            "find F3D container-only dimension parameter",
        )?
        else {
            continue;
        };
        if ctx
            .position_by(
                &native.design_parameters[parameter_index + 1..],
                &mut matches_parameter,
                "find additional F3D container-only dimension parameter",
            )?
            .is_none()
        {
            let parameter = &native.design_parameters[parameter_index];
            let id = crate::ids::neutral_parameter_id_charged(ctx, parameter)?;
            ctx.insert_hash_set(
                &mut parameters_by_id,
                id,
                "collect F3D container-only dimension parameters",
            )
            .map(|_| ())?;
        }
    }
    Ok(parameters_by_id)
}

fn unresolved_dimension_companion_count(
    ctx: &DecodeContext<'_>,
    native: &F3dNative,
    ir: &CadIr,
) -> Result<usize, CodecError> {
    use std::collections::{HashMap, HashSet};

    let mut parameter_storage = ctx.reserve_scoped(0, "index F3D dimension parameters")?;
    let mut parameters = HashMap::new();
    for parameter in ctx.admit_iter(
        &native.design_parameters,
        "scan F3D native design parameters",
    )? {
        parameter_storage.with_storage(|| {
            ctx.insert_hash_map(
                &mut parameters,
                (
                    crate::ids::native_stream(&parameter.id).unwrap_or(crate::ids::DEFAULT_STREAM),
                    parameter.record_index,
                ),
                parameter.kind(),
                "index F3D dimension parameters",
            )
            .map(|_| ())
        })?;
    }
    let mut owner_storage = ctx.reserve_scoped(0, "index F3D dimension owners")?;
    let mut dimension_owners = HashSet::new();
    for owner in ctx.admit_iter(
        &native.design_parameter_owners,
        "scan F3D native design parameter owners",
    )? {
        let stream = crate::ids::native_stream(owner.id()).unwrap_or(crate::ids::DEFAULT_STREAM);
        if ctx.get_hash_map(
            &parameters,
            &(stream, owner.parameter_record_index()),
            "find F3D dimension parameter",
        )? == Some(&crate::records::parameters::DesignParameterKind::Dimension)
        {
            owner_storage.with_storage(|| {
                ctx.insert_hash_set(
                    &mut dimension_owners,
                    (stream, owner.record_index()),
                    "index F3D dimension owners",
                )
                .map(|_| ())
            })?;
        }
    }
    let mut typed_storage = ctx.reserve_scoped(0, "index F3D typed dimension companions")?;
    let mut typed = HashSet::new();
    let mut insert_typed = |key| -> Result<(), CodecError> {
        typed_storage.with_storage(|| {
            ctx.insert_hash_set(&mut typed, key, "index F3D typed dimension companions")
                .map(|_| ())
        })
    };
    for pair in ctx.admit_iter(
        &native.design_dimension_locus_pairs[..],
        "scan F3D native design dimension locus pairs",
    )? {
        insert_typed((
            crate::ids::native_stream(&pair.id).unwrap_or(crate::ids::DEFAULT_STREAM),
            pair.companion_record_index,
        ))?;
        insert_typed((
            crate::ids::native_stream(&pair.id).unwrap_or(crate::ids::DEFAULT_STREAM),
            pair.governing_companion_record_index,
        ))?;
    }
    for frame in ctx.admit_iter(
        &native.design_dimension_annotation_frames,
        "scan F3D native design dimension annotation frames",
    )? {
        insert_typed((
            crate::ids::native_stream(&frame.id).unwrap_or(crate::ids::DEFAULT_STREAM),
            frame.governing_companion_record_index,
        ))?;
    }
    for group in ctx.admit_iter(
        &native.design_dimension_locus_groups,
        "scan F3D native design dimension locus groups",
    )? {
        insert_typed((
            crate::ids::native_stream(&group.id).unwrap_or(crate::ids::DEFAULT_STREAM),
            group.companion_record_index,
        ))?;
    }
    for pair in ctx.admit_iter(
        &native.design_dimension_null_locus_pairs[..],
        "scan F3D native design dimension null locus pairs",
    )? {
        insert_typed((
            crate::ids::native_stream(&pair.id).unwrap_or(crate::ids::DEFAULT_STREAM),
            pair.companion_record_index,
        ))?;
        insert_typed((
            crate::ids::native_stream(&pair.id).unwrap_or(crate::ids::DEFAULT_STREAM),
            pair.governing_companion_record_index,
        ))?;
    }
    for record in ctx.admit_iter(
        &native.design_dimension_recipe_records,
        "scan F3D native design dimension recipe records",
    )? {
        insert_typed((
            crate::ids::native_stream(&record.id).unwrap_or(crate::ids::DEFAULT_STREAM),
            record.companion_record_index,
        ))?;
    }
    for constraint in ctx.admit_iter(
        &ir.model.sketch_constraints,
        "scan F3D ir model sketch constraints",
    )? {
        if !matches!(
            constraint.definition.kind(),
            cadmpeg_ir::sketches::SketchConstraintDefinitionInput::Native { .. }
        ) {
            if let Some(native_ref) = &constraint.native_ref {
                if let Some(companion) = ctx.find_by(
                    &native.design_parameter_companions,
                    |companion| {
                        ctx.equal(
                            companion.id(),
                            native_ref.as_str(),
                            "compare F3D dimension companion identity",
                        )
                    },
                    "find F3D dimension companion",
                )? {
                    insert_typed((
                        crate::ids::native_stream(native_ref).unwrap_or(crate::ids::DEFAULT_STREAM),
                        companion.record_index(),
                    ))?;
                }
            }
        }
    }
    let mut count = 0usize;
    for companion in ctx.admit_iter(
        &native.design_parameter_companions,
        "scan F3D dimension companions",
    )? {
        let stream =
            crate::ids::native_stream(companion.id()).unwrap_or(crate::ids::DEFAULT_STREAM);
        if companion
            .payload()
            .is_some_and(|payload| payload.byte_length() > 0)
            && ctx.contains_hash_set(
                &dimension_owners,
                &(stream, companion.owner_record_index()),
                "find F3D dimension companion owner",
            )?
            && !ctx.contains_hash_set(
                &typed,
                &(stream, companion.record_index()),
                "find F3D typed dimension companion",
            )?
        {
            count = count.checked_add(1).ok_or_else(|| {
                ctx.refuse_codec_limit("count F3D unresolved dimension companions", 0, u64::MAX)
            })?;
        }
    }
    Ok(count)
}

fn report_unresolved_dimension_companions(
    ctx: &DecodeContext<'_>,
    report: &mut DecodeBody,
    native: &F3dNative,
    ir: &CadIr,
) -> Result<(), CodecError> {
    let count = unresolved_dimension_companion_count(ctx, native, ir)?;
    if count != 0 {
        push_loss_vec(ctx, &mut report.losses, F3dLossCode::DimensionCompanionUntyped, format_args!(
            "{count} payload-bearing Design dimension companion(s) were retained without a typed locus frame."
        ), "report unresolved F3D dimensions", "retain F3D unresolved dimension loss")?;
    }
    Ok(())
}

fn report_unresolved_configuration_rules(
    ctx: &DecodeContext<'_>,
    report: &mut DecodeBody,
    native: &F3dNative,
    ir: &CadIr,
) -> Result<(), CodecError> {
    let count = crate::design::configurations::unresolved_configuration_member_count(
        ctx,
        &native.design_configurations,
    )?;
    if count != 0 {
        push_loss_vec(ctx, &mut report.losses, F3dLossCode::ConfigurationMemberUnassigned, format_args!(
            "{count} Design configuration JSON member(s) were retained without assigned neutral configuration semantics."
        ), "collect F3D decode losses", "retain F3D decode loss")?;
    }
    let count = crate::design::configurations::unresolved_configuration_rule_count(
        ctx,
        &native.design_configurations,
        &ir.model.configurations,
    )?;
    if count != 0 {
        push_loss_vec(ctx, &mut report.losses, F3dLossCode::ConfigurationRuleUnbound, format_args!(
            "{count} nonempty Design configuration rule(s) were retained without an unambiguous neutral activation target."
        ), "collect F3D decode losses", "retain F3D decode loss")?;
    }
    let count = crate::design::configurations::unresolved_configuration_parameter_override_count(
        ctx,
        &ir.model.configurations,
    )?;
    if count != 0 {
        push_loss_vec(ctx, &mut report.losses, F3dLossCode::ConfigurationParameterOverrideUnbound, format_args!(
            "{count} Design configuration parameter override(s) were retained without an unambiguous neutral parameter identity."
        ), "collect F3D decode losses", "retain F3D decode loss")?;
    }
    let count = crate::design::configurations::unresolved_configuration_suppressed_feature_count(
        ctx,
        &ir.model.configurations,
    )?;
    if count != 0 {
        push_loss_vec(ctx, &mut report.losses, F3dLossCode::ConfigurationFeatureSuppressionUnbound, format_args!(
            "{count} Design configuration feature suppression(s) were retained without an unambiguous neutral feature identity."
        ), "collect F3D decode losses", "retain F3D decode loss")?;
    }
    Ok(())
}

fn report_unretained_act_component_links(
    ctx: &DecodeContext<'_>,
    report: &mut DecodeBody,
    count: usize,
) -> Result<(), CodecError> {
    if count != 0 {
        push_loss_vec(ctx, &mut report.losses, F3dLossCode::ActComponentLinkUnresolved, format_args!(
            "{count} non-root ACT component link(s) remain source-only because their product-structure role is unresolved."
        ), "collect F3D decode losses", "retain F3D decode loss")?;
    }
    Ok(())
}

fn report_untyped_material_distances(
    ctx: &DecodeContext<'_>,
    report: &mut DecodeBody,
    count: usize,
) -> Result<(), CodecError> {
    if count != 0 {
        push_loss_vec(ctx, &mut report.losses, F3dLossCode::MaterialDistanceUnitUntyped, format_args!(
            "{count} Protein texture Distance property value(s) retain an untyped unit tag; their typed texture carriers were omitted."
        ), "collect F3D decode losses", "retain F3D decode loss")?;
    }
    Ok(())
}

#[derive(Debug, Default, PartialEq, Eq)]
struct DesignProjectionGaps {
    unresolved_body_bindings: usize,
    incomplete_features: usize,
    native_reference_images: usize,
    native_decals: usize,
    unprojected_feature_scopes: usize,
    unprojected_parameters: usize,
    unresolved_parameter_owners: usize,
    untyped_parameter_units: usize,
    unresolved_expression_dependencies: usize,
    unprojected_history_dependencies: usize,
    ambiguous_history_dependencies: usize,
    native_sketch_relations: usize,
    native_dimensions: usize,
    unprojected_sketch_placements: usize,
    unprojected_sketch_points: usize,
    unprojected_sketch_curves: usize,
    unprojected_sketch_surfaces: usize,
    unprojected_sketch_texts: usize,
    unprojected_sketch_relations: usize,
    unprojected_dimensions: usize,
    profile_selections: usize,
    path_selections: usize,
    face_selections: usize,
    active_face_substitutions: usize,
    body_selections: usize,
    partially_resolved_face_members: usize,
    native_edge_selections: usize,
    partially_resolved_edge_members: usize,
    unresolved_edge_selections: usize,
    unrepaired_lost_edge_references: usize,
}

/// Returns whether a face selection supplies the operation's neutral faces.
///
/// Native or absent selections leave the definition incomplete.
fn face_selection_is_resolved(selection: &cadmpeg_ir::features::FaceSelection) -> bool {
    use cadmpeg_ir::features::FaceSelection;

    match selection {
        FaceSelection::Faces(faces) | FaceSelection::Resolved { faces, .. } => !faces.is_empty(),
        FaceSelection::Historical { .. } => true,
        FaceSelection::Generated { .. } => true,
        FaceSelection::HistoricalPartial { .. } => false,
        FaceSelection::Unresolved | FaceSelection::Native(_) => false,
    }
}

fn draft_neutral_plane_is_resolved(
    ctx: &DecodeContext<'_>,
    selection: &cadmpeg_ir::features::FaceSelection,
    pull_plane: Option<&cadmpeg_ir::features::FeatureId>,
    pull_direction: Option<&cadmpeg_ir::math::Vector3>,
) -> Result<bool, CodecError> {
    Ok(face_selection_is_resolved(selection)
        || match selection {
            cadmpeg_ir::features::FaceSelection::Native(native) => {
                (match pull_plane {
                    Some(plane) => ctx.equal(
                        plane.as_str(),
                        native.as_str(),
                        "compare F3D Draft neutral plane",
                    )?,
                    None => false,
                }) && pull_direction.is_some_and(|direction| direction.unit().is_some())
            }
            _ => false,
        })
}

fn edge_selection_is_resolved(selection: &cadmpeg_ir::features::EdgeSelection) -> bool {
    use cadmpeg_ir::features::EdgeSelection;

    match selection {
        EdgeSelection::All => true,
        EdgeSelection::Edges(edges) | EdgeSelection::Resolved { edges, .. } => !edges.is_empty(),
        EdgeSelection::Historical { .. } => true,
        EdgeSelection::Generated { .. } => true,
        EdgeSelection::HistoricalPartial { .. } => false,
        EdgeSelection::Unresolved | EdgeSelection::Native(_) => false,
    }
}

fn datum_plane_reference_is_resolved(
    reference: &cadmpeg_ir::features::DatumPlaneReference,
) -> bool {
    match reference {
        cadmpeg_ir::features::DatumPlaneReference::Feature { .. } => true,
        cadmpeg_ir::features::DatumPlaneReference::Face { face } => {
            face_selection_is_resolved(face)
        }
        cadmpeg_ir::features::DatumPlaneReference::ResolvedPlane { .. } => true,
    }
}

fn datum_point_construction_is_resolved(
    ctx: &DecodeContext<'_>,
    construction: &cadmpeg_ir::features::DatumPointConstruction,
) -> Result<bool, CodecError> {
    use cadmpeg_ir::features::{DatumPointConstruction, SketchPointSelection, VertexSelection};

    Ok(match construction {
        DatumPointConstruction::CircleCenter { edge } => edge_selection_is_resolved(edge),
        DatumPointConstruction::TwoEdgeIntersection { edges } => {
            edges.iter().all(edge_selection_is_resolved)
        }
        DatumPointConstruction::ThreePlaneIntersection { planes } => ctx
            .admit_iter(planes.as_ref(), "scan F3D datum point construction planes")?
            .all(datum_plane_reference_is_resolved),
        DatumPointConstruction::Vertex { vertex } => matches!(
            vertex,
            VertexSelection::Generated { .. } | VertexSelection::Historical { .. }
        ),
        DatumPointConstruction::SketchPoint { point } => matches!(
            point,
            SketchPointSelection::Planar { .. } | SketchPointSelection::Spatial { .. }
        ),
        DatumPointConstruction::EdgePlaneIntersection { edge, plane } => {
            edge_selection_is_resolved(edge) && datum_plane_reference_is_resolved(plane)
        }
        DatumPointConstruction::DistanceOnEdge { edge, .. } => edge_selection_is_resolved(edge),
    })
}

fn body_selection_is_resolved(selection: &cadmpeg_ir::features::BodySelection) -> bool {
    use cadmpeg_ir::features::BodySelection;

    match selection {
        BodySelection::Bodies(bodies) | BodySelection::Resolved { bodies, .. } => {
            !bodies.is_empty()
        }
        BodySelection::Historical { bodies, .. } => !bodies.is_empty(),
        BodySelection::ResolvedSet { .. } | BodySelection::HistoricalSet { .. } => true,
        BodySelection::Generated { bodies, .. } => !bodies.is_empty(),
        BodySelection::Local { bodies, .. } => !bodies.is_empty(),
        BodySelection::Unresolved | BodySelection::Native(_) | BodySelection::NativeSet(_) => false,
    }
}

fn base_feature_body_selection_is_resolved(
    selection: &cadmpeg_ir::features::BodySelection,
) -> bool {
    body_selection_is_resolved(selection)
        || matches!(
            selection,
            cadmpeg_ir::features::BodySelection::Resolved { bodies, .. } if bodies.is_empty()
        )
}

fn datum_plane_frame_is_resolved(frame: cadmpeg_ir::features::FeatureDatumPlaneFrame) -> bool {
    const EPS_DATUM_PLANE_ORTHOGONAL: f64 = 1.0e-10;

    let (Some(normal), Some(u_axis)) = (
        UnitVector3::normalized(frame.normal().get()),
        UnitVector3::normalized(frame.u_axis().get()),
    ) else {
        return false;
    };
    normal.as_raw().dot(*u_axis.as_raw()).abs() <= EPS_DATUM_PLANE_ORTHOGONAL
}

fn axis_angle_is_resolved(axis_angle: &cadmpeg_ir::features::AxisAngle) -> bool {
    axis_angle.direction.unit().is_some()
}

fn face_motion_is_resolved(motion: &cadmpeg_ir::features::FaceMotion) -> bool {
    use cadmpeg_ir::features::FaceMotion;
    match motion {
        FaceMotion::Offset { .. } => true,
        FaceMotion::Translate { direction, .. } => direction.unit().is_some(),
        FaceMotion::Rotate { axis_dir, .. } => axis_dir.unit().is_some(),
    }
}

fn planar_profile_ref_is_resolved(profile: &PlanarProfileRef) -> bool {
    match profile {
        PlanarProfileRef::Unresolved(_)
        | PlanarProfileRef::Native(_)
        | PlanarProfileRef::SketchSelection { .. } => false,
        PlanarProfileRef::Sketch(_)
        | PlanarProfileRef::Feature(_)
        | PlanarProfileRef::SketchProfiles { .. }
        | PlanarProfileRef::SketchRegions { .. }
        | PlanarProfileRef::SketchEntities { .. }
        | PlanarProfileRef::HistoricalFaces { .. }
        | PlanarProfileRef::Generated { .. } => true,
        PlanarProfileRef::Faces(faces) => !faces.is_empty(),
    }
}

fn profile_ref_is_resolved(profile: &cadmpeg_ir::features::ProfileRef) -> bool {
    match profile {
        ProfileRef::SpatialSketchSelection { .. } => false,
        ProfileRef::SpatialSketchProfiles { .. } => true,
        ProfileRef::Planar(profile) => planar_profile_ref_is_resolved(profile),
    }
}

fn linear_termination_is_resolved(termination: &cadmpeg_ir::features::LinearTermination) -> bool {
    use cadmpeg_ir::features::{LinearTermination, VertexSelection};

    match termination {
        LinearTermination::Unresolved {} => false,
        LinearTermination::ToFace { face, .. }
        | LinearTermination::OffsetFromFace { face, .. }
        | LinearTermination::ToShape { target: face } => face_selection_is_resolved(face),
        LinearTermination::ToVertex { vertex } => matches!(
            vertex,
            VertexSelection::Generated { .. } | VertexSelection::Historical { .. }
        ),
        LinearTermination::Blind { .. }
        | LinearTermination::ThroughAll {}
        | LinearTermination::ThroughNext {}
        | LinearTermination::ToFirst {}
        | LinearTermination::ToLast {} => true,
    }
}

fn angular_termination_is_resolved(termination: &cadmpeg_ir::features::AngularTermination) -> bool {
    use cadmpeg_ir::features::{AngularTermination, VertexSelection};

    match termination {
        AngularTermination::Unresolved {} => false,
        AngularTermination::ToFace { face, .. }
        | AngularTermination::OffsetFromFace { face, .. }
        | AngularTermination::ToShape { target: face } => face_selection_is_resolved(face),
        AngularTermination::ToVertex { vertex } => matches!(
            vertex,
            VertexSelection::Generated { .. } | VertexSelection::Historical { .. }
        ),
        AngularTermination::Angle { .. }
        | AngularTermination::ThroughAll {}
        | AngularTermination::ThroughNext {}
        | AngularTermination::ToFirst {}
        | AngularTermination::ToLast {} => true,
    }
}

fn loft_path_is_resolved(path: &cadmpeg_ir::features::PathRef) -> bool {
    use cadmpeg_ir::features::PathRef;

    match path {
        PathRef::Unresolved(_) | PathRef::Native(_) | PathRef::SpatialSketchSelection { .. } => {
            false
        }
        PathRef::Sketch(_) => true,
        PathRef::SketchCurves { .. } => true,
        PathRef::SpatialSketchCurves { .. } => true,
        PathRef::Edges(edges) => !edges.is_empty(),
        PathRef::Curves(curves) => !curves.is_empty(),
        PathRef::HistoricalEdges { .. } => true,
    }
}

fn feature_definition_is_incomplete(
    ctx: &DecodeContext<'_>,
    definition: &cadmpeg_ir::features::FeatureDefinition,
) -> Result<bool, CodecError> {
    use cadmpeg_ir::features::{FeatureDefinition, FeatureOperation, NativeFeatureKind};

    Ok(match definition {
        FeatureDefinition::Operation(FeatureOperation::Native { kind, .. }) => {
            !matches!(kind, NativeFeatureKind::Canvas | NativeFeatureKind::Decal)
        }
        FeatureDefinition::Operation(
            FeatureOperation::ReferenceImage { .. } | FeatureOperation::DatumPrincipalPlane { .. },
        ) => false,
        FeatureDefinition::Operation(FeatureOperation::MeshImport { tessellations }) => {
            tessellations.is_empty()
        }
        FeatureDefinition::Operation(FeatureOperation::Decal { faces, .. }) => {
            !face_selection_is_resolved(faces)
        }
        FeatureDefinition::Operation(FeatureOperation::TrimSurface {
            faces, tool, keep, ..
        }) => {
            !face_selection_is_resolved(faces)
                || !loft_path_is_resolved(tool)
                || matches!(keep, cadmpeg_ir::features::TrimRegion::Unresolved)
        }
        FeatureDefinition::Operation(FeatureOperation::CosmeticThread {
            face,
            diameter,
            extent,
        }) => !face_selection_is_resolved(face) || diameter.is_none() || extent.is_none(),
        FeatureDefinition::Operation(FeatureOperation::Unresolved { .. }) => true,
        FeatureDefinition::Operation(FeatureOperation::DatumPlane { frame }) => {
            !datum_plane_frame_is_resolved(*frame)
        }
        FeatureDefinition::Operation(FeatureOperation::DatumAxis { direction, .. }) => {
            direction.unit().is_none()
        }
        FeatureDefinition::Operation(FeatureOperation::DatumCoordinateSystem { .. }) => false,
        FeatureDefinition::Operation(FeatureOperation::DatumThreePointPlane { frame, points }) => {
            !datum_plane_frame_is_resolved(*frame)
                || !ctx.admit_iter(&**points, "scan F3D three-point plane construction")?.all(|point| {
                    matches!(
                        point,
                        cadmpeg_ir::features::VertexSelection::Generated { .. }
                            | cadmpeg_ir::features::VertexSelection::Historical { .. }
                    )
                })
        }
        FeatureDefinition::Operation(FeatureOperation::DatumOffsetPlane { reference, .. }) => {
            reference
                .as_ref()
                .is_none_or(|reference| !datum_plane_reference_is_resolved(reference))
        }
        FeatureDefinition::Operation(
            FeatureOperation::Sphere { op, .. } | FeatureOperation::Torus { op, .. },
        ) => *op == cadmpeg_ir::features::BooleanOp::Unresolved,
        FeatureDefinition::Operation(FeatureOperation::Extrude {
            profile,
            start,
            extent,
            ..
        }) => {
            use cadmpeg_ir::features::{ExtrudeExtent, ExtrudeStart};

            let start_is_resolved = match start {
                ExtrudeStart::Unresolved {} => false,
                ExtrudeStart::FromFace { face, .. } => face_selection_is_resolved(face),
                ExtrudeStart::ProfilePlane {} | ExtrudeStart::OffsetProfilePlane { .. } => true,
            };
            let extent_is_resolved = match extent {
                ExtrudeExtent::OneSided { side } | ExtrudeExtent::Symmetric { side } => {
                    linear_termination_is_resolved(&side.termination)
                }
                ExtrudeExtent::TwoSided { first, second } => {
                    linear_termination_is_resolved(&first.termination)
                        && linear_termination_is_resolved(&second.termination)
                }
            };
            !profile_ref_is_resolved(profile) || !start_is_resolved || !extent_is_resolved
        }
        FeatureDefinition::Operation(FeatureOperation::Revolve { construction, op }) => {
            use cadmpeg_ir::features::{RevolveConstruction, RevolveExtent};

            match construction {
                RevolveConstruction::Unresolved(_) => true,
                RevolveConstruction::Resolved {
                    profile,
                    axis,
                    extent,
                    ..
                } => {
                    let extent_is_resolved = match extent {
                        RevolveExtent::OneSided { termination }
                        | RevolveExtent::Symmetric { termination } => {
                            angular_termination_is_resolved(termination)
                        }
                        RevolveExtent::TwoSided { first, second } => {
                            angular_termination_is_resolved(first)
                                && angular_termination_is_resolved(second)
                        }
                    };
                    !planar_profile_ref_is_resolved(profile)
                        || axis.direction.unit().is_none()
                        || !extent_is_resolved
                        || *op == cadmpeg_ir::features::BooleanOp::Unresolved
                }
            }
        }
        FeatureDefinition::Operation(FeatureOperation::Sweep {
            shape,

            path,

            orientation,
            guide_rail,
            ..
        }) => {
            use cadmpeg_ir::features::{SweepMode, SweepOrientation};

            let mode = shape.mode();

            let sections_are_resolved = match shape {
                cadmpeg_ir::features::SweepShape::Unresolved { section, sections }
                | cadmpeg_ir::features::SweepShape::Surface { section, sections } => !section.is_unresolved()
                    && section.referenced_profile().is_none_or(planar_profile_ref_is_resolved)
                    && ctx.admit_iter(sections.as_slice(), "scan F3D Sweep completeness sections")?.all(|section| !section.is_unresolved()
                        && section.referenced_profile().is_none_or(planar_profile_ref_is_resolved)),
                cadmpeg_ir::features::SweepShape::Solid { section, sections, .. } => !section.is_unresolved()
                    && section.referenced_profile().is_none_or(planar_profile_ref_is_resolved)
                    && ctx.admit_iter(sections.as_slice(), "scan F3D Sweep completeness sections")?.all(|section| !section.is_unresolved()
                        && section.referenced_profile().is_none_or(planar_profile_ref_is_resolved)),
            };
            let mode_is_resolved = match mode {
                SweepMode::Unresolved {} => false,
                SweepMode::Solid {
                    op: cadmpeg_ir::features::SolidSweepOperation::NewBody,
                }
                | SweepMode::Solid { .. }
                | SweepMode::Surface {} => true,
            };
            let orientation_is_resolved = match orientation {
                Some(SweepOrientation::Auxiliary { path, .. }) => loft_path_is_resolved(path),
                Some(SweepOrientation::GuideSurface { faces }) => face_selection_is_resolved(faces),
                None
                | Some(
                    SweepOrientation::Binormal { .. }
                    | SweepOrientation::CorrectedFrenet {}
                    | SweepOrientation::Fixed {}
                    | SweepOrientation::Frenet {},
                ) => true,
            };
            let guide_is_resolved = guide_rail
                .as_ref()
                .is_none_or(|guide| loft_path_is_resolved(&guide.path));

            !sections_are_resolved
                || !path.as_ref().is_some_and(loft_path_is_resolved)
                || !mode_is_resolved
                || !orientation_is_resolved
                || !guide_is_resolved
        }
        FeatureDefinition::Operation(FeatureOperation::Hole {
            profile,
            face,
            placements,
            shape,
            extent,
            ..
        }) => {
            use cadmpeg_ir::features::holes::HolePlacement;

            let cadmpeg_ir::features::holes::HoleConstruction::Form { kind, .. } = shape.construction()
            else {
                return Ok(true);
            };
            let diameter = shape.diameter();

            let support_is_resolved = profile.as_ref().is_some_and(planar_profile_ref_is_resolved)
                || face.as_ref().is_some_and(face_selection_is_resolved);
            let placements_are_resolved = placements.as_ref().map(|placements| Ok::<_, CodecError>(
                !placements.is_empty()
                    && ctx.admit_iter(placements.as_slice(), "scan F3D Hole completeness placements")?.all(|placement| match placement {
                        HolePlacement::Directed { direction, .. } => direction.unit().is_some(),
                        HolePlacement::Axis { axis, .. } => axis.unit().is_some(),
                    })
            )).transpose()?.unwrap_or(false);

            !support_is_resolved
                || !placements_are_resolved
                || kind.is_unresolved()
                || diameter.is_none()
                || extent
                    .as_ref()
                    .is_none_or(|extent| !linear_termination_is_resolved(extent))
        }
        FeatureDefinition::Operation(FeatureOperation::Coil {
            construction,
            result,
        }) => {
            use cadmpeg_ir::features::{CoilPlacement, CoilResult};

            matches!(construction.placement, CoilPlacement::Native { .. })
                || match result {
                    CoilResult::NewBody {} => false,
                    CoilResult::Boolean { targets, .. } => !body_selection_is_resolved(targets),
                }
        }
        // The draft angle remains available when face recipes fail, but replay
        // also requires resolved selections and the material-side convention.
        FeatureDefinition::Operation(FeatureOperation::Draft {
            faces,
            anchor,
            angle,
            outward,
            ..
        }) => {
            angle.is_none()
                || outward.is_none()
                || !face_selection_is_resolved(faces)
                || match anchor {
                    cadmpeg_ir::features::DraftAnchor::PartingLine { tool, pull } => {
                        !face_selection_is_resolved(tool)
                            || pull.direction.unit().is_none()
                            || pull.plane.is_none()
                    }
                    cadmpeg_ir::features::DraftAnchor::NeutralPlane { plane, pull } => {
                        !draft_neutral_plane_is_resolved(
                            ctx, plane,
                            pull.as_ref().and_then(|pull| pull.plane.as_ref()),
                            pull.as_ref().map(|pull| &*pull.direction),
                        )?
                    }
                }
        }
        FeatureDefinition::Operation(FeatureOperation::Sketch { sketch }) => sketch.id().is_none(),
        FeatureDefinition::Operation(FeatureOperation::DatumPoint { construction, .. }) => {
            construction
                .as_deref()
                .map(|construction| datum_point_construction_is_resolved(ctx, construction).map(|resolved| !resolved)).transpose()?.unwrap_or(true)
        }
        FeatureDefinition::Operation(FeatureOperation::SpatialSketch { sketch }) => {
            sketch.is_none()
        }
        FeatureDefinition::Operation(FeatureOperation::SketchBlockDefinition { sketch }) => {
            sketch.is_none()
        }
        FeatureDefinition::Operation(FeatureOperation::SketchBlockInstance { block, .. }) => {
            block.is_none()
        }
        FeatureDefinition::Operation(FeatureOperation::Form { cages }) => cages.is_empty(),
        FeatureDefinition::Operation(FeatureOperation::BaseFeature { bodies }) => {
            !base_feature_body_selection_is_resolved(bodies)
        }
        FeatureDefinition::Operation(FeatureOperation::InsertBodies { bodies }) => {
            !bodies.is_resolved()
        }
        FeatureDefinition::Operation(FeatureOperation::DeleteBody { bodies, mode }) => {
            !body_selection_is_resolved(bodies)
                || *mode == cadmpeg_ir::features::BodyRetentionMode::Unresolved
        }
        FeatureDefinition::Operation(FeatureOperation::InsertComponent { .. }) => false,
        FeatureDefinition::Operation(FeatureOperation::AssemblyJoint { .. }) => false,
        FeatureDefinition::Operation(FeatureOperation::Shell {
            bodies,
            removed_faces,
            thickness,
            outward,
            ..
        }) => {
            let bodies_are_resolved = bodies.as_ref().is_none_or(body_selection_is_resolved);
            let empty_removed_faces_are_resolved =
                matches!(
                    removed_faces,
                    cadmpeg_ir::features::FaceSelection::Faces(faces) if faces.is_empty()
                ) && bodies.as_ref().is_some_and(body_selection_is_resolved);
            !bodies_are_resolved
                || (!face_selection_is_resolved(removed_faces) && !empty_removed_faces_are_resolved)
                || thickness.is_none()
                || outward.is_none()
        }
        FeatureDefinition::Operation(FeatureOperation::Thicken {
            faces,
            thickness,
            side,
        }) => {
            !face_selection_is_resolved(faces)
                || thickness.is_none()
                || side.is_none()
        }
        FeatureDefinition::Operation(FeatureOperation::KnitSurface {
            faces,
            merge_entities,
            create_solid,
            gap_tolerance,
        }) => {
            !face_selection_is_resolved(faces)
                || merge_entities.is_none()
                || create_solid.is_none()
                || !gap_tolerance.is_some_and(|tolerance| tolerance.get() > 0.0)
        }
        FeatureDefinition::Operation(FeatureOperation::Block {
            dimensions,
            placement,
            op,
        }) => {
            dimensions.is_none()
                || placement.is_none()
                || *op == cadmpeg_ir::features::BooleanOp::Unresolved
        }
        FeatureDefinition::Operation(FeatureOperation::Primitive { op, .. }) => {
            *op == cadmpeg_ir::features::BooleanOp::Unresolved
        }
        FeatureDefinition::Operation(FeatureOperation::MoveFace { faces, motion }) => {
            !face_selection_is_resolved(faces) || !face_motion_is_resolved(motion)
        }
        FeatureDefinition::Operation(FeatureOperation::MoveBody {
            bodies, rotation, ..
        }) => {
            !body_selection_is_resolved(bodies)
                || rotation
                    .as_ref()
                    .is_some_and(|rotation| !axis_angle_is_resolved(rotation))
        }
        FeatureDefinition::Operation(FeatureOperation::Scale {
            bodies,
            center,
            factors,
        }) => {
            !body_selection_is_resolved(bodies)
                || center.is_none()
                || center.as_ref().is_some_and(|center| {
                    matches!(center, cadmpeg_ir::features::ScaleCenter::Native(_))
                })
                || factors.resolved().is_none()
        }
        FeatureDefinition::Operation(FeatureOperation::Pattern { seeds, pattern }) => {
            seeds.is_empty() || pattern.is_unresolved()
        }
        FeatureDefinition::Operation(FeatureOperation::Chamfer { groups, .. }) => {
            groups.is_empty()
                || ctx.admit_iter(groups.as_slice(), "scan F3D treatment completeness groups")?.any(|group| {
                    !edge_selection_is_resolved(&group.edges) || group.spec.is_unresolved()
                })
        }
        FeatureDefinition::Operation(FeatureOperation::Fillet { groups }) => {
            groups.is_empty()
                || ctx.admit_iter(groups.as_slice(), "scan F3D Fillet completeness groups")?
                    .any(|group| !edge_selection_is_resolved(&group.edges))
        }
        FeatureDefinition::Operation(FeatureOperation::DeleteFace { faces, .. }) => {
            !face_selection_is_resolved(faces)
        }
        FeatureDefinition::Operation(FeatureOperation::ReplaceFace { operands }) => {
            let targets = operands.targets();
            let replacements = operands.replacements();
            !face_selection_is_resolved(targets) || !face_selection_is_resolved(replacements)
        }
        FeatureDefinition::Operation(FeatureOperation::SplitBody { targets, tools }) => {
            !body_selection_is_resolved(targets) || !face_selection_is_resolved(tools)
        }
        FeatureDefinition::Operation(FeatureOperation::OffsetSurface {
            faces, distance, ..
        }) => !face_selection_is_resolved(faces) || distance.is_none(),
        FeatureDefinition::Operation(FeatureOperation::SheetMetalBaseFlange {
            profile, ..
        }) => !planar_profile_ref_is_resolved(profile),
        FeatureDefinition::Operation(FeatureOperation::SheetMetalEdgeFlange {
            edges,
            height,
            ..
        }) => {
            !edge_selection_is_resolved(edges)
                || matches!(
                    height,
                    cadmpeg_ir::features::SheetMetalFlangeHeight::ToObject {
                        target: cadmpeg_ir::features::SheetMetalFlangeHeightTarget::Native(_),
                        ..
                    }
                )
        }
        FeatureDefinition::Operation(FeatureOperation::SheetMetalHem {
            edges,
            form,
            direction,
            ..
        }) => {
            !edge_selection_is_resolved(edges)
                || matches!(
                    direction,
                    cadmpeg_ir::features::SheetMetalHemDirection::Unresolved
                )
                || matches!(
                    form,
                    cadmpeg_ir::features::SheetMetalHemForm::GapLength { .. }
                )
        }
        FeatureDefinition::Operation(FeatureOperation::SplitFace { targets, tool }) => {
            !face_selection_is_resolved(targets)
                || match tool {
                    cadmpeg_ir::features::SplitFaceTool::Plane { .. }
                    | cadmpeg_ir::features::SplitFaceTool::Planes { .. } => false,
                    cadmpeg_ir::features::SplitFaceTool::Path(path) => matches!(
                        path,
                        cadmpeg_ir::features::PathRef::Native(_)
                            | cadmpeg_ir::features::PathRef::Unresolved(_)
                    ),
                }
        }
        FeatureDefinition::Operation(FeatureOperation::Loft {
            sections, guidance, ..
        }) => {
            let guidance_incomplete = match guidance {
                cadmpeg_ir::features::LoftGuidance::Guides(paths) => {
                    ctx.admit_iter(paths.as_slice(), "scan F3D Loft completeness guides")?.any(|path| !loft_path_is_resolved(path))
                }
                cadmpeg_ir::features::LoftGuidance::Centerline(path) => {
                    !loft_path_is_resolved(path)
                }
            };
            sections.len() < 2
                || ctx.admit_iter(sections.as_slice(), "scan F3D Loft completeness sections")?.any(|section| match section {
                    cadmpeg_ir::features::LoftSection::Profile(profile) => {
                        !profile_ref_is_resolved(profile)
                    }
                    cadmpeg_ir::features::LoftSection::Point(
                        cadmpeg_ir::features::LoftPointSection::Native(_),
                    ) => true,
                    cadmpeg_ir::features::LoftSection::Point(
                        cadmpeg_ir::features::LoftPointSection::Point(_)
                        | cadmpeg_ir::features::LoftPointSection::Vertex(_),
                    ) => false,
                })
                || guidance_incomplete
        }
        FeatureDefinition::Operation(FeatureOperation::FilledSurface {
            boundary,
            support_faces,
            continuity,
            merge_result,
        }) => {
            use cadmpeg_ir::features::SurfaceBoundary;

            let boundary_is_resolved = match boundary {
                SurfaceBoundary::Edges(edges) => edge_selection_is_resolved(edges),
                SurfaceBoundary::Path(path) => loft_path_is_resolved(path),
            };
            let continuity_is_resolved = !continuity.is_unresolved();
            let needs_support = |continuity| {
                matches!(
                    continuity,
                    cadmpeg_ir::features::SurfaceContinuity::Tangent
                        | cadmpeg_ir::features::SurfaceContinuity::Curvature
                )
            };
            let support_is_required = continuity
                .resolved()
                .map(|continuity| Ok::<_, CodecError>(ctx.admit_iter(continuity.conditions.as_slice(), "scan F3D surface continuity conditions")?.copied().any(needs_support))).transpose()?.unwrap_or(false);

            !boundary_is_resolved
                || !continuity_is_resolved
                || (support_is_required && !face_selection_is_resolved(support_faces))
                || merge_result.is_none()
        }
        FeatureDefinition::Operation(FeatureOperation::FullRoundFillet { groups }) => {
            groups.is_empty()
                || ctx.admit_iter(groups.as_slice(), "scan F3D treatment completeness groups")?.any(|group| {
                    !face_selection_is_resolved(group.center_faces())
                        || matches!(
                            group.side_one_faces(),
                            cadmpeg_ir::features::edge_treatments::FullRoundSideSelection::Unresolved
                        )
                        || matches!(
                            group.side_two_faces(),
                            cadmpeg_ir::features::edge_treatments::FullRoundSideSelection::Unresolved
                        )
                        || matches!(
                            group.side_one_faces(),
                            cadmpeg_ir::features::edge_treatments::FullRoundSideSelection::Explicit(ref selection)
                                if !face_selection_is_resolved(selection)
                        )
                        || matches!(
                            group.side_two_faces(),
                            cadmpeg_ir::features::edge_treatments::FullRoundSideSelection::Explicit(ref selection)
                                if !face_selection_is_resolved(selection)
                        )
                })
        }
        FeatureDefinition::Operation(FeatureOperation::Combine { operands, .. }) => {
            let target = operands.target();
            let tools = operands.tools();
            !body_selection_is_resolved(target) || !body_selection_is_resolved(tools)
        }
        // A typed family is not replayable until this match states and checks
        // its complete construction invariants.
        _ => true,
    })
}

fn incomplete_feature_families<'a, 'ctx>(
    ctx: &'ctx DecodeContext<'_>,
    ir: &'a CadIr,
) -> Result<
    (
        cadmpeg_core::decode::ScopedReservation<'ctx>,
        std::collections::BTreeMap<&'a str, usize>,
    ),
    CodecError,
> {
    let operation = "index incomplete F3D feature families";
    let mut storage = ctx.reserve_scoped(0, operation)?;
    let mut families = std::collections::BTreeMap::<&str, usize>::new();
    for feature in ctx.admit_iter(&ir.model.features, "scan F3D ir model features")? {
        if !feature_definition_is_incomplete(ctx, feature.evaluation.definition())? {
            continue;
        }
        let family = feature.source_tag.as_deref().unwrap_or_else(|| {
            if let cadmpeg_ir::features::FeatureDefinition::Operation(
                cadmpeg_ir::features::FeatureOperation::Native { kind, .. },
            ) = feature.evaluation.definition()
            {
                kind.as_str()
            } else {
                "<missing source tag>"
            }
        });
        storage.with_storage(|| {
            if let Some(count) = ctx.get_mut_btree_map(&mut families, family, operation)? {
                *count = count
                    .checked_add(1)
                    .ok_or_else(|| ctx.refuse_codec_limit(operation, 0, u64::MAX))?;
            } else {
                ctx.insert_btree_map(&mut families, family, 1, operation)?;
            }
            Ok::<(), CodecError>(())
        })?;
    }
    Ok((storage, families))
}

fn design_projection_gaps(
    ctx: &DecodeContext<'_>,
    ir: &CadIr,
    native: &F3dNative,
) -> Result<DesignProjectionGaps, CodecError> {
    use cadmpeg_ir::features::{
        BodySelection, EdgeSelection, ExtrudeExtent, ExtrudeStart, FaceSelection, LinearTermination,
    };
    use cadmpeg_ir::features::{
        FeatureDefinition, FeatureOperation, NativeFeatureKind, PathRef, PlanarProfileRef,
        ProfileRef,
    };
    use cadmpeg_ir::sketches::SketchConstraintDefinitionInput;
    use std::collections::HashSet;

    let source_lost_edge_reference_ids = ctx.collect_hash_set(
        native
            .lost_edge_references
            .iter()
            .map(|reference| reference.id.as_str()),
        "index F3D lost edge references",
    )?;
    let mut complete_edge_selection_native_ids = HashSet::<String>::new();
    let projected_constraint_refs = ctx.collect_hash_set(
        ctx.admit_iter(
            &ir.model.sketch_constraints,
            "scan F3D projection sketch_constraints",
        )?
        .filter_map(|constraint| constraint.native_ref.as_deref())
        .chain(
            ctx.admit_iter(
                &ir.model.spatial_sketch_constraints,
                "scan F3D projection spatial_sketch_constraints",
            )?
            .filter_map(|constraint| constraint.native_ref.as_deref()),
        ),
        "index projected F3D constraints",
    )?;
    let projected_sketch_refs = ctx.collect_hash_set(
        ctx.admit_iter(&ir.model.sketches, "scan F3D projection sketches")?
            .filter_map(|sketch| sketch.native_ref.as_deref())
            .chain(
                ctx.admit_iter(
                    &ir.model.spatial_sketches,
                    "scan F3D projection spatial_sketches",
                )?
                .filter_map(|sketch| sketch.native_ref.as_deref()),
            ),
        "index projected F3D sketches",
    )?;
    let projected_sketch_entity_refs = ctx.collect_hash_set(
        ctx.admit_iter(
            &ir.model.sketch_entities,
            "scan F3D projection sketch_entities",
        )?
        .filter_map(|entity| entity.native_ref.as_deref())
        .chain(
            ctx.admit_iter(
                &ir.model.spatial_sketch_entities,
                "scan F3D projection spatial_sketch_entities",
            )?
            .filter_map(|entity| entity.native_ref.as_deref()),
        ),
        "index projected F3D sketch entities",
    )?;
    let projected_feature_refs = ctx.collect_hash_set(
        ctx.admit_iter(&ir.model.features, "scan F3D projection features")?
            .filter_map(|feature| feature.native_ref.as_deref()),
        "index projected F3D features",
    )?;
    let projected_parameter_refs = ctx.collect_hash_set(
        ctx.admit_iter(&ir.model.parameters, "scan F3D projection parameters")?
            .filter_map(|parameter| parameter.native_ref.as_deref()),
        "index projected F3D parameters",
    )?;
    let projected_features = ctx.collect_hash_map(
        ctx.admit_iter(&ir.model.features, "scan F3D projection features")?
            .filter_map(|feature| Some((feature.native_ref.as_deref()?, feature))),
        "index projected F3D feature records",
    )?;
    let mut unprojected_history_dependencies = 0;
    let mut ambiguous_history_dependencies = 0;
    let scope_history = crate::design::feature_project::ScopeHistoryGraph::new(
        ctx,
        &native.design_parameter_scopes,
        &native.design_body_bindings,
        &native.design_body_recipe_operands,
        &native.design_component_naming_spaces,
        &native.asm_histories,
    )?;
    for scope in ctx.admit_iter(
        &native.design_parameter_scopes,
        "scan F3D native design parameter scopes",
    )? {
        let Some(feature) = ctx.get_hash_map(
            &projected_features,
            scope.id.as_str(),
            "find F3D projected feature",
        )?
        else {
            continue;
        };
        let predecessor_scope = match scope_history.predecessor(ctx, scope, |candidate| {
            ctx.contains_key_hash_map(
                &projected_features,
                candidate.id.as_str(),
                "find F3D projected predecessor feature",
            )
        }) {
            Ok(crate::design::feature_project::ScopeHistoryPredecessor::Scope(predecessor)) => {
                predecessor
            }
            Err(error @ CodecError::ResourceLimit(_)) => return Err(error),
            Ok(crate::design::feature_project::ScopeHistoryPredecessor::Ambiguous) | Err(_) => {
                ambiguous_history_dependencies += 1;
                continue;
            }
            Ok(crate::design::feature_project::ScopeHistoryPredecessor::None) => {
                continue;
            }
        };
        let Some(predecessor) = ctx.get_hash_map(
            &projected_features,
            predecessor_scope.id.as_str(),
            "find F3D projected predecessor",
        )?
        else {
            unprojected_history_dependencies += 1;
            continue;
        };
        if !ctx.equal(
            &predecessor.id,
            &feature.id,
            "compare F3D projected dependency IDs",
        )? && !ctx.contains(
            feature.dependencies.as_slice(),
            &predecessor.id,
            "find F3D projected dependency",
        )? {
            unprojected_history_dependencies += 1;
        }
    }
    let projected_dimension_parameters = ctx.collect_hash_set(ctx.admit_iter(&ir.model.sketch_constraints, "scan F3D projection sketch_constraints")?
            .flat_map(|constraint| {
                crate::design::dimensions::constraint_parameters(constraint.definition.kind())
            })
            .chain(
                ctx.admit_iter(&ir.model.spatial_sketch_constraints, "scan F3D projection spatial_sketch_constraints")?.filter_map(
                    |constraint| match constraint.definition.kind() {
                        cadmpeg_ir::sketches::SpatialSketchConstraintDefinitionInput::Native {
                            parameter,
                            ..
                        } => parameter.as_ref(),
                        cadmpeg_ir::sketches::SpatialSketchConstraintDefinitionInput::PointDistance {
                            parameter,
                            ..
                        }
                        | cadmpeg_ir::sketches::SpatialSketchConstraintDefinitionInput::PointLineDistance {
                            parameter,
                            ..
                        }
                        | cadmpeg_ir::sketches::SpatialSketchConstraintDefinitionInput::ParallelLineDistance {
                            parameter,
                            ..
                        }
                        | cadmpeg_ir::sketches::SpatialSketchConstraintDefinitionInput::RepeatedParallelLineDistance {
                            parameter,
                            ..
                        }
                        | cadmpeg_ir::sketches::SpatialSketchConstraintDefinitionInput::LineLength {
                            parameter,
                            ..
                        }
                        | cadmpeg_ir::sketches::SpatialSketchConstraintDefinitionInput::RepeatedLineLength {
                            parameter,
                            ..
                        }
                        | cadmpeg_ir::sketches::SpatialSketchConstraintDefinitionInput::ParallelLineSetDistance {
                            parameter,
                            ..
                        } => Some(parameter),
                        cadmpeg_ir::sketches::SpatialSketchConstraintDefinitionInput::Offset {
                            parameter,
                            ..
                        } => parameter.as_ref().map(|parameter| &parameter.id),
                        _ => None,
                    },
                ),
            ), "index projected F3D dimension parameters")?;

    let native_sketch_relation_ids = ctx.collect_hash_set(
        native
            .sketch_relations
            .iter()
            .map(|relation| relation.id.as_str()),
        "index native F3D sketch relations",
    )?;
    let mut native_sketch_relations = 0;
    let mut native_dimensions = 0;
    for constraint in ctx.admit_iter(
        &ir.model.sketch_constraints,
        "scan F3D ir model sketch constraints",
    )? {
        if !matches!(
            constraint.definition.kind(),
            SketchConstraintDefinitionInput::Native { .. }
        ) {
            continue;
        }
        if constraint
            .native_ref
            .as_deref()
            .map(|native_ref| {
                ctx.contains_hash_set(
                    &native_sketch_relation_ids,
                    native_ref,
                    "find F3D native sketch relation",
                )
            })
            .transpose()?
            .unwrap_or(false)
        {
            native_sketch_relations += 1;
        } else {
            native_dimensions += 1;
        }
    }
    for constraint in ctx.admit_iter(
        &ir.model.spatial_sketch_constraints,
        "scan F3D ir model spatial sketch constraints",
    )? {
        if !matches!(
            constraint.definition.kind(),
            cadmpeg_ir::sketches::SpatialSketchConstraintDefinitionInput::Native { .. }
        ) {
            continue;
        }
        if constraint
            .native_ref
            .as_deref()
            .map(|native_ref| {
                ctx.contains_hash_set(
                    &native_sketch_relation_ids,
                    native_ref,
                    "find F3D native sketch relation",
                )
            })
            .transpose()?
            .unwrap_or(false)
        {
            native_sketch_relations += 1;
        } else {
            native_dimensions += 1;
        }
    }

    let authored_scopes = match crate::design::feature_project::authored_scope_ordinals_per_stream(
        ctx,
        &native.design_parameter_scopes,
        &native.design_feature_timelines,
    ) {
        Ok(scopes) => Some(scopes),
        Err(error @ CodecError::ResourceLimit(_)) => return Err(error),
        Err(_) => None,
    };
    let mut gaps = DesignProjectionGaps {
        unresolved_body_bindings: ctx.admit_iter(&native.design_body_bindings, "scan F3D projection design_body_bindings")?
            .try_fold(0usize, |count, binding| { let selected = binding.body.is_none(); count.checked_add(usize::from(selected)).ok_or_else(|| ctx.refuse_codec_limit("count F3D projection gaps", u64::MAX - 1, u64::MAX)) })?,
        unprojected_history_dependencies,
        ambiguous_history_dependencies,
        unprojected_feature_scopes: ctx.admit_iter(&native.design_parameter_scopes, "scan F3D projection design_parameter_scopes")?
            .try_fold(0usize, |count, scope| { let selected = {
                let authored = match authored_scopes.as_ref() {
                    None => {
                        scope
                            .assembly_alignment()
                            .and_then(super::records::feature::assembly::DesignAssemblyAlignment::joint_origin_scope_record_index)
                            .is_none()
                    },
                    Some(ordinals) => {
                        let stream = crate::ids::native_stream(&scope.id)
                            .unwrap_or(crate::ids::DEFAULT_STREAM);
                        ctx.contains_key_hash_map(ordinals, &(stream, scope.record_index), "find F3D authored scope ordinal")?
                    },
                };
                authored && !ctx.contains_hash_set(&projected_feature_refs, scope.id.as_str(), "find F3D projection projected_feature_refs")?
            }; count.checked_add(usize::from(selected)).ok_or_else(|| ctx.refuse_codec_limit("count F3D projection gaps", u64::MAX - 1, u64::MAX)) })?,
        unprojected_parameters: ctx.admit_iter(&native.design_parameters, "scan F3D projection design_parameters")?
            .try_fold(0usize, |count, parameter| { let selected = !ctx.contains_hash_set(&projected_parameter_refs, parameter.id.as_str(), "find F3D projection projected_parameter_refs")?; count.checked_add(usize::from(selected)).ok_or_else(|| ctx.refuse_codec_limit("count F3D projection gaps", u64::MAX - 1, u64::MAX)) })?,
        unresolved_parameter_owners: ctx.admit_iter(&native.design_parameters, "scan F3D projection design_parameters")?
            .try_fold(0usize, |count, parameter| { let selected = {
                let Some(owner_record_index) = parameter.owner_record_index() else {
                    return Ok(count);
                };
                let Some(stream) = crate::ids::native_stream(&parameter.id) else {
                    return count.checked_add(1).ok_or_else(|| ctx.refuse_codec_limit("count F3D projection gaps", u64::MAX - 1, u64::MAX));
                };
                !ctx.any_by(native.design_parameter_owners.as_slice(), |owner| { Ok(
                    ctx.equal(&crate::ids::native_stream(owner.id()), &Some(stream), "compare F3D parameter owner streams")?
                        && owner.record_index() == owner_record_index
                        && ctx.any_by(native.design_parameter_scopes.as_slice(), |scope| { Ok(
                            ctx.equal(&crate::ids::native_stream(&scope.id), &Some(stream), "compare F3D parameter scope streams")?
                                && scope.record_index == owner.scope_record_index())
                        }, "find F3D parameter owner scope")?)
                }, "find F3D parameter owner")?
            }; count.checked_add(usize::from(selected)).ok_or_else(|| ctx.refuse_codec_limit("count F3D projection gaps", u64::MAX - 1, u64::MAX)) })?,
        untyped_parameter_units: crate::design::feature_project::untyped_parameter_unit_count(
            &native.design_parameters,
        ),
        unresolved_expression_dependencies:
            crate::design::dimensions::unresolved_parameter_expression_dependency_count(ctx, &native.design_parameters, &ir.model.parameters)?,
        native_sketch_relations,
        native_dimensions,
        unprojected_sketch_placements: ctx.admit_iter(&native.design_sketch_placements, "scan F3D projection design_sketch_placements")?
            .try_fold(0usize, |count, placement| { let selected = !ctx.contains_hash_set(&projected_sketch_refs, placement.id.as_str(), "find F3D projection projected_sketch_refs")?; count.checked_add(usize::from(selected)).ok_or_else(|| ctx.refuse_codec_limit("count F3D projection gaps", u64::MAX - 1, u64::MAX)) })?,
        unprojected_sketch_points: ctx.admit_iter(&native.sketch_points, "scan F3D projection sketch_points")?
            .try_fold(0usize, |count, point| { let selected = {
                point.owner_reference.is_some()
                    && !ctx.contains_hash_set(&projected_sketch_entity_refs, point.id.as_str(), "find F3D projection projected_sketch_entity_refs")?
            }; count.checked_add(usize::from(selected)).ok_or_else(|| ctx.refuse_codec_limit("count F3D projection gaps", u64::MAX - 1, u64::MAX)) })?,
        unprojected_sketch_curves: ctx.admit_iter(&native.sketch_curve_identities, "scan F3D projection sketch_curve_identities")?
            .try_fold(0usize, |count, curve| { let selected = {
                curve.owner_reference.is_some()
                    && !ctx.contains_hash_set(&projected_sketch_entity_refs, curve.id.as_str(), "find F3D projection projected_sketch_entity_refs")?
            }; count.checked_add(usize::from(selected)).ok_or_else(|| ctx.refuse_codec_limit("count F3D projection gaps", u64::MAX - 1, u64::MAX)) })?,
        unprojected_sketch_surfaces: ctx.admit_iter(&native.sketch_surfaces, "scan F3D projection sketch_surfaces")?
            .try_fold(0usize, |count, surface| { let selected = {
                surface.owner_reference.is_some()
                    && !ctx.contains_hash_set(&projected_sketch_entity_refs, surface.id.as_str(), "find F3D projection projected_sketch_entity_refs")?
            }; count.checked_add(usize::from(selected)).ok_or_else(|| ctx.refuse_codec_limit("count F3D projection gaps", u64::MAX - 1, u64::MAX)) })?,
        unprojected_sketch_texts: ctx.admit_iter(&native.sketch_texts, "scan F3D projection sketch_texts")?
            .try_fold(0usize, |count, text| { let selected = !ctx.contains_hash_set(&projected_sketch_entity_refs, text.id.as_str(), "find F3D projection projected_sketch_entity_refs")?; count.checked_add(usize::from(selected)).ok_or_else(|| ctx.refuse_codec_limit("count F3D projection gaps", u64::MAX - 1, u64::MAX)) })?,
        unprojected_sketch_relations: ctx.admit_iter(&native.sketch_relations, "scan F3D projection sketch_relations")?
            .try_fold(0usize, |count, relation| { let selected = !ctx.contains_hash_set(&projected_constraint_refs, relation.id.as_str(), "find F3D projection projected_constraint_refs")?; count.checked_add(usize::from(selected)).ok_or_else(|| ctx.refuse_codec_limit("count F3D projection gaps", u64::MAX - 1, u64::MAX)) })?,
        unprojected_dimensions: {
            let container_only = container_only_dimension_parameters(ctx, native)?;
            let relation_bearing_companions = ctx.collect_hash_set(ctx.admit_iter(&native.design_parameter_companions, "scan F3D projection design_parameter_companions")?
                .filter(|companion| {
                    companion
                        .payload()
                        .is_some_and(|payload| payload.byte_length() > 0)
                })
                .filter_map(|companion| {
                    Some((
                        crate::ids::native_stream(companion.id())?,
                        companion.record_index(),
                    ))
                })
                .chain(
                    ctx.admit_iter(&*native.design_dimension_locus_pairs, "scan F3D projection design_dimension_locus_pairs")?
                        .filter_map(|pair| {
                            Some((
                                crate::ids::native_stream(&pair.id)?,
                                pair.governing_companion_record_index,
                            ))
                        }),
                )
                .chain(
                    ctx.admit_iter(&*native.design_dimension_null_locus_pairs, "scan F3D projection design_dimension_null_locus_pairs")?
                        .filter_map(|pair| {
                            Some((
                                crate::ids::native_stream(&pair.id)?,
                                pair.governing_companion_record_index,
                            ))
                        }),
                )
                .chain(
                    ctx.admit_iter(&native.design_dimension_annotation_frames, "scan F3D projection design_dimension_annotation_frames")?
                        .filter_map(|frame| {
                            Some((
                                crate::ids::native_stream(&frame.id)?,
                                frame.governing_companion_record_index,
                            ))
                        }),
                )
                .chain(
                    ctx.admit_iter(&native.design_dimension_locus_groups, "scan F3D projection design_dimension_locus_groups")?
                        .filter_map(|group| {
                            Some((
                                crate::ids::native_stream(&group.id)?,
                                group.companion_record_index,
                            ))
                        }),
                )
                .chain(
                    ctx.admit_iter(&native.design_dimension_recipe_records, "scan F3D projection design_dimension_recipe_records")?
                        .filter_map(|record| {
                            Some((
                                crate::ids::native_stream(&record.id)?,
                                record.companion_record_index,
                            ))
                        }),
                ), "index F3D relation-bearing companions")?;
            let mut relation_storage = ctx.reserve_scoped(0, "hold F3D relation-bearing parameters")?;
            let mut relation_bearing_parameters = HashSet::new();
            for owner in ctx.admit_iter(&native.design_parameter_owners, "scan F3D relation-bearing parameter owners")? {
                let Some(stream) = crate::ids::native_stream(owner.id()) else { continue; };
                if ctx.contains_hash_set(&relation_bearing_companions, &(stream, owner.companion_record_index()), "find F3D relation-bearing companion")? {
                    relation_storage.with_storage(|| ctx.insert_hash_set(&mut relation_bearing_parameters, (stream, owner.parameter_record_index()), "index F3D relation-bearing parameters"))?;
                }
            }
            ctx.admit_iter(&native.design_parameters, "scan F3D projection design_parameters")?
                .try_fold(0usize, |count, parameter| {
                    let stream = crate::ids::native_stream(&parameter.id)
                        .unwrap_or(crate::ids::DEFAULT_STREAM);
                    if parameter.kind() != crate::records::parameters::DesignParameterKind::Dimension
                        || !ctx.contains_hash_set(&relation_bearing_parameters, &(stream, parameter.record_index), "find F3D projection relation_bearing_parameters")?
                    {
                        return Ok(count);
                    }
                    let id = crate::ids::neutral_parameter_id_charged(ctx, parameter)?;
                    count.checked_add(usize::from(
                        !ctx.contains_hash_set(&projected_dimension_parameters, &id, "find F3D projection projected_dimension_parameters")?
                            && !ctx.contains_hash_set(&container_only, &id, "find F3D projection container_only")?,
                    )).ok_or_else(|| ctx.refuse_codec_limit("count F3D unprojected dimensions", u64::MAX - 1, u64::MAX))
                })
                ?
        },
        active_face_substitutions: ctx.admit_iter(&native.design_face_operands, "scan F3D projection design_face_operands")?
            .try_fold(0usize, |count, operand| { let selected = operand.resolved_active_face.is_some(); count.checked_add(usize::from(selected)).ok_or_else(|| ctx.refuse_codec_limit("count F3D projection gaps", u64::MAX - 1, u64::MAX)) })?,
        ..DesignProjectionGaps::default()
    };
    let mut edge_selection = |selection: &EdgeSelection| -> Result<(), CodecError> {
        match selection {
            EdgeSelection::Native(_) => gaps.native_edge_selections += 1,
            EdgeSelection::Unresolved => gaps.unresolved_edge_selections += 1,
            EdgeSelection::HistoricalPartial { unresolved, .. } => {
                let unresolved_count = ctx
                    .admit_iter(
                        unresolved.as_slice(),
                        "scan F3D unresolved historical edges",
                    )?
                    .try_fold(0usize, |count, id| {
                        let selected = !ctx.contains_hash_set(
                            &source_lost_edge_reference_ids,
                            id.as_str(),
                            "find F3D projection source_lost_edge_reference_ids",
                        )?;
                        count.checked_add(usize::from(selected)).ok_or_else(|| {
                            ctx.refuse_codec_limit(
                                "count F3D projection gaps",
                                u64::MAX - 1,
                                u64::MAX,
                            )
                        })
                    })?;
                gaps.partially_resolved_edge_members = gaps
                    .partially_resolved_edge_members
                    .checked_add(unresolved_count)
                    .ok_or_else(|| {
                        ctx.refuse_codec_limit(
                            "count F3D unresolved historical edges",
                            u64::MAX - 1,
                            u64::MAX,
                        )
                    })?;
            }
            EdgeSelection::Resolved { native, .. } => ctx
                .insert_string_set(
                    &mut complete_edge_selection_native_ids,
                    native.as_str(),
                    "index complete F3D edge selections",
                )
                .map(|_| ())?,
            EdgeSelection::Generated { native, .. } => ctx
                .insert_string_set(
                    &mut complete_edge_selection_native_ids,
                    native.as_str(),
                    "index complete F3D edge selections",
                )
                .map(|_| ())?,
            EdgeSelection::Historical { native, .. } => ctx
                .insert_string_set(
                    &mut complete_edge_selection_native_ids,
                    native.as_str(),
                    "index complete F3D edge selections",
                )
                .map(|_| ())?,
            EdgeSelection::All | EdgeSelection::Edges(_) => {}
        }
        Ok(())
    };
    let mut face_selection = |selection: &FaceSelection| match selection {
        FaceSelection::Native(_) | FaceSelection::Unresolved => gaps.face_selections += 1,
        FaceSelection::HistoricalPartial { unresolved, .. } => {
            gaps.partially_resolved_face_members += unresolved.len();
        }
        FaceSelection::Faces(_)
        | FaceSelection::Resolved { .. }
        | FaceSelection::Generated { .. }
        | FaceSelection::Historical { .. } => {}
    };
    let native_body_selection_count = |selection: &BodySelection| match selection {
        BodySelection::Native(_) | BodySelection::Unresolved => 1,
        BodySelection::NativeSet(members) => members.len(),
        BodySelection::Bodies(_)
        | BodySelection::Resolved { .. }
        | BodySelection::ResolvedSet { .. }
        | BodySelection::Historical { .. }
        | BodySelection::HistoricalSet { .. }
        | BodySelection::Generated { .. }
        | BodySelection::Local { .. } => 0,
    };
    for feature in ctx.admit_iter(&ir.model.features, "scan F3D ir model features")? {
        gaps.incomplete_features += usize::from(feature_definition_is_incomplete(
            ctx,
            feature.evaluation.definition(),
        )?);
        gaps.native_reference_images += usize::from(matches!(
            feature.evaluation.definition(),
            FeatureDefinition::Operation(FeatureOperation::Native {
                kind: NativeFeatureKind::Canvas,
                ..
            })
        ));
        gaps.native_decals += usize::from(matches!(
            feature.evaluation.definition(),
            FeatureDefinition::Operation(FeatureOperation::Native {
                kind: NativeFeatureKind::Decal,
                ..
            })
        ));
        match feature.evaluation.definition() {
            FeatureDefinition::Operation(FeatureOperation::BaseFeature { bodies }) => {
                gaps.body_selections += native_body_selection_count(bodies);
            }
            FeatureDefinition::Operation(FeatureOperation::InsertBodies { bodies }) => {
                gaps.body_selections += usize::from(!bodies.is_resolved());
            }
            FeatureDefinition::Operation(FeatureOperation::Combine { operands, .. }) => {
                let target = operands.target();
                let tools = operands.tools();
                gaps.body_selections +=
                    native_body_selection_count(target) + native_body_selection_count(tools);
            }
            FeatureDefinition::Operation(FeatureOperation::Coil {
                result: cadmpeg_ir::features::CoilResult::Boolean { targets, .. },
                ..
            }) => gaps.body_selections += native_body_selection_count(targets),
            FeatureDefinition::Operation(FeatureOperation::Extrude {
                profile,
                start,
                extent,
                ..
            }) => {
                if matches!(
                    profile,
                    ProfileRef::Planar(
                        PlanarProfileRef::Native(_) | PlanarProfileRef::SketchSelection { .. }
                    )
                ) {
                    gaps.profile_selections += 1;
                }
                if let ExtrudeStart::FromFace { face, .. } = start {
                    face_selection(face);
                }
                let (first, second) = match extent {
                    ExtrudeExtent::OneSided { side } | ExtrudeExtent::Symmetric { side } => {
                        (side, None)
                    }
                    ExtrudeExtent::TwoSided { first, second } => (first, Some(second)),
                };
                for side in [Some(first), second].into_iter().flatten() {
                    if let LinearTermination::ToFace { face, .. } = &side.termination {
                        face_selection(face);
                    }
                }
            }
            FeatureDefinition::Operation(FeatureOperation::Fillet { groups }) => {
                for group in ctx.admit_iter(&groups[..], "scan F3D fillet groups")? {
                    edge_selection(&group.edges)?;
                }
            }
            FeatureDefinition::Operation(FeatureOperation::FullRoundFillet { groups }) => {
                for group in ctx.admit_iter(&groups[..], "scan F3D full round fillet groups")? {
                    face_selection(group.center_faces());
                    for side in [group.side_one_faces(), group.side_two_faces()] {
                        if let cadmpeg_ir::features::edge_treatments::FullRoundSideSelection::Explicit(selection) =
                            side
                        {
                            face_selection(selection);
                        }
                    }
                }
            }
            FeatureDefinition::Operation(FeatureOperation::Chamfer { groups, .. }) => {
                for group in ctx.admit_iter(&groups[..], "scan F3D chamfer groups")? {
                    edge_selection(&group.edges)?;
                }
            }
            FeatureDefinition::Operation(FeatureOperation::Sweep {
                shape,

                path,
                guide_rail,
                ..
            }) => {
                if shape.any_section_names_an_unresolved_carrier() {
                    gaps.profile_selections += 1;
                }
                let (primary, sheet_sections, solid_sections): (
                    Option<&PlanarProfileRef>,
                    &[cadmpeg_ir::features::SheetSweepSection],
                    &[cadmpeg_ir::features::SweepSection],
                ) = match shape {
                    cadmpeg_ir::features::SweepShape::Unresolved { section, sections }
                    | cadmpeg_ir::features::SweepShape::Surface { section, sections } => {
                        (section.referenced_profile(), sections.as_slice(), &[])
                    }
                    cadmpeg_ir::features::SweepShape::Solid {
                        section, sections, ..
                    } => (section.referenced_profile(), &[], sections.as_slice()),
                };
                let mut count_profile = |profile: &PlanarProfileRef| -> Result<(), CodecError> {
                    if matches!(
                        profile,
                        PlanarProfileRef::Native(_)
                            | PlanarProfileRef::Unresolved(_)
                            | PlanarProfileRef::SketchSelection { .. }
                    ) {
                        gaps.profile_selections =
                            gaps.profile_selections.checked_add(1).ok_or_else(|| {
                                ctx.refuse_codec_limit(
                                    "count F3D Sweep profile gaps",
                                    u64::MAX - 1,
                                    u64::MAX,
                                )
                            })?;
                    }
                    Ok(())
                };
                if let Some(profile) = primary {
                    count_profile(profile)?;
                }
                for profile in ctx
                    .admit_iter(sheet_sections, "scan F3D sheet Sweep sections")?
                    .filter_map(cadmpeg_ir::features::SweepSection::referenced_profile)
                    .chain(
                        ctx.admit_iter(solid_sections, "scan F3D solid Sweep sections")?
                            .filter_map(cadmpeg_ir::features::SweepSection::referenced_profile),
                    )
                {
                    count_profile(profile)?;
                }
                if path.as_ref().is_some_and(|path| {
                    matches!(
                        path,
                        PathRef::Native(_)
                            | PathRef::Unresolved(_)
                            | PathRef::SpatialSketchSelection { .. }
                    )
                }) {
                    gaps.path_selections += 1;
                }
                if guide_rail.as_ref().is_some_and(|guide| {
                    matches!(
                        &guide.path,
                        PathRef::Native(_)
                            | PathRef::Unresolved(_)
                            | PathRef::SpatialSketchSelection { .. }
                    )
                }) {
                    gaps.path_selections += 1;
                }
            }
            FeatureDefinition::Operation(FeatureOperation::DatumPoint {
                construction: Some(construction),
                ..
            }) => {
                use cadmpeg_ir::features::{DatumPlaneReference, DatumPointConstruction};

                let mut plane = |reference: &DatumPlaneReference| {
                    if let DatumPlaneReference::Face { face } = reference {
                        face_selection(face);
                    }
                };
                match construction.as_ref() {
                    DatumPointConstruction::CircleCenter { edge }
                    | DatumPointConstruction::DistanceOnEdge { edge, .. } => edge_selection(edge)?,
                    DatumPointConstruction::TwoEdgeIntersection { edges } => {
                        for edge in edges {
                            edge_selection(edge)?;
                        }
                    }
                    DatumPointConstruction::ThreePlaneIntersection { planes } => {
                        for reference in
                            ctx.admit_iter(planes.as_ref(), "scan F3D datum point planes")?
                        {
                            plane(reference);
                        }
                    }
                    DatumPointConstruction::Vertex { .. }
                    | DatumPointConstruction::SketchPoint { .. } => {}
                    DatumPointConstruction::EdgePlaneIntersection {
                        edge,
                        plane: reference,
                    } => {
                        edge_selection(edge)?;
                        plane(reference);
                    }
                }
            }
            FeatureDefinition::Operation(FeatureOperation::FilledSurface {
                boundary,
                support_faces,
                ..
            }) => {
                match boundary {
                    cadmpeg_ir::features::SurfaceBoundary::Edges(edges) => edge_selection(edges)?,
                    cadmpeg_ir::features::SurfaceBoundary::Path(path) => {
                        gaps.path_selections += usize::from(!loft_path_is_resolved(path));
                    }
                }
                face_selection(support_faces);
            }
            FeatureDefinition::Operation(FeatureOperation::Loft {
                sections, guidance, ..
            }) => {
                let section_gaps = ctx
                    .admit_iter(sections.as_slice(), "scan F3D Loft section projections")?
                    .try_fold(0usize, |count, section| {
                        let selected = {
                            matches!(
                                section,
                                cadmpeg_ir::features::LoftSection::Profile(profile)
                                    if !profile_ref_is_resolved(profile)
                            )
                        };
                        count.checked_add(usize::from(selected)).ok_or_else(|| {
                            ctx.refuse_codec_limit(
                                "count F3D projection gaps",
                                u64::MAX - 1,
                                u64::MAX,
                            )
                        })
                    })?;
                gaps.profile_selections = gaps
                    .profile_selections
                    .checked_add(section_gaps)
                    .ok_or_else(|| {
                        ctx.refuse_codec_limit(
                            "count F3D Loft profile gaps",
                            u64::MAX - 1,
                            u64::MAX,
                        )
                    })?;
                let guide_gaps = match guidance {
                    cadmpeg_ir::features::LoftGuidance::Guides(paths) => ctx
                        .admit_iter(paths.as_slice(), "scan F3D Loft guide projections")?
                        .try_fold(0usize, |count, path| {
                            let selected = !loft_path_is_resolved(path);
                            count.checked_add(usize::from(selected)).ok_or_else(|| {
                                ctx.refuse_codec_limit(
                                    "count F3D projection gaps",
                                    u64::MAX - 1,
                                    u64::MAX,
                                )
                            })
                        })?,
                    cadmpeg_ir::features::LoftGuidance::Centerline(path) => {
                        usize::from(!loft_path_is_resolved(path))
                    }
                };
                gaps.path_selections =
                    gaps.path_selections
                        .checked_add(guide_gaps)
                        .ok_or_else(|| {
                            ctx.refuse_codec_limit(
                                "count F3D Loft path gaps",
                                u64::MAX - 1,
                                u64::MAX,
                            )
                        })?;
            }
            FeatureDefinition::Operation(FeatureOperation::Shell {
                bodies,
                removed_faces,
                ..
            }) => {
                if let Some(bodies) = bodies {
                    gaps.body_selections += native_body_selection_count(bodies);
                }
                face_selection(removed_faces);
            }
            FeatureDefinition::Operation(FeatureOperation::CosmeticThread { face, .. }) => {
                face_selection(face);
            }
            FeatureDefinition::Operation(FeatureOperation::Decal { faces, .. }) => {
                face_selection(faces);
            }
            FeatureDefinition::Operation(
                FeatureOperation::DeleteFace { faces, .. }
                | FeatureOperation::OffsetSurface { faces, .. },
            ) => face_selection(faces),
            FeatureDefinition::Operation(FeatureOperation::ReplaceFace { operands }) => {
                let targets = operands.targets();
                let replacements = operands.replacements();
                face_selection(targets);
                face_selection(replacements);
            }
            FeatureDefinition::Operation(FeatureOperation::SheetMetalBaseFlange {
                profile,
                ..
            }) => {
                gaps.profile_selections += usize::from(!planar_profile_ref_is_resolved(profile));
            }
            FeatureDefinition::Operation(
                FeatureOperation::SheetMetalEdgeFlange { edges, .. }
                | FeatureOperation::SheetMetalHem { edges, .. },
            ) => edge_selection(edges)?,
            FeatureDefinition::Operation(FeatureOperation::MoveFace { faces, .. }) => {
                face_selection(faces);
            }
            _ => {}
        }
    }
    let mut repaired_storage = ctx.reserve_scoped(0, "hold F3D repaired lost-edge index")?;
    let mut repaired_lost_edge_reference_ids = HashSet::new();
    for group in ctx.admit_iter(
        &native.design_construction_operand_groups,
        "scan F3D complete edge groups",
    )? {
        if ctx.contains_hash_set(
            &complete_edge_selection_native_ids,
            group.id.as_str(),
            "find F3D complete edge group",
        )? {
            for id in ctx.admit_iter(
                &group.lost_edge_references,
                "scan F3D repaired lost-edge references",
            )? {
                repaired_storage.with_storage(|| {
                    ctx.insert_hash_set(
                        &mut repaired_lost_edge_reference_ids,
                        id.as_str(),
                        "index repaired F3D lost edge references",
                    )
                })?;
            }
        }
    }
    gaps.unrepaired_lost_edge_references = ctx
        .admit_iter(
            &native.lost_edge_references,
            "scan F3D projection lost_edge_references",
        )?
        .try_fold(0usize, |count, reference| {
            let selected = !ctx.contains_hash_set(
                &repaired_lost_edge_reference_ids,
                reference.id.as_str(),
                "find F3D repaired lost-edge reference",
            )?;
            count.checked_add(usize::from(selected)).ok_or_else(|| {
                ctx.refuse_codec_limit("count F3D projection gaps", u64::MAX - 1, u64::MAX)
            })
        })?;
    Ok(gaps)
}

struct IncompleteFamilyCounts<'a>(&'a std::collections::BTreeMap<&'a str, usize>);

impl std::fmt::Display for IncompleteFamilyCounts<'_> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        for (index, (family, count)) in self.0.iter().enumerate() {
            if index != 0 {
                formatter.write_str(", ")?;
            }
            write!(formatter, "{family}={count}")?;
        }
        Ok(())
    }
}

fn push_loss_vec(
    ctx: &DecodeContext<'_>,
    losses: &mut Vec<cadmpeg_ir::report::loss::LossNote>,
    code: F3dLossCode,
    args: std::fmt::Arguments<'_>,
    collection_operation: &'static str,
    retained_operation: &'static str,
) -> Result<(), CodecError> {
    ctx.reserve_vec(losses, 1, collection_operation)?;
    losses.push(code.note(ctx.format_retained(args, retained_operation)?));
    Ok(())
}

fn report_design_projection_gaps(
    ctx: &DecodeContext<'_>,
    report: &mut DecodeBody,
    ir: &CadIr,
    native: &F3dNative,
) -> Result<(), CodecError> {
    let gaps = design_projection_gaps(ctx, ir, native)?;
    let (_family_storage, incomplete_families) = incomplete_feature_families(ctx, ir)?;
    let history_budget_skips = ctx
        .admit_iter(
            &native.asm_histories,
            "scan F3D history binding budget results",
        )?
        .filter(|history| history.record_table_binding_budget_exceeded)
        .count();
    if history_budget_skips != 0 {
        push_loss_vec(ctx, &mut report.losses, F3dLossCode::HistoryBindingBudgetExceeded, format_args!(
            "{history_budget_skips} ASM history stream(s) retain no historical topology because their binding work exceeded the decoder safety budget."
        ), "collect F3D projection losses", "retain F3D projection loss")?;
    }
    for history in ctx.admit_iter(&native.asm_histories, "scan F3D history framing results")? {
        for state in ctx.admit_iter(&history.states, "scan F3D history framing states")? {
            for record in ctx.admit_iter(&state.records, "scan F3D history framing records")? {
                let Some(error) = record.framing_error() else {
                    continue;
                };
                push_loss_vec(
                    ctx,
                    &mut report.losses,
                    F3dLossCode::HistoryRecordFramingFailed,
                    format_args!(
                        "An ASM history span remains opaque because record framing failed: {error}."
                    ),
                    "collect F3D projection losses",
                    "retain F3D projection loss",
                )?;
            }
        }
    }
    if gaps.unresolved_body_bindings != 0 {
        push_loss_vec(
            ctx,
            &mut report.losses,
            F3dLossCode::DesignBodyBindingUnresolved,
            format_args!(
                "{} Design body-map pair(s) do not resolve to a body in the named BREP blob.",
                gaps.unresolved_body_bindings
            ),
            "collect F3D projection losses",
            "retain F3D projection loss",
        )?;
    }
    if gaps.native_reference_images != 0 {
        push_loss_vec(ctx, &mut report.losses, F3dLossCode::ReferenceImageNativeRetained, format_args!(
            "{} reference-image timeline object(s) retain native Canvas records because no neutral image-plane binding was resolved.",
            gaps.native_reference_images
        ), "collect F3D projection losses", "retain F3D projection loss")?;
    }
    if gaps.native_decals != 0 {
        push_loss_vec(ctx, &mut report.losses, F3dLossCode::DecalNativeRetained, format_args!(
            "{} decal timeline object(s) retain native image and mapping records because no neutral decal binding was resolved.",
            gaps.native_decals
        ), "collect F3D projection losses", "retain F3D projection loss")?;
    }
    if gaps.unrepaired_lost_edge_references != 0 {
        push_loss_vec(ctx, &mut report.losses, F3dLossCode::EdgeReferenceLostUnrepaired, format_args!(
            "{} source parametric edge reference(s) were marked EDGE_REFERENCE_LOST and have no independent complete selection proof.",
            gaps.unrepaired_lost_edge_references
        ), "collect F3D projection losses", "retain F3D projection loss")?;
    }
    let mut push = |code: F3dLossCode,
                    count: usize,
                    message: std::fmt::Arguments<'_>|
     -> Result<(), CodecError> {
        if count != 0 {
            push_loss_vec(
                ctx,
                &mut report.losses,
                code,
                message,
                "collect F3D projection losses",
                "retain F3D projection loss",
            )?;
        }
        Ok(())
    };
    push(
        F3dLossCode::FeatureDefinitionIncomplete,
        gaps.incomplete_features,
        format_args!(
            "{} feature scope(s) have no complete neutral feature definition: {}.",
            gaps.incomplete_features,
            IncompleteFamilyCounts(&incomplete_families)
        ),
    )?;
    push(
        F3dLossCode::FeatureScopeUnprojected,
        gaps.unprojected_feature_scopes,
        format_args!(
            "{} decoded feature scope(s) have no neutral construction-history feature.",
            gaps.unprojected_feature_scopes
        ),
    )?;
    push(
        F3dLossCode::ParameterUnprojected,
        gaps.unprojected_parameters,
        format_args!(
            "{} decoded Design parameter(s) have no neutral parameter.",
            gaps.unprojected_parameters
        ),
    )?;
    push(
        F3dLossCode::ParameterOwnerUnrecognized,
        gaps.unresolved_parameter_owners,
        format_args!(
            "{} decoded Design parameter owner binding(s) have no recognized feature scope.",
            gaps.unresolved_parameter_owners
        ),
    )?;
    push(
        F3dLossCode::ParameterUnitUntyped,
        gaps.untyped_parameter_units,
        format_args!(
            "{} decoded Design parameter(s) retain unit tokens without a settled neutral quantity kind.",
            gaps.untyped_parameter_units
        ),
    )?;
    push(
        F3dLossCode::ParameterExpressionUnbound,
        gaps.unresolved_expression_dependencies,
        format_args!(
            "{} decoded parameter expression symbol(s) name same-stream parameters without a neutral dependency edge.",
            gaps.unresolved_expression_dependencies
        ),
    )?;
    push(
        F3dLossCode::HistoryDependencyUnprojected,
        gaps.unprojected_history_dependencies,
        format_args!(
            "{} feature history-state dependency link(s) were not projected into neutral construction history.",
            gaps.unprojected_history_dependencies
        ),
    )?;
    push(
        F3dLossCode::HistoryDependencyAmbiguous,
        gaps.ambiguous_history_dependencies,
        format_args!(
            "{} feature history-state dependency link(s) have multiple source scopes for the preceding state identity.",
            gaps.ambiguous_history_dependencies
        ),
    )?;
    push(
        F3dLossCode::SketchRelationNativeRetained,
        gaps.native_sketch_relations,
        format_args!(
            "{} sketch relation(s) retain native operands because no unique neutral relation was resolved.",
            gaps.native_sketch_relations
        ),
    )?;
    push(
        F3dLossCode::SketchDimensionNativeRetained,
        gaps.native_dimensions,
        format_args!(
            "{} sketch dimension(s) retain native operands because no unique neutral dimension was resolved.",
            gaps.native_dimensions
        ),
    )?;
    push(
        F3dLossCode::SketchPlacementUnprojected,
        gaps.unprojected_sketch_placements,
        format_args!(
            "{} decoded Sketch placement(s) have no neutral sketch.",
            gaps.unprojected_sketch_placements
        ),
    )?;
    push(
        F3dLossCode::SketchPointUnprojected,
        gaps.unprojected_sketch_points,
        format_args!(
            "{} decoded sketch point(s) have no neutral sketch entity.",
            gaps.unprojected_sketch_points
        ),
    )?;
    push(
        F3dLossCode::SketchCurveUnprojected,
        gaps.unprojected_sketch_curves,
        format_args!(
            "{} decoded sketch curve(s) have no neutral sketch entity.",
            gaps.unprojected_sketch_curves
        ),
    )?;
    push(
        F3dLossCode::SketchSurfaceUnprojected,
        gaps.unprojected_sketch_surfaces,
        format_args!(
            "{} decoded sketch surface(s) have no neutral spatial sketch entity.",
            gaps.unprojected_sketch_surfaces
        ),
    )?;
    push(
        F3dLossCode::SketchTextUnprojected,
        gaps.unprojected_sketch_texts,
        format_args!(
            "{} decoded sketch text record(s) have no neutral sketch entity.",
            gaps.unprojected_sketch_texts
        ),
    )?;
    push(
        F3dLossCode::SketchRelationUnprojected,
        gaps.unprojected_sketch_relations,
        format_args!(
            "{} decoded sketch relation(s) have no neutral constraint.",
            gaps.unprojected_sketch_relations
        ),
    )?;
    push(
        F3dLossCode::DimensionUnprojected,
        gaps.unprojected_dimensions,
        format_args!(
            "{} Design dimension parameter(s) have no parameter-backed neutral or native sketch constraint.",
            gaps.unprojected_dimensions
        ),
    )?;
    push(
        F3dLossCode::FeatureProfileSelectionNative,
        gaps.profile_selections,
        format_args!(
            "{} feature profile selection(s) retain native selection identities because no unique neutral profile was resolved.",
            gaps.profile_selections
        ),
    )?;
    push(
        F3dLossCode::FeaturePathSelectionNative,
        gaps.path_selections,
        format_args!(
            "{} feature path selection(s) retain native selection identities because no unique neutral path was resolved.",
            gaps.path_selections
        ),
    )?;
    push(
        F3dLossCode::FeatureFaceSelectionNative,
        gaps.face_selections,
        format_args!(
            "{} feature face selection(s) retain native candidates because no unique topological face was resolved.",
            gaps.face_selections
        ),
    )?;
    push(
        F3dLossCode::FeatureFaceSelectionActiveSubstituted,
        gaps.active_face_substitutions,
        format_args!(
            "{} legacy face operand(s) use a current active-BREP face because no unique preceding-state face slot resolved.",
            gaps.active_face_substitutions
        ),
    )?;
    push(
        F3dLossCode::FeatureBodySelectionNative,
        gaps.body_selections,
        format_args!(
            "{} feature body selection(s) retain native identities because no unique solved body was resolved.",
            gaps.body_selections
        ),
    )?;
    push(
        F3dLossCode::FeatureFaceOperandUnresolved,
        gaps.partially_resolved_face_members,
        format_args!(
            "{} feature face operand(s) remain unresolved inside state-bound historical selections.",
            gaps.partially_resolved_face_members
        ),
    )?;
    push(
        F3dLossCode::FeatureEdgeSelectionNative,
        gaps.native_edge_selections,
        format_args!(
            "{} edge-treatment selection(s) retain native construction recipes because no neutral historical edge selection was resolved.",
            gaps.native_edge_selections
        ),
    )?;
    push(
        F3dLossCode::FeatureEdgeOperandUnresolved,
        gaps.partially_resolved_edge_members,
        format_args!(
            "{} edge-treatment operand(s) remain unresolved inside state-bound historical selections.",
            gaps.partially_resolved_edge_members
        ),
    )?;
    push(
        F3dLossCode::FeatureEdgeSelectionLost,
        gaps.unresolved_edge_selections,
        format_args!(
            "{} edge-treatment selection(s) are unresolved because their source edge references were lost.",
            gaps.unresolved_edge_selections
        ),
    )?;
    Ok(())
}

fn model_brep_candidates<'s>(
    ctx: &DecodeContext<'_>,
    scan: &'s ContainerScan<'_>,
    blob_names: &[String],
) -> Result<Vec<&'s BrepFacts>, CodecError> {
    if blob_names.is_empty() {
        return Ok(Vec::new());
    }
    // Design BREP entries by archive basename, built once; a repeated
    // basename is kept as ambiguous and refused when a body map names it.
    let mut by_basename_storage = ctx.reserve_scoped(0, "index F3D model BREP basenames")?;
    let mut by_basename = std::collections::HashMap::new();
    for brep in container::design_breps(ctx, scan)? {
        let brep = brep?;
        let basename = entry_basename(ctx, &brep.name)?;
        by_basename_storage.with_storage(|| {
            match ctx.entry_hash_map(
                &mut by_basename,
                basename,
                "index F3D model BREP basenames",
            )? {
                std::collections::hash_map::Entry::Vacant(entry) => {
                    entry.insert(Some(brep));
                }
                std::collections::hash_map::Entry::Occupied(mut entry) => {
                    entry.insert(None);
                }
            }
            Ok::<(), CodecError>(())
        })?;
    }
    let mut candidates = Vec::new();
    for blob_name in ctx.admit_iter(blob_names, "scan F3D blob names")? {
        let brep = match ctx.get_hash_map(
            &by_basename,
            blob_name.as_str(),
            "find F3D model BREP by basename",
        )? {
            None => {
                return Err(CodecError::malformed(format_args!(
                    "Design body map references missing BREP entry {blob_name}"
                )))
            }
            Some(None) => {
                return Err(CodecError::malformed(format_args!(
                    "Design body map BREP basename is ambiguous: {blob_name}"
                )))
            }
            Some(Some(brep)) => *brep,
        };
        ctx.push_vec(&mut candidates, brep, "collect F3D model BREP candidates")?;
    }
    Ok(candidates)
}

/// The text after the last `/` of an archive path, or the whole path.
fn entry_basename<'n>(ctx: &DecodeContext<'_>, name: &'n str) -> Result<&'n str, CodecError> {
    Ok(ctx
        .rsplit_once(name, "/", "split F3D archive basename")?
        .map_or(name, |(_, base)| base))
}

/// Decode the document model from its text-encoded carriers.
///
/// The text encoding is the model carrier only when no binary stream decoded,
/// so the caller runs this after the binary candidate loop. Every `.sat` and
/// `.smt` entry that parses and produces geometry joins the merged graph;
/// with more than one contributing entry, each graph is qualified by its
/// entry basename.
fn try_decode_text_model(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan<'_>,
) -> Result<Option<(BrepFacts, Brep)>, CodecError> {
    let mut parts: Vec<(BrepFacts, Brep)> = Vec::new();
    for name in container::text_brep_names(ctx, scan)? {
        let name = name?;
        let bytes = scan.entry_bytes(ctx, name)?;
        let stream = match ctx.get_hash_map(&scan.text_breps, name, "find F3D text BREP stream")? {
            Some(crate::container::TextBrepFraming::Parsed(stream)) => stream,
            Some(crate::container::TextBrepFraming::Unframed(error)) => {
                return Err(CodecError::malformed(format_args!(
                    "text BREP entry {name} failed to parse: {error}"
                )));
            }
            Some(crate::container::TextBrepFraming::Malformed(error)) => {
                return Err(CodecError::Malformed(ctx.format_retained(
                    format_args!("{error}"),
                    "format F3D malformed text BREP error",
                )?));
            }
            Some(crate::container::TextBrepFraming::UnsupportedLength(error)) => {
                return Err(CodecError::NotImplemented(ctx.format_retained(
                    format_args!("{error}"),
                    "format F3D unsupported text BREP error",
                )?));
            }
            None => {
                return Err(CodecError::malformed(format_args!(
                    "text BREP entry {name} was not framed"
                )))
            }
        };
        let decoded = brep::decode_text(ctx, stream, bytes, name, crate::ids::ID_FORMAT)?;
        if decoded.asm.surfaces.is_empty()
            && decoded.asm.points.is_empty()
            && decoded.asm.faces.is_empty()
        {
            continue;
        }
        // Facts for the report and source attributes. The header carries the
        // stream's own unit; the decoded token values are already in the
        // centimetre convention.
        let mut header = stream.header.as_kernel_header(ctx)?;
        header.scale = Some(stream.header.scale().get());

        ctx.reserve_vec(&mut parts, 1, "collect F3D text B-rep parts")?;
        parts.push((
            BrepFacts {
                name: ctx.copy_retained_text(name, "retain F3D text B-rep fact name")?,
                uncompressed_len: u64_from_index(bytes.len()),
                kernel: Some(crate::container::KernelFraming::Text {
                    header,
                    terminator: stream.terminator,
                }),
                sha256: cadmpeg_ir::hash::digest::Sha256Digest::digest_for_decode(
                    ctx,
                    bytes,
                    "retain source digest",
                )?,
            },
            decoded,
        ));
    }
    let qualify = parts.len() > 1;
    let mut merged: Option<(BrepFacts, Brep)> = None;
    for (facts, mut part) in parts {
        if qualify {
            let namespace = entry_basename(ctx, &facts.name)?;
            part.qualify_ids(ctx, crate::ids::ID_FORMAT, namespace)?;
        }
        match &mut merged {
            None => merged = Some((facts, part)),
            Some((_, whole)) => whole.append(ctx, part)?,
        }
    }
    Ok(merged)
}

/// Assemble the complete document result from one decoded model B-rep graph.
///
/// Both encodings converge here: the binary candidate loop and the text-stream
/// decode hand over the merged graph, the facts of the primary carrier, the
/// resolved body visibilities, and the count of candidates whose decode
/// produced nothing.
fn finish_model_decode<'a>(
    ctx: &DecodeContext<'a>,
    scan: &ContainerScan<'a>,
    primary_model_brep: &BrepFacts,
    brep: Brep,
    body_visibilities: Vec<crate::records::bodies::BodyVisibility>,
    undecoded_candidates: usize,
    session_state: DecodeSessionState,
) -> Result<AuthoredDecoded, CodecError> {
    let (session, path) = F3dDecodeSession::from_geometry(
        ctx,
        scan,
        primary_model_brep,
        brep,
        body_visibilities,
        undecoded_candidates,
        session_state,
    )?;
    session.into_result(path)
}

/// Optional geometry index after a successful B-rep transfer.
///
/// Absence means the document is assembled from Design / mesh / presentation
/// content alone; product decoding still runs through the same session path.
struct GeometryIndex {
    primary_model_brep_name: String,
    annotation_records: Vec<cadmpeg_asm::brep::annotations::AnnotationRecord>,
    mesh_projection: MeshProjection,
}

/// State shared by every finalization path for one decoded F3D member.
#[derive(Debug)]
struct DecodeSessionState {
    admitted_entities: u64,
    report_scope: crate::report::ReportScope,
}

struct DeferredBodylessInputs {
    xref: Result<Option<crate::xref::XrefTable>, CodecError>,
    non_root_act: usize,
    has_appearance: bool,
}

/// Geometry-path inputs carried from construction to product decoding.
struct GeometrySessionPath {
    index: GeometryIndex,
    materials: materials::DecodedMaterials,
}

enum SessionPath {
    Geometry(Box<GeometrySessionPath>),
    Bodyless(Box<materials::DecodedMaterials>),
}

/// The inputs one decoded path hands to finalization.
enum FinalizePath {
    Geometry(GeometryIndex),
    Bodyless(DeferredBodylessInputs),
}

/// Private decode accumulator for one `.f3d` document.
struct F3dDecodeSession<'a> {
    ctx: &'a DecodeContext<'a>,
    scan: &'a ContainerScan<'a>,
    native: F3dNative,
    ir: CadIr,
    source_attributes: std::collections::BTreeMap<String, String>,
    report: DecodeBody,
    report_scope: crate::report::ReportScope,
    unknowns: Vec<UnknownRecord>,
    admitted_entities: u64,
}

impl<'a> F3dDecodeSession<'a> {
    fn from_geometry(
        ctx: &'a DecodeContext<'a>,
        scan: &'a ContainerScan<'a>,
        primary_model_brep: &BrepFacts,
        brep: Brep,
        body_visibilities: Vec<crate::records::bodies::BodyVisibility>,
        undecoded_candidates: usize,
        session_state: DecodeSessionState,
    ) -> Result<(Self, SessionPath), CodecError> {
        let DecodeSessionState {
            admitted_entities: _,
            report_scope,
        } = session_state;
        let mut report = crate::report::build_decode_report(
            ctx,
            scan,
            cadmpeg_ir::report::decode::DecodeTransfer::full(true),
            geometry_losses(ctx, &brep)?,
        )?;
        if undecoded_candidates != 0 {
            push_loss_vec(
                ctx,
                &mut report.losses,
                F3dLossCode::BrepBlobUndecoded,
                format_args!(
                    "{undecoded_candidates} Design-referenced BREP blob(s) could not be decoded."
                ),
                "collect F3D undecoded BREP loss",
                "retain F3D undecoded BREP loss",
            )?;
        }
        let design_body_bindings = crate::design::decode::body::decode_design_body_bindings(
            ctx,
            scan,
            Some(&primary_model_brep.name),
            &brep.asm.body_native_keys,
        )?;
        let geometry_materials =
            materials::decode_with_body_bindings(ctx, scan, &design_body_bindings)?;
        let (mut ir, source_attributes, mut native, asm_remainder) =
            build_geometry_ir(ctx, scan, primary_model_brep, brep)?;
        // ASM transfer already charged its delta; keep the running counter in
        // sync so a later admit_entities call cannot double-count those bodies.
        let mut admitted_entities = u64_from_index(ir.model.entity_count());
        let AsmTransferRemainder {
            unknowns,
            stats: _,
            annotation_records,
        } = asm_remainder;
        let (subds, subd_losses) = crate::tsm::decode(ctx, scan)?;
        ir.model.subds = subds;
        let mesh_projection = project_mesh_bodies(ctx, scan, &mut ir, &mut native, &mut report)?;
        ctx.extend_vec(
            &mut report.losses,
            subd_losses,
            "append F3D T-spline losses",
        )?;
        native.body_visibilities = body_visibilities;
        native.design_body_bindings = design_body_bindings;
        ctx.admit_entities(
            u64_from_index(ir.model.entity_count()),
            &mut admitted_entities,
            "admit F3D geometry entities",
        )?;
        Ok((
            Self {
                ctx,
                scan,
                native,
                ir,
                source_attributes,
                report,
                report_scope,
                unknowns,
                admitted_entities,
            },
            SessionPath::Geometry(Box::new(GeometrySessionPath {
                index: GeometryIndex {
                    primary_model_brep_name: ctx.copy_retained_text(
                        &primary_model_brep.name,
                        "retain F3D primary BREP name",
                    )?,
                    annotation_records,
                    mesh_projection,
                },
                materials: geometry_materials,
            })),
        ))
    }

    fn from_metadata(
        ctx: &'a DecodeContext<'a>,
        scan: &'a ContainerScan<'a>,
        session_state: DecodeSessionState,
    ) -> Result<(Self, SessionPath), CodecError> {
        let DecodeSessionState {
            admitted_entities,
            report_scope,
        } = session_state;
        let MetadataIr {
            ir,
            source_attributes,
            unknowns,
        } = build_metadata_ir(ctx, scan)?;
        let decoded_materials = materials::decode(ctx, scan)?;
        Ok((
            Self {
                ctx,
                scan,
                native: F3dNative::default(),
                ir,
                source_attributes,
                report: crate::report::build_decode_report(
                    ctx,
                    scan,
                    cadmpeg_ir::report::decode::DecodeTransfer::full(false),
                    container_losses(ctx, scan)?,
                )?,
                report_scope,
                unknowns,
                admitted_entities,
            },
            SessionPath::Bodyless(Box::new(decoded_materials)),
        ))
    }

    fn admit_model_entities(&mut self, operation: &'static str) -> Result<(), CodecError> {
        self.ctx.admit_entities(
            u64_from_index(self.ir.model.entity_count()),
            &mut self.admitted_entities,
            operation,
        )
    }

    /// Decode design graph, products, annotations, and the report.
    fn into_result(mut self, path: SessionPath) -> Result<AuthoredDecoded, CodecError> {
        self.admit_model_entities("admit F3D geometry entities")?;
        let mut path = path;
        self.decode_design_graph(&mut path)?;
        let path = self.decode_products(path)?;
        self.finalize(path)
    }

    fn decode_design_graph(&mut self, path: &mut SessionPath) -> Result<(), CodecError> {
        let scan = self.scan;
        let ctx = self.ctx;
        for history_brep in container::history_breps(ctx, scan)? {
            let history_brep = history_brep?;
            if let Some(history) = decode_asm_history(ctx, scan, history_brep)? {
                ctx.push_vec(
                    &mut self.native.asm_histories,
                    history,
                    "collect F3D ASM histories",
                )?;
            }
        }
        self.native.construction_recipes =
            crate::design::decode::parameters::decode_recipes(ctx, scan)?;
        self.native.persistent_references =
            crate::design::decode::sketch::decode_persistent_references(ctx, scan)?;
        self.native.lost_edge_references =
            crate::design::decode::sketch::decode_lost_edge_references(ctx, scan)?;
        self.native.design_material_assignments = std::mem::take(match path {
            SessionPath::Geometry(geometry_path) => &mut geometry_path.materials.assignments,
            SessionPath::Bodyless(decoded_materials) => &mut decoded_materials.assignments,
        });
        self.native.design_types = crate::design::decode::meta::decode_types(ctx, scan)?;
        self.native.design_parameters =
            crate::design::decode::parameters::decode_parameters(ctx, scan)?;
        self.native.design_entity_headers = crate::design::decode::sketch::decode_entity_headers(
            ctx,
            scan,
            &self.native.design_types,
        )?;
        self.native.design_record_headers = crate::design::decode::sketch::decode_record_headers(
            ctx,
            scan,
            &self.native.design_entity_headers,
        )?;
        self.native.sketch_relations = crate::design::decode::sketch::decode_sketch_relations(
            ctx,
            scan,
            &self.native.design_types,
            &self.native.design_record_headers,
        )?;
        extend_related_design_records(self.ctx, scan, &mut self.native)?;
        self.native.sketch_points = crate::design::decode::sketch::decode_sketch_points(ctx, scan)?;
        self.native.sketch_texts = crate::design::decode::sketch::decode_sketch_texts(ctx, scan)?;
        self.native.sketch_curve_identities =
            crate::design::decode::sketch::decode_sketch_curve_identities(ctx, scan)?;
        self.native.sketch_surfaces =
            crate::design::decode::sketch::decode_sketch_surfaces(ctx, scan)?;
        crate::design::decode::sketch::bind_sketch_graph(
            ctx,
            &self.native.design_entity_headers,
            &mut self.native.sketch_points,
            &mut self.native.sketch_curve_identities,
            &mut self.native.sketch_surfaces,
            &mut self.native.sketch_relations,
        )?;
        crate::design::decode::operands::bind_work_point_input_carriers(
            ctx,
            scan,
            &mut self.native.design_parameter_scopes,
            &self.native.design_record_headers,
            &self.native.construction_recipes,
            &self.native.design_edge_operands,
            &self.native.sketch_points,
        )?;
        crate::design::decode::operands::bind_extrude_selection_geometry(
            ctx,
            &mut self.native.design_extrude_selection_members,
            &self.native.design_extrude_selection_groups,
            &self.native.design_parameter_scopes,
            &self.native.sketch_points,
            &self.native.sketch_curve_identities,
        )?;
        let dimension_inputs = crate::design::decode::dimension_frames::DimensionDecodeInputs {
            scan,
            placements: &self.native.design_sketch_placements,
            parameters: &self.native.design_parameters,
            owners: &self.native.design_parameter_owners,
            companions: &self.native.design_parameter_companions,
            scopes: &self.native.design_parameter_scopes,
            headers: &self.native.design_record_headers,
            points: &self.native.sketch_points,
            curves: &self.native.sketch_curve_identities,
        };
        // The dimension passes share one index of each record stream.
        let mut dimension_records = crate::design::decode::sketch::RecordOffsetCache::new(ctx)?;
        self.native.design_dimension_locus_pairs =
            crate::design::decode::dimension_frames::decode_dimension_locus_pairs(
                ctx,
                &dimension_inputs,
                &mut dimension_records,
            )?
            .try_into()
            .map_err(|error: String| CodecError::malformed(format_args!("{error}")))?;
        self.native.design_dimension_annotation_frames =
            crate::design::decode::dimension_frames::decode_dimension_annotation_frames(
                ctx,
                &dimension_inputs,
                &mut dimension_records,
                &self.native.design_entity_headers,
            )?;
        self.native.design_dimension_presentation_frames =
            crate::design::decode::dimension_frames::decode_dimension_presentation_frames(
                ctx,
                &dimension_inputs,
                &mut dimension_records,
                &self.native.design_types,
                &self.native.design_entity_headers,
            )?;
        self.native.design_dimension_locus_groups =
            crate::design::decode::dimension_frames::decode_dimension_locus_groups(
                ctx,
                &dimension_inputs,
                &mut dimension_records,
                &self.native.design_entity_headers,
            )?;
        self.native.design_dimension_null_locus_pairs =
            crate::design::decode::dimension_frames::decode_dimension_null_locus_pairs(
                ctx,
                &dimension_inputs,
                &mut dimension_records,
                &self.native.design_dimension_locus_pairs,
                &self.native.design_dimension_locus_groups,
            )?
            .try_into()
            .map_err(|error: String| CodecError::malformed(format_args!("{error}")))?;
        drop(dimension_records);
        crate::design::dimensions::remove_dimension_frame_relations(
            ctx,
            &mut self.native.sketch_relations,
            &self.native.design_dimension_locus_pairs,
            &self.native.design_dimension_locus_groups,
            &self.native.design_dimension_null_locus_pairs,
        )?;
        crate::design::dimensions::bind_dimension_loci(
            ctx,
            crate::design::dimensions::DimensionLocusInputs {
                placements: &self.native.design_sketch_placements,
                owners: &self.native.design_parameter_owners,
                pairs: &self.native.design_dimension_locus_pairs,
                groups: &self.native.design_dimension_locus_groups,
                annotation_frames: &self.native.design_dimension_annotation_frames,
                null_pairs: &self.native.design_dimension_null_locus_pairs,
            },
            &mut self.native.sketch_points,
            &mut self.native.sketch_curve_identities,
        )?;
        self.native.design_body_members =
            crate::design::decode::body::decode_body_members(ctx, scan)?;
        if matches!(path, SessionPath::Bodyless(_)) {
            self.native.design_body_bindings =
                crate::design::decode::body::decode_design_body_bindings(
                    ctx,
                    scan,
                    None,
                    &self.native.body_native_keys,
                )?;
        }
        self.native.design_body_bounds = crate::design::decode::body::decode_body_bounds(
            ctx,
            scan,
            &self.native.design_entity_headers,
        )?;
        crate::design::decode::body::bind_body_bounds(
            ctx,
            &mut self.native.design_body_bounds,
            &self.native.design_body_bindings,
        )?;
        self.native.design_configurations =
            crate::design::configurations::decode_configurations(self.ctx, scan)?;
        self.ir.model.configurations = crate::design::configurations::project_configurations(
            self.ctx,
            &self.native.design_configurations,
        )?;
        (self.ir.model.features, self.ir.model.parameters) =
            crate::design::feature_project::project_parameter_design_with_edge_identities(
                self.ctx,
                &crate::design::feature_project::ProjectInputs {
                    native: &self.native.design_parameters,
                    owners: &self.native.design_parameter_owners,
                    scopes: &self.native.design_parameter_scopes,
                    timelines: &self.native.design_feature_timelines,
                    construction_groups: &self.native.design_construction_operand_groups,
                    fillet_radius_groups: &self.native.design_fillet_radius_groups,
                    edge_operands: &self.native.design_edge_operands,
                    edge_identity_operands: &self.native.design_edge_identity_operands,
                    edge_treatment_vertex_operands: &self
                        .native
                        .design_edge_treatment_vertex_operands,
                    entity_selection_operands: &self.native.design_entity_selection_operands,
                    curve_identities: &self.native.sketch_curve_identities,
                    face_operands: &self.native.design_face_operands,
                    body_recipe_operands: &self.native.design_body_recipe_operands,
                    legacy_loft_body_carriers: &self.native.design_loft_legacy_body_carriers,
                    placements: &self.native.design_sketch_placements,
                    body_bindings: &self.native.design_body_bindings,
                    component_naming_spaces: &self.native.design_component_naming_spaces,
                    histories: &self.native.asm_histories,
                },
            )?;
        crate::design::feature_project::bind_surface_trim_cell_selections(
            self.ctx,
            &mut self.ir.model.features,
            &self.native.design_parameter_scopes,
            &self.native.design_surface_trim_operations,
        )?;
        if let SessionPath::Geometry(geometry_path) = path {
            let geometry = &geometry_path.index;
            bind_mesh_feature_definitions(
                ctx,
                &mut self.ir.model.features,
                &self.native.design_parameter_scopes,
                &geometry.mesh_projection,
            )?;
        }
        crate::design::feature_project::form_cages::bind_form_cages(
            ctx,
            scan,
            &self.native.design_parameter_scopes,
            &mut self.ir.model.features,
            &self.ir.model.subds,
        )?;
        let canvas_assets = crate::design::decode::canvas::project_canvas_images(
            ctx,
            scan,
            &self.native.design_parameter_scopes,
            &self.native.design_canvas_images,
            &mut self.ir.model.features,
        )?;
        extend_unique_assets(ctx, &mut self.ir.model.assets, canvas_assets)?;
        let decal_assets = crate::design::decode::decal::project_decal_images(
            ctx,
            scan,
            &self.native.design_parameter_scopes,
            &self.native.design_decal_images,
            &self.native.design_construction_operand_groups,
            &self.native.design_body_recipe_operands,
            &mut self.ir.model.features,
        )?;
        extend_unique_assets(ctx, &mut self.ir.model.assets, decal_assets)?;
        crate::design::configurations::bind_configuration_parameter_overrides(
            self.ctx,
            &mut self.ir.model.configurations,
            &self.ir.model.parameters,
        )?;
        self.ir.model.feature_input_topologies = crate::history::project_feature_input_topologies(
            ctx,
            &self.ir.model.features,
            &self.native.design_parameter_scopes,
            &self.native.asm_histories,
            &self.native.design_edge_operands,
        )?;
        crate::history::bind_feature_outputs(
            ctx,
            &mut self.ir.model.features,
            &self.native.design_parameter_scopes,
            &self.native.asm_histories,
            &self.ir.model.bodies,
        )?;
        crate::history::bind_sweep_result_modes(
            ctx,
            &mut self.ir.model.features,
            &self.ir.model.bodies,
        )?;
        crate::history::bind_feature_body_selections(
            ctx,
            &mut self.ir.model.features,
            &crate::history::FeatureBodySelectionInputs {
                scopes: &self.native.design_parameter_scopes,
                groups: &self.native.design_construction_operand_groups,
                body_recipe_operands: &self.native.design_body_recipe_operands,
                construction_recipes: &self.native.construction_recipes,
                persistent_design_links: &self.native.persistent_design_links,
                histories: &self.native.asm_histories,
                bodies: &self.ir.model.bodies,
                regions: &self.ir.model.regions,
                shells: &self.ir.model.shells,
            },
        )?;
        crate::history::bind_feature_face_selections(
            self.ctx,
            &mut self.ir.model.features,
            &mut self.ir.model.feature_input_topologies,
            crate::history::FeatureFaceSelectionInputs {
                scopes: &self.native.design_parameter_scopes,
                groups: &self.native.design_construction_operand_groups,
                operands: &self.native.design_face_operands,
                entity_operands: &self.native.design_entity_selection_operands,
                body_recipe_operands: &self.native.design_body_recipe_operands,
                histories: &self.native.asm_histories,
            },
        )?;
        crate::history::bind_feature_path_selections(
            self.ctx,
            &mut self.ir.model.features,
            &self.native.design_parameter_scopes,
            &self.native.design_construction_operand_groups,
            &self.native.design_entity_selection_operands,
        )?;
        crate::design::feature_project::bind_revolve_face_axes(
            &mut self.ir.model.features,
            &self.native.design_parameter_scopes,
            &self.native.design_construction_operand_groups,
            &self.native.design_entity_selection_operands,
            &self.native.design_face_operands,
            &self.ir.model.faces,
            &self.ir.model.surfaces,
        );
        (self.ir.model.sketches, self.ir.model.sketch_entities) =
            crate::design::sketch_project::project_sketch_design(
                self.ctx,
                &self.native.design_sketch_placements,
                &self.native.sketch_points,
                &self.native.sketch_curve_identities,
                &self.native.sketch_relations,
                &self.native.sketch_texts,
                self.ir.tolerances.linear.get(),
            )?;
        (
            self.ir.model.spatial_sketches,
            self.ir.model.spatial_sketch_entities,
        ) = crate::design::sketch_project::project_spatial_sketch_design(
            self.ctx,
            &self.native.design_sketch_placements,
            &self.native.sketch_points,
            &self.native.sketch_curve_identities,
            &self.native.sketch_surfaces,
            &self.native.sketch_relations,
            self.ir.tolerances.linear.get(),
        )?;
        crate::design::feature_project::bind_work_point_sketch_point_constructions(
            self.ctx,
            &mut self.ir.model.features,
            &self.native.design_parameter_scopes,
            &self.ir.model.sketch_entities,
            &self.ir.model.spatial_sketch_entities,
        )?;
        let arrangement_budget = ctx.work_budget(u64_from_index(
            crate::design::geometry::MAX_ARRANGEMENT_WALK_WORK,
        ));
        crate::design::profile_select::bind_sweep_sketch_selections(
            &mut self.ir.model.features,
            &crate::design::profile_select::SketchCurveSelectionResolution {
                scopes: &self.native.design_parameter_scopes,
                groups: &self.native.design_construction_operand_groups,
                operands: &self.native.design_entity_selection_operands,
                placements: &self.native.design_sketch_placements,
                curve_identities: &self.native.sketch_curve_identities,
                sketches: &self.ir.model.sketches,
                sketch_entities: &self.ir.model.sketch_entities,
                spatial_sketches: &self.ir.model.spatial_sketches,
                spatial_sketch_entities: &self.ir.model.spatial_sketch_entities,
            },
            ctx,
        )?;
        crate::design::profile_select::bind_split_face_sketch_selections(
            &mut self.ir.model.features,
            &crate::design::profile_select::SketchCurveSelectionResolution {
                scopes: &self.native.design_parameter_scopes,
                groups: &self.native.design_construction_operand_groups,
                operands: &self.native.design_entity_selection_operands,
                placements: &self.native.design_sketch_placements,
                curve_identities: &self.native.sketch_curve_identities,
                sketches: &self.ir.model.sketches,
                sketch_entities: &self.ir.model.sketch_entities,
                spatial_sketches: &self.ir.model.spatial_sketches,
                spatial_sketch_entities: &self.ir.model.spatial_sketch_entities,
            },
            ctx,
        )?;
        crate::design::profile_select::bind_surface_trim_sketch_selections(
            &mut self.ir.model.features,
            &crate::design::profile_select::SketchCurveSelectionResolution {
                scopes: &self.native.design_parameter_scopes,
                groups: &self.native.design_construction_operand_groups,
                operands: &self.native.design_entity_selection_operands,
                placements: &self.native.design_sketch_placements,
                curve_identities: &self.native.sketch_curve_identities,
                sketches: &self.ir.model.sketches,
                sketch_entities: &self.ir.model.sketch_entities,
                spatial_sketches: &self.ir.model.spatial_sketches,
                spatial_sketch_entities: &self.ir.model.spatial_sketch_entities,
            },
            ctx,
        )?;
        crate::design::profile_select::bind_loft_and_revolve_sketch_selections(
            ctx,
            scan,
            &self.native.design_construction_operand_groups,
            &self.native.design_record_headers,
            &crate::design::profile_select::SketchProfileResolution {
                entities: &self.native.design_entity_headers,
                entity_selection_operands: &self.native.design_entity_selection_operands,
                placements: &self.native.design_sketch_placements,
                curve_identities: &self.native.sketch_curve_identities,
                sketches: &self.ir.model.sketches,
                sketch_entities: &self.ir.model.sketch_entities,
                spatial_sketches: &self.ir.model.spatial_sketches,
                spatial_sketch_entities: &self.ir.model.spatial_sketch_entities,
                linear_tolerance: self.ir.tolerances.linear.get(),
                angular_tolerance: self.ir.tolerances.angular.get(),
            },
            &mut self.ir.model.features,
        )?;
        crate::design::feature_project::bind_sketch_feature_geometry(
            self.ctx,
            &mut self.ir.model.features,
            &self.native.design_parameter_scopes,
            &self.native.design_sketch_placements,
            &self.ir.model.sketches,
            &self.ir.model.spatial_sketches,
        )?;
        self.ir.model.spatial_sketch_constraints =
            crate::design::sketch_project::project_spatial_sketch_constraints(
                self.ctx,
                &self.native.design_sketch_placements,
                &self.native.sketch_relations,
                &self.native.sketch_points,
                &self.native.sketch_curve_identities,
                &self.native.sketch_surfaces,
                &self.ir.model.spatial_sketch_entities,
            )?;
        let scope_histories = crate::history::bind_scope_histories(
            self.ctx,
            &self.native.design_parameter_scopes,
            &self.native.design_body_bindings,
            &self.native.design_body_recipe_operands,
            &self.native.asm_histories,
        )?;
        crate::design::profile_select::bind_extrude_profile_selections(
            &mut self.ir.model.features,
            &self.native.design_parameter_scopes,
            &self.native.design_extrude_selection_groups,
            &self.native.design_extrude_selection_members,
            &self.ir.model.sketches,
            &crate::design::profile_select::SketchCurveSelectionResolution {
                scopes: &self.native.design_parameter_scopes,
                groups: &self.native.design_construction_operand_groups,
                operands: &self.native.design_entity_selection_operands,
                placements: &self.native.design_sketch_placements,
                curve_identities: &self.native.sketch_curve_identities,
                sketches: &self.ir.model.sketches,
                sketch_entities: &self.ir.model.sketch_entities,
                spatial_sketches: &self.ir.model.spatial_sketches,
                spatial_sketch_entities: &self.ir.model.spatial_sketch_entities,
            },
            crate::design::profile_select::ExtrudeProfileResolution {
                entities: &self.ir.model.sketch_entities,
                spatial_sketches: &self.ir.model.spatial_sketches,
                spatial_entities: &self.ir.model.spatial_sketch_entities,
                histories: &self.native.asm_histories,
                scope_histories: &scope_histories,
                linear_tolerance: self.ir.tolerances.linear.get(),
                angular_tolerance: self.ir.tolerances.angular.get(),
                arrangement_budget: &arrangement_budget,
                ctx: self.ctx,
            },
        )?;
        if matches!(path, SessionPath::Geometry(_)) {
            crate::history::discard_projection_caches(self.ctx, &mut self.native.asm_histories)?;
        }
        let mut extrude_face_resolution = crate::design::face_resolve::ExtrudeFaceResolution {
            faces: &self.ir.model.faces,
            surfaces: &self.ir.model.surfaces,
            groups: &self.native.design_construction_operand_groups,
            operands: &mut self.native.design_face_operands,
            linear_tolerance: self.ir.tolerances.linear.get(),
            angular_tolerance: self.ir.tolerances.angular.get(),
        };
        crate::design::face_resolve::bind_extrude_start_planes(
            self.ctx,
            &mut self.ir.model.features,
            &self.ir.model.sketches,
            &mut extrude_face_resolution,
        )?;
        crate::design::face_resolve::bind_extrude_target_faces(
            self.ctx,
            &mut self.ir.model.features,
            &self.ir.model.sketches,
            &mut extrude_face_resolution,
        )?;
        self.ir.model.sketch_constraints = crate::design::constraints::project_sketch_constraints(
            self.ctx,
            &self.native.design_sketch_placements,
            &self.native.design_parameters,
            (
                &self.native.sketch_points,
                &self.native.sketch_curve_identities,
                &self.native.sketch_texts,
            ),
            &self.native.sketch_relations,
            &self.ir.model.sketch_entities,
        )?;
        let constraint_inputs = crate::design::dimensions::DimensionConstraintInputs {
            placements: &self.native.design_sketch_placements,
            parameters: &self.native.design_parameters,
            owners: &self.native.design_parameter_owners,
            pairs: &self.native.design_dimension_locus_pairs,
            groups: &self.native.design_dimension_locus_groups,
            annotation_frames: &self.native.design_dimension_annotation_frames,
            null_pairs: &self.native.design_dimension_null_locus_pairs,
            companions: &self.native.design_parameter_companions,
            recipe_records: &self.native.design_dimension_recipe_records,
            points: &self.native.sketch_points,
            curves: &self.native.sketch_curve_identities,
            entities: &self.ir.model.sketch_entities,
        };
        let dimension_constraints = if self.native.design_dimension_presentation_frames.is_empty() {
            crate::design::dimensions::project_dimension_constraints(
                self.ctx,
                &constraint_inputs,
                &self.ir.model.spatial_sketches,
                self.ir.tolerances.linear.get(),
            )
        } else {
            crate::design::dimensions::project_dimension_constraints_with_presentations(
                self.ctx,
                &constraint_inputs,
                &self.native.design_dimension_presentation_frames,
                &self.ir.model.spatial_sketches,
                self.ir.tolerances.linear.get(),
            )
        }?;
        self.ctx.extend_vec(
            &mut self.ir.model.sketch_constraints,
            dimension_constraints,
            "append F3D dimension constraints",
        )?;
        let spatial_dimension_constraints =
            crate::design::dimensions::project_spatial_dimension_constraints(
                self.ctx,
                &constraint_inputs,
                &self.ir.model.spatial_sketches,
                &self.ir.model.spatial_sketch_entities,
                self.ir.tolerances.linear.get(),
            )?;
        self.ctx.extend_vec(
            &mut self.ir.model.spatial_sketch_constraints,
            spatial_dimension_constraints,
            "append F3D spatial dimension constraints",
        )?;
        crate::design::dimensions::bind_offset_dimension_parameters(
            ctx,
            &mut self.ir.model.sketch_constraints,
            &self.native.design_parameters,
        )?;
        ctx.stable_sort_by(
            &mut self.ir.model.sketch_constraints,
            |value| &value.id,
            Ord::cmp,
            "sort F3D sketch constraints",
        )?;
        ctx.stable_sort_by(
            &mut self.ir.model.spatial_sketch_constraints,
            |value| &value.id,
            Ord::cmp,
            "sort F3D spatial sketch constraints",
        )?;
        crate::design::configurations::bind_configuration_suppressed_features(
            ctx,
            &mut self.ir.model.configurations,
            &self.ir.model.features,
        )?;
        Ok(())
    }

    fn decode_products(&mut self, path: SessionPath) -> Result<FinalizePath, CodecError> {
        let scan = self.scan;
        let act = crate::act::decode(self.ctx, scan)?;
        let non_root_act_component_links = act.non_root_component_links;
        self.native.act_entities = act.entities;
        self.native.act_guids = act.guids;
        self.native.act_registry_channels = act.registry_channels;
        self.native.act_root_components = act.root_components;
        self.native.act_table_references = act.table_references;

        let finalize_path = match path {
            SessionPath::Geometry(geometry_path) => {
                let GeometrySessionPath { index, materials } = *geometry_path;
                report_unretained_act_component_links(
                    self.ctx,
                    &mut self.report,
                    non_root_act_component_links,
                )?;
                report_unresolved_dimension_companions(
                    self.ctx,
                    &mut self.report,
                    &self.native,
                    &self.ir,
                )?;
                report_unresolved_configuration_rules(
                    self.ctx,
                    &mut self.report,
                    &self.native,
                    &self.ir,
                )?;
                report_untyped_material_distances(
                    self.ctx,
                    &mut self.report,
                    materials.untyped_distance_properties,
                )?;
                self.ctx.extend_vec(
                    &mut self.report.notes,
                    materials.notes,
                    "append F3D material notes",
                )?;
                self.ir.model.appearances = materials.appearances;
                self.ir.model.appearance_bindings = materials.bindings;
                resolve_face_appearance_bindings(
                    self.ctx,
                    &mut self.ir,
                    &materials.face_assignments,
                )?;
                apply_appearance_base_colors(self.ctx, &mut self.ir)?;
                self.ctx.stable_sort_by(
                    &mut self.ir.model.appearance_bindings,
                    |value| &value.id,
                    Ord::cmp,
                    "sort F3D appearance bindings",
                )?;
                reconcile_appearance_loss(
                    self.ctx,
                    &mut self.report,
                    &self.ir,
                    materials.has_topology_assignments,
                )?;
                annotate_docstruct(self.ctx, &mut self.source_attributes, scan)?;
                match crate::xref::decode_with_scopes(
                    self.ctx,
                    scan,
                    &self.native.design_parameter_scopes,
                ) {
                    Ok(Some(table)) => {
                        report_xref_placement_failures(self.ctx, &mut self.report, &table)?;
                        report_xref_placement_overrides(self.ctx, &mut self.report, &table)?;
                        self.ir.model.occurrences =
                            crate::xref::project_occurrences(self.ctx, &table)?;
                        crate::xref::bind_component_insert_features(
                            self.ctx,
                            &mut self.ir.model.features,
                            &self.native.design_parameter_scopes,
                            &table,
                        )?;
                        self.native.xref_designs = table.designs;
                        self.native.xref_references = table.references;
                    }
                    Ok(None) => {}
                    Err(error @ (CodecError::ResourceLimit(_) | CodecError::NotImplemented(_))) => {
                        return Err(error)
                    }
                    Err(error) => report_xref_parse_loss(self.ctx, &mut self.report, &error)?,
                }
                FinalizePath::Geometry(index)
            }
            SessionPath::Bodyless(decoded_materials) => {
                report_untyped_material_distances(
                    self.ctx,
                    &mut self.report,
                    decoded_materials.untyped_distance_properties,
                )?;
                self.ctx.extend_vec(
                    &mut self.report.notes,
                    decoded_materials.notes,
                    "append F3D material notes",
                )?;
                self.ir.model.appearances = decoded_materials.appearances;
                self.ir.model.appearance_bindings = decoded_materials.bindings;
                annotate_docstruct(self.ctx, &mut self.source_attributes, scan)?;
                let xref_table = match crate::xref::decode_with_scopes(
                    self.ctx,
                    scan,
                    &self.native.design_parameter_scopes,
                ) {
                    Err(error @ (CodecError::ResourceLimit(_) | CodecError::NotImplemented(_))) => {
                        return Err(error)
                    }
                    other => other,
                };
                if let Ok(Some(table)) = &xref_table {
                    report_xref_placement_failures(self.ctx, &mut self.report, table)?;
                    report_xref_placement_overrides(self.ctx, &mut self.report, table)?;
                    self.ir.model.occurrences = crate::xref::project_occurrences(self.ctx, table)?;
                    crate::xref::bind_component_insert_features(
                        self.ctx,
                        &mut self.ir.model.features,
                        &self.native.design_parameter_scopes,
                        table,
                    )?;
                }
                FinalizePath::Bodyless(DeferredBodylessInputs {
                    xref: xref_table,
                    non_root_act: non_root_act_component_links,
                    has_appearance: decoded_materials.has_topology_assignments,
                })
            }
        };

        let (components, occurrences) = crate::design::components::project_local_components(
            self.ctx,
            &self.native.design_parameter_scopes,
            &self.native.design_component_occurrences,
        )?;
        self.ctx.extend_vec(
            &mut self.ir.model.product_definitions,
            components,
            "append F3D local components",
        )?;
        self.ctx.extend_vec(
            &mut self.ir.model.occurrences,
            occurrences,
            "append F3D local occurrences",
        )?;
        crate::design::components::project_derived_instance_features(
            self.ctx,
            &mut self.ir.model.features,
            &self.native.design_parameter_scopes,
        )?;
        let unresolved_component_inserts =
            crate::design::components::project_unresolved_component_insert_occurrences(
                self.ctx,
                &mut self.ir.model.features,
                &self.native.design_parameter_scopes,
                self.ir.model.occurrences.len(),
            )?;
        self.ctx.extend_vec(
            &mut self.ir.model.occurrences,
            unresolved_component_inserts,
            "append F3D unresolved occurrences",
        )?;
        self.ir.model.assembly_joints = crate::design::assembly::project_assembly_joints(
            self.ctx,
            &self.native.design_parameter_scopes,
            &self.native.design_component_occurrences,
            &self.ir.model.features,
        )?;
        Ok(finalize_path)
    }

    fn finalize(mut self, path: FinalizePath) -> Result<AuthoredDecoded, CodecError> {
        let scan = self.scan;
        let ctx = self.ctx;
        let geometry = match path {
            FinalizePath::Geometry(index) => index,
            FinalizePath::Bodyless(inputs) => {
                report_unretained_act_component_links(ctx, &mut self.report, inputs.non_root_act)?;
                reconcile_appearance_loss(ctx, &mut self.report, &self.ir, inputs.has_appearance)?;
                let mesh_projection = project_mesh_bodies(
                    ctx,
                    scan,
                    &mut self.ir,
                    &mut self.native,
                    &mut self.report,
                )?;
                bind_mesh_feature_definitions(
                    ctx,
                    &mut self.ir.model.features,
                    &self.native.design_parameter_scopes,
                    &mesh_projection,
                )?;
                report_design_projection_gaps(ctx, &mut self.report, &self.ir, &self.native)?;
                ctx.admit_entities(
                    u64_from_index(self.ir.model.entity_count()),
                    &mut self.admitted_entities,
                    "admit F3D entities",
                )?;
                if mesh_projection.count > 0 {
                    apply_mesh_body_classification(
                        ctx,
                        &mut self.report,
                        scan,
                        mesh_projection.count,
                    )?;
                } else {
                    apply_bodyless_design_classification(
                        ctx,
                        &mut self.report,
                        count_result_items(
                            ctx,
                            container::design_breps(ctx, scan)?,
                            "count F3D Design BREP candidates",
                        )?,
                        count_result_items(
                            ctx,
                            container::text_brep_names(ctx, scan)?,
                            "count F3D text BREP carriers",
                        )?,
                        self.native.design_body_bindings.len()
                            + self.native.design_body_members.len(),
                        self.ir.model.sketch_entities.len()
                            + self.ir.model.spatial_sketch_entities.len(),
                        self.native.design_canvas_images.len(),
                    )?;
                }
                report_unresolved_dimension_companions(
                    self.ctx,
                    &mut self.report,
                    &self.native,
                    &self.ir,
                )?;
                match inputs.xref {
                    Ok(Some(table)) => {
                        apply_assembly_classification(self.ctx, &mut self.report, scan, &table)?;
                        self.native.xref_designs = table.designs;
                        self.native.xref_references = table.references;
                    }
                    Ok(None) => {}
                    Err(error @ (CodecError::ResourceLimit(_) | CodecError::NotImplemented(_))) => {
                        return Err(error)
                    }
                    Err(error) => report_xref_parse_loss(ctx, &mut self.report, &error)?,
                }
                self.native
                    .store(ctx, self.ir.native.namespace_mut("f3d"))?;
                let annotations =
                    populate_annotations(ctx, &self.ir, scan, &self.native, None, &self.unknowns)?;
                let source_image = preserve_source_image(ctx, scan)?;
                let mut admitted_entities = self.admitted_entities;
                return decode_result(
                    ctx,
                    scan,
                    self.report_scope,
                    self.ir,
                    self.report,
                    RetainedArtifacts {
                        annotations,
                        unknowns: self.unknowns,
                        source_image,
                        source_attributes: self.source_attributes,
                    },
                    &mut admitted_entities,
                );
            }
        };

        report_design_projection_gaps(ctx, &mut self.report, &self.ir, &self.native)?;
        ctx.admit_entities(
            u64_from_index(self.ir.model.entity_count()),
            &mut self.admitted_entities,
            "admit F3D entities",
        )?;
        self.native
            .store(ctx, self.ir.native.namespace_mut("f3d"))?;
        let annotations = populate_annotations(
            ctx,
            &self.ir,
            scan,
            &self.native,
            Some((
                &geometry.primary_model_brep_name,
                &geometry.annotation_records,
            )),
            &self.unknowns,
        )?;
        let source_image = preserve_source_image(ctx, scan)?;
        let mut admitted_entities = self.admitted_entities;
        decode_result(
            ctx,
            scan,
            self.report_scope,
            self.ir,
            self.report,
            RetainedArtifacts {
                annotations,
                unknowns: self.unknowns,
                source_image,
                source_attributes: self.source_attributes,
            },
            &mut admitted_entities,
        )
    }
}

fn brep_identity_namespace<'a>(
    ctx: &DecodeContext<'_>,
    entry: &'a str,
) -> Result<Option<&'a str>, CodecError> {
    let base = entry_basename(ctx, entry)?;
    ctx.strip_prefix(base, "BREP.", "strip F3D BREP identity namespace prefix")
}

/// Decode an F3D or F3Z reader.
pub(crate) fn decode<'a>(ctx: &DecodeContext<'a>, root: View<'a>) -> Result<Decoded, CodecError> {
    let scan = container::scan(ctx, root)?;
    match &scan.kind {
        container::F3dContainerKind::MultiDocument { .. } => crate::f3z::decode(ctx, &scan),
        container::F3dContainerKind::Document { .. } => {
            decode_scanned_document(ctx, &scan, crate::report::ReportScope::Standalone)
                .map(AuthoredDecoded::into_decoded)
        }
    }
}

/// Decode one already-scanned F3Z member under archive-owned identity.
pub(crate) fn decode_archive_member<'a>(
    ctx: &DecodeContext<'a>,
    scan: &'a ContainerScan<'a>,
    dialects: &cadmpeg_core::dialect::DialectLayers,
) -> Result<AuthoredDecoded, CodecError> {
    decode_scanned_document(
        ctx,
        scan,
        crate::report::ReportScope::ArchiveMember(
            dialects.try_clone_for_decode(ctx, "copy dialect layers")?,
        ),
    )
}

fn decode_scanned_document<'a>(
    ctx: &DecodeContext<'a>,
    scan: &'a ContainerScan<'a>,
    report_scope: crate::report::ReportScope,
) -> Result<AuthoredDecoded, CodecError> {
    let mut admitted_entities = 0_u64;
    ctx.charge_entities(
        u64_from_index(scan.entries.len()),
        "admit F3D archive entries",
    )?;

    if ctx.container_only() {
        let MetadataIr {
            ir,
            mut source_attributes,
            unknowns,
        } = build_metadata_ir(ctx, scan)?;
        annotate_docstruct(ctx, &mut source_attributes, scan)?;
        let annotations =
            populate_annotations(ctx, &ir, scan, &F3dNative::default(), None, &unknowns)?;
        let source_image = preserve_source_image(ctx, scan)?;
        let mut report = crate::report::build_decode_report(
            ctx,
            scan,
            cadmpeg_ir::report::decode::DecodeTransfer::ContainerOnly {},
            container_losses(ctx, scan)?,
        )?;
        match crate::xref::decode(ctx, scan) {
            Ok(Some(table)) => apply_assembly_classification(ctx, &mut report, scan, &table)?,
            Ok(None) => {}
            Err(error @ (CodecError::ResourceLimit(_) | CodecError::NotImplemented(_))) => {
                return Err(error)
            }
            Err(error) => report_xref_parse_loss(ctx, &mut report, &error)?,
        }
        return decode_result(
            ctx,
            scan,
            report_scope,
            ir,
            report,
            RetainedArtifacts {
                annotations,
                unknowns,
                source_image,
                source_attributes,
            },
            &mut admitted_entities,
        );
    }

    let (model_blob_names, _model_blob_names_storage) =
        crate::design::decode::body::design_model_blob_names(ctx, scan)?;
    let unbound_body_bindings =
        crate::design::decode::body::decode_design_body_bindings(ctx, scan, None, &[])?;
    let model_breps = model_brep_candidates(ctx, scan, &model_blob_names)?;

    // Every Design body-map pair names its owning BREP blob. Decode the
    // complete referenced set; a document-level model is not confined to one
    // arbitrary `.smbh` entry.
    if !model_breps.is_empty() {
        let mut primary_model_brep = None;
        let qualify_ids = model_breps.len() > 1;
        let mut brep = Brep::default();
        let mut body_visibilities = Vec::new();
        let mut decoded_brep_count = 0usize;
        let (all_body_visibility, _all_body_visibility_storage) =
            crate::design::decode::body::decode_all_body_visibility(ctx, scan)?;
        let mut selected_body_storage = ctx.reserve_scoped(0, "index F3D selected body blobs")?;
        let mut selected_body_keys =
            std::collections::HashMap::<String, std::collections::BTreeSet<u64>>::new();
        for binding in ctx.admit_iter(&unbound_body_bindings, "scan F3D unbound body bindings")? {
            selected_body_storage.with_storage(|| {
                index_selected_body_key(
                    ctx,
                    &mut selected_body_keys,
                    binding.blob_name(),
                    binding.asm_body_key,
                )
            })?;
        }
        for &candidate in ctx.admit_iter(&model_breps, "scan F3D model BREP candidates")? {
            let Some(mut part) = try_decode_brep(ctx, scan, candidate)? else {
                continue;
            };
            let blob_name = entry_basename(ctx, &candidate.name)?;
            if let Some(keys) = ctx.get_hash_map(
                &selected_body_keys,
                blob_name,
                "find F3D selected body keys",
            )? {
                part.retain_body_keys(ctx, keys)?;
            }
            if part.asm.surfaces.is_empty()
                && part.asm.points.is_empty()
                && part.asm.faces.is_empty()
            {
                continue;
            }
            primary_model_brep.get_or_insert(candidate);
            let mut body_selectors = match ctx.get_hash_map(
                &selected_body_keys,
                blob_name,
                "find F3D selected body keys",
            )? {
                Some(keys) => part.body_selectors_for(ctx, keys)?,
                None => part.body_selectors(ctx)?,
            };
            for body in &mut part.asm.bodies {
                if let Some(visibility) = ctx
                    .get_btree_map(&body_selectors, &body.id, "find F3D body selector")?
                    .map(|selector| {
                        body_visibility_for(ctx, &all_body_visibility, blob_name, *selector)
                    })
                    .transpose()?
                    .flatten()
                {
                    body.visible = Some(visibility.visible);
                }
            }
            if qualify_ids {
                let namespace =
                    brep_identity_namespace(ctx, &candidate.name)?.ok_or_else(|| {
                        CodecError::malformed(format_args!(
                            "BREP entry has no stable blob identity: {}",
                            candidate.name
                        ))
                    })?;
                part.qualify_ids(ctx, crate::ids::ID_FORMAT, namespace)?;
                body_selectors = match ctx.get_hash_map(
                    &selected_body_keys,
                    blob_name,
                    "find F3D selected body keys",
                )? {
                    Some(keys) => part.body_selectors_for(ctx, keys)?,
                    None => part.body_selectors(ctx)?,
                };
            }
            for body in ctx.admit_iter(&part.asm.bodies, "scan F3D part asm bodies")? {
                if let Some((body_selector, visibility)) = ctx
                    .get_btree_map(&body_selectors, &body.id, "find F3D body selector")?
                    .map(|selector| {
                        body_visibility_for(ctx, &all_body_visibility, blob_name, *selector)
                            .map(|visibility| visibility.map(|visibility| (*selector, visibility)))
                    })
                    .transpose()?
                    .flatten()
                {
                    let id = crate::ids::native_scoped_id(
                        ctx,
                        &candidate.name,
                        "body-visibility",
                        body_selector,
                    )?;
                    let visibility = crate::records::bodies::BodyVisibility::try_new_charged(
                        ctx,
                        crate::records::bodies::BodyVisibilityWire {
                            id,
                            body: body
                                .id
                                .try_clone_for_decode(ctx, "retain F3D visible body ID")?,
                            stream: ctx.validate_nonblank_text(
                                ctx.copy_retained_text(
                                    &visibility.stream,
                                    "retain F3D body visibility stream",
                                )?,
                                "validate stream",
                            )?,
                            byte_offset: visibility.byte_offset,
                            asm_body_key_offset: visibility.asm_body_key_offset,
                            asm_body_key: body_selector,
                            entity_suffix: visibility.entity_suffix,
                            visible: visibility.visible,
                        },
                    )?
                    .map_err(CodecError::Malformed)?;
                    ctx.push_vec(
                        &mut body_visibilities,
                        visibility,
                        "collect F3D body visibilities",
                    )?;
                }
            }
            brep.append(ctx, part)?;
            decoded_brep_count += 1;
        }
        if let Some(primary_model_brep) = primary_model_brep {
            return finish_model_decode(
                ctx,
                scan,
                primary_model_brep,
                brep,
                body_visibilities,
                model_breps.len() - decoded_brep_count,
                DecodeSessionState {
                    admitted_entities,
                    report_scope,
                },
            );
        }
    }

    // No binary stream decoded: the model may be carried only in the text
    // encoding.
    if let Some((text_facts, text_brep)) = try_decode_text_model(ctx, scan)? {
        return finish_model_decode(
            ctx,
            scan,
            &text_facts,
            text_brep,
            Vec::new(),
            0,
            DecodeSessionState {
                admitted_entities,
                report_scope,
            },
        );
    }

    // No decodable SAB stream: use container metadata through the shared session.
    let (session, path) = F3dDecodeSession::from_metadata(
        ctx,
        scan,
        DecodeSessionState {
            admitted_entities,
            report_scope,
        },
    )?;
    session.into_result(path)
}

/// Projected mesh geometry and the Design records that own it.
struct MeshProjection {
    /// Number of joined mesh-body containers.
    count: usize,
    /// Tessellation identities grouped by their Design stream and feature scope record.
    tessellations_by_scope: std::collections::HashMap<(String, u32), Vec<String>>,
}

fn extend_unique_assets(
    ctx: &DecodeContext<'_>,
    assets: &mut Vec<cadmpeg_ir::assets::Asset>,
    incoming: Vec<cadmpeg_ir::assets::Asset>,
) -> Result<(), CodecError> {
    for asset in incoming {
        let existing = ctx
            .position_by(
                assets.as_slice(),
                |existing| {
                    ctx.equal(
                        &existing.id,
                        &asset.id,
                        "compare F3D embedded asset identities",
                    )
                },
                "find F3D embedded asset identity",
            )?
            .map(|index| &assets[index]);
        match existing {
            Some(existing) if existing != &asset => {
                return Err(CodecError::malformed(format_args!(
                    "F3D embedded asset {} has conflicting projections",
                    asset.id.as_str()
                )));
            }
            Some(_) => {}
            None => ctx.push_vec(assets, asset, "append F3D unique assets")?,
        }
    }
    Ok(())
}

fn collect_mesh_outcome(
    ctx: &DecodeContext<'_>,
    bodies: &mut Vec<crate::design::decode::mesh::MeshBody>,
    report: &mut DecodeBody,
    outcome: crate::design::decode::mesh::MeshContainerOutcome,
) -> Result<(), CodecError> {
    use crate::design::decode::mesh::MeshContainerOutcome;
    match outcome {
        MeshContainerOutcome::Joined(body) => {
            ctx.push_vec(bodies, body, "collect F3D joined mesh bodies")?;
        }
        MeshContainerOutcome::Unjoined { entry_name } => push_loss_vec(ctx, &mut report.losses, F3dLossCode::MeshContainerUnjoined, format_args!(
                "mesh geometry container `{entry_name}` decoded but has no complete Design body join"
            ), "collect F3D unjoined mesh loss", "retain F3D unjoined mesh loss")?,
        MeshContainerOutcome::Failed {
            error: error @ CodecError::ResourceLimit(_),
            ..
        } => return Err(error),
        MeshContainerOutcome::Failed { entry_name, error } => push_loss_vec(ctx, &mut report.losses, F3dLossCode::MeshContainerUndecoded, format_args!("mesh geometry container `{entry_name}` was not decoded: {error}"), "collect F3D undecoded mesh loss", "retain F3D undecoded mesh loss")?,
        MeshContainerOutcome::Missing { entry_name } => push_loss_vec(ctx, &mut report.losses, F3dLossCode::MeshContainerMissing, format_args!(
                "Design mesh body names `{entry_name}`, but no unique geometry container joined it"
            ), "collect F3D missing mesh loss", "retain F3D missing mesh loss")?,
    }
    Ok(())
}

fn mesh_texture_asset_bytes(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan,
    entry_name: &str,
) -> Result<Vec<u8>, CodecError> {
    ctx.copy_retained(
        scan.entry_bytes(ctx, entry_name)?,
        "retain F3D mesh texture bytes",
    )
}

fn clone_mesh_texture_table(
    ctx: &DecodeContext<'_>,
    table: &[(String, cadmpeg_ir::assets::AssetId)],
) -> Result<Vec<(String, cadmpeg_ir::assets::AssetId)>, CodecError> {
    let mut copy = Vec::new();
    for (source_id, asset) in ctx.admit_iter(table, "scan F3D mesh texture table")? {
        let source_id =
            ctx.copy_retained_text(source_id, "copy F3D mesh texture table source ID")?;
        let asset = asset.try_clone_for_decode(ctx, "copy F3D mesh texture table asset ID")?;
        ctx.push_vec(&mut copy, (source_id, asset), "copy F3D mesh texture table")?;
    }
    Ok(copy)
}

fn insert_mesh_texture_table(
    ctx: &DecodeContext<'_>,
    tables: &mut std::collections::HashMap<String, Vec<(String, cadmpeg_ir::assets::AssetId)>>,
    tessellation_id: &str,
    table: &[(String, cadmpeg_ir::assets::AssetId)],
) -> Result<(), CodecError> {
    if ctx.contains_key_hash_map(tables, tessellation_id, "find F3D mesh texture table")? {
        return Err(CodecError::Malformed(
            "F3D mesh tessellation belongs to more than one texture table".into(),
        ));
    }

    let key = ctx.copy_retained_text(tessellation_id, "retain F3D mesh texture table key")?;
    let table = clone_mesh_texture_table(ctx, table)?;
    ctx.insert_hash_map(tables, key, table, "index F3D mesh texture tables")?;
    Ok(())
}

fn insert_mesh_scope_tessellations<'a, I: Iterator>(
    ctx: &DecodeContext<'_>,
    index: &mut std::collections::HashMap<(String, u32), Vec<String>>,
    stream: &str,
    record_index: u32,
    source: cadmpeg_core::decode::scan::AdmittedIter<I>,
    mut tessellation_id: impl FnMut(I::Item) -> Option<&'a str>,
) -> Result<(), CodecError> {
    let mut tessellations = Vec::new();
    for body in source {
        let Some(id) = tessellation_id(body) else {
            continue;
        };
        let copy = ctx.copy_retained_text(id, "retain F3D mesh scope tessellation ID")?;
        ctx.push_vec(
            &mut tessellations,
            copy,
            "collect F3D mesh scope tessellations",
        )?;
    }
    if tessellations.is_empty() {
        return Ok(());
    }
    let stream = ctx.copy_retained_text(stream, "retain F3D mesh scope stream")?;
    let key = (stream, record_index);
    if ctx.contains_key_hash_map(index, &key, "find F3D mesh feature scope")? {
        return Err(CodecError::Malformed(
            "F3D Design mesh feature scope is not unique".into(),
        ));
    }

    ctx.insert_hash_map(index, key, tessellations, "index F3D mesh feature scopes")?;
    Ok(())
}

fn corner_shaded_mesh(
    ctx: &DecodeContext<'_>,
    body_id: &str,
    vertices: Vec<cadmpeg_ir::features::FinitePoint3>,
    triangles: Vec<[u32; 3]>,
    corner_normals: Option<Vec<cadmpeg_ir::features::FiniteVector3>>,
) -> Result<
    cadmpeg_ir::tessellation::TessellationMesh<
        cadmpeg_ir::features::FinitePoint3,
        cadmpeg_ir::features::FiniteVector3,
    >,
    CodecError,
> {
    let Some(corner_normals) = corner_normals else {
        return Ok(cadmpeg_ir::tessellation::TessellationMesh::List {
            vertices,
            triangles,
        });
    };
    let triangle_count = triangles.len();
    let normal_count = corner_normals.len();
    let Some(corner_count) = triangle_count.checked_mul(3) else {
        let error = cadmpeg_ir::tessellation::TessellationLaneError::CornerCountOverflow {
            triangles: triangle_count,
        };
        return Err(CodecError::malformed(format_args!(
            "paramesh body record {body_id}: {error}"
        )));
    };
    if normal_count != corner_count {
        let error = cadmpeg_ir::tessellation::TessellationLaneError::CornerNormalLane {
            corners: corner_count,
            normals: normal_count,
        };
        return Err(CodecError::malformed(format_args!(
            "paramesh body record {body_id}: {error}"
        )));
    }
    let mut normals = ctx.admit_iter(&corner_normals, "scan F3D corner-shaded normal values")?;
    let mut shaded_triangles =
        ctx.collection_vec(triangle_count, "collect F3D corner-shaded triangles")?;
    for corners in ctx.admit_iter(&triangles, "scan F3D corner-shaded triangle rows")? {
        let (Some(first), Some(second), Some(third)) =
            (normals.next(), normals.next(), normals.next())
        else {
            let error = cadmpeg_ir::tessellation::TessellationLaneError::CornerNormalLane {
                corners: corner_count,
                normals: normal_count,
            };
            return Err(CodecError::malformed(format_args!(
                "paramesh body record {body_id}: {error}"
            )));
        };
        shaded_triangles.push(cadmpeg_ir::tessellation::ShadedTriangle {
            corners: *corners,
            normals: [*first, *second, *third],
        });
    }
    Ok(
        cadmpeg_ir::tessellation::TessellationMesh::CornerShadedList {
            vertices,
            triangles: shaded_triangles,
        },
    )
}

/// Project each mesh body's container geometry into the tessellation arena.
///
/// A mesh body carries no B-rep topology: its geometry is a triangle list, and
/// the neutral tessellation arena is where a standalone triangle list belongs.
/// Returns the number of bodies projected and reports every mesh-geometry
/// container that no body claimed.
fn project_mesh_bodies(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan,
    ir: &mut CadIr,
    native: &mut F3dNative,
    report: &mut DecodeBody,
) -> Result<MeshProjection, CodecError> {
    let decoded = crate::design::decode::mesh::decode_mesh_bodies(ctx, scan)?;
    native.design_mesh_features = decoded.features;
    let mut texture_assets = Vec::new();
    for feature in ctx.admit_iter(
        &native.design_mesh_features,
        "scan F3D mesh texture features",
    )? {
        for texture in ctx.admit_iter(
            feature.texture_table.resources(),
            "scan F3D mesh texture resources",
        )? {
            let mut has_asset = |asset: &cadmpeg_ir::assets::Asset| {
                ctx.equal(
                    &asset.id,
                    &texture.asset,
                    "compare F3D mesh texture asset IDs",
                )
            };
            if ctx.any_by(
                &ir.model.assets,
                &mut has_asset,
                "find F3D existing mesh texture asset",
            )? || ctx.any_by(
                &texture_assets,
                &mut has_asset,
                "find F3D staged mesh texture asset",
            )? {
                continue;
            }
            let media_type = match std::path::Path::new(texture.file.filename()).extension() {
                Some(extension) => {
                    match ctx
                        .validate_utf8(
                            extension.as_encoded_bytes(),
                            "validate F3D mesh texture extension",
                        )?
                        .ok()
                    {
                        Some(extension)
                            if ctx.eq_ignore_ascii_case(
                                extension,
                                "jpg",
                                "match F3D JPEG texture extension",
                            )? || ctx.eq_ignore_ascii_case(
                                extension,
                                "jpeg",
                                "match F3D JPEG texture extension",
                            )? =>
                        {
                            Some("image/jpeg")
                        }
                        Some(extension)
                            if ctx.eq_ignore_ascii_case(
                                extension,
                                "png",
                                "match F3D PNG texture extension",
                            )? =>
                        {
                            Some("image/png")
                        }
                        _ => None,
                    }
                }
                None => None,
            }
            .map(str::to_owned);
            let asset = cadmpeg_ir::assets::Asset::try_new(
                ctx,
                texture
                    .asset
                    .try_clone_for_decode(ctx, "retain F3D mesh texture asset ID")?,
                Some(ctx.copy_retained_text(
                    texture.file.filename(),
                    "retain F3D mesh texture filename",
                )?),
                media_type,
                cadmpeg_ir::assets::AssetContent::Embedded {
                    data: cadmpeg_ir::assets::AssetData::new(mesh_texture_asset_bytes(
                        ctx,
                        scan,
                        texture.file.archive_entry_name(),
                    )?)
                    .ok_or_else(|| CodecError::Malformed("asset data must not be empty".into()))?,
                },
                Some(crate::ids::native_scope(
                    ctx,
                    texture.file.archive_entry_name(),
                    "retain F3D native scope",
                )?),
            )?;
            ctx.push_vec(
                &mut texture_assets,
                asset,
                "collect F3D mesh texture assets",
            )?;
        }
    }
    extend_unique_assets(ctx, &mut ir.model.assets, texture_assets)?;
    let mut texture_tables_storage = ctx.reserve_scoped(0, "index F3D mesh texture tables")?;
    let mut texture_tables = std::collections::HashMap::new();
    texture_tables_storage.with_storage(|| {
        for feature in ctx.admit_iter(
            &native.design_mesh_features,
            "scan F3D native design mesh features",
        )? {
            let mut texture_table_storage =
                ctx.reserve_scoped(0, "collect F3D mesh texture resource table")?;
            let mut texture_table = Vec::new();
            texture_table_storage.with_storage(|| {
                let ordered_resources = feature.texture_table.resources_in_flags_order(ctx)?;
                for texture in ctx.admit_iter(
                    &ordered_resources,
                    "scan F3D ordered mesh texture resources",
                )? {
                    let source_id = ctx.copy_retained_text(
                        texture.resource_guid.as_str(),
                        "retain F3D mesh texture resource GUID",
                    )?;
                    let asset = texture
                        .asset
                        .try_clone_for_decode(ctx, "retain F3D mesh texture table asset ID")?;
                    ctx.push_vec(
                        &mut texture_table,
                        (source_id, asset),
                        "collect F3D mesh texture resource table",
                    )?;
                }
                Ok::<(), CodecError>(())
            })?;
            for body in ctx.admit_iter(feature.bodies(), "scan F3D mesh texture body bindings")? {
                if let Some(tessellation_id) = &body.tessellation_id {
                    insert_mesh_texture_table(
                        ctx,
                        &mut texture_tables,
                        tessellation_id,
                        &texture_table,
                    )?;
                }
            }
        }
        Ok::<(), CodecError>(())
    })?;
    let mut bodies = Vec::new();
    for outcome in decoded.outcomes {
        collect_mesh_outcome(ctx, &mut bodies, report, outcome)?;
    }
    let mut unresolved = std::collections::BTreeMap::new();
    let mut projection = MeshProjection {
        count: bodies.len(),
        tessellations_by_scope: std::collections::HashMap::new(),
    };
    for mut body in bodies {
        let texture_table = ctx
            .remove_hash_map(
                &mut texture_tables,
                &body.id,
                "remove F3D mesh body texture table",
            )?
            .ok_or_else(|| {
                CodecError::Malformed("F3D joined mesh body has no owning texture table".into())
            })?;
        let texture_assignments = mesh_texture_assignments(
            ctx,
            body.texture_ids.as_deref(),
            &texture_table,
            body.triangles.len(),
        )?;
        let triangle_groups = ctx.collect_vec(
            std::mem::take(&mut body.triangle_groups)
                .into_iter()
                .map(
                    |group| cadmpeg_ir::tessellation::TessellationTriangleGroup {
                        source_id: Some(group.source_id),
                        triangles: group.triangles,
                    },
                ),
            "collect F3D mesh triangle groups",
        )?;
        let channels = mesh_attribute_channels(
            ctx,
            &body.attributes,
            body.vertices.len(),
            &body.triangles,
            &mut unresolved,
        )?;
        // The paramesh registry states an unshaded mesh by carrying no
        // corner-normal channel, so the lane arrives absent, never empty.
        let corner_normals = match body.corner_normals.take() {
            Some(normals) => Some(
                ctx.collect_vec(
                    normals
                        .into_iter()
                        .map(cadmpeg_ir::features::FiniteVector3::from),
                    "collect F3D mesh corner normals",
                )?,
            ),
            None => None,
        };
        let mesh =
            corner_shaded_mesh(ctx, &body.id, body.vertices, body.triangles, corner_normals)?;
        let id = match cadmpeg_ir::tessellation::TessellationId::mint(body.id) {
            Ok(id) => id,
            Err(error) => {
                return Err(CodecError::Malformed(ctx.format_retained(
                    format_args!("{error}"),
                    "retain F3D tessellation identity error",
                )?))
            }
        };
        let tessellation =
            match cadmpeg_ir::tessellation::Tessellation::from_parts(id, mesh, channels) {
                Ok(tessellation) => tessellation,
                Err(error) => {
                    return Err(CodecError::Malformed(ctx.format_retained(
                        format_args!("{error}"),
                        "retain F3D tessellation construction error",
                    )?))
                }
            };
        let tessellation = match tessellation
            .with_feature_edges(body.feature_edges)
            .and_then(|mesh| mesh.with_triangle_groups(triangle_groups))
            .and_then(|mesh| mesh.with_texture_assignments(texture_assignments))
        {
            Ok(tessellation) => tessellation,
            Err(error) => {
                return Err(CodecError::Malformed(ctx.format_retained(
                    format_args!("{error}"),
                    "retain F3D tessellation channel error",
                )?))
            }
        };
        ctx.push_vec(
            &mut ir.model.tessellations,
            tessellation,
            "collect F3D mesh tessellations",
        )?;
    }
    if !texture_tables.is_empty() {
        return Err(CodecError::Malformed(
            "F3D mesh texture table has no joined tessellation body".into(),
        ));
    }
    for feature in ctx.admit_iter(
        &native.design_mesh_features,
        "scan F3D native design mesh features",
    )? {
        let stream = crate::ids::native_stream(&feature.id).unwrap_or(crate::ids::DEFAULT_STREAM);
        insert_mesh_scope_tessellations(
            ctx,
            &mut projection.tessellations_by_scope,
            stream,
            feature.scope().record().record_index(),
            ctx.admit_iter(feature.bodies(), "scan F3D mesh scope body bindings")?,
            |body| body.tessellation_id.as_deref(),
        )?;
    }
    report_unresolved_mesh_attributes(ctx, report, &unresolved)?;
    Ok(projection)
}

/// Resolve one-based `tid` values through a Design texture table. Zero leaves
/// the triangle untextured.
fn mesh_texture_assignments(
    ctx: &DecodeContext<'_>,
    texture_ids: Option<&[u32]>,
    textures: &[(String, cadmpeg_ir::assets::AssetId)],
    triangle_count: usize,
) -> Result<Vec<cadmpeg_ir::tessellation::TessellationTextureAssignment>, CodecError> {
    let Some(texture_ids) = texture_ids else {
        return Ok(Vec::new());
    };
    if texture_ids.len() != triangle_count {
        return Err(CodecError::Malformed(
            "F3D mesh texture-id count differs from the triangle count".into(),
        ));
    }
    let mut triangles =
        ctx.collect_indexed_vec(textures.len(), "f3d mesh texture assignments", |_| {
            Ok(Vec::new())
        })?;
    for (triangle, texture_id) in texture_ids.iter().enumerate() {
        ctx.charge_work(1, "resolve F3D mesh texture triangle")?;
        if *texture_id == 0 {
            continue;
        }
        let index = texture_id
            .checked_sub(1)
            .and_then(|index| usize::try_from(index).ok())
            .filter(|index| *index < textures.len())
            .ok_or_else(|| {
                CodecError::Malformed(
                    "F3D mesh triangle texture id names no Design texture resource".into(),
                )
            })?;

        ctx.reserve_vec(
            &mut triangles[index],
            1,
            "collect F3D mesh texture triangles",
        )?;
        triangles[index].push(u32::try_from(triangle).map_err(|_| {
            CodecError::Malformed("F3D mesh triangle ordinal is out of range".into())
        })?);
    }
    let mut assignments = Vec::new();
    for ((source_id, texture), triangles) in ctx
        .admit_iter(textures, "scan F3D mesh texture assignment rows")?
        .zip(triangles)
    {
        if triangles.is_empty() {
            continue;
        }

        ctx.reserve_vec(&mut assignments, 1, "collect F3D mesh texture assignments")?;
        let source_id = ctx.copy_retained_text(source_id, "copy F3D mesh texture source ID")?;
        let texture = texture.try_clone_for_decode(ctx, "copy F3D mesh texture asset ID")?;
        assignments.push(cadmpeg_ir::tessellation::TessellationTextureAssignment {
            source_id: Some(source_id),
            texture,
            triangles,
        });
    }
    Ok(assignments)
}

/// Replace the native definition of each mesh-import scope with its exact
/// tessellation identities.
fn mesh_feature_tessellations<'a>(
    ctx: &DecodeContext<'_>,
    projection: &'a MeshProjection,
    stream: &str,
    record_index: u32,
) -> Result<Option<&'a [String]>, CodecError> {
    let operation = "look up F3D mesh feature tessellations";
    let (key, _reservation) = ctx.format_scoped(format_args!("{stream}"), operation)?;
    Ok(ctx
        .get_hash_map(
            &projection.tessellations_by_scope,
            &(key, record_index),
            operation,
        )?
        .map(Vec::as_slice))
}

fn bind_mesh_feature_definitions(
    ctx: &DecodeContext<'_>,
    features: &mut [cadmpeg_ir::features::Feature],
    scopes: &[crate::records::feature::scope::DesignParameterScope],
    projection: &MeshProjection,
) -> Result<(), cadmpeg_core::CodecError> {
    for feature in features {
        if feature.source_tag.as_deref() != Some("Base Mesh Feature") {
            continue;
        }
        let Some(native_ref) = feature.native_ref.as_deref() else {
            continue;
        };
        let Some(scope_index) = ctx.position_by(
            scopes,
            |scope| {
                ctx.equal(
                    scope.id.as_str(),
                    native_ref,
                    "compare F3D mesh feature scope identities",
                )
            },
            "find F3D mesh feature scope",
        )?
        else {
            continue;
        };
        let scope = &scopes[scope_index];
        let stream = crate::ids::native_stream(&scope.id).unwrap_or(crate::ids::DEFAULT_STREAM);
        let Some(tessellations) =
            mesh_feature_tessellations(ctx, projection, stream, scope.record_index)?
        else {
            continue;
        };
        if tessellations.is_empty() {
            continue;
        }
        let mut copies = Vec::new();
        for tessellation in ctx.admit_iter(tessellations, "scan F3D mesh feature tessellations")? {
            let copy =
                ctx.copy_retained_text(tessellation, "retain F3D mesh feature tessellation ID")?;
            ctx.push_vec(&mut copies, copy, "collect F3D mesh feature tessellations")?;
        }
        feature
            .evaluation
            .set_definition(cadmpeg_ir::features::FeatureDefinition::Operation(
                cadmpeg_ir::features::FeatureOperation::MeshImport {
                    tessellations: copies
                        .try_into()
                        .map_err(cadmpeg_core::CodecError::malformed)?,
                },
            ));
    }

    Ok(())
}

/// Project channels with a settled element layout and count unresolved channels
/// by domain.
///
/// An indexed channel stores one default value per vertex followed by values
/// selected at the explicit corner positions in its index stream. The IR keeps
/// the value table and expands those selections into one selector per triangle
/// corner.
fn mesh_attribute_channels(
    ctx: &DecodeContext<'_>,
    attributes: &[crate::paramesh::MeshAttribute],
    vertices: usize,
    triangles: &[[u32; 3]],
    unresolved: &mut std::collections::BTreeMap<crate::paramesh::MeshAttributeDomain, usize>,
) -> Result<Vec<cadmpeg_ir::tessellation::TessellationChannel>, CodecError> {
    use crate::paramesh::MeshAttributeDomain;

    let mut channels = Vec::new();
    for attribute in ctx.admit_iter(attributes, "scan F3D attributes")? {
        match (
            attribute.addressing.domain(),
            attribute.item_size(),
            attribute.count(),
        ) {
            (MeshAttributeDomain::Vertex, Some(item_size), Some(_)) => {
                match cadmpeg_ir::tessellation::TessellationChannel::new(
                    cadmpeg_ir::tessellation::ChannelAddressing::Vertex {},
                    item_size,
                    attribute.role,
                    attribute.element_code(),
                    ctx.copy_retained(attribute.values(), "copy F3D mesh channel values")?,
                ) {
                    Ok(channel) => {
                        ctx.reserve_vec(&mut channels, 1, "collect F3D mesh channels")?;
                        channels.push(channel);
                    }
                    Err(_) => *unresolved.entry(MeshAttributeDomain::Vertex).or_default() += 1,
                }
            }
            (MeshAttributeDomain::Corner, Some(item_size), Some(_)) => {
                let Some(selectors) = attribute.corner_selectors(ctx, vertices, triangles)? else {
                    *unresolved.entry(MeshAttributeDomain::Corner).or_default() += 1;
                    continue;
                };
                match cadmpeg_ir::tessellation::TessellationChannel::new(
                    cadmpeg_ir::tessellation::ChannelAddressing::Corner { indices: selectors },
                    item_size,
                    attribute.role,
                    attribute.element_code(),
                    ctx.copy_retained(attribute.values(), "copy F3D mesh channel values")?,
                ) {
                    Ok(channel) => {
                        ctx.reserve_vec(&mut channels, 1, "collect F3D mesh channels")?;
                        channels.push(channel);
                    }
                    Err(_) => *unresolved.entry(MeshAttributeDomain::Corner).or_default() += 1,
                }
            }
            (MeshAttributeDomain::Triangle, Some(item_size), Some(count))
                if usize::try_from(count) == Ok(triangles.len()) && item_size == 4 =>
            {
                if u32::try_from(triangles.len()).is_err() {
                    *unresolved.entry(MeshAttributeDomain::Triangle).or_default() += 1;
                    continue;
                }

                let mut indices = Vec::new();
                ctx.reserve_vec(
                    &mut indices,
                    triangles.len(),
                    "collect F3D mesh triangle selectors",
                )?;
                for index in ctx.admit_iter(
                    &(0..triangles.len()),
                    "scan F3D mesh triangle attribute selectors",
                )? {
                    indices.push(u32::try_from(index).map_err(|_| {
                        CodecError::Malformed("F3D mesh triangle selector exceeds u32".into())
                    })?);
                }
                match cadmpeg_ir::tessellation::TessellationChannel::new(
                    cadmpeg_ir::tessellation::ChannelAddressing::Triangle { indices },
                    item_size,
                    attribute.role,
                    attribute.element_code(),
                    ctx.copy_retained(attribute.values(), "copy F3D mesh channel values")?,
                ) {
                    Ok(channel) => {
                        ctx.reserve_vec(&mut channels, 1, "collect F3D mesh channels")?;
                        channels.push(channel);
                    }
                    Err(_) => *unresolved.entry(MeshAttributeDomain::Triangle).or_default() += 1,
                }
            }
            (domain, _, _) => *unresolved.entry(domain).or_default() += 1,
        }
    }
    Ok(channels)
}

/// Report mesh attribute channels that the projector left unresolved, grouped by
/// domain.
fn report_unresolved_mesh_attributes(
    ctx: &DecodeContext<'_>,
    report: &mut DecodeBody,
    unresolved: &std::collections::BTreeMap<crate::paramesh::MeshAttributeDomain, usize>,
) -> Result<(), CodecError> {
    use crate::paramesh::MeshAttributeDomain;

    for (domain, count) in ctx.admit_iter(unresolved, "scan F3D unresolved mesh attributes")? {
        let (addressing, reason) = match domain {
            MeshAttributeDomain::Corner => (
                "triangle corners",
                "their indexed value table or corner selector stream has no settled layout",
            ),
            MeshAttributeDomain::Triangle => (
                "triangles",
                "their element code or value count has no settled layout",
            ),
            MeshAttributeDomain::Vertex => (
                "vertices",
                "their element code does not settle a stored element layout of one value per \
                 vertex",
            ),
        };
        push_loss_vec(
            ctx,
            &mut report.losses,
            F3dLossCode::MeshAttributeNotTransferred,
            format_args!(
                "{count} mesh attribute channel(s) addressing {addressing} were not transferred: \
             {reason}."
            ),
            "collect F3D unresolved mesh attribute loss",
            "retain F3D unresolved mesh attribute loss",
        )?;
    }
    Ok(())
}

/// Record the `Properties.dat` docstruct declaration on the source metadata.
fn annotate_docstruct(
    ctx: &DecodeContext<'_>,
    attributes: &mut std::collections::BTreeMap<String, String>,
    scan: &ContainerScan,
) -> Result<(), CodecError> {
    let Some(docstruct) = crate::xref::docstruct(ctx, scan)? else {
        return Ok(());
    };
    ctx.insert_btree_map(
        attributes,
        "docstruct_type".into(),
        docstruct.doc_type,
        "record F3D docstruct type",
    )?;
    if let Some(subtype) = docstruct.subtype {
        ctx.insert_btree_map(
            attributes,
            "docstruct_subtype".into(),
            subtype,
            "record F3D docstruct subtype",
        )?;
    }
    Ok(())
}

/// A warning for a present but unparseable `RedirectionsStream.dat`.
fn report_xref_parse_loss(
    ctx: &DecodeContext<'_>,
    report: &mut DecodeBody,
    error: &CodecError,
) -> Result<(), CodecError> {
    push_loss_vec(
        ctx,
        &mut report.losses,
        F3dLossCode::XrefTableUndecoded,
        format_args!("external-reference table was not decoded: {error}"),
        "collect F3D xref parse losses",
        "retain F3D xref parse loss",
    )
}

/// Report typed occurrence placements whose role path was readable but whose
/// generation-specific payload did not close and had no valid carrier.
fn report_xref_placement_failures(
    ctx: &DecodeContext<'_>,
    report: &mut DecodeBody,
    table: &crate::xref::XrefTable,
) -> Result<(), CodecError> {
    for ordinal in ctx.admit_iter(
        &table.placement_failures,
        "scan F3D table placement failures",
    )? {
        let Some(reference_index) = ctx.position_by(
            &table.references,
            |reference| Ok(reference.ordinal == *ordinal),
            "find F3D failed placement reference",
        )?
        else {
            continue;
        };
        let reference = &table.references[reference_index];
        push_loss_vec(
            ctx,
            &mut report.losses,
            F3dLossCode::XrefPlacementUndecoded,
            format_args!(
                "external occurrence {} for role {} has a typed placement record that did not \
                 decode under its generation grammar; no valid placement carrier was available",
                reference.relative_path, reference.neutron_role
            ),
            "collect F3D xref placement losses",
            "retain F3D xref placement loss",
        )?;
    }
    Ok(())
}

/// Report structured placements that were ignored because a scope-bound
/// Component Insert carrier supplied the occurrence transform for the role.
fn report_xref_placement_overrides(
    ctx: &DecodeContext<'_>,
    report: &mut DecodeBody,
    table: &crate::xref::XrefTable,
) -> Result<(), CodecError> {
    for override_ in ctx.admit_iter(
        &table.placement_overrides,
        "scan F3D table placement overrides",
    )? {
        let ordinal = override_.ordinal;
        let count = override_.count;
        let Some(reference_index) = ctx.position_by(
            &table.references,
            |reference| Ok(reference.ordinal == ordinal),
            "find F3D superseded placement reference",
        )?
        else {
            continue;
        };
        let reference = &table.references[reference_index];
        push_loss_vec(ctx, &mut report.losses, F3dLossCode::XrefPlacementSuperseded, format_args!(
                "{count} structured placement record(s) for external occurrence {} and role {} were superseded by scope-bound Component Insert carrier(s)",
                reference.relative_path, reference.neutron_role
            ), "collect F3D xref placement losses", "retain F3D xref placement loss")?;
    }
    Ok(())
}

/// Classify a mesh-body document.
///
/// Mesh bodies use tessellation as their geometry carrier. The report marks
/// geometry as transferred and records vertex precision.
fn apply_mesh_body_classification(
    ctx: &DecodeContext<'_>,
    report: &mut DecodeBody,
    scan: &ContainerScan,
    bodies: usize,
) -> Result<(), CodecError> {
    if container::design_breps(ctx, scan)?
        .next()
        .transpose()?
        .is_some()
    {
        return Ok(());
    }
    ctx.retain_vec(
        &mut report.losses,
        |loss| {
            Ok(!matches!(
                loss.code.taxonomy(),
                LossTaxonomy::GeometryNotTransferred
                    | LossTaxonomy::TopologyNotTransferred
                    | LossTaxonomy::MissingGeometryStream
            ))
        },
        "drop F3D geometry transfer losses",
    )?;
    report.transfer = cadmpeg_ir::report::decode::DecodeTransfer::full(true);
    push_loss_vec(
        ctx,
        &mut report.losses,
        F3dLossCode::MeshVertexPrecisionReduced,
        format_args!(
            "{bodies} mesh body geometry container(s) store vertex coordinates at f32 precision"
        ),
        "collect F3D mesh classification losses",
        "retain F3D mesh classification loss",
    )
}

/// Classify a bodyless design whose transferred content requires no BREP.
///
/// The container has zero BREP streams and the Design segment has zero bodies.
/// Sketch entities can supply the complete geometry. Reference-image timeline
/// objects are presentation content and require no geometry carrier.
fn apply_bodyless_design_classification(
    ctx: &DecodeContext<'_>,
    report: &mut DecodeBody,
    brep_streams: usize,
    text_brep_streams: usize,
    declared_bodies: usize,
    sketch_entities: usize,
    reference_images: usize,
) -> Result<(), CodecError> {
    const OPERATION: &str = "collect F3D bodyless classification losses";
    if brep_streams != 0
        || text_brep_streams != 0
        || declared_bodies != 0
        || (sketch_entities == 0 && reference_images == 0)
    {
        return Ok(());
    }
    ctx.retain_vec(
        &mut report.losses,
        |loss| {
            Ok(!matches!(
                loss.code.taxonomy(),
                LossTaxonomy::GeometryNotTransferred
                    | LossTaxonomy::TopologyNotTransferred
                    | LossTaxonomy::MissingGeometryStream
            ))
        },
        "drop F3D geometry transfer losses",
    )?;
    report.transfer = cadmpeg_ir::report::decode::DecodeTransfer::full(true);
    let message = match (sketch_entities, reference_images) {
        (0, _) => ctx.format_retained(format_args!(
                "presentation-only design: the document declares no body, and its {reference_images} reference-image timeline object(s) require no BREP geometry"
            ), "retain F3D bodyless classification loss")?,
        (_, 0) => ctx.format_retained(format_args!(
                "sketch-only design: the document declares no body, and its {sketch_entities} sketch entity(s) are its complete geometry"
            ), "retain F3D bodyless classification loss")?,
        _ => ctx.format_retained(format_args!(
                "bodyless design: the document declares no body; its {sketch_entities} sketch entity(s) are its complete geometry, and its {reference_images} reference-image timeline object(s) require no BREP geometry"
            ), "retain F3D bodyless classification loss")?,
    };

    ctx.reserve_vec(&mut report.losses, 1, OPERATION)?;
    report
        .losses
        .push(F3dLossCode::BodylessDesignCarrier.note(message));
    Ok(())
}

/// Reclassify a BREP-less assembly document: its model is the placement of
/// its XREF targets, so producing no geometry is not a loss
/// ([spec §1.4](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/f3d.md#14-external-references)).
fn apply_assembly_classification(
    ctx: &DecodeContext<'_>,
    report: &mut DecodeBody,
    scan: &ContainerScan,
    table: &crate::xref::XrefTable,
) -> Result<(), CodecError> {
    if !crate::xref::is_assembly(ctx, scan, Some(table))? {
        return Ok(());
    }
    ctx.retain_vec(
        &mut report.losses,
        |loss| {
            Ok(!(loss.severity >= Severity::Error
                && matches!(
                    loss.code.category(),
                    LossCategory::Geometry | LossCategory::Topology
                )))
        },
        "drop F3D assembly geometry losses",
    )?;
    push_loss_vec(
        ctx,
        &mut report.losses,
        F3dLossCode::AssemblyComponentsExternal,
        format_args!(
            "assembly document: geometry is defined by {} external reference(s); decode the \
         containing .f3z archive to resolve them",
            table.references.len()
        ),
        "collect F3D assembly classification losses",
        "retain F3D assembly classification loss",
    )?;
    for reference in ctx.admit_iter(&table.references, "scan F3D table references")? {
        let property_note = XrefPropertyNote(reference);
        match crate::xref::design_for(ctx, table, reference)? {
            Some(design) => ctx.push_formatted_retained(
                &mut report.notes,
                format_args!(
                    "xref {}: {} -> {} (lineage {}, version {}, {})",
                    reference.ordinal,
                    design.display_name,
                    design.target_file_name,
                    design.lineage_urn,
                    design.version_urn,
                    property_note
                ),
                "collect F3D decode notes",
                "retain F3D decode note",
            )?,
            None => ctx.push_formatted_retained(
                &mut report.notes,
                format_args!(
                    "xref {}: -> {} ({})",
                    reference.ordinal, reference.relative_path, property_note
                ),
                "collect F3D decode notes",
                "retain F3D decode note",
            )?,
        }
    }
    Ok(())
}

struct XrefPropertyNote<'a>(&'a crate::records::xref::XrefReference);

impl std::fmt::Display for XrefPropertyNote<'_> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "neutronRole {}", self.0.neutron_role)?;
        if !self.0.neutron_data.is_empty() && self.0.neutron_data != self.0.neutron_role.as_str() {
            write!(formatter, ", neutronData {}", self.0.neutron_data)?;
        }
        Ok(())
    }
}

/// A decoded member whose authored source remains available to archive composition.
pub(crate) struct AuthoredDecoded {
    pub(crate) ir: CadIr,
    pub(crate) source: cadmpeg_ir::SourceMeta,
    pub(crate) body: DecodeBody,
    pub(crate) source_fidelity: cadmpeg_ir::SourceFidelity,
}

impl AuthoredDecoded {
    pub(crate) fn into_decoded(mut self) -> Decoded {
        self.ir.source = Some(self.source);
        Decoded {
            ir: self.ir,
            body: self.body,
            source_fidelity: self.source_fidelity,
        }
    }
}

struct RetainedArtifacts {
    annotations: cadmpeg_ir::Annotations,
    unknowns: Vec<UnknownRecord>,
    source_image: UnknownRecord,
    source_attributes: std::collections::BTreeMap<String, String>,
}

fn decode_result(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan<'_>,
    report_scope: crate::report::ReportScope,
    mut ir: CadIr,
    mut report: DecodeBody,
    retained: RetainedArtifacts,
    admitted_entities: &mut u64,
) -> Result<AuthoredDecoded, CodecError> {
    // ASM transfer already charged its delta; admit any remaining neutral entities
    // (sketches, appearances, products) before finalizing.
    ctx.admit_entities(
        u64_from_index(ir.model.entity_count()),
        admitted_entities,
        "admit F3D entities",
    )?;
    let mut source_fidelity = cadmpeg_ir::SourceFidelity::with_annotations(retained.annotations);
    source_fidelity.attach_native_unknown_records(&mut ir, "f3d", retained.unknowns, ctx)?;
    source_fidelity.retain_unknown_records("f3d", [retained.source_image])?;
    let mut source = crate::report::classify_document(
        ctx,
        scan,
        report_scope,
        retained.source_attributes,
        &mut report,
    )?;
    // Stamped on the finalized, classified document, so the write path
    // compares against the exact document the sealed wrapper returns.
    ir.finalize(ctx)?;
    let hash = cadmpeg_ir::hash::document_local_sha256(
        ctx,
        &ir,
        Some(&source),
        "f3d",
        crate::ids::FILE_SOURCE_IMAGE_ID,
        "record F3D document digest",
    )?;
    ctx.insert_btree_map(
        &mut source.attributes,
        cadmpeg_core::nonblank_const!(cadmpeg_ir::hash::DOCUMENT_LOCAL_DIGEST_ATTRIBUTE),
        hash,
        "record F3D document digest",
    )
    .map(|_| ())?;
    Ok(AuthoredDecoded {
        ir,
        source,
        body: report,
        source_fidelity,
    })
}

pub(crate) fn preserve_source_image(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan,
) -> Result<UnknownRecord, CodecError> {
    Ok(UnknownRecord::retained(
        crate::ids::file_source_image_id(ctx)?,
        0,
        ctx.copy_retained(scan.source_image, "retain F3D source image")?,
        Vec::new(),
    ))
}

/// Machine-local `document_local_sha256` for the F3D write-path edit oracle.
///
/// See [`cadmpeg_ir::hash::document_local_sha256`].
pub(crate) fn document_local_sha256(ir: &CadIr) -> Result<String, CodecError> {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let policy = cadmpeg_core::decode::DecodePolicy::default();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)?;
    let digest = cadmpeg_ir::hash::document_local_sha256(
        &ctx,
        ir,
        ir.source.as_ref(),
        "f3d",
        crate::ids::FILE_SOURCE_IMAGE_ID,
        "record F3D document digest",
    )?;
    ctx.finish_session()?;
    Ok(digest)
}

fn annotation_stream(
    ctx: &DecodeContext<'_>,
    entry_name: &str,
) -> Result<StreamHandle, CodecError> {
    let name = crate::ids::native_scope(ctx, entry_name, "retain F3D native scope")?;
    let name = cadmpeg_ir::StreamName::try_from(name).map_err(CodecError::malformed)?;
    StreamHandle::new(ctx, name, "allocate annotation stream handle")
}

fn note_native_annotation(
    ctx: &DecodeContext<'_>,
    annotations: &mut AnnotationBuilder,
    stream: &StreamHandle,
    id: &str,
    tag: &str,
) -> Result<(), CodecError> {
    annotations.note(ctx, id, stream, trailing_offset(ctx, id)?, Some(tag))
}

fn populate_annotations(
    ctx: &DecodeContext<'_>,
    ir: &CadIr,
    scan: &ContainerScan,
    native: &F3dNative,
    brep: Option<(&str, &[cadmpeg_asm::brep::annotations::AnnotationRecord])>,
    unknowns: &[UnknownRecord],
) -> Result<cadmpeg_ir::Annotations, cadmpeg_core::CodecError> {
    use std::collections::HashMap;

    let mut annotations = AnnotationBuilder::new();
    if let Some((stream_name, records)) = brep {
        let stream = annotation_stream(ctx, stream_name)?;
        for record in records {
            annotations.note(
                ctx,
                &record.id,
                &stream,
                record.offset,
                Some(record.tag.as_str()),
            )?;
            for field in
                ctx.admit_iter(&record.derived_fields, "scan F3D derived annotation fields")?
            {
                annotations.derived(ctx, &record.id, field)?;
            }
        }
    }

    let mut constraint_storage = ctx.reserve_scoped(0, "index F3D annotation constraints")?;
    let mut constraints_by_native = HashMap::new();
    for constraint in ctx.admit_iter(
        &ir.model.sketch_constraints,
        "scan F3D ir model sketch constraints",
    )? {
        if let Some(native_ref) = constraint.native_ref.as_deref() {
            constraint_storage.with_storage(|| {
                if !ctx.contains_key_hash_map(
                    &constraints_by_native,
                    native_ref,
                    "index F3D annotation constraints",
                )? {
                    ctx.insert_hash_map(
                        &mut constraints_by_native,
                        native_ref,
                        constraint.id.as_str(),
                        "index F3D annotation constraints",
                    )?;
                }
                Ok::<(), CodecError>(())
            })?;
        }
    }
    let mut entity_storage = ctx.reserve_scoped(0, "index F3D annotation entities")?;
    let mut entities_by_native = HashMap::new();
    for entity in ctx.admit_iter(
        &ir.model.sketch_entities,
        "scan F3D ir model sketch entities",
    )? {
        if let Some(native_ref) = entity.native_ref.as_deref() {
            entity_storage.with_storage(|| {
                if !ctx.contains_key_hash_map(
                    &entities_by_native,
                    native_ref,
                    "index F3D annotation entities",
                )? {
                    ctx.insert_hash_map(
                        &mut entities_by_native,
                        native_ref,
                        entity.id().as_str(),
                        "index F3D annotation entities",
                    )?;
                }
                Ok::<(), CodecError>(())
            })?;
        }
    }
    let planar_sketches = ctx.collect_hash_set(
        ir.model.sketches.iter().map(|sketch| sketch.id.as_str()),
        "index F3D annotation planar sketches",
    )?;
    let spatial_sketches = ctx.collect_hash_set(
        ir.model
            .spatial_sketches
            .iter()
            .map(|sketch| sketch.id.as_str()),
        "index F3D annotation spatial sketches",
    )?;

    let native_stream = StreamHandle::new(
        ctx,
        cadmpeg_ir::stream_name!("f3d:native"),
        "allocate annotation stream handle",
    )?;
    macro_rules! note {
        ($id:expr, $tag:expr $(,)?) => {{
            note_native_annotation(ctx, &mut annotations, &native_stream, $id, $tag)?;
        }};
    }
    {
        for entity in ctx.admit_iter(
            &native.construction_recipes,
            "scan F3D native construction recipes",
        )? {
            note!(&entity.id, "construction_recipe");
        }
        for entity in ctx.admit_iter(
            &native.persistent_references,
            "scan F3D native persistent references",
        )? {
            note!(&entity.id, "persistent_reference");
        }
        for entity in ctx.admit_iter(
            &native.lost_edge_references,
            "scan F3D native lost edge references",
        )? {
            note!(&entity.id, "EDGE_REFERENCE_LOST");
        }
        for entity in ctx.admit_iter(&native.design_types, "scan F3D native design types")? {
            note!(entity.id(), "design_type");
        }
        for entity in ctx.admit_iter(
            &native.design_parameters,
            "scan F3D native design parameters",
        )? {
            note!(&entity.id, "design_parameter");
        }
        for entity in ctx.admit_iter(
            &native.design_parameter_companions,
            "scan F3D native design parameter companions",
        )? {
            note!(entity.id(), "design_parameter_companion");
        }
        for entity in ctx.admit_iter(
            &native.design_dimension_locus_pairs[..],
            "scan F3D native design dimension locus pairs",
        )? {
            note!(&entity.id, "design_dimension_locus_pair");
            if let Some(projected) = ctx.get_hash_map(
                &constraints_by_native,
                entity.id.as_str(),
                "find F3D annotated constraint",
            )? {
                note!(projected, "sketch_constraint");
            }
        }
        for entity in ctx.admit_iter(
            &native.design_dimension_annotation_frames,
            "scan F3D native design dimension annotation frames",
        )? {
            note!(&entity.id, "design_dimension_annotation_frame");
            if let Some(projected) = ctx.get_hash_map(
                &constraints_by_native,
                entity.id.as_str(),
                "find F3D annotated constraint",
            )? {
                note!(projected, "sketch_constraint");
            }
        }
        for entity in ctx.admit_iter(
            &native.design_dimension_presentation_frames,
            "scan F3D native design dimension presentation frames",
        )? {
            note!(&entity.id, "design_dimension_presentation_frame");
            let projected = ctx.find_map(
                &native.design_parameter_companions,
                |companion| {
                    if ctx.equal(
                        &crate::ids::native_stream(companion.id()),
                        &crate::ids::native_stream(&entity.id),
                        "compare F3D annotation streams",
                    )? && companion.record_index() == entity.governing_companion_record_index
                    {
                        ctx.get_hash_map(
                            &constraints_by_native,
                            companion.id(),
                            "find F3D annotated constraint",
                        )
                    } else {
                        Ok(None)
                    }
                },
                "find F3D annotated dimension companion",
            )?;
            if let Some(projected) = projected {
                note!(projected, "sketch_constraint");
            }
        }
        for entity in ctx.admit_iter(
            &native.design_dimension_locus_groups,
            "scan F3D native design dimension locus groups",
        )? {
            note!(&entity.id, "design_dimension_locus_group");
            if let Some(projected) = ctx.get_hash_map(
                &constraints_by_native,
                entity.id.as_str(),
                "find F3D annotated constraint",
            )? {
                note!(projected, "sketch_constraint");
            }
        }
        for entity in ctx.admit_iter(
            &native.design_dimension_null_locus_pairs[..],
            "scan F3D native design dimension null locus pairs",
        )? {
            note!(&entity.id, "design_dimension_null_locus_pair");
            if let Some(projected) = ctx.get_hash_map(
                &constraints_by_native,
                entity.id.as_str(),
                "find F3D annotated constraint",
            )? {
                note!(projected, "sketch_constraint");
            }
        }
        for entity in ctx.admit_iter(
            &native.design_parameter_owners,
            "scan F3D native design parameter owners",
        )? {
            note!(entity.id(), "design_parameter_owner");
        }
        for entity in ctx.admit_iter(
            &native.design_parameter_scopes,
            "scan F3D native design parameter scopes",
        )? {
            note!(&entity.id, "design_parameter_scope");
        }
        for entity in ctx.admit_iter(
            &native.design_edge_operands,
            "scan F3D native design edge operands",
        )? {
            note!(&entity.id, "design_edge_operand");
        }
        for entity in ctx.admit_iter(
            &native.design_face_operands,
            "scan F3D native design face operands",
        )? {
            note!(&entity.id, "design_face_operand");
        }
        for entity in ctx.admit_iter(
            &native.design_face_source_groups,
            "scan F3D native design face source groups",
        )? {
            note!(&entity.id, "design_face_source_group");
        }
        for entity in ctx.admit_iter(
            &native.design_sketch_placements,
            "scan F3D native design sketch placements",
        )? {
            note!(&entity.id, "design_sketch_placement");
            let planar = crate::ids::neutral_sketch_id_charged(ctx, entity)?;
            if ctx.contains_hash_set(
                &planar_sketches,
                planar.as_str(),
                "find F3D planar annotation sketch",
            )? {
                note!(planar.as_str(), "sketch");
            }
            let spatial = crate::ids::neutral_spatial_sketch_id_charged(ctx, entity)?;
            if ctx.contains_hash_set(
                &spatial_sketches,
                spatial.as_str(),
                "find F3D spatial annotation sketch",
            )? {
                note!(spatial.as_str(), "spatial_sketch");
            }
        }
        for entity in ctx.admit_iter(
            &native.design_entity_headers,
            "scan F3D native design entity headers",
        )? {
            note!(&entity.id, "design_entity_header");
        }
        for entity in ctx.admit_iter(
            &native.design_record_headers,
            "scan F3D native design record headers",
        )? {
            note!(&entity.id, "design_record_header");
        }
        for entity in ctx.admit_iter(
            &native.design_body_members,
            "scan F3D native design body members",
        )? {
            note!(entity.id(), "BodiesRoot");
        }
        for entity in ctx.admit_iter(
            &native.design_material_assignments,
            "scan F3D native design material assignments",
        )? {
            note!(&entity.id, "material_assignment");
        }
        for entity in
            ctx.admit_iter(&native.sketch_relations, "scan F3D native sketch relations")?
        {
            note!(&entity.id, "sketch_relation");
            if ctx.contains_key_hash_map(
                &constraints_by_native,
                entity.id.as_str(),
                "find F3D annotated constraint",
            )? {
                let constraint = crate::ids::neutral_sketch_constraint_id_charged(
                    ctx,
                    &entity.id,
                    entity.record_index,
                )?;
                note!(constraint.as_str(), "sketch_constraint");
            }
        }
        for entity in ctx.admit_iter(&native.sketch_points, "scan F3D native sketch points")? {
            note!(&entity.id, "sketch_point");
            if let Some(projected) = ctx.get_hash_map(
                &entities_by_native,
                entity.id.as_str(),
                "find F3D annotated entity",
            )? {
                note!(projected, "sketch_entity");
            }
        }
        for entity in ctx.admit_iter(
            &native.sketch_curve_identities,
            "scan F3D native sketch curve identities",
        )? {
            note!(&entity.id, "sketch_curve");
            if let Some(projected) = ctx.get_hash_map(
                &entities_by_native,
                entity.id.as_str(),
                "find F3D annotated entity",
            )? {
                note!(projected, "sketch_entity");
            }
        }
        for entity in ctx.admit_iter(&native.sketch_surfaces, "scan F3D native sketch surfaces")? {
            note!(&entity.id, "sketch_surface");
        }
        for entity in ctx.admit_iter(
            &native.sketch_curve_links,
            "scan F3D native sketch curve links",
        )? {
            note!(&entity.id, "sketch_curve_link");
        }
        for entity in ctx.admit_iter(
            &native.persistent_design_links,
            "scan F3D native persistent design links",
        )? {
            note!(&entity.id, "persistent_design_link");
        }
        for entity in ctx.admit_iter(
            &native.persistent_subentity_tags,
            "scan F3D native persistent subentity tags",
        )? {
            note!(&entity.id, "persistent_subentity_tag");
        }
        for entity in ctx.admit_iter(&native.act_entities, "scan F3D native act entities")? {
            note!(entity.id(), "ACTEntity");
        }
        for entity in ctx.admit_iter(&native.act_guids, "scan F3D native act guids")? {
            note!(entity.id(), "ACTGuid");
        }
        for entity in ctx.admit_iter(
            &native.act_registry_channels,
            "scan F3D native act registry channels",
        )? {
            note!(entity.id(), "ACTRegistryChannel");
        }
        for entity in ctx.admit_iter(
            &native.act_root_components,
            "scan F3D native act root components",
        )? {
            note!(entity.id(), "ACTRootComponent");
        }
        for entity in ctx.admit_iter(
            &native.act_table_references,
            "scan F3D native act table references",
        )? {
            note!(entity.id(), "ACTTableReference");
        }
        for history in ctx.admit_iter(&native.asm_histories, "scan F3D native asm histories")? {
            note!(&history.id, "history_stream");
            for state in ctx.admit_iter(&history.states, "scan F3D annotation history states")? {
                note!(&state.id, "delta_state");
                for board in ctx.admit_iter(
                    &state.bulletin_boards,
                    "scan F3D annotation bulletin boards",
                )? {
                    note!(&board.id, "BulletinBoard");
                    for change in
                        ctx.admit_iter(&board.changes, "scan F3D annotation entity changes")?
                    {
                        note!(&change.id, "entity_change");
                    }
                }
                for record in
                    ctx.admit_iter(&state.records, "scan F3D annotation history records")?
                {
                    note!(&record.id, record.name());
                }
            }
        }
    }

    let appearance_stream_index = ctx.position_by(
        &scan.entries,
        |entry| scan.is_design_asset_entry(ctx, entry, ContainerRole::ProteinAssets),
        "find F3D protein asset entry",
    )?;
    let appearance_stream = appearance_stream_index
        .map(|index| annotation_stream(ctx, &scan.entries[index].name))
        .transpose()?;
    if let Some(stream) = appearance_stream {
        for appearance in ctx.admit_iter(&ir.model.appearances, "scan F3D ir model appearances")? {
            annotations.note(
                ctx,
                appearance.id.as_str(),
                &stream,
                0,
                Some(appearance.schema.as_deref().unwrap_or("appearance")),
            )?;
        }
    }
    for binding in ctx.admit_iter(
        &ir.model.appearance_bindings,
        "scan F3D ir model appearance bindings",
    )? {
        annotations.note(
            ctx,
            binding.id.as_str(),
            &native_stream,
            0,
            Some("appearance_binding"),
        )?;
    }
    if brep.is_none() {
        if let Some(fallback) = container::select_fallback_brep(ctx, scan)? {
            let stream = annotation_stream(ctx, &fallback.name)?;
            for unknown in ctx.admit_iter(unknowns, "scan F3D annotation unknown records")? {
                annotations.note(
                    ctx,
                    unknown.id().as_str(),
                    &stream,
                    unknown.offset(),
                    Some("opaque_brep"),
                )?;
            }
        }
    }
    Ok(annotations.build())
}

fn trailing_offset(ctx: &DecodeContext<'_>, id: &str) -> Result<u64, CodecError> {
    let mut end = id.len();
    for (index, byte) in ctx
        .admit_iter(id.as_bytes(), "scan F3D annotation offset components")?
        .enumerate()
        .rev()
    {
        if *byte == b':' {
            if let Ok(offset) =
                ctx.parse_text::<u64>(&id[index + 1..end], "parse F3D annotation offset")?
            {
                return Ok(offset);
            }
            end = index;
        }
    }
    match ctx.parse_text::<u64>(&id[..end], "parse F3D annotation offset")? {
        Ok(offset) => Ok(offset),
        Err(_) => Ok(0),
    }
}

fn decode_asm_history(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan,
    history_brep: &BrepFacts,
) -> Result<Option<crate::history_records::AsmHistory>, CodecError> {
    let width = history_brep
        .kernel
        .as_ref()
        .and_then(crate::container::KernelFraming::asm_header)
        .map_or(cadmpeg_asm::kernel_header::RefWidth::Eight, |header| {
            header.width
        });
    let bytes = scan.entry_bytes(ctx, &history_brep.name)?;
    crate::history::decode(ctx, bytes, &history_brep.name, width, &ctx.policy().limits)
}

#[derive(Clone, Copy)]
enum RelatedRecordIndexSource<'a> {
    RelationAndParameterLinks(&'a F3dNative),
    ParameterOwnerLinks(&'a F3dNative),
    ScopeReferences(&'a F3dNative),
    SelectionGroupMembers(&'a F3dNative),
    IdentityWrappersAndMembers(&'a F3dNative),
    #[cfg(test)]
    Explicit(&'a [(&'a str, u32)]),
}

fn collect_related_indices(
    ctx: &DecodeContext<'_>,
    source: RelatedRecordIndexSource<'_>,
) -> Result<Vec<(String, u32)>, CodecError> {
    let mut collected = Vec::new();
    let mut append = |stream: &str, index| {
        let stream = ctx.copy_retained_text(stream, "retain F3D related record stream")?;
        ctx.push_vec(
            &mut collected,
            (stream, index),
            "collect F3D related record indices",
        )
    };
    match source {
        RelatedRecordIndexSource::RelationAndParameterLinks(native) => {
            for relation in ctx.admit_iter(
                &native.sketch_relations,
                "scan F3D related sketch relations",
            )? {
                let stream =
                    crate::ids::native_stream(&relation.id).unwrap_or(crate::ids::DEFAULT_STREAM);
                for member in
                    ctx.admit_iter(&relation.members()[..], "scan F3D related sketch members")?
                {
                    append(stream, member.reference.record_index())?;
                }
                for member in ctx.admit_iter(
                    &relation.return_members()[..],
                    "scan F3D related sketch return members",
                )? {
                    append(stream, member.reference.record_index())?;
                }
            }
            for parameter in ctx.admit_iter(
                &native.design_parameters,
                "scan F3D related parameter owners",
            )? {
                if let (Some(stream), Some(index)) = (
                    crate::ids::native_stream(&parameter.id),
                    parameter.owner_record_index(),
                ) {
                    append(stream, index)?;
                }
            }
        }
        RelatedRecordIndexSource::ParameterOwnerLinks(native) => {
            for owner in ctx.admit_iter(
                &native.design_parameter_owners,
                "scan F3D related owner links",
            )? {
                let stream =
                    crate::ids::native_stream(owner.id()).unwrap_or(crate::ids::DEFAULT_STREAM);
                let links = [
                    owner.scope_record_index(),
                    owner.parameter_record_index(),
                    owner.companion_record_index(),
                ];
                for index in ctx.admit_iter(&links, "scan F3D related owner record links")? {
                    append(stream, *index)?;
                }
            }
        }
        RelatedRecordIndexSource::ScopeReferences(native) => {
            for scope in ctx.admit_iter(
                &native.design_parameter_scopes,
                "scan F3D related scope references",
            )? {
                let stream =
                    crate::ids::native_stream(&scope.id).unwrap_or(crate::ids::DEFAULT_STREAM);
                let (values, rows) = scope.reference_members().storage_slices();
                for index in ctx.admit_iter(values, "scan F3D related scope member values")? {
                    append(stream, *index)?;
                }
                for row in ctx.admit_iter(rows, "scan F3D related located scope members")? {
                    append(stream, row.value)?;
                }
            }
        }
        RelatedRecordIndexSource::SelectionGroupMembers(native) => {
            for group in ctx.admit_iter(
                &native.design_extrude_selection_groups,
                "scan F3D related extrude groups",
            )? {
                let stream =
                    crate::ids::native_stream(&group.id).unwrap_or(crate::ids::DEFAULT_STREAM);
                for member in ctx.admit_iter(group.members(), "scan F3D related extrude members")? {
                    append(stream, member.value)?;
                }
            }
            for group in ctx.admit_iter(
                &native.design_construction_operand_groups,
                "scan F3D related construction groups",
            )? {
                let stream =
                    crate::ids::native_stream(&group.id).unwrap_or(crate::ids::DEFAULT_STREAM);
                for member in
                    ctx.admit_iter(group.members(), "scan F3D related construction members")?
                {
                    append(stream, member.value)?;
                }
                for record in ctx.admit_iter(
                    group.frame.trailing_records(),
                    "scan F3D related trailing records",
                )? {
                    let index = record.value;
                    append(stream, index)?;
                    if let Some(next) = index.checked_add(1) {
                        append(stream, next)?;
                    }
                    if let Some(next) = index.checked_add(2) {
                        append(stream, next)?;
                    }
                    if let Some(next) = index.checked_add(3) {
                        append(stream, next)?;
                    }
                }
                for record in ctx.admit_iter(
                    &group.frame.auxiliary_records,
                    "scan F3D related auxiliary records",
                )? {
                    let index = record.value;
                    append(stream, index)?;
                    if let Some(next) = index.checked_add(1) {
                        append(stream, next)?;
                    }
                    if let Some(next) = index.checked_add(2) {
                        append(stream, next)?;
                    }
                }
            }
        }
        RelatedRecordIndexSource::IdentityWrappersAndMembers(native) => {
            for identity in ctx.admit_iter(
                &native.design_construction_operand_identities,
                "scan F3D related construction identities",
            )? {
                let stream =
                    crate::ids::native_stream(&identity.id).unwrap_or(crate::ids::DEFAULT_STREAM);
                for wrapper in
                    ctx.admit_iter(identity.wrappers(), "scan F3D related identity wrappers")?
                {
                    append(stream, wrapper.record_index)?;
                }
                append(stream, identity.following_record_index())?;
            }
            for group in ctx.admit_iter(
                &native.design_construction_operand_groups,
                "scan F3D related identity member groups",
            )? {
                let Some(stream) = crate::ids::native_stream(&group.id) else {
                    continue;
                };
                for member in
                    ctx.admit_iter(group.members(), "scan F3D related identity group members")?
                {
                    append(stream, member.value)?;
                }
            }
        }
        #[cfg(test)]
        RelatedRecordIndexSource::Explicit(indices) => {
            for (stream, index) in ctx.admit_iter(indices, "scan F3D related record index pairs")? {
                append(stream, *index)?;
            }
        }
    }
    Ok(collected)
}

fn append_related_record_headers(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan,
    native: &mut F3dNative,
    indices: &[(String, u32)],
) -> Result<(), CodecError> {
    let mut existing_header_storage = ctx.reserve_scoped(0, "index F3D existing record headers")?;
    let existing = existing_header_storage.with_storage(|| {
        ctx.collect_hash_set(
            ctx.admit_iter(
                &native.design_record_headers,
                "scan F3D existing design record headers",
            )?
            .filter_map(|record| {
                Some((crate::ids::native_stream(&record.id)?, record.record_index))
            }),
            "index F3D existing record headers",
        )
    })?;
    let mut related =
        crate::design::decode::sketch::decode_related_record_headers(ctx, scan, indices)?;
    ctx.retain_vec(
        &mut related,
        |record| match crate::ids::native_stream(&record.id) {
            Some(stream) => Ok(!ctx.contains_hash_set(
                &existing,
                &(stream, record.record_index),
                "find F3D existing related record header",
            )?),
            None => Ok(true),
        },
        "retain F3D additional related record headers",
    )?;
    drop(existing);
    drop(existing_header_storage);
    ctx.extend_vec(
        &mut native.design_record_headers,
        related,
        "append F3D related record headers",
    )?;
    ctx.stable_sort_by(
        &mut native.design_record_headers,
        |value| &value.id,
        Ord::cmp,
        "sort F3D design record headers",
    )?;
    Ok(())
}

fn extend_related_design_records(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan,
    native: &mut F3dNative,
) -> Result<(), CodecError> {
    let mut related_indices_storage = ctx.reserve_scoped(0, "hold F3D related record indices")?;
    let indices = related_indices_storage.with_storage(|| {
        collect_related_indices(
            ctx,
            RelatedRecordIndexSource::RelationAndParameterLinks(native),
        )
    })?;
    append_related_record_headers(ctx, scan, native, &indices)?;
    drop(indices);
    drop(related_indices_storage);
    native.design_parameter_owners = crate::design::decode::parameters::decode_parameter_owners(
        ctx,
        scan,
        &native.design_parameters,
        &native.design_record_headers,
    )?;
    let mut related_indices_storage = ctx.reserve_scoped(0, "hold F3D related record indices")?;
    let indices = related_indices_storage.with_storage(|| {
        collect_related_indices(ctx, RelatedRecordIndexSource::ParameterOwnerLinks(native))
    })?;
    append_related_record_headers(ctx, scan, native, &indices)?;
    drop(indices);
    drop(related_indices_storage);
    native.design_parameter_companions =
        crate::design::decode::parameters::decode_parameter_companions(
            ctx,
            scan,
            &native.design_parameter_owners,
            &native.design_record_headers,
        )?;
    native.design_component_occurrences =
        crate::design::decode::components::decode_component_occurrences(ctx, scan)?;
    native.design_parameter_scopes =
        crate::design::decode::scopes::parameter_scope::decode_parameter_scopes(ctx, scan, native)?;
    native.design_surface_trim_operations =
        crate::design::decode::surface_trim::decode_surface_trim_operations(
            ctx,
            scan,
            &native.design_parameter_scopes,
        )?;
    crate::design::decode::scopes::parameter_scope::admit_history_bound_scope_variants(
        ctx,
        &mut native.design_parameter_scopes,
        &native.asm_histories,
    )?;
    native.design_face_source_groups = crate::design::decode::operands::decode_face_source_groups(
        ctx,
        scan,
        &native.design_parameter_scopes,
    )?;
    native.design_feature_timelines =
        crate::design::decode::meta::decode_feature_timelines(ctx, scan)?;
    native.design_component_naming_spaces =
        crate::design::decode::meta::decode_component_naming_spaces(ctx, scan)?;
    native.design_canvas_images = crate::design::decode::canvas::decode_canvas_images(
        ctx,
        scan,
        &native.design_parameter_scopes,
    )?;
    native.design_decal_images = crate::design::decode::decal::decode_decal_images(
        ctx,
        scan,
        &native.design_parameter_scopes,
    )?;
    crate::design::decode::operands::disambiguate_fixed_fillet_parameters(
        ctx,
        &mut native.design_parameter_scopes,
        &native.design_parameter_owners,
    )?;
    let mut scope_header_storage = ctx.reserve_scoped(0, "index F3D scope record headers")?;
    let mut existing = scope_header_storage.with_storage(|| {
        ctx.collect_hash_set(
            ctx.admit_iter(
                &native.design_record_headers,
                "scan F3D existing design record headers",
            )?
            .filter_map(|record| {
                Some((crate::ids::native_stream(&record.id)?, record.record_index))
            }),
            "index F3D scope record headers",
        )
    })?;
    let mut scope_headers_storage = ctx.reserve_scoped(0, "collect F3D scope record headers")?;
    let mut scope_headers = Vec::new();
    for scope in ctx.admit_iter(
        &native.design_parameter_scopes,
        "scan F3D native design parameter scopes",
    )? {
        let Some(stream) = crate::ids::native_stream(&scope.id) else {
            continue;
        };
        if !ctx.contains_hash_set(
            &existing,
            &(stream, scope.record_index),
            "find F3D existing scope record header",
        )? {
            scope_header_storage.with_storage(|| {
                ctx.insert_hash_set(
                    &mut existing,
                    (stream, scope.record_index),
                    "index F3D scope record headers",
                )
                .map(|_| ())
            })?;
            let id = ctx.format_retained(
                format_args!("{stream}:design-record-header#{}", scope.byte_offset()),
                "retain F3D scope record header ID",
            )?;
            ctx.push_scoped_vec(
                &mut scope_headers_storage,
                &mut scope_headers,
                crate::records::decal::DesignRecordHeader {
                    id,
                    record_index: scope.record_index,
                    class_tag: scope
                        .class_tag
                        .try_clone_for_decode(ctx, "copy F3D scope record class tag")?,
                    byte_offset: scope.byte_offset(),
                },
                "collect F3D scope record headers",
            )?;
        }
        if let Some(operation) = scope.copy_paste_bodies_operation() {
            if !ctx.contains_hash_set(
                &existing,
                &(stream, operation.relation_record_index),
                "find F3D existing copy-paste relation header",
            )? {
                scope_header_storage.with_storage(|| {
                    ctx.insert_hash_set(
                        &mut existing,
                        (stream, operation.relation_record_index),
                        "index F3D scope record headers",
                    )
                    .map(|_| ())
                })?;
                let id = ctx.format_retained(
                    format_args!(
                        "{stream}:design-record-header#{}",
                        operation.relation_byte_offset()
                    ),
                    "retain F3D scope record header ID",
                )?;
                ctx.push_scoped_vec(
                    &mut scope_headers_storage,
                    &mut scope_headers,
                    crate::records::decal::DesignRecordHeader {
                        id,
                        record_index: operation.relation_record_index,
                        class_tag: operation
                            .relation_class_tag
                            .try_clone_for_decode(ctx, "copy F3D relation record class tag")?,
                        byte_offset: operation.relation_byte_offset(),
                    },
                    "collect F3D scope record headers",
                )?;
            }
        }
    }
    drop(existing);
    drop(scope_header_storage);
    ctx.extend_vec(
        &mut native.design_record_headers,
        scope_headers,
        "append F3D scope record headers",
    )?;
    drop(scope_headers_storage);
    let mut related_indices_storage = ctx.reserve_scoped(0, "hold F3D related record indices")?;
    let indices = related_indices_storage.with_storage(|| {
        collect_related_indices(ctx, RelatedRecordIndexSource::ScopeReferences(native))
    })?;
    append_related_record_headers(ctx, scan, native, &indices)?;
    drop(indices);
    drop(related_indices_storage);
    crate::design::decode::operands::bind_sketch_profiles(
        ctx,
        scan,
        &mut native.design_parameter_scopes,
        &native.design_record_headers,
        &native.design_entity_headers,
    )?;
    native.design_construction_operand_groups =
        crate::design::decode::operands::decode_construction_operand_groups(
            ctx,
            scan,
            &mut native.design_parameter_scopes,
            &native.design_record_headers,
        )?;
    native.design_loft_legacy_body_carriers =
        crate::design::decode::operands::decode_loft_legacy_body_carriers(
            ctx,
            scan,
            &native.design_parameter_scopes,
            &native.design_record_headers,
        )?;
    crate::design::decode::scopes::mirror::bind_mirror_constructions(
        ctx,
        scan,
        &mut native.design_parameter_scopes,
        &native.design_construction_operand_groups,
        &native.design_record_headers,
        &native.design_parameter_owners,
        &native.construction_recipes,
    )?;
    native.design_extrude_selection_groups =
        crate::design::decode::operands::decode_extrude_selection_groups(
            ctx,
            scan,
            &native.design_parameter_scopes,
            &native.design_record_headers,
        )?;
    let mut related_indices_storage = ctx.reserve_scoped(0, "hold F3D related record indices")?;
    let indices = related_indices_storage.with_storage(|| {
        collect_related_indices(ctx, RelatedRecordIndexSource::SelectionGroupMembers(native))
    })?;
    append_related_record_headers(ctx, scan, native, &indices)?;
    drop(indices);
    drop(related_indices_storage);
    crate::design::decode::operands::bind_construction_operand_trailing_records(
        ctx,
        scan,
        &mut native.design_construction_operand_groups,
        &native.design_record_headers,
    )?;
    crate::design::decode::operands::bind_construction_operand_paths(
        ctx,
        scan,
        &mut native.design_construction_operand_groups,
        &native.design_record_headers,
    )?;
    native.design_construction_operand_identities =
        crate::design::decode::operands::decode_construction_operand_identities(
            ctx,
            scan,
            &native.design_construction_operand_groups,
            &native.design_record_headers,
        )?;
    let mut related_scope_storage = ctx.reserve_scoped(0, "index F3D related parameter scopes")?;
    let scopes = related_scope_storage.with_storage(|| {
        ctx.collect_hash_map(
            ctx.admit_iter(
                &native.design_parameter_scopes,
                "scan F3D related parameter scopes",
            )?
            .filter_map(|scope| {
                Some((
                    (crate::ids::native_stream(&scope.id)?, scope.record_index),
                    scope.kind(),
                ))
            }),
            "index F3D related parameter scopes",
        )
    })?;
    let mut identified_group_storage =
        ctx.reserve_scoped(0, "index F3D identified construction groups")?;
    let identified_groups = identified_group_storage.with_storage(|| {
        ctx.collect_hash_set(
            ctx.admit_iter(
                &native.design_construction_operand_identities,
                "scan F3D identified construction groups",
            )?
            .filter_map(|identity| {
                Some((
                    crate::ids::native_stream(&identity.id)?,
                    identity.group_record_index,
                ))
            }),
            "index F3D identified construction groups",
        )
    })?;
    native.design_edge_identity_operands =
        crate::design::decode::operands::decode_edge_identity_operands(
            ctx,
            scan,
            &native.design_parameter_scopes,
            &native.design_construction_operand_groups,
            &native.design_record_headers,
        )?;
    let mut identity_group_storage = ctx.reserve_scoped(0, "index F3D edge identity groups")?;
    let identity_member_groups = identity_group_storage.with_storage(|| {
        ctx.collect_hash_set(
            ctx.admit_iter(
                &native.design_edge_identity_operands,
                "scan F3D edge identity groups",
            )?
            .filter_map(|operand| {
                Some((
                    crate::ids::native_stream(&operand.id)?,
                    operand.group_record_index,
                ))
            }),
            "index F3D edge identity groups",
        )
    })?;
    ctx.retain_vec(
        &mut native.design_construction_operand_groups,
        |group| {
            let Some(stream) = crate::ids::native_stream(&group.id) else {
                return Ok(true);
            };
            let kind = ctx.get_hash_map(
                &scopes,
                &(stream, group.scope_record_index),
                "find F3D construction operand scope kind",
            )?;
            Ok(
                crate::design::decode::operands::construction_operand_group_is_retained(
                    kind,
                    ctx.contains_hash_set(
                        &identified_groups,
                        &(stream, group.record_index),
                        "find F3D identified construction group",
                    )? || ctx.contains_hash_set(
                        &identity_member_groups,
                        &(stream, group.record_index),
                        "find F3D edge identity member group",
                    )?,
                ),
            )
        },
        "retain F3D construction operand groups",
    )?;
    native.design_fillet_radius_groups =
        crate::design::decode::operands::decode_fillet_radius_groups(
            ctx,
            &native.design_parameter_scopes,
            &native.design_construction_operand_groups,
            &native.design_parameter_owners,
            &native.design_parameters,
        )?;
    crate::design::decode::operands::bind_lost_edge_groups(
        ctx,
        &mut native.design_construction_operand_groups,
        &native.design_construction_operand_identities,
        &native.lost_edge_references,
    )?;
    let mut related_indices_storage = ctx.reserve_scoped(0, "hold F3D related record indices")?;
    let indices = related_indices_storage.with_storage(|| {
        collect_related_indices(
            ctx,
            RelatedRecordIndexSource::IdentityWrappersAndMembers(native),
        )
    })?;
    append_related_record_headers(ctx, scan, native, &indices)?;
    drop(indices);
    drop(related_indices_storage);
    native.design_extrude_selection_members =
        crate::design::decode::operands::decode_extrude_selection_members(
            ctx,
            scan,
            &native.design_extrude_selection_groups,
            &native.design_record_headers,
        )?;
    native.design_entity_selection_operands =
        crate::design::decode::operands::decode_entity_selection_operands(
            ctx,
            scan,
            &native.design_construction_operand_groups,
            &native.design_record_headers,
        )?;
    crate::history::selection::bind_entity_selection_history(
        ctx,
        &mut native.design_entity_selection_operands,
        &native.design_parameter_scopes,
        &native.asm_histories,
    )?;
    crate::history::selection::bind_hole_selection_history(
        ctx,
        &mut native.design_parameter_scopes,
        &native.asm_histories,
    )?;
    native.design_body_recipe_operands =
        crate::design::decode::operands::decode_body_recipe_operands(
            ctx,
            scan,
            &native.design_parameter_scopes,
            &native.design_construction_operand_groups,
            &native.design_record_headers,
            &native.construction_recipes,
        )?;
    crate::design::decode::operands::bind_body_recipe_operand_candidates(
        ctx,
        &mut native.design_body_recipe_operands,
        &native.construction_recipes,
        &native.persistent_subentity_tags,
        &native.design_parameter_scopes,
    )?;
    crate::history::bind_body_recipe_operand_history_candidates(
        ctx,
        &mut native.design_body_recipe_operands,
        &native.construction_recipes,
        &native.design_parameter_scopes,
        &native.asm_histories,
    )?;
    crate::design::decode::operands::bind_extrude_selection_identities(
        ctx,
        &mut native.design_extrude_selection_members,
        &native.design_construction_operand_identities,
    )?;
    crate::history::selection::bind_extrude_selection_history(
        ctx,
        &mut native.design_extrude_selection_members,
        &native.design_component_naming_spaces,
        &native.design_body_bindings,
        &native.asm_histories,
    )?;
    let scope_histories = crate::history::bind_scope_histories(
        ctx,
        &native.design_parameter_scopes,
        &native.design_body_bindings,
        &native.design_body_recipe_operands,
        &native.asm_histories,
    )?;
    crate::history::selection::bind_circular_pattern_axes(
        ctx,
        &mut native.design_parameter_scopes,
        &native.asm_histories,
        &scope_histories,
    )?;
    crate::history::selection::bind_edge_identity_history(
        ctx,
        &mut native.design_edge_identity_operands,
        &native.design_construction_operand_identities,
        &native.design_parameter_scopes,
        &native.asm_histories,
        &scope_histories,
    )?;
    native.design_edge_operands = crate::design::decode::operands::decode_edge_operands(
        ctx,
        scan,
        &native.design_parameter_scopes,
        &native.design_construction_operand_groups,
        &native.design_record_headers,
        &native.construction_recipes,
    )?;
    crate::design::decode::operands::bind_edge_operand_candidates(
        ctx,
        &mut native.design_edge_operands,
        &native.construction_recipes,
        &native.persistent_subentity_tags,
    )?;
    crate::history::bind_edge_operand_history_candidates(
        ctx,
        &mut native.design_edge_operands,
        &native.design_parameter_scopes,
        &native.construction_recipes,
        &native.asm_histories,
        &scope_histories,
    )?;
    native.design_edge_treatment_vertex_operands =
        crate::design::decode::operands::decode_edge_treatment_vertex_operands(
            ctx,
            scan,
            &native.design_parameter_scopes,
            &native.design_construction_operand_groups,
            &native.design_record_headers,
            &native.construction_recipes,
        )?;
    crate::design::decode::operands::bind_edge_treatment_vertex_candidates(
        ctx,
        &mut native.design_edge_treatment_vertex_operands,
        &native.persistent_subentity_tags,
    )?;
    crate::history::bind_edge_treatment_vertex_history(
        ctx,
        &mut native.design_edge_treatment_vertex_operands,
        &native.design_parameter_scopes,
        &native.asm_histories,
        &scope_histories,
    )?;
    crate::design::decode::operands::bind_work_plane_constructions(
        ctx,
        scan,
        &mut native.design_parameter_scopes,
        &native.design_record_headers,
        &native.construction_recipes,
        &native.design_parameter_owners,
        &native.design_parameters,
    )?;
    crate::design::decode::operands::bind_vertex_recipe_candidates(
        ctx,
        &mut native.design_parameter_scopes,
        &native.persistent_subentity_tags,
    )?;
    crate::history::bind_vertex_recipe_history(
        ctx,
        &mut native.design_parameter_scopes,
        &native.design_feature_timelines,
        &native.asm_histories,
    )?;
    native.design_face_operands = crate::design::decode::operands::decode_face_operands(
        ctx,
        scan,
        &native.design_parameter_scopes,
        &native.design_construction_operand_groups,
        &native.design_record_headers,
        &native.construction_recipes,
    )?;
    crate::design::decode::operands::bind_face_operand_candidates(
        ctx,
        &mut native.design_face_operands,
        &native.construction_recipes,
        &native.persistent_subentity_tags,
    )?;
    crate::history::bind_face_operand_history_candidates(
        ctx,
        &mut native.design_face_operands,
        &native.design_parameter_scopes,
        &native.design_construction_operand_groups,
        &native.construction_recipes,
        &native.asm_histories,
        &scope_histories,
    )?;
    crate::history::selection::bind_mirror_selection_planes(
        ctx,
        &mut native.design_parameter_scopes,
        &native.design_construction_operand_groups,
        &native.design_entity_selection_operands,
        &native.design_face_operands,
        &native.design_construction_operand_identities,
        &native.asm_histories,
    )?;
    crate::history::selection::bind_edge_identity_bounded_face_rules(
        ctx,
        &mut native.design_edge_identity_operands,
        &native.design_face_operands,
    )?;
    native.design_sketch_placements = crate::design::decode::sketch::decode_sketch_placements(
        ctx,
        scan,
        &native.design_parameter_scopes,
        &native.design_entity_headers,
    )?;
    let mut stream_length_storage = ctx.reserve_scoped(0, "index F3D design stream lengths")?;
    let mut stream_lengths = std::collections::HashMap::new();
    for entry in ctx.admit_iter(&scan.entries, "scan F3D Design stream entries")? {
        if !scan.is_design_stream(ctx, entry, ContainerRole::Bulkstream)? {
            continue;
        }
        let bytes = scan.entry_bytes(ctx, &entry.name)?;
        stream_length_storage.with_storage(|| {
            let stream = crate::ids::native_scope(ctx, &entry.name, "retain F3D native scope")?;
            ctx.insert_hash_map(
                &mut stream_lengths,
                stream,
                bytes.len(),
                "index F3D design stream lengths",
            )
            .map(|_| ())
        })?;
    }
    native.design_parameter_companions =
        crate::design::decode::parameters::bind_parameter_companion_payloads(
            ctx,
            std::mem::take(&mut native.design_parameter_companions),
            &crate::design::decode::parameters::ParameterCompanionInputs {
                parameters: &native.design_parameters,
                owners: &native.design_parameter_owners,
                scopes: &native.design_parameter_scopes,
                entities: &native.design_entity_headers,
                headers: &native.design_record_headers,
                recipes: &native.construction_recipes,
                stream_lengths: &stream_lengths,
            },
        )?;
    native.design_dimension_recipe_records =
        crate::design::decode::dimension_frames::decode_dimension_recipe_records(
            ctx,
            scan,
            &native.design_parameters,
            &native.design_parameter_owners,
            &native.design_parameter_companions,
            &native.construction_recipes,
        )?;
    crate::design::decode::dimension_frames::bind_dimension_recipe_reference_candidates(
        ctx,
        &mut native.design_dimension_recipe_records,
        &native.persistent_subentity_tags,
    )?;
    crate::design::decode::dimension_frames::bind_dimension_recipe_edge_operands(
        ctx,
        &mut native.design_dimension_recipe_records,
        &native.design_edge_operands,
    )?;
    Ok(())
}

/// Frame and decode one Design-selected or explicit fallback BREP SAB stream.
///
/// The function returns `None` for an invalid header or a framed stream with no
/// geometry. The caller then builds the container-metadata IR.
fn try_decode_brep(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan,
    brep_entry: &BrepFacts,
) -> Result<Option<Brep>, CodecError> {
    let Some(crate::container::KernelFraming::Asm {
        header,
        solved_record_limit,
    }) = &brep_entry.kernel
    else {
        return Ok(None);
    };
    let width = header.width;

    let bytes = scan.entry_bytes(ctx, &brep_entry.name)?;
    let Some(start) = asm_header::record_stream_start(bytes) else {
        return Ok(None);
    };
    // A stream without a delta-state boundary is history-less: its final
    // `End-of-ASM-data` record ends at EOF without the `0x11` terminator, so
    // it needs the EOF-tolerant framer used for the history partition.
    let framed = match *solved_record_limit {
        Some(limit) => sab::frame(ctx, bytes, start, limit, width, None),
        None => sab::frame_history(ctx, bytes, start, bytes.len(), width, None),
    };
    let records = match framed {
        Ok(r) if !r.is_empty() => r,
        Err(cadmpeg_asm::stream_error::StreamFailure::Resource(error)) => return Err(error.into()),
        Err(cadmpeg_asm::stream_error::StreamFailure::Operation(error)) => {
            return Err(error.into_codec_error())
        }
        _ => return Ok(None),
    };

    let decoded = brep::decode(
        ctx,
        &records,
        bytes,
        &brep_entry.name,
        crate::ids::ID_FORMAT,
    )?;
    if decoded.asm.surfaces.is_empty()
        && decoded.asm.points.is_empty()
        && decoded.asm.faces.is_empty()
    {
        return Ok(None);
    }
    Ok(Some(decoded))
}

/// Assemble the IR document from the decoded B-rep graph.
fn build_geometry_ir(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan,
    primary_model_brep: &BrepFacts,
    brep: Brep,
) -> Result<
    (
        CadIr,
        std::collections::BTreeMap<String, String>,
        F3dNative,
        AsmTransferRemainder,
    ),
    CodecError,
> {
    let mut ir = CadIr::empty();
    let (source_attributes, tolerances) =
        source_attributes_and_tolerances(ctx, scan, primary_model_brep)?;
    ir.tolerances = tolerances;
    let Brep {
        asm,
        sketch_curve_links,
        persistent_design_links,
        persistent_subentity_tags,
        creation_timestamps,
    } = brep;
    let (namespace, remainder) = transfer_into_ir(ctx, &mut ir, "f3d", asm)?;
    let mut native = F3dNative::load_charged(ctx, namespace)?;
    native.sketch_curve_links = sketch_curve_links;
    native.persistent_design_links = persistent_design_links;
    native.persistent_subentity_tags = persistent_subentity_tags;
    native.creation_timestamps = creation_timestamps;
    Ok((ir, source_attributes, native, remainder))
}

/// Smallest linear tolerance, in millimeters, that the analytic sketch, profile
/// and region comparisons in this crate act on.
const MIN_ANALYTIC_LINEAR_TOLERANCE_MM: f64 = 1.0e-7;

/// Admit the kernel header tolerances `resabs` and `resnor`. A stated `resabs`
/// below the analytic floor cannot drive profile and region matching, so it is
/// refused here and never floored at a comparison site.
fn admit_kernel_tolerances(resabs: f64, resnor: f64) -> Result<Tolerances, CodecError> {
    let linear_mm = resabs * 10.0;
    let tolerances = Tolerances::new(linear_mm, resnor).map_err(CodecError::Malformed)?;
    if linear_mm < MIN_ANALYTIC_LINEAR_TOLERANCE_MM {
        return Err(CodecError::NotImplemented(format!(
            "kernel header resabs {linear_mm} mm is below the analytic linear \
             tolerance floor {MIN_ANALYTIC_LINEAR_TOLERANCE_MM} mm"
        )));
    }
    Ok(tolerances)
}

/// Source metadata attributes and kernel tolerances from the primary model BREP header.
fn source_attributes_and_tolerances(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan,
    primary_model_brep: &BrepFacts,
) -> Result<(std::collections::BTreeMap<String, String>, Tolerances), CodecError> {
    let mut attributes = std::collections::BTreeMap::new();
    if let Some(folder) = scan.design_asset_folder() {
        {
            let copy = ctx.copy_retained_text(folder, "retain F3D source attribute value")?;
            ctx.insert_btree_map(
                &mut attributes,
                "asset_folder".to_owned(),
                copy,
                "collect F3D source attributes",
            )?;
        }
    }
    ctx.insert_btree_map(
        &mut attributes,
        "zip_entry_count".to_owned(),
        scan.entries.len().to_string(),
        "collect F3D source attributes",
    )
    .map(|_| ())?;
    {
        let copy = ctx.copy_retained_text(
            &primary_model_brep.name,
            "retain F3D source attribute value",
        )?;
        ctx.insert_btree_map(
            &mut attributes,
            "active_brep".to_owned(),
            copy,
            "collect F3D source attributes",
        )?;
    }
    {
        let copy = ctx.copy_retained_text(
            primary_model_brep.sha256.as_str(),
            "retain F3D source attribute value",
        )?;
        ctx.insert_btree_map(
            &mut attributes,
            "active_brep_sha256".to_owned(),
            copy,
            "collect F3D source attributes",
        )?;
    }
    if let Some(off) = primary_model_brep
        .kernel
        .as_ref()
        .and_then(crate::container::KernelFraming::solved_record_limit)
    {
        ctx.insert_btree_map(
            &mut attributes,
            "solved_record_len".to_owned(),
            off.to_string(),
            "collect F3D source attributes",
        )
        .map(|_| ())?;
    }
    if let Some(unit) = crate::design::decode::units::decode_document_length_unit(ctx, scan)? {
        ctx.insert_btree_map(
            &mut attributes,
            "modeling_length_unit".to_owned(),
            unit,
            "collect F3D source attributes",
        )
        .map(|_| ())?;
    }

    let mut tolerances = Tolerances::default();
    if let Some(h) = primary_model_brep
        .kernel
        .as_ref()
        .and_then(crate::container::KernelFraming::model_metadata)
    {
        if let Some(pf) = &h.product_family {
            {
                let copy = ctx.copy_retained_text(pf, "retain F3D source attribute value")?;
                ctx.insert_btree_map(
                    &mut attributes,
                    "product_family".to_owned(),
                    copy,
                    "collect F3D source attributes",
                )?;
            }
        }
        if let Some(pv) = &h.product_version {
            {
                let copy = ctx.copy_retained_text(pv, "retain F3D source attribute value")?;
                ctx.insert_btree_map(
                    &mut attributes,
                    "product_version".to_owned(),
                    copy,
                    "collect F3D source attributes",
                )?;
            }
        }
        if let Some(sd) = &h.save_date {
            {
                let copy = ctx.copy_retained_text(sd, "retain F3D source attribute value")?;
                ctx.insert_btree_map(
                    &mut attributes,
                    "save_date".to_owned(),
                    copy,
                    "collect F3D source attributes",
                )?;
            }
        }
        if let (Some(resabs), Some(resnor)) = (h.linear, h.angular) {
            tolerances = admit_kernel_tolerances(resabs, resnor)?;
        }
    }

    Ok((attributes, tolerances))
}

/// Loss report for a successful geometry decode.
struct KindCounts<'a>(&'a std::collections::BTreeMap<String, usize>);

impl std::fmt::Display for KindCounts<'_> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        for (index, (name, count)) in self.0.iter().enumerate() {
            if index != 0 {
                formatter.write_str(", ")?;
            }
            write!(formatter, "{name}={count}")?;
        }
        Ok(())
    }
}

fn geometry_losses(
    ctx: &DecodeContext<'_>,
    decoded: &Brep,
) -> Result<Vec<cadmpeg_ir::report::loss::LossNote>, CodecError> {
    let s = &decoded.asm.stats;
    let mut losses = Vec::new();

    if s.nurbs_surfaces > 0 {
        push_loss_vec(
            ctx,
            &mut losses,
            F3dLossCode::NurbsSurfaceCarrier,
            format_args!(
                "{} spline surface record(s) were decoded into NURBS carriers from their inline \
             cached B-spline block.",
                s.nurbs_surfaces
            ),
            "collect F3D geometry losses",
            "retain F3D geometry loss",
        )?;
    }
    if s.nurbs_curves > 0 {
        push_loss_vec(
            ctx,
            &mut losses,
            F3dLossCode::NurbsCurveCarrier,
            format_args!(
                "{} procedural curve record(s) were decoded into NURBS carriers from their inline \
             cached 3D B-spline block.",
                s.nurbs_curves
            ),
            "collect F3D geometry losses",
            "retain F3D geometry loss",
        )?;
    }
    if s.missing_face_surfaces() > 0 {
        push_loss_vec(ctx, &mut losses, F3dLossCode::FaceSurfaceReferenceDangling, format_args!(
            "{} face(s) were omitted because their required surface reference was null or dangling. Reference conditions: {}.",
            s.missing_face_surfaces(),
            KindCounts(&s.missing_face_surface_kinds)
        ), "collect F3D geometry losses", "retain F3D geometry loss")?;
    }
    if s.unknown_surface_faces() > 0 {
        push_loss_vec(
            ctx,
            &mut losses,
            F3dLossCode::SurfaceShapeNotDecoded,
            format_args!(
                "{} face(s) rest on spline/procedural surfaces whose shape was not decoded into a \
             typed carrier (no inline cached B-spline block: the cache is reached through a \
             subtype reference, or the record is a procedural form this codec does not \
             evaluate); the face, its loops, and trims are emitted with an unknown-geometry \
             surface linking to the preserved record bytes. Topology is transferred; the \
             underlying surface shape is not. Native kinds: {}.",
                s.unknown_surface_faces(),
                KindCounts(&s.unknown_surface_kinds)
            ),
            "collect F3D geometry losses",
            "retain F3D geometry loss",
        )?;
    }
    if s.mesh_surface_faces > 0 {
        push_loss_vec(ctx, &mut losses, F3dLossCode::MeshSurfaceSentinel, format_args!(
            "{} face(s) use zero-payload mesh_surface sentinels. Their exact surfaces are absent by definition; the emitted unknown surface preserves that distinction from tessellation attributes.",
            s.mesh_surface_faces
        ), "collect F3D geometry losses", "retain F3D geometry loss")?;
    }
    if s.procedural_curve_edges() > 0 {
        push_loss_vec(
            ctx,
            &mut losses,
            F3dLossCode::ProceduralCurveUndecoded,
            format_args!(
            "{} edge(s) reference a procedural intcurve/spline 3D curve with no decodable inline \
             B-spline cache; the edge was emitted with its vertices and parameter range but no \
             attributed curve carrier. Native kinds: {}.",
            s.procedural_curve_edges(),
            KindCounts(&s.procedural_curve_kinds)
        ),
            "collect F3D geometry losses",
            "retain F3D geometry loss",
        )?;
    }
    if s.undecoded_pcurve_refs() > 0 {
        push_loss_vec(
            ctx,
            &mut losses,
            F3dLossCode::PcurveUndecoded,
            format_args!(
                "{} coedge(s) carry an explicit UV pcurve reference with no decodable 2D \
             carrier on the face surface's parameterization; those coedges were emitted \
             without a pcurve. Native kinds: {}.",
                s.undecoded_pcurve_refs(),
                KindCounts(&s.undecoded_pcurve_kinds)
            ),
            "collect F3D geometry losses",
            "retain F3D geometry loss",
        )?;
    }
    if s.partial_procedural_supports > 0 {
        push_loss_vec(ctx, &mut losses, F3dLossCode::BlendSupportPartial, format_args!(
            "{} rolling-ball blend definition(s) retain their signed radius and solved cache, but only one of two native supports resolved.",
            s.partial_procedural_supports
        ), "collect F3D geometry losses", "retain F3D geometry loss")?;
    }
    if s.other_records() > 0 {
        push_loss_vec(
            ctx,
            &mut losses,
            F3dLossCode::SolvedRecordUntyped,
            format_args!(
                "{} solved-record application/refinement record(s) were not transferred: {}.",
                s.other_records(),
                KindCounts(&s.other_record_kinds)
            ),
            "collect F3D geometry losses",
            "retain F3D geometry loss",
        )?;
    }
    push_loss_vec(
        ctx,
        &mut losses,
        F3dLossCode::MaterialNotTransferred,
        format_args!(
            "Materials/appearances (.protein assets, ACT/design assignments) were not \
         transferred."
        ),
        "collect F3D geometry losses",
        "retain F3D geometry loss",
    )?;
    Ok(losses)
}

struct MetadataIr {
    ir: CadIr,
    source_attributes: std::collections::BTreeMap<String, String>,
    unknowns: Vec<UnknownRecord>,
}

fn append_metadata_unknown(
    ctx: &DecodeContext<'_>,
    unknowns: &mut Vec<UnknownRecord>,
    brep: &BrepFacts,
) -> Result<(), CodecError> {
    let id_text = crate::ids::native_scoped_id(ctx, &brep.name, "unknown", 0_u64)?;
    let id = UnknownId::mint(id_text).map_err(|error| {
        CodecError::malformed(format_args!(
            "F3D BREP name cannot form an unknown-record identity: {error}"
        ))
    })?;
    let digest =
        ctx.copy_retained_text(brep.sha256.as_str(), "retain F3D unavailable BREP digest")?;
    ctx.push_vec(
        unknowns,
        UnknownRecord::unavailable(id, 0, brep.uncompressed_len, digest, Vec::new()),
        "collect F3D metadata unknowns",
    )
}

fn build_metadata_ir(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan,
) -> Result<MetadataIr, CodecError> {
    let mut ir = CadIr::empty();
    let mut unknowns = Vec::new();

    let mut attributes = std::collections::BTreeMap::new();
    if let Some(folder) = scan.design_asset_folder() {
        {
            let copy = ctx.copy_retained_text(folder, "retain F3D source attribute value")?;
            ctx.insert_btree_map(
                &mut attributes,
                "asset_folder".to_owned(),
                copy,
                "collect F3D source attributes",
            )?;
        }
    }
    ctx.insert_btree_map(
        &mut attributes,
        "zip_entry_count".to_owned(),
        scan.entries.len().to_string(),
        "collect F3D source attributes",
    )
    .map(|_| ())?;
    if let Some(unit) = crate::design::decode::units::decode_document_length_unit(ctx, scan)? {
        ctx.insert_btree_map(
            &mut attributes,
            "modeling_length_unit".to_owned(),
            unit,
            "collect F3D source attributes",
        )
        .map(|_| ())?;
    }

    if let Some(brep) = container::select_fallback_brep(ctx, scan)? {
        {
            let copy = ctx.copy_retained_text(&brep.name, "retain F3D source attribute value")?;
            ctx.insert_btree_map(
                &mut attributes,
                "active_brep".to_owned(),
                copy,
                "collect F3D source attributes",
            )?;
        }
        {
            let copy =
                ctx.copy_retained_text(brep.sha256.as_str(), "retain F3D source attribute value")?;
            ctx.insert_btree_map(
                &mut attributes,
                "active_brep_sha256".to_owned(),
                copy,
                "collect F3D source attributes",
            )?;
        }
        if let Some(off) = brep
            .kernel
            .as_ref()
            .and_then(crate::container::KernelFraming::solved_record_limit)
        {
            ctx.insert_btree_map(
                &mut attributes,
                "solved_record_len".to_owned(),
                off.to_string(),
                "collect F3D source attributes",
            )
            .map(|_| ())?;
        }
        if let Some(h) = brep
            .kernel
            .as_ref()
            .and_then(crate::container::KernelFraming::model_metadata)
        {
            if let Some(pf) = &h.product_family {
                {
                    let copy = ctx.copy_retained_text(pf, "retain F3D source attribute value")?;
                    ctx.insert_btree_map(
                        &mut attributes,
                        "product_family".to_owned(),
                        copy,
                        "collect F3D source attributes",
                    )?;
                }
            }
            if let Some(pv) = &h.product_version {
                {
                    let copy = ctx.copy_retained_text(pv, "retain F3D source attribute value")?;
                    ctx.insert_btree_map(
                        &mut attributes,
                        "product_version".to_owned(),
                        copy,
                        "collect F3D source attributes",
                    )?;
                }
            }
            if let Some(sd) = &h.save_date {
                {
                    let copy = ctx.copy_retained_text(sd, "retain F3D source attribute value")?;
                    ctx.insert_btree_map(
                        &mut attributes,
                        "save_date".to_owned(),
                        copy,
                        "collect F3D source attributes",
                    )?;
                }
            }
            if let (Some(resabs), Some(resnor)) = (h.linear, h.angular) {
                ir.tolerances = admit_kernel_tolerances(resabs, resnor)?;
            }
        }

        append_metadata_unknown(ctx, &mut unknowns, brep)?;
    }

    Ok(MetadataIr {
        ir,
        source_attributes: attributes,
        unknowns,
    })
}

/// Build geometry and topology loss notes from the container state.
///
/// The report names the BREP carrier state. A failed binary decode gets a
/// decode-failure note. Each remaining state gets its own loss description.
fn container_losses(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan,
) -> Result<Vec<cadmpeg_ir::report::loss::LossNote>, CodecError> {
    let brep_count = count_result_items(
        ctx,
        container::design_breps(ctx, scan)?,
        "count F3D Design BREP candidates",
    )?;
    let selected = container::select_fallback_brep(ctx, scan)?;
    let text_count = count_result_items(
        ctx,
        container::text_brep_names(ctx, scan)?,
        "count F3D text BREP carriers",
    )?;

    let (geometry, topology) = match (brep_count, selected) {
        // The text carrier is present but its decode produced no geometry.
        (0, _) if text_count != 0 => {
            let text_names = join_text_brep_names(ctx, scan)?;
            (
                ctx.format_retained(format_args!(
                "ASM BREP geometry was not transferred: the document's only geometry carrier is \
                 the text-encoded ASM stream(s) `{text_names}`, and their decode produced no surfaces, \
                 curves, or points."
            ), "report F3D text geometry loss")?,
                ctx.format_retained(format_args!(
                "B-rep topology graph (body/region/shell/face/loop/coedge/edge/vertex) was not \
                 built from the text-encoded carrier(s) `{text_names}`."
            ), "report F3D text topology loss")?,
            )
        }
        (0, _) => (
            "ASM BREP geometry was not transferred: the container declares no ASM BREP stream, so \
             no surfaces, curves, or points were produced."
                .to_string(),
            "B-rep topology graph (body/region/shell/face/loop/coedge/edge/vertex) was not built: \
             the container declares no ASM BREP stream."
                .to_string(),
        ),
        (_, Some(brep)) => (
            ctx.format_retained(
                format_args!(
                    "ASM BREP geometry was not transferred: the selected stream `{}` is not a \
                 decodable BinaryFile4/BinaryFile8 SAB (or its framing failed). {brep_count} BREP \
                 stream(s) were located, but no surfaces, curves, or points were produced.",
                    brep.name
                ),
                "report F3D selected geometry loss",
            )?,
            ctx.format_retained(
                format_args!(
                "B-rep topology graph (body/region/shell/face/loop/coedge/edge/vertex) was not \
                 built for the selected stream `{}`.",
                brep.name
            ),
                "report F3D selected topology loss",
            )?,
        ),
        (_, None) => (
            format!(
                "ASM BREP geometry was not transferred: {brep_count} BREP stream(s) were located, \
                 but none of them is the document's geometry stream. The Design body map that \
                 binds a body to its blob was not read, so the selection is ambiguous."
            ),
            "B-rep topology graph (body/region/shell/face/loop/coedge/edge/vertex) was not built: \
             no BREP stream was selected."
                .to_string(),
        ),
    };

    let mut losses = vec![
        F3dLossCode::GeometryNotTransferred.note(geometry),
        F3dLossCode::TopologyNotTransferred.note(topology),
        F3dLossCode::MaterialNotTransferred.note(
            "Materials/appearances (.protein assets, ACT/design assignments) were not \
             transferred.",
        ),
    ];

    // An absent carrier and an unselectable carrier produce different findings.
    // Full decode rejects an ambiguous selection before it builds this report.
    if selected.is_none() {
        ctx.push_vec(&mut losses, F3dLossCode::MissingGeometryStream.note(
            if brep_count == 0 && text_count != 0 {
                format!(
                    "{text_count} ASM BREP stream(s) are present in the text encoding (.sat/.smt) and \
                     produced no geometry; no binary stream (.smb/.smbh) was found"
                )
            } else if brep_count == 0 {
                "no ASM BREP stream (.smb/.smbh) was found in the container".to_string()
            } else {
                format!(
                    "{brep_count} ASM BREP stream(s) are present, but none of them was selected as \
                     the document's geometry stream"
                )
            },
        ), "collect F3D container losses")?;
    }

    Ok(losses)
}

/// Resolve the appearance loss note against the appearances in the IR.
///
/// The report adds this note before appearance decoding. This function removes
/// it when the IR carries a complete document-local catalog and keeps it when a
/// serialized assignment failed to resolve.
pub(crate) fn reconcile_appearance_loss(
    ctx: &DecodeContext<'_>,
    report: &mut DecodeBody,
    ir: &CadIr,
    has_topology_assignments: bool,
) -> Result<(), CodecError> {
    if ir.model.appearances.is_empty() {
        return Ok(());
    }
    if has_topology_assignments && ir.model.appearance_bindings.is_empty() {
        if let Some(index) = ctx.position_by(
            &report.losses,
            |loss| Ok(loss.code.category() == LossCategory::Material),
            "find F3D unresolved appearance loss",
        )? {
            let loss = &mut report.losses[index];
            loss.message = format!(
                "{} Protein appearance asset(s) were decoded, but no topology assignment was resolved.",
                ir.model.appearances.len()
            );
        }
        return Ok(());
    }
    ctx.retain_vec(
        &mut report.losses,
        |loss| Ok(loss.code.category() != LossCategory::Material),
        "drop F3D material losses",
    )?;
    Ok(())
}

/// Join per-face appearance assignments to BREP faces through the face GUID
/// carried by each face's `NEUTRON_Material_attrib_def` attribute
/// ([spec §3.2](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/f3d.md#32-materials)).
pub(crate) fn resolve_face_appearance_bindings(
    ctx: &DecodeContext<'_>,
    ir: &mut CadIr,
    face_assignments: &[materials::FaceAppearanceAssignment],
) -> Result<(), CodecError> {
    use cadmpeg_ir::appearance::{AppearanceBinding, AppearanceTarget};
    use cadmpeg_ir::attributes::{AttributeTarget, AttributeValue};
    use std::collections::btree_map::Entry;

    struct Assignment<'a> {
        visual_guid: &'a crate::records::references::DesignVisualToken,
        color: Option<cadmpeg_ir::topology::Color>,
    }

    if face_assignments.is_empty() {
        return Ok(());
    }

    let mut assignments_storage = ctx.reserve_scoped(0, "index F3D face appearance assignments")?;
    let mut assignments_by_guid = std::collections::BTreeMap::new();
    for assignment in ctx.admit_iter(face_assignments, "scan F3D face appearance assignments")? {
        assignments_storage.with_storage(|| {
            let entry = ctx.entry_btree_map(
                &mut assignments_by_guid,
                assignment.face_guid.as_str(),
                "index F3D face appearance assignments",
            )?;
            match entry {
                Entry::Vacant(entry) => {
                    entry.insert(Assignment {
                        visual_guid: &assignment.visual_guid,
                        color: assignment.color,
                    });
                }
                Entry::Occupied(mut entry) => {
                    let existing = entry.get_mut();
                    if !ctx.eq_ignore_ascii_case(
                        existing.visual_guid,
                        &assignment.visual_guid,
                        "compare F3D face appearance assignment visual tokens",
                    )? {
                        return Err(CodecError::malformed(format_args!(
                            "F3D face material GUID {} carries conflicting visual tokens",
                            assignment.face_guid
                        )));
                    }
                    match (existing.color, assignment.color) {
                        (Some(left), Some(right)) if left != right => {
                            return Err(CodecError::malformed(format_args!(
                                "F3D face material GUID {} carries conflicting neutral colors",
                                assignment.face_guid
                            )));
                        }
                        (None, Some(color)) => existing.color = Some(color),
                        _ => {}
                    }
                }
            }
            Ok::<(), CodecError>(())
        })?;
    }

    let mut faces_by_guid_storage = ctx.reserve_scoped(0, "index F3D faces by material GUID")?;
    let mut faces_by_guid =
        std::collections::BTreeMap::<&str, Vec<&cadmpeg_ir::ids::FaceId>>::new();
    let mut guid_by_face_storage = ctx.reserve_scoped(0, "index F3D face material GUIDs")?;
    let mut guid_by_face = std::collections::BTreeMap::<&cadmpeg_ir::ids::FaceId, &str>::new();
    for attribute in ctx.admit_iter(&ir.model.attributes, "scan F3D ir model attributes")? {
        let AttributeTarget::Face(face) = &attribute.target else {
            continue;
        };
        let material_name_count = ctx
            .admit_iter(&attribute.values, "scan F3D face material attribute names")?
            .filter_map(|value| match value {
                AttributeValue::String(value) => Some(value.as_str()),
                _ => None,
            })
            .filter(|value| *value == "NEUTRON_Material_attrib_def")
            .count();
        if material_name_count == 0 {
            continue;
        }
        if material_name_count != 1 {
            return Err(CodecError::Malformed(
                "F3D face material attribute repeats its attribute-definition name".into(),
            ));
        }
        let mut face_guid = None;
        for value in ctx.admit_iter(&attribute.values, "scan F3D face material GUID values")? {
            let AttributeValue::String(value) = value else {
                continue;
            };
            if !crate::bytes::is_guid_hyphenated(value) {
                continue;
            }
            if !ctx
                .admit_iter(value.as_str(), "scan F3D face material GUID case")?
                .all(|character| !character.is_ascii_uppercase())
            {
                continue;
            }
            if face_guid.is_some() {
                return Err(CodecError::Malformed(
                    "F3D face material attribute does not carry exactly one lower-case face GUID"
                        .into(),
                ));
            }
            face_guid = Some(value.as_str());
        }
        let Some(face_guid) = face_guid else {
            return Err(CodecError::Malformed(
                "F3D face material attribute does not carry exactly one lower-case face GUID"
                    .into(),
            ));
        };
        let previous = guid_by_face_storage.with_storage(|| {
            ctx.insert_btree_map(
                &mut guid_by_face,
                face,
                face_guid,
                "index F3D face material GUIDs",
            )
        })?;
        if let Some(previous) = previous {
            if !ctx.equal(previous, face_guid, "compare F3D face material GUIDs")? {
                return Err(CodecError::malformed(format_args!(
                    "F3D face {face} carries multiple material GUIDs"
                )));
            }
        }
        faces_by_guid_storage.with_storage(|| {
            let entry = ctx.entry_btree_map(
                &mut faces_by_guid,
                face_guid,
                "index F3D faces by material GUID",
            )?;
            let faces = match entry {
                Entry::Vacant(entry) => entry.insert(Vec::new()),
                Entry::Occupied(entry) => entry.into_mut(),
            };
            ctx.push_vec(faces, face, "collect F3D faces by material GUID")
        })?;
    }
    drop(guid_by_face);
    drop(guid_by_face_storage);
    for (_, faces) in ctx.admit_iter(&mut faces_by_guid, "order F3D faces by material GUID")? {
        faces_by_guid_storage.with_storage(|| {
            ctx.stable_sort_by(
                faces,
                |value| value,
                Ord::cmp,
                "sort F3D faces by material GUID",
            )?;
            ctx.dedup_vec(faces, "deduplicate F3D faces by material GUID")
        })?;
    }
    let mut bound_faces_storage = ctx.reserve_scoped(0, "index F3D bound appearance faces")?;
    let mut bound_faces = std::collections::HashMap::new();
    for binding in ctx.admit_iter(
        &ir.model.appearance_bindings,
        "scan F3D appearance bindings for face targets",
    )? {
        let AppearanceTarget::Face(face) = &binding.target else {
            continue;
        };
        bound_faces_storage.with_storage(|| {
            ctx.insert_hash_map(
                &mut bound_faces,
                face,
                &binding.appearance,
                "index F3D bound appearance faces",
            )
            .map(|_| ())
        })?;
    }
    let mut face_indices_storage = ctx.reserve_scoped(0, "index F3D appearance faces")?;
    let mut face_indices = std::collections::HashMap::new();
    for (index, face) in ctx
        .admit_iter(&ir.model.faces, "scan F3D appearance face IDs")?
        .enumerate()
    {
        let id = ctx.copy_scoped_text(
            face.id.as_str(),
            &mut face_indices_storage,
            "retain F3D face index ID",
        )?;
        face_indices_storage.with_storage(|| {
            ctx.insert_hash_map(&mut face_indices, id, index, "index F3D appearance faces")
                .map(|_| ())
        })?;
    }
    let appearance_index = materials::AppearanceIndex::new(ctx, &ir.model.appearances)?;
    let mut new_bindings = Vec::new();
    for (face_guid, assignment) in ctx.admit_iter(
        &assignments_by_guid,
        "scan F3D face appearance assignment index",
    )? {
        let Some(faces) = ctx.get_btree_map(
            &faces_by_guid,
            *face_guid,
            "find F3D faces by material GUID",
        )?
        else {
            continue;
        };
        let appearance = appearance_index.for_visual_token(
            ctx,
            &ir.model.appearances,
            assignment.visual_guid,
            None,
        )?;
        for face in ctx.admit_iter(faces, "scan F3D appearance assignment faces")? {
            if let Some(color) = assignment.color {
                if let Some(index) = ctx
                    .get_hash_map(
                        &face_indices,
                        face.as_str(),
                        "find F3D appearance face index",
                    )?
                    .copied()
                {
                    let target = &mut ir.model.faces[index];
                    if target.color.is_none() {
                        target.color = Some(color);
                    }
                }
            }
            let Some(appearance) = appearance.as_ref() else {
                continue;
            };
            if let Some(existing) =
                ctx.get_hash_map(&bound_faces, *face, "find F3D bound appearance face")?
            {
                if !ctx.equal(
                    *existing,
                    &appearance.id,
                    "compare F3D face appearance assignments",
                )? {
                    return Err(CodecError::malformed(format_args!(
                        "F3D face {face} carries conflicting appearance assignments"
                    )));
                }
                continue;
            }

            bound_faces_storage.with_storage(|| {
                ctx.insert_hash_map(
                    &mut bound_faces,
                    *face,
                    &appearance.id,
                    "index F3D new appearance faces",
                )
                .map(|_| ())
            })?;
            let target = AppearanceTarget::Face(
                face.try_clone_for_decode(ctx, "retain F3D appearance target face")?,
            );
            let appearance_id = appearance
                .id
                .try_clone_for_decode(ctx, "retain F3D face appearance ID")?;
            let id = crate::ids::face_appearance_binding_id(
                ctx,
                face_guid,
                assignment.visual_guid,
                face,
            )?;
            ctx.push_vec(
                &mut new_bindings,
                AppearanceBinding {
                    // The face id completes the key: one appearance attribute GUID
                    // reaches every face carrying it, so the assignment pair alone
                    // repeats across those faces.
                    id,
                    target,
                    appearance: appearance_id,
                    source_entity_id: None,
                    object_type: None,
                    visible: None,
                    channels: std::collections::BTreeMap::new(),
                },
                "collect F3D face appearance bindings",
            )?;
        }
    }
    drop(bound_faces);
    drop(bound_faces_storage);
    drop(face_indices);
    drop(face_indices_storage);
    drop(faces_by_guid);
    drop(faces_by_guid_storage);
    drop(assignments_by_guid);
    drop(assignments_storage);
    ctx.extend_vec(
        &mut ir.model.appearance_bindings,
        new_bindings,
        "append F3D face appearance bindings",
    )?;
    Ok(())
}

/// Fill absent explicit topology colors from uniquely bound appearance assets.
/// Native RGB/truecolor attributes remain authoritative on the same target.
fn insert_appearance_color<'a, K: Eq + std::hash::Hash + cadmpeg_core::decode::cost::DecodeCost>(
    ctx: &DecodeContext<'_>,
    colors: &mut std::collections::HashMap<&'a K, Option<cadmpeg_ir::topology::Color>>,
    id: &'a K,
    color: cadmpeg_ir::topology::Color,
    operation: &'static str,
) -> Result<(), CodecError> {
    if let Some(existing) = ctx.get_mut_hash_map(colors, id, operation)? {
        *existing = None;
    } else {
        ctx.insert_hash_map(colors, id, Some(color), operation)?;
    }
    Ok(())
}

fn apply_appearance_base_colors(ctx: &DecodeContext<'_>, ir: &mut CadIr) -> Result<(), CodecError> {
    use cadmpeg_ir::appearance::AppearanceTarget;

    let mut colors_storage = ctx.reserve_scoped(0, "index F3D appearance colors")?;
    let mut colors = std::collections::HashMap::new();
    for appearance in ctx.admit_iter(&ir.model.appearances, "scan F3D appearance colors")? {
        let Some(color) = appearance.base_color else {
            continue;
        };
        colors_storage.with_storage(|| {
            ctx.insert_hash_map(
                &mut colors,
                &appearance.id,
                color,
                "index F3D appearance colors",
            )
            .map(|_| ())
        })?;
    }
    let mut body_colors_storage = ctx.reserve_scoped(0, "index F3D body appearance colors")?;
    let mut body_colors = std::collections::HashMap::new();
    let mut face_colors_storage = ctx.reserve_scoped(0, "index F3D face appearance colors")?;
    let mut face_colors = std::collections::HashMap::new();
    for binding in ctx.admit_iter(
        &ir.model.appearance_bindings,
        "scan F3D ir model appearance bindings",
    )? {
        let Some(color) = ctx
            .get_hash_map(&colors, &binding.appearance, "find F3D appearance color")?
            .copied()
        else {
            continue;
        };
        match &binding.target {
            AppearanceTarget::Body(id) => body_colors_storage.with_storage(|| {
                insert_appearance_color(
                    ctx,
                    &mut body_colors,
                    id,
                    color,
                    "index F3D body appearance colors",
                )
            })?,
            AppearanceTarget::Face(id) => face_colors_storage.with_storage(|| {
                insert_appearance_color(
                    ctx,
                    &mut face_colors,
                    id,
                    color,
                    "index F3D face appearance colors",
                )
            })?,
            _ => {}
        }
    }
    drop(colors);
    drop(colors_storage);
    for body in &mut ir.model.bodies {
        if body.color.is_none() {
            body.color = ctx
                .get_hash_map(&body_colors, &body.id, "find F3D body appearance color")?
                .copied()
                .flatten();
        }
    }
    for face in &mut ir.model.faces {
        if face.color.is_none() {
            face.color = ctx
                .get_hash_map(&face_colors, &face.id, "find F3D face appearance color")?
                .copied()
                .flatten();
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;
