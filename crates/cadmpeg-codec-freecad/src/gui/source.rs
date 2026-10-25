// SPDX-License-Identifier: Apache-2.0
//! One source-owned GUI parse shared by early framing and late transfer.

use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::CodecError;
#[cfg(test)]
use cadmpeg_ir::CadIr;

use super::{Graph, GuiSources};
#[cfg(test)]
use crate::xml_framing::XmlSource;
use cadmpeg_ir::source_fidelity::parts::PartOutput;

mod parts;
pub(crate) mod presentation;

pub(crate) struct Source<'a> {
    source: View<'a>,
}

/// Parse the original member once before source inventory admission.
pub(crate) fn parse<'a>(
    ctx: &DecodeContext<'_>,
    source: View<'a>,
) -> Result<Source<'a>, CodecError> {
    ctx.capture_part_extent(source)?;
    Ok(Source { source })
}

impl<'a> Source<'a> {
    #[cfg(test)]
    pub(crate) fn view(&self) -> View<'a> {
        self.source
    }

    #[cfg(test)]
    pub(crate) fn scanner(&self, ctx: &DecodeContext<'_>) -> Result<XmlSource<'a>, CodecError> {
        XmlSource::new(ctx, self.source)
    }

    /// Construct typed GUI fields without projecting neutral presentation.
    #[cfg(test)]
    pub(crate) fn decode(
        self,
        ctx: &DecodeContext<'a>,
        sources: &GuiSources<'_, 'a>,
        output: &mut PartOutput,
    ) -> Result<Graph, CodecError> {
        self.decode_with_whitespace(ctx, sources, output)
            .map(|(graph, _, _)| graph)
    }

    /// Return navigation facts from the genuine bounded GUI XML callback.
    pub(crate) fn decode_with_whitespace(
        self,
        ctx: &DecodeContext<'a>,
        sources: &GuiSources<'_, 'a>,
        output: &mut PartOutput,
    ) -> Result<
        (
            Graph,
            Vec<View<'a>>,
            super::payloads::sources::EntryInterpretations<'a>,
        ),
        CodecError,
    > {
        parts::decode(ctx, self.source, sources, output)
    }

    #[cfg(test)]
    pub(crate) fn transfer(
        self,
        ctx: &DecodeContext<'a>,
        ir: &mut CadIr,
        sources: &GuiSources<'_, 'a>,
        output: &mut PartOutput,
    ) -> Result<Graph, CodecError> {
        let (mut graph, _, memo) = parts::decode(ctx, self.source, sources, output)?;
        let sources = GuiSources {
            gui_entry_interpretations: Some(&memo),
            ..*sources
        };
        super::project_records(ctx, ir, &mut graph, &sources)?;
        Ok(graph)
    }
}

#[cfg(test)]
mod tests;
