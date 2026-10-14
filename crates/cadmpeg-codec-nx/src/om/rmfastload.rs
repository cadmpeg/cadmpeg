// SPDX-License-Identifier: Apache-2.0
//! Entry-relative `RMFastLoad` physical storage and opaque terminal triples.

use cadmpeg_core::decode::{bounded_len, index_from_u32, u64_from_index, DecodeContext, View};
use cadmpeg_core::CodecError;
use cadmpeg_ir::native::bytes::NativeBytes;
use serde::{Deserialize, Serialize};

use crate::layout::{rmfastload_header as header, rmfastload_prefix as prefix};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AtomBytes {
    One([u8; 1]),
    Two([u8; 2]),
    Three([u8; 3]),
    Four([u8; 4]),
}

/// A complete terminal token, without a numeric or class interpretation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct TerminalAtom(AtomBytes);

impl TerminalAtom {
    fn width(first: u8) -> Option<usize> {
        match first {
            0x00..=0x7f => Some(1),
            0x80..=0x8f => Some(2),
            0xa0..=0xaf => Some(3),
            0xc0 => Some(4),
            _ => None,
        }
    }

    fn from_bytes(bytes: &[u8]) -> Option<Self> {
        if Self::width(*bytes.first()?)? != bytes.len() {
            return None;
        }
        Some(Self(match bytes {
            [p] => AtomBytes::One([*p]),
            [p, a] => AtomBytes::Two([*p, *a]),
            [p, a, b] => AtomBytes::Three([*p, *a, *b]),
            [p, a, b, c] => AtomBytes::Four([*p, *a, *b, *c]),
            _ => return None,
        }))
    }

    fn read(view: &mut View<'_>) -> Option<Self> {
        let first = view.u8()?;
        let width = Self::width(first)?;
        let mut bytes = [0_u8; 4];
        bytes[0] = first;
        for byte in &mut bytes[1..width] {
            *byte = view.u8()?;
        }
        Self::from_bytes(&bytes[..width])
    }

    pub(crate) fn bytes(&self) -> &[u8] {
        match &self.0 {
            AtomBytes::One(b) => b,
            AtomBytes::Two(b) => b,
            AtomBytes::Three(b) => b,
            AtomBytes::Four(b) => b,
        }
    }
}

impl Serialize for TerminalAtom {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        NativeBytes::from(self.bytes()).serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for TerminalAtom {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct AtomVisitor;
        impl serde::de::Visitor<'_> for AtomVisitor {
            type Value = TerminalAtom;
            fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str("one complete opaque RMFastLoad terminal token in lowercase hex")
            }
            fn visit_str<E: serde::de::Error>(self, value: &str) -> Result<Self::Value, E> {
                if !matches!(value.len(), 2 | 4 | 6 | 8) {
                    return Err(E::custom(
                        "terminal token width must be one through four bytes",
                    ));
                }
                let digit = |b| match b {
                    b'0'..=b'9' => Some(b - b'0'),
                    b'a'..=b'f' => Some(b - b'a' + 10),
                    _ => None,
                };
                let mut bytes = [0_u8; 4];
                for (target, pair) in bytes.iter_mut().zip(value.as_bytes().chunks_exact(2)) {
                    *target = digit(pair[0])
                        .zip(digit(pair[1]))
                        .map(|(a, b)| a * 16 + b)
                        .ok_or_else(|| {
                            E::custom("terminal token must use lowercase hexadecimal")
                        })?;
                }
                TerminalAtom::from_bytes(&bytes[..value.len() / 2])
                    .ok_or_else(|| E::custom("terminal token leader and width disagree"))
            }
        }
        deserializer.deserialize_str(AtomVisitor)
    }
}

/// One complete `05 00, token[3], 4e` row in physical source order.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Triple {
    pub(crate) offset: usize,
    pub(crate) atoms: [TerminalAtom; 3],
}

/// A fully validated borrowed physical layout; its spans are not entities.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Storage<'a> {
    bytes: &'a [u8],
    pub(crate) header_offset: usize,
    pub(crate) header_words: [u32; 9],
    pub(crate) boundary_offset: usize,
    pub(crate) count_offset: usize,
    pub(crate) table_end: usize,
    pub(crate) terminal_offset: usize,
    pub(crate) trailer_offset: usize,
}

impl<'a> Storage<'a> {
    /// Require complete framing and the independently parsed owning class count.
    pub(crate) fn read(
        ctx: &DecodeContext<'_>,
        bytes: &'a [u8],
        class_count: usize,
    ) -> Result<Option<Self>, CodecError> {
        let head = (|| {
            if bytes.get(..8)? != prefix::SIGNATURE_VALUE
                || bytes.get(prefix::MARKER..prefix::MARKER + 4)? != prefix::MARKER_VALUE
                || index_from_u32(View::u32_be_at(bytes, prefix::PAYLOAD_LEN)?).checked_add(12)?
                    != bytes.len()
            {
                return None;
            }
            let mut view = View::over_retained(bytes);
            view.skip(prefix::PLATFORM_LENGTH)?;
            let platform_len = index_from_u32(view.u32_le()?);
            view.skip(platform_len)?;
            let name_len = index_from_u32(view.u32_le()?);
            view.skip(name_len)?;
            let header_offset = view.position();
            let mut words = [0_u32; 9];
            for word in &mut words {
                *word = view.u32_le()?;
            }
            if words[0] == 0
                || index_from_u32(words[3]) != class_count
                || [2, 5, 6, 7].into_iter().any(|i| words[i] != 0)
            {
                return None;
            }
            let terminal = index_from_u32(words[8]);
            if terminal < header_offset.checked_add(header::LEN)?
                || terminal.checked_add(24)? > bytes.len()
            {
                return None;
            }
            Some((header_offset, words, terminal))
        })();
        let Some((header_offset, header_words, terminal_offset)) = head else {
            return Ok(None);
        };
        let count = index_from_u32(header_words[0]);
        let Some(index_len) = count.checked_add(1).and_then(|n| n.checked_mul(4)) else {
            return Ok(None);
        };
        let mut selected = None;
        let Some(scan_end) = bytes.len().checked_sub(3) else {
            return Ok(None);
        };
        for at in 0..scan_end {
            ctx.charge_work(1, "scan NX RMFastLoad boundary count")?;
            if View::u32_le_at(bytes, at) != Some(header_words[0]) {
                continue;
            }
            let Some(start) = at.checked_sub(index_len) else {
                continue;
            };
            let Some(end) = count
                .checked_mul(4)
                .and_then(|n| at.checked_add(4)?.checked_add(n))
            else {
                continue;
            };
            if start < header_offset + header::LEN
                || end > terminal_offset
                || View::u32_le_at(bytes, start) != Some(0)
                || View::u32_le_at(bytes, start + 4).is_none_or(|n| n == 0)
            {
                continue;
            }
            ctx.charge_work(
                u64_from_index(count + 1),
                "check NX RMFastLoad physical boundaries",
            )?;
            let mut previous = 0;
            let monotone = (0..=count).all(|i| {
                let Some(value) = View::u32_le_at(bytes, start + i * 4) else {
                    return false;
                };
                let valid = value >= previous && index_from_u32(value) <= terminal_offset;
                previous = value;
                valid
            });
            if !monotone {
                continue;
            }
            if selected.replace((start, at, end)).is_some() {
                return Ok(None);
            }
        }
        let Some((boundary_offset, count_offset, table_end)) = selected else {
            return Ok(None);
        };
        let row_count = index_from_u32(header_words[4]);
        if bounded_len(
            u64::from(header_words[4]),
            6,
            bytes.len() - terminal_offset - 24,
        )
        .is_none()
        {
            return Ok(None);
        }
        let row_start = terminal_offset + 12;
        let mut view = View::over_retained(&bytes[row_start..]);
        for _ in 0..row_count {
            ctx.charge_work(15, "check NX RMFastLoad terminal triple")?;
            if view.take(2) != Some(&[0x05, 0x00]) {
                return Ok(None);
            }
            for _ in 0..3 {
                if TerminalAtom::read(&mut view).is_none() {
                    return Ok(None);
                }
            }
            if view.u8() != Some(0x4e) {
                return Ok(None);
            }
        }
        if view.remaining() != 12 {
            return Ok(None);
        }
        Ok(Some(Self {
            bytes,
            header_offset,
            header_words,
            boundary_offset,
            count_offset,
            table_end,
            terminal_offset,
            trailer_offset: bytes.len() - 12,
        }))
    }

    pub(crate) fn boundaries(self) -> impl Iterator<Item = u32> + 'a {
        let mut view = View::over_retained(&self.bytes[self.boundary_offset..self.count_offset]);
        std::iter::from_fn(move || view.u32_le())
    }

    pub(crate) fn table_words(self) -> impl Iterator<Item = u32> + 'a {
        let mut view = View::over_retained(&self.bytes[self.count_offset + 4..self.table_end]);
        std::iter::from_fn(move || view.u32_le())
    }

    pub(crate) fn triples(self) -> impl Iterator<Item = Triple> + 'a {
        let start = self.terminal_offset + 12;
        let mut view = View::over_retained(&self.bytes[start..self.trailer_offset]);
        std::iter::from_fn(move || {
            let offset = start + view.position();
            view.skip(2)?;
            let atoms = [
                TerminalAtom::read(&mut view)?,
                TerminalAtom::read(&mut view)?,
                TerminalAtom::read(&mut view)?,
            ];
            view.skip(1)?;
            Some(Triple { offset, atoms })
        })
    }
}

#[cfg(test)]
mod tests;
