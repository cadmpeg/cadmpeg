// SPDX-License-Identifier: Apache-2.0
use cadmpeg_core::decode::iter_source::IncrementalSource;
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;

pub fn incremental_slice(ctx: &DecodeContext<'_>, bytes: &[u8]) -> Result<(), CodecError> {
    for byte in ctx.admit_iter(IncrementalSource::new(bytes.iter()), "slice")? {
        let byte = byte?;
        if *byte == 0 {
            continue;
        }
    }
    Ok(())
}

pub fn incremental_fixed_chain(ctx: &DecodeContext<'_>, bytes: &[u8]) -> Result<(), CodecError> {
    for byte in ctx
        .admit_iter(
            IncrementalSource::new(std::iter::once(0_u8).chain(bytes.iter().copied())),
            "chain",
        )?
    {
        let byte = byte?;
        if byte == 0 {
            continue;
        }
    }
    Ok(())
}

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

pub fn incremental_filter(ctx: &DecodeContext<'_>, bytes: &[u8]) -> Result<(), CodecError> {
    let source = IncrementalSource::new(bytes.iter().filter(|byte| **byte != 0)); // finding: unproven_decode_charge
    let values = ctx.admit_iter(source, "filter")?; // finding: unproven_decode_charge
    for byte in values {
        let byte = byte?;
        if *byte == 0 {
            continue;
        }
    }
    Ok(())
}

pub fn incremental_zip(ctx: &DecodeContext<'_>, bytes: &[u8]) -> Result<(), CodecError> {
    let source = IncrementalSource::new(bytes.iter().copied().zip(bytes.iter().copied())); // finding: unproven_decode_charge
    let values = ctx.admit_iter(source, "zip")?; // finding: unproven_decode_charge
    for pair in values {
        let pair = pair?;
        if pair.0 == pair.1 {
            continue;
        }
    }
    Ok(())
}

pub fn incremental_count(ctx: &DecodeContext<'_>, bytes: &[u8]) -> Result<(), CodecError> {
    let values = ctx.admit_iter(IncrementalSource::new(bytes.iter()), "count")?;
    let _count = values.count(); // finding: unproven_decode_charge
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

struct ScanningDrop(String);

impl Drop for ScanningDrop {
    fn drop(&mut self) {
        for byte in self.0.bytes() {
            std::hint::black_box(byte);
        }
    }
}

pub fn owned_source_with_custom_drop(
    ctx: &DecodeContext<'_>,
    values: Vec<ScanningDrop>,
) -> Result<(), CodecError> {
    for value in ctx.admit_iter(values, "owned custom-drop source")? { // finding: unproven_decode_charge
        if value.0.is_empty() {
            break;
        }
    }
    Ok(())
}

pub fn stepwise_source_with_custom_callback_drop(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    capture: ScanningDrop,
) -> Result<(), CodecError> {
    let source = IncrementalSource::new(bytes.iter().map(move |byte| {
        let _captured = std::hint::black_box(&capture).0.len();
        Ok::<_, CodecError>(*byte)
    }));
    for value in ctx.admit_iter(source, "custom callback drop")? { // finding: unproven_decode_charge
        if value?? == 0 {
            break;
        }
    }
    Ok(())
}

pub fn stepwise_source_with_string_callback_capture(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    capture: String,
) -> Result<(), CodecError> {
    let source = IncrementalSource::new(bytes.iter().map(move |byte| {
        let _length = capture.len();
        Ok::<_, CodecError>(*byte)
    }));
    for value in ctx.admit_iter(source, "string callback capture")? {
        let _value = value?;
        break;
    }
    Ok(())
}

pub fn incremental_result_ok(ctx: &DecodeContext<'_>, bytes: &[u8]) -> Result<(), CodecError> {
    let values = ctx.admit_iter(IncrementalSource::new(bytes.iter()), "result adapter")?;
    for value in values.map(Result::ok).flatten() { // finding: unproven_decode_charge
        if *value == 0 {
            continue;
        }
    }
    Ok(())
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

pub fn incremental_filter_map(ctx: &DecodeContext<'_>, bytes: &[u8]) -> Result<(), CodecError> {
    let filtered = bytes
        .iter()
        .filter_map(|byte| (*byte != 0).then_some(byte));
    let source = IncrementalSource::new(filtered); // finding: unproven_decode_charge
    let values = ctx.admit_iter(source, "filter_map")?; // finding: unproven_decode_charge
    for byte in values {
        let byte = byte?;
        if *byte == 0 {
            continue;
        }
    }
    Ok(())
}

pub fn incremental_flat_map(ctx: &DecodeContext<'_>, bytes: &[u8]) -> Result<(), CodecError> {
    let expanded = bytes
        .iter()
        .flat_map(|byte| std::iter::repeat(*byte));
    let source = IncrementalSource::new(expanded); // finding: unproven_decode_charge
    let values = ctx.admit_iter(source, "flat_map")?; // finding: unproven_decode_charge
    for byte in values {
        let byte = byte?;
        if byte == 0 {
            continue;
        }
    }
    Ok(())
}

pub fn incremental_try_fold(ctx: &DecodeContext<'_>, bytes: &[u8]) -> Result<(), CodecError> {
    let mut values = ctx.admit_iter(IncrementalSource::new(bytes.iter()), "fallible fold")?;
    let _count = values.try_fold(0_usize, |count, _result| Some(count + 1)); // finding: unproven_decode_charge
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
    for (wire, record) in wires.iter().filter(|wire| **wire != 0).zip(records) { // finding: unproven_decode_charge
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

pub fn incremental_nested_result_condition(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
) -> Result<(), CodecError> {
    let source = IncrementalSource::new(bytes.iter().map(|byte| Ok::<_, CodecError>(*byte)));
    for value in ctx.admit_iter(source, "nested result condition")? {
        if value?? == 0 {
            break;
        }
    }
    Ok(())
}

pub fn incremental_short_circuit_does_not_propagate_refusal(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    condition: bool,
) -> Result<(), CodecError> {
    for value in ctx.admit_iter(IncrementalSource::new(bytes.iter()), "conditional refusal")? { // finding: unproven_decode_charge
        if condition && *value? == 0 {
            break;
        }
    }
    Ok(())
}
