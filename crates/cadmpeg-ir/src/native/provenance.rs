// SPDX-License-Identifier: Apache-2.0
//! Read native source ownership without reconstructing its content fields.

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;

use super::{read_record, NativeRecord};
use crate::unknown::NativeUnknownRecord;

impl NativeRecord {
    pub(crate) fn provenance_unknown(
        &self,
        ctx: &DecodeContext<'_>,
    ) -> Result<NativeUnknownRecord, CodecError> {
        read_record(ctx, "unknowns", self).map_err(CodecError::from)
    }
}
