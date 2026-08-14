// SPDX-License-Identifier: Apache-2.0
//! Split-face, cosmetic-thread, and sketch-block projection.

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
    parse_angle_rad, parse_dimension_display_length, parse_point3_mm,
    parse_positive_dimension_length_mm, strip_diameter_modifier,
};

pub(crate) fn project_split_face(feature: &Feature) -> Option<FeatureDefinition> {
    if feature.input_class.as_deref() != Some("moPLine_c")
        || feature
            .properties
            .get(crate::resolved_features::operations::SPLIT_LINE_MODE_PROPERTY)
            .map(String::as_str)
            != Some(crate::resolved_features::operations::SPLIT_LINE_PROJECTION_MODE)
    {
        return None;
    }
    let native = feature
        .properties
        .get(crate::resolved_features::operations::SPLIT_LINE_TOOL_PROPERTY)
        .map(String::as_str)?;
    Some(FeatureDefinition::SplitFace {
        targets: FaceSelection::Unresolved,
        tool: SplitFaceTool::Path(PathRef::Native(native.into())),
    })
}

pub(crate) fn project_cosmetic_thread(feature: &Feature) -> FeatureDefinition {
    let diameter = feature
        .parameters
        .get("D2")
        .and_then(|value| parse_dimension_display_length(value))
        .or_else(|| {
            let mut tagged = feature
                .parameters
                .values()
                .filter(|value| strip_diameter_modifier(value).is_some())
                .filter_map(|value| parse_dimension_display_length(value));
            let diameter = tagged.next()?;
            tagged.next().is_none().then_some(diameter)
        })
        .filter(|value| *value > 0.0)
        .map(Length);
    let extent = match feature.parameters.get("D1") {
        Some(value) => parse_positive_dimension_length_mm(value)
            .map(|length| CosmeticThreadExtent::Blind {
                length: Length(length),
            })
            .or_else(|| {
                (parse_angle_rad(value).is_some()
                    || parse_dimension_display_length(value) == Some(0.0))
                .then_some(CosmeticThreadExtent::Through)
            }),
        None => Some(CosmeticThreadExtent::Through),
    };
    FeatureDefinition::CosmeticThread {
        face: feature
            .properties
            .get("Face")
            .cloned()
            .map_or(FaceSelection::Unresolved, FaceSelection::Native),
        diameter,
        extent,
    }
}

pub(crate) fn sketch_block_placement(feature: &Feature) -> Option<Transform> {
    let origin = parse_point3_mm(feature.properties.get("BlockOrigin")?)?;
    let mut placement = Transform::identity();
    placement.rows[0][3] = origin.x;
    placement.rows[1][3] = origin.y;
    placement.rows[2][3] = origin.z;
    Some(placement)
}
