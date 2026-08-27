// SPDX-License-Identifier: Apache-2.0
//! Reads Autodesk Inventor IPT and IAM documents into [`cadmpeg_ir::CadIr`].
//!
//! [`InventorCodec`] detects Inventor documents from the compound-file
//! directory structure. It does not classify unrelated CFB files as Inventor.
//!
//! <!-- generated: capability -->
//! Support: L1 ([ladder](https://github.com/cadmpeg/cadmpeg/blob/main/docs/format-support.md#autodesk-inventor-ipt-and-iam)).
//! <!-- /generated: capability -->

mod assembly;
mod container;
mod database;
mod decode;
mod design;
mod dialect;
mod external_reference;
mod feature;
#[doc(hidden)]
pub mod fuzz;
mod kernel;
/// Byte-offset constants generated from `docs/layouts/inventor.toml`.
pub(crate) mod layout;
#[allow(dead_code)] // Loss catalog is consumed by tests and the writer.
mod loss;
mod materials;
mod native;
mod pmdc;
mod presentation;
mod property_set;
mod protein;
mod records;
mod rse;
mod sketch;
mod validate;

use cadmpeg_container::compound::CompoundPrefixProbe;
use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::{CodecError, ContainerSummary};
use cadmpeg_ir::codec::{CodecBackend, Confidence, DecodeResult};
use cadmpeg_ir::{CadIr, Finding};

pub(crate) fn issue_detail(error: CodecError) -> Result<String, CodecError> {
    if matches!(&error, CodecError::ResourceLimit(_)) {
        Err(error)
    } else {
        Ok(error.to_string())
    }
}

/// Read-only Autodesk Inventor IPT and IAM codec.
#[derive(Debug, Default, Clone, Copy)]
pub struct InventorCodec;

impl CodecBackend for InventorCodec {
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
mod golden_tests;
#[cfg(test)]
pub(crate) mod test_support;
