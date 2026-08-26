// SPDX-License-Identifier: Apache-2.0
//! Write-only helpers for native feature-record encoding.

use crate::classification::{classify, FeatureClass};
use crate::history::classify::feature_family;
use crate::records::Feature;
use cadmpeg_core::CodecError;
use cadmpeg_ir::features::{
    BodySelection, BooleanOp, EdgeSelection, FaceSelection, FeatureId, FeatureTreeNodeRole,
    PathRef, ProfileRef, VertexSelection,
};
use cadmpeg_ir::math::Vector3;
use std::collections::{BTreeMap, HashMap};

pub(super) fn feature_tree_node_kind(role: FeatureTreeNodeRole) -> &'static str {
    match role {
        FeatureTreeNodeRole::Annotations => "Annotations",
        FeatureTreeNodeRole::AmbientLight => "Ambient",
        FeatureTreeNodeRole::Comments => "Comments",
        FeatureTreeNodeRole::CrossSections => "Cross Sections",
        FeatureTreeNodeRole::DesignBinder => "Design Binder",
        FeatureTreeNodeRole::Details => "Details",
        FeatureTreeNodeRole::DissectedProfile => "Profile Selection",
        FeatureTreeNodeRole::DirectionalLight => "Directional",
        FeatureTreeNodeRole::Equations => "Equations",
        FeatureTreeNodeRole::ExplodedViews => "Exploded Views",
        FeatureTreeNodeRole::Favorites => "Favorites",
        FeatureTreeNodeRole::FeatureFolder => "Folder",
        FeatureTreeNodeRole::History => "History",
        FeatureTreeNodeRole::LightsAndCameras => "Lights and Cameras",
        FeatureTreeNodeRole::Markups => "Markups",
        FeatureTreeNodeRole::ModelOrigin => "Origin",
        FeatureTreeNodeRole::PointLight => "Point Light",
        FeatureTreeNodeRole::Materials => "SOLIDWORKS Materials",
        FeatureTreeNodeRole::Notes => "Notes",
        FeatureTreeNodeRole::SelectionSets => "Selection Sets",
        FeatureTreeNodeRole::Sensors => "Sensors",
        FeatureTreeNodeRole::SheetMetal => "Sheet Metal",
        FeatureTreeNodeRole::SolidBodies => "Solid Bodies",
        FeatureTreeNodeRole::SpotLight => "Spot Light",
        FeatureTreeNodeRole::SurfaceBodies => "Surface Bodies",
        FeatureTreeNodeRole::Tables => "Tables",
    }
}

/// Reject a neutral edit that retargets an existing native record to an
/// operation family it did not originate in. A missing record (a freshly
/// synthesized feature) always passes.
pub(super) fn require_same_family(
    existing: Option<&Feature>,
    feature_id: &FeatureId,
    families: &[&str],
) -> Result<(), CodecError> {
    if existing.is_some_and(|record| !families.iter().any(|family| feature_family(record, family)))
    {
        return Err(CodecError::NotImplemented(format!(
            "SLDPRT feature {feature_id} changes operation family"
        )));
    }
    Ok(())
}

pub(super) fn is_revolve(feature: &Feature) -> bool {
    classify(feature) == Some(FeatureClass::Revolve)
}

pub(super) fn is_loft(feature: &Feature) -> bool {
    classify(feature) == Some(FeatureClass::Loft)
}

pub(super) fn is_sweep(feature: &Feature) -> bool {
    classify(feature) == Some(FeatureClass::Sweep)
}

pub(super) fn is_helix(feature: &Feature) -> bool {
    classify(feature) == Some(FeatureClass::Helix)
}

pub(super) fn write_native_selection(
    properties: &mut BTreeMap<String, String>,
    key: &str,
    selection: &str,
    fallback: &str,
) {
    if selection != fallback || properties.contains_key(key) {
        properties.insert(key.into(), selection.into());
    } else {
        properties.remove(key);
    }
}

pub(super) fn face_selection_value(selection: &FaceSelection) -> Option<String> {
    match selection {
        FaceSelection::Native(native)
        | FaceSelection::Resolved { native, .. }
        | FaceSelection::Generated { native, .. }
            if !native.trim().is_empty() =>
        {
            Some(native.clone())
        }
        FaceSelection::Faces(faces) if !faces.is_empty() => Some(
            faces
                .iter()
                .map(|face| face.0.as_str())
                .collect::<Vec<_>>()
                .join(","),
        ),
        _ => None,
    }
}

pub(super) fn vertex_selection_value(selection: &VertexSelection) -> Option<String> {
    match selection {
        VertexSelection::Native(native)
        | VertexSelection::Generated { native, .. }
        | VertexSelection::Historical { native, .. }
            if !native.trim().is_empty() =>
        {
            Some(native.clone())
        }
        _ => None,
    }
}

pub(super) fn edge_selection_value(selection: &EdgeSelection) -> Option<String> {
    match selection {
        EdgeSelection::Native(native)
        | EdgeSelection::Resolved { native, .. }
        | EdgeSelection::Generated { native, .. }
            if !native.trim().is_empty() =>
        {
            Some(native.clone())
        }
        EdgeSelection::Edges(edges) if !edges.is_empty() => Some(
            edges
                .iter()
                .map(|edge| edge.0.as_str())
                .collect::<Vec<_>>()
                .join(","),
        ),
        _ => None,
    }
}

pub(super) fn body_selection_value(selection: &BodySelection) -> Option<String> {
    match selection {
        BodySelection::Native(native)
        | BodySelection::Resolved { native, .. }
        | BodySelection::Generated { native, .. }
        | BodySelection::Local { native, .. }
            if !native.trim().is_empty() =>
        {
            Some(native.clone())
        }
        BodySelection::Bodies(bodies) if !bodies.is_empty() => Some(
            bodies
                .iter()
                .map(|body| body.0.as_str())
                .collect::<Vec<_>>()
                .join(","),
        ),
        _ => None,
    }
}

fn format_boolean_op(value: BooleanOp) -> Option<&'static str> {
    Some(match value {
        BooleanOp::Unresolved => return None,
        BooleanOp::Join => "Join",
        BooleanOp::Cut => "Cut",
        BooleanOp::Intersect => "Intersect",
        BooleanOp::NewBody => "NewBody",
    })
}

pub(super) fn resolved_boolean_op(
    value: BooleanOp,
    feature: &FeatureId,
) -> Result<&'static str, CodecError> {
    format_boolean_op(value).ok_or_else(|| {
        CodecError::NotImplemented(format!(
            "SLDPRT feature {feature} has an unresolved boolean operation"
        ))
    })
}

pub(super) fn profile_source(
    profile: &ProfileRef,
    native: &HashMap<String, String>,
    features: &HashMap<&FeatureId, &str>,
    sketches: &HashMap<cadmpeg_ir::sketches::SketchId, String>,
) -> Option<String> {
    match profile {
        ProfileRef::Unresolved(_) => None,
        ProfileRef::Native(id) => Some(native.get(id).cloned().unwrap_or_else(|| id.clone())),
        ProfileRef::Sketch(id) => sketches.get(id).cloned(),
        ProfileRef::SketchProfiles { sketch, .. }
        | ProfileRef::SketchRegions { sketch, .. }
        | ProfileRef::SketchEntities { sketch, .. }
        | ProfileRef::SketchSelection { sketch, .. } => sketches.get(sketch).cloned(),
        ProfileRef::SpatialSketchProfiles { .. }
        | ProfileRef::SpatialSketchSelection { .. }
        | ProfileRef::HistoricalFaces { .. } => None,
        ProfileRef::Feature(id) => features.get(id).map(|source| (*source).to_string()),
        ProfileRef::Generated { .. } => None,
        ProfileRef::Faces(faces) if !faces.is_empty() => Some(
            faces
                .iter()
                .map(|face| face.0.as_str())
                .collect::<Vec<_>>()
                .join(","),
        ),
        ProfileRef::Faces(_) => None,
    }
}

pub(super) fn path_source(
    path: &PathRef,
    native: &HashMap<String, String>,
    sketches: &HashMap<cadmpeg_ir::sketches::SketchId, String>,
) -> Option<String> {
    match path {
        PathRef::Unresolved(_) => None,
        PathRef::Native(id) => Some(native.get(id).cloned().unwrap_or_else(|| id.clone())),
        PathRef::Sketch(id) => sketches.get(id).cloned(),
        PathRef::SketchCurves { .. }
        | PathRef::SpatialSketchCurves { .. }
        | PathRef::SpatialSketchSelection { .. }
        | PathRef::HistoricalEdges { .. } => None,
        PathRef::Edges(edges) if !edges.is_empty() => Some(
            edges
                .iter()
                .map(|edge| edge.0.as_str())
                .collect::<Vec<_>>()
                .join(","),
        ),
        PathRef::Curves(curves) if !curves.is_empty() => Some(
            curves
                .iter()
                .map(|curve| curve.0.as_str())
                .collect::<Vec<_>>()
                .join(","),
        ),
        PathRef::Edges(_) | PathRef::Curves(_) => None,
    }
}

pub(super) fn require_direction(
    direction: Vector3,
    feature: &FeatureId,
    role: &str,
) -> Result<(), CodecError> {
    if direction.norm().is_finite() && direction.norm() > 0.0 {
        Ok(())
    } else {
        Err(CodecError::malformed(format_args!(
            "SLDPRT feature {feature} has a degenerate {role}"
        )))
    }
}

pub(super) fn require_count(count: u32, feature: &FeatureId) -> Result<(), CodecError> {
    if count > 0 {
        Ok(())
    } else {
        Err(CodecError::malformed(format_args!(
            "SLDPRT feature {feature} has a zero pattern count"
        )))
    }
}
