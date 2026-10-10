// SPDX-License-Identifier: Apache-2.0

use super::*;

#[test]
fn compressed_global_digit_runs_charge_each_copy_route() {
    // A numeric field can carry its digits through multiple cards.
    for suffix in [b'H', b',', b' '] {
        let mut first = [b' '; 80];
        first[..72].fill(b'0');
        let mut last = [b' '; 80];
        last[0] = b'0';
        last[1] = suffix;
        let cards = [&first[..], &last[..]];
        let error = cadmpeg_test_support::refusal::resource_limit_at(
            ResourceDimension::WorkUnits,
            "iges compressed Global digit copy",
            |cap| {
                with_work_limit(&[], cap, |ctx| {
                    logical_global_stream(&cards, ctx).map(|_| ())
                })
            },
        );
        assert_work_limit(&error, "iges compressed Global digit copy", 73);
        with_work_limit(&[], u64::MAX, |ctx| {
            let stream = logical_global_stream(&cards, ctx).unwrap().0;
            let mut expected = vec![b'0'; 73];
            if suffix != b' ' {
                expected.push(suffix);
            }
            assert_eq!(stream, expected);
        });
    }
}

#[test]
fn compressed_trailing_records_charge_only_copied_bytes() {
    let mut source = compressed_points_file();
    let expected = normalize_for_test(&source).unwrap();
    source.extend_from_slice(b"trailing record\n");
    let error = normalize_work_refusal(&source, "iges compressed trailing record copy");
    assert_work_limit(&error, "iges compressed trailing record copy", 15);
    let mut expected = expected;
    expected.extend_from_slice(b"trailing record\n");
    assert_eq!(normalize_for_test(&source).unwrap(), expected);
}

#[test]
fn compressed_generated_card_work_excludes_uncopied_source() {
    use cadmpeg_core::decode::refusal_probe::RefusalProbe;
    let mut source = compressed_points_file();
    for trailing in [false, true] {
        if trailing {
            source.extend_from_slice(b"trailing record\n");
        }
        let output = normalize_for_test(&source).unwrap();
        let generated_bytes = output.len() - if trailing { 16 } else { 0 };
        let refusal = cadmpeg_test_support::refusal::resource_limit_at(
            ResourceDimension::WorkUnits,
            "iges_compressed_ascii_normalization",
            |cap| {
                let _probe = (cap == u64::MAX).then(|| {
                    RefusalProbe::arm(
                        ResourceDimension::WorkUnits,
                        "iges_compressed_ascii_normalization",
                        Some(u64::try_from(generated_bytes).unwrap()),
                    )
                });
                with_work_limit(&source, cap, |ctx| normalize(&source, ctx).map(|_| ()))
            },
        );
        assert_work_limit(
            &refusal,
            "iges_compressed_ascii_normalization",
            u64::try_from(generated_bytes).unwrap(),
        );
    }
}

#[test]
fn compressed_trailing_record_visits_are_charged_individually() {
    let mut source = compressed_points_file();
    let mut expected = normalize_for_test(&source).unwrap();
    let trailing = b"trailing record\nsecond record\n";
    source.extend_from_slice(trailing);
    expected.extend_from_slice(trailing);
    let error = normalize_work_refusal(&source, "iges compressed trailing records");
    assert_work_limit(&error, "iges compressed trailing records", 1);
    assert_eq!(normalize_for_test(&source).unwrap(), expected);
}
