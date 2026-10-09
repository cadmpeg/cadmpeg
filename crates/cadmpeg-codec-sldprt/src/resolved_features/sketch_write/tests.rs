// SPDX-License-Identifier: Apache-2.0
//! Generated profile geometry, reversal and preflight error order.

use cadmpeg_ir::geometry::SolvedCurveGeometry;
use cadmpeg_ir::math::{Point2, Point3, Vector3};
use cadmpeg_ir::sketches::{
    Sketch, SketchEntity, SketchEntityId, SketchEntityUse, SketchGeometry,
    SketchGeometryDefinition, SketchId, SketchPlacement, SketchProfiles,
};
use cadmpeg_ir::topology::Sense;

fn fixture(geometries: Vec<SketchGeometry>, reversed: bool) -> (cadmpeg_ir::CadIr, Sketch) {
    let id = SketchId::mint("synthetic:test:sketch#profile").unwrap();
    let mut source = cadmpeg_ir::CadIr::empty();
    let mut profile = Vec::new();
    for (index, geometry) in geometries.into_iter().enumerate() {
        let entity = SketchEntityId::mint(format!("synthetic:test:sketch-entity#profile-{index}"))
            .unwrap();
        source.model.sketch_entities.push(SketchEntity::new(entity.clone(), id.clone(), geometry));
        profile.push(SketchEntityUse { entity, reversed });
    }
    let sketch = Sketch {
        id, name: None, configuration: None, visible: None, native_ref: None,
        placement: SketchPlacement::try_resolved(
            Point3::new(0.0, 0.0, 0.0), Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
        ).unwrap(),
        profiles: SketchProfiles::try_from(vec![profile]).unwrap(),
    };
    (source, sketch)
}

#[test]
fn generated_nurbs_profiles_keep_poles_weights_and_reversed_uses() {
    for rational in [false, true] {
        let curve = cadmpeg_ir::geometry::pcurve::PcurveNurbs::from_lanes(
            &cadmpeg_test_support::service_decode_context(), 2,
            vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
            vec![Point2::new(0.0, 0.0), Point2::new(1.0, 1.0), Point2::new(2.0, 0.0)],
            rational.then(|| vec![1.0, 0.5, 1.0]), false,
        ).unwrap().unwrap();
        let (source, sketch) = fixture(vec![
            SketchGeometry::try_from(SketchGeometryDefinition::Nurbs { curve }).unwrap(),
            SketchGeometry::try_from(SketchGeometryDefinition::Line {
                start: Point2::new(2.0, 0.0), end: Point2::new(0.0, 0.0),
            }).unwrap(),
        ], true);
        let generated = super::sketch_brep(&source, &sketch).unwrap();
        assert_eq!(generated.model.curves.len(), 2);
        assert_eq!(generated.model.coedges.len(), 2);
        assert!(generated.model.coedges.iter().all(|coedge| coedge.sense == Sense::Reversed));
        let curve = generated.model.curves.iter().find_map(|curve| match curve.geometry.solved() {
            Some(SolvedCurveGeometry::Nurbs(curve)) => Some(curve),
            _ => None,
        }).unwrap();
        assert_eq!(curve.pole_count(), 3);
        assert_eq!(curve.knots().as_slice(), [0.0, 0.0, 0.0, 1.0, 1.0, 1.0]);
        assert_eq!(curve.pole_rows().point_at(1).unwrap().get(), Point3::new(1.0, 1.0, 0.0));
        assert_eq!(curve.pole_rows().weight_at(1), rational.then_some(0.5));
        assert_eq!(source.model.sketch_entities.len(), 2);
    }
}

#[test]
fn unsupported_profile_entity_refuses_before_closed_chain_validation() {
    let (source, sketch) = fixture(vec![
        SketchGeometry::try_from(SketchGeometryDefinition::Line {
            start: Point2::new(0.0, 0.0), end: Point2::new(1.0, 0.0),
        }).unwrap(),
        SketchGeometry::try_from(SketchGeometryDefinition::Point {
            position: Point2::new(2.0, 0.0),
        }).unwrap(),
    ], false);
    let Err(cadmpeg_core::CodecError::NotImplemented(message)) = super::sketch_brep(&source, &sketch)
    else { panic!("unsupported profile entity"); };
    assert_eq!(message, "source-less SLDPRT sketch writing does not support point or native-only profile entities");
    assert_eq!(source.model.sketch_entities.len(), 2);
}
