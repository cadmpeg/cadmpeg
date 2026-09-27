// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::disallowed_methods)]

use super::{
    append_link_to_record, append_record_links, brep_free_vertex_indices, c2_curve_to_nurbs_join,
    coedge_sense, commit_curve_tree, copy_retained_link, edge_param_range, edge_vertices,
    face_components, face_sense, hatch_loop_ids, hatch_plane_transform, hatch_source_links,
    region_shell_groups, region_shell_groups_without_records, scaled_tolerance, seal_for_test,
    set_exactness, snapshot_instance_links, snapshot_instance_statuses, stage_brep,
    stage_curve_tree, stage_extrusion_caps, transform_decoded_curve, transform_surface,
    with_expand, with_expand_bytes, BrepDraft, BrepTransferInput, BrepTransferKind, CandidateError,
    CommittedExtrusionBoundary, CurveCommitSource, DecodeContext, GeometryOutcome,
    ReferenceFailure, ReportBuckets,
};
use crate::chunks::ArchiveVersion;
use crate::loss::Diagnostics;
use crate::loss::RhinoLossCode;
use crate::objects::ObjectRecord;
use crate::settings::MillimeterScale;
use crate::test_support::test_dump::{
    minimal_document, object_record, object_record_with_payload, point_payload, scan_with_objects,
    set_test_units, table, MESH_CLASS, POINT_CLASS, REV_SURFACE_CLASS,
};
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::draft::ModelCheckpoint;
use cadmpeg_ir::geometry::{nurbs::NurbsCurve, CurveGeometry, SolvedCurveGeometry};
use cadmpeg_ir::geometry::{pcurve::PcurveGeometry, Curve, SolvedSurfaceGeometry, Surface};
use cadmpeg_ir::ids::UnknownId;
use cadmpeg_ir::math::Point2;
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::report::Severity;
use cadmpeg_ir::topology::{Body, BodyKind, Point, Sense};
use cadmpeg_ir::unknown::{NativeUnknownRecord, UnknownRecord};
use cadmpeg_ir::{Exactness, SourceObjectAssociation};

fn line_nurbs(start: f64, end: f64, rational: bool) -> NurbsCurve {
    NurbsCurve::from_lanes(
        1,
        vec![start, start, end, end],
        vec![Point3::new(start, 0.0, 0.0), Point3::new(end, 0.0, 0.0)],
        rational.then(|| vec![2.0, 1.0]),
        false,
    )
    .expect("valid test line")
}

fn decoded_nurbs(curve: NurbsCurve) -> crate::curves::DecodedCurve {
    crate::curves::DecodedCurve::leaf(
        CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(curve)),
        crate::loss::Diagnostics::new(),
    )
}

fn one_child_compound() -> crate::curves::DecodedCurve {
    crate::curves::DecodedCurve::Compound {
        children: vec![(
            finite_parameter(0.0),
            decoded_nurbs(line_nurbs(0.0, 1.0, false)),
        )],
        end_parameter: finite_parameter(1.0),
        warnings: Diagnostics::new(),
    }
}

fn with_collection_limit<R>(
    limit: u64,
    f: impl FnOnce(&cadmpeg_core::decode::DecodeContext<'_>) -> R,
) -> R {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_collection_items = limit;
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("empty root is admitted");
    f(&ctx)
}

fn with_transaction_limits<R>(
    scan: &crate::container::Scan<'_>,
    collection_limit: u64,
    retained_limit: Option<u64>,
    f: impl FnOnce(crate::mesh::MeshExpand<'_>) -> R,
) -> R {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_collection_items = collection_limit;
    if let Some(retained_limit) = retained_limit {
        policy.limits.max_retained_bytes = retained_limit;
    }
    let (ctx, root) =
        cadmpeg_core::decode::DecodeContext::from_root_bytes(scan.data, &arena, &policy)
            .expect("test scan fits the root limit");
    f(crate::mesh::MeshExpand::new(&ctx, root))
}

fn with_entity_limit<R>(
    scan: &crate::container::Scan<'_>,
    limit: u64,
    f: impl FnOnce(crate::mesh::MeshExpand<'_>) -> R,
) -> R {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_entities = limit;
    let (ctx, root) =
        cadmpeg_core::decode::DecodeContext::from_root_bytes(scan.data, &arena, &policy)
            .expect("test scan fits the root limit");
    f(crate::mesh::MeshExpand::new(&ctx, root))
}

#[test]
fn point_commit_propagates_entity_limit() {
    let object = object_record_with_payload(
        ArchiveVersion::V5,
        1,
        POINT_CLASS,
        &point_payload([2.0, 0.0, 0.0]),
    );
    let mut scan = scan_with_objects(&[object]);
    set_test_units(&mut scan, 1.0);
    let refusal = with_entity_limit(&scan, 4, |expand| {
        let mut context = DecodeContext::new(&scan, expand).expect("transaction admitted");
        let result = context.decode_geometry();
        assert!(
            result.is_err(),
            "points: {}, warnings: {:?}",
            context.ir.model.points.len(),
            context.report.phase_warnings
        );
        result.expect_err("five point entities exceed four")
    });
    assert!(matches!(
        refusal,
        cadmpeg_core::CodecError::ResourceLimit(ref limit)
            if limit.operation == "rhino_instance_entities"
    ));
    with_entity_limit(&scan, 5, |expand| {
        let mut context = DecodeContext::new(&scan, expand).expect("transaction admitted");
        context.decode_geometry().expect("five point entities fit");
        assert_eq!(context.ir.model.points.len(), 1);
    });
}

#[test]
fn mesh_commit_propagates_entity_limit() {
    let object = object_record_with_payload(
        ArchiveVersion::V5,
        0x20,
        MESH_CLASS,
        &crate::test_support::test_dump::mesh_payload(),
    );
    let scan = scan_with_objects(&[object]);
    let refusal = with_entity_limit(&scan, 0, |expand| {
        let mut context = DecodeContext::new(&scan, expand).expect("transaction admitted");
        context
            .decode_geometry()
            .expect_err("mesh tessellation exceeds zero entities")
    });
    assert!(matches!(
        refusal,
        cadmpeg_core::CodecError::ResourceLimit(ref limit)
            if limit.operation == "rhino_instance_entities"
    ));
    with_entity_limit(&scan, 1, |expand| {
        let mut context = DecodeContext::new(&scan, expand).expect("transaction admitted");
        context.decode_geometry().expect("mesh tessellation fits");
        assert_eq!(context.ir.model.tessellations.len(), 1);
    });
}

#[test]
fn point_cloud_vertices_refuse_collection_limit() {
    let scan = scan_with_objects(&[object_record(ArchiveVersion::V5, 1, POINT_CLASS)]);
    let cloud = || {
        crate::curves::DecodedGeometry::PointCloud(crate::curves::PointCloud {
            points: vec![
                cadmpeg_ir::features::FinitePoint3::new(Point3::new(1.0, 2.0, 3.0))
                    .expect("finite point"),
            ],
            scaled: false,
            warnings: Diagnostics::new(),
        })
    };
    let refusal = with_transaction_limits(&scan, 4, None, |expand| {
        let mut context = DecodeContext::new(&scan, expand).expect("transaction admitted");
        context
            .commit_geometry(0, cloud())
            .expect_err("point-cloud vertex exceeds four collection items")
    });
    assert!(matches!(
        refusal,
        cadmpeg_core::CodecError::ResourceLimit(ref limit)
            if limit.operation == "Rhino point-cloud vertices"
    ));
    with_transaction_limits(&scan, 5, None, |expand| {
        let mut context = DecodeContext::new(&scan, expand).expect("transaction admitted");
        assert!(context
            .commit_geometry(0, cloud())
            .expect("vertex admitted"));
        assert_eq!(context.ir.model.vertices.len(), 1);
    });
}

#[test]
fn candidate_validation_propagates_entity_limit() {
    let scan = scan_with_objects(&[]);
    let refusal = with_entity_limit(&scan, 0, |expand| {
        let mut context = DecodeContext::new(&scan, expand).expect("transaction admitted");
        context
            .validate_candidate(|candidate, _| {
                candidate.model.points.push(Point::new(
                    "rhino:test:point#limited".try_into().expect("valid id"),
                    cadmpeg_ir::features::FinitePoint3::new(Point3::new(0.0, 0.0, 0.0))
                        .expect("finite point"),
                    Some(SourceObjectAssociation {
                        format: cadmpeg_ir::CodecFormat::Rhino,
                        object_id: cadmpeg_core::text::NonBlankString::new("point-limited")
                            .expect("nonblank source id"),
                        name: None,
                        color: None,
                        visible: None,
                        layer: None,
                        instance_path: Vec::new(),
                    }),
                ));
            })
            .expect_err("candidate entity exceeds zero")
    });
    assert!(
        matches!(
            refusal,
            CandidateError::Codec(cadmpeg_core::CodecError::ResourceLimit(ref limit))
                if limit.operation == "rhino_instance_entities"
        ),
        "{refusal:?}"
    );
}

fn assert_transaction_refusal(
    scan: &crate::container::Scan<'_>,
    collection_limit: u64,
    retained_limit: Option<u64>,
    operation: &str,
) {
    let error = with_transaction_limits(scan, collection_limit, retained_limit, |expand| {
        DecodeContext::new(scan, expand)
            .err()
            .expect("transaction exceeds the configured resource limit")
    });
    assert!(
        matches!(
            &error,
            cadmpeg_core::CodecError::ResourceLimit(refusal) if refusal.operation == operation
        ),
        "unexpected transaction refusal: {error}"
    );
}

#[test]
fn transaction_object_candidate_keys_refuse_collection_limit() {
    let scan = scan_with_objects(&[object_record(ArchiveVersion::V5, 1, POINT_CLASS)]);
    assert!(scan.objects[0].identity().is_some());
    assert_transaction_refusal(&scan, 0, None, "Rhino object candidate keys");
}

#[test]
fn transaction_object_candidate_positions_refuse_collection_limit() {
    let scan = scan_with_objects(&[object_record(ArchiveVersion::V5, 1, POINT_CLASS)]);
    assert_transaction_refusal(&scan, 1, None, "Rhino object candidate positions");
}

#[test]
fn transaction_definition_candidate_keys_refuse_collection_limit() {
    let archive = ArchiveVersion::V5;
    let payload =
        crate::test_support::test_dump::v5_definition_payload(archive, 7, [0x10; 16], &[], false);
    let record = crate::test_support::test_dump::definition_record(archive, &payload);
    let scan = crate::container::scan_owned(
        crate::test_support::test_dump::document_with_definitions("50", archive, &[record], &[]),
    )
    .expect("one definition scan");
    assert_eq!(scan.definitions.definitions().len(), 1);
    assert_transaction_refusal(&scan, 0, None, "Rhino definition candidate keys");
}

fn one_degraded_object_scan() -> crate::container::Scan<'static> {
    let mut scan = scan_with_objects(&[object_record(ArchiveVersion::V5, 1, POINT_CLASS)]);
    let range = scan.objects[0].range();
    scan.objects[0] = ObjectRecord::Degraded {
        range,
        warning: "degraded test object".to_string(),
    };
    scan
}

#[test]
fn transaction_unknown_records_refuse_collection_limit() {
    let scan = one_degraded_object_scan();
    assert_transaction_refusal(&scan, 0, None, "Rhino object unknown records");
}

#[test]
fn transaction_statuses_refuse_collection_limit() {
    let scan = one_degraded_object_scan();
    assert_transaction_refusal(&scan, 1, None, "Rhino object statuses");
}

#[test]
fn transaction_opaque_records_refuse_collection_limit() {
    let mut scan = scan_with_objects(&[]);
    scan.opaque_records.push(crate::container::OpaqueRecord {
        table_typecode: 0x1000_0013,
        record: crate::container::Record::short(0x2000_8070, 0..1, 0),
    });
    assert_transaction_refusal(&scan, 0, None, "Rhino opaque source records");
}

#[test]
fn transaction_source_record_bytes_refuse_retained_limit() {
    let scan = scan_with_objects(&[object_record(ArchiveVersion::V5, 1, POINT_CLASS)]);
    let length = scan.objects[0].range().len();
    assert!(length > 0);
    assert_transaction_refusal(
        &scan,
        100,
        Some((length - 1) as u64),
        "Rhino source record bytes",
    );
    assert!(with_expand(&scan, |expand| DecodeContext::new(
        &scan, expand
    )
    .is_ok()));
}

/// Brep staging judges values the model already holds, so a refusal there names
/// no offset instead of naming byte 0.
#[test]
fn a_brep_staging_refusal_names_no_byte() {
    let error = scaled_tolerance(
        crate::brep::BrepTolerance::new(f64::MAX).expect("valid source tolerance"),
        crate::test_support::millimeter_scale(f64::MAX),
    )
    .expect_err("overflowing scaled tolerance");
    assert!(matches!(
        error,
        crate::curves::GeometryError::Malformed(
            crate::chunks::FramingError::Unpositioned { ref message }
        ) if message == "scaled tolerance is invalid"
    ));
    assert_eq!(
        error.to_string(),
        "framing error: scaled tolerance is invalid"
    );
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

    assert_eq!(
        report.phase_warnings.messages().collect::<Vec<_>>(),
        ["existing warning"]
    );
    assert_eq!(report.phase_losses.len(), 1);
    assert_eq!(report.phase_losses[0].message, "existing parse-phase loss");
    assert_eq!(report.typed_losses.len(), 1);
    assert_eq!(report.typed_losses[0].message, "existing typed loss");
}

#[test]
fn hatch_plane_places_and_scales_plane_space_loops_once() {
    let admitted = |values| {
        crate::settings::CoordinateLane::Admitted(
            cadmpeg_ir::units::FiniteVector::new(values).expect("finite test plane"),
        )
    };
    let plane = crate::settings::Plane {
        origin: admitted([10.0, 20.0, 30.0]),
        xaxis: cadmpeg_ir::units::FiniteVector::new([0.0, 1.0, 0.0]).expect("finite test x axis"),
        yaxis: cadmpeg_ir::units::FiniteVector::new([-1.0, 0.0, 0.0]).expect("finite test y axis"),
        zaxis: cadmpeg_ir::units::FiniteVector::new([0.0, 0.0, 1.0]).expect("finite test z axis"),
        equation: crate::settings::CoordinateLane::Admitted(
            cadmpeg_ir::units::FiniteVector::new([0.0, 0.0, 1.0, -30.0])
                .expect("finite test plane equation"),
        ),
    };
    let mut curve = decoded_nurbs(line_nurbs(0.0, 2.0, false));
    with_expand_bytes(&[], |expand| {
        transform_decoded_curve(
            expand.ctx(),
            &mut curve,
            hatch_plane_transform(
                &plane,
                crate::test_support::millimeter_scale(10.0),
                "rhino hatch record #test",
            )
            .expect("a finite plane states a transform"),
        )
    })
    .expect("required invariant");
    let CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(curve)) = curve.reported_geometry() else {
        panic!("hatch loop must remain NURBS");
    };
    assert_eq!(curve.control_points()[0], Point3::new(100.0, 200.0, 300.0));
    assert_eq!(curve.control_points()[1], Point3::new(100.0, 220.0, 300.0));
}

#[test]
fn instance_line_transform_keeps_component_division_and_collapse_refusal() {
    use cadmpeg_ir::features::FinitePoint3;
    use cadmpeg_ir::geometry::analytic::LineCurve;
    use cadmpeg_ir::transform::Transform;
    use cadmpeg_ir::units::UnitVector3;

    let line = || {
        crate::curves::DecodedCurve::leaf(
            CurveGeometry::Solved(SolvedCurveGeometry::Line(LineCurve::new(
                FinitePoint3::new(Point3::new(1.0, 2.0, 3.0)).expect("finite origin"),
                UnitVector3::new(Vector3::new(0.6, 0.8, 0.0)).expect("unit direction"),
            ))),
            Diagnostics::new(),
        )
    };
    let transform = Transform::affine([
        [2.0, 0.0, 0.0, 4.0],
        [0.0, 3.0, 0.0, 5.0],
        [0.0, 0.0, 4.0, 6.0],
    ])
    .expect("finite affine transform");
    let mut decoded = line();
    with_expand_bytes(&[], |expand| {
        transform_decoded_curve(expand.ctx(), &mut decoded, transform)
    })
    .expect("transformed line");
    let CurveGeometry::Solved(SolvedCurveGeometry::Line(transformed_line)) =
        decoded.reported_geometry()
    else {
        panic!("line remains solved");
    };
    assert_eq!(
        transformed_line.origin().get(),
        Point3::new(6.0, 11.0, 18.0)
    );
    let delta = Vector3::new(
        (1.0 + 0.6) * 2.0 + 4.0 - 6.0,
        (2.0 + 0.8) * 3.0 + 5.0 - 11.0,
        0.0,
    );
    let length = delta.norm();
    assert_eq!(
        [
            transformed_line.direction().as_raw().x,
            transformed_line.direction().as_raw().y,
            transformed_line.direction().as_raw().z,
        ]
        .map(f64::to_bits),
        [delta.x / length, delta.y / length, delta.z / length].map(f64::to_bits)
    );

    let mut collapsed = line();
    let zero_linear = Transform::affine([
        [0.0, 0.0, 0.0, 0.0],
        [0.0, 0.0, 0.0, 0.0],
        [0.0, 0.0, 0.0, 0.0],
    ])
    .expect("finite collapsed transform");
    assert!(matches!(
        with_expand_bytes(&[], |expand| {
            transform_decoded_curve(expand.ctx(), &mut collapsed, zero_linear)
        }),
        Err(ReferenceFailure::Semantic(message))
            if message == "instance line transform collapsed its direction"
    ));
}

#[test]
fn instance_plane_transform_keeps_component_division() {
    use cadmpeg_ir::features::FinitePoint3;
    use cadmpeg_ir::geometry::analytic::PlaneSurface;
    use cadmpeg_ir::geometry::SurfaceGeometry;
    use cadmpeg_ir::transform::Transform;
    use cadmpeg_ir::units::{OrthonormalFrame3, UnitVector3};

    let frame = OrthonormalFrame3::from_units(
        UnitVector3::Z_AXIS,
        UnitVector3::new(Vector3::new(0.6, 0.8, 0.0)).expect("unit reference"),
    )
    .expect("orthogonal frame");
    let mut surface = Surface {
        id: "rhino:object:surface#normalized"
            .try_into()
            .expect("surface id"),
        geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(PlaneSurface::new(
            FinitePoint3::new(Point3::new(1.0, 2.0, 3.0)).expect("finite origin"),
            frame,
        ))),
        source_object: None,
    };
    let transform = Transform::affine([
        [2.0, 0.0, 0.0, 4.0],
        [0.0, 3.0, 0.0, 5.0],
        [0.0, 0.0, 4.0, 6.0],
    ])
    .expect("finite affine transform");
    transform_surface(&mut surface, transform).expect("transformed plane");
    let SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(plane)) = surface.geometry else {
        panic!("plane remains solved");
    };
    assert_eq!(plane.origin().get(), Point3::new(6.0, 11.0, 18.0));
    let delta = Vector3::new(
        (1.0 + 0.6) * 2.0 + 4.0 - 6.0,
        (2.0 + 0.8) * 3.0 + 5.0 - 11.0,
        0.0,
    );
    let length = delta.norm();
    let reference = plane.frame().reference().as_raw();
    assert_eq!(
        [reference.x, reference.y, reference.z].map(f64::to_bits),
        [delta.x / length, delta.y / length, delta.z / length].map(f64::to_bits)
    );
}

/// The region fixture resolved the way validation resolves it.
fn region_resolved(raw: &crate::brep::RawBrep) -> crate::brep::ResolvedBrep {
    crate::brep::ResolvedBrep {
        faces: vec![crate::brep::ResolvedFace {
            surface: 0,
            loops: Vec::new(),
        }],
        face_sides: raw
            .face_sides
            .iter()
            .map(|side| crate::brep::ResolvedFaceSide {
                face: usize::try_from(side.face).expect("face position"),
                region: usize::try_from(side.region).ok(),
            })
            .collect(),
        ..crate::brep::ResolvedBrep::default()
    }
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
            reversed_surface: false,
            material_channel: 0,
            uuid: None,
            color: None,
            source_range: 0..0,
        }],
        bounds: crate::settings::BoundingBox {
            minimum: crate::test_support::point3([0.0, 0.0, 0.0]),
            maximum: crate::test_support::point3([1.0, 1.0, 1.0]),
        },
        render_meshes: Vec::new(),
        analysis_meshes: Vec::new(),
        is_solid: crate::brep::RawSolidFlag::Unstamped,
        face_sides,
        regions,
        source_range: 0..0,
    }
}

fn region(region_type: i32) -> crate::brep::RawBrepRegion {
    crate::brep::RawBrepRegion {
        region_type,
        sides: Vec::new(),
        bounds: crate::settings::BoundingBox {
            minimum: crate::test_support::point3([0.0, 0.0, 0.0]),
            maximum: crate::test_support::point3([1.0, 1.0, 1.0]),
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
) -> crate::brep::RawBrepChild {
    crate::brep::RawBrepChild {
        class_uuid,
        source_range: class_data_range.clone(),
        class_data_range,
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
    let interval = finite_interval([0.0, 1.0]);
    let endpoints = [[0, 1], [1, 2], [2, 0]];
    let vertices = [[0, 2], [0, 1], [1, 2]]
        .into_iter()
        .enumerate()
        .map(|(index, edges)| crate::brep::RawBrepVertex {
            index: i32::try_from(index).expect("index"),
            point: crate::settings::CoordinateLane::Admitted(
                crate::test_support::point3([
                    f64::from((index == 1) as u8),
                    f64::from((index == 2) as u8),
                    0.0,
                ])
                .0,
            ),
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
            proxy_reversed: false,
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
            curve: Some(i32::try_from(index).expect("index")),
            proxy_domain: interval,
            edge: Some(i32::try_from(index).expect("index")),
            vertices,
            reversed_3d: false,
            trim_type: crate::brep::RawTrimKind::Boundary,
            iso: crate::brep::RawTrimIso::None,
            loop_index: 0,
            tolerances: [0.02, 0.03],
            domain: interval,
            proxy_reversed: false,
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
                    .map(|range| Some(child(line_uuid, range)))
                    .collect(),
                source_range: 0..0,
                expected_type: crate::brep::RawBrepBaseType::Curve,
            },
            c3: crate::brep::RawBrepChildren {
                slots: c3_ranges
                    .into_iter()
                    .map(|range| Some(child(line_uuid, range)))
                    .collect(),
                source_range: 0..0,
                expected_type: crate::brep::RawBrepBaseType::Curve,
            },
            surfaces: crate::brep::RawBrepChildren {
                slots: vec![Some(child(plane_uuid, surface_range))],
                source_range: 0..0,
                expected_type: crate::brep::RawBrepBaseType::Surface,
            },
            vertices,
            edges,
            trims,
            loops: vec![crate::brep::RawBrepLoop {
                index: 0,
                trims: vec![0, 1, 2],
                loop_type: crate::brep::RawLoopKind::Outer,
                face: 0,
                source_range: 0..0,
            }],
            faces: vec![crate::brep::RawBrepFace {
                index: 0,
                loops: vec![0],
                surface: 0,
                reversed_surface: false,
                material_channel: 0,
                uuid: None,
                color: None,
                source_range: 0..0,
            }],
            bounds: crate::settings::BoundingBox {
                minimum: crate::test_support::point3([0.0, 0.0, 0.0]),
                maximum: crate::test_support::point3([1.0, 1.0, 0.0]),
            },
            render_meshes: Vec::new(),
            analysis_meshes: Vec::new(),
            is_solid: crate::brep::RawSolidFlag::OutOfRange(3),
            face_sides: Vec::new(),
            regions: Vec::new(),
            source_range: 0..0,
        },
    )
}

#[test]
fn tolerance_scaling_maps_unset_and_zero_to_none() {
    let admitted = |value| crate::brep::BrepTolerance::new(value).expect("valid source tolerance");
    assert_eq!(
        scaled_tolerance(admitted(0.0), crate::test_support::millimeter_scale(25.4))
            .expect("required invariant"),
        None
    );
    assert_eq!(
        scaled_tolerance(admitted(0.5), crate::test_support::millimeter_scale(25.4))
            .expect("required invariant")
            .map(cadmpeg_ir::scalar::PositiveReal::get),
        Some(12.7)
    );
    assert_eq!(
        crate::brep::BrepTolerance::new(0.5)
            .and_then(crate::brep::BrepTolerance::fit)
            .map(cadmpeg_ir::geometry::FitTolerance::get),
        Some(0.5)
    );
    assert_eq!(crate::brep::BrepTolerance::new(-1.0), None);
}

#[test]
fn edge_proxy_reversal_normalizes_endpoints_and_keeps_an_ascending_range() {
    let edge = crate::brep::RawBrepEdge {
        index: 0,
        curve: 0,
        proxy_reversed: false,
        proxy_domain: finite_interval([3.0, 7.0]),
        vertices: [0, 1],
        trims: Vec::new(),
        tolerance: 0.0,
        domain: finite_interval([100.0, 200.0]),
        source_range: 0..0,
    };
    let resolved = crate::brep::ResolvedEdge {
        curve: 0,
        vertices: [0, 1],
        trims: Vec::new(),
        tolerance: crate::brep::BrepTolerance::new(0.0).expect("valid source tolerance"),
    };
    assert_eq!(edge_param_range(&edge), [3.0, 7.0]);
    assert_eq!(edge_vertices(&edge, &resolved), [0, 1]);
    let reversed = crate::brep::RawBrepEdge {
        proxy_reversed: true,
        ..edge
    };
    assert_eq!(edge_param_range(&reversed), [3.0, 7.0]);
    assert_eq!(edge_vertices(&reversed, &resolved), [1, 0]);
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
        vec![region(0), region(1)],
    );
    let grouping = with_expand_bytes(&[], |expand| {
        region_shell_groups(expand.ctx(), &raw, &region_resolved(&raw), &[0])
            .expect("shell-group allocation")
    });
    assert!(!grouping.fallback);
    assert_eq!(grouping.face_groups, vec![0]);
    assert_eq!(
        grouping
            .shells
            .iter()
            .map(|shell| shell.region)
            .collect::<Vec<_>>(),
        vec![1]
    );
    assert_eq!(
        grouping
            .shells
            .iter()
            .map(|shell| shell.faces.clone())
            .collect::<Vec<_>>(),
        vec![vec![0]]
    );
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
        vec![region(0), region(1), region(1)],
    );
    let grouping = with_expand_bytes(&[], |expand| {
        region_shell_groups(expand.ctx(), &raw, &region_resolved(&raw), &[0])
            .expect("shell-group allocation")
    });
    assert!(grouping.fallback);
    assert_eq!(
        grouping
            .shells
            .iter()
            .map(|shell| shell.region)
            .collect::<Vec<_>>(),
        vec![0]
    );
    assert_eq!(
        grouping
            .shells
            .iter()
            .map(|shell| shell.faces.clone())
            .collect::<Vec<_>>(),
        vec![vec![0]]
    );
}

#[test]
fn c2_polycurve_merges_clamped_rational_segments_in_parent_domain() {
    let compound = crate::curves::DecodedCurve::Compound {
        children: vec![
            (
                finite_parameter(10.0),
                decoded_nurbs(line_nurbs(0.0, 1.0, true)),
            ),
            (
                finite_parameter(20.0),
                decoded_nurbs(line_nurbs(-2.0, 2.0, false)),
            ),
        ],
        end_parameter: finite_parameter(40.0),
        warnings: Diagnostics::new(),
    };
    let merged = with_expand_bytes(&[], |expand| {
        c2_curve_to_nurbs_join(expand.ctx(), compound, 0)
    })
    .expect("merge")
    .curve;
    assert_eq!(
        merged.knots().as_slice(),
        vec![10.0, 10.0, 20.0, 40.0, 40.0]
    );
    assert_eq!(merged.control_points().len(), 3);
    assert_eq!(merged.pole_rows().weights(), Some(vec![2.0, 1.0, 1.0]));
    assert!(!merged.periodic());
}

#[test]
fn c2_joined_segments_refuse_collection_limit() {
    let compound = crate::curves::DecodedCurve::Compound {
        children: vec![(
            finite_parameter(0.0),
            decoded_nurbs(line_nurbs(0.0, 1.0, true)),
        )],
        end_parameter: finite_parameter(1.0),
        warnings: Diagnostics::new(),
    };
    let error = with_collection_limit(0, |ctx| c2_curve_to_nurbs_join(ctx, compound, 0))
        .err()
        .expect("one C2 segment exceeds zero collection items");
    assert!(matches!(
        error,
        crate::curves::GeometryError::Codec(cadmpeg_core::CodecError::ResourceLimit(refusal))
            if refusal.operation == "Rhino C2 joined segments"
    ));
}

#[test]
fn recursive_c2_polycurve_preserves_nested_parent_parameterization() {
    let nested = crate::curves::DecodedCurve::Compound {
        children: vec![
            (
                finite_parameter(0.0),
                decoded_nurbs(line_nurbs(0.0, 1.0, false)),
            ),
            (
                finite_parameter(1.0),
                decoded_nurbs(line_nurbs(0.0, 1.0, false)),
            ),
        ],
        end_parameter: finite_parameter(2.0),
        warnings: Diagnostics::new(),
    };
    let outer = crate::curves::DecodedCurve::Compound {
        children: vec![(finite_parameter(5.0), nested)],
        end_parameter: finite_parameter(9.0),
        warnings: Diagnostics::new(),
    };
    let merged = with_expand_bytes(&[], |expand| c2_curve_to_nurbs_join(expand.ctx(), outer, 0))
        .expect("nested merge")
        .curve;
    assert_eq!(merged.knots().as_slice(), vec![5.0, 5.0, 7.0, 9.0, 9.0]);
}

#[test]
fn unequal_degree_c2_polycurve_elevates_lower_degree() {
    let quadratic = NurbsCurve::from_lanes(
        2,
        vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
        vec![
            Point3::new(0.0, 0.0, 0.0),
            Point3::new(0.5, 1.0, 0.0),
            Point3::new(1.0, 0.0, 0.0),
        ],
        Some(vec![1.0, 0.5, 1.0]),
        false,
    )
    .expect("valid quadratic");
    let compound = crate::curves::DecodedCurve::Compound {
        children: vec![
            (
                finite_parameter(0.0),
                decoded_nurbs(line_nurbs(0.0, 1.0, false)),
            ),
            (finite_parameter(1.0), decoded_nurbs(quadratic)),
        ],
        end_parameter: finite_parameter(2.0),
        warnings: Diagnostics::new(),
    };
    let merged = with_expand_bytes(&[], |expand| {
        c2_curve_to_nurbs_join(expand.ctx(), compound, 0)
    })
    .expect("degree elevation")
    .curve;
    assert_eq!(merged.degree(), 2);
    assert_eq!(merged.control_points().len(), 5);
    assert_eq!(
        merged.knots().as_slice(),
        vec![0.0, 0.0, 0.0, 1.0, 1.0, 2.0, 2.0, 2.0]
    );
}

fn cap_boundary(points: &[Point3]) -> crate::extrusion::ExtrusionBoundary {
    let knots = vec![0.0, 0.0, 1.0, 2.0, 3.0, 4.0, 4.0];
    let start = NurbsCurve::from_lanes(1, knots.clone(), points.to_vec(), None, false)
        .expect("valid cap start");
    let end_points = points
        .iter()
        .map(|point| Point3::new(point.x, point.y, point.z + 5.0))
        .collect::<Vec<_>>();
    let end =
        NurbsCurve::from_lanes(1, knots.clone(), end_points, None, false).expect("valid cap end");
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
        start_nurbs: start.clone(),
        end_nurbs: end.clone(),
        start_pcurve: pcurve.clone(),
        end_pcurve: pcurve,
        lateral: crate::surfaces::extrusion_nurbs(
            &cadmpeg_test_support::service_decode_context(),
            &start,
            &end,
            cadmpeg_ir::units::FiniteVector::new([0.0, 5.0]).expect("finite path domain"),
            false,
            0,
        )
        .expect("valid cap lateral"),
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
        direction: Vector3::new(0.0, 0.0, 5.0),
        cap_origins: [Point3::new(0.0, 0.0, 0.0), Point3::new(0.0, 0.0, 5.0)],
        cap_normals: [cadmpeg_ir::units::UnitVector3::Z_AXIS; 2],
        cap_u_axes: [cadmpeg_ir::units::UnitVector3::X_AXIS; 2],
        caps,
        meshes: Vec::new(),
        warnings: Diagnostics::new(),
    }
}

fn test_association() -> SourceObjectAssociation {
    SourceObjectAssociation {
        format: cadmpeg_ir::CodecFormat::Rhino,
        object_id: cadmpeg_core::text::NonBlankString::new("extrusion".to_string())
            .expect("nonempty source identity"),
        name: Some("Extrusion".to_string()),
        color: None,
        visible: Some(true),
        layer: None,
        instance_path: Vec::new(),
    }
}

#[test]
fn extrusion_cap_admission_error_is_not_reported_as_ir_validation() {
    let object = object_record(ArchiveVersion::V5, 8, [0; 16]);
    let scan = scan_with_objects(&[object]);
    with_expand(&scan, |expand| {
        let mut context = DecodeContext::new(&scan, expand).expect("test transaction");
        let mut extrusion = cap_extrusion([true, false]);
        assert_eq!(
            cadmpeg_ir::units::UnitVector3::new(Vector3::new(0.0, 0.0, 0.0)),
            None
        );
        extrusion.cap_normals[0] = cadmpeg_ir::units::UnitVector3::X_AXIS;
        assert!(!context
            .commit_extrusion(0, extrusion)
            .expect("candidate validation completes"));
        assert!(context.report.phase_warnings.iter().any(|warning| {
            warning.contains("extrusion cap staging: PlaneSurface.normal/u_axis")
        }));
        assert!(context
            .report
            .phase_warnings
            .iter()
            .all(|warning| { !warning.contains("IR validation") }));
        assert!(context.ir.model.surfaces.is_empty());
    });
}

#[test]
fn committed_extrusion_boundaries_refuse_collection_limit() {
    let object = object_record(ArchiveVersion::V5, 8, [0; 16]);
    let scan = scan_with_objects(&[object]);
    let error = with_transaction_limits(&scan, 5, None, |expand| {
        let mut context = DecodeContext::new(&scan, expand).expect("transaction admitted");
        context
            .commit_extrusion(0, cap_extrusion([false, false]))
            .expect_err("two extrusion boundaries exceed remaining collection item")
    });
    assert!(matches!(
        error,
        cadmpeg_core::CodecError::ResourceLimit(refusal)
            if refusal.operation == "Rhino committed extrusion boundaries"
    ));
}

#[test]
fn candidate_rejections_distinguish_admission_from_validation() {
    let scan = scan_with_objects(&[]);
    with_expand(&scan, |expand| {
        let mut context = DecodeContext::new(&scan, expand).expect("test transaction");
        let admission =
            context.validate_candidate_fallible::<(), String>(|_, _| Err("admission".into()));
        assert!(
            matches!(admission, Err(CandidateError::Admission(message)) if message == "admission")
        );
        let validation = context.validate_candidate_fallible(|candidate, _| {
            let point = Point::new(
                "rhino:test:point#duplicate"
                    .try_into()
                    .expect("point identity"),
                cadmpeg_ir::features::FinitePoint3::new(Point3::new(0.0, 0.0, 0.0))
                    .expect("a finite position is a point"),
                None,
            );
            candidate.model.points.extend([point.clone(), point]);
            Ok::<(), String>(())
        });
        assert!(matches!(validation, Err(CandidateError::Validation(_))));
        assert!(context.ir.model.points.is_empty());
    });
}

#[test]
fn candidate_rejection_restores_native_records_annotations_and_all_model_arenas() {
    use cadmpeg_ir::assets::{Asset, AssetContent, AssetData};
    use cadmpeg_ir::native::NativeRecord;

    let scan = scan_with_objects(&[object_record(ArchiveVersion::V5, 1, [0; 16])]);
    for admission_failure in [true, false] {
        with_expand(&scan, |expand| {
            let mut context = DecodeContext::new(&scan, expand).expect("test transaction");
            let before_ir = context.ir.clone();
            let before_annotations = context.annotations.clone();
            let before_budget = context.expansion_budget.entities;
            let result = context.validate_candidate_fallible(|candidate, annotations| {
                candidate.model.assets.push(Asset {
                    id: "rhino:test:asset#rejected".try_into().unwrap(),
                    name: None,
                    media_type: None,
                    content: AssetContent::Embedded {
                        data: AssetData::new(vec![1]).unwrap(),
                    },
                    native_ref: None,
                });
                candidate.native.namespace_mut("rhino").arenas_mut().insert(
                    "history_records".into(),
                    vec![NativeRecord::new(
                        "rhino:history:record#rejected",
                        serde_json::Map::new(),
                    )
                    .unwrap()],
                );
                set_exactness(annotations, "rhino:test:asset#rejected", Exactness::Derived);
                if admission_failure {
                    return Err::<(), String>("source admission refusal".into());
                }
                let point = Point::new(
                    "rhino:test:point#duplicate".try_into().unwrap(),
                    cadmpeg_ir::features::FinitePoint3::new(Point3::new(0.0, 0.0, 0.0))
                        .expect("a finite position is a point"),
                    None,
                );
                candidate.model.points.extend([point.clone(), point]);
                Ok(())
            });
            assert!(result.is_err());
            assert_eq!(context.ir, before_ir);
            assert_eq!(context.annotations, before_annotations);
            assert_eq!(context.expansion_budget.entities, before_budget);
            let decoded = context
                .commit()
                .expect("rejected candidate remains committable");
            assert_eq!(decoded.ir.native_unknowns("rhino").unwrap().len(), 1);
            assert_eq!(decoded.source_fidelity.retained_records().len(), 1);
        });
    }
}

#[test]
fn successful_candidate_leaves_final_unknown_attachment_as_its_single_owner() {
    let scan = scan_with_objects(&[object_record(ArchiveVersion::V5, 1, [0; 16])]);
    with_expand(&scan, |expand| {
        let mut context = DecodeContext::new(&scan, expand).expect("test transaction");
        context
            .validate_candidate(|_, _| ())
            .expect("empty candidate admitted");
        assert!(context.ir.native_unknowns("rhino").unwrap().is_empty());
        let decoded = context.commit().expect("one final unknown attachment");
        assert_eq!(decoded.ir.native_unknowns("rhino").unwrap().len(), 1);
        assert_eq!(decoded.source_fidelity.retained_records().len(), 1);
    });
}

#[test]
fn successful_candidate_keeps_preceding_arena_order_for_instance_checkpoints() {
    let scan = scan_with_objects(&[]);
    with_expand(&scan, |expand| {
        let mut context = DecodeContext::new(&scan, expand).expect("test transaction");
        let point = |key| {
            Point::new(
                format!("rhino:test:point#{key}").try_into().unwrap(),
                cadmpeg_ir::features::FinitePoint3::new(Point3::new(0.0, 0.0, 0.0))
                    .expect("a finite position is a point"),
                Some(SourceObjectAssociation {
                    format: cadmpeg_ir::CodecFormat::Rhino,
                    object_id: cadmpeg_core::text::NonBlankString::new(format!("point-{key}"))
                        .unwrap(),
                    name: None,
                    color: None,
                    visible: None,
                    layer: None,
                    instance_path: Vec::new(),
                }),
            )
        };
        context.ir.model.points.push(point("z"));
        let checkpoint = ModelCheckpoint::capture(&context.ir.model);
        context
            .validate_candidate(|candidate, _| {
                candidate.model.points.push(point("a"));
            })
            .expect("distinct point admitted");
        assert_eq!(
            context
                .ir
                .model
                .points
                .get(checkpoint.arena_len::<Point>()..)
                .unwrap()[0]
                .id
                .as_str(),
            "rhino:test:point#a"
        );
        checkpoint.discard_appended(&mut context.ir.model);
        assert_eq!(context.ir.model.points.len(), 1);
        assert_eq!(context.ir.model.points[0].id.as_str(), "rhino:test:point#z");
    });
}

#[test]
fn extrusion_cap_staging_preserves_pcurve_rejection_details() {
    for (knots, weights, expected) in [
        (Vec::new(), None, "pcurve knot count 0"),
        (vec![0.0, 0.0], None, "knots"),
        (
            vec![0.0, 0.0, 1.0, 2.0, 3.0, 4.0, 4.0],
            Some(vec![0.0; 5]),
            "weight 0 at index 0",
        ),
    ] {
        let mut extrusion = cap_extrusion([true, false]);
        extrusion.boundaries[0].start_pcurve.knots = knots;
        extrusion.boundaries[0].start_pcurve.weights = weights;
        let boundaries = vec![CommittedExtrusionBoundary {
            boundary: &extrusion.boundaries[0],
            directrix: "rhino:test:curve#cap".try_into().expect("curve identity"),
        }];
        let error = with_collection_limit(u64::MAX, |ctx| {
            stage_extrusion_caps(
                ctx,
                &mut CadIr::empty(),
                &mut cadmpeg_ir::Annotations::default(),
                "caps",
                &test_association(),
                &extrusion,
                &boundaries,
            )
            .expect_err("invalid cap pcurve")
        });
        assert!(
            error.to_string().starts_with("extrusion cap staging: "),
            "{error}"
        );
        assert!(error.to_string().contains(expected), "{error}");
    }
}

#[test]
fn extrusion_cap_loop_ids_refuse_collection_limit() {
    let extrusion = cap_extrusion([true, false]);
    let boundaries = [CommittedExtrusionBoundary {
        boundary: &extrusion.boundaries[0],
        directrix: "rhino:test:curve#cap".try_into().expect("curve identity"),
    }];
    let error = with_collection_limit(0, |ctx| {
        stage_extrusion_caps(
            ctx,
            &mut CadIr::empty(),
            &mut cadmpeg_ir::Annotations::default(),
            "caps",
            &test_association(),
            &extrusion,
            &boundaries,
        )
        .expect_err("cap loop ID exceeds collection limit")
    });
    assert!(matches!(
        error,
        super::CandidateError::Codec(cadmpeg_core::CodecError::ResourceLimit(refusal))
            if refusal.operation == "Rhino extrusion cap loop IDs"
    ));
}

#[test]
fn extrusion_caps_build_outer_and_hole_loops_with_opposite_face_senses() {
    for (caps, expected_faces) in [([true, false], 1), ([false, true], 1), ([true, true], 2)] {
        let mut ir = CadIr::empty();
        let association = test_association();
        let extrusion = cap_extrusion(caps);
        let boundaries = extrusion
            .boundaries
            .iter()
            .enumerate()
            .map(|(index, boundary)| {
                let id: cadmpeg_ir::ids::CurveId = format!("rhino:object:curve#cap-{index}")
                    .try_into()
                    .expect("valid identity");
                ir.model.curves.push(Curve {
                    id: id.clone(),
                    geometry: CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(
                        boundary.start_nurbs.clone(),
                    )),
                    source_object: Some(association.clone()),
                });
                CommittedExtrusionBoundary {
                    boundary,
                    directrix: id,
                }
            })
            .collect::<Vec<_>>();
        with_collection_limit(u64::MAX, |ctx| {
            assert!(stage_extrusion_caps(
                ctx,
                &mut ir,
                &mut cadmpeg_ir::Annotations::default(),
                "caps",
                &association,
                &extrusion,
                &boundaries,
            )
            .is_ok());
        });
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

/// Phase 5 freeze: draft/instance admit predicates vs shared accept/reject builders.
#[test]
fn phase5_freeze_shared_admissibility_fixtures() {
    let accepted = cadmpeg_test_support::admissibility::accepted_empty();
    let rejected = cadmpeg_test_support::admissibility::rejected_missing_point("rhino:test")
        .expect("fixture identities are valid");
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
        let mut context =
            crate::decode::DecodeContext::new(&scan, expand).expect("test transaction");
        assert!(context.object(0).is_some());
        assert!(context.unknown(0).is_some());
        assert_eq!(
            context.unit_binding(),
            crate::settings::UnitBinding::Unavailable
        );
        assert_eq!(context.archive(), archive);
        assert!(context
            .append_link(0, "rhino:curve#2")
            .expect("admitted link"));
        assert!(context
            .append_link(0, "rhino:curve#1")
            .expect("admitted link"));
        assert!(context
            .append_link(0, "rhino:curve#2")
            .expect("admitted link"));
        assert_eq!(
            context.unknown(0).expect("required invariant").links(),
            vec!["rhino:curve#1".to_string(), "rhino:curve#2".to_string()]
        );
        let own_id = context
            .unknown(0)
            .expect("required invariant")
            .id()
            .to_string();
        assert!(context
            .append_links(
                0,
                &[
                    "rhino:curve#3".to_string(),
                    "rhino:curve#1".to_string(),
                    own_id,
                    "rhino:curve#0".to_string(),
                ],
            )
            .expect("admitted links"));
        assert_eq!(
            context.unknown(0).expect("required invariant").links(),
            [
                "rhino:curve#0",
                "rhino:curve#1",
                "rhino:curve#2",
                "rhino:curve#3"
            ]
        );
        assert!(context.mark_decoded(0));
        assert!(!context.mark_decoded(0));
        assert!(!context.mark_failed(0));
        assert_eq!(context.ir_mut().model.bodies.len(), 0);
        context
            .unknown_mut(0)
            .expect("required invariant")
            .links_mut()
            .clear();
        let result =
            crate::decode::seal_for_test(context.commit().expect("test decode commit"), false);
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
fn unknown_record_link_insertion_refuses_collection_limit() {
    let refusal = with_collection_limit(0, |ctx| {
        let mut record = UnknownRecord::unavailable(
            UnknownId::mint("rhino:object:unknown#0").expect("valid identity"),
            0,
            0,
            "",
            Vec::new(),
        );
        append_link_to_record(ctx, &mut record, "rhino:curve#1".to_string())
            .expect_err("one link exceeds the collection limit")
    });
    assert!(matches!(
        refusal,
        cadmpeg_core::CodecError::ResourceLimit(ref limit)
            if limit.operation == "Rhino unknown record links"
    ));
}

#[test]
fn unknown_record_link_copy_refuses_retained_limit() {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_retained_bytes = 4;
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("empty root is admitted");
    let refusal =
        copy_retained_link(&ctx, "curve").expect_err("five retained bytes exceed the limit");
    assert!(matches!(
        refusal,
        cadmpeg_core::CodecError::ResourceLimit(ref limit)
            if limit.operation == "Rhino unknown record link copy"
    ));
}

fn one_instance_link_record() -> UnknownRecord {
    UnknownRecord::unavailable(
        UnknownId::mint("rhino:object:unknown#0").expect("valid identity"),
        0,
        0,
        "",
        vec!["rhino:curve#1".to_string()],
    )
}

#[test]
fn instance_link_snapshot_rows_refuse_collection_limit() {
    let refusal = with_collection_limit(0, |ctx| {
        snapshot_instance_links(ctx, &[one_instance_link_record()])
            .err()
            .expect("one row exceeds the collection limit")
    });
    assert!(matches!(
        refusal,
        cadmpeg_core::CodecError::ResourceLimit(ref limit)
            if limit.operation == "Rhino instance link snapshot rows"
    ));
}

#[test]
fn instance_link_snapshot_entries_refuse_collection_limit() {
    let refusal = with_collection_limit(1, |ctx| {
        snapshot_instance_links(ctx, &[one_instance_link_record()])
            .err()
            .expect("one entry exceeds the collection limit")
    });
    assert!(matches!(
        refusal,
        cadmpeg_core::CodecError::ResourceLimit(ref limit)
            if limit.operation == "Rhino instance link snapshot entries"
    ));
}

#[test]
fn instance_link_snapshot_bytes_refuse_materialized_limit() {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_materialized_bytes = 12;
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("empty root is admitted");
    let refusal = snapshot_instance_links(&ctx, &[one_instance_link_record()])
        .err()
        .expect("thirteen temporary bytes exceed the limit");
    assert!(matches!(
        refusal,
        cadmpeg_core::CodecError::ResourceLimit(ref limit)
            if limit.operation == "Rhino instance link snapshot bytes"
    ));
}

#[test]
fn instance_status_snapshot_refuses_collection_limit() {
    let refusal = with_collection_limit(0, |ctx| {
        snapshot_instance_statuses(ctx, &[Some(GeometryOutcome::Decoded)])
            .expect_err("one status exceeds the collection limit")
    });
    assert!(matches!(
        refusal,
        cadmpeg_core::CodecError::ResourceLimit(ref limit)
            if limit.operation == "Rhino instance status snapshot"
    ));
}

#[test]
fn instance_status_snapshot_bytes_refuse_materialized_limit() {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    let bytes = std::mem::size_of::<Option<GeometryOutcome>>();
    policy.limits.max_materialized_bytes = cadmpeg_core::decode::u64_from_index(bytes - 1);
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("empty root is admitted");
    let refusal = snapshot_instance_statuses(&ctx, &[Some(GeometryOutcome::Decoded)])
        .expect_err("one status exceeds the temporary-byte limit");
    assert!(matches!(
        refusal,
        cadmpeg_core::CodecError::ResourceLimit(ref limit)
            if limit.operation == "Rhino instance status snapshot bytes"
    ));
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
        let mut context =
            crate::decode::DecodeContext::new(&scan, expand).expect("test transaction");
        let original = context
            .unknown(0)
            .expect("required invariant")
            .data()
            .expect("required invariant")
            .to_vec();
        let findings = context.reject_duplicate_entity_candidate();
        assert!(findings.contains("identity"));
        assert_eq!(
            context.unknown(0).expect("required invariant").data(),
            Some(original.as_slice())
        );
        assert_eq!(context.unknown_count(), 1);
        let matching = context
            .ir_mut()
            .model
            .points
            .iter()
            .filter(|point| point.id.as_str() == "rhino:test:point#duplicate")
            .collect::<Vec<_>>();
        assert_eq!(matching.len(), 1);
        assert_eq!(
            matching[0].position().get(),
            cadmpeg_ir::math::Point3::new(1.0, 2.0, 3.0)
        );
    });
}

#[test]
fn unset_settings_angular_tolerance_uses_default_and_records_repair() {
    // The settings record holds its admitted positive angular tolerance, so a
    // zero tolerance is refused before a scan can hold it and decode has no
    // repair to make.
    assert!(cadmpeg_ir::scalar::PositiveAngle::new(0.0).is_none());
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
        cadmpeg_ir::geometry::SurfaceGeometry::Solved(SolvedSurfaceGeometry::Unknown { .. })
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
    let solid_brep = crate::test_support::test_archive::object_record(
        0x10,
        crate::test_support::test_archive::BREP_CLASS,
        &crate::test_support::test_archive::solid_flagged_brep_payload(1),
    );

    let unstamped = decode_archive(crate::test_support::test_archive::archive(
        std::slice::from_ref(&solid_brep),
    ));
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
    let stamped = decode_archive(crate::test_support::test_archive::archive_writer(
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

#[test]
fn class_report_counts_terminal_outcomes_once() {
    let archive = ArchiveVersion::V5;
    let class = crate::hatch::CLASS;
    let objects = (0..5)
        .map(|_| object_record(archive, 1, class.to_wire()))
        .collect::<Vec<_>>();
    let bytes = minimal_document(
        "50",
        &[
            table(archive, 0x1000_0014, &[]),
            table(archive, 0x1000_0015, &[]),
            table(archive, 0x1000_0013, &objects),
        ],
    );
    let scan = crate::container::scan_owned(bytes).expect("object table");
    with_expand(&scan, |expand| {
        let mut context = DecodeContext::new(&scan, expand).expect("test transaction");
        assert!(context.mark_native_retained(3, RhinoLossCode::HatchFillNotTransferred));
        assert!(context.mark_native_retained(1, RhinoLossCode::HatchFillNotTransferred));
        assert!(!context.mark_native_retained(3, RhinoLossCode::HatchFillNotTransferred));
        assert!(context.mark_decoded(0));
        assert!(context.mark_failed(2));
        assert!(!context.mark_decoded(2));
        let result = seal_for_test(context.commit().expect("test decode commit"), false);
        for (code, message) in [
            (RhinoLossCode::ObjectRecordCensus, "decoded 1/5 Rhino object records".to_string()),
            (RhinoLossCode::HatchFillNotTransferred, format!("framed and read 2 object record(s) for class {class}; construction state is retained as native passthrough")),
            (RhinoLossCode::ObjectFamilyNotTransferred, format!("retained 1 object record(s) for class {class}; geometry is not decoded")),
            (RhinoLossCode::ObjectFramingUndecodable, format!("1 framed object record(s) for class {class} could not be decoded")),
        ] {
            let losses = result.report().losses.iter().filter(|loss| loss.code == code.kind()).collect::<Vec<_>>();
            assert_eq!(losses.len(), 1);
            assert_eq!(losses[0].message, message);
        }
    });
}

#[test]
fn class_report_preserves_nil_class_source_selection() {
    let archive = ArchiveVersion::V5;
    let objects = (0..3)
        .map(|_| object_record(archive, 1, [0; 16]))
        .collect::<Vec<_>>();
    let bytes = minimal_document(
        "50",
        &[
            table(archive, 0x1000_0014, &[]),
            table(archive, 0x1000_0015, &[]),
            table(archive, 0x1000_0013, &objects),
        ],
    );
    let mut scan = crate::container::scan_owned(bytes).expect("object table");
    for order in [0, 2] {
        scan.objects[order] = ObjectRecord::Degraded {
            range: scan.objects[order].range(),
            warning: "degraded test object".to_string(),
        };
    }
    for expected_source in [1, 2] {
        if expected_source == 2 {
            scan.objects[1] = ObjectRecord::Degraded {
                range: scan.objects[1].range(),
                warning: "degraded test object".to_string(),
            };
        }
        with_expand(&scan, |expand| {
            let context = DecodeContext::new(&scan, expand).expect("test transaction");
            let result = seal_for_test(context.commit().expect("test decode commit"), false);
            let loss = result
                .report()
                .losses
                .iter()
                .find(|loss| loss.code == RhinoLossCode::ObjectFramingUndecodable.kind())
                .expect("framing loss");
            assert_eq!(
                loss.provenance.as_ref().expect("source location").offset,
                scan.objects[expected_source].range().start as u64
            );
        });
    }
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

fn finite_interval(endpoints: [f64; 2]) -> crate::settings::Interval {
    crate::settings::Interval(
        cadmpeg_ir::units::FiniteVector::new(endpoints).expect("finite interval"),
    )
}

fn finite_parameter(value: f64) -> cadmpeg_ir::scalar::FiniteReal {
    cadmpeg_ir::scalar::FiniteReal::new(value).expect("finite parameter")
}

mod brep;

mod resource_limits;
