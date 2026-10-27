// SPDX-License-Identifier: Apache-2.0
use super::{push_constant_finding, Ctx};
use cadmpeg_core::CodecError;
use cadmpeg_ir::report::check::{Check, Finding};

pub(super) fn validate_xref_placement_sources(
    ctx: &Ctx<'_, '_>,
    findings: &mut Vec<Finding>,
) -> Result<(), CodecError> {
    for reference in &ctx.native.xref_references {
        let Some(source) = &reference.placement_source else {
            continue;
        };
        if !crate::xref::placement_source_matches(source)? {
            push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion XREF placement source does not match its repeated-target frame",
                Some(reference.id.clone()),
            );
        }
    }
    Ok(())
}
