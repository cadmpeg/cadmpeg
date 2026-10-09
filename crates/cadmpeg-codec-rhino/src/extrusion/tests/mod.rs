// SPDX-License-Identifier: Apache-2.0
fn exact_orientation(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    curve: &crate::curves::DecodedCurve,
    offset: usize,
) -> Result<i8, crate::curves::GeometryError> {
    let mut storage = ctx.reserve_scoped(0, "Rhino extrusion source NURBS")?;
    let curve = storage.with_storage(|| super::exact_nurbs(ctx, curve, offset))?;
    super::nurbs_orientation(ctx, &curve, offset)
}

const EPS_MITER_DIRECTION: f64 = 1.0e-12;

use super::{
    active_miter, cap_frame, cap_pcurve, mitered_local, read_mesh_cache, read_v5_mesh_cache,
    split_profiles, transform_nurbs, ExtrusionFormat, ANONYMOUS, CLOSURE_ABSOLUTE_TOLERANCE,
    ON_V5_EXTRUSION_DISPLAY_MESH_CACHE,
};
use crate::chunks::ArchiveVersion;
use crate::curves::DecodedCurve;
use crate::curves::GeometryError;
use crate::layout::anonymous_version_prefix as anon_ver;
use crate::layout::long_chunk_header_wide as long_wide;
use crate::layout::uuid_wire_form as uuid_wire;
use crate::loss::Diagnostics;
use crate::objects::ClassUserdata;
use crate::objects::UserdataDescriptor;
use crate::settings::MillimeterScale;
use crate::test_support::test_dump::{
    crc_chunk, crc_chunk_excluding, long_chunk, push_f64, push_i32,
};
use cadmpeg_ir::features::FinitePoint3;
use cadmpeg_ir::geometry::{nurbs::NurbsCurve, CurveGeometry, SolvedCurveGeometry};
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::scalar::FiniteReal;

/// Every fixture this module builds is decoded at an archive word of 50,
/// so its chunks use the eight-byte value grammar.
const CHUNKS: ArchiveVersion = ArchiveVersion::V5;

fn with_collection_limit<R>(
    max_collection_items: u64,
    f: impl FnOnce(&cadmpeg_core::decode::DecodeContext<'_>) -> R,
) -> R {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_collection_items = max_collection_items;
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("test context input fits service profile");
    f(&ctx)
}

fn decode(
    data: &[u8],
    range: std::ops::Range<usize>,
    archive: ArchiveVersion,
    writer_version: Option<i64>,
    scale: MillimeterScale,
    mesh_budget: &mut crate::mesh::MeshBudget,
) -> Result<super::DecodedExtrusion<'static>, GeometryError> {
    crate::decode::with_expand_bytes(data, |expand| {
        let decoded = super::decode(
            expand,
            data,
            range,
            ExtrusionFormat {
                archive,
                writer_version,
                scale,
            },
            &[],
            mesh_budget,
        )?;
        let super::DecodedExtrusion {
            boundaries,
            direction,
            cap_origins,
            cap_normals,
            cap_u_axes,
            caps,
            meshes,
            warnings,
        } = decoded;
        Ok(super::DecodedExtrusion {
            boundaries,
            direction,
            cap_origins,
            cap_normals,
            cap_u_axes,
            caps,
            meshes: super::ScopedMeshList::from_test_values(meshes.into_test_values()),
            warnings,
        })
    })
}

fn polyline_wrapper(clockwise: bool, closed: bool) -> Vec<u8> {
    let mut points = vec![
        [0.0, 0.0, 0.0],
        [2.0, 0.0, 0.0],
        [2.0, 1.0, 0.0],
        [0.0, 1.0, 0.0],
    ];
    if closed {
        points.push(points[0]);
    }
    if clockwise {
        points.reverse();
    }
    let point_count = points.len();
    let mut payload = vec![0x10];
    push_i32(
        &mut payload,
        i32::try_from(point_count).expect("fixture value fits i32"),
    );
    for point in points {
        for value in point {
            push_f64(&mut payload, value);
        }
    }
    push_i32(
        &mut payload,
        i32::try_from(point_count).expect("fixture value fits i32"),
    );
    for value in 0..point_count {
        push_f64(
            &mut payload,
            f64::from(u32::try_from(value).expect("required invariant")),
        );
    }
    push_i32(&mut payload, 2);
    let wire_uuid = [
        0xe6, 0xd4, 0xd7, 0x4e, 0x47, 0xe9, 0xd3, 0x11, 0xbf, 0xe5, 0x00, 0x10, 0x83, 0x01, 0x22,
        0xf0,
    ];
    let mut class_body = crc_chunk(CHUNKS, 0x0002_fffb, &wire_uuid);
    class_body.extend(crc_chunk(CHUNKS, 0x0002_fffc, &payload));
    class_body.extend(0x8002_7fff_u32.to_le_bytes());
    class_body.extend(0_i64.to_le_bytes());
    long_chunk(CHUNKS, 0x0002_7ffa, &class_body)
}

fn polycurve_wrapper() -> Vec<u8> {
    let children = [polyline_wrapper(false, true), polyline_wrapper(true, true)];
    let mut payload = vec![0x10];
    push_i32(&mut payload, 2);
    push_i32(&mut payload, 0);
    push_i32(&mut payload, 0);
    payload.extend([0_u8; 48]);
    push_i32(&mut payload, 3);
    for value in [0.0, 1.0, 2.0] {
        push_f64(&mut payload, value);
    }
    payload.extend(children.concat());
    let wire_uuid = [
        0xe0, 0xd4, 0xd7, 0x4e, 0x47, 0xe9, 0xd3, 0x11, 0xbf, 0xe5, 0x00, 0x10, 0x83, 0x01, 0x22,
        0xf0,
    ];
    let mut class_body = crc_chunk(CHUNKS, 0x0002_fffb, &wire_uuid);
    class_body.extend(crc_chunk(CHUNKS, 0x0002_fffc, &payload));
    class_body.extend(0x8002_7fff_u32.to_le_bytes());
    class_body.extend(0_i64.to_le_bytes());
    long_chunk(CHUNKS, 0x0002_7ffa, &class_body)
}

fn payload(minor: i32, caps: [bool; 2], cache: Option<Vec<u8>>) -> Vec<u8> {
    payload_with_profile(minor, caps, cache, polyline_wrapper(false, true))
}

fn payload_with_profile(
    minor: i32,
    caps: [bool; 2],
    cache: Option<Vec<u8>>,
    profile: Vec<u8>,
) -> Vec<u8> {
    let mut body = Vec::new();
    push_i32(&mut body, 1);
    push_i32(&mut body, minor);
    let profile_range = body.len()..body.len() + profile.len();
    body.extend(profile);
    for value in [10.0, 20.0, 30.0, 10.0, 20.0, 40.0] {
        push_f64(&mut body, value);
    }
    for value in [0.25, 0.75] {
        push_f64(&mut body, value);
    }
    for value in [0.0, 1.0, 0.0] {
        push_f64(&mut body, value);
    }
    body.extend([0, 0]);
    for value in [0.0; 6] {
        push_f64(&mut body, value);
    }
    for value in [4.0, 9.0] {
        push_f64(&mut body, value);
    }
    body.push(0);
    if minor >= 1 {
        push_i32(&mut body, 1);
    }
    if minor >= 2 {
        body.push(u8::from(caps[0]));
        body.push(u8::from(caps[1]));
    }
    let mut children = vec![profile_range];
    if minor >= 3 {
        let cache = cache.unwrap_or_else(empty_mesh_cache);
        children.push(body.len()..body.len() + cache.len());
        body.extend(cache);
    }
    crc_chunk_excluding(CHUNKS, ANONYMOUS, &body, &children)
}

pub(crate) fn archive_payload(
    minor: i32,
    caps: [bool; 2],
    with_hole: bool,
    mesh_cache: bool,
) -> Vec<u8> {
    let profile = if with_hole {
        polycurve_wrapper()
    } else {
        polyline_wrapper(false, true)
    };
    let cache = (minor >= 3).then(|| {
        if mesh_cache {
            one_mesh_cache()
        } else {
            empty_mesh_cache()
        }
    });
    let profile_len = profile.len();
    let mut payload = payload_with_profile(minor, caps, cache, profile);
    if with_hole {
        let cap_bytes = usize::from(minor >= 2) * 2;
        let cache_bytes = if minor >= 3 {
            if mesh_cache {
                one_mesh_cache().len()
            } else {
                empty_mesh_cache().len()
            }
        } else {
            0
        };
        let count = payload.len() - 4 - cap_bytes - cache_bytes - 4;
        payload[count..count + 4].copy_from_slice(&2_i32.to_le_bytes());
        let body = &payload[12..payload.len() - 4];
        #[allow(clippy::single_range_in_vec_init)] // The range is one checksum child.
        let mut children = vec![8..8 + profile_len];
        if cache_bytes != 0 {
            children.push(body.len() - cache_bytes..body.len());
        }
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let ctx = cadmpeg_core::decode::DecodeContext::new(
            &arena,
            &cadmpeg_core::decode::DecodePolicy::service(),
            false,
        );
        let direct = crate::chunks::direct_checksum_ranges(&ctx, &(0..body.len()), &children)
            .expect("valid extrusion children");
        let mut hasher = crc32fast::Hasher::new();
        for range in &direct {
            hasher.update(&body[range.expect("admitted fixture checksum traversal")]);
        }
        let crc = hasher.finalize();
        let end = payload.len();
        payload[end - 4..].copy_from_slice(&crc.to_le_bytes());
    }
    payload
}

fn empty_mesh_cache() -> Vec<u8> {
    let mut body = Vec::new();
    push_i32(&mut body, 1);
    push_i32(&mut body, 0);
    body.push(0);
    crc_chunk(CHUNKS, ANONYMOUS, &body)
}

fn one_mesh_wrapper() -> Vec<u8> {
    let mut mesh = vec![0x30];
    push_i32(&mut mesh, 1);
    push_i32(&mut mesh, 0);
    for _ in 0..8 {
        push_f64(&mut mesh, 0.0);
    }
    for _ in 0..2 {
        push_f64(&mut mesh, 1.0);
    }
    for _ in 0..16 {
        mesh.extend(0.0_f32.to_le_bytes());
    }
    push_i32(&mut mesh, 0);
    mesh.extend([0, 0, 0, 0, 0]);
    push_i32(&mut mesh, 1);
    let vertex = [0_u8; 12];
    mesh.extend((u32::try_from(vertex.len()).expect("fixture value fits u32")).to_le_bytes());
    mesh.extend(crc32fast::hash(&vertex).to_le_bytes());
    mesh.push(0);
    mesh.extend(vertex);
    for _ in 0..4 {
        mesh.extend(0_u32.to_le_bytes());
    }
    let mesh_uuid = [
        0xe4, 0xd4, 0xd7, 0x4e, 0x47, 0xe9, 0xd3, 0x11, 0xbf, 0xe5, 0x00, 0x10, 0x83, 0x01, 0x22,
        0xf0,
    ];
    let mut class_body = crc_chunk(CHUNKS, 0x0002_fffb, &mesh_uuid);
    class_body.extend(crc_chunk(CHUNKS, 0x0002_fffc, &mesh));
    class_body.extend(0x8002_7fff_u32.to_le_bytes());
    class_body.extend(0_i64.to_le_bytes());
    long_chunk(CHUNKS, 0x0002_7ffa, &class_body)
}

fn one_mesh_cache() -> Vec<u8> {
    let wrapper = one_mesh_wrapper();
    let wrapper_len = wrapper.len();
    let mut item = Vec::new();
    push_i32(&mut item, 1);
    push_i32(&mut item, 0);
    item.extend([7; uuid_wire::LEN]);
    item.extend(wrapper);
    let wrapper_range =
        anon_ver::LEN + uuid_wire::LEN..anon_ver::LEN + uuid_wire::LEN + wrapper_len;
    let item = crc_chunk_excluding(CHUNKS, ANONYMOUS, &item, &[wrapper_range]);
    let item_len = item.len();
    let mut cache = Vec::new();
    push_i32(&mut cache, 1);
    push_i32(&mut cache, 0);
    cache.push(1);
    cache.extend(item);
    cache.push(0);
    #[allow(clippy::single_range_in_vec_init)] // The range is one checksum child.
    let wrapped = crc_chunk_excluding(CHUNKS, ANONYMOUS, &cache, &[9..9 + item_len]);
    wrapped
}

fn null_object_wrapper() -> Vec<u8> {
    let uuid = crc_chunk(CHUNKS, 0x0002_fffb, &[0; 16]);
    long_chunk(CHUNKS, 0x0002_7ffa, &uuid)
}

fn decoded_polygon(clockwise: bool, closed: bool) -> DecodedCurve {
    let mut points = vec![
        Point3::new(0.0, 0.0, 0.0),
        Point3::new(2.0, 0.0, 0.0),
        Point3::new(2.0, 1.0, 0.0),
        Point3::new(0.0, 1.0, 0.0),
    ];
    if closed {
        points.push(points[0]);
    }
    if clockwise {
        points.reverse();
    }
    let count = points.len();
    DecodedCurve::leaf(
        CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(
            NurbsCurve::from_lanes(
                &cadmpeg_test_support::service_decode_context(),
                1,
                (0..count + 2)
                    .map(|value| {
                        cadmpeg_core::convert::f64_from_index(value)
                            .expect("fixture index is exactly representable")
                    })
                    .collect::<Vec<f64>>(),
                points,
                None,
                false,
            )
            .expect("fixture constructor admission")
            .expect("valid polygon curve"),
        )),
        Diagnostics::new(),
    )
}

fn polygon_nurbs() -> NurbsCurve {
    let DecodedCurve::Leaf {
        geometry: CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(curve)),
        ..
    } = decoded_polygon(false, true)
    else {
        unreachable!("polygon fixture is a NURBS leaf")
    };
    curve
}

fn decoded_quadratic_circle(clockwise: bool) -> DecodedCurve {
    let radius = 2.0;
    let mut points = vec![
        Point3::new(radius, 0.0, 0.0),
        Point3::new(radius, radius, 0.0),
        Point3::new(0.0, radius, 0.0),
        Point3::new(-radius, radius, 0.0),
        Point3::new(-radius, 0.0, 0.0),
        Point3::new(-radius, -radius, 0.0),
        Point3::new(0.0, -radius, 0.0),
        Point3::new(radius, -radius, 0.0),
        Point3::new(radius, CLOSURE_ABSOLUTE_TOLERANCE / 2.0, 0.0),
    ];
    let mut weights = vec![
        1.0,
        std::f64::consts::FRAC_1_SQRT_2,
        1.0,
        std::f64::consts::FRAC_1_SQRT_2,
        1.0,
        std::f64::consts::FRAC_1_SQRT_2,
        1.0,
        std::f64::consts::FRAC_1_SQRT_2,
        1.0,
    ];
    if clockwise {
        points.reverse();
        weights.reverse();
    }
    DecodedCurve::leaf(
        CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(
            NurbsCurve::from_lanes(
                &cadmpeg_test_support::service_decode_context(),
                2,
                vec![0.0, 0.0, 0.0, 1.0, 1.0, 2.0, 2.0, 3.0, 3.0, 4.0, 4.0, 4.0],
                points,
                Some(weights),
                false,
            )
            .expect("fixture constructor admission")
            .expect("valid circle curve"),
        )),
        Diagnostics::new(),
    )
}

#[test]
fn anonymous_versions_1_0_through_1_3_apply_exact_gates_and_defaults() {
    for minor in 0..=3 {
        let bytes = payload(minor, [true, false], None);
        let decoded = decode(
            &bytes,
            0..bytes.len(),
            ArchiveVersion::V5,
            None,
            MillimeterScale::IDENTITY,
            &mut crate::mesh::MeshBudget::new(),
        )
        .expect("required invariant");
        assert_eq!(decoded.boundaries.len(), 1);
        assert_eq!(
            decoded.boundaries[0].lateral.v_knots().as_slice(),
            vec![4.0, 4.0, 9.0, 9.0]
        );
        assert_eq!(
            decoded.caps,
            if minor < 2 {
                [true, true]
            } else {
                [true, false]
            }
        );
        assert!(decoded.meshes.is_empty());
        assert!(decoded.warnings.is_empty());
    }
}

#[test]
fn nil_profile_is_rejected_before_extrusion_transfer() {
    let bytes = payload_with_profile(2, [false, false], None, null_object_wrapper());
    assert!(decode(
        &bytes,
        0..bytes.len(),
        ArchiveVersion::V5,
        None,
        MillimeterScale::IDENTITY,
        &mut crate::mesh::MeshBudget::new(),
    )
    .is_err());
}

#[test]
fn profile_frame_uses_trim_start_up_cross_path_and_scales_once() {
    let bytes = payload(2, [false, false], None);
    let decoded = decode(
        &bytes,
        0..bytes.len(),
        ArchiveVersion::V5,
        None,
        crate::test_support::millimeter_scale(25.4),
        &mut crate::mesh::MeshBudget::new(),
    )
    .expect("required invariant");
    assert_eq!(decoded.cap_origins[0], Point3::new(254.0, 508.0, 825.5));
    assert_eq!(decoded.cap_origins[1], Point3::new(254.0, 508.0, 952.5));
    let first = decoded.boundaries[0].start_nurbs.control_points()[1];
    assert_eq!(first, Point3::new(304.8, 508.0, 825.5));
    assert_eq!(decoded.direction, Vector3::new(0.0, 0.0, 127.0));
}

#[test]
fn single_open_profile_is_exact_when_uncapped_and_rejected_when_capped() {
    let open = polyline_wrapper(false, false);
    let legacy = payload_with_profile(0, [false, false], None, open.clone());
    let decoded = decode(
        &legacy,
        0..legacy.len(),
        ArchiveVersion::V5,
        None,
        MillimeterScale::IDENTITY,
        &mut crate::mesh::MeshBudget::new(),
    )
    .expect("required invariant");
    assert_eq!(decoded.caps, [false, false]);
    assert_eq!(decoded.boundaries.len(), 1);
    let capped = payload_with_profile(2, [true, false], None, open);
    assert!(decode(
        &capped,
        0..capped.len(),
        ArchiveVersion::V5,
        None,
        MillimeterScale::IDENTITY,
        &mut crate::mesh::MeshBudget::new(),
    )
    .is_err());
}

#[test]
fn profile_basis_refuses_collection_limit_before_evaluation() {
    let source = decoded_polygon(false, true);
    let curve = polygon_nurbs();
    let copied = u64::try_from(curve.knots().len() + curve.pole_count())
        .expect("fixture copy count fits u64");
    let basis_items = u64::from(curve.degree()) + 1;
    with_collection_limit(copied + basis_items - 1, |ctx| {
        let refusal = exact_orientation(ctx, &source, 0)
            .expect_err("basis exceeds the remaining collection item");
        assert!(matches!(
            refusal,
            GeometryError::Codec(cadmpeg_core::CodecError::ResourceLimit(limit))
                if limit.operation == "Rhino extrusion profile basis"
        ));
    });
    with_collection_limit(copied + basis_items, |ctx| {
        assert_eq!(
            exact_orientation(ctx, &source, 0).expect("basis admitted"),
            1
        );
    });
}

#[test]
fn orientation_supports_polygon_rational_and_open_profiles() {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let policy = cadmpeg_core::decode::DecodePolicy::default();
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("test context input fits the default profile");
    assert_eq!(
        exact_orientation(&ctx, &decoded_polygon(false, true), 0).expect("required invariant"),
        1
    );
    assert_eq!(
        exact_orientation(&ctx, &decoded_polygon(true, true), 0).expect("required invariant"),
        -1
    );
    assert_eq!(
        exact_orientation(&ctx, &decoded_polygon(false, false), 0).expect("required invariant"),
        0
    );
    assert_eq!(
        exact_orientation(&ctx, &decoded_quadratic_circle(false), 0).expect("required invariant"),
        1
    );
    assert_eq!(
        exact_orientation(&ctx, &decoded_quadratic_circle(true), 0).expect("required invariant"),
        -1
    );
    let mut off_plane = decoded_polygon(false, true);
    let crate::curves::DecodedCurve::Leaf {
        geometry: CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(curve)),
        ..
    } = &mut off_plane
    else {
        unreachable!()
    };
    curve
        .try_map_control_points(
            |index, point| {
                let mut point = point.get();
                if index == 1 {
                    point.z = 1.0;
                }
                cadmpeg_ir::features::FinitePoint3::new(point).ok_or_else(|| {
                    cadmpeg_ir::geometry::nurbs::NurbsError::Structure(
                        "control_points contains a non-finite point".into(),
                    )
                })
            },
            &cadmpeg_test_support::service_decode_context(),
        )
        .expect("pole edit admission")
        .expect("valid test curve edit");
    assert!(exact_orientation(&ctx, &off_plane, 0).is_err());
}

#[test]
fn orientation_keeps_finite_samples_across_a_wide_profile_domain() {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let policy = cadmpeg_core::decode::DecodePolicy::default();
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("test context input fits the default profile");
    let mut profile = decoded_polygon(false, true);
    let DecodedCurve::Leaf {
        geometry: CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(curve)),
        ..
    } = &mut profile
    else {
        panic!("polygon NURBS fixture");
    };
    curve
        .edit_knots(&ctx, |knots| {
            knots.copy_from_slice(&[
                -f64::MAX,
                -f64::MAX,
                -f64::MAX * 0.5,
                0.0,
                f64::MAX * 0.5,
                f64::MAX,
                f64::MAX,
            ]);
        })
        .expect("knot edit admission")
        .expect("wide polygon knot interval");
    assert_eq!(
        exact_orientation(&ctx, &profile, 0).expect("finite orientation"),
        1
    );
}

#[test]
fn multiple_profiles_require_exact_polycurve_count_and_outer_hole_orientation() {
    let outer = decoded_polygon(false, true);
    let inner = decoded_polygon(true, true);
    let finite = |value: f64| FiniteReal::new(value).expect("finite parameter");
    let profile = DecodedCurve::Compound {
        children: vec![(finite(0.0), outer), (finite(1.0), inner)],
        end_parameter: finite(2.0),
        warnings: Diagnostics::new(),
    };
    crate::decode::with_expand_bytes(&[], |expand| {
        assert_eq!(
            split_profiles(expand.ctx(), profile.clone(), 2, 0)
                .expect("required invariant")
                .len(),
            2
        );
        assert!(split_profiles(expand.ctx(), profile, 3, 0).is_err());
    });
}

#[test]
fn extrusion_profile_split_refuses_collection_limit() {
    let finite = |value: f64| FiniteReal::new(value).expect("finite parameter");
    let profile = DecodedCurve::Compound {
        children: vec![
            (finite(0.0), decoded_polygon(false, true)),
            (finite(1.0), decoded_polygon(true, true)),
        ],
        end_parameter: finite(2.0),
        warnings: Diagnostics::new(),
    };
    let refusal = with_collection_limit(1, |ctx| split_profiles(ctx, profile, 2, 0))
        .expect_err("two profiles exceed one collection item");
    assert!(matches!(
        refusal,
        GeometryError::Codec(cadmpeg_core::CodecError::ResourceLimit(limit))
            if limit.operation == "Rhino extrusion profile split"
    ));
}

#[test]
fn extrusion_single_profile_refuses_collection_limit() {
    let profile = decoded_polygon(false, true);
    let refusal = with_collection_limit(0, |ctx| split_profiles(ctx, profile, 1, 0))
        .expect_err("one profile exceeds zero collection items");
    assert!(matches!(
        refusal,
        GeometryError::Codec(cadmpeg_core::CodecError::ResourceLimit(limit))
            if limit.operation == "Rhino extrusion profile split"
    ));
}

#[test]
fn extrusion_transformed_nurbs_refuses_collection_limit() {
    let curve = polygon_nurbs();
    let needed = curve.knots().len() + curve.pole_count();
    let refusal = with_collection_limit(cadmpeg_core::decode::u64_from_index(needed - 1), |ctx| {
        transform_nurbs(
            ctx,
            &curve,
            &super::ProfileFrame {
                origin: Point3::new(0.0, 0.0, 0.0),
                xaxis: Vector3::new(1.0, 0.0, 0.0),
                yaxis: Vector3::new(0.0, 1.0, 0.0),
                zaxis: Vector3::new(0.0, 0.0, 1.0),
                miter: None,
            },
            0,
        )
    })
    .expect_err("curve copy exceeds collection limit");
    assert!(matches!(
        refusal,
        GeometryError::Codec(cadmpeg_core::CodecError::ResourceLimit(limit))
            if limit.operation == "Rhino extrusion transformed NURBS"
    ));
}

#[test]
fn extrusion_transformed_nurbs_refuses_retained_limit_one_byte_below_copy() {
    let curve = polygon_nurbs();
    let bytes = curve.knots().len() * std::mem::size_of::<f64>()
        + curve.pole_count() * std::mem::size_of::<FinitePoint3>();
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_retained_bytes = u64::try_from(bytes - 1).expect("copy size");
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("empty test input");
    let refusal = transform_nurbs(
        &ctx,
        &curve,
        &super::ProfileFrame {
            origin: Point3::new(0.0, 0.0, 0.0),
            xaxis: Vector3::new(1.0, 0.0, 0.0),
            yaxis: Vector3::new(0.0, 1.0, 0.0),
            zaxis: Vector3::new(0.0, 0.0, 1.0),
            miter: None,
        },
        0,
    )
    .expect_err("full transformed copy exceeds limit by one byte");
    assert!(matches!(
        refusal,
        GeometryError::Codec(cadmpeg_core::CodecError::ResourceLimit(limit))
            if limit.operation == "Rhino extrusion transformed NURBS"
                && limit.dimension == cadmpeg_core::decode::ResourceDimension::RetainedBytes
    ));
}

#[test]
fn extrusion_start_curve_copy_refuses_collection_limit() {
    let curve = polygon_nurbs();
    let needed = curve.knots().len() + curve.pole_count();
    let refusal = with_collection_limit(cadmpeg_core::decode::u64_from_index(needed - 1), |ctx| {
        curve.try_clone_for_decode(ctx, "Rhino extrusion start curve")
    })
    .expect_err("start curve copy exceeds collection limit");
    assert!(matches!(
        refusal,
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.operation == "Rhino extrusion start curve"
    ));
}

#[test]
fn extrusion_nurbs_copy_refuses_retained_limit_one_byte_below_full_copy() {
    let curve = polygon_nurbs();
    let bytes = curve.knots().len() * std::mem::size_of::<f64>()
        + curve.pole_count() * std::mem::size_of::<FinitePoint3>();
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_retained_bytes = u64::try_from(bytes - 1).expect("copy size");
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("empty test input");
    let error = curve
        .try_clone_for_decode(&ctx, "Rhino extrusion start curve")
        .expect_err("full copy exceeds limit by one byte");
    assert!(matches!(
        error,
        cadmpeg_core::CodecError::ResourceLimit(refusal)
            if refusal.operation == "Rhino extrusion start curve"
                && refusal.dimension == cadmpeg_core::decode::ResourceDimension::RetainedBytes
    ));
}

#[test]
fn extrusion_cap_points_refuse_collection_limit() {
    let arena = cadmpeg_core::decode::DecodeArena::default();
    let policy = cadmpeg_core::decode::DecodePolicy::default();
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("context");
    let curve = polygon_nurbs();
    let frame = cap_frame(
        &ctx,
        Vector3::new(1.0, 0.0, 0.0),
        Vector3::new(0.0, 1.0, 0.0),
        Vector3::new(0.0, 0.0, 1.0),
        None,
        0,
    )
    .expect("unit cap frame");
    let refusal = with_collection_limit(
        cadmpeg_core::decode::u64_from_index(curve.pole_count() - 1),
        |ctx| cap_pcurve(ctx, &curve, Point3::new(0.0, 0.0, 0.0), frame, 0),
    )
    .expect_err("cap points exceed collection limit");
    assert!(matches!(
        refusal,
        GeometryError::Codec(cadmpeg_core::CodecError::ResourceLimit(limit))
            if limit.operation == "Rhino extrusion cap points"
    ));
}

#[test]
fn extrusion_cap_knots_refuse_collection_limit() {
    let arena = cadmpeg_core::decode::DecodeArena::default();
    let policy = cadmpeg_core::decode::DecodePolicy::default();
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("context");
    let curve = polygon_nurbs();
    let frame = cap_frame(
        &ctx,
        Vector3::new(1.0, 0.0, 0.0),
        Vector3::new(0.0, 1.0, 0.0),
        Vector3::new(0.0, 0.0, 1.0),
        None,
        0,
    )
    .expect("unit cap frame");
    let needed = curve.pole_count() + curve.knots().len();
    let refusal = with_collection_limit(cadmpeg_core::decode::u64_from_index(needed - 1), |ctx| {
        cap_pcurve(ctx, &curve, Point3::new(0.0, 0.0, 0.0), frame, 0)
    })
    .expect_err("cap knots exceed collection limit");
    assert!(matches!(
        refusal,
        GeometryError::Codec(cadmpeg_core::CodecError::ResourceLimit(limit))
            if limit.operation == "Rhino extrusion cap knots"
    ));
}

#[test]
fn extrusion_cap_weights_refuse_collection_limit() {
    let arena = cadmpeg_core::decode::DecodeArena::default();
    let policy = cadmpeg_core::decode::DecodePolicy::default();
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("context");
    let curve = NurbsCurve::from_lanes(
        &cadmpeg_test_support::service_decode_context(),
        1,
        vec![0.0, 0.0, 1.0, 1.0],
        vec![Point3::new(0.0, 0.0, 0.0), Point3::new(1.0, 0.0, 0.0)],
        Some(vec![1.0, 0.5]),
        false,
    )
    .expect("fixture constructor admission")
    .expect("valid rational cap profile");
    let frame = cap_frame(
        &ctx,
        Vector3::new(1.0, 0.0, 0.0),
        Vector3::new(0.0, 1.0, 0.0),
        Vector3::new(0.0, 0.0, 1.0),
        None,
        0,
    )
    .expect("unit cap frame");
    let needed = curve.pole_count() * 2 + curve.knots().len();
    let refusal = with_collection_limit(cadmpeg_core::decode::u64_from_index(needed - 1), |ctx| {
        cap_pcurve(ctx, &curve, Point3::new(0.0, 0.0, 0.0), frame, 0)
    })
    .expect_err("cap weights exceed collection limit");
    assert!(matches!(
        refusal,
        GeometryError::Codec(cadmpeg_core::CodecError::ResourceLimit(limit))
            if limit.operation == "Rhino extrusion cap weights"
    ));
}

#[test]
fn strict_flags_trim_domains_and_later_minor_versions_are_accepted() {
    let valid = payload(2, [false, false], None);
    let body_start = long_wide::LEN;
    let profile_len = polyline_wrapper(false, true).len();
    let common = body_start + anon_ver::LEN + profile_len;
    let trim_start = common + 48;
    let up_start = trim_start + 16;
    let miter_flags = up_start + 24;
    let path_domain = miter_flags + 2 + 48;
    let mut cases = Vec::new();
    let mut noncanonical_bool = valid.clone();
    noncanonical_bool[miter_flags] = 2;
    assert!(decode(
        &noncanonical_bool,
        0..noncanonical_bool.len(),
        ArchiveVersion::V5,
        None,
        MillimeterScale::IDENTITY,
        &mut crate::mesh::MeshBudget::new(),
    )
    .is_ok());
    let mut bad_trim = valid.clone();
    bad_trim[trim_start..trim_start + 8].copy_from_slice(&(-0.1_f64).to_le_bytes());
    cases.push(bad_trim);
    let mut bad_domain = valid.clone();
    bad_domain[path_domain + 8..path_domain + 16].copy_from_slice(&4.0_f64.to_le_bytes());
    cases.push(bad_domain);
    for bytes in cases {
        assert!(decode(
            &bytes,
            0..bytes.len(),
            ArchiveVersion::V5,
            None,
            MillimeterScale::IDENTITY,
            &mut crate::mesh::MeshBudget::new(),
        )
        .is_err());
    }
    let mut future = payload(3, [false, false], Some(one_mesh_cache()));
    future[body_start + anon_ver::MINOR..body_start + anon_ver::LEN]
        .copy_from_slice(&4_i32.to_le_bytes());
    assert!(decode(
        &future,
        0..future.len(),
        ArchiveVersion::V5,
        None,
        MillimeterScale::IDENTITY,
        &mut crate::mesh::MeshBudget::new(),
    )
    .is_ok());
}

#[test]
fn optional_mesh_cache_over_document_budget_propagates_resource_refusal() {
    let bytes = payload(3, [false, false], Some(one_mesh_cache()));
    let refusal = decode(
        &bytes,
        0..bytes.len(),
        ArchiveVersion::V5,
        None,
        MillimeterScale::IDENTITY,
        &mut crate::mesh::MeshBudget::with_limit(0),
    )
    .expect_err("optional cache resource refusal reaches the caller");
    assert!(matches!(refusal,
        GeometryError::Codec(cadmpeg_core::CodecError::ResourceLimit(limit))
            if limit.operation == "Rhino document mesh buffer bytes"
                && limit.limit == 0 && limit.used == 0 && limit.additional == 12));
}

#[test]
fn active_miter_unitizes_and_applies_only_above_the_z_threshold() {
    let arena = cadmpeg_core::decode::DecodeArena::default();
    let policy = cadmpeg_core::decode::DecodePolicy::default();
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("context");
    assert_eq!(
        active_miter(true, Vector3::new(0.0, 1.2, 1.6)).map(Vector3::from),
        Some(Vector3::new(0.0, 0.6, 0.8))
    );
    assert_eq!(active_miter(true, Vector3::new(1.0, 0.0, 0.01)), None);
    assert_eq!(active_miter(true, Vector3::new(0.0, 0.0, 0.0)), None);
    assert!(mitered_local(
        &ctx,
        Vector3::new(1.0, 0.0, 0.0),
        active_miter(true, Vector3::new(0.0, 1.2, 1.6)),
        0
    )
    .is_ok());
    let plain = cap_frame(
        &ctx,
        Vector3::new(1.0, 0.0, 0.0),
        Vector3::new(0.0, 1.0, 0.0),
        Vector3::new(0.0, 0.0, 1.0),
        None,
        0,
    )
    .expect("required invariant");
    let mitered = cap_frame(
        &ctx,
        Vector3::new(1.0, 0.0, 0.0),
        Vector3::new(0.0, 1.0, 0.0),
        Vector3::new(0.0, 0.0, 1.0),
        active_miter(true, Vector3::new(0.0, 1.2, 1.6)),
        0,
    )
    .expect("required invariant");
    assert_eq!(Vector3::from(plain.2), Vector3::new(0.0, 0.0, 1.0));
    let mitered_normal: Vector3 = mitered.2.into();
    assert!((mitered_normal.y - 0.6).abs() < EPS_MITER_DIRECTION);
    assert!((mitered_normal.z - 0.8).abs() < EPS_MITER_DIRECTION);
}

#[test]
fn malformed_mesh_cache_is_dropped_without_losing_analytic_geometry() {
    let malformed = crc_chunk(CHUNKS, ANONYMOUS, &[1, 0, 0, 0, 0, 0, 0, 0, 2]);
    let bytes = payload(3, [false, false], Some(malformed));
    let decoded = decode(
        &bytes,
        0..bytes.len(),
        ArchiveVersion::V5,
        None,
        MillimeterScale::IDENTITY,
        &mut crate::mesh::MeshBudget::new(),
    )
    .expect("required invariant");
    assert_eq!(decoded.boundaries.len(), 1);
    assert!(decoded.meshes.is_empty());
    assert_eq!(decoded.warnings.len(), 1);
}

#[test]
fn malformed_mesh_cache_diagnostic_refuses_collection_limit() {
    let malformed = crc_chunk(CHUNKS, ANONYMOUS, &[1, 0, 0, 0, 0, 0, 0, 0, 2]);
    let bytes = payload(3, [false, false], Some(malformed));
    let mut limit = 0_u64;
    for _ in 0..128 {
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let mut policy = cadmpeg_core::decode::DecodePolicy::service();
        policy.limits.max_collection_items = limit;
        let (ctx, root) =
            cadmpeg_core::decode::DecodeContext::from_root_bytes(&bytes, &arena, &policy)
                .expect("root view");
        let refusal = super::decode(
            crate::mesh::MeshExpand::new(&ctx, root),
            &bytes,
            0..bytes.len(),
            ExtrusionFormat {
                archive: ArchiveVersion::V5,
                writer_version: None,
                scale: MillimeterScale::IDENTITY,
            },
            &[],
            &mut crate::mesh::MeshBudget::new(),
        )
        .expect_err("mesh cache warning exceeds collection limit");
        let GeometryError::Codec(cadmpeg_core::CodecError::ResourceLimit(item)) = refusal else {
            panic!("expected resource refusal, got {refusal:?}");
        };
        if item.operation == "Rhino diagnostics" {
            return;
        }
        limit = (item.used + item.additional).max(limit + 1);
    }
    panic!("mesh cache diagnostic boundary was not reached");
}

#[test]
fn optional_mesh_cache_propagates_decode_retained_limit() {
    let bytes = payload(3, [false, false], Some(one_mesh_cache()));
    let run = |cap| {
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let mut policy = cadmpeg_core::decode::DecodePolicy::service();
        policy.limits.max_retained_bytes = cap;
        let (ctx, root) =
            cadmpeg_core::decode::DecodeContext::from_root_bytes(&bytes, &arena, &policy)
                .expect("root view");
        let refusal = super::decode(
            crate::mesh::MeshExpand::new(&ctx, root),
            &bytes,
            0..bytes.len(),
            ExtrusionFormat {
                archive: ArchiveVersion::V5,
                writer_version: None,
                scale: MillimeterScale::IDENTITY,
            },
            &[],
            &mut crate::mesh::MeshBudget::new(),
        )
        .expect_err("cache output vertices exceed the retained limit");
        if let GeometryError::Codec(cadmpeg_core::CodecError::ResourceLimit(first)) = &refusal {
            assert_eq!(
                first.dimension,
                cadmpeg_core::decode::ResourceDimension::RetainedBytes
            );
            assert!(matches!(ctx.finish_session(),
            Err(cadmpeg_core::CodecError::ResourceLimit(sticky)) if sticky == *first));
        }
        refusal
    };
    let refusal = run(crate::test_support::retained_limit_at(
        "Rhino mesh scaled vertices",
        0,
        |cap| match run(cap) {
            GeometryError::Codec(cadmpeg_core::CodecError::ResourceLimit(limit)) => limit,
            error => panic!("unexpected resource refusal: {error:?}"),
        },
    ));
    assert!(matches!(
        refusal,
        GeometryError::Codec(cadmpeg_core::CodecError::ResourceLimit(limit))
            if limit.operation == "Rhino mesh scaled vertices"
    ));
}

#[test]
fn extrusion_mesh_cache_child_range_refuses_collection_limit() {
    let bytes = one_mesh_cache();
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, root) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&bytes, &arena, &policy)
        .expect("root view");
    let mut reader =
        crate::chunks::BoundedReader::new(&bytes, 0, bytes.len()).expect("valid cache range");
    let refusal = read_mesh_cache(
        crate::mesh::MeshExpand::new(&ctx, root),
        &bytes,
        &mut reader,
        ExtrusionFormat {
            archive: ArchiveVersion::V5,
            writer_version: None,
            scale: MillimeterScale::IDENTITY,
        },
        &mut crate::mesh::MeshBudget::new(),
        &mut Diagnostics::new(),
    )
    .expect_err("one cache child exceeds zero collection items");
    assert!(matches!(
        refusal,
        GeometryError::Codec(cadmpeg_core::CodecError::ResourceLimit(limit))
            if limit.operation == "Rhino extrusion mesh-cache children"
    ));

    let meshes = crate::decode::with_expand_bytes(&bytes, |expand| {
        let mut reader =
            crate::chunks::BoundedReader::new(&bytes, 0, bytes.len()).expect("valid cache range");
        read_mesh_cache(
            expand,
            &bytes,
            &mut reader,
            ExtrusionFormat {
                archive: ArchiveVersion::V5,
                writer_version: None,
                scale: MillimeterScale::IDENTITY,
            },
            &mut crate::mesh::MeshBudget::new(),
            &mut Diagnostics::new(),
        )
        .map(super::ScopedMeshList::into_test_values)
    })
    .expect("service profile admits the cache");
    assert_eq!(meshes.len(), 1);
}

#[test]
fn extrusion_mesh_cache_id_refuses_retained_limit() {
    let bytes = one_mesh_cache();
    let run = |cap| {
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let mut policy = cadmpeg_core::decode::DecodePolicy::service();
        policy.limits.max_retained_bytes = cap;
        let (ctx, root) =
            cadmpeg_core::decode::DecodeContext::from_root_bytes(&bytes, &arena, &policy)
                .expect("root view");
        let mut reader =
            crate::chunks::BoundedReader::new(&bytes, 0, bytes.len()).expect("valid cache range");
        let refusal = read_mesh_cache(
            crate::mesh::MeshExpand::new(&ctx, root),
            &bytes,
            &mut reader,
            ExtrusionFormat {
                archive: ArchiveVersion::V5,
                writer_version: None,
                scale: MillimeterScale::IDENTITY,
            },
            &mut crate::mesh::MeshBudget::new(),
            &mut Diagnostics::new(),
        )
        .expect_err("cache ID exceeds the retained vertex buffer allowance");
        refusal
    };
    let refusal = run(crate::test_support::retained_limit_at(
        "Rhino extrusion mesh-cache ID",
        0,
        |cap| match run(cap) {
            GeometryError::Codec(cadmpeg_core::CodecError::ResourceLimit(limit)) => limit,
            error => panic!("unexpected resource refusal: {error:?}"),
        },
    ));
    assert!(matches!(
        refusal,
        GeometryError::Codec(cadmpeg_core::CodecError::ResourceLimit(limit))
            if limit.operation == "Rhino extrusion mesh-cache ID"
    ));
}

#[test]
fn valid_mesh_cache_item_reuses_bounded_mesh_decoder() {
    let bytes = payload(3, [false, false], Some(one_mesh_cache()));
    let decoded = decode(
        &bytes,
        0..bytes.len(),
        ArchiveVersion::V5,
        None,
        MillimeterScale::IDENTITY,
        &mut crate::mesh::MeshBudget::new(),
    )
    .expect("required invariant");
    assert_eq!(decoded.boundaries.len(), 1);
    assert_eq!(decoded.meshes.len(), 1);
    assert!(decoded.warnings.is_empty());
}

#[test]
fn v5_mesh_cache_consumes_two_mesh_slots_and_a_null_slot() {
    let mut bytes = one_mesh_wrapper();
    bytes.extend(null_object_wrapper());
    bytes.extend(null_object_wrapper());
    let descriptor = UserdataDescriptor::Known(ClassUserdata {
        range: 0..bytes.len(),
        version: (2, 2),
        class_uuid: ON_V5_EXTRUSION_DISPLAY_MESH_CACHE,
        item_uuid: ON_V5_EXTRUSION_DISPLAY_MESH_CACHE,
        copy_count: 1,
        transform_range: 0..0,
        application_uuid: None,
        save_context: None,
        payload_range: 0..bytes.len(),
    });
    let result = crate::decode::with_expand_bytes(&bytes, |expand| {
        read_v5_mesh_cache(
            expand,
            &bytes,
            ExtrusionFormat {
                archive: ArchiveVersion::V5,
                writer_version: None,
                scale: MillimeterScale::IDENTITY,
            },
            std::slice::from_ref(&descriptor),
            &mut crate::mesh::MeshBudget::new(),
            &mut Diagnostics::new(),
        )
        .map(super::ScopedMeshList::into_test_values)
    })
    .expect("V5 mesh cache");
    assert_eq!(result.len(), 1);
}

#[test]
fn v5_extrusion_mesh_cache_id_refuses_retained_limit() {
    let mut bytes = one_mesh_wrapper();
    bytes.extend(null_object_wrapper());
    bytes.extend(null_object_wrapper());
    let descriptor = UserdataDescriptor::Known(ClassUserdata {
        range: 0..bytes.len(),
        version: (2, 2),
        class_uuid: ON_V5_EXTRUSION_DISPLAY_MESH_CACHE,
        item_uuid: ON_V5_EXTRUSION_DISPLAY_MESH_CACHE,
        copy_count: 1,
        transform_range: 0..0,
        application_uuid: None,
        save_context: None,
        payload_range: 0..bytes.len(),
    });
    let run = |cap| {
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let mut policy = cadmpeg_core::decode::DecodePolicy::service();
        policy.limits.max_retained_bytes = cap;
        let (ctx, root) =
            cadmpeg_core::decode::DecodeContext::from_root_bytes(&bytes, &arena, &policy)
                .expect("root view");
        let refusal = read_v5_mesh_cache(
            crate::mesh::MeshExpand::new(&ctx, root),
            &bytes,
            ExtrusionFormat {
                archive: ArchiveVersion::V5,
                writer_version: None,
                scale: MillimeterScale::IDENTITY,
            },
            std::slice::from_ref(&descriptor),
            &mut crate::mesh::MeshBudget::new(),
            &mut Diagnostics::new(),
        )
        .expect_err("V5 cache ID exceeds the retained vertex buffer allowance");
        refusal
    };
    let refusal = run(crate::test_support::retained_limit_at(
        "Rhino V5 extrusion mesh-cache ID",
        0,
        |cap| match run(cap) {
            GeometryError::Codec(cadmpeg_core::CodecError::ResourceLimit(limit)) => limit,
            error => panic!("unexpected resource refusal: {error:?}"),
        },
    ));
    assert!(matches!(
        refusal,
        GeometryError::Codec(cadmpeg_core::CodecError::ResourceLimit(limit))
            if limit.operation == "Rhino V5 extrusion mesh-cache ID"
    ));
}

#[test]
fn v5_mesh_cache_skips_a_bounded_suffix_after_three_slots() {
    let mut bytes = one_mesh_wrapper();
    bytes.extend(null_object_wrapper());
    bytes.extend(null_object_wrapper());
    bytes.extend([0xa5, 0x5a]);
    let descriptor = UserdataDescriptor::Known(ClassUserdata {
        range: 0..bytes.len(),
        version: (2, 2),
        class_uuid: ON_V5_EXTRUSION_DISPLAY_MESH_CACHE,
        item_uuid: ON_V5_EXTRUSION_DISPLAY_MESH_CACHE,
        copy_count: 1,
        transform_range: 0..0,
        application_uuid: None,
        save_context: None,
        payload_range: 0..bytes.len(),
    });
    let result = crate::decode::with_expand_bytes(&bytes, |expand| {
        read_v5_mesh_cache(
            expand,
            &bytes,
            ExtrusionFormat {
                archive: ArchiveVersion::V5,
                writer_version: None,
                scale: MillimeterScale::IDENTITY,
            },
            std::slice::from_ref(&descriptor),
            &mut crate::mesh::MeshBudget::new(),
            &mut Diagnostics::new(),
        )
        .map(super::ScopedMeshList::into_test_values)
    })
    .expect("V5 mesh cache suffix");
    assert_eq!(result.len(), 1);
}

#[test]
fn payload_suffix_is_skipped() {
    let mut bytes = payload(2, [false, false], None);
    let crc_offset = bytes.len() - 4;
    bytes.insert(crc_offset, 0xff);
    let body_len = i64::from_le_bytes(bytes[4..12].try_into().expect("required invariant")) + 1;
    bytes[4..12].copy_from_slice(&body_len.to_le_bytes());
    let body = &bytes[12..bytes.len() - 4];
    let crc = crc32fast::hash(body);
    let end = bytes.len();
    bytes[end - 4..].copy_from_slice(&crc.to_le_bytes());
    assert!(decode(
        &bytes,
        0..bytes.len(),
        ArchiveVersion::V5,
        None,
        MillimeterScale::IDENTITY,
        &mut crate::mesh::MeshBudget::new(),
    )
    .is_ok());
}

const SMALL_MITER_TILT: f64 = 1.0e-8;
#[test]
fn numerical_seventh_miter_preserves_a_shallow_plane_tilt() {
    let arena = cadmpeg_core::decode::DecodeArena::default();
    let policy = cadmpeg_core::decode::DecodePolicy::default();
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("context");
    let normal = super::active_miter(true, Vector3::new(SMALL_MITER_TILT, 0.0, 1.0)).unwrap();
    let point = super::mitered_local(&ctx, Vector3::new(1.0e8, 0.0, 0.0), Some(normal), 0).unwrap();
    assert!((point.z + 1.0).abs() <= 8.0 * f64::EPSILON);
    assert!(Vector3::from(normal).dot(point).abs() <= 8.0 * f64::EPSILON);
}

mod checksum_recovery;
