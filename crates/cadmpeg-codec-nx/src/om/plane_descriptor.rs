// SPDX-License-Identifier: Apache-2.0
//! Exact forty-byte datum-plane descriptors.

use super::compact::CompactIndexAtom;
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PlaneDescriptor {
    identity: String,
    schema: CompactIndexAtom,
    label: String,
}

impl PlaneDescriptor {
    fn parse(bytes: &[u8]) -> Option<(&[u8], CompactIndexAtom, &[u8])> {
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
        Some((identity, schema, label))
    }

    pub(crate) fn read(bytes: &[u8]) -> Option<Self> {
        let (identity, schema, label) = Self::parse(bytes)?;
        Some(Self {
            identity: identity.iter().copied().map(char::from).collect(),
            schema,
            label: label.iter().copied().map(char::from).collect(),
        })
    }

    pub(crate) fn from_bytes(
        ctx: &DecodeContext<'_>,
        bytes: &[u8],
    ) -> Result<Option<Self>, CodecError> {
        let Some((identity, schema, label)) = Self::parse(bytes) else {
            return Ok(None);
        };
        let identity = std::str::from_utf8(identity);
        let label = std::str::from_utf8(label);
        let (Ok(identity), Ok(label)) = (identity, label) else {
            return Ok(None);
        };
        let identity = ctx.copy_retained_text(identity, "NX datum plane descriptor identity")?;
        let label = ctx.copy_retained_text(label, "NX datum plane descriptor label")?;
        Ok(Some(Self {
            identity,
            schema,
            label,
        }))
    }

    pub(crate) fn from_wire(
        identity: &str,
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
    #[cfg(test)]
    pub(crate) fn suffix(&self) -> Vec<u8> {
        self.suffix_bytes().collect()
    }
    pub(crate) fn suffix_bytes(&self) -> impl Iterator<Item = u8> + Clone + '_ {
        b"?A"
            .iter()
            .chain(self.schema.raw())
            .chain(&[0xff, 0x02, 0x01])
            .chain(self.label.as_bytes())
            .copied()
    }
}

#[cfg(test)]
mod tests {
    use super::PlaneDescriptor;

    #[test]
    fn plane_descriptor_text_copy_refusals_propagate() {
        let bytes = b"012345678901234567890123456789?A\x00\xff\x02\x01abcd";
        let descriptor =
            crate::test_support::with_decode_context(|ctx| PlaneDescriptor::from_bytes(ctx, bytes))
                .unwrap()
                .unwrap();
        assert_eq!(descriptor.identity(), "012345678901234567890123456789");
        assert_eq!(descriptor.label(), "abcd");
        assert_eq!(descriptor.schema_index(), 0);
        for (operation, additional) in [
            ("NX datum plane descriptor identity", 30),
            ("NX datum plane descriptor label", 4),
        ] {
            let error = crate::test_support::resource_refusal_at(
                &[],
                cadmpeg_core::decode::ResourceDimension::WorkUnits,
                operation,
                |ctx| PlaneDescriptor::from_bytes(ctx, bytes),
            );
            assert!(matches!(
                error,
                cadmpeg_core::CodecError::ResourceLimit(limit)
                    if limit.additional == additional
            ));
        }
    }

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
                descriptor.identity(),
                &descriptor.suffix(),
                1,
                label
            )
            .is_err());
            assert!(PlaneDescriptor::from_wire(
                descriptor.identity(),
                &descriptor.suffix(),
                0,
                "other"
            )
            .is_err());
            bytes.pop();
            assert!(PlaneDescriptor::read(&bytes).is_none());
        }
    }

    #[test]
    fn plane_descriptor_identity_copy_refusal_propagates() {
        use cadmpeg_core::decode::ResourceDimension;
        use cadmpeg_core::CodecError;
        let error = crate::test_support::resource_refusal_at(
            &[],
            ResourceDimension::WorkUnits,
            "NX datum plane descriptor identity",
            |ctx| {
                PlaneDescriptor::from_bytes(
                    ctx,
                    b"012345678901234567890123456789?A\x00\xff\x02\x01abcd",
                )
            },
        );
        assert!(matches!(error, CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::WorkUnits && limit.operation == "NX datum plane descriptor identity"));
    }
}
