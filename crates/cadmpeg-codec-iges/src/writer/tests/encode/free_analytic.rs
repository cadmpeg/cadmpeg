// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::geometry::{SolvedSurfaceGeometry, Surface, SurfaceGeometry};
use cadmpeg_ir::ids::SurfaceId;
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::report::strictness::code_name;
use cadmpeg_ir::CadIr;
use cadmpeg_test_support::EditableDecodeResult;

use crate::test_support::plan_at;
use crate::test_support::test_solids_and_structure::explicit_tetrahedron_solid_file;
use crate::{IgesCodec, IgesVersion};

#[test]
fn a_model_of_free_analytic_surfaces_writes_an_empty_file_and_names_them() {
    let mut ir = CadIr::empty();
    ir.geometry.surfaces.extend([
        Surface {
            id: SurfaceId::mint("test:model:surface#cylinder").expect("identity grammar"),
            geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cylinder(
                cadmpeg_ir::geometry::analytic::CylinderSurface::try_new(
                    Point3::new(1.0, 2.0, 3.0),
                    Vector3::new(0.0, 0.0, 1.0),
                    Vector3::new(1.0, 0.0, 0.0),
                    2.0,
                )
                .unwrap(),
            )),
            source_object: None,
            native_ref: None,
        },
        Surface {
            id: SurfaceId::mint("test:model:surface#cone").expect("identity grammar"),
            geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cone(
                cadmpeg_ir::geometry::analytic::ConeSurface::try_new(
                    Point3::new(-1.0, 0.0, 0.0),
                    Vector3::new(0.0, 0.0, 1.0),
                    Vector3::new(1.0, 0.0, 0.0),
                    1.0,
                    1.0,
                    std::f64::consts::FRAC_PI_6,
                )
                .unwrap(),
            )),
            source_object: None,
            native_ref: None,
        },
        Surface {
            id: SurfaceId::mint("test:model:surface#sphere").expect("identity grammar"),
            geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Sphere(
                cadmpeg_ir::geometry::analytic::SphereSurface::try_new(
                    Point3::new(0.0, 4.0, 0.0),
                    Vector3::new(0.0, 0.0, 1.0),
                    Vector3::new(1.0, 0.0, 0.0),
                    3.0,
                )
                .unwrap(),
            )),
            source_object: None,
            native_ref: None,
        },
        Surface {
            id: SurfaceId::mint("test:model:surface#torus").expect("identity grammar"),
            geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Torus(
                cadmpeg_ir::geometry::analytic::TorusSurface::try_new(
                    Point3::new(0.0, 0.0, 5.0),
                    Vector3::new(0.0, 0.0, 1.0),
                    Vector3::new(1.0, 0.0, 0.0),
                    4.0,
                    1.0,
                )
                .unwrap(),
            )),
            source_object: None,
            native_ref: None,
        },
    ]);

    let plan = plan_at(IgesVersion::V5_3, &ir, None)
        .expect("a model whose only surfaces are free analytic ones writes an empty file");
    let losses = &plan.report().losses;
    assert!(
        losses
            .iter()
            .any(|loss| code_name(&loss.code) == "export:output.empty"),
        "{losses:?}"
    );
    let message = &losses
        .iter()
        .find(|loss| {
            loss.message
                .starts_with("4 analytic surface record(s) on no face were withheld")
        })
        .unwrap_or_else(|| panic!("the withheld surfaces are named: {losses:?}"))
        .message;
    for name in ["cylinder", "cone", "sphere", "torus"] {
        assert!(
            message.contains(&format!("test:model:surface#{name}")),
            "{message}"
        );
    }
}

#[test]
fn a_free_analytic_surface_beside_brep_topology_is_withheld_and_named() {
    let decoded = IgesCodec
        .decode(
            &mut Cursor::new(explicit_tetrahedron_solid_file()),
            &DecodeOptions::default(),
        )
        .unwrap();
    let mut decoded = EditableDecodeResult::from(decoded);
    decoded.ir_mut().geometry.surfaces.push(Surface {
        id: SurfaceId::mint("test:model:surface#free-sphere").expect("identity grammar"),
        geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Sphere(
            cadmpeg_ir::geometry::analytic::SphereSurface::try_new(
                Point3::new(10.0, 0.0, 0.0),
                Vector3::new(0.0, 0.0, 1.0),
                Vector3::new(1.0, 0.0, 0.0),
                1.0,
            )
            .unwrap(),
        )),
        source_object: None,
        native_ref: None,
    });

    let plan = plan_at(IgesVersion::V5_3, decoded.ir(), None)
        .expect("the solid is written without the free sphere");
    assert!(
        plan.report().losses.iter().any(|loss| loss
            .message
            .starts_with("1 analytic surface record(s) on no face were withheld")
            && loss.message.ends_with("test:model:surface#free-sphere")),
        "{:?}",
        plan.report().losses
    );
    let mut bytes = Vec::new();
    plan.write_to(&mut bytes).expect("write");
    let written = IgesCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("the written file decodes");
    assert_eq!(written.ir().geometry.faces.len(), 4);
    assert!(!written
        .ir()
        .geometry
        .surfaces
        .iter()
        .any(|surface| matches!(
            surface.geometry.solved(),
            Some(SolvedSurfaceGeometry::Sphere(_))
        )));
}
