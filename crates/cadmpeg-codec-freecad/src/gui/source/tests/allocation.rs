// SPDX-License-Identifier: Apache-2.0
//! Local slot admission preserves actual headers, small cut arities and peers.
use crate::native::{entry_contents::EntryContents, EntryRecord};
use crate::test_support::test_archive::archive_entries;
use cadmpeg_ir::source_fidelity::parts::PartState;
use cadmpeg_ir::{Codec, DecodeOptions};
use std::io::Cursor;

#[test]
fn huge_list_count_gives_up_locally_while_a_small_cut_keeps_original_arity() {
    let document=b"<Document SchemaVersion='4' FileVersion='1'><Objects Count='1'><Object type='App::Feature' name='A'/></Objects><ObjectData Count='1'><Object name='A'><Properties Count='0'/></Object></ObjectData></Document>";
    let gui=b"<Document SchemaVersion='1'><ViewProviderData Count='1'><ViewProvider name='A'><Properties Count='2'><Property name='Bad' type='App::PropertyFloatList'><FloatList file='bad.bin'/></Property><Property name='Good' type='App::PropertyFloatList'><FloatList file='good.bin'/></Property></Properties></ViewProvider></ViewProviderData></Document>";
    let mut good = 1_u32.to_le_bytes().to_vec();
    good.extend_from_slice(&7_f64.to_le_bytes());
    let huge = u32::MAX.to_le_bytes().to_vec();
    let mut cut = 3_u32.to_le_bytes().to_vec();
    cut.extend_from_slice(&1_f64.to_le_bytes());
    cut.extend_from_slice(&2_f64.to_le_bytes()[..4]);
    for (payload, level) in [
        (&huge, "freecad.float_list_entry"),
        (&cut, "freecad.float_list_value"),
    ] {
        let bytes = archive_entries(&[
            ("Document.xml", document),
            ("GuiDocument.xml", gui),
            ("bad.bin", payload),
            ("good.bin", &good),
        ]);
        let decoded = crate::FcstdCodec
            .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
            .unwrap();
        assert!(crate::test_support::validate_native(decoded.ir()).is_empty());
        let entries = decoded
            .ir()
            .native
            .namespace("fcstd")
            .unwrap()
            .arena_as::<EntryRecord>("entries")
            .unwrap();
        let healthy = entries
            .iter()
            .find(|entry| entry.name() == "good.bin")
            .unwrap();
        assert!(
            matches!(&healthy.contents()[0],EntryContents::Float{values} if values.len()==1 && values[0].is_some_and(|value|value.get()==7.0))
        );
        let bad = entries
            .iter()
            .find(|entry| entry.name() == "bad.bin")
            .unwrap();
        if payload.len() == 4 {
            assert!(bad.contents().is_empty());
        } else {
            let EntryContents::Float { values } = &bad.contents()[0] else {
                panic!("known float list");
            };
            assert_eq!(values.len(), 3);
            assert_eq!(values[0].unwrap().get(), 1.0);
            assert!(values[1].is_none() && values[2].is_none());
        }
        let failed = decoded
            .source_fidelity()
            .parts()
            .values()
            .filter(|part| part.state() == &PartState::GivenUp {})
            .collect::<Vec<_>>();
        assert_eq!(failed.len(), 1);
        assert_eq!(failed[0].level().as_str(), level);
        assert_eq!(
            decoded
                .report()
                .losses
                .iter()
                .filter(|loss| loss.code.local_code() == "record.unreadable")
                .count(),
            1
        );
    }
}
