// SPDX-License-Identifier: Apache-2.0
//! Typed views over `SolidWorks` `ResolvedFeatures` sketch records.

use crate::classification::{native_object_class, NativeClassKind};
use crate::records::{
    FeatureInputBodySelection, FeatureInputClass, FeatureInputClassRole,
    FeatureInputComponentPathEntry, FeatureInputEdgeSelection, FeatureInputLane, FeatureInputName,
    FeatureInputOperand, FeatureInputOperandKind, FeatureInputReference,
    FeatureInputRelationBinding, FeatureInputRelationFamily, FeatureInputRelationInstance,
    FeatureInputScalar, FeatureInputScalarRole, FeatureInputSurfaceSelection, SketchInputEntity,
    SketchInputKind, SketchInputLink, SketchRelationKind,
};
use cadmpeg_ir::annotations::Annotations;
use cadmpeg_ir::cursor::bounded_len;
use cadmpeg_ir::features::{BooleanOp, FeatureDefinition, Length, PathRef, PatternKind};
use cadmpeg_ir::geometry::{Curve, CurveGeometry, NurbsCurve, Surface, SurfaceGeometry};
use cadmpeg_ir::ids::{
    BodyId, CoedgeId, CurveId, EdgeId, FaceId, LoopId, PointId, RegionId, ShellId, SurfaceId,
    VertexId,
};
use cadmpeg_ir::math::{Point2, Point3, Vector3};
use cadmpeg_ir::sketches::{
    Sketch, SketchConstraint, SketchConstraintDefinition, SketchConstraintId, SketchEntity,
    SketchEntityId, SketchEntityUse, SketchGeometry, SketchId, SketchLocus, SketchNativeOperand,
    SpatialSketch, SpatialSketchEntity, SpatialSketchEntityId, SpatialSketchGeometry,
    SpatialSketchId,
};
use cadmpeg_ir::topology::{
    Body, BodyKind, Coedge, Edge, Face, Loop, Point, Region, Sense, Shell, Vertex,
};
use cadmpeg_ir::Exactness;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fmt::Write as _;
use std::io::{Read, Write};

use crate::container::ContainerScan;

const SKETCH_MARKER: &[u8] = &[0xff, 0xff, 0x1f, 0x00, 0x03];
const CLASS_MARKER: &[u8] = &[0xff, 0xff, 0x01, 0x00];
const NAME_MARKER: &[u8] = &[0x04, 0x80, 0xff, 0xfe, 0xff];
const SCALAR_HEADER: &[u8] = &[
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x40, 0xff, 0xff, 0xff, 0xff, 0x00, 0x00, 0x00, 0x00,
    0xff, 0xfe, 0xff, 0x00, 0x00, 0x00,
];
const SKETCH_POINT_TOLERANCE: f64 = 1.0e-9;
const SPATIAL_VERTEX_PREFIX: &[u8] = &[
    0xff, 0xfe, 0xff, 0x06, b'V', 0x00, b'e', 0x00, b'r', 0x00, b't', 0x00, b'e', 0x00, b'x', 0x00,
];

pub fn lanes(scan: &ContainerScan, annotations: &mut Annotations) -> Vec<FeatureInputLane> {
    scan.blocks
        .iter()
        .filter_map(|block| {
            let section = block.section.as_deref()?;
            if !section.to_ascii_lowercase().contains("resolvedfeatures") {
                return None;
            }
            let parent = format!("sldprt:feature-input:resolved-features#{}", block.offset);
            let classes = class_declarations(&block.payload, &parent);
            let names = object_names(&block.payload, &parent);
            let scalars = named_scalars(&block.payload, &parent, &names);
            let relation_bindings = relation_bindings(&parent, &classes, &scalars);
            let references = reference_cells(&scalars);
            let sketch_entities = sketch_input_entities(&block.payload, &parent);
            for entity in &sketch_entities {
                crate::annotations::note(
                    annotations,
                    entity.id.clone(),
                    section,
                    entity.offset,
                    "ff_ff_1f_00_03",
                    Exactness::ByteExact,
                );
            }
            let id = parent;
            crate::annotations::note(
                annotations,
                id.clone(),
                section,
                0,
                "ResolvedFeatures",
                Exactness::ByteExact,
            );
            Some(FeatureInputLane {
                id,
                configuration: configuration(section),
                native_payload: block.payload.clone(),
                classes,
                names,
                scalars,
                relation_bindings,
                relation_instances: Vec::new(),
                body_selections: Vec::new(),
                edge_selections: Vec::new(),
                surface_selections: Vec::new(),
                references,
                sketch_entities,
            })
        })
        .collect()
}

/// Project spatial sketches whose feature object contains one bounded line.
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
        if !matches!(feature.definition, FeatureDefinition::SpatialSketch { .. }) {
            continue;
        }
        let Some(native_ref) = feature.native_ref.as_deref() else {
            continue;
        };
        let Some(record) = records.get(native_ref).copied() else {
            continue;
        };
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
            if let [start, end] = vertices.as_slice() {
                candidates.push((lane, *start, *end));
            }
        }
        let [(lane, start, end)] = candidates.as_slice() else {
            continue;
        };
        if start == end {
            continue;
        }
        let sketch_id = SpatialSketchId(feature.id.0.replacen(
            ":model:feature#",
            ":model:spatial-sketch#",
            1,
        ));
        let entity_id = SpatialSketchEntityId(format!("{}:entity:0", sketch_id.0));
        sketches.push(SpatialSketch {
            id: sketch_id.clone(),
            name: feature.name.clone(),
            configuration: lane.configuration.clone(),
            entities: vec![entity_id.clone()],
            native_ref: Some(lane.id.clone()),
        });
        entities.push(SpatialSketchEntity {
            id: entity_id,
            sketch: sketch_id.clone(),
            construction: false,
            native_ref: None,
            geometry: SpatialSketchGeometry::Line {
                start: *start,
                end: *end,
            },
        });
        feature.definition = FeatureDefinition::SpatialSketch {
            sketch: Some(sketch_id),
        };
    }
    (sketches, entities)
}

pub(crate) fn spatial_vertex_coordinates(payload: &[u8]) -> Vec<Point3> {
    spatial_vertex_offsets(payload)
        .into_iter()
        .filter_map(|offset| {
            let point = Point3::new(
                f64::from_le_bytes(payload.get(offset + 45..offset + 53)?.try_into().ok()?),
                f64::from_le_bytes(payload.get(offset + 53..offset + 61)?.try_into().ok()?),
                f64::from_le_bytes(payload.get(offset + 61..offset + 69)?.try_into().ok()?),
            );
            [point.x, point.y, point.z]
                .into_iter()
                .all(f64::is_finite)
                .then_some(point)
        })
        .collect()
}

fn spatial_vertex_offsets(payload: &[u8]) -> Vec<usize> {
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

fn sketch_input_entities(payload: &[u8], parent: &str) -> Vec<SketchInputEntity> {
    let lane_key = parent.rsplit_once('#').map_or(parent, |(_, key)| key);
    payload
        .windows(SKETCH_MARKER.len())
        .enumerate()
        .filter_map(|(offset, bytes)| (bytes == SKETCH_MARKER).then_some(offset))
        .filter_map(|offset| {
            let code = u32::from_le_bytes(payload.get(offset + 17..offset + 21)?.try_into().ok()?);
            Some((offset, code))
        })
        .enumerate()
        .map(|(ordinal, (offset, code))| {
            let coordinates_m = marker_coordinates(payload, offset);
            SketchInputEntity {
                id: format!("sldprt:feature-input:sketch-entity#{lane_key}:{offset}"),
                parent: parent.to_string(),
                feature_ref: None,
                ordinal: ordinal as u32,
                offset: offset as u64,
                object_index: marker_object_index(payload, offset),
                local_id: marker_local_id(payload, offset),
                kind: SketchInputKind::from_native_code_and_layout(code, coordinates_m.is_some()),
                state_value: marker_state_value(payload, offset),
                coordinates_m,
                links: Vec::new(),
                link_selector: None,
            }
        })
        .collect()
}

pub(crate) fn relation_bindings(
    parent: &str,
    classes: &[FeatureInputClass],
    scalars: &[FeatureInputScalar],
) -> Vec<FeatureInputRelationBinding> {
    let lane_key = parent.rsplit_once('#').map_or(parent, |(_, key)| key);
    classes
        .iter()
        .filter_map(|class| {
            let family = match class.name.as_str() {
                "sgLLDist" => FeatureInputRelationFamily::LineLineDistance,
                "sgPntPntDist" => FeatureInputRelationFamily::PointPointDistance,
                "sgPntLineDist" => FeatureInputRelationFamily::PointLineDistance,
                "sgPntPntHorDist" => FeatureInputRelationFamily::PointPointHorizontalDistance,
                "sgPntPntVertDist" => FeatureInputRelationFamily::PointPointVerticalDistance,
                "sgAnglDim" => FeatureInputRelationFamily::Angle,
                "sgCircleDim" => FeatureInputRelationFamily::CircleDiameter,
                _ => return None,
            };
            let scalar = scalars
                .iter()
                .filter(|scalar| scalar.offset > class.offset)
                .min_by_key(|scalar| scalar.offset)?;
            (scalar.offset - class.offset <= 128).then_some((class, scalar, family))
        })
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

pub(crate) fn reference_cells(scalars: &[FeatureInputScalar]) -> Vec<FeatureInputReference> {
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
                object_index: operand.entity_index,
            })
        })
        .collect::<Vec<_>>();
    cells.sort_by_key(|cell| cell.offset);
    cells.dedup_by_key(|cell| cell.offset);
    for (ordinal, cell) in cells.iter_mut().enumerate() {
        cell.ordinal = ordinal as u32;
    }
    cells
}

pub(crate) fn marker_local_id(payload: &[u8], offset: usize) -> Option<u32> {
    let relative = if marker_local_links(payload, offset).is_some() {
        88
    } else if marker_coordinates(payload, offset).is_some() {
        let search_start = offset.checked_add(SKETCH_MARKER.len())?;
        let next = payload
            .get(search_start..)?
            .windows(SKETCH_MARKER.len())
            .position(|bytes| bytes == SKETCH_MARKER)?
            .checked_add(search_start)?;
        match next.checked_sub(offset)? {
            142 | 146 => 138,
            152 | 156 => 148,
            162 | 166 | 167 => 158,
            _ => return None,
        }
    } else {
        return None;
    };
    let start = offset.checked_add(relative)?;
    let end = start.checked_add(4)?;
    let id = u32::from_le_bytes(payload.get(start..end)?.try_into().ok()?);
    (id != u32::MAX).then_some(id)
}

fn marker_state_value(payload: &[u8], offset: usize) -> Option<f64> {
    let offset = offset.checked_add(48)?;
    let value = f64::from_le_bytes(payload.get(offset..offset + 8)?.try_into().ok()?);
    value.is_finite().then_some(value)
}

pub(crate) fn marker_coordinates(payload: &[u8], offset: usize) -> Option<[f64; 2]> {
    const GEOMETRY_PREFIX: [u8; 12] = [
        0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0x00, 0x00, 0x80, 0xbf,
    ];
    if payload.get(offset + 5..offset + 17)? != GEOMETRY_PREFIX
        || payload.get(offset + 64..offset + 66)? != [0x1e, 0x00]
    {
        return None;
    }
    let first = f64::from_le_bytes(payload.get(offset + 66..offset + 74)?.try_into().ok()?);
    let second = f64::from_le_bytes(payload.get(offset + 74..offset + 82)?.try_into().ok()?);
    (first.is_finite() && second.is_finite()).then_some([first, second])
}

pub(crate) fn marker_object_index(payload: &[u8], offset: usize) -> Option<u32> {
    let start = offset.checked_sub(4)?;
    let index = u32::from_le_bytes(payload.get(start..offset)?.try_into().ok()?);
    (index != u32::MAX).then_some(index)
}

pub(crate) fn marker_is_geometry_locus(payload: &[u8], offset: usize) -> bool {
    payload.get(offset + 23..offset + 27) == Some(&[0x05, 0x00, 0x01, 0x00])
}

#[cfg(test)]
mod marker_tests {
    use super::{
        append_spatial_vertex, arc_angle_relation_kind, compact_body_component_path_at,
        compact_body_path_at, compact_body_retention_mode, compact_body_selection_at,
        compact_body_selection_vector, compact_body_state_ids, compact_combine_operation_at,
        compact_edge_component_path_at, compact_edge_selection_at,
        compact_extrusion_blind_through_all_second_at, compact_extrusion_mid_plane_at,
        compact_extrusion_offset_from_face_at, compact_extrusion_through_all_at,
        compact_extrusion_through_all_both_at, compact_extrusion_through_next_at,
        compact_extrusion_to_face_at, compact_extrusion_to_vertex_at, compact_general_curve_ref_at,
        compact_line_chain_addresses, compact_line_region_addresses,
        compact_profile_reference_plane_source, compact_reference_plane_source,
        compact_single_face_reference_path_at, compact_surface_selection_at,
        complete_ordered_compact_line_profile, component_path_features,
        component_path_terminal_feature, component_profile_source_at,
        component_reference_curve_path_at, coordinate_marker_local_links, marker_coordinates,
        marker_is_geometry_locus, marker_local_id, marker_local_links, marker_object_index,
        named_scalars, native_scalar_matches_discrete_parameter, object_names,
        ordered_compact_line_profile, ordered_rectangle_corners, patch_spatial_vertex,
        principal_sketch_frame, resolve_operand_marker, resolve_operand_marker_excluding,
        resolve_scalar_operand_markers, solved_tangent, spatial_vertex_coordinates,
        unique_dimensioned_rectangle_markers, unique_locus, unique_marker_candidate,
        CompactPointReferenceKind, CLASS_MARKER, COMPACT_EDGE_VECTOR_MARKER, NAME_MARKER,
        SCALAR_HEADER,
    };
    use crate::records::{
        Feature, FeatureInputComponentPathEntry, FeatureInputOperand, FeatureInputOperandKind,
        SketchInputEntity, SketchInputKind, SketchInputLink, SketchRelationKind,
    };
    use cadmpeg_ir::math::{Point2, Point3};
    use cadmpeg_ir::sketches::{SketchEntityId, SketchGeometry, SketchLocus};
    use std::collections::HashSet;

    #[test]
    fn spatial_vertex_patch_preserves_record_shape_and_order() {
        let first = Point3::new(1.0, 2.0, 3.0);
        let second = Point3::new(4.0, 5.0, 6.0);
        let mut payload = Vec::new();
        append_spatial_vertex(&mut payload, first);
        append_spatial_vertex(&mut payload, second);

        let replacement = Point3::new(-7.5, 8.25, 9.0);
        patch_spatial_vertex(&mut payload, 0, replacement).unwrap();

        assert_eq!(
            spatial_vertex_coordinates(&payload),
            vec![replacement, second]
        );
        assert_eq!(payload.len(), 138);
    }

    #[test]
    fn marker_local_id_is_the_trailing_u32() {
        let mut payload = vec![0; 92];
        payload[72..80].copy_from_slice(&(-1.0f64).to_le_bytes());
        payload[88..92].copy_from_slice(&37u32.to_le_bytes());
        assert_eq!(marker_local_id(&payload, 0), Some(37));
        payload[88..92].fill(0xff);
        assert_eq!(marker_local_id(&payload, 0), None);
    }

    #[test]
    fn marker_object_index_precedes_the_marker() {
        let mut payload = 37u32.to_le_bytes().to_vec();
        payload.extend(super::SKETCH_MARKER);
        assert_eq!(marker_object_index(&payload, 4), Some(37));
        assert_eq!(marker_object_index(&payload, 3), None);
        payload[0..4].fill(0xff);
        assert_eq!(marker_object_index(&payload, 4), None);
    }

    #[test]
    fn compact_body_states_require_a_duplicated_local_identity() {
        let token = 0x89a4u16;
        let mut payload = vec![0; 180];
        let header = &mut payload[12..95];
        header[0..2].copy_from_slice(&token.to_le_bytes());
        header[2..11].copy_from_slice(&[0x2b, 0x80, 0x02, 0, 0, 0, 0, 0, 0]);
        header[11..15].copy_from_slice(&205u32.to_le_bytes());
        header[15..19].copy_from_slice(&205u32.to_le_bytes());
        header[47..63].fill(0xff);

        assert_eq!(compact_body_state_ids(&payload, 0, 180, token), [205]);

        payload[12 + 15..12 + 19].copy_from_slice(&206u32.to_le_bytes());
        assert!(compact_body_state_ids(&payload, 0, 180, token).is_empty());
    }

    #[test]
    fn compact_body_retention_mode_follows_the_state_roster() {
        use cadmpeg_ir::features::BodyRetentionMode::{DeleteSelected, KeepSelected};

        let token = 0x89a4u16;
        let mut payload = vec![0; 112];
        let header = &mut payload[12..95];
        header[0..2].copy_from_slice(&token.to_le_bytes());
        header[2..11].copy_from_slice(&[0x2b, 0x80, 0x02, 0, 0, 0, 0, 0, 0]);
        header[11..15].copy_from_slice(&205u32.to_le_bytes());
        header[15..19].copy_from_slice(&205u32.to_le_bytes());
        header[47..63].fill(0xff);
        payload[95..97].copy_from_slice(&[0x30, 0x80]);

        assert_eq!(
            compact_body_retention_mode(&payload, 0, payload.len(), token),
            Some(KeepSelected)
        );
        payload[97..101].copy_from_slice(&1u32.to_le_bytes());
        assert_eq!(
            compact_body_retention_mode(&payload, 0, payload.len(), token),
            Some(DeleteSelected)
        );
        payload[101] = 1;
        assert_eq!(
            compact_body_retention_mode(&payload, 0, payload.len(), token),
            None
        );
    }

    #[test]
    fn compact_line_region_is_an_ordered_one_based_curve_roster() {
        let mut payload = b"moSketchRegion_c".to_vec();
        payload.extend(0x8060u16.to_le_bytes());
        payload.extend(4u16.to_le_bytes());
        for address in [2u16, 1, 4, 3] {
            payload.extend(0x80e1u16.to_le_bytes());
            payload.extend(address.to_le_bytes());
            payload.extend([0xff; 4]);
            payload.extend([0; 4]);
        }
        assert_eq!(
            compact_line_region_addresses(&payload),
            Some(vec![2, 1, 4, 3])
        );
        payload[22] = 1;
        assert_eq!(compact_line_region_addresses(&payload), None);
    }

    #[test]
    fn compact_line_chain_is_an_ordered_one_based_vertex_roster() {
        let mut payload = Vec::new();
        payload.extend(4u16.to_le_bytes());
        for address in [3u32, 2, 1, 4] {
            payload.extend(address.to_le_bytes());
        }
        payload.extend(1u32.to_le_bytes());
        payload.extend(0u16.to_le_bytes());
        payload.extend(6u32.to_le_bytes());
        payload.extend([0xff; 4]);
        payload.extend([0; 8]);
        payload.extend(5u32.to_le_bytes());
        payload.extend(5u32.to_le_bytes());
        payload.extend([0xff, 0xfe, 0xff, 0, 0, 0]);
        payload.extend([0xff; 4]);
        assert_eq!(
            compact_line_chain_addresses(&payload),
            Some(vec![3, 2, 1, 4])
        );
        payload[24] = 4;
        assert_eq!(compact_line_chain_addresses(&payload), None);
    }

    #[test]
    fn compact_rectangle_requires_each_axis_corner_exactly_once() {
        let corners = [
            Point2::new(25.75, 14.15),
            Point2::new(-25.75, -14.15),
            Point2::new(-25.75, 14.15),
            Point2::new(25.75, -14.15),
        ];
        assert_eq!(
            ordered_rectangle_corners(&corners),
            Some([
                Point2::new(-25.75, -14.15),
                Point2::new(25.75, -14.15),
                Point2::new(25.75, 14.15),
                Point2::new(-25.75, 14.15),
            ])
        );

        let duplicate = [corners[0], corners[0], corners[2], corners[3]];
        assert_eq!(ordered_rectangle_corners(&duplicate), None);
        let non_rectangular = [
            corners[0],
            corners[1],
            corners[2],
            Point2::new(24.0, -14.15),
        ];
        assert_eq!(ordered_rectangle_corners(&non_rectangular), None);
    }

    #[test]
    fn dimensioned_rectangle_selects_one_complete_marker_product() {
        let marker = |id: &str, u, v| SketchInputEntity {
            id: id.into(),
            parent: "lane".into(),
            feature_ref: Some("feature".into()),
            ordinal: 0,
            offset: 0,
            object_index: None,
            local_id: None,
            kind: SketchInputKind::Point,
            state_value: None,
            coordinates_m: Some([u, v]),
            links: Vec::new(),
            link_selector: None,
        };
        let markers = [
            marker("center", -0.023, 0.0),
            marker("lower-left", -0.02575, -0.00425),
            marker("upper-right", -0.02025, 0.00425),
            marker("lower-right", -0.02025, -0.00425),
            marker("upper-left", -0.02575, 0.00425),
            marker("axis-top", -0.02575, 0.01415),
            marker("axis-bottom", -0.02575, -0.01415),
            marker("origin", 0.0, 0.0),
        ];
        let marker_refs = markers.iter().collect::<Vec<_>>();
        assert_eq!(
            unique_dimensioned_rectangle_markers(&marker_refs, &[8.5, 5.5])
                .map(|markers| markers.map(|marker| marker.id.as_str())),
            Some(["lower-left", "lower-right", "upper-right", "upper-left"])
        );
        assert_eq!(
            unique_dimensioned_rectangle_markers(&marker_refs, &[8.5]),
            None
        );
        assert_eq!(
            unique_dimensioned_rectangle_markers(&marker_refs, &[28.3, 5.5]),
            None
        );

        let second_rectangle = [
            marker("second-lower-left", 0.010, 0.020),
            marker("second-lower-right", 0.0155, 0.020),
            marker("second-upper-right", 0.0155, 0.0285),
            marker("second-upper-left", 0.010, 0.0285),
        ];
        let ambiguous = marker_refs
            .iter()
            .copied()
            .chain(second_rectangle.iter())
            .collect::<Vec<_>>();
        assert_eq!(
            unique_dimensioned_rectangle_markers(&ambiguous, &[8.5, 5.5]),
            None
        );
    }

    #[test]
    fn compact_line_endpoint_pairs_form_one_oriented_cycle() {
        let marker = SketchInputEntity {
            id: "marker".into(),
            parent: "lane".into(),
            feature_ref: None,
            ordinal: 0,
            offset: 0,
            object_index: None,
            local_id: None,
            kind: SketchInputKind::Point,
            state_value: None,
            coordinates_m: None,
            links: Vec::new(),
            link_selector: None,
        };
        let point = |u, v| Point2::new(u, v);
        let lines = vec![
            (
                SketchEntityId("top".into()),
                &marker,
                &marker,
                point(0.0, 1.0),
                point(1.0, 1.0),
            ),
            (
                SketchEntityId("bottom".into()),
                &marker,
                &marker,
                point(0.0, 0.0),
                point(1.0, 0.0),
            ),
            (
                SketchEntityId("right".into()),
                &marker,
                &marker,
                point(1.0, 0.0),
                point(1.0, 1.0),
            ),
            (
                SketchEntityId("left".into()),
                &marker,
                &marker,
                point(0.0, 1.0),
                point(0.0, 0.0),
            ),
        ];

        let profile = ordered_compact_line_profile(&lines).expect("closed line cycle");
        assert_eq!(
            profile
                .iter()
                .map(|use_| (use_.entity.0.as_str(), use_.reversed))
                .collect::<Vec<_>>(),
            [
                ("top", false),
                ("right", true),
                ("bottom", true),
                ("left", true)
            ]
        );
        assert_eq!(complete_ordered_compact_line_profile(&lines, 5), None);
    }

    #[test]
    fn compact_reference_plane_source_requires_the_complete_trailer() {
        let mut payload = b"moCompRefPlane_c".to_vec();
        payload.extend([0; 12]);
        let start = payload.len();
        payload.extend(2u32.to_le_bytes());
        payload.extend(0x6554f1b8u32.to_le_bytes());
        payload.extend([0, 0, 3, 0]);
        payload.extend([0; 27]);
        payload.extend(1.0f64.to_le_bytes());
        payload.extend([
            0x00, 0x00, 0x00, 0x04, 0x00, 0x00, 0x00, 0xf9, 0xff, 0xff, 0xff, 0x00, 0x00, 0x00,
            0x00, 0x65,
        ]);
        payload.extend([0; 4]);
        assert_eq!(compact_reference_plane_source(&payload), Some(2));
        payload[start + 59] ^= 1;
        assert_eq!(compact_reference_plane_source(&payload), None);
    }

    #[test]
    fn compact_profile_uses_a_unique_lane_scoped_reference_plane() {
        let mut payload = b"moCompRefPlane_c".to_vec();
        payload.extend([0; 11]);
        payload.extend(2u32.to_le_bytes());
        payload.extend(19u32.to_le_bytes());
        payload.extend([0, 0, 3, 0]);
        payload.extend([0; 27]);
        payload.extend(1.0f64.to_le_bytes());
        payload.extend([
            0x00, 0x00, 0x00, 0x04, 0x00, 0x00, 0x00, 0xf9, 0xff, 0xff, 0xff, 0x00, 0x00, 0x00,
            0x00, 0x65,
        ]);
        payload.extend([0; 80]);
        let profile_start = payload.len();
        payload.extend([0xaa; 64]);

        assert_eq!(
            compact_profile_reference_plane_source(
                &payload,
                profile_start,
                profile_start,
                payload.len(),
            ),
            Some(2)
        );
    }

    #[test]
    fn qualified_operand_falls_back_to_marker_family_ordinal() {
        let markers = [4, 8, 11]
            .into_iter()
            .enumerate()
            .map(|(ordinal, local_id)| SketchInputEntity {
                id: format!("marker-{local_id}"),
                parent: "lane".into(),
                feature_ref: Some("feature".into()),
                ordinal: ordinal as u32,
                offset: ordinal as u64,
                object_index: None,
                local_id: Some(local_id),
                kind: SketchInputKind::LineOrCircle,
                state_value: None,
                coordinates_m: None,
                links: Vec::new(),
                link_selector: None,
            })
            .collect::<Vec<_>>();
        let kind = FeatureInputOperandKind::Native(0x8386);
        assert_eq!(
            resolve_operand_marker(&markers, kind, 4).map(|marker| marker.id.as_str()),
            Some("marker-4")
        );
        assert_eq!(
            resolve_operand_marker(&markers, kind, 2).map(|marker| marker.id.as_str()),
            Some("marker-11")
        );
    }

    #[test]
    fn qualified_operand_selects_one_coordinate_marker_in_a_reused_local_id() {
        let marker = |id: &str, coordinates_m| SketchInputEntity {
            id: id.into(),
            parent: "lane".into(),
            feature_ref: Some("feature".into()),
            ordinal: 0,
            offset: 0,
            object_index: None,
            local_id: Some(7),
            kind: SketchInputKind::Point,
            state_value: None,
            coordinates_m,
            links: Vec::new(),
            link_selector: None,
        };
        let markers = [
            marker("reference", None),
            marker("geometry", Some([1.0, 2.0])),
        ];
        assert_eq!(
            resolve_operand_marker(&markers, FeatureInputOperandKind::Native(0x837b), 7,)
                .map(|marker| marker.id.as_str()),
            Some("geometry")
        );
    }

    #[test]
    fn qualified_point_operand_selects_a_curve_marker_locus() {
        let marker = SketchInputEntity {
            id: "line-locus".into(),
            parent: "lane".into(),
            feature_ref: Some("feature".into()),
            ordinal: 0,
            offset: 0,
            object_index: None,
            local_id: Some(16),
            kind: SketchInputKind::LineOrCircle,
            state_value: None,
            coordinates_m: Some([1.0, 2.0]),
            links: Vec::new(),
            link_selector: None,
        };
        for tag in [0x837b, 0xbc7c] {
            assert_eq!(
                resolve_operand_marker(
                    std::slice::from_ref(&marker),
                    FeatureInputOperandKind::Native(tag),
                    16,
                )
                .map(|resolved| resolved.id.as_str()),
                Some("line-locus")
            );
        }
        let mut markers = vec![marker];
        markers.extend((0..3).map(|index| SketchInputEntity {
            id: format!("point-{index}"),
            parent: "lane".into(),
            feature_ref: Some("feature".into()),
            ordinal: index,
            offset: u64::from(index + 1),
            object_index: None,
            local_id: Some(10 + index),
            kind: SketchInputKind::Point,
            state_value: None,
            coordinates_m: Some([f64::from(index), 0.0]),
            links: Vec::new(),
            link_selector: None,
        }));
        markers[0].local_id = Some(1);
        assert_eq!(
            resolve_operand_marker(&markers, FeatureInputOperandKind::Native(0xbc7c), 1)
                .map(|resolved| resolved.id.as_str()),
            Some("point-1")
        );
    }

    #[test]
    fn object_indexed_bc_operands_precede_local_and_ordinal_fallbacks() {
        let marker = |id: &str, offset, object_index, kind, coordinates_m| SketchInputEntity {
            id: id.into(),
            parent: "lane".into(),
            feature_ref: Some("feature".into()),
            ordinal: offset as u32,
            offset,
            object_index,
            local_id: Some(100 + offset as u32),
            kind,
            state_value: None,
            coordinates_m,
            links: Vec::new(),
            link_selector: None,
        };
        let markers = [
            marker(
                "unrelated-point",
                0,
                Some(3),
                SketchInputKind::Point,
                Some([0.0, 0.0]),
            ),
            marker(
                "indexed-curve-locus",
                1,
                Some(0),
                SketchInputKind::LineOrCircle,
                Some([1.0, 0.0]),
            ),
            marker(
                "indexed-relation",
                2,
                Some(0),
                SketchInputKind::Relation(SketchRelationKind::Distance),
                None,
            ),
            SketchInputEntity {
                local_id: Some(0),
                ..marker(
                    "local-id-curve",
                    3,
                    Some(2),
                    SketchInputKind::LineOrCircle,
                    Some([2.0, 0.0]),
                )
            },
        ];

        assert_eq!(
            resolve_operand_marker(&markers, FeatureInputOperandKind::Native(0xbc7c), 0)
                .map(|marker| marker.id.as_str()),
            Some("indexed-curve-locus")
        );
        assert_eq!(
            resolve_operand_marker(&markers, FeatureInputOperandKind::Native(0xbc87), 0)
                .map(|marker| marker.id.as_str()),
            Some("indexed-curve-locus")
        );
    }

    #[test]
    fn point_operand_follows_relation_handle_graph_and_excludes_its_sibling() {
        let marker = |id: &str, local_id, kind, links: &[&str]| SketchInputEntity {
            id: id.into(),
            parent: "lane".into(),
            feature_ref: Some("feature".into()),
            ordinal: 0,
            offset: 0,
            object_index: None,
            local_id,
            kind,
            state_value: None,
            coordinates_m: None,
            links: links
                .iter()
                .map(|target| SketchInputLink {
                    local_id: 0,
                    entity_ref: (*target).into(),
                })
                .collect(),
            link_selector: None,
        };
        let markers = [
            marker("first", Some(5), SketchInputKind::Point, &[]),
            marker("second", Some(1), SketchInputKind::Point, &[]),
            marker(
                "relation-2",
                Some(2),
                SketchInputKind::Relation(SketchRelationKind::Distance),
                &["relation-0"],
            ),
            marker(
                "relation-0",
                Some(0),
                SketchInputKind::Relation(SketchRelationKind::Distance),
                &["second"],
            ),
        ];
        let operands = [
            FeatureInputOperand {
                kind: FeatureInputOperandKind::D6,
                entity_index: 0,
                offset: 0,
                reference_ref: "first-ref".into(),
                entity_ref: None,
            },
            FeatureInputOperand {
                kind: FeatureInputOperandKind::D6,
                entity_index: 2,
                offset: 0,
                reference_ref: "second-ref".into(),
                entity_ref: None,
            },
        ];
        let resolved = resolve_scalar_operand_markers(&markers, &operands);
        assert_eq!(resolved[0].map(|marker| marker.id.as_str()), Some("first"));
        assert_eq!(resolved[1].map(|marker| marker.id.as_str()), Some("second"));

        let duplicate = [
            operands[1].clone(),
            FeatureInputOperand {
                kind: FeatureInputOperandKind::D6,
                entity_index: 1,
                offset: 0,
                reference_ref: "known-second-ref".into(),
                entity_ref: None,
            },
        ];
        let resolved = resolve_scalar_operand_markers(&markers, &duplicate);
        assert_eq!(resolved[0].map(|marker| marker.id.as_str()), Some("first"));
        assert_eq!(resolved[1].map(|marker| marker.id.as_str()), Some("second"));
    }

    #[test]
    fn curve_operand_selects_an_arc_by_local_identifier() {
        let markers = [
            SketchInputEntity {
                id: "line-11".into(),
                parent: "lane".into(),
                feature_ref: Some("feature".into()),
                ordinal: 0,
                offset: 0,
                object_index: None,
                local_id: Some(11),
                kind: SketchInputKind::LineOrCircle,
                state_value: None,
                coordinates_m: Some([0.0, 0.0]),
                links: Vec::new(),
                link_selector: None,
            },
            SketchInputEntity {
                id: "arc-3".into(),
                parent: "lane".into(),
                feature_ref: Some("feature".into()),
                ordinal: 1,
                offset: 1,
                object_index: None,
                local_id: Some(3),
                kind: SketchInputKind::Arc,
                state_value: None,
                coordinates_m: Some([1.0, 1.0]),
                links: Vec::new(),
                link_selector: None,
            },
        ];
        assert_eq!(
            resolve_operand_marker(&markers, FeatureInputOperandKind::Native(0x8dda), 3,)
                .map(|marker| marker.id.as_str()),
            Some("arc-3")
        );
    }

    #[test]
    fn curve_operand_follows_a_unique_local_reference_handle() {
        let markers = [
            SketchInputEntity {
                id: "line-11".into(),
                parent: "lane".into(),
                feature_ref: Some("feature".into()),
                ordinal: 0,
                offset: 0,
                object_index: None,
                local_id: Some(11),
                kind: SketchInputKind::LineOrCircle,
                state_value: None,
                coordinates_m: Some([0.0, 0.0]),
                links: Vec::new(),
                link_selector: None,
            },
            SketchInputEntity {
                id: "arc-8".into(),
                parent: "lane".into(),
                feature_ref: Some("feature".into()),
                ordinal: 1,
                offset: 1,
                object_index: None,
                local_id: Some(8),
                kind: SketchInputKind::Arc,
                state_value: None,
                coordinates_m: Some([1.0, 1.0]),
                links: Vec::new(),
                link_selector: None,
            },
            SketchInputEntity {
                id: "reference-3".into(),
                parent: "lane".into(),
                feature_ref: Some("feature".into()),
                ordinal: 2,
                offset: 2,
                object_index: None,
                local_id: Some(3),
                kind: SketchInputKind::Relation(SketchRelationKind::Angle),
                state_value: None,
                coordinates_m: None,
                links: vec![crate::records::SketchInputLink {
                    local_id: 8,
                    entity_ref: "arc-8".into(),
                }],
                link_selector: Some(0),
            },
        ];
        assert_eq!(
            resolve_operand_marker(&markers, FeatureInputOperandKind::Native(0x8dda), 3,)
                .map(|marker| marker.id.as_str()),
            Some("arc-8")
        );
    }

    #[test]
    fn curve_operand_excludes_an_already_resolved_sibling_from_a_reference_handle() {
        let curve = |id: &str, local_id, offset| SketchInputEntity {
            id: id.into(),
            parent: "lane".into(),
            feature_ref: Some("feature".into()),
            ordinal: offset as u32,
            offset,
            object_index: None,
            local_id: Some(local_id),
            kind: SketchInputKind::LineOrCircle,
            state_value: None,
            coordinates_m: Some([offset as f64, 0.0]),
            links: Vec::new(),
            link_selector: None,
        };
        let markers = [
            curve("curve-7", 7, 0),
            curve("curve-5", 5, 1),
            SketchInputEntity {
                id: "reference-10".into(),
                parent: "lane".into(),
                feature_ref: Some("feature".into()),
                ordinal: 2,
                offset: 2,
                object_index: None,
                local_id: Some(10),
                kind: SketchInputKind::Relation(SketchRelationKind::Distance),
                state_value: None,
                coordinates_m: None,
                links: vec![
                    crate::records::SketchInputLink {
                        local_id: 7,
                        entity_ref: "curve-7".into(),
                    },
                    crate::records::SketchInputLink {
                        local_id: 5,
                        entity_ref: "curve-5".into(),
                    },
                ],
                link_selector: Some(0),
            },
        ];
        assert!(
            resolve_operand_marker(&markers, FeatureInputOperandKind::Native(0x8386), 10).is_none()
        );
        assert_eq!(
            resolve_operand_marker_excluding(
                &markers,
                FeatureInputOperandKind::Native(0x8386),
                10,
                &HashSet::from(["curve-7".into()]),
            )
            .map(|marker| marker.id.as_str()),
            Some("curve-5")
        );
    }

    #[test]
    fn exact_local_operand_excludes_an_already_resolved_sibling() {
        let point = |id: &str, offset| SketchInputEntity {
            id: id.into(),
            parent: "lane".into(),
            feature_ref: Some("feature".into()),
            ordinal: offset as u32,
            offset,
            object_index: None,
            local_id: Some(3),
            kind: SketchInputKind::Point,
            state_value: None,
            coordinates_m: Some([offset as f64, 0.0]),
            links: Vec::new(),
            link_selector: None,
        };
        let markers = [point("first", 0), point("second", 1)];
        assert_eq!(
            resolve_operand_marker_excluding(
                &markers,
                FeatureInputOperandKind::Native(0xbc7c),
                3,
                &HashSet::from(["first".into()]),
            )
            .map(|marker| marker.id.as_str()),
            Some("second")
        );
    }

    #[test]
    fn generated_arc_angles_use_only_exact_native_quadrants() {
        assert_eq!(
            arc_angle_relation_kind(std::f64::consts::FRAC_PI_2),
            Some(SketchRelationKind::ArcAngle90)
        );
        assert_eq!(
            arc_angle_relation_kind(std::f64::consts::PI),
            Some(SketchRelationKind::ArcAngle180)
        );
        assert_eq!(
            arc_angle_relation_kind(3.0 * std::f64::consts::FRAC_PI_2),
            Some(SketchRelationKind::ArcAngle270)
        );
        assert_eq!(arc_angle_relation_kind(std::f64::consts::FRAC_PI_3), None);
    }

    #[test]
    fn compact_extrusion_through_all_requires_the_complete_end_spec() {
        let mut payload = vec![0; 104];
        payload[..2].copy_from_slice(&[0x0c, 0x8e]);
        payload[4] = 1;
        payload[18] = 1;
        payload[30..34].copy_from_slice(&[1, 0, 0, 1]);
        payload[92] = 1;
        assert!(compact_extrusion_through_all_at(&payload, 0));

        payload[18] = 0;
        assert!(!compact_extrusion_through_all_at(&payload, 0));
        payload[18] = 1;
        payload[103] = 1;
        assert!(!compact_extrusion_through_all_at(&payload, 0));
    }

    #[test]
    fn compact_extrusion_to_face_requires_a_single_face_reference_child() {
        let mut payload = vec![0; 200];
        payload[..2].copy_from_slice(&[0x0c, 0x8e]);
        payload[4] = 1;
        payload[18] = 4;
        payload[30..33].copy_from_slice(&[1, 1, 0]);
        payload[33..35].copy_from_slice(&[0x7f, 0x9d]);
        payload[35..46].copy_from_slice(&[0x2d, 0x80, 0x2b, 0x80, 2, 0, 0, 0, 0x40, 0, 0]);
        payload[88..92].copy_from_slice(&1u32.to_le_bytes());
        payload[92..96].copy_from_slice(&[0, 2, 0, 0]);
        payload[100..116].copy_from_slice(&COMPACT_EDGE_VECTOR_MARKER);
        payload[118..122].copy_from_slice(&[0x32, 0x80, 0, 0]);
        payload[122..134].fill(1);
        payload[134..138].copy_from_slice(&7u32.to_le_bytes());
        assert_eq!(compact_extrusion_to_face_at(&payload, 0), Some(100));
        let path = compact_single_face_reference_path_at(&payload, 100).unwrap();
        assert_eq!(path.len(), 1);
        assert_eq!(path[0].instance, 0x8032);
        assert_eq!(path[0].type_signature, [1; 12]);
        assert_eq!(path[0].local_id, 7);

        payload[12] = 1;
        payload[22] = 1;
        assert_eq!(compact_extrusion_to_face_at(&payload, 0), Some(100));

        payload[88..92].fill(0);
        assert_eq!(compact_extrusion_to_face_at(&payload, 0), None);
    }

    #[test]
    fn compact_extrusion_through_next_shares_the_traversal_tail() {
        let mut payload = vec![0; 104];
        payload[..2].copy_from_slice(&[0x0c, 0x8e]);
        payload[4] = 1;
        payload[18] = 2;
        payload[30..34].copy_from_slice(&[1, 0, 0, 1]);
        payload[92] = 1;
        assert!(compact_extrusion_through_next_at(&payload, 0));
        assert!(!compact_extrusion_through_all_at(&payload, 0));

        payload[18] = 1;
        assert!(compact_extrusion_through_all_at(&payload, 0));
        assert!(!compact_extrusion_through_next_at(&payload, 0));
        payload[18] = 2;
        payload[103] = 1;
        assert!(!compact_extrusion_through_next_at(&payload, 0));
    }

    #[test]
    fn compact_extrusion_mid_plane_requires_the_dimension_child() {
        let dimension_tail = |payload: &mut Vec<u8>| {
            let block = payload.len();
            payload.resize(block + 16, 0);
            payload[block + 9] = 0x20;
            payload.extend_from_slice(&[0xff, 0xff, 0, 0, 3]);
            payload.extend_from_slice(&[0xff, 0xff, 0xff, 0xff]);
            payload.extend_from_slice(&[0, 0, 0, 0, 0, 0, 0x80, 0xbf]);
        };

        let mut payload = vec![0; 26];
        payload[..2].copy_from_slice(&[0x0c, 0x8e]);
        payload[4] = 1;
        payload[18] = 6;
        payload.extend_from_slice(&[0x6a, 0x81]);
        dimension_tail(&mut payload);
        assert!(compact_extrusion_mid_plane_at(&payload, 0));

        payload[18] = 5;
        assert!(!compact_extrusion_mid_plane_at(&payload, 0));
        payload[18] = 6;
        let last = payload.len() - 1;
        payload[last] = 0;
        assert!(!compact_extrusion_mid_plane_at(&payload, 0));

        let mut payload = vec![0; 26];
        payload[..2].copy_from_slice(&[0x0c, 0x8e]);
        payload[4] = 1;
        payload[18] = 6;
        payload.extend_from_slice(b"\xff\xff\x01\x00\x16\x00moDisplayDistanceDim_c");
        dimension_tail(&mut payload);
        assert!(compact_extrusion_mid_plane_at(&payload, 0));
    }

    #[test]
    fn inline_operation_binds_join_and_cut_to_their_family_words() {
        use super::{feature_inline_operation, feature_inline_operation_fields};
        use crate::records::{FeatureInputLane, FeatureInputName};
        use cadmpeg_ir::features::BooleanOp;

        let value = "F";
        let name_offset = 10usize;
        let mut payload = vec![0; 40];
        let trailer = name_offset + 6 + 2;
        payload[trailer + 4] = 0x40;
        payload[trailer + 5] = 1;
        payload[trailer + 7] = 0xc0;
        payload[trailer + 8..trailer + 12].copy_from_slice(&7u32.to_le_bytes());
        payload[trailer + 16..trailer + 19].copy_from_slice(&[0xff, 0xfe, 0xff]);
        let lane = FeatureInputLane {
            id: "lane".into(),
            configuration: None,
            native_payload: payload,
            classes: Vec::new(),
            names: Vec::new(),
            scalars: Vec::new(),
            relation_bindings: Vec::new(),
            relation_instances: Vec::new(),
            body_selections: Vec::new(),
            edge_selections: Vec::new(),
            surface_selections: Vec::new(),
            references: Vec::new(),
            sketch_entities: Vec::new(),
        };
        let name = FeatureInputName {
            id: "name".into(),
            parent: "lane".into(),
            ordinal: 0,
            offset: name_offset as u64,
            value: value.into(),
            object_id: Some(7),
        };
        let mut lane = lane;
        assert_eq!(
            feature_inline_operation(&lane, &name),
            Some(BooleanOp::Join)
        );
        // A zero operation byte on an moICE_c object carries no operation.
        lane.native_payload[trailer + 4] = 0xca;
        assert_eq!(feature_inline_operation(&lane, &name), None);
        assert!(feature_inline_operation_fields(&lane, &name).is_some());
        lane.native_payload[trailer + 6] = 2;
        assert_eq!(feature_inline_operation(&lane, &name), Some(BooleanOp::Cut));
        lane.native_payload[trailer + 4] = 0x40;
        assert_eq!(feature_inline_operation(&lane, &name), None);
        lane.native_payload[trailer + 6] = 3;
        assert_eq!(feature_inline_operation_fields(&lane, &name), None);
    }

    #[test]
    fn compact_extrusion_through_all_both_accepts_both_carriers() {
        let dimension_tail = |payload: &mut Vec<u8>| {
            let block = payload.len();
            payload.resize(block + 16, 0);
            payload[block + 9] = 0x20;
            payload.extend_from_slice(&[0xff, 0xff, 0, 0, 3]);
            payload.extend_from_slice(&[0xff, 0xff, 0xff, 0xff]);
            payload.extend_from_slice(&[0, 0, 0, 0, 0, 0, 0x80, 0xbf]);
        };

        // Traversal carrier: first-direction code 1 with second-direction 1.
        let mut payload = vec![0; 104];
        payload[..2].copy_from_slice(&[0x0c, 0x8e]);
        payload[18] = 1;
        payload[22] = 1;
        payload[30..34].copy_from_slice(&[1, 0, 0, 1]);
        payload[92] = 1;
        assert!(compact_extrusion_through_all_both_at(&payload, 0));
        assert!(!compact_extrusion_through_all_at(&payload, 0));
        payload[8] = 1;
        assert!(compact_extrusion_through_all_both_at(&payload, 0));
        payload[8] = 2;
        assert!(!compact_extrusion_through_all_both_at(&payload, 0));
        payload[8] = 0;
        payload[22] = 0;
        assert!(!compact_extrusion_through_all_both_at(&payload, 0));
        payload[22] = 1;
        payload[18] = 2;
        assert!(!compact_extrusion_through_all_both_at(&payload, 0));

        // Dedicated code 9 carrier with the retained dimension child.
        let mut payload = vec![0; 26];
        payload[..2].copy_from_slice(&[0x0c, 0x8e]);
        payload[18] = 9;
        payload[22] = 1;
        payload.extend_from_slice(&[0x6a, 0x81]);
        dimension_tail(&mut payload);
        assert!(compact_extrusion_through_all_both_at(&payload, 0));
        payload[22] = 0;
        assert!(!compact_extrusion_through_all_both_at(&payload, 0));
        payload[22] = 1;
        payload[4] = 2;
        assert!(!compact_extrusion_through_all_both_at(&payload, 0));
    }

    #[test]
    fn compact_extrusion_blind_second_direction_requires_the_dimension_child() {
        let mut payload = vec![0; 26];
        payload[..2].copy_from_slice(&[0x0c, 0x8e]);
        payload[4] = 1;
        payload[22] = 1;
        payload.extend_from_slice(&[0x6a, 0x81]);
        let block = payload.len();
        payload.resize(block + 16, 0);
        payload[block + 9] = 0x20;
        payload.extend_from_slice(&[0xff, 0xff, 0, 0, 3]);
        payload.extend_from_slice(&[0xff, 0xff, 0xff, 0xff]);
        payload.extend_from_slice(&[0, 0, 0, 0, 0, 0, 0x80, 0xbf]);
        assert!(compact_extrusion_blind_through_all_second_at(&payload, 0));
        assert!(!compact_extrusion_through_all_both_at(&payload, 0));
        payload[22] = 0;
        assert!(!compact_extrusion_blind_through_all_second_at(&payload, 0));
        payload[22] = 1;
        payload[4] = 0;
        assert!(!compact_extrusion_blind_through_all_second_at(&payload, 0));
        payload[4] = 1;
        let last = payload.len() - 1;
        payload[last] = 0;
        assert!(!compact_extrusion_blind_through_all_second_at(&payload, 0));
    }

    #[test]
    fn end_spec_headers_require_the_anchor_class_identity() {
        let mut payload = vec![0; 104];
        payload[4] = 1;
        payload[18] = 1;
        payload[30..34].copy_from_slice(&[1, 0, 0, 1]);
        payload[92] = 1;
        // Header-shaped run without a class token or declaration at the anchor
        // is a fillet edge-set impostor, not an end spec.
        assert!(!compact_extrusion_through_all_at(&payload, 0));
        payload[..2].copy_from_slice(&[0x0c, 0x8e]);
        assert!(compact_extrusion_through_all_at(&payload, 0));
        payload[..2].copy_from_slice(&[0xff, 0xff]);
        assert!(!compact_extrusion_through_all_at(&payload, 0));

        let mut payload = vec![0; 15];
        payload.extend_from_slice(&vec![0; 104]);
        payload[15 + 4] = 1;
        payload[15 + 18] = 1;
        payload[15 + 30..15 + 34].copy_from_slice(&[1, 0, 0, 1]);
        payload[15 + 92] = 1;
        assert!(!compact_extrusion_through_all_at(&payload, 15));
        payload[..17].copy_from_slice(b"\xff\xff\x01\x00\x0b\x00moEndSpec_c");
        assert!(compact_extrusion_through_all_at(&payload, 15));
    }

    fn selection_vector_tail(payload: &mut Vec<u8>, entries: &[u32]) -> usize {
        payload.extend_from_slice(&(entries.len() as u32).to_le_bytes());
        payload.extend_from_slice(&[0, 2, 0, 0]);
        payload.extend_from_slice(&[0, 0, 0, 0]);
        let marker = payload.len();
        payload.extend_from_slice(&COMPACT_EDGE_VECTOR_MARKER);
        payload.extend_from_slice(&[0, 0]);
        for local_id in entries {
            payload.extend_from_slice(&[0x32, 0x80, 0, 0]);
            payload.extend_from_slice(&[1; 12]);
            payload.extend_from_slice(&local_id.to_le_bytes());
        }
        marker
    }

    #[test]
    fn compact_extrusion_to_vertex_accepts_both_point_reference_forms() {
        // Variant A, repeated-token form.
        let mut payload = vec![0; 30];
        payload[..2].copy_from_slice(&[0x0c, 0x8e]);
        payload[4] = 1;
        payload[18] = 3;
        payload.extend_from_slice(&[0x82, 0x92, 0x2b, 0x80, 2, 0, 0, 0, 0, 0, 0]);
        payload.extend_from_slice(&[0; 12]);
        let marker = selection_vector_tail(&mut payload, &[4, 7]);
        let (found, kind) = compact_extrusion_to_vertex_at(&payload, 0).unwrap();
        assert_eq!(found, marker);
        assert_eq!(kind, CompactPointReferenceKind::Point);
        let path = compact_single_face_reference_path_at(&payload, marker).unwrap();
        assert_eq!(path.last().unwrap().local_id, 7);

        // A to-face selector byte is not a point reference.
        payload[38] = 0x40;
        assert_eq!(compact_extrusion_to_vertex_at(&payload, 0), None);
        payload[38] = 0;
        payload[18] = 4;
        assert_eq!(compact_extrusion_to_vertex_at(&payload, 0), None);
        payload[18] = 3;

        // Variant B, edge endpoint reference.
        let mut payload = vec![0; 30];
        payload[..2].copy_from_slice(&[0x0c, 0x8e]);
        payload[4] = 1;
        payload[18] = 3;
        payload.extend_from_slice(b"\xff\xff\x01\x00\x0f\x00moEndPointRef_w");
        payload.extend_from_slice(b"\xff\xff\x01\x00\x0c\x00moCompEdge_c");
        payload.extend_from_slice(&[0xcb, 0x80, 2, 0, 0, 0, 0x40, 0, 0]);
        payload.extend_from_slice(&[0; 12]);
        let marker = selection_vector_tail(&mut payload, &[2]);
        let (found, kind) = compact_extrusion_to_vertex_at(&payload, 0).unwrap();
        assert_eq!(found, marker);
        assert_eq!(kind, CompactPointReferenceKind::EdgeEndpoint);
    }

    #[test]
    fn compact_extrusion_offset_from_face_requires_the_late_face_reference() {
        let mut payload = vec![0; 26];
        payload[..2].copy_from_slice(&[0x0c, 0x8e]);
        payload[4] = 1;
        payload[18] = 5;
        payload.extend_from_slice(&[0x6a, 0x81]);
        let block = payload.len();
        payload.resize(block + 16, 0);
        payload[block + 9] = 0x20;
        payload.extend_from_slice(&[0xff, 0xff, 0, 0, 3]);
        payload.extend_from_slice(&[0xff, 0xff, 0xff, 0xff]);
        payload.extend_from_slice(&[0, 0, 0, 0, 0, 0, 0x80, 0xbf]);
        payload.extend_from_slice(&[0; 40]);
        payload.extend_from_slice(&[1, 1, 0]);
        payload.extend_from_slice(b"\xff\xff\x01\x00\x11\x00moSingleFaceRef_w");
        payload.extend_from_slice(&[0xf2, 0x82, 0xe6, 0x80, 2, 0, 0, 0, 0x40, 0, 0]);
        payload.extend_from_slice(&[0; 8]);
        let marker = selection_vector_tail(&mut payload, &[9]);
        let end = payload.len();
        assert_eq!(
            compact_extrusion_offset_from_face_at(&payload, 0, end),
            Some(marker)
        );

        // Wrong code or a missing face-reference anchor yields no detection.
        payload[18] = 6;
        assert_eq!(
            compact_extrusion_offset_from_face_at(&payload, 0, end),
            None
        );
        payload[18] = 5;
        let anchor = payload
            .windows(3)
            .position(|window| window == [1, 1, 0])
            .unwrap();
        payload[anchor] = 0;
        assert_eq!(
            compact_extrusion_offset_from_face_at(&payload, 0, end),
            None
        );
    }

    #[test]
    fn object_names_follow_the_lane_name_class_token() {
        let mut payload = vec![0x42, 0, 0, 0, 0x13, 0];
        payload.extend_from_slice(CLASS_MARKER);
        payload.extend_from_slice(&18u16.to_le_bytes());
        payload.extend_from_slice(b"moFavoriteFolder_c");
        payload.extend_from_slice(&[0x87, 0x80, 0xff, 0xfe, 0xff]);
        payload.push(9);
        for unit in "Favorites".encode_utf16() {
            payload.extend_from_slice(&unit.to_le_bytes());
        }
        payload.resize(payload.len() + 12, 0);
        payload.extend_from_slice(&[0x87, 0x80, 0xff, 0xfe, 0xff]);
        payload.push(4);
        for unit in "Boss".encode_utf16() {
            payload.extend_from_slice(&unit.to_le_bytes());
        }
        payload.resize(payload.len() + 12, 0);

        let names = object_names(&payload, "lane");
        assert_eq!(
            names
                .iter()
                .map(|name| name.value.as_str())
                .collect::<Vec<_>>(),
            ["Favorites", "Boss"]
        );
    }

    #[test]
    fn compact_general_curve_reference_requires_the_nested_profile_prefix() {
        let mut payload = vec![0; 24];
        payload[2..4].copy_from_slice(&0xe1u16.to_le_bytes());
        payload[6..8].copy_from_slice(&0x802du16.to_le_bytes());
        payload[8..18].copy_from_slice(&[0x2b, 0x80, 0x02, 0, 0, 0, 0, 0, 0, 0]);
        assert!(compact_general_curve_ref_at(&payload, 2));
        payload[12] = 1;
        assert!(!compact_general_curve_ref_at(&payload, 2));
    }

    #[test]
    fn general_curve_component_profile_requires_a_complete_reference_record() {
        let mut payload = vec![0; 192];
        let prefix = 24;
        payload[prefix..prefix + 10].copy_from_slice(&[0x2b, 0x80, 0x02, 0, 0, 0, 0, 0, 0, 0]);
        payload[prefix + 45..prefix + 61].fill(0xff);
        let source = prefix + 81;
        payload[source..source + 4].copy_from_slice(&134u32.to_le_bytes());
        payload[source + 4..source + 8].copy_from_slice(&0x5edf_5674u32.to_le_bytes());
        payload[source + 16..source + 20].copy_from_slice(&0x65u32.to_le_bytes());
        payload[source + 24..source + 28].fill(0xff);
        for at in [source + 32, source + 36, source + 40] {
            payload[at..at + 4].copy_from_slice(&[0xc7, 0xcf, 0xff, 0xff]);
        }
        payload[source + 48..source + 52].copy_from_slice(&[0xf8, 0x2a, 0, 0]);

        assert_eq!(component_profile_source_at(&payload, prefix), Some(134));
        payload[source + 40] ^= 1;
        assert_eq!(component_profile_source_at(&payload, prefix), None);
    }

    #[test]
    fn component_reference_curve_accepts_count_minus_one_with_instance_separator() {
        let marker = 24;
        let mut payload = vec![0; 180];
        payload[marker - 12..marker - 8].copy_from_slice(&5u32.to_le_bytes());
        payload[marker - 8..marker - 4].copy_from_slice(&[4, 2, 0, 0]);
        payload[marker..marker + 16].copy_from_slice(&COMPACT_EDGE_VECTOR_MARKER);
        let mut cursor = marker + 18;
        let mut signature = [0u8; 12];
        signature[4..8].copy_from_slice(&137u32.to_le_bytes());
        for (index, instance) in [0x8c20u16, 0x8c25, 0x8c1a, 0x8c15].into_iter().enumerate() {
            if index == 1 {
                payload[cursor..cursor + 6].copy_from_slice(&[1, 0, 0, 0, 0, 0]);
                cursor += 6;
            }
            payload[cursor..cursor + 2].copy_from_slice(&instance.to_le_bytes());
            payload[cursor + 4..cursor + 16].copy_from_slice(&signature);
            payload[cursor + 16..cursor + 20].copy_from_slice(&1u32.to_le_bytes());
            cursor += 20;
        }
        payload[cursor + 8..cursor + 12].copy_from_slice(&[0xf8, 0x2a, 0, 0]);

        let components = component_reference_curve_path_at(&payload, marker).unwrap();
        assert_eq!(components.len(), 4);
        assert_eq!(components[0].instance, 0x8c20);
        assert!(components.iter().all(|component| component.local_id == 1));

        payload[cursor + 8] ^= 1;
        assert_eq!(component_reference_curve_path_at(&payload, marker), None);
    }

    #[test]
    fn scalar_trailer_is_relative_to_variable_length_name() {
        let mut payload = Vec::new();
        payload.extend_from_slice(NAME_MARKER);
        payload.push(3);
        for unit in "D10".encode_utf16() {
            payload.extend_from_slice(&unit.to_le_bytes());
        }
        payload.extend_from_slice(SCALAR_HEADER);
        payload.extend_from_slice(&0.025f64.to_le_bytes());
        let trailer = payload.len();
        payload.resize(trailer + 59, 0);
        payload[trailer + 3..trailer + 7].copy_from_slice(&42u32.to_le_bytes());
        payload[trailer + 24..trailer + 29].copy_from_slice(&[0, 0, 0, 2, 0]);
        for (relative, index) in [(35usize, 7u16), (47, 9)] {
            payload[trailer + relative..trailer + relative + 2].copy_from_slice(&[0xd6, 0x80]);
            payload[trailer + relative + 2..trailer + relative + 4]
                .copy_from_slice(&index.to_le_bytes());
            payload[trailer + relative + 4..trailer + relative + 8].fill(0xff);
        }
        let names = object_names(&payload, "lane");
        let scalars = named_scalars(&payload, "lane", &names);
        let [scalar] = scalars.as_slice() else {
            panic!("expected one scalar");
        };
        assert_eq!(scalar.object_id, 42);
        assert_eq!(scalar.role, crate::records::FeatureInputScalarRole::Driving);
        assert_eq!(scalar.entity_indices, [7, 9]);
    }

    #[test]
    fn legacy_scalar_layout_carries_shifted_role_and_operand() {
        let mut payload = Vec::new();
        payload.extend_from_slice(NAME_MARKER);
        payload.push(2);
        for unit in "D1".encode_utf16() {
            payload.extend_from_slice(&unit.to_le_bytes());
        }
        payload.extend_from_slice(SCALAR_HEADER);
        payload.extend_from_slice(&0.004f64.to_le_bytes());
        let trailer = payload.len();
        payload.resize(trailer + 48, 0);
        payload[trailer + 3..trailer + 7].copy_from_slice(&28u32.to_le_bytes());
        payload[trailer + 24..trailer + 30].copy_from_slice(&[0x0f, 0, 0, 0, 2, 0]);
        payload[trailer + 30] = 0;
        payload[trailer + 36..trailer + 38].copy_from_slice(&[0xcc, 0x80]);
        payload[trailer + 38..trailer + 40].copy_from_slice(&0u16.to_le_bytes());
        payload[trailer + 40..trailer + 44].fill(0xff);

        let names = object_names(&payload, "lane");
        let scalars = named_scalars(&payload, "lane", &names);
        let [scalar] = scalars.as_slice() else {
            panic!("expected one scalar");
        };
        assert_eq!(scalar.role, crate::records::FeatureInputScalarRole::Driving);
        assert_eq!(scalar.operands.len(), 1);
        assert_eq!(scalar.operands[0].offset, (trailer + 36) as u64);
        assert_eq!(
            scalar.operands[0].kind,
            crate::records::FeatureInputOperandKind::Native(0x80cc)
        );
        assert_eq!(scalar.operands[0].entity_index, 0);
    }

    #[test]
    fn coordinate_marker_local_id_uses_the_variant_footer() {
        let mut payload = vec![0; 142 + 5];
        payload[..5].copy_from_slice(super::SKETCH_MARKER);
        payload[5..13].fill(0xff);
        payload[13..17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
        payload[64..66].copy_from_slice(&[0x1e, 0x00]);
        payload[138..142].copy_from_slice(&41u32.to_le_bytes());
        payload[142..147].copy_from_slice(super::SKETCH_MARKER);
        assert_eq!(marker_local_id(&payload, 0), Some(41));
    }

    #[test]
    fn geometry_marker_coordinates_are_selected_by_layout() {
        let mut payload = vec![0; 82];
        payload[5..13].fill(0xff);
        payload[13..17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
        payload[17..21].copy_from_slice(&10u32.to_le_bytes());
        payload[64..66].copy_from_slice(&[0x1e, 0x00]);
        payload[66..74].copy_from_slice(&1.25f64.to_le_bytes());
        payload[74..82].copy_from_slice(&(-2.5f64).to_le_bytes());
        assert_eq!(marker_coordinates(&payload, 0), Some([1.25, -2.5]));
        payload[64..66].copy_from_slice(&[0x14, 0x00]);
        assert_eq!(marker_coordinates(&payload, 0), None);
        payload[64..66].copy_from_slice(&[0x1e, 0x00]);
        payload[5] = 0;
        assert_eq!(marker_coordinates(&payload, 0), None);
    }

    #[test]
    fn geometry_locus_role_excludes_display_handles() {
        let mut payload = vec![0; 27];
        payload[23..27].copy_from_slice(&[0x05, 0x00, 0x01, 0x00]);
        assert!(marker_is_geometry_locus(&payload, 0));
        payload[23..27].copy_from_slice(&[0x04, 0x00, 0x02, 0x00]);
        assert!(!marker_is_geometry_locus(&payload, 0));
        payload[23..27].copy_from_slice(&[0x05, 0x00, 0x02, 0x00]);
        assert!(!marker_is_geometry_locus(&payload, 0));
    }

    #[test]
    fn local_links_require_the_reference_trailer() {
        let mut payload = vec![0; 80];
        payload[64..66].copy_from_slice(&37u16.to_le_bytes());
        payload[66..68].copy_from_slice(&39u16.to_le_bytes());
        payload[68..70].copy_from_slice(&1u16.to_le_bytes());
        payload[72..80].copy_from_slice(&(-1.0f64).to_le_bytes());
        assert_eq!(marker_local_links(&payload, 0), Some(([37, 39], 1)));
        payload[70] = 1;
        assert_eq!(marker_local_links(&payload, 0), None);
        payload[70] = 0;
        payload[72..80].copy_from_slice(&0.0f64.to_le_bytes());
        assert_eq!(marker_local_links(&payload, 0), None);
        payload[5..17].copy_from_slice(&[
            0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0x00, 0x00, 0x80, 0xbf,
        ]);
        payload[64..66].copy_from_slice(&[0x1e, 0x00]);
        payload[72..80].copy_from_slice(&(-1.0f64).to_le_bytes());
        assert_eq!(marker_local_links(&payload, 0), None);
    }

    #[test]
    fn coordinate_marker_links_are_counted_reference_cells() {
        let mut payload = vec![0; 118];
        payload[5..13].fill(0xff);
        payload[13..17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
        payload[64..66].copy_from_slice(&[0x1e, 0x00]);
        payload[66..74].copy_from_slice(&1.25f64.to_le_bytes());
        payload[74..82].copy_from_slice(&(-2.5f64).to_le_bytes());
        payload[84..86].copy_from_slice(&2u16.to_le_bytes());
        for (index, local_id) in [7u16, 11].into_iter().enumerate() {
            let start = 86 + index * 12;
            payload[start..start + 2].copy_from_slice(&0x8386u16.to_le_bytes());
            payload[start + 2..start + 4].copy_from_slice(&local_id.to_le_bytes());
            payload[start + 4..start + 8].fill(0xff);
        }
        payload[112..116].copy_from_slice(&[0xfe, 0xff, 0xff, 0xff]);
        assert_eq!(
            coordinate_marker_local_links(&payload, 0),
            Some((vec![7, 11], 0x8386))
        );
        for start in [86, 98] {
            payload[start..start + 2].copy_from_slice(&0xbc87u16.to_le_bytes());
        }
        assert_eq!(
            coordinate_marker_local_links(&payload, 0),
            Some((vec![7, 11], 0xbc87))
        );
        payload[98] ^= 1;
        assert_eq!(coordinate_marker_local_links(&payload, 0), None);
    }

    #[test]
    fn coordinate_namespace_disambiguates_reused_local_id() {
        let candidates = vec![("relation".into(), false), ("geometry".into(), true)];
        assert_eq!(unique_marker_candidate(&candidates), Some("geometry"));
        let ambiguous = vec![("first".into(), true), ("second".into(), true)];
        assert_eq!(unique_marker_candidate(&ambiguous), None);
    }

    #[test]
    fn point_operand_requires_one_profile_locus() {
        let entity = SketchEntityId("entity".into());
        let locus = SketchLocus::Start(entity.clone());
        assert_eq!(unique_locus(&[locus.clone()]), Some(locus));
        assert_eq!(unique_locus(&[]), None);
        assert_eq!(
            unique_locus(&[SketchLocus::Start(entity.clone()), SketchLocus::End(entity)]),
            None
        );
    }

    #[test]
    fn compact_body_selection_requires_the_complete_trailer() {
        let mut payload = vec![0xaa; 9];
        payload.extend(11000u32.to_le_bytes());
        payload.extend([0; 8]);
        payload.extend(2u32.to_le_bytes());
        payload.extend(287u32.to_le_bytes());
        payload.extend(115u32.to_le_bytes());
        payload.extend(u32::MAX.to_le_bytes());
        payload.extend([0; 12]);
        payload.extend([0x6a, 0xcb]);
        assert_eq!(
            compact_body_selection_vector(&payload, 100, Some(0xcb6a)),
            Some((109, vec![287, 115]))
        );
        assert_eq!(compact_body_selection_at(&payload, 9), Some(vec![287, 115]));
        let mut embedded_false_header = vec![0xaa; 9];
        embedded_false_header.extend(11000u32.to_le_bytes());
        embedded_false_header.extend([0; 8]);
        embedded_false_header.extend(5u32.to_le_bytes());
        for id in [287, 11000, 0, 0, u32::MAX] {
            embedded_false_header.extend(id.to_le_bytes());
        }
        embedded_false_header.extend(u32::MAX.to_le_bytes());
        embedded_false_header.extend([0; 12]);
        assert_eq!(
            compact_body_selection_vector(&embedded_false_header, 100, None),
            Some((109, vec![287, 11000, 0, 0, u32::MAX]))
        );
        let zero_trailer = payload.len() - 3;
        payload[zero_trailer] = 1;
        assert_eq!(
            compact_body_selection_vector(&payload, 100, Some(0xcb6a)),
            None
        );
    }

    #[test]
    fn compact_edge_selection_is_count_delimited_and_signature_typed() {
        let mut payload = Vec::new();
        payload.extend(3u32.to_le_bytes());
        payload.extend([0x00, 0x02, 0x00, 0x00, 0, 0, 0, 0]);
        payload.extend(COMPACT_EDGE_VECTOR_MARKER);
        payload.extend([0, 0]);
        let signature = [
            0x00, 0x81, 0x03, 0x01, 0x2c, 0, 0, 0, 0x63, 0x18, 0x58, 0x69,
        ];
        for (index, edge_id) in [4u32, 0, 5].into_iter().enumerate() {
            payload.extend((0x818bu32 + index as u32).to_le_bytes());
            payload.extend(signature);
            payload.extend(edge_id.to_le_bytes());
            if index == 0 {
                payload.extend([0xff, 0xff, 0xff, 0xff, 0, 0, 0, 0]);
            } else if index == 1 {
                payload.extend([0; 8]);
            }
        }
        assert_eq!(compact_edge_selection_at(&payload, 12), Some(vec![4, 0, 5]));
        payload[12 + 18 + 28 + 4] ^= 1;
        assert_eq!(compact_edge_selection_at(&payload, 12), Some(vec![4, 0, 5]));
    }

    #[test]
    fn compact_edge_selection_rejects_unbounded_counts_and_short_headers() {
        let mut payload = vec![0; 40];
        payload[..4].copy_from_slice(&u32::MAX.to_le_bytes());
        payload[4..8].copy_from_slice(&[0, 2, 0, 0]);
        payload[12..28].copy_from_slice(&COMPACT_EDGE_VECTOR_MARKER);
        assert_eq!(compact_edge_selection_at(&payload, 12), None);
        assert_eq!(compact_edge_component_path_at(&payload, 12), None);

        payload[..16].copy_from_slice(&COMPACT_EDGE_VECTOR_MARKER);
        assert_eq!(compact_edge_selection_at(&payload, 0), None);
        assert_eq!(compact_edge_component_path_at(&payload, 0), None);
        assert_eq!(compact_surface_selection_at(&payload, 0), None);
    }

    #[test]
    fn solved_tangent_treats_arcs_as_bounded_circles() {
        use cadmpeg_ir::features::{Angle, Length};

        let line = SketchGeometry::Line {
            start: Point2::new(-2.0, 1.0),
            end: Point2::new(2.0, 1.0),
        };
        let arc = SketchGeometry::Arc {
            center: Point2::new(0.0, 0.0),
            radius: Length(1.0),
            start_angle: Angle(0.0),
            end_angle: Angle(std::f64::consts::PI),
        };
        let circle = SketchGeometry::Circle {
            center: Point2::new(2.0, 0.0),
            radius: Length(1.0),
        };
        assert_eq!(solved_tangent(&line, &arc), Some(true));
        assert_eq!(solved_tangent(&arc, &circle), Some(true));
    }

    #[test]
    fn every_principal_plane_has_a_sketch_frame() {
        use cadmpeg_ir::features::PrincipalPlane;

        for plane in [
            PrincipalPlane::Front,
            PrincipalPlane::Top,
            PrincipalPlane::Right,
        ] {
            let (_, normal, u_axis) = principal_sketch_frame(plane);
            assert!((super::dot(normal, normal) - 1.0).abs() <= 1.0e-12);
            assert!((super::dot(u_axis, u_axis) - 1.0).abs() <= 1.0e-12);
            assert!(super::dot(normal, u_axis).abs() <= 1.0e-12);
        }
    }

    #[test]
    fn compact_edge_selection_accepts_heterogeneous_component_paths() {
        let marker = 12;
        let mut payload = vec![0; 100];
        payload[..4].copy_from_slice(&2u32.to_le_bytes());
        payload[4..8].copy_from_slice(&[0, 2, 0, 0]);
        payload[8..12].copy_from_slice(&37u32.to_le_bytes());
        payload[marker..marker + 16].copy_from_slice(&COMPACT_EDGE_VECTOR_MARKER);
        let first = marker + 18;
        payload[first..first + 4].copy_from_slice(&[0x3d, 0x80, 0, 0]);
        payload[first + 4..first + 16].copy_from_slice(&[1; 12]);
        payload[first + 16..first + 20].copy_from_slice(&2u32.to_le_bytes());
        let second = first + 28;
        payload[second..second + 4].copy_from_slice(&[0x4a, 0x80, 0, 0]);
        payload[second + 4..second + 16].copy_from_slice(&[2; 12]);
        payload[second + 16..second + 20].copy_from_slice(&3u32.to_le_bytes());
        assert_eq!(
            compact_edge_selection_at(&payload, marker),
            Some(vec![2, 3])
        );
        assert_eq!(
            compact_edge_component_path_at(&payload, marker),
            Some(vec![
                FeatureInputComponentPathEntry {
                    instance: 0x803d,
                    type_signature: [1; 12],
                    local_id: 2,
                },
                FeatureInputComponentPathEntry {
                    instance: 0x804a,
                    type_signature: [2; 12],
                    local_id: 3,
                },
            ])
        );
    }

    #[test]
    fn compact_edge_selection_excludes_terminal_feature_reference_cell() {
        let marker = 12;
        let mut payload = vec![0; 160];
        payload[..4].copy_from_slice(&4u32.to_le_bytes());
        payload[4..8].copy_from_slice(&[0, 2, 0, 0]);
        payload[marker..marker + 16].copy_from_slice(&COMPACT_EDGE_VECTOR_MARKER);
        let signature = [0x34, 0x80, 0x37, 0, 121, 0, 0, 0, 0x9b, 0x95, 0x90, 0x5f];
        let mut cursor = marker + 18;
        for (index, local_id) in [32u32, 34, 1].into_iter().enumerate() {
            payload[cursor..cursor + 4].copy_from_slice(&[0x3d, 0x80, 0, 0]);
            payload[cursor + 4..cursor + 16].copy_from_slice(&signature);
            payload[cursor + 16..cursor + 20].copy_from_slice(&local_id.to_le_bytes());
            cursor += 20;
            if index != 2 {
                payload[cursor..cursor + 8].copy_from_slice(&[0xff, 0xff, 0xff, 0xff, 0, 0, 0, 0]);
                cursor += 8;
            }
        }
        payload[cursor..cursor + 36].copy_from_slice(&[
            1, 0, 0, 0, 0, 0, 0, 0, 0x4a, 0x80, 0, 0, 0x34, 0x80, 0x37, 0, 35, 0, 0, 0, 0x89, 0x6b,
            0x90, 0x5f, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        ]);
        assert_eq!(
            compact_edge_selection_at(&payload, marker),
            Some(vec![32, 34, 1])
        );
        assert_eq!(
            compact_edge_component_path_at(&payload, marker).map(|components| components.len()),
            Some(3)
        );
    }

    #[test]
    fn compact_body_path_requires_type_three_vector() {
        let marker = 12;
        let mut payload = vec![0; 100];
        payload[..4].copy_from_slice(&2u32.to_le_bytes());
        payload[4..8].copy_from_slice(&[0, 3, 0, 0]);
        payload[marker..marker + 16].copy_from_slice(&COMPACT_EDGE_VECTOR_MARKER);
        let first = marker + 18;
        payload[first..first + 4].copy_from_slice(&[0x32, 0x80, 0, 0]);
        payload[first + 4..first + 16].copy_from_slice(&[1; 12]);
        payload[first + 16..first + 20].copy_from_slice(&6u32.to_le_bytes());
        let second = first + 28;
        payload[second..second + 4].copy_from_slice(&[0x3b, 0x80, 0, 0]);
        payload[second + 4..second + 16].copy_from_slice(&[2; 12]);
        payload[second + 16..second + 20].copy_from_slice(&7u32.to_le_bytes());
        assert_eq!(compact_body_path_at(&payload, marker), Some(vec![6, 7]));
        assert_eq!(
            compact_body_component_path_at(&payload, marker).map(|components| components.len()),
            Some(2)
        );

        payload[..4].copy_from_slice(&3u32.to_le_bytes());
        payload[second + 20..second + 28].copy_from_slice(&[0xff, 0xff, 0xff, 0xff, 0, 0, 0, 0]);
        assert_eq!(compact_body_path_at(&payload, marker), Some(vec![6, 7]));

        payload[4] = 2;
        assert_eq!(compact_body_path_at(&payload, marker), None);
    }

    #[test]
    fn compact_combine_operation_is_name_length_relative() {
        let offset = 7;
        let mut payload = vec![0; 180];
        payload[offset..offset + 5].copy_from_slice(&[0x04, 0x80, 0xff, 0xfe, 0xff]);
        payload[offset + 5] = 8;
        let operation = offset + 117 + 16;
        payload[operation..operation + 4].copy_from_slice(&2u32.to_le_bytes());
        payload[operation + 10..operation + 14].copy_from_slice(&[0xff; 4]);
        assert_eq!(
            compact_combine_operation_at(&payload, offset),
            Some("Intersect")
        );
        payload[operation - 1] = 1;
        assert_eq!(compact_combine_operation_at(&payload, offset), None);
    }

    #[test]
    fn compact_edge_selection_accepts_counted_u16_ids() {
        let marker = 12;
        let mut payload = vec![0; 80];
        payload[..4].copy_from_slice(&3u32.to_le_bytes());
        payload[4..8].copy_from_slice(&[0, 2, 0, 0]);
        payload[marker..marker + 16].copy_from_slice(&COMPACT_EDGE_VECTOR_MARKER);
        let ids = marker + 18;
        payload[ids..ids + 6].copy_from_slice(&[4, 0, 8, 0, 12, 0]);
        payload[ids + 22..ids + 25].copy_from_slice(&[0xff, 0xfe, 0xff]);
        assert_eq!(
            compact_edge_selection_at(&payload, marker),
            Some(vec![4, 8, 12])
        );
        assert_eq!(compact_edge_component_path_at(&payload, marker), None);
    }

    #[test]
    fn native_scalar_must_match_an_existing_discrete_parameter() {
        let feature = crate::records::Feature {
            id: "feature".into(),
            parent: "history".into(),
            xml_tag: "Feature".into(),
            tree_parent: None,
            source_id: None,
            parent_source_id: None,
            ordinal: 0,
            name: "Pattern".into(),
            kind: "Pattern".into(),
            input_class: Some("moLPattern_c".into()),
            suppressed: false,
            parameters: Default::default(),
            dimension_properties: Default::default(),
            properties: Default::default(),
            text: None,
            content: Vec::new(),
        };
        assert!(native_scalar_matches_discrete_parameter(
            &feature, "D1", "15", 15.0
        ));
        assert!(!native_scalar_matches_discrete_parameter(
            &feature,
            "D1",
            "15",
            8.371160993642741e298
        ));
    }

    #[test]
    fn compact_surface_selection_ends_with_its_entry_signature() {
        let mut payload = Vec::new();
        payload.extend(6u32.to_le_bytes());
        payload.extend([0x04, 0x02, 0, 0]);
        payload.extend(0x1234u32.to_le_bytes());
        payload.extend(COMPACT_EDGE_VECTOR_MARKER);
        payload.extend([0, 0]);
        let signature = [0x34, 0x80, 0x37, 0, 0x89, 0, 0, 0, 0xe2, 0x56, 0xdf, 0x5e];
        for (index, id) in [2u32, 1, 11].into_iter().enumerate() {
            payload.extend((0x8c20u32 + index as u32).to_le_bytes());
            payload.extend(signature);
            payload.extend(id.to_le_bytes());
            if index == 0 {
                payload.extend(1u32.to_le_bytes());
            }
        }
        payload.extend([0; 24]);
        let components = compact_surface_selection_at(&payload, 12).unwrap();
        assert_eq!(
            components
                .iter()
                .map(|component| (
                    component.instance,
                    component.type_signature,
                    component.local_id
                ))
                .collect::<Vec<_>>(),
            vec![
                (0x8c20, signature, 2),
                (0x8c21, signature, 1),
                (0x8c22, signature, 11)
            ]
        );
        payload[12 + 18 + 24 + 4] ^= 1;
        assert_eq!(
            compact_surface_selection_at(&payload, 12)
                .unwrap()
                .iter()
                .map(|component| component.local_id)
                .collect::<Vec<_>>(),
            vec![2]
        );
    }

    #[test]
    fn component_path_type_identities_name_ordered_features() {
        let feature = |id: &str, source_id: &str| Feature {
            id: id.into(),
            parent: "history".into(),
            xml_tag: "Feature".into(),
            tree_parent: None,
            source_id: Some(source_id.into()),
            parent_source_id: None,
            ordinal: 0,
            name: String::new(),
            kind: String::new(),
            input_class: None,
            suppressed: false,
            parameters: Default::default(),
            dimension_properties: Default::default(),
            properties: Default::default(),
            text: None,
            content: Vec::new(),
        };
        let mut signature = [0u8; 12];
        signature[4..8].copy_from_slice(&42u32.to_le_bytes());
        let components = vec![
            FeatureInputComponentPathEntry {
                instance: 0x8032,
                type_signature: signature,
                local_id: 7,
            },
            FeatureInputComponentPathEntry {
                instance: 0x803b,
                type_signature: signature,
                local_id: 1,
            },
        ];
        assert_eq!(
            component_path_features(&components, &[feature("producer", "42")]),
            vec!["producer"]
        );
        assert_eq!(
            component_path_features(
                &components,
                &[feature("first", "42"), feature("second", "42")]
            ),
            Vec::<String>::new()
        );
        let mut mixed = components;
        mixed[1].type_signature[4..8].copy_from_slice(&43u32.to_le_bytes());
        assert_eq!(
            component_path_features(&mixed, &[feature("producer", "42"), feature("other", "43")]),
            vec!["producer", "other"]
        );
        assert_eq!(
            component_path_terminal_feature(
                &mixed,
                &[feature("producer", "42"), feature("other", "43")]
            ),
            Some("other".into())
        );
    }
}

pub(crate) fn named_scalars(
    payload: &[u8],
    parent: &str,
    names: &[FeatureInputName],
) -> Vec<FeatureInputScalar> {
    let lane_key = parent.rsplit_once('#').map_or(parent, |(_, key)| key);
    names
        .iter()
        .filter_map(|name| {
            let name_offset = usize::try_from(name.offset).ok()?;
            let value_offset = scalar_value_offset(name_offset, &name.value)?;
            let header_offset = value_offset.checked_sub(SCALAR_HEADER.len())?;
            if payload.get(header_offset..value_offset)? != SCALAR_HEADER {
                return None;
            }
            let value = f64::from_le_bytes(
                payload
                    .get(value_offset..value_offset + 8)?
                    .try_into()
                    .ok()?,
            );
            let trailer_offset = value_offset.checked_add(8)?;
            let object_id = u32::from_le_bytes(
                payload
                    .get(trailer_offset + 3..trailer_offset + 7)?
                    .try_into()
                    .ok()?,
            );
            let role = scalar_role(payload, trailer_offset);
            let operands = scalar_operands(payload, trailer_offset, parent);
            let entity_indices = operands
                .iter()
                .filter(|operand| operand.kind == FeatureInputOperandKind::D6)
                .map(|operand| operand.entity_index)
                .collect();
            value.is_finite().then_some((
                name,
                value_offset,
                object_id,
                value,
                role,
                entity_indices,
                operands,
            ))
        })
        .enumerate()
        .map(
            |(ordinal, (name, offset, object_id, value, role, entity_indices, operands))| {
                FeatureInputScalar {
                    id: format!("sldprt:feature-input:scalar#{lane_key}:{offset}"),
                    parent: parent.to_string(),
                    feature_ref: None,
                    ordinal: ordinal as u32,
                    offset: offset as u64,
                    object_id,
                    name: name.id.clone(),
                    value,
                    role,
                    entity_indices,
                    operands,
                }
            },
        )
        .collect()
}

fn scalar_value_offset(name_offset: usize, name: &str) -> Option<usize> {
    name_offset
        .checked_add(NAME_MARKER.len() + 1)?
        .checked_add(name.encode_utf16().count().checked_mul(2)?)?
        .checked_add(SCALAR_HEADER.len())
}

pub(crate) fn scalar_indices_match(
    actual: &[FeatureInputScalar],
    expected: &[FeatureInputScalar],
) -> bool {
    actual.len() == expected.len()
        && actual.iter().zip(expected).all(|(actual, expected)| {
            actual.id == expected.id
                && actual.parent == expected.parent
                && actual.feature_ref == expected.feature_ref
                && actual.ordinal == expected.ordinal
                && actual.offset == expected.offset
                && actual.object_id == expected.object_id
                && actual.name == expected.name
                && ulp_distance(actual.value, expected.value) <= 4
                && actual.role == expected.role
                && actual.entity_indices == expected.entity_indices
                && actual.operands == expected.operands
        })
}

fn ulp_distance(left: f64, right: f64) -> u64 {
    fn ordered(value: f64) -> u64 {
        let bits = value.to_bits();
        if bits & (1 << 63) == 0 {
            bits | (1 << 63)
        } else {
            !bits
        }
    }
    ordered(left).abs_diff(ordered(right))
}

fn scalar_operands(
    payload: &[u8],
    trailer_offset: usize,
    parent: &str,
) -> Vec<FeatureInputOperand> {
    let lane_key = parent.rsplit_once('#').map_or(parent, |(_, key)| key);
    let first = if legacy_scalar_layout(payload, trailer_offset) {
        36
    } else {
        35
    };
    [first, first + 12]
        .into_iter()
        .filter_map(|relative| {
            let offset = trailer_offset.checked_add(relative)?;
            let cell = payload.get(offset..offset + 12)?;
            if cell[4..8] != [0xff; 4] || cell[8..12] != [0; 4] {
                return None;
            }
            let kind = operand_kind([cell[0], cell[1]])?;
            Some(FeatureInputOperand {
                offset: offset as u64,
                reference_ref: format!("sldprt:feature-input:reference#{lane_key}:{offset}"),
                kind,
                entity_index: u16::from_le_bytes([cell[2], cell[3]]),
                entity_ref: None,
            })
        })
        .collect()
}

fn operand_kind(tag: [u8; 2]) -> Option<FeatureInputOperandKind> {
    match tag {
        [0, 0] | [0xff, 0xff] => None,
        [0xd6, 0x80] => Some(FeatureInputOperandKind::D6),
        [0xe1, 0x80] => Some(FeatureInputOperandKind::E1),
        bytes => Some(FeatureInputOperandKind::Native(u16::from_le_bytes(bytes))),
    }
}

pub(crate) fn feature_object_name<'a>(
    feature: &crate::records::Feature,
    lane: &'a FeatureInputLane,
) -> Option<&'a FeatureInputName> {
    if let Some(source_id) = feature
        .source_id
        .as_deref()
        .and_then(|value| value.parse::<u32>().ok())
    {
        let mut matches = lane
            .names
            .iter()
            .filter(|name| name.object_id == Some(source_id));
        if let Some(first) = matches.next() {
            if matches.next().is_none() {
                return Some(first);
            }
            return None;
        }
    }
    let mut matches = lane.names.iter().filter(|name| name.value == feature.name);
    let first = matches.next()?;
    matches.next().is_none().then_some(first)
}

fn line_reference_direction(payload: &[u8], class_offset: u64) -> Option<Vector3> {
    let class_offset = usize::try_from(class_offset).ok()?;
    let direction_offset = if payload.get(class_offset + 136..class_offset + 144)
        == Some(&[0xc7, 0xcf, 0xff, 0xff, 0xc7, 0xcf, 0xff, 0xff])
        && payload.get(class_offset + 148..class_offset + 152) == Some(&[0xf8, 0x2a, 0, 0])
    {
        class_offset + 200
    } else if payload.get(class_offset + 144..class_offset + 156)
        == Some(&[
            0xc7, 0xcf, 0xff, 0xff, 0xc7, 0xcf, 0xff, 0xff, 0xc7, 0xcf, 0xff, 0xff,
        ])
        && payload.get(class_offset + 160..class_offset + 164) == Some(&[0xf8, 0x2a, 0, 0])
    {
        class_offset + 220
    } else {
        return None;
    };
    let scalar = |offset: usize| {
        let value = f64::from_le_bytes(payload.get(offset..offset + 8)?.try_into().ok()?);
        value.is_finite().then_some(value)
    };
    let direction = Vector3::new(
        scalar(direction_offset)?,
        scalar(direction_offset + 8)?,
        scalar(direction_offset + 16)?,
    );
    let norm =
        (direction.x * direction.x + direction.y * direction.y + direction.z * direction.z).sqrt();
    ((norm - 1.0).abs() <= 1.0e-9).then_some(Vector3::new(
        direction.x / norm,
        direction.y / norm,
        direction.z / norm,
    ))
}

/// Bind pattern operands carried by adjacent feature-input objects.
pub(crate) fn bind_pattern_inputs(
    model_features: &mut [cadmpeg_ir::features::Feature],
    histories: &[crate::records::FeatureHistory],
    lanes: &[FeatureInputLane],
) {
    let history_features = histories
        .iter()
        .flat_map(|history| &history.features)
        .collect::<Vec<_>>();
    let model_by_native = model_features
        .iter()
        .enumerate()
        .filter_map(|(index, feature)| Some((feature.native_ref.as_deref()?, index)))
        .collect::<HashMap<_, _>>();
    let mut assignments = Vec::<(
        usize,
        cadmpeg_ir::features::FeatureId,
        cadmpeg_ir::features::FeatureId,
        PathRef,
    )>::new();
    let mut linear_seed_assignments = Vec::<(usize, cadmpeg_ir::features::FeatureId)>::new();
    let mut linear_direction_assignments = Vec::<(usize, Vector3)>::new();

    for lane in lanes {
        let mut starts = history_features
            .iter()
            .filter_map(|feature| Some((feature_object_name(feature, lane)?.offset, *feature)))
            .collect::<Vec<_>>();
        starts.sort_unstable_by_key(|(offset, _)| *offset);
        for (start_index, (_, feature)) in starts.iter().enumerate() {
            if feature.input_class.as_deref() == Some("moLPattern_c") {
                let Some(&model_index) = model_by_native.get(feature.id.as_str()) else {
                    continue;
                };
                if !matches!(
                    model_features[model_index].definition,
                    FeatureDefinition::Pattern {
                        ref seeds,
                        pattern: PatternKind::Linear { .. },
                    } if seeds.is_empty()
                ) {
                    continue;
                }
                let Some(previous_index) = start_index.checked_sub(1) else {
                    continue;
                };
                let (_, seed) = starts[previous_index];
                let Some(&seed_index) = model_by_native.get(seed.id.as_str()) else {
                    continue;
                };
                linear_seed_assignments.push((model_index, model_features[seed_index].id.clone()));
                let end = starts
                    .get(start_index + 1)
                    .map_or(u64::MAX, |(offset, _)| *offset);
                let mut directions = lane
                    .classes
                    .iter()
                    .filter(|class| {
                        class.name == "moLineRef_w"
                            && class.offset > starts[start_index].0
                            && class.offset < end
                    })
                    .filter_map(|class| {
                        line_reference_direction(&lane.native_payload, class.offset)
                    })
                    .collect::<Vec<_>>();
                directions.sort_by_key(|direction| {
                    [
                        direction.x.to_bits(),
                        direction.y.to_bits(),
                        direction.z.to_bits(),
                    ]
                });
                directions.dedup();
                if let [direction] = directions.as_slice() {
                    linear_direction_assignments.push((model_index, *direction));
                }
                continue;
            }
            if feature.input_class.as_deref() != Some("moCurvePattern_c") {
                continue;
            }
            let Some(&model_index) = model_by_native.get(feature.id.as_str()) else {
                continue;
            };
            if !matches!(
                model_features[model_index].definition,
                FeatureDefinition::Pattern {
                    ref seeds,
                    pattern: PatternKind::CurveDriven { path: None, .. },
                    ..
                } if seeds.is_empty()
            ) {
                continue;
            }
            let Some(previous_index) = start_index.checked_sub(1) else {
                continue;
            };
            let (_, seed) = starts[previous_index];
            let Some(&seed_index) = model_by_native.get(seed.id.as_str()) else {
                continue;
            };
            let Some((_, target)) = starts.get(start_index + 1) else {
                continue;
            };
            if target.input_class.as_deref() != Some("moProfileFeature_c") {
                continue;
            }
            let Some(&target_index) = model_by_native.get(target.id.as_str()) else {
                continue;
            };
            let FeatureDefinition::Sketch {
                sketch: Some(sketch),
                ..
            } = &model_features[target_index].definition
            else {
                continue;
            };
            assignments.push((
                model_index,
                model_features[seed_index].id.clone(),
                model_features[target_index].id.clone(),
                PathRef::Sketch(sketch.clone()),
            ));
        }
    }
    let mut assignments_by_pattern = HashMap::<usize, Vec<_>>::new();
    for (index, seed, path_dependency, path) in assignments {
        let candidates = assignments_by_pattern.entry(index).or_default();
        if !candidates
            .iter()
            .any(|candidate| *candidate == (seed.clone(), path_dependency.clone(), path.clone()))
        {
            candidates.push((seed, path_dependency, path));
        }
    }
    for (index, candidates) in assignments_by_pattern {
        let [(seed, path_dependency, path)] = candidates.as_slice() else {
            continue;
        };
        for dependency in [seed, path_dependency] {
            if !model_features[index].dependencies.contains(dependency) {
                model_features[index].dependencies.push(dependency.clone());
            }
        }
        if let FeatureDefinition::Pattern {
            seeds,
            pattern: PatternKind::CurveDriven { path: slot, .. },
            ..
        } = &mut model_features[index].definition
        {
            if seeds.is_empty() && slot.is_none() {
                seeds.push(seed.clone());
                *slot = Some(path.clone());
            }
        }
    }
    let mut linear_seeds_by_pattern = HashMap::<usize, Vec<cadmpeg_ir::features::FeatureId>>::new();
    for (index, seed) in linear_seed_assignments {
        let candidates = linear_seeds_by_pattern.entry(index).or_default();
        if !candidates.contains(&seed) {
            candidates.push(seed);
        }
    }
    for (index, candidates) in linear_seeds_by_pattern {
        let [seed] = candidates.as_slice() else {
            continue;
        };
        if !model_features[index].dependencies.contains(seed) {
            model_features[index].dependencies.push(seed.clone());
        }
        if let FeatureDefinition::Pattern { seeds, .. } = &mut model_features[index].definition {
            if seeds.is_empty() {
                seeds.push(seed.clone());
            }
        }
    }
    let mut linear_directions_by_pattern = HashMap::<usize, Vec<Vector3>>::new();
    for (index, direction) in linear_direction_assignments {
        let candidates = linear_directions_by_pattern.entry(index).or_default();
        if !candidates.contains(&direction) {
            candidates.push(direction);
        }
    }
    for (index, candidates) in linear_directions_by_pattern {
        let [direction] = candidates.as_slice() else {
            continue;
        };
        if let FeatureDefinition::Pattern {
            pattern: PatternKind::Linear {
                direction: slot, ..
            },
            ..
        } = &mut model_features[index].definition
        {
            if slot.is_none() {
                *slot = Some(*direction);
            }
        }
    }
}

/// Bind solid-sweep cross sections carried by the following profile object.
pub(crate) fn bind_sweep_adjacent_profiles(
    model_features: &mut [cadmpeg_ir::features::Feature],
    histories: &[crate::records::FeatureHistory],
    lanes: &[FeatureInputLane],
) {
    let history_features = histories
        .iter()
        .flat_map(|history| &history.features)
        .collect::<Vec<_>>();
    let model_by_native = model_features
        .iter()
        .enumerate()
        .filter_map(|(index, feature)| Some((feature.native_ref.as_deref()?, index)))
        .collect::<HashMap<_, _>>();
    let mut assignments = HashMap::<
        usize,
        Vec<(
            cadmpeg_ir::features::FeatureId,
            SketchId,
            Option<(cadmpeg_ir::features::FeatureId, SketchId)>,
        )>,
    >::new();
    for lane in lanes {
        let mut starts = history_features
            .iter()
            .filter_map(|feature| Some((feature_object_name(feature, lane)?.offset, *feature)))
            .collect::<Vec<_>>();
        starts.sort_unstable_by_key(|(offset, _)| *offset);
        for (index, (_, feature)) in starts.iter().enumerate() {
            if feature.input_class.as_deref() != Some("moSweep_c") {
                continue;
            }
            let Some(&model_index) = model_by_native.get(feature.id.as_str()) else {
                continue;
            };
            if !matches!(
                model_features[model_index].definition,
                FeatureDefinition::Sweep { profile: None, .. }
            ) {
                continue;
            }
            let Some((_, profile_feature)) = starts.get(index + 1) else {
                continue;
            };
            if profile_feature.input_class.as_deref() != Some("moProfileFeature_c") {
                continue;
            }
            let Some(&profile_index) = model_by_native.get(profile_feature.id.as_str()) else {
                continue;
            };
            let FeatureDefinition::Sketch {
                sketch: Some(sketch),
                ..
            } = &model_features[profile_index].definition
            else {
                continue;
            };
            let path = index.checked_sub(1).and_then(|path_object_index| {
                let (_, path_feature) = starts[path_object_index];
                if path_feature.input_class.as_deref() != Some("moProfileFeature_c") {
                    return None;
                }
                let path_index = *model_by_native.get(path_feature.id.as_str())?;
                let FeatureDefinition::Sketch {
                    sketch: Some(path), ..
                } = &model_features[path_index].definition
                else {
                    return None;
                };
                Some((model_features[path_index].id.clone(), path.clone()))
            });
            let candidate = (
                model_features[profile_index].id.clone(),
                sketch.clone(),
                path,
            );
            let candidates = assignments.entry(model_index).or_default();
            if !candidates.contains(&candidate) {
                candidates.push(candidate);
            }
        }
    }
    for (index, candidates) in assignments {
        let [(profile_dependency, sketch, path)] = candidates.as_slice() else {
            continue;
        };
        let mut profile_bound = false;
        if let FeatureDefinition::Sweep {
            profile,
            path: path_slot,
            ..
        } = &mut model_features[index].definition
        {
            if profile.is_none() {
                *profile = Some(cadmpeg_ir::features::ProfileRef::Sketch(sketch.clone()));
                profile_bound = true;
            }
            if let Some((_, path)) = path {
                if path_slot
                    .as_ref()
                    .is_none_or(|existing| matches!(existing, PathRef::Native(_)))
                {
                    *path_slot = Some(PathRef::Sketch(path.clone()));
                }
            }
        }
        if profile_bound
            && !model_features[index]
                .dependencies
                .contains(profile_dependency)
        {
            model_features[index]
                .dependencies
                .push(profile_dependency.clone());
        }
        if let Some((path_dependency, _)) = path {
            if !model_features[index].dependencies.contains(path_dependency) {
                model_features[index]
                    .dependencies
                    .push(path_dependency.clone());
            }
        }
    }
}

/// Resolve scalar operand indices within their owning feature-object interval.
pub(crate) fn bind_scalar_operands(
    histories: &[crate::records::FeatureHistory],
    lanes: &mut [FeatureInputLane],
) {
    for lane in lanes {
        for entity in &mut lane.sketch_entities {
            entity.feature_ref = None;
            entity.links.clear();
            entity.link_selector = None;
        }
        let mut starts = histories
            .iter()
            .flat_map(|history| &history.features)
            .filter_map(|feature| {
                Some((
                    feature_object_name(feature, lane)?.offset,
                    feature.id.as_str(),
                ))
            })
            .collect::<Vec<_>>();
        starts.sort_unstable_by_key(|start| start.0);
        for (index, &(start, feature_id)) in starts.iter().enumerate() {
            let end = starts.get(index + 1).map_or(u64::MAX, |next| next.0);
            for entity in lane
                .sketch_entities
                .iter_mut()
                .filter(|entity| entity.offset > start && entity.offset < end)
            {
                entity.feature_ref = Some(feature_id.to_string());
            }
            for reference in lane
                .references
                .iter_mut()
                .filter(|reference| reference.offset > start && reference.offset < end)
            {
                reference.feature_ref = Some(feature_id.to_string());
            }
            for scalar in lane
                .scalars
                .iter_mut()
                .filter(|scalar| scalar.offset > start && scalar.offset < end)
            {
                scalar.feature_ref = Some(feature_id.to_string());
            }
        }
        let mut marker_ids = HashMap::<(String, u32), Vec<(String, bool)>>::new();
        for entity in &lane.sketch_entities {
            if let (Some(feature), Some(local_id)) = (&entity.feature_ref, entity.local_id) {
                marker_ids
                    .entry((feature.clone(), local_id))
                    .or_default()
                    .push((entity.id.clone(), entity.coordinates_m.is_some()));
            }
        }
        for entity in &mut lane.sketch_entities {
            let Ok(offset) = usize::try_from(entity.offset) else {
                continue;
            };
            let Some((local_ids, selector)) = marker_local_links(&lane.native_payload, offset)
                .map(|(links, selector)| (links.to_vec(), selector))
                .or_else(|| coordinate_marker_local_links(&lane.native_payload, offset))
            else {
                continue;
            };
            let Some(owner) = &entity.feature_ref else {
                continue;
            };
            let links = local_ids
                .into_iter()
                .filter_map(|local_id| {
                    let entity_ref = unique_marker_candidate(
                        marker_ids.get(&(owner.clone(), u32::from(local_id)))?,
                    )?;
                    Some(SketchInputLink {
                        local_id,
                        entity_ref: entity_ref.to_string(),
                    })
                })
                .collect::<Vec<_>>();
            if !links.is_empty() {
                entity.links = links;
                entity.link_selector = Some(selector);
            }
        }
        let entities_by_feature = lane.sketch_entities.iter().fold(
            HashMap::<&str, Vec<&SketchInputEntity>>::new(),
            |mut by_feature, entity| {
                if let Some(feature) = entity.feature_ref.as_deref() {
                    by_feature.entry(feature).or_default().push(entity);
                }
                by_feature
            },
        );
        for scalar in &mut lane.scalars {
            let Some(entities) = scalar
                .feature_ref
                .as_deref()
                .and_then(|feature| entities_by_feature.get(feature))
            else {
                continue;
            };
            let resolved =
                resolve_scalar_operand_markers(entities.iter().copied(), &scalar.operands);
            for (operand, resolved) in scalar.operands.iter_mut().zip(resolved) {
                operand.entity_ref = resolved.map(|entity| entity.id.clone());
            }
        }
        let scalar_owners = lane
            .scalars
            .iter()
            .map(|scalar| (scalar.id.as_str(), scalar.feature_ref.clone()))
            .collect::<HashMap<_, _>>();
        for binding in &mut lane.relation_bindings {
            binding.feature_ref = scalar_owners
                .get(binding.scalar_ref.as_str())
                .cloned()
                .flatten();
        }
        lane.relation_instances = relation_instances(histories, lane);
        lane.body_selections = compact_body_selections(histories, lane);
        lane.edge_selections = compact_edge_selections(histories, lane);
        lane.surface_selections = compact_surface_selections(histories, lane);
    }
}

/// Resolve helix placement from the counted curve mesh stored in its feature
/// object. Promotion requires one mesh stream and a circular-helix fit whose
/// residual is small relative to its radius.
pub(crate) fn project_helix_axes(
    model_features: &mut [cadmpeg_ir::features::Feature],
    histories: &[crate::records::FeatureHistory],
    lanes: &[FeatureInputLane],
) {
    let records = histories
        .iter()
        .flat_map(|history| &history.features)
        .map(|feature| (feature.id.as_str(), feature))
        .collect::<HashMap<_, _>>();
    for model_feature in model_features {
        let FeatureDefinition::HelixNativeAxis {
            axial_rise,
            revolutions,
            start_angle,
            clockwise,
            ..
        } = &model_feature.definition
        else {
            continue;
        };
        let Some(native_ref) = model_feature.native_ref.as_deref() else {
            continue;
        };
        let Some(record) = records.get(native_ref).copied() else {
            continue;
        };
        let mut meshes = Vec::new();
        for lane in lanes {
            let Some(name) = feature_object_name(record, lane) else {
                continue;
            };
            let start = usize::try_from(name.offset).ok();
            let end = histories
                .iter()
                .flat_map(|history| &history.features)
                .filter_map(|feature| feature_object_name(feature, lane))
                .filter(|candidate| candidate.offset > name.offset)
                .map(|candidate| candidate.offset)
                .min()
                .and_then(|offset| usize::try_from(offset).ok())
                .unwrap_or(lane.native_payload.len());
            let Some(object) = start.and_then(|start| lane.native_payload.get(start..end)) else {
                continue;
            };
            meshes.extend(
                crate::parasolid::extract_streams(object)
                    .into_iter()
                    .filter_map(|stream| crate::parasolid::mesh_polyline(&stream)),
            );
        }
        let [points] = meshes.as_slice() else {
            continue;
        };
        let Some((axis_origin, mut axis_direction, radius, fitted_rise)) =
            fit_helix_polyline(points, *revolutions, *clockwise)
        else {
            continue;
        };
        if fitted_rise * axial_rise.0 < 0.0 {
            axis_direction = Vector3::new(-axis_direction.x, -axis_direction.y, -axis_direction.z);
        }
        let Some(last_point) = points.last() else {
            continue;
        };
        let signed_rise = dot(
            Vector3::new(
                last_point.x - points[0].x,
                last_point.y - points[0].y,
                last_point.z - points[0].z,
            ),
            axis_direction,
        );
        model_feature.definition = FeatureDefinition::Helix {
            axis_origin,
            axis_direction,
            radius: Length(radius),
            pitch: Length(signed_rise / *revolutions),
            revolutions: *revolutions,
            start_angle: *start_angle,
            clockwise: *clockwise,
        };
    }
}

pub(crate) fn fit_helix_polyline(
    points: &[Point3],
    revolutions: f64,
    clockwise: bool,
) -> Option<(Point3, Vector3, f64, f64)> {
    if points.len() < 6 || !revolutions.is_finite() || revolutions <= 0.0 {
        return None;
    }
    let mut parameters = Vec::with_capacity(points.len());
    parameters.push(0.0);
    for pair in points.windows(2) {
        let delta = Vector3::new(
            pair[1].x - pair[0].x,
            pair[1].y - pair[0].y,
            pair[1].z - pair[0].z,
        );
        parameters.push(parameters.last().copied()? + dot(delta, delta).sqrt());
    }
    let total = *parameters.last()?;
    if !total.is_finite() || total <= 0.0 {
        return None;
    }
    let angle = std::f64::consts::TAU * revolutions * if clockwise { -1.0 } else { 1.0 };
    let mut normal = [[0.0; 4]; 4];
    let mut rhs = [[0.0; 3]; 4];
    for (point, distance) in points.iter().zip(parameters) {
        let t = distance / total;
        let row = [1.0, t, (angle * t).cos(), (angle * t).sin()];
        for i in 0..4 {
            for j in 0..4 {
                normal[i][j] += row[i] * row[j];
            }
            rhs[i][0] += row[i] * point.x;
            rhs[i][1] += row[i] * point.y;
            rhs[i][2] += row[i] * point.z;
        }
    }
    let x = solve_four(normal, rhs)?;
    let cosine = Vector3::new(x[2][0], x[2][1], x[2][2]);
    let sine = Vector3::new(x[3][0], x[3][1], x[3][2]);
    let mut axis = cross(cosine, sine);
    let axis_length = dot(axis, axis).sqrt();
    if !axis_length.is_finite() || axis_length <= 0.0 {
        return None;
    }
    axis = Vector3::new(
        axis.x / axis_length,
        axis.y / axis_length,
        axis.z / axis_length,
    );
    let radial_cosine = subtract_axis(cosine, axis);
    let radial_sine = subtract_axis(sine, axis);
    let radius_estimate =
        (dot(radial_cosine, radial_cosine).sqrt() + dot(radial_sine, radial_sine).sqrt()) * 0.5;
    if !radius_estimate.is_finite() || radius_estimate <= 0.0 {
        return None;
    }
    let mut max_error = 0.0f64;
    for (point, distance) in
        points.iter().zip(
            std::iter::once(0.0).chain(points.windows(2).scan(0.0, |sum, pair| {
                let delta = Vector3::new(
                    pair[1].x - pair[0].x,
                    pair[1].y - pair[0].y,
                    pair[1].z - pair[0].z,
                );
                *sum += dot(delta, delta).sqrt();
                Some(*sum)
            })),
        )
    {
        let t = distance / total;
        let row = [1.0, t, (angle * t).cos(), (angle * t).sin()];
        for (coordinate, actual) in [point.x, point.y, point.z].into_iter().enumerate() {
            let fitted = (0..4).map(|i| row[i] * x[i][coordinate]).sum::<f64>();
            max_error = max_error.max((fitted - actual).abs());
        }
    }
    if max_error > radius_estimate * 5.0e-4 {
        return None;
    }
    let snap = (max_error / radius_estimate * 20.0).max(1.0e-10);
    for component in [&mut axis.x, &mut axis.y, &mut axis.z] {
        if component.abs() < snap {
            *component = 0.0;
        }
    }
    let normalized = dot(axis, axis).sqrt();
    axis = Vector3::new(
        axis.x / normalized,
        axis.y / normalized,
        axis.z / normalized,
    );
    let (origin, radius) = fit_circle_on_axis(points, axis)?;
    let displacement = Vector3::new(
        points.last()?.x - points[0].x,
        points.last()?.y - points[0].y,
        points.last()?.z - points[0].z,
    );
    Some((origin, axis, radius, dot(displacement, axis)))
}

fn fit_circle_on_axis(points: &[Point3], axis: Vector3) -> Option<(Point3, f64)> {
    let helper = if axis.x.abs() <= axis.y.abs() && axis.x.abs() <= axis.z.abs() {
        Vector3::new(1.0, 0.0, 0.0)
    } else if axis.y.abs() <= axis.z.abs() {
        Vector3::new(0.0, 1.0, 0.0)
    } else {
        Vector3::new(0.0, 0.0, 1.0)
    };
    let mut u = cross(axis, helper);
    let u_length = dot(u, u).sqrt();
    u = Vector3::new(u.x / u_length, u.y / u_length, u.z / u_length);
    let v = cross(axis, u);
    let reference = points[0];
    let mut normal = [[0.0; 3]; 3];
    let mut rhs = [0.0; 3];
    for point in points {
        let delta = Vector3::new(
            point.x - reference.x,
            point.y - reference.y,
            point.z - reference.z,
        );
        let x = dot(delta, u);
        let y = dot(delta, v);
        let row = [x, y, 1.0];
        let target = -(x * x + y * y);
        for i in 0..3 {
            rhs[i] += row[i] * target;
            for j in 0..3 {
                normal[i][j] += row[i] * row[j];
            }
        }
    }
    let solution = solve_three(normal, rhs)?;
    let center_u = -solution[0] * 0.5;
    let center_v = -solution[1] * 0.5;
    let radius_squared = center_u * center_u + center_v * center_v - solution[2];
    if !radius_squared.is_finite() || radius_squared <= 0.0 {
        return None;
    }
    Some((
        Point3::new(
            reference.x + center_u * u.x + center_v * v.x,
            reference.y + center_u * u.y + center_v * v.y,
            reference.z + center_u * u.z + center_v * v.z,
        ),
        radius_squared.sqrt(),
    ))
}

fn solve_three(mut matrix: [[f64; 3]; 3], mut rhs: [f64; 3]) -> Option<[f64; 3]> {
    for column in 0..3 {
        let pivot = (column..3).max_by(|left, right| {
            matrix[*left][column]
                .abs()
                .total_cmp(&matrix[*right][column].abs())
        })?;
        if matrix[pivot][column].abs() <= 1.0e-14 {
            return None;
        }
        matrix.swap(column, pivot);
        rhs.swap(column, pivot);
        let scale = matrix[column][column];
        for value in &mut matrix[column][column..] {
            *value /= scale;
        }
        rhs[column] /= scale;
        for row in 0..3 {
            if row == column {
                continue;
            }
            let factor = matrix[row][column];
            let pivot_row = matrix[column];
            for (target, pivot) in matrix[row].iter_mut().zip(pivot_row).skip(column) {
                *target -= factor * pivot;
            }
            rhs[row] -= factor * rhs[column];
        }
    }
    Some(rhs)
}

fn subtract_axis(vector: Vector3, axis: Vector3) -> Vector3 {
    let axial = dot(vector, axis);
    Vector3::new(
        vector.x - axial * axis.x,
        vector.y - axial * axis.y,
        vector.z - axial * axis.z,
    )
}

fn solve_four(mut matrix: [[f64; 4]; 4], mut rhs: [[f64; 3]; 4]) -> Option<[[f64; 3]; 4]> {
    for column in 0..4 {
        let pivot = (column..4).max_by(|left, right| {
            matrix[*left][column]
                .abs()
                .total_cmp(&matrix[*right][column].abs())
        })?;
        if matrix[pivot][column].abs() <= 1.0e-14 {
            return None;
        }
        matrix.swap(column, pivot);
        rhs.swap(column, pivot);
        let scale = matrix[column][column];
        for value in &mut matrix[column][column..] {
            *value /= scale;
        }
        for value in &mut rhs[column] {
            *value /= scale;
        }
        for row in 0..4 {
            if row == column {
                continue;
            }
            let factor = matrix[row][column];
            let pivot_row = matrix[column];
            for (target, pivot) in matrix[row].iter_mut().zip(pivot_row).skip(column) {
                *target -= factor * pivot;
            }
            let rhs_pivot = rhs[column];
            for (target, pivot) in rhs[row].iter_mut().zip(rhs_pivot) {
                *target -= factor * pivot;
            }
        }
    }
    Some(rhs)
}

pub(crate) fn resolve_scalar_operand_markers<'a>(
    entities: impl IntoIterator<Item = &'a SketchInputEntity>,
    operands: &[FeatureInputOperand],
) -> Vec<Option<&'a SketchInputEntity>> {
    let entities = entities.into_iter().collect::<Vec<_>>();
    let mut resolved = operands
        .iter()
        .map(|operand| {
            resolve_operand_marker(entities.iter().copied(), operand.kind, operand.entity_index)
        })
        .collect::<Vec<_>>();
    if let ([first_operand, second_operand], [Some(first), Some(second)]) =
        (operands, resolved.as_slice())
    {
        if first.id == second.id && first_operand.entity_index != second_operand.entity_index {
            let alternatives = [
                resolve_operand_marker_excluding(
                    entities.iter().copied(),
                    first_operand.kind,
                    first_operand.entity_index,
                    &HashSet::from([second.id.clone()]),
                )
                .map(|alternative| [alternative, *second]),
                resolve_operand_marker_excluding(
                    entities.iter().copied(),
                    second_operand.kind,
                    second_operand.entity_index,
                    &HashSet::from([first.id.clone()]),
                )
                .map(|alternative| [*first, alternative]),
            ]
            .into_iter()
            .flatten()
            .filter(|[left, right]| left.id != right.id)
            .collect::<Vec<_>>();
            if let [alternative] = alternatives.as_slice() {
                resolved = alternative.iter().copied().map(Some).collect();
            }
        }
    }
    let resolved_siblings = resolved
        .iter()
        .flatten()
        .map(|entity| entity.id.clone())
        .collect::<HashSet<_>>();
    for (operand, target) in operands.iter().zip(&mut resolved) {
        if target.is_none() {
            *target = resolve_operand_marker_excluding(
                entities.iter().copied(),
                operand.kind,
                operand.entity_index,
                &resolved_siblings,
            );
        }
    }
    resolved
}

pub(crate) fn resolve_operand_marker<'a>(
    entities: impl IntoIterator<Item = &'a SketchInputEntity>,
    kind: FeatureInputOperandKind,
    address: u16,
) -> Option<&'a SketchInputEntity> {
    resolve_operand_marker_excluding(entities, kind, address, &HashSet::new())
}

fn resolve_operand_marker_excluding<'a>(
    entities: impl IntoIterator<Item = &'a SketchInputEntity>,
    kind: FeatureInputOperandKind,
    address: u16,
    excluded: &HashSet<String>,
) -> Option<&'a SketchInputEntity> {
    let entities = entities.into_iter().collect::<Vec<_>>();
    if kind == FeatureInputOperandKind::Native(0xbc7c) {
        let indexed = entities
            .iter()
            .copied()
            .filter(|entity| entity.object_index == Some(u32::from(address)))
            .filter(|entity| entity.coordinates_m.is_some())
            .filter(|entity| {
                matches!(
                    entity.kind,
                    SketchInputKind::Point
                        | SketchInputKind::ConstrainedPoint
                        | SketchInputKind::LineOrCircle
                        | SketchInputKind::Arc
                )
            })
            .filter(|entity| !excluded.contains(&entity.id))
            .collect::<Vec<_>>();
        if let [entity] = indexed.as_slice() {
            return Some(*entity);
        }
    }
    if kind == FeatureInputOperandKind::Native(0xbc87) {
        let indexed = entities
            .iter()
            .copied()
            .filter(|entity| entity.object_index == Some(u32::from(address)))
            .filter(|entity| entity.coordinates_m.is_some())
            .filter(|entity| {
                matches!(
                    entity.kind,
                    SketchInputKind::LineOrCircle | SketchInputKind::Arc
                )
            })
            .filter(|entity| !excluded.contains(&entity.id))
            .collect::<Vec<_>>();
        if let [entity] = indexed.as_slice() {
            return Some(*entity);
        }
    }
    let mut compatible = entities
        .iter()
        .copied()
        .filter(|entity| operand_accepts_marker(kind, entity.kind))
        .collect::<Vec<_>>();
    compatible.sort_unstable_by_key(|entity| entity.offset);
    let mut ordinal_link_graph = false;
    if operand_uses_compatible_ordinal(kind) {
        if let Some(entity) = compatible
            .get(usize::from(address))
            .filter(|entity| !excluded.contains(&entity.id))
        {
            return Some(*entity);
        }
        if !point_operand_uses_link_graph(kind) {
            return None;
        }
        ordinal_link_graph = true;
    }
    let exact = if ordinal_link_graph {
        Vec::new()
    } else {
        compatible
            .iter()
            .copied()
            .filter(|entity| entity.local_id == Some(u32::from(address)))
            .filter(|entity| !excluded.contains(&entity.id))
            .collect::<Vec<_>>()
    };
    match exact.as_slice() {
        [entity] => Some(*entity),
        [] => {
            let mut indirect = if point_operand_uses_link_graph(kind) {
                linked_point_markers(&entities, address, kind, excluded)
            } else if operand_accepts_link_indirection(kind) {
                entities
                    .iter()
                    .copied()
                    .filter(|entity| entity.local_id == Some(u32::from(address)))
                    .flat_map(|entity| &entity.links)
                    .filter_map(|link| {
                        entities
                            .iter()
                            .copied()
                            .find(|entity| entity.id == link.entity_ref)
                    })
                    .filter(|entity| operand_accepts_marker(kind, entity.kind))
                    .filter(|entity| !excluded.contains(&entity.id))
                    .collect::<Vec<_>>()
            } else {
                Vec::new()
            };
            indirect.sort_unstable_by_key(|entity| entity.id.as_str());
            indirect.dedup_by_key(|entity| entity.id.as_str());
            match indirect.as_slice() {
                [entity] => Some(*entity),
                [] if point_operand_uses_link_graph(kind) && !excluded.is_empty() && {
                    let linked = linked_point_markers(&entities, address, kind, &HashSet::new());
                    !linked.is_empty() && linked.iter().all(|entity| excluded.contains(&entity.id))
                } =>
                {
                    let remaining = compatible
                        .iter()
                        .copied()
                        .filter(|entity| !excluded.contains(&entity.id))
                        .collect::<Vec<_>>();
                    let [entity] = remaining.as_slice() else {
                        return None;
                    };
                    Some(*entity)
                }
                [] if operand_allows_compatible_ordinal_fallback(kind) => {
                    compatible.get(usize::from(address)).copied().or_else(|| {
                        (kind == FeatureInputOperandKind::Native(0xbc7c))
                            .then(|| {
                                entities
                                    .iter()
                                    .copied()
                                    .filter(|entity| {
                                        matches!(
                                            entity.kind,
                                            SketchInputKind::LineOrCircle | SketchInputKind::Arc
                                        ) && entity.local_id == Some(u32::from(address))
                                            && !excluded.contains(&entity.id)
                                    })
                                    .collect::<Vec<_>>()
                            })
                            .and_then(|candidates| {
                                let [candidate] = candidates.as_slice() else {
                                    return None;
                                };
                                Some(*candidate)
                            })
                    })
                }
                _ => None,
            }
        }
        _ => unique_marker_candidate(
            &exact
                .iter()
                .map(|entity| (entity.id.clone(), entity.coordinates_m.is_some()))
                .collect::<Vec<_>>(),
        )
        .and_then(|id| exact.iter().copied().find(|entity| entity.id == id)),
    }
}

fn point_operand_uses_link_graph(kind: FeatureInputOperandKind) -> bool {
    matches!(kind, FeatureInputOperandKind::D6)
}

fn linked_point_markers<'a>(
    entities: &[&'a SketchInputEntity],
    address: u16,
    kind: FeatureInputOperandKind,
    excluded: &HashSet<String>,
) -> Vec<&'a SketchInputEntity> {
    let by_id = entities
        .iter()
        .map(|entity| (entity.id.as_str(), *entity))
        .collect::<HashMap<_, _>>();
    let mut pending = entities
        .iter()
        .copied()
        .filter(|entity| entity.local_id == Some(u32::from(address)))
        .filter(|entity| !operand_accepts_marker(kind, entity.kind))
        .map(|entity| entity.id.as_str())
        .collect::<Vec<_>>();
    let mut visited = HashSet::new();
    let mut compatible = Vec::new();
    while let Some(id) = pending.pop() {
        if !visited.insert(id) {
            continue;
        }
        let Some(entity) = by_id.get(id).copied() else {
            continue;
        };
        if operand_accepts_marker(kind, entity.kind) && !excluded.contains(&entity.id) {
            compatible.push(entity);
            continue;
        }
        pending.extend(entity.links.iter().map(|link| link.entity_ref.as_str()));
    }
    compatible
}

fn operand_accepts_link_indirection(kind: FeatureInputOperandKind) -> bool {
    matches!(
        kind,
        FeatureInputOperandKind::E1
            | FeatureInputOperandKind::Native(0x8386 | 0x83fe | 0x8dda | 0xbc87)
    )
}

fn compact_body_selections(
    histories: &[crate::records::FeatureHistory],
    lane: &FeatureInputLane,
) -> Vec<FeatureInputBodySelection> {
    let mut objects = histories
        .iter()
        .flat_map(|history| &history.features)
        .filter_map(|feature| Some((feature_object_name(feature, lane)?, feature)))
        .collect::<Vec<_>>();
    objects.sort_by_key(|(name, _)| name.offset);
    let lane_key = lane
        .id
        .rsplit_once('#')
        .map_or(lane.id.as_str(), |(_, key)| key);
    let state_token = compact_body_state_token(lane);
    let mut result = Vec::new();
    for (object_index, &(name, feature)) in objects.iter().enumerate() {
        if native_object_class(feature.input_class.as_deref().unwrap_or_default()).kind
            != NativeClassKind::DeleteBody
        {
            continue;
        }
        let Some(start) = usize::try_from(name.offset).ok() else {
            continue;
        };
        let next = objects.get(object_index + 1);
        let end = next
            .and_then(|(next, _)| usize::try_from(next.offset).ok())
            .unwrap_or(lane.native_payload.len());
        let next_token = next.and_then(|(next, next_feature)| {
            (native_object_class(next_feature.input_class.as_deref().unwrap_or_default()).kind
                == NativeClassKind::DeleteBody)
                .then(|| {
                    usize::try_from(next.offset)
                        .ok()
                        .and_then(|offset| repeated_class_token(&lane.native_payload, offset))
                })
                .flatten()
        });
        let Some((offset, local_body_ids)) = lane
            .native_payload
            .get(start..end)
            .and_then(|payload| compact_body_selection_vector(payload, start, next_token))
        else {
            continue;
        };
        result.push(FeatureInputBodySelection {
            id: format!("sldprt:feature-input:body-selection#{lane_key}:{offset}"),
            parent: lane.id.clone(),
            ordinal: result.len() as u32,
            offset: offset as u64,
            object_name_ref: name.id.clone(),
            feature_ref: feature.id.clone(),
            local_body_ids,
            body_state_ids: state_token.map_or_else(Vec::new, |token| {
                compact_body_state_ids(&lane.native_payload, start, offset, token)
            }),
            mode: state_token.and_then(|token| {
                compact_body_retention_mode(&lane.native_payload, start, offset, token)
            }),
        });
    }
    result
}

fn compact_body_state_token(lane: &FeatureInputLane) -> Option<u16> {
    let mut classes = lane
        .classes
        .iter()
        .filter(|class| class.name == "moDeleteBodyData_c");
    let class = classes.next()?;
    if classes.next().is_some() {
        return None;
    }
    let offset = usize::try_from(class.offset).ok()?;
    Some(u16::from_le_bytes(
        lane.native_payload
            .get(offset + 8 + class.name.len()..offset + 10 + class.name.len())?
            .try_into()
            .ok()?,
    ))
}

pub(crate) fn compact_body_state_ids_for_selection(
    lane: &FeatureInputLane,
    selection: &FeatureInputBodySelection,
) -> Vec<u32> {
    let Some(token) = compact_body_state_token(lane) else {
        return Vec::new();
    };
    let Some(start) = lane
        .names
        .iter()
        .find(|name| name.id == selection.object_name_ref)
        .and_then(|name| usize::try_from(name.offset).ok())
    else {
        return Vec::new();
    };
    let Some(end) = usize::try_from(selection.offset).ok() else {
        return Vec::new();
    };
    compact_body_state_ids(&lane.native_payload, start, end, token)
}

pub(crate) fn compact_body_retention_mode_for_selection(
    lane: &FeatureInputLane,
    selection: &FeatureInputBodySelection,
) -> Option<cadmpeg_ir::features::BodyRetentionMode> {
    let token = compact_body_state_token(lane)?;
    let start = lane
        .names
        .iter()
        .find(|name| name.id == selection.object_name_ref)
        .and_then(|name| usize::try_from(name.offset).ok())?;
    let end = usize::try_from(selection.offset).ok()?;
    compact_body_retention_mode(&lane.native_payload, start, end, token)
}

fn compact_body_retention_mode(
    payload: &[u8],
    start: usize,
    end: usize,
    token: u16,
) -> Option<cadmpeg_ir::features::BodyRetentionMode> {
    const HEADER_LEN: usize = 83;
    let token = token.to_le_bytes();
    let state_end = (start..end.saturating_sub(HEADER_LEN - 1))
        .filter(|offset| compact_body_state_header(payload, *offset, token).is_some())
        .map(|offset| offset + HEADER_LEN)
        .max()?;
    let field = payload.get(state_end..state_end + 10)?;
    if field[0..2] != [0x30, 0x80] || field[6..10] != [0; 4] {
        return None;
    }
    match u32::from_le_bytes(field[2..6].try_into().ok()?) {
        0 => Some(cadmpeg_ir::features::BodyRetentionMode::KeepSelected),
        1 => Some(cadmpeg_ir::features::BodyRetentionMode::DeleteSelected),
        _ => None,
    }
}

fn compact_body_state_header(payload: &[u8], offset: usize, token: [u8; 2]) -> Option<&[u8]> {
    const HEADER_LEN: usize = 83;
    let header = payload.get(offset..offset + HEADER_LEN)?;
    (header[0..2] == token
        && header[2..11] == [0x2b, 0x80, 0x02, 0, 0, 0, 0, 0, 0]
        && header[11..15] == header[15..19]
        && header[19..47].iter().all(|byte| *byte == 0)
        && header[47..63].iter().all(|byte| *byte == 0xff)
        && header[63..83].iter().all(|byte| *byte == 0))
    .then_some(header)
}

fn compact_body_state_ids(payload: &[u8], start: usize, end: usize, token: u16) -> Vec<u32> {
    const HEADER_LEN: usize = 83;
    let token = token.to_le_bytes();
    let mut result = Vec::new();
    for offset in start..end.saturating_sub(HEADER_LEN - 1) {
        let Some(header) = compact_body_state_header(payload, offset, token) else {
            continue;
        };
        result.push(u32::from_le_bytes(
            header[11..15].try_into().expect("four-byte body id"),
        ));
    }
    result
}

fn compact_edge_selections(
    histories: &[crate::records::FeatureHistory],
    lane: &FeatureInputLane,
) -> Vec<FeatureInputEdgeSelection> {
    let history_features = histories
        .iter()
        .flat_map(|history| &history.features)
        .cloned()
        .collect::<Vec<_>>();
    let mut objects = histories
        .iter()
        .flat_map(|history| &history.features)
        .filter_map(|feature| Some((feature_object_name(feature, lane)?, feature)))
        .collect::<Vec<_>>();
    objects.sort_by_key(|(name, _)| name.offset);
    let lane_key = lane
        .id
        .rsplit_once('#')
        .map_or(lane.id.as_str(), |(_, key)| key);
    let mut result = Vec::new();
    let mut compact_edge_classes = lane
        .classes
        .iter()
        .filter(|class| class.name == "moCompEdge_c");
    let Some(compact_edge_class) = compact_edge_classes.next() else {
        return result;
    };
    if compact_edge_classes.next().is_some() {
        return result;
    }
    let class_name_end = usize::try_from(compact_edge_class.offset)
        .ok()
        .and_then(|offset| offset.checked_add(6 + compact_edge_class.name.len()));
    let compact_edge_token = class_name_end.and_then(|offset| {
        Some(u16::from_le_bytes(
            lane.native_payload
                .get(offset..offset + 2)?
                .try_into()
                .ok()?,
        ))
    });
    for (object_index, &(name, feature)) in objects.iter().enumerate() {
        if !matches!(
            native_object_class(feature.input_class.as_deref().unwrap_or_default()).kind,
            NativeClassKind::Fillet | NativeClassKind::Chamfer
        ) {
            continue;
        }
        let Some(start) = usize::try_from(name.offset).ok() else {
            continue;
        };
        let end = objects
            .get(object_index + 1)
            .and_then(|(next, _)| usize::try_from(next.offset).ok())
            .unwrap_or(lane.native_payload.len());
        let direct_child = usize::try_from(compact_edge_class.offset)
            .ok()
            .filter(|offset| (start..end).contains(offset));
        let mut selections = Vec::new();
        if let Some(child_start) = direct_child {
            if let Some(selection) = lane
                .native_payload
                .get(child_start..end)
                .and_then(|payload| compact_edge_selection_vector(payload, child_start))
            {
                selections.push(selection);
            }
        }
        if let Some(token) = compact_edge_token {
            selections.extend(repeated_edge_selections(
                &lane.native_payload,
                start,
                end,
                token,
            ));
        }
        selections.extend(edge_selection_vectors_in_interval(
            &lane.native_payload,
            start,
            end,
        ));
        selections.sort_unstable_by_key(|selection| selection.0);
        selections.dedup_by_key(|selection| selection.0);
        for (offset, local_edge_ids) in selections {
            let components =
                compact_edge_component_path_at(&lane.native_payload, offset).unwrap_or_default();
            let terminal_feature_ref = compact_edge_owner_feature_at(
                &lane.native_payload,
                offset,
                &components,
                &history_features,
            );
            let producer_feature_refs = compact_edge_producer_features_at(
                &lane.native_payload,
                offset,
                &components,
                &history_features,
            );
            result.push(FeatureInputEdgeSelection {
                id: format!("sldprt:feature-input:edge-selection#{lane_key}:{offset}"),
                parent: lane.id.clone(),
                ordinal: result.len() as u32,
                offset: offset as u64,
                object_name_ref: name.id.clone(),
                feature_ref: feature.id.clone(),
                local_edge_ids,
                components,
                producer_feature_refs,
                terminal_feature_ref,
            });
        }
    }
    result
}

fn compact_surface_selections(
    histories: &[crate::records::FeatureHistory],
    lane: &FeatureInputLane,
) -> Vec<FeatureInputSurfaceSelection> {
    let history_features = histories
        .iter()
        .flat_map(|history| &history.features)
        .cloned()
        .collect::<Vec<_>>();
    let mut classes = lane
        .classes
        .iter()
        .filter(|class| class.name == "moCompSurfaceBody_c");
    let surface_class = classes.next().filter(|_| classes.next().is_none());
    let surface_token = surface_class.and_then(|class| {
        usize::try_from(class.offset)
            .ok()
            .and_then(|offset| offset.checked_add(6 + class.name.len()))
            .and_then(|offset| lane.native_payload.get(offset..offset + 2))
    });
    let mut objects = histories
        .iter()
        .flat_map(|history| &history.features)
        .filter_map(|feature| Some((feature_object_name(feature, lane)?, feature)))
        .collect::<Vec<_>>();
    objects.sort_by_key(|(name, _)| name.offset);
    let lane_key = lane
        .id
        .rsplit_once('#')
        .map_or(lane.id.as_str(), |(_, key)| key);
    let mut result = Vec::new();
    for (index, &(name, feature)) in objects.iter().enumerate() {
        let kind = native_object_class(feature.input_class.as_deref().unwrap_or_default()).kind;
        let Some(start) = usize::try_from(name.offset).ok() else {
            continue;
        };
        let mut end_index = index + 1;
        if kind == NativeClassKind::Extrusion
            && objects.get(end_index).is_some_and(|(_, next)| {
                native_object_class(next.input_class.as_deref().unwrap_or_default()).kind
                    == NativeClassKind::ProfileFeature
            })
        {
            end_index += 1;
        }
        let end = objects
            .get(end_index)
            .and_then(|(next, _)| usize::try_from(next.offset).ok())
            .unwrap_or(lane.native_payload.len());
        let candidates = match kind {
            NativeClassKind::Thicken => surface_token.map_or_else(Vec::new, |token| {
                (start..end.saturating_sub(105))
                    .filter(|offset| lane.native_payload.get(*offset..*offset + 2) == Some(token))
                    .filter_map(|offset| {
                        let marker = offset + 103;
                        compact_surface_selection_at(&lane.native_payload, marker)
                            .map(|ids| (marker, ids))
                    })
                    .collect()
            }),
            NativeClassKind::Extrusion => (start..end.saturating_sub(103))
                .filter_map(|offset| {
                    compact_extrusion_to_face_at(&lane.native_payload, offset)
                        .or_else(|| {
                            compact_extrusion_to_vertex_at(&lane.native_payload, offset)
                                .map(|(marker, _)| marker)
                        })
                        .or_else(|| {
                            compact_extrusion_offset_from_face_at(&lane.native_payload, offset, end)
                        })
                })
                .filter_map(|marker| {
                    compact_termination_reference_path_at(&lane.native_payload, marker)
                        .map(|ids| (marker, ids))
                })
                .collect(),
            _ => continue,
        };
        let [(offset, components)] = candidates.as_slice() else {
            continue;
        };
        result.push(FeatureInputSurfaceSelection {
            id: format!("sldprt:feature-input:surface-selection#{lane_key}:{offset}"),
            parent: lane.id.clone(),
            ordinal: result.len() as u32,
            offset: *offset as u64,
            object_name_ref: name.id.clone(),
            feature_ref: feature.id.clone(),
            producer_feature_refs: component_path_features(components, &history_features),
            terminal_feature_ref: component_path_terminal_feature(components, &history_features),
            components: components.clone(),
        });
    }
    result
}

pub(crate) fn compact_surface_selection_at(
    payload: &[u8],
    marker: usize,
) -> Option<Vec<FeatureInputComponentPathEntry>> {
    let count_start = marker.checked_sub(12)?;
    let kind_start = marker.checked_sub(8)?;
    if payload.get(marker..marker + 16)? != COMPACT_EDGE_VECTOR_MARKER
        || payload.get(count_start..count_start + 4)? != 6u32.to_le_bytes()
        || payload.get(kind_start..kind_start + 4)? != [0x04, 0x02, 0, 0]
        || payload.get(marker + 16..marker + 18)? != [0, 0]
    {
        return None;
    }
    let mut cursor = marker + 18;
    let signature = payload.get(cursor + 4..cursor + 16)?.to_vec();
    let mut components = Vec::new();
    while components.len() < 6 && payload.get(cursor + 4..cursor + 16) == Some(signature.as_slice())
    {
        components.push(FeatureInputComponentPathEntry {
            instance: u16::from_le_bytes(payload.get(cursor..cursor + 2)?.try_into().ok()?),
            type_signature: signature.as_slice().try_into().ok()?,
            local_id: u32::from_le_bytes(payload.get(cursor + 16..cursor + 20)?.try_into().ok()?),
        });
        cursor += 20;
        if payload.get(cursor + 4..cursor + 16) != Some(signature.as_slice())
            && payload.get(cursor + 8..cursor + 20) == Some(signature.as_slice())
        {
            cursor += 4;
        }
    }
    (!components.is_empty()).then_some(components)
}

pub(crate) fn compact_surface_reference_at(
    payload: &[u8],
    marker: usize,
) -> Option<Vec<FeatureInputComponentPathEntry>> {
    compact_surface_selection_at(payload, marker)
        .or_else(|| compact_termination_reference_path_at(payload, marker))
}

fn repeated_edge_selections(
    payload: &[u8],
    start: usize,
    end: usize,
    token: u16,
) -> Vec<(usize, Vec<u32>)> {
    let token = token.to_le_bytes();
    let mut selections = Vec::new();
    for offset in start..end.saturating_sub(110) {
        if payload.get(offset..offset + 2) != Some(token.as_slice())
            || payload.get(offset + 2) != Some(&2)
        {
            continue;
        }
        let marker = offset + 108;
        if let Some(ids) = compact_edge_selection_at(payload, marker) {
            selections.push((marker, ids));
        }
    }
    selections
}

fn edge_selection_vectors_in_interval(
    payload: &[u8],
    start: usize,
    end: usize,
) -> Vec<(usize, Vec<u32>)> {
    (start.saturating_add(12)..end.saturating_sub(COMPACT_EDGE_VECTOR_MARKER.len()))
        .filter(|marker| {
            payload.get(*marker..*marker + COMPACT_EDGE_VECTOR_MARKER.len())
                == Some(COMPACT_EDGE_VECTOR_MARKER.as_slice())
        })
        .filter_map(|marker| compact_edge_selection_at(payload, marker).map(|ids| (marker, ids)))
        .collect()
}

const COMPACT_EDGE_VECTOR_MARKER: [u8; 16] = [
    0x7d, 0xc3, 0x94, 0x25, 0xad, 0x49, 0xb2, 0x54, 0x7d, 0xc3, 0x94, 0x25, 0xad, 0x49, 0xb2, 0x54,
];

fn compact_edge_selection_vector(payload: &[u8], base: usize) -> Option<(usize, Vec<u32>)> {
    for marker in 12..=payload
        .len()
        .saturating_sub(COMPACT_EDGE_VECTOR_MARKER.len())
    {
        if payload.get(marker..marker + 16) != Some(COMPACT_EDGE_VECTOR_MARKER.as_slice()) {
            continue;
        }
        if let Some(ids) = compact_edge_selection_at(payload, marker) {
            return Some((base + marker, ids));
        }
    }
    None
}

pub(crate) fn compact_edge_selection_at(payload: &[u8], marker: usize) -> Option<Vec<u32>> {
    let count_start = marker.checked_sub(12)?;
    let kind_start = marker.checked_sub(8)?;
    if payload.get(marker..marker + 16)? != COMPACT_EDGE_VECTOR_MARKER
        || payload.get(kind_start..kind_start + 4)? != [0x00, 0x02, 0x00, 0x00]
        || payload.get(marker + 16..marker + 18)? != [0, 0]
    {
        return None;
    }
    let count = usize::try_from(u32::from_le_bytes(
        payload.get(count_start..count_start + 4)?.try_into().ok()?,
    ))
    .ok()?;
    if !(1..=64).contains(&count) {
        return None;
    }
    compact_homogeneous_edge_ids(payload, marker + 18, count)
        .or_else(|| {
            compact_edge_component_path(payload, marker, count).map(|(components, _)| {
                components
                    .into_iter()
                    .map(|component| component.local_id)
                    .collect()
            })
        })
        .or_else(|| compact_u16_edge_ids(payload, marker + 18, count))
}

pub(crate) fn compact_edge_component_path_at(
    payload: &[u8],
    marker: usize,
) -> Option<Vec<FeatureInputComponentPathEntry>> {
    let count_start = marker.checked_sub(12)?;
    let kind_start = marker.checked_sub(8)?;
    if payload.get(marker..marker + 16)? != COMPACT_EDGE_VECTOR_MARKER
        || payload.get(kind_start..kind_start + 4)? != [0x00, 0x02, 0x00, 0x00]
        || payload.get(marker + 16..marker + 18)? != [0, 0]
    {
        return None;
    }
    let count = usize::try_from(u32::from_le_bytes(
        payload.get(count_start..count_start + 4)?.try_into().ok()?,
    ))
    .ok()
    .filter(|count| (1..=64).contains(count))?;
    compact_edge_component_path(payload, marker, count).map(|(components, _)| components)
}

fn compact_edge_component_path(
    payload: &[u8],
    marker: usize,
    count: usize,
) -> Option<(Vec<FeatureInputComponentPathEntry>, Option<u32>)> {
    compact_heterogeneous_component_path(payload, marker + 18, count)
        .map(|(components, _)| (components, None))
        .or_else(|| {
            let (components, end) = (count > 1)
                .then(|| compact_heterogeneous_component_path(payload, marker + 18, count - 1))
                .flatten()?;
            let trailer = payload.get(end..end + 36)?;
            if trailer[..8] != [1, 0, 0, 0, 0, 0, 0, 0]
                || trailer[8..12] != [0x4a, 0x80, 0, 0]
                || trailer[12..14] == [0, 0]
                || trailer[14..16] != [0x37, 0]
                || trailer[20..24].iter().all(|byte| *byte == 0)
                || trailer[24..].iter().any(|byte| *byte != 0)
            {
                return None;
            }
            let source = u32::from_le_bytes(trailer[16..20].try_into().ok()?);
            (source != 0).then_some((components, Some(source)))
        })
}

pub(crate) fn compact_edge_owner_feature_at(
    payload: &[u8],
    marker: usize,
    components: &[FeatureInputComponentPathEntry],
    features: &[crate::records::Feature],
) -> Option<String> {
    let count = usize::try_from(u32::from_le_bytes(
        payload
            .get(marker.checked_sub(12)?..marker - 8)?
            .try_into()
            .ok()?,
    ))
    .ok()?;
    let (_, owner_source) = compact_edge_component_path(payload, marker, count)?;
    owner_source
        .and_then(|source| {
            features.iter().find(|feature| {
                feature.source_id.as_deref().and_then(|id| id.parse().ok()) == Some(source)
            })
        })
        .map(|feature| feature.id.clone())
        .or_else(|| component_path_terminal_feature(components, features))
}

pub(crate) fn compact_edge_producer_features_at(
    payload: &[u8],
    marker: usize,
    components: &[FeatureInputComponentPathEntry],
    features: &[crate::records::Feature],
) -> Vec<String> {
    let mut producers = component_path_features(components, features);
    if let Some(owner) = compact_edge_owner_feature_at(payload, marker, components, features) {
        if !producers.contains(&owner) {
            producers.push(owner);
        }
    }
    producers
}

fn compact_homogeneous_edge_ids(
    payload: &[u8],
    mut cursor: usize,
    count: usize,
) -> Option<Vec<u32>> {
    let signature = payload.get(cursor + 4..cursor + 16)?.to_vec();
    // Each edge id consumes at least a 20-byte record from `cursor` onward.
    bounded_len(count as u64, 20, payload.len().saturating_sub(cursor))?;
    let mut ids = Vec::with_capacity(count);
    for index in 0..count {
        if payload.get(cursor + 4..cursor + 16)? != signature {
            return None;
        }
        ids.push(u32::from_le_bytes(
            payload.get(cursor + 16..cursor + 20)?.try_into().ok()?,
        ));
        cursor += 20;
        if index + 1 < count && payload.get(cursor + 4..cursor + 16)? != signature {
            if payload.get(cursor..cursor + 4)? == [0; 4]
                && payload.get(cursor + 8..cursor + 20)? == signature
            {
                cursor += 4;
            } else {
                match payload.get(cursor..cursor + 8)? {
                    [0, 0, 0, 0, 0, 0, 0, 0] | [0xff, 0xff, 0xff, 0xff, 0, 0, 0, 0] => {
                        cursor += 8;
                    }
                    _ => return None,
                }
            }
        }
    }
    Some(ids)
}

fn compact_heterogeneous_component_path(
    payload: &[u8],
    mut cursor: usize,
    count: usize,
) -> Option<(Vec<FeatureInputComponentPathEntry>, usize)> {
    let entry_at = |offset: usize| {
        let instance = payload.get(offset..offset + 4)?;
        (instance[0..2] != [0, 0]
            && instance[0..2] != [0xff, 0xff]
            && instance[2..4] == [0, 0]
            && payload.get(offset + 4..offset + 6)? != [0, 0]
            && payload.get(offset + 4..offset + 20).is_some())
        .then_some(())
    };
    // Each path entry consumes at least a 20-byte record from `cursor` onward.
    bounded_len(count as u64, 20, payload.len().saturating_sub(cursor))?;
    let mut entries = Vec::with_capacity(count);
    for index in 0..count {
        entry_at(cursor)?;
        entries.push(FeatureInputComponentPathEntry {
            instance: u16::from_le_bytes(payload.get(cursor..cursor + 2)?.try_into().ok()?),
            type_signature: payload.get(cursor + 4..cursor + 16)?.try_into().ok()?,
            local_id: u32::from_le_bytes(payload.get(cursor + 16..cursor + 20)?.try_into().ok()?),
        });
        cursor += 20;
        if index + 1 == count {
            continue;
        }
        let gaps = [0usize, 4, 8]
            .into_iter()
            .filter(|gap| match *gap {
                0 => true,
                4 => payload.get(cursor..cursor + 4) == Some(&[0; 4]),
                8 => matches!(
                    payload.get(cursor..cursor + 8),
                    Some(
                        [0, 0, 0, 0, 0, 0, 0, 0]
                            | [0xff, 0xff, 0xff, 0xff, 0, 0, 0, 0]
                            | [0xa0, 0x86, 0x01, 0x00, 0, 0, 0, 0]
                    )
                ),
                _ => false,
            })
            .filter(|gap| entry_at(cursor + gap).is_some())
            .collect::<Vec<_>>();
        let [gap] = gaps.as_slice() else {
            return None;
        };
        cursor += gap;
    }
    Some((entries, cursor))
}

fn compact_heterogeneous_edge_path(
    payload: &[u8],
    cursor: usize,
    count: usize,
) -> Option<(Vec<u32>, usize)> {
    compact_heterogeneous_component_path(payload, cursor, count).map(|(entries, end)| {
        (
            entries.into_iter().map(|entry| entry.local_id).collect(),
            end,
        )
    })
}

fn compact_u16_edge_ids(payload: &[u8], cursor: usize, count: usize) -> Option<Vec<u32>> {
    let end = cursor.checked_add(count.checked_mul(2)?)?;
    let ids = payload
        .get(cursor..end)?
        .chunks_exact(2)
        .map(|bytes| u32::from(u16::from_le_bytes([bytes[0], bytes[1]])))
        .collect::<Vec<_>>();
    let suffix = payload.get(end..end + 19)?;
    (ids.iter().all(|id| *id != 0)
        && suffix[..16].iter().all(|byte| *byte == 0)
        && suffix[16..19] == [0xff, 0xfe, 0xff])
    .then_some(ids)
}

fn compact_body_selection_vector(
    payload: &[u8],
    base: usize,
    next_object_token: Option<u16>,
) -> Option<(usize, Vec<u32>)> {
    const SCHEMA: &[u8] = &11000u32.to_le_bytes();
    for relative in (0..=payload.len().checked_sub(16)?).rev() {
        if payload.get(relative..relative + 4)? != SCHEMA
            || payload.get(relative + 4..relative + 12)? != [0; 8]
        {
            continue;
        }
        let Some(count_bytes) = payload.get(relative + 12..relative + 16) else {
            continue;
        };
        let Ok(count) = usize::try_from(u32::from_le_bytes(
            count_bytes.try_into().expect("four-byte count"),
        )) else {
            continue;
        };
        let Some(ids_end) = count
            .checked_mul(4)
            .and_then(|byte_len| relative.checked_add(16)?.checked_add(byte_len))
        else {
            continue;
        };
        let Some(sentinel_end) = ids_end.checked_add(4) else {
            continue;
        };
        let Some(zeros_end) = sentinel_end.checked_add(12) else {
            continue;
        };
        let Some(suffix) = payload.get(zeros_end..) else {
            continue;
        };
        let valid_suffix = matches!(suffix, [] | [0, 0, 0, 0])
            || next_object_token.is_some_and(|token| suffix == token.to_le_bytes());
        if payload.get(ids_end..sentinel_end) != Some(u32::MAX.to_le_bytes().as_slice())
            || payload.get(sentinel_end..zeros_end) != Some([0; 12].as_slice())
            || !valid_suffix
        {
            continue;
        }
        let Some(ids) = payload.get(relative + 16..ids_end) else {
            continue;
        };
        let local_body_ids = ids
            .chunks_exact(4)
            .map(|bytes| u32::from_le_bytes(bytes.try_into().expect("four-byte chunk")))
            .collect();
        return Some((base + relative, local_body_ids));
    }
    None
}

pub(crate) fn compact_body_selection_at(payload: &[u8], offset: usize) -> Option<Vec<u32>> {
    if payload.get(offset..offset + 4)? != 11000u32.to_le_bytes()
        || payload.get(offset + 4..offset + 12)? != [0; 8]
    {
        return None;
    }
    let count = usize::try_from(u32::from_le_bytes(
        payload.get(offset + 12..offset + 16)?.try_into().ok()?,
    ))
    .ok()?;
    let ids_end = offset.checked_add(16 + count.checked_mul(4)?)?;
    let sentinel_end = ids_end.checked_add(4)?;
    let zeros_end = sentinel_end.checked_add(12)?;
    if payload.get(ids_end..sentinel_end)? != u32::MAX.to_le_bytes()
        || payload.get(sentinel_end..zeros_end)? != [0; 12]
    {
        return None;
    }
    Some(
        payload
            .get(offset + 16..ids_end)?
            .chunks_exact(4)
            .map(|bytes| u32::from_le_bytes(bytes.try_into().expect("four-byte chunk")))
            .collect(),
    )
}

fn compact_general_curve_ref_at(payload: &[u8], offset: usize) -> bool {
    payload.get(offset + 2..offset + 4) == Some(&[0; 2])
        && payload.get(offset + 6..offset + 16) == Some(&[0x2b, 0x80, 0x02, 0, 0, 0, 0, 0, 0, 0])
}

fn compact_profile_general_curve_ref_at(payload: &[u8], offset: usize) -> bool {
    payload.get(offset..offset + 6) == Some(&[1, 0, 0xdd, 0x94, 0xdf, 0x94])
        && payload.get(offset + 6..offset + 16) == Some(&[0x2b, 0x80, 0x02, 0, 0, 0, 0, 0, 0, 0])
}

fn declared_general_curve_profile_prefix(payload: &[u8], offset: usize) -> Option<usize> {
    const COMPONENT_PROFILE: &[u8] = b"moCompProfile_c";
    let interval = payload.get(offset..offset.checked_add(96)?.min(payload.len()))?;
    let name = interval
        .windows(COMPONENT_PROFILE.len())
        .position(|bytes| bytes == COMPONENT_PROFILE)?;
    let prefix = offset.checked_add(name + COMPONENT_PROFILE.len())?;
    (payload.get(prefix..prefix + 10) == Some(&[0x2b, 0x80, 0x02, 0, 0, 0, 0, 0, 0, 0]))
        .then_some(prefix)
}

fn component_profile_source_at(payload: &[u8], prefix: usize) -> Option<u32> {
    const PREFIX: &[u8] = &[0x2b, 0x80, 0x02, 0, 0, 0, 0, 0, 0, 0];
    const HANDLE: &[u8] = &[0xc7, 0xcf, 0xff, 0xff];
    const RECORD_END: &[u8] = &[0xf8, 0x2a, 0, 0];
    if payload.get(prefix..prefix + PREFIX.len()) != Some(PREFIX)
        || payload.get(prefix + 45..prefix + 61) != Some(&[0xff; 16])
    {
        return None;
    }
    let mut sources = [prefix + 69, prefix + 81].into_iter().filter_map(|source| {
        let id = u32::from_le_bytes(payload.get(source..source + 4)?.try_into().ok()?);
        let stamp = u32::from_le_bytes(payload.get(source + 4..source + 8)?.try_into().ok()?);
        if id == 0 || stamp == 0 {
            return None;
        }
        let older = payload.get(source + 12..source + 16) == Some(&[0; 4])
            && payload.get(source + 20..source + 32) == Some(&[0; 12])
            && payload.get(source + 32..source + 36) == Some(HANDLE)
            && payload.get(source + 36..source + 40) == Some(HANDLE)
            && payload.get(source + 40..source + 44) == Some(&[0; 4])
            && payload.get(source + 44..source + 48) == Some(RECORD_END);
        let newer = payload.get(source + 8..source + 16) == Some(&[0; 8])
            && payload.get(source + 16..source + 20) == Some(&0x65u32.to_le_bytes())
            && payload.get(source + 20..source + 24) == Some(&[0; 4])
            && payload.get(source + 24..source + 28) == Some(&[0xff; 4])
            && payload.get(source + 28..source + 32) == Some(&[0; 4])
            && payload.get(source + 32..source + 36) == Some(HANDLE)
            && payload.get(source + 36..source + 40) == Some(HANDLE)
            && payload.get(source + 40..source + 44) == Some(HANDLE)
            && payload.get(source + 44..source + 48) == Some(&[0; 4])
            && payload.get(source + 48..source + 52) == Some(RECORD_END);
        (older || newer).then_some(id)
    });
    let source = sources.next()?;
    sources.next().is_none().then_some(source)
}

fn component_reference_curve_path_at(
    payload: &[u8],
    marker: usize,
) -> Option<Vec<FeatureInputComponentPathEntry>> {
    if payload.get(marker..marker + 16)? != COMPACT_EDGE_VECTOR_MARKER
        || payload.get(marker - 8..marker - 4)? != [0x04, 0x02, 0, 0]
        || payload.get(marker + 16..marker + 18)? != [0, 0]
    {
        return None;
    }
    let count = usize::try_from(u32::from_le_bytes(
        payload.get(marker - 12..marker - 8)?.try_into().ok()?,
    ))
    .ok()
    .filter(|count| (1..=64).contains(count))?;
    let parse = |count: usize| {
        let mut cursor = marker + 18;
        let signature: [u8; 12] = payload.get(cursor + 4..cursor + 16)?.try_into().ok()?;
        let mut components = Vec::with_capacity(count);
        for index in 0..count {
            if payload.get(cursor + 4..cursor + 16) != Some(signature.as_slice()) {
                return None;
            }
            components.push(FeatureInputComponentPathEntry {
                instance: u16::from_le_bytes(payload.get(cursor..cursor + 2)?.try_into().ok()?),
                type_signature: signature,
                local_id: u32::from_le_bytes(
                    payload.get(cursor + 16..cursor + 20)?.try_into().ok()?,
                ),
            });
            cursor += 20;
            if index + 1 != count {
                let gaps = [0usize, 6]
                    .into_iter()
                    .filter(|gap| {
                        payload.get(cursor + gap + 4..cursor + gap + 16)
                            == Some(signature.as_slice())
                            && match *gap {
                                0 => true,
                                6 => {
                                    payload.get(cursor..cursor + 2) != Some(&[0, 0])
                                        && payload.get(cursor + 2..cursor + 6) == Some(&[0; 4])
                                }
                                _ => false,
                            }
                    })
                    .collect::<Vec<_>>();
                let [gap] = gaps.as_slice() else {
                    return None;
                };
                cursor += gap;
            }
        }
        Some((components, cursor))
    };
    parse(count).map(|(components, _)| components).or_else(|| {
        let (components, end) = (count > 1).then(|| parse(count - 1)).flatten()?;
        (payload.get(end..end + 12) == Some(&[0, 0, 0, 0, 0, 0, 0, 0, 0xf8, 0x2a, 0, 0]))
            .then_some(components)
    })
}

fn unique_marker_candidate(candidates: &[(String, bool)]) -> Option<&str> {
    let mut coordinate = candidates
        .iter()
        .filter(|(_, coordinate)| *coordinate)
        .map(|(id, _)| id.as_str());
    if let Some(first) = coordinate.next() {
        return coordinate.next().is_none().then_some(first);
    }
    let [(id, _)] = candidates else {
        return None;
    };
    Some(id)
}

pub(crate) fn operand_accepts_marker(
    kind: FeatureInputOperandKind,
    marker: SketchInputKind,
) -> bool {
    match kind {
        FeatureInputOperandKind::D6
        | FeatureInputOperandKind::Native(0x80cc | 0x8ab6 | 0x8dcb | 0x929d | 0xbc7c | 0xbd69) => {
            matches!(
                marker,
                SketchInputKind::Point | SketchInputKind::ConstrainedPoint
            )
        }
        FeatureInputOperandKind::Native(0x837b) => matches!(
            marker,
            SketchInputKind::Point
                | SketchInputKind::ConstrainedPoint
                | SketchInputKind::LineOrCircle
                | SketchInputKind::Arc
        ),
        FeatureInputOperandKind::E1
        | FeatureInputOperandKind::Native(0x8386 | 0x83fe | 0x8dda | 0xbc87) => {
            matches!(marker, SketchInputKind::LineOrCircle | SketchInputKind::Arc)
        }
        FeatureInputOperandKind::Native(_) => true,
    }
}

fn operand_uses_compatible_ordinal(kind: FeatureInputOperandKind) -> bool {
    matches!(
        kind,
        FeatureInputOperandKind::D6
            | FeatureInputOperandKind::E1
            | FeatureInputOperandKind::Native(0x80cc | 0x83fe | 0x8ab6 | 0x929d | 0xbd69)
    )
}

fn operand_allows_compatible_ordinal_fallback(kind: FeatureInputOperandKind) -> bool {
    matches!(
        kind,
        FeatureInputOperandKind::Native(0x837b | 0x8386 | 0x8dcb | 0x8dda | 0xbc7c | 0xbc87)
    )
}

fn marker_local_links(payload: &[u8], offset: usize) -> Option<([u16; 2], u16)> {
    let coordinate_layout = payload.get(offset + 5..offset + 17)?
        == [
            0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0x00, 0x00, 0x80, 0xbf,
        ]
        && payload.get(offset + 64..offset + 66)? == [0x1e, 0x00];
    if coordinate_layout
        || payload.get(offset + 70..offset + 72)? != [0, 0]
        || payload.get(offset + 72..offset + 80)? != (-1.0f64).to_le_bytes()
    {
        return None;
    }
    Some((
        [
            u16::from_le_bytes(payload.get(offset + 64..offset + 66)?.try_into().ok()?),
            u16::from_le_bytes(payload.get(offset + 66..offset + 68)?.try_into().ok()?),
        ],
        u16::from_le_bytes(payload.get(offset + 68..offset + 70)?.try_into().ok()?),
    ))
}

fn coordinate_marker_local_links(payload: &[u8], offset: usize) -> Option<(Vec<u16>, u16)> {
    marker_coordinates(payload, offset)?;
    let count = usize::from(u16::from_le_bytes(
        payload.get(offset + 84..offset + 86)?.try_into().ok()?,
    ));
    if !(1..=2).contains(&count) {
        return None;
    }
    let mut links = Vec::with_capacity(count);
    let mut selector = None;
    for index in 0..count {
        let start = offset.checked_add(86 + index * 12)?;
        let cell = payload.get(start..start + 12)?;
        let tag = u16::from_le_bytes([cell[0], cell[1]]);
        let kind = operand_kind([cell[0], cell[1]])?;
        if !operand_accepts_marker(kind, SketchInputKind::LineOrCircle)
            || !operand_accepts_marker(kind, SketchInputKind::Arc)
            || selector.is_some_and(|selector| selector != tag)
            || cell[4..8] != [0xff; 4]
            || cell[8..12] != [0; 4]
        {
            return None;
        }
        selector = Some(tag);
        links.push(u16::from_le_bytes([cell[2], cell[3]]));
    }
    let sentinel = offset.checked_add(86 + count * 12)?;
    (payload.get(sentinel..sentinel + 6)? == [0, 0, 0xfe, 0xff, 0xff, 0xff])
        .then_some((links, selector?))
}

fn relation_instances(
    histories: &[crate::records::FeatureHistory],
    lane: &FeatureInputLane,
) -> Vec<FeatureInputRelationInstance> {
    let sketch_features = histories
        .iter()
        .flat_map(|history| &history.features)
        .filter(|feature| feature.xml_tag.eq_ignore_ascii_case("Sketch"))
        .map(|feature| feature.id.as_str())
        .collect::<HashSet<_>>();
    let declarations = lane
        .classes
        .iter()
        .filter_map(|class| {
            relation_family(&class.name).map(|family| (class.offset, family, class.id.as_str()))
        })
        .collect::<Vec<_>>();
    let mut groups = Vec::<(
        String,
        FeatureInputRelationFamily,
        String,
        Vec<FeatureInputOperand>,
        Vec<&FeatureInputScalar>,
        usize,
    )>::new();
    for (scalar_index, scalar) in lane.scalars.iter().enumerate() {
        let Some(feature_ref) = scalar
            .feature_ref
            .as_deref()
            .filter(|feature| sketch_features.contains(feature))
        else {
            continue;
        };
        let Some((_, family, class_ref)) = declarations
            .iter()
            .filter(|(offset, family, _)| {
                *offset < scalar.offset && relation_signature(*family, &scalar.operands)
            })
            .max_by_key(|(offset, _, _)| offset)
        else {
            continue;
        };
        let append = groups.last().is_some_and(
            |(owner, candidate, group_class, operands, scalars, last_index)| {
                owner == feature_ref
                    && candidate == family
                    && group_class == class_ref
                    && *last_index + 1 == scalar_index
                    && scalars.len() == 1
                    && operands
                        .iter()
                        .map(|operand| (operand.kind, operand.entity_index))
                        .eq(scalar
                            .operands
                            .iter()
                            .map(|operand| (operand.kind, operand.entity_index)))
            },
        );
        if append {
            let (_, _, _, _, scalars, last_index) = groups
                .last_mut()
                .expect("append requires an existing relation group");
            scalars.push(scalar);
            *last_index = scalar_index;
        } else {
            groups.push((
                feature_ref.to_string(),
                *family,
                (*class_ref).to_string(),
                scalar.operands.clone(),
                vec![scalar],
                scalar_index,
            ));
        }
    }
    let mut instances = groups
        .into_iter()
        .enumerate()
        .map(
            |(ordinal, (feature_ref, family, class_ref, operands, scalars, _))| {
                let driving = scalars
                    .iter()
                    .filter(|scalar| scalar.role == FeatureInputScalarRole::Driving)
                    .copied()
                    .collect::<Vec<_>>();
                let display = scalars
                    .iter()
                    .filter(|scalar| scalar.role == FeatureInputScalarRole::Display)
                    .copied()
                    .collect::<Vec<_>>();
                let offset = scalars[0].offset;
                FeatureInputRelationInstance {
                    id: format!(
                        "sldprt:feature-input:relation-instance#{}:{offset}",
                        lane.id
                            .rsplit_once('#')
                            .map_or(lane.id.as_str(), |(_, key)| key)
                    ),
                    parent: lane.id.clone(),
                    ordinal: ordinal as u32,
                    offset,
                    family,
                    class_ref,
                    feature_ref,
                    scalar_refs: scalars.iter().map(|scalar| scalar.id.clone()).collect(),
                    parameter_scalar_ref: (driving.len() == 1).then(|| driving[0].id.clone()),
                    display_scalar_ref: (display.len() == 1).then(|| display[0].id.clone()),
                    operands,
                }
            },
        )
        .collect::<Vec<_>>();
    bind_detached_relation_drivers(&mut instances, lane);
    bind_circle_dimension_centers(&mut instances, lane);
    instances
}

fn bind_circle_dimension_centers(
    relations: &mut [FeatureInputRelationInstance],
    lane: &FeatureInputLane,
) {
    let scalars = lane
        .scalars
        .iter()
        .map(|scalar| (scalar.id.as_str(), scalar))
        .collect::<HashMap<_, _>>();
    let names = lane
        .names
        .iter()
        .map(|name| (name.id.as_str(), name.value.as_str()))
        .collect::<HashMap<_, _>>();
    for relation in relations.iter_mut().filter(|relation| {
        relation.family == FeatureInputRelationFamily::CircleDiameter
            && relation.operands.len() == 1
    }) {
        let Some(display) = relation
            .display_scalar_ref
            .as_deref()
            .and_then(|id| scalars.get(id).copied())
        else {
            continue;
        };
        let Some(display_name) = names.get(display.name.as_str()) else {
            continue;
        };
        let Some(display_index) = lane
            .scalars
            .iter()
            .position(|scalar| scalar.id == display.id)
        else {
            continue;
        };
        let first = &relation.operands[0];
        let candidates = lane
            .scalars
            .iter()
            .enumerate()
            .filter(|scalar| {
                let scalar = scalar.1;
                scalar.feature_ref == display.feature_ref
                    && names.get(scalar.name.as_str()) == Some(display_name)
                    && matches!(scalar.operands.as_slice(), [candidate, _]
                        if candidate.kind == first.kind
                            && candidate.entity_index == first.entity_index)
            })
            .collect::<Vec<_>>();
        if candidates.first().map(|candidate| candidate.0) != Some(display_index + 1)
            || candidates.windows(2).any(|pair| pair[1].0 != pair[0].0 + 1)
        {
            continue;
        }
        let centers = candidates
            .iter()
            .filter_map(|(_, scalar)| scalar.operands.get(1))
            .map(|operand| {
                (
                    operand.kind,
                    operand.entity_index,
                    operand.entity_ref.clone(),
                )
            })
            .collect::<Vec<_>>();
        let Some(center) = centers.first() else {
            continue;
        };
        if centers.iter().any(|candidate| candidate != center) {
            continue;
        }
        let Some((_, source)) = candidates.iter().find(|(_, scalar)| {
            scalar.operands.get(1).is_some_and(|operand| {
                (
                    operand.kind,
                    operand.entity_index,
                    operand.entity_ref.clone(),
                ) == *center
            })
        }) else {
            continue;
        };
        relation.operands = source.operands.clone();
        for (_, scalar) in candidates {
            if !relation.scalar_refs.contains(&scalar.id) {
                relation.scalar_refs.push(scalar.id.clone());
            }
        }
    }
}

fn bind_detached_relation_drivers(
    relations: &mut [FeatureInputRelationInstance],
    lane: &FeatureInputLane,
) {
    let scalars = lane
        .scalars
        .iter()
        .map(|scalar| (scalar.id.as_str(), scalar))
        .collect::<HashMap<_, _>>();
    let names = lane
        .names
        .iter()
        .map(|name| (name.id.as_str(), name.value.as_str()))
        .collect::<HashMap<_, _>>();
    let claimed = relations
        .iter()
        .flat_map(|relation| &relation.scalar_refs)
        .map(String::as_str)
        .collect::<HashSet<_>>();
    let mut drivers = HashMap::<(String, String), Vec<&FeatureInputScalar>>::new();
    for scalar in lane.scalars.iter().filter(|scalar| {
        scalar.role == FeatureInputScalarRole::Driving
            && scalar.operands.is_empty()
            && !claimed.contains(scalar.id.as_str())
    }) {
        let (Some(feature), Some(name)) = (
            scalar.feature_ref.as_deref(),
            names.get(scalar.name.as_str()).copied(),
        ) else {
            continue;
        };
        drivers
            .entry((feature.to_string(), name.to_string()))
            .or_default()
            .push(scalar);
    }
    let mut candidates = HashMap::<(String, String), Vec<usize>>::new();
    for (index, relation) in relations.iter().enumerate() {
        if relation.parameter_scalar_ref.is_some() {
            continue;
        }
        let relation_names = relation
            .scalar_refs
            .iter()
            .filter_map(|id| scalars.get(id.as_str()))
            .filter(|scalar| scalar.role == FeatureInputScalarRole::Display)
            .filter_map(|scalar| names.get(scalar.name.as_str()).copied())
            .collect::<HashSet<_>>();
        if relation_names.len() != 1 {
            continue;
        }
        let name = *relation_names
            .iter()
            .next()
            .expect("one display scalar name");
        candidates
            .entry((relation.feature_ref.clone(), name.to_string()))
            .or_default()
            .push(index);
    }
    for (key, relation_indices) in candidates {
        let [relation_index] = relation_indices.as_slice() else {
            continue;
        };
        let Some([driver]) = drivers.get(&key).map(Vec::as_slice) else {
            continue;
        };
        let relation = &mut relations[*relation_index];
        relation.scalar_refs.push(driver.id.clone());
        relation.parameter_scalar_ref = Some(driver.id.clone());
    }
}

fn relation_family(name: &str) -> Option<FeatureInputRelationFamily> {
    match native_object_class(name).kind {
        NativeClassKind::SketchRelation(family) => Some(family),
        _ => None,
    }
}

fn relation_signature(
    family: FeatureInputRelationFamily,
    operands: &[FeatureInputOperand],
) -> bool {
    use FeatureInputOperandKind::{Native, D6, E1};
    use FeatureInputRelationFamily::{
        Angle, CircleDiameter, LineLineDistance, PointLineDistance, PointPointDistance,
        PointPointHorizontalDistance, PointPointVerticalDistance,
    };
    if family == CircleDiameter {
        return matches!(
            operands,
            [operand]
                if matches!(operand.kind, Native(0x80cc | 0x83fe | 0x8ab6 | 0x929d | 0xbd69))
        );
    }
    let [first, second] = operands else {
        return false;
    };
    match family {
        PointPointDistance => {
            (first.kind == D6 && second.kind == D6)
                || (first.kind == Native(0x837b) && second.kind == Native(0x837b))
                || (first.kind == Native(0xbc7c) && second.kind == Native(0xbc7c))
        }
        LineLineDistance => {
            (first.kind == E1 && second.kind == E1)
                || (first.kind == Native(0x8386) && second.kind == Native(0x8386))
                || (first.kind == Native(0xbc87) && second.kind == Native(0xbc87))
        }
        PointLineDistance => {
            (first.kind == D6 && second.kind == E1)
                || (first.kind == Native(0x837b) && second.kind == Native(0x8386))
                || (first.kind == Native(0xbc7c) && second.kind == Native(0xbc87))
        }
        PointPointHorizontalDistance | PointPointVerticalDistance => {
            first.kind == Native(0x8dcb) && second.kind == Native(0x8dcb)
        }
        Angle => first.kind == Native(0x8dda) && second.kind == Native(0x8dda),
        CircleDiameter => unreachable!("handled as a unary relation"),
    }
}

fn scalar_role(payload: &[u8], trailer_offset: usize) -> FeatureInputScalarRole {
    let fixed_layout = payload.get(trailer_offset..trailer_offset + 3) == Some(&[0, 0, 0])
        && payload
            .get(trailer_offset + 7..trailer_offset + 21)
            .is_some_and(|bytes| bytes.iter().all(|byte| *byte == 0))
        && payload.get(trailer_offset + 24..trailer_offset + 29) == Some(&[0, 0, 0, 2, 0]);
    let role_offset = if fixed_layout {
        trailer_offset + 29
    } else if legacy_scalar_layout(payload, trailer_offset) {
        trailer_offset + 30
    } else {
        return FeatureInputScalarRole::Native;
    };
    match payload.get(role_offset) {
        Some(0) => FeatureInputScalarRole::Driving,
        Some(1) => FeatureInputScalarRole::Display,
        _ => FeatureInputScalarRole::Native,
    }
}

fn legacy_scalar_layout(payload: &[u8], trailer_offset: usize) -> bool {
    payload.get(trailer_offset..trailer_offset + 3) == Some(&[0, 0, 0])
        && payload
            .get(trailer_offset + 7..trailer_offset + 24)
            .is_some_and(|bytes| bytes.iter().all(|byte| *byte == 0))
        && payload.get(trailer_offset + 24..trailer_offset + 30) == Some(&[0x0f, 0, 0, 0, 2, 0])
}

/// Add unambiguous `ResolvedFeatures` length parameters to a projection copy of history.
pub(crate) fn enrich_history_parameters<'a>(
    histories: &mut [crate::records::FeatureHistory],
    lanes: impl IntoIterator<Item = &'a FeatureInputLane>,
    replace_existing: bool,
) {
    #[derive(Clone, Copy, PartialEq, Eq)]
    enum ScalarUnit {
        Native,
        Length,
        Angle,
    }
    let mut candidates = BTreeMap::<(usize, usize, String), Vec<(f64, ScalarUnit)>>::new();
    for lane in lanes {
        let relation_unit = |family| match family {
            FeatureInputRelationFamily::Angle => ScalarUnit::Angle,
            FeatureInputRelationFamily::LineLineDistance
            | FeatureInputRelationFamily::PointPointDistance
            | FeatureInputRelationFamily::PointLineDistance
            | FeatureInputRelationFamily::PointPointHorizontalDistance
            | FeatureInputRelationFamily::PointPointVerticalDistance
            | FeatureInputRelationFamily::CircleDiameter => ScalarUnit::Length,
        };
        let mut scalar_units = lane
            .relation_bindings
            .iter()
            .map(|binding| (binding.scalar_ref.as_str(), relation_unit(binding.family)))
            .collect::<HashMap<_, _>>();
        for relation in &lane.relation_instances {
            let unit = relation_unit(relation.family);
            for scalar in &relation.scalar_refs {
                scalar_units.insert(scalar.as_str(), unit);
            }
        }
        let names_by_id = lane
            .names
            .iter()
            .map(|name| (name.id.as_str(), name))
            .collect::<HashMap<_, _>>();
        let mut starts = Vec::<(u64, usize, usize)>::new();
        for (history_index, history) in histories.iter().enumerate() {
            for (feature_index, feature) in history.features.iter().enumerate() {
                let Some(name) = feature_object_name(feature, lane) else {
                    continue;
                };
                starts.push((name.offset, history_index, feature_index));
            }
        }
        starts.sort_by_key(|start| start.0);
        for (index, &(start, history_index, feature_index)) in starts.iter().enumerate() {
            let end = starts.get(index + 1).map_or(u64::MAX, |next| next.0);
            let mut owned = BTreeMap::<&str, Vec<&FeatureInputScalar>>::new();
            for scalar in lane
                .scalars
                .iter()
                .filter(|scalar| scalar.offset > start && scalar.offset < end)
            {
                let Some(name) = names_by_id.get(scalar.name.as_str()) else {
                    continue;
                };
                owned.entry(&name.value).or_default().push(scalar);
            }
            for (name, scalars) in owned {
                let driving = scalars
                    .iter()
                    .filter(|scalar| scalar.role == FeatureInputScalarRole::Driving)
                    .copied()
                    .collect::<Vec<_>>();
                let candidates_for_name = if driving.is_empty() {
                    scalars
                        .into_iter()
                        .filter(|scalar| scalar.role == FeatureInputScalarRole::Native)
                        .collect::<Vec<_>>()
                } else {
                    driving
                };
                if let [scalar] = candidates_for_name.as_slice() {
                    candidates
                        .entry((history_index, feature_index, name.to_string()))
                        .or_default()
                        .push((
                            scalar.value,
                            scalar_units
                                .get(scalar.id.as_str())
                                .copied()
                                .unwrap_or(ScalarUnit::Native),
                        ));
                }
            }
        }
    }

    for ((history_index, feature_index, name), values) in candidates {
        let Some((&(first, unit), rest)) = values.split_first() else {
            continue;
        };
        if rest.iter().any(|(value, candidate_unit)| {
            value.to_bits() != first.to_bits() || *candidate_unit != unit
        }) {
            continue;
        }
        let feature = &mut histories[history_index].features[feature_index];
        if unit == ScalarUnit::Native
            && feature.parameters.get(&name).is_some_and(|expression| {
                !native_scalar_matches_discrete_parameter(feature, &name, expression, first)
            })
        {
            continue;
        }
        let expression = match unit {
            ScalarUnit::Native => crate::history::format_native_scalar(
                feature,
                &name,
                first,
                feature.parameters.get(&name).map(String::as_str),
            ),
            ScalarUnit::Length => crate::history::format_length_mm(first * 1000.0),
            ScalarUnit::Angle => crate::history::format_angle_rad(first),
        };
        if replace_existing {
            feature.parameters.insert(name, expression);
        } else {
            feature.parameters.entry(name).or_insert(expression);
        }
    }
}

fn native_scalar_matches_discrete_parameter(
    feature: &crate::records::Feature,
    name: &str,
    expression: &str,
    value: f64,
) -> bool {
    match crate::history::parse_native_parameter_literal(feature, name, expression) {
        Some(cadmpeg_ir::features::ParameterValue::Integer(expected)) => {
            crate::history::exact_integer_f64(expected) == Some(value)
        }
        Some(cadmpeg_ir::features::ParameterValue::Boolean(expected)) => {
            value == if expected { 1.0 } else { 0.0 }
        }
        _ => true,
    }
}

pub(crate) fn sync_changed_feature_scalars(
    histories: &[crate::records::FeatureHistory],
    lanes: &mut [FeatureInputLane],
    changed: &HashSet<(String, String)>,
) -> Result<(), cadmpeg_ir::codec::CodecError> {
    use cadmpeg_ir::features::ParameterValue;

    for lane in lanes {
        let names_by_id = lane
            .names
            .iter()
            .map(|name| (name.id.as_str(), name.value.as_str()))
            .collect::<HashMap<_, _>>();
        let mut starts = histories
            .iter()
            .flat_map(|history| &history.features)
            .filter_map(|feature| {
                feature_object_name(feature, lane).map(|name| (name.offset, feature))
            })
            .collect::<Vec<_>>();
        starts.sort_by_key(|(offset, _)| *offset);
        let mut updates = Vec::<(usize, f64)>::new();
        for (index, &(start, feature)) in starts.iter().enumerate() {
            let end = starts
                .get(index + 1)
                .map_or(u64::MAX, |(offset, _)| *offset);
            for (name, expression) in &feature.parameters {
                if !changed.contains(&(feature.id.clone(), name.clone())) {
                    continue;
                }
                let candidates = lane
                    .scalars
                    .iter()
                    .enumerate()
                    .filter(|(_, scalar)| scalar.offset > start && scalar.offset < end)
                    .filter(|(_, scalar)| {
                        names_by_id.get(scalar.name.as_str()) == Some(&name.as_str())
                    })
                    .collect::<Vec<_>>();
                let driving = candidates
                    .iter()
                    .filter(|(_, scalar)| scalar.role == FeatureInputScalarRole::Driving)
                    .copied()
                    .collect::<Vec<_>>();
                let candidates = if driving.is_empty() {
                    candidates
                        .into_iter()
                        .filter(|(_, scalar)| scalar.role == FeatureInputScalarRole::Native)
                        .collect::<Vec<_>>()
                } else {
                    driving
                };
                let [(scalar_index, _)] = candidates.as_slice() else {
                    continue;
                };
                let value =
                    match crate::history::parse_native_parameter_literal(feature, name, expression)
                    {
                        Some(ParameterValue::Length(value)) => value.0 / 1000.0,
                        Some(ParameterValue::Angle(value)) => value.0,
                        Some(ParameterValue::Real(value)) => value,
                        _ => continue,
                    };
                updates.push((*scalar_index, value));
            }
        }
        for (scalar_index, value) in updates {
            let scalar = &mut lane.scalars[scalar_index];
            let offset = usize::try_from(scalar.offset).map_err(|_| {
                cadmpeg_ir::codec::CodecError::Malformed(
                    "SLDPRT scalar offset exceeds address space".into(),
                )
            })?;
            let bytes = lane
                .native_payload
                .get_mut(offset..offset + 8)
                .ok_or_else(|| {
                    cadmpeg_ir::codec::CodecError::Malformed(format!(
                        "SLDPRT scalar {} lies outside its payload",
                        scalar.id
                    ))
                })?;
            bytes.copy_from_slice(&value.to_le_bytes());
            scalar.value = value;
        }
    }
    Ok(())
}

/// Add exact fixed-reference-plane frames to a projection copy of history.
pub(crate) fn enrich_history_reference_planes(
    histories: &mut [crate::records::FeatureHistory],
    lanes: &[FeatureInputLane],
) {
    let mut candidates = BTreeMap::<(usize, usize), Vec<(Point3, Vector3, Vector3)>>::new();
    for lane in lanes {
        let mut starts =
            histories
                .iter()
                .enumerate()
                .flat_map(|(history_index, history)| {
                    history.features.iter().enumerate().filter_map(
                        move |(feature_index, feature)| {
                            feature_object_name(feature, lane)
                                .map(|name| (name.offset, history_index, feature_index))
                        },
                    )
                })
                .collect::<Vec<_>>();
        starts.sort_by_key(|start| start.0);
        for (index, &(start, history_index, feature_index)) in starts.iter().enumerate() {
            let feature = &histories[history_index].features[feature_index];
            if native_object_class(feature.input_class.as_deref().unwrap_or_default()).kind
                != NativeClassKind::ReferencePlane
                || feature.properties.contains_key("Origin")
                || feature.properties.contains_key("Normal")
                || feature.properties.contains_key("UAxis")
            {
                continue;
            }
            let end = starts
                .get(index + 1)
                .map_or(lane.native_payload.len(), |next| next.0 as usize);
            let Ok(start) = usize::try_from(start) else {
                continue;
            };
            let Some(bytes) = lane.native_payload.get(start..end) else {
                continue;
            };
            let mut frames = bytes
                .windows(FIXED_REFERENCE_PLANE_FRAME_LEN)
                .filter_map(fixed_reference_plane_frame)
                .collect::<Vec<_>>();
            frames.sort_by_key(|(origin, normal, u_axis)| {
                [
                    origin.x.to_bits(),
                    origin.y.to_bits(),
                    origin.z.to_bits(),
                    normal.x.to_bits(),
                    normal.y.to_bits(),
                    normal.z.to_bits(),
                    u_axis.x.to_bits(),
                    u_axis.y.to_bits(),
                    u_axis.z.to_bits(),
                ]
            });
            frames.dedup_by(|left, right| left == right);
            let [(origin, normal, u_axis)] = frames.as_slice() else {
                continue;
            };
            candidates
                .entry((history_index, feature_index))
                .or_default()
                .push((*origin, *normal, *u_axis));
        }
    }
    for ((history_index, feature_index), mut frames) in candidates {
        frames.sort_by_key(|(origin, normal, u_axis)| {
            [
                origin.x.to_bits(),
                origin.y.to_bits(),
                origin.z.to_bits(),
                normal.x.to_bits(),
                normal.y.to_bits(),
                normal.z.to_bits(),
                u_axis.x.to_bits(),
                u_axis.y.to_bits(),
                u_axis.z.to_bits(),
            ]
        });
        frames.dedup();
        let [(origin, normal, u_axis)] = frames.as_slice() else {
            continue;
        };
        let feature = &mut histories[history_index].features[feature_index];
        feature.properties.insert(
            "Origin".into(),
            format!("{}mm,{}mm,{}mm", origin.x, origin.y, origin.z),
        );
        feature.properties.insert(
            "Normal".into(),
            format!("{},{},{}", normal.x, normal.y, normal.z),
        );
        feature.properties.insert(
            "UAxis".into(),
            format!("{},{},{}", u_axis.x, u_axis.y, u_axis.z),
        );
    }
}

const FIXED_REFERENCE_PLANE_FRAME_LEN: usize = 97;

fn fixed_reference_plane_frame(bytes: &[u8]) -> Option<(Point3, Vector3, Vector3)> {
    const NATIVE_TO_IR: f64 = 1000.0;
    if bytes.get(..8)?.iter().any(|byte| *byte != 0) || bytes.get(48) != Some(&1) {
        return None;
    }
    let scalar = |offset| {
        let value = f64::from_le_bytes(bytes.get(offset..offset + 8)?.try_into().ok()?);
        value.is_finite().then_some(value)
    };
    let native_origin = [scalar(8)?, scalar(16)?, scalar(24)?];
    let origin = Point3::new(
        native_origin[2] * NATIVE_TO_IR,
        native_origin[0] * NATIVE_TO_IR,
        native_origin[1] * NATIVE_TO_IR,
    );
    let normal = Vector3::new(0.0, scalar(32)?, scalar(40)?);
    let u_axis = Vector3::new(scalar(73)?, scalar(81)?, scalar(89)?);
    let norm =
        |vector: Vector3| (vector.x * vector.x + vector.y * vector.y + vector.z * vector.z).sqrt();
    let normal_norm = norm(normal);
    let u_norm = norm(u_axis);
    let dot = normal.x * u_axis.x + normal.y * u_axis.y + normal.z * u_axis.z;
    ((normal_norm - 1.0).abs() <= 1.0e-9 && (u_norm - 1.0).abs() <= 1.0e-9 && dot.abs() <= 1.0e-9)
        .then_some((origin, normal, u_axis))
}

/// Bind Keywords history records to their serialized feature-input object classes.
pub(crate) fn bind_history_classes(
    histories: &mut [crate::records::FeatureHistory],
    lanes: &[FeatureInputLane],
) {
    for feature in histories
        .iter_mut()
        .flat_map(|history| &mut history.features)
    {
        feature.input_class = None;
    }
    let mut classes_by_object = HashMap::<u32, Vec<&str>>::new();
    for lane in lanes {
        let names_by_offset = lane
            .names
            .iter()
            .map(|name| (name.offset, name))
            .collect::<HashMap<_, _>>();
        for class in &lane.classes {
            let name_offset = class.offset + 6 + class.name.len() as u64;
            let Some(name) = names_by_offset.get(&name_offset) else {
                continue;
            };
            if let Some(object_id) = name.object_id {
                classes_by_object
                    .entry(object_id)
                    .or_default()
                    .push(&class.name);
            }
        }
    }

    for feature in histories
        .iter_mut()
        .flat_map(|history| &mut history.features)
    {
        let classes = feature
            .source_id
            .as_deref()
            .and_then(|value| value.parse::<u32>().ok())
            .and_then(|object_id| classes_by_object.get(&object_id));
        let Some(classes) = classes else {
            continue;
        };
        let Some((&first, rest)) = classes.split_first() else {
            continue;
        };
        if rest.iter().all(|class| *class == first) {
            feature.input_class = Some(first.to_string());
        }
    }

    let mut classes_by_type = HashMap::<String, Vec<String>>::new();
    for feature in histories.iter().flat_map(|history| &history.features) {
        if let Some(class) = &feature.input_class {
            classes_by_type
                .entry(feature.kind.clone())
                .or_default()
                .push(class.clone());
        }
    }
    for classes in classes_by_type.values_mut() {
        classes.sort();
        classes.dedup();
    }
    for feature in histories
        .iter_mut()
        .flat_map(|history| &mut history.features)
        .filter(|feature| feature.input_class.is_none())
    {
        if let Some([class]) = classes_by_type.get(&feature.kind).map(Vec::as_slice) {
            feature.input_class = Some(class.clone());
        }
    }

    let direct_name_offsets = lanes
        .iter()
        .flat_map(|lane| {
            lane.classes
                .iter()
                .map(|class| (lane.id.as_str(), class.offset + 6 + class.name.len() as u64))
        })
        .collect::<HashSet<_>>();
    let mut classes_by_token = HashMap::<(&str, u16), Vec<String>>::new();
    for feature in histories.iter().flat_map(|history| &history.features) {
        let (Some(class), Some(object_id)) = (
            &feature.input_class,
            feature
                .source_id
                .as_deref()
                .and_then(|value| value.parse::<u32>().ok()),
        ) else {
            continue;
        };
        for lane in lanes {
            for name in lane.names.iter().filter(|name| {
                name.object_id == Some(object_id)
                    && !direct_name_offsets.contains(&(lane.id.as_str(), name.offset))
            }) {
                let Ok(offset) = usize::try_from(name.offset) else {
                    continue;
                };
                if let Some(token) = repeated_class_token(&lane.native_payload, offset) {
                    classes_by_token
                        .entry((lane.id.as_str(), token))
                        .or_default()
                        .push(class.clone());
                }
            }
        }
    }
    for classes in classes_by_token.values_mut() {
        classes.sort();
        classes.dedup();
    }
    for feature in histories
        .iter_mut()
        .flat_map(|history| &mut history.features)
        .filter(|feature| feature.input_class.is_none())
    {
        let Some(object_id) = feature
            .source_id
            .as_deref()
            .and_then(|value| value.parse::<u32>().ok())
        else {
            continue;
        };
        let mut candidates = Vec::new();
        for lane in lanes {
            for name in lane.names.iter().filter(|name| {
                name.object_id == Some(object_id)
                    && !direct_name_offsets.contains(&(lane.id.as_str(), name.offset))
            }) {
                let Ok(offset) = usize::try_from(name.offset) else {
                    continue;
                };
                let Some(token) = repeated_class_token(&lane.native_payload, offset) else {
                    continue;
                };
                if let Some([class]) = classes_by_token
                    .get(&(lane.id.as_str(), token))
                    .map(Vec::as_slice)
                {
                    candidates.push(class.clone());
                }
            }
        }
        candidates.sort();
        candidates.dedup();
        if let [class] = candidates.as_slice() {
            feature.input_class = Some(class.clone());
        }
    }
}

fn repeated_class_token(payload: &[u8], name_offset: usize) -> Option<u16> {
    let start = name_offset.checked_sub(2)?;
    Some(u16::from_le_bytes(
        payload.get(start..name_offset)?.try_into().ok()?,
    ))
}

fn feature_operation_code(lane: &FeatureInputLane, name: &FeatureInputName) -> Option<u32> {
    let name_offset = usize::try_from(name.offset).ok()?;
    let direct_class = lane
        .classes
        .iter()
        .find(|class| class.offset + 6 + class.name.len() as u64 == name.offset);
    let code_offset = if let Some(class) = direct_class {
        let class_offset = usize::try_from(class.offset).ok()?;
        [8usize, 4].into_iter().find_map(|padding| {
            let code_offset = class_offset.checked_sub(4 + padding)?;
            lane.native_payload
                .get(code_offset + 4..class_offset)?
                .iter()
                .all(|byte| *byte == 0)
                .then_some(code_offset)
        })?
    } else {
        let compact_instance = name_offset.checked_sub(14).filter(|code_offset| {
            lane.native_payload.get(code_offset + 4..code_offset + 8) == Some(&[0; 4])
                && lane.native_payload.get(name_offset - 2..name_offset) == Some(&[0x00, 0x80])
        });
        compact_instance.or_else(|| {
            [8usize, 4].into_iter().find_map(|padding| {
                let code_offset = name_offset.checked_sub(6 + padding)?;
                lane.native_payload
                    .get(code_offset + 4..name_offset - 2)?
                    .iter()
                    .all(|byte| *byte == 0)
                    .then_some(code_offset)
            })
        })?
    };
    Some(u32::from_le_bytes(
        lane.native_payload
            .get(code_offset..code_offset + 4)?
            .try_into()
            .ok()?,
    ))
}

/// Project compact solid-sweep Boolean operation discriminators.
pub(crate) fn bind_sweep_operations(
    features: &mut [cadmpeg_ir::features::Feature],
    histories: &[crate::records::FeatureHistory],
    lanes: &[FeatureInputLane],
) {
    let history_features = histories
        .iter()
        .flat_map(|history| &history.features)
        .map(|feature| (feature.id.as_str(), feature))
        .collect::<HashMap<_, _>>();
    for feature in features {
        let FeatureDefinition::Sweep {
            mode: cadmpeg_ir::features::SweepMode::Solid { op },
            ..
        } = &mut feature.definition
        else {
            continue;
        };
        if *op != BooleanOp::Unresolved {
            continue;
        }
        let Some(history) = feature
            .native_ref
            .as_deref()
            .and_then(|native| history_features.get(native).copied())
        else {
            continue;
        };
        let mut operations = lanes.iter().filter_map(|lane| {
            let name = feature_object_name(history, lane)?;
            match (
                history.input_class.as_deref(),
                feature_operation_code(lane, name)?,
            ) {
                (Some("moSweep_c"), 15) => Some(BooleanOp::Join),
                _ => None,
            }
        });
        let Some(first) = operations.next() else {
            continue;
        };
        if operations.all(|operation| operation == first) {
            *op = first;
        }
    }
}

/// Inline extrusion trailer fields: the low byte of the family word and the
/// operation byte. The family word is `0x0140` for `moExtrusion_c` objects
/// and `0x01ca` for `moICE_c` objects.
fn feature_inline_operation_fields(
    lane: &FeatureInputLane,
    name: &FeatureInputName,
) -> Option<(u8, u8)> {
    let name_offset = usize::try_from(name.offset).ok()?;
    let name_bytes = name.value.encode_utf16().count().checked_mul(2)?;
    let trailer = name_offset.checked_add(6 + name_bytes)?;
    let bytes = lane.native_payload.get(trailer..trailer + 19)?;
    if bytes[..4] != [0; 4]
        || bytes[5] != 1
        || bytes[8..12] != name.object_id?.to_le_bytes()
        || bytes[12..16] != [0; 4]
        || bytes[16..19] != [0xff, 0xfe, 0xff]
        || !matches!(bytes[6], 0 | 2)
    {
        return None;
    }
    Some((bytes[4], bytes[6]))
}

/// Inline Boolean operation, when the trailer carries one. A zero operation
/// byte on an `moICE_c` object is not an operation carrier; those objects use
/// class-scoped form semantics instead.
fn feature_inline_operation(lane: &FeatureInputLane, name: &FeatureInputName) -> Option<BooleanOp> {
    match feature_inline_operation_fields(lane, name)? {
        (0x40, 0) => Some(BooleanOp::Join),
        (0xca, 2) => Some(BooleanOp::Cut),
        _ => None,
    }
}

/// Project the feature-input operation discriminator onto typed extrusions.
pub(crate) fn bind_extrusion_operations(
    features: &mut [cadmpeg_ir::features::Feature],
    histories: &[crate::records::FeatureHistory],
    lanes: &[FeatureInputLane],
) {
    let history_features = histories
        .iter()
        .flat_map(|history| &history.features)
        .map(|feature| (feature.id.as_str(), feature))
        .collect::<HashMap<_, _>>();
    for feature in features {
        let FeatureDefinition::Extrude { op, .. } = &mut feature.definition else {
            continue;
        };
        if *op != BooleanOp::Unresolved {
            continue;
        }
        let Some(history) = feature
            .native_ref
            .as_deref()
            .and_then(|native| history_features.get(native).copied())
        else {
            continue;
        };
        let mut operations = lanes.iter().filter_map(|lane| {
            let name = feature_object_name(history, lane)?;
            if let Some(operation) = feature_inline_operation(lane, name) {
                return Some(operation);
            }
            match (
                history.input_class.as_deref(),
                feature_operation_code(lane, name)?,
            ) {
                (Some("moExtrusion_c"), 1) | (_, 3) => Some(BooleanOp::Join),
                (Some("moICE_c"), 1 | 2 | 10) => Some(BooleanOp::Cut),
                (_, 11) => Some(BooleanOp::Cut),
                _ => None,
            }
        });
        let Some(first) = operations.next() else {
            continue;
        };
        if operations.all(|operation| operation == first) {
            *op = first;
        }
    }
}

/// Bind profile streams to uniquely enclosing sketch feature records.
pub(crate) fn bind_sketch_profiles(
    features: &mut [cadmpeg_ir::features::Feature],
    sketches: &mut [Sketch],
    sketch_entities: &[SketchEntity],
    parameters: &[cadmpeg_ir::features::DesignParameter],
    histories: &[crate::records::FeatureHistory],
    lanes: &[FeatureInputLane],
    annotations: &Annotations,
) {
    let native_features = histories
        .iter()
        .flat_map(|history| &history.features)
        .map(|feature| (feature.id.as_str(), feature))
        .collect::<HashMap<_, _>>();
    for lane in lanes {
        let mut starts = Vec::<(u64, &crate::records::Feature)>::new();
        for feature in native_features.values() {
            let Some(name) = feature_object_name(feature, lane) else {
                continue;
            };
            starts.push((name.offset, feature));
        }
        starts.sort_by_key(|start| start.0);
        for (index, &(start, native_feature)) in starts.iter().enumerate() {
            let Some(feature) = features
                .iter_mut()
                .find(|feature| feature.native_ref.as_deref() == Some(native_feature.id.as_str()))
            else {
                continue;
            };
            let end = starts.get(index + 1).map_or(u64::MAX, |next| next.0);
            let mut enclosed = sketches.iter_mut().filter(|sketch| {
                sketch.native_ref.as_deref() == Some(lane.id.as_str())
                    && annotations
                        .provenance
                        .get(&sketch.id.0)
                        .is_some_and(|source| source.offset > start && source.offset < end)
            });
            let Some(sketch) = enclosed.next() else {
                continue;
            };
            if enclosed.next().is_some() {
                continue;
            }
            match &mut feature.definition {
                cadmpeg_ir::features::FeatureDefinition::Sketch {
                    sketch: feature_sketch,
                    ..
                } => {
                    sketch.name = Some(native_feature.name.clone());
                    *feature_sketch = Some(sketch.id.clone());
                }
                cadmpeg_ir::features::FeatureDefinition::Sweep { profile, .. }
                    if profile.is_none() =>
                {
                    *profile = Some(cadmpeg_ir::features::ProfileRef::Sketch(sketch.id.clone()));
                }
                cadmpeg_ir::features::FeatureDefinition::Extrude { profile, .. } => {
                    if matches!(
                        &*profile,
                        cadmpeg_ir::features::ProfileRef::Unresolved(owner)
                            if owner == &native_feature.id
                    ) {
                        *profile = cadmpeg_ir::features::ProfileRef::Sketch(sketch.id.clone());
                    }
                }
                _ => {}
            }
        }
    }
    bind_circular_profile_by_dimension(features, sketches, sketch_entities, parameters);
}

/// Materialize a planar line profile carried directly by a compact sketch region.
pub(crate) fn project_compact_sketch_profiles(
    features: &mut [cadmpeg_ir::features::Feature],
    sketches: &mut Vec<Sketch>,
    sketch_entities: &mut Vec<SketchEntity>,
    histories: &[crate::records::FeatureHistory],
    lanes: &[FeatureInputLane],
) {
    const NATIVE_TO_IR: f64 = 1000.0;
    const QUANTUM: f64 = 1.0e-8;

    let native_features = histories
        .iter()
        .flat_map(|history| &history.features)
        .map(|feature| (feature.id.as_str(), feature))
        .collect::<HashMap<_, _>>();
    let plane_frames = histories
        .iter()
        .flat_map(|history| &history.features)
        .filter_map(|feature| {
            let source = feature.source_id.as_deref()?.parse::<u32>().ok()?;
            let neutral = features
                .iter()
                .find(|neutral| neutral.native_ref.as_deref() == Some(feature.id.as_str()))?;
            let frame = match neutral.definition {
                cadmpeg_ir::features::FeatureDefinition::DatumPrincipalPlane { plane } => {
                    principal_sketch_frame(plane)
                }
                cadmpeg_ir::features::FeatureDefinition::DatumPlane {
                    origin,
                    normal,
                    u_axis,
                } => (origin, normal, u_axis),
                _ => return None,
            };
            Some((source, frame))
        })
        .collect::<HashMap<_, _>>();

    for lane in lanes {
        let mut objects = native_features
            .values()
            .filter_map(|feature| Some((feature_object_name(feature, lane)?.offset, *feature)))
            .collect::<Vec<_>>();
        objects.sort_by_key(|(offset, _)| *offset);
        for (object_index, &(start, native_feature)) in objects.iter().enumerate() {
            let Some(feature_index) = features.iter().position(|feature| {
                feature.native_ref.as_deref() == Some(native_feature.id.as_str())
                    && matches!(
                        feature.definition,
                        cadmpeg_ir::features::FeatureDefinition::Sketch {
                            space: cadmpeg_ir::features::SketchSpace::Planar,
                            sketch: None,
                        }
                    )
            }) else {
                continue;
            };
            let end = objects
                .get(object_index + 1)
                .map_or(lane.native_payload.len() as u64, |(offset, _)| *offset);
            let (Ok(start), Ok(end)) = (usize::try_from(start), usize::try_from(end)) else {
                continue;
            };
            let Some(interval) = lane.native_payload.get(start..end) else {
                continue;
            };
            let region_addresses = compact_line_region_addresses(interval);
            let chain_addresses = compact_line_chain_addresses(interval);
            let addresses = region_addresses.as_ref().or(chain_addresses.as_ref());
            let owned_markers = lane
                .sketch_entities
                .iter()
                .filter(|marker| marker.feature_ref.as_deref() == Some(native_feature.id.as_str()))
                .collect::<Vec<_>>();
            let dimensions = lane
                .relation_instances
                .iter()
                .filter(|relation| relation.feature_ref == native_feature.id)
                .filter(|relation| {
                    !matches!(
                        relation.family,
                        FeatureInputRelationFamily::Angle
                            | FeatureInputRelationFamily::CircleDiameter
                    )
                })
                .filter_map(|relation| relation.parameter_scalar_ref.as_deref())
                .filter_map(|scalar| lane.scalars.iter().find(|record| record.id == scalar))
                .map(|scalar| scalar.value * NATIVE_TO_IR)
                .collect::<Vec<_>>();
            let dimensioned_rectangle = addresses
                .is_none()
                .then(|| unique_dimensioned_rectangle_markers(&owned_markers, &dimensions))
                .flatten();
            let markers = if let Some(rectangle) = dimensioned_rectangle {
                rectangle.to_vec()
            } else if region_addresses.is_some() {
                let line_classes = lane
                    .classes
                    .iter()
                    .filter(|class| {
                        class.name == "sgLineHandle"
                            && usize::try_from(class.offset)
                                .is_ok_and(|offset| offset >= start && offset < end)
                    })
                    .collect::<Vec<_>>();
                let [line_class] = line_classes.as_slice() else {
                    continue;
                };
                if lane.classes.iter().any(|class| {
                    class.name == "sgArcHandle"
                        && usize::try_from(class.offset)
                            .is_ok_and(|offset| offset >= start && offset < end)
                }) {
                    continue;
                }
                let Some(first_marker) = owned_markers
                    .iter()
                    .copied()
                    .filter(|marker| marker.offset <= line_class.offset)
                    .max_by_key(|marker| marker.offset)
                else {
                    continue;
                };
                owned_markers
                    .iter()
                    .copied()
                    .skip_while(|marker| marker.offset < first_marker.offset)
                    .take_while(|marker| marker.coordinates_m.is_some())
                    .collect::<Vec<_>>()
            } else {
                let runs = owned_markers
                    .split(|marker| {
                        marker.coordinates_m.is_none()
                            || !matches!(
                                marker.kind,
                                SketchInputKind::Point | SketchInputKind::ConstrainedPoint
                            )
                    })
                    .filter(|run| addresses.is_some_and(|addresses| run.len() == addresses.len()))
                    .collect::<Vec<_>>();
                let [run] = runs.as_slice() else {
                    continue;
                };
                run.to_vec()
            };
            if addresses.is_some_and(|addresses| markers.len() != addresses.len())
                || markers.len() < 3
            {
                continue;
            }
            let context_start = object_index
                .checked_sub(1)
                .and_then(|index| objects.get(index))
                .and_then(|(offset, _)| usize::try_from(*offset).ok())
                .unwrap_or(0);
            let Some(source_id) = compact_profile_reference_plane_source(
                &lane.native_payload,
                context_start,
                start,
                end,
            ) else {
                continue;
            };
            let Some(&(origin, normal, u_axis)) = plane_frames.get(&source_id) else {
                continue;
            };
            let lane_key = lane
                .id
                .rsplit_once('#')
                .map_or(lane.id.as_str(), |(_, key)| key);
            let sketch_id = SketchId(format!(
                "sldprt:model:sketch#compact:{lane_key}:{}",
                native_feature.ordinal
            ));
            let sketch = Sketch {
                id: sketch_id.clone(),
                name: Some(native_feature.name.clone()),
                configuration: lane.configuration.clone(),
                origin,
                normal,
                u_axis,
                profiles: Vec::new(),
                native_ref: Some(lane.id.clone()),
            };
            let Some(transform) = sketch_frame_marker_transform(&sketch, QUANTUM) else {
                continue;
            };
            if dimensioned_rectangle.is_some() {
                let points = markers
                    .iter()
                    .filter_map(|marker| {
                        let [u, v] = marker.coordinates_m?;
                        let native =
                            quantize(Point2::new(u * NATIVE_TO_IR, v * NATIVE_TO_IR), QUANTUM);
                        let point = transform.apply(native)?;
                        Some(Point2::new(
                            point.0 as f64 * QUANTUM,
                            point.1 as f64 * QUANTUM,
                        ))
                    })
                    .collect::<Vec<_>>();
                let Some(corners) = ordered_rectangle_corners(&points) else {
                    continue;
                };
                let Some(corner_markers) = corners
                    .iter()
                    .map(|corner| {
                        points
                            .iter()
                            .position(|point| point == corner)
                            .and_then(|index| markers.get(index).copied())
                    })
                    .collect::<Option<Vec<_>>>()
                else {
                    continue;
                };
                let mut profile = Vec::with_capacity(corners.len());
                for (index, start) in corners.iter().enumerate() {
                    let end = corners[(index + 1) % corners.len()];
                    let start_marker = corner_markers[index];
                    let end_marker = corner_markers[(index + 1) % corner_markers.len()];
                    let entity_id = SketchEntityId(format!(
                        "sldprt:model:sketch-entity#compact:{lane_key}:{}:{index}",
                        native_feature.ordinal
                    ));
                    profile.push(SketchEntityUse {
                        entity: entity_id.clone(),
                        reversed: false,
                    });
                    sketch_entities.push(SketchEntity {
                        id: entity_id,
                        sketch: sketch_id.clone(),
                        construction: false,
                        native_ref: Some(start_marker.id.clone()),
                        geometry_ref: None,
                        endpoint_refs: vec![start_marker.id.clone(), end_marker.id.clone()],
                        geometry: SketchGeometry::Line { start: *start, end },
                    });
                }
                let mut sketch = sketch;
                sketch.profiles.push(profile);
                sketches.push(sketch);
                features[feature_index].definition =
                    cadmpeg_ir::features::FeatureDefinition::Sketch {
                        space: cadmpeg_ir::features::SketchSpace::Planar,
                        sketch: Some(sketch_id),
                    };
                continue;
            }
            if let (Some(curves), Some(vertices)) =
                (region_addresses.as_deref(), chain_addresses.as_deref())
            {
                let project = |marker: &SketchInputEntity| {
                    let [u, v] = marker.coordinates_m?;
                    let native = quantize(Point2::new(u * NATIVE_TO_IR, v * NATIVE_TO_IR), QUANTUM);
                    let point = transform.apply(native)?;
                    Some(Point2::new(
                        point.0 as f64 * QUANTUM,
                        point.1 as f64 * QUANTUM,
                    ))
                };
                let lines = curves
                    .iter()
                    .zip(vertices)
                    .enumerate()
                    .filter_map(|(index, (curve, vertex))| {
                        let curve = markers.get(usize::from(*curve).checked_sub(1)?)?;
                        let vertex = markers.get(usize::from(*vertex).checked_sub(1)?)?;
                        let start = project(curve)?;
                        let end = project(vertex)?;
                        (start != end).then(|| {
                            (
                                SketchEntityId(format!(
                                    "sldprt:model:sketch-entity#compact:{lane_key}:{}:{index}",
                                    native_feature.ordinal
                                )),
                                *curve,
                                *vertex,
                                start,
                                end,
                            )
                        })
                    })
                    .collect::<Vec<_>>();
                let profile = if let Some(profile) =
                    complete_ordered_compact_line_profile(&lines, markers.len())
                {
                    for (entity_id, marker, vertex, start, end) in lines {
                        sketch_entities.push(SketchEntity {
                            id: entity_id,
                            sketch: sketch_id.clone(),
                            construction: false,
                            native_ref: Some(marker.id.clone()),
                            geometry_ref: None,
                            endpoint_refs: vec![marker.id.clone(), vertex.id.clone()],
                            geometry: SketchGeometry::Line { start, end },
                        });
                    }
                    profile
                } else {
                    let Some(points) = markers
                        .iter()
                        .map(|marker| project(marker))
                        .collect::<Option<Vec<_>>>()
                    else {
                        continue;
                    };
                    let Some(corners) = ordered_rectangle_corners(&points) else {
                        continue;
                    };
                    let Some(corner_markers) = corners
                        .iter()
                        .map(|corner| {
                            points
                                .iter()
                                .position(|point| point == corner)
                                .and_then(|index| markers.get(index).copied())
                        })
                        .collect::<Option<Vec<_>>>()
                    else {
                        continue;
                    };
                    let mut profile = Vec::with_capacity(corners.len());
                    for (index, start) in corners.iter().enumerate() {
                        let end = corners[(index + 1) % corners.len()];
                        let start_marker = corner_markers[index];
                        let end_marker = corner_markers[(index + 1) % corner_markers.len()];
                        let entity_id = SketchEntityId(format!(
                            "sldprt:model:sketch-entity#compact:{lane_key}:{}:{index}",
                            native_feature.ordinal
                        ));
                        profile.push(SketchEntityUse {
                            entity: entity_id.clone(),
                            reversed: false,
                        });
                        sketch_entities.push(SketchEntity {
                            id: entity_id,
                            sketch: sketch_id.clone(),
                            construction: false,
                            native_ref: Some(start_marker.id.clone()),
                            geometry_ref: None,
                            endpoint_refs: vec![start_marker.id.clone(), end_marker.id.clone()],
                            geometry: SketchGeometry::Line { start: *start, end },
                        });
                    }
                    profile
                };
                let mut sketch = sketch;
                sketch.profiles.push(profile);
                sketches.push(sketch);
                features[feature_index].definition =
                    cadmpeg_ir::features::FeatureDefinition::Sketch {
                        space: cadmpeg_ir::features::SketchSpace::Planar,
                        sketch: Some(sketch_id),
                    };
                continue;
            }
            let Some(addresses) = addresses else {
                continue;
            };
            let points = addresses
                .iter()
                .filter_map(|address| {
                    let marker = markers.get(usize::from(*address).checked_sub(1)?)?;
                    let [u, v] = marker.coordinates_m?;
                    let native = quantize(Point2::new(u * NATIVE_TO_IR, v * NATIVE_TO_IR), QUANTUM);
                    let point = transform.apply(native)?;
                    Some((
                        *marker,
                        Point2::new(point.0 as f64 * QUANTUM, point.1 as f64 * QUANTUM),
                    ))
                })
                .collect::<Vec<_>>();
            if points.len() != addresses.len()
                || points
                    .iter()
                    .enumerate()
                    .any(|(index, (_, point))| *point == points[(index + 1) % points.len()].1)
            {
                continue;
            }
            let mut profile = Vec::with_capacity(points.len());
            for (index, (marker, start)) in points.iter().enumerate() {
                let end = points[(index + 1) % points.len()].1;
                let entity_id = SketchEntityId(format!(
                    "sldprt:model:sketch-entity#compact:{lane_key}:{}:{index}",
                    native_feature.ordinal
                ));
                profile.push(SketchEntityUse {
                    entity: entity_id.clone(),
                    reversed: false,
                });
                sketch_entities.push(SketchEntity {
                    id: entity_id,
                    sketch: sketch_id.clone(),
                    construction: false,
                    native_ref: Some(marker.id.clone()),
                    geometry_ref: None,
                    endpoint_refs: Vec::new(),
                    geometry: SketchGeometry::Line { start: *start, end },
                });
            }
            let mut sketch = sketch;
            sketch.profiles.push(profile);
            sketches.push(sketch);
            features[feature_index].definition = cadmpeg_ir::features::FeatureDefinition::Sketch {
                space: cadmpeg_ir::features::SketchSpace::Planar,
                sketch: Some(sketch_id),
            };
        }
    }
}

fn ordered_rectangle_corners(points: &[Point2]) -> Option<[Point2; 4]> {
    let [_, _, _, _] = points else {
        return None;
    };
    let mut u = points.iter().map(|point| point.u).collect::<Vec<_>>();
    u.sort_by(f64::total_cmp);
    u.dedup();
    let mut v = points.iter().map(|point| point.v).collect::<Vec<_>>();
    v.sort_by(f64::total_cmp);
    v.dedup();
    let ([u0, u1], [v0, v1]) = (u.as_slice(), v.as_slice()) else {
        return None;
    };
    let corners = [
        Point2::new(*u0, *v0),
        Point2::new(*u1, *v0),
        Point2::new(*u1, *v1),
        Point2::new(*u0, *v1),
    ];
    corners
        .iter()
        .all(|corner| points.iter().filter(|point| *point == corner).count() == 1)
        .then_some(corners)
}

fn unique_dimensioned_rectangle_markers<'a>(
    markers: &[&'a SketchInputEntity],
    dimensions_mm: &[f64],
) -> Option<[&'a SketchInputEntity; 4]> {
    const NATIVE_TO_IR: f64 = 1000.0;
    const QUANTUM: f64 = 1.0e-8;
    if dimensions_mm.len() < 2 {
        return None;
    }
    let points = markers
        .iter()
        .filter_map(|marker| {
            let [u, v] = marker.coordinates_m?;
            Some((
                *marker,
                quantize(Point2::new(u * NATIVE_TO_IR, v * NATIVE_TO_IR), QUANTUM),
            ))
        })
        .collect::<Vec<_>>();
    let mut u = points.iter().map(|(_, point)| point.0).collect::<Vec<_>>();
    u.sort_unstable();
    u.dedup();
    let mut v = points.iter().map(|(_, point)| point.1).collect::<Vec<_>>();
    v.sort_unstable();
    v.dedup();
    let dimensions_match = |u0: i64, u1: i64, v0: i64, v1: i64| {
        let u_span = (u1 - u0) as f64 * QUANTUM;
        let v_span = (v1 - v0) as f64 * QUANTUM;
        dimensions_mm
            .iter()
            .enumerate()
            .any(|(first_index, first)| {
                dimensions_mm
                    .iter()
                    .enumerate()
                    .any(|(second_index, second)| {
                        first_index != second_index
                            && ((same_dimension_length(*first, u_span)
                                && same_dimension_length(*second, v_span))
                                || (same_dimension_length(*first, v_span)
                                    && same_dimension_length(*second, u_span)))
                    })
            })
    };
    let mut candidates = Vec::new();
    for (first_u_index, &u0) in u.iter().enumerate() {
        for &u1 in &u[first_u_index + 1..] {
            for (first_v_index, &v0) in v.iter().enumerate() {
                for &v1 in &v[first_v_index + 1..] {
                    if !dimensions_match(u0, u1, v0, v1) {
                        continue;
                    }
                    let corners = [(u0, v0), (u1, v0), (u1, v1), (u0, v1)];
                    let matched = corners.map(|corner| {
                        let mut matches = points
                            .iter()
                            .filter(|(_, point)| *point == corner)
                            .map(|(marker, _)| *marker);
                        let marker = matches.next()?;
                        matches.next().is_none().then_some(marker)
                    });
                    let [Some(first), Some(second), Some(third), Some(fourth)] = matched else {
                        continue;
                    };
                    candidates.push([first, second, third, fourth]);
                }
            }
        }
    }
    let [candidate] = candidates.as_slice() else {
        return None;
    };
    Some(*candidate)
}

fn ordered_compact_line_profile(
    lines: &[(
        SketchEntityId,
        &SketchInputEntity,
        &SketchInputEntity,
        Point2,
        Point2,
    )],
) -> Option<Vec<SketchEntityUse>> {
    if lines.len() < 3 {
        return None;
    }
    let mut used = vec![false; lines.len()];
    let mut profile = Vec::with_capacity(lines.len());
    let first = lines.first()?;
    used[0] = true;
    profile.push(SketchEntityUse {
        entity: first.0.clone(),
        reversed: false,
    });
    let origin = first.3;
    let mut current = first.4;
    while profile.len() < lines.len() {
        let mut candidates = lines.iter().enumerate().filter_map(|(index, line)| {
            if used[index] {
                None
            } else if line.3 == current {
                Some((index, false, line.4))
            } else if line.4 == current {
                Some((index, true, line.3))
            } else {
                None
            }
        });
        let candidate = candidates.next()?;
        if candidates.next().is_some() {
            return None;
        }
        used[candidate.0] = true;
        profile.push(SketchEntityUse {
            entity: lines[candidate.0].0.clone(),
            reversed: candidate.1,
        });
        current = candidate.2;
    }
    (current == origin).then_some(profile)
}

fn complete_ordered_compact_line_profile(
    lines: &[(
        SketchEntityId,
        &SketchInputEntity,
        &SketchInputEntity,
        Point2,
        Point2,
    )],
    marker_count: usize,
) -> Option<Vec<SketchEntityUse>> {
    (lines.len() == marker_count)
        .then(|| ordered_compact_line_profile(lines))
        .flatten()
}

fn compact_line_region_addresses(payload: &[u8]) -> Option<Vec<u16>> {
    const NAME: &[u8] = b"moSketchRegion_c";
    let matches = payload
        .windows(NAME.len())
        .enumerate()
        .filter_map(|(offset, bytes)| (bytes == NAME).then_some(offset))
        .collect::<Vec<_>>();
    let [offset] = matches.as_slice() else {
        return None;
    };
    let header = offset.checked_add(NAME.len())?;
    let region_token = u16::from_le_bytes(payload.get(header..header + 2)?.try_into().ok()?);
    if region_token == 0 {
        return None;
    }
    let count = usize::from(u16::from_le_bytes(
        payload.get(header + 2..header + 4)?.try_into().ok()?,
    ));
    if count < 3 {
        return None;
    }
    // Each region entry consumes a 12-byte record from `header + 4` onward.
    bounded_len(count as u64, 12, payload.len().saturating_sub(header + 4))?;
    let mut addresses = Vec::with_capacity(count);
    let mut entry_token = None;
    for index in 0..count {
        let entry = header.checked_add(4 + index * 12)?;
        let token = u16::from_le_bytes(payload.get(entry..entry + 2)?.try_into().ok()?);
        if !matches!(token, 0x80e1 | 0x8386 | 0xbc87)
            || entry_token.is_some_and(|existing| existing != token)
            || payload.get(entry + 4..entry + 8)? != [0xff; 4]
            || payload.get(entry + 8..entry + 12)? != [0; 4]
        {
            return None;
        }
        entry_token = Some(token);
        addresses.push(u16::from_le_bytes(
            payload.get(entry + 2..entry + 4)?.try_into().ok()?,
        ));
    }
    let expected = (1..=u16::try_from(count).ok()?).collect::<HashSet<_>>();
    (addresses.iter().copied().collect::<HashSet<_>>() == expected).then_some(addresses)
}

fn compact_line_chain_addresses(payload: &[u8]) -> Option<Vec<u16>> {
    let matches = (0..payload.len()).filter_map(|offset| {
        let bytes = payload.get(offset..)?;
        let count = usize::from(u16::from_le_bytes(bytes.get(..2)?.try_into().ok()?));
        if !(3..=64).contains(&count) {
            return None;
        }
        let addresses_end = 2usize.checked_add(count.checked_mul(4)?)?;
        let trailer = bytes.get(addresses_end..addresses_end.checked_add(40)?)?;
        if u32::from_le_bytes(trailer.get(..4)?.try_into().ok()?) != 1
            || trailer.get(4..6)? != [0, 0]
            || u32::from_le_bytes(trailer.get(6..10)?.try_into().ok()?)
                != u32::try_from(count + 2).ok()?
            || trailer.get(10..14)? != [0xff; 4]
            || trailer.get(14..22)?.iter().any(|byte| *byte != 0)
            || u32::from_le_bytes(trailer.get(22..26)?.try_into().ok()?)
                != u32::try_from(count + 1).ok()?
            || u32::from_le_bytes(trailer.get(26..30)?.try_into().ok()?)
                != u32::try_from(count + 1).ok()?
            || trailer.get(30..36)? != [0xff, 0xfe, 0xff, 0, 0, 0]
            || trailer.get(36..40)? != [0xff; 4]
        {
            return None;
        }
        let addresses = (0..count)
            .filter_map(|index| {
                let offset = 2 + index * 4;
                u16::try_from(u32::from_le_bytes(
                    bytes.get(offset..offset + 4)?.try_into().ok()?,
                ))
                .ok()
            })
            .collect::<Vec<_>>();
        let expected = (1..=u16::try_from(count).ok()?).collect::<HashSet<_>>();
        (addresses.len() == count && addresses.iter().copied().collect::<HashSet<_>>() == expected)
            .then_some(addresses)
    });
    let mut matches = matches.collect::<Vec<_>>();
    matches.dedup();
    let [addresses] = matches.as_slice() else {
        return None;
    };
    Some(addresses.clone())
}

fn compact_reference_plane_source(payload: &[u8]) -> Option<u32> {
    const CLASS: &[u8] = b"moCompRefPlane_c";
    let class_count = payload
        .windows(CLASS.len())
        .filter(|bytes| *bytes == CLASS)
        .count();
    let declared = (class_count == 1)
        .then(|| {
            payload.windows(67).filter_map(|bytes| {
                let source = u32::from_le_bytes(bytes.get(..4)?.try_into().ok()?);
                (source != 0
                    && bytes.get(8..12) == Some(&[0, 0, 3, 0])
                    && bytes.get(12..39)?.iter().all(|byte| *byte == 0)
                    && bytes.get(39..47) == Some(&1.0f64.to_le_bytes())
                    && bytes.get(47..63)
                        == Some(&[
                            0x00, 0x00, 0x00, 0x04, 0x00, 0x00, 0x00, 0xf9, 0xff, 0xff, 0xff, 0x00,
                            0x00, 0x00, 0x00, 0x65,
                        ]))
                .then_some(source)
            })
        })
        .into_iter()
        .flatten();
    let component = payload.windows(138).filter_map(|bytes| {
        let source = u32::from_le_bytes(bytes.get(..4)?.try_into().ok()?);
        let scalar = |offset| {
            let value = f64::from_le_bytes(bytes.get(offset..offset + 8)?.try_into().ok()?);
            value.is_finite().then_some(value)
        };
        let basis = [
            Vector3::new(scalar(15)?, scalar(23)?, scalar(31)?),
            Vector3::new(scalar(39)?, scalar(47)?, scalar(55)?),
            Vector3::new(scalar(63)?, scalar(71)?, scalar(79)?),
        ];
        let norm = |vector: Vector3| {
            (vector.x * vector.x + vector.y * vector.y + vector.z * vector.z).sqrt()
        };
        let dot =
            |left: Vector3, right: Vector3| left.x * right.x + left.y * right.y + left.z * right.z;
        (source != 0
            && bytes.get(8..14)?.iter().all(|byte| *byte == 0)
            && bytes.get(14) == Some(&1)
            && basis
                .iter()
                .all(|vector| (norm(*vector) - 1.0).abs() <= 1.0e-9)
            && dot(basis[0], basis[1]).abs() <= 1.0e-9
            && dot(basis[0], basis[2]).abs() <= 1.0e-9
            && dot(basis[1], basis[2]).abs() <= 1.0e-9
            && bytes.get(122..126) == Some(&4u32.to_le_bytes())
            && bytes.get(126..130) == Some(&[0xff; 4]))
        .then_some(source)
    });
    let matches = declared.chain(component).collect::<HashSet<_>>();
    let mut matches = matches.into_iter();
    let source = matches.next()?;
    matches.next().is_none().then_some(source)
}

fn compact_profile_reference_plane_source(
    payload: &[u8],
    context_start: usize,
    profile_start: usize,
    profile_end: usize,
) -> Option<u32> {
    let profile = payload.get(profile_start..profile_end)?;
    compact_reference_plane_source(profile)
        .or_else(|| {
            payload
                .get(context_start..profile_end)
                .and_then(compact_reference_plane_source)
        })
        .or_else(|| compact_reference_plane_source(payload))
}

fn principal_sketch_frame(
    plane: cadmpeg_ir::features::PrincipalPlane,
) -> (Point3, Vector3, Vector3) {
    use cadmpeg_ir::features::PrincipalPlane;
    match plane {
        PrincipalPlane::Front => (
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, -1.0, 0.0),
            Vector3::new(0.0, 0.0, -1.0),
        ),
        PrincipalPlane::Top => (
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
        ),
        PrincipalPlane::Right => (
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(1.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, -1.0),
        ),
    }
}

fn bind_circular_profile_by_dimension(
    features: &mut [cadmpeg_ir::features::Feature],
    sketches: &mut [Sketch],
    sketch_entities: &[SketchEntity],
    parameters: &[cadmpeg_ir::features::DesignParameter],
) {
    let geometry_by_entity = sketch_entities
        .iter()
        .map(|entity| (&entity.id, &entity.geometry))
        .collect::<HashMap<_, _>>();
    let circular_profiles = sketches
        .iter()
        .filter_map(|sketch| {
            let [profile] = sketch.profiles.as_slice() else {
                return None;
            };
            let [entity] = profile.as_slice() else {
                return None;
            };
            let SketchGeometry::Circle { radius, .. } = geometry_by_entity.get(&entity.entity)?
            else {
                return None;
            };
            Some((sketch.id.clone(), radius.0))
        })
        .collect::<Vec<_>>();
    let mut proposals = Vec::new();
    for (sketch, radius) in circular_profiles {
        let matches = features
            .iter()
            .enumerate()
            .filter(|(_, feature)| {
                matches!(
                    feature.definition,
                    cadmpeg_ir::features::FeatureDefinition::Sketch {
                        space: cadmpeg_ir::features::SketchSpace::Planar,
                        ..
                    }
                )
            })
            .filter(|(_, feature)| {
                parameters.iter().any(|parameter| {
                    if parameter.owner != feature.id {
                        return false;
                    }
                    let Some(cadmpeg_ir::features::ParameterValue::Length(value)) =
                        &parameter.value
                    else {
                        return false;
                    };
                    let expected = match parameter.display {
                        Some(cadmpeg_ir::features::DimensionDisplay::Radius) => value.0,
                        Some(cadmpeg_ir::features::DimensionDisplay::Diameter) => value.0 * 0.5,
                        None => return false,
                    };
                    same_dimension_length(expected, radius)
                })
            })
            .map(|(index, _)| index)
            .collect::<Vec<_>>();
        if let [feature] = matches.as_slice() {
            proposals.push((sketch, *feature));
        }
    }
    let mut feature_counts = HashMap::new();
    for (_, feature) in &proposals {
        *feature_counts.entry(*feature).or_insert(0usize) += 1;
    }
    for (sketch_id, feature_index) in proposals {
        if feature_counts.get(&feature_index) != Some(&1) {
            continue;
        }
        for feature in features.iter_mut() {
            let cadmpeg_ir::features::FeatureDefinition::Sketch { sketch: bound, .. } =
                &mut feature.definition
            else {
                continue;
            };
            if bound.as_ref() == Some(&sketch_id) {
                *bound = None;
            }
        }
        let name = features[feature_index].name.clone();
        let cadmpeg_ir::features::FeatureDefinition::Sketch { sketch, .. } =
            &mut features[feature_index].definition
        else {
            continue;
        };
        *sketch = Some(sketch_id.clone());
        if let Some(native) = sketches.iter_mut().find(|sketch| sketch.id == sketch_id) {
            native.name = name;
        }
    }
}

/// Bind neutral parameters to uniquely owned native scalar records.
pub(crate) fn bind_parameter_scalars<'a>(
    parameters: &mut [cadmpeg_ir::features::DesignParameter],
    features: &[cadmpeg_ir::features::Feature],
    histories: &[crate::records::FeatureHistory],
    lanes: impl IntoIterator<Item = &'a FeatureInputLane>,
) {
    let neutral_owners = features
        .iter()
        .filter_map(|feature| Some((&feature.id, feature.native_ref.as_deref()?)))
        .collect::<HashMap<_, _>>();
    let native_features = histories
        .iter()
        .flat_map(|history| &history.features)
        .map(|feature| (feature.id.as_str(), feature))
        .collect::<HashMap<_, _>>();
    for lane in lanes {
        let length_scalars = lane
            .relation_instances
            .iter()
            .filter(|relation| relation.family != FeatureInputRelationFamily::Angle)
            .filter_map(|relation| relation.parameter_scalar_ref.as_deref())
            .collect::<HashSet<_>>();
        let angle_scalars = lane
            .relation_instances
            .iter()
            .filter(|relation| relation.family == FeatureInputRelationFamily::Angle)
            .filter_map(|relation| relation.parameter_scalar_ref.as_deref())
            .collect::<HashSet<_>>();
        let detached_scalars = lane
            .relation_instances
            .iter()
            .filter_map(|relation| relation.parameter_scalar_ref.as_deref())
            .filter(|id| {
                lane.scalars
                    .iter()
                    .find(|scalar| scalar.id == **id)
                    .is_some_and(|scalar| scalar.operands.is_empty())
            })
            .collect::<HashSet<_>>();
        let names_by_id = lane
            .names
            .iter()
            .map(|name| (name.id.as_str(), name.value.as_str()))
            .collect::<HashMap<_, _>>();
        let mut starts = Vec::<(u64, &crate::records::Feature)>::new();
        for feature in native_features.values() {
            let start = feature_object_name(feature, lane).map_or(u64::MAX, |name| name.offset);
            starts.push((start, feature));
        }
        starts.sort_by_key(|start| start.0);
        for (index, &(start, native_feature)) in starts.iter().enumerate() {
            let end = starts.get(index + 1).map_or(u64::MAX, |next| next.0);
            let owner_parameters = parameters.iter_mut().filter(|parameter| {
                neutral_owners.get(&parameter.owner).copied() == Some(native_feature.id.as_str())
            });
            for parameter in owner_parameters {
                if parameter.native_ref.is_some() {
                    continue;
                }
                let scalars = lane
                    .scalars
                    .iter()
                    .filter(|scalar| match scalar.feature_ref.as_deref() {
                        Some(owner) => owner == native_feature.id,
                        None => scalar.offset > start && scalar.offset < end,
                    })
                    .filter(|scalar| {
                        names_by_id.get(scalar.name.as_str()).copied()
                            == Some(parameter.name.as_str())
                    })
                    .collect::<Vec<_>>();
                let driving = scalars
                    .iter()
                    .filter(|scalar| scalar.role == FeatureInputScalarRole::Driving)
                    .copied()
                    .collect::<Vec<_>>();
                let candidates = if driving.is_empty() {
                    scalars
                        .into_iter()
                        .filter(|scalar| scalar.role == FeatureInputScalarRole::Native)
                        .collect::<Vec<_>>()
                } else {
                    driving
                };
                let compatible = candidates
                    .into_iter()
                    .filter(|scalar| match parameter.value.as_ref() {
                        Some(cadmpeg_ir::features::ParameterValue::Integer(expected)) => {
                            let Some(expected) = crate::history::exact_integer_f64(*expected)
                            else {
                                return false;
                            };
                            if length_scalars.contains(scalar.id.as_str())
                                || angle_scalars.contains(scalar.id.as_str())
                            {
                                same_dimension_length(scalar.value * 1000.0, expected)
                            } else {
                                scalar.value == expected
                            }
                        }
                        Some(cadmpeg_ir::features::ParameterValue::Boolean(expected)) => {
                            let expected = if *expected { 1.0 } else { 0.0 };
                            if length_scalars.contains(scalar.id.as_str())
                                || angle_scalars.contains(scalar.id.as_str())
                            {
                                same_dimension_length(scalar.value * 1000.0, expected)
                            } else {
                                scalar.value == expected
                            }
                        }
                        _ => true,
                    })
                    .collect::<Vec<_>>();
                if let [scalar] = compatible.as_slice() {
                    parameter.native_ref = Some(scalar.id.clone());
                    let scalar_is_detached = detached_scalars.contains(scalar.id.as_str());
                    let scalar_is_untyped_real = matches!(
                        parameter.value,
                        Some(cadmpeg_ir::features::ParameterValue::Real(_))
                    ) && !scalar_is_detached;
                    if scalar_is_detached && length_scalars.contains(scalar.id.as_str()) {
                        parameter.expression =
                            crate::history::format_length_mm(scalar.value * 1000.0);
                    } else if scalar_is_detached && angle_scalars.contains(scalar.id.as_str()) {
                        parameter.expression = crate::history::format_angle_rad(scalar.value);
                    }
                    let evaluated = if length_scalars.contains(scalar.id.as_str())
                        && !scalar_is_untyped_real
                    {
                        Some(cadmpeg_ir::features::ParameterValue::Length(
                            cadmpeg_ir::features::Length(scalar.value * 1000.0),
                        ))
                    } else if angle_scalars.contains(scalar.id.as_str()) && !scalar_is_untyped_real
                    {
                        Some(cadmpeg_ir::features::ParameterValue::Angle(
                            cadmpeg_ir::features::Angle(scalar.value),
                        ))
                    } else {
                        match parameter.value.as_ref() {
                            Some(cadmpeg_ir::features::ParameterValue::Length(_)) => {
                                Some(cadmpeg_ir::features::ParameterValue::Length(
                                    cadmpeg_ir::features::Length(scalar.value * 1000.0),
                                ))
                            }
                            Some(cadmpeg_ir::features::ParameterValue::Angle(_)) => {
                                Some(cadmpeg_ir::features::ParameterValue::Angle(
                                    cadmpeg_ir::features::Angle(scalar.value),
                                ))
                            }
                            Some(cadmpeg_ir::features::ParameterValue::Real(_)) => {
                                Some(cadmpeg_ir::features::ParameterValue::Real(scalar.value))
                            }
                            _ => None,
                        }
                    };
                    if let Some(evaluated) = evaluated {
                        parameter.value = Some(evaluated);
                    }
                }
            }
        }
    }
}

/// Apply relation-defined units and display semantics to parameters named by display scalars.
pub(crate) fn type_display_relation_parameters(
    parameters: &mut [cadmpeg_ir::features::DesignParameter],
    features: &[cadmpeg_ir::features::Feature],
    lanes: &[FeatureInputLane],
) {
    let mut families = HashMap::<cadmpeg_ir::features::ParameterId, HashSet<_>>::new();
    for lane in lanes {
        for relation in &lane.relation_instances {
            let parameter = relation
                .parameter_scalar_ref
                .as_deref()
                .and_then(|scalar| {
                    parameters
                        .iter()
                        .find(|parameter| parameter.native_ref.as_deref() == Some(scalar))
                })
                .or_else(|| {
                    relation.parameter_scalar_ref.is_none().then_some(())?;
                    relation_parameter_by_display_name(relation, lane, features, parameters)
                });
            let Some(parameter) = parameter else { continue };
            families
                .entry(parameter.id.clone())
                .or_default()
                .insert(relation.family);
        }
    }
    for parameter in parameters {
        let Some(families) = families.get(&parameter.id) else {
            continue;
        };
        if families.len() != 1 {
            continue;
        }
        let family = *families.iter().next().expect("one relation family");
        match family {
            FeatureInputRelationFamily::Angle => {
                if let Some(cadmpeg_ir::features::ParameterValue::Real(value)) = parameter.value {
                    parameter.expression = crate::history::format_angle_rad(value);
                    parameter.value = Some(cadmpeg_ir::features::ParameterValue::Angle(
                        cadmpeg_ir::features::Angle(value),
                    ));
                }
            }
            FeatureInputRelationFamily::LineLineDistance
            | FeatureInputRelationFamily::PointPointDistance
            | FeatureInputRelationFamily::PointLineDistance
            | FeatureInputRelationFamily::PointPointHorizontalDistance
            | FeatureInputRelationFamily::PointPointVerticalDistance
            | FeatureInputRelationFamily::CircleDiameter => {
                if let Some(cadmpeg_ir::features::ParameterValue::Real(value)) = parameter.value {
                    let value = value * 1000.0;
                    parameter.expression = if family == FeatureInputRelationFamily::CircleDiameter {
                        format!("<MOD-DIAM>{}", crate::history::format_length_mm(value))
                    } else {
                        crate::history::format_length_mm(value)
                    };
                    parameter.value = Some(cadmpeg_ir::features::ParameterValue::Length(
                        cadmpeg_ir::features::Length(value),
                    ));
                }
                if let Some(cadmpeg_ir::features::ParameterValue::Integer(value)) =
                    parameter.value.as_ref()
                {
                    let Some(value) = crate::history::exact_integer_f64(*value) else {
                        continue;
                    };
                    parameter.expression = if family == FeatureInputRelationFamily::CircleDiameter {
                        format!("<MOD-DIAM>{}", crate::history::format_length_mm(value))
                    } else {
                        crate::history::format_length_mm(value)
                    };
                    parameter.value = Some(cadmpeg_ir::features::ParameterValue::Length(
                        cadmpeg_ir::features::Length(value),
                    ));
                }
                if family == FeatureInputRelationFamily::CircleDiameter
                    && matches!(
                        parameter.value,
                        Some(cadmpeg_ir::features::ParameterValue::Length(_))
                    )
                    && parameter.display.is_none()
                {
                    parameter.display = Some(cadmpeg_ir::features::DimensionDisplay::Diameter);
                }
            }
        }
    }
}

pub(crate) fn project_compact_body_selections(
    features: &mut [cadmpeg_ir::features::Feature],
    lanes: &[FeatureInputLane],
) {
    let selections = lanes.iter().flat_map(|lane| &lane.body_selections).fold(
        HashMap::<&str, Vec<&FeatureInputBodySelection>>::new(),
        |mut by_feature, selection| {
            by_feature
                .entry(selection.feature_ref.as_str())
                .or_default()
                .push(selection);
            by_feature
        },
    );
    for feature in features {
        let Some(native_ref) = feature.native_ref.as_deref() else {
            continue;
        };
        let Some([selection]) = selections.get(native_ref).map(Vec::as_slice) else {
            continue;
        };
        let FeatureDefinition::DeleteBody { bodies, mode } = &mut feature.definition else {
            continue;
        };
        if matches!(bodies, cadmpeg_ir::features::BodySelection::Unresolved) {
            *bodies = cadmpeg_ir::features::BodySelection::Local {
                bodies: selection
                    .local_body_ids
                    .iter()
                    .map(u32::to_string)
                    .collect(),
                native: compact_body_selection_value(&selection.local_body_ids),
            };
        }
        if matches!(mode, cadmpeg_ir::features::BodyRetentionMode::Unresolved) {
            if let Some(native_mode) = selection.mode {
                *mode = native_mode;
            }
        }
    }
}

pub(crate) fn project_compact_edge_selections(
    features: &mut [cadmpeg_ir::features::Feature],
    lanes: &[FeatureInputLane],
) {
    let feature_ids_by_native = features
        .iter()
        .filter_map(|feature| Some((feature.native_ref.clone()?, feature.id.clone())))
        .collect::<HashMap<_, _>>();
    let selections = lanes.iter().flat_map(|lane| &lane.edge_selections).fold(
        HashMap::<&str, Vec<&FeatureInputEdgeSelection>>::new(),
        |mut by_feature, selection| {
            by_feature
                .entry(selection.feature_ref.as_str())
                .or_default()
                .push(selection);
            by_feature
        },
    );
    for feature in features {
        let Some(native_ref) = feature.native_ref.as_deref() else {
            continue;
        };
        let Some(edge_selections) = selections
            .get(native_ref)
            .filter(|selections| !selections.is_empty())
        else {
            continue;
        };
        let (FeatureDefinition::Fillet { edges, .. } | FeatureDefinition::Chamfer { edges, .. }) =
            &mut feature.definition
        else {
            continue;
        };
        if matches!(edges, cadmpeg_ir::features::EdgeSelection::Unresolved) {
            let native = compact_edge_selection_set_value(edge_selections);
            let generated = edge_selections
                .iter()
                .map(|selection| {
                    let native_feature = selection.terminal_feature_ref.as_ref()?;
                    let feature = feature_ids_by_native.get(native_feature)?.clone();
                    let local_id = selection
                        .local_edge_ids
                        .iter()
                        .map(u32::to_string)
                        .collect::<Vec<_>>()
                        .join(",");
                    Some(cadmpeg_ir::features::GeneratedEdgeRef { feature, local_id })
                })
                .collect::<Option<Vec<_>>>();
            *edges = match generated.filter(|edges| !edges.is_empty()) {
                Some(edges) => cadmpeg_ir::features::EdgeSelection::Generated { edges, native },
                None => cadmpeg_ir::features::EdgeSelection::Native(native),
            };
            for dependency in edge_selections
                .iter()
                .flat_map(|selection| &selection.producer_feature_refs)
                .filter_map(|native| feature_ids_by_native.get(native))
            {
                if dependency != &feature.id && !feature.dependencies.contains(dependency) {
                    feature.dependencies.push(dependency.clone());
                }
            }
        }
    }
}

pub(crate) fn project_compact_surface_selections(
    features: &mut [cadmpeg_ir::features::Feature],
    lanes: &[FeatureInputLane],
) {
    enum SelectionSlot<'a> {
        Face(&'a mut cadmpeg_ir::features::FaceSelection),
        Vertex(&'a mut cadmpeg_ir::features::VertexSelection),
    }
    let feature_ids_by_native = features
        .iter()
        .filter_map(|feature| Some((feature.native_ref.clone()?, feature.id.clone())))
        .collect::<HashMap<_, _>>();
    let selections = lanes.iter().flat_map(|lane| &lane.surface_selections).fold(
        HashMap::<&str, Vec<&FeatureInputSurfaceSelection>>::new(),
        |mut map, selection| {
            map.entry(selection.feature_ref.as_str())
                .or_default()
                .push(selection);
            map
        },
    );
    for feature in features {
        let Some(native_ref) = feature.native_ref.as_deref() else {
            continue;
        };
        let Some([selection]) = selections.get(native_ref).map(Vec::as_slice) else {
            continue;
        };
        let slot = match &mut feature.definition {
            FeatureDefinition::Thicken { faces, .. } => SelectionSlot::Face(faces),
            FeatureDefinition::Extrude {
                extent:
                    cadmpeg_ir::features::Extent::ToFace { face }
                    | cadmpeg_ir::features::Extent::OffsetFromFace { face, .. },
                ..
            } => SelectionSlot::Face(face),
            FeatureDefinition::Extrude {
                extent: cadmpeg_ir::features::Extent::ToVertex { vertex },
                ..
            } => SelectionSlot::Vertex(vertex),
            _ => continue,
        };
        let native = compact_surface_selection_value(&selection.components);
        let generated = selection
            .terminal_feature_ref
            .as_ref()
            .and_then(|producer| feature_ids_by_native.get(producer))
            .zip(selection.components.last());
        match slot {
            SelectionSlot::Face(faces) => {
                if matches!(
                    faces,
                    cadmpeg_ir::features::FaceSelection::Unresolved
                        | cadmpeg_ir::features::FaceSelection::Native(_)
                ) {
                    *faces = match generated {
                        Some((feature, component)) => {
                            cadmpeg_ir::features::FaceSelection::Generated {
                                faces: vec![cadmpeg_ir::features::GeneratedFaceRef {
                                    feature: feature.clone(),
                                    local_id: component.local_id.to_string(),
                                }],
                                native,
                            }
                        }
                        None => cadmpeg_ir::features::FaceSelection::Native(native),
                    };
                }
            }
            SelectionSlot::Vertex(vertex) => {
                // Edge-endpoint references keep the endpoint selector native.
                let retain_native = matches!(
                    &*vertex,
                    cadmpeg_ir::features::VertexSelection::Native(value)
                        if value.starts_with("sldprt:feature-input:edge-endpoint-ref:")
                );
                if !retain_native
                    && matches!(
                        vertex,
                        cadmpeg_ir::features::VertexSelection::Unresolved
                            | cadmpeg_ir::features::VertexSelection::Native(_)
                    )
                {
                    *vertex = match generated {
                        Some((feature, component)) => {
                            cadmpeg_ir::features::VertexSelection::Generated {
                                vertex: cadmpeg_ir::features::GeneratedVertexRef {
                                    feature: feature.clone(),
                                    local_id: component.local_id.to_string(),
                                },
                                native,
                            }
                        }
                        None => cadmpeg_ir::features::VertexSelection::Native(native),
                    };
                }
            }
        }
        for producer in selection
            .producer_feature_refs
            .iter()
            .filter_map(|producer| feature_ids_by_native.get(producer))
            .filter(|producer| *producer != &feature.id)
        {
            if !feature.dependencies.contains(producer) {
                feature.dependencies.push(producer.clone());
            }
        }
    }
}

/// Add semantic termination forms carried by compact extrusion end-spec children.
pub(crate) fn enrich_history_extrusion_terminations(
    histories: &mut [crate::records::FeatureHistory],
    lanes: &[FeatureInputLane],
) {
    type TerminationVote = (String, Option<String>, Option<String>);
    let mut terminations = HashMap::<String, Vec<Option<TerminationVote>>>::new();
    for lane in lanes {
        let mut objects = histories
            .iter()
            .flat_map(|history| &history.features)
            .filter_map(|feature| {
                Some((
                    feature_object_name(feature, lane)?.offset,
                    feature.id.clone(),
                ))
            })
            .collect::<Vec<_>>();
        objects.sort_unstable_by_key(|object| object.0);
        for (index, (start, feature_id)) in objects.iter().enumerate() {
            let Some(feature) = histories
                .iter()
                .flat_map(|history| &history.features)
                .find(|feature| feature.id == *feature_id)
            else {
                continue;
            };
            if !matches!(feature.xml_tag.as_str(), "Extrusion" | "Cut") {
                continue;
            }
            let has_depth =
                feature.parameters.contains_key("Depth") || feature.parameters.contains_key("D1");
            let Ok(start) = usize::try_from(*start) else {
                continue;
            };
            let mut end_index = index + 1;
            while let Some((_, next_id)) = objects.get(end_index) {
                let skip = histories
                    .iter()
                    .flat_map(|history| &history.features)
                    .find(|feature| feature.id == *next_id)
                    .is_some_and(|feature| {
                        let class = feature.input_class.as_deref().unwrap_or_default();
                        native_object_class(class).kind == NativeClassKind::ProfileFeature
                            || class == "moCosmeticThread_c"
                    });
                if !skip {
                    break;
                }
                end_index += 1;
            }
            let end = objects
                .get(end_index)
                .and_then(|object| usize::try_from(object.0).ok())
                .unwrap_or(lane.native_payload.len());
            let lane_key = lane
                .id
                .rsplit_once('#')
                .map_or(lane.id.as_str(), |(_, key)| key);
            let candidates = (start..end.saturating_sub(103))
                .filter_map(|offset| {
                    if compact_extrusion_mid_plane_at(&lane.native_payload, offset) {
                        return Some(("Symmetric".to_string(), None, None));
                    }
                    if let Some(reference) =
                        compact_extrusion_offset_from_face_at(&lane.native_payload, offset, end)
                    {
                        return Some((
                            "OffsetFromFace".to_string(),
                            Some(format!(
                                "sldprt:feature-input:single-face-ref:{lane_key}:{reference}"
                            )),
                            None,
                        ));
                    }
                    if compact_extrusion_through_all_both_at(&lane.native_payload, offset) {
                        return Some(("ThroughAllBoth".to_string(), None, None));
                    }
                    if has_depth
                        && compact_extrusion_blind_through_all_second_at(
                            &lane.native_payload,
                            offset,
                        )
                    {
                        return Some(("Blind".to_string(), None, Some("ThroughAll".to_string())));
                    }
                    if has_depth {
                        return None;
                    }
                    if compact_extrusion_through_all_at(&lane.native_payload, offset) {
                        Some(("ThroughAll".to_string(), None, None))
                    } else if compact_extrusion_through_next_at(&lane.native_payload, offset) {
                        Some(("ThroughNext".to_string(), None, None))
                    } else if let Some((reference, kind)) =
                        compact_extrusion_to_vertex_at(&lane.native_payload, offset)
                    {
                        let prefix = match kind {
                            CompactPointReferenceKind::Point => "point-ref",
                            CompactPointReferenceKind::EdgeEndpoint => "edge-endpoint-ref",
                        };
                        Some((
                            "ToVertex".to_string(),
                            Some(format!(
                                "sldprt:feature-input:{prefix}:{lane_key}:{reference}"
                            )),
                            None,
                        ))
                    } else {
                        compact_extrusion_to_face_at(&lane.native_payload, offset).map(
                            |reference| {
                                (
                                    "ToFace".to_string(),
                                    Some(format!(
                                    "sldprt:feature-input:single-face-ref:{lane_key}:{reference}"
                                )),
                                    None,
                                )
                            },
                        )
                    }
                })
                .collect::<Vec<_>>();
            terminations.entry(feature_id.clone()).or_default().push(
                candidates
                    .as_slice()
                    .first()
                    .cloned()
                    .filter(|_| candidates.len() == 1),
            );
        }
    }
    for feature in histories
        .iter_mut()
        .flat_map(|history| &mut history.features)
    {
        if feature.properties.contains_key("EndCondition") {
            continue;
        }
        let Some(votes) = terminations.get(&feature.id) else {
            continue;
        };
        let Some(Some(first)) = votes.first() else {
            continue;
        };
        if !votes.iter().all(|vote| vote.as_ref() == Some(first)) {
            continue;
        }
        feature
            .properties
            .insert("EndCondition".into(), first.0.clone());
        if let Some(reference) = &first.1 {
            let key = if first.0 == "ToVertex" {
                "Vertex"
            } else {
                "Face"
            };
            feature
                .properties
                .entry(key.into())
                .or_insert_with(|| reference.clone());
        }
        if let Some(second) = &first.2 {
            feature
                .properties
                .insert("EndCondition2".into(), second.clone());
        }
    }
}

/// Add target and tool body paths carried by compact combine objects.
pub(crate) fn enrich_history_combine_selections(
    histories: &mut [crate::records::FeatureHistory],
    lanes: &[FeatureInputLane],
) {
    let mut selections = HashMap::<String, Vec<Option<(String, String, Option<String>)>>>::new();
    for lane in lanes {
        let mut objects = histories
            .iter()
            .flat_map(|history| &history.features)
            .filter_map(|feature| {
                Some((
                    feature_object_name(feature, lane)?.offset,
                    feature.id.clone(),
                ))
            })
            .collect::<Vec<_>>();
        objects.sort_unstable_by_key(|object| object.0);
        for (index, (start, feature_id)) in objects.iter().enumerate() {
            let Some(feature) = histories
                .iter()
                .flat_map(|history| &history.features)
                .find(|feature| feature.id == *feature_id)
            else {
                continue;
            };
            if native_object_class(feature.input_class.as_deref().unwrap_or_default()).kind
                != NativeClassKind::Combine
            {
                continue;
            }
            let Ok(start) = usize::try_from(*start) else {
                continue;
            };
            let end = objects
                .get(index + 1)
                .and_then(|object| usize::try_from(object.0).ok())
                .unwrap_or(lane.native_payload.len());
            let paths = (start.saturating_add(12)
                ..end.saturating_sub(COMPACT_EDGE_VECTOR_MARKER.len()))
                .filter_map(|marker| {
                    compact_body_path_at(&lane.native_payload, marker).map(|_| marker)
                })
                .collect::<Vec<_>>();
            let selection = if let [target, tools] = paths.as_slice() {
                let operation = compact_combine_operation_at(&lane.native_payload, start);
                let lane_key = lane
                    .id
                    .rsplit_once('#')
                    .map_or(lane.id.as_str(), |(_, key)| key);
                Some((
                    format!("sldprt:feature-input:body-path:{lane_key}:{target}"),
                    format!("sldprt:feature-input:body-path:{lane_key}:{tools}"),
                    operation.map(str::to_string),
                ))
            } else {
                None
            };
            selections
                .entry(feature_id.clone())
                .or_default()
                .push(selection);
        }
    }
    for feature in histories
        .iter_mut()
        .flat_map(|history| &mut history.features)
    {
        let Some(votes) = selections.get(&feature.id) else {
            continue;
        };
        let Some(Some(first)) = votes.first() else {
            continue;
        };
        if !votes.iter().all(|vote| vote.as_ref() == Some(first)) {
            continue;
        }
        feature
            .properties
            .entry("Target".into())
            .or_insert_with(|| first.0.clone());
        feature
            .properties
            .entry("Tools".into())
            .or_insert_with(|| first.1.clone());
        if let Some(operation) = &first.2 {
            feature
                .properties
                .entry("Operation".into())
                .or_insert_with(|| operation.clone());
        }
    }
}

pub(crate) fn compact_combine_operation_at(
    payload: &[u8],
    name_offset: usize,
) -> Option<&'static str> {
    if payload.get(name_offset..name_offset + 5)? != [0x04, 0x80, 0xff, 0xfe, 0xff] {
        return None;
    }
    let name_units = usize::from(*payload.get(name_offset + 5)?);
    let operation = name_offset.checked_add(117 + name_units.checked_mul(2)?)?;
    if payload
        .get(operation - 12..operation)?
        .iter()
        .any(|byte| *byte != 0)
        || payload.get(operation + 4..operation + 14)? != [0, 0, 0, 0, 0, 0, 0xff, 0xff, 0xff, 0xff]
    {
        return None;
    }
    match u32::from_le_bytes(payload.get(operation..operation + 4)?.try_into().ok()?) {
        0 => Some("Join"),
        1 => Some("Cut"),
        2 => Some("Intersect"),
        _ => None,
    }
}

/// Add compact general-curve reference identities carried by solid sweeps.
pub(crate) fn enrich_history_sweep_paths(
    histories: &mut [crate::records::FeatureHistory],
    lanes: &[FeatureInputLane],
) {
    let mut paths = HashMap::<String, Vec<Option<String>>>::new();
    for lane in lanes {
        let mut objects = histories
            .iter()
            .flat_map(|history| &history.features)
            .filter_map(|feature| {
                Some((
                    feature_object_name(feature, lane)?.offset,
                    feature.id.clone(),
                ))
            })
            .collect::<Vec<_>>();
        objects.sort_unstable_by_key(|object| object.0);
        for (index, &(start, ref feature_id)) in objects.iter().enumerate() {
            let Some(feature) = histories
                .iter()
                .flat_map(|history| &history.features)
                .find(|feature| feature.id == *feature_id)
            else {
                continue;
            };
            if !matches!(
                native_object_class(feature.input_class.as_deref().unwrap_or_default()).kind,
                NativeClassKind::Sweep | NativeClassKind::SweepReferenceSurface
            ) || feature.properties.contains_key("Path")
            {
                continue;
            }
            let (Ok(start), end) = (
                usize::try_from(start),
                objects
                    .get(index + 1)
                    .and_then(|object| usize::try_from(object.0).ok())
                    .unwrap_or(lane.native_payload.len()),
            ) else {
                continue;
            };
            let declared = lane
                .classes
                .iter()
                .filter(|class| {
                    class.name == "moGeneralCurveRef_w"
                        && usize::try_from(class.offset)
                            .is_ok_and(|offset| offset >= start && offset < end)
                })
                .filter_map(|class| usize::try_from(class.offset).ok())
                .collect::<Vec<_>>();
            let compact = (start..end.saturating_sub(16))
                .filter(|offset| compact_general_curve_ref_at(&lane.native_payload, *offset))
                .collect::<Vec<_>>();
            let compact_profiles = (start..end.saturating_sub(16))
                .filter(|offset| {
                    compact_profile_general_curve_ref_at(&lane.native_payload, *offset)
                })
                .collect::<Vec<_>>();
            let mut source_candidates = declared
                .iter()
                .filter_map(|offset| {
                    declared_general_curve_profile_prefix(&lane.native_payload, *offset)
                })
                .chain(compact_profiles.iter().map(|offset| offset + 6))
                .filter_map(|prefix| component_profile_source_at(&lane.native_payload, prefix))
                .collect::<Vec<_>>();
            source_candidates.sort_unstable();
            source_candidates.dedup();
            let path = if let [source] = source_candidates.as_slice() {
                Some(source.to_string())
            } else {
                let mut candidates = declared;
                candidates.extend(compact);
                candidates.extend(compact_profiles);
                candidates.sort_unstable();
                candidates.dedup();
                if let [offset] = candidates.as_slice() {
                    let lane_key = lane
                        .id
                        .rsplit_once('#')
                        .map_or(lane.id.as_str(), |(_, key)| key);
                    Some(format!(
                        "sldprt:feature-input:general-curve-ref:{lane_key}:{offset}"
                    ))
                } else {
                    None
                }
            };
            paths.entry(feature_id.clone()).or_default().push(path);
        }
    }
    for feature in histories
        .iter_mut()
        .flat_map(|history| &mut history.features)
    {
        if feature.properties.contains_key("Path") {
            continue;
        }
        let Some(votes) = paths.get(&feature.id) else {
            continue;
        };
        let Some(Some(first)) = votes.first() else {
            continue;
        };
        if votes.iter().all(|vote| vote.as_ref() == Some(first)) {
            feature.properties.insert("Path".into(), first.clone());
        }
    }
}

/// Bind reference-curve cross sections consumed by surface sweeps.
pub(crate) fn project_surface_sweep_profiles(
    features: &mut [cadmpeg_ir::features::Feature],
    histories: &[crate::records::FeatureHistory],
    lanes: &[FeatureInputLane],
) {
    use cadmpeg_ir::features::{GeneratedCurveRef, ProfileRef};

    let history_features = histories
        .iter()
        .flat_map(|history| &history.features)
        .cloned()
        .collect::<Vec<_>>();
    let feature_ids_by_native = features
        .iter()
        .filter_map(|feature| Some((feature.native_ref.as_deref()?, feature.id.clone())))
        .collect::<HashMap<_, _>>();
    let mut projections = HashMap::new();
    for lane in lanes {
        let Some(reference_class) = lane
            .classes
            .iter()
            .find(|class| class.name == "moCompReferenceCurve_c")
        else {
            continue;
        };
        let Some(class_offset) = usize::try_from(reference_class.offset).ok() else {
            continue;
        };
        let Some(wrapper_token) = class_offset
            .checked_sub(2)
            .and_then(|offset| lane.native_payload.get(offset..offset + 2))
        else {
            continue;
        };
        let wrapper_token = [wrapper_token[0], wrapper_token[1]];
        let declared_prefix = class_offset.checked_add(6 + reference_class.name.len());
        let lane_key = lane
            .id
            .rsplit_once('#')
            .map_or(lane.id.as_str(), |(_, key)| key);
        let mut objects = history_features
            .iter()
            .filter_map(|feature| Some((feature_object_name(feature, lane)?.offset, feature)))
            .collect::<Vec<_>>();
        objects.sort_unstable_by_key(|(offset, _)| *offset);
        for (index, &(start, feature)) in objects.iter().enumerate() {
            if native_object_class(feature.input_class.as_deref().unwrap_or_default()).kind
                != NativeClassKind::SweepReferenceSurface
            {
                continue;
            }
            let (Ok(start), end) = (
                usize::try_from(start),
                objects
                    .get(index + 1)
                    .and_then(|(offset, _)| usize::try_from(*offset).ok())
                    .unwrap_or(lane.native_payload.len()),
            ) else {
                continue;
            };
            let direct = declared_prefix
                .filter(|prefix| (start..end).contains(prefix))
                .and_then(|prefix| component_profile_source_at(&lane.native_payload, prefix))
                .and_then(|source| {
                    let native = history_features.iter().find(|candidate| {
                        candidate
                            .source_id
                            .as_deref()
                            .and_then(|value| value.parse::<u32>().ok())
                            == Some(source)
                    })?;
                    feature_ids_by_native
                        .get(native.id.as_str())
                        .cloned()
                        .map(ProfileRef::Feature)
                });
            let generated = (start..end.saturating_sub(6))
                .filter(|offset| {
                    lane.native_payload.get(*offset..*offset + 2) == Some(&wrapper_token)
                        && lane.native_payload.get(*offset + 4..*offset + 9)
                            == Some(&[0x2b, 0x80, 0x02, 0, 0])
                        && offset.checked_sub(2).is_none_or(|prefix| {
                            lane.native_payload.get(prefix..*offset) != Some(&[1, 0])
                        })
                })
                .filter_map(|wrapper| {
                    let candidates = (wrapper + 4..end.saturating_sub(16))
                        .filter(|marker| {
                            lane.native_payload.get(*marker..*marker + 16)
                                == Some(COMPACT_EDGE_VECTOR_MARKER.as_slice())
                        })
                        .filter_map(|marker| {
                            component_reference_curve_path_at(&lane.native_payload, marker)
                                .map(|components| (marker, components))
                        })
                        .collect::<Vec<_>>();
                    let [(_, components)] = candidates.as_slice() else {
                        return None;
                    };
                    let owner = component_path_terminal_feature(components, &history_features)?;
                    let feature_id = feature_ids_by_native.get(owner.as_str())?.clone();
                    let local_id = components
                        .iter()
                        .map(|component| component.local_id.to_string())
                        .collect::<Vec<_>>()
                        .join(",");
                    let native = format!(
                        "sldprt:feature-input:component-reference-curve:{lane_key}:{wrapper}"
                    );
                    Some((
                        ProfileRef::Generated {
                            curves: vec![GeneratedCurveRef {
                                feature: feature_id,
                                local_id,
                            }],
                            native,
                        },
                        components.clone(),
                    ))
                })
                .collect::<Vec<_>>();
            let profile = match (direct, generated.as_slice()) {
                (Some(profile), []) => profile,
                (None, [(profile, _)]) => profile.clone(),
                _ => continue,
            };
            let mut dependencies = match generated.as_slice() {
                [(_, components)] => component_path_features(components, &history_features)
                    .into_iter()
                    .filter_map(|native| feature_ids_by_native.get(native.as_str()).cloned())
                    .collect::<Vec<_>>(),
                _ => Vec::new(),
            };
            match &profile {
                ProfileRef::Feature(feature) => dependencies.push(feature.clone()),
                ProfileRef::Generated { curves, .. } => {
                    dependencies.extend(curves.iter().map(|curve| curve.feature.clone()));
                }
                _ => {}
            }
            projections.insert(feature.id.clone(), (profile, dependencies));
        }
    }
    for feature in features {
        let Some((profile, dependencies)) = feature
            .native_ref
            .as_ref()
            .and_then(|native| projections.remove(native))
        else {
            continue;
        };
        let FeatureDefinition::Sweep {
            profile: profile_slot,
            ..
        } = &mut feature.definition
        else {
            continue;
        };
        if profile_slot.is_some() {
            continue;
        }
        *profile_slot = Some(profile);
        for dependency in dependencies {
            if dependency != feature.id && !feature.dependencies.contains(&dependency) {
                feature.dependencies.push(dependency);
            }
        }
    }
}

pub(crate) fn compact_body_path_at(payload: &[u8], marker: usize) -> Option<Vec<u32>> {
    if marker < 12
        || payload.get(marker..marker + 16) != Some(COMPACT_EDGE_VECTOR_MARKER.as_slice())
        || payload.get(marker - 8..marker - 4) != Some(&[0, 3, 0, 0])
        || payload.get(marker + 16..marker + 18) != Some(&[0, 0])
    {
        return None;
    }
    let count = usize::try_from(u32::from_le_bytes(
        payload.get(marker - 12..marker - 8)?.try_into().ok()?,
    ))
    .ok()?;
    if count == 0 {
        return None;
    }
    compact_heterogeneous_edge_path(payload, marker + 18, count)
        .map(|(ids, _)| ids)
        .or_else(|| {
            let (ids, end) = (count > 1)
                .then(|| compact_heterogeneous_edge_path(payload, marker + 18, count - 1))
                .flatten()?;
            (payload.get(end..end + 8) == Some(&[0xff, 0xff, 0xff, 0xff, 0, 0, 0, 0]))
                .then_some(ids)
        })
}

pub(crate) fn compact_body_component_path_at(
    payload: &[u8],
    marker: usize,
) -> Option<Vec<FeatureInputComponentPathEntry>> {
    if marker < 12
        || payload.get(marker..marker + 16) != Some(COMPACT_EDGE_VECTOR_MARKER.as_slice())
        || payload.get(marker - 8..marker - 4) != Some(&[0, 3, 0, 0])
        || payload.get(marker + 16..marker + 18) != Some(&[0, 0])
    {
        return None;
    }
    let count = usize::try_from(u32::from_le_bytes(
        payload.get(marker - 12..marker - 8)?.try_into().ok()?,
    ))
    .ok()
    .filter(|count| *count != 0)?;
    compact_heterogeneous_component_path(payload, marker + 18, count)
        .map(|(components, _)| components)
        .or_else(|| {
            let (components, end) = (count > 1)
                .then(|| compact_heterogeneous_component_path(payload, marker + 18, count - 1))
                .flatten()?;
            (payload.get(end..end + 8) == Some(&[0xff, 0xff, 0xff, 0xff, 0, 0, 0, 0]))
                .then_some(components)
        })
}

pub(crate) fn project_compact_combine_paths(
    features: &mut [cadmpeg_ir::features::Feature],
    histories: &[crate::records::FeatureHistory],
    lanes: &[FeatureInputLane],
) {
    struct Projection {
        target: cadmpeg_ir::features::BodySelection,
        tools: cadmpeg_ir::features::BodySelection,
        dependencies: Vec<cadmpeg_ir::features::FeatureId>,
    }

    let feature_ids_by_native = features
        .iter()
        .filter_map(|feature| Some((feature.native_ref.clone()?, feature.id.clone())))
        .collect::<HashMap<_, _>>();
    let history_features = histories
        .iter()
        .flat_map(|history| &history.features)
        .cloned()
        .collect::<Vec<_>>();
    let mut projections = HashMap::<String, Projection>::new();
    for history_feature in &history_features {
        let (Some(target), Some(tools)) = (
            history_feature.properties.get("Target"),
            history_feature.properties.get("Tools"),
        ) else {
            continue;
        };
        let project = |native: &str| {
            let (prefix, offset) = native.rsplit_once(':')?;
            let offset = offset.parse::<usize>().ok()?;
            let lane_key = prefix.rsplit_once(':')?.1;
            let lane = lanes.iter().find(|lane| {
                lane.id
                    .rsplit_once('#')
                    .map_or(lane.id.as_str(), |(_, key)| key)
                    == lane_key
            })?;
            let components = compact_body_component_path_at(&lane.native_payload, offset)?;
            let producer = component_path_terminal_feature(&components, &history_features)?;
            let feature = feature_ids_by_native.get(&producer)?.clone();
            let local_id = components
                .iter()
                .map(|component| component.local_id.to_string())
                .collect::<Vec<_>>()
                .join(",");
            Some((
                cadmpeg_ir::features::BodySelection::Generated {
                    bodies: vec![cadmpeg_ir::features::GeneratedBodyRef {
                        feature: feature.clone(),
                        local_id,
                    }],
                    native: native.to_owned(),
                },
                components,
                feature,
            ))
        };
        let (
            Some((target, target_components, target_owner)),
            Some((tools, tool_components, tool_owner)),
        ) = (project(target), project(tools))
        else {
            continue;
        };
        let mut dependencies = target_components
            .iter()
            .chain(&tool_components)
            .filter_map(|component| {
                let native = component_path_terminal_feature(
                    std::slice::from_ref(component),
                    &history_features,
                )?;
                feature_ids_by_native.get(&native).cloned()
            })
            .collect::<Vec<_>>();
        dependencies.push(target_owner);
        dependencies.push(tool_owner);
        dependencies.sort_by_key(|dependency| {
            features
                .iter()
                .find(|feature| feature.id == *dependency)
                .map_or(u64::MAX, |feature| feature.ordinal)
        });
        dependencies.dedup();
        projections.insert(
            history_feature.id.clone(),
            Projection {
                target,
                tools,
                dependencies,
            },
        );
    }
    for feature in features {
        let Some(projection) = feature
            .native_ref
            .as_ref()
            .and_then(|native| projections.remove(native))
        else {
            continue;
        };
        let FeatureDefinition::Combine { target, tools, .. } = &mut feature.definition else {
            continue;
        };
        *target = projection.target;
        *tools = projection.tools;
        for dependency in projection.dependencies {
            if dependency != feature.id && !feature.dependencies.contains(&dependency) {
                feature.dependencies.push(dependency);
            }
        }
    }
}

pub(crate) fn compact_extrusion_through_all_at(payload: &[u8], offset: usize) -> bool {
    compact_extrusion_end_spec_header(payload, offset, 1)
        && compact_extrusion_traversal_tail_at(payload, offset)
}

pub(crate) fn compact_extrusion_through_next_at(payload: &[u8], offset: usize) -> bool {
    compact_extrusion_end_spec_header(payload, offset, 2)
        && compact_extrusion_traversal_tail_at(payload, offset)
}

/// Through-all in both directions. Two carriers exist: a first-direction
/// traversal code `1` with second-direction code `1` and the shared traversal
/// tail, and the dedicated code `9` whose second-direction word is `1` and
/// whose retained blind dimension child follows immediately.
pub(crate) fn compact_extrusion_through_all_both_at(payload: &[u8], offset: usize) -> bool {
    (compact_extrusion_two_direction_header(payload, offset, 1)
        && payload.get(offset + 26..offset + 30) == Some(&[0, 0, 0, 0])
        && compact_extrusion_traversal_body_at(payload, offset))
        || (compact_extrusion_two_direction_header(payload, offset, 9)
            && compact_extrusion_dimension_child_at(payload, offset + 26).is_some())
}

/// Blind first direction with a through-all second direction: a code `0`
/// header whose second-direction word is `1`, owning the blind dimension
/// child.
pub(crate) fn compact_extrusion_blind_through_all_second_at(payload: &[u8], offset: usize) -> bool {
    compact_end_spec_identity_at(payload, offset)
        && payload.get(offset + 2..offset + 12) == Some(&[0, 0, 1, 0, 0, 0, 0, 0, 0, 0])
        && payload
            .get(offset + 12..offset + 16)
            .and_then(|bytes| bytes.try_into().ok())
            .map(u32::from_le_bytes)
            .is_some_and(|flag| flag <= 1)
        && payload.get(offset + 16..offset + 22) == Some(&[0, 0, 0, 0, 0, 0])
        && payload.get(offset + 22..offset + 26) == Some(&[1, 0, 0, 0])
        && compact_extrusion_dimension_child_at(payload, offset + 26).is_some()
}

/// Two-direction end-spec header: the words at `+4` and `+8` carry `0` or
/// `1`, the first-direction code sits at `+18`, and the second-direction
/// code `1` sits at `+22`.
fn compact_extrusion_two_direction_header(payload: &[u8], offset: usize, code: u32) -> bool {
    compact_end_spec_identity_at(payload, offset)
        && payload.get(offset + 2..offset + 4) == Some(&[0, 0])
        && payload
            .get(offset + 4..offset + 8)
            .and_then(|bytes| bytes.try_into().ok())
            .map(u32::from_le_bytes)
            .is_some_and(|word| word <= 1)
        && payload
            .get(offset + 8..offset + 12)
            .and_then(|bytes| bytes.try_into().ok())
            .map(u32::from_le_bytes)
            .is_some_and(|word| word <= 1)
        && payload
            .get(offset + 12..offset + 16)
            .and_then(|bytes| bytes.try_into().ok())
            .map(u32::from_le_bytes)
            .is_some_and(|flag| flag <= 1)
        && payload.get(offset + 16..offset + 18) == Some(&[0, 0])
        && payload.get(offset + 18..offset + 22) == Some(code.to_le_bytes().as_slice())
        && payload.get(offset + 22..offset + 26) == Some(&[1, 0, 0, 0])
}

fn compact_extrusion_traversal_tail_at(payload: &[u8], offset: usize) -> bool {
    payload.get(offset + 22..offset + 30) == Some(&[0, 0, 0, 0, 0, 0, 0, 0])
        && compact_extrusion_traversal_body_at(payload, offset)
}

/// Shared traversal run from `+30`: the `[1, 0, 0, 1]` marker and the fixed
/// zero fill through the `+90` word.
fn compact_extrusion_traversal_body_at(payload: &[u8], offset: usize) -> bool {
    payload.get(offset + 30..offset + 34) == Some(&[1, 0, 0, 1])
        && payload
            .get(offset + 34..offset + 90)
            .is_some_and(|bytes| bytes.iter().all(|byte| *byte == 0))
        && payload.get(offset + 90..offset + 94) == Some(&[0, 0, 1, 0])
        && payload
            .get(offset + 94..offset + 100)
            .is_some_and(|bytes| bytes.iter().all(|byte| *byte == 0))
        && payload
            .get(offset + 100..offset + 102)
            .is_some_and(|bytes| bytes == [0, 0] || bytes[1] & 0x80 != 0)
        && payload.get(offset + 102..offset + 104) == Some(&[0, 0])
}

pub(crate) fn compact_extrusion_mid_plane_at(payload: &[u8], offset: usize) -> bool {
    compact_extrusion_end_spec_header(payload, offset, 6)
        && payload.get(offset + 22..offset + 26) == Some(&[0, 0, 0, 0])
        && compact_extrusion_dimension_child_at(payload, offset + 26).is_some()
}

/// Validate the owned dimension child at `child` and return the offset just
/// past its fixed tail.
fn compact_extrusion_dimension_child_at(payload: &[u8], child: usize) -> Option<usize> {
    let declaration = b"\xff\xff\x01\x00\x16\x00moDisplayDistanceDim_c";
    let block = if payload.get(child..child + declaration.len()) == Some(declaration) {
        child + declaration.len()
    } else if payload
        .get(child + 1)
        .is_some_and(|byte| byte & 0x80 != 0 && *byte != 0xff)
    {
        child + 2
    } else {
        return None;
    };
    (payload.get(block..block + 16).is_some_and(|bytes| {
        bytes
            .iter()
            .enumerate()
            .all(|(index, byte)| *byte == 0 || (index == 9 && byte.trailing_zeros() >= 3))
    }) && payload.get(block + 16..block + 20) == Some(&[0xff, 0xff, 0, 0])
        && payload
            .get(block + 20)
            .is_some_and(|byte| *byte == 1 || *byte == 3)
        && payload.get(block + 21..block + 25) == Some(&[0xff, 0xff, 0xff, 0xff])
        && payload.get(block + 25..block + 31) == Some(&[0, 0, 0, 0, 0, 0])
        && payload.get(block + 31..block + 33) == Some(&[0x80, 0xbf]))
    .then_some(block + 33)
}

/// Form of the point reference owned by an up-to-vertex end spec.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CompactPointReferenceKind {
    /// Direct vertex reference; the final path entry's component id is the
    /// feature-local vertex id.
    Point,
    /// Edge endpoint reference; the path selects an edge and the endpoint
    /// selector stays native.
    EdgeEndpoint,
}

pub(crate) fn compact_extrusion_to_vertex_at(
    payload: &[u8],
    offset: usize,
) -> Option<(usize, CompactPointReferenceKind)> {
    if !compact_extrusion_end_spec_header(payload, offset, 3)
        || payload.get(offset + 22..offset + 30) != Some(&[0, 0, 0, 0, 0, 0, 0, 0])
    {
        return None;
    }
    let child = offset + 30;
    let point_declaration = b"\xff\xff\x01\x00\x0c\x00moPointRef_w";
    let endpoint_declaration = b"\xff\xff\x01\x00\x0f\x00moEndPointRef_w";
    let point_body_at = |body: usize| {
        payload.get(body + 1).is_some_and(|byte| byte & 0x80 != 0)
            && payload
                .get(body + 2..body + 4)
                .is_some_and(|bytes| bytes == [0xa9, 0x80] || bytes == [0x2b, 0x80])
            && payload.get(body + 4..body + 9) == Some(&[2, 0, 0, 0, 0])
    };
    let kind = if (payload.get(child..child + point_declaration.len()) == Some(point_declaration)
        && point_body_at(child + point_declaration.len()))
        || point_body_at(child)
    {
        CompactPointReferenceKind::Point
    } else if payload.get(child..child + endpoint_declaration.len()) == Some(endpoint_declaration) {
        let edge_declaration = b"\xff\xff\x01\x00\x0c\x00moCompEdge_c";
        let inner = child + endpoint_declaration.len();
        let body = inner + edge_declaration.len();
        if payload.get(inner..inner + edge_declaration.len()) != Some(edge_declaration)
            || payload.get(body + 1).is_none_or(|byte| byte & 0x80 == 0)
            || payload.get(body + 2..body + 7) != Some(&[2, 0, 0, 0, 0x40])
        {
            return None;
        }
        CompactPointReferenceKind::EdgeEndpoint
    } else {
        return None;
    };
    (child..child.saturating_add(240))
        .find(|marker| compact_termination_reference_at(payload, *marker))
        .map(|marker| (marker, kind))
}

pub(crate) fn compact_extrusion_offset_from_face_at(
    payload: &[u8],
    offset: usize,
    end: usize,
) -> Option<usize> {
    if !compact_extrusion_end_spec_header(payload, offset, 5)
        || payload.get(offset + 22..offset + 26) != Some(&[0, 0, 0, 0])
    {
        return None;
    }
    let resume = compact_extrusion_dimension_child_at(payload, offset + 26)?;
    let declaration = b"\xff\xff\x01\x00\x11\x00moSingleFaceRef_w";
    let end = end.min(payload.len());
    for anchor in resume..end.saturating_sub(3) {
        if payload.get(anchor..anchor + 3) != Some(&[1, 1, 0]) {
            continue;
        }
        let child = anchor + 3;
        let body = if payload.get(child..child + declaration.len()) == Some(declaration) {
            child + declaration.len()
        } else {
            child
        };
        // The reference body opens with lane tokens followed by the selector.
        let Some(open) = (body + 2..body + 9).find(|cursor| {
            payload.get(cursor - 1).is_some_and(|byte| byte & 0x80 != 0)
                && payload.get(*cursor..cursor + 7) == Some(&[2, 0, 0, 0, 0x40, 0, 0])
        }) else {
            continue;
        };
        if let Some(marker) = (open..open.saturating_add(200))
            .find(|marker| compact_termination_reference_at(payload, *marker))
        {
            return Some(marker);
        }
    }
    None
}

pub(crate) fn compact_extrusion_to_face_at(payload: &[u8], offset: usize) -> Option<usize> {
    if !compact_extrusion_end_spec_header(payload, offset, 4)
        || payload
            .get(offset + 22..offset + 26)
            .and_then(|bytes| bytes.try_into().ok())
            .map(u32::from_le_bytes)
            .is_none_or(|flag| flag > 1)
        || payload.get(offset + 26..offset + 30) != Some(&[0, 0, 0, 0])
        || payload.get(offset + 30..offset + 33) != Some(&[1, 1, 0])
    {
        return None;
    }
    let declaration = b"\xff\xff\x01\x00\x11\x00moSingleFaceRef_w";
    let body = b"\x2d\x80\x2b\x80\x02\x00\x00\x00\x40\x00\x00";
    let child = offset + 33;
    let body_offset = if payload.get(child..child + declaration.len()) == Some(declaration) {
        child + declaration.len()
    } else if payload.get(child + 2..child + 2 + body.len()) == Some(body) {
        child + 2
    } else {
        return None;
    };
    if payload.get(body_offset..body_offset + body.len()) != Some(body) {
        return None;
    }
    (body_offset..body_offset.saturating_add(160))
        .find(|marker| compact_single_face_reference_at(payload, *marker))
}

/// End-spec children carry their class at the anchor: either a lane-scoped
/// class token or a direct `moEndSpec_c` declaration ending at the anchor.
/// Header-shaped runs without this identity belong to fillet edge-set records.
fn compact_end_spec_identity_at(payload: &[u8], offset: usize) -> bool {
    payload
        .get(offset..offset + 2)
        .is_some_and(|bytes| bytes[1] & 0x80 != 0 && bytes != [0xff, 0xff])
        || offset
            .checked_sub(15)
            .and_then(|start| payload.get(start..offset + 2))
            == Some(b"\xff\xff\x01\x00\x0b\x00moEndSpec_c".as_slice())
}

fn compact_extrusion_end_spec_header(payload: &[u8], offset: usize, code: u32) -> bool {
    compact_end_spec_identity_at(payload, offset)
        && payload.get(offset + 2..offset + 12) == Some(&[0, 0, 1, 0, 0, 0, 0, 0, 0, 0])
        && payload
            .get(offset + 12..offset + 16)
            .and_then(|bytes| bytes.try_into().ok())
            .map(u32::from_le_bytes)
            .is_some_and(|flag| flag <= 1)
        && payload.get(offset + 16..offset + 18) == Some(&[0, 0])
        && payload.get(offset + 18..offset + 22) == Some(code.to_le_bytes().as_slice())
}

fn compact_single_face_reference_at(payload: &[u8], marker: usize) -> bool {
    compact_single_face_reference_path_at(payload, marker).is_some()
}

fn compact_single_face_reference_path_at(
    payload: &[u8],
    marker: usize,
) -> Option<Vec<FeatureInputComponentPathEntry>> {
    let count = marker.checked_sub(12).and_then(|offset| {
        Some(u32::from_le_bytes(
            payload.get(offset..offset + 4)?.try_into().ok()?,
        ))
    })?;
    let count = usize::try_from(count)
        .ok()
        .filter(|count| (1..=64).contains(count))?;
    if payload.get(marker..marker + 16) != Some(COMPACT_EDGE_VECTOR_MARKER.as_slice())
        || payload.get(marker - 8..marker - 4) != Some(&[0, 2, 0, 0])
        || payload.get(marker + 16..marker + 18) != Some(&[0, 0])
    {
        return None;
    }
    compact_heterogeneous_component_path(payload, marker + 18, count)
        .map(|(components, _)| components)
        .or_else(|| {
            let (components, end) = (count > 1)
                .then(|| compact_heterogeneous_component_path(payload, marker + 18, count - 1))
                .flatten()?;
            (payload.get(end..end + 8) == Some(&[0xff, 0xff, 0xff, 0xff, 0, 0, 0, 0]))
                .then_some(components)
        })
}

fn compact_termination_reference_at(payload: &[u8], marker: usize) -> bool {
    compact_termination_reference_path_at(payload, marker).is_some()
}

/// Decode the component path of an up-to-vertex or offset-from-face
/// termination reference. These vectors share the single-face-reference
/// grammar and may additionally carry a leading identifier-less component
/// cell, `a0 86 01 00` filler words, or an `01 00 00 00` slot word between
/// counted entries.
pub(crate) fn compact_termination_reference_path_at(
    payload: &[u8],
    marker: usize,
) -> Option<Vec<FeatureInputComponentPathEntry>> {
    if let Some(components) = compact_single_face_reference_path_at(payload, marker) {
        return Some(components);
    }
    let count = marker.checked_sub(12).and_then(|offset| {
        Some(u32::from_le_bytes(
            payload.get(offset..offset + 4)?.try_into().ok()?,
        ))
    })?;
    let count = usize::try_from(count)
        .ok()
        .filter(|count| (1..=64).contains(count))?;
    if payload.get(marker..marker + 16) != Some(COMPACT_EDGE_VECTOR_MARKER.as_slice())
        || payload.get(marker - 8..marker - 4) != Some(&[0, 2, 0, 0])
        || payload.get(marker + 16..marker + 18) != Some(&[0, 0])
    {
        return None;
    }
    let entry_at = |offset: usize| -> Option<FeatureInputComponentPathEntry> {
        let instance = payload.get(offset..offset + 4)?;
        if instance[0..2] == [0, 0]
            || instance[0..2] == [0xff, 0xff]
            || instance[2..4] != [0, 0]
            || payload.get(offset + 4..offset + 6)? == [0, 0]
        {
            return None;
        }
        Some(FeatureInputComponentPathEntry {
            instance: u16::from_le_bytes(instance[0..2].try_into().ok()?),
            type_signature: payload.get(offset + 4..offset + 16)?.try_into().ok()?,
            local_id: u32::from_le_bytes(payload.get(offset + 16..offset + 20)?.try_into().ok()?),
        })
    };
    let mut cursor = marker + 18;
    // A leading identifier-less cell repeats the first counted entry's
    // signature immediately after its own.
    if entry_at(cursor).is_some()
        && entry_at(cursor + 16).is_some()
        && payload.get(cursor + 20..cursor + 32) == payload.get(cursor + 4..cursor + 16)
    {
        cursor += 16;
    }
    let mut entries = Vec::new();
    while entries.len() < count {
        if let Some(entry) = entry_at(cursor) {
            entries.push(entry);
            cursor += 20;
            continue;
        }
        let gap = [4usize, 8].into_iter().find(|gap| {
            let filler_ok = match gap {
                4 => matches!(
                    payload.get(cursor..cursor + 4),
                    Some([0, 0, 0, 0] | [0xa0, 0x86, 0x01, 0x00])
                ),
                8 => matches!(
                    payload.get(cursor..cursor + 8),
                    Some(
                        [0, 0, 0, 0, 0, 0, 0, 0]
                            | [0xff, 0xff, 0xff, 0xff, 0, 0, 0, 0]
                            | [0xa0, 0x86, 0x01, 0x00, 0, 0, 0, 0]
                            | [0x01, 0x00, 0x00, 0x00, 0, 0, 0, 0]
                    )
                ),
                _ => false,
            };
            filler_ok && entry_at(cursor + gap).is_some()
        });
        match gap {
            Some(gap) => cursor += gap,
            None => break,
        }
    }
    (!entries.is_empty()).then_some(entries)
}

pub(crate) fn compact_surface_selection_value(
    components: &[FeatureInputComponentPathEntry],
) -> String {
    let mut value = String::from("sldprt:feature-input:surface-component-ids:");
    for (index, component) in components.iter().enumerate() {
        if index != 0 {
            value.push(',');
        }
        write!(&mut value, "{}", component.local_id).expect("writing to String cannot fail");
    }
    value
}

pub(crate) fn component_path_features(
    components: &[FeatureInputComponentPathEntry],
    features: &[crate::records::Feature],
) -> Vec<String> {
    let mut by_source = HashMap::<u32, Option<&str>>::new();
    for feature in features {
        let Some(source_id) = feature
            .source_id
            .as_deref()
            .and_then(|id| id.parse::<u32>().ok())
        else {
            continue;
        };
        by_source
            .entry(source_id)
            .and_modify(|candidate| *candidate = None)
            .or_insert(Some(feature.id.as_str()));
    }
    let mut result = Vec::new();
    for component in components {
        let mut source_id = [0; 4];
        source_id.copy_from_slice(&component.type_signature[4..8]);
        let source_id = u32::from_le_bytes(source_id);
        if let Some(Some(feature)) = by_source.get(&source_id) {
            if !result.iter().any(|existing| existing == feature) {
                result.push((*feature).to_string());
            }
        }
    }
    result
}

pub(crate) fn component_path_terminal_feature(
    components: &[FeatureInputComponentPathEntry],
    features: &[crate::records::Feature],
) -> Option<String> {
    let component = components.last()?;
    let mut source_id = [0; 4];
    source_id.copy_from_slice(&component.type_signature[4..8]);
    let source_id = u32::from_le_bytes(source_id);
    let mut candidates = features.iter().filter(|feature| {
        feature
            .source_id
            .as_deref()
            .and_then(|id| id.parse::<u32>().ok())
            == Some(source_id)
    });
    let feature = candidates.next()?;
    candidates.next().is_none().then(|| feature.id.clone())
}

pub(crate) fn project_adjacent_extrusion_profiles(
    features: &mut [cadmpeg_ir::features::Feature],
    histories: &[crate::records::FeatureHistory],
    lanes: &[FeatureInputLane],
) {
    #[derive(PartialEq)]
    enum ProfileVote {
        Missing,
        Unique(String),
        Ambiguous,
    }

    let native_features = histories
        .iter()
        .flat_map(|history| &history.features)
        .map(|feature| (feature.id.as_str(), feature))
        .collect::<HashMap<_, _>>();
    let neutral_indices = features
        .iter()
        .enumerate()
        .filter_map(|(index, feature)| Some((feature.native_ref.clone()?, index)))
        .collect::<HashMap<_, _>>();
    let mut profiles = HashMap::<String, Vec<ProfileVote>>::new();
    for lane in lanes {
        let mut objects = native_features
            .values()
            .filter_map(|feature| Some((feature_object_name(feature, lane)?, *feature)))
            .collect::<Vec<_>>();
        objects.sort_by_key(|(name, _)| name.offset);
        let object_kind = |name: &FeatureInputName, feature: &crate::records::Feature| {
            let kind = native_object_class(feature.input_class.as_deref().unwrap_or_default()).kind;
            if kind == NativeClassKind::Unknown
                && feature_inline_operation_fields(lane, name).is_some()
            {
                NativeClassKind::Extrusion
            } else {
                kind
            }
        };
        for (name, feature) in &objects {
            if object_kind(name, feature) == NativeClassKind::Extrusion
                && !feature.properties.contains_key("DissectableChildren")
            {
                profiles
                    .entry(feature.id.clone())
                    .or_default()
                    .push(ProfileVote::Missing);
            }
        }
        for pair in objects.windows(2) {
            let [(first_name, first), (second_name, second)] = pair else {
                continue;
            };
            let first_kind = object_kind(first_name, first);
            let second_kind = object_kind(second_name, second);
            let (profile, extrusion) = match (first_kind, second_kind) {
                (NativeClassKind::ProfileFeature, NativeClassKind::Extrusion) => (*first, *second),
                (NativeClassKind::Extrusion, NativeClassKind::ProfileFeature) => (*second, *first),
                _ => continue,
            };
            if extrusion.properties.contains_key("DissectableChildren") {
                continue;
            }
            let Some(vote) = profiles
                .get_mut(&extrusion.id)
                .and_then(|votes| votes.last_mut())
            else {
                continue;
            };
            *vote = match vote {
                ProfileVote::Missing => ProfileVote::Unique(profile.id.clone()),
                ProfileVote::Unique(existing) if existing == &profile.id => {
                    ProfileVote::Unique(existing.clone())
                }
                ProfileVote::Unique(_) | ProfileVote::Ambiguous => ProfileVote::Ambiguous,
            };
        }
    }
    for (extrusion, votes) in profiles {
        let Some(ProfileVote::Unique(profile)) = votes.first() else {
            continue;
        };
        if !votes
            .iter()
            .all(|vote| matches!(vote, ProfileVote::Unique(candidate) if candidate == profile))
        {
            continue;
        }
        let Some(&index) = neutral_indices.get(&extrusion) else {
            continue;
        };
        let FeatureDefinition::Extrude {
            profile: neutral_profile,
            ..
        } = &mut features[index].definition
        else {
            continue;
        };
        if !matches!(neutral_profile, cadmpeg_ir::features::ProfileRef::Unresolved(owner) if owner == &extrusion)
        {
            continue;
        }
        *neutral_profile = cadmpeg_ir::features::ProfileRef::Native(profile.clone());
        if let Some(&profile_index) = neutral_indices.get(profile) {
            let dependency = features[profile_index].id.clone();
            if !features[index].dependencies.contains(&dependency) {
                features[index].dependencies.push(dependency);
            }
        }
    }
}

pub(crate) fn compact_edge_selection_value(local_edge_ids: &[u32]) -> String {
    let mut value = String::from("sldprt:feature-input:edge-ids:");
    for (index, edge_id) in local_edge_ids.iter().enumerate() {
        if index != 0 {
            value.push(',');
        }
        write!(&mut value, "{edge_id}").expect("writing to String cannot fail");
    }
    value
}

pub(crate) fn compact_edge_selection_set_value(
    selections: &[&FeatureInputEdgeSelection],
) -> String {
    if let [selection] = selections {
        return compact_edge_selection_value(&selection.local_edge_ids);
    }
    let mut value = String::from("sldprt:feature-input:edge-selection-vectors:");
    for (selection_index, selection) in selections.iter().enumerate() {
        if selection_index != 0 {
            value.push(';');
        }
        for (id_index, id) in selection.local_edge_ids.iter().enumerate() {
            if id_index != 0 {
                value.push(',');
            }
            write!(&mut value, "{id}").expect("writing to String cannot fail");
        }
    }
    value
}

pub(crate) fn compact_body_selection_value(local_body_ids: &[u32]) -> String {
    let mut value = String::from("sldprt:feature-input:body-ids:");
    for (index, body_id) in local_body_ids.iter().enumerate() {
        if index != 0 {
            value.push(',');
        }
        write!(&mut value, "{body_id}").expect("writing to String cannot fail");
    }
    value
}

pub(crate) fn is_compact_body_selection_value(value: &str) -> bool {
    value.starts_with("sldprt:feature-input:body-ids:")
}

/// Materialize dimensioned circular sketch geometry omitted by a selected-profile stream.
pub(crate) fn project_dimensioned_sketch_geometry(
    entities: &mut Vec<SketchEntity>,
    sketches: &[cadmpeg_ir::sketches::Sketch],
    surfaces: &[cadmpeg_ir::geometry::Surface],
    features: &[cadmpeg_ir::features::Feature],
    parameters: &[cadmpeg_ir::features::DesignParameter],
    lanes: &[FeatureInputLane],
) {
    const NATIVE_TO_IR: f64 = 1000.0;
    const QUANTUM: f64 = 1.0e-8;

    let sketches_by_feature = features
        .iter()
        .filter_map(|feature| {
            let cadmpeg_ir::features::FeatureDefinition::Sketch {
                sketch: Some(sketch),
                ..
            } = &feature.definition
            else {
                return None;
            };
            Some((feature.native_ref.as_deref()?, sketch.clone()))
        })
        .collect::<HashMap<_, _>>();
    let parameters_by_scalar = parameters
        .iter()
        .filter_map(|parameter| Some((parameter.native_ref.as_deref()?, parameter)))
        .collect::<HashMap<_, _>>();
    let markers_by_id = lanes
        .iter()
        .flat_map(|lane| &lane.sketch_entities)
        .map(|marker| (marker.id.as_str(), marker))
        .collect::<HashMap<_, _>>();
    let marker_transforms =
        marker_transform_candidates_by_feature(features, sketches, entities, lanes);
    let transforms = sketches_by_feature
        .iter()
        .filter_map(|(feature, sketch_id)| {
            let circles = lanes
                .iter()
                .flat_map(|lane| &lane.relation_instances)
                .filter(|relation| {
                    relation.feature_ref == *feature
                        && relation.family == FeatureInputRelationFamily::CircleDiameter
                })
                .filter_map(|relation| {
                    let ([operand] | [_, operand]) = relation.operands.as_slice() else {
                        return None;
                    };
                    let explicit = operand
                        .entity_ref
                        .as_deref()
                        .and_then(|id| markers_by_id.get(id).copied());
                    let implicit = explicit.is_none().then(|| {
                        implicit_circle_marker(
                            lanes,
                            relation.feature_ref.as_str(),
                            operand.kind,
                            operand.entity_index,
                        )
                    });
                    let (marker, encoded_radius) = match (explicit, implicit.flatten()) {
                        (Some(marker), _) => (marker, None),
                        (None, Some((marker, radius))) => (marker, Some(radius)),
                        (None, None) => return None,
                    };
                    if !matches!(
                        marker.kind,
                        SketchInputKind::Point
                            | SketchInputKind::ConstrainedPoint
                            | SketchInputKind::LineOrCircle
                    ) {
                        return None;
                    }
                    let [u, v] = marker.coordinates_m?;
                    let parameter = relation
                        .parameter_scalar_ref
                        .as_deref()
                        .and_then(|id| parameters_by_scalar.get(id).copied())
                        .or_else(|| {
                            relation.parameter_scalar_ref.is_none().then_some(())?;
                            let lane = lanes.iter().find(|lane| {
                                lane.relation_instances
                                    .iter()
                                    .any(|candidate| candidate.id == relation.id)
                            })?;
                            relation_parameter_by_display_name(relation, lane, features, parameters)
                        })?;
                    let cadmpeg_ir::features::ParameterValue::Length(value) =
                        parameter.value.as_ref()?
                    else {
                        return None;
                    };
                    let radius = match parameter.display {
                        Some(cadmpeg_ir::features::DimensionDisplay::Radius) => value.0,
                        Some(cadmpeg_ir::features::DimensionDisplay::Diameter) => value.0 * 0.5,
                        None => return None,
                    };
                    if !(radius.is_finite() && radius > 0.0) {
                        return None;
                    }
                    if encoded_radius.is_some_and(|encoded| !same_dimension_length(encoded, radius))
                    {
                        return None;
                    }
                    Some((
                        quantize(Point2::new(u * NATIVE_TO_IR, v * NATIVE_TO_IR), QUANTUM),
                        (radius / QUANTUM).round() as i64,
                    ))
                })
                .collect::<Vec<_>>();
            let candidates = marker_transforms.get(*feature).cloned().unwrap_or_else(|| {
                sketches
                    .iter()
                    .find(|sketch| sketch.id == *sketch_id)
                    .map_or_else(Vec::new, |sketch| {
                        dimensioned_circle_surface_transforms(sketch, surfaces, &circles, QUANTUM)
                    })
            });
            let candidates = sketches
                .iter()
                .find(|sketch| sketch.id == *sketch_id)
                .map_or(candidates.clone(), |sketch| {
                    select_marker_transforms_by_frame(&candidates, sketch, QUANTUM)
                });
            dimensioned_circle_transform(&candidates, &circles)
                .map(|transform| ((*feature).to_string(), transform))
        })
        .collect::<HashMap<_, _>>();
    for lane in lanes {
        let lane_key = lane
            .id
            .rsplit_once('#')
            .map_or(lane.id.as_str(), |(_, key)| key);
        for relation in &lane.relation_instances {
            if relation.family != FeatureInputRelationFamily::CircleDiameter {
                continue;
            }
            let (Some(sketch), Some(transform)) = (
                sketches_by_feature.get(relation.feature_ref.as_str()),
                transforms.get(relation.feature_ref.as_str()),
            ) else {
                continue;
            };
            let ([operand] | [_, operand]) = relation.operands.as_slice() else {
                continue;
            };
            let explicit_marker = operand
                .entity_ref
                .as_deref()
                .and_then(|id| markers_by_id.get(id).copied());
            let implicit_marker = explicit_marker.is_none().then(|| {
                implicit_circle_marker(
                    lanes,
                    relation.feature_ref.as_str(),
                    operand.kind,
                    operand.entity_index,
                )
            });
            let (marker, encoded_radius) = match (explicit_marker, implicit_marker.flatten()) {
                (Some(marker), _) => (marker, None),
                (None, Some((marker, radius))) => (marker, Some(radius)),
                (None, None) => continue,
            };
            if !matches!(
                marker.kind,
                SketchInputKind::Point
                    | SketchInputKind::ConstrainedPoint
                    | SketchInputKind::LineOrCircle
            ) {
                continue;
            }
            let parameter = relation
                .parameter_scalar_ref
                .as_deref()
                .and_then(|id| parameters_by_scalar.get(id).copied())
                .or_else(|| {
                    relation.parameter_scalar_ref.is_none().then_some(())?;
                    relation_parameter_by_display_name(relation, lane, features, parameters)
                });
            let (Some([u, v]), Some(parameter)) = (marker.coordinates_m, parameter) else {
                continue;
            };
            let Some(cadmpeg_ir::features::ParameterValue::Length(value)) =
                parameter.value.as_ref()
            else {
                continue;
            };
            let radius = match parameter.display {
                Some(cadmpeg_ir::features::DimensionDisplay::Radius) => value.0,
                Some(cadmpeg_ir::features::DimensionDisplay::Diameter) => value.0 * 0.5,
                None => continue,
            };
            if !(radius.is_finite() && radius > 0.0) {
                continue;
            }
            if encoded_radius.is_some_and(|encoded| !same_dimension_length(encoded, radius)) {
                continue;
            }
            let native = quantize(Point2::new(u * NATIVE_TO_IR, v * NATIVE_TO_IR), QUANTUM);
            let Some(center) = transform.apply(native) else {
                continue;
            };
            let center = Point2::new(center.0 as f64 * QUANTUM, center.1 as f64 * QUANTUM);
            if entities.iter().any(|entity| {
                entity.sketch == *sketch
                    && match &entity.geometry {
                        SketchGeometry::Circle {
                            center: existing,
                            radius: existing_radius,
                        } => {
                            quantize(*existing, QUANTUM) == quantize(center, QUANTUM)
                                && same_dimension_length(existing_radius.0, radius)
                        }
                        _ => false,
                    }
            }) {
                continue;
            }
            entities.push(SketchEntity {
                id: SketchEntityId(format!(
                    "sldprt:model:sketch-entity#dimension:{lane_key}:{}",
                    relation.offset
                )),
                sketch: sketch.clone(),
                construction: false,
                native_ref: Some(marker.id.clone()),
                geometry_ref: Some(relation.id.clone()),
                endpoint_refs: Vec::new(),
                geometry: SketchGeometry::Circle {
                    center,
                    radius: cadmpeg_ir::features::Length(radius),
                },
            });
        }
    }
}

/// Materialize relation-addressed point geometry omitted from selected profile streams.
pub(crate) fn project_relation_point_geometry(
    entities: &mut Vec<SketchEntity>,
    sketches: &[cadmpeg_ir::sketches::Sketch],
    features: &[cadmpeg_ir::features::Feature],
    lanes: &[FeatureInputLane],
) {
    const NATIVE_TO_IR: f64 = 1000.0;
    const QUANTUM: f64 = 1.0e-8;

    let sketches_by_feature = features
        .iter()
        .filter_map(|feature| {
            let cadmpeg_ir::features::FeatureDefinition::Sketch {
                sketch: Some(sketch),
                ..
            } = &feature.definition
            else {
                return None;
            };
            Some((feature.native_ref.as_deref()?, sketch.clone()))
        })
        .collect::<HashMap<_, _>>();
    let transforms = marker_transform_candidates_by_feature(features, sketches, entities, lanes);
    let markers_by_id = lanes
        .iter()
        .flat_map(|lane| &lane.sketch_entities)
        .map(|marker| (marker.id.as_str(), marker))
        .collect::<HashMap<_, _>>();
    let point_operands = lanes
        .iter()
        .flat_map(|lane| &lane.relation_instances)
        .flat_map(|relation| {
            let count = match relation.family {
                FeatureInputRelationFamily::PointPointDistance
                | FeatureInputRelationFamily::PointPointHorizontalDistance
                | FeatureInputRelationFamily::PointPointVerticalDistance => 2,
                FeatureInputRelationFamily::PointLineDistance => 1,
                _ => 0,
            };
            relation
                .operands
                .iter()
                .take(count)
                .filter_map(|operand| operand.entity_ref.as_deref())
        })
        .collect::<HashSet<_>>();
    let mut referenced = lanes
        .iter()
        .flat_map(|lane| {
            lane.relation_instances
                .iter()
                .flat_map(|relation| &relation.operands)
                .filter_map(|operand| operand.entity_ref.as_deref())
                .chain(
                    lane.sketch_entities
                        .iter()
                        .filter(|marker| matches!(marker.kind, SketchInputKind::Relation(_)))
                        .map(|marker| marker.id.as_str()),
                )
        })
        .collect::<HashSet<_>>();
    loop {
        let mut linked = Vec::new();
        for marker in markers_by_id.values().copied() {
            let marker_referenced = referenced.contains(marker.id.as_str());
            for link in &marker.links {
                let adjacent = if marker_referenced {
                    Some(link.entity_ref.as_str())
                } else if referenced.contains(link.entity_ref.as_str()) {
                    Some(marker.id.as_str())
                } else {
                    None
                };
                if let Some(id) = adjacent.filter(|id| !referenced.contains(id)) {
                    linked.push(id);
                }
            }
        }
        if linked.is_empty() {
            break;
        }
        referenced.extend(linked);
    }
    for lane in lanes {
        let lane_key = lane
            .id
            .rsplit_once('#')
            .map_or(lane.id.as_str(), |(_, key)| key);
        for marker in &lane.sketch_entities {
            let qualified_point = point_operands.contains(marker.id.as_str());
            if !referenced.contains(marker.id.as_str())
                || !(qualified_point
                    && matches!(
                        marker.kind,
                        SketchInputKind::Point
                            | SketchInputKind::ConstrainedPoint
                            | SketchInputKind::LineOrCircle
                            | SketchInputKind::Arc
                    )
                    || matches!(
                        marker.kind,
                        SketchInputKind::Point | SketchInputKind::ConstrainedPoint
                    ))
                || entities.iter().any(|entity| {
                    entity.native_ref.as_deref() == Some(marker.id.as_str())
                        || entity
                            .endpoint_refs
                            .iter()
                            .any(|reference| reference == &marker.id)
                })
            {
                continue;
            }
            let (Some(feature), Some([u, v])) =
                (marker.feature_ref.as_deref(), marker.coordinates_m)
            else {
                continue;
            };
            let Some(sketch) = sketches_by_feature.get(feature) else {
                continue;
            };
            if sketch.0.contains("sketch#compact:")
                && !marker_is_geometry_locus(&lane.native_payload, marker.offset as usize)
                && !entities.iter().any(|entity| {
                    entity
                        .endpoint_refs
                        .iter()
                        .any(|reference| reference == &marker.id)
                })
            {
                continue;
            }
            let native = quantize(Point2::new(u * NATIVE_TO_IR, v * NATIVE_TO_IR), QUANTUM);
            let positions = transforms
                .get(feature)
                .into_iter()
                .flatten()
                .filter_map(|transform| transform.apply(native))
                .collect::<HashSet<_>>();
            if positions.len() != 1 {
                continue;
            }
            let position = positions
                .into_iter()
                .next()
                .expect("one transformed position");
            let position = Point2::new(position.0 as f64 * QUANTUM, position.1 as f64 * QUANTUM);
            entities.push(SketchEntity {
                id: SketchEntityId(format!(
                    "sldprt:model:sketch-entity#relation-point:{lane_key}:{}",
                    marker.offset
                )),
                sketch: sketch.clone(),
                construction: true,
                native_ref: matches!(
                    marker.kind,
                    SketchInputKind::Point | SketchInputKind::ConstrainedPoint
                )
                .then(|| marker.id.clone()),
                geometry_ref: qualified_point.then(|| marker.id.clone()).filter(|_| {
                    matches!(
                        marker.kind,
                        SketchInputKind::LineOrCircle | SketchInputKind::Arc
                    )
                }),
                endpoint_refs: Vec::new(),
                geometry: SketchGeometry::Point { position },
            });
        }
        let markers_by_id = lane
            .sketch_entities
            .iter()
            .map(|marker| (marker.id.as_str(), marker))
            .collect::<HashMap<_, _>>();
        for marker in &lane.sketch_entities {
            if !referenced.contains(marker.id.as_str())
                || marker.kind != SketchInputKind::LineOrCircle
                || entities
                    .iter()
                    .any(|entity| entity.native_ref.as_deref() == Some(marker.id.as_str()))
            {
                continue;
            }
            let Some(feature) = marker.feature_ref.as_deref() else {
                continue;
            };
            let Some(sketch) = sketches_by_feature.get(feature) else {
                continue;
            };
            let endpoints = line_endpoint_markers(marker, &markers_by_id);
            let [first_marker, second_marker] = endpoints.as_slice() else {
                continue;
            };
            let (Some(first), Some(second)) =
                (first_marker.coordinates_m, second_marker.coordinates_m)
            else {
                continue;
            };
            let first_native = quantize(
                Point2::new(first[0] * NATIVE_TO_IR, first[1] * NATIVE_TO_IR),
                QUANTUM,
            );
            let second_native = quantize(
                Point2::new(second[0] * NATIVE_TO_IR, second[1] * NATIVE_TO_IR),
                QUANTUM,
            );
            let candidates = transforms
                .get(feature)
                .into_iter()
                .flatten()
                .filter_map(|transform| {
                    Some((
                        transform.apply(first_native)?,
                        transform.apply(second_native)?,
                    ))
                })
                .collect::<HashSet<_>>();
            let candidates = candidates.into_iter().collect::<Vec<_>>();
            let [(start, end)] = candidates.as_slice() else {
                continue;
            };
            if start == end {
                continue;
            }
            let start = Point2::new(start.0 as f64 * QUANTUM, start.1 as f64 * QUANTUM);
            let end = Point2::new(end.0 as f64 * QUANTUM, end.1 as f64 * QUANTUM);
            let already_present = entities.iter().any(|entity| {
                entity.sketch == *sketch
                    && matches!(&entity.geometry, SketchGeometry::Line { start: existing_start, end: existing_end }
                        if (quantize(*existing_start, QUANTUM) == quantize(start, QUANTUM)
                            && quantize(*existing_end, QUANTUM) == quantize(end, QUANTUM))
                            || (quantize(*existing_start, QUANTUM) == quantize(end, QUANTUM)
                                && quantize(*existing_end, QUANTUM) == quantize(start, QUANTUM)))
            });
            if already_present {
                continue;
            }
            entities.push(SketchEntity {
                id: SketchEntityId(format!(
                    "sldprt:model:sketch-entity#relation-line:{lane_key}:{}",
                    marker.offset
                )),
                sketch: sketch.clone(),
                construction: true,
                native_ref: Some(marker.id.clone()),
                geometry_ref: None,
                endpoint_refs: vec![first_marker.id.clone(), second_marker.id.clone()],
                geometry: SketchGeometry::Line { start, end },
            });
        }
    }
}

fn relation_operand_geometry_ref(
    relation: &FeatureInputRelationInstance,
    operand_index: usize,
) -> String {
    format!("{}:operand:{operand_index}", relation.id)
}

/// Materialize point operands whose position is defined only within one dimension relation.
pub(crate) fn project_relation_solved_point_geometry(
    entities: &mut Vec<SketchEntity>,
    sketches: &[cadmpeg_ir::sketches::Sketch],
    features: &[cadmpeg_ir::features::Feature],
    parameters: &[cadmpeg_ir::features::DesignParameter],
    lanes: &[FeatureInputLane],
) {
    const QUANTUM: f64 = 1.0e-8;

    let sketches_by_feature = features
        .iter()
        .filter_map(|feature| {
            let cadmpeg_ir::features::FeatureDefinition::Sketch {
                sketch: Some(sketch),
                ..
            } = &feature.definition
            else {
                return None;
            };
            Some((feature.native_ref.as_deref()?, sketch.clone()))
        })
        .collect::<HashMap<_, _>>();
    let parameters_by_scalar = parameters
        .iter()
        .filter_map(|parameter| Some((parameter.native_ref.as_deref()?, parameter)))
        .collect::<HashMap<_, _>>();
    let markers_by_id = lanes
        .iter()
        .flat_map(|lane| &lane.sketch_entities)
        .map(|marker| (marker.id.as_str(), marker))
        .collect::<HashMap<_, _>>();
    let loci_by_marker = profile_loci_by_marker(features, sketches, entities, lanes);

    for lane in lanes {
        let lane_key = lane
            .id
            .rsplit_once('#')
            .map_or(lane.id.as_str(), |(_, key)| key);
        for relation in &lane.relation_instances {
            if !matches!(
                relation.family,
                FeatureInputRelationFamily::PointPointDistance
                    | FeatureInputRelationFamily::PointPointHorizontalDistance
                    | FeatureInputRelationFamily::PointPointVerticalDistance
            ) || relation.operands.len() != 2
            {
                continue;
            }
            let Some(sketch) = sketches_by_feature.get(relation.feature_ref.as_str()) else {
                continue;
            };
            let parameter = relation
                .parameter_scalar_ref
                .as_deref()
                .and_then(|scalar| parameters_by_scalar.get(scalar))
                .copied()
                .or_else(|| {
                    relation.parameter_scalar_ref.is_none().then_some(())?;
                    relation_parameter_by_display_name(relation, lane, features, parameters)
                });
            let Some(cadmpeg_ir::features::ParameterValue::Length(distance)) =
                parameter.and_then(|parameter| parameter.value.as_ref())
            else {
                continue;
            };
            let resolved = [0, 1].map(|index| {
                relation.operands[index]
                    .entity_ref
                    .as_deref()
                    .and_then(|marker| marker_point_locus(marker, &markers_by_id, &loci_by_marker))
            });
            let (known, missing_index) = match resolved {
                [Some(known), None] => (known, 1),
                [None, Some(known)] => (known, 0),
                _ => continue,
            };
            let Some(missing_marker) = relation.operands[missing_index]
                .entity_ref
                .as_deref()
                .and_then(|marker| markers_by_id.get(marker).copied())
            else {
                continue;
            };
            if missing_marker.coordinates_m.is_some()
                || !matches!(
                    missing_marker.kind,
                    SketchInputKind::Point | SketchInputKind::ConstrainedPoint
                )
            {
                continue;
            }
            let Some(known_point) = profile_locus_point(&known, entities) else {
                continue;
            };
            let mut candidates = entities
                .iter()
                .filter(|entity| entity.sketch == *sketch)
                .flat_map(sketch_entity_loci)
                .filter_map(|(point, _)| {
                    let measured = match relation.family {
                        FeatureInputRelationFamily::PointPointDistance => {
                            (point.u - known_point.u).hypot(point.v - known_point.v)
                        }
                        FeatureInputRelationFamily::PointPointHorizontalDistance => {
                            (point.u - known_point.u).abs()
                        }
                        FeatureInputRelationFamily::PointPointVerticalDistance => {
                            (point.v - known_point.v).abs()
                        }
                        _ => unreachable!("relation family was filtered above"),
                    };
                    same_dimension_length(measured, distance.0).then_some(quantize(point, QUANTUM))
                })
                .collect::<Vec<_>>();
            candidates.sort_unstable();
            candidates.dedup();
            let [(u, v)] = candidates.as_slice() else {
                continue;
            };
            let geometry_ref = relation_operand_geometry_ref(relation, missing_index);
            if entities
                .iter()
                .any(|entity| entity.geometry_ref.as_deref() == Some(geometry_ref.as_str()))
            {
                continue;
            }
            entities.push(SketchEntity {
                id: SketchEntityId(format!(
                    "sldprt:model:sketch-entity#dimension-point:{lane_key}:{}:{missing_index}",
                    relation.offset
                )),
                sketch: sketch.clone(),
                construction: true,
                native_ref: None,
                geometry_ref: Some(geometry_ref),
                endpoint_refs: Vec::new(),
                geometry: SketchGeometry::Point {
                    position: Point2::new(*u as f64 * QUANTUM, *v as f64 * QUANTUM),
                },
            });
        }
    }
}

fn implicit_circle_marker<'a>(
    lanes: &'a [FeatureInputLane],
    feature: &str,
    operand_kind: FeatureInputOperandKind,
    index: u16,
) -> Option<(&'a SketchInputEntity, f64)> {
    if operand_kind != FeatureInputOperandKind::Native(0x83fe) {
        return None;
    }
    let mut markers = lanes
        .iter()
        .flat_map(|lane| &lane.sketch_entities)
        .filter(|marker| marker.feature_ref.as_deref() == Some(feature))
        .filter(|marker| marker.local_id != Some(0))
        .filter(|marker| {
            marker.coordinates_m.is_some()
                && matches!(
                    marker.kind,
                    SketchInputKind::Point | SketchInputKind::ConstrainedPoint
                )
        })
        .collect::<Vec<_>>();
    markers.sort_unstable_by_key(|marker| marker.offset);
    if markers.is_empty() || markers.len() % 2 != 0 {
        return None;
    }
    let pair = markers.chunks_exact(2).nth(usize::from(index))?;
    let [center, radial] = pair else {
        return None;
    };
    let [cu, cv] = center.coordinates_m?;
    let [ru, rv] = radial.coordinates_m?;
    let radius = (ru - cu).hypot(rv - cv) * 1000.0;
    (radius.is_finite() && radius > 0.0).then_some((*center, radius))
}

/// Project owned native relation bindings into their neutral sketches.
pub(crate) fn project_relation_bindings(
    constraints: &mut Vec<SketchConstraint>,
    sketches: &[cadmpeg_ir::sketches::Sketch],
    features: &[cadmpeg_ir::features::Feature],
    sketch_entities: &[SketchEntity],
    parameters: &[cadmpeg_ir::features::DesignParameter],
    lanes: &[FeatureInputLane],
) {
    let sketches_by_feature = features
        .iter()
        .filter_map(|feature| {
            let cadmpeg_ir::features::FeatureDefinition::Sketch {
                sketch: Some(sketch),
                ..
            } = &feature.definition
            else {
                return None;
            };
            Some((feature.native_ref.as_deref()?, sketch))
        })
        .collect::<HashMap<_, _>>();
    let loci_by_marker = profile_loci_by_marker(features, sketches, sketch_entities, lanes);
    let markers_by_id = lanes
        .iter()
        .flat_map(|lane| &lane.sketch_entities)
        .map(|marker| (marker.id.as_str(), marker))
        .collect::<HashMap<_, _>>();
    let parameters_by_scalar = parameters
        .iter()
        .filter_map(|parameter| Some((parameter.native_ref.as_deref()?, parameter)))
        .collect::<HashMap<_, _>>();
    let mut claimed_parameter_ids = lanes
        .iter()
        .flat_map(|lane| &lane.relation_instances)
        .filter_map(|relation| relation.parameter_scalar_ref.as_deref())
        .filter_map(|scalar| parameters_by_scalar.get(scalar))
        .map(|parameter| parameter.id.clone())
        .collect::<HashSet<_>>();

    for lane in lanes {
        let lane_key = lane
            .id
            .rsplit_once('#')
            .map_or(lane.id.as_str(), |(_, key)| key);
        for relation in &lane.relation_instances {
            let Some(sketch) = sketches_by_feature.get(relation.feature_ref.as_str()) else {
                continue;
            };
            let direct_parameter = relation
                .parameter_scalar_ref
                .as_deref()
                .and_then(|scalar| parameters_by_scalar.get(scalar))
                .copied();
            let display_parameter = relation
                .parameter_scalar_ref
                .is_none()
                .then(|| relation_parameter_by_display_name(relation, lane, features, parameters))
                .flatten();
            let parameter = direct_parameter.or(display_parameter);
            if relation.parameter_scalar_ref.is_none() && parameter.is_none() {
                continue;
            }
            if let Some(parameter) = display_parameter {
                if !claimed_parameter_ids.insert(parameter.id.clone()) {
                    continue;
                }
            }
            let parameter_id = parameter.map(|parameter| parameter.id.clone());
            let native_kind = match relation.family {
                FeatureInputRelationFamily::LineLineDistance => "sgLLDist",
                FeatureInputRelationFamily::PointPointDistance => "sgPntPntDist",
                FeatureInputRelationFamily::PointLineDistance => "sgPntLineDist",
                FeatureInputRelationFamily::PointPointHorizontalDistance => "sgPntPntHorDist",
                FeatureInputRelationFamily::PointPointVerticalDistance => "sgPntPntVertDist",
                FeatureInputRelationFamily::Angle => "sgAnglDim",
                FeatureInputRelationFamily::CircleDiameter => "sgCircleDim",
            };
            let mut entities = relation
                .operands
                .iter()
                .filter_map(|operand| operand.entity_ref.as_deref())
                .flat_map(|marker| {
                    marker_entities(marker, &markers_by_id, &loci_by_marker).into_iter()
                })
                .collect::<Vec<_>>();
            entities.sort_by(|left, right| left.0.cmp(&right.0));
            entities.dedup();
            let definition = typed_relation_definition(
                relation,
                parameter,
                sketch,
                sketch_entities,
                &markers_by_id,
                &loci_by_marker,
            )
            .unwrap_or_else(|| SketchConstraintDefinition::Native {
                native_kind: native_kind.into(),
                entities,
                parameter: parameter_id,
                operands: relation
                    .operands
                    .iter()
                    .map(|operand| SketchNativeOperand {
                        native_kind: operand_kind_name(operand.kind),
                        object_index: u32::from(operand.entity_index),
                        native_ref: operand.entity_ref.clone(),
                    })
                    .collect(),
            });
            constraints.push(SketchConstraint {
                id: SketchConstraintId(format!(
                    "sldprt:model:sketch-constraint#relation:{lane_key}:{}",
                    relation.offset
                )),
                sketch: (*sketch).clone(),
                definition,
                native_ref: Some(relation.id.clone()),
            });
        }
        for marker in &lane.sketch_entities {
            let Some(sketch) = marker
                .feature_ref
                .as_deref()
                .and_then(|feature| sketches_by_feature.get(feature))
            else {
                continue;
            };
            let Some(definition) = typed_marker_relation_definition_in_sketch(
                marker,
                sketch,
                sketch_entities,
                &markers_by_id,
                &loci_by_marker,
            ) else {
                continue;
            };
            constraints.push(SketchConstraint {
                id: SketchConstraintId(format!(
                    "sldprt:model:sketch-constraint#marker:{lane_key}:{}",
                    marker.offset
                )),
                sketch: (*sketch).clone(),
                definition,
                native_ref: Some(marker.id.clone()),
            });
        }
    }
}

fn relation_parameter_by_display_name<'a>(
    relation: &FeatureInputRelationInstance,
    lane: &FeatureInputLane,
    features: &[cadmpeg_ir::features::Feature],
    parameters: &'a [cadmpeg_ir::features::DesignParameter],
) -> Option<&'a cadmpeg_ir::features::DesignParameter> {
    let owner = features
        .iter()
        .find(|feature| feature.native_ref.as_deref() == Some(relation.feature_ref.as_str()))?
        .id
        .clone();
    let scalars = lane
        .scalars
        .iter()
        .map(|scalar| (scalar.id.as_str(), scalar))
        .collect::<HashMap<_, _>>();
    let names = lane
        .names
        .iter()
        .map(|name| (name.id.as_str(), name.value.as_str()))
        .collect::<HashMap<_, _>>();
    let owner = &owner;
    let mut matches = relation
        .scalar_refs
        .iter()
        .filter_map(|scalar| scalars.get(scalar.as_str()))
        .filter(|scalar| scalar.role == FeatureInputScalarRole::Display)
        .filter_map(|scalar| names.get(scalar.name.as_str()).copied())
        .flat_map(|name| {
            parameters.iter().filter(move |parameter| {
                &parameter.owner == owner
                    && parameter.name == name
                    && parameter.native_ref.is_none()
            })
        });
    let first = matches.next()?;
    matches
        .all(|parameter| parameter.id == first.id)
        .then_some(first)
}

#[cfg(test)]
fn typed_marker_relation_definition(
    marker: &SketchInputEntity,
    markers_by_id: &HashMap<&str, &SketchInputEntity>,
    loci_by_marker: &HashMap<String, Vec<SketchLocus>>,
) -> Option<SketchConstraintDefinition> {
    typed_marker_relation_definition_in_sketch(
        marker,
        &SketchId(String::new()),
        &[],
        markers_by_id,
        loci_by_marker,
    )
}

fn typed_marker_relation_definition_in_sketch(
    marker: &SketchInputEntity,
    sketch: &SketchId,
    sketch_entities: &[SketchEntity],
    markers_by_id: &HashMap<&str, &SketchInputEntity>,
    loci_by_marker: &HashMap<String, Vec<SketchLocus>>,
) -> Option<SketchConstraintDefinition> {
    use crate::records::SketchRelationKind::{
        ArcAngle180, ArcAngle270, ArcAngle90, AtIntersection, Coincident, Collinear, Concentric,
        Coradial, EllipseAngle180, EllipseAngle270, EllipseAngle90, Equal, Fixed, Horizontal,
        HorizontalPoints, MergePoints, Midpoint, Parallel, Perpendicular, Symmetric, Tangent,
        Vertical, VerticalPoints,
    };
    let kind = match marker.kind {
        SketchInputKind::Relation(kind) => Some(kind),
        SketchInputKind::Native(_) => None,
        _ => return None,
    };
    if marker.links.is_empty()
        && !loci_by_marker.contains_key(&marker.id)
        && relation_owner_markers(marker, markers_by_id).is_empty()
    {
        return None;
    }
    let native = || {
        let mut entities = marker
            .links
            .iter()
            .flat_map(|link| marker_entities(&link.entity_ref, markers_by_id, loci_by_marker))
            .collect::<Vec<_>>();
        entities.sort_by(|left, right| left.0.cmp(&right.0));
        entities.dedup();
        let owners = relation_owner_markers(marker, markers_by_id);
        entities.extend(
            owners
                .iter()
                .flat_map(|owner| marker_entities(&owner.id, markers_by_id, loci_by_marker)),
        );
        entities.sort_by(|left, right| left.0.cmp(&right.0));
        entities.dedup();
        let mut operands = marker
            .links
            .iter()
            .map(|link| SketchNativeOperand {
                native_kind: "sldprt:marker-local-id".into(),
                object_index: u32::from(link.local_id),
                native_ref: Some(link.entity_ref.clone()),
            })
            .collect::<Vec<_>>();
        operands.extend(owners.into_iter().map(|owner| SketchNativeOperand {
            native_kind: "sldprt:marker-constraint-owner".into(),
            object_index: owner.object_index.or(owner.local_id).unwrap_or(u32::MAX),
            native_ref: Some(owner.id.clone()),
        }));
        SketchConstraintDefinition::Native {
            native_kind: match marker.kind {
                SketchInputKind::Relation(kind) => {
                    format!("sldprt:marker-relation:{}", kind.native_code())
                }
                SketchInputKind::Native(code) => format!("sldprt:marker-relation:{code}"),
                _ => unreachable!("non-relation markers were rejected"),
            },
            entities,
            parameter: None,
            operands,
        }
    };
    let Some(kind) = kind else {
        return Some(native());
    };
    Some(match kind {
        Horizontal | Vertical | Fixed => {
            let direct_entities =
                marker_entities(marker.id.as_str(), markers_by_id, loci_by_marker);
            let owner_entities =
                relation_owner_curve_entities(marker, markers_by_id, loci_by_marker);
            let entities = match owner_entities.as_slice() {
                [owner]
                    if direct_entities.iter().all(|entity| {
                        entity == owner || entity.0.contains("sketch-entity#relation-point:")
                    }) =>
                {
                    owner_entities
                }
                _ => direct_entities,
            };
            if let [entity] = entities.as_slice() {
                let invalid_axis_entity = !sketch_entities.is_empty()
                    && sketch_entities
                        .iter()
                        .find(|candidate| candidate.id == *entity)
                        .is_none_or(|candidate| {
                            let SketchGeometry::Line { start, end } = &candidate.geometry else {
                                return true;
                            };
                            if kind == Horizontal {
                                !same_dimension_length(start.v, end.v)
                            } else {
                                !same_dimension_length(start.u, end.u)
                            }
                        });
                if matches!(kind, Horizontal | Vertical)
                    && (invalid_axis_entity
                        || (sketch_entities.is_empty()
                            && entity.0.contains("sketch-entity#relation-point:")))
                {
                    return Some(native());
                }
                match kind {
                    Horizontal => SketchConstraintDefinition::Horizontal {
                        entity: entity.clone(),
                    },
                    Vertical => SketchConstraintDefinition::Vertical {
                        entity: entity.clone(),
                    },
                    Fixed => SketchConstraintDefinition::Fixed {
                        entity: entity.clone(),
                    },
                    _ => unreachable!("relation kind was filtered above"),
                }
            } else if matches!(kind, Horizontal | Vertical) {
                let loci =
                    relation_operand_loci(marker, markers_by_id, loci_by_marker).or_else(|| {
                        unique_axis_aligned_linked_loci(
                            marker,
                            sketch,
                            sketch_entities,
                            markers_by_id,
                            loci_by_marker,
                            kind == Horizontal,
                        )
                    });
                let Some(loci) = loci else {
                    return Some(native());
                };
                let [first, second] = loci.as_slice() else {
                    return Some(native());
                };
                if !sketch_entities.is_empty() {
                    let (Some(first_point), Some(second_point)) = (
                        profile_locus_point(first, sketch_entities),
                        profile_locus_point(second, sketch_entities),
                    ) else {
                        return Some(native());
                    };
                    let aligned = if kind == Horizontal {
                        same_dimension_length(first_point.v, second_point.v)
                    } else {
                        same_dimension_length(first_point.u, second_point.u)
                    };
                    if !aligned {
                        return Some(native());
                    }
                }
                match kind {
                    Horizontal => SketchConstraintDefinition::HorizontalPoints {
                        first: first.clone(),
                        second: second.clone(),
                    },
                    Vertical => SketchConstraintDefinition::VerticalPoints {
                        first: first.clone(),
                        second: second.clone(),
                    },
                    _ => unreachable!("relation kind was filtered above"),
                }
            } else {
                return Some(native());
            }
        }
        ArcAngle90 | ArcAngle180 | ArcAngle270 => {
            let Some(entity) = linked_single_arc_entity(marker, markers_by_id, loci_by_marker)
            else {
                return Some(native());
            };
            let angle = match kind {
                ArcAngle90 => std::f64::consts::FRAC_PI_2,
                ArcAngle180 => std::f64::consts::PI,
                ArcAngle270 => 3.0 * std::f64::consts::FRAC_PI_2,
                _ => unreachable!("relation kind was filtered above"),
            };
            if !sketch_entities.is_empty() {
                let Some(SketchEntity {
                    geometry:
                        SketchGeometry::Arc {
                            start_angle,
                            end_angle,
                            ..
                        },
                    ..
                }) = sketch_entities
                    .iter()
                    .find(|candidate| candidate.id == entity)
                else {
                    return Some(native());
                };
                let raw = end_angle.0 - start_angle.0;
                let mut sweep = raw.rem_euclid(std::f64::consts::TAU);
                if sweep <= 1.0e-12 && raw.abs() > 1.0e-12 {
                    sweep = std::f64::consts::TAU;
                }
                if !same_dimension_angle(sweep, angle) {
                    return Some(native());
                }
            }
            SketchConstraintDefinition::ArcAngle {
                entity,
                angle: cadmpeg_ir::features::Angle(angle),
            }
        }
        EllipseAngle90 | EllipseAngle180 | EllipseAngle270 => {
            let Some(entity) = linked_single_ellipse_entity(
                marker,
                markers_by_id,
                loci_by_marker,
                sketch_entities,
            ) else {
                return Some(native());
            };
            let angle = match kind {
                EllipseAngle90 => std::f64::consts::FRAC_PI_2,
                EllipseAngle180 => std::f64::consts::PI,
                EllipseAngle270 => 3.0 * std::f64::consts::FRAC_PI_2,
                _ => unreachable!("relation kind was filtered above"),
            };
            let Some(SketchEntity {
                geometry:
                    SketchGeometry::Ellipse {
                        start_angle: Some(start),
                        end_angle: Some(end),
                        ..
                    },
                ..
            }) = sketch_entities
                .iter()
                .find(|candidate| candidate.id == entity)
            else {
                return Some(native());
            };
            let raw = end.0 - start.0;
            let mut sweep = raw.rem_euclid(std::f64::consts::TAU);
            if sweep <= 1.0e-12 && raw.abs() > 1.0e-12 {
                sweep = std::f64::consts::TAU;
            }
            if !same_dimension_angle(sweep, angle) {
                return Some(native());
            }
            SketchConstraintDefinition::EllipseAngle {
                entity,
                angle: cadmpeg_ir::features::Angle(angle),
            }
        }
        Parallel | Perpendicular | Tangent | Equal | Collinear | Concentric | Coradial => {
            let owner_entities =
                relation_owner_curve_entities(marker, markers_by_id, loci_by_marker);
            let forward_entities = marker
                .links
                .iter()
                .flat_map(|link| marker_entities(&link.entity_ref, markers_by_id, loci_by_marker))
                .filter(|entity| !entity.0.contains("sketch-entity#relation-point:"))
                .collect::<Vec<_>>();
            let entities = if owner_entities.len() == 2
                && forward_entities
                    .iter()
                    .all(|entity| owner_entities.contains(entity))
            {
                owner_entities
            } else {
                let Some(entities) = linked_single_entities(marker, markers_by_id, loci_by_marker)
                else {
                    return Some(native());
                };
                entities
            };
            let [first, second] = entities.as_slice() else {
                return Some(native());
            };
            if !sketch_entities.is_empty() {
                let Some(first_entity) = sketch_entities
                    .iter()
                    .find(|candidate| candidate.id == *first)
                else {
                    return Some(native());
                };
                let Some(second_entity) = sketch_entities
                    .iter()
                    .find(|candidate| candidate.id == *second)
                else {
                    return Some(native());
                };
                if !binary_relation_matches_evaluated_geometry(kind, first_entity, second_entity) {
                    return Some(native());
                }
            }
            match kind {
                Parallel => SketchConstraintDefinition::Parallel {
                    first: first.clone(),
                    second: second.clone(),
                },
                Perpendicular => SketchConstraintDefinition::Perpendicular {
                    first: first.clone(),
                    second: second.clone(),
                },
                Tangent => SketchConstraintDefinition::Tangent {
                    first: first.clone(),
                    second: second.clone(),
                },
                Equal => SketchConstraintDefinition::Equal {
                    first: first.clone(),
                    second: second.clone(),
                },
                Collinear => SketchConstraintDefinition::Collinear {
                    first: first.clone(),
                    second: second.clone(),
                },
                Concentric => SketchConstraintDefinition::Concentric {
                    first: first.clone(),
                    second: second.clone(),
                },
                Coradial => SketchConstraintDefinition::Coradial {
                    first: first.clone(),
                    second: second.clone(),
                },
                _ => unreachable!("relation kind was filtered above"),
            }
        }
        Coincident | MergePoints => {
            let Some(loci) = relation_operand_loci(marker, markers_by_id, loci_by_marker) else {
                return Some(native());
            };
            if loci.len() < 2 {
                return Some(native());
            }
            if !sketch_entities.is_empty() {
                let Some(points) = loci
                    .iter()
                    .map(|locus| profile_locus_point(locus, sketch_entities))
                    .collect::<Option<Vec<_>>>()
                else {
                    return Some(native());
                };
                if points.iter().skip(1).any(|point| {
                    !same_dimension_length(point.u, points[0].u)
                        || !same_dimension_length(point.v, points[0].v)
                }) {
                    return Some(native());
                }
            }
            SketchConstraintDefinition::CoincidentLoci { loci }
        }
        HorizontalPoints | VerticalPoints => {
            let Some(loci) = relation_operand_loci(marker, markers_by_id, loci_by_marker) else {
                return Some(native());
            };
            let [first, second] = loci.as_slice() else {
                return Some(native());
            };
            if !sketch_entities.is_empty() {
                let (Some(first_point), Some(second_point)) = (
                    profile_locus_point(first, sketch_entities),
                    profile_locus_point(second, sketch_entities),
                ) else {
                    return Some(native());
                };
                let aligned = if kind == HorizontalPoints {
                    same_dimension_length(first_point.v, second_point.v)
                } else {
                    same_dimension_length(first_point.u, second_point.u)
                };
                if !aligned {
                    return Some(native());
                }
            }
            match kind {
                HorizontalPoints => SketchConstraintDefinition::HorizontalPoints {
                    first: first.clone(),
                    second: second.clone(),
                },
                VerticalPoints => SketchConstraintDefinition::VerticalPoints {
                    first: first.clone(),
                    second: second.clone(),
                },
                _ => unreachable!("relation kind was filtered above"),
            }
        }
        AtIntersection => {
            if sketch_entities.is_empty() {
                return Some(native());
            }
            let Some(loci) = relation_operand_loci(marker, markers_by_id, loci_by_marker) else {
                return Some(native());
            };
            let mut point = None;
            let mut entities = Vec::new();
            for locus in loci {
                let Some(entity) = sketch_entities
                    .iter()
                    .find(|candidate| candidate.id == locus_entity(&locus))
                else {
                    return Some(native());
                };
                let entity_locus = matches!(locus, SketchLocus::Entity(_));
                if entity_locus
                    && !matches!(
                        entity.geometry,
                        SketchGeometry::Point { .. } | SketchGeometry::Native { .. }
                    )
                {
                    entities.push(entity.id.clone());
                } else if point.replace(locus).is_some() {
                    return Some(native());
                }
            }
            let (Some(point), [first, second]) = (point, entities.as_slice()) else {
                return Some(native());
            };
            if first == second {
                return Some(native());
            }
            let Some(position) = profile_locus_point(&point, sketch_entities) else {
                return Some(native());
            };
            if [first, second].into_iter().any(|id| {
                sketch_entities
                    .iter()
                    .find(|entity| entity.id == *id)
                    .is_none_or(|entity| !sketch_entity_contains_point(entity, position))
            }) {
                return Some(native());
            }
            SketchConstraintDefinition::AtIntersection {
                point,
                first: first.clone(),
                second: second.clone(),
            }
        }
        Symmetric => {
            if sketch_entities.is_empty() {
                return Some(native());
            }
            let Some(loci) = relation_operand_loci(marker, markers_by_id, loci_by_marker) else {
                return Some(native());
            };
            let mut axis = None;
            let mut points = Vec::new();
            for locus in loci {
                let entity = sketch_entities
                    .iter()
                    .find(|candidate| candidate.id == locus_entity(&locus));
                if matches!(locus, SketchLocus::Entity(_))
                    && entity.is_some_and(|entity| {
                        matches!(entity.geometry, SketchGeometry::Line { .. })
                    })
                {
                    if axis.replace(locus_entity(&locus)).is_some() {
                        return Some(native());
                    }
                } else {
                    points.push(locus);
                }
            }
            let (Some(axis), [first, second]) = (axis, points.as_slice()) else {
                return Some(native());
            };
            if first == second {
                return Some(native());
            }
            let Some(first_point) = profile_locus_point(first, sketch_entities) else {
                return Some(native());
            };
            let Some(second_point) = profile_locus_point(second, sketch_entities) else {
                return Some(native());
            };
            let Some(SketchEntity {
                geometry: SketchGeometry::Line { start, end },
                ..
            }) = sketch_entities.iter().find(|entity| entity.id == axis)
            else {
                return Some(native());
            };
            let du = end.u - start.u;
            let dv = end.v - start.v;
            let length = du.hypot(dv);
            if length <= SKETCH_POINT_TOLERANCE {
                return Some(native());
            }
            let axis_coordinate = |point: Point2| {
                (
                    ((point.u - start.u) * du + (point.v - start.v) * dv) / length,
                    ((point.u - start.u) * dv - (point.v - start.v) * du) / length,
                )
            };
            let (first_along, first_across) = axis_coordinate(first_point);
            let (second_along, second_across) = axis_coordinate(second_point);
            if !same_dimension_length(first_along, second_along)
                || !same_dimension_length(first_across, -second_across)
            {
                return Some(native());
            }
            SketchConstraintDefinition::Symmetric {
                first: first.clone(),
                second: second.clone(),
                axis,
            }
        }
        Midpoint => {
            let Some((point, entity)) =
                linked_midpoint_operands(marker, markers_by_id, loci_by_marker)
            else {
                return Some(native());
            };
            if !sketch_entities.is_empty() {
                let Some(point_position) = profile_locus_point(&point, sketch_entities) else {
                    return Some(native());
                };
                let Some(midpoint) = sketch_entities
                    .iter()
                    .find(|candidate| candidate.id == entity)
                    .and_then(sketch_entity_midpoint)
                else {
                    return Some(native());
                };
                if !same_dimension_length(point_position.u, midpoint.u)
                    || !same_dimension_length(point_position.v, midpoint.v)
                {
                    return Some(native());
                }
            }
            SketchConstraintDefinition::Midpoint { point, entity }
        }
        crate::records::SketchRelationKind::Distance
        | crate::records::SketchRelationKind::Angle
        | crate::records::SketchRelationKind::Radius
        | crate::records::SketchRelationKind::Diameter => return None,
        _ => native(),
    })
}

fn sketch_entity_midpoint(entity: &SketchEntity) -> Option<Point2> {
    match &entity.geometry {
        SketchGeometry::Line { start, end } => Some(Point2::new(
            (start.u + end.u) * 0.5,
            (start.v + end.v) * 0.5,
        )),
        SketchGeometry::Arc {
            center,
            radius,
            start_angle,
            end_angle,
        } => {
            let raw = end_angle.0 - start_angle.0;
            let mut sweep = raw.rem_euclid(std::f64::consts::TAU);
            if sweep <= 1.0e-12 && raw.abs() > 1.0e-12 {
                sweep = std::f64::consts::TAU;
            }
            let angle = start_angle.0 + sweep * 0.5;
            Some(Point2::new(
                center.u + radius.0 * angle.cos(),
                center.v + radius.0 * angle.sin(),
            ))
        }
        _ => None,
    }
}

fn sketch_entity_contains_point(entity: &SketchEntity, point: Point2) -> bool {
    match &entity.geometry {
        SketchGeometry::Line { start, end } => {
            let du = end.u - start.u;
            let dv = end.v - start.v;
            let length_squared = du * du + dv * dv;
            if length_squared <= SKETCH_POINT_TOLERANCE * SKETCH_POINT_TOLERANCE {
                return false;
            }
            let parameter = ((point.u - start.u) * du + (point.v - start.v) * dv) / length_squared;
            let distance =
                ((point.u - start.u) * dv - (point.v - start.v) * du).abs() / length_squared.sqrt();
            distance <= SKETCH_POINT_TOLERANCE
                && (-SKETCH_POINT_TOLERANCE..=1.0 + SKETCH_POINT_TOLERANCE).contains(&parameter)
        }
        SketchGeometry::Circle { center, radius } => {
            same_dimension_length((point.u - center.u).hypot(point.v - center.v), radius.0)
        }
        SketchGeometry::Arc {
            center,
            radius,
            start_angle,
            end_angle,
        } => {
            if !same_dimension_length((point.u - center.u).hypot(point.v - center.v), radius.0) {
                return false;
            }
            let raw = end_angle.0 - start_angle.0;
            let mut sweep = raw.rem_euclid(std::f64::consts::TAU);
            if sweep <= 1.0e-12 && raw.abs() > 1.0e-12 {
                sweep = std::f64::consts::TAU;
            }
            let parameter = ((point.v - center.v).atan2(point.u - center.u) - start_angle.0)
                .rem_euclid(std::f64::consts::TAU);
            parameter <= sweep + 1.0e-9
        }
        SketchGeometry::Ellipse {
            center,
            major_angle,
            major_radius,
            minor_radius,
            start_angle,
            end_angle,
        } => {
            let cosine = major_angle.0.cos();
            let sine = major_angle.0.sin();
            let du = point.u - center.u;
            let dv = point.v - center.v;
            let x = du * cosine + dv * sine;
            let y = -du * sine + dv * cosine;
            let equation = (x / major_radius.0).powi(2) + (y / minor_radius.0).powi(2);
            if (equation - 1.0).abs() > 1.0e-9 {
                return false;
            }
            match (start_angle, end_angle) {
                (Some(start), Some(end)) => {
                    let parameter = ((y / minor_radius.0).atan2(x / major_radius.0) - start.0)
                        .rem_euclid(std::f64::consts::TAU);
                    let raw = end.0 - start.0;
                    let mut sweep = raw.rem_euclid(std::f64::consts::TAU);
                    if sweep <= 1.0e-12 && raw.abs() > 1.0e-12 {
                        sweep = std::f64::consts::TAU;
                    }
                    parameter <= sweep + 1.0e-9
                }
                (None, None) => true,
                _ => false,
            }
        }
        SketchGeometry::Point { .. }
        | SketchGeometry::Nurbs { .. }
        | SketchGeometry::Native { .. } => false,
    }
}

fn binary_relation_matches_evaluated_geometry(
    kind: crate::records::SketchRelationKind,
    first: &SketchEntity,
    second: &SketchEntity,
) -> bool {
    use crate::records::SketchRelationKind::{
        Collinear, Concentric, Coradial, Equal, Parallel, Perpendicular, Tangent,
    };
    match kind {
        Parallel => line_relation_value(first, second, |cross, _dot, lengths| {
            cross.abs() <= SKETCH_POINT_TOLERANCE * lengths
        }),
        Perpendicular => line_relation_value(first, second, |_cross, dot, lengths| {
            dot.abs() <= SKETCH_POINT_TOLERANCE * lengths
        }),
        Collinear => line_line_distance(first, second)
            .is_some_and(|distance| same_dimension_length(distance, 0.0)),
        Concentric => centered_geometry(first)
            .zip(centered_geometry(second))
            .is_some_and(|(first, second)| {
                same_dimension_length(first.u, second.u) && same_dimension_length(first.v, second.v)
            }),
        Coradial => centered_geometry(first)
            .zip(circular_radius(first))
            .zip(centered_geometry(second).zip(circular_radius(second)))
            .is_some_and(
                |((first_center, first_radius), (second_center, second_radius))| {
                    same_dimension_length(first_center.u, second_center.u)
                        && same_dimension_length(first_center.v, second_center.v)
                        && same_dimension_length(first_radius, second_radius)
                },
            ),
        Equal => equal_geometry_size(first, second),
        Tangent => tangent_geometry(first, second),
        _ => false,
    }
}

fn line_relation_value(
    first: &SketchEntity,
    second: &SketchEntity,
    predicate: impl FnOnce(f64, f64, f64) -> bool,
) -> bool {
    let Some((first_u, first_v, first_length)) = line_direction(first) else {
        return false;
    };
    let Some((second_u, second_v, second_length)) = line_direction(second) else {
        return false;
    };
    predicate(
        first_u * second_v - first_v * second_u,
        first_u * second_u + first_v * second_v,
        first_length * second_length,
    )
}

fn line_direction(entity: &SketchEntity) -> Option<(f64, f64, f64)> {
    let SketchGeometry::Line { start, end } = &entity.geometry else {
        return None;
    };
    let u = end.u - start.u;
    let v = end.v - start.v;
    let length = u.hypot(v);
    (length > SKETCH_POINT_TOLERANCE).then_some((u, v, length))
}

fn centered_geometry(entity: &SketchEntity) -> Option<Point2> {
    match &entity.geometry {
        SketchGeometry::Circle { center, .. }
        | SketchGeometry::Arc { center, .. }
        | SketchGeometry::Ellipse { center, .. } => Some(*center),
        _ => None,
    }
}

fn circular_radius(entity: &SketchEntity) -> Option<f64> {
    match &entity.geometry {
        SketchGeometry::Circle { radius, .. } | SketchGeometry::Arc { radius, .. } => {
            Some(radius.0)
        }
        _ => None,
    }
}

fn equal_geometry_size(first: &SketchEntity, second: &SketchEntity) -> bool {
    match (&first.geometry, &second.geometry) {
        (
            SketchGeometry::Line {
                start: first_start,
                end: first_end,
            },
            SketchGeometry::Line {
                start: second_start,
                end: second_end,
            },
        ) => same_dimension_length(
            (first_end.u - first_start.u).hypot(first_end.v - first_start.v),
            (second_end.u - second_start.u).hypot(second_end.v - second_start.v),
        ),
        (
            SketchGeometry::Circle {
                radius: first_radius,
                ..
            }
            | SketchGeometry::Arc {
                radius: first_radius,
                ..
            },
            SketchGeometry::Circle {
                radius: second_radius,
                ..
            }
            | SketchGeometry::Arc {
                radius: second_radius,
                ..
            },
        ) => same_dimension_length(first_radius.0, second_radius.0),
        (
            SketchGeometry::Ellipse {
                major_radius: first_major,
                minor_radius: first_minor,
                ..
            },
            SketchGeometry::Ellipse {
                major_radius: second_major,
                minor_radius: second_minor,
                ..
            },
        ) => {
            same_dimension_length(first_major.0, second_major.0)
                && same_dimension_length(first_minor.0, second_minor.0)
        }
        _ => false,
    }
}

fn tangent_geometry(first: &SketchEntity, second: &SketchEntity) -> bool {
    let line_circle = |line: &SketchEntity, circle: &SketchEntity| {
        if let SketchGeometry::Ellipse {
            center,
            major_angle,
            major_radius,
            minor_radius,
            ..
        } = &circle.geometry
        {
            let Some((du, dv, length)) = line_direction(line) else {
                return false;
            };
            let normal = [-dv / length, du / length];
            let major = [major_angle.0.cos(), major_angle.0.sin()];
            let minor = [-major[1], major[0]];
            let support = ((major_radius.0 * (normal[0] * major[0] + normal[1] * major[1]))
                .powi(2)
                + (minor_radius.0 * (normal[0] * minor[0] + normal[1] * minor[1])).powi(2))
            .sqrt();
            return point_line_distance_value(*center, line)
                .is_some_and(|distance| same_dimension_length(distance, support));
        }
        centered_geometry(circle)
            .zip(circular_radius(circle))
            .and_then(|(center, radius)| {
                point_line_distance_value(center, line).map(|distance| (distance, radius))
            })
            .is_some_and(|(distance, radius)| same_dimension_length(distance, radius))
    };
    if matches!(first.geometry, SketchGeometry::Line { .. }) {
        return line_circle(first, second);
    }
    if matches!(second.geometry, SketchGeometry::Line { .. }) {
        return line_circle(second, first);
    }
    centered_geometry(first)
        .zip(circular_radius(first))
        .zip(centered_geometry(second).zip(circular_radius(second)))
        .is_some_and(
            |((first_center, first_radius), (second_center, second_radius))| {
                let center_distance =
                    (second_center.u - first_center.u).hypot(second_center.v - first_center.v);
                same_dimension_length(center_distance, first_radius + second_radius)
                    || same_dimension_length(center_distance, (first_radius - second_radius).abs())
            },
        )
}

fn unique_axis_aligned_linked_loci(
    marker: &SketchInputEntity,
    sketch: &SketchId,
    sketch_entities: &[SketchEntity],
    markers_by_id: &HashMap<&str, &SketchInputEntity>,
    loci_by_marker: &HashMap<String, Vec<SketchLocus>>,
    horizontal: bool,
) -> Option<Vec<SketchLocus>> {
    let [first_link, second_link] = marker.links.as_slice() else {
        return None;
    };
    let first = marker_point_locus(&first_link.entity_ref, markers_by_id, loci_by_marker);
    let second = marker_point_locus(&second_link.entity_ref, markers_by_id, loci_by_marker);
    let (known, known_is_first) = match (first, second) {
        (Some(known), None) => (known, true),
        (None, Some(known)) => (known, false),
        _ => return None,
    };
    let point = |locus: &SketchLocus| {
        let entity = sketch_entities
            .iter()
            .find(|entity| entity.id == locus_entity(locus))?;
        sketch_entity_loci(entity)
            .into_iter()
            .find_map(|(point, candidate)| (candidate == *locus).then_some(point))
    };
    let known_point = point(&known)?;
    let mut candidates = canonical_profile_loci(sketch, sketch_entities)
        .into_iter()
        .filter_map(|(candidate_point, candidate)| {
            let aligned = if horizontal {
                same_dimension_length(candidate_point.v, known_point.v)
            } else {
                same_dimension_length(candidate_point.u, known_point.u)
            };
            (candidate != known && aligned).then_some(candidate)
        })
        .collect::<Vec<_>>();
    candidates.sort_by(|left, right| locus_key(left).cmp(&locus_key(right)));
    candidates.dedup();
    let [candidate] = candidates.as_slice() else {
        return None;
    };
    Some(if known_is_first {
        vec![known, candidate.clone()]
    } else {
        vec![candidate.clone(), known]
    })
}

fn relation_owner_markers<'a>(
    relation: &SketchInputEntity,
    markers_by_id: &'a HashMap<&str, &SketchInputEntity>,
) -> Vec<&'a SketchInputEntity> {
    let mut owners = markers_by_id
        .values()
        .copied()
        .filter(|marker| marker.feature_ref == relation.feature_ref)
        .filter(|marker| {
            matches!(
                marker.kind,
                SketchInputKind::Point
                    | SketchInputKind::LineOrCircle
                    | SketchInputKind::Arc
                    | SketchInputKind::ConstrainedPoint
            )
        })
        .filter(|marker| {
            marker
                .links
                .iter()
                .any(|link| link.entity_ref == relation.id)
        })
        .collect::<Vec<_>>();
    owners.sort_unstable_by_key(|marker| marker.offset);
    owners
}

fn relation_owner_curve_entities(
    relation: &SketchInputEntity,
    markers_by_id: &HashMap<&str, &SketchInputEntity>,
    loci_by_marker: &HashMap<String, Vec<SketchLocus>>,
) -> Vec<SketchEntityId> {
    let mut entities = relation_owner_markers(relation, markers_by_id)
        .into_iter()
        .filter(|owner| {
            matches!(
                owner.kind,
                SketchInputKind::LineOrCircle | SketchInputKind::Arc
            )
        })
        .flat_map(|owner| marker_entities(&owner.id, markers_by_id, loci_by_marker))
        .collect::<Vec<_>>();
    entities.sort_by(|left, right| left.0.cmp(&right.0));
    entities.dedup();
    entities
}

fn line_endpoint_markers<'a>(
    line: &SketchInputEntity,
    markers_by_id: &'a HashMap<&str, &SketchInputEntity>,
) -> Vec<&'a SketchInputEntity> {
    let mut endpoints = line
        .links
        .iter()
        .filter_map(|link| markers_by_id.get(link.entity_ref.as_str()).copied())
        .chain(markers_by_id.values().copied().filter(|candidate| {
            candidate
                .links
                .iter()
                .any(|link| link.entity_ref == line.id)
        }))
        .filter(|endpoint| {
            endpoint.feature_ref == line.feature_ref
                && endpoint.coordinates_m.is_some()
                && matches!(
                    endpoint.kind,
                    SketchInputKind::Point | SketchInputKind::ConstrainedPoint
                )
        })
        .collect::<Vec<_>>();
    endpoints.sort_unstable_by_key(|endpoint| endpoint.offset);
    endpoints.dedup_by_key(|endpoint| endpoint.id.as_str());
    endpoints
}

fn linked_single_arc_entity(
    marker: &SketchInputEntity,
    markers_by_id: &HashMap<&str, &SketchInputEntity>,
    loci_by_marker: &HashMap<String, Vec<SketchLocus>>,
) -> Option<SketchEntityId> {
    if marker.links.is_empty()
        || marker.links.iter().any(|link| {
            !matches!(
                markers_by_id
                    .get(link.entity_ref.as_str())
                    .map(|marker| marker.kind),
                Some(SketchInputKind::Arc)
            )
        })
    {
        return None;
    }
    let entities = linked_single_entities(marker, markers_by_id, loci_by_marker)?;
    let [entity] = entities.as_slice() else {
        return None;
    };
    Some(entity.clone())
}

fn linked_single_ellipse_entity(
    marker: &SketchInputEntity,
    markers_by_id: &HashMap<&str, &SketchInputEntity>,
    loci_by_marker: &HashMap<String, Vec<SketchLocus>>,
    sketch_entities: &[SketchEntity],
) -> Option<SketchEntityId> {
    let entities = linked_single_entities(marker, markers_by_id, loci_by_marker)?;
    let [entity] = entities.as_slice() else {
        return None;
    };
    sketch_entities
        .iter()
        .find(|candidate| candidate.id == *entity)
        .filter(|candidate| matches!(candidate.geometry, SketchGeometry::Ellipse { .. }))?;
    Some(entity.clone())
}

fn linked_midpoint_operands(
    marker: &SketchInputEntity,
    markers_by_id: &HashMap<&str, &SketchInputEntity>,
    loci_by_marker: &HashMap<String, Vec<SketchLocus>>,
) -> Option<(SketchLocus, SketchEntityId)> {
    let [first, second] = marker.links.as_slice() else {
        return None;
    };
    let mut point = None;
    let mut entity = None;
    for link in [first, second] {
        let linked_marker = markers_by_id.get(link.entity_ref.as_str())?;
        let locus = unique_locus(loci_by_marker.get(&link.entity_ref)?)?;
        match linked_marker.kind {
            SketchInputKind::Point | SketchInputKind::ConstrainedPoint if point.is_none() => {
                point = Some(locus);
            }
            SketchInputKind::LineOrCircle | SketchInputKind::Arc if entity.is_none() => {
                entity = Some(locus_entity(&locus));
            }
            _ => return None,
        }
    }
    Some((point?, entity?))
}

fn relation_operand_loci(
    relation: &SketchInputEntity,
    markers_by_id: &HashMap<&str, &SketchInputEntity>,
    loci_by_marker: &HashMap<String, Vec<SketchLocus>>,
) -> Option<Vec<SketchLocus>> {
    let owners = relation_owner_markers(relation, markers_by_id);
    let loci = relation
        .links
        .iter()
        .map(|link| link.entity_ref.as_str())
        .chain(owners.iter().map(|owner| owner.id.as_str()))
        .map(|marker| marker_point_locus(marker, markers_by_id, loci_by_marker))
        .collect::<Option<Vec<_>>>()?;
    Some(loci.into_iter().fold(Vec::new(), |mut unique, locus| {
        if !unique.contains(&locus) {
            unique.push(locus);
        }
        unique
    }))
}

fn linked_single_entities(
    marker: &SketchInputEntity,
    markers_by_id: &HashMap<&str, &SketchInputEntity>,
    loci_by_marker: &HashMap<String, Vec<SketchLocus>>,
) -> Option<Vec<SketchEntityId>> {
    let mut result = Vec::new();
    for link in &marker.links {
        let entities = marker_entities(&link.entity_ref, markers_by_id, loci_by_marker);
        let [entity] = entities.as_slice() else {
            return None;
        };
        if !result.contains(entity) {
            result.push(entity.clone());
        }
    }
    Some(result)
}

fn typed_relation_definition(
    relation: &FeatureInputRelationInstance,
    parameter: Option<&cadmpeg_ir::features::DesignParameter>,
    sketch: &SketchId,
    sketch_entities: &[SketchEntity],
    markers_by_id: &HashMap<&str, &SketchInputEntity>,
    loci_by_marker: &HashMap<String, Vec<SketchLocus>>,
) -> Option<SketchConstraintDefinition> {
    use FeatureInputRelationFamily::{
        Angle, CircleDiameter, LineLineDistance, PointLineDistance, PointPointDistance,
        PointPointHorizontalDistance, PointPointVerticalDistance,
    };
    let parameter = parameter?;
    let parameter_id = parameter.id.clone();
    let marker = |index: usize| relation_operand_marker(relation, index, sketch, markers_by_id);
    let point = |index: usize| {
        let scoped_ref = relation_operand_geometry_ref(relation, index);
        sketch_entities
            .iter()
            .find(|entity| entity.geometry_ref.as_deref() == Some(scoped_ref.as_str()))
            .map(|entity| SketchLocus::Entity(entity.id.clone()))
            .or_else(|| {
                let marker = marker(index)?;
                if matches!(
                    relation.operands.get(index).map(|operand| operand.kind),
                    Some(FeatureInputOperandKind::Native(0x837b | 0xbc7c))
                ) {
                    loci_by_marker
                        .get(&qualified_point_marker_key(marker))
                        .and_then(|loci| unique_locus(loci))
                } else {
                    marker_point_locus(marker, markers_by_id, loci_by_marker)
                }
            })
    };
    match relation.family {
        PointPointDistance => {
            let first = point(0);
            let second = point(1);
            let (mut first, mut second) = match (first, second) {
                (Some(first), Some(second)) => (first, second),
                (Some(known), None) => (
                    known.clone(),
                    unique_profile_distance_locus(sketch, &known, parameter, sketch_entities)?,
                ),
                (None, Some(known)) => (
                    unique_profile_distance_locus(sketch, &known, parameter, sketch_entities)?,
                    known,
                ),
                (None, None) => {
                    unique_profile_distance_loci_pair(sketch, parameter, sketch_entities)?
                }
            };
            if first == second {
                return None;
            }
            if !sketch_entities.is_empty() {
                let cadmpeg_ir::features::ParameterValue::Length(expected) =
                    parameter.value.as_ref()?
                else {
                    return None;
                };
                let first_point = profile_locus_point(&first, sketch_entities)?;
                let second_point = profile_locus_point(&second, sketch_entities)?;
                if !same_dimension_length(
                    (second_point.u - first_point.u).hypot(second_point.v - first_point.v),
                    expected.0,
                ) {
                    (first, second) = unique_repaired_profile_distance_loci_pair(
                        sketch,
                        &first,
                        &second,
                        parameter,
                        sketch_entities,
                    )?;
                }
            }
            Some(SketchConstraintDefinition::DistanceLoci {
                first,
                second,
                parameter: parameter_id,
            })
        }
        PointPointHorizontalDistance | PointPointVerticalDistance => {
            let horizontal = relation.family == PointPointHorizontalDistance;
            let first = point(0);
            let second = point(1);
            let (mut first, mut second) = match (first, second) {
                (Some(first), Some(second)) => (first, second),
                (Some(known), None) => (
                    known.clone(),
                    unique_profile_axis_distance_locus(
                        sketch,
                        &known,
                        parameter,
                        sketch_entities,
                        horizontal,
                    )?,
                ),
                (None, Some(known)) => (
                    unique_profile_axis_distance_locus(
                        sketch,
                        &known,
                        parameter,
                        sketch_entities,
                        horizontal,
                    )?,
                    known,
                ),
                (None, None) => unique_profile_axis_distance_pair(
                    sketch,
                    parameter,
                    sketch_entities,
                    horizontal,
                )?,
            };
            if first == second {
                return None;
            }
            if !sketch_entities.is_empty() {
                let cadmpeg_ir::features::ParameterValue::Length(expected) =
                    parameter.value.as_ref()?
                else {
                    return None;
                };
                let first_point = profile_locus_point(&first, sketch_entities)?;
                let second_point = profile_locus_point(&second, sketch_entities)?;
                let measured = if horizontal {
                    (second_point.u - first_point.u).abs()
                } else {
                    (second_point.v - first_point.v).abs()
                };
                if !same_dimension_length(measured, expected.0) {
                    (first, second) = unique_repaired_profile_axis_distance_pair(
                        sketch,
                        &first,
                        &second,
                        parameter,
                        sketch_entities,
                        horizontal,
                    )?;
                }
            }
            Some(match relation.family {
                PointPointHorizontalDistance => SketchConstraintDefinition::HorizontalDistance {
                    first,
                    second,
                    parameter: parameter_id,
                },
                PointPointVerticalDistance => SketchConstraintDefinition::VerticalDistance {
                    first,
                    second,
                    parameter: parameter_id,
                },
                _ => unreachable!("relation family was filtered above"),
            })
        }
        PointLineDistance => {
            let point = marker(0)
                .and_then(|marker| marker_point_locus(marker, markers_by_id, loci_by_marker));
            let line = marker(1).and_then(|marker| {
                single_marker_line_entity(marker, markers_by_id, loci_by_marker, sketch_entities)
            });
            let (mut point, mut line) = match (point, line) {
                (Some(point), Some(line)) => (point, line),
                (Some(point), None) => (
                    point.clone(),
                    unique_profile_point_line_entity(sketch, &point, parameter, sketch_entities)?,
                ),
                (None, Some(line)) => (
                    unique_profile_line_point_locus(sketch, &line, parameter, sketch_entities)?,
                    line,
                ),
                (None, None) => unique_profile_point_line_pair(sketch, parameter, sketch_entities)?,
            };
            let cadmpeg_ir::features::ParameterValue::Length(expected) =
                parameter.value.as_ref()?
            else {
                return None;
            };
            let point_position = profile_locus_point(&point, sketch_entities)?;
            let line_entity = sketch_entities.iter().find(|entity| entity.id == line)?;
            if !point_line_distance_value(point_position, line_entity)
                .is_some_and(|measured| same_dimension_length(measured, expected.0))
            {
                (point, line) = unique_repaired_profile_point_line_pair(
                    sketch,
                    &point,
                    &line,
                    parameter,
                    sketch_entities,
                )?;
            }
            Some(SketchConstraintDefinition::DistanceLoci {
                first: point,
                second: SketchLocus::Entity(line),
                parameter: parameter_id,
            })
        }
        LineLineDistance => {
            let curve = |index| {
                marker(index).and_then(|marker| {
                    single_marker_line_entity(
                        marker,
                        markers_by_id,
                        loci_by_marker,
                        sketch_entities,
                    )
                })
            };
            let first = curve(0);
            let second = curve(1);
            let (mut first, mut second) = match (first, second) {
                (Some(first), Some(second)) => (first, second),
                (Some(known), None) => (
                    known.clone(),
                    unique_profile_line_distance_entity(
                        sketch,
                        &known,
                        parameter,
                        sketch_entities,
                    )?,
                ),
                (None, Some(known)) => (
                    unique_profile_line_distance_entity(
                        sketch,
                        &known,
                        parameter,
                        sketch_entities,
                    )?,
                    known,
                ),
                (None, None) => {
                    unique_profile_line_distance_pair(sketch, parameter, sketch_entities)?
                }
            };
            if first == second {
                return None;
            }
            let cadmpeg_ir::features::ParameterValue::Length(expected) =
                parameter.value.as_ref()?
            else {
                return None;
            };
            let first_line = sketch_entities.iter().find(|entity| entity.id == first)?;
            let second_line = sketch_entities.iter().find(|entity| entity.id == second)?;
            if !line_line_distance(first_line, second_line)
                .is_some_and(|measured| same_dimension_length(measured, expected.0))
            {
                (first, second) = unique_repaired_profile_line_distance_pair(
                    sketch,
                    &first,
                    &second,
                    parameter,
                    sketch_entities,
                )?;
            }
            Some(SketchConstraintDefinition::Distance {
                entities: vec![first, second],
                parameter: parameter_id,
            })
        }
        Angle => {
            let curve = |index| {
                marker(index).and_then(|marker| {
                    single_marker_line_entity(
                        marker,
                        markers_by_id,
                        loci_by_marker,
                        sketch_entities,
                    )
                })
            };
            let first = curve(0);
            let second = curve(1);
            let (mut first, mut second) = match (first, second) {
                (Some(first), Some(second)) => (first, second),
                (Some(known), None) => (
                    known.clone(),
                    unique_profile_line_angle_entity(sketch, &known, parameter, sketch_entities)?,
                ),
                (None, Some(known)) => (
                    unique_profile_line_angle_entity(sketch, &known, parameter, sketch_entities)?,
                    known,
                ),
                (None, None) => unique_profile_line_angle_pair(sketch, parameter, sketch_entities)?,
            };
            if first == second {
                return None;
            }
            let cadmpeg_ir::features::ParameterValue::Angle(expected) = parameter.value.as_ref()?
            else {
                return None;
            };
            let first_line = sketch_entities.iter().find(|entity| entity.id == first)?;
            let second_line = sketch_entities.iter().find(|entity| entity.id == second)?;
            if !line_line_angle(first_line, second_line)
                .is_some_and(|measured| same_dimension_angle(measured, expected.0))
            {
                (first, second) = unique_repaired_profile_line_angle_pair(
                    sketch,
                    &first,
                    &second,
                    parameter,
                    sketch_entities,
                )?;
            }
            Some(SketchConstraintDefinition::Angle {
                first,
                second,
                parameter: parameter_id,
            })
        }
        CircleDiameter => {
            let entity = sketch_entities
                .iter()
                .find(|entity| {
                    entity.sketch == *sketch
                        && entity.geometry_ref.as_deref() == Some(relation.id.as_str())
                        && matches!(entity.geometry, SketchGeometry::Circle { .. })
                })
                .map(|entity| entity.id.clone())
                .or_else(|| {
                    marker(0)
                        .and_then(|marker| {
                            if sketch_entities.is_empty() {
                                single_marker_entity(marker, markers_by_id, loci_by_marker)
                            } else {
                                single_marker_curve_entity(
                                    marker,
                                    markers_by_id,
                                    loci_by_marker,
                                    sketch_entities,
                                )
                            }
                        })
                        .or_else(|| {
                            unique_dimensioned_circle_entity(sketch, sketch_entities, parameter)
                        })
                })?;
            if !sketch_entities.is_empty() {
                let cadmpeg_ir::features::ParameterValue::Length(expected) =
                    parameter.value.as_ref()?
                else {
                    return None;
                };
                let geometry = &sketch_entities
                    .iter()
                    .find(|candidate| candidate.id == entity)?
                    .geometry;
                let radius = match geometry {
                    SketchGeometry::Circle { radius, .. } | SketchGeometry::Arc { radius, .. } => {
                        radius.0
                    }
                    _ => return None,
                };
                let expected_radius = match parameter.display {
                    Some(cadmpeg_ir::features::DimensionDisplay::Radius) => expected.0,
                    Some(cadmpeg_ir::features::DimensionDisplay::Diameter) => expected.0 * 0.5,
                    None => return None,
                };
                if !same_dimension_length(radius, expected_radius) {
                    return None;
                }
            }
            match parameter.display {
                Some(cadmpeg_ir::features::DimensionDisplay::Radius) => {
                    Some(SketchConstraintDefinition::Radius {
                        entity,
                        parameter: parameter_id,
                    })
                }
                Some(cadmpeg_ir::features::DimensionDisplay::Diameter) => {
                    Some(SketchConstraintDefinition::Diameter {
                        entity,
                        parameter: parameter_id,
                    })
                }
                None => None,
            }
        }
    }
}

fn unique_profile_distance_locus(
    sketch: &SketchId,
    known: &SketchLocus,
    parameter: &cadmpeg_ir::features::DesignParameter,
    sketch_entities: &[SketchEntity],
) -> Option<SketchLocus> {
    let cadmpeg_ir::features::ParameterValue::Length(distance) = parameter.value.as_ref()? else {
        return None;
    };
    let point = |locus: &SketchLocus| {
        let entity = sketch_entities
            .iter()
            .find(|entity| entity.id == locus_entity(locus))?;
        sketch_entity_loci(entity)
            .into_iter()
            .find_map(|(point, candidate)| (candidate == *locus).then_some(point))
    };
    let known_point = point(known)?;
    let mut candidates = canonical_profile_loci(sketch, sketch_entities)
        .into_iter()
        .filter_map(|(candidate_point, candidate)| {
            let measured =
                (candidate_point.u - known_point.u).hypot(candidate_point.v - known_point.v);
            (candidate != *known && same_dimension_length(measured, distance.0))
                .then_some(candidate)
        })
        .collect::<Vec<_>>();
    candidates.sort_by(|left, right| locus_key(left).cmp(&locus_key(right)));
    candidates.dedup();
    let mut candidates = candidates.into_iter();
    let candidate = candidates.next()?;
    candidates.next().is_none().then_some(candidate)
}

fn unique_repaired_profile_distance_loci_pair(
    sketch: &SketchId,
    first: &SketchLocus,
    second: &SketchLocus,
    parameter: &cadmpeg_ir::features::DesignParameter,
    sketch_entities: &[SketchEntity],
) -> Option<(SketchLocus, SketchLocus)> {
    let mut candidates = [first, second]
        .into_iter()
        .filter_map(|known| {
            let partner = unique_profile_distance_locus(sketch, known, parameter, sketch_entities)?;
            let mut pair = [known.clone(), partner];
            pair.sort_by(|left, right| locus_key(left).cmp(&locus_key(right)));
            Some((pair[0].clone(), pair[1].clone()))
        })
        .collect::<Vec<_>>();
    candidates.sort_by(|(first_left, second_left), (first_right, second_right)| {
        locus_key(first_left)
            .cmp(&locus_key(first_right))
            .then_with(|| locus_key(second_left).cmp(&locus_key(second_right)))
    });
    candidates.dedup();
    let [candidate] = candidates.as_slice() else {
        return None;
    };
    Some(candidate.clone())
}

fn unique_profile_axis_distance_locus(
    sketch: &SketchId,
    known: &SketchLocus,
    parameter: &cadmpeg_ir::features::DesignParameter,
    sketch_entities: &[SketchEntity],
    horizontal: bool,
) -> Option<SketchLocus> {
    let cadmpeg_ir::features::ParameterValue::Length(distance) = parameter.value.as_ref()? else {
        return None;
    };
    let known_point = profile_locus_point(known, sketch_entities)?;
    let mut candidates = canonical_profile_loci(sketch, sketch_entities)
        .into_iter()
        .filter_map(|(candidate_point, candidate)| {
            let measured = if horizontal {
                (candidate_point.u - known_point.u).abs()
            } else {
                (candidate_point.v - known_point.v).abs()
            };
            (candidate != *known && same_dimension_length(measured, distance.0))
                .then_some(candidate)
        })
        .collect::<Vec<_>>();
    candidates.sort_by(|left, right| locus_key(left).cmp(&locus_key(right)));
    candidates.dedup();
    let [candidate] = candidates.as_slice() else {
        return None;
    };
    Some(candidate.clone())
}

fn unique_repaired_profile_axis_distance_pair(
    sketch: &SketchId,
    first: &SketchLocus,
    second: &SketchLocus,
    parameter: &cadmpeg_ir::features::DesignParameter,
    sketch_entities: &[SketchEntity],
    horizontal: bool,
) -> Option<(SketchLocus, SketchLocus)> {
    let mut candidates = [first, second]
        .into_iter()
        .filter_map(|known| {
            let partner = unique_profile_axis_distance_locus(
                sketch,
                known,
                parameter,
                sketch_entities,
                horizontal,
            )?;
            let mut pair = [known.clone(), partner];
            pair.sort_by(|left, right| locus_key(left).cmp(&locus_key(right)));
            Some((pair[0].clone(), pair[1].clone()))
        })
        .collect::<Vec<_>>();
    candidates.sort_by(|(first_left, second_left), (first_right, second_right)| {
        locus_key(first_left)
            .cmp(&locus_key(first_right))
            .then_with(|| locus_key(second_left).cmp(&locus_key(second_right)))
    });
    candidates.dedup();
    let [candidate] = candidates.as_slice() else {
        return None;
    };
    Some(candidate.clone())
}

fn unique_profile_axis_distance_pair(
    sketch: &SketchId,
    parameter: &cadmpeg_ir::features::DesignParameter,
    sketch_entities: &[SketchEntity],
    horizontal: bool,
) -> Option<(SketchLocus, SketchLocus)> {
    let cadmpeg_ir::features::ParameterValue::Length(distance) = parameter.value.as_ref()? else {
        return None;
    };
    let loci = canonical_profile_loci(sketch, sketch_entities);
    let mut candidates = Vec::new();
    for (first_index, (first_point, first)) in loci.iter().enumerate() {
        for (second_point, second) in &loci[first_index + 1..] {
            let measured = if horizontal {
                (second_point.u - first_point.u).abs()
            } else {
                (second_point.v - first_point.v).abs()
            };
            if same_dimension_length(measured, distance.0) {
                candidates.push((first.clone(), second.clone()));
            }
        }
    }
    candidates.sort_by(|(first_left, second_left), (first_right, second_right)| {
        locus_key(first_left)
            .cmp(&locus_key(first_right))
            .then_with(|| locus_key(second_left).cmp(&locus_key(second_right)))
    });
    candidates.dedup();
    let [candidate] = candidates.as_slice() else {
        return None;
    };
    Some(candidate.clone())
}

fn unique_profile_distance_loci_pair(
    sketch: &SketchId,
    parameter: &cadmpeg_ir::features::DesignParameter,
    sketch_entities: &[SketchEntity],
) -> Option<(SketchLocus, SketchLocus)> {
    let cadmpeg_ir::features::ParameterValue::Length(distance) = parameter.value.as_ref()? else {
        return None;
    };
    let loci = canonical_profile_loci(sketch, sketch_entities);
    let mut candidates = Vec::new();
    for (first_index, (first_point, first)) in loci.iter().enumerate() {
        for (second_point, second) in &loci[first_index + 1..] {
            let measured = (second_point.u - first_point.u).hypot(second_point.v - first_point.v);
            if same_dimension_length(measured, distance.0) {
                candidates.push((first.clone(), second.clone()));
            }
        }
    }
    candidates.sort_by(|(first_left, second_left), (first_right, second_right)| {
        locus_key(first_left)
            .cmp(&locus_key(first_right))
            .then_with(|| locus_key(second_left).cmp(&locus_key(second_right)))
    });
    candidates.dedup();
    let [candidate] = candidates.as_slice() else {
        return None;
    };
    Some(candidate.clone())
}

fn canonical_profile_loci(
    sketch: &SketchId,
    sketch_entities: &[SketchEntity],
) -> Vec<(Point2, SketchLocus)> {
    const QUANTUM: f64 = 1.0e-8;
    let mut loci = sketch_entities
        .iter()
        .filter(|entity| entity.sketch == *sketch)
        .flat_map(sketch_entity_loci)
        .collect::<Vec<_>>();
    loci.sort_by(|(left_point, left_locus), (right_point, right_locus)| {
        quantize(*left_point, QUANTUM)
            .cmp(&quantize(*right_point, QUANTUM))
            .then_with(|| locus_key(left_locus).cmp(&locus_key(right_locus)))
    });
    loci.dedup_by(|(left_point, _), (right_point, _)| {
        quantize(*left_point, QUANTUM) == quantize(*right_point, QUANTUM)
    });
    loci
}

fn unique_profile_line_distance_entity(
    sketch: &SketchId,
    known: &SketchEntityId,
    parameter: &cadmpeg_ir::features::DesignParameter,
    sketch_entities: &[SketchEntity],
) -> Option<SketchEntityId> {
    let cadmpeg_ir::features::ParameterValue::Length(distance) = parameter.value.as_ref()? else {
        return None;
    };
    let known = sketch_entities.iter().find(|entity| entity.id == *known)?;
    let mut candidates = sketch_entities
        .iter()
        .filter(|entity| entity.sketch == *sketch && entity.id != known.id)
        .filter_map(|candidate| {
            line_line_distance(known, candidate)
                .filter(|measured| same_dimension_length(*measured, distance.0))
                .map(|_| candidate.id.clone())
        })
        .collect::<Vec<_>>();
    candidates.sort();
    candidates.dedup();
    let [candidate] = candidates.as_slice() else {
        return None;
    };
    Some(candidate.clone())
}

fn unique_profile_line_distance_pair(
    sketch: &SketchId,
    parameter: &cadmpeg_ir::features::DesignParameter,
    sketch_entities: &[SketchEntity],
) -> Option<(SketchEntityId, SketchEntityId)> {
    let cadmpeg_ir::features::ParameterValue::Length(distance) = parameter.value.as_ref()? else {
        return None;
    };
    let lines = sketch_entities
        .iter()
        .filter(|entity| entity.sketch == *sketch)
        .filter(|entity| matches!(entity.geometry, SketchGeometry::Line { .. }))
        .collect::<Vec<_>>();
    let mut candidates = Vec::new();
    for (first_index, first) in lines.iter().enumerate() {
        for second in &lines[first_index + 1..] {
            if line_line_distance(first, second)
                .is_some_and(|measured| same_dimension_length(measured, distance.0))
            {
                candidates.push((first.id.clone(), second.id.clone()));
            }
        }
    }
    candidates.sort();
    candidates.dedup();
    let [candidate] = candidates.as_slice() else {
        return None;
    };
    Some(candidate.clone())
}

fn unique_repaired_profile_line_distance_pair(
    sketch: &SketchId,
    first: &SketchEntityId,
    second: &SketchEntityId,
    parameter: &cadmpeg_ir::features::DesignParameter,
    sketch_entities: &[SketchEntity],
) -> Option<(SketchEntityId, SketchEntityId)> {
    let mut candidates = [first, second]
        .into_iter()
        .filter_map(|known| {
            let partner =
                unique_profile_line_distance_entity(sketch, known, parameter, sketch_entities)?;
            let mut pair = [known.clone(), partner];
            pair.sort();
            Some((pair[0].clone(), pair[1].clone()))
        })
        .collect::<Vec<_>>();
    candidates.sort();
    candidates.dedup();
    let [candidate] = candidates.as_slice() else {
        return None;
    };
    Some(candidate.clone())
}

fn line_line_distance(first: &SketchEntity, second: &SketchEntity) -> Option<f64> {
    let SketchGeometry::Line {
        start: first_start,
        end: first_end,
    } = &first.geometry
    else {
        return None;
    };
    let SketchGeometry::Line {
        start: second_start,
        end: second_end,
    } = &second.geometry
    else {
        return None;
    };
    let first_direction = [first_end.u - first_start.u, first_end.v - first_start.v];
    let second_direction = [second_end.u - second_start.u, second_end.v - second_start.v];
    let first_length = first_direction[0].hypot(first_direction[1]);
    let second_length = second_direction[0].hypot(second_direction[1]);
    if first_length <= SKETCH_POINT_TOLERANCE || second_length <= SKETCH_POINT_TOLERANCE {
        return None;
    }
    let cross = |left: [f64; 2], right: [f64; 2]| left[0] * right[1] - left[1] * right[0];
    if cross(first_direction, second_direction).abs()
        > SKETCH_POINT_TOLERANCE * first_length * second_length
    {
        return None;
    }
    Some(
        cross(
            [
                second_start.u - first_start.u,
                second_start.v - first_start.v,
            ],
            first_direction,
        )
        .abs()
            / first_length,
    )
}

fn unique_profile_line_angle_entity(
    sketch: &SketchId,
    known: &SketchEntityId,
    parameter: &cadmpeg_ir::features::DesignParameter,
    sketch_entities: &[SketchEntity],
) -> Option<SketchEntityId> {
    let cadmpeg_ir::features::ParameterValue::Angle(angle) = parameter.value.as_ref()? else {
        return None;
    };
    let known = sketch_entities.iter().find(|entity| entity.id == *known)?;
    let mut candidates = sketch_entities
        .iter()
        .filter(|entity| entity.sketch == *sketch && entity.id != known.id)
        .filter_map(|candidate| {
            line_line_angle(known, candidate)
                .filter(|measured| same_dimension_angle(*measured, angle.0))
                .map(|_| candidate.id.clone())
        })
        .collect::<Vec<_>>();
    candidates.sort();
    candidates.dedup();
    let [candidate] = candidates.as_slice() else {
        return None;
    };
    Some(candidate.clone())
}

fn unique_profile_line_angle_pair(
    sketch: &SketchId,
    parameter: &cadmpeg_ir::features::DesignParameter,
    sketch_entities: &[SketchEntity],
) -> Option<(SketchEntityId, SketchEntityId)> {
    let cadmpeg_ir::features::ParameterValue::Angle(angle) = parameter.value.as_ref()? else {
        return None;
    };
    let lines = sketch_entities
        .iter()
        .filter(|entity| entity.sketch == *sketch)
        .filter(|entity| matches!(entity.geometry, SketchGeometry::Line { .. }))
        .collect::<Vec<_>>();
    let mut candidates = Vec::new();
    for (first_index, first) in lines.iter().enumerate() {
        for second in &lines[first_index + 1..] {
            if line_line_angle(first, second)
                .is_some_and(|measured| same_dimension_angle(measured, angle.0))
            {
                candidates.push((first.id.clone(), second.id.clone()));
            }
        }
    }
    candidates.sort();
    candidates.dedup();
    let [candidate] = candidates.as_slice() else {
        return None;
    };
    Some(candidate.clone())
}

fn unique_repaired_profile_line_angle_pair(
    sketch: &SketchId,
    first: &SketchEntityId,
    second: &SketchEntityId,
    parameter: &cadmpeg_ir::features::DesignParameter,
    sketch_entities: &[SketchEntity],
) -> Option<(SketchEntityId, SketchEntityId)> {
    let mut candidates = [first, second]
        .into_iter()
        .filter_map(|known| {
            let partner =
                unique_profile_line_angle_entity(sketch, known, parameter, sketch_entities)?;
            let mut pair = [known.clone(), partner];
            pair.sort();
            Some((pair[0].clone(), pair[1].clone()))
        })
        .collect::<Vec<_>>();
    candidates.sort();
    candidates.dedup();
    let [candidate] = candidates.as_slice() else {
        return None;
    };
    Some(candidate.clone())
}

fn line_line_angle(first: &SketchEntity, second: &SketchEntity) -> Option<f64> {
    let SketchGeometry::Line {
        start: first_start,
        end: first_end,
    } = &first.geometry
    else {
        return None;
    };
    let SketchGeometry::Line {
        start: second_start,
        end: second_end,
    } = &second.geometry
    else {
        return None;
    };
    let first_direction = [first_end.u - first_start.u, first_end.v - first_start.v];
    let second_direction = [second_end.u - second_start.u, second_end.v - second_start.v];
    let first_length = first_direction[0].hypot(first_direction[1]);
    let second_length = second_direction[0].hypot(second_direction[1]);
    if first_length <= SKETCH_POINT_TOLERANCE || second_length <= SKETCH_POINT_TOLERANCE {
        return None;
    }
    Some(
        ((first_direction[0] * second_direction[0] + first_direction[1] * second_direction[1])
            / (first_length * second_length))
            .clamp(-1.0, 1.0)
            .acos(),
    )
}

fn same_dimension_angle(left: f64, right: f64) -> bool {
    (left - right).abs() <= 1.0e-9 * left.abs().max(right.abs()).max(1.0)
}

fn unique_profile_point_line_entity(
    sketch: &SketchId,
    point: &SketchLocus,
    parameter: &cadmpeg_ir::features::DesignParameter,
    sketch_entities: &[SketchEntity],
) -> Option<SketchEntityId> {
    let cadmpeg_ir::features::ParameterValue::Length(distance) = parameter.value.as_ref()? else {
        return None;
    };
    let point = profile_locus_point(point, sketch_entities)?;
    let mut candidates = sketch_entities
        .iter()
        .filter(|entity| entity.sketch == *sketch)
        .filter_map(|line| {
            point_line_distance_value(point, line)
                .filter(|measured| same_dimension_length(*measured, distance.0))
                .map(|_| line.id.clone())
        })
        .collect::<Vec<_>>();
    candidates.sort();
    candidates.dedup();
    let [candidate] = candidates.as_slice() else {
        return None;
    };
    Some(candidate.clone())
}

fn unique_profile_line_point_locus(
    sketch: &SketchId,
    line: &SketchEntityId,
    parameter: &cadmpeg_ir::features::DesignParameter,
    sketch_entities: &[SketchEntity],
) -> Option<SketchLocus> {
    let cadmpeg_ir::features::ParameterValue::Length(distance) = parameter.value.as_ref()? else {
        return None;
    };
    let line = sketch_entities.iter().find(|entity| entity.id == *line)?;
    let mut candidates = sketch_entities
        .iter()
        .filter(|entity| entity.sketch == *sketch)
        .flat_map(sketch_entity_loci)
        .filter_map(|(point, locus)| {
            point_line_distance_value(point, line)
                .filter(|measured| same_dimension_length(*measured, distance.0))
                .map(|_| locus)
        })
        .collect::<Vec<_>>();
    candidates.sort_by(|left, right| locus_key(left).cmp(&locus_key(right)));
    candidates.dedup();
    let [candidate] = candidates.as_slice() else {
        return None;
    };
    Some(candidate.clone())
}

fn unique_profile_point_line_pair(
    sketch: &SketchId,
    parameter: &cadmpeg_ir::features::DesignParameter,
    sketch_entities: &[SketchEntity],
) -> Option<(SketchLocus, SketchEntityId)> {
    let cadmpeg_ir::features::ParameterValue::Length(distance) = parameter.value.as_ref()? else {
        return None;
    };
    let loci = sketch_entities
        .iter()
        .filter(|entity| entity.sketch == *sketch)
        .flat_map(sketch_entity_loci)
        .collect::<Vec<_>>();
    let lines = sketch_entities
        .iter()
        .filter(|entity| entity.sketch == *sketch)
        .filter(|entity| matches!(entity.geometry, SketchGeometry::Line { .. }))
        .collect::<Vec<_>>();
    let mut candidates = Vec::new();
    for (point, locus) in loci {
        for line in &lines {
            if point_line_distance_value(point, line)
                .is_some_and(|measured| same_dimension_length(measured, distance.0))
            {
                candidates.push((locus.clone(), line.id.clone()));
            }
        }
    }
    candidates.sort_by(|(left_locus, left_line), (right_locus, right_line)| {
        locus_key(left_locus)
            .cmp(&locus_key(right_locus))
            .then_with(|| left_line.cmp(right_line))
    });
    candidates.dedup();
    let [candidate] = candidates.as_slice() else {
        return None;
    };
    Some(candidate.clone())
}

fn unique_repaired_profile_point_line_pair(
    sketch: &SketchId,
    point: &SketchLocus,
    line: &SketchEntityId,
    parameter: &cadmpeg_ir::features::DesignParameter,
    sketch_entities: &[SketchEntity],
) -> Option<(SketchLocus, SketchEntityId)> {
    let mut candidates = Vec::new();
    if let Some(candidate_line) =
        unique_profile_point_line_entity(sketch, point, parameter, sketch_entities)
    {
        candidates.push((point.clone(), candidate_line));
    }
    if let Some(candidate_point) =
        unique_profile_line_point_locus(sketch, line, parameter, sketch_entities)
    {
        candidates.push((candidate_point, line.clone()));
    }
    candidates.sort_by(|(left_point, left_line), (right_point, right_line)| {
        locus_key(left_point)
            .cmp(&locus_key(right_point))
            .then_with(|| left_line.cmp(right_line))
    });
    candidates.dedup();
    let [candidate] = candidates.as_slice() else {
        return None;
    };
    Some(candidate.clone())
}

fn profile_locus_point(locus: &SketchLocus, sketch_entities: &[SketchEntity]) -> Option<Point2> {
    let entity = sketch_entities
        .iter()
        .find(|entity| entity.id == locus_entity(locus))?;
    sketch_entity_loci(entity)
        .into_iter()
        .find_map(|(point, candidate)| (candidate == *locus).then_some(point))
}

fn canonicalize_physical_loci(
    loci: &mut Vec<SketchLocus>,
    sketch_entities: &[SketchEntity],
    quantum: f64,
) {
    if loci.len() < 2 {
        return;
    }
    let points = loci
        .iter()
        .map(|locus| {
            profile_locus_point(locus, sketch_entities).map(|point| quantize(point, quantum))
        })
        .collect::<Option<Vec<_>>>();
    let Some(points) = points else {
        return;
    };
    if points.iter().all(|point| *point == points[0]) {
        loci.sort_by(|left, right| locus_key(left).cmp(&locus_key(right)));
        loci.truncate(1);
    }
}

fn point_line_distance_value(point: Point2, line: &SketchEntity) -> Option<f64> {
    let SketchGeometry::Line { start, end } = &line.geometry else {
        return None;
    };
    let direction = [end.u - start.u, end.v - start.v];
    let length = direction[0].hypot(direction[1]);
    (length > SKETCH_POINT_TOLERANCE).then(|| {
        ((point.u - start.u) * direction[1] - (point.v - start.v) * direction[0]).abs() / length
    })
}

fn relation_operand_marker<'a>(
    relation: &'a FeatureInputRelationInstance,
    index: usize,
    sketch: &SketchId,
    markers_by_id: &HashMap<&str, &'a SketchInputEntity>,
) -> Option<&'a str> {
    let operand = relation.operands.get(index)?;
    if sketch.0.contains("sketch#compact:") && operand.kind == FeatureInputOperandKind::D6 {
        let mut coordinate_handles = markers_by_id
            .values()
            .copied()
            .filter(|marker| marker.feature_ref.as_deref() == Some(&relation.feature_ref))
            .filter(|marker| marker.coordinates_m.is_some())
            .collect::<Vec<_>>();
        coordinate_handles.sort_unstable_by_key(|marker| marker.offset);
        return coordinate_handles
            .get(usize::from(operand.entity_index))
            .map(|marker| marker.id.as_str());
    }
    operand.entity_ref.as_deref()
}

fn unique_dimensioned_circle_entity(
    sketch: &SketchId,
    sketch_entities: &[SketchEntity],
    parameter: &cadmpeg_ir::features::DesignParameter,
) -> Option<SketchEntityId> {
    let cadmpeg_ir::features::ParameterValue::Length(value) = parameter.value.as_ref()? else {
        return None;
    };
    let expected_radius = match parameter.display {
        Some(cadmpeg_ir::features::DimensionDisplay::Radius) => value.0,
        Some(cadmpeg_ir::features::DimensionDisplay::Diameter) => value.0 * 0.5,
        None => return None,
    };
    let mut matches = sketch_entities.iter().filter_map(|entity| {
        if entity.sketch != *sketch {
            return None;
        }
        let radius = match &entity.geometry {
            SketchGeometry::Circle { radius, .. } | SketchGeometry::Arc { radius, .. } => radius.0,
            _ => return None,
        };
        same_dimension_length(radius, expected_radius).then_some(entity.id.clone())
    });
    let first = matches.next()?;
    matches.next().is_none().then_some(first)
}

fn same_dimension_length(left: f64, right: f64) -> bool {
    (left - right).abs() <= 1.0e-9 * left.abs().max(right.abs()).max(1.0)
}

fn marker_point_locus(
    marker_id: &str,
    markers_by_id: &HashMap<&str, &SketchInputEntity>,
    loci_by_marker: &HashMap<String, Vec<SketchLocus>>,
) -> Option<SketchLocus> {
    if let Some(locus) = loci_by_marker
        .get(&qualified_point_marker_key(marker_id))
        .and_then(|loci| unique_locus(loci))
    {
        return Some(locus);
    }
    resolved_marker_locus(
        marker_id,
        markers_by_id,
        loci_by_marker,
        &mut HashSet::new(),
    )
}

fn qualified_point_marker_key(marker_id: &str) -> String {
    format!("{marker_id}:qualified-point")
}

fn resolved_marker_locus(
    marker_id: &str,
    markers_by_id: &HashMap<&str, &SketchInputEntity>,
    loci_by_marker: &HashMap<String, Vec<SketchLocus>>,
    visited: &mut HashSet<String>,
) -> Option<SketchLocus> {
    if let Some(locus) = loci_by_marker
        .get(marker_id)
        .and_then(|loci| unique_locus(loci))
    {
        return Some(locus);
    }
    if !visited.insert(marker_id.to_string()) {
        return None;
    }
    let marker = markers_by_id.get(marker_id)?;
    let mut linked = marker
        .links
        .iter()
        .filter(|link| link.entity_ref != marker_id)
        .filter(|link| {
            !matches!(
                markers_by_id
                    .get(link.entity_ref.as_str())
                    .map(|marker| marker.kind),
                Some(SketchInputKind::Relation(_))
            )
        })
        .filter_map(|link| {
            resolved_marker_locus(
                &link.entity_ref,
                markers_by_id,
                loci_by_marker,
                &mut visited.clone(),
            )
        })
        .collect::<Vec<_>>();
    linked.sort_by(|left, right| locus_key(left).cmp(&locus_key(right)));
    linked.dedup();
    unique_locus(&linked)
}

fn unique_locus(loci: &[SketchLocus]) -> Option<SketchLocus> {
    let [locus] = loci else {
        return None;
    };
    Some(locus.clone())
}

fn single_marker_entity(
    marker_id: &str,
    markers_by_id: &HashMap<&str, &SketchInputEntity>,
    loci_by_marker: &HashMap<String, Vec<SketchLocus>>,
) -> Option<SketchEntityId> {
    let entities = marker_entities(marker_id, markers_by_id, loci_by_marker);
    let [entity] = entities.as_slice() else {
        return None;
    };
    Some(entity.clone())
}

fn single_marker_curve_entity(
    marker_id: &str,
    markers_by_id: &HashMap<&str, &SketchInputEntity>,
    loci_by_marker: &HashMap<String, Vec<SketchLocus>>,
    sketch_entities: &[SketchEntity],
) -> Option<SketchEntityId> {
    let mut entities = marker_entities(marker_id, markers_by_id, loci_by_marker)
        .into_iter()
        .filter(|id| {
            sketch_entities
                .iter()
                .find(|entity| entity.id == *id)
                .is_some_and(|entity| {
                    matches!(
                        entity.geometry,
                        SketchGeometry::Line { .. }
                            | SketchGeometry::Circle { .. }
                            | SketchGeometry::Arc { .. }
                    )
                })
        })
        .collect::<Vec<_>>();
    entities.sort();
    entities.dedup();
    let [entity] = entities.as_slice() else {
        return None;
    };
    Some(entity.clone())
}

fn single_marker_line_entity(
    marker_id: &str,
    markers_by_id: &HashMap<&str, &SketchInputEntity>,
    loci_by_marker: &HashMap<String, Vec<SketchLocus>>,
    sketch_entities: &[SketchEntity],
) -> Option<SketchEntityId> {
    let mut entities = marker_entities(marker_id, markers_by_id, loci_by_marker)
        .into_iter()
        .filter(|id| {
            sketch_entities
                .iter()
                .find(|entity| entity.id == *id)
                .is_some_and(|entity| matches!(entity.geometry, SketchGeometry::Line { .. }))
        })
        .collect::<Vec<_>>();
    entities.sort();
    entities.dedup();
    let [entity] = entities.as_slice() else {
        return None;
    };
    Some(entity.clone())
}

fn profile_loci_by_marker(
    features: &[cadmpeg_ir::features::Feature],
    sketches: &[cadmpeg_ir::sketches::Sketch],
    sketch_entities: &[SketchEntity],
    lanes: &[FeatureInputLane],
) -> HashMap<String, Vec<SketchLocus>> {
    const NATIVE_TO_IR: f64 = 1000.0;
    const QUANTUM: f64 = 1.0e-8;
    let qualified_point_markers = lanes
        .iter()
        .flat_map(|lane| &lane.relation_instances)
        .flat_map(|relation| &relation.operands)
        .filter(|operand| {
            matches!(
                operand.kind,
                FeatureInputOperandKind::Native(0x837b | 0xbc7c)
            )
        })
        .filter_map(|operand| operand.entity_ref.as_deref())
        .collect::<HashSet<_>>();

    let sketches_by_feature = features
        .iter()
        .filter_map(|feature| {
            let cadmpeg_ir::features::FeatureDefinition::Sketch {
                sketch: Some(sketch),
                ..
            } = &feature.definition
            else {
                return None;
            };
            Some((feature.native_ref.as_deref()?, sketch))
        })
        .collect::<HashMap<_, _>>();
    let mut profile_loci = HashMap::<&SketchId, Vec<(Point2, SketchLocus)>>::new();
    let mut line_midpoints = HashMap::<&SketchId, Vec<(Point2, SketchLocus)>>::new();
    let geometry_by_entity = sketch_entities
        .iter()
        .map(|entity| (&entity.id, &entity.geometry))
        .collect::<HashMap<_, _>>();
    let transforms =
        marker_transform_candidates_by_feature(features, sketches, sketch_entities, lanes);
    let markers_by_id = lanes
        .iter()
        .flat_map(|lane| &lane.sketch_entities)
        .map(|marker| (marker.id.as_str(), marker))
        .collect::<HashMap<_, _>>();
    for entity in sketch_entities {
        for (point, locus) in sketch_entity_loci(entity) {
            profile_loci
                .entry(&entity.sketch)
                .or_default()
                .push((point, locus));
        }
        if let SketchGeometry::Line { start, end } = &entity.geometry {
            line_midpoints.entry(&entity.sketch).or_default().push((
                Point2::new((start.u + end.u) * 0.5, (start.v + end.v) * 0.5),
                SketchLocus::Entity(entity.id.clone()),
            ));
        }
    }
    let mut result = sketch_entities
        .iter()
        .filter_map(|entity| {
            let (marker, qualified_point) = if let Some(marker) = entity.native_ref.as_ref() {
                (marker, false)
            } else {
                (
                    entity.geometry_ref.as_ref().filter(|reference| {
                        reference.starts_with("sldprt:feature-input:sketch-entity#")
                            && matches!(entity.geometry, SketchGeometry::Point { .. })
                    })?,
                    true,
                )
            };
            markers_by_id.contains_key(marker.as_str()).then(|| {
                let locus = if entity.id.0.contains("sketch-entity#compact:")
                    && matches!(entity.geometry, SketchGeometry::Line { .. })
                {
                    SketchLocus::Start(entity.id.clone())
                } else if markers_by_id.get(marker.as_str()).is_some_and(|marker| {
                    matches!(
                        marker.kind,
                        SketchInputKind::Point | SketchInputKind::ConstrainedPoint
                    )
                }) && matches!(
                    entity.geometry,
                    SketchGeometry::Circle { .. }
                        | SketchGeometry::Arc { .. }
                        | SketchGeometry::Ellipse { .. }
                ) {
                    SketchLocus::Center(entity.id.clone())
                } else {
                    SketchLocus::Entity(entity.id.clone())
                };
                let marker = if qualified_point {
                    qualified_point_marker_key(marker)
                } else {
                    marker.clone()
                };
                (marker, vec![locus])
            })
        })
        .collect::<HashMap<String, Vec<SketchLocus>>>();
    let mut endpoint_marker_keys = HashSet::new();
    for entity in sketch_entities {
        let [start, end] = entity.endpoint_refs.as_slice() else {
            continue;
        };
        for (marker, locus) in [
            (start, SketchLocus::Start(entity.id.clone())),
            (end, SketchLocus::End(entity.id.clone())),
        ] {
            if !markers_by_id.contains_key(marker.as_str()) {
                continue;
            }
            endpoint_marker_keys.insert(marker.clone());
            let loci = result.entry(marker.clone()).or_default();
            if !loci.contains(&locus) {
                loci.push(locus.clone());
            }
            if qualified_point_markers.contains(marker.as_str()) {
                let qualified_key = qualified_point_marker_key(marker);
                endpoint_marker_keys.insert(qualified_key.clone());
                let loci = result.entry(qualified_key).or_default();
                if !loci.contains(&locus) {
                    loci.push(locus);
                }
            }
        }
    }
    for marker in endpoint_marker_keys {
        if let Some(loci) = result.get_mut(&marker) {
            canonicalize_physical_loci(loci, sketch_entities, QUANTUM);
        }
    }
    for lane in lanes {
        let mut markers_by_feature = HashMap::<&str, Vec<&SketchInputEntity>>::new();
        for marker in &lane.sketch_entities {
            let Some(feature) = marker.feature_ref.as_deref() else {
                continue;
            };
            if marker.coordinates_m.is_some() && sketches_by_feature.contains_key(feature) {
                markers_by_feature.entry(feature).or_default().push(marker);
            }
        }
        for (feature, markers) in markers_by_feature {
            let Some(sketch) = sketches_by_feature.get(feature) else {
                continue;
            };
            let Some(loci) = profile_loci.get(sketch) else {
                continue;
            };
            let transforms = transforms
                .get(feature)
                .map(Vec::as_slice)
                .unwrap_or_default();
            let loci_by_point = loci.iter().fold(
                HashMap::<(i64, i64), Vec<SketchLocus>>::new(),
                |mut by_point, (point, locus)| {
                    by_point
                        .entry(quantize(*point, QUANTUM))
                        .or_default()
                        .push(locus.clone());
                    by_point
                },
            );
            for marker in markers {
                let qualified_point = qualified_point_markers.contains(marker.id.as_str());
                let result_key = if qualified_point {
                    qualified_point_marker_key(&marker.id)
                } else {
                    marker.id.clone()
                };
                if result.contains_key(&result_key) {
                    continue;
                }
                if qualified_point && sketch.0.contains("sketch#compact:") {
                    continue;
                }
                let Some([u, v]) = marker.coordinates_m else {
                    continue;
                };
                let primary_geometry_locus = usize::try_from(marker.offset)
                    .ok()
                    .is_some_and(|offset| marker_is_geometry_locus(&lane.native_payload, offset));
                let point = quantize(Point2::new(u * NATIVE_TO_IR, v * NATIVE_TO_IR), QUANTUM);
                let translated_points = transforms
                    .iter()
                    .filter_map(|transform| transform.apply(point))
                    .collect::<HashSet<_>>();
                let marker_loci = translated_points
                    .into_iter()
                    .filter_map(|translated| {
                        let mut marker_loci = loci_by_point
                            .get(&translated)
                            .into_iter()
                            .flatten()
                            .filter(|locus| {
                                geometry_by_entity.get(&locus_entity(locus)).is_some_and(
                                    |geometry| marker_accepts_locus(marker.kind, geometry),
                                )
                            })
                            .map(|locus| {
                                if !qualified_point
                                    && matches!(
                                        marker.kind,
                                        SketchInputKind::LineOrCircle | SketchInputKind::Arc
                                    )
                                {
                                    SketchLocus::Entity(locus_entity(locus))
                                } else {
                                    locus.clone()
                                }
                            })
                            .collect::<Vec<_>>();
                        if marker_loci.is_empty() && marker.kind == SketchInputKind::LineOrCircle {
                            marker_loci.extend(
                                line_midpoints.get(sketch).into_iter().flatten().filter_map(
                                    |(point, locus)| {
                                        (quantize(*point, QUANTUM) == translated)
                                            .then_some(locus.clone())
                                    },
                                ),
                            );
                        }
                        if marker_loci.is_empty()
                            && primary_geometry_locus
                            && marker.kind == SketchInputKind::LineOrCircle
                        {
                            marker_loci.extend(sketch_entities.iter().filter_map(|entity| {
                                if entity.sketch != **sketch {
                                    return None;
                                }
                                let SketchGeometry::Line { start, end } = &entity.geometry else {
                                    return None;
                                };
                                point_on_quantized_segment(
                                    translated,
                                    quantize(*start, QUANTUM),
                                    quantize(*end, QUANTUM),
                                )
                                .then(|| SketchLocus::Entity(entity.id.clone()))
                            }));
                        }
                        marker_loci.sort_by(|left, right| locus_key(left).cmp(&locus_key(right)));
                        marker_loci.dedup();
                        if qualified_point {
                            canonicalize_physical_loci(&mut marker_loci, sketch_entities, QUANTUM);
                        }
                        (!marker_loci.is_empty()).then_some(marker_loci)
                    })
                    .collect::<Vec<_>>();
                let Some(first) = marker_loci.first() else {
                    continue;
                };
                if !marker_loci.is_empty() && marker_loci.iter().all(|candidate| candidate == first)
                {
                    result.insert(result_key, first.clone());
                }
            }
        }
    }
    let markers_by_id = lanes
        .iter()
        .flat_map(|lane| &lane.sketch_entities)
        .map(|marker| (marker.id.as_str(), marker))
        .collect::<HashMap<_, _>>();
    for marker in markers_by_id.values().copied() {
        if marker.kind != SketchInputKind::LineOrCircle || result.contains_key(&marker.id) {
            continue;
        }
        let endpoints = line_endpoint_markers(marker, &markers_by_id);
        let (Some(feature), [first, second]) =
            (marker.feature_ref.as_deref(), endpoints.as_slice())
        else {
            continue;
        };
        let (Some(sketch_id), Some(first), Some(second)) = (
            sketches_by_feature.get(feature),
            first.coordinates_m,
            second.coordinates_m,
        ) else {
            continue;
        };
        let first_native = quantize(
            Point2::new(first[0] * NATIVE_TO_IR, first[1] * NATIVE_TO_IR),
            QUANTUM,
        );
        let second_native = quantize(
            Point2::new(second[0] * NATIVE_TO_IR, second[1] * NATIVE_TO_IR),
            QUANTUM,
        );
        let endpoint_pairs = transforms
            .get(feature)
            .into_iter()
            .flatten()
            .filter_map(|transform| {
                Some((
                    transform.apply(first_native)?,
                    transform.apply(second_native)?,
                ))
            })
            .collect::<HashSet<_>>();
        if endpoint_pairs.is_empty() {
            continue;
        }
        let mut matches = HashSet::new();
        let mut complete = true;
        for (start, end) in endpoint_pairs {
            let candidates = sketch_entities
                .iter()
                .filter(|entity| entity.sketch == **sketch_id)
                .filter_map(|entity| {
                    let SketchGeometry::Line {
                        start: candidate_start,
                        end: candidate_end,
                    } = entity.geometry
                    else {
                        return None;
                    };
                    let candidate_start = quantize(candidate_start, QUANTUM);
                    let candidate_end = quantize(candidate_end, QUANTUM);
                    ((candidate_start == start && candidate_end == end)
                        || (candidate_start == end && candidate_end == start))
                        .then_some(entity.id.clone())
                })
                .collect::<Vec<_>>();
            let [entity] = candidates.as_slice() else {
                complete = false;
                break;
            };
            matches.insert(entity.clone());
        }
        if complete {
            if let [entity] = matches.into_iter().collect::<Vec<_>>().as_slice() {
                result.insert(marker.id.clone(), vec![SketchLocus::Entity(entity.clone())]);
            }
        }
    }
    let entities_by_id = sketch_entities
        .iter()
        .map(|entity| (&entity.id, entity))
        .collect::<HashMap<_, _>>();
    loop {
        let additions = markers_by_id
            .values()
            .filter(|marker| {
                marker.coordinates_m.is_none()
                    && matches!(
                        marker.kind,
                        SketchInputKind::Point | SketchInputKind::ConstrainedPoint
                    )
                    && !result.contains_key(&marker.id)
            })
            .filter_map(|marker| {
                unique_linked_endpoint_locus(
                    marker,
                    &markers_by_id,
                    &result,
                    &entities_by_id,
                    QUANTUM,
                )
                .map(|locus| (marker.id.clone(), vec![locus]))
            })
            .collect::<Vec<_>>();
        if additions.is_empty() {
            break;
        }
        result.extend(additions);
    }
    result
}

fn unique_linked_endpoint_locus(
    marker: &SketchInputEntity,
    markers_by_id: &HashMap<&str, &SketchInputEntity>,
    loci_by_marker: &HashMap<String, Vec<SketchLocus>>,
    entities_by_id: &HashMap<&SketchEntityId, &SketchEntity>,
    quantum: f64,
) -> Option<SketchLocus> {
    if marker.links.len() < 2 {
        return None;
    }
    let mut groups = Vec::<HashMap<(i64, i64), Vec<SketchLocus>>>::new();
    let mut sketches = HashSet::new();
    for link in &marker.links {
        let entities = marker_entities(&link.entity_ref, markers_by_id, loci_by_marker);
        if entities.is_empty() {
            return None;
        }
        let mut endpoints = HashMap::<(i64, i64), Vec<SketchLocus>>::new();
        for entity_id in entities {
            let entity = entities_by_id.get(&entity_id)?;
            sketches.insert(&entity.sketch);
            for (point, locus) in sketch_entity_loci(entity) {
                if matches!(
                    locus,
                    SketchLocus::Start(_) | SketchLocus::End(_) | SketchLocus::Entity(_)
                ) {
                    endpoints
                        .entry(quantize(point, quantum))
                        .or_default()
                        .push(locus);
                }
            }
        }
        if endpoints.is_empty() {
            return None;
        }
        groups.push(endpoints);
    }
    if sketches.len() != 1 {
        return None;
    }
    let mut shared = groups[0].keys().copied().collect::<HashSet<_>>();
    for group in &groups[1..] {
        shared.retain(|point| group.contains_key(point));
    }
    let shared = shared.into_iter().collect::<Vec<_>>();
    let [point] = shared.as_slice() else {
        return None;
    };
    let mut loci = groups
        .iter()
        .flat_map(|group| group.get(point).into_iter().flatten().cloned())
        .collect::<Vec<_>>();
    loci.sort_by(|left, right| locus_key(left).cmp(&locus_key(right)));
    loci.dedup();
    loci.into_iter().next()
}

fn point_on_quantized_segment(point: (i64, i64), start: (i64, i64), end: (i64, i64)) -> bool {
    let ab = (
        i128::from(end.0) - i128::from(start.0),
        i128::from(end.1) - i128::from(start.1),
    );
    let ap = (
        i128::from(point.0) - i128::from(start.0),
        i128::from(point.1) - i128::from(start.1),
    );
    let cross = ab.0 * ap.1 - ab.1 * ap.0;
    let projection = ab.0 * ap.0 + ab.1 * ap.1;
    let squared_length = ab.0 * ab.0 + ab.1 * ab.1;
    squared_length != 0 && cross == 0 && (0..=squared_length).contains(&projection)
}

fn marker_transform_candidates_by_feature(
    features: &[cadmpeg_ir::features::Feature],
    sketches: &[cadmpeg_ir::sketches::Sketch],
    sketch_entities: &[SketchEntity],
    lanes: &[FeatureInputLane],
) -> HashMap<String, Vec<MarkerTransform>> {
    const NATIVE_TO_IR: f64 = 1000.0;
    const QUANTUM: f64 = 1.0e-8;

    let sketches_by_feature = features
        .iter()
        .filter_map(|feature| {
            let cadmpeg_ir::features::FeatureDefinition::Sketch {
                sketch: Some(sketch),
                ..
            } = &feature.definition
            else {
                return None;
            };
            Some((feature.native_ref.as_deref()?, sketch))
        })
        .collect::<HashMap<_, _>>();
    let mut result = HashMap::new();
    for lane in lanes {
        let mut markers_by_feature = HashMap::<&str, Vec<&SketchInputEntity>>::new();
        for marker in &lane.sketch_entities {
            let Some(feature) = marker.feature_ref.as_deref() else {
                continue;
            };
            if marker.coordinates_m.is_some() && sketches_by_feature.contains_key(feature) {
                markers_by_feature.entry(feature).or_default().push(marker);
            }
        }
        for (feature, markers) in markers_by_feature {
            let Some(sketch) = sketches_by_feature.get(feature) else {
                continue;
            };
            if !sketch_entities
                .iter()
                .any(|entity| entity.sketch == **sketch)
            {
                continue;
            }
            let compatible = |primary_only: bool| {
                let mut points = HashMap::<(i64, i64), HashSet<(i64, i64)>>::new();
                for marker in &markers {
                    if !matches!(
                        marker.kind,
                        SketchInputKind::Point
                            | SketchInputKind::LineOrCircle
                            | SketchInputKind::Arc
                            | SketchInputKind::ConstrainedPoint
                    ) {
                        continue;
                    }
                    let Some([u, v]) = marker.coordinates_m else {
                        continue;
                    };
                    if primary_only
                        && usize::try_from(marker.offset).ok().is_none_or(|offset| {
                            !marker_is_geometry_locus(&lane.native_payload, offset)
                        })
                    {
                        continue;
                    }
                    let marker_point =
                        quantize(Point2::new(u * NATIVE_TO_IR, v * NATIVE_TO_IR), QUANTUM);
                    let anchors = sketch_entities
                        .iter()
                        .filter(|entity| entity.sketch == **sketch)
                        .flat_map(|entity| {
                            if primary_only {
                                sketch_entity_loci(entity)
                                    .into_iter()
                                    .filter_map(|(point, locus)| {
                                        marker_accepts_locus(marker.kind, &entity.geometry)
                                            .then_some((point, locus))
                                    })
                                    .map(|(point, _)| point)
                                    .collect::<Vec<_>>()
                            } else {
                                marker_geometry_anchors(marker.kind, &entity.geometry)
                            }
                        });
                    for point in anchors {
                        points
                            .entry(marker_point)
                            .or_default()
                            .insert(quantize(point, QUANTUM));
                    }
                }
                points
            };
            let primary = compatible_marker_transform_candidates(&compatible(true));
            let fallback = compatible_marker_transform_candidates(&compatible(false));
            let candidates = if primary.len() == 1 || fallback.is_empty() {
                primary
            } else {
                fallback
            };
            let candidates = sketches
                .iter()
                .find(|candidate| candidate.id == **sketch)
                .map_or(candidates.clone(), |sketch| {
                    select_marker_transforms_by_frame(&candidates, sketch, QUANTUM)
                });
            if !candidates.is_empty() {
                result.insert(feature.to_string(), candidates);
            }
        }
    }
    result
}

fn marker_geometry_anchors(kind: SketchInputKind, geometry: &SketchGeometry) -> Vec<Point2> {
    match (kind, geometry) {
        (
            SketchInputKind::Point | SketchInputKind::ConstrainedPoint,
            SketchGeometry::Point { position },
        ) => vec![*position],
        (
            SketchInputKind::Point | SketchInputKind::ConstrainedPoint,
            SketchGeometry::Line { start, end },
        ) => vec![*start, *end],
        (
            SketchInputKind::Point | SketchInputKind::ConstrainedPoint,
            SketchGeometry::Circle { center, .. }
            | SketchGeometry::Arc { center, .. }
            | SketchGeometry::Ellipse { center, .. },
        ) => vec![*center],
        (SketchInputKind::LineOrCircle, SketchGeometry::Line { start, end }) => {
            vec![
                *start,
                *end,
                Point2::new((start.u + end.u) * 0.5, (start.v + end.v) * 0.5),
            ]
        }
        (
            SketchInputKind::LineOrCircle,
            SketchGeometry::Circle { center, .. } | SketchGeometry::Ellipse { center, .. },
        )
        | (SketchInputKind::Arc, SketchGeometry::Arc { center, .. }) => vec![*center],
        _ => Vec::new(),
    }
}

fn marker_accepts_locus(kind: SketchInputKind, geometry: &SketchGeometry) -> bool {
    match kind {
        SketchInputKind::Arc => matches!(geometry, SketchGeometry::Arc { .. }),
        SketchInputKind::LineOrCircle => matches!(
            geometry,
            SketchGeometry::Line { .. }
                | SketchGeometry::Circle { .. }
                | SketchGeometry::Ellipse { .. }
        ),
        SketchInputKind::Point
        | SketchInputKind::ConstrainedPoint
        | SketchInputKind::Relation(_)
        | SketchInputKind::Native(_) => true,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct MarkerTransform {
    swap: bool,
    u_sign: i8,
    v_sign: i8,
    affine_matrix: Option<[i64; 4]>,
    translation: (i64, i64),
}

impl MarkerTransform {
    fn apply_axes(self, point: (i64, i64)) -> Option<(i64, i64)> {
        if let Some([uu, uv, vu, vv]) = self.affine_matrix {
            const SCALE: i128 = 1_000_000_000_000;
            let u = i128::from(uu) * i128::from(point.0) + i128::from(uv) * i128::from(point.1);
            let v = i128::from(vu) * i128::from(point.0) + i128::from(vv) * i128::from(point.1);
            let rounded = |value: i128| {
                let adjustment = if value < 0 { -(SCALE / 2) } else { SCALE / 2 };
                i64::try_from((value + adjustment) / SCALE).ok()
            };
            return Some((rounded(u)?, rounded(v)?));
        }
        let (u, v) = if self.swap { (point.1, point.0) } else { point };
        Some((
            i64::try_from(i128::from(u) * i128::from(self.u_sign)).ok()?,
            i64::try_from(i128::from(v) * i128::from(self.v_sign)).ok()?,
        ))
    }

    fn apply(self, point: (i64, i64)) -> Option<(i64, i64)> {
        let point = self.apply_axes(point)?;
        Some((
            point.0.checked_add(self.translation.0)?,
            point.1.checked_add(self.translation.1)?,
        ))
    }
}

fn sketch_frame_marker_transform(
    sketch: &cadmpeg_ir::sketches::Sketch,
    quantum: f64,
) -> Option<MarkerTransform> {
    axis_aligned_sketch_frame_marker_transform(sketch, quantum)
        .or_else(|| affine_sketch_frame_marker_transform(sketch, quantum))
}

fn axis_aligned_sketch_frame_marker_transform(
    sketch: &cadmpeg_ir::sketches::Sketch,
    quantum: f64,
) -> Option<MarkerTransform> {
    let normal = [sketch.normal.x, sketch.normal.y, sketch.normal.z];
    let u_axis = [sketch.u_axis.x, sketch.u_axis.y, sketch.u_axis.z];
    let v_axis = [
        sketch.normal.y * sketch.u_axis.z - sketch.normal.z * sketch.u_axis.y,
        sketch.normal.z * sketch.u_axis.x - sketch.normal.x * sketch.u_axis.z,
        sketch.normal.x * sketch.u_axis.y - sketch.normal.y * sketch.u_axis.x,
    ];
    let origin = [sketch.origin.x, sketch.origin.y, sketch.origin.z];
    let axis = |vector: [f64; 3]| {
        let matches = vector
            .iter()
            .enumerate()
            .filter(|(_, value)| (value.abs() - 1.0).abs() <= 1.0e-8)
            .map(|(index, value)| (index, if *value < 0.0 { -1 } else { 1 }))
            .collect::<Vec<_>>();
        let [(index, sign)] = matches.as_slice() else {
            return None;
        };
        vector
            .iter()
            .enumerate()
            .all(|(candidate, value)| candidate == *index || value.abs() <= 1.0e-8)
            .then_some((*index, *sign))
    };
    let (normal_axis, _) = axis(normal)?;
    let native_axes = (0..3)
        .filter(|candidate| *candidate != normal_axis)
        .collect::<Vec<_>>();
    let [first_native_axis, second_native_axis] = native_axes.as_slice() else {
        return None;
    };
    let (u_axis_index, u_sign) = axis(u_axis)?;
    let (v_axis_index, v_sign) = axis(v_axis)?;
    if u_axis_index == normal_axis || v_axis_index == normal_axis || u_axis_index == v_axis_index {
        return None;
    }
    let swap = match (u_axis_index, v_axis_index) {
        (u, v) if u == *first_native_axis && v == *second_native_axis => false,
        (u, v) if u == *second_native_axis && v == *first_native_axis => true,
        _ => return None,
    };
    Some(MarkerTransform {
        swap,
        u_sign,
        v_sign,
        affine_matrix: None,
        translation: (
            (-origin[u_axis_index] * f64::from(u_sign) / quantum).round() as i64,
            (-origin[v_axis_index] * f64::from(v_sign) / quantum).round() as i64,
        ),
    })
}

fn affine_sketch_frame_marker_transform(
    sketch: &cadmpeg_ir::sketches::Sketch,
    quantum: f64,
) -> Option<MarkerTransform> {
    const SCALE: f64 = 1_000_000_000_000.0;
    let normal = [sketch.normal.x, sketch.normal.y, sketch.normal.z];
    let u_axis = [sketch.u_axis.x, sketch.u_axis.y, sketch.u_axis.z];
    let v_axis = [
        sketch.normal.y * sketch.u_axis.z - sketch.normal.z * sketch.u_axis.y,
        sketch.normal.z * sketch.u_axis.x - sketch.normal.x * sketch.u_axis.z,
        sketch.normal.x * sketch.u_axis.y - sketch.normal.y * sketch.u_axis.x,
    ];
    let origin = [sketch.origin.x, sketch.origin.y, sketch.origin.z];
    if !(normal
        .into_iter()
        .chain(u_axis)
        .chain(v_axis)
        .chain(origin)
        .all(f64::is_finite)
        && quantum.is_finite()
        && quantum > 0.0)
    {
        return None;
    }
    let normal_axis =
        (0..3).max_by(|left, right| normal[*left].abs().total_cmp(&normal[*right].abs()))?;
    if normal[normal_axis].abs() <= 1.0e-8 {
        return None;
    }
    let native_axes = (0..3)
        .filter(|candidate| *candidate != normal_axis)
        .collect::<Vec<_>>();
    let [first_axis, second_axis] = native_axes.as_slice() else {
        return None;
    };
    let tangent = |axis: usize| {
        let mut value = [0.0; 3];
        value[axis] = 1.0;
        value[normal_axis] = -normal[axis] / normal[normal_axis];
        value
    };
    let dot = |left: [f64; 3], right: [f64; 3]| {
        left[0] * right[0] + left[1] * right[1] + left[2] * right[2]
    };
    let first = tangent(*first_axis);
    let second = tangent(*second_axis);
    let matrix = [
        (dot(first, u_axis) * SCALE).round() as i64,
        (dot(second, u_axis) * SCALE).round() as i64,
        (dot(first, v_axis) * SCALE).round() as i64,
        (dot(second, v_axis) * SCALE).round() as i64,
    ];
    let mut zero_world_delta = [0.0; 3];
    zero_world_delta[*first_axis] = -origin[*first_axis];
    zero_world_delta[*second_axis] = -origin[*second_axis];
    zero_world_delta[normal_axis] = -(normal[*first_axis] * zero_world_delta[*first_axis]
        + normal[*second_axis] * zero_world_delta[*second_axis])
        / normal[normal_axis];
    Some(MarkerTransform {
        swap: false,
        u_sign: 1,
        v_sign: 1,
        affine_matrix: Some(matrix),
        translation: (
            (dot(zero_world_delta, u_axis) / quantum).round() as i64,
            (dot(zero_world_delta, v_axis) / quantum).round() as i64,
        ),
    })
}

fn select_marker_transforms_by_frame(
    candidates: &[MarkerTransform],
    sketch: &cadmpeg_ir::sketches::Sketch,
    quantum: f64,
) -> Vec<MarkerTransform> {
    if let [candidate] = candidates {
        return vec![*candidate];
    }
    let frame = sketch_frame_marker_transform(sketch, quantum);
    if candidates.is_empty() {
        return frame.into_iter().collect();
    }
    let Some(frame) = frame else {
        return candidates.to_vec();
    };
    if candidates.contains(&frame) {
        return vec![frame];
    }
    if frame.affine_matrix.is_some() {
        return candidates.to_vec();
    }
    let oriented = candidates
        .iter()
        .copied()
        .filter(|candidate| {
            candidate.swap == frame.swap
                && candidate.u_sign == frame.u_sign
                && candidate.v_sign == frame.v_sign
        })
        .collect::<Vec<_>>();
    if oriented.is_empty() {
        candidates.to_vec()
    } else {
        oriented
    }
}

fn dimensioned_circle_surface_transforms(
    sketch: &cadmpeg_ir::sketches::Sketch,
    surfaces: &[cadmpeg_ir::geometry::Surface],
    circles: &[((i64, i64), i64)],
    quantum: f64,
) -> Vec<MarkerTransform> {
    use cadmpeg_ir::geometry::SurfaceGeometry;

    if circles.is_empty() {
        return Vec::new();
    }
    let v_axis = cadmpeg_ir::math::Vector3::new(
        sketch.normal.y * sketch.u_axis.z - sketch.normal.z * sketch.u_axis.y,
        sketch.normal.z * sketch.u_axis.x - sketch.normal.x * sketch.u_axis.z,
        sketch.normal.x * sketch.u_axis.y - sketch.normal.y * sketch.u_axis.x,
    );
    let mut targets_by_radius = HashMap::<i64, HashSet<(i64, i64)>>::new();
    for surface in surfaces {
        let SurfaceGeometry::Cylinder {
            origin,
            axis,
            radius,
            ..
        } = &surface.geometry
        else {
            continue;
        };
        let alignment =
            axis.x * sketch.normal.x + axis.y * sketch.normal.y + axis.z * sketch.normal.z;
        if !alignment.is_finite() || (alignment.abs() - 1.0).abs() > 1.0e-8 {
            continue;
        }
        let radius_key = (radius / quantum).round() as i64;
        if !circles
            .iter()
            .any(|(_, candidate)| *candidate == radius_key)
        {
            continue;
        }
        let delta = cadmpeg_ir::math::Vector3::new(
            origin.x - sketch.origin.x,
            origin.y - sketch.origin.y,
            origin.z - sketch.origin.z,
        );
        let center = Point2::new(
            delta.x * sketch.u_axis.x + delta.y * sketch.u_axis.y + delta.z * sketch.u_axis.z,
            delta.x * v_axis.x + delta.y * v_axis.y + delta.z * v_axis.z,
        );
        targets_by_radius
            .entry(radius_key)
            .or_default()
            .insert(quantize(center, quantum));
    }
    let compatible = circles
        .iter()
        .filter_map(|(center, radius)| Some((*center, targets_by_radius.get(radius)?.clone())))
        .collect::<HashMap<_, _>>();
    if compatible.len() != circles.len() {
        return Vec::new();
    }
    let candidates = compatible_marker_transform_candidates(&compatible);
    candidates
        .into_iter()
        .filter(|transform| {
            let mut used = HashSet::new();
            circles.iter().all(|(center, radius)| {
                transform.apply(*center).is_some_and(|center| {
                    targets_by_radius
                        .get(radius)
                        .is_some_and(|targets| targets.contains(&center))
                        && used.insert((*radius, center))
                })
            })
        })
        .collect()
}

fn dimensioned_circle_transform(
    candidates: &[MarkerTransform],
    circles: &[((i64, i64), i64)],
) -> Option<MarkerTransform> {
    let signature = |transform: MarkerTransform| {
        let mut transformed = circles
            .iter()
            .filter_map(|(center, radius)| {
                let center = transform.apply(*center)?;
                Some((center.0, center.1, *radius))
            })
            .collect::<Vec<_>>();
        transformed.sort_unstable();
        (transformed.len() == circles.len() && !transformed.is_empty()).then_some(transformed)
    };
    let first_signature = signature(*candidates.first()?)?;
    if candidates
        .iter()
        .skip(1)
        .any(|transform| signature(*transform).as_ref() != Some(&first_signature))
    {
        return None;
    }
    candidates.iter().copied().min_by_key(|transform| {
        (
            transform.swap,
            transform.u_sign,
            transform.v_sign,
            transform.affine_matrix,
            transform.translation,
        )
    })
}

#[cfg(test)]
fn unique_marker_transform(
    marker_points: &HashSet<(i64, i64)>,
    locus_points: &HashSet<(i64, i64)>,
) -> Option<MarkerTransform> {
    let identity = MarkerTransform {
        swap: false,
        u_sign: 1,
        v_sign: 1,
        affine_matrix: None,
        translation: (0, 0),
    };
    if let Some(transform) = unique_transform_translation(identity, marker_points, locus_points) {
        return Some(transform);
    }
    let mut scored = Vec::new();
    for swap in [false, true] {
        for u_sign in [-1, 1] {
            for v_sign in [-1, 1] {
                if !swap && u_sign == 1 && v_sign == 1 {
                    continue;
                }
                let transform = MarkerTransform {
                    swap,
                    u_sign,
                    v_sign,
                    affine_matrix: None,
                    translation: (0, 0),
                };
                let transformed = marker_points
                    .iter()
                    .filter_map(|point| transform.apply_axes(*point))
                    .collect::<HashSet<_>>();
                let mut translations = HashMap::<(i64, i64), usize>::new();
                for marker in &transformed {
                    for locus in locus_points {
                        let Some(translation) = locus
                            .0
                            .checked_sub(marker.0)
                            .zip(locus.1.checked_sub(marker.1))
                        else {
                            continue;
                        };
                        *translations.entry(translation).or_default() += 1;
                    }
                }
                scored.extend(translations.into_iter().map(|(translation, count)| {
                    (
                        MarkerTransform {
                            translation,
                            ..transform
                        },
                        count,
                    )
                }));
            }
        }
    }
    let maximum = scored
        .iter()
        .map(|(_, count)| *count)
        .max()
        .filter(|count| *count >= 2)?;
    let candidates = scored
        .into_iter()
        .filter_map(|(transform, count)| (count == maximum).then_some(transform))
        .collect::<Vec<_>>();
    if let [transform] = candidates.as_slice() {
        return Some(*transform);
    }
    let mut zero_translation = candidates
        .iter()
        .copied()
        .filter(|transform| transform.translation == (0, 0));
    let first = zero_translation.next()?;
    zero_translation.next().is_none().then_some(first)
}

#[cfg(test)]
fn unique_compatible_marker_transform(
    compatible_locus_points: &HashMap<(i64, i64), HashSet<(i64, i64)>>,
) -> Option<MarkerTransform> {
    let candidates = compatible_marker_transform_candidates(compatible_locus_points);
    let [transform] = candidates.as_slice() else {
        return None;
    };
    Some(*transform)
}

fn compatible_marker_transform_candidates(
    compatible_locus_points: &HashMap<(i64, i64), HashSet<(i64, i64)>>,
) -> Vec<MarkerTransform> {
    let score = |axes: MarkerTransform| {
        let mut translations = HashMap::<(i64, i64), usize>::new();
        for (marker, loci) in compatible_locus_points {
            let Some(marker) = axes.apply_axes(*marker) else {
                continue;
            };
            for locus in loci {
                let Some(translation) = locus
                    .0
                    .checked_sub(marker.0)
                    .zip(locus.1.checked_sub(marker.1))
                else {
                    continue;
                };
                *translations.entry(translation).or_default() += 1;
            }
        }
        translations
    };
    let identity = MarkerTransform {
        swap: false,
        u_sign: 1,
        v_sign: 1,
        affine_matrix: None,
        translation: (0, 0),
    };
    if let Some(transform) = unique_scored_transform(identity, score(identity)) {
        return vec![transform];
    }
    let mut scored = Vec::new();
    for swap in [false, true] {
        for u_sign in [-1, 1] {
            for v_sign in [-1, 1] {
                if !swap && u_sign == 1 && v_sign == 1 {
                    continue;
                }
                let axes = MarkerTransform {
                    swap,
                    u_sign,
                    v_sign,
                    affine_matrix: None,
                    translation: (0, 0),
                };
                scored.extend(score(axes).into_iter().map(|(translation, count)| {
                    (
                        MarkerTransform {
                            translation,
                            ..axes
                        },
                        count,
                    )
                }));
            }
        }
    }
    let Some(maximum) = scored
        .iter()
        .map(|(_, count)| *count)
        .max()
        .filter(|count| *count >= 2)
    else {
        return Vec::new();
    };
    let candidates = scored
        .into_iter()
        .filter_map(|(transform, count)| (count == maximum).then_some(transform))
        .collect::<Vec<_>>();
    if let [transform] = candidates.as_slice() {
        return vec![*transform];
    }
    let zero_translation = candidates
        .iter()
        .copied()
        .filter(|transform| transform.translation == (0, 0))
        .collect::<Vec<_>>();
    if !zero_translation.is_empty() {
        return zero_translation;
    }
    candidates
}

fn unique_scored_transform(
    axes: MarkerTransform,
    translations: HashMap<(i64, i64), usize>,
) -> Option<MarkerTransform> {
    let maximum = translations
        .values()
        .copied()
        .max()
        .filter(|count| *count >= 2)?;
    let mut candidates = translations
        .into_iter()
        .filter_map(|(translation, count)| (count == maximum).then_some(translation));
    let translation = candidates.next()?;
    candidates.next().is_none().then_some(MarkerTransform {
        translation,
        ..axes
    })
}

#[cfg(test)]
fn unique_transform_translation(
    transform: MarkerTransform,
    marker_points: &HashSet<(i64, i64)>,
    locus_points: &HashSet<(i64, i64)>,
) -> Option<MarkerTransform> {
    let transformed = marker_points
        .iter()
        .filter_map(|point| transform.apply_axes(*point))
        .collect::<HashSet<_>>();
    let mut translations = HashMap::<(i64, i64), usize>::new();
    for marker in &transformed {
        for locus in locus_points {
            let Some(translation) = locus
                .0
                .checked_sub(marker.0)
                .zip(locus.1.checked_sub(marker.1))
            else {
                continue;
            };
            *translations.entry(translation).or_default() += 1;
        }
    }
    let maximum = translations
        .values()
        .copied()
        .max()
        .filter(|count| *count >= 2)?;
    let mut candidates = translations
        .into_iter()
        .filter_map(|(translation, count)| (count == maximum).then_some(translation));
    let translation = candidates.next()?;
    candidates.next().is_none().then_some(MarkerTransform {
        translation,
        ..transform
    })
}

fn quantize(point: Point2, quantum: f64) -> (i64, i64) {
    (
        (point.u / quantum).round() as i64,
        (point.v / quantum).round() as i64,
    )
}

fn sketch_entity_loci(entity: &SketchEntity) -> Vec<(Point2, SketchLocus)> {
    let locus = |point, locus| (point, locus);
    match &entity.geometry {
        SketchGeometry::Point { position } => {
            vec![locus(*position, SketchLocus::Entity(entity.id.clone()))]
        }
        SketchGeometry::Line { start, end } => vec![
            locus(*start, SketchLocus::Start(entity.id.clone())),
            locus(*end, SketchLocus::End(entity.id.clone())),
        ],
        SketchGeometry::Circle { center, .. } => {
            vec![locus(*center, SketchLocus::Center(entity.id.clone()))]
        }
        SketchGeometry::Ellipse {
            center,
            major_angle,
            major_radius,
            minor_radius,
            start_angle,
            end_angle,
        } => {
            let mut loci = vec![locus(*center, SketchLocus::Center(entity.id.clone()))];
            if let (Some(start), Some(end)) = (start_angle, end_angle) {
                let point = |parameter: f64| {
                    Point2::new(
                        center.u + major_angle.0.cos() * major_radius.0 * parameter.cos()
                            - major_angle.0.sin() * minor_radius.0 * parameter.sin(),
                        center.v
                            + major_angle.0.sin() * major_radius.0 * parameter.cos()
                            + major_angle.0.cos() * minor_radius.0 * parameter.sin(),
                    )
                };
                loci.push(locus(point(start.0), SketchLocus::Start(entity.id.clone())));
                loci.push(locus(point(end.0), SketchLocus::End(entity.id.clone())));
            }
            loci
        }
        SketchGeometry::Arc {
            center,
            radius,
            start_angle,
            end_angle,
        } => vec![
            locus(*center, SketchLocus::Center(entity.id.clone())),
            locus(
                Point2::new(
                    center.u + radius.0 * start_angle.0.cos(),
                    center.v + radius.0 * start_angle.0.sin(),
                ),
                SketchLocus::Start(entity.id.clone()),
            ),
            locus(
                Point2::new(
                    center.u + radius.0 * end_angle.0.cos(),
                    center.v + radius.0 * end_angle.0.sin(),
                ),
                SketchLocus::End(entity.id.clone()),
            ),
        ],
        SketchGeometry::Nurbs { control_points, .. } if !control_points.is_empty() => vec![
            locus(control_points[0], SketchLocus::Start(entity.id.clone())),
            locus(
                control_points[control_points.len() - 1],
                SketchLocus::End(entity.id.clone()),
            ),
        ],
        SketchGeometry::Nurbs { .. } | SketchGeometry::Native { .. } => Vec::new(),
    }
}

fn locus_key(locus: &SketchLocus) -> (&str, u8) {
    match locus {
        SketchLocus::Entity(entity) => (&entity.0, 0),
        SketchLocus::Start(entity) => (&entity.0, 1),
        SketchLocus::End(entity) => (&entity.0, 2),
        SketchLocus::Center(entity) => (&entity.0, 3),
    }
}

fn locus_entity(locus: &SketchLocus) -> SketchEntityId {
    match locus {
        SketchLocus::Entity(entity)
        | SketchLocus::Start(entity)
        | SketchLocus::End(entity)
        | SketchLocus::Center(entity) => entity.clone(),
    }
}

fn marker_entities(
    marker_id: &str,
    markers_by_id: &HashMap<&str, &SketchInputEntity>,
    loci_by_marker: &HashMap<String, Vec<SketchLocus>>,
) -> Vec<SketchEntityId> {
    marker_entities_inner(
        marker_id,
        markers_by_id,
        loci_by_marker,
        &mut HashSet::new(),
    )
}

fn marker_entities_inner(
    marker_id: &str,
    markers_by_id: &HashMap<&str, &SketchInputEntity>,
    loci_by_marker: &HashMap<String, Vec<SketchLocus>>,
    visited: &mut HashSet<String>,
) -> Vec<SketchEntityId> {
    let direct = loci_by_marker.get(marker_id).map(|loci| {
        loci.iter()
            .map(locus_entity)
            .collect::<HashSet<SketchEntityId>>()
    });
    if direct.as_ref().is_some_and(|entities| entities.len() == 1) {
        return direct.into_iter().flatten().collect();
    }
    if !visited.insert(marker_id.to_string()) {
        return Vec::new();
    }
    let Some(marker) = markers_by_id.get(marker_id) else {
        return direct.into_iter().flatten().collect();
    };
    let mut linked = marker
        .links
        .iter()
        .filter(|link| link.entity_ref != marker_id)
        .map(|link| {
            marker_entities_inner(
                &link.entity_ref,
                markers_by_id,
                loci_by_marker,
                &mut visited.clone(),
            )
            .into_iter()
            .collect::<HashSet<_>>()
        })
        .filter(|entities| !entities.is_empty());
    let mut entities = if let Some(direct) = direct {
        direct
    } else if let Some(linked) = linked.next() {
        linked
    } else {
        return Vec::new();
    };
    for candidates in linked {
        entities.retain(|entity| candidates.contains(entity));
    }
    let mut entities = entities.into_iter().collect::<Vec<_>>();
    entities.sort();
    entities
}

#[cfg(test)]
mod profile_join_tests {
    use super::{
        binary_relation_matches_evaluated_geometry, bind_circle_dimension_centers,
        bind_circular_profile_by_dimension, bind_detached_relation_drivers, bind_pattern_inputs,
        bind_sweep_adjacent_profiles, dimensioned_circle_surface_transforms,
        dimensioned_circle_transform, implicit_circle_marker, line_endpoint_markers,
        line_reference_direction, marker_entities, marker_point_locus, profile_loci_by_marker,
        project_dimensioned_sketch_geometry, project_relation_point_geometry,
        project_relation_solved_point_geometry, relation_operand_marker, relation_owner_markers,
        relation_parameter_by_display_name, resolved_marker_locus,
        select_marker_transforms_by_frame, single_marker_curve_entity, single_marker_line_entity,
        sketch_frame_marker_transform, type_display_relation_parameters,
        typed_marker_relation_definition, typed_marker_relation_definition_in_sketch,
        typed_relation_definition, unique_axis_aligned_linked_loci,
        unique_compatible_marker_transform, unique_linked_endpoint_locus, unique_marker_transform,
        unique_profile_axis_distance_locus, unique_profile_axis_distance_pair,
        unique_profile_distance_loci_pair, unique_profile_distance_locus,
        unique_profile_line_angle_entity, unique_profile_line_angle_pair,
        unique_profile_line_distance_entity, unique_profile_line_distance_pair,
        unique_profile_line_point_locus, unique_profile_point_line_entity,
        unique_profile_point_line_pair, unique_repaired_profile_line_angle_pair,
        unique_repaired_profile_line_distance_pair, unique_repaired_profile_point_line_pair,
        MarkerTransform,
    };
    use crate::records::{
        Feature as NativeFeature, FeatureHistory, FeatureInputClass, FeatureInputClassRole,
        FeatureInputLane, FeatureInputName, FeatureInputOperand, FeatureInputOperandKind,
        FeatureInputRelationFamily, FeatureInputRelationInstance, FeatureInputScalar,
        FeatureInputScalarRole, SketchInputEntity, SketchInputKind, SketchInputLink,
        SketchRelationKind,
    };
    use cadmpeg_ir::features::{
        Angle, DesignParameter, DimensionDisplay, Feature, FeatureDefinition, FeatureId, Length,
        ParameterId, ParameterValue, PathRef, PatternKind, SketchSpace, SweepMode,
    };
    use cadmpeg_ir::geometry::{Surface, SurfaceGeometry};
    use cadmpeg_ir::ids::SurfaceId;
    use cadmpeg_ir::math::{Point2, Point3, Vector3};
    use cadmpeg_ir::sketches::{
        Sketch, SketchConstraintDefinition, SketchEntity, SketchEntityId, SketchGeometry, SketchId,
        SketchLocus, SketchNativeOperand,
    };
    use std::collections::{BTreeMap, HashMap, HashSet};

    fn marker(id: &str, coordinates_m: Option<[f64; 2]>) -> SketchInputEntity {
        SketchInputEntity {
            id: id.into(),
            parent: "lane".into(),
            feature_ref: Some("feature-native".into()),
            ordinal: 0,
            offset: 0,
            object_index: None,
            local_id: None,
            kind: SketchInputKind::Point,
            state_value: None,
            coordinates_m,
            links: Vec::new(),
            link_selector: None,
        }
    }

    #[test]
    fn compact_d6_operand_indexes_coordinate_handles_in_byte_order() {
        let mut first = marker("first", Some([0.0, 0.0]));
        first.offset = 10;
        first.kind = SketchInputKind::Arc;
        let mut second = marker("second", Some([1.0, 0.0]));
        second.offset = 20;
        second.kind = SketchInputKind::LineOrCircle;
        let markers = HashMap::from([(first.id.as_str(), &first), (second.id.as_str(), &second)]);
        let relation = FeatureInputRelationInstance {
            id: "relation".into(),
            parent: "lane".into(),
            ordinal: 0,
            offset: 0,
            family: FeatureInputRelationFamily::PointPointDistance,
            class_ref: "class".into(),
            feature_ref: "feature-native".into(),
            scalar_refs: Vec::new(),
            parameter_scalar_ref: None,
            display_scalar_ref: None,
            operands: vec![FeatureInputOperand {
                offset: 0,
                reference_ref: "reference".into(),
                kind: FeatureInputOperandKind::D6,
                entity_index: 1,
                entity_ref: Some("stored-marker".into()),
            }],
        };

        assert_eq!(
            relation_operand_marker(
                &relation,
                0,
                &SketchId("sldprt:model:sketch#compact:lane:1".into()),
                &markers,
            ),
            Some("second")
        );
        assert_eq!(
            relation_operand_marker(&relation, 0, &SketchId("sketch".into()), &markers),
            Some("stored-marker")
        );
    }

    #[test]
    fn coordinate_curve_links_carry_reverse_constraint_incidence() {
        let mut relation = marker("relation", None);
        relation.kind = SketchInputKind::Relation(SketchRelationKind::Horizontal);
        let mut owner = marker("owner", Some([1.0, 2.0]));
        owner.kind = SketchInputKind::LineOrCircle;
        owner.object_index = Some(7);
        owner.offset = 1;
        owner.links = vec![SketchInputLink {
            local_id: 4,
            entity_ref: relation.id.clone(),
        }];
        let mut point = marker("point", Some([1.0, 2.0]));
        point.object_index = Some(8);
        point.offset = 2;
        point.links = owner.links.clone();
        let markers = HashMap::from([
            (relation.id.as_str(), &relation),
            (owner.id.as_str(), &owner),
            (point.id.as_str(), &point),
        ]);

        assert_eq!(
            relation_owner_markers(&relation, &markers),
            vec![&owner, &point]
        );
        let Some(SketchConstraintDefinition::Native { operands, .. }) =
            typed_marker_relation_definition(&relation, &markers, &HashMap::new())
        else {
            panic!("native relation");
        };
        assert_eq!(
            operands,
            vec![
                SketchNativeOperand {
                    native_kind: "sldprt:marker-constraint-owner".into(),
                    object_index: 7,
                    native_ref: Some(owner.id),
                },
                SketchNativeOperand {
                    native_kind: "sldprt:marker-constraint-owner".into(),
                    object_index: 8,
                    native_ref: Some(point.id),
                },
            ]
        );
    }

    #[test]
    fn unary_relation_uses_one_resolved_reverse_curve_owner() {
        let mut relation = marker("relation", None);
        relation.kind = SketchInputKind::Relation(SketchRelationKind::Horizontal);
        relation.links = vec![SketchInputLink {
            local_id: 1,
            entity_ref: "point".into(),
        }];
        let mut owner = marker("owner", Some([1.0, 2.0]));
        owner.kind = SketchInputKind::LineOrCircle;
        owner.links = vec![SketchInputLink {
            local_id: 4,
            entity_ref: relation.id.clone(),
        }];
        let point = marker("point", None);
        let markers = HashMap::from([
            (relation.id.as_str(), &relation),
            (owner.id.as_str(), &owner),
            (point.id.as_str(), &point),
        ]);
        let line = SketchEntityId("line".into());
        let loci = HashMap::from([
            (owner.id.clone(), vec![SketchLocus::Entity(line.clone())]),
            (
                point.id.clone(),
                vec![SketchLocus::Entity(SketchEntityId(
                    "sldprt:model:sketch-entity#relation-point:1".into(),
                ))],
            ),
        ]);

        assert_eq!(
            typed_marker_relation_definition(&relation, &markers, &loci),
            Some(SketchConstraintDefinition::Horizontal { entity: line })
        );
    }

    #[test]
    fn binary_relation_uses_two_resolved_reverse_curve_owners() {
        let mut relation = marker("relation", None);
        relation.kind = SketchInputKind::Relation(SketchRelationKind::Parallel);
        let mut first_owner = marker("first-owner", Some([1.0, 2.0]));
        first_owner.kind = SketchInputKind::LineOrCircle;
        first_owner.offset = 1;
        first_owner.links = vec![SketchInputLink {
            local_id: 4,
            entity_ref: relation.id.clone(),
        }];
        let mut second_owner = marker("second-owner", Some([3.0, 4.0]));
        second_owner.kind = SketchInputKind::LineOrCircle;
        second_owner.offset = 2;
        second_owner.links = first_owner.links.clone();
        let markers = HashMap::from([
            (relation.id.as_str(), &relation),
            (first_owner.id.as_str(), &first_owner),
            (second_owner.id.as_str(), &second_owner),
        ]);
        let first = SketchEntityId("first".into());
        let second = SketchEntityId("second".into());
        let loci = HashMap::from([
            (
                first_owner.id.clone(),
                vec![SketchLocus::Entity(first.clone())],
            ),
            (
                second_owner.id.clone(),
                vec![SketchLocus::Entity(second.clone())],
            ),
        ]);

        assert_eq!(
            typed_marker_relation_definition(&relation, &markers, &loci),
            Some(SketchConstraintDefinition::Parallel {
                first: first.clone(),
                second: second.clone(),
            })
        );
        let sketch = SketchId("sketch".into());
        let line = |id, start, end| SketchEntity {
            id,
            sketch: sketch.clone(),
            construction: false,
            native_ref: None,
            geometry_ref: None,
            endpoint_refs: Vec::new(),
            geometry: SketchGeometry::Line { start, end },
        };
        let first_line = line(first, Point2::new(0.0, 0.0), Point2::new(4.0, 0.0));
        let mut second_line = line(second, Point2::new(0.0, 2.0), Point2::new(4.0, 2.0));
        assert!(matches!(
            typed_marker_relation_definition_in_sketch(
                &relation,
                &sketch,
                &[first_line.clone(), second_line.clone()],
                &markers,
                &loci,
            ),
            Some(SketchConstraintDefinition::Parallel { .. })
        ));
        second_line.geometry = SketchGeometry::Line {
            start: Point2::new(0.0, 2.0),
            end: Point2::new(0.0, 6.0),
        };
        assert!(matches!(
            typed_marker_relation_definition_in_sketch(
                &relation,
                &sketch,
                &[first_line, second_line],
                &markers,
                &loci,
            ),
            Some(SketchConstraintDefinition::Native { .. })
        ));
    }

    #[test]
    fn construction_line_endpoints_accept_reverse_incidence() {
        let mut line = marker("line", Some([0.5, 0.0]));
        line.kind = SketchInputKind::LineOrCircle;
        let mut first = marker("first", Some([0.0, 0.0]));
        first.offset = 1;
        first.links = vec![SketchInputLink {
            local_id: 4,
            entity_ref: line.id.clone(),
        }];
        let mut second = marker("second", Some([1.0, 0.0]));
        second.offset = 2;
        second.links = first.links.clone();
        let markers = HashMap::from([
            (line.id.as_str(), &line),
            (first.id.as_str(), &first),
            (second.id.as_str(), &second),
        ]);

        assert_eq!(
            line_endpoint_markers(&line, &markers),
            vec![&first, &second]
        );
    }

    #[test]
    fn endpoint_incidence_binds_an_existing_profile_line() {
        let sketch_id = SketchId("sketch".into());
        let line_id = SketchEntityId("profile-line".into());
        let sketch = Sketch {
            id: sketch_id.clone(),
            name: None,
            configuration: None,
            origin: Point3::new(0.0, 0.0, 0.0),
            normal: Vector3::new(0.0, 0.0, 1.0),
            u_axis: Vector3::new(1.0, 0.0, 0.0),
            profiles: Vec::new(),
            native_ref: None,
        };
        let feature = Feature {
            id: FeatureId("feature".into()),
            ordinal: 0,
            name: None,
            suppressed: false,
            parent: None,
            dependencies: Vec::new(),
            source_properties: BTreeMap::new(),
            source_tag: None,
            source_text: None,
            source_content: Vec::new(),
            outputs: Vec::new(),
            definition: FeatureDefinition::Sketch {
                space: SketchSpace::Planar,
                sketch: Some(sketch_id.clone()),
            },
            native_ref: Some("feature-native".into()),
        };
        let entity = SketchEntity {
            id: line_id.clone(),
            sketch: sketch_id,
            construction: false,
            native_ref: None,
            geometry_ref: None,
            endpoint_refs: Vec::new(),
            geometry: SketchGeometry::Line {
                start: Point2::new(0.0, 0.0),
                end: Point2::new(1.0, 0.0),
            },
        };
        let mut line = marker("line", Some([0.0005, 0.0]));
        line.kind = SketchInputKind::LineOrCircle;
        let mut first = marker("first", Some([0.0, 0.0]));
        first.offset = 1;
        first.links = vec![SketchInputLink {
            local_id: 4,
            entity_ref: line.id.clone(),
        }];
        let mut second = marker("second", Some([0.001, 0.0]));
        second.offset = 2;
        second.links = first.links.clone();
        let lane = FeatureInputLane {
            id: "lane".into(),
            configuration: None,
            native_payload: Vec::new(),
            classes: Vec::new(),
            names: Vec::new(),
            scalars: Vec::new(),
            relation_bindings: Vec::new(),
            relation_instances: Vec::new(),
            body_selections: Vec::new(),
            edge_selections: Vec::new(),
            surface_selections: Vec::new(),
            references: Vec::new(),
            sketch_entities: vec![line, first, second],
        };

        assert_eq!(
            profile_loci_by_marker(&[feature], &[sketch], &[entity], &[lane])["line"],
            vec![SketchLocus::Entity(line_id)]
        );
    }

    #[test]
    fn point_marker_materializing_a_circle_binds_its_center() {
        let sketch_id = SketchId("sketch".into());
        let circle_id = SketchEntityId("circle".into());
        let sketch = Sketch {
            id: sketch_id.clone(),
            name: None,
            configuration: None,
            origin: Point3::new(0.0, 0.0, 0.0),
            normal: Vector3::new(0.0, 0.0, 1.0),
            u_axis: Vector3::new(1.0, 0.0, 0.0),
            profiles: Vec::new(),
            native_ref: None,
        };
        let feature = Feature {
            id: FeatureId("feature".into()),
            ordinal: 0,
            name: None,
            suppressed: false,
            parent: None,
            dependencies: Vec::new(),
            source_properties: BTreeMap::new(),
            source_tag: None,
            source_text: None,
            source_content: Vec::new(),
            outputs: Vec::new(),
            definition: FeatureDefinition::Sketch {
                space: SketchSpace::Planar,
                sketch: Some(sketch_id.clone()),
            },
            native_ref: Some("feature-native".into()),
        };
        let entity = SketchEntity {
            id: circle_id.clone(),
            sketch: sketch_id,
            construction: false,
            native_ref: Some("circle-marker".into()),
            geometry_ref: None,
            endpoint_refs: Vec::new(),
            geometry: SketchGeometry::Circle {
                center: Point2::new(1.0, 2.0),
                radius: Length(3.0),
            },
        };
        let mut circle_marker = marker("circle-marker", Some([1.0, 2.0]));
        circle_marker.kind = SketchInputKind::Point;
        circle_marker.feature_ref = Some("feature-native".into());
        let lane = FeatureInputLane {
            id: "lane".into(),
            configuration: None,
            native_payload: Vec::new(),
            classes: Vec::new(),
            names: Vec::new(),
            scalars: Vec::new(),
            relation_bindings: Vec::new(),
            relation_instances: Vec::new(),
            body_selections: Vec::new(),
            edge_selections: Vec::new(),
            surface_selections: Vec::new(),
            references: Vec::new(),
            sketch_entities: vec![circle_marker],
        };

        assert_eq!(
            profile_loci_by_marker(&[feature], &[sketch], &[entity], &[lane])["circle-marker"],
            vec![SketchLocus::Center(circle_id)]
        );
    }

    #[test]
    fn distance_fallback_requires_one_locus_in_the_complete_sketch() {
        let sketch = SketchId("sketch".into());
        let point = |id: &str, u: f64, v: f64| SketchEntity {
            id: SketchEntityId(id.into()),
            sketch: sketch.clone(),
            construction: false,
            native_ref: None,
            geometry_ref: None,
            endpoint_refs: Vec::new(),
            geometry: SketchGeometry::Point {
                position: Point2::new(u, v),
            },
        };
        let known = point("known", 0.0, 0.0);
        let candidate = point("candidate", 3.0, 4.0);
        let parameter = DesignParameter {
            id: ParameterId("distance".into()),
            owner: FeatureId("feature".into()),
            ordinal: 0,
            name: "D1".into(),
            expression: "5mm".into(),
            display: None,
            value: Some(ParameterValue::Length(Length(5.0))),
            dependencies: Vec::new(),
            properties: BTreeMap::new(),
            pmi: None,
            native_ref: None,
        };
        let known_locus = SketchLocus::Entity(known.id.clone());
        assert_eq!(
            unique_profile_distance_locus(
                &sketch,
                &known_locus,
                &parameter,
                &[known.clone(), candidate.clone()],
            ),
            Some(SketchLocus::Entity(candidate.id.clone()))
        );

        let ambiguous = point("ambiguous", -3.0, -4.0);
        assert_eq!(
            unique_profile_distance_locus(
                &sketch,
                &known_locus,
                &parameter,
                &[known, candidate, ambiguous],
            ),
            None
        );
    }

    #[test]
    fn curve_operand_rejects_a_point_qualified_geometry_alias() {
        let sketch = SketchId("sketch".into());
        let point_id = SketchEntityId("point".into());
        let line_id = SketchEntityId("line".into());
        let circle_id = SketchEntityId("circle".into());
        let entities = vec![
            SketchEntity {
                id: point_id.clone(),
                sketch: sketch.clone(),
                construction: true,
                native_ref: None,
                geometry_ref: Some("curve-marker".into()),
                endpoint_refs: Vec::new(),
                geometry: SketchGeometry::Point {
                    position: Point2::new(0.5, 0.0),
                },
            },
            SketchEntity {
                id: line_id.clone(),
                sketch,
                construction: false,
                native_ref: Some("line-marker".into()),
                geometry_ref: None,
                endpoint_refs: Vec::new(),
                geometry: SketchGeometry::Line {
                    start: Point2::new(0.0, 0.0),
                    end: Point2::new(1.0, 0.0),
                },
            },
            SketchEntity {
                id: circle_id.clone(),
                sketch: SketchId("sketch".into()),
                construction: false,
                native_ref: Some("circle-marker".into()),
                geometry_ref: None,
                endpoint_refs: Vec::new(),
                geometry: SketchGeometry::Circle {
                    center: Point2::new(0.0, 0.0),
                    radius: Length(1.0),
                },
            },
        ];
        let loci = HashMap::from([
            ("curve-marker".into(), vec![SketchLocus::Entity(point_id)]),
            (
                "line-marker".into(),
                vec![SketchLocus::Entity(line_id.clone())],
            ),
            (
                "circle-marker".into(),
                vec![SketchLocus::Entity(circle_id.clone())],
            ),
        ]);

        assert_eq!(
            single_marker_curve_entity("curve-marker", &HashMap::new(), &loci, &entities),
            None
        );
        assert_eq!(
            single_marker_curve_entity("line-marker", &HashMap::new(), &loci, &entities),
            Some(line_id)
        );
        assert_eq!(
            single_marker_line_entity("circle-marker", &HashMap::new(), &loci, &entities),
            None
        );
        assert_eq!(
            single_marker_curve_entity("circle-marker", &HashMap::new(), &loci, &entities),
            Some(circle_id)
        );
    }

    #[test]
    fn axis_relation_requires_aligned_evaluated_geometry() {
        let sketch = SketchId("sketch".into());
        let first_id = SketchEntityId("first".into());
        let second_id = SketchEntityId("second".into());
        let line = |id: SketchEntityId, start: Point2, end: Point2| SketchEntity {
            id,
            sketch: sketch.clone(),
            construction: false,
            native_ref: None,
            geometry_ref: None,
            endpoint_refs: Vec::new(),
            geometry: SketchGeometry::Line { start, end },
        };
        let entities = vec![
            line(
                first_id.clone(),
                Point2::new(0.0, 0.0),
                Point2::new(1.0, 0.0),
            ),
            line(
                second_id.clone(),
                Point2::new(1.0, 0.0),
                Point2::new(1.0, 1.0),
            ),
        ];
        let first = marker("first-marker", None);
        let second = marker("second-marker", None);
        let mut relation = marker("relation", None);
        relation.kind = SketchInputKind::Relation(SketchRelationKind::HorizontalPoints);
        relation.links = vec![
            SketchInputLink {
                local_id: 1,
                entity_ref: first.id.clone(),
            },
            SketchInputLink {
                local_id: 2,
                entity_ref: second.id.clone(),
            },
        ];
        let markers = HashMap::from([
            (first.id.as_str(), &first),
            (second.id.as_str(), &second),
            (relation.id.as_str(), &relation),
        ]);
        let loci = HashMap::from([
            (first.id.clone(), vec![SketchLocus::Start(first_id)]),
            (second.id.clone(), vec![SketchLocus::End(second_id)]),
        ]);

        assert!(matches!(
            typed_marker_relation_definition_in_sketch(
                &relation, &sketch, &entities, &markers, &loci,
            ),
            Some(SketchConstraintDefinition::Native { .. })
        ));
    }

    #[test]
    fn dimension_requires_matching_evaluated_geometry() {
        let sketch = SketchId("sketch".into());
        let entities = [
            SketchEntity {
                id: SketchEntityId("first".into()),
                sketch: sketch.clone(),
                construction: true,
                native_ref: None,
                geometry_ref: None,
                endpoint_refs: Vec::new(),
                geometry: SketchGeometry::Point {
                    position: Point2::new(0.0, 0.0),
                },
            },
            SketchEntity {
                id: SketchEntityId("second".into()),
                sketch: sketch.clone(),
                construction: true,
                native_ref: None,
                geometry_ref: None,
                endpoint_refs: Vec::new(),
                geometry: SketchGeometry::Point {
                    position: Point2::new(3.0, 4.0),
                },
            },
        ];
        let first = marker("first-marker", None);
        let second = marker("second-marker", None);
        let markers = HashMap::from([(first.id.as_str(), &first), (second.id.as_str(), &second)]);
        let loci = HashMap::from([
            (
                first.id.clone(),
                vec![SketchLocus::Entity(entities[0].id.clone())],
            ),
            (
                second.id.clone(),
                vec![SketchLocus::Entity(entities[1].id.clone())],
            ),
        ]);
        let relation = FeatureInputRelationInstance {
            id: "relation".into(),
            parent: "lane".into(),
            ordinal: 0,
            offset: 0,
            family: FeatureInputRelationFamily::PointPointDistance,
            class_ref: "class".into(),
            feature_ref: "feature".into(),
            scalar_refs: Vec::new(),
            parameter_scalar_ref: Some("scalar".into()),
            display_scalar_ref: None,
            operands: [&first, &second]
                .into_iter()
                .enumerate()
                .map(|(index, marker)| FeatureInputOperand {
                    offset: index as u64,
                    reference_ref: format!("reference-{index}"),
                    kind: FeatureInputOperandKind::D6,
                    entity_index: index as u16,
                    entity_ref: Some(marker.id.clone()),
                })
                .collect(),
        };
        let parameter = DesignParameter {
            id: ParameterId("distance".into()),
            owner: FeatureId("feature".into()),
            ordinal: 0,
            name: "D1".into(),
            expression: "4mm".into(),
            display: None,
            value: Some(ParameterValue::Length(Length(4.0))),
            dependencies: Vec::new(),
            properties: BTreeMap::new(),
            pmi: None,
            native_ref: Some("scalar".into()),
        };

        assert_eq!(
            typed_relation_definition(
                &relation,
                Some(&parameter),
                &sketch,
                &entities,
                &markers,
                &loci,
            ),
            None
        );
    }

    #[test]
    fn binary_relations_require_matching_evaluated_geometry() {
        use SketchRelationKind::{
            Collinear, Concentric, Coradial, Equal, Parallel, Perpendicular, Tangent,
        };
        let sketch = SketchId("sketch".into());
        let entity = |id: &str, geometry| SketchEntity {
            id: SketchEntityId(id.into()),
            sketch: sketch.clone(),
            construction: false,
            native_ref: None,
            geometry_ref: None,
            endpoint_refs: Vec::new(),
            geometry,
        };
        let horizontal = entity(
            "horizontal",
            SketchGeometry::Line {
                start: Point2::new(0.0, 0.0),
                end: Point2::new(4.0, 0.0),
            },
        );
        let parallel = entity(
            "parallel",
            SketchGeometry::Line {
                start: Point2::new(0.0, 2.0),
                end: Point2::new(4.0, 2.0),
            },
        );
        let perpendicular = entity(
            "perpendicular",
            SketchGeometry::Line {
                start: Point2::new(0.0, 0.0),
                end: Point2::new(0.0, 4.0),
            },
        );
        let collinear = entity(
            "collinear",
            SketchGeometry::Line {
                start: Point2::new(6.0, 0.0),
                end: Point2::new(10.0, 0.0),
            },
        );
        let circle = |id: &str, u, v, radius| {
            entity(
                id,
                SketchGeometry::Circle {
                    center: Point2::new(u, v),
                    radius: Length(radius),
                },
            )
        };
        let first_circle = circle("first-circle", 0.0, 2.0, 2.0);
        let equal_circle = circle("equal-circle", 4.0, 2.0, 2.0);
        let concentric_circle = circle("concentric-circle", 0.0, 2.0, 1.0);
        let coradial_circle = circle("coradial-circle", 0.0, 2.0, 2.0);
        let unrelated_circle = circle("unrelated-circle", 8.0, 8.0, 3.0);

        for (kind, first, second) in [
            (Parallel, &horizontal, &parallel),
            (Perpendicular, &horizontal, &perpendicular),
            (Collinear, &horizontal, &collinear),
            (Equal, &first_circle, &equal_circle),
            (Concentric, &first_circle, &concentric_circle),
            (Coradial, &first_circle, &coradial_circle),
            (Tangent, &horizontal, &first_circle),
            (Tangent, &first_circle, &equal_circle),
        ] {
            assert!(binary_relation_matches_evaluated_geometry(
                kind, first, second
            ));
        }
        for kind in [
            Parallel,
            Perpendicular,
            Collinear,
            Equal,
            Concentric,
            Tangent,
            Coradial,
        ] {
            assert!(!binary_relation_matches_evaluated_geometry(
                kind,
                &horizontal,
                &unrelated_circle,
            ));
        }
    }

    #[test]
    fn locus_relations_require_matching_evaluated_geometry() {
        let sketch = SketchId("sketch".into());
        let entity = |id: &str, geometry| SketchEntity {
            id: SketchEntityId(id.into()),
            sketch: sketch.clone(),
            construction: true,
            native_ref: None,
            geometry_ref: None,
            endpoint_refs: Vec::new(),
            geometry,
        };
        let mut first = entity(
            "first",
            SketchGeometry::Point {
                position: Point2::new(0.0, 0.0),
            },
        );
        let mut second = entity(
            "second",
            SketchGeometry::Point {
                position: Point2::new(0.0, 0.0),
            },
        );
        let line = entity(
            "line",
            SketchGeometry::Line {
                start: Point2::new(-2.0, 0.0),
                end: Point2::new(2.0, 0.0),
            },
        );
        let mut arc = entity(
            "arc",
            SketchGeometry::Arc {
                center: Point2::new(0.0, 0.0),
                radius: Length(1.0),
                start_angle: cadmpeg_ir::features::Angle(0.0),
                end_angle: cadmpeg_ir::features::Angle(std::f64::consts::FRAC_PI_2),
            },
        );
        let symmetric_first = entity(
            "symmetric-first",
            SketchGeometry::Point {
                position: Point2::new(-1.0, 2.0),
            },
        );
        let mut symmetric_second = entity(
            "symmetric-second",
            SketchGeometry::Point {
                position: Point2::new(1.0, 2.0),
            },
        );
        let symmetry_axis = entity(
            "symmetry-axis",
            SketchGeometry::Line {
                start: Point2::new(0.0, -3.0),
                end: Point2::new(0.0, 3.0),
            },
        );
        let mut first_marker = marker("first-marker", None);
        let second_marker = marker("second-marker", None);
        let mut line_marker = marker("line-marker", None);
        line_marker.kind = SketchInputKind::LineOrCircle;
        let mut arc_marker = marker("arc-marker", None);
        arc_marker.kind = SketchInputKind::Arc;
        let symmetric_first_marker = marker("symmetric-first-marker", None);
        let symmetric_second_marker = marker("symmetric-second-marker", None);
        let mut symmetry_axis_marker = marker("symmetry-axis-marker", None);
        symmetry_axis_marker.kind = SketchInputKind::LineOrCircle;
        let mut coincident = marker("coincident", None);
        coincident.kind = SketchInputKind::Relation(SketchRelationKind::Coincident);
        coincident.links = [(&first_marker, 1), (&second_marker, 2)]
            .map(|(marker, local_id)| SketchInputLink {
                local_id,
                entity_ref: marker.id.clone(),
            })
            .to_vec();
        let mut merge_points = coincident.clone();
        merge_points.id = "merge-points".into();
        merge_points.kind = SketchInputKind::Relation(SketchRelationKind::MergePoints);
        let mut midpoint = marker("midpoint", None);
        midpoint.kind = SketchInputKind::Relation(SketchRelationKind::Midpoint);
        midpoint.links = [(&first_marker, 1), (&line_marker, 3)]
            .map(|(marker, local_id)| SketchInputLink {
                local_id,
                entity_ref: marker.id.clone(),
            })
            .to_vec();
        let mut arc_angle = marker("arc-angle", None);
        arc_angle.kind = SketchInputKind::Relation(SketchRelationKind::ArcAngle90);
        arc_angle.links = vec![SketchInputLink {
            local_id: 4,
            entity_ref: arc_marker.id.clone(),
        }];
        let mut symmetric = marker("symmetric", None);
        symmetric.kind = SketchInputKind::Relation(SketchRelationKind::Symmetric);
        symmetric.links = [(&symmetric_first_marker, 5), (&symmetric_second_marker, 6)]
            .map(|(marker, local_id)| SketchInputLink {
                local_id,
                entity_ref: marker.id.clone(),
            })
            .to_vec();
        symmetry_axis_marker.links.push(SketchInputLink {
            local_id: 7,
            entity_ref: symmetric.id.clone(),
        });
        let mut at_intersection = marker("at-intersection", None);
        at_intersection.kind = SketchInputKind::Relation(SketchRelationKind::AtIntersection);
        at_intersection.links = [(&line_marker, 9), (&symmetry_axis_marker, 10)]
            .map(|(marker, local_id)| SketchInputLink {
                local_id,
                entity_ref: marker.id.clone(),
            })
            .to_vec();
        first_marker.links.push(SketchInputLink {
            local_id: 8,
            entity_ref: at_intersection.id.clone(),
        });
        let markers = HashMap::from([
            (first_marker.id.as_str(), &first_marker),
            (second_marker.id.as_str(), &second_marker),
            (line_marker.id.as_str(), &line_marker),
            (arc_marker.id.as_str(), &arc_marker),
            (symmetric_first_marker.id.as_str(), &symmetric_first_marker),
            (
                symmetric_second_marker.id.as_str(),
                &symmetric_second_marker,
            ),
            (symmetry_axis_marker.id.as_str(), &symmetry_axis_marker),
            (coincident.id.as_str(), &coincident),
            (merge_points.id.as_str(), &merge_points),
            (midpoint.id.as_str(), &midpoint),
            (arc_angle.id.as_str(), &arc_angle),
            (symmetric.id.as_str(), &symmetric),
            (at_intersection.id.as_str(), &at_intersection),
        ]);
        let loci = HashMap::from([
            (
                first_marker.id.clone(),
                vec![SketchLocus::Entity(first.id.clone())],
            ),
            (
                second_marker.id.clone(),
                vec![SketchLocus::Entity(second.id.clone())],
            ),
            (
                line_marker.id.clone(),
                vec![SketchLocus::Entity(line.id.clone())],
            ),
            (
                arc_marker.id.clone(),
                vec![SketchLocus::Entity(arc.id.clone())],
            ),
            (
                symmetric_first_marker.id.clone(),
                vec![SketchLocus::Entity(symmetric_first.id.clone())],
            ),
            (
                symmetric_second_marker.id.clone(),
                vec![SketchLocus::Entity(symmetric_second.id.clone())],
            ),
            (
                symmetry_axis_marker.id.clone(),
                vec![SketchLocus::Entity(symmetry_axis.id.clone())],
            ),
        ]);
        assert!(matches!(
            typed_marker_relation_definition_in_sketch(
                &coincident,
                &sketch,
                &[first.clone(), second.clone(), line.clone(), arc.clone()],
                &markers,
                &loci,
            ),
            Some(SketchConstraintDefinition::CoincidentLoci { .. })
        ));
        assert!(matches!(
            typed_marker_relation_definition_in_sketch(
                &merge_points,
                &sketch,
                &[first.clone(), second.clone(), line.clone(), arc.clone()],
                &markers,
                &loci,
            ),
            Some(SketchConstraintDefinition::CoincidentLoci { .. })
        ));
        assert!(matches!(
            typed_marker_relation_definition_in_sketch(
                &midpoint,
                &sketch,
                &[first.clone(), second.clone(), line.clone(), arc.clone()],
                &markers,
                &loci,
            ),
            Some(SketchConstraintDefinition::Midpoint { .. })
        ));
        assert!(matches!(
            typed_marker_relation_definition_in_sketch(
                &arc_angle,
                &sketch,
                &[first.clone(), second.clone(), line.clone(), arc.clone()],
                &markers,
                &loci,
            ),
            Some(SketchConstraintDefinition::ArcAngle { .. })
        ));
        assert_eq!(
            typed_marker_relation_definition_in_sketch(
                &symmetric,
                &sketch,
                &[
                    symmetric_first.clone(),
                    symmetric_second.clone(),
                    symmetry_axis.clone(),
                ],
                &markers,
                &loci,
            ),
            Some(SketchConstraintDefinition::Symmetric {
                first: SketchLocus::Entity(symmetric_first.id.clone()),
                second: SketchLocus::Entity(symmetric_second.id.clone()),
                axis: symmetry_axis.id.clone(),
            })
        );
        assert_eq!(
            typed_marker_relation_definition_in_sketch(
                &at_intersection,
                &sketch,
                &[first.clone(), line.clone(), symmetry_axis.clone()],
                &markers,
                &loci,
            ),
            Some(SketchConstraintDefinition::AtIntersection {
                point: SketchLocus::Entity(first.id.clone()),
                first: line.id.clone(),
                second: symmetry_axis.id.clone(),
            })
        );

        second.geometry = SketchGeometry::Point {
            position: Point2::new(1.0, 0.0),
        };
        assert!(matches!(
            typed_marker_relation_definition_in_sketch(
                &coincident,
                &sketch,
                &[first.clone(), second.clone(), line.clone(), arc.clone()],
                &markers,
                &loci,
            ),
            Some(SketchConstraintDefinition::Native { .. })
        ));
        first.clone_from(&entity(
            "first",
            SketchGeometry::Point {
                position: Point2::new(1.0, 0.0),
            },
        ));
        assert!(matches!(
            typed_marker_relation_definition_in_sketch(
                &at_intersection,
                &sketch,
                &[first.clone(), line.clone(), symmetry_axis.clone()],
                &markers,
                &loci,
            ),
            Some(SketchConstraintDefinition::Native { .. })
        ));
        assert!(matches!(
            typed_marker_relation_definition_in_sketch(
                &midpoint,
                &sketch,
                &[first.clone(), second.clone(), line.clone(), arc.clone()],
                &markers,
                &loci,
            ),
            Some(SketchConstraintDefinition::Native { .. })
        ));
        arc.geometry = SketchGeometry::Arc {
            center: Point2::new(0.0, 0.0),
            radius: Length(1.0),
            start_angle: cadmpeg_ir::features::Angle(0.0),
            end_angle: cadmpeg_ir::features::Angle(std::f64::consts::PI),
        };
        assert!(matches!(
            typed_marker_relation_definition_in_sketch(
                &arc_angle,
                &sketch,
                &[first.clone(), second.clone(), line.clone(), arc.clone()],
                &markers,
                &loci,
            ),
            Some(SketchConstraintDefinition::Native { .. })
        ));
        symmetric_second.geometry = SketchGeometry::Point {
            position: Point2::new(2.0, 2.0),
        };
        assert!(matches!(
            typed_marker_relation_definition_in_sketch(
                &symmetric,
                &sketch,
                &[symmetric_first, symmetric_second, symmetry_axis],
                &markers,
                &loci,
            ),
            Some(SketchConstraintDefinition::Native { .. })
        ));
    }

    #[test]
    fn distance_pair_fallback_requires_one_pair_in_the_complete_sketch() {
        let sketch = SketchId("sketch".into());
        let point = |id: &str, u: f64, v: f64| SketchEntity {
            id: SketchEntityId(id.into()),
            sketch: sketch.clone(),
            construction: false,
            native_ref: None,
            geometry_ref: None,
            endpoint_refs: Vec::new(),
            geometry: SketchGeometry::Point {
                position: Point2::new(u, v),
            },
        };
        let parameter = DesignParameter {
            id: ParameterId("distance".into()),
            owner: FeatureId("feature".into()),
            ordinal: 0,
            name: "D1".into(),
            expression: "5mm".into(),
            display: None,
            value: Some(ParameterValue::Length(Length(5.0))),
            dependencies: Vec::new(),
            properties: BTreeMap::new(),
            pmi: None,
            native_ref: None,
        };
        let first = point("first", 0.0, 0.0);
        let coincident_first = point("z-coincident-first", 0.0, 0.0);
        let second = point("second", 3.0, 4.0);
        let unrelated = point("unrelated", 20.0, 20.0);
        assert_eq!(
            unique_profile_distance_loci_pair(
                &sketch,
                &parameter,
                &[
                    first.clone(),
                    coincident_first,
                    second.clone(),
                    unrelated.clone(),
                ],
            ),
            Some((
                SketchLocus::Entity(first.id.clone()),
                SketchLocus::Entity(second.id.clone()),
            ))
        );

        let ambiguous = point("ambiguous", 23.0, 24.0);
        assert_eq!(
            unique_profile_distance_loci_pair(
                &sketch,
                &parameter,
                &[first, second, unrelated, ambiguous],
            ),
            None
        );
    }

    #[test]
    fn axis_distance_fallback_requires_one_pair_in_the_complete_sketch() {
        let sketch = SketchId("sketch".into());
        let point = |id: &str, u: f64, v: f64| SketchEntity {
            id: SketchEntityId(id.into()),
            sketch: sketch.clone(),
            construction: false,
            native_ref: None,
            geometry_ref: None,
            endpoint_refs: Vec::new(),
            geometry: SketchGeometry::Point {
                position: Point2::new(u, v),
            },
        };
        let first = point("first", 0.0, 0.0);
        let second = point("second", 5.0, 20.0);
        let unrelated = point("unrelated", 100.0, 100.0);
        let parameter = DesignParameter {
            id: ParameterId("distance".into()),
            owner: FeatureId("feature".into()),
            ordinal: 0,
            name: "D1".into(),
            expression: "5mm".into(),
            display: None,
            value: Some(ParameterValue::Length(Length(5.0))),
            dependencies: Vec::new(),
            properties: BTreeMap::new(),
            pmi: None,
            native_ref: None,
        };
        let first_locus = SketchLocus::Entity(first.id.clone());
        let second_locus = SketchLocus::Entity(second.id.clone());
        let entities = [first.clone(), second.clone(), unrelated.clone()];
        assert_eq!(
            unique_profile_axis_distance_locus(&sketch, &first_locus, &parameter, &entities, true,),
            Some(second_locus.clone())
        );
        assert_eq!(
            unique_profile_axis_distance_pair(&sketch, &parameter, &entities, true),
            Some((first_locus, second_locus))
        );

        let ambiguous = point("ambiguous", 10.0, 30.0);
        assert_eq!(
            unique_profile_axis_distance_pair(
                &sketch,
                &parameter,
                &[first, second, unrelated, ambiguous],
                true,
            ),
            None
        );
    }

    #[test]
    fn line_distance_fallback_requires_one_parallel_pair_in_the_complete_sketch() {
        let sketch = SketchId("sketch".into());
        let line = |id: &str, start: Point2, end: Point2| SketchEntity {
            id: SketchEntityId(id.into()),
            sketch: sketch.clone(),
            construction: false,
            native_ref: None,
            geometry_ref: None,
            endpoint_refs: Vec::new(),
            geometry: SketchGeometry::Line { start, end },
        };
        let first = line("first", Point2::new(0.0, 0.0), Point2::new(10.0, 0.0));
        let second = line("second", Point2::new(0.0, 5.0), Point2::new(10.0, 5.0));
        let unrelated = line(
            "unrelated",
            Point2::new(20.0, 20.0),
            Point2::new(21.0, 21.0),
        );
        let parameter = DesignParameter {
            id: ParameterId("distance".into()),
            owner: FeatureId("feature".into()),
            ordinal: 0,
            name: "D1".into(),
            expression: "5mm".into(),
            display: None,
            value: Some(ParameterValue::Length(Length(5.0))),
            dependencies: Vec::new(),
            properties: BTreeMap::new(),
            pmi: None,
            native_ref: None,
        };
        let entities = [first.clone(), second.clone(), unrelated.clone()];
        assert_eq!(
            unique_profile_line_distance_entity(&sketch, &first.id, &parameter, &entities),
            Some(second.id.clone())
        );
        assert_eq!(
            unique_profile_line_distance_pair(&sketch, &parameter, &entities),
            Some((first.id.clone(), second.id.clone()))
        );

        let wrong = line("wrong", Point2::new(0.0, 2.0), Point2::new(10.0, 2.0));
        assert_eq!(
            unique_repaired_profile_line_distance_pair(
                &sketch,
                &first.id,
                &wrong.id,
                &parameter,
                &[
                    first.clone(),
                    wrong.clone(),
                    second.clone(),
                    unrelated.clone(),
                ],
            ),
            Some((first.id.clone(), second.id.clone()))
        );

        let other_solved = line(
            "other-solved",
            Point2::new(0.0, -5.0),
            Point2::new(10.0, -5.0),
        );
        assert_eq!(
            unique_repaired_profile_line_distance_pair(
                &sketch,
                &first.id,
                &wrong.id,
                &parameter,
                &[first.clone(), wrong.clone(), second.clone(), other_solved,],
            ),
            None
        );

        let unrelated_first = line(
            "unrelated-first",
            Point2::new(20.0, 20.0),
            Point2::new(30.0, 20.0),
        );
        let unrelated_second = line(
            "unrelated-second",
            Point2::new(20.0, 25.0),
            Point2::new(30.0, 25.0),
        );
        assert_eq!(
            unique_repaired_profile_line_distance_pair(
                &sketch,
                &first.id,
                &wrong.id,
                &parameter,
                &[
                    first.clone(),
                    wrong.clone(),
                    unrelated_first,
                    unrelated_second,
                ],
            ),
            None
        );

        let ambiguous = line("ambiguous", Point2::new(0.0, 10.0), Point2::new(10.0, 10.0));
        assert_eq!(
            unique_profile_line_distance_pair(
                &sketch,
                &parameter,
                &[first, second, unrelated, ambiguous],
            ),
            None
        );
    }

    #[test]
    fn line_angle_fallback_requires_one_pair_in_the_complete_sketch() {
        let sketch = SketchId("sketch".into());
        let line = |id: &str, start: Point2, end: Point2| SketchEntity {
            id: SketchEntityId(id.into()),
            sketch: sketch.clone(),
            construction: false,
            native_ref: None,
            geometry_ref: None,
            endpoint_refs: Vec::new(),
            geometry: SketchGeometry::Line { start, end },
        };
        let horizontal = line("horizontal", Point2::new(0.0, 0.0), Point2::new(10.0, 0.0));
        let vertical = line("vertical", Point2::new(0.0, 0.0), Point2::new(0.0, 10.0));
        let diagonal = line("diagonal", Point2::new(20.0, 20.0), Point2::new(21.0, 21.0));
        let parameter = DesignParameter {
            id: ParameterId("angle".into()),
            owner: FeatureId("feature".into()),
            ordinal: 0,
            name: "D1".into(),
            expression: "90deg".into(),
            display: None,
            value: Some(ParameterValue::Angle(Angle(std::f64::consts::FRAC_PI_2))),
            dependencies: Vec::new(),
            properties: BTreeMap::new(),
            pmi: None,
            native_ref: None,
        };
        let entities = [horizontal.clone(), vertical.clone(), diagonal.clone()];
        assert_eq!(
            unique_profile_line_angle_entity(&sketch, &horizontal.id, &parameter, &entities),
            Some(vertical.id.clone())
        );
        assert_eq!(
            unique_profile_line_angle_pair(&sketch, &parameter, &entities),
            Some((horizontal.id.clone(), vertical.id.clone()))
        );

        let wrong = line(
            "wrong",
            Point2::new(0.0, 0.0),
            Point2::new(3.0_f64.sqrt(), 1.0),
        );
        assert_eq!(
            unique_repaired_profile_line_angle_pair(
                &sketch,
                &horizontal.id,
                &wrong.id,
                &parameter,
                &[
                    horizontal.clone(),
                    wrong.clone(),
                    vertical.clone(),
                    diagonal.clone(),
                ],
            ),
            Some((horizontal.id.clone(), vertical.id.clone()))
        );

        let ambiguous = line("ambiguous", Point2::new(5.0, 0.0), Point2::new(5.0, 10.0));
        assert_eq!(
            unique_repaired_profile_line_angle_pair(
                &sketch,
                &horizontal.id,
                &wrong.id,
                &parameter,
                &[
                    horizontal.clone(),
                    wrong.clone(),
                    vertical.clone(),
                    ambiguous.clone(),
                ],
            ),
            None
        );

        let unrelated_first = line(
            "unrelated-first",
            Point2::new(0.0, 0.0),
            Point2::new(0.5, 3.0_f64.sqrt() * 0.5),
        );
        let unrelated_second = line(
            "unrelated-second",
            Point2::new(0.0, 0.0),
            Point2::new(-3.0_f64.sqrt() * 0.5, 0.5),
        );
        assert_eq!(
            unique_repaired_profile_line_angle_pair(
                &sketch,
                &horizontal.id,
                &wrong.id,
                &parameter,
                &[
                    horizontal.clone(),
                    wrong.clone(),
                    unrelated_first,
                    unrelated_second,
                ],
            ),
            None
        );
        assert_eq!(
            unique_profile_line_angle_pair(
                &sketch,
                &parameter,
                &[horizontal, vertical, diagonal, ambiguous],
            ),
            None
        );
    }

    #[test]
    fn point_line_fallback_requires_one_pair_in_the_complete_sketch() {
        let sketch = SketchId("sketch".into());
        let point = SketchEntity {
            id: SketchEntityId("point".into()),
            sketch: sketch.clone(),
            construction: false,
            native_ref: None,
            geometry_ref: None,
            endpoint_refs: Vec::new(),
            geometry: SketchGeometry::Point {
                position: Point2::new(0.0, 5.0),
            },
        };
        let line = |id: &str, start: Point2, end: Point2| SketchEntity {
            id: SketchEntityId(id.into()),
            sketch: sketch.clone(),
            construction: false,
            native_ref: None,
            geometry_ref: None,
            endpoint_refs: Vec::new(),
            geometry: SketchGeometry::Line { start, end },
        };
        let horizontal = line("horizontal", Point2::new(0.0, 0.0), Point2::new(10.0, 0.0));
        let unrelated = line(
            "unrelated",
            Point2::new(100.0, 20.0),
            Point2::new(100.0, 30.0),
        );
        let parameter = DesignParameter {
            id: ParameterId("distance".into()),
            owner: FeatureId("feature".into()),
            ordinal: 0,
            name: "D1".into(),
            expression: "5mm".into(),
            display: None,
            value: Some(ParameterValue::Length(Length(5.0))),
            dependencies: Vec::new(),
            properties: BTreeMap::new(),
            pmi: None,
            native_ref: None,
        };
        let point_locus = SketchLocus::Entity(point.id.clone());
        let entities = [point.clone(), horizontal.clone(), unrelated.clone()];
        assert_eq!(
            unique_profile_point_line_entity(&sketch, &point_locus, &parameter, &entities),
            Some(horizontal.id.clone())
        );
        assert_eq!(
            unique_profile_line_point_locus(&sketch, &horizontal.id, &parameter, &entities),
            Some(point_locus.clone())
        );
        assert_eq!(
            unique_profile_point_line_pair(&sketch, &parameter, &entities),
            Some((point_locus, horizontal.id.clone()))
        );

        let wrong = line("wrong", Point2::new(0.0, 2.0), Point2::new(10.0, 2.0));
        assert_eq!(
            unique_repaired_profile_point_line_pair(
                &sketch,
                &SketchLocus::Entity(point.id.clone()),
                &wrong.id,
                &parameter,
                &[
                    point.clone(),
                    wrong.clone(),
                    horizontal.clone(),
                    unrelated.clone(),
                ],
            ),
            Some((SketchLocus::Entity(point.id.clone()), horizontal.id.clone(),))
        );

        let ambiguous = line("ambiguous", Point2::new(0.0, 10.0), Point2::new(10.0, 10.0));
        assert_eq!(
            unique_repaired_profile_point_line_pair(
                &sketch,
                &SketchLocus::Entity(point.id.clone()),
                &wrong.id,
                &parameter,
                &[
                    point.clone(),
                    wrong.clone(),
                    horizontal.clone(),
                    ambiguous.clone(),
                ],
            ),
            None
        );

        let unrelated_point = SketchEntity {
            id: SketchEntityId("unrelated-point".into()),
            geometry: SketchGeometry::Point {
                position: Point2::new(20.0, 25.0),
            },
            ..point.clone()
        };
        let unrelated_line = line(
            "unrelated-line",
            Point2::new(20.0, 20.0),
            Point2::new(30.0, 20.0),
        );
        assert_eq!(
            unique_repaired_profile_point_line_pair(
                &sketch,
                &SketchLocus::Entity(point.id.clone()),
                &wrong.id,
                &parameter,
                &[
                    point.clone(),
                    wrong.clone(),
                    unrelated_point,
                    unrelated_line,
                ],
            ),
            None
        );
        assert_eq!(
            unique_profile_point_line_pair(
                &sketch,
                &parameter,
                &[point, horizontal, unrelated, ambiguous],
            ),
            None
        );
    }

    #[test]
    fn axis_relation_fallback_requires_one_aligned_locus_in_the_complete_sketch() {
        let sketch = SketchId("sketch".into());
        let point = |id: &str, u: f64, v: f64| SketchEntity {
            id: SketchEntityId(id.into()),
            sketch: sketch.clone(),
            construction: false,
            native_ref: None,
            geometry_ref: None,
            endpoint_refs: Vec::new(),
            geometry: SketchGeometry::Point {
                position: Point2::new(u, v),
            },
        };
        let first_entity = point("first-entity", 1.0, 2.0);
        let second_entity = point("second-entity", 4.0, 2.0);
        let unrelated = point("unrelated", 8.0, 9.0);
        let first = marker("first-marker", Some([0.001, 0.002]));
        let second = marker("second-marker", None);
        let mut relation = marker("relation", None);
        relation.links = vec![
            SketchInputLink {
                local_id: 1,
                entity_ref: first.id.clone(),
            },
            SketchInputLink {
                local_id: 2,
                entity_ref: second.id.clone(),
            },
        ];
        let markers = HashMap::from([(first.id.as_str(), &first), (second.id.as_str(), &second)]);
        let loci = HashMap::from([(
            first.id.clone(),
            vec![SketchLocus::Entity(first_entity.id.clone())],
        )]);
        assert_eq!(
            unique_axis_aligned_linked_loci(
                &relation,
                &sketch,
                &[
                    first_entity.clone(),
                    second_entity.clone(),
                    unrelated.clone()
                ],
                &markers,
                &loci,
                true,
            ),
            Some(vec![
                SketchLocus::Entity(first_entity.id.clone()),
                SketchLocus::Entity(second_entity.id.clone()),
            ])
        );

        let ambiguous = point("ambiguous", 6.0, 2.0);
        assert_eq!(
            unique_axis_aligned_linked_loci(
                &relation,
                &sketch,
                &[first_entity, second_entity, unrelated, ambiguous],
                &markers,
                &loci,
                true,
            ),
            None
        );
    }

    #[test]
    fn linked_locus_disambiguates_a_coordinate_collision() {
        let mut ambiguous = marker("ambiguous", None);
        ambiguous.links = vec![SketchInputLink {
            local_id: 2,
            entity_ref: "linked".into(),
        }];
        let linked = marker("linked", None);
        let markers = HashMap::from([
            (ambiguous.id.as_str(), &ambiguous),
            (linked.id.as_str(), &linked),
        ]);
        let expected = SketchLocus::Start(SketchEntityId("line-a".into()));
        let loci = HashMap::from([
            (
                ambiguous.id.clone(),
                vec![
                    expected.clone(),
                    SketchLocus::End(SketchEntityId("line-b".into())),
                ],
            ),
            (linked.id.clone(), vec![expected.clone()]),
        ]);

        assert_eq!(
            resolved_marker_locus(&ambiguous.id, &markers, &loci, &mut HashSet::new()),
            Some(expected)
        );
        assert_eq!(
            marker_entities(&ambiguous.id, &markers, &loci),
            vec![SketchEntityId("line-a".into())]
        );
    }

    #[test]
    fn point_handle_does_not_inherit_a_constraint_sibling_locus() {
        let mut point = marker("point", None);
        point.links = vec![SketchInputLink {
            local_id: 0,
            entity_ref: "relation".into(),
        }];
        let mut relation = marker("relation", None);
        relation.kind = SketchInputKind::Relation(SketchRelationKind::Distance);
        relation.links = vec![
            SketchInputLink {
                local_id: 1,
                entity_ref: point.id.clone(),
            },
            SketchInputLink {
                local_id: 3,
                entity_ref: "known".into(),
            },
        ];
        let known = marker("known", None);
        let markers = HashMap::from([
            (point.id.as_str(), &point),
            (relation.id.as_str(), &relation),
            (known.id.as_str(), &known),
        ]);
        let loci = HashMap::from([(
            known.id.clone(),
            vec![SketchLocus::Start(SketchEntityId("line".into()))],
        )]);

        assert_eq!(
            resolved_marker_locus(&point.id, &markers, &loci, &mut HashSet::new()),
            None
        );
    }

    #[test]
    fn pattern_inputs_bind_adjacent_objects_and_line_reference_direction() {
        let native_feature = |id: &str, source_id: &str, name: &str| NativeFeature {
            id: id.into(),
            parent: "history".into(),
            xml_tag: "Feature".into(),
            tree_parent: None,
            source_id: Some(source_id.into()),
            parent_source_id: None,
            ordinal: 0,
            name: name.into(),
            kind: String::new(),
            input_class: None,
            suppressed: false,
            parameters: BTreeMap::new(),
            dimension_properties: BTreeMap::new(),
            properties: BTreeMap::new(),
            text: None,
            content: Vec::new(),
        };
        let mut seed_native = native_feature("seed-native", "5", "SeedFeature");
        seed_native.input_class = Some("moExtrusion_c".into());
        let mut pattern_native = native_feature("pattern-native", "10", "Pattern1");
        pattern_native.input_class = Some("moCurvePattern_c".into());
        let mut path_native = native_feature("path-native", "20", "PathSketch");
        path_native.input_class = Some("moProfileFeature_c".into());
        let next_native = native_feature("next-native", "30", "NextFeature");
        let history = FeatureHistory {
            id: "history".into(),
            part_name: None,
            properties: BTreeMap::new(),
            content: Vec::new(),
            configurations: Vec::new(),
            features: vec![seed_native, pattern_native, path_native, next_native],
        };
        let name = |offset: u64, object_id: u32, value: &str| FeatureInputName {
            id: format!("name-{offset}"),
            parent: "lane".into(),
            ordinal: 0,
            offset,
            value: value.into(),
            object_id: Some(object_id),
        };
        let line_ref_offset = 120usize;
        let mut native_payload = vec![0; 400];
        native_payload[line_ref_offset + 136..line_ref_offset + 144]
            .copy_from_slice(&[0xc7, 0xcf, 0xff, 0xff, 0xc7, 0xcf, 0xff, 0xff]);
        native_payload[line_ref_offset + 148..line_ref_offset + 152]
            .copy_from_slice(&[0xf8, 0x2a, 0, 0]);
        for (index, value) in [-1.0f64, 0.0, 0.0].into_iter().enumerate() {
            let offset = line_ref_offset + 200 + index * 8;
            native_payload[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
        }
        assert_eq!(
            line_reference_direction(&native_payload, line_ref_offset as u64),
            Some(Vector3::new(-1.0, 0.0, 0.0))
        );
        let mut three_word_payload = vec![0; 400];
        three_word_payload[line_ref_offset + 144..line_ref_offset + 156].copy_from_slice(&[
            0xc7, 0xcf, 0xff, 0xff, 0xc7, 0xcf, 0xff, 0xff, 0xc7, 0xcf, 0xff, 0xff,
        ]);
        three_word_payload[line_ref_offset + 160..line_ref_offset + 164]
            .copy_from_slice(&[0xf8, 0x2a, 0, 0]);
        for (index, value) in [0.0f64, 0.6, 0.8].into_iter().enumerate() {
            let offset = line_ref_offset + 220 + index * 8;
            three_word_payload[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
        }
        assert_eq!(
            line_reference_direction(&three_word_payload, line_ref_offset as u64),
            Some(Vector3::new(0.0, 0.6, 0.8))
        );
        let lane = FeatureInputLane {
            id: "lane".into(),
            configuration: None,
            native_payload,
            classes: vec![FeatureInputClass {
                id: "line-reference".into(),
                parent: "lane".into(),
                ordinal: 0,
                offset: line_ref_offset as u64,
                name: "moLineRef_w".into(),
                role: FeatureInputClassRole::Reference,
            }],
            names: vec![
                name(50, 5, "SeedFeature"),
                name(100, 10, "Pattern1"),
                name(500, 20, "PathSketch"),
                name(600, 30, "NextFeature"),
            ],
            scalars: Vec::new(),
            relation_bindings: Vec::new(),
            relation_instances: Vec::new(),
            body_selections: Vec::new(),
            edge_selections: Vec::new(),
            surface_selections: Vec::new(),
            references: Vec::new(),
            sketch_entities: Vec::new(),
        };
        let model_feature = |id: &str, native_ref: &str, definition| Feature {
            id: FeatureId(id.into()),
            ordinal: 0,
            name: None,
            suppressed: false,
            parent: None,
            dependencies: Vec::new(),
            source_properties: BTreeMap::new(),
            source_tag: None,
            source_text: None,
            source_content: Vec::new(),
            outputs: Vec::new(),
            definition,
            native_ref: Some(native_ref.into()),
        };
        let sketch = SketchId("path-sketch".into());
        let mut features = vec![
            model_feature(
                "pattern",
                "pattern-native",
                FeatureDefinition::Pattern {
                    seeds: Vec::new(),
                    pattern: PatternKind::CurveDriven {
                        path: None,
                        spacing: Length(5.0),
                        count: 3,
                    },
                },
            ),
            model_feature(
                "path",
                "path-native",
                FeatureDefinition::Sketch {
                    space: SketchSpace::Planar,
                    sketch: Some(sketch.clone()),
                },
            ),
            model_feature(
                "seed",
                "seed-native",
                FeatureDefinition::Native {
                    kind: "Extrude".into(),
                    parameters: BTreeMap::new(),
                    properties: BTreeMap::new(),
                },
            ),
        ];

        bind_pattern_inputs(
            &mut features,
            std::slice::from_ref(&history),
            std::slice::from_ref(&lane),
        );

        assert!(matches!(
            features[0].definition,
            FeatureDefinition::Pattern {
                pattern: PatternKind::CurveDriven {
                    path: Some(PathRef::Sketch(ref path)),
                    ..
                },
                ..
            } if path == &sketch
        ));
        assert_eq!(
            features[0].dependencies,
            [features[2].id.clone(), features[1].id.clone()]
        );
        let FeatureDefinition::Pattern { seeds, .. } = &features[0].definition else {
            panic!("expected pattern");
        };
        assert_eq!(seeds, std::slice::from_ref(&features[2].id));

        let mut ambiguous_lane = lane.clone();
        ambiguous_lane.names.insert(2, name(450, 20, "PathSketch"));
        if let FeatureDefinition::Pattern {
            pattern: PatternKind::CurveDriven { path, .. },
            seeds,
            ..
        } = &mut features[0].definition
        {
            *path = None;
            seeds.clear();
        }
        bind_pattern_inputs(
            &mut features,
            std::slice::from_ref(&history),
            &[ambiguous_lane],
        );
        assert!(matches!(
            features[0].definition,
            FeatureDefinition::Pattern {
                pattern: PatternKind::CurveDriven { path: None, .. },
                ..
            }
        ));

        let mut linear_history = history.clone();
        linear_history.features[1].input_class = Some("moLPattern_c".into());
        features[0].dependencies.clear();
        features[0].definition = FeatureDefinition::Pattern {
            seeds: Vec::new(),
            pattern: PatternKind::Linear {
                direction: None,
                spacing: Length(5.0),
                count: 3,
            },
        };
        bind_pattern_inputs(
            &mut features,
            &[linear_history],
            std::slice::from_ref(&lane),
        );
        let FeatureDefinition::Pattern { seeds, .. } = &features[0].definition else {
            panic!("expected pattern");
        };
        assert_eq!(seeds, std::slice::from_ref(&features[2].id));
        assert_eq!(features[0].dependencies, [features[2].id.clone()]);
        assert!(matches!(
            features[0].definition,
            FeatureDefinition::Pattern {
                pattern: PatternKind::Linear {
                    direction: Some(Vector3 { x, y, z }),
                    ..
                },
                ..
            } if x == -1.0 && y == 0.0 && z == 0.0
        ));

        let mut sweep_history = history;
        sweep_history.features[0].input_class = Some("moProfileFeature_c".into());
        sweep_history.features[1].input_class = Some("moSweep_c".into());
        let path_sketch = SketchId("sweep-path".into());
        features[2].definition = FeatureDefinition::Sketch {
            space: SketchSpace::Planar,
            sketch: Some(path_sketch.clone()),
        };
        features[0].dependencies.clear();
        features[0].definition = FeatureDefinition::Sweep {
            profile: None,
            path: Some(PathRef::Native("curve-reference".into())),
            mode: SweepMode::Solid {
                op: cadmpeg_ir::features::BooleanOp::Join,
            },
            twist: None,
            scale: None,
        };
        bind_sweep_adjacent_profiles(&mut features, &[sweep_history], std::slice::from_ref(&lane));
        assert!(matches!(
            features[0].definition,
            FeatureDefinition::Sweep {
                profile: Some(cadmpeg_ir::features::ProfileRef::Sketch(ref profile)),
                path: Some(PathRef::Sketch(ref path)),
                ..
            } if profile == &sketch && path == &path_sketch
        ));
        assert_eq!(
            features[0].dependencies,
            [features[1].id.clone(), features[2].id.clone()]
        );
    }

    #[test]
    fn reused_point_handle_gets_one_solved_locus_per_dimension_relation() {
        let sketch = SketchId("sketch".into());
        let feature = Feature {
            id: FeatureId("feature".into()),
            ordinal: 0,
            name: None,
            suppressed: false,
            parent: None,
            dependencies: Vec::new(),
            source_properties: BTreeMap::new(),
            source_tag: None,
            source_text: None,
            source_content: Vec::new(),
            outputs: Vec::new(),
            definition: FeatureDefinition::Sketch {
                space: SketchSpace::Planar,
                sketch: Some(sketch.clone()),
            },
            native_ref: Some("feature-native".into()),
        };
        let point = |id: &str, marker: Option<&str>, u: f64| SketchEntity {
            id: SketchEntityId(id.into()),
            sketch: sketch.clone(),
            construction: false,
            native_ref: marker.map(str::to_owned),
            geometry_ref: None,
            endpoint_refs: Vec::new(),
            geometry: SketchGeometry::Point {
                position: Point2::new(u, 0.0),
            },
        };
        let mut entities = vec![
            point("origin", Some("known-a"), 0.0),
            point("middle", Some("known-b"), 5.0),
            point("far", None, 12.0),
        ];
        let known_a = marker("known-a", Some([0.0, 0.0]));
        let known_b = marker("known-b", Some([0.005, 0.0]));
        let missing = marker("missing", None);
        let operand = |index: usize, marker: &str| FeatureInputOperand {
            offset: index as u64,
            reference_ref: format!("reference-{index}"),
            kind: FeatureInputOperandKind::D6,
            entity_index: index as u16,
            entity_ref: Some(marker.into()),
        };
        let relation = |id: &str,
                        offset: u64,
                        family: FeatureInputRelationFamily,
                        known: &str,
                        scalar: &str| FeatureInputRelationInstance {
            id: id.into(),
            parent: "lane".into(),
            ordinal: 0,
            offset,
            family,
            class_ref: "class".into(),
            feature_ref: "feature-native".into(),
            scalar_refs: vec![scalar.into()],
            parameter_scalar_ref: Some(scalar.into()),
            display_scalar_ref: None,
            operands: vec![operand(0, known), operand(1, "missing")],
        };
        let relations = vec![
            relation(
                "relation-a",
                10,
                FeatureInputRelationFamily::PointPointDistance,
                "known-a",
                "scalar-a",
            ),
            relation(
                "relation-b",
                20,
                FeatureInputRelationFamily::PointPointDistance,
                "known-b",
                "scalar-b",
            ),
            relation(
                "relation-c",
                30,
                FeatureInputRelationFamily::PointPointHorizontalDistance,
                "known-b",
                "scalar-c",
            ),
        ];
        let lane = FeatureInputLane {
            id: "lane#test".into(),
            configuration: None,
            native_payload: Vec::new(),
            classes: Vec::new(),
            names: Vec::new(),
            scalars: Vec::new(),
            relation_bindings: Vec::new(),
            relation_instances: relations.clone(),
            body_selections: Vec::new(),
            edge_selections: Vec::new(),
            surface_selections: Vec::new(),
            references: Vec::new(),
            sketch_entities: vec![known_a, known_b, missing],
        };
        let parameter = |id: &str, scalar: &str, distance: f64| DesignParameter {
            id: ParameterId(id.into()),
            owner: feature.id.clone(),
            ordinal: 0,
            name: id.into(),
            expression: format!("{distance}mm"),
            display: None,
            value: Some(ParameterValue::Length(Length(distance))),
            dependencies: Vec::new(),
            properties: BTreeMap::new(),
            pmi: None,
            native_ref: Some(scalar.into()),
        };
        let parameters = vec![
            parameter("distance-a", "scalar-a", 5.0),
            parameter("distance-b", "scalar-b", 7.0),
            parameter("distance-c", "scalar-c", 7.0),
        ];

        project_relation_point_geometry(
            &mut entities,
            &[],
            std::slice::from_ref(&feature),
            std::slice::from_ref(&lane),
        );
        project_relation_solved_point_geometry(
            &mut entities,
            &[],
            std::slice::from_ref(&feature),
            &parameters,
            std::slice::from_ref(&lane),
        );

        let solved = entities
            .iter()
            .filter(|entity| entity.id.0.contains("dimension-point:"))
            .collect::<Vec<_>>();
        assert_eq!(solved.len(), 3);
        assert!(matches!(
            solved[0].geometry,
            SketchGeometry::Point { position } if position == Point2::new(5.0, 0.0)
        ));
        assert!(matches!(
            solved[1].geometry,
            SketchGeometry::Point { position } if position == Point2::new(12.0, 0.0)
        ));
        assert!(matches!(
            solved[2].geometry,
            SketchGeometry::Point { position } if position == Point2::new(12.0, 0.0)
        ));
        assert_ne!(solved[0].geometry_ref, solved[1].geometry_ref);
        assert_ne!(solved[1].geometry_ref, solved[2].geometry_ref);

        let markers = lane
            .sketch_entities
            .iter()
            .map(|marker| (marker.id.as_str(), marker))
            .collect::<HashMap<_, _>>();
        let loci = profile_loci_by_marker(
            std::slice::from_ref(&feature),
            &[],
            &entities,
            std::slice::from_ref(&lane),
        );
        for (index, relation) in relations.iter().enumerate() {
            let definition = typed_relation_definition(
                relation,
                Some(&parameters[index]),
                &sketch,
                &entities,
                &markers,
                &loci,
            );
            let second = match definition {
                Some(SketchConstraintDefinition::DistanceLoci { second, .. })
                | Some(SketchConstraintDefinition::HorizontalDistance { second, .. }) => second,
                other => panic!("unexpected relation definition: {other:?}"),
            };
            assert_eq!(second, SketchLocus::Entity(solved[index].id.clone()));
        }
    }

    #[test]
    fn circle_dimension_driver_supplies_the_center_operand() {
        let operand = |index, marker: &str| FeatureInputOperand {
            offset: u64::from(index),
            reference_ref: format!("reference-{index}"),
            kind: FeatureInputOperandKind::Native(0x929d),
            entity_index: index,
            entity_ref: Some(marker.into()),
        };
        let scalar = |id: &str, offset, operands| FeatureInputScalar {
            id: id.into(),
            parent: "lane".into(),
            feature_ref: Some("feature".into()),
            ordinal: 0,
            offset,
            object_id: 1,
            name: "dimension-name".into(),
            value: 1.0,
            role: FeatureInputScalarRole::Native,
            entity_indices: Vec::new(),
            operands,
        };
        let display_operand = operand(2, "display-handle");
        let display = FeatureInputScalar {
            role: FeatureInputScalarRole::Display,
            ..scalar("display", 10, vec![display_operand.clone()])
        };
        let driver = scalar(
            "driver",
            20,
            vec![display_operand.clone(), operand(1, "center")],
        );
        let lane = FeatureInputLane {
            id: "lane".into(),
            configuration: None,
            native_payload: Vec::new(),
            classes: Vec::new(),
            names: vec![FeatureInputName {
                id: "dimension-name".into(),
                parent: "lane".into(),
                ordinal: 0,
                offset: 0,
                value: "D1".into(),
                object_id: None,
            }],
            scalars: vec![display, driver],
            relation_bindings: Vec::new(),
            relation_instances: Vec::new(),
            body_selections: Vec::new(),
            edge_selections: Vec::new(),
            surface_selections: Vec::new(),
            references: Vec::new(),
            sketch_entities: Vec::new(),
        };
        let mut relations = vec![FeatureInputRelationInstance {
            id: "relation".into(),
            parent: "lane".into(),
            ordinal: 0,
            offset: 10,
            family: FeatureInputRelationFamily::CircleDiameter,
            class_ref: "class".into(),
            feature_ref: "feature".into(),
            scalar_refs: vec!["display".into()],
            parameter_scalar_ref: None,
            display_scalar_ref: Some("display".into()),
            operands: vec![display_operand],
        }];

        bind_circle_dimension_centers(&mut relations, &lane);

        assert_eq!(relations[0].scalar_refs, ["display", "driver"]);
        assert_eq!(relations[0].operands.len(), 2);
        assert_eq!(
            relations[0].operands[1].entity_ref.as_deref(),
            Some("center")
        );
    }

    #[test]
    fn point_distance_uses_unique_solved_pair_when_reference_hints_are_inconsistent() {
        let sketch = SketchId("sketch".into());
        let point = |id: &str, u: f64| SketchEntity {
            id: SketchEntityId(id.into()),
            sketch: sketch.clone(),
            construction: false,
            native_ref: Some(id.into()),
            geometry_ref: None,
            endpoint_refs: Vec::new(),
            geometry: SketchGeometry::Point {
                position: Point2::new(u, 0.0),
            },
        };
        let entities = vec![
            point("hint-a", 0.0),
            point("hint-b", 2.0),
            point("solved", 5.0),
        ];
        let hint_a = marker("hint-a", Some([0.0, 0.0]));
        let hint_b = marker("hint-b", Some([0.002, 0.0]));
        let markers = HashMap::from([(hint_a.id.as_str(), &hint_a), (hint_b.id.as_str(), &hint_b)]);
        let loci = HashMap::from([
            (
                hint_a.id.clone(),
                vec![SketchLocus::Entity(SketchEntityId("hint-a".into()))],
            ),
            (
                hint_b.id.clone(),
                vec![SketchLocus::Entity(SketchEntityId("hint-b".into()))],
            ),
        ]);
        let relation = FeatureInputRelationInstance {
            id: "relation".into(),
            parent: "lane".into(),
            ordinal: 0,
            offset: 0,
            family: FeatureInputRelationFamily::PointPointDistance,
            class_ref: "class".into(),
            feature_ref: "feature".into(),
            scalar_refs: vec!["scalar".into()],
            parameter_scalar_ref: Some("scalar".into()),
            display_scalar_ref: None,
            operands: vec![
                FeatureInputOperand {
                    offset: 1,
                    reference_ref: "reference-a".into(),
                    kind: FeatureInputOperandKind::D6,
                    entity_index: 0,
                    entity_ref: Some(hint_a.id.clone()),
                },
                FeatureInputOperand {
                    offset: 2,
                    reference_ref: "reference-b".into(),
                    kind: FeatureInputOperandKind::D6,
                    entity_index: 1,
                    entity_ref: Some(hint_b.id.clone()),
                },
            ],
        };
        let parameter = DesignParameter {
            id: ParameterId("distance".into()),
            owner: FeatureId("feature".into()),
            ordinal: 0,
            name: "D1".into(),
            expression: "5mm".into(),
            display: None,
            value: Some(ParameterValue::Length(Length(5.0))),
            dependencies: Vec::new(),
            properties: BTreeMap::new(),
            pmi: None,
            native_ref: Some("scalar".into()),
        };

        assert!(matches!(
            typed_relation_definition(
                &relation,
                Some(&parameter),
                &sketch,
                &entities,
                &markers,
                &loci,
            ),
            Some(SketchConstraintDefinition::DistanceLoci {
                first: SketchLocus::Entity(first),
                second: SketchLocus::Entity(second),
                ..
            }) if [&first, &second].contains(&&SketchEntityId("hint-a".into()))
                && [&first, &second].contains(&&SketchEntityId("solved".into()))
        ));

        let mut horizontal_relation = relation.clone();
        horizontal_relation.family = FeatureInputRelationFamily::PointPointHorizontalDistance;
        assert!(matches!(
            typed_relation_definition(
                &horizontal_relation,
                Some(&parameter),
                &sketch,
                &entities,
                &markers,
                &loci,
            ),
            Some(SketchConstraintDefinition::HorizontalDistance {
                first: SketchLocus::Entity(first),
                second: SketchLocus::Entity(second),
                ..
            }) if [&first, &second].contains(&&SketchEntityId("hint-a".into()))
                && [&first, &second].contains(&&SketchEntityId("solved".into()))
        ));

        let mut ambiguous_entities = entities;
        ambiguous_entities.push(point("other-solved", -5.0));
        for candidate in [&relation, &horizontal_relation] {
            assert_eq!(
                typed_relation_definition(
                    candidate,
                    Some(&parameter),
                    &sketch,
                    &ambiguous_entities,
                    &markers,
                    &loci,
                ),
                None
            );
        }

        let unrelated_entities = vec![
            point("hint-a", 0.0),
            point("hint-b", 2.0),
            point("unrelated-a", 10.0),
            point("unrelated-b", 15.0),
        ];
        for candidate in [&relation, &horizontal_relation] {
            assert_eq!(
                typed_relation_definition(
                    candidate,
                    Some(&parameter),
                    &sketch,
                    &unrelated_entities,
                    &markers,
                    &loci,
                ),
                None
            );
        }
    }

    #[test]
    fn display_scalar_name_resolves_one_unclaimed_owner_parameter() {
        let feature = Feature {
            id: FeatureId("feature".into()),
            ordinal: 0,
            name: None,
            suppressed: false,
            parent: None,
            dependencies: Vec::new(),
            source_properties: BTreeMap::new(),
            source_tag: None,
            source_text: None,
            source_content: Vec::new(),
            outputs: Vec::new(),
            definition: FeatureDefinition::Sketch {
                space: SketchSpace::Planar,
                sketch: None,
            },
            native_ref: Some("native-feature".into()),
        };
        let parameter = DesignParameter {
            id: ParameterId("parameter".into()),
            owner: feature.id.clone(),
            name: "D1".into(),
            ordinal: 0,
            expression: "12".into(),
            value: Some(ParameterValue::Length(Length(12.0))),
            display: None,
            properties: BTreeMap::new(),
            pmi: None,
            native_ref: None,
            dependencies: Vec::new(),
        };
        let scalar = FeatureInputScalar {
            id: "scalar".into(),
            parent: "lane".into(),
            feature_ref: Some("native-feature".into()),
            ordinal: 0,
            offset: 10,
            object_id: 1,
            name: "name".into(),
            value: 0.012,
            role: FeatureInputScalarRole::Display,
            entity_indices: Vec::new(),
            operands: Vec::new(),
        };
        let lane = FeatureInputLane {
            id: "lane".into(),
            configuration: None,
            native_payload: Vec::new(),
            classes: Vec::new(),
            names: vec![FeatureInputName {
                id: "name".into(),
                parent: "lane".into(),
                ordinal: 0,
                offset: 0,
                value: "D1".into(),
                object_id: None,
            }],
            scalars: vec![scalar.clone()],
            relation_bindings: Vec::new(),
            relation_instances: Vec::new(),
            body_selections: Vec::new(),
            edge_selections: Vec::new(),
            surface_selections: Vec::new(),
            references: Vec::new(),
            sketch_entities: Vec::new(),
        };
        let relation = FeatureInputRelationInstance {
            id: "relation".into(),
            parent: "lane".into(),
            ordinal: 0,
            offset: 10,
            family: FeatureInputRelationFamily::PointPointDistance,
            class_ref: "class".into(),
            feature_ref: "native-feature".into(),
            scalar_refs: vec!["scalar".into()],
            parameter_scalar_ref: None,
            display_scalar_ref: Some("scalar".into()),
            operands: Vec::new(),
        };
        assert_eq!(
            relation_parameter_by_display_name(
                &relation,
                &lane,
                std::slice::from_ref(&feature),
                std::slice::from_ref(&parameter),
            )
            .map(|parameter| &parameter.id),
            Some(&parameter.id)
        );

        let mut detached = scalar;
        detached.id = "driver".into();
        detached.role = FeatureInputScalarRole::Driving;
        detached.operands.clear();
        let mut detached_lane = lane.clone();
        detached_lane.scalars.push(detached);
        let mut detached_relation = vec![relation.clone()];
        bind_detached_relation_drivers(&mut detached_relation, &detached_lane);
        assert_eq!(
            detached_relation[0].parameter_scalar_ref.as_deref(),
            Some("driver")
        );
        assert_eq!(detached_relation[0].scalar_refs, ["scalar", "driver"]);

        let mut parameter = parameter;
        parameter.value = Some(ParameterValue::Integer(12));
        type_display_relation_parameters(
            std::slice::from_mut(&mut parameter),
            std::slice::from_ref(&feature),
            std::slice::from_ref(&FeatureInputLane {
                relation_instances: vec![FeatureInputRelationInstance {
                    family: FeatureInputRelationFamily::CircleDiameter,
                    ..relation.clone()
                }],
                ..lane.clone()
            }),
        );
        assert_eq!(parameter.value, Some(ParameterValue::Length(Length(12.0))));
        assert_eq!(parameter.expression, "<MOD-DIAM>12mm");
        assert_eq!(parameter.display, Some(DimensionDisplay::Diameter));

        parameter.value = Some(ParameterValue::Real(0.012));
        parameter.expression = "0.012".into();
        parameter.display = None;
        parameter.native_ref = Some("driver".into());
        type_display_relation_parameters(
            std::slice::from_mut(&mut parameter),
            std::slice::from_ref(&feature),
            std::slice::from_ref(&FeatureInputLane {
                relation_instances: vec![
                    FeatureInputRelationInstance {
                        family: FeatureInputRelationFamily::PointPointDistance,
                        parameter_scalar_ref: Some("driver".into()),
                        ..relation.clone()
                    },
                    FeatureInputRelationInstance {
                        family: FeatureInputRelationFamily::Angle,
                        parameter_scalar_ref: Some("other-driver".into()),
                        ..relation
                    },
                ],
                ..lane
            }),
        );
        assert_eq!(parameter.value, Some(ParameterValue::Length(Length(12.0))));
        assert_eq!(parameter.expression, "12mm");
    }

    #[test]
    fn axis_aligned_sketch_frame_projects_native_plane_coordinates() {
        let sketch = Sketch {
            id: SketchId("sketch".into()),
            name: None,
            configuration: None,
            origin: Point3::new(28.65, -35.0, 0.35),
            normal: Vector3::new(0.0, -1.0, 0.0),
            u_axis: Vector3::new(0.0, 0.0, -1.0),
            profiles: Vec::new(),
            native_ref: None,
        };
        let transform = sketch_frame_marker_transform(&sketch, 1.0e-8).expect("axis frame");
        assert_eq!(
            transform.apply((2_865_000_000, -2_385_000_000)),
            Some((2_420_000_000, 0))
        );
        let other = MarkerTransform {
            u_sign: 1,
            ..transform
        };
        assert_eq!(
            select_marker_transforms_by_frame(&[other, transform], &sketch, 1.0e-8),
            vec![transform]
        );
        let translated = MarkerTransform {
            translation: (17, 23),
            ..transform
        };
        assert_eq!(
            select_marker_transforms_by_frame(&[other, translated], &sketch, 1.0e-8),
            vec![translated]
        );
        assert_eq!(
            select_marker_transforms_by_frame(&[other], &sketch, 1.0e-8),
            vec![other]
        );
        assert_eq!(
            select_marker_transforms_by_frame(&[], &sketch, 1.0e-8),
            vec![transform]
        );
    }

    #[test]
    fn rotated_sketch_frame_projects_native_plane_coordinates() {
        let diagonal = std::f64::consts::FRAC_1_SQRT_2;
        let sketch = Sketch {
            id: SketchId("sketch".into()),
            name: None,
            configuration: None,
            origin: Point3::new(10.0, 3.0, 20.0),
            normal: Vector3::new(0.0, -1.0, 0.0),
            u_axis: Vector3::new(diagonal, 0.0, -diagonal),
            profiles: Vec::new(),
            native_ref: None,
        };
        let transform = sketch_frame_marker_transform(&sketch, 1.0e-8).expect("rotated frame");

        assert!(transform.affine_matrix.is_some());
        assert_eq!(
            transform.apply((1_100_000_000, 1_900_000_000)),
            Some(((std::f64::consts::SQRT_2 / 1.0e-8).round() as i64, 0))
        );
    }

    #[test]
    fn dimensioned_circle_materializes_from_an_alternate_handle_frame() {
        let sketch = SketchId("sketch".into());
        let feature = Feature {
            id: FeatureId("feature".into()),
            ordinal: 0,
            name: None,
            suppressed: false,
            parent: None,
            dependencies: Vec::new(),
            source_properties: BTreeMap::new(),
            source_tag: None,
            source_text: None,
            source_content: Vec::new(),
            outputs: Vec::new(),
            definition: FeatureDefinition::Sketch {
                space: SketchSpace::Planar,
                sketch: Some(sketch.clone()),
            },
            native_ref: Some("feature-native".into()),
        };
        let mut entities = vec![
            SketchEntity {
                id: SketchEntityId("horizontal".into()),
                sketch: sketch.clone(),
                construction: false,
                native_ref: None,
                geometry_ref: None,
                endpoint_refs: Vec::new(),
                geometry: SketchGeometry::Line {
                    start: Point2::new(10.0, 20.0),
                    end: Point2::new(30.0, 20.0),
                },
            },
            SketchEntity {
                id: SketchEntityId("vertical".into()),
                sketch: sketch.clone(),
                construction: false,
                native_ref: None,
                geometry_ref: None,
                endpoint_refs: Vec::new(),
                geometry: SketchGeometry::Line {
                    start: Point2::new(30.0, 20.0),
                    end: Point2::new(30.0, 50.0),
                },
            },
        ];
        let mut horizontal = marker("horizontal-marker", Some([0.020, 0.020]));
        horizontal.kind = SketchInputKind::LineOrCircle;
        horizontal.offset = 0;
        let mut vertical = marker("vertical-marker", Some([0.035, 0.030]));
        vertical.kind = SketchInputKind::LineOrCircle;
        vertical.offset = 32;
        let mut center = marker("circle-center", Some([0.040, 0.015]));
        center.kind = SketchInputKind::LineOrCircle;
        center.offset = 64;
        let mut native_payload = vec![0; 96];
        for offset in [0, 32, 64] {
            native_payload[offset + 23..offset + 27].copy_from_slice(&[0x04, 0x00, 0x02, 0x00]);
        }
        let relation = FeatureInputRelationInstance {
            id: "circle-relation".into(),
            parent: "lane".into(),
            feature_ref: "feature-native".into(),
            ordinal: 0,
            offset: 80,
            family: FeatureInputRelationFamily::CircleDiameter,
            class_ref: "circle-class".into(),
            parameter_scalar_ref: Some("circle-scalar".into()),
            display_scalar_ref: None,
            operands: vec![FeatureInputOperand {
                offset: 81,
                reference_ref: "circle-reference".into(),
                kind: FeatureInputOperandKind::Native(0x8ab6),
                entity_index: 0,
                entity_ref: Some("circle-center".into()),
            }],
            scalar_refs: Vec::new(),
        };
        let lane = FeatureInputLane {
            id: "lane".into(),
            configuration: None,
            native_payload,
            classes: Vec::new(),
            names: Vec::new(),
            scalars: Vec::new(),
            relation_bindings: Vec::new(),
            relation_instances: vec![relation],
            body_selections: Vec::new(),
            edge_selections: Vec::new(),
            surface_selections: Vec::new(),
            references: Vec::new(),
            sketch_entities: vec![horizontal, vertical, center],
        };
        let parameter = DesignParameter {
            id: ParameterId("diameter".into()),
            owner: FeatureId("feature".into()),
            name: "D1".into(),
            ordinal: 0,
            expression: String::new(),
            value: Some(ParameterValue::Length(Length(8.0))),
            display: Some(DimensionDisplay::Diameter),
            properties: BTreeMap::new(),
            pmi: None,
            native_ref: Some("circle-scalar".into()),
            dependencies: Vec::new(),
        };

        project_dimensioned_sketch_geometry(
            &mut entities,
            &[],
            &[],
            &[feature],
            &[parameter],
            std::slice::from_ref(&lane),
        );
        assert!(matches!(
            &entities[2].geometry,
            SketchGeometry::Circle { center, radius }
                if *center == Point2::new(15.0, 40.0) && *radius == Length(4.0)
        ));
        assert!(!entities[2].construction);

        let mut implicit_lane = lane;
        let mut implicit_center = marker("implicit-center", Some([0.010, 0.020]));
        implicit_center.local_id = Some(1);
        implicit_center.offset = 100;
        let mut implicit_radial = marker("implicit-radial", Some([0.013, 0.024]));
        implicit_radial.local_id = Some(2);
        implicit_radial.offset = 200;
        implicit_lane.sketch_entities = vec![implicit_center, implicit_radial];
        let (resolved, radius) = implicit_circle_marker(
            std::slice::from_ref(&implicit_lane),
            "feature-native",
            FeatureInputOperandKind::Native(0x83fe),
            0,
        )
        .expect("implicit circle pair");
        assert_eq!(resolved.id, "implicit-center");
        assert!((radius - 5.0).abs() < 1.0e-12);
    }

    #[test]
    fn unique_translation_joins_linked_endpoints_to_one_profile_entity() {
        let sketch = SketchId("sketch".into());
        let first = SketchEntityId("first".into());
        let second = SketchEntityId("second".into());
        let entities = vec![
            SketchEntity {
                id: first.clone(),
                sketch: sketch.clone(),
                construction: false,
                native_ref: None,
                geometry_ref: None,
                endpoint_refs: Vec::new(),
                geometry: SketchGeometry::Line {
                    start: Point2::new(10.0, 20.0),
                    end: Point2::new(20.0, 20.0),
                },
            },
            SketchEntity {
                id: second.clone(),
                sketch: sketch.clone(),
                construction: false,
                native_ref: None,
                geometry_ref: None,
                endpoint_refs: Vec::new(),
                geometry: SketchGeometry::Line {
                    start: Point2::new(20.0, 20.0),
                    end: Point2::new(20.0, 30.0),
                },
            },
        ];
        let feature = Feature {
            id: FeatureId("feature".into()),
            ordinal: 0,
            name: None,
            suppressed: false,
            parent: None,
            dependencies: Vec::new(),
            source_properties: BTreeMap::new(),
            source_tag: None,
            source_text: None,
            source_content: Vec::new(),
            outputs: Vec::new(),
            definition: FeatureDefinition::Sketch {
                space: SketchSpace::Planar,
                sketch: Some(sketch),
            },
            native_ref: Some("feature-native".into()),
        };
        let mut reference = marker("reference", None);
        reference.links = vec![
            SketchInputLink {
                local_id: 1,
                entity_ref: "marker-a".into(),
            },
            SketchInputLink {
                local_id: 2,
                entity_ref: "marker-b".into(),
            },
        ];
        reference.kind = SketchInputKind::Relation(SketchRelationKind::Vertical);
        reference.link_selector = Some(0);
        let mut native_payload = vec![0; 108];
        for offset in [0, 27, 54] {
            native_payload[offset + 23..offset + 27].copy_from_slice(&[0x05, 0x00, 0x01, 0x00]);
        }
        native_payload[81 + 23..81 + 27].copy_from_slice(&[0x04, 0x00, 0x02, 0x00]);
        let mut marker_a = marker("marker-a", Some([0.0, 0.0]));
        marker_a.offset = 0;
        let mut marker_b = marker("marker-b", Some([0.01, 0.0]));
        marker_b.offset = 27;
        let mut marker_c = marker("marker-c", Some([0.01, 0.01]));
        marker_c.offset = 54;
        let mut display = marker("display", Some([0.1, 0.1]));
        display.offset = 81;
        let lane = FeatureInputLane {
            id: "lane".into(),
            configuration: None,
            native_payload,
            classes: Vec::new(),
            names: Vec::new(),
            scalars: Vec::new(),
            relation_bindings: Vec::new(),
            relation_instances: Vec::new(),
            body_selections: Vec::new(),
            edge_selections: Vec::new(),
            surface_selections: Vec::new(),
            references: Vec::new(),
            sketch_entities: vec![marker_a, marker_b, marker_c, display, reference.clone()],
        };

        let joins = profile_loci_by_marker(&[feature], &[], &entities, std::slice::from_ref(&lane));
        assert!(joins.contains_key("marker-a"));
        assert!(joins.contains_key("marker-b"));
        assert!(joins.contains_key("marker-c"));
        assert_eq!(joins["marker-b"].len(), 2);
        assert!(!joins.contains_key("display"));
        let mut markers = lane
            .sketch_entities
            .iter()
            .map(|marker| (marker.id.as_str(), marker))
            .collect::<HashMap<_, _>>();
        assert_eq!(
            marker_entities("reference", &markers, &joins),
            vec![first.clone()]
        );
        let mut wrapper = marker("wrapper", None);
        wrapper.links = vec![SketchInputLink {
            local_id: 1,
            entity_ref: "marker-a".into(),
        }];
        let mut nested_reference = reference.clone();
        nested_reference.id = "nested-reference".into();
        nested_reference.links[0].entity_ref = wrapper.id.clone();
        markers.insert(wrapper.id.as_str(), &wrapper);
        markers.insert(nested_reference.id.as_str(), &nested_reference);
        assert_eq!(
            marker_entities("nested-reference", &markers, &joins),
            vec![first.clone()]
        );
        let mut cycle = marker("cycle", None);
        cycle.links = vec![SketchInputLink {
            local_id: 1,
            entity_ref: cycle.id.clone(),
        }];
        markers.insert(cycle.id.as_str(), &cycle);
        assert!(marker_entities("cycle", &markers, &joins).is_empty());
        assert_eq!(
            typed_marker_relation_definition(markers["reference"], &markers, &joins,),
            Some(SketchConstraintDefinition::Vertical {
                entity: first.clone(),
            })
        );
        let mut nested_horizontal = nested_reference.clone();
        nested_horizontal.kind = SketchInputKind::Relation(SketchRelationKind::HorizontalPoints);
        assert!(matches!(
            typed_marker_relation_definition(&nested_horizontal, &markers, &joins),
            Some(SketchConstraintDefinition::Native { ref native_kind, .. })
                if native_kind == "sldprt:marker-relation:25"
        ));
        let mut nested_native = nested_reference.clone();
        nested_native.kind = SketchInputKind::Native(28);
        assert_eq!(
            typed_marker_relation_definition(&nested_native, &markers, &joins),
            Some(SketchConstraintDefinition::Native {
                native_kind: "sldprt:marker-relation:28".into(),
                entities: vec![first.clone(), second.clone()],
                parameter: None,
                operands: vec![
                    SketchNativeOperand {
                        native_kind: "sldprt:marker-local-id".into(),
                        object_index: 1,
                        native_ref: Some("wrapper".into()),
                    },
                    SketchNativeOperand {
                        native_kind: "sldprt:marker-local-id".into(),
                        object_index: 2,
                        native_ref: Some("marker-b".into()),
                    },
                ],
            })
        );
        let mut coordinate_horizontal = marker("coordinate-horizontal", Some([0.0, 0.0]));
        coordinate_horizontal.kind = SketchInputKind::from_native_code_and_layout(4, true);
        let mut coordinate_loci = joins.clone();
        coordinate_loci.insert(
            coordinate_horizontal.id.clone(),
            vec![cadmpeg_ir::sketches::SketchLocus::Start(first.clone())],
        );
        markers.insert(coordinate_horizontal.id.as_str(), &coordinate_horizontal);
        assert_eq!(
            typed_marker_relation_definition(&coordinate_horizontal, &markers, &coordinate_loci,),
            Some(SketchConstraintDefinition::Horizontal {
                entity: first.clone(),
            })
        );
        let relation_point =
            SketchEntityId("sldprt:model:sketch-entity#relation-point:lane:1".into());
        let point_handle = marker("point-handle", None);
        let mut point_horizontal = marker("point-horizontal", None);
        point_horizontal.kind = SketchInputKind::Relation(SketchRelationKind::Horizontal);
        point_horizontal.links = vec![SketchInputLink {
            local_id: 1,
            entity_ref: point_handle.id.clone(),
        }];
        let mut point_loci = joins.clone();
        point_loci.insert(
            point_handle.id.clone(),
            vec![SketchLocus::Entity(relation_point.clone())],
        );
        markers.insert(point_handle.id.as_str(), &point_handle);
        markers.insert(point_horizontal.id.as_str(), &point_horizontal);
        assert!(matches!(
            typed_marker_relation_definition(&point_horizontal, &markers, &point_loci),
            Some(SketchConstraintDefinition::Native { entities, .. })
                if entities == vec![relation_point]
        ));
        let mut operandless_vertical = marker("operandless-vertical", None);
        operandless_vertical.kind = SketchInputKind::Relation(SketchRelationKind::Vertical);
        assert_eq!(
            typed_marker_relation_definition(&operandless_vertical, &markers, &joins),
            None
        );
        operandless_vertical.coordinates_m = Some([0.01, 0.02]);
        assert_eq!(
            typed_marker_relation_definition(&operandless_vertical, &markers, &joins),
            None
        );
        let mut parallel = marker("parallel", None);
        parallel.kind = SketchInputKind::Relation(SketchRelationKind::Parallel);
        parallel.links = vec![
            SketchInputLink {
                local_id: 1,
                entity_ref: "marker-a".into(),
            },
            SketchInputLink {
                local_id: 3,
                entity_ref: "marker-c".into(),
            },
        ];
        markers.insert(parallel.id.as_str(), &parallel);
        assert_eq!(
            typed_marker_relation_definition(&parallel, &markers, &joins),
            Some(SketchConstraintDefinition::Parallel {
                first: first.clone(),
                second: SketchEntityId("second".into()),
            })
        );
        let mut symmetric = marker("symmetric", None);
        symmetric.kind = SketchInputKind::Relation(SketchRelationKind::Symmetric);
        symmetric.links = parallel.links.clone();
        markers.insert(symmetric.id.as_str(), &symmetric);
        assert_eq!(
            typed_marker_relation_definition(&symmetric, &markers, &joins),
            Some(SketchConstraintDefinition::Native {
                native_kind: "sldprt:marker-relation:11".into(),
                entities: vec![first.clone(), SketchEntityId("second".into())],
                parameter: None,
                operands: vec![
                    cadmpeg_ir::sketches::SketchNativeOperand {
                        native_kind: "sldprt:marker-local-id".into(),
                        object_index: 1,
                        native_ref: Some("marker-a".into()),
                    },
                    cadmpeg_ir::sketches::SketchNativeOperand {
                        native_kind: "sldprt:marker-local-id".into(),
                        object_index: 3,
                        native_ref: Some("marker-c".into()),
                    },
                ],
            })
        );
        let mut coincident = marker("coincident", None);
        coincident.kind = SketchInputKind::Relation(SketchRelationKind::Coincident);
        coincident.links = parallel.links.clone();
        markers.insert(coincident.id.as_str(), &coincident);
        assert_eq!(
            typed_marker_relation_definition(&coincident, &markers, &joins),
            Some(SketchConstraintDefinition::CoincidentLoci {
                loci: vec![
                    cadmpeg_ir::sketches::SketchLocus::Start(first.clone()),
                    cadmpeg_ir::sketches::SketchLocus::End(SketchEntityId("second".into())),
                ],
            })
        );
        let mut horizontal_points = marker("horizontal-points", None);
        horizontal_points.kind = SketchInputKind::Relation(SketchRelationKind::HorizontalPoints);
        horizontal_points.links = parallel.links.clone();
        markers.insert(horizontal_points.id.as_str(), &horizontal_points);
        assert_eq!(
            typed_marker_relation_definition(&horizontal_points, &markers, &joins),
            Some(SketchConstraintDefinition::HorizontalPoints {
                first: cadmpeg_ir::sketches::SketchLocus::Start(first.clone()),
                second: cadmpeg_ir::sketches::SketchLocus::End(SketchEntityId("second".into())),
            })
        );
        let mut legacy_horizontal_points = marker("legacy-horizontal-points", None);
        legacy_horizontal_points.kind = SketchInputKind::Relation(SketchRelationKind::Horizontal);
        legacy_horizontal_points.links = parallel.links.clone();
        markers.insert(
            legacy_horizontal_points.id.as_str(),
            &legacy_horizontal_points,
        );
        assert_eq!(
            typed_marker_relation_definition(&legacy_horizontal_points, &markers, &joins),
            Some(SketchConstraintDefinition::HorizontalPoints {
                first: cadmpeg_ir::sketches::SketchLocus::Start(first.clone()),
                second: cadmpeg_ir::sketches::SketchLocus::End(SketchEntityId("second".into())),
            })
        );
        let mut entity_marker = marker("entity-marker", Some([0.01, 0.01]));
        entity_marker.kind = SketchInputKind::LineOrCircle;
        let mut midpoint = marker("midpoint", None);
        midpoint.kind = SketchInputKind::Relation(SketchRelationKind::Midpoint);
        midpoint.links = vec![
            SketchInputLink {
                local_id: 3,
                entity_ref: entity_marker.id.clone(),
            },
            SketchInputLink {
                local_id: 1,
                entity_ref: "marker-a".into(),
            },
        ];
        let mut midpoint_loci = joins.clone();
        midpoint_loci.insert(
            entity_marker.id.clone(),
            vec![cadmpeg_ir::sketches::SketchLocus::End(SketchEntityId(
                "second".into(),
            ))],
        );
        markers.insert(entity_marker.id.as_str(), &entity_marker);
        markers.insert(midpoint.id.as_str(), &midpoint);
        assert_eq!(
            typed_marker_relation_definition(&midpoint, &markers, &midpoint_loci),
            Some(SketchConstraintDefinition::Midpoint {
                point: cadmpeg_ir::sketches::SketchLocus::Start(first.clone()),
                entity: SketchEntityId("second".into()),
            })
        );
        let mut arc_marker = marker("arc-marker", None);
        arc_marker.kind = SketchInputKind::Arc;
        let mut arc_loci = midpoint_loci.clone();
        arc_loci.insert(
            arc_marker.id.clone(),
            vec![cadmpeg_ir::sketches::SketchLocus::Entity(SketchEntityId(
                "second".into(),
            ))],
        );
        markers.insert(arc_marker.id.as_str(), &arc_marker);
        for (kind, angle) in [
            (SketchRelationKind::ArcAngle90, std::f64::consts::FRAC_PI_2),
            (SketchRelationKind::ArcAngle180, std::f64::consts::PI),
            (
                SketchRelationKind::ArcAngle270,
                3.0 * std::f64::consts::FRAC_PI_2,
            ),
        ] {
            let mut arc_angle = marker("arc-angle", None);
            arc_angle.kind = SketchInputKind::Relation(kind);
            arc_angle.links = vec![SketchInputLink {
                local_id: 1,
                entity_ref: arc_marker.id.clone(),
            }];
            assert_eq!(
                typed_marker_relation_definition(&arc_angle, &markers, &arc_loci),
                Some(SketchConstraintDefinition::ArcAngle {
                    entity: SketchEntityId("second".into()),
                    angle: cadmpeg_ir::features::Angle(angle),
                })
            );
            arc_angle.links[0].entity_ref.clone_from(&entity_marker.id);
            assert!(matches!(
                typed_marker_relation_definition(&arc_angle, &markers, &arc_loci),
                Some(SketchConstraintDefinition::Native {
                    native_kind,
                    entities,
                    parameter: None,
                    operands,
                }) if native_kind == format!("sldprt:marker-relation:{}", kind.native_code())
                    && entities == vec![SketchEntityId("second".into())]
                    && operands.len() == 1
                    && operands[0].object_index == 1
                    && operands[0].native_ref.as_deref() == Some("entity-marker")
            ));
        }
        let relation = FeatureInputRelationInstance {
            id: "relation".into(),
            parent: "lane".into(),
            ordinal: 0,
            offset: 0,
            family: FeatureInputRelationFamily::PointPointDistance,
            class_ref: "class".into(),
            feature_ref: "feature-native".into(),
            scalar_refs: Vec::new(),
            parameter_scalar_ref: None,
            display_scalar_ref: None,
            operands: ["marker-a", "marker-c"]
                .into_iter()
                .enumerate()
                .map(|(index, marker)| FeatureInputOperand {
                    offset: index as u64,
                    reference_ref: format!("reference-{index}"),
                    kind: FeatureInputOperandKind::D6,
                    entity_index: index as u16,
                    entity_ref: Some(marker.into()),
                })
                .collect(),
        };
        let parameter = |id: &str, display| DesignParameter {
            id: ParameterId(id.into()),
            owner: FeatureId("feature".into()),
            ordinal: 0,
            name: id.into(),
            expression: String::new(),
            display,
            value: Some(ParameterValue::Length(Length(2.0))),
            dependencies: Vec::new(),
            properties: BTreeMap::new(),
            pmi: None,
            native_ref: None,
        };
        let sketch_id = SketchId("sketch".into());
        let distance = parameter("distance", None);
        assert!(matches!(
            typed_relation_definition(
                &relation,
                Some(&distance),
                &sketch_id,
                &[],
                &markers,
                &joins,
            ),
            Some(cadmpeg_ir::sketches::SketchConstraintDefinition::DistanceLoci {
                parameter,
                ..
            }) if parameter.0 == "distance"
        ));
        let same_locus_relation = FeatureInputRelationInstance {
            operands: relation
                .operands
                .iter()
                .cloned()
                .map(|mut operand| {
                    operand.entity_ref = Some("marker-a".into());
                    operand
                })
                .collect(),
            ..relation.clone()
        };
        assert_eq!(
            typed_relation_definition(
                &same_locus_relation,
                Some(&distance),
                &sketch_id,
                &[],
                &markers,
                &joins,
            ),
            None
        );
        let circle = FeatureInputRelationInstance {
            family: FeatureInputRelationFamily::CircleDiameter,
            operands: vec![FeatureInputOperand {
                offset: 0,
                reference_ref: "circle-reference".into(),
                kind: FeatureInputOperandKind::E1,
                entity_index: 0,
                entity_ref: Some("marker-a".into()),
            }],
            ..relation
        };
        let radius = parameter("circle", Some(DimensionDisplay::Radius));
        assert!(matches!(
            typed_relation_definition(
                &circle,
                Some(&radius),
                &sketch_id,
                &[],
                &markers,
                &joins,
            ),
            Some(SketchConstraintDefinition::Radius { parameter, .. })
                if parameter.0 == "circle"
        ));
        let diameter = parameter("circle", Some(DimensionDisplay::Diameter));
        assert!(matches!(
            typed_relation_definition(
                &circle,
                Some(&diameter),
                &sketch_id,
                &[],
                &markers,
                &joins,
            ),
            Some(SketchConstraintDefinition::Diameter { parameter, .. })
                if parameter.0 == "circle"
        ));
        let undisplayed = parameter("circle", None);
        assert_eq!(
            typed_relation_definition(
                &circle,
                Some(&undisplayed),
                &sketch_id,
                &[],
                &markers,
                &joins,
            ),
            None
        );
        let unresolved_circle = FeatureInputRelationInstance {
            operands: vec![FeatureInputOperand {
                entity_ref: None,
                ..circle.operands[0].clone()
            }],
            ..circle
        };
        let circle_entity = SketchEntity {
            id: SketchEntityId("dimensioned-circle".into()),
            sketch: sketch_id.clone(),
            construction: false,
            native_ref: None,
            geometry_ref: None,
            endpoint_refs: Vec::new(),
            geometry: SketchGeometry::Circle {
                center: Point2::new(0.0, 0.0),
                radius: Length(2.0),
            },
        };
        assert!(matches!(
            typed_relation_definition(
                &unresolved_circle,
                Some(&radius),
                &sketch_id,
                std::slice::from_ref(&circle_entity),
                &markers,
                &joins,
            ),
            Some(SketchConstraintDefinition::Radius { entity, .. })
                if entity == circle_entity.id
        ));
        let mut duplicate_circle = circle_entity.clone();
        duplicate_circle.id = SketchEntityId("duplicate-circle".into());
        assert_eq!(
            typed_relation_definition(
                &unresolved_circle,
                Some(&radius),
                &sketch_id,
                &[circle_entity, duplicate_circle],
                &markers,
                &joins,
            ),
            None
        );
    }

    #[test]
    fn line_handle_interior_points_identify_profile_entities() {
        let sketch = SketchId("sketch".into());
        let line_ids = ["horizontal", "vertical", "offset"].map(|id| SketchEntityId(id.into()));
        let entities = vec![
            SketchEntity {
                id: line_ids[0].clone(),
                sketch: sketch.clone(),
                construction: false,
                native_ref: None,
                geometry_ref: None,
                endpoint_refs: Vec::new(),
                geometry: SketchGeometry::Line {
                    start: Point2::new(0.0, 0.0),
                    end: Point2::new(10.0, 0.0),
                },
            },
            SketchEntity {
                id: line_ids[1].clone(),
                sketch: sketch.clone(),
                construction: false,
                native_ref: None,
                geometry_ref: None,
                endpoint_refs: Vec::new(),
                geometry: SketchGeometry::Line {
                    start: Point2::new(0.0, 0.0),
                    end: Point2::new(0.0, 20.0),
                },
            },
            SketchEntity {
                id: line_ids[2].clone(),
                sketch: sketch.clone(),
                construction: false,
                native_ref: None,
                geometry_ref: None,
                endpoint_refs: Vec::new(),
                geometry: SketchGeometry::Line {
                    start: Point2::new(10.0, 3.0),
                    end: Point2::new(20.0, 3.0),
                },
            },
        ];
        let feature = Feature {
            id: FeatureId("feature".into()),
            ordinal: 0,
            name: None,
            suppressed: false,
            parent: None,
            dependencies: Vec::new(),
            source_properties: BTreeMap::new(),
            source_tag: None,
            source_text: None,
            source_content: Vec::new(),
            outputs: Vec::new(),
            definition: FeatureDefinition::Sketch {
                space: SketchSpace::Planar,
                sketch: Some(sketch),
            },
            native_ref: Some("feature-native".into()),
        };
        let mut native_payload = vec![0; 81];
        let mut markers = Vec::new();
        for (ordinal, (id, coordinates_m)) in [
            ("horizontal-marker", [0.0025, 0.0]),
            ("vertical-marker", [0.0, 0.010]),
            ("offset-marker", [0.015, 0.003]),
        ]
        .into_iter()
        .enumerate()
        {
            let offset = ordinal * 27;
            native_payload[offset + 23..offset + 27].copy_from_slice(&[0x05, 0x00, 0x01, 0x00]);
            let mut handle = marker(id, Some(coordinates_m));
            handle.ordinal = ordinal as u32;
            handle.offset = offset as u64;
            handle.kind = SketchInputKind::LineOrCircle;
            markers.push(handle);
        }
        let lane = FeatureInputLane {
            id: "lane".into(),
            configuration: None,
            native_payload,
            classes: Vec::new(),
            names: Vec::new(),
            scalars: Vec::new(),
            relation_bindings: Vec::new(),
            relation_instances: Vec::new(),
            body_selections: Vec::new(),
            edge_selections: Vec::new(),
            surface_selections: Vec::new(),
            references: Vec::new(),
            sketch_entities: markers,
        };

        let joins = profile_loci_by_marker(&[feature], &[], &entities, std::slice::from_ref(&lane));
        for (marker, entity) in [
            ("horizontal-marker", &line_ids[0]),
            ("vertical-marker", &line_ids[1]),
            ("offset-marker", &line_ids[2]),
        ] {
            assert_eq!(
                joins[marker],
                vec![cadmpeg_ir::sketches::SketchLocus::Entity(entity.clone())]
            );
        }
    }

    #[test]
    fn coordinate_less_point_handle_selects_one_shared_endpoint() {
        let sketch = SketchId("sketch".into());
        let first_id = SketchEntityId("first".into());
        let second_id = SketchEntityId("second".into());
        let first = SketchEntity {
            id: first_id.clone(),
            sketch: sketch.clone(),
            construction: false,
            native_ref: None,
            geometry_ref: None,
            endpoint_refs: Vec::new(),
            geometry: SketchGeometry::Line {
                start: Point2::new(0.0, 0.0),
                end: Point2::new(1.0, 0.0),
            },
        };
        let second = SketchEntity {
            id: second_id.clone(),
            sketch,
            construction: false,
            native_ref: None,
            geometry_ref: None,
            endpoint_refs: Vec::new(),
            geometry: SketchGeometry::Line {
                start: Point2::new(1.0, 0.0),
                end: Point2::new(1.0, 1.0),
            },
        };
        let mut first_marker = marker("first-marker", Some([0.0, 0.0]));
        first_marker.kind = SketchInputKind::LineOrCircle;
        let mut second_marker = marker("second-marker", Some([0.0, 0.0]));
        second_marker.kind = SketchInputKind::LineOrCircle;
        let mut point = marker("point", None);
        point.links = vec![
            SketchInputLink {
                local_id: 1,
                entity_ref: first_marker.id.clone(),
            },
            SketchInputLink {
                local_id: 2,
                entity_ref: second_marker.id.clone(),
            },
        ];
        let markers = HashMap::from([
            (first_marker.id.as_str(), &first_marker),
            (second_marker.id.as_str(), &second_marker),
            (point.id.as_str(), &point),
        ]);
        let loci = HashMap::from([
            (
                first_marker.id.clone(),
                vec![SketchLocus::Entity(first_id.clone())],
            ),
            (
                second_marker.id.clone(),
                vec![SketchLocus::Entity(second_id.clone())],
            ),
        ]);
        let entities = HashMap::from([(&first.id, &first), (&second.id, &second)]);

        assert_eq!(
            unique_linked_endpoint_locus(&point, &markers, &loci, &entities, 1.0e-8),
            Some(SketchLocus::End(first_id))
        );

        let mut ambiguous = second;
        ambiguous.geometry = SketchGeometry::Line {
            start: Point2::new(0.0, 0.0),
            end: Point2::new(1.0, 0.0),
        };
        let entities = HashMap::from([(&first.id, &first), (&ambiguous.id, &ambiguous)]);
        assert_eq!(
            unique_linked_endpoint_locus(&point, &markers, &loci, &entities, 1.0e-8),
            None
        );
    }

    #[test]
    fn curve_handles_reject_point_geometry() {
        let point = SketchGeometry::Point {
            position: Point2::new(0.0, 0.0),
        };
        let line = SketchGeometry::Line {
            start: Point2::new(0.0, 0.0),
            end: Point2::new(1.0, 0.0),
        };
        let circle = SketchGeometry::Circle {
            center: Point2::new(0.0, 0.0),
            radius: Length(1.0),
        };

        assert!(!super::marker_accepts_locus(
            SketchInputKind::LineOrCircle,
            &point
        ));
        assert!(super::marker_accepts_locus(
            SketchInputKind::LineOrCircle,
            &line
        ));
        assert!(super::marker_accepts_locus(
            SketchInputKind::LineOrCircle,
            &circle
        ));
    }

    #[test]
    fn symmetry_invariant_marker_identifies_profile_entity() {
        let sketch = SketchId("sketch".into());
        let circle = SketchEntityId("circle".into());
        let entity = SketchEntity {
            id: circle.clone(),
            sketch: sketch.clone(),
            construction: false,
            native_ref: None,
            geometry_ref: None,
            endpoint_refs: Vec::new(),
            geometry: SketchGeometry::Circle {
                center: Point2::new(0.0, 0.0),
                radius: Length(10.0),
            },
        };
        let points = [-10.0, 10.0].map(|u| SketchEntity {
            id: SketchEntityId(format!("point-{u}")),
            sketch: sketch.clone(),
            construction: true,
            native_ref: None,
            geometry_ref: None,
            endpoint_refs: Vec::new(),
            geometry: SketchGeometry::Point {
                position: Point2::new(u, 0.0),
            },
        });
        let feature = Feature {
            id: FeatureId("feature".into()),
            ordinal: 0,
            name: None,
            suppressed: false,
            parent: None,
            dependencies: Vec::new(),
            source_properties: BTreeMap::new(),
            source_tag: None,
            source_text: None,
            source_content: Vec::new(),
            outputs: Vec::new(),
            definition: FeatureDefinition::Sketch {
                space: SketchSpace::Planar,
                sketch: Some(sketch),
            },
            native_ref: Some("feature-native".into()),
        };
        let mut native_payload = vec![0; 54];
        for offset in [0, 27] {
            native_payload[offset + 23..offset + 27].copy_from_slice(&[0x05, 0x00, 0x01, 0x00]);
        }
        let mut handle = marker("circle-marker", Some([0.0, 0.0]));
        handle.kind = SketchInputKind::LineOrCircle;
        let mut point = marker("point-marker", Some([0.01, 0.0]));
        point.ordinal = 1;
        point.offset = 27;
        let lane = FeatureInputLane {
            id: "lane".into(),
            configuration: None,
            native_payload,
            classes: Vec::new(),
            names: Vec::new(),
            scalars: Vec::new(),
            relation_bindings: Vec::new(),
            relation_instances: Vec::new(),
            body_selections: Vec::new(),
            edge_selections: Vec::new(),
            surface_selections: Vec::new(),
            references: Vec::new(),
            sketch_entities: vec![handle, point],
        };

        let mut entities = vec![entity];
        entities.extend(points);
        let joins = profile_loci_by_marker(&[feature], &[], &entities, &[lane]);
        assert_eq!(
            joins["circle-marker"],
            vec![cadmpeg_ir::sketches::SketchLocus::Entity(circle)]
        );
    }

    #[test]
    fn unique_axis_swap_maps_marker_coordinates_to_profile_loci() {
        let markers = [(0, 0), (2, 1), (7, 4), (3, 9)].into_iter().collect();
        let loci = [(0, 0), (1, 2), (4, 7), (9, 3)].into_iter().collect();
        let transform = unique_marker_transform(&markers, &loci).expect("unique transform");
        assert!(transform.swap);
        assert_eq!(transform.u_sign, 1);
        assert_eq!(transform.v_sign, 1);
        assert!(markers
            .into_iter()
            .all(|point| loci.contains(&transform.apply(point).unwrap())));
    }

    #[test]
    fn relation_point_materializes_under_one_proven_marker_transform() {
        let sketch = SketchId("sketch".into());
        let feature = Feature {
            id: FeatureId("feature".into()),
            ordinal: 0,
            name: None,
            suppressed: false,
            parent: None,
            dependencies: Vec::new(),
            source_properties: BTreeMap::new(),
            source_tag: None,
            source_text: None,
            source_content: Vec::new(),
            outputs: Vec::new(),
            definition: FeatureDefinition::Sketch {
                space: SketchSpace::Planar,
                sketch: Some(sketch.clone()),
            },
            native_ref: Some("feature-native".into()),
        };
        let mut entities = [(0.0, 0.0), (1.0, 2.0), (4.0, 7.0)]
            .into_iter()
            .enumerate()
            .map(|(index, (u, v))| SketchEntity {
                id: SketchEntityId(format!("point-{index}")),
                sketch: sketch.clone(),
                construction: false,
                native_ref: None,
                geometry_ref: None,
                endpoint_refs: Vec::new(),
                geometry: SketchGeometry::Point {
                    position: Point2::new(u, v),
                },
            })
            .collect::<Vec<_>>();
        let mut markers = [[0.0, 0.0], [0.002, 0.001], [0.007, 0.004]]
            .into_iter()
            .enumerate()
            .map(|(index, coordinates)| {
                let mut value = marker(&format!("anchor-{index}"), Some(coordinates));
                value.offset = (index * 27) as u64;
                value
            })
            .collect::<Vec<_>>();
        let mut relation_point = marker("relation-point", Some([0.005, 0.006]));
        relation_point.offset = 81;
        markers.push(relation_point.clone());
        let mut endpoint_a = marker("endpoint-a", Some([0.002, 0.001]));
        endpoint_a.offset = 82;
        let mut endpoint_b = marker("endpoint-b", Some([0.007, 0.004]));
        endpoint_b.offset = 83;
        let mut relation_line = marker("relation-line", None);
        relation_line.offset = 84;
        relation_line.kind = SketchInputKind::LineOrCircle;
        endpoint_a.links = vec![SketchInputLink {
            local_id: 1,
            entity_ref: relation_line.id.clone(),
        }];
        endpoint_b.links = vec![SketchInputLink {
            local_id: 2,
            entity_ref: relation_line.id.clone(),
        }];
        let mut support_handle = marker("support-handle", None);
        support_handle.offset = 85;
        support_handle.links = vec![SketchInputLink {
            local_id: 3,
            entity_ref: relation_line.id.clone(),
        }];
        let mut qualified_curve = marker("qualified-curve", Some([0.0045, 0.0025]));
        qualified_curve.id = "sldprt:feature-input:sketch-entity#qualified-curve".into();
        qualified_curve.offset = 86;
        qualified_curve.kind = SketchInputKind::LineOrCircle;
        let mut coincident_point = marker("coincident-point", Some([0.002, 0.001]));
        coincident_point.offset = 87;
        markers.extend([
            endpoint_a,
            endpoint_b,
            relation_line.clone(),
            support_handle.clone(),
            qualified_curve.clone(),
            coincident_point.clone(),
        ]);
        let mut native_payload = vec![0; 108];
        for offset in [0, 27, 54] {
            native_payload[offset + 23..offset + 27].copy_from_slice(&[0x05, 0x00, 0x01, 0x00]);
        }
        let lane = FeatureInputLane {
            id: "lane".into(),
            configuration: None,
            native_payload,
            classes: Vec::new(),
            names: Vec::new(),
            scalars: Vec::new(),
            relation_bindings: Vec::new(),
            relation_instances: vec![
                FeatureInputRelationInstance {
                    id: "relation".into(),
                    parent: "lane".into(),
                    ordinal: 0,
                    offset: 90,
                    family: FeatureInputRelationFamily::CircleDiameter,
                    class_ref: "class".into(),
                    feature_ref: "feature-native".into(),
                    scalar_refs: Vec::new(),
                    parameter_scalar_ref: None,
                    display_scalar_ref: None,
                    operands: vec![FeatureInputOperand {
                        offset: 91,
                        reference_ref: "reference".into(),
                        kind: FeatureInputOperandKind::Native(0x929d),
                        entity_index: 0,
                        entity_ref: Some(relation_point.id.clone()),
                    }],
                },
                FeatureInputRelationInstance {
                    id: "qualified-point-relation".into(),
                    parent: "lane".into(),
                    ordinal: 2,
                    offset: 94,
                    family: FeatureInputRelationFamily::PointPointDistance,
                    class_ref: "class".into(),
                    feature_ref: "feature-native".into(),
                    scalar_refs: Vec::new(),
                    parameter_scalar_ref: None,
                    display_scalar_ref: None,
                    operands: vec![
                        FeatureInputOperand {
                            offset: 95,
                            reference_ref: "qualified-reference".into(),
                            kind: FeatureInputOperandKind::Native(0x837b),
                            entity_index: 16,
                            entity_ref: Some(qualified_curve.id.clone()),
                        },
                        FeatureInputOperand {
                            offset: 96,
                            reference_ref: "point-reference".into(),
                            kind: FeatureInputOperandKind::Native(0x837b),
                            entity_index: 17,
                            entity_ref: Some(relation_point.id.clone()),
                        },
                    ],
                },
                FeatureInputRelationInstance {
                    id: "line-relation".into(),
                    parent: "lane".into(),
                    ordinal: 1,
                    offset: 92,
                    family: FeatureInputRelationFamily::LineLineDistance,
                    class_ref: "class".into(),
                    feature_ref: "feature-native".into(),
                    scalar_refs: Vec::new(),
                    parameter_scalar_ref: None,
                    display_scalar_ref: None,
                    operands: vec![FeatureInputOperand {
                        offset: 93,
                        reference_ref: "line-reference".into(),
                        kind: FeatureInputOperandKind::Native(0x8386),
                        entity_index: 0,
                        entity_ref: Some(support_handle.id),
                    }],
                },
                FeatureInputRelationInstance {
                    id: "coincident-point-relation".into(),
                    parent: "lane".into(),
                    ordinal: 3,
                    offset: 97,
                    family: FeatureInputRelationFamily::PointPointDistance,
                    class_ref: "class".into(),
                    feature_ref: "feature-native".into(),
                    scalar_refs: Vec::new(),
                    parameter_scalar_ref: None,
                    display_scalar_ref: None,
                    operands: vec![
                        FeatureInputOperand {
                            offset: 98,
                            reference_ref: "coincident-reference".into(),
                            kind: FeatureInputOperandKind::Native(0x837b),
                            entity_index: 18,
                            entity_ref: Some(coincident_point.id.clone()),
                        },
                        FeatureInputOperand {
                            offset: 99,
                            reference_ref: "coincident-pair-reference".into(),
                            kind: FeatureInputOperandKind::Native(0x837b),
                            entity_index: 17,
                            entity_ref: Some(relation_point.id.clone()),
                        },
                    ],
                },
            ],
            body_selections: Vec::new(),
            edge_selections: Vec::new(),
            surface_selections: Vec::new(),
            references: Vec::new(),
            sketch_entities: markers,
        };
        project_relation_point_geometry(
            &mut entities,
            &[],
            std::slice::from_ref(&feature),
            std::slice::from_ref(&lane),
        );
        assert!(entities.iter().any(|entity| {
            entity.construction
                && entity.native_ref.as_deref() == Some("relation-point")
                && matches!(
                    entity.geometry,
                    SketchGeometry::Point { position } if position == Point2::new(6.0, 5.0)
                )
        }));
        assert!(entities.iter().any(|entity| {
            entity.construction
                && entity.native_ref.as_deref() == Some("coincident-point")
                && matches!(
                    entity.geometry,
                    SketchGeometry::Point { position } if position == Point2::new(1.0, 2.0)
                )
        }));
        assert!(entities.iter().any(|entity| {
            entity.construction
                && entity.native_ref.is_none()
                && entity.geometry_ref.as_deref()
                    == Some("sldprt:feature-input:sketch-entity#qualified-curve")
                && matches!(
                    entity.geometry,
                    SketchGeometry::Point { position } if position == Point2::new(2.5, 4.5)
                )
        }));
        assert!(entities.iter().any(|entity| {
            entity.construction
                && entity.native_ref.as_deref() == Some("relation-line")
                && entity.endpoint_refs == ["endpoint-a", "endpoint-b"]
                && matches!(entity.geometry, SketchGeometry::Line { start, end }
                    if start == Point2::new(1.0, 2.0) && end == Point2::new(4.0, 7.0))
        }));
        let loci = profile_loci_by_marker(
            std::slice::from_ref(&feature),
            &[],
            &entities,
            std::slice::from_ref(&lane),
        );
        assert_eq!(
            loci["sldprt:feature-input:sketch-entity#qualified-curve:qualified-point"],
            vec![SketchLocus::Entity(SketchEntityId(
                "sldprt:model:sketch-entity#relation-point:lane:86".into(),
            ))]
        );
        let markers = lane
            .sketch_entities
            .iter()
            .map(|marker| (marker.id.as_str(), marker))
            .collect::<HashMap<_, _>>();
        assert_eq!(
            marker_point_locus(
                "sldprt:feature-input:sketch-entity#qualified-curve",
                &markers,
                &loci,
            ),
            Some(SketchLocus::Entity(SketchEntityId(
                "sldprt:model:sketch-entity#relation-point:lane:86".into(),
            )))
        );
    }

    #[test]
    fn unique_zero_translation_resolves_symmetric_axis_swaps() {
        let markers = [(0, 0), (48, 0), (48, 24), (0, 24)].into_iter().collect();
        let loci = [(0, 0), (24, 0), (24, 48), (0, 48)].into_iter().collect();
        assert_eq!(
            unique_marker_transform(&markers, &loci),
            Some(MarkerTransform {
                swap: true,
                u_sign: 1,
                v_sign: 1,
                affine_matrix: None,
                translation: (0, 0),
            })
        );
    }

    #[test]
    fn marker_kinds_disambiguate_axis_swaps() {
        let compatible = HashMap::from([
            ((0, 0), HashSet::from([(10, 20)])),
            ((0, 2), HashSet::from([(12, 20)])),
            ((3, 1), HashSet::from([(11, 23)])),
        ]);
        let transform = unique_compatible_marker_transform(&compatible).unwrap();
        assert!(transform.swap);
        assert_eq!(transform.u_sign, 1);
        assert_eq!(transform.v_sign, 1);
        assert_eq!(transform.translation, (10, 20));
    }

    #[test]
    fn symmetric_frames_require_the_same_dimensioned_circle_set() {
        let identity = MarkerTransform {
            swap: false,
            u_sign: 1,
            v_sign: 1,
            affine_matrix: None,
            translation: (0, 0),
        };
        let swap = MarkerTransform {
            swap: true,
            ..identity
        };
        assert_eq!(
            dimensioned_circle_transform(&[swap, identity], &[((10, 20), 5), ((20, 10), 5)]),
            Some(identity)
        );
        assert_eq!(
            dimensioned_circle_transform(&[identity, swap], &[((10, 20), 5), ((20, 10), 7)]),
            None
        );
    }

    #[test]
    fn cylinder_centers_resolve_dimensioned_circle_frame() {
        let sketch = Sketch {
            id: SketchId("sketch".into()),
            name: None,
            configuration: None,
            origin: Point3::new(20.0, 20.0, 0.0),
            normal: Vector3::new(-1.0, 0.0, 0.0),
            u_axis: Vector3::new(0.0, 0.0, 1.0),
            profiles: Vec::new(),
            native_ref: None,
        };
        let circles = [((6, 14), 3), ((14, 14), 3), ((14, 7), 3), ((6, 7), 3)];
        let surfaces = [(14.0, -6.0), (14.0, -14.0), (7.0, -14.0), (7.0, -6.0)]
            .into_iter()
            .enumerate()
            .map(|(index, (y, z))| Surface {
                id: SurfaceId(format!("cylinder-{index}")),
                geometry: SurfaceGeometry::Cylinder {
                    origin: Point3::new(19.5, y, z),
                    axis: Vector3::new(1.0, 0.0, 0.0),
                    ref_direction: Vector3::new(0.0, 1.0, 0.0),
                    radius: 3.0,
                },
                source_object: None,
            })
            .collect::<Vec<_>>();
        let candidates = dimensioned_circle_surface_transforms(&sketch, &surfaces, &circles, 1.0);
        let transform = dimensioned_circle_transform(&candidates, &circles).unwrap();
        let transformed = circles
            .iter()
            .map(|(center, _)| transform.apply(*center).unwrap())
            .collect::<HashSet<_>>();
        assert_eq!(
            transformed,
            HashSet::from([(-6, -6), (-14, -6), (-14, -13), (-6, -13)])
        );
    }

    #[test]
    fn circular_profile_binds_by_unique_diameter_signature() {
        let sketch_id = SketchId("circle-profile".into());
        let entity_id = SketchEntityId("circle".into());
        let feature = |id: &str, name: &str, sketch| Feature {
            id: FeatureId(id.into()),
            ordinal: 0,
            name: Some(name.into()),
            suppressed: false,
            parent: None,
            dependencies: Vec::new(),
            source_properties: BTreeMap::new(),
            source_tag: None,
            source_text: None,
            source_content: Vec::new(),
            outputs: Vec::new(),
            definition: FeatureDefinition::Sketch {
                space: SketchSpace::Planar,
                sketch,
            },
            native_ref: Some(format!("native-{id}")),
        };
        let mut features = vec![
            feature("first", "Sketch1", None),
            feature("second", "Sketch2", Some(sketch_id.clone())),
        ];
        let parameter = |id: &str, owner: &str, diameter: f64| DesignParameter {
            id: ParameterId(id.into()),
            owner: FeatureId(owner.into()),
            ordinal: 0,
            name: "D1".into(),
            expression: format!("<MOD-DIAM>{diameter}"),
            display: Some(DimensionDisplay::Diameter),
            value: Some(ParameterValue::Length(Length(diameter))),
            dependencies: Vec::new(),
            properties: BTreeMap::new(),
            pmi: None,
            native_ref: None,
        };
        let parameters = [
            parameter("first-diameter", "first", 4.0),
            parameter("second-diameter", "second", 5.0),
        ];
        let mut sketches = [Sketch {
            id: sketch_id.clone(),
            name: Some("Sketch2".into()),
            configuration: None,
            origin: Point3::new(0.0, 0.0, 0.0),
            normal: Vector3::new(0.0, 0.0, 1.0),
            u_axis: Vector3::new(1.0, 0.0, 0.0),
            profiles: vec![vec![cadmpeg_ir::sketches::SketchEntityUse {
                entity: entity_id.clone(),
                reversed: false,
            }]],
            native_ref: None,
        }];
        let entities = [SketchEntity {
            id: entity_id,
            sketch: sketch_id.clone(),
            construction: false,
            native_ref: None,
            geometry_ref: None,
            endpoint_refs: Vec::new(),
            geometry: SketchGeometry::Circle {
                center: Point2::new(0.0, 0.0),
                radius: Length(2.0),
            },
        }];

        bind_circular_profile_by_dimension(&mut features, &mut sketches, &entities, &parameters);

        assert!(matches!(
            &features[0].definition,
            FeatureDefinition::Sketch { sketch: Some(id), .. } if id == &sketch_id
        ));
        assert!(matches!(
            &features[1].definition,
            FeatureDefinition::Sketch { sketch: None, .. }
        ));
        assert_eq!(sketches[0].name.as_deref(), Some("Sketch1"));
    }
}

fn operand_kind_name(kind: FeatureInputOperandKind) -> String {
    match kind {
        FeatureInputOperandKind::D6 => "d6".into(),
        FeatureInputOperandKind::E1 => "e1".into(),
        FeatureInputOperandKind::Native(tag) => {
            let [first, second] = tag.to_le_bytes();
            format!("{first:02x}{second:02x}")
        }
    }
}

pub(crate) fn object_names(payload: &[u8], parent: &str) -> Vec<FeatureInputName> {
    let lane_key = parent.rsplit_once('#').map_or(parent, |(_, key)| key);
    let mut name_marker = NAME_MARKER.to_vec();
    if let Some(token) = name_class_token(payload) {
        name_marker[..2].copy_from_slice(&token.to_le_bytes());
    }
    payload
        .windows(name_marker.len())
        .enumerate()
        .filter_map(|(offset, marker)| (marker == name_marker).then_some(offset))
        .filter_map(|offset| {
            let length = usize::from(*payload.get(offset + NAME_MARKER.len())?);
            if !(1..=128).contains(&length) {
                return None;
            }
            let start = offset + NAME_MARKER.len() + 1;
            let end = start.checked_add(length.checked_mul(2)?)?;
            let units = payload
                .get(start..end)?
                .chunks_exact(2)
                .map(|bytes| u16::from_le_bytes([bytes[0], bytes[1]]))
                .collect::<Vec<_>>();
            let value = String::from_utf16(&units).ok()?;
            let object_id = end.checked_add(8).and_then(|offset| {
                Some(u32::from_le_bytes(
                    payload.get(offset..offset + 4)?.try_into().ok()?,
                ))
            });
            (!value.chars().any(char::is_control)).then_some((offset, object_id, value))
        })
        .enumerate()
        .map(|(ordinal, (offset, object_id, value))| FeatureInputName {
            id: format!("sldprt:feature-input:name#{lane_key}:{offset}"),
            parent: parent.to_string(),
            ordinal: ordinal as u32,
            offset: offset as u64,
            object_id,
            value,
        })
        .collect()
}

/// Lane-scoped repeated-class token carried by every feature-name record.
///
/// The token is established by the first name record in the lane: the first
/// class declaration directly followed by a repeated-class token and the
/// UTF-16 name prefix `ff fe ff`.
fn name_class_token(payload: &[u8]) -> Option<u16> {
    payload
        .windows(CLASS_MARKER.len())
        .enumerate()
        .filter(|(_, window)| *window == CLASS_MARKER)
        .find_map(|(offset, _)| {
            let length = usize::from(u16::from_le_bytes(
                payload.get(offset + 4..offset + 6)?.try_into().ok()?,
            ));
            if !(1..=128).contains(&length) {
                return None;
            }
            let name = payload.get(offset + 6..offset + 6 + length)?;
            if !name.iter().all(u8::is_ascii_graphic) {
                return None;
            }
            let token_offset = offset + 6 + length;
            let token = u16::from_le_bytes(
                payload
                    .get(token_offset..token_offset + 2)?
                    .try_into()
                    .ok()?,
            );
            if token & 0x8000 == 0 || token == 0xffff {
                return None;
            }
            if payload.get(token_offset + 2..token_offset + 5) != Some(&[0xff, 0xfe, 0xff]) {
                return None;
            }
            let units = usize::from(*payload.get(token_offset + 5)?);
            (1..=128).contains(&units).then_some(token)
        })
}

pub(crate) fn class_declarations(payload: &[u8], parent: &str) -> Vec<FeatureInputClass> {
    let lane_key = parent.rsplit_once('#').map_or(parent, |(_, key)| key);
    payload
        .windows(CLASS_MARKER.len())
        .enumerate()
        .filter_map(|(offset, marker)| (marker == CLASS_MARKER).then_some(offset))
        .filter_map(|offset| {
            let length = usize::from(u16::from_le_bytes(
                payload.get(offset + 4..offset + 6)?.try_into().ok()?,
            ));
            if !(1..=128).contains(&length) {
                return None;
            }
            let bytes = payload.get(offset + 6..offset + 6 + length)?;
            if !bytes
                .iter()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
            {
                return None;
            }
            Some((offset, std::str::from_utf8(bytes).ok()?.to_string()))
        })
        .enumerate()
        .map(|(ordinal, (offset, name))| {
            let role = class_role(&name);
            FeatureInputClass {
                id: format!("sldprt:feature-input:class#{lane_key}:{offset}"),
                parent: parent.to_string(),
                ordinal: ordinal as u32,
                offset: offset as u64,
                name,
                role,
            }
        })
        .collect()
}

fn class_role(name: &str) -> FeatureInputClassRole {
    native_object_class(name).role
}

fn configuration(section: &str) -> Option<String> {
    let start = section.find("Config-")? + "Config-".len();
    let tail = &section[start..];
    let end = tail
        .find("-ResolvedFeatures")
        .or_else(|| tail.find('/'))
        .unwrap_or(tail.len());
    (!tail[..end].is_empty()).then(|| tail[..end].to_string())
}

/// Decode nested feature-input Parasolid streams as placed planar sketches.
pub fn sketches(
    scan: &ContainerScan,
    annotations: &mut Annotations,
) -> (Vec<Sketch>, Vec<SketchEntity>, Vec<SketchConstraint>) {
    let mut sketches = Vec::new();
    let mut entities = Vec::new();
    let mut constraints = Vec::new();
    for block in &scan.blocks {
        let Some(section) = block.section.as_deref() else {
            continue;
        };
        if !section.to_ascii_lowercase().contains("resolvedfeatures") {
            continue;
        }
        let native_ref = format!("sldprt:feature-input:resolved-features#{}", block.offset);
        for (stream_ordinal, payload) in block.ps_streams.iter().enumerate() {
            let stream_offset = block.ps_stream_offsets[stream_ordinal];
            let Some(header) = crate::parasolid::stream_header(payload) else {
                continue;
            };
            let brep = crate::brep::decode(payload, &header, section);
            project_brep(
                &brep,
                block.offset,
                stream_ordinal,
                stream_offset,
                section,
                &header.description,
                configuration(section).as_deref(),
                &native_ref,
                annotations,
                &mut sketches,
                &mut entities,
                &mut constraints,
            );
        }
    }
    (sketches, entities, constraints)
}

#[allow(clippy::too_many_arguments)]
fn project_brep(
    brep: &crate::brep::Brep,
    block_offset: usize,
    stream_ordinal: usize,
    stream_offset: usize,
    section: &str,
    sketch_name: &str,
    configuration: Option<&str>,
    native_ref: &str,
    annotations: &mut Annotations,
    sketches: &mut Vec<Sketch>,
    entities: &mut Vec<SketchEntity>,
    constraints: &mut Vec<SketchConstraint>,
) {
    let surfaces = brep
        .surfaces
        .iter()
        .map(|surface| (&surface.id, &surface.geometry))
        .collect::<HashMap<_, _>>();
    let loops = brep
        .loops
        .iter()
        .map(|loop_| (&loop_.id, loop_))
        .collect::<HashMap<_, _>>();
    let coedges = brep
        .coedges
        .iter()
        .map(|coedge| (&coedge.id, coedge))
        .collect::<HashMap<_, _>>();
    let edges = brep
        .edges
        .iter()
        .map(|edge| (&edge.id, edge))
        .collect::<HashMap<_, _>>();
    let vertices = brep
        .vertices
        .iter()
        .map(|vertex| (&vertex.id, &vertex.point))
        .collect::<HashMap<_, _>>();
    let points = brep
        .points
        .iter()
        .map(|point| (&point.id, point.position))
        .collect::<HashMap<_, _>>();
    let curves = brep
        .curves
        .iter()
        .map(|curve| (&curve.id, &curve.geometry))
        .collect::<HashMap<_, _>>();

    for (face_ordinal, face) in brep.faces.iter().enumerate() {
        let Some(SurfaceGeometry::Plane {
            origin,
            normal,
            u_axis,
        }) = surfaces.get(&face.surface).copied()
        else {
            continue;
        };
        let sketch_id = SketchId(format!(
            "sldprt:model:sketch#{block_offset}:{stream_ordinal}:{face_ordinal}"
        ));
        let v_axis = cross(*normal, *u_axis);
        let first_entity = entities.len();
        let mut edge_entities = HashMap::<&cadmpeg_ir::ids::EdgeId, SketchEntityId>::new();
        let mut used_vertices = HashSet::new();
        let mut profiles = Vec::new();
        for loop_id in &face.loops {
            let Some(loop_) = loops.get(loop_id) else {
                continue;
            };
            let mut profile = Vec::new();
            for coedge_id in &loop_.coedges {
                let Some(coedge) = coedges.get(coedge_id) else {
                    continue;
                };
                let Some(edge) = edges.get(&coedge.edge) else {
                    continue;
                };
                used_vertices.insert(edge.start.clone());
                used_vertices.insert(edge.end.clone());
                let entity_id = if let Some(id) = edge_entities.get(&edge.id) {
                    id.clone()
                } else {
                    let id = SketchEntityId(format!(
                        "sldprt:model:sketch-entity#{block_offset}:{stream_ordinal}:{face_ordinal}:{}",
                        edge_entities.len()
                    ));
                    let Some(geometry) =
                        project_edge(edge, &vertices, &points, &curves, *origin, *u_axis, v_axis)
                    else {
                        continue;
                    };
                    let Some(start_point) = vertices.get(&edge.start) else {
                        continue;
                    };
                    let Some(end_point) = vertices.get(&edge.end) else {
                        continue;
                    };
                    crate::annotations::note(
                        annotations,
                        id.0.clone(),
                        section,
                        0,
                        "feature_input_profile_edge",
                        Exactness::Derived,
                    );
                    entities.push(SketchEntity {
                        id: id.clone(),
                        sketch: sketch_id.clone(),
                        construction: false,
                        native_ref: Some(format!("{stream_ordinal}:{}", edge.id.0)),
                        geometry_ref: edge
                            .curve
                            .as_ref()
                            .map(|id| format!("{stream_ordinal}:{}", id.0)),
                        endpoint_refs: vec![
                            format!("{stream_ordinal}:{}", start_point.0),
                            format!("{stream_ordinal}:{}", end_point.0),
                        ],
                        geometry,
                    });
                    edge_entities.insert(&edge.id, id.clone());
                    id
                };
                if edge.curve.is_some() || edge.start != edge.end {
                    profile.push(SketchEntityUse {
                        entity: entity_id,
                        reversed: coedge.sense == Sense::Reversed,
                    });
                }
            }
            if !profile.is_empty() {
                profiles.push(profile);
            }
        }
        for vertex in &brep.vertices {
            if used_vertices.contains(&vertex.id) {
                continue;
            }
            let Some(position) = points.get(&vertex.point) else {
                continue;
            };
            let id = SketchEntityId(format!(
                "sldprt:model:sketch-entity#{block_offset}:{stream_ordinal}:{face_ordinal}:{}",
                edge_entities.len()
                    + entities
                        .iter()
                        .filter(|entity| entity.sketch == sketch_id)
                        .count()
            ));
            crate::annotations::note(
                annotations,
                id.0.clone(),
                section,
                0,
                "feature_input_profile_point",
                Exactness::Derived,
            );
            entities.push(SketchEntity {
                id,
                sketch: sketch_id.clone(),
                construction: false,
                native_ref: Some(format!("{stream_ordinal}:{}", vertex.id.0)),
                geometry_ref: None,
                endpoint_refs: vec![format!("{stream_ordinal}:{}", vertex.point.0)],
                geometry: SketchGeometry::Point {
                    position: project_point(*position, *origin, *u_axis, v_axis),
                },
            });
        }
        if profiles.is_empty() && !entities.iter().any(|entity| entity.sketch == sketch_id) {
            continue;
        }
        crate::annotations::note(
            annotations,
            sketch_id.0.clone(),
            section,
            stream_offset as u64,
            "feature_input_profile",
            Exactness::Derived,
        );
        project_endpoint_constraints(
            &sketch_id,
            &entities[first_entity..],
            block_offset,
            stream_ordinal,
            face_ordinal,
            section,
            annotations,
            constraints,
        );
        sketches.push(Sketch {
            id: sketch_id,
            name: (!sketch_name.is_empty()).then(|| sketch_name.to_string()),
            configuration: configuration.map(str::to_string),
            origin: *origin,
            normal: *normal,
            u_axis: *u_axis,
            profiles,
            native_ref: Some(native_ref.to_string()),
        });
    }
}

#[allow(clippy::too_many_arguments)]
fn project_endpoint_constraints(
    sketch: &SketchId,
    entities: &[SketchEntity],
    block_offset: usize,
    stream_ordinal: usize,
    face_ordinal: usize,
    section: &str,
    annotations: &mut Annotations,
    constraints: &mut Vec<SketchConstraint>,
) {
    let mut loci_by_endpoint = BTreeMap::<&str, Vec<SketchLocus>>::new();
    for entity in entities {
        if entity.endpoint_refs.len() != 2 {
            continue;
        }
        for (index, endpoint) in entity.endpoint_refs.iter().enumerate() {
            let locus = if index == 0 {
                SketchLocus::Start(entity.id.clone())
            } else {
                SketchLocus::End(entity.id.clone())
            };
            loci_by_endpoint.entry(endpoint).or_default().push(locus);
        }
    }
    for (_endpoint, loci) in loci_by_endpoint {
        let distinct_entities = loci
            .iter()
            .map(|locus| match locus {
                SketchLocus::Start(entity)
                | SketchLocus::End(entity)
                | SketchLocus::Center(entity)
                | SketchLocus::Entity(entity) => entity,
            })
            .collect::<HashSet<_>>();
        if distinct_entities.len() < 2 {
            continue;
        }
        let id = SketchConstraintId(format!(
            "sldprt:model:sketch-constraint#{block_offset}:{stream_ordinal}:{face_ordinal}:{}",
            constraints.len()
        ));
        crate::annotations::note(
            annotations,
            id.0.clone(),
            section,
            0,
            "feature_input_shared_endpoint",
            Exactness::Derived,
        );
        constraints.push(SketchConstraint {
            id,
            sketch: sketch.clone(),
            definition: SketchConstraintDefinition::CoincidentLoci { loci },
            native_ref: None,
        });
    }
}

fn project_edge(
    edge: &cadmpeg_ir::topology::Edge,
    vertices: &HashMap<&cadmpeg_ir::ids::VertexId, &cadmpeg_ir::ids::PointId>,
    points: &HashMap<&cadmpeg_ir::ids::PointId, Point3>,
    curves: &HashMap<&cadmpeg_ir::ids::CurveId, &CurveGeometry>,
    origin: Point3,
    u_axis: Vector3,
    v_axis: Vector3,
) -> Option<SketchGeometry> {
    let start = project_point(
        *points.get(vertices.get(&edge.start)?)?,
        origin,
        u_axis,
        v_axis,
    );
    let end = project_point(
        *points.get(vertices.get(&edge.end)?)?,
        origin,
        u_axis,
        v_axis,
    );
    match edge.curve.as_ref().and_then(|id| curves.get(id).copied()) {
        Some(CurveGeometry::Circle { center, radius, .. }) => {
            let center = project_point(*center, origin, u_axis, v_axis);
            if (start.u - end.u).hypot(start.v - end.v) <= 1.0e-9 {
                Some(SketchGeometry::Circle {
                    center,
                    radius: cadmpeg_ir::features::Length(*radius),
                })
            } else {
                let parameters = edge
                    .param_range
                    .filter(|[start, end]| start.is_finite() && end.is_finite() && start != end);
                Some(SketchGeometry::Arc {
                    center,
                    radius: cadmpeg_ir::features::Length(*radius),
                    start_angle: cadmpeg_ir::features::Angle(parameters.map_or_else(
                        || (start.v - center.v).atan2(start.u - center.u),
                        |range| range[0],
                    )),
                    end_angle: cadmpeg_ir::features::Angle(parameters.map_or_else(
                        || (end.v - center.v).atan2(end.u - center.u),
                        |range| range[1],
                    )),
                })
            }
        }
        Some(CurveGeometry::Ellipse {
            center,
            major_direction,
            major_radius,
            minor_radius,
            ..
        }) => {
            let center = project_point(*center, origin, u_axis, v_axis);
            let major_u = dot(*major_direction, u_axis);
            let major_v = dot(*major_direction, v_axis);
            let major_angle = major_v.atan2(major_u);
            let full = (start.u - end.u).hypot(start.v - end.v) <= 1.0e-9;
            let parameter = |point: Point2| {
                let du = point.u - center.u;
                let dv = point.v - center.v;
                let major_component = du * major_angle.cos() + dv * major_angle.sin();
                let minor_component = -du * major_angle.sin() + dv * major_angle.cos();
                (minor_component / *minor_radius).atan2(major_component / *major_radius)
            };
            let parameters = edge
                .param_range
                .filter(|[start, end]| start.is_finite() && end.is_finite() && start != end);
            Some(SketchGeometry::Ellipse {
                center,
                major_angle: cadmpeg_ir::features::Angle(major_angle),
                major_radius: cadmpeg_ir::features::Length(*major_radius),
                minor_radius: cadmpeg_ir::features::Length(*minor_radius),
                start_angle: (!full).then(|| {
                    cadmpeg_ir::features::Angle(
                        parameters.map_or_else(|| parameter(start), |range| range[0]),
                    )
                }),
                end_angle: (!full).then(|| {
                    cadmpeg_ir::features::Angle(
                        parameters.map_or_else(|| parameter(end), |range| range[1]),
                    )
                }),
            })
        }
        Some(CurveGeometry::Nurbs(nurbs)) => Some(SketchGeometry::Nurbs {
            degree: nurbs.degree,
            knots: nurbs.knots.clone(),
            control_points: nurbs
                .control_points
                .iter()
                .map(|point| project_point(*point, origin, u_axis, v_axis))
                .collect(),
            weights: nurbs.weights.clone(),
            periodic: nurbs.periodic,
        }),
        None if edge.start == edge.end => Some(SketchGeometry::Point { position: start }),
        Some(CurveGeometry::Line { .. }) | None => Some(SketchGeometry::Line { start, end }),
        Some(other) => Some(SketchGeometry::Native {
            native_kind: format!("{other:?}"),
        }),
    }
}

fn project_point(point: Point3, origin: Point3, u_axis: Vector3, v_axis: Vector3) -> Point2 {
    let delta = Vector3::new(point.x - origin.x, point.y - origin.y, point.z - origin.z);
    Point2::new(dot(delta, u_axis), dot(delta, v_axis))
}

fn dot(left: Vector3, right: Vector3) -> f64 {
    left.x * right.x + left.y * right.y + left.z * right.z
}

fn cross(left: Vector3, right: Vector3) -> Vector3 {
    Vector3::new(
        left.y * right.z - left.z * right.y,
        left.z * right.x - left.x * right.z,
        left.x * right.y - left.y * right.x,
    )
}

/// Stable hash of neutral sketch records.
pub fn sketch_hash(ir: &cadmpeg_ir::CadIr) -> String {
    hash_debug(&(
        &ir.model.sketches,
        &ir.model.sketch_entities,
        &ir.model.sketch_constraints,
        &ir.model.spatial_sketches,
        &ir.model.spatial_sketch_entities,
    ))
}

/// Stable hash of neutral sketch constraints.
pub fn constraint_hash(ir: &cadmpeg_ir::CadIr) -> String {
    hash_debug(&ir.model.sketch_constraints)
}

/// Stable hash of retained native feature-input lanes.
pub fn lane_hash(native: &crate::native::SldprtNative) -> String {
    hash_debug(&native.feature_input_lanes)
}

fn hash_debug<T: std::fmt::Debug + ?Sized>(value: &T) -> String {
    let bytes = format!("{value:?}");
    let mut out = String::with_capacity(64);
    for byte in Sha256::digest(bytes.as_bytes()) {
        write!(&mut out, "{byte:02x}").expect("writing to String cannot fail");
    }
    out
}

/// Reject unsupported neutral sketch edits before native lane replay.
pub fn prepare_sketches_for_write(
    ir: &cadmpeg_ir::CadIr,
    native: &mut Option<crate::native::SldprtNative>,
) -> Result<(), cadmpeg_ir::codec::CodecError> {
    let baseline_neutral = ir
        .source
        .as_ref()
        .and_then(|source| source.attributes.get("sldprt_neutral_sketch_sha256"));
    let baseline_native = ir
        .source
        .as_ref()
        .and_then(|source| source.attributes.get("sldprt_native_sketch_sha256"));
    let baseline_constraints = ir.source.as_ref().and_then(|source| {
        source
            .attributes
            .get("sldprt_neutral_sketch_constraint_sha256")
    });
    let current_neutral = sketch_hash(ir);
    let current_native = native.as_ref().map(lane_hash);
    if baseline_neutral.is_none() && baseline_native.is_none() {
        if ir.model.sketches.is_empty()
            && ir.model.sketch_entities.is_empty()
            && ir.model.sketch_constraints.is_empty()
            && ir.model.spatial_sketches.is_empty()
            && ir.model.spatial_sketch_entities.is_empty()
        {
            return Ok(());
        }
        validate_source_less_constraints(ir)?;
        let native = native.get_or_insert_with(crate::native::SldprtNative::default);
        let generated = source_less_lanes(ir, native)?;
        native.feature_input_lanes.extend(generated);
        return Ok(());
    }
    let neutral_changed = baseline_neutral.is_none_or(|hash| hash != &current_neutral);
    if !neutral_changed {
        return Ok(());
    }
    let current_constraints = constraint_hash(ir);
    if baseline_constraints.is_none_or(|hash| hash != &current_constraints) {
        return Err(cadmpeg_ir::codec::CodecError::NotImplemented(
            "SLDPRT native sketch relation editing is not implemented".into(),
        ));
    }
    let native_changed = match (&current_native, baseline_native) {
        (Some(current), Some(baseline)) => current != baseline,
        (Some(_), None) | (None, Some(_)) => true,
        (None, None) => false,
    };
    if native_changed {
        return Err(cadmpeg_ir::codec::CodecError::Malformed(
            "conflicting neutral and native SLDPRT sketch edits".into(),
        ));
    }
    let retained = native.as_mut().ok_or_else(|| {
        cadmpeg_ir::codec::CodecError::NotImplemented(
            "SLDPRT sketch write-back requires retained feature-input lanes".into(),
        )
    })?;
    patch_spatial_line_sketches(ir, retained)?;
    patch_line_profiles(ir, retained)
}

fn patch_spatial_line_sketches(
    ir: &cadmpeg_ir::CadIr,
    native: &mut crate::native::SldprtNative,
) -> Result<(), cadmpeg_ir::codec::CodecError> {
    for sketch in &ir.model.spatial_sketches {
        let owners = ir
            .model
            .features
            .iter()
            .filter(|feature| {
                matches!(
                    &feature.definition,
                    FeatureDefinition::SpatialSketch {
                        sketch: Some(candidate),
                    } if candidate == &sketch.id
                )
            })
            .collect::<Vec<_>>();
        let [owner] = owners.as_slice() else {
            return Err(cadmpeg_ir::codec::CodecError::Malformed(format!(
                "SLDPRT spatial sketch {} requires one owning feature",
                sketch.id.0
            )));
        };
        let native_ref = owner.native_ref.as_deref().ok_or_else(|| {
            cadmpeg_ir::codec::CodecError::NotImplemented(format!(
                "SLDPRT spatial sketch {} requires a retained feature object",
                sketch.id.0
            ))
        })?;
        let record = native
            .feature_histories
            .iter()
            .flat_map(|history| &history.features)
            .find(|record| record.id == native_ref)
            .ok_or_else(|| {
                cadmpeg_ir::codec::CodecError::Malformed(format!(
                    "SLDPRT spatial sketch {} references missing feature object {native_ref}",
                    sketch.id.0
                ))
            })?;
        let [entity_id] = sketch.entities.as_slice() else {
            return Err(cadmpeg_ir::codec::CodecError::NotImplemented(format!(
                "SLDPRT spatial sketch {} requires exactly one retained line",
                sketch.id.0
            )));
        };
        let entity = ir
            .model
            .spatial_sketch_entities
            .iter()
            .find(|entity| entity.id == *entity_id && entity.sketch == sketch.id)
            .ok_or_else(|| {
                cadmpeg_ir::codec::CodecError::Malformed(format!(
                    "SLDPRT spatial sketch {} references missing entity {}",
                    sketch.id.0, entity_id.0
                ))
            })?;
        let SpatialSketchGeometry::Line { start, end } = entity.geometry else {
            return Err(cadmpeg_ir::codec::CodecError::NotImplemented(format!(
                "SLDPRT spatial sketch {} supports retained line geometry only",
                sketch.id.0
            )));
        };
        if start == end {
            return Err(cadmpeg_ir::codec::CodecError::Malformed(format!(
                "SLDPRT spatial sketch {} has a zero-length line",
                sketch.id.0
            )));
        }
        let candidates = native
            .feature_input_lanes
            .iter()
            .enumerate()
            .filter(|(_, lane)| sketch.native_ref.as_deref().is_none_or(|id| id == lane.id))
            .filter_map(|(lane_index, lane)| {
                let name = feature_object_name(record, lane)?;
                let object_start = usize::try_from(name.offset).ok()?;
                let object_end = native
                    .feature_histories
                    .iter()
                    .flat_map(|history| &history.features)
                    .filter_map(|candidate| feature_object_name(candidate, lane))
                    .filter(|candidate| candidate.offset > name.offset)
                    .map(|candidate| candidate.offset)
                    .min()
                    .and_then(|offset| usize::try_from(offset).ok())
                    .unwrap_or(lane.native_payload.len());
                let offsets =
                    spatial_vertex_offsets(lane.native_payload.get(object_start..object_end)?);
                let [first, second] = offsets.as_slice() else {
                    return None;
                };
                Some((lane_index, object_start + first, object_start + second))
            })
            .collect::<Vec<_>>();
        let [(lane_index, first, second)] = candidates.as_slice() else {
            return Err(cadmpeg_ir::codec::CodecError::NotImplemented(format!(
                "SLDPRT spatial sketch {} does not resolve to one two-vertex feature object",
                sketch.id.0
            )));
        };
        let payload = &mut native.feature_input_lanes[*lane_index].native_payload;
        patch_spatial_vertex(payload, *first, start)?;
        patch_spatial_vertex(payload, *second, end)?;
    }

    let mut features = crate::history::project_features(&native.feature_histories);
    let (projected_sketches, projected_entities) = spatial_sketches(
        &mut features,
        &native.feature_histories,
        &native.feature_input_lanes,
    );
    if ir.model.spatial_sketches != projected_sketches
        || ir.model.spatial_sketch_entities != projected_entities
    {
        return Err(cadmpeg_ir::codec::CodecError::NotImplemented(
            "SLDPRT spatial sketch edit has no complete native lane encoding".into(),
        ));
    }
    Ok(())
}

fn patch_spatial_vertex(
    payload: &mut [u8],
    offset: usize,
    point: Point3,
) -> Result<(), cadmpeg_ir::codec::CodecError> {
    let bytes = payload.get_mut(offset..offset + 69).ok_or_else(|| {
        cadmpeg_ir::codec::CodecError::Malformed(
            "SLDPRT spatial vertex record lies outside its feature-input lane".into(),
        )
    })?;
    if bytes.get(..SPATIAL_VERTEX_PREFIX.len()) != Some(SPATIAL_VERTEX_PREFIX)
        || bytes.get(43..45) != Some(&[0x0e, 0x00])
    {
        return Err(cadmpeg_ir::codec::CodecError::Malformed(
            "SLDPRT spatial vertex record changed shape".into(),
        ));
    }
    bytes[45..53].copy_from_slice(&point.x.to_le_bytes());
    bytes[53..61].copy_from_slice(&point.y.to_le_bytes());
    bytes[61..69].copy_from_slice(&point.z.to_le_bytes());
    Ok(())
}

fn validate_source_less_constraints(
    ir: &cadmpeg_ir::CadIr,
) -> Result<(), cadmpeg_ir::codec::CodecError> {
    for constraint in &ir.model.sketch_constraints {
        let SketchConstraintDefinition::CoincidentLoci { loci } = &constraint.definition else {
            validate_generated_marker_constraint(ir, constraint)?;
            continue;
        };
        if loci.len() < 2 {
            return Err(cadmpeg_ir::codec::CodecError::Malformed(format!(
                "sketch constraint {} requires at least two loci",
                constraint.id.0
            )));
        }
        let mut expected = None;
        for locus in loci {
            let point = constraint_locus_point(ir, constraint, locus)?;
            if expected.is_some_and(|expected| !same_sketch_point(expected, point)) {
                return Err(cadmpeg_ir::codec::CodecError::Malformed(format!(
                    "source-less SLDPRT sketch constraint {} has noncoincident locus coordinates",
                    constraint.id.0
                )));
            }
            expected = Some(point);
        }
    }
    Ok(())
}

fn validate_generated_marker_constraint(
    ir: &cadmpeg_ir::CadIr,
    constraint: &SketchConstraint,
) -> Result<(), cadmpeg_ir::codec::CodecError> {
    if !ir.model.features.iter().any(|feature| {
        matches!(
            &feature.definition,
            FeatureDefinition::Sketch {
                sketch: Some(sketch),
                ..
            } if sketch == &constraint.sketch
        )
    }) {
        return Err(cadmpeg_ir::codec::CodecError::NotImplemented(format!(
            "source-less SLDPRT marker relation {} requires an owning sketch feature",
            constraint.id.0
        )));
    }
    match &constraint.definition {
        SketchConstraintDefinition::HorizontalPoints { first, second }
        | SketchConstraintDefinition::VerticalPoints { first, second } => {
            let first_point = constraint_locus_point(ir, constraint, first)?;
            let second_point = constraint_locus_point(ir, constraint, second)?;
            let delta = if matches!(
                &constraint.definition,
                SketchConstraintDefinition::HorizontalPoints { .. }
            ) {
                (first_point.v - second_point.v).abs()
            } else {
                (first_point.u - second_point.u).abs()
            };
            if delta > SKETCH_POINT_TOLERANCE {
                return Err(cadmpeg_ir::codec::CodecError::Malformed(format!(
                    "source-less SLDPRT sketch constraint {} is not satisfied by its locus coordinates",
                    constraint.id.0
                )));
            }
            return Ok(());
        }
        SketchConstraintDefinition::Midpoint { point, entity } => {
            let point = constraint_locus_point(ir, constraint, point)?;
            let entity = sketch_constraint_entity(ir, constraint, entity)?;
            let (start, end) = sketch_line(&entity.geometry).ok_or_else(|| {
                cadmpeg_ir::codec::CodecError::NotImplemented(format!(
                    "source-less SLDPRT midpoint constraint {} requires a line entity",
                    constraint.id.0
                ))
            })?;
            if !same_point2(
                point,
                Point2::new((start.u + end.u) * 0.5, (start.v + end.v) * 0.5),
            ) {
                return Err(cadmpeg_ir::codec::CodecError::Malformed(format!(
                    "source-less SLDPRT sketch constraint {} is not satisfied by its midpoint coordinates",
                    constraint.id.0
                )));
            }
            return Ok(());
        }
        _ => {}
    }
    let dimension_parameter = match &constraint.definition {
        SketchConstraintDefinition::Distance { parameter, .. }
        | SketchConstraintDefinition::DistanceLoci { parameter, .. }
        | SketchConstraintDefinition::HorizontalDistance { parameter, .. }
        | SketchConstraintDefinition::VerticalDistance { parameter, .. }
        | SketchConstraintDefinition::Angle { parameter, .. }
        | SketchConstraintDefinition::Radius { parameter, .. }
        | SketchConstraintDefinition::Diameter { parameter, .. } => Some(parameter),
        _ => None,
    };
    if let Some(parameter_id) = dimension_parameter {
        let parameter = ir
            .model
            .parameters
            .iter()
            .find(|parameter| parameter.id == *parameter_id)
            .ok_or_else(|| {
                cadmpeg_ir::codec::CodecError::Malformed(format!(
                    "source-less SLDPRT dimension {} references missing parameter {}",
                    constraint.id.0, parameter_id.0
                ))
            })?;
        let compatible = match &constraint.definition {
            SketchConstraintDefinition::Angle { .. } => {
                matches!(
                    parameter.value,
                    Some(cadmpeg_ir::features::ParameterValue::Angle(_))
                )
            }
            _ => matches!(
                parameter.value,
                Some(cadmpeg_ir::features::ParameterValue::Length(_))
            ),
        };
        if !compatible {
            return Err(cadmpeg_ir::codec::CodecError::Malformed(format!(
                "source-less SLDPRT dimension parameter {} has no compatible evaluated value",
                parameter.id.0
            )));
        }
        let expected_display = match &constraint.definition {
            SketchConstraintDefinition::Radius { .. } => {
                Some(cadmpeg_ir::features::DimensionDisplay::Radius)
            }
            SketchConstraintDefinition::Diameter { .. } => {
                Some(cadmpeg_ir::features::DimensionDisplay::Diameter)
            }
            _ => None,
        };
        if parameter.display != expected_display {
            return Err(cadmpeg_ir::codec::CodecError::Malformed(format!(
                "source-less SLDPRT dimension parameter {} has incompatible display semantics",
                parameter.id.0
            )));
        }
        let owner = ir.model.features.iter().find(|feature| {
            matches!(
                &feature.definition,
                FeatureDefinition::Sketch { sketch: Some(sketch), .. }
                    if sketch == &constraint.sketch
            )
        });
        if owner.is_none_or(|owner| owner.id != parameter.owner) {
            return Err(cadmpeg_ir::codec::CodecError::Malformed(format!(
                "source-less SLDPRT dimension parameter {} is not owned by its sketch feature",
                parameter.id.0
            )));
        }
        validate_solved_dimension(ir, constraint, parameter)?;
        return Ok(());
    }
    if let Some((kind, first, second)) = binary_marker_relation(&constraint.definition) {
        let first = sketch_constraint_entity(ir, constraint, first)?;
        let second = sketch_constraint_entity(ir, constraint, second)?;
        validate_solved_binary_relation(constraint, kind, first, second)?;
        return Ok(());
    }
    let (entity_id, axis) = match &constraint.definition {
        SketchConstraintDefinition::Horizontal { entity } => (entity, Some(false)),
        SketchConstraintDefinition::Vertical { entity } => (entity, Some(true)),
        SketchConstraintDefinition::Fixed { entity } => (entity, None),
        SketchConstraintDefinition::ArcAngle { entity, angle } => {
            if arc_angle_relation_kind(angle.0).is_none() {
                return Err(cadmpeg_ir::codec::CodecError::NotImplemented(format!(
                    "source-less SLDPRT arc-angle constraint {} is not 90, 180, or 270 degrees",
                    constraint.id.0
                )));
            }
            (entity, None)
        }
        SketchConstraintDefinition::EllipseAngle { entity, angle } => {
            if ellipse_angle_relation_kind(angle.0).is_none() {
                return Err(cadmpeg_ir::codec::CodecError::NotImplemented(format!(
                    "source-less SLDPRT ellipse-angle constraint {} is not 90, 180, or 270 degrees",
                    constraint.id.0
                )));
            }
            (entity, None)
        }
        _ => {
            return Err(cadmpeg_ir::codec::CodecError::NotImplemented(
                "source-less SLDPRT sketch constraints support solved endpoint coincidences and horizontal, vertical, or fixed marker relations"
                    .into(),
            ));
        }
    };
    let entity = ir
        .model
        .sketch_entities
        .iter()
        .find(|entity| entity.id == *entity_id && entity.sketch == constraint.sketch)
        .ok_or_else(|| {
            cadmpeg_ir::codec::CodecError::Malformed(format!(
                "sketch constraint {} references entity {} outside sketch {}",
                constraint.id.0, entity_id.0, constraint.sketch.0
            ))
        })?;
    if matches!(
        &constraint.definition,
        SketchConstraintDefinition::ArcAngle { .. }
    ) && !matches!(&entity.geometry, SketchGeometry::Arc { .. })
    {
        return Err(cadmpeg_ir::codec::CodecError::Malformed(format!(
            "sketch constraint {} applies an arc-angle relation to a non-arc entity",
            constraint.id.0
        )));
    }
    if matches!(
        &constraint.definition,
        SketchConstraintDefinition::EllipseAngle { .. }
    ) && !matches!(&entity.geometry, SketchGeometry::Ellipse { .. })
    {
        return Err(cadmpeg_ir::codec::CodecError::Malformed(format!(
            "sketch constraint {} applies an ellipse-angle relation to a non-ellipse entity",
            constraint.id.0
        )));
    }
    let Some(axis) = axis else {
        return Ok(());
    };
    let SketchGeometry::Line { start, end } = entity.geometry else {
        return Err(cadmpeg_ir::codec::CodecError::Malformed(format!(
            "sketch constraint {} applies an axis relation to a non-line entity",
            constraint.id.0
        )));
    };
    let delta = if axis {
        (end.u - start.u).abs()
    } else {
        (end.v - start.v).abs()
    };
    if delta > SKETCH_POINT_TOLERANCE {
        return Err(cadmpeg_ir::codec::CodecError::Malformed(format!(
            "source-less SLDPRT sketch constraint {} is not satisfied by its line coordinates",
            constraint.id.0
        )));
    }
    Ok(())
}

fn validate_solved_dimension(
    ir: &cadmpeg_ir::CadIr,
    constraint: &SketchConstraint,
    parameter: &cadmpeg_ir::features::DesignParameter,
) -> Result<(), cadmpeg_ir::codec::CodecError> {
    let expected = match parameter.value {
        Some(cadmpeg_ir::features::ParameterValue::Length(value)) => value.0,
        Some(cadmpeg_ir::features::ParameterValue::Angle(value)) => value.0,
        _ => unreachable!("dimension parameter compatibility was checked by the caller"),
    };
    let mut measured = match &constraint.definition {
        SketchConstraintDefinition::DistanceLoci { first, second, .. } => match (first, second) {
            (SketchLocus::Entity(first), SketchLocus::Entity(second))
                if !generated_locus_is_point(ir, first)
                    && !generated_locus_is_point(ir, second) =>
            {
                line_line_dimension(
                    constraint,
                    sketch_constraint_entity(ir, constraint, first)?,
                    sketch_constraint_entity(ir, constraint, second)?,
                )?
            }
            (SketchLocus::Entity(line), point) if !generated_locus_is_point(ir, line) => {
                point_line_dimension(
                    constraint_locus_point(ir, constraint, point)?,
                    sketch_constraint_entity(ir, constraint, line)?,
                    constraint,
                )?
            }
            (point, SketchLocus::Entity(line)) if !generated_locus_is_point(ir, line) => {
                point_line_dimension(
                    constraint_locus_point(ir, constraint, point)?,
                    sketch_constraint_entity(ir, constraint, line)?,
                    constraint,
                )?
            }
            _ => {
                let first = constraint_locus_point(ir, constraint, first)?;
                let second = constraint_locus_point(ir, constraint, second)?;
                vector2_length([second.u - first.u, second.v - first.v])
            }
        },
        SketchConstraintDefinition::Distance { entities, .. } => {
            let [first, second] = entities.as_slice() else {
                return Err(cadmpeg_ir::codec::CodecError::NotImplemented(format!(
                    "source-less SLDPRT distance dimension {} requires exactly two lines",
                    constraint.id.0
                )));
            };
            line_line_dimension(
                constraint,
                sketch_constraint_entity(ir, constraint, first)?,
                sketch_constraint_entity(ir, constraint, second)?,
            )?
        }
        SketchConstraintDefinition::HorizontalDistance { first, second, .. } => {
            let first = constraint_locus_point(ir, constraint, first)?;
            let second = constraint_locus_point(ir, constraint, second)?;
            (second.u - first.u).abs()
        }
        SketchConstraintDefinition::VerticalDistance { first, second, .. } => {
            let first = constraint_locus_point(ir, constraint, first)?;
            let second = constraint_locus_point(ir, constraint, second)?;
            (second.v - first.v).abs()
        }
        SketchConstraintDefinition::Angle { first, second, .. } => {
            let first = sketch_constraint_entity(ir, constraint, first)?;
            let second = sketch_constraint_entity(ir, constraint, second)?;
            let (first_start, first_end) = sketch_line(&first.geometry).ok_or_else(|| {
                cadmpeg_ir::codec::CodecError::NotImplemented(format!(
                    "source-less SLDPRT angular dimension {} requires two lines",
                    constraint.id.0
                ))
            })?;
            let (second_start, second_end) = sketch_line(&second.geometry).ok_or_else(|| {
                cadmpeg_ir::codec::CodecError::NotImplemented(format!(
                    "source-less SLDPRT angular dimension {} requires two lines",
                    constraint.id.0
                ))
            })?;
            let first = [first_end.u - first_start.u, first_end.v - first_start.v];
            let second = [second_end.u - second_start.u, second_end.v - second_start.v];
            let denominator = vector2_length(first) * vector2_length(second);
            if denominator <= SKETCH_POINT_TOLERANCE {
                return Err(cadmpeg_ir::codec::CodecError::Malformed(format!(
                    "source-less SLDPRT angular dimension {} has a degenerate line",
                    constraint.id.0
                )));
            }
            ((first[0] * second[0] + first[1] * second[1]) / denominator)
                .clamp(-1.0, 1.0)
                .acos()
        }
        SketchConstraintDefinition::Radius { entity, .. }
        | SketchConstraintDefinition::Diameter { entity, .. } => {
            let entity = sketch_constraint_entity(ir, constraint, entity)?;
            let radius = match &entity.geometry {
                SketchGeometry::Circle { radius, .. } | SketchGeometry::Arc { radius, .. } => {
                    radius.0
                }
                _ => {
                    return Err(cadmpeg_ir::codec::CodecError::NotImplemented(format!(
                        "source-less SLDPRT radial dimension {} requires circular geometry",
                        constraint.id.0
                    )))
                }
            };
            if matches!(
                constraint.definition,
                SketchConstraintDefinition::Diameter { .. }
            ) {
                radius * 2.0
            } else {
                radius
            }
        }
        _ => unreachable!("only dimension definitions are passed"),
    };
    if matches!(
        &constraint.definition,
        SketchConstraintDefinition::Angle { .. }
    ) {
        let supplement = std::f64::consts::PI - measured;
        if (supplement - expected).abs() < (measured - expected).abs() {
            measured = supplement;
        }
    }
    let tolerance = SKETCH_POINT_TOLERANCE * (1.0 + measured.abs().max(expected.abs()));
    if !measured.is_finite() || (measured - expected).abs() > tolerance {
        return Err(cadmpeg_ir::codec::CodecError::Malformed(format!(
            "source-less SLDPRT dimension {} value {} is not satisfied by measured geometry {}",
            constraint.id.0, expected, measured
        )));
    }
    Ok(())
}

fn point_line_dimension(
    point: Point2,
    line: &SketchEntity,
    constraint: &SketchConstraint,
) -> Result<f64, cadmpeg_ir::codec::CodecError> {
    let (start, end) = sketch_line(&line.geometry).ok_or_else(|| {
        cadmpeg_ir::codec::CodecError::NotImplemented(format!(
            "source-less SLDPRT point-line dimension {} requires a line",
            constraint.id.0
        ))
    })?;
    let direction = [end.u - start.u, end.v - start.v];
    let length = vector2_length(direction);
    if length <= SKETCH_POINT_TOLERANCE {
        return Err(cadmpeg_ir::codec::CodecError::Malformed(format!(
            "source-less SLDPRT point-line dimension {} has a degenerate line",
            constraint.id.0
        )));
    }
    Ok(cross2([point.u - start.u, point.v - start.v], direction).abs() / length)
}

fn line_line_dimension(
    constraint: &SketchConstraint,
    first: &SketchEntity,
    second: &SketchEntity,
) -> Result<f64, cadmpeg_ir::codec::CodecError> {
    let (first_start, first_end) = sketch_line(&first.geometry).ok_or_else(|| {
        cadmpeg_ir::codec::CodecError::NotImplemented(format!(
            "source-less SLDPRT line-line dimension {} requires two lines",
            constraint.id.0
        ))
    })?;
    let (second_start, second_end) = sketch_line(&second.geometry).ok_or_else(|| {
        cadmpeg_ir::codec::CodecError::NotImplemented(format!(
            "source-less SLDPRT line-line dimension {} requires two lines",
            constraint.id.0
        ))
    })?;
    let first_direction = [first_end.u - first_start.u, first_end.v - first_start.v];
    let second_direction = [second_end.u - second_start.u, second_end.v - second_start.v];
    let first_length = vector2_length(first_direction);
    let second_length = vector2_length(second_direction);
    if first_length <= SKETCH_POINT_TOLERANCE || second_length <= SKETCH_POINT_TOLERANCE {
        return Err(cadmpeg_ir::codec::CodecError::Malformed(format!(
            "source-less SLDPRT line-line dimension {} has a degenerate line",
            constraint.id.0
        )));
    }
    if cross2(first_direction, second_direction).abs()
        > SKETCH_POINT_TOLERANCE * first_length * second_length
    {
        return Err(cadmpeg_ir::codec::CodecError::Malformed(format!(
            "source-less SLDPRT line-line dimension {} requires parallel solved lines",
            constraint.id.0
        )));
    }
    Ok(cross2(
        [
            second_start.u - first_start.u,
            second_start.v - first_start.v,
        ],
        first_direction,
    )
    .abs()
        / first_length)
}

fn constraint_locus_point(
    ir: &cadmpeg_ir::CadIr,
    constraint: &SketchConstraint,
    locus: &SketchLocus,
) -> Result<Point2, cadmpeg_ir::codec::CodecError> {
    let entity = sketch_constraint_entity(ir, constraint, &locus_entity(locus))?;
    sketch_entity_loci(entity)
        .into_iter()
        .find_map(|(point, candidate)| (candidate == *locus).then_some(point))
        .ok_or_else(|| {
            cadmpeg_ir::codec::CodecError::Malformed(format!(
                "sketch constraint {} references unavailable locus {:?}",
                constraint.id.0, locus
            ))
        })
}

fn binary_marker_relation(
    definition: &SketchConstraintDefinition,
) -> Option<(SketchRelationKind, &SketchEntityId, &SketchEntityId)> {
    Some(match definition {
        SketchConstraintDefinition::Parallel { first, second } => {
            (SketchRelationKind::Parallel, first, second)
        }
        SketchConstraintDefinition::Perpendicular { first, second } => {
            (SketchRelationKind::Perpendicular, first, second)
        }
        SketchConstraintDefinition::Equal { first, second } => {
            (SketchRelationKind::Equal, first, second)
        }
        SketchConstraintDefinition::Collinear { first, second } => {
            (SketchRelationKind::Collinear, first, second)
        }
        SketchConstraintDefinition::Concentric { first, second } => {
            (SketchRelationKind::Concentric, first, second)
        }
        SketchConstraintDefinition::Coradial { first, second } => {
            (SketchRelationKind::Coradial, first, second)
        }
        SketchConstraintDefinition::Tangent { first, second } => {
            (SketchRelationKind::Tangent, first, second)
        }
        _ => return None,
    })
}

fn sketch_constraint_entity<'a>(
    ir: &'a cadmpeg_ir::CadIr,
    constraint: &SketchConstraint,
    entity: &SketchEntityId,
) -> Result<&'a SketchEntity, cadmpeg_ir::codec::CodecError> {
    ir.model
        .sketch_entities
        .iter()
        .find(|candidate| candidate.id == *entity && candidate.sketch == constraint.sketch)
        .ok_or_else(|| {
            cadmpeg_ir::codec::CodecError::Malformed(format!(
                "sketch constraint {} references entity {} outside sketch {}",
                constraint.id.0, entity.0, constraint.sketch.0
            ))
        })
}

fn validate_solved_binary_relation(
    constraint: &SketchConstraint,
    kind: SketchRelationKind,
    first: &SketchEntity,
    second: &SketchEntity,
) -> Result<(), cadmpeg_ir::codec::CodecError> {
    use SketchRelationKind::{
        Collinear, Concentric, Coradial, Equal, Parallel, Perpendicular, Tangent,
    };
    let solved = match kind {
        Parallel | Perpendicular | Collinear => {
            let (first_start, first_end) = sketch_line(&first.geometry).ok_or_else(|| {
                cadmpeg_ir::codec::CodecError::Malformed(format!(
                    "sketch constraint {} requires two line entities",
                    constraint.id.0
                ))
            })?;
            let (second_start, second_end) = sketch_line(&second.geometry).ok_or_else(|| {
                cadmpeg_ir::codec::CodecError::Malformed(format!(
                    "sketch constraint {} requires two line entities",
                    constraint.id.0
                ))
            })?;
            let first_direction = [first_end.u - first_start.u, first_end.v - first_start.v];
            let second_direction = [second_end.u - second_start.u, second_end.v - second_start.v];
            let scale = vector2_length(first_direction) * vector2_length(second_direction);
            if scale <= SKETCH_POINT_TOLERANCE {
                false
            } else if kind == Perpendicular {
                (first_direction[0] * second_direction[0]
                    + first_direction[1] * second_direction[1])
                    .abs()
                    <= SKETCH_POINT_TOLERANCE * scale
            } else {
                let directions_parallel = cross2(first_direction, second_direction).abs()
                    <= SKETCH_POINT_TOLERANCE * scale;
                kind == Parallel && directions_parallel
                    || kind == Collinear
                        && directions_parallel
                        && cross2(
                            [
                                second_start.u - first_start.u,
                                second_start.v - first_start.v,
                            ],
                            first_direction,
                        )
                        .abs()
                            <= SKETCH_POINT_TOLERANCE
                                * vector2_length(first_direction)
                                * (1.0
                                    + vector2_length([
                                        second_start.u - first_start.u,
                                        second_start.v - first_start.v,
                                    ]))
            }
        }
        Concentric => match (
            sketch_center(&first.geometry),
            sketch_center(&second.geometry),
        ) {
            (Some(first), Some(second)) => same_point2(first, second),
            _ => {
                return Err(cadmpeg_ir::codec::CodecError::Malformed(format!(
                    "sketch constraint {} requires two centered entities",
                    constraint.id.0
                )));
            }
        },
        Coradial => match (
            circular_center_radius(&first.geometry),
            circular_center_radius(&second.geometry),
        ) {
            (Some((first_center, first_radius)), Some((second_center, second_radius))) => {
                same_point2(first_center, second_center)
                    && (first_radius - second_radius).abs()
                        <= SKETCH_POINT_TOLERANCE
                            * (1.0 + first_radius.abs().max(second_radius.abs()))
            }
            _ => {
                return Err(cadmpeg_ir::codec::CodecError::Malformed(format!(
                    "sketch constraint {} requires two circular entities",
                    constraint.id.0
                )));
            }
        },
        Equal => equal_sketch_size(&first.geometry, &second.geometry).ok_or_else(|| {
            cadmpeg_ir::codec::CodecError::NotImplemented(format!(
                "source-less SLDPRT equal constraint {} uses unsupported entity families",
                constraint.id.0
            ))
        })?,
        Tangent => solved_tangent(&first.geometry, &second.geometry).ok_or_else(|| {
            cadmpeg_ir::codec::CodecError::NotImplemented(format!(
                "source-less SLDPRT tangent constraint {} uses unsupported entity families",
                constraint.id.0
            ))
        })?,
        _ => unreachable!("only generated binary relation kinds are passed"),
    };
    if !solved {
        return Err(cadmpeg_ir::codec::CodecError::Malformed(format!(
            "source-less SLDPRT sketch constraint {} is not satisfied by its entity geometry",
            constraint.id.0
        )));
    }
    Ok(())
}

fn solved_tangent(first: &SketchGeometry, second: &SketchGeometry) -> Option<bool> {
    match (first, second) {
        (SketchGeometry::Line { start, end }, circular)
        | (circular, SketchGeometry::Line { start, end }) => {
            let (center, radius) = circular_center_radius(circular)?;
            let direction = [end.u - start.u, end.v - start.v];
            let length = vector2_length(direction);
            if length <= SKETCH_POINT_TOLERANCE {
                return Some(false);
            }
            let distance =
                cross2([center.u - start.u, center.v - start.v], direction).abs() / length;
            Some((distance - radius).abs() <= SKETCH_POINT_TOLERANCE * (1.0 + radius.abs()))
        }
        (first, second) => {
            let (first_center, first_radius) = circular_center_radius(first)?;
            let (second_center, second_radius) = circular_center_radius(second)?;
            let distance = vector2_length([
                second_center.u - first_center.u,
                second_center.v - first_center.v,
            ]);
            let external = first_radius + second_radius;
            let internal = (first_radius - second_radius).abs();
            let tolerance = SKETCH_POINT_TOLERANCE * (1.0 + distance.max(external).max(internal));
            Some(
                (distance - external).abs() <= tolerance
                    || (distance - internal).abs() <= tolerance,
            )
        }
    }
}

fn circular_center_radius(geometry: &SketchGeometry) -> Option<(Point2, f64)> {
    match geometry {
        SketchGeometry::Circle { center, radius } | SketchGeometry::Arc { center, radius, .. } => {
            Some((*center, radius.0))
        }
        _ => None,
    }
}

fn sketch_line(geometry: &SketchGeometry) -> Option<(Point2, Point2)> {
    match geometry {
        SketchGeometry::Line { start, end } => Some((*start, *end)),
        _ => None,
    }
}

fn sketch_center(geometry: &SketchGeometry) -> Option<Point2> {
    match geometry {
        SketchGeometry::Circle { center, .. }
        | SketchGeometry::Arc { center, .. }
        | SketchGeometry::Ellipse { center, .. } => Some(*center),
        _ => None,
    }
}

fn equal_sketch_size(first: &SketchGeometry, second: &SketchGeometry) -> Option<bool> {
    let close = |left: f64, right: f64| {
        (left - right).abs() <= SKETCH_POINT_TOLERANCE * (1.0 + left.abs().max(right.abs()))
    };
    Some(match (first, second) {
        (
            SketchGeometry::Line {
                start: first_start,
                end: first_end,
            },
            SketchGeometry::Line {
                start: second_start,
                end: second_end,
            },
        ) => close(
            vector2_length([first_end.u - first_start.u, first_end.v - first_start.v]),
            vector2_length([second_end.u - second_start.u, second_end.v - second_start.v]),
        ),
        (
            SketchGeometry::Circle { radius: first, .. }
            | SketchGeometry::Arc { radius: first, .. },
            SketchGeometry::Circle { radius: second, .. }
            | SketchGeometry::Arc { radius: second, .. },
        ) => close(first.0, second.0),
        (
            SketchGeometry::Ellipse {
                major_radius: first_major,
                minor_radius: first_minor,
                ..
            },
            SketchGeometry::Ellipse {
                major_radius: second_major,
                minor_radius: second_minor,
                ..
            },
        ) => close(first_major.0, second_major.0) && close(first_minor.0, second_minor.0),
        _ => return None,
    })
}

fn vector2_length(vector: [f64; 2]) -> f64 {
    vector[0].hypot(vector[1])
}

fn cross2(first: [f64; 2], second: [f64; 2]) -> f64 {
    first[0] * second[1] - first[1] * second[0]
}

fn same_point2(first: Point2, second: Point2) -> bool {
    (first.u - second.u).abs() <= SKETCH_POINT_TOLERANCE
        && (first.v - second.v).abs() <= SKETCH_POINT_TOLERANCE
}

fn arc_angle_relation_kind(angle: f64) -> Option<SketchRelationKind> {
    const TOLERANCE: f64 = 1.0e-9;
    [
        (std::f64::consts::FRAC_PI_2, SketchRelationKind::ArcAngle90),
        (std::f64::consts::PI, SketchRelationKind::ArcAngle180),
        (
            3.0 * std::f64::consts::FRAC_PI_2,
            SketchRelationKind::ArcAngle270,
        ),
    ]
    .into_iter()
    .find_map(|(expected, kind)| ((angle - expected).abs() <= TOLERANCE).then_some(kind))
}

fn ellipse_angle_relation_kind(angle: f64) -> Option<SketchRelationKind> {
    const TOLERANCE: f64 = 1.0e-9;
    [
        (
            std::f64::consts::FRAC_PI_2,
            SketchRelationKind::EllipseAngle90,
        ),
        (std::f64::consts::PI, SketchRelationKind::EllipseAngle180),
        (
            3.0 * std::f64::consts::FRAC_PI_2,
            SketchRelationKind::EllipseAngle270,
        ),
    ]
    .into_iter()
    .find_map(|(expected, kind)| ((angle - expected).abs() <= TOLERANCE).then_some(kind))
}

fn unique_planar_sketch_owner<'a>(
    ir: &'a cadmpeg_ir::CadIr,
    sketch: &SketchId,
) -> Result<&'a cadmpeg_ir::features::Feature, cadmpeg_ir::codec::CodecError> {
    unique_sketch_owner(ir, &sketch.0, |feature| {
        matches!(
            &feature.definition,
            FeatureDefinition::Sketch {
                sketch: Some(candidate),
                ..
            } if candidate == sketch
        )
    })
}

fn unique_spatial_sketch_owner<'a>(
    ir: &'a cadmpeg_ir::CadIr,
    sketch: &SpatialSketchId,
) -> Result<&'a cadmpeg_ir::features::Feature, cadmpeg_ir::codec::CodecError> {
    unique_sketch_owner(ir, &sketch.0, |feature| {
        matches!(
            &feature.definition,
            FeatureDefinition::SpatialSketch {
                sketch: Some(candidate),
            } if candidate == sketch
        )
    })
}

fn unique_sketch_owner<'a>(
    ir: &'a cadmpeg_ir::CadIr,
    sketch: &str,
    owns: impl Fn(&cadmpeg_ir::features::Feature) -> bool,
) -> Result<&'a cadmpeg_ir::features::Feature, cadmpeg_ir::codec::CodecError> {
    let mut owners = ir.model.features.iter().filter(|feature| owns(feature));
    let owner = owners.next().ok_or_else(|| {
        cadmpeg_ir::codec::CodecError::Malformed(format!(
            "source-less SLDPRT sketch {sketch} has no owning feature"
        ))
    })?;
    if owners.next().is_some() {
        return Err(cadmpeg_ir::codec::CodecError::Malformed(format!(
            "source-less SLDPRT sketch {sketch} has multiple owning features"
        )));
    }
    Ok(owner)
}

fn generated_sketch_owner_record<'a>(
    native: &'a crate::native::SldprtNative,
    owner: &cadmpeg_ir::features::Feature,
    sketch: &str,
) -> Result<&'a crate::records::Feature, cadmpeg_ir::codec::CodecError> {
    let owner_record_id = owner
        .native_ref
        .clone()
        .unwrap_or_else(|| format!("sldprt:generated:feature#{}", owner.id.0));
    native
        .feature_histories
        .iter()
        .flat_map(|history| &history.features)
        .find(|feature| feature.id == owner_record_id)
        .ok_or_else(|| {
            cadmpeg_ir::codec::CodecError::Malformed(format!(
                "source-less SLDPRT sketch {sketch} has no native feature record"
            ))
        })
}

fn generated_sketch_owner_id(
    owner: &crate::records::Feature,
    sketch: &str,
) -> Result<u32, cadmpeg_ir::codec::CodecError> {
    owner
        .source_id
        .as_deref()
        .and_then(|source_id| source_id.parse::<u32>().ok())
        .ok_or_else(|| {
            cadmpeg_ir::codec::CodecError::Malformed(format!(
                "source-less SLDPRT sketch {sketch} has no numeric feature source id"
            ))
        })
}

fn source_less_lanes(
    ir: &cadmpeg_ir::CadIr,
    native: &crate::native::SldprtNative,
) -> Result<Vec<FeatureInputLane>, cadmpeg_ir::codec::CodecError> {
    let mut objects = Vec::<(String, u64, Vec<u8>)>::new();
    for sketch in &ir.model.sketches {
        let configuration = sketch.configuration.clone().unwrap_or_else(|| "0".into());
        let owner = unique_planar_sketch_owner(ir, &sketch.id)?;
        let owner_record = generated_sketch_owner_record(native, owner, &sketch.id.0)?;
        let object_id = generated_sketch_owner_id(owner_record, &sketch.id.0)?;
        let mut payload = Vec::new();
        append_generated_object_name(
            &mut payload,
            if owner_record.name.is_empty() {
                sketch.name.as_deref().unwrap_or(&sketch.id.0)
            } else {
                owner_record.name.as_str()
            },
            object_id,
        )?;
        append_generated_sketch_markers(ir, sketch, &mut payload)?;
        let sketch_ir = sketch_brep(ir, sketch)?;
        let body = crate::writer::brep_body(&sketch_ir, 0.001, false)?;
        payload.extend(crate::writer::parasolid_stream_named(
            &body,
            "SCH_SW_33103_11000",
            sketch.name.as_deref().unwrap_or(&sketch.id.0),
        ));
        objects.push((configuration, owner.ordinal, payload));
    }
    for sketch in &ir.model.spatial_sketches {
        let configuration = sketch.configuration.clone().unwrap_or_else(|| "0".into());
        let owner = unique_spatial_sketch_owner(ir, &sketch.id)?;
        let owner_record = generated_sketch_owner_record(native, owner, &sketch.id.0)?;
        let object_id = generated_sketch_owner_id(owner_record, &sketch.id.0)?;
        let mut payload = Vec::new();
        append_generated_object_name(
            &mut payload,
            if owner_record.name.is_empty() {
                sketch.name.as_deref().unwrap_or(&sketch.id.0)
            } else {
                owner_record.name.as_str()
            },
            object_id,
        )?;
        let [entity_id] = sketch.entities.as_slice() else {
            return Err(cadmpeg_ir::codec::CodecError::NotImplemented(format!(
                "source-less SLDPRT spatial sketch {} requires exactly one line",
                sketch.id.0
            )));
        };
        let entity = ir
            .model
            .spatial_sketch_entities
            .iter()
            .find(|entity| entity.id == *entity_id && entity.sketch == sketch.id)
            .ok_or_else(|| {
                cadmpeg_ir::codec::CodecError::Malformed(format!(
                    "source-less SLDPRT spatial sketch {} references missing entity {}",
                    sketch.id.0, entity_id.0
                ))
            })?;
        let SpatialSketchGeometry::Line { start, end } = entity.geometry else {
            return Err(cadmpeg_ir::codec::CodecError::NotImplemented(format!(
                "source-less SLDPRT spatial sketch {} supports line geometry only",
                sketch.id.0
            )));
        };
        if start == end {
            return Err(cadmpeg_ir::codec::CodecError::Malformed(format!(
                "source-less SLDPRT spatial sketch {} has a zero-length line",
                sketch.id.0
            )));
        }
        append_spatial_vertex(&mut payload, start);
        append_spatial_vertex(&mut payload, end);
        objects.push((configuration, owner.ordinal, payload));
    }
    let mut lanes = assemble_source_less_lanes(objects);
    for lane in &mut lanes {
        lane.classes = class_declarations(&lane.native_payload, &lane.id);
        lane.names = object_names(&lane.native_payload, &lane.id);
        lane.scalars = named_scalars(&lane.native_payload, &lane.id, &lane.names);
        lane.relation_bindings = relation_bindings(&lane.id, &lane.classes, &lane.scalars);
        lane.references = reference_cells(&lane.scalars);
        lane.sketch_entities = sketch_input_entities(&lane.native_payload, &lane.id);
    }
    bind_scalar_operands(&native.feature_histories, &mut lanes);
    Ok(lanes)
}

fn assemble_source_less_lanes(mut objects: Vec<(String, u64, Vec<u8>)>) -> Vec<FeatureInputLane> {
    objects.sort_by(|left, right| (&left.0, left.1).cmp(&(&right.0, right.1)));
    let mut lanes = Vec::new();
    for (configuration, _, payload) in objects {
        source_less_lane(&mut lanes, &configuration)
            .native_payload
            .extend(payload);
    }
    lanes
}

fn source_less_lane<'a>(
    lanes: &'a mut Vec<FeatureInputLane>,
    configuration: &str,
) -> &'a mut FeatureInputLane {
    if let Some(position) = lanes
        .iter()
        .position(|lane| lane.configuration.as_deref() == Some(configuration))
    {
        return &mut lanes[position];
    }
    lanes.push(FeatureInputLane {
        id: format!("Contents/Config-{configuration}-ResolvedFeatures"),
        configuration: Some(configuration.into()),
        native_payload: Vec::new(),
        classes: Vec::new(),
        names: Vec::new(),
        scalars: Vec::new(),
        relation_bindings: Vec::new(),
        relation_instances: Vec::new(),
        body_selections: Vec::new(),
        edge_selections: Vec::new(),
        surface_selections: Vec::new(),
        references: Vec::new(),
        sketch_entities: Vec::new(),
    });
    lanes.last_mut().expect("lane was inserted")
}

fn append_spatial_vertex(payload: &mut Vec<u8>, point: Point3) {
    let start = payload.len();
    payload.resize(start + 69, 0);
    payload[start..start + SPATIAL_VERTEX_PREFIX.len()].copy_from_slice(SPATIAL_VERTEX_PREFIX);
    payload[start + 43..start + 45].copy_from_slice(&[0x0e, 0x00]);
    payload[start + 45..start + 53].copy_from_slice(&point.x.to_le_bytes());
    payload[start + 53..start + 61].copy_from_slice(&point.y.to_le_bytes());
    payload[start + 61..start + 69].copy_from_slice(&point.z.to_le_bytes());
}

#[cfg(test)]
mod source_less_lane_tests {
    use cadmpeg_ir::sketches::{SketchConstraintDefinition, SketchEntityId, SketchLocus};

    use super::{
        append_coordinate_marker, append_reference_marker, assemble_source_less_lanes,
        generated_marker_relations, marker_local_links, GeneratedMarkerRelation,
    };

    #[test]
    fn objects_follow_feature_ordinals_within_each_configuration() {
        let lanes = assemble_source_less_lanes(vec![
            ("1".into(), 2, vec![2]),
            ("0".into(), 9, vec![9]),
            ("1".into(), 1, vec![1]),
        ]);

        assert_eq!(lanes.len(), 2);
        assert_eq!(lanes[0].configuration.as_deref(), Some("0"));
        assert_eq!(lanes[0].native_payload, [9]);
        assert_eq!(lanes[1].configuration.as_deref(), Some("1"));
        assert_eq!(lanes[1].native_payload, [1, 2]);
    }

    #[test]
    fn non_endpoint_coincidences_form_pairwise_native_relations() {
        let point = SketchLocus::Entity(SketchEntityId("point".into()));
        let center = SketchLocus::Center(SketchEntityId("circle".into()));
        let endpoint = SketchLocus::Start(SketchEntityId("line".into()));
        let definition = SketchConstraintDefinition::CoincidentLoci {
            loci: vec![point.clone(), center.clone(), endpoint.clone()],
        };

        let relations = generated_marker_relations(&definition);

        assert_eq!(relations.len(), 2);
        assert!(matches!(
            relations[0],
            GeneratedMarkerRelation::Loci(
                crate::records::SketchRelationKind::Coincident,
                first,
                second,
            ) if first == &point && second == &center
        ));
        assert!(matches!(
            relations[1],
            GeneratedMarkerRelation::Loci(
                crate::records::SketchRelationKind::Coincident,
                first,
                second,
            ) if first == &point && second == &endpoint
        ));
    }

    #[test]
    fn endpoint_only_coincidences_remain_topology_derived() {
        let definition = SketchConstraintDefinition::CoincidentLoci {
            loci: vec![
                SketchLocus::End(SketchEntityId("first".into())),
                SketchLocus::Start(SketchEntityId("second".into())),
            ],
        };

        assert!(generated_marker_relations(&definition).is_empty());
    }

    #[test]
    fn generated_coincident_relation_uses_parseable_local_links() {
        let mut payload = Vec::new();
        append_coordinate_marker(
            &mut payload,
            crate::records::SketchInputKind::Point,
            [0.0, 0.0],
            1,
        );
        append_coordinate_marker(
            &mut payload,
            crate::records::SketchInputKind::Point,
            [0.0, 0.0],
            2,
        );
        append_reference_marker(
            &mut payload,
            crate::records::SketchRelationKind::Coincident,
            [1, 2],
            3,
        );

        assert_eq!(marker_local_links(&payload, 284), Some(([1, 2], 0)));
    }
}

enum GeneratedMarkerRelation<'a> {
    Unary(SketchRelationKind, &'a SketchEntityId),
    Binary(SketchRelationKind, &'a SketchEntityId, &'a SketchEntityId),
    Loci(SketchRelationKind, &'a SketchLocus, &'a SketchLocus),
    Midpoint(&'a SketchLocus, &'a SketchEntityId),
}

fn generated_marker_relations(
    definition: &SketchConstraintDefinition,
) -> Vec<GeneratedMarkerRelation<'_>> {
    match definition {
        SketchConstraintDefinition::Horizontal { entity } => vec![GeneratedMarkerRelation::Unary(
            SketchRelationKind::Horizontal,
            entity,
        )],
        SketchConstraintDefinition::Vertical { entity } => vec![GeneratedMarkerRelation::Unary(
            SketchRelationKind::Vertical,
            entity,
        )],
        SketchConstraintDefinition::Fixed { entity } => vec![GeneratedMarkerRelation::Unary(
            SketchRelationKind::Fixed,
            entity,
        )],
        SketchConstraintDefinition::ArcAngle { entity, angle } => arc_angle_relation_kind(angle.0)
            .map(|kind| vec![GeneratedMarkerRelation::Unary(kind, entity)])
            .unwrap_or_default(),
        SketchConstraintDefinition::EllipseAngle { entity, angle } => {
            ellipse_angle_relation_kind(angle.0)
                .map(|kind| vec![GeneratedMarkerRelation::Unary(kind, entity)])
                .unwrap_or_default()
        }
        SketchConstraintDefinition::HorizontalPoints { first, second } => {
            vec![GeneratedMarkerRelation::Loci(
                SketchRelationKind::HorizontalPoints,
                first,
                second,
            )]
        }
        SketchConstraintDefinition::VerticalPoints { first, second } => {
            vec![GeneratedMarkerRelation::Loci(
                SketchRelationKind::VerticalPoints,
                first,
                second,
            )]
        }
        SketchConstraintDefinition::Midpoint { point, entity } => {
            vec![GeneratedMarkerRelation::Midpoint(point, entity)]
        }
        SketchConstraintDefinition::CoincidentLoci { loci }
            if !loci
                .iter()
                .all(|locus| matches!(locus, SketchLocus::Start(_) | SketchLocus::End(_))) =>
        {
            loci.first()
                .map(|first| {
                    loci.iter()
                        .skip(1)
                        .map(|locus| {
                            GeneratedMarkerRelation::Loci(
                                SketchRelationKind::Coincident,
                                first,
                                locus,
                            )
                        })
                        .collect()
                })
                .unwrap_or_default()
        }
        definition => binary_marker_relation(definition)
            .map(|(kind, first, second)| vec![GeneratedMarkerRelation::Binary(kind, first, second)])
            .unwrap_or_default(),
    }
}

enum GeneratedDimension<'a> {
    PointPoint(
        &'a SketchLocus,
        &'a SketchLocus,
        &'a cadmpeg_ir::features::ParameterId,
    ),
    PointLine(
        &'a SketchLocus,
        &'a SketchEntityId,
        &'a cadmpeg_ir::features::ParameterId,
    ),
    LineLine(
        &'a SketchEntityId,
        &'a SketchEntityId,
        &'a cadmpeg_ir::features::ParameterId,
    ),
    Horizontal(
        &'a SketchLocus,
        &'a SketchLocus,
        &'a cadmpeg_ir::features::ParameterId,
    ),
    Vertical(
        &'a SketchLocus,
        &'a SketchLocus,
        &'a cadmpeg_ir::features::ParameterId,
    ),
    Angle(
        &'a SketchEntityId,
        &'a SketchEntityId,
        &'a cadmpeg_ir::features::ParameterId,
    ),
    Circle(&'a SketchEntityId, &'a cadmpeg_ir::features::ParameterId),
}

fn append_generated_sketch_markers(
    ir: &cadmpeg_ir::CadIr,
    sketch: &Sketch,
    payload: &mut Vec<u8>,
) -> Result<(), cadmpeg_ir::codec::CodecError> {
    let relations = ir
        .model
        .sketch_constraints
        .iter()
        .filter(|constraint| constraint.sketch == sketch.id)
        .flat_map(|constraint| generated_marker_relations(&constraint.definition))
        .collect::<Vec<_>>();
    let dimensions = ir
        .model
        .sketch_constraints
        .iter()
        .filter(|constraint| constraint.sketch == sketch.id)
        .filter_map(|constraint| generated_dimension(ir, &constraint.definition))
        .collect::<Result<Vec<_>, _>>()?;
    if relations.is_empty() && dimensions.is_empty() {
        return Ok(());
    }

    let mut marker_ids = HashMap::<SketchEntityId, Vec<u16>>::new();
    let mut marker_loci = Vec::<(SketchLocus, Point2, SketchInputKind, u16)>::new();
    let mut next_id = 1u32;
    for entity in ir
        .model
        .sketch_entities
        .iter()
        .filter(|entity| entity.sketch == sketch.id)
    {
        for (point, locus) in sketch_entity_loci(entity) {
            let local_id = u16::try_from(next_id).map_err(|_| {
                cadmpeg_ir::codec::CodecError::Malformed(format!(
                    "source-less SLDPRT sketch {} exceeds the marker-local id space",
                    sketch.id.0
                ))
            })?;
            append_coordinate_marker(
                payload,
                generated_marker_kind(&entity.geometry),
                [point.u * 0.001, point.v * 0.001],
                next_id,
            );
            marker_ids
                .entry(entity.id.clone())
                .or_default()
                .push(local_id);
            marker_loci.push((
                locus,
                point,
                generated_marker_kind(&entity.geometry),
                local_id,
            ));
            next_id += 1;
        }
    }
    for relation in relations {
        let (kind, links) = match relation {
            GeneratedMarkerRelation::Unary(kind, entity) => {
                let ids = marker_ids.get(entity).ok_or_else(|| {
                    cadmpeg_ir::codec::CodecError::NotImplemented(format!(
                        "source-less SLDPRT relation on {} has no coordinate-bearing marker loci",
                        entity.0
                    ))
                })?;
                let links = match unique_generated_entity_marker(ir, sketch, &marker_loci, entity) {
                    Ok(unique) => [unique, unique],
                    Err(_) => match ids.as_slice() {
                        [only] => [*only, *only],
                        [first, second, ..] => [*first, *second],
                        [] => unreachable!("empty marker-id vectors are never inserted"),
                    },
                };
                (kind, links)
            }
            GeneratedMarkerRelation::Binary(kind, first, second) => (
                kind,
                [
                    unique_generated_entity_marker(ir, sketch, &marker_loci, first)?,
                    unique_generated_entity_marker(ir, sketch, &marker_loci, second)?,
                ],
            ),
            GeneratedMarkerRelation::Loci(kind, first, second) => (
                kind,
                [
                    unique_generated_locus_marker(ir, sketch, &marker_loci, first)?,
                    unique_generated_locus_marker(ir, sketch, &marker_loci, second)?,
                ],
            ),
            GeneratedMarkerRelation::Midpoint(point, entity) => (
                SketchRelationKind::Midpoint,
                [
                    unique_generated_locus_marker(ir, sketch, &marker_loci, point)?,
                    unique_generated_entity_marker(ir, sketch, &marker_loci, entity)?,
                ],
            ),
        };
        append_reference_marker(payload, kind, links, next_id);
        next_id = next_id.checked_add(1).ok_or_else(|| {
            cadmpeg_ir::codec::CodecError::Malformed(
                "source-less SLDPRT marker-local id space is exhausted".into(),
            )
        })?;
    }
    for dimension in dimensions {
        let (class, operands, parameter) = match dimension {
            GeneratedDimension::PointPoint(first, second, parameter) => (
                "sgPntPntDist",
                vec![
                    (
                        FeatureInputOperandKind::D6,
                        generated_locus_operand(
                            ir,
                            sketch,
                            &marker_loci,
                            first,
                            FeatureInputOperandKind::D6,
                        )?,
                    ),
                    (
                        FeatureInputOperandKind::D6,
                        generated_locus_operand(
                            ir,
                            sketch,
                            &marker_loci,
                            second,
                            FeatureInputOperandKind::D6,
                        )?,
                    ),
                ],
                parameter,
            ),
            GeneratedDimension::PointLine(point, line, parameter) => (
                "sgPntLineDist",
                vec![
                    (
                        FeatureInputOperandKind::D6,
                        generated_locus_operand(
                            ir,
                            sketch,
                            &marker_loci,
                            point,
                            FeatureInputOperandKind::D6,
                        )?,
                    ),
                    (
                        FeatureInputOperandKind::E1,
                        generated_entity_operand(
                            ir,
                            sketch,
                            &marker_loci,
                            line,
                            FeatureInputOperandKind::E1,
                        )?,
                    ),
                ],
                parameter,
            ),
            GeneratedDimension::LineLine(first, second, parameter) => (
                "sgLLDist",
                vec![
                    (
                        FeatureInputOperandKind::E1,
                        generated_entity_operand(
                            ir,
                            sketch,
                            &marker_loci,
                            first,
                            FeatureInputOperandKind::E1,
                        )?,
                    ),
                    (
                        FeatureInputOperandKind::E1,
                        generated_entity_operand(
                            ir,
                            sketch,
                            &marker_loci,
                            second,
                            FeatureInputOperandKind::E1,
                        )?,
                    ),
                ],
                parameter,
            ),
            GeneratedDimension::Horizontal(first, second, parameter) => (
                "sgPntPntHorDist",
                vec![
                    (
                        FeatureInputOperandKind::Native(0x8dcb),
                        generated_locus_operand(
                            ir,
                            sketch,
                            &marker_loci,
                            first,
                            FeatureInputOperandKind::Native(0x8dcb),
                        )?,
                    ),
                    (
                        FeatureInputOperandKind::Native(0x8dcb),
                        generated_locus_operand(
                            ir,
                            sketch,
                            &marker_loci,
                            second,
                            FeatureInputOperandKind::Native(0x8dcb),
                        )?,
                    ),
                ],
                parameter,
            ),
            GeneratedDimension::Vertical(first, second, parameter) => (
                "sgPntPntVertDist",
                vec![
                    (
                        FeatureInputOperandKind::Native(0x8dcb),
                        generated_locus_operand(
                            ir,
                            sketch,
                            &marker_loci,
                            first,
                            FeatureInputOperandKind::Native(0x8dcb),
                        )?,
                    ),
                    (
                        FeatureInputOperandKind::Native(0x8dcb),
                        generated_locus_operand(
                            ir,
                            sketch,
                            &marker_loci,
                            second,
                            FeatureInputOperandKind::Native(0x8dcb),
                        )?,
                    ),
                ],
                parameter,
            ),
            GeneratedDimension::Angle(first, second, parameter) => (
                "sgAnglDim",
                vec![
                    (
                        FeatureInputOperandKind::Native(0x8dda),
                        generated_entity_operand(
                            ir,
                            sketch,
                            &marker_loci,
                            first,
                            FeatureInputOperandKind::Native(0x8dda),
                        )?,
                    ),
                    (
                        FeatureInputOperandKind::Native(0x8dda),
                        generated_entity_operand(
                            ir,
                            sketch,
                            &marker_loci,
                            second,
                            FeatureInputOperandKind::Native(0x8dda),
                        )?,
                    ),
                ],
                parameter,
            ),
            GeneratedDimension::Circle(entity, parameter) => (
                "sgCircleDim",
                vec![(
                    FeatureInputOperandKind::Native(0x83fe),
                    generated_entity_operand(
                        ir,
                        sketch,
                        &marker_loci,
                        entity,
                        FeatureInputOperandKind::Native(0x83fe),
                    )?,
                )],
                parameter,
            ),
        };
        let parameter = ir
            .model
            .parameters
            .iter()
            .find(|candidate| candidate.id == *parameter)
            .ok_or_else(|| {
                cadmpeg_ir::codec::CodecError::Malformed(format!(
                    "source-less SLDPRT dimension references missing parameter {}",
                    parameter.0
                ))
            })?;
        let value = match (&parameter.value, class) {
            (Some(cadmpeg_ir::features::ParameterValue::Length(_)), "sgAnglDim") => {
                return Err(cadmpeg_ir::codec::CodecError::Malformed(format!(
                    "source-less SLDPRT angular dimension {} has a length value",
                    parameter.id.0
                )))
            }
            (Some(cadmpeg_ir::features::ParameterValue::Angle(value)), "sgAnglDim") => value.0,
            (Some(cadmpeg_ir::features::ParameterValue::Length(value)), _) => value.0 * 0.001,
            _ => {
                return Err(cadmpeg_ir::codec::CodecError::Malformed(format!(
                    "source-less SLDPRT dimension parameter {} has no compatible evaluated value",
                    parameter.id.0
                )))
            }
        };
        append_generated_scalar(payload, class, &parameter.name, value, next_id, &operands)?;
        next_id = next_id.checked_add(1).ok_or_else(|| {
            cadmpeg_ir::codec::CodecError::Malformed(
                "source-less SLDPRT marker-local id space is exhausted".into(),
            )
        })?;
    }
    Ok(())
}

fn generated_dimension<'a>(
    ir: &cadmpeg_ir::CadIr,
    definition: &'a SketchConstraintDefinition,
) -> Option<Result<GeneratedDimension<'a>, cadmpeg_ir::codec::CodecError>> {
    let unsupported = || {
        cadmpeg_ir::codec::CodecError::NotImplemented(
            "source-less SLDPRT distance dimensions require two entities or point/entity loci"
                .into(),
        )
    };
    match definition {
        SketchConstraintDefinition::DistanceLoci {
            first,
            second,
            parameter,
        } => Some(match (first, second) {
            (SketchLocus::Entity(first), SketchLocus::Entity(second))
                if !generated_locus_is_point(ir, first)
                    && !generated_locus_is_point(ir, second) =>
            {
                Ok(GeneratedDimension::LineLine(first, second, parameter))
            }
            (SketchLocus::Entity(line), point) if !generated_locus_is_point(ir, line) => {
                Ok(GeneratedDimension::PointLine(point, line, parameter))
            }
            (point, SketchLocus::Entity(line)) if !generated_locus_is_point(ir, line) => {
                Ok(GeneratedDimension::PointLine(point, line, parameter))
            }
            (first, second) => Ok(GeneratedDimension::PointPoint(first, second, parameter)),
        }),
        SketchConstraintDefinition::Distance {
            entities,
            parameter,
        } => Some(match entities.as_slice() {
            [first, second] => Ok(GeneratedDimension::LineLine(first, second, parameter)),
            _ => Err(unsupported()),
        }),
        SketchConstraintDefinition::HorizontalDistance {
            first,
            second,
            parameter,
        } => Some(Ok(GeneratedDimension::Horizontal(first, second, parameter))),
        SketchConstraintDefinition::VerticalDistance {
            first,
            second,
            parameter,
        } => Some(Ok(GeneratedDimension::Vertical(first, second, parameter))),
        SketchConstraintDefinition::Angle {
            first,
            second,
            parameter,
        } => Some(Ok(GeneratedDimension::Angle(first, second, parameter))),
        SketchConstraintDefinition::Radius { entity, parameter }
        | SketchConstraintDefinition::Diameter { entity, parameter } => {
            Some(Ok(GeneratedDimension::Circle(entity, parameter)))
        }
        _ => None,
    }
}

fn generated_locus_is_point(ir: &cadmpeg_ir::CadIr, entity: &SketchEntityId) -> bool {
    ir.model
        .sketch_entities
        .iter()
        .find(|candidate| candidate.id == *entity)
        .is_some_and(|candidate| matches!(candidate.geometry, SketchGeometry::Point { .. }))
}

fn append_generated_scalar(
    payload: &mut Vec<u8>,
    class: &str,
    name: &str,
    value: f64,
    object_id: u32,
    operands: &[(FeatureInputOperandKind, u16)],
) -> Result<(), cadmpeg_ir::codec::CodecError> {
    let units = name.encode_utf16().collect::<Vec<_>>();
    let length = u8::try_from(units.len()).map_err(|_| {
        cadmpeg_ir::codec::CodecError::Malformed(
            "SLDPRT generated parameter name exceeds 255 UTF-16 code units".into(),
        )
    })?;
    if length == 0 || length > 128 {
        return Err(cadmpeg_ir::codec::CodecError::Malformed(
            "SLDPRT generated parameter name must contain 1 to 128 UTF-16 code units".into(),
        ));
    }
    payload.extend_from_slice(CLASS_MARKER);
    payload.extend_from_slice(&(class.len() as u16).to_le_bytes());
    payload.extend_from_slice(class.as_bytes());
    payload.extend_from_slice(NAME_MARKER);
    payload.push(length);
    for unit in units {
        payload.extend_from_slice(&unit.to_le_bytes());
    }
    payload.extend_from_slice(SCALAR_HEADER);
    payload.extend_from_slice(&value.to_le_bytes());
    let trailer = payload.len();
    payload.resize(trailer + 35 + operands.len() * 12, 0);
    payload[trailer + 3..trailer + 7].copy_from_slice(&object_id.to_le_bytes());
    payload[trailer + 24..trailer + 29].copy_from_slice(&[0, 0, 0, 2, 0]);
    for (index, (kind, entity)) in operands.iter().enumerate() {
        let offset = trailer + 35 + index * 12;
        let tag = match kind {
            FeatureInputOperandKind::D6 => 0x80d6,
            FeatureInputOperandKind::E1 => 0x80e1,
            FeatureInputOperandKind::Native(tag) => *tag,
        };
        payload[offset..offset + 2].copy_from_slice(&tag.to_le_bytes());
        payload[offset + 2..offset + 4].copy_from_slice(&entity.to_le_bytes());
        payload[offset + 4..offset + 8].fill(0xff);
    }
    Ok(())
}

fn unique_generated_entity_marker(
    ir: &cadmpeg_ir::CadIr,
    sketch: &Sketch,
    markers: &[(SketchLocus, Point2, SketchInputKind, u16)],
    entity: &SketchEntityId,
) -> Result<u16, cadmpeg_ir::codec::CodecError> {
    for (_, point, kind, local_id) in markers
        .iter()
        .filter(|(candidate, ..)| locus_entity(candidate) == *entity)
    {
        let mut candidates = ir
            .model
            .sketch_entities
            .iter()
            .filter(|candidate| candidate.sketch == sketch.id)
            .filter(|candidate| marker_accepts_locus(*kind, &candidate.geometry))
            .filter(|candidate| {
                sketch_entity_loci(candidate)
                    .iter()
                    .any(|(candidate, _)| same_point2(*point, *candidate))
            })
            .map(|candidate| &candidate.id);
        if candidates.next() == Some(entity) && candidates.next().is_none() {
            return Ok(*local_id);
        }
    }
    Err(cadmpeg_ir::codec::CodecError::NotImplemented(format!(
        "source-less SLDPRT binary relation cannot identify entity {} with one unambiguous marker locus",
        entity.0
    )))
}

fn generated_entity_operand(
    ir: &cadmpeg_ir::CadIr,
    sketch: &Sketch,
    markers: &[(SketchLocus, Point2, SketchInputKind, u16)],
    entity: &SketchEntityId,
    kind: FeatureInputOperandKind,
) -> Result<u16, cadmpeg_ir::codec::CodecError> {
    let local_id = unique_generated_entity_marker(ir, sketch, markers, entity)?;
    generated_operand_address(markers, local_id, kind, sketch)
}

fn generated_locus_operand(
    ir: &cadmpeg_ir::CadIr,
    sketch: &Sketch,
    markers: &[(SketchLocus, Point2, SketchInputKind, u16)],
    locus: &SketchLocus,
    kind: FeatureInputOperandKind,
) -> Result<u16, cadmpeg_ir::codec::CodecError> {
    let local_id = unique_generated_locus_marker(ir, sketch, markers, locus)?;
    generated_operand_address(markers, local_id, kind, sketch)
}

fn generated_operand_address(
    markers: &[(SketchLocus, Point2, SketchInputKind, u16)],
    local_id: u16,
    kind: FeatureInputOperandKind,
    sketch: &Sketch,
) -> Result<u16, cadmpeg_ir::codec::CodecError> {
    if !operand_uses_compatible_ordinal(kind) {
        return Ok(local_id);
    }
    let ordinal = markers
        .iter()
        .filter(|(_, _, marker_kind, _)| operand_accepts_marker(kind, *marker_kind))
        .position(|(_, _, _, candidate)| *candidate == local_id)
        .ok_or_else(|| {
            cadmpeg_ir::codec::CodecError::NotImplemented(format!(
                "source-less SLDPRT dimension operand cannot address marker {local_id} with tag {kind:?}"
            ))
        })?;
    u16::try_from(ordinal).map_err(|_| {
        cadmpeg_ir::codec::CodecError::Malformed(format!(
            "source-less SLDPRT sketch {} exceeds the dimension operand space",
            sketch.id.0
        ))
    })
}

fn unique_generated_locus_marker(
    ir: &cadmpeg_ir::CadIr,
    sketch: &Sketch,
    markers: &[(SketchLocus, Point2, SketchInputKind, u16)],
    locus: &SketchLocus,
) -> Result<u16, cadmpeg_ir::codec::CodecError> {
    for (_, point, kind, local_id) in markers.iter().filter(|(candidate, ..)| candidate == locus) {
        let mut candidates = ir
            .model
            .sketch_entities
            .iter()
            .filter(|candidate| candidate.sketch == sketch.id)
            .filter(|candidate| marker_accepts_locus(*kind, &candidate.geometry))
            .flat_map(sketch_entity_loci)
            .filter_map(|(candidate_point, candidate_locus)| {
                same_point2(*point, candidate_point).then_some(candidate_locus)
            })
            .collect::<Vec<_>>();
        candidates.sort_by(|left, right| locus_key(left).cmp(&locus_key(right)));
        candidates.dedup();
        if candidates.as_slice() == [locus.clone()] {
            return Ok(*local_id);
        }
    }
    Err(cadmpeg_ir::codec::CodecError::NotImplemented(format!(
        "source-less SLDPRT locus relation cannot identify {locus:?} with one unambiguous marker"
    )))
}

fn generated_marker_kind(geometry: &SketchGeometry) -> SketchInputKind {
    match geometry {
        SketchGeometry::Point { .. } => SketchInputKind::Point,
        SketchGeometry::Arc { .. } => SketchInputKind::Arc,
        SketchGeometry::Line { .. }
        | SketchGeometry::Circle { .. }
        | SketchGeometry::Ellipse { .. }
        | SketchGeometry::Nurbs { .. }
        | SketchGeometry::Native { .. } => SketchInputKind::LineOrCircle,
    }
}

fn append_coordinate_marker(
    payload: &mut Vec<u8>,
    kind: SketchInputKind,
    coordinates_m: [f64; 2],
    local_id: u32,
) {
    let start = payload.len();
    payload.resize(start + 142, 0);
    payload[start..start + SKETCH_MARKER.len()].copy_from_slice(SKETCH_MARKER);
    payload[start + 5..start + 13].fill(0xff);
    payload[start + 13..start + 17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    payload[start + 17..start + 21].copy_from_slice(&kind.native_code().to_le_bytes());
    payload[start + 23..start + 27].copy_from_slice(&[0x05, 0x00, 0x01, 0x00]);
    payload[start + 48..start + 56].copy_from_slice(&1.0f64.to_le_bytes());
    payload[start + 64..start + 66].copy_from_slice(&[0x1e, 0x00]);
    payload[start + 66..start + 74].copy_from_slice(&coordinates_m[0].to_le_bytes());
    payload[start + 74..start + 82].copy_from_slice(&coordinates_m[1].to_le_bytes());
    payload[start + 138..start + 142].copy_from_slice(&local_id.to_le_bytes());
}

fn append_reference_marker(
    payload: &mut Vec<u8>,
    kind: SketchRelationKind,
    links: [u16; 2],
    local_id: u32,
) {
    let start = payload.len();
    payload.resize(start + 92, 0);
    payload[start..start + SKETCH_MARKER.len()].copy_from_slice(SKETCH_MARKER);
    payload[start + 17..start + 21].copy_from_slice(&kind.native_code().to_le_bytes());
    payload[start + 48..start + 56].copy_from_slice(&1.0f64.to_le_bytes());
    payload[start + 64..start + 66].copy_from_slice(&links[0].to_le_bytes());
    payload[start + 66..start + 68].copy_from_slice(&links[1].to_le_bytes());
    payload[start + 72..start + 80].copy_from_slice(&(-1.0f64).to_le_bytes());
    payload[start + 88..start + 92].copy_from_slice(&local_id.to_le_bytes());
}

fn append_generated_object_name(
    payload: &mut Vec<u8>,
    name: &str,
    object_id: u32,
) -> Result<(), cadmpeg_ir::codec::CodecError> {
    let units = name.encode_utf16().collect::<Vec<_>>();
    let length = u8::try_from(units.len()).map_err(|_| {
        cadmpeg_ir::codec::CodecError::Malformed(
            "SLDPRT generated feature name exceeds 255 UTF-16 code units".into(),
        )
    })?;
    if length == 0 || length > 128 {
        return Err(cadmpeg_ir::codec::CodecError::Malformed(
            "SLDPRT generated feature name must contain 1 to 128 UTF-16 code units".into(),
        ));
    }
    payload.extend_from_slice(NAME_MARKER);
    payload.push(length);
    for unit in units {
        payload.extend_from_slice(&unit.to_le_bytes());
    }
    payload.extend_from_slice(&[0; 8]);
    payload.extend_from_slice(&object_id.to_le_bytes());
    Ok(())
}

fn sketch_brep(
    source: &cadmpeg_ir::CadIr,
    sketch: &Sketch,
) -> Result<cadmpeg_ir::CadIr, cadmpeg_ir::codec::CodecError> {
    let mut ir = cadmpeg_ir::CadIr::empty(source.units.clone());
    let prefix = format!("generated:sldprt:sketch:{}", sketch.id.0);
    let body_id = BodyId(format!("{prefix}:body"));
    let region_id = RegionId(format!("{prefix}:region"));
    let shell_id = ShellId(format!("{prefix}:shell"));
    let face_id = FaceId(format!("{prefix}:face"));
    let surface_id = SurfaceId(format!("{prefix}:surface"));
    let v_axis = cross(sketch.normal, sketch.u_axis);
    ir.model.surfaces.push(Surface {
        id: surface_id.clone(),
        geometry: SurfaceGeometry::Plane {
            origin: sketch.origin,
            normal: sketch.normal,
            u_axis: sketch.u_axis,
        },
        source_object: None,
    });
    let ordered_entities = source
        .model
        .sketch_entities
        .iter()
        .filter(|entity| entity.sketch == sketch.id)
        .collect::<Vec<_>>();
    let entities = ordered_entities
        .iter()
        .copied()
        .map(|entity| (entity.id.clone(), entity))
        .collect::<HashMap<_, _>>();
    let referenced = sketch
        .profiles
        .iter()
        .flatten()
        .map(|entity_use| entity_use.entity.clone())
        .collect::<HashSet<_>>();
    if let Some(entity) = ordered_entities.iter().find(|entity| {
        !referenced.contains(&entity.id) && !matches!(entity.geometry, SketchGeometry::Point { .. })
    }) {
        return Err(cadmpeg_ir::codec::CodecError::NotImplemented(format!(
            "source-less SLDPRT sketch writing cannot encode unprofiled curve {}",
            entity.id.0
        )));
    }
    let profiles = sketch.profiles.clone();
    let mut face_loops = Vec::new();
    let mut vertex_by_position = HashMap::<(u64, u64), VertexId>::new();
    for (profile_index, profile) in profiles.iter().enumerate() {
        if profile.is_empty() {
            continue;
        }
        let endpoints = profile
            .iter()
            .map(|entity_use| {
                let entity = entities.get(&entity_use.entity).ok_or_else(|| {
                    cadmpeg_ir::codec::CodecError::Malformed(format!(
                        "sketch {} references missing entity {}",
                        sketch.id.0, entity_use.entity.0
                    ))
                })?;
                let generated = generated_sketch_curve(&entity.geometry, sketch, v_axis)?;
                Ok(if entity_use.reversed {
                    (generated.end, generated.start)
                } else {
                    (generated.start, generated.end)
                })
            })
            .collect::<Result<Vec<_>, cadmpeg_ir::codec::CodecError>>()?;
        if endpoints.iter().enumerate().any(|(index, (_, end))| {
            let (next_start, _) = endpoints[(index + 1) % endpoints.len()];
            !same_sketch_point(*end, next_start)
        }) {
            return Err(cadmpeg_ir::codec::CodecError::NotImplemented(format!(
                "source-less SLDPRT sketch profile {profile_index} is not a closed endpoint chain"
            )));
        }
        let loop_id = LoopId(format!("{prefix}:loop:{profile_index}"));
        face_loops.push(loop_id.clone());
        let mut coedge_ids = Vec::new();
        for (use_index, entity_use) in profile.iter().enumerate() {
            let entity = entities.get(&entity_use.entity).ok_or_else(|| {
                cadmpeg_ir::codec::CodecError::Malformed(format!(
                    "sketch {} references missing entity {}",
                    sketch.id.0, entity_use.entity.0
                ))
            })?;
            let generated = generated_sketch_curve(&entity.geometry, sketch, v_axis)?;
            let start_vertex = sketch_vertex(
                &mut ir,
                &mut vertex_by_position,
                &prefix,
                generated.start,
                sketch,
                v_axis,
            );
            let end_vertex = sketch_vertex(
                &mut ir,
                &mut vertex_by_position,
                &prefix,
                generated.end,
                sketch,
                v_axis,
            );
            let start_3d = lift_point(generated.start, sketch.origin, sketch.u_axis, v_axis);
            let end_3d = lift_point(generated.end, sketch.origin, sketch.u_axis, v_axis);
            let delta = Vector3::new(
                end_3d.x - start_3d.x,
                end_3d.y - start_3d.y,
                end_3d.z - start_3d.z,
            );
            let length = (dot(delta, delta)).sqrt();
            if length == 0.0 && matches!(entity.geometry, SketchGeometry::Line { .. }) {
                return Err(cadmpeg_ir::codec::CodecError::Malformed(format!(
                    "sketch entity {} has zero length",
                    entity.id.0
                )));
            }
            let curve_id = CurveId(format!("{prefix}:curve:{profile_index}:{use_index}"));
            let edge_id = EdgeId(format!("{prefix}:edge:{profile_index}:{use_index}"));
            let coedge_id = CoedgeId(format!("{prefix}:coedge:{profile_index}:{use_index}"));
            ir.model.curves.push(Curve {
                id: curve_id.clone(),
                geometry: generated.curve,
                source_object: None,
            });
            ir.model.edges.push(Edge {
                id: edge_id.clone(),
                curve: Some(curve_id),
                start: start_vertex,
                end: end_vertex,
                param_range: Some(generated.param_range.unwrap_or([0.0, length])),
                tolerance: None,
            });
            coedge_ids.push(coedge_id.clone());
            ir.model.coedges.push(Coedge {
                id: coedge_id.clone(),
                owner_loop: loop_id.clone(),
                edge: edge_id,
                next: coedge_id.clone(),
                previous: coedge_id.clone(),
                radial_next: coedge_id,
                sense: if entity_use.reversed {
                    Sense::Reversed
                } else {
                    Sense::Forward
                },
                pcurve: None,
            });
        }
        let count = coedge_ids.len();
        for (index, coedge) in ir
            .model
            .coedges
            .iter_mut()
            .rev()
            .take(count)
            .rev()
            .enumerate()
        {
            coedge.next = coedge_ids[(index + 1) % count].clone();
            coedge.previous = coedge_ids[(index + count - 1) % count].clone();
        }
        ir.model.loops.push(Loop {
            id: loop_id,
            face: face_id.clone(),
            coedges: coedge_ids,
        });
    }
    for (ordinal, entity) in ordered_entities.iter().enumerate() {
        let SketchGeometry::Point { position } = entity.geometry else {
            continue;
        };
        let point_id = PointId(format!("{prefix}:free-point:{ordinal}"));
        let vertex_id = VertexId(format!("{prefix}:free-vertex:{ordinal}"));
        ir.model.points.push(Point {
            id: point_id.clone(),
            position: lift_point(position, sketch.origin, sketch.u_axis, v_axis),
        });
        ir.model.vertices.push(Vertex {
            id: vertex_id.clone(),
            point: point_id,
            tolerance: None,
        });
        let edge_id = EdgeId(format!("{prefix}:point-edge:{ordinal}"));
        let loop_id = LoopId(format!("{prefix}:point-loop:{ordinal}"));
        let coedge_id = CoedgeId(format!("{prefix}:point-coedge:{ordinal}"));
        ir.model.edges.push(Edge {
            id: edge_id.clone(),
            curve: None,
            start: vertex_id.clone(),
            end: vertex_id,
            param_range: None,
            tolerance: None,
        });
        ir.model.coedges.push(Coedge {
            id: coedge_id.clone(),
            owner_loop: loop_id.clone(),
            edge: edge_id,
            next: coedge_id.clone(),
            previous: coedge_id.clone(),
            radial_next: coedge_id.clone(),
            sense: Sense::Forward,
            pcurve: None,
        });
        ir.model.loops.push(Loop {
            id: loop_id.clone(),
            face: face_id.clone(),
            coedges: vec![coedge_id],
        });
        face_loops.push(loop_id);
    }
    if face_loops.is_empty() {
        return Err(cadmpeg_ir::codec::CodecError::NotImplemented(format!(
            "source-less SLDPRT sketch {} has no profiles",
            sketch.id.0
        )));
    }
    ir.model.faces.push(Face {
        id: face_id.clone(),
        shell: shell_id.clone(),
        surface: surface_id,
        sense: Sense::Forward,
        loops: face_loops,
        name: sketch.name.clone(),
        color: None,
        tolerance: None,
    });
    ir.model.shells.push(Shell {
        id: shell_id.clone(),
        region: region_id.clone(),
        faces: vec![face_id],
        wire_edges: Vec::new(),
        free_vertices: Vec::new(),
    });
    ir.model.regions.push(Region {
        id: region_id.clone(),
        body: body_id.clone(),
        shells: vec![shell_id],
    });
    ir.model.bodies.push(Body {
        id: body_id,
        kind: BodyKind::Sheet,
        regions: vec![region_id],
        transform: None,
        name: sketch.name.clone(),
        color: None,
        visible: None,
    });
    ir.model.finalize();
    Ok(ir)
}

struct GeneratedSketchCurve {
    curve: CurveGeometry,
    start: Point2,
    end: Point2,
    param_range: Option<[f64; 2]>,
}

fn generated_sketch_curve(
    geometry: &SketchGeometry,
    sketch: &Sketch,
    v_axis: Vector3,
) -> Result<GeneratedSketchCurve, cadmpeg_ir::codec::CodecError> {
    let lift = |point| lift_point(point, sketch.origin, sketch.u_axis, v_axis);
    let vector = |u: f64, v: f64| {
        Vector3::new(
            sketch.u_axis.x * u + v_axis.x * v,
            sketch.u_axis.y * u + v_axis.y * v,
            sketch.u_axis.z * u + v_axis.z * v,
        )
    };
    match geometry {
        SketchGeometry::Line { start, end } => {
            let origin = lift(*start);
            let target = lift(*end);
            let delta = Vector3::new(
                target.x - origin.x,
                target.y - origin.y,
                target.z - origin.z,
            );
            let length = dot(delta, delta).sqrt();
            if length == 0.0 {
                return Err(cadmpeg_ir::codec::CodecError::Malformed(
                    "source-less SLDPRT sketch contains a zero-length line".into(),
                ));
            }
            Ok(GeneratedSketchCurve {
                curve: CurveGeometry::Line {
                    origin,
                    direction: Vector3::new(
                        delta.x / length,
                        delta.y / length,
                        delta.z / length,
                    ),
                },
                start: *start,
                end: *end,
                param_range: Some([0.0, length]),
            })
        }
        SketchGeometry::Circle { center, radius } => {
            let point = offset_point(*center, Point2::new(radius.0, 0.0));
            Ok(GeneratedSketchCurve {
                curve: CurveGeometry::Circle {
                    center: lift(*center),
                    axis: sketch.normal,
                    ref_direction: sketch.u_axis,
                    radius: radius.0,
                },
                start: point,
                end: point,
                param_range: Some([0.0, std::f64::consts::TAU]),
            })
        }
        SketchGeometry::Arc {
            center,
            radius,
            start_angle,
            end_angle,
        } => Ok(GeneratedSketchCurve {
            curve: CurveGeometry::Circle {
                center: lift(*center),
                axis: sketch.normal,
                ref_direction: sketch.u_axis,
                radius: radius.0,
            },
            start: offset_point(*center, polar(radius.0, start_angle.0)),
            end: offset_point(*center, polar(radius.0, end_angle.0)),
            param_range: Some([start_angle.0, end_angle.0]),
        }),
        SketchGeometry::Ellipse {
            center,
            major_angle,
            major_radius,
            minor_radius,
            start_angle,
            end_angle,
        } => {
            let point = |parameter: f64| {
                Point2::new(
                    center.u + major_angle.0.cos() * major_radius.0 * parameter.cos()
                        - major_angle.0.sin() * minor_radius.0 * parameter.sin(),
                    center.v
                        + major_angle.0.sin() * major_radius.0 * parameter.cos()
                        + major_angle.0.cos() * minor_radius.0 * parameter.sin(),
                )
            };
            let start = start_angle.as_ref().map_or(0.0, |angle| angle.0);
            let end = end_angle
                .as_ref()
                .map_or(std::f64::consts::TAU, |angle| angle.0);
            let full = start_angle.is_none() && end_angle.is_none();
            Ok(GeneratedSketchCurve {
                curve: CurveGeometry::Ellipse {
                    center: lift(*center),
                    axis: sketch.normal,
                    major_direction: vector(major_angle.0.cos(), major_angle.0.sin()),
                    major_radius: major_radius.0,
                    minor_radius: minor_radius.0,
                },
                start: point(start),
                end: if full { point(start) } else { point(end) },
                param_range: Some([start, end]),
            })
        }
        SketchGeometry::Nurbs {
            degree,
            knots,
            control_points,
            weights,
            periodic,
        } => {
            if *periodic || control_points.len() < 2 {
                return Err(cadmpeg_ir::codec::CodecError::NotImplemented(
                    "source-less SLDPRT sketch writing requires a non-periodic NURBS with at least two poles".into(),
                ));
            }
            let start = control_points[0];
            let end = control_points[control_points.len() - 1];
            Ok(GeneratedSketchCurve {
                curve: CurveGeometry::Nurbs(NurbsCurve {
                    degree: *degree,
                    knots: knots.clone(),
                    control_points: control_points.iter().copied().map(lift).collect(),
                    weights: weights.clone(),
                    periodic: false,
                }),
                start,
                end,
                param_range: knots
                    .get(*degree as usize)
                    .zip(knots.get(knots.len().saturating_sub(*degree as usize + 1)))
                    .map(|(start, end)| [*start, *end]),
            })
        }
        SketchGeometry::Point { .. } | SketchGeometry::Native { .. } => Err(
            cadmpeg_ir::codec::CodecError::NotImplemented(
                "source-less SLDPRT sketch writing does not support point or native-only profile entities".into(),
            ),
        ),
    }
}

fn sketch_vertex(
    ir: &mut cadmpeg_ir::CadIr,
    vertices: &mut HashMap<(u64, u64), VertexId>,
    prefix: &str,
    position: Point2,
    sketch: &Sketch,
    v_axis: Vector3,
) -> VertexId {
    if let Some((_, id)) = vertices.iter().find(|((u, v), _)| {
        same_sketch_point(
            Point2::new(f64::from_bits(*u), f64::from_bits(*v)),
            position,
        )
    }) {
        return id.clone();
    }
    let key = (position.u.to_bits(), position.v.to_bits());
    let ordinal = vertices.len();
    let point_id = PointId(format!("{prefix}:point:{ordinal}"));
    let vertex_id = VertexId(format!("{prefix}:vertex:{ordinal}"));
    ir.model.points.push(Point {
        id: point_id.clone(),
        position: lift_point(position, sketch.origin, sketch.u_axis, v_axis),
    });
    ir.model.vertices.push(Vertex {
        id: vertex_id.clone(),
        point: point_id,
        tolerance: None,
    });
    vertices.insert(key, vertex_id.clone());
    vertex_id
}

fn same_sketch_point(left: Point2, right: Point2) -> bool {
    (left.u - right.u).abs() <= SKETCH_POINT_TOLERANCE
        && (left.v - right.v).abs() <= SKETCH_POINT_TOLERANCE
}

fn patch_line_profiles(
    ir: &cadmpeg_ir::CadIr,
    native: &mut crate::native::SldprtNative,
) -> Result<(), cadmpeg_ir::codec::CodecError> {
    let mut requested = HashMap::<(String, usize, u16), Point3>::new();
    let mut curves = Vec::new();
    for sketch in &ir.model.sketches {
        let lane_id = sketch.native_ref.as_ref().ok_or_else(|| {
            cadmpeg_ir::codec::CodecError::NotImplemented(
                "SLDPRT sketch write-back requires native sketch provenance".into(),
            )
        })?;
        let v_axis = cross(sketch.normal, sketch.u_axis);
        for entity in ir
            .model
            .sketch_entities
            .iter()
            .filter(|entity| entity.sketch == sketch.id)
        {
            if entity.endpoint_refs.len() != 2 {
                return Err(cadmpeg_ir::codec::CodecError::Malformed(format!(
                    "SLDPRT sketch entity {} lacks two endpoint references",
                    entity.id.0
                )));
            }
            match &entity.geometry {
                SketchGeometry::Point { position } => {
                    let reference = &entity.endpoint_refs[0];
                    let (stream, attr) = parse_point_ref(reference)?;
                    let point = lift_point(*position, sketch.origin, sketch.u_axis, v_axis);
                    let key = (lane_id.clone(), stream, attr);
                    if let Some(previous) = requested.insert(key, point) {
                        if distance(previous, point) > 1.0e-9 {
                            return Err(cadmpeg_ir::codec::CodecError::Malformed(format!(
                                "SLDPRT shared sketch point {reference} has conflicting positions"
                            )));
                        }
                    }
                }
                SketchGeometry::Line { start, end } => {
                    for (reference, point) in entity.endpoint_refs.iter().zip([start, end]) {
                        let (stream, attr) = parse_point_ref(reference)?;
                        let point = lift_point(*point, sketch.origin, sketch.u_axis, v_axis);
                        let key = (lane_id.clone(), stream, attr);
                        if let Some(previous) = requested.insert(key, point) {
                            if distance(previous, point) > 1.0e-9 {
                                return Err(cadmpeg_ir::codec::CodecError::Malformed(format!(
                                    "SLDPRT shared sketch point {reference} has conflicting positions"
                                )));
                            }
                        }
                    }
                }
                geometry @ (SketchGeometry::Circle { .. }
                | SketchGeometry::Arc { .. }
                | SketchGeometry::Ellipse { .. }
                | SketchGeometry::Nurbs { .. }) => {
                    let geometry_ref = entity.geometry_ref.as_deref().ok_or_else(|| {
                        cadmpeg_ir::codec::CodecError::Malformed(
                            "SLDPRT sketch curve lacks native carrier provenance".into(),
                        )
                    })?;
                    let (stream, carrier_attr) = parse_point_ref(geometry_ref)?;
                    let (_, start_attr) = parse_point_ref(&entity.endpoint_refs[0])?;
                    let (_, end_attr) = parse_point_ref(&entity.endpoint_refs[1])?;
                    if let Some(endpoints) = bounded_endpoints(geometry) {
                        for (reference, point) in entity.endpoint_refs.iter().zip(endpoints) {
                            let (point_stream, attr) = parse_point_ref(reference)?;
                            let point = lift_point(point, sketch.origin, sketch.u_axis, v_axis);
                            let key = (lane_id.clone(), point_stream, attr);
                            if let Some(previous) = requested.insert(key, point) {
                                if distance(previous, point) > 1.0e-9 {
                                    return Err(cadmpeg_ir::codec::CodecError::Malformed(format!(
                                        "SLDPRT shared sketch point {reference} has conflicting positions"
                                    )));
                                }
                            }
                        }
                    }
                    curves.push(CurvePatch {
                        lane_id: lane_id.clone(),
                        stream,
                        carrier_attr,
                        start_attr,
                        end_attr,
                        geometry: geometry.clone(),
                        origin: sketch.origin,
                        u_axis: sketch.u_axis,
                        v_axis,
                    });
                }
                _ => {
                    return Err(cadmpeg_ir::codec::CodecError::NotImplemented(
                        "SLDPRT sketch write-back does not support this curve family".into(),
                    ))
                }
            }
        }
    }
    for ((lane_id, stream_ordinal, attr), point) in requested {
        let lane = native
            .feature_input_lanes
            .iter_mut()
            .find(|lane| lane.id == lane_id)
            .ok_or_else(|| {
                cadmpeg_ir::codec::CodecError::Malformed(format!(
                    "SLDPRT sketch lane {lane_id} is missing"
                ))
            })?;
        patch_direct_stream_point(&mut lane.native_payload, stream_ordinal, attr, point)?;
    }
    for request in curves {
        let lane = native
            .feature_input_lanes
            .iter_mut()
            .find(|lane| lane.id == request.lane_id)
            .ok_or_else(|| {
                cadmpeg_ir::codec::CodecError::Malformed(format!(
                    "SLDPRT sketch lane {} is missing",
                    request.lane_id
                ))
            })?;
        patch_direct_curve(&mut lane.native_payload, &request)?;
    }
    Ok(())
}

fn bounded_endpoints(geometry: &SketchGeometry) -> Option<[Point2; 2]> {
    match geometry {
        SketchGeometry::Arc {
            center,
            radius,
            start_angle,
            end_angle,
        } => Some([
            offset_point(*center, polar(radius.0, start_angle.0)),
            offset_point(*center, polar(radius.0, end_angle.0)),
        ]),
        SketchGeometry::Ellipse {
            center,
            major_angle,
            major_radius,
            minor_radius,
            start_angle: Some(start),
            end_angle: Some(end),
        } => {
            let point = |parameter: f64| {
                Point2::new(
                    center.u + major_angle.0.cos() * major_radius.0 * parameter.cos()
                        - major_angle.0.sin() * minor_radius.0 * parameter.sin(),
                    center.v
                        + major_angle.0.sin() * major_radius.0 * parameter.cos()
                        + major_angle.0.cos() * minor_radius.0 * parameter.sin(),
                )
            };
            Some([point(start.0), point(end.0)])
        }
        SketchGeometry::Nurbs {
            control_points,
            periodic: false,
            ..
        } if control_points.len() >= 2 => {
            Some([control_points[0], control_points[control_points.len() - 1]])
        }
        _ => None,
    }
}

struct CurvePatch {
    lane_id: String,
    stream: usize,
    carrier_attr: u16,
    start_attr: u16,
    end_attr: u16,
    geometry: SketchGeometry,
    origin: Point3,
    u_axis: Vector3,
    v_axis: Vector3,
}

fn parse_point_ref(reference: &str) -> Result<(usize, u16), cadmpeg_ir::codec::CodecError> {
    let (stream, id) = reference.split_once(':').ok_or_else(|| {
        cadmpeg_ir::codec::CodecError::Malformed(format!(
            "invalid SLDPRT sketch endpoint reference {reference}"
        ))
    })?;
    let attr = id.rsplit('#').next().and_then(|value| value.parse().ok());
    match (stream.parse().ok(), attr) {
        (Some(stream), Some(attr)) => Ok((stream, attr)),
        _ => Err(cadmpeg_ir::codec::CodecError::Malformed(format!(
            "invalid SLDPRT sketch endpoint reference {reference}"
        ))),
    }
}

fn lift_point(point: Point2, origin: Point3, u_axis: Vector3, v_axis: Vector3) -> Point3 {
    Point3::new(
        origin.x + point.u * u_axis.x + point.v * v_axis.x,
        origin.y + point.u * u_axis.y + point.v * v_axis.y,
        origin.z + point.u * u_axis.z + point.v * v_axis.z,
    )
}

fn distance(left: Point3, right: Point3) -> f64 {
    (left.x - right.x)
        .hypot(left.y - right.y)
        .hypot(left.z - right.z)
}

fn patch_direct_stream_point(
    payload: &mut Vec<u8>,
    stream_ordinal: usize,
    attr: u16,
    point_mm: Point3,
) -> Result<(), cadmpeg_ir::codec::CodecError> {
    let xyz_m = [point_mm.x * 0.001, point_mm.y * 0.001, point_mm.z * 0.001];
    edit_stream(payload, stream_ordinal, |body| {
        if !crate::brep::patch_point(body, attr, xyz_m) {
            return Err(cadmpeg_ir::codec::CodecError::Malformed(format!(
                "SLDPRT sketch point {attr} is missing"
            )));
        }
        Ok(())
    })
}

fn patch_direct_curve(
    payload: &mut Vec<u8>,
    request: &CurvePatch,
) -> Result<(), cadmpeg_ir::codec::CodecError> {
    edit_stream(payload, request.stream, |body| {
        patch_direct_curve_body(body, request)
    })
}

fn patch_direct_curve_body(
    body: &mut [u8],
    request: &CurvePatch,
) -> Result<(), cadmpeg_ir::codec::CodecError> {
    if matches!(request.geometry, SketchGeometry::Nurbs { .. }) {
        return patch_direct_nurbs(body, request);
    }
    let Some(CurveGeometry::Circle {
        axis,
        ref_direction,
        ..
    }) = crate::brep::curve_by_attr(body, request.carrier_attr)
    else {
        return patch_direct_ellipse(body, request);
    };
    let (center_2d, radius, angles) = match request.geometry {
        SketchGeometry::Circle { center, radius } => (center, radius.0, None),
        SketchGeometry::Arc {
            center,
            radius,
            start_angle,
            end_angle,
        } => (center, radius.0, Some((start_angle.0, end_angle.0))),
        _ => {
            return Err(cadmpeg_ir::codec::CodecError::Malformed(
                "SLDPRT sketch carrier family changed".into(),
            ))
        }
    };
    let center = lift_point(center_2d, request.origin, request.u_axis, request.v_axis);
    let curve = CurveGeometry::Circle {
        center,
        axis,
        ref_direction,
        radius,
    };
    let (_, values) = crate::writer::curve_values(&curve, 0.001)?;
    if !crate::brep::patch_compact_values(body, request.carrier_attr, &values) {
        return Err(cadmpeg_ir::codec::CodecError::Malformed(
            "SLDPRT sketch circle carrier cannot be patched".into(),
        ));
    }
    let endpoints = angles.map_or(
        [offset_point(center_2d, polar(radius, 0.0)); 2],
        |(start, end)| {
            [
                offset_point(center_2d, polar(radius, start)),
                offset_point(center_2d, polar(radius, end)),
            ]
        },
    );
    for (attr, endpoint) in [request.start_attr, request.end_attr]
        .into_iter()
        .zip(endpoints)
    {
        let point = lift_point(endpoint, request.origin, request.u_axis, request.v_axis);
        if !crate::brep::patch_point(
            body,
            attr,
            [point.x * 0.001, point.y * 0.001, point.z * 0.001],
        ) {
            return Err(cadmpeg_ir::codec::CodecError::Malformed(
                "SLDPRT sketch curve endpoint is missing".into(),
            ));
        }
    }
    Ok(())
}

fn edit_stream(
    payload: &mut Vec<u8>,
    stream_ordinal: usize,
    edit: impl FnOnce(&mut [u8]) -> Result<(), cadmpeg_ir::codec::CodecError>,
) -> Result<(), cadmpeg_ir::codec::CodecError> {
    let stream = crate::parasolid::extract_streams(payload)
        .get(stream_ordinal)
        .cloned()
        .ok_or_else(|| {
            cadmpeg_ir::codec::CodecError::Malformed("SLDPRT sketch stream is missing".into())
        })?;
    if let Some(start) = payload
        .windows(stream.len())
        .position(|candidate| candidate == stream.as_slice())
    {
        let header = crate::parasolid::stream_header(&stream).ok_or_else(|| {
            cadmpeg_ir::codec::CodecError::Malformed("invalid retained SLDPRT sketch stream".into())
        })?;
        return edit(&mut payload[start + header.body_offset..start + stream.len()]);
    }
    let (start, end) = compressed_member(payload, &stream).ok_or_else(|| {
        cadmpeg_ir::codec::CodecError::Malformed(
            "compressed retained SLDPRT sketch stream is missing".into(),
        )
    })?;
    let mut inflated = stream;
    let header = crate::parasolid::stream_header(&inflated).ok_or_else(|| {
        cadmpeg_ir::codec::CodecError::Malformed("invalid retained SLDPRT sketch stream".into())
    })?;
    edit(&mut inflated[header.body_offset..])?;
    let mut encoder = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
    encoder.write_all(&inflated)?;
    payload.splice(start..end, encoder.finish()?);
    Ok(())
}

fn compressed_member(payload: &[u8], target: &[u8]) -> Option<(usize, usize)> {
    for start in 0..payload.len().saturating_sub(1) {
        if payload[start] != 0x78 || !matches!(payload[start + 1], 0x01 | 0x9c | 0xda) {
            continue;
        }
        // Cap inflation at `target.len() + 1` bytes: this scan only accepts a member
        // whose inflated body equals `target`, so any stream that expands past the
        // target length can never match and need not be materialized. Bounding the
        // reader here keeps a crafted zlib member from expanding without limit
        // (present-primitive floor; the general uncapped `inflate_zlib_prefix` path
        // still awaits the platform `begin_expand`/`ExpandWriter` API).
        let ceiling = target.len().saturating_add(1);
        let mut decoder = flate2::read::ZlibDecoder::new(&payload[start..]).take(ceiling as u64);
        let mut inflated = Vec::with_capacity(ceiling);
        if decoder.read_to_end(&mut inflated).is_ok() && inflated == target {
            return Some((start, start + decoder.into_inner().total_in() as usize));
        }
    }
    None
}

fn patch_direct_nurbs(
    body: &mut [u8],
    request: &CurvePatch,
) -> Result<(), cadmpeg_ir::codec::CodecError> {
    let SketchGeometry::Nurbs {
        degree,
        ref knots,
        ref control_points,
        ref weights,
        periodic,
    } = request.geometry
    else {
        unreachable!();
    };
    let curve = cadmpeg_ir::geometry::NurbsCurve {
        degree,
        knots: knots.clone(),
        control_points: control_points
            .iter()
            .map(|point| lift_point(*point, request.origin, request.u_axis, request.v_axis))
            .collect(),
        weights: weights.clone(),
        periodic,
    };
    if !crate::brep::patch_nurbs_by_attr(body, request.carrier_attr, &curve) {
        return Err(cadmpeg_ir::codec::CodecError::NotImplemented(
            "SLDPRT sketch NURBS edit changes native storage shape".into(),
        ));
    }
    Ok(())
}

fn patch_direct_ellipse(
    body: &mut [u8],
    request: &CurvePatch,
) -> Result<(), cadmpeg_ir::codec::CodecError> {
    let Some(CurveGeometry::Ellipse { axis, .. }) =
        crate::brep::curve_by_attr(body, request.carrier_attr)
    else {
        return Err(cadmpeg_ir::codec::CodecError::Malformed(
            "SLDPRT sketch analytic carrier is missing".into(),
        ));
    };
    let SketchGeometry::Ellipse {
        center,
        major_angle,
        major_radius,
        minor_radius,
        start_angle,
        end_angle,
    } = request.geometry
    else {
        return Err(cadmpeg_ir::codec::CodecError::Malformed(
            "SLDPRT sketch carrier family changed".into(),
        ));
    };
    let center_3d = lift_point(center, request.origin, request.u_axis, request.v_axis);
    let major_direction = Vector3::new(
        request.u_axis.x * major_angle.0.cos() + request.v_axis.x * major_angle.0.sin(),
        request.u_axis.y * major_angle.0.cos() + request.v_axis.y * major_angle.0.sin(),
        request.u_axis.z * major_angle.0.cos() + request.v_axis.z * major_angle.0.sin(),
    );
    let curve = CurveGeometry::Ellipse {
        center: center_3d,
        axis,
        major_direction,
        major_radius: major_radius.0,
        minor_radius: minor_radius.0,
    };
    let (_, values) = crate::writer::curve_values(&curve, 0.001)?;
    if !crate::brep::patch_compact_values(body, request.carrier_attr, &values) {
        return Err(cadmpeg_ir::codec::CodecError::Malformed(
            "SLDPRT sketch ellipse carrier cannot be patched".into(),
        ));
    }
    let parameters = match (start_angle, end_angle) {
        (Some(start), Some(end)) => [start.0, end.0],
        (None, None) => [0.0, 0.0],
        _ => {
            return Err(cadmpeg_ir::codec::CodecError::Malformed(
                "SLDPRT sketch ellipse has only one bounded endpoint".into(),
            ));
        }
    };
    for (attr, parameter) in [request.start_attr, request.end_attr]
        .into_iter()
        .zip(parameters)
    {
        let local = Point2::new(
            center.u + major_angle.0.cos() * major_radius.0 * parameter.cos()
                - major_angle.0.sin() * minor_radius.0 * parameter.sin(),
            center.v
                + major_angle.0.sin() * major_radius.0 * parameter.cos()
                + major_angle.0.cos() * minor_radius.0 * parameter.sin(),
        );
        let point = lift_point(local, request.origin, request.u_axis, request.v_axis);
        if !crate::brep::patch_point(
            body,
            attr,
            [point.x * 0.001, point.y * 0.001, point.z * 0.001],
        ) {
            return Err(cadmpeg_ir::codec::CodecError::Malformed(
                "SLDPRT sketch ellipse endpoint is missing".into(),
            ));
        }
    }
    Ok(())
}

fn polar(radius: f64, angle: f64) -> Point2 {
    Point2::new(radius * angle.cos(), radius * angle.sin())
}

fn offset_point(origin: Point2, delta: Point2) -> Point2 {
    Point2::new(origin.u + delta.u, origin.v + delta.v)
}
