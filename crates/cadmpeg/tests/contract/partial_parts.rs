// SPDX-License-Identifier: Apache-2.0
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, View};
use cadmpeg_test_support::contract::zip::frame_stored_members;
use cadmpeg_test_support::framing::zip::{frame, rebuild, RecoveryReason, ZipScan};
use cadmpeg_test_support::framing::{FrameParent, FramedPart, PartTable};
use cadmpeg_test_support::zip_fixture::{archive, RawEntry};

const STEP: &[u8] = b"ISO-10303-21;HEADER;ENDSEC;DATA;#1=CARTESIAN_POINT('one',(1.,2.,3.));#2=CARTESIAN_POINT('two',(4.,5.,6.));ENDSEC;END-ISO-10303-21;";
const XML: &[u8] =
    b"<Document><Objects><Object name='One'/><Object name='Two'/></Objects></Document>";
const GUI: &[u8] =
    b"<Document><ViewProviderData><ViewProvider name='One'/></ViewProviderData></Document>";

fn source(bytes: &[u8], run: impl FnOnce(&DecodeContext<'_>, View<'_>)) {
    let arena = DecodeArena::new();
    let (ctx, root) = DecodeContext::from_root_bytes(bytes, &arena, &DecodePolicy::service())
        .expect("actual ZIP source");
    run(&ctx, root);
}

fn parts<'a>(frames: &'a [FramedPart<'_>], level: &str) -> Vec<&'a [u8]> {
    frames
        .iter()
        .filter(|part| part.level().as_str() == level)
        .map(|part| part.view().window())
        .collect()
}

#[test]
fn step_missing_zip_footer_preserves_actual_root_entities_and_exact_directory_stop() {
    let bytes = archive(&[RawEntry::stored("ISO-10303.p21", STEP)]);
    // docs/layouts/zip.toml: local_header has a 30-byte fixed prefix.
    let central = 30 + "ISO-10303.p21".len() + STEP.len();
    for cut in [central, bytes.len() - 1] {
        source(&bytes[..cut], |ctx, root| {
            let mut frames = Vec::new();
            let reached = frame_stored_members(ctx, root, &mut frames)
                .expect("source-bounded STEP ZIP framing");
            let ZipScan::Local { entries, stop } = reached.scan() else {
                panic!("the actual end record is missing");
            };
            assert_eq!(entries.len(), 1);
            assert_eq!(entries[0].name().window(), b"ISO-10303.p21");
            assert_eq!(entries[0].payload().window(), STEP);
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
            assert_eq!(
                parts(&frames, "step.simple_entity_instance"),
                [
                    b"#1=CARTESIAN_POINT('one',(1.,2.,3.));".as_slice(),
                    b"#2=CARTESIAN_POINT('two',(4.,5.,6.));",
                ]
            );
            let wrapper = frames
                .iter()
                .find(|part| part.level().as_str() == "step.zip_root_member")
                .expect("real member wrapper");
            assert_eq!(
                wrapper.parent(),
                FrameParent::Part(entries[0].frame_index())
            );
            assert_eq!(wrapper.view().window(), STEP);
            assert!(reached.xml().is_empty());
            assert!(!frames.iter().any(|part| matches!(
                part.level().as_str(),
                "zip.central_directory" | "zip.central_header" | "zip.end_record"
            )));
            assert!(frame(ctx, root, &mut Vec::new()).is_err());
            assert!(rebuild(ctx, root, &[]).is_err());
            PartTable::load()
                .expect("Appendix C")
                .validate(ctx, &frames)
                .expect("genuine source containment");
            assert!(ctx
                .take_record_captures()
                .expect("no semantic-read attribution")
                .is_empty());
        });
    }
}

#[test]
fn step_cut_later_zip_member_retains_prior_root_without_promoting_partial_payload() {
    let bytes = archive(&[
        RawEntry::stored("ISO-10303.p21", STEP),
        RawEntry::stored("Document.xml", XML),
    ]);
    let header = 30 + "ISO-10303.p21".len() + STEP.len();
    let payload = header + 30 + "Document.xml".len();
    for (cut, start, reason) in [
        (header + 12, header, RecoveryReason::HeaderIncomplete),
        (payload + 7, payload, RecoveryReason::PayloadIncomplete),
    ] {
        source(&bytes[..cut], |ctx, root| {
            let mut frames = Vec::new();
            let reached = frame_stored_members(ctx, root, &mut frames).expect("bounded source cut");
            let ZipScan::Local { entries, stop } = reached.scan() else {
                panic!("no footer was supplied");
            };
            assert_eq!(entries.len(), 1);
            assert_eq!(entries[0].payload().window(), STEP);
            assert_eq!(stop.reason(), reason);
            assert_eq!(stop.available().window(), &bytes[start..cut]);
            assert_eq!(
                parts(&frames, "step.simple_entity_instance"),
                [
                    b"#1=CARTESIAN_POINT('one',(1.,2.,3.));".as_slice(),
                    b"#2=CARTESIAN_POINT('two',(4.,5.,6.));",
                ]
            );
            assert!(parts(&frames, "freecad.document_xml").is_empty());
            assert!(reached.xml().is_empty());
            let incomplete = &frames[stop.frame_index().expect("real incomplete source part")];
            assert_eq!(incomplete.view().window(), &bytes[start..cut]);
            assert_eq!(
                incomplete.level().as_str(),
                if reason == RecoveryReason::HeaderIncomplete {
                    "zip.local_header"
                } else {
                    "zip.entry"
                }
            );
            assert!(rebuild(ctx, root, &[]).is_err());
        });
    }
}

#[test]
fn freecad_damaged_xml_part_in_recovered_zip_keeps_later_property_and_member() {
    let xml = b"<Document><Properties><Property name='Bad' type='App::PropertyString'><String></Wrong></Property><Property name='Good' type='App::PropertyString'><String value='kept'/></Property></Properties></Document>";
    let bytes = archive(&[
        RawEntry::stored("Document.xml", xml),
        RawEntry::stored("GuiDocument.xml", GUI),
    ]);
    let central = 30 + "Document.xml".len() + xml.len() + 30 + "GuiDocument.xml".len() + GUI.len();
    source(&bytes[..central], |ctx, root| {
        let mut frames = Vec::new();
        let reached =
            frame_stored_members(ctx, root, &mut frames).expect("actual XML and ZIP recovery");
        let ZipScan::Local { entries, stop } = reached.scan() else {
            panic!("missing source footer");
        };
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].payload().window(), xml);
        assert_eq!(entries[1].payload().window(), GUI);
        assert_eq!(stop.reason(), RecoveryReason::SourceEnd);
        assert_eq!(stop.available().start(), central);
        assert!(stop.available().window().is_empty());
        assert_eq!(reached.xml().len(), 2);
        assert!(!reached.xml()[0].scan().faults().is_empty());
        assert!(reached.xml()[1].scan().faults().is_empty());
        let properties = parts(&frames, "freecad.property");
        assert_eq!(properties, [
            b"<Property name='Bad' type='App::PropertyString'><String></Wrong></Property>".as_slice(),
            b"<Property name='Good' type='App::PropertyString'><String value='kept'/></Property>",
        ]);
        assert!(ctx
            .parse_xml(
                std::str::from_utf8(properties[0]).expect("literal XML"),
                "damaged property"
            )
            .is_err());
        assert!(ctx
            .parse_xml(
                std::str::from_utf8(properties[1]).expect("literal XML"),
                "later property"
            )
            .is_ok());
        assert_eq!(
            parts(&frames, "freecad.view_provider"),
            [b"<ViewProvider name='One'/>".as_slice()]
        );
        assert!(frame(ctx, root, &mut Vec::new()).is_err());
        assert!(rebuild(ctx, root, &[]).is_err());
        PartTable::load()
            .expect("Appendix C")
            .validate(ctx, &frames)
            .expect("retained sibling source boundaries");
    });
}

#[test]
fn freecad_complete_archive_contract_preserves_strict_frame_and_rebuild_authority() {
    let bytes = archive(&[
        RawEntry::stored("Document.xml", XML),
        RawEntry::stored("GuiDocument.xml", GUI),
    ]);
    source(&bytes, |ctx, root| {
        let mut frames = Vec::new();
        let reached =
            frame_stored_members(ctx, root, &mut frames).expect("complete original archive");
        let ZipScan::Complete { entries } = reached.scan() else {
            panic!("complete directory must remain authoritative");
        };
        assert_eq!(entries.len(), 2);
        assert_eq!(
            parts(&frames, "freecad.object_declaration"),
            [b"<Object name='One'/>".as_slice(), b"<Object name='Two'/>"]
        );
        assert_eq!(
            parts(&frames, "freecad.view_provider"),
            [b"<ViewProvider name='One'/>".as_slice()]
        );
        assert_eq!(reached.xml().len(), 2);
        assert!(reached
            .xml()
            .iter()
            .all(|member| member.scan().faults().is_empty()));
        let strict = frame(ctx, root, &mut Vec::new()).expect("strict directory framing");
        assert_eq!(strict.len(), entries.len());
        for (strict, actual) in strict.iter().zip(entries) {
            assert_eq!(strict.name().window(), actual.name().window());
            assert_eq!(strict.payload().window(), actual.payload().window());
        }
        assert_eq!(
            rebuild(ctx, root, &[]).expect("original metadata and CRC preservation"),
            bytes
        );
        assert!(ctx
            .take_record_captures()
            .expect("no fake semantic reads")
            .is_empty());
    });
}
