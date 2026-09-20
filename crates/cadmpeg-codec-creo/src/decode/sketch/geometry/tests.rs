// SPDX-License-Identifier: Apache-2.0

use super::*;
use crate::feature::definitions::{FeatureSegment, FeatureSegmentKind};
#[test]
fn numerical_ranges_section_arc_radius_agreement_has_no_length_floor() {
    let segment = FeatureSegment {
        kind: FeatureSegmentKind::Arc([1, 2]),
        directions: [None; 3],
        center_id: Some(3),
        arc_orientation: Some(0),
        vertical_horizontal: None,
        radius_ref: None,
        radius2_ref: None,
        external_id: 4,
        body: Vec::new(),
        offset: 9,
    };
    for radius in [1e-10, 1.0, 1e100] {
        for (factor, accepted) in [(1.0, true), (5.0, false)] {
            let points =
                BTreeMap::from([(1, [radius, 0.]), (2, [0., factor * radius]), (3, [0., 0.])]);
            assert_eq!(section_arc_geometry(&points, &segment).is_some(), accepted);
        }
    }
}

#[test]
fn numerical_ranges_saved_section_arc_rejects_different_tiny_radii() {
    use crate::feature::definitions::*;
    let segment = FeatureSegment {
        kind: FeatureSegmentKind::Arc([1, 2]),
        directions: [None; 3],
        center_id: Some(3),
        arc_orientation: Some(0),
        vertical_horizontal: None,
        radius_ref: None,
        radius2_ref: None,
        external_id: 4,
        body: Vec::new(),
        offset: 9,
    };
    for factor in [1.0, 5.0] {
        let radius = 1e-10;
        let definition = FeatureDefinition {
            identity: DefinitionIdentity::Parsed {
                schema_id: std::num::NonZeroU32::new(5),
                owner_feature_id: Some(6),
            },
            body: Vec::new(),
            parameter_frames: Vec::new(),
            outlines: Vec::new(),
            variables: None,
            segments: None,
            trim_entities: None,
            trim_vertices: None,
            order_table: Some(FeatureOrderTable {
                declared_count: 1,
                has_prototype: false,
                entity_ref: None,
                rows: vec![FeatureOrderRow {
                    external_id: 4,
                    internal_id: 30,
                    bitmask: 0,
                    offset: 10,
                }],
                offset: 8,
            }),
            section_3d: None,
            dimensions: None,
            relations: None,
            saved_section: Some(FeatureSavedSection {
                entities: vec![FeatureSavedEntity::Arc(FeatureSavedArc {
                    entity_id: 30,
                    center: [Some(0.), Some(0.), Some(0.)],
                    radius: None,
                    endpoints: [
                        [Some(radius), Some(0.), Some(0.)],
                        [Some(0.), Some(radius * factor), Some(0.)],
                    ],
                    parameters: [None; 2],
                    body: Vec::new(),
                    offset: 20,
                })],
                offset: 18,
            }),
            offset: 0,
        };
        assert_eq!(
            saved_section_arc_carrier(&definition, &segment).is_some(),
            factor == 1.0
        );
    }
}
