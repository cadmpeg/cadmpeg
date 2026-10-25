// SPDX-License-Identifier: Apache-2.0
//! Physical member suffix removal uses only independently proven source boundaries.

use super::*;

use cadmpeg_ir::source_fidelity::parts::PartState;

fn expectation(fixtures: &[fixtures::Fixture], bytes: &[u8], target: &Boundary) -> Boundary {
    let index = fixtures
        .iter()
        .enumerate()
        .find_map(|(index, _)| {
            ["app", "gui", "app-copy"]
                .iter()
                .any(|prefix| target.member == format!("{prefix}-{index}.bin"))
                .then_some(index)
        })
        .unwrap();
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(bytes, &arena, &DecodePolicy::service()).unwrap();
    let root = ctx.name_stream(root, "file").unwrap();
    let mut frames = Vec::new();
    let entries = zip::frame(&ctx, root, &mut frames).unwrap();
    let entry = entries
        .iter()
        .find(|entry| entry.name().window() == target.member.as_bytes())
        .unwrap();
    let archive = cadmpeg_container::ArchiveSnapshot::new(&ctx, root).unwrap();
    let member = archive.open(&ctx, &target.member).unwrap();
    let first = frames.len();
    assert!(
        frame_binary_member(
            &ctx,
            member,
            fixtures[index].kind,
            entry.frame_index(),
            &mut frames,
        )
        .is_err(),
        "a physical cut must stop inside the independently bounded record"
    );
    let frame = frames[first..].last().unwrap();
    Boundary {
        member: target.member.clone(),
        level: frame.level().as_str().into(),
        stream: frame.capture().stream().into(),
        offset: frame.capture().offset(),
        length: frame.capture().byte_len(),
        framing: frame.framing().to_vec(),
    }
}

fn member_stream(bytes: &[u8], name: &str) -> String {
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(bytes, &arena, &DecodePolicy::service()).unwrap();
    let root = ctx.name_stream(root, "file").unwrap();
    let archive = cadmpeg_container::ArchiveSnapshot::new(&ctx, root).unwrap();
    let member = archive.open(&ctx, name).unwrap();
    ctx.capture_part_extent(member).unwrap().stream().to_owned()
}

fn expected_ir(
    before: &DecodeResult,
    stream: &str,
    bytes: &[u8],
    member: &str,
) -> cadmpeg_ir::CadIr {
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(bytes, &arena, &DecodePolicy::service()).unwrap();
    let archive = cadmpeg_container::ArchiveSnapshot::new(&ctx, root).unwrap();
    let source = archive.open(&ctx, member).unwrap();
    super::oracle::expected(before, stream, source.window())
}

#[test]
fn first_middle_last_binary_carriers_cut_short_keep_only_the_proven_prefix_and_all_peers() {
    let fixtures = fixtures::all();
    let source = fixtures::archive(&fixtures);
    let before = decode(&source);
    let boundaries = assert_parts_agree(&fixtures, &source, &before);
    let levels = boundaries
        .iter()
        .map(|part| part.level.as_str())
        .collect::<BTreeSet<_>>();
    for level in levels {
        let candidates = boundaries
            .iter()
            .filter(|part| part.level == level)
            .collect::<Vec<_>>();
        for index in [0, candidates.len() / 2, candidates.len() - 1]
            .into_iter()
            .collect::<BTreeSet<_>>()
        {
            let selected = candidates[index];
            for cut in [selected.offset + 1, selected.offset + selected.length - 1]
                .into_iter()
                .collect::<BTreeSet<_>>()
            {
                let bytes = fixtures::archive_cut(
                    &fixtures,
                    Some((&selected.member, usize::try_from(cut).unwrap())),
                );
                let after = decode(&bytes);
                let failed = expectation(&fixtures, &bytes, selected);
                let failures = after
                    .source_fidelity()
                    .parts()
                    .values()
                    .filter(|part| part.state() == &PartState::GivenUp {})
                    .collect::<Vec<_>>();
                assert_eq!(
                    failures.len(),
                    1,
                    "{level} cut {cut}: {:#?}",
                    after.report().losses
                );
                let failure = failures[0];
                assert_eq!(failure.level().as_str(), failed.level, "{level} cut {cut}");
                assert_eq!(failure.offset(), failed.offset, "{level} cut {cut}");
                assert_eq!(failure.byte_len(), cut - failed.offset, "{level} cut {cut}");
                let expected = expected_ir(&before, &selected.stream, &bytes, &selected.member);
                let rows = |ir: &cadmpeg_ir::CadIr| {
                    ir.native
                        .namespace("fcstd")
                        .unwrap()
                        .arena_as::<crate::native::EntryRecord>("entries")
                        .unwrap()
                        .into_iter()
                        .map(|row| (row.id().to_owned(), row))
                        .collect::<std::collections::BTreeMap<_, _>>()
                };
                let expected_rows = rows(&expected);
                let actual_rows = rows(after.ir());
                assert_eq!(
                    expected_rows.keys().collect::<Vec<_>>(),
                    actual_rows.keys().collect::<Vec<_>>()
                );
                for (id, expected) in expected_rows {
                    assert_eq!(
                        expected, actual_rows[&id],
                        "exact proven prefix and independent member {id}: {level} cut {cut}"
                    );
                }
                let target = Boundary {
                    member: selected.member.clone(),
                    level: failed.level.clone(),
                    stream: member_stream(&bytes, &selected.member),
                    offset: failed.offset,
                    length: cut - failed.offset,
                    framing: failed
                        .framing
                        .iter()
                        .filter(|range| range.end <= cut)
                        .cloned()
                        .collect(),
                };
                assert_eq!(failure.framing_bytes(), target.framing, "{level} cut {cut}");
                damage::compare_expected(
                    &before,
                    &after,
                    &bytes,
                    &target,
                    &expected,
                    Some(&source),
                );
            }
        }
    }
}
