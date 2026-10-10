// SPDX-License-Identifier: Apache-2.0
//! Structural validation for detached CMS signatures in Part 21.

use std::collections::BTreeMap;
use std::ops::Range;

use base64::{engine::general_purpose::STANDARD, Engine as _};
use cadmpeg_core::decode::DecodeContext;

use crate::parse::ParseError;

pub(crate) fn decode_payload(
    input: &[u8],
    payload: &Range<usize>,
    ctx: &DecodeContext<'_>,
) -> Result<Vec<u8>, ParseError> {
    let mut compact = Vec::new();
    let mut compact_reservation = ctx.reserve_scoped(0, "step_signature_compact_temp")?;
    let mut at = payload.start;
    while at < payload.end {
        ctx.charge_work(1, "STEP signature cursor traversal")?;
        if input[at].is_ascii_control() || input[at] == b' ' {
            at += 1;
            continue;
        }
        if let Some(end) = crate::lex::print_control_end(ctx, input, at)? {
            if end <= payload.end {
                at = end;
                continue;
            }
        }
        if input.get(at..at + 2) == Some(b"/*") {
            let body = at + 2;
            if let Some(end) = ctx.position_by(
                input[body..payload.end].windows(2),
                |window| Ok(window == b"*/"),
                "STEP signature comment traversal",
            )? {
                at = body + end + 2;
                continue;
            }
        }

        ctx.reserve_scoped_vec(
            &mut compact_reservation,
            &mut compact,
            1,
            "step_signature_compact_items",
        )?;
        compact.push(input[at]);
        at += 1;
    }
    let estimate = base64::decoded_len_estimate(compact.len());
    let mut cms = ctx.alloc_filled(estimate, 0_u8, "step_signature_cms_bytes")?;
    let decoded = STANDARD.decode_slice(&compact, &mut cms).or_else(|error| {
        Err(ParseError::Syntax {
            offset: payload.start,
            message: ctx.format_retained(
                format_args!("invalid SIGNATURE Base64 payload: {error}"),
                "STEP decode_payload text",
            )?,
        })
    })?;
    cms.truncate(decoded);
    // SG-04: this is a structural detached-CMS gate. It does not compute the
    // Part 21 alphabet digest, verify a signer key, or apply caller policy;
    // the codec retains an admitted signature as opaque source data.
    match validate_detached_cms(ctx, &cms) {
        Ok(()) => {}
        Err(CmsError::Invalid(message)) => {
            return Err(ParseError::Syntax {
                offset: payload.start,
                message: ctx.format_retained(
                    format_args!("invalid detached CMS SIGNATURE payload: {message}"),
                    "STEP decode_payload text",
                )?,
            })
        }
        Err(CmsError::Resource(error)) => return Err(ParseError::Resource(error)),
    }
    Ok(cms)
}

const CMS_SIGNED_DATA_OID: &[u8] = &[0x2a, 0x86, 0x48, 0x86, 0xf7, 0x0d, 0x01, 0x07, 0x02];

#[derive(Debug, thiserror::Error)]
enum CmsError {
    #[error("{0}")]
    Invalid(&'static str),
    #[error(transparent)]
    Resource(#[from] cadmpeg_core::CodecError),
}

impl From<&'static str> for CmsError {
    fn from(message: &'static str) -> Self {
        Self::Invalid(message)
    }
}

/// A BER value and its offset in the complete CMS input.
#[derive(Clone, Copy, Debug)]
struct BerValue<'a> {
    input: &'a [u8],
    offset: usize,
}

impl<'a> BerValue<'a> {
    fn root(input: &'a [u8]) -> Self {
        Self { input, offset: 0 }
    }
}

#[derive(Debug)]
struct Ber<'a> {
    input: &'a [u8],
    origin: usize,
    at: usize,
}

impl<'a> Ber<'a> {
    fn new(value: BerValue<'a>) -> Self {
        Self {
            input: value.input,
            origin: value.offset,
            at: 0,
        }
    }

    fn remaining(&self) -> Result<usize, &'static str> {
        self.input
            .len()
            .checked_sub(self.at)
            .ok_or("BER cursor exceeds input")
    }

    fn take(
        &mut self,
        ctx: &DecodeContext<'_>,
        extents: &mut BTreeMap<usize, usize>,
    ) -> Result<(u8, BerValue<'a>), CmsError> {
        let _depth = ctx.enter_nested("STEP BER nesting")?;
        let tag = self.take_tag_octet()?;
        let first_length = *self.input.get(self.at).ok_or("missing BER length")?;
        self.at += 1;
        if first_length == 0x80 {
            if tag & 0x20 == 0 {
                return Err(CmsError::Invalid(
                    "indefinite length on primitive CMS value",
                ));
            }
            let value_start = self.at;
            let offset = self.origin + value_start;
            let value_end = if let Some(end) =
                ctx.get_btree_map(extents, &offset, "STEP BER extent lookup")?
            {
                *end - self.origin
            } else {
                let end = self.indefinite_end(ctx, extents, value_start)?;
                ctx.insert_btree_map(
                        extents,
                        offset,
                        self.origin + end,
                        "STEP BER extent entries",
                    )?;
                end
            };
            if !self
                .input
                .get(value_end..)
                .is_some_and(|bytes| bytes.starts_with(&[0, 0]))
            {
                return Err(CmsError::Invalid("unterminated BER indefinite value"));
            }
            self.at = value_end + 2;
            return Ok((
                tag,
                BerValue {
                    input: &self.input[value_start..value_end],
                    offset,
                },
            ));
        }
        let length = if first_length & 0x80 == 0 {
            usize::from(first_length)
        } else {
            let octets = usize::from(first_length & 0x7f);
            if octets == 0 || octets > std::mem::size_of::<usize>() {
                return Err(CmsError::Invalid("unsupported BER length"));
            }
            let end = self.at.checked_add(octets).ok_or("BER length overflow")?;
            let bytes = self.input.get(self.at..end).ok_or("truncated BER length")?;
            self.at = end;
            // The length uses at most one machine word of octets.
            bytes.iter().try_fold(0usize, |value, byte| {
                value
                    .checked_shl(8)
                    .and_then(|value| value.checked_add(usize::from(*byte)))
                    .ok_or("BER length overflow")
            })?
        };
        let end = self.at.checked_add(length).ok_or("BER value overflow")?;
        let input = self.input.get(self.at..end).ok_or("truncated BER value")?;
        let offset = self.origin + self.at;
        self.at = end;
        Ok((tag, BerValue { input, offset }))
    }

    fn take_tag_octet(&mut self) -> Result<u8, &'static str> {
        let tag = *self.input.get(self.at).ok_or("missing BER tag")?;
        self.at += 1;
        if tag & 0x1f == 0x1f {
            let mut octets = 0;
            loop {
                let byte = *self.input.get(self.at).ok_or("truncated BER tag")?;
                self.at += 1;
                octets += 1;
                if octets > std::mem::size_of::<usize>() * 8 {
                    return Err("BER tag is too long");
                }
                if byte & 0x80 == 0 {
                    break;
                }
            }
        }
        Ok(tag)
    }

    fn indefinite_end(
        &self,
        ctx: &DecodeContext<'_>,
        extents: &mut BTreeMap<usize, usize>,
        start: usize,
    ) -> Result<usize, CmsError> {
        let mut contents = Self {
            input: self.input,
            origin: self.origin,
            at: start,
        };
        loop {
            ctx.charge_work(1, "STEP signature cursor traversal")?;
            if contents
                .input
                .get(contents.at..)
                .is_some_and(|remaining| remaining.starts_with(&[0, 0]))
            {
                return Ok(contents.at);
            }
            if contents.at >= contents.input.len() {
                return Err(CmsError::Invalid("unterminated BER indefinite value"));
            }
            contents.take(ctx, extents)?;
        }
    }

    fn take_tag(
        &mut self,
        ctx: &DecodeContext<'_>,
        extents: &mut BTreeMap<usize, usize>,
        expected: u8,
    ) -> Result<BerValue<'a>, CmsError> {
        let (tag, value) = self.take(ctx, extents)?;
        (tag == expected)
            .then_some(value)
            .ok_or(CmsError::Invalid("unexpected BER tag"))
    }
}

fn validate_integer(value: BerValue<'_>) -> Result<(), &'static str> {
    let value = value.input;
    if value.is_empty() {
        return Err("empty CMS integer");
    }
    if value.len() > 1
        && ((value[0] == 0 && value[1] & 0x80 == 0) || (value[0] == 0xff && value[1] & 0x80 != 0))
    {
        return Err("non-minimal CMS integer");
    }
    Ok(())
}

fn validate_algorithm_identifier(
    ctx: &DecodeContext<'_>,
    extents: &mut BTreeMap<usize, usize>,
    value: BerValue<'_>,
) -> Result<(), CmsError> {
    let mut algorithm = Ber::new(value);
    if algorithm.take_tag(ctx, extents, 0x06)?.input.is_empty() {
        return Err(CmsError::Invalid("empty CMS algorithm OID"));
    }
    while algorithm.remaining()? > 0 {
        ctx.charge_work(1, "STEP signature cursor traversal")?;
        algorithm.take(ctx, extents)?;
        if algorithm.remaining()? > 0 {
            return Err(CmsError::Invalid(
                "CMS algorithm identifier has multiple parameters",
            ));
        }
    }
    Ok(())
}

fn validate_octet_string(
    ctx: &DecodeContext<'_>,
    extents: &mut BTreeMap<usize, usize>,
    tag: u8,
    value: BerValue<'_>,
) -> Result<(), CmsError> {
    let _depth = ctx.enter_nested("STEP CMS octet string nesting")?;
    match tag {
        0x04 => Ok(()),
        0x24 => {
            let mut chunks = Ber::new(value);
            while chunks.remaining()? > 0 {
                ctx.charge_work(1, "STEP signature cursor traversal")?;
                let (chunk_tag, chunk_value) = chunks.take(ctx, extents)?;
                validate_octet_string(ctx, extents, chunk_tag, chunk_value)?;
            }
            Ok(())
        }
        _ => Err(CmsError::Invalid("invalid CMS OCTET STRING")),
    }
}

fn validate_subject_key_identifier(
    ctx: &DecodeContext<'_>,
    extents: &mut BTreeMap<usize, usize>,
    tag: u8,
    value: BerValue<'_>,
) -> Result<(), CmsError> {
    match tag {
        0x80 => Ok(()),
        0xa0 => {
            let mut chunks = Ber::new(value);
            while chunks.remaining()? > 0 {
                ctx.charge_work(1, "STEP signature cursor traversal")?;
                let (chunk_tag, chunk_value) = chunks.take(ctx, extents)?;
                validate_octet_string(ctx, extents, chunk_tag, chunk_value)?;
            }
            Ok(())
        }
        _ => Err(CmsError::Invalid("invalid CMS subject key identifier")),
    }
}

fn validate_digest_algorithms(
    ctx: &DecodeContext<'_>,
    extents: &mut BTreeMap<usize, usize>,
    value: BerValue<'_>,
) -> Result<(), CmsError> {
    let mut algorithms = Ber::new(value);
    if algorithms.remaining()? == 0 {
        return Err(CmsError::Invalid("CMS SignedData has no digest algorithm"));
    }
    while algorithms.remaining()? > 0 {
        ctx.charge_work(1, "STEP signature cursor traversal")?;
        let algorithm = algorithms.take_tag(ctx, extents, 0x30)?;
        validate_algorithm_identifier(ctx, extents, algorithm)?;
    }
    Ok(())
}

fn validate_signer_identifier(
    ctx: &DecodeContext<'_>,
    extents: &mut BTreeMap<usize, usize>,
    tag: u8,
    value: BerValue<'_>,
) -> Result<(), CmsError> {
    match tag {
        0x30 => {
            let mut issuer_and_serial = Ber::new(value);
            let issuer = issuer_and_serial.take_tag(ctx, extents, 0x30)?;
            let mut issuer = Ber::new(issuer);
            while issuer.remaining()? > 0 {
                ctx.charge_work(1, "STEP signature cursor traversal")?;
                issuer.take(ctx, extents)?;
            }
            validate_integer(issuer_and_serial.take_tag(ctx, extents, 0x02)?)?;
            require_empty(&issuer_and_serial).map_err(CmsError::from)
        }
        0x80 | 0xa0 => validate_subject_key_identifier(ctx, extents, tag, value),
        _ => Err(CmsError::Invalid("invalid CMS signer identifier")),
    }
}

fn validate_signer_info(
    ctx: &DecodeContext<'_>,
    extents: &mut BTreeMap<usize, usize>,
    value: BerValue<'_>,
) -> Result<(), CmsError> {
    let mut signer = Ber::new(value);
    validate_integer(signer.take_tag(ctx, extents, 0x02)?)?;
    let (signer_identifier_tag, signer_identifier) = signer.take(ctx, extents)?;
    validate_signer_identifier(ctx, extents, signer_identifier_tag, signer_identifier)?;
    let algorithm = signer.take_tag(ctx, extents, 0x30)?;
    validate_algorithm_identifier(ctx, extents, algorithm)?;
    if signer.input.get(signer.at).copied() == Some(0xa0) {
        signer.take(ctx, extents)?;
    }
    let algorithm = signer.take_tag(ctx, extents, 0x30)?;
    validate_algorithm_identifier(ctx, extents, algorithm)?;
    let (signature_tag, signature_value) = signer.take(ctx, extents)?;
    validate_octet_string(ctx, extents, signature_tag, signature_value)?;
    if signer.input.get(signer.at).copied() == Some(0xa1) {
        signer.take(ctx, extents)?;
    }
    require_empty(&signer).map_err(CmsError::from)
}

fn validate_signer_infos(
    ctx: &DecodeContext<'_>,
    extents: &mut BTreeMap<usize, usize>,
    value: BerValue<'_>,
) -> Result<(), CmsError> {
    let mut signers = Ber::new(value);
    if signers.remaining()? == 0 {
        return Err(CmsError::Invalid("CMS SignedData has no signer"));
    }
    while signers.remaining()? > 0 {
        ctx.charge_work(1, "STEP signature cursor traversal")?;
        let signer = signers.take_tag(ctx, extents, 0x30)?;
        validate_signer_info(ctx, extents, signer)?;
    }
    Ok(())
}

fn require_empty(ber: &Ber<'_>) -> Result<(), &'static str> {
    (ber.remaining()? == 0)
        .then_some(())
        .ok_or("trailing BER value")
}

/// Checks the Part 21 CMS envelope and the detached-content invariant.
///
/// This admits structure only. It does not compute a content digest, verify a
/// signature value, select a public key, or apply a caller trust policy.
fn validate_detached_cms(ctx: &DecodeContext<'_>, input: &[u8]) -> Result<(), CmsError> {
    let mut storage = ctx.reserve_scoped(0, "STEP BER extent storage")?;
    storage.with_storage(|| {
    let mut ends = BTreeMap::new();
    let extents = &mut ends;
    let mut content_info = Ber::new(BerValue::root(input));
    let content_info_value = content_info.take_tag(ctx, extents, 0x30)?;
    require_empty(&content_info)?;

    let mut content_info = Ber::new(content_info_value);
    let content_type = content_info.take_tag(ctx, extents, 0x06)?;
    if content_type.input != CMS_SIGNED_DATA_OID {
        return Err(CmsError::Invalid("CMS content type is not signedData"));
    }
    let signed_data_wrapper = content_info.take_tag(ctx, extents, 0xa0)?;
    require_empty(&content_info)?;

    let mut wrapper = Ber::new(signed_data_wrapper);
    let signed_data_value = wrapper.take_tag(ctx, extents, 0x30)?;
    require_empty(&wrapper)?;

    let mut signed_data = Ber::new(signed_data_value);
    validate_integer(signed_data.take_tag(ctx, extents, 0x02)?)?;
    let digest_algorithms = signed_data.take_tag(ctx, extents, 0x31)?;
    validate_digest_algorithms(ctx, extents, digest_algorithms)?;
    let encap_content_info = signed_data.take_tag(ctx, extents, 0x30)?;
    let mut encap_content_info = Ber::new(encap_content_info);
    encap_content_info.take_tag(ctx, extents, 0x06)?;
    if encap_content_info.remaining()? != 0 {
        return Err(CmsError::Invalid("CMS SignedData is not detached"));
    }

    let mut optional_stage = 0;
    while signed_data.remaining()? > 0 {
        ctx.charge_work(1, "STEP signature cursor traversal")?;
        let (tag, value) = signed_data.take(ctx, extents)?;
        match tag {
            0xa0 | 0xa1 => {
                let stage = if tag == 0xa0 { 1 } else { 2 };
                if stage <= optional_stage {
                    return Err(CmsError::Invalid("CMS optional fields are out of order"));
                }
                optional_stage = stage;
                let mut optional = Ber::new(value);
                while optional.remaining()? > 0 {
                    ctx.charge_work(1, "STEP signature cursor traversal")?;
                    optional.take(ctx, extents)?;
                }
            }
            0x31 => {
                validate_signer_infos(ctx, extents, value)?;
                require_empty(&signed_data)?;
                return Ok(());
            }
            _ => return Err(CmsError::Invalid("unexpected CMS SignedData field")),
        }
    }
    Err(CmsError::Invalid("CMS SignedData has no signer set"))
    })
}

#[cfg(test)]
mod tests;
