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
        display_name: None,
    }
}

fn names(
    scopes: &[DesignParameterScope],
    placements: &[DesignSketchPlacement],
    name: Option<&str>,
) -> ((Option<String>, Option<String>), Vec<LossNote>) {
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
    let losses = bind_sketch_names(&features, scopes, placements, &mut sketches, &mut spatial);
    ((sketches[0].name.clone(), spatial[0].name.clone()), losses)
}

#[test]
fn exact_scope_copies_composed_feature_name_and_charges_one_loss() {
    let (result, losses) = names(&[scope(10)], &[placement(20, Some(10))], Some("Sketch 1"));
    assert_eq!(losses.len(), 1);
    assert_eq!(
        losses[0].code,
        F3dLossCode::SketchNameComposedFromFeatureOrdinal.kind()
    );
    assert_eq!(result, (Some("Sketch 1".into()), Some("Sketch 1".into())));
}

#[test]
fn absent_scope_keeps_entity_names() {
    let (result, losses) = names(&[], &[placement(20, None)], Some("Sketch 1"));
    assert!(losses.is_empty());
    assert_eq!(result, (Some("Sketch_42".into()), Some("Sketch_42".into())));
}

#[test]
fn two_scopes_selecting_one_sketch_keep_entity_names() {
    let (result, losses) = names(
        &[scope(10), scope(11)],
        &[placement(20, Some(10)), placement(21, Some(11))],
        Some("Sketch 1"),
    );
    assert!(losses.is_empty());
    assert_eq!(result, (Some("Sketch_42".into()), Some("Sketch_42".into())));
}

#[test]
fn two_placements_carrying_one_scope_keep_entity_names() {
    let (result, losses) = names(
        &[scope(10)],
        &[placement(20, Some(10)), placement(21, Some(10))],
        Some("Sketch 1"),
    );
    assert!(losses.is_empty());
    assert_eq!(result, (Some("Sketch_42".into()), Some("Sketch_42".into())));
}

#[test]
fn absent_or_empty_display_name_keeps_entity_names() {
    for name in [None, Some("")] {
        let (result, losses) = names(&[scope(10)], &[placement(20, Some(10))], name);
        assert!(losses.is_empty());
        assert_eq!(result, (Some("Sketch_42".into()), Some("Sketch_42".into())));
    }
}

#[test]
fn scope_stream_is_part_of_the_placement_join() {
    let mut other = scope(10);
    other.id = "f3d:Other/BulkStream.dat:parameter-scope#10".into();
    let (result, losses) = names(&[other], &[placement(20, Some(10))], Some("Sketch 1"));
    assert!(losses.is_empty());
    assert_eq!(result, (Some("Sketch_42".into()), Some("Sketch_42".into())));
}

#[test]
fn a_named_feature_without_projected_sketches_charges_no_name_loss() {
    let scopes = [scope(10)];
    let placements = [placement(20, Some(10))];
    let timelines = crate::design::test_support::synthetic_feature_timelines(&scopes);
    let features = crate::test_support::with_decode_context(|ctx| {
        project_parameter_design_with_edge_identities(
            ctx,
            &ProjectInputs {
                scopes: &scopes,
                placements: &placements,
                timelines: &timelines,
                ..Default::default()
            },
        )
        .unwrap()
        .features
    });
    assert_eq!(features.len(), 1);
    assert!(bind_sketch_names(&features, &scopes, &placements, &mut [], &mut []).is_empty());
}

#[test]
fn stored_placement_display_name_or_scope_label_skips_composed_name_loss() {
    // 1. Placement with display_name
    let mut named_placement = placement(20, Some(10));
    named_placement.display_name = Some("CustomSketch".into());
    let (result, losses) = names(&[scope(10)], &[named_placement], Some("CustomSketch"));
    assert!(losses.is_empty());
    assert_eq!(
        result,
        (Some("CustomSketch".into()), Some("Sketch_42".into()))
    );

    // 2. Scope with label matching feature name
    let mut labeled_scope = scope(10);
    labeled_scope.label = Some(crate::records::identity::Located {
        value: "RenamedFeature".to_owned(),
        offset: Default::default(),
    });
    let (result2, losses2) = names(
        &[labeled_scope],
        &[placement(20, Some(10))],
        Some("RenamedFeature"),
    );
    assert!(losses2.is_empty());
    assert_eq!(
        result2,
        (Some("Sketch_42".into()), Some("Sketch_42".into()))
    );
}
