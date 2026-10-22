// SPDX-License-Identifier: Apache-2.0
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, View};

use crate::framing::zip::{frame, rebuild, RecoveryReason, ZipScan};
use crate::framing::{FrameParent, FramedPart};
use crate::zip_fixture::{archive, RawEntry};

use super::frame_stored_members;

const STEP: &[u8] =
    b"ISO-10303-21;HEADER;ENDSEC;DATA;#1=ITEM(1);#2=ITEM(2);ENDSEC;END-ISO-10303-21;";
const XML: &[u8] = b"<Document><Objects><Object name='A'/><Object name='B'/></Objects></Document>";
const GUI: &[u8] =
    b"<Document><ViewProviderData><ViewProvider name='A'/></ViewProviderData></Document>";

fn source(bytes: &[u8], run: impl FnOnce(&DecodeContext<'_>, View<'_>)) {
    let arena = DecodeArena::new();
    let (ctx, root) = DecodeContext::from_root_bytes(bytes, &arena, &DecodePolicy::service())
        .expect("literal archive source");
    run(&ctx, root);
}

fn nested<'a>(frames: &'a [FramedPart<'_>], level: &str) -> Vec<&'a [u8]> {
    frames
        .iter()
        .filter(|part| part.level().as_str() == level)
        .map(|part| part.view().window())
        .collect()
}

#[test]
fn step_complete_and_cut_footer_contracts_consume_real_member_views_and_stop() {
    let bytes = archive(&[
        RawEntry::stored("ISO-10303.p21", STEP),
        RawEntry::stored("Document.xml", XML),
        RawEntry::stored("GuiDocument.xml", GUI),
    ]);
    // docs/layouts/zip.toml: each local fixed header is 30 bytes, followed by name and payload.
    let central = 30
        + "ISO-10303.p21".len()
        + STEP.len()
        + 30
        + "Document.xml".len()
        + XML.len()
        + 30
        + "GuiDocument.xml".len()
        + GUI.len();
    for cut in [bytes.len(), bytes.len() - 3, central] {
        source(&bytes[..cut], |ctx, root| {
            let mut frames = Vec::new();
            let observed = frame_stored_members(ctx, root, &mut frames)
                .expect("actual source framing and embedded traversal");
            assert_eq!(
                nested(&frames, "step.simple_entity_instance"),
                [b"#1=ITEM(1);".as_slice(), b"#2=ITEM(2);"]
            );
            assert_eq!(
                nested(&frames, "freecad.object_declaration"),
                [b"<Object name='A'/>".as_slice(), b"<Object name='B'/>"]
            );
            assert_eq!(
                nested(&frames, "freecad.view_provider"),
                [b"<ViewProvider name='A'/>".as_slice()]
            );
            assert_eq!(observed.xml().len(), 2);
            assert!(observed
                .xml()
                .iter()
                .all(|member| member.scan().faults().is_empty()));
            match observed.scan() {
                ZipScan::Complete { entries } => {
                    assert_eq!(cut, bytes.len());
                    assert_eq!(entries.len(), 3);
                    assert_eq!(
                        frame(ctx, root, &mut Vec::new())
                            .expect("strict original archive")
                            .len(),
                        3
                    );
                    assert_eq!(
                        rebuild(ctx, root, &[]).expect("strict no-op rebuild"),
                        bytes
                    );
                }
                ZipScan::Local { entries, stop } => {
                    assert_ne!(cut, bytes.len());
                    assert_eq!(entries.len(), 3);
                    assert_eq!(stop.available().start(), central);
                    assert_eq!(stop.available().window(), &bytes[central..cut]);
                    assert_eq!(
                        stop.reason(),
                        if cut == central {
                            RecoveryReason::SourceEnd
                        } else {
                            RecoveryReason::FollowingStructure
                        }
                    );
                    assert!(frame(ctx, root, &mut Vec::new()).is_err());
                    assert!(rebuild(ctx, root, &[]).is_err());
                    assert!(!frames.iter().any(|part| matches!(
                        part.level().as_str(),
                        "zip.end_record" | "zip.central_directory" | "zip.central_header"
                    )));
                }
            }
            for part in frames.iter().filter(|part| {
                matches!(
                    part.level().as_str(),
                    "step.zip_root_member" | "freecad.document_xml" | "freecad.gui_document_xml"
                )
            }) {
                let FrameParent::Part(parent) = part.parent() else {
                    panic!("actual ZIP entry parent");
                };
                assert_eq!(frames[parent].level().as_str(), "zip.entry");
                assert_eq!(frames[parent].view().window(), part.view().window());
                assert_eq!(
                    ctx.capture_part_extent(part.view())
                        .expect("member provenance")
                        .stream(),
                    ctx.capture_part_extent(root)
                        .expect("source provenance")
                        .stream()
                );
            }
            assert!(ctx
                .take_record_captures()
                .expect("no semantic reads")
                .is_empty());
        });
    }
}

#[test]
fn freecad_cut_member_never_becomes_a_document_but_prior_step_sibling_is_consumed() {
    let bytes = archive(&[
        RawEntry::stored("ISO-10303.p21", STEP),
        RawEntry::stored("Document.xml", XML),
    ]);
    let header = 30 + "ISO-10303.p21".len() + STEP.len();
    let payload = header + 30 + "Document.xml".len();
    for (cut, start, reason) in [
        (header + 12, header, RecoveryReason::HeaderIncomplete),
        (payload + 10, payload, RecoveryReason::PayloadIncomplete),
    ] {
        source(&bytes[..cut], |ctx, root| {
            let mut frames = Vec::new();
            let observed = frame_stored_members(ctx, root, &mut frames).expect("bounded recovery");
            let ZipScan::Local { entries, stop } = observed.scan() else {
                panic!("missing footer");
            };
            assert_eq!(entries.len(), 1);
            assert_eq!(entries[0].name().window(), b"ISO-10303.p21");
            assert_eq!(entries[0].payload().window(), STEP);
            assert_eq!(stop.reason(), reason);
            assert_eq!(stop.available().window(), &bytes[start..cut]);
            assert_eq!(
                nested(&frames, "step.simple_entity_instance"),
                [b"#1=ITEM(1);".as_slice(), b"#2=ITEM(2);"]
            );
            assert!(nested(&frames, "freecad.document_xml").is_empty());
            assert!(observed.xml().is_empty());
            assert!(frame(ctx, root, &mut Vec::new()).is_err());
            assert!(rebuild(ctx, root, &[]).is_err());
        });
    }
}

#[test]
fn freecad_recovered_complete_member_keeps_xml_damage_and_later_source_sibling() {
    let xml = b"<Document><Objects><Object name='Bad'></Wrong></Object><Object name='Later'/></Objects></Document>";
    let bytes = archive(&[
        RawEntry::stored("Document.xml", xml),
        RawEntry::stored("ISO-10303.p21", STEP),
    ]);
    let central = 30 + "Document.xml".len() + xml.len() + 30 + "ISO-10303.p21".len() + STEP.len();
    source(&bytes[..central], |ctx, root| {
        let mut frames = Vec::new();
        let observed = frame_stored_members(ctx, root, &mut frames).expect("physical XML recovery");
        let ZipScan::Local { entries, stop } = observed.scan() else {
            panic!("no source footer");
        };
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].payload().window(), xml);
        assert_eq!(stop.reason(), RecoveryReason::SourceEnd);
        assert_eq!(observed.xml().len(), 1);
        assert!(!observed.xml()[0].scan().faults().is_empty());
        assert!(nested(&frames, "freecad.object_declaration")
            .contains(&b"<Object name='Later'/>".as_slice()));
        assert_eq!(
            nested(&frames, "step.simple_entity_instance"),
            [b"#1=ITEM(1);".as_slice(), b"#2=ITEM(2);"]
        );
        assert_eq!(entries[0].payload().window(), xml);
        assert!(ctx
            .take_record_captures()
            .expect("no semantic reads")
            .is_empty());
    });
}

#[test]
fn step_unopened_compressed_or_encrypted_member_is_an_error_in_complete_and_local_contracts() {
    for entry in [
        RawEntry::compressed("ISO-10303.p21", 8, b"not opened", STEP),
        RawEntry::stored("ISO-10303.p21", STEP).with_flags(1),
    ] {
        let central = 30 + entry.name.len() + entry.payload.len();
        let bytes = archive(&[entry]);
        for cut in [bytes.len(), central] {
            source(&bytes[..cut], |ctx, root| {
                let error = frame_stored_members(ctx, root, &mut Vec::new())
                    .expect_err("no fake expanded member or skipped contract");
                assert!(
                    error.contains("opened unencrypted stored member"),
                    "{error}"
                );
            });
        }
    }
}
