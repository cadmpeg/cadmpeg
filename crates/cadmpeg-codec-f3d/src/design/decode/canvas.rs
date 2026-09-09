// SPDX-License-Identifier: Apache-2.0
//! Parse exact image-plane bindings owned by Design `Canvas` scopes.

use cadmpeg_core::container::ContainerRole;

use crate::bytes::{lp_ascii_filtered, lp_utf16_bounded};
use crate::container::ContainerScan;
use crate::design::decode::image::embedded_image_asset;
use crate::design::decode::sketch::next_indexed_record_offset_with_index;
use crate::ids;
use crate::records::feature::DesignParameterScope;
use crate::records::{
    DesignCanvasAsset, DesignCanvasBounds, DesignCanvasGeometry, DesignCanvasGeometryPayload,
    DesignCanvasImage, DesignCanvasPrologue,
};
use cadmpeg_core::decode::View;
use cadmpeg_core::CodecError;
use cadmpeg_ir::assets::Asset;
use cadmpeg_ir::features::{Feature, FeatureDefinition};
use cadmpeg_ir::math::Point2;

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
                    scope.kind() == crate::records::feature::DesignFeatureKind::Canvas
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
        let (mirror_u, mirror_v) = image.geometry().boundary.mirroring();
        let [minimum, maximum] = image.geometry().boundary.extents();
        let Some(asset) = embedded_image_asset(scan, image.asset_name())? else {
            continue;
        };
        let asset_id = asset.id.clone();
        if !assets
            .iter()
            .any(|candidate: &Asset| candidate.id == asset_id)
        {
            assets.push(asset);
        }
        let (opacity, origin, u_axis, v_axis) = image.geometry().payload.decoded();
        feature
            .evaluation
            .set_definition(FeatureDefinition::ReferenceImage {
                asset: asset_id,
                visible: image.geometry().prologue.visible(),
                mirror_u,
                mirror_v,
                frame: cadmpeg_ir::features::FeatureUnitPlaneFrame::new(origin, u_axis, v_axis)
                    .ok_or_else(|| {
                        CodecError::malformed(
                        "Canvas frame must have finite origin and perpendicular unit directions",
                    )
                    })?,
                bounds: cadmpeg_ir::features::FeatureImageBounds::new([
                    Point2::new(
                        minimum.u * DESIGN_LENGTH_TO_MM,
                        minimum.v * DESIGN_LENGTH_TO_MM,
                    ),
                    Point2::new(
                        maximum.u * DESIGN_LENGTH_TO_MM,
                        maximum.v * DESIGN_LENGTH_TO_MM,
                    ),
                ])
                .ok_or_else(|| {
                    CodecError::malformed(
                        "Canvas bounds must have finite corners and nonzero extents",
                    )
                })?,
                opacity: Some(
                    cadmpeg_ir::features::Fraction::new(f64::from(opacity)).ok_or_else(|| {
                        CodecError::malformed(
                            "Canvas opacity must be finite and between zero and one",
                        )
                    })?,
                ),
            })
            .map_err(cadmpeg_core::CodecError::malformed)?;
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
    let geometry_prologue = DesignCanvasPrologue::try_from(geometry_prologue).ok()?;
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
    let boundary = DesignCanvasBounds::try_from(boundary_segments).ok()?;

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
    let geometry_payload = DesignCanvasGeometryPayload::try_from(geometry_payload).ok()?;

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

    DesignCanvasImage::new(
        ids::native_design_canvas_image_id(stream, geometry_at),
        scope.record_index,
        u64::try_from(geometry_reference_at + 1).ok()?,
        DesignCanvasGeometry::new(
            [geometry_class_tag, paired_geometry_class_tag],
            geometry_record_index,
            u64::try_from(geometry_at).ok()?,
            label,
            geometry_prologue,
            boundary,
            geometry_payload,
        )
        .ok()?,
        DesignCanvasAsset::new(
            asset_class_tag.try_into().ok()?,
            asset_record_index,
            asset_name,
        )
        .ok()?,
        plane_entity_suffix,
        component_entity_suffix,
    )
    .ok()
}

fn marked_reference(bytes: &[u8], at: usize) -> Option<u32> {
    (bytes.get(at) == Some(&1)).then(|| View::u32_le_at(bytes, at + 1))?
}
