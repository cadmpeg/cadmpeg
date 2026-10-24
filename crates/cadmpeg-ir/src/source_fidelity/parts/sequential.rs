// SPDX-License-Identifier: Apache-2.0
//! Sequential recovery keeps earlier records and stops at the first failed row.

use std::convert::Infallible;

use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::CodecError;

use super::{helper, PartFraming, PartLevel, PartOutput, PartSpec};

/// One original parent's sequential children, in source order.
///
/// A successful callback commits its actual row. A recoverable failure owns the
/// source from that row's start through the parent's end. No later callback can
/// run through this sequence. Cumulative charges and Session refusals retain
/// the ordinary part helper's semantics.
#[derive(Debug)]
pub struct SequentialParts<'source> {
    parent: View<'source>,
    level: PartLevel,
    next: usize,
    stopped: bool,
}

impl<'source> SequentialParts<'source> {
    /// Bind a genuine parent to one of its Appendix C sequential child levels.
    /// Located and terminated children cannot use this recovery policy.
    pub fn new(
        ctx: &DecodeContext<'_>,
        parent: View<'source>,
        parent_level: PartLevel,
        level: PartLevel,
    ) -> Result<Self, CodecError> {
        ctx.require_source(parent)?;
        validate_rule(parent_level, level)?;
        Ok(Self {
            next: parent.start(),
            parent,
            level,
            stopped: false,
        })
    }

    /// Whether failure or a fatal callback error stopped this sequence.
    pub fn stopped(&self) -> bool {
        self.stopped
    }

    /// Parse one actual bounded row once, using the ordinary staged output.
    ///
    /// `Some` contains the accepted callback value. `None` means the failed row
    /// and the rest of the parent have one shared give-up home. Calling again
    /// after `None`, a fatal error, or an unwind is an error before the callback.
    pub fn decode_part<T>(
        &mut self,
        ctx: &DecodeContext<'_>,
        output: &mut PartOutput,
        row: View<'source>,
        spec: PartSpec,
        decode: impl FnOnce(&mut PartOutput) -> Result<T, CodecError>,
    ) -> Result<Option<T>, CodecError> {
        let failure = self.failure_extent(ctx, row, &spec, false)?;
        self.stopped = true;
        let result =
            helper::decode_part_with_failure_extent(ctx, output, row, spec, failure, decode);
        if matches!(&result, Ok(Some(_))) {
            self.next = row.end();
            self.stopped = false;
        }
        result
    }

    /// Parse once when only the grammar can discover the row's successful end.
    ///
    /// The genuine prefix locates the current row; the callback returns its
    /// consumed original view with its actual typed result. Empty success,
    /// foreign windows and captured reads beyond the consumed prefix give up
    /// the parent remainder. An actual EOF attempt may use an empty prefix.
    pub fn decode_discovered<T>(
        &mut self,
        ctx: &DecodeContext<'_>,
        output: &mut PartOutput,
        prefix: View<'source>,
        spec: PartSpec,
        decode: impl FnOnce(&mut PartOutput) -> Result<(T, View<'source>), CodecError>,
    ) -> Result<Option<T>, CodecError> {
        let failure = self.failure_extent(ctx, prefix, &spec, true)?;
        self.stopped = true;
        let result = helper::decode_discovered_part(ctx, output, prefix, spec, failure, decode)?;
        match result {
            Some((value, end)) => {
                self.next = end;
                self.stopped = false;
                Ok(Some(value))
            }
            None => Ok(None),
        }
    }

    /// Run an actual failing parser when only the row's original prefix is
    /// located. No successful record can be returned from this callback.
    ///
    /// The prefix supplies the real row start, not a fabricated row end. The
    /// shared failure home extends to the original parent's end. Framing bytes
    /// in `spec` must belong to that actual prefix. An empty prefix is admitted
    /// only at the parent's actual end, for a parser that encounters EOF.
    pub fn fail_from(
        &mut self,
        ctx: &DecodeContext<'_>,
        output: &mut PartOutput,
        prefix: View<'source>,
        spec: PartSpec,
        decode: impl FnOnce(&mut PartOutput) -> Result<Infallible, CodecError>,
    ) -> Result<(), CodecError> {
        let failure = self.failure_extent(ctx, prefix, &spec, true)?;
        self.stopped = true;
        match helper::decode_part_with_failure_extent(ctx, output, prefix, spec, failure, decode)? {
            Some(never) => match never {},
            None => Ok(()),
        }
    }

    fn failure_extent(
        &self,
        ctx: &DecodeContext<'_>,
        row: View<'source>,
        spec: &PartSpec,
        allow_eof: bool,
    ) -> Result<View<'source>, CodecError> {
        if self.stopped {
            return Err(CodecError::malformed(
                "sequential parent has already stopped",
            ));
        }
        ctx.require_source(self.parent)?;
        ctx.require_source(row)?;
        if spec.level != self.level {
            return Err(CodecError::malformed(
                "sequential row has another Appendix C level",
            ));
        }
        if row.location().space != self.parent.location().space {
            return Err(CodecError::malformed(
                "sequential row and parent have different source spaces",
            ));
        }
        let actual_eof = allow_eof && row.start() == row.end() && row.end() == self.parent.end();
        if row.start() < self.next || (row.start() == row.end() && !actual_eof) {
            return Err(CodecError::malformed(
                "sequential row is empty before EOF or precedes accepted source",
            ));
        }
        let child = self
            .parent
            .child(row.start(), row.end())
            .ok_or_else(|| CodecError::malformed("sequential row escapes its original parent"))?;
        if child.window().as_ptr() != row.window().as_ptr() {
            return Err(CodecError::malformed(
                "sequential row is not the original parent window",
            ));
        }
        self.parent
            .child(row.start(), self.parent.end())
            .ok_or_else(|| {
                CodecError::malformed("sequential failure suffix escapes its original parent")
            })
    }
}

fn validate_rule(parent: PartLevel, level: PartLevel) -> Result<(), CodecError> {
    if level.framing() != PartFraming::Sequential {
        return Err(CodecError::malformed(
            "sequential recovery requires a sequential Appendix C row",
        ));
    }
    let (codec, name) = level
        .as_str()
        .split_once('.')
        .ok_or_else(|| CodecError::malformed("sequential level has no codec qualifier"))?;
    for line in include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../scripts/part-framing.tsv"
    ))
    .lines()
    .skip(1)
    {
        let mut fields = line.split('\t');
        if fields.next() != Some(codec) || fields.next() != Some(name) {
            continue;
        }
        let parents = fields.next().ok_or_else(|| {
            CodecError::malformed("Appendix C sequential row has no declared parent")
        })?;
        let framing = fields.next();
        let end = fields
            .next()
            .ok_or_else(|| CodecError::malformed("Appendix C sequential row has no end rule"))?;
        if framing != Some("sequential") || !(end == "none" || end.starts_with("extent only:")) {
            return Err(CodecError::malformed(
                "Appendix C sequential recovery policy disagrees with its end rule",
            ));
        }
        if !parents
            .split('|')
            .any(|candidate| candidate == parent.as_str())
        {
            return Err(CodecError::malformed(
                "Appendix C does not declare this sequential parent",
            ));
        }
        return Ok(());
    }
    Err(CodecError::malformed(
        "sequential row is absent from Appendix C",
    ))
}

#[cfg(test)]
mod tests;
