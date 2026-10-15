// SPDX-License-Identifier: Apache-2.0

use super::super::decode_pcurves;
use super::{
    append_record_links, brep_free_vertex_indices, face_components, line_nurbs,
    region_shell_groups_without_records, source_shaped_plane_brep, stage_brep, with_expand_bytes,
    ArchiveVersion, Body, BodyKind, BrepDraft, BrepTransferInput, BrepTransferKind, CadIr, Curve,
    CurveGeometry, MillimeterScale, NativeUnknownRecord, PcurveGeometry, SolvedCurveGeometry,
    SolvedSurfaceGeometry, SourceObjectAssociation, Surface, UnknownId,
};
use crate::loss::RhinoLossCode;

#[test]
fn plane_pcurve_scaling_keeps_the_plane_surface() {
    let plane = cadmpeg_ir::geometry::analytic::PlaneSurface::try_new(
        cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
        cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0),
        cadmpeg_ir::math::Vector3::new(1.0, 0.0, 0.0),
    )
    .expect("valid plane");
    let surface_id: cadmpeg_ir::ids::SurfaceId = "rhino:object:surface#plane"
        .try_into()
        .expect("valid identity");
    let mut staged = BrepDraft::default();
    staged.draft.model_mut().surfaces.push(Surface {
        id: surface_id.clone(),
        geometry: cadmpeg_ir::geometry::SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
            plane,
        )),
        source_object: None,
    });
    let scale = crate::test_support::millimeter_scale(25.4);
    super::super::scale_plane_pcurves(&mut staged, scale)
        .expect("service profile admits plane lookup");
    assert_eq!(staged.draft.model().surfaces[0].id, surface_id);
}

#[test]
fn fallback_discards_topology_and_unknown_record_self_link() {
    let curve_id: cadmpeg_ir::ids::CurveId = "rhino:object:curve#x.c3-0"
        .try_into()
        .expect("valid identity");
    let surface_id: cadmpeg_ir::ids::SurfaceId = "rhino:object:surface#x.slot-0"
        .try_into()
        .expect("valid identity");
    let mut staged = BrepDraft {
        links: vec![
            curve_id.to_string(),
            surface_id.to_string(),
            "rhino:object:body#x".to_string(),
            "rhino:object:record#x".to_string(),
        ],
        ..BrepDraft::default()
    };
    staged.draft.model_mut().curves.push(Curve {
        parameter_range: None,
        id: curve_id.clone(),
        geometry: CurveGeometry::Solved(SolvedCurveGeometry::Unknown { record: None }),
        source_object: None,
    });
    staged.draft.model_mut().surfaces.push(Surface {
        id: surface_id.clone(),
        geometry: cadmpeg_ir::geometry::SurfaceGeometry::Solved(SolvedSurfaceGeometry::Unknown {
            record: None,
        }),
        source_object: None,
    });
    staged.draft.model_mut().bodies.push(Body {
        id: "rhino:object:body#x".try_into().expect("valid identity"),
        kind: BodyKind::Sheet,
        regions: Vec::new(),
        transform: None,
        name: None,
        color: None,
        visible: None,
    });
    staged = staged.free_carrier_fallback("C2 failure");
    assert_eq!(staged.kind, BrepTransferKind::FreeCarrierFallback);
    assert!(staged.draft.model().bodies.is_empty());
    assert_eq!(
        staged.links,
        vec![curve_id.to_string(), surface_id.to_string()]
    );
    assert!(staged.warnings.iter().any(|warning| warning.contains("C2")));
}

#[test]
fn failed_brep_retains_its_mathematics_as_support_geometry() {
    let (data, raw) = source_shaped_plane_brep();
    let brep = crate::brep::ValidatedRawBrep::try_new(raw).expect("source Brep");
    let association = SourceObjectAssociation {
        format: cadmpeg_ir::CodecFormat::Rhino,
        geometry_role: None,
        object_id: cadmpeg_core::nonblank_literal!("failed-brep"),
        name: Some("source name".into()),
        color: None,
        visible: Some(true),
        layer: None,
        instance_path: Vec::new(),
    };
    let unknown = "rhino:object:record#failed".try_into().expect("identity");
    let staged = with_expand_bytes(&data, |expand| {
        stage_brep(BrepTransferInput {
            expand,
            data: &data,
            archive: ArchiveVersion::V5,
            writer_version: Some(200_206_180),
            brep: &brep,
            key: "failed",
            association: &association,
            unknown: &unknown,
            scale: crate::test_support::millimeter_scale(1.0),
            mesh_budget: &mut crate::mesh::MeshBudget::new(),
        })
    })
    .expect("stage source Brep");
    assert!(staged.draft.model().surfaces.iter().all(|surface| {
        surface
            .source_object
            .as_ref()
            .expect("source")
            .geometry_role
            == Some(cadmpeg_ir::SourceGeometryRole::Support)
    }));
    let geometry = staged.draft.model().surfaces[0].geometry.clone();
    let staged = staged.free_carrier_fallback("unreadable trim");
    let retained = &staged.draft.model().surfaces[0];
    assert_eq!(retained.geometry, geometry);
    let source = retained.source_object.as_ref().expect("source metadata");
    assert_eq!(
        source.geometry_role,
        Some(cadmpeg_ir::SourceGeometryRole::Support)
    );
    assert_eq!(source.object_id.as_str(), "failed-brep");
    assert_eq!(source.name.as_deref(), Some("source name"));
    assert_eq!(source.visible, Some(true));
}

#[test]
fn fallback_candidate_links_free_carrier_before_full_ir_validation() {
    let unknown: UnknownId = "rhino:object:record#x".try_into().expect("valid identity");
    let curve_id: cadmpeg_ir::ids::CurveId = "rhino:object:curve#x.c3-0"
        .try_into()
        .expect("valid identity");
    let mut candidate = CadIr::empty();
    candidate
        .set_native_unknowns(
            &cadmpeg_test_support::service_decode_context(),
            "rhino",
            &[NativeUnknownRecord {
                id: unknown.clone(),
                links: Vec::new(),
            }],
        )
        .expect("required invariant");
    let mut staged = BrepDraft {
        kind: BrepTransferKind::FreeCarrierFallback,
        links: vec![unknown.to_string(), curve_id.to_string()],
        ..BrepDraft::default()
    };
    staged.draft.model_mut().curves.push(Curve {
        parameter_range: None,
        id: curve_id.clone(),
        geometry: CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(line_nurbs(0.0, 1.0, false))),
        source_object: None,
    });
    let links = staged.links.clone();
    staged
        .draft
        .commit(
            &mut candidate,
            &mut cadmpeg_ir::Annotations::default(),
            &cadmpeg_test_support::service_decode_context(),
        )
        .unwrap()
        .expect("commit fallback carrier");
    append_record_links(&mut candidate, &unknown, &links);
    assert_eq!(
        candidate
            .native_unknowns("rhino")
            .expect("required invariant")[0]
            .links
            .iter()
            .map(cadmpeg_ir::ids::Identity::as_str)
            .collect::<Vec<_>>(),
        vec![curve_id.to_string()]
    );
    let report = cadmpeg_ir::validate::validate_neutral(&candidate, Vec::new())
        .expect("resource allocation did not fail");
    assert!(report.is_ok(), "{report:?}");
}

#[test]
fn colliding_staged_ids_are_rejected_without_mutating_the_candidate() {
    let curve_id: cadmpeg_ir::ids::CurveId = "rhino:object:curve#x.c3-0"
        .try_into()
        .expect("valid identity");
    let curve = Curve {
        parameter_range: None,
        id: curve_id,
        geometry: CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(line_nurbs(0.0, 1.0, false))),
        source_object: None,
    };
    let mut live = CadIr::empty();
    live.model.curves.push(curve.clone());
    let mut candidate = live.clone();
    let mut staged = BrepDraft::default();
    staged.draft.model_mut().curves.push(curve);
    assert!(staged
        .draft
        .commit(
            &mut candidate,
            &mut cadmpeg_ir::Annotations::default(),
            &cadmpeg_test_support::service_decode_context()
        )
        .unwrap()
        .is_err());
    assert_eq!(candidate, live);
    assert_eq!(live.model.curves.len(), 1);
}

#[test]
fn source_shaped_plane_brep_stages_complete_scaled_valid_ir() {
    let (data, raw) = source_shaped_plane_brep();
    let brep = crate::brep::ValidatedRawBrep::try_new(raw).expect("validate source-shaped Brep");
    let association = SourceObjectAssociation {
        geometry_role: None,
        format: cadmpeg_ir::CodecFormat::Rhino,
        object_id: cadmpeg_core::text::NonBlankString::new("plane-brep".to_string())
            .expect("nonempty source identity"),
        name: Some("plane".to_string()),
        color: None,
        visible: Some(true),
        layer: None,
        instance_path: Vec::new(),
    };
    let unknown: UnknownId = "rhino:object:record#plane"
        .try_into()
        .expect("valid identity");
    let staged = with_expand_bytes(&data, |expand| {
        stage_brep(BrepTransferInput {
            expand,
            data: &data,
            archive: ArchiveVersion::V5,
            writer_version: Some(200_206_180),
            brep: &brep,
            key: "plane",
            association: &association,
            unknown: &unknown,
            scale: crate::test_support::millimeter_scale(25.4),
            mesh_budget: &mut crate::mesh::MeshBudget::new(),
        })
    })
    .expect("stage plane Brep");
    assert_eq!(staged.kind, BrepTransferKind::FullTopology);
    let model = staged.draft.model();
    assert_eq!(
        (
            model.bodies.len(),
            model.regions.len(),
            model.shells.len(),
            model.faces.len(),
            model.loops.len(),
            model.coedges.len(),
            model.edges.len(),
            model.vertices.len(),
            model.pcurves.len(),
            model.curves.len(),
            model.surfaces.len(),
        ),
        (1, 1, 1, 1, 1, 3, 3, 3, 3, 3, 1)
    );
    assert_eq!(model.points[1].position().get().x, 25.4);
    assert_eq!(
        model.vertices[0]
            .tolerance
            .map(cadmpeg_ir::scalar::PositiveReal::get),
        Some(0.254)
    );
    assert_eq!(
        model.edges[0]
            .tolerance
            .map(cadmpeg_ir::scalar::PositiveReal::get),
        Some(0.254)
    );
    assert_eq!(
        model.pcurves[0]
            .fit_tolerance()
            .map(cadmpeg_ir::geometry::FitTolerance::get),
        Some(0.02)
    );
    let PcurveGeometry::Nurbs { nurbs } = &model.pcurves[0].geometry else {
        panic!("line C2 must be a NURBS pcurve");
    };
    // Plane parameters are lengths: the native `u = 1.0` trim endpoint
    // scales with the document (inches -> millimeters).
    assert_eq!(nurbs.control_points()[1].u, 25.4);
    assert_eq!(model.coedges[0].radial_next, model.coedges[0].id);
    let links = staged.links.clone();
    let mut candidate = CadIr::empty();
    candidate
        .set_native_unknowns(
            &cadmpeg_test_support::service_decode_context(),
            "rhino",
            &[NativeUnknownRecord {
                id: unknown.clone(),
                links: Vec::new(),
            }],
        )
        .expect("required invariant");
    staged
        .draft
        .commit(
            &mut candidate,
            &mut cadmpeg_ir::Annotations::default(),
            &cadmpeg_test_support::service_decode_context(),
        )
        .unwrap()
        .expect("commit staged plane B-rep");
    append_record_links(&mut candidate, &unknown, &links);
    let report = cadmpeg_ir::validate::validate_neutral(&candidate, Vec::new())
        .expect("resource allocation did not fail");
    assert!(report.is_ok(), "{report:?}");
}

#[test]
fn isolated_brep_vertices_are_owned_by_the_only_shell() {
    let (data, mut raw) = source_shaped_plane_brep();
    raw.vertices.push(crate::brep::RawBrepVertex {
        index: 3,
        point: crate::settings::CoordinateLane::Admitted(
            crate::test_support::point3([2.0, 2.0, 0.0]).0,
        ),
        edges: Vec::new(),
        tolerance: 0.0,
        source_range: 0..0,
    });
    let brep = crate::brep::ValidatedRawBrep::try_new(raw).expect("validate Brep");
    let association = SourceObjectAssociation {
        geometry_role: None,
        format: cadmpeg_ir::CodecFormat::Rhino,
        object_id: cadmpeg_core::text::NonBlankString::new("free-vertex-brep".to_string())
            .expect("nonempty source identity"),
        name: None,
        color: None,
        visible: None,
        layer: None,
        instance_path: Vec::new(),
    };
    let unknown: UnknownId = "rhino:object:record#free-vertex"
        .try_into()
        .expect("valid identity");
    let staged = with_expand_bytes(&data, |expand| {
        stage_brep(BrepTransferInput {
            expand,
            data: &data,
            archive: ArchiveVersion::V5,
            writer_version: Some(200_206_180),
            brep: &brep,
            key: "free-vertex",
            association: &association,
            unknown: &unknown,
            scale: MillimeterScale::IDENTITY,
            mesh_budget: &mut crate::mesh::MeshBudget::new(),
        })
    })
    .expect("stage Brep with an isolated vertex");
    assert_eq!(staged.kind, BrepTransferKind::FullTopology);
    assert_eq!(
        staged.draft.model().shells[0].free_vertices(),
        vec!["rhino:object:vertex#free-vertex.slot-3"
            .try_into()
            .expect("valid identity")]
    );

    let mut candidate = CadIr::empty();
    candidate
        .set_native_unknowns(
            &cadmpeg_test_support::service_decode_context(),
            "rhino",
            &[NativeUnknownRecord {
                id: unknown,
                links: Vec::new(),
            }],
        )
        .expect("required invariant");
    staged
        .draft
        .commit(
            &mut candidate,
            &mut cadmpeg_ir::Annotations::default(),
            &cadmpeg_test_support::service_decode_context(),
        )
        .unwrap()
        .expect("commit Brep with an isolated vertex");
    let report = cadmpeg_ir::validate::validate_neutral(&candidate, Vec::new())
        .expect("resource allocation did not fail");
    assert!(report.is_ok(), "{report:?}");
}

#[test]
fn failed_trim_pcurve_does_not_discard_brep_topology() {
    let (mut data, raw) = source_shaped_plane_brep();
    let pcurve = raw.c2.slots[1].as_ref().expect("C2 slot");
    data[pcurve.class_data_range.start] = 0;
    let brep = crate::brep::ValidatedRawBrep::try_new(raw).expect("validate source-shaped Brep");
    let association = SourceObjectAssociation {
        geometry_role: None,
        format: cadmpeg_ir::CodecFormat::Rhino,
        object_id: cadmpeg_core::text::NonBlankString::new("plane-brep".to_string())
            .expect("nonempty source identity"),
        name: None,
        color: None,
        visible: None,
        layer: None,
        instance_path: Vec::new(),
    };
    let unknown: UnknownId = "rhino:object:record#plane"
        .try_into()
        .expect("valid identity");
    let staged = with_expand_bytes(&data, |expand| {
        stage_brep(BrepTransferInput {
            expand,
            data: &data,
            archive: ArchiveVersion::V5,
            writer_version: Some(200_206_180),
            brep: &brep,
            key: "plane",
            association: &association,
            unknown: &unknown,
            scale: MillimeterScale::IDENTITY,
            mesh_budget: &mut crate::mesh::MeshBudget::new(),
        })
    })
    .expect("stage Brep without one pcurve");
    assert_eq!(staged.kind, BrepTransferKind::FullTopology);
    assert_eq!(staged.draft.model().pcurves.len(), 2);
    assert!(staged
        .warnings
        .iter()
        .any(|warning| warning.contains("trim 1 C2 omitted")));
}

#[test]
fn disconnected_incidence_produces_deterministic_shell_groups() {
    let grouping = with_expand_bytes(&[], |expand| {
        region_shell_groups_without_records(expand.ctx(), &[1, 0, 1, 0])
            .expect("shell-group allocation")
    });
    assert!(grouping.fallback);
    assert_eq!(grouping.face_groups, vec![1, 0, 1, 0]);
    assert_eq!(
        grouping
            .shells
            .iter()
            .map(|shell| shell.region)
            .collect::<Vec<_>>(),
        vec![0, 1]
    );
    assert_eq!(
        grouping
            .shells
            .iter()
            .map(|shell| shell.faces.clone())
            .collect::<Vec<_>>(),
        vec![vec![1, 3], vec![0, 2]]
    );
}

#[test]
fn face_components_label_an_isolated_face() {
    let resolved = crate::brep::ResolvedBrep {
        faces: vec![crate::brep::ResolvedFace {
            surface: 0,
            loops: Vec::new(),
        }],
        ..crate::brep::ResolvedBrep::default()
    };
    assert_eq!(
        with_expand_bytes(&[], |expand| face_components(expand.ctx(), &resolved))
            .expect("service profile admits face components"),
        vec![0]
    );
}

fn connected_face_fixture() -> crate::brep::ResolvedBrep {
    let tolerance = crate::brep::BrepTolerance::new(0.0).expect("valid tolerance");
    crate::brep::ResolvedBrep {
        edges: vec![crate::brep::ResolvedEdge {
            curve: 0,
            vertices: [0, 0],
            trims: vec![0],
            tolerance,
        }],
        trims: vec![crate::brep::ResolvedTrim {
            curve: None,
            edge: Some(0),
            vertices: [0, 0],
            loop_index: 0,
            tolerances: [tolerance; 2],
        }],
        loops: vec![crate::brep::ResolvedLoop {
            trims: vec![0],
            face: 0,
        }],
        faces: vec![crate::brep::ResolvedFace {
            surface: 0,
            loops: vec![0],
        }],
        ..crate::brep::ResolvedBrep::default()
    }
}

#[test]
fn face_components_label_a_connected_face() {
    let resolved = connected_face_fixture();
    assert_eq!(
        with_expand_bytes(&[], |expand| face_components(expand.ctx(), &resolved))
            .expect("service profile admits one connected face"),
        vec![0]
    );
}

#[test]
fn brep_free_vertex_indices_list_an_isolated_vertex() {
    let resolved = crate::brep::ResolvedBrep {
        vertices: vec![crate::brep::ResolvedVertex {
            edges: Vec::new(),
            tolerance: crate::brep::BrepTolerance::new(0.0).expect("valid tolerance"),
        }],
        ..crate::brep::ResolvedBrep::default()
    };
    assert_eq!(
        with_expand_bytes(&[], |expand| brep_free_vertex_indices(
            expand.ctx(),
            &resolved
        ))
        .expect("service profile admits one flag"),
        vec![0]
    );
}

#[test]
fn brep_mesh_cache_stages_one_tessellation() {
    let (mut data, mut raw) = source_shaped_plane_brep();
    let mesh_start = data.len();
    data.push(0x30);
    data.extend(3_i32.to_le_bytes());
    data.extend(1_i32.to_le_bytes());
    for _ in 0..4 {
        data.extend(0.0_f64.to_le_bytes());
        data.extend(1.0_f64.to_le_bytes());
    }
    data.extend([0; 16]);
    data.extend([0; 64]);
    data.extend(0_i32.to_le_bytes());
    data.extend([0; 5]);
    data.extend(1_i32.to_le_bytes());
    data.extend([0, 1, 2, 2]);
    let vertex_bytes = [0.0_f32, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0]
        .into_iter()
        .flat_map(f32::to_le_bytes)
        .collect::<Vec<_>>();
    data.extend(
        u32::try_from(vertex_bytes.len())
            .expect("vertex size")
            .to_le_bytes(),
    );
    data.extend(crc32fast::hash(&vertex_bytes).to_le_bytes());
    data.push(0);
    data.extend(vertex_bytes);
    for _ in 0..4 {
        data.extend(0_u32.to_le_bytes());
    }
    raw.render_meshes.push(Some(crate::brep::RawBrepMesh {
        mesh: crate::brep::RawBrepChild {
            class_uuid: crate::mesh::ON_MESH,
            class_data_range: mesh_start..data.len(),
            source_range: mesh_start..data.len(),
        },
        userdata: Vec::new(),
    }));
    let brep = crate::brep::ValidatedRawBrep::try_new(raw)
        .expect("validate Brep with one mesh cache slot");
    let association = SourceObjectAssociation {
        geometry_role: None,
        format: cadmpeg_ir::CodecFormat::Rhino,
        object_id: cadmpeg_core::text::NonBlankString::new("plane-brep".to_string())
            .expect("nonempty source identity"),
        name: Some("plane".to_string()),
        color: None,
        visible: Some(true),
        layer: None,
        instance_path: Vec::new(),
    };
    let unknown: UnknownId = "rhino:object:record#plane"
        .try_into()
        .expect("valid identity");
    let staged = with_expand_bytes(&data, |expand| {
        stage_brep(BrepTransferInput {
            expand,
            data: &data,
            archive: ArchiveVersion::V5,
            writer_version: Some(200_206_180),
            brep: &brep,
            key: "plane",
            association: &association,
            unknown: &unknown,
            scale: MillimeterScale::IDENTITY,
            mesh_budget: &mut crate::mesh::MeshBudget::new(),
        })
    })
    .expect("mesh cache fits service limits");
    assert_eq!(staged.draft.model().tessellations.len(), 1);
}

#[test]
fn reused_brep_c2_curve_decodes_every_pcurve() {
    let (data, mut raw) = source_shaped_plane_brep();
    raw.trims[1].curve = Some(0);
    let brep =
        crate::brep::ValidatedRawBrep::try_new(raw).expect("validate Brep with a shared C2 slot");
    let success = with_expand_bytes(&data, |expand| {
        decode_pcurves(
            expand.ctx(),
            &data,
            ArchiveVersion::V5,
            brep.raw(),
            brep.resolved(),
            "plane",
            &std::collections::HashMap::new(),
        )
    })
    .expect("shared C2 slot decodes under the service profile");
    assert_eq!(success.values.len(), 3);
}

/// A dropped Brep display-mesh cache slot carries the mesh-cache code itself.
#[test]
fn a_dropped_brep_mesh_cache_slot_carries_the_mesh_cache_code() {
    let mut staged = BrepDraft::default();
    staged.mesh_cache_slot_dropped("render", 2, &"payload is truncated");
    assert_eq!(
        staged
            .warnings
            .iter()
            .map(|diagnostic| (diagnostic.code, diagnostic.message.as_str()))
            .collect::<Vec<_>>(),
        [(
            Some(RhinoLossCode::BrepMeshCacheDegraded),
            "invalid render mesh cache slot 2: payload is truncated"
        )]
    );
}

#[test]
fn fallback_keeps_thousands_of_free_carriers() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
    let arena = DecodeArena::new();
    let policy = DecodePolicy::service();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut staged = BrepDraft::default();
    for index in 0..4096 {
        let id: cadmpeg_ir::ids::CurveId =
            format!("rhino:object:curve#{index:08}").try_into().unwrap();
        staged.draft.model_mut().curves.push(Curve {
            id: id.clone(),
            parameter_range: None,
            geometry: CurveGeometry::Solved(SolvedCurveGeometry::Line(
                cadmpeg_ir::geometry::analytic::LineCurve::try_new(
                    cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
                    cadmpeg_ir::math::Vector3::new(1.0, 0.0, 0.0),
                )
                .unwrap(),
            )),
            source_object: None,
        });
        staged.links.push(id.to_string());
        staged.draft.exactness(&id, cadmpeg_ir::Exactness::Derived);
    }
    let staged = staged.free_carrier_fallback("synthetic topology rejection");
    assert_eq!(staged.draft.model().curves.len(), 4096);
    assert_eq!(staged.links.len(), 4096);
    ctx.finish_session().unwrap();
}
