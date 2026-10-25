// SPDX-License-Identifier: Apache-2.0
//! Bind current source names to proved indices in the emitted topology graph.

use super::{CodecError, ElementMapRecord, FreecadLossCode, LossNote};
use crate::native::element_map::{
    ElementMapGroup, ElementMapNode, ElementMapNodes, ElementMappedName,
};
use cadmpeg_ir::codec::write::NativeDisposition;
use std::collections::{BTreeMap, BTreeSet};

mod sparse;

pub(super) fn apply<'a>(
    original: &ElementMapRecord,
    mappings: impl Iterator<Item = (&'a str, usize, usize)>,
    losses: &mut Vec<LossNote>,
    node_ids: &mut BTreeSet<u64>,
) -> Result<(ElementMapRecord, NativeDisposition), CodecError> {
    let mut lookup = BTreeMap::<(String, usize), BTreeSet<usize>>::new();
    for (kind, source, output) in mappings {
        lookup
            .entry((kind.into(), source))
            .or_default()
            .insert(output);
    }
    let mut map = original.clone();
    let loss_start = losses.len();
    let mut regenerated = false;
    if let Some(rows) = &mut map.legacy_records {
        for (slot, row) in rows.iter_mut().enumerate() {
            let Some(current) = row else { continue };
            let (kind, index) = crate::element_map::split_indexed_name(&current.indexed_name)?;
            if let Some(output) = unique(&lookup, &kind, index) {
                current.indexed_name = format!("{kind}{output}");
                regenerated = true;
            } else {
                warn(
                    losses,
                    &map.id,
                    format!("legacy_records[{slot}] {}", current.indexed_name),
                );
                *row = None;
            }
        }
        map.maps =
            crate::native::element_map::legacy::project(rows).map_err(CodecError::malformed)?;
        return Ok((map, disposition(regenerated, losses.len() != loss_start)));
    }
    let mut nodes = Vec::new();
    for (index, node) in map.maps.indexed() {
        while nodes.len() + 1 < index {
            nodes.push(None);
        }
        nodes.push(Some(node.clone()));
    }
    nodes.pop();
    let root = map.maps.root();
    let mut updated = root.clone();
    for (slot, group_slot) in updated.groups.iter_mut().enumerate() {
        let Some(group) = group_slot.as_mut() else {
            continue;
        };
        let original_group = group.clone();
        let sparse = original_group.names.iter().any(Option::is_none);
        group.names = if sparse {
            Vec::new()
        } else {
            original_group
                .names
                .iter()
                .map(|_| Some(Vec::new()))
                .collect()
        };
        let mut children = Vec::new();
        for (source, chain) in original_group.names.iter().enumerate() {
            let Some(chain) = chain.as_ref().filter(|chain| !chain.is_empty()) else {
                continue;
            };
            if let Some(output) = unique(&lookup, &group.indexed_name, source) {
                if sparse {
                    if let Some(child) = sparse::child(
                        &mut nodes,
                        node_ids,
                        &mut map.postfixes,
                        &group.indexed_name,
                        output,
                        chain,
                    )? {
                        children.push(Some(child));
                        regenerated = true;
                    } else {
                        warn_sparse(losses, &map.id, slot, source);
                    }
                } else {
                    insert(group, output, chain.clone())?;
                    regenerated = true;
                }
            } else {
                warn(
                    losses,
                    &map.id,
                    format!("root.groups[{slot}].names[{source}]"),
                );
            }
        }
        for (child_slot, child) in original_group.children.iter().enumerate() {
            let Some(child) = child else {
                children.push(None);
                continue;
            };
            let fields = child.split_ascii_whitespace().collect::<Vec<_>>();
            let local = decimal(&fields, 0)?;
            let offset = decimal(&fields, 1)?;
            let count = decimal(&fields, 2)?;
            let start = local
                .checked_add(offset)
                .ok_or_else(|| CodecError::malformed("child range overflows"))?;
            let end = start
                .checked_add(count)
                .ok_or_else(|| CodecError::malformed("child range overflows"))?;
            let proved = lookup
                .iter()
                .filter(|((kind, index), outputs)| {
                    kind == &group.indexed_name
                        && (start..end).contains(index)
                        && outputs.len() == 1
                })
                .collect::<Vec<_>>();
            let proved_offsets = proved
                .iter()
                .filter_map(|((_, source), outputs)| outputs.first()?.checked_sub(*source - offset))
                .collect::<Vec<_>>();
            let offsets = proved_offsets.iter().copied().collect::<BTreeSet<_>>();
            if count == 0
                || (proved.len() == count && proved_offsets.len() == count && offsets.len() == 1)
            {
                let mut current = fields
                    .iter()
                    .map(|field| (*field).to_owned())
                    .collect::<Vec<_>>();
                if let Some(offset) = offsets.first() {
                    current[1] = offset.to_string();
                }
                children.push(Some(current.join(" ")));
                regenerated |= count != 0;
                continue;
            }
            if proved.len() != count {
                warn(
                    losses,
                    &map.id,
                    format!("root.groups[{slot}].children[{child_slot}] unproved members"),
                );
            }
            for ((_, source), outputs) in proved {
                let output = *outputs
                    .first()
                    .ok_or_else(|| CodecError::malformed("proved topology index disappeared"))?;
                let child_local = source - offset;
                if let Some(new_offset) = output.checked_sub(child_local) {
                    let mut current = fields
                        .iter()
                        .map(|field| (*field).to_owned())
                        .collect::<Vec<_>>();
                    current[0] = child_local.to_string();
                    current[1] = new_offset.to_string();
                    current[2] = "1".into();
                    children.push(Some(current.join(" ")));
                    regenerated = true;
                } else {
                    let target = decimal(&fields, 4)?;
                    let names = resolve(
                        &nodes,
                        &map.postfixes,
                        target,
                        &group.indexed_name,
                        child_local,
                    )?;
                    let Some(names) = names else {
                        warn(
                            losses,
                            &map.id,
                            format!(
                                "root.groups[{slot}].children[{child_slot}] source index {source}"
                            ),
                        );
                        continue;
                    };
                    let chain = materialize(names, fields[5], fields[6], &mut map.postfixes)?;
                    insert(group, output, chain)?;
                    regenerated = true;
                }
            }
        }
        group.children = children;
        if sparse && group.children.iter().all(Option::is_none) {
            *group_slot = None;
        }
    }
    nodes.push(Some(updated));
    map.maps = ElementMapNodes::try_sparse(nodes).map_err(CodecError::malformed)?;
    Ok((map, disposition(regenerated, losses.len() != loss_start)))
}

fn disposition(regenerated: bool, omitted_associations: bool) -> NativeDisposition {
    if regenerated {
        NativeDisposition::Regenerated
    } else if omitted_associations {
        NativeDisposition::Dropped
    } else {
        NativeDisposition::Written
    }
}

fn unique(
    lookup: &BTreeMap<(String, usize), BTreeSet<usize>>,
    kind: &str,
    index: usize,
) -> Option<usize> {
    let outputs = lookup.get(&(kind.into(), index))?;
    (outputs.len() == 1)
        .then(|| outputs.first().copied())
        .flatten()
}

fn warn(losses: &mut Vec<LossNote>, id: &str, position: String) {
    losses.push(FreecadLossCode::ExportNativePropertyNotTransferred.note(format!(
        "element map {id} omits topology association {position}: no unique corresponding emitted topology index is proved"
    )));
}

fn decimal(fields: &[&str], index: usize) -> Result<usize, CodecError> {
    fields
        .get(index)
        .and_then(|field| field.parse().ok())
        .ok_or_else(|| CodecError::malformed("invalid admitted child descriptor"))
}

fn insert(
    group: &mut ElementMapGroup,
    index: usize,
    chain: Vec<ElementMappedName>,
) -> Result<(), CodecError> {
    if index > crate::element_map::MAX_NAMES {
        return Err(CodecError::InvalidInput(
            "emitted naming index exceeds naming limits".into(),
        ));
    }
    if group.names.len() <= index {
        group.names.resize_with(index + 1, || Some(Vec::new()));
    }
    group.names[index]
        .get_or_insert_with(Vec::new)
        .extend(chain);
    Ok(())
}

fn resolve(
    nodes: &[Option<ElementMapNode>],
    postfixes: &[Option<String>],
    mut node: usize,
    kind: &str,
    mut index: usize,
) -> Result<Option<Vec<ElementMappedName>>, CodecError> {
    let mut suffixes = Vec::new();
    loop {
        if node == 0 {
            let name = format!("{kind}{index}");
            return Ok(Some(append_suffixes(
                vec![ElementMappedName {
                    encoded: String::new(),
                    resolved: Some(name),
                    string_ids: Vec::new(),
                }],
                suffixes,
            )));
        }
        let Some(current) = nodes.get(node - 1).and_then(Option::as_ref) else {
            return Ok(None);
        };
        let Some(group) = current
            .groups
            .iter()
            .flatten()
            .find(|group| group.indexed_name == kind)
        else {
            return Ok(None);
        };
        if group.names.get(index).is_some_and(Option::is_none) {
            return Ok(None);
        }
        if let Some(chain) = group
            .names
            .get(index)
            .and_then(Option::as_ref)
            .filter(|chain| !chain.is_empty())
        {
            let names = chain
                .iter()
                .map(|name| crate::element_map::parse_mapped_name(&name.encoded, postfixes))
                .collect::<Result<Vec<_>, _>>()?;
            return Ok(Some(append_suffixes(names, suffixes)));
        }
        let mut found = None;
        for child in group.children.iter().flatten() {
            let fields = child.split_ascii_whitespace().collect::<Vec<_>>();
            let start = decimal(&fields, 0)?
                .checked_add(decimal(&fields, 1)?)
                .ok_or_else(|| CodecError::malformed("child range overflows"))?;
            let end = start
                .checked_add(decimal(&fields, 2)?)
                .ok_or_else(|| CodecError::malformed("child range overflows"))?;
            if (start..end).contains(&index) {
                found = Some((
                    decimal(&fields, 4)?,
                    decimal(&fields, 1)?,
                    fields[5].to_owned(),
                ));
                break;
            }
        }
        let Some((target, offset, suffix)) = found else {
            return Ok(None);
        };
        if target >= node {
            return Err(CodecError::malformed("child map is not a prior node"));
        }
        suffixes.push(suffix);
        node = target;
        index -= offset;
    }
}

fn append_suffixes(
    mut names: Vec<ElementMappedName>,
    suffixes: Vec<String>,
) -> Vec<ElementMappedName> {
    for name in &mut names {
        if let Some(resolved) = &mut name.resolved {
            for suffix in suffixes.iter().rev() {
                resolved.push_str(suffix);
            }
        }
    }
    names
}

fn materialize(
    mut names: Vec<ElementMappedName>,
    suffix: &str,
    selectors: &str,
    postfixes: &mut Vec<Option<String>>,
) -> Result<Vec<ElementMappedName>, CodecError> {
    let selectors = selectors
        .split('.')
        .skip(1)
        .map(|field| field.parse::<i64>().map_err(CodecError::malformed))
        .collect::<Result<Vec<_>, _>>()?;
    for name in &mut names {
        let Some(resolved) = &mut name.resolved else {
            return Err(CodecError::malformed("mapped name has no resolved value"));
        };
        resolved.push_str(suffix);
        let index = postfixes
            .iter()
            .position(|postfix| postfix.as_ref() == Some(resolved))
            .map_or_else(
                || {
                    postfixes.push(Some(resolved.clone()));
                    postfixes.len()
                },
                |index| index + 1,
            );
        name.string_ids.extend(&selectors);
        name.encoded = format!(";.{index:x}");
        for selector in &name.string_ids {
            name.encoded.push('.');
            name.encoded.push_str(&super::hex(*selector));
        }
    }
    Ok(names)
}

fn warn_sparse(losses: &mut Vec<LossNote>, id: &str, group: usize, source: usize) {
    losses.push(FreecadLossCode::ExportNativePropertyNotTransferred.note(format!(
        "element map {id} omits topology association root.groups[{group}].names[{source}]: sparse NameCount has no start selector and this alias chain has no common complete authored postfix with unchanged nonempty prefix/SID tokens (#215 sections 5.2 and 4.5)"
    )));
}
