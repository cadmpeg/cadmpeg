// SPDX-License-Identifier: Apache-2.0
//! Parse exact image-plane bindings owned by Design `Canvas` scopes.

use crate::bytes::lp_ascii_filtered_view;
use crate::bytes::lp_utf16_bounded;
use crate::container::ContainerScan;
use crate::design::decode::image::embedded_image_asset;
use crate::design::decode::scopes::shared_frames::marked_reference;
use crate::design::decode::sketch::next_indexed_record_offset_with_index;

use crate::ids;
use crate::records::{
    canvas::{
        geometry_layout::DesignCanvasGeometryLayout, DesignCanvasAsset, DesignCanvasBounds,
        DesignCanvasGeometry, DesignCanvasGeometryPayload, DesignCanvasImage, DesignCanvasPrologue,
    },
    feature::scope::DesignParameterScope,
};
use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::CodecError;
use cadmpeg_ir::assets::Asset;
use cadmpeg_ir::features::{Feature, FeatureDefinition, FeatureOperation};
use cadmpeg_ir::math::Point2;

/// Decode every structurally complete Canvas geometry and image-asset record.
/// Each image is parsed inside its record scope, whose extent is the
/// geometry record's primary-index frame, and its source offsets are
/// `record_layout` pointers in `pointers`.
pub(crate) fn decode_canvas_images(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan,
    scopes: &[DesignParameterScope],
    pointers: &mut crate::design::decode::sketch::SourcePointers,
) -> Result<Vec<DesignCanvasImage>, CodecError> {
    super::image::decode_scoped_image_records(
        ctx,
        scan,
        scopes,
        &crate::records::feature::scope::DesignFeatureKind::Canvas,
        pointers,
        |bytes, stream, scope| {
            let start = canvas_geometry_at(bytes, scope)?;
            // With no primary index, the geometry, paired and asset records
            // run from the geometry record to the scope.
            let end = usize::try_from(scope.byte_offset()?)
                .ok()
                .filter(|end| *end > start)?;
            Some(super::image::ImageRecord {
                id: ids::native_design_canvas_image_id(stream, start),
                start,
                end,
            })
        },
        parse_canvas_image,
        |image| image.source_offsets(),
    )
}

/// Project uniquely bound Canvas images into neutral raster resources and
/// model-space reference-image features.
pub(crate) fn project_canvas_images(
    ctx: &DecodeContext<'_>,
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
        let feature_id = crate::ids::neutral_feature_id(scope);
        let Some(feature) = features.iter_mut().find(|feature| feature.id == feature_id) else {
            continue;
        };
        let Some(asset) = embedded_image_asset(ctx, scan, image.asset_name())? else {
            continue;
        };
        let asset_id = asset.id.clone();
        if !assets
            .iter()
            .any(|candidate: &Asset| candidate.id == asset_id)
        {
            assets.push(asset);
        }
        let (opacity, plane) = image.geometry().payload.decoded();
        let (frame, bounds) = image
            .geometry()
            .boundary
            .image_placement(plane)
            .map_err(CodecError::malformed)?;
        feature
            .evaluation
            .set_definition(FeatureDefinition::Operation(
                FeatureOperation::ReferenceImage {
                    asset: asset_id,
                    visible: image.geometry().prologue.visible(),
                    mirror_u: false,
                    mirror_v: false,
                    frame,
                    bounds,
                    opacity: Some(Into::into(
                        cadmpeg_ir::scalar::Fraction::new(f64::from(opacity)).ok_or_else(|| {
                            CodecError::malformed(
                                "Canvas opacity must be finite and between zero and one",
                            )
                        })?,
                    )),
                },
            ));
    }
    assets.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(assets)
}

/// The geometry reference of a Canvas scope prefix: after ten zero bytes,
/// or after nine zero bytes and a marked null reference.
fn canvas_geometry_reference<'a>(
    scope_at: usize,
    read: &super::image::ImageRead<'a, '_>,
) -> Option<(usize, u32)> {
    let zeros = read(scope_at + 11..scope_at + 21)?;
    let geometry_reference_at = if zeros == [0; 10] {
        scope_at + 21
    } else if zeros.get(..9)? == [0; 9]
        && marked_reference(read(scope_at + 20..scope_at + 25)?, 0)? == 0
    {
        scope_at + 25
    } else {
        return None;
    };
    let index = marked_reference(read(geometry_reference_at..geometry_reference_at + 5)?, 0)?;
    Some((geometry_reference_at, index))
}

/// The geometry record of a Canvas scope: the indexed header that carries
/// the record index the scope prefix references. The indexed-header-shaped
/// runs locate it (Appendix C `scan_recovery` of `f3d.bulk_class_members`
/// for `design_canvas_images`).
fn canvas_geometry_at(bytes: &[u8], scope: &DesignParameterScope) -> Option<usize> {
    let scope_at = usize::try_from(scope.byte_offset()?).ok()?;
    let (_, index) = canvas_geometry_reference(scope_at, &|range| bytes.get(range))?;
    next_indexed_record_offset_with_index(bytes, 0, index)
}

/// Parse one Canvas image. `bytes` is the stream, whose indexed-header-shaped
/// runs locate the geometry and paired records; `read` returns the stream
/// bytes of a range, and the grammar reads the scope prefix and the records
/// from the geometry record to the scope through it.
fn parse_canvas_image(
    bytes: &[u8],
    stream: &str,
    scope: &DesignParameterScope,
    read: &super::image::ImageRead<'_, '_>,
) -> Result<Option<DesignCanvasImage>, CodecError> {
    let parsed = (|| {
        let scope_at = usize::try_from(scope.byte_offset()?).ok()?;
        let (geometry_reference_at, geometry_record_index) =
            canvas_geometry_reference(scope_at, read)?;
        let geometry_at = next_indexed_record_offset_with_index(bytes, 0, geometry_record_index)?;
        // The geometry, paired and asset records run from the geometry
        // record to the scope.
        if geometry_at >= scope_at {
            return None;
        }
        let records = CanvasRecords {
            stream,
            scope,
            geometry_reference_at,
            geometry_record_index,
            geometry_at,
            records: read(geometry_at..scope_at)?,
        };
        let mut matches = DesignCanvasGeometryLayout::ALL
            .into_iter()
            .filter_map(|layout| parse_canvas_layout(&records, layout));
        let image = matches.next();
        if matches.next().is_none() {
            image
        } else {
            None
        }
    })();
    Ok(parsed)
}

pub(in crate::design::decode) fn typed_canvas_reference(
    bytes: &[u8],
    at: usize,
    type_guid: &str,
) -> Option<u32> {
    canvas_reference(
        bytes,
        at,
        DesignCanvasGeometryLayout::TypedLegacy,
        type_guid,
    )
}

fn canvas_reference(
    bytes: &[u8],
    at: usize,
    layout: DesignCanvasGeometryLayout,
    type_guid: &str,
) -> Option<u32> {
    let target = marked_reference(bytes, at)?;
    let width = layout.reference_len();
    if bytes.get(at + 5..at + 9)? != [0; 4] || bytes.get(at + width - 2..at + width)? != [0; 2] {
        return None;
    }
    if layout.typed() {
        let (guid, end) = lp_ascii_filtered_view(bytes, at + 9, 36..=36, u8::is_ascii_graphic)?;
        if !guid.eq_ignore_ascii_case(type_guid) || end != at + 49 {
            return None;
        }
    }
    Some(target)
}

/// The located records of one Canvas scope: the geometry, paired and asset
/// records from the geometry record to the scope, read through the scope.
struct CanvasRecords<'a> {
    stream: &'a str,
    scope: &'a DesignParameterScope,
    geometry_reference_at: usize,
    geometry_record_index: u32,
    geometry_at: usize,
    records: &'a [u8],
}

/// Parse the located Canvas records in one geometry layout. Offsets are
/// relative to the geometry record.
fn parse_canvas_layout(
    located: &CanvasRecords<'_>,
    layout: DesignCanvasGeometryLayout,
) -> Option<DesignCanvasImage> {
    let records = located.records;
    let geometry_record_index = located.geometry_record_index;
    let offsets = layout.offsets();
    let (geometry_class_tag, after_geometry_tag) =
        lp_ascii_filtered_view(records, 0, 0..=2000, u8::is_ascii_graphic)?;
    let geometry_prologue =
        DesignCanvasPrologue::from_layout(layout, records.get(11..offsets.first_boundary)?).ok()?;
    if View::u32_le_at(records, after_geometry_tag)? != geometry_record_index {
        return None;
    }

    let paired_at = next_indexed_record_offset_with_index(records, 11, geometry_record_index)?;
    let (paired_geometry_class_tag, after_paired_tag) =
        lp_ascii_filtered_view(records, paired_at, 0..=2000, u8::is_ascii_graphic)?;
    let paired_component_at = paired_at + 19;
    if View::u32_le_at(records, after_paired_tag)? != geometry_record_index
        || records.get(paired_at + 11..paired_component_at)? != [0; 8]
    {
        return None;
    }

    let boundary_offsets = [
        offsets.first_boundary,
        offsets.first_boundary + 8,
        offsets.first_boundary + 16,
        offsets.first_boundary + 24,
        offsets.second_boundary,
        offsets.second_boundary + 8,
        offsets.second_boundary + 16,
        offsets.second_boundary + 24,
    ];
    let mut coordinates = [0.0; 8];
    for (coordinate, offset) in coordinates.iter_mut().zip(boundary_offsets) {
        *coordinate = View::f64_le_at(records, offset)?;
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

    let plane_at = offsets.plane;
    let scope_reference_at = offsets.scope;
    let component_at = offsets.component;
    let asset_at = offsets.asset;
    let plane_entity_suffix = canvas_reference(
        records,
        plane_at,
        layout,
        "8D424E29-DE23-4B1F-A3AD-84C52A497D1A",
    )?;
    let scope_record_index = canvas_reference(
        records,
        scope_reference_at,
        layout,
        "0EE4A83E-464B-4235-A62F-984F15834C27",
    )?;
    let component_entity_suffix = canvas_reference(
        records,
        component_at,
        layout,
        "8EF00D65-F18D-4179-A644-FE0CA39943D6",
    )?;
    let asset_record_index = canvas_reference(
        records,
        asset_at,
        layout,
        "830A2A2B-0AA9-4D6A-ACA1-F7A2B2A06573",
    )?;
    let expected_state = u8::from(layout != DesignCanvasGeometryLayout::Legacy);
    if scope_record_index != located.scope.record_index
        || canvas_reference(
            records,
            paired_component_at,
            layout,
            "8EF00D65-F18D-4179-A644-FE0CA39943D6",
        )? != component_entity_suffix
        || records
            .get(component_at + layout.reference_len()..asset_at)?
            .iter()
            .any(|byte| *byte != 0)
        || records.get(offsets.state) != Some(&expected_state)
    {
        return None;
    }
    if layout == DesignCanvasGeometryLayout::Extra12 {
        let extension = plane_at + layout.reference_len();
        if View::u32_le_at(records, extension)? != 0
            || View::u32_le_at(records, extension + 4)? != 1
            || View::u32_le_at(records, extension + 8)? != 0
        {
            return None;
        }
    }
    let geometry_payload = records.get(offsets.payload..scope_reference_at)?;
    let geometry_payload = DesignCanvasGeometryPayload::try_from(geometry_payload).ok()?;

    let (label, after_label) = lp_utf16_bounded(records, offsets.label, 1..=256)?;
    if after_label != paired_at {
        return None;
    }
    let asset_record_at = paired_at.checked_add(usize::try_from(layout.paired_len()).ok()?)?;
    let (asset_class_tag, after_asset_tag) =
        lp_ascii_filtered_view(records, asset_record_at, 0..=2000, u8::is_ascii_graphic)?;
    if View::u32_le_at(records, after_asset_tag)? != asset_record_index
        || records.get(asset_record_at + 11..asset_record_at + 21)? != [0; 10]
    {
        return None;
    }
    let (asset_name, after_asset_name) = lp_utf16_bounded(records, asset_record_at + 21, 1..=1024)?;
    if after_asset_name != records.len() {
        return None;
    }
    let geometry_class_tag = geometry_class_tag.to_owned();
    let paired_geometry_class_tag = paired_geometry_class_tag.to_owned();
    let asset_class_tag = asset_class_tag.to_owned();

    DesignCanvasImage::new(
        ids::native_design_canvas_image_id(located.stream, located.geometry_at),
        located.scope.record_index,
        u64::try_from(located.geometry_reference_at + 1).ok()?,
        DesignCanvasGeometry::new(
            [geometry_class_tag, paired_geometry_class_tag],
            geometry_record_index,
            u64::try_from(located.geometry_at).ok()?,
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

#[cfg(test)]
mod tests {
    use super::parse_canvas_image;

    fn header(bytes: &mut [u8], at: usize, tag: [u8; 3], index: u32) {
        bytes[at..at + 4].copy_from_slice(&3u32.to_le_bytes());
        bytes[at + 4..at + 7].copy_from_slice(&tag);
        bytes[at + 7..at + 11].copy_from_slice(&index.to_le_bytes());
    }

    fn reference(bytes: &mut [u8], at: usize, index: u32) {
        bytes[at] = 1;
        bytes[at + 1..at + 5].copy_from_slice(&index.to_le_bytes());
    }

    fn fixture() -> (
        Vec<u8>,
        crate::records::feature::scope::DesignParameterScope,
    ) {
        let mut bytes = vec![0; 320];
        header(&mut bytes, 0, *b"300", 10);
        header(&mut bytes, 219, *b"301", 10);
        header(&mut bytes, 249, *b"302", 20);
        header(&mut bytes, 284, *b"303", 30);
        for (at, value) in [
            (26, 0.0f64),
            (34, 0.0),
            (42, 1.0),
            (50, 0.0),
            (181, 0.0),
            (189, 1.0),
            (197, 1.0),
            (205, 1.0),
            (98, 1.0),
            (130, 1.0),
        ] {
            bytes[at..at + 8].copy_from_slice(&value.to_le_bytes());
        }
        bytes[69..73].copy_from_slice(&1.0f32.to_le_bytes());
        reference(&mut bytes, 58, 40);
        reference(&mut bytes, 146, 30);
        reference(&mut bytes, 157, 50);
        reference(&mut bytes, 169, 20);
        bytes[180] = 1;
        bytes[213..217].copy_from_slice(&1u32.to_le_bytes());
        bytes[217..219].copy_from_slice(&u16::from(b'L').to_le_bytes());
        reference(&mut bytes, 238, 50);
        bytes[270..274].copy_from_slice(&5u32.to_le_bytes());
        for (ordinal, unit) in "a.png".encode_utf16().enumerate() {
            let at = 274 + ordinal * 2;
            bytes[at..at + 2].copy_from_slice(&unit.to_le_bytes());
        }
        reference(&mut bytes, 305, 10);
        let scope =
            crate::records::feature::test_support::located_scope_from_fixture(serde_json::json!({
                "id": "f3d:Design/BulkStream.dat:scope#30",
                "byte_offset": 284,
                "class_tag": "303",
                "record_index": 30,
                "frame_length": 216,
                "kind": "Canvas",
                "kind_offset": 316,
                "feature_ordinal": 1,
                "feature_ordinal_offset": 428,
                "history_state_id": 8,
                "history_state_id_offset": 308,
                "previous_history_state_id": 7,
                "previous_history_state_id_offset": 458,
                "reference_count_offset": 293,
                "reference_members": [10],
                "reference_member_offsets": [298],
                "paired_class_tag": "304",
                "paired_byte_offset": 500
            }));
        (bytes, scope)
    }

    #[test]
    fn canvas_layouts_preserve_geometry_and_reject_invalid_fields() {
        let (modern, scope) = fixture();
        for (
            visibility,
            first,
            plane,
            payload,
            scope_ref,
            component,
            asset_ref,
            state,
            second,
            label,
            width,
        ) in [
            (25, 26, 58, 69, 146, 157, 169, 180, 181, 213, 11),
            (23, 24, 56, 67, 144, 155, 166, 177, 178, 210, 11),
            (24, 25, 57, 68, 145, 156, 168, 179, 180, 212, 11),
            (25, 26, 58, 81, 158, 169, 181, 192, 193, 225, 11),
            (23, 24, 56, 107, 184, 235, 286, 337, 338, 370, 51),
            (24, 25, 57, 108, 185, 236, 288, 339, 340, 372, 51),
        ] {
            let paired = label + 6;
            let asset = paired + 19 + width;
            let scope_at = asset + 35;
            let mut bytes = vec![0; scope_at + 36];
            header(&mut bytes, 0, *b"300", 10);
            header(&mut bytes, paired, *b"301", 10);
            header(&mut bytes, asset, *b"302", 20);
            header(&mut bytes, scope_at, *b"303", 30);
            if visibility == 24 {
                bytes[21] = 1;
            }
            bytes[visibility] = 1;
            bytes[first..first + 32].copy_from_slice(&modern[26..58]);
            bytes[second..second + 32].copy_from_slice(&modern[181..213]);
            bytes[payload..payload + 77].copy_from_slice(&modern[69..146]);
            for (at, index, guid) in [
                (plane, 40, "8D424E29-DE23-4B1F-A3AD-84C52A497D1A"),
                (scope_ref, 30, "0EE4A83E-464B-4235-A62F-984F15834C27"),
                (component, 50, "8EF00D65-F18D-4179-A644-FE0CA39943D6"),
                (asset_ref, 20, "830A2A2B-0AA9-4D6A-ACA1-F7A2B2A06573"),
                (paired + 19, 50, "8EF00D65-F18D-4179-A644-FE0CA39943D6"),
            ] {
                reference(&mut bytes, at, index);
                if width == 51 {
                    bytes[at + 9..at + 13].copy_from_slice(&36u32.to_le_bytes());
                    bytes[at + 13..at + 49].copy_from_slice(guid.as_bytes());
                }
            }
            if payload == 81 {
                bytes[73..77].copy_from_slice(&1u32.to_le_bytes());
            }
            bytes[state] = u8::from(label != 210);
            bytes[label..label + 6].copy_from_slice(&modern[213..219]);
            bytes[asset + 21..scope_at].copy_from_slice(&modern[270..284]);
            reference(&mut bytes, scope_at + 21, 10);
            let mut pointers = crate::records::source_offsets::collect(&mut scope.clone());
            for offset in pointers.values_mut() {
                *offset = *offset + u64::try_from(scope_at).unwrap() - 284;
            }
            let mut scope = scope.clone();
            crate::records::source_offsets::restore(&mut scope, &pointers);
            let parse = |bytes: &[u8]| {
                parse_canvas_image(bytes, "Design/BulkStream.dat", &scope, &|range| {
                    bytes.get(range)
                })
                .unwrap()
            };
            let image = parse(&bytes).unwrap();
            assert_eq!(image.asset_name(), "a.png");
            assert!(image.geometry().prologue.visible());
            let offsets: std::collections::BTreeMap<_, _> =
                image.source_offsets().into_iter().collect();
            assert_eq!(
                offsets["paired_geometry_byte_offset"],
                Some(u64::try_from(paired).unwrap())
            );
            assert_eq!(
                offsets["asset_byte_offset"],
                Some(u64::try_from(asset).unwrap())
            );
            assert_eq!(
                offsets["plane_reference_offset"],
                Some(u64::try_from(plane + 1).unwrap())
            );
            let wire = serde_json::to_value(&image).unwrap();
            let roundtrip: crate::records::canvas::DesignCanvasImage =
                serde_json::from_value(wire.clone()).unwrap();
            assert_eq!(serde_json::to_value(&roundtrip).unwrap(), wire);
            for offset in [visibility, state, paired + 20, scope_ref + 1, plane + 5] {
                let mut invalid = bytes.clone();
                invalid[offset] = 2;
                assert!(
                    parse(&invalid).is_none(),
                    "label offset {label}, mutation {offset}"
                );
            }
            if width == 51 {
                let mut invalid = bytes.clone();
                invalid[plane + 13] = b'0';
                assert!(parse(&invalid).is_none());
            }
            if payload == 81 {
                bytes[73] = 2;
                assert!(parse(&bytes).is_none());
            }
        }
    }

    #[test]
    fn canvas_image_decodes_asset_name() {
        let (bytes, scope) = fixture();
        let image = parse_canvas_image(&bytes, "Design/BulkStream.dat", &scope, &|range| {
            bytes.get(range)
        })
        .unwrap()
        .unwrap();
        assert_eq!(image.asset_name(), "a.png");
    }

    /// #225 comment 6358238990 item 4 and #214: the Canvas grammar reads the
    /// scope prefix through its geometry reference and the geometry, paired
    /// and asset records from the geometry record (0) to the scope (284)
    /// through the scope's read, so a record scope around the parse captures
    /// exactly those bytes.
    #[test]
    fn canvas_image_reads_its_scope_prefix_and_its_records() {
        let (bytes, scope) = fixture();
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let (ctx, root) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
            &bytes,
            &arena,
            &cadmpeg_core::decode::DecodePolicy::service(),
        )
        .unwrap();
        let read = |range: std::ops::Range<usize>| {
            let view = root.child(range.start, range.end)?;
            view.read_slice(range)
        };
        let extent = root.child(0, 284).unwrap();
        let image = ctx
            .record("f3d:test:canvas", extent, || {
                parse_canvas_image(&bytes, "Design/BulkStream.dat", &scope, &read)
            })
            .unwrap()
            .unwrap();
        assert_eq!(image.asset_name(), "a.png");
        // The scope prefix holds ten zero bytes at 11..21 and the marked
        // geometry reference at 21..26.
        let captures = ctx.take_record_captures().unwrap();
        assert_eq!(captures.len(), 1);
        assert_eq!(
            captures[0].reads(),
            &std::collections::BTreeMap::from([(
                "file".to_owned(),
                vec![0..284, 284 + 11..284 + 26]
            )])
        );
        let offsets: std::collections::BTreeMap<_, _> =
            image.source_offsets().into_iter().collect();
        assert_eq!(offsets["geometry_byte_offset"], Some(0));
        assert_eq!(offsets["geometry_reference_offset"], Some(284 + 22));
        assert_eq!(offsets["boundary_coordinate_offsets.7"], Some(205));
    }

    #[test]
    fn canvas_projection_emits_one_asset() {
        use cadmpeg_ir::features::{
            Feature, FeatureDefinition, FeatureEvaluation, FeatureOperation, SketchFeatureBinding,
        };
        use std::collections::BTreeMap;
        use std::io::{Cursor, Write};
        use zip::CompressionMethod;

        const ENTRY: &str = "FusionAssetName[Active]/Design1/Images.BlobParts/a.png";
        let (bytes, scope) = fixture();
        let image = parse_canvas_image(&bytes, "Design/BulkStream.dat", &scope, &|range| {
            bytes.get(range)
        })
        .unwrap()
        .unwrap();
        let feature = || Feature {
            needs_recompute: false,
            id: crate::ids::neutral_feature_id(&scope),
            ordinal: 0,
            name: None,
            suppressed: None,
            dependencies: cadmpeg_ir::features::DistinctMembers::default(),
            source_properties: BTreeMap::new(),
            source_tag: None,
            source_text: None,
            source_content: cadmpeg_ir::features::FeatureContent::default(),
            evaluation: FeatureEvaluation::from_definition(FeatureDefinition::Operation(
                FeatureOperation::Sketch {
                    sketch: SketchFeatureBinding::Unresolved,
                },
            )),
            native_ref: Some(scope.id.clone()),
        };
        let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
        let stored = crate::zip_write::file_options(CompressionMethod::Stored);
        crate::test_support::manifest_test::write_synthetic_manifests(&mut zip, stored);
        zip.start_file(ENTRY, stored).unwrap();
        zip.write_all(b"PNG").unwrap();
        let archive = zip.finish().unwrap().into_inner();
        crate::test_support::zip_test::with_scan(&archive, |scan| {
            let assets = super::project_canvas_images(
                &cadmpeg_test_support::service_decode_context(),
                scan,
                std::slice::from_ref(&scope),
                std::slice::from_ref(&image),
                &mut [feature()],
            )
            .unwrap();
            assert_eq!(assets.len(), 1);
            assert_eq!(assets[0].id, crate::ids::neutral_asset_id(ENTRY));
        });
    }
}
