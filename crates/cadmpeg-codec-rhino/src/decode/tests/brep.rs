// SPDX-License-Identifier: Apache-2.0

use super::super::decode_pcurves;
use super::{
    append_record_links, brep_free_vertex_indices, face_components, line_nurbs, region, region_raw,
    region_resolved, region_shell_groups, region_shell_groups_without_records,
    source_shaped_plane_brep, stage_brep, with_collection_limit, with_expand_bytes, ArchiveVersion,
    Body, BodyKind, BrepDraft, BrepTransferInput, BrepTransferKind, CadIr, Curve, CurveGeometry,
    MillimeterScale, NativeUnknownRecord, PcurveGeometry, SolvedCurveGeometry,
    SolvedSurfaceGeometry, SourceObjectAssociation, Surface, UnknownId,
};

#[test]
fn plane_pcurve_lookup_set_refuses_collection_limit() {
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
        geometry: cadmpeg_ir::geometry::SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(plane)),
        source_object: None,
    });
    let scale = crate::test_support::millimeter_scale(25.4);
    let refusal = with_collection_limit(0, |ctx| {
        super::super::scale_plane_pcurves(ctx, &mut staged, scale)
            .expect_err("plane ID requires a lookup set node")
    });
    assert!(matches!(
        refusal,
        crate::curves::GeometryError::Codec(cadmpeg_core::CodecError::ResourceLimit(limit))
            if limit.operation == "Rhino plane pcurve lookup IDs"
    ));
    super::super::scale_plane_pcurves(&cadmpeg_test_support::service_decode_context(), &mut staged, scale)
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
    staged = staged
        .free_carrier_fallback(&cadmpeg_test_support::service_decode_context(), "C2 failure")
        .expect("service profile admits fallback IDs and warning");
    assert_eq!(staged.kind, BrepTransferKind::FreeCarrierFallback);
    assert!(staged.draft.model().bodies.is_empty());
    assert_eq!(
        staged.links,
        vec![curve_id.to_string(), surface_id.to_string()]
    );
    assert!(staged.warnings.iter().any(|warning| warning.contains("C2")));
}

#[test]
fn brep_fallback_set_refuses_collection_limit_before_insertion() {
    let curve_id: cadmpeg_ir::ids::CurveId = "rhino:object:curve#fallback"
        .try_into()
        .expect("valid identity");
    let mut staged = BrepDraft::default();
    staged.draft.model_mut().curves.push(Curve {
        id: curve_id,
        geometry: CurveGeometry::Solved(SolvedCurveGeometry::Unknown { record: None }),
        source_object: None,
    });
    let error = with_collection_limit(0, |ctx| {
        staged.free_carrier_fallback(ctx, "invalid topology")
    })
    .expect_err("emitted fallback ID requires one collection item");
    assert!(matches!(
        error,
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.operation == "Rhino Brep emitted fallback IDs"
    ));
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
        id: curve_id.clone(),
        geometry: CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(line_nurbs(0.0, 1.0, false))),
        source_object: None,
    });
    let links = staged.links.clone();
    staged
        .draft.commit(&mut candidate, &mut cadmpeg_ir::Annotations::default())
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
    let report = cadmpeg_ir::validate::validate_neutral(&candidate, Vec::new());
    assert!(report.is_ok(), "{report:?}");
}

#[test]
fn colliding_staged_ids_are_rejected_without_mutating_the_candidate() {
    let curve_id: cadmpeg_ir::ids::CurveId = "rhino:object:curve#x.c3-0"
        .try_into()
        .expect("valid identity");
    let curve = Curve {
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
        .draft.commit(&mut candidate, &mut cadmpeg_ir::Annotations::default())
        .is_err());
    assert_eq!(candidate, live);
    assert_eq!(live.model.curves.len(), 1);
}

#[test]
fn source_shaped_plane_brep_stages_complete_scaled_valid_ir() {
    let (data, raw) = source_shaped_plane_brep();
    let brep = with_expand_bytes(&data, |expand| {
        crate::brep::ValidatedRawBrep::try_new(expand.ctx(), raw)
    })
    .expect("validate source-shaped Brep");
    let association = SourceObjectAssociation {
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
            "rhino",
            &[NativeUnknownRecord {
                id: unknown.clone(),
                links: Vec::new(),
            }],
        )
        .expect("required invariant");
    staged
        .draft.commit(&mut candidate, &mut cadmpeg_ir::Annotations::default())
        .expect("commit staged plane B-rep");
    append_record_links(&mut candidate, &unknown, &links);
    let report = cadmpeg_ir::validate::validate_neutral(&candidate, Vec::new());
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
    let brep = with_expand_bytes(&data, |expand| {
        crate::brep::ValidatedRawBrep::try_new(expand.ctx(), raw)
    })
    .expect("validate Brep");
    let association = SourceObjectAssociation {
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
            "rhino",
            &[NativeUnknownRecord {
                id: unknown,
                links: Vec::new(),
            }],
        )
        .expect("required invariant");
    staged
        .draft.commit(&mut candidate, &mut cadmpeg_ir::Annotations::default())
        .expect("commit Brep with an isolated vertex");
    let report = cadmpeg_ir::validate::validate_neutral(&candidate, Vec::new());
    assert!(report.is_ok(), "{report:?}");
}

#[test]
fn failed_trim_pcurve_does_not_discard_brep_topology() {
    let (mut data, raw) = source_shaped_plane_brep();
    let pcurve = raw.c2.slots[1].as_ref().expect("C2 slot");
    data[pcurve.class_data_range.start] = 0;
    let brep = with_expand_bytes(&data, |expand| {
        crate::brep::ValidatedRawBrep::try_new(expand.ctx(), raw)
    })
    .expect("validate source-shaped Brep");
    let association = SourceObjectAssociation {
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
fn face_components_refuse_collection_limit_before_parent_allocation() {
    let resolved = crate::brep::ResolvedBrep {
        faces: vec![crate::brep::ResolvedFace {
            surface: 0,
            loops: Vec::new(),
        }],
        ..crate::brep::ResolvedBrep::default()
    };
    let error = with_collection_limit(0, |ctx| face_components(ctx, &resolved))
        .expect_err("one face parent exceeds zero collection items");
    assert!(matches!(
        error,
        crate::curves::GeometryError::Codec(cadmpeg_core::CodecError::ResourceLimit(limit))
            if limit.operation == "Rhino Brep face parents"
    ));
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
fn face_components_edge_faces_refuse_collection_limit() {
    let resolved = connected_face_fixture();
    let error = with_collection_limit(1, |ctx| face_components(ctx, &resolved))
        .expect_err("parent and edge-face items exceed one collection item");
    assert!(matches!(
        error,
        crate::curves::GeometryError::Codec(cadmpeg_core::CodecError::ResourceLimit(limit))
            if limit.operation == "Rhino Brep edge faces"
    ));
}

#[test]
fn face_components_roots_refuse_collection_limit() {
    let resolved = connected_face_fixture();
    let error = with_collection_limit(2, |ctx| face_components(ctx, &resolved))
        .expect_err("face root exceeds two collection items");
    assert!(matches!(
        error,
        crate::curves::GeometryError::Codec(cadmpeg_core::CodecError::ResourceLimit(limit))
            if limit.operation == "Rhino Brep face roots"
    ));
}

#[test]
fn face_components_labels_refuse_collection_limit() {
    let resolved = connected_face_fixture();
    let error = with_collection_limit(3, |ctx| face_components(ctx, &resolved))
        .expect_err("face label exceeds three collection items");
    assert!(matches!(
        error,
        crate::curves::GeometryError::Codec(cadmpeg_core::CodecError::ResourceLimit(limit))
            if limit.operation == "Rhino Brep face labels"
    ));
}

#[test]
fn face_components_result_refuses_collection_limit() {
    let resolved = connected_face_fixture();
    let error = with_collection_limit(4, |ctx| face_components(ctx, &resolved))
        .expect_err("component result exceeds four collection items");
    assert!(matches!(
        error,
        crate::curves::GeometryError::Codec(cadmpeg_core::CodecError::ResourceLimit(limit))
            if limit.operation == "Rhino Brep face components"
    ));
    assert_eq!(
        with_expand_bytes(&[], |expand| face_components(expand.ctx(), &resolved))
            .expect("service profile admits one connected face"),
        vec![0]
    );
}

#[test]
fn shell_group_slots_refuse_collection_limit_before_allocation() {
    let Err(error) = with_collection_limit(3, |ctx| {
        region_shell_groups_without_records(ctx, &[1, 0, 1, 0])
    }) else {
        panic!("four face-group slots exceed the limit of three");
    };
    assert!(matches!(
        error,
        crate::curves::GeometryError::Codec(cadmpeg_core::CodecError::ResourceLimit(limit))
            if limit.operation == "Rhino Brep incidence face groups"
    ));
}

#[test]
fn brep_free_vertex_flags_refuse_collection_limit() {
    let resolved = crate::brep::ResolvedBrep {
        vertices: vec![crate::brep::ResolvedVertex {
            edges: Vec::new(),
            tolerance: crate::brep::BrepTolerance::new(0.0).expect("valid tolerance"),
        }],
        ..crate::brep::ResolvedBrep::default()
    };
    let error = with_collection_limit(0, |ctx| brep_free_vertex_indices(ctx, &resolved))
        .expect_err("one attachment flag exceeds zero collection items");
    assert!(matches!(
        error,
        crate::curves::GeometryError::Codec(cadmpeg_core::CodecError::ResourceLimit(limit))
            if limit.operation == "Rhino Brep free-vertex attachment flags"
    ));
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
fn brep_free_vertices_refuse_collection_limit() {
    let resolved = crate::brep::ResolvedBrep {
        vertices: vec![crate::brep::ResolvedVertex {
            edges: Vec::new(),
            tolerance: crate::brep::BrepTolerance::new(0.0).expect("valid tolerance"),
        }],
        ..crate::brep::ResolvedBrep::default()
    };
    let error = with_collection_limit(1, |ctx| brep_free_vertex_indices(ctx, &resolved))
        .expect_err("free-vertex output exceeds one collection item after its flag");
    assert!(matches!(
        error,
        crate::curves::GeometryError::Codec(cadmpeg_core::CodecError::ResourceLimit(limit))
            if limit.operation == "Rhino Brep free vertices"
    ));
}

#[test]
fn brep_fallback_face_groups_refuse_collection_limit() {
    let raw = region_raw(Vec::new(), Vec::new());
    let resolved = region_resolved(&raw);
    let Err(error) =
        with_collection_limit(0, |ctx| region_shell_groups(ctx, &raw, &resolved, &[0]))
    else {
        panic!("one fallback face group exceeds zero collection items");
    };
    assert!(matches!(
        error,
        crate::curves::GeometryError::Codec(cadmpeg_core::CodecError::ResourceLimit(limit))
            if limit.operation == "Rhino Brep fallback face groups"
    ));
    assert!(with_expand_bytes(&[], |expand| {
        region_shell_groups(expand.ctx(), &raw, &resolved, &[0])
    })
    .is_ok());
}

#[test]
fn brep_shell_group_keys_refuse_collection_limit() {
    let raw = region_raw(Vec::new(), Vec::new());
    let resolved = region_resolved(&raw);
    let Err(error) =
        with_collection_limit(1, |ctx| region_shell_groups(ctx, &raw, &resolved, &[0]))
    else {
        panic!("one face group and one group key exceed one collection item");
    };
    assert!(matches!(
        error,
        crate::curves::GeometryError::Codec(cadmpeg_core::CodecError::ResourceLimit(limit))
            if limit.operation == "Rhino Brep shell group keys"
    ));
}

#[test]
fn brep_shell_group_faces_refuse_collection_limit() {
    let raw = region_raw(Vec::new(), Vec::new());
    let resolved = region_resolved(&raw);
    let Err(error) =
        with_collection_limit(2, |ctx| region_shell_groups(ctx, &raw, &resolved, &[0]))
    else {
        panic!("one face group, key and face exceed two collection items");
    };
    assert!(matches!(
        error,
        crate::curves::GeometryError::Codec(cadmpeg_core::CodecError::ResourceLimit(limit))
            if limit.operation == "Rhino Brep shell group faces"
    ));
}

#[test]
fn brep_shell_groups_refuse_collection_limit() {
    let raw = region_raw(Vec::new(), Vec::new());
    let resolved = region_resolved(&raw);
    let Err(error) =
        with_collection_limit(4, |ctx| region_shell_groups(ctx, &raw, &resolved, &[0]))
    else {
        panic!("one face group, key, face, ordered group and shell exceed four collection items");
    };
    assert!(matches!(
        error,
        crate::curves::GeometryError::Codec(cadmpeg_core::CodecError::ResourceLimit(limit))
            if limit.operation == "Rhino Brep shell groups"
    ));
}

#[test]
fn brep_ordered_shell_groups_refuse_collection_limit() {
    let raw = region_raw(Vec::new(), Vec::new());
    let resolved = region_resolved(&raw);
    let Err(error) =
        with_collection_limit(3, |ctx| region_shell_groups(ctx, &raw, &resolved, &[0]))
    else {
        panic!("ordered group exceeds three collection items");
    };
    assert!(matches!(
        error,
        crate::curves::GeometryError::Codec(cadmpeg_core::CodecError::ResourceLimit(limit))
            if limit.operation == "Rhino Brep ordered shell groups"
    ));
}

#[test]
fn brep_region_face_groups_refuse_collection_limit() {
    let raw = region_raw(
        vec![crate::brep::RawBrepFaceSide {
            index: 0,
            region: 0,
            face: 0,
            direction: 1,
            source_range: 0..0,
        }],
        vec![region(1)],
    );
    let resolved = region_resolved(&raw);
    let Err(error) =
        with_collection_limit(0, |ctx| region_shell_groups(ctx, &raw, &resolved, &[0]))
    else {
        panic!("one region face group exceeds zero collection items");
    };
    assert!(matches!(
        error,
        crate::curves::GeometryError::Codec(cadmpeg_core::CodecError::ResourceLimit(limit))
            if limit.operation == "Rhino Brep region face groups"
    ));
    assert!(with_expand_bytes(&[], |expand| {
        region_shell_groups(expand.ctx(), &raw, &resolved, &[0])
    })
    .is_ok());
}

#[test]
fn staged_brep_collections_refuse_just_below_each_required_count() {
    let (data, raw) = source_shaped_plane_brep();
    let brep = with_expand_bytes(&data, |expand| {
        crate::brep::ValidatedRawBrep::try_new(expand.ctx(), raw)
    })
    .expect("validate source-shaped Brep");
    let association = SourceObjectAssociation {
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
    let expected = [
        "Rhino Brep C3 slots",
        "Rhino Brep surface slots",
        "Rhino Brep pcurve IDs",
        "Rhino Brep decoded C2 slots",
        "Rhino Brep cached C2 curve",
        "Rhino Brep pcurve poles",
        "Rhino Brep pcurve knots",
        "Rhino Brep pcurves",
        "Rhino staged Brep vertex IDs",
        "Rhino staged Brep points",
        "Rhino staged Brep vertices",
        "Rhino staged Brep edge IDs",
        "Rhino staged Brep edges",
        "Rhino staged Brep face IDs",
        "Rhino staged Brep pending faces",
        "Rhino staged Brep faces",
        "Rhino staged Brep face loop lists",
        "Rhino staged Brep coedge positions",
        "Rhino staged Brep loops",
        "Rhino staged Brep coedges",
        "Rhino staged Brep loop coedges",
        "Rhino staged Brep coedge pcurves",
        "Rhino staged Brep face loops",
        "Rhino staged Brep shells",
        "Rhino staged Brep shell faces",
        "Rhino staged Brep regions",
        "Rhino staged Brep region shells",
        "Rhino staged Brep body regions",
        "Rhino staged Brep bodies",
        "Rhino staged Brep links",
        "Rhino staged Brep derived IDs",
    ];
    let mut witnessed = std::collections::BTreeSet::new();
    for limit in 0..1024_u64 {
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let mut policy = cadmpeg_core::decode::DecodePolicy::service();
        policy.limits.max_collection_items = limit;
        let (ctx, root) =
            cadmpeg_core::decode::DecodeContext::from_root_bytes(&data, &arena, &policy)
                .expect("source bytes fit the root limit");
        let result = stage_brep(BrepTransferInput {
            expand: crate::mesh::MeshExpand::new(&ctx, root),
            data: &data,
            archive: ArchiveVersion::V5,
            writer_version: Some(200_206_180),
            brep: &brep,
            key: "plane",
            association: &association,
            unknown: &unknown,
            scale: crate::test_support::millimeter_scale(25.4),
            mesh_budget: &mut crate::mesh::MeshBudget::new(),
        });
        match result {
            Err(crate::curves::GeometryError::Codec(cadmpeg_core::CodecError::ResourceLimit(
                refusal,
            ))) if limit == refusal.used + refusal.additional - 1 => {
                witnessed.insert(refusal.operation);
            }
            Ok(_) => break,
            _ => {}
        }
        if expected
            .iter()
            .all(|operation| witnessed.contains(operation))
        {
            break;
        }
    }
    for operation in expected {
        assert!(
            witnessed.contains(operation),
            "missing refusal at {operation}"
        );
    }
}

#[test]
fn brep_mesh_cache_retention_refusal_reaches_the_caller() {
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
    let brep = with_expand_bytes(&data, |expand| {
        crate::brep::ValidatedRawBrep::try_new(expand.ctx(), raw)
    })
    .expect("validate Brep with one mesh cache slot");
    let association = SourceObjectAssociation {
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
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_retained_bytes = 35;
    let (ctx, root) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&data, &arena, &policy)
        .expect("source bytes fit the root limit");
    let refused = stage_brep(BrepTransferInput {
        expand: crate::mesh::MeshExpand::new(&ctx, root),
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
    .expect_err("36 mesh bytes exceed the 35-byte retention limit");
    assert!(matches!(
        refused,
        crate::curves::GeometryError::Codec(cadmpeg_core::CodecError::ResourceLimit(limit))
            if limit.operation == "rhino_mesh_buffer"
    ));
}

fn assert_brep_carrier_slot_refusal(operation: &str) {
    let (data, raw) = source_shaped_plane_brep();
    let brep = with_expand_bytes(&data, |expand| {
        crate::brep::ValidatedRawBrep::try_new(expand.ctx(), raw)
    })
    .expect("validate source-shaped Brep");
    let association = super::test_association();
    let unknown: UnknownId = "rhino:object:record#plane"
        .try_into()
        .expect("valid identity");
    let service = with_expand_bytes(&data, |expand| {
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
    });
    assert!(service.is_ok(), "service Brep staging: {service:?}");
    let mut witnessed = false;
    for limit in 0..1024_u64 {
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let mut policy = cadmpeg_core::decode::DecodePolicy::service();
        policy.limits.max_collection_items = limit;
        let (ctx, root) =
            cadmpeg_core::decode::DecodeContext::from_root_bytes(&data, &arena, &policy)
                .expect("source bytes fit root limit");
        let result = stage_brep(BrepTransferInput {
            expand: crate::mesh::MeshExpand::new(&ctx, root),
            data: &data,
            archive: ArchiveVersion::V5,
            writer_version: Some(200_206_180),
            brep: &brep,
            key: "plane",
            association: &association,
            unknown: &unknown,
            scale: MillimeterScale::IDENTITY,
            mesh_budget: &mut crate::mesh::MeshBudget::new(),
        });
        if matches!(
            result,
            Err(crate::curves::GeometryError::Codec(cadmpeg_core::CodecError::ResourceLimit(ref refusal)))
                if refusal.operation == operation
                    && limit == refusal.used + refusal.additional - 1
        ) {
            witnessed = true;
            break;
        }
    }
    assert!(witnessed, "missing refusal at {operation}");
}

#[test]
fn brep_c3_slot_map_refuses_collection_limit() {
    assert_brep_carrier_slot_refusal("Rhino Brep C3 slots");
}

#[test]
fn brep_surface_slot_map_refuses_collection_limit() {
    assert_brep_carrier_slot_refusal("Rhino Brep surface slots");
}

#[test]
fn reused_brep_c2_curve_refuses_before_the_second_copy() {
    let (data, mut raw) = source_shaped_plane_brep();
    raw.trims[1].curve = Some(0);
    let brep = with_expand_bytes(&data, |expand| {
        crate::brep::ValidatedRawBrep::try_new(expand.ctx(), raw)
    })
    .expect("validate Brep with a shared C2 slot");
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
    let mut witnessed = false;
    for limit in 0..512_u64 {
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let mut policy = cadmpeg_core::decode::DecodePolicy::service();
        policy.limits.max_collection_items = limit;
        let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&data, &arena, &policy)
            .expect("source bytes fit the root limit");
        match decode_pcurves(
            &ctx,
            &data,
            ArchiveVersion::V5,
            brep.raw(),
            brep.resolved(),
            "plane",
            &std::collections::HashMap::new(),
        ) {
            Err(crate::curves::GeometryError::Codec(cadmpeg_core::CodecError::ResourceLimit(
                refusal,
            ))) if refusal.operation == "Rhino Brep reused C2 curve"
                && limit == refusal.used + refusal.additional - 1 =>
            {
                witnessed = true;
                break;
            }
            Ok(_) => break,
            _ => {}
        }
    }
    assert!(
        witnessed,
        "reused C2 curve copy must refuse below its item count"
    );
}
