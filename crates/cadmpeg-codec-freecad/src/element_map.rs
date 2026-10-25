// SPDX-License-Identifier: Apache-2.0
//! Persistent string tables and element maps admitted by their source parts.

use crate::native::element_map::{ElementMapNodes, ElementMappedName};
use cadmpeg_core::CodecError;

pub(crate) mod adopted;
mod parts;

const MAX_TABLE_ENTRIES: usize = 10_000_000;
pub(crate) const MAX_MAP_NODES: usize = 1_000_000;
const MAX_GROUPS: usize = 1_000_000;
pub(crate) const MAX_NAMES: usize = 10_000_000;

type LegacyElementRecord = crate::native::element_map::LegacyMapRecord;

struct MapPayload {
    parsed: ParsedMap,
}

struct ParsedMap {
    legacy_records: Option<Vec<Option<LegacyElementRecord>>>,
    map_id: u64,
    postfixes: Vec<Option<String>>,
    maps: ElementMapNodes,
}

fn element_map_size(parsed: &ParsedMap) -> Result<usize, CodecError> {
    let root = parsed.maps.root();
    let mapped_name_count = root
        .groups
        .iter()
        .flatten()
        .flat_map(|group| &group.names)
        .flatten()
        .map(Vec::len)
        .sum::<usize>();
    let child_element_count = root
        .groups
        .iter()
        .flatten()
        .flat_map(|group| &group.children)
        .flatten()
        .map(|child| {
            let count = child.split_ascii_whitespace().nth(2).ok_or_else(|| {
                CodecError::Malformed("element-map child descriptor has no count".into())
            })?;
            parse_usize(count, "element-map child count")
        })
        .try_fold(0_usize, |total, count| {
            total
                .checked_add(count?)
                .ok_or_else(|| CodecError::Malformed("element-map size overflows".into()))
        })?;
    mapped_name_count
        .checked_add(child_element_count)
        .ok_or_else(|| CodecError::Malformed("element-map size overflows".into()))
}

fn mapped_name_count(parsed: &ParsedMap) -> usize {
    parsed
        .maps
        .iter()
        .flat_map(|node| &node.groups)
        .flatten()
        .flat_map(|group| &group.names)
        .flatten()
        .flat_map(|chain| chain.iter())
        .count()
}

fn parse_legacy_string_ids(value: &str) -> Result<Vec<i64>, CodecError> {
    let mut ids = Vec::new();
    for token in value
        .split(|character: char| !character.is_ascii_digit() && character != '-')
        .filter(|token| !token.is_empty())
    {
        let id = token
            .parse::<i64>()
            .map_err(|_| CodecError::Malformed("invalid legacy string id".into()))?;
        if id != 0 {
            ids.push(id);
        }
    }
    Ok(ids)
}

fn legacy_map_payload(
    records: Vec<Option<LegacyElementRecord>>,
    _declared_count: usize,
    _source_entry: Option<String>,
) -> Result<MapPayload, CodecError> {
    let maps =
        crate::native::element_map::legacy::project(&records).map_err(CodecError::malformed)?;
    Ok(MapPayload {
        parsed: ParsedMap {
            map_id: 0,
            postfixes: Vec::new(),
            maps,
            legacy_records: Some(records),
        },
    })
}

pub(crate) fn split_indexed_name(value: &str) -> Result<(String, usize), CodecError> {
    if value.is_empty() {
        return Err(CodecError::Malformed("legacy indexed name is empty".into()));
    }
    let suffix_start = value
        .as_bytes()
        .iter()
        .rposition(|character| !character.is_ascii_digit())
        .map_or(0, |position| position + 1);
    let (type_name, suffix) = value.split_at(suffix_start);
    if type_name.is_empty()
        || !type_name
            .bytes()
            .all(|character| character.is_ascii_alphabetic() || character == b'_')
    {
        return Err(CodecError::Malformed("invalid legacy indexed name".into()));
    }
    let index = if suffix.is_empty() {
        0
    } else {
        suffix
            .parse::<usize>()
            .map_err(|_| CodecError::Malformed("invalid legacy indexed-name index".into()))?
    };
    if index > MAX_NAMES {
        return Err(CodecError::malformed(format_args!(
            "FreeCAD legacy indexed name {index} exceeds {MAX_NAMES}"
        )));
    }
    Ok((type_name.to_owned(), index))
}

fn require_bool(value: &str) -> Result<bool, CodecError> {
    match value {
        "0" | "false" => Ok(false),
        "1" | "true" => Ok(true),
        _ => Err(CodecError::malformed(format_args!(
            "invalid boolean {value:?}"
        ))),
    }
}

fn marker_enabled(value: &str) -> Result<bool, CodecError> {
    // Appendix C selects the new naming layout for any nonzero integer.
    match value {
        "true" => Ok(true),
        "false" => Ok(false),
        _ => value.parse::<i64>().map(|flag| flag != 0).map_err(|_| {
            CodecError::malformed(format_args!("invalid naming marker flag {value:?}"))
        }),
    }
}

fn parse_decimal(value: &str, field: &str) -> Result<i64, CodecError> {
    value
        .parse()
        .map_err(|_| CodecError::malformed(format_args!("invalid {field} {value:?}")))
}

fn parse_usize(value: &str, field: &str) -> Result<usize, CodecError> {
    value
        .parse()
        .map_err(|_| CodecError::malformed(format_args!("invalid {field} {value:?}")))
}

pub(crate) fn parse_hex(value: &str, field: &str) -> Result<i64, CodecError> {
    let (negative, digits) = value
        .strip_prefix('-')
        .map_or((false, value), |digits| (true, digits));
    if digits.is_empty() {
        return Err(CodecError::malformed(format_args!("empty {field}")));
    }
    let value = i64::from_str_radix(digits, 16)
        .map_err(|_| CodecError::malformed(format_args!("invalid {field} {value:?}")))?;
    Ok(if negative { -value } else { value })
}

pub(crate) fn parse_mapped_name(
    encoded: &str,
    postfixes: &[Option<String>],
) -> Result<ElementMappedName, CodecError> {
    let mut fields = Vec::new();
    fields.extend(encoded.split('.'));
    let (base, postfix_position, id_position) =
        if let Some(dictionary) = fields[0].strip_prefix(':') {
            if fields.len() < 3 {
                return Err(CodecError::Malformed(
                    "indexed mapped name has incomplete dictionary fields".into(),
                ));
            }
            let dictionary = usize::try_from(parse_hex(dictionary, "mapped-name prefix index")?)
                .map_err(|_| CodecError::malformed("negative mapped-name prefix index"))?;
            let prefix = postfixes
                .get(dictionary.checked_sub(1).ok_or_else(|| {
                    CodecError::Malformed("mapped-name prefix index is zero".into())
                })?)
                .and_then(Option::as_ref)
                .ok_or_else(|| {
                    CodecError::Malformed("mapped-name prefix index is out of range".into())
                })?;
            let element = usize::try_from(parse_hex(fields[1], "mapped-name element index")?)
                .map_err(|_| CodecError::Malformed("negative mapped-name element index".into()))?;
            (format!("{prefix}{element}"), 2, 3)
        } else if let Some(base) = fields[0]
            .strip_prefix(';')
            .or_else(|| fields[0].strip_prefix('$'))
        {
            (base.to_owned(), 1, 2)
        } else {
            return Err(CodecError::Malformed(
                "mapped name has unknown base encoding".into(),
            ));
        };
    let postfix_index = fields
        .get(postfix_position)
        .ok_or_else(|| CodecError::Malformed("mapped name has no postfix index".into()))
        .and_then(|value| {
            usize::try_from(parse_hex(value, "mapped-name postfix index")?)
                .map_err(|_| CodecError::Malformed("negative mapped-name postfix index".into()))
        })?;
    let mut resolved = base;
    if postfix_index != 0 {
        resolved.push_str(
            postfixes
                .get(postfix_index - 1)
                .and_then(Option::as_ref)
                .ok_or_else(|| {
                    CodecError::Malformed("mapped-name postfix index is out of range".into())
                })?,
        );
    }
    let mut string_ids = Vec::new();
    for value in fields
        .iter()
        .skip(id_position)
        .filter(|value| !value.is_empty())
    {
        string_ids.push(parse_hex(value, "mapped-name string id")?);
    }
    Ok(ElementMappedName {
        encoded: encoded.to_owned(),
        resolved: Some(resolved),
        string_ids,
    })
}

#[cfg(test)]
use tests::adapters::{
    direct_element_map, node_text_bytes, parse_element_map, parse_legacy_element_map,
    parse_string_table,
};

#[cfg(test)]
mod tests;
