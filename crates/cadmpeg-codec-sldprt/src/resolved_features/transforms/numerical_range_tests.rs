// SPDX-License-Identifier: Apache-2.0

use super::*;
use cadmpeg_ir::scalar::{Angle, Length};
use cadmpeg_ir::sketches::{
    SketchEntity, SketchEntityId, SketchGeometry, SketchGeometryDefinition, SketchId,
};

fn entity(name: &str, definition: SketchGeometryDefinition) -> SketchEntity {
    SketchEntity::new(
        SketchEntityId::mint(format!("f3d:test:entity#{name}")).unwrap(),
        SketchId::mint("f3d:test:sketch#1").unwrap(),
        SketchGeometry::try_from(definition).unwrap(),
    )
}
#[test]
fn numerical_0922_hyperbola_loci_remain_finite() {
    let e = entity(
        "hyperbola",
        SketchGeometryDefinition::Hyperbola {
            center: Point2::new(0., 0.),
            major_angle: Angle::new(0.).unwrap(),
            major_radius: Length::new(1e-100).unwrap(),
            minor_radius: Length::new(1e-100).unwrap(),
            bounds: Some([710., 711.]),
        },
    );
    let r = sketch_entity_loci(&e);
    println!("SW tiny-radius hyperbola endpoints t710,711: {r:?}");
    assert_eq!(r.len(), 3);
    assert!(r.iter().all(|(p, _)| p.is_finite()));
    let reference = cadmpeg_ir::math::scaled_sinh_cosh(
        cadmpeg_ir::scalar::FiniteReal::new(1e-100).unwrap(),
        cadmpeg_ir::scalar::FiniteReal::new(711.).unwrap(),
    )
    .unwrap();
    assert_eq!(r[2].0, Point2::new(reference.1.get(), reference.0.get()));
    assert!(super::super::typed_relations::sketch_entity_contains_point(
        &e, r[2].0
    ));
}

#[test]
fn hyperbola_locus_uses_finite_minor_sinh_when_cosh_overflows() {
    let e = entity(
        "wide-minor-hyperbola",
        SketchGeometryDefinition::Hyperbola {
            center: Point2::new(0.0, 0.0),
            major_angle: Angle::new(0.0).unwrap(),
            major_radius: Length::new(1.0).unwrap(),
            minor_radius: Length::new(f64::MAX).unwrap(),
            bounds: Some([0.0, 1.0e-7]),
        },
    );
    let loci = sketch_entity_loci(&e);
    assert_eq!(loci.len(), 3);
    let end = loci[2].0;
    assert!((end.u - 1.0e-7_f64.cosh()).abs() < 16.0 * f64::EPSILON);
    assert!((end.v / (f64::MAX * 1.0e-7_f64.sinh()) - 1.0).abs() < 16.0 * f64::EPSILON);
}
