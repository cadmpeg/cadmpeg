// SPDX-License-Identifier: Apache-2.0
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use std::collections::BTreeMap;

pub fn owned_sources(
    ctx: &DecodeContext<'_>,
    values: Vec<u8>,
    optional: Option<u8>,
    array: [u8; 3],
) -> Result<(), CodecError> {
    for value in ctx.admit_iter(values, "owned vector")? {
        let _value = value;
    }
    for value in ctx.admit_iter(optional, "owned option")? {
        let _value = value;
    }
    for value in ctx.admit_iter(array, "owned array")? {
        let _value = value;
    }
    Ok(())
}

pub fn mutable_sources(
    ctx: &DecodeContext<'_>,
    values: &mut Vec<u8>,
    slice: &mut [u8],
    array: &mut [u8; 3],
) -> Result<(), CodecError> {
    for value in ctx.admit_iter(values, "mutable vector")? {
        *value = 1;
    }
    for value in ctx.admit_iter(slice, "mutable slice")? {
        *value = 2;
    }
    for value in ctx.admit_iter(array, "mutable array")? {
        *value = 3;
    }
    Ok(())
}

pub fn unadmitted_mutable_source(_ctx: &DecodeContext<'_>, values: &mut Vec<u8>) {
    for value in values.iter_mut() { // finding: uncharged_decode_work
        *value = 1;
    }
}

pub fn mutable_map_values(
    ctx: &DecodeContext<'_>,
    values: &mut BTreeMap<u8, u8>,
) -> Result<(), CodecError> {
    for (_key, value) in ctx.admit_iter(values, "mutable map")? {
        *value = 1;
    }
    Ok(())
}

pub fn unadmitted_mutable_map_values(_ctx: &DecodeContext<'_>, values: &mut BTreeMap<u8, u8>) {
    for value in values.values_mut() { // finding: uncharged_decode_work
        *value = 1;
    }
}
