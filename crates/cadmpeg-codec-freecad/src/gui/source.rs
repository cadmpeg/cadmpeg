// SPDX-License-Identifier: Apache-2.0
//! One source-owned GUI parse consumed by the late transfer.

use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::CodecError;
use cadmpeg_ir::CadIr;

use super::{Graph, GuiSources};

pub(crate) struct Source<'a> {
    source: View<'a>,
    parsed: Parsed<'a>,
}

enum Parsed<'a> {
    Admitted(roxmltree::Document<'a>),
    Unreadable(Graph),
}

/// Parse the original member once.
pub(crate) fn parse<'a>(
    ctx: &DecodeContext<'_>,
    source: View<'a>,
) -> Result<Source<'a>, CodecError> {
    let parsed = match super::parse_xml(ctx, source.window()) {
        Ok(xml) => Parsed::Admitted(xml),
        Err(error @ (CodecError::Malformed(_) | CodecError::Truncated { .. })) => {
            Parsed::Unreadable(super::source_only_graph(None, &error))
        }
        Err(error) => return Err(error),
    };
    Ok(Source { source, parsed })
}

impl<'a> Source<'a> {
    /// Consume the same DOM after the application objects and topology exist.
    pub(crate) fn transfer(
        self,
        ctx: &DecodeContext<'a>,
        ir: &mut CadIr,
        sources: &GuiSources<'_, '_>,
    ) -> Result<Graph, CodecError> {
        match self.parsed {
            Parsed::Admitted(xml) => {
                let source = crate::persistence::source::Source::new(self.source, &xml)?;
                match super::transfer_document(
                    ctx,
                    ir,
                    &xml,
                    sources,
                    Some(&source),
                    super::GuiOrigins::Admitted,
                ) {
                    Err(error @ (CodecError::Malformed(_) | CodecError::Truncated { .. })) => {
                        Ok(super::source_only_graph(None, &error))
                    }
                    result => result,
                }
            }
            Parsed::Unreadable(graph) => Ok(graph),
        }
    }
}

#[cfg(test)]
mod tests;
