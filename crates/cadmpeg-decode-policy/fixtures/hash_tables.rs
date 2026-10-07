// SPDX-License-Identifier: Apache-2.0
use cadmpeg_core::decode::cost::DecodeCost;
use cadmpeg_core::decode::{u64_from_index, DecodeContext};
use cadmpeg_core::CodecError;
use std::collections::{HashMap, HashSet};
use std::hash::{BuildHasherDefault, DefaultHasher, Hash, Hasher};

#[derive(PartialEq, Eq)]
pub struct CustomHashKey(String);

impl Hash for CustomHashKey {
    fn hash<H: Hasher>(&self, state: &mut H) {
        for byte in self.0.bytes() {
            state.write_u8(byte);
            state.write_u8(byte);
        }
    }
}

impl DecodeCost for CustomHashKey {
    fn decode_cost(
        &self,
        _ctx: &DecodeContext<'_>,
        _operation: &'static str,
    ) -> Result<u64, CodecError> {
        Ok(u64_from_index(self.0.len()))
    }
}

pub fn custom_hash_key_lookup(
    ctx: &DecodeContext<'_>,
    values: &HashSet<CustomHashKey>,
    query: &CustomHashKey,
) -> Result<bool, CodecError> {
    ctx.contains_hash_set(values, query, "custom hash lookup") // finding: unproven_decode_charge
}

pub fn fixed_key_hasher_lookup(
    _ctx: &DecodeContext<'_>,
    values: &HashSet<u32, BuildHasherDefault<DefaultHasher>>,
    query: &u32,
) -> bool {
    values.contains(query) // finding: unproven_decode_charge
}

pub fn custom_hash_key_insertion(
    _ctx: &DecodeContext<'_>,
    values: &mut HashSet<CustomHashKey>,
    key: CustomHashKey,
) -> bool {
    values.insert(key) // finding: uncharged_decode_allocation, unproven_decode_charge
}

pub fn traversals(
    _ctx: &DecodeContext<'_>,
    map: &mut HashMap<String, u8>,
    set: &mut HashSet<u32>,
    other: &HashSet<u32>,
    optional: &Option<HashSet<u32>>,
) -> usize {
    let mut total = 0;
    for _entry in map.iter() { // finding: uncharged_decode_work, uncharged_decode_work
        total += 1;
    }
    for _entry in &*map { // finding: uncharged_decode_work
        total += 1;
    }
    total += map.keys().count(); // finding: uncharged_decode_work, unproven_decode_charge
    for value in map.values_mut() { // finding: uncharged_decode_work, uncharged_decode_work
        *value = 0;
    }
    total += set.iter().count(); // finding: uncharged_decode_work, unproven_decode_charge
    set.retain(|value| *value > 1); // finding: uncharged_decode_work
    total += usize::from(set.is_subset(other)); // finding: uncharged_decode_work
    total += usize::from(*set == *other); // finding: uncharged_decode_work
    total += usize::from(optional.as_ref() == Some(other)); // finding: uncharged_decode_work
    let copy = map.clone(); // finding: uncharged_decode_allocation, uncharged_decode_work
    total += copy.len();
    total += map.drain().count(); // finding: uncharged_decode_work, unproven_decode_charge
    total
}

pub fn generic_builder_lookup<S: std::hash::BuildHasher>(
    _ctx: &DecodeContext<'_>,
    values: &HashSet<u32, S>,
    query: &u32,
) -> bool {
    values.contains(query)
}

pub fn fixed_key_hasher_instance(
    ctx: &DecodeContext<'_>,
    values: &HashSet<u32, BuildHasherDefault<DefaultHasher>>,
) -> bool {
    generic_builder_lookup(ctx, values, &1) // finding: unproven_decode_charge
}

macro_rules! marked_hash {
    ($name:ident) => {
        #[derive(PartialEq, Eq)]
        pub struct $name(String);

        #[automatically_derived]
        impl Hash for $name {
            fn hash<H: Hasher>(&self, state: &mut H) {
                state.write_u8(1);
            }
        }

        impl DecodeCost for $name {
            fn decode_cost(
                &self,
                _ctx: &DecodeContext<'_>,
                _operation: &'static str,
            ) -> Result<u64, CodecError> {
                Ok(u64_from_index(self.0.len()))
            }
        }
    };
}

marked_hash!(MacroMarkedKey);

pub fn macro_marked_key_lookup(
    ctx: &DecodeContext<'_>,
    values: &HashSet<MacroMarkedKey>,
    query: &MacroMarkedKey,
) -> Result<bool, CodecError> {
    ctx.contains_hash_set(values, query, "macro marked lookup") // finding: unproven_decode_charge
}

pub fn keyed_indexes(
    _ctx: &DecodeContext<'_>,
    map: &HashMap<String, u8>,
    tree: &std::collections::BTreeMap<String, u8>,
    key: &str,
) -> u8 {
    map[key] + tree[key] // finding: uncharged_decode_work, uncharged_decode_work
}

pub fn set_operators(
    _ctx: &DecodeContext<'_>,
    left: &HashSet<u32>,
    right: &HashSet<u32>,
) -> usize {
    (left | right).len() // finding: uncharged_decode_work
}

#[derive(PartialEq, Eq, PartialOrd)]
pub struct CustomOrderKey(String);

impl Ord for CustomOrderKey {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.0.bytes().rev().cmp(other.0.bytes().rev())
    }
}

impl DecodeCost for CustomOrderKey {
    fn decode_cost(
        &self,
        _ctx: &DecodeContext<'_>,
        _operation: &'static str,
    ) -> Result<u64, CodecError> {
        Ok(u64_from_index(self.0.len()))
    }
}

#[derive(PartialEq, Eq, PartialOrd, Ord)]
pub struct DerivedOrderKey(u32);

impl DecodeCost for DerivedOrderKey {
    const FIXED_BYTES: Option<u64> = Some(4);
    fn decode_cost(
        &self,
        _ctx: &DecodeContext<'_>,
        _operation: &'static str,
    ) -> Result<u64, CodecError> {
        Ok(4)
    }
}

pub fn custom_order_insertion(
    ctx: &DecodeContext<'_>,
    values: &mut std::collections::BTreeSet<CustomOrderKey>,
    key: CustomOrderKey,
) -> Result<bool, CodecError> {
    ctx.insert_btree_set(values, key, "custom order insertion") // finding: unproven_decode_charge
}

pub fn derived_order_insertion(
    ctx: &DecodeContext<'_>,
    values: &mut std::collections::BTreeSet<DerivedOrderKey>,
    key: DerivedOrderKey,
) -> Result<bool, CodecError> {
    ctx.insert_btree_set(values, key, "derived order insertion")
}

pub fn removals(
    _ctx: &DecodeContext<'_>,
    map: &mut HashMap<String, u8>,
    set: &mut HashSet<u32>,
    key: String,
) -> usize {
    let mut removed = usize::from(map.remove("a").is_some()); // finding: uncharged_decode_work
    removed += usize::from(set.take(&1).is_some()); // finding: uncharged_decode_work
    if let std::collections::hash_map::Entry::Occupied(entry) = map.entry(key) { // finding: uncharged_decode_work
        removed += usize::from(entry.remove()); // finding: uncharged_decode_work
    }
    removed
}
