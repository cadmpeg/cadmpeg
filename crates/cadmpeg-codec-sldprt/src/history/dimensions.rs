// SPDX-License-Identifier: Apache-2.0
//! Replace dimension source candidates, then publish only surviving children.

use std::collections::BTreeMap;

use cadmpeg_core::decode::{DecodeContext, ScopedReservation};
use cadmpeg_core::text::NonBlankString;
use cadmpeg_core::CodecError;
use cadmpeg_ir::report::loss::LossNote;

use super::attributes::{borrowed_attributes, BorrowedProperties, Properties};

pub(super) struct DimensionCandidates<'xml, 'ctx, 'arena> {
    context: &'ctx DecodeContext<'arena>,
    properties: BTreeMap<&'xml str, BorrowedProperties<'xml, 'ctx>>,
    parameters: BTreeMap<&'xml str, &'xml str>,
    property_shell: ScopedReservation<'ctx>,
    parameter_shell: ScopedReservation<'ctx>,
}

struct DimensionPropertyEntries<'xml, 'ctx> {
    entries: std::collections::btree_map::IntoIter<&'xml str, BorrowedProperties<'xml, 'ctx>>,
    _shell: ScopedReservation<'ctx>,
}

struct ParameterEntries<'xml, 'ctx> {
    entries: std::collections::btree_map::IntoIter<&'xml str, &'xml str>,
    _shell: ScopedReservation<'ctx>,
}

impl<'xml, 'ctx, 'arena> DimensionCandidates<'xml, 'ctx, 'arena> {
    pub(super) fn new(ctx: &'ctx DecodeContext<'arena>) -> Result<Self, CodecError> {
        Ok(Self {
            context: ctx, properties: BTreeMap::new(), parameters: BTreeMap::new(),
            property_shell: ctx.reserve_scoped(0, "hold SLDPRT dimension property names")?,
            parameter_shell: ctx.reserve_scoped(0, "hold SLDPRT parameter names")?,
        })
    }

    /// Nonempty properties replace; empty properties preserve the old entry.
    /// Every nonblank parameter name takes the later text, including empty text.
    pub(super) fn record(
        &mut self, losses: &mut Vec<LossNote>,
        node: roxmltree::Node<'xml, '_>, name: &'xml str,
    ) -> Result<(), CodecError> {
        let ctx = self.context;
        let properties = borrowed_attributes(ctx, losses, name, node, &["Name"])?;
        if !properties.is_empty() {
            self.property_shell.with_storage(|| {
                ctx.insert_btree_map(&mut self.properties, name, properties, "index SLDPRT dimension properties")
            })?;
        }
        if ctx.all_by(name.chars(), |character| Ok(character.is_whitespace()), "scan SLDPRT dimension name characters")? {
            return Ok(());
        }
        let value = match node.text() {
            Some(text) => ctx.trim_text(text, "trim SLDPRT history text")?,
            None => "",
        };
        self.parameter_shell.with_storage(|| {
            ctx.insert_btree_map(&mut self.parameters, name, value, "index SLDPRT parameters")
        })?;
        Ok(())
    }

    pub(super) fn into_retained(
        self,
    ) -> Result<(BTreeMap<String, Properties>, BTreeMap<NonBlankString, String>), CodecError> {
        let ctx = self.context;
        let mut source_properties = DimensionPropertyEntries { entries: self.properties.into_iter(), _shell: self.property_shell };
        let mut source_parameters = ParameterEntries { entries: self.parameters.into_iter(), _shell: self.parameter_shell };
        let mut properties = BTreeMap::new();
        while source_properties.entries.len() != 0 {
            let Some((name, value)) = ctx.next_charged(&mut source_properties.entries, "publish SLDPRT dimension property names")? else { break; };
            let value = value.into_retained(ctx)?;
            let name = ctx.copy_retained_text(name, "retain SLDPRT dimension property name")?;
            ctx.insert_btree_map(&mut properties, name, value, "index SLDPRT dimension properties")?;
        }
        drop(source_properties);
        let mut parameters = BTreeMap::new();
        while source_parameters.entries.len() != 0 {
            let Some((name, value)) = ctx.next_charged(&mut source_parameters.entries, "publish SLDPRT parameters")? else { break; };
            let name = ctx.copy_retained_text(name, "retain SLDPRT parameter name")?;
            let value = ctx.copy_retained_text(value, "retain SLDPRT parameter value")?;
            if let Some(name) = NonBlankString::for_decode(ctx, name, "validate nonblank text")? {
                ctx.insert_btree_map(&mut parameters, name, value, "index SLDPRT parameters")?;
            }
        }
        drop(source_parameters);
        Ok((properties, parameters))
    }
}

#[cfg(test)]
mod tests;
