// SPDX-License-Identifier: Apache-2.0
use super::*;
use cadmpeg_asm::kernel_header::RefWidth;
use cadmpeg_core::decode::{DecodeArena, DecodePolicy};
use cadmpeg_ir::document::Model;
use cadmpeg_ir::features::{
    BodySelection, DistinctMembers, FeatureContent, FeatureDefinition, FeatureEvaluation,
    FeatureId, FeatureOperation,
};

const EPS_CLOSED_SAMPLE: f64 = 1.0e-12;

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
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(bytes, &arena, &DecodePolicy::default()).unwrap();
    let mut allowance = u64::MAX;
    let history = super::super::decode(
        &ctx,
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
            feature: &feature,
            state: 12,
            slot: 1,
            capture_current: false,
            consumed: false,
            input: None,
            bindings: RefCell::default(),
        })
        .collect::<Vec<_>>();
    let mut bodies = Vec::new();
    let mut losses = Vec::new();
    fill_history(
        &ctx,
        bytes,
        &history,
        &wants,
        &mut bodies,
        &WorkBudget::new(work),
        &mut losses,
    )
    .unwrap();
    (bodies, losses)
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
        cadmpeg_asm::brep::transfer::transfer_into_ir(&ctx, &mut scratch, "f3d", brep).unwrap();
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
                &history,
                &[Want {
                    feature: &cut,
                    state: 12,
                    slot: 1,
                    consumed: true,
                    input: None,
                    bindings: RefCell::default(),
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
    let mut ir = CadIr::empty();
    ir.model.features = features;
    let mut losses = Vec::new();
    crate::test_support::zip_test::with_scan(&archive, |scan| {
        assert_eq!(container::history_breps(scan).count(), 1);
        fill_consumed(
            &cadmpeg_core::decode::test_support::service_decode_context(),
            scan,
            &mut ir,
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
    (ir.model, losses)
}

#[test]
fn initial_cut_snapshot_requires_one_input_body_and_an_unbuilt_component() {
    use crate::records::feature::scope::{DesignFeatureKind, DesignParameterScope};
    use cadmpeg_ir::features::{
        ExtrudeDirection, ExtrudeExtent, ExtrudeSide, ExtrudeStart, LinearTermination,
        PlanarProfileRef, ProfileRef,
    };
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
        &history,
        &[Want {
            feature: &feature,
            state: 13,
            slot: 1,
            capture_current: true,
            consumed: false,
            input: None,
            bindings: RefCell::default(),
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
        cadmpeg_ir::features::FinitePoint3::new(cadmpeg_ir::math::Point3::new(10.0, 10.0, 10.0))
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

#[test]
fn input_edge_bindings_scope_repeated_slots_and_refuse_sample_identity() {
    use cadmpeg_ir::features::{
        FeatureInputTopology, FinitePoint3, HistoricalEdgeGeometry, HistoricalEdgePath,
    };
    let first = feature("first");
    let second = feature("second");
    let cube = cadmpeg_ir::examples::unit_cube().unwrap().geometry;
    let mut nest = BodyTopology::new(cube.bodies[0].clone());
    nest.edges = cube.edges;
    nest.curves = cube.curves;
    nest.faces = cube.faces;
    let input = |owner: &Feature, state, slot| {
        let prefix = crate::ids::feature_input_prefix(&owner.id, state);
        let geometry = HistoricalEdgeGeometry::new(
            crate::ids::history_input_edge_id(&prefix, slot),
            FinitePoint3::ZERO,
        );
        FeatureInputTopology {
            id: crate::ids::history_input_state_id(&prefix),
            input_of: owner.id.clone(),
            bodies: DistinctMembers::default(),
            faces: DistinctMembers::default(),
            edges: DistinctMembers::try_from(vec![geometry.edge.clone()]).unwrap(),
            vertices: DistinctMembers::default(),
            edge_geometry: vec![geometry],
            native_ref: None,
        }
    };
    for (owner, state, nest_index) in [(&first, 12, 0), (&second, 12, 1), (&first, 13, 2)] {
        let input = input(owner, state, 0);
        let want = Want {
            feature: owner,
            state,
            slot: 0,
            capture_current: false,
            consumed: true,
            input: Some(&input),
            bindings: RefCell::default(),
        };
        let bindings = input_bindings(&want, &nest, nest_index);
        assert_eq!(bindings.len(), 1);
        assert_eq!(bindings[0].0, input.edge_geometry[0].edge);
        assert_eq!(
            bindings[0].1.path,
            HistoricalEdgePath::Edge {
                edge: nest.edges[0].id.clone()
            }
        );
        assert_eq!(
            bindings[0].1.history_body.as_str(),
            format!("f3d:history-body:nest#h{nest_index:04}")
        );
    }
    let input = input(&first, 12, 0);
    let wrong_state = Want {
        feature: &first,
        state: 13,
        slot: 0,
        capture_current: false,
        consumed: true,
        input: Some(&input),
        bindings: RefCell::default(),
    };
    assert!(input_bindings(&wrong_state, &nest, 0).is_empty());
    let wrong_history = Want {
        feature: &second,
        state: 12,
        ..wrong_state
    };
    assert!(input_bindings(&wrong_history, &nest, 0).is_empty());
}

#[test]
fn two_closed_carriers_with_equal_samples_require_the_native_edge_slot() {
    use cadmpeg_ir::features::{
        FeatureInputTopology, FinitePoint3, HistoricalEdgeGeometry, HistoricalEdgePath,
    };
    use cadmpeg_ir::geometry::analytic::CircleCurve;
    use cadmpeg_ir::math::{Point3, Vector3};
    let owner = feature("closed");
    let cube = cadmpeg_ir::examples::unit_cube().unwrap().geometry;
    let mut nest = BodyTopology::new(cube.bodies[0].clone());
    nest.edges = cube.edges[..2].to_vec();
    nest.curves = cube.curves[..2].to_vec();
    for (index, sign) in [(0, 1.0), (1, -1.0)] {
        nest.curves[index].geometry = CurveGeometry::Solved(SolvedCurveGeometry::Circle(
            CircleCurve::try_new(
                Point3::new(sign, 0.0, 0.0),
                Vector3::new(0.0, 0.0, 1.0),
                Vector3::new(sign, 0.0, 0.0),
                1.0,
            )
            .unwrap(),
        ));
        nest.edges[index].ends = None;
        nest.edges[index].carrier = cadmpeg_ir::topology::EdgeCarrier::new(
            Some(nest.curves[index].id.clone()),
            Some([0.0, std::f64::consts::TAU]),
        )
        .unwrap();
    }
    let ctx = cadmpeg_core::decode::test_support::service_decode_context();
    for curve in &nest.curves {
        let point = cadmpeg_ir::eval::decode::curve_point_solved(
            &ctx,
            curve.geometry.solved().unwrap(),
            std::f64::consts::PI,
        )
        .unwrap();
        assert!(point.get().vector_from(Point3::new(0.0, 0.0, 0.0)).norm() <= EPS_CLOSED_SAMPLE);
    }
    let prefix = crate::ids::feature_input_prefix(&owner.id, 12);
    let geometry = HistoricalEdgeGeometry::new(
        crate::ids::history_input_edge_id(&prefix, 0),
        FinitePoint3::ZERO,
    );
    let mut input = FeatureInputTopology {
        id: crate::ids::history_input_state_id(&prefix),
        input_of: owner.id.clone(),
        bodies: DistinctMembers::default(),
        faces: DistinctMembers::default(),
        edges: DistinctMembers::try_from(vec![geometry.edge.clone()]).unwrap(),
        vertices: DistinctMembers::default(),
        edge_geometry: vec![geometry],
        native_ref: None,
    };
    let bind = |input: &FeatureInputTopology| {
        input_bindings(
            &Want {
                feature: &owner,
                state: 12,
                slot: 0,
                capture_current: false,
                consumed: true,
                input: Some(input),
                bindings: RefCell::default(),
            },
            &nest,
            0,
        )
    };
    assert_eq!(
        bind(&input)[0].1.path,
        HistoricalEdgePath::Edge {
            edge: nest.edges[0].id.clone()
        }
    );
    input.edge_geometry[0].edge = crate::ids::history_input_edge_id(&prefix, 99);
    assert!(bind(&input).is_empty());
}

#[test]
fn treatment_snapshot_requests_join_history_state_and_body_membership() {
    use crate::history_records::{
        AsmDeltaState, AsmHistoricalCarrierBinding, AsmHistoricalEdge,
        AsmHistoricalOptionalCarrierBinding, AsmHistoricalRelation, AsmHistoricalTopology,
    };
    use crate::records::feature::scope::{DesignFeatureKind, DesignParameterScope};
    use cadmpeg_ir::features::{
        FeatureInputTopology, FinitePoint3, HistoricalEdgeGeometry, NonEmptyMembers,
    };
    let relation = |owner_ref, member_refs| AsmHistoricalRelation {
        owner_ref,
        member_refs,
    };
    let topology = AsmHistoricalTopology {
        bodies: vec![1],
        regions: vec![2],
        shells: vec![3],
        edges: vec![4],
        vertices: vec![5, 6],
        points: vec![7, 8],
        curves: vec![9],
        body_regions: vec![relation(1, vec![2])],
        region_shells: vec![relation(2, vec![3])],
        shell_faces: vec![relation(3, Vec::new())],
        shell_wire_edges: vec![relation(3, vec![4])],
        shell_free_vertices: vec![relation(3, Vec::new())],
        edge_vertices: vec![AsmHistoricalEdge {
            edge: 4,
            start_vertex: 5,
            end_vertex: 6,
        }],
        vertex_points: vec![
            AsmHistoricalCarrierBinding {
                entity: 5,
                carrier: 7,
            },
            AsmHistoricalCarrierBinding {
                entity: 6,
                carrier: 8,
            },
        ],
        edge_curves: vec![AsmHistoricalOptionalCarrierBinding {
            entity: 4,
            carrier: Some(9),
        }],
        ..AsmHistoricalTopology::default()
    };
    let history = |source: &str| {
        crate::test_support::history::history(
            format!("f3d:{source}:asm-history#0"),
            0,
            None,
            false,
            [14, 13, 12]
                .into_iter()
                .map(|value| {
                    (
                        AsmDeltaState {
                            id: format!("f3d:{source}:state#{value}"),
                            parent: format!("f3d:{source}:asm-history#0"),
                            byte_offset: 0,
                            state_id: value,
                            version_flag: 1,
                            state_flag: 0,
                            previous_ref: None,
                            next_ref: None,
                            node_index: 14 - value,
                            partner_ref: None,
                            owner_ref: 0,
                            bulletin_boards: Vec::new(),
                            records: Vec::new(),
                            transition: None,
                        },
                        Some(topology.clone()),
                        Vec::new(),
                    )
                })
                .collect(),
        )
    };
    let histories = [history("a"), history("b")];
    let mut scope =
        DesignParameterScope::empty("f3d:design:scope#treatment", DesignFeatureKind::Fillet, 1);
    scope
        .try_edit(|draft| {
            draft.history_state_id = Some(13);
            draft.previous_history_state_id = Some(12);
            draft.layout_fixture_tail();
        })
        .unwrap();
    let mut feature = feature("treatment");
    feature.native_ref = Some(scope.id.clone());
    let prefix = crate::ids::feature_input_prefix(&feature.id, 12);
    let edge = crate::ids::history_input_edge_id(&prefix, 4);
    feature.evaluation = FeatureEvaluation::from_definition(FeatureDefinition::Operation(
        FeatureOperation::Fillet {
            groups: NonEmptyMembers::try_from(vec![
                cadmpeg_ir::features::edge_treatments::FilletGroup {
                    edges: cadmpeg_ir::features::EdgeSelection::historical(
                        crate::ids::history_input_state_id(&prefix),
                        vec![edge.clone()],
                        "native-group".into(),
                    )
                    .unwrap(),
                    radius: cadmpeg_ir::features::edge_treatments::RadiusSpec::Unresolved {
                        form: None,
                    },
                    tangency_weight: None,
                },
            ])
            .unwrap(),
        },
    ));
    let mut input = FeatureInputTopology {
        id: crate::ids::history_input_state_id(&prefix),
        input_of: feature.id.clone(),
        bodies: DistinctMembers::try_from(vec![crate::ids::history_input_body_id(&prefix, 1)])
            .unwrap(),
        faces: DistinctMembers::default(),
        edges: DistinctMembers::try_from(vec![edge.clone()]).unwrap(),
        vertices: DistinctMembers::default(),
        edge_geometry: vec![HistoricalEdgeGeometry::new(edge, FinitePoint3::ZERO)],
        native_ref: Some("f3d:b:state#12".into()),
    };
    let bindings = HashMap::from([(scope.id.clone(), histories[1].id.clone())]);
    let features = [feature.clone()];
    let scopes = [scope];
    let inputs = Inputs {
        features: &features,
        scopes: &scopes,
        histories: &histories,
        bindings: &bindings,
        primary_brep: None,
    };
    let wants = input_edge_bodies(&feature, inputs, std::slice::from_ref(&input));
    assert_eq!(wants.len(), 1);
    assert_eq!(
        (
            wants[0].0,
            wants[0].1.state,
            wants[0].1.slot,
            wants[0].1.consumed
        ),
        (1, 12, 1, true)
    );
    drop(wants);
    input.bodies.clear();
    assert!(input_edge_bodies(&feature, inputs, std::slice::from_ref(&input)).is_empty());
    input
        .bodies
        .insert(crate::ids::history_input_body_id(&prefix, 1));
    let other = HashMap::from([(scopes[0].id.clone(), histories[0].id.clone())]);
    let inputs = Inputs {
        bindings: &other,
        ..inputs
    };
    assert!(input_edge_bodies(&feature, inputs, std::slice::from_ref(&input)).is_empty());
    let unbound = HashMap::new();
    let inputs = Inputs {
        bindings: &unbound,
        ..inputs
    };
    assert!(input_edge_bodies(&feature, inputs, std::slice::from_ref(&input)).is_empty());
}

#[test]
fn source_procedural_curve_with_a_solved_cache_keeps_the_sample_representation() {
    use cadmpeg_ir::features::{HistoricalEdgePath, HistoricalEdgeTopologyRef};
    let mut source = cadmpeg_ir::examples::unit_cube().unwrap().geometry;
    let reference = HistoricalEdgeTopologyRef {
        history_body: HistoryBodyId::mint("test:history-body:nest#input").unwrap(),
        path: HistoricalEdgePath::Edge {
            edge: source.edges[0].id.clone(),
        },
        faces: Vec::new(),
    };
    assert!(source_carriers_solved(&source, &reference));
    let cache = source.curves[0].geometry.solved().cloned();
    source.curves[0].geometry = CurveGeometry::Procedural {
        construction: cadmpeg_ir::ids::ProceduralCurveId::mint("test:history:construction#curve")
            .unwrap(),
        cache,
    };
    assert!(!source_carriers_solved(&source, &reference));
}

#[test]
fn failed_input_binding_preserves_samples_supports_and_operands_with_a_loss() {
    use cadmpeg_ir::features::{
        EdgeSelection, EdgeSupport, FeatureInputTopology, FinitePoint3, HistoricalEdgeGeometry,
        NonEmptyMembers,
    };
    use std::io::{Cursor, Write};
    let mut ir = CadIr::empty();
    let mut feature = feature("unbound-treatment");
    let prefix = crate::ids::feature_input_prefix(&feature.id, 12);
    let edge = crate::ids::history_input_edge_id(&prefix, 4);
    let face = crate::ids::history_input_face_id(&prefix, 10);
    feature.evaluation = FeatureEvaluation::from_definition(FeatureDefinition::Operation(
        FeatureOperation::Fillet {
            groups: NonEmptyMembers::try_from(vec![
                cadmpeg_ir::features::edge_treatments::FilletGroup {
                    edges: EdgeSelection::historical(
                        crate::ids::history_input_state_id(&prefix),
                        vec![edge.clone()],
                        "native-group".into(),
                    )
                    .unwrap(),
                    radius: cadmpeg_ir::features::edge_treatments::RadiusSpec::Unresolved {
                        form: None,
                    },
                    tangency_weight: None,
                },
            ])
            .unwrap(),
        },
    ));
    let mut geometry = HistoricalEdgeGeometry::new(edge.clone(), FinitePoint3::ZERO);
    geometry.start = Some(FinitePoint3::ZERO);
    geometry.end = Some(FinitePoint3::ZERO);
    geometry.tangent = Some(cadmpeg_ir::units::UnitVector3::X_AXIS);
    geometry.faces.insert(face.clone());
    geometry.supports.push(EdgeSupport {
        face: face.clone(),
        normal: cadmpeg_ir::units::UnitVector3::Y_AXIS,
        with_edge: true,
    });
    ir.model
        .feature_input_topologies
        .push(FeatureInputTopology {
            id: crate::ids::history_input_state_id(&prefix),
            input_of: feature.id.clone(),
            bodies: DistinctMembers::default(),
            faces: DistinctMembers::try_from(vec![face]).unwrap(),
            edges: DistinctMembers::try_from(vec![edge.clone()]).unwrap(),
            vertices: DistinctMembers::default(),
            edge_geometry: vec![geometry.clone()],
            native_ref: Some("f3d:native:state#12".into()),
        });
    let evaluation = feature.evaluation.clone();
    ir.model.features.push(feature.clone());
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let options = crate::zip_write::file_options(zip::CompressionMethod::Stored);
    crate::test_support::manifest_test::write_synthetic_manifests(&mut zip, options);
    zip.start_file("marker", options).unwrap();
    zip.write_all(b"fixture").unwrap();
    let archive = zip.finish().unwrap().into_inner();
    let mut losses = Vec::new();
    crate::test_support::zip_test::with_scan(&archive, |scan| {
        fill_consumed(
            &cadmpeg_core::decode::test_support::service_decode_context(),
            scan,
            &mut ir,
            &F3dNative::default(),
            None,
            &ConsumerContext::default(),
            &mut losses,
        )
        .unwrap();
    });
    assert_eq!(
        ir.model.feature_input_topologies[0].edge_geometry,
        vec![geometry]
    );
    assert_eq!(ir.model.features[0].evaluation, evaluation);
    assert_eq!(losses.len(), 1);
    assert_eq!(
        losses[0].code,
        F3dLossCode::FeatureHistoricalEdgeTopologyUnbound.kind()
    );
    assert!(losses[0].message.contains(feature.id.as_str()));
    assert!(losses[0].message.contains("f3d:native:state#12"));
    assert!(losses[0].message.contains(edge.as_str()));
}
