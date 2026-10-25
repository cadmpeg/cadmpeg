// SPDX-License-Identifier: Apache-2.0
//! The archive walk types feature-input lane scalars with their parameter
//! class, and a stopped walk is a reported loss.

use super::*;
use crate::records::ArchiveParameterClass;
use crate::test_support::native::sldprt_native;

const ARCHIVE_PARAMETER_SCALAR: &[u8] =
    include_bytes!("../../tests/golden/fixtures/archive_parameter_scalar.sldprt");
const ARCHIVE_PARAMETER_WALK_STOPPED: &[u8] =
    include_bytes!("../../tests/golden/fixtures/archive_parameter_walk_stopped.sldprt");

fn decode(bytes: &[u8]) -> cadmpeg_ir::codec::DecodeResult {
    SldprtCodec
        .decode(&mut Cursor::new(bytes.to_vec()), &DecodeOptions::default())
        .expect("the synthetic archive part decodes")
}

#[test]
fn a_reconciled_archive_walk_types_the_scalar_archive_class() {
    let result = decode(ARCHIVE_PARAMETER_SCALAR);
    let native = sldprt_native(result.ir());
    assert_eq!(native.feature_input_lanes.len(), 1);
    assert_eq!(native.feature_input_lanes[0].scalars.len(), 1);
    assert_eq!(
        native.feature_input_lanes[0].scalars[0].archive_class,
        Some(ArchiveParameterClass::Length)
    );
}

#[test]
fn a_stopped_archive_walk_is_a_reported_loss() {
    let result = decode(ARCHIVE_PARAMETER_WALK_STOPPED);
    let native = sldprt_native(result.ir());
    assert_eq!(native.feature_input_lanes[0].scalars[0].archive_class, None);
    assert!(result.report().losses.iter().any(|note| {
        note.code.namespace() == "sldprt"
            && note.code.local_code() == "feature.input-archive-walk-stopped"
    }));
}
