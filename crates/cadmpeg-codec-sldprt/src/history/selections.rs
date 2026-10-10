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

use crate::history::literals::{parse_point3_mm, parse_vector3};
use crate::records::FeatureSource;

const EPS_SELECTIONS_RESOLVE_PLANAR_FACE_SELECTION_E9: f64 = 1e-9;
const EPS_SELECTIONS_RESOLVE_PLANAR_FACE_SELECTION_E8: f64 = 1e-8;

type SurfaceSelectionFaceBindings = HashMap<(String, String), Option<cadmpeg_ir::ids::FaceId>>;

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

const SURFACE_COMPONENT_SELECTION_PREFIX: &str = "sldprt:feature-input:surface-component-ids";

/// Resolve the support origin represented by a frame-backed offset reference.
/// An explicit reference-face origin takes precedence. A surface-component
/// selection stores the resulting plane origin, so its support is one signed
/// `D1` displacement along the stored normal.
fn offset_plane_support_origin(
    source_properties: &BTreeMap<cadmpeg_core::text::NonBlankString, String>,
    native: Option<&str>,
    fallback_origin: Point3,
    normal: Vector3,
    distance: Length,
) -> Point3 {
    if let Some(origin) = source_properties
        .get("ReferenceFaceOrigin")
        .and_then(|value| parse_point3_mm(value))
        .map(cadmpeg_ir::features::FinitePoint3::get)
    {
        return origin;
    }
    let origin = source_properties
        .get("Origin")
        .and_then(|value| parse_point3_mm(value))
        .map_or(fallback_origin, cadmpeg_ir::features::FinitePoint3::get);
    if native.is_some_and(|native| native.starts_with(SURFACE_COMPONENT_SELECTION_PREFIX)) {
        return Point3::new(
            origin.x + normal.x * distance.get(),
            origin.y + normal.y * distance.get(),
            origin.z + normal.z * distance.get(),
        );
    }
    origin
}

fn surface_selection_face_bindings<'a>(
    ctx: &DecodeContext<'_>,
    selections: impl IntoIterator<Item = &'a FeatureInputSurfaceSelection>,
    feature_sources: &HashMap<&str, Option<FeatureSourceId>>,
    face_identities: &[(cadmpeg_ir::ids::FaceId, crate::brep::PersistentFaceIdentity)],
) -> Result<SurfaceSelectionFaceBindings, CodecError> {
    let mut faces_by_identity = HashMap::new();
    for (target, identity) in face_identities {
        reserve_selection_map(ctx, &mut faces_by_identity)?;
        let entry = faces_by_identity
            .entry((identity.feature_source_id, identity.local_id))
            .or_insert(Some(target));
        if entry.as_ref().is_some_and(|existing| *existing != target) {
            *entry = None;
        }
    }
    let mut bindings = SurfaceSelectionFaceBindings::new();
    for selection in selections {
        ctx.charge_work(1, "bind SLDPRT topology selections")?;
        let candidate = selection
            .components
            .last()
            .and_then(|component| {
                let feature_source_id = match selection.terminal_feature_ref.as_deref() {
                    Some(terminal) => feature_sources.get(terminal).copied().flatten(),
                    None => View::u32_le_at(component.type_signature.as_ref(), 4)
                        .and_then(|source| FeatureSourceId::try_from(source).ok()),
                }?;
                faces_by_identity
                    .get(&(feature_source_id, component.local_id?))
                    .copied()
                    .flatten()
            })
            .map(|face| copy_selection_id(ctx, face.as_str()))
            .transpose()?;
        let native = crate::resolved_features::terminations::compact_surface_selection_value(
            ctx,
            &selection.components,
        )?;
        let key = (copy_selection_text(ctx, &selection.feature_ref)?, native);
        reserve_selection_map(ctx, &mut bindings)?;
        match bindings.entry(key) {
            std::collections::hash_map::Entry::Vacant(entry) => {
                entry.insert(candidate);
            }
            std::collections::hash_map::Entry::Occupied(mut entry) => {
                if entry.get() != &candidate {
                    entry.insert(None);
                }
            }
        }
    }
    Ok(bindings)
}

fn extrude_extent_sides_mut(extent: &mut ExtrudeExtent) -> Vec<&mut ExtrudeSide> {
    match extent {
        ExtrudeExtent::OneSided { side } | ExtrudeExtent::Symmetric { side } => vec![side],
        ExtrudeExtent::TwoSided { first, second } => vec![first, second],
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
    let body_ids = selection_ids(
        ctx,
        bodies
            .iter()
            .map(|body| (body.id.as_str(), body.name.as_deref(), &body.id)),
    )?;
    let face_ids = selection_ids(
        ctx,
        faces
            .iter()
            .map(|face| (face.id.as_str(), face.name.as_deref(), &face.id)),
    )?;
    let edge_ids = selection_ids(
        ctx,
        edges.iter().map(|edge| (edge.id.as_str(), None, &edge.id)),
    )?;
    let curve_ids = selection_ids(
        ctx,
        curves
            .iter()
            .map(|curve| (curve.id.as_str(), None, &curve.id)),
    )?;
    let mut surfaces_by_id = HashMap::new();
    for surface in surfaces {
        reserve_selection_map(ctx, &mut surfaces_by_id)?;
        surfaces_by_id.insert(&surface.id, surface);
    }
    let feature_sources = history_feature_sources(ctx, histories, lanes)?;
    let surface_selection_faces = surface_selection_face_bindings(
        ctx,
        lanes.iter().flat_map(|lane| lane.surface_selections.iter()),
        &feature_sources,
        face_identities,
    )?;
    for feature in features {
        ctx.charge_work(1, "bind SLDPRT topology selections")?;
        for history in histories {
            ctx.charge_work(
                cadmpeg_core::decode::u64_from_index(history.features.len()),
                "find SLDPRT topology selection scope",
            )?;
        }
        if let Some(scope) = feature
            .native_ref
            .as_deref()
            .and_then(|native_ref| {
                histories
                    .iter()
                    .flat_map(|history| &history.features)
                    .find(|record| record.id == native_ref)
            })
            .and_then(|record| record.properties.get("Scope"))
        {
            if let Some(outputs) =
                resolve_ids(ctx, scope, &body_ids, cadmpeg_ir::ids::BodyId::as_str)?
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
                                let native = source_properties
                                    .get("ReferenceFaceNative")
                                    .map(String::as_str);
                                let support_origin = offset_plane_support_origin(
                                    source_properties,
                                    native,
                                    origin.get(),
                                    normal,
                                    *distance,
                                );
                                let mut face = match native {
                                    Some(native) => {
                                        FaceSelection::Native(copy_selection_text(ctx, native)?)
                                    }
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
                                let Some(origin) = source_properties
                                    .get("Origin")
                                    .and_then(|value| parse_point3_mm(value))
                                    .map(cadmpeg_ir::features::FinitePoint3::get)
                                else {
                                    break 'feature_edit;
                                };
                                let Some(normal) = source_properties
                                    .get("Normal")
                                    .and_then(|value| parse_vector3(value))
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
                            for side in extrude_extent_sides_mut(extent) {
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
                            for section in sections {
                                if let cadmpeg_ir::features::LoftSection::Profile(profile) = section
                                {
                                    resolve_profile_ref(ctx, profile, &face_ids)?;
                                }
                            }
                            match guidance {
                                cadmpeg_ir::features::LoftGuidance::Guides(guides) => {
                                    for path in guides {
                                        resolve_path_ref(ctx, path, &edge_ids, &curve_ids)?;
                                    }
                                }
                                cadmpeg_ir::features::LoftGuidance::Centerline(centerline) => {
                                    resolve_path_ref(ctx, centerline, &edge_ids, &curve_ids)?;
                                }
                            }
                        }
                        FeatureDefinition::Operation(FeatureOperation::Fillet { groups }) => {
                            for group in groups {
                                resolve_edge_selection(ctx, &mut group.edges, &edge_ids)?;
                            }
                        }
                        FeatureDefinition::Operation(FeatureOperation::Chamfer {
                            groups, ..
                        }) => {
                            for group in groups {
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
                            for segment in segments {
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
    let has_native = match selection {
        FaceSelection::Unresolved => false,
        FaceSelection::Native(_) => true,
        _ => return Ok(()),
    };
    let normal_length = normal.norm();
    if !normal_length.is_finite() || normal_length <= f64::EPSILON {
        return Ok(());
    }
    ctx.charge_work(
        cadmpeg_core::decode::u64_from_index(faces.len()),
        "match SLDPRT planar selection faces",
    )?;
    let candidates = faces.iter().filter_map(|face| {
        let SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(plane_surface)) =
            &surfaces.get(&face.surface)?.geometry
        else {
            return None;
        };
        let candidate_origin = plane_surface.origin();
        let candidate_normal = plane_surface.frame().axis().as_raw();
        let candidate_length = candidate_normal.norm();
        if !candidate_length.is_finite() || candidate_length <= f64::EPSILON {
            return None;
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
        ((alignment.abs() - 1.0).abs() <= EPS_SELECTIONS_RESOLVE_PLANAR_FACE_SELECTION_E9
            && separation.abs() <= EPS_SELECTIONS_RESOLVE_PLANAR_FACE_SELECTION_E8)
            .then_some(&face.id)
    });
    let mut matching = Vec::new();
    for face in candidates {
        ctx.reserve_vec(
            &mut matching,
            1,
            "collect SLDPRT topology selection identities",
        )?;
        matching.push(copy_selection_id(ctx, face.as_str())?);
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

fn resolve_planar_profile_ref(
    ctx: &DecodeContext<'_>,
    profile: &mut PlanarProfileRef,
    faces: &HashMap<&str, Option<&cadmpeg_ir::ids::FaceId>>,
) -> Result<(), CodecError> {
    ctx.charge_work(1, "bind SLDPRT topology selections")?;
    if let PlanarProfileRef::Native(native) = profile {
        if let Some(ids) = resolve_ids(ctx, native, faces, cadmpeg_ir::ids::FaceId::as_str)? {
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
    ctx.charge_work(1, "bind SLDPRT topology selections")?;
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
    ctx.charge_work(1, "bind SLDPRT topology selections")?;
    if let PathRef::Native(native) = path {
        if let Some(ids) = resolve_ids(ctx, native, edges, cadmpeg_ir::ids::EdgeId::as_str)? {
            *path = PathRef::Edges(ids);
        } else if let Some(ids) =
            resolve_ids(ctx, native, curves, cadmpeg_ir::ids::CurveId::as_str)?
        {
            *path = PathRef::Curves(ids);
        }
    }
    Ok(())
}

fn selection_ids<'a, Id: 'a>(
    ctx: &DecodeContext<'_>,
    values: impl Iterator<Item = (&'a str, Option<&'a str>, &'a Id)>,
) -> Result<HashMap<&'a str, Option<&'a Id>>, CodecError> {
    let mut ids = HashMap::new();
    for (id, name, value) in values {
        reserve_selection_map(ctx, &mut ids)?;
        ids.insert(id, Some(value));
        if let Some(name) = name.filter(|name| !name.is_empty()) {
            reserve_selection_map(ctx, &mut ids)?;
            match ids.entry(name) {
                std::collections::hash_map::Entry::Vacant(entry) => {
                    entry.insert(Some(value));
                }
                std::collections::hash_map::Entry::Occupied(mut entry) => {
                    entry.insert(None);
                }
            }
        }
    }
    Ok(ids)
}

fn resolve_ids<Id: TryFrom<String, Error = cadmpeg_ir::ids::IdentityError>>(
    ctx: &DecodeContext<'_>,
    native: &str,
    ids: &HashMap<&str, Option<&Id>>,
    as_str: impl Fn(&Id) -> &str,
) -> Result<Option<Vec<Id>>, CodecError> {
    ctx.charge_work(
        cadmpeg_core::decode::u64_from_index(native.len()),
        "resolve SLDPRT topology selection tokens",
    )?;
    let mut resolved = Vec::new();
    for token in native
        .split(',')
        .map(str::trim)
        .filter(|token| !token.is_empty())
    {
        ctx.charge_work(1, "resolve SLDPRT topology selection tokens")?;
        let Some(Some(id)) = ids.get(token) else {
            return Ok(None);
        };
        ctx.reserve_vec(
            &mut resolved,
            1,
            "collect SLDPRT topology selection identities",
        )?;
        resolved.push(copy_selection_id(ctx, as_str(id))?);
    }
    Ok((!resolved.is_empty()).then_some(resolved))
}

fn resolve_face_selection(
    ctx: &DecodeContext<'_>,
    selection: &mut FaceSelection,
    context: &FaceSelectionContext<'_>,
) -> Result<(), CodecError> {
    ctx.charge_work(1, "bind SLDPRT topology selections")?;
    if let FaceSelection::Native(native) = selection {
        let mut faces = resolve_ids(ctx, native, context.ids, cadmpeg_ir::ids::FaceId::as_str)?;
        if faces.is_none() {
            if let Some(feature_ref) = context.feature_ref {
                ctx.charge_work(
                    cadmpeg_core::decode::u64_from_index(context.surface_selection_faces.len()),
                    "resolve SLDPRT surface selection owner",
                )?;
                if let Some(Some(face)) =
                    context
                        .surface_selection_faces
                        .iter()
                        .find_map(|((owner, key), value)| {
                            (owner == feature_ref && key == native).then_some(value)
                        })
                {
                    let mut copied = Vec::new();
                    ctx.reserve_vec(
                        &mut copied,
                        1,
                        "collect SLDPRT topology selection identities",
                    )?;
                    copied.push(copy_selection_id(ctx, face.as_str())?);
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
    }
    Ok(())
}

fn history_feature_sources<'a>(
    ctx: &DecodeContext<'_>,
    histories: &'a [FeatureHistory],
    lanes: &[crate::records::FeatureInputLane],
) -> Result<HashMap<&'a str, Option<FeatureSourceId>>, CodecError> {
    let mut sources = HashMap::new();
    for feature in histories.iter().flat_map(|history| &history.features) {
        ctx.charge_work(
            cadmpeg_core::decode::u64_from_index(lanes.len()),
            "resolve SLDPRT topology feature sources",
        )?;
        for lane in lanes {
            ctx.charge_work(
                cadmpeg_core::decode::u64_from_index(lane.names.len()),
                "resolve SLDPRT topology feature sources",
            )?;
        }
        let mut source = feature.source_id;
        if source.is_none() {
            let mut candidates = lanes.iter().filter_map(|lane| {
                crate::resolved_features::scalars::feature_object_name(feature, lane)?
                    .object_id?
                    .value()
            });
            if let Some(first) = candidates.next() {
                if candidates.all(|candidate| candidate == first) {
                    source = FeatureSource::from_value(first);
                }
            }
        }
        let source = source.and_then(FeatureSource::id);
        reserve_selection_map(ctx, &mut sources)?;
        match sources.entry(feature.id.as_str()) {
            std::collections::hash_map::Entry::Vacant(entry) => {
                entry.insert(source);
            }
            std::collections::hash_map::Entry::Occupied(mut entry) => {
                if entry.get() != &source {
                    entry.insert(None);
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
    ctx.charge_work(1, "bind SLDPRT topology selections")?;
    if let EdgeSelection::Native(native) = selection {
        if let Some(edges) = resolve_ids(ctx, native, ids, cadmpeg_ir::ids::EdgeId::as_str)? {
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
    ctx.charge_work(1, "bind SLDPRT topology selections")?;
    if let BodySelection::Native(native) = selection {
        let bodies = match resolve_ids(ctx, native, ids, cadmpeg_ir::ids::BodyId::as_str)? {
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

fn copy_selection_text(ctx: &DecodeContext<'_>, value: &str) -> Result<String, CodecError> {
    ctx.charge_work(
        cadmpeg_core::decode::u64_from_index(value.len()),
        "retain SLDPRT topology selection identity",
    )?;
    ctx.format_retained(
        format_args!("{value}"),
        "retain SLDPRT topology selection identity",
    )
}

fn copy_selection_id<Id: TryFrom<String, Error = cadmpeg_ir::ids::IdentityError>>(
    ctx: &DecodeContext<'_>,
    id: &str,
) -> Result<Id, CodecError> {
    let work = cadmpeg_core::decode::u64_from_index(id.len())
        .checked_mul(4)
        .ok_or_else(|| {
            ctx.refuse_codec_limit(
                "retain SLDPRT topology selection identity",
                u64::MAX - 1,
                u64::MAX,
            )
        })?;
    ctx.charge_work(work, "retain SLDPRT topology selection identity")?;
    let text = ctx.format_retained(
        format_args!("{id}"),
        "retain SLDPRT topology selection identity",
    )?;
    Id::try_from(text).map_err(CodecError::malformed)
}

fn reserve_selection_map<K: Eq + std::hash::Hash, V>(
    ctx: &DecodeContext<'_>,
    values: &mut HashMap<K, V>,
) -> Result<(), CodecError> {
    const OPERATION: &str = "index SLDPRT topology selections";
    ctx.charge_work(1, OPERATION)?;
    ctx.reserve_map(values, 1, OPERATION)
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
            type_signature: type_signature.into(),
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
        let key = (
            "feature".to_string(),
            "sldprt:feature-input:surface-component-ids:8,5".to_string(),
        );
        assert_eq!(
            bindings.get(&key).cloned(),
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
        let key = (
            "feature".to_string(),
            "sldprt:feature-input:surface-component-ids:5".to_string(),
        );
        assert_eq!(bindings.get(&key).cloned(), Some(None));
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
        let key = (
            "feature".to_string(),
            "sldprt:feature-input:surface-component-ids:8,5".to_string(),
        );
        assert_eq!(
            bindings.get(&key).cloned(),
            Some(Some(
                cadmpeg_ir::ids::FaceId::mint("test:model:entity#terminal-face")
                    .expect("identity grammar")
            ))
        );
    }
}
