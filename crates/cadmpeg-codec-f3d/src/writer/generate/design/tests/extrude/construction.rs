// SPDX-License-Identifier: Apache-2.0
//! The curves a whole-sketch profile walks: every curve of the sketch that
//! is no construction (cadir-v11.md §6 "Sources", §6.1 "Flags": construction
//! marks geometry that no profile uses).

use cadmpeg_ir::design::ops::sketch::PlanarCurve;

use super::super::sketch::{entity_id, point2};
use super::super::write;
use super::{blind, kinds, sketch_and_extrude_of, square};

/// A diagonal across the square closes no ring once it is construction, and
/// the square is written as the extrude; the same diagonal as profile
/// geometry keeps the Base Feature.
#[test]
fn a_whole_sketch_pick_walks_no_construction_curve() {
    let with_diagonal = |construction| {
        let mut sketch = square();
        sketch.entity(
            "diagonal",
            PlanarCurve::Line {
                start: point2(0.0, 0.0),
                end: point2(10.0, 10.0),
            },
            &[],
        );
        let mut ir = sketch_and_extrude_of(sketch, blind(), false);
        ir.design
            .sketch_entities
            .iter_mut()
            .find(|entity| entity.id == entity_id("profile", "diagonal"))
            .expect("the diagonal")
            .construction = construction;
        ir
    };
    let (report, decoded) = write(&with_diagonal(Some(true)));
    assert_eq!(
        kinds(&decoded),
        ["sketch", "extrude"],
        "{:#?}",
        report.losses
    );
    let (report, decoded) = write(&with_diagonal(None));
    assert_eq!(kinds(&decoded), ["sketch", "base"], "{:#?}", report.losses);
}
