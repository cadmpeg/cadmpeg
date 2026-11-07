// SPDX-License-Identifier: Apache-2.0

use super::*;
use crate::loss::F3dLossCode;
use crate::records::topology::entity_selection::{
    DesignProfileCurveEntry, DesignProfileCurveLoop, DesignProfileCurveSelection,
};

/// Square 1 with both diagonals: primary ids 1..=4 are the sides and 5, 6 the
/// diagonals.
fn x_braced_square() -> (Sketch, Vec<SketchEntity>, Vec<SketchCurveIdentity>) {
    let sketch_id = neutral_sketch_id(&placement());
    let ends = [
        ((0.0, 0.0), (10.0, 0.0)),
        ((10.0, 0.0), (10.0, 10.0)),
        ((10.0, 10.0), (0.0, 10.0)),
        ((0.0, 10.0), (0.0, 0.0)),
        ((0.0, 0.0), (10.0, 10.0)),
        ((10.0, 0.0), (0.0, 10.0)),
    ];
    let entities = ends
        .iter()
        .zip(1u64..)
        .map(|((start, end), primary)| {
            SketchEntity::new(
                neutral_sketch_curve_id(&sketch_id, primary, 0),
                sketch_id.clone(),
                SketchGeometry::try_from(SketchGeometryDefinition::Line {
                    start: Point2::new(start.0, start.1),
                    end: Point2::new(end.0, end.1),
                })
                .unwrap(),
            )
        })
        .collect();
    let identities = (1u32..=6)
        .map(|primary| curve(30 + primary, u64::from(primary), 0))
        .collect();
    let sketch = Sketch {
        id: sketch_id,
        name: None,
        configuration: None,
        visible: None,
        placement: SketchPlacement::try_resolved(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
        )
        .unwrap(),
        profiles: cadmpeg_ir::sketches::SketchProfiles::default(),
        native_ref: None,
    };
    (sketch, entities, identities)
}

fn loop_selection(
    record_index: u32,
    ordinal: u32,
    primaries: &[u64],
) -> DesignProfileCurveSelection {
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
        identity_record_offset: 500,
        identity_end_offset: 0,
        groups: vec![
            crate::records::topology::entity_selection::DesignProfileRegionGroup {
                region: vec![DesignProfileCurveLoop {
                    outer: true,
                    entries: primaries
                        .iter()
                        .zip(0u64..)
                        .map(|(primary, index)| DesignProfileCurveEntry {
                            primary_id: *primary,
                            primary_id_offset: 588 + 40 * index,
                            secondary_id: 0,
                            entry_fields: [1, 1, 1],
                        })
                        .collect(),
                }],
                further: vec![],
            },
        ],
    };
    selection.identity_end_offset = selection.lists_end().unwrap();
    selection
}

fn resolve_with_work(
    primaries: &[u64],
    work: &ProfileGeometryWork,
) -> Option<cadmpeg_ir::features::ProfileRef> {
    let (sketch, entities, identities) = x_braced_square();
    let group = group();
    let placements = [placement()];
    let sketches = [sketch];
    let selections = [
        loop_selection(10, 0, primaries),
        loop_selection(11, 1, primaries),
    ];
    let mut resolution =
        planar_resolution(&[], &placements, &identities, &sketches, &entities).path_resolution();
    resolution.profile_curves = &selections;
    let scope_histories = HashMap::new();
    crate::test_support::with_decode_context(|ctx| {
        let scoped = ExtrudeProfileResolution {
            entities: &[],
            spatial_sketches: &[],
            spatial_entities: &[],
            histories: &[],
            scope_histories: &scope_histories,
            linear_tolerance: 1.0e-7,
            angular_tolerance: 1.0e-9,
            profile_work: work,
            ctx,
        }
        .scoped(&[]);
        super::super::resolve_profile_curve_arrangement_regions(&group, &resolution, scoped)
    })
    .unwrap()
}

#[test]
fn a_split_self_crossing_loop_reports_the_assumed_interpretation() {
    let work = ProfileGeometryWork::new();
    // Four triangles: each side once, each diagonal piece once per triangle.
    let profile =
        resolve_with_work(&[1, 6, 5, 2, 5, 6, 3, 6, 5, 4, 5, 6], &work).expect("four triangles");
    let cadmpeg_ir::features::ProfileRef::Planar(
        cadmpeg_ir::features::PlanarProfileRef::SketchRegions { regions, .. },
    ) = profile
    else {
        panic!("sketch regions");
    };
    assert_eq!(regions.iter().count(), 4);
    work.finish_selection();
    let losses = work.losses();
    let [loss] = losses.as_slice() else {
        panic!("expected one loss, got {losses:?}");
    };
    assert_eq!(
        loss.code,
        F3dLossCode::FeatureProfileCrossingLoopAssumed.kind()
    );
    assert!(loss.message.starts_with("1 extrude profile selection(s)"));
    assert!(loss.message.contains("curve-use counts"));
    assert!(loss
        .message
        .contains("enclose an island were assumed not to be selected"));
}

#[test]
fn an_unresolved_self_crossing_loop_reports_no_assumption() {
    let work = ProfileGeometryWork::new();
    // No set of triangles uses these counts.
    assert_eq!(resolve_with_work(&[1, 6, 5, 5, 6], &work), None);
    work.finish_selection();
    assert!(work.losses().is_empty());
}
