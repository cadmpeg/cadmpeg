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
        crate::xml_recovery::XmlSource {
            member: "GuiDocument.xml",
            scan_operation: "recover FreeCAD GUI elements",
            parse_operation: "FreeCAD XML tree",
        },
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
        Ok((view, xml, _)) => Source {
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
    /// Classify failed GUI side members from their declared references.
    pub(crate) fn mark_failed_members(
        &self,
        failed: &std::collections::BTreeMap<String, Vec<cadmpeg_ir::ids::UnknownId>>,
        classes: &mut std::collections::BTreeMap<String, cadmpeg_ir::ArenaClasses>,
    ) {
        let Parsed::Admitted(xml) = &self.parsed else {
            return;
        };
        for node in xml.descendants().filter(roxmltree::Node::is_element) {
            if node
                .ancestors()
                .find(|ancestor| ancestor.has_tag_name("Property"))
                .and_then(|property| property.attribute("type"))
                .is_some_and(crate::persistence::is_xlink_type)
            {
                continue;
            }
            if let Some(name) = node
                .attribute("file")
                .or_else(|| node.attribute("File"))
                .filter(|name| failed.contains_key(*name))
            {
                classes
                    .entry(name.to_owned())
                    .or_default()
                    .insert(cadmpeg_ir::ArenaClass::Model);
            }
        }
    }

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
