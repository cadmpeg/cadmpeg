// SPDX-License-Identifier: Apache-2.0
//! Write-side target resolution and the sealed encoder contract.

mod backend;
mod resolve;
#[cfg(test)]
mod tests;

pub use backend::{
    CadirEncoder, Catalog, Consumption, DialectFree, EncodeInput, Encoder, EncoderBackend,
    ExportBody, ExportPlan, TargetDomain,
};
pub use resolve::{ResolvedTarget, ResolvedWrite, SourceIdentity, TargetRequest};
