// SPDX-License-Identifier: Apache-2.0
//! Decode, export, loss, and validation reports.

mod check;
mod decode;
mod export;
mod loss;

pub use check::{Check, Finding, ValidationReport};
pub use decode::{
    CoverageKey, DecodeReport, DecodeTransfer, TransferDisposition, TransferLedger, TransferRecord,
};
pub use export::{CensusBasis, EntityCensus, ExportReport, FidelityResolution, WritePath};
pub use loss::{
    LossCategory, LossKind, LossNote, LossTaxonomy, Severity, StrictConsequence,
    SHARED_LOSS_NAMESPACE,
};

#[cfg(test)]
mod tests;
