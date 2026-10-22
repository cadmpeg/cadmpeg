// SPDX-License-Identifier: Apache-2.0

use crate::native::{PropertyRecord, PropertySetRecord};
use crate::property_set::PropertySetState;
use cadmpeg_container::compound::CompoundSnapshot;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_ir::source_fidelity::parts::{PartOutput, PartState};
use cadmpeg_ir::CodecFormat;

fn stream(malformed_value: bool) -> Vec<u8> {
    let values = if malformed_value {
        vec![
            3_u32.to_le_bytes().to_vec(),
            [3_i32.to_le_bytes(), 7_i32.to_le_bytes()].concat(),
        ]
    } else {
        vec![[3_i32.to_le_bytes(), 17_i32.to_le_bytes()].concat()]
    };
    let mut section = vec![0; 8 + 8 * values.len()];
    section[4..8].copy_from_slice(&u32::try_from(values.len()).unwrap().to_le_bytes());
    for (index, value) in values.iter().enumerate() {
        section[8 + index * 8..12 + index * 8]
            .copy_from_slice(&u32::try_from(index + 2).unwrap().to_le_bytes());
        let offset = u32::try_from(section.len()).unwrap();
        section[12 + index * 8..16 + index * 8].copy_from_slice(&offset.to_le_bytes());
        section.extend_from_slice(value);
    }
    let size = u32::try_from(section.len()).unwrap();
    section[..4].copy_from_slice(&size.to_le_bytes());
    let mut bytes = vec![0; 48];
    bytes[..2].copy_from_slice(&0xfffe_u16.to_le_bytes());
    bytes[2..4].copy_from_slice(&1_u16.to_le_bytes());
    bytes[24..28].copy_from_slice(&1_u32.to_le_bytes());
    bytes[28..44].copy_from_slice(&super::super::FMTID_SUMMARY_INFORMATION);
    bytes[44..48].copy_from_slice(&48_u32.to_le_bytes());
    bytes.extend_from_slice(&section);
    bytes
}

#[test]
fn untrusted_header_count_gives_up_only_its_actual_stream_and_keeps_the_peer() {
    let mut bad = stream(false);
    bad[24..28].copy_from_slice(&u32::MAX.to_le_bytes());
    let bytes =
        crate::test_support::test_fixtures::fixture_with_property_set_peers(&bad, &stream(false));
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service()).unwrap();
    let snapshot = CompoundSnapshot::new(&ctx, root).unwrap();
    let descriptors = crate::property_set::inventory(&ctx, &snapshot).unwrap();
    assert_eq!(descriptors.len(), 2);
    assert!(matches!(
        descriptors[0].state,
        PropertySetState::Malformed(_)
    ));
    let mut output = PartOutput::new(CodecFormat::Inventor);
    let mut ordinal = 0;
    assert!(
        super::stage(&ctx, &mut output, &descriptors[0], &mut ordinal)
            .unwrap()
            .is_none()
    );
    let good = super::stage(&ctx, &mut output, &descriptors[1], &mut ordinal)
        .unwrap()
        .unwrap();
    assert!(good.header_formed);
    assert_eq!(
        good.sections[0].1.properties[0]
            .value
            .scalar_text()
            .as_deref(),
        Some("17")
    );
    let (_, native, fidelity, losses) = output.into_parts(&ctx).unwrap().into_parts();
    let namespace = native.namespace("inventor").unwrap();
    assert_eq!(
        namespace
            .arena_as::<PropertySetRecord>("property_sets")
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        namespace.arena_as::<PropertyRecord>("properties").unwrap()[0]
            .scalar_value
            .as_deref(),
        Some("17")
    );
    assert_eq!(losses.len(), 1);
    assert_eq!(losses[0].code.to_string(), "decode/record.unreadable");
    assert_eq!(
        fidelity
            .parts()
            .values()
            .filter(|part| matches!(part.state(), PartState::GivenUp {}))
            .count(),
        1
    );
}

#[test]
fn unreadable_bounded_value_keeps_its_section_and_exact_property_peer() {
    let bytes = crate::test_support::test_fixtures::fixture_with_ufrx(&stream(true));
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service()).unwrap();
    let snapshot = CompoundSnapshot::new(&ctx, root).unwrap();
    let descriptors = crate::property_set::inventory(&ctx, &snapshot).unwrap();
    assert!(matches!(
        descriptors[0].state,
        PropertySetState::Malformed(_)
    ));
    let mut output = PartOutput::new(CodecFormat::Inventor);
    let formed = super::stage(&ctx, &mut output, &descriptors[0], &mut 0)
        .unwrap()
        .unwrap();
    assert!(formed.header_formed);
    assert_eq!(formed.sections.len(), 1);
    assert_eq!(formed.sections[0].1.properties.len(), 1);
    assert_eq!(formed.sections[0].1.properties[0].id, 3);
    assert_eq!(
        formed.sections[0].1.properties[0]
            .value
            .scalar_text()
            .as_deref(),
        Some("7")
    );
    let (_, native, _, losses) = output.into_parts(&ctx).unwrap().into_parts();
    assert_eq!(
        native
            .namespace("inventor")
            .unwrap()
            .arena_as::<PropertyRecord>("properties")
            .unwrap()
            .len(),
        1
    );
    assert_eq!(losses.len(), 1);
    assert!(losses[0].message.contains(&format!(
        "inventor:property:value#{}-0-2",
        descriptors[0].stream.directory_id()
    )));
}

#[test]
fn cached_failure_does_not_override_actual_readable_source() {
    let bytes = crate::test_support::test_fixtures::fixture_with_ufrx(&stream(false));
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service()).unwrap();
    let snapshot = CompoundSnapshot::new(&ctx, root).unwrap();
    let mut descriptors = crate::property_set::inventory(&ctx, &snapshot).unwrap();
    descriptors[0].state = PropertySetState::Malformed("cached cue".into());
    let mut output = PartOutput::new(CodecFormat::Inventor);
    let formed = super::stage(&ctx, &mut output, &descriptors[0], &mut 0)
        .unwrap()
        .unwrap();
    assert_eq!(
        formed.sections[0].1.properties[0]
            .value
            .scalar_text()
            .as_deref(),
        Some("17")
    );
    let (_, _, _, losses) = output.into_parts(&ctx).unwrap().into_parts();
    assert!(losses.is_empty());
}
