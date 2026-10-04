// SPDX-License-Identifier: Apache-2.0

use crate::test_support::test_surface_fixtures::plane_file;
use crate::IgesCodec;
use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::geometry::SolvedSurfaceGeometry;
use std::io::Cursor;

#[test]
fn decode_projects_an_unbounded_plane_from_implicit_coefficients() {
    let result = IgesCodec
        .decode(&mut Cursor::new(plane_file()), &DecodeOptions::default())
        .unwrap();

    let Some(SolvedSurfaceGeometry::Plane(plane_surface)) =
        result.ir().model.surfaces[0].geometry.solved()
    else {
        panic!("expected a plane carrier");
    };
    let origin = plane_surface.origin();
    let normal = plane_surface.frame().axis().as_raw();
    let u_axis = plane_surface.frame().reference().as_raw();
    assert_eq!(*origin, cadmpeg_ir::math::Point3::new(0.0, 0.0, 2.0));
    assert_eq!(*normal, cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0));
    assert_eq!(*u_axis, cadmpeg_ir::math::Vector3::new(1.0, 0.0, 0.0));
    assert_eq!(
        cadmpeg_ir::eval::decode::surface_point(
            cadmpeg_ir::eval::admission::EvaluationAdmission::Standard,
            &result.ir().model.surfaces[0].geometry,
            1.0,
            3.0
        )
        .map(cadmpeg_ir::features::FinitePoint3::get),
        Ok(cadmpeg_ir::math::Point3::new(1.0, 3.0, 2.0))
    );
    assert!(result.report().losses.is_empty());
    let validation = cadmpeg_ir::validate_neutral(result.ir(), Vec::new())
        .expect("resource allocation did not fail");
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}
