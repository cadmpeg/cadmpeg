// SPDX-License-Identifier: Apache-2.0
//! Surface-feature projection.

#![allow(unused_imports)]
use crate::classification::{
    classify, classify_type_token, classify_xml_element, native_object_class,
    principal_plane_with_siblings, FeatureClass, NativeClassKind,
};
use crate::records::{Configuration, Feature, FeatureContent, FeatureHistory, HistoryContent};
use cadmpeg_core::decode::View;
use cadmpeg_core::CodecError;
use cadmpeg_ir::annotations::Annotations;
use cadmpeg_ir::attributes::{AttributeTarget, AttributeValue, SourceAttribute};
use cadmpeg_ir::features::{
    Angle, AxisAngle, BodyRetentionMode, BodySelection, BooleanOp, ChamferForm, ChamferSpec,
    ConfigurationBodies, ConfigurationId, CosmeticThreadExtent, CurveProjectionDirection,
    CurveProjectionDirectionState, DatumPlaneReference, DesignConfiguration, DesignParameter,
    DimensionDisplay, EdgeSelection, ExtrudeExtent, ExtrudeSide, FaceMotion, FaceSelection,
    FeatureDefinition, FeatureId, FeatureSourceContent, FeatureTreeNodeRole, FlexForm, FlexMode,
    HoleBottom, HoleForm, HoleKind, Length, ParameterId, ParameterValue, PathRef, PatternForm,
    PatternKind, PatternSeed, ProfileRef, RadiusForm, RadiusSpec, RevolutionAxis,
    RevolutionConstruction, RevolveExtent, RibConstruction, RibDraft, RibSide, RuledSurfaceMode,
    ScaleCenter, ScaleFactors, SketchSpace, SplitFaceTool, SurfaceExtension, SweepMode,
    Termination, TrimRegion, VariableRadius, VertexSelection, WrapMode,
};
use cadmpeg_ir::geometry::{Curve, Surface, SurfaceGeometry};
use cadmpeg_ir::ids::AttributeId;
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::topology::{Body, Edge, Face};
use cadmpeg_ir::transform::Transform;
use cadmpeg_ir::Exactness;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fmt::Write as _;

use crate::history::literals::{
    parse_bool, parse_length_mm, parse_positive_length_mm, parse_valid_direction,
};

pub(crate) fn project_offset_surface(feature: &Feature) -> FeatureDefinition {
    FeatureDefinition::OffsetSurface {
        faces: feature
            .properties
            .get("Faces")
            .cloned()
            .map_or(FaceSelection::Unresolved, FaceSelection::Native),
        distance: feature
            .parameters
            .get("Distance")
            .or_else(|| feature.parameters.get("D1"))
            .and_then(|value| parse_length_mm(value))
            .map(Length),
    }
}

pub(crate) fn project_knit_surface(feature: &Feature) -> FeatureDefinition {
    let gap_tolerance = match feature.parameters.get("GapTolerance") {
        Some(value) => parse_length_mm(value)
            .filter(|value| *value >= 0.0)
            .map(Length),
        None => None,
    };
    FeatureDefinition::KnitSurface {
        faces: feature
            .properties
            .get("Faces")
            .cloned()
            .map_or(FaceSelection::Unresolved, FaceSelection::Native),
        merge_entities: feature
            .properties
            .get("MergeEntities")
            .and_then(|value| parse_bool(value)),
        create_solid: feature
            .properties
            .get("CreateSolid")
            .and_then(|value| parse_bool(value)),
        gap_tolerance,
    }
}

pub(crate) fn project_filled_surface(feature: &Feature) -> FeatureDefinition {
    let continuity = feature
        .properties
        .get("Continuity")
        .and_then(|value| crate::feature_schema::parse_surface_continuity(value));
    FeatureDefinition::FilledSurface {
        boundary: cadmpeg_ir::features::SurfaceBoundary::Edges(
            feature
                .properties
                .get("Boundary")
                .cloned()
                .map_or(EdgeSelection::Unresolved, EdgeSelection::Native),
        ),
        support_faces: feature
            .properties
            .get("SupportFaces")
            .cloned()
            .map_or(FaceSelection::Unresolved, FaceSelection::Native),
        continuity,
        boundary_continuities: Vec::new(),
        merge_result: feature
            .properties
            .get("MergeResult")
            .and_then(|value| parse_bool(value)),
    }
}

pub(crate) fn project_trim_surface(
    feature: &Feature,
    native_by_source: &HashMap<&str, &str>,
) -> FeatureDefinition {
    let tool = feature.properties.get("Tool").map_or_else(
        || PathRef::Unresolved(format!("{}:tool", feature.id)),
        |tool| {
            PathRef::Native(
                native_by_source
                    .get(tool.as_str())
                    .map_or_else(|| tool.clone(), |id| (*id).to_string()),
            )
        },
    );
    FeatureDefinition::TrimSurface {
        faces: feature
            .properties
            .get("Faces")
            .cloned()
            .map_or(FaceSelection::Unresolved, FaceSelection::Native),
        tool,
        keep: feature
            .properties
            .get("Keep")
            .and_then(|value| crate::feature_schema::parse_trim_region(value))
            .unwrap_or(TrimRegion::Unresolved),
        cell_selection: None,
    }
}

pub(crate) fn project_extend_surface(feature: &Feature) -> FeatureDefinition {
    FeatureDefinition::ExtendSurface {
        faces: feature
            .properties
            .get("Faces")
            .cloned()
            .map_or(FaceSelection::Unresolved, FaceSelection::Native),
        distance: feature
            .parameters
            .get("Distance")
            .or_else(|| feature.parameters.get("D1"))
            .and_then(|value| parse_positive_length_mm(value))
            .map(Length),
        method: feature
            .properties
            .get("Method")
            .and_then(|value| crate::feature_schema::parse_surface_extension(value))
            .unwrap_or(SurfaceExtension::Unresolved),
    }
}

pub(crate) fn project_ruled_surface(feature: &Feature) -> Option<FeatureDefinition> {
    let distance = Length(parse_positive_length_mm(
        feature
            .parameters
            .get("Distance")
            .or_else(|| feature.parameters.get("D1"))?,
    )?);
    let mode = match feature
        .properties
        .get("Mode")?
        .to_ascii_lowercase()
        .as_str()
    {
        "normal" => RuledSurfaceMode::Normal { distance },
        "tangent" => RuledSurfaceMode::Tangent { distance },
        "direction" => RuledSurfaceMode::Direction {
            direction: parse_valid_direction(feature.properties.get("Direction")?)?,
            distance,
        },
        _ => return None,
    };
    Some(FeatureDefinition::RuledSurface {
        edges: EdgeSelection::Native(feature.properties.get("Edges")?.clone()),
        support_faces: FaceSelection::Native(feature.properties.get("SupportFaces")?.clone()),
        mode,
        angle: None,
        alternate_face: None,
        corner: None,
    })
}
