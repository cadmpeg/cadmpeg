// SPDX-License-Identifier: Apache-2.0
//! Class-terminal joins to bounded raw reference declarations.

use super::ClassDefinition;
use crate::container::Container;
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use serde::{Deserialize, Serialize};

/// Raw declaration selected by the class's one-based reference token.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct RegistryReferenceDeclaration {
    name: String,
    code: u8,
    source_offset: u64,
}

impl RegistryReferenceDeclaration {
    pub(super) fn validate(&self, class: &ClassDefinition) -> Result<(), String> {
        let end = self
            .source_offset
            .checked_add(cadmpeg_core::decode::u64_from_index(self.name.len()))
            .and_then(|end| end.checked_add(2));
        if self.name.len() > 254
            || !self.name.starts_with("UGS::")
            || !self.name.bytes().all(|byte| (0x20..0x7f).contains(&byte))
            || self.source_offset < class.section_offset
            || end.is_none_or(|end| end > class.source_offset)
        {
            return Err(
                "ClassDefinition registry_reference_declaration is not a preceding raw declaration"
                    .into(),
            );
        }
        Ok(())
    }
}

pub(super) fn join_references(
    ctx: &DecodeContext<'_>,
    container: &Container,
    classes: &mut [ClassDefinition],
) -> Result<(), CodecError> {
    // registry_definitions emits entries in physical offset order.
    let mut start = 0;
    while start < classes.len() {
        let first = &classes[start];
        let end = start
            + classes[start..]
                .iter()
                .take_while(|class| {
                    class.source_entry == first.source_entry
                        && class.section_offset == first.section_offset
                })
                .count();
        let Some(span) = first.source_offset.checked_sub(first.section_offset) else {
            start = end;
            continue;
        };
        let Some(bytes) = container.bounded_entry_bytes(ctx, first.section_offset, span)? else {
            start = end;
            continue;
        };
        if let Some(references) = crate::om::registry::reference_declarations(ctx, bytes)? {
            for class in &mut classes[start..end] {
                let Some(layout) = class.registry_layout() else {
                    continue;
                };
                let Some(reference) = usize::try_from(layout.reference.get() - 1)
                    .ok()
                    .and_then(|index| references.get(index))
                else {
                    continue;
                };
                let source_offset = class
                    .section_offset
                    .checked_add(cadmpeg_core::decode::u64_from_index(reference.offset))
                    .ok_or_else(|| {
                        ctx.refuse_codec_limit("nx registry reference source offset", 0, 1)
                    })?;
                class.registry_reference_declaration = Some(RegistryReferenceDeclaration {
                    name: ctx.copy_retained_text(reference.name, "nx registry reference name")?,
                    code: reference.code,
                    source_offset,
                });
            }
        }
        start = end;
    }
    Ok(())
}
