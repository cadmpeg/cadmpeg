// SPDX-License-Identifier: Apache-2.0
use super::{frame, frame_zip_root, rebuild, Edit};
use crate::framing::{FrameParent, FramedPart, PartTable};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, View};
use cadmpeg_ir::source_fidelity::parts::PartLevel;

const SOURCE: &[u8] = b"ISO-10303-21;HEADER;FILE_SCHEMA(('A;B'));ENDSEC;DATA;#1=F('x;''y',\"0;\",<a;b>);#2=(A(1) B(2));ENDSEC;END-ISO-10303-21; /*tail*/";

fn source(bytes: &[u8], run: impl FnOnce(&DecodeContext<'_>, View<'_>)) {
    let arena = DecodeArena::new();
    let policy = DecodePolicy::service();
    let (ctx, view) = DecodeContext::from_root_bytes(bytes, &arena, &policy).unwrap();
    run(&ctx, view);
}

#[test]
fn literal_record_ranges_ignore_semicolons_in_all_quoted_forms() {
    source(SOURCE, |ctx, view| {
        let mut frames = Vec::new();
        frame(ctx, view, FrameParent::File, &mut frames).unwrap();
        let actual = frames
            .iter()
            .map(|part| {
                (
                    part.level().as_str(),
                    part.view().start(),
                    part.view().end(),
                    part.parent(),
                )
            })
            .collect::<Vec<_>>();
        assert_eq!(
            actual,
            vec![
                ("step.exchange", 0, 126, FrameParent::File),
                ("step.header_section", 13, 48, FrameParent::Part(0)),
                ("step.header_record", 20, 41, FrameParent::Part(1)),
                ("step.data_section", 48, 100, FrameParent::Part(0)),
                ("step.simple_entity_instance", 53, 78, FrameParent::Part(3)),
                ("step.complex_entity_instance", 78, 93, FrameParent::Part(3)),
            ]
        );
        assert_eq!(frames[0].framing(), &[0..13, 100..117]);
        assert_eq!(frames[4].framing(), &[53..56, 77..78]);
        assert_eq!(frames[5].framing(), &[78..81, 81..82, 91..92, 92..93]);
        let names = frames[5]
            .dispatch()
            .iter()
            .map(|range| {
                &SOURCE[usize::try_from(range.start).unwrap()..usize::try_from(range.end).unwrap()]
            })
            .collect::<Vec<_>>();
        assert_eq!(names, [b"A".as_slice(), b"B".as_slice()]);
        PartTable::load().unwrap().validate(ctx, &frames).unwrap();
    });
}

#[test]
fn header_grammar_and_section_schema_dispatch_are_actual_source_ranges() {
    let bytes=b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('description'),'3;1');FILE_SCHEMA(('AP242','OTHER'));ENDSEC;DATA('body','AP242');#1=!VENDOR(1);ENDSEC;END-ISO-10303-21;";
    source(bytes, |ctx, view| {
        let mut frames = Vec::new();
        frame(ctx, view, FrameParent::File, &mut frames).unwrap();
        let dispatch = |level: &str, index: usize| {
            frames
                .iter()
                .filter(|part| part.level().as_str() == level)
                .nth(index)
                .unwrap()
                .dispatch()
                .iter()
                .map(|range| {
                    &bytes
                        [usize::try_from(range.start).unwrap()..usize::try_from(range.end).unwrap()]
                })
                .collect::<Vec<_>>()
        };
        assert_eq!(
            dispatch("step.header_record", 0),
            [b"FILE_DESCRIPTION".as_slice(), b"'3;1'".as_slice()]
        );
        assert_eq!(
            dispatch("step.header_record", 1),
            [
                b"FILE_SCHEMA".as_slice(),
                b"'AP242'".as_slice(),
                b"'OTHER'".as_slice()
            ]
        );
        assert_eq!(
            dispatch("step.data_section", 0),
            [b"('body','AP242')".as_slice()]
        );
        assert_eq!(
            dispatch("step.simple_entity_instance", 0),
            [b"!VENDOR".as_slice()]
        );
    });
}

#[test]
fn anchor_reference_and_signature_parts_have_their_distinct_parents() {
    let bytes=b"ISO-10303-21;HEADER;ENDSEC;ANCHOR;<name;literal>=#1;ENDSEC;REFERENCE;@9=<uri;literal>;ENDSEC;DATA;#1=F();ENDSEC;END-ISO-10303-21; SIGNATURE;AAENDSEC; /* ENDSEC; */ ENDSEC; /* one */ SIGNATURE;YmFy ENDSEC;";
    source(bytes, |ctx, view| {
        let mut frames = Vec::new();
        frame(ctx, view, FrameParent::File, &mut frames).unwrap();
        let anchor = frames
            .iter()
            .find(|part| part.level().as_str() == "step.anchor_entry")
            .unwrap();
        assert_eq!(anchor.view().window(), b"<name;literal>=#1;");
        assert_eq!(anchor.framing().len(), 2);
        assert!(anchor.dispatch().is_empty());
        let reference = frames
            .iter()
            .find(|part| part.level().as_str() == "step.reference_entry")
            .unwrap();
        assert_eq!(reference.view().window(), b"@9=<uri;literal>;");
        let prefix = reference.dispatch()[0].clone();
        assert_eq!(
            &bytes[usize::try_from(prefix.start).unwrap()..usize::try_from(prefix.end).unwrap()],
            b"@"
        );
        let signatures = frames
            .iter()
            .filter(|part| part.level().as_str() == "step.signature_section")
            .collect::<Vec<_>>();
        assert_eq!(signatures.len(), 2);
        assert!(signatures
            .iter()
            .all(|part| part.parent() == FrameParent::File));
        assert_eq!(
            signatures[0].view().window(),
            b"SIGNATURE;AAENDSEC; /* ENDSEC; */ ENDSEC; /* one */ "
        );
        assert_eq!(signatures[1].view().window(), b"SIGNATURE;YmFy ENDSEC;");
        PartTable::load().unwrap().validate(ctx, &frames).unwrap();
    });
}

#[test]
fn draft_preamble_and_control_octets_do_not_change_source_coordinates() {
    let bytes = b"DRAFT !* preamble ; *! STEP;HEADER;ENDSEC;DATA;@1=F('!* ; *!');ENDSEC;ENDSTEP;";
    source(bytes, |ctx, view| {
        let mut frames = Vec::new();
        frame(ctx, view, FrameParent::File, &mut frames).unwrap();
        assert_eq!(frames[0].level().as_str(), "step.draft_preamble");
        assert_eq!(frames[0].view().window(), b"DRAFT !* preamble ; *! ");
        assert!(frames[0].framing().is_empty());
        assert_eq!(frames[1].level().as_str(), "step.exchange");
        assert_eq!(
            frames[1].view().window(),
            b"STEP;HEADER;ENDSEC;DATA;@1=F('!* ; *!');ENDSEC;ENDSTEP;"
        );
        PartTable::load().unwrap().validate(ctx, &frames).unwrap();
    });
    source(
        b" \x01ISO-10303-\x0221;HEADER;ENDSEC;DATA;#1=F();ENDSEC;END-ISO-10303-21;",
        |ctx, view| {
            let mut frames = Vec::new();
            frame(ctx, view, FrameParent::File, &mut frames).unwrap();
            assert_eq!(frames[0].view().start(), 0);
            assert_eq!(
                frames
                    .iter()
                    .filter(|part| part.level().as_str() == "step.simple_entity_instance")
                    .count(),
                1
            );
        },
    );
}

#[test]
fn cut_short_and_unsupported_content_retain_only_real_available_extents() {
    let bytes = b"ISO-10303-21;HEADER;ENDSEC;DATA;#1=!UNSUPPORTED(0);#2=(A(1) B(";
    source(bytes, |ctx, view| {
        let mut frames = Vec::new();
        frame(ctx, view, FrameParent::File, &mut frames).unwrap();
        assert_eq!(frames[0].view().end(), bytes.len());
        assert_eq!(frames[0].framing(), &[0..13]);
        let last = frames.last().unwrap();
        assert_eq!(last.level().as_str(), "step.complex_entity_instance");
        assert_eq!(last.view().window(), b"#2=(A(1) B(");
        assert!(last
            .framing()
            .iter()
            .all(|range| range.end <= u64::try_from(bytes.len()).unwrap()));
        PartTable::load().unwrap().validate(ctx, &frames).unwrap();
    });
    for bytes in [
        b"not STEP".as_slice(),
        b"ISO-10303-21".as_slice(),
        b"'ISO-10303-21;'".as_slice(),
    ] {
        source(bytes, |ctx, view| {
            let mut frames = Vec::new();
            frame(ctx, view, FrameParent::File, &mut frames).unwrap();
            assert!(frames.is_empty());
            assert!(rebuild(ctx, view, &[]).is_err());
        });
    }
}

#[test]
fn framing_and_dispatch_are_not_semantic_record_reads() {
    source(SOURCE, |ctx, view| {
        let mut frames = Vec::new();
        ctx.record("test:record#framer", view, || {
            frame(ctx, view, FrameParent::File, &mut frames)
                .map_err(cadmpeg_core::CodecError::malformed)
        })
        .unwrap();
        assert!(ctx
            .take_record_captures()
            .unwrap()
            .iter()
            .all(|capture| capture.reads().is_empty()));
        assert!(frames.iter().all(|part| part.capture().stream() == "file"));
    });
}

#[test]
fn zip_root_member_retains_the_real_entry_name_as_external_dispatch() {
    let bytes = crate::zip_fixture::archive(&[crate::zip_fixture::RawEntry::stored(
        "ISO-10303.p21",
        SOURCE,
    )]);
    source(&bytes, |ctx, view| {
        let name_at = bytes
            .windows(b"ISO-10303.p21".len())
            .position(|part| part == b"ISO-10303.p21")
            .unwrap();
        let payload_at = bytes
            .windows(SOURCE.len())
            .position(|part| part == SOURCE)
            .unwrap();
        let payload = view.child(payload_at, payload_at + SOURCE.len()).unwrap();
        let name = view
            .child(name_at, name_at + b"ISO-10303.p21".len())
            .unwrap();
        let archive = FramedPart::new(
            ctx,
            PartLevel::parse("zip.archive").unwrap(),
            view,
            FrameParent::File,
            vec![],
            vec![],
        )
        .unwrap();
        let entry = FramedPart::new(
            ctx,
            PartLevel::parse("zip.entry").unwrap(),
            payload,
            FrameParent::Part(0),
            vec![],
            vec![],
        )
        .unwrap();
        let mut frames = vec![archive, entry];
        frame_zip_root(ctx, payload, name, 1, &mut frames).unwrap();
        assert_eq!(frames[2].level().as_str(), "step.zip_root_member");
        assert_eq!(frames[2].parent(), FrameParent::Part(1));
        assert_eq!(frames[2].external_dispatch()[0].window(), b"ISO-10303.p21");
        assert_eq!(frames[3].parent(), FrameParent::Host(2));
        assert_eq!(frames[3].view().start(), payload_at);
        assert_eq!(
            frames[3].capture().offset(),
            u64::try_from(payload_at).unwrap()
        );
        PartTable::load().unwrap().validate(ctx, &frames).unwrap();
    });
}

#[test]
fn rebuild_splices_content_and_dispatch_without_inventing_or_changing_framing() {
    let bytes = b"ISO-10303-21;HEADER;ENDSEC;DATA;#1=F('old');#2=G(2);ENDSEC;END-ISO-10303-21;";
    source(bytes, |ctx, view| {
        let start = bytes.windows(5).position(|part| part == b"'old'").unwrap();
        let rebuilt = rebuild(
            ctx,
            view,
            &[Edit {
                range: u64::try_from(start).unwrap()..u64::try_from(start + 5).unwrap(),
                replacement: b"'longer;value'",
            }],
        )
        .unwrap();
        assert_eq!(rebuilt,b"ISO-10303-21;HEADER;ENDSEC;DATA;#1=F('longer;value');#2=G(2);ENDSEC;END-ISO-10303-21;");
        let kind = bytes.windows(2).position(|part| part == b"F(").unwrap();
        let rebuilt = rebuild(
            ctx,
            view,
            &[Edit {
                range: u64::try_from(kind).unwrap()..u64::try_from(kind + 1).unwrap(),
                replacement: &[0xff],
            }],
        )
        .unwrap();
        assert_eq!(rebuilt[kind], 0xff);
        assert_eq!(&rebuilt[kind + 1..], &bytes[kind + 1..]);
        assert!(
            rebuild(
                ctx,
                view,
                &[Edit {
                    range: u64::try_from(start).unwrap()..u64::try_from(start + 5).unwrap(),
                    replacement: b"'unterminated",
                }]
            )
            .is_err(),
            "content cannot swallow source framing"
        );
        assert!(rebuild(
            ctx,
            view,
            &[Edit {
                range: 0..1,
                replacement: b"X"
            }]
        )
        .is_err());
    });
}

#[test]
fn half_and_ninety_percent_cuts_keep_every_wholly_preceding_part() {
    for percent in [50, 90] {
        let cut = SOURCE.len() * percent / 100;
        let shortened = &SOURCE[..cut];
        source(shortened, |ctx, view| {
            let mut frames = Vec::new();
            frame(ctx, view, FrameParent::File, &mut frames).unwrap();
            assert!(!frames.is_empty());
            assert_eq!(frames[0].view().end(), cut);
            assert!(frames.iter().all(|part| part.view().end() <= cut));
            let header = frames
                .iter()
                .find(|part| part.level().as_str() == "step.header_record")
                .unwrap();
            assert_eq!(header.view().window(), b"FILE_SCHEMA(('A;B'));");
            PartTable::load().unwrap().validate(ctx, &frames).unwrap();
        });
    }
}

#[test]
fn retained_bytes_cannot_be_promoted_to_a_source_frame() {
    source(SOURCE, |ctx, _| {
        let detached = View::over_retained(SOURCE);
        let mut frames = Vec::new();
        assert!(frame(ctx, detached, FrameParent::File, &mut frames).is_err());
        assert!(frames.is_empty());
    });
}
