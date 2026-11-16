// SPDX-License-Identifier: Apache-2.0

use crate::design::profile_select::unprofiled_curve_losses;
use crate::loss::F3dLossCode;
use cadmpeg_ir::features::{
    BooleanOp, DistinctMembers, ExtrudeDirection, ExtrudeExtent, ExtrudeSide, ExtrudeStart,
    Feature, FeatureContent, FeatureDefinition, FeatureEvaluation, FeatureId, FeatureOperation,
    LinearTermination, PlanarProfileRef, ProfileRef, SketchProfileBoundaryUse, SketchProfileRegion,
    SketchProfileRegions,
};
use cadmpeg_ir::geometry::DirectedParameterRange;
use cadmpeg_ir::math::Point2;
use cadmpeg_ir::scalar::NonZeroLength;
use cadmpeg_ir::sketches::{
    SketchEntity, SketchEntityId, SketchGeometry, SketchGeometryDefinition, SketchId,
};
use std::collections::BTreeMap;

fn line(sketch: &SketchId, id: &str) -> SketchEntity {
    SketchEntity::new(
        SketchEntityId::mint(id).unwrap(),
        sketch.clone(),
        SketchGeometry::try_from(SketchGeometryDefinition::Line {
            start: Point2::new(0.0, 0.0),
            end: Point2::new(1.0, 0.0),
        })
        .unwrap(),
    )
}

fn boundary(entity: &SketchEntityId) -> SketchProfileBoundaryUse {
    SketchProfileBoundaryUse {
        entity: entity.clone(),
        parameter_range: DirectedParameterRange::new([0.0, 1.0]).unwrap(),
        reversed: false,
    }
}

fn extrude_of_regions(sketch: &SketchId, outer: Vec<SketchProfileBoundaryUse>) -> Feature {
    let regions: SketchProfileRegions = vec![SketchProfileRegion::trimmed(outer, vec![]).unwrap()]
        .try_into()
        .unwrap();
    let definition = FeatureDefinition::Operation(FeatureOperation::Extrude {
        profile: ProfileRef::Planar(PlanarProfileRef::SketchRegions {
            sketch: sketch.clone(),
            regions,
        }),
        direction: ExtrudeDirection::default(),
        start: ExtrudeStart::default(),
        extent: ExtrudeExtent::OneSided {
            side: ExtrudeSide {
                termination: LinearTermination::Blind {
                    length: NonZeroLength::new(1.0).unwrap(),
                },
                draft: None,
            },
        },
        op: BooleanOp::NewBody,
        solid: Some(true),
        face_maker: None,
        inner_wire_taper: None,
        length_along_profile_normal: None,
        allow_multi_profile_faces: None,
    });
    Feature {
        id: FeatureId::mint("test:model:feature#1").unwrap(),
        ordinal: 0,
        name: None,
        suppressed: None,
        dependencies: DistinctMembers::default(),
        source_properties: BTreeMap::default(),
        source_tag: None,
        source_text: None,
        source_content: FeatureContent::default(),
        evaluation: FeatureEvaluation::from_definition(definition),
        native_ref: None,
        product_definition: None,
    }
}

fn unprofiled(id: &SketchEntityId) -> (SketchEntityId, cadmpeg_ir::report::loss::LossNote) {
    (
        id.clone(),
        F3dLossCode::SketchCurveUnprofiled.note(format!("sketch entity {id}: open")),
    )
}

#[test]
fn open_curve_in_a_selected_region_boundary_reports_selected() {
    let sketch = SketchId::mint("f3d:model:sketch#a").unwrap();
    let inside = line(&sketch, "synthetic:test:id#split");
    let outside = line(&sketch, "synthetic:test:id#stray");
    let feature = extrude_of_regions(&sketch, vec![boundary(inside.id())]);

    let notes = unprofiled_curve_losses(
        vec![unprofiled(inside.id()), unprofiled(outside.id())],
        &[feature],
        &[inside.clone(), outside.clone()],
    );

    assert_eq!(notes.len(), 2);
    assert_eq!(
        notes[0].code,
        F3dLossCode::SketchCurveUnprofiledSelected.kind()
    );
    assert_eq!(notes[0].severity, cadmpeg_ir::report::Severity::Info);
    assert!(notes[0].message.contains("synthetic:test:id#split"));
    assert_eq!(notes[1].code, F3dLossCode::SketchCurveUnprofiled.kind());
    assert_eq!(notes[1].severity, cadmpeg_ir::report::Severity::Warning);
    assert!(notes[1].message.contains("synthetic:test:id#stray"));
}

#[test]
fn open_curve_keeps_its_loss_without_a_selecting_feature_or_in_another_sketch() {
    let sketch = SketchId::mint("f3d:model:sketch#a").unwrap();
    let other = SketchId::mint("f3d:model:sketch#b").unwrap();
    let curve = line(&sketch, "synthetic:test:id#open");

    // No feature at all.
    let notes = unprofiled_curve_losses(
        vec![unprofiled(curve.id())],
        &[],
        std::slice::from_ref(&curve),
    );
    assert_eq!(notes[0].code, F3dLossCode::SketchCurveUnprofiled.kind());

    // The selected region belongs to a different sketch.
    let feature = extrude_of_regions(&other, vec![boundary(curve.id())]);
    let notes = unprofiled_curve_losses(vec![unprofiled(curve.id())], &[feature], &[curve]);
    assert_eq!(notes[0].code, F3dLossCode::SketchCurveUnprofiled.kind());
}

fn empty_sketch(id: SketchId) -> cadmpeg_ir::sketches::Sketch {
    cadmpeg_ir::sketches::Sketch {
        id,
        name: None,
        configuration: None,
        visible: None,
        placement: cadmpeg_ir::sketches::SketchPlacement::try_resolved(
            cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
            cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0),
            cadmpeg_ir::math::Vector3::new(1.0, 0.0, 0.0),
        )
        .unwrap(),
        profiles: cadmpeg_ir::sketches::SketchProfiles::default(),
        native_ref: None,
    }
}

#[test]
fn interior_intersections_report_region_boundaries_and_keep_open_curve_warnings() {
    let sketch = empty_sketch(SketchId::mint("synthetic:test:id#interior-intersections").unwrap());
    let segment = |id, start: (f64, f64), end: (f64, f64)| {
        SketchEntity::new(
            SketchEntityId::mint(id).unwrap(),
            sketch.id.clone(),
            SketchGeometry::try_from(SketchGeometryDefinition::Line {
                start: Point2::new(start.0, start.1),
                end: Point2::new(end.0, end.1),
            })
            .unwrap(),
        )
    };
    let entities = vec![
        segment("synthetic:test:id#left", (0.0, -1.0), (0.0, 2.0)),
        segment("synthetic:test:id#right", (1.0, -1.0), (1.0, 2.0)),
        segment("synthetic:test:id#bottom", (0.0, 0.0), (1.0, 0.0)),
        segment("synthetic:test:id#top", (0.0, 1.0), (1.0, 1.0)),
        segment("synthetic:test:id#open", (2.0, 0.0), (3.0, 0.0)),
    ];
    let notes = crate::test_support::with_decode_context(|ctx| {
        super::super::unprofiled_region_losses(
            ctx,
            entities
                .iter()
                .map(|entity| unprofiled(entity.id()))
                .collect(),
            std::slice::from_ref(&sketch),
            &entities,
            1.0e-6,
        )
    })
    .unwrap();
    assert_eq!(notes.len(), 5);
    for (id, note) in &notes[..4] {
        assert_eq!(note.code, F3dLossCode::SketchCurveRegionBoundary.kind());
        assert_eq!(note.severity, cadmpeg_ir::report::Severity::Info);
        assert!(note.message.contains(id.as_str()));
    }
    assert_eq!(notes[4].1.code, F3dLossCode::SketchCurveUnprofiled.kind());
    assert_eq!(notes[4].1.severity, cadmpeg_ir::report::Severity::Warning);
}

#[test]
fn construction_curve_keeps_an_open_chain_out_of_the_region_diagnostic() {
    let sketch = empty_sketch(SketchId::mint("synthetic:test:id#construction-boundary").unwrap());
    let entities = [
        ((0.0, 0.0), (1.0, 0.0)),
        ((1.0, 0.0), (1.0, 1.0)),
        ((1.0, 1.0), (0.0, 1.0)),
        ((0.0, 1.0), (0.0, 0.0)),
    ]
    .into_iter()
    .enumerate()
    .map(|(index, (start, end))| {
        SketchEntity::new(
            SketchEntityId::mint(format!("synthetic:test:id#edge-{index}")).unwrap(),
            sketch.id.clone(),
            SketchGeometry::try_from(SketchGeometryDefinition::Line {
                start: Point2::new(start.0, start.1),
                end: Point2::new(end.0, end.1),
            })
            .unwrap(),
        )
        .with_construction(index == 3)
    })
    .collect::<Vec<_>>();
    let notes = crate::test_support::with_decode_context(|ctx| {
        super::super::unprofiled_region_losses(
            ctx,
            vec![unprofiled(entities[0].id())],
            &[sketch],
            &entities,
            1.0e-6,
        )
    })
    .unwrap();
    assert_eq!(notes[0].1.code, F3dLossCode::SketchCurveUnprofiled.kind());
}
