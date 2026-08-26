// SPDX-License-Identifier: Apache-2.0
//! Metadata-only fallback and retained-source-image decode tests.
#![allow(clippy::unwrap_used)]

use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeOptions};

use crate::test_support::*;
use crate::SldprtCodec;

fn direct_extrusion_operation_payload() -> Vec<u8> {
    let class = b"moExtrusion_c";
    let name = "Boss";
    let class_offset = 12usize;
    let name_offset = class_offset + 6 + class.len();
    let name_end = name_offset + 6 + name.encode_utf16().count() * 2;
    let object_id_offset = name_end + 8;
    let mut payload = vec![0; object_id_offset + 4];
    payload[..4].copy_from_slice(&1u32.to_le_bytes());
    payload[class_offset..class_offset + 4].copy_from_slice(&[0xff, 0xff, 0x01, 0x00]);
    payload[class_offset + 4..class_offset + 6]
        .copy_from_slice(&(class.len() as u16).to_le_bytes());
    payload[class_offset + 6..name_offset].copy_from_slice(class);
    payload[name_offset..name_offset + 5].copy_from_slice(&[0x04, 0x80, 0xff, 0xfe, 0xff]);
    payload[name_offset + 5] = name.encode_utf16().count() as u8;
    for (index, unit) in name.encode_utf16().enumerate() {
        let start = name_offset + 6 + index * 2;
        payload[start..start + 2].copy_from_slice(&unit.to_le_bytes());
    }
    payload[object_id_offset..object_id_offset + 4].copy_from_slice(&7u32.to_le_bytes());
    payload
}

#[test]
fn decode_surfaces_preview_and_solidworks_xml_metadata() {
    let mut png = vec![0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];
    png.extend_from_slice(&13u32.to_be_bytes());
    png.extend_from_slice(b"IHDR");
    png.extend_from_slice(&640u32.to_be_bytes());
    png.extend_from_slice(&480u32.to_be_bytes());
    png.extend_from_slice(&[8, 6, 0, 0, 1]);
    png.extend_from_slice(&0u32.to_be_bytes());

    let mut bmp = vec![0; 28];
    bmp[4..8].copy_from_slice(&40u32.to_le_bytes());
    bmp[8..12].copy_from_slice(&320i32.to_le_bytes());
    bmp[12..16].copy_from_slice(&(-200i32).to_le_bytes());
    bmp[16..18].copy_from_slice(&1u16.to_le_bytes());
    bmp[18..20].copy_from_slice(&8u16.to_le_bytes());
    bmp[20..24].copy_from_slice(&1u32.to_le_bytes());
    bmp[24..28].copy_from_slice(&12_345u32.to_le_bytes());

    let xml = br#"<?xml version="1.0"?><swSolidWorks swVersion="34000" swCreationTime="1700000000" swPath="C:\part.SLDPRT"><swModel id="1" swName="Part" swConfigurationName="Default"/><swConfigurationList><swConfiguration swID="0" swName="Default" swMostRecentConfiguration="NO" swConfigurationNeedsUpdate="YES" swConfigurationFlags="384" swConfigurationAlternateName="Default derived"/></swConfigurationList></swSolidWorks>"#;
    let mut source = outer_header();
    source.extend(make_block(0x10, "PreviewPNG", &png));
    source.extend(make_block(0x11, "PreviewBMP", &bmp));
    source.extend(make_block(0x12, "SolidWorksMetadata", xml));
    source.extend(make_block(
        0x20,
        "Contents/Config-0-Partition",
        &parasolid_with_body(
            "partition body",
            "SCH_SW_33103_11000",
            &owned_triangle(0, 700, 0.0),
        ),
    ));

    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode metadata fixture");
    let attributes = &decoded
        .ir()
        .source
        .as_ref()
        .expect("source metadata")
        .attributes;
    assert_eq!(attributes["png_preview_count"], "1");
    assert_eq!(attributes["png_preview_0_width"], "640");
    assert_eq!(attributes["png_preview_0_height"], "480");
    assert_eq!(attributes["png_preview_0_color_type"], "6");
    assert_eq!(attributes["bmp_thumbnail_count"], "1");
    assert_eq!(attributes["bmp_thumbnail_0_width"], "320");
    assert_eq!(attributes["bmp_thumbnail_0_height"], "-200");
    assert_eq!(attributes["bmp_thumbnail_0_compression"], "1");
    assert_eq!(attributes["sw_version"], "34000");
    assert_eq!(attributes["sw_creation_time_unix"], "1700000000");
    assert_eq!(attributes["sw_path"], r"C:\part.SLDPRT");
    assert_eq!(attributes["sw_name"], "Part");
    assert_eq!(attributes["sw_configuration_name"], "Default");
    assert_eq!(attributes["sw_configuration_0_needs_update"], "YES");
    assert_eq!(attributes["sw_configuration_0_most_recent"], "NO");
    assert_eq!(attributes["sw_configuration_0_flags"], "384");
    assert_eq!(
        attributes["sw_configuration_0_alternate_name"],
        "Default derived"
    );
}

#[test]
fn decode_without_geometry_falls_back_to_metadata() {
    let f = synthetic_sldprt();
    let mut cur = Cursor::new(f);
    let result = SldprtCodec
        .decode(&mut cur, &DecodeOptions::default())
        .unwrap();
    assert!(!result.report().geometry_transferred);
    assert_eq!(result.ir().native_unknowns("sldprt").unwrap().len(), 1);
    assert_eq!(result.source_fidelity().retained_records.len(), 2);
    assert!(result
        .source_fidelity()
        .retained_record("sldprt:file:source-image#0")
        .is_some_and(|record| record.data.is_some()));
    assert!(result
        .source_fidelity()
        .retained_records
        .iter()
        .any(|record| record.id != "sldprt:file:source-image#0" && record.sha256.len() == 64));
    let source = result.ir().source.as_ref().expect("source metadata");
    assert_eq!(source.format, "sldprt");
    assert_eq!(
        source
            .attributes
            .get("parasolid_schema")
            .map(String::as_str),
        Some("SCH_SW_33103_11000")
    );
}

#[test]
fn decode_explicit_empty_partition_and_deltas_as_an_empty_model() {
    let source = sldprt_with_partition_and_deltas(&[], &[]);
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();

    assert!(decoded.report().geometry_transferred);
    assert!(decoded.ir().model.bodies.is_empty());
    assert!(!decoded.report().losses.iter().any(|loss| {
        loss.message.contains("geometry was not transferred")
            || loss.message.contains("topology graph")
    }));
}

#[test]
fn metadata_fallback_binds_resolved_feature_scalars() {
    let mut source = synthetic_sldprt();
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Fillet Name="Round1" Type="Fillet"/></Keywords>"#,
    ));
    source.extend(make_block(
        0x45,
        "Contents/Config-0-ResolvedFeatures",
        &resolved_features_payload_with_names(&[0], &["Round1", "D1"]),
    ));

    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    assert!(!decoded.report().geometry_transferred);
    let feature = decoded
        .ir()
        .model
        .features
        .iter()
        .find(|feature| feature.name.as_deref() == Some("Round1"))
        .expect("metadata fillet feature");
    let parameter = decoded
        .ir()
        .model
        .parameters
        .iter()
        .find(|parameter| parameter.owner.as_ref() == Some(&feature.id) && parameter.name == "D1")
        .expect("metadata D1 parameter");
    assert_eq!(
        parameter.value,
        Some(cadmpeg_ir::features::ParameterValue::Length(
            cadmpeg_ir::features::Length(25.0)
        ))
    );
    assert!(parameter.native_ref.is_some());
    assert!(decoded.report().losses.iter().any(|loss| loss
        .message
        .contains("typed feature(s) retain native or unresolved required operation operands")));
}

#[test]
fn metadata_fallback_binds_resolved_extrusion_operation() {
    let mut source = synthetic_sldprt();
    source.extend(make_block(
        0x12,
        "SolidWorksMetadata",
        br#"<?xml version="1.0"?><swSolidWorks swVersion="12000"/>"#,
    ));
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Extrusion Name="Boss" Type="Localized" id="7"><Dimension Name="Depth">10mm</Dimension></Extrusion></Keywords>"#,
    ));
    source.extend(make_block(
        0x45,
        "Contents/Config-0-ResolvedFeatures",
        &direct_extrusion_operation_payload(),
    ));

    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    assert!(!decoded.report().geometry_transferred);
    let feature = decoded
        .ir()
        .model
        .features
        .iter()
        .find(|feature| feature.name.as_deref() == Some("Boss"))
        .expect("metadata extrusion feature");
    assert!(matches!(
        feature.definition,
        cadmpeg_ir::features::FeatureDefinition::Extrude {
            op: cadmpeg_ir::features::BooleanOp::Join,
            ..
        }
    ));
}

#[test]
fn retained_source_image_round_trips_byte_exactly() {
    let source = sldprt_with_body(&triangle_body());
    let mut cur = Cursor::new(source.clone());
    let result = SldprtCodec
        .decode(&mut cur, &DecodeOptions::default())
        .unwrap();
    assert!(!result.source_fidelity().annotations.provenance.is_empty());
    for coedge in &result.ir().model.coedges {
        assert!(result
            .ir()
            .model
            .coedges
            .iter()
            .any(|candidate| candidate.id == coedge.radial_next));
    }
    cadmpeg_test_support::roundtrip::verbatim_replay_holds(
        &SldprtCodec,
        "retained_source_image_round_trips_byte_exactly",
        &source,
    );
}
