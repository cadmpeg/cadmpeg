// SPDX-License-Identifier: Apache-2.0
//! Accepted original XML fields for independent neutral presentation producers.

use crate::gui::GuiSources;
use crate::native::{GuiDocumentRecord, GuiPropertyRecord, GuiViewProviderRecord, ValueSlots};
use crate::xml_framing::XmlSource;
use cadmpeg_core::decode::{DecodeContext, SourceExtentProof, View};
use cadmpeg_core::CodecError;
use std::rc::Rc;

pub(crate) struct Control<'source> {
    xml: Rc<XmlSource<'source>>,
    index: usize,
    proof: SourceExtentProof,
}

impl<'source> Control<'source> {
    pub(crate) fn new(
        ctx: &DecodeContext<'_>,
        xml: Rc<XmlSource<'source>>,
        index: usize,
    ) -> Result<Self, CodecError> {
        let proof = ctx.source_extent_proof(xml.extent(index)?)?;
        Ok(Self { xml, index, proof })
    }

    pub(crate) fn primary(&self, ctx: &DecodeContext<'_>) -> Result<View<'source>, CodecError> {
        ctx.require_source_authority(&self.proof)?;
        let view = self.xml.extent(self.index)?;
        ctx.require_source_ancestry(&self.proof, view)?;
        Ok(view)
    }

    fn required(
        &self,
        ctx: &DecodeContext<'_>,
        index: usize,
        key: &str,
    ) -> Result<String, CodecError> {
        self.primary(ctx)?;
        self.xml.attribute(ctx, index, key)?.ok_or_else(|| {
            CodecError::malformed("admitted presentation field has no original source supplier")
        })
    }

    pub(crate) fn read_provider(
        &self,
        ctx: &DecodeContext<'_>,
        current: &GuiViewProviderRecord,
    ) -> Result<GuiViewProviderRecord, CodecError> {
        self.primary(ctx)?;
        let mut provider = current.clone();
        provider.name = self.required(ctx, self.index, "name")?;
        provider.expanded = if current.expanded.is_some() {
            self.xml
                .attribute(ctx, self.index, "expanded")?
                .as_deref()
                .and_then(crate::native::parse_bool)
        } else {
            None
        };
        Ok(provider)
    }

    pub(crate) fn read_scalar_property(
        &self,
        ctx: &DecodeContext<'_>,
        current: &GuiPropertyRecord,
    ) -> Result<GuiPropertyRecord, CodecError> {
        self.primary(ctx)?;
        let mut property = current.clone();
        property.name = self.required(ctx, self.index, "name")?;
        property.type_name = self.required(ctx, self.index, "type")?;
        let selected = current.values.iter().find_map(|value| {
            value
                .attributes
                .contains_key("value")
                .then_some((value.order, "value"))
                .or_else(|| {
                    value
                        .attributes
                        .contains_key("Value")
                        .then_some((value.order, "Value"))
                })
        });
        if let Some((order, key)) = selected {
            let index = self.value_index(order)?;
            let value = self.required(ctx, index, key)?;
            let held = current
                .values
                .iter()
                .cloned()
                .map(|mut field| {
                    if field.order == order {
                        field.attributes.insert(key.to_owned(), value.clone());
                    }
                    field
                })
                .collect();
            property.values = ValueSlots::new(held, current.values.slot_count())
                .map_err(CodecError::malformed)?;
        }
        Ok(property)
    }

    fn value_index(&self, order: usize) -> Result<usize, CodecError> {
        let index = self
            .index
            .checked_add(1)
            .and_then(|index| index.checked_add(order))
            .ok_or_else(|| CodecError::malformed("presentation value source ordinal overflows"))?;
        let value = self.xml.extent(index)?;
        let owner = self.xml.extent(self.index)?;
        if value.start() < owner.start() || value.end() > owner.end() {
            return Err(CodecError::malformed(
                "presentation value escapes its admitted source owner",
            ));
        }
        Ok(index)
    }

    pub(crate) fn read_document(
        &self,
        ctx: &DecodeContext<'_>,
        current: &GuiDocumentRecord,
        sources: &GuiSources<'_, 'source>,
    ) -> Result<GuiDocumentRecord, CodecError> {
        self.primary(ctx)?;
        let mut document = current.clone();
        document.schema_version = if current.schema_version.is_some() {
            Some(self.required(ctx, self.index, "SchemaVersion")?)
        } else {
            None
        };
        for state in &mut document.states {
            let control = sources
                .gui_entry_interpretations
                .and_then(|memo| memo.presentation_control(&state.id))
                .ok_or_else(|| {
                    CodecError::malformed("GUI presentation state has no accepted original source")
                })?;
            control.primary(ctx)?;
            state.kind = control.xml.read_name(control.index)?.to_owned();
            for (key, value) in &mut state.attributes {
                *value = control.required(ctx, control.index, key)?;
            }
            let mut assets = Vec::new();
            // Neutral states use display asset references, not the other generic
            // descendant values. Read only the fields that actually select assets.
            for value in state.values.iter() {
                for (key, name) in value
                    .attributes
                    .iter()
                    .filter(|(key, _)| matches!(key.as_str(), "file" | "File"))
                    .filter(|(_, name)| state.side_entries.contains(name))
                {
                    let index = control.value_index(value.order)?;
                    let actual = control.required(ctx, index, key)?;
                    assets.push(entry_name(ctx, &actual, name, sources)?);
                }
            }
            for (_, name) in state
                .attributes
                .iter()
                .filter(|(key, _)| matches!(key.as_str(), "file" | "File"))
                .filter(|(_, name)| state.side_entries.contains(name))
            {
                assets.push(entry_name(ctx, name, name, sources)?);
            }
            if assets != state.side_entries {
                return Err(CodecError::malformed(
                    "presentation asset suppliers disagree with admitted references",
                ));
            }
            state.side_entries = assets;
        }
        Ok(document)
    }
}

fn entry_name(
    ctx: &DecodeContext<'_>,
    actual: &str,
    admitted: &str,
    sources: &GuiSources<'_, '_>,
) -> Result<String, CodecError> {
    if actual != admitted {
        return Err(CodecError::malformed(
            "presentation asset differs from its original XML field",
        ));
    }
    let owner = sources
        .entry_owners
        .and_then(|owners| owners.get(actual))
        .ok_or_else(|| {
            CodecError::malformed("presentation asset has no original ZIP name supplier")
        })?;
    let name = owner.read_name(ctx)?;
    if name != actual {
        return Err(CodecError::malformed(
            "presentation asset differs from its original ZIP name",
        ));
    }
    Ok(name)
}
