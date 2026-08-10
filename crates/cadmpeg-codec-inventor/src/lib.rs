// SPDX-License-Identifier: Apache-2.0
//! Reads Autodesk Inventor IPT and IAM documents into [`cadmpeg_ir::CadIr`].
//!
//! [`InventorCodec`] detects Inventor documents from the compound-file
//! directory structure. It does not classify unrelated CFB files as Inventor.

mod container;
mod database;
mod decode;
mod native;
mod rse;
mod validate;

use cadmpeg_container::compound::CompoundPrefixProbe;
use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::{CodecError, ContainerSummary};
use cadmpeg_ir::codec::{Codec, Confidence, DecodeResult};
use cadmpeg_ir::{CadIr, Finding};

/// Read-only Autodesk Inventor IPT and IAM codec.
#[derive(Debug, Default, Clone, Copy)]
pub struct InventorCodec;

impl Codec for InventorCodec {
    fn id(&self) -> &'static str {
        "inventor"
    }

    fn detect(&self, prefix: &[u8]) -> Confidence {
        let CompoundPrefixProbe::DirectoryEvidence(paths) = CompoundPrefixProbe::inspect(prefix)
        else {
            return Confidence::No;
        };
        if container::has_inventor_evidence(&paths) {
            Confidence::High
        } else {
            Confidence::No
        }
    }

    fn inspect_impl(
        &self,
        ctx: &DecodeContext<'_>,
        root: View<'_>,
    ) -> Result<ContainerSummary, CodecError> {
        Ok(
            container::InventorContainer::open(ctx, root, container::ContainerPurpose::Inspect)?
                .summary(),
        )
    }

    fn decode_impl(
        &self,
        ctx: &DecodeContext<'_>,
        root: View<'_>,
    ) -> Result<DecodeResult, CodecError> {
        decode::decode(ctx, root)
    }
}

/// Validates the typed Inventor-native namespace.
pub fn validate_native(ir: &CadIr) -> Vec<Finding> {
    validate::validate_native(ir)
}

#[cfg(test)]
mod tests;
