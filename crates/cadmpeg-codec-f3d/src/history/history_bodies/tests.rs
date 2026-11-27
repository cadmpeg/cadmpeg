// SPDX-License-Identifier: Apache-2.0
use super::*;

mod final_bodies;
mod historical_split;
mod retained_budget;
use crate::records::feature::scope::DesignParameterScope;
use cadmpeg_asm::kernel_header::RefWidth;
use cadmpeg_core::decode::{DecodeArena, DecodePolicy};
use cadmpeg_ir::attributes::{AttributeTarget, SourceAttribute};
use cadmpeg_ir::features::evaluation::FeatureEvaluation;
use cadmpeg_ir::features::selections::{BodySelection, DistinctMembers};
use cadmpeg_ir::features::{FeatureContent, FeatureDefinition, FeatureId, FeatureOperation};

fn feature(native: &str) -> Feature {
    Feature {
        product_definition: None,
        id: FeatureId::mint(format!("f3d:model:feature#{native}")).expect("identity"),
        ordinal: 0,
        name: None,
        suppressed: Some(false),
        dependencies: DistinctMembers::default(),
        source_properties: BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: FeatureContent::default(),
        evaluation: FeatureEvaluation::from_definition(FeatureDefinition::Operation(
            FeatureOperation::BaseFeature {
                bodies: BodySelection::Native("f3d:test:scope#base".into()),
            },
        )),
        native_ref: Some(native.to_owned()),
    }
}

fn put_ref(out: &mut Vec<u8>, width: RefWidth, tag: u8, value: i64) {
    out.push(tag);
    match width {
        RefWidth::Four => out.extend_from_slice(
            &i32::try_from(value)
                .expect("fixture value fits i32")
                .to_le_bytes(),
        ),
        RefWidth::Eight => out.extend_from_slice(&value.to_le_bytes()),
    }
}

fn put_name(out: &mut Vec<u8>, name: &str) {
    let parts: Vec<&str> = name.split('-').collect();
    for (index, part) in parts.iter().enumerate() {
        out.push(if index + 1 == parts.len() { 0x0d } else { 0x0e });
        out.push(u8::try_from(part.len()).expect("fixture name fits a byte"));
        out.extend_from_slice(part.as_bytes());
    }
}

/// A record whose payload is references and integers: `(tag, value)` pairs.
fn put_plain(out: &mut Vec<u8>, width: RefWidth, name: &str, fields: &[(u8, i64)]) {
    put_fields(out, width, name, fields);
    out.push(0x11);
}

/// The name and fields of a record, before its terminator.
fn put_fields(out: &mut Vec<u8>, width: RefWidth, name: &str, fields: &[(u8, i64)]) {
    put_name(out, name);
    for &(tag, value) in fields {
        put_ref(out, width, tag, value);
    }
}

const REF: u8 = 0x0c;
const INT: u8 = 0x04;

fn put_header(out: &mut Vec<u8>, width: RefWidth) {
    let (magic, len): (&[u8], usize) = match width {
        RefWidth::Four => (b"ASM BinaryFile4", 31),
        RefWidth::Eight => (b"ASM BinaryFile8", 47),
    };
    out.extend_from_slice(magic);
    out.extend_from_slice(&23_200_u32.to_le_bytes());
    out.resize(len, 0);
    for _ in 0..3 {
        out.extend_from_slice(&[0x07, 0]);
    }
    for _ in 0..3 {
        out.push(0x06);
        out.extend_from_slice(&1.0f64.to_le_bytes());
    }
}

/// The records of a sphere solid below a body, from slot `body + 1` on:
/// lump, shell, face, sphere surface. `body` is the slot of the body record.
fn put_sphere_solid(
    out: &mut Vec<u8>,
    width: RefWidth,
    body: i64,
    key: i64,
    refer: i64,
    center: [f64; 3],
) {
    let (lump, shell, face, surface) = (body + 1, body + 2, body + 3, body + 4);
    put_plain(
        out,
        width,
        "body",
        &[
            (REF, -1),
            (INT, key),
            (REF, refer),
            (REF, lump),
            (REF, -1),
            (REF, -1),
        ],
    );
    put_plain(
        out,
        width,
        "lump",
        &[
            (REF, -1),
            (INT, -1),
            (REF, -1),
            (REF, -1),
            (REF, shell),
            (REF, body),
        ],
    );
    put_plain(
        out,
        width,
        "shell",
        &[
            (REF, -1),
            (INT, -1),
            (REF, -1),
            (REF, -1),
            (REF, -1),
            (REF, face),
            (REF, -1),
            (REF, lump),
        ],
    );
    put_fields(
        out,
        width,
        "face",
        &[
            (REF, -1),
            (INT, -1),
            (REF, -1),
            (REF, -1),
            (REF, -1),
            (REF, shell),
            (REF, -1),
            (REF, surface),
        ],
    );
    out.extend_from_slice(&[0x0b, 0x0b, 0x11]);
    put_surface(out, width, center);
}

fn put_surface(out: &mut Vec<u8>, width: RefWidth, center: [f64; 3]) {
    put_name(out, "sphere-surface");
    put_ref(out, width, REF, -1);
    put_ref(out, width, INT, -1);
    put_ref(out, width, REF, -1);
    out.push(0x13);
    for value in center {
        out.extend_from_slice(&value.to_le_bytes());
    }
    out.push(0x06);
    out.extend_from_slice(&2.5f64.to_le_bytes());
    for triple in [[1.0f64, 0.0, 0.0], [0.0, 0.0, 1.0]] {
        out.push(0x14);
        for value in triple {
            out.extend_from_slice(&value.to_le_bytes());
        }
    }
    out.extend_from_slice(&[0x0b; 5]);
    out.push(0x11);
}

/// A `delta_state` record with one board of `(old, new)` changes.
fn put_state(
    out: &mut Vec<u8>,
    width: RefWidth,
    ids: (i64, i64, i64, i64),
    changes: &[(i64, i64)],
) {
    let (id, previous, next, node) = ids;
    put_name(out, "delta_state");
    for value in [id, 1, 0] {
        put_ref(out, width, INT, value);
    }
    for value in [previous, next, node, -1, -1] {
        put_ref(out, width, REF, value);
    }
    out.push(0x0b);
    put_ref(out, width, INT, 1);
    put_ref(out, width, REF, node);
    put_ref(out, width, INT, 1);
    for &(old, new) in changes {
        put_ref(out, width, INT, 1);
        put_ref(out, width, REF, old);
        put_ref(out, width, REF, new);
    }
    put_ref(out, width, INT, 0);
    put_ref(out, width, INT, 0);
    out.push(0x11);
}

fn carrier_bytes(width: RefWidth, changed: bool) -> Vec<u8> {
    let mut out = Vec::new();
    put_header(&mut out, width);
    put_plain(&mut out, width, "asmheader", &[]);
    put_sphere_solid(&mut out, width, 1, 7, -1, [10.0, 0.0, 0.0]);
    let changes = if changed {
        vec![(6, 1), (7, 5)]
    } else {
        Vec::new()
    };
    put_state(&mut out, width, (13, -1, 1, 0), &changes);
    if changed {
        put_plain(
            &mut out,
            width,
            "body",
            &[
                (REF, -1),
                (INT, 7),
                (REF, -1),
                (REF, 2),
                (REF, -1),
                (REF, -1),
            ],
        );
        put_surface(&mut out, width, [0.0, 0.0, 0.0]);
    }
    put_state(
        &mut out,
        width,
        (12, 0, -1, 1),
        &[(-1, 1), (-1, 2), (-1, 3), (-1, 4), (-1, 5)],
    );
    out
}

fn run(
    changed: bool,
    width: RefWidth,
    count: usize,
    work: usize,
) -> (Vec<HistoryBody>, Vec<cadmpeg_ir::report::loss::LossNote>) {
    run_bytes(&carrier_bytes(width, changed), width, count, work)
}

fn run_bytes(
    bytes: &[u8],
    width: RefWidth,
    count: usize,
    work: usize,
) -> (Vec<HistoryBody>, Vec<cadmpeg_ir::report::loss::LossNote>) {
    run_roles(bytes, width, count, work, (true, false), 1)
}

fn run_roles(
    bytes: &[u8],
    width: RefWidth,
    count: usize,
    work: usize,
    roles: (bool, bool),
    slot: i64,
) -> (Vec<HistoryBody>, Vec<cadmpeg_ir::report::loss::LossNote>) {
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(bytes, &arena, &DecodePolicy::default()).unwrap();
    fill_roles(&ctx, bytes, width, (count, work), roles, slot).unwrap()
}

/// [`fill_history`] for `count` wants of the body of `slot` at state 12, with
/// a work cap of `work`.
fn fill_roles(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    width: RefWidth,
    (count, work): (usize, usize),
    roles: (bool, bool),
    slot: i64,
) -> Result<(Vec<HistoryBody>, Vec<cadmpeg_ir::report::loss::LossNote>), CodecError> {
    let mut allowance = u64::MAX;
    let history = super::super::decode(
        ctx,
        bytes,
        "test",
        width,
        &mut allowance,
        &WorkBudget::new(1 << 20),
    )
    .unwrap()
    .unwrap();
    let feature = feature("base");
    let wants = (0..count)
        .map(|_| Want {
            origin: &feature,
            producer: roles.0.then_some(&feature),
            consumer: roles.1.then_some(&feature),
            state: 12,
            slot,
            identity: None,
            capture_current: false,
            consumed: false,
        })
        .collect::<Vec<_>>();
    let mut bodies = Vec::new();
    let mut losses = Vec::new();
    fill_history(
        ctx,
        bytes,
        (&history, None),
        &wants,
        &mut bodies,
        &WorkBudget::new(work),
        &mut losses,
    )?;
    Ok((bodies, losses))
}

#[test]
fn a_later_body_revision_preserves_the_base_feature_geometry() {
    for width in [RefWidth::Four, RefWidth::Eight] {
        let (bodies, losses) = run(true, width, 1, MAX_HISTORY_BODY_WORK);
        assert!(losses.is_empty(), "{losses:?}");
        let [body] = bodies.as_slice() else {
            panic!("one history body")
        };
        assert_eq!(body.produced_by, Some(feature("base").id));
        assert_eq!(
            body.state,
            HistoryStateRef {
                ordinal: 0,
                identifier: 12
            }
        );
        assert_eq!(body.topology.faces.len(), 1);
        let SurfaceGeometry::Solved(SolvedSurfaceGeometry::Sphere(sphere)) =
            &body.topology.surfaces[0].geometry
        else {
            panic!("sphere")
        };
        assert_eq!(
            sphere.center().get(),
            cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0)
        );
        let mut ir = CadIr::empty();
        ir.model.features.push(feature("base"));
        ir.model.history_bodies = bodies;
        let report = cadmpeg_ir::validate_neutral(&ir, Vec::new()).unwrap();
        assert!(
            !report
                .findings
                .iter()
                .any(|finding| finding.severity == cadmpeg_ir::report::Severity::Error),
            "{report:?}"
        );
    }
}

#[test]
fn an_incomplete_history_body_closure_reports_the_undecoded_body() {
    for width in [RefWidth::Four, RefWidth::Eight] {
        let mut bytes = Vec::new();
        put_header(&mut bytes, width);
        put_plain(&mut bytes, width, "asmheader", &[]);
        put_sphere_solid(&mut bytes, width, 1, 7, -1, [10.0, 0.0, 0.0]);
        put_plain(&mut bytes, width, "lump", &[]);
        put_state(&mut bytes, width, (13, -1, 1, 0), &[(7, 1), (-1, 6)]);
        put_plain(
            &mut bytes,
            width,
            "body",
            &[
                (REF, -1),
                (INT, 7),
                (REF, -1),
                (REF, 6),
                (REF, -1),
                (REF, -1),
            ],
        );
        put_state(
            &mut bytes,
            width,
            (12, 0, -1, 1),
            &[(-1, 1), (-1, 2), (-1, 3), (-1, 4), (-1, 5)],
        );
        let (bodies, losses) = run_bytes(&bytes, width, 1, MAX_HISTORY_BODY_WORK);
        assert!(bodies.is_empty());
        assert_eq!(losses.len(), 1);
        assert_eq!(
            losses[0].code,
            F3dLossCode::FeatureHistoryBodyUndecoded
                .note("incomplete")
                .code
        );
    }
}

#[test]
fn an_invalid_history_body_nest_reports_the_undecoded_body() {
    for width in [RefWidth::Four, RefWidth::Eight] {
        let mut bytes = Vec::new();
        put_header(&mut bytes, width);
        put_plain(&mut bytes, width, "asmheader", &[]);
        put_sphere_solid(&mut bytes, width, 1, 7, -1, [10.0, 0.0, 0.0]);
        put_plain(
            &mut bytes,
            width,
            "shell",
            &[
                (REF, -1),
                (INT, -1),
                (REF, -1),
                (REF, -1),
                (REF, -1),
                (REF, -1),
                (REF, 8),
                (REF, 2),
            ],
        );
        for owner in [3, 6] {
            put_plain(
                &mut bytes,
                width,
                "wire",
                &[
                    (REF, -1),
                    (INT, -1),
                    (REF, -1),
                    (REF, -1),
                    (REF, -1),
                    (REF, owner),
                    (REF, 9),
                ],
            );
        }
        put_plain(
            &mut bytes,
            width,
            "vertex",
            &[
                (REF, -1),
                (INT, -1),
                (REF, -1),
                (REF, -1),
                (INT, 0),
                (REF, 10),
            ],
        );
        put_fields(
            &mut bytes,
            width,
            "point",
            &[(REF, -1), (INT, -1), (REF, -1)],
        );
        bytes.push(0x13);
        for value in [0.0_f64, 0.0, 0.0] {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        bytes.push(0x11);
        put_state(&mut bytes, width, (13, -1, 1, 0), &[(11, 1), (12, 3)]);
        put_plain(
            &mut bytes,
            width,
            "body",
            &[
                (REF, -1),
                (INT, 7),
                (REF, -1),
                (REF, 2),
                (REF, -1),
                (REF, -1),
            ],
        );
        put_plain(
            &mut bytes,
            width,
            "shell",
            &[
                (REF, -1),
                (INT, -1),
                (REF, -1),
                (REF, 6),
                (REF, -1),
                (REF, 4),
                (REF, 7),
                (REF, 2),
            ],
        );
        let inserted = (1..=10).map(|slot| (-1, slot)).collect::<Vec<_>>();
        put_state(&mut bytes, width, (12, 0, -1, 1), &inserted);
        let arena = DecodeArena::new();
        let (ctx, _) =
            DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::default()).unwrap();
        let header = cadmpeg_asm::asm_header::parse(&bytes).unwrap();
        let mut allowance = u64::MAX;
        let history = super::super::decode(
            &ctx,
            &bytes,
            "test",
            width,
            &mut allowance,
            &WorkBudget::new(1 << 20),
        )
        .unwrap()
        .unwrap();
        let archive = super::super::decode_record_archive(&ctx, history.states(), &bytes, width)
            .unwrap()
            .unwrap();
        let mut versions = (0..=10)
            .map(|slot| (slot, slot))
            .collect::<BTreeMap<_, _>>();
        versions.insert(1, 11);
        versions.insert(3, 12);
        let table = cadmpeg_asm::history::materialize_record_table(&versions, |revision| {
            archive.get(&revision)
        })
        .expect("complete historical record table");
        let subtypes = archive_subtypes(&archive, header.metadata.save_format_version);
        let brep = cadmpeg_asm::brep::decode_with_table(
            &ctx,
            &table,
            &bytes,
            (Some(&header.metadata), &subtypes),
            "f3d:history",
            crate::ids::ID_FORMAT,
            cadmpeg_asm::brep::DecodePurpose::Model,
        )
        .unwrap();
        let mut scratch = CadIr::empty();
        cadmpeg_asm::brep::transfer::transfer_into_ir(
            &ctx,
            &mut scratch,
            cadmpeg_ir::codec_id!("f3d"),
            brep,
        )
        .unwrap();
        let body = &scratch.geometry.bodies[0];
        let Extract::Nest(topology) = extract(&ctx, &scratch.geometry, body.id.as_str()).unwrap()
        else {
            panic!("reference-closed historical nest")
        };
        assert_eq!(topology.shells.len(), 2);
        assert_eq!(topology.vertices.len(), 1);
        for shell in &topology.shells {
            assert_eq!(
                shell.free_vertices().collect::<Vec<_>>(),
                [&topology.vertices[0].id]
            );
        }
        let candidate = HistoryBody {
            id: HistoryBodyId::mint("test:history-body:nest#shared-vertex").unwrap(),
            produced_by: Some(feature("base").id),
            consumed_by: None,
            state_of: None,
            state: HistoryStateRef {
                ordinal: 0,
                identifier: 12,
            },
            topology,
        };
        assert!(admit(&ctx, candidate, &feature("base")).unwrap().is_none());
        let (bodies, losses) = run_bytes(&bytes, width, 1, MAX_HISTORY_BODY_WORK);
        assert!(bodies.is_empty());
        assert_eq!(losses.len(), 1);
        assert_eq!(
            losses[0].code,
            F3dLossCode::FeatureHistoryBodyUndecoded
                .note("invalid")
                .code
        );
    }
}

#[test]
fn an_unchanged_body_needs_no_history_body() {
    let (bodies, losses) = run(false, RefWidth::Eight, 1, MAX_HISTORY_BODY_WORK);
    assert!(bodies.is_empty());
    assert!(losses.is_empty());
}

fn changed_surface_bytes(width: RefWidth) -> Vec<u8> {
    let mut bytes = Vec::new();
    put_header(&mut bytes, width);
    put_plain(&mut bytes, width, "asmheader", &[]);
    put_sphere_solid(&mut bytes, width, 1, 7, -1, [10.0, 0.0, 0.0]);
    put_state(&mut bytes, width, (13, -1, 1, 0), &[(6, 5)]);
    put_surface(&mut bytes, width, [0.0, 0.0, 0.0]);
    put_state(
        &mut bytes,
        width,
        (12, 0, -1, 1),
        &[(-1, 1), (-1, 2), (-1, 3), (-1, 4), (-1, 5)],
    );
    bytes
}

#[test]
fn a_consumed_snapshot_keeps_the_input_geometry_and_consumer() {
    const MILLIMETERS_PER_CENTIMETER: f64 = 10.0;
    for width in [RefWidth::Four, RefWidth::Eight] {
        for (case, (mut bytes, expected_x)) in [
            (
                carrier_bytes(width, false),
                10.0 * MILLIMETERS_PER_CENTIMETER,
            ),
            (carrier_bytes(width, true), 0.0),
            (changed_surface_bytes(width), 0.0),
        ]
        .into_iter()
        .enumerate()
        {
            let flags = match width {
                RefWidth::Four => 27,
                RefWidth::Eight => 39,
            };
            bytes[flags] = 1;
            put_name(&mut bytes, "End-of-ASM-data");
            let arena = DecodeArena::new();
            let (ctx, _) =
                DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::default()).unwrap();
            let mut allowance = u64::MAX;
            let history = super::super::decode(
                &ctx,
                &bytes,
                "test",
                width,
                &mut allowance,
                &WorkBudget::new(1 << 20),
            )
            .unwrap()
            .unwrap();
            let cut = feature("cut");
            let mut bodies = Vec::new();
            let mut losses = Vec::new();
            fill_history(
                &ctx,
                &bytes,
                (&history, None),
                &[Want {
                    origin: &cut,
                    producer: None,
                    consumer: Some(&cut),
                    identity: None,
                    state: 12,
                    slot: 1,
                    consumed: true,
                    capture_current: false,
                }],
                &mut bodies,
                &WorkBudget::new(MAX_HISTORY_BODY_WORK),
                &mut losses,
            )
            .unwrap();
            assert!(losses.is_empty(), "{losses:?}");
            let [body] = bodies.as_slice() else {
                panic!(
                    "one input snapshot: {width:?}, case {case}, states {:?}",
                    history.states()
                )
            };
            assert_eq!(body.produced_by, None);
            assert_eq!(body.consumed_by.as_ref(), Some(&cut.id));
            assert_eq!(body.state.identifier, 12);
            let SurfaceGeometry::Solved(SolvedSurfaceGeometry::Sphere(sphere)) =
                &body.topology.surfaces[0].geometry
            else {
                panic!("sphere")
            };
            assert_eq!(
                sphere.center().get(),
                cadmpeg_ir::math::Point3::new(expected_x, 0.0, 0.0)
            );
        }
    }
}

fn consumed_fixture(
    bytes: &[u8],
    features: Vec<Feature>,
    scope: &DesignParameterScope,
    history: &AsmHistory,
) -> (Model, Vec<cadmpeg_ir::report::loss::LossNote>) {
    use std::io::{Cursor, Write};
    let name = "FusionAssetName[Active]/Breps.BlobParts/BREP.a.smbh";
    let mut blob = bytes.to_vec();
    blob[39..47].copy_from_slice(&1_u64.to_le_bytes());
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let options = crate::zip_write::file_options(zip::CompressionMethod::Stored);
    crate::test_support::manifest_test::write_synthetic_manifests(&mut zip, options);
    zip.start_file(name, options).unwrap();
    zip.write_all(&blob).unwrap();
    let archive = zip.finish().unwrap().into_inner();
    let mut history = history.clone();
    history.id = format!("f3d:{name}:asm-history#0");
    let native = F3dNative {
        asm_histories: vec![history],
        design_parameter_scopes: vec![scope.clone()],
        ..F3dNative::default()
    };
    let mut model = Model::default();
    model.features = features;
    let mut losses = Vec::new();
    crate::test_support::zip_test::with_scan(&archive, |scan| {
        assert_eq!(container::history_breps(scan).count(), 1);
        fill_consumed(
            &cadmpeg_core::decode::test_support::service_decode_context(),
            scan,
            &mut model,
            &native,
            Some(name),
            &ConsumerContext {
                remaining_work: MAX_HISTORY_BODY_WORK,
                ..ConsumerContext::default()
            },
            &mut losses,
        )
        .unwrap();
    });
    (model, losses)
}

#[test]
fn initial_cut_snapshot_requires_one_input_body_and_an_unbuilt_component() {
    use crate::records::feature::scope::{DesignFeatureKind, DesignParameterScope};
    use cadmpeg_ir::features::extents::{
        ExtrudeDirection, ExtrudeExtent, ExtrudeSide, ExtrudeStart, LinearTermination,
    };
    use cadmpeg_ir::features::profiles::{PlanarProfileRef, ProfileRef};
    let bytes = carrier_bytes(RefWidth::Eight, true);
    let arena = DecodeArena::new();
    let (ctx, _) =
        DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::default()).unwrap();
    let mut allowance = u64::MAX;
    let history = super::super::decode(
        &ctx,
        &bytes,
        "Breps.BlobParts/BREP.a.smbh",
        RefWidth::Eight,
        &mut allowance,
        &WorkBudget::new(1 << 20),
    )
    .unwrap()
    .unwrap();
    let mut scope = DesignParameterScope::empty(
        "f3d:Design/BulkStream.dat:design-parameter-scope#1",
        DesignFeatureKind::Extrude,
        1,
    );
    scope
        .try_edit(|draft| {
            draft.history_state_id = Some(13);
            draft.previous_history_state_id = Some(12);
            draft.layout_fixture_tail();
        })
        .unwrap();
    let mut cut = feature("cut");
    cut.native_ref = Some(scope.id.clone());
    cut.product_definition =
        Some(cadmpeg_ir::ids::ProductDefinitionId::mint("test:model:component#target").unwrap());
    cut.ordinal = 1;
    let output = cadmpeg_ir::ids::BodyId::mint("f3d:brep/a.smbh/brep:entity#1").unwrap();
    cut.evaluation = FeatureEvaluation::new(
        FeatureDefinition::Operation(FeatureOperation::Extrude {
            profile: ProfileRef::Planar(PlanarProfileRef::Native("native:profile".into())),
            direction: ExtrudeDirection::ProfileNormal {},
            start: ExtrudeStart::ProfilePlane {},
            extent: ExtrudeExtent::OneSided {
                side: ExtrudeSide {
                    termination: LinearTermination::Blind {
                        length: cadmpeg_ir::scalar::NonZeroLength::new(1.0).unwrap(),
                    },
                    draft: None,
                },
            },
            op: BooleanOp::Cut,
            solid: Some(true),
            face_maker: None,
            inner_wire_taper: None,
            length_along_profile_normal: None,
            allow_multi_profile_faces: None,
        }),
        DistinctMembers::try_from(vec![output.clone()]).unwrap(),
    );
    let scopes = [scope];
    let histories = [history];
    let bindings = HashMap::new();
    let mut earlier = feature("earlier");
    earlier.product_definition = cut.product_definition.clone();
    for (built, same_product, suppressed, expected) in [
        (false, true, false, true),
        (true, true, false, false),
        (true, false, false, true),
        (true, true, true, true),
    ] {
        earlier.product_definition = if same_product {
            cut.product_definition.clone()
        } else {
            Some(cadmpeg_ir::ids::ProductDefinitionId::mint("test:model:component#other").unwrap())
        };
        earlier.suppressed = Some(suppressed);
        earlier.evaluation.set_outputs(if built {
            DistinctMembers::try_from(vec![output.clone()]).unwrap()
        } else {
            DistinctMembers::default()
        });
        let features = [earlier.clone(), cut.clone()];
        let inputs = Inputs {
            features: &features,
            scopes: &scopes,
            histories: &histories,
            bindings: &bindings,
            primary_brep: None,
            groups: &[],
            operands: &[],
            recipes: &[],
        };
        let mut losses = Vec::new();
        let wanted = initial_cut_body(&features[1], inputs, &mut losses);
        assert_eq!(losses.len(), usize::from(!expected));
        if !expected {
            assert_eq!(
                losses[0].code,
                F3dLossCode::FeatureHistoryBodyConsumedWithheld.kind()
            );
            assert!(losses[0].message.contains(cut.id.as_str()));
            assert!(losses[0].message.contains("previous state 12"));
        }
        assert_eq!(wanted.is_some(), expected);
        let (model, filled_losses) =
            consumed_fixture(&bytes, features.to_vec(), &scopes[0], &histories[0]);
        assert_eq!(filled_losses, losses);
        assert_eq!(model.history_bodies.len(), usize::from(expected));
        if expected {
            let body = &model.history_bodies[0];
            assert_eq!(body.consumed_by.as_ref(), Some(&cut.id));
            assert_eq!(body.state.identifier, 12);
            let SurfaceGeometry::Solved(SolvedSurfaceGeometry::Sphere(sphere)) =
                &body.topology.surfaces[0].geometry
            else {
                panic!("sphere")
            };
            assert_eq!(
                sphere.center().get(),
                cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0)
            );
        }
        if let Some((index, want)) = wanted {
            assert_eq!(
                (index, want.state, want.slot, want.consumed),
                (0, 12, 1, true)
            );
        }
    }
    for (outputs, withheld) in [
        (
            vec![
                output.clone(),
                cadmpeg_ir::ids::BodyId::mint("f3d:brep/a.smbh/brep:entity#10").unwrap(),
            ],
            true,
        ),
        (
            vec![cadmpeg_ir::ids::BodyId::mint("f3d:brep/a.smbh/brep:entity#10").unwrap()],
            false,
        ),
        (
            vec![cadmpeg_ir::ids::BodyId::mint("f3d:brep/b/brep:entity#1").unwrap()],
            false,
        ),
    ] {
        let mut candidate = cut.clone();
        candidate
            .evaluation
            .set_outputs(DistinctMembers::try_from(outputs).unwrap());
        let inputs = Inputs {
            features: std::slice::from_ref(&candidate),
            scopes: &scopes,
            histories: &histories,
            bindings: &bindings,
            primary_brep: None,
            groups: &[],
            operands: &[],
            recipes: &[],
        };
        let mut losses = Vec::new();
        assert!(initial_cut_body(&candidate, inputs, &mut losses).is_none());
        assert_eq!(losses.len(), usize::from(withheld));
        let (model, filled_losses) =
            consumed_fixture(&bytes, vec![candidate], &scopes[0], &histories[0]);
        assert!(model.history_bodies.is_empty());
        assert_eq!(filled_losses, losses);
        if withheld {
            assert_eq!(
                losses[0].code,
                F3dLossCode::FeatureHistoryBodyConsumedWithheld.kind()
            );
            assert!(losses[0].message.contains(cut.id.as_str()));
            assert!(losses[0].message.contains("previous state 12"));
        }
    }
    let mut join = earlier.clone();
    join.product_definition = cut.product_definition.clone();
    join.suppressed = Some(false);
    let mut definition = cut.evaluation.definition().clone();
    let FeatureDefinition::Operation(FeatureOperation::Extrude { op, .. }) = &mut definition else {
        panic!("Extrude")
    };
    *op = BooleanOp::Join;
    join.evaluation = FeatureEvaluation::new(
        definition,
        DistinctMembers::try_from(vec![output.clone()]).unwrap(),
    );
    let (model, losses) =
        consumed_fixture(&bytes, vec![join, cut.clone()], &scopes[0], &histories[0]);
    assert!(model.history_bodies.is_empty());
    assert_eq!(losses.len(), 1);
    assert_eq!(
        losses[0].code,
        F3dLossCode::FeatureHistoryBodyConsumedWithheld.kind()
    );
    let record = histories[0]
        .states()
        .iter()
        .flat_map(|state| &state.records)
        .find(|record| record.revision_id.is_some())
        .unwrap();
    let mut malformed = bytes.clone();
    malformed[usize::try_from(record.byte_offset).unwrap()] ^= 1;
    let (model, losses) =
        consumed_fixture(&malformed, vec![cut.clone()], &scopes[0], &histories[0]);
    assert!(model.history_bodies.is_empty());
    assert_eq!(losses.len(), 1);
    assert_eq!(
        losses[0].code,
        F3dLossCode::FeatureHistoryBodyUndecoded.kind()
    );
    assert!(losses[0].message.contains("incomplete record archive"));
    let mut unowned = cut.clone();
    unowned.product_definition = None;
    let inputs = Inputs {
        features: std::slice::from_ref(&unowned),
        scopes: &scopes,
        histories: &histories,
        bindings: &bindings,
        primary_brep: None,
        groups: &[],
        operands: &[],
        recipes: &[],
    };
    assert!(initial_cut_body(&unowned, inputs, &mut Vec::new()).is_none());
    let previous = histories[0]
        .states()
        .iter()
        .find(|state| state.state_id == 12)
        .unwrap();
    let mut topology = histories[0].topology(previous).unwrap().clone();
    topology.bodies.push(10);
    let ambiguous = crate::test_support::history::history(
        histories[0].id.clone(),
        0,
        None,
        false,
        histories[0]
            .states()
            .iter()
            .map(|state| (state.clone(), Some(topology.clone()), Vec::new()))
            .collect(),
    );
    let inputs = Inputs {
        features: std::slice::from_ref(&cut),
        scopes: &scopes,
        histories: std::slice::from_ref(&ambiguous),
        bindings: &bindings,
        primary_brep: None,
        groups: &[],
        operands: &[],
        recipes: &[],
    };
    assert!(initial_cut_body(&cut, inputs, &mut Vec::new()).is_none());
}

#[test]
fn a_captured_derived_result_preserves_the_current_body() {
    let bytes = carrier_bytes(RefWidth::Eight, true);
    let arena = DecodeArena::new();
    let (ctx, _) =
        DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::default()).unwrap();
    let mut allowance = u64::MAX;
    let history = super::super::decode(
        &ctx,
        &bytes,
        "test",
        RefWidth::Eight,
        &mut allowance,
        &WorkBudget::new(1 << 20),
    )
    .unwrap()
    .unwrap();
    let feature = feature("derived");
    let mut bodies = Vec::new();
    let mut losses = Vec::new();
    fill_history(
        &ctx,
        &bytes,
        (&history, None),
        &[Want {
            origin: &feature,
            producer: Some(&feature),
            consumer: None,
            identity: None,
            state: 13,
            slot: 1,
            capture_current: true,
            consumed: false,
        }],
        &mut bodies,
        &WorkBudget::new(MAX_HISTORY_BODY_WORK),
        &mut losses,
    )
    .unwrap();
    assert!(losses.is_empty(), "{losses:?}");
    assert_eq!(bodies.len(), 1);
    assert_eq!(bodies[0].state.identifier, 13);
    assert_eq!(bodies[0].produced_by.as_ref(), Some(&feature.id));
    assert_eq!(bodies[0].topology.faces.len(), 1);
    assert_eq!(bodies[0].topology.surfaces.len(), 1);
    let SurfaceGeometry::Solved(SolvedSurfaceGeometry::Sphere(sphere)) =
        &bodies[0].topology.surfaces[0].geometry
    else {
        panic!("sphere")
    };
    assert_eq!(
        sphere.center().get(),
        cadmpeg_ir::math::Point3::new(10.0 * 10.0, 0.0, 0.0)
    );
}

#[test]
fn history_body_caps_report_the_skipped_body() {
    for (count, work, expected) in [
        (1, 0, 0),
        (
            MAX_HISTORY_BODIES + 1,
            MAX_HISTORY_BODY_WORK,
            MAX_HISTORY_BODIES,
        ),
    ] {
        let (bodies, losses) = run(true, RefWidth::Eight, count, work);
        assert_eq!(bodies.len(), expected);
        assert_eq!(losses.len(), 1);
        assert_eq!(
            losses[0].code,
            F3dLossCode::FeatureHistoryBodyBounded.note("bounded").code
        );
    }
}

#[test]
fn the_output_blob_disambiguates_equal_state_identifiers() {
    let histories = ["a", "b"].map(|source| {
        AsmHistory::new(
            format!("f3d:Breps.BlobParts/BREP.{source}:asm-history#0"),
            0,
            None,
            false,
            Vec::new(),
        )
    });
    assert_eq!(
        output_history(["f3d:brep/a/entity#1"].into_iter(), &histories, None),
        Some(histories[0].id.as_str())
    );
    assert_eq!(
        output_history(
            ["f3d:brep:entity#1"].into_iter(),
            &histories,
            Some("Breps.BlobParts/BREP.b")
        ),
        Some(histories[1].id.as_str())
    );
    assert_eq!(
        output_history(
            ["f3d:brep/a/entity#1", "f3d:brep/b/entity#2"].into_iter(),
            &histories,
            None
        ),
        None
    );
}

#[test]
fn subtype_references_follow_archive_byte_order_across_revisions() {
    use cadmpeg_asm::sab::{Record, Token};
    fn definition(index: usize, offset: usize, z: f64) -> Record {
        let mut tokens = vec![
            Token::SubtypeOpen,
            Token::Ident("exact_spl_sur".into()),
            Token::Ident("nubs".into()),
            Token::Long(1),
            Token::Long(1),
        ];
        tokens.extend([
            Token::Enum(0),
            Token::Enum(0),
            Token::Enum(0),
            Token::Enum(0),
            Token::Long(2),
            Token::Long(2),
        ]);
        for _ in 0..2 {
            tokens.extend([
                Token::Double(0.0),
                Token::Long(1),
                Token::Double(1.0),
                Token::Long(1),
            ]);
        }
        for point in [[0.0, 0.0, z], [1.0, 0.0, z], [0.0, 1.0, z], [1.0, 1.0, z]] {
            tokens.extend(point.map(Token::Double));
        }
        tokens.push(Token::SubtypeClose);
        Record {
            index,
            offset,
            len: 0,
            name: "spline-surface".into(),
            tokens: tokens.into(),
        }
    }
    let archive = HashMap::from([(20, definition(9, 100, 0.0)), (10, definition(1, 200, 1.0))]);
    let table = archive_subtypes(&archive, None);
    let ctx = cadmpeg_core::decode::test_support::service_decode_context();
    for (reference, expected_z) in [(0, 0.0), (1, 10.0)] {
        let tokens = [
            Token::SubtypeOpen,
            Token::Ident("ref".into()),
            Token::Long(reference),
            Token::SubtypeClose,
        ];
        let surface = cadmpeg_asm::nurbs::core::surface_cache_resolving_refs(&ctx, &tokens, &table)
            .unwrap()
            .unwrap();
        assert_eq!(surface.pole(0, 0).unwrap().get().z, expected_z);
    }
}

fn cube_candidate() -> HistoryBody {
    let geometry = cadmpeg_ir::examples::unit_cube().expect("cube").geometry;
    let ctx = cadmpeg_core::decode::test_support::service_decode_context();
    let Extract::Nest(mut topology) =
        extract(&ctx, &geometry, geometry.bodies[0].id.as_str()).expect("extract cube")
    else {
        panic!("closed cube")
    };
    sort_by_identity(&mut topology);
    HistoryBody {
        id: HistoryBodyId::mint("test:history-body:nest#cube").expect("identity"),
        produced_by: Some(feature("base").id),
        consumed_by: None,
        state_of: None,
        state: HistoryStateRef {
            ordinal: 0,
            identifier: 1,
        },
        topology,
    }
}

#[test]
fn admission_preserves_a_valid_nest_and_its_producer() {
    let ctx = cadmpeg_core::decode::test_support::service_decode_context();
    let candidate = cube_candidate();
    let expected = candidate.clone();
    assert_eq!(
        admit(&ctx, candidate, &feature("base")).expect("admission"),
        Some(expected)
    );
}

/// A finding outside the topology checks does not drop a usable nest: its
/// records out of identity order are an `ArenaOrder` Error of the document,
/// and the nest is admitted.
#[test]
fn admission_ignores_findings_outside_the_topology_checks() {
    let ctx = cadmpeg_core::decode::test_support::service_decode_context();
    let mut candidate = cube_candidate();
    candidate.topology.faces.reverse();
    let mut document = CadIr::empty();
    document.model.features.push(feature("base"));
    document.model.history_bodies.push(candidate.clone());
    let report = cadmpeg_ir::validate_neutral(&document, Vec::new()).expect("validation runs");
    assert!(
        report.findings.iter().any(|finding| {
            finding.check == cadmpeg_ir::report::check::Check::ArenaOrder
                && finding.severity == cadmpeg_ir::report::Severity::Error
        }),
        "{:?}",
        report.findings
    );
    let expected = candidate.clone();
    assert_eq!(
        admit(&ctx, candidate, &feature("base")).expect("admission"),
        Some(expected)
    );
}

#[test]
fn admission_rejects_a_reference_closed_nest_with_a_radial_ring_crossing_edges() {
    let ctx = cadmpeg_core::decode::test_support::service_decode_context();
    let mut candidate = cube_candidate();
    let first_edge = candidate.topology.coedges[0].edge.clone();
    let other = candidate
        .topology
        .coedges
        .iter()
        .find(|coedge| coedge.edge != first_edge)
        .expect("another edge")
        .id
        .clone();
    candidate.topology.coedges[0].radial_next = other;
    assert!(!escapes(&ctx, &candidate.topology).expect("references"));
    assert!(admit(&ctx, candidate, &feature("base"))
        .expect("admission")
        .is_none());
}

#[test]
fn admission_rejects_a_reference_closed_nest_with_displaced_endpoints() {
    let ctx = cadmpeg_core::decode::test_support::service_decode_context();
    let mut candidate = cube_candidate();
    candidate.topology.points[0].set_position(
        cadmpeg_ir::features::geometry::FinitePoint3::new(cadmpeg_ir::math::Point3::new(
            10.0, 10.0, 10.0,
        ))
        .expect("finite position"),
    );
    assert!(!escapes(&ctx, &candidate.topology).expect("references"));
    assert!(admit(&ctx, candidate, &feature("base"))
        .expect("admission")
        .is_none());
}

#[test]
fn a_wrapped_surface_preserves_the_base_feature_geometry() {
    for width in [RefWidth::Four, RefWidth::Eight] {
        let mut bytes = Vec::new();
        put_header(&mut bytes, width);
        put_plain(&mut bytes, width, "asmheader", &[]);
        put_sphere_solid(&mut bytes, width, 1, 7, -1, [10.0, 0.0, 0.0]);
        put_state(&mut bytes, width, (13, -1, 1, 0), &[(7, 1), (6, 5)]);
        put_name(&mut bytes, "End-of-ASM-History-Section");
        put_surface(&mut bytes, width, [0.0, 0.0, 0.0]);
        put_plain(
            &mut bytes,
            width,
            "body",
            &[
                (REF, -1),
                (INT, 7),
                (REF, -1),
                (REF, 2),
                (REF, -1),
                (REF, -1),
            ],
        );
        put_state(
            &mut bytes,
            width,
            (12, 0, -1, 1),
            &[(-1, 1), (-1, 2), (-1, 3), (-1, 4), (-1, 5)],
        );
        let (bodies, losses) = run_bytes(&bytes, width, 1, MAX_HISTORY_BODY_WORK);
        assert!(losses.is_empty(), "{losses:?}");
        let [body] = bodies.as_slice() else {
            panic!("one history body")
        };
        assert_eq!(body.produced_by, Some(feature("base").id));
        assert_eq!(body.state.identifier, 12);
        let SurfaceGeometry::Solved(SolvedSurfaceGeometry::Sphere(sphere)) =
            &body.topology.surfaces[0].geometry
        else {
            panic!("sphere")
        };
        assert_eq!(
            sphere.center().get(),
            cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0)
        );
    }
}

/// The final state deletes the body of slots 6 through 10 that the state
/// before it holds.
fn deleted_bytes(width: RefWidth) -> Vec<u8> {
    let mut out = Vec::new();
    put_header(&mut out, width);
    put_plain(&mut out, width, "asmheader", &[]);
    put_sphere_solid(&mut out, width, 1, 7, -1, [10.0, 0.0, 0.0]);
    let deleted = (6..=10).map(|slot| (slot, -1)).collect::<Vec<_>>();
    put_state(&mut out, width, (13, -1, 1, 0), &deleted);
    put_sphere_solid(&mut out, width, 6, 8, -1, [10.0, 0.0, 0.0]);
    put_state(
        &mut out,
        width,
        (12, 0, -1, 1),
        &[(-1, 1), (-1, 2), (-1, 3), (-1, 4), (-1, 5)],
    );
    out
}

#[test]
fn a_consumed_body_the_final_state_lacks_is_a_history_body() {
    for width in [RefWidth::Four, RefWidth::Eight] {
        let bytes = deleted_bytes(width);
        let (bodies, losses) = run_roles(&bytes, width, 1, MAX_HISTORY_BODY_WORK, (false, true), 6);
        assert!(losses.is_empty(), "{losses:?}");
        let [body] = bodies.as_slice() else {
            panic!("one history body")
        };
        assert_eq!(body.produced_by, None);
        assert_eq!(body.consumed_by, Some(feature("base").id));
        assert_eq!(body.state.identifier, 12);
        // The body keeps the geometry of the state that holds it, in
        // millimetres from the stored centimetre length.
        let SurfaceGeometry::Solved(SolvedSurfaceGeometry::Sphere(sphere)) =
            &body.topology.surfaces[0].geometry
        else {
            panic!("sphere")
        };
        assert_eq!(
            sphere.center().get(),
            cadmpeg_ir::math::Point3::new(10.0 * 10.0, 0.0, 0.0)
        );
        let (both, _) = run_roles(&bytes, width, 1, MAX_HISTORY_BODY_WORK, (true, true), 6);
        let [both] = both.as_slice() else {
            panic!("one history body")
        };
        assert_eq!(both.produced_by, Some(feature("base").id));
        assert_eq!(both.consumed_by, Some(feature("base").id));
    }
}

#[test]
fn a_consumed_body_a_later_state_revises_is_a_history_body() {
    let (bodies, losses) = run_roles(
        &carrier_bytes(RefWidth::Eight, true),
        RefWidth::Eight,
        1,
        MAX_HISTORY_BODY_WORK,
        (false, true),
        1,
    );
    assert!(losses.is_empty(), "{losses:?}");
    let [body] = bodies.as_slice() else {
        panic!("one history body")
    };
    assert_eq!(body.consumed_by, Some(feature("base").id));
    assert_eq!(body.produced_by, None);
}

#[test]
fn a_consumed_body_no_later_state_changes_needs_no_history_body() {
    let (bodies, losses) = run_roles(
        &carrier_bytes(RefWidth::Eight, false),
        RefWidth::Eight,
        1,
        MAX_HISTORY_BODY_WORK,
        (false, true),
        1,
    );
    assert!(bodies.is_empty());
    assert!(losses.is_empty());
}

fn split_fixture() -> (Feature, DesignParameterScope, AsmHistory) {
    use crate::history_records::{AsmDeltaState, AsmHistoricalTopology};
    use crate::records::feature::scope::DesignFeatureKind;
    let mut scope = DesignParameterScope::empty(
        "f3d:Design/BulkStream.dat:design-parameter-scope#2",
        DesignFeatureKind::BaseFeature,
        1,
    );
    scope
        .try_edit(|draft| {
            draft.history_state_id = Some(2);
            draft.previous_history_state_id = Some(1);
            draft.layout_fixture_tail();
        })
        .unwrap();
    let mut feature = feature("split");
    feature.native_ref = Some(scope.id.clone());
    let prefix = crate::ids::feature_input_prefix(&feature.id, 1);
    let targets = BodySelection::historical(
        crate::ids::history_input_state_id(&prefix),
        vec![crate::ids::history_input_body_id(&prefix, 7)],
        "f3d:test:scope#split".to_owned(),
    )
    .unwrap();
    feature
        .evaluation
        .set_definition(FeatureDefinition::Operation(FeatureOperation::SplitBody {
            extend_tool: None,
            targets,
            tool: cadmpeg_ir::features::direct_edit::SplitBodyTool::Faces(
                cadmpeg_ir::features::selections::FaceSelection::Unresolved,
            ),
        }));
    let state = |state_id, node_index, next_ref| {
        (
            AsmDeltaState {
                id: format!("f3d:asm-delta-state#{state_id}"),
                parent: "f3d:asm-history#1".into(),
                byte_offset: 0,
                state_id,
                version_flag: 1,
                state_flag: 0,
                previous_ref: None,
                next_ref,
                node_index,
                partner_ref: None,
                owner_ref: 0,
                bulletin_boards: Vec::new(),
                records: Vec::new(),
                transition: None,
            },
            Some(AsmHistoricalTopology::default()),
            Vec::new(),
        )
    };
    let history = crate::test_support::history::history(
        "f3d:asm-history#1".into(),
        0,
        None,
        false,
        vec![state(2, 1, Some(0)), state(1, 0, None)],
    );
    (feature, scope, history)
}

#[test]
fn a_split_names_the_slots_of_its_input_state_in_the_history_that_holds_both_states() {
    let (feature, scope, history) = split_fixture();
    let histories = [history];
    let scopes = [scope];
    let none = HashMap::new();
    assert_eq!(
        split_targets(&feature, &scopes, &histories, &none),
        Some((0, 1, vec![7]))
    );
    let bound = HashMap::from([(scopes[0].id.clone(), histories[0].id.clone())]);
    assert_eq!(
        split_targets(&feature, &scopes, &histories, &bound),
        Some((0, 1, vec![7]))
    );
    let elsewhere = HashMap::from([(scopes[0].id.clone(), "f3d:asm-history#9".to_owned())]);
    assert_eq!(
        split_targets(&feature, &scopes, &histories, &elsewhere),
        None
    );
    // A Base Feature selects no body from an input state.
    let mut base = feature.clone();
    base.evaluation.set_definition(FeatureDefinition::Operation(
        FeatureOperation::BaseFeature {
            bodies: BodySelection::Unresolved,
        },
    ));
    assert_eq!(split_targets(&base, &scopes, &histories, &none), None);
}

fn link_attribute(body: i64, design_id: &str, design_reference: i64) -> SourceAttribute {
    use cadmpeg_ir::attributes::AttributeValue;
    SourceAttribute {
        id: format!("f3d:history:attribute#{body}")
            .try_into()
            .expect("valid identity"),
        target: cadmpeg_ir::references::InstanceTarget::new(AttributeTarget::Body(
            cadmpeg_ir::ids::BodyId::mint(format!("f3d:history:body#{body}"))
                .expect("identity grammar"),
        )),
        name: "ATTRIB_CUSTOM-attrib".into(),
        values: vec![
            Some(AttributeValue::String("generic_tag_attrib_def".into())),
            Some(AttributeValue::Integer(2)),
            Some(AttributeValue::Integer(2)),
            Some(AttributeValue::Integer(-1)),
            Some(AttributeValue::String("generic_tag_attrib_def ".into())),
            Some(AttributeValue::Integer(1)),
            Some(AttributeValue::Integer(3)),
            Some(AttributeValue::String(design_id.into())),
            Some(AttributeValue::Integer(design_reference)),
            Some(AttributeValue::Integer(0)),
        ],
        native_ref: None,
    }
}

fn identity(revised: &[i64]) -> BodyIdentity {
    BodyIdentity {
        design_id: "626".into(),
        selector: 1,
        revised: revised.to_vec(),
    }
}

#[test]
fn a_body_identity_names_the_one_revised_body_that_carries_the_design_link() {
    let brep = |attributes| cadmpeg_asm::brep::AsmBrep {
        attributes,
        ..cadmpeg_asm::brep::AsmBrep::default()
    };
    let linked = brep(vec![
        link_attribute(4, "626", 1),
        link_attribute(5, "627", 1),
        link_attribute(6, "626", 3),
    ]);
    assert_eq!(identity_slot(&linked, &identity(&[4, 9])).unwrap(), Some(4));
    assert_eq!(identity_slot(&linked, &identity(&[9])).unwrap(), None);
    let twice = brep(vec![
        link_attribute(4, "626", 1),
        link_attribute(5, "626", 1),
    ]);
    assert_eq!(identity_slot(&twice, &identity(&[4, 5])).unwrap(), None);
    assert_eq!(
        identity_slot(&brep(Vec::new()), &identity(&[4])).unwrap(),
        None
    );
}

#[test]
fn a_split_identity_that_matches_two_bodies_reports_the_undecoded_input() {
    let width = RefWidth::Eight;
    let mut bytes = Vec::new();
    put_header(&mut bytes, width);
    bytes[39] = 1;
    put_plain(&mut bytes, width, "asmheader", &[]);
    for slot in [1, 2] {
        put_plain(
            &mut bytes,
            width,
            "body",
            &[
                (REF, slot + 2),
                (INT, slot),
                (REF, -1),
                (REF, -1),
                (REF, -1),
                (REF, -1),
            ],
        );
    }
    for _ in 0..2 {
        put_fields(&mut bytes, width, "ATTRIB_CUSTOM-attrib", &[(REF, -1)]);
        for value in link_attribute(1, "626", 1).values.into_iter().flatten() {
            match value {
                cadmpeg_ir::attributes::AttributeValue::String(value) => {
                    bytes.extend_from_slice(&[
                        0x07,
                        u8::try_from(value.len()).expect("short fixture string"),
                    ]);
                    bytes.extend_from_slice(value.as_bytes());
                }
                cadmpeg_ir::attributes::AttributeValue::Integer(value) => {
                    put_ref(&mut bytes, width, INT, value);
                }
                _ => panic!("identity fixture value"),
            }
        }
        bytes.push(0x11);
    }
    put_state(&mut bytes, width, (13, -1, 1, 0), &[]);
    put_state(
        &mut bytes,
        width,
        (12, 0, -1, 1),
        &[(-1, 1), (-1, 2), (-1, 3), (-1, 4)],
    );
    put_name(&mut bytes, "End-of-ASM-data");
    let arena = DecodeArena::new();
    let (ctx, _) =
        DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::default()).unwrap();
    let mut allowance = u64::MAX;
    let history = super::super::decode(
        &ctx,
        &bytes,
        "test",
        width,
        &mut allowance,
        &WorkBudget::new(1 << 20),
    )
    .unwrap()
    .unwrap();
    let archive = super::super::decode_record_archive(&ctx, history.states(), &bytes, width)
        .unwrap()
        .unwrap();
    let versions = (0..5).map(|slot| (slot, slot)).collect();
    let table =
        cadmpeg_asm::history::materialize_record_table(&versions, |id| archive.get(&id)).unwrap();
    let header = cadmpeg_asm::asm_header::parse(&bytes).unwrap();
    let subtypes = archive_subtypes(&archive, header.metadata.save_format_version);
    let brep = cadmpeg_asm::brep::decode_with_table(
        &ctx,
        &table,
        &bytes,
        (Some(&header.metadata), &subtypes),
        "f3d:history",
        crate::ids::ID_FORMAT,
        cadmpeg_asm::brep::DecodePurpose::Model,
    )
    .unwrap();
    let identity = identity(&[1, 2]);
    assert_eq!(brep.bodies.len(), 2);
    assert_eq!(identity_slot(&brep, &identity).unwrap(), None);
    for slot in [1, 2] {
        let single = cadmpeg_asm::brep::AsmBrep {
            attributes: brep
                .attributes
                .iter()
                .filter(|attribute| {
                    matches!(&attribute.target.entity, AttributeTarget::Body(body)
                        if super::super::body_closures::stable_ref(body.as_str()) == Some(slot))
                })
                .cloned()
                .collect(),
            ..cadmpeg_asm::brep::AsmBrep::default()
        };
        assert_eq!(identity_slot(&single, &identity).unwrap(), Some(slot));
    }
    let feature = feature("split");
    let mut bodies = Vec::new();
    let mut losses = Vec::new();
    fill_history(
        &ctx,
        &bytes,
        (&history, None),
        &[Want {
            origin: &feature,
            producer: None,
            consumer: Some(&feature),
            state: 12,
            slot: -1,
            identity: Some(identity),
            capture_current: false,
            consumed: true,
        }],
        &mut bodies,
        &WorkBudget::new(MAX_HISTORY_BODY_WORK),
        &mut losses,
    )
    .unwrap();
    assert!(bodies.is_empty());
    assert_eq!(
        losses,
        vec![F3dLossCode::FeatureHistoryBodyUndecoded
            .note("f3d:model:feature#split state 12 has an unresolved body identity")]
    );
}

fn nest(id: &str, produced_by: Option<&str>, consumed_by: Option<&str>, state: i64) -> HistoryBody {
    HistoryBody {
        produced_by: produced_by.map(|native| feature(native).id),
        consumed_by: consumed_by.map(|native| feature(native).id),
        state_of: None,
        state: HistoryStateRef {
            ordinal: u32::try_from(state).expect("small"),
            identifier: state,
        },
        id: HistoryBodyId::mint(format!("test:history-body:nest#{id}")).expect("identity"),
        ..cube_candidate()
    }
}

#[test]
fn a_producer_of_equal_records_joins_the_nest_of_the_consumer() {
    let arena = DecodeArena::new();
    let ctx = DecodeContext::new(
        &arena,
        &DecodePolicy::default(),
        cadmpeg_core::decode::DecodeDepth::Full,
    );
    let allowance = ctx.retained_bytes_available();
    let records = *cube_candidate().topology;
    let name = "source body name";
    let built = |mut body: HistoryBody, records: &BodyTopology| {
        body.topology.body.name = Some(name.into());
        Built {
            body,
            records: Some(records.clone()),
        }
    };
    let mut bodies = Vec::new();
    let mut joinable = Vec::new();
    place(
        &ctx,
        &mut bodies,
        &mut joinable,
        built(nest("a", None, Some("split"), 4), &records),
        7,
        (true, None),
    )
    .unwrap();
    assert_eq!((bodies.len(), joinable.len()), (1, 1));
    // A producer whose records differ keeps a nest of its own.
    let mut changed = records.clone();
    changed.points.clear();
    place(
        &ctx,
        &mut bodies,
        &mut joinable,
        built(nest("b", Some("base"), None, 1), &changed),
        7,
        (false, Some(0)),
    )
    .unwrap();
    assert_eq!((bodies.len(), joinable.len()), (2, 1));
    assert_eq!(bodies[0].produced_by, None);
    // A producer of equal records joins the consumer nest at its own state.
    place(
        &ctx,
        &mut bodies,
        &mut joinable,
        built(nest("c", Some("base"), None, 1), &records),
        7,
        (false, Some(0)),
    )
    .unwrap();
    assert_eq!((bodies.len(), joinable.len()), (2, 0));
    assert_eq!(bodies[0].produced_by, Some(feature("base").id));
    assert_eq!(bodies[0].consumed_by, Some(feature("split").id));
    assert_eq!(bodies[0].state.identifier, 1);
    assert_eq!(
        allowance - ctx.retained_bytes_available(),
        2 * cadmpeg_core::decode::u64_from_index(name.len())
    );
}

fn leak<T>(value: T) -> &'static T {
    Box::leak(Box::new(value))
}

fn identity_split_fixture() -> (Feature, Inputs<'static>) {
    use crate::history_records::{AsmDeltaState, AsmEntityVersion, AsmHistoricalTopology};
    use crate::records::feature::scope::DesignFeatureKind;
    use crate::records::identity::{Located, RecordedValue};
    use crate::records::recipes::{ConstructionRecipeDesign, ConstructionRecipeSelector};
    use crate::records::topology::body_recipe::{
        DesignBodyRecipeOperandDraft, DesignBodyRecipeReference, DesignOperandOwner,
    };
    use crate::records::topology::construction::{
        DesignConstructionOperandGroupDraft, DesignConstructionOperandGroupFrame,
        DesignConstructionOperandGroupFrameDraft,
    };
    let mut scope = DesignParameterScope::empty(
        "f3d:Design/BulkStream.dat:design-parameter-scope#10",
        DesignFeatureKind::Split,
        10,
    );
    scope
        .try_edit(|draft| {
            draft.history_state_id = Some(2);
            draft.layout_fixture_tail();
        })
        .unwrap();
    let mut feature = feature("split");
    feature.native_ref = Some(scope.id.clone());
    let group_id = "f3d:Design/BulkStream.dat:design-construction-operand-group#100";
    feature
        .evaluation
        .set_definition(FeatureDefinition::Operation(FeatureOperation::SplitBody {
            extend_tool: None,
            targets: BodySelection::Native(group_id.into()),
            tool: cadmpeg_ir::features::direct_edit::SplitBodyTool::Faces(
                cadmpeg_ir::features::selections::FaceSelection::Unresolved,
            ),
        }));
    let group = DesignConstructionOperandGroup::try_from(DesignConstructionOperandGroupDraft {
        id: group_id.into(),
        scope_record_index: 10,
        scope_reference_ordinal: 2,
        record_index: 100,
        byte_offset: 0,
        class_tag: crate::records::references::DesignClassTag::try_from("280".to_owned()).unwrap(),
        members: vec![Located {
            value: 200,
            offset: 0,
        }],
        lost_edge_references: Vec::new(),
        frame: DesignConstructionOperandGroupFrame::try_from(
            DesignConstructionOperandGroupFrameDraft {
                member_count_offset: 0,
                auxiliary_records: Vec::new(),
                auxiliary_paths: Vec::new(),
                trailing_records: Vec::new(),
                trailing_transforms: Vec::new(),
                trailing_dual_transforms: Vec::new(),
                trailing_flags: Vec::new(),
                opaque_index: 88,
                opaque_index_offset: 18,
                opaque_scalar: 0.0,
                opaque_scalar_offset: 22,
                variant: false,
                closing_flag: false,
            },
        )
        .unwrap(),
        operand_role: crate::records::topology::construction::DesignConstructionOperandRole::Other(
            DesignOperandRole::BODIES_A,
        ),
        role_offset: 0,
        paired_class_tag: crate::records::references::DesignClassTag::try_from("267".to_owned())
            .unwrap(),
        paired_byte_offset: 0,
    })
    .unwrap();
    let operand = DesignBodyRecipeOperand::try_new(DesignBodyRecipeOperandDraft {
        id: "f3d:Design/BulkStream.dat:design-body-recipe-operand#200".into(),
        scope_record_index: 10,
        owner: DesignOperandOwner::Group {
            group_record_index: 100,
            group_member_ordinal: 0,
        },
        record_index: 200,
        byte_offset: 0,
        class_tag: crate::records::references::DesignClassTag::try_from("476".to_owned()).unwrap(),
        asset_id: crate::records::mesh::DesignRelaxedGuidText::try_from(
            "0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d".to_owned(),
        )
        .unwrap(),
        asset_id_offset: 56,
        context_id: crate::records::mesh::DesignRelaxedGuidText::try_from(
            "1b2c3d4e-5f6a-4b7c-8d9e-0f1a2b3c4d5e".to_owned(),
        )
        .unwrap(),
        context_id_offset: 132,
        selector_tail: None,
        references: vec![DesignBodyRecipeReference {
            design_reference: 626,
            design_reference_offset: 25,
            form: 3,
            form_offset: 33,
            candidate_faces: Vec::new(),
            preceding_candidate_faces: Vec::new(),
            preceding_body_slots: Vec::new(),
        }],
        nested_record_index: 203,
        nested_record_index_offset: 38,
        recipe_id: "f3d:Design/BulkStream.dat:construction-recipe#202".into(),
        resolved_face_slot: None,
        resolved_body_state_id: None,
        resolved_body_slot: None,
        resolved_body_face_slots: Vec::new(),
        next_record_index: 204,
        next_byte_offset: 256,
    })
    .unwrap();
    let recipe = ConstructionRecipe {
        id: operand.recipe_id.clone(),
        byte_offset: 0,
        kind: ConstructionRecipeKind::Body,
        design: Some(ConstructionRecipeDesign {
            id: RecordedValue {
                value: "626".into(),
                offset: 0,
            },
            selector: Some(ConstructionRecipeSelector {
                value: 1,
                byte_offset: 0,
            }),
        }),
        recipe_index: 0,
        record_index: Some(RecordedValue {
            value: 0,
            offset: 0,
        }),
    };
    // The Split state revises the body of slot 7 and inserts the body of slot 8.
    let state = |state_id, node_index, next_ref, bodies: Vec<i64>, revision: i64| {
        (
            AsmDeltaState {
                id: format!("f3d:asm-delta-state#{state_id}"),
                parent: "f3d:asm-history#1".into(),
                byte_offset: 0,
                state_id,
                version_flag: 1,
                state_flag: 0,
                previous_ref: None,
                next_ref,
                node_index,
                partner_ref: None,
                owner_ref: 0,
                bulletin_boards: Vec::new(),
                records: Vec::new(),
                transition: None,
            },
            Some(AsmHistoricalTopology {
                bodies: bodies.clone(),
                ..AsmHistoricalTopology::default()
            }),
            bodies
                .into_iter()
                .map(|entity_ref| AsmEntityVersion {
                    entity_ref,
                    record_ref: 100 * revision + entity_ref,
                })
                .collect::<Vec<_>>(),
        )
    };
    let history = crate::test_support::history::history(
        "f3d:asm-history#1".into(),
        0,
        None,
        false,
        vec![
            state(2, 1, Some(0), vec![7, 8], 2),
            state(1, 0, None, vec![7], 1),
        ],
    );
    (
        feature,
        Inputs {
            features: &[],
            scopes: std::slice::from_ref(leak(scope)),
            histories: std::slice::from_ref(leak(history)),
            bindings: leak(HashMap::new()),
            primary_brep: None,
            groups: std::slice::from_ref(leak(group)),
            operands: std::slice::from_ref(leak(operand)),
            recipes: std::slice::from_ref(leak(recipe)),
        },
    )
}

#[test]
fn a_native_split_target_names_its_body_by_the_recipe_identity() {
    let (feature, inputs) = identity_split_fixture();
    let (index, state, found) = split_identity_target(&feature, inputs).expect("identity target");
    assert_eq!((index, state), (0, 1));
    assert_eq!(
        found,
        BodyIdentity {
            design_id: "626".into(),
            selector: 1,
            revised: vec![7],
        }
    );
    // A Split that selects bodies of its input state needs no identity.
    let mut historical = feature.clone();
    let prefix = crate::ids::feature_input_prefix(&historical.id, 1);
    historical
        .evaluation
        .set_definition(FeatureDefinition::Operation(FeatureOperation::SplitBody {
            extend_tool: None,
            targets: BodySelection::historical(
                crate::ids::history_input_state_id(&prefix),
                vec![crate::ids::history_input_body_id(&prefix, 7)],
                "f3d:test:scope#split".to_owned(),
            )
            .unwrap(),
            tool: cadmpeg_ir::features::direct_edit::SplitBodyTool::Faces(
                cadmpeg_ir::features::selections::FaceSelection::Unresolved,
            ),
        }));
    assert!(split_identity_target(&historical, inputs).is_none());
    // A recipe without a selector names no body.
    let mut bare = inputs.recipes[0].clone();
    bare.design.as_mut().unwrap().selector = None;
    let bare = Inputs {
        recipes: std::slice::from_ref(Box::leak(Box::new(bare))),
        ..inputs
    };
    assert!(split_identity_target(&feature, bare).is_none());
}
