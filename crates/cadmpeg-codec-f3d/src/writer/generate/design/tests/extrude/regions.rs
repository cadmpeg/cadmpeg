// SPDX-License-Identifier: Apache-2.0
//! The `Extrude` of a profile picked by its regions (cadir-v11.md §6.1,
//! "Profiles and regions").

use cadmpeg_ir::design::feature::FeatureOperation;
use cadmpeg_ir::design::selection::{
    Anchor, BoundaryUse, Members, PickTarget, Picked, Picks, Selection, SketchRegion,
};
use cadmpeg_ir::document::CadIr;

use super::super::sketch::{entity_id, sketch_id};
use super::super::{loss, write};
use super::{blind, kinds, sketch_and_extrude, EVALUATION_MISSING};

/// The cube's design with the extrude's profile picked as one region whose
/// outer ring is `outer`, lines of the square.
fn picked_region(outer: &[&str]) -> CadIr {
    let mut ir = sketch_and_extrude(blind(), false);
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

#[test]
fn a_region_that_leaves_a_curve_out_keeps_the_base_feature() {
    let (report, decoded) = write(&picked_region(&["l1", "l2", "l3"]));
    assert_eq!(kinds(&decoded), ["sketch", "base"], "{:#?}", report.losses);
    assert!(
        loss(&report, EVALUATION_MISSING)
            .iter()
            .any(|message| message.contains(
                "its profile picks regions that are not every region its sketch's curves bound"
            )),
        "{:#?}",
        report.losses
    );
}
