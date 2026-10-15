// SPDX-License-Identifier: Apache-2.0
//! Shared cage validation with a route-specific refusal type.

use std::fmt::Arguments;

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;

use super::SubdError;

pub(super) trait SubdAdmission {
    type Error;

    fn message(&self, message: Arguments<'_>) -> Result<SubdError, Self::Error>;
}

pub(super) struct StandardAdmission;

impl SubdAdmission for StandardAdmission {
    type Error = SubdError;

    fn message(&self, message: Arguments<'_>) -> Result<SubdError, SubdError> {
        Ok(SubdError::Admission(message.to_string()))
    }
}

impl SubdAdmission for DecodeContext<'_> {
    type Error = CodecError;

    fn message(&self, message: Arguments<'_>) -> Result<SubdError, CodecError> {
        Ok(SubdError::Admission(std::fmt::format(message)))
    }
}
