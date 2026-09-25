// SPDX-License-Identifier: Apache-2.0
//! Type 122 directrix ends evaluated on the extrusion's directrix carrier.

use super::super::extrusion_surface_entities;
use cadmpeg_ir::geometry::surface_payloads::ExtrusionSurfaceConstruction;
use cadmpeg_ir::geometry::{
    CacheContract, Curve, CurveGeometry, ProceduralSurface, ProceduralSurfaceDefinition,
    SolvedCurveGeometry,
};
use cadmpeg_ir::ids::{CurveId, ProceduralSurfaceId};
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::CadIr;

#[test]
fn a_type_122_directrix_start_that_overflows_is_refused_as_non_finite() {
    // The circle of radius 1e308 about (MAX, 0, 0) reaches x = MAX + 1e308
    // at angle 0, which has no finite value; at a quarter turn it is finite.
    let directrix = CurveId::mint("test:iges:curve#directrix").expect("identity grammar");
    let construction =
        ProceduralSurfaceId::mint("test:iges:procedural#extrusion").expect("identity grammar");
    let mut ir = CadIr::empty();
    ir.model.curves.push(Curve {
        id: directrix.clone(),
        geometry: CurveGeometry::Solved(SolvedCurveGeometry::Circle(
            cadmpeg_ir::geometry::analytic::CircleCurve::try_new(
                Point3::new(f64::MAX, 0.0, 0.0),
                Vector3::new(0.0, 0.0, 1.0),
                Vector3::new(1.0, 0.0, 0.0),
                1.0e308,
            )
            .expect("valid CircleCurve fixture"),
        )),
        source_object: None,
    });
    ir.model.procedural_surfaces.push(ProceduralSurface::new(
        construction.clone(),
        ProceduralSurfaceDefinition::Extrusion(
            ExtrusionSurfaceConstruction::try_new(
                directrix,
                Some([0.0, std::f64::consts::FRAC_PI_2]),
                Vector3::new(0.0, 0.0, 1.0),
                None,
                CacheContract::from_form(None),
            )
            .expect("valid extrusion fixture"),
        ),
        None,
    ));
    assert_eq!(
        extrusion_surface_entities(&ir, &construction, 0, crate::IgesVersion::V5_3)
            .err()
            .map(|error| error.to_string()),
        Some(
            cadmpeg_core::CodecError::malformed(
                "IGES point Type 122 directrix start has non-finite coordinates"
            )
            .to_string()
        )
    );
}
