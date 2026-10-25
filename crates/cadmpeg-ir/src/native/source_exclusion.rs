// SPDX-License-Identifier: Apache-2.0
//! Subset replacement of authenticated source-unknown references.

use super::{NativeConvertError, NativeRecord};
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;

impl NativeRecord {
    pub(crate) fn working_source_owner<T: serde::Serialize>(
        ctx: &DecodeContext<'_>,
        record: &T,
    ) -> Result<Self, NativeConvertError> {
        Self::canonical(ctx, record)
    }

    pub(crate) fn excluding_unknown_links(
        &self,
        ctx: &DecodeContext<'_>,
        mut excluded: impl FnMut(
            &crate::NativeUnknownRecord,
            &crate::ids::Identity,
        ) -> Result<bool, CodecError>,
    ) -> Result<Option<Self>, CodecError> {
        if let Some(refusal) = ctx.resource_refusal() {
            return Err(refusal.into());
        }
        let mut unknown = self.to_typed::<crate::NativeUnknownRecord>(ctx)?;
        let mut removed = Vec::new();
        ctx.reserve_working(
            &mut removed,
            unknown.links.len(),
            "source exclusion link positions",
        )?;
        for (position, link) in unknown.links.iter().enumerate() {
            if excluded(&unknown, link)? {
                removed.push(position);
            }
        }
        if removed.is_empty() {
            return Ok(None);
        }
        let mut position = 0;
        unknown.links.retain(|_| {
            let keep = removed.binary_search(&position).is_err();
            position += 1;
            keep
        });
        let replacement = Self::canonical(ctx, &unknown)?;
        if replacement.id != self.id || replacement.json.len() > self.json.len() {
            return Err(NativeConvertError::InvalidCollection(
                "source exclusion must preserve unknown identity and shrink its stored text"
                    .to_owned(),
            )
            .into());
        }
        Ok(Some(replacement))
    }
}
