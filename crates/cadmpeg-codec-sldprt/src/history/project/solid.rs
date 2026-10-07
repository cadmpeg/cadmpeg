// SPDX-License-Identifier: Apache-2.0
//! Extrude and hole projection.

use crate::classification::{classify, native_object_class, FeatureClass, NativeClassKind};
use crate::records::{Feature, FeatureContent};
use cadmpeg_core::decode::{DecodeContext, ScopedReservation};
use cadmpeg_core::CodecError;
use cadmpeg_ir::{
    features::{
        holes::{HoleBottom, HoleConstruction, HoleKind},
        BooleanOp, ExtrudeExtent, ExtrudeSide, FaceSelection, FeatureDefinition, FeatureOperation,
        LinearTermination, PlanarProfileRef, ProfileRef, VertexSelection,
    },
    scalar::{NonZeroLength, PositiveLength},
};
use std::collections::{BTreeMap, HashMap};

use super::{
    copy_projected_feature_text, parameter_literal, property_literal, property_text, property_value,
};
use crate::history::classify::extrude_feature_op;
use crate::history::literals::{
    admit_literal, parse_angle_rad, parse_boolean_op, parse_bounded_angle_rad,
    parse_dimension_display_length, parse_point3_mm, parse_positive_dimension_length_mm,
    parse_positive_length_mm, parse_vector3, strip_diameter_modifier, valid_direction,
};

/// Records of one history by identity, `None` where an identity repeats.
pub(super) type RecordsById<'a> = HashMap<&'a str, Option<&'a Feature>>;

/// Source records and non-origin sketches in source-key order. Repeated source
/// keys select the last record in history order.
pub(super) struct SourceFeatures<'f, 'c> {
    pub(super) records: BTreeMap<crate::records::FeatureSource, &'f Feature>,
    profiles: Vec<(crate::records::FeatureSource, &'f Feature)>,
    _storage: ScopedReservation<'c>,
}

impl<'f, 'c> SourceFeatures<'f, 'c> {
    pub(super) fn new(
        ctx: &'c DecodeContext<'_>,
        features: &'f [Feature],
    ) -> Result<Self, CodecError> {
        const OPERATION: &str = "index SLDPRT source profiles";
        let mut storage = ctx.reserve_scoped(0, OPERATION)?;
        let mut records = BTreeMap::new();
        for feature in ctx.admit_iter(features, OPERATION)? {
            if let Some(source) = feature.source_id {
                storage.with_storage(|| {
                    ctx.insert_btree_map(&mut records, source, feature, OPERATION)
                })?;
            }
        }
        let mut profiles = Vec::new();
        for (&source, &feature) in ctx.admit_iter(&records, OPERATION)? {
            if classify(feature) == Some(FeatureClass::Sketch)
                && feature.input_class.as_deref() != Some("moOriginProfileFeature_c")
            {
                ctx.push_scoped_vec(&mut storage, &mut profiles, (source, feature), OPERATION)?;
            }
        }
        Ok(Self { records, profiles, _storage: storage })
    }

    fn preceding_profile(
        &self,
        ctx: &DecodeContext<'_>,
        source: crate::records::FeatureSource,
    ) -> Result<Option<&'f str>, CodecError> {
        let end = ctx.partition_point(
            &self.profiles,
            |(key, _)| Ok(*key < source),
            "find SLDPRT preceding source profile",
        )?;
        Ok(end.checked_sub(1).map(|index| self.profiles[index].1.id.as_str()))
    }
}

/// A length from its literal, in millimetres or as a bare dimension value.
fn positive_length(value: &str) -> Option<PositiveLength> {
    parse_positive_length_mm(value).or_else(|| parse_positive_dimension_length_mm(value))
}

pub(super) fn project_extrude(
    ctx: &DecodeContext<'_>,
    feature: &Feature,
    native_by_source: &HashMap<String, &str>,
    source_features: &SourceFeatures<'_, '_>,
) -> Result<Option<FeatureDefinition>, CodecError> {
    const OPERATION: &str = "scan SLDPRT extrusion dimensions";
    // The one dimension name the content lists, however often it repeats.
    let mut source_depth = None;
    ctx.any_by(
        &feature.content,
        |content| {
            let FeatureContent::Dimension(name) = content else {
                return Ok(false);
            };
            match source_depth {
                Some(first) if !ctx.equal(first, name.as_str(), OPERATION)? => {
                    source_depth = None;
                    Ok(true)
                }
                _ => {
                    source_depth = Some(name.as_str());
                    Ok(false)
                }
            }
        },
        OPERATION,
    )?;
    let legacy_history_extrusion = feature.input_class.is_none()
        && feature.xml_tag.eq_ignore_ascii_case("Extrusion")
        && source_depth.is_some();
    let has_property =
        |name| ctx.contains_key_btree_map(&feature.properties, name, "test SLDPRT map key");
    let has_parameter =
        |name| ctx.contains_key_btree_map(&feature.parameters, name, "test SLDPRT map key");
    // Some modern ICE extrusions retain the dissection-root marker but omit
    // both the child list and the explicit profile property. Their profile is
    // still the nearest preceding non-origin sketch in the source namespace,
    // the same ownership rule used by the legacy history form. Restrict this
    // fallback to a sole source dimension so an operation with several
    // unassigned dimensions cannot acquire a profile by proximity alone.
    let root_history_extrusion = property_value(ctx, feature, "DissectableRoot")? == Some("true")
        && !has_property("DissectableChildren")?
        && !has_property("Profile")?
        && source_depth.is_some();
    let history_profile_extrusion = legacy_history_extrusion || root_history_extrusion;
    let implicit_modern_blind =
        feature.input_class.as_deref() == Some("moExtrusion_c") && source_depth.is_some();
    let history_profile = match feature.source_id {
        Some(source) if history_profile_extrusion => source_features.preceding_profile(ctx, source)?,
        _ => None,
    };
    let op = match property_value(ctx, feature, "Operation")?.and_then(parse_boolean_op) {
        Some(op) => op,
        None => {
            admit_literal(ctx, &feature.kind, "classify SLDPRT extrusion operation")?;
            extrude_feature_op(feature)
                .or_else(|| {
                    (legacy_history_extrusion && history_profile.is_some())
                        .then_some(BooleanOp::Join)
                })
                .unwrap_or(BooleanOp::Unresolved)
        }
    };
    let sole_length = || -> Result<_, CodecError> {
        let mut values = feature.parameters.values();
        let Some(sole) = values.next().filter(|_| values.next().is_none()) else {
            return Ok(None);
        };
        admit_literal(ctx, sole, "read SLDPRT feature literal")?;
        Ok(positive_length(sole))
    };
    let legacy_length = || -> Result<_, CodecError> {
        Ok(match source_depth {
            Some(name) => parameter_literal(ctx, feature, name)?.and_then(positive_length),
            None => None,
        })
    };
    let length = |name| -> Result<_, CodecError> {
        Ok(
            match parameter_literal(ctx, feature, name)?.and_then(parse_positive_length_mm) {
                Some(length) => Some(length),
                None if name == "Depth" => parameter_literal(ctx, feature, "D1")?
                    .and_then(parse_positive_dimension_length_mm),
                None => None,
            },
        )
    };
    let draft = match parameter_literal(ctx, feature, "Draft")? {
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
    let blind = |length: Option<PositiveLength>| match length {
        Some(length) => one_sided(LinearTermination::Blind {
            length: NonZeroLength::from(length),
        }),
        None => one_sided(LinearTermination::Unresolved {}),
    };
    let extent = match property_value(ctx, feature, "EndCondition")? {
        None if !has_parameter("Depth")?
            && !has_parameter("D1")?
            && !legacy_history_extrusion
            && !implicit_modern_blind =>
        {
            one_sided(LinearTermination::Unresolved {})
        }
        None | Some("Blind") => blind(match length("Depth")? {
            Some(length) => Some(length),
            None => match legacy_history_extrusion {
                true => legacy_length()?,
                false => None,
            }
            .map_or_else(sole_length, |length| Ok(Some(length)))?,
        }),
        Some("Symmetric") => {
            match length("Depth")?.map_or_else(sole_length, |length| Ok(Some(length)))? {
                Some(length) => ExtrudeExtent::Symmetric {
                    side: ExtrudeSide {
                        termination: LinearTermination::Blind {
                            length: NonZeroLength::from(length),
                        },
                        draft,
                    },
                },
                None => one_sided(LinearTermination::Unresolved {}),
            }
        }
        Some("TwoSided") => {
            let (Some(first), Some(second)) = (length("Depth")?, length("Depth2")?) else {
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
            let Some(face) = property_text(ctx, feature, "Face")? else {
                return Ok(None);
            };
            one_sided(LinearTermination::ToFace {
                face: FaceSelection::Native(face),
                offset: None,
            })
        }
        Some("ToVertex") => {
            let Some(vertex) = property_text(ctx, feature, "Vertex")? else {
                return Ok(None);
            };
            one_sided(LinearTermination::ToVertex {
                vertex: VertexSelection::native(vertex, ctx)?
                    .unwrap_or(VertexSelection::Unresolved),
            })
        }
        Some("OffsetFromFace") => {
            match length("Depth")?.map_or_else(sole_length, |length| Ok(Some(length)))? {
                Some(offset) => {
                    let Some(face) = property_text(ctx, feature, "Face")? else {
                        return Ok(None);
                    };
                    one_sided(LinearTermination::OffsetFromFace {
                        face: FaceSelection::Native(face),
                        offset,
                    })
                }
                None => one_sided(LinearTermination::Unresolved {}),
            }
        }
        Some(_) => one_sided(LinearTermination::Unresolved {}),
    };
    let direction = match property_literal(ctx, feature, "Direction")? {
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
    let native_profile = |source: &str| -> Result<ProfileRef, CodecError> {
        let native = ctx
            .get_hash_map(native_by_source, source, "look up SLDPRT hash key")?
            .copied()
            .unwrap_or(source);
        Ok(ProfileRef::Planar(PlanarProfileRef::Native(
            copy_projected_feature_text(ctx, native)?,
        )))
    };
    let unresolved_profile = || -> Result<ProfileRef, CodecError> {
        Ok(ProfileRef::Planar(PlanarProfileRef::Unresolved(
            copy_projected_feature_text(ctx, &feature.id)?,
        )))
    };
    let profile = if let Some(source) = property_value(ctx, feature, "Profile")? {
        native_profile(source)?
    } else if let Some(children) = property_value(ctx, feature, "DissectableChildren")? {
        ctx.charge_work(
            cadmpeg_core::decode::u64_from_index(children.len()),
            "scan SLDPRT extrusion child profiles",
        )?;
        let mut profiles = children
            .split(',')
            .map(str::trim)
            .filter(|source| !source.is_empty());
        match profiles.next().filter(|_| profiles.next().is_none()) {
            Some(profile) => native_profile(profile)?,
            None => unresolved_profile()?,
        }
    } else if let Some(profile) = history_profile {
        ProfileRef::Planar(PlanarProfileRef::Native(copy_projected_feature_text(
            ctx, profile,
        )?))
    } else {
        unresolved_profile()?
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
    features_by_source: &BTreeMap<crate::records::FeatureSource, &Feature>,
    records: &RecordsById<'_>,
) -> Result<Option<FeatureDefinition>, CodecError> {
    let Some((shape, profile)) = hole_shape_and_profile(ctx, feature, features_by_source, |id| {
        Ok(ctx
            .get_hash_map(records, id, "look up SLDPRT hole profile record")?
            .copied()
            .flatten())
    })?
    else {
        return Ok(None);
    };
    let extent = match property_value(ctx, feature, "EndCondition")? {
        None | Some("Blind")
            if profile
                .as_ref()
                .is_some_and(|profile| profile.exit_kind.is_some()) =>
        {
            Some(LinearTermination::ThroughAll {})
        }
        None | Some("Blind") => parameter_literal(ctx, feature, "Depth")?
            .and_then(parse_positive_length_mm)
            .or_else(|| profile.as_ref().and_then(|profile| profile.depth))
            .map(|length| LinearTermination::Blind {
                length: NonZeroLength::from(length),
            }),
        Some("ThroughAll") => Some(LinearTermination::ThroughAll {}),
        Some(_) => None,
    };
    let position = property_literal(ctx, feature, "Position")?.and_then(parse_point3_mm);
    let direction = match position {
        Some(_) => property_literal(ctx, feature, "Direction")?
            .and_then(parse_vector3)
            .filter(|direction| valid_direction(*direction)),
        None => None,
    };
    let placements = match position.zip(direction) {
        Some((position, direction)) => {
            match cadmpeg_ir::features::FeatureDirection3::new(direction) {
                Some(direction) => {
                    let mut placements = Vec::new();
                    ctx.push_vec(
                        &mut placements,
                        cadmpeg_ir::features::holes::HolePlacement::Directed {
                            position,
                            direction,
                        },
                        "collect SLDPRT hole placements",
                    )?;
                    Some(placements)
                }
                None => None,
            }
        }
        None => None,
    };
    Ok(Some(FeatureDefinition::Operation(FeatureOperation::Hole {
        profile: None,
        profile_filter: None,
        face: property_text(ctx, feature, "Face")?.map(FaceSelection::Native),
        direction: None,
        placements,
        shape,

        extent,
        bottom: profile.as_ref().and_then(|profile| profile.bottom),
        taper_angle: profile.as_ref().and_then(|profile| profile.taper_angle),
        allow_multi_profile_faces: None,
    })))
}

fn hole_shape_and_profile<'f>(
    ctx: &DecodeContext<'_>,
    feature: &Feature,
    features_by_source: &BTreeMap<crate::records::FeatureSource, &'f Feature>,
    record: impl FnMut(&str) -> Result<Option<&'f Feature>, CodecError>,
) -> Result<
    Option<(
        cadmpeg_ir::features::holes::HoleShape,
        Option<HoleProfileConstruction>,
    )>,
    CodecError,
> {
    let profile = hole_profile_construction(ctx, feature, features_by_source, record)?;
    let length = |name| -> Result<_, CodecError> {
        Ok(parameter_literal(ctx, feature, name)?.and_then(parse_positive_length_mm))
    };
    let angle = |name| -> Result<_, CodecError> {
        Ok(parameter_literal(ctx, feature, name)?.and_then(parse_bounded_angle_rad))
    };
    let has = |name| ctx.contains_key_btree_map(&feature.parameters, name, "test SLDPRT map key");
    let diameter = length("Diameter")?.or_else(|| profile.as_ref().map(|profile| profile.diameter));
    let has_counterbore = has("CounterboreDiameter")? || has("CounterboreDepth")?;
    let has_countersink = has("CountersinkDiameter")? || has("CountersinkAngle")?;
    let counterbore_diameter = length("CounterboreDiameter")?;
    let counterbore_depth = length("CounterboreDepth")?;
    let countersink_diameter = length("CountersinkDiameter")?;
    let countersink_angle = angle("CountersinkAngle")?;
    let drill_point_angle = angle("DrillPointAngle")?;
    let thread = match (
        length("ThreadMajorDiameter")?,
        length("ThreadDepth")?,
        drill_point_angle,
    ) {
        (Some(major_diameter), Some(thread_depth), Some(drill_point_angle)) => {
            Some(HoleConstruction::NativeThread {
                major_diameter,
                thread_depth,
                pitch: length("ThreadPitch")?,
                drill_point_angle,
            })
        }
        _ => None,
    };
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
        match &profile {
            Some(profile) => profile
                .construction
                .try_clone_for_decode(ctx, "copy SLDPRT hole profile construction")?,
            None => hole_form(HoleKind::Simple),
        }
    };
    let Ok(shape) = cadmpeg_ir::features::holes::HoleShape::new(
        construction,
        profile.as_ref().and_then(|profile| profile.exit_kind),
        diameter,
    ) else {
        return Ok(None);
    };
    Ok(Some((shape, profile)))
}

/// The major diameter of a threaded hole. A dissection child named by record
/// identity is found by a charged search of `history_features`.
pub(crate) fn threaded_hole_major_diameter(
    ctx: &DecodeContext<'_>,
    feature: &Feature,
    features_by_source: &BTreeMap<crate::records::FeatureSource, &Feature>,
    history_features: &[Feature],
) -> Result<Option<f64>, CodecError> {
    const OPERATION: &str = "find SLDPRT hole profile record";
    if classify(feature) != Some(FeatureClass::Hole) {
        return Ok(None);
    }
    let Some((shape, _)) = hole_shape_and_profile(ctx, feature, features_by_source, |id| {
        let matches = |candidate: &Feature| ctx.equal(candidate.id.as_str(), id, OPERATION);
        let Some(first) = ctx.position_by(history_features, matches, OPERATION)? else {
            return Ok(None);
        };
        let rest = &history_features[first + 1..];
        Ok(match ctx.position_by(rest, matches, OPERATION)? {
            Some(_) => None,
            None => Some(&history_features[first]),
        })
    })?
    else {
        return Ok(None);
    };
    let HoleConstruction::NativeThread { major_diameter, .. } = shape.construction() else {
        return Ok(None);
    };
    Ok(Some(major_diameter.get()))
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

fn hole_profile_construction<'f>(
    ctx: &DecodeContext<'_>,
    feature: &Feature,
    features_by_source: &BTreeMap<crate::records::FeatureSource, &'f Feature>,
    mut record: impl FnMut(&str) -> Result<Option<&'f Feature>, CodecError>,
) -> Result<Option<HoleProfileConstruction>, CodecError> {
    const OPERATION: &str = "scan SLDPRT hole profile children";
    let Some(children) = property_value(ctx, feature, "DissectableChildren")? else {
        return Ok(None);
    };
    ctx.charge_work(
        cadmpeg_core::decode::u64_from_index(children.len()),
        OPERATION,
    )?;
    let mut sole = None;
    let mut multiple = false;
    let mut complete = None;
    for source in children
        .split(',')
        .map(str::trim)
        .filter(|source| !source.is_empty())
    {
        let by_source = match crate::records::FeatureSource::try_from(source) {
            Ok(source) => ctx
                .get_btree_map(features_by_source, &source, "look up SLDPRT feature source")?
                .copied(),
            Err(_) => None,
        };
        let profile = match by_source {
            Some(profile) => profile,
            None => match record(source)? {
                Some(profile) => profile,
                None => continue,
            },
        };
        if classify(profile) != Some(FeatureClass::Sketch) {
            continue;
        }
        let Some(construction) = hole_sketch_construction(ctx, profile)? else {
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
    ctx: &DecodeContext<'_>,
    profile: &Feature,
) -> Result<Option<HoleProfileConstruction>, CodecError> {
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
    let Some(initial_length) = PositiveLength::new(1.0) else {
        return Ok(None);
    };
    let Some(initial_angle) = cadmpeg_ir::scalar::InteriorAngle::new(std::f64::consts::FRAC_PI_2)
    else {
        return Ok(None);
    };
    let mut dimensions = [ParsedDimension::Length(initial_length); MAX_DIMENSIONS];
    let mut dimension_count = 0;
    let has_source_dimensions = ctx.any_by(
        &profile.content,
        |content| Ok(matches!(content, FeatureContent::Dimension(_))),
        "scan SLDPRT hole_sketch_construction values",
    )?;
    let expressions = ctx
        .admit_iter(&profile.parameters, "scan SLDPRT hole sketch parameters")?
        .map(|(_, value)| Ok::<_, CodecError>(value))
        .filter(|_| !has_source_dimensions)
        .chain(
            ctx.admit_iter(&profile.content, "scan SLDPRT hole sketch dimensions")?
                .map(|content| {
                    Ok::<_, CodecError>(match content {
                        FeatureContent::Dimension(name) => ctx.get_btree_map(
                            &(profile.parameters),
                            name.as_str(),
                            "look up SLDPRT ordered key",
                        )?,
                        FeatureContent::Feature(_) | FeatureContent::Text(_) => None,
                    })
                })
                .filter_map(Result::transpose),
        );
    for expression in expressions {
        let expression = expression?;
        admit_literal(ctx, expression, "parse SLDPRT hole sketch dimension")?;
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
            let Some(slot) = dimensions.get_mut(dimension_count) else {
                return Ok(None);
            };
            *slot = dimension;
            dimension_count += 1;
        }
    }
    let dimensions = &dimensions[..dimension_count];
    let mut diameters = [initial_length; MAX_DIAMETERS];
    let mut lengths = [initial_length; MAX_LENGTHS];
    let mut angles = [initial_angle; MAX_ANGLES];
    let (mut diameter_count, mut length_count, mut angle_count) = (0, 0, 0);
    for dimension in ctx.admit_iter(dimensions, "scan SLDPRT hole profile dimensions")? {
        match dimension {
            ParsedDimension::Diameter(value) => {
                let Some(slot) = diameters.get_mut(diameter_count) else {
                    return Ok(None);
                };
                *slot = *value;
                diameter_count += 1;
            }
            ParsedDimension::Length(value) => {
                let Some(slot) = lengths.get_mut(length_count) else {
                    return Ok(None);
                };
                *slot = *value;
                length_count += 1;
            }
            ParsedDimension::Angle(value) => {
                let Some(slot) = angles.get_mut(angle_count) else {
                    return Ok(None);
                };
                *slot = *value;
                angle_count += 1;
            }
        }
    }
    let diameters = &mut diameters[..diameter_count];
    let lengths = &mut lengths[..length_count];
    let angles = &mut angles[..angle_count];
    ctx.sort_unstable_by_key(
        diameters,
        |value| value.get(),
        f64::total_cmp,
        "sldprt hole profile diameters sort",
    )?;
    ctx.sort_unstable_by_key(
        lengths,
        |value| value.get(),
        f64::total_cmp,
        "sldprt hole profile lengths sort",
    )?;
    ctx.sort_unstable_by_key(
        angles,
        |value| value.get(),
        f64::total_cmp,
        "sldprt hole profile angles sort",
    )?;
    Ok(match (&*diameters, &*lengths, &*angles) {
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
            let Ok(diameters) = cadmpeg_ir::features::holes::CounterdrillDiameters::new(
                *recess_diameter,
                Some(*entry_diameter),
            ) else {
                return Ok(None);
            };
            Some(HoleProfileConstruction {
                diameter: *diameter,
                depth: Some(*drill_depth),
                construction: hole_form(HoleKind::Counterdrill {
                    diameters,
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
    })
}

pub(crate) fn is_hole_profile_construction(
    ctx: &DecodeContext<'_>,
    feature: &Feature,
) -> Result<bool, CodecError> {
    Ok(hole_sketch_construction(ctx, feature)?.is_some())
}

#[cfg(test)]
mod tests {
    use super::SourceFeatures;
    use crate::history::tests::feature;
    use crate::records::{Feature, FeatureSource};
    use cadmpeg_core::decode::ResourceDimension;
    use cadmpeg_core::CodecError;

    fn sketch(id: &str, source: &str) -> Feature {
        let mut record = feature(id, Some(source), 0);
        record.kind = "Sketch".into();
        record.xml_tag = "Sketch".into();
        record
    }

    #[test]
    fn source_profile_index_preserves_key_order_and_last_duplicate() {
        let mut origin = sketch("origin", "18");
        origin.input_class = Some("moOriginProfileFeature_c".into());
        let features = [
            sketch("later", "19"),
            sketch("overwritten", "10"),
            origin,
            sketch("first", "9"),
            feature("last-duplicate-is-not-a-sketch", Some("10"), 0),
        ];
        let ctx = cadmpeg_test_support::service_decode_context();
        let index = SourceFeatures::new(&ctx, &features).unwrap();
        for (source, expected) in [("9", None), ("19", Some("first")), ("20", Some("later"))] {
            assert_eq!(index.preceding_profile(&ctx, FeatureSource::try_from(source).unwrap()).unwrap(), expected);
        }
        assert_eq!(index.records.get(&FeatureSource::try_from("10").unwrap()).unwrap().id, "last-duplicate-is-not-a-sketch");
    }

    #[test]
    fn source_profile_index_refuses_before_search() {
        let features = [sketch("first", "9"), sketch("second", "19")];
        let error = crate::test_support::work_refusal_at("find SLDPRT preceding source profile", |ctx| {
            let index = SourceFeatures::new(ctx, &features)?;
            index.preceding_profile(ctx, FeatureSource::try_from("20").unwrap())
        });
        assert!(matches!(error, CodecError::ResourceLimit(limit) if limit.dimension == ResourceDimension::WorkUnits));
    }
}
