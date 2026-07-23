// SPDX-License-Identifier: Apache-2.0
//! Parse exact image-plane bindings owned by Design `Canvas` scopes.

use crate::bytes::{lp_ascii_filtered, lp_utf16_bounded};
use crate::container::{role, ContainerScan};
use crate::design::decode::sketch::next_indexed_record_offset_with_index;
use crate::ids;
use crate::records::{DesignCanvasImage, DesignParameterScope};
use cadmpeg_ir::codec::CodecError;
use cadmpeg_ir::le::{f64_at, u32_at};
use cadmpeg_ir::math::Point2;

/// Decode every structurally complete Canvas geometry and image-asset record.
pub fn decode_canvas_images(
    scan: &ContainerScan,
    scopes: &[DesignParameterScope],
) -> Result<Vec<DesignCanvasImage>, CodecError> {
    let mut images = Vec::new();
    for entry in scan
        .entries
        .iter()
        .filter(|entry| entry.role == role::BULKSTREAM && entry.name.contains("Design"))
    {
        let bytes = scan.entry_bytes(&entry.name)?;
        let stream = ids::native_scope(&entry.name);
        images.extend(
            scopes
                .iter()
                .filter(|scope| {
                    scope.kind == "Canvas" && ids::native_stream(&scope.id) == Some(stream.as_str())
                })
                .filter_map(|scope| parse_canvas_image(bytes, &entry.name, scope)),
        );
    }
    images.sort_by_key(|image| image.id.clone());
    images.dedup_by_key(|image| image.id.clone());
    Ok(images)
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
    if u32_at(bytes, after_geometry_tag)? != geometry_record_index
        || !valid_geometry_prologue(&geometry_prologue)
    {
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
    if u32_at(bytes, after_paired_tag)? != geometry_record_index
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
        *coordinate = f64_at(bytes, offset)?;
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
    if !opposite_rectangle_edges(boundary_segments) {
        return None;
    }

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

    let (label, after_label) = lp_utf16_bounded(bytes, geometry_at + 213, 1..=256)?;
    if after_label != paired_at {
        return None;
    }
    let asset_record_at = paired_at.checked_add(30)?;
    let (asset_class_tag, after_asset_tag) =
        lp_ascii_filtered(bytes, asset_record_at, 0..=2000, u8::is_ascii_graphic)?;
    if u32_at(bytes, after_asset_tag)? != asset_record_index
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
        geometry_payload: bytes.get(geometry_at + 69..geometry_at + 146)?.to_vec(),
    })
}

fn marked_reference(bytes: &[u8], at: usize) -> Option<u32> {
    (bytes.get(at) == Some(&1)).then(|| u32_at(bytes, at + 1))?
}

fn valid_geometry_prologue(prologue: &[u8; 15]) -> bool {
    (prologue[..14] == [0; 14] && prologue[14] == 1)
        || (prologue[..10] == [0; 10] && prologue[10..] == [1, 0, 0, 0, 0])
}

fn opposite_rectangle_edges(segments: [[Point2; 2]; 2]) -> bool {
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
    horizontal || vertical
}

#[cfg(test)]
mod tests {
    use super::{opposite_rectangle_edges, valid_geometry_prologue};
    use cadmpeg_ir::math::Point2;

    #[test]
    fn canvas_bounds_require_two_opposite_non_degenerate_edges() {
        assert!(opposite_rectangle_edges([
            [Point2::new(-2.0, -1.0), Point2::new(3.0, -1.0)],
            [Point2::new(-2.0, 4.0), Point2::new(3.0, 4.0)],
        ]));
        assert!(opposite_rectangle_edges([
            [Point2::new(-2.0, 4.0), Point2::new(-2.0, -1.0)],
            [Point2::new(3.0, 4.0), Point2::new(3.0, -1.0)],
        ]));
        assert!(opposite_rectangle_edges([
            [
                Point2::new(-2.0, 4.0),
                Point2::new(f64::from_bits((-2.0f64).to_bits() + 4), -1.0),
            ],
            [
                Point2::new(3.0, f64::from_bits(4.0f64.to_bits() + 4)),
                Point2::new(f64::from_bits(3.0f64.to_bits() + 4), -1.0),
            ],
        ]));
        assert!(!opposite_rectangle_edges([
            [Point2::new(-2.0, -1.0), Point2::new(3.0, -1.0)],
            [Point2::new(-2.0, 4.0), Point2::new(2.0, 4.0)],
        ]));
    }

    #[test]
    fn canvas_geometry_prologue_accepts_only_expanded_and_compact_forms() {
        let mut expanded = [0; 15];
        expanded[14] = 1;
        assert!(valid_geometry_prologue(&expanded));

        let mut compact = [0; 15];
        compact[10] = 1;
        assert!(valid_geometry_prologue(&compact));

        compact[14] = 1;
        assert!(!valid_geometry_prologue(&compact));
    }
}
