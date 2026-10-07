//! Hole construction, bore topology and hole axis projection.

use super::compact_reference_planes::{
    compact_profile_component_plane_frame, CompactReferencePlaneIndex,
};
use super::curves::{lane_sketch_plane_frames, SketchPlaneFrame, SketchPlaneUAxisSource};
use super::grid::{quantize, GridCoordinate};
use super::helix::fit_helix_polyline;
use super::reference_geometry::{explicit_reference_plane_frame, reference_plane_frame_key};
use super::relation_loci::same_dimension_length;
use super::scalars::feature_object_name;
use super::transforms::sketch_frame_marker_transform;
use super::{is_class_token, CLASS_MARKER};
use crate::classification::{classify, FeatureClass};
use crate::records::operand_tag::NativeOperandTag;
use crate::records::{
    FeatureInputLane, FeatureInputOperandKind, FeatureInputRelationFamily, FeatureInputScalarRole,
    SketchInputKind,
};
use cadmpeg_core::convert::f64_from_i64;
use cadmpeg_core::decode::{u64_from_index, DecodeContext, View};
use cadmpeg_core::CodecError;
use cadmpeg_ir::geometry::{SolvedSurfaceGeometry, Surface, SurfaceGeometry};
use cadmpeg_ir::math::{Point2, Point3, Vector3};
use cadmpeg_ir::sketches::{
    Sketch, SketchEntity, SketchEntityId, SketchGeometry, SketchGeometryDefinition, SketchId,
    SpatialSketch, SpatialSketchEntity, SpatialSketchGeometryDefinition,
};
use cadmpeg_ir::topology::{Coedge, Edge, Face, Loop, Point, Sense, Vertex};
use cadmpeg_ir::{
    features::{
        holes::{HoleBottom, HoleKind, HolePlacement},
        FeatureDefinition, FeatureDirection3, FeatureOperation, FinitePoint3, LinearTermination,
    },
    scalar::Length,
};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

const EPS_HOLE_POSITION: f64 = 1.0e-8;
const EPS_HOLE_GEOMETRY: f64 = 1.0e-9;
const EPS_HOLE_DEGENERATE_NORMAL: f64 = 1.0e-10;
const EPS_HOLE_EXACT_GEOMETRY: f64 = 1.0e-12;

use crate::records::FeatureSource;
use crate::records::ObjectId;

/// Resolve helix placement from the counted curve mesh stored in its feature
/// object. Promotion requires one mesh stream and a circular-helix fit whose
/// residual is small relative to its radius.
pub(crate) fn project_helix_axes(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    model_features: &mut [cadmpeg_ir::features::Feature],
    histories: &[crate::records::FeatureHistory],
    lanes: &[FeatureInputLane],
) -> Result<(), cadmpeg_core::CodecError> {
    let mut records_storage = ctx.reserve_scoped(0, "index SLDPRT helix features")?;
    let mut records = HashMap::new();

    for iteration_history in ctx.admit_iter(histories, "scan SLDPRT holes source records")? {
        for feature in ctx.admit_iter(&iteration_history.features, "scan SLDPRT holes records")? {
            records_storage.with_storage(|| {
                ctx.insert_hash_map(
                    &mut records,
                    feature.id.as_str(),
                    feature,
                    "resolve SLDPRT holes keys",
                )
            })?;
        }
    }
    for model_feature in model_features {
        ctx.charge_work(1, "scan SLDPRT helix features")?;
        let FeatureDefinition::Operation(FeatureOperation::HelixNativeAxis {
            axial_rise,
            revolutions,
            start_angle,
            clockwise,
            ..
        }) = model_feature.evaluation.definition()
        else {
            continue;
        };
        let Some(native_ref) = model_feature.native_ref.as_deref() else {
            continue;
        };
        let Some(record) = ctx
            .get_hash_map(&records, native_ref, "resolve SLDPRT holes keys")?
            .copied()
        else {
            continue;
        };
        let mut meshes = Vec::new();
        for lane in ctx.admit_iter(lanes, "scan SLDPRT holes records")? {
            let Some(name) = feature_object_name(record, lane) else {
                continue;
            };
            let start = usize::try_from(name.offset).ok();
            let mut next_offset = None;
            for history in ctx.admit_iter(histories, "scan SLDPRT helix object boundaries")? {
                for feature in
                    ctx.admit_iter(&history.features, "scan SLDPRT helix object boundaries")?
                {
                    let Some(candidate) = feature_object_name(feature, lane) else {
                        continue;
                    };
                    if candidate.offset > name.offset
                        && next_offset.is_none_or(|offset| candidate.offset < offset)
                    {
                        next_offset = Some(candidate.offset);
                    }
                }
            }
            let end = next_offset
                .and_then(|offset| usize::try_from(offset).ok())
                .unwrap_or(lane.native_payload.len());
            let Some(object) = start.and_then(|start| lane.native_payload.get(start..end)) else {
                continue;
            };
            let streams = crate::parasolid::extract_streams_with_offsets(object, ctx)?;
            for stream in ctx.admit_iter(&streams, "scan SLDPRT helix streams")? {
                if let Some(points) = crate::parasolid::mesh_polyline_from_header(
                    ctx,
                    &stream.payload,
                    &stream.header,
                )? {
                    ctx.push_vec(&mut meshes, points, "collect SLDPRT helix meshes")?;
                }
            }
        }
        let [points] = meshes.as_slice() else {
            continue;
        };
        let Some((axis_origin, mut axis_direction, radius, fitted_rise)) =
            fit_helix_polyline(ctx, points, *revolutions, *clockwise)?
        else {
            continue;
        };
        if fitted_rise * axial_rise.get() < 0.0 {
            axis_direction = Vector3::new(-axis_direction.x, -axis_direction.y, -axis_direction.z);
        }
        let Some(last_point) = points.last() else {
            continue;
        };
        let signed_rise = Vector3::new(
            last_point.x - points[0].x,
            last_point.y - points[0].y,
            last_point.z - points[0].z,
        )
        .dot(axis_direction);
        let Ok(pitch) = cadmpeg_ir::scalar::NonZeroLength::try_from(
            Length::new(signed_rise / revolutions.get()).ok_or_else(|| {
                cadmpeg_core::CodecError::Malformed(
                    "SolidWorks projected length must be finite".into(),
                )
            })?,
        ) else {
            continue;
        };
        model_feature
            .evaluation
            .set_definition(FeatureDefinition::Operation(FeatureOperation::Helix {
                axis_origin: FinitePoint3::new(axis_origin).ok_or_else(|| {
                    cadmpeg_core::CodecError::Malformed(
                        "SolidWorks helix origin must be finite".into(),
                    )
                })?,
                axis_direction: FeatureDirection3::new(axis_direction).ok_or_else(|| {
                    cadmpeg_core::CodecError::Malformed(
                        "SolidWorks helix direction must have finite nonzero norm".into(),
                    )
                })?,
                radius: cadmpeg_ir::scalar::PositiveLength::new(radius).ok_or_else(|| {
                    cadmpeg_core::CodecError::Malformed(
                        "SolidWorks projected length must be finite".into(),
                    )
                })?,
                shape: cadmpeg_ir::features::HelixShape::Cylindrical { pitch },
                revolutions: *revolutions,
                start_angle: *start_angle,
                clockwise: *clockwise,
                segment_turns: None,
                construction_style: None,
            }));
    }

    Ok(())
}

fn hole_position_sketch_source(
    ctx: &DecodeContext<'_>,
    feature: &crate::records::Feature,
    lane: &FeatureInputLane,
) -> Result<Option<u32>, CodecError> {
    const OPERATION: &str = "scan SLDPRT hole position source";
    if classify(feature) != Some(FeatureClass::Hole) {
        return Ok(None);
    }
    let Some(name) = feature_object_name(feature, lane) else {
        return Ok(None);
    };
    // Legacy keyword records may omit the XML source id while the serialized
    // object name still carries the stable object id used by the input lane.
    let Some(source) = feature
        .source_value()
        .or_else(|| name.object_id.and_then(ObjectId::value))
    else {
        return Ok(None);
    };
    let units = ctx
        .admit_iter(name.value.as_str(), OPERATION)?
        .encode_utf16()
        .count();
    let Some(offset) = usize::try_from(name.offset).ok().and_then(|offset| {
        units
            .checked_mul(2)
            .and_then(|bytes| bytes.checked_add(6))
            .and_then(|bytes| offset.checked_add(bytes))
    }) else {
        return Ok(None);
    };
    if lane.native_payload.get(offset..offset + 8)
        != Some(&[0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x40])
        || lane.native_payload.get(offset + 8..offset + 12) != Some(&source.to_le_bytes())
        || lane.native_payload.get(offset + 12..offset + 16) != Some(&[0x00; 4])
    {
        return Ok(None);
    }
    let body_start = offset + 16;
    let body_end = offset + 144;
    let Some(body) = lane.native_payload.get(body_start..body_end) else {
        return Ok(None);
    };
    let Some(window) = std::num::NonZeroUsize::new(12) else {
        return Err(CodecError::malformed("empty hole position source window"));
    };
    let mut source = None;
    for bytes in ctx.admit_iter(body, OPERATION)?.windows(window) {
        let candidate = (bytes[..2] == [0x00, 0xc0]
            && (bytes[6..12] == [0; 6] || bytes[6..12] == [0, 0, 0, 0, 0xff, 0xfe]))
        .then(|| View::u32_le_at(bytes, 2))
        .flatten()
        .filter(|source| *source != 0 && *source != u32::MAX);
        let Some(candidate) = candidate else {
            continue;
        };
        if source.is_some_and(|source| source != candidate) {
            return Ok(None);
        }
        source = Some(candidate);
    }
    for child in ctx.admit_iter(&lane.names, OPERATION)? {
        let Ok(child_offset) = usize::try_from(child.offset) else {
            continue;
        };
        if !(body_start..body_end).contains(&child_offset) {
            continue;
        }
        let Some(child_source) = child.object_id.and_then(ObjectId::value) else {
            continue;
        };
        let units = ctx
            .admit_iter(child.value.as_str(), OPERATION)?
            .encode_utf16()
            .count();
        let Some(trailer) = units
            .checked_mul(2)
            .and_then(|bytes| bytes.checked_add(6))
            .and_then(|bytes| child_offset.checked_add(bytes))
        else {
            continue;
        };
        let Some(trailer_end) = trailer.checked_add(12) else {
            continue;
        };
        if trailer_end > body_end {
            continue;
        }
        if lane.native_payload.get(trailer..trailer + 8)
            == Some(&[0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x40])
            && lane.native_payload.get(trailer + 8..trailer + 12)
                == Some(&child_source.to_le_bytes())
        {
            if source.is_some_and(|source| source != child_source) {
                return Ok(None);
            }
            source = Some(child_source);
        }
    }
    Ok(source)
}

pub(crate) fn enrich_history_hole_constructions(
    ctx: &DecodeContext<'_>,
    histories: &mut [crate::records::FeatureHistory],
    lanes: &[FeatureInputLane],
) -> Result<(), CodecError> {
    const OPERATION: &str = "enrich SLDPRT hole profile ownership";
    for history in histories {
        ctx.charge_work(1, OPERATION)?;
        let mut additions = Vec::new();
        for (feature_index, feature) in history.features.iter().enumerate() {
            ctx.charge_work(1, OPERATION)?;
            if classify(feature) != Some(FeatureClass::Hole)
                || feature.properties.contains_key("DissectableChildren")
            {
                continue;
            }
            let profile = if let Some(profile) =
                hole_profile_from_position_source(ctx, feature, history, lanes)?
            {
                Some(profile)
            } else {
                ctx.charge_work(
                    u64_from_index(history.features.len())
                        .checked_mul(2)
                        .ok_or_else(|| ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX))?,
                    OPERATION,
                )?;
                hole_profile_from_child_order(ctx, feature, history)?
            };
            let Some((profile, rank)) = profile else {
                continue;
            };
            ctx.reserve_vec(&mut additions, 1, OPERATION)?;
            additions.push((feature_index, copy_hole_profile_source(ctx, profile)?, rank));
        }
        let claimed_profiles = claimed_hole_profiles(ctx, &history.features)?;
        let mut profile_claim_ranks_storage = ctx.reserve_scoped(0, "resolve SLDPRT holes keys")?;
        let mut profile_claim_ranks = HashMap::<String, (u8, usize)>::new();
        for (_, profile, rank) in ctx.admit_iter(&additions, "scan SLDPRT holes records")? {
            if !ctx.contains_key_hash_map(&profile_claim_ranks, profile, OPERATION)? {
                profile_claim_ranks_storage.with_storage(|| {
                    let copy = ctx.format_retained(format_args!("{profile}"), OPERATION)?;
                    ctx.insert_hash_map(
                        &mut profile_claim_ranks,
                        copy,
                        (0, 0),
                        "resolve SLDPRT holes keys",
                    )
                })?;
            }
            if let Some(entry) = ctx.get_mut_hash_map(
                &mut profile_claim_ranks,
                profile,
                "resolve SLDPRT holes keys",
            )? {
                match rank.cmp(&entry.0) {
                    std::cmp::Ordering::Greater => *entry = (*rank, 1),
                    std::cmp::Ordering::Equal => {
                        entry.1 = entry.1.checked_add(1).ok_or_else(|| {
                            ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX)
                        })?;
                    }
                    std::cmp::Ordering::Less => {}
                }
            }
        }
        ctx.retain_vec(
            &mut additions,
            |(_, profile, rank)| {
                Ok(
                    !ctx.contains_hash_set(&claimed_profiles, profile.as_str(), OPERATION)?
                        && ctx.get_hash_map(&profile_claim_ranks, profile, OPERATION)?
                            == Some(&(*rank, 1)),
                )
            },
            OPERATION,
        )?;
        drop(claimed_profiles);
        for (feature_index, profile_source, _) in additions {
            ctx.charge_work(1, OPERATION)?;
            ctx.insert_btree_map(
                &mut history.features[feature_index].properties,
                cadmpeg_core::nonblank_literal!("DissectableChildren"),
                profile_source,
                OPERATION,
            )?;
        }
        let claimed_profiles = claimed_hole_profiles(ctx, &history.features)?;
        let mut interval_additions = Vec::new();
        for (feature_index, feature) in history.features.iter().enumerate() {
            ctx.charge_work(1, OPERATION)?;
            if classify(feature) != Some(FeatureClass::Hole)
                || feature.properties.contains_key("DissectableChildren")
            {
                continue;
            }
            let Some(source) = feature.source_value() else {
                continue;
            };
            ctx.charge_work(u64_from_index(history.features.len()), OPERATION)?;
            let Some(upper) = history
                .features
                .iter()
                .filter(|candidate| classify(candidate) == Some(FeatureClass::Hole))
                .filter_map(crate::records::Feature::source_value)
                .filter(|candidate| *candidate > source)
                .min()
            else {
                continue;
            };
            ctx.charge_work(u64_from_index(history.features.len()), OPERATION)?;
            let mut first = None;
            let mut ambiguous = false;
            for candidate in &history.features {
                let source_text = candidate.source_id.map(String::from);
                let identity = source_text.as_deref().unwrap_or(&candidate.id);
                if ctx.contains_hash_set(
                    &claimed_profiles,
                    identity,
                    "resolve SLDPRT holes keys",
                )? || !candidate
                    .source_value()
                    .is_some_and(|candidate| source < candidate && candidate < upper)
                    || classify(candidate) != Some(FeatureClass::Sketch)
                    || !crate::history::project::solid::is_hole_profile_construction(
                        ctx, candidate,
                    )?
                {
                    continue;
                }
                if first.is_some() {
                    ambiguous = true;
                    break;
                }
                first = Some(candidate);
            }
            let Some(profile) = first else {
                continue;
            };
            if ambiguous {
                continue;
            }
            ctx.reserve_vec(&mut interval_additions, 1, OPERATION)?;
            interval_additions.push((feature_index, copy_hole_profile_source(ctx, profile)?));
        }
        drop(claimed_profiles);
        let mut interval_claim_counts_storage =
            ctx.reserve_scoped(0, "resolve SLDPRT holes keys")?;
        let mut interval_claim_counts = HashMap::<String, usize>::new();
        for (_, profile) in
            ctx.admit_iter(&(interval_additions)[..], "scan SLDPRT holes records")?
        {
            if !ctx.contains_key_hash_map(&interval_claim_counts, profile, OPERATION)? {
                interval_claim_counts_storage.with_storage(|| {
                    let copy = ctx.format_retained(format_args!("{profile}"), OPERATION)?;
                    ctx.insert_hash_map(
                        &mut interval_claim_counts,
                        copy,
                        0,
                        "resolve SLDPRT holes keys",
                    )
                })?;
            }
            if let Some(count) = ctx.get_mut_hash_map(
                &mut interval_claim_counts,
                profile,
                "resolve SLDPRT holes keys",
            )? {
                *count = count
                    .checked_add(1)
                    .ok_or_else(|| ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX))?;
            }
        }
        for (feature_index, profile_source) in interval_additions {
            ctx.charge_work(1, OPERATION)?;
            if ctx.get_hash_map(
                &interval_claim_counts,
                &profile_source,
                "resolve SLDPRT holes keys",
            )? != Some(&1)
            {
                continue;
            }
            ctx.insert_btree_map(
                &mut history.features[feature_index].properties,
                cadmpeg_core::nonblank_literal!("DissectableChildren"),
                profile_source,
                OPERATION,
            )?;
        }
    }
    Ok(())
}

fn copy_hole_profile_source(
    ctx: &DecodeContext<'_>,
    profile: &crate::records::Feature,
) -> Result<String, CodecError> {
    const OPERATION: &str = "copy SLDPRT hole profile ownership";
    match profile.source_id {
        Some(FeatureSource::Reserved) => ctx.format_retained(format_args!("-1"), OPERATION),
        Some(FeatureSource::Id(source)) => {
            ctx.format_retained(format_args!("{}", source.value()), OPERATION)
        }
        None => {
            ctx.charge_work(u64_from_index(profile.id.len()), OPERATION)?;
            ctx.format_retained(format_args!("{}", profile.id), OPERATION)
        }
    }
}

fn hole_child_tokens<'value, 'ctx, 'arena>(
    ctx: &'ctx DecodeContext<'arena>,
    value: &'value str,
) -> Result<
    impl Iterator<Item = Result<&'value str, CodecError>> + use<'value, 'ctx, 'arena>,
    CodecError,
> {
    const OPERATION: &str = "scan SLDPRT hole child references";
    let mut start = 0usize;
    let mut position = 0usize;
    Ok(ctx
        .admit_iter(value, OPERATION)?
        .chain(std::iter::once(','))
        .filter_map(move |character| {
            let end = position;
            let Some(next) = position.checked_add(character.len_utf8()) else {
                return Some(Err(ctx.refuse_codec_limit(
                    OPERATION,
                    u64::MAX - 1,
                    u64::MAX,
                )));
            };
            position = next;
            if character != ',' {
                return None;
            }
            let token = value
                .get(start..end)
                .ok_or_else(|| ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX));
            start = position;
            Some(token)
        }))
}

fn unique_hole_child_profile<'history>(
    ctx: &DecodeContext<'_>,
    history: &'history crate::records::FeatureHistory,
    source: &str,
) -> Result<Option<&'history crate::records::Feature>, CodecError> {
    const OPERATION: &str = "match SLDPRT hole child profile";
    let mut profile = None;
    for candidate in ctx.admit_iter(&history.features, OPERATION)? {
        let source_matches = match FeatureSource::try_from(source) {
            Ok(source) => ctx.equal(&candidate.source_id, &Some(source), OPERATION)?,
            Err(_) => false,
        };
        if source_matches || ctx.equal(candidate.id.as_str(), source, OPERATION)? {
            if profile.is_some() {
                return Ok(None);
            }
            profile = Some(candidate);
        }
    }
    Ok(profile)
}

fn claimed_hole_profiles<'a>(
    ctx: &DecodeContext<'_>,
    features: &'a [crate::records::Feature],
) -> Result<HashSet<&'a str>, CodecError> {
    const OPERATION: &str = "index SLDPRT claimed hole profiles";
    let mut claims = HashSet::new();
    for feature in features {
        ctx.charge_work(1, OPERATION)?;
        let Some(children) = feature.properties.get("DissectableChildren") else {
            continue;
        };
        for child in hole_child_tokens(ctx, children)? {
            let child = ctx.trim_text(child?, OPERATION)?;
            if child.is_empty() {
                continue;
            }
            ctx.insert_hash_set(&mut claims, child, OPERATION)?;
        }
    }
    Ok(claims)
}

fn hole_profile_from_position_source<'a>(
    ctx: &DecodeContext<'_>,
    feature: &crate::records::Feature,
    history: &'a crate::records::FeatureHistory,
    lanes: &[FeatureInputLane],
) -> Result<Option<(&'a crate::records::Feature, u8)>, CodecError> {
    const OPERATION: &str = "resolve SLDPRT hole profile ownership";
    for lane in ctx.admit_iter(lanes, "scan SLDPRT holes records")? {
        ctx.charge_work(
            u64_from_index(lane.names.len())
                .checked_add(128)
                .ok_or_else(|| ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX))?,
            OPERATION,
        )?;
    }
    let mut source = None;
    for lane in ctx.admit_iter(lanes, OPERATION)? {
        let Some(candidate) = hole_position_sketch_source(ctx, feature, lane)? else {
            continue;
        };
        if source.is_some_and(|source| source != candidate) {
            return Ok(None);
        }
        source = Some(candidate);
    }
    let Some(source) = source else {
        return Ok(None);
    };
    let unique_position = || {
        let mut positions = history.features.iter().filter(|candidate| {
            (candidate.source_value() == Some(source) || candidate.ordinal == source)
                && classify(candidate) == Some(FeatureClass::Sketch)
        });
        let position = positions.next()?;
        positions.next().is_none().then_some(position)
    };
    let serialized_successor =
        || -> Result<Option<(&'a crate::records::Feature, u8)>, CodecError> {
            ctx.charge_work(u64_from_index(history.features.len()), OPERATION)?;
            let Some(position) = unique_position() else {
                return Ok(None);
            };
            let mut profile: Option<&crate::records::Feature> = None;
            for lane in ctx.admit_iter(lanes, "scan SLDPRT holes records")? {
                ctx.charge_work(
                    u64_from_index(lane.names.len())
                        .checked_add(128)
                        .ok_or_else(|| ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX))?,
                    OPERATION,
                )?;
                if hole_position_sketch_source(ctx, feature, lane)? != Some(source) {
                    continue;
                }
                ctx.charge_work(u64_from_index(lane.names.len()), OPERATION)?;
                let Some(position_offset) =
                    feature_object_name(position, lane).map(|name| name.offset)
                else {
                    return Ok(None);
                };
                let scan_work =
                    u64_from_index(history.features.len())
                        .checked_mul(u64_from_index(lane.names.len()).checked_add(1).ok_or_else(
                            || ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX),
                        )?)
                        .ok_or_else(|| ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX))?;
                ctx.charge_work(scan_work, OPERATION)?;
                let Some(minimum_offset) = history
                    .features
                    .iter()
                    .filter_map(|candidate| {
                        let offset = feature_object_name(candidate, lane)?.offset;
                        (offset > position_offset).then_some(offset)
                    })
                    .min()
                else {
                    return Ok(None);
                };
                ctx.charge_work(scan_work, OPERATION)?;
                let mut successors = history.features.iter().filter(|candidate| {
                    feature_object_name(candidate, lane)
                        .is_some_and(|name| name.offset == minimum_offset)
                });
                let Some(successor) = successors.next() else {
                    return Ok(None);
                };
                if successors.next().is_some()
                    || classify(successor) != Some(FeatureClass::Sketch)
                    || !crate::history::project::solid::is_hole_profile_construction(
                        ctx, successor,
                    )?
                {
                    return Ok(None);
                }
                if match profile {
                    Some(profile) => !ctx.equal(
                        &(profile.id),
                        &(successor.id),
                        "compare SLDPRT holes records",
                    )?,
                    None => false,
                } {
                    return Ok(None);
                }
                if profile.is_none() {
                    profile = Some(successor);
                }
            }
            Ok(profile.map(|profile| (profile, 4)))
        };
    if let Some(profile) = serialized_successor()? {
        return Ok(Some(profile));
    }
    let adjacent_sources = [source.checked_sub(1), source.checked_add(1)];
    ctx.charge_work(u64_from_index(history.features.len()), OPERATION)?;
    let mut first = None;
    let mut ambiguous = false;
    for candidate in &history.features {
        if !candidate
            .source_value()
            .is_some_and(|source| adjacent_sources.contains(&Some(source)))
            || classify(candidate) != Some(FeatureClass::Sketch)
            || !crate::history::project::solid::is_hole_profile_construction(ctx, candidate)?
        {
            continue;
        }
        if first.is_some() {
            ambiguous = true;
            break;
        }
        first = Some(candidate);
    }
    if let Some(profile) = first {
        if !ambiguous {
            return Ok(Some((profile, 3)));
        }
    }
    let Some(hole_source) = feature.source_value() else {
        return Ok(None);
    };
    let (lower, upper) = if hole_source < source {
        (hole_source, source)
    } else {
        (source, hole_source)
    };
    ctx.charge_work(u64_from_index(history.features.len()), OPERATION)?;
    let mut bounded = None;
    for candidate in &history.features {
        if !candidate
            .source_value()
            .is_some_and(|source| lower < source && source < upper)
            || classify(candidate) != Some(FeatureClass::Sketch)
            || !crate::history::project::solid::is_hole_profile_construction(ctx, candidate)?
        {
            continue;
        }
        if bounded.is_some() {
            return Ok(None);
        }
        bounded = Some(candidate);
    }
    if let Some(profile) = bounded {
        return Ok(Some((profile, 2)));
    }
    ctx.charge_work(u64_from_index(history.features.len()), OPERATION)?;
    let Some(position) = unique_position() else {
        return Ok(None);
    };
    let adjacent_ordinals = [
        position.ordinal.checked_sub(1),
        position.ordinal.checked_add(1),
    ];
    ctx.charge_work(u64_from_index(history.features.len()), OPERATION)?;
    let mut first = None;
    for candidate in &history.features {
        if !adjacent_ordinals.contains(&Some(candidate.ordinal))
            || ctx.equal(
                &(candidate.id),
                &(position.id),
                "compare SLDPRT holes records",
            )?
            || classify(candidate) != Some(FeatureClass::Sketch)
            || !crate::history::project::solid::is_hole_profile_construction(ctx, candidate)?
        {
            continue;
        }
        if first.is_some() {
            return Ok(None);
        }
        first = Some(candidate);
    }
    Ok(first.map(|profile| (profile, 1)))
}

fn hole_profile_from_child_order<'a>(
    ctx: &DecodeContext<'_>,
    feature: &crate::records::Feature,
    history: &'a crate::records::FeatureHistory,
) -> Result<Option<(&'a crate::records::Feature, u8)>, CodecError> {
    let (Some(first_ordinal), Some(second_ordinal)) = (
        feature.ordinal.checked_add(1),
        feature.ordinal.checked_add(2),
    ) else {
        return Ok(None);
    };
    let ordinals = [first_ordinal, second_ordinal];
    let unique_child = |ordinal| {
        let mut children = history
            .features
            .iter()
            .filter(|child| child.ordinal == ordinal);
        let child = children.next()?;
        children.next().is_none().then_some(child)
    };
    let (Some(first_child), Some(second_child)) =
        (unique_child(ordinals[0]), unique_child(ordinals[1]))
    else {
        return Ok(None);
    };
    let children = [first_child, second_child];
    if children
        .iter()
        .any(|child| classify(child) != Some(FeatureClass::Sketch))
    {
        return Ok(None);
    }
    let mut first = None;
    for child in children {
        if !crate::history::project::solid::is_hole_profile_construction(ctx, child)? {
            continue;
        }
        if first.is_some() {
            return Ok(None);
        }
        first = Some(child);
    }
    Ok(first.map(|profile| (profile, 1)))
}

pub(crate) fn enrich_history_cosmetic_thread_diameters(
    ctx: &DecodeContext<'_>,
    histories: &mut [crate::records::FeatureHistory],
    lanes: &[FeatureInputLane],
) -> Result<(), CodecError> {
    const OPERATION: &str = "enrich SLDPRT cosmetic thread diameters";
    for history in histories {
        ctx.charge_work(1, OPERATION)?;
        let mut features_by_id = HashMap::new();
        let mut features_by_source = BTreeMap::new();
        for feature in &history.features {
            ctx.charge_work(1, OPERATION)?;
            ctx.insert_hash_map(&mut features_by_id, feature.id.as_str(), feature, OPERATION)?;
            if let Some(source) = feature.source_id {
                ctx.insert_btree_map(&mut features_by_source, source, feature, OPERATION)?;
            }
        }
        let mut candidates_storage = ctx.reserve_scoped(0, OPERATION)?;
        let mut candidates = HashMap::<String, Option<f64>>::new();
        for lane in lanes {
            ctx.charge_work(1, OPERATION)?;
            for selection in &lane.surface_selections {
                ctx.charge_work(1, OPERATION)?;
                let Some(thread) = ctx.get_hash_map(
                    &features_by_id,
                    selection.feature_ref.as_str(),
                    "resolve SLDPRT holes keys",
                )?
                else {
                    continue;
                };
                if classify(thread) != Some(FeatureClass::CosmeticThread) {
                    continue;
                }
                let mut diameter = None;
                let mut ambiguous = false;
                for producer in selection
                    .producer_feature_refs
                    .iter()
                    .chain(selection.terminal_feature_ref.iter())
                {
                    ctx.charge_work(1, OPERATION)?;
                    let Some(producer) = ctx
                        .get_hash_map(
                            &features_by_id,
                            producer.as_str(),
                            "resolve SLDPRT holes keys",
                        )?
                        .copied()
                    else {
                        continue;
                    };
                    let Some(value) = crate::history::project::solid::threaded_hole_major_diameter(
                        ctx,
                        producer,
                        &features_by_source,
                        &history.features,
                    )?
                    else {
                        continue;
                    };
                    if let Some(existing) = diameter {
                        if f64::to_bits(existing) != value.to_bits() {
                            ambiguous = true;
                            break;
                        }
                    } else {
                        diameter = Some(value);
                    }
                }
                if ambiguous {
                    continue;
                }
                let Some(diameter) = diameter else {
                    continue;
                };
                if let Some(candidate) =
                    ctx.get_mut_hash_map(&mut candidates, &thread.id, "resolve SLDPRT holes keys")?
                {
                    if candidate.is_some_and(|value| value.to_bits() != diameter.to_bits()) {
                        *candidate = None;
                    }
                } else {
                    candidates_storage.with_storage(|| {
                        ctx.charge_work(u64_from_index(thread.id.len()), OPERATION)?;
                        let identity =
                            ctx.format_retained(format_args!("{}", thread.id), OPERATION)?;
                        ctx.insert_hash_map(
                            &mut candidates,
                            identity,
                            Some(diameter),
                            "resolve SLDPRT holes keys",
                        )
                    })?;
                }
            }
        }
        for feature in &mut history.features {
            ctx.charge_work(1, OPERATION)?;
            if feature.parameters.contains_key("D2") {
                continue;
            }
            let Some(Some(diameter)) =
                ctx.get_hash_map(&candidates, &feature.id, "resolve SLDPRT holes keys")?
            else {
                continue;
            };
            let Some(diameter) = cadmpeg_ir::scalar::Length::new(*diameter) else {
                continue;
            };
            let value = ctx.format_retained(
                format_args!(
                    "<MOD-DIAM>{}",
                    crate::history::literals::LengthLiteral(diameter)
                ),
                OPERATION,
            )?;
            ctx.insert_btree_map(
                &mut feature.parameters,
                cadmpeg_core::nonblank_literal!("D2"),
                value,
                OPERATION,
            )?;
        }
    }
    Ok(())
}

pub(crate) fn enrich_history_cosmetic_thread_diameters_without_hole_constructions(
    ctx: &DecodeContext<'_>,
    histories: &mut [crate::records::FeatureHistory],
    lanes: &[FeatureInputLane],
) -> Result<(), CodecError> {
    const OPERATION: &str = "copy SLDPRT fallback thread diameters";
    let mut projection = crate::records::charged_clone::clone_histories_charged(
        ctx,
        histories,
        "clone SLDPRT cosmetic thread histories",
    )?;
    enrich_history_hole_constructions(ctx, &mut projection, lanes)?;
    enrich_history_cosmetic_thread_diameters(ctx, &mut projection, lanes)?;
    let mut fallback_parameters = HashMap::new();
    for iteration_history in ctx.admit_iter(&projection[..], "scan SLDPRT holes source records")? {
        for feature in ctx.admit_iter(&iteration_history.features, "scan SLDPRT holes records")? {
            let Some(diameter) = feature.parameters.get("D2") else {
                continue;
            };
            ctx.insert_hash_map(
                &mut fallback_parameters,
                feature.id.as_str(),
                diameter,
                OPERATION,
            )?;
        }
    }
    for history_index in ctx.admit_iter(&(0..histories.len()), OPERATION)? {
        let history = &mut histories[history_index];
        for feature_index in ctx.admit_iter(&(0..history.features.len()), OPERATION)? {
            let feature = &mut history.features[feature_index];
            if feature.parameters.contains_key("D2") {
                continue;
            }
            let Some(diameter) = ctx.get_hash_map(
                &fallback_parameters,
                feature.id.as_str(),
                "resolve SLDPRT holes keys",
            )?
            else {
                continue;
            };
            ctx.charge_work(u64_from_index(diameter.len()), OPERATION)?;
            let value = ctx.format_retained(format_args!("{diameter}"), OPERATION)?;
            ctx.insert_btree_map(
                &mut feature.parameters,
                cadmpeg_core::nonblank_literal!("D2"),
                value,
                OPERATION,
            )?;
        }
    }
    Ok(())
}

#[derive(Clone)]
struct ProfiledHoleConstruction {
    diameter: cadmpeg_ir::scalar::PositiveLength,
    extent: LinearTermination,
    kind: HoleKind,
    bottom: Option<HoleBottom>,
    taper_angle: Option<cadmpeg_ir::scalar::InteriorAngle>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ProfileEvidence {
    Dimensions,
    AxialTopology,
}

const DISPLAY_DIMENSION_TOLERANCE_MM: f64 = 1.0e-5;
const GENERATED_PROFILE_TERMINAL_OVERRUN_MM: [f64; 4] = [0.0, 0.000_025, 0.000_05, 0.001];

fn profiled_hole_construction(
    ctx: &DecodeContext<'_>,
    profile: &crate::records::Feature,
    sketch: &SketchId,
    entities: &[SketchEntity],
) -> Result<Option<ProfiledHoleConstruction>, CodecError> {
    profiled_hole_construction_with_evidence(
        ctx,
        profile,
        sketch,
        entities,
        ProfileEvidence::Dimensions,
    )
}

#[derive(Clone, Copy)]
struct DimensionOnlyHole {
    diameter: cadmpeg_ir::scalar::PositiveLength,
    depth: cadmpeg_ir::scalar::NonZeroLength,
    drill_point_angle: Option<cadmpeg_ir::scalar::InteriorAngle>,
}

impl DimensionOnlyHole {
    fn into_construction(self) -> ProfiledHoleConstruction {
        let (kind, bottom) = match self.drill_point_angle {
            Some(angle) => (
                HoleKind::SimpleDrilled {
                    drill_point_angle: angle,
                },
                HoleBottom::Angled {
                    included_angle: angle,
                    depth_to_tip: false,
                },
            ),
            None => (HoleKind::Simple, HoleBottom::Flat),
        };
        ProfiledHoleConstruction {
            diameter: self.diameter,
            extent: LinearTermination::Blind { length: self.depth },
            kind,
            bottom: Some(bottom),
            taper_angle: None,
        }
    }
}

fn profiled_hole_construction_with_evidence(
    ctx: &DecodeContext<'_>,
    profile: &crate::records::Feature,
    sketch: &SketchId,
    entities: &[SketchEntity],
    evidence: ProfileEvidence,
) -> Result<Option<ProfiledHoleConstruction>, CodecError> {
    const OPERATION: &str = "resolve SLDPRT axial hole profile";
    let has_source_dimensions = ctx
        .admit_iter(&(profile.content)[..], "scan SLDPRT holes records")?
        .any(|content| matches!(content, crate::records::FeatureContent::Dimension(_)));
    let mut diameters = Vec::new();
    let mut angles = Vec::new();
    let mut lengths = Vec::new();
    let mut flat_bottom = false;
    let mut add_expression = |value: &str| -> Result<(), CodecError> {
        ctx.charge_work(
            u64_from_index(value.len())
                .checked_mul(8)
                .ok_or_else(|| ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX))?,
            OPERATION,
        )?;
        if let Some(value) = crate::history::literals::strip_diameter_modifier(value)
            .and_then(crate::history::literals::parse_dimension_length_mm)
            .and_then(|value| cadmpeg_ir::scalar::PositiveLength::try_from(value).ok())
        {
            ctx.reserve_vec(&mut diameters, 1, OPERATION)?;
            diameters.push(value);
        }
        if let Some(angle) = crate::history::literals::parse_bounded_angle_rad(value) {
            ctx.reserve_vec(&mut angles, 1, OPERATION)?;
            angles.push(angle);
        }
        flat_bottom |= crate::history::literals::parse_angle_rad(value).is_some_and(|angle| {
            (angle.get() - std::f64::consts::PI).abs() <= EPS_HOLE_EXACT_GEOMETRY
        });
        if crate::history::literals::strip_diameter_modifier(value).is_none()
            && crate::history::literals::parse_bounded_angle_rad(value).is_none()
        {
            if let Some(length) = crate::history::literals::parse_dimension_length_mm(value)
                .and_then(|value| cadmpeg_ir::scalar::PositiveLength::try_from(value).ok())
            {
                ctx.reserve_vec(&mut lengths, 1, OPERATION)?;
                lengths.push(length);
            }
        }
        Ok(())
    };
    if has_source_dimensions {
        for content in &profile.content {
            ctx.charge_work(1, OPERATION)?;
            let crate::records::FeatureContent::Dimension(name) = content else {
                continue;
            };
            if let Some(value) = ctx.get_btree_map(&profile.parameters, name.as_str(), OPERATION)? {
                add_expression(value)?;
            }
        }
    } else {
        for value in ctx
            .admit_iter(&profile.parameters, "scan SLDPRT holes records")?
            .map(|(_, value)| value)
        {
            add_expression(value)?;
        }
    }
    ctx.sort_unstable_by_key(
        &mut diameters,
        |value| value.get(),
        f64::total_cmp,
        OPERATION,
    )?;
    ctx.charge_work(u64_from_index(diameters.len()), OPERATION)?;
    diameters.dedup_by(|left, right| (left.get() - right.get()).abs() <= EPS_HOLE_GEOMETRY);
    ctx.sort_unstable_by_key(&mut angles, |value| value.get(), f64::total_cmp, OPERATION)?;
    ctx.charge_work(u64_from_index(angles.len()), OPERATION)?;
    angles.dedup_by(|left, right| (left.get() - right.get()).abs() <= EPS_HOLE_EXACT_GEOMETRY);
    ctx.sort_unstable_by_key(&mut lengths, |value| value.get(), f64::total_cmp, OPERATION)?;
    ctx.charge_work(u64_from_index(lengths.len()), OPERATION)?;
    lengths.dedup_by(|left, right| (left.get() - right.get()).abs() <= EPS_HOLE_GEOMETRY);
    let dimension_only =
        if crate::history::project::solid::is_hole_profile_construction(ctx, profile)? {
            match (diameters.as_slice(), lengths.as_slice(), angles.as_slice()) {
                ([diameter], [depth], []) => Some(DimensionOnlyHole {
                    diameter: *diameter,
                    depth: cadmpeg_ir::scalar::NonZeroLength::from(*depth),
                    drill_point_angle: None,
                }),
                ([diameter], [depth], [drill_point_angle]) => Some(DimensionOnlyHole {
                    diameter: *diameter,
                    depth: cadmpeg_ir::scalar::NonZeroLength::from(*depth),
                    drill_point_angle: Some(*drill_point_angle),
                }),
                _ => None,
            }
        } else {
            None
        };
    if evidence == ProfileEvidence::Dimensions {
        if let Some(construction) = dimension_only {
            return Ok(Some(construction.into_construction()));
        }
    }
    let mut lines = Vec::new();
    let mut points = Vec::new();
    for entity in entities {
        ctx.charge_work(1, OPERATION)?;
        if !ctx.equal(&(entity.sketch), sketch, "compare SLDPRT holes records")?
            || entity.construction
        {
            continue;
        }
        match *entity.geometry.definition() {
            SketchGeometryDefinition::Line { start, end } => {
                ctx.reserve_vec(&mut lines, 1, OPERATION)?;
                lines.push((start, end));
            }
            SketchGeometryDefinition::Point { position } => {
                ctx.reserve_vec(&mut points, 1, OPERATION)?;
                points.push(position);
            }
            _ => {}
        }
    }
    let same_point = |left: Point2, right: Point2| {
        (left.u - right.u).abs() <= DISPLAY_DIMENSION_TOLERANCE_MM
            && (left.v - right.v).abs() <= DISPLAY_DIMENSION_TOLERANCE_MM
    };
    let has_line = |first: Point2, second: Point2| -> Result<bool, CodecError> {
        Ok(ctx.admit_iter(&lines, OPERATION)?.any(|(start, end)| {
            (same_point(start.get(), first) && same_point(end.get(), second))
                || (same_point(start.get(), second) && same_point(end.get(), first))
        }))
    };
    let has_point_pair = |first: Point2, second: Point2| -> Result<bool, CodecError> {
        Ok(ctx
            .admit_iter(&points, OPERATION)?
            .any(|point| same_point(point.get(), first))
            && ctx
                .admit_iter(&points, OPERATION)?
                .any(|point| same_point(point.get(), second)))
    };
    let profile_translation = |edges: &[(Point2, Point2)],
                               minimum_lines: usize|
     -> Result<Option<Point2>, CodecError> {
        for actual in ctx
            .admit_iter(&lines, OPERATION)?
            .flat_map(|(first, second)| [*first, *second])
            .chain(ctx.admit_iter(&points, OPERATION)?.copied())
        {
            for expected in ctx
                .admit_iter(edges, OPERATION)?
                .flat_map(|(first, second)| [*first, *second])
            {
                let translation = Point2::new(actual.u - expected.u, actual.v - expected.v);
                let mut materialized_lines = 0usize;
                for (first, second) in ctx.admit_iter(edges, OPERATION)? {
                    let first = Point2::new(first.u + translation.u, first.v + translation.v);
                    let second = Point2::new(second.u + translation.u, second.v + translation.v);
                    if has_line(first, second)? {
                        materialized_lines =
                            materialized_lines.checked_add(1).ok_or_else(|| {
                                ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX)
                            })?;
                    }
                }
                if materialized_lines < minimum_lines {
                    continue;
                }
                let mut all_edges_materialized = true;
                for (first, second) in ctx.admit_iter(edges, OPERATION)? {
                    let first = Point2::new(first.u + translation.u, first.v + translation.v);
                    let second = Point2::new(second.u + translation.u, second.v + translation.v);
                    if !has_line(first, second)? && !has_point_pair(first, second)? {
                        all_edges_materialized = false;
                        break;
                    }
                }
                if all_edges_materialized {
                    return Ok(Some(translation));
                }
            }
        }
        Ok(None)
    };
    if let Some(construction) = dimension_only {
        let length = construction.depth;
        let radius = construction.diameter.get() / 2.0;
        for swap in [false, true] {
            for axial_sign in [-1.0, 1.0] {
                for radial_sign in [-1.0, 1.0] {
                    let point = |axial: f64, radial: f64| {
                        let axial = axial * axial_sign;
                        let radial = radial * radial_sign;
                        if swap {
                            Point2::new(radial, axial)
                        } else {
                            Point2::new(axial, radial)
                        }
                    };
                    let axis_entry = point(0.0, 0.0);
                    let wall_entry = point(0.0, radius);
                    let wall_end = point(-length.get(), radius);
                    let axis_end = point(-length.get(), 0.0);
                    let edges = [
                        (axis_entry, wall_entry),
                        (wall_entry, wall_end),
                        (wall_end, axis_end),
                        (axis_end, axis_entry),
                    ];
                    if profile_translation(&edges, 2)?.is_some() {
                        return Ok(Some(construction.into_construction()));
                    }
                }
            }
        }
        return Ok(None);
    }
    if let ([diameter, recess_diameter, entry_diameter], [recess_depth, depth], [entry_angle]) =
        (diameters.as_slice(), lengths.as_slice(), angles.as_slice())
    {
        let bore_diameter = *diameter;
        let admitted_recess_diameter = *recess_diameter;
        let admitted_entry_diameter = *entry_diameter;
        let admitted_recess_depth = *recess_depth;
        let admitted_entry_angle = *entry_angle;
        let diameter = diameter.get();
        let recess_diameter = recess_diameter.get();
        let entry_diameter = entry_diameter.get();
        let recess_depth = recess_depth.get();
        let depth = depth.get();
        let entry_angle = entry_angle.get();
        let bore_radius = diameter / 2.0;
        let recess_radius = recess_diameter / 2.0;
        let entry_radius = entry_diameter / 2.0;
        let setback = (entry_radius - recess_radius) / (entry_angle / 2.0).tan();
        if !setback.is_finite()
            || setback <= 0.0
            || recess_depth <= setback
            || depth <= recess_depth
        {
            return Ok(None);
        }
        for swap in [false, true] {
            for axial_sign in [-1.0, 1.0] {
                for radial_sign in [-1.0, 1.0] {
                    let point = |axial: f64, radial: f64| {
                        let axial = axial * axial_sign;
                        let radial = radial * radial_sign;
                        if swap {
                            Point2::new(radial, axial)
                        } else {
                            Point2::new(axial, radial)
                        }
                    };
                    let entry = point(0.0, entry_radius);
                    let recess_start = point(-setback, recess_radius);
                    let recess_end = point(-recess_depth, recess_radius);
                    let bore_start = point(-recess_depth, bore_radius);
                    for terminal_overrun in GENERATED_PROFILE_TERMINAL_OVERRUN_MM {
                        let bore_end = point(-depth - terminal_overrun, bore_radius);
                        let edges = [
                            (entry, recess_start),
                            (recess_start, recess_end),
                            (recess_end, bore_start),
                            (bore_start, bore_end),
                        ];
                        if profile_translation(&edges, 2)?.is_some() {
                            let Ok(diameters) =
                                cadmpeg_ir::features::holes::CounterdrillDiameters::new(
                                    admitted_recess_diameter,
                                    Some(admitted_entry_diameter),
                                )
                            else {
                                return Ok(None);
                            };
                            return Ok(Some(ProfiledHoleConstruction {
                                diameter: bore_diameter,
                                extent: LinearTermination::ThroughAll {},
                                kind: HoleKind::Counterdrill {
                                    diameters,
                                    depth: admitted_recess_depth,
                                    angle: admitted_entry_angle,
                                },
                                bottom: None,
                                taper_angle: None,
                            }));
                        }
                    }
                }
            }
        }
        return Ok(None);
    }
    let [diameter, entry_diameter] = diameters.as_slice() else {
        return Ok(None);
    };
    if diameter >= entry_diameter {
        return Ok(None);
    }
    let bore_radius = diameter.get() / 2.0;
    let entry_radius = entry_diameter.get() / 2.0;
    for swap in [false, true] {
        for axial_sign in [-1.0, 1.0] {
            for radial_sign in [-1.0, 1.0] {
                let point = |axial: f64, radial: f64| {
                    let axial = axial * axial_sign;
                    let radial = radial * radial_sign;
                    if swap {
                        Point2::new(radial, axial)
                    } else {
                        Point2::new(axial, radial)
                    }
                };
                if let ([depth], []) = (lengths.as_slice(), angles.as_slice()) {
                    for (entry_radius, terminal_radius) in
                        [(bore_radius, entry_radius), (entry_radius, bore_radius)]
                    {
                        let axis_entry = point(0.0, 0.0);
                        let wall_entry = point(0.0, entry_radius);
                        let wall_end = point(-depth.get(), terminal_radius);
                        let axis_end = point(-depth.get(), 0.0);
                        let edges = [
                            (axis_entry, wall_entry),
                            (wall_entry, wall_end),
                            (wall_end, axis_end),
                            (axis_end, axis_entry),
                        ];
                        let mut materialized_edges = 0usize;
                        for (first, second) in ctx.admit_iter(&edges, OPERATION)? {
                            if has_line(*first, *second)? {
                                materialized_edges =
                                    materialized_edges.checked_add(1).ok_or_else(|| {
                                        ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX)
                                    })?;
                            }
                        }
                        if materialized_edges < 2 {
                            continue;
                        }
                        let mut all_edges_materialized = true;
                        for (first, second) in ctx.admit_iter(&edges, OPERATION)? {
                            if !has_line(*first, *second)? && !has_point_pair(*first, *second)? {
                                all_edges_materialized = false;
                                break;
                            }
                        }
                        if !all_edges_materialized {
                            continue;
                        }
                        let half_angle =
                            ((terminal_radius - entry_radius).abs() / depth.get()).atan();
                        if !half_angle.is_finite() || half_angle <= 0.0 {
                            continue;
                        }
                        let Some(diameter) =
                            cadmpeg_ir::scalar::PositiveLength::new(entry_radius * 2.0)
                        else {
                            return Ok(None);
                        };
                        let Some(taper_angle) =
                            cadmpeg_ir::scalar::InteriorAngle::new(half_angle * 2.0)
                        else {
                            return Ok(None);
                        };
                        return Ok(Some(ProfiledHoleConstruction {
                            diameter,
                            extent: LinearTermination::Blind {
                                length: cadmpeg_ir::scalar::NonZeroLength::from(*depth),
                            },
                            kind: HoleKind::Simple,
                            bottom: Some(HoleBottom::Flat),
                            taper_angle: Some(taper_angle),
                        }));
                    }
                }
                if let [entry_depth, depth] = lengths.as_slice() {
                    let entry = point(0.0, entry_radius);
                    let entry_corner = point(-entry_depth.get(), entry_radius);
                    let bore_corner = point(-entry_depth.get(), bore_radius);
                    let terminal_overruns = if angles.is_empty() && !flat_bottom {
                        &GENERATED_PROFILE_TERMINAL_OVERRUN_MM[..]
                    } else {
                        &GENERATED_PROFILE_TERMINAL_OVERRUN_MM[..1]
                    };
                    for terminal_overrun in
                        ctx.admit_iter(terminal_overruns, "scan SLDPRT hole profile overruns")?
                    {
                        let bore_end = point(-depth.get() - terminal_overrun, bore_radius);
                        let edges = [
                            (entry, entry_corner),
                            (entry_corner, bore_corner),
                            (bore_corner, bore_end),
                        ];
                        let Some(translation) = profile_translation(&edges, 2)? else {
                            continue;
                        };
                        let (kind, extent) = match angles.as_slice() {
                            [] => {
                                let extent = if flat_bottom {
                                    LinearTermination::Blind {
                                        length: cadmpeg_ir::scalar::NonZeroLength::from(*depth),
                                    }
                                } else {
                                    LinearTermination::ThroughAll {}
                                };
                                (
                                    HoleKind::Counterbore {
                                        diameter: *entry_diameter,
                                        depth: *entry_depth,
                                    },
                                    extent,
                                )
                            }
                            [drill_point_angle] => {
                                let drill_length =
                                    bore_radius / (drill_point_angle.get() / 2.0).tan();
                                let translated = |point: Point2| {
                                    Point2::new(point.u + translation.u, point.v + translation.v)
                                };
                                if !drill_length.is_finite()
                                    || !has_line(
                                        translated(bore_end),
                                        translated(point(-depth.get() - drill_length, 0.0)),
                                    )?
                                {
                                    continue;
                                }
                                (
                                    HoleKind::CounterboreDrilled {
                                        diameter: *entry_diameter,
                                        depth: *entry_depth,
                                        drill_point_angle: *drill_point_angle,
                                    },
                                    LinearTermination::Blind {
                                        length: cadmpeg_ir::scalar::NonZeroLength::from(*depth),
                                    },
                                )
                            }
                            _ => continue,
                        };
                        return Ok(Some(ProfiledHoleConstruction {
                            diameter: *diameter,
                            extent,
                            kind,
                            bottom: (angles.is_empty() && flat_bottom).then_some(HoleBottom::Flat),
                            taper_angle: None,
                        }));
                    }
                }
                let [depth] = lengths.as_slice() else {
                    continue;
                };
                if let [sink_angle] = angles.as_slice() {
                    let setback = (entry_radius - bore_radius) / (sink_angle.get() / 2.0).tan();
                    if !setback.is_finite() {
                        continue;
                    }
                    let entry = point(0.0, entry_radius);
                    let bore_start = point(-setback, bore_radius);
                    let mirrored_bore_start = point(-setback, -bore_radius);
                    let mut profile_matches = false;
                    'overruns: for overrun in GENERATED_PROFILE_TERMINAL_OVERRUN_MM {
                        for (wall_start, wall_radius) in [
                            (bore_start, bore_radius),
                            (mirrored_bore_start, -bore_radius),
                        ] {
                            let edges = [
                                (entry, bore_start),
                                (wall_start, point(-depth.get() - overrun, wall_radius)),
                            ];
                            if profile_translation(&edges, 2)?.is_some() {
                                profile_matches = true;
                                break 'overruns;
                            }
                        }
                    }
                    if profile_matches {
                        return Ok(Some(ProfiledHoleConstruction {
                            diameter: *diameter,
                            extent: LinearTermination::ThroughAll {},
                            kind: HoleKind::Countersink {
                                diameter: *entry_diameter,
                                angle: *sink_angle,
                            },
                            bottom: None,
                            taper_angle: None,
                        }));
                    }
                    continue;
                }
                let [first_angle, second_angle] = angles.as_slice() else {
                    continue;
                };
                for (sink_angle, drill_point_angle) in
                    [(*first_angle, *second_angle), (*second_angle, *first_angle)]
                {
                    let setback = (entry_radius - bore_radius) / (sink_angle.get() / 2.0).tan();
                    let drill_length = bore_radius / (drill_point_angle.get() / 2.0).tan();
                    if !setback.is_finite() || !drill_length.is_finite() {
                        continue;
                    }
                    let entry = point(0.0, entry_radius);
                    let bore_start = point(-setback, bore_radius);
                    let bore_end = point(-depth.get(), bore_radius);
                    let tip = point(-depth.get() - drill_length, 0.0);
                    let edges = [(entry, bore_start), (bore_start, bore_end), (bore_end, tip)];
                    if profile_translation(&edges, 2)?.is_some() {
                        return Ok(Some(ProfiledHoleConstruction {
                            diameter: *diameter,
                            extent: LinearTermination::Blind {
                                length: cadmpeg_ir::scalar::NonZeroLength::from(*depth),
                            },
                            kind: HoleKind::Countersink {
                                diameter: *entry_diameter,
                                angle: sink_angle,
                            },
                            bottom: Some(HoleBottom::Angled {
                                included_angle: drill_point_angle,
                                depth_to_tip: false,
                            }),
                            taper_angle: None,
                        }));
                    }
                }
            }
        }
    }
    Ok(None)
}

pub(crate) fn project_profiled_hole_constructions(
    ctx: &DecodeContext<'_>,
    features: &mut [cadmpeg_ir::features::Feature],
    entities: &[SketchEntity],
    histories: &[crate::records::FeatureHistory],
    lanes: &[FeatureInputLane],
) -> Result<(), CodecError> {
    const OPERATION: &str = "project SLDPRT profiled hole constructions";
    let mut enriched_histories = crate::records::charged_clone::clone_histories_charged(
        ctx,
        histories,
        "SLDPRT unowned incomplete-hole histories",
    )?;
    crate::history::configuration::enrich_history_parameters_semantic(
        ctx,
        &mut enriched_histories,
        lanes,
    )?;
    let mut ownership_histories = crate::records::charged_clone::clone_histories_charged(
        ctx,
        &enriched_histories,
        "SLDPRT unowned incomplete-hole histories",
    )?;
    enrich_history_hole_constructions(ctx, &mut ownership_histories, lanes)?;
    let histories = enriched_histories.as_slice();
    let incomplete =
        |diameter: &Option<cadmpeg_ir::scalar::PositiveLength>,
         extent: &Option<LinearTermination>,
         construction: &cadmpeg_ir::features::holes::HoleConstruction| {
            diameter.is_none()
                || extent
                    .as_ref()
                    .is_none_or(|extent| matches!(extent, LinearTermination::Unresolved {}))
                || matches!(construction, cadmpeg_ir::features::holes::HoleConstruction::Form { kind, .. } if kind.is_unresolved())
        };
    let mut model_sketch_storage = ctx.reserve_scoped(0, "index SLDPRT profiled hole sketches")?;
    let mut complete_native_holes = HashSet::new();
    let mut model_sketches = HashMap::new();
    for feature in features.iter() {
        ctx.charge_work(1, OPERATION)?;
        let Some(native) = feature.native_ref.as_deref() else {
            continue;
        };
        match feature.evaluation.definition() {
            FeatureDefinition::Operation(FeatureOperation::Hole { shape, extent, .. })
                if !incomplete(&shape.diameter(), extent, shape.construction()) =>
            {
                ctx.insert_hash_set(&mut complete_native_holes, native, OPERATION)?;
            }
            FeatureDefinition::Operation(FeatureOperation::Sketch {
                sketch: cadmpeg_ir::features::SketchFeatureBinding::Planar(Some(sketch)),
            }) if !ctx.contains_key_hash_map(
                &model_sketches,
                native,
                "resolve SLDPRT holes keys",
            )? =>
            {
                ctx.charge_work(
                    u64_from_index(native.len())
                        .checked_add(u64_from_index(sketch.as_str().len()))
                        .ok_or_else(|| ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX))?,
                    OPERATION,
                )?;
                model_sketch_storage.with_storage(|| {
                    let native = ctx.format_retained(format_args!("{native}"), OPERATION)?;
                    let identity_text = ctx.format_retained(format_args!("{sketch}"), OPERATION)?;
                    ctx.charge_work(
                        cadmpeg_core::decode::u64_from_index(identity_text.len()),
                        "validate SLDPRT holes identity",
                    )?;
                    let sketch = SketchId::mint(identity_text).map_err(|_| {
                        CodecError::malformed("invalid admitted SLDPRT sketch identity")
                    })?;
                    ctx.insert_hash_map(
                        &mut model_sketches,
                        native,
                        sketch,
                        "resolve SLDPRT holes keys",
                    )
                })?;
            }
            _ => {}
        }
    }
    let mut native_histories = HashMap::new();
    for (history_index, history) in ctx
        .admit_iter(histories, "scan SLDPRT holes records")?
        .enumerate()
    {
        for feature in &history.features {
            ctx.charge_work(1, OPERATION)?;
            ctx.insert_hash_map(
                &mut native_histories,
                feature.id.as_str(),
                history_index,
                OPERATION,
            )?;
        }
    }
    let mut unowned_incomplete_holes = ctx.collect_indexed_vec(
        histories.len(),
        "SLDPRT unowned incomplete-hole histories",
        |_| Ok(Vec::<(&str, u32)>::new()),
    )?;
    for feature in features.iter() {
        ctx.charge_work(1, OPERATION)?;
        let FeatureDefinition::Operation(FeatureOperation::Hole { shape, extent, .. }) =
            feature.evaluation.definition()
        else {
            continue;
        };
        if !incomplete(&shape.diameter(), extent, shape.construction()) {
            continue;
        }
        let Some(native) = feature.native_ref.as_deref() else {
            continue;
        };
        let Some(&history_index) =
            ctx.get_hash_map(&native_histories, native, "resolve SLDPRT holes keys")?
        else {
            continue;
        };
        let Some(native_feature) = ({
            let mut search_result = None;
            for candidate in ctx.admit_iter(
                &(histories[history_index].features)[..],
                "scan SLDPRT holes records",
            )? {
                if ctx.equal(
                    candidate.id.as_str(),
                    native,
                    "compare SLDPRT holes records",
                )? {
                    search_result = Some(candidate);
                    break;
                }
            }
            search_result
        }) else {
            continue;
        };
        if !native_feature
            .properties
            .contains_key("DissectableChildren")
        {
            ctx.reserve_vec(
                &mut unowned_incomplete_holes[history_index],
                1,
                "SLDPRT unowned incomplete holes",
            )?;
            unowned_incomplete_holes[history_index]
                .push((native_feature.id.as_str(), native_feature.ordinal));
        }
    }
    let mut fallback_constructions = HashMap::new();
    for ((history, ownership_history), hole_index) in ctx
        .admit_iter(histories, "scan SLDPRT holes records")?
        .zip(ctx.admit_iter(&ownership_histories, "scan SLDPRT holes records")?)
        .zip(ctx.admit_iter(
            &(0..unowned_incomplete_holes.len()),
            "scan SLDPRT holes records",
        )?)
    {
        let holes = &mut unowned_incomplete_holes[hole_index];
        let mut claimed_profiles = HashSet::new();
        for feature in &history.features {
            ctx.charge_work(1, OPERATION)?;
            if let Some(children) = feature.properties.get("DissectableChildren") {
                collect_claimed_hole_profiles(ctx, history, children, &mut claimed_profiles)?;
            }
        }
        for feature in &ownership_history.features {
            ctx.charge_work(1, OPERATION)?;
            if ctx.contains_hash_set(
                &complete_native_holes,
                feature.id.as_str(),
                "resolve SLDPRT holes keys",
            )? {
                if let Some(children) = feature.properties.get("DissectableChildren") {
                    collect_claimed_hole_profiles(ctx, history, children, &mut claimed_profiles)?;
                }
            }
        }
        let mut profiles = Vec::new();
        for (index, profile) in history.features.iter().enumerate() {
            ctx.charge_work(1, OPERATION)?;
            if ctx.contains_hash_set(
                &claimed_profiles,
                profile.id.as_str(),
                "resolve SLDPRT holes keys",
            )? {
                continue;
            }
            let Some(sketch) = ctx.get_hash_map(
                &model_sketches,
                profile.id.as_str(),
                "resolve SLDPRT holes keys",
            )?
            else {
                continue;
            };
            let Some(construction) = profiled_hole_construction_with_evidence(
                ctx,
                profile,
                sketch,
                entities,
                ProfileEvidence::AxialTopology,
            )?
            else {
                continue;
            };
            ctx.reserve_vec(&mut profiles, 1, OPERATION)?;
            profiles.push((profile.ordinal, index, construction));
        }
        ctx.sort_unstable_by_key(
            holes,
            |value| (value.1, value.0),
            |left, right| left.0.cmp(&right.0).then_with(|| left.1.cmp(right.1)),
            OPERATION,
        )?;
        // The input index preserves history order for equal ordinals.
        ctx.sort_unstable_by_key(
            &mut profiles,
            |value| {
                let (left_ordinal, left_index, _) = value;
                (*left_ordinal, *left_index)
            },
            Ord::cmp,
            OPERATION,
        )?;
        if holes.len() != profiles.len() {
            continue;
        }
        for ((hole, _), (_, _, construction)) in holes.iter().zip(profiles) {
            ctx.charge_work(1, OPERATION)?;
            ctx.insert_hash_map(&mut fallback_constructions, *hole, construction, OPERATION)?;
        }
    }
    drop(complete_native_holes);
    for feature in features.iter_mut() {
        ctx.charge_work(1, OPERATION)?;
        let FeatureDefinition::Operation(FeatureOperation::Hole { shape, extent, .. }) =
            feature.evaluation.definition()
        else {
            continue;
        };
        if !incomplete(&shape.diameter(), extent, shape.construction()) {
            continue;
        }
        let Some(native_id) = feature.native_ref.as_deref() else {
            continue;
        };
        let Some(&history_index) =
            ctx.get_hash_map(&native_histories, native_id, "resolve SLDPRT holes keys")?
        else {
            continue;
        };
        let history = &histories[history_index];
        let Some(native) = ({
            let mut search_result = None;
            for candidate in ctx.admit_iter(&(history.features)[..], "scan SLDPRT holes records")? {
                if ctx.equal(
                    candidate.id.as_str(),
                    native_id,
                    "compare SLDPRT holes records",
                )? {
                    search_result = Some(candidate);
                    break;
                }
            }
            search_result
        }) else {
            continue;
        };
        let position = hole_position_feature(ctx, native, histories, lanes)?
            .map(|feature| feature.id.as_str());
        let mut direct = None;
        if let Some(children) = native.properties.get("DissectableChildren") {
            for source in hole_child_tokens(ctx, children)? {
                let source = ctx.trim_text(source?, OPERATION)?;
                let Some(profile) = unique_hole_child_profile(ctx, history, source)? else {
                    continue;
                };
                if ctx.equal(
                    &(position),
                    &(Some(profile.id.as_str())),
                    "compare SLDPRT holes records",
                )? {
                    continue;
                }
                let Some(sketch) =
                    ctx.get_hash_map(&model_sketches, &profile.id, "resolve SLDPRT holes keys")?
                else {
                    continue;
                };
                if let Some(construction) =
                    profiled_hole_construction(ctx, profile, sketch, entities)?
                {
                    if direct.is_some() {
                        direct = None;
                        break;
                    }
                    direct = Some(construction);
                }
            }
        }
        let construction =
            if direct.is_some() || native.properties.contains_key("DissectableChildren") {
                direct
            } else {
                ctx.get_hash_map(
                    &fallback_constructions,
                    &native.id.as_str(),
                    "resolve SLDPRT holes keys",
                )?
                .map(|source| {
                    source
                        .extent
                        .try_clone_for_decode(ctx, "SLDPRT profiled hole extent copy")
                        .map(|extent| ProfiledHoleConstruction { extent, ..*source })
                })
                .transpose()?
            };
        let Some(construction) = construction else {
            continue;
        };
        let mut edit_result = Ok(());
        feature.evaluation.edit(|definition, _| {
            let FeatureDefinition::Operation(FeatureOperation::Hole {
                shape,
                extent,
                bottom,
                taper_angle,
                ..
            }) = definition
            else {
                return;
            };
            edit_result =
                shape.try_set_form_and_diameter(construction.kind, Some(construction.diameter));
            if edit_result.is_ok() {
                *extent = Some(construction.extent);
                *bottom = construction.bottom;
                *taper_angle = construction.taper_angle;
            }
        });
        edit_result.map_err(CodecError::malformed)?;
    }
    Ok(())
}

fn collect_claimed_hole_profiles<'a>(
    ctx: &DecodeContext<'_>,
    history: &'a crate::records::FeatureHistory,
    children: &str,
    profiles: &mut HashSet<&'a str>,
) -> Result<(), CodecError> {
    const OPERATION: &str = "index SLDPRT claimed axial hole profiles";
    for child in hole_child_tokens(ctx, children)? {
        let child = ctx.trim_text(child?, OPERATION)?;
        if child.is_empty() {
            continue;
        }
        let Some(profile) = unique_hole_child_profile(ctx, history, child)? else {
            continue;
        };
        if ctx.contains_hash_set(profiles, profile.id.as_str(), "resolve SLDPRT holes keys")? {
            continue;
        }
        ctx.insert_hash_set(profiles, profile.id.as_str(), OPERATION)?;
    }
    Ok(())
}

pub(crate) fn project_hole_position_sketches(
    ctx: &DecodeContext<'_>,
    features: &mut [cadmpeg_ir::features::Feature],
    sketches: &[Sketch],
    sketch_entities: &[SketchEntity],
    histories: &[crate::records::FeatureHistory],
    lanes: &[FeatureInputLane],
) -> Result<(), CodecError> {
    const OPERATION: &str = "project SLDPRT hole position sketches";
    const NATIVE_TO_IR: f64 = 1000.0;
    const QUANTUM: f64 = EPS_HOLE_POSITION;
    let mut native_features = BTreeMap::new();
    for iteration_history in ctx.admit_iter(histories, "scan SLDPRT holes source records")? {
        for feature in ctx.admit_iter(&iteration_history.features, "scan SLDPRT holes records")? {
            ctx.insert_btree_map(
                &mut native_features,
                feature.id.as_str(),
                feature,
                OPERATION,
            )?;
        }
    }
    let mut model_sketch_features_storage = ctx.reserve_scoped(0, "resolve SLDPRT holes keys")?;
    let mut model_sketch_features = HashMap::new();
    for feature in features.iter() {
        ctx.charge_work(1, OPERATION)?;
        let FeatureDefinition::Operation(FeatureOperation::Sketch {
            sketch: cadmpeg_ir::features::SketchFeatureBinding::Planar(Some(sketch)),
        }) = feature.evaluation.definition()
        else {
            continue;
        };
        let Some(native) = &feature.native_ref else {
            continue;
        };
        ctx.charge_work(
            u64_from_index(native.len())
                .checked_add(u64_from_index(sketch.as_str().len()))
                .and_then(|count| count.checked_add(u64_from_index(feature.id.as_str().len())))
                .ok_or_else(|| ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX))?,
            OPERATION,
        )?;
        model_sketch_features_storage.with_storage(|| {
            let native_key = ctx.format_retained(format_args!("{native}"), OPERATION)?;
            let feature_id = {
                let identity_text =
                    ctx.format_retained(format_args!("{}", feature.id), OPERATION)?;
                ctx.charge_work(
                    cadmpeg_core::decode::u64_from_index(identity_text.len()),
                    "validate SLDPRT holes identity",
                )?;
                cadmpeg_ir::features::FeatureId::mint(identity_text)
            }
            .map_err(|_| CodecError::malformed("invalid admitted SLDPRT feature identity"))?;
            let sketch_id = {
                let identity_text = ctx.format_retained(format_args!("{sketch}"), OPERATION)?;
                ctx.charge_work(
                    cadmpeg_core::decode::u64_from_index(identity_text.len()),
                    "validate SLDPRT holes identity",
                )?;
                SketchId::mint(identity_text)
            }
            .map_err(|_| CodecError::malformed("invalid admitted SLDPRT sketch identity"))?;
            ctx.insert_hash_map(
                &mut model_sketch_features,
                native_key,
                (feature_id, sketch_id),
                "resolve SLDPRT holes keys",
            )
        })?;
    }
    for feature in features.iter_mut() {
        ctx.charge_work(1, OPERATION)?;
        let mut projection = None;
        'feature_edit: {
            if feature.suppressed == Some(true) {
                break 'feature_edit;
            }
            let FeatureDefinition::Operation(FeatureOperation::Hole { placements, .. }) =
                feature.evaluation.definition()
            else {
                break 'feature_edit;
            };
            if placements.is_some() {
                break 'feature_edit;
            }
            let Some(native) = (match feature.native_ref.as_deref() {
                Some(native) => ctx
                    .get_btree_map(
                        &(native_features),
                        native,
                        "resolve SLDPRT holes references",
                    )?
                    .copied(),
                None => None,
            }) else {
                break 'feature_edit;
            };
            let position_feature = match hole_position_feature(ctx, native, histories, lanes)? {
                Some(position) => Some(position),
                None => direct_hole_position_feature(
                    ctx,
                    native,
                    histories,
                    |id| {
                        Ok(ctx
                            .get_hash_map(&model_sketch_features, id, OPERATION)?
                            .map(|(_, sketch)| sketch))
                    },
                    sketch_entities,
                )?,
            };
            let Some(position_feature) = position_feature else {
                break 'feature_edit;
            };
            let Some((position_dependency, sketch_id)) = ctx.get_hash_map(
                &model_sketch_features,
                position_feature.id.as_str(),
                "resolve SLDPRT holes keys",
            )?
            else {
                break 'feature_edit;
            };
            let Some(sketch) = ({
                let mut search_result = None;
                for sketch in ctx.admit_iter(sketches, "scan SLDPRT holes records")? {
                    if ctx.equal(&(sketch.id), sketch_id, "compare SLDPRT holes records")? {
                        search_result = Some(sketch);
                        break;
                    }
                }
                search_result
            }) else {
                break 'feature_edit;
            };
            let Some((origin, normal, u_axis)) = sketch.resolved_placement() else {
                break 'feature_edit;
            };
            let mut authored_markers = Vec::new();
            for lane in ctx.admit_iter(lanes, OPERATION)? {
                if !ctx.equal(&lane.configuration, &sketch.configuration, OPERATION)? {
                    continue;
                }
                for marker in ctx.admit_iter(&lane.sketch_entities, OPERATION)? {
                    if ctx.equal(
                        &marker.feature_ref.as_deref(),
                        &Some(position_feature.id.as_str()),
                        OPERATION,
                    )? && marker.object_index().is_some()
                        && marker.coordinates_m.is_some()
                        && matches!(
                            marker.kind(),
                            SketchInputKind::Point | SketchInputKind::ConstrainedPoint
                        )
                    {
                        ctx.reserve_vec(&mut authored_markers, 1, OPERATION)?;
                        authored_markers.push(marker);
                    }
                }
            }
            let mut unindexed_marker_storage = ctx.reserve_scoped(0, OPERATION)?;
            let mut unindexed_marker_coordinates = HashMap::new();
            let paired_marker_coordinates = if authored_markers.is_empty() {
                // Direct projection requires a complete alternate object roster.
                // An isolated pair among other coordinates can describe a
                // construction curve or dimension handle instead of a hole locus.
                let mut paired_marker_coordinates = HashMap::new();
                let mut complete_alternate_encoding = true;
                let mut unindexed_locus: Option<(&crate::records::SketchInputEntity, [f64; 2])> =
                    None;
                let mut complete_unindexed_encoding = true;
                for lane in ctx.admit_iter(lanes, OPERATION)? {
                    if !ctx.equal(&lane.configuration, &sketch.configuration, OPERATION)? {
                        continue;
                    }
                    let position_marker =
                        |marker: &crate::records::SketchInputEntity| -> Result<bool, CodecError> {
                            Ok(ctx.equal(
                                &marker.feature_ref.as_deref(),
                                &Some(position_feature.id.as_str()),
                                OPERATION,
                            )? && marker.coordinates_m.is_some())
                        };
                    let mut indexed_markers = 0usize;
                    for marker in ctx.admit_iter(&lane.sketch_entities, OPERATION)? {
                        if position_marker(marker)? && marker.object_index().is_some() {
                            indexed_markers = indexed_markers.checked_add(1).ok_or_else(|| {
                                ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX)
                            })?;
                        }
                    }
                    let mut paired_count = 0usize;
                    paired_object_locus_markers(
                        ctx,
                        lane,
                        position_feature.id.as_str(),
                        |_, _| {
                            paired_count = paired_count.checked_add(1).ok_or_else(|| {
                                ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX)
                            })?;
                            Ok(())
                        },
                    )?;
                    complete_alternate_encoding &= paired_count == indexed_markers;
                    paired_object_locus_markers(
                        ctx,
                        lane,
                        position_feature.id.as_str(),
                        |marker, coordinates| {
                            ctx.insert_hash_map(
                                &mut paired_marker_coordinates,
                                marker.id(),
                                coordinates,
                                OPERATION,
                            )?;
                            ctx.reserve_vec(&mut authored_markers, 1, OPERATION)?;
                            authored_markers.push(marker);
                            Ok(())
                        },
                    )?;
                    let mut points_only = indexed_markers == 0;
                    if points_only {
                        for marker in ctx.admit_iter(&lane.sketch_entities, OPERATION)? {
                            if position_marker(marker)?
                                && !matches!(
                                    marker.kind(),
                                    SketchInputKind::Point | SketchInputKind::ConstrainedPoint
                                )
                            {
                                points_only = false;
                                break;
                            }
                        }
                    }
                    if points_only {
                        let mut locus = None;
                        let mut multiple_loci = false;
                        for marker in ctx.admit_iter(&lane.sketch_entities, OPERATION)? {
                            if !position_marker(marker)? {
                                continue;
                            }
                            let Some([u, v]) = marker
                                .coordinates_m
                                .map(cadmpeg_ir::units::FiniteVector::get)
                            else {
                                continue;
                            };
                            if u == 0.0 && v == 0.0 {
                                continue;
                            }
                            if locus.is_some() {
                                multiple_loci = true;
                                break;
                            }
                            locus = Some((marker, [u, v]));
                        }
                        if let Some((locus, coordinates)) = locus {
                            if multiple_loci
                                || unindexed_locus
                                    .is_some_and(|(_, previous)| previous != coordinates)
                            {
                                complete_unindexed_encoding = false;
                            } else {
                                unindexed_locus = Some((locus, coordinates));
                            }
                        } else {
                            complete_unindexed_encoding = false;
                        }
                    } else {
                        complete_unindexed_encoding = false;
                    }
                }
                // Legacy position sketches omit object indexes from their point
                // records. Accept one locus only when every matching lane has a
                // point-only coordinate roster with exactly one non-origin point;
                // zero points are relation anchors and do not identify a hole.
                if complete_unindexed_encoding {
                    if let Some((marker, coordinates)) = unindexed_locus {
                        unindexed_marker_storage.with_storage(|| {
                            ctx.insert_hash_map(
                                &mut unindexed_marker_coordinates,
                                marker.id(),
                                coordinates,
                                "resolve SLDPRT holes keys",
                            )
                        })?;
                        ctx.reserve_vec(&mut authored_markers, 1, OPERATION)?;
                        authored_markers.push(marker);
                        HashMap::new()
                    } else if complete_alternate_encoding {
                        paired_marker_coordinates
                    } else {
                        authored_markers.clear();
                        HashMap::new()
                    }
                } else if complete_alternate_encoding {
                    paired_marker_coordinates
                } else {
                    authored_markers.clear();
                    HashMap::new()
                }
            } else {
                HashMap::new()
            };
            if authored_markers.is_empty() {
                break 'feature_edit;
            }
            let marker_transform = sketch_frame_marker_transform(sketch, QUANTUM);
            let v_axis = normal.cross(u_axis.get());
            let mut resolved = Vec::new();
            for marker in ctx.admit_iter(&(authored_markers)[..], "scan SLDPRT holes records")? {
                let mut entity = None;
                let mut multiple_positions = false;
                for candidate in ctx.admit_iter(sketch_entities, OPERATION)? {
                    let SketchGeometryDefinition::Point { position } =
                        *candidate.geometry.definition()
                    else {
                        continue;
                    };
                    if ctx.equal(&candidate.sketch, sketch_id, OPERATION)?
                        && ctx.equal(
                            &candidate.native_ref.as_deref(),
                            &Some(marker.id()),
                            OPERATION,
                        )?
                    {
                        if entity.is_some() {
                            multiple_positions = true;
                            break;
                        }
                        entity = Some(position.get());
                    }
                }
                if multiple_positions {
                    ctx.clear_vec(&mut resolved, OPERATION)?;
                    break;
                }
                let position = match entity {
                    Some(position) => position,
                    None => {
                        let Some(&[u, v]) = (match ctx.get_hash_map(
                            &paired_marker_coordinates,
                            marker.id(),
                            "resolve SLDPRT holes keys",
                        )? {
                            Some(value) => Some(value),
                            None => ctx.get_hash_map(
                                &(unindexed_marker_coordinates),
                                marker.id(),
                                "resolve SLDPRT holes references",
                            )?,
                        }) else {
                            ctx.clear_vec(&mut resolved, OPERATION)?;
                            break;
                        };
                        let Some(transform) = marker_transform else {
                            ctx.clear_vec(&mut resolved, OPERATION)?;
                            break;
                        };
                        let native =
                            quantize(Point2::new(u * NATIVE_TO_IR, v * NATIVE_TO_IR), QUANTUM);
                        let Some((u, v)) = transform.apply(native) else {
                            ctx.clear_vec(&mut resolved, OPERATION)?;
                            break;
                        };
                        let (Some(u), Some(v)) = (f64_from_i64(u), f64_from_i64(v)) else {
                            ctx.clear_vec(&mut resolved, OPERATION)?;
                            break;
                        };
                        Point2::new(u * QUANTUM, v * QUANTUM)
                    }
                };
                let (Some(origin), Some(axis)) = (
                    FinitePoint3::new(Point3::new(
                        origin.x + position.u * u_axis.x + position.v * v_axis.x,
                        origin.y + position.u * u_axis.y + position.v * v_axis.y,
                        origin.z + position.u * u_axis.z + position.v * v_axis.z,
                    )),
                    FeatureDirection3::new(normal.get()),
                ) else {
                    ctx.clear_vec(&mut resolved, OPERATION)?;
                    break;
                };
                ctx.push_vec(
                    &mut resolved,
                    HolePlacement::Axis { origin, axis },
                    OPERATION,
                )?;
            }
            if resolved.len() == authored_markers.len() {
                projection = Some((resolved, position_dependency));
            }
        }
        if let Some((resolved, dependency)) = projection {
            feature.evaluation.edit(|definition, _| {
                if let FeatureDefinition::Operation(FeatureOperation::Hole { placements, .. }) =
                    definition
                {
                    *placements = Some(resolved);
                }
            });
            ctx.charge_work(u64_from_index(feature.dependencies.len()), OPERATION)?;
            if !feature.dependencies.contains(dependency) {
                ctx.charge_work(u64_from_index(dependency.as_str().len()), OPERATION)?;
                let dependency = {
                    let identity_text =
                        ctx.format_retained(format_args!("{dependency}"), OPERATION)?;
                    ctx.charge_work(
                        cadmpeg_core::decode::u64_from_index(identity_text.len()),
                        "validate SLDPRT holes identity",
                    )?;
                    cadmpeg_ir::features::FeatureId::mint(identity_text)
                }
                .map_err(|_| CodecError::malformed("invalid admitted SLDPRT feature identity"))?;
                feature.dependencies.insert(ctx, dependency, OPERATION)?;
            }
        }
    }
    Ok(())
}

fn paired_object_locus_markers<'a>(
    ctx: &DecodeContext<'_>,
    lane: &'a FeatureInputLane,
    feature: &str,
    mut visit: impl FnMut(&'a crate::records::SketchInputEntity, [f64; 2]) -> Result<(), CodecError>,
) -> Result<(), CodecError> {
    const OPERATION: &str = "scan SLDPRT paired object loci";
    // Object-locus layouts emit an indexed coordinate handle followed by an
    // unindexed zero point. The adjacent anchor distinguishes object loci from
    // the dimension and display handles in the same feature object.
    let Some(window) = std::num::NonZeroUsize::new(2) else {
        return Err(CodecError::malformed("empty paired object locus window"));
    };
    for pair in ctx
        .admit_iter(&lane.sketch_entities, OPERATION)?
        .windows(window)
    {
        let object = &pair[0];
        let anchor = &pair[1];
        let Some(coordinates) = object
            .coordinates_m
            .map(cadmpeg_ir::units::FiniteVector::get)
        else {
            continue;
        };
        if ctx.equal(&object.feature_ref.as_deref(), &Some(feature), OPERATION)?
            && ctx.equal(&anchor.feature_ref.as_deref(), &Some(feature), OPERATION)?
            && object.object_index().is_some()
            && anchor.object_index().is_none()
            && anchor.kind() == SketchInputKind::Point
            && anchor
                .coordinates_m
                .is_some_and(|coordinates| coordinates == [0.0, 0.0])
        {
            visit(object, coordinates)?;
        }
    }
    Ok(())
}

fn hole_position_feature<'a>(
    ctx: &DecodeContext<'_>,
    hole: &crate::records::Feature,
    histories: &'a [crate::records::FeatureHistory],
    lanes: &[FeatureInputLane],
) -> Result<Option<&'a crate::records::Feature>, CodecError> {
    const OPERATION: &str = "resolve SLDPRT hole position source";
    let mut source = None;
    for lane in ctx.admit_iter(lanes, "scan SLDPRT holes records")? {
        ctx.charge_work(
            u64_from_index(lane.names.len())
                .checked_add(128)
                .ok_or_else(|| ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX))?,
            OPERATION,
        )?;
        for name in &lane.names {
            ctx.charge_work(u64_from_index(name.value.len()), OPERATION)?;
        }
        let Some(candidate) = hole_position_sketch_source(ctx, hole, lane)? else {
            continue;
        };
        if source.is_some_and(|source| source != candidate) {
            return Ok(None);
        }
        source = Some(candidate);
    }
    let Some(source) = source else {
        return Ok(None);
    };
    let mut position = None;
    for iteration_history in ctx.admit_iter(histories, "scan SLDPRT holes source records")? {
        for candidate in ctx.admit_iter(&iteration_history.features, "scan SLDPRT holes records")? {
            if classify(candidate) != Some(FeatureClass::Sketch) {
                continue;
            }
            let mut matches = false;
            for lane in ctx.admit_iter(lanes, "scan SLDPRT holes records")? {
                ctx.charge_work(
                    u64_from_index(lane.names.len())
                        .checked_add(1)
                        .ok_or_else(|| ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX))?,
                    OPERATION,
                )?;
                if candidate.source_value().or_else(|| {
                    feature_object_name(candidate, lane)
                        .and_then(|name| name.object_id.and_then(ObjectId::value))
                }) == Some(source)
                {
                    matches = true;
                    break;
                }
            }
            if matches {
                if position.is_some() {
                    return Ok(None);
                }
                position = Some(candidate);
            }
        }
    }
    Ok(position)
}

/// Whether a hole has a configuration-local position source in the supplied
/// lanes. A lane without this carrier inherits the document hole placements;
/// a lane with one must retain unresolved placement state when projection
/// cannot establish its authored loci.
pub(crate) fn hole_position_carrier_present(
    ctx: &DecodeContext<'_>,
    feature: &cadmpeg_ir::features::Feature,
    histories: &[crate::records::FeatureHistory],
    lanes: &[FeatureInputLane],
) -> Result<bool, CodecError> {
    const OPERATION: &str = "find SLDPRT hole position carrier";
    let Some(native_ref) = feature.native_ref.as_deref() else {
        return Ok(false);
    };
    for history in ctx.admit_iter(histories, OPERATION)? {
        for native in ctx.admit_iter(&history.features, OPERATION)? {
            if !ctx.equal(native.id.as_str(), native_ref, OPERATION)? {
                continue;
            }
            for lane in ctx.admit_iter(lanes, OPERATION)? {
                if hole_position_sketch_source(ctx, native, lane)?.is_some() {
                    return Ok(true);
                }
            }
            return Ok(false);
        }
    }
    Ok(false)
}

pub(crate) fn project_spatial_hole_position_sketches(
    ctx: &DecodeContext<'_>,
    features: &mut [cadmpeg_ir::features::Feature],
    spatial_sketches: &[SpatialSketch],
    spatial_entities: &[SpatialSketchEntity],
    surfaces: &[Surface],
    histories: &[crate::records::FeatureHistory],
    lanes: &[FeatureInputLane],
) -> Result<(), CodecError> {
    const INDEX_OPERATION: &str = "index SLDPRT spatial position features";
    let mut native_features = BTreeMap::new();
    for iteration_history in ctx.admit_iter(histories, "scan SLDPRT holes source records")? {
        for feature in ctx.admit_iter(&iteration_history.features, "scan SLDPRT holes records")? {
            let key = feature.id.as_str();
            ctx.insert_btree_map(&mut native_features, key, feature, INDEX_OPERATION)?;
        }
    }
    for index in 0..features.len() {
        ctx.charge_work(1, "project SLDPRT spatial hole positions")?;
        let feature = &features[index];
        if feature.suppressed == Some(true) {
            continue;
        }
        let FeatureDefinition::Operation(FeatureOperation::Hole {
            placements, shape, ..
        }) = feature.evaluation.definition()
        else {
            continue;
        };
        let Some(diameter) = shape.diameter() else {
            continue;
        };
        if placements.is_some() {
            continue;
        }
        let diameter = diameter.get();
        let Some(native) = (match feature.native_ref.as_deref() {
            Some(native) => ctx
                .get_btree_map(
                    &(native_features),
                    native,
                    "resolve SLDPRT holes references",
                )?
                .copied(),
            None => None,
        }) else {
            continue;
        };
        let Some(position_feature) = hole_position_feature(ctx, native, histories, lanes)? else {
            continue;
        };
        let Some(sketch_index) = ctx.rposition_by(
            features,
            |candidate| {
                let FeatureDefinition::Operation(FeatureOperation::SpatialSketch {
                    sketch: Some(_),
                }) = candidate.evaluation.definition()
                else {
                    return Ok(false);
                };
                ctx.equal(
                    &candidate.native_ref.as_deref(),
                    &Some(position_feature.id.as_str()),
                    "find SLDPRT spatial position sketch",
                )
            },
            "find SLDPRT spatial position sketch",
        )?
        else {
            continue;
        };
        let FeatureDefinition::Operation(FeatureOperation::SpatialSketch {
            sketch: Some(sketch_id),
        }) = features[sketch_index].evaluation.definition()
        else {
            continue;
        };
        let Some(sketch) = ({
            let mut search_result = None;
            for sketch in ctx.admit_iter(spatial_sketches, "scan SLDPRT holes records")? {
                if ctx.equal(&(sketch.id), sketch_id, "compare SLDPRT holes records")? {
                    search_result = Some(sketch);
                    break;
                }
            }
            search_result
        }) else {
            continue;
        };
        let mut authored_markers = Vec::new();
        for lane in ctx.admit_iter(lanes, "scan SLDPRT holes records")? {
            if !ctx.equal(
                &lane.configuration,
                &sketch.configuration,
                "match SLDPRT spatial position configuration",
            )? {
                continue;
            }
            for marker in &lane.sketch_entities {
                ctx.charge_work(1, "scan SLDPRT spatial position markers")?;
                if ctx.equal(
                    &(marker.feature_ref.as_deref()),
                    &(Some(position_feature.id.as_str())),
                    "compare SLDPRT holes records",
                )? && marker.object_index().is_some()
                {
                    ctx.reserve_vec(
                        &mut authored_markers,
                        1,
                        "collect SLDPRT spatial position markers",
                    )?;
                    authored_markers.push(marker);
                }
            }
        }
        let radius = diameter * 0.5;
        let radius_tolerance = (radius.abs() * EPS_HOLE_GEOMETRY).max(EPS_HOLE_GEOMETRY);
        let axis_tolerance_squared = EPS_HOLE_EXACT_GEOMETRY;
        let mut resolved = Vec::new();
        let mut ambiguous = false;
        for marker in ctx.admit_iter(&(authored_markers)[..], "scan SLDPRT holes records")? {
            let mut point = None;
            let mut multiple_points = false;
            for entity in
                ctx.admit_iter(spatial_entities, "match SLDPRT spatial position points")?
            {
                if !ctx.equal(
                    &entity.sketch,
                    sketch_id,
                    "match SLDPRT spatial position points",
                )? || !ctx.equal(
                    &entity.native_ref.as_deref(),
                    &Some(marker.id()),
                    "match SLDPRT spatial position points",
                )? {
                    continue;
                }
                let SpatialSketchGeometryDefinition::Point { position } =
                    entity.geometry.definition()
                else {
                    continue;
                };
                if point.is_some() {
                    multiple_points = true;
                    break;
                }
                point = Some(position.get());
            }
            let Some(point) = point else {
                continue;
            };
            if multiple_points {
                ambiguous = true;
                break;
            }
            let mut axes = Vec::new();
            for surface in surfaces {
                ctx.charge_work(1, "scan SLDPRT spatial bore surfaces")?;
                let candidate_axis = match &surface.geometry {
                    SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cylinder(cylinder_surface)) => {
                        let origin = cylinder_surface.origin().get();
                        let axis = FeatureDirection3::from(*cylinder_surface.frame().axis());
                        let candidate = cylinder_surface.radius().get();
                        ((candidate - radius).abs() <= radius_tolerance
                            && point_axis_distance_squared(point, origin, axis.get())
                                <= axis_tolerance_squared)
                            .then_some((origin, axis))
                    }
                    _ => None,
                };
                if let Some(candidate_axis) = candidate_axis {
                    ctx.reserve_vec(&mut axes, 1, "collect SLDPRT spatial bore axes")?;
                    axes.push(candidate_axis);
                }
            }
            if axes.is_empty() {
                let mut support_axes = Vec::new();
                for surface in surfaces {
                    ctx.charge_work(1, "scan SLDPRT spatial support surfaces")?;
                    if let Some(axis) = cylindrical_support_normal(surface, point) {
                        ctx.reserve_vec(
                            &mut support_axes,
                            1,
                            "collect SLDPRT spatial support axes",
                        )?;
                        support_axes.push(canonical_axis(axis));
                    }
                }
                ctx.sort_unstable_by(
                    &mut support_axes,
                    |value| value,
                    |left, right| {
                        [left.x.to_bits(), left.y.to_bits(), left.z.to_bits()].cmp(&[
                            right.x.to_bits(),
                            right.y.to_bits(),
                            right.z.to_bits(),
                        ])
                    },
                    "sort SLDPRT spatial support axes",
                )?;
                ctx.charge_work(
                    u64_from_index(support_axes.len()),
                    "deduplicate SLDPRT spatial support axes",
                )?;
                support_axes.dedup_by(|left, right| left.dot(*right) >= 1.0 - EPS_HOLE_GEOMETRY);
                if let [axis] = support_axes.as_slice() {
                    let Some(axis) = FeatureDirection3::new(*axis) else {
                        continue;
                    };
                    ctx.reserve_vec(&mut axes, 1, "collect SLDPRT spatial bore axes")?;
                    axes.push((point, axis));
                }
            }
            let Some(axes) = carrier_placements(
                ctx,
                ctx.admit_iter(&axes[..], "collect SLDPRT hole carrier axes")?
                    .copied(),
            )?
            else {
                continue;
            };
            let [placement] = axes.as_slice() else {
                ambiguous = true;
                break;
            };
            ctx.reserve_vec(&mut resolved, 1, "collect SLDPRT spatial hole placements")?;
            resolved
                .push(placement.try_clone_for_decode(ctx, "SLDPRT spatial hole placement copy")?);
        }
        if resolved.is_empty() && !ambiguous {
            let mut points = Vec::new();
            for entity in spatial_entities {
                ctx.charge_work(1, "scan SLDPRT spatial position points")?;
                if ctx.equal(
                    &entity.sketch,
                    sketch_id,
                    "match SLDPRT spatial position points",
                )? {
                    if let SpatialSketchGeometryDefinition::Point { position } =
                        entity.geometry.definition()
                    {
                        ctx.reserve_vec(&mut points, 1, "collect SLDPRT spatial position points")?;
                        points.push(position.get());
                    }
                }
            }
            if let Some(inferred) = coplanar_spatial_position_placements(ctx, &points)? {
                resolved = inferred;
            }
        }
        let placement_key = |placement: &HolePlacement| match placement {
            HolePlacement::Axis { origin, axis } => [
                origin.x.to_bits(),
                origin.y.to_bits(),
                origin.z.to_bits(),
                axis.x.to_bits(),
                axis.y.to_bits(),
                axis.z.to_bits(),
            ],
            HolePlacement::Directed { .. } => [0; 6],
        };
        ctx.sort_unstable_by_key(
            &mut resolved,
            |value| placement_key(value),
            Ord::cmp,
            "sort SLDPRT spatial hole placements",
        )?;
        ctx.charge_work(
            u64_from_index(resolved.len()),
            "deduplicate SLDPRT spatial hole placements",
        )?;
        resolved.dedup();
        if !ambiguous && !resolved.is_empty() {
            features[index].evaluation.edit(|definition, _| {
                if let FeatureDefinition::Operation(FeatureOperation::Hole { placements, .. }) =
                    definition
                {
                    *placements = Some(resolved);
                }
            });
        }
    }
    Ok(())
}

fn coplanar_spatial_position_placements(
    ctx: &DecodeContext<'_>,
    points: &[Point3],
) -> Result<Option<Vec<HolePlacement>>, CodecError> {
    let mut sorted_points = Vec::new();
    ctx.reserve_vec(
        &mut sorted_points,
        points.len(),
        "sort SLDPRT spatial position points",
    )?;
    ctx.charge_work(
        u64_from_index(points.len()),
        "copy SLDPRT spatial position points",
    )?;
    sorted_points.extend_from_slice(points);
    let mut points = sorted_points;
    ctx.sort_unstable_by(
        &mut points,
        |value| value,
        |left, right| {
            [left.x.to_bits(), left.y.to_bits(), left.z.to_bits()].cmp(&[
                right.x.to_bits(),
                right.y.to_bits(),
                right.z.to_bits(),
            ])
        },
        "sort SLDPRT spatial position points",
    )?;
    ctx.charge_work(
        u64_from_index(points.len()),
        "deduplicate SLDPRT spatial position points",
    )?;
    points.dedup();
    if points.len() < 3
        || ctx
            .admit_iter(&(points)[..], "scan SLDPRT holes records")?
            .any(|point| !point.is_finite())
    {
        return Ok(None);
    }
    let displacement = |point: Point3| {
        Vector3::new(
            point.x - points[0].x,
            point.y - points[0].y,
            point.z - points[0].z,
        )
    };
    let extent = ctx
        .admit_iter(&(points)[..], "scan SLDPRT holes records")?
        .skip(1)
        .map(|point| displacement(*point).norm())
        .fold(1.0_f64, f64::max);
    let Some(first) = ctx.max_by(
        &points[1..],
        |left, right| {
            Ok(displacement(*left)
                .norm()
                .total_cmp(&displacement(*right).norm()))
        },
        "select SLDPRT spatial position extent",
    )?
    else {
        return Ok(None);
    };
    let first = displacement(*first);
    let Some(candidate) = ctx.max_by(
        &points[1..],
        |left, right| {
            Ok(first
                .cross(displacement(*left))
                .norm()
                .total_cmp(&first.cross(displacement(*right)).norm()))
        },
        "select SLDPRT spatial position normal",
    )?
    else {
        return Ok(None);
    };
    let candidate = first.cross(displacement(*candidate));
    let norm = candidate.norm();
    if norm <= extent * extent * EPS_HOLE_DEGENERATE_NORMAL {
        return Ok(None);
    }
    let normal = Vector3::new(candidate.x / norm, candidate.y / norm, candidate.z / norm);
    if ctx
        .admit_iter(&(points)[..], "scan SLDPRT holes records")?
        .any(|point| {
            Vector3::new(
                point.x - points[0].x,
                point.y - points[0].y,
                point.z - points[0].z,
            )
            .dot(normal)
            .abs()
                > extent * EPS_HOLE_POSITION
        })
    {
        return Ok(None);
    }
    let axis = canonical_axis(normal);
    let axis = Vector3::new(
        if axis.x.abs() <= EPS_HOLE_EXACT_GEOMETRY {
            0.0
        } else {
            axis.x
        },
        if axis.y.abs() <= EPS_HOLE_EXACT_GEOMETRY {
            0.0
        } else {
            axis.y
        },
        if axis.z.abs() <= EPS_HOLE_EXACT_GEOMETRY {
            0.0
        } else {
            axis.z
        },
    );
    let mut placements = Vec::new();
    for origin in ctx
        .admit_iter(&points[..], "scan SLDPRT holes records")?
        .copied()
    {
        let Some(origin) = FinitePoint3::new(origin) else {
            return Ok(None);
        };
        let Some(axis) = FeatureDirection3::new(axis) else {
            return Ok(None);
        };
        ctx.reserve_vec(&mut placements, 1, "collect SLDPRT coplanar position axes")?;
        placements.push(HolePlacement::Axis { origin, axis });
    }
    Ok(Some(placements))
}

/// Resolve hole axes from persistent identities of faces generated by the
/// operation. Each configuration lane must identify the same cylindrical
/// axes; local identities that name planar or secondary-diameter faces do not
/// participate.
pub(crate) fn project_generated_hole_axes(
    ctx: &DecodeContext<'_>,
    features: &mut [cadmpeg_ir::features::Feature],
    histories: &[crate::records::FeatureHistory],
    lanes: &[FeatureInputLane],
    face_identities: &[(cadmpeg_ir::ids::FaceId, crate::brep::PersistentFaceIdentity)],
    faces: &[Face],
    surfaces: &[Surface],
) -> Result<(), CodecError> {
    const AXIS_QUANTUM: f64 = EPS_HOLE_POSITION;
    let quantize = |value: f64| GridCoordinate::new(value, AXIS_QUANTUM);
    let mut native_features_storage =
        ctx.reserve_scoped(0, "index SLDPRT generated hole features")?;
    let mut native_features = BTreeMap::new();
    for iteration_history in ctx.admit_iter(histories, "scan SLDPRT holes source records")? {
        for feature in ctx.admit_iter(&iteration_history.features, "scan SLDPRT holes records")? {
            let key = feature.id.as_str();
            native_features_storage.with_storage(|| {
                ctx.insert_btree_map(
                    &mut native_features,
                    key,
                    feature,
                    "index SLDPRT generated hole features",
                )
            })?;
        }
    }
    let mut faces_by_id = HashMap::new();
    for face in faces {
        ctx.charge_work(1, "index SLDPRT generated hole faces")?;
        let key = face.id.as_str();
        ctx.insert_hash_map(
            &mut faces_by_id,
            key,
            face,
            "index SLDPRT generated hole faces",
        )?;
    }
    let mut surfaces_by_id = HashMap::new();
    for surface in surfaces {
        ctx.charge_work(1, "index SLDPRT generated hole surfaces")?;
        let key = surface.id.as_str();
        ctx.insert_hash_map(
            &mut surfaces_by_id,
            key,
            surface,
            "index SLDPRT generated hole surfaces",
        )?;
    }

    const OPERATION: &str = "sort SLDPRT generated hole lanes";
    for feature_index in ctx.admit_iter(&(0..features.len()), OPERATION)? {
        let feature = &mut features[feature_index];
        let solution = (|| -> Result<Option<Vec<HolePlacement>>, CodecError> {
            let FeatureDefinition::Operation(FeatureOperation::Hole {
                placements, shape, ..
            }) = feature.evaluation.definition()
            else {
                return Ok(None);
            };
            let Some(diameter) = shape.diameter() else {
                return Ok(None);
            };
            let diameter = diameter.get();

            if placements.is_some() {
                return Ok(None);
            }
            let Some(source) = match feature.native_ref.as_deref() {
                Some(native) => ctx.get_btree_map(
                    &(native_features),
                    native,
                    "resolve SLDPRT holes references",
                )?,
                None => None,
            }
            .and_then(|native| native.source_id)
            .and_then(FeatureSource::id) else {
                return Ok(None);
            };
            let radius = diameter * 0.5;
            let radius_tolerance = (radius.abs() * EPS_HOLE_GEOMETRY).max(EPS_HOLE_GEOMETRY);
            let mut lane_solutions = Vec::new();
            for lane in ctx.admit_iter(lanes, "scan SLDPRT holes records")? {
                let local_identities = &lane.generated_surface_identities;
                let mut local_ids = HashSet::new();
                for identity in ctx.admit_iter(local_identities, "scan SLDPRT holes records")? {
                    if !ctx.equal(&identity.feature_source_id, &source, OPERATION)? {
                        continue;
                    }

                    ctx.insert_hash_set(
                        &mut local_ids,
                        identity.local_identity,
                        "index SLDPRT generated hole surface identities",
                    )?;
                }
                if local_ids.is_empty() {
                    continue;
                }
                let mut axes_storage = ctx.reserve_scoped(0, "index SLDPRT generated hole axes")?;
                let mut axes = HashMap::<[GridCoordinate; 6], HolePlacement>::new();
                for (face, identity) in face_identities {
                    ctx.charge_work(1, "scan SLDPRT generated hole faces")?;
                    if !ctx.equal(
                        &(identity.feature_source_id),
                        &(source),
                        "compare SLDPRT holes records",
                    )? || !local_ids.contains(&identity.local_id)
                    {
                        continue;
                    }
                    let Some(surface) = (match ctx.get_hash_map(
                        &faces_by_id,
                        face.as_str(),
                        "resolve SLDPRT holes keys",
                    )? {
                        Some(face) => ctx.get_hash_map(
                            &(surfaces_by_id),
                            face.surface.as_str(),
                            "resolve SLDPRT holes references",
                        )?,
                        None => None,
                    }) else {
                        continue;
                    };
                    let Some(SolvedSurfaceGeometry::Cylinder(cylinder_surface)) =
                        surface.geometry.solved()
                    else {
                        continue;
                    };
                    let origin = cylinder_surface.origin().get();
                    let candidate_radius = cylinder_surface.radius().get();
                    if (candidate_radius - radius).abs() > radius_tolerance {
                        continue;
                    }
                    let axis = canonical_direction(FeatureDirection3::from(
                        *cylinder_surface.frame().axis(),
                    ));
                    let station = Vector3::new(origin.x, origin.y, origin.z).dot(axis.get());
                    let closest = Point3::new(
                        origin.x - station * axis.x,
                        origin.y - station * axis.y,
                        origin.z - station * axis.z,
                    );
                    let Some(closest) = FinitePoint3::new(closest) else {
                        axes.clear();
                        break;
                    };
                    let key = [
                        quantize(closest.x),
                        quantize(closest.y),
                        quantize(closest.z),
                        quantize(axis.x),
                        quantize(axis.y),
                        quantize(axis.z),
                    ];
                    axes_storage.with_storage(|| {
                        if let std::collections::hash_map::Entry::Vacant(entry) =
                            ctx.entry_hash_map(&mut axes, key, "index SLDPRT generated hole axes")?
                        {
                            entry.insert(HolePlacement::Axis {
                                origin: closest,
                                axis,
                            });
                        }
                        Ok::<(), CodecError>(())
                    })?;
                }
                if axes.is_empty() {
                    continue;
                }
                let mut solution = Vec::new();
                ctx.reserve_vec(&mut solution, axes.len(), "sort SLDPRT generated hole axes")?;
                solution.extend(axes);
                ctx.sort_unstable_by(
                    &mut solution,
                    |value| &value.0,
                    Ord::cmp,
                    "sort SLDPRT generated hole axes",
                )?;
                let mut placements = Vec::new();
                ctx.reserve_vec(
                    &mut placements,
                    solution.len(),
                    "collect SLDPRT generated hole placements",
                )?;
                placements.extend(solution.into_iter().map(|(_, placement)| placement));
                ctx.reserve_vec(
                    &mut lane_solutions,
                    1,
                    "collect SLDPRT generated hole lanes",
                )?;
                let input_index = lane_solutions.len();
                lane_solutions.push((input_index, placements));
            }
            let placement_key = |placement: &HolePlacement| match placement {
                HolePlacement::Axis { origin, axis } => [
                    quantize(origin.x),
                    quantize(origin.y),
                    quantize(origin.z),
                    quantize(axis.x),
                    quantize(axis.y),
                    quantize(axis.z),
                ],
                HolePlacement::Directed { .. } => [GridCoordinate::Cell(0); 6],
            };
            ctx.sort_unstable_by(
                &mut lane_solutions,
                |value| value,
                |(left_index, left), (right_index, right)| {
                    left.iter()
                        .map(placement_key)
                        .cmp(right.iter().map(placement_key))
                        .then_with(|| left_index.cmp(right_index))
                },
                OPERATION,
            )?;
            ctx.dedup_by(
                &mut lane_solutions,
                |left, right| ctx.equal(&left.1, &right.1, OPERATION),
                OPERATION,
            )?;
            if lane_solutions.len() == 1 {
                return Ok(lane_solutions.pop().map(|(_, placements)| placements));
            }
            Ok(None)
        })()?;
        if let Some(solution) = solution {
            feature.evaluation.edit(|definition, _| {
                if let FeatureDefinition::Operation(FeatureOperation::Hole { placements, .. }) =
                    definition
                {
                    *placements = Some(solution);
                }
            });
        }
    }
    Ok(())
}

/// Resolve placements from exact dimensional topology matches.
/// Counterbores require identical primary and counterbore axis sets. Flat
/// blind holes require a finite cylinder span equal to the declared depth.
/// Drilled holes additionally require a coaxial cone with the declared angle.
/// Ownership must be unique, or exact seed placements must partition the
/// remaining carrier set without a shared or unowned direction.
pub(crate) fn project_hole_topology_axes(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    features: &mut [cadmpeg_ir::features::Feature],
    topology: &HoleTopology<'_>,
) -> Result<(), cadmpeg_core::CodecError> {
    const DIAMETER_LOOKUP: &str = "index SLDPRT hole diameters";
    let mut diameter_counts = HashMap::<u64, usize>::new();
    let mut unresolved = Vec::new();
    for (index, feature) in features.iter().enumerate() {
        ctx.charge_work(1, DIAMETER_LOOKUP)?;
        if feature.suppressed == Some(true) {
            continue;
        }
        let FeatureDefinition::Operation(FeatureOperation::Hole {
            placements, shape, ..
        }) = feature.evaluation.definition()
        else {
            continue;
        };
        let Some(diameter) = shape.diameter() else {
            continue;
        };
        let key = diameter.get().to_bits();
        ctx.admit_hash_map_entry(&mut diameter_counts, &key, DIAMETER_LOOKUP)?;
        *diameter_counts.entry(key).or_default() += 1;
        if placements.is_none() {
            ctx.reserve_vec(&mut unresolved, 1, "collect SLDPRT unresolved holes")?;
            unresolved.push((index, diameter));
        }
    }

    for (unresolved_index, diameter) in ctx
        .admit_iter(&unresolved, "scan SLDPRT unresolved counterbores")?
        .copied()
    {
        const CANDIDATE_KEYS: &str = "index SLDPRT counterbore candidate axes";

        let diameter = diameter.get();

        let Some(candidates) = counterbore_topology_candidates(
            ctx,
            features[unresolved_index].evaluation.definition(),
            topology,
        )?
        else {
            continue;
        };
        if diameter_counts.get(&diameter.to_bits()) == Some(&1) {
            set_hole_placements(&mut features[unresolved_index], candidates);
            continue;
        }

        let mut siblings = Vec::new();
        for (index, feature) in features.iter().enumerate() {
            ctx.charge_work(1, "scan SLDPRT counterbore siblings")?;
            if feature.suppressed == Some(true)
                || !same_hole_construction(
                    ctx,
                    features[unresolved_index].evaluation.definition(),
                    feature.evaluation.definition(),
                )?
            {
                continue;
            }
            if let FeatureDefinition::Operation(FeatureOperation::Hole { placements, .. }) =
                feature.evaluation.definition()
            {
                ctx.reserve_vec(&mut siblings, 1, "collect SLDPRT counterbore siblings")?;
                siblings.push((index, placements));
            }
        }
        if siblings.len() < 2
            || ctx
                .admit_iter(&(siblings)[..], "scan SLDPRT holes records")?
                .filter(|(_, placements)| placements.is_none())
                .count()
                != 1
        {
            continue;
        }

        let mut candidate_keys = HashSet::new();
        for key in candidates.iter().filter_map(hole_axis_key) {
            ctx.charge_work(1, CANDIDATE_KEYS)?;
            ctx.insert_hash_set(&mut candidate_keys, key, CANDIDATE_KEYS)?;
        }
        if candidate_keys.len() != candidates.len() {
            continue;
        }

        let mut claimed = HashSet::new();
        let mut complete = true;
        for (_, placements) in ctx
            .admit_iter(&(siblings)[..], "scan SLDPRT holes records")?
            .filter(|(index, _)| *index != unresolved_index)
        {
            let Some(placements) = placements.as_deref() else {
                complete = false;
                break;
            };
            for placement in placements {
                let Some(key) = hole_axis_key(placement) else {
                    complete = false;
                    break;
                };
                if !candidate_keys.contains(&key) || claimed.contains(&key) {
                    complete = false;
                    break;
                }
                ctx.insert_hash_set(&mut claimed, key, "claim SLDPRT counterbore axis")?;
            }
            if !complete {
                break;
            }
        }
        if !complete || claimed.is_empty() {
            continue;
        }

        let mut residual = Vec::new();
        for placement in candidates {
            ctx.charge_work(1, "select SLDPRT residual counterbore axes")?;
            if hole_axis_key(&placement).is_some_and(|key| !claimed.contains(&key)) {
                ctx.reserve_vec(&mut residual, 1, "collect SLDPRT residual counterbore axes")?;
                residual.push(placement);
            }
        }
        if residual.is_empty() {
            continue;
        }
        set_hole_placements(&mut features[unresolved_index], residual);
    }

    let mut cylinders_storage = ctx.reserve_scoped(0, "collect SLDPRT hole bore spans")?;
    let cylinders =
        cylinders_storage.with_storage(|| cylindrical_bore_face_spans(ctx, topology))?;
    project_flat_blind_topology_axes(ctx, features, &cylinders)?;
    project_drilled_hole_topology_axes(ctx, features, &cylinders, topology)?;
    Ok(())
}

fn project_flat_blind_topology_axes(
    ctx: &DecodeContext<'_>,
    features: &mut [cadmpeg_ir::features::Feature],
    cylinders: &[BoreFaceSpan],
) -> Result<(), CodecError> {
    let candidates = ctx
        .admit_iter(&features[..], "scan SLDPRT hole topology candidates")?
        .enumerate()
        .filter(|(_, feature)| feature.suppressed != Some(true))
        .filter_map(|(index, feature)| match feature.evaluation.definition() {
            FeatureDefinition::Operation(FeatureOperation::Hole {
                placements: ref hole_placements,
                shape,

                extent: Some(LinearTermination::Blind { length }),
                bottom: Some(HoleBottom::Flat),
                ..
            }) => match (shape.construction(), &shape.diameter()) {
                (
                    cadmpeg_ir::features::holes::HoleConstruction::Form {
                        kind: HoleKind::Simple,
                        ..
                    },
                    Some(diameter),
                ) if hole_placements.is_none() && length.get() > 0.0 => {
                    let diameter = diameter.get();
                    let length = length.get();

                    Some((index, diameter, length))
                }
                _ => None,
            },
            _ => None,
        });
    let mut unresolved = Vec::new();
    for candidate in candidates {
        ctx.reserve_vec(&mut unresolved, 1, "collect SLDPRT flat blind holes")?;
        unresolved.push(candidate);
    }

    for (index, diameter, length) in ctx
        .admit_iter(&unresolved, "scan SLDPRT unresolved blind holes")?
        .copied()
    {
        if !hole_construction_is_unique(ctx, features, index)? {
            continue;
        }
        let radius = diameter * 0.5;
        let radius_tolerance = (radius.abs() * EPS_HOLE_GEOMETRY).max(EPS_HOLE_GEOMETRY);
        let length_tolerance = (length.abs() * EPS_HOLE_GEOMETRY).max(EPS_HOLE_POSITION);
        let Some(placements) = carrier_placements(
            ctx,
            ctx.admit_iter(cylinders, "collect SLDPRT hole carrier axes")?
                .filter_map(
                    |BoreFaceSpan(origin, axis, candidate_radius, candidate_span, _)| {
                        ((candidate_radius - radius).abs() <= radius_tolerance
                            && (candidate_span - length).abs() <= length_tolerance)
                            .then_some((*origin, *axis))
                    },
                ),
        )?
        else {
            continue;
        };
        set_hole_placements(&mut features[index], placements);
    }
    Ok(())
}

fn project_drilled_hole_topology_axes(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    features: &mut [cadmpeg_ir::features::Feature],
    cylinders: &[BoreFaceSpan],
    topology: &HoleTopology<'_>,
) -> Result<(), cadmpeg_core::CodecError> {
    expand_seeded_drilled_hole_topology_axes(ctx, features, cylinders, topology)?;
    let candidates = ctx
        .admit_iter(&features[..], "scan SLDPRT hole topology candidates")?
        .enumerate()
        .filter(|(_, feature)| feature.suppressed != Some(true))
        .filter_map(|(index, feature)| match feature.evaluation.definition() {
            FeatureDefinition::Operation(FeatureOperation::Hole {
                placements: ref hole_placements,
                shape,

                extent: Some(LinearTermination::Blind { length }),
                bottom:
                    Some(HoleBottom::Angled {
                        included_angle: bottom_angle,
                        depth_to_tip: false,
                    }),
                ..
            }) => match (shape.construction(), &shape.diameter()) {
                (
                    cadmpeg_ir::features::holes::HoleConstruction::Form {
                        kind: HoleKind::SimpleDrilled { drill_point_angle },
                        ..
                    },
                    Some(diameter),
                ) if hole_placements.is_none()
                    && length.get() > 0.0
                    && (bottom_angle.get() - drill_point_angle.get()).abs()
                        <= EPS_HOLE_GEOMETRY =>
                {
                    let drill_point_angle = drill_point_angle.get();
                    let diameter = diameter.get();
                    let length = length.get();

                    Some((index, diameter, length, drill_point_angle))
                }
                _ => None,
            },
            _ => None,
        });
    let mut unresolved = Vec::new();
    for candidate in candidates {
        ctx.reserve_vec(
            &mut unresolved,
            1,
            "collect SLDPRT unresolved drilled holes",
        )?;
        unresolved.push(candidate);
    }

    for (index, diameter, length, drill_point_angle) in ctx
        .admit_iter(&unresolved, "scan SLDPRT unresolved drilled holes")?
        .copied()
    {
        if !hole_construction_is_unique(ctx, features, index)? {
            continue;
        }
        let Some(placements) = drilled_hole_topology_candidates(
            ctx,
            diameter,
            length,
            drill_point_angle,
            cylinders,
            topology.surfaces,
        )?
        else {
            continue;
        };
        set_hole_placements(&mut features[index], placements);
    }
    Ok(())
}

fn drilled_hole_topology_candidates(
    ctx: &DecodeContext<'_>,
    diameter: f64,
    length: f64,
    drill_point_angle: f64,
    cylinders: &[BoreFaceSpan],
    surfaces: &[Surface],
) -> Result<Option<Vec<HolePlacement>>, CodecError> {
    let radius = diameter * 0.5;
    let radius_tolerance = (radius.abs() * EPS_HOLE_GEOMETRY).max(EPS_HOLE_GEOMETRY);
    let length_tolerance = (length.abs() * EPS_HOLE_GEOMETRY).max(EPS_HOLE_POSITION);
    let mut cone_keys = HashSet::new();
    for surface in surfaces {
        ctx.charge_work(1, "scan SLDPRT drilled hole cone surfaces")?;
        let key = match surface.geometry {
            SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cone(cone_surface))
                if {
                    let candidate_radius = cone_surface.radius().get();
                    let ratio = cone_surface.ratio().get();
                    let half_angle = cone_surface.half_angle().get();
                    (candidate_radius - radius).abs() <= radius_tolerance
                        && (ratio - 1.0).abs() <= EPS_HOLE_GEOMETRY
                        && (half_angle - drill_point_angle * 0.5).abs() <= EPS_HOLE_GEOMETRY
                } =>
            {
                hole_axis_key(&HolePlacement::Axis {
                    origin: cone_surface.origin(),
                    axis: FeatureDirection3::from(*cone_surface.frame().axis()),
                })
            }
            _ => None,
        };
        if let Some(key) = key {
            ctx.insert_hash_set(&mut cone_keys, key, "index SLDPRT drilled hole cone axes")?;
        }
    }
    if cone_keys.is_empty() {
        return Ok(None);
    }
    let Some(placements) = carrier_placements(
        ctx,
        ctx.admit_iter(cylinders, "collect SLDPRT hole carrier axes")?
            .filter_map(
                |BoreFaceSpan(origin, axis, candidate_radius, candidate_span, _)| {
                    ((candidate_radius - radius).abs() <= radius_tolerance
                        && (candidate_span - length).abs() <= length_tolerance)
                        .then_some((*origin, *axis))
                },
            ),
    )?
    else {
        return Ok(None);
    };
    let mut matched = Vec::new();
    for placement in placements {
        ctx.charge_work(1, "match SLDPRT drilled hole cone axes")?;
        if hole_axis_key(&placement).is_some_and(|key| cone_keys.contains(&key)) {
            ctx.reserve_vec(&mut matched, 1, "collect SLDPRT drilled hole cone axes")?;
            matched.push(placement);
        }
    }
    Ok((!matched.is_empty()).then_some(matched))
}

fn expand_seeded_drilled_hole_topology_axes(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    features: &mut [cadmpeg_ir::features::Feature],
    cylinders: &[BoreFaceSpan],
    topology: &HoleTopology<'_>,
) -> Result<(), cadmpeg_core::CodecError> {
    let mut visited = HashSet::new();
    for index in 0..features.len() {
        ctx.charge_work(1, "scan SLDPRT seeded drilled holes")?;
        if visited.contains(&index) || features[index].suppressed == Some(true) {
            continue;
        }
        let FeatureDefinition::Operation(FeatureOperation::Hole {
            placements,
            shape,

            extent: Some(LinearTermination::Blind { length }),
            bottom:
                Some(HoleBottom::Angled {
                    included_angle: bottom_angle,
                    depth_to_tip: false,
                }),
            ..
        }) = features[index].evaluation.definition()
        else {
            continue;
        };
        let cadmpeg_ir::features::holes::HoleConstruction::Form {
            kind: HoleKind::SimpleDrilled { drill_point_angle },
            ..
        } = shape.construction()
        else {
            continue;
        };
        let Some(diameter) = &shape.diameter() else {
            continue;
        };
        let drill_point_angle = drill_point_angle.get();
        let diameter = diameter.get();
        let length = length.get();
        let bottom_angle = bottom_angle.get();

        let Some(placements) = placements.as_deref() else {
            continue;
        };
        if placements.is_empty()
            || length <= 0.0
            || (bottom_angle - drill_point_angle).abs() > EPS_HOLE_GEOMETRY
        {
            continue;
        }
        let mut siblings = Vec::new();
        for (sibling, feature) in features.iter().enumerate() {
            ctx.charge_work(1, "scan SLDPRT seeded hole siblings")?;
            if feature.suppressed == Some(true)
                || !same_hole_construction(
                    ctx,
                    features[index].evaluation.definition(),
                    feature.evaluation.definition(),
                )?
            {
                continue;
            }
            ctx.reserve_vec(&mut siblings, 1, "collect SLDPRT seeded hole siblings")?;
            siblings.push(sibling);
        }
        for &sibling in ctx.admit_iter(&(siblings)[..], "scan SLDPRT holes records")? {
            ctx.insert_hash_set(&mut visited, sibling, "index SLDPRT visited seeded holes")?;
        }
        if siblings.len() < 2
            || ctx.admit_iter(&(siblings)[..], "scan SLDPRT holes records")?.any(|&sibling| {
                matches!(
                    features[sibling].evaluation.definition(),
                    FeatureDefinition::Operation(FeatureOperation::Hole { placements, .. }) if placements.is_none()
                )
            })
        {
            continue;
        }
        let primary = drilled_hole_topology_candidates(
            ctx,
            diameter,
            length,
            drill_point_angle,
            cylinders,
            topology.surfaces,
        )?;
        let primary = primary
            .map(|candidates| {
                unclaimed_seeded_hole_candidates(ctx, features, &siblings, diameter, candidates)
            })
            .transpose()?
            .flatten();
        let candidates = match primary {
            Some(candidates) => Some(candidates),
            None => seeded_drilled_bore_candidates(ctx, features, &siblings, diameter, topology)?,
        };
        let Some(candidates) = candidates else {
            continue;
        };
        partition_seeded_hole_axes(ctx, features, &siblings, &candidates)?;
    }
    Ok(())
}

fn seeded_drilled_bore_candidates(
    ctx: &DecodeContext<'_>,
    features: &[cadmpeg_ir::features::Feature],
    siblings: &[usize],
    diameter: f64,
    topology: &HoleTopology<'_>,
) -> Result<Option<Vec<HolePlacement>>, CodecError> {
    let Some(candidates) = bore_carrier_placements(ctx, diameter * 0.5, topology)? else {
        return Ok(None);
    };
    unclaimed_seeded_hole_candidates(ctx, features, siblings, diameter, candidates)
}

fn unclaimed_seeded_hole_candidates(
    ctx: &DecodeContext<'_>,
    features: &[cadmpeg_ir::features::Feature],
    siblings: &[usize],
    diameter: f64,
    candidates: Vec<HolePlacement>,
) -> Result<Option<Vec<HolePlacement>>, CodecError> {
    const OPERATION: &str = "filter SLDPRT seeded drilled bore candidates";
    let mut claimed = HashSet::new();
    for (index, feature) in ctx
        .admit_iter(features, "scan SLDPRT holes records")?
        .enumerate()
    {
        ctx.charge_work(u64_from_index(siblings.len()), OPERATION)?;
        if feature.suppressed == Some(true) || siblings.contains(&index) {
            continue;
        }
        let FeatureDefinition::Operation(FeatureOperation::Hole {
            shape, placements, ..
        }) = feature.evaluation.definition()
        else {
            continue;
        };
        if shape
            .diameter()
            .is_none_or(|candidate| candidate.get().to_bits() != diameter.to_bits())
        {
            continue;
        }
        let Some(placements) = placements.as_deref() else {
            return Ok(None);
        };
        for placement in placements {
            ctx.charge_work(1, OPERATION)?;
            let Some(key) = hole_axis_key(placement) else {
                return Ok(None);
            };
            ctx.insert_hash_set(&mut claimed, key, "index SLDPRT claimed bore axes")?;
        }
    }
    let mut available = Vec::new();
    for placement in candidates {
        ctx.charge_work(1, OPERATION)?;
        if hole_axis_key(&placement).is_some_and(|key| !claimed.contains(&key)) {
            ctx.reserve_vec(&mut available, 1, "collect SLDPRT unclaimed bore axes")?;
            available.push(placement);
        }
    }
    Ok((!available.is_empty()).then_some(available))
}

fn partition_seeded_hole_axes(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    features: &mut [cadmpeg_ir::features::Feature],
    siblings: &[usize],
    candidates: &[HolePlacement],
) -> Result<(), cadmpeg_core::CodecError> {
    const KEY_OPERATION: &str = "SLDPRT seeded hole-axis keys";
    let mut candidate_keys = HashSet::new();
    for key in ctx
        .admit_iter(candidates, "scan SLDPRT holes records")?
        .filter_map(hole_axis_key)
    {
        ctx.insert_hash_set(&mut candidate_keys, key, KEY_OPERATION)?;
    }
    if candidate_keys.len() != candidates.len() {
        return Ok(());
    }
    let mut seed_directions_storage =
        ctx.reserve_scoped(0, "SLDPRT seeded hole-axis directions")?;
    let mut seed_directions: Vec<Vector3> = Vec::new();
    for &sibling in ctx.admit_iter(siblings, "scan SLDPRT holes records")? {
        let FeatureDefinition::Operation(FeatureOperation::Hole {
            placements: Some(placements),
            ..
        }) = features[sibling].evaluation.definition()
        else {
            return Ok(());
        };
        let mut axes = ctx
            .admit_iter(placements, "scan SLDPRT seeded hole axes")?
            .filter_map(|placement| match placement {
                HolePlacement::Axis { axis, .. } => Some(canonical_axis(axis.get())),
                HolePlacement::Directed { .. } => None,
            });
        let Some(direction) = axes.next() else {
            return Ok(());
        };
        if axes.any(|axis| axis.dot(direction) < 1.0 - EPS_HOLE_GEOMETRY)
            || ctx
                .admit_iter(&(placements)[..], "scan SLDPRT holes records")?
                .any(|placement| {
                    hole_axis_key(placement).is_none_or(|key| !candidate_keys.contains(&key))
                })
            || ctx
                .admit_iter(&(seed_directions)[..], "scan SLDPRT holes records")?
                .any(|candidate| candidate.dot(direction) >= 1.0 - EPS_HOLE_GEOMETRY)
        {
            return Ok(());
        }
        seed_directions_storage.with_storage(|| {
            ctx.push_vec(
                &mut seed_directions,
                direction,
                "SLDPRT seeded hole-axis directions",
            )
        })?;
    }

    let mut partitions =
        ctx.collect_indexed_vec(siblings.len(), "SLDPRT seeded hole-axis partitions", |_| {
            Ok(Vec::<HolePlacement>::new())
        })?;
    for placement in ctx.admit_iter(candidates, "scan SLDPRT holes records")? {
        let HolePlacement::Axis { axis, .. } = placement else {
            return Ok(());
        };
        let direction = canonical_axis(axis.get());
        ctx.charge_work(
            u64_from_index(seed_directions.len()),
            "match SLDPRT seeded hole-axis directions",
        )?;
        let mut matches = seed_directions
            .iter()
            .enumerate()
            .filter(|(_, seed)| seed.dot(direction) >= 1.0 - EPS_HOLE_GEOMETRY)
            .map(|(index, _)| index);
        let (Some(partition), None) = (matches.next(), matches.next()) else {
            return Ok(());
        };
        ctx.push_vec(
            &mut partitions[partition],
            placement.try_clone_for_decode(ctx, "SLDPRT seeded hole placement copy")?,
            "SLDPRT seeded hole-axis placements",
        )?;
    }
    if ctx
        .admit_iter(&(partitions)[..], "scan SLDPRT holes records")?
        .any(Vec::is_empty)
    {
        return Ok(());
    }
    for (&sibling, partition) in ctx
        .admit_iter(siblings, "scan SLDPRT holes records")?
        .zip(partitions)
    {
        set_hole_placements(&mut features[sibling], partition);
    }
    Ok(())
}

fn set_hole_placements(feature: &mut cadmpeg_ir::features::Feature, value: Vec<HolePlacement>) {
    feature.evaluation.edit(|definition, _| {
        if let FeatureDefinition::Operation(FeatureOperation::Hole { placements, .. }) = definition
        {
            *placements = Some(value);
        }
    });
}

fn hole_construction_is_unique(
    ctx: &DecodeContext<'_>,
    features: &[cadmpeg_ir::features::Feature],
    index: usize,
) -> Result<bool, CodecError> {
    const OPERATION: &str = "count SLDPRT matching hole constructions";
    let mut count = 0usize;
    for feature in ctx.admit_iter(features, OPERATION)? {
        if feature.suppressed != Some(true)
            && same_hole_construction(
                ctx,
                features[index].evaluation.definition(),
                feature.evaluation.definition(),
            )?
        {
            count = count
                .checked_add(1)
                .ok_or_else(|| ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX))?;
        }
    }
    Ok(count == 1)
}

fn counterbore_topology_candidates(
    ctx: &DecodeContext<'_>,
    definition: &FeatureDefinition,
    topology: &HoleTopology<'_>,
) -> Result<Option<Vec<HolePlacement>>, CodecError> {
    let FeatureDefinition::Operation(FeatureOperation::Hole { shape, .. }) = definition else {
        return Ok(None);
    };
    let Some(diameter) = &shape.diameter() else {
        return Ok(None);
    };
    let cadmpeg_ir::features::holes::HoleConstruction::Form {
        kind:
            HoleKind::Counterbore {
                diameter: counterbore_diameter,
                ..
            }
            | HoleKind::CounterboreDrilled {
                diameter: counterbore_diameter,
                ..
            },
        ..
    } = shape.construction()
    else {
        return Ok(None);
    };
    let diameter = diameter.get();
    let counterbore_diameter = counterbore_diameter.get();

    let Some(primary) = cylindrical_surface_placements(ctx, diameter * 0.5, topology.surfaces)?
    else {
        return Ok(None);
    };
    let Some(counterbores) =
        cylindrical_surface_placements(ctx, counterbore_diameter * 0.5, topology.surfaces)?
    else {
        return Ok(None);
    };
    let mut primary_keys = HashSet::new();
    for key in primary.iter().filter_map(hole_axis_key) {
        ctx.charge_work(1, "index SLDPRT counterbore primary axes")?;
        ctx.insert_hash_set(
            &mut primary_keys,
            key,
            "index SLDPRT counterbore primary axes",
        )?;
    }
    let mut counterbore_keys = HashSet::new();
    for key in counterbores.iter().filter_map(hole_axis_key) {
        ctx.charge_work(1, "index SLDPRT counterbore outer axes")?;
        ctx.insert_hash_set(
            &mut counterbore_keys,
            key,
            "index SLDPRT counterbore outer axes",
        )?;
    }
    Ok((primary_keys.len() == primary.len()
        && counterbore_keys.len() == counterbores.len()
        && primary_keys == counterbore_keys)
        .then_some(primary))
}

fn same_hole_construction(
    ctx: &DecodeContext<'_>,
    left: &FeatureDefinition,
    right: &FeatureDefinition,
) -> Result<bool, CodecError> {
    let FeatureDefinition::Operation(FeatureOperation::Hole {
        shape,

        extent: left_extent,
        bottom: left_bottom,
        taper_angle: left_taper_angle,
        allow_multi_profile_faces: left_allow_multi_profile_faces,
        ..
    }) = left
    else {
        return Ok(false);
    };
    let left_construction = shape.construction();
    let left_exit_kind = shape.exit_kind();
    let left_diameter = shape.diameter();
    let FeatureDefinition::Operation(FeatureOperation::Hole {
        shape,

        extent: right_extent,
        bottom: right_bottom,
        taper_angle: right_taper_angle,
        allow_multi_profile_faces: right_allow_multi_profile_faces,
        ..
    }) = right
    else {
        return Ok(false);
    };
    let right_construction = shape.construction();
    let right_exit_kind = shape.exit_kind();
    let right_diameter = shape.diameter();
    Ok(ctx.equal(
        left_construction,
        right_construction,
        "compare SLDPRT hole construction",
    )? && left_exit_kind == right_exit_kind
        && left_diameter == right_diameter
        && ctx.equal(left_extent, right_extent, "compare SLDPRT hole termination")?
        && left_bottom == right_bottom
        && left_taper_angle == right_taper_angle
        && left_allow_multi_profile_faces == right_allow_multi_profile_faces)
}

fn hole_axis_key(placement: &HolePlacement) -> Option<[GridCoordinate; 6]> {
    const AXIS_QUANTUM: f64 = EPS_HOLE_POSITION;
    let quantize = |value: f64| GridCoordinate::new(value, AXIS_QUANTUM);
    let HolePlacement::Axis { origin, axis } = placement else {
        return None;
    };
    let axis = canonical_axis(axis.get());
    let station = Vector3::new(origin.x, origin.y, origin.z).dot(axis);
    let closest = Point3::new(
        origin.x - station * axis.x,
        origin.y - station * axis.y,
        origin.z - station * axis.z,
    );
    if !closest.is_finite() || !axis.is_finite() {
        return None;
    }
    Some([
        quantize(closest.x),
        quantize(closest.y),
        quantize(closest.z),
        quantize(axis.x),
        quantize(axis.y),
        quantize(axis.z),
    ])
}

fn cylindrical_support_normal(surface: &Surface, point: Point3) -> Option<Vector3> {
    let Some(SolvedSurfaceGeometry::Cylinder(cylinder_surface)) = surface.geometry.solved() else {
        return None;
    };
    let origin = cylinder_surface.origin().get();
    let axis = *cylinder_surface.frame().axis().as_raw();
    let radius = cylinder_surface.radius().get();
    let delta = Vector3::new(point.x - origin.x, point.y - origin.y, point.z - origin.z);
    let along = delta.dot(axis);
    let radial = Vector3::new(
        delta.x - along * axis.x,
        delta.y - along * axis.y,
        delta.z - along * axis.z,
    );
    let radial_length = radial.norm();
    let tolerance = (radius * EPS_HOLE_GEOMETRY).max(EPS_HOLE_GEOMETRY);
    ((radial_length - radius).abs() <= tolerance).then(|| {
        Vector3::new(
            radial.x / radial_length,
            radial.y / radial_length,
            radial.z / radial_length,
        )
    })
}

fn point_axis_distance_squared(point: Point3, origin: Point3, axis: Vector3) -> f64 {
    let delta = Vector3::new(point.x - origin.x, point.y - origin.y, point.z - origin.z);
    let along = delta.x * axis.x + delta.y * axis.y + delta.z * axis.z;
    let across = Vector3::new(
        delta.x - along * axis.x,
        delta.y - along * axis.y,
        delta.z - along * axis.z,
    );
    across.x * across.x + across.y * across.y + across.z * across.z
}

pub(crate) struct HoleTopology<'a> {
    pub(crate) surfaces: &'a [Surface],
    pub(crate) faces: &'a [Face],
    pub(crate) loops: &'a [Loop],
    pub(crate) coedges: &'a [Coedge],
    pub(crate) edges: &'a [Edge],
    pub(crate) vertices: &'a [Vertex],
    pub(crate) points: &'a [Point],
}

fn direct_hole_position_feature<'a, 's>(
    ctx: &DecodeContext<'_>,
    hole: &crate::records::Feature,
    histories: &'a [crate::records::FeatureHistory],
    sketch_for: impl Fn(&str) -> Result<Option<&'s SketchId>, CodecError>,
    sketch_entities: &[SketchEntity],
) -> Result<Option<&'a crate::records::Feature>, CodecError> {
    const OPERATION: &str = "resolve SLDPRT direct hole position";
    let mut history = None;
    for candidate in ctx.admit_iter(histories, "scan SLDPRT holes records")? {
        let contains_hole = {
            let mut search_result = false;
            for feature in ctx.admit_iter(&candidate.features, "scan SLDPRT holes records")? {
                if ctx.equal(&(feature.id), &(hole.id), "compare SLDPRT holes records")? {
                    search_result = true;
                    break;
                }
            }
            search_result
        };
        if contains_hole {
            history = Some(candidate);
            break;
        }
    }
    let Some(history) = history else {
        return Ok(None);
    };
    let mut direct_sketches: [Option<&crate::records::Feature>; 2] = [None, None];
    if let Some(children) = hole.properties.get("DissectableChildren") {
        for source in hole_child_tokens(ctx, children)? {
            let source = ctx.trim_text(source?, OPERATION)?;
            if source.is_empty() {
                continue;
            }
            let Some(child) = unique_hole_child_profile(ctx, history, source)? else {
                continue;
            };
            if classify(child) != Some(FeatureClass::Sketch) {
                continue;
            }
            let mut repeated = false;
            for previous in ctx.admit_iter(&direct_sketches, OPERATION)?.flatten() {
                if ctx.equal(&previous.id, &child.id, OPERATION)? {
                    repeated = true;
                    break;
                }
            }
            if repeated {
                continue;
            }
            if direct_sketches[0].is_none() {
                direct_sketches[0] = Some(child);
            } else if direct_sketches[1].is_none() {
                direct_sketches[1] = Some(child);
            } else {
                return Ok(None);
            }
        }
    }
    let is_axial_profile = |child: &crate::records::Feature| -> Result<bool, CodecError> {
        let Some(sketch) = sketch_for(&child.id)? else {
            return Ok(false);
        };
        Ok(profiled_hole_construction_with_evidence(
            ctx,
            child,
            sketch,
            sketch_entities,
            ProfileEvidence::AxialTopology,
        )?
        .is_some())
    };
    let adjacent_position = || -> Result<Option<(&'a crate::records::Feature, &'a crate::records::Feature)>, CodecError> {
        let (Some(position_ordinal), Some(profile_ordinal)) = (hole.ordinal.checked_add(1), hole.ordinal.checked_add(2)) else { return Ok(None); };
        let unique_at = |ordinal| -> Result<Option<&crate::records::Feature>, CodecError> {
            let mut selected = None;
            for candidate in ctx.admit_iter(&history.features, OPERATION)? {
                if candidate.ordinal == ordinal && classify(candidate) == Some(FeatureClass::Sketch)
                    && sketch_for(&candidate.id)?.is_some() {
                    if selected.is_some() {
                        return Ok(None);
                    }
                    selected = Some(candidate);
                }
            }
            Ok(selected)
        };
        let (Some(position), Some(profile)) = (unique_at(position_ordinal)?, unique_at(profile_ordinal)?) else { return Ok(None); };
        if !is_axial_profile(position)? && is_axial_profile(profile)? { Ok(Some((position, profile))) } else { Ok(None) }
    };
    Ok(match direct_sketches {
        [Some(first), Some(second)] => {
            match (is_axial_profile(first)?, is_axial_profile(second)?) {
                (true, false) => Some(second),
                (false, true) => Some(first),
                _ => None,
            }
        }
        [Some(profile), None] => match adjacent_position()? {
            Some((position, adjacent_profile))
                if ctx.equal(&adjacent_profile.id, &profile.id, OPERATION)? =>
            {
                Some(position)
            }
            _ => None,
        },
        [None, None] => adjacent_position()?.map(|(position, _)| position),
        _ => None,
    })
}

pub(crate) fn project_hole_axes(
    ctx: &DecodeContext<'_>,
    model_features: &mut [cadmpeg_ir::features::Feature],
    sketch_entities: &[SketchEntity],
    topology: &HoleTopology<'_>,
    histories: &[crate::records::FeatureHistory],
    lanes: &[FeatureInputLane],
) -> Result<(), CodecError> {
    const INDEX_OPERATION: &str = "index SLDPRT hole position features";

    let surfaces = topology.surfaces;
    let mut native_features = BTreeMap::new();
    for iteration_history in ctx.admit_iter(histories, "scan SLDPRT holes source records")? {
        for feature in ctx.admit_iter(&iteration_history.features, "scan SLDPRT holes records")? {
            ctx.insert_btree_map(
                &mut native_features,
                feature.id.as_str(),
                feature,
                INDEX_OPERATION,
            )?;
        }
    }
    let mut model_sketches_storage = ctx.reserve_scoped(0, INDEX_OPERATION)?;
    let mut model_sketches = HashMap::new();
    for feature in model_features.iter() {
        ctx.charge_work(1, INDEX_OPERATION)?;
        let FeatureDefinition::Operation(FeatureOperation::Sketch {
            sketch: cadmpeg_ir::features::SketchFeatureBinding::Planar(Some(sketch)),
        }) = feature.evaluation.definition()
        else {
            continue;
        };
        let Some(native) = &feature.native_ref else {
            continue;
        };
        model_sketches_storage.with_storage(|| {
            let native = ctx.format_retained(format_args!("{native}"), INDEX_OPERATION)?;
            let sketch = {
                let identity_text =
                    ctx.format_retained(format_args!("{sketch}"), INDEX_OPERATION)?;
                ctx.charge_work(
                    cadmpeg_core::decode::u64_from_index(identity_text.len()),
                    "validate SLDPRT holes identity",
                )?;
                SketchId::mint(identity_text)
            }
            .map_err(|_| CodecError::malformed("invalid admitted SLDPRT sketch identity"))?;
            ctx.insert_hash_map(
                &mut model_sketches,
                native,
                sketch,
                "resolve SLDPRT holes keys",
            )
        })?;
    }
    let mut hole_positions_storage = ctx.reserve_scoped(0, INDEX_OPERATION)?;
    let mut hole_positions = BTreeMap::new();
    for hole in native_features.values() {
        ctx.charge_work(1, INDEX_OPERATION)?;
        if classify(hole) != Some(FeatureClass::Hole) {
            continue;
        }
        let position = match hole_position_feature(ctx, hole, histories, lanes)? {
            Some(position) => Some(position),
            None => direct_hole_position_feature(
                ctx,
                hole,
                histories,
                |id| ctx.get_hash_map(&model_sketches, id, INDEX_OPERATION),
                sketch_entities,
            )?,
        };
        let Some(position) = position else {
            continue;
        };

        hole_positions_storage.with_storage(|| {
            ctx.insert_btree_map(
                &mut hole_positions,
                hole.id.as_str(),
                position,
                "resolve SLDPRT holes keys",
            )
        })?;
    }
    let mut position_features = HashSet::new();
    for position in hole_positions.values() {
        ctx.charge_work(1, INDEX_OPERATION)?;
        ctx.insert_hash_set(
            &mut position_features,
            position.id.as_str(),
            INDEX_OPERATION,
        )?;
    }
    let mut feature_ranges_storage = ctx.reserve_scoped(0, INDEX_OPERATION)?;
    let mut feature_ranges = HashMap::new();
    for lane in lanes {
        ctx.charge_work(1, INDEX_OPERATION)?;
        feature_ranges_storage.with_storage(|| {
            ctx.insert_hash_map(
                &mut feature_ranges,
                lane.id.as_str(),
                feature_object_byte_ranges(ctx, histories, lane)?,
                "resolve SLDPRT holes keys",
            )
        })?;
    }
    let mut feature_frames = HashMap::new();
    for lane in lanes {
        ctx.charge_work(1, INDEX_OPERATION)?;
        let Some(ranges) = ctx.get_hash_map(
            &feature_ranges,
            lane.id.as_str(),
            "resolve SLDPRT holes keys",
        )?
        else {
            continue;
        };
        let plane_frames = lane_sketch_plane_frames(ctx, model_features, histories, lane)?;
        let plane_index = CompactReferencePlaneIndex::new(ctx, &lane.native_payload)?;
        for feature in native_features.values() {
            ctx.charge_work(1, INDEX_OPERATION)?;
            if !ctx.contains_hash_set(
                &position_features,
                feature.id.as_str(),
                "resolve SLDPRT holes keys",
            )? {
                continue;
            }
            let Some(&range) =
                ctx.get_hash_map(ranges, feature.id.as_str(), "resolve SLDPRT holes keys")?
            else {
                continue;
            };
            let (context_start, start, end) = range;
            let Some(frame) = feature_input_sketch_frame(
                ctx,
                &lane.native_payload,
                &plane_frames,
                &plane_index,
                context_start,
                start,
                end,
            )?
            else {
                continue;
            };
            let key = (lane.id.as_str(), feature.id.as_str());
            ctx.insert_hash_map(&mut feature_frames, key, frame, INDEX_OPERATION)?;
        }
    }
    let mut hole_diameter_counts = HashMap::<u64, usize>::new();
    for feature in model_features.iter() {
        ctx.charge_work(1, INDEX_OPERATION)?;
        if feature.suppressed == Some(true) {
            continue;
        }
        let FeatureDefinition::Operation(FeatureOperation::Hole { shape, .. }) =
            feature.evaluation.definition()
        else {
            continue;
        };
        let Some(diameter) = shape.diameter() else {
            continue;
        };
        let key = diameter.get().to_bits();
        ctx.admit_hash_map_entry(&mut hole_diameter_counts, &key, INDEX_OPERATION)?;
        let count = hole_diameter_counts.entry(key).or_default();
        *count = count
            .checked_add(1)
            .ok_or_else(|| ctx.refuse_codec_limit(INDEX_OPERATION, u64::MAX - 1, u64::MAX))?;
    }

    for feature in model_features {
        ctx.charge_work(1, "project SLDPRT hole positions")?;
        if feature.suppressed == Some(true) {
            continue;
        }
        let FeatureDefinition::Operation(FeatureOperation::Hole {
            placements, shape, ..
        }) = feature.evaluation.definition()
        else {
            continue;
        };
        if placements.is_some() {
            continue;
        }
        let Some(diameter) = shape.diameter() else {
            continue;
        };
        let diameter = diameter.get();
        let radius = diameter / 2.0;
        let Some(native_feature) = (match feature.native_ref.as_deref() {
            Some(native) => ctx
                .get_btree_map(
                    &(native_features),
                    native,
                    "resolve SLDPRT holes references",
                )?
                .copied(),
            None => None,
        }) else {
            continue;
        };
        let Some(position_feature) = ctx
            .get_btree_map(
                &hole_positions,
                native_feature.id.as_str(),
                "resolve SLDPRT holes keys",
            )?
            .copied()
        else {
            continue;
        };
        let solution: Result<Option<Vec<HolePlacement>>, CodecError> = (|| {
            if hole_diameter_counts.get(&diameter.to_bits()) == Some(&1) {
                let mut first_frame: Option<(Point3, Vector3, Vector3)> = None;
                let mut same_frames = true;
                for lane in ctx.admit_iter(lanes, "scan SLDPRT hole position frames")? {
                    let Some(&candidate) = ctx.get_hash_map(
                        &feature_frames,
                        &(lane.id.as_str(), position_feature.id.as_str()),
                        "resolve SLDPRT hole position frames",
                    )?
                    else {
                        continue;
                    };
                    if let Some(frame) = first_frame {
                        if !(frame.1.dot(candidate.1).abs() >= 1.0 - EPS_HOLE_GEOMETRY
                            && Vector3::new(
                                candidate.0.x - frame.0.x,
                                candidate.0.y - frame.0.y,
                                candidate.0.z - frame.0.z,
                            )
                            .dot(frame.1)
                            .abs()
                                <= EPS_HOLE_POSITION)
                        {
                            same_frames = false;
                            break;
                        }
                    } else {
                        first_frame = Some(candidate);
                    }
                }
                if same_frames {
                    if let Some(frame) = first_frame {
                        if let Some(bore_placements) =
                            plane_owned_bore_placements(ctx, frame.0, frame.1, radius, topology)?
                        {
                            return Ok(Some(bore_placements));
                        }
                    }
                }
                if let Some(bore_placements) = bore_carrier_placements(ctx, radius, topology)? {
                    return Ok(Some(bore_placements));
                }
            }
            let mut solutions = Vec::new();
            for lane in lanes {
                ctx.charge_work(1, "scan SLDPRT hole position lanes")?;
                let Some(&frame) = ctx.get_hash_map(
                    &feature_frames,
                    &(lane.id.as_str(), position_feature.id.as_str()),
                    "resolve SLDPRT hole position frames",
                )?
                else {
                    continue;
                };
                let relations =
                    compact_position_relations(ctx, lane, position_feature.id.as_str())?;
                if relations.is_empty() {
                    continue;
                }
                if let Some(solution) =
                    constrained_bore_axes(ctx, frame, radius, surfaces, &relations)?
                {
                    ctx.reserve_vec(&mut solutions, 1, "collect SLDPRT hole position solutions")?;
                    solutions.push(solution);
                }
            }
            if solutions.is_empty() {
                for lane in lanes {
                    ctx.charge_work(1, "scan SLDPRT hole pattern lanes")?;
                    let temporary_axis = if let Some((_, start, end)) = match ctx.get_hash_map(
                        &feature_ranges,
                        lane.id.as_str(),
                        "resolve SLDPRT hole position ranges",
                    )? {
                        Some(ranges) => ctx.get_hash_map(
                            ranges,
                            position_feature.id.as_str(),
                            "resolve SLDPRT hole position ranges",
                        )?,
                        None => None,
                    } {
                        hole_temporary_axis(ctx, &lane.native_payload, *start, *end)?
                            .map(|(_, direction)| direction)
                    } else {
                        None
                    };
                    if let Some(solution) = marker_pattern_bore_axes(
                        ctx,
                        lane,
                        position_feature.id.as_str(),
                        radius,
                        surfaces,
                        temporary_axis,
                    )? {
                        ctx.reserve_vec(
                            &mut solutions,
                            1,
                            "collect SLDPRT hole position solutions",
                        )?;
                        solutions.push(solution);
                    }
                }
            }
            ctx.charge_work(
                u64_from_index(solutions.len()),
                "sort SLDPRT hole position solutions",
            )?;
            let placement_key = |placement: &HolePlacement| match placement {
                HolePlacement::Axis { origin, axis } => [
                    origin.x.to_bits(),
                    origin.y.to_bits(),
                    origin.z.to_bits(),
                    axis.x.to_bits(),
                    axis.y.to_bits(),
                    axis.z.to_bits(),
                ],
                HolePlacement::Directed { .. } => [0; 6],
            };
            ctx.sort_unstable_by(
                &mut solutions,
                |value| value,
                |left, right| {
                    left.iter()
                        .map(placement_key)
                        .cmp(right.iter().map(placement_key))
                },
                "sort SLDPRT hole position solutions",
            )?;
            ctx.dedup_vec(&mut solutions, "deduplicate SLDPRT hole position solutions")?;
            let mut solutions = solutions.into_iter();
            Ok(match (solutions.next(), solutions.next()) {
                (Some(solution), None) => Some(solution),
                _ => None,
            })
        })();
        if let Some(solution) = solution? {
            feature.evaluation.edit(|definition, _| {
                if let FeatureDefinition::Operation(FeatureOperation::Hole { placements, .. }) =
                    definition
                {
                    *placements = Some(solution);
                }
            });
        }
    }
    Ok(())
}

fn cylindrical_bore_axes(
    ctx: &DecodeContext<'_>,
    radius: f64,
    topology: &HoleTopology<'_>,
) -> Result<Vec<(Point3, FeatureDirection3)>, CodecError> {
    let mut surfaces_storage = ctx.reserve_scoped(0, "index SLDPRT bore carrier surfaces")?;
    let surfaces = surfaces_storage
        .with_storage(|| topology_index(ctx, topology.surfaces, |surface| &surface.id))?;
    let tolerance = (radius.abs() * EPS_HOLE_GEOMETRY).max(EPS_HOLE_GEOMETRY);
    let mut axes = Vec::new();
    for face in topology.faces {
        ctx.charge_work(1, "scan SLDPRT bore carrier faces")?;
        if face.sense != Sense::Reversed {
            continue;
        }
        let Some(surface) =
            ctx.get_hash_map(&surfaces, &face.surface, "resolve SLDPRT holes keys")?
        else {
            continue;
        };
        let SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cylinder(cylinder_surface)) =
            surface.geometry
        else {
            continue;
        };
        let origin = cylinder_surface.origin().get();
        let axis = FeatureDirection3::from(*cylinder_surface.frame().axis());
        let candidate = cylinder_surface.radius().get();
        if (candidate - radius).abs() <= tolerance {
            ctx.reserve_vec(&mut axes, 1, "collect SLDPRT bore carrier axes")?;
            axes.push((origin, axis));
        }
    }
    ctx.sort_unstable_by_key(
        &mut axes,
        |value| {
            let (left_origin, left_axis) = value;
            [
                left_origin.x.to_bits(),
                left_origin.y.to_bits(),
                left_origin.z.to_bits(),
                left_axis.x.to_bits(),
                left_axis.y.to_bits(),
                left_axis.z.to_bits(),
            ]
        },
        Ord::cmp,
        "sort SLDPRT bore carrier axes",
    )?;
    ctx.dedup_vec(&mut axes, "deduplicate SLDPRT bore carrier axes")?;
    Ok(axes)
}

fn plane_owned_bore_placements(
    ctx: &DecodeContext<'_>,
    plane_origin: Point3,
    plane_normal: Vector3,
    radius: f64,
    topology: &HoleTopology<'_>,
) -> Result<Option<Vec<HolePlacement>>, CodecError> {
    const AXIS_QUANTUM: f64 = EPS_HOLE_POSITION;
    const OPERATION: &str = "collect SLDPRT plane-owned bore axes";
    let quantize = |value: f64| GridCoordinate::new(value, AXIS_QUANTUM);
    let mut axes_storage = ctx.reserve_scoped(0, "collect SLDPRT bore carrier axes")?;
    let bore_axes = axes_storage.with_storage(|| cylindrical_bore_axes(ctx, radius, topology))?;
    let mut by_position = HashMap::new();
    for (origin, axis) in bore_axes {
        ctx.charge_work(1, "scan SLDPRT plane-owned bore axes")?;
        if axis.dot(plane_normal).abs() < 1.0 - EPS_HOLE_GEOMETRY {
            continue;
        }
        let station = Vector3::new(
            plane_origin.x - origin.x,
            plane_origin.y - origin.y,
            plane_origin.z - origin.z,
        )
        .dot(axis.get());
        let Some(origin) = FinitePoint3::new(Point3::new(
            origin.x + station * axis.x,
            origin.y + station * axis.y,
            origin.z + station * axis.z,
        )) else {
            return Ok(None);
        };
        let Some(axis) = FeatureDirection3::new(plane_normal) else {
            return Ok(None);
        };
        let key = [quantize(origin.x), quantize(origin.y), quantize(origin.z)];
        if !by_position.contains_key(&key) {
            ctx.reserve_map(&mut by_position, 1, OPERATION)?;
            by_position.insert(key, (origin, axis));
        }
    }
    let mut placements = Vec::new();
    ctx.reserve_vec(&mut placements, by_position.len(), OPERATION)?;
    placements.extend(by_position);
    ctx.sort_unstable_by(
        &mut placements,
        |value| &value.0,
        Ord::cmp,
        "sort SLDPRT plane-owned bore axes",
    )?;
    let mut axes = Vec::new();
    ctx.reserve_vec(&mut axes, placements.len(), OPERATION)?;
    axes.extend(
        placements
            .into_iter()
            .map(|(_, (origin, axis))| HolePlacement::Axis { origin, axis }),
    );
    Ok((!axes.is_empty()).then_some(axes))
}

fn bore_carrier_placements(
    ctx: &DecodeContext<'_>,
    radius: f64,
    topology: &HoleTopology<'_>,
) -> Result<Option<Vec<HolePlacement>>, CodecError> {
    let mut axes_storage = ctx.reserve_scoped(0, "collect SLDPRT bore carrier axes")?;
    let axes = axes_storage.with_storage(|| cylindrical_bore_axes(ctx, radius, topology))?;
    carrier_placements(
        ctx,
        ctx.admit_iter(&axes[..], "scan SLDPRT hole carrier axes")?
            .copied(),
    )
}

fn cylindrical_surface_placements(
    ctx: &DecodeContext<'_>,
    radius: f64,
    surfaces: &[Surface],
) -> Result<Option<Vec<HolePlacement>>, CodecError> {
    let tolerance = (radius.abs() * EPS_HOLE_GEOMETRY).max(EPS_HOLE_GEOMETRY);
    carrier_placements(
        ctx,
        ctx.admit_iter(surfaces, "scan SLDPRT hole carrier axes")?
            .filter_map(|surface| {
                let Some(SolvedSurfaceGeometry::Cylinder(cylinder_surface)) =
                    surface.geometry.solved()
                else {
                    return None;
                };
                let origin = cylinder_surface.origin().get();
                let axis = FeatureDirection3::from(*cylinder_surface.frame().axis());
                let candidate = cylinder_surface.radius().get();
                ((candidate - radius).abs() <= tolerance).then_some((origin, axis))
            }),
    )
}

fn carrier_placements(
    ctx: &DecodeContext<'_>,
    axes: impl IntoIterator<Item = (Point3, FeatureDirection3)>,
) -> Result<Option<Vec<HolePlacement>>, CodecError> {
    const AXIS_QUANTUM: f64 = EPS_HOLE_POSITION;
    const OPERATION: &str = "collect SLDPRT hole carrier axes";
    let quantize = |value: f64| GridCoordinate::new(value, AXIS_QUANTUM);
    let mut by_axis = HashMap::new();
    for (origin, axis) in axes {
        let axis = canonical_direction(axis);
        let station = Vector3::new(origin.x, origin.y, origin.z).dot(axis.get());
        let closest = Point3::new(
            origin.x - station * axis.x,
            origin.y - station * axis.y,
            origin.z - station * axis.z,
        );
        let Some(origin) = FinitePoint3::new(closest) else {
            return Ok(None);
        };
        let key = [
            quantize(closest.x),
            quantize(closest.y),
            quantize(closest.z),
            quantize(axis.x),
            quantize(axis.y),
            quantize(axis.z),
        ];
        ctx.insert_hash_map(
            &mut by_axis,
            key,
            HolePlacement::Axis { origin, axis },
            OPERATION,
        )?;
    }
    let mut carriers = Vec::new();
    ctx.reserve_vec(&mut carriers, by_axis.len(), OPERATION)?;
    carriers.extend(by_axis);
    ctx.sort_unstable_by(
        &mut carriers,
        |value| &value.0,
        Ord::cmp,
        "sort SLDPRT hole carrier axes",
    )?;
    let mut placements = Vec::new();
    ctx.reserve_vec(&mut placements, carriers.len(), OPERATION)?;
    placements.extend(carriers.into_iter().map(|(_, placement)| placement));
    Ok((!placements.is_empty()).then_some(placements))
}

fn topology_index<'a, T, K: Eq + std::hash::Hash + cadmpeg_core::decode::cost::DecodeCost>(
    ctx: &DecodeContext<'_>,
    records: &'a [T],
    key: impl Fn(&'a T) -> &'a K,
) -> Result<HashMap<&'a K, &'a T>, CodecError> {
    const OPERATION: &str = "index SLDPRT hole topology records";
    let mut index = HashMap::new();
    for record in ctx.admit_iter(records, OPERATION)? {
        ctx.insert_hash_map(&mut index, key(record), record, OPERATION)?;
    }
    Ok(index)
}

#[derive(Debug)]
struct BoreFaceSpan(Point3, FeatureDirection3, f64, f64, bool);

fn cylindrical_bore_face_spans(
    ctx: &DecodeContext<'_>,
    topology: &HoleTopology<'_>,
) -> Result<Vec<BoreFaceSpan>, CodecError> {
    let mut index_storage = ctx.reserve_scoped(0, "index SLDPRT hole topology records")?;
    let surfaces = index_storage
        .with_storage(|| topology_index(ctx, topology.surfaces, |surface| &surface.id))?;
    let loops =
        index_storage.with_storage(|| topology_index(ctx, topology.loops, |loop_| &loop_.id))?;
    let coedges = index_storage
        .with_storage(|| topology_index(ctx, topology.coedges, |coedge| &coedge.id))?;
    let edges =
        index_storage.with_storage(|| topology_index(ctx, topology.edges, |edge| &edge.id))?;
    let vertices = index_storage
        .with_storage(|| topology_index(ctx, topology.vertices, |vertex| &vertex.id))?;
    let points =
        index_storage.with_storage(|| topology_index(ctx, topology.points, |point| &point.id))?;
    let mut spans = Vec::new();
    for face in topology.faces {
        ctx.charge_work(1, "scan SLDPRT hole bore faces")?;
        let Some(surface) =
            ctx.get_hash_map(&surfaces, &face.surface, "resolve SLDPRT holes keys")?
        else {
            continue;
        };
        let Some(SolvedSurfaceGeometry::Cylinder(cylinder_surface)) = surface.geometry.solved()
        else {
            continue;
        };
        let origin = cylinder_surface.origin().get();
        let axis = FeatureDirection3::from(*cylinder_surface.frame().axis());
        let radius = cylinder_surface.radius().get();
        let (outer, inner): (&[cadmpeg_ir::ids::LoopId], &[cadmpeg_ir::ids::LoopId]) =
            match &face.loops {
                cadmpeg_ir::topology::FaceLoops::Unspecified { loops } => (&[], loops.as_slice()),
                cadmpeg_ir::topology::FaceLoops::Classified { outer, inner } => {
                    (std::slice::from_ref(outer), inner.as_slice())
                }
            };
        let mut station_bounds = None;
        let mut observe_vertex = |vertex_id: &cadmpeg_ir::ids::VertexId| -> Result<(), CodecError> {
            let Some(vertex) =
                ctx.get_hash_map(&vertices, vertex_id, "resolve SLDPRT holes keys")?
            else {
                return Ok(());
            };
            let Some(point) =
                ctx.get_hash_map(&points, &vertex.point, "resolve SLDPRT holes keys")?
            else {
                return Ok(());
            };
            let station = Vector3::new(
                point.position().get().x - origin.x,
                point.position().get().y - origin.y,
                point.position().get().z - origin.z,
            )
            .dot(axis.get());
            station_bounds = Some(match station_bounds {
                Some((minimum, maximum)) => {
                    (f64::min(minimum, station), f64::max(maximum, station))
                }
                None => (station, station),
            });
            Ok(())
        };
        for loop_id in ctx
            .admit_iter(outer, "scan SLDPRT hole bore loops")?
            .chain(ctx.admit_iter(inner, "scan SLDPRT hole bore loops")?)
        {
            let Some(loop_) = ctx.get_hash_map(&loops, loop_id, "resolve SLDPRT holes keys")?
            else {
                continue;
            };
            for coedge_id in ctx.admit_iter(loop_.coedges(), "scan SLDPRT hole bore coedges")? {
                let Some(coedge) =
                    ctx.get_hash_map(&coedges, coedge_id, "resolve SLDPRT holes keys")?
                else {
                    continue;
                };
                let Some(edge) =
                    ctx.get_hash_map(&edges, &coedge.edge, "resolve SLDPRT holes keys")?
                else {
                    continue;
                };
                for vertex_id in [&edge.start, &edge.end] {
                    observe_vertex(vertex_id)?;
                }
            }
            if let Some((vertex, _)) = loop_.singular_vertex() {
                observe_vertex(vertex)?;
            }
            for vertex_use in ctx.admit_iter(
                loop_.anchored_vertex_uses(),
                "scan SLDPRT hole bore vertices",
            )? {
                observe_vertex(&vertex_use.vertex)?;
            }
        }
        let Some((minimum, maximum)) = station_bounds else {
            continue;
        };
        let span = maximum - minimum;
        if span.is_finite() && span > 0.0 {
            ctx.reserve_vec(&mut spans, 1, "collect SLDPRT hole bore spans")?;
            spans.push(BoreFaceSpan(
                origin,
                axis,
                radius,
                span,
                face.sense == Sense::Reversed,
            ));
        }
    }
    Ok(spans)
}

pub(crate) fn project_topological_hole_constructions(
    ctx: &DecodeContext<'_>,
    features: &mut [cadmpeg_ir::features::Feature],
    topology: &HoleTopology<'_>,
) -> Result<(), cadmpeg_core::CodecError> {
    let mut bore_faces_storage = ctx.reserve_scoped(0, "collect SLDPRT hole bore spans")?;
    let bore_faces =
        bore_faces_storage.with_storage(|| cylindrical_bore_face_spans(ctx, topology))?;
    for feature_index in ctx.admit_iter(
        &(0..features.len()),
        "project SLDPRT topological hole constructions",
    )? {
        let feature = &mut features[feature_index];
        let mut result = Ok(());
        feature.evaluation.edit(|definition, _| {
            result = (|| -> Result<(), CodecError> {
                let FeatureDefinition::Operation(FeatureOperation::Hole {
                    placements,
                    shape,
                    extent,
                    ..
                }) = definition
                else {
                    return Ok(());
                };
                let mut diameter = shape.diameter();
                let Some(placements) = placements.as_deref() else {
                    return Ok(());
                };
                if placements.is_empty()
                    || (diameter.is_some()
                        && extent.as_ref().is_some_and(|extent| {
                            !matches!(extent, LinearTermination::Unresolved {})
                        }))
                {
                    return Ok(());
                }
                let mut common = None::<Vec<(f64, f64)>>;
                for placement in placements {
                    ctx.charge_work(1, "scan SLDPRT hole placements")?;
                    let HolePlacement::Axis {
                        origin: placement_origin,
                        axis: placement_axis,
                    } = placement
                    else {
                        common = Some(Vec::new());
                        break;
                    };
                    let mut candidates = Vec::new();
                    for BoreFaceSpan(origin, axis, radius, span, reversed) in &bore_faces {
                        ctx.charge_work(1, "match SLDPRT hole bore faces")?;
                        if !reversed {
                            continue;
                        }
                        let parallel =
                            axis.dot(placement_axis.get()).abs() >= 1.0 - EPS_HOLE_GEOMETRY;
                        let distance = point_axis_distance_squared(
                            placement_origin.get(),
                            *origin,
                            axis.get(),
                        );
                        if parallel && distance <= EPS_HOLE_EXACT_GEOMETRY {
                            ctx.reserve_vec(
                                &mut candidates,
                                1,
                                "collect SLDPRT matching hole bores",
                            )?;
                            candidates.push((*radius, *span));
                        }
                    }
                    ctx.sort_unstable_by_key(
                        &mut candidates,
                        |value| (value.0, value.1),
                        |left, right| {
                            left.0
                                .total_cmp(&right.0)
                                .then_with(|| left.1.total_cmp(&right.1))
                        },
                        "sort SLDPRT matching hole bores",
                    )?;
                    ctx.charge_work(
                        u64_from_index(candidates.len()),
                        "deduplicate SLDPRT matching hole bores",
                    )?;
                    candidates.dedup_by(|left, right| {
                        (left.0 - right.0).abs() <= EPS_HOLE_GEOMETRY
                            && (left.1 - right.1).abs() <= EPS_HOLE_GEOMETRY
                    });
                    common = Some(match common {
                        None => candidates,
                        Some(previous) => {
                            let mut shared = Vec::new();
                            for candidate in ctx
                                .admit_iter(&previous, "scan SLDPRT common hole bores")?
                                .copied()
                            {
                                ctx.charge_work(
                                    u64_from_index(candidates.len()),
                                    "intersect SLDPRT hole bores",
                                )?;
                                if candidates.iter().any(|other| {
                                    (candidate.0 - other.0).abs() <= EPS_HOLE_GEOMETRY
                                        && (candidate.1 - other.1).abs() <= EPS_HOLE_GEOMETRY
                                }) {
                                    ctx.reserve_vec(
                                        &mut shared,
                                        1,
                                        "collect SLDPRT common hole bores",
                                    )?;
                                    shared.push(candidate);
                                }
                            }
                            shared
                        }
                    });
                }
                let Some([(radius, depth)]) = common.as_deref() else {
                    return Ok(());
                };
                if diameter.is_none() {
                    diameter = Some(
                        cadmpeg_ir::scalar::PositiveLength::new(radius * 2.0).ok_or_else(|| {
                            cadmpeg_core::CodecError::Malformed(
                                "SolidWorks projected length must be finite".into(),
                            )
                        })?,
                    );
                }
                let new_extent = if extent
                    .as_ref()
                    .is_none_or(|extent| matches!(extent, LinearTermination::Unresolved {}))
                {
                    Some(LinearTermination::Blind {
                        length: cadmpeg_ir::scalar::NonZeroLength::new(*depth).ok_or_else(
                            || {
                                cadmpeg_core::CodecError::Malformed(
                                    "SolidWorks projected length must be finite".into(),
                                )
                            },
                        )?,
                    })
                } else {
                    None
                };
                shape
                    .try_set_diameter(diameter)
                    .map_err(cadmpeg_core::CodecError::malformed)?;
                if let Some(new_extent) = new_extent {
                    *extent = Some(new_extent);
                }
                Ok(())
            })();
        });
        result?;
    }

    Ok(())
}

pub(crate) fn project_bore_backed_position_sketches(
    ctx: &DecodeContext<'_>,
    features: &mut [cadmpeg_ir::features::Feature],
    sketches: &mut Vec<Sketch>,
    entities: &mut Vec<SketchEntity>,
    surfaces: &[Surface],
    histories: &[crate::records::FeatureHistory],
    lanes: &[FeatureInputLane],
) -> Result<(), CodecError> {
    const OPERATION: &str = "project SLDPRT bore backed position sketches";
    struct Projection {
        feature: usize,
        sketch: Sketch,
        entities: Vec<SketchEntity>,
    }
    let copy_text = |text: &str| -> Result<String, CodecError> {
        ctx.charge_work(u64_from_index(text.len()), OPERATION)?;
        ctx.format_retained(format_args!("{text}"), OPERATION)
    };
    let mut lookup_storage = ctx.reserve_scoped(
        0,
        "SLDPRT project_bore_backed_position_sketches lookup storage",
    )?;
    let mut native_features = BTreeMap::new();
    for iteration_history in ctx.admit_iter(histories, "scan SLDPRT holes source records")? {
        for feature in ctx.admit_iter(&iteration_history.features, "scan SLDPRT holes records")? {
            lookup_storage.with_storage(|| {
                ctx.insert_btree_map(
                    &mut native_features,
                    feature.id.as_str(),
                    feature,
                    OPERATION,
                )
            })?;
        }
    }
    let mut model_features = HashMap::new();
    for feature in features.iter() {
        ctx.charge_work(1, OPERATION)?;
        let Some(native) = feature.native_ref.as_deref() else {
            continue;
        };
        lookup_storage.with_storage(|| {
            ctx.insert_hash_map(&mut model_features, native, &feature.id, OPERATION)
        })?;
    }
    let mut projections = Vec::new();
    for hole in features.iter() {
        ctx.charge_work(1, OPERATION)?;
        let FeatureDefinition::Operation(FeatureOperation::Hole {
            placements: Some(placements),
            ..
        }) = hole.evaluation.definition()
        else {
            continue;
        };
        let axes = || {
            Ok::<_, CodecError>(
                ctx.admit_iter(placements, OPERATION)?
                    .filter_map(|placement| match placement {
                        HolePlacement::Axis { origin, axis } => Some((*origin, *axis)),
                        HolePlacement::Directed { .. } => None,
                    }),
            )
        };
        let Some((_, first_axis)) = axes()?.next() else {
            continue;
        };
        if axes()?.count() != placements.len() {
            continue;
        }
        let Some(native_hole) = (match hole.native_ref.as_deref() {
            Some(native) => ctx
                .get_btree_map(
                    &(native_features),
                    native,
                    "resolve SLDPRT holes references",
                )?
                .copied(),
            None => None,
        }) else {
            continue;
        };
        let Some(position) = hole_position_feature(ctx, native_hole, histories, lanes)? else {
            continue;
        };
        let Some(position_feature) = ctx.get_hash_map(
            &model_features,
            position.id.as_str(),
            "resolve SLDPRT holes keys",
        )?
        else {
            continue;
        };
        let mut feature_index = None;
        for (index, feature) in ctx
            .admit_iter(&features[..], "scan SLDPRT holes records")?
            .enumerate()
        {
            if ctx.equal(&feature.id, *position_feature, OPERATION)? {
                feature_index = Some(index);
                break;
            }
        }
        let Some(feature_index) = feature_index else {
            continue;
        };
        let model_position = &features[feature_index];
        if !matches!(
            model_position.evaluation.definition(),
            FeatureDefinition::Operation(FeatureOperation::Sketch {
                sketch: cadmpeg_ir::features::SketchFeatureBinding::Unresolved
                    | cadmpeg_ir::features::SketchFeatureBinding::Planar(None)
            })
        ) {
            continue;
        }
        let canonical = canonical_axis(first_axis.get());
        if !axes()?
            .all(|(_, axis)| canonical_axis(axis.get()).dot(canonical) >= 1.0 - EPS_HOLE_GEOMETRY)
        {
            continue;
        }
        let mut selected_frame: Option<(Point3, Vector3, Vector3)> = None;
        let mut ambiguous_frame = false;
        for surface in ctx.admit_iter(surfaces, OPERATION)? {
            let SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(plane)) = surface.geometry
            else {
                continue;
            };
            let origin = plane.origin().get();
            let normal = *plane.frame().axis().as_raw();
            if !matches!(
                normal
                    .dot(canonical)
                    .abs()
                    .partial_cmp(&(1.0 - EPS_HOLE_GEOMETRY)),
                Some(std::cmp::Ordering::Greater | std::cmp::Ordering::Equal)
            ) {
                continue;
            }
            if !axes()?.all(|(point, _)| {
                Vector3::new(point.x - origin.x, point.y - origin.y, point.z - origin.z)
                    .dot(normal)
                    .abs()
                    <= EPS_HOLE_POSITION
            }) {
                continue;
            }
            let candidate = (origin, normal, *plane.frame().reference().as_raw());
            if let Some(frame) = selected_frame {
                if !ctx.equal(
                    &reference_plane_frame_key(&candidate),
                    &reference_plane_frame_key(&frame),
                    OPERATION,
                )? {
                    ambiguous_frame = true;
                    break;
                }
            } else {
                selected_frame = Some(candidate);
            }
        }
        let Some(frame) = selected_frame else {
            continue;
        };
        if ambiguous_frame {
            continue;
        }
        let (origin, normal, u_axis) = frame;
        for lane in ctx.admit_iter(lanes, "scan SLDPRT holes records")? {
            ctx.charge_work(
                u64_from_index(lane.names.len())
                    .checked_add(128)
                    .ok_or_else(|| ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX))?,
                OPERATION,
            )?;
            for name in &lane.names {
                ctx.charge_work(u64_from_index(name.value.len()), OPERATION)?;
            }
        }
        let mut owning_lane: Option<&FeatureInputLane> = None;
        let mut ambiguous = false;
        for candidate in ctx.admit_iter(lanes, OPERATION)? {
            if hole_position_sketch_source(ctx, native_hole, candidate)? != position.source_value()
            {
                continue;
            }
            if let Some(lane) = owning_lane {
                if !ctx.equal(candidate.id.as_str(), lane.id.as_str(), OPERATION)? {
                    ambiguous = true;
                    break;
                }
            } else {
                owning_lane = Some(candidate);
            }
        }
        let Some(lane) = owning_lane else {
            continue;
        };
        if ambiguous {
            continue;
        }
        ctx.charge_work(u64_from_index(lane.id.len()), OPERATION)?;
        let lane_key = lane
            .id
            .rsplit_once('#')
            .map_or(lane.id.as_str(), |(_, key)| key);
        let Ok(sketch_id) = ({
            let identity_text = ctx.format_retained(
                format_args!("sldprt:model:sketch#bore:{lane_key}:{}", position.ordinal),
                OPERATION,
            )?;
            ctx.charge_work(
                cadmpeg_core::decode::u64_from_index(identity_text.len()),
                "validate SLDPRT holes identity",
            )?;
            SketchId::mint(identity_text)
        }) else {
            continue;
        };
        let v_axis = normal.cross(u_axis);
        let mut projected_entities = Vec::new();
        let mut admitted_geometry = true;
        for (ordinal, (point, _)) in axes()?.enumerate() {
            ctx.charge_work(u64_from_index(sketch_id.as_str().len()), OPERATION)?;
            let Ok(entity_id) = ({
                let identity_text = ctx.format_retained(
                    format_args!("{}:entity:{ordinal}", sketch_id.as_str()),
                    OPERATION,
                )?;
                ctx.charge_work(
                    cadmpeg_core::decode::u64_from_index(identity_text.len()),
                    "validate SLDPRT holes identity",
                )?;
                SketchEntityId::mint(identity_text)
            }) else {
                admitted_geometry = false;
                break;
            };
            let delta = Vector3::new(point.x - origin.x, point.y - origin.y, point.z - origin.z);
            let Ok(geometry) = SketchGeometry::try_from(SketchGeometryDefinition::Point {
                position: Point2::new(delta.dot(u_axis), delta.dot(v_axis)),
            }) else {
                admitted_geometry = false;
                break;
            };
            let sketch_ref = {
                let identity_text = copy_text(sketch_id.as_str())?;
                ctx.charge_work(
                    cadmpeg_core::decode::u64_from_index(identity_text.len()),
                    "validate SLDPRT holes identity",
                )?;
                SketchId::mint(identity_text)
            }
            .map_err(|_| CodecError::malformed("invalid admitted SLDPRT sketch identity"))?;
            lookup_storage.with_storage(|| {
                ctx.push_vec(
                    &mut projected_entities,
                    SketchEntity::new(entity_id, sketch_ref, geometry),
                    OPERATION,
                )
            })?;
        }
        if !admitted_geometry {
            continue;
        }
        let Ok(placement) =
            cadmpeg_ir::sketches::SketchPlacement::try_resolved(origin, normal, u_axis)
        else {
            continue;
        };
        let name = model_position.name.as_deref().map(copy_text).transpose()?;
        let configuration = lane.configuration.as_deref().map(copy_text).transpose()?;
        let native_ref = Some(copy_text(&lane.id)?);
        lookup_storage.with_storage(|| {
            ctx.push_vec(
                &mut projections,
                Projection {
                    feature: feature_index,
                    sketch: Sketch {
                        id: sketch_id,
                        name,
                        configuration,
                        visible: None,
                        placement,
                        profiles: cadmpeg_ir::sketches::SketchProfiles::default(),
                        native_ref,
                    },
                    entities: projected_entities,
                },
                OPERATION,
            )
        })?;
    }
    drop(model_features);
    for projection in projections {
        ctx.charge_work(1, OPERATION)?;
        let feature = &mut features[projection.feature];
        let FeatureDefinition::Operation(FeatureOperation::Sketch { sketch, .. }) =
            feature.evaluation.definition()
        else {
            continue;
        };
        if sketch.id().is_some() {
            continue;
        }
        let sketch_id = {
            let identity_text = copy_text(projection.sketch.id.as_str())?;
            ctx.charge_work(
                cadmpeg_core::decode::u64_from_index(identity_text.len()),
                "validate SLDPRT holes identity",
            )?;
            SketchId::mint(identity_text)
        }
        .map_err(|_| CodecError::malformed("invalid admitted SLDPRT sketch identity"))?;
        ctx.reserve_vec(sketches, 1, OPERATION)?;
        feature.evaluation.edit(|definition, _| {
            if let FeatureDefinition::Operation(FeatureOperation::Sketch { sketch, .. }) =
                definition
            {
                *sketch = cadmpeg_ir::features::SketchFeatureBinding::Planar(Some(sketch_id));
            }
        });
        ctx.extend_vec(entities, projection.entities, OPERATION)?;
        sketches.push(projection.sketch);
    }
    Ok(())
}

fn marker_pattern_bore_axes(
    ctx: &DecodeContext<'_>,
    lane: &FeatureInputLane,
    feature: &str,
    radius: f64,
    surfaces: &[Surface],
    direction: Option<Vector3>,
) -> Result<Option<Vec<HolePlacement>>, CodecError> {
    const OPERATION: &str = "collect SLDPRT marker bore patterns";
    let mut paired_marker_ids = HashSet::new();
    let mut reduced_marker_ids = HashSet::new();
    paired_object_locus_markers(ctx, lane, feature, |paired, [paired_u, paired_v]| {
        ctx.insert_hash_set(&mut paired_marker_ids, paired.id(), OPERATION)?;
        let reduced = if paired.kind() == SketchInputKind::Point {
            let mut same_locus = false;
            for candidate in ctx.admit_iter(&lane.sketch_entities, OPERATION)? {
                if !ctx.equal(candidate.id(), paired.id(), OPERATION)?
                    && ctx.equal(&candidate.feature_ref.as_deref(), &Some(feature), OPERATION)?
                    && candidate.object_index().is_some()
                    && candidate.coordinates_m.is_some_and(|coordinates| {
                        let [u, v] = coordinates.get();
                        same_dimension_length(paired_u * 1000.0, u * 1000.0)
                            && same_dimension_length(paired_v * 1000.0, v * 1000.0)
                    })
                {
                    same_locus = true;
                    break;
                }
            }
            !same_locus
        } else {
            true
        };
        if reduced {
            ctx.insert_hash_set(&mut reduced_marker_ids, paired.id(), OPERATION)?;
        }
        Ok(())
    })?;
    let marker_loci = |paired: &HashSet<&str>| -> Result<Vec<Point2>, CodecError> {
        let mut loci = Vec::new();
        for marker in &lane.sketch_entities {
            ctx.charge_work(1, OPERATION)?;
            if !ctx.equal(
                &(marker.feature_ref.as_deref()),
                &(Some(feature)),
                "compare SLDPRT holes records",
            )? || marker.object_index().is_none()
                || !(matches!(
                    marker.kind(),
                    SketchInputKind::LineOrCircle | SketchInputKind::Arc
                ) || ctx.contains_hash_set(
                    paired,
                    marker.id(),
                    "resolve SLDPRT holes keys",
                )?)
            {
                continue;
            }
            let Some(coordinates) = marker.coordinates_m else {
                continue;
            };
            let [u, v] = coordinates.get();
            ctx.reserve_vec(&mut loci, 1, OPERATION)?;
            loci.push(Point2::new(u * 1000.0, v * 1000.0));
        }
        ctx.sort_unstable_by_key(
            &mut loci,
            |value| (value.u, value.v),
            |left, right| {
                left.0
                    .total_cmp(&right.0)
                    .then_with(|| left.1.total_cmp(&right.1))
            },
            OPERATION,
        )?;
        ctx.charge_work(u64_from_index(loci.len()), OPERATION)?;
        loci.dedup_by(|left, right| {
            same_dimension_length(left.u, right.u) && same_dimension_length(left.v, right.v)
        });
        Ok(loci)
    };
    let curve_loci = marker_loci(&HashSet::new())?;
    let complete_loci = marker_loci(&paired_marker_ids)?;
    let reduced_loci = marker_loci(&reduced_marker_ids)?;
    if let Some(solution) =
        match_marker_loci_to_bore_axes(ctx, &curve_loci, radius, surfaces, direction)?
    {
        return Ok(Some(solution));
    }
    if let Some(solution) =
        match_marker_loci_to_bore_axes(ctx, &complete_loci, radius, surfaces, direction)?
    {
        return Ok(Some(solution));
    }
    if ctx.equal(&reduced_loci, &complete_loci, OPERATION)? {
        return Ok(None);
    }
    match_marker_loci_to_bore_axes(ctx, &reduced_loci, radius, surfaces, direction)
}

fn match_marker_loci_to_bore_axes(
    ctx: &DecodeContext<'_>,
    marker_loci: &[Point2],
    radius: f64,
    surfaces: &[Surface],
    direction: Option<Vector3>,
) -> Result<Option<Vec<HolePlacement>>, CodecError> {
    const QUANTUM: f64 = EPS_HOLE_POSITION;
    const OPERATION: &str = "match SLDPRT marker loci to bore axes";
    if marker_loci.is_empty() {
        return Ok(None);
    }
    let radius_tolerance = (radius.abs() * EPS_HOLE_GEOMETRY).max(EPS_HOLE_GEOMETRY);
    let quantize_scalar = |value: f64| GridCoordinate::new(value, QUANTUM);
    let mut grouped = BTreeMap::<
        [GridCoordinate; 3],
        BTreeMap<[GridCoordinate; 3], Vec<(FinitePoint3, FeatureDirection3)>>,
    >::new();
    for surface in surfaces {
        ctx.charge_work(1, OPERATION)?;
        let Some(SolvedSurfaceGeometry::Cylinder(cylinder_surface)) = surface.geometry.solved()
        else {
            continue;
        };
        let origin = cylinder_surface.origin();
        let axis = FeatureDirection3::from(*cylinder_surface.frame().axis());
        let candidate = cylinder_surface.radius().get();
        if (candidate - radius).abs() > radius_tolerance {
            continue;
        }
        let canonical = canonical_axis(axis.get());
        let closest_distance = Vector3::new(origin.x, origin.y, origin.z).dot(canonical);
        let closest = Point3::new(
            origin.x - closest_distance * canonical.x,
            origin.y - closest_distance * canonical.y,
            origin.z - closest_distance * canonical.z,
        );
        if !closest.is_finite() || !canonical.is_finite() {
            continue;
        }
        let axis_key = [
            quantize_scalar(canonical.x),
            quantize_scalar(canonical.y),
            quantize_scalar(canonical.z),
        ];
        let point_key = [
            quantize_scalar(closest.x),
            quantize_scalar(closest.y),
            quantize_scalar(closest.z),
        ];
        let lines = ctx
            .entry_btree_map(&mut grouped, axis_key, OPERATION)?
            .or_default();
        ctx.push_btree_group(lines, point_key, (origin, axis), OPERATION, OPERATION)?;
    }
    let mut solutions = BTreeMap::new();
    'lines: for (_, lines) in ctx.admit_iter(&grouped, OPERATION)? {
        let mut candidates = Vec::new();
        for (point, origins) in ctx.admit_iter(lines, OPERATION)? {
            let compare_origins =
                |left: &&(FinitePoint3, FeatureDirection3),
                 right: &&(FinitePoint3, FeatureDirection3)| {
                    left.0
                        .x
                        .total_cmp(&right.0.x)
                        .then_with(|| left.0.y.total_cmp(&right.0.y))
                        .then_with(|| left.0.z.total_cmp(&right.0.z))
                };
            let candidate = match direction {
                Some(expected) => ctx
                    .admit_iter(origins, OPERATION)?
                    .filter(|(_, axis)| expected.dot(axis.get()) >= 1.0 - EPS_HOLE_GEOMETRY)
                    .min_by(compare_origins),
                None => ctx.admit_iter(origins, OPERATION)?.min_by(compare_origins),
            };
            let Some((origin, axis)) = candidate else {
                continue;
            };
            let axis = direction.map_or_else(|| canonical_direction(*axis), |_| *axis);
            ctx.reserve_vec(&mut candidates, 1, OPERATION)?;
            candidates.push((*point, *origin, axis));
        }
        ctx.sort_unstable_by(&mut candidates, |value| &value.0, Ord::cmp, OPERATION)?;
        let mut candidate_loci = Vec::new();
        ctx.reserve_vec(&mut candidate_loci, candidates.len(), OPERATION)?;
        for ([x, y, z], ..) in &candidates {
            ctx.charge_work(1, OPERATION)?;
            let (Some(x), Some(y), Some(z)) = (
                x.coordinate(QUANTUM),
                y.coordinate(QUANTUM),
                z.coordinate(QUANTUM),
            ) else {
                continue 'lines;
            };
            candidate_loci.push(Point3::new(x, y, z));
        }
        if candidates.len() < marker_loci.len() {
            if !has_unique_marker_loci_subset(ctx, marker_loci, &candidate_loci)? {
                continue;
            }
            let indices = ctx.admit_iter(
                &(0..candidates.len()),
                "retain SLDPRT bore pattern solution",
            )?;
            if retain_bore_solution(ctx, &candidates, indices, &mut solutions)? {
                return Ok(None);
            }
            continue;
        }
        let mut subsets = BTreeSet::new();
        if (BoreSubsetSearch {
            ctx,
            marker_loci,
            candidate_loci: &candidate_loci,
            reverse: false,
        })
        .collect(0, &mut Vec::new(), &mut HashSet::new(), &mut subsets)?
        {
            return Ok(None);
        }
        for subset in ctx.admit_iter(&subsets, OPERATION)? {
            let indices = ctx
                .admit_iter(subset, "retain SLDPRT bore pattern solution")?
                .copied();
            if retain_bore_solution(ctx, &candidates, indices, &mut solutions)? {
                return Ok(None);
            }
        }
    }
    let mut solutions = ctx
        .admit_iter(solutions, OPERATION)?
        .map(|(_, solution)| solution);
    Ok(match (solutions.next(), solutions.next()) {
        (Some(solution), None) => Some(solution),
        _ => None,
    })
}

fn retain_bore_solution(
    ctx: &DecodeContext<'_>,
    candidates: &[([GridCoordinate; 3], FinitePoint3, FeatureDirection3)],
    indices: impl Iterator<Item = usize>,
    solutions: &mut BTreeMap<Vec<[GridCoordinate; 6]>, Vec<HolePlacement>>,
) -> Result<bool, CodecError> {
    const OPERATION: &str = "retain SLDPRT bore pattern solution";
    let quantize_scalar = |value: f64| GridCoordinate::new(value, EPS_HOLE_POSITION);
    let mut placements = Vec::new();
    let mut key = Vec::new();
    for index in indices {
        let (_, origin, axis) = candidates[index];
        ctx.reserve_vec(&mut placements, 1, OPERATION)?;
        ctx.reserve_vec(&mut key, 1, OPERATION)?;
        placements.push(HolePlacement::Axis { origin, axis });
        key.push([
            quantize_scalar(origin.x),
            quantize_scalar(origin.y),
            quantize_scalar(origin.z),
            quantize_scalar(axis.x),
            quantize_scalar(axis.y),
            quantize_scalar(axis.z),
        ]);
    }
    ctx.charge_work(u64_from_index(key.len()), OPERATION)?;
    ctx.insert_btree_map(solutions, key, placements, OPERATION)?;
    Ok(solutions.len() > 1)
}

/// The sign that moves `axis` into the canonical hemisphere: the sign of its
/// first component whose magnitude exceeds the exact-geometry tolerance, or
/// one when no component does.
fn canonical_sign(axis: Vector3) -> f64 {
    [axis.x, axis.y, axis.z]
        .into_iter()
        .find(|component| component.abs() > EPS_HOLE_EXACT_GEOMETRY)
        .map_or(1.0, f64::signum)
}

fn canonical_axis(axis: Vector3) -> Vector3 {
    let sign = canonical_sign(axis);
    Vector3::new(axis.x * sign, axis.y * sign, axis.z * sign)
}

/// The admitted form of [`canonical_axis`]. Multiplying a finite component by
/// one or minus one is exact, so the components equal the raw form bit for bit.
fn canonical_direction(axis: FeatureDirection3) -> FeatureDirection3 {
    if canonical_sign(axis.get()) < 0.0 {
        axis.reversed()
    } else {
        axis
    }
}

struct BoreSubsetSearch<'a, 'root> {
    ctx: &'a DecodeContext<'root>,
    marker_loci: &'a [Point2],
    candidate_loci: &'a [Point3],
    reverse: bool,
}

impl BoreSubsetSearch<'_, '_> {
    fn collect(
        &self,
        index: usize,
        assigned: &mut Vec<usize>,
        used: &mut HashSet<usize>,
        subsets: &mut BTreeSet<Vec<usize>>,
    ) -> Result<bool, CodecError> {
        const OPERATION: &str = "search SLDPRT congruent bore subsets";
        let _depth = self.ctx.enter_nested(OPERATION)?;
        self.ctx.charge_work(1, OPERATION)?;
        let (count, choices) = if self.reverse {
            (self.candidate_loci.len(), self.marker_loci.len())
        } else {
            (self.marker_loci.len(), self.candidate_loci.len())
        };
        if index == count {
            let mut subset = Vec::new();
            self.ctx
                .extend_from_slice(&mut subset, assigned, OPERATION)?;
            self.ctx
                .sort_unstable_by(&mut subset, |value| value, Ord::cmp, OPERATION)?;
            self.ctx
                .charge_work(u64_from_index(subset.len()), OPERATION)?;
            self.ctx.insert_btree_set(subsets, subset, OPERATION)?;
            return Ok(subsets.len() > 1);
        }
        for choice in 0..choices {
            self.ctx.charge_work(1, OPERATION)?;
            if used.contains(&choice) {
                continue;
            }
            self.ctx.insert_hash_set(used, choice, OPERATION)?;
            self.ctx
                .charge_work(u64_from_index(assigned.len()), OPERATION)?;
            let valid = assigned
                .iter()
                .copied()
                .enumerate()
                .all(|(previous, previous_choice)| {
                    let (marker, previous_marker, candidate, previous_candidate) = if self.reverse {
                        (choice, previous_choice, index, previous)
                    } else {
                        (index, previous, choice, previous_choice)
                    };
                    let marker_u = self.marker_loci[marker].u - self.marker_loci[previous_marker].u;
                    let marker_v = self.marker_loci[marker].v - self.marker_loci[previous_marker].v;
                    let marker_distance = if self.reverse {
                        Vector3::new(marker_u, marker_v, 0.0).norm()
                    } else {
                        marker_u.hypot(marker_v)
                    };
                    let delta = Vector3::new(
                        self.candidate_loci[candidate].x
                            - self.candidate_loci[previous_candidate].x,
                        self.candidate_loci[candidate].y
                            - self.candidate_loci[previous_candidate].y,
                        self.candidate_loci[candidate].z
                            - self.candidate_loci[previous_candidate].z,
                    );
                    same_dimension_length(marker_distance, delta.norm())
                });
            if valid {
                self.ctx.reserve_vec(assigned, 1, OPERATION)?;
                assigned.push(choice);
                let ambiguous = self.collect(index + 1, assigned, used, subsets)?;
                assigned.pop();
                used.remove(&choice);
                if ambiguous {
                    return Ok(true);
                }
            } else {
                used.remove(&choice);
            }
        }
        Ok(false)
    }
}

fn has_unique_marker_loci_subset(
    ctx: &DecodeContext<'_>,
    marker_loci: &[Point2],
    candidate_loci: &[Point3],
) -> Result<bool, CodecError> {
    if candidate_loci.is_empty() || candidate_loci.len() > marker_loci.len() {
        return Ok(false);
    }
    let mut subsets = BTreeSet::new();
    (BoreSubsetSearch {
        ctx,
        marker_loci,
        candidate_loci,
        reverse: true,
    })
    .collect(0, &mut Vec::new(), &mut HashSet::new(), &mut subsets)?;
    Ok(subsets.len() == 1)
}

pub(super) fn feature_object_byte_ranges<'a>(
    ctx: &DecodeContext<'_>,
    histories: &'a [crate::records::FeatureHistory],
    lane: &FeatureInputLane,
) -> Result<HashMap<&'a str, (usize, usize, usize)>, CodecError> {
    const OPERATION: &str = "index SLDPRT feature object byte ranges";
    let mut objects = Vec::new();
    let mut next_input_index = 0usize;
    for history in ctx.admit_iter(histories, OPERATION)? {
        for feature in ctx.admit_iter(&history.features, OPERATION)? {
            let input_index = next_input_index;
            next_input_index = next_input_index
                .checked_add(1)
                .ok_or_else(|| ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX))?;
            ctx.charge_work(
                u64_from_index(lane.names.len())
                    .checked_add(1)
                    .ok_or_else(|| ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX))?,
                OPERATION,
            )?;
            let Some(name) = feature_object_name(feature, lane) else {
                continue;
            };
            ctx.reserve_vec(&mut objects, 1, OPERATION)?;
            objects.push((name.offset, input_index, feature));
        }
    }
    ctx.sort_unstable_by_key(
        &mut objects,
        |value| {
            let (left_offset, left_index, _) = value;
            (*left_offset, *left_index)
        },
        Ord::cmp,
        OPERATION,
    )?;
    let mut ranges = HashMap::new();
    for (index, (offset, _, feature)) in objects.iter().enumerate() {
        ctx.charge_work(1, OPERATION)?;
        let Some(start) = usize::try_from(*offset).ok() else {
            continue;
        };
        let context_start = index
            .checked_sub(1)
            .and_then(|index| objects.get(index))
            .and_then(|(offset, _, _)| usize::try_from(*offset).ok())
            .unwrap_or(0);
        let end = objects
            .get(index + 1)
            .and_then(|(offset, _, _)| usize::try_from(*offset).ok())
            .unwrap_or(lane.native_payload.len());
        ctx.insert_hash_map(
            &mut ranges,
            feature.id.as_str(),
            (context_start, start, end),
            OPERATION,
        )?;
    }
    Ok(ranges)
}

fn hole_temporary_axis(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
    start: usize,
    end: usize,
) -> Result<Option<(Point3, Vector3)>, CodecError> {
    const DECLARATION: &[u8] = b"\xff\xff\x01\x00\x0f\x00moTempAxisRef_w";
    const HANDLE_PAIR: &[u8] = b"\xc7\xcf\xff\xff\xc7\xcf\xff\xff";
    const NATIVE_TO_IR: f64 = 1000.0;
    const OPERATION: &str = "scan SLDPRT hole temporary axes";

    let Some(last_declaration) = end.checked_sub(364) else {
        return Ok(None);
    };
    if start > last_declaration {
        return Ok(None);
    }
    let Some(available) = payload.get(start..) else {
        return Ok(None);
    };
    let count = last_declaration - start + 1;
    let positions = match available.get(..count) {
        Some(positions) => positions,
        None => available,
    };
    let mut axis = None;
    for (relative, _) in ctx.admit_iter(positions, OPERATION)?.enumerate() {
        let declaration = start + relative;
        if payload.get(declaration..declaration + DECLARATION.len()) != Some(DECLARATION)
            || payload.get(declaration + 267..declaration + 275) != Some(HANDLE_PAIR)
            || payload.get(declaration + 275..declaration + 279) != Some(&[0; 4])
            || View::u32_le_at(payload, declaration + 279).is_none_or(|address| address == 0)
            || payload.get(declaration + 283..declaration + 299) != Some(&[0; 16])
        {
            continue;
        }
        let scalar = |index: usize| {
            let offset = declaration + 299 + index * 8;
            let value = View::f64_le_at(payload, offset)?;
            value.is_finite().then_some(value)
        };
        let Some((depth, origin, direction)) = (|| {
            Some((
                scalar(0)?,
                Point3::new(
                    scalar(1)? * NATIVE_TO_IR,
                    scalar(2)? * NATIVE_TO_IR,
                    scalar(3)? * NATIVE_TO_IR,
                ),
                Vector3::new(scalar(4)?, scalar(5)?, scalar(6)?),
            ))
        })() else {
            continue;
        };
        let norm = direction.norm();
        let record_end = declaration + 355;
        let Some(available) = payload.get(record_end..) else {
            continue;
        };
        let positions = match available.get(..25) {
            Some(positions) => positions,
            None => available,
        };
        let mut next_record = None;
        for (relative, _) in ctx.admit_iter(positions, OPERATION)?.enumerate() {
            let offset = record_end + relative;
            let Some(padding) = payload.get(record_end..offset) else {
                continue;
            };
            if ctx.admit_iter(padding, OPERATION)?.all(|byte| *byte == 0)
                && (payload.get(offset..offset + 4) == Some(CLASS_MARKER)
                    || View::u16_le_at(payload, offset).is_some_and(is_class_token))
            {
                next_record = Some(offset);
                break;
            }
        }
        let Some(next_record) = next_record else {
            continue;
        };
        if !(depth > 0.0 && (norm - 1.0).abs() <= EPS_HOLE_GEOMETRY && next_record < end) {
            continue;
        }
        let candidate = (
            origin,
            Vector3::new(direction.x / norm, direction.y / norm, direction.z / norm),
        );
        if axis.is_some_and(|axis| candidate != axis) {
            return Ok(None);
        }
        axis = Some(candidate);
    }
    Ok(axis)
}

pub(super) fn feature_input_sketch_frame(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
    plane_frames: &HashMap<u32, SketchPlaneFrame>,
    plane_index: &CompactReferencePlaneIndex<'_>,
    context_start: usize,
    start: usize,
    end: usize,
) -> Result<Option<(Point3, Vector3, Vector3)>, CodecError> {
    let reference = plane_index
        .profile_source(ctx, context_start, start, end)?
        .and_then(|source| plane_frames.get(&source).copied());
    let component = compact_profile_component_plane_frame(ctx, payload, context_start, start, end)?;
    let explicit = || -> Result<Option<(Point3, Vector3, Vector3)>, CodecError> {
        let Some(object) = payload.get(start..end) else {
            return Ok(None);
        };
        let Ok(Some((origin, normal, u_axis))) = explicit_reference_plane_frame(ctx, object)?
        else {
            return Ok(None);
        };
        let finite_zero = |value: f64| {
            if value.abs() <= EPS_HOLE_EXACT_GEOMETRY {
                0.0
            } else {
                value
            }
        };
        Ok(Some((
            Point3::new(
                finite_zero(origin.x),
                finite_zero(origin.y),
                finite_zero(origin.z),
            ),
            Vector3::new(
                finite_zero(normal.x),
                finite_zero(normal.y),
                finite_zero(normal.z),
            ),
            Vector3::new(
                finite_zero(u_axis.x),
                finite_zero(u_axis.y),
                finite_zero(u_axis.z),
            ),
        )))
    };
    Ok(match reference {
        Some(reference) => {
            let component = component
                .filter(|component| coplanar_plane_frames(reference.as_tuple(), *component));
            if reference.u_axis_source == SketchPlaneUAxisSource::ConstructedMidPlane {
                match component {
                    Some(component) => Some(component),
                    None => explicit()?
                        .filter(|frame| coplanar_plane_frames(reference.as_tuple(), *frame)),
                }
            } else {
                component.or(Some(reference.as_tuple()))
            }
        }
        None => match component {
            Some(component) => Some(component),
            None => explicit()?,
        },
    })
}

fn coplanar_plane_frames(
    reference: (Point3, Vector3, Vector3),
    candidate: (Point3, Vector3, Vector3),
) -> bool {
    let reference_normal_length = reference.1.norm();
    let candidate_normal_length = candidate.1.norm();
    if !reference_normal_length.is_finite()
        || !candidate_normal_length.is_finite()
        || reference_normal_length <= f64::EPSILON
        || candidate_normal_length <= f64::EPSILON
    {
        return false;
    }
    let normal_alignment = (reference.1.x * candidate.1.x
        + reference.1.y * candidate.1.y
        + reference.1.z * candidate.1.z)
        / (reference_normal_length * candidate_normal_length);
    if (normal_alignment.abs() - 1.0).abs() > EPS_HOLE_POSITION {
        return false;
    }
    let displacement = Vector3::new(
        candidate.0.x - reference.0.x,
        candidate.0.y - reference.0.y,
        candidate.0.z - reference.0.z,
    );
    let normal_distance = (displacement.x * reference.1.x
        + displacement.y * reference.1.y
        + displacement.z * reference.1.z)
        / reference_normal_length;
    let scale = reference
        .0
        .x
        .abs()
        .max(reference.0.y.abs())
        .max(reference.0.z.abs())
        .max(candidate.0.x.abs())
        .max(candidate.0.y.abs())
        .max(candidate.0.z.abs())
        .max(1.0);
    normal_distance.abs() <= EPS_HOLE_POSITION * scale
}

pub(super) fn sketch_feature_frames(
    ctx: &DecodeContext<'_>,
    features: &[cadmpeg_ir::features::Feature],
    histories: &[crate::records::FeatureHistory],
    lanes: &[FeatureInputLane],
) -> Result<HashMap<String, (Point3, Vector3, Vector3)>, CodecError> {
    let mut candidates_storage = ctx.reserve_scoped(0, "index sketch feature frame owners")?;
    let mut candidates = HashMap::<String, Option<(Point3, Vector3, Vector3)>>::new();
    for lane in ctx.admit_iter(lanes, "scan SLDPRT holes records")? {
        let ranges = feature_object_byte_ranges(ctx, histories, lane)?;
        let plane_frames = lane_sketch_plane_frames(ctx, features, histories, lane)?;
        let plane_index = CompactReferencePlaneIndex::new(ctx, &lane.native_payload)?;
        for history in ctx.admit_iter(histories, "resolve sketch feature frames")? {
            for feature in ctx.admit_iter(&history.features, "resolve sketch feature frames")? {
                if feature.xml_tag != "Sketch" {
                    continue;
                }
                let Some(&(context_start, start, end)) =
                    ctx.get_hash_map(&ranges, feature.id.as_str(), "resolve SLDPRT holes keys")?
                else {
                    continue;
                };
                let Some(frame) = feature_input_sketch_frame(
                    ctx,
                    &lane.native_payload,
                    &plane_frames,
                    &plane_index,
                    context_start,
                    start,
                    end,
                )?
                else {
                    continue;
                };
                if let Some(candidate) = ctx.get_mut_hash_map(
                    &mut candidates,
                    feature.id.as_str(),
                    "resolve SLDPRT holes keys",
                )? {
                    if candidate.as_ref().is_some_and(|current| {
                        reference_plane_frame_key(current) != reference_plane_frame_key(&frame)
                    }) {
                        *candidate = None;
                    }
                } else {
                    candidates_storage.with_storage(|| {
                        let mut owner = String::new();
                        ctx.append_retained(
                            &mut owner,
                            &feature.id,
                            "copy sketch feature frame owner",
                        )?;
                        ctx.insert_hash_map(
                            &mut candidates,
                            owner,
                            Some(frame),
                            "resolve SLDPRT holes keys",
                        )
                    })?;
                }
            }
        }
    }
    let mut unique = HashMap::new();
    for (feature, frame) in candidates {
        ctx.charge_work(1, "select unique sketch feature frames")?;
        if let Some(frame) = frame {
            ctx.insert_hash_map(
                &mut unique,
                feature,
                frame,
                "retain unique sketch feature frames",
            )?;
        }
    }
    Ok(unique)
}

fn compact_position_relations(
    ctx: &DecodeContext<'_>,
    lane: &FeatureInputLane,
    feature: &str,
) -> Result<Vec<(FeatureInputRelationFamily, u16, u16, f64)>, CodecError> {
    const OPERATION: &str = "collect SLDPRT compact position relations";
    ctx.charge_work(u64_from_index(lane.scalars.len()), OPERATION)?;
    let mut scalars_storage = ctx.reserve_scoped(0, "resolve SLDPRT holes keys")?;
    let mut scalars = HashMap::new();

    for scalar in &lane.scalars {
        scalars_storage.with_storage(|| {
            ctx.insert_hash_map(
                &mut scalars,
                scalar.id.as_str(),
                scalar,
                "resolve SLDPRT holes keys",
            )
        })?;
    }
    let mut result = Vec::new();
    for relation in &lane.relation_instances {
        ctx.charge_work(1, OPERATION)?;
        if !ctx.equal(
            relation.feature_ref.as_str(),
            feature,
            "compare SLDPRT holes records",
        )? {
            continue;
        }
        let [first, second] = relation.operands.as_slice() else {
            continue;
        };
        if first.kind != FeatureInputOperandKind::Native(NativeOperandTag::TAG_8152)
            || second.kind != FeatureInputOperandKind::Native(NativeOperandTag::TAG_8152)
        {
            continue;
        }
        let Some(scalar) = (match relation.parameter_scalar_ref() {
            Some(id) => ctx.get_hash_map(&(scalars), id, "resolve SLDPRT holes references")?,
            None => None,
        }) else {
            continue;
        };
        if !(scalar.role == FeatureInputScalarRole::Driving && scalar.value.get() >= 0.0) {
            continue;
        }
        ctx.reserve_vec(&mut result, 1, OPERATION)?;
        result.push((
            relation.family,
            first.entity_index,
            second.entity_index,
            scalar.value.get() * 1000.0,
        ));
    }
    Ok(result)
}

fn constrained_bore_axes(
    ctx: &DecodeContext<'_>,
    (origin, normal, u_axis): (Point3, Vector3, Vector3),
    radius: f64,
    surfaces: &[Surface],
    relations: &[(FeatureInputRelationFamily, u16, u16, f64)],
) -> Result<Option<Vec<HolePlacement>>, CodecError> {
    const QUANTUM: f64 = EPS_HOLE_POSITION;
    const OPERATION: &str = "collect SLDPRT constrained bore axes";
    let v_axis = normal.cross(u_axis);
    let radius_tolerance = (radius.abs() * EPS_HOLE_GEOMETRY).max(EPS_HOLE_GEOMETRY);
    let mut axes = Vec::new();
    for surface in surfaces {
        ctx.charge_work(1, OPERATION)?;
        let SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cylinder(cylinder_surface)) =
            surface.geometry
        else {
            continue;
        };
        let axis = *cylinder_surface.frame().axis().as_raw();
        let candidate_radius = cylinder_surface.radius().get();
        if !((candidate_radius - radius).abs() <= radius_tolerance
            && axis.dot(normal).abs() >= 1.0 - EPS_HOLE_GEOMETRY)
        {
            continue;
        }
        let candidate = cylinder_surface.origin().get();
        let delta = Vector3::new(
            candidate.x - origin.x,
            candidate.y - origin.y,
            candidate.z - origin.z,
        );
        ctx.reserve_vec(&mut axes, 1, OPERATION)?;
        axes.push(quantize(
            Point2::new(delta.dot(u_axis), delta.dot(v_axis)),
            QUANTUM,
        ));
    }
    ctx.sort_unstable_by(&mut axes, |value| value, Ord::cmp, OPERATION)?;
    ctx.dedup_vec(&mut axes, OPERATION)?;
    if axes.is_empty() {
        return Ok(None);
    }
    let mut loci = Vec::new();
    ctx.reserve_vec(&mut loci, 1, OPERATION)?;
    loci.push(Point2::new(0.0, 0.0));
    let mut bore_loci = HashSet::new();
    for point in ctx.admit_iter(&axes, OPERATION)? {
        let Some(point) = point.point(QUANTUM) else {
            return Ok(None);
        };
        ctx.charge_work(u64_from_index(loci.len()), OPERATION)?;
        let index = if let Some(index) = loci.iter().position(|candidate| *candidate == point) {
            index
        } else {
            ctx.reserve_vec(&mut loci, 1, OPERATION)?;
            loci.push(point);
            loci.len() - 1
        };
        ctx.insert_hash_set(&mut bore_loci, index, OPERATION)?;
    }
    let Some(indices) = compact_position_loci(ctx, &loci, &bore_loci, relations)? else {
        return Ok(None);
    };
    let mut placements = Vec::new();
    for &index in ctx.admit_iter(&indices, OPERATION)? {
        let point = loci[index];
        let (Some(origin), Some(axis)) = (
            FinitePoint3::new(Point3::new(
                origin.x + point.u * u_axis.x + point.v * v_axis.x,
                origin.y + point.u * u_axis.y + point.v * v_axis.y,
                origin.z + point.u * u_axis.z + point.v * v_axis.z,
            )),
            FeatureDirection3::new(normal),
        ) else {
            return Ok(None);
        };
        ctx.reserve_vec(&mut placements, 1, OPERATION)?;
        placements.push(HolePlacement::Axis { origin, axis });
    }
    Ok(Some(placements))
}

fn compact_position_loci(
    ctx: &DecodeContext<'_>,
    loci: &[Point2],
    placement_loci: &HashSet<usize>,
    relations: &[(FeatureInputRelationFamily, u16, u16, f64)],
) -> Result<Option<Vec<usize>>, CodecError> {
    const OPERATION: &str = "solve SLDPRT compact position loci";
    let mut nodes = Vec::new();
    for (_, first, second, _) in relations {
        ctx.charge_work(1, OPERATION)?;
        ctx.reserve_vec(&mut nodes, 2, OPERATION)?;
        nodes.extend([*first, *second]);
    }
    ctx.sort_unstable_by(&mut nodes, |value| value, Ord::cmp, OPERATION)?;
    ctx.dedup_vec(&mut nodes, OPERATION)?;
    if nodes.is_empty() || nodes.len() > loci.len() {
        return Ok(None);
    }
    let mut solutions = HashSet::new();
    for swap_axes in [false, true] {
        CompactPositionSearch {
            ctx,
            nodes: &nodes,
            loci,
            relations,
            placement_loci,
            swap_axes,
        }
        .assign(0, &mut HashMap::new(), &mut HashSet::new(), &mut solutions)?;
    }
    let mut solutions = solutions.into_iter();
    Ok(match (solutions.next(), solutions.next()) {
        (Some(solution), None) => Some(solution),
        _ => None,
    })
}

struct CompactPositionSearch<'a, 'root> {
    ctx: &'a DecodeContext<'root>,
    nodes: &'a [u16],
    loci: &'a [Point2],
    relations: &'a [(FeatureInputRelationFamily, u16, u16, f64)],
    placement_loci: &'a HashSet<usize>,
    swap_axes: bool,
}

impl CompactPositionSearch<'_, '_> {
    fn assign(
        &self,
        node_index: usize,
        assigned: &mut HashMap<u16, usize>,
        used: &mut HashSet<usize>,
        solutions: &mut HashSet<Vec<usize>>,
    ) -> Result<(), CodecError> {
        const OPERATION: &str = "search SLDPRT compact position assignments";
        let _depth = self.ctx.enter_nested(OPERATION)?;
        self.ctx.charge_work(1, OPERATION)?;
        if solutions.len() > 1 {
            return Ok(());
        }
        if node_index == self.nodes.len() {
            let mut solution = Vec::new();
            for index in used.iter().copied() {
                self.ctx.charge_work(1, OPERATION)?;
                if self.placement_loci.contains(&index) {
                    self.ctx.reserve_vec(&mut solution, 1, OPERATION)?;
                    solution.push(index);
                }
            }
            if solution.is_empty() {
                return Ok(());
            }
            self.ctx
                .sort_unstable_by(&mut solution, |value| value, Ord::cmp, OPERATION)?;
            self.ctx.insert_hash_set(solutions, solution, OPERATION)?;
            return Ok(());
        }
        let node = self.nodes[node_index];
        for locus_index in 0..self.loci.len() {
            self.ctx.charge_work(1, OPERATION)?;
            if used.contains(&locus_index) {
                continue;
            }
            self.ctx.insert_hash_set(used, locus_index, OPERATION)?;
            self.ctx.reserve_map(assigned, 1, OPERATION)?;
            assigned.insert(node, locus_index);
            self.ctx
                .charge_work(u64_from_index(self.relations.len()), OPERATION)?;
            let valid = self
                .relations
                .iter()
                .all(|(family, first, second, distance)| {
                    let (Some(&first), Some(&second)) = (assigned.get(first), assigned.get(second))
                    else {
                        return true;
                    };
                    let first = self.loci[first];
                    let second = self.loci[second];
                    let measured = match family {
                        FeatureInputRelationFamily::PointPointDistance => {
                            (second.u - first.u).hypot(second.v - first.v)
                        }
                        FeatureInputRelationFamily::PointPointHorizontalDistance => {
                            if self.swap_axes {
                                (second.v - first.v).abs()
                            } else {
                                (second.u - first.u).abs()
                            }
                        }
                        FeatureInputRelationFamily::PointPointVerticalDistance => {
                            if self.swap_axes {
                                (second.u - first.u).abs()
                            } else {
                                (second.v - first.v).abs()
                            }
                        }
                        _ => return false,
                    };
                    same_dimension_length(measured, *distance)
                });
            if valid {
                self.assign(node_index + 1, assigned, used, solutions)?;
            }
            assigned.remove(&node);
            used.remove(&locus_index);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
