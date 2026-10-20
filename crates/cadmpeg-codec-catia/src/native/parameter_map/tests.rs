// SPDX-License-Identifier: Apache-2.0
//! Exact retention, source ownership, and checked sampled-map wire data.

use super::CatiaSampledParameterMap;
use crate::native::CatiaNative;

fn frame() -> Vec<u8> {
    let payload = crate::test_support::test_parameter_map::payload(17, true, -0.0);
    let mut bytes = vec![0xa8, 0x03, 0x25];
    bytes.extend_from_slice(
        &u32::try_from(payload.len())
            .expect("small payload")
            .to_le_bytes(),
    );
    bytes.extend_from_slice(&42_u32.to_le_bytes());
    bytes.extend_from_slice(&payload);
    bytes
}

#[test]
fn native_a8_sampled_map_retains_exact_payload_and_checked_wire() {
    let bytes = frame();
    let native = CatiaNative::decode(&bytes);
    let [map] = native.sampled_parameter_maps.as_slice() else {
        panic!("one complete map")
    };
    let wire = serde_json::to_value(map).expect("serialize map");
    assert_eq!(wire["object_id"], 42);
    assert_eq!(&*map.payload, &bytes[11..]);
    assert_eq!(
        serde_json::from_value::<CatiaSampledParameterMap>(wire.clone()).expect("load exact map"),
        *map
    );
    let mut wrong = wire.clone();
    wrong["data"]["reference_parameters"][0] = serde_json::json!(0.5);
    assert!(serde_json::from_value::<CatiaSampledParameterMap>(wrong).is_err());
    let mut wrong = wire;
    wrong["data"]["fit_tolerance"] = serde_json::json!(0.0);
    assert!(serde_json::from_value::<CatiaSampledParameterMap>(wrong).is_err());
    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    native.store(&mut namespace).expect("borrowed store");
    let mut owned = cadmpeg_ir::NativeNamespace::default();
    native
        .clone()
        .store_owned(&cadmpeg_test_support::service_decode_context(), &mut owned)
        .expect("owned store");
    assert_eq!(owned, namespace);
    assert_eq!(
        CatiaNative::load(&namespace).expect("load namespace"),
        native
    );
}

#[test]
fn native_a8_map_is_source_bounded_and_suppresses_consolidated_payload_markers() {
    let frame = frame();
    let mut bytes = vec![0xaa; 7];
    bytes.extend_from_slice(&frame);
    let mut wrapper = vec![0xa5, 0x03, 0x20];
    wrapper.extend_from_slice(
        &u32::try_from(frame.len())
            .expect("bounded payload")
            .to_le_bytes(),
    );
    wrapper.push(0x81);
    wrapper.extend_from_slice(&frame);
    bytes.extend_from_slice(&wrapper);
    let source_range = 7..bytes.len();
    let native =
        CatiaNative::decode_with_record_ranges(&bytes, std::slice::from_ref(&source_range));
    let [map] = native.sampled_parameter_maps.as_slice() else {
        panic!("one peer map")
    };
    assert_eq!(
        (map.byte_offset, map.source_offset, map.source_index),
        (7, 0, 0)
    );
    let truncated_range = 7..7 + frame.len() - 1;
    assert!(
        CatiaNative::decode_with_record_ranges(&bytes, std::slice::from_ref(&truncated_range))
            .sampled_parameter_maps
            .is_empty()
    );
}

#[test]
fn unresolved_a8_maps_create_no_neutral_geometry() {
    use cadmpeg_ir::codec::{Codec, DecodeOptions};
    let input = crate::test_support::test_container::standard_catpart_from_streams(&frame(), &[]);
    let decoded = crate::CatiaCodec
        .decode(&mut std::io::Cursor::new(input), &DecodeOptions::default())
        .expect("decode unresolved sampled map");
    let model = &decoded.ir().model;
    assert!(model.curves.is_empty() && model.procedural_curves.is_empty());
    assert!(model.surfaces.is_empty() && model.procedural_surfaces.is_empty());
    assert!(model.edges.is_empty() && model.faces.is_empty());
    assert_eq!(
        decoded
            .ir()
            .native
            .namespace("catia")
            .expect("native")
            .arenas()["sampled_parameter_maps"]
            .len(),
        1
    );
}

#[test]
fn native_a8_peer_map_survives_consolidated_header_bytes_in_previous_samples() {
    let mut payload = crate::test_support::test_parameter_map::payload(17, true, 0.01);
    let last_co_parameter = 46 + 17 * 16 - 8;
    payload[last_co_parameter..last_co_parameter + 5]
        .copy_from_slice(&[0xb2, 0x03, 0x25, 0xff, 0x81]);
    let mut bytes = vec![0xa8, 0x03, 0x25];
    bytes.extend_from_slice(
        &u32::try_from(payload.len())
            .expect("small payload")
            .to_le_bytes(),
    );
    bytes.extend_from_slice(&41_u32.to_le_bytes());
    bytes.extend_from_slice(&payload);
    let second_offset = bytes.len();
    bytes.extend_from_slice(&frame());
    let native = CatiaNative::decode(&bytes);
    assert_eq!(native.sampled_parameter_maps.len(), 2);
    assert_eq!(
        native.sampled_parameter_maps[1].byte_offset,
        u64::try_from(second_offset).expect("small offset")
    );
}

#[test]
fn native_a8_map_declines_unadmitted_object_frame_flags() {
    for flag in [0x13, 0x83] {
        let mut bytes = frame();
        bytes[1] = flag;
        assert!(CatiaNative::decode(&bytes)
            .sampled_parameter_maps
            .is_empty());
    }
}
