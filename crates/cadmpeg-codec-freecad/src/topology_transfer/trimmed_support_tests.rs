// SPDX-License-Identifier: Apache-2.0

use crate::test_support::test_archive::archive_entries;
use crate::FcstdCodec;
use cadmpeg_ir::eval::admission::EvaluationAdmission;
use cadmpeg_ir::math::Point3;
use cadmpeg_ir::{Codec, DecodeOptions};
use std::io::Cursor;

#[test]
fn trimmed_sphere_face_uses_unrestricted_support_without_healing_pcurves() {
    let document = br#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="1"><Object type="Part::Feature" name="Shape" id="1"/></Objects>
<ObjectData Count="1"><Object name="Shape"><Properties Count="1"><Property name="Shape" type="Part::PropertyPartShape"><Part file="Shape.brp"/></Property></Properties></Object></ObjectData>
</Document>"#;
    for (longitude, last_latitude, trims, placed) in [
        (-1.0e-17_f64, 0.25_f64, 1, false),
        (-0.125_f64, 0.25_f64, 1, false),
        (0.125_f64, 0.75_f64, 2, true),
    ] {
        let (sine, cosine) = longitude.sin_cos();
        let radius = 2.0;
        let first = Point3::new(radius * cosine, radius * sine, 0.0);
        let last = Point3::new(
            radius * last_latitude.cos() * cosine,
            radius * last_latitude.cos() * sine,
            radius * last_latitude.sin(),
        );
        let locations = if placed {
            "Locations 1\n1\n1 0 0 10\n0 1 0 20\n0 0 1 30"
        } else {
            "Locations 0"
        };
        let root_location = usize::from(placed);
        let trims = "10 0 6.283185307179586 -0.5 0.5 ".repeat(trims);
        let brep = format!(
            "CASCADE Topology V1, (c) Matra-Datavision
{locations}
Curve2ds 1
1 {longitude} 0 0 1
Curves 1
2 0 0 0 {sine} {negative_cosine} 0 {cosine} {sine} 0 0 0 1 {radius}
Polygon3D 0
PolygonOnTriangulations 0
Surfaces 1
{trims}4 0 0 0 0 0 1 1 0 0 0 1 0 {radius}
Triangulations 0
TShapes 5
Ve 0.0000001 {first_x} {first_y} {first_z} 0 0 1001000 *
Ve 0.0000001 {last_x} {last_y} {last_z} 0 0 1001000 *
Ed 0.0000001 1 1 0 1 1 0 0 {last_latitude} 2 1 1 0 0 {last_latitude} 0 1001000 +5 0 -4 0 *
Wi 1001000 +3 0 -3 0 *
Fa 0 0.0000001 1 0 1001000 +2 0 *
+1 {root_location} *",
            negative_cosine = -cosine,
            first_x = first.x,
            first_y = first.y,
            first_z = first.z,
            last_x = last.x,
            last_y = last.y,
            last_z = last.z,
        );
        let bytes = archive_entries(&[("Document.xml", document), ("Shape.brp", brep.as_bytes())]);
        let result = FcstdCodec
            .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
            .expect("trimmed sphere with unrestricted pcurve support");
        let ir = result.ir();
        let index = cadmpeg_ir::index::ModelIndex::build(ir);
        let face = &ir.model.faces[0];
        assert!(face.surface.as_str().contains("basis"));
        assert!(ir.model.procedural_surfaces.iter().any(|surface| matches!(
            surface.definition(),
            cadmpeg_ir::geometry::ProceduralSurfaceDefinition::Subset(_)
        )));
        let pcurve = &ir.model.pcurves[0];
        for (parameter, expected) in [(0.0, first), (last_latitude, last)] {
            let uv = cadmpeg_ir::eval::decode::pcurve_uv(
                EvaluationAdmission::Standard,
                &pcurve.geometry,
                parameter,
            )
            .expect("original pcurve evaluates")
            .get();
            assert_eq!(uv.u, longitude);
            assert_eq!(uv.v, parameter);
            let actual = cadmpeg_ir::eval::model_surface_point_by_id(
                EvaluationAdmission::Standard,
                &index,
                &face.surface,
                uv.u,
                uv.v,
            )
            .expect("face support evaluates outside the stored rectangular bounds")
            .get();
            let expected = if placed {
                Point3::new(expected.x + 10.0, expected.y + 20.0, expected.z + 30.0)
            } else {
                expected
            };
            assert!(Point3::distance(actual, expected) < 1.0e-12);
        }
        let validation = cadmpeg_ir::validate_neutral(ir, Vec::new()).unwrap();
        assert!(
            validation.findings.iter().all(|finding| {
                finding.check != cadmpeg_ir::report::check::Check::GeometricConsistency
                    || finding.severity != cadmpeg_ir::report::Severity::Error
            }),
            "{:?}",
            validation.findings,
        );
    }
}
