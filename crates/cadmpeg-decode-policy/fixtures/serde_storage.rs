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
struct ScanningDrop(String);

impl Drop for ScanningDrop {
    fn drop(&mut self) {
        for character in self.0.chars() {
            std::hint::black_box(character);
        }
    }
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

pub fn custom_drop_target(ctx: &DecodeContext, text: &str) {
    let _values = ctx.parse_json::<Vec<ScanningDrop>>(text, "custom drop"); // finding: unproven_decode_charge
}
