// SPDX-License-Identifier: Apache-2.0
//! Literal typed reference positions for partial-record dependency pruning.

use std::collections::BTreeSet;

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;

use super::RewriteIdentities;

/// The wire position declared by a typed owner, without inspecting text values.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DependencyPosition {
    /// A non-array reference, including an optional reference.
    Required,
    /// A direct reference entry in a variable-length array.
    List {
        /// Number of entries in the original list.
        length: usize,
        /// Zero-based position of this reference.
        index: usize,
    },
    /// Removing a direct reference entry cannot inhabit this fixed/checked type.
    UnrepresentablePrunedList {
        /// Owning type whose cardinality prevents deletion.
        owner: &'static str,
        /// Number of entries in the original list.
        length: usize,
        /// Zero-based position of this reference.
        index: usize,
    },
    /// The existing typed visitor knows the reference, but not its field position.
    Unknown {
        /// Type that must declare this reference's field position.
        owner: &'static str,
    },
}

/// One actual typed reference and its declaring object field when available.
#[derive(Debug, Clone, Copy)]
pub struct DependencyReference<'a> {
    /// Referenced document identity.
    pub target: &'a str,
    /// Required, list, or an explicitly deferred position.
    pub position: DependencyPosition,
    /// Name of the object field, not an inferred source-string match.
    pub field: Option<&'static str>,
}

/// Result of analyzing or applying only reference-list entry deletion.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReferencePruneEffect {
    /// No reference to a removed record occurs here.
    Unchanged,
    /// Only variable-length reference list entries change.
    ListsChanged,
    /// A required reference prevents this record from surviving.
    DropRequired,
    /// A list deletion cannot be represented by the current checked type.
    UnrepresentablePrunedList {
        /// Owning type whose cardinality prevents deletion.
        owner: &'static str,
    },
    /// The owner must expose the affected reference's actual position first.
    UnknownReferencePosition {
        /// Type that must declare this reference's field position.
        owner: &'static str,
    },
}
impl ReferencePruneEffect {
    /// Combine fields while keeping an independently proven required drop decisive.
    #[must_use]
    pub fn merge(self, other: Self) -> Self {
        use ReferencePruneEffect::{DropRequired, ListsChanged, Unchanged};
        match (self, other) {
            (DropRequired, _) | (_, DropRequired) => DropRequired,
            (a @ Self::UnrepresentablePrunedList { .. }, _)
            | (_, a @ Self::UnrepresentablePrunedList { .. }) => a,
            (a @ Self::UnknownReferencePosition { .. }, _)
            | (_, a @ Self::UnknownReferencePosition { .. }) => a,
            (ListsChanged, _) | (_, ListsChanged) => ListsChanged,
            (Unchanged, Unchanged) => Unchanged,
        }
    }
    /// Whether list editing has a representable result.
    pub fn can_apply(self) -> bool {
        matches!(self, Self::Unchanged | Self::ListsChanged)
    }
}

/// Analyze only references emitted by the actual typed owner.
pub fn analyze<T: RewriteIdentities>(
    value: &T,
    ctx: &DecodeContext<'_>,
    deleted: &BTreeSet<String>,
) -> Result<ReferencePruneEffect, CodecError> {
    let mut effect = ReferencePruneEffect::Unchanged;
    value.visit_dependency_references(ctx, &mut |reference| {
        if deleted.contains(reference.target) {
            let next = match reference.position {
                DependencyPosition::Required => ReferencePruneEffect::DropRequired,
                DependencyPosition::List { .. } => ReferencePruneEffect::ListsChanged,
                DependencyPosition::UnrepresentablePrunedList { owner, .. } => {
                    ReferencePruneEffect::UnrepresentablePrunedList { owner }
                }
                DependencyPosition::Unknown { owner } => {
                    ReferencePruneEffect::UnknownReferencePosition { owner }
                }
            };
            effect = effect.merge(next);
        }
        Ok(())
    })?;
    Ok(effect)
}

pub(super) fn visit_vec<T: RewriteIdentities>(
    values: &[T],
    ctx: &DecodeContext<'_>,
    visitor: &mut dyn FnMut(DependencyReference<'_>) -> Result<(), CodecError>,
) -> Result<(), CodecError> {
    let _depth = ctx.enter_nested("walk typed dependency list")?;
    for (index, value) in values.iter().enumerate() {
        value.visit_dependency_references(ctx, &mut |mut reference| {
            if T::IS_REFERENCE_LEAF {
                reference.position = DependencyPosition::List {
                    length: values.len(),
                    index,
                };
            }
            visitor(reference)
        })?;
    }
    Ok(())
}

pub(super) fn visit_array<T: RewriteIdentities, const N: usize>(
    values: &[T; N],
    ctx: &DecodeContext<'_>,
    visitor: &mut dyn FnMut(DependencyReference<'_>) -> Result<(), CodecError>,
) -> Result<(), CodecError> {
    let _depth = ctx.enter_nested("walk typed fixed dependency array")?;
    for (index, value) in values.iter().enumerate() {
        value.visit_dependency_references(ctx, &mut |mut reference| {
            if T::IS_REFERENCE_LEAF {
                reference.position = DependencyPosition::UnrepresentablePrunedList {
                    owner: std::any::type_name::<[T; N]>(),
                    length: N,
                    index,
                };
            }
            visitor(reference)
        })?;
    }
    Ok(())
}

pub(super) fn apply_vec<T: RewriteIdentities>(
    values: &mut Vec<T>,
    ctx: &DecodeContext<'_>,
    deleted: &BTreeSet<String>,
) -> Result<ReferencePruneEffect, CodecError> {
    let effect = analyze(values, ctx, deleted)?;
    if !effect.can_apply() || effect == ReferencePruneEffect::Unchanged {
        return Ok(effect);
    }
    if T::IS_REFERENCE_LEAF {
        let mut refusal = None;
        values.retain(|value| {
            if refusal.is_some() {
                return true;
            }
            match analyze(value, ctx, deleted) {
                Ok(ReferencePruneEffect::DropRequired) => false,
                Ok(_) => true,
                Err(error) => {
                    refusal = Some(error);
                    true
                }
            }
        });
        if let Some(error) = refusal {
            return Err(error);
        }
    } else {
        for value in values {
            value.prune_reference_lists(ctx, deleted)?;
        }
    }
    Ok(effect)
}

/// Identity semantics explicitly declared by an existing full-fidelity field owner.
/// Ordinary `String` fields do not implement this behavior through `RewriteIdentities`.
pub(crate) trait DeclaredReferenceField {
    const LEAF: bool;
    fn visit(
        &self,
        ctx: &DecodeContext<'_>,
        visitor: &mut dyn FnMut(DependencyReference<'_>) -> Result<(), CodecError>,
    ) -> Result<(), CodecError>;
    fn apply(
        &mut self,
        ctx: &DecodeContext<'_>,
        deleted: &BTreeSet<String>,
    ) -> Result<ReferencePruneEffect, CodecError>;
}
impl DeclaredReferenceField for String {
    const LEAF: bool = true;
    fn visit(
        &self,
        _ctx: &DecodeContext<'_>,
        visitor: &mut dyn FnMut(DependencyReference<'_>) -> Result<(), CodecError>,
    ) -> Result<(), CodecError> {
        visitor(DependencyReference {
            target: self,
            position: DependencyPosition::Required,
            field: None,
        })
    }
    fn apply(
        &mut self,
        ctx: &DecodeContext<'_>,
        deleted: &BTreeSet<String>,
    ) -> Result<ReferencePruneEffect, CodecError> {
        analyze_declared(self, ctx, deleted)
    }
}
// Only owner-declared native link fields select this implementation. A
// SelectionReference in another field retains its own existing typed owner.
impl DeclaredReferenceField for crate::features::SelectionReference {
    const LEAF: bool = true;
    fn visit(
        &self,
        _ctx: &DecodeContext<'_>,
        visitor: &mut dyn FnMut(DependencyReference<'_>) -> Result<(), CodecError>,
    ) -> Result<(), CodecError> {
        visitor(DependencyReference {
            target: self.as_str(),
            position: DependencyPosition::Required,
            field: None,
        })
    }
    fn apply(
        &mut self,
        ctx: &DecodeContext<'_>,
        deleted: &BTreeSet<String>,
    ) -> Result<ReferencePruneEffect, CodecError> {
        analyze_declared(self, ctx, deleted)
    }
}
impl<T: DeclaredReferenceField> DeclaredReferenceField for Option<T> {
    const LEAF: bool = T::LEAF;
    fn visit(
        &self,
        ctx: &DecodeContext<'_>,
        visitor: &mut dyn FnMut(DependencyReference<'_>) -> Result<(), CodecError>,
    ) -> Result<(), CodecError> {
        if let Some(value) = self {
            value.visit(ctx, visitor)?;
        }
        Ok(())
    }
    fn apply(
        &mut self,
        ctx: &DecodeContext<'_>,
        deleted: &BTreeSet<String>,
    ) -> Result<ReferencePruneEffect, CodecError> {
        self.as_mut()
            .map_or(Ok(ReferencePruneEffect::Unchanged), |value| {
                value.apply(ctx, deleted)
            })
    }
}
impl<T: DeclaredReferenceField> DeclaredReferenceField for Vec<T> {
    const LEAF: bool = false;
    fn visit(
        &self,
        ctx: &DecodeContext<'_>,
        visitor: &mut dyn FnMut(DependencyReference<'_>) -> Result<(), CodecError>,
    ) -> Result<(), CodecError> {
        for (index, value) in self.iter().enumerate() {
            value.visit(ctx, &mut |mut reference| {
                if T::LEAF {
                    reference.position = DependencyPosition::List {
                        length: self.len(),
                        index,
                    };
                }
                visitor(reference)
            })?;
        }
        Ok(())
    }
    fn apply(
        &mut self,
        ctx: &DecodeContext<'_>,
        deleted: &BTreeSet<String>,
    ) -> Result<ReferencePruneEffect, CodecError> {
        let effect = analyze_declared(self, ctx, deleted)?;
        if !effect.can_apply() || effect == ReferencePruneEffect::Unchanged {
            return Ok(effect);
        }
        if T::LEAF {
            let mut refusal = None;
            self.retain(|value| {
                if refusal.is_some() {
                    return true;
                }
                match analyze_declared(value, ctx, deleted) {
                    Ok(ReferencePruneEffect::DropRequired) => false,
                    Ok(_) => true,
                    Err(error) => {
                        refusal = Some(error);
                        true
                    }
                }
            });
            if let Some(error) = refusal {
                return Err(error);
            }
        } else {
            for value in self {
                value.apply(ctx, deleted)?;
            }
        }
        Ok(effect)
    }
}
fn analyze_declared<T: DeclaredReferenceField>(
    value: &T,
    ctx: &DecodeContext<'_>,
    deleted: &BTreeSet<String>,
) -> Result<ReferencePruneEffect, CodecError> {
    let mut effect = ReferencePruneEffect::Unchanged;
    value.visit(ctx, &mut |reference| {
        if deleted.contains(reference.target) {
            effect = effect.merge(match reference.position {
                DependencyPosition::Required => ReferencePruneEffect::DropRequired,
                DependencyPosition::List { .. } => ReferencePruneEffect::ListsChanged,
                DependencyPosition::UnrepresentablePrunedList { owner, .. } => {
                    ReferencePruneEffect::UnrepresentablePrunedList { owner }
                }
                DependencyPosition::Unknown { owner } => {
                    ReferencePruneEffect::UnknownReferencePosition { owner }
                }
            });
        }
        Ok(())
    })?;
    Ok(effect)
}

#[cfg(test)]
mod tests;
