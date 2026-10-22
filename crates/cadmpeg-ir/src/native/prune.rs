// SPDX-License-Identifier: Apache-2.0
//! Typed-owner admission around partial reference-list edits.

use super::NativeRecord;
use crate::schema::rewrite::typed::{
    RewriteIdentities,
    prune::{self, DependencyReference, ReferencePruneEffect},
};
use cadmpeg_core::CodecError;
use cadmpeg_core::decode::DecodeContext;
use serde::{Serialize, de::DeserializeOwned};
use std::collections::BTreeSet;

pub(crate) struct Prepared<T> {
    pub effect: ReferencePruneEffect,
    pub replacement: Option<(NativeRecord, T)>,
}

pub(crate) fn visit<T: DeserializeOwned + RewriteIdentities>(
    ctx: &DecodeContext<'_>,
    record: &NativeRecord,
    visitor: &mut dyn FnMut(DependencyReference<'_>) -> Result<(), CodecError>,
) -> Result<(), CodecError> {
    record
        .to_typed::<T>(ctx)
        .map_err(CodecError::from)?
        .visit_dependency_references(ctx, visitor)
}

pub(crate) fn model<T: DeserializeOwned + Serialize + RewriteIdentities>(
    ctx: &DecodeContext<'_>,
    value: &T,
    deleted: &BTreeSet<String>,
) -> Result<Prepared<T>, CodecError> {
    let effect = prune::analyze(value, ctx, deleted)?;
    if effect != ReferencePruneEffect::ListsChanged {
        return Ok(Prepared {
            effect,
            replacement: None,
        });
    }
    let source = NativeRecord::from_typed_for_decode(ctx, value).map_err(CodecError::from)?;
    record::<T>(ctx, &source, deleted)
}

pub(crate) fn record<T: DeserializeOwned + Serialize + RewriteIdentities>(
    ctx: &DecodeContext<'_>,
    source: &NativeRecord,
    deleted: &BTreeSet<String>,
) -> Result<Prepared<T>, CodecError> {
    let mut value = source.to_typed::<T>(ctx).map_err(CodecError::from)?;
    let effect = prune::analyze(&value, ctx, deleted)?;
    if effect != ReferencePruneEffect::ListsChanged {
        return Ok(Prepared {
            effect,
            replacement: None,
        });
    }
    if !source
        .matches_typed_for_decode(ctx, &value)
        .map_err(CodecError::from)?
    {
        return Ok(Prepared {
            effect: ReferencePruneEffect::UnknownReferencePosition {
                owner: std::any::type_name::<T>(),
            },
            replacement: None,
        });
    }
    let effect = value.prune_reference_lists(ctx, deleted)?;
    if !effect.can_apply() {
        return Ok(Prepared {
            effect,
            replacement: None,
        });
    }
    // Every affected typed reference must be gone before this replacement is
    // eligible for commit. A child owner without a list editor must not turn
    // an unchanged reference into a successful preparation.
    if prune::analyze(&value, ctx, deleted)? != ReferencePruneEffect::Unchanged {
        return Ok(Prepared {
            effect: ReferencePruneEffect::UnknownReferencePosition {
                owner: std::any::type_name::<T>(),
            },
            replacement: None,
        });
    }
    let changed = NativeRecord::from_typed_for_decode(ctx, &value).map_err(CodecError::from)?;
    if changed.id() != source.id() {
        return Err(CodecError::malformed(
            "typed list pruning changed the record identity",
        ));
    }
    let admitted = match changed.to_typed::<T>(ctx) {
        Ok(admitted) => admitted,
        Err(error) => {
            if let Some(limit) = error.resource_limit() {
                return Err(limit.into());
            }
            return Ok(Prepared {
                effect: ReferencePruneEffect::UnrepresentablePrunedList {
                    owner: std::any::type_name::<T>(),
                },
                replacement: None,
            });
        }
    };
    Ok(Prepared {
        effect,
        replacement: Some((changed, admitted)),
    })
}
