// SPDX-License-Identifier: Apache-2.0
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
pub struct DecodeContext;
pub fn decode(
    _ctx: &DecodeContext,
    hash: &HashMap<String, u8>,
    tree: &BTreeMap<String, u8>,
    hash_set: &HashSet<String>,
    tree_set: &BTreeSet<String>,
    values: &[String],
    text: &str,
    bytes: &[u8],
    other: &[u8],
    key: &String,
) {
    // replacement: get_hash_map
    let _value = hash.get(key); // finding: uncharged_decode_work
                                // replacement: contains_key_btree_map
    let _value = tree.contains_key(key); // finding: uncharged_decode_work
                                         // replacement: contains_hash_set
    let _value = hash_set.contains(key); // finding: uncharged_decode_work
                                         // replacement: get_btree_set
    let _value = tree_set.get(key); // finding: uncharged_decode_work
                                    // replacement: contains
    let _value = values.contains(key); // finding: uncharged_decode_work
                                       // replacement: contains_text
    let _value = text.contains(key.as_str()); // finding: uncharged_decode_work
                                              // replacement: find_text
    let _value = text.find(key.as_str()); // finding: uncharged_decode_work
                                          // replacement: copy_retained_text
    let _value = text.to_owned(); // finding: uncharged_decode_allocation, uncharged_decode_work
                                  // replacement: equal_bytes
    let _value = bytes == other; // finding: uncharged_decode_work
                                 // replacement: compare
    let _value = bytes < other; // finding: uncharged_decode_work
                                // replacement: format_retained
    let _value = format!("{text}"); // finding: uncharged_decode_allocation, uncharged_decode_work
                                    // replacement: parse_radix
    let _value = u64::from_str_radix(text, 16); // finding: uncharged_decode_work
}
