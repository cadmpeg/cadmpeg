// SPDX-License-Identifier: Apache-2.0
//! Write-side target resolution and the sealed encoder contract.

mod backend;
mod resolve;

pub use backend::{
    CadirEncoder, EncodeInput, Encoder, EncoderBackend, EncoderTargetDomain, ExportPlan,
    ResolvedEncoderTarget,
};
#[cfg(test)]
pub(super) use resolve::resolve_write_request;
pub use resolve::{ResolvedWrite, TargetRequest};
