// SPDX-License-Identifier: Apache-2.0
//! Independent physical IGES inventory for source-part contract checks.

use cadmpeg_core::decode::{DecodeContext, View};

use crate::framing::iges::{frame_parameter_claims, ParameterClaim};
use crate::framing::{FramedPart, PartTable};

/// Frame physical cards and Parameter owners without requiring valid ownership.
///
/// The Directory and Parameter section frames remain physical siblings. Actual
/// Directory Views link to one shared record frame for identical P extents.
/// Finer semantic fields are left to the strict field framer.
///
/// # Errors
/// Returns ambiguous framing, unsupported representation, source-provenance or
/// resource errors. Already reached physical frames are retained in `out`.
pub fn frame_parameter_owners<'a>(
    ctx: &DecodeContext<'a>,
    source: View<'a>,
    out: &mut Vec<FramedPart<'a>>,
) -> Result<Vec<ParameterClaim<'a>>, String> {
    crate::framing::iges::frame(ctx, source, out)?;
    let claims = frame_parameter_claims(ctx, source, out)?;
    PartTable::load()?.validate(ctx, out)?;
    Ok(claims)
}
