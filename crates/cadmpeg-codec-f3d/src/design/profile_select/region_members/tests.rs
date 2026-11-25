// SPDX-License-Identifier: Apache-2.0

use super::*;
use cadmpeg_ir::math::Point2;
use cadmpeg_ir::sketches::{SketchGeometry, SketchGeometryDefinition, SketchId, SketchPlacement};

#[test]
fn an_unlisted_corner_arc_closes_the_selected_region() {
    let id = SketchId::mint("synthetic:test:sketch#rounded").unwrap();
    let ends = [
        ((0., 0.), (2., 0.)),
        ((2., 0.), (2., 1.)),
        ((1., 2.), (0., 2.)),
        ((0., 2.), (0., 0.)),
    ];
    let mut entities = ends
        .iter()
        .zip(1u64..)
        .map(|((a, b), primary)| {
            SketchEntity::new(
                crate::ids::neutral_sketch_curve_id(&id, primary, 0),
                id.clone(),
                SketchGeometry::try_from(SketchGeometryDefinition::Line {
                    start: Point2::new(a.0, a.1),
                    end: Point2::new(b.0, b.1),
                })
                .unwrap(),
            )
        })
        .collect::<Vec<_>>();
    let arc = crate::ids::neutral_sketch_curve_id(&id, 5, 0);
    entities.push(SketchEntity::new(
        arc.clone(),
        id.clone(),
        SketchGeometry::try_from(SketchGeometryDefinition::Arc {
            center: Point2::new(1., 1.),
            radius: cadmpeg_ir::scalar::Length::new(1.).unwrap(),
            start_angle: cadmpeg_ir::scalar::Angle::new(0.).unwrap(),
            end_angle: cadmpeg_ir::scalar::Angle::new(std::f64::consts::FRAC_PI_2).unwrap(),
        })
        .unwrap(),
    ));
    let sketch = Sketch {
        id,
        name: None,
        configuration: None,
        visible: None,
        placement: SketchPlacement::Unresolved {},
        profiles: cadmpeg_ir::sketches::SketchProfiles::default(),
        native_ref: None,
    };
    let curves = (1u32..=5)
        .map(|primary| SketchCurveIdentity {
            id: format!("stream:curve-{primary}"),
            record_index: primary,
            owner_reference: Some(42),
            geometry_group: None,
            class_tag: "450".to_owned().try_into().unwrap(),
            byte_offset: 0,
            geometry_offset: 0,
            entity_genesis: None,
            primary_id: u64::from(primary).try_into().unwrap(),
            secondary_id: 0,
            geometry: None,
            planar_z_noise: None,
            tail_marker: None,
        })
        .collect::<Vec<_>>();
    let members = (1..=4).map(|primary| serde_json::json!({"kind_offset":0,"curve_primary_id":primary,"curve_primary_id_offset":0,"incidence_words_offset":0,"incidence_words":[0,0,0,0,1,1,0,0],"kind":3})).collect::<Vec<_>>();
    let profile: DesignSketchProfileOperand = serde_json::from_value(serde_json::json!({
        "scope_reference_ordinal":0,"record_index":1,"byte_offset":0,"class_tag":"411","asset_id":"00000000-0000-0000-0000-000000000001","asset_id_offset":1,"entity_id":"1_42","entity_suffix":42,"entity_reference_offset":2,"paired_class_tag":"262","paired_byte_offset":3,
        "region_selection":{"record_index":4,"byte_offset":0,"class_tag":"382","region_count_offset":0,"regions":[{"member_count_offset":0,"members":members}],"companion_class_tag":"291","companion_byte_offset":0}
    })).unwrap();
    crate::test_support::with_decode_context(|ctx| {
        let work = crate::design::geometry::ProfileGeometryWork::new();
        let histories = std::collections::HashMap::new();
        let resolution = crate::design::profile_select::ExtrudeProfileResolution {
            entities: &entities,
            spatial_sketches: &[],
            spatial_entities: &[],
            histories: &[],
            scope_histories: &histories,
            linear_tolerance: 1.0e-7,
            angular_tolerance: 1.0e-9,
            profile_work: &work,
            ctx,
        }
        .scoped(&[]);
        assert!(
            resolve("stream", &profile, &sketch, &curves, &entities, resolution, true)?.is_none()
        );
        let selected =
            resolve_arc_bridges("stream", &profile, &sketch, &curves, &entities, resolution)?
                .unwrap();
        let PlanarProfileRef::SketchRegions { regions, .. } = selected else {
            panic!("explicit region")
        };
        let [cadmpeg_ir::features::profiles::SketchProfileRegion::Trimmed {
            outer_boundary,
            hole_boundaries,
        }] = regions.as_slice()
        else {
            panic!("one trimmed region")
        };
        assert!(hole_boundaries.is_empty());
        assert!(outer_boundary.iter().any(|use_| use_.entity == arc));
        let mut ambiguous = entities.clone();
        ambiguous.push(SketchEntity::new(
            crate::ids::neutral_sketch_curve_id(&sketch.id, 6, 0),
            sketch.id.clone(),
            entities.last().unwrap().geometry.clone(),
        ));
        assert!(
            resolve_arc_bridges("stream", &profile, &sketch, &curves, &ambiguous, resolution,)?
                .is_none()
        );
        Ok::<_, CodecError>(())
    })
    .unwrap();
}
