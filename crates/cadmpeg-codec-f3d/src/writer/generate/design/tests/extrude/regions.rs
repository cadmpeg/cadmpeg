// SPDX-License-Identifier: Apache-2.0
//! The curves the `Extrude` of a profile walks: those of its picked regions,
//! or every curve of its picked sketch that is no construction (cadir-v11.md
//! §6 "Sources", §6.1 "Flags" and "Profiles and regions").

use cadmpeg_ir::design::feature::FeatureOperation;
use cadmpeg_ir::design::ops::sketch::PlanarCurve;
use cadmpeg_ir::design::selection::{
    Anchor, BoundaryUse, Members, PickTarget, Picked, Picks, Selection, SketchRegion,
};
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::scalar::PositiveLength;

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
fn region_of(ir: CadIr, outer: &[&str]) -> CadIr {
    regions_of(ir, &[(outer, &[])])
}

/// `ir` with its extrude's profile picked as `regions`, each its outer ring
/// and its holes by curve name.
fn regions_of(mut ir: CadIr, regions: &[(&[&str], &[&[&str]])]) -> CadIr {
    let ring = |names: &[&str]| {
        names
            .iter()
            .map(|name| BoundaryUse {
                entity: entity_id("profile", name),
                fragment: None,
            })
            .collect::<Vec<_>>()
    };
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
            Members::new(
                regions
                    .iter()
                    .map(|(outer, holes)| Picked {
                        target: PickTarget::SketchRegion {
                            sketch: sketch_id("profile"),
                            region: SketchRegion {
                                outer: ring(outer),
                                holes: holes.iter().map(|hole| ring(hole)).collect(),
                            },
                        },
                        context: None,
                        native: None,
                        domain: None,
                        witness: None,
                        resolved: None,
                    })
                    .collect(),
            )
            .expect("a pick"),
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

/// The square with a circle inside it, picked as the region with the hole
/// and as the disc inside the hole: the even-odd walk takes the circle once,
/// as a hole, so it writes neither pick and keeps the Base Feature.
#[test]
fn a_region_and_the_disc_of_its_hole_keep_the_base_feature() {
    let mut sketch = square();
    sketch.entity(
        "hole",
        PlanarCurve::Circle {
            center: point2(5.0, 5.0),
            radius: PositiveLength::new(2.0).expect("a radius"),
        },
        &[],
    );
    let ir = regions_of(
        sketch_and_extrude_of(sketch, blind(), false),
        &[(&["l1", "l2", "l3", "l4"], &[&["hole"]]), (&["hole"], &[])],
    );
    let (report, decoded) = write(&ir);
    assert_eq!(kinds(&decoded), ["sketch", "base"], "{:#?}", report.losses);
    assert!(
        loss(&report, EVALUATION_MISSING)
            .iter()
            .any(|message| message
                .contains("its profile's loops are not the rings of the regions it picks")),
        "{:#?}",
        report.losses
    );
}

/// A whole-sketch pick walks every curve that is no construction: a
/// diagonal across the square closes no ring once it is construction, and
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
