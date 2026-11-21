// SPDX-License-Identifier: Apache-2.0
//! Pattern row layouts with their exact scalar families.

use cadmpeg_ir::scalar::FiniteReal;
use serde::{Deserialize, Serialize};

use super::branch_items::BranchItems;
use super::scalar::ShiftedBinary64;

/// Every pattern-lane atom states four times its value.
const PATTERN_ATOM_SCALE: f64 = 0.25;

/// A four-byte pattern atom: the leading four bytes of a binary64 whose first
/// byte is stored 0x10 higher; the low four bytes are zero.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct PatternNarrow([u8; 4], FiniteReal);

impl Eq for PatternNarrow {}

impl PatternNarrow {
    pub(crate) fn read(bytes: &[u8]) -> Option<Self> {
        let raw = <[u8; 4]>::try_from(bytes).ok()?;
        if !matches!(raw[0], 0x40..=0x5f | 0xc0..=0xdf) {
            return None;
        }
        let mut widened = [0_u8; 8];
        widened[..4].copy_from_slice(&raw);
        widened[0] -= 0x10;
        // endian-exception: reconstructed-scalar
        let value = FiniteReal::new(f64::from_be_bytes(widened) * PATTERN_ATOM_SCALE)?;
        Some(Self(raw, value))
    }

    pub(crate) fn value(self) -> FiniteReal {
        self.1
    }

    pub(crate) fn as_bytes(&self) -> &[u8; 4] {
        &self.0
    }
}

/// An eight-byte pattern atom: a shifted binary64 that states four times its
/// value.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct PatternWide(ShiftedBinary64, FiniteReal);

impl Eq for PatternWide {}

impl PatternWide {
    pub(crate) fn read(bytes: &[u8]) -> Option<Self> {
        let atom = ShiftedBinary64::read(bytes)?;
        let value = FiniteReal::new(atom.value().get() * PATTERN_ATOM_SCALE)?;
        Some(Self(atom, value))
    }

    pub(crate) fn value(self) -> FiniteReal {
        self.1
    }

    pub(crate) fn as_bytes(&self) -> &[u8; 8] {
        self.0.as_bytes()
    }
}

/// One scalar-row pattern atom.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PatternAtom {
    Narrow(PatternNarrow),
    Wide(PatternWide),
}

impl PatternAtom {
    pub(crate) fn read(bytes: &[u8]) -> Option<Self> {
        match bytes.first()? {
            0x40..=0x5f | 0xc0..=0xdf => PatternNarrow::read(bytes.get(..4)?).map(Self::Narrow),
            0x20..=0x3f | 0xa0..=0xbf => PatternWide::read(bytes.get(..8)?).map(Self::Wide),
            _ => None,
        }
    }

    pub(crate) fn value(self) -> FiniteReal {
        match self {
            Self::Narrow(atom) => atom.value(),
            Self::Wide(atom) => atom.value(),
        }
    }

    pub(crate) fn raw(&self) -> &[u8] {
        match self {
            Self::Narrow(atom) => atom.as_bytes(),
            Self::Wide(atom) => atom.as_bytes(),
        }
    }

    pub(crate) fn encoding(self) -> PatternScalarEncoding {
        match self {
            Self::Narrow(_) => PatternScalarEncoding::Binary32,
            Self::Wide(_) => PatternScalarEncoding::Binary64,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum PatternScalarEncoding {
    ExactOne,
    /// A four-byte atom.
    Binary32,
    /// An eight-byte atom.
    Binary64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PatternTerminal {
    ExactOne,
    Narrow(PatternNarrow),
}

impl PatternTerminal {
    pub(crate) fn read(bytes: &[u8]) -> Option<Self> {
        if bytes.first() == Some(&1) {
            Some(Self::ExactOne)
        } else {
            PatternNarrow::read(bytes.get(..4)?).map(Self::Narrow)
        }
    }

    pub(crate) fn value(self) -> FiniteReal {
        match self {
            Self::ExactOne => FiniteReal::ONE,
            Self::Narrow(atom) => atom.value(),
        }
    }

    pub(crate) fn raw(&self) -> &[u8] {
        match self {
            Self::ExactOne => &[1],
            Self::Narrow(atom) => atom.as_bytes(),
        }
    }

    pub(crate) fn encoding(self) -> PatternScalarEncoding {
        match self {
            Self::ExactOne => PatternScalarEncoding::ExactOne,
            Self::Narrow(_) => PatternScalarEncoding::Binary32,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PatternValue<A, O> {
    pub(crate) scalar: A,
    pub(crate) offset: O,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PatternWideValues<O> {
    pub(crate) first: [PatternValue<PatternWide, O>; 4],
    pub(crate) terminal: PatternValue<PatternTerminal, O>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PatternRow<V, I> {
    pub(crate) values: V,
    pub(crate) selector: I,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum PatternRows<I, O> {
    Scalar(BranchItems<PatternRow<PatternValue<PatternAtom, O>, I>>),
    Wide(BranchItems<PatternRow<PatternWideValues<O>, I>>),
}

impl<I, O> PatternRows<I, O> {
    pub(crate) fn declared_count(&self) -> u8 {
        match self {
            Self::Scalar(rows) => rows.declared_count(),
            Self::Wide(rows) => rows.declared_count(),
        }
    }

    pub(crate) fn map<J, P>(
        self,
        mut selector: impl FnMut(I) -> J,
        mut offset: impl FnMut(O) -> P,
    ) -> PatternRows<J, P> {
        match self {
            Self::Scalar(rows) => PatternRows::Scalar(rows.map_indexed(|_, row| PatternRow {
                values: PatternValue {
                    scalar: row.values.scalar,
                    offset: offset(row.values.offset),
                },
                selector: selector(row.selector),
            })),
            Self::Wide(rows) => PatternRows::Wide(rows.map_indexed(|_, row| PatternRow {
                values: PatternWideValues {
                    first: row.values.first.map(|value| PatternValue {
                        scalar: value.scalar,
                        offset: offset(value.offset),
                    }),
                    terminal: PatternValue {
                        scalar: row.values.terminal.scalar,
                        offset: offset(row.values.terminal.offset),
                    },
                },
                selector: selector(row.selector),
            })),
        }
    }
}

#[cfg(test)]
mod tests;
