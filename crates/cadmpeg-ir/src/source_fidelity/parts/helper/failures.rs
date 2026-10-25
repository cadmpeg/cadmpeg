// SPDX-License-Identifier: Apache-2.0
//! Actual failure homes retained for dependent records constructed after source closure.

use std::sync::{Arc, Weak};

use cadmpeg_core::decode::{u64_from_index, DecodeContext};
use cadmpeg_core::CodecError;

use super::dependencies::FailureHome;
use super::PartState;
use crate::provenance::CodecFormat;
use crate::report::loss::LossNote;
use crate::report::Severity;
use crate::SourceFidelity;

/// Opaque authority for one actual failed source invocation.
/// Only its finalized registry can consume the handle.
#[derive(Debug, Clone)]
pub struct GivenUpHome {
    registry: Weak<()>,
    index: usize,
}

/// Moved actual helper failures, without mutable model or native records.
#[derive(Debug)]
pub struct FinalizedFailureRegistry {
    codec: CodecFormat,
    identity: Option<Arc<()>>,
    homes: Vec<FailureHome>,
}

impl FinalizedFailureRegistry {
    pub(super) fn new(
        ctx: &DecodeContext<'_>,
        codec: CodecFormat,
        homes: Vec<FailureHome>,
    ) -> Result<Self, CodecError> {
        let identity = if homes.is_empty() {
            None
        } else {
            ctx.retain_bytes(
                u64_from_index(size_of::<[usize; 2]>()),
                "finalized failure authority",
            )?;
            Some(Arc::new(()))
        };
        Ok(Self {
            codec,
            identity,
            homes,
        })
    }

    /// Locate a handle only for an actual helper failure in this output.
    pub fn home(&self, part: &str) -> Option<GivenUpHome> {
        self.homes
            .iter()
            .position(|home| home.id.as_str() == part)
            .and_then(|index| self.handle(index))
    }

    /// Locate the genuine cause of an already witnessed unformed record.
    pub fn home_for_dropped_record(&self, record: &str) -> Option<GivenUpHome> {
        self.homes
            .iter()
            .position(|home| home.dropped.contains(record))
            .and_then(|index| self.handle(index))
    }

    /// Enumerate only identities witnessed by actual failed source producers.
    pub fn dropped_record_homes(&self) -> impl Iterator<Item = (&str, GivenUpHome)> {
        self.homes
            .iter()
            .enumerate()
            .flat_map(move |(index, home)| {
                home.dropped.iter().filter_map(move |record| {
                    self.handle(index).map(|handle| (record.as_str(), handle))
                })
            })
    }

    fn handle(&self, index: usize) -> Option<GivenUpHome> {
        self.identity.as_ref().map(|identity| GivenUpHome {
            registry: Arc::downgrade(identity),
            index,
        })
    }

    fn validate(&self, ctx: &DecodeContext<'_>, handle: &GivenUpHome) -> Result<usize, CodecError> {
        let identity = handle
            .registry
            .upgrade()
            .ok_or_else(|| CodecError::malformed("failure registry has expired"))?;
        if !self
            .identity
            .as_ref()
            .is_some_and(|actual| Arc::ptr_eq(actual, &identity))
        {
            return Err(CodecError::malformed(
                "failure home belongs to another registry",
            ));
        }
        let home = self
            .homes
            .get(handle.index)
            .ok_or_else(|| CodecError::malformed("failure home index is invalid"))?;
        ctx.require_source_authority(&home.source)?;
        Ok(handle.index)
    }

    /// Attach never-formed dependent identities to their actual consumed cause.
    /// Existing identities retain their first real home; no closure is rerun.
    ///
    /// # Errors
    /// Rejects expired, foreign, sealed or inactive source authorities and resource refusal.
    pub fn sponsor_unformed(
        &mut self,
        ctx: &DecodeContext<'_>,
        handle: &GivenUpHome,
        dependents: impl IntoIterator<Item = String>,
    ) -> Result<(), CodecError> {
        let index = self.validate(ctx, handle)?;
        for id in dependents {
            if self.homes.iter().any(|home| home.dropped.contains(&id)) {
                continue;
            }
            let bytes = id.len().checked_add(size_of::<String>()).ok_or_else(|| {
                CodecError::malformed("late dependent identity storage overflows")
            })?;
            ctx.retain_bytes(u64_from_index(bytes), "given-up part dependent identity")?;
            self.homes[index].dropped.insert(id);
            self.homes[index].late_sponsored = true;
        }
        Ok(())
    }

    /// Close only a newly constructed neutral graph over the actual unformed identities.
    ///
    /// # Errors
    /// Propagates invalid source authority, typed reference errors and resource refusal.
    pub fn prune_late_model(
        &mut self,
        ctx: &DecodeContext<'_>,
        model: &mut crate::document::Model,
    ) -> Result<(), CodecError> {
        if !self.homes.iter().any(|home| home.late_sponsored) {
            return Ok(());
        }
        for home in &self.homes {
            ctx.require_source_authority(&home.source)?;
        }
        super::dependencies::prune_late_model(ctx, model, &mut self.homes)
    }

    /// Relocate diagnostics when their actual vector is appended to source diagnostics.
    ///
    /// # Errors
    /// Rejects an overflowing diagnostic index before changing any index.
    pub fn offset_losses(&mut self, additional: usize) -> Result<(), CodecError> {
        for home in &self.homes {
            home.loss_index
                .checked_add(additional)
                .ok_or_else(|| CodecError::malformed("part diagnostic index overflows"))?;
        }
        for home in &mut self.homes {
            home.loss_index = home
                .loss_index
                .checked_add(additional)
                .ok_or_else(|| CodecError::malformed("part diagnostic index overflows"))?;
        }
        Ok(())
    }

    /// Update only the original located Error diagnostics for late unformed records.
    ///
    /// # Errors
    /// Rejects changed failure parts, mismatched typed diagnostics, invalid source authority,
    /// or resource refusal. Neither semantic records nor source captures are edited.
    pub fn apply(
        &mut self,
        ctx: &DecodeContext<'_>,
        fidelity: &SourceFidelity,
        losses: &mut [LossNote],
    ) -> Result<(), CodecError> {
        for home in &self.homes {
            ctx.require_source_authority(&home.source)?;
            let part = fidelity
                .parts()
                .get(&home.id)
                .ok_or_else(|| CodecError::malformed("failure home has no actual source part"))?;
            let range = home.source.range();
            let byte_len = range
                .end
                .checked_sub(range.start)
                .ok_or_else(|| CodecError::malformed("failure home source bounds are reversed"))?;
            if part.state() != &(PartState::GivenUp {})
                || part.stream() != home.source.stream()
                || part.offset() != range.start
                || part.byte_len() != byte_len
            {
                return Err(CodecError::malformed(
                    "failure home source part was changed",
                ));
            }
            let loss = losses
                .get(home.loss_index)
                .ok_or_else(|| CodecError::malformed("failure home has no original diagnostic"))?;
            if loss.code.namespace() != "decode"
                || loss.code.local_code() != "record.unreadable"
                || loss.severity != Severity::Error
                || !loss.provenance.as_ref().is_some_and(|location| {
                    location.format() == self.codec.as_str()
                        && location.stream_name() == Some(part.stream())
                        && location.offset == part.offset()
                        && location.byte_len() == (part.byte_len() != 0).then_some(part.byte_len())
                })
            {
                return Err(CodecError::malformed("failure home diagnostic was changed"));
            }
        }
        for home in self.homes.iter_mut().filter(|home| home.late_sponsored) {
            let message = format!(
                "{} dropped={}",
                home.prefix,
                serde_json::to_string(&home.dropped).map_err(CodecError::malformed)?
            );
            ctx.retain_bytes(u64_from_index(message.len()), "part dependency diagnostic")?;
            losses[home.loss_index].message = message;
            home.late_sponsored = false;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
