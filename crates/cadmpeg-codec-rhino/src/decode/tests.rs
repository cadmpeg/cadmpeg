// SPDX-License-Identifier: Apache-2.0
#![allow(dead_code, clippy::disallowed_methods)]

use super::*;
use crate::test_support::test_dump::*;
use cadmpeg_ir::geometry::{CurveGeometry, NurbsCurve};
use cadmpeg_ir::math::{Point3, Vector3};

fn line_nurbs(start: f64, end: f64, rational: bool) -> NurbsCurve {
    NurbsCurve {
        degree: 1,
        knots: vec![start, start, end, end],
        control_points: vec![Point3::new(start, 0.0, 0.0), Point3::new(end, 0.0, 0.0)],
        weights: rational.then(|| vec![2.0, 1.0]),
        periodic: false,
    }
}

fn decoded_nurbs(curve: NurbsCurve) -> crate::curves::DecodedCurve {
    crate::curves::DecodedCurve {
        geometry: CurveGeometry::Nurbs(curve),
        compound: None,
        warnings: Vec::new(),
    }
}

#[test]
fn rejected_expansion_discards_every_report_bucket() {
    let mut report = ReportBuckets::default();
    report.phase_warnings.push("existing warning".to_string());
    report
        .phase_losses
        .push(RhinoLossCode::ContainerScanDiagnostic.note("existing parse-phase loss"));
    report
        .typed_losses
        .push(RhinoLossCode::IntegrityFailure.note("existing typed loss"));
    let checkpoint = report.checkpoint();

    report.phase_warnings.push("rejected warning".to_string());
    report
        .phase_losses
        .push(RhinoLossCode::ContainerScanDiagnostic.note("rejected parse-phase loss"));
    report
        .typed_losses
        .push(RhinoLossCode::IntegrityFailure.note("rejected typed loss"));
    report.rollback(checkpoint);

    assert_eq!(report.phase_warnings, ["existing warning"]);
    assert_eq!(report.phase_losses.len(), 1);
    assert_eq!(report.phase_losses[0].message, "existing parse-phase loss");
    assert_eq!(report.typed_losses.len(), 1);
    assert_eq!(report.typed_losses[0].message, "existing typed loss");
}

#[test]
fn hatch_plane_places_and_scales_plane_space_loops_once() {
    let plane = crate::settings::Plane {
        origin: crate::settings::Point3([10.0, 20.0, 30.0]),
        xaxis: crate::settings::Vector3([0.0, 1.0, 0.0]),
        yaxis: crate::settings::Vector3([-1.0, 0.0, 0.0]),
        zaxis: crate::settings::Vector3([0.0, 0.0, 1.0]),
        equation: [0.0, 0.0, 1.0, -30.0],
    };
    let mut curve = decoded_nurbs(line_nurbs(0.0, 2.0, false));
    transform_decoded_curve(&mut curve, hatch_plane_transform(&plane, 10.0))
        .expect("required invariant");
    let CurveGeometry::Nurbs(curve) = curve.geometry else {
        panic!("hatch loop must remain NURBS");
    };
    assert_eq!(curve.control_points[0], Point3::new(100.0, 200.0, 300.0));
    assert_eq!(curve.control_points[1], Point3::new(100.0, 220.0, 300.0));
}

#[test]
fn body_instance_transform_composes_before_existing_body_transform() {
    let mut body = Body {
        id: "body".into(),
        kind: BodyKind::General,
        regions: Vec::new(),
        transform: Some(Transform {
            rows: [
                [2.0, 0.0, 0.0, 0.0],
                [0.0, 1.0, 0.0, 0.0],
                [0.0, 0.0, 1.0, 0.0],
                [0.0, 0.0, 0.0, 1.0],
            ],
        }),
        name: None,
        color: None,
        visible: None,
    };
    let instance = Transform {
        rows: [
            [1.0, 0.0, 0.0, 10.0],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
            [0.0, 0.0, 0.0, 1.0],
        ],
    };
    compose_body_transform(&mut body, instance);
    assert_eq!(
        body.transform
            .expect("required invariant")
            .apply_point(Point3::new(1.0, 0.0, 0.0)),
        Point3::new(12.0, 0.0, 0.0)
    );
}

fn region_raw(
    face_sides: Vec<crate::brep::RawBrepFaceSide>,
    regions: Vec<crate::brep::RawBrepRegion>,
) -> crate::brep::RawBrep {
    let empty_curves = || crate::brep::RawBrepChildren {
        slots: Vec::new(),
        source_range: 0..0,
        expected_type: crate::brep::RawBrepBaseType::Curve,
    };
    crate::brep::RawBrep {
        losses: Vec::new(),
        minor: 3,
        c2: empty_curves(),
        c3: empty_curves(),
        surfaces: crate::brep::RawBrepChildren {
            slots: Vec::new(),
            source_range: 0..0,
            expected_type: crate::brep::RawBrepBaseType::Surface,
        },
        vertices: Vec::new(),
        edges: Vec::new(),
        trims: Vec::new(),
        loops: Vec::new(),
        faces: vec![crate::brep::RawBrepFace {
            index: 0,
            loops: Vec::new(),
            surface: 0,
            reversed_surface: 0,
            material_channel: 0,
            uuid: None,
            color: None,
            source_range: 0..0,
        }],
        bounds: crate::settings::BoundingBox {
            minimum: crate::settings::Point3([0.0, 0.0, 0.0]),
            maximum: crate::settings::Point3([1.0, 1.0, 1.0]),
        },
        render_meshes: Vec::new(),
        analysis_meshes: Vec::new(),
        render_mesh_array_range: 0..0,
        analysis_mesh_array_range: 0..0,
        is_solid: None,
        face_sides,
        regions,
        region_wrapper_range: Some(0..0),
        source_range: 0..0,
        vertex_array_range: 0..0,
        edge_array_range: 0..0,
        trim_array_range: 0..0,
        loop_array_range: 0..0,
        face_array_range: 0..0,
    }
}

fn region(index: i32, region_type: i32) -> crate::brep::RawBrepRegion {
    crate::brep::RawBrepRegion {
        index,
        region_type,
        sides: Vec::new(),
        bounds: crate::settings::BoundingBox {
            minimum: crate::settings::Point3([0.0, 0.0, 0.0]),
            maximum: crate::settings::Point3([1.0, 1.0, 1.0]),
        },
        source_range: 0..0,
    }
}

fn append_line_payload(
    data: &mut Vec<u8>,
    from: [f64; 3],
    to: [f64; 3],
    dimension: i32,
) -> std::ops::Range<usize> {
    let start = data.len();
    data.push(0x10);
    for value in from.into_iter().chain(to) {
        data.extend_from_slice(&value.to_le_bytes());
    }
    for value in [0.0_f64, 1.0] {
        data.extend_from_slice(&value.to_le_bytes());
    }
    data.extend_from_slice(&dimension.to_le_bytes());
    start..data.len()
}

fn append_plane_payload(data: &mut Vec<u8>) -> std::ops::Range<usize> {
    let start = data.len();
    data.push(0x11);
    for value in [
        0.0_f64, 0.0, 0.0, // origin
        1.0, 0.0, 0.0, // x
        0.0, 1.0, 0.0, // y
        0.0, 0.0, 1.0, // z
        0.0, 0.0, 1.0, 0.0, // equation
    ] {
        data.extend_from_slice(&value.to_le_bytes());
    }
    for _ in 0..4 {
        for value in [0.0_f64, 1.0] {
            data.extend_from_slice(&value.to_le_bytes());
        }
    }
    start..data.len()
}

fn class_uuid(wire: [u8; 16]) -> crate::wire::Uuid {
    crate::wire::Uuid::from_wire(wire)
}

fn child(
    class_uuid: crate::wire::Uuid,
    class_data_range: std::ops::Range<usize>,
    base_type: crate::brep::RawBrepBaseType,
) -> crate::brep::RawBrepChild {
    crate::brep::RawBrepChild {
        class_uuid,
        source_range: class_data_range.clone(),
        class_data_range,
        base_type,
    }
}

fn source_shaped_plane_brep() -> (Vec<u8>, crate::brep::RawBrep) {
    let line_uuid = class_uuid([
        0xdb, 0xd4, 0xd7, 0x4e, 0x47, 0xe9, 0xd3, 0x11, 0xbf, 0xe5, 0x00, 0x10, 0x83, 0x01, 0x22,
        0xf0,
    ]);
    let plane_uuid = class_uuid([
        0xdf, 0xd4, 0xd7, 0x4e, 0x47, 0xe9, 0xd3, 0x11, 0xbf, 0xe5, 0x00, 0x10, 0x83, 0x01, 0x22,
        0xf0,
    ]);
    let mut data = Vec::new();
    let c3_ranges = [
        append_line_payload(&mut data, [0.0, 0.0, 0.0], [1.0, 0.0, 0.0], 3),
        append_line_payload(&mut data, [1.0, 0.0, 0.0], [0.0, 1.0, 0.0], 3),
        append_line_payload(&mut data, [0.0, 1.0, 0.0], [0.0, 0.0, 0.0], 3),
    ];
    let c2_ranges = [
        append_line_payload(&mut data, [0.0, 0.0, 0.0], [1.0, 0.0, 0.0], 2),
        append_line_payload(&mut data, [1.0, 0.0, 0.0], [0.0, 1.0, 0.0], 2),
        append_line_payload(&mut data, [0.0, 1.0, 0.0], [0.0, 0.0, 0.0], 2),
    ];
    let surface_range = append_plane_payload(&mut data);
    let interval = crate::settings::Interval([0.0, 1.0]);
    let endpoints = [[0, 1], [1, 2], [2, 0]];
    let vertices = [[0, 2], [0, 1], [1, 2]]
        .into_iter()
        .enumerate()
        .map(|(index, edges)| crate::brep::RawBrepVertex {
            index: i32::try_from(index).expect("index"),
            point: crate::settings::Point3([
                f64::from((index == 1) as u8),
                f64::from((index == 2) as u8),
                0.0,
            ]),
            edges: edges.into_iter().collect(),
            tolerance: 0.01,
            source_range: 0..0,
        })
        .collect();
    let edges = endpoints
        .into_iter()
        .enumerate()
        .map(|(index, vertices)| crate::brep::RawBrepEdge {
            index: i32::try_from(index).expect("index"),
            curve: i32::try_from(index).expect("index"),
            proxy_reversed: 0,
            proxy_domain: interval,
            vertices,
            trims: vec![i32::try_from(index).expect("index")],
            tolerance: 0.01,
            domain: interval,
            source_range: 0..0,
        })
        .collect();
    let trims = endpoints
        .into_iter()
        .enumerate()
        .map(|(index, vertices)| crate::brep::RawBrepTrim {
            index: i32::try_from(index).expect("index"),
            curve: i32::try_from(index).expect("index"),
            proxy_domain: interval,
            edge: i32::try_from(index).expect("index"),
            vertices,
            reversed_3d: 0,
            trim_type: 1,
            iso: 0,
            loop_index: 0,
            tolerances: [0.02, 0.03],
            domain: interval,
            proxy_reversed: 0,
            reserved: Vec::new(),
            legacy_tolerances: [0.02, 0.03],
            source_range: 0..0,
        })
        .collect();
    (
        data,
        crate::brep::RawBrep {
            losses: Vec::new(),
            minor: 2,
            c2: crate::brep::RawBrepChildren {
                slots: c2_ranges
                    .into_iter()
                    .map(|range| Some(child(line_uuid, range, crate::brep::RawBrepBaseType::Curve)))
                    .collect(),
                source_range: 0..0,
                expected_type: crate::brep::RawBrepBaseType::Curve,
            },
            c3: crate::brep::RawBrepChildren {
                slots: c3_ranges
                    .into_iter()
                    .map(|range| Some(child(line_uuid, range, crate::brep::RawBrepBaseType::Curve)))
                    .collect(),
                source_range: 0..0,
                expected_type: crate::brep::RawBrepBaseType::Curve,
            },
            surfaces: crate::brep::RawBrepChildren {
                slots: vec![Some(child(
                    plane_uuid,
                    surface_range,
                    crate::brep::RawBrepBaseType::Surface,
                ))],
                source_range: 0..0,
                expected_type: crate::brep::RawBrepBaseType::Surface,
            },
            vertices,
            edges,
            trims,
            loops: vec![crate::brep::RawBrepLoop {
                index: 0,
                trims: vec![0, 1, 2],
                loop_type: 1,
                face: 0,
                source_range: 0..0,
            }],
            faces: vec![crate::brep::RawBrepFace {
                index: 0,
                loops: vec![0],
                surface: 0,
                reversed_surface: 0,
                material_channel: 0,
                uuid: None,
                color: None,
                source_range: 0..0,
            }],
            bounds: crate::settings::BoundingBox {
                minimum: crate::settings::Point3([0.0, 0.0, 0.0]),
                maximum: crate::settings::Point3([1.0, 1.0, 0.0]),
            },
            render_meshes: Vec::new(),
            analysis_meshes: Vec::new(),
            render_mesh_array_range: 0..0,
            analysis_mesh_array_range: 0..0,
            is_solid: Some(3),
            face_sides: Vec::new(),
            regions: Vec::new(),
            region_wrapper_range: None,
            source_range: 0..0,
            vertex_array_range: 0..0,
            edge_array_range: 0..0,
            trim_array_range: 0..0,
            loop_array_range: 0..0,
            face_array_range: 0..0,
        },
    )
}

#[test]
fn fallback_discards_topology_and_unknown_record_self_link() {
    let curve_id: cadmpeg_ir::ids::CurveId = "rhino:object:curve#x.c3-0".into();
    let surface_id: cadmpeg_ir::ids::SurfaceId = "rhino:object:surface#x.slot-0".into();
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
        geometry: CurveGeometry::Unknown { record: None },
        source_object: None,
    });
    staged.draft.model_mut().surfaces.push(Surface {
        id: surface_id.clone(),
        geometry: cadmpeg_ir::geometry::SurfaceGeometry::Unknown { record: None },
        source_object: None,
    });
    staged.draft.model_mut().bodies.push(Body {
        id: "rhino:object:body#x".into(),
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
fn fallback_candidate_links_free_carrier_before_full_ir_validation() {
    let unknown: UnknownId = "rhino:object:record#x".into();
    let curve_id: cadmpeg_ir::ids::CurveId = "rhino:object:curve#x.c3-0".into();
    let mut candidate = CadIr::empty(Units::default());
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
        geometry: CurveGeometry::Nurbs(line_nurbs(0.0, 1.0, false)),
        source_object: None,
    });
    let links = staged.links.clone();
    staged
        .apply(&mut candidate, &mut cadmpeg_ir::Annotations::default())
        .expect("commit fallback carrier");
    append_record_links(&mut candidate, &unknown, &links);
    assert_eq!(
        candidate
            .native_unknowns("rhino")
            .expect("required invariant")[0]
            .links,
        vec![curve_id.to_string()]
    );
    let report = cadmpeg_ir::validate::validate_neutral(&candidate, Vec::new());
    assert!(report.is_ok(), "{report:?}");
}

#[test]
fn colliding_staged_ids_are_rejected_without_mutating_the_candidate() {
    let curve_id: cadmpeg_ir::ids::CurveId = "rhino:object:curve#x.c3-0".into();
    let curve = Curve {
        id: curve_id,
        geometry: CurveGeometry::Nurbs(line_nurbs(0.0, 1.0, false)),
        source_object: None,
    };
    let mut live = CadIr::empty(Units::default());
    live.model.curves.push(curve.clone());
    let mut candidate = live.clone();
    let mut staged = BrepDraft::default();
    staged.draft.model_mut().curves.push(curve);
    assert!(staged
        .apply(&mut candidate, &mut cadmpeg_ir::Annotations::default())
        .is_err());
    assert_eq!(candidate, live);
    assert_eq!(live.model.curves.len(), 1);
}

#[test]
fn source_shaped_plane_brep_stages_complete_scaled_valid_ir() {
    let (data, raw) = source_shaped_plane_brep();
    let brep = crate::brep::ValidatedRawBrep::try_new(raw).expect("validate source-shaped Brep");
    let association = SourceObjectAssociation {
        format: "rhino".to_string(),
        object_id: "plane-brep".to_string(),
        name: Some("plane".to_string()),
        color: None,
        visible: Some(true),
        layer: None,
        instance_path: Vec::new(),
    };
    let unknown: UnknownId = "rhino:object:record#plane".into();
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
            scale: 25.4,
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
    assert_eq!(model.points[1].position.x, 25.4);
    assert_eq!(model.vertices[0].tolerance, Some(0.254));
    assert_eq!(model.edges[0].tolerance, Some(0.254));
    assert_eq!(model.pcurves[0].fit_tolerance, Some(0.02));
    let PcurveGeometry::Nurbs { control_points, .. } = &model.pcurves[0].geometry else {
        panic!("line C2 must be a NURBS pcurve");
    };
    // Plane parameters are lengths: the native `u = 1.0` trim endpoint
    // scales with the document (inches -> millimeters).
    assert_eq!(control_points[1].u, 25.4);
    assert_eq!(model.coedges[0].radial_next, model.coedges[0].id);
    let links = staged.links.clone();
    let mut candidate = CadIr::empty(Units::default());
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
        .apply(&mut candidate, &mut cadmpeg_ir::Annotations::default())
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
        point: crate::settings::Point3([2.0, 2.0, 0.0]),
        edges: Vec::new(),
        tolerance: 0.0,
        source_range: 0..0,
    });
    let brep = crate::brep::ValidatedRawBrep::try_new(raw).expect("validate Brep");
    let association = SourceObjectAssociation {
        format: "rhino".to_string(),
        object_id: "free-vertex-brep".to_string(),
        name: None,
        color: None,
        visible: None,
        layer: None,
        instance_path: Vec::new(),
    };
    let unknown: UnknownId = "rhino:object:record#free-vertex".into();
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
            scale: 1.0,
            mesh_budget: &mut crate::mesh::MeshBudget::new(),
        })
    })
    .expect("stage Brep with an isolated vertex");
    assert_eq!(staged.kind, BrepTransferKind::FullTopology);
    assert_eq!(
        staged.draft.model().shells[0].free_vertices,
        vec!["rhino:object:vertex#free-vertex.slot-3".into()]
    );

    let mut candidate = CadIr::empty(Units::default());
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
        .apply(&mut candidate, &mut cadmpeg_ir::Annotations::default())
        .expect("commit Brep with an isolated vertex");
    let report = cadmpeg_ir::validate::validate_neutral(&candidate, Vec::new());
    assert!(report.is_ok(), "{report:?}");
}

#[test]
fn failed_trim_pcurve_does_not_discard_brep_topology() {
    let (data, mut raw) = source_shaped_plane_brep();
    raw.c2.slots[1].as_mut().expect("C2 slot").class_uuid = class_uuid([0; 16]);
    let brep = crate::brep::ValidatedRawBrep::try_new(raw).expect("validate source-shaped Brep");
    let association = SourceObjectAssociation {
        format: "rhino".to_string(),
        object_id: "plane-brep".to_string(),
        name: None,
        color: None,
        visible: None,
        layer: None,
        instance_path: Vec::new(),
    };
    let unknown: UnknownId = "rhino:object:record#plane".into();
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
            scale: 1.0,
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
    let grouping =
        region_shell_groups_without_records(&[1, 0, 1, 0]).expect("shell-group allocation");
    assert!(grouping.fallback);
    assert_eq!(grouping.face_groups, vec![1, 0, 1, 0]);
    assert_eq!(grouping.region_labels, vec![0, 1]);
    assert_eq!(grouping.shell_faces, vec![vec![1, 3], vec![0, 2]]);
}

#[test]
fn tolerance_scaling_maps_unset_and_zero_to_none() {
    assert_eq!(
        scaled_tolerance(0.0, 25.4).expect("required invariant"),
        None
    );
    assert_eq!(
        scaled_tolerance(0.5, 25.4).expect("required invariant"),
        Some(12.7)
    );
    assert_eq!(finite_tolerance(0.5), Some(0.5));
    assert_eq!(finite_tolerance(-1.0), None);
}

#[test]
fn edge_proxy_reversal_normalizes_endpoints_and_keeps_an_ascending_range() {
    let edge = crate::brep::RawBrepEdge {
        index: 0,
        curve: 0,
        proxy_reversed: 0,
        proxy_domain: crate::settings::Interval([3.0, 7.0]),
        vertices: [0, 1],
        trims: Vec::new(),
        tolerance: 0.0,
        domain: crate::settings::Interval([100.0, 200.0]),
        source_range: 0..0,
    };
    assert_eq!(edge_param_range(&edge), [3.0, 7.0]);
    assert_eq!(edge_vertices(&edge), [0, 1]);
    let reversed = crate::brep::RawBrepEdge {
        proxy_reversed: 1,
        ..edge
    };
    assert_eq!(edge_param_range(&reversed), [3.0, 7.0]);
    assert_eq!(edge_vertices(&reversed), [1, 0]);
}

#[test]
fn coedge_and_edge_proxy_reversals_are_independent() {
    for trim_reversed in [false, true] {
        for edge_proxy_reversed in [false, true] {
            assert_eq!(
                coedge_sense(trim_reversed, edge_proxy_reversed),
                if trim_reversed ^ edge_proxy_reversed {
                    Sense::Reversed
                } else {
                    Sense::Forward
                }
            );
        }
    }
}

#[test]
fn face_reversal_selects_face_sense() {
    assert_eq!(face_sense(false), Sense::Forward);
    assert_eq!(face_sense(true), Sense::Reversed);
}

#[test]
fn polymorphic_object_geometry_starts_with_v2() {
    assert!(!ArchiveVersion::V1.is_chunked());
    assert!(ArchiveVersion::V2.is_chunked());
    assert!(ArchiveVersion::V8.is_chunked());
}

#[test]
fn representable_region_uses_bounded_membership_and_serialized_direction() {
    let raw = region_raw(
        vec![
            crate::brep::RawBrepFaceSide {
                index: 0,
                region: 1,
                face: 0,
                direction: 1,
                source_range: 0..0,
            },
            crate::brep::RawBrepFaceSide {
                index: 1,
                region: 0,
                face: 0,
                direction: -1,
                source_range: 0..0,
            },
        ],
        vec![region(0, 0), region(1, 1)],
    );
    let grouping = region_shell_groups(&raw, &[0]).expect("shell-group allocation");
    assert!(!grouping.fallback);
    assert_eq!(grouping.face_groups, vec![0]);
    assert_eq!(grouping.region_labels, vec![1]);
    assert_eq!(grouping.shell_faces, vec![vec![0]]);
}

#[test]
fn two_bounded_regions_sharing_one_face_use_deterministic_incidence_fallback() {
    let raw = region_raw(
        vec![
            crate::brep::RawBrepFaceSide {
                index: 0,
                region: 1,
                face: 0,
                direction: 1,
                source_range: 0..0,
            },
            crate::brep::RawBrepFaceSide {
                index: 1,
                region: 2,
                face: 0,
                direction: -1,
                source_range: 0..0,
            },
        ],
        vec![region(0, 0), region(1, 1), region(2, 1)],
    );
    let grouping = region_shell_groups(&raw, &[0]).expect("shell-group allocation");
    assert!(grouping.fallback);
    assert_eq!(grouping.region_labels, vec![0]);
    assert_eq!(grouping.shell_faces, vec![vec![0]]);
}

#[test]
fn c2_polycurve_merges_clamped_rational_segments_in_parent_domain() {
    let compound = crate::curves::DecodedCurve {
        geometry: CurveGeometry::Unknown { record: None },
        compound: Some(crate::curves::Compound {
            children: vec![
                decoded_nurbs(line_nurbs(0.0, 1.0, true)),
                decoded_nurbs(line_nurbs(-2.0, 2.0, false)),
            ],
            parameters: vec![10.0, 20.0, 40.0],
        }),
        warnings: Vec::new(),
    };
    let merged = c2_curve_to_nurbs_join(compound, 0).expect("merge").curve;
    assert_eq!(merged.knots, vec![10.0, 10.0, 20.0, 40.0, 40.0]);
    assert_eq!(merged.control_points.len(), 3);
    assert_eq!(merged.weights, Some(vec![2.0, 1.0, 1.0]));
    assert!(!merged.periodic);
}

#[test]
fn recursive_c2_polycurve_preserves_nested_parent_parameterization() {
    let nested = crate::curves::DecodedCurve {
        geometry: CurveGeometry::Unknown { record: None },
        compound: Some(crate::curves::Compound {
            children: vec![
                decoded_nurbs(line_nurbs(0.0, 1.0, false)),
                decoded_nurbs(line_nurbs(0.0, 1.0, false)),
            ],
            parameters: vec![0.0, 1.0, 2.0],
        }),
        warnings: Vec::new(),
    };
    let outer = crate::curves::DecodedCurve {
        geometry: CurveGeometry::Unknown { record: None },
        compound: Some(crate::curves::Compound {
            children: vec![nested],
            parameters: vec![5.0, 9.0],
        }),
        warnings: Vec::new(),
    };
    let merged = c2_curve_to_nurbs_join(outer, 0)
        .expect("nested merge")
        .curve;
    assert_eq!(merged.knots, vec![5.0, 5.0, 7.0, 9.0, 9.0]);
}

#[test]
fn unequal_degree_c2_polycurve_elevates_lower_degree() {
    let quadratic = NurbsCurve {
        degree: 2,
        knots: vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
        control_points: vec![
            Point3::new(0.0, 0.0, 0.0),
            Point3::new(0.5, 1.0, 0.0),
            Point3::new(1.0, 0.0, 0.0),
        ],
        weights: Some(vec![1.0, 0.5, 1.0]),
        periodic: false,
    };
    let compound = crate::curves::DecodedCurve {
        geometry: CurveGeometry::Unknown { record: None },
        compound: Some(crate::curves::Compound {
            children: vec![
                decoded_nurbs(line_nurbs(0.0, 1.0, false)),
                decoded_nurbs(quadratic),
            ],
            parameters: vec![0.0, 1.0, 2.0],
        }),
        warnings: Vec::new(),
    };
    let merged = c2_curve_to_nurbs_join(compound, 0)
        .expect("degree elevation")
        .curve;
    assert_eq!(merged.degree, 2);
    assert_eq!(merged.control_points.len(), 5);
    assert_eq!(merged.knots, vec![0.0, 0.0, 0.0, 1.0, 1.0, 2.0, 2.0, 2.0]);
}

fn cap_boundary(points: &[Point3]) -> crate::extrusion::ExtrusionBoundary {
    let knots = vec![0.0, 0.0, 1.0, 2.0, 3.0, 4.0, 4.0];
    let start = NurbsCurve {
        degree: 1,
        knots: knots.clone(),
        control_points: points.to_vec(),
        weights: None,
        periodic: false,
    };
    let end_points = points
        .iter()
        .map(|point| Point3::new(point.x, point.y, point.z + 5.0))
        .collect::<Vec<_>>();
    let end = NurbsCurve {
        degree: 1,
        knots: knots.clone(),
        control_points: end_points,
        weights: None,
        periodic: false,
    };
    let pcurve_points = points
        .iter()
        .map(|point| Point2::new(point.x, point.y))
        .collect::<Vec<_>>();
    let pcurve = crate::extrusion::CapPcurve {
        degree: 1,
        knots,
        control_points: pcurve_points,
        weights: None,
        periodic: false,
    };
    crate::extrusion::ExtrusionBoundary {
        start_curve: decoded_nurbs(start.clone()),
        start_nurbs: start,
        end_nurbs: end,
        start_pcurve: pcurve.clone(),
        end_pcurve: pcurve,
    }
}

fn cap_extrusion(caps: [bool; 2]) -> crate::extrusion::DecodedExtrusion {
    let outer = cap_boundary(&[
        Point3::new(0.0, 0.0, 0.0),
        Point3::new(4.0, 0.0, 0.0),
        Point3::new(4.0, 4.0, 0.0),
        Point3::new(0.0, 4.0, 0.0),
        Point3::new(0.0, 0.0, 0.0),
    ]);
    let inner = cap_boundary(&[
        Point3::new(1.0, 1.0, 0.0),
        Point3::new(1.0, 2.0, 0.0),
        Point3::new(2.0, 2.0, 0.0),
        Point3::new(2.0, 1.0, 0.0),
        Point3::new(1.0, 1.0, 0.0),
    ]);
    crate::extrusion::DecodedExtrusion {
        boundaries: vec![outer, inner],
        laterals: Vec::new(),
        direction: Vector3::new(0.0, 0.0, 5.0),
        cap_origins: [Point3::new(0.0, 0.0, 0.0), Point3::new(0.0, 0.0, 5.0)],
        cap_normals: [Vector3::new(0.0, 0.0, 1.0), Vector3::new(0.0, 0.0, 1.0)],
        cap_u_axes: [Vector3::new(1.0, 0.0, 0.0), Vector3::new(1.0, 0.0, 0.0)],
        caps,
        meshes: Vec::new(),
        warnings: Vec::new(),
    }
}

fn test_association() -> SourceObjectAssociation {
    SourceObjectAssociation {
        format: "rhino".to_string(),
        object_id: "extrusion".to_string(),
        name: Some("Extrusion".to_string()),
        color: None,
        visible: Some(true),
        layer: None,
        instance_path: Vec::new(),
    }
}

#[test]
fn extrusion_caps_build_outer_and_hole_loops_with_opposite_face_senses() {
    for (caps, expected_faces) in [([true, false], 1), ([false, true], 1), ([true, true], 2)] {
        let mut ir = CadIr::empty(Units::default());
        let association = test_association();
        let extrusion = cap_extrusion(caps);
        let directrices = extrusion
            .boundaries
            .iter()
            .enumerate()
            .map(|(index, boundary)| {
                let id: cadmpeg_ir::ids::CurveId = format!("rhino:object:curve#cap-{index}").into();
                ir.model.curves.push(Curve {
                    id: id.clone(),
                    geometry: CurveGeometry::Nurbs(boundary.start_nurbs.clone()),
                    source_object: Some(association.clone()),
                });
                id
            })
            .collect::<Vec<_>>();
        let mut links = Vec::new();
        assert!(stage_extrusion_caps(
            &mut ir,
            &mut cadmpeg_ir::Annotations::default(),
            "caps",
            &association,
            &extrusion,
            &directrices,
            &mut links,
        ));
        assert_eq!(ir.model.faces.len(), expected_faces);
        assert_eq!(ir.model.regions.len(), expected_faces);
        assert_eq!(ir.model.shells.len(), expected_faces);
        assert_eq!(ir.model.loops.len(), expected_faces * 2);
        assert_eq!(ir.model.pcurves.len(), expected_faces * 2);
        if expected_faces == 2 {
            assert_eq!(ir.model.faces[0].sense, Sense::Reversed);
            assert_eq!(ir.model.faces[1].sense, Sense::Forward);
        }
        assert_eq!(
            cadmpeg_ir::validate_neutral(&ir, Vec::new()).error_count(),
            0
        );
    }
}

#[test]
fn cap_staging_failure_leaves_original_transaction_unmodified() {
    let original = CadIr::empty(Units::default());
    let mut candidate = original.clone();
    let mut links = Vec::new();
    assert!(!stage_extrusion_caps(
        &mut candidate,
        &mut cadmpeg_ir::Annotations::default(),
        "failure",
        &test_association(),
        &cap_extrusion([true, true]),
        &[],
        &mut links,
    ));
    assert_eq!(candidate, original);
    assert!(links.is_empty());
}

/// Phase 5 freeze: draft/instance admit predicates vs shared accept/reject builders.
#[test]
fn phase5_freeze_shared_admissibility_fixtures() {
    let accepted = cadmpeg_ir::validate::admissibility_freeze::accepted_empty();
    let rejected = cadmpeg_ir::validate::admissibility_freeze::rejected_missing_point("rhino:test");
    let annotations = cadmpeg_ir::Annotations::default();

    assert!(cadmpeg_ir::admit_with_annotations(
        &accepted,
        &annotations,
        cadmpeg_ir::RHINO_DRAFT_CHECKS,
        Vec::new(),
    )
    .is_ok());
    assert!(cadmpeg_ir::admit(&accepted, cadmpeg_ir::RHINO_INSTANCE_CHECKS, Vec::new()).is_ok());

    assert!(!cadmpeg_ir::admit_with_annotations(
        &rejected,
        &annotations,
        cadmpeg_ir::RHINO_DRAFT_CHECKS,
        Vec::new(),
    )
    .is_ok());
    assert!(!cadmpeg_ir::admit(&rejected, cadmpeg_ir::RHINO_INSTANCE_CHECKS, Vec::new()).is_ok());
}

#[test]
fn decode_context_transitions_object_status_once_and_links_unknowns() {
    let archive = ArchiveVersion::V5;
    let object = object_record(archive, 1, [0; 16]);
    let bytes = minimal_document(
        "50",
        &[
            table(archive, 0x1000_0014, &[]),
            table(archive, 0x1000_0015, &[]),
            table(archive, 0x1000_0013, &[object]),
        ],
    );
    let scan = crate::container::scan_owned(bytes).expect("required invariant");
    crate::decode::with_expand(&scan, |expand| {
        let mut context = crate::decode::DecodeContext::new(&scan, expand);
        assert!(context.object(0).is_some());
        assert!(context.unknown(0).is_some());
        assert_eq!(context.unit_scale(), None);
        assert_eq!(context.archive(), archive);
        assert!(context.append_link(0, "rhino:curve#2".to_string()));
        assert!(context.append_link(0, "rhino:curve#1".to_string()));
        assert!(context.append_link(0, "rhino:curve#2".to_string()));
        assert_eq!(
            context.unknown(0).expect("required invariant").links,
            vec!["rhino:curve#1".to_string(), "rhino:curve#2".to_string()]
        );
        assert!(context.mark_decoded(0));
        assert!(!context.mark_decoded(0));
        assert!(!context.mark_failed(0));
        assert_eq!(context.ir_mut().model.bodies.len(), 0);
        context
            .unknown_mut(0)
            .expect("required invariant")
            .links
            .clear();
        let result = context
            .commit()
            .expect("the Rhino source and report formats agree");
        assert!(result
            .report()
            .losses
            .iter()
            .any(|loss| loss.severity == Severity::Info));
        assert_eq!(
            result
                .ir()
                .native_unknowns("rhino")
                .expect("required invariant")
                .len(),
            1
        );
        let validation = cadmpeg_ir::validate_neutral(result.ir(), result.report().losses.clone());
        assert_eq!(validation.error_count(), 0);
    });
}

#[test]
fn rejected_candidate_rolls_back_entities_and_preserves_retained_bytes() {
    let archive = ArchiveVersion::V5;
    let object = object_record(archive, 1, [0; 16]);
    let bytes = minimal_document(
        "50",
        &[
            table(archive, 0x1000_0014, &[]),
            table(archive, 0x1000_0015, &[]),
            table(archive, 0x1000_0013, &[object]),
        ],
    );
    let scan = crate::container::scan_owned(bytes).expect("required invariant");
    crate::decode::with_expand(&scan, |expand| {
        let mut context = crate::decode::DecodeContext::new(&scan, expand);
        let original = context
            .unknown(0)
            .expect("required invariant")
            .data
            .clone()
            .expect("required invariant");
        let findings = context.reject_duplicate_entity_candidate();
        assert!(findings.contains("identity"));
        assert_eq!(
            context
                .unknown(0)
                .expect("required invariant")
                .data
                .as_deref(),
            Some(original.as_slice())
        );
        assert_eq!(context.unknown_count(), 1);
        let matching = context
            .ir_mut()
            .model
            .points
            .iter()
            .filter(|point| point.id.0 == "rhino:test:duplicate-point")
            .collect::<Vec<_>>();
        assert_eq!(matching.len(), 1);
        assert_eq!(
            matching[0].position,
            cadmpeg_ir::math::Point3::new(1.0, 2.0, 3.0)
        );
    });
}

#[test]
fn unknown_surface_placeholder_does_not_report_geometry_transfer() {
    let archive = ArchiveVersion::V5;
    let object = object_record_with_payload(archive, 8, REV_SURFACE_CLASS, &[0]);
    let mut scan = scan_with_objects(&[object]);
    set_test_units(&mut scan, 1.0);
    let result = crate::decode::decode_for_test(&scan);
    assert_eq!(result.ir().model.surfaces.len(), 1);
    assert!(matches!(
        result.ir().model.surfaces[0].geometry,
        cadmpeg_ir::geometry::SurfaceGeometry::Unknown { .. }
    ));
    assert!(!result.report().geometry_transferred());
}

#[test]
fn scaled_coordinate_overflow_retains_object_transactionally_and_repeats_deterministically() {
    let archive = ArchiveVersion::V5;
    let object =
        object_record_with_payload(archive, 1, POINT_CLASS, &point_payload([2.0, 0.0, 0.0]));
    let mut scan = scan_with_objects(&[object]);
    set_test_units(&mut scan, 1.0e308);
    let first = crate::decode::decode_for_test(&scan);
    let second = crate::decode::decode_for_test(&scan);
    assert!(first.ir().model.points.is_empty());
    assert_eq!(first.ir(), second.ir());
    assert_eq!(first.report(), second.report());
    assert!(first
        .report()
        .losses
        .iter()
        .any(|loss| loss.severity == Severity::Error));
}

#[test]
fn redundant_field_diagnostics_use_the_typed_repair_loss() {
    assert!(redundant_field_diagnostic(
        "redundant mesh channel count mismatch"
    ));
    assert!(redundant_field_diagnostic(
        "rhino:object:curve#1: redundant point-cloud color count mismatch"
    ));
    assert!(!redundant_field_diagnostic("mesh channel count mismatch"));
    assert_eq!(
        RhinoLossCode::RedundantFieldRepaired
            .note("repair")
            .code
            .local_code(),
        "container.redundant-field-repaired"
    );
}

/// The body-kind and B-rep domain charges reach the report as typed codes.
///
/// Both are produced as typed losses at their parse sites. This asserts the
/// loss codes that survive the decode pipeline.
#[test]
fn missing_stamp_carries_brep_typed_loss_codes() {
    use cadmpeg_ir::codec::{Codec, DecodeOptions};

    let decode_archive = |bytes: Vec<u8>| {
        crate::RhinoCodec
            .decode(&mut std::io::Cursor::new(bytes), &DecodeOptions::default())
            .expect("synthesized 3DM archive should decode")
    };
    let solid_brep = crate::test_support::object_record(
        0x10,
        crate::test_support::BREP_CLASS,
        &crate::test_support::solid_flagged_brep_payload(1),
    );

    let unstamped = decode_archive(crate::test_support::archive(std::slice::from_ref(
        &solid_brep,
    )));
    assert_eq!(unstamped.ir().model.bodies.len(), 1);
    // The stored flag is trusted, though the three edges carry one trim each.
    assert_eq!(unstamped.ir().model.bodies[0].kind, BodyKind::Solid);
    assert!(
        unstamped
            .report()
            .losses
            .iter()
            .any(|loss| loss.code == RhinoLossCode::TopologyBodyKindGaugeSubstituted.kind()),
        "{:?}",
        unstamped.report().losses
    );
    assert!(
        unstamped.report().losses.iter().any(|loss| loss.code
            == RhinoLossCode::SourceWriterStampUnverified.kind()
            && loss.message.contains("edge domains")),
        "{:?}",
        unstamped.report().losses
    );

    // A stamp older than both cutoffs keeps the same record layout readable and
    // vouches for the reading, so the body is gauged as a sheet and nothing is
    // charged. Any newer stamp would also change the edge and trim layout.
    let stamped = decode_archive(crate::test_support::archive_writer(
        "50",
        200_206_170,
        &[solid_brep],
    ));
    assert_eq!(stamped.ir().model.bodies.len(), 1);
    assert_eq!(stamped.ir().model.bodies[0].kind, BodyKind::Sheet);
    assert!(
        !stamped.report().losses.iter().any(|loss| {
            loss.code == RhinoLossCode::TopologyBodyKindGaugeSubstituted.kind()
                || loss.code == RhinoLossCode::SourceWriterStampUnverified.kind()
        }),
        "{:?}",
        stamped.report().losses
    );
}
