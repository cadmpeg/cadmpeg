// SPDX-License-Identifier: Apache-2.0
//! Datum-CSYS descriptor identities and bounded source positions.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub(crate) struct CsysIdentity(String);

impl TryFrom<String> for CsysIdentity {
    type Error = &'static str;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        if !(30..=32).contains(&value.len()) || !value.bytes().all(is_identity_byte) {
            return Err("identity must contain 30 through 32 lowercase hexadecimal digits");
        }
        Ok(Self(value))
    }
}

impl From<CsysIdentity> for String {
    fn from(value: CsysIdentity) -> Self {
        value.0
    }
}

impl CsysIdentity {
    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }
}

fn is_identity_byte(byte: u8) -> bool {
    byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CsysDescriptor {
    prefix: Vec<u8>,
    identity: CsysIdentity,
    suffix: Vec<u8>,
}

impl CsysDescriptor {
    pub(crate) fn read(bytes: &[u8]) -> Option<Self> {
        let mut candidate = None;
        let mut at = 0;
        while at < bytes.len() {
            if !is_identity_byte(bytes[at]) {
                at += 1;
                continue;
            }
            let start = at;
            while at < bytes.len() && is_identity_byte(bytes[at]) {
                at += 1;
            }
            if (30..=32).contains(&(at - start)) {
                if candidate.is_some() {
                    return None;
                }
                candidate = Some((start, at));
            }
        }
        let (start, end) = candidate?;
        Some(Self {
            prefix: bytes[..start].to_vec(),
            identity: CsysIdentity(bytes[start..end].iter().copied().map(char::from).collect()),
            suffix: bytes[end..].to_vec(),
        })
    }

    pub(crate) fn from_wire(
        prefix: Vec<u8>,
        identity: CsysIdentity,
        suffix: Vec<u8>,
    ) -> Result<Self, &'static str> {
        let mut bytes = prefix.clone();
        bytes.extend_from_slice(identity.as_str().as_bytes());
        bytes.extend_from_slice(&suffix);
        let parsed = Self::read(&bytes)
            .ok_or("prefix/identity/suffix must contain one maximal descriptor identity")?;
        if parsed.prefix != prefix || parsed.identity != identity || parsed.suffix != suffix {
            return Err("prefix/identity/suffix disagree with the maximal identity run");
        }
        Ok(parsed)
    }

    pub(crate) fn prefix(&self) -> &[u8] {
        &self.prefix
    }
    pub(crate) fn identity(&self) -> &CsysIdentity {
        &self.identity
    }
    pub(crate) fn suffix(&self) -> &[u8] {
        &self.suffix
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LocatedCsysDescriptor {
    descriptor: CsysDescriptor,
    source_offset: u64,
}

impl LocatedCsysDescriptor {
    pub(crate) fn new(
        descriptor: CsysDescriptor,
        source_offset: u64,
    ) -> Result<Self, &'static str> {
        source_offset
            .checked_add(descriptor.prefix.len() as u64)
            .ok_or("source_offset overflows identity_source_offset")?;
        Ok(Self {
            descriptor,
            source_offset,
        })
    }
    pub(crate) fn descriptor(&self) -> &CsysDescriptor {
        &self.descriptor
    }
    pub(crate) fn source_offset(&self) -> u64 {
        self.source_offset
    }
    pub(crate) fn identity_source_offset(&self) -> u64 {
        self.source_offset + self.descriptor.prefix.len() as u64
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "u8", into = "u8")]
#[repr(u8)]
pub(crate) enum CsysDescriptorSlot {
    Five = 5,
    Six = 6,
    Seven = 7,
}

impl TryFrom<u8> for CsysDescriptorSlot {
    type Error = &'static str;
    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            5 => Ok(Self::Five),
            6 => Ok(Self::Six),
            7 => Ok(Self::Seven),
            _ => Err("reference_ordinal must be 5, 6, or 7"),
        }
    }
}
impl From<CsysDescriptorSlot> for u8 {
    fn from(value: CsysDescriptorSlot) -> Self {
        value as Self
    }
}

#[cfg(test)]
mod tests {
    use super::{CsysDescriptor, CsysDescriptorSlot, CsysIdentity, LocatedCsysDescriptor};

    #[test]
    fn identity_lengths_and_maximal_runs_are_checked_at_construction() {
        for len in 29..=33 {
            let value = "a".repeat(len);
            assert_eq!(
                CsysIdentity::try_from(value).is_ok(),
                (30..=32).contains(&len)
            );
        }
        assert!(CsysIdentity::try_from("A".repeat(30)).is_err());
        let identity = CsysIdentity::try_from("a".repeat(30)).unwrap();
        assert!(CsysDescriptor::from_wire(vec![b'b'], identity.clone(), vec![b'?']).is_err());
        assert!(CsysDescriptor::from_wire(
            vec![0],
            identity.clone(),
            b"?bbbbbbbbbbbbbbbbbbbbbbbbbbbbbb".to_vec()
        )
        .is_err());
        let descriptor = CsysDescriptor::from_wire(vec![2, 1], identity, vec![b'?']).unwrap();
        assert_eq!(
            LocatedCsysDescriptor::new(descriptor.clone(), u64::MAX - 2)
                .unwrap()
                .identity_source_offset(),
            u64::MAX
        );
        assert!(LocatedCsysDescriptor::new(descriptor, u64::MAX - 1).is_err());
        for slot in 0..=u8::MAX {
            assert_eq!(
                CsysDescriptorSlot::try_from(slot).is_ok(),
                (5..=7).contains(&slot)
            );
        }
    }
}
