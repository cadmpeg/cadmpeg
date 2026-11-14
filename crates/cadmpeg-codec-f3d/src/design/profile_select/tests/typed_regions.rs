// SPDX-License-Identifier: Apache-2.0

use super::*;

mod compact_binding;
mod compact_exterior;
use crate::records::topology::entity_selection::{
    DesignProfileCurveEntry, DesignProfileCurveLoop, DesignProfileCurveSelection,
};

fn selection(
    record_index: u32,
    ordinal: u32,
    primary: u64,
    outer: bool,
) -> DesignProfileCurveSelection {
    let identity_record_offset = 500;
    let mut selection = DesignProfileCurveSelection {
        id: format!("stream:profile-{record_index}"),
        scope_record_index: 7,
        group_record_index: 9,
        group_member_ordinal: ordinal,
        record_index,
        byte_offset: 100,
        owner_text: "42".into(),
        owner_text_offset: 200,
        sketch_scope_record_index: 7,
        sketch_scope_offset: 300,
        identity_record_offset,
        identity_end_offset: 629,
        groups: vec![
            crate::records::topology::entity_selection::DesignProfileRegionGroup {
                region: vec![DesignProfileCurveLoop {
                    outer,
                    entries: vec![DesignProfileCurveEntry {
                        primary_id: primary,
                        primary_id_offset: 588,
                        secondary_id: 0,
                        entry_fields: [1, 1, 1],
                    }],
                }],
                further: vec![],
            },
        ],
    };
    selection.identity_end_offset = selection.lists_end().unwrap_or(0);
    selection
}

#[test]
fn typed_profile_regions_preserve_holes_and_refuse_ambiguous_loops() {
    let mut group = group();
    group.operand_role =
        crate::records::topology::construction::DesignConstructionOperandRole::Other(
            DesignOperandRole::PROFILE,
        );
    let placements = [placement()];
    let sketch_id = neutral_sketch_id(&placements[0]);
    let curves = [curve(31, 11, 0), curve(32, 12, 0), curve(33, 13, 0)];
    let entities: Vec<_> = [(11, 9.0), (12, 2.0), (13, 1.0)]
        .into_iter()
        .map(|(id, radius)| {
            SketchEntity::new(
                neutral_sketch_curve_id(&sketch_id, id, 0),
                sketch_id.clone(),
                SketchGeometry::try_from(SketchGeometryDefinition::Circle {
                    center: Point2::new(0.0, 0.0),
                    radius: Length::new(radius).unwrap(),
                })
                .unwrap(),
            )
        })
        .collect();
    let mut sketches = [Sketch {
        id: sketch_id.clone(),
        name: None,
        configuration: None,
        visible: None,
        placement: SketchPlacement::try_resolved(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
        )
        .unwrap(),
        profiles: cadmpeg_ir::sketches::SketchProfiles::try_from(
            entities
                .iter()
                .map(|entity| {
                    vec![SketchEntityUse {
                        entity: entity.id().clone(),
                        reversed: false,
                    }]
                })
                .collect::<Vec<_>>(),
        )
        .unwrap(),
        native_ref: None,
    }];
    let mut first = selection(10, 0, 11, true);
    let mut hole = selection(10, 0, 12, true).groups[0].region[0].clone();
    hole.outer = false;
    first.groups[0].region.push(hole);
    first.groups[0].region[1].entries[0].primary_id_offset = 633;
    first.identity_end_offset = first.lists_end().unwrap();
    let second = selection(11, 1, 13, true);
    let mut selections = [first, second];
    let resolve = |sketches: &[Sketch], selections: &[DesignProfileCurveSelection]| {
        let mut resolution =
            planar_resolution(&[], &placements, &curves, sketches, &entities).path_resolution();
        resolution.profile_curves = selections;
        resolve_entity_selection_profile(&group, &resolution)
    };
    assert_eq!(
        resolve(&sketches, &selections),
        Some(cadmpeg_ir::features::ProfileRef::Planar(
            cadmpeg_ir::features::PlanarProfileRef::sketch_regions(
                sketch_id.clone(),
                vec![
                    SketchProfileRegion::loops(0, vec![1]).unwrap(),
                    SketchProfileRegion::loops(2, vec![]).unwrap()
                ]
            )
            .unwrap()
        ))
    );
    let mut path_resolution =
        planar_resolution(&[], &placements, &curves, &sketches, &entities).path_resolution();
    path_resolution.profile_curves = &selections;
    assert!(super::super::resolve_profile_curve_path(&group, "stream", &path_resolution).is_none());
    let mut repeated = selections.clone();
    repeated[1].groups = repeated[0].groups.clone();
    repeated[1].identity_end_offset = repeated[0].identity_end_offset;
    assert_eq!(
        resolve(&sketches, &repeated),
        Some(cadmpeg_ir::features::ProfileRef::Planar(
            cadmpeg_ir::features::PlanarProfileRef::sketch_regions(
                sketch_id.clone(),
                vec![SketchProfileRegion::loops(0, vec![1]).unwrap()],
            )
            .unwrap()
        ))
    );
    selections[0].groups[0].region[1].entries[0].primary_id = 11;
    assert!(resolve(&sketches, &selections).is_none());
    selections[0].groups[0].region[1].entries[0].primary_id = 12;
    let mut profiles = sketches[0].profiles.iter().cloned().collect::<Vec<_>>();
    profiles.push(profiles[0].clone());
    sketches[0].profiles = cadmpeg_ir::sketches::SketchProfiles::try_from(profiles).unwrap();
    assert!(resolve(&sketches, &selections).is_none());
}

#[test]
fn compact_profile_loops_select_exact_subsets_and_the_full_profile_table() {
    const LINEAR_TOLERANCE: f64 = 1.0e-7;
    const ANGULAR_TOLERANCE: f64 = 1.0e-9;
    let mut group = group();
    group.operand_role =
        crate::records::topology::construction::DesignConstructionOperandRole::Other(
            DesignOperandRole::PROFILE,
        );
    let placements = [placement()];
    let sketch_id = neutral_sketch_id(&placements[0]);
    let curves = [curve(31, 11, 0), curve(32, 12, 0), curve(33, 13, 0)];
    let entities: Vec<_> = [(11, 9.0), (12, 2.0), (13, 1.0)]
        .into_iter()
        .map(|(id, radius)| {
            SketchEntity::new(
                neutral_sketch_curve_id(&sketch_id, id, 0),
                sketch_id.clone(),
                SketchGeometry::try_from(SketchGeometryDefinition::Circle {
                    center: Point2::new(0.0, 0.0),
                    radius: Length::new(radius).unwrap(),
                })
                .unwrap(),
            )
        })
        .collect();
    let sketches = [Sketch {
        id: sketch_id.clone(),
        name: None,
        configuration: None,
        visible: None,
        placement: SketchPlacement::try_resolved(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
        )
        .unwrap(),
        profiles: cadmpeg_ir::sketches::SketchProfiles::try_from(
            entities
                .iter()
                .map(|entity| {
                    vec![SketchEntityUse {
                        entity: entity.id().clone(),
                        reversed: false,
                    }]
                })
                .collect::<Vec<_>>(),
        )
        .unwrap(),
        native_ref: None,
    }];

    let mut first = selection(10, 0, 13, true);
    let mut second = selection(11, 1, 11, true);
    for value in [&mut first, &mut second] {
        value.groups[0].region[0].entries[0].primary_id_offset -= 40;
        value.identity_end_offset -= 40;
    }
    let resolve = |selections: &[DesignProfileCurveSelection]| {
        let mut resolution =
            planar_resolution(&[], &placements, &curves, &sketches, &entities).path_resolution();
        resolution.profile_curves = selections;
        super::super::compact_profile_loop_selection(&group, &resolution)
    };
    let expected = |indices| {
        Some(cadmpeg_ir::features::ProfileRef::Planar(
            cadmpeg_ir::features::PlanarProfileRef::sketch_profiles(sketch_id.clone(), indices)
                .unwrap(),
        ))
    };
    let mut selections = [first, second];
    assert_eq!(resolve(&selections), expected(vec![2, 0]));
    let mut path_resolution =
        planar_resolution(&[], &placements, &curves, &sketches, &entities).path_resolution();
    path_resolution.profile_curves = &selections;
    assert!(resolve_entity_selection_profile(&group, &path_resolution).is_none());
    assert!(super::super::resolve_profile_curve_path(&group, "stream", &path_resolution).is_none());
    let mut arrangement_selections = selections.clone();
    arrangement_selections[1].groups[0].region[0].entries[0].primary_id = 13;
    assert_eq!(resolve(&arrangement_selections), expected(vec![2]));
    let mut arrangement_resolution = path_resolution;
    arrangement_resolution.profile_curves = &arrangement_selections;
    let work = ProfileGeometryWork::new();
    let scope_histories = HashMap::new();
    crate::test_support::with_decode_context(|ctx| {
        let scoped = ExtrudeProfileResolution {
            entities: &entities,
            spatial_sketches: &[],
            spatial_entities: &[],
            histories: &[],
            scope_histories: &scope_histories,
            linear_tolerance: LINEAR_TOLERANCE,
            angular_tolerance: ANGULAR_TOLERANCE,
            profile_work: &work,
            ctx,
        }
        .scoped(&[]);
        assert!(super::super::resolve_profile_curve_arrangement_regions(
            &group,
            &arrangement_resolution,
            scoped,
        )
        .unwrap()
        .is_none());
    });
    assert!(work.losses().is_empty());
    let mut holed = selections.clone();
    let mut hole = selection(11, 1, 12, false)
        .groups
        .remove(0)
        .region
        .remove(0);
    hole.entries[0].primary_id_offset =
        holed[1].groups[0].region[0].entries[0].primary_id_offset + 45;
    holed[1].groups[0].region.push(hole);
    holed[1].identity_end_offset = holed[1].lists_end().unwrap();
    assert_eq!(
        resolve(&holed),
        Some(cadmpeg_ir::features::ProfileRef::Planar(
            cadmpeg_ir::features::PlanarProfileRef::sketch_regions(
                sketch_id.clone(),
                vec![
                    SketchProfileRegion::loops(2, vec![]).unwrap(),
                    SketchProfileRegion::loops(0, vec![1]).unwrap(),
                ],
            )
            .unwrap(),
        )),
    );
    let mut third = selection(11, 1, 12, true).groups.remove(0);
    third.region[0].entries[0].primary_id_offset = selections[1].identity_end_offset + 12;
    selections[1].groups.push(third);
    selections[1].identity_end_offset = selections[1].lists_end().unwrap();
    assert_eq!(resolve(&selections), expected(vec![2, 0, 1]));
    selections[1].groups[1].region[0].entries[0].entry_fields[1] = 2;
    assert!(resolve(&selections).is_none());
    selections[1].groups[1].region[0].entries[0].entry_fields[1] = 1;
    selections[1].groups[1].region[0].entries[0].primary_id = 99;
    assert!(resolve(&selections).is_none());
    selections[1].groups[1].region[0].entries[0].primary_id = 12;
    let mut descendant = selections[1].groups[0].region.clone();
    descendant[0].entries[0].primary_id_offset = selections[0].identity_end_offset + 12;
    selections[0].groups[0].further.push(descendant);
    selections[0].identity_end_offset = selections[0].lists_end().unwrap();
    assert!(selections[0].valid_layout());
    assert!(resolve(&selections).is_none());
}
