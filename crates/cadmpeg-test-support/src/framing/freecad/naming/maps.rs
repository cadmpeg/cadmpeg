// SPDX-License-Identifier: Apache-2.0
//! Postfix dictionaries, terminated map nodes and counted node children.

use std::ops::Range;

use cadmpeg_core::decode::{DecodeContext, View};

use super::{emit, retain_push, Tokens};
use crate::framing::{FrameParent, FramedPart};

/// Frame a map side member selected by its actual XML file attribute.
pub fn frame_element_map_entry<'a>(
    ctx: &DecodeContext<'_>,
    source: View<'a>,
    file_attribute: View<'a>,
    zip_parent: usize,
    out: &mut Vec<FramedPart<'a>>,
) -> Result<(), String> {
    let mut locator = Tokens::new(source);
    let first = locator.token()?;
    let modern = &locator.bytes()[first.clone()] == b"BeginElementMap";
    let entry_fields = side_fields(source, modern);
    let (framing, dispatch) = match &entry_fields {
        Ok((framing, dispatch)) => (framing.clone(), dispatch.clone()),
        Err(_) => {
            let dispatch = if modern {
                match locator.token() {
                    Ok(version) => first.start..version.end,
                    Err(_) => first.clone(),
                }
            } else {
                first.clone()
            };
            (vec![], vec![dispatch])
        }
    };
    let absolute = |ranges: Vec<Range<usize>>| -> Result<Vec<Range<u64>>, String> {
        ranges
            .into_iter()
            .map(|range| {
                let field = locator.view(range)?;
                Ok(cadmpeg_core::decode::u64_from_index(field.start())
                    ..cadmpeg_core::decode::u64_from_index(field.end()))
            })
            .collect()
    };
    let frame = FramedPart::new(
        ctx,
        super::super::level("freecad.element_map_entry")?,
        source,
        FrameParent::Part(zip_parent),
        absolute(framing)?,
        absolute(dispatch)?,
    )?
    .with_external_dispatch(ctx, vec![file_attribute])?;
    let parent = out.len();
    super::super::append(ctx, out, frame)?;
    if modern {
        map(ctx, source, true, parent, out)
    } else {
        super::frame_legacy_records(ctx, source, None, parent, out)
    }
}

/// Frame an ElementMap2 character-content stream without a side-member header.
pub fn frame_inline_map<'a>(
    ctx: &DecodeContext<'_>,
    source: View<'a>,
    parent: usize,
    out: &mut Vec<FramedPart<'a>>,
) -> Result<(), String> {
    map(ctx, source, false, parent, out)
}

fn side_fields(
    source: View<'_>,
    modern: bool,
) -> Result<(Vec<Range<usize>>, Vec<Range<usize>>), String> {
    let mut tokens = Tokens::new(source);
    let first = tokens.token()?;
    if !modern {
        return Ok((vec![first.clone()], vec![first]));
    }
    let version = tokens.token()?;
    if tokens.text(version.clone())? != "v1" {
        return Err("element map version does not prove entry framing fields".into());
    }
    tokens.token()?;
    tokens.expect(b"PostfixCount")?;
    let count = tokens.token()?;
    for _ in 0..tokens.count(count)? {
        tokens.token()?;
    }
    let marker = tokens.expect(b"MapCount")?;
    let count = tokens.token()?;
    Ok((
        vec![marker.start..count.end],
        vec![first.start..version.end],
    ))
}

fn map<'a>(
    ctx: &DecodeContext<'_>,
    source: View<'a>,
    side: bool,
    parent: usize,
    out: &mut Vec<FramedPart<'a>>,
) -> Result<(), String> {
    let mut tokens = Tokens::new(source);
    let prefix = if side {
        "freecad.element_map_"
    } else {
        "freecad.inline_map_"
    };
    if side {
        let magic = tokens.expect(b"BeginElementMap")?;
        let version = tokens.token()?;
        let separator = tokens.skip();
        if separator.is_empty() {
            return Err("map header has no closing whitespace".into());
        }
        emit(
            ctx,
            &tokens,
            magic.start..tokens.position,
            "freecad.element_map_header",
            parent,
            vec![tokens.position - 1..tokens.position],
            vec![version.clone()],
            out,
        )?;
        if tokens.text(version)? != "v1" {
            return Err("element map version does not prove a node layout".into());
        }
    }
    let id = tokens.token()?;
    let marker = tokens.expect(b"PostfixCount")?;
    let count = tokens.token()?;
    let mut postfixes = Vec::new();
    for _ in 0..tokens.count(count.clone())? {
        let token = tokens.token()?;
        let gap = tokens.skip();
        retain_push(ctx, &mut postfixes, (token.start..tokens.position, gap))?;
    }
    let dictionary = emit(
        ctx,
        &tokens,
        id.start..tokens.position,
        &format!("{prefix}postfix_dictionary"),
        parent,
        vec![marker.start..count.end],
        vec![],
        out,
    )?;
    for (extent, separator) in postfixes {
        emit(
            ctx,
            &tokens,
            extent,
            &format!("{prefix}postfix"),
            dictionary,
            vec![separator],
            vec![],
            out,
        )?;
    }
    let map_count = tokens.expect(b"MapCount")?;
    let count = tokens.token()?;
    let nodes = tokens.count(count.clone())?;
    let mut failure = None;
    for ordinal in 0..nodes {
        let marker = tokens.expect(b"ElementMap")?;
        let start = if ordinal == 0 {
            map_count.start
        } else {
            marker.start
        };
        tokens.token()?;
        tokens.token()?;
        let group_count = tokens.token()?;
        let body_start = tokens.position;
        let mut closing = None;
        loop {
            match tokens.token() {
                Ok(token) if &tokens.bytes()[token.clone()] == b"EndMap" => {
                    tokens.skip();
                    closing = Some(token.start..tokens.position);
                    break;
                }
                Ok(_) => {}
                Err(_) => break,
            }
        }
        let mut framing = Vec::new();
        if ordinal == 0 {
            framing.push(map_count.start..count.end);
        }
        framing.push(marker.start..group_count.end);
        if let Some(end) = &closing {
            framing.push(end.clone());
        }
        let node = emit(
            ctx,
            &tokens,
            start..tokens.position,
            &format!("{prefix}node"),
            parent,
            framing,
            vec![],
            out,
        )?;
        let result = groups(ctx, &tokens, body_start, group_count, &prefix, node, out);
        if let Err(error) = result {
            if failure.is_none() {
                failure = Some(error);
            }
        }
        if closing.is_none() {
            return Err("map node ends before EndMap".into());
        }
    }
    tokens.skip();
    if tokens.position != tokens.bytes().len() {
        return Err("unassigned element map tail".into());
    }
    match failure {
        Some(error) => Err(error),
        None => Ok(()),
    }
}

fn groups<'a>(
    ctx: &DecodeContext<'_>,
    original: &Tokens<'a>,
    body_start: usize,
    group_count: Range<usize>,
    prefix: &str,
    node: usize,
    out: &mut Vec<FramedPart<'a>>,
) -> Result<(), String> {
    let mut tokens = Tokens {
        source: original.view(0..original.position)?,
        position: body_start,
    };
    for _ in 0..tokens.count(group_count)? {
        let indexed_name = tokens.token()?;
        let child_marker = tokens.expect(b"ChildCount")?;
        let count = tokens.token()?;
        let mut framing = vec![child_marker.start..count.end];
        let mut children = Vec::new();
        for _ in 0..tokens.count(count)? {
            let first = tokens.token()?;
            for _ in 1..7 {
                tokens.token()?;
            }
            let separator = tokens.skip();
            retain_push(
                ctx,
                &mut children,
                (first.start..tokens.position, separator),
            )?;
        }
        let name_marker = tokens.expect(b"NameCount")?;
        let count = tokens.token()?;
        framing.push(name_marker.start..count.end);
        let mut chains = Vec::new();
        for _ in 0..tokens.count(count)? {
            tokens.skip();
            let start = tokens.position;
            loop {
                let token = tokens.token()?;
                if &tokens.bytes()[token.clone()] == b"0" {
                    tokens.skip();
                    retain_push(ctx, &mut chains, (start..tokens.position, token))?;
                    break;
                }
            }
        }
        let group = emit(
            ctx,
            &tokens,
            indexed_name.start..tokens.position,
            &format!("{prefix}group"),
            node,
            framing,
            vec![indexed_name],
            out,
        )?;
        for (extent, separator) in children {
            emit(
                ctx,
                &tokens,
                extent,
                &format!("{prefix}child"),
                group,
                vec![separator],
                vec![],
                out,
            )?;
        }
        for (extent, terminator) in chains {
            emit(
                ctx,
                &tokens,
                extent,
                &format!("{prefix}name_chain"),
                group,
                vec![terminator],
                vec![],
                out,
            )?;
        }
    }
    tokens.expect(b"EndMap")?;
    Ok(())
}
