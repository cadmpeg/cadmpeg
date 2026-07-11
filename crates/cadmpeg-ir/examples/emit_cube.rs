// SPDX-License-Identifier: Apache-2.0
//! Print the canonical `.cadir.json` representation of [`unit_cube`].
//!
//! ```text
//! cargo run -p cadmpeg-ir --example emit_cube
//! ```
//!
//! [`unit_cube`]: cadmpeg_ir::examples::unit_cube

fn main() {
    let ir = cadmpeg_ir::examples::unit_cube();
    println!("{}", ir.to_canonical_json().expect("serialize cube"));
}
