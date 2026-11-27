// SPDX-License-Identifier: Apache-2.0
//! Compare the box pass with the full pass on one `.cadir.json` read from
//! standard input, and print one JSON line: the count of bodies in each
//! agreement class and the bodies that disagree. Run it over a corpus
//! sample after any change to the box pass.
//!
//! ```text
//! cargo run --release -p cadmpeg-measure --features test-support --example box_pass < model.cadir.json
//! ```

use std::collections::BTreeMap;
use std::io::Read;

use cadmpeg_ir::{CadIr, ReadOptions};
use cadmpeg_measure::test_support::box_pass::{agreements, Agreement};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut bytes = Vec::new();
    std::io::stdin().read_to_end(&mut bytes)?;
    let ir = CadIr::read(&bytes, &ReadOptions::default())?
        .without_sidecar()
        .0;
    let bodies = agreements(&ir)?;
    let mut counts: BTreeMap<String, usize> = BTreeMap::new();
    for body in &bodies {
        *counts.entry(format!("{:?}", body.agreement)).or_default() += 1;
    }
    let disagree: Vec<&str> = bodies
        .iter()
        .filter(|body| body.agreement == Agreement::Disagree)
        .map(|body| body.body.as_str())
        .collect();
    println!(
        "{}",
        cadmpeg_core::json!({
            "bodies": bodies.len(),
            "counts": counts,
            "disagree": disagree,
        })
    );
    Ok(())
}
