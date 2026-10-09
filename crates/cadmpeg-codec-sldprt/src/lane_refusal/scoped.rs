// SPDX-License-Identifier: Apache-2.0
//! Temporary carrier refusal messages and their consuming source shell.

use cadmpeg_core::decode::{DecodeContext, ScopedReservation};
use cadmpeg_core::CodecError;
use cadmpeg_ir::geometry::nurbs::NurbsError;

#[cfg(test)]
use super::LaneRefusals;

pub(crate) struct ScopedRecord<'ctx> {
    pub(crate) message: String,
    _message_storage: ScopedReservation<'ctx>,
}

pub(crate) struct ScopedLaneRefusals<'ctx> {
    records: Vec<ScopedRecord<'ctx>>,
    shell: ScopedReservation<'ctx>,
}

impl<'ctx> ScopedLaneRefusals<'ctx> {
    pub(crate) fn new(ctx: &'ctx DecodeContext<'_>) -> Result<Self, CodecError> {
        Ok(Self { records: Vec::new(), shell: ctx.reserve_scoped(0, "hold SLDPRT lane refusal records")? })
    }

    fn note(
        &mut self, ctx: &'ctx DecodeContext<'_>, record: impl std::fmt::Display, error: &NurbsError,
    ) -> Result<(), CodecError> {
        if let NurbsError::ResourceLimit(limit) = error {
            return Err((*limit).into());
        }
        let mut storage = ctx.reserve_scoped(0, "hold SLDPRT lane refusal message")?;
        let message = storage.with_storage(|| super::format_refusal_message(ctx, record, error))?;
        let value = ScopedRecord { message, _message_storage: storage };
        self.shell.with_storage(|| ctx.push_vec(&mut self.records, value, "collect SLDPRT lane refusals"))
    }

    pub(crate) fn into_records(self) -> ScopedRecords<'ctx> {
        ScopedRecords { records: self.records.into_iter(), _shell: self.shell }
    }
}

pub(crate) struct ScopedRecords<'ctx> {
    records: std::vec::IntoIter<ScopedRecord<'ctx>>,
    _shell: ScopedReservation<'ctx>,
}

impl<'ctx> ScopedRecords<'ctx> {
    pub(crate) fn next(
        &mut self, ctx: &DecodeContext<'_>, operation: &'static str,
    ) -> Result<Option<ScopedRecord<'ctx>>, CodecError> {
        if let Some(limit) = ctx.resource_refusal() {
            return Err(CodecError::ResourceLimit(limit));
        }
        if self.records.len() == 0 {
            return Ok(None);
        }
        ctx.next_charged(&mut self.records, operation)
    }
}

/// Both routes use the same refusal grammar; the concrete destination owns
/// temporary message storage or ordinary retained diagnostic output.
pub(crate) enum RefusalSink<'out, 'ctx> {
    #[cfg(test)]
    Retained(&'out mut LaneRefusals),
    Scoped(&'out mut ScopedLaneRefusals<'ctx>),
}

impl<'ctx> RefusalSink<'_, 'ctx> {
    pub(crate) fn note(
        &mut self, ctx: &'ctx DecodeContext<'_>, record: impl std::fmt::Display, error: &NurbsError,
    ) -> Result<(), CodecError> {
        match self {
            #[cfg(test)]
            Self::Retained(out) => out.note(ctx, record, error),
            Self::Scoped(out) => out.note(ctx, record, error),
        }
    }
}
