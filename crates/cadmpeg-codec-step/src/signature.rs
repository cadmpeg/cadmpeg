// SPDX-License-Identifier: Apache-2.0
//! Structural validation for detached CMS signatures in Part 21.

const CMS_SIGNED_DATA_OID: &[u8] = &[0x2a, 0x86, 0x48, 0x86, 0xf7, 0x0d, 0x01, 0x07, 0x02];

#[derive(Debug)]
struct Der<'a> {
    input: &'a [u8],
    at: usize,
}

impl<'a> Der<'a> {
    fn new(input: &'a [u8]) -> Self {
        Self { input, at: 0 }
    }

    fn remaining(&self) -> usize {
        self.input.len().saturating_sub(self.at)
    }

    fn take(&mut self) -> Result<(u8, &'a [u8]), &'static str> {
        let tag = *self.input.get(self.at).ok_or("missing DER tag")?;
        self.at += 1;
        let first_length = *self.input.get(self.at).ok_or("missing DER length")?;
        self.at += 1;
        let length = if first_length & 0x80 == 0 {
            usize::from(first_length)
        } else {
            let octets = usize::from(first_length & 0x7f);
            if octets == 0 || octets > std::mem::size_of::<usize>() {
                return Err("unsupported DER length");
            }
            let end = self.at.checked_add(octets).ok_or("DER length overflow")?;
            let bytes = self.input.get(self.at..end).ok_or("truncated DER length")?;
            if bytes.first() == Some(&0) {
                return Err("non-minimal DER length");
            }
            self.at = end;
            bytes.iter().try_fold(0usize, |value, byte| {
                value
                    .checked_shl(8)
                    .and_then(|value| value.checked_add(usize::from(*byte)))
                    .ok_or("DER length overflow")
            })?
        };
        let end = self.at.checked_add(length).ok_or("DER value overflow")?;
        let value = self.input.get(self.at..end).ok_or("truncated DER value")?;
        self.at = end;
        Ok((tag, value))
    }

    fn take_tag(&mut self, expected: u8) -> Result<&'a [u8], &'static str> {
        let (tag, value) = self.take()?;
        (tag == expected)
            .then_some(value)
            .ok_or("unexpected DER tag")
    }
}

fn validate_integer(value: &[u8]) -> Result<(), &'static str> {
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

fn validate_algorithm_identifier(value: &[u8]) -> Result<(), &'static str> {
    let mut algorithm = Der::new(value);
    if algorithm.take_tag(0x06)?.is_empty() {
        return Err("empty CMS algorithm OID");
    }
    while algorithm.remaining() > 0 {
        algorithm.take()?;
        if algorithm.remaining() > 0 {
            return Err("CMS algorithm identifier has multiple parameters");
        }
    }
    Ok(())
}

fn validate_digest_algorithms(value: &[u8]) -> Result<(), &'static str> {
    let mut algorithms = Der::new(value);
    if algorithms.remaining() == 0 {
        return Err("CMS SignedData has no digest algorithm");
    }
    while algorithms.remaining() > 0 {
        let algorithm = algorithms.take_tag(0x30)?;
        validate_algorithm_identifier(algorithm)?;
    }
    Ok(())
}

fn validate_signer_identifier(tag: u8, value: &[u8]) -> Result<(), &'static str> {
    match tag {
        0x30 => {
            let mut issuer_and_serial = Der::new(value);
            let issuer = issuer_and_serial.take_tag(0x30)?;
            let mut issuer = Der::new(issuer);
            while issuer.remaining() > 0 {
                issuer.take()?;
            }
            validate_integer(issuer_and_serial.take_tag(0x02)?)?;
            require_empty(&issuer_and_serial)
        }
        0xa0 => Ok(()),
        _ => Err("invalid CMS signer identifier"),
    }
}

fn validate_signer_info(value: &[u8]) -> Result<(), &'static str> {
    let mut signer = Der::new(value);
    validate_integer(signer.take_tag(0x02)?)?;
    let (signer_identifier_tag, signer_identifier) = signer.take()?;
    validate_signer_identifier(signer_identifier_tag, signer_identifier)?;
    validate_algorithm_identifier(signer.take_tag(0x30)?)?;
    if signer.input.get(signer.at).copied() == Some(0xa0) {
        signer.take()?;
    }
    validate_algorithm_identifier(signer.take_tag(0x30)?)?;
    signer.take_tag(0x04)?;
    if signer.input.get(signer.at).copied() == Some(0xa1) {
        signer.take()?;
    }
    require_empty(&signer)
}

fn validate_signer_infos(value: &[u8]) -> Result<(), &'static str> {
    let mut signers = Der::new(value);
    if signers.remaining() == 0 {
        return Err("CMS SignedData has no signer");
    }
    while signers.remaining() > 0 {
        validate_signer_info(signers.take_tag(0x30)?)?;
    }
    Ok(())
}

fn require_empty(der: &Der<'_>) -> Result<(), &'static str> {
    (der.remaining() == 0)
        .then_some(())
        .ok_or("trailing DER value")
}

/// Checks the Part 21 CMS envelope and the detached-content invariant.
pub(crate) fn validate_detached_cms(input: &[u8]) -> Result<(), &'static str> {
    let mut content_info = Der::new(input);
    let content_info_value = content_info.take_tag(0x30)?;
    require_empty(&content_info)?;

    let mut content_info = Der::new(content_info_value);
    let content_type = content_info.take_tag(0x06)?;
    if content_type != CMS_SIGNED_DATA_OID {
        return Err("CMS content type is not signedData");
    }
    let signed_data_wrapper = content_info.take_tag(0xa0)?;
    require_empty(&content_info)?;

    let mut wrapper = Der::new(signed_data_wrapper);
    let signed_data_value = wrapper.take_tag(0x30)?;
    require_empty(&wrapper)?;

    let mut signed_data = Der::new(signed_data_value);
    validate_integer(signed_data.take_tag(0x02)?)?;
    validate_digest_algorithms(signed_data.take_tag(0x31)?)?;
    let encap_content_info = signed_data.take_tag(0x30)?;
    let mut encap_content_info = Der::new(encap_content_info);
    encap_content_info.take_tag(0x06)?;
    if encap_content_info.remaining() != 0 {
        return Err("CMS SignedData is not detached");
    }

    let mut optional_stage = 0;
    while signed_data.remaining() > 0 {
        let (tag, value) = signed_data.take()?;
        match tag {
            0xa0 | 0xa1 => {
                let stage = if tag == 0xa0 { 1 } else { 2 };
                if stage <= optional_stage {
                    return Err("CMS optional fields are out of order");
                }
                optional_stage = stage;
                let mut optional = Der::new(value);
                while optional.remaining() > 0 {
                    optional.take()?;
                }
            }
            0x31 => {
                validate_signer_infos(value)?;
                require_empty(&signed_data)?;
                return Ok(());
            }
            _ => return Err("unexpected CMS SignedData field"),
        }
    }
    Err("CMS SignedData has no signer set")
}

#[cfg(test)]
pub(crate) mod tests;
