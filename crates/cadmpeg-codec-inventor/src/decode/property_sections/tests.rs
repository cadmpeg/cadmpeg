// SPDX-License-Identifier: Apache-2.0

use cadmpeg_container::compound::CompoundSnapshot;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_ir::assets::AssetContent;
use cadmpeg_ir::source_fidelity::parts::PartOutput;
use cadmpeg_ir::CodecFormat;

use crate::native::{PropertyRecord, PropertySectionRecord, PropertySetRecord};
use crate::property_set::PropertySetState;
use crate::test_support::given_up_records;

fn padded(mut bytes: Vec<u8>) -> Vec<u8> {
    while !bytes.len().is_multiple_of(4) {
        bytes.push(0);
    }
    bytes
}

fn text(value: &str) -> Vec<u8> {
    let units = value.encode_utf16().chain([0]).collect::<Vec<_>>();
    let mut bytes = 31_u32.to_le_bytes().to_vec();
    bytes.extend_from_slice(
        &u32::try_from(units.len())
            .expect("the text unit count fits u32")
            .to_le_bytes(),
    );
    for unit in units {
        bytes.extend_from_slice(&unit.to_le_bytes());
    }
    padded(bytes)
}

fn blob(value: &[u8]) -> Vec<u8> {
    let mut bytes = 0x41_u32.to_le_bytes().to_vec();
    bytes.extend_from_slice(
        &u32::try_from(value.len())
            .expect("the blob length fits u32")
            .to_le_bytes(),
    );
    bytes.extend_from_slice(value);
    padded(bytes)
}

fn dictionary() -> Vec<u8> {
    let mut bytes = 2_u32.to_le_bytes().to_vec();
    for (id, name) in [(2_u32, "Title"), (17, "Preview")] {
        let units = name.encode_utf16().chain([0]).collect::<Vec<_>>();
        bytes.extend_from_slice(&id.to_le_bytes());
        bytes.extend_from_slice(
            &u32::try_from(units.len())
                .expect("the name unit count fits u32")
                .to_le_bytes(),
        );
        for unit in units {
            bytes.extend_from_slice(&unit.to_le_bytes());
        }
        bytes = padded(bytes);
    }
    bytes
}

fn section(properties: &[(u32, Vec<u8>)]) -> Vec<u8> {
    let mut values = Vec::new();
    let mut table = Vec::new();
    for (id, value) in properties {
        table.extend_from_slice(&id.to_le_bytes());
        table.extend_from_slice(
            &u32::try_from(8 + properties.len() * 8 + values.len())
                .expect("the property offset fits u32")
                .to_le_bytes(),
        );
        values.extend_from_slice(value);
    }
    let mut bytes = u32::try_from(8 + table.len() + values.len())
        .expect("the section size fits u32")
        .to_le_bytes()
        .to_vec();
    bytes.extend_from_slice(
        &u32::try_from(properties.len())
            .expect("the property count fits u32")
            .to_le_bytes(),
    );
    bytes.extend_from_slice(&table);
    bytes.extend_from_slice(&values);
    bytes
}

fn stream() -> Vec<u8> {
    let bad = section(&[
        (2, text("discarded title")),
        (17, blob(b"\x89PNG\r\n\x1a\nbad")),
        (255, text("Inventor Summary Information")),
    ]);
    let good = section(&[
        (0, dictionary()),
        (1, [2, 0, 0, 0, 0xb0, 4, 0, 0].to_vec()),
        (2, text("readable peer title")),
        (17, blob(b"\x89PNG\r\n\x1a\npeer")),
    ]);
    let mut bytes = 0xfffe_u16.to_le_bytes().to_vec();
    bytes.extend_from_slice(&1_u16.to_le_bytes());
    bytes.extend_from_slice(&0_u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 16]);
    bytes.extend_from_slice(&2_u32.to_le_bytes());
    bytes.extend_from_slice(&super::super::FMTID_SUMMARY_INFORMATION);
    bytes.extend_from_slice(&68_u32.to_le_bytes());
    bytes.extend_from_slice(&[0x55; 16]);
    bytes.extend_from_slice(
        &u32::try_from(68 + bad.len())
            .expect("the second section offset fits u32")
            .to_le_bytes(),
    );
    bytes.extend_from_slice(&bad);
    bytes.extend_from_slice(&good);
    bytes
}

#[test]
fn identity_failure_drops_its_real_records_and_asset_and_keeps_the_exact_peer() {
    let bytes = crate::test_support::test_fixtures::fixture_with_ufrx(&stream());
    let arena = DecodeArena::new();
    let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service())
        .expect("the fixture fits the service policy");
    let snapshot = CompoundSnapshot::new(&ctx, root).expect("the fixture is a compound file");
    let descriptors = crate::property_set::inventory(&ctx, &snapshot)
        .expect("the property set inventory succeeds");
    let descriptor = &descriptors[0];
    let PropertySetState::Parsed(parsed) = &descriptor.state else {
        panic!("valid bounded property syntax")
    };
    assert_eq!(parsed.sections.len(), 2);
    let mut output = PartOutput::new(CodecFormat::Inventor);
    assert!(crate::property_set::source_admission::stage_header(
        &ctx,
        &mut output,
        descriptor,
        parsed
    )
    .expect("the header stage succeeds"));
    let mut ordinal = 0;
    assert!(super::stage(&ctx, &mut output, descriptor, 0, &mut ordinal)
        .expect("the failed section stage succeeds")
        .is_none());
    assert_eq!(ordinal, 1, "the failed section formed its original preview");
    let peer = super::stage(&ctx, &mut output, descriptor, 1, &mut ordinal)
        .expect("the peer section stage succeeds")
        .expect("the peer section is formed");
    assert_eq!(ordinal, 2);
    let mut metadata = super::super::MetadataProjection::default();
    for property in &peer.properties {
        let id = format!(
            "inventor:property:value#{}-1-{}",
            descriptor.stream.directory_id(),
            property.id
        );
        metadata.consider(
            &peer.fmtid,
            property.id,
            property.name.as_deref(),
            property.value.scalar_text().as_deref(),
            &id,
        );
    }
    assert_eq!(metadata.title.as_deref(), Some("readable peer title"));
    let (model, native, fidelity, losses) = output.into_parts();
    assert_eq!(model.assets.len(), 1);
    assert_eq!(
        model.assets[0].id.as_str(),
        "inventor:document:asset#preview-1"
    );
    let peer_id = format!(
        "inventor:property:value#{}-1-17",
        descriptor.stream.directory_id()
    );
    assert_eq!(
        model.assets[0].native_ref.as_deref(),
        Some(peer_id.as_str())
    );
    assert!(
        matches!(&model.assets[0].content, AssetContent::Embedded { data } if data.as_bytes() == b"\x89PNG\r\n\x1a\npeer")
    );
    let namespace = native
        .namespace("inventor")
        .expect("the inventor namespace exists");
    let sets = namespace
        .arena_as::<PropertySetRecord>("property_sets")
        .expect("the property sets arena holds property set records");
    assert_eq!(sets.len(), 1);
    assert_eq!(sets[0].section_count, 2);
    let sections = namespace
        .arena_as::<PropertySectionRecord>("property_sections")
        .expect("the property sections arena holds property section records");
    assert_eq!(sections.len(), 1);
    assert_eq!(sections[0].ordinal, 1);
    assert_eq!(sections[0].property_count, 4);
    assert_eq!(sections[0].dictionary_entries, 2);
    assert_eq!(sections[0].code_page, Some(1200));
    let properties = namespace
        .arena_as::<PropertyRecord>("properties")
        .expect("the properties arena holds property records");
    assert_eq!(
        properties.iter().map(|p| p.property_id).collect::<Vec<_>>(),
        [0, 1, 2, 17]
    );
    assert!(properties.iter().all(|p| p.section_ordinal == 1));
    assert_eq!(properties[2].name.as_deref(), Some("Title"));
    assert_eq!(
        properties[2].scalar_value.as_deref(),
        Some("readable peer title")
    );
    assert_eq!(losses.len(), 1);
    assert_eq!(losses[0].code.to_string(), "decode/record.unreadable");
    assert_eq!(losses[0].severity, cadmpeg_ir::report::Severity::Error);
    assert!(losses[0]
        .message
        .contains("embedded property-set name does not match its FMTID"));
    assert_eq!(given_up_records(&fidelity).len(), 1);
}
