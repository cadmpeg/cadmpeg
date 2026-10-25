// SPDX-License-Identifier: Apache-2.0
use super::*;
use crate::{test_support::test_archive::archive, FcstdCodec};
use cadmpeg_ir::{features::ParameterValue, scalar::Length, Codec, DecodeOptions};
use std::io::Cursor;
fn decode(objects: &[Object]) -> CadIr {
    let document = xml::document(objects, &std::collections::BTreeMap::default(), "4");
    FcstdCodec
        .decode(
            &mut Cursor::new(archive(&document)),
            &DecodeOptions::default(),
        )
        .expect("read authored sketch")
        .into_parts()
        .0
}
fn source(geometry: &str, count: usize, constraints: &str, constraint_count: usize) -> CadIr {
    let mut object = Object::new("Sketch".into(), "Sketcher::SketchObject", "Sketch");
    object.set(
        "Placement",
        xml::property(
            "Placement",
            "App::PropertyPlacement",
            "<PropertyPlacement Px=\"0\" Py=\"0\" Pz=\"0\" Q0=\"0\" Q1=\"0\" Q2=\"0\" Q3=\"1\"/>",
        ),
    );
    object.set(
        "Geometry",
        xml::property(
            "Geometry",
            "Part::PropertyGeometryList",
            &format!("<GeometryList count=\"{count}\">{geometry}</GeometryList>"),
        ),
    );
    object.set(
        "Constraints",
        xml::property(
            "Constraints",
            "Sketcher::PropertyConstraintList",
            &format!("<ConstraintList count=\"{constraint_count}\">{constraints}</ConstraintList>"),
        ),
    );
    decode(&[object])
}
fn synthesize(ir: &CadIr) -> (CadIr, Vec<LossNote>) {
    let mut objects = Vec::new();
    let mut names = HashMap::new();
    let mut losses = Vec::new();
    write(ir, &mut objects, &mut names, &mut losses).expect("write neutral sketch");
    (decode(&objects), losses)
}
#[test]
fn dimensional_edits_and_axis_operands_survive_synthesis() {
    let mut ir = source(
        "<Geometry type=\"Part::GeomLineSegment\"><LineSegment StartX=\"0\" StartY=\"0\" EndX=\"10\" EndY=\"0\"/><Construction value=\"0\"/></Geometry>",
        1,
        "<Constrain Type=\"6\" First=\"0\" FirstPos=\"0\" Value=\"10\" Name=\"length\" IsDriving=\"1\" IsActive=\"1\"/><Constrain Type=\"9\" First=\"0\" FirstPos=\"0\" Second=\"-2\" SecondPos=\"0\" Value=\"1.5707963267948966\"/>",
        2,
    );
    assert_eq!(ir.model.parameters.len(), 2);
    ir.model.parameters[0].value = Some(ParameterValue::Length(Length::new(27.0).expect("length")));
    let (written, losses) = synthesize(&ir);
    assert!(losses.is_empty(), "{losses:?}");
    assert_eq!(written.model.sketch_constraints.len(), 2);
    assert!(
        matches!(&written.model.parameters[0].value,Some(ParameterValue::Length(value)) if value.get()==27.0)
    );
    assert_eq!(
        serde_json::to_value(written.model.sketch_constraints[1].definition.kind()).unwrap()
            ["axis"],
        "vertical"
    );
}
#[test]
fn conics_and_weighted_spline_lanes_survive_synthesis() {
    let ir = source(
        concat!(
            "<Geometry type=\"Part::GeomArcOfEllipse\"><ArcOfEllipse CenterX=\"1\" CenterY=\"2\" AngleXU=\"0.3\" MajorRadius=\"7\" MinorRadius=\"3\" StartAngle=\"0.2\" EndAngle=\"2.1\"/><Construction value=\"1\"/></Geometry>",
            "<Geometry type=\"Part::GeomBSplineCurve\"><BSplineCurve Degree=\"2\" IsPeriodic=\"0\" PolesCount=\"3\" KnotsCount=\"2\"><Pole X=\"0\" Y=\"0\" Z=\"0\" Weight=\"1\"/><Pole X=\"2\" Y=\"4\" Z=\"0\" Weight=\"0.5\"/><Pole X=\"5\" Y=\"1\" Z=\"0\" Weight=\"1\"/><Knot Value=\"0\" Mult=\"3\"/><Knot Value=\"1\" Mult=\"3\"/></BSplineCurve><Construction value=\"0\"/></Geometry>"
        ),
        2,
        "",
        0,
    );
    assert_eq!(ir.model.sketch_entities.len(), 2);
    let (written, losses) = synthesize(&ir);
    assert!(losses.is_empty(), "{losses:?}");
    assert_eq!(written.model.sketch_entities.len(), 2);
    for (before, after) in ir
        .model
        .sketch_entities
        .iter()
        .zip(&written.model.sketch_entities)
    {
        assert_eq!(before.geometry, after.geometry);
        assert_eq!(before.construction, after.construction);
    }
}

#[test]
fn external_cache_indices_and_live_dependency_survive_synthesis() {
    let support = Object::new("Support".into(), "Part::Feature", "Support");
    let mut sketch = Object::new("Sketch".into(), "Sketcher::SketchObject", "Sketch");
    sketch.set(
        "Placement",
        xml::property(
            "Placement",
            "App::PropertyPlacement",
            "<PropertyPlacement Px=\"0\" Py=\"0\" Pz=\"0\" Q0=\"0\" Q1=\"0\" Q2=\"0\" Q3=\"1\"/>",
        ),
    );
    sketch.set(
        "Geometry",
        xml::property(
            "Geometry",
            "Part::PropertyGeometryList",
            "<GeometryList count=\"0\"/>",
        ),
    );
    sketch.set(
        "ExternalGeometry",
        xml::property(
            "ExternalGeometry",
            "App::PropertyLinkSubList",
            "<LinkSubList count=\"1\"><Link obj=\"Support\" sub=\"Edge1\"/></LinkSubList>",
        ),
    );
    sketch.set("ExternalGeo", xml::property("ExternalGeo", "Part::PropertyGeometryList", "<GeometryList count=\"3\"><Geometry id=\"-1\"/><Geometry id=\"-2\"/><Geometry type=\"Part::GeomLineSegment\" ref=\"Support.Edge1\"><LineSegment StartX=\"1\" StartY=\"2\" EndX=\"3\" EndY=\"4\"/><Construction value=\"1\"/></Geometry></GeometryList>"));
    sketch.set("Constraints", xml::property("Constraints", "Sketcher::PropertyConstraintList", "<ConstraintList count=\"1\"><Constrain Type=\"17\" First=\"-3\" FirstPos=\"0\"/></ConstraintList>"));
    let ir = decode(&[support.clone(), sketch]);
    assert_eq!(ir.model.sketch_entities.len(), 1);
    assert_eq!(
        ir.model.sketch_entities[0].endpoint_refs,
        ["Edge1"],
        "original live dependency"
    );
    assert_eq!(ir.model.sketch_constraints.len(), 1);
    let mut objects = vec![support];
    // #215: local selectors must name an actual emitted declaration. The
    // complete writer supplies this map; this unit caller supplies its peer.
    let mut names = ir
        .native
        .namespace("fcstd")
        .unwrap()
        .arena_as::<crate::native::ObjectRecord>("objects")
        .unwrap()
        .into_iter()
        .filter(|native| objects.iter().any(|object| &object.name == native.name()))
        .map(|object| (object.id().as_str().to_owned(), object.name().to_owned()))
        .collect::<HashMap<_, _>>();
    let mut losses = Vec::new();
    write(&ir, &mut objects, &mut names, &mut losses).expect("write typed external cache");
    assert!(losses.is_empty(), "{losses:?}");
    let written = decode(&objects);
    assert_eq!(written.model.sketch_entities.len(), 1);
    assert_eq!(written.model.sketch_constraints.len(), 1);
    assert_eq!(
        written.model.sketch_entities[0].geometry,
        ir.model.sketch_entities[0].geometry
    );
    assert_eq!(written.model.sketch_entities[0].endpoint_refs, ["Edge1"]);
}
