// SPDX-License-Identifier: Apache-2.0

use crate::decode::jpeg::jpeg_dimensions;

use super::*;

#[test]
fn inspect_reports_bounded_nx_object_model_entities() {
    let mut cur = Cursor::new(prt_with_indexed_om_section());
    let summary = NxCodec
        .inspect(&mut cur, &InspectOptions::default())
        .unwrap();
    assert!(summary.notes.iter().any(|note| {
        note == "NX object model: 1 indexed section(s), 2 bounded entity record(s)"
    }));
}

#[test]
fn decode_projects_part_attributes_to_document_attributes() {
    let xml = br#"<?xml version="1.0" encoding="UTF-8"?>
<UgAttributes version="4" xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance">
  <Attribute owner="part" pdmBased="false" utf8title="Material"
    utf8value="Steel" version="3" xsi:type="StringAttributeType"/>
</UgAttributes>"#;
    let file = prt_with_named_payloads(&[
        ("/Root/UG_PART/UG_PART", zlib_compress(&partition_stream())),
        ("/Root/part/attrs", xml.to_vec()),
    ]);
    let mut cur = Cursor::new(file);
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();

    assert_eq!(result.ir().model.attributes.len(), 1);
    let attribute = &result.ir().model.attributes[0];
    assert_eq!(attribute.name, "Material");
    assert_eq!(
        attribute.target,
        cadmpeg_ir::attributes::AttributeTarget::Document
    );
    assert_eq!(
        attribute.values,
        vec![cadmpeg_ir::attributes::AttributeValue::String(
            "Steel".to_string()
        )]
    );
    assert!(cadmpeg_ir::validate::validate_neutral(result.ir(), Vec::new()).is_ok());
}

#[test]
fn decode_exposes_strict_nx_jpeg_preview_metadata() {
    let preview = [
        0xff, 0xd8, 0xff, 0xe0, 0x00, 0x04, 0x00, 0x00, 0xff, 0xc0, 0x00, 0x11, 0x08, 0x00, 0xb9,
        0x00, 0xf7, 0x03, 0x01, 0x11, 0x00, 0x02, 0x11, 0x00, 0x03, 0x11, 0x00, 0xff, 0xd9,
    ];
    let file = prt_with_named_payloads(&[
        ("/Root/UG_PART/UG_PART", zlib_compress(&partition_stream())),
        ("/Root/images/preview", preview.to_vec()),
    ]);
    let container_only_file = file.clone();
    let result = NxCodec
        .decode(&mut Cursor::new(file), &DecodeOptions::default())
        .unwrap();
    let attributes = &result.ir().source.as_ref().unwrap().attributes;
    assert_eq!(attributes["jpeg_preview_count"], "1");
    assert_eq!(attributes["jpeg_preview_0_width"], "247");
    assert_eq!(attributes["jpeg_preview_0_height"], "185");
    assert_eq!(attributes["jpeg_preview_0_precision"], "8");
    assert_eq!(attributes["jpeg_preview_0_components"], "3");
    assert_eq!(
        attributes["jpeg_preview_0_byte_len"],
        preview.len().to_string()
    );
    assert_eq!(result.ir().model.assets.len(), 1);
    let asset = &result.ir().model.assets[0];
    assert_eq!(
        asset
            .name
            .as_ref()
            .map(cadmpeg_ir::products::NonBlankString::as_str),
        Some("preview.jpg")
    );
    assert_eq!(
        asset
            .media_type
            .as_ref()
            .map(cadmpeg_ir::products::NonBlankString::as_str),
        Some("image/jpeg")
    );
    assert_eq!(
        asset.native_ref.as_deref(),
        Some("nx:container:jpeg-preview#0")
    );
    assert!(matches!(
        &asset.content,
        cadmpeg_ir::assets::AssetContent::Embedded { data } if data.as_slice() == preview.as_slice()
    ));
    let container_only_result = NxCodec
        .decode(
            &mut Cursor::new(container_only_file),
            &DecodeOptions {
                container_only: true,
                ..DecodeOptions::default()
            },
        )
        .unwrap();
    assert_eq!(
        container_only_result.ir().model.assets,
        result.ir().model.assets
    );

    let mut malformed = preview;
    malformed[10..12].copy_from_slice(&16u16.to_be_bytes());
    assert!(jpeg_dimensions(&malformed).is_none());
    let malformed_file = prt_with_named_payloads(&[
        ("/Root/UG_PART/UG_PART", zlib_compress(&partition_stream())),
        ("/Root/images/preview", malformed.to_vec()),
    ]);
    let malformed_result = NxCodec
        .decode(&mut Cursor::new(malformed_file), &DecodeOptions::default())
        .unwrap();
    assert!(malformed_result.ir().model.assets.is_empty());
    let malformed_unknowns = malformed_result.ir().native_unknowns("nx").unwrap();
    assert!(malformed_unknowns
        .iter()
        .any(|unknown| unknown.id.as_str() == "nx:container:jpeg-preview#0"));
}

#[test]
fn decode_rejects_repeated_nx_arrangement_terminators_atomically() {
    let mut arrangements =
        br#"<Arrangements><Arrangement Default="YES" Name="Model"/></Arrangements>"#.to_vec();
    arrangements.extend_from_slice(&[0, 0]);
    let file = prt_with_named_payloads(&[
        ("/Root/UG_PART/UG_PART", zlib_compress(&partition_stream())),
        ("/Root/part/arrangements", arrangements),
    ]);
    let result = NxCodec
        .decode(&mut Cursor::new(file), &DecodeOptions::default())
        .unwrap();
    assert!(result.ir().model.configurations.is_empty());
}

#[test]
fn retained_material_library_assets_do_not_imply_an_assignment_loss() {
    let file = prt_with_named_payloads(&[
        (
            "/Root/UG_PART/UG_PART",
            zlib_compress(&topology_partition_stream()),
        ),
        (
            "/Root/materialsTif/Steel",
            vec![b'M', b'M', 0, 42, 0, 0, 0, 8, 0, 0],
        ),
    ]);
    let result = NxCodec
        .decode(&mut Cursor::new(file), &DecodeOptions::default())
        .unwrap();

    assert_eq!(result.ir().model.assets.len(), 1);
    let asset = &result.ir().model.assets[0];
    assert_eq!(
        asset
            .name
            .as_ref()
            .map(cadmpeg_ir::products::NonBlankString::as_str),
        Some("Steel")
    );
    assert_eq!(
        asset
            .media_type
            .as_ref()
            .map(cadmpeg_ir::products::NonBlankString::as_str),
        Some("image/tiff")
    );
    assert!(matches!(
        &asset.content,
        cadmpeg_ir::assets::AssetContent::Embedded { data }
            if data.as_slice() == [b'M', b'M', 0, 42, 0, 0, 0, 8, 0, 0]
    ));
    assert_eq!(
        asset.native_ref.as_deref(),
        Some("nx:container:material-texture#0")
    );
    assert!(result
        .report()
        .losses
        .iter()
        .all(|loss| loss.code != LossKind::shared(LossTaxonomy::MaterialNotTransferred)));
}
