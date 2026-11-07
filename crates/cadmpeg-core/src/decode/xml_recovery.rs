// SPDX-License-Identifier: Apache-2.0
//! Recovering XML admission at element boundaries.

use super::{DecodeContext, View, XmlElementScan, XmlEncoding, XmlFramingIssue, XmlTermination};
use crate::CodecError;
use std::collections::BTreeSet;

type DamagedSpan = (usize, usize, Vec<u8>, Vec<Vec<u8>>);

/// Parse XML while reporting each element removed to recover its siblings.
pub fn parse_xml_recovering<'s>(
    ctx: &DecodeContext<'s>,
    view: View<'s>,
    member: &'static str,
    operation: &'static str,
    parse_operation: &'static str,
    shape_error: &dyn Fn(String) -> CodecError,
    mut give_up: impl FnMut(View<'s>, &[u8], &[Vec<u8>], &XmlElementScan<'s>) -> Result<(), CodecError>,
) -> Result<(View<'s>, roxmltree::Document<'s>), CodecError> {
    let parse = |text: &'s str| match ctx.parse_xml_located(text, parse_operation) {
        Ok(xml) => Ok(xml),
        Err((CodecError::Malformed(detail), position)) => Err((shape_error(detail), position)),
        Err((error, position)) => Err((error, position)),
    };
    let scan = ctx.scan_xml_elements(view, XmlEncoding::Utf8, operation)?;
    let mut blanked: Vec<(usize, usize)> = Vec::new();
    loop {
        let current = reassemble(ctx, view, &blanked, operation)?;
        let parsed = match std::str::from_utf8(current.window()) {
            Ok(text) => parse(text).map_err(|(error, position)| {
                let damaged = position
                    .and_then(|position| syntax_damage(&scan, view, text, position, &blanked));
                (error, damaged)
            }),
            Err(error) => {
                let damaged = view
                    .start()
                    .checked_add(error.valid_up_to())
                    .and_then(|at| element_at(&scan, at, &blanked));
                Err((
                    CodecError::malformed(format_args!("{member} is not UTF-8")),
                    damaged,
                ))
            }
        };
        match parsed {
            Ok(xml) => return Ok((current, xml)),
            Err((error, damaged)) => {
                if matches!(error, CodecError::ResourceLimit(_)) {
                    return Err(error);
                }
                let mut fresh = damaged_elements(&scan, &blanked);
                if fresh.is_empty() {
                    if let Some(element) = damaged {
                        fresh.push(element);
                    } else {
                        return Err(error);
                    }
                }
                let mut merged = blanked.clone();
                for (start, end, name, ancestors) in &fresh {
                    let extent = view.child(*start, *end).ok_or_else(|| {
                        CodecError::malformed("damaged XML element escapes its member")
                    })?;
                    give_up(extent, name, ancestors, &scan)?;
                    merged.push((*start, *end));
                }
                merged.sort_unstable();
                let mut collapsed: Vec<(usize, usize)> = Vec::new();
                for (start, end) in merged {
                    if let Some(last) = collapsed.last_mut() {
                        if start <= last.1 {
                            last.1 = last.1.max(end);
                            continue;
                        }
                    }
                    collapsed.push((start, end));
                }
                blanked = collapsed;
            }
        }
    }
}

/// Rebuild the member with every blanked span replaced by spaces.
///
/// Element spans become whitespace between their siblings, so the surviving
/// structure keeps its well-formedness and every surviving byte keeps its
/// source offset relative to the rebuilt view's own window.
fn reassemble<'s>(
    ctx: &DecodeContext<'s>,
    view: View<'s>,
    blanked: &[(usize, usize)],
    operation: &'static str,
) -> Result<View<'s>, CodecError> {
    if blanked.is_empty() {
        return Ok(view);
    }
    let mut cursor = view.start();
    let mut widest = 0_usize;
    for (start, end) in blanked {
        if *start < cursor || *end > view.end() || *start >= *end {
            return Err(CodecError::malformed(
                "damaged XML element escapes its member",
            ));
        }
        widest = widest.max(end - start);
        cursor = cursor.max(*end);
    }
    ctx.retain_bytes(super::u64_from_index(widest), operation)?;
    let padding = ctx.alloc_filled(widest, b' ', "damaged XML element blanks")?;
    let mut blanks: Vec<(usize, usize, View<'_>)> = Vec::new();
    for (start, end) in blanked {
        let width = end - start;
        blanks.push((*start, *end, View::over_retained(&padding[..width])));
    }
    let mut ordered: Vec<View<'_>> = Vec::new();
    let mut cursor = view.start();
    for (start, end, blank) in &blanks {
        if *start > cursor {
            let Some(piece) = view.child(cursor, *start) else {
                return Err(CodecError::malformed(
                    "surviving XML span escapes its member",
                ));
            };
            ordered.push(piece);
        }
        ordered.push(*blank);
        cursor = cursor.max(*end);
    }
    if cursor < view.end() {
        let Some(piece) = view.child(cursor, view.end()) else {
            return Err(CodecError::malformed(
                "surviving XML span escapes its member",
            ));
        };
        ordered.push(piece);
    }
    ctx.concat_views(&ordered)
}

/// Locate a syntax fault inside the innermost framed element. A root fault
/// has no containing element to preserve, so it remains a document failure.
fn syntax_damage(
    scan: &super::XmlElementScan<'_>,
    view: View<'_>,
    text: &str,
    position: roxmltree::TextPos,
    blanked: &[(usize, usize)],
) -> Option<DamagedSpan> {
    let row = usize::try_from(position.row).ok()?.checked_sub(1)?;
    let column = usize::try_from(position.col).ok()?.checked_sub(1)?;
    let mut offset = 0;
    let line = text
        .split_inclusive('\n')
        .enumerate()
        .find_map(|(index, line)| {
            if index == row {
                Some(line)
            } else {
                offset += line.len();
                None
            }
        })?;
    let character = line
        .char_indices()
        .nth(column)
        .map_or(line.len(), |(index, _)| index);
    let at = view.start().checked_add(offset)?.checked_add(character)?;
    element_at(scan, at, blanked)
}

fn element_at(
    scan: &super::XmlElementScan<'_>,
    at: usize,
    blanked: &[(usize, usize)],
) -> Option<DamagedSpan> {
    let elements = scan.elements();
    let (index, element) = elements
        .iter()
        .enumerate()
        .filter(|(_, element)| {
            let extent = element.extent();
            extent.start() <= at
                && at < extent.end()
                && !blanked
                    .iter()
                    .any(|(start, end)| *start <= extent.start() && extent.end() <= *end)
        })
        .min_by_key(|(_, element)| element.extent().end() - element.extent().start())?;
    element.parent()?;
    Some((
        element.extent().start(),
        element.extent().end(),
        element_name(element),
        ancestors(elements, index),
    ))
}

/// Select the damaged elements one recovering pass blanks.
///
/// Every fault is charged to its innermost containing element, so a damaged
/// element costs that element and its children only. A fault strictly between
/// elements blanks its own span when the span is a complete syntactic unit;
/// an unterminated fault blanks its innermost element instead. Elements
/// already blanked never repeat.
fn damaged_elements(
    scan: &super::XmlElementScan<'_>,
    blanked: &[(usize, usize)],
) -> Vec<DamagedSpan> {
    let elements = scan.elements();
    let contains =
        |span: (usize, usize), inner: (usize, usize)| span.0 <= inner.0 && inner.1 <= span.1;
    let is_blanked = |span: (usize, usize)| blanked.iter().any(|blank| contains(*blank, span));
    let innermost = |span: (usize, usize)| {
        elements
            .iter()
            .enumerate()
            .filter(|(_, element)| {
                contains((element.extent().start(), element.extent().end()), span)
            })
            .min_by_key(|(_, element)| element.extent().end() - element.extent().start())
            .map(|(index, _)| index)
    };
    let mut selected: BTreeSet<usize> = BTreeSet::new();
    let mut gap_spans: Vec<(usize, usize)> = Vec::new();
    for fault in scan.faults() {
        let span = (fault.extent().start(), fault.extent().end());
        let Some(index) = innermost(span) else {
            continue;
        };
        let extent = elements[index].extent();
        let own = (extent.start(), extent.end());
        if own == span {
            selected.insert(index);
            continue;
        }
        let inside_child = elements.iter().enumerate().any(|(child, element)| {
            child != index
                && element.parent() == Some(index)
                && contains((element.extent().start(), element.extent().end()), span)
        });
        if inside_child {
            let Some(deep) = innermost(span).and_then(|found| {
                (found != index
                    && !is_blanked((
                        elements[found].extent().start(),
                        elements[found].extent().end(),
                    )))
                .then_some(found)
            }) else {
                selected.insert(index);
                continue;
            };
            selected.insert(deep);
            continue;
        }
        match fault.issue() {
            XmlFramingIssue::UnexpectedEndTag
            | XmlFramingIssue::DeclarationInElement
            | XmlFramingIssue::MalformedTag => {
                gap_spans.push(span);
            }
            XmlFramingIssue::UnterminatedTag
            | XmlFramingIssue::UnterminatedMarkup
            | XmlFramingIssue::UnterminatedDeclaration
            | XmlFramingIssue::IncompleteCodeUnit => {
                selected.insert(index);
            }
        }
    }
    for (index, element) in elements.iter().enumerate() {
        if matches!(
            element.termination(),
            XmlTermination::EndOfInput | XmlTermination::AncestorBoundary
        ) || element.nesting_damage()
        {
            let own = (element.extent().start(), element.extent().end());
            let faulted = scan
                .faults()
                .iter()
                .any(|fault| contains(own, (fault.extent().start(), fault.extent().end())));
            if !faulted && element.termination() != XmlTermination::AncestorBoundary {
                continue;
            }
            if is_blanked(own) {
                continue;
            }
            let covered = selected.iter().any(|other| {
                *other != index
                    && contains(
                        own,
                        (
                            elements[*other].extent().start(),
                            elements[*other].extent().end(),
                        ),
                    )
            });
            if !covered {
                selected.insert(index);
            }
        }
    }
    let mut fresh: Vec<DamagedSpan> = Vec::new();
    let mut previous_end = None;
    for index in &selected {
        let own = (
            elements[*index].extent().start(),
            elements[*index].extent().end(),
        );
        if is_blanked(own) || previous_end.is_some_and(|end| own.1 <= end) {
            continue;
        }
        previous_end = Some(own.1);
        fresh.push((
            own.0,
            own.1,
            element_name(&elements[*index]),
            ancestors(elements, *index),
        ));
    }
    for span in gap_spans {
        if is_blanked(span) {
            continue;
        }
        if fresh
            .iter()
            .any(|existing| contains((existing.0, existing.1), span))
        {
            continue;
        }
        fresh.push((span.0, span.1, b"markup".to_vec(), Vec::new()));
    }
    fresh.sort_unstable();
    fresh.dedup();
    fresh
}

/// The element's tag name without any namespace prefix.
fn element_name(element: &super::XmlElement<'_>) -> Vec<u8> {
    let name = element.name().window();
    name.rsplit(|byte| *byte == b':')
        .next()
        .unwrap_or(name)
        .to_vec()
}

/// Ancestor names from the parent up to the root, without namespace prefixes.
fn ancestors(elements: &[super::XmlElement<'_>], index: usize) -> Vec<Vec<u8>> {
    let mut names = Vec::new();
    let mut parent = elements[index].parent();
    while let Some(current) = parent {
        let Some(element) = elements.get(current) else {
            break;
        };
        names.push(element_name(element));
        parent = element.parent();
    }
    names
}

#[cfg(test)]
mod tests {
    use super::super::{DecodeArena, DecodePolicy};
    use super::*;

    #[test]
    fn missing_child_end_tag_selects_the_child() {
        let bytes = b"<root><parent><broken></parent><sibling/></root>";
        let arena = DecodeArena::new();
        let (ctx, view) = DecodeContext::from_root_bytes(bytes, &arena, &DecodePolicy::service())
            .expect("create decode context");
        let scan = ctx
            .scan_xml_elements(view, XmlEncoding::Utf8, "test")
            .expect("scan XML elements");
        let damaged = damaged_elements(&scan, &[]);
        assert_eq!(damaged.len(), 1);
        assert_eq!(&bytes[damaged[0].0..damaged[0].1], b"<broken>");
    }

    #[test]
    fn interrupted_nested_elements_are_given_up_once() {
        let bytes = b"<root><keep/><parent><child></root>";
        let arena = DecodeArena::new();
        let (ctx, view) = DecodeContext::from_root_bytes(bytes, &arena, &DecodePolicy::service())
            .expect("create decode context");
        let mut damaged = Vec::new();
        let (_, document) = parse_xml_recovering(
            &ctx,
            view,
            "test XML",
            "scan test XML",
            "parse test XML",
            &|detail| CodecError::malformed(detail),
            |extent, name, _, _| {
                damaged.push((extent.window().to_vec(), name.to_vec()));
                Ok(())
            },
        )
        .expect("recover readable sibling");
        assert_eq!(damaged, [(b"<parent><child>".to_vec(), b"parent".to_vec())]);
        assert!(document
            .root_element()
            .children()
            .any(|node| node.has_tag_name("keep")));
    }

    #[test]
    fn interrupted_siblings_remain_distinct_parts() {
        let bytes = b"<root><first><bad></first><keep/><second><nested></root>";
        let arena = DecodeArena::new();
        let (ctx, view) = DecodeContext::from_root_bytes(bytes, &arena, &DecodePolicy::service())
            .expect("create decode context");
        let mut damaged = Vec::new();
        let (_, document) = parse_xml_recovering(
            &ctx,
            view,
            "test XML",
            "scan test XML",
            "parse test XML",
            &|detail| CodecError::malformed(detail),
            |extent, name, _, _| {
                damaged.push((extent.window().to_vec(), name.to_vec()));
                Ok(())
            },
        )
        .expect("recover readable siblings");
        assert_eq!(
            damaged,
            [
                (b"<bad>".to_vec(), b"bad".to_vec()),
                (b"<second><nested>".to_vec(), b"second".to_vec()),
            ]
        );
        assert!(document
            .root_element()
            .children()
            .any(|node| node.has_tag_name("keep")));
    }

    #[test]
    fn recovers_siblings_and_source_offsets_after_unicode() {
        let text = "<root><first>猫😀</first><bad a=\"1\" a=\"2\"/><last/></root>";
        let arena = DecodeArena::new();
        let (ctx, view) =
            DecodeContext::from_root_bytes(text.as_bytes(), &arena, &DecodePolicy::service())
                .expect("create decode context");
        let mut dropped = Vec::new();
        let (recovered, xml) = parse_xml_recovering(
            &ctx,
            view,
            "test XML",
            "scan test XML",
            "parse test XML",
            &|detail| CodecError::malformed(detail),
            |extent, name, ancestors, _| {
                dropped.push((
                    extent.start(),
                    extent.window().to_vec(),
                    name.to_vec(),
                    ancestors.to_vec(),
                ));
                Ok(())
            },
        )
        .expect("recover XML");
        let names: Vec<_> = xml
            .root_element()
            .children()
            .filter(roxmltree::Node::is_element)
            .map(|node| node.tag_name().name())
            .collect();
        assert_eq!(names, ["first", "last"]);
        let bad = "<bad a=\"1\" a=\"2\"/>";
        assert_eq!(
            dropped,
            [(
                text.find(bad).expect("bad element"),
                bad.as_bytes().to_vec(),
                b"bad".to_vec(),
                vec![b"root".to_vec()]
            )]
        );
        let last = xml
            .root_element()
            .last_element_child()
            .expect("last sibling");
        assert_eq!(
            last.range().start,
            text.find("<last/>").expect("last offset")
        );
        assert_eq!(recovered.window().len(), text.len());
    }

    #[test]
    fn recovers_siblings_around_invalid_utf8() {
        let bytes = b"prefix<root><first/><bad>\xff</bad><last/></root>suffix";
        let arena = DecodeArena::new();
        let (ctx, root) = DecodeContext::from_root_bytes(bytes, &arena, &DecodePolicy::service())
            .expect("create decode context");
        let start = b"prefix".len();
        let end = bytes.len() - b"suffix".len();
        let member = root.child(start, end).expect("member view");
        let mut dropped = Vec::new();
        let (_, document) = parse_xml_recovering(
            &ctx,
            member,
            "test XML",
            "scan test XML",
            "parse test XML",
            &|detail| CodecError::malformed(detail),
            |extent, name, _, _| {
                dropped.push((extent.window().to_vec(), name.to_vec()));
                Ok(())
            },
        )
        .expect("recover XML");
        let names: Vec<_> = document
            .root_element()
            .children()
            .filter(roxmltree::Node::is_element)
            .map(|node| node.tag_name().name())
            .collect();
        assert_eq!(names, ["first", "last"]);
        assert_eq!(dropped, [(b"<bad>\xff</bad>".to_vec(), b"bad".to_vec())]);
    }

    #[test]
    fn recovers_multiple_errors_in_a_bounded_member() {
        let xml = "<root><first>猫</first><bad a=\"1\" a=\"2\"/>\n<middle/><bad b=\"1\" b=\"2\"/><last/></root>";
        let source = format!("prefix{xml}suffix");
        let arena = DecodeArena::new();
        let (ctx, root) =
            DecodeContext::from_root_bytes(source.as_bytes(), &arena, &DecodePolicy::service())
                .expect("create decode context");
        let start = source.find(xml).expect("member offset");
        let member = root.child(start, start + xml.len()).expect("member view");
        let mut damaged = Vec::new();
        let (_, document) = parse_xml_recovering(
            &ctx,
            member,
            "test XML",
            "scan test XML",
            "parse test XML",
            &|detail| CodecError::malformed(detail),
            |extent, name, _, _| {
                damaged.push((extent.start(), extent.window().to_vec(), name.to_vec()));
                Ok(())
            },
        )
        .expect("recover XML");
        let names: Vec<_> = document
            .root_element()
            .children()
            .filter(roxmltree::Node::is_element)
            .map(|node| node.tag_name().name())
            .collect();
        assert_eq!(names, ["first", "middle", "last"]);
        let first_bad = "<bad a=\"1\" a=\"2\"/>";
        let second_bad = "<bad b=\"1\" b=\"2\"/>";
        assert_eq!(
            damaged,
            [
                (
                    source.find(first_bad).expect("first element"),
                    first_bad.as_bytes().to_vec(),
                    b"bad".to_vec()
                ),
                (
                    source.find(second_bad).expect("second element"),
                    second_bad.as_bytes().to_vec(),
                    b"bad".to_vec()
                ),
            ]
        );
    }

    #[test]
    fn resource_refusal_does_not_give_up_an_xml_element() {
        use std::fmt::Write;

        let mut xml = String::from("<root><large");
        for index in 0..257 {
            write!(xml, " a{index}='x'").expect("write attribute");
        }
        xml.push_str("/></root>");
        let arena = DecodeArena::new();
        let (ctx, view) =
            DecodeContext::from_root_bytes(xml.as_bytes(), &arena, &DecodePolicy::service())
                .expect("create decode context");
        let mut given_up = false;
        let outcome = ctx
            .run_part(|| {
                parse_xml_recovering(
                    &ctx,
                    view,
                    "test XML",
                    "scan test XML",
                    "parse test XML",
                    &|detail| CodecError::malformed(detail),
                    |_, _, _, _| {
                        given_up = true;
                        Ok(())
                    },
                )
                .map(|_| ())
            })
            .expect("part refusal is recoverable");
        assert!(matches!(outcome, Err(CodecError::ResourceLimit(_))));
        assert!(!given_up);
        let good = View::over_retained(b"<root><sibling/></root>");
        let (_, document) = parse_xml_recovering(
            &ctx,
            good,
            "test XML",
            "scan test XML",
            "parse test XML",
            &|detail| CodecError::malformed(detail),
            |_, _, _, _| Ok(()),
        )
        .expect("decode next part");
        assert!(document
            .descendants()
            .any(|node| node.has_tag_name("sibling")));
    }
}
