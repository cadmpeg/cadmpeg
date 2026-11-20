// SPDX-License-Identifier: Apache-2.0
//! Print the JSON Schema of [`CadIr`], which `scripts/generate-annex-a.py` reads.
//!
//! ```text
//! cargo run -p cadmpeg-ir --example emit_schema --features schema
//! ```
//!
//! [`CadIr`]: cadmpeg_ir::CadIr

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!(
        "{}",
        serde_json::to_string_pretty(&cadmpeg_ir::cadir_json_schema())?
    );
    Ok(())
}
