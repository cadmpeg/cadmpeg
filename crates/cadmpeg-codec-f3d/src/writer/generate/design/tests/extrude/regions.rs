// SPDX-License-Identifier: Apache-2.0
//! The `Extrude` of a profile picked by its regions (cadir-v11.md §6.1,
//! "Profiles and regions").

use cadmpeg_ir::design::feature::FeatureOperation;
use cadmpeg_ir::design::ops::sketch::PlanarCurve;
use cadmpeg_ir::design::selection::{
    Anchor, BoundaryUse, Members, PickTarget, Picked, Picks, Selection, SketchRegion,
};
use cadmpeg_ir::document::CadIr;

use super::super::sketch::{entity_id, point2, sketch_id};
use super::super::{loss, write};
use super::{blind, kinds, sketch_and_extrude, sketch_and_extrude_of, square, EVALUATION_MISSING};

/// The cube's design with the extrude's profile picked as one region whose
/// outer ring is `outer`, lines of the square.
fn picked_region(outer: &[&str]) -> CadIr {
    region_of(sketch_and_extrude(blind(), false), outer)
}

/// `ir` with its extrude's profile picked as one region whose outer ring is
/// `outer`.
fn region_of(mut ir: CadIr, outer: &[&str]) -> CadIr {
    let extrude = ir
        .design
        .features
        .iter_mut()
        .find_map(|feature| match &mut feature.definition.operation {
            FeatureOperation::Extrude(extrude) => Some(extrude),
            _ => None,
        })
        .expect("the cube's extrude");
    extrude.profile = Selection {
        at: Anchor::Input,
        picks: Picks::Named(
            Members::new(vec![Picked {
                target: PickTarget::SketchRegion {
                    sketch: sketch_id("profile"),
                    region: SketchRegion {
                        outer: outer
                            .iter()
                            .map(|name| BoundaryUse {
                                entity: entity_id("profile", name),
                                fragment: None,
                            })
                            .collect(),
                        holes: Vec::new(),
                    },
                },
                context: None,
                native: None,
                domain: None,
                witness: None,
                resolved: None,
            }])
            .expect("one pick"),
        ),
        native: None,
    };
    ir
}

#[test]
fn the_region_its_sketch_bounds_is_written_as_the_extrude() {
    let (report, decoded) = write(&picked_region(&["l1", "l2", "l3", "l4"]));
    assert_eq!(
        kinds(&decoded),
        ["sketch", "extrude"],
        "{:#?}",
        report.losses
    );
}

/// A line of the sketch outside the picked region joins no loop: the whole
/// sketch is open, the region is the square.
#[test]
fn a_curve_outside_the_picked_region_joins_no_loop() {
    let mut sketch = square();
    sketch.entity(
        "tail",
        PlanarCurve::Line {
            start: point2(15.0, 0.0),
            end: point2(20.0, 0.0),
        },
        &[],
    );
    let whole = sketch_and_extrude_of(sketch, blind(), false);
    let (report, decoded) = write(&whole);
    assert_eq!(kinds(&decoded), ["sketch", "base"], "{:#?}", report.losses);
    let (report, decoded) = write(&region_of(whole, &["l1", "l2", "l3", "l4"]));
    assert_eq!(
        kinds(&decoded),
        ["sketch", "extrude"],
        "{:#?}",
        report.losses
    );
}

/// A region whose ring is no closed loop, or names a curve its sketch does
/// not hold, keeps the Base Feature with the reason.
#[test]
fn a_region_that_bounds_nothing_keeps_the_base_feature() {
    for (outer, reason) in [
        (&["l1", "l2", "l3"][..], "its profile is not closed"),
        (
            &["l1", "l2", "l3", "missing"][..],
            "its profile picks a curve that is construction or not in its sketch",
        ),
    ] {
        let (report, decoded) = write(&picked_region(outer));
        assert_eq!(kinds(&decoded), ["sketch", "base"], "{:#?}", report.losses);
        assert!(
            loss(&report, EVALUATION_MISSING)
                .iter()
                .any(|message| message.contains(reason)),
            "{reason}: {:#?}",
            report.losses
        );
    }
}
