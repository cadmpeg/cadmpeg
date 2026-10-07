// SPDX-License-Identifier: Apache-2.0
    #[test]
    fn numerical_followup_legacy_vertex_mean_refuses_a_broken_scaled_sum() {
        // Three endpoints scaled by the largest of them sum to three at most.
        assert_eq!(super::scaled_mean(3.0, 3), Some(1.0));
        assert_eq!(super::scaled_mean(-3.0, 3), Some(-1.0));
        assert_eq!(super::scaled_mean(1.5, 3), Some(0.5));
        // The rounding of three endpoints reaches six ulps of one, and the
        // admitted excess is mapped onto the interval end.
        assert_eq!(super::scaled_mean(3.0 + 5.0 * f64::EPSILON, 3), Some(1.0));
        // A sum beyond the band no longer states the endpoints that were read.
        assert_eq!(super::scaled_mean(3.3, 3), None);
    }

    #[test]
    fn numerical_followup_legacy_vertex_mean_stays_finite() {
        for endpoints in [[1e308, 1e308], [-1e308, 1e308]] {
            let mut vertices = Vec::new();
            let arena = cadmpeg_core::decode::DecodeArena::new();
            let policy = cadmpeg_core::decode::DecodePolicy::service();
            let (ctx, _) =
                cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
                    .expect("empty root fits service profile");
            let mut point_index = None;
            let mut index_storage = ctx.reserve_scoped(0, "Rhino legacy Brep vertex index").unwrap();
            super::legacy_vertex(&ctx, &mut vertices, &mut point_index, &mut index_storage, [endpoints[0], 0., 0.], 0).unwrap();
            for x in endpoints {
                vertices[0].add_point([x, 0., 0.]);
            }
            let vertex = vertices.pop().unwrap().into_vertex().unwrap();
            assert_eq!(
                vertex.point.get(),
                [(endpoints[0] / 2.0 + endpoints[1] / 2.0), 0., 0.]
            );
        }
    }

    use super::{
        body_kind_rests_on_missing_stamp, finite_tolerance, legacy_curve_shape,
        legacy_decoded_curve_endpoints, ordered_interval, read_children, read_edges, read_faces,
        read_legacy_mesh_sides, read_loops, read_mesh_sides, read_region_records,
        read_region_sides, read_region_topology_userdata, read_regions, read_trims, read_vertices,
        serialized_body_kind, supported_class, validate_regions, validate_rings, BrepBodyKind,
        RawBrep, RawBrepBaseType, RawBrepChild, RawBrepChildren, RawBrepEdge, RawBrepFace,
        RawBrepFaceSide, RawBrepLoop, RawBrepRegion, RawBrepTrim, RawBrepVertex, RawLoopKind,
        RawSolidFlag, RawTrimIso, RawTrimKind, ResolvedBrep, ResolvedFace, ResolvedLoop,
        ResolvedTrim, ResolvedVertex, SolidState, ValidatedRawBrep, LEGACY_BREP,
        LEGACY_TRIMMED_SURFACE, ON_BREP, ON_BREP_FACE_SIDE, ON_BREP_REGION,
        ON_UNSET_POSITIVE_VALUE, ON_UNSET_VALUE, OPENNURBS4, TL_BREP,
        V5_BREP_REGION_TOPOLOGY_USERDATA,
    };

    fn parse(
        bytes: &[u8],
        range: std::ops::Range<usize>,
        archive: ArchiveVersion,
        writer_version: Option<i64>,
        userdata: &[crate::objects::UserdataDescriptor],
    ) -> Result<super::BrepParse, GeometryError> {
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let policy = cadmpeg_core::decode::DecodePolicy::service();
        let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(bytes, &arena, &policy)
            .expect("test input fits service profile");
        super::parse(&ctx, bytes, range, archive, writer_version, userdata)
    }

    fn with_test_context<R>(
        bytes: &[u8],
        f: impl FnOnce(&cadmpeg_core::decode::DecodeContext<'_>) -> R,
    ) -> R {
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let policy = cadmpeg_core::decode::DecodePolicy::service();
        let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(bytes, &arena, &policy)
            .expect("test input fits service profile");
        f(&ctx)
    }

    fn with_collection_limit<R>(
        bytes: &[u8],
        limit: u64,
        f: impl FnOnce(&cadmpeg_core::decode::DecodeContext<'_>) -> R,
    ) -> R {
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let mut policy = cadmpeg_core::decode::DecodePolicy::service();
        policy.limits.max_collection_items = limit;
        let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(bytes, &arena, &policy)
            .expect("test input fits service profile");
        f(&ctx)
    }
    use crate::chunks::{ArchiveVersion, BoundedReader};
    use crate::curves::GeometryError;
    use crate::loss::Diagnostics;
    use crate::objects::ClassUserdata;
    use crate::settings::{BoundingBox, Interval};
    use crate::wire::Uuid;
    use cadmpeg_ir::geometry::{CurveGeometry, SolvedCurveGeometry};
    use cadmpeg_ir::units::FiniteVector;
    use std::ops::Range;

    fn finite_interval(endpoints: [f64; 2]) -> Interval {
        Interval(FiniteVector::new(endpoints).expect("finite interval"))
    }

    #[test]
    fn registered_brep_aliases_share_the_brep_payload_reader() {
        assert!(supported_class(ON_BREP));
        assert!(supported_class(LEGACY_TRIMMED_SURFACE));
        assert!(supported_class(LEGACY_BREP));
        assert!(supported_class(TL_BREP));
    }

    fn anonymous(body: &[u8]) -> Vec<u8> {
        let mut bytes = 0x4000_8000_u32.to_le_bytes().to_vec();
        bytes.extend_from_slice(&i64::try_from(body.len() + 4).expect("length").to_le_bytes());
        bytes.extend_from_slice(body);
        bytes.extend_from_slice(&crc32fast::hash(body).to_le_bytes());
        bytes
    }

    fn anonymous_mixed(parts: &[(&[u8], bool)]) -> Vec<u8> {
        let body = parts
            .iter()
            .flat_map(|(bytes, _)| bytes.iter().copied())
            .collect::<Vec<_>>();
        let mut checksum = crc32fast::Hasher::new();
        for (bytes, nested) in parts {
            if !nested {
                checksum.update(bytes);
            }
        }
        let mut bytes = 0x4000_8000_u32.to_le_bytes().to_vec();
        bytes.extend_from_slice(&i64::try_from(body.len() + 4).expect("length").to_le_bytes());
        bytes.extend_from_slice(&body);
        bytes.extend_from_slice(&checksum.finalize().to_le_bytes());
        bytes
    }

    fn packed_array(count: i32, records: &[u8]) -> Vec<u8> {
        let mut body = vec![0x10];
        body.extend_from_slice(&count.to_le_bytes());
        body.extend_from_slice(records);
        anonymous(&body)
    }

    fn region_face_side(index: i32, region: i32, face: i32, direction: i32) -> Vec<u8> {
        let mut body = 1_i32.to_le_bytes().to_vec();
        body.extend(0_i32.to_le_bytes());
        body.extend(index.to_le_bytes());
        body.extend(region.to_le_bytes());
        body.extend(face.to_le_bytes());
        body.extend(direction.to_le_bytes());
        anonymous(&body)
    }

    fn region_record(index: i32, region_type: i32, sides: &[i32], bounds: [f64; 6]) -> Vec<u8> {
        let mut body = 1_i32.to_le_bytes().to_vec();
        body.extend(0_i32.to_le_bytes());
        body.extend(index.to_le_bytes());
        body.extend(region_type.to_le_bytes());
        body.extend(
            i32::try_from(sides.len())
                .expect("side count")
                .to_le_bytes(),
        );
        body.extend(sides.iter().flat_map(|value| value.to_le_bytes()));
        body.extend(bounds.into_iter().flat_map(f64::to_le_bytes));
        anonymous(&body)
    }

    fn region_array(entries: &[u8], count: i32) -> Vec<u8> {
        let mut header = 1_i32.to_le_bytes().to_vec();
        header.extend(0_i32.to_le_bytes());
        header.extend(count.to_le_bytes());
        anonymous_mixed(&[(&header, false), (entries, true)])
    }

    fn region_topology_userdata_payload() -> Vec<u8> {
        let sides = [region_face_side(0, 0, 0, 1), region_face_side(1, 0, 0, -1)].concat();
        let side_array = region_array(&sides, 2);
        let region = region_record(0, 0, &[0, 1], [-1.0, -1.0, 0.0, 2.0, 2.0, 1.0]);
        let region_array = region_array(&region, 1);
        let mut header = 1_i32.to_le_bytes().to_vec();
        header.extend(0_i32.to_le_bytes());
        anonymous_mixed(&[(&header, false), (&side_array, true), (&region_array, true)])
    }

    fn region_topology_v6_payload() -> Vec<u8> {
        let sides = [
            class_wrapper_for(ON_BREP_FACE_SIDE, &region_face_side(0, 0, 0, 1)),
            class_wrapper_for(ON_BREP_FACE_SIDE, &region_face_side(1, 0, 0, -1)),
        ]
        .concat();
        let side_array = region_array(&sides, 2);
        let region = class_wrapper_for(
            ON_BREP_REGION,
            &region_record(0, 0, &[0, 1], [-1.0, -1.0, 0.0, 2.0, 2.0, 1.0]),
        );
        let region_array = region_array(&region, 1);
        let mut header = 1_i32.to_le_bytes().to_vec();
        header.extend(0_i32.to_le_bytes());
        anonymous_mixed(&[(&header, false), (&side_array, true), (&region_array, true)])
    }

    fn region_topology_userdata_descriptor(range: Range<usize>) -> ClassUserdata {
        ClassUserdata {
            range: range.clone(),
            version: (2, 2),
            class_uuid: V5_BREP_REGION_TOPOLOGY_USERDATA,
            item_uuid: V5_BREP_REGION_TOPOLOGY_USERDATA,
            copy_count: 1,
            transform_range: 0..0,
            application_uuid: Some(OPENNURBS4),
            save_context: None,
            payload_range: range,
        }
    }





    fn class_wrapper(data: &[u8]) -> Vec<u8> {
        class_wrapper_for(Uuid::from_canonical([9; 16]), data)
    }

    fn class_wrapper_for(class_uuid: Uuid, data: &[u8]) -> Vec<u8> {
        let mut uuid = 0x0002_fffb_u32.to_le_bytes().to_vec();
        uuid.extend_from_slice(&20_i64.to_le_bytes());
        uuid.extend(class_uuid.to_wire());
        uuid.extend_from_slice(&crc32fast::hash(&class_uuid.to_wire()).to_le_bytes());
        let mut class_data = 0x0002_fffc_u32.to_le_bytes().to_vec();
        class_data.extend_from_slice(&i64::try_from(data.len() + 4).expect("length").to_le_bytes());
        class_data.extend_from_slice(data);
        class_data.extend_from_slice(&crc32fast::hash(data).to_le_bytes());
        let mut end = 0x8002_7fff_u32.to_le_bytes().to_vec();
        end.extend_from_slice(&0_i64.to_le_bytes());
        let mut body = uuid;
        body.extend(class_data);
        body.extend(end);
        let mut wrapper = 0x0002_7ffa_u32.to_le_bytes().to_vec();
        wrapper.extend_from_slice(&i64::try_from(body.len()).expect("length").to_le_bytes());
        wrapper.extend(body);
        wrapper
    }

    fn long_chunk(typecode: u32, body: &[u8]) -> Vec<u8> {
        let mut bytes = typecode.to_le_bytes().to_vec();
        bytes.extend_from_slice(&i64::try_from(body.len() + 4).expect("length").to_le_bytes());
        bytes.extend_from_slice(body);
        bytes.extend_from_slice(&crc32fast::hash(body).to_le_bytes());
        bytes
    }

    fn mesh_class_wrapper_with_userdata() -> Vec<u8> {
        let class_uuid = crate::mesh::ON_MESH;
        let item_uuid = crate::mesh::V5_MESH_DOUBLE_VERTICES;
        let uuid = long_chunk(0x0002_fffb, &class_uuid.to_wire());
        let class_data = long_chunk(0x0002_fffc, &[]);

        let mut header_body = class_uuid.to_wire().to_vec();
        header_body.extend(item_uuid.to_wire());
        header_body.extend(1_i32.to_le_bytes());
        header_body.extend([0_u8; 16 * 8]);
        header_body.extend(Uuid::nil().to_wire());
        header_body.push(0);
        header_body.extend(50_i32.to_le_bytes());
        header_body.extend(202_400_i32.to_le_bytes());
        let header = long_chunk(0x0002_fff9, &header_body);
        let payload = anonymous(&[0]);
        let mut userdata_body = vec![0x22];
        userdata_body.extend(header);
        userdata_body.extend(payload);
        let mut userdata = 0x0002_7ffd_u32.to_le_bytes().to_vec();
        userdata.extend_from_slice(
            &i64::try_from(userdata_body.len() + 4)
                .expect("length")
                .to_le_bytes(),
        );
        userdata.extend(userdata_body);
        userdata.extend_from_slice(&crc32fast::hash(&[0x22]).to_le_bytes());

        let mut end = 0x8002_7fff_u32.to_le_bytes().to_vec();
        end.extend_from_slice(&0_i64.to_le_bytes());
        let mut body = uuid;
        body.extend(class_data);
        body.extend(userdata);
        body.extend(end);
        let mut wrapper = 0x0002_7ffa_u32.to_le_bytes().to_vec();
        wrapper.extend_from_slice(&i64::try_from(body.len()).expect("length").to_le_bytes());
        wrapper.extend(body);
        wrapper
    }

    fn interval_bytes() -> Vec<u8> {
        [0.0_f64, 1.0]
            .into_iter()
            .flat_map(f64::to_le_bytes)
            .collect()
    }

    fn trim_record(current: bool) -> Vec<u8> {
        let mut record = Vec::new();
        record.extend_from_slice(&0_i32.to_le_bytes());
        record.extend_from_slice(&0_i32.to_le_bytes());
        record.extend(interval_bytes());
        record.extend_from_slice(&0_i32.to_le_bytes());
        record.extend_from_slice(&0_i32.to_le_bytes());
        record.extend_from_slice(&1_i32.to_le_bytes());
        record.extend_from_slice(&0_i32.to_le_bytes());
        record.extend_from_slice(&1_i32.to_le_bytes());
        record.extend_from_slice(&0_i32.to_le_bytes());
        record.extend_from_slice(&0_i32.to_le_bytes());
        record.extend([0.0_f64, 0.0].into_iter().flat_map(f64::to_le_bytes));
        if current {
            record.extend(interval_bytes());
            record.push(0);
            record.extend([0; 31]);
        } else {
            record.extend([0_u8; 48]);
        }
        record.extend([0.0_f64, 0.0].into_iter().flat_map(f64::to_le_bytes));
        record
    }

    fn raw_child(base_type: RawBrepBaseType) -> RawBrepChild {
        RawBrepChild {
            class_uuid: match base_type {
                RawBrepBaseType::Curve => crate::curves::POLYCURVE,
                RawBrepBaseType::Surface => crate::surfaces::NURBS_SURFACE,
                RawBrepBaseType::Other => Uuid::nil(),
            },
            class_data_range: 0..0,
            source_range: 0..0,
        }
    }

    fn one_face_raw() -> RawBrep {
        let interval = finite_interval([0.0, 1.0]);
        let vertices = [[0, 2], [0, 1], [1, 2]]
            .into_iter()
            .enumerate()
            .map(|(index, edges)| RawBrepVertex {
                index: i32::try_from(index).expect("index"),
                point: super::CoordinateLane::Admitted(
                    crate::test_support::point3([
                        f64::from(u8::from(index == 1)),
                        f64::from(u8::from(index == 2)),
                        0.0,
                    ])
                    .0,
                ),
                edges: edges.into_iter().collect(),
                tolerance: 0.0,
                source_range: 0..0,
            })
            .collect();
        let endpoints = [[0, 1], [1, 2], [2, 0]];
        let edges = endpoints
            .into_iter()
            .enumerate()
            .map(|(index, vertices)| RawBrepEdge {
                index: i32::try_from(index).expect("index"),
                curve: 0,
                proxy_reversed: false,
                proxy_domain: interval,
                vertices,
                trims: vec![i32::try_from(index).expect("index")],
                tolerance: 0.0,
                domain: interval,
                source_range: 0..0,
            })
            .collect();
        let trims = endpoints
            .into_iter()
            .enumerate()
            .map(|(index, vertices)| RawBrepTrim {
                index: i32::try_from(index).expect("index"),
                curve: Some(0),
                proxy_domain: interval,
                edge: Some(i32::try_from(index).expect("index")),
                vertices,
                reversed_3d: false,
                trim_type: RawTrimKind::Boundary,
                iso: RawTrimIso::None,
                loop_index: 0,
                tolerances: [0.0, 0.0],
                domain: interval,
                proxy_reversed: false,
                reserved: Vec::new(),
                legacy_tolerances: [0.0, 0.0],
                source_range: 0..0,
            })
            .collect();
        RawBrep {
            losses: Vec::new(),
            minor: 0,
            c2: RawBrepChildren {
                slots: vec![Some(raw_child(RawBrepBaseType::Curve))],
                source_range: 0..0,
                expected_type: RawBrepBaseType::Curve,
            },
            c3: RawBrepChildren {
                slots: vec![Some(raw_child(RawBrepBaseType::Curve))],
                source_range: 0..0,
                expected_type: RawBrepBaseType::Curve,
            },
            surfaces: RawBrepChildren {
                slots: vec![Some(raw_child(RawBrepBaseType::Surface))],
                source_range: 0..0,
                expected_type: RawBrepBaseType::Surface,
            },
            vertices,
            edges,
            trims,
            loops: vec![RawBrepLoop {
                index: 0,
                trims: vec![0, 1, 2],
                loop_type: RawLoopKind::Outer,
                face: 0,
                source_range: 0..0,
            }],
            faces: vec![RawBrepFace {
                index: 0,
                loops: vec![0],
                surface: 0,
                reversed_surface: false,
                material_channel: 0,
                uuid: None,
                color: None,
                source_range: 0..0,
            }],
            bounds: BoundingBox {
                minimum: crate::test_support::point3([0.0, 0.0, 0.0]),
                maximum: crate::test_support::point3([1.0, 1.0, 0.0]),
            },
            render_meshes: Vec::new(),
            analysis_meshes: Vec::new(),
            is_solid: RawSolidFlag::Unstamped,
            face_sides: Vec::new(),
            regions: Vec::new(),
            source_range: 0..0,
        }
    }

    #[test]
    fn serialized_solid_state_uses_valid_values_and_topology_fallback() {
        assert_eq!(
            serialized_body_kind(
                RawSolidFlag::Known(SolidState::Closed),
                Some(200_210_020),
                false
            ),
            BrepBodyKind::Solid
        );
        assert_eq!(
            serialized_body_kind(
                RawSolidFlag::Known(SolidState::ClosedManifold),
                Some(200_210_020),
                false
            ),
            BrepBodyKind::Solid
        );
        assert_eq!(
            serialized_body_kind(RawSolidFlag::OutOfRange(3), Some(200_210_020), false),
            BrepBodyKind::Sheet
        );
        assert_eq!(
            serialized_body_kind(RawSolidFlag::OutOfRange(3), Some(200_210_020), true),
            BrepBodyKind::Solid
        );
        assert_eq!(
            serialized_body_kind(
                RawSolidFlag::Known(SolidState::Open),
                Some(200_210_020),
                true
            ),
            BrepBodyKind::Solid
        );
        assert_eq!(
            serialized_body_kind(
                RawSolidFlag::Known(SolidState::Open),
                Some(200_210_020),
                false
            ),
            BrepBodyKind::Sheet
        );
        assert_eq!(
            serialized_body_kind(RawSolidFlag::Unstamped, Some(200_210_020), true),
            BrepBodyKind::Solid
        );
        assert_eq!(
            serialized_body_kind(
                RawSolidFlag::Known(SolidState::Closed),
                Some(200_210_019),
                false
            ),
            BrepBodyKind::Sheet
        );
    }

    /// A missing stamp trusts the stored solid flag; the loss follows that reading.
    ///
    /// The same bytes classify as `Sheet` under any stamp older than the flag, so an
    /// unstamped archive that reads `Solid` was classified on an assumption it does
    /// not carry. Where the two readings agree nothing was substituted.
    #[test]
    fn body_kind_gauge_charges_only_when_a_missing_stamp_changes_the_kind() {
        assert_eq!(
            serialized_body_kind(RawSolidFlag::Known(SolidState::Closed), None, false),
            BrepBodyKind::Solid
        );
        assert!(body_kind_rests_on_missing_stamp(
            RawSolidFlag::Known(SolidState::Closed),
            None,
            false
        ));
        assert!(body_kind_rests_on_missing_stamp(
            RawSolidFlag::Known(SolidState::ClosedManifold),
            None,
            false
        ));

        // A modern stamp vouches for the same flag: the reading is verified.
        assert!(!body_kind_rests_on_missing_stamp(
            RawSolidFlag::Known(SolidState::Closed),
            Some(200_210_020),
            false
        ));
        // Both readings agree, so no kind was substituted.
        assert!(!body_kind_rests_on_missing_stamp(
            RawSolidFlag::Known(SolidState::Closed),
            None,
            true
        ));
        assert!(!body_kind_rests_on_missing_stamp(
            RawSolidFlag::Known(SolidState::Open),
            None,
            false
        ));
        assert!(!body_kind_rests_on_missing_stamp(
            RawSolidFlag::Unstamped,
            None,
            false
        ));

        // The whole-record path reports the substitution as a typed loss.
        let mut raw = one_face_raw();
        raw.minor = 2;
        raw.is_solid = RawSolidFlag::Known(SolidState::Closed);
        let validated =
            with_test_context(&[], |ctx| ValidatedRawBrep::try_new(ctx, raw)).expect("valid Brep");
        let ctx = cadmpeg_test_support::service_decode_context();
        let (kind, substituted) = validated
            .body_kind(&ctx, None)
            .expect("body-kind loss admitted");
        assert_eq!(kind, BrepBodyKind::Solid);
        assert_eq!(
            substituted.as_ref().map(|loss| &loss.code),
            Some(&crate::loss::RhinoLossCode::TopologyBodyKindGaugeSubstituted.kind())
        );
        assert_eq!(
            validated
                .body_kind(&ctx, Some(200_210_020))
                .expect("no loss")
                .1,
            None
        );
    }

    #[test]
    fn body_kind_gauge_loss_refuses_retained_limit() {
        let mut raw = one_face_raw();
        raw.minor = 2;
        raw.is_solid = RawSolidFlag::Known(SolidState::Closed);
        let validated =
            with_test_context(&[], |ctx| ValidatedRawBrep::try_new(ctx, raw)).expect("valid Brep");
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let mut policy = cadmpeg_core::decode::DecodePolicy::service();
        policy.limits.max_retained_bytes = 0;
        let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
            .expect("empty root is admitted");
        let refusal = validated
            .body_kind(&ctx, None)
            .expect_err("body-kind loss needs retained text");
        assert!(matches!(
            refusal,
            cadmpeg_core::CodecError::ResourceLimit(limit)
                if limit.operation == "Rhino Brep body-kind loss message"
        ));
    }

    fn degenerate_trim_raw(trim_type: RawTrimKind, curve: Option<i32>) -> RawBrep {
        let interval = finite_interval([0.0, 1.0]);
        RawBrep {
            losses: Vec::new(),
            minor: 0,
            c2: RawBrepChildren {
                slots: vec![Some(raw_child(RawBrepBaseType::Curve))],
                source_range: 0..0,
                expected_type: RawBrepBaseType::Curve,
            },
            c3: RawBrepChildren {
                slots: Vec::new(),
                source_range: 0..0,
                expected_type: RawBrepBaseType::Curve,
            },
            surfaces: RawBrepChildren {
                slots: vec![Some(raw_child(RawBrepBaseType::Surface))],
                source_range: 0..0,
                expected_type: RawBrepBaseType::Surface,
            },
            vertices: vec![RawBrepVertex {
                index: 0,
                point: super::CoordinateLane::Admitted(crate::test_support::point3([0.0; 3]).0),
                edges: Vec::new(),
                tolerance: 0.0,
                source_range: 0..0,
            }],
            edges: Vec::new(),
            trims: vec![RawBrepTrim {
                index: 0,
                curve,
                proxy_domain: interval,
                edge: None,
                vertices: [0, 0],
                reversed_3d: false,
                trim_type,
                iso: RawTrimIso::None,
                loop_index: 0,
                tolerances: [0.0, 0.0],
                domain: interval,
                proxy_reversed: false,
                reserved: Vec::new(),
                legacy_tolerances: [0.0, 0.0],
                source_range: 0..0,
            }],
            loops: vec![RawBrepLoop {
                index: 0,
                trims: vec![0],
                loop_type: RawLoopKind::Outer,
                face: 0,
                source_range: 0..0,
            }],
            faces: vec![RawBrepFace {
                index: 0,
                loops: vec![0],
                surface: 0,
                reversed_surface: false,
                material_channel: 0,
                uuid: None,
                color: None,
                source_range: 0..0,
            }],
            bounds: BoundingBox {
                minimum: crate::test_support::point3([0.0, 0.0, 0.0]),
                maximum: crate::test_support::point3([0.0, 0.0, 0.0]),
            },
            render_meshes: Vec::new(),
            analysis_meshes: Vec::new(),
            is_solid: RawSolidFlag::Unstamped,
            face_sides: Vec::new(),
            regions: Vec::new(),
            source_range: 0..0,
        }
    }

    #[test]
    fn legacy_brep_major_two_requires_its_payload() {
        let error = parse(&[0x20], 0..1, ArchiveVersion::V5, None, &[])
            .expect_err("truncated major two must fail");
        assert!(matches!(error, GeometryError::Malformed(_)));
    }

    #[test]
    fn legacy_endpoint_paths_visit_single_child_chains_once_per_end() {
        let mut curve = crate::curves::DecodedCurve::leaf(
            CurveGeometry::Solved(SolvedCurveGeometry::Degenerate(
                cadmpeg_ir::geometry::analytic::DegenerateCurve::try_new(cadmpeg_ir::math::Point3::new(4., 5., 6.)).unwrap(),
            )), Diagnostics::new(),
        );
        for _ in 0..32 {
            curve = crate::curves::DecodedCurve::Compound {
                children: vec![(cadmpeg_ir::scalar::FiniteReal::ZERO, curve)],
                end_parameter: cadmpeg_ir::scalar::FiniteReal::ONE,
                warnings: Diagnostics::new(),
            };
        }
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let mut policy = cadmpeg_core::decode::DecodePolicy::service();
        // Each end visits 32 compound nodes and one leaf.
        policy.limits.max_work_units = 66;
        let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        assert_eq!(legacy_decoded_curve_endpoints(&ctx, &curve, 0).unwrap(), [[4., 5., 6.]; 2]);
        cadmpeg_test_support::refusal::resource_limit_at(
            cadmpeg_core::decode::ResourceDimension::WorkUnits, "Rhino legacy Brep endpoint path", |cap| {
                let mut policy = cadmpeg_core::decode::DecodePolicy::service();
                policy.limits.max_work_units = cap;
                let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
                legacy_decoded_curve_endpoints(&ctx, &curve, 0).map_err(|error| match error {
                    GeometryError::Codec(error) => error, error => panic!("unexpected endpoint failure: {error:?}"),
                })
            },
        );
    }

    #[test]
    fn legacy_vertex_index_preserves_the_first_signed_zero_match() {
        let ctx = cadmpeg_test_support::service_decode_context();
        let mut vertices = Vec::new();
        let mut point_index = None;
        let mut storage = ctx.reserve_scoped(0, "Rhino legacy Brep vertex index").unwrap();
        assert_eq!(super::legacy_vertex(&ctx, &mut vertices, &mut point_index, &mut storage, [-0., 2., 3.], 0).unwrap(), 0);
        assert_eq!(super::legacy_vertex(&ctx, &mut vertices, &mut point_index, &mut storage, [0., 2., 3.], 0).unwrap(), 0);
        assert_eq!(vertices.len(), 1);
        assert_eq!(super::legacy_vertex(&ctx, &mut vertices, &mut point_index, &mut storage, [1., 2., 3.], 0).unwrap(), 1);
    }

    #[test]
    fn legacy_curve_endpoints_cover_analytic_and_degenerate_children() {
        let circle = crate::curves::DecodedCurve::leaf(
            CurveGeometry::Solved(SolvedCurveGeometry::Circle(
                cadmpeg_ir::geometry::analytic::CircleCurve::try_new(
                    cadmpeg_ir::math::Point3::new(1.0, 2.0, 3.0),
                    cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0),
                    cadmpeg_ir::math::Vector3::new(1.0, 0.0, 0.0),
                    2.0,
                )
                .unwrap(),
            )),
            Diagnostics::new(),
        );
        assert_eq!(
            legacy_decoded_curve_endpoints(&cadmpeg_test_support::service_decode_context(), &circle, 0).expect("circle endpoints"),
            [[3.0, 2.0, 3.0]; 2]
        );
        let point = cadmpeg_ir::math::Point3::new(4.0, 5.0, 6.0);
        let degenerate = crate::curves::DecodedCurve::leaf(
            CurveGeometry::Solved(SolvedCurveGeometry::Degenerate(
                cadmpeg_ir::geometry::analytic::DegenerateCurve::try_new(point).unwrap(),
            )),
            Diagnostics::new(),
        );
        assert_eq!(
            legacy_decoded_curve_endpoints(&cadmpeg_test_support::service_decode_context(), &degenerate, 0).expect("degenerate endpoints"),
            [[4.0, 5.0, 6.0]; 2]
        );
    }

    #[test]
    fn a_legacy_polycurve_domain_spans_its_admitted_parameters() {
        let point = |x: f64| {
            crate::curves::DecodedCurve::leaf(
                CurveGeometry::Solved(SolvedCurveGeometry::Degenerate(
                    cadmpeg_ir::geometry::analytic::DegenerateCurve::try_new(
                        cadmpeg_ir::math::Point3::new(x, 0.0, 0.0),
                    )
                    .unwrap(),
                )),
                Diagnostics::new(),
            )
        };
        let finite = |value: f64| cadmpeg_ir::scalar::FiniteReal::new(value).unwrap();
        let polycurve = crate::curves::DecodedCurve::Compound {
            children: vec![(finite(-1.5), point(1.0)), (finite(2.0), point(2.0))],
            end_parameter: finite(6.25),
            warnings: Diagnostics::new(),
        };
        let polycurve = crate::curves::DecodedGeometry::Curve { curve: polycurve };
        let (domain, endpoints) = legacy_curve_shape(&cadmpeg_test_support::service_decode_context(), &polycurve, 0).expect("polycurve shape");
        assert_eq!(domain, finite_interval([-1.5, 6.25]));
        assert_eq!(endpoints, [[1.0, 0.0, 0.0], [2.0, 0.0, 0.0]]);
    }

    #[test]
    fn negative_array_count_is_rejected_before_allocation() {
        let mut bytes = vec![0x30, 0x10];
        bytes.extend_from_slice(&(-1_i32).to_le_bytes());
        let error = parse(&bytes, 0..bytes.len(), ArchiveVersion::V5, None, &[])
            .expect_err("negative C2 count must fail");
        assert!(matches!(error, GeometryError::Malformed(_)));
    }

    #[test]
    fn raw_arrays_consume_complete_anonymous_wrappers() {
        let bytes = packed_array(0, &[]);
        let mut reader = BoundedReader::new(&bytes, 0, bytes.len()).expect("reader");
        let (_, range) = with_test_context(&bytes, |ctx| {
            read_vertices(
                ctx,
                &bytes,
                &mut reader,
                ArchiveVersion::V5,
                &mut Diagnostics::new(),
            )
        })
        .expect("vertex");
        assert_eq!(range, 0..bytes.len());
        assert_eq!(reader.remaining(), 0);
    }

    #[test]
    fn brep_child_ranges_refuse_collection_limit() {
        let mut body = vec![0x10];
        body.extend(1_i32.to_le_bytes());
        body.extend(0_i32.to_le_bytes());
        let bytes = anonymous(&body);
        let mut reader = BoundedReader::new(&bytes, 0, bytes.len()).expect("reader");
        let error = with_collection_limit(&bytes, 1, |ctx| {
            read_children(
                ctx,
                &bytes,
                &mut reader,
                ArchiveVersion::V5,
                RawBrepBaseType::Curve,
                &mut Diagnostics::new(),
            )
        })
        .expect_err("two child ranges exceed one collection item");
        assert!(
            matches!(error, GeometryError::Codec(cadmpeg_core::CodecError::ResourceLimit(limit))
            if limit.operation == "Rhino Brep child ranges")
        );
    }

    #[test]
    fn brep_child_slots_refuse_collection_limit() {
        let mut body = vec![0x10];
        body.extend(1_i32.to_le_bytes());
        body.extend(0_i32.to_le_bytes());
        let bytes = anonymous(&body);
        let mut reader = BoundedReader::new(&bytes, 0, bytes.len()).expect("reader");
        let error = with_collection_limit(&bytes, 2, |ctx| {
            read_children(
                ctx,
                &bytes,
                &mut reader,
                ArchiveVersion::V5,
                RawBrepBaseType::Curve,
                &mut Diagnostics::new(),
            )
        })
        .expect_err("one child slot exceeds two charged ranges");
        assert!(
            matches!(error, GeometryError::Codec(cadmpeg_core::CodecError::ResourceLimit(limit))
            if limit.operation == "Rhino Brep child slots")
        );
        let mut reader = BoundedReader::new(&bytes, 0, bytes.len()).expect("reader");
        assert!(with_test_context(&bytes, |ctx| read_children(
            ctx,
            &bytes,
            &mut reader,
            ArchiveVersion::V5,
            RawBrepBaseType::Curve,
            &mut Diagnostics::new()
        ))
        .is_ok());
    }

    fn one_vertex_array() -> Vec<u8> {
        let mut record = 0_i32.to_le_bytes().to_vec();
        record.extend([0.0_f64; 3].into_iter().flat_map(f64::to_le_bytes));
        record.extend(1_i32.to_le_bytes());
        record.extend(0_i32.to_le_bytes());
        record.extend(0.0_f64.to_le_bytes());
        packed_array(1, &record)
    }

    #[test]
    fn brep_vertices_refuse_collection_limit() {
        let bytes = one_vertex_array();
        let mut reader = BoundedReader::new(&bytes, 0, bytes.len()).expect("reader");
        let error = with_collection_limit(&bytes, 0, |ctx| {
            read_vertices(
                ctx,
                &bytes,
                &mut reader,
                ArchiveVersion::V5,
                &mut Diagnostics::new(),
            )
        })
        .expect_err("one vertex exceeds zero collection items");
        assert!(
            matches!(error, GeometryError::Codec(cadmpeg_core::CodecError::ResourceLimit(limit))
            if limit.operation == "Rhino Brep vertices")
        );
    }

    #[test]
    fn brep_indexes_refuse_collection_limit() {
        let bytes = one_vertex_array();
        let mut reader = BoundedReader::new(&bytes, 0, bytes.len()).expect("reader");
        let error = with_collection_limit(&bytes, 1, |ctx| {
            read_vertices(
                ctx,
                &bytes,
                &mut reader,
                ArchiveVersion::V5,
                &mut Diagnostics::new(),
            )
        })
        .expect_err("one edge index exceeds one vertex item");
        assert!(
            matches!(error, GeometryError::Codec(cadmpeg_core::CodecError::ResourceLimit(limit))
            if limit.operation == "Rhino Brep indexes")
        );
        let mut reader = BoundedReader::new(&bytes, 0, bytes.len()).expect("reader");
        assert!(with_test_context(&bytes, |ctx| read_vertices(
            ctx,
            &bytes,
            &mut reader,
            ArchiveVersion::V5,
            &mut Diagnostics::new()
        ))
        .is_ok());
    }

    #[test]
    fn brep_edges_refuse_collection_limit() {
        let mut record = 0_i32.to_le_bytes().to_vec();
        record.extend(0_i32.to_le_bytes());
        record.extend(0_i32.to_le_bytes());
        record.extend([0.0_f64, 1.0].into_iter().flat_map(f64::to_le_bytes));
        record.extend([0_i32; 2].into_iter().flat_map(i32::to_le_bytes));
        record.extend(0_i32.to_le_bytes());
        record.extend(0.0_f64.to_le_bytes());
        let bytes = packed_array(1, &record);
        for (limit, operation) in [
            (0, "Rhino Brep unstamped layout losses"),
            (1, "Rhino Brep edges"),
        ] {
            let mut reader = BoundedReader::new(&bytes, 0, bytes.len()).expect("reader");
            let error = with_collection_limit(&bytes, limit, |ctx| {
                read_edges(
                    ctx,
                    &bytes,
                    &mut reader,
                    ArchiveVersion::V5,
                    None,
                    &mut Diagnostics::new(),
                    &mut Vec::new(),
                )
            })
            .expect_err("one edge and its layout loss exceed the collection limit");
            assert!(
                matches!(error, GeometryError::Codec(cadmpeg_core::CodecError::ResourceLimit(refusal))
                if refusal.operation == operation),
                "limit {limit} must refuse at {operation}"
            );
        }
    }

    #[test]
    fn brep_trims_refuse_collection_limit() {
        let bytes = packed_array(1, &trim_record(true));
        let mut reader = BoundedReader::new(&bytes, 0, bytes.len()).expect("reader");
        let error = with_collection_limit(&bytes, 0, |ctx| {
            read_trims(
                ctx,
                &bytes,
                &mut reader,
                ArchiveVersion::V5,
                Some(200_206_180),
                &mut Diagnostics::new(),
                &mut Vec::new(),
            )
        })
        .expect_err("one trim exceeds zero collection items");
        assert!(
            matches!(error, GeometryError::Codec(cadmpeg_core::CodecError::ResourceLimit(limit))
            if limit.operation == "Rhino Brep trims")
        );
    }

    #[test]
    fn brep_trim_reserved_bytes_refuse_retained_limit() {
        let bytes = packed_array(1, &trim_record(true));
        let run = |cap| {
            let arena = cadmpeg_core::decode::DecodeArena::new();
            let mut policy = cadmpeg_core::decode::DecodePolicy::service();
            policy.limits.max_retained_bytes = cap;
            let (ctx, _) =
                cadmpeg_core::decode::DecodeContext::from_root_bytes(&bytes, &arena, &policy)
                    .expect("test input fits service profile");
            let mut reader = BoundedReader::new(&bytes, 0, bytes.len()).expect("reader");
            read_trims(
                &ctx,
                &bytes,
                &mut reader,
                ArchiveVersion::V5,
                Some(200_206_180),
                &mut Diagnostics::new(),
                &mut Vec::new(),
            )
            .expect_err("31 reserved bytes exceed a 30-byte retained limit")
        };
        let error = run(crate::test_support::retained_limit_at(
            "Rhino Brep trim reserved bytes",
            0,
            |cap| match run(cap) {
                GeometryError::Codec(cadmpeg_core::CodecError::ResourceLimit(limit)) => limit,
                error => panic!("unexpected resource refusal: {error:?}"),
            },
        ));
        assert!(
            matches!(error, GeometryError::Codec(cadmpeg_core::CodecError::ResourceLimit(limit))
            if limit.operation == "Rhino Brep trim reserved bytes")
        );
    }

    #[test]
    fn brep_loops_refuse_collection_limit() {
        let mut record = 0_i32.to_le_bytes().to_vec();
        record.extend(1_i32.to_le_bytes());
        record.extend(0_i32.to_le_bytes());
        record.extend(1_i32.to_le_bytes());
        record.extend(0_i32.to_le_bytes());
        let bytes = packed_array(1, &record);
        let mut reader = BoundedReader::new(&bytes, 0, bytes.len()).expect("reader");
        let error = with_collection_limit(&bytes, 0, |ctx| {
            read_loops(
                ctx,
                &bytes,
                &mut reader,
                ArchiveVersion::V5,
                &mut Diagnostics::new(),
            )
        })
        .expect_err("one loop exceeds zero collection items");
        assert!(
            matches!(error, GeometryError::Codec(cadmpeg_core::CodecError::ResourceLimit(limit))
            if limit.operation == "Rhino Brep loops")
        );
    }

    #[test]
    fn brep_faces_refuse_collection_limit() {
        let mut record = 0_i32.to_le_bytes().to_vec();
        record.extend(0_i32.to_le_bytes());
        record.extend(0_i32.to_le_bytes());
        record.extend(0_i32.to_le_bytes());
        record.extend(0_i32.to_le_bytes());
        let bytes = packed_array(1, &record);
        let mut reader = BoundedReader::new(&bytes, 0, bytes.len()).expect("reader");
        let error = with_collection_limit(&bytes, 0, |ctx| {
            read_faces(
                ctx,
                &bytes,
                &mut reader,
                ArchiveVersion::V5,
                &mut Diagnostics::new(),
            )
        })
        .expect_err("one face exceeds zero collection items");
        assert!(
            matches!(error, GeometryError::Codec(cadmpeg_core::CodecError::ResourceLimit(limit))
            if limit.operation == "Rhino Brep faces")
        );
    }

    #[test]
    fn raw_array_crc_mismatch_warns_and_consumes_wrapper() {
        let mut bytes = packed_array(0, &[]);
        let crc = bytes.len() - 1;
        bytes[crc] ^= 1;
        let mut reader = BoundedReader::new(&bytes, 0, bytes.len()).expect("reader");
        let mut warnings = Diagnostics::new();
        with_test_context(&bytes, |ctx| {
            read_vertices(ctx, &bytes, &mut reader, ArchiveVersion::V5, &mut warnings)
        })
        .expect("recoverable vertex wrapper");
        assert_eq!(reader.remaining(), 0);
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains("Brep anonymous CRC mismatch"));
    }

    #[test]
    fn raw_array_crc_mismatch_refuses_diagnostic_collection_limit() {
        let mut bytes = packed_array(0, &[]);
        let crc = bytes.len() - 1;
        bytes[crc] ^= 1;
        let mut reader = BoundedReader::new(&bytes, 0, bytes.len()).expect("reader");
        let error = with_collection_limit(&bytes, 0, |ctx| {
            read_vertices(
                ctx,
                &bytes,
                &mut reader,
                ArchiveVersion::V5,
                &mut Diagnostics::new(),
            )
        })
        .expect_err("checksum warning requires one diagnostic slot");
        assert!(matches!(
            error,
            GeometryError::Codec(cadmpeg_core::CodecError::ResourceLimit(limit))
                if limit.operation == "Rhino diagnostics"
        ));
    }

    #[test]
    fn unstamped_layout_loss_refuses_collection_limit() {
        let mut losses = Vec::new();
        let error = with_collection_limit(&[], 0, |ctx| {
            super::unstamped_legacy_layout(
                ctx,
                ArchiveVersion::V5,
                None,
                1,
                "edge domains",
                &mut losses,
            )
        })
        .expect_err("unstamped layout loss requires one slot");
        assert!(matches!(
            error,
            GeometryError::Codec(cadmpeg_core::CodecError::ResourceLimit(limit))
                if limit.operation == "Rhino Brep unstamped layout losses"
        ));
        assert!(losses.is_empty());
        with_test_context(&[], |ctx| {
            super::unstamped_legacy_layout(
                ctx,
                ArchiveVersion::V5,
                None,
                1,
                "edge domains",
                &mut losses,
            )
        })
        .expect("service profile admits the loss");
        assert_eq!(losses.len(), 1);
    }

    #[test]
    fn face_reader_accepts_all_packed_minors() {
        for version in [0x10_u8, 0x11, 0x12] {
            let mut body = vec![version, 0, 0, 0, 0];
            if version == 0x12 {
                body.push(0);
            }
            let bytes = anonymous(&body);
            let mut reader = BoundedReader::new(&bytes, 0, bytes.len()).expect("reader");
            let (faces, _) = with_test_context(&bytes, |ctx| {
                read_faces(
                    ctx,
                    &bytes,
                    &mut reader,
                    ArchiveVersion::V5,
                    &mut Diagnostics::new(),
                )
            })
            .expect("faces");
            assert!(faces.is_empty());
        }
    }

    #[test]
    fn trim_gate_preserves_legacy_tail_and_wrapper_range() {
        for writer in [200_000_000_i64, 200_206_180] {
            let record = trim_record(writer >= 200_206_180);
            assert_eq!(record.len(), 132);
            let bytes = packed_array(1, &record);
            let mut reader = BoundedReader::new(&bytes, 0, bytes.len()).expect("reader");
            let (trims, range) = with_test_context(&bytes, |ctx| {
                read_trims(
                    ctx,
                    &bytes,
                    &mut reader,
                    ArchiveVersion::V5,
                    Some(writer),
                    &mut Diagnostics::new(),
                    &mut Vec::new(),
                )
            })
            .expect("trims");
            assert_eq!(range, 0..bytes.len());
            assert_eq!(trims[0].legacy_tolerances, [0.0, 0.0]);
        }
    }

    #[test]
    fn tolerance_accepts_explicit_signed_unset_values() {
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let policy = cadmpeg_core::decode::DecodePolicy::service();
        let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
            .expect("test context");
        assert!(finite_tolerance(&ctx, ON_UNSET_VALUE, "tolerance").is_ok());
        assert!(finite_tolerance(&ctx, ON_UNSET_POSITIVE_VALUE, "tolerance").is_ok());
        assert!(finite_tolerance(&ctx, -1.0, "tolerance").is_err());
    }

    /// Both validators judge an already-decoded record field, which no byte of
    /// the file locates, so their refusals name no offset instead of byte 0.
    #[test]
    fn an_interval_or_tolerance_refusal_names_no_byte() {
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let policy = cadmpeg_core::decode::DecodePolicy::service();
        let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
            .expect("test context");
        let error =
            ordered_interval(&ctx, finite_interval([0.0, 0.0]), "interval").expect_err("ordering");
        assert!(matches!(
            error,
            GeometryError::Malformed(crate::chunks::FramingError::Unpositioned { ref message })
                if message == "interval is invalid"
        ));
        assert_eq!(error.to_string(), "framing error: interval is invalid");
        let error = finite_tolerance(&ctx, -1.0, "tolerance").expect_err("sign");
        assert!(matches!(
            error,
            GeometryError::Malformed(crate::chunks::FramingError::Unpositioned { ref message })
                if message == "tolerance is invalid"
        ));
    }

    #[test]
    fn interval_accepts_explicit_signed_unset_values() {
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let policy = cadmpeg_core::decode::DecodePolicy::service();
        let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
            .expect("test context");
        for value in [
            finite_interval([ON_UNSET_VALUE, ON_UNSET_VALUE]),
            finite_interval([ON_UNSET_POSITIVE_VALUE, ON_UNSET_POSITIVE_VALUE]),
            finite_interval([ON_UNSET_VALUE, ON_UNSET_POSITIVE_VALUE]),
            finite_interval([ON_UNSET_POSITIVE_VALUE, ON_UNSET_VALUE]),
        ] {
            assert!(ordered_interval(&ctx, value, "interval").is_ok());
        }
        assert!(ordered_interval(&ctx, finite_interval([0.0, 0.0]), "interval").is_err());
    }

    /// The one-trim fixture resolved the way validation resolves it.
    fn degenerate_trim_resolved(curve: Option<usize>) -> ResolvedBrep {
        ResolvedBrep {
            vertices: vec![ResolvedVertex {
                edges: Vec::new(),
                tolerance: super::BrepTolerance::new(0.0).expect("valid source tolerance"),
            }],
            edges: Vec::new(),
            trims: vec![ResolvedTrim {
                curve,
                edge: None,
                vertices: [0, 0],
                loop_index: 0,
                tolerances: [super::BrepTolerance::new(0.0).expect("valid source tolerance"); 2],
            }],
            loops: vec![ResolvedLoop {
                trims: vec![0],
                face: 0,
            }],
            faces: vec![ResolvedFace {
                surface: 0,
                loops: vec![0],
            }],
            face_sides: Vec::new(),
        }
    }

    #[test]
    fn procedural_loops_use_one_matching_trim_without_ring_closure() {
        with_test_context(&[], |ctx| {
            let mut curve_loop = degenerate_trim_raw(RawTrimKind::CurveOnSurface, Some(0));
            curve_loop.loops[0].loop_type = RawLoopKind::CurveOnSurface;
            assert!(validate_rings(ctx, &curve_loop, &degenerate_trim_resolved(Some(0))).is_ok());

            let mut point_loop = degenerate_trim_raw(RawTrimKind::PointOnSurface, None);
            point_loop.loops[0].loop_type = RawLoopKind::PointOnSurface;
            assert!(validate_rings(ctx, &point_loop, &degenerate_trim_resolved(None)).is_ok());

            let mut mismatched = degenerate_trim_raw(RawTrimKind::PointOnSurface, None);
            mismatched.loops[0].loop_type = RawLoopKind::CurveOnSurface;
            assert!(validate_rings(ctx, &mismatched, &degenerate_trim_resolved(None)).is_err());
        });
    }





















    #[test]
    fn polymorphic_array_preserves_null_and_classifies_wrong_base() {
        let mut body = vec![0x10];
        body.extend_from_slice(&2_i32.to_le_bytes());
        body.extend_from_slice(&0_i32.to_le_bytes());
        body.extend_from_slice(&1_i32.to_le_bytes());
        body.extend(class_wrapper(&[]));
        let bytes = anonymous(&body);
        let mut reader = BoundedReader::new(&bytes, 0, bytes.len()).expect("reader");
        let array = with_test_context(&bytes, |ctx| {
            read_children(
                ctx,
                &bytes,
                &mut reader,
                ArchiveVersion::V5,
                RawBrepBaseType::Curve,
                &mut Diagnostics::new(),
            )
        })
        .expect("children");
        assert!(array.slots[0].is_none());
        assert_eq!(
            array.slots[1].as_ref().expect("wrong class").base_type(),
            RawBrepBaseType::Other
        );
        assert_eq!(reader.remaining(), 0);
    }













    #[test]
    fn valid_one_face_raw_brep_validates_all_reciprocal_links() {
        assert!(
            with_test_context(&[], |ctx| ValidatedRawBrep::try_new(ctx, one_face_raw())).is_ok()
        );
    }

    fn assert_resolved_brep_limit(limit: u64, operation: &str) {
        let error = with_collection_limit(&[], limit, |ctx| {
            ValidatedRawBrep::try_new(ctx, one_face_raw())
        })
        .expect_err("resolved Brep collections exceed the configured limit");
        assert!(
            matches!(
                &error,
                GeometryError::Codec(cadmpeg_core::CodecError::ResourceLimit(refusal))
                    if refusal.operation == operation
            ),
            "unexpected validation refusal: {error}"
        );
    }

    #[test]
    fn resolved_brep_vertices_refuse_collection_limit() {
        assert_resolved_brep_limit(2, "Rhino resolved Brep vertices");
    }

    #[test]
    fn resolved_brep_edges_refuse_collection_limit() {
        assert_resolved_brep_limit(5, "Rhino resolved Brep edges");
    }

    #[test]
    fn resolved_brep_trims_refuse_collection_limit() {
        assert_resolved_brep_limit(8, "Rhino resolved Brep trims");
    }

    #[test]
    fn resolved_brep_loops_refuse_collection_limit() {
        assert_resolved_brep_limit(9, "Rhino resolved Brep loops");
    }

    #[test]
    fn resolved_brep_faces_refuse_collection_limit() {
        assert_resolved_brep_limit(10, "Rhino resolved Brep faces");
    }

    #[test]
    fn resolved_brep_references_refuse_collection_limit() {
        assert_resolved_brep_limit(11, "Rhino resolved Brep references");
    }

    #[test]
    fn brep_unique_references_refuse_collection_limit() {
        assert_resolved_brep_limit(18, "Rhino Brep unique references");
    }

    #[test]
    fn positional_indexes_are_diagnostic_and_array_order_remains_authoritative() {
        let mut raw = one_face_raw();
        raw.vertices[0].index = 9;
        raw.edges[1].index = 9;
        raw.trims[2].index = 9;
        raw.loops[0].index = 9;
        raw.faces[0].index = 9;
        let validated = with_test_context(&[], |ctx| ValidatedRawBrep::try_new(ctx, raw))
            .expect("positional indexes are redundant");
        assert_eq!(validated.warnings().len(), 5);
    }

    #[test]
    fn positional_index_diagnostic_refuses_collection_limit() {
        let mut raw = one_face_raw();
        raw.vertices[0].index = 9;
        let error = with_collection_limit(&[], 0, |ctx| ValidatedRawBrep::try_new(ctx, raw))
            .expect_err("positional mismatch requires one diagnostic slot");
        assert!(matches!(
            error,
            GeometryError::Codec(cadmpeg_core::CodecError::ResourceLimit(limit))
                if limit.operation == "Rhino diagnostics"
        ));
    }

    #[test]
    fn singular_trim_accepts_c2_without_a_real_edge() {
        assert!(with_test_context(&[], |ctx| ValidatedRawBrep::try_new(
            ctx,
            degenerate_trim_raw(RawTrimKind::Singular, Some(0))
        ))
        .is_ok());
    }

    #[test]
    fn point_on_surface_trim_accepts_no_c2_or_real_edge() {
        assert!(with_test_context(&[], |ctx| ValidatedRawBrep::try_new(
            ctx,
            degenerate_trim_raw(RawTrimKind::PointOnSurface, None)
        ))
        .is_ok());
    }

    #[test]
    fn point_on_surface_trim_rejects_an_attributed_c2() {
        assert!(with_test_context(&[], |ctx| ValidatedRawBrep::try_new(
            ctx,
            degenerate_trim_raw(RawTrimKind::PointOnSurface, Some(0))
        ))
        .is_err());
    }









    fn incidence_star(count: usize) -> (RawBrep, ResolvedBrep) {
        let mut raw = one_face_raw();
        raw.edges = (0..count).map(|_| raw.edges[0].clone()).collect();
        let tolerance = crate::brep::BrepTolerance::Unset;
        let mut vertices = vec![ResolvedVertex { edges: (0..count).collect(), tolerance }];
        vertices.extend((0..count).map(|edge| ResolvedVertex { edges: vec![edge], tolerance }));
        let edges = (0..count).map(|edge| crate::brep::ResolvedEdge {
            curve: 0, vertices: [0, edge + 1], trims: Vec::new(), tolerance,
        }).collect();
        (raw, ResolvedBrep { vertices, edges, ..ResolvedBrep::default() })
    }

    #[test]
    fn vertex_incidence_counts_visit_a_star_once() {
        let (raw, resolved) = incidence_star(32);
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let mut policy = cadmpeg_core::decode::DecodePolicy::service();
        // 32 count initializations + 33 vertices + 64 references + 32 edges.
        policy.limits.max_work_units = 32 + 33 + 64 + 32;
        let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        crate::brep::validate_edge_incidences(&ctx, &raw, &resolved).expect("linear incidence budget");
        ctx.finish_session().unwrap();
        with_test_context(&[], |ctx| {
            let mut missing = resolved.clone();
            missing.vertices[0].edges.pop();
            assert!(crate::brep::validate_edge_incidences(ctx, &raw, &missing).is_err());
            let mut duplicated = resolved.clone();
            duplicated.vertices[0].edges.push(0);
            assert!(crate::brep::validate_edge_incidences(ctx, &raw, &duplicated).is_err());
            let mut closed = resolved.clone();
            closed.edges[0].vertices = [0, 0];
            closed.vertices[0].edges.push(0);
            crate::brep::validate_edge_incidences(ctx, &raw, &closed).expect("closed endpoint counted twice");
        });
    }

    #[test]
    fn endpoint_incidence_counts_refuse_before_building() {
        use cadmpeg_core::decode::{refusal_probe::RefusalProbe, ResourceDimension};
        use cadmpeg_core::CodecError;
        let (raw, resolved) = incidence_star(32);
        for dimension in [ResourceDimension::MaterializedBytes, ResourceDimension::CollectionItems] {
            cadmpeg_test_support::refusal::resource_limit_at(dimension, "Rhino Brep endpoint incidence counts", |limit| {
                let arena = cadmpeg_core::decode::DecodeArena::new();
                let mut policy = cadmpeg_core::decode::DecodePolicy::service();
                match dimension {
                    ResourceDimension::MaterializedBytes => policy.limits.max_materialized_bytes = limit,
                    ResourceDimension::CollectionItems => policy.limits.max_collection_items = limit,
                    _ => unreachable!(),
                }
                let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
                let result = crate::brep::validate_edge_incidences(&ctx, &raw, &resolved);
                assert!(matches!(ctx.charge_work(0, "after incidence refusal"), Err(CodecError::ResourceLimit(_))));
                result.map_err(|error| match error { GeometryError::Codec(error) => error, other => CodecError::malformed(other) })
            });
        }
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let policy = cadmpeg_core::decode::DecodePolicy::service();
        let _probe = RefusalProbe::arm(ResourceDimension::RetainedBytes, "Rhino Brep endpoint incidence counts", None);
        let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        crate::brep::validate_edge_incidences(&ctx, &raw, &resolved).expect("counts are scratch");
        ctx.finish_session().unwrap();
    }

    mod regions;
    mod meshes;
