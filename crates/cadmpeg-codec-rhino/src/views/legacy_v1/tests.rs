// SPDX-License-Identifier: Apache-2.0
use super::{
    kind, parse, V1PresentationKind, TCODE_NAMED_CPLANE, TCODE_NAMED_VIEW, TCODE_VIEWPORT,
};
use crate::chunks::{chunk_at, ArchiveVersion, TCODE_ENDOFTABLE};
use crate::settings::MillimeterScale;

fn chunk(typecode: u32, body: &[u8]) -> Vec<u8> {
    let mut bytes = typecode.to_le_bytes().to_vec();
    bytes.extend(
        i32::try_from(body.len())
            .expect("fixture length")
            .to_le_bytes(),
    );
    bytes.extend(body);
    bytes
}

fn short(typecode: u32, value: i32) -> Vec<u8> {
    let mut bytes = typecode.to_le_bytes().to_vec();
    bytes.extend(value.to_le_bytes());
    bytes
}

fn doubles(values: &[f64]) -> Vec<u8> {
    values
        .iter()
        .flat_map(|value| value.to_le_bytes())
        .collect()
}

fn name(text: &str) -> Vec<u8> {
    let mut bytes = i32::try_from(text.len() + 1)
        .expect("fixture length")
        .to_le_bytes()
        .to_vec();
    bytes.extend(text.as_bytes());
    bytes.push(0);
    bytes
}

fn cplane() -> Vec<u8> {
    let mut body = doubles(&[1.0, 2.0, 3.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 5.0]);
    body.extend(10_i32.to_le_bytes());
    body.extend(4_i32.to_le_bytes());
    chunk(0x0200_0003, &body)
}

fn view(projection: i32, view_size: f64, distance: f64) -> Vec<u8> {
    let mut body = projection.to_le_bytes().to_vec();
    body.extend(1_i32.to_le_bytes());
    body.extend(doubles(&[
        4.0, 5.0, 6.0, 0.1, 0.2, 0.3, view_size, distance,
    ]));
    chunk(0x0200_0002, &body)
}

fn record(typecode: u32, children: &[Vec<u8>]) -> Vec<u8> {
    let mut body = children.concat();
    body.extend(short(TCODE_ENDOFTABLE, 0));
    chunk(typecode, &body)
}

fn parsed(bytes: &[u8], scale: MillimeterScale) -> serde_json::Value {
    let record = chunk_at(bytes, 0, bytes.len(), ArchiveVersion::V1, false).expect("V1 record");
    let kind = kind(record.typecode).expect("presentation record");
    serde_json::to_value(parse(bytes, &record, kind, scale).expect("typed V1 record"))
        .expect("record JSON")
}

#[test]
fn viewport_reads_every_child_field_and_scales_lengths() {
    let position = doubles(&[0.0, 0.25, 0.5, 1.0]);
    let mut trace = doubles(&[1.0, 1.0, 1.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 2.0, 3.0]);
    trace.extend(name("trace.bmp"));
    let bytes = record(
        TCODE_VIEWPORT,
        &[
            chunk(0x0200_0001, &name("Top")),
            cplane(),
            view(2, 4.0, 50.0),
            short(0x8200_0007, 0),
            short(0x8200_0008, 1),
            short(0x8200_0009, 0),
            chunk(0x0200_000a, &position),
            chunk(0x0200_000b, &trace),
            chunk(0x0200_0012, &name("wall.bmp")),
            short(0x8200_0011, 1),
            short(0x8200_0015, 1),
            chunk(0x0200_000d, &doubles(&[0.5])),
            chunk(0x0200_000c, &doubles(&[0.25])),
            chunk(0x0200_00ff, &[1, 2, 3]),
        ],
    );
    let value = parsed(&bytes, crate::test_support::millimeter_scale(10.0));
    assert_eq!(value["kind"], "viewport");
    assert_eq!(value["name"], "Top");
    assert_eq!(
        value["construction_plane"]["origin_mm"],
        serde_json::json!([10.0, 20.0, 30.0])
    );
    assert_eq!(value["construction_plane"]["grid_spacing_mm"], 50.0);
    assert_eq!(value["construction_plane"]["grid_line_count"], 10);
    assert_eq!(value["view"]["projection"], "perspective");
    assert_eq!(
        value["view"]["target_mm"],
        serde_json::json!([40.0, 50.0, 60.0])
    );
    assert_eq!(value["view"]["view_size_mm"], 40.0);
    assert_eq!(value["view"]["camera_distance_mm"], 500.0);
    assert_eq!(value["show_construction_grid"], false);
    assert_eq!(value["show_construction_axes"], true);
    assert_eq!(
        value["window_position"],
        serde_json::json!([0.0, 0.25, 0.5, 1.0])
    );
    assert_eq!(value["trace_image"]["width_mm"], 20.0);
    assert_eq!(value["trace_image"]["file_path"], "trace.bmp");
    assert_eq!(value["wallpaper_path"], "wall.bmp");
    assert_eq!(value["maximized"], true);
    assert_eq!(value["display_mode"], "shaded");
    assert_eq!(value["near_clip_distance_mm"], 5.0);
    assert_eq!(value["snap_size_mm"], 2.5);
}

#[test]
fn view_size_and_distance_outside_the_reader_bounds_take_defaults() {
    let bytes = record(TCODE_NAMED_VIEW, &[view(0, 0.0, -1.0)]);
    let value = parsed(&bytes, MillimeterScale::IDENTITY);
    assert_eq!(value["kind"], "named_view");
    assert_eq!(value["view"]["projection"], "parallel");
    assert_eq!(value["view"]["view_size_mm"], 0.125);
    assert_eq!(value["view"]["camera_distance_mm"], 100.0);
}

#[test]
fn named_construction_plane_ignores_view_children() {
    let bytes = record(
        TCODE_NAMED_CPLANE,
        &[
            chunk(0x0200_0001, &name("Front")),
            cplane(),
            view(2, 1.0, 1.0),
        ],
    );
    let value = parsed(&bytes, MillimeterScale::IDENTITY);
    assert_eq!(value["kind"], "named_construction_plane");
    assert_eq!(value["name"], "Front");
    assert!(value.get("view").is_none());
    assert_eq!(value["construction_plane"]["thick_line_frequency"], 4);
}

#[test]
fn a_record_without_its_end_marker_is_not_admitted() {
    let bytes = chunk(TCODE_VIEWPORT, &chunk(0x0200_0001, &name("Top")));
    let record = chunk_at(&bytes, 0, bytes.len(), ArchiveVersion::V1, false).expect("V1 record");
    assert!(parse(
        &bytes,
        &record,
        V1PresentationKind::Viewport,
        MillimeterScale::IDENTITY
    )
    .is_err());
}
