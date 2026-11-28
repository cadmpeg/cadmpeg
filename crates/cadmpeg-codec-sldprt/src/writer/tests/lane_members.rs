// SPDX-License-Identifier: Apache-2.0
//! A semantic write puts each feature-input lane back in the member it was
//! decoded from, and the decode of the output reads the source's lanes.

use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_test_support::EditableDecodeResult;

use crate::test_support::container::{make_block, native_envelope_fixture, outer_header, scan};
use crate::test_support::history::resolved_features_payload;
use crate::test_support::native::sldprt_native;
use crate::test_support::parasolid::{parasolid_with_body, triangle_body};
use crate::SldprtCodec;

const LEGACY: &str = "Contents/Config-0";
const RESOLVED: &str = "Contents/Config-0-ResolvedFeatures";

fn class_declaration(name: &str) -> Vec<u8> {
    let mut bytes = vec![0xff, 0xff, 0x01, 0x00];
    bytes.extend(u16::try_from(name.len()).unwrap().to_le_bytes());
    bytes.extend(name.as_bytes());
    bytes
}

/// A metadata record the decode reads from the member that holds it, so the
/// member is a patchable metadata owner.
fn with_bounding_envelope(mut payload: Vec<u8>) -> Vec<u8> {
    payload.extend(b"moBBoxCenterData_c");
    payload.extend(1u32.to_le_bytes());
    for value in [0.01f64, 0.02, -0.03, 0.04] {
        payload.extend(value.to_le_bytes());
    }
    payload
}

/// A part with one body and `members`, decoded, with its replay baseline
/// removed so the write takes the semantic path.
fn decoded_without_baseline(members: &[(&str, &[u8])]) -> EditableDecodeResult {
    let mut source = outer_header();
    source.extend(make_block(
        0x20,
        "Contents/Config-0-Partition",
        &parasolid_with_body("partition body", "SCH_SW_33103_11000", &triangle_body()),
    ));
    for (index, (name, payload)) in members.iter().enumerate() {
        source.extend(make_block(
            0x45 + u32::try_from(index).unwrap(),
            name,
            payload,
        ));
    }
    let decoded = SldprtCodec
        .decode(
            &mut Cursor::new(native_envelope_fixture(&source)),
            &DecodeOptions::default(),
        )
        .unwrap();
    let mut decoded = EditableDecodeResult::from(decoded);
    decoded.source_fidelity_mut().content_digest = None;
    decoded
}

/// Each lane as its family, configuration and payload, sorted.
fn lanes(ir: &cadmpeg_ir::CadIr) -> Vec<(String, Option<String>, Vec<u8>)> {
    let mut lanes = sldprt_native(ir)
        .feature_input_lanes
        .iter()
        .map(|lane| {
            let family = lane
                .id
                .strip_prefix("sldprt:feature-input:")
                .and_then(|rest| rest.split_once('#'))
                .unwrap()
                .0
                .to_owned();
            (
                family,
                lane.configuration.clone(),
                lane.native_payload.to_vec(),
            )
        })
        .collect::<Vec<_>>();
    lanes.sort();
    lanes
}

/// The payloads of the written members named `name`.
fn members(written: &[u8], name: &str) -> Vec<Vec<u8>> {
    scan(written)
        .blocks
        .iter()
        .filter(|block| block.section.name() == Some(name))
        .map(|block| block.payload.clone())
        .collect()
}

fn write_and_decode(decoded: &EditableDecodeResult) -> (Vec<u8>, cadmpeg_ir::CadIr) {
    let mut written = Vec::new();
    crate::test_support::plan_inherited_write(
        decoded.ir(),
        decoded.source_fidelity(),
        &mut written,
    )
    .unwrap();
    let back = SldprtCodec
        .decode(&mut Cursor::new(written.clone()), &DecodeOptions::default())
        .unwrap();
    (written, back.ir().clone())
}

/// A part whose configuration has both a legacy `Config-0` sketch-object lane
/// and a `Config-0-ResolvedFeatures` lane that owns metadata: each lane is
/// written to its own member. Written as `Config-0-ResolvedFeatures`, the
/// legacy lane replaced the other lane's bytes, and the decode of the output
/// lost every sketch that lane carried.
#[test]
fn a_legacy_lane_beside_a_resolved_features_lane_keeps_its_own_member() {
    let mut legacy = class_declaration("sgSketch");
    legacy.extend(class_declaration("sgPointHandle"));
    let resolved = with_bounding_envelope(resolved_features_payload(&[0]));
    let decoded = decoded_without_baseline(&[(RESOLVED, &resolved), (LEGACY, &legacy)]);
    let source = lanes(decoded.ir());
    assert_eq!(
        source
            .iter()
            .map(|(family, configuration, _)| (family.as_str(), configuration.as_deref()))
            .collect::<Vec<_>>(),
        [
            ("config-objects", Some("0")),
            ("resolved-features", Some("0"))
        ]
    );

    let (written, back) = write_and_decode(&decoded);
    assert_eq!(members(&written, RESOLVED), [resolved]);
    assert_eq!(members(&written, LEGACY), [legacy]);
    assert_eq!(lanes(&back), source);
}

/// A part whose only lane is a legacy `Config-0` stream that also owns
/// metadata, the form of compound files: the lane stays in `Config-0`.
/// Written as a new `Config-0-ResolvedFeatures` member beside the replayed
/// stream, it decoded back as two lanes.
#[test]
fn a_legacy_only_lane_is_written_to_its_source_stream() {
    let mut payload = resolved_features_payload(&[0]);
    payload.extend(class_declaration("sgSketch"));
    let payload = with_bounding_envelope(payload);
    let decoded = decoded_without_baseline(&[(LEGACY, &payload)]);
    assert!(decoded
        .ir()
        .model
        .attributes
        .iter()
        .any(|attribute| attribute.name == "bounding_envelope"));
    let source = lanes(decoded.ir());
    assert_eq!(source.len(), 1);
    assert_eq!(source[0].1.as_deref(), Some("0"));

    let (written, back) = write_and_decode(&decoded);
    assert_eq!(members(&written, LEGACY), [payload]);
    assert_eq!(members(&written, RESOLVED), Vec::<Vec<u8>>::new());
    assert_eq!(lanes(&back), source);
}

#[test]
fn a_renumbered_lane_keeps_its_stream_family() {
    use crate::resolved_features::names::renumbered_section;
    assert_eq!(
        renumbered_section("Contents/Config-1-ResolvedFeatures", "2").as_deref(),
        Some("Contents/Config-2-ResolvedFeatures")
    );
    assert_eq!(
        renumbered_section("Contents/Config-1", "12").as_deref(),
        Some("Contents/Config-12")
    );
    assert_eq!(renumbered_section("Contents/ResolvedFeatures", "0"), None);
}
