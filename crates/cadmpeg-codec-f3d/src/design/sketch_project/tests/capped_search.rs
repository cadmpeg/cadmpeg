// SPDX-License-Identifier: Apache-2.0

use super::*;

/// A spatial sketch whose closed-profile search exceeds its local cap keeps
/// its entities, has no profiles, and reports the capped search as a loss.
#[test]
fn capped_spatial_profile_search_reports_a_loss() {
    let placement = DesignSketchPlacement {
        frame: crate::records::sketch_placement::DesignSketchFrame::new(
            100,
            crate::records::sketch_placement::DesignSketchFrameForm::ScopeExplicit(
                crate::records::sketch_placement::SketchPlacementMatrix::try_from([
                    [1.0, 0.0, 0.0, 0.0],
                    [0.0, 1.0, 0.0, 0.0],
                    [0.0, 0.0, 1.0, 0.0],
                    [0.0, 0.0, 0.0, 1.0],
                ])
                .unwrap(),
            ),
        )
        .unwrap(),
        id: "f3d:Design/BulkStream.dat:placement#100".into(),
        scope_record_index: None,
        entity_id: crate::records::identity::DesignEntityId::try_from("Sketch_42".to_owned())
            .expect("valid entity ID"),
        visibility: None,
        display_name: None,
        class_tag: crate::records::references::DesignClassTag::try_from("300".to_owned()).unwrap(),
        record_index: 100,
        paired_class_tag: crate::records::references::DesignClassTag::try_from("259".to_owned())
            .unwrap(),
    };
    // Disjoint open segments: each search start scans every unused segment
    // twice and every segment once, about 2 * 3200^2 work units in total.
    let curves = (0..3200_u32)
        .map(|index| {
            let x = f64::from(index) * 10.0;
            let record_index = 1000 + index;
            SketchCurveIdentity {
                planar_z_noise: None,
                id: format!("f3d:Design/BulkStream.dat:curve#{record_index}"),
                record_index,
                owner_reference: Some(42),
                geometry_group: None,
                class_tag: crate::records::references::DesignClassTag::try_from("308".to_owned())
                    .unwrap(),
                byte_offset: u64::from(record_index),
                geometry_offset: 133,
                entity_genesis: None,
                primary_id: std::num::NonZeroU64::new(u64::from(record_index)).unwrap(),
                secondary_id: 0,
                geometry: Some(
                    SketchCurveGeometry::line(
                        Point3::new(x, 0.0, 0.0),
                        Point3::new(x, 0.0, 1.0),
                        Vector3::new(0.0, 0.0, 1.0),
                        Vector3::new(1.0, 0.0, 0.0),
                    )
                    .unwrap(),
                ),
            }
        })
        .collect::<Vec<_>>();
    let SpatialSketchDesign {
        sketches,
        entities,
        losses,
    } = crate::test_support::with_decode_context(|decode_ctx| {
        project_spatial_sketch_design(
            decode_ctx,
            &[placement],
            &[],
            &curves,
            &[],
            &[],
            EPS_POINT_PROJECTION,
        )
    })
    .expect("spatial sketch projects");
    assert_eq!(sketches.len(), 1);
    assert_eq!(entities.len(), curves.len());
    assert!(sketches[0].profiles.is_empty());
    let [loss] = losses.as_slice() else {
        panic!("expected one loss, got {losses:?}");
    };
    assert_eq!(
        loss.code,
        crate::loss::F3dLossCode::SketchProfileSearchCapped.kind()
    );
    assert_eq!(
        loss.message,
        "1 spatial sketch(es) have no closed profiles because their profile search exceeded the local work cap SPATIAL_PROFILE_SEARCH_WORK (16777216)."
    );
}
