//! Reference-plane, reference-axis, reference-point, and coordinate-system geometry.

use super::compact_reference_planes::principal_sketch_frame;
use super::curves::{
    sketch_plane_frames, SketchPlaneUAxisSource, CONSTRUCTED_MID_PLANE_U_AXIS_SOURCE,
    REFERENCE_PLANE_U_AXIS_SOURCE_PROPERTY,
};
use super::scalars::ObjectNames;
use super::selections::{
    compact_component_path_end_at, component_face_reference_in_record, COMPACT_EDGE_VECTOR_MARKER,
};
use super::{is_class_token, CLASS_MARKER, NAME_MARKER};
use crate::classification::{
    classify, native_object_class, principal_plane_in_layout, principal_plane_layout, FeatureClass,
    NativeClassKind,
};
use crate::records::{FeatureInputLane, FeatureInputName};
use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::text::NonBlankString;
use cadmpeg_core::CodecError;
use cadmpeg_ir::math::{Point3, Vector3};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use crate::layout::constructed_reference_plane_fixed_frame as fixed_plane;
use crate::layout::constructed_reference_plane_matrix_frame as matrix_plane;
use crate::layout::coordinate_system_component_path_prefix as cs_path_pre;
use crate::layout::coordinate_system_component_path_suffix as cs_path_suf;
use crate::layout::coordinate_system_component_point as cs_pt;
use crate::layout::coordinate_system_endpoint_path_prefix as ep_path_pre;
use crate::layout::coordinate_system_endpoint_path_suffix as ep_path_suf;
use crate::layout::coordinate_system_extended_component_point as cs_ext;
use crate::layout::coordinate_system_line_axis as line_axis;
use crate::layout::coordinate_system_ordinal_axis_tail as ordinal_tail;
use crate::layout::coordinate_system_two_point_separator as two_pt_sep;
use crate::layout::coordinate_system_two_point_tail as two_pt_tail;
use crate::layout::coordinate_system_xy_tail as xy_tail;
use crate::layout::reference_point_long_solved_cache as pt_long;
use crate::layout::reference_point_short_solved_cache as pt_short;
use crate::records::ObjectId;
use cadmpeg_core::decode::{index_from_u64, u64_from_index};

const EPS_REFERENCE_GEOMETRY_RECONCILE_REFERENCE_PLANE_FRAME_WITH_SOURCE_E9: f64 = 1e-9;
const EPS_REFERENCE_GEOMETRY_COORDINATE_SYSTEM_TWO_POINT_FRAME_E9: f64 = 1e-9;
const EPS_REFERENCE_GEOMETRY_COORDINATE_SYSTEM_LINE_AXES_E12: f64 = 1e-12;
const EPS_REFERENCE_GEOMETRY_COORDINATE_SYSTEM_LINE_AXES_E9: f64 = 1e-9;
const EPS_REFERENCE_GEOMETRY_OFFSET_PLANE_REFERENCE_FRAME_MATCHES_E9: f64 = 1e-9;
const EPS_REFERENCE_GEOMETRY_OFFSET_PLANE_REFERENCE_FRAME_MATCHES_E8: f64 = 1e-8;
const EPS_REFERENCE_GEOMETRY_COMPLETE_REFERENCE_AXIS_TRIAD_E12: f64 = 1e-12;
const EPS_REFERENCE_GEOMETRY_EXPLICIT_REFERENCE_AXIS_FRAME_E12: f64 = 1e-12;
const EPS_REFERENCE_GEOMETRY_CONSTRAINT_MIDPLANE_FRAME_E9: f64 = 1e-9;
const EPS_REFERENCE_GEOMETRY_ANGLED_REFERENCE_PLANE_FRAME_E9: f64 = 1e-9;
const EPS_REFERENCE_GEOMETRY_MATRIX_REFERENCE_PLANE_FRAME_CANDIDATES_E9: f64 = 1e-9;
const EPS_REFERENCE_GEOMETRY_COMPACT_REFERENCE_PLANE_FRAME_E9: f64 = 1e-9;

fn reconcile_reference_plane_frame_with_source(
    explicit: Option<(Point3, Vector3, Vector3)>,
    constraint: Option<(Point3, Vector3, Vector3)>,
) -> Option<((Point3, Vector3, Vector3), SketchPlaneUAxisSource)> {
    let (Some(explicit), Some(constraint)) = (explicit, constraint) else {
        return explicit
            .map(|frame| (frame, SketchPlaneUAxisSource::Native))
            .or_else(|| {
                constraint.map(|frame| (frame, SketchPlaneUAxisSource::ConstructedMidPlane))
            });
    };
    let explicit_distance =
        explicit.1.x * explicit.0.x + explicit.1.y * explicit.0.y + explicit.1.z * explicit.0.z;
    let alignment = explicit.1.x * constraint.1.x
        + explicit.1.y * constraint.1.y
        + explicit.1.z * constraint.1.z;
    let constraint_distance = constraint.1.x * constraint.0.x
        + constraint.1.y * constraint.0.y
        + constraint.1.z * constraint.0.z;
    if (alignment.abs() - 1.0).abs()
        <= EPS_REFERENCE_GEOMETRY_RECONCILE_REFERENCE_PLANE_FRAME_WITH_SOURCE_E9
        && (explicit_distance - alignment.signum() * constraint_distance).abs()
            <= EPS_REFERENCE_GEOMETRY_RECONCILE_REFERENCE_PLANE_FRAME_WITH_SOURCE_E9
    {
        Some((explicit, SketchPlaneUAxisSource::Native))
    } else {
        Some((constraint, SketchPlaneUAxisSource::ConstructedMidPlane))
    }
}


fn insert_reference_plane_property(
    ctx: &DecodeContext<'_>,
    feature: &mut crate::records::Feature,
    key: NonBlankString,
    value: std::fmt::Arguments<'_>,
) -> Result<(), CodecError> {
    let value = ctx.format_retained(value, "retain SLDPRT reference plane property")?;
    ctx.insert_btree_map(
        &mut feature.properties,
        key,
        value,
        "insert SLDPRT reference plane property",
    )?;
    Ok(())
}

fn retained_plane_frame_source(
    ctx: &DecodeContext<'_>,
    feature: &crate::records::Feature,
) -> Result<String, CodecError> {
    match feature.source_id {
        Some(crate::records::FeatureSource::Reserved) => {
            ctx.format_retained(format_args!("-1"), "retain SLDPRT plane frame source")
        }
        Some(crate::records::FeatureSource::Id(id)) => ctx.format_retained(
            format_args!("{}", id.value()),
            "retain SLDPRT plane frame source",
        ),
        None => ctx.format_retained(
            format_args!("{}", feature.id),
            "retain SLDPRT plane frame source",
        ),
    }
}

/// Add validated reference-plane frames to a projection copy of history.
pub(crate) fn enrich_history_reference_planes(
    ctx: &DecodeContext<'_>,
    histories: &mut [crate::records::FeatureHistory],
    lanes: &[FeatureInputLane],
) -> Result<(), CodecError> {
    let mut temporary_storage =
        ctx.reserve_scoped(0, "SLDPRT reference_geometry temporary storage")?;

    let mut candidates = BTreeMap::<(usize, usize), Vec<(Point3, Vector3, Vector3)>>::new();
    let mut candidate_sources = BTreeMap::<(usize, usize), Vec<SketchPlaneUAxisSource>>::new();
    let mut reference_candidates = BTreeMap::<(usize, usize), Vec<String>>::new();
    let mut reference_frame_candidates =
        BTreeMap::<(usize, usize), Vec<(Point3, Vector3, Vector3)>>::new();
    let mut explicit_reference_indices = HashSet::new();
    let mut face_feature_candidates = BTreeMap::<(usize, usize), Vec<String>>::new();
    let mut face_native_candidates = BTreeMap::<(usize, usize), Vec<String>>::new();
    let mut known_sources = Vec::new();
    let mut features_by_source = Vec::new();
    let mut known_reference_plane_sources = Vec::new();
    let mut principal_layouts = Vec::new();
    for history in ctx.admit_iter(&*histories, "index SLDPRT reference plane sources")? {
        let layout = principal_plane_layout(ctx, &history.features)?;
        temporary_storage.with_storage(|| {
            ctx.push_vec(
                &mut principal_layouts,
                layout,
                "collect SLDPRT principal plane layouts",
            )
        })?;
        let mut sources = HashSet::new();
        let mut by_source = HashMap::new();
        let mut reference_sources = HashSet::new();
        for (index, feature) in ctx
            .admit_iter(&history.features, "index SLDPRT reference plane features")?
            .enumerate()
        {
            let Some(source) = feature.source_value() else {
                continue;
            };
            temporary_storage.with_storage(|| {
                ctx.insert_hash_set(&mut sources, source, "index SLDPRT reference plane sources")
            })?;
            temporary_storage.with_storage(|| {
                ctx.insert_hash_map(
                    &mut by_source,
                    source,
                    index,
                    "index SLDPRT reference plane features",
                )
            })?;
            if classify(feature) == Some(FeatureClass::ReferencePlane)
                && !ctx.contains_hash_set(&reference_sources, &source, "index SLDPRT reference plane targets")?
            {
                temporary_storage.with_storage(|| {
                    ctx.insert_hash_set(
                        &mut reference_sources,
                        source,
                        "index SLDPRT reference plane targets",
                    )
                })?;
            }
        }
        temporary_storage.with_storage(|| ctx.push_vec(&mut known_sources, sources, "collect SLDPRT reference plane source indexes"))?;
        temporary_storage.with_storage(|| ctx.push_vec(&mut features_by_source, by_source, "collect SLDPRT reference plane feature indexes"))?;
        temporary_storage.with_storage(|| ctx.push_vec(&mut known_reference_plane_sources, reference_sources, "collect SLDPRT reference plane target indexes"))?;
    }
    for lane in ctx.admit_iter(lanes, "scan SLDPRT reference plane lanes")? {
        let mut lane_storage = ctx.reserve_scoped(0, "SLDPRT reference lane workspace")?;
        let object_names = ObjectNames::new(ctx, lane)?;
        let mut classes = lane_storage.with_storage(|| ctx.collect_vec(lane.classes.iter(), "index SLDPRT reference plane classes"))?;
        ctx.stable_sort_by(&mut classes, |class| &class.offset, Ord::cmp, "sort SLDPRT reference plane classes")?;
        let mut starts = Vec::new();
        for (history_index, history) in ctx
            .admit_iter(&*histories, "scan SLDPRT reference plane features")?
            .enumerate()
        {
            for (feature_index, feature) in ctx
                .admit_iter(&history.features, "scan SLDPRT reference plane features")?
                .enumerate()
            {
                if let Some(name) = object_names.of(ctx, feature)? {
                    lane_storage.with_storage(|| ctx.push_vec(&mut starts, (name.offset, history_index, feature_index), "collect SLDPRT reference plane starts"))?;
                }
            }
        }
        ctx.stable_sort_by(
            &mut starts,
            |value| &value.0,
            Ord::cmp,
            "sort SLDPRT feature starts",
        )?;
        for (index, &(start, history_index, feature_index)) in ctx
            .admit_iter(&starts, "scan SLDPRT reference plane starts")?
            .enumerate()
        {
            let feature = &histories[history_index].features[feature_index];
            if classify(feature) != Some(FeatureClass::ReferencePlane)
                || principal_plane_in_layout(principal_layouts[history_index], feature).is_some()
                || ctx.contains_key_btree_map(&feature.properties, "Origin", "find SLDPRT reference geometry property")?
                || ctx.contains_key_btree_map(&feature.properties, "Normal", "find SLDPRT reference geometry property")?
                || ctx.contains_key_btree_map(&feature.properties, "UAxis", "find SLDPRT reference geometry property")?
            {
                continue;
            }
            let Some(end) = starts
                .get(index + 1)
                .map_or(Some(lane.native_payload.len()), |next| {
                    index_from_u64(next.0)
                })
            else {
                continue;
            };
            let Ok(start) = usize::try_from(start) else {
                continue;
            };
            let Some(bytes) = lane.native_payload.get(start..end) else {
                continue;
            };
            let self_source = feature.source_value();
            if let Some(source) = offset_plane_reference_source(
                ctx,
                bytes,
                &known_sources[history_index],
                &known_reference_plane_sources[history_index],
                self_source,
            )? {
                let index = (history_index, feature_index);
                temporary_storage.with_storage(|| {
                    ctx.insert_hash_set(
                        &mut explicit_reference_indices,
                        index,
                        "index SLDPRT explicit plane references",
                    )
                })?;
                let source = temporary_storage.with_storage(|| ctx.format_retained(
                    format_args!("{source}"),
                    "retain SLDPRT reference plane source",
                ))?;
                temporary_storage.with_storage(|| ctx.push_btree_group(&mut reference_candidates, index, source, "collect SLDPRT reference plane sources", "collect SLDPRT reference plane sources"))?;
            }
            if let Some((relative_offset, owner)) = legacy_offset_plane_face_alias(ctx, bytes)? {
                let native = temporary_storage.with_storage(|| ctx.format_retained(
                    format_args!(
                        "sldprt:feature-input:legacy-face-alias#{}:{}:{}",
                        lane.id,
                        start + relative_offset,
                        owner,
                    ),
                    "retain SLDPRT legacy face alias",
                ))?;
                temporary_storage.with_storage(|| ctx.push_btree_group(&mut face_native_candidates, (history_index, feature_index), native, "collect SLDPRT native face references", "collect SLDPRT native face references"))?;
                if let Some(&target_index) = ctx.get_hash_map(&features_by_source[history_index], &owner, "find SLDPRT face owner feature")? {
                    let target = &histories[history_index].features[target_index].id;
                    let target = temporary_storage.with_storage(|| ctx.format_retained(
                        format_args!("{target}"),
                        "retain SLDPRT face feature reference",
                    ))?;
                    temporary_storage.with_storage(|| ctx.push_btree_group(&mut face_feature_candidates, (history_index, feature_index), target, "collect SLDPRT face feature references", "collect SLDPRT face feature references"))?;
                }
            }
            if let Some((relative_offset, components)) =
                component_face_reference_in_record(ctx, bytes)?
            {
                let mut native = temporary_storage.with_storage(|| ctx.format_retained(
                    format_args!(
                        "sldprt:feature-input:surface-component-ids#{}:{}:",
                        lane.id,
                        start + relative_offset,
                    ),
                    "retain SLDPRT component face reference",
                ))?;
                for (position, local_id) in ctx.admit_iter(&components, "scan SLDPRT component face references")?
                    .filter_map(|component| component.local_id)
                    .enumerate()
                {
                    if position > 0 {
                        temporary_storage.with_storage(|| ctx.push_retained_char(
                            &mut native, ',', "retain SLDPRT component face reference",
                        ))?;
                    }
                    let digits =
                        usize::try_from(local_id.checked_ilog10().unwrap_or(0)).map_err(|_| {
                            ctx.refuse_codec_limit(
                                "retain SLDPRT component face reference",
                                u64::MAX - 1,
                                u64::MAX,
                            )
                        })? + 1;
                    temporary_storage.with_storage(|| ctx.try_reserve_retained_text(
                        &mut native, digits, "retain SLDPRT component face reference",
                    ))?;
                    std::fmt::Write::write_fmt(&mut native, format_args!("{local_id}")).map_err(
                        |_| {
                            ctx.refuse_codec_limit(
                                "retain SLDPRT component face reference",
                                u64::MAX - 1,
                                u64::MAX,
                            )
                        },
                    )?;
                }
                temporary_storage.with_storage(|| ctx.push_btree_group(&mut face_native_candidates, (history_index, feature_index), native, "collect SLDPRT native face references", "collect SLDPRT native face references"))?;
            }
            let offset_frames = crate::history::literals::named_literal(ctx, &feature.parameters, "D1", "parse SLDPRT plane distance parameter")?
                .and_then(|value| crate::history::literals::parse_dimension_length_mm(value));
            let offset_frames = match offset_frames {
                Some(distance) => offset_reference_plane_frame_pair(ctx, bytes, distance)?,
                None => None,
            };
            if let Some((offset, reference)) = offset_frames {
                let index = (history_index, feature_index);
                temporary_storage.with_storage(|| ctx.push_btree_group(&mut reference_frame_candidates, index, reference, "collect SLDPRT reference plane frame candidates", "collect SLDPRT reference plane frame candidates"))?;
                temporary_storage.with_storage(|| ctx.push_btree_group(&mut candidates, index, offset, "collect SLDPRT plane frame candidates", "collect SLDPRT plane frame candidates"))?;
                temporary_storage.with_storage(|| ctx.push_btree_group(&mut candidate_sources, index, SketchPlaneUAxisSource::Native, "collect SLDPRT plane U-axis sources", "collect SLDPRT plane U-axis sources"))?;
            }
            let constraint = constraint_midplane_frame(ctx, bytes)?;
            let mut frame_storage = ctx.reserve_scoped(0, "SLDPRT anchored reference frame workspace")?;
            let mut anchored_frames = Vec::new();
            let first_class = ctx.partition_point(&classes, |class| Ok(class.offset < u64_from_index(start)), "find SLDPRT reference plane classes")?;
            let last_class = ctx.partition_point(&classes, |class| Ok(class.offset < u64_from_index(end)), "find SLDPRT reference plane classes")?;
            for class in ctx.admit_iter(&classes[first_class..last_class], "scan SLDPRT reference plane classes")? {
                let frame = (|| {
                    let offset = usize::try_from(class.offset).ok()?;
                    (start..end).contains(&offset).then(|| {
                        constraint_reference_plane_frame(&lane.native_payload, offset, &class.name)
                    })?
                })();
                if let Some(frame) = frame {
                    frame_storage.with_storage(|| ctx.push_vec(&mut anchored_frames, frame, "collect SLDPRT anchored plane frames"))?;
                }
            }
            ctx.stable_sort_by_key(
                &mut anchored_frames,
                reference_plane_frame_key,
                Ord::cmp,
                "sort SLDPRT reference frames",
            )?;
            ctx.dedup_by_key(
                &mut anchored_frames,
                |frame| Ok(reference_plane_frame_key(frame)),
                "deduplicate SLDPRT reference plane frames",
            )?;
            let explicit = if offset_frames.is_some() {
                None
            } else if anchored_frames.is_empty() {
                match explicit_reference_plane_frame(ctx, bytes)? {
                    Ok(frame) => frame,
                    Err(()) if constraint.is_some() => None,
                    Err(()) => continue,
                }
            } else {
                let [frame] = anchored_frames.as_slice() else {
                    continue;
                };
                Some(*frame)
            };
            let Some((frame, u_axis_source)) =
                reconcile_reference_plane_frame_with_source(explicit, constraint)
            else {
                continue;
            };
            let (origin, normal, u_axis) = frame;
            let index = (history_index, feature_index);
            temporary_storage.with_storage(|| ctx.push_btree_group(&mut candidates, index, (origin, normal, u_axis), "collect SLDPRT plane frame candidates", "collect SLDPRT plane frame candidates"))?;
            temporary_storage.with_storage(|| ctx.push_btree_group(&mut candidate_sources, index, u_axis_source, "collect SLDPRT plane U-axis sources", "collect SLDPRT plane U-axis sources"))?;
        }
    }
    let mut face_reference_indices = BTreeSet::new();
    for &index in ctx
        .admit_iter(&face_native_candidates, "index SLDPRT face references")?
        .map(|(index, _)| index)
        .chain(
            ctx.admit_iter(&face_feature_candidates, "index SLDPRT face references")?
                .map(|(index, _)| index),
        )
    {
        temporary_storage.with_storage(|| {
            ctx.insert_btree_set(
                &mut face_reference_indices,
                index,
                "index SLDPRT face references",
            )
        })?;
    }
    for index in ctx.admit_iter(&face_reference_indices, "clear SLDPRT face references")? {
        ctx.remove_btree_map(&mut reference_candidates, index, "clear SLDPRT face references")?;
        ctx.remove_hash_set(&mut explicit_reference_indices, index, "clear SLDPRT face references")?;
    }
    for ((history_index, feature_index), mut native) in ctx.admit_iter(face_native_candidates, "apply SLDPRT face native references")? {
        ctx.sort_unstable_by(
            &mut native,
            |value| value,
            Ord::cmp,
            "sort SLDPRT face native references",
        )?;
        ctx.dedup_vec(&mut native, "deduplicate SLDPRT face native references")?;
        if let [native] = native.as_slice() {
            insert_reference_plane_property(
                ctx,
                &mut histories[history_index].features[feature_index],
                cadmpeg_core::nonblank_literal!("ReferenceFaceNative"),
                format_args!("{native}"),
            )?;
        }
    }
    for ((history_index, feature_index), mut targets) in ctx.admit_iter(face_feature_candidates, "apply SLDPRT face feature references")? {
        ctx.sort_unstable_by(
            &mut targets,
            |value| value,
            Ord::cmp,
            "sort SLDPRT face feature targets",
        )?;
        ctx.dedup_vec(&mut targets, "deduplicate SLDPRT face feature targets")?;
        if let [target] = targets.as_slice() {
            insert_reference_plane_property(
                ctx,
                &mut histories[history_index].features[feature_index],
                cadmpeg_core::nonblank_literal!("ReferenceFaceFeature"),
                format_args!("{target}"),
            )?;
        }
    }
    let mut unique_frames = BTreeMap::new();
    for (&index, frames) in ctx.admit_iter(&candidates, "select SLDPRT unique plane frames")? {
        if let Some(&frame) = frames.first() {
            if ctx.all_by(
                &frames[1..],
                |candidate| Ok(*candidate == frame),
                "compare SLDPRT plane frame candidates",
            )? {
                temporary_storage.with_storage(|| {
                    ctx.insert_btree_map(
                        &mut unique_frames,
                        index,
                        frame,
                        "index SLDPRT unique plane frames",
                    )
                })?;
            }
        }
    }
    let mut unique_u_axis_sources = HashMap::new();
    for (&index, sources) in
        ctx.admit_iter(&candidate_sources, "select SLDPRT plane U-axis sources")?
    {
        let source = ctx.find_by(sources, |source| Ok(**source == SketchPlaneUAxisSource::Native),
            "select SLDPRT plane U-axis sources")?
            .copied()
            .or_else(|| sources.first().copied());
        if let Some(source) = source {
            temporary_storage.with_storage(|| {
                ctx.insert_hash_map(
                    &mut unique_u_axis_sources,
                    index,
                    source,
                    "index SLDPRT plane U-axis sources",
                )
            })?;
        }
    }
    let mut unique_reference_frames = BTreeMap::new();
    for (&index, frames) in ctx.admit_iter(
        &reference_frame_candidates,
        "select SLDPRT unique reference frames",
    )? {
        if let Some(&frame) = frames.first() {
            if ctx.all_by(&frames[1..], |candidate| Ok(*candidate == frame),
                "compare SLDPRT reference frame candidates")?
            {
                temporary_storage.with_storage(|| {
                    ctx.insert_btree_map(
                        &mut unique_reference_frames,
                        index,
                        frame,
                        "index SLDPRT unique reference frames",
                    )
                })?;
            }
        }
    }
    let mut frames_by_reference = Vec::new();
    for (history_index, history) in ctx
        .admit_iter(&*histories, "collect SLDPRT reference frames")?
        .enumerate()
    {
        for (feature_index, feature) in ctx
            .admit_iter(&history.features, "collect SLDPRT reference frames")?
            .enumerate()
        {
            let Some(plane) = principal_plane_in_layout(principal_layouts[history_index], feature)
            else {
                continue;
            };
            let reference = temporary_storage.with_storage(|| retained_plane_frame_source(ctx, feature))?;
            temporary_storage.with_storage(|| ctx.push_vec(&mut frames_by_reference, ReferencePlaneFrameCandidate {
                source: reference,
                history_index,
                feature_index,
                frame: principal_sketch_frame(plane),
            }, "collect SLDPRT reference frames"))?;
        }
    }
    for (&index, &frame) in ctx.admit_iter(&unique_frames, "collect SLDPRT reference frames")? {
        let feature = &histories[index.0].features[index.1];
        let reference = temporary_storage.with_storage(|| retained_plane_frame_source(ctx, feature))?;
        temporary_storage.with_storage(|| ctx.push_vec(&mut frames_by_reference, ReferencePlaneFrameCandidate {
            source: reference,
            history_index: index.0,
            feature_index: index.1,
            frame,
        }, "collect SLDPRT reference frames"))?;
    }
    for (&index, frames) in ctx.admit_iter(
        &reference_frame_candidates,
        "match SLDPRT reference plane frames",
    )? {
        if ctx.contains_key_btree_map(&reference_candidates, &index, "find SLDPRT reference plane candidate")? || ctx.contains_btree_set(&face_reference_indices, &index, "find SLDPRT face reference")? {
            continue;
        }
        let mut sources = Vec::new();
        for &reference in ctx.admit_iter(frames, "match SLDPRT reference plane frames")? {
            let selected = select_reference_plane_frame_source(
                ctx,
                &frames_by_reference,
                |candidate| {
                    candidate.history_index == index.0
                        && (candidate.feature_index < index.1
                            || principal_plane_in_layout(
                                principal_layouts[candidate.history_index],
                                &histories[candidate.history_index].features
                                    [candidate.feature_index],
                            )
                            .is_some())
                        && offset_plane_reference_frame_matches(candidate.frame, reference, 0.0)
                },
                "select SLDPRT inferred plane reference",
            )?;
            if let Some(source) = selected {
                temporary_storage.with_storage(|| ctx.push_vec(&mut sources, source, "collect SLDPRT inferred plane sources"))?;
            }
        }
        ctx.sort_unstable_by(
            &mut sources,
            |value| value,
            Ord::cmp,
            "sort SLDPRT inferred plane sources",
        )?;
        ctx.dedup_vec(&mut sources, "deduplicate SLDPRT inferred plane sources")?;
        if let [source] = sources.as_slice() {
            let source = temporary_storage.with_storage(|| ctx.format_retained(
                format_args!("{source}"),
                "retain SLDPRT inferred plane source",
            ))?;
            temporary_storage.with_storage(|| ctx.push_btree_group(&mut reference_candidates, index, source, "collect SLDPRT reference plane sources", "collect SLDPRT reference plane sources"))?;
        }
    }
    for (&index, &frame) in ctx.admit_iter(&unique_frames, "match SLDPRT offset plane frames")? {
        let feature = &histories[index.0].features[index.1];
        if !ctx.contains_key_btree_map(&feature.parameters, "D1", "find SLDPRT plane distance parameter")?
            || ctx.contains_key_btree_map(&reference_candidates, &index, "find SLDPRT reference plane candidate")?
            || ctx.contains_btree_set(&face_reference_indices, &index, "find SLDPRT face reference")?
        {
            continue;
        }
        let Some(distance) = crate::history::literals::named_literal(ctx, &feature.parameters, "D1", "parse SLDPRT plane distance parameter")?
            .and_then(|value| crate::history::literals::parse_dimension_length_mm(value))
        else {
            continue;
        };
        if let Some(source) = select_reference_plane_frame_source(
            ctx,
            &frames_by_reference,
            |candidate| {
                candidate.history_index == index.0
                    && offset_plane_reference_frame_matches(candidate.frame, frame, distance.get())
            },
            "select SLDPRT offset plane reference",
        )? {
            let source = temporary_storage.with_storage(|| ctx.format_retained(
                format_args!("{source}"),
                "retain SLDPRT offset plane source",
            ))?;
            temporary_storage.with_storage(|| ctx.push_btree_group(&mut reference_candidates, index, source, "collect SLDPRT reference plane sources", "collect SLDPRT reference plane sources"))?;
        }
    }
    for ((history_index, feature_index), mut sources) in ctx.admit_iter(reference_candidates, "apply SLDPRT reference plane candidates")? {
        let index = (history_index, feature_index);
        if !ctx.contains_hash_set(&explicit_reference_indices, &index, "find SLDPRT explicit plane reference")? {
            if let Some(offset) = ctx.get_btree_map(&unique_frames, &index, "find SLDPRT reference plane frame")? {
                if let Some(distance) = crate::history::literals::named_literal(ctx, &histories[history_index].features[feature_index].parameters, "D1", "parse SLDPRT plane distance parameter")?
                    .and_then(|value| crate::history::literals::parse_dimension_length_mm(value))
                {
                    let compatible = |source: &String| -> Result<bool, CodecError> {
                        ctx.any_by(&frames_by_reference, |candidate| {
                            Ok(candidate.history_index == history_index
                                && ctx.equal(&candidate.source, source, "compare SLDPRT compatible plane sources")?
                                && offset_plane_reference_frame_matches(candidate.frame, *offset, distance.get()))
                        }, "match SLDPRT compatible plane sources")
                    };
                    let has_compatible = ctx.any_by(&sources, compatible, "select SLDPRT compatible plane sources")?;
                    if has_compatible {
                        ctx.retain_vec(
                            &mut sources,
                            compatible,
                            "retain SLDPRT compatible plane sources",
                        )?;
                    }
                }
            }
        }
        ctx.sort_unstable_by(
            &mut sources,
            |value| value,
            Ord::cmp,
            "sort SLDPRT reference plane sources",
        )?;
        ctx.dedup_vec(&mut sources, "deduplicate SLDPRT reference plane sources")?;
        let [source] = sources.as_slice() else {
            continue;
        };
        insert_reference_plane_property(
            ctx,
            &mut histories[history_index].features[feature_index],
            cadmpeg_core::nonblank_literal!("Reference"),
            format_args!("{source}"),
        )?;
    }
    for (&(history_index, feature_index), &(origin, normal, u_axis)) in ctx.admit_iter(
        &unique_reference_frames,
        "write SLDPRT reference plane frames",
    )? {
        let feature = &mut histories[history_index].features[feature_index];
        insert_reference_plane_property(
            ctx,
            feature,
            cadmpeg_core::nonblank_literal!("ReferenceFaceOrigin"),
            format_args!("{}mm,{}mm,{}mm", origin.x, origin.y, origin.z),
        )?;
        insert_reference_plane_property(
            ctx,
            feature,
            cadmpeg_core::nonblank_literal!("ReferenceFaceNormal"),
            format_args!("{},{},{}", normal.x, normal.y, normal.z),
        )?;
        insert_reference_plane_property(
            ctx,
            feature,
            cadmpeg_core::nonblank_literal!("ReferenceFaceUAxis"),
            format_args!("{},{},{}", u_axis.x, u_axis.y, u_axis.z),
        )?;
    }
    for (&(history_index, feature_index), frames) in
        ctx.admit_iter(&candidates, "write SLDPRT reference plane frames")?
    {
        let Some(first_frame @ (origin, normal, u_axis)) = frames.first() else {
            continue;
        };
        let first_key = reference_plane_frame_key(first_frame);
        if !ctx.all_by(&frames[1..], |candidate| Ok(reference_plane_frame_key(candidate) == first_key), "select unique SLDPRT reference plane frame")? { continue; }
        let feature = &mut histories[history_index].features[feature_index];
        insert_reference_plane_property(
            ctx,
            feature,
            cadmpeg_core::nonblank_literal!("Origin"),
            format_args!("{}mm,{}mm,{}mm", origin.x, origin.y, origin.z),
        )?;
        insert_reference_plane_property(
            ctx,
            feature,
            cadmpeg_core::nonblank_literal!("Normal"),
            format_args!("{},{},{}", normal.x, normal.y, normal.z),
        )?;
        insert_reference_plane_property(
            ctx,
            feature,
            cadmpeg_core::nonblank_literal!("UAxis"),
            format_args!("{},{},{}", u_axis.x, u_axis.y, u_axis.z),
        )?;
        if ctx.get_hash_map(&unique_u_axis_sources, &(history_index, feature_index), "find SLDPRT plane U-axis provenance")?
            == Some(&SketchPlaneUAxisSource::ConstructedMidPlane)
        {
            insert_reference_plane_property(
                ctx,
                feature,
                cadmpeg_core::nonblank_const!(REFERENCE_PLANE_U_AXIS_SOURCE_PROPERTY),
                format_args!("{CONSTRUCTED_MID_PLANE_U_AXIS_SOURCE}"),
            )?;
        } else {
            ctx.remove_btree_map(&mut feature.properties, REFERENCE_PLANE_U_AXIS_SOURCE_PROPERTY,
                "remove SLDPRT plane U-axis source")?;
        }
    }
    Ok(())
}

/// Add solved model-space positions to reference-point history records.
pub(crate) fn enrich_history_reference_points(
    ctx: &DecodeContext<'_>,
    histories: &mut [crate::records::FeatureHistory],
    lanes: &[FeatureInputLane],
) -> Result<(), CodecError> {
    let mut temporary_storage = ctx.reserve_scoped(0, "SLDPRT reference point workspace")?;
    let mut candidates = BTreeMap::<(usize, usize), Option<Point3>>::new();
    for lane in ctx.admit_iter(lanes, "scan SLDPRT reference point lanes")? {
        let mut lane_storage = ctx.reserve_scoped(0, "SLDPRT reference lane workspace")?;
        let object_names = ObjectNames::new(ctx, lane)?;
        let mut starts = Vec::new();
        for (history_index, history) in ctx
            .admit_iter(&*histories, "scan SLDPRT reference point features")?
            .enumerate()
        {
            for (feature_index, feature) in ctx
                .admit_iter(&history.features, "scan SLDPRT reference point features")?
                .enumerate()
            {
                if let Some(name) = object_names.of(ctx, feature)? {
                    lane_storage.with_storage(|| ctx.push_vec(&mut starts, (name.offset, history_index, feature_index), "collect SLDPRT reference point starts"))?;
                }
            }
        }
        ctx.stable_sort_by(
            &mut starts,
            |value| &value.0,
            Ord::cmp,
            "sort SLDPRT feature starts",
        )?;
        for (index, &(_, history_index, feature_index)) in ctx
            .admit_iter(&starts, "scan SLDPRT reference point starts")?
            .enumerate()
        {
            let feature = &histories[history_index].features[feature_index];
            if feature.input_class.as_deref() != Some("moRefPoint_c")
                || ctx.contains_key_btree_map(&feature.properties, "Position", "find SLDPRT reference geometry property")?
            {
                continue;
            }
            let Some(name) = object_names.of(ctx, feature)? else {
                continue;
            };
            let record_end = starts
                .get(index + 1)
                .and_then(|next| usize::try_from(next.0).ok())
                .unwrap_or(lane.native_payload.len());
            if let Some(point) =
                resolved_reference_point(ctx, &lane.native_payload, name, record_end)?
            {
                let key = (history_index, feature_index);
                if let Some(existing) = ctx.get_mut_btree_map(&mut candidates, &key, "merge SLDPRT reference geometry candidates")? {
                    if existing.is_some_and(|existing| {
                        reference_point_key(&existing) != reference_point_key(&point)
                    }) {
                        *existing = None;
                    }
                } else {
                    temporary_storage.with_storage(|| ctx.insert_btree_map(
                        &mut candidates,
                        key,
                        Some(point),
                        "collect SLDPRT reference point candidates",
                    ))?;
                }
            }
        }
    }

    for (&(history_index, feature_index), point) in
        ctx.admit_iter(&candidates, "write SLDPRT reference point positions")?
    {
        let Some(point) = *point else {
            continue;
        };
        let value = ctx.format_retained(
            format_args!("{}mm,{}mm,{}mm", point.x, point.y, point.z),
            "retain SLDPRT reference point position",
        )?;
        ctx.insert_btree_map(
            &mut histories[history_index].features[feature_index].properties,
            cadmpeg_core::nonblank_literal!("Position"),
            value,
            "insert SLDPRT reference point position",
        )?;
    }
    Ok(())
}

fn resolved_reference_point(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
    name: &FeatureInputName,
    record_end: usize,
) -> Result<Option<Point3>, CodecError> {
    const HEADER_PREFIX: [u8; 8] = [0, 0, 0, 0, 0, 0, 0, 0xc0];
    const NATIVE_TO_IR: f64 = 1000.0;

    let Some(object_id) = name.object_id.and_then(ObjectId::value) else {
        return Ok(None);
    };
    let Ok(name_start) = usize::try_from(name.offset) else {
        return Ok(None);
    };
    let utf16_units = ctx
        .admit_iter(name.value.as_str(), "measure SLDPRT reference point name")?
        .try_fold(0usize, |units, character| {
            units.checked_add(character.len_utf16()).ok_or_else(|| {
                ctx.refuse_codec_limit(
                    "measure SLDPRT reference point name",
                    u64::MAX - 1,
                    u64::MAX,
                )
            })
        })?;
    let Some(name_end) = name_start
        .checked_add(NAME_MARKER.len() + 1)
        .and_then(|offset| {
            utf16_units
                .checked_mul(2)
                .and_then(|length| offset.checked_add(length))
        })
    else {
        return Ok(None);
    };
    let Some(header_end) = name_end.checked_add(16) else {
        return Ok(None);
    };
    let Some(header) = payload.get(name_end..header_end) else {
        return Ok(None);
    };
    if header[..HEADER_PREFIX.len()] != HEADER_PREFIX
        || header[pt_short::OBJECT_ID..pt_short::ZERO_AFTER_ID] != object_id.to_le_bytes()
        || header[pt_short::ZERO_AFTER_ID..pt_short::ZERO_AFTER_ID + 4] != [0; 4]
    {
        return Ok(None);
    }

    let points = [
        (
            pt_short::ZERO_BEFORE_POSITION,
            pt_short::POSITION,
            pt_short::CONSTRUCTION_FORM,
            pt_short::ZERO_TRAILER,
            pt_short::LEN,
        ),
        (
            pt_long::ZERO_BEFORE_POSITION,
            pt_long::POSITION,
            pt_long::CONSTRUCTION_FORM,
            pt_long::ZERO_TRAILER,
            pt_long::LEN,
        ),
    ]
    .into_iter()
    .filter_map(|(zero_before, position, form, trailer, len)| {
        let start = name_end.checked_add(position)?;
        let end = name_end.checked_add(len)?;
        if end > record_end
            || payload.get(name_end + zero_before..start) != Some(&[0; 16])
            || !matches!(View::u16_le_at(payload, name_end + form)?, 4 | 5)
            || payload.get(name_end + trailer..end) != Some(&[0; 8])
        {
            return None;
        }
        let scalar = |offset: usize| {
            let native = View::f64_le_at(payload, start + offset)?;
            let value = native * NATIVE_TO_IR;
            value.is_finite().then_some(value)
        };
        Some(Point3::new(scalar(0)?, scalar(8)?, scalar(16)?))
    });
    let mut unique = None;
    for point in points {
        match unique {
            Some(existing) if reference_point_key(&existing) != reference_point_key(&point) => {
                return Ok(None);
            }
            None => unique = Some(point),
            Some(_) => {}
        }
    }
    Ok(unique)
}

fn reference_point_key(point: &Point3) -> [u64; 3] {
    [
        (point.x + 0.0).to_bits(),
        (point.y + 0.0).to_bits(),
        (point.z + 0.0).to_bits(),
    ]
}

/// Add solved model-space frames to complete coordinate-system history records.
pub(crate) fn enrich_history_coordinate_systems(
    ctx: &DecodeContext<'_>,
    histories: &mut [crate::records::FeatureHistory],
    lanes: &[FeatureInputLane],
) -> Result<(), CodecError> {
    let mut temporary_storage = ctx.reserve_scoped(0, "SLDPRT coordinate system workspace")?;
    let mut candidates =
        BTreeMap::<(usize, usize), Option<(Point3, Vector3, Vector3, Vector3)>>::new();
    for lane in ctx.admit_iter(lanes, "scan SLDPRT coordinate system lanes")? {
        let mut lane_storage = ctx.reserve_scoped(0, "SLDPRT reference lane workspace")?;
        let object_names = ObjectNames::new(ctx, lane)?;
        let mut starts = Vec::new();
        for (history_index, history) in ctx
            .admit_iter(&*histories, "scan SLDPRT coordinate system features")?
            .enumerate()
        {
            for (feature_index, feature) in ctx
                .admit_iter(&history.features, "scan SLDPRT coordinate system features")?
                .enumerate()
            {
                if let Some(name) = object_names.of(ctx, feature)? {
                    lane_storage.with_storage(|| ctx.push_vec(&mut starts, (name.offset, history_index, feature_index), "collect SLDPRT coordinate system starts"))?;
                }
            }
        }
        ctx.stable_sort_by(
            &mut starts,
            |value| &value.0,
            Ord::cmp,
            "sort SLDPRT feature starts",
        )?;
        for (index, &(start, history_index, feature_index)) in ctx
            .admit_iter(&starts, "scan SLDPRT coordinate system starts")?
            .enumerate()
        {
            let feature = &histories[history_index].features[feature_index];
            if feature.input_class.as_deref() != Some("moCoordSys_c")
                || ctx.contains_key_btree_map(&feature.properties, "Origin", "find SLDPRT reference geometry property")?
                || ctx.contains_key_btree_map(&feature.properties, "XAxis", "find SLDPRT reference geometry property")?
                || ctx.contains_key_btree_map(&feature.properties, "YAxis", "find SLDPRT reference geometry property")?
                || ctx.contains_key_btree_map(&feature.properties, "ZAxis", "find SLDPRT reference geometry property")?
            {
                continue;
            }
            let end = starts
                .get(index + 1)
                .and_then(|next| usize::try_from(next.0).ok())
                .unwrap_or(lane.native_payload.len());
            let Ok(start) = usize::try_from(start) else {
                continue;
            };
            let Some(record) = lane.native_payload.get(start..end) else {
                continue;
            };
            if let Some(frame) = resolved_coordinate_system(ctx, record)? {
                let key = (history_index, feature_index);
                if let Some(existing) = ctx.get_mut_btree_map(&mut candidates, &key, "merge SLDPRT reference geometry candidates")? {
                    if existing.is_some_and(|existing| {
                        coordinate_system_frame_key(&existing)
                            != coordinate_system_frame_key(&frame)
                    }) {
                        *existing = None;
                    }
                } else {
                    temporary_storage.with_storage(|| ctx.insert_btree_map(
                        &mut candidates,
                        key,
                        Some(frame),
                        "collect SLDPRT coordinate system candidates",
                    ))?;
                }
            }
        }
    }

    for (&(history_index, feature_index), frame) in
        ctx.admit_iter(&candidates, "write SLDPRT coordinate system frames")?
    {
        let Some((origin, x_axis, y_axis, z_axis)) = *frame else {
            continue;
        };
        let feature = &mut histories[history_index].features[feature_index];
        let origin_text = ctx.format_retained(
            format_args!("{}mm,{}mm,{}mm", origin.x, origin.y, origin.z),
            "retain SLDPRT coordinate system origin",
        )?;
        ctx.insert_btree_map(
            &mut feature.properties,
            cadmpeg_core::nonblank_literal!("Origin"),
            origin_text,
            "insert SLDPRT coordinate system origin",
        )?;
        for (name, axis) in [
            (cadmpeg_core::nonblank_literal!("XAxis"), x_axis),
            (cadmpeg_core::nonblank_literal!("YAxis"), y_axis),
            (cadmpeg_core::nonblank_literal!("ZAxis"), z_axis),
        ] {
            let text = ctx.format_retained(
                format_args!("{},{},{}", axis.x, axis.y, axis.z),
                "retain SLDPRT coordinate system axis",
            )?;
            ctx.insert_btree_map(
                &mut feature.properties,
                name,
                text,
                "insert SLDPRT coordinate system axis",
            )?;
        }
    }
    Ok(())
}

fn resolved_coordinate_system(
    ctx: &DecodeContext<'_>,
    record: &[u8],
) -> Result<Option<(Point3, Vector3, Vector3, Vector3)>, CodecError> {
    if let Some(frame) = coordinate_system_two_point_frame(ctx, record)? {
        return Ok(Some(frame));
    }
    let Some((origin, generation, origin_end)) = coordinate_system_origin(ctx, record)? else {
        return Ok(None);
    };
    let mut axes = coordinate_system_line_axes(ctx, record, generation, origin_end)?;
    let Some(first_axis) = axes.next().transpose()? else {
        let Some((x_axis, y_axis)) =
            coordinate_system_ordinal_axes(record, origin_end, origin)
        else {
            return Ok(None);
        };
        let Some(z_axis) = x_axis.cross(y_axis).unit() else {
            return Ok(None);
        };
        return Ok(Some((origin, x_axis, y_axis, z_axis)));
    };
    let second_axis = axes.next().transpose()?;
    if axes.next().transpose()?.is_some() { return Ok(None); }
    Ok((|| {
        let (mut x_axis, mut y_axis, tail_offsets) = match (first_axis, second_axis) {
            ((offset, point, direction), None) => (
                direction,
                Vector3::new(point.x - origin.x, point.y - origin.y, point.z - origin.z),
                [
                    Some((offset.checked_add(line_axis::LEN)?, false)),
                    Some((offset.checked_add(line_axis::LEN + 2)?, true)),
                ],
            ),
            ((first_offset, _, first_direction), Some((last_offset, _, last_direction)))
                if first_offset < last_offset =>
            {
                (
                    first_direction,
                    last_direction,
                    [
                        Some((last_offset.checked_add(line_axis::LEN)?, false)),
                        None,
                    ],
                )
            }
            _ => return None,
        };
        let flips = coordinate_system_tail(record, tail_offsets.into_iter().flatten(), origin)?;

        if flips[0] == 1 {
            x_axis = Vector3::new(-x_axis.x, -x_axis.y, -x_axis.z);
        }
        if flips[1] == 1 {
            y_axis = Vector3::new(-y_axis.x, -y_axis.y, -y_axis.z);
        }
        let x_axis = x_axis.unit()?;
        let projection = x_axis.dot(y_axis);
        let y_axis = Vector3::new(
            y_axis.x - projection * x_axis.x,
            y_axis.y - projection * x_axis.y,
            y_axis.z - projection * x_axis.z,
        )
        .unit()?;
        let z_axis = x_axis.cross(y_axis).unit()?;
        Some((origin, x_axis, y_axis, z_axis))
    })())
}

fn coordinate_system_ordinal_axes(
    record: &[u8],
    origin_end: usize,
    origin: Point3,
) -> Option<(Vector3, Vector3)> {
    let Some(tail) = record.get(origin_end..) else {
        return None;
    };
    if !matches!(tail.len(), 37 | 39)
        || tail.get(ordinal_tail::ZERO_BEFORE_ORIGIN_Z..ordinal_tail::ORIGIN_Z) != Some(&[0; 23])
    {
        return None;
    }
    let Some(ordinal_bytes) = tail.get(ordinal_tail::LEN..) else {
        return None;
    };
    if ordinal_bytes.chunks(2)
        .any(|token| token == [0, 0])
    {
        return None;
    }
    let (Some(x_ordinal), Some(y_ordinal)) = (
        View::u16_le_at(tail, ordinal_tail::X_AXIS_ORDINAL),
        View::u16_le_at(tail, ordinal_tail::Y_AXIS_ORDINAL),
    ) else {
        return None;
    };
    let ordinals = [usize::from(x_ordinal), usize::from(y_ordinal)];
    if ordinals[0] == ordinals[1] || ordinals.iter().any(|ordinal| !(1..=3).contains(ordinal)) {
        return None;
    }
    let Some(repeated_z) = finite_f64(tail, ordinal_tail::ORIGIN_Z) else {
        return None;
    };
    let repeated_z = repeated_z * 1000.0;
    if (repeated_z + 0.0).to_bits() != (origin.z + 0.0).to_bits() {
        return None;
    }
    let basis = [
        Vector3::new(1.0, 0.0, 0.0),
        Vector3::new(0.0, 1.0, 0.0),
        Vector3::new(0.0, 0.0, 1.0),
    ];
    Some((basis[ordinals[0] - 1], basis[ordinals[1] - 1]))
}

fn coordinate_system_tail(
    record: &[u8],
    offsets: impl IntoIterator<Item = (usize, bool)>,
    origin: Point3,
) -> Option<[u8; 3]> {
    let mut candidates = offsets.into_iter().filter_map(|(offset, has_zero_gap)| {
        if has_zero_gap && record.get(offset.checked_sub(2)?..offset) != Some(&[0; 2]) {
            return None;
        }
        let bytes = record.get(offset..offset.checked_add(xy_tail::LEN)?)?;
        let flips: [u8; 3] = bytes.get(..xy_tail::ORIGIN)?.try_into().ok()?;
        if flips.iter().any(|value| !matches!(value, 0 | 1))
            || flips[2] != 0
            || bytes.get(xy_tail::TERMINATOR..xy_tail::LEN) == Some(&[0, 0])
        {
            return None;
        }
        let tail_origin = Point3::new(
            finite_f64(bytes, xy_tail::ORIGIN)? * 1000.0,
            finite_f64(bytes, xy_tail::ORIGIN + 8)? * 1000.0,
            finite_f64(bytes, xy_tail::ORIGIN + 16)? * 1000.0,
        );
        (reference_point_key(&tail_origin) == reference_point_key(&origin)).then_some(flips)
    });
    let flips = candidates.next()?;
    candidates.next().is_none().then_some(flips)
}

#[derive(Clone, Copy)]
struct CoordinateSystemOrigin {
    point: Point3,
    generation: u32,
    start: usize,
    end: usize,
    extended: bool,
}

fn coordinate_system_origin(
    ctx: &DecodeContext<'_>,
    record: &[u8],
) -> Result<Option<(Point3, u32, usize)>, CodecError> {
    let mut candidates = coordinate_system_origins(ctx, record)?;
    let candidate = if let Some(candidate) = candidates.next().transpose()? {
        if candidates.next().transpose()?.is_some() {
            return Ok(None);
        }
        candidate
    } else {
        let mut endpoint_candidates = coordinate_system_endpoint_origins(ctx, record)?;
        let Some(candidate) = endpoint_candidates.next().transpose()? else {
            return Ok(None);
        };
        if endpoint_candidates.next().transpose()?.is_some() {
            return Ok(None);
        }
        candidate
    };
    Ok(Some((candidate.point, candidate.generation, candidate.end)))
}

fn coordinate_system_endpoint_origins<'a>(
    ctx: &'a DecodeContext<'_>,
    record: &'a [u8],
) -> Result<impl Iterator<Item = Result<CoordinateSystemOrigin, CodecError>> + 'a, CodecError> {
    const PREFIX: &[u8] = &[
        0x2f, 0x80, 0x02, 0, 0, 0, 0x40, 0, 0, 0x75, 0, 0, 0, 0x75, 0, 0, 0,
    ];
    const NULL_SLOT: &[u8] = &[0xff, 0xff, 0xff, 0xff, 0, 0, 0, 0];
    const HANDLES: &[u8] = &[0xc7, 0xcf, 0xff, 0xff, 0xc7, 0xcf, 0xff, 0xff];
    let Some(window_width) = std::num::NonZeroUsize::new(COMPACT_EDGE_VECTOR_MARKER.len()) else {
        return Err(ctx.refuse_codec_limit("scan SLDPRT coordinate system origins", 1, 0));
    };
    let mut windows = record.windows(window_width.get()).enumerate();
    let mut parse = move |(marker, bytes): (usize, &[u8])| -> Result<Option<CoordinateSystemOrigin>, CodecError> {
                if bytes != COMPACT_EDGE_VECTOR_MARKER {
                    return Ok(None);
                }

                let header = (|| {
                    let prefix = marker.checked_sub(ep_path_pre::COMPONENT_MARKER)?;
                    if record.get(prefix..prefix + ep_path_pre::ZERO_HEADER)? != PREFIX
                        || record.get(
                            prefix + ep_path_pre::ZERO_HEADER..prefix + ep_path_pre::SENTINEL,
                        )? != [0; 28]
                        || record.get(
                            prefix + ep_path_pre::SENTINEL
                                ..prefix + ep_path_pre::ZERO_BEFORE_SELECTOR,
                        )? != [0xff; 16]
                        || record.get(
                            prefix + ep_path_pre::ZERO_BEFORE_SELECTOR
                                ..prefix + ep_path_pre::SELECTOR,
                        )? != [0; 8]
                        || record.get(
                            prefix + ep_path_pre::ZERO_BEFORE_COUNT
                                ..prefix + ep_path_pre::PATH_ENTRY_COUNT,
                        )? != [0; 7]
                    {
                        return None;
                    }
                    let selector = View::u32_le_at(record, prefix + ep_path_pre::SELECTOR)?;
                    let token = View::u32_le_at(record, prefix + ep_path_pre::TOKEN)?;
                    if matches!(selector, 0 | u32::MAX) || matches!(token, 0 | u32::MAX) {
                        return None;
                    }
                    Some(prefix)
                })();
                let Some(prefix) = header else {
                    return Ok(None);
                };
                let Some(path_end) = compact_component_path_end_at(ctx, record, marker)? else {
                    return Ok(None);
                };
                Ok((|| {
                    if record.get(path_end..path_end + 8)? != NULL_SLOT {
                        return None;
                    }
                    let trailer = path_end.checked_add(8)?;
                    if record.get(trailer..trailer + ep_path_suf::ONE)? != [0; 70]
                        || record.get(
                            trailer + ep_path_suf::ONE..trailer + ep_path_suf::ZERO_BEFORE_OBJECT,
                        )? != 1u32.to_le_bytes()
                        || record.get(
                            trailer + ep_path_suf::ZERO_BEFORE_OBJECT
                                ..trailer + ep_path_suf::OBJECT_ID,
                        )? != [0; 4]
                        || record.get(
                            trailer + ep_path_suf::ZERO_BEFORE_HANDLES
                                ..trailer + ep_path_suf::HANDLES,
                        )? != [0; 12]
                    {
                        return None;
                    }
                    let object = View::u32_le_at(record, trailer + ep_path_suf::OBJECT_ID)?;
                    let handles = trailer.checked_add(ep_path_suf::HANDLES)?;
                    if matches!(object, 0 | u32::MAX)
                        || record.get(handles..handles + 8)? != HANDLES
                        || record.get(
                            trailer + ep_path_suf::ZERO_BEFORE_GENERATION
                                ..trailer + ep_path_suf::GENERATION,
                        )? != [0; 4]
                        || record.get(
                            trailer + ep_path_suf::ZERO_BEFORE_ORIGIN
                                ..trailer + ep_path_suf::ORIGIN,
                        )? != [0; 8]
                    {
                        return None;
                    }
                    let generation = View::u32_le_at(record, trailer + ep_path_suf::GENERATION)?;
                    if matches!(generation, 0 | u32::MAX) {
                        return None;
                    }
                    Some(CoordinateSystemOrigin {
                        point: Point3::new(
                            finite_f64(record, trailer + ep_path_suf::ORIGIN)? * 1000.0,
                            finite_f64(record, trailer + ep_path_suf::ORIGIN + 8)? * 1000.0,
                            finite_f64(record, trailer + ep_path_suf::ORIGIN + 16)? * 1000.0,
                        ),
                        generation,
                        start: prefix,
                        end: trailer.checked_add(ep_path_suf::LEN)?,
                        extended: false,
                    })
                })())
            };
    let mut done = false;
    Ok(std::iter::from_fn(move || {
        if done { return None; }
        let found = ctx.find_map(windows.by_ref(), &mut parse, "scan SLDPRT coordinate system origins");
        if !matches!(found, Ok(Some(_))) { done = true; }
        found.transpose()
    }))
}

fn coordinate_system_origins<'a>(
    ctx: &'a DecodeContext<'_>,
    record: &'a [u8],
) -> Result<impl Iterator<Item = Result<CoordinateSystemOrigin, CodecError>> + 'a, CodecError> {
    const PREFIX_SUFFIX: &[u8] = &[0x80, 0x02, 0, 0, 0, 0, 0, 0, 0];
    const HANDLES: &[u8] = &[0xc7, 0xcf, 0xff, 0xff, 0xc7, 0xcf, 0xff, 0xff];
    let Some(window_width) = std::num::NonZeroUsize::new(PREFIX_SUFFIX.len() + 1) else {
        return Err(ctx.refuse_codec_limit("scan SLDPRT coordinate system origins", 1, 0));
    };
    let mut windows = record.windows(window_width.get()).enumerate();
    let mut parse = move |(prefix, bytes): (usize, &[u8])| -> Result<Option<CoordinateSystemOrigin>, CodecError> {
                if !(matches!(bytes[0], 0x2d | 0x2f) && bytes[1..] == *PREFIX_SUFFIX) {
                    return Ok(None);
                }

                let header = (|| {
                    if record.get(prefix + cs_pt::ZERO_HEADER..prefix + cs_pt::SENTINEL)
                        != Some(&[0; 35])
                        || record.get(prefix + cs_pt::SENTINEL..prefix + cs_pt::ZERO_BEFORE_SOURCE)
                            != Some(&[0xff; 16])
                        || record.get(prefix + cs_pt::ZERO_BEFORE_SOURCE..prefix + cs_pt::SOURCE_ID)
                            != Some(&[0; 8])
                    {
                        return None;
                    }
                    let source = View::u32_le_at(record, prefix + cs_pt::SOURCE_ID)?;
                    if source == 0 {
                        return None;
                    }
                    Some(())
                })();
                if header.is_none() {
                    return Ok(None);
                }
                if let Some(candidate) =
                    coordinate_system_component_path_origin(ctx, record, prefix)?
                {
                    return Ok(Some(CoordinateSystemOrigin {
                        point: candidate.0,
                        generation: candidate.1,
                        start: prefix,
                        end: candidate.2,
                        extended: false,
                    }));
                }
                Ok((|| {
                    let stamp = View::u32_le_at(record, prefix + cs_pt::SOURCE_STAMP)?;
                    if stamp == 0 || stamp == u32::MAX {
                        return None;
                    }
                    let (object, generation, origin_offset, record_len, extended) = if record
                        .get(prefix + cs_pt::ZERO_SELECTOR..prefix + cs_pt::ONE_SELECTOR)
                        == Some(&[0; 2])
                        && record
                            .get(prefix + cs_pt::ONE_SELECTOR..prefix + cs_pt::ZERO_BEFORE_OBJECT)
                            == Some(&1u16.to_le_bytes())
                        && record.get(prefix + cs_pt::ZERO_BEFORE_OBJECT..prefix + cs_pt::OBJECT_ID)
                            == Some(&[0; 6])
                        && record.get(prefix + cs_pt::ZERO_BEFORE_HANDLES..prefix + cs_pt::HANDLES)
                            == Some(&[0; 12])
                        && record
                            .get(prefix + cs_pt::HANDLES..prefix + cs_pt::ZERO_BEFORE_GENERATION)
                            == Some(HANDLES)
                        && record
                            .get(prefix + cs_pt::ZERO_BEFORE_GENERATION..prefix + cs_pt::GENERATION)
                            == Some(&[0; 4])
                        && record.get(prefix + cs_pt::ZERO_BEFORE_ORIGIN..prefix + cs_pt::ORIGIN)
                            == Some(&[0; 8])
                    {
                        (
                            View::u32_le_at(record, prefix + cs_pt::OBJECT_ID)?,
                            View::u32_le_at(record, prefix + cs_pt::GENERATION)?,
                            cs_pt::ORIGIN,
                            cs_pt::LEN,
                            false,
                        )
                    } else if record
                        .get(prefix + cs_ext::SENTINEL..prefix + cs_ext::ZERO_BEFORE_COUNT)
                        == Some(&[0xff; 4])
                        && record.get(
                            prefix + cs_ext::ZERO_BEFORE_COUNT..prefix + cs_ext::REFERENCE_COUNT,
                        ) == Some(&[0; 4])
                        && record.get(prefix + cs_ext::ONE..prefix + cs_ext::ZERO_BEFORE_OBJECT)
                            == Some(&1u32.to_le_bytes())
                        && record
                            .get(prefix + cs_ext::ZERO_BEFORE_OBJECT..prefix + cs_ext::OBJECT_ID)
                            == Some(&[0; 4])
                        && record
                            .get(prefix + cs_ext::ZERO_BEFORE_HANDLES..prefix + cs_ext::HANDLES)
                            == Some(&[0; 12])
                        && record
                            .get(prefix + cs_ext::HANDLES..prefix + cs_ext::ZERO_BEFORE_GENERATION)
                            == Some(HANDLES)
                        && record.get(
                            prefix + cs_ext::ZERO_BEFORE_GENERATION..prefix + cs_ext::GENERATION,
                        ) == Some(&[0; 4])
                        && record.get(prefix + cs_ext::ZERO_BEFORE_ORIGIN..prefix + cs_ext::ORIGIN)
                            == Some(&[0; 8])
                    {
                        let reference = View::u32_le_at(record, prefix + cs_ext::REFERENCE_ID)?;
                        let count = View::u32_le_at(record, prefix + cs_ext::REFERENCE_COUNT)?;
                        if matches!(reference, 0 | u32::MAX) || matches!(count, 0 | u32::MAX) {
                            return None;
                        }
                        (
                            View::u32_le_at(record, prefix + cs_ext::OBJECT_ID)?,
                            View::u32_le_at(record, prefix + cs_ext::GENERATION)?,
                            cs_ext::ORIGIN,
                            cs_ext::LEN,
                            true,
                        )
                    } else {
                        return None;
                    };
                    if object == 0 || generation == 0 || generation == u32::MAX {
                        return None;
                    }
                    Some(CoordinateSystemOrigin {
                        point: Point3::new(
                            finite_f64(record, prefix + origin_offset)? * 1000.0,
                            finite_f64(record, prefix + origin_offset + 8)? * 1000.0,
                            finite_f64(record, prefix + origin_offset + 16)? * 1000.0,
                        ),
                        generation,
                        start: prefix,
                        end: prefix.checked_add(record_len)?,
                        extended,
                    })
                })())
            };
    let mut done = false;
    Ok(std::iter::from_fn(move || {
        if done { return None; }
        let found = ctx.find_map(windows.by_ref(), &mut parse, "scan SLDPRT coordinate system origins");
        if !matches!(found, Ok(Some(_))) { done = true; }
        found.transpose()
    }))
}

fn coordinate_system_two_point_frame(
    ctx: &DecodeContext<'_>,
    record: &[u8],
) -> Result<Option<(Point3, Vector3, Vector3, Vector3)>, CodecError> {
    let mut origins = coordinate_system_origins(ctx, record)?;
    let (Some(origin), Some(axis_point)) =
        (origins.next().transpose()?, origins.next().transpose()?)
    else {
        return Ok(None);
    };
    if origins.next().transpose()?.is_some() {
        return Ok(None);
    }
    Ok((|| {
        if !origin.extended || !axis_point.extended || origin.generation != axis_point.generation {
            return None;
        }
        let separator = record.get(origin.end..axis_point.start)?;
        if separator.len() != two_pt_sep::LEN
            || separator.get(two_pt_sep::SELECTORS..two_pt_sep::FIRST_TOKEN)? != [2, 0, 1, 0, 0, 0]
            || separator.get(two_pt_sep::ONE..two_pt_sep::FINAL_TOKENS)? != [1, 0]
            || [
                two_pt_sep::FIRST_TOKEN,
                two_pt_sep::FINAL_TOKENS,
                two_pt_sep::FINAL_TOKENS + 2,
            ]
            .into_iter()
            .any(|offset| separator.get(offset..offset + 2) == Some(&[0, 0]))
        {
            return None;
        }
        let tail = record.get(axis_point.end..)?;
        if tail.len() != two_pt_tail::LEN
            || tail.get(two_pt_tail::SEPARATOR) != Some(&0)
            || tail.get(two_pt_tail::ZERO_BEFORE_ORIGIN..two_pt_tail::ORIGIN)? != [0; 3]
            || tail.get(two_pt_tail::TERMINAL_TOKEN..two_pt_tail::LEN) == Some(&[0, 0])
        {
            return None;
        }
        let cached_origin = Point3::new(
            finite_f64(tail, two_pt_tail::ORIGIN)? * 1000.0,
            finite_f64(tail, two_pt_tail::ORIGIN + 8)? * 1000.0,
            finite_f64(tail, two_pt_tail::ORIGIN + 16)? * 1000.0,
        );
        if reference_point_key(&cached_origin) != reference_point_key(&origin.point)
            || (finite_f64(tail, two_pt_tail::ORIGIN_YZ)? * 1000.0 + 0.0).to_bits()
                != (origin.point.y + 0.0).to_bits()
            || (finite_f64(tail, two_pt_tail::ORIGIN_YZ + 8)? * 1000.0 + 0.0).to_bits()
                != (origin.point.z + 0.0).to_bits()
        {
            return None;
        }
        let x_axis = Vector3::new(
            finite_f64(tail, two_pt_tail::X_DIRECTION)?,
            finite_f64(tail, two_pt_tail::X_DIRECTION + 8)?,
            finite_f64(tail, two_pt_tail::X_DIRECTION + 16)?,
        );
        let repeated = Vector3::new(
            finite_f64(tail, two_pt_tail::REPEATED_X_DIRECTION)?,
            finite_f64(tail, two_pt_tail::REPEATED_X_DIRECTION + 8)?,
            finite_f64(tail, two_pt_tail::REPEATED_X_DIRECTION + 16)?,
        );
        if (x_axis.dot(x_axis) - 1.0).abs()
            > EPS_REFERENCE_GEOMETRY_COORDINATE_SYSTEM_TWO_POINT_FRAME_E9
            || x_axis != repeated
        {
            return None;
        }
        let y_source = Vector3::new(
            axis_point.point.x - origin.point.x,
            axis_point.point.y - origin.point.y,
            axis_point.point.z - origin.point.z,
        );
        let projection = x_axis.dot(y_source);
        let y_axis = Vector3::new(
            y_source.x - projection * x_axis.x,
            y_source.y - projection * x_axis.y,
            y_source.z - projection * x_axis.z,
        )
        .unit()?;
        Some((origin.point, x_axis, y_axis, x_axis.cross(y_axis).unit()?))
    })())
}

fn coordinate_system_component_path_origin(
    ctx: &DecodeContext<'_>,
    record: &[u8],
    prefix: usize,
) -> Result<Option<(Point3, u32, usize)>, CodecError> {
    const HANDLES: &[u8] = &[0xc7, 0xcf, 0xff, 0xff, 0xc7, 0xcf, 0xff, 0xff];
    const NULL_SLOT: &[u8] = &[0xff, 0xff, 0xff, 0xff, 0, 0, 0, 0];
    let marker = (|| {
        let marker = prefix.checked_add(cs_path_pre::COMPONENT_MARKER)?;
        if record.get(prefix + cs_path_pre::SENTINEL..prefix + cs_path_pre::PATH_ENTRY_COUNT)?
            != [0xff; 7]
        {
            return None;
        }
        Some(marker)
    })();
    let Some(marker) = marker else {
        return Ok(None);
    };
    let Some(path_end) = compact_component_path_end_at(ctx, record, marker)? else {
        return Ok(None);
    };
    Ok((|| {
        let trailer = if record.get(path_end..path_end + NULL_SLOT.len()) == Some(NULL_SLOT) {
            path_end + NULL_SLOT.len()
        } else {
            path_end
        };
        if record.get(trailer..trailer + cs_path_suf::ONE)? != [0; 14]
            || record.get(trailer + cs_path_suf::ONE..trailer + cs_path_suf::ZERO_BEFORE_OBJECT)?
                != 1u32.to_le_bytes()
            || record
                .get(trailer + cs_path_suf::ZERO_BEFORE_OBJECT..trailer + cs_path_suf::OBJECT_ID)?
                != [0; 4]
            || record
                .get(trailer + cs_path_suf::ZERO_BEFORE_HANDLES..trailer + cs_path_suf::HANDLES)?
                != [0; 12]
        {
            return None;
        }
        let object = View::u32_le_at(record, trailer + cs_path_suf::OBJECT_ID)?;
        let handles = trailer.checked_add(cs_path_suf::HANDLES)?;
        if matches!(object, 0 | u32::MAX)
            || record.get(handles..handles + 8)? != HANDLES
            || record.get(
                trailer + cs_path_suf::ZERO_BEFORE_GENERATION..trailer + cs_path_suf::GENERATION,
            )? != [0; 4]
            || record
                .get(trailer + cs_path_suf::ZERO_BEFORE_ORIGIN..trailer + cs_path_suf::ORIGIN)?
                != [0; 8]
        {
            return None;
        }
        let generation = View::u32_le_at(record, trailer + cs_path_suf::GENERATION)?;
        if matches!(generation, 0 | u32::MAX) {
            return None;
        }
        Some((
            Point3::new(
                finite_f64(record, trailer + cs_path_suf::ORIGIN)? * 1000.0,
                finite_f64(record, trailer + cs_path_suf::ORIGIN + 8)? * 1000.0,
                finite_f64(record, trailer + cs_path_suf::ORIGIN + 16)? * 1000.0,
            ),
            generation,
            trailer.checked_add(cs_path_suf::LEN)?,
        ))
    })())
}

fn coordinate_system_line_axes<'a>(
    ctx: &'a DecodeContext<'_>,
    record: &'a [u8],
    generation: u32,
    origin_end: usize,
) -> Result<impl Iterator<Item = Result<(usize, Point3, Vector3), CodecError>> + 'a, CodecError> {
    const PREFIX: &[u8] = &[0xc7, 0xcf, 0xff, 0xff, 0xc7, 0xcf, 0xff, 0xff];
    let Some(window_width) = std::num::NonZeroUsize::new(PREFIX.len()) else {
        return Err(ctx.refuse_codec_limit("scan SLDPRT coordinate system line axes", 1, 0));
    };
    let mut windows = record.windows(window_width.get()).enumerate();
    let mut done = false;
    Ok(std::iter::from_fn(move || {
        if done { return None; }
        let found = ctx.find_map(windows.by_ref(), |(prefix, bytes)| {
            if prefix < origin_end || bytes != PREFIX { return Ok(None); }
            Ok((|| {
            if record
                .get(prefix + line_axis::ZERO_BEFORE_GENERATION..prefix + line_axis::GENERATION)
                != Some(&[0; 4])
                || record
                    .get(prefix + line_axis::GENERATION..prefix + line_axis::ZERO_BEFORE_SCALAR)
                    != Some(&generation.to_le_bytes())
                || record
                    .get(prefix + line_axis::ZERO_BEFORE_SCALAR..prefix + line_axis::CARRIER_SCALAR)
                    != Some(&[0; 16])
                || record.get(prefix + line_axis::SEPARATOR) != Some(&0)
            {
                return None;
            }
            let scalar = finite_f64(record, prefix + line_axis::CARRIER_SCALAR)?;
            let point = Point3::new(
                finite_f64(record, prefix + line_axis::LINE_POINT)? * 1000.0,
                finite_f64(record, prefix + line_axis::LINE_POINT + 8)? * 1000.0,
                finite_f64(record, prefix + line_axis::LINE_POINT + 16)? * 1000.0,
            );
            let direction = Vector3::new(
                finite_f64(record, prefix + line_axis::DIRECTION)?,
                finite_f64(record, prefix + line_axis::DIRECTION + 8)?,
                finite_f64(record, prefix + line_axis::DIRECTION + 16)?,
            );
            let repeated = Vector3::new(
                finite_f64(record, prefix + line_axis::REPEATED_DIRECTION)?,
                finite_f64(record, prefix + line_axis::REPEATED_DIRECTION + 8)?,
                finite_f64(record, prefix + line_axis::REPEATED_DIRECTION + 16)?,
            );
            let repeated_matches = (direction.x - repeated.x).abs()
                <= EPS_REFERENCE_GEOMETRY_COORDINATE_SYSTEM_LINE_AXES_E12
                && (direction.y - repeated.y).abs()
                    <= EPS_REFERENCE_GEOMETRY_COORDINATE_SYSTEM_LINE_AXES_E12
                && (direction.z - repeated.z).abs()
                    <= EPS_REFERENCE_GEOMETRY_COORDINATE_SYSTEM_LINE_AXES_E12;
            (scalar > 0.0
                && (direction.norm() - 1.0).abs()
                    <= EPS_REFERENCE_GEOMETRY_COORDINATE_SYSTEM_LINE_AXES_E9
                && repeated_matches)
                .then_some((prefix, point, direction))
            })())
        }, "scan SLDPRT coordinate system line axes");
        if !matches!(found, Ok(Some(_))) { done = true; }
        found.transpose()
    }))
}

fn finite_f64(bytes: &[u8], offset: usize) -> Option<f64> {
    let value = View::f64_le_at(bytes, offset)?;
    value.is_finite().then_some(value)
}

fn coordinate_system_frame_key(
    (origin, x_axis, y_axis, z_axis): &(Point3, Vector3, Vector3, Vector3),
) -> [u64; 12] {
    let bits = |value: f64| (value + 0.0).to_bits();
    [
        bits(origin.x),
        bits(origin.y),
        bits(origin.z),
        bits(x_axis.x),
        bits(x_axis.y),
        bits(x_axis.z),
        bits(y_axis.x),
        bits(y_axis.y),
        bits(y_axis.z),
        bits(z_axis.x),
        bits(z_axis.y),
        bits(z_axis.z),
    ]
}

struct ReferencePlaneFrameCandidate {
    source: String,
    history_index: usize,
    feature_index: usize,
    frame: (Point3, Vector3, Vector3),
}

fn select_reference_plane_frame_source<'a>(
    ctx: &DecodeContext<'_>,
    candidates: &'a [ReferencePlaneFrameCandidate],
    mut include: impl FnMut(&ReferencePlaneFrameCandidate) -> bool,
    operation: &'static str,
) -> Result<Option<&'a str>, CodecError> {
    let mut selected = None;
    let mut values = candidates.iter();
    while let Some(candidate) = ctx.next_charged(&mut values, operation)? {
        if !include(candidate) {
            continue;
        }
        if let Some(first) = selected {
            if !ctx.equal(first, candidate.source.as_str(), operation)? {
                return Ok(None);
            }
        } else {
            selected = Some(candidate.source.as_str());
        }
    }
    Ok(selected)
}

fn offset_plane_reference_frame_matches(
    reference: (Point3, Vector3, Vector3),
    offset: (Point3, Vector3, Vector3),
    distance: f64,
) -> bool {
    let (reference_origin, reference_normal, _) = reference;
    let (offset_origin, offset_normal, _) = offset;
    let delta = Vector3::new(
        offset_origin.x - reference_origin.x,
        offset_origin.y - reference_origin.y,
        offset_origin.z - reference_origin.z,
    );
    let axial = delta.dot(reference_normal);
    let tangential = Vector3::new(
        delta.x - axial * reference_normal.x,
        delta.y - axial * reference_normal.y,
        delta.z - axial * reference_normal.z,
    );
    let tangential_length = tangential.norm();
    (reference_normal.dot(offset_normal).abs() - 1.0).abs()
        <= EPS_REFERENCE_GEOMETRY_OFFSET_PLANE_REFERENCE_FRAME_MATCHES_E9
        && tangential_length <= EPS_REFERENCE_GEOMETRY_OFFSET_PLANE_REFERENCE_FRAME_MATCHES_E8
        && (axial.abs() - distance.abs()).abs()
            <= EPS_REFERENCE_GEOMETRY_OFFSET_PLANE_REFERENCE_FRAME_MATCHES_E8
}


/// Resolve sketch-block definition ownership and placement from typed object records.
pub(crate) fn enrich_history_sketch_block_references(
    ctx: &DecodeContext<'_>,
    histories: &mut [crate::records::FeatureHistory],
    lanes: &[FeatureInputLane],
) -> Result<(), CodecError> {
    for history_index in
        ctx.admit_iter(&(0..histories.len()), "scan SLDPRT sketch-block histories")?
    {
        let mut temporary_storage = ctx.reserve_scoped(0, "SLDPRT sketch block history workspace")?;
        let history = &mut histories[history_index];
        let mut by_source = HashMap::<u32, Option<(usize, NativeClassKind)>>::new();
        for (feature_index, feature) in ctx
            .admit_iter(&history.features, "scan SLDPRT sketch block features")?
            .enumerate()
        {
            let Some(source) = feature.source_value() else {
                continue;
            };
            let identity = (
                feature_index,
                native_object_class(feature.input_class.as_deref().unwrap_or_default()),
            );
            temporary_storage.with_storage(|| {
                ctx.entry_hash_map(&mut by_source, source, "index SLDPRT sketch block sources")?
                    .and_modify(|entry| *entry = None).or_insert(Some(identity));
                Ok::<_, CodecError>(())
            })?;
        }
        let mut candidates = BTreeMap::<usize, Vec<u32>>::new();
        let mut placement_candidates = BTreeMap::<usize, Vec<Point3>>::new();
        let Some(pair_width) = std::num::NonZeroUsize::new(2) else {
            return Err(ctx.refuse_codec_limit("scan SLDPRT sketch block name pairs", 1, 0));
        };
        for lane in ctx.admit_iter(lanes, "scan SLDPRT sketch block lanes")? {
            let mut lane_storage = ctx.reserve_scoped(0, "SLDPRT sketch block lane workspace")?;
            let mut names = Vec::new();
            for name in ctx.admit_iter(&lane.names, "collect SLDPRT sketch block names")? {
                lane_storage.with_storage(|| ctx.push_vec(&mut names, name, "collect SLDPRT sketch block names"))?;
            }
            ctx.stable_sort_by(
                &mut names,
                |value| &value.offset,
                Ord::cmp,
                "sort SLDPRT sketch block names",
            )?;
            let mut instance_names = Vec::new();
            for &name in ctx.admit_iter(&names, "scan SLDPRT sketch block names")? {
                let Some(source) = name.object_id.and_then(ObjectId::value) else {
                    continue;
                };
                let Some((feature_index, kind)) = ctx.get_hash_map(&by_source, &source, "find SLDPRT sketch block source")?.and_then(|entry| *entry)
                else {
                    continue;
                };
                if kind == NativeClassKind::SketchBlockInstance {
                    lane_storage.with_storage(|| ctx.push_vec(&mut instance_names, (name, feature_index), "collect SLDPRT sketch block instances"))?;
                }
            }
            let mut definitions_by_local_id = HashMap::<u16, Option<u32>>::new();
            for (position, (name, _)) in ctx
                .admit_iter(&instance_names, "scan SLDPRT sketch block instances")?
                .enumerate()
            {
                let next_index = ctx.partition_point(&names, |next| Ok(next.offset <= name.offset),
                    "find SLDPRT sketch block definition name")?;
                let Some(next) = names.get(next_index) else { continue; };
                let Some(definition_source) = next.object_id.and_then(ObjectId::value) else { continue; };
                if !ctx.get_hash_map(&by_source, &definition_source, "find SLDPRT sketch block definition source")?
                    .and_then(|entry| *entry).is_some_and(|(_, kind)| kind == NativeClassKind::SketchBlockDefinition)
                { continue; }
                let end = instance_names
                    .get(position + 1)
                    .and_then(|(next, _)| usize::try_from(next.offset).ok())
                    .unwrap_or(lane.native_payload.len());
                let Some(start) = usize::try_from(name.offset).ok() else {
                    continue;
                };
                let Some(local_id) =
                    sketch_block_record_local_id(ctx, &lane.native_payload, start, end)?
                else {
                    continue;
                };
                lane_storage.with_storage(|| {
                    ctx.entry_hash_map(&mut definitions_by_local_id, local_id, "index SLDPRT sketch block local IDs")?
                        .and_modify(|entry| *entry = None).or_insert(Some(definition_source));
                    Ok::<_, CodecError>(())
                })?;
            }
            for (position, (name, instance_index)) in ctx
                .admit_iter(&instance_names, "scan SLDPRT sketch block placements")?
                .enumerate()
            {
                let end = instance_names
                    .get(position + 1)
                    .and_then(|(next, _)| usize::try_from(next.offset).ok())
                    .unwrap_or(lane.native_payload.len());
                let Some(start) = usize::try_from(name.offset).ok() else {
                    continue;
                };
                let origin = sketch_block_record_origin(ctx, &lane.native_payload, start, end)?;
                let origin = origin.or_else(|| {
                    sketch_block_identity_normalization_origin(&lane.native_payload, start, end)
                });
                if let Some(origin) = origin {
                    temporary_storage.with_storage(|| {
                        ctx.push_btree_group(&mut placement_candidates, *instance_index, origin, "collect SLDPRT sketch block placements", "collect SLDPRT sketch block placements")
                    })?;
                }
            }
            for pair in ctx
                .admit_iter(&names, "match SLDPRT sketch block names")?
                .windows(pair_width)
            {
                let (Some(instance_source), Some(definition_source)) = (
                    pair[0].object_id.and_then(ObjectId::value),
                    pair[1].object_id.and_then(ObjectId::value),
                ) else {
                    continue;
                };
                let Some((instance_index, NativeClassKind::SketchBlockInstance)) =
                    ctx.get_hash_map(&by_source, &instance_source, "find SLDPRT sketch block instance source")?.and_then(|entry| *entry)
                else {
                    continue;
                };
                let Some((_, NativeClassKind::SketchBlockDefinition)) =
                    ctx.get_hash_map(&by_source, &definition_source, "find SLDPRT sketch block definition source")?.and_then(|entry| *entry)
                else {
                    continue;
                };
                temporary_storage.with_storage(|| {
                    ctx.push_btree_group(&mut candidates, instance_index, definition_source, "collect SLDPRT sketch block definitions", "collect SLDPRT sketch block definitions")
                })?;
            }
            for (position, (name, instance_index)) in ctx
                .admit_iter(
                    &instance_names,
                    "scan SLDPRT compact sketch block instances",
                )?
                .enumerate()
            {
                let end = instance_names
                    .get(position + 1)
                    .and_then(|(next, _)| usize::try_from(next.offset).ok())
                    .unwrap_or(lane.native_payload.len());
                let Some(local_id) =
                    sketch_block_compact_local_id(ctx, &lane.native_payload, name, end)?
                else {
                    continue;
                };
                let Some(definition_source) = ctx.get_hash_map(&definitions_by_local_id, &local_id, "find SLDPRT sketch block local ID")?
                    .and_then(|entry| *entry)
                else {
                    continue;
                };
                temporary_storage.with_storage(|| {
                    ctx.push_btree_group(&mut candidates, *instance_index, definition_source, "collect SLDPRT sketch block definitions", "collect SLDPRT sketch block definitions")
                })?;
            }
        }
        for (feature_index, mut sources) in ctx.admit_iter(candidates, "apply SLDPRT sketch block definitions")? {
            ctx.sort_unstable_by(
                &mut sources,
                |value| value,
                Ord::cmp,
                "sort SLDPRT sketch block definitions",
            )?;
            ctx.dedup_vec(&mut sources, "deduplicate SLDPRT sketch block definitions")?;
            let [source] = sources.as_slice() else {
                continue;
            };
            let value = ctx.format_retained(
                format_args!("{source}"),
                "retain SLDPRT sketch block definition",
            )?;
            let properties = &mut history.features[feature_index].properties;
            ctx.insert_btree_map(
                properties,
                cadmpeg_core::nonblank_literal!("BlockDefinition"),
                value,
                "insert SLDPRT sketch block definition",
            )?;
        }
        for (feature_index, mut origins) in ctx.admit_iter(placement_candidates, "apply SLDPRT sketch block placements")? {
            ctx.stable_sort_by(
                &mut origins,
                |value| value,
                |left, right| {
                    [left.x.to_bits(), left.y.to_bits(), left.z.to_bits()].cmp(&[
                        right.x.to_bits(),
                        right.y.to_bits(),
                        right.z.to_bits(),
                    ])
                },
                "sort SLDPRT sketch block origins",
            )?;
            ctx.dedup_vec(&mut origins, "deduplicate SLDPRT sketch block origins")?;
            let [origin] = origins.as_slice() else {
                continue;
            };
            let value = ctx.format_retained(
                format_args!("{}mm,{}mm,{}mm", origin.x, origin.y, origin.z),
                "retain SLDPRT sketch block origin",
            )?;
            let properties = &mut history.features[feature_index].properties;
            ctx.insert_btree_map(
                properties,
                cadmpeg_core::nonblank_literal!("BlockOrigin"),
                value,
                "insert SLDPRT sketch block origin",
            )?;
        }
    }
    Ok(())
}

fn sketch_block_record_identity(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
    start: usize,
    end: usize,
) -> Result<Option<(u16, usize)>, CodecError> {
    let Some(bytes) = payload.get(start..end) else {
        return Ok(None);
    };
    let Some(window_width) = std::num::NonZeroUsize::new(44) else {
        return Err(ctx.refuse_codec_limit("scan SLDPRT sketch block identity", 1, 0));
    };
    ctx.find_map(bytes.windows(window_width.get()).enumerate().rev(), |(relative, window)| {
        let identity = (|| {
            let local_id = View::u16_le_at(window, 18)?;
            (window.get(..4) == Some(&[0xff; 4])
                && window.get(12..18) == Some(&[0x02, 0, 0, 0, 0, 0]) && local_id != 0
                && window.get(40..44) == Some(&[0, 0, 1, 0]))
                .then_some((local_id, start + relative))
        })();
        Ok(identity)
    }, "scan SLDPRT sketch block identity")
}

fn sketch_block_record_local_id(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
    start: usize,
    end: usize,
) -> Result<Option<u16>, CodecError> {
    Ok(sketch_block_record_identity(ctx, payload, start, end)?.map(|(local_id, _)| local_id))
}

fn sketch_block_record_origin(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
    start: usize,
    end: usize,
) -> Result<Option<Point3>, CodecError> {
    const ABSOLUTE_POINT_CLASS: &[u8] = b"moAbsolutePoint_c";
    const NATIVE_TO_IR: f64 = 1000.0;

    let Some((_, identity)) = sketch_block_record_identity(ctx, payload, start, end)? else {
        return Ok(None);
    };
    let Some(body) = identity.checked_add(44) else {
        return Err(ctx.refuse_codec_limit(
            "address SLDPRT sketch block point",
            u64::MAX - 1,
            u64::MAX,
        ));
    };
    Ok((|| {
        let class_len = u16::try_from(ABSOLUTE_POINT_CLASS.len())
            .ok()?
            .to_le_bytes();
        if payload.get(body..body + CLASS_MARKER.len()) == Some(CLASS_MARKER)
            && payload.get(body + 4..body + 6) == Some(&class_len)
            && payload.get(body + 6..body + 6 + ABSOLUTE_POINT_CLASS.len())
                == Some(ABSOLUTE_POINT_CLASS)
        {
            return Some(Point3::new(0.0, 0.0, 0.0));
        }
        let point_token = View::u16_le_at(payload, body)?;
        if !is_class_token(point_token) {
            return None;
        }
        let scalar = |relative: usize| {
            let value = View::f64_le_at(payload, body + 2 + relative)? * NATIVE_TO_IR;
            (value.is_finite() && value.abs() <= 1.0e9).then_some(value)
        };
        Some(Point3::new(scalar(0)?, scalar(8)?, scalar(16)?))
    })())
}

fn sketch_block_identity_normalization_origin(
    payload: &[u8],
    start: usize,
    end: usize,
) -> Option<Point3> {
    const CLASS: &[u8] = b"sgBlock";
    const NATIVE_TO_IR: f64 = 1000.0;

    let bytes = payload.get(start..end)?;
    let record_len = CLASS_MARKER.len() + 2 + CLASS.len();
    let class_len = u16::try_from(CLASS.len()).ok()?.to_le_bytes();
    let mut records = bytes
        .windows(record_len)
        .enumerate()
        .filter_map(|(relative, record)| {
            (record.get(..CLASS_MARKER.len()) == Some(CLASS_MARKER)
                && record.get(CLASS_MARKER.len()..CLASS_MARKER.len() + 2) == Some(&class_len)
                && record.get(CLASS_MARKER.len() + 2..) == Some(CLASS))
            .then_some(start + relative + record_len)
        });
    let body = records.next()?;
    if records.next().is_some() {
        return None;
    }
    let scalar = |relative: usize| {
        let value = View::f64_le_at(payload, body + relative)?;
        value.is_finite().then_some(value)
    };
    let basis = [
        scalar(72)?,
        scalar(80)?,
        scalar(88)?,
        scalar(96)?,
        scalar(104)?,
        scalar(112)?,
        scalar(120)?,
        scalar(128)?,
        scalar(136)?,
    ];
    if basis != [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0]
        || payload.get(body + 144..body + 152) != Some(&1_u64.to_le_bytes())
        || scalar(176)? != 1.0
    {
        return None;
    }
    let translation = [scalar(152)?, scalar(160)?, scalar(168)?];
    (translation.iter().all(|value| value.abs() <= 1.0e6)).then(|| {
        Point3::new(
            -translation[0] * NATIVE_TO_IR,
            -translation[1] * NATIVE_TO_IR,
            -translation[2] * NATIVE_TO_IR,
        )
    })
}

fn sketch_block_compact_local_id(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
    name: &FeatureInputName,
    record_end: usize,
) -> Result<Option<u16>, CodecError> {
    let Ok(name_start) = usize::try_from(name.offset) else {
        return Ok(None);
    };
    let unit_count = ctx
        .admit_iter(name.value.as_str(), "scan SLDPRT sketch block name")?
        .encode_utf16()
        .count();
    let Some(name_bytes) = unit_count.checked_mul(2) else {
        return Err(ctx.refuse_codec_limit(
            "size SLDPRT sketch block name",
            u64::MAX - 1,
            u64::MAX,
        ));
    };
    let Some(name_end) = name_start
        .checked_add(NAME_MARKER.len() + 1)
        .and_then(|start| start.checked_add(name_bytes))
    else {
        return Err(ctx.refuse_codec_limit(
            "size SLDPRT sketch block name",
            u64::MAX - 1,
            u64::MAX,
        ));
    };
    let Some(header_start) = name_end.checked_add(28) else {
        return Err(ctx.refuse_codec_limit(
            "address SLDPRT sketch block header",
            u64::MAX - 1,
            u64::MAX,
        ));
    };
    let Some(header) = View::u16_le_at(payload, header_start) else {
        return Ok(None);
    };
    let local_id = sketch_block_record_local_id(ctx, payload, name_start, record_end)?;
    Ok((local_id == Some(header)).then_some(header))
}

fn insert_reference_axis_property(
    ctx: &DecodeContext<'_>,
    feature: &mut crate::records::Feature,
    key: NonBlankString,
    value: std::fmt::Arguments<'_>,
) -> Result<(), CodecError> {
    let value = ctx.format_retained(value, "format SLDPRT reference axis property")?;
    ctx.insert_btree_map(
        &mut feature.properties,
        key,
        value,
        "insert SLDPRT reference axis property",
    )?;
    Ok(())
}

/// Add the two serialized construction-plane operands to plane-intersection axes.
pub(crate) fn enrich_history_reference_axes(
    ctx: &DecodeContext<'_>,
    histories: &mut [crate::records::FeatureHistory],
    lanes: &[FeatureInputLane],
) -> Result<(), CodecError> {
    let mut temporary_storage =
        ctx.reserve_scoped(0, "SLDPRT reference_geometry temporary storage")?;

    let mut known_sources = HashSet::new();
    for history in ctx.admit_iter(&*histories, "index SLDPRT reference axis sources")? {
        for feature in ctx.admit_iter(&history.features, "index SLDPRT reference axis sources")? {
            if let Some(source) = feature.source_value() {
                temporary_storage.with_storage(|| {
                    ctx.insert_hash_set(
                        &mut known_sources,
                        source,
                        "index SLDPRT reference axis sources",
                    )
                })?;
            }
        }
    }
    for lane in ctx.admit_iter(lanes, "scan SLDPRT reference axis lanes")? {
        let mut lane_storage = ctx.reserve_scoped(0, "SLDPRT reference lane workspace")?;
        let object_names = ObjectNames::new(ctx, lane)?;
        let mut axis_data_classes = Vec::new();
        for class in ctx.admit_iter(&lane.classes, "scan SLDPRT reference axis classes")? {
            if matches!(class.name.as_str(), "moPlaneInterAxisData_c" | "moSurfaceAxisData_c" | "moTwoPtsAxisData_c") {
                lane_storage.with_storage(|| ctx.push_vec(&mut axis_data_classes, class, "collect SLDPRT reference axis data classes"))?;
            }
        }
        ctx.stable_sort_by(&mut axis_data_classes, |class| &class.offset, Ord::cmp, "sort SLDPRT reference axis classes")?;
        let mut starts = Vec::new();
        for (history_index, history) in ctx
            .admit_iter(&*histories, "collect SLDPRT reference axis starts")?
            .enumerate()
        {
            for (feature_index, feature) in ctx
                .admit_iter(&history.features, "collect SLDPRT reference axis starts")?
                .enumerate()
            {
                if let Some(name) = object_names.of(ctx, feature)? {
                    lane_storage.with_storage(|| ctx.push_vec(&mut starts, (name.offset, history_index, feature_index), "collect SLDPRT reference axis starts"))?;
                }
            }
        }
        ctx.stable_sort_by(
            &mut starts,
            |value| &value.0,
            Ord::cmp,
            "sort SLDPRT feature starts",
        )?;
        for (index, &(start, history_index, feature_index)) in ctx
            .admit_iter(&starts, "scan SLDPRT reference axis starts")?
            .enumerate()
        {
            let feature = &histories[history_index].features[feature_index];
            if native_object_class(feature.input_class.as_deref().unwrap_or_default())
                != NativeClassKind::ReferenceAxis
                || ctx.contains_key_btree_map(&feature.properties, "Planes", "find SLDPRT reference geometry property")?
            {
                continue;
            }
            let Some(end) = starts
                .get(index + 1)
                .map_or(Some(lane.native_payload.len()), |next| {
                    index_from_u64(next.0)
                })
            else {
                continue;
            };
            let Ok(start) = usize::try_from(start) else {
                continue;
            };
            let Some(bytes) = lane.native_payload.get(start..end) else {
                continue;
            };
            let first_class = ctx.partition_point(&axis_data_classes, |class| Ok(class.offset < u64_from_index(start)), "find SLDPRT reference axis classes")?;
            let last_class = ctx.partition_point(&axis_data_classes, |class| Ok(class.offset < u64_from_index(end)), "find SLDPRT reference axis classes")?;
            let axis_data_classes = &axis_data_classes[first_class..last_class];
            let mut frame_storage = ctx.reserve_scoped(0, "SLDPRT anchored reference frame workspace")?;
            let mut anchored_frames = Vec::new();
            for class in ctx.admit_iter(
                axis_data_classes,
                "scan SLDPRT anchored reference axis classes",
            )? {
                let Some(body) = usize::try_from(class.offset)
                    .ok()
                    .and_then(|offset| offset.checked_add(6 + class.name.len()))
                else {
                    continue;
                };
                let Some(end) = body.checked_add(88) else {
                    continue;
                };
                let Some(payload) = lane.native_payload.get(body..end) else {
                    continue;
                };
                if let Some(frame) = explicit_reference_axis_frame(ctx, payload)? {
                    frame_storage.with_storage(|| ctx.push_vec(&mut anchored_frames, frame, "collect SLDPRT anchored reference axis frames"))?;
                }
            }
            ctx.stable_sort_by_key(
                &mut anchored_frames,
                reference_axis_frame_key,
                Ord::cmp,
                "sort SLDPRT reference frames",
            )?;
            ctx.dedup_by_key(
                &mut anchored_frames,
                |frame| Ok(reference_axis_frame_key(frame)),
                "deduplicate SLDPRT reference axis frames",
            )?;
            let explicit_frame = if axis_data_classes.is_empty() {
                explicit_reference_axis_frame(ctx, bytes)?
            } else {
                let [frame] = anchored_frames.as_slice() else {
                    continue;
                };
                Some(*frame)
            };
            if let Some((origin, direction)) = explicit_frame {
                let feature = &mut histories[history_index].features[feature_index];
                insert_reference_axis_property(
                    ctx,
                    feature,
                    cadmpeg_core::nonblank_literal!("Origin"),
                    format_args!("{}mm,{}mm,{}mm", origin.x, origin.y, origin.z),
                )?;
                insert_reference_axis_property(
                    ctx,
                    feature,
                    cadmpeg_core::nonblank_literal!("Direction"),
                    format_args!("{},{},{}", direction.x, direction.y, direction.z),
                )?;
                continue;
            }
            let Some([first, second]) =
                plane_intersection_axis_sources(ctx, bytes, &known_sources)?
            else {
                continue;
            };
            insert_reference_axis_property(
                ctx,
                &mut histories[history_index].features[feature_index],
                cadmpeg_core::nonblank_literal!("Planes"),
                format_args!("{first},{second}"),
            )?;
        }
    }

    for history_index in ctx.admit_iter(
        &(0..histories.len()),
        "apply SLDPRT legacy reference-axis histories",
    )? {
        let history = &mut histories[history_index];
        let triads = temporary_storage.with_storage(|| legacy_reference_axis_triads(ctx, &history.features))?;
        for ReferenceAxisTriad(axes, pairs) in
            ctx.admit_iter(&triads, "apply SLDPRT legacy reference axis triads")?
        {
            for (&axis_index, &planes) in axes.iter().zip(pairs) {
                let axis = &mut history.features[axis_index];
                if !ctx.contains_key_btree_map(&axis.properties, "Planes", "find SLDPRT reference geometry property")? {
                    insert_reference_axis_property(
                        ctx,
                        axis,
                        cadmpeg_core::nonblank_literal!("Planes"),
                        format_args!("{},{}", planes[0], planes[1]),
                    )?;
                }
            }
        }
    }

    let projected = match crate::history::project::project_features(ctx, histories) {
        Ok(projected) => projected,
        Err(error @ CodecError::ResourceLimit(_)) => return Err(error),
        Err(_) => return Ok(()),
    };
    let plane_frames = sketch_plane_frames(ctx, &projected, histories)?;
    for history_index in ctx.admit_iter(
        &(0..histories.len()),
        "scan SLDPRT reference-axis histories",
    )? {
        let feature_count = histories[history_index].features.len();
        for feature_index in
            ctx.admit_iter(&(0..feature_count), "scan SLDPRT reference-axis features")?
        {
            let feature = &mut histories[history_index].features[feature_index];
            if native_object_class(feature.input_class.as_deref().unwrap_or_default())
                != NativeClassKind::ReferenceAxis
                || ctx.contains_key_btree_map(&feature.properties, "Origin", "find SLDPRT reference geometry property")?
                || ctx.contains_key_btree_map(&feature.properties, "Direction", "find SLDPRT reference geometry property")?
            {
                continue;
            }
            let Some(planes) = ctx.get_btree_map(&feature.properties, "Planes", "find SLDPRT reference axis planes")? else { continue; };
            let Some((first, second)) = ctx.split_once(planes.as_str(), ",", "split SLDPRT reference axis planes")? else { continue; };
            let (Ok(first), Ok(second)) = (
                ctx.parse_text::<u32>(first, "parse SLDPRT reference axis plane")?,
                ctx.parse_text::<u32>(second, "parse SLDPRT reference axis plane")?,
            ) else { continue; };
            let Some(frame) = ctx.get_hash_map(&plane_frames, &first, "find SLDPRT reference axis plane")?
                .zip(ctx.get_hash_map(&plane_frames, &second, "find SLDPRT reference axis plane")?)
                .and_then(|(first, second)| {
                    plane_intersection_axis_frame(first.as_tuple(), second.as_tuple())
                })
            else {
                continue;
            };
            insert_reference_axis_property(
                ctx,
                feature,
                cadmpeg_core::nonblank_literal!("Origin"),
                format_args!("{}mm,{}mm,{}mm", frame.0.x, frame.0.y, frame.0.z),
            )?;
            insert_reference_axis_property(
                ctx,
                feature,
                cadmpeg_core::nonblank_literal!("Direction"),
                format_args!("{},{},{}", frame.1.x, frame.1.y, frame.1.z),
            )?;
        }
    }

    for history_index in ctx.admit_iter(
        &(0..histories.len()),
        "complete SLDPRT reference-axis histories",
    )? {
        let history = &mut histories[history_index];
        let mut history_storage = ctx.reserve_scoped(0, "SLDPRT reference axis triad workspace")?;
        let mut completions = Vec::new();
        let triads = history_storage.with_storage(|| legacy_reference_axis_triads(ctx, &history.features))?;
        for ReferenceAxisTriad(indices, _) in
            ctx.admit_iter(&triads, "complete SLDPRT legacy reference axis triads")?
        {
            let mut frames = [None; 3];
            for (slot, &index) in indices.iter().enumerate() {
                let feature = &history.features[index];
                let origin = crate::history::literals::named_literal(ctx, &feature.properties, "Origin", "parse SLDPRT reference axis origin")?.and_then(crate::history::literals::parse_point3_mm);
                let Some(origin) = origin else { continue; };
                let direction = crate::history::literals::named_literal(ctx, &feature.properties, "Direction", "parse SLDPRT reference axis direction")?.and_then(crate::history::literals::parse_vector3);
                frames[slot] = direction.map(|direction| (origin.get(), direction));
            }
            let Some(completion) = complete_reference_axis_triad(ctx, frames)? else {
                continue;
            };
            let completion = (
                indices[completion.axis_index],
                (completion.origin, completion.direction),
            );
            history_storage.with_storage(|| ctx.push_vec(&mut completions, completion, "collect SLDPRT reference axis completions"))?;
        }
        for &(index, (origin, direction)) in
            ctx.admit_iter(&completions, "write SLDPRT reference axis completions")?
        {
            let feature = &mut history.features[index];
            insert_reference_axis_property(
                ctx,
                feature,
                cadmpeg_core::nonblank_literal!("Origin"),
                format_args!("{}mm,{}mm,{}mm", origin.x, origin.y, origin.z),
            )?;
            insert_reference_axis_property(
                ctx,
                feature,
                cadmpeg_core::nonblank_literal!("Direction"),
                format_args!("{},{},{}", direction.x, direction.y, direction.z),
            )?;
        }
    }
    Ok(())
}

#[derive(Debug, PartialEq)]
struct ReferenceAxisCompletion {
    axis_index: usize,
    origin: Point3,
    direction: Vector3,
}

fn complete_reference_axis_triad(
    ctx: &DecodeContext<'_>,
    frames: [Option<(Point3, Vector3)>; 3],
) -> Result<Option<ReferenceAxisCompletion>, CodecError> {
    const ANGULAR_TOLERANCE: f64 = 1e-9;
    const POSITION_TOLERANCE_MM: f64 = 1e-8;

    let mut missing = None;
    let mut missing_count = 0usize;
    let mut present = [None, None];
    let mut present_count = 0usize;
    for (index, frame) in ctx
        .admit_iter(&frames, "complete SLDPRT reference axis triad")?
        .enumerate()
    {
        if let Some(frame) = frame {
            if present_count < present.len() {
                present[present_count] = Some((index, frame));
                present_count += 1;
            }
        } else {
            missing = Some(index);
            missing_count += 1;
        }
    }
    if missing_count != 1 {
        return Ok(None);
    }
    let Some(missing) = missing else {
        return Ok(None);
    };
    let [Some((_, (first_origin, first_direction))), Some((_, (second_origin, second_direction)))] =
        &present
    else {
        return Ok(None);
    };
    let completed = (|| {
        let normalize = |direction: Vector3| {
            let length = direction.norm();
            (length.is_finite()
                && length > EPS_REFERENCE_GEOMETRY_COMPLETE_REFERENCE_AXIS_TRIAD_E12)
                .then(|| {
                    Vector3::new(
                        direction.x / length,
                        direction.y / length,
                        direction.z / length,
                    )
                })
        };
        let first_direction = normalize(*first_direction)?;
        let second_direction = normalize(*second_direction)?;
        if first_direction.dot(second_direction).abs() > ANGULAR_TOLERANCE {
            return None;
        }
        let displacement = Vector3::new(
            second_origin.x - first_origin.x,
            second_origin.y - first_origin.y,
            second_origin.z - first_origin.z,
        );
        let first_along = displacement.dot(first_direction);
        let first_point = Point3::new(
            first_origin.x + first_direction.x * first_along,
            first_origin.y + first_direction.y * first_along,
            first_origin.z + first_direction.z * first_along,
        );
        let second_along = displacement.dot(second_direction);
        let second_point = Point3::new(
            second_origin.x - second_direction.x * second_along,
            second_origin.y - second_direction.y * second_along,
            second_origin.z - second_direction.z * second_along,
        );
        let separation = Vector3::new(
            first_point.x - second_point.x,
            first_point.y - second_point.y,
            first_point.z - second_point.z,
        );
        let scale = [
            first_point.x,
            first_point.y,
            first_point.z,
            second_point.x,
            second_point.y,
            second_point.z,
        ]
        .into_iter()
        .map(f64::abs)
        .fold(1.0_f64, f64::max);
        if separation.norm() > POSITION_TOLERANCE_MM * scale {
            return None;
        }
        let origin = Point3::new(
            (first_point.x + second_point.x) * 0.5,
            (first_point.y + second_point.y) * 0.5,
            (first_point.z + second_point.z) * 0.5,
        );
        let directions = frames.map(|frame| frame.and_then(|(_, direction)| normalize(direction)));
        let direction = match missing {
            0 => directions[2]?.cross(directions[1]?),
            1 => directions[0]?.cross(directions[2]?),
            2 => directions[1]?.cross(directions[0]?),
            _ => return None,
        };
        Some(ReferenceAxisCompletion {
            axis_index: missing,
            origin,
            direction: normalize(direction)?,
        })
    })();
    Ok(completed)
}

fn explicit_reference_axis_frame(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
) -> Result<Option<(Point3, Vector3)>, CodecError> {
    const NATIVE_TO_IR: f64 = 1000.0;
    const UNIT_TOLERANCE: f64 = 1e-9;
    const ORIGIN_ZERO_TOLERANCE_MM: f64 = 1e-9;
    const DIRECTION_ZERO_TOLERANCE: f64 = 1e-12;

    let scalar = |bytes: &[u8], offset: usize| {
        let value = View::f64_le_at(bytes, offset)?;
        value.is_finite().then_some(value)
    };
    let candidate = |bytes: &[u8]| {
        let first = Vector3::new(scalar(bytes, 0)?, scalar(bytes, 8)?, scalar(bytes, 16)?);
        let second = Vector3::new(scalar(bytes, 24)?, scalar(bytes, 32)?, scalar(bytes, 40)?);
        let _first_parameter = scalar(bytes, 48)?;
        let _second_parameter = scalar(bytes, 56)?;
        let stored_direction =
            Vector3::new(scalar(bytes, 64)?, scalar(bytes, 72)?, scalar(bytes, 80)?);
        let delta = Vector3::new(second.x - first.x, second.y - first.y, second.z - first.z);
        let delta_length = (delta.x * delta.x + delta.y * delta.y + delta.z * delta.z).sqrt();
        let direction_length = (stored_direction.x * stored_direction.x
            + stored_direction.y * stored_direction.y
            + stored_direction.z * stored_direction.z)
            .sqrt();
        if delta_length <= EPS_REFERENCE_GEOMETRY_EXPLICIT_REFERENCE_AXIS_FRAME_E12
            || (direction_length - 1.0).abs() > UNIT_TOLERANCE
            || [first.x, first.y, first.z, second.x, second.y, second.z]
                .into_iter()
                .any(|value| value.abs() > 1.0e6)
        {
            return None;
        }
        let direction = Vector3::new(
            stored_direction.x / direction_length,
            stored_direction.y / direction_length,
            stored_direction.z / direction_length,
        );
        let aligned =
            (delta.x * direction.x + delta.y * direction.y + delta.z * direction.z) / delta_length;
        if aligned < 1.0 - UNIT_TOLERANCE {
            return None;
        }
        let projection = first.x * direction.x + first.y * direction.y + first.z * direction.z;
        let origin = Point3::new(
            (first.x - projection * direction.x) * NATIVE_TO_IR,
            (first.y - projection * direction.y) * NATIVE_TO_IR,
            (first.z - projection * direction.z) * NATIVE_TO_IR,
        );
        let canonical_zero =
            |value: f64, tolerance: f64| if value.abs() <= tolerance { 0.0 } else { value };
        Some((
            Point3::new(
                canonical_zero(origin.x, ORIGIN_ZERO_TOLERANCE_MM),
                canonical_zero(origin.y, ORIGIN_ZERO_TOLERANCE_MM),
                canonical_zero(origin.z, ORIGIN_ZERO_TOLERANCE_MM),
            ),
            Vector3::new(
                canonical_zero(direction.x, DIRECTION_ZERO_TOLERANCE),
                canonical_zero(direction.y, DIRECTION_ZERO_TOLERANCE),
                canonical_zero(direction.z, DIRECTION_ZERO_TOLERANCE),
            ),
        ))
    };
    let mut unique = None;
    let mut source_items = payload.windows(88).into_iter();
    while let Some(bytes) = ctx.next_charged(&mut source_items, "scan SLDPRT explicit reference axis windows")? {
        let Some(frame) = candidate(bytes) else {
            continue;
        };
        match unique {
            Some(existing)
                if reference_axis_frame_key(&existing) != reference_axis_frame_key(&frame) =>
            {
                return Ok(None);
            }
            None => unique = Some(frame),
            Some(_) => {}
        }
    }
    Ok(unique)
}

fn reference_axis_frame_key((origin, direction): &(Point3, Vector3)) -> [u64; 6] {
    [
        origin.x.to_bits(),
        origin.y.to_bits(),
        origin.z.to_bits(),
        direction.x.to_bits(),
        direction.y.to_bits(),
        direction.z.to_bits(),
    ]
}

#[derive(Debug, PartialEq, Eq)]
struct ReferenceAxisTriad([usize; 3], [[u32; 2]; 3]);

fn legacy_reference_axis_triads(
    ctx: &DecodeContext<'_>,
    features: &[crate::records::Feature],
) -> Result<Vec<ReferenceAxisTriad>, CodecError> {
    let mut temporary_storage =
        ctx.reserve_scoped(0, "SLDPRT reference_geometry temporary storage")?;

    let mut by_source = HashMap::<u32, Option<usize>>::new();
    for (index, feature) in ctx
        .admit_iter(features, "index SLDPRT reference axis triad features")?
        .enumerate()
    {
        let Some(source) = feature.source_value() else {
            continue;
        };
        if let Some(existing) = ctx.get_mut_hash_map(&mut by_source, &source, "index SLDPRT reference sources")? {
            *existing = None;
        } else {
            temporary_storage.with_storage(|| {
                ctx.insert_hash_map(
                    &mut by_source,
                    source,
                    Some(index),
                    "index SLDPRT reference axis triad sources",
                )
            })?;
        }
    }
    let mut triads = Vec::new();
    for first in ctx.admit_iter(features, "scan SLDPRT reference axis triad candidates")? {
        let Some(source) = first.source_value() else {
            continue;
        };
        let mut indices = [0usize; 6];
        let mut complete = true;
        for (offset, index) in indices.iter_mut().enumerate() {
            let Some(candidate_source) = u32::try_from(offset)
                .ok()
                .and_then(|offset| source.checked_add(offset))
            else {
                complete = false;
                break;
            };
            let Some(candidate_index) = ctx.get_hash_map(&by_source, &candidate_source, "find SLDPRT reference axis triad source")?.copied().flatten() else {
                complete = false;
                break;
            };
            *index = candidate_index;
        }
        if !complete
            || indices[..3].iter()
                .any(|index| {
                    native_object_class(features[*index].input_class.as_deref().unwrap_or_default())
                        != NativeClassKind::ReferencePlane
                })
            || indices[3..].iter()
                .any(|index| {
                    native_object_class(features[*index].input_class.as_deref().unwrap_or_default())
                        != NativeClassKind::ReferenceAxis
                })
        {
            continue;
        }
        let [Some(first_source), Some(second_source), Some(third_source)] = [
            features[indices[0]].source_value(),
            features[indices[1]].source_value(),
            features[indices[2]].source_value(),
        ] else {
            continue;
        };
        ctx.reserve_vec(&mut triads, 1, "collect SLDPRT reference axis triads")?;
        triads.push(ReferenceAxisTriad(
            [indices[3], indices[4], indices[5]],
            [
                [first_source, second_source],
                [first_source, third_source],
                [third_source, second_source],
            ],
        ));
    }
    Ok(triads)
}

fn plane_intersection_axis_frame(
    first: (Point3, Vector3, Vector3),
    second: (Point3, Vector3, Vector3),
) -> Option<(Point3, Vector3)> {
    let (first_origin, first_normal, _) = first;
    let (second_origin, second_normal, _) = second;
    let direction = first_normal.cross(second_normal);
    let squared_length = direction.dot(direction);
    if !squared_length.is_finite() || squared_length <= 1.0e-18 {
        return None;
    }
    let first_distance = first_normal.x * first_origin.x
        + first_normal.y * first_origin.y
        + first_normal.z * first_origin.z;
    let second_distance = second_normal.x * second_origin.x
        + second_normal.y * second_origin.y
        + second_normal.z * second_origin.z;
    let first_term = second_normal.cross(direction);
    let second_term = direction.cross(first_normal);
    let origin = Point3::new(
        (first_distance * first_term.x + second_distance * second_term.x) / squared_length,
        (first_distance * first_term.y + second_distance * second_term.y) / squared_length,
        (first_distance * first_term.z + second_distance * second_term.z) / squared_length,
    );
    let length = squared_length.sqrt();
    let direction = Vector3::new(
        direction.x / length,
        direction.y / length,
        direction.z / length,
    );
    [
        origin.x,
        origin.y,
        origin.z,
        direction.x,
        direction.y,
        direction.z,
    ]
    .into_iter()
    .all(f64::is_finite)
    .then_some((origin, direction))
}

fn plane_intersection_axis_sources(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
    known_sources: &HashSet<u32>,
) -> Result<Option<[u32; 2]>, CodecError> {
    const RECORD_LEN: usize = 46;
    const TERMINATOR: &[u8] = &[0xc7, 0xcf, 0xff, 0xff, 0xc7, 0xcf, 0xff, 0xff];
    let mut sources = [0; 2];
    let mut source_count = 0;
    let mut source_items = payload.windows(RECORD_LEN).into_iter();
    while let Some(bytes) = ctx.next_charged(&mut source_items, "scan SLDPRT plane intersection axis records")? {
        let source = (|| {
            let source = View::u32_le_at(bytes, 0)?;
            (bytes.get(8..14)?.iter().all(|byte| *byte == 0)
                && bytes.get(14..16) == Some(&[1, 0])
                && bytes.get(16..22)?.iter().all(|byte| *byte == 0)
                && bytes.get(22).is_some_and(|object| *object != 0xff)
                && bytes.get(23..30)?.iter().all(|byte| *byte == 0)
                && matches!(bytes.get(30), Some(0 | 3))
                && bytes.get(31..38)?.iter().all(|byte| *byte == 0)
                && bytes.get(38..46) == Some(TERMINATOR))
            .then_some(source)
        })();
        if let Some(source) = source {
            if !ctx.contains_hash_set(known_sources, &source, "find SLDPRT plane intersection axis source")? { continue; }
            if sources[..source_count].contains(&source) {
                continue;
            }
            let Some(slot) = sources.get_mut(source_count) else {
                return Ok(None);
            };
            *slot = source;
            source_count += 1;
        }
    }
    Ok((source_count == 2).then_some(sources))
}

fn compact_offset_plane_source(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
) -> Result<Option<u32>, CodecError> {
    const TRAILER: &[u8] = &[
        0x02, 0x00, 0x00, 0x00, 0x00, 0x05, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x2d, 0x80, 0x2b, 0x80,
    ];
    let Some(window_width) = std::num::NonZeroUsize::new(4 + TRAILER.len()) else {
        return Err(ctx.refuse_codec_limit("scan SLDPRT compact plane source", 1, 0));
    };
    let mut unique = None;
    let mut windows = payload.windows(window_width.get());
    while let Some(bytes) = ctx.next_charged(&mut windows, "scan SLDPRT compact plane source")? {
        let Some(source) = View::u32_le_at(bytes, 0) else {
            continue;
        };
        if source == 0 || bytes.get(4..) != Some(TRAILER) {
            continue;
        }
        match unique {
            Some(existing) if existing != source => return Ok(None),
            None => unique = Some(source),
            Some(_) => {}
        }
    }
    Ok(unique)
}

fn for_each_structured_offset_plane_source(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
    mut visit: impl FnMut(u32) -> Result<bool, CodecError>,
) -> Result<(), CodecError> {
    const RECORD_LEN: usize = 140;
    const TERMINATOR: &[u8] = &[0xc7, 0xcf, 0xff, 0xff, 0xc7, 0xcf, 0xff, 0xff];
    let Some(window_width) = std::num::NonZeroUsize::new(RECORD_LEN) else {
        return Err(ctx.refuse_codec_limit("scan SLDPRT structured plane sources", 1, 0));
    };
    let mut windows = payload.windows(window_width.get());
    while let Some(bytes) = ctx.next_charged(&mut windows, "scan SLDPRT structured plane sources")? {
        let source = (|| {
            let header = bytes.get(4..8)?;
            let identity = bytes.get(8..20)?;
            let link = bytes.get(28..32)?;
            let source = View::u32_le_at(bytes, 44)?;
            let address = bytes.get(116..120)?;
            (bytes.get(..4) == Some(&4u32.to_le_bytes())
                && header != [0; 4]
                && bytes.get(48..52) == Some(header)
                && identity != [0; 12]
                && bytes.get(32..44) == Some(identity)
                && bytes.get(52..64) == Some(identity)
                && bytes.get(76..88) == Some(identity)
                && bytes.get(20..28) == Some(&[0; 8])
                && link != [0; 4]
                && bytes.get(72..76) == Some(link)
                && bytes.get(64..68) == Some(&1u32.to_le_bytes())
                && bytes.get(68..72) == Some(&[0; 4])
                && bytes.get(88..92) == Some(&1u32.to_le_bytes())
                && bytes.get(92..108) == Some(&[0; 16])
                && bytes.get(108..112) == Some(&1u32.to_le_bytes())
                && bytes.get(112..116) == Some(&[0; 4])
                && address != [0; 4]
                && bytes.get(120..132) == Some(&[0; 12])
                && bytes.get(132..140) == Some(TERMINATOR))
            .then_some(source)
        })();
        if let Some(source) = source {
            if !visit(source)? {
                break;
            }
        }
    }
    Ok(())
}

fn for_each_classed_offset_plane_source(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
    mut visit: impl FnMut(u32) -> Result<bool, CodecError>,
) -> Result<(), CodecError> {
    const TRAILER: &[u8] = b"\xff\xff\x01\x00\x1b\x00moFromSktEnt3IntSurfIdRep_c\x00\x00";
    let Some(window_width) = std::num::NonZeroUsize::new(4 + TRAILER.len()) else {
        return Err(ctx.refuse_codec_limit("scan SLDPRT classed plane sources", 1, 0));
    };
    let mut windows = payload.windows(window_width.get());
    while let Some(bytes) = ctx.next_charged(&mut windows, "scan SLDPRT classed plane sources")? {
        let Some(source) = View::u32_le_at(bytes, 0) else {
            continue;
        };
        if bytes.get(4..) == Some(TRAILER) && !visit(source)? {
            break;
        }
    }
    Ok(())
}

fn offset_plane_reference_source(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
    known_sources: &HashSet<u32>,
    known_reference_plane_sources: &HashSet<u32>,
    self_source: Option<u32>,
) -> Result<Option<u32>, CodecError> {
    const RECORD_LEN: usize = 46;
    const TERMINATOR: &[u8] = &[0xc7, 0xcf, 0xff, 0xff, 0xc7, 0xcf, 0xff, 0xff];
    let Some(window_width) = std::num::NonZeroUsize::new(RECORD_LEN) else {
        return Err(ctx.refuse_codec_limit("scan SLDPRT typed offset plane sources", 1, 0));
    };
    let mut unique = None;
    let mut observe = |source: u32| -> Result<bool, CodecError> {
        if Some(source) == self_source || !ctx.contains_hash_set(known_sources, &source, "find SLDPRT known plane source")? {
            return Ok(true);
        }
        match unique {
            Some(existing) if existing != source => Ok(false),
            None => {
                unique = Some(source);
                Ok(true)
            }
            Some(_) => Ok(true),
        }
    };
    let mut windows = payload.windows(window_width.get());
    while let Some(bytes) = ctx.next_charged(&mut windows, "scan SLDPRT typed offset plane sources")? {
        let Some(source) = View::u32_le_at(bytes, 0) else {
            continue;
        };
        let (Some(signature), Some(selector)) = (bytes.get(4..8), View::u32_le_at(bytes, 10))
        else {
            continue;
        };
        if ctx.contains_hash_set(known_reference_plane_sources, &source, "find SLDPRT known reference plane source")?
            && Some(source) != self_source
            && signature != [0; 4]
            && bytes.get(8..10) == Some(&[0, 0])
            && selector <= 3
            && bytes.get(14..18) == Some(&1u32.to_le_bytes())
            && bytes.get(18..22) == Some(&[0; 4])
            && bytes.get(26..38) == Some(&[0; 12])
            && bytes.get(38..46) == Some(TERMINATOR)
            && !observe(source)?
        {
            return Ok(None);
        }
    }
    if let Some(source) = compact_offset_plane_source(ctx, payload)? {
        if Some(source) != self_source && ctx.contains_hash_set(known_sources, &source, "find SLDPRT known plane source")? && !observe(source)? {
            return Ok(None);
        }
    }
    let mut ambiguous = false;
    for_each_structured_offset_plane_source(ctx, payload, |source| {
        let proceed = observe(source)?;
        ambiguous |= !proceed;
        Ok(proceed)
    })?;
    if ambiguous {
        return Ok(None);
    }
    for_each_classed_offset_plane_source(ctx, payload, |source| {
        let proceed = observe(source)?;
        ambiguous |= !proceed;
        Ok(proceed)
    })?;
    Ok(if ambiguous { None } else { unique })
}

fn legacy_offset_plane_face_alias(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
) -> Result<Option<(usize, u32)>, CodecError> {
    const TERMINATOR: &[u8] = b"\xc7\xcf\xff\xff\xc7\xcf\xff\xff";
    let Some(window_width) = std::num::NonZeroUsize::new(115) else {
        return Err(ctx.refuse_codec_limit("scan SLDPRT legacy face aliases", 1, 0));
    };
    let mut unique = None;
    let mut windows = payload.windows(window_width.get()).enumerate();
    while let Some((offset, body)) = ctx.next_charged(&mut windows, "scan SLDPRT legacy face aliases")? {
        let alias = (|| {
            let token = View::u16_le_at(body, 0)?;
            if !is_class_token(token)
                || body[2..6] != 2u32.to_le_bytes()
                || body[6..42] != [0; 36]
                || body[42..45] != [0; 3]
                || body[45..61] != [0xff; 16]
                || body[61..69] != [0; 8]
                || body[69..73] != 2u32.to_le_bytes()
                || body[73..77] == [0; 4]
                || body[77..83] != [0, 0, 3, 0, 0, 0]
                || body[83..91] != [1, 0, 0, 0, 0, 0, 0, 0]
                || body[95..99] != [0; 4]
                || body[99..103] != 3u32.to_le_bytes()
                || body[103..107] != [0; 4]
                || &body[107..115] != TERMINATOR
            {
                return None;
            }
            let owner = View::u32_le_at(body, 91)?;
            (owner != 0 && owner != u32::MAX).then_some((offset, owner))
        })();
        let Some(alias) = alias else {
            continue;
        };
        match unique {
            Some(existing) if existing != alias => return Ok(None),
            None => unique = Some(alias),
            Some(_) => {}
        }
    }
    Ok(unique)
}

const MINIMAL_REFERENCE_PLANE_FRAME_LEN: usize = 81;
const COMPACT_REFERENCE_PLANE_FRAME_LEN: usize = 82;
const ANGLED_REFERENCE_PLANE_FRAME_LEN: usize = 121;
const REFERENCE_PLANE_FRAME_TOLERANCE: f64 = 1.0e-9;

type PlaneFrame = (Point3, Vector3, Vector3);

fn include_reference_plane_frame(first: &mut Option<PlaneFrame>, frame: PlaneFrame) -> bool {
    match first {
        None => {
            *first = Some(frame);
            true
        }
        Some(existing) => *existing == frame,
    }
}

pub(super) fn explicit_reference_plane_frame(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
) -> Result<Result<Option<PlaneFrame>, ()>, CodecError> {
    let mut first = None;
    let mut ambiguous = false;
    for_each_matrix_reference_plane_frame(ctx, payload, |_, frame| {
        if include_reference_plane_frame(&mut first, frame) {
            Ok(true)
        } else {
            ambiguous = true;
            Ok(false)
        }
    })?;
    if ambiguous {
        return Ok(Err(()));
    }
    for_each_fixed_reference_plane_frame(ctx, payload, |_, frame| {
        if include_reference_plane_frame(&mut first, frame) {
            Ok(true)
        } else {
            ambiguous = true;
            Ok(false)
        }
    })?;
    if ambiguous {
        return Ok(Err(()));
    }
    for_each_angled_reference_plane_frame(ctx, payload, |offset, frame| {
        if strong_reference_plane_overlap(ctx, payload, offset, ANGLED_REFERENCE_PLANE_FRAME_LEN)? {
            return Ok(true);
        }
        if include_reference_plane_frame(&mut first, frame) {
            Ok(true)
        } else {
            ambiguous = true;
            Ok(false)
        }
    })?;
    if ambiguous {
        return Ok(Err(()));
    }
    for_each_minimal_reference_plane_frame(ctx, payload, |_, frame| {
        if include_reference_plane_frame(&mut first, frame) {
            Ok(true)
        } else {
            ambiguous = true;
            Ok(false)
        }
    })?;
    if ambiguous {
        return Ok(Err(()));
    }
    for_each_compact_reference_plane_frame(ctx, payload, |offset, frame| {
        if strong_reference_plane_overlap(ctx, payload, offset, COMPACT_REFERENCE_PLANE_FRAME_LEN)?
        {
            return Ok(true);
        }
        if include_reference_plane_frame(&mut first, frame) {
            Ok(true)
        } else {
            ambiguous = true;
            Ok(false)
        }
    })?;
    if ambiguous {
        return Ok(Err(()));
    }
    Ok(Ok(first))
}

fn strong_reference_plane_overlap(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
    offset: usize,
    len: usize,
) -> Result<bool, CodecError> {
    let preceding_bytes = matrix_plane::LEN.max(fixed_plane::LEN) - 1;
    let start = offset
        .checked_sub(preceding_bytes)
        .map_or(0, std::convert::identity);
    let end = offset
        .checked_add(len)
        .ok_or_else(|| {
            ctx.refuse_codec_limit(
                "scan SLDPRT strong reference plane overlap",
                u64::MAX - 1,
                u64::MAX,
            )
        })?
        .min(payload.len());
    let Some(strong_starts) = payload.get(start..end) else {
        return Ok(false);
    };
    for (relative, _) in ctx
        .admit_iter(strong_starts, "scan SLDPRT strong reference plane overlap")?
        .enumerate()
    {
        let strong_offset = start + relative;
        let matrix = strong_offset
            .checked_add(matrix_plane::LEN)
            .and_then(|end| payload.get(strong_offset..end));
        if ranges_overlap(offset, len, strong_offset, matrix_plane::LEN)
            && matrix
                .and_then(matrix_reference_plane_frame_record)
                .is_some()
        {
            return Ok(true);
        }
        let fixed = strong_offset
            .checked_add(fixed_plane::LEN)
            .and_then(|end| payload.get(strong_offset..end));
        if ranges_overlap(offset, len, strong_offset, fixed_plane::LEN)
            && fixed
                .and_then(|bytes| {
                    fixed_reference_plane_frame(bytes)
                        .or_else(|| repeated_normal_reference_plane_frame(bytes))
                })
                .is_some()
        {
            return Ok(true);
        }
    }
    Ok(false)
}

fn fixed_reference_plane_overlap(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
    offset: usize,
    len: usize,
) -> Result<bool, CodecError> {
    let start = offset
        .checked_sub(fixed_plane::LEN - 1)
        .map_or(0, std::convert::identity);
    let end = offset
        .checked_add(len)
        .ok_or_else(|| {
            ctx.refuse_codec_limit(
                "scan SLDPRT fixed reference plane overlap",
                u64::MAX - 1,
                u64::MAX,
            )
        })?
        .min(payload.len());
    let Some(fixed_starts) = payload.get(start..end) else {
        return Ok(false);
    };
    for (relative, _) in ctx
        .admit_iter(fixed_starts, "scan SLDPRT fixed reference plane overlap")?
        .enumerate()
    {
        let fixed_offset = start + relative;
        let fixed = fixed_offset
            .checked_add(fixed_plane::LEN)
            .and_then(|end| payload.get(fixed_offset..end));
        if ranges_overlap(offset, len, fixed_offset, fixed_plane::LEN)
            && fixed
                .and_then(|bytes| {
                    fixed_reference_plane_frame(bytes)
                        .or_else(|| repeated_normal_reference_plane_frame(bytes))
                })
                .is_some()
        {
            return Ok(true);
        }
    }
    Ok(false)
}

fn constraint_reference_plane_frame(
    payload: &[u8],
    class_offset: usize,
    class_name: &str,
) -> Option<(Point3, Vector3, Vector3)> {
    let body = class_offset.checked_add(6 + class_name.len())?;
    match class_name {
        "moConstraintCoincLineAtAnglePlaneRefplaneData_c" => {
            matrix_reference_plane_frame_record(payload.get(body..body + matrix_plane::LEN)?)
        }
        "moConstraintCoincLineParallelPlaneRefplaneData_c"
        | "moConstraintPerpPlnTanOneCylinderRefplaneData_c"
        | "moFacePtRefPlnData_c"
        | "moFixedRefPlnData_c" => {
            let frame = payload.get(body..body + fixed_plane::LEN)?;
            if class_name == "moFixedRefPlnData_c" {
                fixed_reference_plane_frame(frame)
                    .or_else(|| repeated_normal_reference_plane_frame(frame))
            } else {
                fixed_reference_plane_frame(frame)
            }
        }
        "moDefaultRefPlnData_c" | "moConstraintPrllPlnTanOneCylinderRefplaneData_c" => {
            minimal_reference_plane_frame_record(
                payload.get(body..body + MINIMAL_REFERENCE_PLANE_FRAME_LEN)?,
            )
        }
        "moFaceRefPlnData_c" => {
            fixed_reference_plane_frame(payload.get(body..body + fixed_plane::LEN)?).or_else(|| {
                minimal_reference_plane_frame_record(
                    payload.get(body..body + MINIMAL_REFERENCE_PLANE_FRAME_LEN)?,
                )
            })
        }
        _ => None,
    }
}

pub(super) fn reference_plane_frame_key(
    (origin, normal, u_axis): &(Point3, Vector3, Vector3),
) -> [u64; 9] {
    let canonical_bits = |value: f64| if value == 0.0 { 0 } else { value.to_bits() };
    [
        canonical_bits(origin.x),
        canonical_bits(origin.y),
        canonical_bits(origin.z),
        canonical_bits(normal.x),
        canonical_bits(normal.y),
        canonical_bits(normal.z),
        canonical_bits(u_axis.x),
        canonical_bits(u_axis.y),
        canonical_bits(u_axis.z),
    ]
}

fn fixed_reference_plane_frame(bytes: &[u8]) -> Option<(Point3, Vector3, Vector3)> {
    const NATIVE_TO_IR: f64 = 1000.0;
    if bytes.len() != fixed_plane::LEN || bytes.get(fixed_plane::FRAME_MARKER) != Some(&1) {
        return None;
    }
    let scalar = |offset| {
        let value = View::f64_le_at(bytes, offset)?;
        value.is_finite().then_some(value)
    };
    let native_origin = [
        scalar(fixed_plane::ORIGIN)?,
        scalar(fixed_plane::ORIGIN + 8)?,
        scalar(fixed_plane::ORIGIN + 16)?,
    ];
    let origin = Point3::new(
        native_origin[0] * NATIVE_TO_IR,
        native_origin[1] * NATIVE_TO_IR,
        native_origin[2] * NATIVE_TO_IR,
    );
    let normal = Vector3::new(
        scalar(fixed_plane::NORMAL)?,
        scalar(fixed_plane::NORMAL + 8)?,
        scalar(fixed_plane::NORMAL + 16)?,
    );
    let u_axis = Vector3::new(
        scalar(fixed_plane::U_AXIS)?,
        scalar(fixed_plane::U_AXIS + 8)?,
        scalar(fixed_plane::U_AXIS + 16)?,
    );
    let v_axis = Vector3::new(
        scalar(fixed_plane::V_AXIS)?,
        scalar(fixed_plane::V_AXIS + 8)?,
        scalar(fixed_plane::V_AXIS + 16)?,
    );
    ([normal, u_axis, v_axis]
        .into_iter()
        .all(|vector| (vector.norm() - 1.0).abs() <= REFERENCE_PLANE_FRAME_TOLERANCE)
        && normal.dot(u_axis).abs() <= REFERENCE_PLANE_FRAME_TOLERANCE
        && normal.dot(v_axis).abs() <= REFERENCE_PLANE_FRAME_TOLERANCE
        && u_axis.dot(v_axis).abs() <= REFERENCE_PLANE_FRAME_TOLERANCE)
        .then_some((origin, normal, u_axis))
}

fn repeated_normal_reference_plane_frame(bytes: &[u8]) -> Option<(Point3, Vector3, Vector3)> {
    const NATIVE_TO_IR: f64 = 1000.0;
    if bytes.len() != fixed_plane::LEN || bytes.get(fixed_plane::FRAME_MARKER) != Some(&1) {
        return None;
    }
    let scalar = |offset| {
        let value = View::f64_le_at(bytes, offset)?;
        value.is_finite().then_some(value)
    };
    let origin = Point3::new(
        scalar(fixed_plane::ORIGIN)? * NATIVE_TO_IR,
        scalar(fixed_plane::ORIGIN + 8)? * NATIVE_TO_IR,
        scalar(fixed_plane::ORIGIN + 16)? * NATIVE_TO_IR,
    );
    let normal = Vector3::new(
        scalar(fixed_plane::NORMAL)?,
        scalar(fixed_plane::NORMAL + 8)?,
        scalar(fixed_plane::NORMAL + 16)?,
    );
    let first_axis = Vector3::new(
        scalar(fixed_plane::U_AXIS)?,
        scalar(fixed_plane::U_AXIS + 8)?,
        scalar(fixed_plane::U_AXIS + 16)?,
    );
    let second_axis = Vector3::new(
        scalar(fixed_plane::V_AXIS)?,
        scalar(fixed_plane::V_AXIS + 8)?,
        scalar(fixed_plane::V_AXIS + 16)?,
    );
    let unit = |vector: Vector3| (vector.norm() - 1.0).abs() <= REFERENCE_PLANE_FRAME_TOLERANCE;
    let plane_axis = |vector: Vector3| {
        unit(vector) && normal.dot(vector).abs() <= REFERENCE_PLANE_FRAME_TOLERANCE
    };
    let repeated_normal = |vector: Vector3| {
        unit(vector)
            && (normal.dot(vector).abs() - 1.0).abs() <= REFERENCE_PLANE_FRAME_TOLERANCE
            && normal.cross(vector).norm() <= REFERENCE_PLANE_FRAME_TOLERANCE
    };
    let u_axis = if plane_axis(first_axis) && repeated_normal(second_axis) {
        first_axis
    } else if plane_axis(second_axis) && repeated_normal(first_axis) {
        second_axis
    } else {
        return None;
    };
    Some((origin, normal, u_axis))
}

type ReferencePlaneFrame = (Point3, Vector3, Vector3);

fn for_each_fixed_reference_plane_frame(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
    mut visit: impl FnMut(usize, ReferencePlaneFrame) -> Result<bool, CodecError>,
) -> Result<(), CodecError> {
    let Some(window_width) = std::num::NonZeroUsize::new(fixed_plane::LEN) else {
        return Err(ctx.refuse_codec_limit("scan SLDPRT fixed reference plane frames", 1, 0));
    };
    let mut windows = payload.windows(window_width.get()).enumerate();
    while let Some((offset, bytes)) = ctx.next_charged(&mut windows, "scan SLDPRT fixed reference plane frames")? {
        let matrix = offset
            .checked_add(matrix_plane::LEN)
            .and_then(|end| payload.get(offset..end))
            .and_then(matrix_reference_plane_frame_record);
        if matrix.is_some() {
            continue;
        }
        let Some(frame) = fixed_reference_plane_frame(bytes)
            .or_else(|| repeated_normal_reference_plane_frame(bytes))
        else {
            continue;
        };
        if !visit(offset, frame)? {
            break;
        }
    }
    Ok(())
}

fn offset_reference_plane_frame_pair(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
    distance: cadmpeg_ir::scalar::Length,
) -> Result<Option<(ReferencePlaneFrame, ReferencePlaneFrame)>, CodecError> {
    let valid_pair = |result: ReferencePlaneFrame, reference: ReferencePlaneFrame| {
        offset_plane_reference_frame_matches(reference, result, distance.get())
            .then_some((result, reference))
    };
    let mut temporary_storage = ctx.reserve_scoped(0, "SLDPRT offset frame workspace")?;
    let mut matrix_candidates = Vec::new();
    for_each_matrix_reference_plane_frame(ctx, payload, |offset, frame| {
        temporary_storage.with_storage(|| ctx.push_vec(&mut matrix_candidates, (offset, frame), "collect SLDPRT matrix plane frames"))?;
        Ok(true)
    })?;
    let mut fixed_candidates = Vec::new();
    for_each_fixed_reference_plane_frame(ctx, payload, |offset, frame| {
        temporary_storage.with_storage(|| ctx.push_vec(&mut fixed_candidates, (offset, frame), "collect SLDPRT fixed plane frames"))?;
        Ok(true)
    })?;
    if let [(_, result), (_, reference)] = fixed_candidates.as_slice() {
        return Ok(valid_pair(*result, *reference));
    }
    let mut matrix_unique = [None; 3];
    let mut matrix_count = 0;
    let mut matrices = matrix_candidates.iter();
    while let Some((_, frame)) = ctx.next_charged(&mut matrices, "deduplicate SLDPRT matrix plane frames")? {
        if matrix_unique[..matrix_count].contains(&Some(*frame)) {
            continue;
        }
        if matrix_count == matrix_unique.len() {
            break;
        }
        matrix_unique[matrix_count] = Some(*frame);
        matrix_count += 1;
    }
    if let [Some(result), Some(reference), None] = matrix_unique {
        return Ok(valid_pair(result, reference));
    }
    let mut frames = Vec::new();
    let mut fixed_cursor = 0;
    let mut matrix_cursor = 0;
    for (offset, _) in ctx
        .admit_iter(payload, "scan SLDPRT offset plane frame positions")?
        .enumerate()
    {
        let fixed = fixed_candidates.get(fixed_cursor).and_then(|(candidate_offset, frame)| {
            (*candidate_offset == offset).then_some(*frame)
        });
        if fixed.is_some() { fixed_cursor += 1; }
        let matrix = matrix_candidates.get(matrix_cursor).and_then(|(candidate_offset, frame)| {
            (*candidate_offset == offset).then_some(*frame)
        });
        if matrix.is_some() { matrix_cursor += 1; }
        let compact = match payload.get(offset..offset + COMPACT_REFERENCE_PLANE_FRAME_LEN) {
            Some(window) => compact_reference_plane_frame(ctx, window)?,
            None => None,
        };
        let candidates = [
            fixed,
            matrix,
            payload
                .get(offset..offset + MINIMAL_REFERENCE_PLANE_FRAME_LEN)
                .and_then(minimal_reference_plane_frame_record),
            compact,
        ];
        let mut unique = [None; 4];
        let mut count = 0;
        for frame in candidates.into_iter().flatten() {
            if unique[..count].contains(&Some(frame)) { continue; }
            unique[count] = Some(frame);
            count += 1;
            temporary_storage.with_storage(|| ctx.push_vec(&mut frames, (offset, frame), "collect SLDPRT offset plane frames"))?;
        }
    }
    let mut unique_pair = None;
    let mut ambiguous = false;
    for (result_index, (result_offset, result)) in ctx
        .admit_iter(&frames, "match SLDPRT offset plane frame pairs")?
        .enumerate()
    {
        let Some(reference_candidates) = frames.get(result_index + 1..) else {
            continue;
        };
        for (reference_offset, reference) in ctx.admit_iter(
            reference_candidates,
            "match SLDPRT offset plane frame pairs",
        )? {
            if result_offset >= reference_offset {
                continue;
            }
            if let Some(pair) = valid_pair(*result, *reference) {
                match unique_pair {
                    None => unique_pair = Some(pair),
                    Some(previous) if previous != pair => ambiguous = true,
                    Some(_) => {}
                }
            }
        }
    }
    Ok(if ambiguous { None } else { unique_pair })
}

fn constraint_midplane_frame(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
) -> Result<Option<(Point3, Vector3, Vector3)>, CodecError> {
    const CLASS: &[u8] = b"moConstraintMidPlaneRefplaneData_c";
    const NATIVE_TO_IR: f64 = 1000.0;
    let record_len = CLASS_MARKER.len() + 2 + CLASS.len();
    let class_len = u16::try_from(CLASS.len())
        .map_err(|_| CodecError::malformed("SLDPRT midplane constraint class name is too long"))?
        .to_le_bytes();
    let mut unique = None;
    let mut ambiguous = false;
    let mut source_items = payload.windows(record_len).enumerate().into_iter();
    while let Some((offset, bytes)) = ctx.next_charged(&mut source_items, "scan SLDPRT midplane constraints")? {
        if bytes.get(..CLASS_MARKER.len()) != Some(CLASS_MARKER)
            || bytes.get(CLASS_MARKER.len()..CLASS_MARKER.len() + 2) != Some(&class_len)
            || bytes.get(CLASS_MARKER.len() + 2..) != Some(CLASS)
        {
            continue;
        }
        let body = offset + record_len;
        let frame = (|| {
            payload.get(body..body + 8)?;
            let scalar = |relative| {
                let value = View::f64_le_at(payload, body + relative)?;
                value.is_finite().then_some(value)
            };
            let tolerance = scalar(8)?;
            if tolerance.abs() > EPS_REFERENCE_GEOMETRY_CONSTRAINT_MIDPLANE_FRAME_E9 {
                return None;
            }
            let distance = scalar(16)?;
            let normal = Vector3::new(scalar(24)?, scalar(32)?, scalar(40)?);
            let squared_norm = normal.x * normal.x + normal.y * normal.y + normal.z * normal.z;
            if (squared_norm - 1.0).abs() > EPS_REFERENCE_GEOMETRY_CONSTRAINT_MIDPLANE_FRAME_E9 {
                return None;
            }
            let reference = if normal.x.abs() <= normal.y.abs() && normal.x.abs() <= normal.z.abs()
            {
                Vector3::new(1.0, 0.0, 0.0)
            } else if normal.y.abs() <= normal.z.abs() {
                Vector3::new(0.0, 1.0, 0.0)
            } else {
                Vector3::new(0.0, 0.0, 1.0)
            };
            let projection =
                reference.x * normal.x + reference.y * normal.y + reference.z * normal.z;
            let u_axis = Vector3::new(
                reference.x - projection * normal.x,
                reference.y - projection * normal.y,
                reference.z - projection * normal.z,
            );
            let u_length = (u_axis.x * u_axis.x + u_axis.y * u_axis.y + u_axis.z * u_axis.z).sqrt();
            let u_axis = Vector3::new(
                u_axis.x / u_length,
                u_axis.y / u_length,
                u_axis.z / u_length,
            );
            Some((
                Point3::new(
                    normal.x * distance * NATIVE_TO_IR,
                    normal.y * distance * NATIVE_TO_IR,
                    normal.z * distance * NATIVE_TO_IR,
                ),
                normal,
                u_axis,
            ))
        })();
        if let Some(frame) = frame {
            match unique {
                None => unique = Some(frame),
                Some(previous) if previous != frame => ambiguous = true,
                Some(_) => {}
            }
        }
    }
    Ok(if ambiguous { None } else { unique })
}

fn for_each_angled_reference_plane_frame(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
    mut visit: impl FnMut(usize, ReferencePlaneFrame) -> Result<bool, CodecError>,
) -> Result<(), CodecError> {
    let scalar = |bytes: &[u8], relative| {
        let value = View::f64_le_at(bytes, relative)?;
        value.is_finite().then_some(value)
    };
    let Some(window_width) = std::num::NonZeroUsize::new(ANGLED_REFERENCE_PLANE_FRAME_LEN) else {
        return Err(ctx.refuse_codec_limit("scan SLDPRT angled reference plane frames", 1, 0));
    };
    let mut windows = payload.windows(window_width.get()).enumerate();
    while let Some((offset, bytes)) = ctx.next_charged(&mut windows, "scan SLDPRT angled reference plane frames")? {
        if bytes.get(16) != Some(&1) {
            continue;
        }
        let padding_nonzero = match bytes.get(89..113) {
            Some(padding) => padding.iter()
                .any(|byte| *byte != 0),
            None => true,
        };
        if padding_nonzero || scalar(bytes, 113) != Some(1.0) {
            continue;
        }
        let (Some(u_x), Some(u_y), Some(u_z)) =
            (scalar(bytes, 17), scalar(bytes, 25), scalar(bytes, 33))
        else {
            continue;
        };
        let (Some(n_x), Some(n_y), Some(n_z)) =
            (scalar(bytes, 41), scalar(bytes, 49), scalar(bytes, 57))
        else {
            continue;
        };
        let (Some(v_x), Some(v_y), Some(v_z)) =
            (scalar(bytes, 65), scalar(bytes, 73), scalar(bytes, 81))
        else {
            continue;
        };
        let (Some(origin_z), Some(origin_y)) = (scalar(bytes, 0), scalar(bytes, 8)) else {
            continue;
        };
        let u_axis = Vector3::new(u_x, u_y, u_z);
        let normal = Vector3::new(n_x, n_y, n_z);
        let v_axis = Vector3::new(v_x, v_y, v_z);
        let directions = [u_axis, normal, v_axis];
        if normal.x != 0.0
            || origin_z.to_bits() != normal.z.to_bits()
            || origin_y.to_bits() != normal.y.to_bits()
            || directions.iter()
                .any(|vector| {
                    (vector.norm() - 1.0).abs()
                        > EPS_REFERENCE_GEOMETRY_ANGLED_REFERENCE_PLANE_FRAME_E9
                })
            || u_axis.dot(normal).abs() > EPS_REFERENCE_GEOMETRY_ANGLED_REFERENCE_PLANE_FRAME_E9
            || u_axis.dot(v_axis).abs() > EPS_REFERENCE_GEOMETRY_ANGLED_REFERENCE_PLANE_FRAME_E9
            || normal.dot(v_axis).abs() > EPS_REFERENCE_GEOMETRY_ANGLED_REFERENCE_PLANE_FRAME_E9
            || fixed_reference_plane_overlap(
                ctx,
                payload,
                offset,
                ANGLED_REFERENCE_PLANE_FRAME_LEN,
            )?
        {
            continue;
        }
        let frame = (Point3::new(0.0, 0.0, 0.0), normal, u_axis);
        if !visit(offset, frame)? {
            break;
        }
    }
    Ok(())
}

#[cfg(test)]
fn matrix_reference_plane_frame(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
) -> Result<Option<ReferencePlaneFrame>, CodecError> {
    let mut selected = None;
    let mut ambiguous = false;
    for_each_matrix_reference_plane_frame(ctx, payload, |_, frame| {
        if include_reference_plane_frame(&mut selected, frame) {
            Ok(true)
        } else {
            ambiguous = true;
            Ok(false)
        }
    })?;
    Ok(if ambiguous { None } else { selected })
}

fn for_each_matrix_reference_plane_frame(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
    mut visit: impl FnMut(usize, ReferencePlaneFrame) -> Result<bool, CodecError>,
) -> Result<(), CodecError> {
    let Some(window_width) = std::num::NonZeroUsize::new(matrix_plane::LEN) else {
        return Err(ctx.refuse_codec_limit("scan SLDPRT matrix reference plane frames", 1, 0));
    };
    let mut windows = payload.windows(window_width.get()).enumerate();
    while let Some((offset, bytes)) = ctx.next_charged(&mut windows, "scan SLDPRT matrix reference plane frames")? {
        let Some(frame) = matrix_reference_plane_frame_record(bytes) else {
            continue;
        };
        if !visit(offset, frame)? {
            break;
        }
    }
    Ok(())
}

fn matrix_reference_plane_frame_record(bytes: &[u8]) -> Option<ReferencePlaneFrame> {
    const NATIVE_TO_IR: f64 = 1000.0;
    let scalar = |bytes: &[u8], relative| {
        let value = View::f64_le_at(bytes, relative)?;
        value.is_finite().then_some(value)
    };
    if bytes.len() != matrix_plane::LEN || bytes[matrix_plane::FRAME_MARKER] != 1 {
        return None;
    }
    let origin = Point3::new(
        scalar(bytes, matrix_plane::ORIGIN)? * NATIVE_TO_IR,
        scalar(bytes, matrix_plane::ORIGIN + 8)? * NATIVE_TO_IR,
        scalar(bytes, matrix_plane::ORIGIN + 16)? * NATIVE_TO_IR,
    );
    let normal = Vector3::new(
        scalar(bytes, matrix_plane::NORMAL)?,
        scalar(bytes, matrix_plane::NORMAL + 8)?,
        scalar(bytes, matrix_plane::NORMAL + 16)?,
    );
    let rows = [
        Vector3::new(
            scalar(bytes, matrix_plane::BASIS_MATRIX)?,
            scalar(bytes, matrix_plane::BASIS_MATRIX + 8)?,
            scalar(bytes, matrix_plane::BASIS_MATRIX + 16)?,
        ),
        Vector3::new(
            scalar(bytes, matrix_plane::BASIS_MATRIX + 24)?,
            scalar(bytes, matrix_plane::BASIS_MATRIX + 32)?,
            scalar(bytes, matrix_plane::BASIS_MATRIX + 40)?,
        ),
        Vector3::new(
            scalar(bytes, matrix_plane::BASIS_MATRIX + 48)?,
            scalar(bytes, matrix_plane::BASIS_MATRIX + 56)?,
            scalar(bytes, matrix_plane::BASIS_MATRIX + 64)?,
        ),
    ];
    let u_axis = Vector3::new(rows[0].x, rows[1].x, rows[2].x);
    let v_axis = Vector3::new(rows[0].y, rows[1].y, rows[2].y);
    let matrix_normal = Vector3::new(rows[0].z, rows[1].z, rows[2].z);
    if [normal, u_axis, v_axis, matrix_normal]
        .into_iter()
        .any(|vector| {
            (vector.norm() - 1.0).abs()
                > EPS_REFERENCE_GEOMETRY_MATRIX_REFERENCE_PLANE_FRAME_CANDIDATES_E9
        })
        || u_axis.dot(v_axis).abs()
            > EPS_REFERENCE_GEOMETRY_MATRIX_REFERENCE_PLANE_FRAME_CANDIDATES_E9
        || u_axis.dot(matrix_normal).abs()
            > EPS_REFERENCE_GEOMETRY_MATRIX_REFERENCE_PLANE_FRAME_CANDIDATES_E9
        || v_axis.dot(matrix_normal).abs()
            > EPS_REFERENCE_GEOMETRY_MATRIX_REFERENCE_PLANE_FRAME_CANDIDATES_E9
        || normal.dot(matrix_normal)
            < 1.0 - EPS_REFERENCE_GEOMETRY_MATRIX_REFERENCE_PLANE_FRAME_CANDIDATES_E9
        || u_axis.cross(v_axis).dot(matrix_normal)
            < 1.0 - EPS_REFERENCE_GEOMETRY_MATRIX_REFERENCE_PLANE_FRAME_CANDIDATES_E9
    {
        return None;
    }
    Some((origin, normal, u_axis))
}

#[cfg(test)]
fn minimal_reference_plane_frame(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
) -> Result<Option<ReferencePlaneFrame>, CodecError> {
    let mut selected = None;
    let mut ambiguous = false;
    for_each_minimal_reference_plane_frame(ctx, payload, |_, frame| {
        if include_reference_plane_frame(&mut selected, frame) {
            Ok(true)
        } else {
            ambiguous = true;
            Ok(false)
        }
    })?;
    Ok(if ambiguous { None } else { selected })
}

fn for_each_minimal_reference_plane_frame(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
    mut visit: impl FnMut(usize, ReferencePlaneFrame) -> Result<bool, CodecError>,
) -> Result<(), CodecError> {
    let Some(window_width) = std::num::NonZeroUsize::new(MINIMAL_REFERENCE_PLANE_FRAME_LEN) else {
        return Err(ctx.refuse_codec_limit("scan SLDPRT minimal reference plane frames", 1, 0));
    };
    let mut windows = payload.windows(window_width.get()).enumerate();
    while let Some((offset, bytes)) = ctx.next_charged(&mut windows, "scan SLDPRT minimal reference plane frames")? {
        if let Some(frame) = minimal_reference_plane_frame_record(bytes) {
            if !visit(offset, frame)? {
                break;
            }
        }
    }
    Ok(())
}

fn minimal_reference_plane_frame_record(bytes: &[u8]) -> Option<ReferencePlaneFrame> {
    const NATIVE_TO_IR: f64 = 1000.0;
    let scalar = |bytes: &[u8], relative| {
        let value = View::f64_le_at(bytes, relative)?;
        value.is_finite().then_some(value)
    };
    if bytes.len() != MINIMAL_REFERENCE_PLANE_FRAME_LEN {
        return None;
    }
    let origin = Point3::new(scalar(bytes, 0)?, scalar(bytes, 8)?, scalar(bytes, 16)?);
    let normal = Vector3::new(scalar(bytes, 24)?, scalar(bytes, 32)?, scalar(bytes, 40)?);
    let tail = [scalar(bytes, 57)?, scalar(bytes, 65)?, scalar(bytes, 73)?];
    if normal != Vector3::new(0.0, 0.0, 1.0)
        || bytes[48..56].iter().any(|byte| *byte != 0)
        || bytes[56] != 0x80
        || tail[0].to_bits() != (-0.0_f64).to_bits()
        || tail[1].to_bits() != (-origin.z).to_bits()
        || tail[2] != 1.0
    {
        return None;
    }
    Some((
        Point3::new(
            origin.x * NATIVE_TO_IR,
            origin.y * NATIVE_TO_IR,
            origin.z * NATIVE_TO_IR,
        ),
        normal,
        Vector3::new(1.0, 0.0, 0.0),
    ))
}

fn compact_reference_plane_frame(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
) -> Result<Option<(Point3, Vector3, Vector3)>, CodecError> {
    let mut selected = None;
    let mut ambiguous = false;
    for_each_compact_reference_plane_frame(ctx, payload, |_, frame| {
        if include_reference_plane_frame(&mut selected, frame) {
            Ok(true)
        } else {
            ambiguous = true;
            Ok(false)
        }
    })?;
    Ok(if ambiguous { None } else { selected })
}

fn for_each_compact_reference_plane_frame(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
    mut visit: impl FnMut(usize, ReferencePlaneFrame) -> Result<bool, CodecError>,
) -> Result<(), CodecError> {
    const NATIVE_TO_IR: f64 = 1000.0;
    let scalar = |bytes: &[u8], relative| {
        let value = View::f64_le_at(bytes, relative)?;
        value.is_finite().then_some(value)
    };
    let Some(window_width) = std::num::NonZeroUsize::new(COMPACT_REFERENCE_PLANE_FRAME_LEN) else {
        return Err(ctx.refuse_codec_limit("scan SLDPRT compact reference plane frames", 1, 0));
    };
    let mut windows = payload.windows(window_width.get()).enumerate();
    while let Some((offset, bytes)) = ctx.next_charged(&mut windows, "scan SLDPRT compact reference plane frames")? {
        if bytes[64] != 0 || bytes[81] != 0 {
            continue;
        }
        let candidates = (|| {
            let origin = Point3::new(
                scalar(bytes, 0)? * NATIVE_TO_IR,
                scalar(bytes, 8)? * NATIVE_TO_IR,
                scalar(bytes, 16)? * NATIVE_TO_IR,
            );
            let normal_xy = scalar(bytes, 24).zip(scalar(bytes, 32))?;
            let u_axis = Vector3::new(scalar(bytes, 40)?, scalar(bytes, 48)?, scalar(bytes, 56)?);
            let v_xy = scalar(bytes, 65).zip(scalar(bytes, 73))?;
            if (u_axis.dot(u_axis) - 1.0).abs()
                > EPS_REFERENCE_GEOMETRY_COMPACT_REFERENCE_PLANE_FRAME_E9
            {
                return None;
            }
            let remaining = 1.0 - v_xy.0 * v_xy.0 - v_xy.1 * v_xy.1;
            if remaining < -EPS_REFERENCE_GEOMETRY_COMPACT_REFERENCE_PLANE_FRAME_E9 {
                return None;
            }
            let omitted = remaining.max(0.0).sqrt();
            let pair = [omitted, -omitted].map(|v_z| {
                let v_axis = Vector3::new(v_xy.0, v_xy.1, v_z);
                let normal = u_axis.cross(v_axis);
                (u_axis.dot(v_axis).abs()
                    <= EPS_REFERENCE_GEOMETRY_COMPACT_REFERENCE_PLANE_FRAME_E9
                    && (normal.dot(normal) - 1.0).abs()
                        <= EPS_REFERENCE_GEOMETRY_COMPACT_REFERENCE_PLANE_FRAME_E9
                    && (normal.x - normal_xy.0).abs()
                        <= EPS_REFERENCE_GEOMETRY_COMPACT_REFERENCE_PLANE_FRAME_E9
                    && (normal.y - normal_xy.1).abs()
                        <= EPS_REFERENCE_GEOMETRY_COMPACT_REFERENCE_PLANE_FRAME_E9)
                    .then_some((offset, (origin, normal, u_axis)))
            });
            Some(pair)
        })();
        let Some(mut pair) = candidates else {
            continue;
        };
        ctx.stable_sort_by_key(
            &mut pair,
            |value| {
                value
                    .as_ref()
                    .map_or([u64::MAX; 9], |(_, frame)| reference_plane_frame_key(frame))
            },
            Ord::cmp,
            "sldprt compact reference plane frame pair sort",
        )?;
        for candidate in pair.into_iter().flatten() {
            if !visit(candidate.0, candidate.1)? {
                return Ok(());
            }
        }
    }
    Ok(())
}

fn ranges_overlap(
    left_offset: usize,
    left_len: usize,
    right_offset: usize,
    right_len: usize,
) -> bool {
    // A range end past `usize::MAX` lies beyond every offset.
    right_offset
        .checked_add(right_len)
        .is_none_or(|right_end| left_offset < right_end)
        && left_offset
            .checked_add(left_len)
            .is_none_or(|left_end| right_offset < left_end)
}

#[cfg(test)]
mod reference_geometry_tests;

#[cfg(test)]
mod tests;

#[cfg(test)]
mod frame_ownership;
