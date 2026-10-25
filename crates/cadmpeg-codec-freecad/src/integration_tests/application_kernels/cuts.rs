// SPDX-License-Identifier: Apache-2.0
//! Each independently located kernel carrier has a closed physical suffix control.

use super::*;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_ir::source_fidelity::parts::PartState;
use cadmpeg_test_support::comparison::{assert_decodes_agree, Allowances, DecodeSnapshot, Mode};
use cadmpeg_test_support::framing::{
    freecad::binary::{frame_binary_member, BinaryKind},
    zip,
};
use std::collections::{BTreeMap, BTreeSet};

mod oracle;

fn decode(bytes: &[u8]) -> cadmpeg_ir::codec::DecodeResult {
    FcstdCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap()
}

pub(super) fn compare(
    before: &cadmpeg_ir::codec::DecodeResult,
    after: &cadmpeg_ir::codec::DecodeResult,
    original: &[u8],
    bytes: &[u8],
) {
    compare_expected(before, after, original, bytes, before.ir());
}

fn compare_expected(
    before: &cadmpeg_ir::codec::DecodeResult,
    after: &cadmpeg_ir::codec::DecodeResult,
    original: &[u8],
    bytes: &[u8],
    expected: &cadmpeg_ir::CadIr,
) {
    let old_arena = DecodeArena::new();
    let (old_ctx, old_root) =
        DecodeContext::from_root_bytes(original, &old_arena, &DecodePolicy::service()).unwrap();
    let old_root = old_ctx.name_stream(old_root, "file").unwrap();
    let new_arena = DecodeArena::new();
    let (new_ctx, new_root) =
        DecodeContext::from_root_bytes(bytes, &new_arena, &DecodePolicy::service()).unwrap();
    let new_root = new_ctx.name_stream(new_root, "file").unwrap();
    let allowances = if after
        .source_fidelity()
        .parts()
        .values()
        .any(|part| part.state() == &(PartState::GivenUp {}))
    {
        Allowances::given_up_zip(
            DecodeSnapshot::new(before.ir(), before.source_fidelity()),
            DecodeSnapshot::new(after.ir(), after.source_fidelity()),
            after.report(),
            (&old_ctx, old_root),
            (&new_ctx, new_root),
        )
        .unwrap()
    } else {
        Allowances::default()
    };
    assert_decodes_agree(
        &serde_json::to_value(expected).unwrap(),
        &serde_json::to_value(after.ir()).unwrap(),
        &allowances,
        Mode::Same,
    );
    crate::fidelity::b1::assert_classified_spans(after);
    assert!(crate::test_support::validate_native(after.ir()).is_empty());
}

#[test]
fn every_kernel_level_cut_short_has_its_actual_home_and_preserves_the_other_kernel() {
    let original = source();
    let before = decode(&original);
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(&original, &arena, &DecodePolicy::service()).unwrap();
    let root = ctx.name_stream(root, "file").unwrap();
    let mut frames = Vec::new();
    let entries = zip::frame(&ctx, root, &mut frames).unwrap();
    let archive = cadmpeg_container::ArchiveSnapshot::new(&ctx, root).unwrap();
    let mut levels = BTreeMap::<String, Vec<(String, u64, u64)>>::new();
    for (name, kind) in [
        ("MeshKernel.bms", BinaryKind::Mesh),
        ("Cloud", BinaryKind::Points),
    ] {
        let entry = entries
            .iter()
            .find(|entry| entry.name().window() == name.as_bytes())
            .unwrap();
        let start = frames.len();
        frame_binary_member(
            &ctx,
            archive.open(&ctx, name).unwrap(),
            kind,
            entry.frame_index(),
            &mut frames,
        )
        .unwrap();
        for frame in &frames[start..] {
            levels
                .entry(frame.level().as_str().to_owned())
                .or_default()
                .push((
                    name.into(),
                    frame.capture().offset(),
                    frame.capture().byte_len(),
                ));
        }
    }
    assert_eq!(levels.len(), 7);
    let original_parts = levels.clone();
    for (level, parts) in levels {
        for index in [0, parts.len() / 2, parts.len() - 1]
            .into_iter()
            .collect::<BTreeSet<_>>()
        {
            let (name, start, length) = &parts[index];
            for cut in [start + 1, start + length - 1]
                .into_iter()
                .collect::<BTreeSet<_>>()
            {
                let mesh = name == "MeshKernel.bms";
                let cut = usize::try_from(cut).unwrap();
                let bytes = source_with_cuts(mesh.then_some(cut), (!mesh).then_some(cut));
                let after = decode(&bytes);
                let failures = after
                    .source_fidelity()
                    .parts()
                    .values()
                    .filter(|part| part.state() == &(PartState::GivenUp {}))
                    .collect::<Vec<_>>();
                assert_eq!(
                    failures.len(),
                    1,
                    "{level} cut {cut}: {:?}",
                    after.report().losses
                );
                let failed = failures[0];
                let changed_arena = DecodeArena::new();
                let (changed_ctx, changed_root) = DecodeContext::from_root_bytes(
                    &bytes,
                    &changed_arena,
                    &DecodePolicy::service(),
                )
                .unwrap();
                let changed_root = changed_ctx.name_stream(changed_root, "file").unwrap();
                let mut changed_frames = Vec::new();
                let changed_entries =
                    zip::frame(&changed_ctx, changed_root, &mut changed_frames).unwrap();
                let changed_archive =
                    cadmpeg_container::ArchiveSnapshot::new(&changed_ctx, changed_root).unwrap();
                let changed_entry = changed_entries
                    .iter()
                    .find(|entry| entry.name().window() == name.as_bytes())
                    .unwrap();
                let first = changed_frames.len();
                let framing = frame_binary_member(
                    &changed_ctx,
                    changed_archive.open(&changed_ctx, name).unwrap(),
                    if mesh {
                        BinaryKind::Mesh
                    } else {
                        BinaryKind::Points
                    },
                    changed_entry.frame_index(),
                    &mut changed_frames,
                );
                assert_eq!(framing.is_err(), !mesh || cut < 272);
                let expected = changed_frames[first..].last().unwrap();
                assert_eq!(failed.level(), expected.level(), "{level} cut {cut}");
                assert_eq!(failed.stream(), expected.capture().stream());
                assert_eq!(failed.offset(), expected.capture().offset());
                assert_eq!(failed.byte_len(), expected.capture().byte_len());
                assert_eq!(failed.framing_bytes(), expected.framing());
                let reached = after
                    .source_fidelity()
                    .parts()
                    .values()
                    .filter(|part| {
                        part.stream() == expected.capture().stream()
                            && original_parts.contains_key(part.level().as_str())
                    })
                    .collect::<Vec<_>>();
                assert_eq!(reached.len(), changed_frames.len() - first);
                for frame in &changed_frames[first..] {
                    let actual = reached
                        .iter()
                        .filter(|part| {
                            part.level() == frame.level()
                                && part.offset() == frame.capture().offset()
                                && part.byte_len() == frame.capture().byte_len()
                        })
                        .collect::<Vec<_>>();
                    assert_eq!(actual.len(), 1, "one actual bounded kernel part");
                    assert_eq!(actual[0].framing_bytes(), frame.framing());
                }
                let errors = after
                    .report()
                    .losses
                    .iter()
                    .filter(|loss| loss.code.local_code() == "record.unreadable")
                    .collect::<Vec<_>>();
                assert_eq!(errors.len(), 1);
                let provenance = errors[0].provenance.as_ref().unwrap();
                assert_eq!(provenance.stream_name(), Some(failed.stream()));
                assert_eq!(provenance.offset, failed.offset());
                assert_eq!(provenance.byte_len(), Some(failed.byte_len()));
                let expected = oracle::prefix(&ctx, before.ir(), name, cut, &original_parts);
                assert_eq!(
                    serde_json::to_value(&expected.model).unwrap(),
                    serde_json::to_value(&after.ir().model).unwrap(),
                    "exact known-arity neutral prefix: {level} cut {cut}"
                );
                assert_eq!(expected.scope, after.ir().scope);
                let entry_rows = |ir: &cadmpeg_ir::CadIr| {
                    ir.native
                        .namespace("fcstd")
                        .unwrap()
                        .arena_as::<crate::native::EntryRecord>("entries")
                        .unwrap()
                };
                assert_eq!(
                    serde_json::to_value(entry_rows(&expected)).unwrap(),
                    serde_json::to_value(entry_rows(after.ir())).unwrap(),
                    "exact native slots and every independent entry: {level} cut {cut}"
                );
                compare_expected(&before, &after, &original, &bytes, &expected);
                if mesh {
                    assert_eq!(after.ir().model.points, before.ir().model.points);
                } else {
                    assert_eq!(
                        after.ir().model.tessellations,
                        before.ir().model.tessellations
                    );
                }
                compare(&before, &after, &original, &bytes);
            }
        }
    }
}

#[test]
fn kernel_archive_half_and_ninety_percent_cuts_have_one_actual_archive_failure() {
    let original = source();
    let before = decode(&original);
    for percent in [50, 90] {
        let bytes = &original[..original.len() * percent / 100];
        let arena = DecodeArena::new();
        let (ctx, root) =
            DecodeContext::from_root_bytes(bytes, &arena, &DecodePolicy::service()).unwrap();
        let root = ctx.name_stream(root, "file").unwrap();
        let mut independent = Vec::new();
        let scanned = zip::scan(&ctx, root, &mut independent)
            .expect("the actual cut begins with a bounded local ZIP header");
        let after = decode(bytes);
        let failures = after
            .source_fidelity()
            .parts()
            .values()
            .filter(|part| part.state() == &(PartState::GivenUp {}))
            .collect::<Vec<_>>();
        match scanned {
            zip::ZipScan::Complete { .. } => {
                panic!("a strict prefix cannot retain the original ZIP end record")
            }
            zip::ZipScan::Local { entries, stop }
                if matches!(
                    stop.reason(),
                    zip::RecoveryReason::SourceEnd | zip::RecoveryReason::FollowingStructure
                ) =>
            {
                assert!(
                    failures.is_empty(),
                    "archive cut {percent}% stopped after complete local members: {:?}",
                    after.report().losses
                );
                let original_entries = before
                    .ir()
                    .native
                    .namespace("fcstd")
                    .unwrap()
                    .arena_as::<crate::native::EntryRecord>("entries")
                    .unwrap();
                assert_eq!(entries.len(), original_entries.len(), "all original local members must be physically complete for this no-failure control");
                assert_eq!(
                    serde_json::to_value(before.ir()).unwrap(),
                    serde_json::to_value(after.ir()).unwrap(),
                    "recovering complete local members preserves every held record"
                );
            }
            zip::ZipScan::Local { stop, .. } => {
                assert!(
                    stop.frame_index().is_some(),
                    "the actual incomplete carrier has no independent locator"
                );
                assert_eq!(
                    failures.len(),
                    1,
                    "archive cut {percent}%: {:?}",
                    after.report().losses
                );
                for failed in &failures {
                    assert!(
                        independent.iter().any(|part| part.level() == failed.level()
                            && part.capture().stream() == failed.stream()
                            && part.capture().offset() == failed.offset()
                            && part.capture().byte_len() == failed.byte_len()),
                        "failed part must have its actual independent source boundary: {failed:?}"
                    );
                    let losses = after
                        .report()
                        .losses
                        .iter()
                        .filter(|loss| {
                            loss.code.local_code() == "record.unreadable"
                                && loss.provenance.as_ref().is_some_and(|source| {
                                    source.stream_name() == Some(failed.stream())
                                        && source.offset == failed.offset()
                                        && source.byte_len().unwrap_or(0) == failed.byte_len()
                                })
                        })
                        .count();
                    assert_eq!(losses, 1);
                }
            }
        }
        let compared = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            compare(&before, &after, &original, bytes);
        }));
        assert!(
            compared.is_ok(),
            "kernel archive {percent}% prefix ({} of {} bytes), actual failures={failures:?}; applications before={} after={}; entries after={}",
            bytes.len(),
            original.len(),
            serde_json::to_value(before.ir()).unwrap()["native"]["fcstd"]["applications"],
            serde_json::to_value(after.ir()).unwrap()["native"]["fcstd"]["applications"],
            serde_json::to_value(after.ir()).unwrap()["native"]["fcstd"]["entries"],
        );
    }
}
