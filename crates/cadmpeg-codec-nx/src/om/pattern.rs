// SPDX-License-Identifier: Apache-2.0
//! Pattern row layouts with their exact scalar families.

use serde::{Deserialize, Serialize};

use super::branch_items::BranchItems;
use super::scalar::{ShiftedBinary32, ShiftedBinary64, ShiftedScalar};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum PatternScalarEncoding {
    ExactOne,
    Binary32,
    Binary64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PatternTerminal {
    ExactOne,
    Binary32(ShiftedBinary32),
}

impl PatternTerminal {
    pub(crate) fn read(bytes: &[u8]) -> Option<Self> {
        if bytes.first() == Some(&1) {
            Some(Self::ExactOne)
        } else {
            ShiftedBinary32::read(bytes.get(..4)?).map(Self::Binary32)
        }
    }

    pub(crate) fn value(self) -> f64 {
        match self {
            Self::ExactOne => 1.0,
            Self::Binary32(atom) => atom.value(),
        }
    }

    pub(crate) fn raw(&self) -> &[u8] {
        match self {
            Self::ExactOne => &[1],
            Self::Binary32(atom) => atom.as_bytes(),
        }
    }

    pub(crate) fn encoding(self) -> PatternScalarEncoding {
        match self {
            Self::ExactOne => PatternScalarEncoding::ExactOne,
            Self::Binary32(_) => PatternScalarEncoding::Binary32,
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
    pub(crate) first: [PatternValue<ShiftedBinary64, O>; 4],
    pub(crate) terminal: PatternValue<PatternTerminal, O>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PatternRow<V, I> {
    pub(crate) values: V,
    pub(crate) selector: I,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum PatternRows<I, O> {
    Scalar(BranchItems<PatternRow<PatternValue<ShiftedScalar, O>, I>>),
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
