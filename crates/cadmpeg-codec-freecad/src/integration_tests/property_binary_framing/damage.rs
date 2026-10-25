// SPDX-License-Identifier: Apache-2.0
//! Literal nonframing mutations and checked shared decode comparison.

use super::*;
use cadmpeg_ir::source_fidelity::parts::PartState;

pub(super) fn mutate(bytes: &[u8], target: &Boundary, fill: u8) -> Vec<u8> {
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(bytes, &arena, &DecodePolicy::service()).unwrap();
    let entries = zip::frame(&ctx, root, &mut Vec::new()).unwrap();
    let entry = entries
        .iter()
        .find(|entry| entry.name().window() == target.member.as_bytes())
        .unwrap();
    assert_eq!(entry.method(), 0);
    let mut member = entry.payload().window().to_vec();
    // The target's Appendix C framing alone is protected. Descendant framing
    // does not widen the target's own declared framing byte set.
    for position in target.offset..target.offset + target.length {
        if !target.framing.iter().any(|range| range.contains(&position)) {
            member[usize::try_from(position).unwrap()] = fill;
        }
    }
    zip::rebuild(
        &ctx,
        root,
        &[zip::Edit {
            range: u64::try_from(entry.payload().start()).unwrap()
                ..u64::try_from(entry.payload().end()).unwrap(),
            replacement: &member,
        }],
    )
    .unwrap()
}

pub(super) fn compare(
    before: &DecodeResult,
    after: &DecodeResult,
    bytes: &[u8],
    target: &Boundary,
) {
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(bytes, &arena, &DecodePolicy::service()).unwrap();
    let root = ctx.name_stream(root, "file").unwrap();
    let archive = cadmpeg_container::ArchiveSnapshot::new(&ctx, root).unwrap();
    let changed = archive.open(&ctx, &target.member).unwrap();
    let expected = super::oracle::expected(before, &target.stream, changed.window());
    compare_expected(before, after, bytes, target, &expected, None);
}

pub(super) fn compare_expected(
    before: &DecodeResult,
    after: &DecodeResult,
    bytes: &[u8],
    target: &Boundary,
    expected: &cadmpeg_ir::CadIr,
    original_bytes: Option<&[u8]>,
) {
    use cadmpeg_test_support::comparison::{
        assert_decodes_agree, Allowances, DecodeSnapshot, Mode,
    };
    let failures = after
        .source_fidelity()
        .parts()
        .values()
        .filter(|part| matches!(part.state(), PartState::GivenUp {}))
        .collect::<Vec<_>>();
    let errors = after
        .report()
        .losses
        .iter()
        .filter(|loss| loss.code.local_code() == "record.unreadable")
        .collect::<Vec<_>>();
    assert_eq!(
        errors.len(),
        failures.len(),
        "one error per actual failed part: {}",
        target.level
    );
    assert!(errors
        .iter()
        .all(|loss| loss.severity == cadmpeg_ir::report::Severity::Error));
    for part in failures {
        assert_eq!(part.stream(), target.stream, "{}", target.level);
        assert!(
            part.offset() >= target.offset
                && part.offset() + part.byte_len() <= target.offset + target.length,
            "failure escapes mutated part {}: {part:?}",
            target.level
        );
        let exact = errors
            .iter()
            .filter(|loss| {
                loss.provenance.as_ref().is_some_and(|p| {
                    p.stream_name() == Some(part.stream())
                        && p.offset == part.offset()
                        && p.byte_len() == Some(part.byte_len())
                })
            })
            .count();
        assert_eq!(exact, 1, "{}", target.level);
    }
    crate::fidelity::b1::assert_classified_spans(after);
    assert!(
        crate::test_support::validate_native(after.ir()).is_empty(),
        "{}",
        target.level
    );
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(bytes, &arena, &DecodePolicy::service()).unwrap();
    let root = ctx.name_stream(root, "file").unwrap();
    let archive = cadmpeg_container::ArchiveSnapshot::new(&ctx, root).unwrap();
    let entries = zip::frame(&ctx, root, &mut Vec::new()).unwrap();
    let sources = entries
        .iter()
        .map(|entry| {
            let member = archive
                .open(&ctx, std::str::from_utf8(entry.name().window()).unwrap())
                .unwrap();
            (ctx.capture_part_extent(member).unwrap(), member)
        })
        .collect::<Vec<_>>();
    let allowances = if let Some(original_bytes) = original_bytes {
        let original_arena = DecodeArena::new();
        let (original_ctx, original_root) = DecodeContext::from_root_bytes(
            original_bytes,
            &original_arena,
            &DecodePolicy::service(),
        )
        .unwrap();
        let original_root = original_ctx.name_stream(original_root, "file").unwrap();
        Allowances::given_up_zip(
            DecodeSnapshot::new(before.ir(), before.source_fidelity()),
            DecodeSnapshot::new(after.ir(), after.source_fidelity()),
            after.report(),
            (&original_ctx, original_root),
            (&ctx, root),
        )
        .unwrap()
    } else {
        Allowances::given_up(
            DecodeSnapshot::new(before.ir(), before.source_fidelity()),
            DecodeSnapshot::new(after.ir(), after.source_fidelity()),
            after.report(),
            "file",
            |stream, range| {
                if stream == "file" {
                    return root
                        .window()
                        .get(usize::try_from(range.start).ok()?..usize::try_from(range.end).ok()?);
                }
                let (capture, member) = sources
                    .iter()
                    .find(|(capture, _)| capture.stream() == stream)?;
                member.window().get(
                    usize::try_from(range.start.checked_sub(capture.offset())?).ok()?
                        ..usize::try_from(range.end.checked_sub(capture.offset())?).ok()?,
                )
            },
        )
        .unwrap()
    };
    assert_decodes_agree(
        &serde_json::to_value(expected).unwrap(),
        &serde_json::to_value(after.ir()).unwrap(),
        &allowances,
        Mode::Same,
    );
}
