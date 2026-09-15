// SPDX-License-Identifier: Apache-2.0
use super::*;
use crate::chunks::ArchiveVersion;
use crate::objects::AttributeUserdata;
use std::io::Write;

fn utf16(value: &str) -> Vec<u8> {
    let mut units = value.encode_utf16().collect::<Vec<_>>();
    units.push(0);
    let mut bytes = (units.len() as u32).to_le_bytes().to_vec();
    for unit in units {
        bytes.extend(unit.to_le_bytes());
    }
    bytes
}

fn anonymous(minor: i32, body: &[u8]) -> Vec<u8> {
    let mut payload = 1_i32.to_le_bytes().to_vec();
    payload.extend(minor.to_le_bytes());
    payload.extend(body);
    payload.extend(crc32fast::hash(&payload).to_le_bytes());
    let mut bytes = 0x4000_8000_u32.to_le_bytes().to_vec();
    bytes.extend((payload.len() as i64).to_le_bytes());
    bytes.extend(payload);
    bytes
}

fn anonymous_body(body: &[u8]) -> Vec<u8> {
    let mut payload = body.to_vec();
    payload.extend(crc32fast::hash(&payload).to_le_bytes());
    let mut bytes = 0x4000_8000_u32.to_le_bytes().to_vec();
    bytes.extend((payload.len() as i64).to_le_bytes());
    bytes.extend(payload);
    bytes
}

fn physically_based_payload(version: i32, suffix: &[u8]) -> Vec<u8> {
    let mut body = Vec::new();
    for value in [0.1_f32, 0.2, 0.3, 0.4] {
        body.extend(value.to_le_bytes());
    }
    body.extend(1_i32.to_le_bytes());
    body.extend(0.5_f64.to_le_bytes());
    for value in [0.6_f32, 0.7, 0.8, 0.9] {
        body.extend(value.to_le_bytes());
    }
    for value in 1..=14 {
        body.extend((value as f64).to_le_bytes());
    }
    for value in [0.11_f32, 0.22, 0.33, 0.44] {
        body.extend(value.to_le_bytes());
    }
    if version >= 2 {
        body.extend(0.77_f64.to_le_bytes());
    }
    body.extend(suffix);
    let inner = anonymous(version, &body);
    let mut payload = inner.clone();
    payload.extend(crc32fast::hash(&inner).to_le_bytes());
    let mut bytes = 0x4000_8000_u32.to_le_bytes().to_vec();
    bytes.extend((payload.len() as i64).to_le_bytes());
    bytes.extend(payload);
    bytes
}

fn wide_string_chunk(value: &str) -> Vec<u8> {
    let mut payload = vec![1];
    payload.extend(value.as_bytes());
    payload.extend(crc32fast::hash(&payload).to_le_bytes());
    let mut bytes = 0x4000_8001_u32.to_le_bytes().to_vec();
    bytes.extend((payload.len() as i64).to_le_bytes());
    bytes.extend(payload);
    bytes
}

fn panose_chunk() -> Vec<u8> {
    let mut payload = vec![0x10];
    payload.extend([2, 1, 2, 3, 4, 5, 6, 7, 8, 9]);
    payload.extend(crc32fast::hash(&payload).to_le_bytes());
    let mut bytes = 0x4000_8000_u32.to_le_bytes().to_vec();
    bytes.extend((payload.len() as i64).to_le_bytes());
    bytes.extend(payload);
    bytes
}

fn modern_font_chunk(minor: i32, suffix: &[u8]) -> Vec<u8> {
    let mut body = 0x1234_5678_u32.to_le_bytes().to_vec();
    body.extend(wide_string_chunk("Arial"));
    body.extend(utf16("ArialMT"));
    body.extend(utf16("Arial Regular"));
    body.extend(400_i32.to_le_bytes());
    body.extend(0.5_f64.to_le_bytes());
    body.extend(12.0_f64.to_le_bytes());
    body.push(0);
    body.extend(utf16("Arial"));
    for value in [
        "en-US", "ArialMT", "ArialMT", "Arial", "Arial", "Arial", "Arial", "Regular", "Regular",
    ] {
        body.extend(utf16(value));
    }
    body.extend(panose_chunk());
    body.push(2);
    body.extend(suffix);
    anonymous(minor, &body)
}

fn model_attributes_chunk(index: i32, name: &str) -> Vec<u8> {
    let mut payload = 1_i32.to_le_bytes().to_vec();
    payload.extend(0_i32.to_le_bytes());
    payload.extend([0, 2, 0, 1]);
    payload.extend(index.to_le_bytes());
    payload.push(1);
    payload.extend(utf16(name));
    payload.extend(crc32fast::hash(&payload).to_le_bytes());
    let mut bytes = MODEL_ATTRIBUTES.to_le_bytes().to_vec();
    bytes.extend((payload.len() as i64).to_le_bytes());
    bytes.extend(payload);
    bytes
}

fn model_attributes_status_chunk(statuses: [u8; 5], name: &str, suffix: &[u8]) -> Vec<u8> {
    let mut payload = 1_i32.to_le_bytes().to_vec();
    payload.extend(0_i32.to_le_bytes());
    payload.extend(statuses);
    if statuses[0] == 1 {
        payload.extend([1_u32, 2, 3].into_iter().flat_map(u32::to_le_bytes));
    }
    if statuses[1] == 1 {
        payload.extend([0x22; 16]);
    }
    if statuses[2] == 1 {
        payload.extend(4_u32.to_le_bytes());
    }
    if statuses[3] == 1 {
        payload.extend(5_i32.to_le_bytes());
    }
    if statuses[4] == 1 {
        payload.extend(utf16(name));
    }
    payload.extend(suffix);
    payload.extend(crc32fast::hash(&payload).to_le_bytes());
    let mut bytes = MODEL_ATTRIBUTES.to_le_bytes().to_vec();
    bytes.extend((payload.len() as i64).to_le_bytes());
    bytes.extend(payload);
    bytes
}

fn dimension_style_chunk(minor: i32) -> Vec<u8> {
    let mut body = model_attributes_chunk(7, "dimension style");
    for value in [1.0_f64, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0] {
        body.extend(value.to_le_bytes());
    }
    body.extend(1_u32.to_le_bytes());
    body.extend(2_u32.to_le_bytes());
    body.extend(3_u32.to_le_bytes());
    body.extend(4_i32.to_le_bytes());
    body.extend(5_i32.to_le_bytes());
    body.extend((-1_i32).to_le_bytes());
    body.extend(1.0_f64.to_le_bytes());
    body.push(1);
    body.extend(1.5_f64.to_le_bytes());
    body.extend(6_u32.to_le_bytes());
    body.extend(7_i32.to_le_bytes());
    for value in ["<", ">", "[", "]"] {
        body.extend(utf16(value));
    }
    body.extend(8.0_f64.to_le_bytes());
    body.extend([0, 1]);
    body.extend([0x11; 16]);
    body.extend(9_u32.to_le_bytes());
    body.push(0);
    body.extend(10_u32.to_le_bytes());
    body.extend(11_i32.to_le_bytes());
    for value in [12.0_f64, 13.0, 14.0] {
        body.extend(value.to_le_bytes());
    }
    body.extend(15.0_f64.to_le_bytes());
    body.push(1);
    body.extend(16_u32.to_le_bytes());
    body.extend([17, 18, 19, 20]);
    body.extend(21.0_f64.to_le_bytes());
    body.extend(22_i32.to_le_bytes());
    body.extend([0x22; 16]);
    body.extend([23, 24, 25, 26]);
    for color in [
        [27, 28, 29, 30],
        [31, 32, 33, 34],
        [35, 36, 37, 38],
        [39, 40, 41, 42],
    ] {
        body.extend(color);
    }
    body.extend([43, 44, 45, 46]);
    for color in [
        [47, 48, 49, 50],
        [51, 52, 53, 54],
        [55, 56, 57, 58],
        [59, 60, 61, 62],
    ] {
        body.extend(color);
    }
    body.extend([63, 64]);
    for value in [65.0_f64, 66.0, 67.0] {
        body.extend(value.to_le_bytes());
    }
    body.push(1);
    body.extend(68.0_f64.to_le_bytes());
    body.extend(69_i32.to_le_bytes());
    body.extend(70.0_f64.to_le_bytes());
    body.extend([0, 1]);
    body.extend(71_i32.to_le_bytes());
    body.extend(72_i32.to_le_bytes());
    body.extend(73.0_f64.to_le_bytes());
    body.extend(74_u32.to_le_bytes());
    for value in [75.0_f64, 76.0, 77.0] {
        body.extend(value.to_le_bytes());
    }
    body.extend([78_u32, 79, 80, 81].into_iter().flat_map(u32::to_le_bytes));
    body.push(1);
    body.extend([82_u32, 83, 84].into_iter().flat_map(u32::to_le_bytes));
    body.extend([0x66; 16]);
    body.extend([0x77; 16]);
    body.extend([0x88; 16]);

    body.extend(
        [85_u32, 86, 87, 88, 89]
            .into_iter()
            .flat_map(u32::to_le_bytes),
    );
    body.extend(90.0_f64.to_le_bytes());
    body.push(1);
    body.extend(91.0_f64.to_le_bytes());
    body.extend([90_u32, 91].into_iter().flat_map(u32::to_le_bytes));
    body.push(1);
    body.push(0);
    body.extend(anonymous(0, &[]));
    body.extend(92_u32.to_le_bytes());
    body.extend(anonymous(0, &[]));
    body.extend(anonymous(0, &[]));
    for value in 93_u32..105 {
        body.extend(value.to_le_bytes());
    }
    body.push(1);
    body.extend(
        [105_u32, 106, 107, 108, 109]
            .into_iter()
            .flat_map(u32::to_le_bytes),
    );
    body.extend(110_u32.to_le_bytes());
    body.extend(111_u32.to_le_bytes());
    body.push(1);
    body.extend(112_u32.to_le_bytes());
    if minor >= 10 {
        body.push(1);
    }
    if minor >= 11 {
        body.extend(1.75_f64.to_le_bytes());
    }
    body.extend([0xaa, 0xbb]);
    anonymous(minor, &body)
}

fn future_dimension_style_chunk() -> Vec<u8> {
    dimension_style_chunk(12)
}

fn current_dimension_style_chunk() -> Vec<u8> {
    dimension_style_chunk(11)
}

fn v5_dimension_style_chunk() -> Vec<u8> {
    let mut bytes = vec![0x15];
    bytes.extend(7_i32.to_le_bytes());
    bytes.extend(utf16("legacy dimension style"));
    for value in [1.0_f64, 2.0, 3.0, 4.0, 5.0] {
        bytes.extend(value.to_le_bytes());
    }
    bytes.extend(6_u32.to_le_bytes());
    bytes.extend(7_i32.to_le_bytes());
    bytes.extend(8_i32.to_le_bytes());
    bytes.extend(9_u32.to_le_bytes());
    bytes.extend(10_u32.to_le_bytes());
    bytes.extend(11_i32.to_le_bytes());
    bytes.extend(12_i32.to_le_bytes());
    bytes.extend(13_i32.to_le_bytes());
    bytes.extend(14.0_f64.to_le_bytes());
    bytes.extend(15.0_f64.to_le_bytes());
    bytes.extend(utf16("<"));
    bytes.extend(utf16(">"));
    bytes.push(1);
    bytes.extend(16.0_f64.to_le_bytes());
    bytes.extend(17_u32.to_le_bytes());
    bytes.extend(18_i32.to_le_bytes());
    bytes.extend(19_u32.to_le_bytes());
    bytes.extend(20_i32.to_le_bytes());
    bytes.extend(utf16("["));
    bytes.extend(utf16("]"));
    bytes.extend(21_u32.to_le_bytes());
    bytes.extend([0x33; 16]);
    bytes.extend(22.0_f64.to_le_bytes());
    bytes.extend(23.0_f64.to_le_bytes());
    bytes.extend(24_i32.to_le_bytes());
    bytes.extend([1, 0]);
    bytes.extend([0xaa, 0xbb]);
    bytes
}

fn v5_dimension_style_extra_chunk() -> Vec<u8> {
    let mut body = Vec::new();
    body.extend([0x11; 16]);
    body.extend(3_i32.to_le_bytes());
    body.extend([0, 1, 2]);
    body.extend(3_i32.to_le_bytes());
    body.extend(4_i32.to_le_bytes());
    body.extend(0.25_f64.to_le_bytes());
    body.extend((-0.125_f64).to_le_bytes());
    body.extend(1.25_f64.to_le_bytes());
    body.extend(2.5_f64.to_le_bytes());
    body.push(1);
    body.extend(2_i32.to_le_bytes());
    body.extend([11, 22, 33, 44]);
    body.extend(1.75_f64.to_le_bytes());
    body.extend(2_i32.to_le_bytes());
    body.extend([0x22; 16]);
    body.extend([0xcc, 0xdd]);
    anonymous(3, &body)
}

fn embedded_bitmap_payload(minor: u8, id: Uuid, compression_method: i32) -> Vec<u8> {
    let mut bytes = vec![0x10 | minor];
    bytes.extend(utf16("image.png"));
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
        bytes.extend(utf16("preview"));
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
    let mut buffer = (bytes.len() as u32).to_le_bytes().to_vec();
    if !bytes.is_empty() {
        buffer.extend(crc32fast::hash(bytes).to_le_bytes());
        buffer.push(0);
        buffer.extend(bytes);
    }
    buffer
}

fn compressed_bitmap_buffer(bytes: &[u8]) -> Vec<u8> {
    let mut encoder = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
    encoder.write_all(bytes).expect("bitmap zlib input");
    let compressed = encoder.finish().expect("bitmap zlib output");
    let mut buffer = (bytes.len() as u32).to_le_bytes().to_vec();
    buffer.extend(crc32fast::hash(bytes).to_le_bytes());
    buffer.push(1);
    buffer.extend(crate::test_support::test_dump::crc_chunk(
        ArchiveVersion::V8,
        ANONYMOUS,
        &compressed,
    ));
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
        bytes.extend(utf16(path));
    }
    bytes.extend(header);
    for buffer in buffers {
        bytes.extend(buffer);
    }
    bytes.extend(suffix);
    bytes
}

#[test]
fn light_table_class_data_stops_before_record_children() {
    let archive = ArchiveVersion::V5;
    let payload = [0x12, 0xaa, 0xbb];
    let mut body =
        crate::test_support::test_dump::class_wrapper(archive, LIGHT.to_wire(), &payload);
    body.extend(crate::test_support::test_dump::crc_chunk(
        archive,
        0x0200_8061,
        &[],
    ));
    body.extend(crate::test_support::test_dump::short_chunk(
        archive,
        0x8200_006f,
        0,
    ));
    let record = Record::long(0x2000_8060, 0..body.len(), 0..body.len());

    let range = class_data_prefix(&body, &record, archive, LIGHT).expect("light class");
    assert_eq!(&body[range], payload);
}

#[test]
fn light_record_attributes_use_the_object_attribute_projection() {
    let archive = ArchiveVersion::V5;
    let mut attributes = vec![0x20];
    attributes.extend([0; 16]);
    attributes.extend(7_i32.to_le_bytes());
    attributes.push(1);
    attributes.extend(utf16("table light"));
    attributes.push(11);
    attributes.push(0);
    attributes.push(0);
    let mut body =
        crate::test_support::test_dump::class_wrapper(archive, LIGHT.to_wire(), &[0x12, 0xaa]);
    body.extend(crate::test_support::test_dump::crc_chunk(
        archive,
        LIGHT_RECORD_ATTRIBUTES,
        &attributes,
    ));
    let user_string_body = [
        1_i32.to_le_bytes().as_slice(),
        crate::test_support::test_dump::anonymous_chunk(
            archive,
            0,
            &[utf16("attribute key"), utf16("attribute value")].concat(),
        )
        .as_slice(),
    ]
    .concat();
    let userdata = crate::test_support::test_dump::class_userdata_with_payload(
        archive,
        USER_STRING_LIST.to_wire(),
        [0; 16],
        &user_string_body,
    );
    body.extend(crate::test_support::test_dump::long_chunk(
        archive,
        LIGHT_RECORD_ATTRIBUTES_USERDATA,
        &[
            userdata,
            crate::test_support::test_dump::short_chunk(archive, 0x8002_7fff, 0),
        ]
        .concat(),
    ));
    body.extend(crate::test_support::test_dump::short_chunk(
        archive,
        LIGHT_RECORD_END,
        0,
    ));
    let record = Record::long(0x2000_8060, 0..body.len(), 0..body.len());
    let mut losses = Vec::new();
    let value =
        parse_light_record_attributes(&body, &record, archive, Some(2_024_071_000), &mut losses)
            .expect("light record attributes")
            .expect("light attributes child");
    assert!(losses.is_empty());
    assert_eq!(value.attributes.layer_index, 7);
    assert_eq!(value.attributes.name, "table light");
    assert!(!value.attributes.visible);
    assert_eq!(value.attributes.source_uuid, Uuid::nil().to_string());
    assert_eq!(
        value.attributes.attribute_user_strings[0].key,
        "attribute key"
    );
    assert_eq!(
        value.attributes.attribute_user_strings[0].value,
        "attribute value"
    );
    assert!(value.source_offset > 0);
}

#[test]
fn embedded_bitmap_minor_gate_preserves_suffix_boundary() {
    let id = Uuid::from_canonical([
        0x77, 0x2e, 0x6f, 0xc1, 0xb1, 0x7b, 0x4f, 0xc4, 0x8f, 0x54, 0x5f, 0xda, 0x51, 0x1d, 0x76,
        0xd2,
    ]);
    let minor_zero_bytes = embedded_bitmap_payload(0, id, 1);
    let minor_zero = parse_embedded_image(
        &minor_zero_bytes,
        0..minor_zero_bytes.len(),
        ArchiveVersion::V8,
        42,
    )
    .expect("minor zero embedded bitmap");
    assert_eq!(minor_zero.source_uuid, None);
    assert_eq!(minor_zero.name, "");
    assert_eq!(minor_zero.buffer_byte_len, 4);

    let minor_one_bytes = embedded_bitmap_payload(1, id, 1);
    let minor_one = parse_embedded_image(
        &minor_one_bytes,
        0..minor_one_bytes.len(),
        ArchiveVersion::V8,
        42,
    )
    .expect("minor one embedded bitmap");
    assert_eq!(minor_one.source_uuid, Some(id.to_string()));
    assert_eq!(minor_one.name, "preview");
    assert_eq!(minor_one.image_crc32, 0x1122_3344);
    assert_eq!(
        minor_one.compression_method,
        EmbeddedImageCompression::Compressed
    );
    assert_eq!(minor_one.buffer_byte_len, 4);

    let raw_bytes = embedded_bitmap_payload(0, id, 0);
    let raw = parse_embedded_image(&raw_bytes, 0..raw_bytes.len(), ArchiveVersion::V8, 42)
        .expect("raw embedded bitmap");
    assert_eq!(raw.compression_method, EmbeddedImageCompression::Raw);
    assert_eq!(raw.uncompressed_byte_len, 3);
    assert_eq!(raw.buffer_byte_len, 7);
}

#[test]
fn windows_bitmap_consumes_source_buffer_variants_and_suffix() {
    let image = vec![0x11; 24];
    let contiguous = windows_bitmap_payload(
        WINDOWS_BITMAP,
        0,
        "",
        bitmap_header(3, 2, 24, image.len() as i32, 0),
        &[stored_bitmap_buffer(&image)],
        &[0xaa, 0xbb],
    );
    let contiguous_record = parse_windows_bitmap(
        &contiguous,
        0..contiguous.len(),
        WINDOWS_BITMAP,
        ArchiveVersion::V8,
        70,
    )
    .expect("contiguous Windows bitmap");
    assert_eq!(contiguous_record.width_pixels, 3);
    assert_eq!(contiguous_record.height_pixels, 2);
    assert_eq!(contiguous_record.pixel_buffer_offset, 40);
    assert_eq!(
        contiguous_record.pixel_buffer_byte_len as usize,
        contiguous.len() - 42
    );

    let palette = vec![0x22; 256 * 4];
    let pixels = vec![0x33; 8];
    let split = windows_bitmap_payload(
        WINDOWS_BITMAP,
        0,
        "",
        bitmap_header(2, 2, 8, pixels.len() as i32, 0),
        &[
            compressed_bitmap_buffer(&palette),
            stored_bitmap_buffer(&pixels),
        ],
        &[0xcc, 0xdd],
    );
    let split_record = parse_windows_bitmap(
        &split,
        0..split.len(),
        WINDOWS_BITMAP,
        ArchiveVersion::V8,
        71,
    )
    .expect("split Windows bitmap");
    assert_eq!(split_record.bits_per_pixel, 8);
    assert_eq!(split_record.colors_used, 0);
    assert_eq!(
        split_record.pixel_buffer_byte_len as usize,
        split.len() - 42
    );

    let ex = windows_bitmap_payload(
        WINDOWS_BITMAP_EX,
        5,
        "relative/example.bmp",
        bitmap_header(2, 2, 24, pixels.len() as i32, 0),
        &[stored_bitmap_buffer(&pixels)],
        &[0xee, 0xff],
    );
    let ex_record =
        parse_windows_bitmap(&ex, 0..ex.len(), WINDOWS_BITMAP_EX, ArchiveVersion::V8, 72)
            .expect("minor-five Windows bitmap Ex");
    assert_eq!(ex_record.file_path, "relative/example.bmp");
    assert_eq!(
        ex_record.pixel_buffer_byte_len as usize,
        ex.len() - 47 - 40 - 2
    );
}

#[test]
fn legacy_windows_bitmap_uses_raw_palette_and_pixels() {
    let palette = vec![0x55; 256 * 4];
    let pixels = vec![0x66; 8];
    let mut raw = palette;
    raw.extend(pixels);
    let bytes = windows_bitmap_payload(
        WINDOWS_BITMAP,
        0,
        "",
        bitmap_header(2, 2, 8, 8, 0),
        &[raw],
        &[0xaa, 0xbb],
    );
    let record = parse_windows_bitmap(
        &bytes,
        0..bytes.len(),
        WINDOWS_BITMAP,
        ArchiveVersion::V1,
        74,
    )
    .expect("legacy raw Windows bitmap");
    assert_eq!(record.pixel_buffer_byte_len, (256 * 4 + 8) as u64);
}

#[test]
fn windows_bitmap_rejects_a_buffer_size_that_disagrees_with_header() {
    let image = [0x44; 24];
    let bytes = windows_bitmap_payload(
        WINDOWS_BITMAP,
        0,
        "",
        bitmap_header(3, 2, 24, image.len() as i32, 0),
        &[stored_bitmap_buffer(&image[..1])],
        &[],
    );
    assert!(parse_windows_bitmap(
        &bytes,
        0..bytes.len(),
        WINDOWS_BITMAP,
        ArchiveVersion::V8,
        73,
    )
    .is_err());
}

/// The PostScript name only comes from the description when the stamp says
/// the writer is newer than 2018-02-23. An unstamped archive drops it.
#[test]
fn unstamped_legacy_text_style_charges_the_font_name_stamp_loss() {
    let bytes = legacy_text_style_bytes();
    let mut losses = Vec::new();
    let value = parse_text_style(
        &bytes,
        0..bytes.len(),
        ArchiveVersion::V8,
        None,
        false,
        42,
        &mut losses,
    )
    .expect("legacy text style without a writer stamp");
    assert_eq!(value.font.postscript_name, "");
    assert_eq!(losses.len(), 1, "{losses:?}");
    assert_eq!(
        losses[0].code.local_code(),
        RhinoLossCode::SourceWriterStampUnverified.code()
    );

    let mut stamped_losses = Vec::new();
    let stamped = parse_text_style(
        &bytes,
        0..bytes.len(),
        ArchiveVersion::V8,
        Some(201_802_231),
        false,
        42,
        &mut stamped_losses,
    )
    .expect("legacy text style with a modern writer stamp");
    assert_eq!(stamped.font.postscript_name, "Helvetica Neue");
    assert!(stamped_losses.is_empty(), "{stamped_losses:?}");
}

/// One legacy text style whose description carries a real font name.
fn legacy_text_style_bytes() -> Vec<u8> {
    let mut bytes = vec![0x12];
    bytes.extend(7_i32.to_le_bytes());
    bytes.extend(utf16("Helvetica Neue"));
    let mut face = [0_u16; 64];
    for (target, source) in face.iter_mut().zip("Helvetica Neue".encode_utf16()) {
        *target = source;
    }
    for unit in face {
        bytes.extend(unit.to_le_bytes());
    }
    bytes.extend(700_i32.to_le_bytes());
    bytes.extend(1_i32.to_le_bytes());
    bytes.extend(1.6_f64.to_le_bytes());
    bytes.extend([0x11; 16]);
    bytes
}

#[test]
fn legacy_text_style_preserves_font_identity_and_characteristics() {
    let bytes = legacy_text_style_bytes();
    let value = parse_text_style(
        &bytes,
        0..bytes.len(),
        ArchiveVersion::V8,
        Some(201_802_231),
        false,
        42,
        &mut Vec::new(),
    )
    .expect("valid legacy text style");
    assert_eq!(value.archive_index, Some(7));
    assert_eq!(value.font.windows_logfont_name, "Helvetica Neue");
    assert!(matches!(
        value.font.weight,
        FontWeight::Legacy { windows: 700, .. }
    ));
    assert_eq!(value.font.characteristics, 0);
    assert!(matches!(
        value.font.weight,
        FontWeight::Legacy { italic: true, .. }
    ));
    assert_eq!(value.source_offset, 42);
}

#[test]
fn modern_font_matches_producer_wide_string_and_future_suffix() {
    let bytes = modern_font_chunk(7, &[0xaa, 0xbb]);
    let value = parse_font(
        &bytes,
        &mut BoundedReader::new(&bytes, 0, bytes.len()).unwrap(),
        ArchiveVersion::V8,
        None,
    )
    .expect("modern font with future minor");
    assert_eq!(value.characteristics, 0x1234_5678);
    assert_eq!(value.windows_logfont_name, "Arial");
    assert_eq!(value.postscript_name, "ArialMT");
    assert_eq!(value.family_name, "Arial");
    assert_eq!(value.panose, Some([2, 1, 2, 3, 4, 5, 6, 7, 8, 9]));
    assert_eq!(value.quartet_member, Some(2));
}

#[test]
fn modern_text_style_preserves_identity_after_future_font_and_outer_suffix() {
    let id = Uuid::from_canonical([
        0x73, 0x8f, 0x5c, 0x29, 0x7f, 0x42, 0x4c, 0x89, 0xa4, 0xf5, 0x34, 0x0a, 0x2d, 0x88, 0xc1,
        0x10,
    ]);
    let mut body = model_attributes_chunk(7, "Arial style");
    body.push(1);
    body.extend(utf16("ArialMT"));
    body.push(1);
    body.extend(modern_font_chunk(7, &[0xcc, 0xdd]));
    body.extend(id.to_wire());
    body.extend(utf16("Arial style"));
    body.extend([0xee, 0xff]);
    let bytes = anonymous(2, &body);
    let value = parse_text_style(
        &bytes,
        0..bytes.len(),
        ArchiveVersion::V8,
        None,
        false,
        99,
        &mut Vec::new(),
    )
    .expect("modern text style with future suffixes");
    assert_eq!(value.archive_index, Some(7));
    assert_eq!(value.name, "Arial style");
    assert_eq!(value.font_description, "ArialMT");
    assert_eq!(value.source_uuid, Some(id.to_string()));
    assert_eq!(value.font.windows_logfont_name, "Arial");
    assert_eq!(value.source_offset, 99);
}

#[test]
fn dimension_style_future_minor_preserves_known_prefix_and_suffix() {
    let bytes = future_dimension_style_chunk();
    let value = parse_dimension_style(&bytes, 0..bytes.len(), ArchiveVersion::V8, 1.0, 321)
        .expect("dimension style with future minor");
    assert_eq!(value.archive_index, Some(7));
    assert_eq!(value.name, "dimension style");
    assert_eq!(value.extension_line_extension_mm, 1.0);
    assert_eq!(
        value.details.controls()["decimal_separator"],
        serde_json::json!(112)
    );
    assert_eq!(
        value.details.controls()["use_kerning"],
        serde_json::json!(true)
    );
    assert_eq!(
        value.details.controls()["line_space_scale"],
        serde_json::json!(1.75)
    );
    assert_eq!(
        value.details.controls()["dimension_length_display"],
        serde_json::json!(107)
    );
    assert_eq!(
        value.details.controls()["font_characteristics"]["byte_len"],
        serde_json::json!(24)
    );
    assert_eq!(value.source_offset, 321);
}

#[test]
fn dimension_style_current_minor_transfers_new_text_controls() {
    let bytes = current_dimension_style_chunk();
    let value = parse_dimension_style(&bytes, 0..bytes.len(), ArchiveVersion::V8, 1.0, 654)
        .expect("dimension style with current minor");
    assert_eq!(
        value.details.controls()["use_kerning"],
        serde_json::json!(true)
    );
    assert_eq!(
        value.details.controls()["line_space_scale"],
        serde_json::json!(1.75)
    );
    assert_eq!(value.source_offset, 654);
}

#[test]
fn v5_dimension_style_and_extra_follow_source_gates_and_scaling() {
    let base = v5_dimension_style_chunk();
    let extra_bytes = v5_dimension_style_extra_chunk();
    let descriptor = ClassUserdata {
        range: 0..extra_bytes.len(),
        version: (2, 2),
        class_uuid: DIMSTYLE_EXTRA,
        item_uuid: DIMSTYLE_EXTRA,
        copy_count: 1,
        transform_range: 0..0,
        application_uuid: None,
        save_context: None,
        payload_range: 0..extra_bytes.len(),
    };
    let extra = parse_v5_dimension_style_extra(&extra_bytes, &descriptor, ArchiveVersion::V5, 2.0)
        .expect("V5 dimension-style extra");
    assert_eq!(extra.valid_fields, vec![false, true, true]);
    assert_eq!(extra.baseline_spacing_mm, 5.0);
    assert_eq!(extra.mask_color, [11, 22, 33, 44]);
    assert_eq!(
        extra.source_style_uuid,
        Some(Uuid::from_canonical([0x22; 16]).to_string())
    );

    let value = parse_v5_dimension_style(&base, 0..base.len(), 2.0, 321, Some(extra))
        .expect("V5 dimension style");
    assert_eq!(value.archive_index, Some(7));
    assert_eq!(value.name, "legacy dimension style");
    assert_eq!(value.extension_line_extension_mm, 2.0);
    assert_eq!(value.center_mark_size_mm, 8.0);
    assert_eq!(value.text_height_mm, 28.0);
    assert_eq!(value.leader_arrow_size_mm, 46.0);
    assert_eq!(value.length_factor, 15.0);
    assert_eq!(value.alternate_length_format, 17);
    assert_eq!(
        value.details.parent_style_uuid().cloned(),
        Some(Uuid::from_canonical([0x11; 16]).to_string())
    );
    assert_eq!(
        value.details.controls()["v5_arrow_type"],
        serde_json::json!(7)
    );
    assert_eq!(
        value.source_uuid,
        Some(Uuid::from_canonical([0x33; 16]).to_string())
    );
    assert!(matches!(
        value.details,
        DimensionStyleDetails::V5 { extra: Some(_), .. }
    ));

    let mut minor_zero_body = Vec::new();
    minor_zero_body.extend([0; 16]);
    minor_zero_body.extend(0_i32.to_le_bytes());
    minor_zero_body.extend(0_i32.to_le_bytes());
    minor_zero_body.extend(4_i32.to_le_bytes());
    minor_zero_body.extend(0.0_f64.to_le_bytes());
    minor_zero_body.extend(0.0_f64.to_le_bytes());
    minor_zero_body.extend(1.0_f64.to_le_bytes());
    minor_zero_body.extend(1.0_f64.to_le_bytes());
    minor_zero_body.extend([0xee, 0xff]);
    let minor_zero = anonymous(0, &minor_zero_body);
    let mut minor_zero_descriptor = descriptor;
    minor_zero_descriptor.payload_range = 0..minor_zero.len();
    let minor_zero = parse_v5_dimension_style_extra(
        &minor_zero,
        &minor_zero_descriptor,
        ArchiveVersion::V5,
        2.0,
    )
    .expect("V5 dimension-style extra minor zero");
    assert_eq!(minor_zero.mask_color, [255, 255, 255, 0]);
    assert_eq!(minor_zero.dimension_scale, 1.0);

    let mut invalid = base;
    invalid[0] = 0x25;
    assert!(parse_v5_dimension_style(&invalid, 0..invalid.len(), 1.0, 322, None).is_err());
}

#[test]
fn user_string_owner_mapping_preserves_order_and_source_cleanup() {
    let geometry = anonymous(
        0,
        &[
            1_i32.to_le_bytes().as_slice(),
            anonymous(0, &[utf16("GeometryKey"), utf16("geometry value")].concat()).as_slice(),
        ]
        .concat(),
    );
    let attributes = anonymous(
        0,
        &[
            3_i32.to_le_bytes().as_slice(),
            anonymous(0, &[utf16("$TEMP_OBJECT$"), utf16("temporary")].concat()).as_slice(),
            anonymous(
                0,
                &[utf16("AttributeKey"), utf16("attribute value")].concat(),
            )
            .as_slice(),
            anonymous(0, &[utf16("MixedCase"), utf16("mixed value")].concat()).as_slice(),
        ]
        .concat(),
    );
    let geometry_start = 0;
    let attributes_start = geometry.len();
    let data = [geometry, attributes].concat();
    let descriptor = |range: Range<usize>| {
        UserdataDescriptor::Known(ClassUserdata {
            range: range.clone(),
            version: (2, 2),
            class_uuid: USER_STRING_LIST,
            item_uuid: USER_STRING_LIST,
            copy_count: 1,
            transform_range: 0..0,
            application_uuid: None,
            save_context: None,
            payload_range: range,
        })
    };
    let attribute_descriptor = |range: Range<usize>| {
        AttributeUserdataDescriptor::Known(AttributeUserdata {
            range,
            class_uuid: USER_STRING_LIST,
            item_uuid: USER_STRING_LIST,
            application_uuid: None,
            writer_version: None,
            payload_range: attributes_start..data.len(),
        })
    };
    let mut losses = Vec::new();
    let (geometry_values, attribute_values) = first_user_string_records(
        &data,
        ArchiveVersion::V8,
        &[descriptor(geometry_start..attributes_start)],
        &[attribute_descriptor(attributes_start..data.len())],
        42,
        &mut losses,
    );
    assert!(losses.is_empty());
    assert_eq!(geometry_values.len(), 1);
    assert_eq!(geometry_values[0].key, "GeometryKey");
    assert_eq!(geometry_values[0].value, "geometry value");
    assert_eq!(
        attribute_values
            .iter()
            .map(|value| (value.key.as_str(), value.value.as_str()))
            .collect::<Vec<_>>(),
        [
            ("AttributeKey", "attribute value"),
            ("MixedCase", "mixed value")
        ]
    );
}

#[test]
fn model_component_readers_follow_source_unknown_mask_and_status_rules() {
    let mut legacy_body = 0x28_u32.to_le_bytes().to_vec();
    legacy_body.extend(utf16("mask-compatible"));
    legacy_body.extend([0xaa, 0xbb]);
    let legacy = anonymous(0, &legacy_body);
    let mut legacy_reader = BoundedReader::new(&legacy, 0, legacy.len()).unwrap();
    let legacy_component = component(&legacy, &mut legacy_reader, ArchiveVersion::V8)
        .expect("unknown legacy mask bit is ignored");
    assert_eq!(legacy_component.index, None);
    assert!(legacy_component.id.is_nil());
    assert_eq!(legacy_component.name, "mask-compatible");
    assert_eq!(legacy_reader.remaining(), 0);

    let modern = model_attributes_status_chunk([3, 2, 3, 2, 1], "status-compatible", &[0xcc, 0xdd]);
    let mut modern_reader = BoundedReader::new(&modern, 0, modern.len()).unwrap();
    let modern_component = component(&modern, &mut modern_reader, ArchiveVersion::V8)
        .expect("unknown modern status values are ignored");
    assert_eq!(modern_component.index, None);
    assert!(modern_component.id.is_nil());
    assert_eq!(modern_component.name, "status-compatible");
    assert_eq!(modern_reader.remaining(), 0);
}

#[test]
fn group_preserves_component_identity() {
    let mut bytes = vec![0x1f];
    bytes.extend(7_i32.to_le_bytes());
    bytes.extend(utf16("fixtures"));
    bytes.extend([0x44; 16]);
    bytes.extend([0xaa, 0xbb]);
    let group = parse_group(&bytes, 0..bytes.len(), 120).expect("required invariant");
    assert_eq!(group.archive_index, 7);
    assert_eq!(group.name, "fixtures");
    assert_eq!(
        group.source_uuid.as_deref(),
        Some("44444444-4444-4444-4444-444444444444")
    );
    assert_eq!(group.source_offset, 120);
}

#[test]
fn duplicate_group_source_ids_are_disambiguated_without_rewriting_source_fields() {
    let mut bytes = vec![0x1f];
    bytes.extend(7_i32.to_le_bytes());
    bytes.extend(utf16("fixtures"));
    bytes.extend([0x44; 16]);
    let first = parse_group(&bytes, 0..bytes.len(), 120).expect("first group");
    let second = parse_group(&bytes, 0..bytes.len(), 240).expect("second group");
    let mut groups = vec![first, second];
    assert_eq!(disambiguate_group_ids(&mut groups), 2);
    assert_ne!(groups[0].id, groups[1].id);
    assert_eq!(groups[0].archive_index, 7);
    assert_eq!(groups[1].archive_index, 7);
    assert_eq!(groups[0].source_uuid, groups[1].source_uuid);
    assert!(groups[0].id.contains("source-offset-0000000000000078"));
    assert!(groups[1].id.contains("source-offset-00000000000000f0"));
}

fn light_payload(packed: u8, hotspot: f64) -> Vec<u8> {
    let mut bytes = vec![packed];
    bytes.extend(1_i32.to_le_bytes());
    bytes.extend(4_i32.to_le_bytes());
    bytes.extend(0.5_f64.to_le_bytes());
    bytes.extend(20.0_f64.to_le_bytes());
    bytes.extend([1, 2, 3, 4]);
    bytes.extend([5, 6, 7, 8]);
    bytes.extend([9, 10, 11, 12]);
    for value in [0.0_f64, 0.0, -1.0, 1.0, 2.0, 3.0] {
        bytes.extend(value.to_le_bytes());
    }
    bytes.extend(0.25_f64.to_le_bytes());
    bytes.extend(16.0_f64.to_le_bytes());
    for value in [1.0_f64, 0.0, 0.0] {
        bytes.extend(value.to_le_bytes());
    }
    bytes.extend(0.75_f64.to_le_bytes());
    bytes.extend(3_i32.to_le_bytes());
    bytes.extend([0x55; 16]);
    bytes.extend(utf16("key"));
    for value in [4.0_f64, 0.0, 0.0, 0.0, 5.0, 0.0] {
        bytes.extend(value.to_le_bytes());
    }
    bytes.extend(hotspot.to_le_bytes());
    bytes.extend([0xaa, 0xbb]);
    bytes
}

fn texture_payload(minor: i32, suffix: &[u8]) -> Vec<u8> {
    let mut body = vec![0x11; 16];
    body.extend(7_u32.to_le_bytes());
    body.extend(utf16("texture.png"));
    body.push(1);
    for value in 1..=7_u32 {
        body.extend(value.to_le_bytes());
    }
    for index in 0..16 {
        body.extend((if index % 5 == 0 { 1.0_f64 } else { 0.0 }).to_le_bytes());
    }
    body.extend([1, 2, 3, 4]);
    body.extend([5, 6, 7, 8]);
    body.extend([0x22; 16]);
    for value in [0.25_f64, 1.25] {
        body.extend(value.to_le_bytes());
    }
    for value in [0.1_f64, 0.2, 0.3, 0.4, 0.5] {
        body.extend(value.to_le_bytes());
    }
    body.extend([9, 10, 11, 12]);
    for value in [0.6_f64, 0.7, 0.8, 0.9] {
        body.extend(value.to_le_bytes());
    }
    body.extend(4_i32.to_le_bytes());
    if minor >= 1 {
        body.extend(crate::test_support::test_dump::file_reference(
            ArchiveVersion::V8,
            "/full/source.3dm",
            "source.3dm",
        ));
    }
    if minor >= 2 {
        body.push(1);
    }
    body.extend(suffix);
    anonymous(minor, &body)
}

#[test]
fn light_scales_spatial_values_but_not_direction_or_angles() {
    let bytes = light_payload(0x1f, 0.8);
    let light = parse_light(&bytes, 0..bytes.len(), 10.0, 0, None).expect("required invariant");
    assert_eq!(light.location, [10.0, 20.0, 30.0]);
    assert_eq!(light.direction, [0.0, 0.0, -1.0]);
    assert_eq!(light.length, [40.0, 0.0, 0.0]);
    assert_eq!(light.spot_angle_degrees, 0.25);
    assert_eq!(light.spot_exponent, 16.0);
    assert_eq!(light.hotspot, 0.8);
}

#[test]
fn light_preserves_unset_hotspot_for_exponent_interface() {
    let bytes = light_payload(0x12, -1.234_321_012_343_21e308);
    let light = parse_light(&bytes, 0..bytes.len(), 1.0, 0, None).expect("required invariant");
    assert_eq!(light.spot_angle_degrees, 0.25);
    assert_eq!(light.spot_exponent, 16.0);
    assert_eq!(light.hotspot, -1.234_321_012_343_21e308);
}

/// One legacy (outer version 2.0) material whose transparent color is the
/// bogus [128, 128, 128] that the pre-2009 rule replaces with `diffuse`.
fn legacy_material_bytes(diffuse: [u8; 4]) -> Vec<u8> {
    let mut body = [[0x11; 16].as_slice(), 2_i32.to_le_bytes().as_slice()].concat();
    body.extend(utf16("steel"));
    body.extend([0x22; 16]);
    for color in [
        [1, 2, 3, 4],
        diffuse,
        [9, 10, 11, 12],
        [13, 14, 15, 16],
        [17, 18, 19, 20],
        [128, 128, 128, 24],
    ] {
        body.extend(color);
    }
    for value in [1.5_f64, 0.25, 64.0, 0.1] {
        body.extend(value.to_le_bytes());
    }
    body.extend(anonymous(0, &0_i32.to_le_bytes()));
    body.extend(utf16(""));
    body.extend(0_i32.to_le_bytes());
    body.extend([1, 0]);
    body.push(1);
    for value in [0.9_f64, 0.8, 1.4] {
        body.extend(value.to_le_bytes());
    }
    body.extend([0x33; 16]);
    body.push(1);
    body.extend([0xaa, 0xbb]);
    let inner = anonymous(7, &body);
    let mut bytes = vec![0x20];
    bytes.extend(inner);
    bytes
}

#[test]
fn legacy_material_preserves_core_appearance_and_switches() {
    let bytes = legacy_material_bytes([5, 6, 7, 8]);
    let material = parse_material(
        &bytes,
        0..bytes.len(),
        ArchiveVersion::V5,
        Some(200_912_009),
        0,
        None,
        &mut Vec::new(),
    )
    .expect("required invariant");
    assert_eq!(material.name, "steel");
    assert_eq!(material.diffuse, [5, 6, 7, 8]);
    assert_eq!(material.transparent, material.diffuse);
    assert_eq!(material.index_of_refraction, 1.5);
    assert!(material.shareable);
    assert!(!material.disable_lighting);
}

/// The pre-2009 transparency substitution rests on the stamp.
///
/// The same bytes give diffuse under an old stamp and the stored
/// [128, 128, 128] under none, so an unstamped archive emits a color the
/// archive does not vouch for - unless diffuse already equals the stored
/// color, where both readings agree and nothing was substituted.
#[test]
fn unstamped_legacy_material_charges_the_transparency_stamp_loss() {
    let bytes = legacy_material_bytes([5, 6, 7, 8]);
    let mut losses = Vec::new();
    let material = parse_material(
        &bytes,
        0..bytes.len(),
        ArchiveVersion::V5,
        None,
        0,
        None,
        &mut losses,
    )
    .expect("legacy material without a writer stamp");
    assert_eq!(material.transparent, [128, 128, 128, 24]);
    assert_eq!(losses.len(), 1, "{losses:?}");
    assert_eq!(
        losses[0].code.local_code(),
        RhinoLossCode::SourceWriterStampUnverified.code()
    );

    let mut stamped_losses = Vec::new();
    let stamped = parse_material(
        &bytes,
        0..bytes.len(),
        ArchiveVersion::V5,
        Some(200_912_010),
        0,
        None,
        &mut stamped_losses,
    )
    .expect("legacy material with a modern writer stamp");
    assert_eq!(stamped.transparent, [128, 128, 128, 24]);
    assert!(stamped_losses.is_empty(), "{stamped_losses:?}");

    // Both readings give the same color, so no color was substituted.
    let agreeing = legacy_material_bytes([128, 128, 128, 24]);
    let mut agreeing_losses = Vec::new();
    let material = parse_material(
        &agreeing,
        0..agreeing.len(),
        ArchiveVersion::V5,
        None,
        0,
        None,
        &mut agreeing_losses,
    )
    .expect("legacy material whose diffuse equals its transparent color");
    assert_eq!(material.transparent, material.diffuse);
    assert!(agreeing_losses.is_empty(), "{agreeing_losses:?}");
}

fn v2_v3_material_payload(minor: u8) -> Vec<u8> {
    let mut bytes = vec![0x10 | minor];
    for color in [
        [1, 2, 3, 4],
        [5, 6, 7, 8],
        [9, 10, 11, 12],
        [13, 14, 15, 16],
    ] {
        bytes.extend(color);
    }
    for value in [64.0_f64, 0.25] {
        bytes.extend(value.to_le_bytes());
    }
    bytes.extend([21, 22, 23, 24]);
    bytes.extend([25, 26, 27, 28]);
    bytes.extend(3_i16.to_le_bytes());
    bytes.extend(4_i16.to_le_bytes());
    bytes.extend(0.5_f64.to_le_bytes());
    bytes.extend(1.5_f64.to_le_bytes());

    bytes.extend(utf16("bitmap.png"));
    bytes.extend(2_i32.to_le_bytes());
    bytes.extend(31_i32.to_le_bytes());
    bytes.extend(utf16("bump.png"));
    bytes.extend(1_i32.to_le_bytes());
    bytes.extend(32_i32.to_le_bytes());
    bytes.extend(2.5_f64.to_le_bytes());
    bytes.extend(utf16("environment.png"));
    bytes.extend(9_i32.to_le_bytes());
    bytes.extend(33_i32.to_le_bytes());

    bytes.extend(7_i32.to_le_bytes());
    bytes.extend([0x44; 16]);
    bytes.extend(utf16("obsolete library"));
    bytes.extend(utf16("old steel"));
    if minor >= 1 {
        bytes.extend([0x55; 16]);
        bytes.extend([41, 42, 43, 44]);
        bytes.extend([45, 46, 47, 48]);
        bytes.extend(1.45_f64.to_le_bytes());
    }
    bytes.extend([0xaa, 0xbb]);
    bytes
}

#[test]
fn v2_v3_material_reads_direct_prefix_and_legacy_textures() {
    for archive in [ArchiveVersion::V2, ArchiveVersion::V3] {
        let bytes = v2_v3_material_payload(1);
        let material = parse_material(
            &bytes,
            0..bytes.len(),
            archive,
            None,
            77,
            None,
            &mut Vec::new(),
        )
        .expect("V2/V3 material payload");
        assert_eq!(material.archive_index, Some(7));
        assert_eq!(material.name, "old steel");
        assert_eq!(
            material.plugin_uuid,
            Uuid::from_wire([0x44; 16]).to_string()
        );
        assert_eq!(material.ambient, [1, 2, 3, 4]);
        assert_eq!(material.diffuse, [5, 6, 7, 8]);
        assert_eq!(material.shine, 64.0);
        assert_eq!(material.transparency, 0.25);
        assert_eq!(material.reflection, [41, 42, 43, 44]);
        assert_eq!(material.transparent, [45, 46, 47, 48]);
        assert_eq!(material.index_of_refraction, 1.45);
        assert_eq!(
            material.source_uuid,
            Some(Uuid::from_wire([0x55; 16]).to_string())
        );
        assert_eq!(material.textures.len(), 3);
        assert_eq!(material.textures[0].legacy_file_path, "bitmap.png");
        assert_eq!(material.textures[0].texture_type, 1);
        assert_eq!(material.textures[0].mode, 2);
        assert_eq!(material.textures[1].legacy_file_path, "bump.png");
        assert_eq!(material.textures[1].texture_type, 2);
        assert_eq!(material.textures[1].mode, 1);
        assert_eq!(material.textures[1].bump_scale, [0.0, 2.5]);
        assert_eq!(material.textures[2].legacy_file_path, "environment.png");
        assert_eq!(material.textures[2].texture_type, 86);
        assert_eq!(material.textures[2].mode, 1);
        assert_eq!(material.textures[0].source_offset, 77);
        assert_eq!(material.textures[0].wrap, [0, 0, 0]);
        assert_eq!(material.textures[0].uvw_transform[0], [1.0, 0.0, 0.0, 0.0]);
    }
}

#[test]
fn v2_v3_material_minor_zero_uses_source_defaults_without_fabricating_identity() {
    let bytes = v2_v3_material_payload(0);
    let material = parse_material(
        &bytes,
        0..bytes.len(),
        ArchiveVersion::V2,
        None,
        77,
        None,
        &mut Vec::new(),
    )
    .expect("V2 minor-zero material payload");
    assert_eq!(material.id, "rhino:presentation:material#record-77");
    assert_eq!(material.source_uuid, None);
    assert_eq!(material.reflection, [255, 255, 255, 0]);
    assert_eq!(material.transparent, [255, 255, 255, 0]);
    assert_eq!(material.index_of_refraction, 1.0);
}

#[test]
fn physically_based_material_reads_versioned_prefix_and_suffix() {
    let bytes = physically_based_payload(2, &[0xaa, 0xbb]);
    let payload = chunk_at(&bytes, 0, bytes.len(), ArchiveVersion::V8, false)
        .expect("outer userdata payload");
    let material = parse_physically_based_material(&bytes, payload.body(), ArchiveVersion::V8)
        .expect("physically based material");
    assert_eq!(material.revision.version(), 2);
    assert_eq!(material.base_color, [0.1, 0.2, 0.3, 0.4]);
    assert_eq!(material.brdf, 1);
    assert_eq!(material.subsurface, 0.5);
    assert_eq!(material.subsurface_scattering_color, [0.6, 0.7, 0.8, 0.9]);
    assert_eq!(material.subsurface_scattering_radius, 1.0);
    assert_eq!(material.metallic, 2.0);
    assert_eq!(material.specular, 3.0);
    assert_eq!(material.specular_tint, 4.0);
    assert_eq!(material.roughness, 5.0);
    assert_eq!(material.anisotropic, 6.0);
    assert_eq!(material.anisotropic_rotation, 7.0);
    assert_eq!(material.sheen, 8.0);
    assert_eq!(material.sheen_tint, 9.0);
    assert_eq!(material.clearcoat, 10.0);
    assert_eq!(material.clearcoat_roughness, 11.0);
    assert_eq!(material.opacity_ior, 12.0);
    assert_eq!(material.opacity, 13.0);
    assert_eq!(material.opacity_roughness, 14.0);
    assert_eq!(material.emission, [0.11, 0.22, 0.33, 0.44]);
    assert_eq!(material.revision.alpha(), 0.77);
}

#[test]
fn physically_based_material_version_one_defaults_alpha() {
    let bytes = physically_based_payload(1, &[0xcc, 0xdd]);
    let payload = chunk_at(&bytes, 0, bytes.len(), ArchiveVersion::V8, false)
        .expect("outer userdata payload");
    let material = parse_physically_based_material(&bytes, payload.body(), ArchiveVersion::V8)
        .expect("version one physically based material");
    assert_eq!(material.revision.version(), 1);
    assert_eq!(material.revision.alpha(), 1.0);
}

fn legacy_rdk_payload(xml: &str, terminated: bool, suffix: &[u8]) -> Vec<u8> {
    let mut xml = xml.as_bytes().to_vec();
    if terminated {
        xml.push(0);
    }
    let mut bytes = 2_i32.to_le_bytes().to_vec();
    bytes.extend((xml.len() as i32).to_le_bytes());
    bytes.extend(xml);
    bytes.extend(suffix);
    bytes
}

fn legacy_rdk_descriptor(payload_range: Range<usize>) -> UserdataDescriptor {
    UserdataDescriptor::Known(ClassUserdata {
        range: payload_range.clone(),
        version: (2, 2),
        class_uuid: RDK_CLASS,
        item_uuid: RDK_USERDATA,
        copy_count: 1,
        transform_range: 0..0,
        application_uuid: Some(RDK_APPLICATION),
        save_context: Some(crate::objects::UserdataSaveContext {
            last_saved_as_goo: false,
            archive_version: 5,
            writer_version: 0,
        }),
        payload_range,
    })
}

#[test]
fn legacy_rdk_material_userdata_transfers_uuid_from_unterminated_xml() {
    let xml = "<xml><render-content-manager-data><material instance-id=\"AABBCCDD-EEFF-0011-2233-445566778899\" /></render-content-manager-data></xml>";
    let bytes = legacy_rdk_payload(xml, false, &[0xaa, 0xbb]);
    let userdata = [legacy_rdk_descriptor(0..bytes.len())];
    assert_eq!(
        legacy_rdk_material_instance_id(&bytes, &userdata),
        Some(Uuid::from_canonical([
            0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0xff, 0x00, 0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77,
            0x88, 0x99,
        ]))
    );
}

#[test]
fn legacy_rdk_material_userdata_ignores_terminated_callback_xml() {
    let xml = "<xml><render-content-manager-data><material instance-id=\"AABBCCDD-EEFF-0011-2233-445566778899\" /></render-content-manager-data></xml>";
    let bytes = legacy_rdk_payload(xml, true, &[]);
    let userdata = [legacy_rdk_descriptor(0..bytes.len())];
    assert_eq!(legacy_rdk_material_instance_id(&bytes, &userdata), None);
}

#[test]
fn legacy_rdk_material_userdata_rejects_malformed_xml() {
    let bytes = legacy_rdk_payload("<xml><render-content-manager-data><material>", false, &[]);
    let error = parse_legacy_rdk_material_instance_id(&bytes, 0..bytes.len())
        .expect_err("malformed legacy XML");
    assert!(matches!(error, FramingError::Structural { .. }));
}

#[test]
fn rendering_attributes_transfer_mapping_channels_and_flags() {
    let mut channel_body = 7_i32.to_le_bytes().to_vec();
    channel_body.extend([0x11; 16]);
    channel_body.extend((0..16).flat_map(|value| f64::from(value).to_le_bytes()));
    channel_body.extend([0xaa, 0xbb]);
    let channel = anonymous(1, &channel_body);
    let mut mapping_body = vec![0x22; 16];
    mapping_body.extend(1_i32.to_le_bytes());
    mapping_body.extend(channel);
    mapping_body.extend([0xcc, 0xdd]);
    let mapping = anonymous(0, &mapping_body);
    let mut rendering_body = 0_i32.to_le_bytes().to_vec();
    rendering_body.extend(1_i32.to_le_bytes());
    rendering_body.extend(mapping);
    rendering_body.extend([0, 0, 1]);
    rendering_body.extend([0xee, 0xff]);
    let bytes = anonymous(3, &rendering_body);

    let value = rendering_attributes(
        &bytes,
        Some(0..bytes.len()),
        ArchiveVersion::V8,
        settings::RenderingAttributesKind::Object,
    )
    .expect("object rendering attributes");
    assert!(value.materials.is_empty());
    assert_eq!(value.mappings.len(), 1);
    assert_eq!(
        value.mappings[0].plugin_uuid,
        Uuid::from_wire([0x22; 16]).to_string()
    );
    assert_eq!(value.mappings[0].channels.len(), 1);
    assert_eq!(value.mappings[0].channels[0].mapping_channel_id, 7);
    assert_eq!(
        value.mappings[0].channels[0].mapping_uuid,
        Uuid::from_wire([0x11; 16]).to_string()
    );
    let transform = value.mappings[0].channels[0]
        .object_transform
        .expect("minor-one mapping transform");
    assert_eq!(transform[0][0], 0.0);
    assert_eq!(transform[3][3], 15.0);
    assert_eq!(value.casts_shadows, Some(false));
    assert_eq!(value.receives_shadows, Some(false));
    assert_eq!(value.advanced_texture_preview, Some(true));
}

#[test]
fn rendering_material_reference_consumes_obsolete_mapping_channels() {
    let mut obsolete_channel_body = 7_i32.to_le_bytes().to_vec();
    obsolete_channel_body.extend([0x33; 16]);
    obsolete_channel_body.extend((0..16).flat_map(|value| f64::from(value).to_le_bytes()));
    let obsolete_channel = anonymous(1, &obsolete_channel_body);

    let mut material_body = vec![0x11; 16];
    material_body.extend([0x22; 16]);
    material_body.extend(1_i32.to_le_bytes());
    material_body.extend(obsolete_channel);
    material_body.extend([0x44; 16]);
    material_body.extend([3, 0, 0, 0]);
    material_body.extend([0xaa, 0xbb]);
    let material = anonymous(1, &material_body);

    let mut rendering_body = 1_i32.to_le_bytes().to_vec();
    rendering_body.extend(material);
    rendering_body.extend(0_i32.to_le_bytes());
    rendering_body.extend([1, 1, 0]);
    let bytes = anonymous(3, &rendering_body);

    let value = rendering_attributes(
        &bytes,
        Some(0..bytes.len()),
        ArchiveVersion::V8,
        settings::RenderingAttributesKind::Object,
    )
    .expect("obsolete material-reference mapping array");
    assert_eq!(value.materials.len(), 1);
    assert_eq!(
        value.materials[0].front_material_uuid,
        Uuid::from_wire([0x22; 16]).to_string()
    );
    assert_eq!(
        value.materials[0]
            .back_face
            .as_ref()
            .and_then(|value| value.back_material_uuid.clone()),
        Some(Uuid::from_wire([0x44; 16]).to_string())
    );
    assert_eq!(
        value.materials[0]
            .back_face
            .as_ref()
            .map(|value| value.material_source),
        Some(3)
    );
}

#[test]
fn texture_reads_minor_gates_before_future_suffix() {
    let bytes = texture_payload(2, &[0xaa, 0xbb]);
    let mut losses = Vec::new();
    let value = parse_texture(&bytes, 0..bytes.len(), ArchiveVersion::V8, 42, &mut losses)
        .expect("texture minor gates and suffix");
    assert_eq!(value.mapping_channel_id, 7);
    assert_eq!(value.legacy_file_path, "texture.png");
    assert_eq!(
        value
            .file_reference
            .as_ref()
            .map(|reference| reference.full_path.as_str()),
        Some("/full/source.3dm")
    );
    assert_eq!(value.treat_as_linear, Some(true));
    assert_eq!(value.source_offset, 42);
    assert!(losses.is_empty(), "{losses:?}");
}

#[test]
fn texture_file_reference_checksum_warning_is_located() {
    let mut bytes = texture_payload(2, &[]);
    let reference = crate::test_support::test_dump::file_reference(
        ArchiveVersion::V8,
        "/full/source.3dm",
        "source.3dm",
    );
    let reference_start = bytes
        .windows(reference.len())
        .position(|window| window == reference)
        .expect("texture contains the expected file-reference child");
    bytes[reference_start + reference.len() - 1] ^= 1;

    let mut losses = Vec::new();
    let value = parse_texture(&bytes, 0..bytes.len(), ArchiveVersion::V8, 42, &mut losses)
        .expect("a nested checksum mismatch does not prevent texture admission");
    assert!(value.file_reference.is_some());
    assert_eq!(losses.len(), 1, "{losses:?}");
    assert_eq!(
        losses[0].code,
        crate::loss::RhinoLossCode::IntegrityFailure.kind()
    );
    let provenance = losses[0]
        .provenance
        .as_ref()
        .expect("texture checksum loss is located");
    assert_eq!(provenance.offset, reference_start as u64);
    assert_eq!(
        provenance.tag.as_deref(),
        Some("PRESENTATION/TEXTURE/FILE_REFERENCE")
    );
}

#[test]
fn texture_array_closes_after_class_items_and_future_suffix() {
    let texture = crate::test_support::test_dump::class_wrapper(
        ArchiveVersion::V8,
        TEXTURE.to_wire(),
        &texture_payload(0, &[]),
    );
    let mut body = 1_i32.to_le_bytes().to_vec();
    body.extend(texture);
    body.extend([0xcc, 0xdd]);
    let bytes = anonymous(4, &body);
    let mut reader = BoundedReader::new(&bytes, 0, bytes.len()).expect("texture array bounds");
    let mut losses = Vec::new();
    let values = texture_array(&bytes, &mut reader, ArchiveVersion::V8, &mut losses)
        .expect("texture array child and suffix");
    assert_eq!(values.len(), 1);
    assert_eq!(values[0].legacy_file_path, "texture.png");
    assert_eq!(values[0].mapping_channel_id, 7);
    assert_eq!(reader.remaining(), 0);
    assert!(losses.is_empty(), "{losses:?}");
}

#[test]
fn texture_mapping_reads_nested_primitive_class_wrapper() {
    let mut body = crate::test_support::MESH_CLASS.to_vec();
    body.extend(6_u32.to_le_bytes());
    body.extend(1_u32.to_le_bytes());
    for index in 0..16 {
        body.extend((if index % 5 == 0 { 1.0_f64 } else { 0.0 }).to_le_bytes());
    }
    for index in 0..16 {
        body.extend((if index % 5 == 0 { 1.0_f64 } else { 0.0 }).to_le_bytes());
    }
    body.extend(utf16("custom mesh mapping"));
    body.extend(crate::test_support::test_dump::class_wrapper(
        ArchiveVersion::V8,
        crate::test_support::MESH_CLASS,
        &[],
    ));
    body.extend(0_u32.to_le_bytes());
    body.push(0);
    body.extend([0xaa, 0xbb]);
    let bytes = anonymous(1, &body);

    let mapping = parse_texture_mapping(&bytes, 0..bytes.len(), ArchiveVersion::V8, 42)
        .expect("texture mapping with primitive class wrapper")
        .value;
    assert_eq!(mapping.mapping_type, 6);
    assert_eq!(
        mapping.primitive_class_uuid,
        Some(Uuid::from_wire(crate::test_support::MESH_CLASS).to_string())
    );
}

#[test]
fn mapping_crc_cache_reads_version_one_and_bounded_suffix() {
    let mut bytes = 1_i32.to_le_bytes().to_vec();
    bytes.extend((-17_i32).to_le_bytes());
    bytes.extend([0xaa, 0xbb]);
    parse_mapping_crc_cache(&bytes, 0..bytes.len())
        .expect("version-one mapping cache with bounded suffix");
}

mod patterns;
