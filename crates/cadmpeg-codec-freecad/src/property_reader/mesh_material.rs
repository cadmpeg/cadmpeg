// SPDX-License-Identifier: Apache-2.0
//! Independent mesh-material projections use their admitted original fields.

use super::PropertyReader;
use crate::gui::payloads::sources::mesh_material::Header;
use crate::native::PropertyRecord;
use cadmpeg_core::decode::RecordOwner;
use cadmpeg_core::CodecError;
use cadmpeg_ir::scalar::FiniteReal;

impl<'ctx, 'arena> PropertyReader<'ctx, 'arena> {
    pub(crate) fn source_mesh_binding(
        &self,
        owner: &RecordOwner<'_>,
        property: &PropertyRecord,
    ) -> Result<Option<u32>, CodecError> {
        let Some(sources) = self.payload_sources else {
            return Ok(None);
        };
        if self
            .mesh_material_slots(property)
            .is_none_or(|slots| slots.binding.is_none())
        {
            return Ok(None);
        }
        sources
            .mesh_binding(self.context, owner, &property.id)?
            .map(Some)
            .ok_or_else(|| {
                CodecError::malformed("held mesh material binding has no original supplier")
            })
    }

    pub(crate) fn source_mesh_header<'a>(
        &'a self,
        owner: &RecordOwner<'_>,
        property: &'a PropertyRecord,
        role: usize,
    ) -> Result<Option<Header<'a>>, CodecError> {
        let Some(sources) = self.payload_sources else {
            return Ok(None);
        };
        let Some(slots) = self.mesh_material_slots(property) else {
            return Ok(None);
        };
        let count = match role {
            0..4 => slots.colors[role].map(<[_]>::len),
            4 => slots.shininess.map(<[_]>::len),
            5 => slots.transparency.map(<[_]>::len),
            _ => return Err(CodecError::malformed("invalid mesh material array role")),
        };
        let Some(count) = count else {
            return Ok(None);
        };
        let header = sources
            .mesh_header(self.context, owner, &property.id, role)?
            .ok_or_else(|| {
                CodecError::malformed("held mesh material array has no original header")
            })?;
        if header.count() != count {
            return Err(CodecError::malformed(
                "mesh material array differs from its original count",
            ));
        }
        Ok(Some(header))
    }

    pub(crate) fn source_mesh_color(
        &self,
        owner: &RecordOwner<'_>,
        property: &PropertyRecord,
        role: usize,
        ordinal: usize,
        header: &Header<'_>,
    ) -> Result<Option<u32>, CodecError> {
        let Some(sources) = self.payload_sources else {
            return Ok(None);
        };
        let held = self.mesh_material_slots(property).and_then(|slots| {
            slots
                .colors
                .get(role)
                .copied()
                .flatten()
                .and_then(|values| values.get(ordinal))
                .copied()
                .flatten()
        });
        if held.is_none() {
            return Err(CodecError::malformed(
                "requested mesh material color slot is not held",
            ));
        }
        sources
            .mesh_color(self.context, owner, &property.id, role, ordinal, header)?
            .map(Some)
            .ok_or_else(|| {
                CodecError::malformed("held mesh material color has no original supplier")
            })
    }

    pub(crate) fn source_mesh_float(
        &self,
        owner: &RecordOwner<'_>,
        property: &PropertyRecord,
        role: usize,
        ordinal: usize,
        header: &Header<'_>,
    ) -> Result<Option<FiniteReal>, CodecError> {
        let Some(sources) = self.payload_sources else {
            return Ok(None);
        };
        let values = self
            .mesh_material_slots(property)
            .and_then(|slots| match role {
                4 => slots.shininess,
                5 => slots.transparency,
                _ => None,
            });
        if values
            .and_then(|values| values.get(ordinal))
            .is_none_or(Option::is_none)
        {
            return Err(CodecError::malformed(
                "requested mesh material scalar slot is not held",
            ));
        }
        sources
            .mesh_float(self.context, owner, &property.id, role, ordinal, header)?
            .map(Some)
            .ok_or_else(|| {
                CodecError::malformed("held mesh material scalar has no original supplier")
            })
    }

    pub(crate) fn source_mesh_binding_failure(
        &self,
        property: &PropertyRecord,
    ) -> Result<bool, CodecError> {
        let Some(sources): Option<&'ctx crate::gui::payloads::sources::PayloadSources<'arena>> =
            self.payload_sources
        else {
            return Ok(false);
        };
        let Some(part) = sources.mesh_binding_failure(self.context, &property.id)? else {
            return Ok(false);
        };
        self.unavailable_part(part)
    }
}
