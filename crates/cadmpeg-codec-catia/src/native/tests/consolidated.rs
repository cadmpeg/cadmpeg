// SPDX-License-Identifier: Apache-2.0
//! Native-namespace tests for consolidated family layouts.

#![allow(clippy::doc_markdown, clippy::unwrap_used)]

use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeOptions};

use crate::test_support::*;
use crate::CatiaCodec;

#[test]
fn native_namespace_retains_unbound_consolidated_pcurve_jets() {
    let mut bytes = Vec::new();
    for _ in 0..6 {
        bytes.extend(a5_pcurve_stream());
        bytes.extend(b2_pcurve_stream());
    }
    let native = crate::native::CatiaNative::decode(&bytes);

    assert_eq!(native.consolidated_pcurves.len(), 12);
    assert_eq!(
        native.consolidated_pcurves[0].family,
        crate::native::CatiaConsolidatedFamily::A
    );
    assert_eq!(
        native.consolidated_pcurves[1].family,
        crate::native::CatiaConsolidatedFamily::B
    );
    assert_eq!(native.consolidated_pcurves[0].support_id, 0x1234);
    assert_eq!(
        native.consolidated_pcurves[0].points,
        vec![[0.0, 0.0], [1.0, 1.0]]
    );

    let mut namespace = cadmpeg_ir::NativeNamespace::new(std::num::NonZeroU32::MIN);
    native.store(&mut namespace).expect("store CATIA pcurves");
    assert_eq!(
        crate::native::CatiaNative::load(&namespace).expect("load CATIA pcurves"),
        native
    );

    let mut invalid = native;
    invalid.consolidated_pcurves[0].degree = 4;
    let mut invalid_namespace = cadmpeg_ir::NativeNamespace::new(std::num::NonZeroU32::MIN);
    invalid
        .store(&mut invalid_namespace)
        .expect("store invalid CATIA pcurve for load validation");
    assert!(crate::native::CatiaNative::load(&invalid_namespace).is_err());
}

#[test]
fn native_namespace_retains_typed_consolidated_groups() {
    let native = crate::native::CatiaNative::decode(&b2_group_stream());
    let [group] = native.consolidated_groups.as_slice() else {
        panic!("one consolidated group")
    };
    assert_eq!(group.byte_offset, 9);
    assert_eq!(group.group_type, 3);

    let mut namespace = cadmpeg_ir::NativeNamespace::new(std::num::NonZeroU32::MIN);
    native
        .store(&mut namespace)
        .expect("store CATIA consolidated groups");
    assert_eq!(
        crate::native::CatiaNative::load(&namespace).expect("load CATIA consolidated groups"),
        native
    );

    let mut invalid = native;
    invalid.consolidated_groups[0].id.push_str("-changed");
    let mut invalid_namespace = cadmpeg_ir::NativeNamespace::new(std::num::NonZeroU32::MIN);
    invalid
        .store(&mut invalid_namespace)
        .expect("store invalid CATIA consolidated group for load validation");
    assert!(crate::native::CatiaNative::load(&invalid_namespace).is_err());
}

#[test]
fn native_namespace_retains_consolidated_class61_records() {
    let mut stream = b2_counted_61_stream();
    stream.extend_from_slice(&b2_long_61_stream());
    let native = crate::native::CatiaNative::decode(&stream);
    let [counted, long] = native.consolidated_class61_records.as_slice() else {
        panic!("two consolidated class-0x61 records")
    };
    let crate::native::CatiaConsolidatedClass61Payload::Counted { references, tail } =
        &counted.payload
    else {
        panic!("counted class-0x61 record")
    };
    assert_eq!(references, &[1300, 1294, 30, 74]);
    assert_eq!(tail, &[0x41, 0x03]);
    let crate::native::CatiaConsolidatedClass61Payload::Long {
        prefix,
        members,
        references,
        scalar,
    } = &long.payload
    else {
        panic!("long class-0x61 record")
    };
    assert_eq!(prefix, &[0xb5, 0x03, 0x2b, 0x47, 0x8f, 0xb3, 0xd7, 0xfb]);
    assert_eq!(members, &[0x064a, 0x0650, 0x0656]);
    assert_eq!(references, &[0x0100, 0x0103, 0x0106, 0x0109, 0x010c]);
    assert_eq!(*scalar, 42.5);

    let mut namespace = cadmpeg_ir::NativeNamespace::new(std::num::NonZeroU32::MIN);
    native
        .store(&mut namespace)
        .expect("store CATIA class-0x61 records");
    assert_eq!(
        crate::native::CatiaNative::load(&namespace).expect("load CATIA class-0x61 records"),
        native
    );

    let mut invalid = native;
    let crate::native::CatiaConsolidatedClass61Payload::Long { members, .. } =
        &mut invalid.consolidated_class61_records[1].payload
    else {
        panic!("long class-0x61 record")
    };
    members.swap(0, 1);
    let mut invalid_namespace = cadmpeg_ir::NativeNamespace::new(std::num::NonZeroU32::MIN);
    invalid
        .store(&mut invalid_namespace)
        .expect("store invalid CATIA class-0x61 record for load validation");
    assert!(crate::native::CatiaNative::load(&invalid_namespace).is_err());
}

#[test]
fn native_namespace_retains_class5b5c_control_records_without_assigning_roles() {
    let native = crate::native::CatiaNative::decode(&b2_class5b5c_stream());
    let records = &native.consolidated_class5b5c_records;
    assert_eq!(records.len(), 3);
    assert_eq!(
        records
            .iter()
            .map(|record| record.class)
            .collect::<Vec<_>>(),
        [0x5b, 0x5c, 0x5b]
    );
    assert_eq!(records[0].source_index, 0);
    assert_eq!(records[0].source_offset, records[0].byte_offset);
    assert_eq!(records[1].width, 2);
    assert!(records.iter().all(|record| !record.payload.is_empty()));

    let mut namespace = cadmpeg_ir::NativeNamespace::new(std::num::NonZeroU32::MIN);
    native
        .store(&mut namespace)
        .expect("store CATIA class-0x5b/0x5c records");
    assert_eq!(
        crate::native::CatiaNative::load(&namespace).expect("load CATIA class-0x5b/0x5c records"),
        native
    );

    let mut invalid = native;
    invalid.consolidated_class5b5c_records[0].class = 0x5a;
    let mut invalid_namespace = cadmpeg_ir::NativeNamespace::new(std::num::NonZeroU32::MIN);
    invalid
        .store(&mut invalid_namespace)
        .expect("store invalid CATIA class-0x5b/0x5c record");
    assert!(crate::native::CatiaNative::load(&invalid_namespace).is_err());
}

#[test]
fn native_namespace_retains_all_consolidated_parameter_point_layouts() {
    let native = crate::native::CatiaNative::decode(&b2_parameter_point_stream());
    let [uv, station_uv, five_scalars, station_uv_last] =
        native.consolidated_parameter_points.as_slice()
    else {
        panic!("four consolidated parameter points")
    };
    assert_eq!(
        [
            uv.prefix.as_u8(),
            station_uv.prefix.as_u8(),
            five_scalars.prefix.as_u8(),
            station_uv_last.prefix.as_u8()
        ],
        [0x05, 0x09, 0x0d, 0x11]
    );
    assert_eq!(uv.payload.layout(), 0x12);
    assert_eq!(uv.control, 0x12);
    assert!(matches!(
        &uv.payload,
        crate::native::CatiaConsolidatedParameterPointPayload::Uv { uv: [2.0, 3.0] }
    ));
    assert_eq!(station_uv.payload.layout(), 0x1a);
    assert!(matches!(
        &station_uv.payload,
        crate::native::CatiaConsolidatedParameterPointPayload::StationUv {
            station: 11.0,
            uv: [4.0, 5.0],
        }
    ));
    assert_eq!(five_scalars.payload.layout(), 0x2a);
    assert!(matches!(
        &five_scalars.payload,
        crate::native::CatiaConsolidatedParameterPointPayload::FiveScalars {
            values: [1.0, 2.0, 3.0, 4.0, 5.0],
        }
    ));

    let mut namespace = cadmpeg_ir::NativeNamespace::new(std::num::NonZeroU32::MIN);
    native
        .store(&mut namespace)
        .expect("store CATIA parameter points");
    assert_eq!(
        crate::native::CatiaNative::load(&namespace).expect("load CATIA parameter points"),
        native
    );
}

#[test]
fn native_namespace_retains_all_consolidated_plane_carrier_layouts() {
    let plane_stream = b2_plane_carrier_stream();
    let native = crate::native::CatiaNative::decode(&plane_stream);
    let [direction2, direction3, tail] = native.consolidated_plane_carriers.as_slice() else {
        panic!("three consolidated plane carriers")
    };
    assert_eq!(
        [
            direction2.payload.selector(),
            direction3.payload.selector(),
            tail.payload.selector()
        ],
        [0xe4, 0xc4, 0xec]
    );
    assert!(matches!(
        &direction2.payload,
        crate::native::CatiaConsolidatedPlaneCarrierPayload::PointDirection2 {
            point: [10.0, 20.0],
            direction: [1.0, 0.0],
            tail: [5.0, -2.0, 3.0],
        }
    ));
    assert!(matches!(
        &direction3.payload,
        crate::native::CatiaConsolidatedPlaneCarrierPayload::PointDirection3 {
            point: [10.0, 20.0],
            direction: [1.0, 0.0, 0.0],
            tail: [5.0, -2.0, 3.0],
        }
    ));
    assert!(matches!(
        &tail.payload,
        crate::native::CatiaConsolidatedPlaneCarrierPayload::PointTail {
            point: [10.0, 20.0],
            tail: [-2.0, 5.0, -2.0, 3.0],
        }
    ));

    let mut namespace = cadmpeg_ir::NativeNamespace::new(std::num::NonZeroU32::MIN);
    native
        .store(&mut namespace)
        .expect("store CATIA plane carriers");
    assert_eq!(
        crate::native::CatiaNative::load(&namespace).expect("load CATIA plane carriers"),
        native
    );

    let mut file = standard_catpart();
    file.splice(16..16, plane_stream);
    let file_len = u32::try_from(file.len()).expect("bounded CATPart fixture");
    file[8..12].copy_from_slice(&be32(file_len));
    let decoded = CatiaCodec
        .decode(&mut Cursor::new(file), &DecodeOptions::default())
        .expect("decode CATIA plane carrier coverage");
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_CONSOLIDATED_PLANE_CARRIER_COUNT),
        3
    );
}

#[test]
fn native_namespace_retains_unclassified_consolidated_plane_carrier_lanes() {
    let mut stream = b2_plane_carrier_stream();
    let values = [1.0, 2.0, 3.0, 4.0, 5.0, 6.0];
    stream.extend_from_slice(&[
        0xb2,
        0x03,
        0x27,
        2 + u8::try_from(values.len() * 8).expect("scalar lane fixture"),
        0x05,
        0xb4,
        0x40,
    ]);
    for value in values {
        stream.extend_from_slice(&le_f64(value));
    }

    let native = crate::native::CatiaNative::decode(&stream);
    let Some(carrier) = native.consolidated_plane_carriers.get(3) else {
        panic!("unclassified consolidated plane carrier")
    };
    assert_eq!(carrier.payload.selector(), 0x40);
    assert!(matches!(
        &carrier.payload,
        crate::native::CatiaConsolidatedPlaneCarrierPayload::ScalarLane { values: lane, .. }
            if lane == &values
    ));

    let mut namespace = cadmpeg_ir::NativeNamespace::new(std::num::NonZeroU32::MIN);
    native
        .store(&mut namespace)
        .expect("store unclassified plane carrier");
    assert_eq!(
        crate::native::CatiaNative::load(&namespace).expect("load unclassified plane carrier"),
        native
    );
}

#[test]
fn native_namespace_retains_consolidated_reference_lists() {
    let native = crate::native::CatiaNative::decode(&b2_reference_list_stream());
    let [list] = native.consolidated_reference_lists.as_slice() else {
        panic!("one consolidated reference list")
    };
    assert_eq!(list.references, (0u32..26).collect::<Vec<_>>());

    let mut namespace = cadmpeg_ir::NativeNamespace::new(std::num::NonZeroU32::MIN);
    native
        .store(&mut namespace)
        .expect("store CATIA reference list");
    assert_eq!(
        crate::native::CatiaNative::load(&namespace).expect("load CATIA reference list"),
        native
    );

    let mut invalid = native;
    invalid.consolidated_reference_lists[0].references.clear();
    let mut invalid_namespace = cadmpeg_ir::NativeNamespace::new(std::num::NonZeroU32::MIN);
    invalid
        .store(&mut invalid_namespace)
        .expect("store invalid CATIA reference list");
    assert!(crate::native::CatiaNative::load(&invalid_namespace).is_err());
}

#[test]
fn native_namespace_retains_standalone_consolidated_circle_supports() {
    let native = crate::native::CatiaNative::decode(&b2_circle_stream());
    let [circle] = native.consolidated_circles.as_slice() else {
        panic!("one consolidated circle")
    };
    assert_eq!(
        circle.layout,
        crate::native::CatiaCircleLayout::Identity16Bit
    );
    assert_eq!(circle.record_id, 0x1234);
    assert_eq!(circle.frame_token, 0x05);
    assert_eq!(circle.center_pair, [4.0, -2.0]);
    assert_eq!(circle.radius, 3.0);
    assert_eq!(circle.range, [0.0, std::f64::consts::TAU * circle.radius]);
    assert!(circle.full_circle);
    assert_eq!(circle.chart_shift, 0.0);

    let mut namespace = cadmpeg_ir::NativeNamespace::new(std::num::NonZeroU32::MIN);
    native.store(&mut namespace).expect("store CATIA circle");
    assert_eq!(
        crate::native::CatiaNative::load(&namespace).expect("load CATIA circle"),
        native
    );

    let mut invalid = native;
    invalid.consolidated_circles[0].full_circle = false;
    let mut invalid_namespace = cadmpeg_ir::NativeNamespace::new(std::num::NonZeroU32::MIN);
    invalid
        .store(&mut invalid_namespace)
        .expect("store invalid CATIA circle for load validation");
    assert!(crate::native::CatiaNative::load(&invalid_namespace).is_err());
}

#[test]
fn native_namespace_retains_all_consolidated_cylinder_layouts() {
    let mut stream = b2_cylinder_stream();
    stream.extend_from_slice(&b2_implicit_axis_cylinder_stream());
    stream.extend_from_slice(&b2_range_origin_cylinder_stream());
    let native = crate::native::CatiaNative::decode(&stream);
    let [explicit, implicit, range_origin] = native.consolidated_cylinders.as_slice() else {
        panic!("three consolidated cylinders")
    };
    assert_eq!(explicit.payload.layout(), 0x5a);
    assert_eq!(explicit.origin, [1.0, 2.0, 3.0]);
    assert_eq!(explicit.radius, 2.0);
    assert!(matches!(
        explicit.payload,
        crate::native::CatiaConsolidatedCylinderPayload::Layout5a {
            frame_token: 0x19,
            axis: [1.0, 0.0, 0.0],
            reference_direction: [0.0, 1.0, 0.0],
        }
    ));
    assert_eq!(implicit.payload.layout(), 0x52);
    assert!(matches!(
        implicit.payload,
        crate::native::CatiaConsolidatedCylinderPayload::Layout52 { .. }
    ));
    assert_eq!(range_origin.payload.layout(), 0x62);
    assert_eq!(range_origin.radius, 4.0);
    assert!(matches!(
        range_origin.payload,
        crate::native::CatiaConsolidatedCylinderPayload::RangeOrigin {
            stored_vector: [0.0, 1.0],
            axis: [0.0, 1.0, 0.0],
            reference_direction: [0.0, 0.0, 1.0],
            range_origin,
        } if range_origin.to_bits()
            == ((0.0 + 8.0) * 0.5 - std::f64::consts::PI * 4.0).to_bits()
    ));

    let mut namespace = cadmpeg_ir::NativeNamespace::new(std::num::NonZeroU32::MIN);
    native.store(&mut namespace).expect("store CATIA cylinders");
    assert_eq!(
        crate::native::CatiaNative::load(&namespace).expect("load CATIA cylinders"),
        native
    );

    let mut invalid = native;
    let crate::native::CatiaConsolidatedCylinderPayload::RangeOrigin { range_origin, .. } =
        &mut invalid.consolidated_cylinders[2].payload
    else {
        panic!("range-origin cylinder")
    };
    *range_origin += 1.0;
    let mut invalid_namespace = cadmpeg_ir::NativeNamespace::new(std::num::NonZeroU32::MIN);
    invalid
        .store(&mut invalid_namespace)
        .expect("store invalid CATIA cylinder for load validation");
    assert!(crate::native::CatiaNative::load(&invalid_namespace).is_err());
}

#[test]
fn native_namespace_retains_exact_consolidated_cone_charts() {
    let native = crate::native::CatiaNative::decode(&b2_cone_stream());
    let [cone] = native.consolidated_cones.as_slice() else {
        panic!("one consolidated cone")
    };
    assert_eq!(cone.apex, [1.0, 2.0, 3.0]);
    assert_eq!(cone.direction_x, [1.0, 0.0, 0.0]);
    assert_eq!(cone.direction_y, [0.0, 1.0, 0.0]);
    assert_eq!(cone.axis, [0.0, 0.0, 1.0]);
    assert_eq!(cone.half_angle, 0.25);
    assert_eq!(cone.reference_radius, 4.0);
    assert_eq!(cone.angular_range, [0.5, 0.5 + std::f64::consts::PI]);
    assert_eq!(cone.slant_range, [2.0, 8.0]);
    assert_eq!(cone.angular_scale, 3.0);
    assert_eq!(
        cone.angular_domain,
        [
            0.5 - std::f64::consts::FRAC_PI_2,
            0.5 + 3.0 * std::f64::consts::FRAC_PI_2
        ]
    );

    let mut namespace = cadmpeg_ir::NativeNamespace::new(std::num::NonZeroU32::MIN);
    native.store(&mut namespace).expect("store CATIA cone");
    assert_eq!(
        crate::native::CatiaNative::load(&namespace).expect("load CATIA cone"),
        native
    );

    let mut invalid = native;
    invalid.consolidated_cones[0].angular_domain[0] += 0.25;
    let mut invalid_namespace = cadmpeg_ir::NativeNamespace::new(std::num::NonZeroU32::MIN);
    invalid
        .store(&mut invalid_namespace)
        .expect("store invalid CATIA cone for load validation");
    assert!(crate::native::CatiaNative::load(&invalid_namespace).is_err());
}

#[test]
fn native_namespace_retains_consolidated_cone_face_charts() {
    let native = crate::native::CatiaNative::decode(&b2_cone_face_parameter_point_stream());
    let [face] = native.consolidated_cone_faces.as_slice() else {
        panic!("one consolidated cone-face chart")
    };
    assert_eq!(face.program.len(), 16);
    assert_eq!(face.angular_scale, 1.5);
    assert_eq!(face.half_angle, std::f64::consts::FRAC_PI_4);
    assert_eq!(
        face.parameter_points,
        [
            "catia:consolidated:parameter-point#0",
            "catia:consolidated:parameter-point#1",
            "catia:consolidated:parameter-point#2",
            "catia:consolidated:parameter-point#3",
        ]
    );

    let mut namespace = cadmpeg_ir::NativeNamespace::new(std::num::NonZeroU32::MIN);
    native
        .store(&mut namespace)
        .expect("store CATIA cone-face chart");
    assert_eq!(
        crate::native::CatiaNative::load(&namespace).expect("load CATIA cone-face chart"),
        native
    );

    let mut invalid = native.clone();
    invalid.consolidated_cone_faces[0].program.clear();
    let mut invalid_namespace = cadmpeg_ir::NativeNamespace::new(std::num::NonZeroU32::MIN);
    invalid
        .store(&mut invalid_namespace)
        .expect("store invalid CATIA cone-face chart");
    assert!(crate::native::CatiaNative::load(&invalid_namespace).is_err());

    let mut invalid = native.clone();
    invalid.consolidated_cone_faces[0]
        .parameter_points
        .swap(0, 1);
    let mut invalid_namespace = cadmpeg_ir::NativeNamespace::new(std::num::NonZeroU32::MIN);
    invalid
        .store(&mut invalid_namespace)
        .expect("store invalid CATIA cone-face parameter run");
    assert!(crate::native::CatiaNative::load(&invalid_namespace).is_err());

    let mut mixed = b2_cone_face_parameter_point_stream();
    mixed.extend_from_slice(&[0xb2, 0x03, 0x18, 0x02, 0x05, 0x99, 0x99]);
    let mixed = crate::native::CatiaNative::decode(&mixed);
    assert!(mixed.consolidated_cone_faces[0].parameter_points.is_empty());

    let mut file = standard_catpart();
    file.splice(16..16, b2_cone_face_parameter_point_stream());
    let file_len = u32::try_from(file.len()).expect("bounded CATPart fixture");
    file[8..12].copy_from_slice(&be32(file_len));
    let decoded = CatiaCodec
        .decode(&mut Cursor::new(file), &DecodeOptions::default())
        .expect("decode CATIA cone-face chart");
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_CONSOLIDATED_CONE_FACE_COUNT),
        1
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_CONSOLIDATED_CONE_FACE_PARAMETER_POINT_COUNT),
        4
    );
}

#[test]
fn native_namespace_retains_resolved_consolidated_revolution_carriers() {
    let native = crate::native::CatiaNative::decode(&b2_resolved_revolution_stream());
    let [revolution] = native.consolidated_revolutions.as_slice() else {
        panic!("one consolidated revolution carrier")
    };
    assert_eq!(
        revolution.reference_token,
        crate::native::CatiaRevolutionReferenceToken::Wide
    );
    assert_eq!(revolution.profile_allocation_id, 0x1234);
    assert_eq!(revolution.origin, [1.0, 2.0, 3.0]);
    assert_eq!(revolution.direction_x, [1.0, 0.0, 0.0]);
    assert_eq!(revolution.direction_y, [0.0, 1.0, 0.0]);
    assert_eq!(revolution.axis, [0.0, 0.0, 1.0]);
    assert_eq!(revolution.profile_range, [-4.0, 9.0]);
    assert_eq!(
        revolution.profile_circle.as_deref(),
        Some("catia:consolidated:circle#0")
    );

    let mut namespace = cadmpeg_ir::NativeNamespace::new(std::num::NonZeroU32::MIN);
    native
        .store(&mut namespace)
        .expect("store CATIA revolution");
    assert_eq!(
        crate::native::CatiaNative::load(&namespace).expect("load CATIA revolution"),
        native
    );

    let mut invalid = native.clone();
    invalid.consolidated_revolutions[0].profile_circle = None;
    let mut invalid_namespace = cadmpeg_ir::NativeNamespace::new(std::num::NonZeroU32::MIN);
    invalid
        .store(&mut invalid_namespace)
        .expect("store invalid CATIA revolution profile binding");
    assert!(crate::native::CatiaNative::load(&invalid_namespace).is_err());

    let mut invalid = native;
    invalid.consolidated_revolutions[0].axis = [0.0, 0.0, -1.0];
    let mut invalid_namespace = cadmpeg_ir::NativeNamespace::new(std::num::NonZeroU32::MIN);
    invalid
        .store(&mut invalid_namespace)
        .expect("store invalid CATIA revolution for load validation");
    assert!(crate::native::CatiaNative::load(&invalid_namespace).is_err());

    let mut file = standard_catpart();
    file.splice(16..16, b2_resolved_revolution_stream());
    let file_len = u32::try_from(file.len()).expect("bounded CATPart fixture");
    file[8..12].copy_from_slice(&be32(file_len));
    let decoded = CatiaCodec
        .decode(&mut Cursor::new(file), &DecodeOptions::default())
        .expect("decode resolved CATIA revolution");
    let directrix = decoded
        .ir()
        .model
        .curves
        .iter()
        .find(|curve| {
            curve
                .id
                .0
                .starts_with("catia:consolidated:surface-revolution-directrix#")
        })
        .expect("transferred revolution directrix");
    assert!(matches!(
        directrix.geometry,
        cadmpeg_ir::geometry::CurveGeometry::Circle {
            center,
            axis,
            ref_direction,
            radius: 3.0,
        } if center == cadmpeg_ir::math::Point3::new(1.0, 4.0, -2.0)
            && axis == cadmpeg_ir::math::Vector3::new(1.0, 0.0, 0.0)
            && ref_direction == cadmpeg_ir::math::Vector3::new(0.0, 1.0, 0.0)
    ));
    let revolution = decoded
        .ir()
        .model
        .procedural_surfaces
        .iter()
        .find(|surface| {
            surface
                .id
                .0
                .starts_with("catia:consolidated:surface-revolution#")
        })
        .expect("transferred revolution construction");
    assert!(decoded.ir().model.surfaces.iter().any(|surface| {
        decoded.ir().model.procedural_surface_owner(&revolution.id) == Some(&surface.id)
            && matches!(
                surface.geometry.solved_cache(),
                Some(cadmpeg_ir::geometry::SurfaceGeometry::Torus {
                    center,
                    axis,
                    ref_direction,
                    major_radius: 2.0,
                    minor_radius: 3.0,
                }) if *center == cadmpeg_ir::math::Point3::new(1.0, 2.0, -2.0)
                    && *axis == cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0)
                    && *ref_direction == cadmpeg_ir::math::Vector3::new(0.0, 1.0, 0.0)
            )
    }));
    assert!(cadmpeg_ir::validate::validate_neutral(decoded.ir(), Vec::new()).is_ok());
    assert!(matches!(
        revolution.definition(),
        cadmpeg_ir::geometry::ProceduralSurfaceDefinition::Revolution {
            angular_interval,
            parameter_interval: Some([-4.0, 9.0]),
            ..
        } if *angular_interval == [0.5, 0.5 + std::f64::consts::TAU]
    ));
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::TRANSFERRED_CONSOLIDATED_REVOLUTION_COUNT),
        1
    );
    assert!(!decoded.report().losses.iter().any(|loss| loss
        .message
        .contains("consolidated surface-of-revolution record")));
}

#[test]
fn native_namespace_retains_exact_consolidated_line_profiles() {
    let native = crate::native::CatiaNative::decode(&b2_line_profile_stream());
    let [line] = native.consolidated_line_profiles.as_slice() else {
        panic!("one consolidated line profile")
    };
    assert_eq!(line.origin, [1.0, 2.0, 3.0]);
    assert_eq!(line.direction, [0.0, 0.6, 0.8]);
    assert_eq!(line.range, [-4.0, 9.0]);

    let mut namespace = cadmpeg_ir::NativeNamespace::new(std::num::NonZeroU32::MIN);
    native
        .store(&mut namespace)
        .expect("store CATIA line profile");
    assert_eq!(
        crate::native::CatiaNative::load(&namespace).expect("load CATIA line profile"),
        native
    );

    let mut invalid = native;
    invalid.consolidated_line_profiles[0].direction = [0.0, 0.0, 2.0];
    let mut invalid_namespace = cadmpeg_ir::NativeNamespace::new(std::num::NonZeroU32::MIN);
    invalid
        .store(&mut invalid_namespace)
        .expect("store invalid CATIA line profile for load validation");
    assert!(crate::native::CatiaNative::load(&invalid_namespace).is_err());
}

#[test]
fn native_namespace_retains_exact_consolidated_torus_charts() {
    let native = crate::native::CatiaNative::decode(&b2_torus_stream());
    let [torus] = native.consolidated_tori.as_slice() else {
        panic!("one consolidated torus")
    };
    assert_eq!(torus.center, [1.0, 2.0, 3.0]);
    assert_eq!(torus.direction_x, [1.0, 0.0, 0.0]);
    assert_eq!(torus.direction_y, [0.0, 1.0, 0.0]);
    assert_eq!(torus.axis, [0.0, 0.0, 1.0]);
    assert_eq!(torus.major_radius, 7.0);
    assert_eq!(torus.minor_radius, 2.0);
    assert_eq!(
        torus.major_angular_range,
        [
            std::f64::consts::FRAC_PI_2,
            3.0 * std::f64::consts::FRAC_PI_2
        ]
    );
    assert_eq!(torus.major_angular_domain, [0.0, std::f64::consts::TAU]);
    assert_eq!(torus.minor_angular_range, [0.0, std::f64::consts::PI]);
    assert_eq!(
        torus.minor_angular_domain,
        [
            -std::f64::consts::FRAC_PI_2,
            3.0 * std::f64::consts::FRAC_PI_2
        ]
    );
    assert_eq!(torus.major_scale, 14.0);
    assert_eq!(torus.minor_scale, 4.0);

    let mut namespace = cadmpeg_ir::NativeNamespace::new(std::num::NonZeroU32::MIN);
    native.store(&mut namespace).expect("store CATIA torus");
    assert_eq!(
        crate::native::CatiaNative::load(&namespace).expect("load CATIA torus"),
        native
    );

    let mut invalid = native;
    invalid.consolidated_tori[0].major_angular_domain[0] += 0.25;
    let mut invalid_namespace = cadmpeg_ir::NativeNamespace::new(std::num::NonZeroU32::MIN);
    invalid
        .store(&mut invalid_namespace)
        .expect("store invalid CATIA torus for load validation");
    assert!(crate::native::CatiaNative::load(&invalid_namespace).is_err());
}

#[test]
fn native_namespace_retains_exact_consolidated_sphere_charts() {
    let native = crate::native::CatiaNative::decode(&b2_sphere_stream());
    let [sphere] = native.consolidated_spheres.as_slice() else {
        panic!("one consolidated sphere")
    };
    assert_eq!(sphere.center, [1.0, 2.0, 3.0]);
    assert_eq!(sphere.direction_x, [1.0, 0.0, 0.0]);
    assert_eq!(sphere.direction_y, [0.0, 1.0, 0.0]);
    assert_eq!(sphere.axis, [0.0, 0.0, 1.0]);
    assert_eq!(sphere.radius, 5.0);
    assert_eq!(sphere.azimuth_range, [-2.0, 4.0]);
    assert_eq!(sphere.latitude_range, [-1.0, std::f64::consts::FRAC_PI_2]);

    let mut namespace = cadmpeg_ir::NativeNamespace::new(std::num::NonZeroU32::MIN);
    native.store(&mut namespace).expect("store CATIA sphere");
    assert_eq!(
        crate::native::CatiaNative::load(&namespace).expect("load CATIA sphere"),
        native
    );

    let mut invalid = native;
    invalid.consolidated_spheres[0].latitude_range.reverse();
    let mut invalid_namespace = cadmpeg_ir::NativeNamespace::new(std::num::NonZeroU32::MIN);
    invalid
        .store(&mut invalid_namespace)
        .expect("store invalid CATIA sphere for load validation");
    assert!(crate::native::CatiaNative::load(&invalid_namespace).is_err());
}

#[test]
fn native_namespace_retains_consolidated_owner_packet_and_face_node_relation() {
    let native = crate::native::CatiaNative::decode(&b2_adjacent_face_owner_stream());
    let [packet] = native.consolidated_owner_packets.as_slice() else {
        panic!("one consolidated owner packet")
    };
    assert_eq!(packet.source_index, 0);
    assert!(packet.identity_targets().is_empty());
    let crate::native::CatiaOwnerPacketPayload::FixedNine {
        references,
        identity_encodings,
        numeric_tail,
        ..
    } = &packet.payload
    else {
        panic!("fixed-nine owner payload")
    };
    assert_eq!(*references, [1000, 1, 1001, 2, 1002, 3, 1003, 4, 1004]);
    assert_eq!(
        *identity_encodings,
        std::array::from_fn(
            |index| crate::native::CatiaOwnerIdentityEncoding::Allocation(if index % 2 == 0 {
                crate::native::CatiaAllocationReferenceEncoding::TaggedU16
            } else {
                crate::native::CatiaAllocationReferenceEncoding::BackwardDistance
            },)
        )
    );
    assert_eq!(numeric_tail.header, [0x84, 0x41, 0xbb, 0x05, 0x0d]);
    assert_eq!(numeric_tail.lower, [-0.0, 4.5]);
    assert_eq!(numeric_tail.upper, [12.25, 7.0]);
    assert_eq!(numeric_tail.bounds, [[-2.0, 1.0], [3.5, 4.0], [5.25, 6.0]]);
    let face_node = packet.face_node.expect("face-node relation");
    assert_eq!(face_node.byte_len, 11);
    assert_eq!(
        face_node.target_encoding,
        crate::native::CatiaFaceNodeTargetEncoding::Compact
    );
    assert_eq!(face_node.target, 1003);
    assert_eq!(face_node.terminal, [0x03, 0x05]);
    assert_eq!(face_node.target + 1, references[8]);

    let mut namespace = cadmpeg_ir::NativeNamespace::new(std::num::NonZeroU32::MIN);
    native
        .store(&mut namespace)
        .expect("store CATIA owner packet");
    assert_eq!(
        crate::native::CatiaNative::load(&namespace).expect("load CATIA owner packet"),
        native
    );

    let mut invalid = native;
    invalid.consolidated_owner_packets[0]
        .face_node
        .as_mut()
        .expect("face-node relation")
        .target -= 1;
    let mut namespace = cadmpeg_ir::NativeNamespace::new(std::num::NonZeroU32::MIN);
    invalid
        .store(&mut namespace)
        .expect("store invalid CATIA owner packet");
    assert!(crate::native::CatiaNative::load(&namespace).is_err());
}

#[test]
fn native_namespace_retains_fixed_owner_allocation_targets() {
    let (bytes, target_positions, owner_pos) = b2_width_coded_owner_with_allocation_stream();
    let native = crate::native::CatiaNative::decode(&bytes);
    let [packet] = native.consolidated_owner_packets.as_slice() else {
        panic!("one consolidated owner packet")
    };

    assert_eq!(packet.byte_offset, owner_pos as u64);
    assert_eq!(packet.source_index, 0);
    assert_eq!(
        packet
            .identity_targets()
            .iter()
            .map(|target| (
                target.slot,
                target.distance,
                target.target_byte_offset,
                u8::from(target.target_class),
            ))
            .collect::<Vec<_>>(),
        [
            (0, 1, target_positions[4] as u64, 0x5e),
            (2, 4, target_positions[1] as u64, 0x5e),
            (4, 2, target_positions[3] as u64, 0x5e),
            (6, 3, target_positions[2] as u64, 0x5d),
            (8, 5, target_positions[0] as u64, 0x5d),
        ]
    );
}

#[test]
fn native_namespace_retains_closed_fixed_owner_boundary_cycle() {
    let (bytes, edge_positions, owner_pos, endpoint_records) =
        b2_fixed_owner_boundary_cycle_stream();
    let native = crate::native::CatiaNative::decode(&bytes);
    let [packet] = native.consolidated_owner_packets.as_slice() else {
        panic!("one consolidated owner packet")
    };

    assert_eq!(packet.byte_offset, owner_pos as u64);
    let cycle = packet
        .boundary_cycle()
        .expect("closed fixed-owner boundary cycle");
    assert!(cycle.face_node.is_none());
    assert_eq!(
        cycle.edges.map(|edge| edge.byte_offset),
        edge_positions.map(|position| position as u64)
    );
    assert_eq!(
        cycle.edges.map(|edge| edge.endpoint_records),
        endpoint_records.map(|pair| pair.map(|position| position as u64))
    );
}

#[test]
fn native_namespace_retains_boundary_face_node_for_checked_cycle_prelude() {
    let (bytes, edge_positions, owner_pos, endpoint_records, face_node_pos) =
        b2_fixed_owner_boundary_face_node_cycle_stream();
    let native = crate::native::CatiaNative::decode(&bytes);
    let [packet] = native.consolidated_owner_packets.as_slice() else {
        panic!("one consolidated owner packet")
    };
    let cycle = packet
        .boundary_cycle()
        .expect("closed fixed-owner boundary cycle");
    let face_node = cycle
        .face_node
        .as_ref()
        .expect("source-scoped boundary face node");
    assert_eq!(face_node.byte_offset, face_node_pos as u64);
    assert_eq!(face_node.byte_len, (owner_pos - face_node_pos) as u64);
    assert_eq!(face_node.target, 1014);
    assert_eq!(face_node.terminal, [0x27, 0x05]);
    assert_eq!(
        cycle.edges.map(|edge| edge.byte_offset),
        edge_positions.map(|position| position as u64)
    );
    assert_eq!(
        cycle.edges.map(|edge| edge.endpoint_records),
        endpoint_records.map(|pair| pair.map(|position| position as u64))
    );

    let mut wrong_terminal = bytes.clone();
    wrong_terminal[face_node_pos + 12] = 0x04;
    let native = crate::native::CatiaNative::decode(&wrong_terminal);
    let [packet] = native.consolidated_owner_packets.as_slice() else {
        panic!("one consolidated owner packet")
    };
    assert!(packet
        .boundary_cycle()
        .expect("cycle survives terminal change")
        .face_node
        .is_none());

    let mut wrong_identity = bytes;
    wrong_identity[face_node_pos + 9] = 0xe1;
    let native = crate::native::CatiaNative::decode(&wrong_identity);
    let [packet] = native.consolidated_owner_packets.as_slice() else {
        panic!("one consolidated owner packet")
    };
    assert!(packet
        .boundary_cycle()
        .expect("cycle survives identity change")
        .face_node
        .is_none());
}

#[test]
fn native_namespace_retains_source_closed_owner_chart() {
    let native = crate::native::CatiaNative::decode(&b2_owner_chart_stream(0x2b));
    let [packet] = native.consolidated_owner_packets.as_slice() else {
        panic!("one consolidated owner packet")
    };
    let chart = packet.owner_chart().expect("owner chart relation");
    assert_eq!(chart.carrier, crate::native::CatiaOwnerChartCarrier::B2b);
    assert_eq!(
        chart.side_axis,
        crate::native::CatiaOwnerChartSideAxis::SecondParameter
    );
    let crate::native::CatiaOwnerChartBridge::SupportedSurface {
        byte_offset,
        carrier_surface,
        support_surfaces,
        support_pcurves,
        controls,
        construction_radius,
    } = &chart.bridge
    else {
        panic!("supported-surface owner bridge")
    };
    assert!(chart.carrier_byte_offset < *byte_offset);
    assert!(*byte_offset < chart.parameter_point_byte_offsets[0]);
    assert_eq!(
        [
            carrier_surface.value,
            support_surfaces[0].value,
            support_surfaces[1].value,
            support_pcurves[0].value,
            support_pcurves[1].value,
        ],
        [1, 100, 0, 101, 1]
    );
    assert_eq!(
        carrier_surface.encoding,
        crate::native::CatiaAllocationReferenceEncoding::BackwardDistance
    );
    assert_eq!(*controls, [0x09, 0x05, 0x03, 0x05, 0x01, 0x05]);
    assert_eq!(*construction_radius, 1.0);
    assert!(chart.parameter_point_byte_offsets[3] < packet.byte_offset);

    let mut namespace = cadmpeg_ir::NativeNamespace::new(std::num::NonZeroU32::MIN);
    native
        .store(&mut namespace)
        .expect("store CATIA owner chart");
    assert_eq!(
        crate::native::CatiaNative::load(&namespace).expect("load CATIA owner chart"),
        native
    );
}

#[test]
fn owner_chart_width_coded_supports_select_unique_alias_rows() {
    let mut bytes = b2_owner_chart_stream(0x2b);
    bytes.extend(grouped_surface_alias_stream(0, 100, 0x148));
    bytes.extend(grouped_surface_alias_stream(1, 200, 0x148));
    let mut pcurve_alias = surface_alias_stream();
    pcurve_alias[8..12].copy_from_slice(&101u32.to_le_bytes());
    bytes.extend(pcurve_alias);
    let mut local_collision = surface_alias_stream();
    local_collision[8..12].copy_from_slice(&1u32.to_le_bytes());
    bytes.extend(local_collision);

    let native = crate::native::CatiaNative::decode(&bytes);
    let chart = native.consolidated_owner_packets[0]
        .owner_chart()
        .expect("owner chart");
    let crate::native::CatiaOwnerChartBridge::SupportedSurface {
        carrier_surface,
        support_surfaces,
        support_pcurves,
        ..
    } = &chart.bridge
    else {
        panic!("supported-surface bridge")
    };
    let surface_alias = native
        .alias_rows
        .iter()
        .find(|alias| alias.tag == 100)
        .expect("support-surface alias");
    let pcurve_alias = native
        .alias_rows
        .iter()
        .find(|alias| alias.tag == 101)
        .expect("support-pcurve alias");
    assert_eq!(
        support_surfaces[0]
            .alias
            .as_ref()
            .map(|binding| binding.row.as_str()),
        Some(surface_alias.id.as_str())
    );
    assert_eq!(
        support_surfaces[0]
            .alias
            .as_ref()
            .and_then(|binding| binding.canonical_tag),
        Some(200)
    );
    assert_eq!(
        support_pcurves[0]
            .alias
            .as_ref()
            .map(|binding| binding.row.as_str()),
        Some(pcurve_alias.id.as_str())
    );
    assert_eq!(
        support_pcurves[0]
            .alias
            .as_ref()
            .and_then(|binding| binding.canonical_tag),
        Some(101)
    );
    assert_ne!(
        support_pcurves[1].encoding,
        crate::native::CatiaAllocationReferenceEncoding::WidthCoded
    );
    assert_eq!(support_pcurves[1].alias, None);
    assert_eq!(carrier_surface.alias, None);

    let mut legacy = native.clone();
    let Some(chart) = legacy.consolidated_owner_packets[0].owner_chart_mut() else {
        panic!("owner chart")
    };
    let crate::native::CatiaOwnerChartBridge::SupportedSurface {
        support_surfaces,
        support_pcurves,
        ..
    } = &mut chart.bridge
    else {
        panic!("supported-surface bridge")
    };
    for reference in support_surfaces.iter_mut().chain(support_pcurves) {
        reference.alias = None;
    }
    let mut namespace = cadmpeg_ir::NativeNamespace::new(std::num::NonZeroU32::MIN);
    legacy
        .store(&mut namespace)
        .expect("store legacy chart links");
    namespace.set_version(
        std::num::NonZeroU32::new(crate::native::CATIA_OWNER_CHART_ALIAS_VERSION - 1).unwrap(),
    );
    crate::native::CatiaNative::load(&namespace).expect("load legacy chart links");

    let mut invalid = native;
    let Some(chart) = invalid.consolidated_owner_packets[0].owner_chart_mut() else {
        panic!("owner chart")
    };
    let crate::native::CatiaOwnerChartBridge::SupportedSurface {
        support_surfaces, ..
    } = &mut chart.bridge
    else {
        panic!("supported-surface bridge")
    };
    if let Some(alias) = &mut support_surfaces[0].alias {
        alias.canonical_tag = Some(100);
    }
    let mut namespace = cadmpeg_ir::NativeNamespace::new(std::num::NonZeroU32::MIN);
    invalid
        .store(&mut namespace)
        .expect("store invalid support alias");
    assert!(matches!(
        crate::native::CatiaNative::load(&namespace),
        Err(cadmpeg_ir::NativeConvertError::InvalidOwner(_))
    ));
}

#[test]
fn owner_chart_duplicate_alias_tags_remain_unresolved() {
    let mut bytes = b2_owner_chart_stream(0x2b);
    for lead in [1u32, 0x8e] {
        let mut alias = surface_alias_stream();
        alias[..4].copy_from_slice(&lead.to_le_bytes());
        alias[8..12].copy_from_slice(&100u32.to_le_bytes());
        bytes.extend(alias);
    }

    let native = crate::native::CatiaNative::decode(&bytes);
    let chart = native.consolidated_owner_packets[0]
        .owner_chart()
        .expect("owner chart");
    let crate::native::CatiaOwnerChartBridge::SupportedSurface {
        support_surfaces, ..
    } = &chart.bridge
    else {
        panic!("supported-surface bridge")
    };
    assert_eq!(support_surfaces[0].alias, None);
}

#[test]
fn native_namespace_retains_count_framed_owner_packet_and_face_node_relation() {
    let native = crate::native::CatiaNative::decode(&b2_adjacent_face_counted_owner_stream());
    let [packet] = native.consolidated_owner_packets.as_slice() else {
        panic!("one consolidated owner packet")
    };
    let crate::native::CatiaOwnerPacketPayload::Counted { references, tail } = &packet.payload
    else {
        panic!("count-framed owner payload")
    };
    assert_eq!(references, &[911, 7, 263, 258, 281, 276, 917]);
    assert_eq!(tail, &[0x83, 0x41, 0x92, 0x00, 0x01]);
    let face_node = packet.face_node.expect("face-node relation");
    assert_eq!(face_node.target, 916);
    assert_eq!(
        face_node.target + 1,
        *references.last().expect("final owner reference")
    );

    let mut namespace = cadmpeg_ir::NativeNamespace::new(std::num::NonZeroU32::MIN);
    native
        .store(&mut namespace)
        .expect("store count-framed CATIA owner packet");
    assert_eq!(
        crate::native::CatiaNative::load(&namespace).expect("load count-framed CATIA owner packet"),
        native
    );

    let mut invalid = native;
    let crate::native::CatiaOwnerPacketPayload::Counted { tail, .. } =
        &mut invalid.consolidated_owner_packets[0].payload
    else {
        panic!("count-framed owner payload")
    };
    tail.clear();
    let mut namespace = cadmpeg_ir::NativeNamespace::new(std::num::NonZeroU32::MIN);
    invalid
        .store(&mut namespace)
        .expect("store invalid count-framed CATIA owner packet");
    assert!(crate::native::CatiaNative::load(&namespace).is_err());
}

#[test]
fn native_namespace_retains_consolidated_historical_edge_runs() {
    let bytes = a5_native_edge_run_stream(6, 139, 142);
    let native = crate::native::CatiaNative::decode(&bytes);
    assert_eq!(native.consolidated_pcurves.len(), 2);
    assert_eq!(native.consolidated_edge_runs.len(), 1);
    let run = &native.consolidated_edge_runs[0];
    assert_eq!(
        run.pcurves,
        ["catia:consolidated:pcurve#0", "catia:consolidated:pcurve#1"]
    );
    assert_eq!(run.node, "catia:consolidated:edge-node#0");
    let [node] = native.consolidated_edge_nodes.as_slice() else {
        panic!("one consolidated edge node");
    };
    assert_eq!(node.vertex_refs, [139, 142]);
    assert_eq!(
        node.vertex_identity_ids(),
        [
            "catia:consolidated:vertex-identity#0",
            "catia:consolidated:vertex-identity#1"
        ]
    );
    assert_eq!(node.parameter_selectors, [2, 1]);
    let uses = node.uses.as_ref().expect("edge-owned oriented uses");
    assert_eq!(uses.references, [[4, 5], [5, 6]]);
    let definition = node.definition.as_ref().expect("edge-owned definition");
    assert_eq!(definition.class, 0x23);
    assert!(definition.byte_offset < node.byte_offset);
    assert_eq!(native.consolidated_vertex_identities.len(), 2);
    assert_eq!(native.consolidated_vertex_identities[0].identity, 139);
    assert_eq!(
        native.consolidated_vertex_identities[0].incident_edge_nodes,
        ["catia:consolidated:edge-node#0"]
    );

    let mut file = standard_catpart();
    file.splice(16..16, bytes.clone());
    let file_len = u32::try_from(file.len()).expect("bounded CATPart fixture");
    file[8..12].copy_from_slice(&be32(file_len));
    let decoded = CatiaCodec
        .decode(&mut Cursor::new(file), &DecodeOptions::default())
        .expect("decode consolidated edge-run coverage");
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_CONSOLIDATED_EDGE_RUN_COUNT),
        1
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_CONSOLIDATED_EDGE_RUN_SUPPORT_BINDING_COUNT),
        0
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::UNRESOLVED_CONSOLIDATED_EDGE_RUN_COUNT),
        1
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::PARTIALLY_RESOLVED_CONSOLIDATED_EDGE_RUN_COUNT),
        0
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::FULLY_RESOLVED_CONSOLIDATED_EDGE_RUN_COUNT),
        0
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_CONSOLIDATED_EDGE_RUN_SHARED_LOCUS_COUNT),
        0
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_CONSOLIDATED_EDGE_RUN_ENDPOINT_LOCUS_COUNT),
        0
    );

    let mut namespace = cadmpeg_ir::NativeNamespace::new(std::num::NonZeroU32::MIN);
    native.store(&mut namespace).expect("store CATIA edge run");
    assert_eq!(
        crate::native::CatiaNative::load(&namespace).expect("load CATIA edge run"),
        native
    );

    let mut invalid = native;
    invalid.consolidated_edge_runs[0].pcurves[1] = "missing".to_string();
    let mut invalid_namespace = cadmpeg_ir::NativeNamespace::new(std::num::NonZeroU32::MIN);
    invalid
        .store(&mut invalid_namespace)
        .expect("store invalid CATIA edge run for load validation");
    assert!(crate::native::CatiaNative::load(&invalid_namespace).is_err());

    let mut invalid = crate::native::CatiaNative::decode(&bytes);
    invalid.consolidated_edge_nodes[0]
        .definition
        .as_mut()
        .expect("edge definition")
        .class = 0x26;
    let mut invalid_namespace = cadmpeg_ir::NativeNamespace::new(std::num::NonZeroU32::MIN);
    invalid
        .store(&mut invalid_namespace)
        .expect("store invalid CATIA edge definition");
    assert!(crate::native::CatiaNative::load(&invalid_namespace).is_err());

    let mut invalid = crate::native::CatiaNative::decode(&bytes);
    invalid.consolidated_edge_nodes[0].uses = None;
    let mut invalid_namespace = cadmpeg_ir::NativeNamespace::new(std::num::NonZeroU32::MIN);
    invalid
        .store(&mut invalid_namespace)
        .expect("store orphaned CATIA edge definition");
    assert!(crate::native::CatiaNative::load(&invalid_namespace).is_err());

    let mut invalid = crate::native::CatiaNative::decode(&bytes);
    invalid.consolidated_vertex_identities[0]
        .incident_edge_nodes
        .clear();
    let mut invalid_namespace = cadmpeg_ir::NativeNamespace::new(std::num::NonZeroU32::MIN);
    invalid
        .store(&mut invalid_namespace)
        .expect("store invalid CATIA vertex incidence for load validation");
    assert!(crate::native::CatiaNative::load(&invalid_namespace).is_err());
}

#[test]
fn compact_owner_does_not_type_unresolved_edge_references_as_vertices() {
    let allocation = [
        0xb2, 0x03, 0x5f, 0x04, 0x05, 0x82, 0x1d, 0x03, 0x05, 0xb2, 0x03, 0x62, 0x08, 0x05, 0x82,
        0x0b, 0x21, 0x84, 0x41, 0xff, 0x0f, 0x01, 0xb2, 0x03, 0x5d, 0x02, 0x05, 0x03, 0x00, 0xb2,
        0x03, 0x05, 0x03, 0x05, 0x82, 0x0b, 0x57, 0xb2, 0x03, 0x5e, 0x06, 0x05, 0x03, 0x09, 0x0f,
        0x07, 0x0b, 0x21,
    ];
    let bytes = allocation.repeat(2);
    let native = crate::native::CatiaNative::decode(&bytes);
    assert_eq!(native.consolidated_edge_nodes.len(), 2);
    assert!(native.consolidated_vertex_identities.is_empty());
    assert_ne!(
        native.consolidated_edge_nodes[0]
            .allocation
            .as_ref()
            .map(|(owner, _)| owner),
        native.consolidated_edge_nodes[1]
            .allocation
            .as_ref()
            .map(|(owner, _)| owner)
    );
    assert_eq!(
        native.consolidated_edge_nodes[0]
            .allocation
            .as_ref()
            .map(|(_, ordinal)| *ordinal),
        Some(2)
    );
    assert_eq!(
        native.consolidated_edge_nodes[1]
            .allocation
            .as_ref()
            .map(|(_, ordinal)| *ordinal),
        Some(2)
    );
    assert_eq!(
        native.consolidated_edge_nodes[0].vertex_identity_ids(),
        ["", ""]
    );
    assert_eq!(
        native.consolidated_edge_nodes[1].vertex_identity_ids(),
        ["", ""]
    );
}

#[test]
fn compact_vertex_identity_uses_resolved_endpoint_records() {
    let first_edge = [
        0xb2, 0x03, 0x5e, 0x09, 0x05, 0x06, 0x20, 0x03, 0x07, 0x06, 0x30, 0x06, 0x31, 0x21,
    ];
    let vertex = [0xb2, 0x03, 0x5d, 0x02, 0x05, 0x03, 0x00];
    let second_edge = [
        0xb2, 0x03, 0x5e, 0x09, 0x05, 0x06, 0x21, 0x09, 0x0d, 0x06, 0x32, 0x06, 0x33, 0x21,
    ];
    let first_vertex_pos = first_edge.len() as u64;
    let second_vertex_pos = first_vertex_pos + vertex.len() as u64;
    let mut bytes = first_edge.to_vec();
    bytes.extend_from_slice(&vertex);
    bytes.extend_from_slice(&vertex);
    bytes.extend_from_slice(&second_edge);

    let native = crate::native::CatiaNative::decode(&bytes);
    assert_eq!(native.consolidated_edge_nodes.len(), 2);
    assert_eq!(native.consolidated_vertex_identities.len(), 2);
    assert_eq!(
        native.consolidated_edge_nodes[0].endpoint_records,
        Some([first_vertex_pos, second_vertex_pos])
    );
    assert_eq!(
        native.consolidated_edge_nodes[1].endpoint_records,
        Some([first_vertex_pos, second_vertex_pos])
    );
    assert_eq!(
        native.consolidated_edge_nodes[0].vertex_identity_ids(),
        native.consolidated_edge_nodes[1].vertex_identity_ids()
    );
    assert_eq!(
        native
            .consolidated_vertex_identities
            .iter()
            .map(|identity| identity.endpoint_record)
            .collect::<Vec<_>>(),
        vec![Some(first_vertex_pos), Some(second_vertex_pos)]
    );
}

#[test]
fn width_coded_forward_endpoints_merge_by_class18_record_identity() {
    fn edge(start_distance: u8, end_distance: u8) -> Vec<u8> {
        vec![
            0xb2,
            0x03,
            0x5e,
            0x0a,
            0x05,
            0x03,
            0x08,
            start_distance,
            0x00,
            0x08,
            end_distance,
            0x00,
            0x07,
            0x0b,
            0x21,
        ]
    }

    let filler = [0xb2, 0x03, 0x05, 0x01, 0x05, 0x01];
    let endpoint = [0xb2, 0x03, 0x18, 0x01, 0x05, 0x01];
    let mut bytes = edge(4, 5);
    bytes.extend_from_slice(&edge(3, 4));
    bytes.extend_from_slice(&filler);
    bytes.extend_from_slice(&filler);
    let first_endpoint = bytes.len() as u64;
    bytes.extend_from_slice(&endpoint);
    let second_endpoint = bytes.len() as u64;
    bytes.extend_from_slice(&endpoint);

    let native = crate::native::CatiaNative::decode(&bytes);
    assert_eq!(native.consolidated_edge_nodes.len(), 2);
    assert_eq!(native.consolidated_vertex_identities.len(), 2);
    assert_eq!(
        native.consolidated_edge_nodes[0].endpoint_records,
        Some([first_endpoint, second_endpoint])
    );
    assert_eq!(
        native.consolidated_edge_nodes[0].vertex_identity_ids(),
        native.consolidated_edge_nodes[1].vertex_identity_ids()
    );
    assert_eq!(
        native
            .consolidated_vertex_identities
            .iter()
            .map(|identity| identity.endpoint_record)
            .collect::<Vec<_>>(),
        [Some(first_endpoint), Some(second_endpoint)]
    );
}

#[test]
fn native_namespace_merges_shared_consolidated_vertex_identity() {
    let mut bytes = a5_native_edge_run_stream(6, 139, 142);
    bytes.extend_from_slice(&a5_native_edge_run_stream(9, 142, 151));
    let native = crate::native::CatiaNative::decode(&bytes);

    assert_eq!(native.consolidated_edge_runs.len(), 2);
    assert_eq!(native.consolidated_vertex_identities.len(), 3);
    let shared = native
        .consolidated_vertex_identities
        .iter()
        .find(|vertex| vertex.identity == 142)
        .expect("shared consolidated vertex identity");
    assert_eq!(
        shared.incident_edge_nodes,
        [
            "catia:consolidated:edge-node#0",
            "catia:consolidated:edge-node#1"
        ]
    );
    assert_eq!(
        native.consolidated_edge_nodes[0].vertex_identity_ids()[1],
        native.consolidated_edge_nodes[1].vertex_identity_ids()[0]
    );
}

#[test]
fn native_vertex_identity_namespace_is_bounded_by_record_source() {
    let first = a5_native_edge_run_stream(6, 14, 15);
    let mut bytes = first.clone();
    bytes.extend_from_slice(&a5_native_edge_run_stream(9, 15, 16));
    let native = crate::native::CatiaNative::decode_with_record_ranges(
        &bytes,
        &[0..first.len(), first.len()..bytes.len()],
    );

    assert_eq!(native.consolidated_edge_runs.len(), 2);
    assert_eq!(native.consolidated_vertex_identities.len(), 4);
    assert_eq!(
        native
            .consolidated_edge_nodes
            .iter()
            .map(|node| node.source_index)
            .collect::<Vec<_>>(),
        [0, 1]
    );
    let repeated = native
        .consolidated_vertex_identities
        .iter()
        .filter(|vertex| vertex.identity == 15)
        .collect::<Vec<_>>();
    assert_eq!(repeated.len(), 2);
    assert_eq!(repeated[0].source_index, 0);
    assert_eq!(repeated[1].source_index, 1);
    assert_ne!(
        native.consolidated_edge_nodes[0].vertex_identity_ids()[1],
        native.consolidated_edge_nodes[1].vertex_identity_ids()[0]
    );
}

#[test]
fn explicit_vertex_encodings_share_one_complete_run_identity_namespace() {
    fn replace_node(mut stream: Vec<u8>, payload: &[u8]) -> Vec<u8> {
        let node = stream
            .windows(3)
            .position(|window| window == [0xb2, 0x03, 0x5e])
            .expect("edge node frame");
        stream.truncate(node);
        stream.extend_from_slice(&[
            0xb2,
            0x03,
            0x5e,
            u8::try_from(payload.len()).expect("bounded edge payload"),
            0x05,
        ]);
        stream.extend_from_slice(payload);
        stream
    }

    let mut bytes = a5_native_edge_run_stream(6, 14, 15);
    let tagged_u16 = replace_node(
        a5_native_edge_run_stream(9, 15, 16),
        &[37, 0x0a, 15, 0, 0x0a, 16, 0, 9, 5, 0x21],
    );
    bytes.extend_from_slice(&tagged_u16);
    let selector2 = replace_node(
        a5_native_edge_run_stream(12, 16, 17),
        &[49, 4 * 16 + 2, 4 * 17 + 2, 9, 5, 0x21],
    );
    bytes.extend_from_slice(&selector2);

    let native = crate::native::CatiaNative::decode(&bytes);

    assert_eq!(native.consolidated_edge_runs.len(), 3);
    assert_eq!(native.consolidated_vertex_identities.len(), 4);
    assert_eq!(
        native.consolidated_edge_nodes[0].reference_encodings[1..3],
        [
            crate::native::CatiaAllocationReferenceEncoding::TaggedU8,
            crate::native::CatiaAllocationReferenceEncoding::TaggedU8,
        ]
    );
    assert_eq!(
        native.consolidated_edge_nodes[1].reference_encodings[1..3],
        [
            crate::native::CatiaAllocationReferenceEncoding::TaggedU16,
            crate::native::CatiaAllocationReferenceEncoding::TaggedU16,
        ]
    );
    assert_eq!(
        native.consolidated_edge_nodes[0].vertex_identity_ids()[1],
        native.consolidated_edge_nodes[1].vertex_identity_ids()[0]
    );
    assert_eq!(
        native.consolidated_edge_nodes[2].reference_encodings[1..3],
        [
            crate::native::CatiaAllocationReferenceEncoding::Selector2,
            crate::native::CatiaAllocationReferenceEncoding::Selector2,
        ]
    );
    assert_eq!(
        native.consolidated_edge_nodes[1].vertex_identity_ids()[1],
        native.consolidated_edge_nodes[2].vertex_identity_ids()[0]
    );
}

#[test]
fn native_namespace_retains_standalone_consolidated_edge_nodes() {
    let bytes = b2_edge_node_stream();
    let native = crate::native::CatiaNative::decode(&bytes);

    assert!(native.consolidated_edge_runs.is_empty());
    let [node] = native.consolidated_edge_nodes.as_slice() else {
        panic!("one standalone consolidated edge node");
    };
    assert_eq!(node.width, 1);
    assert_eq!(node.flag, 0x03);
    assert_eq!(node.header_token, 5);
    assert_eq!(node.terminal_value, 8);
    assert_eq!(
        node.terminal_encoding,
        crate::native::CatiaAllocationReferenceEncoding::BackwardDistance,
    );
    assert_eq!(node.vertex_refs, [889, 895]);
    assert!(node.uses.is_none());
    assert_eq!(node.vertex_identity_ids(), ["", ""]);
    assert!(native.consolidated_vertex_identities.is_empty());

    let mut namespace = cadmpeg_ir::NativeNamespace::new(std::num::NonZeroU32::MIN);
    native
        .store(&mut namespace)
        .expect("store standalone consolidated edge node");
    assert_eq!(
        crate::native::CatiaNative::load(&namespace)
            .expect("load standalone consolidated edge node"),
        native
    );
}

#[test]
fn native_namespace_attaches_oriented_uses_without_pcurves() {
    let bytes = a5_native_edge_identity_stream(6, 139, 142);
    let native = crate::native::CatiaNative::decode(&bytes);

    assert!(native.consolidated_edge_runs.is_empty());
    let [node] = native.consolidated_edge_nodes.as_slice() else {
        panic!("one consolidated edge node");
    };
    let uses = node.uses.as_ref().expect("standalone edge-owned uses");
    assert_eq!(uses.references, [[4, 5], [5, 6]]);
}

#[test]
fn native_namespace_retains_resolved_consolidated_edge_supports_and_loci() {
    use crate::native::CatiaConsolidatedSupportBinding;

    let mut bytes = b2_cylinder_stream();
    for point in [
        [1.0f32, 4.0, 3.0],
        [2.0, 2.0 + 2.0 * 0.5f32.cos(), 3.0 + 2.0 * 0.5f32.sin()],
    ] {
        bytes.extend_from_slice(&[0x05, 0x08, 0x01]);
        for value in point {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
    }
    bytes.extend_from_slice(&a5_native_edge_run_stream(6, 139, 142));

    let native = crate::native::CatiaNative::decode(&bytes);
    let [run] = native.consolidated_edge_runs.as_slice() else {
        panic!("one consolidated edge run");
    };
    assert!(run.support_bindings.iter().all(|binding| matches!(
        binding,
        Some(CatiaConsolidatedSupportBinding::Cylinder { .. })
    )));
    assert_eq!(run.shared_loci.as_ref().map(Vec::len), Some(2));
    assert_eq!(
        run.endpoint_loci,
        run.shared_loci
            .as_ref()
            .map(|loci| [loci[0], loci[loci.len() - 1]])
    );

    let mut namespace = cadmpeg_ir::NativeNamespace::new(std::num::NonZeroU32::MIN);
    native
        .store(&mut namespace)
        .expect("store resolved CATIA edge run");
    assert_eq!(
        crate::native::CatiaNative::load(&namespace).expect("load resolved CATIA edge run"),
        native
    );

    namespace
        .set_arena(
            "consolidated_cylinders",
            &Vec::<crate::native::CatiaConsolidatedCylinder>::new(),
        )
        .expect("remove retained cylinders");
    assert!(crate::native::CatiaNative::load(&namespace).is_err());
}

#[test]
fn native_namespace_retains_resolved_consolidated_plane_supports() {
    use crate::native::CatiaConsolidatedSupportBinding;

    let plane_stream = b2_plane_carrier_stream();
    let plane_carriers = crate::families::b2::records::b2_plane_carriers(&plane_stream);
    let plane_end = plane_carriers[0].end;
    let mut bytes = plane_stream[..plane_end].to_vec();
    for point in [[10.0f32, 20.0, 0.0], [11.0, 20.0, 1.0]] {
        bytes.extend_from_slice(&[0x05, 0x08, 0x01]);
        for value in point {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
    }
    bytes.extend_from_slice(&a5_native_edge_run_stream(6, 139, 142));
    bytes.extend_from_slice(&plane_stream[plane_carriers[2].pos..plane_carriers[2].end]);

    let native = crate::native::CatiaNative::decode(&bytes);
    let [run] = native.consolidated_edge_runs.as_slice() else {
        panic!("one consolidated plane-bound edge run");
    };
    assert!(run
        .support_bindings
        .iter()
        .all(|binding| matches!(binding, Some(CatiaConsolidatedSupportBinding::Plane { .. }))));
    assert_eq!(run.shared_loci.as_ref().map(Vec::len), Some(2));

    let mut namespace = cadmpeg_ir::NativeNamespace::new(std::num::NonZeroU32::MIN);
    native
        .store(&mut namespace)
        .expect("store plane-bound CATIA edge run");
    assert_eq!(
        crate::native::CatiaNative::load(&namespace).expect("load plane-bound CATIA edge run"),
        native
    );

    let mut invalid = native.clone();
    let directionless_offset = invalid
        .consolidated_plane_carriers
        .iter()
        .find(|carrier| carrier.payload.selector() == 0xec)
        .expect("directionless class-27 carrier")
        .byte_offset;
    invalid.consolidated_edge_runs[0].support_bindings[0] =
        Some(CatiaConsolidatedSupportBinding::Plane {
            byte_offset: directionless_offset,
        });
    let mut invalid_namespace = cadmpeg_ir::NativeNamespace::new(std::num::NonZeroU32::MIN);
    invalid
        .store(&mut invalid_namespace)
        .expect("store invalid directionless plane binding");
    assert!(crate::native::CatiaNative::load(&invalid_namespace).is_err());

    namespace
        .set_arena(
            "consolidated_plane_carriers",
            &Vec::<crate::native::CatiaConsolidatedPlaneCarrier>::new(),
        )
        .expect("remove retained plane carriers");
    assert!(crate::native::CatiaNative::load(&namespace).is_err());
}

#[test]
fn native_namespace_retains_resolved_consolidated_torus_supports() {
    use crate::native::CatiaConsolidatedSupportBinding;

    let native = crate::native::CatiaNative::decode(&a5_torus_bound_edge_stream());
    let [run] = native.consolidated_edge_runs.as_slice() else {
        panic!("one consolidated torus edge run");
    };
    assert!(run
        .support_bindings
        .iter()
        .all(|binding| matches!(binding, Some(CatiaConsolidatedSupportBinding::Torus { .. }))));
    assert_eq!(run.shared_loci.as_ref().map(Vec::len), Some(2));

    let mut namespace = cadmpeg_ir::NativeNamespace::new(std::num::NonZeroU32::MIN);
    native
        .store(&mut namespace)
        .expect("store torus-bound CATIA edge run");
    assert_eq!(
        crate::native::CatiaNative::load(&namespace).expect("load torus-bound CATIA edge run"),
        native
    );

    namespace
        .set_arena(
            "consolidated_tori",
            &Vec::<crate::native::CatiaConsolidatedTorus>::new(),
        )
        .expect("remove retained tori");
    assert!(crate::native::CatiaNative::load(&namespace).is_err());
}

#[test]
fn native_namespace_retains_resolved_consolidated_sphere_supports() {
    use crate::native::CatiaConsolidatedSupportBinding;

    let native = crate::native::CatiaNative::decode(&a5_sphere_bound_edge_stream());
    let [run] = native.consolidated_edge_runs.as_slice() else {
        panic!("one consolidated sphere edge run");
    };
    assert!(run.support_bindings.iter().all(|binding| matches!(
        binding,
        Some(CatiaConsolidatedSupportBinding::Sphere { .. })
    )));
    assert_eq!(run.shared_loci.as_ref().map(Vec::len), Some(2));

    let mut namespace = cadmpeg_ir::NativeNamespace::new(std::num::NonZeroU32::MIN);
    native
        .store(&mut namespace)
        .expect("store sphere-bound CATIA edge run");
    assert_eq!(
        crate::native::CatiaNative::load(&namespace).expect("load sphere-bound CATIA edge run"),
        native
    );

    namespace
        .set_arena(
            "consolidated_spheres",
            &Vec::<crate::native::CatiaConsolidatedSphere>::new(),
        )
        .expect("remove retained spheres");
    assert!(crate::native::CatiaNative::load(&namespace).is_err());
}

#[test]
fn native_namespace_retains_embedded_cylinders_with_their_owning_group() {
    let native = crate::native::CatiaNative::decode(&b2_embedded_cylinder_stream());
    assert!(native.consolidated_cylinders.is_empty());
    let [group] = native.consolidated_groups.as_slice() else {
        panic!("one consolidated group");
    };
    let [cylinder] = native.consolidated_embedded_cylinders.as_slice() else {
        panic!("one embedded consolidated cylinder");
    };
    assert_eq!(group.group_type, 3);
    assert_eq!(cylinder.group, group.id);
    assert_eq!(cylinder.object_id, 0x5678);
    assert_eq!(cylinder.u_range, [0.0, 4.0 * std::f64::consts::PI]);

    let mut namespace = cadmpeg_ir::NativeNamespace::new(std::num::NonZeroU32::MIN);
    native
        .store(&mut namespace)
        .expect("store embedded CATIA cylinder");
    assert_eq!(
        crate::native::CatiaNative::load(&namespace).expect("load embedded CATIA cylinder"),
        native
    );

    namespace
        .set_arena(
            "consolidated_groups",
            &Vec::<crate::native::CatiaConsolidatedGroup>::new(),
        )
        .expect("remove owning consolidated group");
    assert!(crate::native::CatiaNative::load(&namespace).is_err());

    let mut two_groups = b2_embedded_cylinder_stream();
    two_groups.extend_from_slice(&b2_embedded_cylinder_stream());
    let mut invalid = crate::native::CatiaNative::decode(&two_groups);
    assert_eq!(invalid.consolidated_groups.len(), 2);
    assert_eq!(invalid.consolidated_embedded_cylinders.len(), 2);
    invalid.consolidated_embedded_cylinders[1]
        .group
        .clone_from(&invalid.consolidated_groups[0].id);
    let mut invalid_namespace = cadmpeg_ir::NativeNamespace::new(std::num::NonZeroU32::MIN);
    invalid
        .store(&mut invalid_namespace)
        .expect("store cross-group embedded cylinder");
    assert!(crate::native::CatiaNative::load(&invalid_namespace).is_err());
}

#[test]
fn native_namespace_binds_edges_to_retained_embedded_cylinders() {
    use crate::native::CatiaConsolidatedSupportBinding;

    let mut bytes = b2_embedded_cylinder_stream();
    for point in [
        [1.0f32, 4.0, 3.0],
        [2.0, 2.0 + 2.0 * 0.5f32.cos(), 3.0 + 2.0 * 0.5f32.sin()],
    ] {
        bytes.extend_from_slice(&[0x05, 0x08, 0x01]);
        for value in point {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
    }
    bytes.extend_from_slice(&a5_native_edge_run_stream(6, 139, 142));

    let native = crate::native::CatiaNative::decode(&bytes);
    let [run] = native.consolidated_edge_runs.as_slice() else {
        panic!("one consolidated edge run");
    };
    assert!(run.support_bindings.iter().all(|binding| matches!(
        binding,
        Some(CatiaConsolidatedSupportBinding::EmbeddedCylinder { .. })
    )));

    let mut namespace = cadmpeg_ir::NativeNamespace::new(std::num::NonZeroU32::MIN);
    native
        .store(&mut namespace)
        .expect("store embedded-cylinder edge binding");
    assert_eq!(
        crate::native::CatiaNative::load(&namespace).expect("load embedded-cylinder edge binding"),
        native
    );
}

#[test]
fn native_namespace_binds_embedded_cylinder_by_unique_pcurve_support_identity() {
    use crate::native::CatiaConsolidatedSupportBinding;

    let mut bytes = b2_embedded_cylinder_stream_with_object_id(0x5678);
    bytes.extend_from_slice(&b2_embedded_cylinder_stream_with_object_id(0x9abc));
    for point in [
        [1.0f32, 4.0, 3.0],
        [2.0, 2.0 + 2.0 * 0.5f32.cos(), 3.0 + 2.0 * 0.5f32.sin()],
    ] {
        bytes.extend_from_slice(&[0x05, 0x08, 0x01]);
        for value in point {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
    }
    bytes.extend_from_slice(&a5_native_edge_run_stream_with_support(6, 139, 142, 0x5678));

    let native = crate::native::CatiaNative::decode(&bytes);
    let [first, second] = native.consolidated_embedded_cylinders.as_slice() else {
        panic!("two embedded consolidated cylinders");
    };
    assert_ne!(first.object_id, second.object_id);
    let [first_group, _second_group] = native.consolidated_groups.as_slice() else {
        panic!("two consolidated groups");
    };
    let [run] = native.consolidated_edge_runs.as_slice() else {
        panic!("one consolidated edge run");
    };
    let expected = Some(CatiaConsolidatedSupportBinding::EmbeddedCylinder {
        byte_offset: first.byte_offset,
        wrapper_byte_offset: first_group.byte_offset,
    });
    assert_eq!(run.support_bindings, [expected.clone(), expected]);
}

#[test]
fn native_namespace_withholds_duplicate_embedded_pcurve_support_identity() {
    let mut bytes = b2_embedded_cylinder_stream_with_object_id(0x5678);
    bytes.extend_from_slice(&b2_embedded_cylinder_stream_with_object_id(0x5678));
    for point in [
        [1.0f32, 4.0, 3.0],
        [2.0, 2.0 + 2.0 * 0.5f32.cos(), 3.0 + 2.0 * 0.5f32.sin()],
    ] {
        bytes.extend_from_slice(&[0x05, 0x08, 0x01]);
        for value in point {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
    }
    bytes.extend_from_slice(&a5_native_edge_run_stream_with_support(6, 139, 142, 0x5678));

    let native = crate::native::CatiaNative::decode(&bytes);
    let [run] = native.consolidated_edge_runs.as_slice() else {
        panic!("one consolidated edge run");
    };
    assert_eq!(run.support_bindings, [None, None]);
}
