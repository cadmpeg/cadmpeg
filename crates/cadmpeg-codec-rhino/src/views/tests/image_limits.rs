// SPDX-License-Identifier: Apache-2.0

use super::super::{parse_trace_image, parse_wallpaper};
use super::{assert_resource, serialized_plane, with_materialized_limit};
use crate::chunks::ArchiveVersion;
use crate::test_support::test_dump::utf16_bytes;

#[test]
fn trace_image_path_refuses_materialized_limit() {
    let mut bytes = vec![0x10];
    bytes.extend(utf16_bytes("trace.png"));
    bytes.extend(1.0_f64.to_le_bytes());
    bytes.extend(1.0_f64.to_le_bytes());
    serialized_plane(&mut bytes);
    let error = with_materialized_limit(&bytes, 0, |ctx| {
        {
            let context = ctx;
            let mut staging = context
                .reserve_scoped(0, "Rhino test view staging")
                .unwrap();
            parse_trace_image(
                context,
                &bytes,
                0..bytes.len(),
                ArchiveVersion::V5,
                crate::settings::MillimeterScale::IDENTITY,
                &mut Vec::new(),
                &mut staging,
            )
        }
        .expect_err("trace path exceeds materialized limit")
    });
    assert_resource(&error, "Rhino trace image path");
}

#[test]
fn wallpaper_path_refuses_materialized_limit() {
    let mut bytes = vec![0x10];
    bytes.extend(utf16_bytes("wallpaper.png"));
    bytes.push(1);
    let error = with_materialized_limit(&bytes, 0, |ctx| {
        {
            let context = ctx;
            let mut staging = context
                .reserve_scoped(0, "Rhino test view staging")
                .unwrap();
            parse_wallpaper(
                context,
                &bytes,
                0..bytes.len(),
                ArchiveVersion::V5,
                &mut Vec::new(),
                &mut staging,
            )
        }
        .expect_err("wallpaper path exceeds materialized limit")
    });
    assert_resource(&error, "Rhino wallpaper path");
}
