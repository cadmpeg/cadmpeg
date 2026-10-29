// SPDX-License-Identifier: Apache-2.0
//! Borrowed references from the neutral arena registry's typed owners.

use super::CadIr;
use crate::schema::EntitySchema;
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;

impl CadIr {
    /// Visit each neutral arena record's references with its owning identity.
    /// Identity-shaped plain text is not a reference. Native arenas are separate.
    pub fn visit_neutral_references(
        &self,
        ctx: &DecodeContext<'_>,
        visitor: &mut dyn FnMut(&str, &str) -> Result<(), CodecError>,
    ) -> Result<(), CodecError> {
        macro_rules! visit {
            ($($field:ident: $ty:ty, $doc:literal, [$($attribute:meta),*] $(, [$($schema_attr:meta),*])?;)*) => {$(
                for record in &self.model.$field {
                    record.visit_references(ctx, &mut |target| visitor(record.identity(), target))?;
                }
            )*};
        }
        super::arena_registry!(visit);
        for (child, parent) in &self.model.feature_regeneration_parents.0 {
            visitor(child.as_str(), parent.as_str())?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
