// SPDX-License-Identifier: Apache-2.0
//! Bind native topology selections to decoded B-rep identities.

use crate::records::FeatureHistory;
use cadmpeg_ir::features::{
    BodySelection, DatumPlaneReference, EdgeSelection, ExtrudeExtent, ExtrudeSide, FaceSelection,
    FeatureDefinition, PathRef, PatternKind, ProfileRef, Termination,
};
use cadmpeg_ir::geometry::{Curve, Surface, SurfaceGeometry};
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::topology::{Body, Edge, Face};
use std::collections::HashMap;

use crate::history::literals::{parse_point3_mm, parse_vector3};

const EPS_SELECTIONS_RESOLVE_PLANAR_FACE_SELECTION_E9: f64 = 1.0e-9;
const EPS_SELECTIONS_RESOLVE_PLANAR_FACE_SELECTION_E8: f64 = 1.0e-8;

pub(crate) fn extrude_extent_sides_mut(extent: &mut ExtrudeExtent) -> Vec<&mut ExtrudeSide> {
    match extent {
        ExtrudeExtent::OneSided { side } | ExtrudeExtent::Symmetric { side } => vec![side],
        ExtrudeExtent::TwoSided { first, second } => vec![first, second],
    }
}

pub fn bind_topology_selections(
    features: &mut [cadmpeg_ir::features::Feature],
    histories: &[FeatureHistory],
    bodies: &[Body],
    faces: &[Face],
    surfaces: &[Surface],
    edges: &[Edge],
    curves: &[Curve],
) {
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
    for feature in features {
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
                ..
            } => {
                resolve_offset_plane_face_selection(
                    reference,
                    *origin,
                    *normal,
                    faces,
                    &surfaces_by_id,
                );
            }
            FeatureDefinition::DatumOffsetPlane {
                reference,
                distance,
            } if reference.is_none() => {
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
                let support_origin = Point3::new(
                    origin.x + normal.x * distance.0,
                    origin.y + normal.y * distance.0,
                    origin.z + normal.z * distance.0,
                );
                let mut face = FaceSelection::Unresolved;
                resolve_planar_face_selection(
                    &mut face,
                    support_origin,
                    normal,
                    faces,
                    &surfaces_by_id,
                );
                if !matches!(face, FaceSelection::Unresolved) {
                    *reference = Some(DatumPlaneReference::Face {
                        face,
                        origin: support_origin,
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
                        resolve_face_selection(face, &face_ids);
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
                resolve_face_selection(removed_faces, &face_ids);
            }
            FeatureDefinition::Thicken { faces, .. } => {
                resolve_face_selection(faces, &face_ids);
            }
            FeatureDefinition::OffsetSurface { faces, .. } => {
                resolve_face_selection(faces, &face_ids);
            }
            FeatureDefinition::KnitSurface { faces, .. } => {
                resolve_face_selection(faces, &face_ids);
            }
            FeatureDefinition::FilledSurface {
                boundary,
                support_faces,
                ..
            } => {
                if let cadmpeg_ir::features::SurfaceBoundary::Edges(edges) = boundary {
                    resolve_edge_selection(edges, &edge_ids);
                }
                resolve_face_selection(support_faces, &face_ids);
            }
            FeatureDefinition::TrimSurface { faces, tool, .. } => {
                resolve_face_selection(faces, &face_ids);
                resolve_path_ref(tool, &edge_ids, &curve_ids);
            }
            FeatureDefinition::ExtendSurface { faces, .. } => {
                resolve_face_selection(faces, &face_ids);
            }
            FeatureDefinition::RuledSurface {
                edges,
                support_faces,
                ..
            } => {
                resolve_edge_selection(edges, &edge_ids);
                resolve_face_selection(support_faces, &face_ids);
            }
            FeatureDefinition::Draft {
                faces,
                neutral_plane,
                ..
            } => {
                resolve_face_selection(faces, &face_ids);
                resolve_face_selection(neutral_plane, &face_ids);
            }
            FeatureDefinition::Combine { target, tools, .. } => {
                resolve_body_selection(target, &body_ids);
                resolve_body_selection(tools, &body_ids);
            }
            FeatureDefinition::CutWithSurface { targets, tools, .. } => {
                resolve_body_selection(targets, &body_ids);
                resolve_face_selection(tools, &face_ids);
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
                resolve_face_selection(faces, &face_ids);
            }
            FeatureDefinition::ReplaceFace {
                targets,
                replacements,
            } => {
                resolve_face_selection(targets, &face_ids);
                resolve_face_selection(replacements, &face_ids);
            }
            FeatureDefinition::Hole {
                face: Some(face), ..
            } => {
                resolve_face_selection(face, &face_ids);
            }
            FeatureDefinition::Wrap { profile, face, .. } => {
                resolve_profile_ref(profile, &face_ids);
                resolve_face_selection(face, &face_ids);
            }
            FeatureDefinition::ProjectedCurve {
                source,
                target_faces,
                ..
            } => {
                resolve_path_ref(source, &edge_ids, &curve_ids);
                resolve_face_selection(target_faces, &face_ids);
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
    faces: &[Face],
    surfaces: &HashMap<&cadmpeg_ir::ids::SurfaceId, &Surface>,
) {
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
    ids: &HashMap<String, Option<cadmpeg_ir::ids::FaceId>>,
) {
    if let FaceSelection::Native(native) = selection {
        if let Some(faces) = resolve_ids(native, ids) {
            *selection = FaceSelection::Resolved {
                faces,
                native: native.clone(),
            };
        }
    }
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
