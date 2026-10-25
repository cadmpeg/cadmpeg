// SPDX-License-Identifier: Apache-2.0
//! Every listed BRep level receives genuine available-prefix EOF controls.
use super::*;
use cadmpeg_ir::source_fidelity::parts::PartState;

fn cut_short_carrier(binary: bool) {
    let mut member = fixtures::member(binary);
    if !binary {
        // An absent optional root is valid. Use an actual non-null root here
        // so its midpoint is a genuine incomplete root reference.
        assert_eq!(member.pop(), Some(b'*'));
        member.extend_from_slice(b"+1 0 *");
    }
    let original = fixtures::archive(binary, &member);
    let before = decode(&original);
    let boundaries = assert_parts_agree(&original, &before);
    let levels = boundaries
        .iter()
        .map(|part| part.level.as_str())
        .collect::<BTreeSet<_>>();
    for level in levels {
        let candidates = boundaries
            .iter()
            .filter(|part| part.level == level)
            .collect::<Vec<_>>();
        for index in recovery::choices(&candidates) {
            let target = candidates[index];
            let cut =
                usize::try_from(target.offset - target.member_offset + target.length / 2).unwrap();
            assert!(cut < member.len());
            // This authors an actually shorter ZIP member. The same-length
            // metadata-preserving damage rebuilder intentionally cannot do it.
            let bytes = fixtures::archive(binary, &member[..cut]);
            let after = decode(&bytes);
            let damaged = assert_parts_agree(&bytes, &after);
            let current_member = damaged
                .iter()
                .find(|part| part.member == target.member)
                .unwrap();
            let end = current_member.member_offset + u64::try_from(cut).unwrap();
            let failures = after
                .source_fidelity()
                .parts()
                .values()
                .filter(|part| matches!(part.state(), PartState::GivenUp {}))
                .collect::<Vec<_>>();
            assert!(!failures.is_empty(), "cut-short {target:?} at {cut}");
            for failure in &failures {
                assert_eq!(failure.stream(), current_member.stream, "{target:?}");
                assert!(
                    failure.offset() + failure.byte_len() <= end,
                    "{target:?}: {failure:?}"
                );
                assert!(
                    damaged
                        .iter()
                        .any(|part| part.level == failure.level().as_str()
                            && part.offset == failure.offset()
                            && part.length == failure.byte_len()),
                    "failure must have an independent available-source frame: {failure:?}"
                );
            }
            eprintln!("BRep cut {target:?} at {cut}; actual failures {failures:?}");
            let streams = recovery::stream_pairs(&boundaries, &damaged);
            compare_damage(&original, &before, &after, &bytes, &streams, None);
            crate::fidelity::b1::assert_classified_spans(&after);
            assert!(
                crate::test_support::validate_native(after.ir()).is_empty(),
                "{target:?}"
            );
        }
    }
}

#[test]
fn every_text_brep_level_has_closed_cut_short_controls() {
    cut_short_carrier(false);
}

#[test]
fn every_binary_brep_level_has_closed_cut_short_controls() {
    cut_short_carrier(true);
}
