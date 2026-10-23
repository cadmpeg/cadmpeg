// SPDX-License-Identifier: Apache-2.0
//! Image resources retain native fields and link inward through each asset.
use super::*;

use crate::test_support::test_dump as bytes;
use crate::test_support::test_dump::utf16_bytes;
use crate::wire::Uuid;
use cadmpeg_ir::assets::AssetContent;
use cadmpeg_ir::codec::write::{target::TargetRequest, EncodeInput, Encoder};

const WINDOWS_BITMAP_EX: Uuid = Uuid::from_canonical([
    0x20, 0x3a, 0xfc, 0x17, 0xbc, 0xc9, 0x44, 0xfb, 0xa0, 0x7b, 0x7f, 0x5c, 0x31, 0xbd, 0x5e, 0xd9,
]);

fn embedded_bitmap_payload(minor: u8, id: Uuid, compression_method: i32) -> Vec<u8> {
    let mut bytes = vec![0x10 | minor];
    bytes.extend(utf16_bytes("image.png"));
    bytes.extend(0x1122_3344_u32.to_le_bytes());
    bytes.extend(compression_method.to_le_bytes());
    if compression_method == 0 {
        bytes.extend(3_u32.to_le_bytes());
        bytes.extend([0x11, 0x22, 0x33]);
    } else {
        bytes.extend(0_u32.to_le_bytes());
    }
    if minor >= 1 {
        bytes.extend(id.to_wire());
        bytes.extend(utf16_bytes("preview"));
    }
    bytes.extend([0xaa, 0xbb]);
    bytes
}

fn bitmap_header(
    width: i32,
    height: i32,
    bits_per_pixel: u16,
    image_byte_len: i32,
    colors_used: i32,
) -> Vec<u8> {
    let mut bytes = 40_i32.to_le_bytes().to_vec();
    bytes.extend(width.to_le_bytes());
    bytes.extend(height.to_le_bytes());
    bytes.extend(1_u16.to_le_bytes());
    bytes.extend(bits_per_pixel.to_le_bytes());
    bytes.extend(0_i32.to_le_bytes());
    bytes.extend(image_byte_len.to_le_bytes());
    bytes.extend(0_i32.to_le_bytes());
    bytes.extend(0_i32.to_le_bytes());
    bytes.extend(colors_used.to_le_bytes());
    bytes.extend(0_i32.to_le_bytes());
    bytes
}

fn stored_bitmap_buffer(bytes: &[u8]) -> Vec<u8> {
    let mut buffer = (u32::try_from(bytes.len()).expect("fixture value fits u32"))
        .to_le_bytes()
        .to_vec();
    if !bytes.is_empty() {
        buffer.extend(crc32fast::hash(bytes).to_le_bytes());
        buffer.push(0);
        buffer.extend(bytes);
    }
    buffer
}

fn windows_bitmap_payload(
    class_uuid: Uuid,
    minor: u8,
    path: &str,
    header: Vec<u8>,
    buffers: &[Vec<u8>],
    suffix: &[u8],
) -> Vec<u8> {
    let mut bytes = Vec::new();
    if class_uuid == WINDOWS_BITMAP_EX {
        bytes.push(0x10 | minor);
        bytes.extend(utf16_bytes(path));
    }
    bytes.extend(header);
    for buffer in buffers {
        bytes.extend(buffer);
    }
    bytes.extend(suffix);
    bytes
}

fn source() -> (Vec<u8>, Vec<u8>) {
    let archive = ArchiveVersion::V8;
    let embedded = Uuid::from_canonical([
        0x77, 0x2e, 0x6f, 0xc1, 0xb1, 0x7b, 0x4f, 0xc4, 0x8f, 0x54, 0x5f, 0xda, 0x51, 0x1d, 0x76,
        0xd2,
    ]);
    let bitmap = Uuid::from_canonical([
        0x20, 0x3a, 0xfc, 0x17, 0xbc, 0xc9, 0x44, 0xfb, 0xa0, 0x7b, 0x7f, 0x5c, 0x31, 0xbd, 0x5e,
        0xd9,
    ]);
    let mut image = embedded_bitmap_payload(1, embedded, 0);
    let crc = 1 + bytes::utf16_bytes("image.png").len();
    image[crc..crc + 4].copy_from_slice(&crc32fast::hash(&[0x11, 0x22, 0x33]).to_le_bytes());
    let pixels = vec![0x45; 24];
    let buffer = stored_bitmap_buffer(&pixels);
    let windows = windows_bitmap_payload(
        bitmap,
        1,
        "legacy.bmp",
        bitmap_header(3, 2, 24, 24, 0),
        std::slice::from_ref(&buffer),
        &[0xaa, 0xbb],
    );
    let records = [(embedded, image), (bitmap, windows)]
        .into_iter()
        .map(|(class, payload)| {
            bytes::nested_crc_chunk(
                archive,
                0x2000_8090,
                &bytes::class_wrapper(archive, class.to_wire(), &payload),
            )
        })
        .collect::<Vec<_>>();
    (
        bytes::minimal_document(
            "80",
            &[
                bytes::table(archive, 0x1000_0014, &[]),
                bytes::table(archive, 0x1000_0015, &[]),
                bytes::table(archive, 0x1000_0016, &records),
                bytes::table(archive, 0x1000_0013, &[]),
            ],
        ),
        buffer,
    )
}

fn payload(asset: &cadmpeg_ir::assets::Asset) -> &[u8] {
    let AssetContent::Embedded { data } = &asset.content else {
        panic!("embedded resource")
    };
    data.as_bytes()
}

#[test]
fn image_source_buffers_are_record_layout_extents_not_document_fields() {
    let (source, bitmap_buffer) = source();
    let decoded = RhinoCodec
        .decode(&mut Cursor::new(source.clone()), &DecodeOptions::default())
        .unwrap();
    let namespace = decoded.ir().native.namespace("rhino").unwrap();
    let image = &namespace.arenas()["embedded_images"][0];
    let bitmap = &namespace.arenas()["windows_bitmaps"][0];
    for key in ["buffer_offset", "buffer_byte_len", "buffer_sha256"] {
        assert_eq!(image.field(key), None, "{key}");
    }
    for key in [
        "pixel_buffer_offset",
        "pixel_buffer_byte_len",
        "pixel_buffer_sha256",
    ] {
        assert_eq!(bitmap.field(key), None, "{key}");
    }
    assert!(bitmap.field("image_byte_len").is_some());
    let mut image_buffer = 3_u32.to_le_bytes().to_vec();
    image_buffer.extend([0x11, 0x22, 0x33]);
    for (record, buffer) in [(image, image_buffer), (bitmap, bitmap_buffer)] {
        let layout = &decoded.source_fidelity().record_layout()[record.id()];
        let start = usize::try_from(layout.offset()).unwrap();
        let len = usize::try_from(layout.byte_len()).unwrap();
        assert_eq!(len, buffer.len(), "{}", record.id());
        assert_eq!(
            &source[start..start + len],
            buffer.as_slice(),
            "{}",
            record.id()
        );
        assert!(layout.payloads().is_empty(), "{}", record.id());
    }
}

#[test]
fn images_keep_native_headers_buffers_and_payloads_without_asset_backlinks() {
    let (source, _) = source();
    let decoded = RhinoCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let ir = decoded.ir();
    assert_eq!(ir.model.assets.len(), 2);
    let namespace = ir.native.namespace("rhino").unwrap();
    let image = &namespace.arenas()["embedded_images"][0];
    for (key, value) in [
        (
            "source_uuid",
            serde_json::json!("772e6fc1-b17b-4fc4-8f54-5fda511d76d2"),
        ),
        ("name", serde_json::json!("preview")),
        ("file_path", serde_json::json!("image.png")),
    ] {
        assert_eq!(image.field(key), Some(value));
    }
    let bitmap = &namespace.arenas()["windows_bitmaps"][0];
    for (key, value) in [
        (
            "class_uuid",
            serde_json::json!("203afc17-bcc9-44fb-a07b-7f5c31bd5ed9"),
        ),
        ("file_path", serde_json::json!("legacy.bmp")),
        ("header_size", serde_json::json!(40)),
        ("width_pixels", serde_json::json!(3)),
        ("height_pixels", serde_json::json!(2)),
        ("planes", serde_json::json!(1)),
        ("bits_per_pixel", serde_json::json!(24)),
        ("compression", serde_json::json!(0)),
        ("image_byte_len", serde_json::json!(24)),
        ("pixels_per_meter", serde_json::json!([0, 0])),
        ("colors_used", serde_json::json!(0)),
        ("important_colors", serde_json::json!(0)),
    ] {
        assert_eq!(bitmap.field(key), Some(value));
    }
    for record in [image, bitmap] {
        assert_eq!(record.field("asset"), None);
        let asset = ir
            .model
            .assets
            .iter()
            .find(|asset| asset.native_ref.as_deref() == Some(record.id()))
            .unwrap();
        assert!(!payload(asset).is_empty());
    }
    assert_eq!(payload(&ir.model.assets[0]), [0x11, 0x22, 0x33]);
    assert!(payload(&ir.model.assets[1]).starts_with(b"BM"));
    assert!(payload(&ir.model.assets[1]).ends_with(&[0x45; 24]));
}

#[test]
fn source_free_image_write_uses_current_asset_bytes_and_forward_native_identity() {
    let (source, _) = source();
    let decoded = RhinoCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let mut ir = decoded.ir().clone();
    let native = ir.native.clone();
    ir.model.assets[0].name = cadmpeg_core::text::NonBlankString::new("Edited preview");
    ir.model.assets[0].content = AssetContent::Embedded {
        data: cadmpeg_ir::assets::AssetData::new(vec![0x44, 0x55, 0x66]).unwrap(),
    };
    let model = ir.model.clone();
    let plan = RhinoCodec
        .plan(
            EncodeInput::new(&ir, None),
            TargetRequest::Explicit("rhino:archive-80"),
        )
        .unwrap();
    let mut output = Vec::new();
    plan.write_to(&mut output).unwrap();
    let written = RhinoCodec
        .decode(&mut Cursor::new(output), &DecodeOptions::default())
        .unwrap();
    assert_eq!(written.ir().model.assets.len(), 2);
    for (before, after) in ir.model.assets.iter().zip(&written.ir().model.assets) {
        assert_eq!(payload(after), payload(before));
        let id = after.native_ref.as_deref().unwrap();
        assert!(written
            .ir()
            .native
            .namespace("rhino")
            .unwrap()
            .arenas()
            .values()
            .flatten()
            .any(|record| record.id() == id));
    }
    let images = &written.ir().native.namespace("rhino").unwrap().arenas()["embedded_images"];
    let owner = written.ir().model.assets[0].native_ref.as_deref().unwrap();
    let image = images.iter().find(|record| record.id() == owner).unwrap();
    assert_eq!(
        image.field("source_uuid"),
        Some(serde_json::json!("772e6fc1-b17b-4fc4-8f54-5fda511d76d2"))
    );
    assert_eq!(
        image.field("file_path"),
        Some(serde_json::json!("image.png"))
    );
    assert_eq!(
        image.field("name"),
        Some(serde_json::json!("Edited preview"))
    );
    assert!(images.iter().all(|record| record.field("asset").is_none()));
    assert_eq!(ir.model, model);
    assert_eq!(ir.native, native);
}
