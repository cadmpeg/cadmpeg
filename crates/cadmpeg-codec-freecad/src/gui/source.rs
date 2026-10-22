// SPDX-License-Identifier: Apache-2.0
//! One source-owned GUI parse shared by early framing and late transfer.

use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::CodecError;
use cadmpeg_ir::CadIr;

use super::{Graph, GuiSources};
use crate::xml_framing::XmlFraming;

pub(crate) struct Source<'a> {
    source: View<'a>,
    parsed: Parsed<'a>,
}

enum Parsed<'a> {
    Admitted {
        xml: roxmltree::Document<'a>,
        framing: XmlFraming<'a>,
    },
    Unreadable(Graph),
}

/// Parse the original member once before source inventory admission.
pub(crate) fn parse<'a>(
    ctx: &DecodeContext<'_>,
    source: View<'a>,
) -> Result<Source<'a>, CodecError> {
    ctx.capture_part_extent(source)?;
    let parsed = match super::parse_xml(ctx, source.window()) {
        Ok(xml) => {
            let framing = crate::xml_framing::frame(ctx, source, &xml, true)?;
            Parsed::Admitted { xml, framing }
        }
        Err(error @ (CodecError::Malformed(_) | CodecError::Truncated { .. })) => {
            Parsed::Unreadable(super::source_only_graph(None, &error))
        }
        Err(error) => return Err(error),
    };
    Ok(Source { source, parsed })
}

impl<'a> Source<'a> {
    pub(crate) fn view(&self) -> View<'a> {
        self.source
    }

    pub(crate) fn document(&self) -> Option<&roxmltree::Document<'a>> {
        match &self.parsed {
            Parsed::Admitted { xml, .. } => Some(xml),
            Parsed::Unreadable(_) => None,
        }
    }

    pub(crate) fn framing(&self) -> Option<&XmlFraming<'a>> {
        match &self.parsed {
            Parsed::Admitted { framing, .. } => Some(framing),
            Parsed::Unreadable(_) => None,
        }
    }

    /// Consume the same DOM after the application objects and topology exist.
    pub(crate) fn transfer(
        self,
        ctx: &DecodeContext<'a>,
        ir: &mut CadIr,
        sources: &GuiSources<'_, '_>,
    ) -> Result<Graph, CodecError> {
        match self.parsed {
            Parsed::Admitted { xml, .. } => {
                let source = crate::persistence::source::Source::new(ctx, self.source, &xml)?;
                match super::transfer_document(ir, &xml, sources, Some(&source)) {
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
