// SPDX-License-Identifier: Apache-2.0
use super::*;
use cadmpeg_asm::kernel_header::RefWidth;
use cadmpeg_core::decode::{DecodeArena, DecodePolicy};
use cadmpeg_ir::features::{
    BodySelection, DistinctMembers, FeatureContent, FeatureDefinition, FeatureEvaluation,
    FeatureId, FeatureOperation,
};

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
    let bytes = carrier_bytes(width, changed);
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
    let feature = feature("base");
    let wants = (0..count)
        .map(|_| Want {
            feature: &feature,
            state: 12,
            slot: 1,
        })
        .collect::<Vec<_>>();
    let mut bodies = Vec::new();
    let mut losses = Vec::new();
    fill_history(
        &ctx,
        &bytes,
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
fn an_unchanged_body_needs_no_history_body() {
    let (bodies, losses) = run(false, RefWidth::Eight, 1, MAX_HISTORY_BODY_WORK);
    assert!(bodies.is_empty());
    assert!(losses.is_empty());
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
    let ctx = cadmpeg_test_support::service_decode_context();
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
