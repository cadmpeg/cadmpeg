// SPDX-License-Identifier: Apache-2.0
use cadmpeg_core::decode::{DecodeContext, ResourceLimit};
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::index::ModelIndex;

pub fn complete_index(ctx: &DecodeContext<'_>, ir: &CadIr) -> Result<(), ResourceLimit> {
    let _index = ModelIndex::build(ir, ctx)?;
    Ok(())
}

pub fn model_only_index(ctx: &DecodeContext<'_>, ir: &CadIr) -> Result<(), ResourceLimit> {
    let _index = ModelIndex::new_model_only(ir, ctx)?;
    Ok(())
}
