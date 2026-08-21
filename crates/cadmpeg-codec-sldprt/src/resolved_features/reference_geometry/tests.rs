// SPDX-License-Identifier: Apache-2.0
//! Fixed reference-plane frame and coordinate-system projection tests.
#![allow(clippy::unwrap_used)]

use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeOptions};

use crate::test_support::*;
use crate::SldprtCodec;

#[test]
fn decode_projects_fixed_reference_plane_frame() {
    use cadmpeg_ir::features::FeatureDefinition;
    use cadmpeg_ir::math::{Point3, Vector3};

    let mut resolved = resolved_feature_classes_with_ids(&[("moRefPlane_c", "Plano", 42)]);
    resolved.extend_from_slice(&[0xff, 0xff, 0x01, 0x00]);
    resolved.extend_from_slice(&("moFixedRefPlnData_c".len() as u16).to_le_bytes());
    resolved.extend_from_slice(b"moFixedRefPlnData_c");
    let mut frame = [0u8; 97];
    frame[0..8].copy_from_slice(&2.5f64.to_le_bytes());
    frame[8..16].copy_from_slice(&(-0.25f64).to_le_bytes());
    frame[16..24].copy_from_slice(&1.5f64.to_le_bytes());
    frame[24..32].copy_from_slice(&(-1.0f64).to_le_bytes());
    frame[32..40].copy_from_slice(&0.0f64.to_le_bytes());
    frame[40..48].copy_from_slice(&0.0f64.to_le_bytes());
    frame[48] = 1;
    frame[49..57].copy_from_slice(&0.0f64.to_le_bytes());
    frame[57..65].copy_from_slice(&0.0f64.to_le_bytes());
    frame[65..73].copy_from_slice(&(-1.0f64).to_le_bytes());
    frame[73..81].copy_from_slice(&0.0f64.to_le_bytes());
    frame[81..89].copy_from_slice(&(-1.0f64).to_le_bytes());
    frame[89..97].copy_from_slice(&0.0f64.to_le_bytes());
    resolved.extend_from_slice(&frame);

    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Feature Name="Plano" Type="Plano" id="42"/></Keywords>"#,
    ));
    source.extend(make_block(
        0x42,
        "Contents/Config-0-ResolvedFeatures",
        &resolved,
    ));
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    assert!(matches!(
        decoded.ir().model.features[0].definition,
        FeatureDefinition::DatumPlane {
            origin: Point3 {
                x: 2500.0,
                y: -250.0,
                z: 1500.0,
            },
            normal: Vector3 {
                x: -1.0,
                y: 0.0,
                z: 0.0,
            },
            u_axis: Vector3 {
                x: 0.0,
                y: 0.0,
                z: -1.0,
            },
        }
    ));
}

#[test]
fn decode_rejects_nonorthogonal_fixed_reference_plane_frame() {
    use cadmpeg_ir::features::FeatureDefinition;

    let mut resolved = resolved_feature_classes_with_ids(&[("moRefPlane_c", "Plane", 42)]);
    resolved.extend_from_slice(&[0xff, 0xff, 0x01, 0x00]);
    resolved.extend_from_slice(&("moFixedRefPlnData_c".len() as u16).to_le_bytes());
    resolved.extend_from_slice(b"moFixedRefPlnData_c");
    let mut frame = [0u8; 97];
    frame[24..32].copy_from_slice(&1.0f64.to_le_bytes());
    frame[48] = 1;
    frame[49..57].copy_from_slice(&1.0f64.to_le_bytes());
    frame[73..81].copy_from_slice(&1.0f64.to_le_bytes());
    resolved.extend_from_slice(&frame);

    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Feature Name="Plane" Type="Plane" id="42"/></Keywords>"#,
    ));
    source.extend(make_block(
        0x42,
        "Contents/Config-0-ResolvedFeatures",
        &resolved,
    ));
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    assert!(matches!(
        decoded.ir().model.features[0].definition,
        FeatureDefinition::DatumPlaneUnresolved
    ));
}

#[test]
fn incomplete_coordinate_system_projects_as_typed_unresolved() {
    use cadmpeg_ir::features::FeatureDefinition;

    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Feature Name="Fixture" Type="Coordinate System" id="28"/></Keywords>"#,
    ));
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();

    assert!(matches!(
        decoded.ir().model.features[0].definition,
        FeatureDefinition::DatumCoordinateSystemUnresolved
    ));
}
