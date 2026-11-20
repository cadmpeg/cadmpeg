// SPDX-License-Identifier: Apache-2.0
//! Time the box pass and the service-profile full pass over one
//! `.cadir.json` read from standard input, and print one JSON line: the
//! counts, both times, the box kinds and how many body boxes of the two
//! passes are equal.
//!
//! ```text
//! cargo run --release -p cadmpeg-measure --example body_boxes < model.cadir.json
//! ```

use std::io::Read;
use std::time::Instant;

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_ir::CadIr;
use cadmpeg_measure::report::BodyMeasures;
use cadmpeg_measure::value::{BoxKind, Quantity};
use cadmpeg_measure::{body_boxes, MeasureInput, MeasureOptions, Measurer, Selection, WorkProfile};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut text = String::new();
    std::io::stdin().read_to_string(&mut text)?;
    let ir = CadIr::from_json(&text)?;

    let start = Instant::now();
    let boxes = body_boxes(&ir);
    let box_seconds = start.elapsed().as_secs_f64();

    let arena = DecodeArena::new();
    let ctx = DecodeContext::for_loaded_input(&arena, &DecodePolicy::service(), 0)?;
    let input = MeasureInput {
        ir: &ir,
        fidelity: None,
        geometry_transferred: true,
    };
    let options = MeasureOptions {
        work: WorkProfile::Service,
        density: None,
    };
    let start = Instant::now();
    let full = Measurer::new(input, options).measure(&ctx, &Selection::default());
    let full_seconds = start.elapsed().as_secs_f64();

    let rows: Vec<&BodyMeasures> = full
        .as_ref()
        .map(|report| report.bodies().iter().collect())
        .unwrap_or_default();
    let (mut tight, mut conservative, mut unavailable, mut equal) = (0, 0, 0, 0);
    for body_box in boxes.as_deref().unwrap_or_default() {
        match body_box.bbox() {
            Quantity::Measured(measured) if measured.value().kind == BoxKind::Tight => tight += 1,
            Quantity::Measured(_) => conservative += 1,
            Quantity::Unavailable(_) => unavailable += 1,
        }
        let same = rows.iter().any(|row| {
            row.body() == body_box.body()
                && serde_json::to_value(row.bbox()).ok()
                    == serde_json::to_value(body_box.bbox()).ok()
        });
        equal += usize::from(same);
    }
    println!(
        "{}",
        serde_json::json!({
            "faces": ir.geometry.faces.len(),
            "edges": ir.geometry.edges.len(),
            "bodies": ir.geometry.bodies.len(),
            "box_seconds": box_seconds,
            "box_error": boxes.as_ref().err().map(ToString::to_string),
            "tight": tight,
            "conservative": conservative,
            "unavailable": unavailable,
            "full_seconds": full_seconds,
            "full_error": full.as_ref().err().map(ToString::to_string),
            "full_measured": rows.iter().filter(|row| row.bbox().measured().is_some()).count(),
            "equal": equal,
        })
    );
    Ok(())
}
