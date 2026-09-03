// SPDX-License-Identifier: Apache-2.0
//! Zero-entity dump tests over synthetic CATPart streams.

#![allow(clippy::doc_markdown, clippy::unwrap_used)]

use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::geometry::SurfaceGeometry;

use crate::test_support::*;
use crate::variant::Variant;
use crate::CatiaCodec;

#[test]
fn decode_zero_entity_falls_back_to_metadata() {
    let f = zero_entity_catpart();
    let scan = crate::container::scan_bytes(f.clone());
    assert_eq!(scan.variant, Variant::ZeroEntity);
    assert!(scan.inner.is_none());

    let mut cur = Cursor::new(f);
    let result = CatiaCodec
        .decode(&mut cur, &DecodeOptions::default())
        .unwrap();
    assert!(!result.report().geometry_transferred());
    let source = result.ir().source.as_ref().expect("source metadata");
    assert_eq!(
        source
            .dialect()
            .expect("classified source")
            .dialect()
            .as_str(),
        "catia:zero-entity"
    );
    assert!(result
        .report()
        .losses
        .iter()
        .any(|l| l.message.contains("catia:zero-entity")));
}

#[test]
fn zero_entity_directory_markers_stay_outside_the_record_stream() {
    let mut body = vec![0u8; 16];
    body[12..].copy_from_slice(&[0xa9, 0x03, 0x10, 0x08]);
    let directory = [0xa9, 0x03, 0x10, 0x00, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
    let directory_offset = 16 + body.len();
    let mut file = Vec::new();
    file.extend_from_slice(OUTER_MAGIC);
    file.extend_from_slice(&be32(
        u32::try_from(directory_offset).expect("bounded directory offset"),
    ));
    file.extend_from_slice(&be32(
        u32::try_from(directory.len()).expect("bounded directory length"),
    ));
    file.extend_from_slice(&body);
    file.extend_from_slice(&directory);

    let scan = crate::container::scan_bytes(file);
    assert_eq!(scan.census.a9_records, 0);
    assert_eq!(scan.variant, Variant::Unknown);
    let ranges = crate::container::consolidated_record_ranges(&scan);
    let native = crate::native::CatiaNative::decode_with_record_ranges(&scan.data, &ranges);
    assert!(native.zero_entity_records.is_empty());
    assert!(native.zero_entity_support_runs.is_empty());
}

#[test]
fn zero_entity_finjpl_records_stay_outside_the_record_stream() {
    let record = [0xa9, 0x03, 0x10, 0x00, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
    let mut body = record.to_vec();
    body.extend_from_slice(b"FINJPL  ");
    body.extend_from_slice(&record);
    let directory = [0u8; 16];
    let directory_offset = 16 + body.len();
    let mut file = Vec::new();
    file.extend_from_slice(OUTER_MAGIC);
    file.extend_from_slice(&be32(
        u32::try_from(directory_offset).expect("bounded directory offset"),
    ));
    file.extend_from_slice(&be32(
        u32::try_from(directory.len()).expect("bounded directory length"),
    ));
    file.extend_from_slice(&body);
    file.extend_from_slice(&directory);

    let scan = crate::container::scan_bytes(file);
    assert_eq!(scan.census.a9_records, 1);
    assert_eq!(scan.variant, Variant::ZeroEntity);
    let ranges = crate::container::consolidated_record_ranges(&scan);
    let native = crate::native::CatiaNative::decode_with_record_ranges(&scan.data, &ranges);
    assert_eq!(native.zero_entity_records.len(), 1);
}

#[test]
fn decode_zero_entity_transfers_framed_cylinder() {
    let mut cur = Cursor::new(zero_entity_cylinder_catpart());
    let result = CatiaCodec
        .decode(&mut cur, &DecodeOptions::default())
        .unwrap();
    assert!(result.report().geometry_transferred());
    assert_eq!(result.ir().model.surfaces.len(), 1);
    assert!(result.ir().model.points.is_empty());
    assert!(result.ir().model.vertices.is_empty());
    assert!(result.ir().model.bodies.is_empty());
    assert!(result.ir().model.shells.is_empty());
    match &result.ir().model.surfaces[0].geometry {
        SurfaceGeometry::Cylinder {
            origin,
            axis,
            ref_direction,
            radius,
        } => {
            assert_eq!(*origin, cadmpeg_ir::math::Point3::new(1.0, 2.0, 3.0));
            assert_eq!(*axis, cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0));
            assert_eq!(
                *ref_direction,
                cadmpeg_ir::math::Vector3::new(1.0, 0.0, 0.0)
            );
            assert_eq!(*radius, 4.0);
        }
        other => panic!("expected cylinder, got {other:?}"),
    }
    let validation = cadmpeg_ir::validate::validate_neutral(result.ir(), Vec::new());
    assert!(validation.is_ok(), "findings: {:?}", validation.findings);
}

#[test]
fn decode_zero_entity_transfers_parametric_surface_curve_without_a_cache() {
    let result = CatiaCodec
        .decode(
            &mut Cursor::new(zero_entity_cylinder_parametric_support_catpart()),
            &DecodeOptions::default(),
        )
        .expect("decode zero-entity parametric support");

    assert_eq!(
        result
            .report()
            .coverage_count(crate::coverage::TRANSFERRED_ZERO_ENTITY_SUPPORT_CURVE_COUNT),
        1
    );
    assert_eq!(
        result.report().coverage_count(
            crate::coverage::TRANSFERRED_ZERO_ENTITY_PARAMETRIC_SURFACE_CURVE_COUNT
        ),
        1
    );
    let [curve] = result.ir().model.curves.as_slice() else {
        panic!("one transferred support curve")
    };
    let [construction] = result.ir().model.procedural_curves.as_slice() else {
        panic!("one cacheless support construction")
    };
    assert!(matches!(
        &curve.geometry,
        cadmpeg_ir::geometry::CurveGeometry::Procedural {
            construction: id
        } if id == &construction.id
    ));
    assert_eq!(construction.curve, curve.id);
    assert_eq!(construction.cache_fit_tolerance(), None);
    let cadmpeg_ir::geometry::ProceduralCurveDefinition::SurfaceCurve {
        family,
        context,
        tail: None,
    } = construction.definition()
    else {
        panic!("parametric surface-curve construction")
    };
    assert_eq!(
        *family,
        cadmpeg_ir::geometry::SurfaceCurveFamily::Parametric
    );
    assert_eq!(context.parameter_range, [0.0, 1.0]);
    assert_eq!(
        context.sides[0].surface.as_ref(),
        Some(&result.ir().model.surfaces[0].id)
    );
    assert!(context.sides[0].pcurve.is_some());
    assert_eq!(context.sides[1].surface, None);
    assert_eq!(context.sides[1].pcurve, None);

    let validation = cadmpeg_ir::validate::validate_neutral(result.ir(), Vec::new());
    assert!(validation.is_ok(), "findings: {:?}", validation.findings);
}

#[test]
fn decode_zero_entity_transfers_exact_model_curve_directly() {
    let mut file = vec![0u8; 16];
    file[..8].copy_from_slice(OUTER_MAGIC);
    file.extend(zero_entity_support_stream());
    let result = CatiaCodec
        .decode(&mut Cursor::new(file), &DecodeOptions::default())
        .expect("decode zero-entity exact support");

    assert_eq!(
        result
            .report()
            .coverage_count(crate::coverage::TRANSFERRED_ZERO_ENTITY_SUPPORT_CURVE_COUNT),
        1
    );
    assert_eq!(
        result.report().coverage_count(
            crate::coverage::TRANSFERRED_ZERO_ENTITY_PARAMETRIC_SURFACE_CURVE_COUNT
        ),
        0
    );
    assert!(matches!(
        result.ir().model.curves.as_slice(),
        [cadmpeg_ir::geometry::Curve {
            geometry: cadmpeg_ir::geometry::CurveGeometry::Nurbs(_),
            ..
        }]
    ));
    assert!(result.ir().model.procedural_curves.is_empty());

    let validation = cadmpeg_ir::validate::validate_neutral(result.ir(), Vec::new());
    assert!(validation.is_ok(), "findings: {:?}", validation.findings);
}

#[test]
fn decode_zero_entity_transfers_inline_nurbs_surface() {
    let mut cur = Cursor::new(zero_entity_nurbs_catpart());
    let result = CatiaCodec
        .decode(&mut cur, &DecodeOptions::default())
        .unwrap();
    assert_eq!(result.ir().model.surfaces.len(), 1);
    match &result.ir().model.surfaces[0].geometry {
        SurfaceGeometry::Nurbs(surface) => {
            assert_eq!((surface.u_degree, surface.v_degree), (3, 3));
            assert_eq!((surface.u_count, surface.v_count), (7, 7));
            assert_eq!(
                surface.u_knots,
                vec![0.0, 0.0, 0.0, 0.0, 0.25, 0.5, 0.75, 1.0, 1.0, 1.0, 1.0]
            );
            assert_eq!(surface.control_points.len(), 49);
            assert_eq!(surface.control_points[48].x, 48.0);
        }
        other => panic!("expected NURBS surface, got {other:?}"),
    }
}

#[test]
fn native_namespace_retains_zero_entity_surface_support_runs() {
    let mut stream = zero_entity_face_loop_support_stream();
    let support_slot = 0x6a + 12 + 13;
    stream[support_slot..support_slot + 4].copy_from_slice(&1u32.to_le_bytes());
    let native = crate::native::CatiaNative::decode(&stream);
    assert!(native.zero_entity_endpoint_pair_candidates.is_empty());
    let [run] = native.zero_entity_support_runs.as_slice() else {
        panic!("one zero-entity support run")
    };
    assert_eq!(run.carrier_byte_offset, 0);
    assert_eq!(run.carrier_record_ordinal, 1);
    let face = run.face.as_ref().expect("positionally aligned face");
    assert_eq!(face.record_ordinal, 3);
    assert_eq!(face.allocations, [10, 3]);
    assert_eq!(face.loop_terminals, [7]);
    let [loop_record] = face.loops.as_slice() else {
        panic!("one loop")
    };
    assert_eq!(loop_record.member_ids, [6]);
    assert_eq!(loop_record.typed_references, [1]);
    assert_eq!(
        loop_record.typed_records,
        ["catia:zero-entity:record#1".to_string()]
    );
    assert_eq!(loop_record.terminal_id, 7);
    assert_eq!(loop_record.loop_class, 0x41);
    assert_eq!(loop_record.forward_senses, [true]);
    assert_eq!(loop_record.support_record_ordinals, [2]);
    assert!(loop_record.oriented_model_endpoints.is_empty());
    let [support] = run.supports.as_slice() else {
        panic!("one zero-entity support occurrence")
    };
    assert_eq!(support.tag, [0x21, 0x71]);
    assert_eq!(support.record_ordinal, 2);
    assert_eq!(support.face_local_slot, 1);
    assert_eq!(support.uv_endpoints, Some([[-2.0, 4.0], [6.0, 8.0]]));
    assert!(matches!(
        support.pcurve,
        Some(cadmpeg_ir::geometry::PcurveGeometry::Nurbs {
            degree: 1,
            ref control_points,
            weights: None,
            periodic: false,
            ..
        }) if control_points.len() == 2
    ));
    assert!(matches!(
        support.model_curve,
        Some(cadmpeg_ir::geometry::CurveGeometry::Nurbs(
            cadmpeg_ir::geometry::NurbsCurve {
                degree: 1,
                ref control_points,
                weights: None,
                periodic: false,
                ..
            }
        )) if control_points.len() == 2
    ));
    assert!(support.model_curve_construction.is_none());
    assert_eq!(support.model_parameters, Some([0.0, 1.0]));
    assert_eq!(
        support.model_midpoint,
        Some(cadmpeg_ir::math::Point3::new(3.0, 8.0, 3.0))
    );
    assert_eq!(
        support.model_endpoints,
        Some([
            cadmpeg_ir::math::Point3::new(-1.0, 6.0, 3.0),
            cadmpeg_ir::math::Point3::new(7.0, 10.0, 3.0),
        ])
    );

    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    native
        .store(&mut namespace)
        .expect("store CATIA zero-entity support run");
    assert_eq!(
        crate::native::CatiaNative::load(&namespace).expect("load CATIA zero-entity support run"),
        native
    );

    let mut invalid_face = native.clone();
    invalid_face.zero_entity_support_runs[0]
        .face
        .as_mut()
        .expect("face")
        .loop_terminals[0] = 8;
    let mut invalid_face_namespace = cadmpeg_ir::NativeNamespace::default();
    invalid_face
        .store(&mut invalid_face_namespace)
        .expect("store invalid CATIA zero-entity face");
    assert!(crate::native::CatiaNative::load(&invalid_face_namespace).is_err());

    let mut zero_face_terminal = native.clone();
    zero_face_terminal.zero_entity_support_runs[0]
        .face
        .as_mut()
        .expect("face")
        .loop_terminals[0] = 0;
    let mut zero_face_terminal_namespace = cadmpeg_ir::NativeNamespace::default();
    zero_face_terminal
        .store(&mut zero_face_terminal_namespace)
        .expect("store zero CATIA zero-entity face loop terminal");
    assert!(crate::native::CatiaNative::load(&zero_face_terminal_namespace).is_err());

    let mut invalid_loop_roster = native.clone();
    invalid_loop_roster.zero_entity_support_runs[0]
        .face
        .as_mut()
        .expect("face")
        .loops[0]
        .loop_class = 0x50;
    let mut invalid_loop_roster_namespace = cadmpeg_ir::NativeNamespace::default();
    invalid_loop_roster
        .store(&mut invalid_loop_roster_namespace)
        .expect("store invalid CATIA zero-entity loop roster");
    assert!(crate::native::CatiaNative::load(&invalid_loop_roster_namespace).is_err());

    let mut invalid_face_allocation = native.clone();
    invalid_face_allocation.zero_entity_support_runs[0]
        .face
        .as_mut()
        .expect("face")
        .allocations[0] = 0;
    let mut invalid_face_allocation_namespace = cadmpeg_ir::NativeNamespace::default();
    invalid_face_allocation
        .store(&mut invalid_face_allocation_namespace)
        .expect("store invalid CATIA zero-entity face allocation");
    assert!(crate::native::CatiaNative::load(&invalid_face_allocation_namespace).is_err());

    let mut invalid_face_control = native.clone();
    invalid_face_control.zero_entity_support_runs[0]
        .face
        .as_mut()
        .expect("face")
        .terminal_control = 0x04;
    let mut invalid_face_control_namespace = cadmpeg_ir::NativeNamespace::default();
    invalid_face_control
        .store(&mut invalid_face_control_namespace)
        .expect("store invalid CATIA zero-entity face control");
    assert!(crate::native::CatiaNative::load(&invalid_face_control_namespace).is_err());

    let mut invalid_loop_gap = native.clone();
    invalid_loop_gap.zero_entity_support_runs[0]
        .face
        .as_mut()
        .expect("face")
        .loops[0]
        .gap = 0;
    let mut invalid_loop_gap_namespace = cadmpeg_ir::NativeNamespace::default();
    invalid_loop_gap
        .store(&mut invalid_loop_gap_namespace)
        .expect("store invalid CATIA zero-entity loop gap");
    assert!(crate::native::CatiaNative::load(&invalid_loop_gap_namespace).is_err());

    let mut invalid_support_slot = native.clone();
    invalid_support_slot.zero_entity_support_runs[0].supports[0].face_local_slot = 0;
    let mut invalid_support_slot_namespace = cadmpeg_ir::NativeNamespace::default();
    invalid_support_slot
        .store(&mut invalid_support_slot_namespace)
        .expect("store invalid CATIA zero-entity support slot");
    assert!(crate::native::CatiaNative::load(&invalid_support_slot_namespace).is_err());

    let mut invalid_loop = native.clone();
    invalid_loop.zero_entity_support_runs[0]
        .face
        .as_mut()
        .expect("face")
        .loops[0]
        .forward_senses
        .clear();
    let mut invalid_loop_namespace = cadmpeg_ir::NativeNamespace::default();
    invalid_loop
        .store(&mut invalid_loop_namespace)
        .expect("store invalid CATIA zero-entity loop");
    assert!(crate::native::CatiaNative::load(&invalid_loop_namespace).is_err());

    let mut invalid_typed_record = native.clone();
    invalid_typed_record.zero_entity_support_runs[0]
        .face
        .as_mut()
        .expect("face")
        .loops[0]
        .typed_records[0] = "catia:zero-entity:record#2".to_string();
    let mut invalid_typed_record_namespace = cadmpeg_ir::NativeNamespace::default();
    invalid_typed_record
        .store(&mut invalid_typed_record_namespace)
        .expect("store invalid CATIA zero-entity typed loop reference");
    assert!(crate::native::CatiaNative::load(&invalid_typed_record_namespace).is_err());

    let mut invalid_binding = native.clone();
    invalid_binding.zero_entity_support_runs[0]
        .face
        .as_mut()
        .expect("face")
        .loops[0]
        .support_record_ordinals[0] = 1;
    let mut invalid_binding_namespace = cadmpeg_ir::NativeNamespace::default();
    invalid_binding
        .store(&mut invalid_binding_namespace)
        .expect("store invalid CATIA zero-entity loop support binding");
    assert!(crate::native::CatiaNative::load(&invalid_binding_namespace).is_err());

    let mut invalid_pcurve = native.clone();
    let Some(cadmpeg_ir::geometry::PcurveGeometry::Nurbs { degree, .. }) =
        invalid_pcurve.zero_entity_support_runs[0].supports[0]
            .pcurve
            .as_mut()
    else {
        panic!("NURBS support pcurve")
    };
    *degree = 2;
    let mut invalid_pcurve_namespace = cadmpeg_ir::NativeNamespace::default();
    invalid_pcurve
        .store(&mut invalid_pcurve_namespace)
        .expect("store invalid CATIA zero-entity support pcurve");
    assert!(crate::native::CatiaNative::load(&invalid_pcurve_namespace).is_err());

    let mut invalid_model_curve = native.clone();
    let Some(cadmpeg_ir::geometry::CurveGeometry::Nurbs(model_curve)) =
        invalid_model_curve.zero_entity_support_runs[0].supports[0]
            .model_curve
            .as_mut()
    else {
        panic!("NURBS support model curve")
    };
    model_curve.periodic = true;
    let mut invalid_model_curve_namespace = cadmpeg_ir::NativeNamespace::default();
    invalid_model_curve
        .store(&mut invalid_model_curve_namespace)
        .expect("store invalid CATIA zero-entity support model curve");
    assert!(crate::native::CatiaNative::load(&invalid_model_curve_namespace).is_err());

    let mut invalid_model_parameters = native.clone();
    invalid_model_parameters.zero_entity_support_runs[0].supports[0].model_parameters =
        Some([1.0, 1.0]);
    let mut invalid_model_parameters_namespace = cadmpeg_ir::NativeNamespace::default();
    invalid_model_parameters
        .store(&mut invalid_model_parameters_namespace)
        .expect("store invalid CATIA zero-entity support model parameters");
    assert!(crate::native::CatiaNative::load(&invalid_model_parameters_namespace).is_err());

    let mut missing_model_midpoint = native.clone();
    missing_model_midpoint.zero_entity_support_runs[0].supports[0].model_midpoint = None;
    let mut missing_model_midpoint_namespace = cadmpeg_ir::NativeNamespace::default();
    missing_model_midpoint
        .store(&mut missing_model_midpoint_namespace)
        .expect("store CATIA zero-entity support without its model midpoint");
    assert!(crate::native::CatiaNative::load(&missing_model_midpoint_namespace).is_err());

    let mut invalid_model_construction = native.clone();
    invalid_model_construction.zero_entity_support_runs[0].supports[0].model_curve_construction =
        Some(cadmpeg_ir::geometry::ProceduralCurveDefinition::Helix {
            angle_range: [0.0, 1.0],
            center: cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
            major: cadmpeg_ir::math::Vector3::new(1.0, 0.0, 0.0),
            minor: cadmpeg_ir::math::Vector3::new(0.0, 1.0, 0.0),
            pitch: cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0),
            apex_factor: 1.0,
            axis: cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0),
        });
    let mut invalid_model_construction_namespace = cadmpeg_ir::NativeNamespace::default();
    invalid_model_construction
        .store(&mut invalid_model_construction_namespace)
        .expect("store invalid CATIA zero-entity support model construction");
    assert!(crate::native::CatiaNative::load(&invalid_model_construction_namespace).is_err());

    let mut invalid_oriented_endpoints = native.clone();
    invalid_oriented_endpoints.zero_entity_support_runs[0]
        .face
        .as_mut()
        .expect("face")
        .loops[0]
        .oriented_model_endpoints
        .push([
            cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
            cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
        ]);
    let mut invalid_oriented_endpoint_namespace = cadmpeg_ir::NativeNamespace::default();
    invalid_oriented_endpoints
        .store(&mut invalid_oriented_endpoint_namespace)
        .expect("store invalid CATIA zero-entity oriented endpoints");
    assert!(crate::native::CatiaNative::load(&invalid_oriented_endpoint_namespace).is_err());

    let mut invalid_endpoint_pair = native.clone();
    invalid_endpoint_pair
        .zero_entity_endpoint_pair_candidates
        .push(crate::native::CatiaZeroEntityEndpointPairCandidate {
            id: "catia:zero-entity:endpoint-pair-candidate#0".to_string(),
            face_records: [
                "catia:zero-entity:record#3".to_string(),
                "catia:zero-entity:record#3".to_string(),
            ],
            support_records: [
                "catia:zero-entity:record#2".to_string(),
                "catia:zero-entity:record#2".to_string(),
            ],
            model_endpoints: [
                cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
                cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
            ],
            model_midpoint: cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
        });
    let mut invalid_endpoint_pair_namespace = cadmpeg_ir::NativeNamespace::default();
    invalid_endpoint_pair
        .store(&mut invalid_endpoint_pair_namespace)
        .expect("store invalid CATIA zero-entity endpoint pair");
    assert!(crate::native::CatiaNative::load(&invalid_endpoint_pair_namespace).is_err());

    let mut invalid_endpoint_locus = native.clone();
    invalid_endpoint_locus
        .zero_entity_endpoint_locus_candidates
        .push(crate::native::CatiaZeroEntityEndpointLocusCandidate {
            id: "catia:zero-entity:endpoint-locus-candidate#0".to_string(),
            incident_endpoint_pair_endpoints: vec![
                crate::native::CatiaZeroEntityEndpointPairEndpoint {
                    endpoint_pair: "catia:zero-entity:endpoint-pair-candidate#0".to_string(),
                    endpoint_index: 0,
                },
            ],
            representative_point: cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
            maximum_deviation: 0.0,
        });
    let mut invalid_endpoint_locus_namespace = cadmpeg_ir::NativeNamespace::default();
    invalid_endpoint_locus
        .store(&mut invalid_endpoint_locus_namespace)
        .expect("store invalid CATIA zero-entity endpoint-locus candidate");
    assert!(crate::native::CatiaNative::load(&invalid_endpoint_locus_namespace).is_err());

    let mut invalid_model_endpoint = native.clone();
    invalid_model_endpoint.zero_entity_support_runs[0].supports[0]
        .model_endpoints
        .as_mut()
        .expect("model endpoints")[0]
        .x = f64::NAN;
    let mut invalid_model_endpoint_namespace = cadmpeg_ir::NativeNamespace::default();
    invalid_model_endpoint
        .store(&mut invalid_model_endpoint_namespace)
        .expect("store invalid CATIA zero-entity model endpoint");
    assert!(crate::native::CatiaNative::load(&invalid_model_endpoint_namespace).is_err());

    let mut invalid = native;
    invalid.zero_entity_support_runs[0].supports[0].uv_endpoints = None;
    let mut invalid_namespace = cadmpeg_ir::NativeNamespace::default();
    invalid
        .store(&mut invalid_namespace)
        .expect("store invalid CATIA zero-entity support run");
    assert!(crate::native::CatiaNative::load(&invalid_namespace).is_err());
}

#[test]
fn native_namespace_retains_closed_zero_entity_endpoint_tapes() {
    let mut stream = zero_entity_face_loop_support_stream();
    let support = 0x6a + 12;
    stream[support + 13..support + 17].copy_from_slice(&1u32.to_le_bytes());
    let first_endpoint: [u8; 16] = stream[support + 93..support + 109]
        .try_into()
        .expect("endpoint pair");
    stream[support + 109..support + 125].copy_from_slice(&first_endpoint);

    let native = crate::native::CatiaNative::decode(&stream);
    let loop_record = &native.zero_entity_support_runs[0]
        .face
        .as_ref()
        .expect("face")
        .loops[0];
    assert_eq!(
        loop_record.oriented_model_endpoints,
        [[
            cadmpeg_ir::math::Point3::new(-1.0, 6.0, 3.0),
            cadmpeg_ir::math::Point3::new(-1.0, 6.0, 3.0),
        ]]
    );

    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    native
        .store(&mut namespace)
        .expect("store CATIA zero-entity endpoint tape");
    assert_eq!(
        crate::native::CatiaNative::load(&namespace).expect("load CATIA zero-entity endpoint tape"),
        native
    );
}

#[test]
fn native_namespace_retains_zero_entity_ownership_root() {
    let mut stream = zero_entity_face_support_stream();
    stream.extend(zero_entity_ownership_stream(1));
    let native = crate::native::CatiaNative::decode(&stream);
    let [root] = native.zero_entity_ownership_roots.as_slice() else {
        panic!("one zero-entity ownership root")
    };
    assert_eq!(root.face_slots, [1]);
    assert_eq!(root.face_roster_record_ordinal, 4);
    assert_eq!(root.shell_record_ordinal, 5);
    assert_eq!(root.body_record_ordinal, 6);
    assert_eq!(
        native.zero_entity_records[3].logical_end,
        root.shell_byte_offset
    );

    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    native
        .store(&mut namespace)
        .expect("store CATIA zero-entity ownership root");
    assert_eq!(
        crate::native::CatiaNative::load(&namespace)
            .expect("load CATIA zero-entity ownership root"),
        native
    );

    let mut invalid = native;
    invalid.zero_entity_ownership_roots[0].face_slots.clear();
    let mut invalid_namespace = cadmpeg_ir::NativeNamespace::default();
    invalid
        .store(&mut invalid_namespace)
        .expect("store invalid CATIA zero-entity ownership root");
    assert!(crate::native::CatiaNative::load(&invalid_namespace).is_err());
}

#[test]
fn native_namespace_retains_multiple_zero_entity_ownership_candidates() {
    let mut stream = zero_entity_face_support_stream();
    stream.extend(zero_entity_ownership_stream(1));
    stream.extend(zero_entity_ownership_stream(1));

    let native = crate::native::CatiaNative::decode(&stream);
    assert_eq!(native.zero_entity_ownership_roots.len(), 2);
    assert_eq!(
        native.zero_entity_ownership_roots[0].id,
        "catia:zero-entity:ownership-root#0"
    );
    assert_eq!(
        native.zero_entity_ownership_roots[1].id,
        "catia:zero-entity:ownership-root#1"
    );
    assert_eq!(
        native.zero_entity_ownership_roots[0].face_roster_record_ordinal,
        4
    );
    assert_eq!(
        native.zero_entity_ownership_roots[1].face_roster_record_ordinal,
        7
    );

    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    native
        .store(&mut namespace)
        .expect("store multiple CATIA zero-entity ownership candidates");
    assert_eq!(
        crate::native::CatiaNative::load(&namespace)
            .expect("load multiple CATIA zero-entity ownership candidates"),
        native
    );
}

#[test]
fn native_namespace_retains_separate_zero_entity_topology_registries() {
    let native = crate::native::CatiaNative::decode(&zero_entity_topology_stream());
    assert_eq!(native.zero_entity_records.len(), 8);
    assert_eq!(native.zero_entity_records[0].record_ordinal, 1);
    assert_eq!(native.zero_entity_records[0].tag, [0x5e, 0x1a]);
    let [edge_stride] = native.zero_entity_edge_strides.as_slice() else {
        panic!("one zero-entity edge stride")
    };
    assert_eq!(edge_stride.record_ordinal, 1);
    assert_eq!(edge_stride.allocations, [5, 7, 8, 4, 3]);
    assert_eq!(edge_stride.topology_refs, [5, 4, 3]);
    assert_eq!(edge_stride.surface_support_refs, [7, 8]);

    let [pair] = native.zero_entity_oriented_use_pairs.as_slice() else {
        panic!("one zero-entity oriented-use pair")
    };
    assert_eq!(pair.header_record_ordinal, 2);
    assert_eq!(pair.base_columns, [100, 200]);

    let [incidence] = native.zero_entity_vertex_incidences.as_slice() else {
        panic!("one zero-entity vertex incidence")
    };
    assert_eq!(incidence.record_ordinal, 5);
    assert_eq!(incidence.allocations, [1, 2, 5]);
    assert_eq!(
        incidence.vertex_record.as_deref(),
        Some("catia:zero-entity:record#6")
    );

    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    native
        .store(&mut namespace)
        .expect("store CATIA zero-entity topology registries");
    assert_eq!(
        crate::native::CatiaNative::load(&namespace)
            .expect("load CATIA zero-entity topology registries"),
        native
    );

    let mut invalid = native.clone();
    invalid.zero_entity_edge_strides[0].allocations[0] = 0;
    let mut invalid_namespace = cadmpeg_ir::NativeNamespace::default();
    invalid
        .store(&mut invalid_namespace)
        .expect("store invalid CATIA zero-entity edge allocation");
    assert!(crate::native::CatiaNative::load(&invalid_namespace).is_err());

    let mut invalid = native.clone();
    invalid.zero_entity_vertex_incidences[0].vertex_record = None;
    let mut invalid_namespace = cadmpeg_ir::NativeNamespace::default();
    invalid
        .store(&mut invalid_namespace)
        .expect("store invalid CATIA zero-entity vertex owner");
    assert!(crate::native::CatiaNative::load(&invalid_namespace).is_err());

    let mut invalid = native;
    invalid.zero_entity_oriented_use_pairs[0].uses[1].allocations[0] += 1;
    let mut invalid_namespace = cadmpeg_ir::NativeNamespace::default();
    invalid
        .store(&mut invalid_namespace)
        .expect("store invalid CATIA zero-entity topology registries");
    assert!(crate::native::CatiaNative::load(&invalid_namespace).is_err());
}

#[test]
fn zero_entity_vertex_binding_declines_atomically_when_structure_changes() {
    let bytes = zero_entity_topology_stream();
    let native = crate::native::CatiaNative::decode(&bytes);
    let vertex_offset =
        usize::try_from(native.zero_entity_records[5].byte_offset).expect("fixture byte offset");

    let mut missing_vertex = bytes;
    missing_vertex[vertex_offset + 2] = 0x60;
    let missing_vertex = crate::native::CatiaNative::decode(&missing_vertex);
    assert!(missing_vertex.zero_entity_vertex_incidences.is_empty());

    let mut separated_vertex = zero_entity_topology_stream();
    separated_vertex.insert(vertex_offset, 0xff);
    let separated_vertex = crate::native::CatiaNative::decode(&separated_vertex);
    assert!(separated_vertex.zero_entity_vertex_incidences.is_empty());
}

#[test]
fn decode_reports_zero_entity_surface_support_runs() {
    let mut file = standard_catpart();
    file.splice(16..16, zero_entity_face_loop_support_stream());
    let file_len = u32::try_from(file.len()).expect("bounded CATPart fixture");
    file[8..12].copy_from_slice(&be32(file_len));
    let decoded = CatiaCodec
        .decode(&mut Cursor::new(file), &DecodeOptions::default())
        .expect("decode zero-entity support run");
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_ZERO_ENTITY_FACE_BOUND_SUPPORT_RUN_COUNT),
        1
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_ZERO_ENTITY_FACE_TERMINAL_CONTROL_03_COUNT),
        0
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_ZERO_ENTITY_FACE_TERMINAL_CONTROL_05_COUNT),
        1
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_ZERO_ENTITY_LOOP_TERMINAL_COUNT),
        1
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_ZERO_ENTITY_LOOP_RECORD_COUNT),
        1
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_ZERO_ENTITY_LOOP_CLASS_41_COUNT),
        1
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_ZERO_ENTITY_LOOP_CLASS_50_COUNT),
        0
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_ZERO_ENTITY_LOOP_CLASS_C1_COUNT),
        0
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_ZERO_ENTITY_FORWARD_LOOP_MEMBER_COUNT),
        1
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_ZERO_ENTITY_REVERSED_LOOP_MEMBER_COUNT),
        0
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_ZERO_ENTITY_ORIENTED_LOOP_MEMBER_COUNT),
        1
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_ZERO_ENTITY_SUPPORT_RUN_COUNT),
        1
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_ZERO_ENTITY_SUPPORT_OCCURRENCE_COUNT),
        1
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_ZERO_ENTITY_SUPPORT_PCURVE_COUNT),
        1
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_ZERO_ENTITY_SUPPORT_MODEL_CURVE_COUNT),
        1
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_ZERO_ENTITY_SUPPORT_MODEL_CONSTRUCTION_COUNT),
        0
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_ZERO_ENTITY_UV_ENDPOINT_PAIR_COUNT),
        1
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_ZERO_ENTITY_MODEL_MIDPOINT_COUNT),
        1
    );
    assert!(decoded.report().losses.iter().any(|loss| {
        loss.code.category() == cadmpeg_ir::report::LossCategory::Topology
            && loss
                .message
                .contains("1 zero-entity surface-support run(s)")
            && loss
                .message
                .contains("1 run(s) bind the complete face roster")
            && loss.message.contains("1 stored member sense(s)")
            && loss.message.contains("oriented-use")
    }));
}

#[test]
fn decode_reports_separate_zero_entity_topology_registries() {
    let mut file = standard_catpart();
    file.splice(16..16, zero_entity_topology_stream());
    let file_len = u32::try_from(file.len()).expect("bounded CATPart fixture");
    file[8..12].copy_from_slice(&be32(file_len));
    let decoded = CatiaCodec
        .decode(&mut Cursor::new(file), &DecodeOptions::default())
        .expect("decode zero-entity topology registries");
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_ZERO_ENTITY_RECORD_COUNT),
        8
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_ZERO_ENTITY_EDGE_STRIDE_COUNT),
        1
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_ZERO_ENTITY_EDGE_STRIDE_ALLOCATION_COUNT),
        5
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_ZERO_ENTITY_EDGE_STRIDE_TOPOLOGY_REF_COUNT,),
        3
    );
    assert_eq!(
        decoded.report().coverage_count(
            crate::coverage::DECODED_ZERO_ENTITY_EDGE_STRIDE_SURFACE_SUPPORT_REF_COUNT,
        ),
        2
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_ZERO_ENTITY_ORIENTED_USE_PAIR_COUNT),
        1
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_ZERO_ENTITY_ORIENTED_USE_COUNT),
        2
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_ZERO_ENTITY_ORIENTED_USE_ALLOCATION_COUNT),
        4
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_ZERO_ENTITY_VERTEX_INCIDENCE_COUNT),
        1
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_ZERO_ENTITY_VERTEX_INCIDENCE_ALLOCATION_COUNT),
        3
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_ZERO_ENTITY_VERTEX_OWNER_BINDING_COUNT),
        1
    );
    assert!(decoded.report().losses.iter().any(|loss| {
        loss.code.category() == cadmpeg_ir::report::LossCategory::Topology
            && loss.message.contains("1 edge-stride allocation tuple(s)")
            && loss.message.contains("1 oriented-use pair(s)")
            && loss.message.contains("1 vertex-incidence record(s)")
            && loss.message.contains("remain separate")
            && loss.message.contains("bind their adjacent vertex owner")
            && loss.message.contains("loop-to-use")
    }));
}
