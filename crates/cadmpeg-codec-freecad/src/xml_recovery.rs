// SPDX-License-Identifier: Apache-2.0
//! Recovering element-level XML admission.
//!
//! The strict parser reads a member all or nothing. This helper splits the
//! member at element boundaries with the recovering element scanner before
//! parsing: each damaged element is given up as its own part while the
//! surviving elements still decode. The surviving bytes are reassembled from
//! the original member views, so every admitted range keeps its source
//! offsets and the strict parser still owns XML validity.

use std::collections::BTreeSet;

use cadmpeg_core::decode::{DecodeContext, View, XmlEncoding, XmlFramingIssue, XmlTermination};
use cadmpeg_core::CodecError;
use cadmpeg_ir::source_fidelity::parts::{DecodeParts, PartOutput, PartSpec};

/// A damaged element's name and its ancestor names, parent first.
pub(crate) type KindMapper = dyn Fn(&[u8], &[&[u8]]) -> PartSpec;

/// Admit one XML member, recovering readable elements around damaged ones.
///
/// The fast path parses the member unchanged. When it fails, the scanner
/// frames the member's elements and each damaged element is blanked out of a
/// reassembled copy while its original bytes are given up as their own part.
/// The returned view is the admitted bytes and the document borrows them, so
/// source ranges stay consistent. A member with no recoverable element
/// returns the original parse error and the caller gives up the whole member.
pub(crate) fn admit_recovering<'s>(
    ctx: &DecodeContext<'s>,
    stage: &mut PartOutput,
    view: View<'s>,
    member: &'static str,
    operation: &'static str,
    parse_operation: &'static str,
    map_kind: &KindMapper,
    shape_error: &dyn Fn(String) -> CodecError,
) -> Result<(View<'s>, roxmltree::Document<'s>), CodecError> {
    let parse = |text: &'s str| match ctx.parse_xml(text, parse_operation) {
        Ok(xml) => Ok(xml),
        Err(CodecError::Malformed(detail)) => Err(shape_error(detail)),
        Err(error) => Err(error),
    };
    let text = std::str::from_utf8(view.window())
        .map_err(|_| CodecError::malformed(format_args!("{member} is not UTF-8")))?;
    if let Ok(xml) = parse(text) {
        return Ok((view, xml));
    }
    let mut blanked: Vec<(usize, usize)> = Vec::new();
    loop {
        let current = reassemble(ctx, view, &blanked, operation)?;
        let text = std::str::from_utf8(current.window())
            .map_err(|_| CodecError::malformed(format_args!("{member} is not UTF-8")))?;
        match parse(text) {
            Ok(xml) => return Ok((current, xml)),
            Err(error) => {
                let scan = ctx.scan_xml_elements(view, XmlEncoding::Utf8, operation)?;
                let fresh = damaged_elements(&scan, &blanked);
                if fresh.is_empty() {
                    return Err(error);
                }
                let mut merged = blanked.clone();
                for (start, end, name, ancestors) in &fresh {
                    let extent = view.child(*start, *end).ok_or_else(|| {
                        CodecError::malformed("damaged XML element escapes its member")
                    })?;
                    let borrowed: Vec<&[u8]> = ancestors.iter().map(Vec::as_slice).collect();
                    let spec = map_kind(name, &borrowed);
                    let detail = format!(
                        "damaged XML element <{}> cannot be admitted",
                        String::from_utf8_lossy(name)
                    );
                    ctx.decode_part(stage, extent, spec, |_| {
                        Err::<(), _>(CodecError::malformed(detail.clone()))
                    })?;
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
    ctx.retain_bytes(cadmpeg_core::decode::u64_from_index(widest), operation)?;
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

/// Select the damaged elements one recovering pass blanks.
///
/// Every fault is charged to its innermost containing element, so a damaged
/// element costs that element and its children only. A fault strictly between
/// elements blanks its own span when the span is a complete syntactic unit;
/// an unterminated fault blanks its innermost element instead. Elements
/// already blanked never repeat.
fn damaged_elements(
    scan: &cadmpeg_core::decode::XmlElementScan<'_>,
    blanked: &[(usize, usize)],
) -> Vec<(usize, usize, Vec<u8>, Vec<Vec<u8>>)> {
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
            if !faulted {
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
    let mut fresh: Vec<(usize, usize, Vec<u8>, Vec<Vec<u8>>)> = Vec::new();
    for index in selected {
        let own = (
            elements[index].extent().start(),
            elements[index].extent().end(),
        );
        if is_blanked(own) {
            continue;
        }
        fresh.push((
            own.0,
            own.1,
            element_name(elements[index]),
            ancestors(elements, index),
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
fn element_name(element: cadmpeg_core::decode::XmlElement<'_>) -> Vec<u8> {
    let name = element.name().window();
    name.rsplit(|byte| *byte == b':')
        .next()
        .unwrap_or(name)
        .to_vec()
}

/// Ancestor names from the parent up to the root, without namespace prefixes.
fn ancestors(elements: &[cadmpeg_core::decode::XmlElement<'_>], index: usize) -> Vec<Vec<u8>> {
    let mut names = Vec::new();
    let mut parent = elements[index].parent();
    while let Some(current) = parent {
        let Some(element) = elements.get(current) else {
            break;
        };
        names.push(element_name(*element));
        parent = element.parent();
    }
    names
}
