// SPDX-License-Identifier: Apache-2.0
use cadmpeg_ir::geometry::pcurve::{
    CirclePcurve, EllipsePcurve, HyperbolaPcurve, ParabolaPcurve, PcurveGeometry, PlacedPcurve,
};
use cadmpeg_ir::math::Point2;
use cadmpeg_ir::transform::{Transform, Transform2};

use crate::writer::Emitter;

fn operator_attributes<'a>(lines: &'a [String], entity: &str, arity: usize) -> Vec<&'a str> {
    let marker = format!(" = {entity}(");
    let attributes = lines
        .iter()
        .find_map(|line| line.split_once(&marker))
        .and_then(|(_, attributes)| attributes.strip_suffix(");"))
        .expect("transformation operator simple instance")
        .split(',')
        .collect::<Vec<_>>();
    assert_eq!(attributes.len(), arity);
    assert_eq!(&attributes[..3], &["''", "''", "''"]);
    attributes
}

fn referenced_record<'a>(lines: &'a [String], reference: &str) -> &'a str {
    let prefix = format!("{reference} = ");
    lines
        .iter()
        .find_map(|line| line.strip_prefix(&prefix))
        .expect("referenced entity")
}

#[test]
fn three_dimensional_operator_has_all_inherited_attributes_in_schema_order() {
    let transform = Transform::affine([
        [0.0, -2.0, 0.0, 10.0],
        [2.0, 0.0, 0.0, 20.0],
        [0.0, 0.0, -2.0, 30.0],
    ])
    .expect("reflected similarity");
    let mut emitter = Emitter::new();
    super::transformation_operator(&mut emitter, transform);
    let lines = emitter.into_lines().expect("finite operator");
    let attributes = operator_attributes(&lines, "CARTESIAN_TRANSFORMATION_OPERATOR_3D", 8);
    assert_eq!(
        referenced_record(&lines, attributes[3]),
        "DIRECTION('',(0.,1.,0.));"
    );
    assert_eq!(
        referenced_record(&lines, attributes[4]),
        "DIRECTION('',(-1.,0.,0.));"
    );
    assert_eq!(
        referenced_record(&lines, attributes[5]),
        "CARTESIAN_POINT('',(10.,20.,30.));"
    );
    assert_eq!(attributes[6], "2.");
    assert_eq!(
        referenced_record(&lines, attributes[7]),
        "DIRECTION('',(0.,0.,-1.));"
    );
}

#[test]
fn reflected_conic_pcurves_keep_schema_complete_replica_operators() {
    let center = Point2::new(1.0, 2.0);
    let x = Point2::new(1.0, 0.0);
    let y = Point2::new(0.0, 1.0);
    let conics = [
        PcurveGeometry::Circle(CirclePcurve::try_new(center, x, y, 3.0).expect("circle")),
        PcurveGeometry::Ellipse(EllipsePcurve::try_new(center, x, y, 3.0, 2.0).expect("ellipse")),
        PcurveGeometry::Parabola(ParabolaPcurve::try_new(center, x, y, 2.0).expect("parabola")),
        PcurveGeometry::Hyperbola(
            HyperbolaPcurve::try_new(center, x, y, 3.0, 2.0).expect("hyperbola"),
        ),
    ];
    let transform =
        Transform2::affine([[0.0, 2.0, 10.0], [2.0, 0.0, 20.0]]).expect("reflected similarity");
    for conic in conics {
        let mut emitter = Emitter::new();
        let reflected = PcurveGeometry::Transformed(
            PlacedPcurve::try_new(Box::new(conic), transform).expect("placed conic"),
        );
        let replica = super::pcurve(&mut emitter, &reflected).expect("writable reflected conic");
        let lines = emitter.into_lines().expect("finite reflected conic");
        let attributes = operator_attributes(&lines, "CARTESIAN_TRANSFORMATION_OPERATOR_2D", 7);
        assert_eq!(
            referenced_record(&lines, attributes[3]),
            "DIRECTION('',(0.,1.));"
        );
        assert_eq!(
            referenced_record(&lines, attributes[4]),
            "DIRECTION('',(1.,0.));"
        );
        assert_eq!(
            referenced_record(&lines, attributes[5]),
            "CARTESIAN_POINT('',(10.,20.));"
        );
        assert_eq!(attributes[6], "2.");
        assert!(referenced_record(&lines, &replica.to_string()).starts_with("CURVE_REPLICA('',"));
    }
}
