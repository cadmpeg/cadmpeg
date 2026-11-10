// SPDX-License-Identifier: Apache-2.0

use super::*;
use crate::design::feature_project::{
    project_parameter_design_with_edge_identities, ProjectInputs,
};
use crate::design::sketch_project::{project_sketch_design, SketchProjectInputs};
use crate::records::sketch_placement::{DesignSketchFrame, DesignSketchFrameForm};

const EPS_LINEAR: f64 = 1.0e-6;

fn scope(record: u32) -> DesignParameterScope {
    DesignParameterScope::empty(
        &format!("f3d:Design/BulkStream.dat:parameter-scope#{record}"),
        DesignFeatureKind::Sketch,
        record,
    )
}

fn placement(record: u32, scope_record_index: Option<u32>) -> DesignSketchPlacement {
    DesignSketchPlacement {
        frame: DesignSketchFrame::new(0, DesignSketchFrameForm::ScopeCompact).unwrap(),
        id: format!("f3d:Design/BulkStream.dat:placement#{record}"),
        scope_record_index,
        entity_id: "Sketch_42".to_owned().try_into().unwrap(),
        visibility: None,
        class_tag: "256".to_owned().try_into().unwrap(),
        record_index: record,
        paired_class_tag: "257".to_owned().try_into().unwrap(),
    }
}

fn names(
    scopes: &[DesignParameterScope],
    placements: &[DesignSketchPlacement],
    name: Option<&str>,
) -> (Option<String>, Option<String>) {
    let timelines = crate::design::test_support::synthetic_feature_timelines(scopes);
    let mut features = crate::test_support::with_decode_context(|ctx| {
        project_parameter_design_with_edge_identities(
            ctx,
            &ProjectInputs {
                scopes,
                placements,
                timelines: &timelines,
                ..Default::default()
            },
        )
        .unwrap()
        .features
    });
    for feature in &mut features {
        feature.name = name.map(str::to_owned);
    }
    let mut sketches = crate::test_support::with_decode_context(|ctx| {
        project_sketch_design(
            ctx,
            &SketchProjectInputs::new(&placements[..1], &[], &[], &[], &[]),
            EPS_LINEAR,
        )
        .unwrap()
        .sketches
    });
    let mut spatial = [SpatialSketch {
        id: neutral_spatial_sketch_id(&placements[0]),
        name: Some("Sketch_42".into()),
        configuration: None,
        visible: None,
        profiles: Vec::new(),
        native_ref: Some(placements[0].id.clone()),
    }];
    bind_sketch_names(&features, scopes, placements, &mut sketches, &mut spatial);
    (sketches[0].name.clone(), spatial[0].name.clone())
}

#[test]
fn exact_scope_copies_the_feature_display_name_to_planar_and_spatial_sketches() {
    let result = names(
        &[scope(10)],
        &[placement(20, Some(10))],
        Some("Browser label"),
    );
    assert_eq!(
        result,
        (Some("Browser label".into()), Some("Browser label".into()))
    );
}

#[test]
fn absent_scope_keeps_entity_names() {
    let result = names(&[], &[placement(20, None)], Some("Browser label"));
    assert_eq!(result, (Some("Sketch_42".into()), Some("Sketch_42".into())));
}

#[test]
fn two_scopes_selecting_one_sketch_keep_entity_names() {
    let result = names(
        &[scope(10), scope(11)],
        &[placement(20, Some(10)), placement(21, Some(11))],
        Some("Browser label"),
    );
    assert_eq!(result, (Some("Sketch_42".into()), Some("Sketch_42".into())));
}

#[test]
fn two_placements_carrying_one_scope_keep_entity_names() {
    let result = names(
        &[scope(10)],
        &[placement(20, Some(10)), placement(21, Some(10))],
        Some("Browser label"),
    );
    assert_eq!(result, (Some("Sketch_42".into()), Some("Sketch_42".into())));
}

#[test]
fn absent_or_empty_display_name_keeps_entity_names() {
    for name in [None, Some("")] {
        let result = names(&[scope(10)], &[placement(20, Some(10))], name);
        assert_eq!(result, (Some("Sketch_42".into()), Some("Sketch_42".into())));
    }
}

#[test]
fn scope_stream_is_part_of_the_placement_join() {
    let mut other = scope(10);
    other.id = "f3d:Other/BulkStream.dat:parameter-scope#10".into();
    let result = names(&[other], &[placement(20, Some(10))], Some("Browser label"));
    assert_eq!(result, (Some("Sketch_42".into()), Some("Sketch_42".into())));
}
