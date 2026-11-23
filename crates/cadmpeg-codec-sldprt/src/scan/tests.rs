// SPDX-License-Identifier: Apache-2.0
use super::*;
use cadmpeg_core::scan::ManualClock;

#[test]
fn scan_ends_without_error_in_a_partial_block_marker() {
    // The decoder reads a marker only when all six bytes are present.
    for prefix in 1..block::MARKER_VALUE.len() {
        let mut bytes = vec![0; outer::LEN];
        bytes.extend_from_slice(&block::MARKER_VALUE[..prefix]);
        let scan = crate::SldprtCodec::scan_design(
            View::over_retained(&bytes),
            ScanLimits::default(),
            &ManualClock::default(),
        );
        assert!(
            matches!(scan.stop_reason, Some(StopReason::Unsupported(_))),
            "{prefix}: {:?}",
            scan.stop_reason
        );
    }
}
