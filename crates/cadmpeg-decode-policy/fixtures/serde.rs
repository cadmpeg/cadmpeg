// SPDX-License-Identifier: Apache-2.0
use cadmpeg_core::decode::DecodeContext;
use serde::Deserialize;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::hash::{BuildHasher, Hash, Hasher};
use std::marker::PhantomData;
#[derive(Clone, PartialEq, serde::Serialize, Deserialize)]
pub struct Owned {
    pub values: Vec<String>,
}
pub fn parse(_ctx: &DecodeContext, text: &str) {
    Local(text).serialize();
    let _record = serde_json::from_str::<Owned>(text); // finding: unproven_decode_charge
    let mut deserializer = serde_json::Deserializer::from_str(text);
    let _record = Owned::deserialize(&mut deserializer); // finding: unproven_decode_charge
}

trait Serialize {
    fn serialize(&self);
}
struct Local<'a>(&'a str);
impl Serialize for Local<'_> {
    fn serialize(&self) {
        let _copy = self.0.to_owned(); // finding: uncharged_decode_allocation, uncharged_decode_work
    }
}

struct DecodeText<'a>(&'a str);
impl serde::Serialize for DecodeText<'_> {
    fn serialize<S: serde::Serializer>(&self, _serializer: S) -> Result<S::Ok, S::Error> {
        for byte in self.0.as_bytes() {
            std::hint::black_box(byte);
        }
        let _deferred: fn(&[u8]) = |bytes| {
            for byte in bytes {
                std::hint::black_box(byte);
            }
        };
        Err(serde::ser::Error::custom("fixed"))
    }
}
pub fn decode_serialization<S: serde::Serializer>(ctx: &DecodeContext, text: &str, serializer: S) {
    let _context = ctx;
    let _record = serde::Serialize::serialize(&DecodeText(text), serializer); // finding: unproven_decode_charge
}

struct WriteText<'a>(&'a str);
impl serde::Serialize for WriteText<'_> {
    fn serialize<S: serde::Serializer>(&self, _serializer: S) -> Result<S::Ok, S::Error> {
        for byte in self.0.as_bytes() {
            std::hint::black_box(byte);
        }
        let _deferred: fn(&[u8]) = |bytes| {
            for byte in bytes {
                std::hint::black_box(byte);
            }
        };
        Err(serde::ser::Error::custom("fixed"))
    }
}
pub fn serialization_only<S: serde::Serializer>(text: &str, serializer: S) {
    let _record = serde::Serialize::serialize(&WriteText(text), serializer);
}

struct Custom;
impl<'de> Deserialize<'de> for Custom {
    fn deserialize<D: serde::Deserializer<'de>>(_parser: D) -> Result<Self, D::Error> {
        let input = String::new();
        for byte in input.as_bytes() {
            std::hint::black_box(byte);
        }
        Err(serde::de::Error::custom("fixed"))
    }
}
#[derive(Deserialize)]
struct ContainsCustom {
    values: Vec<Custom>,
}
pub fn custom_decode(_ctx: &DecodeContext, text: &str) {
    let _value = serde_json::from_str::<ContainsCustom>(text); // finding: unproven_decode_charge
}

pub fn admitted_decode(ctx: &DecodeContext, text: &str) {
    let _derived = ctx.parse_json::<Owned>(text, "derived");
    let _custom = ctx.parse_json::<Custom>(text, "custom"); // finding: unproven_decode_charge
    let _contained = ctx.parse_json::<ContainsCustom>(text, "contained custom");
    // finding: unproven_decode_charge
}

#[derive(Deserialize)]
#[serde(try_from = "String")]
struct FromWire(String);

impl TryFrom<String> for FromWire {
    type Error = std::convert::Infallible;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Ok(Self(value))
    }
}

pub fn try_from_decode(ctx: &DecodeContext, text: &str) {
    let _record = ctx.parse_json::<FromWire>(text, "try from"); // finding: unproven_decode_charge
}

fn deserialize_text<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: serde::Deserializer<'de>,
{
    String::deserialize(deserializer)
}

#[derive(Deserialize)]
struct CustomField {
    #[serde(deserialize_with = "deserialize_text")]
    value: String,
}

pub fn custom_field_decode(ctx: &DecodeContext, text: &str) {
    let _record = ctx.parse_json::<CustomField>(text, "custom field"); // finding: unproven_decode_charge
}

#[derive(Deserialize)]
enum CustomVariant {
    Plain,
    #[serde(skip_deserializing)]
    Skipped,
}

pub fn custom_variant_decode(ctx: &DecodeContext, text: &str) {
    let _record = ctx.parse_json::<CustomVariant>(text, "custom variant"); // finding: unproven_decode_charge
}

#[derive(Deserialize)]
#[serde(untagged)]
enum UntaggedValue {
    Text(String),
    Number(u64),
}

pub fn untagged_decode(ctx: &DecodeContext, text: &str) {
    let _record = ctx.parse_json::<UntaggedValue>(text, "untagged value"); // finding: unproven_decode_charge
}

#[derive(Deserialize)]
enum PartlyUntaggedValue {
    Tagged { value: u64 },
    #[serde(untagged)]
    Fallback(String),
}

pub fn partly_untagged_decode(ctx: &DecodeContext, text: &str) {
    let _record = ctx.parse_json::<PartlyUntaggedValue>(text, "partly untagged value"); // finding: unproven_decode_charge
}

// Rust rejects a self-recursive generic field that expands T under Vec with
// E0320 before the checker can inspect it. This finite chain keeps the
// Deserialize obligations shallow while its distinct derived types reach the
// checker's type-proof recursion limit.
macro_rules! expanding_decode_chain {
    ($($current:ident => $next:ident,)* ; $last:ident) => {
        $(
            #[derive(Deserialize)]
            struct $current<T> {
                marker: PhantomData<$next<T>>,
            }
        )+
        #[derive(Deserialize)]
        struct $last<T> {
            marker: PhantomData<T>,
        }
    };
}

expanding_decode_chain! {
    ExpandDecode00 => ExpandDecode01,
    ExpandDecode01 => ExpandDecode02,
    ExpandDecode02 => ExpandDecode03,
    ExpandDecode03 => ExpandDecode04,
    ExpandDecode04 => ExpandDecode05,
    ExpandDecode05 => ExpandDecode06,
    ExpandDecode06 => ExpandDecode07,
    ExpandDecode07 => ExpandDecode08,
    ExpandDecode08 => ExpandDecode09,
    ExpandDecode09 => ExpandDecode10,
    ExpandDecode10 => ExpandDecode11,
    ExpandDecode11 => ExpandDecode12,
    ExpandDecode12 => ExpandDecode13,
    ExpandDecode13 => ExpandDecode14,
    ExpandDecode14 => ExpandDecode15,
    ExpandDecode15 => ExpandDecode16,
    ExpandDecode16 => ExpandDecode17,
    ExpandDecode17 => ExpandDecode18,
    ExpandDecode18 => ExpandDecode19,
    ExpandDecode19 => ExpandDecode20,
    ExpandDecode20 => ExpandDecode21,
    ExpandDecode21 => ExpandDecode22,
    ExpandDecode22 => ExpandDecode23,
    ExpandDecode23 => ExpandDecode24,
    ExpandDecode24 => ExpandDecode25,
    ExpandDecode25 => ExpandDecode26,
    ExpandDecode26 => ExpandDecode27,
    ExpandDecode27 => ExpandDecode28,
    ExpandDecode28 => ExpandDecode29,
    ExpandDecode29 => ExpandDecode30,
    ExpandDecode30 => ExpandDecode31,
    ExpandDecode31 => ExpandDecode32,
    ExpandDecode32 => ExpandDecode33,
    ExpandDecode33 => ExpandDecode34,
    ExpandDecode34 => ExpandDecode35,
    ExpandDecode35 => ExpandDecode36,
    ExpandDecode36 => ExpandDecode37,
    ExpandDecode37 => ExpandDecode38,
    ExpandDecode38 => ExpandDecode39,
    ExpandDecode39 => ExpandDecode40,
    ExpandDecode40 => ExpandDecode41,
    ExpandDecode41 => ExpandDecode42,
    ExpandDecode42 => ExpandDecode43,
    ExpandDecode43 => ExpandDecode44,
    ExpandDecode44 => ExpandDecode45,
    ExpandDecode45 => ExpandDecode46,
    ExpandDecode46 => ExpandDecode47,
    ExpandDecode47 => ExpandDecode48,
    ExpandDecode48 => ExpandDecode49,
    ExpandDecode49 => ExpandDecode50,
    ExpandDecode50 => ExpandDecode51,
    ExpandDecode51 => ExpandDecode52,
    ExpandDecode52 => ExpandDecode53,
    ExpandDecode53 => ExpandDecode54,
    ExpandDecode54 => ExpandDecode55,
    ExpandDecode55 => ExpandDecode56,
    ExpandDecode56 => ExpandDecode57,
    ExpandDecode57 => ExpandDecode58,
    ExpandDecode58 => ExpandDecode59,
    ExpandDecode59 => ExpandDecode60,
    ExpandDecode60 => ExpandDecode61,
    ExpandDecode61 => ExpandDecode62,
    ExpandDecode62 => ExpandDecode63,
    ExpandDecode63 => ExpandDecode64,
    ExpandDecode64 => ExpandDecode65,
    ExpandDecode65 => ExpandDecode66,
    ExpandDecode66 => ExpandDecode67,
    ExpandDecode67 => ExpandDecode68,
    ExpandDecode68 => ExpandDecode69,
    ExpandDecode69 => ExpandDecode70,
    ; ExpandDecode70
}

pub fn expanding_derived_decode(ctx: &DecodeContext, text: &str) {
    let _record = ctx.parse_json::<ExpandDecode00<u32>>(text, "expanding derived source"); // finding: unproven_decode_charge
}

struct MarkerOnly(String);

#[automatically_derived]
impl<'de> Deserialize<'de> for MarkerOnly {
    fn deserialize<D: serde::Deserializer<'de>>(parser: D) -> Result<Self, D::Error> {
        let value = String::deserialize(parser)?;
        for byte in value.as_bytes() {
            std::hint::black_box(byte);
        }
        Ok(Self(value))
    }
}

pub fn marker_only_decode(ctx: &DecodeContext, text: &str) {
    let _record = ctx.parse_json::<MarkerOnly>(text, "marker-only derived marker"); // finding: unproven_decode_charge
}

pub fn imported_try_from_decode(ctx: &DecodeContext, text: &str) {
    let _record = ctx.parse_json::<cadmpeg_ir::assets::Asset>(text, "imported try from"); // finding: unproven_decode_charge
}

pub fn imported_field_hook_decode(ctx: &DecodeContext, text: &str) {
    let _record = ctx.parse_json::<cadmpeg_ir::appearance::Appearance>(text, "imported field hook"); // finding: unproven_decode_charge
}

#[derive(Deserialize)]
pub struct StandardTextCollections {
    pub hash_map: HashMap<String, u32>,
    pub hash_set: HashSet<u32>,
    pub btree_map: BTreeMap<String, u32>,
    pub btree_set: BTreeSet<String>,
}

pub fn standard_text_collection_decode(ctx: &DecodeContext, text: &str) {
    let _collections = ctx.parse_json::<StandardTextCollections>(text, "standard collections");
}

#[derive(Deserialize)]
#[serde(transparent)]
struct QuadraticHashKey(String);

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

pub fn custom_hash_key_decode(ctx: &DecodeContext, text: &str) {
    let _record = ctx.parse_json::<HashMap<QuadraticHashKey, u32>>(text, "custom hash key"); // finding: unproven_decode_charge
}

pub fn custom_hash_set_key_decode(ctx: &DecodeContext, text: &str) {
    let _record = ctx.parse_json::<HashSet<QuadraticHashKey>>(text, "custom hash set key"); // finding: unproven_decode_charge
}

#[derive(Default)]
struct QuadraticBuildHasher;

#[derive(Default)]
struct QuadraticHasher(u64);

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

pub fn custom_hash_builder_decode(ctx: &DecodeContext, text: &str) {
    let _record = ctx.parse_json::<HashMap<String, u32, QuadraticBuildHasher>>( // finding: unproven_decode_charge
        text,
        "custom hash builder",
    );
}

#[derive(Deserialize)]
#[serde(transparent)]
struct QuadraticOrdKey(String);

impl PartialEq for QuadraticOrdKey {
    fn eq(&self, other: &Self) -> bool {
        self.0 == other.0
    }
}

impl Eq for QuadraticOrdKey {}

impl PartialOrd for QuadraticOrdKey {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for QuadraticOrdKey {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        let left = self.0.as_bytes();
        let right = other.0.as_bytes();
        let mut order = std::cmp::Ordering::Equal;
        for (left_index, left_byte) in left.iter().enumerate() {
            for (right_index, right_byte) in right.iter().enumerate() {
                if left_index == right_index && order == std::cmp::Ordering::Equal {
                    order = left_byte.cmp(right_byte);
                } else {
                    std::hint::black_box(right_byte);
                }
            }
        }
        if order == std::cmp::Ordering::Equal {
            left.len().cmp(&right.len())
        } else {
            order
        }
    }
}

pub fn custom_btree_map_key_decode(ctx: &DecodeContext, text: &str) {
    let _record = ctx.parse_json::<BTreeMap<QuadraticOrdKey, u32>>(text, "custom ordered key"); // finding: unproven_decode_charge
}

pub fn custom_btree_set_key_decode(ctx: &DecodeContext, text: &str) {
    let _record = ctx.parse_json::<BTreeSet<QuadraticOrdKey>>(text, "custom ordered key"); // finding: unproven_decode_charge
}
