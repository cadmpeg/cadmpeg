use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::geometry::SurfaceGeometry;

use crate::test_support::{
    assert_unknown_visible_surface, build_prt, push_named_analytic_prototype,
};
use crate::CreoCodec;

#[test]
fn first_instance_cone_prototype_requires_a_model_space_placement() {
    let body = [
        197, 251, 126, 24, 209, 212, 112, 107, 81, 235, 133, 30, 184, 70, 125, 251, 126, 24, 209,
        212, 112, 123, 0, 68, 204, 99, 17, 228, 72, 66, 64, 192, 170, 175, 125, 232, 45, 177, 195,
        0, 68, 204, 99, 17, 220, 70, 66, 1, 69, 135, 177, 98, 82, 120, 170, 175, 125, 232, 45, 187,
        65, 200, 122, 225, 71, 174, 20, 128, 227, 24, 228, 15, 24, 15, 24, 16, 24, 228, 70, 66,
        129, 71, 174, 20, 122, 225, 25, 194, 145, 29, 33, 143, 32, 210, 52, 233, 0, 116, 33, 251,
        84, 68, 45, 5,
    ];
    let (local_system, half_angle) = body.split_at(body.len() - 7);
    let mut payload = b"srf_array\0\xf8\x01".to_vec();
    payload.extend_from_slice(&[7, 0x25, 4, 0x01, 0, 0]);
    payload.extend_from_slice(b"srf_prim_ptr(cone)\0\xe0\x02local_sys\0\xf9\x04\x03");
    payload.extend_from_slice(local_system);
    payload.extend_from_slice(b"\xe0\x01half_angle\0");
    payload.extend_from_slice(half_angle);
    payload.extend_from_slice(b"\xe0\x00parent_feats\0\xf8\x01\x04");
    payload.extend_from_slice(b"crv_array\0\xf3\xf8\0");

    let data = build_prt("c", &[("ND:0:VisibGeom:0", payload)]);
    let scan = crate::container::scan_bytes(data.clone());
    let [prototype] = scan.surfaces.prototype_records.as_slice() else {
        panic!("complete cone prototype");
    };
    assert_eq!(
        prototype.family,
        crate::surface::SurfacePrototypeFamily::Cone
    );
    assert!(crate::surface::prototype_cone_frame(prototype).is_some());
    assert_eq!(super::unique_surface_prototype_associations(&scan).len(), 1);

    let result = CreoCodec
        .decode(&mut Cursor::new(data), &DecodeOptions::default())
        .expect("decode");
    assert_unknown_visible_surface(&result.ir().model.surfaces, 7);
}

#[test]
fn later_positional_spline_replay_transfers_as_a_nurbs_surface() {
    let mut payload = b"srf_array\0\xf8\x02".to_vec();
    payload.extend_from_slice(&[7, 0x28, 4, 0x01, 0, 8, 0xe3]);
    payload.extend_from_slice(b"srf_prim_ptr(splsrf)\0");
    payload.extend_from_slice(b"\xe0\x01tan_cond\0\xf8\x02\x03\xe4");
    let mut push_vectors = |name: &str, values: &[u8]| {
        payload.extend_from_slice(&[0xe0, 0x02]);
        payload.extend_from_slice(name.as_bytes());
        payload.extend_from_slice(b"\0\xf9\x04\x03");
        payload.extend_from_slice(values);
    };
    push_vectors(
        "i_points",
        &[
            0x0f, 0x0f, 0x0f, 0x0f, 0xe4, 0x0f, 0xe4, 0x0f, 0x0f, 0xe4, 0xe4, 0x0f,
        ],
    );
    push_vectors(
        "end_u_tangts",
        &[
            0xe4, 0x0f, 0x0f, 0xe4, 0x0f, 0x0f, 0xe4, 0x0f, 0x0f, 0xe4, 0x0f, 0x0f,
        ],
    );
    push_vectors(
        "end_v_tangts",
        &[
            0x0f, 0xe4, 0x0f, 0x0f, 0xe4, 0x0f, 0x0f, 0xe4, 0x0f, 0x0f, 0xe4, 0x0f,
        ],
    );
    push_vectors("end_uv_deriv", &[0x0f; 12]);
    for name in ["u_params", "v_params"] {
        payload.extend_from_slice(&[0xe0, 0x01]);
        payload.extend_from_slice(name.as_bytes());
        payload.extend_from_slice(b"\0\xf8\x02\x0f\xe4");
    }
    payload.push(0xe3);
    payload.extend_from_slice(&[8, 0x28, 4, 0x01, 0, 0, 0xe3, 0x03, 0xe4]);
    payload.extend_from_slice(&[
        0x0f, 0x0f, 0x0f, 0x0f, 0xe4, 0x0f, 0xe4, 0x0f, 0x0f, 0xe4, 0xe4, 0x0f,
    ]);
    payload.extend_from_slice(&[
        0xe4, 0x0f, 0x0f, 0xe4, 0x0f, 0x0f, 0xe4, 0x0f, 0x0f, 0xe4, 0x0f, 0x0f,
    ]);
    payload.extend_from_slice(&[
        0x0f, 0xe4, 0x0f, 0x0f, 0xe4, 0x0f, 0x0f, 0xe4, 0x0f, 0x0f, 0xe4, 0x0f,
    ]);
    payload.extend_from_slice(&[0x0f; 12]);
    payload.extend_from_slice(&[0x0f, 0xe4, 0x0f, 0xe4, 0xe3]);
    payload.extend_from_slice(b"crv_array\0\xf3\xf8\0");

    let data = build_prt("spline-replay", &[("ND:0:VisibGeom:0", payload)]);
    let result = CreoCodec
        .decode(&mut Cursor::new(data), &DecodeOptions::default())
        .expect("decode");
    for surface_id in [7, 8] {
        let surface = result
            .ir()
            .model
            .surfaces
            .iter()
            .find(|surface| surface.id.as_str() == format!("creo:visibgeom:surface#{surface_id}"))
            .expect("spline surface");
        assert!(matches!(surface.geometry, SurfaceGeometry::Nurbs(_)));
    }
}

#[test]
fn first_instance_type26_radius_override_replaces_prototype_radii() {
    let mut payload = b"srf_array\0\xf8\x01".to_vec();
    payload.extend_from_slice(&[7, 0x26, 4, 0x01, 0, 0]);
    payload.extend_from_slice(&[
        0x18, 0x0d, 0x41, 0xcf, 0xff, 0xff, 0xff, 0xe5, 0x79, 0x7b, 0x0e, 0x29, 0xdf, 0xff,
    ]);
    payload.push(0xe3);
    push_named_analytic_prototype(&mut payload, "torus", &[("radius1", 1.0), ("radius2", 2.0)]);
    payload.extend_from_slice(b"crv_array\0\xf3\xf8\0");

    let data = build_prt("prototype-override", &[("ND:0:VisibGeom:0", payload)]);
    let result = CreoCodec
        .decode(&mut Cursor::new(data), &DecodeOptions::default())
        .expect("decode");
    let surface = result
        .ir()
        .model
        .surfaces
        .iter()
        .find(|surface| surface.id.as_str() == "creo:visibgeom:surface#7")
        .expect("first instance surface");
    let SurfaceGeometry::Torus {
        center,
        axis,
        ref_direction,
        major_radius,
        minor_radius,
    } = surface.geometry
    else {
        panic!("first instance geometry: {:?}", surface.geometry);
    };
    assert_eq!(center, [0.0, 0.0, 0.0].into());
    assert_eq!(axis, [1.0, 0.0, 0.0].into());
    assert_eq!(ref_direction, [0.0, 1.0, 0.0].into());
    assert_eq!(major_radius, 0.499_999_999_999_999_94);
    assert_eq!(minor_radius, 0.249_999_999_951_747_04);
}

#[test]
fn prototype_local_frame_rejects_nonfinite_origin() {
    let record = crate::surface::SurfacePrototypeRecord {
        declared_family: "torus".to_string(),
        family: crate::surface::SurfacePrototypeFamily::Torus,
        parameters: vec![crate::surface::SurfaceNamedParameter {
            name: "local_sys".to_string(),
            value: crate::surface::SurfaceNamedValue::ScalarArray {
                dimensions: 4,
                count: 3,
                values: [
                    1.0,
                    0.0,
                    0.0,
                    0.0,
                    1.0,
                    0.0,
                    0.0,
                    0.0,
                    1.0,
                    f64::NAN,
                    0.0,
                    0.0,
                ]
                .into_iter()
                .map(Some)
                .collect(),
                tokens: Vec::new(),
            },
            body: Vec::new(),
            offset: 0,
            value_offset: 0,
        }],
        offset: 0,
    };

    assert_eq!(super::prototype_local_frame(&record), None);
}

#[test]
fn prototype_local_frame_rejects_nonfinite_unused_support_values() {
    let record = crate::surface::SurfacePrototypeRecord {
        declared_family: "torus".to_string(),
        family: crate::surface::SurfacePrototypeFamily::Torus,
        parameters: vec![crate::surface::SurfaceNamedParameter {
            name: "local_sys".to_string(),
            value: crate::surface::SurfaceNamedValue::ScalarArray {
                dimensions: 4,
                count: 3,
                values: [
                    1.0,
                    0.0,
                    0.0,
                    f64::NAN,
                    1.0,
                    0.0,
                    0.0,
                    0.0,
                    1.0,
                    0.0,
                    0.0,
                    0.0,
                ]
                .into_iter()
                .map(Some)
                .collect(),
                tokens: Vec::new(),
            },
            body: Vec::new(),
            offset: 0,
            value_offset: 0,
        }],
        offset: 0,
    };

    assert_eq!(super::prototype_local_frame(&record), None);
}

#[test]
fn legacy_ascii_cylinder_carrier_transfers_with_canonical_units() {
    let data = r"#UGC:2 PART 1
#-END_OF_UGC_HEADER
#P_OBJECT 6
@Sld_VisGeom 1 0
@active_geom 2 0
@srf_array 3 0
@geom_type 4 1
@geom_id 5 1
@feat_id 6 1
@boundary_type 7 1
@next_geom_ptr 8 1
@orient 9 1
@srf_prim_ptr(cylinder) 10 0
@local_sys 11 2
@radius 12 2
@principal_sys_units 13 10
0 13 Inch lbm Second (Pro/E Default)
0 1 ->
1 2 ->
2 3 [1]
3 3 ->
4 4 36
4 5 42
4 6 7
4 7 0
4 8 0
4 9 1
4 10 ->
5 11 [4][3]
$3FF,0,0,0,3FF,0,0,0,3FF,0,0,0
5 12 4000000000000000
#END_OF_P_OBJECT
#Pro/ENGINEER  TM  Version H-01-21
"
    .to_owned();
    let result = CreoCodec
        .decode(
            &mut Cursor::new(data.into_bytes()),
            &DecodeOptions::default(),
        )
        .expect("legacy analytic surface decode");
    let surface = result
        .ir()
        .model
        .surfaces
        .iter()
        .find(|surface| surface.id.as_str() == "creo:visibgeom:surface#42")
        .expect("legacy cylinder surface");
    let SurfaceGeometry::Cylinder {
        origin,
        axis,
        ref_direction,
        radius,
    } = surface.geometry
    else {
        panic!("legacy surface geometry: {:?}", surface.geometry);
    };
    assert_eq!(origin, [0.0, 0.0, 0.0].into());
    assert_eq!(axis, [0.0, 0.0, 1.0].into());
    assert_eq!(ref_direction, [1.0, 0.0, 0.0].into());
    assert_eq!(radius, 50.8);
    assert_eq!(
        result.report().coverage["transferred_legacy_ascii_surface_carrier_count"],
        1
    );
    assert_eq!(
        result.report().coverage["untransferred_visible_surface_row_count"],
        0
    );
}

#[test]
fn legacy_ascii_cone_carrier_transfers_signed_apex_frame() {
    let data = r"#UGC:2 PART 1
#-END_OF_UGC_HEADER
#P_OBJECT 6
@Sld_VisGeom 1 0
@active_geom 2 0
@srf_array 3 0
@geom_type 4 1
@geom_id 5 1
@feat_id 6 1
@boundary_type 7 1
@next_geom_ptr 8 1
@orient 9 1
@srf_prim_ptr(cone) 10 0
@local_sys 11 2
@half_angle 12 2
@principal_sys_units 13 10
0 13 millimeter Newton Second (mmNs)
0 1 ->
1 2 ->
2 3 [1]
3 3 ->
4 4 37
4 5 42
4 6 7
4 7 0
4 8 0
4 9 1
4 10 ->
5 11 [4][3]
$3FF,0,0,0,3FF,0,0,0,3FF,3FF0000000000000,4000000000000000,4008000000000000
5 12 BFE921FB54442D18
#END_OF_P_OBJECT
#Pro/ENGINEER  TM  Version H-01-21
"
    .to_owned();
    let result = CreoCodec
        .decode(
            &mut Cursor::new(data.into_bytes()),
            &DecodeOptions::default(),
        )
        .expect("legacy cone decode");
    let surface = result
        .ir()
        .model
        .surfaces
        .iter()
        .find(|surface| surface.id.as_str() == "creo:visibgeom:surface#42")
        .expect("legacy cone surface");
    let SurfaceGeometry::Cone {
        origin,
        axis,
        ref_direction,
        radius,
        ratio,
        half_angle,
    } = surface.geometry
    else {
        panic!("legacy surface geometry: {:?}", surface.geometry);
    };
    assert_eq!(origin, [1.0, 2.0, 3.0].into());
    assert_eq!(axis, [0.0, 0.0, -1.0].into());
    assert_eq!(ref_direction, [1.0, 0.0, 0.0].into());
    assert_eq!(radius, 0.0);
    assert_eq!(ratio, 1.0);
    assert_eq!(half_angle, std::f64::consts::FRAC_PI_4);
    assert_eq!(
        result.report().coverage["transferred_legacy_ascii_surface_carrier_count"],
        1
    );
}

#[test]
fn legacy_ascii_plane_carrier_transfers_row_major_origin_and_axes() {
    let data = r"#UGC:2 PART 1
#-END_OF_UGC_HEADER
#P_OBJECT 6
@Sld_VisGeom 1 0
@active_geom 2 0
@srf_array 3 0
@geom_type 4 1
@geom_id 5 1
@feat_id 6 1
@boundary_type 7 1
@next_geom_ptr 8 1
@orient 9 1
@srf_prim_ptr(plane) 10 0
@local_sys 11 2
@principal_sys_units 12 10
0 12 millimeter Newton Second (mmNs)
0 1 ->
1 2 ->
2 3 [1]
3 3 ->
4 4 34
4 5 42
4 6 7
4 7 0
4 8 0
4 9 1
4 10 ->
5 11 [4][3]
$3FF,0,0,0,3FF,0,0,0,3FF,3FF0000000000000,4000000000000000,4008000000000000
#END_OF_P_OBJECT
#Pro/ENGINEER  TM  Version H-01-21
"
    .to_owned();
    let result = CreoCodec
        .decode(
            &mut Cursor::new(data.into_bytes()),
            &DecodeOptions::default(),
        )
        .expect("legacy analytic plane decode");
    let surface = result
        .ir()
        .model
        .surfaces
        .iter()
        .find(|surface| surface.id.as_str() == "creo:visibgeom:surface#42")
        .expect("legacy plane surface");
    let SurfaceGeometry::Plane {
        origin,
        normal,
        u_axis,
    } = surface.geometry
    else {
        panic!("legacy surface geometry: {:?}", surface.geometry);
    };
    assert_eq!(origin, [1.0, 2.0, 3.0].into());
    assert_eq!(normal, [0.0, 0.0, 1.0].into());
    assert_eq!(u_axis, [1.0, 0.0, 0.0].into());
    assert_eq!(
        result.report().coverage["transferred_legacy_ascii_surface_carrier_count"],
        1
    );
}

#[test]
fn legacy_ascii_spline_carrier_transfers_complete_interpolation_arrays() {
    let real_array = |values: &[f64]| {
        values
            .iter()
            .map(|value| format!("{:016X}", value.to_bits()))
            .collect::<Vec<_>>()
            .join(",")
    };
    let data = format!(
        r"#UGC:2 PART 1
#-END_OF_UGC_HEADER
#P_OBJECT 6
@Sld_VisGeom 1 0
@active_geom 2 0
@srf_array 3 0
@geom_type 4 1
@geom_id 5 1
@feat_id 6 1
@boundary_type 7 1
@next_geom_ptr 8 1
@orient 9 1
@srf_prim_ptr(splsrf) 10 0
@tan_cond 11 1
@i_points 12 2
@u_params 13 2
@v_params 14 2
@u_tangts 15 2
@v_tangts 16 2
@uv_deriv 17 2
@principal_sys_units 18 10
0 18 millimeter Newton Second (mmNs)
0 1 ->
1 2 ->
2 3 [1]
3 3 ->
4 4 40
4 5 42
4 6 7
4 7 0
4 8 0
4 9 1
4 10 ->
5 11 [2]
0 0
5 12 [4][3]
${}
5 13 [2]
${}
5 14 [2]
${}
5 15 [4][3]
${}
5 16 [4][3]
${}
5 17 [4][3]
${}
#END_OF_P_OBJECT
#Pro/ENGINEER  TM  Version H-01-21
",
        real_array(&[0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 1.0, 1.0, 0.0,]),
        real_array(&[0.0, 1.0]),
        real_array(&[0.0, 1.0]),
        real_array(&[1.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 0.0, 0.0,]),
        real_array(&[0.0, 1.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 0.0,]),
        real_array(&[0.0; 12]),
    );
    let result = CreoCodec
        .decode(
            &mut Cursor::new(data.into_bytes()),
            &DecodeOptions::default(),
        )
        .expect("legacy spline surface decode");
    let surface = result
        .ir()
        .model
        .surfaces
        .iter()
        .find(|surface| surface.id.as_str() == "creo:visibgeom:surface#42")
        .expect("legacy spline surface");
    let SurfaceGeometry::Nurbs(surface) = &surface.geometry else {
        panic!("legacy spline geometry: {:?}", surface.geometry);
    };
    assert_eq!(surface.control_points.len(), 16);
    assert_eq!(surface.u_count, 4);
    assert_eq!(surface.v_count, 4);
    assert_eq!(
        result.report().coverage["transferred_legacy_ascii_surface_carrier_count"],
        1
    );
    assert_eq!(
        result.report().coverage["transferred_visible_spline_surface_row_count"],
        1
    );
}
