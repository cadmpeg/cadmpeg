// SPDX-License-Identifier: Apache-2.0
//! Free constructions are real saved roots; wire admission stays truthful.

use crate::{loss::F3dLossCode, F3dCodec};
use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::geometry::{
    nurbs::{NurbsCurve, NurbsSurface, NurbsSurfaceAxis, NurbsSurfaceLanes},
    surface_payloads::ExtrusionSurfaceConstruction,
    Curve, CurveGeometry, LegacyCache, ProceduralSurface, ProceduralSurfaceDefinition,
    SolvedCurveGeometry, SolvedSurfaceGeometry, Surface, SurfaceGeometry,
};
use cadmpeg_ir::ids::{CurveId, ProceduralSurfaceId, SurfaceId};
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::{native::Native, CadIr};
use std::io::Cursor;

fn add_free_extrusion(ir: &mut CadIr) -> SurfaceId {
    let curve = CurveId::mint("synthetic:free-control:curve#directrix").unwrap();
    let construction =
        ProceduralSurfaceId::mint("synthetic:free-control:construction#extrusion").unwrap();
    let surface = SurfaceId::mint("synthetic:free-control:surface#extrusion").unwrap();
    ir.model.curves.push(Curve {
        id: curve.clone(),
        geometry: CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(
            NurbsCurve::from_lanes(
                1,
                vec![0.0, 0.0, 1.0, 1.0],
                vec![Point3::new(0.0, 0.0, 0.0), Point3::new(2.0, 0.0, 0.0)],
                None,
                false,
            )
            .unwrap(),
        )),
        parameter_range: None,
        source_object: None,
    });
    ir.model.procedural_surfaces.push(ProceduralSurface::new(
        construction.clone(),
        ProceduralSurfaceDefinition::Extrusion(ExtrusionSurfaceConstruction::legacy(
            curve,
            Some(cadmpeg_ir::units::FiniteVector::new([0.0, 1.0]).unwrap()),
            cadmpeg_ir::features::FiniteVector3::new(Vector3::new(0.0, 0.0, 3.0)).unwrap(),
            Some(cadmpeg_ir::features::FinitePoint3::new(Point3::new(0.0, 0.0, 0.0)).unwrap()),
            Some(LegacyCache::try_new(0.001).unwrap()),
        )),
        None,
    ));
    let cache = NurbsSurface::from_lanes(
        NurbsSurfaceAxis::new(1, vec![0.0, 0.0, 1.0, 1.0], false),
        NurbsSurfaceAxis::new(1, vec![0.0, 0.0, 1.0, 1.0], false),
        NurbsSurfaceLanes::new(
            vec![
                vec![Point3::new(0.0, 0.0, 0.0), Point3::new(0.0, 0.0, 3.0)],
                vec![Point3::new(2.0, 0.0, 0.0), Point3::new(2.0, 0.0, 3.0)],
            ],
            None,
        ),
        false,
    )
    .unwrap();
    ir.model.surfaces.push(Surface {
        id: surface.clone(),
        geometry: SurfaceGeometry::Procedural {
            construction,
            cache: Some(SolvedSurfaceGeometry::Nurbs(cache)),
        },
        source_object: None,
    });
    ir.finalize();
    surface
}

fn valid(ir: &CadIr) {
    let report = cadmpeg_ir::validate_neutral(ir, vec![]).unwrap();
    assert!(report.is_ok(), "{report:#?}");
    let findings = F3dCodec
        .validate_native(&cadmpeg_test_support::service_decode_context(), ir)
        .unwrap();
    assert!(findings.is_empty(), "{findings:#?}");
}

#[test]
fn standalone_procedural_surface_roundtrips_without_fabricated_topology() {
    let mut source = CadIr::empty();
    add_free_extrusion(&mut source);
    valid(&source);
    let mut bytes = Vec::new();
    let report = F3dCodec.encode(&source, &mut bytes).unwrap();
    assert!(!report
        .losses
        .iter()
        .any(|loss| loss.code == F3dLossCode::WriterNoExportableSolids.kind()));
    let decoded = F3dCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    valid(decoded.ir());
    assert!(decoded.ir().model.bodies.is_empty());
    assert!(decoded.ir().model.faces.is_empty());
    assert!(decoded.ir().model.edges.is_empty());
    assert_eq!(decoded.ir().model.surfaces.len(), 1);
    assert_eq!(decoded.ir().model.curves.len(), 1);
    assert_eq!(decoded.ir().model.procedural_surfaces.len(), 1);
    assert!(decoded.source_fidelity().unrepresented().is_empty());
    let point = cadmpeg_ir::eval::decode::surface_point_solved(
        cadmpeg_ir::eval::admission::EvaluationAdmission::Standard,
        decoded.ir().model.surfaces[0]
            .geometry
            .solved_cache()
            .unwrap(),
        0.5,
        0.5,
    )
    .unwrap()
    .get();
    assert_eq!(point, Point3::new(1.0, 0.0, 1.5));
    cadmpeg_test_support::contract::assert_written_arenas_agree(&report, &source, decoded.ir());
}

#[test]
fn mixed_wire_omits_unsupported_free_surface_before_record_layout() {
    let decoded = F3dCodec
        .decode(
            &mut Cursor::new(crate::test_support::zip_test::f3d_with_smbh(
                &crate::test_support::smbh_geometry_test::synthetic_wire_body_smbh(),
            )),
            &DecodeOptions::default(),
        )
        .unwrap();
    let mut source = decoded.into_parts().0;
    source.source = None;
    source.native = Native::default();
    let wire = source.model.clone();
    let surface = add_free_extrusion(&mut source);
    valid(&source);
    let mut bytes = Vec::new();
    let report = F3dCodec.encode(&source, &mut bytes).unwrap();
    assert!(report.losses.iter().any(|loss| loss.code
        == F3dLossCode::WriterGeometryOmitted.kind()
        && loss.message.contains(surface.as_str())));
    let decoded = F3dCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    let actual = &decoded.ir().model;
    valid(decoded.ir());
    assert_eq!(actual.bodies.len(), wire.bodies.len());
    assert_eq!(actual.regions.len(), wire.regions.len());
    assert_eq!(actual.shells.len(), wire.shells.len());
    assert_eq!(actual.edges.len(), wire.edges.len());
    assert_eq!(actual.vertices.len(), wire.vertices.len());
    assert_eq!(actual.points.len(), wire.points.len());
    assert!(actual.faces.is_empty());
    assert!(actual.surfaces.is_empty());
    assert!(actual.procedural_surfaces.is_empty());
    assert_eq!(
        actual
            .curves
            .iter()
            .map(|curve| &curve.geometry)
            .collect::<Vec<_>>(),
        wire.curves
            .iter()
            .map(|curve| &curve.geometry)
            .collect::<Vec<_>>()
    );
    assert_eq!(
        actual
            .points
            .iter()
            .map(|point| point.position())
            .collect::<Vec<_>>(),
        wire.points
            .iter()
            .map(|point| point.position())
            .collect::<Vec<_>>()
    );
    cadmpeg_test_support::contract::assert_written_arenas_agree(&report, &source, decoded.ir());
}
