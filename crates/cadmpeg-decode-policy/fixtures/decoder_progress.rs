// SPDX-License-Identifier: Apache-2.0
use cadmpeg_core::decode::{u64_from_index, DecodeContext};
use cadmpeg_core::CodecError;
use encoding_rs::{Decoder, DecoderResult, Encoding};

// Slice decoder calls have a known input-sized scan. A missing progress proof
// leaves that work uncharged. Full decoding also needs result-storage admission.

pub fn direct_source(
    ctx: &DecodeContext<'_>,
    encoding: &'static Encoding,
    source: &[u8],
) -> Result<(), CodecError> {
    ctx.charge_work(u64_from_index(source.len()), "direct measured decode")?;
    let mut decoder = encoding.new_decoder_without_bom_handling();
    let mut buffer = [0_u8; 4];
    let mut read = 0_usize;
    let mut written_total = 0_usize;
    loop {
        let (result, consumed, written) =
            decoder.decode_to_utf8_without_replacement(&source[read..], &mut buffer, true);
        read += consumed;
        written_total += written;
        match result {
            DecoderResult::InputEmpty => break,
            DecoderResult::OutputFull => {}
            DecoderResult::Malformed(..) => {
                return Err(CodecError::Malformed("invalid measured text".into()));
            }
        }
    }
    let _ = written_total;
    Ok(())
}

pub fn bom_derived_source(
    ctx: &DecodeContext<'_>,
    fallback: &'static Encoding,
    bytes: &[u8],
) -> Result<(), CodecError> {
    ctx.charge_work(u64_from_index(bytes.len()), "BOM measured decode")?;
    let (selected_encoding, source) = Encoding::for_bom(bytes)
        .map_or((fallback, bytes), |(selected, bom_len)| {
            (selected, &bytes[bom_len..])
        });
    let mut decoder = selected_encoding.new_decoder_without_bom_handling();
    let mut buffer = [0_u8; 4];
    let mut read = 0_usize;
    let mut written_total = 0_usize;
    loop {
        let (result, consumed, written) =
            decoder.decode_to_utf8_without_replacement(&source[read..], &mut buffer, true);
        read += consumed;
        written_total += written;
        match result {
            DecoderResult::InputEmpty => break,
            DecoderResult::OutputFull => {}
            DecoderResult::Malformed(..) => {
                return Err(CodecError::Malformed("invalid BOM measured text".into()));
            }
        }
    }
    let _ = written_total;
    Ok(())
}

pub fn wrong_source(
    ctx: &DecodeContext<'_>,
    encoding: &'static Encoding,
    charged: &[u8],
    source: &[u8],
) -> Result<(), CodecError> {
    ctx.charge_work(u64_from_index(charged.len()), "wrong source charge")?;
    let mut decoder = encoding.new_decoder_without_bom_handling();
    let mut buffer = [0_u8; 4];
    let mut read = 0_usize;
    loop { // finding: uncharged_decode_work
        let (result, consumed, _written) =
            decoder.decode_to_utf8_without_replacement(&source[read..], &mut buffer, true);
        read += consumed;
        match result {
            DecoderResult::InputEmpty => break,
            DecoderResult::OutputFull => {}
            DecoderResult::Malformed(..) => {
                return Err(CodecError::Malformed("invalid wrong-source text".into()));
            }
        }
    }
    Ok(())
}

pub fn reordered_offset(
    ctx: &DecodeContext<'_>,
    encoding: &'static Encoding,
    bytes: &[u8],
) -> Result<(), CodecError> {
    let suffix = &bytes[3..];
    ctx.charge_work(u64_from_index(suffix.len()), "offset source charge")?;
    let mut decoder = encoding.new_decoder_without_bom_handling();
    let mut buffer = [0_u8; 4];
    let mut read = 0_usize;
    loop { // finding: uncharged_decode_work
        let (result, consumed, _written) =
            decoder.decode_to_utf8_without_replacement(&bytes[read..], &mut buffer, true);
        read += consumed;
        match result {
            DecoderResult::InputEmpty => break,
            DecoderResult::OutputFull => {}
            DecoderResult::Malformed(..) => {
                return Err(CodecError::Malformed("invalid reordered-offset text".into()));
            }
        }
    }
    Ok(())
}

pub fn ignored_consumed(
    ctx: &DecodeContext<'_>,
    encoding: &'static Encoding,
    source: &[u8],
) -> Result<(), CodecError> {
    ctx.charge_work(u64_from_index(source.len()), "ignored consumed charge")?;
    let mut decoder = encoding.new_decoder_without_bom_handling();
    let mut buffer = [0_u8; 4];
    let mut read = 0_usize;
    loop { // finding: uncharged_decode_work
        let (result, _consumed, written) =
            decoder.decode_to_utf8_without_replacement(&source[read..], &mut buffer, true); // finding: uncharged_decode_work
        read += written;
        match result {
            DecoderResult::InputEmpty => break,
            DecoderResult::OutputFull => {}
            DecoderResult::Malformed(..) => {
                return Err(CodecError::Malformed("invalid ignored-consumed text".into()));
            }
        }
    }
    Ok(())
}

pub fn swallowed_charge(
    ctx: &DecodeContext<'_>,
    encoding: &'static Encoding,
    source: &[u8],
) -> Result<(), CodecError> {
    let _ = ctx.charge_work(u64_from_index(source.len()), "swallowed work charge");
    let mut decoder = encoding.new_decoder_without_bom_handling();
    let mut buffer = [0_u8; 4];
    let mut read = 0_usize;
    loop { // finding: uncharged_decode_work
        let (result, consumed, _written) =
            decoder.decode_to_utf8_without_replacement(&source[read..], &mut buffer, true);
        read += consumed;
        match result {
            DecoderResult::InputEmpty => break,
            DecoderResult::OutputFull => {}
            DecoderResult::Malformed(..) => {
                return Err(CodecError::Malformed("invalid swallowed-charge text".into()));
            }
        }
    }
    Ok(())
}

pub fn borrowed_decoder(
    ctx: &DecodeContext<'_>,
    decoder: &mut Decoder,
    source: &[u8],
) -> Result<(), CodecError> {
    ctx.charge_work(u64_from_index(source.len()), "borrowed decoder charge")?;
    let mut buffer = [0_u8; 4];
    let mut read = 0_usize;
    loop { // finding: uncharged_decode_work
        let (result, consumed, _written) =
            decoder.decode_to_utf8_without_replacement(&source[read..], &mut buffer, true); // finding: uncharged_decode_work
        read += consumed;
        match result {
            DecoderResult::InputEmpty => break,
            DecoderResult::OutputFull => {}
            DecoderResult::Malformed(..) => {
                return Err(CodecError::Malformed("invalid borrowed-decoder text".into()));
            }
        }
    }
    Ok(())
}

pub fn short_buffer(
    ctx: &DecodeContext<'_>,
    encoding: &'static Encoding,
    source: &[u8],
) -> Result<(), CodecError> {
    ctx.charge_work(u64_from_index(source.len()), "short buffer charge")?;
    let mut decoder = encoding.new_decoder_without_bom_handling();
    let mut buffer = [0_u8; 1];
    let mut read = 0_usize;
    loop { // finding: uncharged_decode_work
        let (result, consumed, _written) =
            decoder.decode_to_utf8_without_replacement(&source[read..], &mut buffer, true); // finding: uncharged_decode_work
        read += consumed;
        match result {
            DecoderResult::InputEmpty => break,
            DecoderResult::OutputFull => {}
            DecoderResult::Malformed(..) => {
                return Err(CodecError::Malformed("invalid short-buffer text".into()));
            }
        }
    }
    Ok(())
}

pub fn decoder_used_before_loop(
    ctx: &DecodeContext<'_>,
    encoding: &'static Encoding,
    source: &[u8],
) -> Result<(), CodecError> {
    let mut decoder = encoding.new_decoder_without_bom_handling();
    let mut buffer = [0_u8; 4];
    let (_result, _consumed, _written) = decoder // finding: uncharged_decode_work
        .decode_to_utf8_without_replacement(source, &mut buffer, false);
    ctx.charge_work(u64_from_index(source.len()), "loop source charge")?;
    let mut read = 0_usize;
    loop { // finding: uncharged_decode_work
        let (result, consumed, _written) =
            decoder.decode_to_utf8_without_replacement(&source[read..], &mut buffer, true); // finding: uncharged_decode_work
        read += consumed;
        match result {
            DecoderResult::InputEmpty => break,
            DecoderResult::OutputFull => {}
            DecoderResult::Malformed(..) => {
                return Err(CodecError::Malformed("invalid pre-used decoder text".into()));
            }
        }
    }
    Ok(())
}

pub fn conditional_consumed_update(
    ctx: &DecodeContext<'_>,
    encoding: &'static Encoding,
    source: &[u8],
) -> Result<(), CodecError> {
    ctx.charge_work(u64_from_index(source.len()), "conditional update charge")?;
    let mut decoder = encoding.new_decoder_without_bom_handling();
    let mut buffer = [0_u8; 4];
    let mut read = 0_usize;
    loop { // finding: uncharged_decode_work
        let (result, consumed, _written) =
            decoder.decode_to_utf8_without_replacement(&source[read..], &mut buffer, true); // finding: uncharged_decode_work
        if matches!(result, DecoderResult::OutputFull) {
            read += consumed;
        }
        match result {
            DecoderResult::InputEmpty => break,
            DecoderResult::OutputFull => {}
            DecoderResult::Malformed(..) => {
                return Err(CodecError::Malformed("invalid conditional-update text".into()));
            }
        }
    }
    Ok(())
}

pub fn one_charge_cannot_cover_two_passes(
    ctx: &DecodeContext<'_>,
    encoding: &'static Encoding,
    source: &[u8],
) -> Result<(), CodecError> {
    ctx.charge_work(u64_from_index(source.len()), "single decoder charge")?;
    let mut first_decoder = encoding.new_decoder_without_bom_handling();
    let mut first_buffer = [0_u8; 4];
    let mut first_read = 0_usize;
    loop {
        let (result, consumed, _written) = first_decoder
            .decode_to_utf8_without_replacement(&source[first_read..], &mut first_buffer, true);
        first_read += consumed;
        match result {
            DecoderResult::InputEmpty => break,
            DecoderResult::OutputFull => {}
            DecoderResult::Malformed(..) => {
                return Err(CodecError::Malformed("invalid first-pass text".into()));
            }
        }
    }
    let mut second_decoder = encoding.new_decoder_without_bom_handling();
    let mut second_buffer = [0_u8; 4];
    let mut second_read = 0_usize;
    loop { // finding: uncharged_decode_work
        let (result, consumed, _written) = second_decoder
            .decode_to_utf8_without_replacement(&source[second_read..], &mut second_buffer, true);
        second_read += consumed;
        match result {
            DecoderResult::InputEmpty => break,
            DecoderResult::OutputFull => {}
            DecoderResult::Malformed(..) => {
                return Err(CodecError::Malformed("invalid second-pass text".into()));
            }
        }
    }
    Ok(())
}

pub fn incremental_then_full_decode_is_not_one_pass(
    ctx: &DecodeContext<'_>,
    encoding: &'static Encoding,
    source: &[u8],
) -> Result<(), CodecError> {
    ctx.charge_work(u64_from_index(source.len()), "single full-decode charge")?;
    let mut decoder = encoding.new_decoder_without_bom_handling();
    let mut buffer = [0_u8; 4];
    let mut read = 0_usize;
    loop {
        let (result, consumed, _written) =
            decoder.decode_to_utf8_without_replacement(&source[read..], &mut buffer, true);
        read += consumed;
        match result {
            DecoderResult::InputEmpty => break,
            DecoderResult::OutputFull => {}
            DecoderResult::Malformed(..) => {
                return Err(CodecError::Malformed("invalid incremental text".into()));
            }
        }
    }
    let _second_pass = encoding.decode(source); // finding: uncharged_decode_allocation, uncharged_decode_work
    Ok(())
}

pub fn captured_read_reset(
    ctx: &DecodeContext<'_>,
    encoding: &'static Encoding,
    source: &[u8],
) -> Result<(), CodecError> {
    ctx.charge_work(u64_from_index(source.len()), "captured reset charge")?;
    let mut decoder = encoding.new_decoder_without_bom_handling();
    let mut buffer = [0_u8; 4];
    let mut read = 0_usize;
    loop { // finding: uncharged_decode_work
        let (result, consumed, _written) =
            decoder.decode_to_utf8_without_replacement(&source[read..], &mut buffer, true); // finding: uncharged_decode_work
        read += consumed;
        let mut reset = || read = 0;
        reset();
        match result {
            DecoderResult::InputEmpty => break,
            DecoderResult::OutputFull => {}
            DecoderResult::Malformed(..) => {
                return Err(CodecError::Malformed("invalid captured-reset text".into()));
            }
        }
    }
    Ok(())
}
