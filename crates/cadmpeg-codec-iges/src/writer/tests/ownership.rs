// SPDX-License-Identifier: Apache-2.0
//! Exact domains and source dependencies at semantic export.
#![allow(clippy::unwrap_used)]
use crate::{IgesCodec, IgesVersion};
use cadmpeg_ir::codec::{
    write::{target::TargetRequest, EncodeInput, Encoder},
    DecodeOptions,
};
use cadmpeg_ir::geometry::{
    nurbs::{NurbsSurface, NurbsSurfaceAxis, NurbsSurfaceLanes},
    surface_payloads::ExactSurfacePayload,
    ProceduralSurface, ProceduralSurfaceDefinition, SolvedSurfaceGeometry, Surface,
    SurfaceGeometry,
};
use cadmpeg_ir::math::Point3;
use cadmpeg_ir::topology::IncreasingParameterInterval;
use cadmpeg_ir::{CadIr, Codec, SourceGeometryRole, SourceObjectAssociation};
use std::io::Cursor;

fn rectangle() -> CadIr {
    let ctx = cadmpeg_test_support::service_decode_context();
    let basis = NurbsSurface::from_lanes(
        &ctx,
        NurbsSurfaceAxis::new(1, vec![0.0, 0.0, 5.0, 5.0], false),
        NurbsSurfaceAxis::new(1, vec![0.0, 0.0, 5.0, 5.0], false),
        NurbsSurfaceLanes::new(
            vec![
                vec![Point3::new(0.0, 0.0, 0.0), Point3::new(0.0, 10.0, 0.0)],
                vec![Point3::new(10.0, 0.0, 0.0), Point3::new(10.0, 10.0, 0.0)],
            ],
            None,
        ),
        false,
    )
    .unwrap()
    .unwrap();
    let mut ir = CadIr::empty();
    let id = "test:model:surface#rectangle".try_into().unwrap();
    ir.model.surfaces.push(Surface {
        id,
        geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Nurbs(basis)),
        source_object: None,
    });
    ir
}

fn round_trip(ir: &CadIr) -> cadmpeg_ir::codec::DecodeResult {
    let plan = IgesCodec
        .plan(
            EncodeInput::new(ir, None),
            TargetRequest::Explicit(IgesVersion::V5_3.descriptor().id.as_str()),
        )
        .unwrap();
    let mut bytes = Vec::new();
    plan.write_to(&mut bytes).unwrap();
    IgesCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap()
}

#[test]
fn exact_surface_export_keeps_its_active_rectangle() {
    let mut ir = rectangle();
    let owner = ir.model.surfaces[0].id.clone();
    let ctx = cadmpeg_test_support::service_decode_context();
    ir.model
        .add_procedural_surface(
            &ctx,
            &owner,
            ProceduralSurface::new(
                "test:model:procedure#active".try_into().unwrap(),
                ProceduralSurfaceDefinition::Exact(ExactSurfacePayload::from_legacy_intervals(
                    IncreasingParameterInterval::new([0.0, 3.0]).unwrap(),
                    IncreasingParameterInterval::new([1.0, 4.0]).unwrap(),
                    0,
                    None,
                )),
                None,
            ),
        )
        .unwrap()
        .unwrap();
    let result = round_trip(&ir);
    let Some(SolvedSurfaceGeometry::Nurbs(basis)) =
        result.ir().model.surfaces[0].geometry.solved_cache()
    else {
        panic!("exact NURBS basis")
    };
    assert_eq!(basis.u_knots().as_slice(), &[0.0, 0.0, 3.0, 3.0]);
    assert_eq!(basis.v_knots().as_slice(), &[1.0, 1.0, 4.0, 4.0]);
    assert_eq!(basis.pole(0, 0).unwrap(), Point3::new(0.0, 2.0, 0.0));
    assert_eq!(basis.pole(1, 1).unwrap(), Point3::new(6.0, 8.0, 0.0));
    let definition = &result.ir().model.procedural_surfaces[0];
    let ProceduralSurfaceDefinition::Exact(payload) = definition.definition() else {
        panic!("exact domain")
    };
    let cadmpeg_ir::geometry::ExactSpline::Legacy { ranges, .. } = payload.spline() else {
        panic!("legacy exact domain")
    };
    assert_eq!(
        ranges.map(|range| range.map(cadmpeg_ir::scalar::FiniteReal::get)),
        [[0.0, 3.0], [1.0, 4.0]]
    );
}

fn support_association() -> SourceObjectAssociation {
    SourceObjectAssociation {
        format: cadmpeg_ir::CodecFormat::Step,
        geometry_role: Some(SourceGeometryRole::Support),
        object_id: cadmpeg_core::nonblank_literal!("support"),
        name: None,
        color: None,
        visible: None,
        layer: None,
        instance_path: Vec::new(),
    }
}

#[test]
fn unowned_support_surface_and_point_are_withheld_with_loss() {
    let mut ir = rectangle();
    let mut support = ir.model.surfaces[0].clone();
    support.id = "test:model:surface#support".try_into().unwrap();
    support.source_object = Some(support_association());
    ir.model.surfaces.push(support);
    ir.model.points.push(cadmpeg_ir::topology::Point::new(
        "test:model:point#support".try_into().unwrap(),
        cadmpeg_ir::features::FinitePoint3::new(Point3::new(2.0, 3.0, 4.0)).unwrap(),
        Some(support_association()),
    ));
    let generated = super::super::synthesize(&ir, IgesVersion::V5_3).unwrap();
    assert_eq!(
        generated
            .losses
            .iter()
            .filter(|loss| {
                loss.code == crate::loss::IgesLossCode::WriterSupportGeometryNotRepresented.kind()
            })
            .count(),
        2
    );
    assert!(generated
        .losses
        .iter()
        .filter(|loss| loss.code
            == crate::loss::IgesLossCode::WriterSupportGeometryNotRepresented.kind())
        .all(|loss| loss.message.starts_with("1 support geometry record")));
    let result = IgesCodec
        .decode(&mut Cursor::new(generated.bytes), &DecodeOptions::default())
        .unwrap();
    assert_eq!(result.ir().model.surfaces.len(), 1);
    assert!(result.ir().model.faces.is_empty());
    assert_eq!(
        result.ir().model.surfaces[0]
            .source_object
            .as_ref()
            .unwrap()
            .geometry_role,
        Some(SourceGeometryRole::Independent)
    );
    assert!(result.ir().model.points.is_empty());
    assert!(result.ir().model.vertices.is_empty());
}

#[test]
fn support_only_surface_export_refuses_an_empty_transfer() {
    let mut ir = rectangle();
    ir.model.surfaces[0].source_object = Some(support_association());
    assert!(matches!(
        super::super::synthesize(&ir, IgesVersion::V5_3),
        Err(cadmpeg_core::CodecError::NotImplemented(_))
    ));
}

#[test]
fn support_only_point_export_refuses_an_empty_transfer() {
    let mut ir = CadIr::empty();
    ir.model.points.push(cadmpeg_ir::topology::Point::new(
        "test:model:point#support".try_into().unwrap(),
        cadmpeg_ir::features::FinitePoint3::new(Point3::new(2.0, 3.0, 4.0)).unwrap(),
        Some(support_association()),
    ));
    assert!(matches!(
        super::super::synthesize(&ir, IgesVersion::V5_3),
        Err(cadmpeg_core::CodecError::NotImplemented(_))
    ));
}

#[test]
fn face_owned_support_surface_remains_available_to_its_owner() {
    let decoded = IgesCodec
        .decode(
            &mut Cursor::new(crate::test_support::test_surface_fixtures::trimmed_plane_file()),
            &DecodeOptions::default(),
        )
        .unwrap();
    let mut ir = decoded.ir().clone();
    assert_eq!(ir.model.surfaces.len(), 1);
    ir.model.surfaces[0].source_object = Some(support_association());
    ir.native = cadmpeg_ir::native::Native::default();
    let generated = super::super::synthesize(&ir, IgesVersion::V5_3).unwrap();
    assert!(!generated
        .losses
        .iter()
        .any(|loss| loss.code
            == crate::loss::IgesLossCode::WriterSupportGeometryNotRepresented.kind()));
    let result = IgesCodec
        .decode(&mut Cursor::new(generated.bytes), &DecodeOptions::default())
        .unwrap();
    assert_eq!(result.ir().model.surfaces.len(), 1);
    assert_eq!(result.ir().model.faces.len(), 1);
    assert_eq!(
        result.ir().model.surfaces[0]
            .source_object
            .as_ref()
            .unwrap()
            .geometry_role,
        Some(SourceGeometryRole::Support)
    );
    assert!(cadmpeg_ir::validate_neutral(result.ir(), Vec::new())
        .unwrap()
        .is_ok());
}

#[test]
fn support_curve_without_topology_keeps_physical_dependency() {
    let mut ir = rectangle();
    let fixture = IgesCodec
        .decode(
            &mut Cursor::new(crate::test_support::test_curves_and_surfaces::line_file(0)),
            &DecodeOptions::default(),
        )
        .unwrap();
    let mut curve = fixture.ir().model.curves[0].clone();
    assert!(curve.parameter_range.is_some());
    curve.source_object = Some(support_association());
    ir.model.curves.push(curve);
    let result = round_trip(&ir);
    assert_eq!(result.ir().model.curves.len(), 1);
    assert!(result.ir().model.edges.is_empty());
    assert_eq!(
        result.ir().model.curves[0]
            .source_object
            .as_ref()
            .unwrap()
            .geometry_role,
        Some(SourceGeometryRole::Support)
    );
}

#[test]
fn dependent_iges_curve_cannot_round_trip_as_an_independent_edge() {
    let mut file = crate::test_support::test_curves_and_surfaces::line_file(0);
    let directory = file
        .chunks_exact_mut(81)
        .find(|card| card[72] == b'D')
        .unwrap();
    directory[64..72].copy_from_slice(b"00010000");
    let decoded = IgesCodec
        .decode(&mut Cursor::new(file), &DecodeOptions::default())
        .unwrap();
    assert_eq!(decoded.ir().model.curves.len(), 1);
    assert!(decoded.ir().model.edges.is_empty());
    let result = round_trip(decoded.ir());
    assert_eq!(result.ir().model.curves.len(), 1);
    assert!(result.ir().model.edges.is_empty());
    assert_eq!(
        result.ir().model.curves[0]
            .source_object
            .as_ref()
            .unwrap()
            .geometry_role,
        Some(SourceGeometryRole::Support)
    );
}

#[test]
fn standalone_edge_with_support_curve_is_withheld_with_counted_export_loss() {
    let decoded = IgesCodec
        .decode(
            &mut Cursor::new(crate::test_support::test_curves_and_surfaces::line_file(0)),
            &DecodeOptions::default(),
        )
        .unwrap();
    let mut ir = decoded.ir().clone();
    assert_eq!(ir.model.edges.len(), 1);
    ir.model.curves[0]
        .source_object
        .as_mut()
        .unwrap()
        .geometry_role = Some(SourceGeometryRole::Support);
    let plan = IgesCodec
        .plan(EncodeInput::new(&ir, None), TargetRequest::Inherit)
        .unwrap();
    assert!(plan.report().losses.iter().any(|loss| loss.code
        == crate::loss::IgesLossCode::WriterSupportEdgeNotRepresented.kind()
        && loss.message.starts_with("1 standalone edge record")));
    let mut written = Vec::new();
    plan.write_to(&mut written).unwrap();
    let result = IgesCodec
        .decode(&mut Cursor::new(written), &DecodeOptions::default())
        .unwrap();
    assert_eq!(result.ir().model.curves.len(), 1);
    assert!(result.ir().model.edges.is_empty());
    assert_eq!(
        result.ir().model.curves[0]
            .source_object
            .as_ref()
            .unwrap()
            .geometry_role,
        Some(SourceGeometryRole::Support)
    );
}

#[test]
fn topology_owned_curve_preserves_support_and_independent_roles() {
    let decoded = IgesCodec
        .decode(
            &mut Cursor::new(
                crate::test_support::test_solids_and_structure::explicit_tetrahedron_solid_file(),
            ),
            &DecodeOptions::default(),
        )
        .unwrap();
    for role in [SourceGeometryRole::Support, SourceGeometryRole::Independent] {
        let mut ir = decoded.ir().clone();
        ir.native = cadmpeg_ir::native::Native::default();
        for curve in &mut ir.model.curves {
            let mut source = support_association();
            source.geometry_role = Some(role);
            curve.source_object = Some(source);
        }
        let generated = super::super::synthesize(&ir, IgesVersion::V5_3).unwrap();
        let expected_status = match role {
            SourceGeometryRole::Support => b"00010000",
            SourceGeometryRole::Independent => b"00000000",
        };
        let curve_entries: Vec<_> = generated
            .bytes
            .chunks_exact(81)
            .filter(|card| card[72] == b'D' && card[..8].trim_ascii() == b"110")
            .collect();
        assert_eq!(curve_entries.len(), ir.model.curves.len() * 2);
        assert!(curve_entries
            .chunks_exact(2)
            .all(|entry| &entry[0][64..72] == expected_status));
        let result = IgesCodec
            .decode(&mut Cursor::new(generated.bytes), &DecodeOptions::default())
            .unwrap();
        assert_eq!(result.ir().model.curves.len(), ir.model.curves.len());
        assert!(result.ir().model.curves.iter().all(|curve| curve
            .source_object
            .as_ref()
            .unwrap()
            .geometry_role
            == Some(SourceGeometryRole::Support)));
    }
}

#[test]
fn free_points_and_vertices_on_brep_and_trimmed_sheet_paths_have_export_losses() {
    for file in [
        crate::test_support::test_solids_and_structure::explicit_tetrahedron_solid_file(),
        crate::test_support::test_surface_fixtures::trimmed_plane_file(),
    ] {
        let decoded = IgesCodec
            .decode(&mut Cursor::new(file), &DecodeOptions::default())
            .unwrap();
        let mut ir = decoded.ir().clone();
        ir.native = cadmpeg_ir::native::Native::default();
        let free = IgesCodec
            .decode(
                &mut Cursor::new(crate::test_support::test_curves_and_surfaces::point_file()),
                &DecodeOptions::default(),
            )
            .unwrap();
        let mut point = free.ir().model.points[0].clone();
        point.id = "test:model:point#free".try_into().unwrap();
        let mut vertex = free.ir().model.vertices[0].clone();
        vertex.id = "test:model:vertex#free".try_into().unwrap();
        vertex.point = point.id.clone();
        ir.model.points.push(point);
        ir.model.vertices.push(vertex.clone());
        if let Some(body) = ir
            .model
            .bodies
            .iter()
            .find(|body| super::super::is_decoder_free_geometry_body(body))
        {
            let region = ir
                .model
                .regions
                .iter()
                .find(|region| region.id == body.regions[0])
                .unwrap();
            let shell = ir
                .model
                .shells
                .iter_mut()
                .find(|shell| shell.id == region.shells[0])
                .unwrap();
            let mut vertices = shell.free_vertices().to_vec();
            vertices.push(vertex.id);
            *shell = cadmpeg_ir::topology::Shell::new(
                shell.id.clone(),
                shell.region.clone(),
                shell.faces().to_vec(),
                shell.wire_edges().to_vec(),
                vertices,
            )
            .unwrap();
        } else {
            ir.model.bodies.extend(free.ir().model.bodies.clone());
            ir.model.regions.extend(free.ir().model.regions.clone());
            let shell = &free.ir().model.shells[0];
            ir.model.shells.push(
                cadmpeg_ir::topology::Shell::new(
                    shell.id.clone(),
                    shell.region.clone(),
                    Vec::new(),
                    Vec::new(),
                    vec![vertex.id],
                )
                .unwrap(),
            );
        }
        let generated = super::super::synthesize(&ir, IgesVersion::V5_3).unwrap();
        assert!(generated.losses.iter().any(|loss| loss.code
            == crate::loss::IgesLossCode::WriterFreeGeometryOmitted.kind()
            && loss.message.contains("free vertex record(s)")));
        assert!(generated.losses.iter().any(|loss| loss.code
            == crate::loss::IgesLossCode::WriterFreeGeometryOmitted.kind()
            && loss
                .message
                .starts_with("1 free point record (test:model:point#free)")));
    }
}
