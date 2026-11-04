// SPDX-License-Identifier: Apache-2.0
//! One source-owned GUI parse consumed by the late transfer.

use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::CodecError;
use cadmpeg_ir::source_fidelity::parts::PartOutput;
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

fn part(label: &'static str) -> cadmpeg_ir::source_fidelity::parts::PartSpec {
    crate::container::parts::part(label).in_stream(cadmpeg_ir::stream_name!("GuiDocument.xml"))
}

/// Parse the original member once, recovering readable providers around
/// damaged elements.
pub(crate) fn parse<'a>(
    ctx: &DecodeContext<'a>,
    parts: &mut PartOutput,
    source: View<'a>,
) -> Result<Source<'a>, CodecError> {
    let shape =
        |detail: String| CodecError::malformed(format_args!("invalid GuiDocument.xml: {detail}"));
    let recovered = match crate::xml_recovery::admit_recovering(
        ctx,
        parts,
        source,
        "GuiDocument.xml",
        "recover FreeCAD GUI elements",
        "FreeCAD XML tree",
        &|name, ancestors| {
            if name == b"ViewProvider" {
                part("freecad.gui_view_provider")
            } else if name == b"Property"
                || ancestors
                    .iter()
                    .any(|ancestor| *ancestor == b"ViewProvider")
            {
                part("freecad.gui_property")
            } else {
                part("freecad.gui_document")
            }
        },
        &shape,
    ) {
        Ok((view, xml)) => Source {
            source: view,
            parsed: Parsed::Admitted(xml),
        },
        Err(error @ (CodecError::Malformed(_) | CodecError::Truncated { .. })) => Source {
            source,
            parsed: Parsed::Unreadable(super::source_only_graph(None, &error)),
        },
        Err(error) => return Err(error),
    };
    Ok(recovered)
}

impl<'a> Source<'a> {
    /// Consume the same DOM after the application objects and topology exist.
    pub(crate) fn transfer(
        self,
        ctx: &DecodeContext<'a>,
        ir: &mut CadIr,
        sources: &GuiSources<'_, '_>,
        stage: Option<&mut PartOutput>,
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
                    stage,
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
