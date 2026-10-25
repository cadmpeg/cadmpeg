// SPDX-License-Identifier: Apache-2.0
//! Actual shape-list references and public inspection share the BRep schedule.
use super::*;
use cadmpeg_core::decode::{DecodeDepth, InspectOptions};
use cadmpeg_ir::source_fidelity::parts::PartState;

fn container(bytes: &[u8]) -> DecodeResult {
    crate::FcstdCodec
        .decode(
            &mut Cursor::new(bytes),
            &DecodeOptions {
                depth: DecodeDepth::Container,
                ..DecodeOptions::default()
            },
        )
        .unwrap()
}

#[test]
fn shape_lists_open_both_text_and_binary_members_under_the_actual_app_or_gui_property() {
    let property = "<Property name='Shapes' type='Part::PropertyTopoShapeList'><ShapeList count='2'><TopoShape file='Shapes.0.brp'/><TopoShape file='Shapes.1.BIN'/></ShapeList></Property>";
    for gui in [false, true] {
        let app = if gui { "" } else { property };
        let provider = if gui { property } else { "" };
        let document = format!("<Document SchemaVersion='4' FileVersion='1'><Objects Count='1'><Object type='Part::Feature' name='Owner'/></Objects><ObjectData Count='1'><Object name='Owner'><Properties Count='{}'>{app}</Properties></Object></ObjectData></Document>", usize::from(!gui));
        let gui_document = format!("<Document SchemaVersion='1'><ViewProviderData Count='1'><ViewProvider name='Owner'><Properties Count='{}'>{provider}</Properties></ViewProvider></ViewProviderData></Document>", usize::from(gui));
        let text = fixtures::member(false);
        let binary = fixtures::member(true);
        let bytes = crate::test_support::test_archive::archive_entries(&[
            ("Document.xml", document.as_bytes()),
            ("GuiDocument.xml", gui_document.as_bytes()),
            ("Shapes.0.brp", &text),
            ("Shapes.1.BIN", &binary),
        ]);
        let decoded = container(&bytes);
        let boundaries = assert_parts_agree(&bytes, &decoded);
        assert_eq!(
            boundaries
                .iter()
                .map(|part| part.level.as_str())
                .collect::<BTreeSet<_>>()
                .len(),
            16
        );
        let native = decoded.ir().native.namespace("fcstd").unwrap();
        let owner = if gui {
            native
                .arena_as::<crate::native::GuiPropertyRecord>("gui_properties")
                .unwrap()
                .into_iter()
                .find(|property| property.type_name == "Part::PropertyTopoShapeList")
                .unwrap()
                .id
        } else {
            native
                .arena_as::<crate::native::PropertyRecord>("properties")
                .unwrap()
                .into_iter()
                .find(|property| property.type_name == "Part::PropertyTopoShapeList")
                .unwrap()
                .id
        };
        let payloads = crate::brep::records::payloads_untracked(native).unwrap();
        assert_eq!(payloads.len(), 2);
        let entries = native
            .arena_as::<crate::native::EntryRecord>("entries")
            .unwrap();
        let names = payloads
            .iter()
            .map(|payload| {
                assert_eq!(payload.property, owner);
                let entry = entries
                    .iter()
                    .find(|entry| entry.id() == payload.entry)
                    .unwrap();
                assert_eq!(
                    serde_json::to_value(payload).unwrap()["form"],
                    if entry.name().ends_with(".BIN") {
                        "binary"
                    } else {
                        "text"
                    }
                );
                entry.name().to_owned()
            })
            .collect::<BTreeSet<_>>();
        assert_eq!(
            names,
            BTreeSet::from(["Shapes.0.brp".to_owned(), "Shapes.1.BIN".to_owned()])
        );
        assert!(crate::test_support::validate_native(decoded.ir()).is_empty());
        crate::fidelity::b1::assert_classified_spans(&decoded);
    }
}

#[test]
fn brep_inspection_matches_container_decode_for_valid_and_failed_member_records() {
    for binary in [false, true] {
        let original = fixtures::member(binary);
        let bytes = fixtures::archive(binary, &original);
        let healthy = container(&bytes);
        let boundaries = assert_parts_agree(&bytes, &healthy);
        let (start, end, level) = if binary {
            let start = original
                .windows(b"Curve2ds ".len())
                .position(|bytes| bytes == b"Curve2ds ")
                .unwrap();
            (
                u64::try_from(start).unwrap(),
                u64::try_from(original.len()).unwrap(),
                "freecad.brep_binary_record",
            )
        } else {
            let row = boundaries
                .iter()
                .find(|part| part.level == "freecad.brep_text_curve")
                .unwrap();
            (row.offset, row.parent_end, "freecad.brep_text_curve")
        };
        for damaged in [false, true] {
            let mut member = original.clone();
            if damaged {
                member[usize::try_from(start).unwrap()] = b'!';
            }
            let bytes = fixtures::archive(binary, &member);
            let decoded = container(&bytes);
            let inspected = crate::FcstdCodec
                .inspect(&mut Cursor::new(&bytes), &InspectOptions::default())
                .unwrap();
            assert_eq!(inspected.losses, decoded.report().losses);
            assert_eq!(inspected.notes, decoded.report().notes);
            crate::fidelity::b1::assert_classified_spans(&decoded);
            let failures = decoded
                .source_fidelity()
                .parts()
                .values()
                .filter(|part| part.state() == &PartState::GivenUp {})
                .collect::<Vec<_>>();
            if damaged {
                let [failure] = failures.as_slice() else {
                    panic!("one bounded BRep refusal: {failures:?}");
                };
                assert_eq!(failure.level().as_str(), level);
                assert_eq!((failure.offset(), failure.byte_len()), (start, end - start));
                let error = cadmpeg_ir::codec::DecodeLossCode::RecordUnreadable.note(String::new());
                let errors = decoded
                    .report()
                    .losses
                    .iter()
                    .filter(|loss| loss.code == error.code)
                    .collect::<Vec<_>>();
                let [error] = errors.as_slice() else {
                    panic!("one reported helper refusal");
                };
                assert_eq!(error.provenance.as_ref().unwrap().offset, start);
                assert_eq!(
                    error.provenance.as_ref().unwrap().byte_len(),
                    Some(end - start)
                );
            } else {
                assert!(failures.is_empty());
                assert_parts_agree(&bytes, &decoded);
            }
            assert!(crate::test_support::validate_native(decoded.ir()).is_empty());
        }
    }
}
