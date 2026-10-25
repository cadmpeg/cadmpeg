// SPDX-License-Identifier: Apache-2.0
//! Delivery of an original prefix without asserting a terminated closer.

use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::CodecError;

use super::{helper, PartFraming, PartOutput, PartSpec};

/// A reached terminated container and its original navigation window.
///
/// The callback may deliver readable containing fields and independent child
/// records before a missing later marker. Its returned view is the actual
/// consumed prefix, not evidence that the terminating marker exists. A codec
/// separately checks its next locator. This facade creates no native failure
/// state and uses the ordinary shared staged output and give-up finalizer.
#[derive(Debug)]
pub struct TerminatedContainer<'source> {
    prefix: View<'source>,
    available: View<'source>,
    spec: PartSpec,
}

impl<'source> TerminatedContainer<'source> {
    /// Bind a positive original opening frame to a terminated Appendix C level.
    ///
    /// # Errors
    /// Rejects located/sequential levels, foreign or detached source, an empty
    /// opening frame, or an opening outside the original navigation prefix.
    pub fn new(
        ctx: &DecodeContext<'_>,
        prefix: View<'source>,
        available: View<'source>,
        spec: PartSpec,
    ) -> Result<Self, CodecError> {
        if spec.level.framing() != PartFraming::Terminated {
            return Err(CodecError::malformed(
                "incremental terminated container requires a terminated Appendix C level",
            ));
        }
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
                "terminated container opening is not its original navigation prefix",
            ));
        }
        Ok(Self {
            prefix,
            available,
            spec,
        })
    }

    /// Run the actual parser once and deliver its actual consumed prefix.
    ///
    /// Successful primary captures and all actual reads must fit the returned
    /// original view or its authenticated source projections. Child helpers
    /// retain their ordinary rollback and one-home behavior. A recoverable
    /// container error gives up the navigation window; I/O and Session refusals
    /// escape. Source-end attempts require a separate actual EOF check and a
    /// failure-only helper callback, not a successful empty container result.
    ///
    /// # Errors
    /// Rejects invalid source bounds and propagates fatal resource/I/O errors.
    /// A callback returning foreign, empty, short or post-window source gives
    /// up this container through the same shared failure path.
    pub fn decode<T>(
        self,
        ctx: &DecodeContext<'_>,
        output: &mut PartOutput,
        decode: impl FnOnce(&mut PartOutput) -> Result<(T, View<'source>), CodecError>,
    ) -> Result<Option<(T, View<'source>)>, CodecError> {
        let result = helper::decode_discovered_part(
            ctx,
            output,
            self.prefix,
            self.spec,
            self.available,
            |stage| {
                let (value, consumed) = decode(stage)?;
                Ok(((value, consumed), consumed))
            },
        )?;
        Ok(result.map(|(value, _end)| value))
    }
}

#[cfg(test)]
mod tests;
