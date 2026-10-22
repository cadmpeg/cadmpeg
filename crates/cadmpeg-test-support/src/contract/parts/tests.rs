// SPDX-License-Identifier: Apache-2.0
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_ir::codec::DecodeLossCode;
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::source_fidelity::parts::{DecodeParts, ParserPartOutput, PartLevel, PartSpec};
use cadmpeg_ir::{CodecFormat, SourceFidelity};

fn image(run: impl FnOnce(&[u8], &cadmpeg_ir::Native, &SourceFidelity, &[LossNote])) {
    let bytes = b"#1=BAD;";
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(bytes, &arena, &DecodePolicy::service()).unwrap();
    let mut output = ParserPartOutput::new(CodecFormat::Step);
    assert!(ctx
        .decode_parser_part(
            &mut output,
            root,
            PartSpec::new(
                PartLevel::parse("step.simple_entity_instance").unwrap(),
                vec![6..7]
            ),
            || {
                let mut reader = root;
                reader.req_u64_le()?;
                Ok(())
            }
        )
        .unwrap()
        .is_none());
    let (_, native, fidelity, losses) = output.into_parts(&ctx).unwrap().into_parts();
    run(bytes, &native, &fidelity, &losses);
}

fn check(
    bytes: &[u8],
    native: &cadmpeg_ir::Native,
    fidelity: &SourceFidelity,
    losses: &[LossNote],
) -> Result<(), String> {
    super::check_one_home(
        CodecFormat::Step,
        native,
        fidelity,
        losses,
        "file",
        |name, range| {
            if name != "file" {
                return None;
            }
            bytes.get(usize::try_from(range.start).ok()?..usize::try_from(range.end).ok()?)
        },
    )
}

#[test]
fn exact_helper_home_passes_and_unavailable_or_changed_source_fails() {
    image(|bytes, native, fidelity, losses| {
        check(bytes, native, fidelity, losses).unwrap();
        assert!(check(b"#1=BAD?", native, fidelity, losses)
            .unwrap_err()
            .contains("retained image"));
        assert!(super::check_one_home(
            CodecFormat::Step,
            native,
            fidelity,
            losses,
            "file",
            |_, _| None
        )
        .unwrap_err()
        .contains("source bytes are unavailable"));
    });
}

#[test]
fn missing_duplicate_stubs_and_absent_or_duplicate_unreadable_losses_fail() {
    image(|bytes, native, fidelity, losses| {
        let mut missing = native.clone();
        missing
            .namespace_mut("step")
            .arenas_mut()
            .get_mut("unknowns")
            .unwrap()
            .clear();
        assert!(check(bytes, &missing, fidelity, losses)
            .unwrap_err()
            .contains("0 unknown stubs"));
        let mut duplicate = native.clone();
        let arena = duplicate
            .namespace_mut("step")
            .arenas_mut()
            .get_mut("unknowns")
            .unwrap();
        arena.push(arena[0].clone());
        assert!(check(bytes, &duplicate, fidelity, losses)
            .unwrap_err()
            .contains("2 unknown stubs"));
        let mut other_namespace = native.clone();
        let duplicate = native.namespace("step").unwrap().arenas()["unknowns"][0].clone();
        other_namespace
            .namespace_mut("rhino")
            .arenas_mut()
            .insert("unknowns".into(), vec![duplicate]);
        assert!(check(bytes, &other_namespace, fidelity, losses)
            .unwrap_err()
            .contains("2 native records across namespaces"));
        assert!(check(bytes, native, fidelity, &[])
            .unwrap_err()
            .contains("0 exact unreadable losses"));
        let duplicate = [losses[0].clone(), losses[0].clone()];
        assert!(check(bytes, native, fidelity, &duplicate)
            .unwrap_err()
            .contains("2 exact unreadable losses"));
    });
}

#[test]
fn an_other_loss_inside_the_part_and_a_late_contained_record_layout_fail() {
    image(|bytes, native, fidelity, losses| {
        let mut conflicting = losses.to_vec();
        let mut other = DecodeLossCode::SidecarStale.note("competing span");
        other.provenance = losses[0].provenance.clone();
        conflicting.push(other);
        assert!(check(bytes, native, fidelity, &conflicting)
            .unwrap_err()
            .contains("also names source inside"));
        let arena = DecodeArena::new();
        let (ctx, root) =
            DecodeContext::from_root_bytes(bytes, &arena, &DecodePolicy::service()).unwrap();
        ctx.record("step:native:intruder#1", root.child(1, 5).unwrap(), || {
            Ok(())
        })
        .unwrap();
        let mut stale = fidelity.clone();
        stale.install_mapped_record_captures(&ctx, Ok).unwrap();
        assert!(check(bytes, native, &stale, losses)
            .unwrap_err()
            .contains("layout inside given-up"));
    });
}

#[test]
fn a_reserved_part_stub_cannot_be_orphaned_in_another_native_namespace() {
    image(|bytes, native, _fidelity, losses| {
        let mut misplaced = native.clone();
        let stub = misplaced
            .namespace_mut("step")
            .arenas_mut()
            .get_mut("unknowns")
            .unwrap()
            .pop()
            .unwrap();
        misplaced
            .namespace_mut("rhino")
            .arenas_mut()
            .insert("unknowns".into(), vec![stub]);
        assert!(check(bytes, &misplaced, &SourceFidelity::default(), losses)
            .unwrap_err()
            .contains("has no given-up source part"));
    });
}

#[test]
fn unrelated_ordinary_unknowns_in_another_native_namespace_remain_allowed() {
    image(|bytes, native, fidelity, losses| {
        let mut combined = native.clone();
        let mut ordinary = native
            .namespace("step")
            .unwrap()
            .arena_as::<cadmpeg_ir::NativeUnknownRecord>("unknowns")
            .unwrap()[0]
            .clone();
        ordinary.id =
            cadmpeg_ir::ids::UnknownId::try_from("rhino:object:unknown#ordinary".to_owned())
                .unwrap();
        let arena = DecodeArena::new();
        let (ctx, _) =
            DecodeContext::from_root_bytes(bytes, &arena, &DecodePolicy::service()).unwrap();
        combined
            .namespace_mut("rhino")
            .set_arena(&ctx, "unknowns", &[ordinary])
            .unwrap();
        check(bytes, &combined, fidelity, losses).unwrap();
    });
}
