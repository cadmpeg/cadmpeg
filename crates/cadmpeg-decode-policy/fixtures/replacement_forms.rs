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

pub fn collection_growth(
    _ctx: &DecodeContext,
    output: &mut String,
    characters: &[char],
    texts: &[String],
    text: &str,
    values: &mut Vec<u8>,
    byte: u8,
) {
    use std::fmt::Write as _;

    // replacement: DecodeContext::push_retained_char
    output.push('é'); // finding: uncharged_decode_allocation
    // replacement: DecodeContext::push_retained_char
    let _ = output.write_char('ö'); // finding: unproven_decode_charge
    // replacement: DecodeContext::append_retained
    output.push_str(text); // finding: unproven_decode_charge, uncharged_decode_work
    // replacement: DecodeContext::append_retained
    let _ = output.write_str(text); // finding: unproven_decode_charge, uncharged_decode_work
    // replacement: DecodeContext::replace_text_range over index..index with value.encode_utf8(&mut [0; 4])
    output.insert(0, 'é'); // finding: unproven_decode_charge, uncharged_decode_work
    // replacement: DecodeContext::admit_iter followed by DecodeContext::push_retained_char for char/&char items or DecodeContext::append_retained for &str/String/Cow<str>/Box<str> items
    output.extend(characters.iter().copied()); // finding: unproven_decode_charge, uncharged_decode_work
    // replacement: DecodeContext::admit_iter followed by DecodeContext::push_retained_char for char/&char items or DecodeContext::append_retained for &str/String/Cow<str>/Box<str> items
    output.extend(texts.iter().map(String::as_str)); // finding: unproven_decode_charge, uncharged_decode_work
    // replacement: push_vec
    values.push(byte); // finding: uncharged_decode_allocation
}

impl DecodeContext {
    pub fn retained_string(&self, _capacity: usize, _operation: &str) -> Result<String, ()> { Ok(String::new()) }
    pub fn collect_text(&self, _values: impl IntoIterator<Item = char>, _operation: &str) -> Result<String, ()> { Ok(String::new()) }
    pub fn strip_prefix<'a>(&self, bytes: &'a [u8], _prefix: &[u8], _operation: &str) -> Result<Option<&'a [u8]>, ()> { Ok(Some(bytes)) }
    pub fn strip_suffix<'a>(&self, bytes: &'a [u8], _suffix: &[u8], _operation: &str) -> Result<Option<&'a [u8]>, ()> { Ok(Some(bytes)) }
}
pub fn string_collection(ctx: &DecodeContext, chars: &[char], capacity: usize) -> Result<(), ()> {
    // replacement: retained_string
    let _value = String::with_capacity(capacity); // finding: uncharged_decode_allocation
    // replacement: collect_text
    let _value: String = chars.iter().copied().collect(); // finding: uncharged_decode_allocation, uncharged_decode_work
    let _value = ctx.retained_string(capacity, "capacity")?;
    let _value = ctx.collect_text(chars.iter().copied(), "characters")?;
    Ok(())
}
pub fn byte_queries(ctx: &DecodeContext, bytes: &[u8], pattern: &[u8]) -> Result<(), ()> {
    // replacement: strip_prefix
    let _value = bytes.strip_prefix(pattern); // finding: uncharged_decode_work
    // replacement: strip_suffix
    let _value = bytes.strip_suffix(pattern); // finding: uncharged_decode_work
    let _value = ctx.strip_prefix(bytes, pattern, "prefix")?;
    let _value = ctx.strip_suffix(bytes, pattern, "suffix")?;
    Ok(())
}
