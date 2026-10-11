// SPDX-License-Identifier: Apache-2.0

use super::*;
use cadmpeg_core::decode::{DecodeArena, DecodePolicy, ResourceDimension};

#[test]
fn pcurve_diagnostics_keep_storage_after_the_identity_map_drops() {
    let (data, raw) = source_shaped_plane_brep();
    let brep = with_expand_bytes(&data, |expand| {
        crate::brep::ValidatedRawBrep::try_new(expand.ctx(), raw)
    })
    .unwrap();
    let mut costs = [0; 3];
    for (index, (backing_only, release)) in [(false, false), (false, true), (true, true)]
        .into_iter()
        .enumerate()
    {
        let error = cadmpeg_test_support::refusal::resource_limit_at(
            ResourceDimension::MaterializedBytes,
            "pcurve diagnostic lifetime checkpoint",
            |cap| {
                let arena = DecodeArena::new();
                let mut policy = DecodePolicy::service();
                policy.limits.max_materialized_bytes = cap;
                let (ctx, _) =
                    cadmpeg_core::decode::DecodeContext::from_root_bytes(&data, &arena, &policy)?;
                let mut backing = ctx.reserve_scoped(0, "pcurve fixture backing")?;
                let mut metadata = ctx.reserve_scoped(0, "pcurve fixture metadata")?;
                let super::super::DecodedPcurves {
                    ids,
                    values,
                    warnings,
                    _storage: id_storage,
                } = super::super::decode_pcurves(
                    (&ctx, &mut backing),
                    &data,
                    ArchiveVersion::V5,
                    brep.raw(),
                    brep.resolved(),
                    "invalid key",
                    &std::collections::HashMap::new(),
                )
                .map_err(|error| match error {
                    crate::curves::GeometryError::Codec(error) => error,
                    other => panic!("unexpected pcurve error: {other}"),
                })?;
                assert!(ids.is_empty());
                assert!(values.is_empty());
                let mut destination = Diagnostics::new();
                metadata.with_storage(|| {
                    destination.append_scoped_admitted(
                        &ctx,
                        warnings,
                        "Rhino Brep pcurve diagnostic promotion",
                    )
                })?;
                drop((ids, id_storage));
                assert_eq!(destination.len(), 1);
                assert!(destination[0]
                    .message
                    .starts_with("Brep pcurve identity key is invalid:"));
                if release {
                    drop(destination);
                    drop(metadata);
                }
                // Reproduce the same vector growth without retaining a message.
                let mut control_storage = ctx.reserve_scoped(0, "diagnostic backing control")?;
                let mut control = Diagnostics::new();
                if backing_only {
                    control_storage.with_storage(|| {
                        control.push_coded_admitted(&ctx, None, format_args!(""))
                    })?;
                }
                let held = ctx.reserve_scoped(1 << 20, "pcurve diagnostic lifetime checkpoint")?;
                drop(held);
                Ok(())
            },
        );
        let cadmpeg_core::CodecError::ResourceLimit(limit) = error else {
            panic!("checkpoint refusal");
        };
        costs[index] = limit.used;
        if release && !backing_only {
            assert_eq!(limit.used, 0, "consumed diagnostics release all scratch");
        } else if !release {
            assert!(limit.used > 0, "live diagnostics retain their reservation");
        }
    }
    assert!(
        costs[0] > costs[2],
        "the live diagnostic message remains charged beyond vector backing"
    );
}

#[test]
fn failed_brep_polycurve_slot_releases_its_parsed_prefix() {
    use crate::test_support::test_archive as bytes;
    for count in [4_u32, 128] {
        let points = (0..count)
            .map(|i| [f64::from(i), 0.0, 0.0])
            .collect::<Vec<_>>();
        let parameters = (0..count).map(f64::from).collect::<Vec<_>>();
        let data = bytes::polycurve_payload(
            &[0.0, 1.0, 2.0],
            &[
                (
                    bytes::POLYLINE_CLASS,
                    bytes::polyline_payload(&points, &parameters),
                ),
                (
                    POINT_CLASS,
                    crate::test_support::test_dump::point_payload([0.0; 3]),
                ),
            ],
        );
        let (_, mut raw) = source_shaped_plane_brep();
        raw.render_meshes.clear();
        raw.analysis_meshes.clear();
        raw.surfaces.slots.clear();
        raw.c3.slots = vec![Some(crate::brep::RawBrepChild {
            class_uuid: crate::curves::POLYCURVE,
            class_data_range: 0..data.len(),
            source_range: 0..data.len(),
        })];
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = 0;
        policy.limits.max_materialized_bytes = 1 << 20;
        let (ctx, root) =
            cadmpeg_core::decode::DecodeContext::from_root_bytes(&data, &arena, &policy).unwrap();
        let mut backing = ctx.reserve_scoped(0, "failed slot backing").unwrap();
        let mut metadata = ctx.reserve_scoped(0, "failed slot metadata").unwrap();
        let association = test_association();
        let unknown = DecodeContext::mint_unknown_id(0);
        let mut mesh_budget = crate::mesh::MeshBudget::new();
        let (carriers, fields) = ctx
            .with_scoped_storage("surviving carrier fields", || {
                super::super::stage_brep_carriers(
                    super::super::BrepCarrierInput {
                        expand: crate::mesh::MeshExpand::new(&ctx, root),
                        data: &data,
                        archive: ArchiveVersion::V5,
                        writer_version: None,
                        raw: &raw,
                        key: "failed-prefix",
                        association: &association,
                        unknown: &unknown,
                        scale: MillimeterScale::IDENTITY,
                        mesh_budget: &mut mesh_budget,
                    },
                    false,
                    &mut backing,
                    &mut metadata,
                )
            })
            .unwrap();
        assert!(carriers.staged.draft.model().curves.is_empty());
        assert!(carriers.staged.draft.model().procedural_curves.is_empty());
        assert!(carriers.staged.links.is_empty());
        fields
            .commit()
            .expect("a discarded parsed prefix retains no fields");
        drop(carriers);
        drop(backing);
        drop(metadata);
        let reusable = ctx.reserve_scoped(1 << 20, "next Brep carrier").unwrap();
        drop(reusable);
        ctx.finish_session().unwrap();
    }
}

fn named_scan(
    class: [u8; 16],
    object_type: i64,
    payload: &[u8],
    name_length: usize,
) -> crate::container::Scan<'static> {
    let mut scan = scan_with_objects(&[object_record_with_payload(
        ArchiveVersion::V5,
        object_type,
        class,
        payload,
    )]);
    let ObjectRecord::Framed(object) = &mut scan.objects[0] else {
        panic!("framed object");
    };
    object.identity.name = "n".repeat(name_length);
    scan
}

fn geometry_rejection_cost(scan: &crate::container::Scan<'_>, warning: &str) -> u64 {
    let used = |reject| {
        let error = cadmpeg_test_support::refusal::resource_limit_at(
            ResourceDimension::RetainedBytes,
            "geometry rejection checkpoint",
            |cap| {
                let arena = DecodeArena::new();
                let mut policy = DecodePolicy::service();
                policy.limits.max_retained_bytes = cap;
                let (ctx, root) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
                    scan.data, &arena, &policy,
                )?;
                let mut transaction =
                    DecodeContext::new(scan, crate::mesh::MeshExpand::new(&ctx, root))?;
                transaction.decode_geometry()?;
                let counts = (
                    transaction.session.document().model.subds.len(),
                    transaction.session.document().model.surfaces.len(),
                    transaction.session.document().model.curves.len(),
                );
                assert!(counts.0 + counts.1 != 0, "the first carrier is accepted");
                if reject {
                    transaction.decode_geometry()?;
                    assert_eq!(
                        (
                            transaction.session.document().model.subds.len(),
                            transaction.session.document().model.surfaces.len(),
                            transaction.session.document().model.curves.len()
                        ),
                        counts
                    );
                    assert!(transaction
                        .report
                        .phase_warnings
                        .iter()
                        .any(|entry| entry.message.contains(warning)));
                }
                ctx.copy_retained_text("x", "geometry rejection checkpoint")?;
                drop(transaction);
                ctx.finish_session()
            },
        );
        let cadmpeg_core::CodecError::ResourceLimit(limit) = error else {
            panic!("checkpoint refusal");
        };
        limit.used
    };
    used(true) - used(false)
}

#[test]
fn rejected_subd_releases_its_source_association() {
    let payload = crate::subd::tests::quad_payload(ArchiveVersion::V5);
    let cost = |length| {
        geometry_rejection_cost(
            &named_scan(
                crate::test_support::test_dump::SUBD_CLASS,
                0x4_0000,
                &payload,
                length,
            ),
            "SubD validation rejected candidate",
        )
    };
    assert_eq!(
        cost(16),
        cost(4096),
        "rejected source names do not remain retained"
    );
}

#[test]
fn rejected_procedural_surface_releases_its_source_association() {
    let payload = crate::surfaces::tests::valid_revolution_payload(0x20);
    let cost = |length| {
        geometry_rejection_cost(
            &named_scan(REV_SURFACE_CLASS, 8, &payload, length),
            "procedural-surface: candidate rejected",
        )
    };
    assert_eq!(
        cost(16),
        cost(4096),
        "rejected source names do not remain retained"
    );
}

fn dimension_scan(text_length: usize) -> crate::container::Scan<'static> {
    use crate::test_support::test_dump as bytes;
    let mut payload = vec![0x10];
    bytes::push_i32(&mut payload, 1);
    payload.extend(crate::dimensions::tests::plane_bytes(
        [0.0; 3],
        [1.0, 0.0, 0.0],
        [0.0, 1.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
    ));
    bytes::push_i32(&mut payload, 5);
    for point in [[1.0, 2.0], [0.0, 0.0], [5.0, 0.0], [3.0, 0.0], [7.0, 4.0]] {
        for coordinate in point {
            bytes::push_f64(&mut payload, coordinate);
        }
    }
    payload.extend(bytes::utf16_bytes(&"t".repeat(text_length)));
    payload.extend(bytes::utf16_bytes("default"));
    bytes::push_i32(&mut payload, 0);
    let class = crate::wire::Uuid::from_canonical([
        0x5d, 0xe6, 0xb2, 0x0d, 0x48, 0x6b, 0x11, 0xd4, 0x80, 0x14, 0x00, 0x10, 0x83, 0x01, 0x22,
        0xf0,
    ])
    .to_wire();
    scan_with_objects(&[object_record_with_payload(
        ArchiveVersion::V5,
        0x200,
        class,
        &payload,
    )])
}

#[test]
fn rejected_dimension_releases_its_projected_text() {
    let mut previous_cost = None;
    for text_length in [16, 4096] {
        let scan = dimension_scan(text_length);
        let used = |reject| {
            let error =
                cadmpeg_test_support::refusal::resource_limit_at(
                    ResourceDimension::RetainedBytes,
                    "dimension rejection checkpoint",
                    |cap| {
                        let arena = DecodeArena::new();
                        let mut policy = DecodePolicy::service();
                        policy.limits.max_retained_bytes = cap;
                        let (ctx, root) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
                            scan.data, &arena, &policy,
                        )?;
                        let mut transaction =
                            DecodeContext::new(&scan, crate::mesh::MeshExpand::new(&ctx, root))?;
                        transaction.decode_dimensions()?;
                        assert_eq!(
                            transaction
                                .session
                                .document()
                                .model
                                .semantic_annotations
                                .len(),
                            1
                        );
                        assert_eq!(
                            transaction.session.document().model.semantic_annotations[0]
                                .parameters
                                .get("rich_text")
                                .unwrap(),
                            &"t".repeat(text_length)
                        );
                        if reject {
                            transaction.decode_dimensions()?;
                            assert_eq!(
                                transaction
                                    .session
                                    .document()
                                    .model
                                    .semantic_annotations
                                    .len(),
                                1
                            );
                            assert!(transaction.report.phase_warnings.iter().any(|entry| entry
                                .message
                                .contains("dimension candidate rejected")));
                        }
                        ctx.copy_retained_text("x", "dimension rejection checkpoint")?;
                        drop(transaction);
                        ctx.finish_session()
                    },
                );
            let cadmpeg_core::CodecError::ResourceLimit(limit) = error else {
                panic!("checkpoint refusal");
            };
            limit.used
        };
        let cost = used(true) - used(false);
        if let Some(previous) = previous_cost {
            assert_eq!(
                cost, previous,
                "discarded projected text adds no retained storage"
            );
        }
        previous_cost = Some(cost);
    }
}

#[test]
fn accepted_subd_proxy_keeps_parent_mesh_fields_speculative() {
    use crate::test_support::test_dump as bytes;
    let archive = ArchiveVersion::V5;
    let mut scan = scan_with_objects(&[object_record_with_payload(
        archive,
        0x20,
        MESH_CLASS,
        &bytes::mesh_payload(),
    )]);
    let mut proxy_userdata = crate::objects::ClassUserdata {
        range: 0..0,
        version: (2, 2),
        class_uuid: crate::subd::SUBD_MESH_PROXY_USERDATA,
        item_uuid: crate::subd::SUBD_MESH_PROXY_USERDATA,
        copy_count: 1,
        transform_range: 0..0,
        application_uuid: None,
        save_context: None,
        payload_range: 0..0,
    };
    let fingerprint = with_expand(&scan, |expand| {
        let object = scan.objects[0].framed().unwrap();
        crate::mesh::decode(
            expand,
            scan.data,
            object.class_data_range.clone(),
            archive,
            crate::mesh::MeshDecodeOptions {
                writer_version: None,
                association: None,
                id: crate::mesh::MeshId::Ready("rhino:test:mesh#parent".try_into().unwrap()),
                scale: MillimeterScale::IDENTITY,
                userdata: &[crate::objects::UserdataDescriptor::Known(
                    proxy_userdata.clone(),
                )],
            },
            &mut crate::mesh::MeshBudget::new(),
        )
        .unwrap()
        .proxy_fingerprint
        .unwrap()
    });
    let mut data = scan.data.to_vec();
    let transform_start = data.len();
    for index in 0..16 {
        bytes::push_f64(&mut data, if index % 5 == 0 { 1.0 } else { 0.0 });
    }
    let transform_range = transform_start..data.len();
    let mut body = Vec::new();
    bytes::push_i32(&mut body, 1);
    bytes::push_i32(&mut body, 1);
    body.push(1);
    body.extend(crate::subd::tests::quad_payload(archive));
    bytes::push_i32(&mut body, i32::try_from(fingerprint.face_count).unwrap());
    bytes::push_i32(&mut body, i32::try_from(fingerprint.vertex_count).unwrap());
    for digest in [fingerprint.face_sha1, fingerprint.vertex_sha1] {
        let mut hash = Vec::new();
        bytes::push_i32(&mut hash, 1);
        bytes::push_i32(&mut hash, 0);
        hash.extend(digest);
        body.extend(bytes::crc_chunk(archive, 0x4000_8000, &hash));
    }
    let payload_start = data.len();
    data.extend(bytes::crc_chunk(archive, 0x4000_8000, &body));
    let payload_range = payload_start..data.len();
    scan.data = Box::leak(data.into_boxed_slice());
    let ObjectRecord::Framed(object) = &mut scan.objects[0] else {
        panic!("framed mesh");
    };
    proxy_userdata.range = payload_range.clone();
    proxy_userdata.transform_range = transform_range;
    proxy_userdata.payload_range = payload_range;
    object
        .userdata
        .push(crate::objects::UserdataDescriptor::Known(proxy_userdata));
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = u64::MAX;
    let (ctx, root) =
        cadmpeg_core::decode::DecodeContext::from_root_bytes(scan.data, &arena, &policy).unwrap();
    let expand = crate::mesh::MeshExpand::new(&ctx, root);
    {
        let mut transaction = DecodeContext::new(&scan, expand).unwrap();
        let _probe = cadmpeg_core::decode::refusal_probe::RefusalProbe::arm(
            ResourceDimension::RetainedBytes,
            "Rhino mesh scaled vertices",
            None,
        );
        transaction.decode_geometry().unwrap();
        assert_eq!(transaction.session.document().model.subds.len(), 1);
        assert!(transaction
            .session
            .document()
            .model
            .tessellations
            .is_empty());
        assert!(expand.ctx().resource_refusal().is_none());
    }
    ctx.finish_session().unwrap();
}

#[test]
fn failed_brep_mesh_slot_releases_its_parsed_fields() {
    let mut data = crate::test_support::test_dump::mesh_payload();
    data.pop().unwrap();
    let (_, mut raw) = source_shaped_plane_brep();
    raw.c3.slots.clear();
    raw.surfaces.slots.clear();
    raw.analysis_meshes.clear();
    raw.render_meshes = vec![Some(crate::brep::RawBrepMesh {
        mesh: crate::brep::RawBrepChild {
            class_uuid: crate::mesh::ON_MESH,
            class_data_range: 0..data.len(),
            source_range: 0..data.len(),
        },
        userdata: Vec::new(),
    })];
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_materialized_bytes = 1 << 20;
    let (ctx, root) =
        cadmpeg_core::decode::DecodeContext::from_root_bytes(&data, &arena, &policy).unwrap();
    let mut backing = ctx.reserve_scoped(0, "failed mesh backing").unwrap();
    let mut metadata = ctx.reserve_scoped(0, "failed mesh metadata").unwrap();
    let association = test_association();
    let unknown = DecodeContext::mint_unknown_id(0);
    let mut mesh_budget = crate::mesh::MeshBudget::new();
    let (carriers, fields) = ctx
        .with_scoped_storage("surviving mesh fields", || {
            super::super::stage_brep_carriers(
                super::super::BrepCarrierInput {
                    expand: crate::mesh::MeshExpand::new(&ctx, root),
                    data: &data,
                    archive: ArchiveVersion::V5,
                    writer_version: None,
                    raw: &raw,
                    key: "failed-mesh",
                    association: &association,
                    unknown: &unknown,
                    scale: MillimeterScale::IDENTITY,
                    mesh_budget: &mut mesh_budget,
                },
                false,
                &mut backing,
                &mut metadata,
            )
        })
        .unwrap();
    assert!(carriers.staged.draft.model().tessellations.is_empty());
    assert!(carriers.staged.links.is_empty());
    assert_eq!(carriers.staged.warnings.len(), 1);
    assert!(carriers.staged.warnings[0]
        .message
        .starts_with("invalid render mesh cache slot 0:"));
    fields.commit().expect("a failed mesh retains no fields");
    drop(carriers);
    drop(backing);
    drop(metadata);
    let reusable = ctx.reserve_scoped(1 << 20, "next mesh carrier").unwrap();
    drop(reusable);
    ctx.finish_session().unwrap();
}

#[test]
fn brep_mesh_diagnostics_keep_storage_after_candidate_fields_drop() {
    let mut data = crate::test_support::test_dump::mesh_payload();
    data.truncate(data.len() - std::mem::size_of::<i32>());
    crate::test_support::test_dump::push_i32(&mut data, -1);
    let (_, mut raw) = source_shaped_plane_brep();
    raw.c3.slots.clear();
    raw.surfaces.slots.clear();
    raw.analysis_meshes.clear();
    raw.render_meshes = vec![Some(crate::brep::RawBrepMesh {
        mesh: crate::brep::RawBrepChild {
            class_uuid: crate::mesh::ON_MESH,
            class_data_range: 0..data.len(),
            source_range: 0..data.len(),
        },
        userdata: Vec::new(),
    })];
    let mut costs = [0; 3];
    for (index, (backing_only, release)) in [(false, false), (false, true), (true, true)]
        .into_iter()
        .enumerate()
    {
        let error = cadmpeg_test_support::refusal::resource_limit_at(
            ResourceDimension::MaterializedBytes,
            "mesh diagnostic lifetime checkpoint",
            |cap| {
                let arena = DecodeArena::new();
                let mut policy = DecodePolicy::service();
                policy.limits.max_materialized_bytes = cap;
                let (ctx, root) =
                    cadmpeg_core::decode::DecodeContext::from_root_bytes(&data, &arena, &policy)?;
                let mut backing = ctx.reserve_scoped(0, "mesh diagnostic backing")?;
                let mut metadata = ctx.reserve_scoped(0, "mesh diagnostic metadata")?;
                let association = test_association();
                let unknown = DecodeContext::mint_unknown_id(0);
                let mut mesh_budget = crate::mesh::MeshBudget::new();
                let (mut carriers, fields) = ctx
                    .with_scoped_storage("mesh diagnostic candidate fields", || {
                        super::super::stage_brep_carriers(
                            super::super::BrepCarrierInput {
                                expand: crate::mesh::MeshExpand::new(&ctx, root),
                                data: &data,
                                archive: ArchiveVersion::V5,
                                writer_version: None,
                                raw: &raw,
                                key: "mesh-diagnostic",
                                association: &association,
                                unknown: &unknown,
                                scale: MillimeterScale::IDENTITY,
                                mesh_budget: &mut mesh_budget,
                            },
                            false,
                            &mut backing,
                            &mut metadata,
                        )
                    })
                    .map_err(|error| match error {
                        crate::curves::GeometryError::Codec(error) => error,
                        error => panic!("valid mesh with a redundant-channel warning: {error}"),
                    })?;
                assert_eq!(carriers.staged.draft.model().tessellations.len(), 1);
                let warnings = std::mem::take(&mut carriers.staged.warnings);
                assert_eq!(warnings.len(), 1);
                assert_eq!(
                    warnings[0].code,
                    Some(RhinoLossCode::RedundantFieldRepaired)
                );
                assert_eq!(
                    warnings[0].message,
                    "redundant mesh colors channel has a negative count; channel dropped"
                );
                drop(carriers);
                drop(fields);
                drop(backing);
                if release {
                    drop(warnings);
                    drop(metadata);
                }
                // Reproduce the same vector growth without retaining a message.
                let mut control_storage = ctx.reserve_scoped(0, "diagnostic backing control")?;
                let mut control = Diagnostics::new();
                if backing_only {
                    control_storage.with_storage(|| {
                        control.push_coded_admitted(&ctx, None, format_args!(""))
                    })?;
                }
                let held = ctx.reserve_scoped(1 << 20, "mesh diagnostic lifetime checkpoint")?;
                drop(held);
                Ok(())
            },
        );
        let cadmpeg_core::CodecError::ResourceLimit(limit) = error else {
            panic!("checkpoint refusal");
        };
        costs[index] = limit.used;
        if release && !backing_only {
            assert_eq!(limit.used, 0);
        } else if !release {
            assert!(limit.used > 0, "live diagnostic fields retain scratch");
        }
    }
    assert!(
        costs[0] > costs[2],
        "the live diagnostic message remains charged beyond vector backing"
    );
}

#[test]
fn accepted_extrusion_copies_its_start_curve_without_retaining_staging() {
    let payload = crate::extrusion::tests::archive_payload(2, [false, false], false, false);
    let scan = scan_with_objects(&[object_record_with_payload(
        ArchiveVersion::V5,
        0x40_0000,
        crate::test_support::test_archive::EXTRUSION_CLASS,
        &payload,
    )]);
    {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = u64::MAX;
        let (ctx, root) =
            cadmpeg_core::decode::DecodeContext::from_root_bytes(scan.data, &arena, &policy)
                .unwrap();
        let mut transaction =
            DecodeContext::new(&scan, crate::mesh::MeshExpand::new(&ctx, root)).unwrap();
        let _probe = cadmpeg_core::decode::refusal_probe::RefusalProbe::arm(
            ResourceDimension::RetainedBytes,
            "Rhino extrusion boundary staging",
            None,
        );
        transaction.decode_geometry().unwrap();
        assert_eq!(transaction.session.document().model.curves.len(), 1);
        assert_eq!(transaction.session.document().model.surfaces.len(), 1);
        assert!(ctx.resource_refusal().is_none());
        drop(transaction);
        ctx.finish_session().unwrap();
    }
    let error = cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::WorkUnits,
        "Rhino committed curve geometry copy",
        |cap| {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_work_units = cap;
            let (ctx, root) =
                cadmpeg_core::decode::DecodeContext::from_root_bytes(scan.data, &arena, &policy)?;
            let mut transaction =
                DecodeContext::new(&scan, crate::mesh::MeshExpand::new(&ctx, root))?;
            transaction.decode_geometry()
        },
    );
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "Rhino committed curve geometry copy")
    );
}

#[test]
fn accepted_point_cloud_releases_its_consumed_parser_vector() {
    use crate::test_support::test_archive as bytes;
    use cadmpeg_core::decode::refusal_probe::RefusalProbe;
    for count in [4_u32, 128] {
        let points = (0..count)
            .map(|index| [f64::from(index), 2.0, 3.0])
            .collect::<Vec<_>>();
        let payload = bytes::point_cloud_payload(&points);
        let scan = named_scan(bytes::POINT_CLOUD_CLASS, 2, &payload, 16);
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = u64::MAX;
        let (ctx, root) =
            cadmpeg_core::decode::DecodeContext::from_root_bytes(scan.data, &arena, &policy)
                .unwrap();
        let mut transaction =
            DecodeContext::new(&scan, crate::mesh::MeshExpand::new(&ctx, root)).unwrap();
        {
            let _probe = RefusalProbe::arm(
                ResourceDimension::RetainedBytes,
                "Rhino speculative geometry fields",
                None,
            );
            transaction.decode_geometry().unwrap();
            let model = &transaction.session.document().model;
            assert_eq!(model.points.len(), usize::try_from(count).unwrap());
            assert_eq!(model.vertices.len(), model.points.len());
            assert_eq!(model.bodies.len(), 1);
            for (point, expected) in model.points.iter().zip(&points) {
                assert_eq!(
                    point.position().get(),
                    Point3::new(expected[0], expected[1], expected[2])
                );
                assert_eq!(
                    point.source_object.as_ref().unwrap().name.as_deref(),
                    Some("nnnnnnnnnnnnnnnn")
                );
            }
            assert!(ctx.resource_refusal().is_none());
        }
        drop(transaction);
        ctx.finish_session().unwrap();
    }
}

#[test]
fn accepted_polycurve_releases_its_parser_children() {
    use crate::test_support::test_archive as bytes;
    use cadmpeg_core::decode::refusal_probe::RefusalProbe;
    let payload = bytes::polycurve_payload(
        &[0.0, 1.0, 2.0],
        &[
            (
                bytes::POLYLINE_CLASS,
                bytes::polyline_payload(&[[0.0, 0.0, 0.0], [1.0, 0.0, 0.0]], &[0.0, 1.0]),
            ),
            (
                bytes::POLYLINE_CLASS,
                bytes::polyline_payload(&[[1.0, 0.0, 0.0], [2.0, 0.0, 0.0]], &[0.0, 1.0]),
            ),
        ],
    );
    let scan = named_scan(bytes::POLYCURVE_CLASS, 4, &payload, 16);
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = u64::MAX;
    let (ctx, root) =
        cadmpeg_core::decode::DecodeContext::from_root_bytes(scan.data, &arena, &policy).unwrap();
    let mut transaction =
        DecodeContext::new(&scan, crate::mesh::MeshExpand::new(&ctx, root)).unwrap();
    {
        let _probe = RefusalProbe::arm(
            ResourceDimension::RetainedBytes,
            "Rhino speculative geometry fields",
            None,
        );
        transaction.decode_geometry().unwrap();
        let model = &transaction.session.document().model;
        assert_eq!(model.curves.len(), 3);
        assert_eq!(model.procedural_curves.len(), 1);
        for (index, curve) in model.curves[..2].iter().enumerate() {
            let CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(nurbs)) = &curve.geometry else {
                panic!("polyline child stays a solved NURBS curve");
            };
            let start = f64::from(u8::try_from(index).unwrap());
            assert_eq!(
                nurbs.control_points().first().unwrap(),
                &Point3::new(start, 0.0, 0.0)
            );
            assert_eq!(
                nurbs.control_points().last().unwrap(),
                &Point3::new(start + 1.0, 0.0, 0.0)
            );
        }
        assert!(matches!(
            model.curves[2].geometry,
            CurveGeometry::Procedural { .. }
        ));
        assert!(ctx.resource_refusal().is_none());
    }
    drop(transaction);
    ctx.finish_session().unwrap();
}
