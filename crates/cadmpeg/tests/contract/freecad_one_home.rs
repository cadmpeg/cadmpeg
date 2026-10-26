// SPDX-License-Identifier: Apache-2.0
use std::io::Cursor;

use cadmpeg_container::ArchiveSnapshot;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_ir::attributes::{AttributeTarget, AttributeValue};
use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::source_fidelity::parts::PartState;
use cadmpeg_ir::CodecFormat;
use cadmpeg_test_support::contract::parts::check_one_home;
use cadmpeg_test_support::zip_fixture::{archive, RawEntry};

#[test]
fn freecad_bad_direct_property_has_one_home_and_keeps_the_real_neutral_peer() {
    let bad = "<Property name='Bad' type='App::PropertyInteger'><Integer value='9223372036854775808'/></Property>";
    let raw_value = "<Integer value='9223372036854775808'/>";
    let value_start = bad.find(raw_value).unwrap();
    let good =
        "<Property name='Good' type='App::PropertyString'><String value='A&amp;B'/></Property>";
    let document = format!("<Document SchemaVersion='4' FileVersion='1'><Properties Count='2'>{bad}{good}</Properties><Objects Count='0'/><ObjectData Count='0'/></Document>");
    let bytes = archive(&[RawEntry::stored("Document.xml", document.as_bytes())]);
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service()).unwrap();
    let source = ArchiveSnapshot::new(&ctx, root).expect("independent source ZIP index");
    let member = source
        .open(&ctx, "Document.xml")
        .expect("original Document.xml member");
    assert_eq!(member.window(), document.as_bytes());
    let member_extent = ctx.capture_part_extent(member).unwrap();
    let decoded = cadmpeg_codec_freecad::FcstdCodec
        .decode(&mut Cursor::new(&bytes), &DecodeOptions::default())
        .expect("bounded bad property preserves its readable peer");
    let peers = decoded
        .ir()
        .model
        .attributes
        .iter()
        .filter(|attribute| attribute.name == "Good")
        .collect::<Vec<_>>();
    assert_eq!(peers.len(), 1);
    assert_eq!(peers[0].target, AttributeTarget::Document);
    assert_eq!(
        peers[0].values,
        [Some(AttributeValue::String("A&B".into()))]
    );
    let native_id = peers[0].native_ref.as_deref().expect("actual native peer");
    assert!(
        decoded.ir().native.namespace("fcstd").unwrap().arenas()["properties"]
            .iter()
            .any(|record| record.id() == native_id)
    );
    assert!(!decoded
        .ir()
        .model
        .attributes
        .iter()
        .any(|attribute| attribute.name == "Bad"));
    let given_up = decoded
        .source_fidelity()
        .parts()
        .iter()
        .filter(|(_, part)| matches!(part.state(), PartState::GivenUp {}))
        .collect::<Vec<_>>();
    assert_eq!(given_up.len(), 1);
    let (id, part) = given_up[0];
    assert_eq!(part.level().as_str(), "freecad.property_value");
    assert_eq!(part.stream(), member_extent.stream());
    assert_eq!(
        part.offset(),
        member_extent.offset() + u64::try_from(document.find(bad).unwrap() + value_start).unwrap()
    );
    assert_eq!(part.byte_len(), u64::try_from(raw_value.len()).unwrap());
    assert_eq!(
        decoded
            .source_fidelity()
            .retained_record(id.as_str())
            .unwrap()
            .data(),
        Some(raw_value.as_bytes())
    );
    let unknowns = decoded.ir().native_unknowns("fcstd").unwrap();
    let [unknown] = unknowns.as_slice() else {
        panic!("one linked value failure")
    };
    assert_eq!(&unknown.id, id);
    assert_eq!(unknown.links.len(), 1);
    let property = decoded.ir().native.namespace("fcstd").unwrap().arenas()["properties"]
        .iter()
        .find(|record| record.id() == unknown.links[0].as_str())
        .unwrap();
    let fields = property.fields().unwrap();
    assert_eq!(fields["name"], "Bad");
    assert_eq!(fields["type_name"], "App::PropertyInteger");
    for key in ["values", "links", "side_entries"] {
        assert!(fields[key].is_null());
    }
    assert!(decoded.source_fidelity().record_layout()[property.id()]
        .payloads()
        .is_empty());
    let parent = &decoded.source_fidelity().parts()[part.parent().unwrap()];
    assert_eq!(parent.level().as_str(), "freecad.property");
    assert_eq!(parent.state(), &PartState::Decoded {});
    check_one_home(
        CodecFormat::Fcstd,
        &decoded.ir().native,
        decoded.source_fidelity(),
        &decoded.report().losses,
        "file",
        |stream, range| {
            let start = usize::try_from(range.start).ok()?;
            let end = usize::try_from(range.end).ok()?;
            if stream == "file" {
                return bytes.get(start..end);
            }
            if stream == member_extent.stream() {
                return member.child(start, end).map(|view| view.window());
            }
            None
        },
    )
    .expect("unique helper stub, exact original member bytes and sole unreadable loss");
    assert!(ctx.take_record_captures().unwrap().is_empty());
}
