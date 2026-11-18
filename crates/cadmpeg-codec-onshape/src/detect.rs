// SPDX-License-Identifier: Apache-2.0
//! ZIP-prefix and complete-bundle recognition.
use cadmpeg_container::ArchiveSnapshot;
use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::CodecError;
use cadmpeg_ir::codec::Confidence;

pub(super) fn prefix(ctx: &DecodeContext<'_>, root: View<'_>) -> Result<Confidence, CodecError> {
    if !root.window().starts_with(b"PK\x03\x04") {
        return Ok(Confidence::No);
    }
    if !ArchiveSnapshot::has_footer(root.window()) {
        return Ok(Confidence::Low);
    }
    if crate::archive::directory(root).is_none() {
        return Ok(Confidence::Low);
    }
    let mut manifest = false;
    let mut features = false;
    let mut rollback = false;
    let named = ArchiveSnapshot::contains_matching_name(ctx, root, |name| {
        manifest |= name == "manifest.json";
        features |= name.starts_with("partstudios/") && name.ends_with("/features.json");
        rollback |= name.starts_with("partstudios/") && name.ends_with("/bodydetails/index.json");
        manifest && features && rollback
    });
    match named {
        Ok(true) => Ok(Confidence::Medium),
        Err(error @ CodecError::ResourceLimit(_)) => Err(error),
        Ok(false) | Err(_) => Ok(Confidence::Low),
    }
}

pub(super) fn whole(ctx: &DecodeContext<'_>, root: View<'_>) -> Result<Confidence, CodecError> {
    let manifest = match crate::archive::manifest(ctx, root) {
        Ok(value) => value,
        Err(crate::error::Error::Codec(
            error @ (CodecError::ResourceLimit(_) | CodecError::NotImplemented(_)),
        )) => return Err(error),
        Err(_) => return prefix(ctx, root),
    };
    Ok(if manifest["schema"] == crate::archive::SCHEMA {
        Confidence::High
    } else {
        Confidence::Low
    })
}
