// SPDX-License-Identifier: Apache-2.0
use super::values;

fn payload(minor: u32, filetime: u64) -> Vec<u8> {
    let mut bytes = b"moConfigurationMgr_c".to_vec();
    let mut body = [0; 125];
    body[66..70].copy_from_slice(&minor.to_le_bytes());
    body[107] = 7;
    body[117..125].copy_from_slice(&filetime.to_le_bytes());
    bytes.extend(body);
    bytes
}

#[test]
fn minor_version_and_utc_filetime_bounds_preserve_raw_values() {
    for (minor, time, accepted) in [
        (0, 124_333_920_000_000_000, true),
        (65_535, 157_469_183_999_999_999, true),
        (65_536, 132_537_600_000_000_000, false),
        (0, 124_333_919_999_999_999, false),
        (0, 157_469_184_000_000_000, false),
        (0, u64::MAX, false),
    ] {
        assert_eq!(
            values(&payload(minor, time), 0),
            accepted.then(|| [i64::from(minor), 7, i64::try_from(time).unwrap()]),
        );
    }
}

#[test]
fn complete_prefix_and_exact_token_are_required_at_the_stored_offset() {
    let bytes = payload(17, 132_537_600_000_000_000);
    for end in 0..bytes.len() {
        assert_eq!(values(&bytes[..end], 0), None);
    }
    let mut embedded = vec![0x35; 9];
    embedded.extend(&bytes);
    assert_eq!(values(&embedded, 9), Some([17, 7, 132_537_600_000_000_000]));
    assert_eq!(values(&embedded, 8), None);
    assert_eq!(values(&embedded, usize::MAX), None);
}
