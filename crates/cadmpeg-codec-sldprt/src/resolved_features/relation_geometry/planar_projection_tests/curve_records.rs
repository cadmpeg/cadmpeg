use super::{planar_fixture, project_relation_bindings};
use crate::records::{
    SketchInputEntity, SketchInputKind, SketchInputLink, SketchInputLinks, SketchRelationKind,
};
use crate::test_support::sketch_records::framed_line_sketch;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_ir::math::Point2;
use cadmpeg_ir::sketches::{
    SketchEntity, SketchEntityId, SketchGeometry, SketchGeometryDefinition,
};

/// Whether a horizontal relation marker at `offset`, linked to a horizontal
/// marker over two point markers, projects a constraint from a lane whose
/// payload holds one framed `sgSketch` object with a line record.
fn relation_at_projects(offset: impl FnOnce(&[u64]) -> u64) -> bool {
    let (sketch, feature, mut lane) = planar_fixture();
    let (payload, _, lines) = framed_line_sketch(&[[0.0, 0.0], [0.002, 0.0]], &[(0, 1, 4)]);
    lane.native_payload = payload.into();
    lane.relation_instances.clear();
    lane.sketch_entities.clear();
    let mut root = SketchInputEntity::new(
        "axis-root",
        "lane",
        0,
        offset(&lines),
        SketchInputKind::Relation(SketchRelationKind::Horizontal),
    );
    root.feature_ref = Some("feature".into());
    root.links = SketchInputLinks::new(
        0,
        vec![SketchInputLink {
            local_id: 1,
            entity_ref: "axis-child".into(),
        }],
    );
    let mut child = SketchInputEntity::new(
        "axis-child",
        "lane",
        1,
        1,
        SketchInputKind::Relation(SketchRelationKind::Horizontal),
    );
    child.feature_ref = Some("feature".into());
    child.links = SketchInputLinks::new(
        0,
        (0u16..2)
            .map(|index| SketchInputLink {
                local_id: index + 2,
                entity_ref: format!("axis-point-{index}"),
            })
            .collect(),
    );
    lane.sketch_entities.extend([root, child]);
    let entities: Vec<_> = (0u32..2)
        .map(|index| {
            let marker_id = format!("axis-point-{index}");
            let mut marker = SketchInputEntity::new(
                marker_id.clone(),
                "lane",
                index + 2,
                u64::from(index) + 2,
                SketchInputKind::Point,
            );
            marker.feature_ref = Some("feature".into());
            marker.coordinates_m =
                cadmpeg_ir::units::FiniteVector::new([f64::from(index) * 0.002, 0.0]);
            lane.sketch_entities.push(marker);
            SketchEntity::new(
                SketchEntityId::mint(format!("synthetic:test:id#axis-point-{index}")).unwrap(),
                sketch.id.clone(),
                SketchGeometry::try_from(SketchGeometryDefinition::Point {
                    position: Point2::new(f64::from(index) * 2.0, 0.0),
                })
                .unwrap(),
            )
            .with_native_ref(Some(marker_id))
        })
        .collect();
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
        .expect("fixture fits service policy");
    let mut constraints = Vec::new();
    project_relation_bindings(
        &ctx,
        &mut constraints,
        &[sketch],
        &[feature],
        &entities,
        &[],
        &[lane],
    )
    .expect("fixture fits service policy");
    constraints
        .iter()
        .any(|constraint| constraint.native_ref.as_deref() == Some("axis-root"))
}

/// A marker whose record is a curve record of an `sgSketch` object is a
/// curve, whatever relation code its u32 at +17 holds, and projects no
/// constraint; the same marker off the record projects one.
#[test]
fn a_marker_on_a_curve_record_projects_no_constraint() {
    assert!(relation_at_projects(|_| 0));
    assert!(!relation_at_projects(|lines| lines[0]));
}
