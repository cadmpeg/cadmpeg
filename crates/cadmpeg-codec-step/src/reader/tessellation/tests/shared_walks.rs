// SPDX-License-Identifier: Apache-2.0
//! Tessellation association follows each shared item once per placement.

use std::fmt::Write as _;

use cadmpeg_core::decode::DecodePolicy;
use cadmpeg_ir::math::Point3;
use cadmpeg_ir::report::loss::LossNote;

use super::{assert_point3_close, decode_tessellation_stage};
use crate::loss::StepLossCode;

const LEAF: &str = "#1=COORDINATES_LIST('',3,((0.,0.,0.),(1.,0.,0.),(0.,1.,0.)));\n#2=TRIANGULATED_SURFACE_SET('',#1,3,$,$,((1,2,3)));\n#90=DIRECTION('',(0.,0.,1.));\n#91=DIRECTION('',(1.,0.,0.));\n";

/// Nested sets in which level `k` holds the level below once in place and
/// once moved by `2^k` along x, so the leaf `#2` is reached through
/// `2^levels - 1` distinct placements and once unmoved.
fn repositioned_levels(levels: u32) -> String {
    let mut records = String::from(LEAF);
    let mut below = 2;
    for level in 1..=levels {
        let point = 100 + 10 * level;
        let (placement, moved, set) = (point + 1, point + 2, point + 3);
        let offset = 2_u64.pow(level);
        writeln!(
            records,
            "#{point}=CARTESIAN_POINT('',({offset}.,0.,0.));\n#{placement}=AXIS2_PLACEMENT_3D('',#{point},#90,#91);\n#{moved}=(GEOMETRIC_REPRESENTATION_ITEM() REPOSITIONED_TESSELLATED_ITEM(#{placement}) REPRESENTATION_ITEM('') TESSELLATED_GEOMETRIC_SET((#{below})) TESSELLATED_ITEM());\n#{set}=TESSELLATED_GEOMETRIC_SET('',(#{below},#{moved}));"
        )
        .expect("write level");
        below = set;
    }
    writeln!(
        records,
        "#9000=TESSELLATED_ANNOTATION_OCCURRENCE('',(),#{below});"
    )
    .expect("write occurrence");
    records
}

fn placement_losses(losses: &[LossNote]) -> Vec<&str> {
    losses
        .iter()
        .filter(|loss| loss.code == StepLossCode::TessellationPlacementAmbiguous.kind())
        .map(|loss| loss.message.as_str())
        .collect()
}

#[test]
fn shared_sets_are_followed_once_per_placement() {
    let mut records = String::from(LEAF);
    for id in 3..=42 {
        writeln!(
            records,
            "#{id}=TESSELLATED_GEOMETRIC_SET('',(#{},#{}));",
            id - 1,
            id - 1
        )
        .expect("write shared set");
    }
    records.push_str("#43=TESSELLATED_SOLID('',(#42),$);");
    let (ir, losses) = decode_tessellation_stage(&records, DecodePolicy::service())
        .expect("decode the tessellation");
    assert_eq!(ir.model.tessellations.len(), 1);
    assert!(placement_losses(&losses).is_empty());
}

#[test]
fn distinct_placements_within_the_limit_are_counted() {
    let (ir, losses) = decode_tessellation_stage(&repositioned_levels(5), DecodePolicy::service())
        .expect("decode the tessellation");
    assert_point3_close(
        ir.model.tessellations[0].vertices()[1].get(),
        Point3::new(1.0, 0.0, 0.0),
    );
    assert_eq!(
        placement_losses(&losses),
        ["tessellation item #2 has 31 distinct repositioning placements; mesh retained in source coordinates"]
    );
}

#[test]
fn distinct_placements_past_the_limit_keep_source_coordinates() {
    let (ir, losses) = decode_tessellation_stage(&repositioned_levels(30), DecodePolicy::service())
        .expect("decode the tessellation");
    assert_point3_close(
        ir.model.tessellations[0].vertices()[1].get(),
        Point3::new(1.0, 0.0, 0.0),
    );
    assert_eq!(
        placement_losses(&losses),
        ["tessellation item #2 has more than 64 distinct repositioning placements; mesh retained in source coordinates"]
    );
}
