// SPDX-License-Identifier: Apache-2.0
//! Shared synthetic byte-fixture builders for crate tests.

#![allow(clippy::unwrap_used)]

use std::io::Write;

use cadmpeg_core::CodecError;
use cadmpeg_ir::codec::write::{EncodeInput, Encoder, TargetRequest};
use cadmpeg_ir::{CadIr, SourceFidelity, WritePath};

use crate::SldprtCodec;

/// Plans an inherited write through the sealed encoder and writes its bytes.
pub(crate) fn plan_inherited_write(
    ir: &CadIr,
    fidelity: &SourceFidelity,
    writer: &mut dyn Write,
) -> Result<WritePath, CodecError> {
    let plan = SldprtCodec.plan(EncodeInput::new(ir, Some(fidelity)), TargetRequest::Inherit)?;
    Ok(plan.write_to(writer)?.write_path())
}

mod appearance;
mod container;
mod history;
mod ir;
mod native;
mod parasolid;
mod pmi;
mod tessellation;

pub(crate) use appearance::*;
pub(crate) use container::*;
pub(crate) use history::*;
pub(crate) use ir::*;
pub(crate) use native::*;
pub(crate) use parasolid::*;
pub(crate) use pmi::*;
pub(crate) use tessellation::*;
