// SPDX-License-Identifier: Apache-2.0
mod contains;
mod class_296;
mod coil;
mod extent;

fn assert_work_refusal<T>(
    operation: &'static str,
    decode: impl Fn(&cadmpeg_core::decode::DecodeContext<'_>) -> Result<T, cadmpeg_core::CodecError>,
) {
    let refusal = crate::test_support::resource_refusal_at(
        cadmpeg_core::decode::ResourceDimension::WorkUnits,
        operation,
        0,
        decode,
    );
    assert!(matches!(
        refusal,
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == cadmpeg_core::decode::ResourceDimension::WorkUnits
                && limit.operation == operation
                && limit.additional == 1
    ));
}
