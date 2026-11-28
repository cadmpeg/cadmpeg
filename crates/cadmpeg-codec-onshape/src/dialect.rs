// SPDX-License-Identifier: Apache-2.0
//! Bundle schema classification.
use cadmpeg_core::dialect::{DialectLayers, DialectMatch};
use cadmpeg_core::CodecError;
#[path = "dialect/registry_ids.rs"]
pub(crate) mod registry_ids;
use self::registry_ids::ONSHAPE_BUNDLE_1;
pub(super) fn layers() -> DialectLayers {
    DialectLayers::of(
        DialectMatch::admitted(ONSHAPE_BUNDLE_1).with_declared_entry(
            cadmpeg_core::nonblank_const!("schema"),
            crate::archive::SCHEMA,
        ),
    )
}
pub(super) fn validate(manifest: &serde_json::Value) -> Result<(), CodecError> {
    if manifest["schema"] == crate::archive::SCHEMA {
        Ok(())
    } else {
        Err(CodecError::malformed("unsupported Onshape bundle schema"))
    }
}
