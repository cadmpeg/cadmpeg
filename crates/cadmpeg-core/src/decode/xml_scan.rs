// SPDX-License-Identifier: Apache-2.0
//! Recovering XML element framing without semantic parsing or read attribution.

use std::collections::HashMap;

use super::{u64_from_index, DecodeContext, ResourceDimension, ResourceLimit, View};
use crate::CodecError;

mod lex;

/// Explicit source code-unit encoding. No decoded replacement buffer is made.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum XmlEncoding {
    /// ASCII framing in UTF-8 source bytes.
    Utf8,
    /// ASCII framing in little-endian two-byte code units.
    Utf16Le,
    /// ASCII framing in big-endian two-byte code units.
    Utf16Be,
}
impl XmlEncoding {
    fn width(self) -> usize {
        if self == Self::Utf8 {
            1
        } else {
            2
        }
    }
}

/// The independently observed boundary of one reached element.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum XmlTermination {
    /// The source contains the empty-element marker `/>`.
    EmptyElement,
    /// The source contains a matching closing name outside shielded markup.
    EndTag,
    /// A containing element closed before this element's end tag was found.
    AncestorBoundary,
    /// The source window ended before a matching end tag was found.
    EndOfInput,
}

/// A source-framing fault, without a claim about decoded XML semantics.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum XmlFramingIssue {
    /// A tag header cannot be framed as a start or end tag.
    MalformedTag,
    /// An opened tag or quoted attribute has no closing marker.
    UnterminatedTag,
    /// A comment, CDATA section or processing instruction has no terminator.
    UnterminatedMarkup,
    /// A declaration or internal subset has no closing marker.
    UnterminatedDeclaration,
    /// A closing name has no active opening name.
    UnexpectedEndTag,
    /// A declaration appeared within an element.
    DeclarationInElement,
    /// The last UTF-16 code unit lacks one source byte.
    IncompleteCodeUnit,
}

/// Exact source region whose XML framing failed.
#[derive(Debug, Clone, Copy)]
pub struct XmlFramingFault<'a> {
    extent: View<'a>,
    issue: XmlFramingIssue,
}
impl<'a> XmlFramingFault<'a> {
    /// Bounded bytes containing the framing fault.
    pub fn extent(self) -> View<'a> {
        self.extent
    }
    /// Observed framing fault.
    pub fn issue(self) -> XmlFramingIssue {
        self.issue
    }
}

/// A reached element and its source framing. This is not XML validation.
#[derive(Debug, Clone, Copy)]
pub struct XmlElement<'a> {
    extent: View<'a>,
    name: View<'a>,
    opening_prefix: View<'a>,
    opening_suffix: View<'a>,
    closing: Option<View<'a>>,
    parent: Option<usize>,
    previous_name: Option<usize>,
    termination: XmlTermination,
    nesting_damage: bool,
}
impl<'a> XmlElement<'a> {
    /// Element bytes including actual start and any actual end marker.
    pub fn extent(self) -> View<'a> {
        self.extent
    }
    /// Encoded source name, with no transcoding or semantic read attribution.
    pub fn name(self) -> View<'a> {
        self.name
    }
    /// Index of the enclosing reached element in this scan.
    pub fn parent(self) -> Option<usize> {
        self.parent
    }
    /// Actual or cut-short boundary fact.
    pub fn termination(self) -> XmlTermination {
        self.termination
    }
    /// Whether unmatched or malformed closing markup interrupted this element.
    pub fn nesting_damage(self) -> bool {
        self.nesting_damage
    }
    /// `<Name`, start-tag `>` or `/>`, and the matching end tag if present.
    pub fn framing(self) -> impl Iterator<Item = View<'a>> {
        [
            Some(self.opening_prefix),
            Some(self.opening_suffix),
            self.closing,
        ]
        .into_iter()
        .flatten()
    }
}

/// Reached XML elements in start-tag order and any unassigned framing faults.
#[derive(Debug)]
pub struct XmlElementScan<'a> {
    elements: Vec<XmlElement<'a>>,
    faults: Vec<XmlFramingFault<'a>>,
}
impl<'a> XmlElementScan<'a> {
    /// Borrow elements in source opening order.
    pub fn elements(&self) -> &[XmlElement<'a>] {
        &self.elements
    }
    /// Borrow source regions that could not be assigned an element boundary.
    pub fn faults(&self) -> &[XmlFramingFault<'a>] {
        &self.faults
    }
}

impl DecodeContext<'_> {
    /// Frames elements iteratively, before a decoder parses individual parts.
    ///
    /// Comments, CDATA, processing instructions, quotes and declaration subsets
    /// shield apparent tags. Missing delimiters are reported, not guessed.
    /// Returned Views keep source identity; this scan adds no semantic reads.
    /// Node and result storage use the caller session budget. A framing node is
    /// not an entity: the decoder charges `max_entities` for the records it
    /// builds from an element, so the scan charges none. The strict parser remains
    /// responsible for XML validity and per-element parse-depth limits.
    pub fn scan_xml_elements<'a>(
        &self,
        source: View<'a>,
        encoding: XmlEncoding,
        operation: &'static str,
    ) -> Result<XmlElementScan<'a>, CodecError> {
        self.require_source(source)?;
        let mut scan = XmlElementScan {
            elements: Vec::new(),
            faults: Vec::new(),
        };
        let mut active: HashMap<&[u8], usize> = HashMap::new();
        let mut open = None;
        let mut lexer = lex::Lexer::new(source.window(), encoding);
        while let Some(token) = lexer.next() {
            let extent = child(source, token.start, token.end)?;
            match token.kind {
                lex::Kind::Start {
                    name,
                    suffix,
                    empty,
                } => {
                    reserve(self, &mut scan.elements, operation)?;
                    let name_end = name.1;
                    let name = child(source, name.0, name.1)?;
                    let index = scan.elements.len();
                    let previous_name = if empty {
                        None
                    } else {
                        admit_name(self, &mut active, name.window(), operation)?;
                        active.insert(name.window(), index)
                    };
                    scan.elements.push(XmlElement {
                        extent,
                        name,
                        opening_prefix: child(source, token.start, name_end)?,
                        opening_suffix: child(source, suffix, token.end)?,
                        closing: None,
                        parent: open,
                        previous_name,
                        termination: if empty {
                            XmlTermination::EmptyElement
                        } else {
                            XmlTermination::EndOfInput
                        },
                        nesting_damage: false,
                    });
                    if !empty {
                        open = Some(index);
                    }
                }
                lex::Kind::End { name, clean } => {
                    let name = child(source, name.0, name.1)?;
                    if let Some(&matching) = active.get(name.window()) {
                        let interrupted = open != Some(matching);
                        while let Some(index) = open {
                            let node = &mut scan.elements[index];
                            open = node.parent;
                            if let Some(previous) = node.previous_name {
                                active.insert(node.name.window(), previous);
                            } else {
                                active.remove(node.name.window());
                            }
                            let end = if index == matching {
                                token.end
                            } else {
                                token.start
                            };
                            let start =
                                node.extent.start().checked_sub(source.start()).ok_or_else(
                                    || CodecError::malformed("XML element start precedes source"),
                                )?;
                            node.extent = child(source, start, end)?;
                            node.nesting_damage |= interrupted || !clean;
                            if index == matching {
                                node.closing = Some(extent);
                                node.termination = XmlTermination::EndTag;
                                break;
                            }
                            node.termination = XmlTermination::AncestorBoundary;
                        }
                    } else {
                        fault(
                            self,
                            &mut scan,
                            extent,
                            XmlFramingIssue::UnexpectedEndTag,
                            operation,
                        )?;
                        if let Some(index) = open {
                            scan.elements[index].nesting_damage = true;
                        }
                    }
                }
                lex::Kind::Declaration if open.is_some() => {
                    fault(
                        self,
                        &mut scan,
                        extent,
                        XmlFramingIssue::DeclarationInElement,
                        operation,
                    )?;
                    if let Some(index) = open {
                        scan.elements[index].nesting_damage = true;
                    }
                }
                lex::Kind::Bad(issue) => {
                    fault(self, &mut scan, extent, issue, operation)?;
                    if let Some(index) = open {
                        scan.elements[index].nesting_damage = true;
                    }
                }
                lex::Kind::Ignored | lex::Kind::Declaration => {}
            }
        }
        while let Some(index) = open {
            let node = &mut scan.elements[index];
            node.extent = source
                .child(node.extent.start(), source.end())
                .ok_or_else(|| CodecError::malformed("XML cut-short extent escaped its source"))?;
            node.nesting_damage = true;
            open = node.parent;
        }
        Ok(scan)
    }
}

fn child(source: View<'_>, start: usize, end: usize) -> Result<View<'_>, CodecError> {
    let start = source
        .start()
        .checked_add(start)
        .ok_or_else(|| CodecError::malformed("XML source offset overflows"))?;
    let end = source
        .start()
        .checked_add(end)
        .ok_or_else(|| CodecError::malformed("XML source end overflows"))?;
    source
        .child(start, end)
        .ok_or_else(|| CodecError::malformed("XML framing escaped its source"))
}

fn fault<'a>(
    ctx: &DecodeContext<'_>,
    scan: &mut XmlElementScan<'a>,
    extent: View<'a>,
    issue: XmlFramingIssue,
    operation: &'static str,
) -> Result<(), CodecError> {
    reserve(ctx, &mut scan.faults, operation)?;
    scan.faults.push(XmlFramingFault { extent, issue });
    Ok(())
}

fn reserve<T>(
    ctx: &DecodeContext<'_>,
    values: &mut Vec<T>,
    operation: &'static str,
) -> Result<(), CodecError> {
    if values.len() == values.capacity() {
        let additional = values.capacity().max(1);
        let bytes = additional
            .checked_mul(size_of::<T>())
            .ok_or_else(|| CodecError::malformed("XML scan storage overflows"))?;
        ctx.retain_bytes(u64_from_index(bytes), operation)?;
        values
            .try_reserve_exact(additional)
            .map_err(|_| allocation(ctx, u64_from_index(bytes), operation))?;
    }
    Ok(())
}

fn admit_name(
    ctx: &DecodeContext<'_>,
    names: &mut HashMap<&[u8], usize>,
    name: &[u8],
    operation: &'static str,
) -> Result<(), CodecError> {
    if !names.contains_key(name) {
        // Four initial borrowed-name buckets (24 bytes each), controls and alignment.
        let estimate = names
            .len()
            .checked_add(1)
            .and_then(|n| n.checked_mul(128))
            .ok_or_else(|| CodecError::malformed("XML name-index storage overflows"))?;
        let allowance = ctx.per_expand_allowance();
        if u64_from_index(estimate) > allowance {
            return Err(ctx.refuse_part_limit(
                ResourceDimension::RetainedBytes,
                allowance,
                0,
                u64_from_index(estimate),
                operation,
            ));
        }
        names
            .try_reserve(1)
            .map_err(|_| allocation(ctx, u64_from_index(estimate), operation))?;
    }
    Ok(())
}

fn allocation(ctx: &DecodeContext<'_>, bytes: u64, operation: &'static str) -> CodecError {
    ResourceLimit::allocation_failed(
        ResourceDimension::RetainedBytes,
        ctx.policy().limits.max_retained_bytes,
        bytes,
        operation,
    )
    .into()
}

#[cfg(test)]
mod tests;
