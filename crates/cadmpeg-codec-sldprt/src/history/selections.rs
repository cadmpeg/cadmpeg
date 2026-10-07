// SPDX-License-Identifier: Apache-2.0
//! Bind native topology selections to decoded B-rep identities.

use crate::brep::feature_source::FeatureSourceId;
use crate::records::{FeatureHistory, FeatureInputSurfaceSelection};
use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::CodecError;
use cadmpeg_ir::geometry::{Curve, SolvedSurfaceGeometry, Surface, SurfaceGeometry};
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::topology::{Body, Edge, Face};
use cadmpeg_ir::{
    features::{
        BodySelection, DatumPlaneReference, EdgeSelection, ExtrudeExtent, ExtrudeSide,
        FaceSelection, FeatureDefinition, FeatureOperation, LinearTermination, PathRef,
        PlanarProfileRef, ProfileRef,
    },
    scalar::Length,
};
use std::collections::{BTreeMap, HashMap};

use crate::history::literals::{named_literal, parse_point3_mm, parse_vector3};
use crate::records::FeatureSource;

const EPS_SELECTIONS_RESOLVE_PLANAR_FACE_SELECTION_E9: f64 = 1e-9;
const EPS_SELECTIONS_RESOLVE_PLANAR_FACE_SELECTION_E8: f64 = 1e-8;

/// Faces bound to surface selections, by owning feature and native selection value.
type SurfaceSelectionFaceBindings =
    HashMap<String, HashMap<String, Option<cadmpeg_ir::ids::FaceId>>>;

struct FaceSelectionContext<'a> {
    ids: &'a HashMap<&'a str, Option<&'a cadmpeg_ir::ids::FaceId>>,
    feature_ref: Option<&'a str>,
    surface_selection_faces: &'a SurfaceSelectionFaceBindings,
}

pub(crate) struct TopologySelectionInputs<'a> {
    pub(crate) bodies: &'a [Body],
    pub(crate) faces: &'a [Face],
    pub(crate) surfaces: &'a [Surface],
    pub(crate) edges: &'a [Edge],
    pub(crate) curves: &'a [Curve],
    pub(crate) lanes: &'a [crate::records::FeatureInputLane],
    pub(crate) face_identities:
        &'a [(cadmpeg_ir::ids::FaceId, crate::brep::PersistentFaceIdentity)],
}

const SELECTION_IDENTITY: &str = "retain SLDPRT topology selection identity";

const SURFACE_COMPONENT_SELECTION_PREFIX: &str = "sldprt:feature-input:surface-component-ids";

/// Resolve the support origin represented by a frame-backed offset reference.
/// An explicit reference-face origin takes precedence. A surface-component
/// selection stores the resulting plane origin, so its support is one signed
/// `D1` displacement along the stored normal.
fn offset_plane_support_origin(
    ctx: &DecodeContext<'_>,
    source_properties: &BTreeMap<cadmpeg_core::text::NonBlankString, String>,
    native: Option<&str>,
    fallback_origin: Point3,
    normal: Vector3,
    distance: Length,
) -> Result<Point3, CodecError> {
    const OPERATION: &str = "resolve SLDPRT offset plane support origin";
    if let Some(origin) = named_literal(ctx, source_properties, "ReferenceFaceOrigin", OPERATION)?
        .and_then(parse_point3_mm)
    {
        return Ok(origin.get());
    }
    let origin = named_literal(ctx, source_properties, "Origin", OPERATION)?
        .and_then(parse_point3_mm)
        .map_or(fallback_origin, cadmpeg_ir::features::FinitePoint3::get);
    if native.is_some_and(|native| native.starts_with(SURFACE_COMPONENT_SELECTION_PREFIX)) {
        return Ok(Point3::new(
            origin.x + normal.x * distance.get(),
            origin.y + normal.y * distance.get(),
            origin.z + normal.z * distance.get(),
        ));
    }
    Ok(origin)
}

fn surface_selection_face_bindings<'a>(
    ctx: &DecodeContext<'_>,
    selections: impl IntoIterator<Item = &'a FeatureInputSurfaceSelection>,
    feature_sources: &HashMap<&str, Option<FeatureSourceId>>,
    face_identities: &[(cadmpeg_ir::ids::FaceId, crate::brep::PersistentFaceIdentity)],
) -> Result<SurfaceSelectionFaceBindings, CodecError> {
    const OPERATION: &str = "index SLDPRT topology selections";
    let mut faces_by_identity = HashMap::new();
    for (target, identity) in ctx.admit_iter(
        face_identities,
        "scan SLDPRT surface_selection_face_bindings values",
    )? {
        let key = (identity.feature_source_id, identity.local_id);
        match ctx.get_mut_hash_map(&mut faces_by_identity, &key, OPERATION)? {
            Some(entry) => {
                if let Some(existing) = *entry {
                    if !ctx.equal(existing, target, "compare SLDPRT topology selections")? {
                        *entry = None;
                    }
                }
            }
            None => {
                ctx.insert_hash_map(&mut faces_by_identity, key, Some(target), OPERATION)?;
            }
        }
    }
    let mut bindings = SurfaceSelectionFaceBindings::new();
    let mut selections = selections.into_iter();
    while let Some(selection) = ctx.next_charged(&mut selections, "bind SLDPRT topology selections")? {
        let candidate = selection
            .components
            .last()
            .map(|component| {
                let feature_source_id = match match selection.terminal_feature_ref.as_deref() {
                    Some(terminal) => ctx
                        .get_hash_map(feature_sources, terminal, "look up SLDPRT hash key")?
                        .copied()
                        .flatten(),
                    None => View::u32_le_at(&component.type_signature, 4)
                        .and_then(|source| FeatureSourceId::try_from(source).ok()),
                } {
                    Some(value) => value,
                    None => return Ok::<_, cadmpeg_core::CodecError>(None),
                };
                Ok::<_, cadmpeg_core::CodecError>(
                    ctx.get_hash_map(
                        &faces_by_identity,
                        &(
                            feature_source_id,
                            match component.local_id {
                                Some(value) => value,
                                None => return Ok::<_, cadmpeg_core::CodecError>(None),
                            },
                        ),
                        "look up SLDPRT hash key",
                    )?
                    .copied()
                    .flatten(),
                )
            })
            .transpose()?
            .flatten()
            .map(|face| face.try_clone_for_decode(ctx, "retain SLDPRT topology selection identity"))
            .transpose()?;
        let native = crate::resolved_features::terminations::compact_surface_selection_value(
            ctx,
            &selection.components,
        )?;
        let owner_bindings = match ctx.get_mut_hash_map(
            &mut bindings,
            selection.feature_ref.as_str(),
            "index SLDPRT topology selections",
        )? {
            Some(owner_bindings) => owner_bindings,
            None => ctx
                .entry_hash_map(
                    &mut bindings,
                    ctx.copy_retained_text(&selection.feature_ref, SELECTION_IDENTITY)?,
                    "index SLDPRT topology selections",
                )?
                .or_default(),
        };
        match ctx.entry_hash_map(owner_bindings, native, "index SLDPRT topology selections")? {
            std::collections::hash_map::Entry::Vacant(entry) => {
                entry.insert(candidate);
            }
            std::collections::hash_map::Entry::Occupied(mut entry) => {
                if !ctx.equal(
                    entry.get(),
                    &candidate,
                    "compare SLDPRT topology selections",
                )? {
                    entry.insert(None);
                }
            }
        }
    }
    Ok(bindings)
}

fn extrude_extent_sides_mut(extent: &mut ExtrudeExtent) -> [Option<&mut ExtrudeSide>; 2] {
    match extent {
        ExtrudeExtent::OneSided { side } | ExtrudeExtent::Symmetric { side } => [Some(side), None],
        ExtrudeExtent::TwoSided { first, second } => [Some(first), Some(second)],
    }
}

pub(crate) fn bind_topology_selections(
    ctx: &DecodeContext<'_>,
    features: &mut [cadmpeg_ir::features::Feature],
    histories: &[FeatureHistory],
    inputs: &TopologySelectionInputs<'_>,
) -> Result<(), cadmpeg_core::CodecError> {
    let bodies = inputs.bodies;
    let faces = inputs.faces;
    let surfaces = inputs.surfaces;
    let edges = inputs.edges;
    let curves = inputs.curves;
    let lanes = inputs.lanes;
    let face_identities = inputs.face_identities;
    const OPERATION: &str = "bind SLDPRT topology selections";
    let mut scratch = ctx.reserve_scoped(0, "index SLDPRT topology selections")?;
    let (body_ids, face_ids, edge_ids, curve_ids, surfaces_by_id, records) =
        scratch.with_storage(|| {
            let body_ids = selection_ids(ctx, bodies, |body| {
                (body.id.as_str(), body.name.as_deref(), &body.id)
            })?;
            let face_ids = selection_ids(ctx, faces, |face| {
                (face.id.as_str(), face.name.as_deref(), &face.id)
            })?;
            let edge_ids = selection_ids(ctx, edges, |edge| (edge.id.as_str(), None, &edge.id))?;
            let curve_ids =
                selection_ids(ctx, curves, |curve| (curve.id.as_str(), None, &curve.id))?;
            let mut surfaces_by_id = HashMap::new();
            for surface in ctx.admit_iter(surfaces, "scan SLDPRT surfaces values")? {
                ctx.insert_hash_map(
                    &mut surfaces_by_id,
                    &surface.id,
                    surface,
                    "index SLDPRT topology selections",
                )?;
            }
            // The first native record bearing each identity.
            let mut records = HashMap::new();
            for history in ctx.admit_iter(histories, "scan SLDPRT topology selection histories")? {
                for record in
                    ctx.admit_iter(&history.features, "scan SLDPRT topology selection records")?
                {
                    if !ctx.contains_key_hash_map(&records, record.id.as_str(), OPERATION)? {
                        ctx.insert_hash_map(&mut records, record.id.as_str(), record, OPERATION)?;
                    }
                }
            }
            Ok::<_, CodecError>((
                body_ids,
                face_ids,
                edge_ids,
                curve_ids,
                surfaces_by_id,
                records,
            ))
        })?;
    let surface_selection_faces = scratch.with_storage(|| {
        let feature_sources = history_feature_sources(ctx, histories, lanes)?;
        surface_selection_face_bindings(
            ctx,
            ctx.admit_iter(lanes, OPERATION)?.flat_map(|lane| lane.surface_selections.iter()),
            &feature_sources,
            face_identities,
        )
    })?;
    for feature in ctx.admit_iter(features, OPERATION)? {
        let scope = match feature.native_ref.as_deref() {
            Some(native_ref) => match ctx.get_hash_map(&records, native_ref, OPERATION)? {
                Some(record) => ctx.get_btree_map(&record.properties, "Scope", OPERATION)?,
                None => None,
            },
            None => None,
        };
        if let Some(scope) = scope {
            if let Some(outputs) =
                resolve_ids(ctx, scope, &body_ids, |id: &cadmpeg_ir::ids::BodyId| {
                    id.try_clone_for_decode(ctx, SELECTION_IDENTITY)
                })?
            {
                feature
                    .evaluation
                    .set_outputs(cadmpeg_ir::features::DistinctMembers::try_from(
                        outputs, ctx,
                    )?);
            }
        }
        let source_properties = &feature.source_properties;
        let feature_native_ref = feature.native_ref.as_deref();
        let mut edited: Result<(), CodecError> = Ok(());
        feature.evaluation.edit(|definition, _outputs| {
            edited = (|| {
                'feature_edit: {
                    let face_selection_context = FaceSelectionContext {
                        ids: &face_ids,
                        feature_ref: feature_native_ref,
                        surface_selection_faces: &surface_selection_faces,
                    };
                    let resolve_face = |selection: &mut FaceSelection| {
                        resolve_face_selection(ctx, selection, &face_selection_context)
                    };
                    match definition {
                        FeatureDefinition::Operation(FeatureOperation::DatumOffsetPlane {
                            reference,
                            distance,
                        }) => match reference {
                            Some(DatumPlaneReference::Face { face: reference }) => {
                                resolve_face(reference)?;
                            }
                            Some(DatumPlaneReference::ResolvedPlane { frame }) => {
                                let origin = frame.origin();
                                let normal = frame.normal().get();
                                let native = ctx
                                    .get_btree_map(
                                        source_properties,
                                        "ReferenceFaceNative",
                                        OPERATION,
                                    )?
                                    .map(String::as_str);
                                let support_origin = offset_plane_support_origin(
                                    ctx,
                                    source_properties,
                                    native,
                                    origin.get(),
                                    normal,
                                    *distance,
                                )?;
                                let mut face = match native {
                                    Some(native) => FaceSelection::Native(
                                        ctx.copy_retained_text(native, SELECTION_IDENTITY)?,
                                    ),
                                    None => FaceSelection::Unresolved,
                                };
                                resolve_offset_plane_face_selection(
                                    ctx,
                                    &mut face,
                                    support_origin,
                                    normal,
                                    &face_selection_context,
                                    faces,
                                    &surfaces_by_id,
                                )?;
                                if !matches!(face, FaceSelection::Unresolved) {
                                    *reference = Some(DatumPlaneReference::Face { face });
                                }
                            }
                            None => {
                                let Some(origin) =
                                    named_literal(ctx, source_properties, "Origin", OPERATION)?
                                        .and_then(parse_point3_mm)
                                        .map(cadmpeg_ir::features::FinitePoint3::get)
                                else {
                                    break 'feature_edit;
                                };
                                let Some(normal) =
                                    named_literal(ctx, source_properties, "Normal", OPERATION)?
                                        .and_then(parse_vector3)
                                else {
                                    break 'feature_edit;
                                };
                                let mut face = FaceSelection::Unresolved;
                                resolve_planar_face_selection(
                                    ctx,
                                    &mut face,
                                    origin,
                                    normal,
                                    faces,
                                    &surfaces_by_id,
                                )?;
                                if !matches!(face, FaceSelection::Unresolved) {
                                    *reference = Some(DatumPlaneReference::Face { face });
                                }
                            }
                            Some(DatumPlaneReference::Feature { .. }) => {}
                        },
                        FeatureDefinition::Operation(FeatureOperation::Extrude {
                            profile,
                            extent,
                            ..
                        }) => {
                            resolve_profile_ref(ctx, profile, &face_ids)?;
                            for side in extrude_extent_sides_mut(extent).into_iter().flatten() {
                                if let LinearTermination::ToFace { face, .. }
                                | LinearTermination::OffsetFromFace { face, .. } =
                                    &mut side.termination
                                {
                                    resolve_face(face)?;
                                }
                            }
                        }
                        FeatureDefinition::Operation(FeatureOperation::Revolve {
                            construction,
                            ..
                        }) => {
                            if let Some(profile) = construction.profile_mut() {
                                resolve_planar_profile_ref(ctx, profile, &face_ids)?;
                            }
                        }
                        FeatureDefinition::Operation(FeatureOperation::Rib {
                            construction,
                            ..
                        }) => {
                            if let Some(profile) = &mut construction.profile {
                                resolve_planar_profile_ref(ctx, profile, &face_ids)?;
                            }
                        }
                        FeatureDefinition::Operation(FeatureOperation::Sweep {
                            shape,
                            path,
                            ..
                        }) => {
                            if let Some(profile) = shape.referenced_profile_mut() {
                                resolve_planar_profile_ref(ctx, profile, &face_ids)?;
                            }
                            if let Some(path) = path {
                                resolve_path_ref(ctx, path, &edge_ids, &curve_ids)?;
                            }
                        }
                        FeatureDefinition::Operation(FeatureOperation::Loft {
                            sections,
                            guidance,
                            ..
                        }) => {
                            for section in ctx.admit_iter(&mut sections[..], OPERATION)? {
                                if let cadmpeg_ir::features::LoftSection::Profile(profile) = section
                                {
                                    resolve_profile_ref(ctx, profile, &face_ids)?;
                                }
                            }
                            match guidance {
                                cadmpeg_ir::features::LoftGuidance::Guides(guides) => {
                                    for path in ctx.admit_iter(&mut guides[..], OPERATION)? {
                                        resolve_path_ref(ctx, path, &edge_ids, &curve_ids)?;
                                    }
                                }
                                cadmpeg_ir::features::LoftGuidance::Centerline(centerline) => {
                                    resolve_path_ref(ctx, centerline, &edge_ids, &curve_ids)?;
                                }
                            }
                        }
                        FeatureDefinition::Operation(FeatureOperation::Fillet { groups }) => {
                            for group in ctx.admit_iter(&mut groups[..], OPERATION)? {
                                resolve_edge_selection(ctx, &mut group.edges, &edge_ids)?;
                            }
                        }
                        FeatureDefinition::Operation(FeatureOperation::Chamfer {
                            groups, ..
                        }) => {
                            for group in ctx.admit_iter(&mut groups[..], OPERATION)? {
                                resolve_edge_selection(ctx, &mut group.edges, &edge_ids)?;
                            }
                        }
                        FeatureDefinition::Operation(FeatureOperation::Shell {
                            removed_faces,
                            ..
                        }) => {
                            resolve_face(removed_faces)?;
                        }
                        FeatureDefinition::Operation(FeatureOperation::Thicken {
                            faces, ..
                        }) => {
                            resolve_face(faces)?;
                        }
                        FeatureDefinition::Operation(FeatureOperation::OffsetSurface {
                            faces,
                            ..
                        }) => {
                            resolve_face(faces)?;
                        }
                        FeatureDefinition::Operation(FeatureOperation::KnitSurface {
                            faces,
                            ..
                        }) => {
                            resolve_face(faces)?;
                        }
                        FeatureDefinition::Operation(FeatureOperation::FilledSurface {
                            boundary,
                            support_faces,
                            ..
                        }) => {
                            if let cadmpeg_ir::features::SurfaceBoundary::Edges(edges) = boundary {
                                resolve_edge_selection(ctx, edges, &edge_ids)?;
                            }
                            resolve_face(support_faces)?;
                        }
                        FeatureDefinition::Operation(FeatureOperation::TrimSurface {
                            faces,
                            tool,
                            ..
                        }) => {
                            resolve_face(faces)?;
                            resolve_path_ref(ctx, tool, &edge_ids, &curve_ids)?;
                        }
                        FeatureDefinition::Operation(FeatureOperation::ExtendSurface {
                            faces,
                            ..
                        }) => {
                            resolve_face(faces)?;
                        }
                        FeatureDefinition::Operation(FeatureOperation::RuledSurface {
                            edges,
                            support_faces,
                            ..
                        }) => {
                            resolve_edge_selection(ctx, edges, &edge_ids)?;
                            resolve_face(support_faces)?;
                        }
                        FeatureDefinition::Operation(FeatureOperation::Draft {
                            faces,
                            anchor,
                            ..
                        }) => {
                            resolve_face(faces)?;
                            match anchor {
                                cadmpeg_ir::features::DraftAnchor::NeutralPlane {
                                    plane, ..
                                } => {
                                    resolve_face(plane)?;
                                }
                                cadmpeg_ir::features::DraftAnchor::PartingLine { tool, .. } => {
                                    resolve_face(tool)?;
                                }
                            }
                        }
                        FeatureDefinition::Operation(FeatureOperation::Combine {
                            operands,
                            ..
                        }) => {
                            let empty = cadmpeg_ir::features::CombineOperands::new(
                                BodySelection::Unresolved,
                                BodySelection::Unresolved,
                                ctx,
                            )?
                            .map_err(CodecError::malformed)?;
                            let (mut target, mut tools) =
                                std::mem::replace(operands, empty).into_parts();
                            resolve_body_selection(ctx, &mut target, &body_ids)?;
                            resolve_body_selection(ctx, &mut tools, &body_ids)?;
                            *operands =
                                cadmpeg_ir::features::CombineOperands::new(target, tools, ctx)?
                                    .map_err(CodecError::malformed)?;
                        }
                        FeatureDefinition::Operation(FeatureOperation::CutWithSurface {
                            targets,
                            tools,
                            ..
                        }) => {
                            resolve_body_selection(ctx, targets, &body_ids)?;
                            resolve_face(tools)?;
                        }
                        FeatureDefinition::Operation(FeatureOperation::DeleteBody {
                            bodies,
                            ..
                        }) => {
                            resolve_body_selection(ctx, bodies, &body_ids)?;
                        }
                        FeatureDefinition::Operation(FeatureOperation::Pattern {
                            pattern, ..
                        }) => {
                            if let Some(Some(path)) = pattern.curve_path_mut() {
                                resolve_path_ref(ctx, path, &edge_ids, &curve_ids)?;
                            }
                        }
                        FeatureDefinition::Operation(FeatureOperation::Scale {
                            bodies, ..
                        }) => {
                            resolve_body_selection(ctx, bodies, &body_ids)?;
                        }
                        FeatureDefinition::Operation(FeatureOperation::MoveBody {
                            bodies, ..
                        }) => {
                            resolve_body_selection(ctx, bodies, &body_ids)?;
                        }
                        FeatureDefinition::Operation(
                            FeatureOperation::DeleteFace { faces, .. }
                            | FeatureOperation::MoveFace { faces, .. }
                            | FeatureOperation::Dome { faces, .. },
                        ) => {
                            resolve_face(faces)?;
                        }
                        FeatureDefinition::Operation(FeatureOperation::ReplaceFace {
                            operands,
                        }) => {
                            let empty = cadmpeg_ir::features::ReplaceFaceOperands::new(
                                FaceSelection::Unresolved,
                                FaceSelection::Unresolved,
                                ctx,
                            )?
                            .map_err(CodecError::malformed)?;
                            let (mut targets, mut replacements) =
                                std::mem::replace(operands, empty).into_parts();
                            resolve_face(&mut targets)?;
                            resolve_face(&mut replacements)?;
                            *operands = cadmpeg_ir::features::ReplaceFaceOperands::new(
                                targets,
                                replacements,
                                ctx,
                            )?
                            .map_err(CodecError::malformed)?;
                        }
                        FeatureDefinition::Operation(FeatureOperation::Hole {
                            face: Some(face),
                            ..
                        }) => {
                            resolve_face(face)?;
                        }
                        FeatureDefinition::Operation(FeatureOperation::Wrap {
                            profile,
                            face,
                            ..
                        }) => {
                            resolve_planar_profile_ref(ctx, profile, &face_ids)?;
                            resolve_face(face)?;
                        }
                        FeatureDefinition::Operation(FeatureOperation::ProjectedCurve {
                            source,
                            target_faces,
                            ..
                        }) => {
                            resolve_path_ref(ctx, source, &edge_ids, &curve_ids)?;
                            resolve_face(target_faces)?;
                        }
                        FeatureDefinition::Operation(FeatureOperation::CompositeCurve {
                            segments,
                            ..
                        }) => {
                            for segment in ctx.admit_iter(&mut segments[..], OPERATION)? {
                                resolve_path_ref(ctx, segment, &edge_ids, &curve_ids)?;
                            }
                        }
                        _ => {}
                    }
                }
                Ok(())
            })();
        });
        edited?;
    }

    Ok(())
}

fn resolve_planar_face_selection(
    ctx: &DecodeContext<'_>,
    selection: &mut FaceSelection,
    origin: Point3,
    normal: Vector3,
    faces: &[Face],
    surfaces: &HashMap<&cadmpeg_ir::ids::SurfaceId, &Surface>,
) -> Result<(), CodecError> {
    const OPERATION: &str = "match SLDPRT planar selection faces";
    let has_native = match selection {
        FaceSelection::Unresolved => false,
        FaceSelection::Native(_) => true,
        _ => return Ok(()),
    };
    let normal_length = normal.norm();
    if !normal_length.is_finite() || normal_length <= f64::EPSILON {
        return Ok(());
    }
    let mut matching = Vec::new();
    for face in ctx.admit_iter(faces, OPERATION)? {
        let Some(surface) = ctx.get_hash_map(surfaces, &face.surface, OPERATION)? else {
            continue;
        };
        let SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(plane_surface)) =
            &surface.geometry
        else {
            continue;
        };
        let candidate_origin = plane_surface.origin();
        let candidate_normal = plane_surface.frame().axis().as_raw();
        let candidate_length = candidate_normal.norm();
        if !candidate_length.is_finite() || candidate_length <= f64::EPSILON {
            continue;
        }
        let alignment = (normal.x * candidate_normal.x
            + normal.y * candidate_normal.y
            + normal.z * candidate_normal.z)
            / (normal_length * candidate_length);
        let displacement = Vector3::new(
            origin.x - candidate_origin.x,
            origin.y - candidate_origin.y,
            origin.z - candidate_origin.z,
        );
        let separation = (displacement.x * candidate_normal.x
            + displacement.y * candidate_normal.y
            + displacement.z * candidate_normal.z)
            / candidate_length;
        if (alignment.abs() - 1.0).abs() <= EPS_SELECTIONS_RESOLVE_PLANAR_FACE_SELECTION_E9
            && separation.abs() <= EPS_SELECTIONS_RESOLVE_PLANAR_FACE_SELECTION_E8
        {
            let face = face.id.try_clone_for_decode(ctx, SELECTION_IDENTITY)?;
            ctx.push_vec(
                &mut matching,
                face,
                "collect SLDPRT topology selection identities",
            )?;
        }
    }
    if (has_native && !matching.is_empty()) || (!has_native && matching.len() == 1) {
        let old = std::mem::replace(selection, FaceSelection::Unresolved);
        *selection = match old {
            FaceSelection::Native(native) => FaceSelection::Resolved {
                faces: matching,
                native,
            },
            _ => FaceSelection::Faces(matching),
        };
    }
    Ok(())
}

fn resolve_offset_plane_face_selection(
    ctx: &DecodeContext<'_>,
    selection: &mut FaceSelection,
    origin: Point3,
    normal: Vector3,
    context: &FaceSelectionContext<'_>,
    faces: &[Face],
    surfaces: &HashMap<&cadmpeg_ir::ids::SurfaceId, &Surface>,
) -> Result<(), CodecError> {
    if !matches!(selection, FaceSelection::Native(_)) {
        return Ok(());
    }
    resolve_face_selection(ctx, selection, context)?;
    if !matches!(selection, FaceSelection::Native(_)) {
        return Ok(());
    }
    resolve_planar_face_selection(ctx, selection, origin, normal, faces, surfaces)
}

fn clone_face(
    ctx: &DecodeContext<'_>,
    id: &cadmpeg_ir::ids::FaceId,
) -> Result<cadmpeg_ir::ids::FaceId, CodecError> {
    id.try_clone_for_decode(ctx, SELECTION_IDENTITY)
}

fn resolve_planar_profile_ref(
    ctx: &DecodeContext<'_>,
    profile: &mut PlanarProfileRef,
    faces: &HashMap<&str, Option<&cadmpeg_ir::ids::FaceId>>,
) -> Result<(), CodecError> {
    if let PlanarProfileRef::Native(native) = profile {
        if let Some(ids) = resolve_ids(ctx, native, faces, |id| clone_face(ctx, id))? {
            *profile = PlanarProfileRef::Faces(ids);
        }
    }
    Ok(())
}

fn resolve_profile_ref(
    ctx: &DecodeContext<'_>,
    profile: &mut ProfileRef,
    faces: &HashMap<&str, Option<&cadmpeg_ir::ids::FaceId>>,
) -> Result<(), CodecError> {
    if let ProfileRef::Planar(profile) = profile {
        resolve_planar_profile_ref(ctx, profile, faces)?;
    }
    Ok(())
}

fn resolve_path_ref(
    ctx: &DecodeContext<'_>,
    path: &mut PathRef,
    edges: &HashMap<&str, Option<&cadmpeg_ir::ids::EdgeId>>,
    curves: &HashMap<&str, Option<&cadmpeg_ir::ids::CurveId>>,
) -> Result<(), CodecError> {
    if let PathRef::Native(native) = path {
        if let Some(ids) = resolve_ids(ctx, native, edges, |id| {
            id.try_clone_for_decode(ctx, SELECTION_IDENTITY)
        })? {
            *path = PathRef::Edges(ids);
        } else if let Some(ids) = resolve_ids(ctx, native, curves, |id| {
            id.try_clone_for_decode(ctx, SELECTION_IDENTITY)
        })? {
            *path = PathRef::Curves(ids);
        }
    }
    Ok(())
}

/// Each entity's identity, and its name when no other entity bears that name,
/// as selection tokens.
fn selection_ids<'a, T, Id: 'a>(
    ctx: &DecodeContext<'_>,
    values: &'a [T],
    project: impl Fn(&'a T) -> (&'a str, Option<&'a str>, &'a Id),
) -> Result<HashMap<&'a str, Option<&'a Id>>, CodecError> {
    const OPERATION: &str = "index SLDPRT topology selections";
    let mut ids = HashMap::new();
    for value in ctx.admit_iter(values, OPERATION)? {
        let (id, name, value) = project(value);
        ctx.insert_hash_map(&mut ids, id, Some(value), OPERATION)?;
        if let Some(name) = name.filter(|name| !name.is_empty()) {
            match ctx.get_mut_hash_map(&mut ids, name, OPERATION)? {
                Some(entry) => *entry = None,
                None => {
                    ctx.insert_hash_map(&mut ids, name, Some(value), OPERATION)?;
                }
            }
        }
    }
    Ok(ids)
}

/// The entities a comma-separated native selection names, when every token
/// names exactly one entity.
fn resolve_ids<Id>(
    ctx: &DecodeContext<'_>,
    native: &str,
    ids: &HashMap<&str, Option<&Id>>,
    clone: impl Fn(&Id) -> Result<Id, CodecError>,
) -> Result<Option<Vec<Id>>, CodecError> {
    const OPERATION: &str = "resolve SLDPRT topology selection tokens";
    let mut storage = ctx.reserve_scoped(0, OPERATION)?;
    let mut candidates = Vec::new();
    let mut rest = native;
    loop {
        let end = ctx.find_map(rest.char_indices(), |(at, character)| {
            Ok((character == ',').then_some(at))
        }, OPERATION)?;
        let (token, tail) = match end {
            Some(end) => (&rest[..end], Some(&rest[end + 1..])),
            None => (rest, None),
        };
        let token = ctx.trim_text(token, OPERATION)?;
        if !token.is_empty() {
            let Some(Some(id)) = ctx.get_hash_map(ids, token, OPERATION)? else {
                return Ok(None);
            };
            ctx.push_scoped_vec(&mut storage, &mut candidates, *id, OPERATION)?;
        }
        let Some(tail) = tail else { break; };
        rest = tail;
    }
    if candidates.is_empty() {
        return Ok(None);
    }
    Ok(Some(ctx.try_collect_retained_with(candidates, "collect SLDPRT topology selection identities", clone)?))
}

fn resolve_face_selection(
    ctx: &DecodeContext<'_>,
    selection: &mut FaceSelection,
    context: &FaceSelectionContext<'_>,
) -> Result<(), CodecError> {
    const OPERATION: &str = "resolve SLDPRT surface selection owner";
    let FaceSelection::Native(native) = selection else {
        return Ok(());
    };
    let mut faces = resolve_ids(ctx, native, context.ids, |id| clone_face(ctx, id))?;
    if faces.is_none() {
        if let Some(feature_ref) = context.feature_ref {
            let face =
                match ctx.get_hash_map(context.surface_selection_faces, feature_ref, OPERATION)? {
                    Some(owner_bindings) => {
                        ctx.get_hash_map(owner_bindings, native.as_str(), OPERATION)?
                    }
                    None => None,
                };
            if let Some(Some(face)) = face {
                let mut copied = Vec::new();
                ctx.push_vec(
                    &mut copied,
                    clone_face(ctx, face)?,
                    "collect SLDPRT topology selection identities",
                )?;
                faces = Some(copied);
            }
        }
    }
    if let Some(faces) = faces {
        let old = std::mem::replace(selection, FaceSelection::Unresolved);
        if let FaceSelection::Native(native) = old {
            *selection = FaceSelection::Resolved { faces, native };
        }
    }
    Ok(())
}

fn history_feature_sources<'a>(
    ctx: &DecodeContext<'_>,
    histories: &'a [FeatureHistory],
    lanes: &[crate::records::FeatureInputLane],
) -> Result<HashMap<&'a str, Option<FeatureSourceId>>, CodecError> {
    const OPERATION: &str = "index SLDPRT topology selections";
    let mut sources = HashMap::new();
    let mut name_storage = ctx.reserve_scoped(0, OPERATION)?;
    let mut lane_names = None;
    for history in ctx.admit_iter(histories, "scan SLDPRT feature histories")? {
        for feature in ctx.admit_iter(&history.features, "scan SLDPRT topology history features")? {
            let mut source = feature.source_id;
            if source.is_none() {
                if lane_names.is_none() {
                    lane_names = Some(crate::resolved_features::scalars::lane_object_names(
                        ctx, &mut name_storage, lanes,
                    )?);
                }
                let mut first = None;
                let mut names = lane_names.as_deref().unwrap_or_default().iter();
                while let Some(names) = ctx.next_charged(
                    &mut names, "resolve SLDPRT topology source candidates",
                )? {
                    let Some(candidate) = names.of(ctx, feature)?
                        .and_then(|name| name.object_id?.value()) else {
                        continue;
                    };
                    if first.is_some_and(|known| known != candidate) {
                        first = None;
                        break;
                    }
                    first = Some(candidate);
                }
                source = first.and_then(FeatureSource::from_value);
            }
            let source = source.and_then(FeatureSource::id);
            match ctx.get_mut_hash_map(&mut sources, feature.id.as_str(), OPERATION)? {
                Some(existing) => {
                    if *existing != source {
                        *existing = None;
                    }
                }
                None => {
                    ctx.insert_hash_map(&mut sources, feature.id.as_str(), source, OPERATION)?;
                }
            }
        }
    }
    Ok(sources)
}

fn resolve_edge_selection(
    ctx: &DecodeContext<'_>,
    selection: &mut EdgeSelection,
    ids: &HashMap<&str, Option<&cadmpeg_ir::ids::EdgeId>>,
) -> Result<(), CodecError> {
    if let EdgeSelection::Native(native) = selection {
        if let Some(edges) = resolve_ids(ctx, native, ids, |id| {
            id.try_clone_for_decode(ctx, SELECTION_IDENTITY)
        })? {
            let old = std::mem::replace(selection, EdgeSelection::Unresolved);
            if let EdgeSelection::Native(native) = old {
                *selection = EdgeSelection::Resolved { edges, native };
            }
        }
    }
    Ok(())
}

fn resolve_body_selection(
    ctx: &DecodeContext<'_>,
    selection: &mut BodySelection,
    ids: &HashMap<&str, Option<&cadmpeg_ir::ids::BodyId>>,
) -> Result<(), CodecError> {
    if let BodySelection::Native(native) = selection {
        let bodies = match resolve_ids(ctx, native, ids, |id| {
            id.try_clone_for_decode(ctx, SELECTION_IDENTITY)
        })? {
            Some(bodies) => match cadmpeg_ir::features::DistinctMembers::try_from(bodies, ctx) {
                Ok(bodies) => Some(bodies),
                Err(error @ cadmpeg_ir::features::FeatureCollectionError::Resource(_)) => {
                    return Err(error.into())
                }
                Err(_) => None,
            },
            None => None,
        };
        if let Some(bodies) = bodies {
            let old = std::mem::replace(selection, BodySelection::Unresolved);
            if let BodySelection::Native(native) = old {
                *selection = BodySelection::Resolved { bodies, native };
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod offset_plane_tests;

#[cfg(test)]
mod tests {
    use super::surface_selection_face_bindings;
    use crate::records::FeatureInputComponentPathEntry;
    use crate::records::FeatureInputSurfaceSelection;
    use std::collections::HashMap;

    fn component(feature_source_id: u32, local_face_id: u32) -> FeatureInputComponentPathEntry {
        let mut type_signature = [0; 12];
        type_signature[4..8].copy_from_slice(&feature_source_id.to_le_bytes());
        FeatureInputComponentPathEntry {
            instance: Some(0x8001),
            type_signature,
            local_id: Some(local_face_id),
        }
    }

    fn surface_selection(
        feature_ref: &str,
        components: Vec<FeatureInputComponentPathEntry>,
    ) -> FeatureInputSurfaceSelection {
        FeatureInputSurfaceSelection {
            id: "selection".into(),
            parent: "lane".into(),
            ordinal: 0,
            offset: 0,
            selector: 2,
            kind: crate::records::FeatureInputSurfaceSelectionKind::Component,
            object_name_ref: "feature".into(),
            feature_ref: feature_ref.into(),
            producer_feature_refs: Vec::new(),
            terminal_feature_ref: None,
            components,
        }
    }

    #[test]
    fn surface_selection_binds_terminal_component_identity() {
        let selection = surface_selection("feature", vec![component(47, 8), component(50, 5)]);
        let feature_sources = HashMap::new();
        let bindings = surface_selection_face_bindings(
            &cadmpeg_test_support::service_decode_context(),
            std::iter::once(&selection),
            &feature_sources,
            &[
                (
                    cadmpeg_ir::ids::FaceId::mint("test:model:entity#intermediate-face")
                        .expect("identity grammar"),
                    crate::brep::PersistentFaceIdentity {
                        feature_source_id: 47_u32.try_into().unwrap(),
                        local_id: 8,
                        trailing_fields: Vec::new(),
                    },
                ),
                (
                    cadmpeg_ir::ids::FaceId::mint("test:model:entity#terminal-face")
                        .expect("identity grammar"),
                    crate::brep::PersistentFaceIdentity {
                        feature_source_id: 50_u32.try_into().unwrap(),
                        local_id: 5,
                        trailing_fields: Vec::new(),
                    },
                ),
            ],
        )
        .unwrap();
        let key = ("feature", "sldprt:feature-input:surface-component-ids:8,5");
        assert_eq!(
            bindings
                .get(key.0)
                .and_then(|owner| owner.get(key.1))
                .cloned(),
            Some(Some(
                cadmpeg_ir::ids::FaceId::mint("test:model:entity#terminal-face")
                    .expect("identity grammar")
            ))
        );
    }

    #[test]
    fn surface_selection_keeps_ambiguous_terminal_identity_native() {
        let selection = surface_selection("feature", vec![component(50, 5)]);
        let feature_sources = HashMap::new();
        let bindings = surface_selection_face_bindings(
            &cadmpeg_test_support::service_decode_context(),
            std::iter::once(&selection),
            &feature_sources,
            &[
                (
                    cadmpeg_ir::ids::FaceId::mint("test:model:entity#first-face")
                        .expect("identity grammar"),
                    crate::brep::PersistentFaceIdentity {
                        feature_source_id: 50_u32.try_into().unwrap(),
                        local_id: 5,
                        trailing_fields: Vec::new(),
                    },
                ),
                (
                    cadmpeg_ir::ids::FaceId::mint("test:model:entity#second-face")
                        .expect("identity grammar"),
                    crate::brep::PersistentFaceIdentity {
                        feature_source_id: 50_u32.try_into().unwrap(),
                        local_id: 5,
                        trailing_fields: Vec::new(),
                    },
                ),
            ],
        )
        .unwrap();
        let key = ("feature", "sldprt:feature-input:surface-component-ids:5");
        assert_eq!(
            bindings
                .get(key.0)
                .and_then(|owner| owner.get(key.1))
                .cloned(),
            Some(None)
        );
    }

    #[test]
    fn explicit_terminal_owner_overrides_component_source() {
        let mut selection = surface_selection("feature", vec![component(47, 8), component(99, 5)]);
        selection.terminal_feature_ref = Some("terminal".into());
        let feature_sources = HashMap::from([("terminal", Some(50_u32.try_into().unwrap()))]);
        let bindings = surface_selection_face_bindings(
            &cadmpeg_test_support::service_decode_context(),
            std::iter::once(&selection),
            &feature_sources,
            &[(
                cadmpeg_ir::ids::FaceId::mint("test:model:entity#terminal-face")
                    .expect("identity grammar"),
                crate::brep::PersistentFaceIdentity {
                    feature_source_id: 50_u32.try_into().unwrap(),
                    local_id: 5,
                    trailing_fields: Vec::new(),
                },
            )],
        )
        .unwrap();
        let key = ("feature", "sldprt:feature-input:surface-component-ids:8,5");
        assert_eq!(
            bindings
                .get(key.0)
                .and_then(|owner| owner.get(key.1))
                .cloned(),
            Some(Some(
                cadmpeg_ir::ids::FaceId::mint("test:model:entity#terminal-face")
                    .expect("identity grammar")
            ))
        );
    }
}

#[cfg(test)]
mod source_index_tests {
    use super::history_feature_sources;
    use crate::history::tests::{feature, feature_input_lane};
    use crate::records::{FeatureHistory, FeatureInputName, ObjectId};
    use std::collections::BTreeMap;

    #[test]
    fn topology_source_indexes_preserve_unique_names_and_cross_lane_conflicts() {
        let mut first = feature_input_lane("first", None);
        let mut second = feature_input_lane("second", None);
        let name = |value: &str, source| FeatureInputName {
            id: value.into(), parent: "lane".into(), ordinal: 0, offset: 0,
            object_id: ObjectId::from_value(source), value: value.into(),
        };
        first.names = vec![name("unique", 10), name("repeated", 11), name("repeated", 12), name("conflict", 13)];
        second.names = vec![name("unique", 10), name("conflict", 14)];
        let history = FeatureHistory {
            id: "history".into(), part_name: None, properties: BTreeMap::new(), content: Vec::new(), configurations: Vec::new(),
            features: vec![feature("unique", None, 0), feature("repeated", None, 1), feature("conflict", None, 2), feature("explicit", Some("15"), 3)],
        };
        let histories = [history];
        let lanes = [first, second];
        let ctx = cadmpeg_test_support::service_decode_context();
        let sources = history_feature_sources(&ctx, &histories, &lanes).unwrap();
        let source = |name| sources.get(name).copied().flatten().map(super::FeatureSourceId::value);
        assert_eq!(source("unique"), Some(10));
        assert_eq!(source("repeated"), None);
        assert_eq!(source("conflict"), None);
        assert_eq!(source("explicit"), Some(15));
    }
}
