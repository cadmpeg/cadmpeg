// SPDX-License-Identifier: Apache-2.0
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::native::NativeRecord;
use cadmpeg_ir::schema::structural::project;
use serde::ser::{SerializeMap, Serializer};
use serde::Serialize;
use serde_json::{Map, Number, Value};
use std::collections::{BTreeMap, HashMap};

pub fn native_record_declared_map_is_bounded(
    ctx: &DecodeContext<'_>,
    source: &NativeRecord,
) -> Result<(), CodecError> {
    drop(project(ctx, source, "native record declared map")?);
    Ok(())
}

pub struct BorrowedMap<T> {
    entries: BTreeMap<String, T>,
}

impl<T: Serialize> Serialize for BorrowedMap<T> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(Some(self.entries.len()))?;
        for (key, value) in &self.entries {
            map.serialize_entry(key, value)?;
        }
        map.end()
    }
}

pub fn generic_borrowed_map_is_bounded(
    ctx: &DecodeContext<'_>,
    source: &BorrowedMap<Vec<u8>>,
) -> Result<(), CodecError> {
    drop(project(ctx, source, "generic borrowed map")?);
    Ok(())
}

pub struct ScanningValue {
    bytes: Vec<u8>,
}

impl Serialize for ScanningValue {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut total = 0_u64;
        for byte in &self.bytes {
            total = total.wrapping_add(u64::from(*byte));
        }
        serializer.serialize_u64(total)
    }
}

pub fn generic_borrowed_map_with_scanning_value_is_unproven(
    ctx: &DecodeContext<'_>,
    source: &BorrowedMap<ScanningValue>,
) -> Result<(), CodecError> {
    drop(project(ctx, source, "generic borrowed map with scanning values")?); // finding: unproven_decode_charge
    Ok(())
}

pub fn json_value_is_bounded(
    ctx: &DecodeContext<'_>,
    source: &Value,
) -> Result<(), CodecError> {
    drop(project(ctx, source, "JSON value children")?);
    Ok(())
}

pub fn json_map_is_bounded(
    ctx: &DecodeContext<'_>,
    source: &Map<String, Value>,
) -> Result<(), CodecError> {
    drop(project(ctx, source, "JSON map entries")?);
    Ok(())
}

pub fn json_number_is_bounded(
    ctx: &DecodeContext<'_>,
    source: &Number,
) -> Result<(), CodecError> {
    drop(project(ctx, source, "JSON number scalar")?);
    Ok(())
}

pub struct LengthFromOtherMap {
    declared: BTreeMap<String, u8>,
    entries: BTreeMap<String, u8>,
}

impl Serialize for LengthFromOtherMap {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(Some(self.declared.len()))?;
        for (key, value) in &self.entries {
            map.serialize_entry(key, value)?;
        }
        map.end()
    }
}

pub fn mismatched_length_source_is_unproven(
    ctx: &DecodeContext<'_>,
    source: &LengthFromOtherMap,
) -> Result<(), CodecError> {
    drop(project(ctx, source, "mismatched map length source")?); // finding: unproven_decode_charge
    Ok(())
}

pub struct NestedMaps {
    declared: BTreeMap<String, u8>,
    emitted: BTreeMap<String, u8>,
}

impl NestedMaps {
    fn declared(&self) -> &BTreeMap<String, u8> {
        &self.declared
    }

    fn emitted(&self) -> &BTreeMap<String, u8> {
        &self.emitted
    }
}

pub struct AccessorMap {
    maps: NestedMaps,
}

impl Serialize for AccessorMap {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(Some(self.maps.declared().len()))?;
        for (key, value) in self.maps.emitted() {
            map.serialize_entry(key, value)?;
        }
        map.end()
    }
}

pub fn mismatched_accessor_sources_are_unproven(
    ctx: &DecodeContext<'_>,
    source: &AccessorMap,
) -> Result<(), CodecError> {
    drop(project(ctx, source, "mismatched accessor map sources")?); // finding: unproven_decode_charge
    Ok(())
}

pub struct DynamicMapCount {
    entries: BTreeMap<String, u8>,
    extra: usize,
}

impl Serialize for DynamicMapCount {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(Some(self.entries.len() + self.extra))?;
        for (key, value) in &self.entries {
            map.serialize_entry(key, value)?;
        }
        map.end()
    }
}

pub fn dynamic_map_count_is_unproven(
    ctx: &DecodeContext<'_>,
    source: &DynamicMapCount,
) -> Result<(), CodecError> {
    drop(project(ctx, source, "dynamic map count")?); // finding: unproven_decode_charge
    Ok(())
}

pub struct UnknownLengthMap {
    entries: BTreeMap<String, u8>,
}

impl Serialize for UnknownLengthMap {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(None)?;
        for (key, value) in &self.entries {
            map.serialize_entry(key, value)?;
        }
        map.end()
    }
}

pub fn unknown_map_count_is_unproven(
    ctx: &DecodeContext<'_>,
    source: &UnknownLengthMap,
) -> Result<(), CodecError> {
    drop(project(ctx, source, "unknown map count")?); // finding: unproven_decode_charge
    Ok(())
}

pub struct PrefixCountMismatch {
    entries: BTreeMap<String, u8>,
    value: u8,
}

impl Serialize for PrefixCountMismatch {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(Some(self.entries.len()))?;
        map.serialize_entry("fixed", &self.value)?;
        for (key, value) in &self.entries {
            map.serialize_entry(key, value)?;
        }
        map.end()
    }
}

pub fn missing_prefix_count_is_unproven(
    ctx: &DecodeContext<'_>,
    source: &PrefixCountMismatch,
) -> Result<(), CodecError> {
    drop(project(ctx, source, "missing prefix map count")?); // finding: unproven_decode_charge
    Ok(())
}

pub struct FilteredMap {
    entries: BTreeMap<String, u8>,
}

impl Serialize for FilteredMap {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(Some(self.entries.len()))?;
        for (key, value) in &self.entries {
            if key.is_empty() {
                continue;
            }
            map.serialize_entry(key, value)?;
        }
        map.end()
    }
}

pub fn filtered_map_loop_is_unproven(
    ctx: &DecodeContext<'_>,
    source: &FilteredMap,
) -> Result<(), CodecError> {
    drop(project(ctx, source, "filtered map loop")?); // finding: unproven_decode_charge
    Ok(())
}

pub struct SwallowedEntryResult {
    entries: BTreeMap<String, u8>,
}

impl Serialize for SwallowedEntryResult {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(Some(self.entries.len()))?;
        for (key, value) in &self.entries {
            let _ = map.serialize_entry(key, value);
        }
        map.end()
    }
}

pub fn swallowed_entry_refusal_is_unproven(
    ctx: &DecodeContext<'_>,
    source: &SwallowedEntryResult,
) -> Result<(), CodecError> {
    drop(project(ctx, source, "swallowed map-entry refusal")?); // finding: unproven_decode_charge
    Ok(())
}

pub struct TruncatedMap {
    entries: BTreeMap<String, u8>,
}

impl Serialize for TruncatedMap {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(Some(self.entries.len()))?;
        for (key, value) in &self.entries {
            map.serialize_entry(key, value)?;
            break;
        }
        map.end()
    }
}

pub fn truncated_map_loop_is_unproven(
    ctx: &DecodeContext<'_>,
    source: &TruncatedMap,
) -> Result<(), CodecError> {
    drop(project(ctx, source, "truncated map loop")?); // finding: unproven_decode_charge
    Ok(())
}

pub struct DifferentMapState {
    entries: BTreeMap<String, u8>,
}

impl Serialize for DifferentMapState {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let alternate = unsafe { std::ptr::read(&serializer) };
        let mut first = serializer.serialize_map(Some(self.entries.len()))?;
        let mut second = alternate.serialize_map(Some(0))?;
        for (key, value) in &self.entries {
            second.serialize_entry(key, value)?;
        }
        first.end()?;
        second.end()
    }
}

pub fn different_map_state_is_unproven(
    ctx: &DecodeContext<'_>,
    source: &DifferentMapState,
) -> Result<(), CodecError> {
    drop(project(ctx, source, "different map state")?); // finding: unproven_decode_charge
    Ok(())
}

pub struct LookupPerEntry {
    entries: BTreeMap<String, u8>,
}

impl Serialize for LookupPerEntry {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(Some(self.entries.len()))?;
        for key in self.entries.keys() {
            if let Some(value) = self.entries.get(key) {
                map.serialize_entry(key, value)?;
            }
        }
        map.end()
    }
}

pub fn per_entry_map_lookup_is_unproven(
    ctx: &DecodeContext<'_>,
    source: &LookupPerEntry,
) -> Result<(), CodecError> {
    drop(project(ctx, source, "per entry map lookup")?); // finding: unproven_decode_charge
    Ok(())
}

pub struct AllocatingMap {
    entries: BTreeMap<String, u8>,
}

impl Serialize for AllocatingMap {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let cloned = self.entries.clone();
        let mut map = serializer.serialize_map(Some(cloned.len()))?;
        for (key, value) in &cloned {
            map.serialize_entry(key, value)?;
        }
        map.end()
    }
}

pub fn cloned_source_map_is_unproven(
    ctx: &DecodeContext<'_>,
    source: &AllocatingMap,
) -> Result<(), CodecError> {
    drop(project(ctx, source, "cloned source map")?); // finding: unproven_decode_charge
    Ok(())
}

pub struct DropScanningMap {
    entries: BTreeMap<String, u8>,
}

struct DropScanningGuard<'a> {
    entries: &'a BTreeMap<String, u8>,
    visited_bytes: usize,
}

impl Drop for DropScanningGuard<'_> {
    fn drop(&mut self) {
        for key in self.entries.keys() {
            self.visited_bytes += key.len();
        }
    }
}

impl Serialize for DropScanningMap {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let _guard = DropScanningGuard {
            entries: &self.entries,
            visited_bytes: 0,
        };
        let mut map = serializer.serialize_map(Some(self.entries.len()))?;
        for (key, value) in &self.entries {
            map.serialize_entry(key, value)?;
        }
        map.end()
    }
}

pub fn dropping_source_work_is_unproven(
    ctx: &DecodeContext<'_>,
    source: &DropScanningMap,
) -> Result<(), CodecError> {
    drop(project(ctx, source, "map destructor scan")?); // finding: unproven_decode_charge
    Ok(())
}

pub struct HashMapSource {
    entries: HashMap<String, u8>,
}

impl Serialize for HashMapSource {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(Some(self.entries.len()))?;
        for (key, value) in &self.entries {
            map.serialize_entry(key, value)?;
        }
        map.end()
    }
}

pub fn hash_map_capacity_scan_is_unproven(
    ctx: &DecodeContext<'_>,
    source: &HashMapSource,
) -> Result<(), CodecError> {
    drop(project(ctx, source, "hash map capacity scan")?); // finding: unproven_decode_charge
    Ok(())
}
