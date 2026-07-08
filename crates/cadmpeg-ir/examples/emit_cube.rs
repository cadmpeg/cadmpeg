// SPDX-License-Identifier: Apache-2.0
//! Print the canonical `.cadir.json` for the worked unit-cube example.
//!
//! This is the exact command used to generate the serialized document embedded
//! in `docs/cad-ir.md`:
//!
//! ```text
//! cargo run -p cadmpeg-ir --example emit_cube
//! ```

fn main() {
    let ir = cadmpeg_ir::examples::unit_cube();
    println!("{}", ir.to_canonical_json().expect("serialize cube"));
}
