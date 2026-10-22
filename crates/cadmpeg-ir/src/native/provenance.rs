// SPDX-License-Identifier: Apache-2.0
//! Read native source ownership without reconstructing its content fields.

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use serde_json::Value;

use super::{read_record, NativeRecord};
use crate::ids::Identity;
use crate::unknown::NativeUnknownRecord;

impl NativeRecord {
    pub(crate) fn provenance_owner(&self) -> Result<Option<Identity>, CodecError> {
        match self.field("owner") {
            None | Some(Value::Null) => Ok(None),
            Some(Value::String(owner)) => Identity::new(owner).map(Some).map_err(|error| {
                CodecError::malformed(format_args!("native record {} owner: {error}", self.id()))
            }),
            Some(_) => Err(CodecError::malformed(format_args!(
                "native record {} owner must be an identity string",
                self.id()
            ))),
        }
    }

    pub(crate) fn provenance_unknown(
        &self,
        ctx: &DecodeContext<'_>,
    ) -> Result<NativeUnknownRecord, CodecError> {
        read_record(ctx, "unknowns", self).map_err(CodecError::from)
    }
}
