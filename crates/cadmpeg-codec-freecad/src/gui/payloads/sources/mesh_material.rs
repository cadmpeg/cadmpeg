// SPDX-License-Identifier: Apache-2.0
//! Original mesh-material fields supply independent attribute producers.
use super::{PayloadSources, RowSource};
use cadmpeg_core::decode::{u64_from_index, DecodeContext, RecordOwner, View};
use cadmpeg_core::CodecError;
use cadmpeg_ir::scalar::FiniteReal;
use std::collections::BTreeMap;

pub(super) struct Source<'source> {
    binding: Option<RowSource<'source>>,
    failed: Option<String>,
    arrays: [Option<Array<'source>>; 6],
}
impl Default for Source<'_> {
    fn default() -> Self {
        Self {
            binding: None,
            failed: None,
            arrays: std::array::from_fn(|_| None),
        }
    }
}
pub(in crate::gui::payloads) struct Array<'source> {
    header: RowSource<'source>,
    rows: BTreeMap<usize, RowSource<'source>>,
}
pub(super) struct Alias<'source> {
    binding: Option<View<'source>>,
    failed: Option<String>,
    arrays: [Option<(View<'source>, Vec<(usize, View<'source>)>)>; 6],
}
pub(crate) struct Header<'a> {
    property: &'a str,
    role: usize,
    count: usize,
}
impl Header<'_> {
    pub(crate) fn count(&self) -> usize {
        self.count
    }
}
fn storage(
    ctx: &DecodeContext<'_>,
    bytes: usize,
    operation: &'static str,
) -> Result<(), CodecError> {
    let allowance = ctx.per_expand_allowance();
    if u64_from_index(bytes) > allowance {
        return Err(ctx.refuse_part_limit(
            cadmpeg_core::decode::ResourceDimension::RetainedBytes,
            allowance,
            0,
            u64_from_index(bytes),
            operation,
        ));
    }
    Ok(())
}
fn field<'source>(
    ctx: &DecodeContext<'_>,
    view: View<'source>,
) -> Result<RowSource<'source>, CodecError> {
    Ok(RowSource {
        extent: view,
        proof: ctx.source_extent_proof(view)?,
    })
}
fn checked(ctx: &DecodeContext<'_>, source: &RowSource<'_>) -> Result<(), CodecError> {
    ctx.require_source_authority(&source.proof)?;
    ctx.require_source_ancestry(&source.proof, source.extent)
}
impl<'source> Array<'source> {
    pub(in crate::gui::payloads) fn new(
        ctx: &DecodeContext<'_>,
        header: View<'source>,
    ) -> Result<Self, CodecError> {
        Ok(Self {
            header: field(ctx, header)?,
            rows: BTreeMap::new(),
        })
    }
    pub(in crate::gui::payloads) fn row(
        &mut self,
        ctx: &DecodeContext<'_>,
        ordinal: usize,
        view: View<'source>,
    ) -> Result<(), CodecError> {
        let bytes = self
            .rows
            .len()
            .checked_add(1)
            .and_then(|count| count.checked_mul(size_of::<(usize, RowSource<'_>)>()))
            .ok_or_else(|| CodecError::malformed("mesh material row supplier storage overflows"))?;
        storage(ctx, bytes, "mesh material row supplier")?;
        if self.rows.insert(ordinal, field(ctx, view)?).is_some() {
            return Err(CodecError::malformed(
                "mesh material supplier repeats a row",
            ));
        }
        Ok(())
    }
}
impl<'source> PayloadSources<'source> {
    fn mesh_source_mut(
        &mut self,
        ctx: &DecodeContext<'_>,
        property: &str,
    ) -> Result<&mut Source<'source>, CodecError> {
        if !self.mesh_materials.contains_key(property) {
            let bytes = property
                .len()
                .checked_add(size_of::<(String, Source<'source>)>())
                .ok_or_else(|| CodecError::malformed("mesh material supplier storage overflows"))?;
            storage(ctx, bytes, "mesh material property supplier")?;
            self.mesh_materials
                .insert(property.to_owned(), Source::default());
        }
        self.mesh_materials
            .get_mut(property)
            .ok_or_else(|| CodecError::malformed("mesh material supplier is absent"))
    }
    pub(in crate::gui::payloads) fn mesh_binding_source(
        &mut self,
        ctx: &DecodeContext<'_>,
        property: &str,
        view: View<'source>,
        failed: Option<&str>,
    ) -> Result<(), CodecError> {
        let source = field(ctx, view)?;
        if let Some(failed) = failed {
            storage(ctx, failed.len(), "mesh material binding failure home")?;
        }
        let target = self.mesh_source_mut(ctx, property)?;
        if target.binding.is_some() {
            return Err(CodecError::malformed(
                "mesh material binding supplier repeats",
            ));
        }
        target.binding = Some(source);
        target.failed = failed.map(str::to_owned);
        Ok(())
    }
    pub(in crate::gui::payloads) fn mesh_array_source(
        &mut self,
        ctx: &DecodeContext<'_>,
        property: &str,
        role: usize,
        array: Array<'source>,
    ) -> Result<(), CodecError> {
        let target = self
            .mesh_source_mut(ctx, property)?
            .arrays
            .get_mut(role)
            .ok_or_else(|| {
                CodecError::malformed("mesh material role escapes its registered fields")
            })?;
        if target.replace(array).is_some() {
            return Err(CodecError::malformed(
                "mesh material array supplier repeats",
            ));
        }
        Ok(())
    }
    pub(crate) fn mesh_binding_failure(
        &self,
        ctx: &DecodeContext<'_>,
        property: &str,
    ) -> Result<Option<&str>, CodecError> {
        let Some(source) = self.mesh_materials.get(property) else {
            return Ok(None);
        };
        if let Some(binding) = &source.binding {
            checked(ctx, binding)?;
        }
        Ok(source.failed.as_deref())
    }
    pub(crate) fn mesh_binding(
        &self,
        ctx: &DecodeContext<'_>,
        owner: &RecordOwner<'_>,
        property: &str,
    ) -> Result<Option<u32>, CodecError> {
        let Some(source) = self.mesh_materials.get(property) else {
            return Ok(None);
        };
        if source.failed.is_some() {
            return Ok(None);
        }
        let Some(binding) = &source.binding else {
            return Ok(None);
        };
        checked(ctx, binding)?;
        ctx.record_fragment(owner, binding.extent, || {
            let mut value = binding.extent;
            let binding = value.req_u32_le()?;
            if binding > 2 {
                return Err(CodecError::malformed(
                    "invalid mesh material binding supplier",
                ));
            }
            Ok(Some(binding))
        })
    }
    pub(crate) fn mesh_header<'a>(
        &'a self,
        ctx: &DecodeContext<'_>,
        owner: &RecordOwner<'_>,
        property: &'a str,
        role: usize,
    ) -> Result<Option<Header<'a>>, CodecError> {
        let Some(source) = self
            .mesh_materials
            .get(property)
            .and_then(|source| source.arrays.get(role))
            .and_then(Option::as_ref)
        else {
            return Ok(None);
        };
        checked(ctx, &source.header)?;
        ctx.record_fragment(owner, source.header.extent, || {
            let mut value = source.header.extent;
            let count = usize::try_from(value.req_u32_le()?).map_err(CodecError::malformed)?;
            Ok(Some(Header {
                property,
                role,
                count,
            }))
        })
    }
    fn mesh_row<'a>(
        &'a self,
        ctx: &DecodeContext<'_>,
        property: &str,
        role: usize,
        ordinal: usize,
        header: &Header<'_>,
    ) -> Result<Option<&'a RowSource<'source>>, CodecError> {
        if property != header.property || role != header.role || ordinal >= header.count {
            return Err(CodecError::malformed(
                "mesh material row differs from its actual header",
            ));
        }
        let source = self
            .mesh_materials
            .get(property)
            .and_then(|source| source.arrays.get(role))
            .and_then(Option::as_ref)
            .and_then(|array| array.rows.get(&ordinal));
        if let Some(source) = source {
            checked(ctx, source)?;
        }
        Ok(source)
    }
    pub(crate) fn mesh_color(
        &self,
        ctx: &DecodeContext<'_>,
        owner: &RecordOwner<'_>,
        property: &str,
        role: usize,
        ordinal: usize,
        header: &Header<'_>,
    ) -> Result<Option<u32>, CodecError> {
        if role >= 4 {
            return Err(CodecError::malformed(
                "mesh material color has a scalar role",
            ));
        }
        let Some(source) = self.mesh_row(ctx, property, role, ordinal, header)? else {
            return Ok(None);
        };
        ctx.record_fragment(owner, source.extent, || {
            let mut value = source.extent;
            Ok(Some(value.req_u32_le()?))
        })
    }
    pub(crate) fn mesh_float(
        &self,
        ctx: &DecodeContext<'_>,
        owner: &RecordOwner<'_>,
        property: &str,
        role: usize,
        ordinal: usize,
        header: &Header<'_>,
    ) -> Result<Option<FiniteReal>, CodecError> {
        if !(4..6).contains(&role) {
            return Err(CodecError::malformed(
                "mesh material scalar has a color role",
            ));
        }
        let Some(source) = self.mesh_row(ctx, property, role, ordinal, header)? else {
            return Ok(None);
        };
        ctx.record_fragment(owner, source.extent, || {
            let mut value = source.extent;
            Ok(Some(super::super::finite32(&mut value)?.into()))
        })
    }
    pub(super) fn export_mesh_alias(
        &self,
        ctx: &DecodeContext<'_>,
        property: &str,
    ) -> Result<Option<Alias<'source>>, CodecError> {
        let Some(source) = self.mesh_materials.get(property) else {
            return Ok(None);
        };
        if let Some(failed) = &source.failed {
            storage(ctx, failed.len(), "mesh material alias failure home")?;
        }
        let mut arrays = std::array::from_fn(|_| None);
        for (role, array) in source.arrays.iter().enumerate() {
            if let Some(array) = array {
                let bytes = array
                    .rows
                    .len()
                    .checked_mul(size_of::<(usize, View<'source>)>())
                    .ok_or_else(|| {
                        CodecError::malformed("mesh material alias storage overflows")
                    })?;
                storage(ctx, bytes, "mesh material alias row views")?;
                let mut rows = Vec::new();
                rows.try_reserve_exact(array.rows.len()).map_err(|_| {
                    cadmpeg_core::decode::ResourceLimit::allocation_failed(
                        cadmpeg_core::decode::ResourceDimension::RetainedBytes,
                        ctx.policy().limits.max_retained_bytes,
                        u64_from_index(bytes),
                        "mesh material alias row views",
                    )
                })?;
                for (ordinal, row) in &array.rows {
                    rows.push((*ordinal, row.extent));
                }
                arrays[role] = Some((array.header.extent, rows));
            }
        }
        Ok(Some(Alias {
            binding: source.binding.as_ref().map(|binding| binding.extent),
            failed: source.failed.clone(),
            arrays,
        }))
    }
    pub(super) fn import_mesh_alias(
        &mut self,
        ctx: &DecodeContext<'_>,
        property: &str,
        alias: &Alias<'source>,
    ) -> Result<(), CodecError> {
        if let Some(binding) = alias.binding {
            self.mesh_binding_source(ctx, property, binding, alias.failed.as_deref())?;
        }
        for (role, array) in alias.arrays.iter().enumerate() {
            if let Some((header, rows)) = array {
                let mut source = Array::new(ctx, *header)?;
                for (ordinal, row) in rows {
                    source.row(ctx, *ordinal, *row)?;
                }
                self.mesh_array_source(ctx, property, role, source)?;
            }
        }
        Ok(())
    }
}
