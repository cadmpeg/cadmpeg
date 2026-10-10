// SPDX-License-Identifier: Apache-2.0
//! Exact native payload bytes with a hexadecimal wire representation.

use std::fmt;
use std::ops::{Deref, DerefMut};

use serde::{de::Visitor, Deserialize, Deserializer, Serialize, Serializer};

/// A native byte payload serialized as two lowercase hexadecimal digits per byte.
///
/// The storage can be an owned vector, a fixed array, or a borrowed slice. Borrowed
/// serialization does not copy the payload. Deserialization requires a string and
/// rejects odd lengths, non-hexadecimal characters, and a wrong fixed-array length.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct NativeBytes<T = Vec<u8>>(T);

impl<T> NativeBytes<T> {
    /// Wraps storage without copying it.
    pub const fn new(value: T) -> Self {
        Self(value)
    }

    /// Returns the underlying storage without copying it.
    pub fn into_inner(self) -> T {
        self.0
    }
}

impl NativeBytes {
    /// Serializes reconstructed byte lanes without collecting an intermediate vector.
    pub fn iter_wire<I: Iterator<Item = u8> + Clone>(bytes: I) -> impl Serialize {
        IteratorBytes(bytes)
    }
}

struct IteratorBytes<I>(I);

impl<I: Iterator<Item = u8> + Clone> fmt::Display for IteratorBytes<I> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut bytes = self.0.clone();
        let mut buffer = [0u8; 256];
        loop {
            let mut len = 0;
            for target in &mut buffer {
                let Some(byte) = bytes.next() else { break };
                *target = byte;
                len += 1;
            }
            if len == 0 {
                return Ok(());
            }
            fmt::Display::fmt(&NativeBytes::from(&buffer[..len]), formatter)?;
        }
    }
}

impl<I: Iterator<Item = u8> + Clone> Serialize for IteratorBytes<I> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

impl<T> From<T> for NativeBytes<T> {
    fn from(value: T) -> Self {
        Self(value)
    }
}

impl<'a> From<&'a Vec<u8>> for NativeBytes<&'a [u8]> {
    fn from(value: &'a Vec<u8>) -> Self {
        Self(value.as_slice())
    }
}

impl<'a, const N: usize> From<&'a [u8; N]> for NativeBytes<&'a [u8]> {
    fn from(value: &'a [u8; N]) -> Self {
        Self(value.as_slice())
    }
}

impl<'a, T: AsRef<[u8]>> From<&'a NativeBytes<T>> for NativeBytes<&'a [u8]> {
    fn from(value: &'a NativeBytes<T>) -> Self {
        Self(value.as_ref())
    }
}

impl<T: IntoIterator> IntoIterator for NativeBytes<T> {
    type Item = T::Item;
    type IntoIter = T::IntoIter;

    fn into_iter(self) -> Self::IntoIter {
        self.0.into_iter()
    }
}

impl<T: AsRef<[u8]>> NativeBytes<T> {
    /// Iterates over the exact payload bytes.
    pub fn iter(&self) -> std::slice::Iter<'_, u8> {
        self.as_ref().iter()
    }
}

impl<'a, T: AsRef<[u8]>> IntoIterator for &'a NativeBytes<T> {
    type Item = &'a u8;
    type IntoIter = std::slice::Iter<'a, u8>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

impl<const N: usize> TryFrom<&[u8]> for NativeBytes<[u8; N]> {
    type Error = std::array::TryFromSliceError;

    fn try_from(value: &[u8]) -> Result<Self, Self::Error> {
        value.try_into().map(Self)
    }
}

impl<T: AsRef<[u8]>, const N: usize> PartialEq<[u8; N]> for NativeBytes<T> {
    fn eq(&self, other: &[u8; N]) -> bool {
        self.as_ref() == other
    }
}

impl<T: AsRef<[u8]>> PartialEq<Vec<u8>> for NativeBytes<T> {
    fn eq(&self, other: &Vec<u8>) -> bool {
        self.as_ref() == other.as_slice()
    }
}

impl<T: AsRef<[u8]>, const N: usize> PartialEq<NativeBytes<T>> for [u8; N] {
    fn eq(&self, other: &NativeBytes<T>) -> bool {
        self.as_slice() == other.as_ref()
    }
}

impl<T: AsRef<[u8]>> PartialEq<NativeBytes<T>> for Vec<u8> {
    fn eq(&self, other: &NativeBytes<T>) -> bool {
        self.as_slice() == other.as_ref()
    }
}

impl FromIterator<u8> for NativeBytes {
    fn from_iter<I: IntoIterator<Item = u8>>(iter: I) -> Self {
        Self(iter.into_iter().collect())
    }
}

impl<T: AsRef<[u8]>, const N: usize> PartialEq<&[u8; N]> for NativeBytes<T> {
    fn eq(&self, other: &&[u8; N]) -> bool {
        self.as_ref() == *other
    }
}

impl<T: AsRef<[u8]>> PartialEq<&[u8]> for NativeBytes<T> {
    fn eq(&self, other: &&[u8]) -> bool {
        self.as_ref() == *other
    }
}

impl<T> Deref for NativeBytes<T> {
    type Target = T;

    fn deref(&self) -> &T {
        &self.0
    }
}

impl<T> DerefMut for NativeBytes<T> {
    fn deref_mut(&mut self) -> &mut T {
        &mut self.0
    }
}

impl<T: AsRef<[u8]>> AsRef<[u8]> for NativeBytes<T> {
    fn as_ref(&self) -> &[u8] {
        self.0.as_ref()
    }
}

impl<T: AsRef<[u8]>> fmt::Display for NativeBytes<T> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        const HEX: &[u8; 16] = b"0123456789abcdef";
        let mut output = [0u8; 512];
        for chunk in self.as_ref().chunks(output.len() / 2) {
            for (byte, pair) in chunk.iter().zip(output.chunks_exact_mut(2)) {
                pair[0] = HEX[usize::from(byte >> 4)];
                pair[1] = HEX[usize::from(byte & 15)];
            }
            let text = std::str::from_utf8(&output[..chunk.len() * 2]).map_err(|_| fmt::Error)?;
            formatter.write_str(text)?;
        }
        Ok(())
    }
}

impl<T: AsRef<[u8]>> Serialize for NativeBytes<T> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

impl<'de, T: TryFrom<Vec<u8>>> Deserialize<'de> for NativeBytes<T> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct HexVisitor<T>(std::marker::PhantomData<T>);

        impl<T: TryFrom<Vec<u8>>> Visitor<'_> for HexVisitor<T> {
            type Value = NativeBytes<T>;

            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str("an even-length hexadecimal byte string")
            }

            fn visit_str<E: serde::de::Error>(self, text: &str) -> Result<Self::Value, E> {
                fn digit(byte: u8) -> u8 {
                    match byte {
                        b'0'..=b'9' => byte - b'0',
                        b'a'..=b'f' => byte - b'a' + 10,
                        _ => byte - b'A' + 10,
                    }
                }
                if !text.len().is_multiple_of(2)
                    || !text.bytes().all(|byte| byte.is_ascii_hexdigit())
                {
                    return Err(E::custom(
                        "native bytes must be an even-length hexadecimal string",
                    ));
                }
                let bytes = text
                    .as_bytes()
                    .chunks_exact(2)
                    .map(|pair| (digit(pair[0]) << 4) | digit(pair[1]))
                    .collect::<Vec<_>>();
                T::try_from(bytes)
                    .map(NativeBytes::from)
                    .map_err(|_| E::custom("native byte string has the wrong payload length"))
            }
        }

        deserializer.deserialize_str(HexVisitor(std::marker::PhantomData))
    }
}

impl<T> crate::schema::rewrite::typed::RewriteIdentities for NativeBytes<T> {
    fn rewrite_native_value<F: FnMut(&str) -> Result<String, cadmpeg_core::CodecError>>(
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        _value: &mut serde_json::Value,
        _map: &mut crate::schema::rewrite::typed::IdentityMap<'_, F>,
    ) -> Result<(), cadmpeg_core::CodecError> {
        ctx.charge_work(1, "walk native identity scalar")
    }

    fn visit_identity_references(
        &self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        _visitor: &mut dyn FnMut(&str) -> Result<(), cadmpeg_core::CodecError>,
    ) -> Result<(), cadmpeg_core::CodecError> {
        ctx.charge_work(1, "walk typed reference scalar")
    }

    fn rewrite_identities<F: FnMut(&str) -> Result<String, cadmpeg_core::CodecError>>(
        self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        _map: &mut crate::schema::rewrite::typed::IdentityMap<'_, F>,
    ) -> Result<Self, cadmpeg_core::CodecError> {
        ctx.charge_work(1, "identity rewrite scalar")?;
        Ok(self)
    }
}

#[cfg(test)]
mod tests;
