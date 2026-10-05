// SPDX-License-Identifier: Apache-2.0
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;

pub fn precharged_and_fixed_chain_orders(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
) -> Result<(), CodecError> {
    let admitted = ctx.admit_iter(bytes, "fixed chain source")?;
    let first = 0_u8;
    for byte in std::iter::once(&first).chain(admitted) {
        std::hint::black_box(byte);
    }

    let admitted = ctx.admit_iter(bytes, "fixed chain source")?;
    let second = 1_u8;
    for byte in admitted.chain(std::iter::once(&second)) {
        std::hint::black_box(byte);
    }
    Ok(())
}

pub fn fully_precharged_dynamic_chain(
    ctx: &DecodeContext<'_>,
    left: &[u8],
    right: &[u8],
) -> Result<(), CodecError> {
    let left = ctx.admit_iter(left, "left chain source")?;
    let right = ctx.admit_iter(right, "right chain source")?;
    for byte in left.chain(right) {
        std::hint::black_box(byte);
    }
    Ok(())
}

pub fn mixed_precharged_then_raw_chain(
    ctx: &DecodeContext<'_>,
    admitted: &[u8],
    raw: &[u8],
) -> Result<(), CodecError> {
    let admitted = ctx.admit_iter(admitted, "precharged chain source")?;
    for byte in admitted.chain(raw.iter()) { // finding: uncharged_decode_work
        std::hint::black_box(byte);
    }
    Ok(())
}

pub fn mixed_raw_then_precharged_chain(
    ctx: &DecodeContext<'_>,
    raw: &[u8],
    admitted: &[u8],
) -> Result<(), CodecError> {
    let admitted = ctx.admit_iter(admitted, "precharged chain source")?;
    for byte in raw.iter().chain(admitted) { // finding: uncharged_decode_work
        std::hint::black_box(byte);
    }
    Ok(())
}

pub fn owned_string_source(
    ctx: &DecodeContext<'_>,
    values: Vec<String>,
) -> Result<(), CodecError> {
    for value in ctx.admit_iter(values, "owned string source")? {
        let _length = value.len();
    }
    Ok(())
}

pub fn discarded_admission_refusal(ctx: &DecodeContext<'_>, bytes: &[u8]) {
    if let Ok(values) = ctx.admit_iter(bytes, "discarded refusal") { // finding: unproven_decode_charge
        for byte in values {
            std::hint::black_box(byte);
        }
    }
}

struct ScanningSource<'a>(&'a [u8]);

impl<'a> IntoIterator for ScanningSource<'a> {
    type Item = Result<u8, CodecError>;
    type IntoIter = std::vec::IntoIter<Self::Item>;

    fn into_iter(self) -> Self::IntoIter {
        let mut values = Vec::new();
        for value in self.0 {
            // finding: uncharged_decode_work
            values.push(Ok(*value)); // finding: uncharged_decode_allocation, uncharged_decode_work
        }
        values.into_iter()
    }
}

pub fn custom_into_iter_constructor(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
) -> Result<(), CodecError> {
    let _values = ctx.try_collect_scoped_vec(ScanningSource(bytes), "custom constructor")?; // finding: unproven_decode_charge
    Ok(())
}

pub fn standard_into_iter_constructor(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
) -> Result<(), CodecError> {
    let values = bytes.iter().copied().map(Ok::<u8, CodecError>);
    let _values = ctx.try_collect_scoped_vec(values, "standard constructor")?;
    Ok(())
}

pub fn prepaid_filter_flat_map_fold(
    ctx: &DecodeContext<'_>,
    name: &str,
) -> Result<(), CodecError> {
    let chars = ctx.admit_iter(name, "normalize source")?;
    let _length = chars
        .filter(|character| character.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .try_fold(0_usize, |length, character| {
            length.checked_add(character.len_utf8())
        })
        .ok_or_else(|| CodecError::Malformed("normalized length overflow".into()))?;
    Ok(())
}

pub fn prepaid_flat_map_with_dynamic_inner(
    ctx: &DecodeContext<'_>,
    name: &str,
    suffix: &str,
) -> Result<(), CodecError> {
    let chars = ctx.admit_iter(name, "suffixed source")?;
    let _length = chars.flat_map(|_| suffix.chars()).count(); // finding: unproven_decode_charge
    Ok(())
}

pub fn prepaid_records_zip_wires(
    ctx: &DecodeContext<'_>,
    records: &[u8],
    wires: &[u8],
) -> Result<(), CodecError> {
    let records = ctx.admit_iter(records, "record source")?;
    for (record, wire) in records.zip(wires.iter()) {
        if record == wire {
            continue;
        }
    }
    Ok(())
}

pub fn raw_wires_zip_prepaid_records(
    ctx: &DecodeContext<'_>,
    wires: &[u8],
    records: &[u8],
) -> Result<(), CodecError> {
    let records = ctx.admit_iter(records, "record source")?;
    for (wire, record) in wires.iter().zip(records) {
        if wire == record {
            continue;
        }
    }
    Ok(())
}

pub fn unadmitted_records_zip_wires(
    _ctx: &DecodeContext<'_>,
    records: &[u8],
    wires: &[u8],
) -> Result<(), CodecError> {
    for (record, wire) in records.iter().zip(wires.iter()) { // finding: uncharged_decode_work
        if record == wire {
            continue;
        }
    }
    Ok(())
}

pub fn reversed_zip_source(
    ctx: &DecodeContext<'_>,
    wires: &[u8],
    records: &[u8],
) -> Result<(), CodecError> {
    let records = ctx.admit_iter(records, "record source")?;
    for (wire, record) in wires.iter().filter(|wire| **wire != 0).zip(records) { // finding: uncharged_decode_work
        if wire == record {
            continue;
        }
    }
    Ok(())
}

pub fn prepaid_nested_fallible_producer(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
) -> Result<(), CodecError> {
    let source = ctx.admit_iter(bytes, "nested producer source")?;
    let produced = source.map(|byte| {
        Ok::<_, CodecError>(Ok::<u8, CodecError>(*byte))
    });
    let _values = ctx.try_collect_vec(produced, "nested fallible producer")?;
    Ok(())
}

pub fn prepaid_by_ref_search(ctx: &DecodeContext<'_>, bytes: &[u8]) -> Result<bool, CodecError> {
    let mut admitted = ctx.admit_iter(bytes, "by-ref search")?;
    let found = admitted.by_ref().any(|byte| *byte == 0); // finding: unproven_decode_charge
    let _remaining = admitted.count();
    Ok(found)
}

pub fn raw_by_ref_search(_ctx: &DecodeContext<'_>, bytes: &[u8]) -> bool {
    let mut raw = bytes.iter();
    raw.by_ref().any(|byte| *byte == 0) // finding: uncharged_decode_work
}

pub fn prepaid_consumer_with_unchecked_callback(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    predicate: fn(&u8) -> bool,
) -> Result<bool, CodecError> {
    let mut admitted = ctx.admit_iter(bytes, "pointer predicate")?;
    Ok(admitted.any(predicate)) // finding: unproven_decode_charge
}

pub fn prepaid_copy_clones(ctx: &DecodeContext<'_>, bytes: &[u8]) -> Result<usize, CodecError> {
    Ok(ctx.admit_iter(bytes, "copied clones")?.cloned().count())
}

pub fn prepaid_owned_clones(ctx: &DecodeContext<'_>, names: &[String]) -> Result<usize, CodecError> {
    Ok(ctx.admit_iter(names, "string clones")?.cloned().count()) // finding: unproven_decode_charge, unproven_decode_charge
}

pub fn charged_steps_over_prepaid_filter(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
) -> Result<(), CodecError> {
    let mut nonzero = ctx.admit_iter(bytes, "filtered steps")?.filter(|byte| **byte != 0);
    while let Some(byte) = ctx.next_charged(&mut nonzero, "filtered step")? {
        std::hint::black_box(byte);
    }
    Ok(())
}

pub fn charged_steps_over_raw_filter(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
) -> Result<(), CodecError> {
    let mut nonzero = bytes.iter().filter(|byte| **byte != 0);
    while let Some(byte) = ctx.next_charged(&mut nonzero, "raw filtered step")? { // finding: unproven_decode_charge
        std::hint::black_box(byte);
    }
    Ok(())
}

pub fn charged_steps_over_from_fn(ctx: &DecodeContext<'_>, bytes: &[u8]) -> Result<(), CodecError> {
    let mut index = 0_usize;
    let mut steps = std::iter::from_fn(|| {
        let byte = bytes.get(index).copied();
        index += 1;
        byte
    });
    while let Some(byte) = ctx.next_charged(&mut steps, "from_fn step")? {
        std::hint::black_box(byte);
    }
    Ok(())
}

pub fn charged_steps_over_opaque_from_fn(
    ctx: &DecodeContext<'_>,
    step: fn() -> Option<u8>,
) -> Result<(), CodecError> {
    let mut steps = std::iter::from_fn(step);
    while let Some(byte) = ctx.next_charged(&mut steps, "opaque step")? { // finding: unproven_decode_charge
        std::hint::black_box(byte);
    }
    Ok(())
}

pub fn fixed_count_outer_flatten(_ctx: &DecodeContext<'_>, left: &[u8], right: &[u8]) {
    for byte in [left, right].into_iter().flatten() { // finding: unproven_decode_charge
        std::hint::black_box(byte);
    }
}

pub fn charged_search_over_tree(
    ctx: &DecodeContext<'_>,
    values: &std::collections::BTreeSet<u32>,
) -> Result<bool, CodecError> {
    ctx.any_by(values, |value| Ok(*value == 3), "tree search")
}
