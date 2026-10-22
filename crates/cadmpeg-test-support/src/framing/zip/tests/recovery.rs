// SPDX-License-Identifier: Apache-2.0
use super::{archive, descriptor_archive, frame, put, source, word, RawEntry};
use crate::framing::zip::{scan, RecoveryReason, ZipScan};
use crate::framing::PartTable;

fn central(bytes: &[u8]) -> usize {
    usize::try_from(word(bytes, bytes.len() - 6)).expect("literal central offset")
}

fn two_members() -> (Vec<u8>, usize, usize) {
    let bytes = archive(&[
        RawEntry::stored("one", b"111"),
        RawEntry::stored("second", b"ABCDE"),
    ]);
    let second = 36;
    let payload = second + 36;
    (bytes, second, payload)
}

#[test]
fn complete_sources_use_actual_directory_and_missing_footer_preserves_proved_members() {
    let (bytes, _, _) = two_members();
    source(&bytes, |ctx, view| {
        assert!(
            matches!(scan(ctx, view, &mut Vec::new()).expect("complete source"), ZipScan::Complete { entries } if entries.len() == 2)
        );
    });
    for cut in [central(&bytes), bytes.len() - 3] {
        source(&bytes[..cut], |ctx, view| {
            let mut parts = Vec::new();
            let ZipScan::Local { entries, stop } =
                scan(ctx, view, &mut parts).expect("source-only recovered prefix")
            else {
                panic!("no complete end record exists");
            };
            assert_eq!(
                entries
                    .iter()
                    .map(|entry| entry.name().window())
                    .collect::<Vec<_>>(),
                [b"one".as_slice(), b"second"]
            );
            assert_eq!(
                entries
                    .iter()
                    .map(|entry| entry.payload().window())
                    .collect::<Vec<_>>(),
                [b"111".as_slice(), b"ABCDE"]
            );
            assert_eq!(
                stop.reason(),
                if cut == central(&bytes) {
                    RecoveryReason::SourceEnd
                } else {
                    RecoveryReason::FollowingStructure
                }
            );
            assert_eq!(stop.available().start(), central(&bytes));
            assert!(!parts.iter().any(|part| matches!(
                part.level().as_str(),
                "zip.central_directory"
                    | "zip.central_header"
                    | "zip.end_record"
                    | "zip.zip64_end_record"
            )));
            PartTable::load()
                .expect("table")
                .validate(ctx, &parts)
                .expect("only actual reached spans");
            assert!(ctx
                .take_record_captures()
                .expect("no semantic reads")
                .is_empty());
        });
    }
}

#[test]
fn cut_short_headers_and_payloads_retain_exact_incomplete_extents_and_prior_sibling() {
    let (bytes, second, payload) = two_members();
    for header_bytes in [4, 12, 29, 33] {
        let cut = second + header_bytes;
        source(&bytes[..cut], |ctx, view| {
            let mut parts = Vec::new();
            let ZipScan::Local { entries, stop } =
                scan(ctx, view, &mut parts).expect("actual partial header")
            else {
                panic!("no directory");
            };
            assert_eq!(entries.len(), 1);
            assert_eq!(entries[0].payload().window(), b"111");
            assert_eq!(stop.reason(), RecoveryReason::HeaderIncomplete);
            assert_eq!(stop.available().window(), &bytes[second..cut]);
            let incomplete = &parts[stop.frame_index().expect("actual incomplete header frame")];
            assert_eq!(incomplete.level().as_str(), "zip.local_header");
            assert_eq!(
                (incomplete.view().start(), incomplete.view().end()),
                (second, cut)
            );
            if header_bytes >= 30 {
                assert_eq!(
                    stop.declared_end(),
                    Some(u64::try_from(payload).expect("declared header end"))
                );
            }
        });
    }
    source(&bytes[..payload + 2], |ctx, view| {
        let mut parts = Vec::new();
        let ZipScan::Local { entries, stop } =
            scan(ctx, view, &mut parts).expect("actual partial payload")
        else {
            panic!("no directory");
        };
        assert_eq!(entries.len(), 1);
        assert_eq!(stop.reason(), RecoveryReason::PayloadIncomplete);
        assert_eq!(
            stop.declared_end(),
            Some(u64::try_from(payload + 5).expect("actual declared payload end"))
        );
        let incomplete = &parts[stop.frame_index().expect("actual incomplete entry")];
        assert_eq!(incomplete.level().as_str(), "zip.entry");
        assert_eq!(incomplete.view().window(), b"AB");
        assert_eq!(stop.available().window(), b"AB");
        assert!(
            u64::try_from(incomplete.view().end()).expect("available end")
                < stop.declared_end().expect("missing source extent")
        );
    });
}

#[test]
fn header_like_payload_bytes_never_become_siblings_and_prefix_search_is_not_recovery() {
    let fake = archive(&[RawEntry::stored("fake", b"not-a-member")]);
    let fake_prefix = &fake[..central(&fake)];
    let complete = archive(&[
        RawEntry::stored("first", fake_prefix),
        RawEntry::stored("later", b"real"),
    ]);
    let cut = central(&complete);
    source(&complete[..cut], |ctx, view| {
        let mut parts = Vec::new();
        let ZipScan::Local { entries, .. } =
            scan(ctx, view, &mut parts).expect("declared sizes advance over payload")
        else {
            panic!("missing directory");
        };
        assert_eq!(
            entries
                .iter()
                .map(|entry| entry.name().window())
                .collect::<Vec<_>>(),
            [b"first".as_slice(), b"later"]
        );
        assert_eq!(entries[0].payload().window(), fake_prefix);
        assert_eq!(
            parts
                .iter()
                .filter(|part| part.level().as_str() == "zip.local_header")
                .count(),
            2
        );
    });
    let mut prefixed = b"prefix".to_vec();
    prefixed.extend_from_slice(&complete[..cut]);
    source(&prefixed, |ctx, view| {
        assert!(scan(ctx, view, &mut Vec::new()).is_err())
    });
    let mut unknown = archive(&[
        RawEntry::stored("unknown", fake_prefix).with_flags(8),
        RawEntry::stored("later", b"real"),
    ]);
    let cut = central(&unknown);
    put(&mut unknown, 18, 0);
    put(&mut unknown, 22, 0);
    source(&unknown[..cut], |ctx, view| {
        let mut parts = Vec::new();
        let ZipScan::Local { entries, stop } =
            scan(ctx, view, &mut parts).expect("honest unknown-size stop")
        else {
            panic!("missing directory");
        };
        assert!(entries.is_empty());
        assert_eq!(stop.reason(), RecoveryReason::SizeUnavailable);
        assert!(stop.frame_index().is_none());
        assert_eq!(
            parts
                .iter()
                .filter(|part| part.level().as_str() == "zip.local_header")
                .count(),
            1
        );
        assert!(!parts
            .iter()
            .any(|part| part.level().as_str() == "zip.entry"));
    });
}

#[test]
fn known_size_stored_descriptors_require_actual_checksum_and_exact_post_payload_position() {
    for signed in [false, true] {
        let (mut bytes, _, descriptor) = descriptor_archive(signed);
        let cut = central(&bytes);
        put(&mut bytes, 18, 5);
        put(&mut bytes, 22, 5);
        source(&bytes[..cut], |ctx, view| {
            let mut parts = Vec::new();
            let ZipScan::Local { entries, stop } =
                scan(ctx, view, &mut parts).expect("proved stored descriptor")
            else {
                panic!("no directory");
            };
            assert_eq!(entries.len(), 1);
            assert_eq!(entries[0].payload().window(), b"ABCDE");
            assert_eq!(stop.reason(), RecoveryReason::SourceEnd);
            let actual = parts
                .iter()
                .find(|part| part.level().as_str() == "zip.data_descriptor")
                .expect("descriptor at proved payload end");
            assert_eq!(
                (actual.view().start(), actual.view().end()),
                (descriptor, cut)
            );
        });
        let crc_at = descriptor + if signed { 4 } else { 0 };
        put(&mut bytes, crc_at, 0x1234_5678);
        source(&bytes[..cut], |ctx, view| {
            let mut parts = Vec::new();
            let ZipScan::Local { entries, stop } =
                scan(ctx, view, &mut parts).expect("checksum disagreement stops recovery")
            else {
                panic!("no directory");
            };
            assert!(entries.is_empty());
            assert_eq!(stop.reason(), RecoveryReason::DescriptorUnavailable);
            assert_eq!(stop.available().start(), descriptor);
            assert!(!parts
                .iter()
                .any(|part| part.level().as_str() == "zip.data_descriptor"));
        });
    }
    let (mut bytes, _, descriptor) = descriptor_archive(true);
    put(&mut bytes, 18, 5);
    put(&mut bytes, 22, 5);
    source(&bytes[..descriptor + 8], |ctx, view| {
        let ZipScan::Local { entries, stop } =
            scan(ctx, view, &mut Vec::new()).expect("actual partial descriptor")
        else {
            panic!("no directory");
        };
        assert!(entries.is_empty());
        assert_eq!(stop.reason(), RecoveryReason::DescriptorUnavailable);
    });
}

#[test]
fn actual_local_zip64_sizes_are_used_without_clamping_huge_sizes_or_fabricating_a_footer() {
    let complete = archive(&[RawEntry::stored("wide", b"ABCDE")]);
    let mut bytes = complete[..central(&complete)].to_vec();
    let mut extra = vec![1, 0, 16, 0];
    extra.extend_from_slice(&5_u64.to_le_bytes());
    extra.extend_from_slice(&5_u64.to_le_bytes());
    bytes.splice(34..34, extra);
    bytes[4..6].copy_from_slice(&45_u16.to_le_bytes());
    bytes[28..30].copy_from_slice(&20_u16.to_le_bytes());
    put(&mut bytes, 18, u32::MAX);
    put(&mut bytes, 22, u32::MAX);
    source(&bytes, |ctx, view| {
        let mut parts = Vec::new();
        let ZipScan::Local { entries, stop } =
            scan(ctx, view, &mut parts).expect("actual local64 sizes")
        else {
            panic!("no footer");
        };
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].payload().window(), b"ABCDE");
        assert_eq!(stop.reason(), RecoveryReason::SourceEnd);
        assert!(!parts
            .iter()
            .any(|part| part.level().as_str() == "zip.zip64_end_record"));
    });
    bytes[46..54].copy_from_slice(&u64::MAX.to_le_bytes());
    source(&bytes, |ctx, view| {
        let mut parts = Vec::new();
        let ZipScan::Local { entries, stop } =
            scan(ctx, view, &mut parts).expect("huge source length is an explicit incomplete part")
        else {
            panic!("no footer");
        };
        assert!(entries.is_empty());
        assert_eq!(stop.reason(), RecoveryReason::PayloadIncomplete);
        assert!(stop.declared_end().is_none());
        assert_eq!(
            parts[stop.frame_index().expect("available entry prefix")]
                .view()
                .window(),
            b"ABCDE"
        );
    });
}

#[test]
fn existing_complete_refusals_stay_strict_and_local_recovery_keeps_budget_errors_fatal() {
    let (bytes, _, _) = two_members();
    let truncated = &bytes[..central(&bytes)];
    source(truncated, |ctx, view| {
        assert!(frame(ctx, view, &mut Vec::new()).is_err());
        assert!(
            matches!(scan(ctx, view, &mut Vec::new()).expect("explicit recovering entry point"), ZipScan::Local { entries, .. } if entries.len() == 2)
        );
    });
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_retained_bytes = 1;
    let (ctx, view) =
        cadmpeg_core::decode::DecodeContext::from_root_bytes(truncated, &arena, &policy)
            .expect("root input budget");
    assert!(scan(&ctx, view, &mut Vec::new()).is_err());
    assert!(
        matches!(ctx.resource_refusal(), Some(limit) if limit.scope == cadmpeg_core::decode::LimitScope::Session)
    );
}
