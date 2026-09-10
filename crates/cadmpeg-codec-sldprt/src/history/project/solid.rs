// SPDX-License-Identifier: Apache-2.0
//! Extrude and hole projection.

use crate::classification::{classify, native_object_class, FeatureClass, NativeClassKind};
use crate::records::{Feature, FeatureContent};
use cadmpeg_ir::{
    features::{
        BooleanOp, ExtrudeExtent, ExtrudeSide, FaceSelection, FeatureDefinition, HoleBottom,
        HoleConstruction, HoleKind, LinearTermination, ProfileRef, VertexSelection,
    },
    scalar::Length,
};
use std::collections::{HashMap, HashSet};

use super::resolve_native_refs;
use crate::history::classify::extrude_feature_op;
use crate::history::literals::{
    parse_angle_rad, parse_boolean_op, parse_bounded_angle_rad, parse_dimension_display_length,
    parse_point3_mm, parse_positive_dimension_length_mm, parse_positive_length_mm, parse_vector3,
    strip_diameter_modifier, valid_direction,
};

pub(crate) fn project_extrude(
    feature: &Feature,
    native_by_source: &HashMap<String, &str>,
    features_by_source: &HashMap<crate::records::FeatureSource, &Feature>,
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
    // Some modern ICE extrusions retain the dissection-root marker but omit
    // both the child list and the explicit profile property. Their profile is
    // still the nearest preceding non-origin sketch in the source namespace,
    // the same ownership rule used by the legacy history form. Restrict this
    // fallback to a sole source dimension so an operation with several
    // unassigned dimensions cannot acquire a profile by proximity alone.
    let root_history_extrusion = feature
        .properties
        .get("DissectableRoot")
        .is_some_and(|value| value == "true")
        && !feature.properties.contains_key("DissectableChildren")
        && !feature.properties.contains_key("Profile")
        && source_depth.is_some();
    let history_profile_extrusion = legacy_history_extrusion || root_history_extrusion;
    let implicit_modern_blind =
        feature.input_class.as_deref() == Some("moExtrusion_c") && source_dimensions.len() == 1;
    let history_profile = history_profile_extrusion
        .then(|| {
            let source = feature.source_id?.value().map_or(-1i64, i64::from);
            features_by_source
                .iter()
                .filter_map(|(candidate_source, candidate)| {
                    let candidate_source = candidate_source.value().map_or(-1i64, i64::from);
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
        .or_else(|| {
            (legacy_history_extrusion && history_profile.is_some()).then_some(BooleanOp::Join)
        })
        .unwrap_or(BooleanOp::Unresolved);
    let sole_length = || {
        let mut values = feature.parameters.values();
        let sole = values.next().filter(|_| values.next().is_none())?;
        parse_positive_length_mm(sole)
            .or_else(|| parse_positive_dimension_length_mm(sole))
            .and_then(cadmpeg_ir::scalar::NonZeroLength::new)
    };
    let legacy_length = || {
        source_depth
            .and_then(|name| feature.parameters.get(name))
            .and_then(|value| {
                parse_positive_length_mm(value)
                    .or_else(|| parse_positive_dimension_length_mm(value))
            })
            .and_then(cadmpeg_ir::scalar::NonZeroLength::new)
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
            .and_then(cadmpeg_ir::scalar::NonZeroLength::new)
    };
    let draft = match feature.parameters.get("Draft") {
        Some(value) => Some(cadmpeg_ir::scalar::SlopeAngle::new(parse_angle_rad(
            value,
        )?)?),
        None => None,
    };
    let one_sided = |termination| ExtrudeExtent::OneSided {
        side: ExtrudeSide { termination, draft },
    };
    let extent = match feature.properties.get("EndCondition").map(String::as_str) {
        None if !feature.parameters.contains_key("Depth")
            && !feature.parameters.contains_key("D1")
            && !legacy_history_extrusion
            && !implicit_modern_blind =>
        {
            one_sided(LinearTermination::Unresolved)
        }
        None | Some("Blind") => match length("Depth")
            .or_else(|| legacy_history_extrusion.then(legacy_length).flatten())
            .or_else(sole_length)
        {
            Some(length) => one_sided(LinearTermination::Blind { length }),
            None => one_sided(LinearTermination::Unresolved),
        },
        Some("Symmetric") => match length("Depth").or_else(sole_length) {
            Some(length) => ExtrudeExtent::Symmetric {
                side: ExtrudeSide {
                    termination: LinearTermination::Blind { length },
                    draft,
                },
            },
            None => one_sided(LinearTermination::Unresolved),
        },
        Some("TwoSided") => ExtrudeExtent::TwoSided {
            first: ExtrudeSide {
                termination: LinearTermination::Blind {
                    length: length("Depth")?,
                },
                draft,
            },
            second: ExtrudeSide {
                termination: LinearTermination::Blind {
                    length: length("Depth2")?,
                },
                draft: None,
            },
        },
        Some("ThroughAll") => one_sided(LinearTermination::ThroughAll),
        Some("ThroughAllBoth") => ExtrudeExtent::TwoSided {
            first: ExtrudeSide {
                termination: LinearTermination::ThroughAll,
                draft,
            },
            second: ExtrudeSide {
                termination: LinearTermination::ThroughAll,
                draft: None,
            },
        },
        Some("ThroughNext") => one_sided(LinearTermination::ThroughNext),
        Some("ToFace") => one_sided(LinearTermination::ToFace {
            face: FaceSelection::Native(feature.properties.get("Face")?.clone()),
            offset: None,
        }),
        Some("ToVertex") => one_sided(LinearTermination::ToVertex {
            vertex: VertexSelection::native(feature.properties.get("Vertex")?.clone())
                .unwrap_or(VertexSelection::Unresolved),
        }),
        Some("OffsetFromFace") => match length("Depth")
            .or_else(sole_length)
            .and_then(|offset| cadmpeg_ir::scalar::PositiveLength::new(offset.get()))
        {
            Some(offset) => one_sided(LinearTermination::OffsetFromFace {
                face: FaceSelection::Native(feature.properties.get("Face")?.clone()),
                offset,
            }),
            None => one_sided(LinearTermination::Unresolved),
        },
        Some(_) => one_sided(LinearTermination::Unresolved),
    };
    let direction = match feature.properties.get("Direction") {
        Some(value) => cadmpeg_ir::features::ExtrudeDirection::Explicit {
            vector: cadmpeg_ir::features::FeatureDirection3::new(parse_vector3(value)?)?,
            source: None,
        },
        None => cadmpeg_ir::features::ExtrudeDirection::ProfileNormal,
    };
    if matches!(direction, cadmpeg_ir::features::ExtrudeDirection::Explicit { vector, .. } if !valid_direction(vector.get()))
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
    } else if let Some(profile) = history_profile {
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
        solid: Some(!matches!(
            feature.input_class.as_deref().map(native_object_class),
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
    features_by_source: &HashMap<crate::records::FeatureSource, &Feature>,
    history_features: &[Feature],
) -> Option<FeatureDefinition> {
    let profile = hole_profile_construction(feature, features_by_source, history_features);
    let diameter = feature
        .parameters
        .get("Diameter")
        .and_then(|value| parse_positive_length_mm(value))
        .and_then(cadmpeg_ir::scalar::PositiveLength::new)
        .or_else(|| profile.as_ref().map(|profile| profile.diameter));
    let has_counterbore = feature.parameters.contains_key("CounterboreDiameter")
        || feature.parameters.contains_key("CounterboreDepth");
    let has_countersink = feature.parameters.contains_key("CountersinkDiameter")
        || feature.parameters.contains_key("CountersinkAngle");
    let counterbore_diameter = feature
        .parameters
        .get("CounterboreDiameter")
        .and_then(|value| parse_positive_length_mm(value))
        .and_then(cadmpeg_ir::scalar::PositiveLength::new);
    let counterbore_depth = feature
        .parameters
        .get("CounterboreDepth")
        .and_then(|value| parse_positive_length_mm(value))
        .and_then(cadmpeg_ir::scalar::PositiveLength::new);
    let countersink_diameter = feature
        .parameters
        .get("CountersinkDiameter")
        .and_then(|value| parse_positive_length_mm(value))
        .and_then(cadmpeg_ir::scalar::PositiveLength::new);
    let countersink_angle = feature
        .parameters
        .get("CountersinkAngle")
        .and_then(|value| parse_bounded_angle_rad(value))
        .and_then(cadmpeg_ir::scalar::InteriorAngle::new);
    let drill_point_angle = feature
        .parameters
        .get("DrillPointAngle")
        .and_then(|value| parse_bounded_angle_rad(value))
        .and_then(cadmpeg_ir::scalar::InteriorAngle::new);
    let thread = feature
        .parameters
        .get("ThreadMajorDiameter")
        .and_then(|value| parse_positive_length_mm(value))
        .and_then(cadmpeg_ir::scalar::PositiveLength::new)
        .zip(
            feature
                .parameters
                .get("ThreadDepth")
                .and_then(|value| parse_positive_length_mm(value))
                .and_then(cadmpeg_ir::scalar::PositiveLength::new),
        )
        .zip(drill_point_angle)
        .map(|((major_diameter, thread_depth), drill_point_angle)| {
            HoleConstruction::NativeThread {
                major_diameter,
                thread_depth,
                pitch: feature
                    .parameters
                    .get("ThreadPitch")
                    .and_then(|value| parse_positive_length_mm(value))
                    .and_then(cadmpeg_ir::scalar::PositiveLength::new),
                drill_point_angle,
            }
        });
    let construction = if has_counterbore && has_countersink {
        hole_form(HoleKind::Unresolved(None))
    } else if has_counterbore {
        match cadmpeg_ir::features::split(counterbore_diameter, counterbore_depth) {
            cadmpeg_ir::features::Split::Both(diameter, depth) => {
                hole_form(drill_point_angle.map_or(
                    HoleKind::Counterbore { diameter, depth },
                    |drill_point_angle| HoleKind::CounterboreDrilled {
                        diameter,
                        depth,
                        drill_point_angle,
                    },
                ))
            }
            cadmpeg_ir::features::Split::Partial(pair) => {
                hole_form(HoleKind::PartialCounterbore(pair))
            }
            cadmpeg_ir::features::Split::Neither => hole_form(HoleKind::Unresolved(Some(
                cadmpeg_ir::features::HoleForm::Counterbore,
            ))),
        }
    } else if has_countersink {
        match cadmpeg_ir::features::split(countersink_diameter, countersink_angle) {
            cadmpeg_ir::features::Split::Both(diameter, angle) => {
                hole_form(HoleKind::Countersink { diameter, angle })
            }
            cadmpeg_ir::features::Split::Partial(pair) => {
                hole_form(HoleKind::PartialCountersink(pair))
            }
            cadmpeg_ir::features::Split::Neither => hole_form(HoleKind::Unresolved(Some(
                cadmpeg_ir::features::HoleForm::Countersink,
            ))),
        }
    } else if let Some(thread) = thread {
        thread
    } else if let Some(drill_point_angle) = drill_point_angle {
        hole_form(HoleKind::SimpleDrilled { drill_point_angle })
    } else {
        profile.as_ref().map_or_else(
            || hole_form(HoleKind::Simple),
            |profile| profile.construction.clone(),
        )
    };
    let extent = match feature.properties.get("EndCondition").map(String::as_str) {
        None | Some("Blind")
            if profile
                .as_ref()
                .is_some_and(|profile| profile.exit_kind.is_some()) =>
        {
            Some(LinearTermination::ThroughAll)
        }
        None | Some("Blind") => feature
            .parameters
            .get("Depth")
            .and_then(|value| parse_positive_length_mm(value))
            .and_then(Length::new)
            .or_else(|| profile.as_ref().and_then(|profile| profile.depth))
            .and_then(|length| cadmpeg_ir::scalar::NonZeroLength::new(length.get()))
            .map(|length| LinearTermination::Blind { length }),
        Some("ThroughAll") => Some(LinearTermination::ThroughAll),
        Some(_) => None,
    };
    Some(FeatureDefinition::Hole {
        profile: None,
        profile_filter: None,
        face: feature
            .properties
            .get("Face")
            .cloned()
            .map(FaceSelection::Native),
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
            .and_then(|(position, direction)| {
                Some(vec![cadmpeg_ir::features::HolePlacement::Directed {
                    position: cadmpeg_ir::features::FinitePoint3::new(position)?,
                    direction: cadmpeg_ir::features::FeatureDirection3::new(direction)?,
                }])
            }),
        shape: cadmpeg_ir::features::HoleShape::new(
            construction,
            profile.as_ref().and_then(|profile| profile.exit_kind),
            diameter,
        )
        .ok()?,

        extent,
        bottom: profile.as_ref().and_then(|profile| profile.bottom),
        taper_angle: profile.as_ref().and_then(|profile| profile.taper_angle),
        allow_multi_profile_faces: None,
    })
}

pub(crate) fn threaded_hole_major_diameter(
    feature: &Feature,
    features_by_source: &HashMap<crate::records::FeatureSource, &Feature>,
    history_features: &[Feature],
) -> Option<f64> {
    if classify(feature) != Some(FeatureClass::Hole) {
        return None;
    }
    let FeatureDefinition::Hole { shape, .. } =
        project_hole(feature, features_by_source, history_features)?
    else {
        return None;
    };
    let HoleConstruction::NativeThread { major_diameter, .. } = shape.construction() else {
        return None;
    };
    Some(major_diameter.get())
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct HoleProfileConstruction {
    pub(crate) diameter: cadmpeg_ir::scalar::PositiveLength,
    pub(crate) depth: Option<Length>,
    pub(crate) construction: HoleConstruction,
    pub(crate) exit_kind: Option<HoleKind>,
    pub(crate) bottom: Option<HoleBottom>,
    pub(crate) taper_angle: Option<cadmpeg_ir::scalar::InteriorAngle>,
}

fn hole_form(kind: HoleKind) -> HoleConstruction {
    HoleConstruction::Form {
        kind,
        specification: None,
    }
}

pub(crate) fn hole_profile_construction(
    feature: &Feature,
    features_by_source: &HashMap<crate::records::FeatureSource, &Feature>,
    history_features: &[Feature],
) -> Option<HoleProfileConstruction> {
    let children = feature.properties.get("DissectableChildren")?;
    let constructions = children
        .split(',')
        .map(str::trim)
        .filter(|source| !source.is_empty())
        .filter_map(|source| {
            crate::records::FeatureSource::try_from(source)
                .ok()
                .and_then(|source| features_by_source.get(&source).copied())
                .or_else(|| {
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
    enum ParsedDimension {
        Diameter(Length),
        Length(Length),
        Angle(cadmpeg_ir::scalar::InteriorAngle),
    }

    let mut dimensions = Vec::new();
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
                .and_then(Length::new)
            {
                dimensions.push(ParsedDimension::Diameter(value));
            }
        } else if let Some(value) =
            parse_bounded_angle_rad(expression).and_then(cadmpeg_ir::scalar::InteriorAngle::new)
        {
            dimensions.push(ParsedDimension::Angle(value));
        } else if let Some(value) =
            parse_positive_dimension_length_mm(expression).and_then(Length::new)
        {
            dimensions.push(ParsedDimension::Length(value));
        }
    }
    let mut diameters = Vec::new();
    let mut lengths = Vec::new();
    let mut angles = Vec::new();
    for dimension in &dimensions {
        match dimension {
            ParsedDimension::Diameter(value) => diameters.push(*value),
            ParsedDimension::Length(value) => lengths.push(*value),
            ParsedDimension::Angle(value) => angles.push(*value),
        }
    }
    diameters.sort_by(|left, right| left.get().total_cmp(&right.get()));
    lengths.sort_by(|left, right| left.get().total_cmp(&right.get()));
    angles.sort_by(|left, right| left.get().total_cmp(&right.get()));
    match (diameters.as_slice(), lengths.as_slice(), angles.as_slice()) {
        ([diameter], [depth], []) => Some(HoleProfileConstruction {
            diameter: cadmpeg_ir::scalar::PositiveLength::new(diameter.get())?,
            depth: Some(*depth),
            construction: hole_form(HoleKind::Simple),
            exit_kind: None,
            bottom: Some(HoleBottom::Flat),
            taper_angle: None,
        }),
        ([diameter], [depth], [drill_point_angle]) => Some(HoleProfileConstruction {
            diameter: cadmpeg_ir::scalar::PositiveLength::new(diameter.get())?,
            depth: Some(*depth),
            construction: hole_form(HoleKind::SimpleDrilled {
                drill_point_angle: *drill_point_angle,
            }),
            exit_kind: None,
            bottom: Some(HoleBottom::Angled {
                included_angle: *drill_point_angle,
                depth_to_tip: false,
            }),
            taper_angle: None,
        }),
        ([diameter, major_diameter], [thread_depth, drill_depth], [drill_point_angle])
            if matches!(
                dimensions.as_slice(),
                [
                    ParsedDimension::Diameter(_),
                    ParsedDimension::Length(_),
                    ParsedDimension::Diameter(_),
                    ParsedDimension::Length(_),
                    ParsedDimension::Angle(_),
                ]
            ) && diameter.get() < major_diameter.get()
                && thread_depth.get() < drill_depth.get() =>
        {
            Some(HoleProfileConstruction {
                diameter: cadmpeg_ir::scalar::PositiveLength::new(diameter.get())?,
                depth: Some(*drill_depth),
                construction: HoleConstruction::NativeThread {
                    major_diameter: cadmpeg_ir::scalar::PositiveLength::new(major_diameter.get())?,
                    thread_depth: cadmpeg_ir::scalar::PositiveLength::new(thread_depth.get())?,
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
        ) if diameter.get() < major_diameter.get()
            && thread_depth.get() < drill_depth.get()
            && taper_angle.get() < drill_point_angle.get() =>
        {
            Some(HoleProfileConstruction {
                diameter: cadmpeg_ir::scalar::PositiveLength::new(diameter.get())?,
                depth: Some(*drill_depth),
                construction: HoleConstruction::NativeThread {
                    major_diameter: cadmpeg_ir::scalar::PositiveLength::new(major_diameter.get())?,
                    thread_depth: cadmpeg_ir::scalar::PositiveLength::new(thread_depth.get())?,
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
            if matches!(dimensions.last(), Some(ParsedDimension::Diameter(_)))
                && diameter.get() < entry_diameter.get()
                && entry_depth.get() < depth.get() =>
        {
            Some(HoleProfileConstruction {
                diameter: cadmpeg_ir::scalar::PositiveLength::new(diameter.get())?,
                depth: Some(*depth),
                construction: hole_form(HoleKind::CounterboreDrilled {
                    diameter: cadmpeg_ir::scalar::PositiveLength::new(entry_diameter.get())?,
                    depth: cadmpeg_ir::scalar::PositiveLength::new(entry_depth.get())?,
                    drill_point_angle: *drill_point_angle,
                }),
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
        ) if matches!(
            dimensions.as_slice(),
            [
                ParsedDimension::Length(_),
                ParsedDimension::Diameter(_),
                ParsedDimension::Angle(_),
                ParsedDimension::Length(_),
                ParsedDimension::Diameter(_),
                ParsedDimension::Diameter(_),
            ]
        ) && diameter.get() < exit_diameter.get()
            && exit_diameter.get() < counterbore_diameter.get()
            && counterbore_depth.get() < through_depth.get() =>
        {
            Some(HoleProfileConstruction {
                diameter: cadmpeg_ir::scalar::PositiveLength::new(diameter.get())?,
                depth: Some(*through_depth),
                construction: hole_form(HoleKind::Counterbore {
                    diameter: cadmpeg_ir::scalar::PositiveLength::new(counterbore_diameter.get())?,
                    depth: cadmpeg_ir::scalar::PositiveLength::new(counterbore_depth.get())?,
                }),
                exit_kind: Some(HoleKind::Countersink {
                    diameter: cadmpeg_ir::scalar::PositiveLength::new(exit_diameter.get())?,
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
        ) if diameter.get() < recess_diameter.get()
            && recess_diameter.get() < entry_diameter.get()
            && recess_depth.get() < drill_depth.get()
            && entry_angle.get() < drill_point_angle.get() =>
        {
            Some(HoleProfileConstruction {
                diameter: cadmpeg_ir::scalar::PositiveLength::new(diameter.get())?,
                depth: Some(*drill_depth),
                construction: hole_form(HoleKind::Counterdrill {
                    diameters: cadmpeg_ir::features::CounterdrillDiameters::new(
                        cadmpeg_ir::scalar::PositiveLength::new(recess_diameter.get())?,
                        Some(cadmpeg_ir::scalar::PositiveLength::new(
                            entry_diameter.get(),
                        )?),
                    )
                    .ok()?,

                    depth: cadmpeg_ir::scalar::PositiveLength::new(recess_depth.get())?,
                    angle: *entry_angle,
                }),
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
