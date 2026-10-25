// SPDX-License-Identifier: Apache-2.0
//! Incremental terminated children inside an original available parent window.

use std::convert::Infallible;

use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::CodecError;

use super::{helper, DecodeParts, PartFraming, PartLevel, PartOutput, PartSpec};

/// Transient lexical completion, never a native failure field or terminator.
#[derive(Debug, Clone, Copy)]
pub struct TerminatedCompletion<'source> {
    closed: Option<View<'source>>,
}

impl<'source> TerminatedCompletion<'source> {
    /// The original closed parent extent, only after every declared delimiter.
    /// Zero declared children close at the actual opening/count frame.
    /// An unterminated EOF result supplies no locator for a following structure.
    pub fn closed_extent(self) -> Option<View<'source>> {
        self.closed
    }
}

/// Source-ordered terminated children of one incrementally admitted container.
///
/// An unreadable marker-bounded child does not stop later marker-bounded peers.
/// An unterminated source-end prefix has no successful row result and stops
/// this container. Its complete prior children remain independently admitted.
#[derive(Debug)]
pub struct TerminatedParts<'source> {
    available: View<'source>,
    level: PartLevel,
    next: usize,
    remaining: usize,
    incomplete: bool,
    stopped: bool,
}

/// An original opening/count frame and its bounded navigation window.
///
/// The window is source framing authority, not a claim that a closing marker
/// exists. Its actual child delimiters or authenticated stream end select the
/// delivered parent extent.
#[derive(Debug)]
pub struct TerminatedParent<'source> {
    prefix: View<'source>,
    spec: PartSpec,
    children: TerminatedParts<'source>,
}

impl<'source> TerminatedParent<'source> {
    /// Bind the actual opening/count frame to a declared terminated child level.
    ///
    /// # Errors
    /// Rejects nonterminated levels, an undeclared parent, a foreign source,
    /// or an opening frame outside the original available navigation window.
    pub fn new(
        ctx: &DecodeContext<'_>,
        prefix: View<'source>,
        available: View<'source>,
        spec: PartSpec,
        child_level: PartLevel,
        declared: usize,
    ) -> Result<Self, CodecError> {
        validate_rule(spec.level, child_level)?;
        ctx.require_source(prefix)?;
        ctx.require_source(available)?;
        if prefix.start() != available.start()
            || prefix.start() == prefix.end()
            || prefix.location().space != available.location().space
            || !available
                .child(prefix.start(), prefix.end())
                .is_some_and(|original| original.window().as_ptr() == prefix.window().as_ptr())
        {
            return Err(CodecError::malformed(
                "terminated opening frame is not its original parent prefix",
            ));
        }
        Ok(Self {
            prefix,
            spec,
            children: TerminatedParts {
                available,
                level: child_level,
                next: prefix.end(),
                remaining: declared,
                incomplete: false,
                stopped: false,
            },
        })
    }

    /// Decode the container fields and each reached child incrementally.
    ///
    /// A complete result captures only through the final actual child delimiter;
    /// zero declared children end at the actual opening/count frame. An
    /// incomplete result delivers a sparse containing callback over its actual
    /// available prefix, with one given-up EOF child. It does not assert that
    /// EOF terminates a row or permit the next structure's locator.
    /// `Decoded` records successful container-field delivery; the transient
    /// completion result separately controls the caller's next locator.
    ///
    /// # Errors
    /// Rejects a callback returning before all declared children were reached.
    /// I/O and Session refusals escape without partial delivery.
    pub fn decode<T>(
        mut self,
        ctx: &DecodeContext<'_>,
        output: &mut PartOutput,
        decode: impl FnOnce(&mut PartOutput, &mut TerminatedParts<'source>) -> Result<T, CodecError>,
    ) -> Result<Option<(T, TerminatedCompletion<'source>)>, CodecError> {
        let available = self.children.available;
        let result = helper::decode_discovered_part(
            ctx,
            output,
            self.prefix,
            self.spec,
            available,
            |stage| {
                let value = decode(stage, &mut self.children)?;
                let (complete, end) = if self.children.incomplete {
                    (false, available.end())
                } else if self.children.remaining == 0 && !self.children.stopped {
                    (true, self.children.next)
                } else {
                    return Err(CodecError::malformed(
                        "terminated container returned before its declared children",
                    ));
                };
                let consumed = available.child(available.start(), end).ok_or_else(|| {
                    CodecError::malformed("terminated container escapes its original window")
                })?;
                let completion = TerminatedCompletion {
                    closed: complete.then_some(consumed),
                };
                Ok(((value, completion), consumed))
            },
        )?;
        Ok(result.map(|(value, _end)| value))
    }
}

impl<'source> TerminatedParts<'source> {
    /// Run one original child ending at an actual framing interval.
    ///
    /// A recoverable content error gives up exactly that child and advances to
    /// its actual delimiter. The callback cannot run after EOF or the declared
    /// child count. A delimiter must be a nonempty final interval in `spec`.
    pub fn decode_part<T>(
        &mut self,
        ctx: &DecodeContext<'_>,
        output: &mut PartOutput,
        row: View<'source>,
        spec: PartSpec,
        decode: impl FnOnce(&mut PartOutput) -> Result<T, CodecError>,
    ) -> Result<Option<T>, CodecError> {
        self.validate_row(ctx, row, &spec)?;
        spec.validate(row)?;
        if !spec.framing_bytes.last().is_some_and(|range| {
            range.start < range.end && range.end == cadmpeg_core::decode::u64_from_index(row.end())
        }) {
            return Err(CodecError::malformed(
                "terminated child has no actual final framing interval",
            ));
        }
        self.stopped = true;
        let result = ctx.decode_part(output, row, spec, decode)?;
        self.next = row.end();
        self.remaining -= 1;
        self.stopped = false;
        Ok(result)
    }

    /// Attempt only failure on the original unterminated prefix at genuine EOF.
    ///
    /// The final prefix may be empty only at the registered stream's end. Its
    /// available bytes, not a fabricated delimiter or later child, get the
    /// ordinary helper's sole unknown stub and Error loss. No success value can
    /// escape this callback. Previously completed children are unchanged.
    pub fn fail_at_source_end(
        &mut self,
        ctx: &DecodeContext<'_>,
        output: &mut PartOutput,
        prefix: View<'source>,
        spec: PartSpec,
        decode: impl FnOnce(&mut PartOutput) -> Result<Infallible, CodecError>,
    ) -> Result<(), CodecError> {
        self.validate_row(ctx, prefix, &spec)?;
        ctx.require_source_end(self.available)?;
        let tail = self
            .available
            .child(prefix.start(), self.available.end())
            .ok_or_else(|| {
                CodecError::malformed("unterminated child escapes its available parent")
            })?;
        self.stopped = true;
        match helper::decode_part_with_failure_extent(ctx, output, prefix, spec, tail, decode)? {
            Some(never) => match never {},
            None => {
                self.incomplete = true;
                Ok(())
            }
        }
    }

    fn validate_row(
        &self,
        ctx: &DecodeContext<'_>,
        row: View<'source>,
        spec: &PartSpec,
    ) -> Result<(), CodecError> {
        ctx.require_source(self.available)?;
        ctx.require_source(row)?;
        if self.stopped || self.remaining == 0 || spec.level != self.level {
            return Err(CodecError::malformed(
                "terminated child is not the next declared active child",
            ));
        }
        let original = self.available.child(row.start(), row.end());
        if row.location().space != self.available.location().space
            || row.start() < self.next
            || !original.is_some_and(|view| view.window().as_ptr() == row.window().as_ptr())
        {
            return Err(CodecError::malformed(
                "terminated child is not an original ordered parent window",
            ));
        }
        if row.start() == row.end() && row.end() != self.available.end() {
            return Err(CodecError::malformed(
                "empty terminated child prefix is not at available EOF",
            ));
        }
        Ok(())
    }
}

fn validate_rule(parent: PartLevel, level: PartLevel) -> Result<(), CodecError> {
    if parent.framing() != PartFraming::Terminated || level.framing() != PartFraming::Terminated {
        return Err(CodecError::malformed(
            "terminated recovery requires terminated Appendix C levels",
        ));
    }
    let declared = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../scripts/part-framing.tsv"
    ))
    .lines()
    .skip(1)
    .any(|line| {
        let mut fields = line.split('\t');
        let codec = fields.next();
        let name = fields.next();
        let parents = fields.next();
        level
            .as_str()
            .split_once('.')
            .is_some_and(|(expected_codec, expected_name)| {
                codec == Some(expected_codec)
                    && name == Some(expected_name)
                    && parents.is_some_and(|parents| {
                        parents
                            .split('|')
                            .any(|candidate| candidate == parent.as_str())
                    })
            })
    });
    if !declared {
        return Err(CodecError::malformed(
            "Appendix C does not declare this terminated parent",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests;
