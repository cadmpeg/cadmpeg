// SPDX-License-Identifier: Apache-2.0
use cadmpeg_core::serde_erase::json;
use std::io::Read;

use base64::Engine;
use serde_json::{Map, Value};

use super::Mapping;

const BYTES: usize = 1024 * 1024;
const CACHE_BYTES: usize = 4 * BYTES;
const REFUSED: &str = "the compressed query encoding is not supported yet";

pub(super) fn needs_deterministic_ids(query: &Value) -> bool {
    let has_expression = query.get("queryString").is_some_and(|value| {
        !value.is_null() && value.as_str().is_none_or(|text| !text.trim().is_empty())
    });
    has_expression
        && match query.get("deterministicIds") {
            None | Some(Value::Null) => true,
            Some(Value::Array(ids)) => ids.is_empty(),
            _ => false,
        }
}

pub(super) fn has_unresolved_ids(parameter: &Value) -> bool {
    super::rows(parameter, "queries").is_ok_and(|rows| rows.iter().any(needs_deterministic_ids))
}

pub(super) fn resolved_leaves(source: &Value) -> Mapping<Vec<Value>> {
    let mut pending = vec![resolved(source)?];
    let mut leaves = Vec::new();
    while let Some(query) = pending.pop() {
        if query.get("queryType").and_then(Value::as_str) == Some("UNION") {
            let children = super::rows(&query, "subqueries")?;
            if children.is_empty() {
                return Err("the query union is empty".into());
            }
            pending.extend(children.iter().rev().cloned());
        } else {
            leaves.push(query);
        }
        if pending
            .len()
            .checked_add(leaves.len())
            .ok_or("the query union exceeds the row limit")?
            > super::ROWS
        {
            return Err("the query union exceeds the row limit".into());
        }
    }
    Ok(leaves)
}

pub(crate) fn resolved(query: &Value) -> Mapping<Value> {
    let source = super::text(query, "queryString")?;
    if source.len() > BYTES {
        return Err("the compressed query exceeds the size limit".into());
    }
    let source = source
        .trim()
        .strip_prefix("query=")
        .unwrap_or(source.trim());
    let args = source
        .strip_prefix("qCompressed(")
        .and_then(|s| s.trim_end_matches(';').strip_suffix(')'))
        .ok_or(REFUSED)?;
    let (version, args) = args.split_once(',').ok_or(REFUSED)?;
    let (literal, binding) = args.rsplit_once(',').ok_or(REFUSED)?;
    if version.trim() != "1.0" || binding.trim() != "id" {
        return Err(REFUSED.into());
    }
    let literal: String = serde_json::from_str(literal.trim()).map_err(|_| REFUSED)?;
    let expanded = if let Some(compressed) = literal.strip_prefix('&') {
        let (size, data) = compressed.split_once('$').ok_or(REFUSED)?;
        let size = usize::from_str_radix(size, 16).map_err(|_| REFUSED)?;
        if size > BYTES {
            return Err("the compressed query exceeds the size limit".into());
        }
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(data)
            .map_err(|_| REFUSED)?;
        let mut expanded = String::new();
        flate2::read::ZlibDecoder::new(bytes.as_slice())
            .take(cadmpeg_core::decode::u64_from_index(BYTES + 1))
            .read_to_string(&mut expanded)
            .map_err(|_| REFUSED)?;
        if expanded.len() != size {
            return Err("the compressed query length does not match its data".into());
        }
        expanded
    } else {
        literal
    };
    let mut reader = Reader {
        rest: expanded.strip_prefix('%').ok_or(REFUSED)?,
        strings: Vec::new(),
        values: Vec::new(),
        cached: 0,
        nodes: 0,
    };
    let value = reader.value(0)?;
    if !reader.rest.is_empty() {
        return Err(REFUSED.into());
    }
    Ok(value)
}

struct Reader<'a> {
    rest: &'a str,
    strings: Vec<String>,
    values: Vec<(Value, usize)>,
    cached: usize,
    nodes: usize,
}

impl Reader<'_> {
    fn charge(&mut self, bytes: usize) -> Mapping<()> {
        self.cached = self.cached.checked_add(bytes).ok_or(REFUSED)?;
        if self.cached > CACHE_BYTES {
            return Err("the compressed query exceeds the reference limit".into());
        }
        Ok(())
    }

    fn integer(&mut self) -> Mapping<i64> {
        let end = self
            .rest
            .char_indices()
            .take_while(|(i, c)| matches!(c, '0'..='9' | 'a'..='f') || (*i == 0 && *c == '-'))
            .map(|(i, _)| i + 1)
            .last()
            .ok_or(REFUSED)?;
        let (number, rest) = self.rest.split_at(end);
        self.rest = rest;
        i64::from_str_radix(number, 16).map_err(|_| REFUSED.into())
    }

    fn count(&mut self) -> Mapping<usize> {
        let count = usize::try_from(self.integer()?).map_err(|_| REFUSED)?;
        if count > super::ROWS {
            return Err("the compressed query exceeds the row limit".into());
        }
        Ok(count)
    }

    fn intern(&mut self, string: String) -> Mapping<String> {
        if self.strings.len() >= super::ROWS {
            return Err("the compressed query exceeds the reference limit".into());
        }
        self.charge(string.len())?;
        self.strings.push(string.clone());
        Ok(string)
    }

    fn fragment(&mut self, size: i64) -> Mapping<String> {
        if size < 0 {
            let index = size
                .checked_neg()
                .and_then(|n| usize::try_from(n).ok())
                .ok_or(REFUSED)?;
            return self.strings.get(index).cloned().ok_or(REFUSED.into());
        }
        let size = usize::try_from(size).map_err(|_| REFUSED)?;
        let string = self.rest.get(..size).ok_or(REFUSED)?.to_owned();
        self.rest = self.rest.get(size..).ok_or(REFUSED)?;
        self.intern(string)
    }

    fn string(&mut self) -> Mapping<String> {
        let first = self.integer()?;
        let second = if let Some(rest) = self.rest.strip_prefix('.') {
            self.rest = rest;
            Some(self.integer()?)
        } else {
            None
        };
        self.rest = self.rest.strip_prefix('$').ok_or(REFUSED)?;
        let mut string = self.fragment(first)?;
        if let Some(second) = second {
            string.push('.');
            string.push_str(&self.fragment(second)?);
            string = self.intern(string)?;
        }
        Ok(string)
    }

    fn value(&mut self, depth: usize) -> Mapping<Value> {
        self.nodes += 1;
        if depth > 64 || self.nodes > super::ROWS {
            return Err("the compressed query exceeds the nesting or row limit".into());
        }
        let tag = self.rest.chars().next().ok_or(REFUSED)?;
        self.rest = self.rest.get(1..).ok_or(REFUSED)?;
        if matches!(tag, 'B' | 'C') {
            let class = if tag == 'B' {
                self.string()?
            } else {
                let index = self.count()?;
                self.strings.get(index).cloned().ok_or(REFUSED)?
            };
            if !matches!(class.as_str(), "Query" | "QueryType" | "EntityType" | "Id") {
                return Err("this compressed query value type is not supported yet".into());
            }
            return self.value(depth + 1);
        }
        let value = match tag {
            'S' => Value::String(self.string()?),
            'T' => Value::Bool(true),
            'F' => Value::Bool(false),
            'R' => {
                let index = self.count()?;
                let size = self.values.get(index).ok_or(REFUSED)?.1;
                self.charge(size)?;
                return self
                    .values
                    .get(index)
                    .map(|(v, _)| v.clone())
                    .ok_or(REFUSED.into());
            }
            'D' => Value::from(self.integer()?),
            'A' => {
                let count = self.count()?;
                let mut array = Vec::new();
                for _ in 0..count {
                    array.push(self.value(depth + 1)?);
                }
                Value::Array(array)
            }
            'M' => {
                let count = self.count()?;
                let mut map = Map::new();
                for _ in 0..count {
                    let key = self.value(depth + 1)?.as_str().ok_or(REFUSED)?.to_owned();
                    if map.insert(key, self.value(depth + 1)?).is_some() {
                        return Err("the compressed query repeats a field".into());
                    }
                }
                Value::Object(map)
            }
            _ => return Err(REFUSED.into()),
        };
        bounded_depth(&value, 0)?;
        let size = json::to_vec(&value).map_err(|_| REFUSED)?.len();
        self.charge(size)?;
        self.values.push((value.clone(), size));
        Ok(value)
    }
}
fn bounded_depth(value: &Value, depth: usize) -> Mapping<()> {
    if depth > 64 {
        return Err("the compressed query exceeds the nesting limit".into());
    }
    match value {
        Value::Array(a) => a.iter().try_for_each(|v| bounded_depth(v, depth + 1)),
        Value::Object(o) => o.values().try_for_each(|v| bounded_depth(v, depth + 1)),
        _ => Ok(()),
    }
}

#[cfg(test)]
mod tests;
