// SPDX-License-Identifier: Apache-2.0

use super::super::*;
use cadmpeg_core::decode::ResourceDimension;
use cadmpeg_core::CodecError;
use cadmpeg_ir::hash::digest::Sha256Digest;

const STRING_ATOM: [u8; 16] = [
    0x6e, 0x10, 0xdd, 0x10, 0xc8, 0x2a, 0xd1, 0x11, 0x9b, 0x6b, 0x00, 0x80, 0xc7, 0xbb, 0x59, 0x97,
];
const LATE_ATOM: [u8; 16] = [
    0xe5, 0x5b, 0xb0, 0xe0, 0xbd, 0xfb, 0xd1, 0x11, 0xa3, 0xa7, 0x00, 0xaa, 0x00, 0xd1, 0x09, 0x54,
];

fn element(output: &mut Vec<u8>, kind: [u8; 16], base: u8, id: u32, body: &[u8]) {
    output.extend_from_slice(&u32::try_from(21 + body.len()).unwrap().to_le_bytes());
    output.extend_from_slice(&kind);
    output.push(base);
    output.extend_from_slice(&id.to_le_bytes());
    output.extend_from_slice(body);
}

fn binding_fixture(strings: &[Vec<u16>]) -> (Container<'static>, [DisplayJtSegment; 2], usize) {
    let mut inflated = Vec::new();
    inflated.extend_from_slice(&16_u32.to_le_bytes());
    inflated.extend_from_slice(&[0xff; 16]);
    let mut late = vec![1, 0];
    late.extend_from_slice(&0x4000_0000_u32.to_le_bytes());
    late.extend_from_slice(&1_u16.to_le_bytes());
    late.extend_from_slice(&[9; 16]);
    late.extend_from_slice(&7_u32.to_le_bytes());
    late.extend_from_slice(&12_u32.to_le_bytes());
    late.extend_from_slice(&1_u32.to_le_bytes());
    element(&mut inflated, LATE_ATOM, 8, 3, &late);
    for units in strings {
        let mut body = vec![1, 0, 0, 0, 0, 0x40, 1, 0];
        body.extend_from_slice(&u32::try_from(units.len()).unwrap().to_le_bytes());
        for unit in units {
            body.extend_from_slice(&unit.to_le_bytes());
        }
        element(&mut inflated, STRING_ATOM, 5, 4, &body);
    }
    inflated.extend_from_slice(&16_u32.to_le_bytes());
    inflated.extend_from_slice(&[0xff; 16]);
    inflated.extend_from_slice(&1_u16.to_le_bytes());
    inflated.extend_from_slice(&1_u32.to_le_bytes());
    inflated.extend_from_slice(&2_u32.to_le_bytes());
    inflated.extend_from_slice(&4_u32.to_le_bytes());
    inflated.extend_from_slice(&3_u32.to_le_bytes());
    inflated.extend_from_slice(&0_u32.to_le_bytes());
    let inflated_len = inflated.len();
    let compressed = crate::test_support::test_bytes::zlib_compress_at_level(&inflated, 1);
    let mut data = vec![0; 33];
    data.extend_from_slice(&compressed);
    let physical_size = cadmpeg_core::decode::u64_from_index(data.len());
    let container = Container {
        data: data.into(), physical_size,
        layout: crate::container::test_modern_layout(6),
        entries: vec![crate::container::DirEntry {
            name: "/Root/UG_PART/DisplayJT".into(),
            region: crate::container::Region::Header,
            body: crate::container::DirEntryBody::File { offset: 0, len: physical_size },
        }],
        fastload_table: None, segment_index: None, segment_wrappers: Vec::new(),
        indexed_section_layouts: std::sync::OnceLock::new(),
        om_section_cache: std::sync::OnceLock::new(),
    };
    let segment = |id: &str, segment_id, segment_type, byte_len| DisplayJtSegment {
        id: id.into(), document: "document".into(), toc_entry: "entry".into(),
        segment_id, segment_type, segment_byte_len: byte_len,
        payload_sha256: Sha256Digest::digest(&[]), compression: None, source_offset: 0,
    };
    (container, [
        segment("scene", [1; 16], 1, u32::try_from(physical_size).unwrap()),
        segment("shape", [9; 16], 7, 0),
    ], inflated_len)
}

#[test]
fn binding_properties_release_each_string_before_the_next_atom() {
    // U+0800 has three UTF8 bytes per two UTF16LE bytes. Three replaced
    // values would retain 294912 bytes; one live value needs 98304 bytes.
    let long = vec![0x0800; 32768];
    let key = "JT_LLPROP_SHAPEIMPL".encode_utf16().collect();
    let (container, segments, inflated_len) = binding_fixture(&[long.clone(), long.clone(), long, key]);
    // Inflated bytes remain live with eight framing slots, four buckets in
    // each map and one UTF8 string. A four-bucket map includes 15 alignment
    // bytes, four control bytes and a 16-byte control suffix. The smaller
    // expansion/framing relocation peaks precede the large string peak.
    let target_map = 4 * std::mem::size_of::<((&str, [u8; 16], u32), Option<&DisplayJtSegment>)>() + 35;
    let late_map = 4 * std::mem::size_of::<(u32, JtLateLoadedProperty)>() + 35;
    let key_map = 4 * std::mem::size_of::<(u32, bool)>() + 35;
    let peak = cadmpeg_core::decode::u64_from_index(
        inflated_len + target_map + 8 * std::mem::size_of::<ParsedJtElement<'_>>()
            + late_map + key_map + 3 * 32768,
    );
    crate::test_support::with_decode_context_over(container.data.as_ref(), |policy| {
        policy.limits.max_materialized_bytes = peak;
    }, |ctx| {
        let bindings = display_jt_shape_lod_bindings(ctx, &container, &segments)
            .expect("one string plus bounded indexes fits the source-derived peak");
        assert_eq!(bindings.len(), 1);
        assert_eq!(bindings[0].key, "JT_LLPROP_SHAPEIMPL");
        assert_eq!(bindings[0].shape_segment, "shape");
        assert_eq!(bindings[0].payload_object_id, 12);
        assert!(ctx.resource_refusal().is_none());
    });
    // This shorter member expands in one fixed chunk. Its first string is
    // the next peak, so a materialized refusal can reach that allocation.
    let (limited_container, limited_segments, _) = binding_fixture(&[vec![0x0800; 1024]]);
    let error = crate::test_support::resource_refusal_at(
        limited_container.data.as_ref(), ResourceDimension::MaterializedBytes,
        "retain DisplayJT string property",
        |ctx| display_jt_shape_lod_bindings(ctx, &limited_container, &limited_segments),
    );
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::MaterializedBytes
            && limit.operation == "retain DisplayJT string property"));
}

#[test]
fn binding_property_classification_keeps_last_value_and_strict_utf16() {
    let key: Vec<u16> = "JT_LLPROP_SHAPEIMPL".encode_utf16().collect();
    for (strings, expected_count) in [
        (vec![vec![u16::from(b'x')], key.clone()], 1),
        (vec![key.clone(), vec![u16::from(b'x')]], 0),
        (vec![vec![0xd800], key], 0),
    ] {
        let (container, segments, _) = binding_fixture(&strings);
        crate::test_support::with_decode_context(|ctx| {
            let bindings = display_jt_shape_lod_bindings(ctx, &container, &segments).unwrap();
            assert_eq!(bindings.len(), expected_count);
            assert!(ctx.resource_refusal().is_none());
        });
    }
}
