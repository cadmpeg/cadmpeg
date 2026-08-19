//! Sketch marker record decoding and profile point coordinates.

use super::bindings::spatial_relation_manager_ranges;
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
use super::relation_records::unique_relation_declaration_candidates;
use super::scalars::{feature_object_name, operand_kind};
use super::selections::{marker_local_links, operand_accepts_marker};
use super::{
    is_class_token, LEGACY_EXTENDED_SKETCH_MARKER, LEGACY_SKETCH_MARKER, SKETCH_MARKER,
    SPATIAL_VERTEX_PREFIX,
};
use crate::records::{
    FeatureInputClass, FeatureInputLane, FeatureInputOperandKind, FeatureInputReference,
    FeatureInputRelationBinding, FeatureInputScalar, SketchInputEntity, SketchInputKind,
};
use cadmpeg_core::decode::View;
use cadmpeg_ir::features::FeatureDefinition;
use cadmpeg_ir::math::Point3;
use cadmpeg_ir::sketches::{
    SpatialSketch, SpatialSketchEntity, SpatialSketchEntityId, SpatialSketchGeometry,
    SpatialSketchId,
};
use std::collections::{BTreeMap, HashMap};

use crate::layout::compact_current_spatial_marker_point as compact_spatial;
use crate::layout::compact_legacy_142_profile_curve as legacy_142;
use crate::layout::compact_legacy_code_two_profile_point as code_two;
use crate::layout::current_geometry_locus_arc_handle_point as current_arc_handle;
use crate::layout::current_geometry_locus_arc_handle_point_terminal as current_arc_handle_terminal;
use crate::layout::legacy_140_single_incidence_profile_point as pt_140;
use crate::layout::legacy_144_single_incidence_profile_point as pt_144;
use crate::layout::wide_spatial_marker_coordinate_prefix as spatial_pre;

/// Project spatial sketches from their model-space marker coordinates or bounded lines.
pub(crate) fn spatial_sketches(
    model_features: &mut [cadmpeg_ir::features::Feature],
    histories: &[crate::records::FeatureHistory],
    lanes: &[FeatureInputLane],
) -> (Vec<SpatialSketch>, Vec<SpatialSketchEntity>) {
    let records = histories
        .iter()
        .flat_map(|history| &history.features)
        .map(|feature| (feature.id.as_str(), feature))
        .collect::<HashMap<_, _>>();
    let mut sketches = Vec::new();
    let mut entities = Vec::new();
    for feature in model_features {
        let declared_spatial =
            matches!(feature.definition, FeatureDefinition::SpatialSketch { .. });
        if !declared_spatial && !matches!(feature.definition, FeatureDefinition::Sketch { .. }) {
            continue;
        }
        let Some(native_ref) = feature.native_ref.as_deref() else {
            continue;
        };
        let Some(record) = records.get(native_ref).copied() else {
            continue;
        };
        let mut point_candidates = Vec::new();
        for lane in lanes {
            let relation_ranges = spatial_relation_manager_ranges(lane)
                .into_iter()
                .filter(|(start, end)| {
                    lane.scalars.iter().any(|scalar| {
                        scalar.feature_ref.as_deref() == Some(native_ref)
                            && scalar.offset > *start
                            && scalar.offset < *end
                    })
                })
                .collect::<Vec<_>>();
            let points = lane
                .sketch_entities
                .iter()
                .filter(|marker| marker.feature_ref.as_deref() == Some(native_ref))
                // A spatial sketch stores an indexed geometry marker and an
                // unindexed zero-valued anchor for the same point. Only the
                // indexed marker is a model-space locus.
                .filter(|marker| marker.object_index.is_some())
                .filter_map(|marker| {
                    let offset = usize::try_from(marker.offset).ok()?;
                    if !relation_ranges.is_empty()
                        && (!relation_ranges
                            .iter()
                            .any(|(start, end)| marker.offset > *start && marker.offset < *end)
                            || marker.object_index.is_none()
                            || !matches!(
                                marker_native_code(&lane.native_payload, offset),
                                Some(1..=85)
                            ))
                    {
                        return None;
                    }
                    marker_spatial_coordinates(&lane.native_payload, offset)
                        .map(|point| (marker.id.clone(), point, offset))
                })
                .collect::<Vec<_>>();
            if !points.is_empty() {
                point_candidates.push((lane, points));
            }
        }
        if let Some((lane, points)) = point_candidates.first().filter(|(_, points)| {
            point_candidates.iter().all(|(_, candidate)| {
                candidate
                    .iter()
                    .map(|(_, point, _)| point)
                    .eq(points.iter().map(|(_, point, _)| point))
            })
        }) {
            let sketch_id = SpatialSketchId(feature.id.0.replacen(
                ":model:feature#",
                ":model:spatial-sketch#",
                1,
            ));
            let mut projected = points
                .iter()
                .map(|(native_ref, point, offset)| {
                    (
                        *offset,
                        Some(native_ref.clone()),
                        SpatialSketchGeometry::Point { position: *point },
                    )
                })
                .collect::<Vec<_>>();
            let lines = feature_object_name(record, lane)
                .and_then(|name| {
                    let start = usize::try_from(name.offset).ok()?;
                    let end = histories
                        .iter()
                        .flat_map(|history| &history.features)
                        .filter_map(|candidate| feature_object_name(candidate, lane))
                        .filter(|candidate| candidate.offset > name.offset)
                        .map(|candidate| candidate.offset)
                        .min()
                        .and_then(|offset| usize::try_from(offset).ok())
                        .unwrap_or(lane.native_payload.len());
                    let object = lane.native_payload.get(start..end)?;
                    let offsets = spatial_vertex_offsets(object);
                    let vertices = spatial_vertex_coordinates(object);
                    (offsets.len().is_multiple_of(2)
                        && offsets.len() == vertices.len()
                        && vertices
                            .chunks_exact(2)
                            .all(|vertices| vertices[0] != vertices[1]))
                    .then_some((start, offsets, vertices))
                })
                .unwrap_or_default();
            projected.extend(lines.1.chunks_exact(2).zip(lines.2.chunks_exact(2)).map(
                |(offsets, vertices)| {
                    (
                        lines.0 + offsets[0],
                        None,
                        SpatialSketchGeometry::Line {
                            start: vertices[0],
                            end: vertices[1],
                        },
                    )
                },
            ));
            projected.sort_unstable_by_key(|(offset, ..)| *offset);
            sketches.push(SpatialSketch {
                id: sketch_id.clone(),
                name: feature.name.clone(),
                configuration: if point_candidates.len() == 1 {
                    lane.configuration.clone()
                } else {
                    None
                },
                visible: None,
                profiles: Vec::new(),
                native_ref: Some(lane.id.clone()),
            });
            entities.extend(projected.into_iter().enumerate().map(
                |(index, (_, native_ref, geometry))| SpatialSketchEntity {
                    id: SpatialSketchEntityId(format!("{}:entity:{index}", sketch_id.0)),
                    sketch: sketch_id.clone(),
                    construction: false,
                    native_ref,
                    geometry_ref: None,
                    endpoint_refs: Vec::new(),
                    geometry,
                },
            ));
            feature.definition = FeatureDefinition::SpatialSketch {
                sketch: Some(sketch_id),
            };
            continue;
        }
        if !declared_spatial {
            continue;
        }
        let mut candidates = Vec::new();
        for lane in lanes {
            let Some(name) = feature_object_name(record, lane) else {
                continue;
            };
            let Some(start) = usize::try_from(name.offset).ok() else {
                continue;
            };
            let end = histories
                .iter()
                .flat_map(|history| &history.features)
                .filter_map(|candidate| feature_object_name(candidate, lane))
                .filter(|candidate| candidate.offset > name.offset)
                .map(|candidate| candidate.offset)
                .min()
                .and_then(|offset| usize::try_from(offset).ok())
                .unwrap_or(lane.native_payload.len());
            let Some(object) = lane.native_payload.get(start..end) else {
                continue;
            };
            let vertices = spatial_vertex_coordinates(object);
            if vertices.len() >= 2 && vertices.len().is_multiple_of(2) {
                candidates.push((lane, vertices));
            }
        }
        let [(lane, vertices)] = candidates.as_slice() else {
            continue;
        };
        if vertices
            .chunks_exact(2)
            .any(|vertices| vertices[0] == vertices[1])
        {
            continue;
        }
        let sketch_id = SpatialSketchId(feature.id.0.replacen(
            ":model:feature#",
            ":model:spatial-sketch#",
            1,
        ));
        sketches.push(SpatialSketch {
            id: sketch_id.clone(),
            name: feature.name.clone(),
            configuration: lane.configuration.clone(),
            visible: None,
            profiles: Vec::new(),
            native_ref: Some(lane.id.clone()),
        });
        entities.extend(
            vertices
                .chunks_exact(2)
                .enumerate()
                .map(|(index, vertices)| SpatialSketchEntity {
                    id: SpatialSketchEntityId(format!("{}:entity:{index}", sketch_id.0)),
                    sketch: sketch_id.clone(),
                    construction: false,
                    native_ref: None,
                    geometry_ref: None,
                    endpoint_refs: Vec::new(),
                    geometry: SpatialSketchGeometry::Line {
                        start: vertices[0],
                        end: vertices[1],
                    },
                }),
        );
        feature.definition = FeatureDefinition::SpatialSketch {
            sketch: Some(sketch_id),
        };
    }
    (sketches, entities)
}

pub(super) fn marker_spatial_coordinate_offset(payload: &[u8], offset: usize) -> Option<usize> {
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

fn marker_spatial_coordinates(payload: &[u8], offset: usize) -> Option<Point3> {
    const NATIVE_TO_IR: f64 = 1000.0;
    let coordinate_offset = marker_spatial_coordinate_offset(payload, offset)?;
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

pub(crate) fn spatial_vertex_coordinates(payload: &[u8]) -> Vec<Point3> {
    spatial_vertex_offsets(payload)
        .into_iter()
        .filter_map(|offset| {
            let point = Point3::new(
                View::f64_le_at(payload, offset + 45)?,
                View::f64_le_at(payload, offset + 53)?,
                View::f64_le_at(payload, offset + 61)?,
            );
            [point.x, point.y, point.z]
                .into_iter()
                .all(f64::is_finite)
                .then_some(point)
        })
        .collect()
}

pub(super) fn spatial_vertex_offsets(payload: &[u8]) -> Vec<usize> {
    payload
        .windows(SPATIAL_VERTEX_PREFIX.len())
        .enumerate()
        .filter_map(|(offset, bytes)| {
            (bytes == SPATIAL_VERTEX_PREFIX
                && payload.get(offset + 43..offset + 45) == Some(&[0x0e, 0x00]))
            .then_some(offset)
        })
        .collect()
}

pub(super) fn sketch_input_entities(payload: &[u8], parent: &str) -> Vec<SketchInputEntity> {
    let lane_key = parent.rsplit_once('#').map_or(parent, |(_, key)| key);
    (0..payload.len().saturating_sub(SKETCH_MARKER.len() - 1))
        .filter(|offset| sketch_marker_at(payload, *offset))
        .filter_map(|offset| {
            let code = marker_native_code(payload, offset)?;
            Some((offset, code))
        })
        .enumerate()
        .map(|(ordinal, (offset, code))| {
            let linked_point = linked_profile_point(payload, offset);
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
            let coordinates_m = linked_point
                .map(|(coordinates, _)| coordinates)
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
                .or_else(|| inline_arc.map(|[center, _, _]| center))
                .or_else(|| marker_coordinates(payload, offset));
            let kind = if slot_curve_and_center_indices(payload, offset).is_some() {
                SketchInputKind::Native(code)
            } else if inline_arc.is_some() {
                SketchInputKind::Arc
            } else if marker_spatial_coordinates(payload, offset).is_some()
                || legacy_declared_handle_coordinates(payload, offset).is_some()
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
            SketchInputEntity {
                id: format!("sldprt:feature-input:sketch-entity#{lane_key}:{offset}"),
                parent: parent.to_string(),
                feature_ref: None,
                ordinal: ordinal as u32,
                offset: offset as u64,
                object_index: marker_object_index(payload, offset),
                local_id: marker_local_id(payload, offset),
                kind,
                state_value: marker_state_value(payload, offset),
                coordinates_m,
                links: Vec::new(),
                link_selector: None,
            }
        })
        .collect()
}

fn current_geometry_locus_profile_line(payload: &[u8], offset: usize, code: u32) -> bool {
    code == 2
        && payload.get(offset..offset + SKETCH_MARKER.len()) == Some(SKETCH_MARKER)
        && marker_is_geometry_locus(payload, offset)
        && marker_profile_curve_role(payload, offset) == Some(1)
}

pub(super) fn sketch_marker_at(payload: &[u8], offset: usize) -> bool {
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

pub(crate) fn relation_bindings(
    parent: &str,
    classes: &[FeatureInputClass],
    scalars: &[FeatureInputScalar],
) -> Vec<FeatureInputRelationBinding> {
    relation_bindings_scoped(parent, classes, scalars, &[])
}

pub(crate) fn relation_bindings_scoped(
    parent: &str,
    classes: &[FeatureInputClass],
    scalars: &[FeatureInputScalar],
    intervals: &[(u64, u64, String)],
) -> Vec<FeatureInputRelationBinding> {
    let lane_key = parent.rsplit_once('#').map_or(parent, |(_, key)| key);
    unique_relation_declaration_candidates(classes, scalars, intervals)
        .into_iter()
        .enumerate()
        .map(
            |(ordinal, (class, scalar, family))| FeatureInputRelationBinding {
                id: format!(
                    "sldprt:feature-input:relation-binding#{lane_key}:{}",
                    class.offset
                ),
                parent: parent.to_string(),
                ordinal: ordinal as u32,
                offset: class.offset,
                class_ref: class.id.clone(),
                family,
                scalar_ref: scalar.id.clone(),
                feature_ref: scalar.feature_ref.clone(),
            },
        )
        .collect()
}

pub(crate) fn reference_cells(
    scalars: &[FeatureInputScalar],
    classes: &[FeatureInputClass],
) -> Vec<FeatureInputReference> {
    let mut cells = scalars
        .iter()
        .flat_map(|scalar| {
            scalar.operands.iter().map(|operand| FeatureInputReference {
                id: operand.reference_ref.clone(),
                parent: scalar.parent.clone(),
                feature_ref: scalar.feature_ref.clone(),
                ordinal: 0,
                offset: operand.offset,
                kind: operand.kind,
                class_ref: None,
                object_index: operand.entity_index,
            })
        })
        .collect::<Vec<_>>();
    cells.sort_by_key(|cell| cell.offset);
    cells.dedup_by_key(|cell| cell.offset);
    for (ordinal, cell) in cells.iter_mut().enumerate() {
        cell.ordinal = ordinal as u32;
    }
    let mut declarations = HashMap::<FeatureInputOperandKind, Vec<&FeatureInputClass>>::new();
    for cell in &cells {
        for class in classes.iter().filter(|class| {
            class.parent == cell.parent && class.offset.checked_sub(cell.offset) == Some(12)
        }) {
            declarations.entry(cell.kind).or_default().push(class);
        }
    }
    for declared in declarations.values_mut() {
        declared.sort_unstable_by_key(|class| class.offset);
        declared.dedup_by_key(|class| class.id.as_str());
    }
    for cell in &mut cells {
        if let Some([class]) = declarations.get(&cell.kind).map(Vec::as_slice) {
            cell.class_ref = Some(class.id.clone());
        }
    }
    cells
}

pub(crate) fn marker_local_id(payload: &[u8], offset: usize) -> Option<u32> {
    let relative = if compact_legacy_code_two_profile_point_coordinates(payload, offset).is_some() {
        128
    } else if legacy_wide_profile_roster_curve(payload, offset)
        || marker_local_links(payload, offset).is_some()
    {
        88
    } else if marker_coordinates(payload, offset).is_some()
        || marker_is_geometry_locus(payload, offset)
    {
        let search_start = offset.checked_add(SKETCH_MARKER.len())?;
        let next = (search_start..payload.len().saturating_sub(SKETCH_MARKER.len() - 1))
            .find(|next| sketch_marker_prefix_at(payload, *next))?;
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
    let start = offset.checked_add(relative)?;
    let id = View::u32_le_at(payload, start)?;
    (id != u32::MAX).then_some(id)
}

fn marker_state_value(payload: &[u8], offset: usize) -> Option<f64> {
    if compact_legacy_code_two_profile_point_coordinates(payload, offset).is_some() {
        return None;
    }
    let relative = if packed_legacy_marker_body(payload, offset) {
        40
    } else {
        48
    };
    let offset = offset.checked_add(relative)?;
    let value = View::f64_le_at(payload, offset)?;
    value.is_finite().then_some(value)
}

pub(crate) fn marker_coordinates(payload: &[u8], offset: usize) -> Option<[f64; 2]> {
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
            && sketch_marker_prefix_at(payload, offset.saturating_add(84));
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

pub(super) fn finite_coordinate_pair(payload: &[u8], offset: usize) -> Option<[f64; 2]> {
    let first = View::f64_le_at(payload, offset)?;
    let second = View::f64_le_at(payload, offset + 8)?;
    (first.is_finite() && second.is_finite()).then_some([first, second])
}

pub(super) fn extended_four_link_profile_point_coordinates(
    payload: &[u8],
    offset: usize,
) -> Option<[f64; 2]> {
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
) -> Option<[f64; 2]> {
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
        && (sketch_marker_prefix_at(payload, offset.saturating_add(146))
            || payload.get(offset + 146..offset + 150) == Some(&[0; 4])
                && sketch_marker_prefix_at(payload, offset.saturating_add(150)));
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
        && sketch_marker_prefix_at(payload, offset.saturating_add(146));
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
        && sketch_marker_prefix_at(payload, offset.saturating_add(146));
    let terminal_sentinel = payload.get(offset + 100..offset + 142) == Some(&[0; 42])
        && payload.get(offset + 74..offset + 78) == Some(&[0x00, 0x00, 0x02, 0x00])
        && identity(142) == Some(u32::MAX)
        && sketch_marker_prefix_at(payload, offset.saturating_add(146));
    let continuation = payload.get(offset + 100..offset + 136) == Some(&[0; 36])
        && payload.get(offset + 74..offset + 78) == Some(&[0x00, 0x00, 0x02, 0x00])
        && identity(136).is_some_and(|identity| !matches!(identity, 0 | u32::MAX))
        && payload.get(offset + 140..offset + 142) == Some(&[0; 2])
        && payload.get(offset + 142..offset + 146) == Some(&1u32.to_le_bytes())
        && identity(146).is_some_and(|identity| !matches!(identity, 0 | u32::MAX))
        && sketch_marker_prefix_at(payload, offset.saturating_add(150));
    let scaled_identity = scaled_extended_link_state
        && payload.get(offset + 100..offset + 134) == Some(&[0; 34])
        && payload.get(offset + 136..offset + 142) == Some(&[0; 6])
        && identity(142).is_some_and(|identity| identity != u32::MAX)
        && sketch_marker_prefix_at(payload, offset.saturating_add(146));
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
) -> Option<[f64; 2]> {
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

fn legacy_144_profile_point_variant_coordinates(payload: &[u8], offset: usize) -> Option<[f64; 2]> {
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
) -> Option<[f64; 2]> {
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
) -> Option<[f64; 2]> {
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
) -> Option<[f64; 2]> {
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
        || !sketch_marker_prefix_at(payload, offset.saturating_add(code_two::LEN))
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
) -> Option<[f64; 2]> {
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
        || !sketch_marker_prefix_at(payload, offset.saturating_add(120))
    {
        return None;
    }
    finite_coordinate_pair(payload, offset + 44)
}

pub(super) fn compact_legacy_coordinate_roster_coordinates(
    payload: &[u8],
    offset: usize,
) -> Option<[f64; 2]> {
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
) -> Option<[f64; 2]> {
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

fn terminal_extended_profile_point_coordinates(payload: &[u8], offset: usize) -> Option<[f64; 2]> {
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
    center: [f64; 2],
    start: [f64; 2],
    end: [f64; 2],
) -> Option<[[f64; 2]; 3]> {
    let start_radius = (start[0] - center[0]).hypot(start[1] - center[1]);
    let end_radius = (end[0] - center[0]).hypot(end[1] - center[1]);
    (start != end && start_radius > 0.0 && same_dimension_length(start_radius, end_radius))
        .then_some([center, start, end])
}

fn opposite_corner_inline_arc_coordinates(
    corner: [f64; 2],
    start: [f64; 2],
    end: [f64; 2],
) -> Option<[[f64; 2]; 3]> {
    let candidates = [
        ([start[0], end[1]], [end[0], start[1]]),
        ([end[0], start[1]], [start[0], end[1]]),
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

pub(super) fn compact_legacy_142_profile_curve_coordinates(
    payload: &[u8],
    offset: usize,
) -> Option<[[f64; 2]; 3]> {
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
) -> Option<[[f64; 2]; 2]> {
    let [_, start, end] = compact_legacy_142_profile_curve_coordinates(payload, offset)?;
    Some([start, end])
}

pub(super) fn inline_arc_coordinates(payload: &[u8], offset: usize) -> Option<[[f64; 2]; 3]> {
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

fn legacy_declared_handle_coordinates(payload: &[u8], offset: usize) -> Option<[f64; 2]> {
    let code = marker_native_code(payload, offset)?;
    let handle_state = View::u16_le_at(payload, offset + 76)?;
    let current_prefix = payload.get(offset..offset + SKETCH_MARKER.len()) == Some(SKETCH_MARKER);
    if !matches!((code, handle_state), (0..=2, 2 | 3)) {
        return None;
    }
    if !matches!(
        payload.get(offset..offset + SKETCH_MARKER.len()),
        Some(prefix) if prefix == SKETCH_MARKER || prefix == LEGACY_SKETCH_MARKER
    ) || !matches!(
        payload.get(offset + 23..offset + 27),
        Some([0x04, 0x00, 0x02, 0x00] | [0x05, 0x00, 0x01, 0x00])
    ) || marker_profile_curve_role(payload, offset) != Some(1)
        || payload.get(offset + 29..offset + 31) != Some(&[0; 2])
        || payload.get(offset + 31..offset + 39)
            != Some(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00])
        || payload.get(offset + 48..offset + 56) != Some(&1.0f64.to_le_bytes())
        || payload.get(offset + 56..offset + 58) != Some(&[0x1e, 0x00])
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
        && payload
            .get(offset + 78..offset + 82)
            .is_some_and(|reference| {
                reference[..2] != [0; 2]
                    && reference[..2] != [0xff; 2]
                    && reference[2..] != [0; 2]
                    && reference[2..] != [0xff; 2]
            })
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

fn extended_profile_point_coordinates(payload: &[u8], offset: usize) -> Option<[f64; 2]> {
    let code = marker_native_code(payload, offset)?;
    let extended_prefix = payload.get(offset..offset + LEGACY_EXTENDED_SKETCH_MARKER.len())
        == Some(LEGACY_EXTENDED_SKETCH_MARKER);
    let legacy_prefix =
        payload.get(offset..offset + LEGACY_SKETCH_MARKER.len()) == Some(LEGACY_SKETCH_MARKER);
    let profile_locus = payload.get(offset + 23..offset + 27) == Some(&[0x04, 0x00, 0x02, 0x00]);
    let geometry_locus = payload.get(offset + 23..offset + 27) == Some(&[0x05, 0x00, 0x01, 0x00]);
    let handle_state = match payload.get(offset + 74..offset + 78) {
        Some([0x00, 0x00, state @ (0x02 | 0x03), 0x00]) => *state,
        _ => return None,
    };
    if !matches!((code, handle_state), (1 | 2, 2) | (0 | 2, 3)) {
        return None;
    }
    let declaration_tag = match handle_state {
        2 => payload.get(offset + 96..offset + 98) == Some(&[0x00, 0x00]),
        3 => matches!(
            payload.get(offset + 96..offset + 98),
            Some([0x01 | 0x03, 0x00])
        ),
        _ => unreachable!(),
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
        && sketch_marker_prefix_at(payload, offset.saturating_add(170));
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
        (true, false, true, false, 2, 2, 0 | 1)
            | (true, false, true, false, 2, 3, 3)
            | (false, true, false, true, 2, 2, 0)
            | (true, false, false, true, 1, 2, 12)
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
        && sketch_marker_prefix_at(payload, offset.saturating_add(162));
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
        && sketch_marker_prefix_at(payload, offset.saturating_add(154));
    (declaration || compact_declaration || linked)
        .then(|| finite_coordinate_pair(payload, offset + 58))
        .flatten()
}

fn legacy_linked_coordinates(payload: &[u8], offset: usize) -> Option<[f64; 2]> {
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
    record_sizes
        .iter()
        .any(|size| sketch_marker_prefix_at(payload, offset.saturating_add(*size)))
}

fn compact_legacy_profile_vertex(payload: &[u8], offset: usize) -> bool {
    compact_legacy_marker_body(payload, offset)
        && marker_profile_curve_role(payload, offset) == Some(1)
        && marker_coordinates(payload, offset).is_some()
}

fn packed_legacy_profile_vertex(payload: &[u8], offset: usize) -> bool {
    packed_legacy_marker_body(payload, offset)
        && marker_profile_curve_role(payload, offset) == Some(1)
        && payload.get(offset + 48..offset + 50) == Some(&[0x1e, 0x00])
        && finite_coordinate_pair(payload, offset.saturating_add(50)).is_some()
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
        && finite_coordinate_pair(payload, offset.saturating_add(66)).is_some()
        && payload.get(offset + 82..offset + 86) == Some(&1u32.to_le_bytes())
        && payload.get(offset + 86..offset + 92) == Some(&[0; 6])
        && payload.get(offset + 92..offset + 98) == Some(&[0xfe, 0xff, 0xff, 0xff, 0x00, 0x00])
        && payload.get(offset + 98..offset + 132) == Some(&[0; 34])
        && payload
            .get(offset + 132..offset + 136)
            .is_some_and(|identity| identity != [0; 4] && identity != [0xff; 4])
        && payload.get(offset + 136..offset + 142) == Some(&[0; 6])
        && payload.get(offset + 142..offset + 146) == Some(&[0xff; 4])
        && sketch_marker_prefix_at(payload, offset.saturating_add(146))
}

fn compact_geometry_locus_point_coordinates(payload: &[u8], offset: usize) -> Option<[f64; 2]> {
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
        || !sketch_marker_prefix_at(payload, offset.saturating_add(134))
    {
        return None;
    }
    finite_coordinate_pair(payload, offset + 58)
}

fn shifted_geometry_locus_coordinates(payload: &[u8], offset: usize) -> Option<[f64; 2]> {
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
            Some([0x12 | 0x13 | 0x16, 0x00])
        )
    {
        return None;
    }
    let valid_record = [(142, 92), (146, 92), (162, 112), (177, 127), (178, 128)]
        .into_iter()
        .any(|(length, sentinel)| {
            payload.get(offset + sentinel..offset + sentinel + 4) == Some(&[0xfe, 0xff, 0xff, 0xff])
                && sketch_marker_prefix_at(payload, offset.saturating_add(length))
        });
    if !valid_record {
        return None;
    }
    finite_coordinate_pair(payload, offset + 66)
}

fn shifted_geometry_handle_coordinates(payload: &[u8], offset: usize) -> Option<[f64; 2]> {
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
        && finite_coordinate_pair(payload, offset.saturating_add(66)).is_some()
        && payload.get(offset + 92..offset + 96) == Some(&[0xfe, 0xff, 0xff, 0xff])
        && trailer
        && sketch_marker_prefix_at(payload, offset.saturating_add(142))
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
        || finite_coordinate_pair(payload, offset.saturating_add(58)).is_none()
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
        && sketch_marker_prefix_at(payload, offset.saturating_add(134));
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
        && sketch_marker_prefix_at(payload, offset.saturating_add(134));
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
        && sketch_marker_prefix_at(payload, offset.saturating_add(134));
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
        && sketch_marker_prefix_at(payload, offset.saturating_add(134));
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
        && sketch_marker_prefix_at(payload, offset.saturating_add(138));
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
    payload.get(offset..offset + LEGACY_EXTENDED_SKETCH_MARKER.len())
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
        && finite_coordinate_pair(payload, offset.saturating_add(58)).is_some()
        && payload.get(offset + 74..offset + 78) == Some(&[0x00, 0x00, 0x01, 0x00])
        && payload.get(offset + 78..offset + 82) == Some(&[0; 4])
        && payload.get(offset + 82..offset + 86) == Some(&(-1i32).to_le_bytes())
        && payload.get(offset + 86..offset + 124) == Some(&[0; 38])
        && identity(124)
        && identity(128)
        && payload.get(offset + 124..offset + 128) != payload.get(offset + 128..offset + 132)
        && payload.get(offset + 132..offset + 138) == Some(&[0x00, 0x00, 0x01, 0x00, 0x00, 0x00])
        && sketch_marker_prefix_at(payload, offset.saturating_add(138))
}

type LinkedProfilePoint = ([f64; 2], [(u16, u16); 2]);

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
        || !matches!(link_count, Some([2 | 3, 0]))
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
    let valid_tail = long_tail
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

fn additional_linked_profile_point_coordinates(payload: &[u8], offset: usize) -> Option<[f64; 2]> {
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

pub(super) fn current_reverse_incidence_endpoint_offsets(
    payload: &[u8],
    curve: &SketchInputEntity,
    markers: &[&SketchInputEntity],
) -> Option<[u64; 2]> {
    let offset = usize::try_from(curve.offset).ok()?;
    let curve_index = u16::try_from(curve.object_index?).ok()?;
    if payload.get(offset..offset + SKETCH_MARKER.len()) != Some(SKETCH_MARKER)
        || marker_native_code(payload, offset) != Some(1)
        || marker_profile_curve_role(payload, offset) != Some(1)
        || compact_indexed_curve_endpoint_indices(payload, offset).is_none()
    {
        return None;
    }
    let mut by_selector = BTreeMap::<u16, Vec<u64>>::new();
    for marker in markers
        .iter()
        .copied()
        .filter(|marker| marker.feature_ref == curve.feature_ref)
    {
        let marker_offset = usize::try_from(marker.offset).ok()?;
        let Some((_, links)) = linked_profile_point(payload, marker_offset) else {
            continue;
        };
        for (selector, linked_curve) in links {
            if linked_curve == curve_index {
                by_selector.entry(selector).or_default().push(marker.offset);
            }
        }
    }
    let mut candidates = by_selector.into_values().filter_map(|mut offsets| {
        offsets.sort_unstable();
        offsets.dedup();
        <[u64; 2]>::try_from(offsets).ok()
    });
    let endpoints = candidates.next()?;
    candidates.next().is_none().then_some(endpoints)
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
        || finite_coordinate_pair(payload, offset.saturating_add(58)).is_none()
        || payload.get(offset + 74..offset + 76) != Some(&[0; 2])
        || payload.get(offset + 102..offset + 108) != Some(&[0x00, 0x00, 0xfe, 0xff, 0xff, 0xff])
        || marker_object_index(payload, offset).is_none()
        || marker_local_id(payload, offset).is_none()
        || !sketch_marker_prefix_at(payload, offset.saturating_add(154))
    {
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
        || finite_coordinate_pair(payload, offset.saturating_add(58)).is_none()
        || payload.get(offset + 74..offset + 78) != Some(&[0x00, 0x00, 0x02, 0x00])
        || payload.get(offset + 94..offset + 100) != Some(&[0x00, 0x00, 0xfe, 0xff, 0xff, 0xff])
        || payload.get(offset + 100..offset + 142) != Some(&[0; 42])
        || payload
            .get(offset + 142..offset + 146)
            .is_none_or(|identity| identity == [0; 4] || identity == [0xff; 4])
        || !sketch_marker_prefix_at(payload, offset.saturating_add(146))
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
