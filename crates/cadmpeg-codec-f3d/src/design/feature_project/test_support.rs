// SPDX-License-Identifier: Apache-2.0
//! Shared feature projection refusal assertions.

use cadmpeg_core::decode::{DecodeContext, ResourceDimension};
use cadmpeg_core::CodecError;

pub(super) fn assert_original_refusal<T>(
    operation: &'static str,
    project: impl Fn(&DecodeContext<'_>) -> Result<T, CodecError>,
) {
    let error = crate::test_support::resource_refusal_at(
        ResourceDimension::WorkUnits,
        operation,
        0,
        project,
    );
    assert!(
        matches!(error, CodecError::ResourceLimit(limit) if limit.operation == operation && limit.dimension == ResourceDimension::WorkUnits)
    );
}
