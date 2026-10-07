//! Hole construction, bore topology and hole axis projection.

use super::compact_reference_planes::{
    compact_profile_component_plane_frame, CompactReferencePlaneIndex,
};
use super::curves::{lane_sketch_plane_frames, SketchPlaneFrame, SketchPlaneUAxisSource};
use super::grid::{quantize, GridCoordinate};
use super::helix::fit_helix_polyline;
use super::reference_geometry::{explicit_reference_plane_frame, reference_plane_frame_key};
use super::relation_loci::same_dimension_length;
use super::scalars::{lane_object_names, ObjectNames};
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
    let object_names = lane_object_names(ctx, &mut records_storage, lanes)?;
    let mut object_offsets = Vec::new();
    for names in ctx.admit_iter(&object_names, "index SLDPRT helix object boundaries")? {
        let mut offsets = Vec::new();
        for history in ctx.admit_iter(histories, "index SLDPRT helix object boundaries")? {
            for feature in
                ctx.admit_iter(&history.features, "index SLDPRT helix object boundaries")?
            {
                if let Some(name) = names.of(ctx, feature)? {
                    records_storage.with_storage(|| {
                        ctx.push_vec(
                            &mut offsets,
                            name.offset,
                            "index SLDPRT helix object boundaries",
                        )
                    })?;
                }
            }
        }
        ctx.sort_unstable_by(
            &mut offsets,
            |offset| offset,
            Ord::cmp,
            "sort SLDPRT helix object boundaries",
        )?;
        records_storage.with_storage(|| {
            ctx.push_vec(
                &mut object_offsets,
                offsets,
                "index SLDPRT helix object boundaries",
            )
        })?;
    }
    let mut source_items = model_features.into_iter();
    while let Some(model_feature) =
        ctx.next_charged(&mut source_items, "scan SLDPRT helix features")?
    {
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
        let mut mesh_storage = ctx.reserve_scoped(0, "SLDPRT helix mesh workspace")?;
        let mut meshes = Vec::new();
        for (lane_index, (lane, names)) in ctx
            .admit_iter(lanes, "scan SLDPRT holes records")?
            .zip(&object_names)
            .enumerate()
        {
            let Some(name) = names.of(ctx, record)? else {
                continue;
            };
            let start = usize::try_from(name.offset).ok();
            let offsets = &object_offsets[lane_index];
            let next = ctx.partition_point(
                offsets,
                |offset| Ok(*offset <= name.offset),
                "find SLDPRT helix object boundary",
            )?;
            let end = offsets
                .get(next)
                .and_then(|offset| usize::try_from(*offset).ok())
                .unwrap_or(lane.native_payload.len());
            let Some(object) = start.and_then(|start| lane.native_payload.get(start..end)) else {
                continue;
            };
            let (streams, _stream_storage) = ctx
                .with_scoped_storage("SLDPRT helix stream workspace", || {
                    crate::parasolid::extract_streams_with_offsets(object, ctx)
                })?;
            for stream in ctx.admit_iter(&streams, "scan SLDPRT helix streams")? {
                if let Some(points) = mesh_storage.with_storage(|| {
                    crate::parasolid::mesh_polyline_from_header(
                        ctx,
                        &stream.payload,
                        &stream.header,
                    )
                })? {
                    mesh_storage.with_storage(|| {
                        ctx.push_vec(&mut meshes, points, "collect SLDPRT helix meshes")
                    })?;
                }
            }
        }
        let [points] = meshes.as_slice() else {
            continue;
        };
        let Some((axis_origin, mut axis_direction, radius, fitted_rise)) = ctx
            .with_scoped_storage("SLDPRT helix fit workspace", || {
                fit_helix_polyline(ctx, points, *revolutions, *clockwise)
            })?
            .0
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

struct HoleLaneNames<'lane, 'ctx> {
    names: ObjectNames<'lane, 'ctx>,
    by_offset: Vec<&'lane crate::records::FeatureInputName>,
    _storage: cadmpeg_core::decode::ScopedReservation<'ctx>,
}

impl<'lane, 'ctx> HoleLaneNames<'lane, 'ctx> {
    fn new(
        ctx: &'ctx DecodeContext<'_>,
        lane: &'lane FeatureInputLane,
    ) -> Result<Self, CodecError> {
        const OPERATION: &str = "index SLDPRT hole child name offsets";
        let names = ObjectNames::new(ctx, lane)?;
        let (mut by_offset, storage) =
            ctx.with_scoped_storage(OPERATION, || ctx.collect_vec(lane.names.iter(), OPERATION))?;
        ctx.stable_sort_by(&mut by_offset, |name| &name.offset, Ord::cmp, OPERATION)?;
        Ok(Self {
            names,
            by_offset,
            _storage: storage,
        })
    }
}

fn hole_lane_names<'lane, 'ctx>(
    ctx: &'ctx DecodeContext<'_>,
    storage: &mut cadmpeg_core::decode::ScopedReservation<'_>,
    lanes: &'lane [FeatureInputLane],
) -> Result<Vec<HoleLaneNames<'lane, 'ctx>>, CodecError> {
    const OPERATION: &str = "index SLDPRT hole lane names";
    let mut result = Vec::new();
    for lane in ctx.admit_iter(lanes, OPERATION)? {
        let names = HoleLaneNames::new(ctx, lane)?;
        storage.with_storage(|| ctx.push_vec(&mut result, names, OPERATION))?;
    }
    Ok(result)
}

fn hole_position_sketch_source(
    ctx: &DecodeContext<'_>,
    feature: &crate::records::Feature,
    lane: &FeatureInputLane,
    object_names: &HoleLaneNames<'_, '_>,
) -> Result<Option<u32>, CodecError> {
    const OPERATION: &str = "scan SLDPRT hole position source";
    if classify(feature) != Some(FeatureClass::Hole) {
        return Ok(None);
    }
    let Some(name) = object_names.names.of(ctx, feature)? else {
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
    for bytes in body.windows(window.get()) {
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
    let first = ctx.partition_point(
        &object_names.by_offset,
        |child| Ok(child.offset < u64_from_index(body_start)),
        OPERATION,
    )?;
    let last = ctx.partition_point(
        &object_names.by_offset,
        |child| Ok(child.offset < u64_from_index(body_end)),
        OPERATION,
    )?;
    let mut children = object_names.by_offset[first..last].iter();
    while let Some(child) = ctx.next_charged(&mut children, OPERATION)? {
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
    let mut names_storage = ctx.reserve_scoped(0, "index SLDPRT hole object names")?;
    let object_names = hole_lane_names(ctx, &mut names_storage, lanes)?;
    let mut source_items = histories.into_iter();
    while let Some(history) = ctx.next_charged(&mut source_items, OPERATION)? {
        let mut object_storage = ctx.reserve_scoped(0, "SLDPRT hole object order workspace")?;
        let mut object_orders = Vec::new();
        for names in ctx.admit_iter(&object_names, OPERATION)? {
            let mut objects = Vec::new();
            for (index, feature) in ctx.admit_iter(&history.features, OPERATION)?.enumerate() {
                if let Some(name) = names.names.of(ctx, feature)? {
                    object_storage.with_storage(|| {
                        ctx.push_vec(&mut objects, (name.offset, index), OPERATION)
                    })?;
                }
            }
            ctx.sort_unstable_by(&mut objects, |value| value, Ord::cmp, OPERATION)?;
            object_storage.with_storage(|| ctx.push_vec(&mut object_orders, objects, OPERATION))?;
        }
        let records = HoleHistoryIndex::new(ctx, history)?;
        let mut addition_storage = ctx.reserve_scoped(0, OPERATION)?;
        let mut additions = Vec::new();
        let mut source_items = history.features.iter().enumerate().into_iter();
        while let Some((feature_index, feature)) = ctx.next_charged(&mut source_items, OPERATION)? {
            if classify(feature) != Some(FeatureClass::Hole)
                || ctx.contains_key_btree_map(
                    &feature.properties,
                    "DissectableChildren",
                    "find SLDPRT hole property",
                )?
            {
                continue;
            }
            let profile = if let Some(profile) = hole_profile_from_position_source(
                ctx,
                feature,
                history,
                &records,
                lanes,
                &object_names,
                &object_orders,
            )? {
                Some(profile)
            } else {
                hole_profile_from_child_order(ctx, feature, history, &records)?
            };
            let Some((profile, rank)) = profile else {
                continue;
            };
            let source =
                addition_storage.with_storage(|| copy_hole_profile_source(ctx, profile))?;
            addition_storage.with_storage(|| {
                ctx.push_vec(&mut additions, (feature_index, source, rank), OPERATION)
            })?;
        }
        drop(records);
        let (claimed_profiles, claims_storage) =
            ctx.with_scoped_storage(OPERATION, || claimed_hole_profiles(ctx, &history.features))?;
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
        drop((claimed_profiles, claims_storage));
        let mut source_items = additions.into_iter();
        while let Some((feature_index, profile_source, _)) =
            ctx.next_charged(&mut source_items, OPERATION)?
        {
            ctx.insert_btree_map(
                &mut history.features[feature_index].properties,
                cadmpeg_core::nonblank_literal!("DissectableChildren"),
                ctx.copy_retained_text(&profile_source, "copy SLDPRT hole profile ownership")?,
                OPERATION,
            )?;
        }
        drop((
            addition_storage,
            profile_claim_ranks,
            profile_claim_ranks_storage,
        ));
        let (claimed_profiles, claims_storage) =
            ctx.with_scoped_storage(OPERATION, || claimed_hole_profiles(ctx, &history.features))?;
        let mut interval_storage =
            ctx.reserve_scoped(0, "SLDPRT hole source interval workspace")?;
        let mut hole_sources = Vec::new();
        let mut profiles_by_source = Vec::new();
        for (index, candidate) in ctx.admit_iter(&history.features, OPERATION)?.enumerate() {
            let Some(source) = candidate.source_value() else {
                continue;
            };
            match classify(candidate) {
                Some(FeatureClass::Hole) => {
                    interval_storage
                        .with_storage(|| ctx.push_vec(&mut hole_sources, source, OPERATION))?;
                }
                Some(FeatureClass::Sketch) => {
                    interval_storage.with_storage(|| {
                        ctx.push_vec(&mut profiles_by_source, (source, index), OPERATION)
                    })?;
                }
                _ => {}
            }
        }
        ctx.sort_unstable_by(&mut hole_sources, |value| value, Ord::cmp, OPERATION)?;
        ctx.sort_unstable_by(&mut profiles_by_source, |value| value, Ord::cmp, OPERATION)?;
        let mut interval_additions = Vec::new();
        let mut source_items = history.features.iter().enumerate().into_iter();
        while let Some((feature_index, feature)) = ctx.next_charged(&mut source_items, OPERATION)? {
            if classify(feature) != Some(FeatureClass::Hole)
                || ctx.contains_key_btree_map(
                    &feature.properties,
                    "DissectableChildren",
                    "find SLDPRT hole property",
                )?
            {
                continue;
            }
            let Some(source) = feature.source_value() else {
                continue;
            };
            let next = ctx.partition_point(
                &hole_sources,
                |candidate| Ok(*candidate <= source),
                OPERATION,
            )?;
            let Some(&upper) = hole_sources.get(next) else {
                continue;
            };
            let first_profile = ctx.partition_point(
                &profiles_by_source,
                |(candidate, _)| Ok(*candidate <= source),
                OPERATION,
            )?;
            let last_profile = ctx.partition_point(
                &profiles_by_source,
                |(candidate, _)| Ok(*candidate < upper),
                OPERATION,
            )?;
            let mut candidates = profiles_by_source[first_profile..last_profile].iter();
            let mut first = None;
            let mut ambiguous = false;
            while let Some(&(candidate_source, index)) =
                ctx.next_charged(&mut candidates, OPERATION)?
            {
                let candidate = &history.features[index];
                let (identity, _identity_storage) = ctx
                    .with_scoped_storage("SLDPRT hole profile source identity", || {
                        ctx.format_retained(format_args!("{candidate_source}"), OPERATION)
                    })?;
                if ctx.contains_hash_set(
                    &claimed_profiles,
                    identity.as_str(),
                    "resolve SLDPRT holes keys",
                )? || !crate::history::project::solid::is_hole_profile_construction(
                    ctx, candidate,
                )? {
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
            let source =
                interval_storage.with_storage(|| copy_hole_profile_source(ctx, profile))?;
            interval_storage.with_storage(|| {
                ctx.push_vec(&mut interval_additions, (feature_index, source), OPERATION)
            })?;
        }
        drop((claimed_profiles, claims_storage));
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
        let mut source_items = interval_additions.into_iter();
        while let Some((feature_index, profile_source)) =
            ctx.next_charged(&mut source_items, OPERATION)?
        {
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
                ctx.copy_retained_text(&profile_source, "copy SLDPRT hole profile ownership")?,
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
        None => ctx.copy_retained_text(&profile.id, OPERATION),
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
    let mut characters = value
        .char_indices()
        .chain(std::iter::once((value.len(), ',')));
    let mut done = false;
    Ok(std::iter::from_fn(move || {
        if done {
            return None;
        }
        let token = ctx.find_map(
            characters.by_ref(),
            |(offset, character)| {
                if character != ',' {
                    return Ok(None);
                }
                let token = value
                    .get(start..offset)
                    .ok_or_else(|| ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX))?;
                start = offset
                    .checked_add(1)
                    .ok_or_else(|| ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX))?;
                Ok(Some(token))
            },
            OPERATION,
        );
        if !matches!(token, Ok(Some(_))) {
            done = true;
        }
        token.transpose()
    }))
}

struct HoleHistoryIndex<'history, 'ctx> {
    history: &'history crate::records::FeatureHistory,
    by_id: HashMap<&'history str, Vec<usize>>,
    by_source: HashMap<FeatureSource, Vec<usize>>,
    by_ordinal: HashMap<u32, Vec<usize>>,
    numeric_sources: Vec<(u32, usize)>,
    _storage: cadmpeg_core::decode::ScopedReservation<'ctx>,
}

impl<'history, 'ctx> HoleHistoryIndex<'history, 'ctx> {
    fn new(
        ctx: &'ctx DecodeContext<'_>,
        history: &'history crate::records::FeatureHistory,
    ) -> Result<Self, CodecError> {
        const OPERATION: &str = "index SLDPRT hole history records";
        let mut storage = ctx.reserve_scoped(0, OPERATION)?;
        let mut by_id = HashMap::new();
        let mut by_source = HashMap::new();
        let mut by_ordinal = HashMap::new();
        let mut numeric_sources = Vec::new();
        for (index, feature) in ctx.admit_iter(&history.features, OPERATION)?.enumerate() {
            storage.with_storage(|| {
                ctx.push_hash_group(&mut by_id, feature.id.as_str(), index, OPERATION, OPERATION)?;
                ctx.push_hash_group(
                    &mut by_ordinal,
                    feature.ordinal,
                    index,
                    OPERATION,
                    OPERATION,
                )?;
                if let Some(source) = feature.source_id {
                    ctx.push_hash_group(&mut by_source, source, index, OPERATION, OPERATION)?;
                }
                if let Some(source) = feature.source_value() {
                    ctx.push_vec(&mut numeric_sources, (source, index), OPERATION)?;
                }
                Ok::<_, CodecError>(())
            })?;
        }
        ctx.sort_unstable_by(&mut numeric_sources, |record| record, Ord::cmp, OPERATION)?;
        Ok(Self {
            history,
            by_id,
            by_source,
            by_ordinal,
            numeric_sources,
            _storage: storage,
        })
    }

    fn child(
        &self,
        ctx: &DecodeContext<'_>,
        source: &str,
    ) -> Result<Option<&'history crate::records::Feature>, CodecError> {
        const OPERATION: &str = "match SLDPRT hole child profile";
        let parsed = if source == "-1" {
            Some(FeatureSource::Reserved)
        } else {
            ctx.parse_text::<u32>(source, OPERATION)?
                .ok()
                .and_then(FeatureSource::from_value)
        };
        let by_id = ctx
            .get_hash_map(&self.by_id, source, OPERATION)?
            .map_or(&[][..], Vec::as_slice);
        let by_source = match parsed {
            Some(source) => ctx
                .get_hash_map(&self.by_source, &source, OPERATION)?
                .map_or(&[][..], Vec::as_slice),
            None => &[],
        };
        if by_id.len() > 1 || by_source.len() > 1 {
            return Ok(None);
        }
        let index = match (by_id.first(), by_source.first()) {
            (Some(first), Some(second)) if first != second => None,
            (Some(index), _) | (_, Some(index)) => Some(*index),
            _ => None,
        };
        Ok(index.map(|index| &self.history.features[index]))
    }
}

fn claimed_hole_profiles<'a>(
    ctx: &DecodeContext<'_>,
    features: &'a [crate::records::Feature],
) -> Result<HashSet<&'a str>, CodecError> {
    const OPERATION: &str = "index SLDPRT claimed hole profiles";
    let mut claims = HashSet::new();
    let mut source_items = features.into_iter();
    while let Some(feature) = ctx.next_charged(&mut source_items, OPERATION)? {
        let Some(children) = ctx.get_btree_map(
            &feature.properties,
            "DissectableChildren",
            "find SLDPRT hole property",
        )?
        else {
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
    records: &HoleHistoryIndex<'a, '_>,
    lanes: &[FeatureInputLane],
    object_names: &[HoleLaneNames<'_, '_>],
    object_orders: &[Vec<(u64, usize)>],
) -> Result<Option<(&'a crate::records::Feature, u8)>, CodecError> {
    const OPERATION: &str = "resolve SLDPRT hole profile ownership";
    let mut source = None;
    for (lane, names) in ctx.admit_iter(lanes, OPERATION)?.zip(object_names) {
        let Some(candidate) = hole_position_sketch_source(ctx, feature, lane, names)? else {
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
    let unique_position = || -> Result<Option<&crate::records::Feature>, CodecError> {
        let sourced = match FeatureSource::from_value(source) {
            Some(source) => ctx
                .get_hash_map(&records.by_source, &source, OPERATION)?
                .map_or(&[][..], Vec::as_slice),
            None => &[],
        };
        let ordinals = ctx
            .get_hash_map(&records.by_ordinal, &source, OPERATION)?
            .map_or(&[][..], Vec::as_slice);
        let mut selected = None;
        let mut remaining = sourced.iter().chain(ordinals);
        while let Some(&index) = ctx.next_charged(&mut remaining, OPERATION)? {
            if classify(&history.features[index]) != Some(FeatureClass::Sketch) {
                continue;
            }
            if selected.is_some_and(|previous| previous != index) {
                return Ok(None);
            }
            selected = Some(index);
        }
        Ok(selected.map(|index| &history.features[index]))
    };
    let position = unique_position()?;
    let serialized_successor =
        || -> Result<Option<(&'a crate::records::Feature, u8)>, CodecError> {
            let Some(position) = position else {
                return Ok(None);
            };
            let mut profile: Option<&crate::records::Feature> = None;
            for (lane_index, (lane, names)) in ctx
                .admit_iter(lanes, OPERATION)?
                .zip(object_names)
                .enumerate()
            {
                if hole_position_sketch_source(ctx, feature, lane, names)? != Some(source) {
                    continue;
                }
                let Some(position_offset) = names.names.of(ctx, position)?.map(|name| name.offset)
                else {
                    return Ok(None);
                };
                let objects = &object_orders[lane_index];
                let next = ctx.partition_point(
                    objects,
                    |(offset, _)| Ok(*offset <= position_offset),
                    OPERATION,
                )?;
                let Some(&(offset, feature_index)) = objects.get(next) else {
                    return Ok(None);
                };
                let successor = &history.features[feature_index];
                if objects
                    .get(next + 1)
                    .is_some_and(|(next_offset, _)| *next_offset == offset)
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
    let mut first = None;
    let mut ambiguous = false;
    let source_group = |source: Option<u32>| -> Result<&[usize], CodecError> {
        match source.and_then(FeatureSource::from_value) {
            Some(source) => Ok(ctx
                .get_hash_map(&records.by_source, &source, OPERATION)?
                .map_or(&[][..], Vec::as_slice)),
            None => Ok(&[]),
        }
    };
    let mut candidates = source_group(adjacent_sources[0])?
        .iter()
        .chain(source_group(adjacent_sources[1])?);
    while let Some(&index) = ctx.next_charged(&mut candidates, OPERATION)? {
        let candidate = &history.features[index];
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
    let mut bounded = None;
    let first = ctx.partition_point(
        &records.numeric_sources,
        |(source, _)| Ok(*source <= lower),
        OPERATION,
    )?;
    let last = ctx.partition_point(
        &records.numeric_sources,
        |(source, _)| Ok(*source < upper),
        OPERATION,
    )?;
    let mut candidates = records.numeric_sources[first..last.max(first)].iter();
    while let Some(&(_, index)) = ctx.next_charged(&mut candidates, OPERATION)? {
        let candidate = &history.features[index];
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
    let Some(position) = position else {
        return Ok(None);
    };
    let adjacent_ordinals = [
        position.ordinal.checked_sub(1),
        position.ordinal.checked_add(1),
    ];
    let mut first = None;
    let ordinal_group = |ordinal: Option<u32>| -> Result<&[usize], CodecError> {
        match ordinal {
            Some(ordinal) => Ok(ctx
                .get_hash_map(&records.by_ordinal, &ordinal, OPERATION)?
                .map_or(&[][..], Vec::as_slice)),
            None => Ok(&[]),
        }
    };
    let mut candidates = ordinal_group(adjacent_ordinals[0])?
        .iter()
        .chain(ordinal_group(adjacent_ordinals[1])?);
    while let Some(&index) = ctx.next_charged(&mut candidates, OPERATION)? {
        let candidate = &history.features[index];
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
    records: &HoleHistoryIndex<'a, '_>,
) -> Result<Option<(&'a crate::records::Feature, u8)>, CodecError> {
    let (Some(first_ordinal), Some(second_ordinal)) = (
        feature.ordinal.checked_add(1),
        feature.ordinal.checked_add(2),
    ) else {
        return Ok(None);
    };
    let ordinals = [first_ordinal, second_ordinal];
    let unique_child = |ordinal| -> Result<Option<&crate::records::Feature>, CodecError> {
        let children = ctx
            .get_hash_map(
                &records.by_ordinal,
                &ordinal,
                "find SLDPRT hole profile child",
            )?
            .map_or(&[][..], Vec::as_slice);
        Ok(match children {
            [index] => Some(&history.features[*index]),
            _ => None,
        })
    };
    let (Some(first_child), Some(second_child)) =
        (unique_child(ordinals[0])?, unique_child(ordinals[1])?)
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
    let mut source_items = histories.into_iter();
    while let Some(history) = ctx.next_charged(&mut source_items, OPERATION)? {
        let mut history_storage = ctx.reserve_scoped(0, OPERATION)?;
        let mut features_by_id = HashMap::new();
        let mut features_by_source = BTreeMap::new();
        let mut source_items = history.features.iter();
        while let Some(feature) = ctx.next_charged(&mut source_items, OPERATION)? {
            history_storage.with_storage(|| {
                ctx.insert_hash_map(&mut features_by_id, feature.id.as_str(), feature, OPERATION)
            })?;
            if let Some(source) = feature.source_id {
                history_storage.with_storage(|| {
                    ctx.insert_btree_map(&mut features_by_source, source, feature, OPERATION)
                })?;
            }
        }
        let mut candidates_storage = ctx.reserve_scoped(0, OPERATION)?;
        let mut candidates = HashMap::<String, Option<f64>>::new();
        let mut source_items = lanes.into_iter();
        while let Some(lane) = ctx.next_charged(&mut source_items, OPERATION)? {
            let mut source_items = lane.surface_selections.iter();
            while let Some(selection) = ctx.next_charged(&mut source_items, OPERATION)? {
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
                let mut producers = selection
                    .producer_feature_refs
                    .iter()
                    .chain(selection.terminal_feature_ref.iter());
                while let Some(producer) = ctx.next_charged(&mut producers, OPERATION)? {
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
                        let identity = ctx.copy_retained_text(&thread.id, OPERATION)?;
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
        let mut source_items = history.features.iter_mut();
        while let Some(feature) = ctx.next_charged(&mut source_items, OPERATION)? {
            if ctx.contains_key_btree_map(
                &feature.parameters,
                "D2",
                "find SLDPRT thread diameter",
            )? {
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
    let (mut projection, mut projection_storage) = ctx.with_scoped_storage(OPERATION, || {
        crate::records::charged_clone::clone_histories_charged(
            ctx,
            histories,
            "clone SLDPRT cosmetic thread histories",
        )
    })?;
    projection_storage
        .with_storage(|| enrich_history_hole_constructions(ctx, &mut projection, lanes))?;
    projection_storage
        .with_storage(|| enrich_history_cosmetic_thread_diameters(ctx, &mut projection, lanes))?;
    let mut fallback_storage = ctx.reserve_scoped(0, OPERATION)?;
    let mut fallback_parameters = HashMap::new();
    for iteration_history in ctx.admit_iter(&projection[..], "scan SLDPRT holes source records")? {
        for feature in ctx.admit_iter(&iteration_history.features, "scan SLDPRT holes records")? {
            let Some(diameter) =
                ctx.get_btree_map(&feature.parameters, "D2", "find SLDPRT thread diameter")?
            else {
                continue;
            };
            fallback_storage.with_storage(|| {
                ctx.insert_hash_map(
                    &mut fallback_parameters,
                    feature.id.as_str(),
                    diameter,
                    OPERATION,
                )
            })?;
        }
    }
    for history_index in ctx.admit_iter(&(0..histories.len()), OPERATION)? {
        let history = &mut histories[history_index];
        for feature_index in ctx.admit_iter(&(0..history.features.len()), OPERATION)? {
            let feature = &mut history.features[feature_index];
            if ctx.contains_key_btree_map(
                &feature.parameters,
                "D2",
                "find SLDPRT thread diameter",
            )? {
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
            let value = ctx.copy_retained_text(diameter, OPERATION)?;
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
    let has_source_dimensions = ctx.any_by(
        &profile.content,
        |content| {
            Ok(matches!(
                content,
                crate::records::FeatureContent::Dimension(_)
            ))
        },
        "scan SLDPRT holes records",
    )?;
    let mut storage = ctx.reserve_scoped(0, "SLDPRT axial hole profile workspace")?;
    let mut diameters = Vec::new();
    let mut angles = Vec::new();
    let mut lengths = Vec::new();
    let mut flat_bottom = false;
    let mut add_expression = |value: &str| -> Result<(), CodecError> {
        crate::history::literals::admit_literal(ctx, value, OPERATION)?;
        if let Some(value) = crate::history::literals::strip_diameter_modifier(value)
            .and_then(crate::history::literals::parse_dimension_length_mm)
            .and_then(|value| cadmpeg_ir::scalar::PositiveLength::try_from(value).ok())
        {
            storage.with_storage(|| ctx.push_vec(&mut diameters, value, OPERATION))?;
        }
        if let Some(angle) = crate::history::literals::parse_bounded_angle_rad(value) {
            storage.with_storage(|| ctx.push_vec(&mut angles, angle, OPERATION))?;
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
                storage.with_storage(|| ctx.push_vec(&mut lengths, length, OPERATION))?;
            }
        }
        Ok(())
    };
    if has_source_dimensions {
        let mut source_items = profile.content.iter();
        while let Some(content) = ctx.next_charged(&mut source_items, OPERATION)? {
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
    ctx.dedup_by(
        &mut diameters,
        |left, right| Ok((left.get() - right.get()).abs() <= EPS_HOLE_GEOMETRY),
        OPERATION,
    )?;
    ctx.sort_unstable_by_key(&mut angles, |value| value.get(), f64::total_cmp, OPERATION)?;
    ctx.dedup_by(
        &mut angles,
        |left, right| Ok((left.get() - right.get()).abs() <= EPS_HOLE_EXACT_GEOMETRY),
        OPERATION,
    )?;
    ctx.sort_unstable_by_key(&mut lengths, |value| value.get(), f64::total_cmp, OPERATION)?;
    ctx.dedup_by(
        &mut lengths,
        |left, right| Ok((left.get() - right.get()).abs() <= EPS_HOLE_GEOMETRY),
        OPERATION,
    )?;
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
    let mut source_items = entities.into_iter();
    while let Some(entity) = ctx.next_charged(&mut source_items, OPERATION)? {
        if !ctx.equal(&(entity.sketch), sketch, "compare SLDPRT holes records")?
            || entity.construction
        {
            continue;
        }
        match *entity.geometry.definition() {
            SketchGeometryDefinition::Line { start, end } => {
                storage.with_storage(|| ctx.push_vec(&mut lines, (start, end), OPERATION))?;
            }
            SketchGeometryDefinition::Point { position } => {
                storage.with_storage(|| ctx.push_vec(&mut points, position, OPERATION))?;
            }
            _ => {}
        }
    }
    let same_point = |left: Point2, right: Point2| {
        (left.u - right.u).abs() <= DISPLAY_DIMENSION_TOLERANCE_MM
            && (left.v - right.v).abs() <= DISPLAY_DIMENSION_TOLERANCE_MM
    };
    let has_line = |first: Point2, second: Point2| -> Result<bool, CodecError> {
        ctx.any_by(
            &lines,
            |(start, end)| {
                Ok(
                    (same_point(start.get(), first) && same_point(end.get(), second))
                        || (same_point(start.get(), second) && same_point(end.get(), first)),
                )
            },
            OPERATION,
        )
    };
    let has_point_pair = |first: Point2, second: Point2| -> Result<bool, CodecError> {
        Ok(ctx.any_by(
            &points,
            |point| Ok(same_point(point.get(), first)),
            OPERATION,
        )? && ctx.any_by(
            &points,
            |point| Ok(same_point(point.get(), second)),
            OPERATION,
        )?)
    };
    let profile_translation = |edges: &[(Point2, Point2)],
                               minimum_lines: usize|
     -> Result<Option<Point2>, CodecError> {
        let mut actuals = lines
            .iter()
            .flat_map(|(first, second)| [*first, *second])
            .chain(points.iter().copied());
        while let Some(actual) = ctx.next_charged(&mut actuals, OPERATION)? {
            for expected in edges.iter().flat_map(|(first, second)| [*first, *second]) {
                let translation = Point2::new(actual.u - expected.u, actual.v - expected.v);
                let mut materialized_lines = 0usize;
                for (first, second) in edges {
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
                for (first, second) in edges {
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
                    for terminal_overrun in terminal_overruns {
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
    let mut names_storage = ctx.reserve_scoped(0, "index SLDPRT hole object names")?;
    let object_names = hole_lane_names(ctx, &mut names_storage, lanes)?;
    let (mut enriched_histories, mut enriched_storage) =
        ctx.with_scoped_storage("SLDPRT enriched hole history workspace", || {
            crate::records::charged_clone::clone_histories_charged(
                ctx,
                histories,
                "SLDPRT unowned incomplete-hole histories",
            )
        })?;
    enriched_storage.with_storage(|| {
        crate::history::configuration::enrich_history_parameters_semantic(
            ctx,
            &mut enriched_histories,
            lanes,
        )
    })?;
    let (mut ownership_histories, mut ownership_storage) =
        ctx.with_scoped_storage("SLDPRT hole ownership history workspace", || {
            crate::records::charged_clone::clone_histories_charged(
                ctx,
                &enriched_histories,
                "SLDPRT unowned incomplete-hole histories",
            )
        })?;
    ownership_storage
        .with_storage(|| enrich_history_hole_constructions(ctx, &mut ownership_histories, lanes))?;
    let histories = enriched_histories.as_slice();
    let (records, _records_storage) = ctx.with_scoped_storage(OPERATION, || {
        ctx.try_collect_vec(
            histories
                .iter()
                .map(|history| HoleHistoryIndex::new(ctx, history)),
            OPERATION,
        )
    })?;
    let positions = PositionSketches::new(ctx, histories, &object_names)?;
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
    let mut source_items = features.iter();
    while let Some(feature) = ctx.next_charged(&mut source_items, OPERATION)? {
        let Some(native) = feature.native_ref.as_deref() else {
            continue;
        };
        match feature.evaluation.definition() {
            FeatureDefinition::Operation(FeatureOperation::Hole { shape, extent, .. })
                if !incomplete(&shape.diameter(), extent, shape.construction()) =>
            {
                model_sketch_storage.with_storage(|| {
                    ctx.insert_hash_set(&mut complete_native_holes, native, OPERATION)
                })?;
            }
            FeatureDefinition::Operation(FeatureOperation::Sketch {
                sketch: cadmpeg_ir::features::SketchFeatureBinding::Planar(Some(sketch)),
            }) if !ctx.contains_key_hash_map(
                &model_sketches,
                native,
                "resolve SLDPRT holes keys",
            )? =>
            {
                model_sketch_storage.with_storage(|| {
                    let native = ctx.copy_retained_text(native, OPERATION)?;
                    let sketch = sketch.try_clone_for_decode(ctx, OPERATION)?;
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
        let mut source_items = history.features.iter();
        while let Some(feature) = ctx.next_charged(&mut source_items, OPERATION)? {
            model_sketch_storage.with_storage(|| {
                ctx.insert_hash_map(
                    &mut native_histories,
                    feature.id.as_str(),
                    history_index,
                    OPERATION,
                )
            })?;
        }
    }
    let mut unowned_incomplete_holes = model_sketch_storage.with_storage(|| {
        ctx.collect_indexed_vec(
            histories.len(),
            "SLDPRT unowned incomplete-hole histories",
            |_| Ok(Vec::<(&str, u32)>::new()),
        )
    })?;
    let mut source_items = features.iter();
    while let Some(feature) = ctx.next_charged(&mut source_items, OPERATION)? {
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
        let Some(index) = ctx
            .get_hash_map(&records[history_index].by_id, native, OPERATION)?
            .and_then(|group| group.first())
        else {
            continue;
        };
        let native_feature = &histories[history_index].features[*index];
        if !ctx.contains_key_btree_map(
            &native_feature.properties,
            "DissectableChildren",
            "find SLDPRT hole property",
        )? {
            model_sketch_storage.with_storage(|| {
                ctx.reserve_vec(
                    &mut unowned_incomplete_holes[history_index],
                    1,
                    "SLDPRT unowned incomplete holes",
                )
            })?;
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
        let mut history_storage = ctx.reserve_scoped(0, OPERATION)?;
        let mut claimed_profiles = HashSet::new();
        let mut source_items = history.features.iter();
        while let Some(feature) = ctx.next_charged(&mut source_items, OPERATION)? {
            if let Some(children) = ctx.get_btree_map(
                &feature.properties,
                "DissectableChildren",
                "find SLDPRT hole property",
            )? {
                history_storage.with_storage(|| {
                    collect_claimed_hole_profiles(
                        ctx,
                        &records[hole_index],
                        children,
                        &mut claimed_profiles,
                    )
                })?;
            }
        }
        let mut source_items = ownership_history.features.iter();
        while let Some(feature) = ctx.next_charged(&mut source_items, OPERATION)? {
            if ctx.contains_hash_set(
                &complete_native_holes,
                feature.id.as_str(),
                "resolve SLDPRT holes keys",
            )? {
                if let Some(children) = ctx.get_btree_map(
                    &feature.properties,
                    "DissectableChildren",
                    "find SLDPRT hole property",
                )? {
                    history_storage.with_storage(|| {
                        collect_claimed_hole_profiles(
                            ctx,
                            &records[hole_index],
                            children,
                            &mut claimed_profiles,
                        )
                    })?;
                }
            }
        }
        let mut profiles = Vec::new();
        let mut source_items = history.features.iter().enumerate().into_iter();
        while let Some((index, profile)) = ctx.next_charged(&mut source_items, OPERATION)? {
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
            let (construction, _construction_storage) =
                ctx.with_scoped_storage(OPERATION, || {
                    profiled_hole_construction_with_evidence(
                        ctx,
                        profile,
                        sketch,
                        entities,
                        ProfileEvidence::AxialTopology,
                    )
                })?;
            let Some(construction) = construction else {
                continue;
            };
            history_storage.with_storage(|| ctx.reserve_vec(&mut profiles, 1, OPERATION))?;
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
        let mut source_items = holes.iter().zip(profiles).into_iter();
        while let Some(((hole, _), (_, _, construction))) =
            ctx.next_charged(&mut source_items, OPERATION)?
        {
            model_sketch_storage.with_storage(|| {
                ctx.insert_hash_map(&mut fallback_constructions, *hole, construction, OPERATION)
            })?;
        }
    }
    drop(complete_native_holes);
    let mut source_items = features.iter_mut();
    while let Some(feature) = ctx.next_charged(&mut source_items, OPERATION)? {
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
        let Some(index) = ctx
            .get_hash_map(&records[history_index].by_id, native_id, OPERATION)?
            .and_then(|group| group.first())
        else {
            continue;
        };
        let native = &histories[history_index].features[*index];
        let position = hole_position_feature(ctx, native, &positions, lanes, &object_names)?
            .map(|feature| feature.id.as_str());
        let mut direct = None;
        if let Some(children) =
            ctx.get_btree_map(&native.properties, "DissectableChildren", OPERATION)?
        {
            for source in hole_child_tokens(ctx, children)? {
                let source = ctx.trim_text(source?, OPERATION)?;
                let Some(profile) = records[history_index].child(ctx, source)? else {
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
                if let Some(construction) = ctx
                    .with_scoped_storage(OPERATION, || {
                        profiled_hole_construction(ctx, profile, sketch, entities)
                    })?
                    .0
                {
                    if direct.is_some() {
                        direct = None;
                        break;
                    }
                    direct = Some(construction);
                }
            }
        }
        let construction = if direct.is_some()
            || ctx.contains_key_btree_map(&native.properties, "DissectableChildren", OPERATION)?
        {
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
    records: &HoleHistoryIndex<'a, '_>,
    children: &str,
    profiles: &mut HashSet<&'a str>,
) -> Result<(), CodecError> {
    const OPERATION: &str = "index SLDPRT claimed axial hole profiles";
    for child in hole_child_tokens(ctx, children)? {
        let child = ctx.trim_text(child?, OPERATION)?;
        if child.is_empty() {
            continue;
        }
        let Some(profile) = records.child(ctx, child)? else {
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
    let mut names_storage = ctx.reserve_scoped(0, "index SLDPRT hole object names")?;
    let object_names = hole_lane_names(ctx, &mut names_storage, lanes)?;
    let positions = PositionSketches::new(ctx, histories, &object_names)?;
    let (records, _records_storage) =
        ctx.with_scoped_storage("index SLDPRT hole history records", || {
            ctx.try_collect_vec(
                histories
                    .iter()
                    .map(|history| HoleHistoryIndex::new(ctx, history)),
                "index SLDPRT hole history records",
            )
        })?;
    let mut native_features = BTreeMap::new();
    for iteration_history in ctx.admit_iter(histories, "scan SLDPRT holes source records")? {
        for feature in ctx.admit_iter(&iteration_history.features, "scan SLDPRT holes records")? {
            names_storage.with_storage(|| {
                ctx.insert_btree_map(
                    &mut native_features,
                    feature.id.as_str(),
                    feature,
                    OPERATION,
                )
            })?;
        }
    }
    let mut model_sketch_features_storage = ctx.reserve_scoped(0, "resolve SLDPRT holes keys")?;
    let mut model_sketch_features = HashMap::new();
    let mut source_items = features.iter();
    while let Some(feature) = ctx.next_charged(&mut source_items, OPERATION)? {
        let FeatureDefinition::Operation(FeatureOperation::Sketch {
            sketch: cadmpeg_ir::features::SketchFeatureBinding::Planar(Some(sketch)),
        }) = feature.evaluation.definition()
        else {
            continue;
        };
        let Some(native) = &feature.native_ref else {
            continue;
        };
        let (binding, binding_storage) = ctx.with_scoped_storage(OPERATION, || {
            Ok::<_, CodecError>((
                feature.id.try_clone_for_decode(ctx, OPERATION)?,
                sketch.try_clone_for_decode(ctx, OPERATION)?,
            ))
        })?;
        if let Some(indexed) = ctx.get_mut_hash_map(
            &mut model_sketch_features,
            native.as_str(),
            "resolve SLDPRT holes keys",
        )? {
            *indexed = (binding, binding_storage);
        } else {
            model_sketch_features_storage.with_storage(|| {
                let native_key = ctx.copy_retained_text(native, OPERATION)?;
                ctx.insert_hash_map(
                    &mut model_sketch_features,
                    native_key,
                    (binding, binding_storage),
                    "resolve SLDPRT holes keys",
                )
            })?;
        }
    }
    let mut lookup_storage = ctx.reserve_scoped(0, OPERATION)?;
    let mut first_sketch = HashMap::new();
    for sketch in ctx.admit_iter(sketches, OPERATION)? {
        lookup_storage.with_storage(|| {
            ctx.entry_hash_map(&mut first_sketch, sketch.id.as_str(), OPERATION)?
                .or_insert(sketch);
            Ok::<_, CodecError>(())
        })?;
    }
    let mut points = HashMap::new();
    for entity in ctx.admit_iter(sketch_entities, OPERATION)? {
        let SketchGeometryDefinition::Point { position } = entity.geometry.definition() else {
            continue;
        };
        let Some(native) = entity.native_ref.as_deref() else {
            continue;
        };
        lookup_storage.with_storage(|| {
            match ctx.entry_hash_map(&mut points, (entity.sketch.as_str(), native), OPERATION)? {
                std::collections::hash_map::Entry::Vacant(entry) => {
                    entry.insert(Some(position.get()));
                }
                std::collections::hash_map::Entry::Occupied(mut entry) => {
                    entry.insert(None);
                }
            }
            Ok::<_, CodecError>(())
        })?;
    }
    let (markers, _markers_storage) = ctx.with_scoped_storage(OPERATION, || {
        ctx.try_collect_vec(
            lanes.iter().map(|lane| HoleMarkers::new(ctx, lane)),
            OPERATION,
        )
    })?;
    let mut source_items = features.iter_mut();
    while let Some(feature) = ctx.next_charged(&mut source_items, OPERATION)? {
        let (projection, _projection_storage) =
            ctx.with_scoped_storage(OPERATION, || -> Result<_, CodecError> {
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
                    let position_feature =
                        match hole_position_feature(ctx, native, &positions, lanes, &object_names)?
                        {
                            Some(position) => Some(position),
                            None => direct_hole_position_feature(
                                ctx,
                                native,
                                &records,
                                |id| {
                                    Ok(ctx
                                        .get_hash_map(&model_sketch_features, id, OPERATION)?
                                        .map(|((_, sketch), _)| sketch))
                                },
                                sketch_entities,
                            )?,
                        };
                    let Some(position_feature) = position_feature else {
                        break 'feature_edit;
                    };
                    let Some(((position_dependency, sketch_id), _)) = ctx.get_hash_map(
                        &model_sketch_features,
                        position_feature.id.as_str(),
                        "resolve SLDPRT holes keys",
                    )?
                    else {
                        break 'feature_edit;
                    };
                    let Some(&sketch) =
                        ctx.get_hash_map(&first_sketch, sketch_id.as_str(), OPERATION)?
                    else {
                        break 'feature_edit;
                    };
                    let Some((origin, normal, u_axis)) = sketch.resolved_placement() else {
                        break 'feature_edit;
                    };
                    let mut authored_markers = Vec::new();
                    for (lane, markers) in ctx.admit_iter(lanes, OPERATION)?.zip(&markers) {
                        if !ctx.equal(&lane.configuration, &sketch.configuration, OPERATION)? {
                            continue;
                        }
                        for marker in ctx.admit_iter(
                            ctx.get_hash_map(
                                &markers.by_feature,
                                position_feature.id.as_str(),
                                OPERATION,
                            )?
                            .map_or(&[][..], Vec::as_slice),
                            OPERATION,
                        )? {
                            if marker.object_index().is_some()
                                && marker.coordinates_m.is_some()
                                && matches!(
                                    marker.kind(),
                                    SketchInputKind::Point | SketchInputKind::ConstrainedPoint
                                )
                            {
                                ctx.reserve_vec(&mut authored_markers, 1, OPERATION)?;
                                authored_markers.push(*marker);
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
                        let mut unindexed_locus: Option<(
                            &crate::records::SketchInputEntity,
                            [f64; 2],
                        )> = None;
                        let mut complete_unindexed_encoding = true;
                        for (lane, markers) in ctx.admit_iter(lanes, OPERATION)?.zip(&markers) {
                            if !ctx.equal(&lane.configuration, &sketch.configuration, OPERATION)? {
                                continue;
                            }
                            let position_marker =
                        |marker: &crate::records::SketchInputEntity| -> Result<bool, CodecError> {
                            Ok(marker.coordinates_m.is_some())
                        };
                            let mut indexed_markers = 0usize;
                            for marker in ctx.admit_iter(
                                ctx.get_hash_map(
                                    &markers.by_feature,
                                    position_feature.id.as_str(),
                                    OPERATION,
                                )?
                                .map_or(&[][..], Vec::as_slice),
                                OPERATION,
                            )? {
                                if position_marker(marker)? && marker.object_index().is_some() {
                                    indexed_markers =
                                        indexed_markers.checked_add(1).ok_or_else(|| {
                                            ctx.refuse_codec_limit(
                                                OPERATION,
                                                u64::MAX - 1,
                                                u64::MAX,
                                            )
                                        })?;
                                }
                            }
                            let pairs = ctx
                                .get_hash_map(
                                    &markers.paired,
                                    position_feature.id.as_str(),
                                    OPERATION,
                                )?
                                .map_or(&[][..], Vec::as_slice);
                            complete_alternate_encoding &= pairs.len() == indexed_markers;
                            for &(marker, coordinates) in ctx.admit_iter(pairs, OPERATION)? {
                                ctx.insert_hash_map(
                                    &mut paired_marker_coordinates,
                                    marker.id(),
                                    coordinates,
                                    OPERATION,
                                )?;
                                ctx.push_vec(&mut authored_markers, marker, OPERATION)?;
                            }
                            let mut points_only = indexed_markers == 0;
                            if points_only {
                                let mut remaining = ctx
                                    .get_hash_map(
                                        &markers.by_feature,
                                        position_feature.id.as_str(),
                                        OPERATION,
                                    )?
                                    .map_or(&[][..], Vec::as_slice)
                                    .iter();
                                while let Some(marker) =
                                    ctx.next_charged(&mut remaining, OPERATION)?
                                {
                                    if position_marker(marker)?
                                        && !matches!(
                                            marker.kind(),
                                            SketchInputKind::Point
                                                | SketchInputKind::ConstrainedPoint
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
                                let mut remaining = ctx
                                    .get_hash_map(
                                        &markers.by_feature,
                                        position_feature.id.as_str(),
                                        OPERATION,
                                    )?
                                    .map_or(&[][..], Vec::as_slice)
                                    .iter();
                                while let Some(marker) =
                                    ctx.next_charged(&mut remaining, OPERATION)?
                                {
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
                                    locus = Some((*marker, [u, v]));
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
                    let mut remaining = authored_markers.iter();
                    while let Some(marker) =
                        ctx.next_charged(&mut remaining, "scan SLDPRT holes records")?
                    {
                        let entity = match ctx.get_hash_map(
                            &points,
                            &(sketch_id.as_str(), marker.id()),
                            OPERATION,
                        )? {
                            Some(Some(point)) => Some(*point),
                            Some(None) => {
                                ctx.clear_vec(&mut resolved, OPERATION)?;
                                break;
                            }
                            None => None,
                        };
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
                                let native = quantize(
                                    Point2::new(u * NATIVE_TO_IR, v * NATIVE_TO_IR),
                                    QUANTUM,
                                );
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
                Ok(projection)
            })?;
        if let Some((resolved, dependency)) = projection {
            let resolved = ctx.try_collect_vec(
                resolved
                    .iter()
                    .map(|placement| placement.try_clone_for_decode(ctx, OPERATION)),
                OPERATION,
            )?;
            feature.evaluation.edit(|definition, _| {
                if let FeatureDefinition::Operation(FeatureOperation::Hole { placements, .. }) =
                    definition
                {
                    *placements = Some(resolved);
                }
            });
            if !ctx.contains(feature.dependencies.as_slice(), dependency, OPERATION)? {
                let dependency = dependency.try_clone_for_decode(ctx, OPERATION)?;
                feature.dependencies.insert(ctx, dependency, OPERATION)?;
            }
        }
    }
    Ok(())
}

struct HoleMarkers<'lane, 'ctx> {
    by_feature: HashMap<&'lane str, Vec<&'lane crate::records::SketchInputEntity>>,
    paired: HashMap<&'lane str, Vec<(&'lane crate::records::SketchInputEntity, [f64; 2])>>,
    _storage: cadmpeg_core::decode::ScopedReservation<'ctx>,
}

impl<'lane, 'ctx> HoleMarkers<'lane, 'ctx> {
    fn new(
        ctx: &'ctx DecodeContext<'_>,
        lane: &'lane FeatureInputLane,
    ) -> Result<Self, CodecError> {
        const OPERATION: &str = "index SLDPRT hole position markers";
        let mut storage = ctx.reserve_scoped(0, OPERATION)?;
        let mut by_feature = HashMap::new();
        let mut paired = HashMap::new();
        for marker in ctx.admit_iter(&lane.sketch_entities, OPERATION)? {
            if let Some(feature) = marker.feature_ref.as_deref() {
                storage.with_storage(|| {
                    ctx.push_hash_group(&mut by_feature, feature, marker, OPERATION, OPERATION)
                })?;
            }
        }
        let window = std::num::NonZeroUsize::new(2)
            .ok_or_else(|| CodecError::malformed("empty paired object locus window"))?;
        for pair in ctx
            .admit_iter(&lane.sketch_entities, OPERATION)?
            .windows(window)
        {
            let object = &pair[0];
            let anchor = &pair[1];
            let (Some(feature), Some(coordinates)) = (
                object.feature_ref.as_deref(),
                object
                    .coordinates_m
                    .map(cadmpeg_ir::units::FiniteVector::get),
            ) else {
                continue;
            };
            if ctx.equal(&anchor.feature_ref.as_deref(), &Some(feature), OPERATION)?
                && object.object_index().is_some()
                && anchor.object_index().is_none()
                && anchor.kind() == SketchInputKind::Point
                && anchor
                    .coordinates_m
                    .is_some_and(|coordinates| coordinates == [0.0, 0.0])
            {
                storage.with_storage(|| {
                    ctx.push_hash_group(
                        &mut paired,
                        feature,
                        (object, coordinates),
                        OPERATION,
                        OPERATION,
                    )
                })?;
            }
        }
        Ok(Self {
            by_feature,
            paired,
            _storage: storage,
        })
    }
}

struct PositionSketches<'history, 'ctx> {
    by_source: HashMap<u32, Option<&'history crate::records::Feature>>,
    _storage: cadmpeg_core::decode::ScopedReservation<'ctx>,
}

impl<'history, 'ctx> PositionSketches<'history, 'ctx> {
    fn new(
        ctx: &'ctx DecodeContext<'_>,
        histories: &'history [crate::records::FeatureHistory],
        names: &[HoleLaneNames<'_, '_>],
    ) -> Result<Self, CodecError> {
        const OPERATION: &str = "index SLDPRT hole position sketches";
        let mut storage = ctx.reserve_scoped(0, OPERATION)?;
        let mut by_source = HashMap::new();
        for history in ctx.admit_iter(histories, OPERATION)? {
            for feature in ctx.admit_iter(&history.features, OPERATION)? {
                if classify(feature) != Some(FeatureClass::Sketch) {
                    continue;
                }
                let mut feature_storage = ctx.reserve_scoped(0, OPERATION)?;
                let mut seen = HashSet::new();
                let source_names = if feature.source_value().is_some() {
                    names.get(..1).unwrap_or(&[])
                } else {
                    names
                };
                for names in ctx.admit_iter(source_names, OPERATION)? {
                    let source = match feature.source_value() {
                        Some(source) => Some(source),
                        None => names
                            .names
                            .of(ctx, feature)?
                            .and_then(|name| name.object_id.and_then(ObjectId::value)),
                    };
                    let Some(source) = source else {
                        continue;
                    };
                    if feature.source_value().is_none()
                        && !feature_storage
                            .with_storage(|| ctx.insert_hash_set(&mut seen, source, OPERATION))?
                    {
                        continue;
                    }
                    storage.with_storage(|| {
                        match ctx.entry_hash_map(&mut by_source, source, OPERATION)? {
                            std::collections::hash_map::Entry::Vacant(entry) => {
                                entry.insert(Some(feature));
                            }
                            std::collections::hash_map::Entry::Occupied(mut entry) => {
                                entry.insert(None);
                            }
                        }
                        Ok::<_, CodecError>(())
                    })?;
                }
            }
        }
        Ok(Self {
            by_source,
            _storage: storage,
        })
    }
}

fn hole_position_feature<'a>(
    ctx: &DecodeContext<'_>,
    hole: &crate::records::Feature,
    positions: &PositionSketches<'a, '_>,
    lanes: &[FeatureInputLane],
    object_names: &[HoleLaneNames<'_, '_>],
) -> Result<Option<&'a crate::records::Feature>, CodecError> {
    const OPERATION: &str = "resolve SLDPRT hole position source";
    let mut source = None;
    let mut lane_pairs = lanes.iter().zip(object_names);
    while let Some((lane, names)) = ctx.next_charged(&mut lane_pairs, OPERATION)? {
        let Some(candidate) = hole_position_sketch_source(ctx, hole, lane, names)? else {
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
    Ok(ctx
        .get_hash_map(&positions.by_source, &source, OPERATION)?
        .copied()
        .flatten())
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
    let mut names_storage = ctx.reserve_scoped(0, "index SLDPRT hole object names")?;
    let object_names = hole_lane_names(ctx, &mut names_storage, lanes)?;
    let mut histories = histories.iter();
    while let Some(history) = ctx.next_charged(&mut histories, OPERATION)? {
        if let Some(native) = ctx.find_by(
            &history.features,
            |native| ctx.equal(native.id.as_str(), native_ref, OPERATION),
            OPERATION,
        )? {
            return ctx.any_by(lanes.iter().zip(&object_names), |(lane, names)| Ok(hole_position_sketch_source(ctx, native, lane, names)?.is_some()), OPERATION);
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
    let mut names_storage = ctx.reserve_scoped(0, "index SLDPRT hole object names")?;
    let object_names = hole_lane_names(ctx, &mut names_storage, lanes)?;
    let positions = PositionSketches::new(ctx, histories, &object_names)?;
    let mut native_features = BTreeMap::new();
    for iteration_history in ctx.admit_iter(histories, "scan SLDPRT holes source records")? {
        for feature in ctx.admit_iter(&iteration_history.features, "scan SLDPRT holes records")? {
            let key = feature.id.as_str();
            names_storage.with_storage(|| {
                ctx.insert_btree_map(&mut native_features, key, feature, INDEX_OPERATION)
            })?;
        }
    }
    let mut lookup_storage = ctx.reserve_scoped(0, INDEX_OPERATION)?;
    let mut position_sketches = HashMap::new();
    for (index, feature) in ctx.admit_iter(&*features, INDEX_OPERATION)?.enumerate() {
        if matches!(
            feature.evaluation.definition(),
            FeatureDefinition::Operation(FeatureOperation::SpatialSketch { sketch: Some(_) })
        ) {
            if let Some(native) = feature.native_ref.as_deref() {
                if let Some(indexed) =
                    ctx.get_mut_hash_map(&mut position_sketches, native, INDEX_OPERATION)?
                {
                    *indexed = index;
                } else {
                    lookup_storage.with_storage(|| {
                        let native = ctx.copy_retained_text(native, INDEX_OPERATION)?;
                        ctx.insert_hash_map(&mut position_sketches, native, index, INDEX_OPERATION)
                    })?;
                }
            }
        }
    }
    let mut first_sketch = HashMap::new();
    for sketch in ctx.admit_iter(spatial_sketches, INDEX_OPERATION)? {
        lookup_storage.with_storage(|| {
            ctx.entry_hash_map(&mut first_sketch, sketch.id.as_str(), INDEX_OPERATION)?
                .or_insert(sketch);
            Ok::<_, CodecError>(())
        })?;
    }
    let mut points = HashMap::new();
    let mut sketch_points = HashMap::new();
    for entity in ctx.admit_iter(spatial_entities, INDEX_OPERATION)? {
        let SpatialSketchGeometryDefinition::Point { position } = entity.geometry.definition()
        else {
            continue;
        };
        lookup_storage.with_storage(|| {
            ctx.push_hash_group(
                &mut sketch_points,
                entity.sketch.as_str(),
                position.get(),
                INDEX_OPERATION,
                INDEX_OPERATION,
            )
        })?;
        let Some(native) = entity.native_ref.as_deref() else {
            continue;
        };
        lookup_storage.with_storage(|| {
            match ctx.entry_hash_map(
                &mut points,
                (entity.sketch.as_str(), native),
                INDEX_OPERATION,
            )? {
                std::collections::hash_map::Entry::Vacant(entry) => {
                    entry.insert(Some(position.get()));
                }
                std::collections::hash_map::Entry::Occupied(mut entry) => {
                    entry.insert(None);
                }
            }
            Ok::<_, CodecError>(())
        })?;
    }
    let (markers, _markers_storage) = ctx.with_scoped_storage(INDEX_OPERATION, || {
        ctx.try_collect_vec(
            lanes.iter().map(|lane| HoleMarkers::new(ctx, lane)),
            INDEX_OPERATION,
        )
    })?;
    let mut source_items = 0..features.len();
    while let Some(index) =
        ctx.next_charged(&mut source_items, "project SLDPRT spatial hole positions")?
    {
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
        let Some(position_feature) =
            hole_position_feature(ctx, native, &positions, lanes, &object_names)?
        else {
            continue;
        };
        let Some(&sketch_index) = ctx.get_hash_map(
            &position_sketches,
            position_feature.id.as_str(),
            INDEX_OPERATION,
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
        let Some(&sketch) = ctx.get_hash_map(&first_sketch, sketch_id.as_str(), INDEX_OPERATION)?
        else {
            continue;
        };
        let (solution, _solution_storage) =
            ctx.with_scoped_storage(INDEX_OPERATION, || -> Result<_, CodecError> {
                let mut authored_markers = Vec::new();
                for (lane, markers) in ctx
                    .admit_iter(lanes, "scan SLDPRT holes records")?
                    .zip(&markers)
                {
                    if !ctx.equal(
                        &lane.configuration,
                        &sketch.configuration,
                        "match SLDPRT spatial position configuration",
                    )? {
                        continue;
                    }
                    let mut source_items = ctx
                        .get_hash_map(
                            &markers.by_feature,
                            position_feature.id.as_str(),
                            INDEX_OPERATION,
                        )?
                        .map_or(&[][..], Vec::as_slice)
                        .iter();
                    while let Some(marker) =
                        ctx.next_charged(&mut source_items, "scan SLDPRT spatial position markers")?
                    {
                        if marker.object_index().is_some() {
                            ctx.reserve_vec(
                                &mut authored_markers,
                                1,
                                "collect SLDPRT spatial position markers",
                            )?;
                            authored_markers.push(*marker);
                        }
                    }
                }
                let radius = diameter * 0.5;
                let radius_tolerance = (radius.abs() * EPS_HOLE_GEOMETRY).max(EPS_HOLE_GEOMETRY);
                let axis_tolerance_squared = EPS_HOLE_EXACT_GEOMETRY;
                let mut resolved = Vec::new();
                let mut ambiguous = false;
                let mut remaining = authored_markers.iter();
                while let Some(marker) =
                    ctx.next_charged(&mut remaining, "scan SLDPRT holes records")?
                {
                    let point = match ctx.get_hash_map(
                        &points,
                        &(sketch_id.as_str(), marker.id()),
                        INDEX_OPERATION,
                    )? {
                        Some(Some(point)) => *point,
                        Some(None) => {
                            ambiguous = true;
                            break;
                        }
                        None => continue,
                    };
                    let (axes, _axes_storage) = ctx.with_scoped_storage(
                        "SLDPRT spatial marker workspace",
                        || -> Result<_, CodecError> {
                            let mut axes = Vec::new();
                            let mut source_items = surfaces.into_iter();
                            while let Some(surface) = ctx.next_charged(
                                &mut source_items,
                                "scan SLDPRT spatial bore surfaces",
                            )? {
                                let candidate_axis = match &surface.geometry {
                                    SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cylinder(
                                        cylinder_surface,
                                    )) => {
                                        let origin = cylinder_surface.origin().get();
                                        let axis = FeatureDirection3::from(
                                            *cylinder_surface.frame().axis(),
                                        );
                                        let candidate = cylinder_surface.radius().get();
                                        ((candidate - radius).abs() <= radius_tolerance
                                            && point_axis_distance_squared(
                                                point,
                                                origin,
                                                axis.get(),
                                            ) <= axis_tolerance_squared)
                                            .then_some((origin, axis))
                                    }
                                    _ => None,
                                };
                                if let Some(candidate_axis) = candidate_axis {
                                    ctx.reserve_vec(
                                        &mut axes,
                                        1,
                                        "collect SLDPRT spatial bore axes",
                                    )?;
                                    axes.push(candidate_axis);
                                }
                            }
                            if axes.is_empty() {
                                let mut support_axes = Vec::new();
                                let mut source_items = surfaces.into_iter();
                                while let Some(surface) = ctx.next_charged(
                                    &mut source_items,
                                    "scan SLDPRT spatial support surfaces",
                                )? {
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
                                        [left.x.to_bits(), left.y.to_bits(), left.z.to_bits()].cmp(
                                            &[
                                                right.x.to_bits(),
                                                right.y.to_bits(),
                                                right.z.to_bits(),
                                            ],
                                        )
                                    },
                                    "sort SLDPRT spatial support axes",
                                )?;
                                ctx.dedup_by(
                                    &mut support_axes,
                                    |left, right| Ok(left.dot(*right) >= 1.0 - EPS_HOLE_GEOMETRY),
                                    "deduplicate SLDPRT spatial support axes",
                                )?;
                                if let [axis] = support_axes.as_slice() {
                                    let Some(axis) = FeatureDirection3::new(*axis) else {
                                        return Ok(None);
                                    };
                                    ctx.reserve_vec(
                                        &mut axes,
                                        1,
                                        "collect SLDPRT spatial bore axes",
                                    )?;
                                    axes.push((point, axis));
                                }
                            }
                            carrier_placements(
                                ctx,
                                ctx.admit_iter(&axes[..], "collect SLDPRT hole carrier axes")?
                                    .copied(),
                            )
                        },
                    )?;
                    let Some(axes) = axes else {
                        continue;
                    };
                    let [placement] = axes.as_slice() else {
                        ambiguous = true;
                        break;
                    };
                    ctx.reserve_vec(&mut resolved, 1, "collect SLDPRT spatial hole placements")?;
                    resolved.push(
                        placement
                            .try_clone_for_decode(ctx, "SLDPRT spatial hole placement copy")?,
                    );
                }
                if resolved.is_empty() && !ambiguous {
                    let points = ctx
                        .get_hash_map(&sketch_points, sketch_id.as_str(), INDEX_OPERATION)?
                        .map_or(&[][..], Vec::as_slice);
                    if let Some(inferred) = coplanar_spatial_position_placements(ctx, points)? {
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
                ctx.dedup_vec(&mut resolved, "deduplicate SLDPRT spatial hole placements")?;
                Ok((!ambiguous && !resolved.is_empty()).then_some(resolved))
            })?;
        if let Some(resolved) = solution {
            let resolved = ctx.try_collect_vec(
                resolved.iter().map(|placement| {
                    placement.try_clone_for_decode(ctx, "retain SLDPRT spatial hole placements")
                }),
                "retain SLDPRT spatial hole placements",
            )?;
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
    let (mut points, _storage) = ctx
        .with_scoped_storage("SLDPRT coplanar spatial point workspace", || {
            ctx.copy_slice(points, "sort SLDPRT spatial position points")
        })?;
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
    ctx.dedup_vec(&mut points, "deduplicate SLDPRT spatial position points")?;
    if points.len() < 3
        || ctx.any_by(
            &points,
            |point| Ok(!point.is_finite()),
            "scan SLDPRT holes records",
        )?
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
    if ctx.any_by(
        &points,
        |point| {
            Ok(Vector3::new(
                point.x - points[0].x,
                point.y - points[0].y,
                point.z - points[0].z,
            )
            .dot(normal)
            .abs()
                > extent * EPS_HOLE_POSITION)
        },
        "scan SLDPRT holes records",
    )? {
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
    const OPERATION: &str = "sort SLDPRT generated hole lanes";
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
    let mut source_items = faces.into_iter();
    while let Some(face) =
        ctx.next_charged(&mut source_items, "index SLDPRT generated hole faces")?
    {
        let key = face.id.as_str();
        native_features_storage.with_storage(|| {
            ctx.insert_hash_map(
                &mut faces_by_id,
                key,
                face,
                "index SLDPRT generated hole faces",
            )
        })?;
    }
    let mut surfaces_by_id = HashMap::new();
    let mut source_items = surfaces.into_iter();
    while let Some(surface) =
        ctx.next_charged(&mut source_items, "index SLDPRT generated hole surfaces")?
    {
        let key = surface.id.as_str();
        native_features_storage.with_storage(|| {
            ctx.insert_hash_map(
                &mut surfaces_by_id,
                key,
                surface,
                "index SLDPRT generated hole surfaces",
            )
        })?;
    }

    let mut faces_by_source = HashMap::new();
    for (face, identity) in
        ctx.admit_iter(face_identities, "index SLDPRT generated hole face sources")?
    {
        native_features_storage.with_storage(|| {
            ctx.push_hash_group(
                &mut faces_by_source,
                identity.feature_source_id,
                (face, identity),
                "index SLDPRT generated hole face sources",
                "index SLDPRT generated hole face sources",
            )
        })?;
    }
    let mut lane_local_ids = Vec::new();
    for lane in ctx.admit_iter(lanes, "index SLDPRT generated hole lane identities")? {
        let mut by_source = HashMap::new();
        for identity in ctx.admit_iter(
            &lane.generated_surface_identities,
            "index SLDPRT generated hole surface identities",
        )? {
            native_features_storage.with_storage(|| {
                let local_ids = ctx
                    .entry_hash_map(
                        &mut by_source,
                        identity.feature_source_id,
                        "index SLDPRT generated hole surface identities",
                    )?
                    .or_insert_with(HashSet::new);
                ctx.insert_hash_set(
                    local_ids,
                    identity.local_identity,
                    "index SLDPRT generated hole surface identities",
                )
            })?;
        }
        native_features_storage.with_storage(|| {
            ctx.push_vec(
                &mut lane_local_ids,
                by_source,
                "index SLDPRT generated hole lane identities",
            )
        })?;
    }

    for feature_index in ctx.admit_iter(&(0..features.len()), OPERATION)? {
        let feature = &mut features[feature_index];
        let mut solution_storage =
            ctx.reserve_scoped(0, "SLDPRT generated hole solution workspace")?;
        let solution = solution_storage.with_storage(
            || -> Result<Option<Vec<HolePlacement>>, CodecError> {
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
                let mut first_solution = None::<(
                    Vec<HolePlacement>,
                    cadmpeg_core::decode::ScopedReservation<'_>,
                )>;
                let faces = ctx
                    .get_hash_map(
                        &faces_by_source,
                        &source,
                        "find SLDPRT generated hole face source",
                    )?
                    .map_or(&[][..], Vec::as_slice);
                for by_source in ctx.admit_iter(&lane_local_ids, "scan SLDPRT holes records")? {
                    let Some(local_ids) = ctx.get_hash_map(
                        by_source,
                        &source,
                        "find SLDPRT generated hole surface source",
                    )?
                    else {
                        continue;
                    };
                    let mut axes_storage =
                        ctx.reserve_scoped(0, "index SLDPRT generated hole axes")?;
                    let mut axes = BTreeMap::<[GridCoordinate; 6], HolePlacement>::new();
                    let mut invalid = false;
                    let mut source_items = faces.iter();
                    while let Some((face, identity)) =
                        ctx.next_charged(&mut source_items, "scan SLDPRT generated hole faces")?
                    {
                        if !ctx.contains_hash_set(
                            local_ids,
                            &identity.local_id,
                            "find SLDPRT generated hole face identity",
                        )? {
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
                            invalid = true;
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
                            if let std::collections::btree_map::Entry::Vacant(entry) = ctx
                                .entry_btree_map(
                                    &mut axes,
                                    key,
                                    "index SLDPRT generated hole axes",
                                )?
                            {
                                entry.insert(HolePlacement::Axis {
                                    origin: closest,
                                    axis,
                                });
                            }
                            Ok::<(), CodecError>(())
                        })?;
                    }
                    if invalid || axes.is_empty() {
                        continue;
                    }
                    let (placements, placements_storage) =
                        ctx.with_scoped_storage("SLDPRT generated lane placements", || {
                            ctx.collect_vec(
                                axes.into_values(),
                                "collect SLDPRT generated hole placements",
                            )
                        })?;
                    match &first_solution {
                        Some((first, _)) if !ctx.equal(first, &placements, OPERATION)? => {
                            return Ok(None)
                        }
                        None => first_solution = Some((placements, placements_storage)),
                        Some(_) => {}
                    }
                }
                first_solution
                    .map(|(placements, _storage)| {
                        ctx.try_collect_vec(
                            placements
                                .iter()
                                .map(|placement| placement.try_clone_for_decode(ctx, OPERATION)),
                            OPERATION,
                        )
                    })
                    .transpose()
            },
        )?;
        if let Some(solution) = solution {
            let mut retained = Vec::new();
            for placement in ctx.admit_iter(&solution, "retain SLDPRT generated hole placements")? {
                ctx.push_vec(
                    &mut retained,
                    placement
                        .try_clone_for_decode(ctx, "retain SLDPRT generated hole placement")?,
                    "retain SLDPRT generated hole placements",
                )?;
            }
            let solution = retained;
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
    let mut storage = ctx.reserve_scoped(0, "SLDPRT hole topology lookup workspace")?;
    let mut unresolved = Vec::new();
    let mut source_items = features.iter().enumerate().into_iter();
    while let Some((index, feature)) = ctx.next_charged(&mut source_items, DIAMETER_LOOKUP)? {
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
        storage.with_storage(|| -> Result<_, CodecError> {
            *ctx.entry_hash_map(&mut diameter_counts, key, DIAMETER_LOOKUP)?
                .or_default() += 1;
            Ok(())
        })?;
        if placements.is_none() {
            storage.with_storage(|| {
                ctx.reserve_vec(&mut unresolved, 1, "collect SLDPRT unresolved holes")
            })?;
            unresolved.push((index, diameter));
        }
    }

    for (unresolved_index, diameter) in ctx
        .admit_iter(&unresolved, "scan SLDPRT unresolved counterbores")?
        .copied()
    {
        const CANDIDATE_KEYS: &str = "index SLDPRT counterbore candidate axes";

        let diameter = diameter.get();

        let (solution, _solution_storage) =
            ctx.with_scoped_storage(CANDIDATE_KEYS, || -> Result<_, CodecError> {
                let Some(candidates) = counterbore_topology_candidates(
                    ctx,
                    features[unresolved_index].evaluation.definition(),
                    topology,
                )?
                else {
                    return Ok(None);
                };
                if ctx.get_hash_map(
                    &diameter_counts,
                    &diameter.to_bits(),
                    "find SLDPRT hole diameter count",
                )? == Some(&1)
                {
                    return Ok(Some(candidates));
                }

                let mut siblings = Vec::new();
                let mut source_items = features.iter().enumerate().into_iter();
                while let Some((index, feature)) =
                    ctx.next_charged(&mut source_items, "scan SLDPRT counterbore siblings")?
                {
                    if feature.suppressed == Some(true)
                        || !same_hole_construction(
                            ctx,
                            features[unresolved_index].evaluation.definition(),
                            feature.evaluation.definition(),
                        )?
                    {
                        continue;
                    }
                    if let FeatureDefinition::Operation(FeatureOperation::Hole {
                        placements, ..
                    }) = feature.evaluation.definition()
                    {
                        ctx.reserve_vec(&mut siblings, 1, "collect SLDPRT counterbore siblings")?;
                        siblings.push((index, placements));
                    }
                }
                let mut remaining = siblings.iter();
                if siblings.len() < 2
                    || ctx
                        .find_by(
                            &mut remaining,
                            |(_, placements)| Ok(placements.is_none()),
                            "scan SLDPRT holes records",
                        )?
                        .is_none()
                    || ctx.any_by(
                        &mut remaining,
                        |(_, placements)| Ok(placements.is_none()),
                        "scan SLDPRT holes records",
                    )?
                {
                    return Ok(None);
                }

                let mut candidate_keys = HashSet::new();
                for key in ctx
                    .admit_iter(&candidates, CANDIDATE_KEYS)?
                    .filter_map(hole_axis_key)
                {
                    ctx.insert_hash_set(&mut candidate_keys, key, CANDIDATE_KEYS)?;
                }
                if candidate_keys.len() != candidates.len() {
                    return Ok(None);
                }

                let mut claimed = HashSet::new();
                let mut complete = true;
                let mut remaining = siblings.iter();
                while let Some((index, placements)) =
                    ctx.next_charged(&mut remaining, "scan SLDPRT holes records")?
                {
                    if *index == unresolved_index {
                        continue;
                    }
                    let Some(placements) = placements.as_deref() else {
                        complete = false;
                        break;
                    };
                    let mut remaining = placements.iter();
                    while let Some(placement) =
                        ctx.next_charged(&mut remaining, "claim SLDPRT counterbore axis")?
                    {
                        let Some(key) = hole_axis_key(placement) else {
                            complete = false;
                            break;
                        };
                        if !ctx.contains_hash_set(
                            &candidate_keys,
                            &key,
                            "find SLDPRT candidate hole axis",
                        )? || ctx.contains_hash_set(
                            &claimed,
                            &key,
                            "find SLDPRT claimed hole axis",
                        )? {
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
                    return Ok(None);
                }

                let mut residual = Vec::new();
                let mut source_items = candidates.into_iter();
                while let Some(placement) =
                    ctx.next_charged(&mut source_items, "select SLDPRT residual counterbore axes")?
                {
                    if let Some(key) = hole_axis_key(&placement) {
                        if ctx.contains_hash_set(&claimed, &key, "find SLDPRT claimed hole axis")? {
                            continue;
                        }
                        ctx.reserve_vec(
                            &mut residual,
                            1,
                            "collect SLDPRT residual counterbore axes",
                        )?;
                        residual.push(placement);
                    }
                }
                if residual.is_empty() {
                    return Ok(None);
                }
                Ok(Some(residual))
            })?;
        if let Some(solution) = solution {
            let retained = ctx.try_collect_vec(
                solution
                    .iter()
                    .map(|placement| placement.try_clone_for_decode(ctx, CANDIDATE_KEYS)),
                CANDIDATE_KEYS,
            )?;
            set_hole_placements(&mut features[unresolved_index], retained);
        }
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
    let mut storage = ctx.reserve_scoped(0, "SLDPRT unresolved topology workspace")?;
    let mut unresolved = Vec::new();
    for candidate in candidates {
        storage.with_storage(|| {
            ctx.reserve_vec(&mut unresolved, 1, "collect SLDPRT flat blind holes")
        })?;
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
    let mut storage = ctx.reserve_scoped(0, "SLDPRT unresolved topology workspace")?;
    let mut unresolved = Vec::new();
    for candidate in candidates {
        storage.with_storage(|| {
            ctx.reserve_vec(
                &mut unresolved,
                1,
                "collect SLDPRT unresolved drilled holes",
            )
        })?;
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
    let mut storage = ctx.reserve_scoped(0, "SLDPRT drilled hole candidate workspace")?;
    let mut cone_keys = HashSet::new();
    let mut source_items = surfaces.into_iter();
    while let Some(surface) =
        ctx.next_charged(&mut source_items, "scan SLDPRT drilled hole cone surfaces")?
    {
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
            storage.with_storage(|| {
                ctx.insert_hash_set(&mut cone_keys, key, "index SLDPRT drilled hole cone axes")
            })?;
        }
    }
    if cone_keys.is_empty() {
        return Ok(None);
    }
    let Some(placements) = storage.with_storage(|| {
        carrier_placements(
            ctx,
            ctx.admit_iter(cylinders, "collect SLDPRT hole carrier axes")?
                .filter_map(
                    |BoreFaceSpan(origin, axis, candidate_radius, candidate_span, _)| {
                        ((candidate_radius - radius).abs() <= radius_tolerance
                            && (candidate_span - length).abs() <= length_tolerance)
                            .then_some((*origin, *axis))
                    },
                ),
        )
    })?
    else {
        return Ok(None);
    };
    let mut matched = Vec::new();
    let mut source_items = placements.into_iter();
    while let Some(placement) =
        ctx.next_charged(&mut source_items, "match SLDPRT drilled hole cone axes")?
    {
        if let Some(key) = hole_axis_key(&placement) {
            if !ctx.contains_hash_set(&cone_keys, &key, "match SLDPRT drilled hole cone axes")? {
                continue;
            }
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
    let mut visited_storage = ctx.reserve_scoped(0, "SLDPRT seeded hole visit workspace")?;
    let mut visited = HashSet::new();
    let mut source_items = 0..features.len();
    while let Some(index) =
        ctx.next_charged(&mut source_items, "scan SLDPRT seeded drilled holes")?
    {
        if ctx.contains_hash_set(&visited, &index, "find SLDPRT seeded drilled hole")?
            || features[index].suppressed == Some(true)
        {
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
        let mut sibling_storage = ctx.reserve_scoped(0, "SLDPRT seeded hole sibling workspace")?;
        let mut siblings = Vec::new();
        let mut source_items = features.iter().enumerate().into_iter();
        while let Some((sibling, feature)) =
            ctx.next_charged(&mut source_items, "scan SLDPRT seeded hole siblings")?
        {
            if feature.suppressed == Some(true)
                || !same_hole_construction(
                    ctx,
                    features[index].evaluation.definition(),
                    feature.evaluation.definition(),
                )?
            {
                continue;
            }
            sibling_storage.with_storage(|| {
                ctx.reserve_vec(&mut siblings, 1, "collect SLDPRT seeded hole siblings")
            })?;
            siblings.push(sibling);
        }
        for &sibling in ctx.admit_iter(&(siblings)[..], "scan SLDPRT holes records")? {
            visited_storage.with_storage(|| {
                ctx.insert_hash_set(&mut visited, sibling, "index SLDPRT visited seeded holes")
            })?;
        }
        if siblings.len() < 2
            || ctx.any_by(&siblings, |&sibling| {
                Ok(matches!(
                    features[sibling].evaluation.definition(),
                    FeatureDefinition::Operation(FeatureOperation::Hole { placements, .. }) if placements.is_none()
                ))
            }, "scan SLDPRT holes records")?
        {
            continue;
        }
        let (candidates, _candidate_storage) = ctx.with_scoped_storage(
            "SLDPRT seeded hole candidate workspace",
            || -> Result<_, CodecError> {
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
                        unclaimed_seeded_hole_candidates(
                            ctx, features, &siblings, diameter, candidates,
                        )
                    })
                    .transpose()?
                    .flatten();
                let candidates = match primary {
                    Some(candidates) => Some(candidates),
                    None => seeded_drilled_bore_candidates(
                        ctx, features, &siblings, diameter, topology,
                    )?,
                };
                Ok(candidates)
            },
        )?;
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
    let mut storage = ctx.reserve_scoped(0, OPERATION)?;
    let sibling_ids =
        storage.with_storage(|| ctx.collect_hash_set(siblings.iter().copied(), OPERATION))?;
    let mut claimed = HashSet::new();
    for (index, feature) in ctx
        .admit_iter(features, "scan SLDPRT holes records")?
        .enumerate()
    {
        if feature.suppressed == Some(true)
            || ctx.contains_hash_set(&sibling_ids, &index, OPERATION)?
        {
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
        let mut source_items = placements.into_iter();
        while let Some(placement) = ctx.next_charged(&mut source_items, OPERATION)? {
            let Some(key) = hole_axis_key(placement) else {
                return Ok(None);
            };
            storage.with_storage(|| {
                ctx.insert_hash_set(&mut claimed, key, "index SLDPRT claimed bore axes")
            })?;
        }
    }
    let mut available = Vec::new();
    let mut source_items = candidates.into_iter();
    while let Some(placement) = ctx.next_charged(&mut source_items, OPERATION)? {
        if let Some(key) = hole_axis_key(&placement) {
            if ctx.contains_hash_set(&claimed, &key, OPERATION)? {
                continue;
            }
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
    let mut key_storage = ctx.reserve_scoped(0, KEY_OPERATION)?;
    let mut candidate_keys = HashSet::new();
    for key in ctx
        .admit_iter(candidates, "scan SLDPRT holes records")?
        .filter_map(hole_axis_key)
    {
        key_storage
            .with_storage(|| ctx.insert_hash_set(&mut candidate_keys, key, KEY_OPERATION))?;
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
        let mut remaining = placements.iter();
        let Some(direction) = ctx.find_map(
            &mut remaining,
            |placement| {
                Ok(match placement {
                    HolePlacement::Axis { axis, .. } => Some(canonical_axis(axis.get())),
                    HolePlacement::Directed { .. } => None,
                })
            },
            "scan SLDPRT seeded hole axes",
        )?
        else {
            return Ok(());
        };
        if ctx.any_by(
            &mut remaining,
            |placement| {
                Ok(match placement {
                    HolePlacement::Axis { axis, .. } => {
                        canonical_axis(axis.get()).dot(direction) < 1.0 - EPS_HOLE_GEOMETRY
                    }
                    HolePlacement::Directed { .. } => false,
                })
            },
            "scan SLDPRT seeded hole axes",
        )? || ctx.any_by(
            placements,
            |placement| match hole_axis_key(placement) {
                Some(key) => Ok(!ctx.contains_hash_set(&candidate_keys, &key, KEY_OPERATION)?),
                None => Ok(true),
            },
            "scan SLDPRT holes records",
        )? || ctx.any_by(
            &seed_directions,
            |candidate| Ok(candidate.dot(direction) >= 1.0 - EPS_HOLE_GEOMETRY),
            "scan SLDPRT holes records",
        )? {
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

    let mut partition_storage = ctx.reserve_scoped(0, "SLDPRT seeded hole partition workspace")?;
    let mut partitions = partition_storage.with_storage(|| {
        ctx.collect_indexed_vec(siblings.len(), "SLDPRT seeded hole-axis partitions", |_| {
            Ok(Vec::<HolePlacement>::new())
        })
    })?;
    for placement in ctx.admit_iter(candidates, "scan SLDPRT holes records")? {
        let HolePlacement::Axis { axis, .. } = placement else {
            return Ok(());
        };
        let direction = canonical_axis(axis.get());
        let mut seeds = seed_directions.iter().enumerate();
        let Some((partition, _)) = ctx.find_by(
            &mut seeds,
            |(_, seed)| Ok(seed.dot(direction) >= 1.0 - EPS_HOLE_GEOMETRY),
            "match SLDPRT seeded hole-axis directions",
        )?
        else {
            return Ok(());
        };
        if ctx.any_by(
            &mut seeds,
            |(_, seed)| Ok(seed.dot(direction) >= 1.0 - EPS_HOLE_GEOMETRY),
            "match SLDPRT seeded hole-axis directions",
        )? {
            return Ok(());
        }
        partition_storage.with_storage(|| {
            ctx.push_vec(
                &mut partitions[partition],
                placement.try_clone_for_decode(ctx, "SLDPRT seeded hole placement copy")?,
                "SLDPRT seeded hole-axis placements",
            )
        })?;
    }
    if ctx.any_by(
        &partitions,
        |partition| Ok(partition.is_empty()),
        "scan SLDPRT holes records",
    )? {
        return Ok(());
    }
    for (&sibling, partition) in ctx
        .admit_iter(siblings, "scan SLDPRT holes records")?
        .zip(partitions)
    {
        let retained = ctx.try_collect_vec(
            partition.iter().map(|placement| {
                placement.try_clone_for_decode(ctx, "retain SLDPRT seeded hole placements")
            }),
            "retain SLDPRT seeded hole placements",
        )?;
        set_hole_placements(&mut features[sibling], retained);
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
    let mut remaining = features.iter();
    let matches = |feature: &&cadmpeg_ir::features::Feature| -> Result<_, CodecError> {
        Ok(feature.suppressed != Some(true)
            && same_hole_construction(
                ctx,
                features[index].evaluation.definition(),
                feature.evaluation.definition(),
            )?)
    };
    Ok(ctx.find_by(&mut remaining, matches, OPERATION)?.is_some()
        && !ctx.any_by(&mut remaining, |feature| matches(&feature), OPERATION)?)
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

    const OPERATION: &str = "SLDPRT counterbore placement workspace";
    let (solution, _solution_storage) =
        ctx.with_scoped_storage(OPERATION, || -> Result<_, CodecError> {
            let Some(primary) =
                cylindrical_surface_placements(ctx, diameter * 0.5, topology.surfaces)?
            else {
                return Ok(None);
            };
            let Some(counterbores) =
                cylindrical_surface_placements(ctx, counterbore_diameter * 0.5, topology.surfaces)?
            else {
                return Ok(None);
            };
            let mut storage = ctx.reserve_scoped(0, "SLDPRT counterbore axis workspace")?;
            let mut primary_keys = BTreeSet::new();
            for key in ctx
                .admit_iter(&primary, "index SLDPRT counterbore primary axes")?
                .filter_map(hole_axis_key)
            {
                storage.with_storage(|| {
                    ctx.insert_btree_set(
                        &mut primary_keys,
                        key,
                        "index SLDPRT counterbore primary axes",
                    )
                })?;
            }
            let mut counterbore_keys = BTreeSet::new();
            for key in ctx
                .admit_iter(&counterbores, "index SLDPRT counterbore outer axes")?
                .filter_map(hole_axis_key)
            {
                storage.with_storage(|| {
                    ctx.insert_btree_set(
                        &mut counterbore_keys,
                        key,
                        "index SLDPRT counterbore outer axes",
                    )
                })?;
            }
            Ok((primary_keys.len() == primary.len()
                && counterbore_keys.len() == counterbores.len()
                && primary_keys.len() == counterbore_keys.len()
                && ctx.all_by(
                    &primary_keys,
                    |key| {
                        ctx.contains_btree_set(
                            &counterbore_keys,
                            key,
                            "compare SLDPRT counterbore axes",
                        )
                    },
                    "compare SLDPRT counterbore axes",
                )?)
            .then_some(primary))
        })?;
    solution
        .map(|solution| {
            ctx.try_collect_vec(
                solution
                    .iter()
                    .map(|placement| placement.try_clone_for_decode(ctx, OPERATION)),
                OPERATION,
            )
        })
        .transpose()
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
    records: &[HoleHistoryIndex<'a, '_>],
    sketch_for: impl Fn(&str) -> Result<Option<&'s SketchId>, CodecError>,
    sketch_entities: &[SketchEntity],
) -> Result<Option<&'a crate::records::Feature>, CodecError> {
    const OPERATION: &str = "resolve SLDPRT direct hole position";
    let Some(records) = ctx.find_by(
        records,
        |records| ctx.contains_key_hash_map(&records.by_id, hole.id.as_str(), OPERATION),
        OPERATION,
    )?
    else {
        return Ok(None);
    };
    let history = records.history;
    let mut direct_sketches: [Option<&crate::records::Feature>; 2] = [None, None];
    if let Some(children) = ctx.get_btree_map(
        &hole.properties,
        "DissectableChildren",
        "find SLDPRT hole property",
    )? {
        for source in hole_child_tokens(ctx, children)? {
            let source = ctx.trim_text(source?, OPERATION)?;
            if source.is_empty() {
                continue;
            }
            let Some(child) = records.child(ctx, source)? else {
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
            let group = ctx.get_hash_map(&records.by_ordinal, &ordinal, OPERATION)?.map_or(&[][..], Vec::as_slice);
            let mut remaining = group.iter();
            while let Some(&index) = ctx.next_charged(&mut remaining, OPERATION)? {
                let candidate = &history.features[index];
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

    let mut names_storage = ctx.reserve_scoped(0, "index SLDPRT hole object names")?;
    let object_names = hole_lane_names(ctx, &mut names_storage, lanes)?;
    let positions = PositionSketches::new(ctx, histories, &object_names)?;
    let (markers, _markers_storage) = ctx.with_scoped_storage(INDEX_OPERATION, || {
        ctx.try_collect_vec(
            lanes.iter().map(|lane| HoleMarkers::new(ctx, lane)),
            INDEX_OPERATION,
        )
    })?;
    let (records, _records_storage) =
        ctx.with_scoped_storage("index SLDPRT hole history records", || {
            ctx.try_collect_vec(
                histories
                    .iter()
                    .map(|history| HoleHistoryIndex::new(ctx, history)),
                "index SLDPRT hole history records",
            )
        })?;
    let surfaces = topology.surfaces;
    let mut native_features = BTreeMap::new();
    for iteration_history in ctx.admit_iter(histories, "scan SLDPRT holes source records")? {
        for feature in ctx.admit_iter(&iteration_history.features, "scan SLDPRT holes records")? {
            names_storage.with_storage(|| {
                ctx.insert_btree_map(
                    &mut native_features,
                    feature.id.as_str(),
                    feature,
                    INDEX_OPERATION,
                )
            })?;
        }
    }
    let mut model_sketches_storage = ctx.reserve_scoped(0, INDEX_OPERATION)?;
    let mut model_sketches = HashMap::new();
    let mut source_items = model_features.iter();
    while let Some(feature) = ctx.next_charged(&mut source_items, INDEX_OPERATION)? {
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
            ctx.insert_hash_map(
                &mut model_sketches,
                native.as_str(),
                sketch,
                "resolve SLDPRT holes keys",
            )
        })?;
    }
    let mut hole_positions_storage = ctx.reserve_scoped(0, INDEX_OPERATION)?;
    let mut hole_positions = BTreeMap::new();
    let mut source_items = native_features.values();
    while let Some(hole) = ctx.next_charged(&mut source_items, INDEX_OPERATION)? {
        if classify(hole) != Some(FeatureClass::Hole) {
            continue;
        }
        let position = match hole_position_feature(ctx, hole, &positions, lanes, &object_names)? {
            Some(position) => Some(position),
            None => direct_hole_position_feature(
                ctx,
                hole,
                &records,
                |id| {
                    ctx.get_hash_map(&model_sketches, id, INDEX_OPERATION)
                        .map(|sketch| sketch.copied())
                },
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
    drop(model_sketches);
    drop(model_sketches_storage);
    let mut position_features = HashSet::new();
    let mut source_items = hole_positions.values();
    while let Some(position) = ctx.next_charged(&mut source_items, INDEX_OPERATION)? {
        names_storage.with_storage(|| {
            ctx.insert_hash_set(
                &mut position_features,
                position.id.as_str(),
                INDEX_OPERATION,
            )
        })?;
    }
    let mut feature_ranges_storage = ctx.reserve_scoped(0, INDEX_OPERATION)?;
    let mut feature_ranges = HashMap::new();
    let mut source_items = lanes.into_iter();
    while let Some(lane) = ctx.next_charged(&mut source_items, INDEX_OPERATION)? {
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
    let mut source_items = lanes.into_iter();
    while let Some(lane) = ctx.next_charged(&mut source_items, INDEX_OPERATION)? {
        let Some(ranges) = ctx.get_hash_map(
            &feature_ranges,
            lane.id.as_str(),
            "resolve SLDPRT holes keys",
        )?
        else {
            continue;
        };
        let (plane_frames, _plane_frames_storage) = ctx
            .with_scoped_storage(INDEX_OPERATION, || {
                lane_sketch_plane_frames(ctx, model_features, histories, lane)
            })?;
        let plane_index = CompactReferencePlaneIndex::new(ctx, &lane.native_payload)?;
        let mut source_items = native_features.values();
        while let Some(feature) = ctx.next_charged(&mut source_items, INDEX_OPERATION)? {
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
            names_storage.with_storage(|| {
                ctx.insert_hash_map(&mut feature_frames, key, frame, INDEX_OPERATION)
            })?;
        }
    }
    let mut hole_diameter_counts = HashMap::<u64, usize>::new();
    let mut source_items = model_features.iter();
    while let Some(feature) = ctx.next_charged(&mut source_items, INDEX_OPERATION)? {
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
        names_storage.with_storage(|| -> Result<_, CodecError> {
            let count = ctx
                .entry_hash_map(&mut hole_diameter_counts, key, INDEX_OPERATION)?
                .or_default();
            *count = count
                .checked_add(1)
                .ok_or_else(|| ctx.refuse_codec_limit(INDEX_OPERATION, u64::MAX - 1, u64::MAX))?;
            Ok(())
        })?;
    }

    let mut source_items = model_features.into_iter();
    while let Some(feature) =
        ctx.next_charged(&mut source_items, "project SLDPRT hole positions")?
    {
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
        let (solution, _solution_storage) = ctx.with_scoped_storage(
            "SLDPRT hole axis candidate workspace",
            || -> Result<Option<Vec<HolePlacement>>, CodecError> {
                if ctx.get_hash_map(
                    &hole_diameter_counts,
                    &diameter.to_bits(),
                    "find SLDPRT hole diameter count",
                )? == Some(&1)
                {
                    let mut first_frame: Option<(Point3, Vector3, Vector3)> = None;
                    let mut same_frames = true;
                    let mut remaining = lanes.iter();
                    while let Some(lane) =
                        ctx.next_charged(&mut remaining, "scan SLDPRT hole position frames")?
                    {
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
                            if let Some(bore_placements) = plane_owned_bore_placements(
                                ctx, frame.0, frame.1, radius, topology,
                            )? {
                                return Ok(Some(bore_placements));
                            }
                        }
                    }
                    if let Some(bore_placements) = bore_carrier_placements(ctx, radius, topology)? {
                        return Ok(Some(bore_placements));
                    }
                }
                let mut solutions = Vec::new();
                let mut source_items = lanes.into_iter();
                while let Some(lane) =
                    ctx.next_charged(&mut source_items, "scan SLDPRT hole position lanes")?
                {
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
                        ctx.reserve_vec(
                            &mut solutions,
                            1,
                            "collect SLDPRT hole position solutions",
                        )?;
                        solutions.push(solution);
                    }
                }
                if solutions.is_empty() {
                    let mut source_items = lanes.iter().enumerate();
                    while let Some((lane_index, lane)) =
                        ctx.next_charged(&mut source_items, "scan SLDPRT hole pattern lanes")?
                    {
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
                            &markers[lane_index],
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
            },
        )?;
        if let Some(solution) = solution {
            let solution = ctx.try_collect_vec(
                solution.iter().map(|placement| {
                    placement.try_clone_for_decode(ctx, "retain SLDPRT hole position solution")
                }),
                "retain SLDPRT hole position solution",
            )?;
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
    let mut source_items = topology.faces.into_iter();
    while let Some(face) = ctx.next_charged(&mut source_items, "scan SLDPRT bore carrier faces")? {
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
    let mut by_position = BTreeMap::new();
    let mut source_items = bore_axes.into_iter();
    while let Some((origin, axis)) =
        ctx.next_charged(&mut source_items, "scan SLDPRT plane-owned bore axes")?
    {
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
        axes_storage.with_storage(|| {
            ctx.entry_btree_map(&mut by_position, key, OPERATION)?
                .or_insert((origin, axis));
            Ok::<_, CodecError>(())
        })?;
    }
    let axes = ctx.collect_vec(
        by_position
            .into_values()
            .map(|(origin, axis)| HolePlacement::Axis { origin, axis }),
        OPERATION,
    )?;
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
    let mut storage = ctx.reserve_scoped(0, OPERATION)?;
    let mut by_axis = BTreeMap::new();
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
        storage.with_storage(|| {
            ctx.insert_btree_map(
                &mut by_axis,
                key,
                HolePlacement::Axis { origin, axis },
                OPERATION,
            )
        })?;
    }
    let placements = ctx.collect_vec(by_axis.into_values(), OPERATION)?;
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
    let mut source_items = topology.faces.into_iter();
    while let Some(face) = ctx.next_charged(&mut source_items, "scan SLDPRT hole bore faces")? {
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
                let mut common =
                    None::<(Vec<(f64, f64)>, cadmpeg_core::decode::ScopedReservation<'_>)>;
                let mut source_items = placements.into_iter();
                while let Some(placement) =
                    ctx.next_charged(&mut source_items, "scan SLDPRT hole placements")?
                {
                    let HolePlacement::Axis {
                        origin: placement_origin,
                        axis: placement_axis,
                    } = placement
                    else {
                        common = None;
                        break;
                    };
                    let mut candidate_storage =
                        ctx.reserve_scoped(0, "SLDPRT matching bore workspace")?;
                    let mut candidates = Vec::new();
                    let mut source_items = bore_faces.iter();
                    while let Some(BoreFaceSpan(origin, axis, radius, span, reversed)) =
                        ctx.next_charged(&mut source_items, "match SLDPRT hole bore faces")?
                    {
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
                            candidate_storage.with_storage(|| {
                                ctx.reserve_vec(
                                    &mut candidates,
                                    1,
                                    "collect SLDPRT matching hole bores",
                                )
                            })?;
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
                    ctx.dedup_by(
                        &mut candidates,
                        |left, right| {
                            Ok((left.0 - right.0).abs() <= EPS_HOLE_GEOMETRY
                                && (left.1 - right.1).abs() <= EPS_HOLE_GEOMETRY)
                        },
                        "deduplicate SLDPRT matching hole bores",
                    )?;
                    common = Some(match common {
                        None => (candidates, candidate_storage),
                        Some((previous, _previous_storage)) => {
                            let mut shared_storage =
                                ctx.reserve_scoped(0, "SLDPRT common bore workspace")?;
                            let mut shared = Vec::new();
                            for candidate in ctx
                                .admit_iter(&previous, "scan SLDPRT common hole bores")?
                                .copied()
                            {
                                if ctx.any_by(
                                    &candidates,
                                    |other| {
                                        Ok((candidate.0 - other.0).abs() <= EPS_HOLE_GEOMETRY
                                            && (candidate.1 - other.1).abs() <= EPS_HOLE_GEOMETRY)
                                    },
                                    "intersect SLDPRT hole bores",
                                )? {
                                    shared_storage.with_storage(|| {
                                        ctx.reserve_vec(
                                            &mut shared,
                                            1,
                                            "collect SLDPRT common hole bores",
                                        )
                                    })?;
                                    shared.push(candidate);
                                }
                            }
                            (shared, shared_storage)
                        }
                    });
                }
                let Some([(radius, depth)]) = common.as_ref().map(|(values, _)| values.as_slice())
                else {
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
    let mut names_storage = ctx.reserve_scoped(0, "index SLDPRT hole object names")?;
    let object_names = hole_lane_names(ctx, &mut names_storage, lanes)?;
    let positions = PositionSketches::new(ctx, histories, &object_names)?;
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
    let mut first_by_id = HashMap::new();
    let mut source_items = features.iter().enumerate();
    while let Some((index, feature)) = ctx.next_charged(&mut source_items, OPERATION)? {
        lookup_storage.with_storage(|| {
            ctx.entry_hash_map(&mut first_by_id, feature.id.as_str(), OPERATION)?
                .or_insert(index);
            Ok::<_, CodecError>(())
        })?;
        let Some(native) = feature.native_ref.as_deref() else {
            continue;
        };
        lookup_storage.with_storage(|| {
            ctx.insert_hash_map(&mut model_features, native, &feature.id, OPERATION)
        })?;
    }
    let mut projections = Vec::new();
    let mut source_items = features.iter();
    while let Some(hole) = ctx.next_charged(&mut source_items, OPERATION)? {
        let FeatureDefinition::Operation(FeatureOperation::Hole {
            placements: Some(placements),
            ..
        }) = hole.evaluation.definition()
        else {
            continue;
        };
        let Some(HolePlacement::Axis {
            axis: first_axis, ..
        }) = placements.first()
        else {
            continue;
        };
        let canonical = canonical_axis(first_axis.get());
        if !ctx.all_by(
            placements,
            |placement| {
                Ok(match placement {
                    HolePlacement::Axis { axis, .. } => {
                        canonical_axis(axis.get()).dot(canonical) >= 1.0 - EPS_HOLE_GEOMETRY
                    }
                    HolePlacement::Directed { .. } => false,
                })
            },
            OPERATION,
        )? {
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
        let Some(position) =
            hole_position_feature(ctx, native_hole, &positions, lanes, &object_names)?
        else {
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
        let Some(&feature_index) =
            ctx.get_hash_map(&first_by_id, position_feature.as_str(), OPERATION)?
        else {
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
        let mut selected_frame: Option<(Point3, Vector3, Vector3)> = None;
        let mut ambiguous_frame = false;
        let mut remaining = surfaces.iter();
        while let Some(surface) = ctx.next_charged(&mut remaining, OPERATION)? {
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
            if !ctx.all_by(
                placements,
                |placement| {
                    Ok(match placement {
                        HolePlacement::Axis { origin: point, .. } => {
                            Vector3::new(point.x - origin.x, point.y - origin.y, point.z - origin.z)
                                .dot(normal)
                                .abs()
                                <= EPS_HOLE_POSITION
                        }
                        HolePlacement::Directed { .. } => false,
                    })
                },
                OPERATION,
            )? {
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
        let mut owning_lane: Option<&FeatureInputLane> = None;
        let mut ambiguous = false;
        let mut remaining = lanes.iter().zip(&object_names);
        while let Some((candidate, names)) = ctx.next_charged(&mut remaining, OPERATION)? {
            if hole_position_sketch_source(ctx, native_hole, candidate, names)?
                != position.source_value()
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
        let (projection, projection_storage) =
            ctx.with_scoped_storage(OPERATION, || -> Result<_, CodecError> {
                let lane_key = ctx
                    .rsplit_once(lane.id.as_str(), "#", OPERATION)?
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
                    return Ok(None);
                };
                let v_axis = normal.cross(u_axis);
                let mut projected_entities = Vec::new();
                let mut admitted_geometry = true;
                for (ordinal, placement) in ctx.admit_iter(placements, OPERATION)?.enumerate() {
                    let HolePlacement::Axis { origin: point, .. } = placement else {
                        continue;
                    };
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
                    let delta =
                        Vector3::new(point.x - origin.x, point.y - origin.y, point.z - origin.z);
                    let Ok(geometry) = SketchGeometry::try_from(SketchGeometryDefinition::Point {
                        position: Point2::new(delta.dot(u_axis), delta.dot(v_axis)),
                    }) else {
                        admitted_geometry = false;
                        break;
                    };
                    let sketch_ref = sketch_id.try_clone_for_decode(ctx, OPERATION)?;
                    ctx.push_vec(
                        &mut projected_entities,
                        SketchEntity::new(entity_id, sketch_ref, geometry),
                        OPERATION,
                    )?;
                }
                if !admitted_geometry {
                    return Ok(None);
                }
                let Ok(placement) =
                    cadmpeg_ir::sketches::SketchPlacement::try_resolved(origin, normal, u_axis)
                else {
                    return Ok(None);
                };
                let name = model_position
                    .name
                    .as_deref()
                    .map(|text| ctx.copy_retained_text(text, OPERATION))
                    .transpose()?;
                let configuration = lane
                    .configuration
                    .as_deref()
                    .map(|text| ctx.copy_retained_text(text, OPERATION))
                    .transpose()?;
                let native_ref = Some(ctx.copy_retained_text(&lane.id, OPERATION)?);
                Ok(Some(Projection {
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
                }))
            })?;
        if let Some(projection) = projection {
            lookup_storage.with_storage(|| {
                ctx.push_vec(
                    &mut projections,
                    (projection, projection_storage),
                    OPERATION,
                )
            })?;
        }
    }
    drop((model_features, first_by_id));
    let mut source_items = projections.into_iter();
    while let Some((projection, _projection_storage)) =
        ctx.next_charged(&mut source_items, OPERATION)?
    {
        let feature = &mut features[projection.feature];
        let FeatureDefinition::Operation(FeatureOperation::Sketch { sketch, .. }) =
            feature.evaluation.definition()
        else {
            continue;
        };
        if sketch.id().is_some() {
            continue;
        }
        let sketch_id = projection.sketch.id.try_clone_for_decode(ctx, OPERATION)?;
        ctx.reserve_vec(sketches, 1, OPERATION)?;
        feature.evaluation.edit(|definition, _| {
            if let FeatureDefinition::Operation(FeatureOperation::Sketch { sketch, .. }) =
                definition
            {
                *sketch = cadmpeg_ir::features::SketchFeatureBinding::Planar(Some(sketch_id));
            }
        });
        for entity in ctx.admit_iter(&projection.entities, OPERATION)? {
            let entity_id = entity.id().try_clone_for_decode(ctx, OPERATION)?;
            let entity_sketch = entity.sketch.try_clone_for_decode(ctx, OPERATION)?;
            // These projections contain only fixed point geometry.
            ctx.push_vec(
                entities,
                SketchEntity::new(entity_id, entity_sketch, entity.geometry.clone()),
                OPERATION,
            )?;
        }
        let sketch = projection.sketch;
        sketches.push(Sketch {
            id: sketch.id.try_clone_for_decode(ctx, OPERATION)?,
            name: sketch
                .name
                .as_deref()
                .map(|text| ctx.copy_retained_text(text, OPERATION))
                .transpose()?,
            configuration: sketch
                .configuration
                .as_deref()
                .map(|text| ctx.copy_retained_text(text, OPERATION))
                .transpose()?,
            visible: sketch.visible,
            placement: sketch.placement,
            profiles: cadmpeg_ir::sketches::SketchProfiles::default(),
            native_ref: sketch
                .native_ref
                .as_deref()
                .map(|text| ctx.copy_retained_text(text, OPERATION))
                .transpose()?,
        });
    }
    Ok(())
}

fn marker_pattern_bore_axes(
    ctx: &DecodeContext<'_>,
    markers: &HoleMarkers<'_, '_>,
    feature: &str,
    radius: f64,
    surfaces: &[Surface],
    direction: Option<Vector3>,
) -> Result<Option<Vec<HolePlacement>>, CodecError> {
    const OPERATION: &str = "collect SLDPRT marker bore patterns";
    let (solution, _storage) =
        ctx.with_scoped_storage(OPERATION, || -> Result<_, CodecError> {
            let mut paired_marker_ids = HashSet::new();
            let mut reduced_marker_ids = HashSet::new();
            let roster = ctx
                .get_hash_map(&markers.by_feature, feature, OPERATION)?
                .map_or(&[][..], Vec::as_slice);
            let pairs = ctx
                .get_hash_map(&markers.paired, feature, OPERATION)?
                .map_or(&[][..], Vec::as_slice);
            for &(paired, [paired_u, paired_v]) in ctx.admit_iter(pairs, OPERATION)? {
                ctx.insert_hash_set(&mut paired_marker_ids, paired.id(), OPERATION)?;
                let reduced = if paired.kind() == SketchInputKind::Point {
                    let mut same_locus = false;
                    let mut remaining = roster.iter();
                    while let Some(candidate) = ctx.next_charged(&mut remaining, OPERATION)? {
                        if !ctx.equal(candidate.id(), paired.id(), OPERATION)?
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
            }
            let marker_loci = |paired: &HashSet<&str>| -> Result<Vec<Point2>, CodecError> {
                let mut loci = Vec::new();
                let mut source_items = roster.iter();
                while let Some(marker) = ctx.next_charged(&mut source_items, OPERATION)? {
                    if marker.object_index().is_none()
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
                ctx.dedup_by(
                    &mut loci,
                    |left, right| {
                        Ok(same_dimension_length(left.u, right.u)
                            && same_dimension_length(left.v, right.v))
                    },
                    OPERATION,
                )?;
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
        })?;
    solution
        .map(|solution| {
            ctx.try_collect_vec(
                solution
                    .iter()
                    .map(|placement| placement.try_clone_for_decode(ctx, OPERATION)),
                OPERATION,
            )
        })
        .transpose()
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
    let mut storage = ctx.reserve_scoped(0, OPERATION)?;
    let mut grouped = BTreeMap::<
        [GridCoordinate; 3],
        BTreeMap<[GridCoordinate; 3], Vec<(FinitePoint3, FeatureDirection3)>>,
    >::new();
    let mut source_items = surfaces.into_iter();
    while let Some(surface) = ctx.next_charged(&mut source_items, OPERATION)? {
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
        storage.with_storage(|| {
            let lines = ctx
                .entry_btree_map(&mut grouped, axis_key, OPERATION)?
                .or_default();
            ctx.push_btree_group(lines, point_key, (origin, axis), OPERATION, OPERATION)
        })?;
    }
    let mut solutions = BTreeMap::new();
    'lines: for (_, lines) in ctx.admit_iter(&grouped, OPERATION)? {
        let mut candidate_storage = ctx.reserve_scoped(0, OPERATION)?;
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
            candidate_storage.with_storage(|| ctx.reserve_vec(&mut candidates, 1, OPERATION))?;
            candidates.push((*point, *origin, axis));
        }
        ctx.sort_unstable_by(&mut candidates, |value| &value.0, Ord::cmp, OPERATION)?;
        let mut candidate_loci = Vec::new();
        candidate_storage
            .with_storage(|| ctx.reserve_vec(&mut candidate_loci, candidates.len(), OPERATION))?;
        let mut source_items = candidates.iter();
        while let Some(([x, y, z], ..)) = ctx.next_charged(&mut source_items, OPERATION)? {
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
            if storage
                .with_storage(|| retain_bore_solution(ctx, &candidates, indices, &mut solutions))?
            {
                return Ok(None);
            }
            continue;
        }
        let mut subsets = BTreeSet::new();
        if candidate_storage.with_storage(|| {
            (BoreSubsetSearch {
                ctx,
                marker_loci,
                candidate_loci: &candidate_loci,
                reverse: false,
            })
            .collect(0, &mut Vec::new(), &mut HashSet::new(), &mut subsets)
        })? {
            return Ok(None);
        }
        for subset in ctx.admit_iter(&subsets, OPERATION)? {
            let indices = ctx
                .admit_iter(subset, "retain SLDPRT bore pattern solution")?
                .copied();
            if storage
                .with_storage(|| retain_bore_solution(ctx, &candidates, indices, &mut solutions))?
            {
                return Ok(None);
            }
        }
    }
    if solutions.len() != 1 {
        return Ok(None);
    }
    let Some((_, (solution, _solution_storage))) =
        ctx.next_charged(&mut solutions.into_iter(), OPERATION)?
    else {
        return Ok(None);
    };
    Ok(Some(
        ctx.try_collect_vec(
            solution
                .iter()
                .map(|placement| placement.try_clone_for_decode(ctx, OPERATION)),
            OPERATION,
        )?,
    ))
}

fn retain_bore_solution<'ctx>(
    ctx: &'ctx DecodeContext<'_>,
    candidates: &[([GridCoordinate; 3], FinitePoint3, FeatureDirection3)],
    indices: impl Iterator<Item = usize>,
    solutions: &mut BTreeMap<
        Vec<[GridCoordinate; 6]>,
        (
            Vec<HolePlacement>,
            cadmpeg_core::decode::ScopedReservation<'ctx>,
        ),
    >,
) -> Result<bool, CodecError> {
    const OPERATION: &str = "retain SLDPRT bore pattern solution";
    let quantize_scalar = |value: f64| GridCoordinate::new(value, EPS_HOLE_POSITION);
    let ((key, placements), storage) =
        ctx.with_scoped_storage(OPERATION, || -> Result<_, CodecError> {
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
            Ok((key, placements))
        })?;
    ctx.insert_btree_map(solutions, key, (placements, storage), OPERATION)?;
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
            self.ctx.insert_btree_set(subsets, subset, OPERATION)?;
            return Ok(subsets.len() > 1);
        }
        for choice in 0..choices {
            self.ctx.charge_work(1, OPERATION)?;
            if self.ctx.contains_hash_set(used, &choice, OPERATION)? {
                continue;
            }
            self.ctx.insert_hash_set(used, choice, OPERATION)?;
            let valid = self.ctx.all_by(
                assigned.iter().copied().enumerate(),
                |(previous, previous_choice)| {
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
                    Ok(same_dimension_length(marker_distance, delta.norm()))
                },
                OPERATION,
            )?;
            if valid {
                self.ctx.push_vec(assigned, choice, OPERATION)?;
                let ambiguous = self.collect(index + 1, assigned, used, subsets)?;
                assigned.pop();
                self.ctx.remove_hash_set(used, &choice, OPERATION)?;
                if ambiguous {
                    return Ok(true);
                }
            } else {
                self.ctx.remove_hash_set(used, &choice, OPERATION)?;
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
    let mut storage = ctx.reserve_scoped(0, "SLDPRT bore subset workspace")?;
    storage.with_storage(|| {
        let mut subsets = BTreeSet::new();
        (BoreSubsetSearch {
            ctx,
            marker_loci,
            candidate_loci,
            reverse: true,
        })
        .collect(0, &mut Vec::new(), &mut HashSet::new(), &mut subsets)?;
        Ok(subsets.len() == 1)
    })
}

pub(super) fn feature_object_byte_ranges<'a>(
    ctx: &DecodeContext<'_>,
    histories: &'a [crate::records::FeatureHistory],
    lane: &FeatureInputLane,
) -> Result<HashMap<&'a str, (usize, usize, usize)>, CodecError> {
    const OPERATION: &str = "index SLDPRT feature object byte ranges";
    let object_names = ObjectNames::new(ctx, lane)?;
    let mut storage = ctx.reserve_scoped(0, OPERATION)?;
    let mut objects = Vec::new();
    let mut next_input_index = 0usize;
    for history in ctx.admit_iter(histories, OPERATION)? {
        for feature in ctx.admit_iter(&history.features, OPERATION)? {
            let input_index = next_input_index;
            next_input_index = next_input_index
                .checked_add(1)
                .ok_or_else(|| ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX))?;
            let Some(name) = object_names.of(ctx, feature)? else {
                continue;
            };
            storage.with_storage(|| {
                ctx.push_vec(&mut objects, (name.offset, input_index, feature), OPERATION)
            })?;
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
    for (index, (offset, _, feature)) in ctx.admit_iter(&objects, OPERATION)?.enumerate() {
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
            if padding.iter().all(|byte| *byte == 0)
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
    let reference = match plane_index.profile_source(ctx, context_start, start, end)? {
        Some(source) => ctx
            .get_hash_map(plane_frames, &source, "find SLDPRT sketch reference plane")?
            .copied(),
        None => None,
    };
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
    let mut candidates = BTreeMap::<String, Option<(Point3, Vector3, Vector3)>>::new();
    for lane in ctx.admit_iter(lanes, "scan SLDPRT holes records")? {
        let mut lane_storage = ctx.reserve_scoped(0, "SLDPRT sketch frame lane workspace")?;
        let ranges =
            lane_storage.with_storage(|| feature_object_byte_ranges(ctx, histories, lane))?;
        let plane_frames = lane_storage
            .with_storage(|| lane_sketch_plane_frames(ctx, features, histories, lane))?;
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
                if let Some(candidate) = ctx.get_mut_btree_map(
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
                        ctx.insert_btree_map(
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
    let mut source_items = candidates.into_iter();
    while let Some((feature, frame)) =
        ctx.next_charged(&mut source_items, "select unique sketch feature frames")?
    {
        if let Some(frame) = frame {
            let feature = ctx.copy_retained_text(&feature, "retain unique sketch frame owner")?;
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
    let mut scalars_storage = ctx.reserve_scoped(0, "resolve SLDPRT holes keys")?;
    let mut scalars = HashMap::new();

    for scalar in ctx.admit_iter(&lane.scalars, OPERATION)? {
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
    let mut source_items = lane.relation_instances.iter();
    while let Some(relation) = ctx.next_charged(&mut source_items, OPERATION)? {
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
    let mut storage = ctx.reserve_scoped(0, OPERATION)?;
    let mut axes = Vec::new();
    let mut source_items = surfaces.into_iter();
    while let Some(surface) = ctx.next_charged(&mut source_items, OPERATION)? {
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
        storage.with_storage(|| ctx.reserve_vec(&mut axes, 1, OPERATION))?;
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
    storage.with_storage(|| ctx.reserve_vec(&mut loci, 1, OPERATION))?;
    loci.push(Point2::new(0.0, 0.0));
    let mut bore_loci = HashSet::new();
    for point in ctx.admit_iter(&axes, OPERATION)? {
        let Some(point) = point.point(QUANTUM) else {
            return Ok(None);
        };
        let index = if let Some(index) =
            ctx.position_by(&loci, |candidate| Ok(*candidate == point), OPERATION)?
        {
            index
        } else {
            storage.with_storage(|| ctx.reserve_vec(&mut loci, 1, OPERATION))?;
            loci.push(point);
            loci.len() - 1
        };
        storage.with_storage(|| ctx.insert_hash_set(&mut bore_loci, index, OPERATION))?;
    }
    let Some(indices) =
        storage.with_storage(|| compact_position_loci(ctx, &loci, &bore_loci, relations))?
    else {
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
    let mut storage = ctx.reserve_scoped(0, OPERATION)?;
    let selected = storage.with_storage(|| -> Result<Option<Vec<usize>>, CodecError> {
        let mut nodes = Vec::new();
        let mut source_items = relations.into_iter();
        while let Some((_, first, second, _)) = ctx.next_charged(&mut source_items, OPERATION)? {
            ctx.extend_vec(&mut nodes, [*first, *second], OPERATION)?;
        }
        ctx.sort_unstable_by(&mut nodes, |value| value, Ord::cmp, OPERATION)?;
        ctx.dedup_vec(&mut nodes, OPERATION)?;
        if nodes.is_empty() || nodes.len() > loci.len() {
            return Ok(None);
        }
        let mut solutions = BTreeSet::new();
        for swap_axes in [false, true] {
            CompactPositionSearch {
                ctx,
                nodes: &nodes,
                loci,
                relations,
                placement_loci,
                swap_axes,
            }
            .assign(0, &mut HashMap::new(), &mut BTreeSet::new(), &mut solutions)?;
        }
        let mut solutions = solutions.into_iter();
        Ok(match (solutions.next(), solutions.next()) {
            (Some(solution), None) => Some(solution),
            _ => None,
        })
    })?;
    selected
        .map(|indices| ctx.copy_slice(&indices, OPERATION))
        .transpose()
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
        used: &mut BTreeSet<usize>,
        solutions: &mut BTreeSet<Vec<usize>>,
    ) -> Result<(), CodecError> {
        const OPERATION: &str = "search SLDPRT compact position assignments";
        let _depth = self.ctx.enter_nested(OPERATION)?;
        self.ctx.charge_work(1, OPERATION)?;
        if solutions.len() > 1 {
            return Ok(());
        }
        if node_index == self.nodes.len() {
            let mut solution = Vec::new();
            for &index in self.ctx.admit_iter(&*used, OPERATION)? {
                if self
                    .ctx
                    .contains_hash_set(self.placement_loci, &index, OPERATION)?
                {
                    self.ctx.reserve_vec(&mut solution, 1, OPERATION)?;
                    solution.push(index);
                }
            }
            if solution.is_empty() {
                return Ok(());
            }
            self.ctx.insert_btree_set(solutions, solution, OPERATION)?;
            return Ok(());
        }
        let node = self.nodes[node_index];
        for locus_index in 0..self.loci.len() {
            self.ctx.charge_work(1, OPERATION)?;
            if self.ctx.contains_btree_set(used, &locus_index, OPERATION)? {
                continue;
            }
            self.ctx.insert_btree_set(used, locus_index, OPERATION)?;
            self.ctx
                .insert_hash_map(assigned, node, locus_index, OPERATION)?;
            let valid = self.ctx.all_by(
                self.relations,
                |(family, first, second, distance)| {
                    let (Some(&first), Some(&second)) = (
                        self.ctx.get_hash_map(assigned, first, OPERATION)?,
                        self.ctx.get_hash_map(assigned, second, OPERATION)?,
                    ) else {
                        return Ok(true);
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
                        _ => return Ok(false),
                    };
                    Ok(same_dimension_length(measured, *distance))
                },
                OPERATION,
            )?;
            if valid {
                self.assign(node_index + 1, assigned, used, solutions)?;
            }
            self.ctx.remove_hash_map(assigned, &node, OPERATION)?;
            self.ctx.remove_btree_set(used, &locus_index, OPERATION)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
