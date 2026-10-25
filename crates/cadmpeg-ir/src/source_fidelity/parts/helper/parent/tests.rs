// SPDX-License-Identifier: Apache-2.0

use super::*;
use crate::source_fidelity::parts::{DecodeParts, PartSpec};
use cadmpeg_core::decode::{DecodeArena, DecodePolicy};

fn spec(level: &str) -> PartSpec {
    PartSpec::new(PartLevel::parse(level).unwrap(), Vec::new())
}

#[test]
fn declared_source_parent_is_present_at_birth_and_survives_unrelated_xml_rollback() {
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(b"xml-entry", &arena, &DecodePolicy::default()).unwrap();
    let mut output = PartOutput::new(crate::CodecFormat::Fcstd);
    let entry = root.child(4, 9).unwrap();
    ctx.decode_part(&mut output, entry, spec("zip.entry"), |_| Ok(()))
        .unwrap();
    let parent = output.parent(&ctx, entry, "zip.entry").unwrap();
    ctx.decode_part(
        &mut output,
        root.child(0, 3).unwrap(),
        spec("freecad.property"),
        |stage| {
            ctx.decode_part(
                stage,
                root.child(4, 6).unwrap(),
                spec("freecad.string_table_entry").with_parent(&parent),
                |child| {
                    ctx.record(
                        "test:native:row#accepted",
                        root.child(4, 6).unwrap(),
                        || {
                            assert_eq!(root.read_slice(4..6), Some(b"en".as_slice()));
                            child.insert_native(
                                &ctx,
                                "fixture_records",
                                crate::NativeUnknownRecord {
                                    id: UnknownId::try_from("test:native:row#accepted".to_owned())
                                        .map_err(CodecError::malformed)?,
                                    links: Vec::new(),
                                },
                            )
                        },
                    )
                },
            )?;
            Err::<(), _>(CodecError::malformed("damaged XML header"))
        },
    )
    .unwrap();
    let (_, native, fidelity, losses) = output.into_parts(&ctx).unwrap().into_parts();
    assert!(!native
        .namespace("fcstd")
        .unwrap()
        .arenas()
        .contains_key("fixture_records"));
    assert!(ctx
        .take_record_captures()
        .unwrap()
        .iter()
        .all(|capture| capture.id() != "test:native:row#accepted"));
    let entry_id = fidelity
        .parts()
        .iter()
        .find(|(_, part)| part.level().as_str() == "zip.entry")
        .unwrap()
        .0;
    let child = fidelity
        .parts()
        .values()
        .find(|part| part.level().as_str() == "freecad.string_table_entry")
        .unwrap();
    assert_eq!(child.parent(), Some(entry_id));
    assert_eq!(child.state(), &PartState::Decoded {});
    assert_eq!(
        fidelity
            .parts()
            .values()
            .filter(|part| matches!(part.state(), PartState::GivenUp {}))
            .count(),
        1
    );
    assert_eq!(losses.len(), 1);
}

#[test]
fn a_given_up_independent_source_child_keeps_its_own_home_after_xml_rollback() {
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(b"xml-entry", &arena, &DecodePolicy::default()).unwrap();
    let mut output = PartOutput::new(crate::CodecFormat::Fcstd);
    let entry = root.child(4, 9).unwrap();
    ctx.decode_part(&mut output, entry, spec("zip.entry"), |_| Ok(()))
        .unwrap();
    let parent = output.parent(&ctx, entry, "zip.entry").unwrap();
    ctx.decode_part(
        &mut output,
        root.child(0, 3).unwrap(),
        spec("freecad.property"),
        |stage| {
            ctx.decode_part(
                stage,
                root.child(4, 6).unwrap(),
                spec("freecad.string_table_entry").with_parent(&parent),
                |_| {
                    ctx.record("test:native:row#failed", root.child(4, 6).unwrap(), || {
                        Err::<(), _>(CodecError::malformed("damaged entry"))
                    })
                },
            )?;
            Err::<(), _>(CodecError::malformed("damaged XML header"))
        },
    )
    .unwrap();
    let (_, native, fidelity, losses) = output.into_parts(&ctx).unwrap().into_parts();
    assert_eq!(
        native.namespace("fcstd").unwrap().arenas()["unknowns"].len(),
        2
    );
    assert_eq!(losses.len(), 2);
    assert_eq!(
        losses
            .iter()
            .filter(|loss| loss
                .message
                .contains("dropped=[\"test:native:row#failed\"]"))
            .count(),
        1
    );
    let child = fidelity
        .parts()
        .iter()
        .find(|(_, part)| part.level().as_str() == "freecad.string_table_entry")
        .unwrap();
    assert_eq!(child.1.state(), &PartState::GivenUp {});
    assert_eq!(
        fidelity.retained_record(child.0.as_str()).unwrap().data(),
        Some(b"en".as_slice())
    );
}

#[test]
fn parent_authority_rejects_rollback_foreign_session_and_unrelated_source() {
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(b"xml-entry", &arena, &DecodePolicy::default()).unwrap();
    let mut output = PartOutput::new(crate::CodecFormat::Fcstd);
    let entry = root.child(4, 9).unwrap();
    let mut stale = None;
    ctx.decode_part(&mut output, root, spec("zip.archive"), |stage| {
        ctx.decode_part(stage, entry, spec("zip.entry"), |_| Ok(()))?;
        stale = Some(stage.parent(&ctx, entry, "zip.entry")?);
        Err::<(), _>(CodecError::malformed("archive rollback"))
    })
    .unwrap();
    let stale = stale.unwrap();
    assert!(ctx
        .decode_part(
            &mut output,
            root.child(4, 6).unwrap(),
            spec("freecad.string_table_entry").with_parent(&stale),
            |_| Ok(())
        )
        .is_err());
    let mut output = PartOutput::new(crate::CodecFormat::Fcstd);
    ctx.decode_part(&mut output, entry, spec("zip.entry"), |_| Ok(()))
        .unwrap();
    let parent = output.parent(&ctx, entry, "zip.entry").unwrap();
    assert!(ctx
        .decode_part(
            &mut output,
            root.child(0, 2).unwrap(),
            spec("freecad.string_table_entry").with_parent(&parent),
            |_| Ok(())
        )
        .is_err());
    let other_arena = DecodeArena::new();
    let (other, other_root) =
        DecodeContext::from_root_bytes(b"xml-entry", &other_arena, &DecodePolicy::default())
            .unwrap();
    assert!(other
        .decode_part(
            &mut output,
            other_root.child(4, 6).unwrap(),
            spec("freecad.string_table_entry").with_parent(&parent),
            |_| Ok(())
        )
        .is_err());
}
