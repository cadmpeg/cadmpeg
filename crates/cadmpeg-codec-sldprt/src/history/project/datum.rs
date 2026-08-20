// SPDX-License-Identifier: Apache-2.0
//! Datum, curve, helix, and wrap projection.

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
use cadmpeg_ir::math::Vector3;
use cadmpeg_ir::topology::{Body, Edge, Face};
use cadmpeg_ir::transform::Transform;
use cadmpeg_ir::Exactness;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fmt::Write as _;

use crate::history::literals::{
    parse_angle_rad, parse_bool, parse_dimension_length_mm, parse_length_mm, parse_point3_mm,
    parse_positive_length_mm, parse_valid_direction, parse_vector3, valid_coordinate_frame,
    valid_direction, valid_plane_frame,
};

pub(crate) fn project_datum_plane(feature: &Feature) -> Option<FeatureDefinition> {
    let origin = parse_point3_mm(feature.properties.get("Origin")?)?;
    let normal = parse_vector3(feature.properties.get("Normal")?)?;
    let u_axis = parse_vector3(feature.properties.get("UAxis")?)?;
    valid_plane_frame(normal, u_axis).then_some(FeatureDefinition::DatumPlane {
        origin,
        normal,
        u_axis,
    })
}

pub(crate) fn project_offset_plane(
    feature: &Feature,
    by_source: &HashMap<&str, FeatureId>,
) -> Option<FeatureDefinition> {
    let distance = Length(parse_dimension_length_mm(feature.parameters.get("D1")?)?);
    let reference = feature
        .properties
        .get("Reference")
        .or_else(|| feature.properties.get("Plane"))
        .and_then(|source| by_source.get(source.as_str()).cloned())
        .map(DatumPlaneReference::Feature)
        .or_else(|| {
            Some(DatumPlaneReference::Face {
                face: FaceSelection::Unresolved,
                origin: parse_point3_mm(feature.properties.get("ReferenceFaceOrigin")?)?,
                normal: parse_vector3(feature.properties.get("ReferenceFaceNormal")?)?,
                u_axis: parse_vector3(feature.properties.get("ReferenceFaceUAxis")?)?,
            })
        })
        .or_else(|| {
            let origin = parse_point3_mm(feature.properties.get("Origin")?)?;
            let normal = parse_vector3(feature.properties.get("Normal")?)?;
            Some(DatumPlaneReference::Face {
                face: FaceSelection::Native(feature.properties.get("ReferenceFaceNative")?.clone()),
                origin,
                normal,
                u_axis: parse_vector3(feature.properties.get("UAxis")?)?,
            })
        });
    Some(FeatureDefinition::DatumOffsetPlane {
        reference,
        distance,
    })
}

pub(crate) fn project_datum_axis(feature: &Feature) -> Option<FeatureDefinition> {
    let origin = parse_point3_mm(feature.properties.get("Origin")?)?;
    let direction = parse_vector3(feature.properties.get("Direction")?)?;
    valid_direction(direction).then_some(FeatureDefinition::DatumAxis { origin, direction })
}

pub(crate) fn project_datum_point(feature: &Feature) -> Option<FeatureDefinition> {
    Some(FeatureDefinition::DatumPoint {
        position: parse_point3_mm(feature.properties.get("Position")?)?,
        construction: None,
    })
}

pub(crate) fn project_datum_coordinate_system(feature: &Feature) -> Option<FeatureDefinition> {
    let origin = parse_point3_mm(feature.properties.get("Origin")?)?;
    let x_axis = parse_vector3(feature.properties.get("XAxis")?)?;
    let y_axis = parse_vector3(feature.properties.get("YAxis")?)?;
    let z_axis = parse_vector3(feature.properties.get("ZAxis")?)?;
    valid_coordinate_frame(origin, x_axis, y_axis, z_axis).then_some(
        FeatureDefinition::DatumCoordinateSystem {
            origin,
            x_axis,
            y_axis,
            z_axis,
        },
    )
}

pub(crate) fn project_equation_curve(feature: &Feature) -> Option<FeatureDefinition> {
    let parameter = feature.properties.get("Parameter")?.trim().to_string();
    let x_expression = feature.properties.get("XEquation")?.trim().to_string();
    let y_expression = feature.properties.get("YEquation")?.trim().to_string();
    let z_expression = feature.properties.get("ZEquation")?.trim().to_string();
    let start = feature
        .properties
        .get("Start")?
        .trim()
        .parse::<f64>()
        .ok()?;
    let end = feature.properties.get("End")?.trim().parse::<f64>().ok()?;
    (!parameter.is_empty()
        && !x_expression.is_empty()
        && !y_expression.is_empty()
        && !z_expression.is_empty()
        && start.is_finite()
        && end.is_finite()
        && start < end)
        .then_some(FeatureDefinition::EquationCurve {
            parameter,
            x_expression,
            y_expression,
            z_expression,
            start,
            end,
        })
}

pub(crate) fn project_projected_curve(
    feature: &Feature,
    native_by_source: &HashMap<&str, &str>,
) -> Option<FeatureDefinition> {
    let source = feature.properties.get("Source")?;
    let source = native_by_source
        .get(source.as_str())
        .map_or_else(|| source.clone(), |id| (*id).to_string());
    let direction = match feature.properties.get("Direction") {
        Some(value) => CurveProjectionDirection::Vector(parse_valid_direction(value)?),
        None => CurveProjectionDirection::State(CurveProjectionDirectionState::TargetNormal),
    };
    Some(FeatureDefinition::ProjectedCurve {
        source: PathRef::Native(source),
        target_faces: FaceSelection::Native(feature.properties.get("TargetFaces")?.clone()),
        direction,
        bidirectional: Some(
            feature
                .properties
                .get("Bidirectional")
                .and_then(|value| parse_bool(value))
                .unwrap_or(false),
        ),
    })
}

pub(crate) fn project_composite_curve(
    feature: &Feature,
    native_by_source: &HashMap<&str, &str>,
) -> Option<FeatureDefinition> {
    let segments = feature
        .properties
        .get("Segments")?
        .split(';')
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|source| {
            PathRef::Native(
                native_by_source
                    .get(source)
                    .map_or_else(|| source.to_string(), |id| (*id).to_string()),
            )
        })
        .collect::<Vec<_>>();
    if segments.is_empty() {
        return None;
    }
    Some(FeatureDefinition::CompositeCurve {
        segments,
        closed: feature
            .properties
            .get("Closed")
            .map_or(Some(false), |value| parse_bool(value))?,
    })
}

pub(crate) fn project_helix(feature: &Feature) -> Option<FeatureDefinition> {
    let axis_origin = parse_point3_mm(feature.properties.get("AxisOrigin")?)?;
    let axis_direction = parse_valid_direction(feature.properties.get("AxisDirection")?)?;
    let radius = parse_positive_length_mm(feature.parameters.get("Radius")?)?;
    let pitch = parse_length_mm(feature.parameters.get("Pitch")?)?;
    let revolutions = feature
        .parameters
        .get("Revolutions")?
        .trim()
        .parse::<f64>()
        .ok()
        .filter(|value| value.is_finite() && *value > 0.0)?;
    let clockwise = feature
        .properties
        .get("Clockwise")
        .and_then(|value| parse_bool(value))
        .unwrap_or(false);
    let start_angle = match feature.parameters.get("StartAngle") {
        Some(value) => parse_angle_rad(value)?,
        None => 0.0,
    };
    Some(FeatureDefinition::Helix {
        axis_origin,
        axis_direction,
        radius: Length(radius),
        pitch: Length(pitch),
        revolutions,
        start_angle: Angle(start_angle),
        clockwise,
        radial_growth: None,
        cone_angle: None,
        segment_turns: None,
        construction_style: None,
    })
}

pub(crate) fn project_native_axis_helix(feature: &Feature) -> Option<FeatureDefinition> {
    let axial_rise = parse_dimension_length_mm(feature.parameters.get("D3")?)?;
    let pitch = parse_dimension_length_mm(feature.parameters.get("D4")?)?;
    let revolutions = feature
        .parameters
        .get("D5")?
        .trim()
        .parse::<f64>()
        .ok()
        .filter(|value| value.is_finite() && *value > 0.0)?;
    let start_angle = Angle(parse_angle_rad(feature.parameters.get("D7")?)?);
    let clockwise = feature
        .properties
        .get("Clockwise")
        .and_then(|value| parse_bool(value))
        .unwrap_or(false);
    Some(FeatureDefinition::HelixNativeAxis {
        axis_native_ref: feature.id.clone(),
        axial_rise: Length(axial_rise),
        pitch: Length(pitch),
        revolutions,
        start_angle,
        clockwise,
    })
}

pub(crate) fn project_wrap(
    feature: &Feature,
    native_by_source: &HashMap<&str, &str>,
) -> Option<FeatureDefinition> {
    let profile = feature.properties.get("Profile")?;
    let profile = native_by_source
        .get(profile.as_str())
        .map_or_else(|| profile.clone(), |id| (*id).to_string());
    let face = FaceSelection::Native(feature.properties.get("Face")?.clone());
    let mode = match feature
        .properties
        .get("Mode")?
        .to_ascii_lowercase()
        .as_str()
    {
        "emboss" => WrapMode::Emboss,
        "deboss" => WrapMode::Deboss,
        "scribe" => WrapMode::Scribe,
        _ => return None,
    };
    let depth = match mode {
        WrapMode::Emboss | WrapMode::Deboss => Some(Length(parse_positive_length_mm(
            feature.parameters.get("Depth")?,
        )?)),
        WrapMode::Scribe => None,
    };
    Some(FeatureDefinition::Wrap {
        profile: ProfileRef::Native(profile),
        face,
        mode,
        depth,
    })
}
