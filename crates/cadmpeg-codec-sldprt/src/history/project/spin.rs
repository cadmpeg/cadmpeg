// SPDX-License-Identifier: Apache-2.0
//! Rib, loft, sweep, and revolve projection.

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

use crate::history::classify::{feature_input_class, loft_op};
use crate::history::literals::{
    parse_angle_rad, parse_bool, parse_boolean_op, parse_point3_mm, parse_positive_angle_rad,
    parse_positive_length_mm, parse_valid_direction,
};

pub(crate) fn project_rib(
    feature: &Feature,
    native_by_source: &HashMap<&str, &str>,
) -> FeatureDefinition {
    let profile = feature.properties.get("Profile").map(|profile| {
        ProfileRef::Native(
            native_by_source
                .get(profile.as_str())
                .map_or_else(|| profile.clone(), |id| (*id).to_string()),
        )
    });
    let direction = feature
        .properties
        .get("Direction")
        .and_then(|value| parse_valid_direction(value));
    let draft = match feature.parameters.get("Draft") {
        Some(value) => parse_angle_rad(value)
            .map(Angle)
            .map_or(RibDraft::Unresolved, RibDraft::Angle),
        None => RibDraft::None,
    };
    FeatureDefinition::Rib {
        construction: RibConstruction {
            profile,
            direction,
            thickness: feature
                .parameters
                .get("Thickness")
                .or_else(|| feature.parameters.get("D1"))
                .and_then(|value| parse_positive_length_mm(value))
                .map(Length),
            side: feature
                .properties
                .get("BothSides")
                .and_then(|value| parse_bool(value))
                .map(|both_sides| {
                    if both_sides {
                        RibSide::Centered
                    } else {
                        RibSide::OneSided
                    }
                }),
            draft,
        },
        op: feature
            .properties
            .get("Operation")
            .and_then(|value| parse_boolean_op(value))
            .unwrap_or(BooleanOp::Unresolved),
    }
}

pub(crate) fn project_loft(
    feature: &Feature,
    native_by_source: &HashMap<&str, &str>,
) -> Option<FeatureDefinition> {
    let sections = feature.properties.get("Profiles").map_or_else(
        || Some(Vec::new()),
        |value| {
            Some(
                resolve_native_refs(value, native_by_source)?
                    .into_iter()
                    .map(|profile| {
                        cadmpeg_ir::features::LoftSection::Profile(ProfileRef::Native(profile))
                    })
                    .collect::<Vec<_>>(),
            )
        },
    )?;
    let guides = feature.properties.get("Guides").map_or_else(
        || Some(Vec::new()),
        |value| resolve_native_refs(value, native_by_source),
    )?;
    Some(FeatureDefinition::Loft {
        sections,
        guides: guides.into_iter().map(PathRef::Native).collect(),
        centerline: None,
        op: feature
            .properties
            .get("Operation")
            .and_then(|operation| parse_boolean_op(operation))
            .or_else(|| {
                matches!(
                    feature
                        .input_class
                        .as_deref()
                        .map(native_object_class)
                        .map(|class| class.kind),
                    Some(NativeClassKind::LoftCut)
                )
                .then_some(BooleanOp::Cut)
            })
            .or_else(|| loft_op(&feature.kind))
            .unwrap_or(BooleanOp::Unresolved),
        closed: feature
            .properties
            .get("Closed")
            .map_or(Some(false), |closed| parse_bool(closed))?,
        solid: !matches!(
            feature
                .input_class
                .as_deref()
                .map(native_object_class)
                .map(|class| class.kind),
            Some(NativeClassKind::SurfaceLoft)
        ),
        ruled: false,
        linearize: false,
        max_degree: None,
        check_compatibility: None,
        allow_multi_profile_faces: None,
    })
}

pub(crate) fn resolve_native_refs(
    value: &str,
    native_by_source: &HashMap<&str, &str>,
) -> Option<Vec<String>> {
    value
        .split(',')
        .map(str::trim)
        .filter(|source| !source.is_empty())
        .map(|source| {
            Some(
                native_by_source
                    .get(source)
                    .map_or_else(|| source.to_string(), |id| (*id).to_string()),
            )
        })
        .collect()
}

pub(crate) fn project_sweep(
    feature: &Feature,
    native_by_source: &HashMap<&str, &str>,
) -> Option<FeatureDefinition> {
    let native_ref = |source: &String| {
        native_by_source
            .get(source.as_str())
            .map_or_else(|| source.clone(), |id| (*id).to_string())
    };
    let profile = feature
        .properties
        .get("Profile")
        .map(|source| ProfileRef::Native(native_ref(source)));
    let path = feature
        .properties
        .get("Path")
        .map(|source| PathRef::Native(native_ref(source)));
    let mode = if feature_input_class(feature, NativeClassKind::SweepReferenceSurface)
        || feature.xml_tag == "Surface-Sweep"
        || feature.kind == "Surface-Sweep"
    {
        SweepMode::Surface
    } else if feature_input_class(feature, NativeClassKind::Sweep)
        || feature_input_class(feature, NativeClassKind::SweepCut)
    {
        SweepMode::Solid {
            op: feature_sweep_operation(feature),
        }
    } else if let Some(op) = feature
        .properties
        .get("Operation")
        .and_then(|value| parse_boolean_op(value))
    {
        SweepMode::Solid { op }
    } else {
        SweepMode::Unresolved
    };
    let twist = match feature.parameters.get("Twist") {
        Some(value) => Some(Angle(parse_angle_rad(value)?)),
        None => None,
    };
    let scale = match feature.parameters.get("Scale") {
        Some(value) => Some(
            value
                .trim()
                .parse::<f64>()
                .ok()
                .filter(|value| value.is_finite() && *value > 0.0)?,
        ),
        None => None,
    };
    Some(FeatureDefinition::Sweep {
        section: profile.map_or(
            cadmpeg_ir::features::SweepSection::Unresolved(None),
            cadmpeg_ir::features::SweepSection::Profile,
        ),
        sections: Vec::new(),
        path,
        mode,
        orientation: None,
        transition: None,
        transformation: None,
        path_tangent: false,
        linearize: false,
        twist,
        path_extent: None,
        guide_rail: None,
        taper: None,
        scale,
        allow_multi_profile_faces: None,
    })
}

pub(crate) fn feature_sweep_operation(feature: &Feature) -> BooleanOp {
    feature
        .properties
        .get("Operation")
        .and_then(|value| parse_boolean_op(value))
        .or_else(|| {
            matches!(
                feature
                    .input_class
                    .as_deref()
                    .map(native_object_class)
                    .map(|class| class.kind),
                Some(NativeClassKind::SweepCut)
            )
            .then_some(BooleanOp::Cut)
        })
        .or_else(|| {
            feature
                .kind
                .eq_ignore_ascii_case("Cut-Sweep")
                .then_some(BooleanOp::Cut)
        })
        .unwrap_or(BooleanOp::Unresolved)
}

pub(crate) fn project_revolve(
    feature: &Feature,
    native_by_source: &HashMap<&str, &str>,
) -> FeatureDefinition {
    let ordered_angle = |ordinal| {
        feature
            .content
            .iter()
            .filter_map(|content| match content {
                FeatureContent::Dimension(name) => feature.parameters.get(name),
                FeatureContent::Feature(_) | FeatureContent::Text(_) => None,
            })
            .filter_map(|value| parse_positive_angle_rad(value))
            .nth(ordinal)
    };
    let angle = |name, ordinal| {
        feature
            .parameters
            .get(name)
            .or_else(|| match name {
                "Angle" => feature.parameters.get("D1"),
                "Angle2" => feature.parameters.get("D2"),
                _ => None,
            })
            .and_then(|value| parse_positive_angle_rad(value))
            .or_else(|| ordered_angle(ordinal))
            .map(Angle)
    };
    let extent = match feature.properties.get("EndCondition").map(String::as_str) {
        None | Some("OneSided") => angle("Angle", 0).map(|angle| RevolveExtent::OneSided {
            termination: Termination::Angle { angle },
        }),
        Some("Symmetric") => angle("Angle", 0).map(|angle| RevolveExtent::Symmetric {
            termination: Termination::Angle { angle },
        }),
        Some("TwoSided") => angle("Angle", 0)
            .zip(angle("Angle2", 1))
            .map(|(first, second)| RevolveExtent::TwoSided {
                first: Termination::Angle { angle: first },
                second: Termination::Angle { angle: second },
            }),
        Some(_) => None,
    };
    let profile = feature.properties.get("Profile").and_then(|source| {
        native_by_source
            .get(source.as_str())
            .map(|id| ProfileRef::Native((*id).to_string()))
    });
    let axis = feature
        .properties
        .get("AxisOrigin")
        .and_then(|value| parse_point3_mm(value))
        .zip(
            feature
                .properties
                .get("AxisDirection")
                .and_then(|value| parse_valid_direction(value)),
        )
        .map(|(origin, direction)| RevolutionAxis { origin, direction });
    let op = feature
        .properties
        .get("Operation")
        .and_then(|value| parse_boolean_op(value))
        .or_else(|| {
            (feature.input_class.as_deref() == Some("moRevCut_c")).then_some(BooleanOp::Cut)
        })
        .unwrap_or(BooleanOp::Unresolved);
    FeatureDefinition::Revolve {
        construction: RevolutionConstruction {
            profile,
            axis,
            extent,
            axis_reference: None,
            solid: Some(true),
            face_maker_class: None,
            fuse_order: None,
            allow_multi_profile_faces: None,
        },
        op,
    }
}
