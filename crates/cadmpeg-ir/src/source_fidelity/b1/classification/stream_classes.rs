// SPDX-License-Identifier: Apache-2.0
//! Format-bound defaults for source spans without an owner class.

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;

use crate::{CadIr, CodecFormat};

/// The design-only entries of a codec's `const STREAM_CLASSES` table.
/// Every other stream defaults to both classes. A listed stream requires the
/// §3.1 proof over every golden; this type does not assert that proof exists.
#[derive(Debug, Clone, Copy)]
pub struct StreamClasses {
    format: CodecFormat,
    design_only: &'static [&'static str],
}

impl StreamClasses {
    /// Declare exact stream names for one source format.
    pub const fn new(format: CodecFormat, design_only: &'static [&'static str]) -> Self {
        Self {
            format,
            design_only,
        }
    }

    /// Format whose source streams this table classifies.
    pub const fn format(self) -> CodecFormat {
        self.format
    }

    /// Exact stream names whose default is Design.
    pub const fn design_only(self) -> &'static [&'static str] {
        self.design_only
    }

    pub(super) fn validate(self, ctx: &DecodeContext<'_>, ir: &CadIr) -> Result<(), CodecError> {
        if ir.source.as_ref().map(|source| source.format()) != Some(self.format.as_str()) {
            return Err(CodecError::malformed(
                "stream class table differs from the source format",
            ));
        }
        let mut names = Vec::new();
        ctx.reserve_working(
            &mut names,
            self.design_only.len(),
            "validate source stream classes",
        )?;
        names.extend_from_slice(self.design_only);
        names.sort_unstable();
        if names.iter().any(|name| name.is_empty())
            || names.windows(2).any(|pair| pair[0] == pair[1])
        {
            return Err(CodecError::malformed(
                "stream class table has an empty or duplicate stream",
            ));
        }
        Ok(())
    }

    pub(super) fn mask(self, stream: &str) -> u8 {
        if self.design_only.contains(&stream) {
            super::DESIGN
        } else {
            super::MODEL | super::DESIGN
        }
    }
}
