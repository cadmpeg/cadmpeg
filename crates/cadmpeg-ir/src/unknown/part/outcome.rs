// SPDX-License-Identifier: Apache-2.0
//! Outcomes of independently framed source parts.

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;

use super::{Cause, DecodeParts, GivenUpPart, Part};

/// A source address, its framing result, and the reader's admission decision.
#[derive(Debug)]
pub struct PartOutcome<'a> {
    part: Part<'a>,
    state: State,
}

#[derive(Debug)]
enum State {
    Damaged(String),
    Framed(Admission),
}

#[derive(Debug)]
enum Admission {
    Inventory,
    Read,
    Refused { cause: Cause, detail: String },
}

impl<'a> PartOutcome<'a> {
    /// A framed part catalogued at the requested depth.
    #[must_use]
    pub fn inventory(part: Part<'a>) -> Self {
        Self {
            part,
            state: State::Framed(Admission::Inventory),
        }
    }

    /// A framed part read by the decoder.
    #[must_use]
    pub fn admitted(part: Part<'a>) -> Self {
        Self {
            part,
            state: State::Framed(Admission::Read),
        }
    }

    /// A part whose independently declared physical extent does not read.
    #[must_use]
    pub fn damaged(part: Part<'a>, error: &CodecError) -> Self {
        Self {
            part,
            state: State::Damaged(detail(error)),
        }
    }

    /// A framed part whose contents could not be read. The default cause
    /// applies to grammar errors. Limits and unsupported forms are undecoded.
    #[must_use]
    pub fn refused(part: Part<'a>, cause: Cause, error: &CodecError) -> Self {
        Self {
            part,
            state: State::Framed(Admission::Refused {
                cause: cause.of(error),
                detail: detail(error),
            }),
        }
    }

    /// The diagnostic of a part this reader gave up.
    #[must_use]
    pub fn failure(&self) -> Option<(Cause, &str)> {
        match &self.state {
            State::Damaged(detail) => Some((Cause::Unreadable, detail)),
            State::Framed(Admission::Refused { cause, detail }) => Some((*cause, detail)),
            State::Framed(Admission::Inventory | Admission::Read) => None,
        }
    }

    /// The identity used if the part needs a give-up stub.
    ///
    /// # Errors
    ///
    /// A malformed source identity.
    pub fn id(&self) -> Result<crate::ids::UnknownId, CodecError> {
        self.part.id()
    }

    /// Whether the part's physical frame failed.
    #[must_use]
    pub fn framing_failed(&self) -> bool {
        matches!(self.state, State::Damaged(_))
    }

    /// Projects a failure to the same retained evidence and report entry at
    /// every consumer. Inspect emits digest evidence without copying bytes.
    ///
    /// # Errors
    ///
    /// A malformed identity or a refusal of capacity for the report.
    pub fn give_up(
        &self,
        ctx: &DecodeContext<'_>,
        inspect: bool,
    ) -> Result<Option<GivenUpPart>, CodecError> {
        let Some((cause, detail)) = self.failure() else {
            return Ok(None);
        };
        let mut part = self.part.clone();
        if inspect {
            part = part.digest_only();
        }
        let error = match cause {
            Cause::Unreadable => CodecError::Malformed(detail.to_owned()),
            Cause::Undecoded => CodecError::NotImplemented(detail.to_owned()),
        };
        ctx.give_up_part(part, cause, error).map(Some)
    }
}

fn detail(error: &CodecError) -> String {
    match error {
        CodecError::Malformed(detail) | CodecError::NotImplemented(detail) => detail.clone(),
        _ => error.to_string(),
    }
}
