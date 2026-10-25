// SPDX-License-Identifier: Apache-2.0
//! Source damage conserves every independent field through the shared comparator.

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodeDepth, DecodePolicy};
use cadmpeg_ir::source_fidelity::parts::PartState;
use cadmpeg_ir::{Codec, DecodeOptions};
use cadmpeg_test_support::comparison::{assert_decodes_agree, Allowances, DecodeSnapshot, Mode};
use cadmpeg_test_support::zip_fixture::{self, RawEntry};
use std::io::Cursor;

fn archive(document: &str) -> Vec<u8> {
    zip_fixture::archive(&[
        RawEntry::stored("Document.xml", document.as_bytes()),
        RawEntry::stored("independent.bin", b"unrelated retained member"),
    ])
}

#[test]
fn damaged_property_value_conserves_its_header_and_independent_records_at_every_depth() {
    let document = "<Document SchemaVersion='4' FileVersion='1'><Properties Count='2'><Property name='Damaged' type='App::PropertyInteger' group='Custom' doc='retained declaration'><Integer value='7'/></Property><Property name='Peer' type='App::PropertyString'><String value='independent'/></Property></Properties><Objects Count='0'/><ObjectData Count='0'/></Document>";
    let damaged = document.replacen("value='7'", "value='x'", 1);
    let previous = archive(document);
    let current = archive(&damaged);
    for depth in [
        DecodeDepth::Full,
        DecodeDepth::Geometry,
        DecodeDepth::Container,
    ] {
        let options = DecodeOptions {
            depth,
            ..DecodeOptions::default()
        };
        let before = crate::FcstdCodec
            .decode(&mut Cursor::new(&previous), &options)
            .unwrap();
        let after = crate::FcstdCodec
            .decode(&mut Cursor::new(&current), &options)
            .unwrap();
        let failures = after
            .source_fidelity()
            .parts()
            .values()
            .filter(|part| part.state() == &PartState::GivenUp {})
            .collect::<Vec<_>>();
        let [failure] = failures.as_slice() else {
            panic!("one real damaged leaf: {failures:?}")
        };
        assert_eq!(failure.level().as_str(), "freecad.property_value");
        assert_eq!(
            failure.offset(),
            u64::try_from(damaged.find("<Integer").unwrap()).unwrap()
        );
        assert_eq!(
            failure.byte_len(),
            u64::try_from("<Integer value='x'/>".len()).unwrap()
        );
        let properties = after
            .ir()
            .native
            .namespace("fcstd")
            .unwrap()
            .arena_as::<crate::native::PropertyRecord>("properties")
            .unwrap();
        let retained = properties
            .iter()
            .find(|property| property.name == "Damaged")
            .unwrap();
        assert!(retained.body.is_none());
        assert_eq!(retained.dynamic.as_ref().unwrap().group, "Custom");
        assert_eq!(
            retained.dynamic.as_ref().unwrap().documentation.as_deref(),
            Some("retained declaration")
        );
        assert_eq!(
            properties
                .iter()
                .find(|property| property.name == "Peer")
                .unwrap()
                .values()[0]
                .attributes["value"],
            "independent"
        );
        crate::test_support::framing::assert_xml_parts_agree(&current, &after);
        let arena = DecodeArena::new();
        let (ctx, root) =
            DecodeContext::from_root_bytes(&current, &arena, &DecodePolicy::service()).unwrap();
        let root = ctx.name_stream(root, "file").unwrap();
        let source = cadmpeg_container::ArchiveSnapshot::new(&ctx, root).unwrap();
        let member = source.open(&ctx, "Document.xml").unwrap();
        let member_extent = ctx.capture_part_extent(member).unwrap();
        let allowances = Allowances::given_up(
            DecodeSnapshot::new(before.ir(), before.source_fidelity()),
            DecodeSnapshot::new(after.ir(), after.source_fidelity()),
            after.report(),
            "file",
            |stream, range| {
                let bytes = if stream == "file" {
                    root.window()
                } else if stream == member_extent.stream() {
                    member.window()
                } else {
                    return None;
                };
                bytes.get(usize::try_from(range.start).ok()?..usize::try_from(range.end).ok()?)
            },
        )
        .unwrap();
        assert_decodes_agree(
            &serde_json::to_value(before.ir()).unwrap(),
            &serde_json::to_value(after.ir()).unwrap(),
            &allowances,
            Mode::Same,
        );
    }
}
