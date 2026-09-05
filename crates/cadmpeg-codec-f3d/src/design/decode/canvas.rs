// SPDX-License-Identifier: Apache-2.0
//! Parse exact image-plane bindings owned by Design `Canvas` scopes.

use cadmpeg_core::container::ContainerRole;

use crate::bytes::{lp_ascii_filtered, lp_utf16_bounded};
use crate::container::ContainerScan;
use crate::design::decode::image::embedded_image_asset;
use crate::design::decode::sketch::next_indexed_record_offset_with_index;
use crate::ids;
use crate::records::{DesignCanvasImage, DesignParameterScope};
use cadmpeg_core::CodecError;
use cadmpeg_core::decode::View;
use cadmpeg_ir::assets::Asset;
use cadmpeg_ir::features::{Feature, FeatureDefinition};
use cadmpeg_ir::math::{Point2, Point3, Vector3};

const EPS_CANVAS_DECODE_GEOMETRY_PAYLOAD_E9: f64 = 1.0e-9;

const DESIGN_LENGTH_TO_MM: f64 = 10.0;

/// Decode every structurally complete Canvas geometry and image-asset record.
pub fn decode_canvas_images(
    scan: &ContainerScan,
    scopes: &[DesignParameterScope],
) -> Result<Vec<DesignCanvasImage>, CodecError> {
    let mut images = Vec::new();
    for entry in scan
        .entries
        .iter()
        .filter(|entry| scan.is_design_stream(entry, ContainerRole::Bulkstream))
    {
        let bytes = scan.entry_bytes(&entry.name)?;
        let stream = ids::native_scope(&entry.name);
        images.extend(
            scopes
                .iter()
                .filter(|scope| {
                    scope.kind == crate::records::DesignFeatureKind::Canvas
                        && ids::native_stream(&scope.id) == Some(stream.as_str())
                })
                .filter_map(|scope| parse_canvas_image(bytes, &entry.name, scope)),
        );
    }
    images.sort_by(|a, b| a.id.cmp(&b.id));
    images.dedup_by(|a, b| a.id == b.id);
    Ok(images)
}

/// Project uniquely bound Canvas images into neutral raster resources and
/// model-space reference-image features.
pub fn project_canvas_images(
    scan: &ContainerScan,
    scopes: &[DesignParameterScope],
    images: &[DesignCanvasImage],
    features: &mut [Feature],
) -> Result<Vec<Asset>, CodecError> {
    let mut assets = Vec::new();
    for image in images {
        let Some(scope) = scopes.iter().find(|scope| {
            scope.record_index == image.scope_record_index
                && crate::ids::native_stream(&scope.id) == crate::ids::native_stream(&image.id)
        }) else {
            continue;
        };
        let Some(feature) = features
            .iter_mut()
            .find(|feature| feature.id == crate::ids::neutral_feature_id(scope))
        else {
            continue;
        };
        let Some((mirror_u, mirror_v)) = canvas_mirroring(image.boundary_segments) else {
            continue;
        };
        let mut u_values = image
            .boundary_segments
            .iter()
            .flatten()
            .map(|point| point.u);
        let mut v_values = image
            .boundary_segments
            .iter()
            .flatten()
            .map(|point| point.v);
        let (Some(mut u_min), Some(mut v_min)) = (u_values.next(), v_values.next()) else {
            continue;
        };
        let (mut u_max, mut v_max) = (u_min, v_min);
        for value in u_values {
            u_min = u_min.min(value);
            u_max = u_max.max(value);
        }
        for value in v_values {
            v_min = v_min.min(value);
            v_max = v_max.max(value);
        }
        let Some(asset) = embedded_image_asset(scan, &image.asset_name)? else {
            continue;
        };
        let asset_id = asset.id.clone();
        if !assets
            .iter()
            .any(|candidate: &Asset| candidate.id == asset_id)
        {
            assets.push(asset);
        }
        feature.definition = FeatureDefinition::ReferenceImage {
            asset: asset_id,
            visible: image.visible,
            mirror_u,
            mirror_v,
            origin: image.origin,
            u_axis: image.u_axis,
            v_axis: image.v_axis,
            bounds: [
                Point2::new(u_min * DESIGN_LENGTH_TO_MM, v_min * DESIGN_LENGTH_TO_MM),
                Point2::new(u_max * DESIGN_LENGTH_TO_MM, v_max * DESIGN_LENGTH_TO_MM),
            ],
            opacity: Some(f64::from(image.opacity)),
        };
    }
    assets.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(assets)
}

fn parse_canvas_image(
    bytes: &[u8],
    stream: &str,
    scope: &DesignParameterScope,
) -> Option<DesignCanvasImage> {
    let scope_at = usize::try_from(scope.byte_offset).ok()?;
    let geometry_reference_at = if bytes.get(scope_at + 11..scope_at + 21)? == [0; 10] {
        scope_at + 21
    } else if bytes.get(scope_at + 11..scope_at + 20)? == [0; 9]
        && marked_reference(bytes, scope_at + 20)? == 0
    {
        scope_at + 25
    } else {
        return None;
    };
    let geometry_record_index = marked_reference(bytes, geometry_reference_at)?;
    let geometry_at = next_indexed_record_offset_with_index(bytes, 0, geometry_record_index)?;
    let (geometry_class_tag, after_geometry_tag) =
        lp_ascii_filtered(bytes, geometry_at, 0..=2000, u8::is_ascii_graphic)?;
    let geometry_prologue: [u8; 15] = bytes
        .get(geometry_at + 11..geometry_at + 26)?
        .try_into()
        .ok()?;
    let visible = geometry_prologue_visibility(&geometry_prologue)?;
    if View::u32_le_at(bytes, after_geometry_tag)? != geometry_record_index {
        return None;
    }

    let paired_at = next_indexed_record_offset_with_index(
        bytes,
        geometry_at.checked_add(11)?,
        geometry_record_index,
    )?;
    let (paired_geometry_class_tag, after_paired_tag) =
        lp_ascii_filtered(bytes, paired_at, 0..=2000, u8::is_ascii_graphic)?;
    let paired_component_at = paired_at + 19;
    if View::u32_le_at(bytes, after_paired_tag)? != geometry_record_index
        || paired_at <= geometry_at
        || bytes.get(paired_at + 11..paired_component_at)? != [0; 8]
    {
        return None;
    }

    let boundary_offsets = [
        geometry_at + 26,
        geometry_at + 34,
        geometry_at + 42,
        geometry_at + 50,
        geometry_at + 181,
        geometry_at + 189,
        geometry_at + 197,
        geometry_at + 205,
    ];
    let mut coordinates = [0.0; 8];
    for (coordinate, offset) in coordinates.iter_mut().zip(boundary_offsets) {
        *coordinate = View::f64_le_at(bytes, offset)?;
        if !coordinate.is_finite() {
            return None;
        }
    }
    let boundary_segments = [
        [
            Point2::new(coordinates[0], coordinates[1]),
            Point2::new(coordinates[2], coordinates[3]),
        ],
        [
            Point2::new(coordinates[4], coordinates[5]),
            Point2::new(coordinates[6], coordinates[7]),
        ],
    ];
    canvas_mirroring(boundary_segments)?;

    let plane_at = geometry_at + 58;
    let scope_reference_at = geometry_at + 146;
    let component_at = geometry_at + 157;
    let asset_at = geometry_at + 169;
    let plane_entity_suffix = marked_reference(bytes, plane_at)?;
    let scope_record_index = marked_reference(bytes, scope_reference_at)?;
    let component_entity_suffix = marked_reference(bytes, component_at)?;
    let asset_record_index = marked_reference(bytes, asset_at)?;
    if scope_record_index != scope.record_index
        || marked_reference(bytes, paired_component_at)? != component_entity_suffix
        || bytes.get(paired_component_at + 5..paired_component_at + 11)? != [0; 6]
        || bytes.get(plane_at + 5..plane_at + 11)? != [0; 6]
        || bytes.get(scope_reference_at + 5..scope_reference_at + 11)? != [0; 6]
        || bytes.get(component_at + 5..component_at + 12)? != [0; 7]
        || bytes.get(asset_at + 5..asset_at + 11)? != [0; 6]
        || bytes.get(asset_at + 11) != Some(&1)
    {
        return None;
    }
    let geometry_payload = bytes.get(geometry_at + 69..geometry_at + 146)?;
    let (opacity, origin, u_axis, v_axis) = decode_geometry_payload(geometry_payload)?;

    let (label, after_label) = lp_utf16_bounded(bytes, geometry_at + 213, 1..=256)?;
    if after_label != paired_at {
        return None;
    }
    let asset_record_at = paired_at.checked_add(30)?;
    let (asset_class_tag, after_asset_tag) =
        lp_ascii_filtered(bytes, asset_record_at, 0..=2000, u8::is_ascii_graphic)?;
    if View::u32_le_at(bytes, after_asset_tag)? != asset_record_index
        || bytes.get(asset_record_at + 11..asset_record_at + 21)? != [0; 10]
    {
        return None;
    }
    let (asset_name, after_asset_name) = lp_utf16_bounded(bytes, asset_record_at + 21, 1..=1024)?;
    if after_asset_name != scope_at {
        return None;
    }

    Some(DesignCanvasImage {
        id: ids::native_design_canvas_image_id(stream, geometry_at),
        scope_record_index: scope.record_index,
        scope_reference_offset: u64::try_from(scope_reference_at + 1).ok()?,
        geometry_class_tag,
        geometry_record_index,
        geometry_reference_offset: u64::try_from(geometry_reference_at + 1).ok()?,
        geometry_byte_offset: u64::try_from(geometry_at).ok()?,
        geometry_prologue,
        visible,
        visibility_offset: u64::try_from(geometry_at + 25).ok()?,
        geometry_frame_length: u64::try_from(paired_at.checked_sub(geometry_at)?).ok()?,
        paired_geometry_class_tag,
        paired_geometry_byte_offset: u64::try_from(paired_at).ok()?,
        paired_component_reference_offset: u64::try_from(paired_component_at + 1).ok()?,
        boundary_segments,
        boundary_coordinate_offsets: boundary_offsets.map(|offset| offset as u64),
        second_boundary_present_offset: u64::try_from(asset_at + 11).ok()?,
        plane_entity_suffix,
        plane_reference_offset: u64::try_from(plane_at + 1).ok()?,
        component_entity_suffix,
        component_reference_offset: u64::try_from(component_at + 1).ok()?,
        asset_class_tag,
        asset_record_index,
        asset_reference_offset: u64::try_from(asset_at + 1).ok()?,
        asset_byte_offset: u64::try_from(asset_record_at).ok()?,
        asset_name,
        asset_name_offset: u64::try_from(asset_record_at + 25).ok()?,
        label,
        label_offset: u64::try_from(geometry_at + 217).ok()?,
        opacity,
        origin,
        u_axis,
        v_axis,
        geometry_payload: geometry_payload.to_vec(),
    })
}

fn decode_geometry_payload(payload: &[u8]) -> Option<(f32, Point3, Vector3, Vector3)> {
    let opacity = View::f32_le_at(payload, 0)?;
    if !opacity.is_finite() || !(0.0..=1.0).contains(&opacity) || payload.get(4) != Some(&0) {
        return None;
    }
    let vector = |offset| {
        Some([
            View::f64_le_at(payload, offset)?,
            View::f64_le_at(payload, offset + 8)?,
            View::f64_le_at(payload, offset + 16)?,
        ])
    };
    let origin = vector(5)?;
    let u = vector(29)?;
    let v = vector(53)?;
    let u_axis = Vector3::new(u[0], u[1], u[2]);
    let v_axis = Vector3::new(v[0], v[1], v[2]);
    if !origin.into_iter().all(f64::is_finite)
        || (u_axis.norm() - 1.0).abs() > EPS_CANVAS_DECODE_GEOMETRY_PAYLOAD_E9
        || (v_axis.norm() - 1.0).abs() > EPS_CANVAS_DECODE_GEOMETRY_PAYLOAD_E9
        || u_axis.dot(v_axis).abs() > EPS_CANVAS_DECODE_GEOMETRY_PAYLOAD_E9
    {
        return None;
    }
    Some((
        opacity,
        Point3::new(
            origin[0] * DESIGN_LENGTH_TO_MM,
            origin[1] * DESIGN_LENGTH_TO_MM,
            origin[2] * DESIGN_LENGTH_TO_MM,
        ),
        u_axis,
        v_axis,
    ))
}

fn marked_reference(bytes: &[u8], at: usize) -> Option<u32> {
    (bytes.get(at) == Some(&1)).then(|| View::u32_le_at(bytes, at + 1))?
}

pub(crate) fn valid_geometry_prologue(prologue: &[u8; 15]) -> bool {
    geometry_prologue_visibility(prologue).is_some()
}

pub(crate) fn geometry_prologue_visibility(prologue: &[u8; 15]) -> Option<bool> {
    (prologue[..10] == [0; 10]
        && matches!(prologue[10], 0 | 1)
        && prologue[11..14] == [0; 3]
        && matches!(prologue[14], 0 | 1))
    .then_some(prologue[14] != 0)
}

pub(crate) fn canvas_mirroring(segments: [[Point2; 2]; 2]) -> Option<(bool, bool)> {
    let [[a, b], [c, d]] = segments;
    let close = |left: f64, right: f64| {
        (left - right).abs() <= 64.0 * f64::EPSILON * left.abs().max(right.abs()).max(1.0)
    };
    let horizontal = close(a.v, b.v)
        && close(c.v, d.v)
        && close(a.u, c.u)
        && close(b.u, d.u)
        && !close(a.v, c.v);
    let vertical = close(a.u, b.u)
        && close(c.u, d.u)
        && close(a.v, c.v)
        && close(b.v, d.v)
        && !close(a.u, c.u);
    if horizontal {
        Some((a.u > b.u, a.v > c.v))
    } else if vertical {
        Some((a.u > c.u, a.v > b.v))
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::{
        canvas_mirroring, decode_geometry_payload, geometry_prologue_visibility,
        valid_geometry_prologue,
    };
    use cadmpeg_ir::math::{Point2, Point3, Vector3};

    #[test]
    fn canvas_geometry_payload_decodes_opacity_and_plane_frame() {
        let mut payload = [0; 77];
        payload[..4].copy_from_slice(&0.75f32.to_le_bytes());
        for (offset, value) in [
            (5, 1.0f64),
            (13, 2.0),
            (21, 3.0),
            (29, 1.0),
            (37, 0.0),
            (45, 0.0),
            (53, 0.0),
            (61, 0.0),
            (69, 1.0),
        ] {
            payload[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
        }

        assert_eq!(
            decode_geometry_payload(&payload),
            Some((
                0.75,
                Point3::new(10.0, 20.0, 30.0),
                Vector3::new(1.0, 0.0, 0.0),
                Vector3::new(0.0, 0.0, 1.0),
            ))
        );

        payload[4] = 1;
        assert!(decode_geometry_payload(&payload).is_none());
        payload[4] = 0;
        payload[53..61].copy_from_slice(&1.0f64.to_le_bytes());
        assert!(decode_geometry_payload(&payload).is_none());
    }

    #[test]
    fn canvas_bounds_decode_u_and_v_mirroring_from_endpoint_order() {
        assert_eq!(
            canvas_mirroring([
                [Point2::new(-2.0, -1.0), Point2::new(3.0, -1.0)],
                [Point2::new(-2.0, 4.0), Point2::new(3.0, 4.0)],
            ]),
            Some((false, false))
        );
        assert_eq!(
            canvas_mirroring([
                [Point2::new(3.0, -1.0), Point2::new(-2.0, -1.0)],
                [Point2::new(3.0, 4.0), Point2::new(-2.0, 4.0)],
            ]),
            Some((true, false))
        );
        assert_eq!(
            canvas_mirroring([
                [Point2::new(-2.0, 4.0), Point2::new(3.0, 4.0)],
                [Point2::new(-2.0, -1.0), Point2::new(3.0, -1.0)],
            ]),
            Some((false, true))
        );
        assert_eq!(
            canvas_mirroring([
                [Point2::new(3.0, 4.0), Point2::new(-2.0, 4.0)],
                [Point2::new(3.0, -1.0), Point2::new(-2.0, -1.0)],
            ]),
            Some((true, true))
        );
        assert_eq!(
            canvas_mirroring([
                [Point2::new(-2.0, 4.0), Point2::new(-2.0, -1.0)],
                [Point2::new(3.0, 4.0), Point2::new(3.0, -1.0)],
            ]),
            Some((false, true))
        );
        assert_eq!(
            canvas_mirroring([
                [Point2::new(3.0, -1.0), Point2::new(3.0, 4.0)],
                [Point2::new(-2.0, -1.0), Point2::new(-2.0, 4.0)],
            ]),
            Some((true, false))
        );
        assert_eq!(
            canvas_mirroring([
                [Point2::new(-2.0, -1.0), Point2::new(3.0, -1.0)],
                [
                    Point2::new(f64::from_bits((-2.0f64).to_bits() + 4), 4.0),
                    Point2::new(f64::from_bits(3.0f64.to_bits() + 4), 4.0),
                ],
            ]),
            Some((false, false))
        );
        assert_eq!(
            canvas_mirroring([
                [Point2::new(-2.0, -1.0), Point2::new(3.0, -1.0)],
                [Point2::new(-2.0, 4.0), Point2::new(2.0, 4.0)],
            ]),
            None
        );
    }

    #[test]
    fn canvas_geometry_prologue_decodes_visibility_in_both_forms() {
        let mut expanded = [0; 15];
        expanded[14] = 1;
        assert!(valid_geometry_prologue(&expanded));
        assert_eq!(geometry_prologue_visibility(&expanded), Some(true));

        expanded[14] = 0;
        assert_eq!(geometry_prologue_visibility(&expanded), Some(false));

        let mut compact = [0; 15];
        compact[10] = 1;
        assert!(valid_geometry_prologue(&compact));
        assert_eq!(geometry_prologue_visibility(&compact), Some(false));

        compact[14] = 1;
        assert_eq!(geometry_prologue_visibility(&compact), Some(true));

        compact[11] = 1;
        assert!(!valid_geometry_prologue(&compact));
    }
}
