// SPDX-License-Identifier: Apache-2.0
//! Synthetic feature-history and `ResolvedFeatures` payload builders for crate tests.
#![allow(clippy::unwrap_used)]

use super::{
    arc_sketch_body, bridge, circular_sketch_body, coedge, edge_use, ellipse_carrier, loop_head,
    make_block, nurbs_sketch_body, parasolid_with_body, plane_carrier, sldprt_with_body,
    triangle_body, vertex_use, world_point, zlib,
};

pub(crate) fn sldprt_with_body_and_history(body: &[u8]) -> Vec<u8> {
    let mut f = sldprt_with_body(body);
    f.extend(make_block(0x42, "Contents/Keywords", br#"<Keywords Name="Bracket"><Configuration Name="Default" SourceIndex="0" Material="Steel" DisplayState="Shaded"/><Extrusion Name="Boss" Type="BossExtrude" id="7" Scope="Body1"><Dimension Name="Depth">12.5mm</Dimension><EquationDrivenCurve Name="Profile" id="8"/></Extrusion></Keywords>"#));
    f
}

pub(crate) fn resolved_features_payload(codes: &[u32]) -> Vec<u8> {
    resolved_features_payload_with_names(codes, &["Sketch1", "Boss-Extrude1", "D1"])
}

pub(crate) fn resolved_features_payload_with_names(codes: &[u32], names: &[&str]) -> Vec<u8> {
    resolved_features_payload_with_names_and_relation(codes, names, "sgPntPntDist")
}

pub(crate) fn resolved_feature_classes_with_ids(entries: &[(&str, &str, u32)]) -> Vec<u8> {
    let mut payload = Vec::new();
    for (class, name, object_id) in entries {
        payload.extend_from_slice(&[0xff, 0xff, 0x01, 0x00]);
        payload.extend_from_slice(&(class.len() as u16).to_le_bytes());
        payload.extend_from_slice(class.as_bytes());
        payload.extend_from_slice(&[0x04, 0x80, 0xff, 0xfe, 0xff, name.len() as u8]);
        for unit in name.encode_utf16() {
            payload.extend_from_slice(&unit.to_le_bytes());
        }
        payload.extend_from_slice(&[0; 8]);
        payload.extend_from_slice(&object_id.to_le_bytes());
    }
    payload
}

pub(crate) fn resolved_features_payload_with_names_and_relation(
    codes: &[u32],
    names: &[&str],
    relation_class: &str,
) -> Vec<u8> {
    resolved_features_payload_with_names_relation_and_scalar(codes, names, relation_class, 0.025)
}

pub(crate) fn resolved_features_payload_with_names_relation_and_scalar(
    codes: &[u32],
    names: &[&str],
    relation_class: &str,
    scalar_value: f64,
) -> Vec<u8> {
    let mut payload = Vec::new();
    for name in ["sgPointHandle", "sgLineHandle", "sgArcHandle"] {
        payload.extend_from_slice(&[0xff, 0xff, 0x01, 0x00]);
        payload.extend_from_slice(&(name.len() as u16).to_le_bytes());
        payload.extend_from_slice(name.as_bytes());
    }
    for name in names {
        if *name == "D1" {
            let class = relation_class;
            payload.extend_from_slice(&[0xff, 0xff, 0x01, 0x00]);
            payload.extend_from_slice(&(class.len() as u16).to_le_bytes());
            payload.extend_from_slice(class.as_bytes());
        }
        payload.extend_from_slice(&[0x04, 0x80, 0xff, 0xfe, 0xff, name.len() as u8]);
        for unit in name.encode_utf16() {
            payload.extend_from_slice(&unit.to_le_bytes());
        }
        if name.starts_with('D')
            && name[1..]
                .chars()
                .all(|character| character.is_ascii_digit())
        {
            payload.extend_from_slice(&[
                0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x40, 0xff, 0xff, 0xff, 0xff, 0x00, 0x00,
                0x00, 0x00, 0xff, 0xfe, 0xff, 0x00, 0x00, 0x00,
            ]);
            payload.extend_from_slice(&scalar_value.to_le_bytes());
            payload.extend_from_slice(&[
                0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
                0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x02,
                0x00, 0x00,
            ]);
            payload.extend_from_slice(&[0; 5]);
            for index in [0u16, 2] {
                payload.extend_from_slice(&[0xd6, 0x80]);
                payload.extend_from_slice(&index.to_le_bytes());
                payload.extend_from_slice(&[0xff; 4]);
                payload.extend_from_slice(&[0; 4]);
            }
        }
    }
    for (ordinal, code) in codes.iter().enumerate() {
        payload.extend_from_slice(&[0xff, 0xff, 0x1f, 0x00, 0x03]);
        let mut record = [0u8; 87];
        // o+5..13: shared-geometry header (eight 0xff bytes).
        record[..8].fill(0xff);
        // o+13..17: -1.0f32 geometry sentinel.
        record[8..12].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
        // o+17..21: native sketch-entity type code.
        record[12..16].copy_from_slice(&code.to_le_bytes());
        // o+21..27: profile-curve locus descriptor.
        record[16..22].copy_from_slice(&[0x00, 0x00, 0x04, 0x00, 0x02, 0x00]);
        // o+27..29: profile-curve role.
        record[22..24].copy_from_slice(&1u16.to_le_bytes());
        // o+31..39: -1.0f32 sentinel followed by the marker state descriptor.
        record[26..34].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00]);
        // o+48..56: state value.
        record[43..51].copy_from_slice(&(ordinal as f64 + 1.0).to_le_bytes());
        // o+70..80: local-link sentinel (zero selector padding, -1.0f64 marker).
        record[65..67].copy_from_slice(&[0, 0]);
        record[67..75].copy_from_slice(&(-1.0f64).to_le_bytes());
        // o+88..92: trailing local id.
        record[83..87].copy_from_slice(&((ordinal + 1) as u32).to_le_bytes());
        payload.extend_from_slice(&record);
    }
    payload
}

pub(crate) fn sldprt_with_body_and_resolved_features(body: &[u8], codes: &[u32]) -> Vec<u8> {
    let mut file = sldprt_with_body(body);
    file.extend(make_block(
        0x45,
        "Contents/Config-0-ResolvedFeatures",
        &resolved_features_payload(codes),
    ));
    file
}

pub(crate) fn sldprt_with_nested_sketch_profile(body: &[u8]) -> Vec<u8> {
    sldprt_with_nested_sketch_profiles(body, 1)
}

pub(crate) fn sldprt_with_nested_sketch_profiles(body: &[u8], count: usize) -> Vec<u8> {
    let mut file = sldprt_with_body(body);
    let mut payload = resolved_features_payload(&[0, 1, 1, 1]);
    for _ in 0..count {
        payload.extend(parasolid_with_body(
            "feature input sketch",
            "SCH_SW_33103_11000",
            &triangle_body(),
        ));
    }
    file.extend(make_block(
        0x45,
        "Contents/Config-0-ResolvedFeatures",
        &payload,
    ));
    file
}

pub(crate) fn sldprt_with_compact_relation_pair(body: &[u8]) -> Vec<u8> {
    sldprt_with_tagged_compact_relation(body, "sgPntPntDist", [[0xd6, 0x80]; 2])
}

pub(crate) fn sldprt_with_tagged_compact_relation(
    body: &[u8],
    relation_class: &str,
    operand_tags: [[u8; 2]; 2],
) -> Vec<u8> {
    sldprt_with_tagged_compact_relation_names(
        body,
        relation_class,
        operand_tags,
        &["Sketch1", "D1", "D2"],
    )
}

pub(crate) fn sldprt_with_tagged_compact_relation_names(
    body: &[u8],
    relation_class: &str,
    operand_tags: [[u8; 2]; 2],
    names: &[&str],
) -> Vec<u8> {
    let mut file = sldprt_with_body(body);
    let mut payload =
        resolved_features_payload_with_names_and_relation(&[0, 1, 1, 1], names, relation_class);
    let operand_offsets = payload
        .windows(2)
        .enumerate()
        .filter_map(|(offset, bytes)| (bytes == [0xd6, 0x80]).then_some(offset))
        .collect::<Vec<_>>();
    for (ordinal, offset) in operand_offsets.into_iter().enumerate() {
        payload[offset..offset + 2].copy_from_slice(&operand_tags[ordinal % 2]);
    }
    let d1_marker = [0x04, 0x80, 0xff, 0xfe, 0xff, 2, b'D', 0, b'1', 0];
    let d1_offset = payload
        .windows(d1_marker.len())
        .position(|window| window == d1_marker)
        .expect("D1 scalar name");
    payload[d1_offset + 69] = 1;
    payload.extend(parasolid_with_body(
        "feature input sketch",
        "SCH_SW_33103_11000",
        &triangle_body(),
    ));
    file.extend(make_block(
        0x45,
        "Contents/Config-0-ResolvedFeatures",
        &payload,
    ));
    file
}

pub(crate) fn sldprt_with_tagged_compact_relation_scalar(
    body: &[u8],
    relation_class: &str,
    operand_tags: [[u8; 2]; 2],
    scalar_value: f64,
) -> Vec<u8> {
    let mut file = sldprt_with_body(body);
    let mut payload = resolved_features_payload_with_names_relation_and_scalar(
        &[0, 1, 1, 1],
        &["Sketch1", "D1", "D2"],
        relation_class,
        scalar_value,
    );
    let operand_offsets = payload
        .windows(2)
        .enumerate()
        .filter_map(|(offset, bytes)| (bytes == [0xd6, 0x80]).then_some(offset))
        .collect::<Vec<_>>();
    for (ordinal, offset) in operand_offsets.into_iter().enumerate() {
        payload[offset..offset + 2].copy_from_slice(&operand_tags[ordinal % 2]);
    }
    let d1_marker = [0x04, 0x80, 0xff, 0xfe, 0xff, 2, b'D', 0, b'1', 0];
    let d1_offset = payload
        .windows(d1_marker.len())
        .position(|window| window == d1_marker)
        .expect("D1 scalar name");
    payload[d1_offset + 69] = 1;
    payload.extend(parasolid_with_body(
        "feature input sketch",
        "SCH_SW_33103_11000",
        &triangle_body(),
    ));
    file.extend(make_block(
        0x45,
        "Contents/Config-0-ResolvedFeatures",
        &payload,
    ));
    file
}

pub(crate) fn sldprt_with_compressed_nested_sketch_profile(body: &[u8]) -> Vec<u8> {
    let mut file = sldprt_with_body(body);
    let mut payload = resolved_features_payload(&[0, 1, 1, 1]);
    payload.extend_from_slice(&[
        0x23, 0x1d, 0xd5, 0x71, 0xda, 0x81, 0x48, 0xa2, 0xa8, 0x58, 0x98, 0xb2, 0x1b, 0x89, 0xef,
        0x99, 0, 0, 0, 0,
    ]);
    payload.extend(zlib(&parasolid_with_body(
        "feature input compressed sketch",
        "SCH_SW_33103_11000",
        &triangle_body(),
    )));
    file.extend(make_block(
        0x45,
        "Contents/Config-0-ResolvedFeatures",
        &payload,
    ));
    file
}

pub(crate) fn sldprt_with_nested_circular_sketch(body: &[u8]) -> Vec<u8> {
    let mut file = sldprt_with_body(body);
    let mut payload = resolved_features_payload(&[2]);
    payload.extend(parasolid_with_body(
        "feature input circular sketch",
        "SCH_SW_33103_11000",
        &circular_sketch_body(),
    ));
    file.extend(make_block(
        0x45,
        "Contents/Config-0-ResolvedFeatures",
        &payload,
    ));
    file
}

pub(crate) fn sldprt_with_nested_arc_sketch(body: &[u8]) -> Vec<u8> {
    let mut file = sldprt_with_body(body);
    let mut payload = resolved_features_payload(&[0, 2, 1, 1]);
    payload.extend(parasolid_with_body(
        "feature input arc sketch",
        "SCH_SW_33103_11000",
        &arc_sketch_body(),
    ));
    file.extend(make_block(
        0x45,
        "Contents/Config-0-ResolvedFeatures",
        &payload,
    ));
    file
}

pub(crate) fn sldprt_with_nested_elliptical_sketch(body: &[u8]) -> Vec<u8> {
    let mut sketch = Vec::new();
    sketch.extend(plane_carrier(
        100,
        [0.0, 0.0, 0.0],
        [0.0, 0.0, 1.0],
        [1.0, 0.0, 0.0],
    ));
    sketch.extend(ellipse_carrier(
        70,
        [0.0, 0.0, 0.0],
        [0.0, 0.0, 1.0],
        [0.0, 1.0, 0.0],
        2.0,
        1.0,
    ));
    sketch.extend(bridge(10, 20, 100));
    sketch.extend(loop_head(20, 30, 10));
    sketch.extend(coedge(30, 20, 30, 50, 0, 40, false));
    sketch.extend(edge_use(40, 70));
    sketch.extend(vertex_use(50, 60));
    sketch.extend(world_point(60, [0.0, 2.0, 0.0]));

    let mut file = sldprt_with_body(body);
    let mut payload = resolved_features_payload(&[2]);
    payload.extend(parasolid_with_body(
        "feature input elliptical sketch",
        "SCH_SW_33103_11000",
        &sketch,
    ));
    file.extend(make_block(
        0x45,
        "Contents/Config-0-ResolvedFeatures",
        &payload,
    ));
    file
}

pub(crate) fn sldprt_with_nested_nurbs_sketches(body: &[u8]) -> Vec<u8> {
    let mut file = sldprt_with_body(body);
    let mut payload = resolved_features_payload(&[1, 1]);
    payload.extend(parasolid_with_body(
        "feature input spline sketch",
        "SCH_SW_33103_11000",
        &nurbs_sketch_body(false),
    ));
    payload.extend(parasolid_with_body(
        "feature input rational spline sketch",
        "SCH_SW_33103_11000",
        &nurbs_sketch_body(true),
    ));
    file.extend(make_block(
        0x45,
        "Contents/Config-0-ResolvedFeatures",
        &payload,
    ));
    file
}
