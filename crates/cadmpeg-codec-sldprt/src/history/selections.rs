// SPDX-License-Identifier: Apache-2.0
//! Bind native topology selections to decoded B-rep identities.

use crate::records::{FeatureHistory, FeatureInputSurfaceSelection};
use cadmpeg_core::decode::View;
use cadmpeg_ir::features::{
    BodySelection, DatumPlaneReference, EdgeSelection, ExtrudeExtent, ExtrudeSide, FaceSelection,
    FeatureDefinition, Length, PathRef, PatternKind, ProfileRef, Termination,
};
use cadmpeg_ir::geometry::{Curve, Surface, SurfaceGeometry};
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::topology::{Body, Edge, Face};
use std::collections::{BTreeMap, HashMap};

use crate::history::literals::{parse_point3_mm, parse_vector3};

const EPS_SELECTIONS_RESOLVE_PLANAR_FACE_SELECTION_E9: f64 = 1e-9;
const EPS_SELECTIONS_RESOLVE_PLANAR_FACE_SELECTION_E8: f64 = 1e-8;

pub(crate) type SurfaceSelectionFaceBindings =
    HashMap<(String, String), Option<Vec<cadmpeg_ir::ids::FaceId>>>;

pub(crate) struct FaceSelectionContext<'a> {
    pub(crate) ids: &'a HashMap<String, Option<cadmpeg_ir::ids::FaceId>>,
    pub(crate) feature_ref: Option<&'a str>,
    pub(crate) surface_selection_faces: &'a SurfaceSelectionFaceBindings,
}

pub(crate) struct TopologySelectionInputs<'a> {
    pub(crate) bodies: &'a [Body],
    pub(crate) faces: &'a [Face],
    pub(crate) surfaces: &'a [Surface],
    pub(crate) edges: &'a [Edge],
    pub(crate) curves: &'a [Curve],
    pub(crate) lanes: &'a [crate::records::FeatureInputLane],
    pub(crate) face_identities: &'a [(String, u32, u32)],
}

const SURFACE_COMPONENT_SELECTION_PREFIX: &str = "sldprt:feature-input:surface-component-ids";

/// Return the native expression retained by a face selection, when present.
pub(crate) fn face_selection_native(selection: &FaceSelection) -> Option<&str> {
    match selection {
        FaceSelection::Resolved { native, .. }
        | FaceSelection::Historical { native, .. }
        | FaceSelection::HistoricalPartial { native, .. }
        | FaceSelection::Generated { native, .. }
        | FaceSelection::Native(native) => Some(native),
        FaceSelection::Unresolved | FaceSelection::Faces(_) => None,
    }
}

/// Resolve the support origin represented by a face-backed offset reference.
/// Explicit face frames and legacy face aliases store the support origin. A
/// surface-component selection stores the resulting plane origin, so its
/// support is one signed `D1` displacement along the stored normal.
pub(crate) fn offset_plane_support_origin(
    source_properties: &BTreeMap<String, String>,
    native: Option<&str>,
    fallback_origin: Point3,
    normal: Vector3,
    distance: Length,
) -> Point3 {
    if let Some(origin) = source_properties
        .get("ReferenceFaceOrigin")
        .and_then(|value| parse_point3_mm(value))
    {
        return origin;
    }
    let origin = source_properties
        .get("Origin")
        .and_then(|value| parse_point3_mm(value))
        .unwrap_or(fallback_origin);
    if native.is_some_and(|native| native.starts_with(SURFACE_COMPONENT_SELECTION_PREFIX)) {
        return Point3::new(
            origin.x + normal.x * distance.0,
            origin.y + normal.y * distance.0,
            origin.z + normal.z * distance.0,
        );
    }
    origin
}

fn surface_selection_face_bindings<'a>(
    selections: impl IntoIterator<Item = &'a FeatureInputSurfaceSelection>,
    feature_sources: &HashMap<String, Option<u32>>,
    face_identities: &[(String, u32, u32)],
) -> SurfaceSelectionFaceBindings {
    let mut faces_by_identity = HashMap::<(u32, u32), Option<cadmpeg_ir::ids::FaceId>>::new();
    for (target, feature_source_id, local_face_id) in face_identities {
        let candidate = cadmpeg_ir::ids::FaceId(target.clone());
        let entry = faces_by_identity
            .entry((*feature_source_id, *local_face_id))
            .or_insert_with(|| Some(candidate.clone()));
        if entry
            .as_ref()
            .is_some_and(|existing| existing != &candidate)
        {
            *entry = None;
        }
    }

    let mut bindings = SurfaceSelectionFaceBindings::new();
    for selection in selections {
        let candidate = selection.components.last().and_then(|component| {
            let feature_source_id = match selection.terminal_feature_ref.as_deref() {
                Some(terminal_feature) => feature_sources.get(terminal_feature).copied().flatten(),
                None => View::u32_le_at(&component.type_signature, 4),
            }?;
            let local_face_id = component.local_id?;
            faces_by_identity
                .get(&(feature_source_id, local_face_id))
                .cloned()
                .flatten()
        });
        let native = crate::resolved_features::terminations::compact_surface_selection_value(
            &selection.components,
        );
        let key = (selection.feature_ref.clone(), native);
        let candidate = candidate.map(|face| vec![face]);
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
    bindings
}

pub(crate) fn extrude_extent_sides_mut(extent: &mut ExtrudeExtent) -> Vec<&mut ExtrudeSide> {
    match extent {
        ExtrudeExtent::OneSided { side } | ExtrudeExtent::Symmetric { side } => vec![side],
        ExtrudeExtent::TwoSided { first, second } => vec![first, second],
    }
}

pub fn bind_topology_selections(
    features: &mut [cadmpeg_ir::features::Feature],
    histories: &[FeatureHistory],
    inputs: &TopologySelectionInputs<'_>,
) {
    let bodies = inputs.bodies;
    let faces = inputs.faces;
    let surfaces = inputs.surfaces;
    let edges = inputs.edges;
    let curves = inputs.curves;
    let lanes = inputs.lanes;
    let face_identities = inputs.face_identities;
    let body_ids = selection_ids(
        bodies
            .iter()
            .map(|body| (body.id.0.as_str(), body.name.as_deref(), body.id.clone())),
    );
    let face_ids = selection_ids(
        faces
            .iter()
            .map(|face| (face.id.0.as_str(), face.name.as_deref(), face.id.clone())),
    );
    let edge_ids = selection_ids(
        edges
            .iter()
            .map(|edge| (edge.id.0.as_str(), None, edge.id.clone())),
    );
    let curve_ids = selection_ids(
        curves
            .iter()
            .map(|curve| (curve.id.0.as_str(), None, curve.id.clone())),
    );
    let surfaces_by_id = surfaces
        .iter()
        .map(|surface| (&surface.id, surface))
        .collect::<HashMap<_, _>>();
    let feature_sources = history_feature_sources(histories, lanes);
    let surface_selection_faces = surface_selection_face_bindings(
        lanes.iter().flat_map(|lane| lane.surface_selections.iter()),
        &feature_sources,
        face_identities,
    );
    for feature in features {
        let feature_native_ref = feature.native_ref.clone();
        let face_selection_context = FaceSelectionContext {
            ids: &face_ids,
            feature_ref: feature_native_ref.as_deref(),
            surface_selection_faces: &surface_selection_faces,
        };
        let resolve_face = |selection: &mut FaceSelection| {
            resolve_face_selection(selection, &face_selection_context);
        };
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
            if let Some(outputs) = resolve_ids(scope, &body_ids) {
                feature.outputs = outputs;
            }
        }
        match &mut feature.definition {
            FeatureDefinition::DatumOffsetPlane {
                reference:
                    Some(DatumPlaneReference::Face {
                        face: reference,
                        origin,
                        normal,
                        ..
                    }),
                distance,
            } => {
                let native = face_selection_native(reference).or_else(|| {
                    feature
                        .source_properties
                        .get("ReferenceFaceNative")
                        .map(String::as_str)
                });
                let support_origin = offset_plane_support_origin(
                    &feature.source_properties,
                    native,
                    *origin,
                    *normal,
                    *distance,
                );
                *origin = support_origin;
                resolve_offset_plane_face_selection(
                    reference,
                    support_origin,
                    *normal,
                    &face_selection_context,
                    faces,
                    &surfaces_by_id,
                );
            }
            FeatureDefinition::DatumOffsetPlane { reference, .. } if reference.is_none() => {
                let Some(origin) = feature
                    .source_properties
                    .get("Origin")
                    .and_then(|value| parse_point3_mm(value))
                else {
                    continue;
                };
                let Some(normal) = feature
                    .source_properties
                    .get("Normal")
                    .and_then(|value| parse_vector3(value))
                else {
                    continue;
                };
                let Some(u_axis) = feature
                    .source_properties
                    .get("UAxis")
                    .and_then(|value| parse_vector3(value))
                else {
                    continue;
                };
                let mut face = FaceSelection::Unresolved;
                resolve_planar_face_selection(&mut face, origin, normal, faces, &surfaces_by_id);
                if !matches!(face, FaceSelection::Unresolved) {
                    *reference = Some(DatumPlaneReference::Face {
                        face,
                        origin,
                        normal,
                        u_axis,
                    });
                }
            }
            FeatureDefinition::Extrude {
                profile, extent, ..
            } => {
                resolve_profile_ref(profile, &face_ids);
                for side in extrude_extent_sides_mut(extent) {
                    if let Termination::ToFace { face, .. }
                    | Termination::OffsetFromFace { face, .. } = &mut side.termination
                    {
                        resolve_face(face);
                    }
                }
            }
            FeatureDefinition::Revolve { construction, .. } => {
                if let Some(profile) = &mut construction.profile {
                    resolve_profile_ref(profile, &face_ids);
                }
            }
            FeatureDefinition::Rib { construction, .. } => {
                if let Some(profile) = &mut construction.profile {
                    resolve_profile_ref(profile, &face_ids);
                }
            }
            FeatureDefinition::Sweep { section, path, .. } => {
                if let Some(profile) = section.referenced_profile_mut() {
                    resolve_profile_ref(profile, &face_ids);
                }
                if let Some(path) = path {
                    resolve_path_ref(path, &edge_ids, &curve_ids);
                }
            }
            FeatureDefinition::Loft {
                sections, guides, ..
            } => {
                for section in sections {
                    if let cadmpeg_ir::features::LoftSection::Profile(profile) = section {
                        resolve_profile_ref(profile, &face_ids);
                    }
                }
                for path in guides {
                    resolve_path_ref(path, &edge_ids, &curve_ids);
                }
            }
            FeatureDefinition::Fillet { groups } => {
                for group in groups {
                    resolve_edge_selection(&mut group.edges, &edge_ids);
                }
            }
            FeatureDefinition::Chamfer { groups, .. } => {
                for group in groups {
                    resolve_edge_selection(&mut group.edges, &edge_ids);
                }
            }
            FeatureDefinition::Shell { removed_faces, .. } => {
                resolve_face(removed_faces);
            }
            FeatureDefinition::Thicken { faces, .. } => {
                resolve_face(faces);
            }
            FeatureDefinition::OffsetSurface { faces, .. } => {
                resolve_face(faces);
            }
            FeatureDefinition::KnitSurface { faces, .. } => {
                resolve_face(faces);
            }
            FeatureDefinition::FilledSurface {
                boundary,
                support_faces,
                ..
            } => {
                if let cadmpeg_ir::features::SurfaceBoundary::Edges(edges) = boundary {
                    resolve_edge_selection(edges, &edge_ids);
                }
                resolve_face(support_faces);
            }
            FeatureDefinition::TrimSurface { faces, tool, .. } => {
                resolve_face(faces);
                resolve_path_ref(tool, &edge_ids, &curve_ids);
            }
            FeatureDefinition::ExtendSurface { faces, .. } => {
                resolve_face(faces);
            }
            FeatureDefinition::RuledSurface {
                edges,
                support_faces,
                ..
            } => {
                resolve_edge_selection(edges, &edge_ids);
                resolve_face(support_faces);
            }
            FeatureDefinition::Draft {
                faces,
                neutral_plane,
                ..
            } => {
                resolve_face(faces);
                resolve_face(neutral_plane);
            }
            FeatureDefinition::Combine { target, tools, .. } => {
                resolve_body_selection(target, &body_ids);
                resolve_body_selection(tools, &body_ids);
            }
            FeatureDefinition::CutWithSurface { targets, tools, .. } => {
                resolve_body_selection(targets, &body_ids);
                resolve_face(tools);
            }
            FeatureDefinition::DeleteBody { bodies, .. } => {
                resolve_body_selection(bodies, &body_ids);
            }
            FeatureDefinition::Pattern {
                pattern:
                    PatternKind::CurveDriven {
                        path: Some(path), ..
                    },
                ..
            } => resolve_path_ref(path, &edge_ids, &curve_ids),
            FeatureDefinition::Scale { bodies, .. } => {
                resolve_body_selection(bodies, &body_ids);
            }
            FeatureDefinition::MoveBody { bodies, .. } => {
                resolve_body_selection(bodies, &body_ids);
            }
            FeatureDefinition::DeleteFace { faces, .. }
            | FeatureDefinition::MoveFace { faces, .. }
            | FeatureDefinition::Dome { faces, .. } => {
                resolve_face(faces);
            }
            FeatureDefinition::ReplaceFace {
                targets,
                replacements,
            } => {
                resolve_face(targets);
                resolve_face(replacements);
            }
            FeatureDefinition::Hole {
                face: Some(face), ..
            } => {
                resolve_face(face);
            }
            FeatureDefinition::Wrap { profile, face, .. } => {
                resolve_profile_ref(profile, &face_ids);
                resolve_face(face);
            }
            FeatureDefinition::ProjectedCurve {
                source,
                target_faces,
                ..
            } => {
                resolve_path_ref(source, &edge_ids, &curve_ids);
                resolve_face(target_faces);
            }
            FeatureDefinition::CompositeCurve { segments, .. } => {
                for segment in segments {
                    resolve_path_ref(segment, &edge_ids, &curve_ids);
                }
            }
            _ => {}
        }
    }
}

pub(crate) fn resolve_planar_face_selection(
    selection: &mut FaceSelection,
    origin: Point3,
    normal: Vector3,
    faces: &[Face],
    surfaces: &HashMap<&cadmpeg_ir::ids::SurfaceId, &Surface>,
) {
    let native = match selection {
        FaceSelection::Unresolved => None,
        FaceSelection::Native(native) => Some(native.clone()),
        _ => return,
    };
    let normal_length = normal.norm();
    if !normal_length.is_finite() || normal_length <= f64::EPSILON {
        return;
    }
    let matching = faces
        .iter()
        .filter_map(|face| {
            let SurfaceGeometry::Plane {
                origin: candidate_origin,
                normal: candidate_normal,
                ..
            } = &surfaces.get(&face.surface)?.geometry
            else {
                return None;
            };
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
                .then_some(face.id.clone())
        })
        .collect::<Vec<_>>();
    *selection = match (native, matching.as_slice()) {
        (Some(native), [_, ..]) => FaceSelection::Resolved {
            faces: matching,
            native,
        },
        (None, [face]) => FaceSelection::Faces(vec![face.clone()]),
        _ => return,
    };
}

pub(crate) fn resolve_offset_plane_face_selection(
    selection: &mut FaceSelection,
    origin: Point3,
    normal: Vector3,
    context: &FaceSelectionContext<'_>,
    faces: &[Face],
    surfaces: &HashMap<&cadmpeg_ir::ids::SurfaceId, &Surface>,
) {
    if !matches!(selection, FaceSelection::Native(_)) {
        return;
    }
    resolve_face_selection(selection, context);
    if !matches!(selection, FaceSelection::Native(_)) {
        return;
    }
    resolve_planar_face_selection(selection, origin, normal, faces, surfaces);
}

pub(crate) fn resolve_profile_ref(
    profile: &mut ProfileRef,
    faces: &HashMap<String, Option<cadmpeg_ir::ids::FaceId>>,
) {
    if let ProfileRef::Native(native) = profile {
        if let Some(ids) = resolve_ids(native, faces) {
            *profile = ProfileRef::Faces(ids);
        }
    }
}

pub(crate) fn resolve_path_ref(
    path: &mut PathRef,
    edges: &HashMap<String, Option<cadmpeg_ir::ids::EdgeId>>,
    curves: &HashMap<String, Option<cadmpeg_ir::ids::CurveId>>,
) {
    if let PathRef::Native(native) = path {
        if let Some(ids) = resolve_ids(native, edges) {
            *path = PathRef::Edges(ids);
        } else if let Some(ids) = resolve_ids(native, curves) {
            *path = PathRef::Curves(ids);
        }
    }
}

pub(crate) fn selection_ids<'a, Id: Clone + 'a>(
    values: impl Iterator<Item = (&'a str, Option<&'a str>, Id)>,
) -> HashMap<String, Option<Id>> {
    let mut ids = HashMap::new();
    for (id, name, value) in values {
        ids.insert(id.to_string(), Some(value.clone()));
        if let Some(name) = name.filter(|name| !name.is_empty()) {
            ids.entry(name.to_string())
                .and_modify(|candidate| *candidate = None)
                .or_insert(Some(value));
        }
    }
    ids
}

pub(crate) fn resolve_ids<Id: Clone>(
    native: &str,
    ids: &HashMap<String, Option<Id>>,
) -> Option<Vec<Id>> {
    let resolved = native
        .split(',')
        .map(str::trim)
        .filter(|token| !token.is_empty())
        .map(|token| ids.get(token).and_then(Clone::clone))
        .collect::<Option<Vec<_>>>()?;
    (!resolved.is_empty()).then_some(resolved)
}

pub(crate) fn resolve_face_selection(
    selection: &mut FaceSelection,
    context: &FaceSelectionContext<'_>,
) {
    if let FaceSelection::Native(native) = selection {
        let faces = resolve_ids(native, context.ids).or_else(|| {
            let feature_ref = context.feature_ref?;
            context
                .surface_selection_faces
                .get(&(feature_ref.to_string(), native.clone()))
                .cloned()
                .flatten()
        });
        if let Some(faces) = faces {
            *selection = FaceSelection::Resolved {
                faces,
                native: native.clone(),
            };
        }
    }
}

fn history_feature_sources(
    histories: &[FeatureHistory],
    lanes: &[crate::records::FeatureInputLane],
) -> HashMap<String, Option<u32>> {
    let mut features = histories
        .iter()
        .flat_map(|history| &history.features)
        .cloned()
        .collect::<Vec<_>>();
    crate::resolved_features::selections::enrich_feature_object_sources(&mut features, lanes);
    let mut sources = HashMap::new();
    for feature in features {
        let source = feature
            .source_id
            .as_deref()
            .and_then(|value| value.parse::<u32>().ok());
        match sources.entry(feature.id.clone()) {
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
    sources
}

pub(crate) fn resolve_edge_selection(
    selection: &mut EdgeSelection,
    ids: &HashMap<String, Option<cadmpeg_ir::ids::EdgeId>>,
) {
    if let EdgeSelection::Native(native) = selection {
        if let Some(edges) = resolve_ids(native, ids) {
            *selection = EdgeSelection::Resolved {
                edges,
                native: native.clone(),
            };
        }
    }
}

pub(crate) fn resolve_body_selection(
    selection: &mut BodySelection,
    ids: &HashMap<String, Option<cadmpeg_ir::ids::BodyId>>,
) {
    if let BodySelection::Native(native) = selection {
        if let Some(bodies) = resolve_ids(native, ids) {
            *selection = BodySelection::Resolved {
                bodies,
                native: native.clone(),
            };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::records::FeatureInputComponentPathEntry;

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
            endpoint_selector: None,
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
            std::iter::once(&selection),
            &feature_sources,
            &[
                ("intermediate-face".into(), 47, 8),
                ("terminal-face".into(), 50, 5),
            ],
        );
        let key = (
            "feature".to_string(),
            "sldprt:feature-input:surface-component-ids:8,5".to_string(),
        );
        assert_eq!(
            bindings.get(&key).cloned(),
            Some(Some(vec![cadmpeg_ir::ids::FaceId("terminal-face".into())]))
        );
    }

    #[test]
    fn surface_selection_keeps_ambiguous_terminal_identity_native() {
        let selection = surface_selection("feature", vec![component(50, 5)]);
        let feature_sources = HashMap::new();
        let bindings = surface_selection_face_bindings(
            std::iter::once(&selection),
            &feature_sources,
            &[("first-face".into(), 50, 5), ("second-face".into(), 50, 5)],
        );
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
        let feature_sources = HashMap::from([(String::from("terminal"), Some(50))]);
        let bindings = surface_selection_face_bindings(
            std::iter::once(&selection),
            &feature_sources,
            &[("terminal-face".into(), 50, 5)],
        );
        let key = (
            "feature".to_string(),
            "sldprt:feature-input:surface-component-ids:8,5".to_string(),
        );
        assert_eq!(
            bindings.get(&key).cloned(),
            Some(Some(vec![cadmpeg_ir::ids::FaceId("terminal-face".into())]))
        );
    }
}
