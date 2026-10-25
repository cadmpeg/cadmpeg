// SPDX-License-Identifier: Apache-2.0
//! Requested source exclusions retain genuine failure and layout facts.

use super::parts::{PartLevel, PartState};
use super::SourceFidelity;
use crate::native::NativeNamespace;
use crate::NativeUnknownRecord;
use cadmpeg_core::decode::{ArenaClass, DecodeContext, DecodeSessionToken, PartCapture, View};
use cadmpeg_core::CodecError;

/// One source record excluded by the caller's requested content classes.
///
/// The producer supplies the actual record identity, its owning Appendix C part
/// level, and its original source extent after resolving independent consumers.
#[derive(Debug)]
pub struct SourceRecordExclusion<'record> {
    session: DecodeSessionToken,
    identity: &'record str,
    parent_level: PartLevel,
    source: PartCapture,
    source_owner: Option<crate::native::NativeRecord>,
}

impl<'record> SourceRecordExclusion<'record> {
    /// Authenticate an original source extent for a class the request excludes.
    ///
    /// # Errors
    /// Rejects a requested class, a foreign source, or the caller's prior refusal.
    pub fn new(
        ctx: &DecodeContext<'_>,
        identity: &'record str,
        class: ArenaClass,
        parent_level: PartLevel,
        source: View<'_>,
    ) -> Result<Self, CodecError> {
        ctx.require_source(source)?;
        if ctx.depth().required_scope().holds(class) {
            return Err(CodecError::malformed(
                "source exclusion cannot remove a requested class",
            ));
        }
        Ok(Self {
            session: ctx.session_token()?,
            identity,
            parent_level,
            source: ctx.working_part_extent(source)?,
            source_owner: None,
        })
    }

    /// Authenticate a typed source owner before its final native installation.
    ///
    /// The producer lends its actual parsed record. Canonicalization uses working
    /// storage and never registers or retains a second native output record.
    ///
    /// # Errors
    /// Rejects an invalid typed record, a mismatched identity, a requested class,
    /// a foreign source, or the caller's prior refusal.
    pub fn from_typed_source<T: serde::Serialize>(
        ctx: &DecodeContext<'_>,
        identity: &'record str,
        record: &T,
        class: ArenaClass,
        parent_level: PartLevel,
        source: View<'_>,
    ) -> Result<Self, CodecError> {
        let mut proof = Self::new(ctx, identity, class, parent_level, source)?;
        let owner = crate::native::NativeRecord::working_source_owner(ctx, record)?;
        if owner.id() != identity {
            return Err(CodecError::malformed(
                "typed source owner identity disagrees with its exclusion",
            ));
        }
        proof.source_owner = Some(owner);
        Ok(proof)
    }
}

impl SourceFidelity {
    /// Close optional failure links to proven request-excluded source records.
    ///
    /// The original part, unknown identity, retained bytes, diagnostics, source
    /// layout and independently held links remain unchanged. The caller removes
    /// the excluded native records after this handoff succeeds.
    ///
    /// # Errors
    /// Rejects foreign or duplicate proofs, absent native and typed source owners, unmatched
    /// failure ancestry, missing failure facts, and the caller's resource refusal.
    pub fn exclude_requested_native_sources(
        &self,
        ctx: &DecodeContext<'_>,
        namespace: &mut NativeNamespace,
        exclusions: &[SourceRecordExclusion<'_>],
    ) -> Result<(), CodecError> {
        if let Some(refusal) = ctx.resource_refusal() {
            return Err(refusal.into());
        }
        if exclusions.is_empty() {
            return Ok(());
        }
        let session = ctx.session_token()?;
        let mut ordered = Vec::new();
        ctx.reserve_working(
            &mut ordered,
            exclusions.len(),
            "source exclusion identity index",
        )?;
        for exclusion in exclusions {
            if exclusion.session != session {
                return Err(CodecError::malformed(
                    "source exclusion belongs to another decode session",
                ));
            }
            ordered.push(exclusion);
        }
        ordered.sort_unstable_by_key(|exclusion| exclusion.identity);
        if ordered
            .windows(2)
            .any(|pair| pair[0].identity == pair[1].identity)
        {
            return Err(CodecError::malformed("duplicate source exclusion identity"));
        }
        let mut native_ids = Vec::new();
        for records in namespace.arenas().values() {
            ctx.reserve_working(
                &mut native_ids,
                records.len(),
                "source exclusion actual native owners",
            )?;
            native_ids.extend(records.iter().map(|record| record.id()));
        }
        native_ids.sort_unstable();
        for exclusion in &ordered {
            if native_ids.binary_search(&exclusion.identity).is_err()
                && exclusion.source_owner.is_none()
            {
                return Err(CodecError::malformed(
                    "source exclusion has neither a held native owner nor authenticated typed source owner",
                ));
            }
        }
        drop(native_ids);
        let Some(records) = namespace.arenas_mut().get_mut("unknowns") else {
            return Ok(());
        };
        let mut replacements = Vec::new();
        ctx.reserve_working(
            &mut replacements,
            records.len(),
            "source exclusion staged unknown replacements",
        )?;
        for (position, record) in records.iter().enumerate() {
            let replacement = record.excluding_unknown_links(ctx, |unknown, link| {
                let Ok(at) =
                    ordered.binary_search_by_key(&link.as_str(), |exclusion| exclusion.identity)
                else {
                    return Ok(false);
                };
                self.authenticate_excluded_failure(unknown, ordered[at])?;
                Ok(true)
            })?;
            if let Some(replacement) = replacement {
                replacements.push((position, replacement));
            }
        }
        for (position, replacement) in replacements {
            records[position] = replacement;
        }
        Ok(())
    }

    fn authenticate_excluded_failure(
        &self,
        unknown: &NativeUnknownRecord,
        exclusion: &SourceRecordExclusion<'_>,
    ) -> Result<(), CodecError> {
        let part = self
            .parts
            .get(&unknown.id)
            .filter(|part| part.state() == &PartState::GivenUp {})
            .ok_or_else(|| {
                CodecError::malformed("excluded source link has no actual given-up part")
            })?;
        let retained = self.retained_records.get(&unknown.id).ok_or_else(|| {
            CodecError::malformed("excluded source failure has no retained bytes")
        })?;
        let layout = self.record_layout.get(unknown.id.as_str()).ok_or_else(|| {
            CodecError::malformed("excluded source failure has no original layout")
        })?;
        if retained.stream() != part.stream()
            || retained.offset() != part.offset()
            || retained.byte_len() != part.byte_len()
            || layout.stream() != part.stream()
            || layout.offset() != part.offset()
            || layout.byte_len() != part.byte_len()
        {
            return Err(CodecError::malformed(
                "excluded source failure facts disagree",
            ));
        }
        let source = &exclusion.source;
        let end = source
            .offset()
            .checked_add(source.byte_len())
            .ok_or_else(|| CodecError::malformed("source exclusion extent overflows"))?;
        let part_end = part
            .offset()
            .checked_add(part.byte_len())
            .ok_or_else(|| CodecError::malformed("source failure extent overflows"))?;
        if part.stream() != source.stream() || part.offset() < source.offset() || part_end > end {
            return Err(CodecError::malformed(
                "source exclusion does not contain the given-up part",
            ));
        }
        let mut parent = part.parent();
        for _ in 0..self.parts.len() {
            let Some(id) = parent else { break };
            let node = self
                .parts
                .get(id)
                .ok_or_else(|| CodecError::malformed("source failure parent is absent"))?;
            if node.level() == exclusion.parent_level
                && node.stream() == source.stream()
                && node.offset() == source.offset()
                && node.byte_len() == source.byte_len()
            {
                return Ok(());
            }
            parent = node.parent();
        }
        Err(CodecError::malformed(
            "source exclusion has no matching actual parent owner",
        ))
    }
}

#[cfg(test)]
mod tests;
