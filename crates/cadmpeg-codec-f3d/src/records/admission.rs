// SPDX-License-Identifier: Apache-2.0
//! Collection reconstruction with caller admission or aggregate wire admission.

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use std::collections::{BTreeMap, HashSet};
use std::hash::Hash;

#[derive(Clone, Copy)]
pub(crate) enum RecordAdmission<'ctx, 'arena> {
    Charged(&'ctx DecodeContext<'arena>),
    Admitted,
}

impl RecordAdmission<'_, '_> {
    pub(crate) fn reserve_vec<T>(self, values: &mut Vec<T>, count: usize, operation: &'static str) -> Result<(), CodecError> {
        match self {
            Self::Charged(ctx) => ctx.reserve_vec(values, count, operation),
            Self::Admitted => DecodeContext::reserve_admitted_vec(values, count, operation),
        }
    }

    pub(crate) fn collection_vec<T>(self, count: usize, operation: &'static str) -> Result<Vec<T>, CodecError> {
        match self {
            Self::Charged(ctx) => ctx.collection_vec(count, operation),
            Self::Admitted => DecodeContext::admitted_vec(count, operation),
        }
    }

    pub(crate) fn reserve_set<T: Eq + Hash>(self, values: &mut HashSet<T>, count: usize, operation: &'static str) -> Result<(), CodecError> {
        match self {
            Self::Charged(ctx) => ctx.reserve_set(values, count, operation),
            Self::Admitted => DecodeContext::reserve_admitted_set(values, count, operation),
        }
    }

    pub(crate) fn collect_vec<T>(self, values: impl IntoIterator<Item = T>, operation: &'static str) -> Result<Vec<T>, CodecError> {
        match self {
            Self::Charged(ctx) => ctx.collect_vec(values, operation),
            Self::Admitted => {
                let mut out = Vec::new();
                for value in values {
                    DecodeContext::reserve_admitted_vec(&mut out, 1, operation)?;
                    out.push(value);
                }
                Ok(out)
            }
        }
    }

    pub(crate) fn try_collect_vec<T, E: From<CodecError>>(self, values: impl IntoIterator<Item = Result<T, E>>, operation: &'static str) -> Result<Vec<T>, E> {
        match self {
            Self::Charged(ctx) => ctx.try_collect_vec(values, operation),
            Self::Admitted => {
                let mut out = Vec::new();
                for value in values {
                    let value = value?;
                    DecodeContext::reserve_admitted_vec(&mut out, 1, operation).map_err(E::from)?;
                    out.push(value);
                }
                Ok(out)
            }
        }
    }

    pub(crate) fn alloc_filled<T: Clone>(self, count: usize, value: T, operation: &'static str) -> Result<Vec<T>, CodecError> {
        match self {
            Self::Charged(ctx) => ctx.alloc_filled(count, value, operation),
            Self::Admitted => cadmpeg_core::decode::alloc_filled(count, value, operation),
        }
    }

    pub(crate) fn insert_btree_map<K: Ord, V>(self, values: &mut BTreeMap<K, V>, key: K, value: V, operation: &'static str) -> Result<Option<V>, CodecError> {
        match self {
            Self::Charged(ctx) => ctx.insert_btree_map(values, key, value, operation),
            Self::Admitted => Ok(values.insert(key, value)),
        }
    }
}
