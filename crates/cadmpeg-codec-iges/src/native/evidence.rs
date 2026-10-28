// SPDX-License-Identifier: Apache-2.0
//! Source-only evidence that cannot count as typed document content.

use cadmpeg_ir::CadIr;

pub(crate) fn untyped_records(ir: &CadIr) -> Vec<String> {
    ir.native
        .namespace("iges")
        .and_then(|namespace| namespace.arenas().get("macro_definitions"))
        .into_iter()
        .flatten()
        .map(|record| record.id().to_owned())
        .collect()
}

#[cfg(test)]
mod tests;
