// SPDX-License-Identifier: Apache-2.0
pub struct DecodeContext;
impl DecodeContext {
    pub fn charge_work(&self, _units: u64, _operation: &str) -> Result<(), ()> {
        Ok(())
    }
    pub fn retained_string(&self, _capacity: usize, _operation: &str) -> Result<String, ()> {
        Ok(String::new())
    }
}

pub fn exact_input_charge(
    ctx: &DecodeContext,
    decoder: &mut encoding_rs::Decoder,
    source: &[u8],
    output: &mut String,
) -> Result<(), ()> {
    ctx.charge_work(u64::try_from(source.len()).map_err(|_| ())?, "encoded source")?;
    let _decoded = decoder.decode_to_string_without_replacement(source, output, true);
    Ok(())
}

pub fn missing_input_charge(
    _ctx: &DecodeContext,
    decoder: &mut encoding_rs::Decoder,
    source: &[u8],
    output: &mut String,
) {
    let _decoded = decoder.decode_to_string_without_replacement(source, output, true); // finding: uncharged_decode_work
}

pub fn wrong_input_charge(
    ctx: &DecodeContext,
    decoder: &mut encoding_rs::Decoder,
    source: &[u8],
    other: &[u8],
    output: &mut String,
) -> Result<(), ()> {
    ctx.charge_work(u64::try_from(other.len()).map_err(|_| ())?, "other source")?;
    let _decoded = decoder.decode_to_string_without_replacement(source, output, true); // finding: uncharged_decode_work
    Ok(())
}

pub fn unsupported_decoder_operation(
    _ctx: &DecodeContext,
    decoder: &mut encoding_rs::Decoder,
    source: &[u8],
    output: &mut str,
) {
    let _decoded = decoder.decode_to_str_without_replacement(source, output, true); // finding: unproven_decode_charge
}

pub fn admitted_output_capacity(
    ctx: &DecodeContext,
    decoder: &mut encoding_rs::Decoder,
    source: &[u8],
    output_capacity: usize,
) -> Result<(), ()> {
    ctx.charge_work(u64::try_from(source.len()).map_err(|_| ())?, "encoded source")?;
    let mut output = ctx.retained_string(output_capacity, "decoded output")?;
    let _decoded = decoder.decode_to_string_without_replacement(source, &mut output, true);
    Ok(())
}

pub fn missing_output_capacity(
    ctx: &DecodeContext,
    decoder: &mut encoding_rs::Decoder,
    source: &[u8],
    output_capacity: usize,
) -> Result<(), ()> {
    ctx.charge_work(u64::try_from(source.len()).map_err(|_| ())?, "encoded source")?;
    let mut output = String::with_capacity(output_capacity); // finding: uncharged_decode_allocation
    let _decoded = decoder.decode_to_string_without_replacement(source, &mut output, true);
    Ok(())
}
