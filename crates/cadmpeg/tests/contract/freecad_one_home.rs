// SPDX-License-Identifier: Apache-2.0
use std::io::Cursor;

use cadmpeg_container::ArchiveSnapshot;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_ir::attributes::{AttributeTarget, AttributeValue};
use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::report::Severity;
use cadmpeg_ir::ArenaClass;
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
    let unreadable = decoded
        .report()
        .losses
        .iter()
        .filter(|loss| loss.code.to_string() == "decode/record.unreadable")
        .collect::<Vec<_>>();
    assert_eq!(unreadable.len(), 1, "{:?}", decoded.report().losses);
    assert_eq!(unreadable[0].severity, Severity::Error);
    assert!(unreadable[0]
        .message
        .starts_with("part freecad.property_value "));
    let retained = decoded
        .source_fidelity()
        .retained_records()
        .values()
        .filter(|record| record.data() == Some(raw_value.as_bytes()))
        .collect::<Vec<_>>();
    assert_eq!(retained.len(), 1);
    assert_eq!(
        retained[0].offset(),
        u64::try_from(member.start() + document.find(bad).unwrap() + value_start).unwrap()
    );
    assert!(retained[0].untyped().contains(&ArenaClass::Model));
    assert!(retained[0].untyped().contains(&ArenaClass::Design));
    let property = decoded.ir().native.namespace("fcstd").unwrap().arenas()["properties"]
        .iter()
        .find(|record| {
            record
                .fields()
                .is_ok_and(|fields| fields.get("name").is_some_and(|name| name == "Bad"))
        })
        .unwrap();
    let fields = property.fields().unwrap();
    assert_eq!(fields["type_name"], "App::PropertyInteger");
    for key in ["values", "links", "side_entries"] {
        assert!(fields[key].is_null());
    }
}
