// SPDX-License-Identifier: Apache-2.0
//! Typed same-segment presentation dependencies and paired reference-list edits.

use std::collections::BTreeSet;
use std::fmt::Write as _;

use cadmpeg_core::decode::{u64_from_index, DecodeContext, ResourceDimension, ResourceLimit};
use cadmpeg_core::CodecError;
use cadmpeg_ir::schema::rewrite::typed::{
    prune::{self, DependencyPosition, DependencyReference, ReferencePruneEffect},
    IdentityMap, RewriteIdentities,
};

use super::{PmAppDefaultStyleRecord, PmGraphicsFaceRecord, PmGraphicsStyleCollectionRecord};
use crate::pmdc::PmDcPairedReferenceList;

fn target(
    ctx: &DecodeContext<'_>,
    token: &str,
    kind: &'static str,
    index: u32,
) -> Result<Option<String>, CodecError> {
    let Some(ordinal) = index.checked_sub(1) else {
        return Ok(None);
    };
    let capacity = "inventor:presentation:"
        .len()
        .checked_add(kind.len())
        .and_then(|n| n.checked_add(token.len()))
        .and_then(|n| n.checked_add(12))
        .ok_or_else(|| CodecError::malformed("presentation dependency identity size overflows"))?;
    ctx.retain_bytes(
        u64_from_index(capacity),
        "Inventor presentation dependency identity",
    )?;
    let mut id = String::new();
    id.try_reserve(capacity).map_err(|_| {
        ResourceLimit::allocation_failed(
            ResourceDimension::RetainedBytes,
            ctx.policy().limits.max_retained_bytes,
            u64_from_index(capacity),
            "Inventor presentation dependency identity",
        )
    })?;
    write!(&mut id, "inventor:presentation:{kind}#{token}-{ordinal}")
        .map_err(|_| CodecError::malformed("presentation dependency identity formatting failed"))?;
    Ok(Some(id))
}

fn visit(
    ctx: &DecodeContext<'_>,
    token: &str,
    kind: &'static str,
    index: u32,
    field: &'static str,
    position: DependencyPosition,
    visitor: &mut dyn FnMut(DependencyReference<'_>) -> Result<(), CodecError>,
) -> Result<(), CodecError> {
    if let Some(id) = target(ctx, token, kind, index)? {
        visitor(DependencyReference {
            target: &id,
            field: Some(field),
            position,
        })?;
    }
    Ok(())
}

macro_rules! own_identity {
    () => {
        fn visit_identity_references(
            &self,
            _ctx: &DecodeContext<'_>,
            _visitor: &mut dyn FnMut(&str) -> Result<(), CodecError>,
        ) -> Result<(), CodecError> {
            Ok(())
        }
        fn rewrite_identities<F>(
            mut self,
            _ctx: &DecodeContext<'_>,
            map: &mut IdentityMap<'_, F>,
        ) -> Result<Self, CodecError>
        where
            F: FnMut(&str) -> Result<String, CodecError>,
        {
            self.id = map.identity(&self.id)?;
            Ok(self)
        }
        fn rewrite_native_value<F: FnMut(&str) -> Result<String, CodecError>>(
            _ctx: &DecodeContext<'_>,
            value: &mut serde_json::Value,
            map: &mut IdentityMap<'_, F>,
        ) -> Result<(), CodecError> {
            if let Some(serde_json::Value::String(id)) = value.get_mut("id") {
                *id = map.identity(id)?;
            }
            Ok(())
        }
    };
}

impl RewriteIdentities for PmAppDefaultStyleRecord {
    own_identity!();
    fn visit_dependency_references(
        &self,
        ctx: &DecodeContext<'_>,
        visitor: &mut dyn FnMut(DependencyReference<'_>) -> Result<(), CodecError>,
    ) -> Result<(), CodecError> {
        visit(
            ctx,
            &self.segment_token,
            "rendering-style",
            self.rendering_style_reference,
            "rendering_style_reference",
            DependencyPosition::Required,
            visitor,
        )
    }
    fn prune_reference_lists(
        &mut self,
        ctx: &DecodeContext<'_>,
        deleted: &BTreeSet<String>,
    ) -> Result<ReferencePruneEffect, CodecError> {
        prune::analyze(self, ctx, deleted)
    }
}

impl RewriteIdentities for PmGraphicsFaceRecord {
    own_identity!();
    fn visit_dependency_references(
        &self,
        ctx: &DecodeContext<'_>,
        visitor: &mut dyn FnMut(DependencyReference<'_>) -> Result<(), CodecError>,
    ) -> Result<(), CodecError> {
        visit(
            ctx,
            &self.segment_token,
            "graphics-style-collection",
            self.styles.index(),
            "styles_reference",
            DependencyPosition::Required,
            visitor,
        )
    }
    fn prune_reference_lists(
        &mut self,
        ctx: &DecodeContext<'_>,
        deleted: &BTreeSet<String>,
    ) -> Result<ReferencePruneEffect, CodecError> {
        prune::analyze(self, ctx, deleted)
    }
}

impl RewriteIdentities for PmGraphicsStyleCollectionRecord {
    own_identity!();
    fn visit_dependency_references(
        &self,
        ctx: &DecodeContext<'_>,
        visitor: &mut dyn FnMut(DependencyReference<'_>) -> Result<(), CodecError>,
    ) -> Result<(), CodecError> {
        let references = self.style_references.references();
        for (index, reference) in references.iter().enumerate() {
            visit(
                ctx,
                self.segment_token(),
                "graphics-primary-color",
                reference.index(),
                "style_references",
                DependencyPosition::List {
                    length: references.len(),
                    index,
                },
                visitor,
            )?;
        }
        Ok(())
    }
    fn prune_reference_lists(
        &mut self,
        ctx: &DecodeContext<'_>,
        deleted: &BTreeSet<String>,
    ) -> Result<ReferencePruneEffect, CodecError> {
        if prune::analyze(self, ctx, deleted)? != ReferencePruneEffect::ListsChanged {
            return Ok(ReferencePruneEffect::Unchanged);
        }
        let references = self.style_references.references();
        let bytes = references
            .len()
            .checked_mul(size_of::<crate::pmdc::PmDcReference>())
            .ok_or_else(|| {
                CodecError::malformed("presentation reference list storage overflows")
            })?;
        ctx.retain_bytes(
            u64_from_index(bytes),
            "Inventor pruned presentation reference list",
        )?;
        let mut kept = Vec::new();
        kept.try_reserve(references.len()).map_err(|_| {
            ResourceLimit::allocation_failed(
                ResourceDimension::RetainedBytes,
                ctx.policy().limits.max_retained_bytes,
                u64_from_index(bytes),
                "Inventor pruned presentation reference list",
            )
        })?;
        for reference in references {
            let removed = target(
                ctx,
                self.segment_token(),
                "graphics-primary-color",
                reference.index(),
            )?
            .is_some_and(|id| deleted.contains(&id));
            if !removed {
                kept.push(*reference);
            }
        }
        let Some(list) =
            PmDcPairedReferenceList::from_document(self.style_references.metadata().copied(), kept)
        else {
            return Ok(ReferencePruneEffect::UnrepresentablePrunedList {
                owner: std::any::type_name::<Self>(),
            });
        };
        self.style_references = list;
        Ok(ReferencePruneEffect::ListsChanged)
    }
}

#[cfg(test)]
mod tests;
