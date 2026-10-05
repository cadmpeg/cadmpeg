// SPDX-License-Identifier: Apache-2.0
//! Parse exact image-plane bindings owned by Design `Canvas` scopes.

use crate::bytes::lp_utf16_bounded_charged;
use crate::container::ContainerScan;
use crate::design::decode::byte_fields::{bytes_at, zeros_at};
use crate::design::decode::image::{
    embedded_image_asset, embedded_image_entry, neutral_asset_id_charged,
};
use crate::design::decode::record_streams::{has_stream, record_stream};
use crate::design::decode::scopes::shared_frames::{exact_indexed_header_at, marked_reference};
use crate::design::decode::sketch::{indexed_record_header_at, IndexedRecordOffsets};
use crate::design::decode::text::retain_class_tag;

use crate::ids;
use crate::records::{
    canvas::{
        DesignCanvasAsset, DesignCanvasBounds, DesignCanvasGeometry, DesignCanvasGeometryPayload,
        DesignCanvasImage, DesignCanvasPrologue,
    },
    feature::scope::DesignParameterScope,
};
use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::CodecError;
use cadmpeg_ir::assets::Asset;
use cadmpeg_ir::features::{Feature, FeatureDefinition, FeatureOperation};
use cadmpeg_ir::math::Point2;

const DESIGN_LENGTH_TO_MM: f64 = 10.0;

/// Decode every structurally complete Canvas geometry and image-asset record.
pub(crate) fn decode_canvas_images(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan,
    scopes: &[DesignParameterScope],
) -> Result<Vec<DesignCanvasImage>, CodecError> {
    super::image::decode_scoped_images(
        ctx,
        scan,
        scopes,
        &crate::records::feature::scope::DesignFeatureKind::Canvas,
        parse_canvas_image,
        |image| image.id.as_str(),
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
    for image in ctx.admit_iter(images, "scan F3D Canvas images")? {
        let image_stream = record_stream(ctx, &image.id)?;
        let Some(scope) = ctx.find_by(
            scopes,
            |scope| {
                Ok(scope.record_index == image.scope_record_index
                    && has_stream(ctx, &scope.id, image_stream)?)
            },
            "find F3D Canvas image scopes",
        )?
        else {
            continue;
        };
        let (feature_id, _feature_id_storage) = ctx
            .with_scoped_storage("f3d Canvas neutral feature ID", || {
                crate::design::identity::neutral_feature_id(ctx, scope)
            })?;
        let Some(feature_index) = ctx.position_by(
            &*features,
            |feature| {
                ctx.equal_bytes(
                    feature.id.as_str().as_bytes(),
                    feature_id.as_str().as_bytes(),
                    "match F3D Canvas neutral feature",
                )
            },
            "find F3D Canvas neutral feature",
        )?
        else {
            continue;
        };
        let Some(feature) = features.get_mut(feature_index) else {
            continue;
        };
        let (mirror_u, mirror_v) = image.geometry().boundary.mirroring();
        let [minimum, maximum] = image.geometry().boundary.extents();
        let Some(entry) = embedded_image_entry(ctx, scan, image.asset_name())? else {
            continue;
        };
        let asset_id = neutral_asset_id_charged(ctx, &entry.name)?;
        // The image bytes are copied only for the first asset with its ID.
        if !ctx.any_by(
            &assets,
            |candidate: &Asset| {
                ctx.equal_bytes(
                    candidate.id.as_str().as_bytes(),
                    asset_id.as_str().as_bytes(),
                    "match F3D Canvas asset",
                )
            },
            "find F3D Canvas asset",
        )? {
            let id = asset_id.try_clone_for_decode(ctx, "f3d image feature asset identifier")?;
            let asset = embedded_image_asset(ctx, scan, entry, image.asset_name(), id)?;
            ctx.push_vec(&mut assets, asset, "f3d Canvas assets")?;
        }
        let (opacity, frame) = image.geometry().payload.decoded();
        feature
            .evaluation
            .set_definition(FeatureDefinition::Operation(
                FeatureOperation::ReferenceImage {
                    asset: asset_id,
                    visible: image.geometry().prologue.visible(),
                    mirror_u,
                    mirror_v,
                    frame,
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
                        cadmpeg_ir::scalar::Fraction::new(f64::from(opacity)).ok_or_else(|| {
                            CodecError::malformed(
                                "Canvas opacity must be finite and between zero and one",
                            )
                        })?,
                    ),
                },
            ));
    }
    ctx.stable_sort_by(
        &mut assets[..],
        |value| &value.id,
        Ord::cmp,
        "sort f3d design canvas 1",
    )?;
    Ok(assets)
}

/// The fixed members after the prologue of a Canvas geometry record at
/// `geometry_at` whose paired record opens at `paired_at`.
struct CanvasGeometryFrame {
    boundary: DesignCanvasBounds,
    payload: DesignCanvasGeometryPayload,
    plane_entity_suffix: u32,
    component_entity_suffix: u32,
    asset_record_index: u32,
}

/// Offset of the geometry reference in the Canvas scope at `scope_at`: the
/// scope either stores ten zero bytes or nine zero bytes and a marked zero
/// reference before it.
fn geometry_reference_at(bytes: &[u8], scope_at: usize) -> Option<usize> {
    if zeros_at::<10>(bytes, scope_at + 11) {
        Some(scope_at + 21)
    } else if zeros_at::<9>(bytes, scope_at + 11) && marked_reference(bytes, scope_at + 20)? == 0 {
        Some(scope_at + 25)
    } else {
        None
    }
}

/// Read the fixed layout of a Canvas geometry record. The test reads a
/// constant number of bytes.
fn canvas_geometry_frame(
    bytes: &[u8],
    scope_record_index: u32,
    geometry_at: usize,
    paired_at: usize,
) -> Option<CanvasGeometryFrame> {
    let paired_component_at = paired_at + 19;
    if !zeros_at::<8>(bytes, paired_at + 11) {
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
    let scope_reference = marked_reference(bytes, scope_reference_at)?;
    let component_entity_suffix = marked_reference(bytes, component_at)?;
    let asset_record_index = marked_reference(bytes, asset_at)?;
    if scope_reference != scope_record_index
        || marked_reference(bytes, paired_component_at)? != component_entity_suffix
        || !zeros_at::<6>(bytes, paired_component_at + 5)
        || !zeros_at::<6>(bytes, plane_at + 5)
        || !zeros_at::<6>(bytes, scope_reference_at + 5)
        || !zeros_at::<7>(bytes, component_at + 5)
        || !zeros_at::<6>(bytes, asset_at + 5)
        || bytes.get(asset_at + 11) != Some(&1)
    {
        return None;
    }
    let payload =
        DesignCanvasGeometryPayload::try_from(bytes.get(geometry_at + 69..geometry_at + 146)?)
            .ok()?;
    Some(CanvasGeometryFrame {
        boundary,
        payload,
        plane_entity_suffix,
        component_entity_suffix,
        asset_record_index,
    })
}

/// The Canvas image of `scope`. Its geometry record is the first header that
/// carries the geometry record index, and the paired geometry header is the
/// first one with that index at least eleven bytes later.
fn parse_canvas_image(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    stream: &str,
    scope: &DesignParameterScope,
) -> Result<Option<DesignCanvasImage>, CodecError> {
    let Ok(scope_at) = usize::try_from(scope.byte_offset()) else {
        return Ok(None);
    };
    let Some(geometry_reference_at) = geometry_reference_at(bytes, scope_at) else {
        return Ok(None);
    };
    let Some(geometry_record_index) = marked_reference(bytes, geometry_reference_at) else {
        return Ok(None);
    };
    let geometry_offsets = records.offsets(geometry_record_index);
    let Some(geometry) = geometry_offsets
        .first()
        .and_then(|&at| indexed_record_header_at(bytes, at))
    else {
        return Ok(None);
    };
    let geometry_at = geometry.offset;
    let Some(prologue) = bytes_at::<15>(bytes, geometry_at + 11)
        .and_then(|prologue| DesignCanvasPrologue::try_from(*prologue).ok())
    else {
        return Ok(None);
    };
    let paired_position = ctx.partition_point(
        geometry_offsets,
        |offset| Ok(*offset < geometry_at + 11),
        "find F3D Canvas paired geometry header",
    )?;
    let Some(paired) = geometry_offsets
        .get(paired_position)
        .and_then(|&at| indexed_record_header_at(bytes, at))
    else {
        return Ok(None);
    };
    let paired_at = paired.offset;
    let Some(frame) = canvas_geometry_frame(bytes, scope.record_index, geometry_at, paired_at)
    else {
        return Ok(None);
    };
    let asset_record_at = paired_at + 30;
    let Some(asset_class_tag) =
        exact_indexed_header_at(bytes, asset_record_at, frame.asset_record_index)
    else {
        return Ok(None);
    };
    if !zeros_at::<10>(bytes, asset_record_at + 11) {
        return Ok(None);
    }
    let Ok(geometry_reference_offset) = u64::try_from(geometry_reference_at + 1) else {
        return Ok(None);
    };
    let Ok(geometry_offset) = u64::try_from(geometry_at) else {
        return Ok(None);
    };

    let Some((label, after_label)) = lp_utf16_bounded_charged(
        ctx,
        bytes,
        geometry_at + 213,
        1..=256,
        "f3d Design UTF-16 text",
    )?
    else {
        return Ok(None);
    };
    if after_label != paired_at {
        return Ok(None);
    }
    let Some((asset_name, after_asset_name)) = lp_utf16_bounded_charged(
        ctx,
        bytes,
        asset_record_at + 21,
        1..=1024,
        "f3d Design UTF-16 text",
    )?
    else {
        return Ok(None);
    };
    if after_asset_name != scope_at {
        return Ok(None);
    }
    let geometry_class_tag = geometry.retain_class_tag(ctx, "f3d Canvas geometry class tag")?;
    let paired_geometry_class_tag =
        paired.retain_class_tag(ctx, "f3d Canvas paired geometry class tag")?;
    let asset_class_tag = retain_class_tag(ctx, *asset_class_tag, "f3d Canvas asset class tag")?;
    let id = ids::native_scoped_id(ctx, stream, "design-canvas-image", geometry_at)?;

    let Ok(geometry) = DesignCanvasGeometry::new(
        [
            String::from(geometry_class_tag),
            String::from(paired_geometry_class_tag),
        ],
        geometry_record_index,
        geometry_offset,
        label,
        prologue,
        frame.boundary,
        frame.payload,
    ) else {
        return Ok(None);
    };
    let Ok(asset) = DesignCanvasAsset::new(asset_class_tag, frame.asset_record_index, asset_name)
    else {
        return Ok(None);
    };
    Ok(DesignCanvasImage::new(
        id,
        scope.record_index,
        geometry_reference_offset,
        geometry,
        asset,
        frame.plane_entity_suffix,
        frame.component_entity_suffix,
    )
    .ok())
}

#[cfg(test)]
mod tests {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;

    use crate::records::canvas::DesignCanvasImage;
    use crate::records::feature::scope::DesignParameterScope;

    /// Parse `scope` against a stream index built outside `ctx`.
    fn parse_canvas_image(
        ctx: &DecodeContext<'_>,
        bytes: &[u8],
        stream: &str,
        scope: &DesignParameterScope,
    ) -> Result<Option<DesignCanvasImage>, CodecError> {
        let records = crate::design::test_support::indexed_record_offsets_for_test(bytes);
        super::parse_canvas_image(ctx, bytes, &records, stream, scope)
    }

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
        let scope = serde_json::from_value(serde_json::json!({
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
        }))
        .unwrap();
        (bytes, scope)
    }

    fn assert_class_tag_retained_refusal(limit: u64, operation: &'static str) {
        let (bytes, scope) = fixture();
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = limit;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let error = parse_canvas_image(&ctx, &bytes, "Design/BulkStream.dat", &scope)
            .err()
            .unwrap();
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(refusal)
            if refusal.dimension == ResourceDimension::RetainedBytes
                && refusal.operation == operation)
        );
    }

    #[test]
    fn canvas_geometry_class_tag_refuses_retained_limit() {
        assert_class_tag_retained_refusal(8, "f3d Canvas geometry class tag");
    }

    #[test]
    fn canvas_paired_geometry_class_tag_refuses_retained_limit() {
        assert_class_tag_retained_refusal(11, "f3d Canvas paired geometry class tag");
    }

    #[test]
    fn canvas_asset_class_tag_refuses_retained_limit() {
        assert_class_tag_retained_refusal(14, "f3d Canvas asset class tag");
    }

    #[test]
    fn canvas_label_and_asset_name_refuse_retained_limits() {
        let (bytes, scope) = fixture();
        let image = parse_canvas_image(
            &cadmpeg_test_support::service_decode_context(),
            &bytes,
            "Design/BulkStream.dat",
            &scope,
        )
        .unwrap()
        .unwrap();
        assert_eq!(image.asset_name(), "a.png");
        for limit in [0, 5] {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_retained_bytes = limit;
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
            let error = parse_canvas_image(&ctx, &bytes, "Design/BulkStream.dat", &scope)
                .err()
                .unwrap();
            assert!(
                matches!(error, cadmpeg_core::CodecError::ResourceLimit(refusal)
                if refusal.dimension == ResourceDimension::RetainedBytes
                    && refusal.operation == "f3d Design UTF-16 text")
            );
        }
    }

    #[test]
    fn canvas_paired_header_search_refuses_work_limit_through_optional_parse() {
        let (bytes, scope) = fixture();
        let operation = "find F3D Canvas paired geometry header";
        let error = crate::test_support::resource_refusal_at(
            ResourceDimension::WorkUnits,
            operation,
            0,
            |ctx| parse_canvas_image(ctx, &bytes, "Design/BulkStream.dat", &scope).map(|_| ()),
        );
        assert!(matches!(
            error,
            cadmpeg_core::CodecError::ResourceLimit(refusal)
                if refusal.dimension == ResourceDimension::WorkUnits
                    && refusal.operation == operation
        ));
    }

    #[test]
    fn canvas_projection_refuses_asset_copy_and_output_limits() {
        use cadmpeg_ir::features::{
            Feature, FeatureDefinition, FeatureEvaluation, FeatureOperation, SketchFeatureBinding,
        };
        use std::collections::BTreeMap;
        use std::io::{Cursor, Write};
        use zip::CompressionMethod;

        const ENTRY: &str = "FusionAssetName[Active]/Design1/Images.BlobParts/a.png";
        let (bytes, scope) = fixture();
        let image = parse_canvas_image(
            &cadmpeg_test_support::service_decode_context(),
            &bytes,
            "Design/BulkStream.dat",
            &scope,
        )
        .unwrap()
        .unwrap();
        let feature = || Feature {
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
            let error = crate::test_support::resource_refusal_at(
                ResourceDimension::WorkUnits,
                "find F3D Canvas neutral feature",
                0,
                |ctx| {
                    super::project_canvas_images(
                        ctx,
                        scan,
                        std::slice::from_ref(&scope),
                        std::slice::from_ref(&image),
                        &mut [feature()],
                    )
                },
            );
            assert!(
                matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
                if limit.dimension == ResourceDimension::WorkUnits
                    && limit.operation == "find F3D Canvas neutral feature")
            );
            for (dimension, operation) in [
                (ResourceDimension::RetainedBytes, "f3d asset identifier"),
                (
                    ResourceDimension::RetainedBytes,
                    "f3d image feature asset identifier",
                ),
                (ResourceDimension::RetainedBytes, "f3d embedded image data"),
                (
                    ResourceDimension::MaterializedBytes,
                    "f3d feature identifier",
                ),
                (ResourceDimension::CollectionItems, "f3d Canvas assets"),
            ] {
                let error =
                    crate::test_support::resource_refusal_at(dimension, operation, 0, |ctx| {
                        super::project_canvas_images(
                            ctx,
                            scan,
                            std::slice::from_ref(&scope),
                            std::slice::from_ref(&image),
                            &mut [feature()],
                        )
                    });
                assert!(matches!(
                    error,
                    cadmpeg_core::CodecError::ResourceLimit(failure)
                        if failure.dimension == dimension && failure.operation == operation
                ));
            }
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
