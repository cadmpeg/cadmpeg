// SPDX-License-Identifier: Apache-2.0

use super::super::{parse_trace_image, parse_wallpaper};
use super::{assert_resource, serialized_plane, with_retained_limit};
use crate::chunks::ArchiveVersion;
use crate::test_support::test_dump::utf16_bytes;

#[test]
fn trace_image_path_refuses_retained_limit() {
    let mut bytes = vec![0x10];
    bytes.extend(utf16_bytes("trace.png"));
    bytes.extend(1.0_f64.to_le_bytes());
    bytes.extend(1.0_f64.to_le_bytes());
    serialized_plane(&mut bytes);
    let error = with_retained_limit(&bytes, 0, |ctx| {
        parse_trace_image(
            ctx,
            &bytes,
            0..bytes.len(),
            ArchiveVersion::V5,
            crate::settings::MillimeterScale::IDENTITY,
            &mut Vec::new(),
        )
        .expect_err("trace path exceeds retained limit")
    });
    assert_resource(&error, "Rhino trace image path");
}

#[test]
fn wallpaper_path_refuses_retained_limit() {
    let mut bytes = vec![0x10];
    bytes.extend(utf16_bytes("wallpaper.png"));
    bytes.push(1);
    let error = with_retained_limit(&bytes, 0, |ctx| {
        parse_wallpaper(
            ctx,
            &bytes,
            0..bytes.len(),
            ArchiveVersion::V5,
            &mut Vec::new(),
        )
        .expect_err("wallpaper path exceeds retained limit")
    });
    assert_resource(&error, "Rhino wallpaper path");
}
