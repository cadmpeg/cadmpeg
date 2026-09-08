// SPDX-License-Identifier: Apache-2.0
//! Exact forty-byte datum-plane descriptors.

use super::compact::CompactIndexAtom;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PlaneDescriptor {
    identity: String,
    schema: CompactIndexAtom,
    label: String,
}

impl PlaneDescriptor {
    pub(crate) fn read(bytes: &[u8]) -> Option<Self> {
        if bytes.len() != 40 {
            return None;
        }
        let delimiter = bytes.iter().position(|byte| *byte == b'?')?;
        let identity = bytes.get(..delimiter)?;
        if identity.is_empty()
            || !identity
                .iter()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(byte))
        {
            return None;
        }
        let suffix = bytes.get(delimiter..)?;
        if suffix.get(..2) != Some(b"?A") {
            return None;
        }
        let schema = CompactIndexAtom::read(suffix.get(2..)?)?;
        let label_start = 2 + schema.raw().len() + 3;
        if suffix.get(2 + schema.raw().len()..label_start) != Some(&[0xff, 0x02, 0x01]) {
            return None;
        }
        let label = suffix.get(label_start..)?;
        if label.is_empty() || !label.iter().all(u8::is_ascii_graphic) {
            return None;
        }
        Some(Self {
            identity: identity.iter().copied().map(char::from).collect(),
            schema,
            label: label.iter().copied().map(char::from).collect(),
        })
    }

    // This conversion consumes the input carrier at the typed construction boundary.
    #[allow(clippy::needless_pass_by_value)]
    pub(crate) fn from_wire(
        identity: String,
        suffix: &[u8],
        schema_index: u32,
        label: &str,
    ) -> Result<Self, &'static str> {
        let mut bytes = identity.as_bytes().to_vec();
        bytes.extend_from_slice(suffix);
        let descriptor = Self::read(&bytes)
            .ok_or("identity/suffix must form a forty-byte datum-plane descriptor")?;
        if descriptor.identity() != identity {
            return Err("identity disagrees with descriptor delimiter");
        }
        if descriptor.schema_index() != schema_index {
            return Err("schema_index disagrees with suffix");
        }
        if descriptor.label() != label {
            return Err("label disagrees with suffix");
        }
        Ok(descriptor)
    }

    pub(crate) fn identity(&self) -> &str {
        &self.identity
    }
    pub(crate) fn schema_index(&self) -> u32 {
        self.schema.value()
    }
    pub(crate) fn label(&self) -> &str {
        &self.label
    }
    pub(crate) fn suffix(&self) -> Vec<u8> {
        let mut bytes = b"?A".to_vec();
        bytes.extend_from_slice(self.schema.raw());
        bytes.extend_from_slice(&[0xff, 0x02, 0x01]);
        bytes.extend_from_slice(self.label.as_bytes());
        bytes
    }
}

#[cfg(test)]
mod tests {
    use super::PlaneDescriptor;

    #[test]
    fn descriptor_retains_exact_schema_token_width() {
        for (schema, label) in [(&[0][..], "abcd"), (&[0x80, 0][..], "abc")] {
            let mut bytes = b"012345678901234567890123456789?A".to_vec();
            bytes.extend_from_slice(schema);
            bytes.extend_from_slice(&[0xff, 2, 1]);
            bytes.extend_from_slice(label.as_bytes());
            let descriptor = PlaneDescriptor::read(&bytes).unwrap();
            assert_eq!(descriptor.identity(), "012345678901234567890123456789");
            assert_eq!(descriptor.schema_index(), 0);
            assert_eq!(descriptor.suffix(), bytes[30..]);
            assert!(PlaneDescriptor::from_wire(
                descriptor.identity().into(),
                &descriptor.suffix(),
                1,
                label
            )
            .is_err());
            assert!(PlaneDescriptor::from_wire(
                descriptor.identity().into(),
                &descriptor.suffix(),
                0,
                "other"
            )
            .is_err());
            bytes.pop();
            assert!(PlaneDescriptor::read(&bytes).is_none());
        }
    }
}
