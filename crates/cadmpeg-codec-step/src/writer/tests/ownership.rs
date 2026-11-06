// SPDX-License-Identifier: Apache-2.0
//! Geometry ownership and active domains at STEP export admission.
#![allow(clippy::unwrap_used)]
#![allow(clippy::default_trait_access)]

use super::reports::edgeless_doc;
use crate::{export::write_step, loss::StepLossCode, StepSchema, StepWriteOptions};
use cadmpeg_ir::{
    geometry::{Curve, CurveGeometry, SolvedCurveGeometry},
    ids::CurveId,
    math::{Point3, Vector3},
    CadIr,
};

#[test]
fn degraded_compound_surface_keeps_its_support_carriers() {
    use cadmpeg_ir::codec::{Codec, DecodeOptions};
    use cadmpeg_ir::geometry::{
        surface_payloads::CompoundSurfacePayload, CompoundComponent, ProceduralSurface,
        ProceduralSurfaceDefinition,
    };
    use std::io::Cursor;

    let source = cadmpeg_ir::examples::unit_cube().expect("cube");
    let mut ir = CadIr::empty();
    ir.model
        .surfaces
        .extend(source.model.surfaces.iter().take(3).cloned());
    let owner = ir.model.surfaces[0].id.clone();
    let support_geometry: Vec<_> = ir.model.surfaces[1..]
        .iter()
        .map(|surface| surface.geometry.clone())
        .collect();
    let components = ir.model.surfaces[1..]
        .iter()
        .zip([0.0, 1.0])
        .map(|(surface, parameter)| CompoundComponent {
            parameter,
            component: surface.id.clone(),
        })
        .collect();
    let definition = ProceduralSurfaceDefinition::Compound(
        CompoundSurfacePayload::try_new(components, None).expect("compound"),
    );
    let construction = ProceduralSurface::new(
        "test:model:procedural_surface#compound"
            .try_into()
            .expect("identity"),
        definition,
        None,
    );
    ir.model
        .add_procedural_surface(&owner, construction)
        .expect("attach construction");
    let mut bytes = Vec::new();
    let report = write_step(
        &ir,
        &mut bytes,
        StepSchema::Ap242Edition3,
        &StepWriteOptions::default(),
    )
    .expect("write compound");
    assert!(report
        .losses
        .iter()
        .any(|loss| loss.code == StepLossCode::ProceduralReducedToCarrier.kind()));
    let decoded = crate::StepCodec::default()
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("decode supports");
    for support in support_geometry {
        assert!(decoded
            .ir()
            .model
            .surfaces
            .iter()
            .any(|surface| surface.geometry == support));
    }
}

#[test]
fn orphaned_carriers_are_written_as_roots_whatever_role_they_carry() {
    for role in [
        cadmpeg_ir::SourceGeometryRole::Support,
        cadmpeg_ir::SourceGeometryRole::Independent,
    ] {
        let mut ir = edgeless_doc();
        ir.model = Default::default();
        let mut surface = edgeless_doc().model.surfaces.remove(0);
        surface.source_object = Some(cadmpeg_ir::SourceObjectAssociation {
            format: cadmpeg_ir::CodecFormat::Iges,
            geometry_role: Some(role),
            object_id: cadmpeg_core::nonblank_literal!("D1"),

            instance_path: Vec::new(),
        });
        ir.model.surfaces.push(surface);
        let mut bytes = Vec::new();
        let report = write_step(
            &ir,
            &mut bytes,
            StepSchema::Ap242Edition3,
            &StepWriteOptions::default(),
        )
        .unwrap();
        let written = std::str::from_utf8(&bytes).unwrap();
        // No live record references the carrier: its owners did not survive
        // the decode, so the support role is vacuous and the plane is
        // written as a root (#233 §5.1: carriers reappear).
        assert!(written.contains("PLANE("), "{role:?} carrier is written");
        assert!(
            !report
                .losses
                .iter()
                .any(|loss| loss.code == StepLossCode::GeometryCarrierNotWritten.kind()),
            "{role:?} carrier names no unwritten loss"
        );
    }
}

#[test]
fn failed_face_does_not_promote_its_surface_to_a_geometric_set() {
    let ir = edgeless_doc();
    let mut bytes = Vec::new();
    let report = write_step(
        &ir,
        &mut bytes,
        StepSchema::Ap242Edition3,
        &StepWriteOptions::default(),
    )
    .unwrap();
    assert!(!std::str::from_utf8(&bytes)
        .unwrap()
        .contains("GEOMETRIC_SET("));
    assert!(!report.losses.is_empty());
}

fn face_with_unbounded_support() -> CadIr {
    use cadmpeg_ir::topology::FaceLoops;
    let mut ir = edgeless_doc();
    ir.model.faces[0].loops = FaceLoops::unspecified(Vec::new());
    ir.model.loops.clear();
    ir.model.coedges.clear();
    ir.model.edges.clear();
    ir.model.vertices.clear();
    ir.model.points.clear();
    ir.model.surfaces[0].source_object = Some(cadmpeg_ir::SourceObjectAssociation {
        format: cadmpeg_ir::CodecFormat::Iges,
        geometry_role: Some(cadmpeg_ir::SourceGeometryRole::Support),
        object_id: cadmpeg_core::nonblank_literal!("support-surface"),
        instance_path: Vec::new(),
    });
    ir
}

#[test]
fn failed_face_retains_its_surface_without_a_shape_root() {
    use cadmpeg_ir::codec::{Codec, DecodeOptions};
    use std::io::Cursor;

    let ir = face_with_unbounded_support();
    let expected = ir.model.surfaces[0].geometry.clone();
    let surface_id = ir.model.surfaces[0].id.clone();
    let face_id = ir.model.faces[0].id.clone();
    let mut bytes = Vec::new();
    let report = write_step(
        &ir,
        &mut bytes,
        StepSchema::Ap242Edition3,
        &StepWriteOptions::default(),
    )
    .unwrap();
    assert!(report.losses.iter().any(|loss| {
        loss.code == StepLossCode::FaceNoWritableBounds.kind()
            && loss.message.contains(face_id.as_str())
    }));
    assert!(report.losses.iter().any(|loss| {
        loss.code == StepLossCode::SourceAssociationOmitted.kind()
            && loss.message.contains(surface_id.as_str())
    }));
    let text = std::str::from_utf8(&bytes).unwrap();
    assert!(text
        .lines()
        .any(|line| line.contains(" = REPRESENTATION('',(#")));
    assert!(!text.contains("GEOMETRIC_SET("));
    let decoded = crate::StepCodec::default()
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    let surface = decoded
        .ir()
        .model
        .surfaces
        .iter()
        .find(|surface| surface.geometry == expected)
        .expect("surface retained");
    assert_ne!(
        surface
            .source_object
            .as_ref()
            .and_then(|source| source.geometry_role),
        Some(cadmpeg_ir::SourceGeometryRole::Independent)
    );
    assert!(decoded.ir().model.faces.is_empty());
}

#[test]
fn failed_face_supports_do_not_gain_a_shared_representation() {
    use cadmpeg_ir::codec::{Codec, DecodeOptions};
    use cadmpeg_ir::geometry::analytic::PlaneSurface;
    use std::io::Cursor;

    let mut ir = face_with_unbounded_support();
    let mut surface = ir.model.surfaces[0].clone();
    surface.id = "test:model:surface#second".try_into().unwrap();
    surface.source_object.as_mut().unwrap().object_id =
        cadmpeg_core::nonblank_literal!("second-support-surface");
    surface.geometry = cadmpeg_ir::geometry::SurfaceGeometry::Solved(
        cadmpeg_ir::geometry::SolvedSurfaceGeometry::Plane(
            PlaneSurface::try_new(
                Point3::new(0.0, 0.0, 5.0),
                Vector3::new(0.0, 0.0, 1.0),
                Vector3::new(1.0, 0.0, 0.0),
            )
            .unwrap(),
        ),
    );
    let mut face = ir.model.faces[0].clone();
    face.id = "test:model:face#second".try_into().unwrap();
    face.surface = surface.id.clone();
    face.shell = "test:model:shell#second".try_into().unwrap();
    let shell = cadmpeg_ir::topology::Shell::with_face(
        face.shell.clone(),
        "test:model:region#second".try_into().unwrap(),
        face.id.clone(),
    );
    let mut region = ir.model.regions[0].clone();
    region.id = shell.region.clone();
    region.body = "test:model:body#second".try_into().unwrap();
    region.shells = vec![shell.id.clone()];
    let mut body = ir.model.bodies[0].clone();
    body.id = region.body.clone();
    body.regions = vec![region.id.clone()];
    ir.model.faces.push(face);
    ir.model.surfaces.push(surface);
    ir.model.shells.push(shell);
    ir.model.regions.push(region);
    ir.model.bodies.push(body);
    let mut bytes = Vec::new();
    write_step(
        &ir,
        &mut bytes,
        StepSchema::Ap242Edition3,
        &StepWriteOptions::default(),
    )
    .unwrap();
    let (exchange, _) =
        crate::test_support::with_service_context(&bytes, crate::parse::parse_inner).unwrap();
    let members = exchange
        .records()
        .values()
        .flat_map(|record| &record.partials)
        .filter(|partial| partial.name == "REPRESENTATION")
        .map(|partial| match &partial.parameters[1] {
            crate::parse::Value::List(members) => members.len(),
            _ => panic!("representation items"),
        })
        .collect::<Vec<_>>();
    assert_eq!(members, vec![1, 1]);
    let decoded = crate::StepCodec::default()
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    for surface in &ir.model.surfaces {
        assert!(decoded
            .ir()
            .model
            .surfaces
            .iter()
            .any(|actual| actual.geometry == surface.geometry));
    }
}

#[test]
fn failed_face_does_not_promote_a_written_construction_support() {
    use cadmpeg_ir::codec::{Codec, DecodeOptions};
    use cadmpeg_ir::geometry::{
        surface_payloads::SubsetSurfaceConstruction, ProceduralSurface,
        ProceduralSurfaceDefinition, Surface, SurfaceGeometry,
    };
    use std::io::Cursor;

    let mut ir = face_with_unbounded_support();
    let support = ir.model.surfaces[0].id.clone();
    let expected = ir.model.surfaces[0].geometry.clone();
    let construction = ProceduralSurface::new(
        "test:model:procedural_surface#subset".try_into().unwrap(),
        ProceduralSurfaceDefinition::Subset(
            SubsetSurfaceConstruction::try_new(
                support,
                [[0.0, 1.0], [0.0, 1.0]],
                Some(true),
                Some(true),
                None,
            )
            .unwrap(),
        ),
        None,
    );
    ir.model.surfaces.push(Surface {
        native_ref: None,
        id: "test:model:surface#subset".try_into().unwrap(),
        geometry: SurfaceGeometry::Procedural {
            construction: construction.id.clone(),
            cache: None,
        },
        source_object: None,
    });
    ir.model.procedural_surfaces.push(construction);
    let mut bytes = Vec::new();
    write_step(
        &ir,
        &mut bytes,
        StepSchema::Ap242Edition3,
        &StepWriteOptions::default(),
    )
    .unwrap();
    let decoded = crate::StepCodec::default()
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    let surface = decoded
        .ir()
        .model
        .surfaces
        .iter()
        .find(|surface| surface.geometry == expected)
        .unwrap();
    assert_eq!(
        surface
            .source_object
            .as_ref()
            .and_then(|source| source.geometry_role),
        Some(cadmpeg_ir::SourceGeometryRole::Support)
    );
}

#[test]
fn an_unwritten_vertex_does_not_promote_its_support_point() {
    let mut fixture = edgeless_doc();
    let mut ir = CadIr::empty();
    ir.model.points.push(fixture.model.points.remove(0));
    ir.model.vertices.push(fixture.model.vertices.remove(0));
    let mut bytes = Vec::new();
    let report = write_step(
        &ir,
        &mut bytes,
        StepSchema::Ap242Edition3,
        &StepWriteOptions::default(),
    )
    .unwrap();
    assert!(!std::str::from_utf8(&bytes)
        .unwrap()
        .contains("GEOMETRIC_CURVE_SET("));
    assert!(report.losses.iter().any(|loss| {
        loss.code == StepLossCode::GeometryCarrierNotWritten.kind()
            && loss.message.contains(ir.model.points[0].id.as_str())
    }));
}

#[test]
fn independent_curve_preserves_its_active_parameter_interval() {
    let mut ir = CadIr::empty();
    ir.model.curves.push(Curve {
        native_ref: None,
        id: CurveId::mint("test:model:curve#active").unwrap(),
        geometry: CurveGeometry::Solved(SolvedCurveGeometry::Line(
            cadmpeg_ir::geometry::analytic::LineCurve::new(
                cadmpeg_ir::features::FinitePoint3::new(Point3::new(0.0, 0.0, 0.0)).unwrap(),
                cadmpeg_ir::units::UnitVector3::normalized(Vector3::new(1.0, 0.0, 0.0)).unwrap(),
            ),
        )),
        parameter_range: cadmpeg_ir::topology::IncreasingParameterInterval::new([2.0, 3.0]),
        source_object: None,
    });
    let mut bytes = Vec::new();
    write_step(
        &ir,
        &mut bytes,
        StepSchema::Ap242Edition3,
        &StepWriteOptions::default(),
    )
    .unwrap();
    let (exchange, _) =
        crate::test_support::with_service_context(&bytes, crate::parse::parse_inner).unwrap();
    let trimmed = exchange
        .records()
        .values()
        .flat_map(|record| &record.partials)
        .find(|partial| partial.name == "TRIMMED_CURVE")
        .expect("active interval is encoded as a trimmed curve");
    for (slot, expected) in [(2, 2.0), (3, 3.0)] {
        assert_eq!(
            trimmed.parameters[slot],
            crate::parse::Value::List(vec![crate::parse::Value::Typed(
                "PARAMETER_VALUE".into(),
                Box::new(crate::parse::Value::Real(
                    cadmpeg_ir::scalar::FiniteReal::new(expected).unwrap()
                ))
            )])
        );
    }
}

#[test]
fn independent_exact_surface_preserves_its_active_uv_rectangle() {
    use cadmpeg_ir::geometry::{
        nurbs::{NurbsSurface, NurbsSurfaceAxis, NurbsSurfaceLanes},
        surface_payloads::ExactSurfacePayload,
        ProceduralSurface, ProceduralSurfaceDefinition, SolvedSurfaceGeometry, Surface,
        SurfaceGeometry,
    };
    let nurbs = NurbsSurface::from_lanes(
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
    .unwrap();
    let mut ir = CadIr::empty();
    let id = cadmpeg_ir::ids::SurfaceId::mint("test:model:surface#active").unwrap();
    ir.model.surfaces.push(Surface {
        native_ref: None,
        id: id.clone(),
        geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Nurbs(nurbs)),
        source_object: None,
    });
    ir.model
        .add_procedural_surface(
            &id,
            ProceduralSurface::new(
                "test:model:procedural_surface#active".try_into().unwrap(),
                ProceduralSurfaceDefinition::Exact(ExactSurfacePayload::from_legacy_intervals(
                    cadmpeg_ir::topology::IncreasingParameterInterval::new([0.0, 3.0]).unwrap(),
                    cadmpeg_ir::topology::IncreasingParameterInterval::new([1.0, 4.0]).unwrap(),
                    0,
                    None,
                )),
                None,
            ),
        )
        .unwrap();
    let mut bytes = Vec::new();
    write_step(
        &ir,
        &mut bytes,
        StepSchema::Ap242Edition3,
        &StepWriteOptions::default(),
    )
    .unwrap();
    let (exchange, _) =
        crate::test_support::with_service_context(&bytes, crate::parse::parse_inner).unwrap();
    let root = exchange
        .records()
        .values()
        .flat_map(|record| &record.partials)
        .find(|partial| partial.name == "SHAPE_REPRESENTATION")
        .unwrap();
    let crate::parse::Value::List(members) = &root.parameters[1] else {
        panic!("root members are a list");
    };
    assert_eq!(members.len(), 1);
    let crate::parse::Value::Reference(id) = members[0] else {
        panic!("root member is a reference");
    };
    let trimmed = &exchange.records()[&id].partials[0];
    assert_eq!(trimmed.name, "RECTANGULAR_TRIMMED_SURFACE");
    for (slot, expected) in [(2, 0.0), (3, 3.0), (4, 1.0), (5, 4.0)] {
        assert_eq!(
            trimmed.parameters[slot],
            crate::parse::Value::Real(cadmpeg_ir::scalar::FiniteReal::new(expected).unwrap())
        );
    }
}

#[test]
fn procedural_support_is_not_an_export_root_even_when_construction_fails() {
    use cadmpeg_ir::geometry::{
        surface_payloads::SubsetSurfaceConstruction, ProceduralSurface,
        ProceduralSurfaceDefinition, Surface, SurfaceGeometry,
    };
    for senses in [Some(true), None] {
        for independent in [false, true] {
            let mut ir = CadIr::empty();
            let mut support = edgeless_doc().model.surfaces.remove(0);
            if senses.is_none() {
                // Absent senses state their direction in the endpoint
                // order, so the trim writes; an unknown support keeps the
                // failure arm failing. It states no plane, so the
                // independent leak arm does not apply to it.
                if independent {
                    continue;
                }
                support.geometry =
                    SurfaceGeometry::Solved(cadmpeg_ir::geometry::SolvedSurfaceGeometry::Unknown {
                        record: None,
                    });
            }
            if independent {
                support.source_object = Some(cadmpeg_ir::SourceObjectAssociation {
                    format: cadmpeg_ir::CodecFormat::Fcstd,
                    geometry_role: Some(cadmpeg_ir::SourceGeometryRole::Independent),
                    object_id: cadmpeg_core::nonblank_literal!("authored-plane"),

                    instance_path: Vec::new(),
                });
            }
            let construction = ProceduralSurface::new(
                "test:model:procedural_surface#subset".try_into().unwrap(),
                ProceduralSurfaceDefinition::Subset(
                    SubsetSurfaceConstruction::try_new(
                        support.id.clone(),
                        [[0.0, 1.0], [0.0, 1.0]],
                        senses,
                        senses,
                        None,
                    )
                    .unwrap(),
                ),
                None,
            );
            ir.model.surfaces.push(support);
            ir.model.surfaces.push(Surface {
                native_ref: None,
                id: "test:model:surface#subset".try_into().unwrap(),
                geometry: SurfaceGeometry::Procedural {
                    construction: construction.id.clone(),
                    cache: None,
                },
                source_object: None,
            });
            ir.model.procedural_surfaces.push(construction);
            let mut bytes = Vec::new();
            let report = write_step(
                &ir,
                &mut bytes,
                StepSchema::Ap242Edition3,
                &StepWriteOptions::default(),
            )
            .unwrap();
            let (exchange, _) =
                crate::test_support::with_service_context(&bytes, crate::parse::parse_inner)
                    .unwrap();
            let members: Vec<_> = exchange
                .records()
                .values()
                .flat_map(|record| &record.partials)
                .filter(|partial| partial.name == "SHAPE_REPRESENTATION")
                .flat_map(|partial| match &partial.parameters[1] {
                    crate::parse::Value::List(items) => items.as_slice(),
                    _ => panic!("root members are a list"),
                })
                .map(|value| match value {
                    crate::parse::Value::Reference(id) => {
                        exchange.records()[id].partials[0].name.as_str()
                    }
                    _ => panic!("root member is a reference"),
                })
                .collect();
            assert_eq!(members.contains(&"PLANE"), independent);
            assert_eq!(
                members.contains(&"RECTANGULAR_TRIMMED_SURFACE"),
                senses.is_some()
            );
            assert_eq!(
                report
                    .losses
                    .iter()
                    .any(|loss| loss.code == StepLossCode::ProceduralDefinitionNotWritten.kind()),
                senses.is_none()
            );
            assert!(!report
                .losses
                .iter()
                .any(|loss| loss.code == StepLossCode::ProceduralReducedToCarrier.kind()));
            if senses.is_some() {
                assert!(!report
                    .losses
                    .iter()
                    .any(|loss| loss.code == StepLossCode::GeometryCarrierNotWritten.kind()));
            }
        }
    }
}

fn standalone_subset_curve(cache: Option<cadmpeg_ir::geometry::LegacyCache>) -> CadIr {
    use cadmpeg_ir::geometry::{
        curve_payloads::SubsetCurveConstruction, ProceduralCurve, ProceduralCurveDefinition,
    };
    let mut ir = CadIr::empty();
    let basis: CurveId = "test:model:curve#basis".try_into().unwrap();
    let construction = ProceduralCurve::new(
        "test:model:procedural_curve#subset".try_into().unwrap(),
        ProceduralCurveDefinition::Subset(
            SubsetCurveConstruction::try_new(basis.clone(), [2.0, 3.0], true, cache).unwrap(),
        ),
    );
    ir.model.curves.push(Curve {
        native_ref: None,
        id: basis,
        parameter_range: None,
        source_object: None,
        geometry: CurveGeometry::Solved(SolvedCurveGeometry::Line(
            cadmpeg_ir::geometry::analytic::LineCurve::new(
                cadmpeg_ir::features::FinitePoint3::new(Point3::new(0.0, 0.0, 0.0)).unwrap(),
                cadmpeg_ir::units::UnitVector3::normalized(Vector3::new(1.0, 0.0, 0.0)).unwrap(),
            ),
        )),
    });
    let solved = cache.map(|_| {
        let ProceduralCurveDefinition::Subset(subset) = construction.definition() else {
            unreachable!("the fixture constructs a subset")
        };
        subset
            .local_cache(ir.model.curves[0].geometry.solved().unwrap())
            .unwrap()
    });
    ir.model.curves.push(Curve {
        native_ref: None,
        id: "test:model:curve#subset".try_into().unwrap(),
        parameter_range: None,
        source_object: None,
        geometry: CurveGeometry::Procedural {
            construction: construction.id.clone(),
            cache: solved,
        },
    });
    ir.model.procedural_curves.push(construction);
    ir
}

#[test]
fn standalone_subset_curve_does_not_export_its_untrimmed_basis_as_a_root() {
    let ir = standalone_subset_curve(None);
    let mut bytes = Vec::new();
    let report = write_step(
        &ir,
        &mut bytes,
        StepSchema::Ap242Edition3,
        &StepWriteOptions::default(),
    )
    .unwrap();
    let (exchange, _) =
        crate::test_support::with_service_context(&bytes, crate::parse::parse_inner).unwrap();
    let root = exchange
        .records()
        .values()
        .flat_map(|record| &record.partials)
        .find(|partial| partial.name == "SHAPE_REPRESENTATION")
        .unwrap();
    let crate::parse::Value::List(members) = &root.parameters[1] else {
        panic!("root list");
    };
    assert_eq!(members.len(), 1);
    let crate::parse::Value::Reference(id) = members[0] else {
        panic!("root reference");
    };
    assert_eq!(exchange.records()[&id].partials[0].name, "TRIMMED_CURVE");
    assert!(!report
        .losses
        .iter()
        .any(|loss| loss.code == StepLossCode::GeometryCarrierNotWritten.kind()));
}

#[test]
fn subset_curve_reports_its_unwritten_cache_fit_tolerance() {
    use cadmpeg_ir::codec::write::{target::TargetRequest, ArenaDisposition, EncodeInput, Encoder};
    use cadmpeg_ir::codec::{Codec, DecodeOptions};
    use cadmpeg_ir::geometry::{LegacyCache, ProceduralCurve};
    use cadmpeg_ir::schema::EntityKind;
    use std::io::Cursor;

    let mut ir = standalone_subset_curve(Some(LegacyCache::try_new(0.01).unwrap()));
    let validation = cadmpeg_ir::validate_neutral(&ir, Vec::new()).unwrap();
    assert!(validation.is_ok(), "{:?}", validation.findings);
    let row = &crate::writer::REDUCED_FIELDS[2];
    assert_eq!(row.path, "model.procedural_curves[].definition");
    assert_eq!(
        row.storage,
        cadmpeg_ir::codec::write::ReducedStorage::Normalized {
            loss: "step/geometry.procedural-curve-cache-tolerance-omitted",
        }
    );
    let original = serde_json::to_value(ir.model.procedural_curves[0].definition()).unwrap();
    let normalized = cadmpeg_test_support::reduced::step_trimmed_curve_cache(&original);
    assert_ne!(original, normalized);
    assert_eq!(normalized["cache"]["fit_tolerance"], 0.0);
    let plan = crate::StepCodec::default()
        .plan(EncodeInput::new(&ir, None), TargetRequest::Inherit)
        .unwrap();
    assert_eq!(
        plan.report()
            .coverage
            .disposition(EntityKind::ProceduralCurve),
        ArenaDisposition::Reported
    );
    assert_eq!(
        plan.report()
            .coverage
            .disposition(EntityKind::ProceduralSurface),
        ArenaDisposition::Written
    );
    let mut bytes = Vec::new();
    let report = write_step(
        &ir,
        &mut bytes,
        StepSchema::Ap242Edition3,
        &StepWriteOptions::default(),
    )
    .unwrap();
    let losses = report
        .losses
        .iter()
        .filter(|loss| {
            loss.code == StepLossCode::ProceduralCurveCacheToleranceOmitted.kind()
                && loss.message.contains("test:model:procedural_curve#subset")
        })
        .count();
    assert_eq!(losses, 1);
    let decoded = crate::StepCodec::default()
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    let fit = decoded
        .ir()
        .model
        .procedural_curves
        .iter()
        .find_map(ProceduralCurve::cache_fit_tolerance)
        .unwrap();
    assert_eq!(fit.get(), 0.0);
    ir.model.procedural_curves[0]
        .set_cache_fit_tolerance(Some(0.0))
        .unwrap();
    let mut exact_bytes = Vec::new();
    let exact_report = write_step(
        &ir,
        &mut exact_bytes,
        StepSchema::Ap242Edition3,
        &StepWriteOptions::default(),
    )
    .unwrap();
    assert!(!exact_report
        .losses
        .iter()
        .any(|loss| { loss.code == StepLossCode::ProceduralCurveCacheToleranceOmitted.kind() }));
}

#[test]
fn subset_surface_reports_its_unwritten_cache_fit_tolerance() {
    use cadmpeg_ir::codec::write::{target::TargetRequest, ArenaDisposition, EncodeInput, Encoder};
    use cadmpeg_ir::geometry::{
        surface_payloads::SubsetSurfaceConstruction, LegacyCache, ProceduralSurface,
        ProceduralSurfaceDefinition, Surface, SurfaceGeometry,
    };
    use cadmpeg_ir::schema::EntityKind;
    let mut ir = CadIr::empty();
    let support = edgeless_doc().model.surfaces.remove(0);
    let solved = support.geometry.solved().unwrap().clone();
    let construction = ProceduralSurface::new(
        "test:model:procedural_surface#subset".try_into().unwrap(),
        ProceduralSurfaceDefinition::Subset(
            SubsetSurfaceConstruction::try_new(
                support.id.clone(),
                [[0.0, 1.0], [0.0, 1.0]],
                Some(true),
                Some(true),
                Some(LegacyCache::try_new(0.01).unwrap()),
            )
            .unwrap(),
        ),
        None,
    );
    ir.model.surfaces.push(support);
    ir.model.surfaces.push(Surface {
        native_ref: None,
        id: "test:model:surface#subset".try_into().unwrap(),
        geometry: SurfaceGeometry::Procedural {
            construction: construction.id.clone(),
            cache: Some(solved),
        },
        source_object: Some(cadmpeg_ir::SourceObjectAssociation {
            format: cadmpeg_ir::CodecFormat::Step,
            geometry_role: Some(cadmpeg_ir::SourceGeometryRole::Independent),
            object_id: cadmpeg_core::nonblank_literal!("authored-surface"),
            instance_path: Vec::new(),
        }),
    });
    ir.model.procedural_surfaces.push(construction);
    let validation = cadmpeg_ir::validate_neutral(&ir, Vec::new()).unwrap();
    assert!(validation.is_ok(), "{:?}", validation.findings);
    let plan = crate::StepCodec::default()
        .plan(EncodeInput::new(&ir, None), TargetRequest::Inherit)
        .unwrap();
    assert_eq!(
        plan.report()
            .coverage
            .disposition(EntityKind::ProceduralSurface),
        ArenaDisposition::Reported
    );
    assert_eq!(
        plan.report()
            .coverage
            .disposition(EntityKind::ProceduralCurve),
        ArenaDisposition::Written
    );
    let mut bytes = Vec::new();
    let report = write_step(
        &ir,
        &mut bytes,
        StepSchema::Ap242Edition3,
        &StepWriteOptions::default(),
    )
    .unwrap();
    assert_eq!(
        report
            .losses
            .iter()
            .filter(|loss| {
                loss.code == StepLossCode::ProceduralSurfaceCacheToleranceOmitted.kind()
                    && loss
                        .message
                        .contains("test:model:procedural_surface#subset")
            })
            .count(),
        1
    );
    assert!(std::str::from_utf8(&bytes)
        .unwrap()
        .contains("RECTANGULAR_TRIMMED_SURFACE("));
}

#[test]
fn failed_composite_curve_does_not_export_its_children_as_roots() {
    use cadmpeg_ir::geometry::{
        CompositeCurveSegment, CompositeCurveSegments, CompositeCurveTransition,
    };
    for missing_child in [false, true] {
        let mut ir = CadIr::empty();
        let child: CurveId = "test:model:curve#child".try_into().unwrap();
        ir.model.curves.push(Curve {
            native_ref: None,
            id: child.clone(),
            parameter_range: None,
            source_object: None,
            geometry: CurveGeometry::Solved(SolvedCurveGeometry::Line(
                cadmpeg_ir::geometry::analytic::LineCurve::new(
                    cadmpeg_ir::features::FinitePoint3::new(Point3::new(0.0, 0.0, 0.0)).unwrap(),
                    cadmpeg_ir::units::UnitVector3::normalized(Vector3::new(1.0, 0.0, 0.0))
                        .unwrap(),
                ),
            )),
        });
        let mut segments = vec![CompositeCurveSegment {
            curve: child,
            same_sense: true,
            transition: CompositeCurveTransition::Continuous,
        }];
        if missing_child {
            segments.push(CompositeCurveSegment {
                curve: "test:model:curve#missing".try_into().unwrap(),
                same_sense: true,
                transition: CompositeCurveTransition::Continuous,
            });
        }
        ir.model.curves.push(Curve {
            native_ref: None,
            id: "test:model:curve#composite".try_into().unwrap(),
            parameter_range: None,
            source_object: None,
            geometry: CurveGeometry::Solved(SolvedCurveGeometry::Composite {
                segments: CompositeCurveSegments::try_from(segments).unwrap(),
                self_intersect: Some(false),
            }),
        });
        let mut bytes = Vec::new();
        let report = write_step(
            &ir,
            &mut bytes,
            StepSchema::Ap242Edition3,
            &StepWriteOptions::default(),
        )
        .unwrap();
        let (exchange, _) =
            crate::test_support::with_service_context(&bytes, crate::parse::parse_inner).unwrap();
        let root = exchange
            .records()
            .values()
            .flat_map(|record| &record.partials)
            .find(|partial| partial.name == "SHAPE_REPRESENTATION");
        if missing_child {
            assert!(root.is_none());
            assert!(report
                .losses
                .iter()
                .any(|loss| loss.code == StepLossCode::GeometryCarrierNotWritten.kind()));
        } else {
            let crate::parse::Value::List(members) = &root.unwrap().parameters[1] else {
                panic!("root list");
            };
            assert_eq!(members.len(), 1);
            let crate::parse::Value::Reference(id) = members[0] else {
                panic!("root reference");
            };
            assert_eq!(exchange.records()[&id].partials[0].name, "COMPOSITE_CURVE");
            assert!(!report
                .losses
                .iter()
                .any(|loss| loss.code == StepLossCode::GeometryCarrierNotWritten.kind()));
        }
    }
}

#[test]
fn additional_carrier_name_attributes_are_written_as_properties() {
    use cadmpeg_ir::attributes::{AttributeTarget, AttributeValue, SourceAttribute};

    let mut ir = cadmpeg_ir::examples::unit_cube().expect("cube");
    let target = AttributeTarget::Surface(ir.model.surfaces[0].id.clone());
    for (id, value) in [
        ("test:model:attribute#first", "First"),
        ("test:model:attribute#second", "Second"),
        ("test:model:attribute#duplicate", "First"),
    ] {
        ir.model.attributes.push(SourceAttribute {
            id: id.try_into().expect("identity"),
            native_ref: None,
            target: target.clone(),
            name: "name".into(),
            values: vec![Some(AttributeValue::String(value.into()))],
        });
    }
    let mut bytes = Vec::new();
    let report = write_step(
        &ir,
        &mut bytes,
        StepSchema::Ap242Edition3,
        &StepWriteOptions::default(),
    )
    .expect("write");
    assert!(std::str::from_utf8(&bytes)
        .unwrap()
        .contains("PLANE('First',"));
    let text = std::str::from_utf8(&bytes).unwrap();
    assert!(text.contains("DESCRIPTIVE_REPRESENTATION_ITEM('','Second')"));
    assert!(text.contains("DESCRIPTIVE_REPRESENTATION_ITEM('','First')"));
    for id in [
        "test:model:attribute#second",
        "test:model:attribute#duplicate",
    ] {
        assert!(!report.losses.iter().any(|loss| loss.code
            == StepLossCode::SourceAttributeNotWritten.kind()
            && loss.message.contains(id)));
    }
    assert!(!report.losses.iter().any(|loss| loss.code
        == StepLossCode::SourceAttributeNotWritten.kind()
        && loss.message.contains("test:model:attribute#first")));
}

#[test]
fn writer_names_a_wire_shell_without_writable_edges() {
    let mut ir = cadmpeg_ir::examples::unit_cube().expect("unit cube fixture is admitted");
    ir.model.bodies[0].kind = cadmpeg_ir::topology::BodyKind::Wire;
    let shell = ir.model.shells[0].id.clone();
    let vertex = ir.model.vertices[0].id.clone();
    ir.model.shells[0]
        .edit_topology(|faces, edges, vertices| {
            faces.clear();
            edges.clear();
            *vertices = vec![vertex];
        })
        .unwrap();

    let report = write_step(
        &ir,
        &mut Vec::new(),
        StepSchema::Ap242Edition3,
        &StepWriteOptions::default(),
    )
    .unwrap();
    assert!(report.losses.iter().any(|loss| {
        loss.code == StepLossCode::WireShellNoWritableEdges.kind()
            && loss.message.contains(shell.as_str())
    }));
}

#[test]
fn a_surface_shell_in_a_mixed_body_is_not_reported_as_an_omitted_wire_shell() {
    let mut ir = cadmpeg_ir::examples::unit_cube().expect("unit cube fixture is admitted");
    ir.model.bodies[0].kind = cadmpeg_ir::topology::BodyKind::General;
    let surface_shell = ir.model.shells[0].id.clone();
    let wire_shell = cadmpeg_ir::ids::ShellId::mint("test:model:shell#free-vertex").unwrap();
    ir.model
        .shells
        .push(cadmpeg_ir::topology::Shell::with_free_vertex(
            wire_shell.clone(),
            ir.model.regions[0].id.clone(),
            ir.model.vertices[0].id.clone(),
        ));
    ir.model.regions[0].shells.push(wire_shell.clone());
    let report = write_step(
        &ir,
        &mut Vec::new(),
        StepSchema::Ap242Edition3,
        &StepWriteOptions::default(),
    )
    .unwrap();
    let losses = report
        .losses
        .iter()
        .filter(|loss| loss.code == StepLossCode::WireShellNoWritableEdges.kind())
        .collect::<Vec<_>>();
    assert_eq!(losses.len(), 1);
    assert!(losses[0].message.contains(wire_shell.as_str()));
    assert!(!losses[0].message.contains(surface_shell.as_str()));
}

#[test]
fn scalar_attributes_characterize_their_step_geometry_items() {
    use cadmpeg_ir::attributes::{AttributeTarget, AttributeValue, SourceAttribute};
    let mut ir = cadmpeg_ir::examples::unit_cube().unwrap();
    let body = ir.model.bodies[0].id.clone();
    let face = ir.model.faces[0].id.clone();
    let surface = ir.model.faces[0].surface.clone();
    ir.model.attributes.push(SourceAttribute {
        id: "test:model:attribute#body-grade".try_into().unwrap(),
        native_ref: None,
        target: AttributeTarget::Body(body.clone()),
        name: "Body grade".into(),
        values: vec![Some(AttributeValue::Integer(5))],
    });
    ir.model.attributes.push(SourceAttribute {
        id: "test:model:attribute#body-label".try_into().unwrap(),
        native_ref: None,
        target: AttributeTarget::Record {
            arena: cadmpeg_ir::document::ArenaName::parse("bodies").unwrap(),
            id: body.as_str().into(),
        },
        name: "Body label".into(),
        values: vec![Some(AttributeValue::String("cast".into()))],
    });
    ir.model.attributes.push(SourceAttribute {
        id: "test:model:attribute#face-finish".try_into().unwrap(),
        native_ref: None,
        target: AttributeTarget::Face(face),
        name: "Finish".into(),
        values: vec![Some(AttributeValue::String("brushed".into()))],
    });
    ir.model.attributes.push(SourceAttribute {
        id: "test:model:attribute#surface-grade".try_into().unwrap(),
        native_ref: None,
        target: AttributeTarget::Record {
            arena: cadmpeg_ir::document::ArenaName::parse("surfaces").unwrap(),
            id: surface.as_str().into(),
        },
        name: "Grade".into(),
        values: vec![Some(AttributeValue::Integer(7))],
    });
    let mut bytes = Vec::new();
    let report = write_step(
        &ir,
        &mut bytes,
        StepSchema::Ap242Edition3,
        &StepWriteOptions::default(),
    )
    .unwrap();
    let text = String::from_utf8(bytes).unwrap();
    for (name, expected_type, expected_value, attribute_id) in [
        (
            "Body grade",
            "MANIFOLD_SOLID_BREP",
            "INTEGER_REPRESENTATION_ITEM('',5)",
            "test:model:attribute#body-grade",
        ),
        (
            "Body label",
            "MANIFOLD_SOLID_BREP",
            "DESCRIPTIVE_REPRESENTATION_ITEM('','cast')",
            "test:model:attribute#body-label",
        ),
        (
            "Finish",
            "ADVANCED_FACE",
            "DESCRIPTIVE_REPRESENTATION_ITEM('','brushed')",
            "test:model:attribute#face-finish",
        ),
        (
            "Grade",
            "PLANE",
            "INTEGER_REPRESENTATION_ITEM('',7)",
            "test:model:attribute#surface-grade",
        ),
    ] {
        let prefix = format!("PROPERTY_DEFINITION('{name}','',#");
        let property = text
            .lines()
            .find(|line| line.contains(&prefix))
            .expect("property definition");
        let owner = property
            .split_once(&prefix)
            .unwrap()
            .1
            .split(')')
            .next()
            .unwrap();
        let owner_prefix = format!("#{owner} = CHARACTERIZED_ITEM_WITHIN_REPRESENTATION('','',#");
        let owner_line = text
            .lines()
            .find(|line| line.starts_with(&owner_prefix))
            .expect("characterized geometry");
        let item = owner_line
            .strip_prefix(&owner_prefix)
            .unwrap()
            .split(',')
            .next()
            .unwrap();
        assert!(text
            .lines()
            .any(|line| line.starts_with(&format!("#{item} = {expected_type}("))));
        let property_id = property.split_once(" = ").unwrap().0;
        let link_prefix = format!("PROPERTY_DEFINITION_REPRESENTATION({property_id},#");
        let link = text
            .lines()
            .find(|line| line.contains(&link_prefix))
            .unwrap();
        let rep = link
            .split_once(&link_prefix)
            .unwrap()
            .1
            .split(')')
            .next()
            .unwrap();
        let rep_prefix = format!("#{rep} = REPRESENTATION('',(#");
        let rep_line = text
            .lines()
            .find(|line| line.starts_with(&rep_prefix))
            .unwrap();
        let value = rep_line
            .strip_prefix(&rep_prefix)
            .unwrap()
            .split(')')
            .next()
            .unwrap();
        assert!(text.contains(&format!("#{value} = {expected_value}")));
        assert!(!report
            .losses
            .iter()
            .any(|loss| loss.message.contains(attribute_id)));
    }
}
