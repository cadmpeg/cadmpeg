//! Sketch marker record decoding and profile point coordinates.

use super::bindings::spatial_relation_manager_ranges_charged;
use super::curves::slot_curve_and_center_indices;
use super::endpoints::{
    compact_curve_endpoint_indices, compact_indexed_curve_endpoint_indices,
    compact_legacy_90_geometry_line_roster_indices, current_compact_104_profile_line,
    current_direct_92_profile_line_endpoint_indices,
    extended_geometry_locus_construction_line_endpoint_indices,
    extended_identity_inline_line_record, extended_selector44_indexed_line,
    extended_tagged_indexed_curve_endpoint_indices, extended_terminal_profile_line,
    extended_wide_horizontal_relation_endpoint_indices, legacy_compact_profile_line,
    legacy_referenced_wide_arc_endpoint_indices, legacy_wide_profile_roster_curve,
    marker_is_selected_construction_line, marker_profile_curve_role,
    wide_indexed_curve_endpoint_indices,
};
use super::relation_loci::same_dimension_length;
use super::relation_records::unique_relation_declaration_candidates_charged;
use super::scalars::{operand_kind, ObjectNames};
use super::selections::{marker_local_links, operand_accepts_marker};
use super::{
    is_class_token, LEGACY_EXTENDED_SKETCH_MARKER, LEGACY_SKETCH_MARKER, SKETCH_MARKER,
    SPATIAL_VERTEX_PREFIX,
};
use crate::records::{
    FeatureInputClass, FeatureInputLane, FeatureInputOperandKind, FeatureInputReference,
    FeatureInputRelationBinding, FeatureInputScalar, SketchInputEntity, SketchInputKind,
};
use cadmpeg_core::decode::{DecodeContext, ScopedReservation, View};
use cadmpeg_core::CodecError;
use cadmpeg_ir::features::{FeatureDefinition, FeatureOperation, FinitePoint3};
use cadmpeg_ir::math::Point3;
use cadmpeg_ir::sketches::{
    SpatialSketch, SpatialSketchEntity, SpatialSketchEntityId, SpatialSketchGeometry,
    SpatialSketchGeometryDefinition, SpatialSketchId,
};
use cadmpeg_ir::units::FiniteVector;
use std::collections::{BTreeMap, HashMap};

use crate::layout::compact_current_spatial_marker_point as compact_spatial;
use crate::layout::compact_legacy_142_profile_curve as legacy_142;
use crate::layout::compact_legacy_code_two_profile_point as code_two;
use crate::layout::current_geometry_locus_arc_handle_point as current_arc_handle;
use crate::layout::current_geometry_locus_arc_handle_point_terminal as current_arc_handle_terminal;
use crate::layout::current_indexed_spatial_xyz_point_prefix as spatial_xyz;
use crate::layout::current_indexed_spatial_xyz_terminal_reference_prefix_long as spatial_xyz_terminal_long;
use crate::layout::current_indexed_spatial_xyz_terminal_reference_prefix_short as spatial_xyz_terminal_short;
use crate::layout::legacy_140_single_incidence_profile_point as pt_140;
use crate::layout::legacy_144_single_incidence_profile_point as pt_144;
use crate::layout::wide_spatial_marker_coordinate_prefix as spatial_pre;

/// Project spatial sketches from their model-space marker coordinates or bounded lines.
/// Relation-owned indexed point markers use the relation-tail decoder below;
/// their native relation kind must remain intact for downstream binding.
pub(crate) fn spatial_sketches(
    ctx: &DecodeContext<'_>,
    model_features: &mut [cadmpeg_ir::features::Feature],
    histories: &[crate::records::FeatureHistory],
    lanes: &[FeatureInputLane],
) -> Result<(Vec<SpatialSketch>, Vec<SpatialSketchEntity>), CodecError> {
    const RECORD_INDEX: &str = "index SLDPRT spatial feature records";
    const IDENTITY: &str = "retain SLDPRT spatial identity";
    let mut temporary_storage = ctx.reserve_scoped(0, "SLDPRT markers temporary storage")?;

    let mut records = HashMap::new();
    for history in ctx.admit_iter(histories, RECORD_INDEX)? {
        for record in ctx.admit_iter(&history.features, RECORD_INDEX)? {
            temporary_storage.with_storage(|| {
                ctx.insert_hash_map(&mut records, record.id.as_str(), record, RECORD_INDEX)
            })?;
        }
    }
    // The lane indexes serve every sketch feature; they are built for the first one.
    let mut lane_indexes: Option<Vec<SpatialLaneIndex<'_, '_>>> = None;
    let mut sketches = Vec::new();
    let mut entities = Vec::new();
    for feature in ctx.admit_iter(&mut *model_features, "scan SLDPRT spatial features")? {
        let declared_spatial = matches!(
            feature.evaluation.definition(),
            FeatureDefinition::Operation(FeatureOperation::SpatialSketch { .. })
        );
        if !declared_spatial
            && !matches!(
                feature.evaluation.definition(),
                FeatureDefinition::Operation(FeatureOperation::Sketch { .. })
            )
        {
            continue;
        }
        let Some(native_ref) = feature.native_ref.as_deref() else {
            continue;
        };
        let Some(record) = ctx
            .get_hash_map(&records, native_ref, "lookup SLDPRT spatial feature record")?
            .copied()
        else {
            continue;
        };
        if lane_indexes.is_none() {
            lane_indexes = Some(temporary_storage.with_storage(|| {
                let mut indexes = Vec::new();
                for lane in ctx.admit_iter(lanes, "index SLDPRT spatial feature lanes")? {
                    ctx.push_vec(
                        &mut indexes,
                        SpatialLaneIndex::new(ctx, lane)?,
                        "index SLDPRT spatial feature lanes",
                    )?;
                }
                Ok::<_, CodecError>(indexes)
            })?);
        }
        let Some(lane_indexes) = lane_indexes.as_mut() else {
            continue;
        };
        let mut point_candidates = Vec::new();
        for (position, index) in ctx
            .admit_iter(&*lane_indexes, "scan SLDPRT spatial feature lanes")?
            .enumerate()
        {
            let points = index.feature_points(ctx, native_ref, declared_spatial)?;
            if !points.is_empty() {
                ctx.push_vec(
                    &mut point_candidates,
                    (position, points),
                    "collect SLDPRT spatial point lanes",
                )?;
            }
        }
        let uniform_points = match point_candidates.first() {
            Some((_, first)) => ctx.all_by(
                &point_candidates,
                |(_, candidate)| {
                    Ok(candidate.len() == first.len()
                        && ctx.all_by(
                            candidate.iter().zip(first),
                            |(candidate, first)| Ok(candidate.1 == first.1),
                            "compare SLDPRT spatial point lanes",
                        )?)
                },
                "compare SLDPRT spatial point lanes",
            )?,
            None => false,
        };
        if let Some((position, points)) = point_candidates.first().filter(|_| uniform_points) {
            let Some(index) = lane_indexes.get_mut(*position) else {
                continue;
            };
            let Some(sketch_id) = spatial_sketch_id_charged(ctx, feature.id.as_str())? else {
                continue;
            };
            let mut projected = Vec::new();
            let mut valid_points = true;
            for &(native_ref, point, offset) in
                ctx.admit_iter(points, "project SLDPRT spatial points")?
            {
                let Ok(geometry) =
                    SpatialSketchGeometry::try_from(SpatialSketchGeometryDefinition::Point {
                        position: point,
                    })
                else {
                    valid_points = false;
                    break;
                };
                let native_ref = ctx.copy_retained_text(native_ref, IDENTITY)?;
                ctx.push_vec(
                    &mut projected,
                    (offset, Some(native_ref), geometry),
                    "project SLDPRT spatial points",
                )?;
            }
            if !valid_points {
                continue;
            }
            let lines = spatial_line_vertices_charged(
                ctx,
                &mut temporary_storage,
                histories,
                record,
                index,
            )?
            .unwrap_or_default();
            let lane = index.lane;
            let mut projected_lines = Vec::new();
            let mut valid_lines = true;
            for (offsets, vertices) in ctx
                .admit_iter(&lines.1, "project SLDPRT spatial line offsets")?
                .chunks(const { crate::nonzero(2) })
                .zip(
                    ctx.admit_iter(&lines.2, "project SLDPRT spatial line vertices")?
                        .chunks(const { crate::nonzero(2) }),
                )
            {
                let ([first_offset, _], [start, end]) = (offsets, vertices) else {
                    continue;
                };
                let Ok(geometry) = SpatialSketchGeometry::try_line_from_parts(*start, *end) else {
                    valid_lines = false;
                    break;
                };
                ctx.push_vec(
                    &mut projected_lines,
                    (lines.0 + first_offset, None, geometry),
                    "project SLDPRT spatial lines",
                )?;
            }
            if !valid_lines {
                continue;
            }
            ctx.extend_vec(
                &mut projected,
                projected_lines,
                "merge SLDPRT spatial lines",
            )?;
            ctx.sort_unstable_by(
                &mut projected,
                |value| &value.0,
                Ord::cmp,
                "sort SLDPRT spatial projected points",
            )?;
            let sketch_record_id = sketch_id.try_clone_for_decode(ctx, IDENTITY)?;
            let name = feature
                .name
                .as_deref()
                .map(|name| ctx.copy_retained_text(name, IDENTITY))
                .transpose()?;
            let configuration = if point_candidates.len() == 1 {
                lane.configuration
                    .as_deref()
                    .map(|name| ctx.copy_retained_text(name, IDENTITY))
                    .transpose()?
            } else {
                None
            };
            let native_lane_ref = ctx.copy_retained_text(&lane.id, IDENTITY)?;
            ctx.push_vec(
                &mut sketches,
                SpatialSketch {
                    id: sketch_record_id,
                    name,
                    configuration,
                    visible: None,
                    profiles: Vec::new(),
                    native_ref: Some(native_lane_ref),
                },
                "collect SLDPRT spatial sketches",
            )?;
            for (index, (_, native_ref, geometry)) in ctx
                .admit_iter(projected, "project SLDPRT spatial entities")?
                .enumerate()
            {
                let entity_id = ctx.format_retained(
                    format_args!("{}:entity:{index}", sketch_id.as_str()),
                    "retain SLDPRT spatial entity identity",
                )?;
                ctx.charge_work(
                    cadmpeg_core::decode::u64_from_index(entity_id.len()),
                    "validate SLDPRT spatial entity identity",
                )?;
                let Ok(entity_id) = SpatialSketchEntityId::mint(entity_id) else {
                    continue;
                };
                let owner = sketch_id.try_clone_for_decode(ctx, IDENTITY)?;
                ctx.push_vec(
                    &mut entities,
                    SpatialSketchEntity::new(entity_id, owner, geometry)
                        .with_native_ref(native_ref),
                    "collect SLDPRT spatial entities",
                )?;
            }
            feature
                .evaluation
                .set_definition(FeatureDefinition::Operation(
                    FeatureOperation::SpatialSketch {
                        sketch: Some(sketch_id),
                    },
                ));
            continue;
        }
        if !declared_spatial {
            continue;
        }
        let mut candidates = Vec::new();
        for index in ctx.admit_iter(&mut *lane_indexes, "scan SLDPRT spatial feature lanes")? {
            let lane = index.lane;
            let Some(name) = index.object_name(ctx, record)? else {
                continue;
            };
            let Some(start) = usize::try_from(name.offset).ok() else {
                continue;
            };
            let end = index
                .next_feature_offset(ctx, &mut temporary_storage, histories, name.offset)?
                .and_then(|offset| usize::try_from(offset).ok())
                .unwrap_or(lane.native_payload.len());
            let Some(object) = lane.native_payload.get(start..end) else {
                continue;
            };
            let vertices = spatial_vertex_coordinates_charged(ctx, object)?;
            if vertices.len() >= 2 && vertices.len().is_multiple_of(2) {
                ctx.push_vec(
                    &mut candidates,
                    (lane, vertices),
                    "collect SLDPRT spatial line lanes",
                )?;
            }
        }
        let [(lane, vertices)] = candidates.as_slice() else {
            continue;
        };
        if ctx.any_by(
            vertices.chunks_exact(2),
            |pair| Ok(matches!(pair, [start, end] if start == end)),
            "validate SLDPRT spatial line vertices",
        )? {
            continue;
        }
        let Some(sketch_id) = spatial_sketch_id_charged(ctx, feature.id.as_str())? else {
            continue;
        };
        let mut projected = Vec::new();
        let mut valid_lines = true;
        for (index, pair) in ctx
            .admit_iter(vertices, "project SLDPRT spatial line vertices")?
            .chunks(const { crate::nonzero(2) })
            .filter(|pair| pair.len() == 2)
            .enumerate()
        {
            let [start, end] = pair else {
                continue;
            };
            let entity_id = ctx.format_retained(
                format_args!("{}:entity:{index}", sketch_id.as_str()),
                "retain SLDPRT spatial entity identity",
            )?;
            ctx.charge_work(
                cadmpeg_core::decode::u64_from_index(entity_id.len()),
                "validate SLDPRT spatial entity identity",
            )?;
            let (Ok(entity_id), Ok(geometry)) = (
                SpatialSketchEntityId::mint(entity_id),
                SpatialSketchGeometry::try_line_from_parts(*start, *end),
            ) else {
                valid_lines = false;
                break;
            };
            let owner = sketch_id.try_clone_for_decode(ctx, IDENTITY)?;
            ctx.push_vec(
                &mut projected,
                SpatialSketchEntity::new(entity_id, owner, geometry),
                "project SLDPRT spatial line entities",
            )?;
        }
        if !valid_lines {
            continue;
        }

        let sketch_record_id = sketch_id.try_clone_for_decode(ctx, IDENTITY)?;
        let name = feature
            .name
            .as_deref()
            .map(|name| ctx.copy_retained_text(name, IDENTITY))
            .transpose()?;
        let configuration = lane
            .configuration
            .as_deref()
            .map(|name| ctx.copy_retained_text(name, IDENTITY))
            .transpose()?;
        let native_lane_ref = ctx.copy_retained_text(&lane.id, IDENTITY)?;
        ctx.push_vec(
            &mut sketches,
            SpatialSketch {
                id: sketch_record_id,
                name,
                configuration,
                visible: None,
                profiles: Vec::new(),
                native_ref: Some(native_lane_ref),
            },
            "collect SLDPRT spatial sketches",
        )?;
        ctx.extend_vec(
            &mut entities,
            projected,
            "merge SLDPRT spatial line entities",
        )?;
        feature
            .evaluation
            .set_definition(FeatureDefinition::Operation(
                FeatureOperation::SpatialSketch {
                    sketch: Some(sketch_id),
                },
            ));
    }
    Ok((sketches, entities))
}

/// What spatial sketch projection looks up in one lane for every sketch feature.
struct SpatialLaneIndex<'a, 'ctx> {
    lane: &'a FeatureInputLane,
    /// The lane's object names, indexed when a projection first needs one.
    names: Option<ObjectNames<'a, 'ctx>>,
    /// Relation-manager ranges of the lane, sorted and deduplicated.
    relation_ranges: Vec<(u64, u64)>,
    /// Offsets of the scalars each feature owns. A lane without relation
    /// ranges never consults them, so it indexes none.
    scalar_offsets: HashMap<&'a str, Vec<u64>>,
    /// The markers with an object index each feature owns, in lane order.
    markers: HashMap<&'a str, Vec<&'a SketchInputEntity>>,
    /// Ascending object-name offsets of every history feature in the lane,
    /// built when a projection first needs the end of a feature object.
    feature_offsets: Option<Vec<u64>>,
}

impl<'a, 'ctx> SpatialLaneIndex<'a, 'ctx> {
    fn new(ctx: &DecodeContext<'_>, lane: &'a FeatureInputLane) -> Result<Self, CodecError> {
        const SCALARS: &str = "index SLDPRT spatial scalars";
        const MARKERS: &str = "index SLDPRT spatial sketch markers";
        let relation_ranges = spatial_relation_manager_ranges_charged(ctx, lane)?;
        let mut scalar_offsets = HashMap::new();
        if !relation_ranges.is_empty() {
            for scalar in ctx.admit_iter(&lane.scalars, SCALARS)? {
                if let Some(feature_ref) = scalar.feature_ref.as_deref() {
                    ctx.push_hash_group(
                        &mut scalar_offsets,
                        feature_ref,
                        scalar.offset,
                        SCALARS,
                        SCALARS,
                    )?;
                }
            }
        }
        let mut markers = HashMap::new();
        for marker in ctx.admit_iter(&lane.sketch_entities, MARKERS)? {
            if let (Some(feature_ref), Some(_)) =
                (marker.feature_ref.as_deref(), marker.object_index())
            {
                ctx.push_hash_group(&mut markers, feature_ref, marker, MARKERS, MARKERS)?;
            }
        }
        Ok(Self {
            lane,
            names: None,
            relation_ranges,
            scalar_offsets,
            markers,
            feature_offsets: None,
        })
    }

    /// The spatial points of the feature's markers in this lane, in lane order.
    ///
    /// When a relation-manager range holds one of the feature's scalars, only
    /// markers inside such a range with a native code in `1..=85` qualify.
    fn feature_points(
        &self,
        ctx: &DecodeContext<'_>,
        native_ref: &str,
        declared_spatial: bool,
    ) -> Result<Vec<(&'a str, Point3, usize)>, CodecError> {
        const RANGES: &str = "scan SLDPRT active spatial relation ranges";
        let mut active_ranges = Vec::new();
        if let Some(offsets) = ctx.get_hash_map(
            &self.scalar_offsets,
            native_ref,
            "lookup SLDPRT spatial scalars",
        )? {
            for &(start, end) in
                ctx.admit_iter(&self.relation_ranges, "scan SLDPRT spatial relation ranges")?
            {
                if ctx.any_by(
                    offsets,
                    |offset| Ok(*offset > start && *offset < end),
                    "scan SLDPRT spatial scalars",
                )? {
                    ctx.push_vec(
                        &mut active_ranges,
                        (start, end),
                        "collect SLDPRT active spatial relation ranges",
                    )?;
                }
            }
        }
        let mut points = Vec::new();
        let Some(markers) =
            ctx.get_hash_map(&self.markers, native_ref, "lookup SLDPRT spatial markers")?
        else {
            return Ok(points);
        };
        let payload = &self.lane.native_payload;
        for marker in ctx
            .admit_iter(markers, "scan SLDPRT spatial sketch markers")?
        {
            let Ok(offset) = usize::try_from(marker.offset()) else {
                continue;
            };
            if !active_ranges.is_empty()
                && (!ctx.any_by(
                    &active_ranges,
                    |(start, end)| Ok(marker.offset() > *start && marker.offset() < *end),
                    RANGES,
                )? || !matches!(marker_native_code(payload, offset), Some(1..=85)))
            {
                continue;
            }
            let point = marker_spatial_coordinates(payload, offset).or_else(|| {
                declared_spatial
                    .then(|| current_indexed_spatial_relation_coordinates(payload, offset))
                    .flatten()
            });
            if let Some(point) = point {
                ctx.push_vec(
                    &mut points,
                    (marker.id(), point, offset),
                    "collect SLDPRT spatial points",
                )?;
            }
        }
        Ok(points)
    }

    /// The lane name that serializes the feature's object.
    fn object_name(
        &mut self,
        ctx: &'ctx DecodeContext<'_>,
        feature: &crate::records::Feature,
    ) -> Result<Option<&'a crate::records::FeatureInputName>, CodecError> {
        if self.names.is_none() {
            self.names = Some(ObjectNames::new(ctx, self.lane)?);
        }
        match &self.names {
            Some(names) => names.of(ctx, feature),
            None => Ok(None),
        }
    }

    /// The first history feature object in this lane that starts after `after_offset`.
    fn next_feature_offset(
        &mut self,
        ctx: &'ctx DecodeContext<'_>,
        storage: &mut cadmpeg_core::decode::ScopedReservation<'_>,
        histories: &[crate::records::FeatureHistory],
        after_offset: u64,
    ) -> Result<Option<u64>, CodecError> {
        const OPERATION: &str = "find next SLDPRT spatial feature object";
        if self.feature_offsets.is_none() {
            let offsets = storage.with_storage(|| {
                let mut offsets = Vec::new();
                for history in ctx.admit_iter(histories, OPERATION)? {
                    for candidate in ctx.admit_iter(&history.features, OPERATION)? {
                        if let Some(name) = self.object_name(ctx, candidate)? {
                            ctx.push_vec(&mut offsets, name.offset, OPERATION)?;
                        }
                    }
                }
                ctx.sort_unstable_by(&mut offsets, |offset| offset, Ord::cmp, OPERATION)?;
                Ok::<_, CodecError>(offsets)
            })?;
            self.feature_offsets = Some(offsets);
        }
        let Some(offsets) = self.feature_offsets.as_deref() else {
            return Ok(None);
        };
        let next = ctx.partition_point(offsets, |offset| Ok(*offset <= after_offset), OPERATION)?;
        Ok(offsets.get(next).copied())
    }
}

fn spatial_sketch_id_charged(
    ctx: &DecodeContext<'_>,
    feature_id: &str,
) -> Result<Option<SpatialSketchId>, CodecError> {
    const FEATURE_PREFIX: &str = ":model:feature#";
    const SKETCH_PREFIX: &str = ":model:spatial-sketch#";
    let value = if let Some(index) = ctx.find_text(
        feature_id,
        FEATURE_PREFIX,
        "find SLDPRT spatial feature identity prefix",
    )? {
        let (head, tail) = feature_id.split_at(index);
        let suffix = &tail[FEATURE_PREFIX.len()..];
        ctx.format_retained(
            format_args!("{head}{SKETCH_PREFIX}{suffix}"),
            "retain SLDPRT spatial sketch identity",
        )?
    } else {
        ctx.copy_retained_text(feature_id, "retain SLDPRT spatial sketch identity")?
    };
    ctx.charge_work(
        cadmpeg_core::decode::u64_from_index(value.len()),
        "validate SLDPRT spatial sketch identity",
    )?;
    Ok(SpatialSketchId::mint(value).ok())
}

/// The object start, vertex offsets and vertices of a feature's bounded spatial lines.
#[derive(Debug, Default)]
struct SpatialLineVertices(usize, Vec<usize>, Vec<FinitePoint3>);

fn spatial_line_vertices_charged<'ctx>(
    ctx: &'ctx DecodeContext<'_>,
    storage: &mut cadmpeg_core::decode::ScopedReservation<'_>,
    histories: &[crate::records::FeatureHistory],
    record: &crate::records::Feature,
    index: &mut SpatialLaneIndex<'_, 'ctx>,
) -> Result<Option<SpatialLineVertices>, CodecError> {
    let lane = index.lane;
    let Some(name) = index.object_name(ctx, record)? else {
        return Ok(None);
    };
    let Ok(start) = usize::try_from(name.offset) else {
        return Ok(None);
    };
    let end = index
        .next_feature_offset(ctx, storage, histories, name.offset)?
        .and_then(|offset| usize::try_from(offset).ok())
        .unwrap_or(lane.native_payload.len());
    let Some(object) = lane.native_payload.get(start..end) else {
        return Ok(None);
    };
    let offsets = spatial_vertex_offsets_charged(ctx, object)?;
    let vertices = spatial_vertices_at(ctx, object, &offsets)?;
    Ok((offsets.len().is_multiple_of(2)
        && offsets.len() == vertices.len()
        && ctx.all_by(
            vertices.chunks_exact(2),
            |pair| Ok(matches!(pair, [start, end] if start != end)),
            "validate SLDPRT spatial line vertices",
        )?)
    .then_some(SpatialLineVertices(start, offsets, vertices)))
}

pub(super) fn marker_spatial_coordinate_offset(payload: &[u8], offset: usize) -> Option<usize> {
    if current_indexed_spatial_xyz_point(payload, offset) {
        return offset.checked_add(spatial_xyz::COORDINATES);
    }
    if packed_legacy_marker_body(payload, offset)
        && marker_profile_curve_role(payload, offset) == Some(1)
        && payload.get(offset + 48..offset + 50) == Some(&[0x0e, 0x00])
    {
        return offset.checked_add(50);
    }
    let locus = payload.get(offset + 23..offset + 27)?;
    if payload.get(offset..offset + SKETCH_MARKER.len())? == SKETCH_MARKER
        && marker_native_code(payload, offset) == Some(0)
        && locus == [0x04, 0x00, 0x02, 0x00]
        && marker_profile_curve_role(payload, offset) == Some(1)
        && payload
            .get(offset + compact_spatial::COORDINATE_TAG..offset + compact_spatial::COORDINATES)
            == Some(&[0x0e, 0x00])
        && compact_spatial_point_boundary(payload, offset)
    {
        return offset.checked_add(compact_spatial::COORDINATES);
    }
    let (coordinate_offset, requires_profile_role) =
        match payload.get(offset..offset + SKETCH_MARKER.len())? {
            prefix
                if prefix == SKETCH_MARKER
                    && marker_native_code(payload, offset) == Some(0)
                    && matches!(locus, [0x04, 0x00, 0x02, 0x00] | [0x05, 0x00, 0x01, 0x00])
                    && payload.get(
                        offset + spatial_pre::COORDINATE_TAG..offset + spatial_pre::COORDINATES,
                    ) == Some(&[0x0e, 0x00]) =>
            {
                (offset.checked_add(spatial_pre::COORDINATES)?, true)
            }
            prefix
                if prefix == SKETCH_MARKER
                    && marker_native_code(payload, offset) == Some(0)
                    && locus == [0x05, 0x00, 0x01, 0x00]
                    && payload.get(offset + 56..offset + 58) == Some(&[0x0e, 0x00])
                    && compact_spatial_point_boundary(payload, offset) =>
            {
                (offset.checked_add(58)?, true)
            }
            prefix
                if prefix == SKETCH_MARKER
                    && marker_native_code(payload, offset) == Some(1)
                    && locus == [0x04, 0x00, 0x02, 0x00]
                    && payload.get(
                        offset + compact_spatial::SELECTOR..offset + compact_spatial::SELECTOR + 8,
                    ) == Some(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00])
                    && payload.get(
                        offset + compact_spatial::STATE_VALUE
                            ..offset + compact_spatial::STATE_VALUE + 8,
                    ) == Some(&1.0f64.to_le_bytes())
                    && payload.get(
                        offset + compact_spatial::COORDINATE_TAG
                            ..offset + compact_spatial::COORDINATE_TAG + 2,
                    ) == Some(&[0x0e, 0x00])
                    && compact_spatial_point_boundary(payload, offset) =>
            {
                (offset.checked_add(compact_spatial::COORDINATES)?, true)
            }
            prefix
                if prefix == SKETCH_MARKER
                    && marker_native_code(payload, offset) == Some(1)
                    && locus == [0x05, 0x00, 0x01, 0x00]
                    && payload.get(
                        offset + spatial_pre::COORDINATE_TAG..offset + spatial_pre::COORDINATES,
                    ) == Some(&[0x0e, 0x00]) =>
            {
                (offset.checked_add(spatial_pre::COORDINATES)?, true)
            }
            prefix
                if (prefix == SKETCH_MARKER || prefix == LEGACY_EXTENDED_SKETCH_MARKER)
                    && matches!(marker_native_code(payload, offset), Some(1..=85))
                    && locus == [0x04, 0x00, 0x02, 0x00]
                    && payload.get(offset + 48..offset + 56) == Some(&1.0f64.to_le_bytes())
                    && payload.get(offset + 56..offset + 64) == Some(&[0; 8])
                    && payload.get(
                        offset + spatial_pre::COORDINATE_TAG..offset + spatial_pre::COORDINATES,
                    ) == Some(&[0x0e, 0x00]) =>
            {
                (offset.checked_add(spatial_pre::COORDINATES)?, true)
            }
            prefix
                if prefix == LEGACY_SKETCH_MARKER
                    && marker_native_code(payload, offset).is_some()
                    && matches!(locus, [0x04, 0x00, 0x02, 0x00] | [0x05, 0x00, 0x01, 0x00])
                    && marker_profile_curve_role(payload, offset) == Some(1)
                    && marker_object_index(payload, offset).is_some()
                    && payload.get(offset + 56..offset + 58) == Some(&[0x0e, 0x00]) =>
            {
                (offset.checked_add(58)?, false)
            }
            prefix
                if prefix == LEGACY_SKETCH_MARKER
                    && marker_native_code(payload, offset) == Some(3)
                    && matches!(locus, [0x04, 0x00, 0x02, 0x00] | [0x05, 0x00, 0x01, 0x00])
                    && marker_object_index(payload, offset).is_some()
                    && payload.get(offset + 56..offset + 58) == Some(&[0x0e, 0x00]) =>
            {
                (offset.checked_add(58)?, false)
            }
            prefix
                if prefix == LEGACY_SKETCH_MARKER
                    && matches!(marker_native_code(payload, offset), Some(0 | 2 | 3))
                    && matches!(locus, [0x04, 0x00, 0x02, 0x00] | [0x05, 0x00, 0x01, 0x00])
                    && marker_object_index(payload, offset).is_some()
                    && payload.get(
                        offset + spatial_pre::COORDINATE_TAG..offset + spatial_pre::COORDINATES,
                    ) == Some(&[0x0e, 0x00]) =>
            {
                (offset.checked_add(spatial_pre::COORDINATES)?, false)
            }
            prefix
                if prefix == LEGACY_EXTENDED_SKETCH_MARKER
                    && matches!(
                        (marker_native_code(payload, offset), locus),
                        (Some(1), [0x04, 0x00, 0x02, 0x00]) | (Some(0), [0x05, 0x00, 0x01, 0x00])
                    )
                    && payload.get(offset + 56..offset + 58) == Some(&[0x0e, 0x00]) =>
            {
                (offset.checked_add(58)?, true)
            }
            prefix
                if prefix == LEGACY_EXTENDED_SKETCH_MARKER
                    && marker_native_code(payload, offset) == Some(3)
                    && matches!(locus, [0x04, 0x00, 0x02, 0x00] | [0x05, 0x00, 0x01, 0x00])
                    && marker_object_index(payload, offset).is_some()
                    && payload.get(offset + 56..offset + 58) == Some(&[0x0e, 0x00]) =>
            {
                (offset.checked_add(58)?, false)
            }
            prefix
                if prefix == LEGACY_EXTENDED_SKETCH_MARKER
                    && marker_native_code(payload, offset) == Some(1)
                    && locus == [0x04, 0x00, 0x02, 0x00]
                    && payload.get(
                        offset + spatial_pre::COORDINATE_TAG..offset + spatial_pre::COORDINATES,
                    ) == Some(&[0x0e, 0x00]) =>
            {
                (offset.checked_add(spatial_pre::COORDINATES)?, true)
            }
            prefix
                if prefix == LEGACY_EXTENDED_SKETCH_MARKER
                    && marker_native_code(payload, offset) == Some(0)
                    && locus == [0x05, 0x00, 0x01, 0x00]
                    && marker_object_index(payload, offset).is_some()
                    && payload.get(
                        offset + spatial_pre::COORDINATE_TAG..offset + spatial_pre::COORDINATES,
                    ) == Some(&[0x0e, 0x00]) =>
            {
                (offset.checked_add(spatial_pre::COORDINATES)?, true)
            }
            _ => return None,
        };
    (!requires_profile_role || marker_profile_curve_role(payload, offset) == Some(1))
        .then_some(coordinate_offset)
}

fn current_indexed_spatial_xyz_point(payload: &[u8], offset: usize) -> bool {
    const NEXT_MARKER_OFFSETS: [usize; 2] = [158, 162];

    let continuation_tail = View::u16_le_at(payload, offset + spatial_xyz::TAIL_WORD_0) == Some(8)
        && View::u16_le_at(payload, offset + spatial_xyz::TAIL_WORD_1) == Some(1);
    let terminal_geometry_locus = View::u32_le_at(payload, offset + spatial_xyz::NATIVE_KIND)
        == Some(0)
        && payload.get(offset + spatial_xyz::PROFILE_LOCUS..offset + spatial_xyz::PROFILE_ROLE)
            == Some(&[0x05, 0x00, 0x01, 0x00]);
    let terminal_tail = terminal_geometry_locus
        && View::u16_le_at(payload, offset + spatial_xyz::TAIL_WORD_0) == Some(1)
        && View::u16_le_at(payload, offset + spatial_xyz::TAIL_WORD_1) == Some(0);

    current_indexed_spatial_xyz_common(payload, offset)
        && (continuation_tail || terminal_tail)
        && payload.get(offset + spatial_xyz::TAIL_ZERO..offset + spatial_xyz::TERMINATOR)
            == Some(&[0; 6])
        && payload.get(offset + spatial_xyz::TERMINATOR..offset + spatial_xyz::TERMINATOR + 4)
            == Some(&[0xfe, 0xff, 0xff, 0xff])
        && ((continuation_tail
            && NEXT_MARKER_OFFSETS.iter().any(|relative| {
                offset
                    .checked_add(*relative)
                    .is_some_and(|next| sketch_marker_prefix_at(payload, next))
            }))
            || (terminal_tail && current_indexed_spatial_xyz_terminal_tail(payload, offset)))
}

fn current_indexed_spatial_xyz_common(payload: &[u8], offset: usize) -> bool {
    let kind_locus = matches!(
        (
            View::u32_le_at(payload, offset + spatial_xyz::NATIVE_KIND),
            payload.get(offset + spatial_xyz::PROFILE_LOCUS..offset + spatial_xyz::PROFILE_ROLE),
        ),
        (Some(0 | 1), Some([0x04, 0x00, 0x02, 0x00])) | (Some(0), Some([0x05, 0x00, 0x01, 0x00]))
    );
    payload.get(offset..offset + spatial_xyz::HEADER) == Some(SKETCH_MARKER)
        && payload.get(offset + spatial_xyz::HEADER..offset + spatial_xyz::SENTINEL)
            == Some(&[0xff; 8])
        && payload.get(offset + spatial_xyz::SENTINEL..offset + spatial_xyz::NATIVE_KIND)
            == Some(&(-1.0f32).to_le_bytes())
        && kind_locus
        && View::u16_le_at(payload, offset + spatial_xyz::PROFILE_ROLE) == Some(1)
        && payload.get(offset + spatial_xyz::PROFILE_ROLE + 2..offset + spatial_xyz::SELECTOR)
            == Some(&[0; 2])
        && payload.get(offset + spatial_xyz::SELECTOR..offset + spatial_xyz::SELECTOR + 8)
            == Some(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00])
        && payload.get(offset + spatial_xyz::SELECTOR + 8..offset + spatial_xyz::STATE_VALUE)
            == Some(&[0; 9])
        && View::f64_le_at(payload, offset + spatial_xyz::STATE_VALUE) == Some(1.0)
        && payload.get(offset + spatial_xyz::COORDINATE_TAG..offset + spatial_xyz::COORDINATES)
            == Some(&[0x0e, 0x00])
        && marker_object_index(payload, offset).is_some()
}

fn current_indexed_spatial_relation_coordinates(payload: &[u8], offset: usize) -> Option<Point3> {
    const NEXT_MARKER_OFFSETS: [usize; 2] = [158, 162];

    (current_indexed_spatial_xyz_common(payload, offset)
        && View::u32_le_at(payload, offset + spatial_xyz::NATIVE_KIND) == Some(1)
        && payload.get(offset + spatial_xyz::PROFILE_LOCUS..offset + spatial_xyz::PROFILE_ROLE)
            == Some(&[0x04, 0x00, 0x02, 0x00])
        && View::u16_le_at(payload, offset + spatial_xyz::TAIL_WORD_0) == Some(1)
        && View::u16_le_at(payload, offset + spatial_xyz::TAIL_WORD_1) == Some(0)
        && payload.get(offset + spatial_xyz::TAIL_ZERO..offset + spatial_xyz::TERMINATOR)
            == Some(&[0; 6])
        && payload.get(offset + spatial_xyz::TERMINATOR..offset + spatial_xyz::TERMINATOR + 4)
            == Some(&[0xfe, 0xff, 0xff, 0xff])
        && NEXT_MARKER_OFFSETS.iter().any(|relative| {
            offset
                .checked_add(*relative)
                .is_some_and(|next| sketch_marker_prefix_at(payload, next))
        }))
    .then(|| marker_spatial_point(payload, offset + spatial_xyz::COORDINATES))
    .flatten()
}

fn current_indexed_spatial_xyz_terminal_tail(payload: &[u8], offset: usize) -> bool {
    const CONTROL_SEQUENCE: [u8; 24] = [
        0x00, 0x00, 0xff, 0xfe, 0xff, 0x00, 0xff, 0xff, 0x00, 0x00, 0x80, 0xbf, 0xff, 0xff, 0xff,
        0xff, 0x01, 0x00, 0x00, 0x00, 0xff, 0xff, 0xff, 0xff,
    ];

    [
        (
            spatial_xyz_terminal_short::TERMINAL_TAG,
            spatial_xyz_terminal_short::ZERO_ALIGNMENT_SUFFIX,
            spatial_xyz_terminal_short::TABLE_HEADER,
            spatial_xyz_terminal_short::FIRST_COUNT,
            spatial_xyz_terminal_short::SECOND_COUNT,
            spatial_xyz_terminal_short::ONE_RUN,
            spatial_xyz_terminal_short::ZERO_AFTER_ONE_RUN,
            spatial_xyz_terminal_short::ONE_AFTER_ZERO,
            spatial_xyz_terminal_short::ZERO_BEFORE_CONTROL,
            spatial_xyz_terminal_short::CONTROL_SEQUENCE,
        ),
        (
            spatial_xyz_terminal_long::TERMINAL_TAG,
            spatial_xyz_terminal_long::ZERO_ALIGNMENT_SUFFIX,
            spatial_xyz_terminal_long::TABLE_HEADER,
            spatial_xyz_terminal_long::FIRST_COUNT,
            spatial_xyz_terminal_long::SECOND_COUNT,
            spatial_xyz_terminal_long::ONE_RUN,
            spatial_xyz_terminal_long::ZERO_AFTER_ONE_RUN,
            spatial_xyz_terminal_long::ONE_AFTER_ZERO,
            spatial_xyz_terminal_long::ZERO_BEFORE_CONTROL,
            spatial_xyz_terminal_long::CONTROL_SEQUENCE,
        ),
    ]
    .into_iter()
    .any(
        |(
            terminal_tag,
            zero_alignment_suffix,
            table_header,
            first_count,
            second_count,
            one_run,
            zero_after_one_run,
            one_after_zero,
            zero_before_control,
            control_sequence,
        )| {
            payload.get(offset + spatial_xyz::TERMINATOR + 4..offset + terminal_tag)
                == Some(&[0; 124])
                && payload.get(offset + terminal_tag..offset + terminal_tag + 2)
                    == Some(&[0x08, 0x80])
                && payload
                    .get(offset + zero_alignment_suffix..offset + table_header)
                    .is_some_and(|bytes| bytes.iter().all(|byte| *byte == 0))
                && payload.get(offset + table_header..offset + table_header + 4)
                    == Some(&[0x01, 0x00, 0x01, 0x00])
                && View::u32_le_at(payload, offset + first_count) == Some(1)
                && View::u32_le_at(payload, offset + second_count) == Some(2)
                && payload
                    .get(offset + one_run..offset + zero_after_one_run)
                    .is_some_and(|values| {
                        values
                            .chunks_exact(4)
                            .all(|value| value == [0x01, 0x00, 0x00, 0x00])
                    })
                && View::u32_le_at(payload, offset + zero_after_one_run) == Some(0)
                && View::u32_le_at(payload, offset + one_after_zero) == Some(1)
                && payload.get(offset + zero_before_control..offset + control_sequence)
                    == Some(&[0; 6])
                && payload.get(offset + control_sequence..offset + control_sequence + 24)
                    == Some(&CONTROL_SEQUENCE)
        },
    )
}

fn marker_spatial_coordinates(payload: &[u8], offset: usize) -> Option<Point3> {
    let coordinate_offset = marker_spatial_coordinate_offset(payload, offset)?;
    marker_spatial_point(payload, coordinate_offset)
}

fn marker_spatial_point(payload: &[u8], coordinate_offset: usize) -> Option<Point3> {
    const NATIVE_TO_IR: f64 = 1000.0;
    let coordinate = |offset: usize| {
        let value = View::f64_le_at(payload, offset)?;
        (value == 0.0 || value.is_normal()).then_some(value * NATIVE_TO_IR)
    };
    Some(Point3::new(
        coordinate(coordinate_offset)?,
        coordinate(coordinate_offset + 8)?,
        coordinate(coordinate_offset + 16)?,
    ))
}

/// Read the guarded 3D coordinate layouts used by spatial-relation markers.
///
/// The established indexed layout is shared with ordinary spatial points. A
/// relation can also carry a source point in the current indexed layout or in
/// one of the older profile-locus layouts. Relation-owned records retain their
/// relation kind in the input roster; this function only exposes their model
/// coordinates to relation and spatial-sketch projection. Require the complete
/// record boundary so an incidental byte pattern cannot become geometry.
pub(super) fn spatial_relation_marker_coordinates(payload: &[u8], offset: usize) -> Option<Point3> {
    if let Some(point) = marker_spatial_coordinates(payload, offset) {
        return Some(point);
    }
    if let Some(point) = current_indexed_spatial_relation_coordinates(payload, offset) {
        return Some(point);
    }
    let code = marker_native_code(payload, offset)?;
    let supported_prefix = payload.get(offset..offset + SKETCH_MARKER.len()) == Some(SKETCH_MARKER)
        || payload.get(offset..offset + LEGACY_EXTENDED_SKETCH_MARKER.len())
            == Some(LEGACY_EXTENDED_SKETCH_MARKER);
    if !(code == 0 || matches!(code, 2..=5))
        || !supported_prefix
        || !matches!(
            payload.get(offset + 23..offset + 27),
            Some([0x04, 0x00, 0x02, 0x00] | [0x05, 0x00, 0x01, 0x00])
        )
        || View::u16_le_at(payload, offset + 27) != Some(1)
    {
        return None;
    }
    let tagged_offsets = [56usize, 64]
        .into_iter()
        .filter_map(|relative| {
            let tag = offset.checked_add(relative)?;
            (payload.get(tag..tag + 2) == Some(&[0x0e, 0x00])).then_some(tag.checked_add(2)?)
        })
        .collect::<Vec<_>>();
    let [coordinate_offset] = tagged_offsets.as_slice() else {
        return None;
    };
    if !(0..3).all(|index| {
        View::f64_le_at(payload, coordinate_offset + index * 8)
            .is_some_and(|value| value == 0.0 || value.is_normal())
    }) {
        return None;
    }
    let coordinate =
        |relative| Some(View::f64_le_at(payload, coordinate_offset + relative)? * 1000.0);
    Some(Point3::new(coordinate(0)?, coordinate(8)?, coordinate(16)?))
}

pub(super) fn spatial_vertex_coordinates_charged(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
) -> Result<Vec<FinitePoint3>, CodecError> {
    let offsets = spatial_vertex_offsets_charged(ctx, payload)?;
    spatial_vertices_at(ctx, payload, &offsets)
}

/// The finite coordinates at spatial vertex offsets, skipping an offset whose
/// coordinates are not finite.
fn spatial_vertices_at(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
    offsets: &[usize],
) -> Result<Vec<FinitePoint3>, CodecError> {
    let mut vertices = Vec::new();
    for &offset in ctx.admit_iter(offsets, "decode SLDPRT spatial vertices")? {
        let Some(point) = View::f64_le_at(payload, offset + 45)
            .zip(View::f64_le_at(payload, offset + 53))
            .zip(View::f64_le_at(payload, offset + 61))
            .map(|((x, y), z)| Point3::new(x, y, z))
            .and_then(FinitePoint3::new)
        else {
            continue;
        };
        ctx.push_vec(&mut vertices, point, "collect SLDPRT spatial vertices")?;
    }
    Ok(vertices)
}

pub(super) fn spatial_vertex_offsets_charged(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
) -> Result<Vec<usize>, CodecError> {
    let mut offsets = Vec::new();
    for (offset, bytes) in ctx
        .admit_iter(payload, "scan SLDPRT spatial vertices")?
        .windows(const { crate::nonzero(SPATIAL_VERTEX_PREFIX.len()) })
        .enumerate()
    {
        if bytes == SPATIAL_VERTEX_PREFIX
            && payload.get(offset + 43..offset + 45) == Some(&[0x0e, 0x00])
        {
            ctx.push_vec(
                &mut offsets,
                offset,
                "collect SLDPRT spatial vertex offsets",
            )?;
        }
    }
    Ok(offsets)
}

/// Admit every sketch marker in a retained feature-input payload.
///
/// A marker is a record boundary: once [`sketch_marker_at`] recognizes it,
/// failure to read its native code, ordinal, or checked identity is a
/// malformed lane. Returning an error keeps the marker from disappearing
/// through an iterator's `filter_map`.
pub(super) fn admit_sketch_input_entities(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
    parent: &str,
) -> Result<Vec<SketchInputEntity>, cadmpeg_core::CodecError> {
    let lane_key = ctx
        .rsplit_once(parent, "#", "split SLDPRT feature-input parent")?
        .map_or(parent, |(_, key)| key);
    let Some(last_start) = payload.len().checked_sub(SKETCH_MARKER.len() - 1) else {
        return Ok(Vec::new());
    };
    let Some(marker_starts) = payload.get(..last_start) else {
        return Ok(Vec::new());
    };
    ctx.admit_iter(marker_starts, "scan SLDPRT sketch marker offsets")?
        .enumerate()
        .filter(|(offset, _)| sketch_marker_at(payload, *offset))
        .map(|(offset, _)| offset)
        .enumerate()
        .try_fold(Vec::new(), |mut entities, (ordinal, offset)| {
            let Some(code) = marker_native_code(payload, offset) else {
                return Err(CodecError::Malformed(ctx.format_retained(format_args!("SolidWorks feature-input marker at byte {offset} has no native code"), "report SLDPRT marker native code")?));
            };
            let linked_point = linked_profile_point(payload, offset);
            let legacy_alternate_profile_point =
                legacy_geometry_locus_alternate_profile_point_coordinates(payload, offset);
            let extended_profile_point = extended_profile_point_coordinates(payload, offset);
            let additional_linked_profile_point =
                additional_linked_profile_point_coordinates(payload, offset);
            let extended_four_link_profile_point =
                extended_four_link_profile_point_coordinates(payload, offset);
            let compact_profile_point =
                legacy_extended_linked_profile_point_coordinates(payload, offset);
            let single_incidence_profile_point =
                legacy_single_incidence_profile_point_coordinates(payload, offset);
            let legacy_144_profile_point_variant =
                legacy_144_profile_point_variant_coordinates(payload, offset);
            let legacy_140_profile_point_variant =
                legacy_140_profile_point_variant_coordinates(payload, offset);
            let packed_profile_point =
                packed_legacy_linked_profile_point_coordinates(payload, offset);
            let compact_code_two_profile_point =
                compact_legacy_code_two_profile_point_coordinates(payload, offset);
            let compact_legacy_profile_point =
                compact_legacy_linked_profile_point_coordinates(payload, offset);
            let terminal_profile_point =
                terminal_extended_profile_point_coordinates(payload, offset);
            let compact_geometry_locus_point =
                compact_geometry_locus_point_coordinates(payload, offset);
            let shifted_geometry_handle = shifted_geometry_handle_coordinates(payload, offset);
            let shifted_geometry_locus = shifted_geometry_handle
                .or_else(|| shifted_geometry_locus_coordinates(payload, offset));
            let inline_arc = inline_arc_coordinates(payload, offset);
            let mut coordinates_m = linked_point
                .map(|(coordinates, _)| coordinates)
                .or(legacy_alternate_profile_point)
                .or(extended_profile_point)
                .or(additional_linked_profile_point)
                .or(extended_four_link_profile_point)
                .or(compact_profile_point)
                .or(single_incidence_profile_point)
                .or(legacy_144_profile_point_variant)
                .or(legacy_140_profile_point_variant)
                .or(packed_profile_point)
                .or(compact_code_two_profile_point)
                .or(compact_legacy_profile_point)
                .or(terminal_profile_point)
                .or(compact_geometry_locus_point)
                .or(shifted_geometry_locus)
                .or_else(|| inline_arc.map(|[center, _, _]| center));
            if coordinates_m.is_none() {
                coordinates_m = marker_coordinates(payload, offset);
            }
            let kind = if slot_curve_and_center_indices(payload, offset).is_some() {
                SketchInputKind::from_handle_code(code)
            } else if inline_arc.is_some() {
                SketchInputKind::Arc
            } else if marker_spatial_coordinates(payload, offset).is_some()
                || legacy_declared_handle_coordinates(payload, offset).is_some()
                || legacy_alternate_profile_point.is_some()
                || extended_profile_point.is_some()
                || additional_linked_profile_point.is_some()
                || extended_four_link_profile_point.is_some()
                || compact_profile_point.is_some()
                || single_incidence_profile_point.is_some()
                || legacy_144_profile_point_variant.is_some()
                || legacy_140_profile_point_variant.is_some()
                || packed_profile_point.is_some()
                || compact_code_two_profile_point.is_some()
                || compact_legacy_profile_point.is_some()
                || terminal_profile_point.is_some()
                || compact_geometry_locus_point.is_some()
                || shifted_geometry_handle.is_some()
                || linked_point.is_some()
                || coordinates_m.is_some()
                    && (compact_legacy_profile_vertex(payload, offset)
                        || packed_legacy_profile_vertex(payload, offset)
                        || indexed_profile_vertex(payload, offset)
                        || current_geometry_locus_profile_vertex(payload, offset)
                        || terminal_wide_geometry_locus_profile_vertex(payload, offset)
                        || extended_geometry_locus_single_link_point(payload, offset)
                        || geometry_locus_profile_vertex(payload, offset)
                        || compact_linked_profile_vertex(payload, offset)
                        || linked_profile_vertex(payload, offset))
            {
                SketchInputKind::Point
            } else if current_geometry_locus_profile_line(payload, offset, code)
                || current_compact_104_profile_line(payload, offset)
                || current_direct_92_profile_line_endpoint_indices(payload, offset).is_some()
                || extended_terminal_profile_line(payload, offset)
                || extended_identity_inline_line_record(payload, offset)
                || legacy_compact_profile_line(payload, offset)
                || compact_legacy_90_geometry_line_roster_indices(payload, offset).is_some()
                || coordinates_m.is_none()
                    && (marker_is_selected_construction_line(payload, offset)
                        || compact_curve_endpoint_indices(payload, offset).is_some())
            {
                SketchInputKind::LineOrCircle
            } else if legacy_referenced_wide_arc_endpoint_indices(payload, offset).is_some() {
                SketchInputKind::Arc
            } else if coordinates_m.is_none()
                && payload.get(offset + 60..offset + 64) == Some(&1u32.to_le_bytes())
                && payload.get(offset..offset + LEGACY_EXTENDED_SKETCH_MARKER.len())
                    == Some(LEGACY_EXTENDED_SKETCH_MARKER)
            {
                legacy_extended_profile_curve_kind(payload, offset).unwrap_or_else(|| match code {
                    0 => SketchInputKind::Arc,
                    1 | 2 => SketchInputKind::LineOrCircle,
                    _ => SketchInputKind::from_native_code_and_layout(code, false),
                })
            } else if coordinates_m.is_none()
                && payload.get(offset..offset + LEGACY_SKETCH_MARKER.len())
                    == Some(LEGACY_SKETCH_MARKER)
                && matches!(code, 0..=2)
                && matches!(marker_profile_curve_role(payload, offset), Some(1 | 2))
                && compact_indexed_curve_endpoint_indices(payload, offset).is_none()
            {
                SketchInputKind::LineOrCircle
            } else if wide_indexed_curve_endpoint_indices(payload, offset).is_some() {
                match code {
                    0 | 1 => SketchInputKind::LineOrCircle,
                    2 => SketchInputKind::Arc,
                    _ => SketchInputKind::from_native_code_and_layout(code, false),
                }
            } else if compact_indexed_curve_endpoint_indices(payload, offset).is_some() {
                match code {
                    0 | 1 => SketchInputKind::LineOrCircle,
                    2 => SketchInputKind::Arc,
                    _ => SketchInputKind::from_native_code_and_layout(code, false),
                }
            } else if alternate_current_curve_body(payload, offset) {
                match code {
                    0 => SketchInputKind::LineOrCircle,
                    2 => SketchInputKind::Arc,
                    _ => SketchInputKind::from_native_code_and_layout(code, false),
                }
            } else {
                SketchInputKind::from_native_code_and_layout(code, coordinates_m.is_some())
            };
            let Ok(ordinal) = u32::try_from(ordinal) else {
                    return Err(CodecError::Malformed(ctx.format_retained(format_args!("SolidWorks feature-input lane {parent} has more than u32::MAX sketch markers"), "report SLDPRT marker count")?));
            };
            let id = ctx.format_retained(format_args!("sldprt:feature-input:sketch-entity#{lane_key}:{offset}"), "retain SLDPRT sketch marker identity")?;
            let mut parent_copy = String::new();
            ctx.append_retained(
                &mut parent_copy,
                parent,
                "retain SLDPRT sketch marker parent",
            )?;
            let offset_u64 = u64::try_from(offset).map_err(|_| {
                ctx.refuse_codec_limit("address SLDPRT sketch marker", u64::MAX - 1, u64::MAX)
            })?;
            let mut entity = match SketchInputEntity::try_new(
                id,
                parent_copy,
                ordinal,
                offset_u64,
                kind,
                payload,
            ) {
                Ok(entity) => entity,
                Err(error) => {
                    return Err(CodecError::Malformed(ctx.format_retained(format_args!("SolidWorks feature-input lane {parent} marker at byte {offset}: {error}"), "report SLDPRT marker construction")?));
                }
            };
            entity.state_value = marker_state_value(payload, offset);
            entity.coordinates_m = coordinates_m;
            ctx.reserve_vec(&mut entities, 1, "collect SLDPRT sketch markers")?;
            entities.push(entity);
            Ok(entities)
        })
}

#[cfg(test)]
pub(super) fn sketch_input_entities(payload: &[u8], parent: &str) -> Vec<SketchInputEntity> {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(
        payload,
        &arena,
        &cadmpeg_core::decode::DecodePolicy::service(),
    )
    .expect("synthetic sketch marker payload fits service policy");
    admit_sketch_input_entities(&ctx, payload, parent).expect("synthetic sketch marker payload")
}

fn current_geometry_locus_profile_line(payload: &[u8], offset: usize, code: u32) -> bool {
    code == 2
        && payload.get(offset..offset + SKETCH_MARKER.len()) == Some(SKETCH_MARKER)
        && marker_is_geometry_locus(payload, offset)
        && marker_profile_curve_role(payload, offset) == Some(1)
}

pub(crate) fn sketch_marker_at(payload: &[u8], offset: usize) -> bool {
    if !sketch_marker_prefix_at(payload, offset) {
        return false;
    }
    let shared_geometry_body = payload.get(offset + 5..offset + 13) == Some(&[0xff; 8])
        && payload.get(offset + 13..offset + 17) == Some(&[0x00, 0x00, 0x80, 0xbf])
        && payload
            .get(offset + 35..offset + 39)
            .is_some_and(|state| state[0] == 0 && state[1] == 0 && state[3] == 0);
    shared_geometry_body
        || compact_legacy_marker_body(payload, offset)
        || packed_legacy_marker_body(payload, offset)
        || compact_legacy_code_two_profile_point_coordinates(payload, offset).is_some()
        || extended_geometry_locus_construction_line_endpoint_indices(payload, offset).is_some()
        || alternate_current_curve_body(payload, offset)
}

pub(super) fn alternate_current_curve_body(payload: &[u8], offset: usize) -> bool {
    let role = marker_profile_curve_role(payload, offset);
    let header = payload.get(offset + 5..offset + 13);
    let supported_header = match role {
        Some(1) => matches!(
            header,
            Some(bytes)
                if bytes == [0xff; 8]
                    || bytes == [0xff, 0xff, 0xff, 0xff, 0x00, 0x00, 0xff, 0xff]
        ),
        Some(2) => header == Some(&[0xff, 0xff, 0xff, 0xff, 0x04, 0x00, 0xff, 0xff]),
        _ => false,
    };
    payload.get(offset..offset + SKETCH_MARKER.len()) == Some(SKETCH_MARKER)
        && supported_header
        && payload.get(offset + 13..offset + 17) == Some(&[0x00, 0x00, 0x80, 0xbf])
        && View::u32_le_at(payload, offset + 17).is_some_and(|code| matches!(code, 0 | 2))
        && marker_is_geometry_locus(payload, offset)
        && payload.get(offset + 29..offset + 31)
            == Some(&if role == Some(1) { [1, 0] } else { [0, 0] })
        && payload.get(offset + 31..offset + 35) == Some(&[0x00, 0x00, 0x80, 0xbf])
        && View::u32_le_at(payload, offset + 35).is_some_and(|state| state != 0)
        && payload.get(offset + 48..offset + 56) == Some(&1.0f64.to_le_bytes())
        && payload.get(offset + 60..offset + 64)
            == Some(&if role == Some(1) {
                1u32.to_le_bytes()
            } else {
                0u32.to_le_bytes()
            })
        && payload.get(offset + 64..offset + 72) == Some(&(-1.0f64).to_le_bytes())
        && payload.get(offset + 72..offset + 76) == Some(&0u32.to_le_bytes())
        && offset
            .checked_add(84)
            .is_some_and(|next| sketch_marker_prefix_at(payload, next))
}

pub(super) fn compact_legacy_marker_body(payload: &[u8], offset: usize) -> bool {
    let locus = payload.get(offset + 19..offset + 23);
    payload.get(offset..offset + LEGACY_SKETCH_MARKER.len()) == Some(LEGACY_SKETCH_MARKER)
        && (payload.get(offset + 5..offset + 13) == Some(&[0xff; 8])
            || payload.get(offset + 5..offset + 13)
                == Some(&[0xff, 0xff, 0xff, 0xff, 0x04, 0x00, 0xff, 0xff]))
        && View::u32_le_at(payload, offset + 13).is_some_and(|code| matches!(code, 0 | 1))
        && payload.get(offset + 17..offset + 19) == Some(&[0; 2])
        && matches!(
            locus,
            Some([0x04, 0x00, 0x02, 0x00] | [0x05, 0x00, 0x01, 0x00])
        )
        && View::u16_le_at(payload, offset + 23).is_some_and(|role| matches!(role, 1 | 2))
        && payload.get(offset + 27..offset + 31) == Some(&[0; 4])
        && matches!(payload.get(offset + 31), Some(0x04 | 0x0c))
}

pub(super) fn packed_legacy_marker_body(payload: &[u8], offset: usize) -> bool {
    payload.get(offset..offset + LEGACY_SKETCH_MARKER.len()) == Some(LEGACY_SKETCH_MARKER)
        && payload.get(offset + 5..offset + 13) == Some(&[0xff; 8])
        && View::u32_le_at(payload, offset + 13).is_some_and(|code| code <= 3)
        && payload.get(offset + 17..offset + 19) == Some(&[0; 2])
        && matches!(
            payload.get(offset + 19..offset + 23),
            Some([0x04, 0x00, 0x02, 0x00] | [0x05, 0x00, 0x01, 0x00])
        )
        && View::u16_le_at(payload, offset + 23).is_some_and(|role| matches!(role, 1 | 2))
        && payload.get(offset + 27..offset + 29) == Some(&[0; 2])
        && matches!(payload.get(offset + 29), Some(0x04 | 0x05 | 0x0c | 0x44))
        && payload.get(offset + 40..offset + 48) == Some(&1.0f64.to_le_bytes())
}

pub(super) fn marker_native_code(payload: &[u8], offset: usize) -> Option<u32> {
    let relative = if compact_legacy_marker_body(payload, offset)
        || packed_legacy_marker_body(payload, offset)
        || compact_legacy_code_two_profile_point_coordinates(payload, offset).is_some()
    {
        13
    } else {
        17
    };
    View::u32_le_at(payload, offset + relative)
}

pub(super) fn sketch_marker_prefix_at(payload: &[u8], offset: usize) -> bool {
    let marker = payload.get(offset..offset + SKETCH_MARKER.len());
    marker == Some(SKETCH_MARKER)
        || marker == Some(LEGACY_SKETCH_MARKER)
        || marker == Some(LEGACY_EXTENDED_SKETCH_MARKER)
}

fn compact_spatial_point_boundary(payload: &[u8], offset: usize) -> bool {
    let Some(end) = offset.checked_add(compact_spatial::LEN) else {
        return false;
    };
    end == payload.len()
        || sketch_marker_prefix_at(payload, end)
        || end
            .checked_add(4)
            .is_some_and(|next| sketch_marker_prefix_at(payload, next))
}

/// Recognize the short current-prefix point that declares an embedded
/// `sgArcHandle` child. The following marker boundary is part of the form:
/// a zero-based marker roster index precedes the next sketch marker.
pub(super) fn current_geometry_locus_arc_handle_point(payload: &[u8], offset: usize) -> bool {
    let common = payload.get(offset..offset + current_arc_handle::HEADER) == Some(SKETCH_MARKER)
        && payload
            .get(offset + current_arc_handle::HEADER..offset + current_arc_handle::SHARED_SELECTOR)
            == Some(&[0xff; 8])
        && payload.get(
            offset + current_arc_handle::SHARED_SELECTOR..offset + current_arc_handle::NATIVE_KIND,
        ) == Some(&[0x00, 0x00, 0x80, 0xbf])
        && View::u32_le_at(payload, offset + current_arc_handle::NATIVE_KIND)
            == Some(current_arc_handle::NATIVE_KIND_VALUE)
        && payload.get(
            offset + current_arc_handle::ZERO_LOCUS_PREFIX
                ..offset + current_arc_handle::GEOMETRY_LOCUS,
        ) == Some(&[0; 2])
        && payload
            .get(offset + current_arc_handle::GEOMETRY_LOCUS..offset + current_arc_handle::ROLE)
            == Some(&[0x05, 0x00, 0x01, 0x00])
        && View::u16_le_at(payload, offset + current_arc_handle::ROLE)
            == Some(current_arc_handle::ROLE_VALUE)
        && payload
            .get(offset + current_arc_handle::ZERO_STATE..offset + current_arc_handle::SELECTOR)
            == Some(&[0; 2])
        && payload.get(
            offset + current_arc_handle::SELECTOR
                ..offset + current_arc_handle::ZERO_BEFORE_STATE_VALUE,
        ) == Some(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00])
        && payload.get(
            offset + current_arc_handle::ZERO_BEFORE_STATE_VALUE
                ..offset + current_arc_handle::STATE_VALUE,
        ) == Some(&[0; 9])
        && View::f64_le_at(payload, offset + current_arc_handle::STATE_VALUE)
            == Some(current_arc_handle::STATE_VALUE_VALUE)
        && payload.get(
            offset + current_arc_handle::ZERO_BEFORE_COORDINATE
                ..offset + current_arc_handle::COORDINATE_TAG,
        ) == Some(&[0; 8])
        && payload.get(
            offset + current_arc_handle::COORDINATE_TAG
                ..offset + current_arc_handle::COORDINATE_FIRST,
        ) == Some(&[0x1e, 0x00])
        && finite_coordinate_pair(payload, offset + current_arc_handle::COORDINATE_FIRST).is_some()
        && payload.get(
            offset + current_arc_handle::HANDLE_PREFIX..offset + current_arc_handle::CLASS_MARKER,
        ) == Some(&[0x02, 0x00, 0x02, 0x00])
        && payload.get(
            offset + current_arc_handle::CLASS_MARKER..offset + current_arc_handle::CLASS_LENGTH,
        ) == Some(&[0xff, 0xff, 0x01, 0x00])
        && View::u16_le_at(payload, offset + current_arc_handle::CLASS_LENGTH)
            == Some(current_arc_handle::CLASS_LENGTH_VALUE)
        && payload
            .get(offset + current_arc_handle::CLASS_NAME..offset + current_arc_handle::HANDLE_ID)
            == Some(b"sgArcHandle")
        && View::u16_le_at(payload, offset + current_arc_handle::HANDLE_ID)
            .is_some_and(|id| id != u16::MAX)
        && payload.get(
            offset + current_arc_handle::REFERENCE_SENTINEL
                ..offset + current_arc_handle::ZERO_REFERENCE_TAIL,
        ) == Some(&[0xff; 4])
        && payload.get(
            offset + current_arc_handle::ZERO_REFERENCE_TAIL
                ..offset + current_arc_handle::TERMINATOR,
        ) == Some(&[0; 8])
        && payload.get(
            offset + current_arc_handle::TERMINATOR..offset + current_arc_handle::ZERO_TRAILER,
        ) == Some(&[0xfe, 0xff, 0xff, 0xff]);
    if !common {
        return false;
    }
    let following_index_is_valid = |index: usize, marker: usize| {
        View::u32_le_at(payload, offset + index).is_some_and(|value| value != u32::MAX)
            && sketch_marker_prefix_at(payload, offset + marker)
    };
    let ordinary = payload.get(
        offset + current_arc_handle::ZERO_TRAILER
            ..offset + current_arc_handle::FOLLOWING_OBJECT_INDEX,
    ) == Some(&[0; 42])
        && following_index_is_valid(
            current_arc_handle::FOLLOWING_OBJECT_INDEX,
            current_arc_handle::LEN,
        );
    let terminal = payload.get(
        offset + current_arc_handle::ZERO_TRAILER
            ..offset + current_arc_handle_terminal::FOLLOWING_OBJECT_INDEX,
    ) == Some(&[0; 46])
        && following_index_is_valid(
            current_arc_handle_terminal::FOLLOWING_OBJECT_INDEX,
            current_arc_handle_terminal::LEN,
        );
    ordinary || terminal
}

pub(crate) fn relation_bindings_charged(
    ctx: &DecodeContext<'_>,
    parent: &str,
    classes: &[FeatureInputClass],
    scalars: &[FeatureInputScalar],
) -> Result<Vec<FeatureInputRelationBinding>, CodecError> {
    relation_bindings_scoped(ctx, parent, classes, scalars, &[])
}

pub(super) fn relation_bindings_scoped(
    ctx: &DecodeContext<'_>,
    parent: &str,
    classes: &[FeatureInputClass],
    scalars: &[FeatureInputScalar],
    intervals: &[(u64, Option<u64>, String)],
) -> Result<Vec<FeatureInputRelationBinding>, CodecError> {
    const IDENTITY: &str = "retain SLDPRT reference identity";
    let lane_key = ctx
        .rsplit_once(parent, "#", "split SLDPRT relation parent")?
        .map_or(parent, |(_, key)| key);
    let candidates =
        unique_relation_declaration_candidates_charged(ctx, classes, scalars, intervals)?;
    let mut bindings = Vec::new();
    for &(class, scalar, family) in
        ctx.admit_iter(&candidates, "project SLDPRT relation binding candidates")?
    {
        let ordinal = u32::try_from(bindings.len()).map_err(|_| {
            ctx.refuse_codec_limit("number SLDPRT relation bindings", u64::MAX - 1, u64::MAX)
        })?;
        let id = ctx.format_retained(
            format_args!(
                "sldprt:feature-input:relation-binding#{lane_key}:{}",
                class.offset
            ),
            "retain SLDPRT relation binding identity",
        )?;
        let parent = ctx.copy_retained_text(parent, IDENTITY)?;
        let class_ref = ctx.copy_retained_text(&class.id, IDENTITY)?;
        let scalar_ref = ctx.copy_retained_text(&scalar.id, IDENTITY)?;
        let feature_ref = scalar
            .feature_ref
            .as_deref()
            .map(|feature| ctx.copy_retained_text(feature, IDENTITY))
            .transpose()?;
        ctx.reserve_vec(&mut bindings, 1, "collect SLDPRT relation bindings")?;
        bindings.push(FeatureInputRelationBinding {
            id,
            parent,
            ordinal,
            offset: class.offset,
            class_ref,
            family,
            scalar_ref,
            feature_ref,
        });
    }
    Ok(bindings)
}

pub(crate) fn reference_cells_charged(
    ctx: &DecodeContext<'_>,
    scalars: &[FeatureInputScalar],
    classes: &[FeatureInputClass],
) -> Result<Vec<FeatureInputReference>, CodecError> {
    const COLLECT: &str = "collect SLDPRT reference cells";
    const DECLARATIONS: &str = "match SLDPRT reference declarations";
    const IDENTITY: &str = "retain SLDPRT reference identity";
    let mut temporary_storage = ctx.reserve_scoped(0, "SLDPRT reference cell scratch")?;
    let mut sources = Vec::new();
    for scalar in ctx.admit_iter(scalars, COLLECT)? {
        for operand in ctx.admit_iter(&scalar.operands, COLLECT)? {
            temporary_storage
                .with_storage(|| ctx.push_vec(&mut sources, (scalar, operand), COLLECT))?;
        }
    }
    // The stable sort keeps the first cell the scalars state at each offset.
    ctx.stable_sort_by(
        &mut sources,
        |(_, operand)| &operand.offset,
        Ord::cmp,
        "sort SLDPRT reference cells",
    )?;
    ctx.dedup_by_key(
        &mut sources,
        |(_, operand)| Ok(operand.offset),
        "deduplicate SLDPRT reference cells",
    )?;
    // A class declares a cell 12 bytes after it in the same lane. Each operand
    // kind keeps its declaring class while every declaration names the same
    // class, and `None` once two classes disagree.
    let mut declarations = HashMap::<FeatureInputOperandKind, Option<&FeatureInputClass>>::new();
    if !sources.is_empty() {
        let mut classes_by_offset = HashMap::new();
        for class in ctx.admit_iter(classes, DECLARATIONS)? {
            temporary_storage.with_storage(|| {
                ctx.push_hash_group(
                    &mut classes_by_offset,
                    class.offset,
                    class,
                    DECLARATIONS,
                    DECLARATIONS,
                )
            })?;
        }
        for (scalar, operand) in ctx.admit_iter(&sources, DECLARATIONS)? {
            let Some(class_offset) = operand.offset.checked_add(12) else {
                continue;
            };
            let Some(candidates) =
                ctx.get_hash_map(&classes_by_offset, &class_offset, DECLARATIONS)?
            else {
                continue;
            };
            for class in ctx.admit_iter(candidates, DECLARATIONS)?.copied() {
                if !ctx.equal(
                    class.parent.as_str(),
                    scalar.parent.as_str(),
                    "compare SLDPRT reference declaration parents",
                )? {
                    continue;
                }
                match ctx.get_mut_hash_map(&mut declarations, &operand.kind, DECLARATIONS)? {
                    Some(declared) => {
                        if let Some(existing) = *declared {
                            if !ctx.equal(
                                existing.id.as_str(),
                                class.id.as_str(),
                                "compare SLDPRT reference declaration identities",
                            )? {
                                *declared = None;
                            }
                        }
                    }
                    None => {
                        temporary_storage.with_storage(|| {
                            ctx.insert_hash_map(
                                &mut declarations,
                                operand.kind,
                                Some(class),
                                DECLARATIONS,
                            )
                        })?;
                    }
                }
            }
        }
    }
    let mut cells = Vec::new();
    for (ordinal, (scalar, operand)) in ctx.admit_iter(&sources, COLLECT)?.enumerate() {
        let ordinal = u32::try_from(ordinal).map_err(|_| {
            ctx.refuse_codec_limit("number SLDPRT reference cells", u64::MAX - 1, u64::MAX)
        })?;
        let class_ref = match ctx.get_hash_map(
            &declarations,
            &operand.kind,
            "assign SLDPRT reference declarations",
        )? {
            Some(Some(class)) => Some(ctx.copy_retained_text(&class.id, IDENTITY)?),
            Some(None) | None => None,
        };
        let cell = FeatureInputReference {
            id: ctx.copy_retained_text(&operand.reference_ref, IDENTITY)?,
            parent: ctx.copy_retained_text(&scalar.parent, IDENTITY)?,
            feature_ref: scalar
                .feature_ref
                .as_deref()
                .map(|feature| ctx.copy_retained_text(feature, IDENTITY))
                .transpose()?,
            ordinal,
            offset: operand.offset,
            kind: operand.kind,
            class_ref,
            object_index: operand.entity_index,
        };
        ctx.push_vec(&mut cells, cell, COLLECT)?;
    }
    Ok(cells)
}

pub(crate) fn marker_local_id_offset(payload: &[u8], offset: usize) -> Option<usize> {
    let relative = if compact_legacy_code_two_profile_point_coordinates(payload, offset).is_some() {
        128
    } else if legacy_wide_profile_roster_curve(payload, offset)
        || marker_local_links(payload, offset).is_some()
    {
        88
    } else if marker_coordinates(payload, offset).is_some()
        || marker_is_geometry_locus(payload, offset)
    {
        // Only a next marker within LOCAL_ID_SEARCH bytes selects a local id
        // slot, so the search for it stops there: a farther marker, or none,
        // gives no id either way.
        const LOCAL_ID_SEARCH: usize = 168;
        let search_start = offset.checked_add(SKETCH_MARKER.len())?;
        let search_end = payload
            .len()
            .checked_sub(SKETCH_MARKER.len() - 1)?
            .min(offset.checked_add(LOCAL_ID_SEARCH)?);
        let next =
            (search_start..search_end).find(|next| sketch_marker_prefix_at(payload, *next))?;
        match next.checked_sub(offset)? {
            142 | 146 => 138,
            152 | 156 => 148,
            154 => 150,
            158 => 144,
            162 | 166 | 167 => 158,
            _ => return None,
        }
    } else {
        return None;
    };
    offset.checked_add(relative)
}

pub(crate) fn marker_local_id(payload: &[u8], offset: usize) -> Option<u32> {
    let start = marker_local_id_offset(payload, offset)?;
    let id = View::u32_le_at(payload, start)?;
    (id != u32::MAX).then_some(id)
}

fn marker_state_value(payload: &[u8], offset: usize) -> Option<cadmpeg_ir::scalar::FiniteReal> {
    if compact_legacy_code_two_profile_point_coordinates(payload, offset).is_some() {
        return None;
    }
    let relative = if packed_legacy_marker_body(payload, offset) {
        40
    } else {
        48
    };
    let offset = offset.checked_add(relative)?;
    cadmpeg_ir::scalar::FiniteReal::new(View::f64_le_at(payload, offset)?)
}

pub(crate) fn marker_coordinates(payload: &[u8], offset: usize) -> Option<FiniteVector<2>> {
    const GEOMETRY_PREFIX: [u8; 12] = [
        0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0x00, 0x00, 0x80, 0xbf,
    ];
    if let Some(coordinates) = compact_legacy_code_two_profile_point_coordinates(payload, offset) {
        return Some(coordinates);
    }
    if compact_legacy_marker_body(payload, offset) {
        let coordinate_kind = matches!(
            (
                marker_native_code(payload, offset),
                payload.get(offset + 19..offset + 23)
            ),
            (Some(0 | 1), Some([0x04, 0x00, 0x02, 0x00]))
                | (Some(0), Some([0x05, 0x00, 0x01, 0x00]))
        );
        if !coordinate_kind
            || marker_profile_curve_role(payload, offset) != Some(1)
            || payload.get(offset + 42..offset + 44) != Some(&[0x1e, 0x00])
        {
            return None;
        }
        return finite_coordinate_pair(payload, offset.checked_add(44)?);
    }
    if packed_legacy_marker_body(payload, offset) {
        if payload.get(offset + 48..offset + 50) != Some(&[0x1e, 0x00]) {
            return None;
        }
        return finite_coordinate_pair(payload, offset.checked_add(50)?);
    }
    if payload.get(offset + 5..offset + 17)? != GEOMETRY_PREFIX {
        return None;
    }
    if let Some((coordinates, _)) = linked_profile_point(payload, offset) {
        return Some(coordinates);
    }
    if let Some(coordinates) =
        legacy_geometry_locus_alternate_profile_point_coordinates(payload, offset)
    {
        return Some(coordinates);
    }
    if let Some(coordinates) = extended_four_link_profile_point_coordinates(payload, offset) {
        return Some(coordinates);
    }
    if let Some(coordinates) = legacy_144_profile_point_variant_coordinates(payload, offset) {
        return Some(coordinates);
    }
    let compact_indexed_value_body =
        matches!(
            payload.get(offset..offset + SKETCH_MARKER.len()),
            Some(prefix)
                if prefix == SKETCH_MARKER
                    || prefix == LEGACY_SKETCH_MARKER
                    || prefix == LEGACY_EXTENDED_SKETCH_MARKER
        ) && matches!(marker_native_code(payload, offset), Some(0..=2))
            && (marker_is_geometry_locus(payload, offset)
                || payload.get(offset + 23..offset + 27) == Some(&[0x04, 0x00, 0x02, 0x00]))
            && marker_profile_curve_role(payload, offset) == Some(1)
            && payload.get(offset + 31..offset + 39)
                == Some(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00])
            && payload.get(offset + 48..offset + 56) == Some(&1.0f64.to_le_bytes())
            && payload.get(offset + 60..offset + 64) == Some(&1u32.to_le_bytes())
            && payload.get(offset + 64..offset + 72) == Some(&(-1.0f64).to_le_bytes())
            && offset
                .checked_add(84)
                .is_some_and(|at| sketch_marker_prefix_at(payload, at));
    if compact_indexed_value_body {
        return None;
    }
    if let Some(coordinates) = legacy_linked_coordinates(payload, offset) {
        return Some(coordinates);
    }
    if let Some(coordinates) = legacy_declared_handle_coordinates(payload, offset) {
        return Some(coordinates);
    }
    if let Some([center, _, _]) = inline_arc_coordinates(payload, offset) {
        return Some(center);
    }
    if let Some(coordinates) = shifted_geometry_locus_coordinates(payload, offset) {
        return Some(coordinates);
    }
    if geometry_locus_profile_vertex(payload, offset)
        && payload.get(offset + 56..offset + 58) == Some(&[0x1e, 0x00])
    {
        return finite_coordinate_pair(payload, offset.checked_add(58)?);
    }
    let indexed_endpoint_body = extended_tagged_indexed_curve_endpoint_indices(payload, offset)
        .is_some()
        || wide_indexed_curve_endpoint_indices(payload, offset).is_some()
        || extended_wide_horizontal_relation_endpoint_indices(payload, offset).is_some();
    let coordinate_offset =
        if !indexed_endpoint_body && payload.get(offset + 64..offset + 66)? == [0x1e, 0x00] {
            offset.checked_add(66)?
        } else if !indexed_endpoint_body
            && (matches!(
                payload.get(offset..offset + LEGACY_SKETCH_MARKER.len())?,
                LEGACY_SKETCH_MARKER | LEGACY_EXTENDED_SKETCH_MARKER
            ) || (payload.get(offset..offset + SKETCH_MARKER.len())? == SKETCH_MARKER
                && (payload.get(offset + 17..offset + 21)? == 0u32.to_le_bytes()
                    || indexed_profile_vertex(payload, offset))))
            && (marker_is_geometry_locus(payload, offset)
                || indexed_profile_vertex(payload, offset)
                || indexed_profile_coordinate_candidate(payload, offset))
            && payload.get(offset + 56..offset + 58)? == [0x1e, 0x00]
        {
            offset.checked_add(58)?
        } else {
            return None;
        };
    finite_coordinate_pair(payload, coordinate_offset)
}

pub(super) fn finite_coordinate_pair(payload: &[u8], offset: usize) -> Option<FiniteVector<2>> {
    let first = View::f64_le_at(payload, offset)?;
    let second = View::f64_le_at(payload, offset + 8)?;
    FiniteVector::new([first, second])
}

fn extended_four_link_profile_point_coordinates(
    payload: &[u8],
    offset: usize,
) -> Option<FiniteVector<2>> {
    let valid_trailer_marker = [146, 150, 162, 174].into_iter().any(|relative| {
        offset
            .checked_add(relative)
            .is_some_and(|candidate| sketch_marker_prefix_at(payload, candidate))
    });
    let cells = [78, 86].map(|relative| {
        let cell = payload.get(offset + relative..offset + relative + 8)?;
        Some((
            View::u16_le_at(cell, 0)?,
            View::u16_le_at(cell, 2)?,
            cell[4..8] == [0xff; 4],
        ))
    });
    if payload.get(offset..offset + LEGACY_EXTENDED_SKETCH_MARKER.len())
        != Some(LEGACY_EXTENDED_SKETCH_MARKER)
        || marker_native_code(payload, offset) != Some(0)
        || payload.get(offset + 5..offset + 13) != Some(&[0xff; 8])
        || payload.get(offset + 23..offset + 27) != Some(&[0x04, 0x00, 0x02, 0x00])
        || marker_profile_curve_role(payload, offset) != Some(1)
        || payload.get(offset + 29..offset + 31) != Some(&[0; 2])
        || payload.get(offset + 31..offset + 39)
            != Some(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00])
        || payload.get(offset + 48..offset + 56) != Some(&1.0f64.to_le_bytes())
        || payload.get(offset + 56..offset + 58) != Some(&[0x1e, 0x00])
        || payload.get(offset + 74..offset + 76) != Some(&[0; 2])
        || payload.get(offset + 76..offset + 78) != Some(&4u16.to_le_bytes())
        || payload.get(offset + 94..offset + 100) != Some(&[0x00, 0x00, 0xfe, 0xff, 0xff, 0xff])
        || payload.get(offset + 100..offset + 134) != Some(&[0; 34])
        || payload.get(offset + 134..offset + 136) != Some(&[0x02, 0x00])
        || !valid_trailer_marker
        || !matches!(
            cells,
            [Some((first_tag, first_id, true)), Some((second_tag, second_id, true))]
                if first_tag != 0
                    && first_tag != u16::MAX
                    && second_tag != 0
                    && second_tag != u16::MAX
                    && (first_tag, first_id) != (second_tag, second_id)
        )
    {
        return None;
    }
    finite_coordinate_pair(payload, offset + 58)
}

fn legacy_extended_linked_profile_point_coordinates(
    payload: &[u8],
    offset: usize,
) -> Option<FiniteVector<2>> {
    let marker = payload.get(offset..offset + LEGACY_EXTENDED_SKETCH_MARKER.len());
    let coordinate_tag = payload.get(offset + 56..offset + 58);
    let valid_marker_and_coordinate_tag = match marker {
        Some(marker) if marker == LEGACY_EXTENDED_SKETCH_MARKER => {
            coordinate_tag == Some(&[0x1e, 0x00])
        }
        Some(marker) if marker == LEGACY_SKETCH_MARKER => {
            matches!(coordinate_tag, Some([0x1a | 0x1e, 0x00]))
        }
        _ => false,
    };
    let link_count = View::u16_le_at(payload, offset + 76);
    let trailer_state = View::u16_le_at(payload, offset + 134);
    let standard_link_state = matches!(
        payload.get(offset + 74..offset + 78),
        Some([0x00 | 0x01, 0x00, 0x02 | 0x03, 0x00])
    );
    let scaled_extended_link_state = payload
        .get(offset..offset + LEGACY_EXTENDED_SKETCH_MARKER.len())
        == Some(LEGACY_EXTENDED_SKETCH_MARKER)
        && marker_native_code(payload, offset) == Some(0)
        && payload.get(offset + 23..offset + 27) == Some(&[0x04, 0x00, 0x02, 0x00])
        && payload.get(offset + 74..offset + 76) == Some(&[0; 2])
        && matches!((link_count, trailer_state), (Some(count), Some(state))
            if state >= 2
                && state.checked_mul(2).is_some_and(|expected| count == expected));
    if !valid_marker_and_coordinate_tag
        || !matches!(marker_native_code(payload, offset), Some(0..=2))
        || payload.get(offset + 5..offset + 13) != Some(&[0xff; 8])
        || !matches!(
            payload.get(offset + 23..offset + 27),
            Some([0x04, 0x00, 0x02, 0x00] | [0x05, 0x00, 0x01, 0x00])
        )
        || marker_profile_curve_role(payload, offset) != Some(1)
        || payload.get(offset + 29..offset + 31) != Some(&[0; 2])
        || payload.get(offset + 31..offset + 39)
            != Some(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00])
        || payload.get(offset + 48..offset + 56) != Some(&1.0f64.to_le_bytes())
        || !(standard_link_state || scaled_extended_link_state)
    {
        return None;
    }
    let identity = |relative| View::u32_le_at(payload, offset + relative);
    let single_identity = payload.get(offset + 100..offset + 132) == Some(&[0; 32])
        && payload.get(offset + 132..offset + 134) == Some(&[0; 2])
        && matches!(payload.get(offset + 134..offset + 136), Some([0 | 1, 0]))
        && payload.get(offset + 136..offset + 142) == Some(&[0; 6])
        && identity(142).is_some_and(|identity| identity != u32::MAX)
        && (offset
            .checked_add(146)
            .is_some_and(|at| sketch_marker_prefix_at(payload, at))
            || payload.get(offset + 146..offset + 150) == Some(&[0; 4])
                && offset
                    .checked_add(150)
                    .is_some_and(|at| sketch_marker_prefix_at(payload, at)));
    let paired_identities = payload.get(offset + 100..offset + 138) == Some(&[0; 38])
        && matches!(
            [identity(138), identity(142)],
            [Some(first), Some(second)]
                if first != 0
                    && first != u32::MAX
                    && second != 0
                    && second != u32::MAX
                    && first != second
        )
        && offset
            .checked_add(146)
            .is_some_and(|at| sketch_marker_prefix_at(payload, at));
    let split_identities = payload.get(offset + 100..offset + 136) == Some(&[0; 36])
        && matches!(
            [identity(136), identity(142)],
            [Some(first), Some(second)]
                if first != 0
                    && first != u32::MAX
                    && second != 0
                    && second != u32::MAX
                    && first != second
        )
        && payload.get(offset + 140..offset + 142) == Some(&[0; 2])
        && offset
            .checked_add(146)
            .is_some_and(|at| sketch_marker_prefix_at(payload, at));
    let terminal_sentinel = payload.get(offset + 100..offset + 142) == Some(&[0; 42])
        && payload.get(offset + 74..offset + 78) == Some(&[0x00, 0x00, 0x02, 0x00])
        && identity(142) == Some(u32::MAX)
        && offset
            .checked_add(146)
            .is_some_and(|at| sketch_marker_prefix_at(payload, at));
    let continuation = payload.get(offset + 100..offset + 136) == Some(&[0; 36])
        && payload.get(offset + 74..offset + 78) == Some(&[0x00, 0x00, 0x02, 0x00])
        && identity(136).is_some_and(|identity| !matches!(identity, 0 | u32::MAX))
        && payload.get(offset + 140..offset + 142) == Some(&[0; 2])
        && payload.get(offset + 142..offset + 146) == Some(&1u32.to_le_bytes())
        && identity(146).is_some_and(|identity| !matches!(identity, 0 | u32::MAX))
        && offset
            .checked_add(150)
            .is_some_and(|at| sketch_marker_prefix_at(payload, at));
    let scaled_identity = scaled_extended_link_state
        && payload.get(offset + 100..offset + 134) == Some(&[0; 34])
        && payload.get(offset + 136..offset + 142) == Some(&[0; 6])
        && identity(142).is_some_and(|identity| identity != u32::MAX)
        && offset
            .checked_add(146)
            .is_some_and(|at| sketch_marker_prefix_at(payload, at));
    let cells = [78, 86].map(|relative| {
        let cell = payload.get(offset + relative..offset + relative + 8)?;
        Some((
            View::u16_le_at(cell, 0)?,
            View::u16_le_at(cell, 2)?,
            cell[4..8] == [0xff; 4],
        ))
    });
    let valid_cells = matches!(
        cells,
        [Some((first_tag, first_id, true)), Some((second_tag, second_id, true))]
            if first_tag != 0
                && second_tag != 0
                && (first_tag, first_id) != (second_tag, second_id)
    );
    if !valid_cells
        || payload.get(offset + 94..offset + 100) != Some(&[0x00, 0x00, 0xfe, 0xff, 0xff, 0xff])
        || !(single_identity
            || paired_identities
            || split_identities
            || terminal_sentinel
            || continuation
            || scaled_identity)
    {
        return None;
    }
    finite_coordinate_pair(payload, offset + 58)
}

fn legacy_single_incidence_profile_point_coordinates(
    payload: &[u8],
    offset: usize,
) -> Option<FiniteVector<2>> {
    let code = marker_native_code(payload, offset)?;
    let link_state = View::u16_le_at(payload, offset + 76)?;
    if payload.get(offset..offset + LEGACY_SKETCH_MARKER.len()) != Some(LEGACY_SKETCH_MARKER)
        || !matches!((code, link_state), (0 | 1, 2) | (2, 1))
        || payload.get(offset + 5..offset + 13) != Some(&[0xff; 8])
        || payload.get(offset + 23..offset + 29) != Some(&[0x04, 0x00, 0x02, 0x00, 0x01, 0x00])
        || payload.get(offset + 29..offset + 31) != Some(&[0; 2])
        || payload.get(offset + 31..offset + 39)
            != Some(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00])
        || payload.get(offset + 48..offset + 56) != Some(&1.0f64.to_le_bytes())
        || payload.get(offset + 56..offset + 58) != Some(&[0x1e, 0x00])
        || payload.get(offset + 74..offset + 76) != Some(&[0; 2])
    {
        return None;
    }
    let cell = payload.get(offset + 78..offset + 90)?;
    let selector = View::u16_le_at(cell, 0)?;
    let identifier = View::u16_le_at(cell, 2)?;
    if matches!(selector, 0 | u16::MAX)
        || matches!(identifier, 0 | u16::MAX)
        || cell[4..8] != [0xff; 4]
        || cell[8..12] != [0; 4]
        || payload.get(offset + 90..offset + 96) != Some(&[0xfe, 0xff, 0xff, 0xff, 0x00, 0x00])
    {
        return None;
    }
    let identity = |relative| View::u32_le_at(payload, offset + relative);
    let terminal_identity = payload.get(offset + 96..offset + 136) == Some(&[0; 40])
        && identity(136).is_some_and(|identity| identity != 0);
    let paired_identities = payload.get(offset + 96..offset + 128) == Some(&[0; 32])
        && matches!(
            [identity(128), identity(136)],
            [Some(first), Some(second)]
                if first != 0
                    && first != u32::MAX
                    && second != 0
                    && second != u32::MAX
                    && first != second
        )
        && payload.get(offset + 132..offset + 136) == Some(&[0; 4]);
    if !(terminal_identity || paired_identities)
        || !sketch_marker_prefix_at(payload, offset.checked_add(140)?)
    {
        return None;
    }
    finite_coordinate_pair(payload, offset + 58)
}

fn legacy_144_profile_point_variant_coordinates(
    payload: &[u8],
    offset: usize,
) -> Option<FiniteVector<2>> {
    let code = marker_native_code(payload, offset)?;
    let link_state = View::u16_le_at(payload, offset + pt_144::LINK_STATE)?;
    if payload.get(offset..offset + LEGACY_SKETCH_MARKER.len()) != Some(LEGACY_SKETCH_MARKER)
        || code != pt_144::NATIVE_KIND_VALUE
        || !matches!(link_state, 1..=3)
        || payload.get(offset + pt_144::HEADER..offset + pt_144::SENTINEL) != Some(&[0xff; 8])
        || payload.get(offset + pt_144::ZERO_PREFIX..offset + pt_144::PROFILE_LOCUS)
            != Some(&[0; 2])
        || payload.get(offset + pt_144::PROFILE_LOCUS..offset + pt_144::ZERO_STATE)
            != Some(&[0x04, 0x00, 0x02, 0x00, 0x01, 0x00])
        || payload.get(offset + pt_144::ZERO_STATE..offset + pt_144::SELECTOR) != Some(&[0; 2])
        || payload.get(offset + pt_144::SELECTOR..offset + pt_144::ZERO_STATE_PREFIX)
            != Some(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00])
        || payload.get(offset + pt_144::STATE_VALUE..offset + pt_144::COORDINATE_TAG)
            != Some(&1.0f64.to_le_bytes())
        || payload.get(offset + pt_144::COORDINATE_TAG..offset + pt_144::COORDINATE_FIRST)
            != Some(&[0x1e, 0x00])
        || payload.get(offset + pt_144::ZERO_LINK_PREFIX..offset + pt_144::LINK_STATE)
            != Some(&[0; 2])
    {
        return None;
    }
    let cell = payload.get(offset + pt_144::INCIDENCE_CELL..offset + pt_144::ZERO_POST_CELL)?;
    let selector = View::u16_le_at(cell, 0)?;
    let identifier = View::u16_le_at(cell, 2)?;
    if selector == 0
        || selector == u16::MAX
        || identifier == u16::MAX
        || cell[4..8] != [0xff; 4]
        || cell[8..12] != [0; 4]
        || payload.get(offset + pt_144::ZERO_POST_CELL..offset + pt_144::LINK_TERMINATOR)
            != Some(&[0; 4])
        || payload.get(offset + pt_144::LINK_TERMINATOR..offset + pt_144::TRAILER_PREFIX)
            != Some(&[0xfe, 0xff, 0xff, 0xff, 0x00, 0x00])
        || payload.get(offset + pt_144::TRAILER_PREFIX..offset + pt_144::IDENTITY)
            != Some(&[0; pt_144::IDENTITY - pt_144::TRAILER_PREFIX])
    {
        return None;
    }
    let identity = View::u32_le_at(payload, offset + pt_144::IDENTITY)?;
    (identity != 0
        && identity != u32::MAX
        && sketch_marker_prefix_at(payload, offset.checked_add(pt_144::LEN)?))
    .then(|| finite_coordinate_pair(payload, offset + pt_144::COORDINATE_FIRST))?
}

pub(super) fn legacy_140_profile_point_variant_coordinates(
    payload: &[u8],
    offset: usize,
) -> Option<FiniteVector<2>> {
    let code = marker_native_code(payload, offset)?;
    let link_state = View::u16_le_at(payload, offset + pt_140::LINK_STATE)?;
    if payload.get(offset..offset + LEGACY_SKETCH_MARKER.len()) != Some(LEGACY_SKETCH_MARKER)
        || code != 1
        || !matches!(link_state, 1..=3)
        || payload.get(offset + 5..offset + 13) != Some(&[0xff; 8])
        || payload.get(offset + 23..offset + 29) != Some(&[0x04, 0x00, 0x02, 0x00, 0x01, 0x00])
        || payload.get(offset + 29..offset + 31) != Some(&[0; 2])
        || payload.get(offset + 31..offset + 39)
            != Some(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00])
        || payload.get(offset + pt_140::STATE_VALUE..offset + pt_140::COORDINATE_TAG)
            != Some(&1.0f64.to_le_bytes())
        || payload.get(offset + pt_140::COORDINATE_TAG..offset + pt_140::COORDINATE_FIRST)
            != Some(&[0x1e, 0x00])
        || payload.get(offset + pt_140::ZERO_LINK_PREFIX..offset + pt_140::LINK_STATE)
            != Some(&[0; 2])
    {
        return None;
    }
    let cell = payload.get(offset + pt_140::INCIDENCE_CELL..offset + pt_140::LINK_TERMINATOR)?;
    let selector = View::u16_le_at(cell, 0)?;
    let identifier = View::u16_le_at(cell, 2)?;
    if selector == 0
        || selector == u16::MAX
        || identifier == u16::MAX
        || cell[4..8] != [0xff; 4]
        || cell[8..12] != [0; 4]
        || payload.get(offset + pt_140::LINK_TERMINATOR..offset + pt_140::TRAILER_PREFIX)
            != Some(&[0xfe, 0xff, 0xff, 0xff, 0x00, 0x00])
        || !sketch_marker_prefix_at(payload, offset.checked_add(pt_140::LEN)?)
    {
        return None;
    }
    let identity = |relative| View::u32_le_at(payload, offset + relative);
    let valid_identity = |identity: Option<u32>| {
        identity.is_some_and(|identity| identity != 0 && identity != u32::MAX)
    };
    let terminal =
        payload.get(offset + 96..offset + 136) == Some(&[0; 40]) && valid_identity(identity(136));
    let paired_at_128 = payload.get(offset + 96..offset + 128) == Some(&[0; 32])
        && valid_identity(identity(128))
        && payload.get(offset + 132..offset + 136) == Some(&[0; 4])
        && valid_identity(identity(136))
        && identity(128) != identity(136);
    let paired_at_132 = payload.get(offset + 96..offset + 128) == Some(&[0; 32])
        && payload.get(offset + 128..offset + 132) == Some(&[0; 4])
        && valid_identity(identity(132))
        && valid_identity(identity(136))
        && identity(132) != identity(136);
    (terminal || paired_at_128 || paired_at_132)
        .then(|| finite_coordinate_pair(payload, offset + pt_140::COORDINATE_FIRST))?
}

fn compact_legacy_linked_profile_point_coordinates(
    payload: &[u8],
    offset: usize,
) -> Option<FiniteVector<2>> {
    if !compact_legacy_marker_body(payload, offset)
        || marker_native_code(payload, offset) != Some(0)
        || payload.get(offset + 19..offset + 25) != Some(&[0x04, 0x00, 0x02, 0x00, 0x01, 0x00])
        || payload.get(offset + 25..offset + 31) != Some(&[0; 6])
        || payload.get(offset + 31..offset + 42) != Some(&[0x04, 0x00, 0, 0, 0, 0, 0, 0, 0, 0, 0])
        || !matches!(
            payload.get(offset + 42..offset + 44),
            Some([0x1a | 0x1e, 0])
        )
        || !matches!(payload.get(offset + 62..offset + 64), Some([2 | 3, 0]))
        || payload.get(offset + 80..offset + 86) != Some(&[0x00, 0x00, 0xfe, 0xff, 0xff, 0xff])
        || payload.get(offset + 86..offset + 128) != Some(&[0; 42])
        || View::u32_le_at(payload, offset + 128)
            .is_none_or(|identity| matches!(identity, 0 | u32::MAX))
        || !sketch_marker_prefix_at(payload, offset.checked_add(132)?)
    {
        return None;
    }
    let cells = [64, 72].map(|relative| {
        let cell = payload.get(offset + relative..offset + relative + 8)?;
        Some((
            operand_kind(cell[..2].try_into().ok()?)?,
            View::u16_le_at(cell, 2)?,
            cell[4..8] == [0xff; 4],
        ))
    });
    if !matches!(
        cells,
        [Some((first_kind, first_id, true)), Some((second_kind, second_id, true))]
            if first_kind == second_kind
                && first_id != u16::MAX
                && second_id != u16::MAX
                && first_id != second_id
    ) {
        return None;
    }
    finite_coordinate_pair(payload, offset + 44)
}

pub(super) fn compact_legacy_code_two_profile_point_coordinates(
    payload: &[u8],
    offset: usize,
) -> Option<FiniteVector<2>> {
    if payload.get(offset..offset + LEGACY_SKETCH_MARKER.len()) != Some(LEGACY_SKETCH_MARKER)
        || payload.get(offset + 5..offset + 13) != Some(&[0xff; 8])
        || payload.get(offset + code_two::NATIVE_KIND..offset + code_two::ZERO_PREFIX)
            != Some(&2u32.to_le_bytes())
        || payload.get(offset + code_two::ZERO_PREFIX..offset + code_two::PROFILE_LOCUS)
            != Some(&[0; 2])
        || payload.get(offset + code_two::PROFILE_LOCUS..offset + code_two::ZERO_STATE)
            != Some(&[0x04, 0x00, 0x02, 0x00, 0x01, 0x00])
        || payload.get(offset + code_two::ZERO_STATE..offset + code_two::SELECTOR) != Some(&[0; 6])
        || payload.get(offset + code_two::SELECTOR..offset + code_two::COORDINATE_TAG)
            != Some(&[0x04, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0])
        || payload.get(offset + code_two::COORDINATE_TAG..offset + code_two::COORDINATE_FIRST)
            != Some(&[0x1e, 0x00])
        || payload.get(offset + code_two::ZERO_LINK_PREFIX..offset + code_two::OPERAND_TAG)
            != Some(&[0; 2])
        || payload.get(offset + code_two::OPERAND_TAG..offset + code_two::OPERAND_FIRST)
            != Some(&[0x04, 0x00])
        || payload.get(offset + code_two::LINK_TERMINATOR..offset + code_two::ZERO_TRAILER)
            != Some(&[0, 0, 0xfe, 0xff, 0xff, 0xff])
        || payload.get(offset + code_two::ZERO_TRAILER..offset + code_two::TRAILER_KIND)
            != Some(&[0; 34])
        || payload.get(offset + code_two::TRAILER_KIND..offset + code_two::ZERO_IDENTITY_PREFIX)
            != Some(&2u32.to_le_bytes())
        || payload.get(offset + code_two::ZERO_IDENTITY_PREFIX..offset + code_two::IDENTITY)
            != Some(&[0; 4])
        || payload
            .get(offset + code_two::IDENTITY..offset + code_two::LEN)
            .is_none_or(|identity| identity == [0; 4] || identity == [0xff; 4])
        || !offset
            .checked_add(code_two::LEN)
            .is_some_and(|at| sketch_marker_prefix_at(payload, at))
    {
        return None;
    }
    let cells = [code_two::OPERAND_FIRST, code_two::OPERAND_SECOND].map(|relative| {
        let cell = payload.get(offset + relative..offset + relative + 8)?;
        Some((
            operand_kind(cell[..2].try_into().ok()?)?,
            View::u16_le_at(cell, 2)?,
            cell[4..8] == [0xff; 4],
        ))
    });
    if !matches!(
        cells,
        [Some((first_kind, first_id, true)), Some((second_kind, second_id, true))]
            if first_kind == second_kind
                && first_id != u16::MAX
                && second_id != u16::MAX
                && first_id != second_id
    ) {
        return None;
    }
    finite_coordinate_pair(payload, offset + code_two::COORDINATE_FIRST)
}

pub(super) fn compact_legacy_embedded_geometry_coordinates(
    payload: &[u8],
    offset: usize,
) -> Option<FiniteVector<2>> {
    if payload.get(offset..offset + LEGACY_SKETCH_MARKER.len()) != Some(LEGACY_SKETCH_MARKER)
        || payload.get(offset + 5..offset + 13) != Some(&[0xff; 8])
        || payload.get(offset + 13..offset + 17) != Some(&[0; 4])
        || payload.get(offset + 17..offset + 19) != Some(&[0; 2])
        || payload.get(offset + 19..offset + 25) != Some(&[0x05, 0x00, 0x01, 0x00, 0x01, 0x00])
        || payload.get(offset + 25..offset + 31) != Some(&[0; 6])
        || payload.get(offset + 31..offset + 42) != Some(&[0x05, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0])
        || payload.get(offset + 42..offset + 44) != Some(&[0x1e, 0x00])
        || !payload.get(offset + 60..offset + 64).is_some_and(|state| {
            state == [0; 4]
                || (state != [0xff; 4] && View::u32_le_at(state, 0).is_some_and(|value| value != 0))
        })
        || payload.get(offset + 64..offset + 70) != Some(&[0; 6])
        || payload.get(offset + 70..offset + 74) != Some(&[0xfe, 0xff, 0xff, 0xff])
        || payload.get(offset + 74..offset + 116) != Some(&[0; 42])
        || payload
            .get(offset + 116..offset + 120)
            .is_none_or(|identity| identity == [0; 4] || identity == [0xff; 4])
        || !offset
            .checked_add(120)
            .is_some_and(|at| sketch_marker_prefix_at(payload, at))
    {
        return None;
    }
    finite_coordinate_pair(payload, offset + 44)
}

pub(super) fn compact_legacy_coordinate_roster_coordinates(
    payload: &[u8],
    offset: usize,
) -> Option<FiniteVector<2>> {
    compact_legacy_code_two_profile_point_coordinates(payload, offset)
        .or_else(|| compact_legacy_embedded_geometry_coordinates(payload, offset))
        .or_else(|| {
            (payload.get(offset..offset + LEGACY_SKETCH_MARKER.len()) == Some(LEGACY_SKETCH_MARKER))
                .then(|| marker_coordinates(payload, offset))
                .flatten()
        })
}

fn packed_legacy_linked_profile_point_coordinates(
    payload: &[u8],
    offset: usize,
) -> Option<FiniteVector<2>> {
    if !packed_legacy_marker_body(payload, offset)
        || marker_native_code(payload, offset) != Some(0)
        || payload.get(offset + 19..offset + 23) != Some(&[0x04, 0x00, 0x02, 0x00])
        || marker_profile_curve_role(payload, offset) != Some(1)
        || payload.get(offset + 25..offset + 27) != Some(&[0; 2])
        || payload.get(offset + 27..offset + 31) != Some(&[0x00, 0x00, 0x04, 0x00])
        || payload.get(offset + 40..offset + 48) != Some(&1.0f64.to_le_bytes())
        || payload.get(offset + 48..offset + 50) != Some(&[0x1a, 0x00])
        || payload.get(offset + 66..offset + 70) != Some(&[0x00, 0x00, 0x02, 0x00])
        || payload.get(offset + 86..offset + 92) != Some(&[0x00, 0x00, 0xfe, 0xff, 0xff, 0xff])
        || payload.get(offset + 92..offset + 134) != Some(&[0; 42])
        || payload
            .get(offset + 134..offset + 138)
            .is_none_or(|identity| identity == [0; 4] || identity == [0xff; 4])
        || !sketch_marker_prefix_at(payload, offset.checked_add(138)?)
    {
        return None;
    }
    let cells = [70, 78].map(|relative| {
        let cell = payload.get(offset + relative..offset + relative + 8)?;
        let kind = operand_kind(cell[..2].try_into().ok()?)?;
        (operand_accepts_marker(kind, SketchInputKind::LineOrCircle)
            && operand_accepts_marker(kind, SketchInputKind::Arc)
            && cell[4..8] == [0xff; 4])
            .then_some((View::u16_le_at(cell, 0)?, View::u16_le_at(cell, 2)?))
    });
    let [Some((first_kind, first_id)), Some((second_kind, second_id))] = cells else {
        return None;
    };
    (first_kind == second_kind && first_id != 0 && second_id != 0 && first_id != second_id)
        .then(|| finite_coordinate_pair(payload, offset + 50))
        .flatten()
}

fn terminal_extended_profile_point_coordinates(
    payload: &[u8],
    offset: usize,
) -> Option<FiniteVector<2>> {
    if payload.get(offset..offset + LEGACY_EXTENDED_SKETCH_MARKER.len())
        != Some(LEGACY_EXTENDED_SKETCH_MARKER)
        || marker_native_code(payload, offset) != Some(2)
        || payload.get(offset + 23..offset + 27) != Some(&[0x04, 0x00, 0x02, 0x00])
        || marker_profile_curve_role(payload, offset) != Some(1)
        || payload.get(offset + 29..offset + 31) != Some(&[0; 2])
        || payload.get(offset + 31..offset + 39)
            != Some(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00])
        || payload.get(offset + 48..offset + 56) != Some(&1.0f64.to_le_bytes())
        || payload.get(offset + 56..offset + 58) != Some(&[0x1e, 0x00])
        || payload.get(offset + 74..offset + 78) != Some(&1u32.to_le_bytes())
        || payload.get(offset + 78..offset + 84) != Some(&[0; 6])
        || payload.get(offset + 84..offset + 88) != Some(&(-2i32).to_le_bytes())
        || payload.get(offset + 88..offset + 174) != Some(&[0; 86])
    {
        return None;
    }
    let identity = View::u16_le_at(payload, offset + 174)?;
    let selector = payload.get(offset + 176..offset + 178)?;
    if identity == 0
        || identity == u16::MAX
        || matches!(selector, [0, 0] | [0xff, 0xff])
        || payload.get(offset + 178..offset + 180) != Some(&[0; 2])
    {
        return None;
    }
    finite_coordinate_pair(payload, offset + 58)
}

fn validated_inline_arc_coordinates(
    center: FiniteVector<2>,
    start: FiniteVector<2>,
    end: FiniteVector<2>,
) -> Option<[FiniteVector<2>; 3]> {
    let start_radius = (start[0] - center[0]).hypot(start[1] - center[1]);
    let end_radius = (end[0] - center[0]).hypot(end[1] - center[1]);
    (start != end && start_radius > 0.0 && same_dimension_length(start_radius, end_radius))
        .then_some([center, start, end])
}

fn opposite_corner_inline_arc_coordinates(
    corner: FiniteVector<2>,
    start: FiniteVector<2>,
    end: FiniteVector<2>,
) -> Option<[FiniteVector<2>; 3]> {
    let start_components = start.finite_components();
    let end_components = end.finite_components();
    let candidates = [
        (
            [start[0], end[1]],
            FiniteVector::from([end_components[0], start_components[1]]),
        ),
        (
            [end[0], start[1]],
            FiniteVector::from([start_components[0], end_components[1]]),
        ),
    ];
    let mut centers = candidates
        .into_iter()
        .filter_map(|(candidate_corner, center)| {
            (same_dimension_length(candidate_corner[0], corner[0])
                && same_dimension_length(candidate_corner[1], corner[1]))
            .then_some(center)
        });
    let center = centers.next()?;
    if centers.next().is_some() {
        return None;
    }
    validated_inline_arc_coordinates(center, start, end)
}

fn compact_legacy_142_profile_curve_coordinates(
    payload: &[u8],
    offset: usize,
) -> Option<[FiniteVector<2>; 3]> {
    let end = offset.checked_add(legacy_142::LEN)?;
    let record = payload.get(offset..end)?;
    let next_marker = sketch_marker_prefix_at(payload, end)
        || end
            .checked_add(4)
            .is_some_and(|next| sketch_marker_prefix_at(payload, next));
    if record.get(legacy_142::MARKER..legacy_142::HEADER) != Some(LEGACY_SKETCH_MARKER)
        || record.get(legacy_142::HEADER..legacy_142::SHARED_SELECTOR) != Some(&[0xff; 8])
        || record.get(legacy_142::SHARED_SELECTOR..legacy_142::NATIVE_KIND)
            != Some(&legacy_142::SHARED_SELECTOR_VALUE.to_le_bytes())
        || record.get(legacy_142::NATIVE_KIND..legacy_142::NATIVE_KIND + 4)
            != Some(&legacy_142::NATIVE_KIND_VALUE.to_le_bytes())
        || record.get(legacy_142::PROFILE_LOCUS..legacy_142::ROLE)
            != Some(&[0x04, 0x00, 0x02, 0x00])
        || record.get(legacy_142::ROLE..legacy_142::ROLE + 2)
            != Some(&legacy_142::ROLE_VALUE.to_le_bytes())
        || record.get(legacy_142::NATIVE_KIND + 4..legacy_142::PROFILE_LOCUS) != Some(&[0; 2])
        || record.get(legacy_142::ROLE + 2..legacy_142::SELECTOR) != Some(&[0; 2])
        || record.get(legacy_142::SELECTOR..legacy_142::SELECTOR + 8)
            != Some(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00])
        || record.get(legacy_142::SELECTOR + 8..legacy_142::STATE_VALUE) != Some(&[0; 9])
        || record.get(legacy_142::STATE_VALUE..legacy_142::STATE_VALUE + 8)
            != Some(&legacy_142::STATE_VALUE_VALUE.to_le_bytes())
        || record.get(legacy_142::STATE_VALUE + 8..legacy_142::CURVE_TAG) != Some(&[0; 8])
        || !matches!(
            record.get(legacy_142::CURVE_TAG..legacy_142::AUXILIARY_FIRST),
            Some([0x12 | 0x16 | 0x1a, 0x00])
        )
        || record.get(legacy_142::BODY_KIND..legacy_142::BODY_KIND + 4)
            != Some(&legacy_142::BODY_KIND_VALUE.to_le_bytes())
        || record.get(legacy_142::BODY_KIND + 4..legacy_142::VARIANT) != Some(&[0; 6])
        || !next_marker
    {
        return None;
    }
    let auxiliary = finite_coordinate_pair(record, legacy_142::AUXILIARY_FIRST)?;
    let start = finite_coordinate_pair(record, legacy_142::START_FIRST)?;
    let end_point = finite_coordinate_pair(record, legacy_142::END_FIRST)?;
    let identity = View::u32_le_at(record, legacy_142::IDENTITY)?;
    if identity == 0 || identity == u32::MAX || start == end_point {
        return None;
    }
    Some([auxiliary, start, end_point])
}

pub(super) fn compact_legacy_142_profile_curve_endpoints(
    payload: &[u8],
    offset: usize,
) -> Option<[FiniteVector<2>; 2]> {
    let [_, start, end] = compact_legacy_142_profile_curve_coordinates(payload, offset)?;
    Some([start, end])
}

pub(super) fn inline_arc_coordinates(
    payload: &[u8],
    offset: usize,
) -> Option<[FiniteVector<2>; 3]> {
    if packed_legacy_marker_body(payload, offset)
        && marker_native_code(payload, offset) == Some(2)
        && payload.get(offset + 19..offset + 23) == Some(&[0x04, 0x00, 0x02, 0x00])
        && marker_profile_curve_role(payload, offset) == Some(1)
        && payload.get(offset + 25..offset + 29) == Some(&[0; 4])
        && payload.get(offset + 29..offset + 33) == Some(&[0x04, 0x00, 0x00, 0x00])
        && matches!(
            payload.get(offset + 48..offset + 50),
            Some([0x12 | 0x16, 0x00])
        )
        && payload.get(offset + 66..offset + 68) == Some(&11u16.to_le_bytes())
        && payload.get(offset + 68..offset + 76) == Some(&[0; 8])
        && payload
            .get(offset + 76..offset + 80)
            .is_some_and(|identity| identity != [0; 4] && identity != [0xff; 4])
        && payload.get(offset + 112..offset + 122) == Some(&[0; 10])
        && payload
            .get(offset + 122..offset + 126)
            .is_some_and(|identity| identity != [0; 4] && identity != [0xff; 4])
        && sketch_marker_prefix_at(payload, offset.checked_add(126)?)
    {
        let center = finite_coordinate_pair(payload, offset + 50)?;
        let start = finite_coordinate_pair(payload, offset + 80)?;
        let end = finite_coordinate_pair(payload, offset + 96)?;
        return validated_inline_arc_coordinates(center, start, end);
    }
    if payload.get(offset..offset + LEGACY_EXTENDED_SKETCH_MARKER.len())
        == Some(LEGACY_EXTENDED_SKETCH_MARKER)
        && marker_native_code(payload, offset) == Some(2)
        && payload.get(offset + 23..offset + 27) == Some(&[0x05, 0x00, 0x01, 0x00])
        && marker_profile_curve_role(payload, offset) == Some(1)
        && payload.get(offset + 29..offset + 31) == Some(&[0; 2])
        && payload.get(offset + 31..offset + 39)
            == Some(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00])
        && payload.get(offset + 48..offset + 56) == Some(&1.0f64.to_le_bytes())
        && payload.get(offset + 56..offset + 58) == Some(&[0x1a, 0x00])
        && payload.get(offset + 74..offset + 76) == Some(&11u16.to_le_bytes())
        && payload.get(offset + 76..offset + 88) == Some(&[0; 12])
        && payload.get(offset + 120..offset + 130) == Some(&[0; 10])
        && payload
            .get(offset + 130..offset + 134)
            .is_some_and(|object| object != [0; 4] && object != [0xff; 4])
        && sketch_marker_prefix_at(payload, offset.checked_add(134)?)
    {
        let center = finite_coordinate_pair(payload, offset + 58)?;
        let start = finite_coordinate_pair(payload, offset + 88)?;
        let end = finite_coordinate_pair(payload, offset + 104)?;
        return validated_inline_arc_coordinates(center, start, end);
    }
    let marker = payload.get(offset..offset + LEGACY_SKETCH_MARKER.len());
    let tag = payload.get(offset + 56..offset + 58);
    let direct_center_layout = marker == Some(LEGACY_EXTENDED_SKETCH_MARKER)
        && marker_native_code(payload, offset) == Some(2)
        && tag == Some(&[0x12, 0x00])
        && payload.get(offset + 128..offset + 134) == Some(&[0x00, 0x00, 0x02, 0x00, 0x00, 0x00]);
    let opposite_corner_layout = marker == Some(LEGACY_SKETCH_MARKER)
        && marker_native_code(payload, offset) == Some(1)
        && tag == Some(&[0x1a, 0x00])
        && payload.get(offset + 128..offset + 134) == Some(&[0x01, 0x00, 0x00, 0x00, 0x00, 0x00]);
    if (direct_center_layout || opposite_corner_layout)
        && payload.get(offset + 23..offset + 27) == Some(&[0x05, 0x00, 0x01, 0x00])
        && marker_profile_curve_role(payload, offset) == Some(1)
        && payload.get(offset + 29..offset + 31) == Some(&[0; 2])
        && payload.get(offset + 31..offset + 39)
            == Some(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00])
        && payload.get(offset + 48..offset + 56) == Some(&1.0f64.to_le_bytes())
        && payload.get(offset + 74..offset + 76) == Some(&11u16.to_le_bytes())
        && payload.get(offset + 76..offset + 84) == Some(&[0; 8])
        && payload.get(offset + 84..offset + 88) == Some(&1u32.to_le_bytes())
        && payload.get(offset + 120..offset + 124) == Some(&[0; 4])
        && payload
            .get(offset + 124..offset + 128)
            .is_some_and(|identity| identity != [0; 4] && identity != [0xff; 4])
        && payload
            .get(offset + 134..offset + 138)
            .is_some_and(|object| object != [0; 4] && object != [0xff; 4])
        && sketch_marker_prefix_at(payload, offset.checked_add(138)?)
    {
        let stored = finite_coordinate_pair(payload, offset + 58)?;
        let start = finite_coordinate_pair(payload, offset + 88)?;
        let end = finite_coordinate_pair(payload, offset + 104)?;
        return if direct_center_layout {
            validated_inline_arc_coordinates(stored, start, end)
        } else {
            opposite_corner_inline_arc_coordinates(stored, start, end)
        };
    }
    let common = payload.get(offset..offset + LEGACY_SKETCH_MARKER.len())
        == Some(LEGACY_SKETCH_MARKER)
        && marker_native_code(payload, offset) == Some(2)
        && payload.get(offset + 23..offset + 27) == Some(&[0x04, 0x00, 0x02, 0x00])
        && marker_profile_curve_role(payload, offset) == Some(1)
        && payload.get(offset + 29..offset + 31) == Some(&[0; 2])
        && payload.get(offset + 31..offset + 39)
            == Some(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00])
        && payload.get(offset + 48..offset + 56) == Some(&1.0f64.to_le_bytes());
    if common
        && payload.get(offset + 56..offset + 58) == Some(&[0x16, 0x00])
        && payload.get(offset + 74..offset + 76) == Some(&11u16.to_le_bytes())
        && payload.get(offset + 76..offset + 84) == Some(&[0; 8])
        && payload.get(offset + 84..offset + 88) == Some(&9u32.to_le_bytes())
        && payload.get(offset + 120..offset + 124) == Some(&[0; 4])
        && payload.get(offset + 128..offset + 132) == Some(&2u32.to_le_bytes())
        && payload.get(offset + 132..offset + 134) == Some(&[0; 2])
        && View::u32_le_at(payload, offset + 134).is_some_and(|object| object != u32::MAX)
        && sketch_marker_prefix_at(payload, offset.checked_add(138)?)
    {
        let corner = finite_coordinate_pair(payload, offset + 58)?;
        let start = finite_coordinate_pair(payload, offset + 88)?;
        let end = finite_coordinate_pair(payload, offset + 104)?;
        return opposite_corner_inline_arc_coordinates(corner, start, end);
    }
    if let Some([center, start, end]) =
        compact_legacy_142_profile_curve_coordinates(payload, offset)
    {
        return validated_inline_arc_coordinates(center, start, end);
    }
    if !common
        || payload.get(offset + 56..offset + 64) != Some(&[0; 8])
        || payload.get(offset + 64..offset + 66) != Some(&[0x1a, 0x00])
        || payload.get(offset + 86..offset + 92) != Some(&[0; 6])
        || payload.get(offset + 92..offset + 94) != Some(&1u16.to_le_bytes())
        || payload.get(offset + 128..offset + 132) != Some(&[0; 4])
        || payload.get(offset + 136..offset + 142) != Some(&[0; 6])
        || !sketch_marker_prefix_at(payload, offset.checked_add(146)?)
    {
        return None;
    }
    let center = finite_coordinate_pair(payload, offset + 66)?;
    let start = finite_coordinate_pair(payload, offset + 96)?;
    let end = finite_coordinate_pair(payload, offset + 112)?;
    validated_inline_arc_coordinates(center, start, end)
}

fn legacy_geometry_locus_alternate_point_common(payload: &[u8], offset: usize) -> bool {
    payload.get(offset..offset + LEGACY_SKETCH_MARKER.len()) == Some(LEGACY_SKETCH_MARKER)
        && payload.get(offset + 5..offset + 13) == Some(&[0xff; 8])
        && payload.get(offset + 13..offset + 17) == Some(&[0x00, 0x00, 0x80, 0xbf])
        && matches!(marker_native_code(payload, offset), Some(0..=2))
        && payload.get(offset + 21..offset + 23) == Some(&[0; 2])
        && marker_is_geometry_locus(payload, offset)
        && marker_profile_curve_role(payload, offset) == Some(1)
        && matches!(payload.get(offset + 29..offset + 31), Some([0 | 1, 0]))
        && payload.get(offset + 31..offset + 35) == Some(&[0x00, 0x00, 0x80, 0xbf])
        && matches!(
            payload.get(offset + 35..offset + 39),
            Some([0x00, 0x00, 0x04 | 0x05, 0x00])
        )
        && payload.get(offset + 39..offset + 48) == Some(&[0; 9])
        && payload.get(offset + 48..offset + 56) == Some(&1.0f64.to_le_bytes())
        && matches!(
            payload.get(offset + 56..offset + 58),
            Some([0x12 | 0x13 | 0x16, 0x00])
        )
        && finite_coordinate_pair(payload, offset + 58).is_some()
}

fn legacy_geometry_locus_alternate_point_134(payload: &[u8], offset: usize) -> bool {
    if !legacy_geometry_locus_alternate_point_common(payload, offset)
        || payload.get(offset + 84..offset + 88) != Some(&[0xfe, 0xff, 0xff, 0xff])
        || payload.get(offset + 88..offset + 130) != Some(&[0; 42])
        || View::u32_le_at(payload, offset + 130).is_none_or(|value| value == u32::MAX)
        || !offset
            .checked_add(134)
            .is_some_and(|at| sketch_marker_prefix_at(payload, at))
    {
        return false;
    }
    let unlinked = View::u16_le_at(payload, offset + 74)
        .is_some_and(|value| value != 0 && value != u16::MAX)
        && payload.get(offset + 76..offset + 84) == Some(&[0; 8]);
    let profile_vertex = payload.get(offset + 74..offset + 76) == Some(&[0; 2])
        && payload.get(offset + 76..offset + 78) == Some(&1u16.to_le_bytes())
        && payload.get(offset + 78..offset + 82) == Some(&[0; 4])
        && payload.get(offset + 82..offset + 84) == Some(&1u16.to_le_bytes())
        && payload.get(offset + 84..offset + 88) == Some(&(-2i32).to_le_bytes());
    unlinked || profile_vertex
}

fn legacy_geometry_locus_alternate_point_138(payload: &[u8], offset: usize) -> bool {
    let Some(first_identity) = View::u32_le_at(payload, offset + 124) else {
        return false;
    };
    let Some(second_identity) = View::u32_le_at(payload, offset + 130) else {
        return false;
    };
    let Some(following_identity) = View::u32_le_at(payload, offset + 134) else {
        return false;
    };
    legacy_geometry_locus_alternate_point_common(payload, offset)
        && payload.get(offset + 74..offset + 76) == Some(&[0; 2])
        && payload.get(offset + 76..offset + 78) == Some(&1u16.to_le_bytes())
        && payload.get(offset + 78..offset + 82) == Some(&[0; 4])
        && payload.get(offset + 82..offset + 84) == Some(&1u16.to_le_bytes())
        && payload.get(offset + 84..offset + 88) == Some(&(-2i32).to_le_bytes())
        && payload.get(offset + 88..offset + 124) == Some(&[0; 36])
        && payload.get(offset + 128..offset + 130) == Some(&[0; 2])
        && first_identity != 0
        && first_identity != u32::MAX
        && second_identity != 0
        && second_identity != u32::MAX
        && following_identity != 0
        && following_identity != u32::MAX
        && second_identity == following_identity
        && offset
            .checked_add(138)
            .is_some_and(|at| sketch_marker_prefix_at(payload, at))
}

fn legacy_geometry_locus_alternate_link_cell(
    payload: &[u8],
    offset: usize,
    relative: usize,
) -> Option<(u16, u16)> {
    let cell = payload.get(offset + relative..offset + relative + 12)?;
    let kind = operand_kind([cell[0], cell[1]])?;
    let selector = View::u16_le_at(cell, 0)?;
    let identifier = View::u16_le_at(cell, 2)?;
    (operand_accepts_marker(kind, SketchInputKind::LineOrCircle)
        && operand_accepts_marker(kind, SketchInputKind::Arc)
        && selector != 0
        && selector != u16::MAX
        && identifier != u16::MAX
        && cell[4..8] == [0xff; 4]
        && cell[8..12] == [0; 4])
        .then_some((selector, identifier))
}

fn legacy_geometry_locus_alternate_linked_point_tail(payload: &[u8], offset: usize) -> bool {
    let cells = [
        legacy_geometry_locus_alternate_link_cell(payload, offset, 78),
        legacy_geometry_locus_alternate_link_cell(payload, offset, 90),
    ];
    legacy_geometry_locus_alternate_point_common(payload, offset)
        && payload.get(offset + 74..offset + 76) == Some(&[0; 2])
        && payload.get(offset + 76..offset + 78) == Some(&2u16.to_le_bytes())
        && payload.get(offset + 102..offset + 108) == Some(&[0x00, 0x00, 0xfe, 0xff, 0xff, 0xff])
        && payload.get(offset + 108..offset + 150) == Some(&[0; 42])
        && View::u32_le_at(payload, offset + 150).is_some_and(|value| value != u32::MAX)
        && offset
            .checked_add(154)
            .is_some_and(|at| sketch_marker_prefix_at(payload, at))
        && matches!(
            cells,
            [Some(first), Some(second)] if first != second
        )
}

fn legacy_geometry_locus_alternate_profile_point_coordinates(
    payload: &[u8],
    offset: usize,
) -> Option<FiniteVector<2>> {
    if let Some((coordinates, _)) =
        legacy_geometry_locus_alternate_linked_profile_point(payload, offset)
    {
        return Some(coordinates);
    }
    (legacy_geometry_locus_alternate_point_134(payload, offset)
        || legacy_geometry_locus_alternate_point_138(payload, offset))
    .then(|| finite_coordinate_pair(payload, offset + 58))?
}

fn legacy_declared_handle_coordinates(payload: &[u8], offset: usize) -> Option<FiniteVector<2>> {
    let code = marker_native_code(payload, offset)?;
    let handle_state = View::u16_le_at(payload, offset + 76)?;
    let current_prefix = payload.get(offset..offset + SKETCH_MARKER.len()) == Some(SKETCH_MARKER);
    if !matches!((code, handle_state), (0..=2, 2 | 3)) {
        return None;
    }
    let legacy_alternate_prefix =
        payload.get(offset..offset + LEGACY_SKETCH_MARKER.len()) == Some(LEGACY_SKETCH_MARKER);
    let valid_state = payload.get(offset + 29..offset + 31) == Some(&[0; 2])
        || legacy_alternate_prefix && payload.get(offset + 29..offset + 31) == Some(&[1, 0]);
    let valid_coordinate_tag = if legacy_alternate_prefix {
        matches!(
            payload.get(offset + 56..offset + 58),
            Some([0x1e | 0x12 | 0x13 | 0x16, 0x00])
        )
    } else {
        payload.get(offset + 56..offset + 58) == Some(&[0x1e, 0x00])
    };
    let alternate_zero_identifier_tag = legacy_alternate_prefix
        && matches!(
            payload.get(offset + 56..offset + 58),
            Some([0x12 | 0x13 | 0x16, 0x00])
        );
    let valid_arc_reference = payload
        .get(offset + 78..offset + 82)
        .is_some_and(|reference| {
            reference[..2] != [0; 2]
                && reference[..2] != [0xff; 2]
                && reference[2..] != [0xff; 2]
                && (reference[2..] != [0; 2] || alternate_zero_identifier_tag)
        });
    if !matches!(
        payload.get(offset..offset + SKETCH_MARKER.len()),
        Some(prefix) if prefix == SKETCH_MARKER || prefix == LEGACY_SKETCH_MARKER
    ) || !matches!(
        payload.get(offset + 23..offset + 27),
        Some([0x04, 0x00, 0x02, 0x00] | [0x05, 0x00, 0x01, 0x00])
    ) || marker_profile_curve_role(payload, offset) != Some(1)
        || !valid_state
        || payload.get(offset + 31..offset + 39)
            != Some(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00])
        || payload.get(offset + 48..offset + 56) != Some(&1.0f64.to_le_bytes())
        || !valid_coordinate_tag
        || payload.get(offset + 74..offset + 76) != Some(&[0; 2])
    {
        return None;
    }
    let line_declaration = payload.get(offset + 78..offset + 84)
        == Some(&[0xff, 0xff, 0x01, 0x00, 0x0c, 0x00])
        && payload.get(offset + 84..offset + 96) == Some(b"sgLineHandle");
    let line_handle_id = View::u16_le_at(payload, offset + 96)?;
    let identity_bearing_tail = payload.get(offset + 124..offset + 162) == Some(&[0; 38])
        && matches!(
            (
                payload.get(offset + 162..offset + 166),
                payload.get(offset + 166..offset + 170)
            ),
            (Some(identity), Some(next_object))
                if identity != [0; 4]
                    && identity != [0xff; 4]
                    && next_object != [0; 4]
                    && next_object != [0xff; 4]
        );
    let line_handle = line_declaration
        && line_handle_id != u16::MAX
        && payload.get(offset + 98..offset + 106)
            == Some(&[0xff, 0xff, 0xff, 0xff, 0x00, 0x00, 0x00, 0x00])
        && payload.get(offset + 110..offset + 114) == Some(&[0xff; 4])
        && payload.get(offset + 114..offset + 118) == Some(&[0; 4])
        && payload.get(offset + 118..offset + 124) == Some(&[0x00, 0x00, 0xfe, 0xff, 0xff, 0xff])
        && (payload.get(offset + 124..offset + 166) == Some(&[0; 42]) || identity_bearing_tail)
        && sketch_marker_prefix_at(payload, offset.checked_add(170)?);
    let linked_line_handle_id = View::u16_le_at(payload, offset + 108)?;
    let linked_line_handle = payload
        .get(offset + 78..offset + 82)
        .is_some_and(|reference| reference[..2] != [0; 2] && reference[..2] != [0xff; 2])
        && payload.get(offset + 82..offset + 90)
            == Some(&[0xff, 0xff, 0xff, 0xff, 0x00, 0x00, 0x00, 0x00])
        && payload.get(offset + 90..offset + 96) == Some(&[0xff, 0xff, 0x01, 0x00, 0x0c, 0x00])
        && payload.get(offset + 96..offset + 108) == Some(b"sgLineHandle")
        && linked_line_handle_id != u16::MAX
        && payload.get(offset + 110..offset + 118)
            == Some(&[0xff, 0xff, 0xff, 0xff, 0x00, 0x00, 0x00, 0x00])
        && payload.get(offset + 118..offset + 124) == Some(&[0x00, 0x00, 0xfe, 0xff, 0xff, 0xff])
        && payload.get(offset + 124..offset + 166) == Some(&[0; 42])
        && payload.get(offset + 166..offset + 170) != Some(&[0; 4])
        && sketch_marker_prefix_at(payload, offset.checked_add(170)?);
    let arc_handle_id = View::u16_le_at(payload, offset + 119);
    let line_arc_handle = line_declaration
        && handle_state == 2
        && line_handle_id != u16::MAX
        && payload.get(offset + 98..offset + 108)
            == Some(&[0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0x01, 0x00, 0x0b, 0x00])
        && payload.get(offset + 108..offset + 119) == Some(b"sgArcHandle")
        && arc_handle_id.is_some_and(|arc_handle_id| {
            arc_handle_id != u16::MAX && arc_handle_id != line_handle_id
        })
        && payload.get(offset + 121..offset + 125) == Some(&[0xff; 4])
        && payload.get(offset + 125..offset + 131) == Some(&[0x00, 0x00, 0xfe, 0xff, 0xff, 0xff])
        && payload.get(offset + 131..offset + 173) == Some(&[0; 42])
        && payload
            .get(offset + 173..offset + 177)
            .is_some_and(|identity| identity != [0; 4] && identity != [0xff; 4])
        && sketch_marker_prefix_at(payload, offset.checked_add(177)?);
    let padded_arc_handle_id = View::u16_le_at(payload, offset + 123);
    let padded_line_arc_handle = line_declaration
        && handle_state == 2
        && line_handle_id != u16::MAX
        && payload.get(offset + 98..offset + 106)
            == Some(&[0xff, 0xff, 0xff, 0xff, 0x00, 0x00, 0x00, 0x00])
        && payload.get(offset + 106..offset + 112) == Some(&[0xff, 0xff, 0x01, 0x00, 0x0b, 0x00])
        && payload.get(offset + 112..offset + 123) == Some(b"sgArcHandle")
        && padded_arc_handle_id.is_some_and(|arc_handle_id| {
            arc_handle_id != u16::MAX && arc_handle_id != line_handle_id
        })
        && payload.get(offset + 125..offset + 133)
            == Some(&[0xff, 0xff, 0xff, 0xff, 0x00, 0x00, 0x00, 0x00])
        && payload.get(offset + 133..offset + 139) == Some(&[0x00, 0x00, 0xfe, 0xff, 0xff, 0xff])
        && payload.get(offset + 139..offset + 181) == Some(&[0; 42])
        && payload
            .get(offset + 181..offset + 185)
            .is_some_and(|identity| identity != [0; 4] && identity != [0xff; 4])
        && sketch_marker_prefix_at(payload, offset.checked_add(185)?);
    let valid_arc_handle_state = matches!((code, handle_state), (1 | 2, 2))
        || current_prefix && matches!((code, handle_state), (0, 3));
    let arc_handle = valid_arc_handle_state
        && valid_arc_reference
        && payload.get(offset + 82..offset + 90)
            == Some(&[0xff, 0xff, 0xff, 0xff, 0x00, 0x00, 0x00, 0x00])
        && payload.get(offset + 90..offset + 96) == Some(&[0xff, 0xff, 0x01, 0x00, 0x0b, 0x00])
        && payload.get(offset + 96..offset + 107) == Some(b"sgArcHandle")
        && payload.get(offset + 107..offset + 109) == Some(&[0; 2])
        && payload.get(offset + 109..offset + 117)
            == Some(&[0xff, 0xff, 0xff, 0xff, 0x00, 0x00, 0x00, 0x00])
        && payload.get(offset + 117..offset + 123) == Some(&[0x00, 0x00, 0xfe, 0xff, 0xff, 0xff])
        && payload.get(offset + 123..offset + 165) == Some(&[0; 42])
        && payload
            .get(offset + 165..offset + 169)
            .is_some_and(|identity| identity != [0; 4] && identity != [0xff; 4])
        && sketch_marker_prefix_at(payload, offset.checked_add(169)?);
    if !line_handle
        && !linked_line_handle
        && !line_arc_handle
        && !padded_line_arc_handle
        && !arc_handle
    {
        return None;
    }
    finite_coordinate_pair(payload, offset + 58)
}

fn extended_profile_point_coordinates(payload: &[u8], offset: usize) -> Option<FiniteVector<2>> {
    #[derive(Clone, Copy)]
    enum HandleState {
        Two,
        Three,
    }

    let code = marker_native_code(payload, offset)?;
    let extended_prefix = payload.get(offset..offset + LEGACY_EXTENDED_SKETCH_MARKER.len())
        == Some(LEGACY_EXTENDED_SKETCH_MARKER);
    let legacy_prefix =
        payload.get(offset..offset + LEGACY_SKETCH_MARKER.len()) == Some(LEGACY_SKETCH_MARKER);
    let profile_locus = payload.get(offset + 23..offset + 27) == Some(&[0x04, 0x00, 0x02, 0x00]);
    let geometry_locus = payload.get(offset + 23..offset + 27) == Some(&[0x05, 0x00, 0x01, 0x00]);
    let handle_state = match payload.get(offset + 74..offset + 78) {
        Some([0x00, 0x00, 0x02, 0x00]) => HandleState::Two,
        Some([0x00, 0x00, 0x03, 0x00]) => HandleState::Three,
        _ => return None,
    };
    if !matches!(
        (code, handle_state),
        (1 | 2, HandleState::Two) | (0 | 2, HandleState::Three)
    ) {
        return None;
    }
    let declaration_tag = match handle_state {
        HandleState::Two => payload.get(offset + 96..offset + 98) == Some(&[0x00, 0x00]),
        HandleState::Three => matches!(
            payload.get(offset + 96..offset + 98),
            Some([0x01 | 0x03, 0x00])
        ),
    };
    if (!extended_prefix && !legacy_prefix)
        || (!profile_locus && !geometry_locus)
        || marker_profile_curve_role(payload, offset) != Some(1)
        || payload.get(offset + 29..offset + 31) != Some(&[0; 2])
        || payload.get(offset + 31..offset + 39)
            != Some(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00])
        || payload.get(offset + 48..offset + 56) != Some(&1.0f64.to_le_bytes())
        || payload.get(offset + 56..offset + 58) != Some(&[0x1e, 0x00])
    {
        return None;
    }
    let declaration = extended_prefix
        && profile_locus
        && payload.get(offset + 78..offset + 84) == Some(&[0xff, 0xff, 0x01, 0x00, 0x0c, 0x00])
        && payload.get(offset + 84..offset + 96) == Some(b"sgLineHandle")
        && declaration_tag
        && payload.get(offset + 98..offset + 106)
            == Some(&[0xff, 0xff, 0xff, 0xff, 0x00, 0x00, 0x00, 0x00])
        && payload
            .get(offset + 106..offset + 108)
            .is_some_and(|selector| selector != [0; 2])
        && payload.get(offset + 110..offset + 118)
            == Some(&[0xff, 0xff, 0xff, 0xff, 0x00, 0x00, 0x00, 0x00])
        && payload.get(offset + 118..offset + 124) == Some(&[0x00, 0x00, 0xfe, 0xff, 0xff, 0xff])
        && payload.get(offset + 124..offset + 166) == Some(&[0; 42])
        && offset
            .checked_add(170)
            .is_some_and(|at| sketch_marker_prefix_at(payload, at));
    let compact_declaration_tag = View::u16_le_at(payload, offset + 96)?;
    let compact_declaration_variant = matches!(
        (
            extended_prefix,
            legacy_prefix,
            profile_locus,
            geometry_locus,
            code,
            handle_state,
            compact_declaration_tag,
        ),
        (true, false, true, false, 2, HandleState::Two, 0 | 1)
            | (true, false, true, false, 2, HandleState::Three, 3)
            | (false, true, false, true, 2, HandleState::Two, 0)
            | (true, false, false, true, 1, HandleState::Two, 12)
    );
    let compact_declaration = compact_declaration_variant
        && payload.get(offset + 78..offset + 84) == Some(&[0xff, 0xff, 0x01, 0x00, 0x0c, 0x00])
        && payload.get(offset + 84..offset + 96) == Some(b"sgLineHandle")
        && payload.get(offset + 98..offset + 102) == Some(&[0xff; 4])
        && payload
            .get(offset + 102..offset + 104)
            .is_some_and(|selector| selector != [0; 2] && selector != [0xff; 2])
        && payload
            .get(offset + 104..offset + 106)
            .is_some_and(|identifier| identifier != [0; 2] && identifier != [0xff; 2])
        && payload.get(offset + 106..offset + 110) == Some(&[0xff; 4])
        && payload.get(offset + 110..offset + 116) == Some(&[0x00, 0x00, 0xfe, 0xff, 0xff, 0xff])
        && payload.get(offset + 116..offset + 154) == Some(&[0; 38])
        && matches!(
            payload.get(offset + 154..offset + 158),
            Some([0 | 1, 0, 0, 0])
        )
        && payload
            .get(offset + 158..offset + 162)
            .is_some_and(|identity| identity != [0; 4] && identity != [0xff; 4])
        && offset
            .checked_add(162)
            .is_some_and(|at| sketch_marker_prefix_at(payload, at));
    let cells = [78, 90].map(|relative| {
        let cell = payload.get(offset + relative..offset + relative + 12)?;
        Some((
            View::u16_le_at(cell, 0)?,
            View::u16_le_at(cell, 2)?,
            cell[4..8] == [0xff; 4] && cell[8..12] == [0; 4],
        ))
    });
    let linked = extended_prefix
        && profile_locus
        && matches!(
            cells,
            [Some((first_tag, first_id, true)), Some((second_tag, second_id, true))]
                if first_tag != 0
                    && first_tag == second_tag
                    && first_id != second_id
        )
        && payload.get(offset + 102..offset + 108) == Some(&[0x00, 0x00, 0xfe, 0xff, 0xff, 0xff])
        && payload.get(offset + 108..offset + 144) == Some(&[0; 36])
        && payload.get(offset + 148..offset + 150) == Some(&[0; 2])
        && payload
            .get(offset + 150..offset + 154)
            .is_some_and(|identity| identity != [0; 4])
        && offset
            .checked_add(154)
            .is_some_and(|at| sketch_marker_prefix_at(payload, at));
    (declaration || compact_declaration || linked)
        .then(|| finite_coordinate_pair(payload, offset + 58))
        .flatten()
}

fn legacy_linked_coordinates(payload: &[u8], offset: usize) -> Option<FiniteVector<2>> {
    if payload.get(offset..offset + LEGACY_SKETCH_MARKER.len()) != Some(LEGACY_SKETCH_MARKER)
        || marker_native_code(payload, offset) != Some(0)
        || payload.get(offset + 23..offset + 27) != Some(&[0x04, 0x00, 0x02, 0x00])
        || marker_profile_curve_role(payload, offset) != Some(1)
        || payload.get(offset + 29..offset + 31) != Some(&[0; 2])
        || payload.get(offset + 31..offset + 39)
            != Some(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00])
        || payload.get(offset + 48..offset + 56) != Some(&1.0f64.to_le_bytes())
    {
        return None;
    }
    let (coordinate_offset, cells) = if payload.get(offset + 56..offset + 64) == Some(&[0; 8])
        && payload.get(offset + 64..offset + 66) == Some(&[0x1a, 0x00])
        && payload.get(offset + 82..offset + 84) == Some(&[0; 2])
        && payload.get(offset + 84..offset + 86) == Some(&2u16.to_le_bytes())
        && payload.get(offset + 110..offset + 116) == Some(&[0x00, 0x00, 0xfe, 0xff, 0xff, 0xff])
        && payload.get(offset + 116..offset + 158) == Some(&[0; 42])
        && View::u32_le_at(payload, offset + 158)
            .is_some_and(|local_id| !matches!(local_id, 0 | u32::MAX))
        && sketch_marker_prefix_at(payload, offset.checked_add(162)?)
    {
        (
            offset + 66,
            [
                &payload[offset + 86..offset + 98],
                &payload[offset + 98..offset + 110],
            ],
        )
    } else if payload.get(offset + 56..offset + 58) == Some(&[0x1a, 0x00])
        && payload.get(offset + 74..offset + 76) == Some(&[0; 2])
        && payload.get(offset + 76..offset + 78) == Some(&2u16.to_le_bytes())
        && payload.get(offset + 102..offset + 108) == Some(&[0x00, 0x00, 0xfe, 0xff, 0xff, 0xff])
        && payload.get(offset + 108..offset + 150) == Some(&[0; 42])
        && sketch_marker_prefix_at(payload, offset.checked_add(154)?)
    {
        (
            offset + 58,
            [
                &payload[offset + 78..offset + 90],
                &payload[offset + 90..offset + 102],
            ],
        )
    } else if payload.get(offset + 56..offset + 58) == Some(&[0x1a, 0x00])
        && payload.get(offset + 74..offset + 76) == Some(&[0; 2])
        && payload.get(offset + 76..offset + 78) == Some(&2u16.to_le_bytes())
        && payload.get(offset + 94..offset + 100) == Some(&[0x00, 0x00, 0xfe, 0xff, 0xff, 0xff])
        && payload.get(offset + 100..offset + 138) == Some(&[0; 38])
        && sketch_marker_prefix_at(payload, offset.checked_add(146)?)
    {
        (
            offset + 58,
            [
                &payload[offset + 78..offset + 86],
                &payload[offset + 86..offset + 94],
            ],
        )
    } else {
        return None;
    };
    let [first, second] = cells;
    if first[..2] != second[..2]
        || first[2..4] == [0; 2]
        || second[2..4] == [0; 2]
        || first[2..4] == second[2..4]
        || first[4..8] != [0xff; 4]
        || second[4..8] != [0xff; 4]
        || first.get(8..12).is_some_and(|tail| tail != [0; 4])
        || second.get(8..12).is_some_and(|tail| tail != [0; 4])
    {
        return None;
    }
    finite_coordinate_pair(payload, coordinate_offset)
}

fn indexed_profile_coordinate_candidate(payload: &[u8], offset: usize) -> bool {
    if payload.get(offset + 23..offset + 27) != Some(&[0x04, 0x00, 0x02, 0x00]) {
        return false;
    }
    let record_sizes: &[usize] = match payload.get(offset..offset + LEGACY_SKETCH_MARKER.len()) {
        Some(prefix) if prefix == LEGACY_SKETCH_MARKER => &[134, 138, 146, 150, 154, 161, 162],
        Some(prefix) if prefix == LEGACY_EXTENDED_SKETCH_MARKER => {
            if marker_profile_curve_role(payload, offset) != Some(1)
                || !matches!(marker_native_code(payload, offset), Some(0..=2))
                || marker_object_index(payload, offset).is_none()
            {
                return false;
            }
            &[134, 138, 140, 144]
        }
        Some(prefix) if prefix == SKETCH_MARKER => {
            if marker_profile_curve_role(payload, offset) != Some(1)
                || !matches!(marker_native_code(payload, offset), Some(0..=2))
                || marker_object_index(payload, offset).is_none()
            {
                return false;
            }
            &[134]
        }
        _ => return false,
    };
    record_sizes.iter().any(|size| {
        offset
            .checked_add(*size)
            .is_some_and(|at| sketch_marker_prefix_at(payload, at))
    })
}

fn compact_legacy_profile_vertex(payload: &[u8], offset: usize) -> bool {
    if !compact_legacy_marker_body(payload, offset)
        || marker_profile_curve_role(payload, offset) != Some(1)
    {
        return false;
    }
    marker_coordinates(payload, offset).is_some()
}

fn packed_legacy_profile_vertex(payload: &[u8], offset: usize) -> bool {
    packed_legacy_marker_body(payload, offset)
        && marker_profile_curve_role(payload, offset) == Some(1)
        && payload.get(offset + 48..offset + 50) == Some(&[0x1e, 0x00])
        && offset
            .checked_add(50)
            .and_then(|at| finite_coordinate_pair(payload, at))
            .is_some()
}

pub(crate) fn marker_object_index(payload: &[u8], offset: usize) -> Option<u32> {
    let start = offset.checked_sub(4)?;
    let index = View::u32_le_at(payload, start)?;
    (index != u32::MAX).then_some(index)
}

pub(super) fn marker_is_geometry_locus(payload: &[u8], offset: usize) -> bool {
    payload.get(offset + 23..offset + 27) == Some(&[0x05, 0x00, 0x01, 0x00])
}

fn indexed_profile_vertex(payload: &[u8], offset: usize) -> bool {
    matches!(
        payload.get(offset..offset + SKETCH_MARKER.len()),
        Some(prefix) if prefix == SKETCH_MARKER || prefix == LEGACY_EXTENDED_SKETCH_MARKER
    ) && payload.get(offset + 17..offset + 21) == Some(&1u32.to_le_bytes())
        && payload.get(offset + 23..offset + 27) == Some(&[0x04, 0x00, 0x02, 0x00])
        && marker_profile_curve_role(payload, offset) == Some(1)
}

fn current_geometry_locus_profile_vertex(payload: &[u8], offset: usize) -> bool {
    payload.get(offset..offset + SKETCH_MARKER.len()) == Some(SKETCH_MARKER)
        && marker_native_code(payload, offset) == Some(2)
        && marker_is_geometry_locus(payload, offset)
        && marker_profile_curve_role(payload, offset) == Some(1)
        && payload.get(offset + 29..offset + 31) == Some(&[0; 2])
        && payload.get(offset + 31..offset + 39)
            == Some(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00])
        && payload.get(offset + 48..offset + 56) == Some(&1.0f64.to_le_bytes())
        && payload.get(offset + 56..offset + 64) == Some(&[0; 8])
        && payload.get(offset + 64..offset + 66) == Some(&[0x1e, 0x00])
        && offset
            .checked_add(66)
            .and_then(|at| finite_coordinate_pair(payload, at))
            .is_some()
        && payload.get(offset + 82..offset + 86) == Some(&1u32.to_le_bytes())
        && payload.get(offset + 86..offset + 92) == Some(&[0; 6])
        && payload.get(offset + 92..offset + 98) == Some(&[0xfe, 0xff, 0xff, 0xff, 0x00, 0x00])
        && payload.get(offset + 98..offset + 132) == Some(&[0; 34])
        && payload
            .get(offset + 132..offset + 136)
            .is_some_and(|identity| identity != [0; 4] && identity != [0xff; 4])
        && payload.get(offset + 136..offset + 142) == Some(&[0; 6])
        && payload.get(offset + 142..offset + 146) == Some(&[0xff; 4])
        && offset
            .checked_add(146)
            .is_some_and(|at| sketch_marker_prefix_at(payload, at))
}

fn compact_geometry_locus_point_coordinates(
    payload: &[u8],
    offset: usize,
) -> Option<FiniteVector<2>> {
    if !matches!(
        payload.get(offset..offset + SKETCH_MARKER.len()),
        Some(prefix) if prefix == SKETCH_MARKER || prefix == LEGACY_SKETCH_MARKER
    ) || payload.get(offset + 5..offset + 13) != Some(&[0xff; 8])
        || payload.get(offset + 13..offset + 17) != Some(&[0x00, 0x00, 0x80, 0xbf])
        || marker_native_code(payload, offset) != Some(1)
        || !marker_is_geometry_locus(payload, offset)
        || marker_profile_curve_role(payload, offset) != Some(1)
        || payload.get(offset + 29..offset + 31) != Some(&[0; 2])
        || payload.get(offset + 31..offset + 39)
            != Some(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00])
        || payload.get(offset + 48..offset + 56) != Some(&1.0f64.to_le_bytes())
        || payload.get(offset + 56..offset + 58) != Some(&[0x1e, 0x00])
        || payload.get(offset + 74..offset + 78) != Some(&[0x00, 0x00, 0x01, 0x00])
        || payload.get(offset + 78..offset + 82) != Some(&[0; 4])
        || payload.get(offset + 82..offset + 84) != Some(&1u16.to_le_bytes())
        || payload.get(offset + 84..offset + 88) != Some(&(-2i32).to_le_bytes())
        || payload.get(offset + 88..offset + 130) != Some(&[0; 42])
        || payload
            .get(offset + 130..offset + 134)
            .is_none_or(|identity| identity == [0; 4] || identity == [0xff; 4])
        || !offset
            .checked_add(134)
            .is_some_and(|at| sketch_marker_prefix_at(payload, at))
    {
        return None;
    }
    finite_coordinate_pair(payload, offset + 58)
}

fn shifted_geometry_locus_coordinates(payload: &[u8], offset: usize) -> Option<FiniteVector<2>> {
    // This compact geometry-locus family moves the coordinate tag and pair eight
    // bytes past the ordinary linked-point positions. The framed trailer is part
    // of the discriminator: unlocated handles share the header but carry no pair.
    if payload.get(offset..offset + LEGACY_SKETCH_MARKER.len()) != Some(LEGACY_SKETCH_MARKER)
        || payload.get(offset + 5..offset + 13) != Some(&[0xff; 8])
        || !matches!(marker_native_code(payload, offset), Some(0..=2))
        || !marker_is_geometry_locus(payload, offset)
        || marker_profile_curve_role(payload, offset) != Some(1)
        || !matches!(
            payload.get(offset + 29..offset + 31),
            Some([0x00 | 0x01, 0x00])
        )
        || payload.get(offset + 31..offset + 39)
            != Some(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00])
        || payload.get(offset + 48..offset + 56) != Some(&1.0f64.to_le_bytes())
        || payload.get(offset + 56..offset + 64) != Some(&[0; 8])
        || !matches!(
            payload.get(offset + 64..offset + 66),
            Some([0x12 | 0x13 | 0x16 | 0x1e, 0x00])
        )
    {
        return None;
    }
    let valid_record = [(142, 92), (146, 92), (162, 112), (177, 127), (178, 128)]
        .into_iter()
        .any(|(length, sentinel)| {
            payload.get(offset + sentinel..offset + sentinel + 4) == Some(&[0xfe, 0xff, 0xff, 0xff])
                && offset
                    .checked_add(length)
                    .is_some_and(|at| sketch_marker_prefix_at(payload, at))
        });
    if !valid_record {
        return None;
    }
    finite_coordinate_pair(payload, offset + 66)
}

fn shifted_geometry_handle_coordinates(payload: &[u8], offset: usize) -> Option<FiniteVector<2>> {
    let coordinates = shifted_geometry_locus_coordinates(payload, offset)?;
    let code = marker_native_code(payload, offset)?;
    let line_handle = code == 2
        && payload.get(offset + 84..offset + 86) == Some(&2u16.to_le_bytes())
        && payload.get(offset + 86..offset + 92) == Some(&[0xff, 0xff, 0x01, 0x00, 0x0c, 0x00])
        && payload.get(offset + 92..offset + 104) == Some(b"sgLineHandle");
    let arc_handle = code == 1
        && payload.get(offset + 84..offset + 86) == Some(&2u16.to_le_bytes())
        && payload.get(offset + 98..offset + 104) == Some(&[0xff, 0xff, 0x01, 0x00, 0x0b, 0x00])
        && payload.get(offset + 104..offset + 115) == Some(b"sgArcHandle");
    (line_handle || arc_handle).then_some(coordinates)
}

fn terminal_wide_geometry_locus_profile_vertex(payload: &[u8], offset: usize) -> bool {
    let identity = |relative| {
        View::u32_le_at(payload, offset + relative)
            .is_some_and(|identity| !matches!(identity, 0 | u32::MAX))
    };
    let trailer = payload.get(offset + 96..offset + 138) == Some(&[0; 42])
        || payload.get(offset + 96..offset + 134) == Some(&[0; 38])
            && identity(134)
            && identity(138);
    matches!(
        payload.get(offset..offset + SKETCH_MARKER.len()),
        Some(prefix)
            if prefix == SKETCH_MARKER
                || prefix == LEGACY_SKETCH_MARKER
                || prefix == LEGACY_EXTENDED_SKETCH_MARKER
    ) && matches!(marker_native_code(payload, offset), Some(1 | 2))
        && marker_is_geometry_locus(payload, offset)
        && marker_profile_curve_role(payload, offset) == Some(1)
        && payload.get(offset + 29..offset + 31) == Some(&[0; 2])
        && payload.get(offset + 31..offset + 39)
            == Some(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00])
        && payload.get(offset + 48..offset + 56) == Some(&1.0f64.to_le_bytes())
        && payload.get(offset + 56..offset + 64) == Some(&[0; 8])
        && payload.get(offset + 64..offset + 66) == Some(&[0x1e, 0x00])
        && offset
            .checked_add(66)
            .and_then(|at| finite_coordinate_pair(payload, at))
            .is_some()
        && payload.get(offset + 92..offset + 96) == Some(&[0xfe, 0xff, 0xff, 0xff])
        && trailer
        && offset
            .checked_add(142)
            .is_some_and(|at| sketch_marker_prefix_at(payload, at))
}

fn geometry_locus_profile_vertex(payload: &[u8], offset: usize) -> bool {
    if !matches!(
        payload.get(offset..offset + SKETCH_MARKER.len()),
        Some(prefix)
            if prefix == SKETCH_MARKER
                || prefix == LEGACY_SKETCH_MARKER
                || prefix == LEGACY_EXTENDED_SKETCH_MARKER
    ) || !matches!(marker_native_code(payload, offset), Some(1 | 2))
        || !marker_is_geometry_locus(payload, offset)
        || marker_profile_curve_role(payload, offset) != Some(1)
        || payload.get(offset + 29..offset + 31) != Some(&[0; 2])
        || !matches!(
            payload.get(offset + 31..offset + 39),
            Some([0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04 | 0x05, 0x00])
        )
        || payload.get(offset + 48..offset + 56) != Some(&1.0f64.to_le_bytes())
        || payload.get(offset + 56..offset + 58) != Some(&[0x1e, 0x00])
        || offset
            .checked_add(58)
            .and_then(|at| finite_coordinate_pair(payload, at))
            .is_none()
    {
        return false;
    }
    let compact = matches!(
        payload.get(offset + 74..offset + 78),
        Some(value) if value == 0u32.to_le_bytes() || value == 1u32.to_le_bytes()
    ) && payload.get(offset + 78..offset + 84) == Some(&[0; 6])
        && payload.get(offset + 84..offset + 88) == Some(&[0xfe, 0xff, 0xff, 0xff])
        && payload.get(offset + 88..offset + 130) == Some(&[0; 42])
        && payload.get(offset + 130..offset + 134) == Some(&[0xff; 4])
        && offset
            .checked_add(134)
            .is_some_and(|at| sketch_marker_prefix_at(payload, at));
    let compact_local_identity = matches!(
        payload.get(offset + 74..offset + 78),
        Some([0x01, 0x00, 0x00, 0x00] | [0x00, 0x00, 0x01 | 0x02, 0x00])
    ) && payload.get(offset + 78..offset + 82) == Some(&[0; 4])
        && matches!(payload.get(offset + 82..offset + 84), Some([0 | 2, 0]))
        && payload.get(offset + 84..offset + 88) == Some(&[0xfe, 0xff, 0xff, 0xff])
        && payload.get(offset + 88..offset + 130) == Some(&[0; 42])
        && payload
            .get(offset + 130..offset + 134)
            .is_some_and(|identity| identity != [0; 4] && identity != [0xff; 4])
        && offset
            .checked_add(134)
            .is_some_and(|at| sketch_marker_prefix_at(payload, at));
    let compact_identity_pair = payload.get(offset + 74..offset + 78)
        == Some(&[0x00, 0x00, 0x01, 0x00])
        && payload.get(offset + 78..offset + 84) == Some(&[0; 6])
        && payload.get(offset + 84..offset + 88) == Some(&[0xfe, 0xff, 0xff, 0xff])
        && payload.get(offset + 88..offset + 126) == Some(&[0; 38])
        && matches!(
            (
                payload.get(offset + 126..offset + 130),
                payload.get(offset + 130..offset + 134)
            ),
            (Some(first), Some(second))
                if first != [0; 4]
                    && first != [0xff; 4]
                    && second != [0; 4]
                    && second != [0xff; 4]
                    && first != second
        )
        && offset
            .checked_add(134)
            .is_some_and(|at| sketch_marker_prefix_at(payload, at));
    let compact_value_two_identity_pair = payload.get(offset..offset + LEGACY_SKETCH_MARKER.len())
        == Some(LEGACY_SKETCH_MARKER)
        && payload.get(offset + 74..offset + 78) == Some(&1u32.to_le_bytes())
        && payload.get(offset + 78..offset + 84) == Some(&[0; 6])
        && payload.get(offset + 84..offset + 88) == Some(&[0xfe, 0xff, 0xff, 0xff])
        && payload.get(offset + 88..offset + 126) == Some(&[0; 38])
        && matches!(
            (
                payload.get(offset + 126..offset + 130),
                payload.get(offset + 130..offset + 134)
            ),
            (Some(first), Some(second))
                if first != [0; 4]
                    && first != [0xff; 4]
                    && second != [0; 4]
                    && second != [0xff; 4]
                    && first != second
        )
        && offset
            .checked_add(134)
            .is_some_and(|at| sketch_marker_prefix_at(payload, at));
    let identities = [
        payload.get(offset + 124..offset + 128),
        payload.get(offset + 128..offset + 132),
    ];
    let identity = matches!(
        identities,
        [Some(first), Some(second)]
            if first == second && first != [0; 4] && first != [0xff; 4]
    );
    let identity_bearing = payload.get(offset + 74..offset + 78) == Some(&1u32.to_le_bytes())
        && payload.get(offset + 78..offset + 84) == Some(&[0; 6])
        && payload.get(offset + 84..offset + 88) == Some(&[0xfe, 0xff, 0xff, 0xff])
        && payload.get(offset + 88..offset + 124) == Some(&[0; 36])
        && identity
        && payload.get(offset + 132..offset + 138) == Some(&[0x00, 0x00, 0x01, 0x00, 0x00, 0x00])
        && offset
            .checked_add(138)
            .is_some_and(|at| sketch_marker_prefix_at(payload, at));
    compact
        || compact_local_identity
        || compact_identity_pair
        || compact_value_two_identity_pair
        || identity_bearing
}

fn extended_geometry_locus_single_link_point(payload: &[u8], offset: usize) -> bool {
    let identity = |relative| {
        View::u32_le_at(payload, offset + relative)
            .is_some_and(|identity| identity != 0 && identity != u32::MAX)
    };
    let has_prefix = payload.get(offset..offset + LEGACY_EXTENDED_SKETCH_MARKER.len())
        == Some(LEGACY_EXTENDED_SKETCH_MARKER)
        && payload.get(offset + 5..offset + 13) == Some(&[0xff; 8])
        && payload.get(offset + 13..offset + 17) == Some(&[0x00, 0x00, 0x80, 0xbf])
        && marker_native_code(payload, offset) == Some(2)
        && marker_is_geometry_locus(payload, offset)
        && marker_profile_curve_role(payload, offset) == Some(1)
        && payload.get(offset + 29..offset + 31) == Some(&[0; 2])
        && payload.get(offset + 31..offset + 39)
            == Some(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00])
        && payload.get(offset + 39..offset + 48) == Some(&[0; 9])
        && payload.get(offset + 48..offset + 56) == Some(&1.0f64.to_le_bytes())
        && payload.get(offset + 56..offset + 58) == Some(&[0x1e, 0x00])
        && offset
            .checked_add(58)
            .and_then(|at| finite_coordinate_pair(payload, at))
            .is_some()
        && payload.get(offset + 74..offset + 78) == Some(&[0x00, 0x00, 0x01, 0x00])
        && payload.get(offset + 78..offset + 82) == Some(&[0; 4])
        && payload.get(offset + 82..offset + 86) == Some(&(-1i32).to_le_bytes())
        && payload.get(offset + 86..offset + 124) == Some(&[0; 38])
        && identity(124)
        && identity(128);
    has_prefix
        && View::u32_le_at(payload, offset + 124) != View::u32_le_at(payload, offset + 128)
        && payload.get(offset + 132..offset + 138) == Some(&[0x00, 0x00, 0x01, 0x00, 0x00, 0x00])
        && offset
            .checked_add(138)
            .is_some_and(|at| sketch_marker_prefix_at(payload, at))
}

type LinkedProfilePoint = (FiniteVector<2>, [(u16, u16); 2]);

pub(super) fn linked_profile_point(payload: &[u8], offset: usize) -> Option<LinkedProfilePoint> {
    let marker = payload.get(offset..offset + SKETCH_MARKER.len());
    let native_code = marker_native_code(payload, offset);
    let locus = payload.get(offset + 23..offset + 27);
    let link_count = payload.get(offset + 76..offset + 78);
    let profile_layout = locus == Some(&[0x04, 0x00, 0x02, 0x00]);
    let legacy_geometry_long_layout = marker == Some(LEGACY_SKETCH_MARKER)
        && native_code == Some(1)
        && locus == Some(&[0x05, 0x00, 0x01, 0x00])
        && link_count == Some(&2u16.to_le_bytes());
    let current_geometry_long_layout = marker == Some(SKETCH_MARKER)
        && native_code == Some(2)
        && locus == Some(&[0x05, 0x00, 0x01, 0x00])
        && link_count == Some(&2u16.to_le_bytes());
    let current_four_link_profile_layout = marker == Some(SKETCH_MARKER)
        && native_code == Some(0)
        && profile_layout
        && link_count == Some(&4u16.to_le_bytes());
    let current_four_link_short_tail = current_four_link_profile_layout
        && payload.get(offset + 108..offset + 142) == Some(&[0; 34])
        && payload.get(offset + 142..offset + 144) == Some(&[0x02, 0x00])
        && payload.get(offset + 144..offset + 150) == Some(&[0; 6])
        && View::u32_le_at(payload, offset + 150)
            .is_some_and(|identity| !matches!(identity, 0 | u32::MAX))
        && offset
            .checked_add(154)
            .is_some_and(|at| sketch_marker_prefix_at(payload, at));
    let current_four_link_long_tail = current_four_link_profile_layout
        && payload.get(offset + 108..offset + 142) == Some(&[0; 34])
        && payload.get(offset + 142..offset + 144) == Some(&[0x02, 0x00])
        && View::u32_le_at(payload, offset + 144)
            .is_some_and(|identity| !matches!(identity, 0 | u32::MAX))
        && payload.get(offset + 148..offset + 152) == Some(&[0; 4])
        && payload.get(offset + 152..offset + 154) == Some(&[0; 2])
        && payload.get(offset + 154..offset + 158) == Some(&[1, 0, 0, 0])
        && offset
            .checked_add(158)
            .is_some_and(|at| sketch_marker_prefix_at(payload, at));
    if !matches!(
        marker,
        Some(prefix)
            if prefix == SKETCH_MARKER
                || prefix == LEGACY_SKETCH_MARKER
                || prefix == LEGACY_EXTENDED_SKETCH_MARKER
    ) || !matches!(native_code, Some(0..=2))
        || (!profile_layout && !legacy_geometry_long_layout && !current_geometry_long_layout)
        || marker_profile_curve_role(payload, offset) != Some(1)
        || payload.get(offset + 29..offset + 31) != Some(&[0; 2])
        || payload.get(offset + 31..offset + 39)
            != Some(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00])
        || payload.get(offset + 48..offset + 56) != Some(&1.0f64.to_le_bytes())
        || payload.get(offset + 56..offset + 58) != Some(&[0x1e, 0x00])
        || payload.get(offset + 74..offset + 76) != Some(&[0; 2])
        || (!matches!(link_count, Some([2 | 3, 0])) && !current_four_link_profile_layout)
        || payload.get(offset + 102..offset + 108) != Some(&[0x00, 0x00, 0xfe, 0xff, 0xff, 0xff])
    {
        return None;
    }
    let standard_tail = payload.get(offset + 108..offset + 146) == Some(&[0; 38])
        && payload.get(offset + 146..offset + 150) != Some(&u32::MAX.to_le_bytes())
        && sketch_marker_prefix_at(payload, offset.checked_add(154)?);
    let long_tail = matches!(
        payload.get(offset..offset + SKETCH_MARKER.len()),
        Some(prefix)
            if prefix == SKETCH_MARKER
                || prefix == LEGACY_SKETCH_MARKER
                || prefix == LEGACY_EXTENDED_SKETCH_MARKER
    ) && payload.get(offset + 108..offset + 144) == Some(&[0; 36])
        && payload
            .get(offset + 144..offset + 148)
            .is_some_and(|identity| identity != [0; 4] && identity != [0xff; 4])
        && payload
            .get(offset + 148..offset + 152)
            .is_some_and(|state| state != [0xff; 4])
        && payload.get(offset + 152..offset + 154) == Some(&[0; 2])
        && payload.get(offset + 154..offset + 158) != Some(&0u32.to_le_bytes())
        && sketch_marker_prefix_at(payload, offset.checked_add(158)?);
    let valid_tail = current_four_link_short_tail
        || current_four_link_long_tail
        || long_tail
        || ((profile_layout || legacy_geometry_long_layout || current_geometry_long_layout)
            && standard_tail);
    if !valid_tail {
        return None;
    }
    let first = payload.get(offset + 78..offset + 90)?;
    let second = payload.get(offset + 90..offset + 102)?;
    let typed_curve_link = |cell: &[u8]| {
        operand_kind([cell[0], cell[1]]).is_some_and(|kind| {
            operand_accepts_marker(kind, SketchInputKind::LineOrCircle)
                && operand_accepts_marker(kind, SketchInputKind::Arc)
        })
    };
    if !typed_curve_link(first)
        || !typed_curve_link(second)
        || first[4..8] != [0xff; 4]
        || second[4..8] != [0xff; 4]
        || first[8..12] != [0; 4]
        || second[8..12] != [0; 4]
    {
        return None;
    }
    Some((
        finite_coordinate_pair(payload, offset + 58)?,
        [
            (View::u16_le_at(first, 0)?, View::u16_le_at(first, 2)?),
            (View::u16_le_at(second, 0)?, View::u16_le_at(second, 2)?),
        ],
    ))
}

fn legacy_geometry_locus_alternate_linked_profile_point(
    payload: &[u8],
    offset: usize,
) -> Option<LinkedProfilePoint> {
    if !legacy_geometry_locus_alternate_linked_point_tail(payload, offset) {
        return None;
    }
    Some((
        finite_coordinate_pair(payload, offset + 58)?,
        [
            (
                View::u16_le_at(payload, offset + 78)?,
                View::u16_le_at(payload, offset + 80)?,
            ),
            (
                View::u16_le_at(payload, offset + 90)?,
                View::u16_le_at(payload, offset + 92)?,
            ),
        ],
    ))
}

fn additional_linked_profile_point_coordinates(
    payload: &[u8],
    offset: usize,
) -> Option<FiniteVector<2>> {
    let marker = payload.get(offset..offset + LEGACY_EXTENDED_SKETCH_MARKER.len())?;
    let locus_and_state = (
        payload.get(offset + 23..offset + 27)?,
        payload.get(offset + 74..offset + 78)?,
    );
    let alternate_layout = (marker == LEGACY_EXTENDED_SKETCH_MARKER
        && locus_and_state == (&[0x04, 0x00, 0x02, 0x00], &[0x01, 0x00, 0x03, 0x00]))
        || (marker == LEGACY_SKETCH_MARKER
            && locus_and_state == (&[0x05, 0x00, 0x01, 0x00], &[0x00, 0x00, 0x02, 0x00]));
    if !alternate_layout
        || !matches!(marker_native_code(payload, offset), Some(0 | 2))
        || payload.get(offset + 5..offset + 13) != Some(&[0xff; 8])
        || marker_profile_curve_role(payload, offset) != Some(1)
        || payload.get(offset + 29..offset + 31) != Some(&[0; 2])
        || payload.get(offset + 31..offset + 39)
            != Some(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00])
        || payload.get(offset + 48..offset + 56) != Some(&1.0f64.to_le_bytes())
        || payload.get(offset + 56..offset + 58) != Some(&[0x1e, 0x00])
        || payload.get(offset + 102..offset + 108) != Some(&[0x00, 0x00, 0xfe, 0xff, 0xff, 0xff])
        || payload.get(offset + 108..offset + 150) != Some(&[0; 42])
        || !sketch_marker_prefix_at(payload, offset.checked_add(154)?)
    {
        return None;
    }
    let cells = [
        payload.get(offset + 78..offset + 90)?,
        payload.get(offset + 90..offset + 102)?,
    ];
    let links = [
        (
            View::u16_le_at(cells[0], 0)?,
            View::u16_le_at(cells[0], 2)?,
            cells[0][4..8] == [0xff; 4] && cells[0][8..12] == [0; 4],
        ),
        (
            View::u16_le_at(cells[1], 0)?,
            View::u16_le_at(cells[1], 2)?,
            cells[1][4..8] == [0xff; 4] && cells[1][8..12] == [0; 4],
        ),
    ];
    if !matches!(
        links,
        [(first_selector, first_id, true), (second_selector, second_id, true)]
            if first_selector != 0
                && first_selector != u16::MAX
                && second_selector != 0
                && second_selector != u16::MAX
                && (first_selector, first_id) != (second_selector, second_id)
    ) {
        return None;
    }
    finite_coordinate_pair(payload, offset + 58)
}

enum ReverseIncidenceOffsets {
    One(u64),
    Pair([u64; 2]),
    Many,
}

impl ReverseIncidenceOffsets {
    fn include_offset(&mut self, offset: u64) {
        match self {
            Self::One(first) if *first != offset => {
                *self = Self::Pair([(*first).min(offset), (*first).max(offset)]);
            }
            Self::Pair(pair) if !pair.contains(&offset) => {
                *self = Self::Many;
            }
            Self::One(_) | Self::Pair(_) | Self::Many => {}
        }
    }
}

pub(super) fn current_reverse_incidence_endpoint_offsets(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
    curve: &SketchInputEntity,
    markers: &[&SketchInputEntity],
) -> Result<Option<[u64; 2]>, CodecError> {
    if reverse_incidence_curve_index(payload, curve).is_none() {
        return Ok(None);
    }
    let (index, _storage) = ReverseIncidenceIndex::new(ctx, payload, markers)?;
    current_reverse_incidence_endpoint_offsets_in(ctx, payload, curve, &index)
}

fn reverse_incidence_curve_index(payload: &[u8], curve: &SketchInputEntity) -> Option<u16> {
    let offset = usize::try_from(curve.offset()).ok()?;
    let curve_index = u16::try_from(curve.object_index()?).ok()?;
    (payload.get(offset..offset + SKETCH_MARKER.len()) == Some(SKETCH_MARKER)
        && marker_native_code(payload, offset) == Some(1)
        && marker_profile_curve_role(payload, offset) == Some(1)
        && compact_indexed_curve_endpoint_indices(payload, offset).is_some())
        .then_some(curve_index)
}

/// The distinct endpoint offsets by feature, curve index and selector.
pub(super) struct ReverseIncidenceIndex<'a> {
    by_curve: HashMap<(Option<&'a str>, u16), Option<[u64; 2]>>,
    invalid_features: std::collections::BTreeSet<Option<&'a str>>,
}

impl<'a> ReverseIncidenceIndex<'a> {
    pub(super) fn new<'ctx>(
        ctx: &'ctx DecodeContext<'_>,
        payload: &[u8],
        markers: &[&'a SketchInputEntity],
    ) -> Result<(Self, ScopedReservation<'ctx>), CodecError> {
        const OPERATION: &str = "index SLDPRT reverse incidence endpoints";
        ctx.with_scoped_storage(OPERATION, || {
            let mut selectors_storage = ctx.reserve_scoped(0, OPERATION)?;
            let mut selectors = BTreeMap::<(Option<&str>, u16, u16), ReverseIncidenceOffsets>::new();
            let mut index = Self { by_curve: HashMap::new(), invalid_features: std::collections::BTreeSet::new() };
            for &marker in ctx.admit_iter(markers, OPERATION)? {
                let feature = marker.feature_ref.as_deref();
                let Ok(offset) = usize::try_from(marker.offset()) else {
                    ctx.insert_btree_set(&mut index.invalid_features, feature, OPERATION)?;
                    continue;
                };
                let Some((_, links)) = linked_profile_point(payload, offset) else {
                    continue;
                };
                for (selector, curve) in links {
                    selectors_storage.with_storage(|| {
                        match ctx.entry_btree_map(&mut selectors, (feature, curve, selector), OPERATION)? {
                            std::collections::btree_map::Entry::Occupied(mut entry) => entry.get_mut().include_offset(marker.offset()),
                            std::collections::btree_map::Entry::Vacant(entry) => { entry.insert(ReverseIncidenceOffsets::One(marker.offset())); }
                        }
                        Ok::<_, CodecError>(())
                    })?;
                }
            }
            for ((feature, curve, _), offsets) in ctx.admit_iter(selectors, OPERATION)? {
                let ReverseIncidenceOffsets::Pair(pair) = offsets else { continue; };
                match ctx.entry_hash_map(&mut index.by_curve, (feature, curve), OPERATION)? {
                    std::collections::hash_map::Entry::Occupied(mut entry) => { entry.insert(None); }
                    std::collections::hash_map::Entry::Vacant(entry) => { entry.insert(Some(pair)); }
                }
            }
            Ok(index)
        })
    }
}

pub(super) fn current_reverse_incidence_endpoint_offsets_in(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
    curve: &SketchInputEntity,
    index: &ReverseIncidenceIndex<'_>,
) -> Result<Option<[u64; 2]>, CodecError> {
    const OPERATION: &str = "resolve SLDPRT reverse incidence endpoints";
    let Some(curve_index) = reverse_incidence_curve_index(payload, curve) else {
        return Ok(None);
    };
    let feature = curve.feature_ref.as_deref();
    if ctx.contains_btree_set(&index.invalid_features, &feature, OPERATION)? {
        return Ok(None);
    }
    Ok(ctx.get_hash_map(&index.by_curve, &(feature, curve_index), OPERATION)?
        .and_then(Option::as_ref).copied())
}

fn linked_profile_vertex(payload: &[u8], offset: usize) -> bool {
    if payload.get(offset..offset + LEGACY_EXTENDED_SKETCH_MARKER.len())
        != Some(LEGACY_EXTENDED_SKETCH_MARKER)
        || marker_native_code(payload, offset) != Some(1)
        || !marker_is_geometry_locus(payload, offset)
        || marker_profile_curve_role(payload, offset) != Some(1)
        || payload.get(offset + 31..offset + 39)
            != Some(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00])
        || payload.get(offset + 48..offset + 56) != Some(&1.0f64.to_le_bytes())
        || payload.get(offset + 56..offset + 58) != Some(&[0x1e, 0x00])
        || offset
            .checked_add(58)
            .and_then(|at| finite_coordinate_pair(payload, at))
            .is_none()
        || payload.get(offset + 74..offset + 76) != Some(&[0; 2])
        || payload.get(offset + 102..offset + 108) != Some(&[0x00, 0x00, 0xfe, 0xff, 0xff, 0xff])
        || marker_object_index(payload, offset).is_none()
        || !offset
            .checked_add(154)
            .is_some_and(|at| sketch_marker_prefix_at(payload, at))
    {
        return false;
    }
    if marker_local_id(payload, offset).is_none() {
        return false;
    }
    let Some(first) = payload.get(offset + 78..offset + 90) else {
        return false;
    };
    let Some(second) = payload.get(offset + 90..offset + 102) else {
        return false;
    };
    first[..2] == second[..2]
        && first[2..4] != [0; 2]
        && second[2..4] != [0; 2]
        && first[4..8] == [0xff; 4]
        && second[4..8] == [0xff; 4]
        && first[8..12] == [0; 4]
        && second[8..12] == [0; 4]
}

fn compact_linked_profile_vertex(payload: &[u8], offset: usize) -> bool {
    if !matches!(
        payload.get(offset..offset + LEGACY_SKETCH_MARKER.len()),
        Some(prefix)
            if prefix == LEGACY_SKETCH_MARKER || prefix == LEGACY_EXTENDED_SKETCH_MARKER
    ) || marker_native_code(payload, offset) != Some(1)
        || !marker_is_geometry_locus(payload, offset)
        || marker_profile_curve_role(payload, offset) != Some(1)
        || payload.get(offset + 29..offset + 31) != Some(&[0; 2])
        || payload.get(offset + 31..offset + 39)
            != Some(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00])
        || payload.get(offset + 48..offset + 56) != Some(&1.0f64.to_le_bytes())
        || payload.get(offset + 56..offset + 58) != Some(&[0x1e, 0x00])
        || offset
            .checked_add(58)
            .and_then(|at| finite_coordinate_pair(payload, at))
            .is_none()
        || payload.get(offset + 74..offset + 78) != Some(&[0x00, 0x00, 0x02, 0x00])
        || payload.get(offset + 94..offset + 100) != Some(&[0x00, 0x00, 0xfe, 0xff, 0xff, 0xff])
        || payload.get(offset + 100..offset + 142) != Some(&[0; 42])
        || payload
            .get(offset + 142..offset + 146)
            .is_none_or(|identity| identity == [0; 4] || identity == [0xff; 4])
        || !offset
            .checked_add(146)
            .is_some_and(|at| sketch_marker_prefix_at(payload, at))
    {
        return false;
    }
    let cells = [
        payload.get(offset + 78..offset + 86),
        payload.get(offset + 86..offset + 94),
    ];
    matches!(
        cells,
        [Some(first), Some(second)]
            if View::u16_le_at(first, 0).is_some_and(is_class_token)
                && View::u16_le_at(second, 0).is_some_and(is_class_token)
                && first[..4] != second[..4]
                && first[4..8] == [0xff; 4]
                && second[4..8] == [0xff; 4]
    )
}

pub(super) fn legacy_extended_profile_curve_kind(
    payload: &[u8],
    offset: usize,
) -> Option<SketchInputKind> {
    if payload.get(offset..offset + LEGACY_EXTENDED_SKETCH_MARKER.len())
        != Some(LEGACY_EXTENDED_SKETCH_MARKER)
        || payload.get(offset + 17..offset + 21) != Some(&0u32.to_le_bytes())
        || !matches!(
            payload.get(offset + 23..offset + 27),
            Some(locus) if locus == [0x04, 0x00, 0x02, 0x00] || locus == [0x05, 0x00, 0x01, 0x00]
        )
        || marker_profile_curve_role(payload, offset) != Some(1)
    {
        return None;
    }
    if extended_selector44_indexed_line(payload, offset) {
        return Some(SketchInputKind::LineOrCircle);
    }
    let next = offset.checked_add(84)?;
    sketch_marker_prefix_at(payload, next).then(|| {
        if sketch_marker_at(payload, next) {
            SketchInputKind::LineOrCircle
        } else {
            SketchInputKind::Arc
        }
    })
}

#[cfg(test)]
mod tests;
