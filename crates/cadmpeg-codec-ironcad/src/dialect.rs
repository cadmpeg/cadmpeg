// SPDX-License-Identifier: Apache-2.0
//! `IronCAD` source identity.

use cadmpeg_core::dialect::{DialectId, DialectLayers, DialectMatch};

include!("dialect/registry_ids.rs");

impl crate::IronCadCodec {
    /// Parasolid dialect grammars this host verifies under their own identities.
    pub const VERIFIED_PARASOLID_GRAMMARS: &'static [DialectId] =
        &[cadmpeg_core::dialect_id!("parasolid:format-13006")];
}

pub(crate) fn layers() -> DialectLayers {
    DialectLayers::of(DialectMatch::admitted(IRONCAD_CFB))
}
