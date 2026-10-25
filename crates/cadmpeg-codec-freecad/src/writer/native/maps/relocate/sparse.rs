// SPDX-License-Identifier: Apache-2.0
//! Preserve sparse root names through exact finite child-map associations.

use super::{CodecError, ElementMapGroup, ElementMapNode, ElementMappedName};
use std::collections::BTreeSet;

pub(super) fn child(
    nodes: &mut Vec<Option<ElementMapNode>>,
    ids: &mut BTreeSet<u64>,
    dictionary: &[Option<String>],
    kind: &str,
    output: usize,
    chain: &[ElementMappedName],
) -> Result<Option<String>, CodecError> {
    let mut prefixes = Vec::new();
    let mut common_postfix: Option<String> = None;
    for name in chain {
        let actual = crate::element_map::parse_mapped_name(&name.encoded, dictionary)?;
        let mut fields = name
            .encoded
            .split('.')
            .map(str::to_owned)
            .collect::<Vec<_>>();
        let position = if fields[0].starts_with(':') { 2 } else { 1 };
        let postfix = fields
            .get(position)
            .and_then(|value| crate::element_map::parse_hex(value, "postfix selector").ok())
            .and_then(|value| usize::try_from(value).ok())
            .and_then(|value| value.checked_sub(1))
            .and_then(|index| dictionary.get(index))
            .and_then(Option::as_ref);
        let Some(postfix) = postfix
            .filter(|postfix| !postfix.is_empty() && !postfix.chars().any(char::is_whitespace))
        else {
            return Ok(None);
        };
        if common_postfix
            .as_ref()
            .is_some_and(|common| common != postfix)
        {
            return Ok(None);
        }
        common_postfix = Some(postfix.clone());
        fields[position] = "0".into();
        let prefix = crate::element_map::parse_mapped_name(&fields.join("."), dictionary)?;
        // Move only an existing complete postfix token. The original base,
        // prefix selector and SID tokens remain unchanged; no string-table
        // payload is rewritten or reinterpreted as a different substring.
        if prefix.string_ids != actual.string_ids
            || prefix.resolved.as_deref().is_none_or(str::is_empty)
            || prefix.resolved.as_deref()
                != actual
                    .resolved
                    .as_deref()
                    .and_then(|value| value.strip_suffix(postfix))
        {
            return Ok(None);
        }
        prefixes.push(prefix);
    }
    let Some(postfix) = common_postfix else {
        return Ok(None);
    };
    if nodes.len() + 1 >= crate::element_map::MAX_MAP_NODES {
        return Err(CodecError::InvalidInput(
            "sparse output map-node count exceeds limit".into(),
        ));
    }
    let mut id = 0_u64;
    while ids.contains(&id) {
        id = id.checked_add(1).ok_or_else(|| {
            CodecError::InvalidInput("sparse output map identities exhausted".into())
        })?;
    }
    ids.insert(id);
    nodes.push(Some(ElementMapNode {
        map_id: id,
        groups: vec![Some(ElementMapGroup {
            indexed_name: kind.into(),
            children: Vec::new(),
            names: vec![Some(prefixes)],
        })],
    }));
    // Local index zero is legal (IndexedName bool tests the type only). The
    // child map contributes the unchanged prefix+SIDs, then its actual postfix.
    Ok(Some(format!("0 {output} 1 0 {} {postfix} 0", nodes.len())))
}
