// SPDX-License-Identifier: Apache-2.0
pub fn copy<T: Clone>(value: &T) -> T {
    value.clone()
}
pub fn fixed(bytes: &[u8]) -> usize {
    bytes.len()
}
pub fn with<F: FnOnce() -> usize>(callback: F) -> usize {
    callback()
}

pub fn slice_copy<T: Copy>(values: &[T]) -> Vec<T> {
    values.to_vec()
}
pub fn capacity<T>() -> Vec<T> {
    Vec::with_capacity(4)
}

pub fn grow<T>(values: &mut Vec<T>, value: T) {
    values.push(value);
}

pub mod writer;

pub trait Fixed {
    fn fixed(&self) -> usize {
        1
    }
}
impl Fixed for u32 {}

pub struct DecodeContext;
pub fn context_copy<T: Clone>(_ctx: &DecodeContext, value: &T) -> T {
    value.clone()
}

pub fn constant_capacity<T, const N: usize>() -> Vec<T> {
    Vec::with_capacity(N)
}

pub fn array_copy<T: Copy>(values: &[T; 4]) -> Vec<T> {
    values.to_vec()
}
pub fn vector_copy<T: Clone>(values: &Vec<T>) -> Vec<T> {
    values.clone()
}

pub fn array_move<T>(values: [T; 4]) -> Vec<T> {
    Vec::from_iter(values)
}
pub fn array_collect<T>(values: [T; 4]) -> Vec<T> {
    values.into_iter().collect()
}

pub fn minimum<T: Ord>(first: T, second: T) -> T {
    std::cmp::min(first, second)
}

impl DecodeContext {
    pub fn charge_retained(&self, _bytes: u64, _operation: &str) -> Result<(), ()> {
        Ok(())
    }
}
pub fn reserve<T>(ctx: &DecodeContext, values: &mut Vec<T>, n: usize) -> Result<(), ()> {
    let bytes = n.checked_mul(std::mem::size_of::<T>()).ok_or(())?;
    ctx.charge_retained(u64::try_from(bytes).map_err(|_| ())?, "slots")?;
    values.try_reserve_exact(n).map_err(|_| ())?;
    Ok(())
}

pub fn text<T: Into<String>>(message: T) -> String {
    message.into()
}
pub fn forward_text<T: Into<String>>(message: T) -> String {
    text(message)
}

fn overwrite<T>(value: &mut T, input: T) {
    *value = input;
}
pub fn changed_text<T: Into<String>>(message: T, input: T) -> String {
    let mut message = message;
    overwrite(&mut message, input);
    text(message)
}

impl DecodeContext {
    fn charge_work(&self, _count: u64) -> Result<(), ()> {
        Ok(())
    }
}
pub fn filled<T: Clone>(ctx: &DecodeContext, count: usize, value: T) -> Result<Vec<T>, ()> {
    let mut values = Vec::new();
    reserve(ctx, &mut values, count)?;
    ctx.charge_work(u64::try_from(count).map_err(|_| ())?)?;
    values.resize(count, value);
    Ok(values)
}

pub mod decode {
    pub mod cost {
        pub trait DecodeCost {}
        impl DecodeCost for String {}
        impl DecodeCost for Vec<String> {}
        impl DecodeCost for super::super::key_callbacks::ImportedMarkerHashKey {}
        impl DecodeCost for super::super::key_callbacks::ImportedMarkerEqKey {}
        impl DecodeCost for super::super::key_callbacks::ImportedSelfEqWithOtherRhs {}
    }

    pub mod context {
        use std::collections::HashSet;
        use std::hash::{BuildHasher, Hash};

        use super::cost::DecodeCost;

        pub struct DecodeContext;

        impl DecodeContext {
            pub fn charge_key<K: DecodeCost>(
                &self,
                _key: &K,
                _factor: u64,
                _operation: &'static str,
            ) -> Result<(), ()> {
                Ok(())
            }

            pub fn contains_hash_set<K, S>(
                &self,
                values: &HashSet<K, S>,
                key: &K,
                operation: &'static str,
            ) -> Result<bool, ()>
            where
                K: DecodeCost + Eq + Hash,
                S: BuildHasher,
            {
                self.charge_key(key, 1, operation)?;
                Ok(values.contains(key))
            }
        }
    }
}
impl DecodeContext {
    pub fn charge_key<T: decode::cost::DecodeCost>(
        &self,
        _key: &T,
        _count: u64,
        _operation: &str,
    ) -> Result<(), ()> {
        Ok(())
    }
}
pub fn admitted_lookup<K: Ord + decode::cost::DecodeCost, V>(
    ctx: &DecodeContext,
    values: &std::collections::BTreeMap<K, V>,
    key: &K,
) -> Result<bool, ()> {
    ctx.charge_key(key, ctx.tree_comparisons(values.len()), "lookup")?;
    Ok(values.contains_key(key))
}
impl DecodeContext {
    pub fn tree_comparisons(&self, _count: usize) -> u64 {
        1
    }
}
pub fn wrong_lookup<K: Ord + decode::cost::DecodeCost, V>(
    ctx: &DecodeContext,
    values: &std::collections::BTreeMap<K, V>,
    key: &K,
    other: &K,
) -> Result<bool, ()> {
    ctx.charge_key(other, ctx.tree_comparisons(values.len()), "lookup")?;
    Ok(values.contains_key(key))
}

pub fn generic_write_char<W: std::fmt::Write>(output: &mut W, character: char) -> std::fmt::Result {
    output.write_char(character)
}

pub fn generic_write_str<W: std::fmt::Write>(output: &mut W, suffix: &str) -> std::fmt::Result {
    output.write_str(suffix)
}

pub mod key_callbacks {
    use std::hash::{Hash, Hasher};

    pub struct ImportedMarkerHashKey(pub String);

    #[automatically_derived]
    impl Hash for ImportedMarkerHashKey {
        fn hash<H: Hasher>(&self, state: &mut H) {
            for byte in self.0.bytes() {
                state.write_u8(byte);
            }
        }
    }

    #[automatically_derived]
    impl PartialEq for ImportedMarkerHashKey {
        fn eq(&self, other: &Self) -> bool {
            self.0 == other.0
        }
    }

    #[automatically_derived]
    impl Eq for ImportedMarkerHashKey {}

    pub struct ImportedMarkerEqKey(pub String);

    #[automatically_derived]
    impl Hash for ImportedMarkerEqKey {
        fn hash<H: Hasher>(&self, state: &mut H) {
            self.0.hash(state);
        }
    }

    #[automatically_derived]
    impl PartialEq for ImportedMarkerEqKey {
        fn eq(&self, other: &Self) -> bool {
            for (left, right) in self.0.bytes().zip(other.0.bytes()) {
                if left != right {
                    return false;
                }
            }
            true
        }
    }

    #[automatically_derived]
    impl Eq for ImportedMarkerEqKey {}

    #[derive(Hash, PartialEq, Eq)]
    pub struct ImportedSelfEqWithOtherRhs(pub String);

    pub struct ImportedScanningRhs(pub String);

    #[automatically_derived]
    impl PartialEq<ImportedScanningRhs> for ImportedSelfEqWithOtherRhs {
        fn eq(&self, other: &ImportedScanningRhs) -> bool {
            if self.0.len() != other.0.len() {
                return false;
            }
            for (left, right) in self.0.bytes().zip(other.0.bytes()) {
                if left != right {
                    return false;
                }
            }
            true
        }
    }
}
