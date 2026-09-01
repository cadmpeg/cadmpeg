// SPDX-License-Identifier: Apache-2.0
//! Write-side target resolution and the sealed encoder contract.

mod backend;
mod resolve;

pub use backend::{
    CadirEncoder, Catalog, Consumption, DialectFree, EncodeInput, Encoder, EncoderBackend,
    ExportBody, ExportPlan, TargetDomain,
};
#[cfg(test)]
pub(super) use resolve::resolve_write_request;
pub use resolve::{ResolvedTarget, ResolvedWrite, SourceIdentity, TargetRequest};
