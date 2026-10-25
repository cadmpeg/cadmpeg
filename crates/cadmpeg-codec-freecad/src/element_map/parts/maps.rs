// SPDX-License-Identifier: Apache-2.0
//! Element-map leaves are bounded before their semantic tokens are read.

use super::{checked_count, spec, Scanner};
use crate::native::element_map::{ElementMapGroup, ElementMapNode, ElementMapNodes};
use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::CodecError;
use cadmpeg_ir::source_fidelity::parts::{DecodeParts, PartOutput};
use std::ops::Range;

pub(in crate::element_map) fn map<'source>(
    ctx: &DecodeContext<'_>,
    view: View<'source>,
    side: bool,
    symbols: super::StringIds<'_>,
    spacing: Option<&super::Spacing<'_, 'source>>,
    stage: &mut PartOutput,
) -> Result<Option<super::super::ParsedMap>, CodecError> {
    let mut scanner = Scanner::tracking(view, spacing);
    if side {
        let start = scanner.whitespace_end()?;
        scanner.token_range()?;
        scanner.token_range()?;
        let end = scanner.whitespace_end()?;
        let extent = scanner.extent(start..end)?;
        let fields = if end > start {
            vec![end - start - 1..end - start]
        } else {
            Vec::new()
        };
        if ctx
            .decode_part(
                stage,
                extent,
                spec("freecad.element_map_header", extent, &fields)?,
                |_stage| {
                    let mut header = Scanner::new(extent);
                    header.expect("BeginElementMap")?;
                    header.expect("v1")
                },
            )?
            .is_none()
        {
            return Ok(None);
        }
    }
    let dictionary_start = scanner.whitespace_end()?;
    scanner.token_range()?;
    let marker = scanner.token_range()?;
    if scanner.navigation(marker.clone())? != "PostfixCount" {
        return Err(CodecError::malformed(
            "element-map postfix locator is absent",
        ));
    }
    let count_range = scanner.token_range()?;
    let count = scanner
        .navigation(count_range.clone())?
        .parse::<usize>()
        .map_err(|_| CodecError::malformed("invalid postfix framing count"))?;
    checked_count(ctx, count, view, "postfix count exceeds input")?;
    let mut postfix_ranges = ctx.alloc_filled(count, 0..0, "FreeCAD postfix extents")?;
    for range in &mut postfix_ranges {
        let start = scanner.whitespace_end()?;
        scanner.token_range()?;
        let end = scanner.whitespace_end()?;
        *range = start..end;
    }
    let dictionary_end = scanner.at;
    let extent = scanner.extent(dictionary_start..dictionary_end)?;
    let field = marker.start - dictionary_start..count_range.end - dictionary_start;
    let prefix = if side {
        "freecad.element_map_"
    } else {
        "freecad.inline_map_"
    };
    let dictionary = ctx.decode_part(
        stage,
        extent,
        spec(&format!("{prefix}postfix_dictionary"), extent, &[field])?,
        |dictionary_stage| {
            let mut header = Scanner::new(extent);
            let map_id = header
                .token()?
                .parse::<u64>()
                .map_err(|_| CodecError::malformed("invalid element-map id"))?;
            header.expect("PostfixCount")?;
            header.count(super::super::MAX_NAMES)?;
            let mut postfixes = ctx.alloc_filled(count, None, "FreeCAD postfix dictionary")?;
            for (slot, range) in postfixes.iter_mut().zip(&postfix_ranges) {
                let leaf = scanner.extent(range.clone())?;
                let token_end = leaf
                    .window()
                    .iter()
                    .position(u8::is_ascii_whitespace)
                    .unwrap_or(leaf.window().len());
                let fields = if token_end < leaf.window().len() {
                    vec![token_end..leaf.window().len()]
                } else {
                    Vec::new()
                };
                *slot = ctx.decode_part(
                    dictionary_stage,
                    leaf,
                    spec(&format!("{prefix}postfix"), leaf, &fields)?,
                    |_stage| Ok(Scanner::new(leaf).token()?.to_owned()),
                )?;
            }
            Ok((map_id, postfixes))
        },
    )?;
    let count_start = scanner.whitespace_end()?;
    scanner.expect("MapCount")?;
    let count = scanner.count(super::super::MAX_MAP_NODES)?;
    if count == 0 {
        return Err(CodecError::malformed("element map has no owning root node"));
    }
    let count_end = scanner.at;
    checked_count(ctx, count, view, "map-node count exceeds input")?;
    let mut nodes = ctx.alloc_filled(count, None, "FreeCAD element-map nodes")?;
    for position in 0..count {
        scanner.skip()?;
        let start = if position == 0 {
            count_start
        } else {
            scanner.at
        };
        let marker = scanner.token_range()?;
        if scanner.navigation(marker.clone())? != "ElementMap" {
            return Err(CodecError::malformed("element-map node locator is absent"));
        }
        let mut end_marker = None;
        while scanner.at < view.window().len() {
            scanner.skip()?;
            if scanner.at == view.window().len() {
                break;
            }
            let token = scanner.token_range()?;
            if &scanner.view.window()[token.clone()] == b"EndMap" {
                end_marker = Some(token);
                scanner.skip()?;
                break;
            }
        }
        let end = scanner.at;
        let cut_short = end_marker.is_none();
        let extent = scanner.extent(start..end)?;
        let mut nav = Scanner::new(extent);
        if position == 0 {
            nav.token_range()?;
            nav.token_range()?;
        }
        let line_start = nav.token_range()?.start;
        nav.token_range()?;
        nav.token_range()?;
        let groups = nav.token_range()?;
        let mut fields = vec![line_start..groups.end];
        if let Some(end_marker) = end_marker {
            fields.push(end_marker.start - start..end - start);
        }
        if position == 0 {
            fields.insert(0, count_start - start..count_end - start);
        }
        let prior = nodes[..position]
            .iter()
            .map(Option::is_some)
            .collect::<Vec<_>>();
        nodes[position] = ctx.decode_part(
            stage,
            extent,
            spec(&format!("{prefix}node"), extent, &fields)?,
            |node_stage| {
                decode_node(
                    ctx,
                    extent,
                    position + 1,
                    &prior,
                    dictionary
                        .as_ref()
                        .map(|(_, postfixes)| postfixes.as_slice()),
                    symbols,
                    prefix,
                    node_stage,
                )
            },
        )?;
        if cut_short {
            // The declared later nodes were never reached. The current
            // terminated node owns this actual EOF tail, without row resync.
            break;
        }
    }
    if !scanner.done()? {
        return Err(CodecError::malformed("element map has trailing data"));
    }
    let Some((map_id, postfixes)) = dictionary else {
        return Ok(None);
    };
    if nodes.last().is_none_or(Option::is_none) {
        return Ok(None);
    }
    Ok(Some(super::super::ParsedMap {
        legacy_records: None,
        map_id,
        postfixes,
        maps: ElementMapNodes::try_sparse(nodes).map_err(CodecError::malformed)?,
    }))
}

fn decode_node(
    ctx: &DecodeContext<'_>,
    view: View<'_>,
    index: usize,
    prior: &[bool],
    postfixes: Option<&[Option<String>]>,
    symbols: super::StringIds<'_>,
    prefix: &str,
    stage: &mut PartOutput,
) -> Result<ElementMapNode, CodecError> {
    let mut scanner = Scanner::new(view);
    if index == 1 {
        scanner.expect("MapCount")?;
        scanner.count(super::super::MAX_MAP_NODES)?;
    }
    scanner.expect("ElementMap")?;
    if scanner.count(super::super::MAX_MAP_NODES)? != index {
        return Err(CodecError::malformed(
            "element-map node indices are not contiguous",
        ));
    }
    let map_id = scanner
        .token()?
        .parse()
        .map_err(|_| CodecError::malformed("invalid map-node id"))?;
    let count = scanner.count(super::super::MAX_GROUPS)?;
    checked_count(ctx, count, view, "map-group count exceeds input")?;
    let mut groups = ctx.alloc_filled(count, None, "FreeCAD element-map groups")?;
    for group in &mut groups {
        scanner.skip()?;
        let start = scanner.at;
        let (end, fields, children, names) = frame_group(&mut scanner)?;
        let extent = scanner.extent(start..end)?;
        let fields = fields
            .into_iter()
            .map(|r| r.start - start..r.end - start)
            .collect::<Vec<_>>();
        *group = ctx.decode_part(
            stage,
            extent,
            spec(&format!("{prefix}group"), extent, &fields)?,
            |group_stage| {
                let mut header = Scanner::new(extent);
                let indexed_name = header.token()?.to_owned();
                header.expect("ChildCount")?;
                header.count(super::super::MAX_NAMES)?;
                let mut child_values =
                    ctx.alloc_filled(children.len(), None, "FreeCAD map children")?;
                for (child_index, (slot, range)) in
                    child_values.iter_mut().zip(children).enumerate()
                {
                    let leaf = scanner.extent(range)?;
                    let content_end = leaf
                        .window()
                        .iter()
                        .rposition(|byte| !byte.is_ascii_whitespace())
                        .map_or(0, |at| at + 1);
                    let fields = if content_end < leaf.window().len() {
                        vec![content_end..leaf.window().len()]
                    } else {
                        Vec::new()
                    };
                    let mut dependency = Scanner::new(leaf);
                    for _ in 0..4 {
                        dependency.token_range()?;
                    }
                    let target = dependency.token_range()?;
                    // Navigation decides only whether a known prior null slot
                    // makes this list entry unavailable under #215 rule 2.
                    // Malformed and forward selectors still reach the leaf.
                    let missing_prior = dependency
                        .navigation(target)
                        .ok()
                        .and_then(|text| text.parse::<usize>().ok())
                        .filter(|index| *index != 0)
                        .is_some_and(|index| prior.get(index - 1) == Some(&false));
                    dependency.token_range()?;
                    let sid_range = dependency.token_range()?;
                    let missing_strings = dependency.navigation(sid_range).ok()
                        .and_then(|text| text.strip_prefix("0."))
                        .map(|text| text.split('.').filter_map(|id| id.parse::<i64>().ok()).collect::<Vec<_>>())
                        .is_some_and(|ids| symbols.missing(&ids));
                    *slot = ctx
                        .decode_part(
                            group_stage,
                            leaf,
                            spec(&format!("{prefix}child"), leaf, &fields)?,
                            |_stage| {
                                if missing_prior || missing_strings {
                                    return Ok(None);
                                }
                                let mut tokens = Scanner::new(leaf);
                                let mut fields = Vec::new();
                                for _ in 0..7 {
                                    fields.push(tokens.token()?.to_owned());
                                }
                                let ids = fields[6].split('.').skip(1)
                                    .filter_map(|id| id.parse::<i64>().ok()).collect::<Vec<_>>();
                                if symbols.known.is_some_and(|known| ids.iter().any(|id| !known.contains(id))) {
                                    return Err(CodecError::malformed("child descriptor needs an unavailable persistent string id"));
                                }
                                let child = fields.join(" ");
                                ElementMapNodes::validate_child(
                                    index,
                                    &indexed_name,
                                    child_index,
                                    &child,
                                )
                                .map_err(CodecError::malformed)?;
                                let map_index = fields[4].parse::<usize>().map_err(|_| {
                                    CodecError::malformed("invalid child map index")
                                })?;
                                if map_index != 0 && prior.get(map_index - 1) != Some(&true) {
                                    return Err(CodecError::malformed(
                                        "child descriptor has no prior map node",
                                    ));
                                }
                                Ok(Some(child))
                            },
                        )?
                        .flatten();
                }
                let mut name_values =
                    ctx.alloc_filled(names.len(), None, "FreeCAD mapped-name chains")?;
                for (slot, range) in name_values.iter_mut().zip(names) {
                    let leaf = scanner.extent(range)?;
                    let mut nav = Scanner::new(leaf);
                    let mut terminator = 0..0;
                    while !nav.done()? {
                        terminator = nav.token_range()?;
                    }
                    *slot = ctx
                        .decode_part(
                            group_stage,
                            leaf,
                            spec(&format!("{prefix}name_chain"), leaf, &[terminator])?,
                            |_stage| {
                                let Some(postfixes) = postfixes else {
                                    return Ok(None);
                                };
                                let mut navigation = Scanner::new(leaf);
                                let mut missing = false;
                                while !navigation.done()? {
                                    let range = navigation.token_range()?;
                                    let token = navigation.navigation(range)?;
                                    if token == "0" {
                                        break;
                                    }
                                    missing |= missing_name_dependency(token, postfixes, symbols);
                                }
                                if missing {
                                    return Ok(None);
                                }
                                let mut tokens = Scanner::new(leaf);
                                let mut chain = Vec::new();
                                loop {
                                    let name = tokens.token()?;
                                    if name == "0" {
                                        break;
                                    }
                                    let name = super::super::parse_mapped_name(name, postfixes)?;
                                    if symbols.known.is_some_and(|ids| {
                                        name.string_ids.iter().any(|id| !ids.contains(id))
                                    }) {
                                        return Err(CodecError::malformed(
                                            "mapped name needs an unavailable persistent string id",
                                        ));
                                    }
                                    chain.push(name);
                                }
                                Ok(Some(chain))
                            },
                        )?
                        .flatten();
                }
                Ok(ElementMapGroup {
                    indexed_name,
                    children: child_values,
                    names: name_values,
                })
            },
        )?;
    }
    scanner.expect("EndMap")?;
    Ok(ElementMapNode { map_id, groups })
}

fn frame_group(
    scanner: &mut Scanner<'_, '_>,
) -> Result<
    (
        usize,
        Vec<Range<usize>>,
        Vec<Range<usize>>,
        Vec<Range<usize>>,
    ),
    CodecError,
> {
    scanner.token_range()?;
    let child_marker = scanner.token_range()?;
    if scanner.navigation(child_marker.clone())? != "ChildCount" {
        return Err(CodecError::malformed("map group child locator is absent"));
    }
    let count = scanner.token_range()?;
    let children_count = scanner
        .navigation(count.clone())?
        .parse::<usize>()
        .map_err(|_| CodecError::malformed("invalid map child framing count"))?;
    if children_count > super::super::MAX_NAMES {
        return Err(CodecError::malformed(
            "map child framing count exceeds limit",
        ));
    }
    let mut framing = vec![child_marker.start..count.end];
    let mut children = Vec::new();
    for _ in 0..children_count {
        let start = scanner.whitespace_end()?;
        for _ in 0..7 {
            scanner.token_range()?;
        }
        let end = scanner.whitespace_end()?;
        children.push(start..end);
    }
    let name_marker = scanner.token_range()?;
    if scanner.navigation(name_marker.clone())? != "NameCount" {
        return Err(CodecError::malformed("map group name locator is absent"));
    }
    let count = scanner.token_range()?;
    let names_count = scanner
        .navigation(count.clone())?
        .parse::<usize>()
        .map_err(|_| CodecError::malformed("invalid map name framing count"))?;
    if names_count > super::super::MAX_NAMES {
        return Err(CodecError::malformed(
            "map name framing count exceeds limit",
        ));
    }
    framing.push(name_marker.start..count.end);
    let mut names = Vec::new();
    for _ in 0..names_count {
        let start = scanner.whitespace_end()?;
        loop {
            let token = scanner.token_range()?;
            if &scanner.view.window()[token] == b"0" {
                break;
            }
        }
        let end = scanner.whitespace_end()?;
        names.push(start..end);
    }
    Ok((scanner.at, framing, children, names))
}

pub(in crate::element_map) fn legacy<'source>(
    ctx: &DecodeContext<'_>,
    view: View<'source>,
    count: Option<usize>,
    symbols: super::StringIds<'_>,
    spacing: Option<&super::Spacing<'_, 'source>>,
    stage: &mut PartOutput,
) -> Result<(usize, Vec<Option<super::super::LegacyElementRecord>>), CodecError> {
    let mut scanner = Scanner::tracking(view, spacing);
    let side = count.is_none();
    let count = match count {
        Some(count) => count,
        None => scanner.count(super::super::MAX_NAMES)?,
    };
    checked_count(ctx, count, view, "legacy map count exceeds input")?;
    let mut records = ctx.alloc_filled(count, None, "FreeCAD legacy map record slots")?;
    for slot in &mut records {
        let start = scanner.whitespace_end()?;
        scanner.token_range()?;
        let indexed_end = scanner.at;
        let mapped = scanner.token_range()?;
        let mut fields = vec![indexed_end..mapped.start];
        let mapped_end = scanner.at;
        let count_range = scanner.token_range()?;
        fields.push(mapped_end..count_range.start);
        fields.push(count_range.clone());
        let count = scanner
            .navigation(count_range.clone())?
            .parse::<usize>()
            .map_err(|_| CodecError::malformed("invalid legacy string-id framing count"))?;
        if count > super::super::MAX_NAMES {
            return Err(CodecError::malformed(
                "legacy string-id framing count exceeds limit",
            ));
        }
        let mut missing = false;
        for _ in 0..count {
            let before = scanner.at;
            let token = scanner.token_range()?;
            if let Ok(id) = scanner.navigation(token.clone())?.parse::<i64>() {
                missing |= symbols.missing(&[id]);
            }
            fields.push(before..token.start);
        }
        let content_end = scanner.at;
        let end = scanner.whitespace_end()?;
        fields.push(content_end..end);
        let extent = scanner.extent(start..end)?;
        let fields = fields
            .into_iter()
            .filter(|range| !range.is_empty())
            .map(|range| range.start - start..range.end - start)
            .collect::<Vec<_>>();
        let level = if side {
            "freecad.legacy_map_record"
        } else {
            "freecad.legacy_map_inline_record"
        };
        *slot = ctx
            .decode_part(stage, extent, spec(level, extent, &fields)?, |_stage| {
                if missing {
                    return Ok(None);
                }
                let mut tokens = Scanner::new(extent);
                let indexed_name = tokens.token()?.to_owned();
                super::super::split_indexed_name(&indexed_name)?;
                let mapped_name = tokens.token()?.to_owned();
                let count = tokens.count(super::super::MAX_NAMES)?;
                let mut string_ids = ctx.alloc_filled(count, 0, "legacy map string ids")?;
                for id in &mut string_ids {
                    *id = tokens
                        .token()?
                        .parse()
                        .map_err(|_| CodecError::malformed("invalid legacy string id"))?;
                }
                if symbols
                    .known
                    .is_some_and(|ids| string_ids.iter().any(|id| !ids.contains(id)))
                {
                    return Err(CodecError::malformed(
                        "legacy mapped name needs an unavailable persistent string id",
                    ));
                }
                Ok(Some(super::super::LegacyElementRecord {
                    indexed_name,
                    mapped_name,
                    string_ids,
                }))
            })?
            .flatten();
    }
    if !scanner.done()? {
        return Err(CodecError::malformed(
            "legacy element map has trailing data",
        ));
    }
    Ok((count, records))
}

pub(in crate::element_map) fn entry_framing(
    view: View<'_>,
) -> Result<Vec<Range<usize>>, CodecError> {
    let mut scanner = Scanner::new(view);
    let first = scanner.token_range()?;
    if scanner.navigation(first.clone())? != "BeginElementMap" {
        scanner
            .navigation(first.clone())?
            .parse::<usize>()
            .map_err(|_| CodecError::malformed("invalid legacy map entry count"))?;
        return Ok(vec![first]);
    }
    scanner.token_range()?;
    scanner.token_range()?;
    let postfix_marker = scanner.token_range()?;
    if scanner.navigation(postfix_marker)? != "PostfixCount" {
        return Err(CodecError::malformed(
            "element-map postfix locator is absent",
        ));
    }
    let count = scanner.token_range()?;
    let count = scanner
        .navigation(count)?
        .parse::<usize>()
        .map_err(|_| CodecError::malformed("invalid postfix framing count"))?;
    if count > super::super::MAX_NAMES {
        return Err(CodecError::malformed(
            "postfix framing count exceeds its limit",
        ));
    }
    for _ in 0..count {
        scanner.token_range()?;
    }
    let marker = scanner.token_range()?;
    if scanner.navigation(marker.clone())? != "MapCount" {
        return Err(CodecError::malformed("map-node locator is absent"));
    }
    let count = scanner.token_range()?;
    scanner
        .navigation(count.clone())?
        .parse::<usize>()
        .map_err(|_| CodecError::malformed("invalid map-node framing count"))?;
    Ok(vec![marker.start..count.end])
}

// Source selectors establish dependency only. A malformed or never-declared
// selector still reaches its actual leaf getters and produces its own failure.
fn missing_name_dependency(
    encoded: &str,
    postfixes: &[Option<String>],
    symbols: super::StringIds<'_>,
) -> bool {
    let fields = encoded.split('.').collect::<Vec<_>>();
    let Some(base) = fields.first() else {
        return false;
    };
    let postfix_position = if let Some(prefix) = base.strip_prefix(':') {
        if missing_dictionary_slot(prefix, postfixes) {
            return true;
        }
        2
    } else if base.starts_with(';') || base.starts_with('$') {
        1
    } else {
        return false;
    };
    if fields
        .get(postfix_position)
        .is_some_and(|value| missing_dictionary_slot(value, postfixes))
    {
        return true;
    }
    fields.iter().skip(postfix_position + 1).any(|value| {
        super::super::parse_hex(value, "mapped-name string id")
            .is_ok_and(|id| symbols.missing(&[id]))
    })
}

fn missing_dictionary_slot(value: &str, dictionary: &[Option<String>]) -> bool {
    super::super::parse_hex(value, "dictionary selector")
        .ok()
        .and_then(|value| usize::try_from(value).ok())
        .and_then(|value| value.checked_sub(1))
        .is_some_and(|index| dictionary.get(index) == Some(&None))
}
