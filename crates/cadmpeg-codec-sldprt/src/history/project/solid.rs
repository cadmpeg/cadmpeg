// SPDX-License-Identifier: Apache-2.0
//! Extrude and hole projection.

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

use super::resolve_native_refs;
use crate::history::classify::extrude_feature_op;
use crate::history::literals::{
    parse_angle_rad, parse_boolean_op, parse_bounded_angle_rad, parse_dimension_display_length,
    parse_point3_mm, parse_positive_dimension_length_mm, parse_positive_length_mm, parse_vector3,
    strip_diameter_modifier, valid_direction,
};

pub(crate) fn project_extrude(
    feature: &Feature,
    native_by_source: &HashMap<&str, &str>,
    features_by_source: &HashMap<&str, &Feature>,
) -> Option<FeatureDefinition> {
    let source_dimensions = feature
        .content
        .iter()
        .filter_map(|content| match content {
            FeatureContent::Dimension(name) => Some(name.as_str()),
            _ => None,
        })
        .collect::<HashSet<_>>();
    let source_depth = (source_dimensions.len() == 1)
        .then(|| source_dimensions.iter().copied().next())
        .flatten();
    let legacy_history_extrusion = feature.input_class.is_none()
        && feature.xml_tag.eq_ignore_ascii_case("Extrusion")
        && source_depth.is_some();
    let implicit_modern_blind =
        feature.input_class.as_deref() == Some("moExtrusion_c") && source_dimensions.len() == 1;
    let legacy_profile = legacy_history_extrusion
        .then(|| {
            let source = feature.source_id.as_deref()?.parse::<i64>().ok()?;
            features_by_source
                .iter()
                .filter_map(|(candidate_source, candidate)| {
                    let candidate_source = candidate_source.parse::<i64>().ok()?;
                    (candidate_source < source
                        && classify(candidate) == Some(FeatureClass::Sketch)
                        && candidate.input_class.as_deref() != Some("moOriginProfileFeature_c"))
                    .then_some((candidate_source, candidate.id.as_str()))
                })
                .max_by_key(|candidate| candidate.0)
                .map(|(_, profile)| profile.to_string())
        })
        .flatten();
    let op = feature
        .properties
        .get("Operation")
        .and_then(|value| parse_boolean_op(value))
        .or_else(|| extrude_feature_op(feature))
        .or_else(|| legacy_profile.is_some().then_some(BooleanOp::Join))
        .unwrap_or(BooleanOp::Unresolved);
    let sole_length = || {
        let mut values = feature.parameters.values();
        let sole = values.next().filter(|_| values.next().is_none())?;
        parse_positive_length_mm(sole)
            .or_else(|| parse_positive_dimension_length_mm(sole))
            .map(Length)
    };
    let legacy_length = || {
        source_depth
            .and_then(|name| feature.parameters.get(name))
            .and_then(|value| {
                parse_positive_length_mm(value)
                    .or_else(|| parse_positive_dimension_length_mm(value))
            })
            .map(Length)
    };
    let length = |name| {
        feature
            .parameters
            .get(name)
            .and_then(|value| parse_positive_length_mm(value))
            .or_else(|| {
                (name == "Depth")
                    .then(|| feature.parameters.get("D1"))
                    .flatten()
                    .and_then(|value| parse_positive_dimension_length_mm(value))
            })
            .map(Length)
    };
    let draft = match feature.parameters.get("Draft") {
        Some(value) => Some(Angle(parse_angle_rad(value)?)),
        None => None,
    };
    let one_sided = |termination| ExtrudeExtent::OneSided {
        side: ExtrudeSide {
            termination,
            draft,
            offset: None,
        },
    };
    let extent = match feature.properties.get("EndCondition").map(String::as_str) {
        None if !feature.parameters.contains_key("Depth")
            && !feature.parameters.contains_key("D1")
            && !legacy_history_extrusion
            && !implicit_modern_blind =>
        {
            one_sided(Termination::Unresolved)
        }
        None | Some("Blind") => match length("Depth")
            .or_else(|| legacy_history_extrusion.then(legacy_length).flatten())
            .or_else(sole_length)
        {
            Some(length) => one_sided(Termination::Blind { length }),
            None => one_sided(Termination::Unresolved),
        },
        Some("Symmetric") => match length("Depth").or_else(sole_length) {
            Some(length) => ExtrudeExtent::Symmetric {
                side: ExtrudeSide {
                    termination: Termination::Blind { length },
                    draft,
                    offset: None,
                },
            },
            None => one_sided(Termination::Unresolved),
        },
        Some("TwoSided") => ExtrudeExtent::TwoSided {
            first: ExtrudeSide {
                termination: Termination::Blind {
                    length: length("Depth")?,
                },
                draft,
                offset: None,
            },
            second: ExtrudeSide {
                termination: Termination::Blind {
                    length: length("Depth2")?,
                },
                draft: None,
                offset: None,
            },
        },
        Some("ThroughAll") => one_sided(Termination::ThroughAll),
        Some("ThroughAllBoth") => ExtrudeExtent::TwoSided {
            first: ExtrudeSide {
                termination: Termination::ThroughAll,
                draft,
                offset: None,
            },
            second: ExtrudeSide {
                termination: Termination::ThroughAll,
                draft: None,
                offset: None,
            },
        },
        Some("ThroughNext") => one_sided(Termination::ThroughNext),
        Some("ToFace") => one_sided(Termination::ToFace {
            face: FaceSelection::Native(feature.properties.get("Face")?.clone()),
            offset: None,
        }),
        Some("ToVertex") => one_sided(Termination::ToVertex {
            vertex: VertexSelection::Native(feature.properties.get("Vertex")?.clone()),
        }),
        Some("OffsetFromFace") => match length("Depth").or_else(sole_length) {
            Some(offset) => one_sided(Termination::OffsetFromFace {
                face: FaceSelection::Native(feature.properties.get("Face")?.clone()),
                offset,
            }),
            None => one_sided(Termination::Unresolved),
        },
        Some(_) => one_sided(Termination::Unresolved),
    };
    let direction = match feature.properties.get("Direction") {
        Some(value) => cadmpeg_ir::features::ExtrudeDirection::Explicit(parse_vector3(value)?),
        None => cadmpeg_ir::features::ExtrudeDirection::ProfileNormal,
    };
    if matches!(direction, cadmpeg_ir::features::ExtrudeDirection::Explicit(value) if !valid_direction(value))
    {
        return None;
    }
    let profile = if let Some(source) = feature.properties.get("Profile") {
        ProfileRef::Native(
            native_by_source
                .get(source.as_str())
                .map_or_else(|| source.clone(), |id| (*id).to_string()),
        )
    } else if let Some(children) = feature.properties.get("DissectableChildren") {
        let profiles = resolve_native_refs(children, native_by_source)?;
        match profiles.as_slice() {
            [profile] => ProfileRef::Native(profile.clone()),
            _ => ProfileRef::Unresolved(feature.id.clone()),
        }
    } else if let Some(profile) = legacy_profile {
        ProfileRef::Native(profile)
    } else {
        ProfileRef::Unresolved(feature.id.clone())
    };
    Some(FeatureDefinition::Extrude {
        profile,
        direction,
        start: cadmpeg_ir::features::ExtrudeStart::ProfilePlane,
        extent,
        op,
        direction_source: None,
        solid: Some(!matches!(
            feature
                .input_class
                .as_deref()
                .map(native_object_class)
                .map(|class| class.kind),
            Some(NativeClassKind::SurfaceExtrusion)
        )),
        face_maker: None,
        inner_wire_taper: None,
        length_along_profile_normal: None,
        allow_multi_profile_faces: None,
    })
}

pub(crate) fn project_hole(
    feature: &Feature,
    features_by_source: &HashMap<&str, &Feature>,
    history_features: &[Feature],
) -> FeatureDefinition {
    let profile = hole_profile_construction(feature, features_by_source, history_features);
    let diameter = feature
        .parameters
        .get("Diameter")
        .and_then(|value| parse_positive_length_mm(value))
        .map(Length)
        .or_else(|| profile.as_ref().map(|profile| profile.diameter));
    let has_counterbore = feature.parameters.contains_key("CounterboreDiameter")
        || feature.parameters.contains_key("CounterboreDepth");
    let has_countersink = feature.parameters.contains_key("CountersinkDiameter")
        || feature.parameters.contains_key("CountersinkAngle");
    let counterbore_diameter = feature
        .parameters
        .get("CounterboreDiameter")
        .and_then(|value| parse_positive_length_mm(value))
        .map(Length);
    let counterbore_depth = feature
        .parameters
        .get("CounterboreDepth")
        .and_then(|value| parse_positive_length_mm(value))
        .map(Length);
    let countersink_diameter = feature
        .parameters
        .get("CountersinkDiameter")
        .and_then(|value| parse_positive_length_mm(value))
        .map(Length);
    let countersink_angle = feature
        .parameters
        .get("CountersinkAngle")
        .and_then(|value| parse_bounded_angle_rad(value))
        .map(Angle);
    let drill_point_angle = feature
        .parameters
        .get("DrillPointAngle")
        .and_then(|value| parse_bounded_angle_rad(value))
        .map(Angle);
    let thread = feature
        .parameters
        .get("ThreadMajorDiameter")
        .and_then(|value| parse_positive_length_mm(value))
        .map(Length)
        .zip(
            feature
                .parameters
                .get("ThreadDepth")
                .and_then(|value| parse_positive_length_mm(value))
                .map(Length),
        )
        .zip(drill_point_angle)
        .map(
            |((major_diameter, thread_depth), drill_point_angle)| HoleKind::Threaded {
                major_diameter,
                thread_depth,
                pitch: feature
                    .parameters
                    .get("ThreadPitch")
                    .and_then(|value| parse_positive_length_mm(value))
                    .map(Length),
                drill_point_angle,
            },
        );
    let kind = if has_counterbore && has_countersink {
        HoleKind::Unresolved {
            form: None,
            counterbore_diameter,
            counterbore_depth,
            countersink_diameter,
            countersink_angle,
        }
    } else if has_counterbore {
        match (counterbore_diameter, counterbore_depth) {
            (Some(diameter), Some(depth)) => drill_point_angle.map_or(
                HoleKind::Counterbore { diameter, depth },
                |drill_point_angle| HoleKind::CounterboreDrilled {
                    diameter,
                    depth,
                    drill_point_angle,
                },
            ),
            (diameter, depth) => HoleKind::Unresolved {
                form: Some(HoleForm::Counterbore),
                counterbore_diameter: diameter,
                counterbore_depth: depth,
                countersink_diameter: None,
                countersink_angle: None,
            },
        }
    } else if has_countersink {
        match (countersink_diameter, countersink_angle) {
            (Some(diameter), Some(angle)) => HoleKind::Countersink { diameter, angle },
            (diameter, angle) => HoleKind::Unresolved {
                form: Some(HoleForm::Countersink),
                counterbore_diameter: None,
                counterbore_depth: None,
                countersink_diameter: diameter,
                countersink_angle: angle,
            },
        }
    } else if let Some(thread) = thread {
        thread
    } else if let Some(drill_point_angle) = drill_point_angle {
        HoleKind::SimpleDrilled { drill_point_angle }
    } else {
        profile
            .as_ref()
            .map_or(HoleKind::Simple, |profile| profile.kind)
    };
    let extent = match feature.properties.get("EndCondition").map(String::as_str) {
        None | Some("Blind")
            if profile
                .as_ref()
                .is_some_and(|profile| profile.exit_kind.is_some()) =>
        {
            Some(Termination::ThroughAll)
        }
        None | Some("Blind") => feature
            .parameters
            .get("Depth")
            .and_then(|value| parse_positive_length_mm(value))
            .map(Length)
            .or_else(|| profile.as_ref().and_then(|profile| profile.depth))
            .map(|length| Termination::Blind { length }),
        Some("ThroughAll") => Some(Termination::ThroughAll),
        Some(_) => None,
    };
    FeatureDefinition::Hole {
        profile: None,
        profile_filter: None,
        face: feature
            .properties
            .get("Face")
            .cloned()
            .map(FaceSelection::Native),
        position: None,
        direction: None,
        placements: feature
            .properties
            .get("Position")
            .and_then(|value| parse_point3_mm(value))
            .zip(
                feature
                    .properties
                    .get("Direction")
                    .and_then(|value| parse_vector3(value))
                    .filter(|direction| valid_direction(*direction)),
            )
            .map(|(position, direction)| {
                vec![cadmpeg_ir::features::HolePlacement::Directed {
                    position,
                    direction,
                }]
            })
            .unwrap_or_default(),
        kind,
        exit_kind: profile.as_ref().and_then(|profile| profile.exit_kind),
        diameter,
        extent,
        bottom: profile.as_ref().and_then(|profile| profile.bottom),
        taper_angle: profile.as_ref().and_then(|profile| profile.taper_angle),
        specification: None,
        allow_multi_profile_faces: None,
    }
}

pub(crate) fn threaded_hole_major_diameter(
    feature: &Feature,
    features_by_source: &HashMap<&str, &Feature>,
    history_features: &[Feature],
) -> Option<f64> {
    if classify(feature) != Some(FeatureClass::Hole) {
        return None;
    }
    let FeatureDefinition::Hole {
        kind: HoleKind::Threaded { major_diameter, .. },
        ..
    } = project_hole(feature, features_by_source, history_features)
    else {
        return None;
    };
    (major_diameter.0.is_finite() && major_diameter.0 > 0.0).then_some(major_diameter.0)
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct HoleProfileConstruction {
    pub(crate) diameter: Length,
    pub(crate) depth: Option<Length>,
    pub(crate) kind: HoleKind,
    pub(crate) exit_kind: Option<HoleKind>,
    pub(crate) bottom: Option<HoleBottom>,
    pub(crate) taper_angle: Option<Angle>,
}

pub(crate) fn hole_profile_construction(
    feature: &Feature,
    features_by_source: &HashMap<&str, &Feature>,
    history_features: &[Feature],
) -> Option<HoleProfileConstruction> {
    let children = feature.properties.get("DissectableChildren")?;
    let constructions = children
        .split(',')
        .map(str::trim)
        .filter(|source| !source.is_empty())
        .filter_map(|source| {
            features_by_source.get(source).copied().or_else(|| {
                let mut profiles = history_features
                    .iter()
                    .filter(|candidate| candidate.id == source);
                let profile = profiles.next()?;
                profiles.next().is_none().then_some(profile)
            })
        })
        .filter(|profile| classify(profile) == Some(FeatureClass::Sketch))
        .filter_map(hole_sketch_construction)
        .collect::<Vec<_>>();
    let complete = constructions
        .iter()
        .filter(|construction| construction.depth.is_some())
        .collect::<Vec<_>>();
    match complete.as_slice() {
        [construction] => Some((**construction).clone()),
        [] => match constructions.as_slice() {
            [construction] => Some(construction.clone()),
            _ => None,
        },
        _ => None,
    }
}

pub(crate) fn hole_sketch_construction(profile: &Feature) -> Option<HoleProfileConstruction> {
    #[derive(Clone, Copy, PartialEq, Eq)]
    enum DimensionRole {
        Diameter,
        Length,
        Angle,
    }

    let mut diameters = Vec::new();
    let mut lengths = Vec::new();
    let mut angles = Vec::new();
    let mut roles = Vec::new();
    let source_dimensions = profile
        .content
        .iter()
        .filter_map(|content| match content {
            crate::records::FeatureContent::Dimension(name) => Some(name.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>();
    let expressions = if source_dimensions.is_empty() {
        profile.parameters.values().collect::<Vec<_>>()
    } else {
        source_dimensions
            .into_iter()
            .filter_map(|name| profile.parameters.get(name))
            .collect::<Vec<_>>()
    };
    for expression in expressions {
        if strip_diameter_modifier(expression).is_some() {
            if let Some(value) = parse_dimension_display_length(expression)
                .filter(|value| *value > 0.0)
                .map(Length)
            {
                diameters.push(value);
                roles.push(DimensionRole::Diameter);
            }
        } else if let Some(value) = parse_bounded_angle_rad(expression).map(Angle) {
            angles.push(value);
            roles.push(DimensionRole::Angle);
        } else if let Some(value) = parse_positive_dimension_length_mm(expression).map(Length) {
            lengths.push(value);
            roles.push(DimensionRole::Length);
        }
    }
    diameters.sort_by(|left, right| left.0.total_cmp(&right.0));
    lengths.sort_by(|left, right| left.0.total_cmp(&right.0));
    angles.sort_by(|left, right| left.0.total_cmp(&right.0));
    match (diameters.as_slice(), lengths.as_slice(), angles.as_slice()) {
        ([diameter], [depth], []) => Some(HoleProfileConstruction {
            diameter: *diameter,
            depth: Some(*depth),
            kind: HoleKind::Simple,
            exit_kind: None,
            bottom: Some(HoleBottom::Flat),
            taper_angle: None,
        }),
        ([diameter], [depth], [drill_point_angle]) => Some(HoleProfileConstruction {
            diameter: *diameter,
            depth: Some(*depth),
            kind: HoleKind::SimpleDrilled {
                drill_point_angle: *drill_point_angle,
            },
            exit_kind: None,
            bottom: Some(HoleBottom::Angled {
                included_angle: *drill_point_angle,
                depth_to_tip: false,
            }),
            taper_angle: None,
        }),
        ([diameter, major_diameter], [thread_depth, drill_depth], [drill_point_angle])
            if roles
                == [
                    DimensionRole::Diameter,
                    DimensionRole::Length,
                    DimensionRole::Diameter,
                    DimensionRole::Length,
                    DimensionRole::Angle,
                ]
                && diameter.0 < major_diameter.0
                && thread_depth.0 < drill_depth.0 =>
        {
            Some(HoleProfileConstruction {
                diameter: *diameter,
                depth: Some(*drill_depth),
                kind: HoleKind::Threaded {
                    major_diameter: *major_diameter,
                    thread_depth: *thread_depth,
                    pitch: None,
                    drill_point_angle: *drill_point_angle,
                },
                exit_kind: None,
                bottom: Some(HoleBottom::Angled {
                    included_angle: *drill_point_angle,
                    depth_to_tip: false,
                }),
                taper_angle: None,
            })
        }
        (
            [diameter, major_diameter],
            [thread_depth, drill_depth],
            [taper_angle, drill_point_angle],
        ) if diameter.0 < major_diameter.0
            && thread_depth.0 < drill_depth.0
            && taper_angle.0 < drill_point_angle.0 =>
        {
            Some(HoleProfileConstruction {
                diameter: *diameter,
                depth: Some(*drill_depth),
                kind: HoleKind::Threaded {
                    major_diameter: *major_diameter,
                    thread_depth: *thread_depth,
                    pitch: None,
                    drill_point_angle: *drill_point_angle,
                },
                exit_kind: None,
                bottom: Some(HoleBottom::Angled {
                    included_angle: *drill_point_angle,
                    depth_to_tip: false,
                }),
                taper_angle: Some(*taper_angle),
            })
        }
        ([diameter, entry_diameter], [entry_depth, depth], [drill_point_angle])
            if roles.last() == Some(&DimensionRole::Diameter)
                && diameter.0 < entry_diameter.0
                && entry_depth.0 < depth.0 =>
        {
            Some(HoleProfileConstruction {
                diameter: *diameter,
                depth: Some(*depth),
                kind: HoleKind::CounterboreDrilled {
                    diameter: *entry_diameter,
                    depth: *entry_depth,
                    drill_point_angle: *drill_point_angle,
                },
                exit_kind: None,
                bottom: Some(HoleBottom::Angled {
                    included_angle: *drill_point_angle,
                    depth_to_tip: false,
                }),
                taper_angle: None,
            })
        }
        (
            [diameter, exit_diameter, counterbore_diameter],
            [counterbore_depth, through_depth],
            [exit_angle],
        ) if roles
            == [
                DimensionRole::Length,
                DimensionRole::Diameter,
                DimensionRole::Angle,
                DimensionRole::Length,
                DimensionRole::Diameter,
                DimensionRole::Diameter,
            ]
            && diameter.0 < exit_diameter.0
            && exit_diameter.0 < counterbore_diameter.0
            && counterbore_depth.0 < through_depth.0 =>
        {
            Some(HoleProfileConstruction {
                diameter: *diameter,
                depth: Some(*through_depth),
                kind: HoleKind::Counterbore {
                    diameter: *counterbore_diameter,
                    depth: *counterbore_depth,
                },
                exit_kind: Some(HoleKind::Countersink {
                    diameter: *exit_diameter,
                    angle: *exit_angle,
                }),
                bottom: None,
                taper_angle: None,
            })
        }
        (
            [diameter, recess_diameter, entry_diameter],
            [recess_depth, drill_depth],
            [entry_angle, drill_point_angle],
        ) if diameter.0 < recess_diameter.0
            && recess_diameter.0 < entry_diameter.0
            && recess_depth.0 < drill_depth.0
            && entry_angle.0 < drill_point_angle.0 =>
        {
            Some(HoleProfileConstruction {
                diameter: *diameter,
                depth: Some(*drill_depth),
                kind: HoleKind::Counterdrill {
                    diameter: *recess_diameter,
                    entry_diameter: Some(*entry_diameter),
                    depth: *recess_depth,
                    angle: *entry_angle,
                },
                exit_kind: None,
                bottom: Some(HoleBottom::Angled {
                    included_angle: *drill_point_angle,
                    depth_to_tip: false,
                }),
                taper_angle: None,
            })
        }
        _ => None,
    }
}

pub(crate) fn is_hole_profile_construction(feature: &Feature) -> bool {
    hole_sketch_construction(feature).is_some()
}
