// SPDX-License-Identifier: Apache-2.0
use cadmpeg_core::decode::DecodeContext;

#[derive(serde::Deserialize)]
enum HeavyEnum {
    Empty,
    Large([[u64; 32]; 32]),
}

#[derive(serde::Deserialize)]
struct OrdinaryRecord {
    name: String,
    values: Vec<u8>,
}

#[derive(serde::Deserialize)]
struct RecursiveRecord {
    value: u8,
    child: Option<Box<RecursiveRecord>>,
}

#[derive(serde::Deserialize)]
struct NonConsumingLoop(Option<Box<NonConsumingLoop>>);

#[derive(serde::Deserialize)]
#[serde(transparent)]
struct TransparentLoop(Option<Box<TransparentLoop>>);

#[derive(serde::Deserialize)]
struct WideRecord {
    ordinal: u64,
    name: String,
    path: String,
    children: Vec<u64>,
}

#[derive(serde::Deserialize)]
struct WideOptionalRecord {
    ordinal: Option<u64>,
    name: Option<String>,
    path: Option<String>,
    children: Option<Vec<u64>>,
}

pub fn bounded_targets(ctx: &DecodeContext, text: &str) {
    let _values = ctx.parse_json::<Vec<u8>>(text, "scalar sequence");
    let _records = ctx.parse_json::<Vec<OrdinaryRecord>>(text, "records");
    let _map = ctx.parse_json::<std::collections::HashMap<String, OrdinaryRecord>>(
        text,
        "record map",
    );
    let _sets = ctx.parse_json::<StandardSets>(text, "array-backed sets");
    let _recursive = ctx.parse_json::<Vec<RecursiveRecord>>(text, "recursive records");
}

#[derive(serde::Deserialize)]
struct StandardSets {
    hash: std::collections::HashSet<u32>,
    ordered: std::collections::BTreeSet<String>,
}

pub fn oversized_inline_variant(ctx: &DecodeContext, text: &str) {
    let _values = ctx.parse_json::<Vec<HeavyEnum>>(text, "large inline enum"); // finding: unproven_decode_charge
}

pub fn non_consuming_recursion(ctx: &DecodeContext, text: &str) {
    let _value = ctx.parse_json::<NonConsumingLoop>(text, "newtype recursion"); // finding: unproven_decode_charge
    let _transparent = ctx.parse_json::<TransparentLoop>(text, "transparent recursion"); // finding: unproven_decode_charge
}

pub fn wide_records(ctx: &DecodeContext, text: &str) {
    // Each required field is its own JSON node, so its member allowance covers the record.
    let _values = ctx.parse_json::<Vec<WideRecord>>(text, "wide records");
    // Optional fields may be absent, leaving one node for a record wider than one slot.
    let _values = ctx.parse_json::<Vec<WideOptionalRecord>>(text, "optional fields"); // finding: unproven_decode_charge
}

#[derive(serde::Deserialize)]
#[serde(tag = "kind")]
enum InternallyTagged {
    Point { x: u8 },
}

pub fn buffered_tagged_target(ctx: &DecodeContext, text: &str) {
    let _value = ctx.parse_json::<InternallyTagged>(text, "internally tagged"); // finding: unproven_decode_charge
}
