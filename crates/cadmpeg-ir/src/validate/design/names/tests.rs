// SPDX-License-Identifier: Apache-2.0

use super::valid_pattern_index;
use crate::design::construction::Construction;
use crate::design::dimension::Dimension;
use crate::design::names::InstanceIndex;
use crate::design::ops::shared::{LinearRun, Locus, PatternLayout, PatternRun, Spacing};
use crate::scalar::{Length, PositiveReal};

fn linear(count: u32) -> PatternLayout {
    PatternLayout::Linear {
        first: LinearRun {
            direction: Construction::default(),
            run: PatternRun {
                spacing: Spacing::Pitch {
                    count: Dimension::Literal(count),
                    pitch: Dimension::Literal(Length::new(5.0).unwrap()),
                },
                sides: None,
                seed: None,
                increments: Vec::new(),
            },
        },
        second: None,
        seed_only: None,
    }
}

fn scale(count: u32) -> PatternLayout {
    PatternLayout::Scale {
        factor: Dimension::Literal(PositiveReal::new(2.0).unwrap()),
        count: Dimension::Literal(count),
        center: Locus::Centroid,
    }
}

fn mirror() -> PatternLayout {
    PatternLayout::Mirror {
        plane: Construction::default(),
        stitch_tolerance: None,
    }
}

fn valid(layout: &PatternLayout, index: &[i32]) -> Option<bool> {
    valid_pattern_index(layout, &InstanceIndex(index.to_vec()))
}

#[test]
fn a_composite_stage_numbers_its_coordinate_from_the_seed() {
    let sliced = PatternLayout::Composite(vec![linear(3), scale(3)]);
    assert_eq!(valid(&sliced, &[1, 0]), Some(true));
    assert_eq!(valid(&sliced, &[2, 2]), Some(true));
    assert_eq!(valid(&sliced, &[0, 1]), Some(true));
    assert_eq!(valid(&sliced, &[1, 3]), Some(false));
    assert_eq!(valid(&sliced, &[0, 0]), Some(false));

    let mirrored = PatternLayout::Composite(vec![linear(3), mirror()]);
    assert_eq!(valid(&mirrored, &[1, 0]), Some(true));
    assert_eq!(valid(&mirrored, &[0, 1]), Some(true));
    assert_eq!(valid(&mirrored, &[1, 2]), Some(false));

    assert_eq!(valid(&scale(3), &[2]), Some(true));
    assert_eq!(valid(&scale(3), &[3]), Some(false));
    assert_eq!(valid(&scale(3), &[0]), Some(false));
    assert_eq!(valid(&mirror(), &[1]), Some(true));
    assert_eq!(valid(&mirror(), &[0]), Some(false));
}
