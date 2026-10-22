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
    super::super::scale_plane_pcurves(&mut staged, scale).expect("scale plane pcurves");
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
        .expect("validate the neutral model");
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
        .expect("validate the neutral model");
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
        .expect("validate the neutral model");
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
        region_shell_groups_without_records(expand.ctx(), &[1, 0, 1, 0], true)
            .expect("shell-group allocation")
    });
    assert!(grouping.fallback);
    assert!(!grouping.serialized_regions);
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
            .expect("compute the face components"),
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
            .expect("compute the face components"),
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
        .expect("compute the B-rep free vertex indices"),
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
    .expect("decode pcurves");
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

/// Two triangles share edge 1. Face 1 lies on a plane whose normal is `-z`
/// and is reversed, so both face normals point along `+z`. Every loop runs
/// counterclockwise in its plane's parameter space, as `ON_Brep` requires,
/// and the shared edge satisfies the `ON_Brep::IsManifold` orientation rule:
/// `reversed 3D XOR reversed surface` differs between its two trims.
fn two_face_shared_edge_brep() -> (Vec<u8>, crate::brep::RawBrep) {
    let line_uuid = super::class_uuid([
        0xdb, 0xd4, 0xd7, 0x4e, 0x47, 0xe9, 0xd3, 0x11, 0xbf, 0xe5, 0x00, 0x10, 0x83, 0x01, 0x22,
        0xf0,
    ]);
    let plane_uuid = super::class_uuid([
        0xdf, 0xd4, 0xd7, 0x4e, 0x47, 0xe9, 0xd3, 0x11, 0xbf, 0xe5, 0x00, 0x10, 0x83, 0x01, 0x22,
        0xf0,
    ]);
    let points = [
        [0.0, 0.0, 0.0],
        [1.0, 0.0, 0.0],
        [0.0, 1.0, 0.0],
        [1.0, 1.0, 0.0],
    ];
    // Face 1 parameters are `(x, -y)`.
    let uv = |face: usize, vertex: usize| {
        let [x, y, _] = points[vertex];
        if face == 0 {
            [x, y, 0.0]
        } else {
            [x, -y, 0.0]
        }
    };
    let edge_vertices = [[0, 1], [1, 2], [2, 0], [2, 3], [3, 1]];
    // (face, edge, trim start vertex, trim end vertex), loop order.
    let trim_rows = [
        (0, 0, 0, 1),
        (0, 1, 1, 2),
        (0, 2, 2, 0),
        (1, 1, 1, 2),
        (1, 3, 2, 3),
        (1, 4, 3, 1),
    ];
    let mut data = Vec::new();
    let c3_ranges = edge_vertices
        .iter()
        .map(|[start, end]| super::append_line_payload(&mut data, points[*start], points[*end], 3))
        .collect::<Vec<_>>();
    let c2_ranges = trim_rows
        .iter()
        .map(|(face, _, start, end)| {
            super::append_line_payload(&mut data, uv(*face, *start), uv(*face, *end), 2)
        })
        .collect::<Vec<_>>();
    let mut surface_ranges = Vec::new();
    for (y_axis, z_axis) in [
        ([0.0, 1.0, 0.0], [0.0, 0.0, 1.0]),
        ([0.0, -1.0, 0.0], [0.0, 0.0, -1.0]),
    ] {
        let start = data.len();
        data.push(0x11);
        for value in [0.0_f64, 0.0, 0.0, 1.0, 0.0, 0.0]
            .into_iter()
            .chain(y_axis)
            .chain(z_axis)
            .chain([z_axis[0], z_axis[1], z_axis[2], 0.0])
        {
            data.extend_from_slice(&value.to_le_bytes());
        }
        for _ in 0..4 {
            for value in [-2.0_f64, 2.0] {
                data.extend_from_slice(&value.to_le_bytes());
            }
        }
        surface_ranges.push(start..data.len());
    }
    let interval = super::finite_interval([0.0, 1.0]);
    let index = |value: usize| i32::try_from(value).expect("index");
    let vertices = (0..points.len())
        .map(|vertex| crate::brep::RawBrepVertex {
            index: index(vertex),
            point: crate::settings::CoordinateLane::Admitted(
                crate::test_support::point3(points[vertex]).0,
            ),
            edges: edge_vertices
                .iter()
                .enumerate()
                .filter(|(_, ends)| ends.contains(&vertex))
                .map(|(edge, _)| index(edge))
                .collect(),
            tolerance: 0.0,
            source_range: 0..0,
        })
        .collect();
    let edges = edge_vertices
        .iter()
        .enumerate()
        .map(|(edge, ends)| crate::brep::RawBrepEdge {
            index: index(edge),
            curve: index(edge),
            proxy_reversed: false,
            proxy_domain: interval,
            vertices: [index(ends[0]), index(ends[1])],
            trims: trim_rows
                .iter()
                .enumerate()
                .filter(|(_, row)| row.1 == edge)
                .map(|(trim, _)| index(trim))
                .collect(),
            tolerance: 0.0,
            domain: interval,
            source_range: 0..0,
        })
        .collect();
    let trims = trim_rows
        .iter()
        .enumerate()
        .map(
            |(trim, (face, edge, start, end))| crate::brep::RawBrepTrim {
                index: index(trim),
                curve: Some(index(trim)),
                proxy_domain: interval,
                edge: Some(index(*edge)),
                vertices: [index(*start), index(*end)],
                reversed_3d: edge_vertices[*edge] != [*start, *end],
                trim_type: if *edge == 1 {
                    crate::brep::RawTrimKind::Mated
                } else {
                    crate::brep::RawTrimKind::Boundary
                },
                iso: crate::brep::RawTrimIso::None,
                loop_index: index(*face),
                tolerances: [0.0, 0.0],
                domain: interval,
                proxy_reversed: false,
                reserved: Vec::new(),
                legacy_tolerances: [0.0, 0.0],
                source_range: 0..0,
            },
        )
        .collect();
    let children =
        |ranges: Vec<std::ops::Range<usize>>, uuid, expected_type| crate::brep::RawBrepChildren {
            slots: ranges
                .into_iter()
                .map(|range| Some(super::child(uuid, range)))
                .collect(),
            source_range: 0..0,
            expected_type,
        };
    (
        data,
        crate::brep::RawBrep {
            losses: Vec::new(),
            minor: 2,
            c2: children(c2_ranges, line_uuid, crate::brep::RawBrepBaseType::Curve),
            c3: children(c3_ranges, line_uuid, crate::brep::RawBrepBaseType::Curve),
            surfaces: children(
                surface_ranges,
                plane_uuid,
                crate::brep::RawBrepBaseType::Surface,
            ),
            vertices,
            edges,
            trims,
            loops: (0..2)
                .map(|face| crate::brep::RawBrepLoop {
                    index: index(face),
                    trims: (face * 3..face * 3 + 3).map(index).collect(),
                    loop_type: crate::brep::RawLoopKind::Outer,
                    face: index(face),
                    source_range: 0..0,
                })
                .collect(),
            faces: (0..2)
                .map(|face| crate::brep::RawBrepFace {
                    index: index(face),
                    loops: vec![index(face)],
                    surface: index(face),
                    reversed_surface: face == 1,
                    material_channel: 0,
                    uuid: None,
                    color: None,
                    source_range: 0..0,
                })
                .collect(),
            bounds: crate::settings::BoundingBox {
                minimum: crate::test_support::point3([0.0, 0.0, 0.0]),
                maximum: crate::test_support::point3([1.0, 1.0, 0.0]),
            },
            render_meshes: Vec::new(),
            analysis_meshes: Vec::new(),
            is_solid: crate::brep::RawSolidFlag::Known(crate::brep::SolidState::NotSolid),
            face_sides: Vec::new(),
            regions: Vec::new(),
            source_range: 0..0,
        },
    )
}

/// The neutral loop of a reversed face runs about the face normal: its trims
/// are traversed backwards with flipped senses, so an oriented shared edge has
/// opposite coedge senses and every loop stays endpoint-continuous.
#[test]
fn reversed_face_loop_runs_about_the_face_normal() {
    let (data, raw) = two_face_shared_edge_brep();
    let brep = crate::brep::ValidatedRawBrep::try_new(raw).expect("validate two-face Brep");
    let association = SourceObjectAssociation {
        geometry_role: None,
        format: cadmpeg_ir::CodecFormat::Rhino,
        object_id: cadmpeg_core::nonblank_literal!("two-face"),
        name: None,
        color: None,
        visible: None,
        layer: None,
        instance_path: Vec::new(),
    };
    let unknown: UnknownId = "rhino:object:record#two-face"
        .try_into()
        .expect("valid identity");
    let staged = with_expand_bytes(&data, |expand| {
        stage_brep(BrepTransferInput {
            expand,
            data: &data,
            archive: ArchiveVersion::V5,
            writer_version: Some(200_206_180),
            brep: &brep,
            key: "two-face",
            association: &association,
            unknown: &unknown,
            scale: MillimeterScale::IDENTITY,
            mesh_budget: &mut crate::mesh::MeshBudget::new(),
        })
    })
    .expect("stage two-face Brep");
    assert_eq!(staged.kind, BrepTransferKind::FullTopology);
    let model = staged.draft.model();
    let coedge = |trim: usize| {
        model
            .coedges
            .iter()
            .find(|coedge| coedge.id.as_str().ends_with(&format!(".slot-{trim}")))
            .expect("coedge for trim")
    };
    let loop_coedges = |face: usize| {
        model.loops[face]
            .coedges()
            .iter()
            .map(|id| id.as_str().rsplit('-').next().expect("slot").to_owned())
            .collect::<Vec<_>>()
    };
    assert_eq!(loop_coedges(0), ["0", "1", "2"]);
    assert_eq!(loop_coedges(1), ["5", "4", "3"]);
    assert_eq!(coedge(1).sense, cadmpeg_ir::topology::Sense::Forward);
    assert_eq!(coedge(3).sense, cadmpeg_ir::topology::Sense::Reversed);
    assert_eq!(coedge(1).radial_next, coedge(3).id);
    assert_eq!(coedge(3).radial_next, coedge(1).id);
    for trim in [4, 5] {
        assert_eq!(coedge(trim).sense, cadmpeg_ir::topology::Sense::Reversed);
    }
    let mut candidate = CadIr::empty();
    candidate.model = model.clone();
    let report = cadmpeg_ir::validate::validate_neutral(&candidate, Vec::new())
        .expect("validate the neutral model");
    assert!(
        !report.findings.iter().any(|finding| matches!(
            finding.check,
            cadmpeg_ir::report::check::Check::CoedgePairing
                | cadmpeg_ir::report::check::Check::GeometricConsistency
        )),
        "{report:?}"
    );
    assert!(super::super::brep_topology_rejection(
        &cadmpeg_test_support::service_decode_context(),
        model,
        candidate.tolerances,
    )
    .expect("validate staged Brep")
    .is_none());
}
