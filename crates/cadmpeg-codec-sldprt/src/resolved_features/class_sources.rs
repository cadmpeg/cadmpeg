// SPDX-License-Identifier: Apache-2.0
//! Decode-local class locators from genuine declaration Views.

use crate::records::FeatureInputClass;
use cadmpeg_core::CodecError;
use std::collections::BTreeMap;
use std::rc::Rc;

/// A shared immutable handle to pending per-owner layout pointers. This is
/// source state, never a native class field or an ordinary document validator.
#[derive(Clone, Default)]
pub(crate) struct ClassSources {
    pointers: Option<Rc<BTreeMap<String, BTreeMap<String, u64>>>>,
}

impl ClassSources {
    pub(crate) fn from_source(
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        source: cadmpeg_core::decode::View<'_>,
        classes: &[FeatureInputClass],
    ) -> Result<Self, CodecError> {
        let mut result = Self::default();
        result.admit_source(ctx, source, classes)?;
        Ok(result)
    }

    pub(crate) fn admit_source(
        &mut self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        source: cadmpeg_core::decode::View<'_>,
        classes: &[FeatureInputClass],
    ) -> Result<(), CodecError> {
        ctx.capture_part_extent(source)?;
        let Some(first) = classes.first() else {
            if super::names::payload_classes(source.window())
                .next()
                .is_some()
            {
                return Err(CodecError::malformed(
                    "source class declarations are absent from staged content",
                ));
            }
            return Ok(());
        };
        let parsed = super::names::class_declarations(source.window(), &first.parent)?;
        if parsed.len() != classes.len()
            || parsed.iter().zip(classes).any(|(actual, staged)| {
                actual.id != staged.id
                    || actual.parent != staged.parent
                    || actual.ordinal != staged.ordinal
                    || actual.name != staged.name
            })
        {
            return Err(CodecError::malformed(
                "class declaration content differs from source",
            ));
        }
        for (class, (position, name)) in parsed
            .iter()
            .zip(super::names::payload_classes(source.window()))
        {
            if class.name != name {
                return Err(CodecError::malformed(
                    "source class declaration order changed",
                ));
            }
            self.insert(
                ctx,
                &class.id,
                cadmpeg_core::decode::u64_from_index(position),
            )?;
        }
        Ok(())
    }

    /// Return the declaration marker position relative to its owning lane View.
    pub(crate) fn position(&self, class: &FeatureInputClass) -> Option<u64> {
        self.pointer(&class.id)
    }

    pub(super) fn pointer(&self, id: &str) -> Option<u64> {
        self.pointers.as_deref()?.get(id)?.get("offset").copied()
    }

    pub(super) fn insert(
        &mut self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        id: &str,
        position: u64,
    ) -> Result<(), CodecError> {
        if self.pointer(id).is_some() {
            return Err(CodecError::malformed("duplicate class source pointer"));
        }
        let entry_bytes = entry_storage(id)?;
        let header_bytes = if self.pointers.is_none() {
            std::mem::size_of::<BTreeMap<String, BTreeMap<String, u64>>>()
                .checked_add(2 * std::mem::size_of::<usize>())
                .ok_or_else(|| CodecError::malformed("class pointer bank size overflow"))?
        } else {
            0
        };
        let bytes = entry_bytes
            .checked_add(header_bytes)
            .ok_or_else(|| CodecError::malformed("class pointer storage overflow"))?;
        ctx.retain_bytes(
            cadmpeg_core::decode::u64_from_index(bytes),
            "class source pointer bank",
        )?;
        if self.pointers.is_none() {
            self.pointers = Some(Rc::new(BTreeMap::new()));
        }
        let pointers = self
            .pointers
            .as_mut()
            .and_then(Rc::get_mut)
            .ok_or_else(|| CodecError::malformed("class source bank is already shared"))?;
        pointers.insert(
            owned_key(ctx, id)?,
            BTreeMap::from([(owned_key(ctx, "offset")?, position)]),
        );
        Ok(())
    }

    pub(crate) fn into_pointers(
        self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    ) -> Result<BTreeMap<String, BTreeMap<String, u64>>, CodecError> {
        let Some(pointers) = self.pointers else {
            return Ok(BTreeMap::new());
        };
        match Rc::try_unwrap(pointers) {
            Ok(pointers) => Ok(pointers),
            Err(shared) => {
                let bytes = shared.keys().try_fold(
                    std::mem::size_of::<BTreeMap<String, BTreeMap<String, u64>>>(),
                    |bytes, id| {
                        bytes.checked_add(entry_storage(id)?).ok_or_else(|| {
                            CodecError::malformed("class pointer copy size overflow")
                        })
                    },
                )?;
                ctx.retain_bytes(
                    cadmpeg_core::decode::u64_from_index(bytes),
                    "class source pointer copy",
                )?;
                let mut pointers = BTreeMap::new();
                for (id, fields) in shared.iter() {
                    let position = fields
                        .get("offset")
                        .copied()
                        .ok_or_else(|| CodecError::malformed("class pointer lacks its offset"))?;
                    pointers.insert(
                        owned_key(ctx, id)?,
                        BTreeMap::from([(owned_key(ctx, "offset")?, position)]),
                    );
                }
                Ok(pointers)
            }
        }
    }
}

fn entry_storage(id: &str) -> Result<usize, CodecError> {
    std::mem::size_of::<(String, BTreeMap<String, u64>)>()
        .checked_add(std::mem::size_of::<(String, u64)>())
        .and_then(|bytes| bytes.checked_add(id.len()))
        .and_then(|bytes| bytes.checked_add("offset".len()))
        .ok_or_else(|| CodecError::malformed("class pointer entry size overflow"))
}

fn owned_key(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    text: &str,
) -> Result<String, CodecError> {
    let mut bytes = ctx.alloc_filled(text.len(), 0u8, "class source pointer key")?;
    bytes.copy_from_slice(text.as_bytes());
    String::from_utf8(bytes).map_err(|_| CodecError::malformed("class pointer key is not UTF-8"))
}

#[cfg(test)]
mod tests;
