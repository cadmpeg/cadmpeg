// SPDX-License-Identifier: Apache-2.0
//! Extrude and hole projection.

use crate::classification::{classify, native_object_class, FeatureClass, NativeClassKind};
use crate::records::{Feature, FeatureContent};
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::{
    features::{
        holes::{HoleBottom, HoleConstruction, HoleKind},
        BooleanOp, ExtrudeExtent, ExtrudeSide, FaceSelection, FeatureDefinition, FeatureOperation,
        LinearTermination, PlanarProfileRef, ProfileRef, VertexSelection,
    },
    scalar::{NonZeroLength, PositiveLength},
};
use std::collections::HashMap;

use super::copy_projected_feature_text;
use crate::history::classify::extrude_feature_op;
use crate::history::literals::{
    parse_angle_rad, parse_boolean_op, parse_bounded_angle_rad, parse_dimension_display_length,
    parse_point3_mm, parse_positive_dimension_length_mm, parse_positive_length_mm, parse_vector3,
    strip_diameter_modifier, valid_direction,
};

pub(super) fn project_extrude(
    ctx: &DecodeContext<'_>,
    feature: &Feature,
    native_by_source: &HashMap<String, &str>,
    features_by_source: &HashMap<crate::records::FeatureSource, &Feature>,
) -> Result<Option<FeatureDefinition>, CodecError> {
    ctx.charge_work(
        feature.content.len() as u64,
        "scan SLDPRT extrusion dimensions",
    )?;
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
    if history_profile_extrusion {
        ctx.charge_work(
            features_by_source.len() as u64,
            "scan SLDPRT extrusion source profiles",
        )?;
    }
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
        let mut values = feature.parameters.values();
        let sole = values.next().filter(|_| values.next().is_none())?;
        parse_positive_length_mm(sole).or_else(|| parse_positive_dimension_length_mm(sole))
    };
    let legacy_length = || {
        source_depth
            .and_then(|name| feature.parameters.get(name))
            .and_then(|value| {
                parse_positive_length_mm(value)
                    .or_else(|| parse_positive_dimension_length_mm(value))
            })
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
    };
    let draft = match feature.parameters.get("Draft") {
        Some(value) => {
            let Some(angle) = parse_angle_rad(value)
                .and_then(|angle| cadmpeg_ir::scalar::SlopeAngle::try_from(angle).ok())
            else {
                return Ok(None);
            };
            Some(angle)
        }
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
            one_sided(LinearTermination::Unresolved {})
        }
        None | Some("Blind") => match length("Depth")
            .or_else(|| legacy_history_extrusion.then(legacy_length).flatten())
            .or_else(sole_length)
        {
            Some(length) => one_sided(LinearTermination::Blind {
                length: NonZeroLength::from(length),
            }),
            None => one_sided(LinearTermination::Unresolved {}),
        },
        Some("Symmetric") => match length("Depth").or_else(sole_length) {
            Some(length) => ExtrudeExtent::Symmetric {
                side: ExtrudeSide {
                    termination: LinearTermination::Blind {
                        length: NonZeroLength::from(length),
                    },
                    draft,
                },
            },
            None => one_sided(LinearTermination::Unresolved {}),
        },
        Some("TwoSided") => {
            let (Some(first), Some(second)) = (length("Depth"), length("Depth2")) else {
                return Ok(None);
            };
            ExtrudeExtent::TwoSided {
                first: ExtrudeSide {
                    termination: LinearTermination::Blind {
                        length: NonZeroLength::from(first),
                    },
                    draft,
                },
                second: ExtrudeSide {
                    termination: LinearTermination::Blind {
                        length: NonZeroLength::from(second),
                    },
                    draft: None,
                },
            }
        }
        Some("ThroughAll") => one_sided(LinearTermination::ThroughAll {}),
        Some("ThroughAllBoth") => ExtrudeExtent::TwoSided {
            first: ExtrudeSide {
                termination: LinearTermination::ThroughAll {},
                draft,
            },
            second: ExtrudeSide {
                termination: LinearTermination::ThroughAll {},
                draft: None,
            },
        },
        Some("ThroughNext") => one_sided(LinearTermination::ThroughNext {}),
        Some("ToFace") => {
            let Some(face) = feature.properties.get("Face") else {
                return Ok(None);
            };
            one_sided(LinearTermination::ToFace {
                face: FaceSelection::Native(copy_projected_feature_text(ctx, face)?),
                offset: None,
            })
        }
        Some("ToVertex") => {
            let Some(vertex) = feature.properties.get("Vertex") else {
                return Ok(None);
            };
            one_sided(LinearTermination::ToVertex {
                vertex: VertexSelection::native(copy_projected_feature_text(ctx, vertex)?)
                    .unwrap_or(VertexSelection::Unresolved),
            })
        }
        Some("OffsetFromFace") => match length("Depth").or_else(sole_length) {
            Some(offset) => {
                let Some(face) = feature.properties.get("Face") else {
                    return Ok(None);
                };
                one_sided(LinearTermination::OffsetFromFace {
                    face: FaceSelection::Native(copy_projected_feature_text(ctx, face)?),
                    offset,
                })
            }
            None => one_sided(LinearTermination::Unresolved {}),
        },
        Some(_) => one_sided(LinearTermination::Unresolved {}),
    };
    let direction = match feature.properties.get("Direction") {
        Some(value) => {
            let Some(vector) =
                parse_vector3(value).and_then(cadmpeg_ir::features::FeatureDirection3::new)
            else {
                return Ok(None);
            };
            cadmpeg_ir::features::ExtrudeDirection::Explicit {
                vector,
                source: None,
            }
        }
        None => cadmpeg_ir::features::ExtrudeDirection::ProfileNormal {},
    };
    if matches!(direction, cadmpeg_ir::features::ExtrudeDirection::Explicit { vector, .. } if !valid_direction(vector.get()))
    {
        return Ok(None);
    }
    let profile = if let Some(source) = feature.properties.get("Profile") {
        ProfileRef::Planar(PlanarProfileRef::Native(copy_projected_feature_text(
            ctx,
            native_by_source
                .get(source.as_str())
                .copied()
                .unwrap_or(source.as_str()),
        )?))
    } else if let Some(children) = feature.properties.get("DissectableChildren") {
        ctx.charge_work(
            children.len() as u64,
            "scan SLDPRT extrusion child profiles",
        )?;
        let mut profiles = children
            .split(',')
            .map(str::trim)
            .filter(|source| !source.is_empty());
        let sole = profiles.next().filter(|_| profiles.next().is_none());
        match sole {
            Some(profile) => {
                ProfileRef::Planar(PlanarProfileRef::Native(copy_projected_feature_text(
                    ctx,
                    native_by_source.get(profile).copied().unwrap_or(profile),
                )?))
            }
            None => ProfileRef::Planar(PlanarProfileRef::Unresolved(copy_projected_feature_text(
                ctx,
                &feature.id,
            )?)),
        }
    } else if let Some(profile) = history_profile {
        ProfileRef::Planar(PlanarProfileRef::Native(copy_projected_feature_text(
            ctx, profile,
        )?))
    } else {
        ProfileRef::Planar(PlanarProfileRef::Unresolved(copy_projected_feature_text(
            ctx,
            &feature.id,
        )?))
    };
    Ok(Some(FeatureDefinition::Operation(
        FeatureOperation::Extrude {
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
        },
    )))
}

pub(super) fn project_hole(
    ctx: &DecodeContext<'_>,
    feature: &Feature,
    features_by_source: &HashMap<crate::records::FeatureSource, &Feature>,
    history_features: &[Feature],
) -> Result<Option<FeatureDefinition>, CodecError> {
    let Some((shape, profile)) =
        hole_shape_and_profile(feature, features_by_source, history_features)
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
        None | Some("Blind") => feature
            .parameters
            .get("Depth")
            .and_then(|value| parse_positive_length_mm(value))
            .or_else(|| profile.as_ref().and_then(|profile| profile.depth))
            .map(|length| LinearTermination::Blind {
                length: NonZeroLength::from(length),
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
            .map(|face| {
                crate::text_admission::format_retained(
                    ctx,
                    format_args!("{face}"),
                    "retain SLDPRT hole face reference",
                )
            })
            .transpose()?
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
        bottom: profile.as_ref().and_then(|profile| profile.bottom),
        taper_angle: profile.as_ref().and_then(|profile| profile.taper_angle),
        allow_multi_profile_faces: None,
    })))
}
fn hole_shape_and_profile(
    feature: &Feature,
    features_by_source: &HashMap<crate::records::FeatureSource, &Feature>,
    history_features: &[Feature],
) -> Option<(
    cadmpeg_ir::features::holes::HoleShape,
    Option<HoleProfileConstruction>,
)> {
    let profile = hole_profile_construction(feature, features_by_source, history_features);
    let diameter = feature
        .parameters
        .get("Diameter")
        .and_then(|value| parse_positive_length_mm(value))
        .or_else(|| profile.as_ref().map(|profile| profile.diameter));
    let has_counterbore = feature.parameters.contains_key("CounterboreDiameter")
        || feature.parameters.contains_key("CounterboreDepth");
    let has_countersink = feature.parameters.contains_key("CountersinkDiameter")
        || feature.parameters.contains_key("CountersinkAngle");
    let counterbore_diameter = feature
        .parameters
        .get("CounterboreDiameter")
        .and_then(|value| parse_positive_length_mm(value));
    let counterbore_depth = feature
        .parameters
        .get("CounterboreDepth")
        .and_then(|value| parse_positive_length_mm(value));
    let countersink_diameter = feature
        .parameters
        .get("CountersinkDiameter")
        .and_then(|value| parse_positive_length_mm(value));
    let countersink_angle = feature
        .parameters
        .get("CountersinkAngle")
        .and_then(|value| parse_bounded_angle_rad(value));
    let drill_point_angle = feature
        .parameters
        .get("DrillPointAngle")
        .and_then(|value| parse_bounded_angle_rad(value));
    let thread = feature
        .parameters
        .get("ThreadMajorDiameter")
        .and_then(|value| parse_positive_length_mm(value))
        .zip(
            feature
                .parameters
                .get("ThreadDepth")
                .and_then(|value| parse_positive_length_mm(value)),
        )
        .zip(drill_point_angle)
        .map(|((major_diameter, thread_depth), drill_point_angle)| {
            HoleConstruction::NativeThread {
                major_diameter,
                thread_depth,
                pitch: feature
                    .parameters
                    .get("ThreadPitch")
                    .and_then(|value| parse_positive_length_mm(value)),
                drill_point_angle,
            }
        });
    let construction = if has_counterbore && has_countersink {
        hole_form(HoleKind::Unresolved(None))
    } else if has_counterbore {
        match (counterbore_diameter, counterbore_depth) {
            (Some(diameter), Some(depth)) => hole_form(drill_point_angle.map_or(
                HoleKind::Counterbore { diameter, depth },
                |drill_point_angle| HoleKind::CounterboreDrilled {
                    diameter,
                    depth,
                    drill_point_angle,
                },
            )),
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
        profile.as_ref().map_or_else(
            || hole_form(HoleKind::Simple),
            |profile| profile.construction.clone(),
        )
    };
    let shape = cadmpeg_ir::features::holes::HoleShape::new(
        construction,
        profile.as_ref().and_then(|profile| profile.exit_kind),
        diameter,
    )
    .ok()?;
    Some((shape, profile))
}

pub(crate) fn threaded_hole_major_diameter(
    feature: &Feature,
    features_by_source: &HashMap<crate::records::FeatureSource, &Feature>,
    history_features: &[Feature],
) -> Option<f64> {
    if classify(feature) != Some(FeatureClass::Hole) {
        return None;
    }
    let (shape, _) = hole_shape_and_profile(feature, features_by_source, history_features)?;
    let HoleConstruction::NativeThread { major_diameter, .. } = shape.construction() else {
        return None;
    };
    Some(major_diameter.get())
}

#[derive(Debug, Clone, PartialEq)]
pub(super) struct HoleProfileConstruction {
    pub(in crate::history) diameter: cadmpeg_ir::scalar::PositiveLength,
    pub(in crate::history) depth: Option<PositiveLength>,
    pub(in crate::history) construction: HoleConstruction,
    pub(in crate::history) exit_kind: Option<HoleKind>,
    pub(in crate::history) bottom: Option<HoleBottom>,
    pub(in crate::history) taper_angle: Option<cadmpeg_ir::scalar::InteriorAngle>,
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
        .filter_map(hole_sketch_construction);
    let mut sole = None;
    let mut multiple = false;
    let mut complete = None;
    for construction in constructions {
        if construction.depth.is_some() {
            if complete.is_some() {
                return None;
            }
            complete = Some(construction.clone());
        }
        if sole.is_some() {
            multiple = true;
        } else {
            sole = Some(construction);
        }
    }
    complete.or_else(|| if multiple { None } else { sole })
}

pub(super) fn hole_sketch_construction(profile: &Feature) -> Option<HoleProfileConstruction> {
    #[derive(Clone, Copy)]
    enum ParsedDimension {
        Diameter(PositiveLength),
        Length(PositiveLength),
        Angle(cadmpeg_ir::scalar::InteriorAngle),
    }

    const MAX_DIMENSIONS: usize = 7;
    const MAX_DIAMETERS: usize = 3;
    const MAX_LENGTHS: usize = 2;
    const MAX_ANGLES: usize = 2;
    let initial_length = PositiveLength::new(1.0)?;
    let initial_angle = cadmpeg_ir::scalar::InteriorAngle::new(std::f64::consts::FRAC_PI_2)?;
    let mut dimensions = [ParsedDimension::Length(initial_length); MAX_DIMENSIONS];
    let mut dimension_count = 0;
    let has_source_dimensions = profile
        .content
        .iter()
        .any(|content| matches!(content, FeatureContent::Dimension(_)));
    let expressions = profile
        .parameters
        .values()
        .filter(|_| !has_source_dimensions)
        .chain(profile.content.iter().filter_map(|content| match content {
            FeatureContent::Dimension(name) => profile.parameters.get(name.as_str()),
            FeatureContent::Feature(_) | FeatureContent::Text(_) => None,
        }));
    for expression in expressions {
        let dimension = if strip_diameter_modifier(expression).is_some() {
            parse_dimension_display_length(expression)
                .and_then(|value| PositiveLength::try_from(value).ok())
                .map(ParsedDimension::Diameter)
        } else if let Some(value) = parse_bounded_angle_rad(expression) {
            Some(ParsedDimension::Angle(value))
        } else {
            parse_positive_dimension_length_mm(expression).map(ParsedDimension::Length)
        };
        if let Some(dimension) = dimension {
            *dimensions.get_mut(dimension_count)? = dimension;
            dimension_count += 1;
        }
    }
    let dimensions = &dimensions[..dimension_count];
    let mut diameters = [initial_length; MAX_DIAMETERS];
    let mut lengths = [initial_length; MAX_LENGTHS];
    let mut angles = [initial_angle; MAX_ANGLES];
    let (mut diameter_count, mut length_count, mut angle_count) = (0, 0, 0);
    for dimension in dimensions {
        match dimension {
            ParsedDimension::Diameter(value) => {
                *diameters.get_mut(diameter_count)? = *value;
                diameter_count += 1;
            }
            ParsedDimension::Length(value) => {
                *lengths.get_mut(length_count)? = *value;
                length_count += 1;
            }
            ParsedDimension::Angle(value) => {
                *angles.get_mut(angle_count)? = *value;
                angle_count += 1;
            }
        }
    }
    let diameters = &mut diameters[..diameter_count];
    let lengths = &mut lengths[..length_count];
    let angles = &mut angles[..angle_count];
    diameters.sort_unstable_by(|left, right| left.get().total_cmp(&right.get()));
    lengths.sort_unstable_by(|left, right| left.get().total_cmp(&right.get()));
    angles.sort_unstable_by(|left, right| left.get().total_cmp(&right.get()));
    match (&*diameters, &*lengths, &*angles) {
        ([diameter], [depth], []) => Some(HoleProfileConstruction {
            diameter: *diameter,
            depth: Some(*depth),
            construction: hole_form(HoleKind::Simple),
            exit_kind: None,
            bottom: Some(HoleBottom::Flat),
            taper_angle: None,
        }),
        ([diameter], [depth], [drill_point_angle]) => Some(HoleProfileConstruction {
            diameter: *diameter,
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
                dimensions,
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
                diameter: *diameter,
                depth: Some(*drill_depth),
                construction: HoleConstruction::NativeThread {
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
        ) if diameter.get() < major_diameter.get()
            && thread_depth.get() < drill_depth.get()
            && taper_angle.get() < drill_point_angle.get() =>
        {
            Some(HoleProfileConstruction {
                diameter: *diameter,
                depth: Some(*drill_depth),
                construction: HoleConstruction::NativeThread {
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
            if matches!(dimensions.last(), Some(ParsedDimension::Diameter(_)))
                && diameter.get() < entry_diameter.get()
                && entry_depth.get() < depth.get() =>
        {
            Some(HoleProfileConstruction {
                diameter: *diameter,
                depth: Some(*depth),
                construction: hole_form(HoleKind::CounterboreDrilled {
                    diameter: *entry_diameter,
                    depth: *entry_depth,
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
            dimensions,
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
                diameter: *diameter,
                depth: Some(*through_depth),
                construction: hole_form(HoleKind::Counterbore {
                    diameter: *counterbore_diameter,
                    depth: *counterbore_depth,
                }),
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
        ) if diameter.get() < recess_diameter.get()
            && recess_diameter.get() < entry_diameter.get()
            && recess_depth.get() < drill_depth.get()
            && entry_angle.get() < drill_point_angle.get() =>
        {
            Some(HoleProfileConstruction {
                diameter: *diameter,
                depth: Some(*drill_depth),
                construction: hole_form(HoleKind::Counterdrill {
                    diameters: cadmpeg_ir::features::holes::CounterdrillDiameters::new(
                        *recess_diameter,
                        Some(*entry_diameter),
                    )
                    .ok()?,

                    depth: *recess_depth,
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
