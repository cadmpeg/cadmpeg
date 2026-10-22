// SPDX-License-Identifier: Apache-2.0
//! Source-bound projection links from actual typed entity accessors.

use anyhow::{Context, Result};
use serde::Serialize;
use sha2::{Digest, Sha256};

use super::{print_json, read_input, QueryArgs};

#[derive(Serialize)]
struct Link<'a> {
    arena: &'static str,
    rust_type: &'static str,
    id: &'a str,
    native_ref: Option<&'a str>,
}

pub(super) fn run(args: &QueryArgs) -> Result<()> {
    let bytes = read_input(&args.file)?;
    let text = std::str::from_utf8(&bytes).context("CADIR is not UTF-8")?;
    let ir = cadmpeg_ir::CadIr::from_json(text).context("admitting CADIR projection source")?;
    let mut rows = Vec::new();
    cadmpeg_ir::schema::query::visit_native_references(
        &ir.model,
        &mut |arena, rust_type, id, native_ref| {
            rows.push(Link {
                arena,
                rust_type,
                id,
                native_ref,
            });
        },
    );
    if args.json {
        print_json(
            "native_links",
            serde_json::json!({
                "protocol": "cadmpeg-typed-native-links-v1",
                "ir_sha256": cadmpeg_ir::hash::LowerHex(&Sha256::digest(&bytes)).to_string(),
                "rows": rows,
            }),
        )?;
    } else {
        println!("arena\tid\tnative_ref");
        for row in rows {
            println!(
                "{}\t{}\t{}",
                row.arena,
                row.id,
                row.native_ref.unwrap_or("")
            );
        }
    }
    Ok(())
}
