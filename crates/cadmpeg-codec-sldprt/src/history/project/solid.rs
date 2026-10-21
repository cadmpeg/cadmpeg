// SPDX-License-Identifier: Apache-2.0
//! Extrude and hole projection.

use crate::classification::{classify, native_object_class, FeatureClass, NativeClassKind};
use crate::records::{Feature, FeatureContent};
use cadmpeg_core::CodecError;
use cadmpeg_ir::{
    features::{
        holes::{HoleBottom, HoleConstruction, HoleKind},
        BooleanOp, Dimension, ExtrudeExtent, ExtrudeSide, FaceSelection, FeatureDefinition,
        FeatureOperation, LinearTermination, PlanarProfileRef, ProfileRef, VertexSelection,
    },
    scalar::{NonZeroLength, PositiveLength},
};
use std::collections::HashMap;

use crate::history::classify::extrude_feature_op;
use crate::history::literals::{
    parse_boolean_op, parse_bounded_angle_rad, parse_dimension_display_length, parse_point3_mm,
    parse_positive_dimension_length_mm, parse_vector3, strip_diameter_modifier, valid_direction,
};

pub(super) fn project_extrude(
    feature: &Feature,
    native_by_source: &HashMap<String, &str>,
    features_by_source: &HashMap<crate::records::FeatureSource, &Feature>,
) -> Option<FeatureDefinition> {
    let mut source_dimensions = feature.content.iter().filter_map(|content| match content {
        FeatureContent::Dimension(name) => Some(name.as_str()),
        FeatureContent::Feature(_) | FeatureContent::Text(_) => None,
    });
    let source_depth = source_dimensions
        .next()
        .filter(|first| source_dimensions.all(|name| name == *first));
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
        feature.input_class.as_deref() == Some("moExtrusion_c") && source_depth.is_some();
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
                .map(|(_, profile)| profile)
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
        let mut names = feature.parameters.keys();
        let name = names.next().filter(|_| names.next().is_none())?;
        super::parameter_dimension::<cadmpeg_ir::scalar::PositiveLength>(feature, &[name.as_str()])
    };
    let legacy_length = || {
        super::parameter_dimension::<cadmpeg_ir::scalar::PositiveLength>(feature, &[source_depth?])
    };
    let length = |name| {
        if name == "Depth" {
            super::parameter_dimension::<cadmpeg_ir::scalar::PositiveLength>(
                feature,
                &["Depth", "D1"],
            )
        } else {
            super::parameter_dimension(feature, &[name])
        }
    };
    let draft = super::parameter_dimension::<cadmpeg_ir::scalar::SlopeAngle>(feature, &["Draft"]);
    let one_sided = |termination| ExtrudeExtent::OneSided {
        side: ExtrudeSide {
            termination,
            draft: draft.clone(),
        },
    };
    let extent = match feature.properties.get("EndCondition").map(String::as_str) {
        None if !feature.parameters.contains_key("Depth")
            && !feature.parameters.contains_key("D1")
            && !legacy_history_extrusion
            && !implicit_modern_blind =>
        {
            one_sided(LinearTermination::Unresolved {})
        }
        None | Some("Blind") => match length("Depth")
            .or_else(|| legacy_history_extrusion.then(legacy_length).flatten())
            .or_else(sole_length)
        {
            Some(length) => one_sided(LinearTermination::Blind {
                length: length.map_literal(NonZeroLength::from),
            }),
            None => one_sided(LinearTermination::Unresolved {}),
        },
        Some("Symmetric") => match length("Depth").or_else(sole_length) {
            Some(length) => ExtrudeExtent::Symmetric {
                side: ExtrudeSide {
                    termination: LinearTermination::Blind {
                        length: length.map_literal(NonZeroLength::from),
                    },
                    draft: draft.clone(),
                },
            },
            None => one_sided(LinearTermination::Unresolved {}),
        },
        Some("TwoSided") => {
            let (Some(first), Some(second)) = (length("Depth"), length("Depth2")) else {
                return None;
            };
            ExtrudeExtent::TwoSided {
                first: ExtrudeSide {
                    termination: LinearTermination::Blind {
                        length: first.map_literal(NonZeroLength::from),
                    },
                    draft: draft.clone(),
                },
                second: ExtrudeSide {
                    termination: LinearTermination::Blind {
                        length: second.map_literal(NonZeroLength::from),
                    },
                    draft: None,
                },
            }
        }
        Some("ThroughAll") => one_sided(LinearTermination::ThroughAll {}),
        Some("ThroughAllBoth") => ExtrudeExtent::TwoSided {
            first: ExtrudeSide {
                termination: LinearTermination::ThroughAll {},
                draft: draft.clone(),
            },
            second: ExtrudeSide {
                termination: LinearTermination::ThroughAll {},
                draft: None,
            },
        },
        Some("ThroughNext") => one_sided(LinearTermination::ThroughNext {}),
        Some("ToFace") => {
            let face = feature.properties.get("Face")?;
            one_sided(LinearTermination::ToFace {
                face: FaceSelection::Native(face.to_owned()),
                offset: None,
            })
        }
        Some("ToVertex") => {
            let vertex = feature.properties.get("Vertex")?;
            one_sided(LinearTermination::ToVertex {
                vertex: VertexSelection::native(vertex.to_owned())
                    .unwrap_or(VertexSelection::Unresolved),
            })
        }
        Some("OffsetFromFace") => match length("Depth").or_else(sole_length) {
            Some(offset) => {
                let face = feature.properties.get("Face")?;
                one_sided(LinearTermination::OffsetFromFace {
                    face: FaceSelection::Native(face.to_owned()),
                    offset,
                })
            }
            None => one_sided(LinearTermination::Unresolved {}),
        },
        Some(_) => one_sided(LinearTermination::Unresolved {}),
    };
    let direction = match feature.properties.get("Direction") {
        Some(value) => {
            let vector =
                parse_vector3(value).and_then(cadmpeg_ir::features::FeatureDirection3::new)?;
            cadmpeg_ir::features::ExtrudeDirection::Explicit {
                vector,
                source: None,
            }
        }
        None => cadmpeg_ir::features::ExtrudeDirection::ProfileNormal {},
    };
    if matches!(direction, cadmpeg_ir::features::ExtrudeDirection::Explicit { vector, .. } if !valid_direction(vector.get()))
    {
        return None;
    }
    let profile = if let Some(source) = feature.properties.get("Profile") {
        ProfileRef::Planar(PlanarProfileRef::Native(
            (native_by_source
                .get(source.as_str())
                .copied()
                .unwrap_or(source.as_str()))
            .to_owned(),
        ))
    } else if let Some(children) = feature.properties.get("DissectableChildren") {
        let mut profiles = children
            .split(',')
            .map(str::trim)
            .filter(|source| !source.is_empty());
        let sole = profiles.next().filter(|_| profiles.next().is_none());
        match sole {
            Some(profile) => ProfileRef::Planar(PlanarProfileRef::Native(
                (native_by_source.get(profile).copied().unwrap_or(profile)).to_owned(),
            )),
            None => ProfileRef::Planar(PlanarProfileRef::Unresolved(feature.id.clone())),
        }
    } else if let Some(profile) = history_profile {
        ProfileRef::Planar(PlanarProfileRef::Native(profile.to_owned()))
    } else {
        ProfileRef::Planar(PlanarProfileRef::Unresolved(feature.id.clone()))
    };
    Some(FeatureDefinition::Operation(FeatureOperation::Extrude {
        profile,
        direction,
        start: cadmpeg_ir::features::ExtrudeStart::ProfilePlane {},
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
    }))
}

pub(super) fn project_hole(
    feature: &Feature,
    features_by_source: &HashMap<crate::records::FeatureSource, &Feature>,
    history_features: &[Feature],
) -> Result<Option<FeatureDefinition>, CodecError> {
    let Some((shape, profile)) =
        hole_shape_and_profile(feature, features_by_source, history_features)?
    else {
        return Ok(None);
    };
    let extent = match feature.properties.get("EndCondition").map(String::as_str) {
        None | Some("Blind")
            if profile
                .as_ref()
                .is_some_and(|profile| profile.exit_kind.is_some()) =>
        {
            Some(LinearTermination::ThroughAll {})
        }
        None | Some("Blind") => super::parameter_dimension(feature, &["Depth"])
            .or_else(|| profile.as_ref().and_then(|profile| profile.depth.clone()))
            .map(|length| LinearTermination::Blind {
                length: length.map_literal(NonZeroLength::from),
            }),
        Some("ThroughAll") => Some(LinearTermination::ThroughAll {}),
        Some(_) => None,
    };
    Ok(Some(FeatureDefinition::Operation(FeatureOperation::Hole {
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
                Some(vec![cadmpeg_ir::features::holes::HolePlacement::Directed {
                    position,
                    direction: cadmpeg_ir::features::FeatureDirection3::new(direction)?,
                }])
            }),
        shape,
        extent,
        bottom: profile.as_ref().and_then(|profile| profile.bottom.clone()),
        taper_angle: profile
            .as_ref()
            .and_then(|profile| profile.taper_angle.clone()),
        allow_multi_profile_faces: None,
    })))
}
fn hole_shape_and_profile(
    feature: &Feature,
    features_by_source: &HashMap<crate::records::FeatureSource, &Feature>,
    history_features: &[Feature],
) -> Result<
    Option<(
        cadmpeg_ir::features::holes::HoleShape,
        Option<HoleProfileConstruction>,
    )>,
    CodecError,
> {
    let profile = hole_profile_construction(feature, features_by_source, history_features)?;
    let diameter = super::parameter_dimension(feature, &["Diameter"])
        .or_else(|| profile.as_ref().map(|profile| profile.diameter.clone()));
    let has_counterbore = feature.parameters.contains_key("CounterboreDiameter")
        || feature.parameters.contains_key("CounterboreDepth");
    let has_countersink = feature.parameters.contains_key("CountersinkDiameter")
        || feature.parameters.contains_key("CountersinkAngle");
    let counterbore_diameter = super::parameter_dimension(feature, &["CounterboreDiameter"]);
    let counterbore_depth = super::parameter_dimension(feature, &["CounterboreDepth"]);
    let countersink_diameter = super::parameter_dimension(feature, &["CountersinkDiameter"]);
    let countersink_angle = super::parameter_dimension(feature, &["CountersinkAngle"]);
    let drill_point_angle = super::parameter_dimension(feature, &["DrillPointAngle"]);
    let thread = super::parameter_dimension(feature, &["ThreadMajorDiameter"])
        .zip(super::parameter_dimension(feature, &["ThreadDepth"]))
        .zip(drill_point_angle.clone())
        .map(|((major_diameter, thread_depth), drill_point_angle)| {
            HoleConstruction::NativeThread {
                major_diameter,
                thread_depth,
                pitch: super::parameter_dimension(feature, &["ThreadPitch"]),
                drill_point_angle,
            }
        });
    let construction = if has_counterbore && has_countersink {
        hole_form(HoleKind::Unresolved(None))
    } else if has_counterbore {
        match (counterbore_diameter, counterbore_depth) {
            (Some(diameter), Some(depth)) => hole_form(match drill_point_angle {
                None => HoleKind::Counterbore { diameter, depth },
                Some(drill_point_angle) => HoleKind::CounterboreDrilled {
                    diameter,
                    depth,
                    drill_point_angle,
                },
            }),
            (Some(diameter), None) => hole_form(HoleKind::PartialCounterbore(
                cadmpeg_ir::features::holes::PartialPair::First(diameter),
            )),
            (None, Some(depth)) => hole_form(HoleKind::PartialCounterbore(
                cadmpeg_ir::features::holes::PartialPair::Second(depth),
            )),
            (None, None) => hole_form(HoleKind::Unresolved(Some(
                cadmpeg_ir::features::holes::HoleForm::Counterbore,
            ))),
        }
    } else if has_countersink {
        match (countersink_diameter, countersink_angle) {
            (Some(diameter), Some(angle)) => hole_form(HoleKind::Countersink { diameter, angle }),
            (Some(diameter), None) => hole_form(HoleKind::PartialCountersink(
                cadmpeg_ir::features::holes::PartialPair::First(diameter),
            )),
            (None, Some(angle)) => hole_form(HoleKind::PartialCountersink(
                cadmpeg_ir::features::holes::PartialPair::Second(angle),
            )),
            (None, None) => hole_form(HoleKind::Unresolved(Some(
                cadmpeg_ir::features::holes::HoleForm::Countersink,
            ))),
        }
    } else if let Some(thread) = thread {
        thread
    } else if let Some(drill_point_angle) = drill_point_angle {
        hole_form(HoleKind::SimpleDrilled { drill_point_angle })
    } else {
        match &profile {
            Some(profile) => profile.construction.clone(),
            None => hole_form(HoleKind::Simple),
        }
    };
    let Ok(shape) = cadmpeg_ir::features::holes::HoleShape::new(
        construction,
        profile
            .as_ref()
            .and_then(|profile| profile.exit_kind.clone()),
        diameter,
    ) else {
        return Ok(None);
    };
    Ok(Some((shape, profile)))
}

pub(crate) fn threaded_hole_major_diameter(
    feature: &Feature,
    features_by_source: &HashMap<crate::records::FeatureSource, &Feature>,
    history_features: &[Feature],
) -> Result<Option<f64>, CodecError> {
    if classify(feature) != Some(FeatureClass::Hole) {
        return Ok(None);
    }
    let Some((shape, _)) = hole_shape_and_profile(feature, features_by_source, history_features)?
    else {
        return Ok(None);
    };
    let HoleConstruction::NativeThread { major_diameter, .. } = shape.construction() else {
        return Ok(None);
    };
    let value = major_diameter.literal().copied().or_else(|| {
        let id = major_diameter.parameter()?;
        history_features
            .iter()
            .chain(std::iter::once(feature))
            .find_map(|source| {
                source.parameters.iter().find_map(|(name, value)| {
                    (super::parameter_dimension::<PositiveLength>(source, &[name.as_str()])
                        .as_ref()
                        .and_then(Dimension::parameter)
                        == Some(id))
                    .then(|| parse_positive_dimension_length_mm(value))
                    .flatten()
                })
            })
    });
    Ok(value.map(PositiveLength::get))
}

#[derive(Debug, Clone, PartialEq)]
pub(super) struct HoleProfileConstruction {
    pub(in crate::history) diameter: Dimension<PositiveLength>,
    pub(in crate::history) depth: Option<Dimension<PositiveLength>>,
    pub(in crate::history) construction: HoleConstruction,
    pub(in crate::history) exit_kind: Option<HoleKind>,
    pub(in crate::history) bottom: Option<HoleBottom>,
    pub(in crate::history) taper_angle: Option<Dimension<cadmpeg_ir::scalar::InteriorAngle>>,
}

fn hole_form(kind: HoleKind) -> HoleConstruction {
    HoleConstruction::Form {
        kind,
        specification: None,
    }
}

fn hole_profile_construction(
    feature: &Feature,
    features_by_source: &HashMap<crate::records::FeatureSource, &Feature>,
    history_features: &[Feature],
) -> Result<Option<HoleProfileConstruction>, CodecError> {
    let Some(children) = feature.properties.get("DissectableChildren") else {
        return Ok(None);
    };
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
        .filter(|profile| classify(profile) == Some(FeatureClass::Sketch));
    let mut sole = None;
    let mut multiple = false;
    let mut complete = None;
    for profile in constructions {
        let Some(construction) = hole_sketch_construction(profile)? else {
            continue;
        };
        if construction.depth.is_some() {
            if complete.is_some() {
                return Ok(None);
            }
            complete = Some(construction);
            continue;
        }
        if sole.is_some() {
            multiple = true;
        } else {
            sole = Some(construction);
        }
    }
    Ok(complete.or(if multiple { None } else { sole }))
}

pub(super) fn hole_sketch_construction(
    profile: &Feature,
) -> Result<Option<HoleProfileConstruction>, CodecError> {
    Ok(hole_sketch_construction_details(profile)?.map(|(construction, _)| construction))
}

/// Source quantity roles proven by a supported ordered hole-profile construction.
pub(in crate::history) fn hole_profile_parameter_values(
    profile: &Feature,
) -> Result<
    Option<std::collections::BTreeMap<String, cadmpeg_ir::features::ParameterValue>>,
    CodecError,
> {
    Ok(hole_sketch_construction_details(profile)?.map(|(_, controls)| controls))
}

fn hole_profile_dimension<Q>(
    profile: &Feature,
    name: &str,
    parse: impl Fn(&str) -> Option<Q>,
) -> Option<Dimension<Q>> {
    if let Some(cache) = profile
        .dimension_properties
        .get(name)
        .and_then(|properties| properties.get("Value"))
    {
        parse(cache)?;
    }
    let ordinal = super::projected_parameter_names(profile)
        .iter()
        .position(|candidate| candidate == name)?;
    super::neutral_parameter_id(profile, ordinal)
        .ok()
        .map(Dimension::Parameter)
}

fn hole_sketch_construction_details(
    profile: &Feature,
) -> Result<
    Option<(
        HoleProfileConstruction,
        std::collections::BTreeMap<String, cadmpeg_ir::features::ParameterValue>,
    )>,
    CodecError,
> {
    // Numeric values below classify the source's ordered construction. The
    // resulting design stores only parameter bindings, never these parse values.
    #[derive(Clone)]
    struct Measure<Q> {
        value: Q,
        binding: Dimension<Q>,
    }
    #[derive(Clone)]
    enum ParsedDimension {
        Diameter(Measure<PositiveLength>),
        Length(Measure<PositiveLength>),
        Angle(Measure<cadmpeg_ir::scalar::InteriorAngle>),
    }

    const MAX_DIMENSIONS: usize = 7;
    const MAX_DIAMETERS: usize = 3;
    const MAX_LENGTHS: usize = 2;
    const MAX_ANGLES: usize = 2;
    let has_source_dimensions = profile
        .content
        .iter()
        .any(|content| matches!(content, FeatureContent::Dimension(_)));
    let expressions = profile
        .parameters
        .iter()
        .filter(|_| !has_source_dimensions)
        .map(|(name, value)| (name.as_str(), value))
        .chain(profile.content.iter().filter_map(|content| {
            match content {
                FeatureContent::Dimension(name) => profile
                    .parameters
                    .get(name.as_str())
                    .map(|value| (name.as_str(), value)),
                FeatureContent::Feature(_) | FeatureContent::Text(_) => None,
            }
        }));
    let mut dimensions = Vec::new();
    let mut controls = std::collections::BTreeMap::new();
    for (name, expression) in expressions {
        let dimension = if strip_diameter_modifier(expression).is_some() {
            parse_dimension_display_length(expression)
                .and_then(|value| PositiveLength::try_from(value).ok())
                .zip(hole_profile_dimension(profile, name, |text| {
                    parse_dimension_display_length(text)
                        .and_then(|value| PositiveLength::try_from(value).ok())
                }))
                .map(|(value, binding)| ParsedDimension::Diameter(Measure { value, binding }))
        } else if let Some(value) = parse_bounded_angle_rad(expression) {
            hole_profile_dimension(profile, name, parse_bounded_angle_rad)
                .map(|binding| ParsedDimension::Angle(Measure { value, binding }))
        } else {
            parse_positive_dimension_length_mm(expression)
                .zip(hole_profile_dimension(
                    profile,
                    name,
                    parse_positive_dimension_length_mm,
                ))
                .map(|(value, binding)| ParsedDimension::Length(Measure { value, binding }))
        };
        if let Some(dimension) = dimension {
            let value = match &dimension {
                ParsedDimension::Diameter(measure) | ParsedDimension::Length(measure) => {
                    cadmpeg_ir::features::ParameterValue::Length(cadmpeg_ir::scalar::Length::from(
                        measure.value,
                    ))
                }
                ParsedDimension::Angle(measure) => cadmpeg_ir::features::ParameterValue::Angle(
                    cadmpeg_ir::scalar::Angle::from(measure.value),
                ),
            };
            controls.insert(name.to_owned(), value);
            if dimensions.len() == MAX_DIMENSIONS {
                return Ok(None);
            }
            dimensions.push(dimension);
        }
    }
    let mut diameters = Vec::new();
    let mut lengths = Vec::new();
    let mut angles = Vec::new();
    for dimension in &dimensions {
        match dimension {
            ParsedDimension::Diameter(value) => diameters.push(value.clone()),
            ParsedDimension::Length(value) => lengths.push(value.clone()),
            ParsedDimension::Angle(value) => angles.push(value.clone()),
        }
    }
    if diameters.len() > MAX_DIAMETERS || lengths.len() > MAX_LENGTHS || angles.len() > MAX_ANGLES {
        return Ok(None);
    }
    diameters.sort_unstable_by(|left, right| left.value.get().total_cmp(&right.value.get()));
    lengths.sort_unstable_by(|left, right| left.value.get().total_cmp(&right.value.get()));
    angles.sort_unstable_by(|left, right| left.value.get().total_cmp(&right.value.get()));
    let construction = match (&*diameters, &*lengths, &*angles) {
        ([diameter], [depth], []) => Some(HoleProfileConstruction {
            diameter: diameter.binding.clone(),
            depth: Some(depth.binding.clone()),
            construction: hole_form(HoleKind::Simple),
            exit_kind: None,
            bottom: Some(HoleBottom::Flat),
            taper_angle: None,
        }),
        ([diameter], [depth], [drill_point_angle]) => Some(HoleProfileConstruction {
            diameter: diameter.binding.clone(),
            depth: Some(depth.binding.clone()),
            construction: hole_form(HoleKind::SimpleDrilled {
                drill_point_angle: drill_point_angle.binding.clone(),
            }),
            exit_kind: None,
            bottom: Some(HoleBottom::Angled {
                included_angle: drill_point_angle.binding.clone(),
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
            ) && diameter.value.get() < major_diameter.value.get()
                && thread_depth.value.get() < drill_depth.value.get() =>
        {
            Some(HoleProfileConstruction {
                diameter: diameter.binding.clone(),
                depth: Some(drill_depth.binding.clone()),
                construction: HoleConstruction::NativeThread {
                    major_diameter: major_diameter.binding.clone(),
                    thread_depth: thread_depth.binding.clone(),
                    pitch: None,
                    drill_point_angle: drill_point_angle.binding.clone(),
                },
                exit_kind: None,
                bottom: Some(HoleBottom::Angled {
                    included_angle: drill_point_angle.binding.clone(),
                    depth_to_tip: false,
                }),
                taper_angle: None,
            })
        }
        (
            [diameter, major_diameter],
            [thread_depth, drill_depth],
            [taper_angle, drill_point_angle],
        ) if diameter.value.get() < major_diameter.value.get()
            && thread_depth.value.get() < drill_depth.value.get()
            && taper_angle.value.get() < drill_point_angle.value.get() =>
        {
            Some(HoleProfileConstruction {
                diameter: diameter.binding.clone(),
                depth: Some(drill_depth.binding.clone()),
                construction: HoleConstruction::NativeThread {
                    major_diameter: major_diameter.binding.clone(),
                    thread_depth: thread_depth.binding.clone(),
                    pitch: None,
                    drill_point_angle: drill_point_angle.binding.clone(),
                },
                exit_kind: None,
                bottom: Some(HoleBottom::Angled {
                    included_angle: drill_point_angle.binding.clone(),
                    depth_to_tip: false,
                }),
                taper_angle: Some(taper_angle.binding.clone()),
            })
        }
        ([diameter, entry_diameter], [entry_depth, depth], [drill_point_angle])
            if matches!(dimensions.last(), Some(ParsedDimension::Diameter(_)))
                && diameter.value.get() < entry_diameter.value.get()
                && entry_depth.value.get() < depth.value.get() =>
        {
            Some(HoleProfileConstruction {
                diameter: diameter.binding.clone(),
                depth: Some(depth.binding.clone()),
                construction: hole_form(HoleKind::CounterboreDrilled {
                    diameter: entry_diameter.binding.clone(),
                    depth: entry_depth.binding.clone(),
                    drill_point_angle: drill_point_angle.binding.clone(),
                }),
                exit_kind: None,
                bottom: Some(HoleBottom::Angled {
                    included_angle: drill_point_angle.binding.clone(),
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
        ) && diameter.value.get() < exit_diameter.value.get()
            && exit_diameter.value.get() < counterbore_diameter.value.get()
            && counterbore_depth.value.get() < through_depth.value.get() =>
        {
            Some(HoleProfileConstruction {
                diameter: diameter.binding.clone(),
                depth: Some(through_depth.binding.clone()),
                construction: hole_form(HoleKind::Counterbore {
                    diameter: counterbore_diameter.binding.clone(),
                    depth: counterbore_depth.binding.clone(),
                }),
                exit_kind: Some(HoleKind::Countersink {
                    diameter: exit_diameter.binding.clone(),
                    angle: exit_angle.binding.clone(),
                }),
                bottom: None,
                taper_angle: None,
            })
        }
        (
            [diameter, recess_diameter, entry_diameter],
            [recess_depth, drill_depth],
            [entry_angle, drill_point_angle],
        ) if diameter.value.get() < recess_diameter.value.get()
            && recess_diameter.value.get() < entry_diameter.value.get()
            && recess_depth.value.get() < drill_depth.value.get()
            && entry_angle.value.get() < drill_point_angle.value.get() =>
        {
            let Ok(diameters) = cadmpeg_ir::features::holes::CounterdrillDiameters::new(
                recess_diameter.binding.clone(),
                Some(entry_diameter.binding.clone()),
            ) else {
                return Ok(None);
            };
            Some(HoleProfileConstruction {
                diameter: diameter.binding.clone(),
                depth: Some(drill_depth.binding.clone()),
                construction: hole_form(HoleKind::Counterdrill {
                    diameters,
                    depth: recess_depth.binding.clone(),
                    angle: entry_angle.binding.clone(),
                }),
                exit_kind: None,
                bottom: Some(HoleBottom::Angled {
                    included_angle: drill_point_angle.binding.clone(),
                    depth_to_tip: false,
                }),
                taper_angle: None,
            })
        }
        _ => None,
    };
    Ok(construction.map(|construction| (construction, controls)))
}

pub(crate) fn is_hole_profile_construction(feature: &Feature) -> Result<bool, CodecError> {
    Ok(hole_sketch_construction(feature)?.is_some())
}
