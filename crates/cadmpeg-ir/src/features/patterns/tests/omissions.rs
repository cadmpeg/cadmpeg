// SPDX-License-Identifier: Apache-2.0

use super::*;

fn grid<C>(omitted_instances: Vec<[u32; 2]>) -> PatternTransform<C> {
    PatternTransform::Linear {
        direction: Some(direction(1.0, 0.0, 0.0)),
        distance: PositiveLength::new(3.0).unwrap().into(),
        count: Dimension::Literal(3),
        second: Some(LinearPatternDirection {
            direction: direction(0.0, 1.0, 0.0),
            distance: PositiveLength::new(4.0).unwrap().into(),
            count: Dimension::Literal(2),
        }),
        omitted_instances,
    }
}

#[test]
fn omissions_reject_seed_duplicates_and_each_out_of_bounds_coordinate() {
    for omitted in [
        vec![[0, 0]],
        vec![[3, 0]],
        vec![[0, 2]],
        vec![[1, 0], [1, 0]],
    ] {
        assert!(PatternKind::<CompositePattern>::new(grid(omitted)).is_err());
    }
    let mut transform = linear::<CompositePattern>(3);
    let PatternTransform::Linear {
        omitted_instances, ..
    } = &mut transform
    else {
        panic!("linear pattern");
    };
    omitted_instances.push([1, 1]);
    assert!(PatternKind::new(transform).is_err());
}

#[test]
fn grid_counts_include_both_directions_and_exclude_omitted_positions() {
    assert_eq!(
        PatternKind::<CompositePattern>::new(grid(Vec::new()))
            .unwrap()
            .occurrence_count(),
        Some(6)
    );
    let pattern = PatternKind::<CompositePattern>::new(grid(vec![[2, 1], [1, 0]])).unwrap();
    assert_eq!(pattern.occurrence_count(), Some(4));
    let wire = serde_json::to_value(&pattern).unwrap();
    assert_eq!(wire["count"], json!({"kind": "literal", "value": 3}));
    assert_eq!(
        wire["second"]["count"],
        json!({"kind": "literal", "value": 2})
    );
    assert_eq!(wire["omitted_instances"], json!([[2, 1], [1, 0]]));
    assert_eq!(
        serde_json::from_value::<PatternKind>(wire).unwrap(),
        pattern
    );
    let empty =
        serde_json::to_value(PatternKind::<CompositePattern>::new(grid(Vec::new())).unwrap())
            .unwrap();
    assert!(empty.get("omitted_instances").is_none());
    assert!(serde_json::from_value::<PatternKind>(empty).is_ok());
}

#[test]
fn only_the_seed_can_remain_and_composite_counts_use_retained_instances() {
    let retained =
        PatternKind::<CompositePattern>::new(grid(vec![[1, 0], [2, 0], [0, 1], [1, 1], [2, 1]]))
            .unwrap();
    assert_eq!(retained.occurrence_count(), Some(1));
    let stages =
        CompositePattern::new(vec![stage(grid(vec![[2, 1], [1, 0]])), stage(linear(3))]).unwrap();
    let composite = PatternKind::new(PatternTransform::Composite { stages }).unwrap();
    assert_eq!(composite.occurrence_count(), Some(12));
}

#[test]
fn length_edits_and_stage_widening_keep_grid_omissions() {
    let pattern = PatternKind::new(grid::<crate::features::patterns::NoNestedComposite>(vec![
        [2, 1],
    ]))
    .unwrap();
    let widened: PatternKind = pattern.widen();
    let scaled = widened
        .try_map_lengths::<()>(&mut |field| {
            if let PatternLengthField::Length(value) = field {
                *value = Length::new(value.get() * 2.0).unwrap();
            }
            Ok(())
        })
        .unwrap();
    assert!(
        matches!(scaled.definition(), PatternTransform::Linear { omitted_instances, .. } if omitted_instances == &[[2, 1]])
    );
    assert_eq!(scaled.occurrence_count(), Some(5));
}
