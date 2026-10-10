// SPDX-License-Identifier: Apache-2.0
//! Retained storage of parameter frames that the decode does not keep.

use std::io::{Cursor, Write};

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use zip::CompressionMethod;

use super::super::decode_parameters;
use crate::design::test_support::parameter_record;
use crate::test_support::manifest_test::write_synthetic_manifests;
use crate::test_support::zip_test::with_scan;

/// A Design archive whose one `BulkStream` holds `frames` in order.
fn archive(frames: &[&[u8]]) -> Vec<u8> {
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let stored = crate::zip_write::file_options(CompressionMethod::Stored);
    write_synthetic_manifests(&mut zip, stored);
    zip.start_file("FusionAssetName[Active]/Design1/BulkStream.dat", stored)
        .unwrap();
    for frame in frames {
        zip.write_all(frame).unwrap();
    }
    zip.finish().unwrap().into_inner()
}

/// The parameter count `archive` decodes to under a retained-byte ceiling,
/// or `None` when the decode refuses.
fn decoded_within(archive: &[u8], retained: u64) -> Option<usize> {
    with_scan(archive, |scan| {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = retained;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        decode_parameters(&ctx, scan)
            .ok()
            .map(|parameters| parameters.len())
    })
}

#[test]
fn stale_parameter_frame_retains_nothing() {
    let frame = parameter_record(Some(44), "1", "AlongDistance", Some("mm"), "d71", 1.0);
    let single = archive(&[&frame]);
    // A later frame with the same record index is a stale copy; the decode
    // keeps the first frame only.
    let doubled = archive(&[&frame, &frame]);
    let (mut low, mut high) = (0, 1 << 16);
    assert_eq!(decoded_within(&single, high), Some(1));
    while low < high {
        let middle = low + (high - low) / 2;
        if decoded_within(&single, middle).is_some() {
            high = middle;
        } else {
            low = middle + 1;
        }
    }
    assert_eq!(decoded_within(&single, low - 1), None);
    assert_eq!(decoded_within(&doubled, low), Some(1));
}
