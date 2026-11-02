// SPDX-License-Identifier: Apache-2.0
//! The framed entity-record stream is one §4.5 part: a failure inside it gives
//! up the part, retains its bytes and keeps the framed scope.

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_ir::document::{ArenaClass, CadIr};
use cadmpeg_ir::source_fidelity::parts::{DecodeParts, PartOutput, PartSpec};
use cadmpeg_ir::CodecFormat;
use cadmpeg_test_support::EditableDecodeResult;

use super::{decode_bytes, text_sphere_stream, SatLossCode};
use crate::test_support::test_streams::{
    attributed_sphere_stream, binary_sphere_stream, BinaryFixtureKind,
};

fn unreadable_losses(losses: &[cadmpeg_ir::report::loss::LossNote]) -> usize {
    losses
        .iter()
        .filter(|loss| loss.code.local_code() == "record.unreadable")
        .count()
}

#[test]
fn failed_stream_part_gives_up_with_scope_held_and_named_bytes() {
    let bytes = text_sphere_stream(1.0);
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service()).expect("source");
    let stream = cadmpeg_asm::sat::parse(&ctx, &bytes).expect("records frame");
    let header = stream.header.as_kernel_header();
    let evidence = super::super::StreamEvidence::Text(Some(super::super::TextEvidence {
        branch: stream.terminator,
        header: &header,
    }));
    let (_, kernel) = super::super::layers(&evidence);
    let first = stream.records.first().expect("sphere records");
    let last = stream.records.last().expect("sphere records");
    let start = first.offset;
    let end = last.offset + last.len;
    let extent = root.child(start, end).expect("record span");
    let mut output = PartOutput::new(CodecFormat::Sat, &ctx);
    let spec = PartSpec::new("stream").holding([ArenaClass::Model]);
    let decoded = ctx
        .decode_part(&mut output, extent, spec, |_stage| {
            Err::<(), _>(cadmpeg_core::CodecError::malformed(
                "synthetic part failure",
            ))
        })
        .expect("a part give-up is not a refusal");
    assert!(decoded.is_none());
    let part = super::super::stream_part_given_up(output).expect("a give-up assembles");
    let decoded = super::super::assemble_result(
        &ctx,
        CadIr::empty(),
        Vec::new(),
        true,
        Some(stream.terminator),
        &kernel,
        part,
    )
    .expect("a give-up decodes");
    assert!(decoded.ir.scope.model);
    assert!(decoded.ir.scope.design);
    assert!(!decoded.body.transfer.geometry_transferred());
    assert_eq!(unreadable_losses(&decoded.body.losses), 1);
    let loss = decoded
        .body
        .losses
        .iter()
        .find(|loss| loss.code.local_code() == "record.unreadable")
        .expect("give-up loss");
    assert!(loss.message.contains(&format!("offset={start}")));
    assert!(loss.message.contains(&format!("length={}", end - start)));
    assert!(decoded
        .body
        .losses
        .iter()
        .any(|loss| loss.code == SatLossCode::GeometryFramedWithoutCarriers.kind()));
    let stubs = decoded
        .ir
        .native
        .namespace("sat")
        .and_then(|namespace| namespace.arenas().get("unknowns"));
    let stub = stubs
        .and_then(|records| records.iter().next())
        .expect("one give-up stub");
    assert_eq!(stubs.map_or(0, Vec::len), 1);
    assert!(stub.id().starts_with("sat:source:part#"));
    assert_eq!(
        decoded
            .source_fidelity
            .retained_record(stub.id())
            .expect("stub bytes")
            .data(),
        Some(&bytes[start..end])
    );
    let unrepresented = decoded.source_fidelity.unrepresented();
    assert_eq!(unrepresented.len(), 1);
    assert!(unrepresented.contains(&ArenaClass::Model));
    assert!(cadmpeg_ir::validate_neutral(&decoded.ir, Vec::new())
        .expect("give-up document validates")
        .is_ok());
}

#[test]
fn damaged_records_keep_decoding_with_scope_held() {
    let sphere = String::from_utf8(text_sphere_stream(1.0)).expect("text fixture");
    let damaged = [
        sphere.replace(
            "face $-1 -1 $-1 $-1 $-1 $3 $-1 $5 forward single #",
            "face $-1 -1 BROKEN #",
        ),
        sphere.replace(
            "sphere-surface $-1 -1 $-1 0 0 0 25 1 0 0 0 0 1 forward_v I I I I #",
            "sphere-surface $-1 -1 $-1 0 0 0 25 #",
        ),
        sphere.replace("lump $-1 -1 $-1 $-1 $3 $1 #", "lump $-1 -1 surprise #"),
        sphere.replace(
            "face $-1 -1 $-1 $-1 $-1 $3 $-1 $5 forward single #",
            "face $-1 -1 $-1 $-1 $-1 $99 $-1 $5 forward single #",
        ),
    ];
    for source in damaged {
        let result = EditableDecodeResult::from(decode_bytes(source.as_bytes()));
        assert!(result.ir().scope.model);
        assert!(result.ir().scope.design);
        assert_eq!(unreadable_losses(&result.report().losses), 0);
    }
}

#[test]
fn cut_short_prefix_decodes_with_scope_held() {
    let bytes = text_sphere_stream(1.0);
    for percent in [50, 90] {
        let cut = &bytes[..bytes.len() * percent / 100];
        let result = EditableDecodeResult::from(decode_bytes(cut));
        assert!(result.ir().scope.model, "cut at {percent}%");
        assert!(result.ir().scope.design, "cut at {percent}%");
        assert!(!result.report().geometry_transferred());
        assert!(result
            .report()
            .losses
            .iter()
            .any(|loss| loss.code == SatLossCode::GeometryFramedWithoutCarriers.kind()));
    }
}

#[test]
fn goldens_charge_no_unexpected_give_up() {
    for bytes in [
        text_sphere_stream(1.0),
        attributed_sphere_stream(),
        binary_sphere_stream(BinaryFixtureKind::Asm),
        binary_sphere_stream(BinaryFixtureKind::Acis),
    ] {
        let result = EditableDecodeResult::from(decode_bytes(&bytes));
        assert_eq!(unreadable_losses(&result.report().losses), 0);
    }
}
