// SPDX-License-Identifier: Apache-2.0
use cadmpeg_core::decode::cost::DecodeCost;
use cadmpeg_core::decode::{u64_from_index, DecodeContext};
use cadmpeg_core::dialect::DialectId;
use cadmpeg_core::CodecError;
use std::borrow::{Borrow, Cow, ToOwned};
use std::collections::{HashMap, HashSet};
use std::hash::{BuildHasher, BuildHasherDefault, DefaultHasher, Hash, Hasher};

#[derive(Hash, PartialEq, Eq)]
pub struct DerivedCodeKey(pub u32);

impl DecodeCost for DerivedCodeKey {
    const FIXED_BYTES: Option<u64> = Some(4);

    fn decode_cost(
        &self,
        _ctx: &DecodeContext<'_>,
        _operation: &'static str,
    ) -> Result<u64, CodecError> {
        Ok(u64_from_index(std::mem::size_of::<u32>()))
    }
}

pub fn scalar_random_state(
    ctx: &DecodeContext<'_>,
    left: &HashSet<u32>,
    right: &HashSet<u32>,
) -> Result<(), CodecError> {
    let _equal = ctx.equal_hash_set(left, right, "scalar hash-set lookup")?;
    Ok(())
}

pub fn string_random_state(
    ctx: &DecodeContext<'_>,
    left: &HashSet<String>,
    right: &HashSet<String>,
) -> Result<(), CodecError> {
    let _equal = ctx.equal_hash_set(left, right, "string hash-set lookup")?;
    Ok(())
}

pub fn derived_key_random_state(
    ctx: &DecodeContext<'_>,
    left: &HashSet<DerivedCodeKey>,
    right: &HashSet<DerivedCodeKey>,
) -> Result<(), CodecError> {
    let _equal = ctx.equal_hash_set(left, right, "derived hash-set key")?;
    Ok(())
}

#[derive(Hash, PartialEq, Eq)]
pub struct RecursiveKey(pub Option<Box<RecursiveKey>>);

impl DecodeCost for RecursiveKey {
    fn decode_cost(
        &self,
        _ctx: &DecodeContext<'_>,
        _operation: &'static str,
    ) -> Result<u64, CodecError> {
        Ok(1)
    }
}

pub fn recursive_key(
    ctx: &DecodeContext<'_>,
    left: &HashSet<RecursiveKey>,
    right: &HashSet<RecursiveKey>,
) -> Result<(), CodecError> {
    let _equal = ctx.equal_hash_set(left, right, "recursive key callbacks")?; // finding: unproven_decode_charge
    Ok(())
}

pub fn imported_dialect_id(
    ctx: &DecodeContext<'_>,
    left: &HashSet<DialectId>,
    right: &HashSet<DialectId>,
) -> Result<(), CodecError> {
    let _equal = ctx.equal_hash_set(left, right, "imported derived hash-set key")?;
    Ok(())
}

pub fn standard_default_hasher(
    ctx: &DecodeContext<'_>,
    left: &HashSet<u32, BuildHasherDefault<DefaultHasher>>,
    right: &HashSet<u32, BuildHasherDefault<DefaultHasher>>,
) -> Result<(), CodecError> {
    let _equal = ctx.equal_hash_set(left, right, "standard default hasher")?;
    Ok(())
}

pub struct QuadraticHashKey(pub String);

impl PartialEq for QuadraticHashKey {
    fn eq(&self, other: &Self) -> bool {
        self.0 == other.0
    }
}

impl Eq for QuadraticHashKey {}

impl Hash for QuadraticHashKey {
    fn hash<H: Hasher>(&self, state: &mut H) {
        let bytes = self.0.as_bytes();
        for _ in bytes {
            for byte in bytes {
                state.write_u8(*byte);
            }
        }
    }
}

impl DecodeCost for QuadraticHashKey {
    fn decode_cost(
        &self,
        ctx: &DecodeContext<'_>,
        operation: &'static str,
    ) -> Result<u64, CodecError> {
        self.0.decode_cost(ctx, operation)
    }
}

pub fn quadratic_hash_key(
    ctx: &DecodeContext<'_>,
    left: &HashSet<QuadraticHashKey>,
    right: &HashSet<QuadraticHashKey>,
) -> Result<(), CodecError> {
    let _equal = ctx.equal_hash_set(left, right, "quadratic hash callback")?; // finding: unproven_decode_charge
    Ok(())
}

#[derive(Hash)]
pub struct QuadraticEqKey(pub String);

impl PartialEq for QuadraticEqKey {
    fn eq(&self, other: &Self) -> bool {
        let left = self.0.as_bytes();
        let right = other.0.as_bytes();
        if left.len() != right.len() {
            return false;
        }
        for (left_index, left_byte) in left.iter().enumerate() {
            for (right_index, right_byte) in right.iter().enumerate() {
                if left_index == right_index && left_byte != right_byte {
                    return false;
                }
            }
        }
        true
    }
}

impl Eq for QuadraticEqKey {}

impl DecodeCost for QuadraticEqKey {
    fn decode_cost(
        &self,
        ctx: &DecodeContext<'_>,
        operation: &'static str,
    ) -> Result<u64, CodecError> {
        self.0.decode_cost(ctx, operation)
    }
}

pub fn quadratic_partial_eq_key(
    ctx: &DecodeContext<'_>,
    left: &HashSet<QuadraticEqKey>,
    right: &HashSet<QuadraticEqKey>,
) -> Result<(), CodecError> {
    let _equal = ctx.equal_hash_set(left, right, "quadratic equality callback")?; // finding: unproven_decode_charge
    Ok(())
}

pub struct MarkerOnlyKey(pub String);

#[automatically_derived]
impl Hash for MarkerOnlyKey {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.0.hash(state);
    }
}

#[automatically_derived]
impl PartialEq for MarkerOnlyKey {
    fn eq(&self, other: &Self) -> bool {
        self.0 == other.0
    }
}

#[automatically_derived]
impl Eq for MarkerOnlyKey {}

impl DecodeCost for MarkerOnlyKey {
    fn decode_cost(
        &self,
        ctx: &DecodeContext<'_>,
        operation: &'static str,
    ) -> Result<u64, CodecError> {
        self.0.decode_cost(ctx, operation)
    }
}

pub fn marker_only_key(
    ctx: &DecodeContext<'_>,
    left: &HashSet<MarkerOnlyKey>,
    right: &HashSet<MarkerOnlyKey>,
) -> Result<(), CodecError> {
    let _equal = ctx.equal_hash_set(left, right, "manually marked derive")?; // finding: unproven_decode_charge
    Ok(())
}

#[derive(Default)]
pub struct QuadraticBuildHasher;

#[derive(Default)]
pub struct QuadraticHasher(u64);

impl BuildHasher for QuadraticBuildHasher {
    type Hasher = QuadraticHasher;

    fn build_hasher(&self) -> Self::Hasher {
        QuadraticHasher::default()
    }
}

impl Hasher for QuadraticHasher {
    fn finish(&self) -> u64 {
        self.0
    }

    fn write(&mut self, bytes: &[u8]) {
        for _ in bytes {
            for byte in bytes {
                self.0 = self.0.wrapping_mul(31).wrapping_add(u64::from(*byte));
            }
        }
    }
}

pub fn quadratic_hasher(
    ctx: &DecodeContext<'_>,
    left: &HashSet<String, QuadraticBuildHasher>,
    right: &HashSet<String, QuadraticBuildHasher>,
) -> Result<(), CodecError> {
    let _equal = ctx.equal_hash_set(left, right, "quadratic custom hasher")?; // finding: unproven_decode_charge
    Ok(())
}

pub fn unused_left_builder(
    ctx: &DecodeContext<'_>,
    left: &HashSet<String, QuadraticBuildHasher>,
    right: &HashSet<String>,
) -> Result<(), CodecError> {
    let _equal = ctx.equal_hash_set(left, right, "left builder is not used")?;
    Ok(())
}

pub fn bounded_borrowed_hash_lookup_routes(
    ctx: &DecodeContext<'_>,
    map: &mut HashMap<String, u8>,
    set: &mut HashSet<String>,
    query: &str,
) -> Result<(), CodecError> {
    let _get = ctx.get_hash_map(&*map, query, "string map get")?;
    let _get_mut = ctx.get_mut_hash_map(map, query, "string map get mut")?;
    let _contains_key = ctx.contains_key_hash_map(map, query, "string map contains")?;
    let _removed = ctx.remove_hash_map(map, query, "string map remove")?;
    let _key_value = ctx.get_key_value_hash_map(map, query, "string map get key value")?;
    let _entry = ctx.remove_entry_hash_map(map, query, "string map remove entry")?;
    let _contains = ctx.contains_hash_set(set, query, "string set contains")?;
    let _stored = ctx.get_hash_set(set, query, "string set get")?;
    let _removed = ctx.remove_hash_set(set, query, "string set remove")?;
    Ok(())
}

pub fn cow_str_borrowed_lookup(
    ctx: &DecodeContext<'_>,
    values: &HashSet<Cow<'static, str>>,
    query: &str,
) -> Result<(), CodecError> {
    let _stored = ctx.get_hash_set(values, query, "Cow<str> borrowed lookup")?;
    Ok(())
}

pub fn derived_identity_map_lookup(
    ctx: &DecodeContext<'_>,
    values: &HashMap<DerivedCodeKey, u8>,
    query: &DerivedCodeKey,
) -> Result<(), CodecError> {
    let _stored = ctx.get_key_value_hash_map(values, query, "derived identity lookup")?;
    Ok(())
}

pub fn reference_key_borrowed_lookup(
    ctx: &DecodeContext<'_>,
    map: &HashMap<&str, u8>,
    set: &HashSet<&str>,
    query: &str,
) -> Result<(), CodecError> {
    let _value = ctx.get_hash_map(map, query, "borrowed string map")?;
    let _stored = ctx.get_hash_set(set, query, "borrowed string set")?;
    Ok(())
}

pub fn mutable_reference_key_borrowed_lookup(
    ctx: &DecodeContext<'_>,
    set: &HashSet<&mut str>,
    query: &str,
) -> Result<(), CodecError> {
    let _stored = ctx.get_hash_set(set, query, "mutable borrowed string set")?;
    Ok(())
}

#[derive(Hash, PartialEq, Eq)]
pub struct LocalKey {
    code: u8,
    decoy: String,
}

impl DecodeCost for LocalKey {
    fn decode_cost(
        &self,
        _ctx: &DecodeContext<'_>,
        _operation: &'static str,
    ) -> Result<u64, CodecError> {
        Ok(u64_from_index(self.decoy.len()) + 1)
    }
}

pub struct ScanningOwnedKey(LocalKey);

impl Borrow<LocalKey> for ScanningOwnedKey {
    fn borrow(&self) -> &LocalKey {
        for _byte in self.0.decoy.bytes() {}
        &self.0
    }
}

impl ToOwned for LocalKey {
    type Owned = ScanningOwnedKey;

    fn to_owned(&self) -> Self::Owned {
        ScanningOwnedKey(LocalKey {
            code: self.code,
            decoy: self.decoy.clone(),
        })
    }
}

pub fn cow_local_stored_borrow_scan(
    ctx: &DecodeContext<'_>,
    values: &HashSet<Cow<'static, LocalKey>>,
    query: &LocalKey,
) -> Result<(), CodecError> {
    let _stored = ctx.get_hash_set(values, query, "Cow<LocalKey> stored Borrow")?; // finding: unproven_decode_charge
    Ok(())
}

pub fn cow_local_raw_stored_borrow_scan(
    _ctx: &DecodeContext<'_>,
    values: &HashSet<Cow<'static, LocalKey>>,
    query: &LocalKey,
) -> bool {
    values.contains(query) // finding: unproven_decode_charge
}
