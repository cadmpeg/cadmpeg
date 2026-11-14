// SPDX-License-Identifier: Apache-2.0

use super::*;

const COMPACT_UNION_LINEAR_TOLERANCE: f64 = 1.0e-7;
const COMPACT_UNION_ANGULAR_TOLERANCE: f64 = 1.0e-9;

#[test]
fn compact_exteriors_resolve_subdivisions_and_refuse_descendant_groups() {
    let placements = [placement()];
    let sketch_id = neutral_sketch_id(&placements[0]);
    let ends = [
        ((0.0, 0.0), (4.0, 0.0)),
        ((4.0, 0.0), (4.0, 3.0)),
        ((4.0, 3.0), (0.0, 3.0)),
        ((0.0, 3.0), (0.0, 0.0)),
        ((2.0, 0.0), (2.0, 3.0)),
    ];
    let entities = ends
        .iter()
        .zip(1u64..)
        .map(|((a, b), id)| {
            SketchEntity::new(
                neutral_sketch_curve_id(&sketch_id, id, 0),
                sketch_id.clone(),
                SketchGeometry::try_from(SketchGeometryDefinition::Line {
                    start: Point2::new(a.0, a.1),
                    end: Point2::new(b.0, b.1),
                })
                .unwrap(),
            )
        })
        .collect::<Vec<_>>();
    let curves = (1u32..=5)
        .map(|id| curve(id + 30, u64::from(id), 0))
        .collect::<Vec<_>>();
    let sketches = [Sketch {
        id: sketch_id,
        name: None,
        configuration: None,
        visible: None,
        placement: SketchPlacement::Unresolved {},
        profiles: cadmpeg_ir::sketches::SketchProfiles::default(),
        native_ref: None,
    }];
    let mut selections = [selection(10, 0, 1, true), selection(11, 1, 1, true)];
    for value in &mut selections {
        value.groups[0].region[0].entries = (1u64..=4)
            .map(|primary| DesignProfileCurveEntry {
                primary_id: primary,
                primary_id_offset: 548 + 40 * (primary - 1),
                secondary_id: 0,
                entry_fields: [0, 1, 1],
            })
            .collect();
        value.identity_end_offset = value.lists_end().unwrap();
        assert!(value.compact_references());
        assert!(value.valid_layout());
    }
    let work = ProfileGeometryWork::new();
    let scope_histories = HashMap::new();
    let resolve = |values: &[DesignProfileCurveSelection]| {
        let mut resolution =
            planar_resolution(&[], &placements, &curves, &sketches, &entities).path_resolution();
        resolution.profile_curves = values;
        crate::test_support::with_decode_context(|ctx| {
            let scoped = ExtrudeProfileResolution {
                entities: &entities,
                spatial_sketches: &[],
                spatial_entities: &[],
                histories: &[],
                scope_histories: &scope_histories,
                linear_tolerance: COMPACT_UNION_LINEAR_TOLERANCE,
                angular_tolerance: COMPACT_UNION_ANGULAR_TOLERANCE,
                profile_work: &work,
                ctx,
            }
            .scoped(&[]);
            super::super::super::resolve_profile_curve_arrangement_regions_with_policy(
                &group(),
                &resolution,
                scoped,
                false,
            )
        })
        .unwrap()
    };
    let Some(cadmpeg_ir::features::ProfileRef::Planar(
        cadmpeg_ir::features::PlanarProfileRef::SketchRegions { regions, .. },
    )) = resolve(&selections)
    else {
        panic!("exact compact exterior union");
    };
    assert_eq!(regions.iter().count(), 2);
    assert!(regions
        .iter()
        .all(|region| matches!(region, SketchProfileRegion::Trimmed { .. })));
    let mut bad_counts = selections.clone();
    bad_counts[0].groups[0].region[0].entries[0].entry_fields = [0, 2, 2];
    assert!(resolve(&bad_counts).is_none());
    let mut descendants = selections.clone();
    let mut further = descendants[0].groups[0].region.clone();
    for (ordinal, entry) in further[0].entries.iter_mut().enumerate() {
        entry.primary_id_offset =
            descendants[0].identity_end_offset + 12 + u64_from_index(ordinal) * 40;
    }
    descendants[0].groups[0].further.push(further);
    descendants[0].identity_end_offset = descendants[0].lists_end().unwrap();
    assert!(descendants[0].valid_layout());
    assert!(resolve(&descendants).is_none());
    assert!(work.losses().is_empty());
}
