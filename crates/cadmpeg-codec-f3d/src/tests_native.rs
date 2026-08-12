// SPDX-License-Identifier: Apache-2.0
//! Native-domain synthetic tests and fixtures.

use super::*;

pub(super) trait TestEncode {
    fn encode(
        &self,
        ir: &cadmpeg_ir::CadIr,
        output: &mut dyn Write,
    ) -> Result<cadmpeg_ir::ExportReport, cadmpeg_core::CodecError>;
}

impl TestEncode for F3dCodec {
    fn encode(
        &self,
        ir: &cadmpeg_ir::CadIr,
        output: &mut dyn Write,
    ) -> Result<cadmpeg_ir::ExportReport, cadmpeg_core::CodecError> {
        self.plan(cadmpeg_ir::codec::EncodeInput { ir, fidelity: None })?
            .write_to(output)
    }
}

pub(super) fn with_scan<T>(bytes: &[u8], f: impl FnOnce(&container::ContainerScan<'_>) -> T) -> T {
    let arena = DecodeArena::new();
    let policy = DecodePolicy::default();
    let (ctx, root) = DecodeContext::from_root_bytes(bytes, &arena, &policy).unwrap();
    let scan = container::scan(&ctx, root).unwrap();
    f(&scan)
}

pub(super) fn write_synthetic_manifests<W: Write + Seek>(
    zip: &mut zip::ZipWriter<W>,
    options: zip::write::SimpleFileOptions,
) {
    zip.start_file("Manifest.dat", options).unwrap();
    zip.write_all(&crate::manifest::generated_top_level().unwrap())
        .unwrap();
    zip.start_file(
        format!(
            "{}/Manifest.dat",
            crate::manifest::GENERATED_DESIGN_ASSET_FOLDER
        ),
        options,
    )
    .unwrap();
    zip.write_all(&crate::manifest::generated_design_asset().unwrap())
        .unwrap();
}

/// Build a synthetic ASM `BinaryFile8` BREP stream: a spec-shaped header
/// followed by a couple of filler records and a `delta_state` history marker.
pub(super) fn synthetic_smbh() -> Vec<u8> {
    let mut b = Vec::new();
    b.extend_from_slice(b"ASM BinaryFile8"); // 0..15 magic
    b.extend_from_slice(&23100u32.to_le_bytes()); // 15..19 save-format version
    b.extend_from_slice(&[0u8; 12]); // 19..31 zero
    b.extend_from_slice(&7u64.to_le_bytes()); // 31..39 entity-count word
    b.extend_from_slice(&3u64.to_le_bytes()); // 39..47 flags: history partition
    push_u8_string(&mut b, "Autodesk Neutron"); // 0x07 tag at offset 47
    push_u8_string(&mut b, "ASM 231.6.3.65535 OSX");
    push_u8_string(&mut b, "Tue Mar 31 16:16:19 2026");
    push_tagged_f64(&mut b, 60.0); // scale
    push_tagged_f64(&mut b, 1e-6); // resabs
    push_tagged_f64(&mut b, 1e-10); // resnor

    // Some active-model filler (no delta_state here).
    b.extend_from_slice(&[0x0d, 0x04, b'b', b'o', b'd', b'y', 0x11]);
    let active_len = b.len();

    // History boundary: the preceding record's `0x11` terminator is followed
    // by the exact `0x0d 0x0b "delta_state"` record-name token.
    b.extend_from_slice(&[0x0d, 0x0b]);
    b.extend_from_slice(b"delta_state");
    b.extend_from_slice(&[0u8; 16]);

    // Sanity: the delta-state identifier starts immediately after the solved
    // record sequence.
    assert_eq!(&b[active_len..active_len + 2], &[0x0d, 0x0b]);
    assert_eq!(&b[active_len + 2..active_len + 13], b"delta_state");
    b
}

pub(super) fn push_u8_string(b: &mut Vec<u8>, s: &str) {
    b.push(0x07);
    b.push(s.len() as u8);
    b.extend_from_slice(s.as_bytes());
}

// ---- SAB record-stream fixtures ---------------------------------------------
//
// The helpers below assemble a minimal but genuine active model slice: an
// `asmheader` at RecordTable index 0 followed by a single planar face bounded by
// a closed three-coedge loop, with its edges, vertices, and points. Entity
// references are RecordTable indices; `-1` is null. This exercises the framer,
// topology graph builder, and analytic surface decode end to end.

/// The three `0x07`-tagged strings + three `0x06`-tagged doubles of a
/// `BinaryFile8` header, i.e. the bytes up to the start of the record stream.
pub(super) fn smbh_header_prefix() -> Vec<u8> {
    let mut b = Vec::new();
    b.extend_from_slice(b"ASM BinaryFile8");
    b.extend_from_slice(&23100u32.to_le_bytes()); // save-format version
    b.extend_from_slice(&[0u8; 12]); // zero region
    b.extend_from_slice(&5u64.to_le_bytes()); // entity-count word
    b.extend_from_slice(&3u64.to_le_bytes()); // flags: history partition
    push_u8_string(&mut b, "Autodesk Neutron");
    push_u8_string(&mut b, "ASM 231.6.3.65535 OSX");
    push_u8_string(&mut b, "Tue Mar 31 16:16:19 2026");
    push_tagged_f64(&mut b, 60.0);
    push_tagged_f64(&mut b, 1e-6);
    push_tagged_f64(&mut b, 1e-10);
    b
}

pub(super) fn t_ref(b: &mut Vec<u8>, v: i64) {
    b.push(0x0c);
    b.extend_from_slice(&v.to_le_bytes());
}
pub(super) fn t_long(b: &mut Vec<u8>, v: i64) {
    b.push(0x04);
    b.extend_from_slice(&v.to_le_bytes());
}
pub(super) fn t_dbl(b: &mut Vec<u8>, v: f64) {
    b.push(0x06);
    b.extend_from_slice(&v.to_le_bytes());
}
pub(super) fn t_pos(b: &mut Vec<u8>, p: [f64; 3]) {
    b.push(0x13);
    for c in p {
        b.extend_from_slice(&c.to_le_bytes());
    }
}
pub(super) fn t_vec(b: &mut Vec<u8>, p: [f64; 3]) {
    b.push(0x14);
    for c in p {
        b.extend_from_slice(&c.to_le_bytes());
    }
}
pub(super) fn t_ident(b: &mut Vec<u8>, s: &str) {
    b.push(0x0d);
    b.push(s.len() as u8);
    b.extend_from_slice(s.as_bytes());
}
pub(super) fn t_u16_string(b: &mut Vec<u8>, value: &str) {
    b.push(0x08);
    b.extend_from_slice(&u16::try_from(value.len()).unwrap().to_le_bytes());
    b.extend_from_slice(value.as_bytes());
}

pub(super) fn renamed_generated_subtype(mut bytes: Vec<u8>, old: &str, new: &str) -> Vec<u8> {
    let old = old.as_bytes();
    let position = bytes
        .windows(old.len())
        .position(|window| window == old)
        .expect("generated subtype name");
    assert!(matches!(
        bytes.get(position.wrapping_sub(2)),
        Some(0x0d | 0x0e)
    ));
    bytes[position - 1] = u8::try_from(new.len()).expect("short subtype name");
    bytes.splice(position..position + old.len(), new.bytes());
    bytes
}
pub(super) fn t_subident(b: &mut Vec<u8>, s: &str) {
    b.push(0x0e);
    b.push(s.len() as u8);
    b.extend_from_slice(s.as_bytes());
}
pub(super) fn t_end(b: &mut Vec<u8>) {
    b.push(0x11);
}

pub(super) fn t_attribute_base(b: &mut Vec<u8>, next: i64, previous: i64, owner: i64) {
    t_ref(b, -1);
    t_long(b, -1);
    t_ref(b, next);
    t_ref(b, previous);
    t_ref(b, owner);
}

pub(super) fn assert_f3d_native_parity(ir: &cadmpeg_ir::document::CadIr) {
    let native = ir.native.namespace("f3d").expect("F3D native namespace");
    assert_eq!(native.version, crate::native::F3D_NATIVE_VERSION);
}

pub(super) fn f3d_native(ir: &cadmpeg_ir::document::CadIr) -> crate::native::F3dNative {
    crate::native::F3dNative::load(ir.native.namespace("f3d").expect("F3D native namespace"))
        .unwrap()
}

pub(super) struct F3dNativeMut<'a> {
    ir: &'a mut cadmpeg_ir::document::CadIr,
    native: crate::native::F3dNative,
}

impl std::ops::Deref for F3dNativeMut<'_> {
    type Target = crate::native::F3dNative;

    fn deref(&self) -> &Self::Target {
        &self.native
    }
}

impl std::ops::DerefMut for F3dNativeMut<'_> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.native
    }
}

impl Drop for F3dNativeMut<'_> {
    fn drop(&mut self) {
        self.native
            .store(self.ir.native.namespace_mut("f3d"))
            .unwrap();
    }
}

pub(super) fn f3d_native_mut(ir: &mut cadmpeg_ir::document::CadIr) -> F3dNativeMut<'_> {
    let native = ir
        .native
        .namespace("f3d")
        .map(crate::native::F3dNative::load)
        .transpose()
        .unwrap()
        .unwrap_or_default();
    F3dNativeMut { ir, native }
}

#[test]
fn native_arenas_have_pinned_shape_and_typed_round_trip() {
    let catalogue_names = crate::native::F3D_FAMILIES
        .iter()
        .map(|row| row.arena)
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(crate::native::F3D_FAMILIES.len(), 70);
    assert_eq!(
        catalogue_names,
        crate::native::F3D_ARENA_NAMES
            .iter()
            .copied()
            .collect::<std::collections::BTreeSet<_>>()
    );
    let decoded = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh_and_protein(&synthetic_geometry_smbh())),
            &DecodeOptions::default(),
        )
        .unwrap();
    let original = decoded.ir.native.namespace("f3d").unwrap();
    let typed = crate::native::F3dNative::load(original).unwrap();
    let mut round_trip = cadmpeg_ir::NativeNamespace::default();
    typed.store(&mut round_trip).unwrap();
    assert_eq!(typed, crate::native::F3dNative::load(&round_trip).unwrap());
    for name in crate::native::F3D_ARENA_NAMES {
        assert_eq!(
            round_trip.arenas.get(*name),
            original.arenas.get(*name),
            "native arena {name} did not survive a typed round trip"
        );
    }
    assert_eq!(round_trip.version, crate::native::F3D_NATIVE_VERSION);
    assert_eq!(
        round_trip
            .arenas
            .keys()
            .map(String::as_str)
            .collect::<Vec<_>>(),
        crate::native::F3D_ARENA_NAMES
    );
    for records in round_trip.arenas.values() {
        for record in records {
            let json = serde_json::to_value(record).unwrap();
            assert_eq!(json["id"], record.id());
            assert!(json.as_object().unwrap().len() > 1);
        }
    }
}

#[test]
fn diff_reports_design_material_assignment_changes() {
    let decoded = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh_and_protein(&synthetic_geometry_smbh())),
            &DecodeOptions::default(),
        )
        .unwrap();
    let mut edited = decoded.ir.clone();
    let assignment = &mut edited
        .native
        .namespace_mut("f3d")
        .arenas
        .get_mut("design_material_assignments")
        .unwrap()[0];
    let mut assignment_fields = assignment.fields();
    assignment_fields.insert("entity_suffix".into(), serde_json::json!(123_456));
    *assignment = cadmpeg_ir::NativeRecord::new(assignment.id().to_string(), assignment_fields);
    let report = cadmpeg_ir::diff(&decoded.ir, &edited);
    let arena = report
        .per_arena
        .iter()
        .find(|arena| arena.kind == "native.f3d.design_material_assignments")
        .unwrap();
    assert_eq!(arena.modified.len(), 1);
}

pub(super) fn update_f3d_native<R>(
    ir: &mut cadmpeg_ir::document::CadIr,
    update: impl FnOnce(&mut crate::native::F3dNative) -> R,
) -> R {
    let mut native = f3d_native_mut(ir);
    update(&mut native)
}

/// Assemble the active slice: header prefix + records + `delta_state` boundary.
/// `RecordTable` indices are the order below, starting at 0 (`asmheader`).
pub(super) fn synthetic_geometry_smbh() -> Vec<u8> {
    // Indices: 0 asmheader, 1 body, 2 region, 3 shell, 4 face, 5 loop,
    // 6 plane, 7/8/9 coedges, 10/11/12 edges, 13/14/15 vertices,
    // 16/17/18 points.
    let mut r = Vec::new();

    // 0: asmheader
    t_ident(&mut r, "asmheader");
    push_u8_string(&mut r, "231.6.3.65535");
    t_end(&mut r);

    // 1: body  (chunk3 = first_region)
    t_ident(&mut r, "body");
    t_ref(&mut r, -1); // 0 attrib
    t_long(&mut r, 42); // 1 native ASM body key
    t_ref(&mut r, -1); // 2 null
    t_ref(&mut r, 2); // 3 first_region
    t_ref(&mut r, -1); // 4 wire
    t_ref(&mut r, -1); // 5 transform
    t_end(&mut r);

    // 2: region  (chunk4 = first_shell, chunk5 = owner_body)
    t_ident(&mut r, "region");
    t_ref(&mut r, -1); // 0 next
    t_long(&mut r, -1); // 1 history
    t_ref(&mut r, -1); // 2 null
    t_ref(&mut r, -1); // 3 null
    t_ref(&mut r, 3); // 4 first_shell
    t_ref(&mut r, 1); // 5 owner_body
    t_end(&mut r);

    // 3: shell  (chunk5 = first_face, chunk7 = owner_region)
    t_ident(&mut r, "shell");
    t_ref(&mut r, -1); // 0 next
    t_long(&mut r, -1); // 1 history
    t_ref(&mut r, -1); // 2 null
    t_ref(&mut r, -1); // 3 null
    t_ref(&mut r, -1); // 4 null
    t_ref(&mut r, 4); // 5 first_face
    t_ref(&mut r, -1); // 6 wire
    t_ref(&mut r, 2); // 7 owner_region
    t_end(&mut r);

    // 4: face  (chunk4 first_loop, chunk5 owner_shell, chunk7 surface, chunk8 sense)
    t_ident(&mut r, "face");
    t_ref(&mut r, -1); // 0 attrib
    t_long(&mut r, -1); // 1 history
    t_ref(&mut r, -1); // 2 null
    t_ref(&mut r, -1); // 3 next_face
    t_ref(&mut r, 5); // 4 first_loop
    t_ref(&mut r, 3); // 5 owner_shell
    t_ref(&mut r, -1); // 6 null
    t_ref(&mut r, 6); // 7 surface
    r.push(0x0b); // 8 sense = forward
    r.push(0x0b); // 9 sides = single
    t_end(&mut r);

    // 5: loop  (chunk4 first_coedge, chunk5 owner_face)
    t_ident(&mut r, "loop");
    t_ref(&mut r, -1); // 0 attrib
    t_long(&mut r, -1); // 1 history
    t_ref(&mut r, -1); // 2 null
    t_ref(&mut r, -1); // 3 next_loop
    t_ref(&mut r, 7); // 4 first_coedge
    t_ref(&mut r, 4); // 5 owner_face
    t_end(&mut r);

    // 6: plane-surface  (origin, normal, uv-origin)
    t_subident(&mut r, "plane");
    t_ident(&mut r, "surface");
    t_ref(&mut r, -1); // attrib
    t_long(&mut r, -1); // history
    t_ref(&mut r, -1); // null
    t_pos(&mut r, [0.0, 0.0, 0.0]); // root
    t_vec(&mut r, [0.0, 0.0, 1.0]); // normal
    t_vec(&mut r, [1.0, 0.0, 0.0]); // UV reference direction
    r.push(0x0b); // sense
    t_end(&mut r);

    // 7/8/9: coedges forming the ring 7 -> 8 -> 9 -> 7
    let coedges = [(7i64, 8, 9, 10), (8, 9, 7, 11), (9, 7, 8, 12)];
    for (_id, next, prev, edge) in coedges {
        t_ident(&mut r, "coedge");
        t_ref(&mut r, -1); // 0 attrib
        t_long(&mut r, -1); // 1 history
        t_ref(&mut r, -1); // 2 null
        t_ref(&mut r, next); // 3 next
        t_ref(&mut r, prev); // 4 prev
        t_ref(&mut r, -1); // 5 partner (open loop, none)
        t_ref(&mut r, edge); // 6 edge
        r.push(0x0b); // 7 sense = forward
        t_ref(&mut r, 5); // 8 owner_loop
        t_long(&mut r, 0); // 9 reserved
        t_ref(&mut r, -1); // 10 pcurve
        t_end(&mut r);
    }

    // 10/11/12: edges  (start, end vertices), curve = null
    let edges = [(10i64, 13, 14), (11, 14, 15), (12, 15, 13)];
    for (_id, start, end) in edges {
        t_ident(&mut r, "edge");
        t_ref(&mut r, -1); // 0 attrib
        t_long(&mut r, -1); // 1 history
        t_ref(&mut r, -1); // 2 null
        t_ref(&mut r, start); // 3 start_vertex
        t_dbl(&mut r, 0.0); // 4 t_start
        t_ref(&mut r, end); // 5 end_vertex
        t_dbl(&mut r, 1.0); // 6 t_end
        t_ref(&mut r, -1); // 7 owner_coedge
        t_ref(&mut r, -1); // 8 curve (degenerate: none)
        r.push(0x0b); // 9 sense
        push_u8_string(&mut r, "unknown"); // 10 continuity text
        t_end(&mut r);
    }

    // 13/14/15: vertices (owning_edge, index_flag, point)
    let verts = [(13i64, 10, 0, 16), (14, 10, 1, 17), (15, 12, 0, 18)];
    for (_id, edge, index_flag, point) in verts {
        t_ident(&mut r, "vertex");
        t_ref(&mut r, -1); // 0 attrib
        t_long(&mut r, -1); // 1 history
        t_ref(&mut r, -1); // 2 null
        t_ref(&mut r, edge); // 3 owning_edge
        t_long(&mut r, index_flag); // 4 index_flag
        t_ref(&mut r, point); // 5 point
        t_end(&mut r);
    }

    // 16/17/18: points  (coordinates in cm; ×10 = mm)
    let points = [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]];
    for p in points {
        t_ident(&mut r, "point");
        t_ref(&mut r, -1); // attrib
        t_long(&mut r, -1); // history
        t_ref(&mut r, -1); // null
        t_pos(&mut r, p);
        t_end(&mut r);
    }

    // History boundary: previous record's 0x11 + 0x0d 0x0b 'delta_state'.
    t_ident(&mut r, "delta_state"); // 0x0d 0x0b 'delta_state'

    let mut out = smbh_header_prefix();
    out.extend_from_slice(&r);
    out
}

pub(super) fn replace_generated_record_head(bytes: &mut Vec<u8>, from: &str, to: &str) {
    let mut needle = vec![0x0d, from.len() as u8];
    needle.extend_from_slice(from.as_bytes());
    let mut replacement = vec![0x0d, to.len() as u8];
    replacement.extend_from_slice(to.as_bytes());
    let offsets = bytes
        .windows(needle.len())
        .enumerate()
        .filter_map(|(offset, window)| (window == needle).then_some(offset))
        .collect::<Vec<_>>();
    for offset in offsets.into_iter().rev() {
        bytes.splice(offset..offset + needle.len(), replacement.iter().copied());
    }
}

pub(super) fn append_generated_record_tail(bytes: &mut Vec<u8>, head: &str, tail: &[u8]) {
    let record_start = bytes
        .windows(b"\x0d\x09asmheader".len())
        .position(|window| window == b"\x0d\x09asmheader")
        .expect("generated ASM record table");
    let offsets = cadmpeg_asm::sab::frame(bytes, record_start, bytes.len(), 8)
        .expect("generated ASM records must frame")
        .into_iter()
        .filter(|record| record.head == head)
        .map(|record| record.offset + record.len - 1)
        .collect::<Vec<_>>();
    for offset in offsets.into_iter().rev() {
        bytes.splice(offset..offset, tail.iter().copied());
    }
}

#[test]
fn decode_transfers_generated_tolerant_coedge_parameters_and_topology() {
    let mut smbh = synthetic_geometry_smbh();
    let mut parameter_tail = Vec::new();
    t_dbl(&mut parameter_tail, 0.25);
    t_dbl(&mut parameter_tail, 0.75);
    t_ref(&mut parameter_tail, -1);
    t_long(&mut parameter_tail, 0);
    t_long(&mut parameter_tail, 0);
    append_generated_record_tail(&mut smbh, "coedge", &parameter_tail);
    replace_generated_record_head(&mut smbh, "coedge", "tcoedge");
    let mut decoded = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh_and_protein(&smbh)),
            &DecodeOptions::default(),
        )
        .expect("generated tolerant coedges must decode");

    assert_eq!(decoded.ir.model.coedges.len(), 3);
    assert_eq!(decoded.ir.model.edges.len(), 3);
    assert_eq!(decoded.ir.model.shells[0].faces.len(), 1);
    assert_eq!(
        f3d_native(&decoded.ir)
            .tolerant_coedge_parameters
            .iter()
            .map(|parameters| parameters.parameter_range)
            .collect::<Vec<_>>(),
        vec![[0.25, 0.75]; 3]
    );
    assert!(f3d_native(&decoded.ir)
        .tolerant_coedge_parameters
        .iter()
        .all(|parameters| matches!(
            parameters.extension,
            cadmpeg_asm::brep::records::TolerantCoedgeExtension::Empty { target: None }
        )));

    decoded.ir.model.coedges[0].sense = cadmpeg_ir::topology::Sense::Reversed;
    update_f3d_native(&mut decoded.ir, |native| {
        native.tolerant_coedge_parameters[0].parameter_range = [-1.5, 2.25];
    });
    let mut edited = Vec::new();
    F3dCodec
        .write_preserved_with_source_fidelity(&decoded.ir, &decoded.source_fidelity, &mut edited)
        .expect("tolerant coedge sense edit");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(edited), &DecodeOptions::default())
        .expect("edited tolerant coedge round trip");
    assert_eq!(
        round_trip.ir.model.coedges[0].sense,
        cadmpeg_ir::topology::Sense::Reversed
    );
    assert_eq!(
        f3d_native(&round_trip.ir).tolerant_coedge_parameters[0].parameter_range,
        [-1.5, 2.25]
    );
}

#[test]
fn decode_selects_tolerant_coedge_extension_from_save_format() {
    for (release, fixed_tail, expected) in [
        (
            23000u32,
            {
                let mut bytes = Vec::new();
                t_ref(&mut bytes, -1);
                t_long(&mut bytes, 1);
                bytes.extend_from_slice(&[0x0a, 0x0f]);
                t_long(&mut bytes, 22800);
                bytes.extend_from_slice(&[0x10, 0x0a]);
                t_dbl(&mut bytes, -2.0);
                bytes.push(0x0a);
                t_dbl(&mut bytes, 3.0);
                t_long(&mut bytes, 0);
                bytes
            },
            cadmpeg_asm::brep::records::TolerantCoedgeExtension::EmbeddedCurve {
                target: None,
                curve_reversed: true,
                payload_token_count: 1,
                parameter_range: Some([-2.0, 3.0]),
            },
        ),
        (
            21900u32,
            {
                let mut bytes = Vec::new();
                t_ref(&mut bytes, 17);
                bytes
            },
            cadmpeg_asm::brep::records::TolerantCoedgeExtension::Reference { target: Some(17) },
        ),
        (
            21400u32,
            Vec::new(),
            cadmpeg_asm::brep::records::TolerantCoedgeExtension::None,
        ),
    ] {
        let mut smbh = synthetic_geometry_smbh();
        smbh[15..19].copy_from_slice(&release.to_le_bytes());
        let mut tail = Vec::new();
        t_dbl(&mut tail, -0.5);
        t_dbl(&mut tail, 1.5);
        tail.extend_from_slice(&fixed_tail);
        append_generated_record_tail(&mut smbh, "coedge", &tail);
        replace_generated_record_head(&mut smbh, "coedge", "tcoedge");

        let decoded = F3dCodec
            .decode(
                &mut Cursor::new(f3d_with_smbh_and_protein(&smbh)),
                &DecodeOptions::default(),
            )
            .expect("release-selected tolerant coedges must decode");
        assert_eq!(
            f3d_native(&decoded.ir)
                .tolerant_coedge_parameters
                .iter()
                .map(|parameters| parameters.extension.clone())
                .collect::<Vec<_>>(),
            vec![expected; 3]
        );
    }
}

/// A tolerant coedge whose payload carries identifier tokens outside the
/// embedded scope: the freestanding embedded-curve type name before the sense
/// flag and trailing `null_curve` placeholders after the extension fields.
/// Identifiers are not fields, so the extension decodes exactly as it does
/// without them and the serialized token count stays defined over the value
/// tokens.
#[test]
fn tolerant_coedge_extension_ignores_payload_identifiers() {
    let mut smbh = synthetic_geometry_smbh();
    smbh[15..19].copy_from_slice(&23000u32.to_le_bytes());
    let mut tail = Vec::new();
    t_dbl(&mut tail, -0.5);
    t_dbl(&mut tail, 1.5);
    t_ref(&mut tail, -1);
    t_long(&mut tail, 1);
    t_ident(&mut tail, "intcurve");
    tail.extend_from_slice(&[0x0a, 0x0f]);
    t_ident(&mut tail, "par_int_cur");
    t_long(&mut tail, 22800);
    tail.extend_from_slice(&[0x10, 0x0a]);
    t_dbl(&mut tail, -2.0);
    tail.push(0x0a);
    t_dbl(&mut tail, 3.0);
    t_long(&mut tail, 0);
    t_ident(&mut tail, "null_curve");
    t_ident(&mut tail, "null_curve");
    append_generated_record_tail(&mut smbh, "coedge", &tail);
    replace_generated_record_head(&mut smbh, "coedge", "tcoedge");

    let decoded = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh_and_protein(&smbh)),
            &DecodeOptions::default(),
        )
        .expect("ident-bearing tolerant coedges must decode");
    assert_eq!(
        f3d_native(&decoded.ir)
            .tolerant_coedge_parameters
            .iter()
            .map(|parameters| parameters.extension.clone())
            .collect::<Vec<_>>(),
        vec![
            cadmpeg_asm::brep::records::TolerantCoedgeExtension::EmbeddedCurve {
                target: None,
                curve_reversed: true,
                payload_token_count: 1,
                parameter_range: Some([-2.0, 3.0]),
            };
            3
        ]
    );
}

#[test]
fn decode_transfers_embedded_tolerant_coedge_use_curves() {
    let mut smbh = synthetic_geometry_smbh();
    let mut tail = Vec::new();
    t_dbl(&mut tail, 0.0);
    t_dbl(&mut tail, 1.0);
    t_ref(&mut tail, -1);
    t_long(&mut tail, 1);
    tail.extend_from_slice(&[0x0a, 0x0f]);
    tail.extend_from_slice(&generated_curve_block());
    tail.extend_from_slice(&[0x10, 0x0a]);
    t_dbl(&mut tail, -2.0);
    tail.push(0x0a);
    t_dbl(&mut tail, 3.0);
    t_long(&mut tail, 0);
    append_generated_record_tail(&mut smbh, "coedge", &tail);
    replace_generated_record_head(&mut smbh, "coedge", "tcoedge");

    let decoded = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh_and_protein(&smbh)),
            &DecodeOptions::default(),
        )
        .expect("embedded tolerant-coedge curves must decode");
    assert_eq!(
        decoded
            .ir
            .model
            .coedges
            .iter()
            .filter(|coedge| coedge.use_curve.is_some())
            .count(),
        3
    );
    assert!(decoded.ir.model.coedges.iter().all(|coedge| {
        coedge.use_curve_parameter_range == Some([-2.0, 3.0])
            && coedge.use_curve.as_ref().is_some_and(|id| {
                decoded.ir.model.curves.iter().any(|curve| {
                    curve.id == *id
                        && matches!(curve.geometry, cadmpeg_ir::geometry::CurveGeometry::Nurbs(ref nurbs) if nurbs.degree == 2)
                })
            })
    }));
    let first_use_curve = decoded.ir.model.coedges[0]
        .use_curve
        .as_ref()
        .and_then(|id| decoded.ir.model.curves.iter().find(|curve| curve.id == *id))
        .expect("first embedded use curve");
    let cadmpeg_ir::geometry::CurveGeometry::Nurbs(first_use_curve) = &first_use_curve.geometry
    else {
        panic!("embedded use curve must be NURBS")
    };
    assert_eq!(
        first_use_curve.control_points[0],
        cadmpeg_ir::math::Point3::new(20.0, 0.0, 0.0)
    );
    assert_eq!(
        first_use_curve.control_points[2],
        cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0)
    );
    assert_eq!(first_use_curve.knots, [-1.0, -1.0, -1.0, -0.0, -0.0, -0.0]);

    let mut edited = decoded.ir.clone();
    let use_curve = edited.model.coedges[0]
        .use_curve
        .clone()
        .expect("first coedge use curve");
    let curve = edited
        .model
        .curves
        .iter_mut()
        .find(|curve| curve.id == use_curve)
        .expect("embedded use-curve carrier");
    let cadmpeg_ir::geometry::CurveGeometry::Nurbs(nurbs) = &mut curve.geometry else {
        panic!("embedded use curve must be NURBS")
    };
    nurbs.control_points[0].x += 1.0;
    let expected = nurbs.clone();
    let mut preserved = Vec::new();
    F3dCodec
        .write_preserved_with_source_fidelity(&edited, &decoded.source_fidelity, &mut preserved)
        .expect("embedded use-curve edit");
    let preserved = F3dCodec
        .decode(&mut Cursor::new(preserved), &DecodeOptions::default())
        .expect("embedded use-curve edit round trip");
    assert!(preserved.ir.model.curves.iter().any(|curve| {
        curve.id == use_curve
            && matches!(curve.geometry, cadmpeg_ir::geometry::CurveGeometry::Nurbs(ref curve) if *curve == expected)
    }));

    let mut source_less = cadmpeg_ir::examples::unit_cube();
    let generated_curve_id = cadmpeg_ir::ids::CurveId("generated:tolerant-use-curve#0".into());
    source_less.model.curves.push(cadmpeg_ir::geometry::Curve {
        id: generated_curve_id.clone(),
        geometry: cadmpeg_ir::geometry::CurveGeometry::Nurbs(expected.clone()),
        source_object: None,
    });
    let tolerant_coedge = source_less.model.coedges[0].id.clone();
    source_less.model.coedges[0].use_curve = Some(generated_curve_id);
    source_less.model.coedges[0].use_curve_parameter_range = Some([-2.0, 3.0]);
    f3d_native_mut(&mut source_less).tolerant_coedge_parameters =
        vec![cadmpeg_asm::brep::records::TolerantCoedgeParameters {
            id: "generated:tolerant-coedge-parameters#0".into(),
            coedge: tolerant_coedge,
            record_index: 0,
            parameter_range: [0.0, 1.0],
            extension: cadmpeg_asm::brep::records::TolerantCoedgeExtension::EmbeddedCurve {
                target: None,
                curve_reversed: false,
                payload_token_count: 0,
                parameter_range: Some([-2.0, 3.0]),
            },
        }];
    let mut generated = Vec::new();
    F3dCodec
        .plan(cadmpeg_ir::codec::EncodeInput {
            ir: &source_less,
            fidelity: None,
        })
        .and_then(|plan| plan.write_to(&mut generated))
        .expect("source-less embedded use curves");
    let generated = F3dCodec
        .decode(&mut Cursor::new(generated), &DecodeOptions::default())
        .expect("source-less embedded use-curve round trip");
    assert_eq!(
        generated
            .ir
            .model
            .coedges
            .iter()
            .filter(|coedge| coedge.use_curve.is_some())
            .count(),
        1
    );
    assert!(generated.ir.model.curves.iter().any(|curve| {
        matches!(curve.geometry, cadmpeg_ir::geometry::CurveGeometry::Nurbs(ref curve) if *curve == expected)
    }));
}

#[test]
fn decode_frames_history_less_stream_whose_final_record_ends_at_eof() {
    // A history-less `.smb` stream has no `delta_state` boundary and its final
    // `End-of-ASM-data` record ends at EOF without the `0x11` terminator.
    let mut smbh = synthetic_geometry_smbh();
    let marker = smbh
        .windows(b"\x0d\x0bdelta_state".len())
        .position(|window| window == b"\x0d\x0bdelta_state")
        .expect("generated history boundary");
    smbh.truncate(marker);
    for name in ["End", "of", "ASM"] {
        t_subident(&mut smbh, name);
    }
    t_ident(&mut smbh, "data"); // no trailing 0x11
    assert!(cadmpeg_asm::asm_header::solved_record_limit(&smbh).is_none());

    let decoded = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh_and_protein(&smbh)),
            &DecodeOptions::default(),
        )
        .expect("history-less stream must decode");
    assert_eq!(decoded.ir.model.faces.len(), 1);
    assert_eq!(decoded.ir.model.edges.len(), 3);
    assert_eq!(decoded.ir.model.vertices.len(), 3);
}

pub(super) fn synthetic_geometry_with_history_smbh() -> Vec<u8> {
    let mut bytes = synthetic_geometry_smbh();
    let name_tag = bytes
        .windows(b"\x0d\x0bdelta_state".len())
        .position(|window| window == b"\x0d\x0bdelta_state")
        .unwrap();
    let mut preamble = Vec::new();
    for name in ["Begin", "of", "ASM", "History"] {
        t_subident(&mut preamble, name);
    }
    t_ident(&mut preamble, "Data");
    t_ident(&mut preamble, "history_stream");
    for value in [2, 2, 0, 99] {
        t_long(&mut preamble, value);
    }
    for reference in [-1, 0, 1, -1] {
        t_ref(&mut preamble, reference);
    }
    t_end(&mut preamble);
    bytes.splice(name_tag..name_tag, preamble);

    let first_name_end = bytes
        .windows(b"delta_state".len())
        .position(|window| window == b"delta_state")
        .unwrap()
        + b"delta_state".len();
    let mut tail = Vec::new();
    for value in [2, 1, 0] {
        t_long(&mut tail, value);
    }
    for reference in [-1, 1, 0, -1, 0] {
        t_ref(&mut tail, reference);
    }
    tail.push(0x0b);
    t_long(&mut tail, 1); // board present
    t_ref(&mut tail, 0); // board owner
    t_long(&mut tail, 2); // board number
    t_long(&mut tail, 1); // change present
    t_ref(&mut tail, 1830); // old
    t_ref(&mut tail, 1); // new: update
    t_long(&mut tail, 1); // change present
    t_ref(&mut tail, -1); // old null
    t_ref(&mut tail, 8); // new: insert
    t_long(&mut tail, 0); // end changes
    t_long(&mut tail, 0); // end boards
    t_end(&mut tail);
    t_ident(&mut tail, "history_payload");
    t_long(&mut tail, 37);
    t_ref(&mut tail, 1830);
    t_ref(&mut tail, -1);
    t_end(&mut tail);
    t_ident(&mut tail, "delta_state");
    for value in [3, 1, 0] {
        t_long(&mut tail, value);
    }
    for reference in [0, -1, 1, -1, 0] {
        t_ref(&mut tail, reference);
    }
    tail.push(0x0b);
    t_end(&mut tail);
    bytes.splice(first_name_end.., tail);
    bytes
}

pub(super) fn synthetic_geometry_with_transform_smbh() -> Vec<u8> {
    let mut bytes = synthetic_geometry_smbh();
    let limit = cadmpeg_asm::asm_header::solved_record_limit(&bytes).expect("history boundary");
    let start = cadmpeg_asm::asm_header::record_stream_start(&bytes).expect("record stream");
    let records = cadmpeg_asm::sab::frame(&bytes, start, limit, 8).expect("generated SAB");
    let body = &records[1];
    let transform_ref = cadmpeg_asm::sab::payload_token_offsets(&bytes, body, 8, 0x0c)
        .expect("body reference tokens")[4];
    bytes[transform_ref + 1..transform_ref + 9].copy_from_slice(&19i64.to_le_bytes());

    let mut transform = Vec::new();
    t_ident(&mut transform, "transform");
    for vector in [
        [1.0, 0.0, 0.0],
        [0.0, 1.0, 0.0],
        [0.0, 0.0, 1.0],
        [1.0, 2.0, 3.0],
    ] {
        t_vec(&mut transform, vector);
    }
    t_dbl(&mut transform, 1.0);
    transform.extend_from_slice(&[0x0b, 0x0b, 0x0b]);
    t_end(&mut transform);
    bytes.splice(limit..limit, transform);
    bytes
}

pub(super) fn synthetic_geometry_with_body_color_smbh() -> Vec<u8> {
    let mut bytes = synthetic_geometry_smbh();
    let limit = cadmpeg_asm::asm_header::solved_record_limit(&bytes).expect("history boundary");
    let start = cadmpeg_asm::asm_header::record_stream_start(&bytes).expect("record stream");
    let records = cadmpeg_asm::sab::frame(&bytes, start, limit, 8).expect("generated SAB");
    let body = &records[1];
    let attribute_ref = cadmpeg_asm::sab::payload_token_offsets(&bytes, body, 8, 0x0c)
        .expect("body reference tokens")[0];
    bytes[attribute_ref + 1..attribute_ref + 9].copy_from_slice(&19i64.to_le_bytes());

    let mut attribute = Vec::new();
    t_subident(&mut attribute, "rgb_color");
    t_subident(&mut attribute, "st");
    t_ident(&mut attribute, "attrib");
    t_attribute_base(&mut attribute, -1, -1, 1);
    t_dbl(&mut attribute, 0.1);
    t_dbl(&mut attribute, 0.2);
    t_dbl(&mut attribute, 0.3);
    t_dbl(&mut attribute, 1.0);
    t_end(&mut attribute);
    bytes.splice(limit..limit, attribute);
    bytes
}

pub(super) fn synthetic_geometry_with_body_attribute_chain_smbh(
    attribute_chain: Vec<u8>,
) -> Vec<u8> {
    let mut bytes = synthetic_geometry_smbh();
    let limit = cadmpeg_asm::asm_header::solved_record_limit(&bytes).expect("history boundary");
    let start = cadmpeg_asm::asm_header::record_stream_start(&bytes).expect("record stream");
    let records = cadmpeg_asm::sab::frame(&bytes, start, limit, 8).expect("generated SAB");
    let body = &records[1];
    let attribute_ref = cadmpeg_asm::sab::payload_token_offsets(&bytes, body, 8, 0x0c)
        .expect("body reference tokens")[0];
    bytes[attribute_ref + 1..attribute_ref + 9].copy_from_slice(&19i64.to_le_bytes());
    bytes.splice(limit..limit, attribute_chain);
    bytes
}

pub(super) fn synthetic_geometry_with_body_truecolor_chain_smbh() -> Vec<u8> {
    let mut attributes = Vec::new();
    t_subident(&mut attributes, "truecolor");
    t_subident(&mut attributes, "adesk");
    t_ident(&mut attributes, "attrib");
    t_attribute_base(&mut attributes, 20, -1, 1);
    attributes.push(0x17);
    attributes.extend_from_slice(&i64::from(0xc2_20_40_60_u32).to_le_bytes());
    t_end(&mut attributes);

    t_subident(&mut attributes, "rgb_color");
    t_subident(&mut attributes, "st");
    t_ident(&mut attributes, "attrib");
    t_attribute_base(&mut attributes, -1, 19, 1);
    for channel in [0.8, 0.7, 0.6, 1.0] {
        t_dbl(&mut attributes, channel);
    }
    t_end(&mut attributes);
    synthetic_geometry_with_body_attribute_chain_smbh(attributes)
}

pub(super) fn synthetic_geometry_with_body_decimal_color_chain_smbh(decimal: &str) -> Vec<u8> {
    let mut attributes = Vec::new();
    t_subident(&mut attributes, "entatt_color");
    t_subident(&mut attributes, "bt");
    t_ident(&mut attributes, "attrib");
    t_attribute_base(&mut attributes, 20, -1, 1);
    push_u8_string(&mut attributes, decimal);
    t_end(&mut attributes);

    t_subident(&mut attributes, "rgb_color");
    t_subident(&mut attributes, "st");
    t_ident(&mut attributes, "attrib");
    t_attribute_base(&mut attributes, -1, 19, 1);
    for channel in [0.8, 0.7, 0.6, 1.0] {
        t_dbl(&mut attributes, channel);
    }
    t_end(&mut attributes);
    synthetic_geometry_with_body_attribute_chain_smbh(attributes)
}

pub(super) fn synthetic_geometry_with_face_color_smbh() -> Vec<u8> {
    let mut bytes = synthetic_geometry_smbh();
    let limit = cadmpeg_asm::asm_header::solved_record_limit(&bytes).expect("history boundary");
    let start = cadmpeg_asm::asm_header::record_stream_start(&bytes).expect("record stream");
    let records = cadmpeg_asm::sab::frame(&bytes, start, limit, 8).expect("generated SAB");
    let face = &records[4];
    let attribute_ref = cadmpeg_asm::sab::payload_token_offsets(&bytes, face, 8, 0x0c)
        .expect("face reference tokens")[0];
    bytes[attribute_ref + 1..attribute_ref + 9].copy_from_slice(&19i64.to_le_bytes());

    let mut attribute = Vec::new();
    t_subident(&mut attribute, "rgb_color");
    t_subident(&mut attribute, "st");
    t_ident(&mut attribute, "attrib");
    t_attribute_base(&mut attribute, -1, -1, 4);
    t_dbl(&mut attribute, 0.15);
    t_dbl(&mut attribute, 0.25);
    t_dbl(&mut attribute, 0.35);
    t_dbl(&mut attribute, 1.0);
    t_end(&mut attribute);
    bytes.splice(limit..limit, attribute);
    bytes
}

pub(super) fn synthetic_geometry_with_mesh_surface_smbh() -> Vec<u8> {
    let mut bytes = synthetic_geometry_smbh();
    let limit = cadmpeg_asm::asm_header::solved_record_limit(&bytes).expect("history boundary");
    let start = cadmpeg_asm::asm_header::record_stream_start(&bytes).expect("record stream");
    let records = cadmpeg_asm::sab::frame(&bytes, start, limit, 8).expect("generated SAB");
    let plane = records
        .iter()
        .find(|record| record.head == "plane")
        .expect("generated plane surface");
    let mut sentinel = Vec::new();
    t_ident(&mut sentinel, "mesh_surface");
    t_end(&mut sentinel);
    bytes.splice(plane.offset..plane.offset + plane.len, sentinel);
    bytes
}

/// Add a generated inline 2D `nubs` pcurve to the first coedge of the base
/// topology fixture. The new record is appended at `RecordTable` index 19.
pub(super) fn synthetic_geometry_with_pcurve_smbh() -> Vec<u8> {
    synthetic_geometry_with_pcurve_block_smbh(generated_planar_pcurve_block())
}

pub(super) fn synthetic_geometry_with_wrapped_ref_pcurve_smbh() -> Vec<u8> {
    let mut bytes = synthetic_geometry_with_pcurve_smbh();
    let opener = bytes
        .windows(b"\x0f\x0d\x0bexp_par_cur".len())
        .position(|window| window == b"\x0f\x0d\x0bexp_par_cur")
        .expect("generated wrapped pcurve subtype");
    let close = bytes[opener..]
        .windows([0x10, 0x0a, 0x0b, 0x0a, 0x0b].len())
        .position(|window| window == [0x10, 0x0a, 0x0b, 0x0a, 0x0b])
        .map(|offset| opener + offset)
        .expect("generated wrapped pcurve subtype close");
    let mut reference = vec![0x0f];
    t_ident(&mut reference, "ref");
    t_long(&mut reference, 0);
    reference.push(0x10);
    bytes.splice(opener..=close, reference);

    let delta = bytes
        .windows(b"delta_state".len())
        .position(|window| window == b"delta_state")
        .unwrap()
        - 2;
    let mut target = Vec::new();
    t_subident(&mut target, "intcurve");
    t_ident(&mut target, "curve");
    t_ref(&mut target, -1);
    t_long(&mut target, -1);
    t_ref(&mut target, -1);
    target.push(0x0f);
    t_ident(&mut target, "int_int_cur");
    target.extend_from_slice(&generated_pcurve_block());
    target.push(0x10);
    t_end(&mut target);
    bytes.splice(delta..delta, target);
    bytes
}

pub(super) fn synthetic_geometry_with_inline_pcurve_on_nurbs_surface_smbh() -> Vec<u8> {
    replace_generated_face_with_nurbs_surface(synthetic_geometry_with_pcurve_smbh())
}

pub(super) fn synthetic_inline_pcurve_with_referenced_support_smbh() -> Vec<u8> {
    let mut bytes = synthetic_geometry_with_inline_pcurve_on_nurbs_surface_smbh();
    let start = asm_header::record_stream_start(&bytes).unwrap();
    let limit = asm_header::solved_record_limit(&bytes).unwrap();
    let records = cadmpeg_asm::sab::frame(&bytes, start, limit, 8).unwrap();
    let asmheader_end = records[0].offset + records[0].len - 1;

    let mut target = vec![0x0f];
    t_ident(&mut target, "int_int_cur");
    target.extend_from_slice(&generated_pcurve_block());
    target.push(0x10);
    bytes.splice(asmheader_end..asmheader_end, target);

    let opener = bytes
        .windows(b"\x0f\x0d\x0bexp_par_cur".len())
        .position(|window| window == b"\x0f\x0d\x0bexp_par_cur")
        .expect("inline pcurve scope");
    let close = bytes[opener..]
        .windows([0x10, 0x0a, 0x0b, 0x0a, 0x0b].len())
        .position(|window| window == [0x10, 0x0a, 0x0b, 0x0a, 0x0b])
        .map(|offset| opener + offset)
        .expect("inline pcurve scope close");
    let mut reference = vec![0x0f];
    t_ident(&mut reference, "ref");
    t_long(&mut reference, 0);
    reference.push(0x10);
    bytes.splice(close..close, reference);
    bytes
}

pub(super) fn replace_generated_face_with_nurbs_surface(mut bytes: Vec<u8>) -> Vec<u8> {
    let planar_pcurve = generated_planar_pcurve_block();
    if let Some(offset) = bytes
        .windows(planar_pcurve.len())
        .position(|window| window == planar_pcurve)
    {
        bytes.splice(
            offset..offset + planar_pcurve.len(),
            generated_pcurve_block(),
        );
    }
    let start = asm_header::record_stream_start(&bytes).unwrap();
    let limit = asm_header::solved_record_limit(&bytes).unwrap();
    let records = cadmpeg_asm::sab::frame(&bytes, start, limit, 8).unwrap();
    let old = &records[6];
    let mut surface = Vec::new();
    t_subident(&mut surface, "spline");
    t_ident(&mut surface, "surface");
    t_ref(&mut surface, -1);
    t_long(&mut surface, -1);
    t_ref(&mut surface, -1);
    surface.extend_from_slice(&generated_surface_block());
    t_end(&mut surface);
    bytes.splice(old.offset..old.offset + old.len, surface);
    bytes
}

pub(super) fn synthetic_geometry_with_ref_pcurve_on_nurbs_surface_smbh() -> Vec<u8> {
    replace_generated_face_with_nurbs_surface(synthetic_geometry_with_ref_pcurve_smbh())
}

pub(super) fn synthetic_geometry_with_short_pcurve_tail_smbh() -> Vec<u8> {
    let mut bytes = synthetic_geometry_with_pcurve_smbh();
    let marker = [0x10, 0x0a, 0x0b, 0x0a, 0x0b, 0x06];
    let tail = bytes
        .windows(marker.len())
        .position(|window| window == marker)
        .expect("generated inline pcurve tail");
    bytes.remove(tail + 1);
    bytes
}

pub(super) fn synthetic_geometry_with_out_of_scope_pcurve_cache_smbh() -> Vec<u8> {
    let mut bytes = synthetic_geometry_with_additional_out_of_scope_pcurve_cache_smbh();
    let subtype = bytes
        .windows(b"exp_par_cur".len())
        .position(|window| window == b"exp_par_cur")
        .expect("generated inline pcurve subtype");
    let cache = bytes[subtype..]
        .windows(b"nubs".len())
        .position(|window| window == b"nubs")
        .map(|offset| subtype + offset)
        .expect("generated inline pcurve cache");
    bytes[cache] = b'x';
    bytes
}

pub(super) fn synthetic_geometry_with_additional_out_of_scope_pcurve_cache_smbh() -> Vec<u8> {
    let mut bytes = synthetic_geometry_with_pcurve_smbh();
    let subtype = bytes
        .windows(b"exp_par_cur".len())
        .position(|window| window == b"exp_par_cur")
        .expect("generated inline pcurve subtype");
    let tail = bytes[subtype..]
        .windows([0x10, 0x0a, 0x0b, 0x0a, 0x0b].len())
        .position(|window| window == [0x10, 0x0a, 0x0b, 0x0a, 0x0b])
        .map(|offset| subtype + offset)
        .expect("generated inline pcurve subtype close");
    bytes.splice(tail + 1..tail + 1, generated_pcurve_block());
    bytes
}

pub(super) fn synthetic_geometry_with_rational_pcurve_smbh() -> Vec<u8> {
    synthetic_geometry_with_pcurve_block_smbh(generated_planar_rational_pcurve_block())
}

pub(super) fn synthetic_geometry_with_pcurve_block_smbh(block: Vec<u8>) -> Vec<u8> {
    let mut bytes = synthetic_geometry_smbh();
    let start = asm_header::record_stream_start(&bytes).unwrap();
    let limit = asm_header::solved_record_limit(&bytes).unwrap();
    let records = cadmpeg_asm::sab::frame(&bytes, start, limit, 8).unwrap();
    let coedge = &records[7];
    let record = &mut bytes[coedge.offset..coedge.offset + coedge.len];
    let pcurve_ref_tag = record.iter().rposition(|b| *b == 0x0c).unwrap();
    record[pcurve_ref_tag + 1..pcurve_ref_tag + 9].copy_from_slice(&19i64.to_le_bytes());

    // Move the coedge's edge endpoints onto the pcurve's neutral surface image.
    // The native plane chart stores neutral `(u, v)` as `(u / 10, v / -10)`.
    for (index, position_cm) in [(16usize, [0.025, 0.05, 0.0]), (17, [0.075, 0.15, 0.0])] {
        let point = &records[index];
        let record = &mut bytes[point.offset..point.offset + point.len];
        let tag = record.iter().position(|b| *b == 0x13).unwrap();
        for (slot, value) in position_cm.iter().copied().enumerate() {
            record[tag + 1 + slot * 8..tag + 9 + slot * 8]
                .copy_from_slice(&f64::to_le_bytes(value));
        }
    }

    let delta = bytes[..]
        .windows(b"delta_state".len())
        .position(|w| w == b"delta_state")
        .unwrap()
        - 2;
    let mut pcurve = Vec::new();
    t_ident(&mut pcurve, "pcurve");
    t_ref(&mut pcurve, -1);
    t_long(&mut pcurve, -1);
    t_ref(&mut pcurve, -1);
    t_long(&mut pcurve, 0);
    pcurve.push(0x0b);
    pcurve.push(0x0f);
    t_ident(&mut pcurve, "exp_par_cur");
    pcurve.extend_from_slice(&block);
    t_dbl(&mut pcurve, 0.001);
    pcurve.push(0x10);
    pcurve.extend_from_slice(&[0x0a, 0x0b, 0x0a, 0x0b]);
    t_dbl(&mut pcurve, -1.0);
    t_dbl(&mut pcurve, 2.0);
    t_end(&mut pcurve);
    bytes.splice(delta..delta, pcurve);
    bytes
}

pub(super) fn synthetic_geometry_with_ref_pcurve_smbh() -> Vec<u8> {
    let mut bytes = synthetic_geometry_smbh();
    let start = asm_header::record_stream_start(&bytes).unwrap();
    let limit = asm_header::solved_record_limit(&bytes).unwrap();
    let records = cadmpeg_asm::sab::frame(&bytes, start, limit, 8).unwrap();
    let coedge = &records[7];
    let record = &mut bytes[coedge.offset..coedge.offset + coedge.len];
    let pcurve_ref_tag = record.iter().rposition(|byte| *byte == 0x0c).unwrap();
    record[pcurve_ref_tag + 1..pcurve_ref_tag + 9].copy_from_slice(&19i64.to_le_bytes());

    let delta = bytes
        .windows(b"delta_state".len())
        .position(|window| window == b"delta_state")
        .unwrap()
        - 2;
    let mut records = Vec::new();
    t_ident(&mut records, "pcurve");
    t_ref(&mut records, -1);
    t_long(&mut records, -1);
    t_ref(&mut records, -1);
    t_long(&mut records, 2);
    t_ref(&mut records, 20);
    t_dbl(&mut records, -2.0);
    t_dbl(&mut records, 4.0);
    t_end(&mut records);
    t_subident(&mut records, "intcurve");
    t_ident(&mut records, "curve");
    t_ref(&mut records, -1);
    t_long(&mut records, -1);
    t_ref(&mut records, -1);
    records.extend_from_slice(&generated_curve_block());
    records.extend_from_slice(&generated_planar_pcurve_block());
    t_end(&mut records);
    bytes.splice(delta..delta, records);
    bytes
}

pub(super) fn with_pcurve_discriminator(mut bytes: Vec<u8>, discriminator: i64) -> Vec<u8> {
    let start = asm_header::record_stream_start(&bytes).unwrap();
    let limit = asm_header::solved_record_limit(&bytes).unwrap();
    let records = cadmpeg_asm::sab::frame(&bytes, start, limit, 8).unwrap();
    let pcurve = records
        .iter()
        .find(|record| record.head == "pcurve")
        .expect("generated pcurve record");
    let offsets = cadmpeg_asm::sab::payload_token_offsets(&bytes, pcurve, 8, 0x04)
        .expect("generated pcurve integer offsets");
    bytes[offsets[1] + 1..offsets[1] + 9].copy_from_slice(&discriminator.to_le_bytes());
    bytes
}

pub(super) fn with_inline_pcurve_non_boolean_wrapper(mut bytes: Vec<u8>) -> Vec<u8> {
    let start = asm_header::record_stream_start(&bytes).unwrap();
    let limit = asm_header::solved_record_limit(&bytes).unwrap();
    let records = cadmpeg_asm::sab::frame(&bytes, start, limit, 8).unwrap();
    let pcurve = records
        .iter()
        .find(|record| record.head == "pcurve")
        .expect("generated pcurve record");
    let integers = cadmpeg_asm::sab::payload_token_offsets(&bytes, pcurve, 8, 0x04)
        .expect("generated pcurve integer offsets");
    let wrapper = integers[1] + 9;
    assert_eq!(bytes[wrapper], 0x0b, "generated inline wrapper boolean");
    bytes.splice(wrapper..=wrapper, [0x02, 0x00]);
    bytes
}

pub(super) fn with_ref_pcurve_companion_name(mut bytes: Vec<u8>, name: &[u8; 8]) -> Vec<u8> {
    let start = asm_header::record_stream_start(&bytes).unwrap();
    let limit = asm_header::solved_record_limit(&bytes).unwrap();
    let records = cadmpeg_asm::sab::frame(&bytes, start, limit, 8).unwrap();
    let pcurve = records
        .iter()
        .find(|record| record.head == "pcurve")
        .expect("generated pcurve record");
    let companion_index = pcurve.ref_at(4).expect("generated ref-form companion");
    let companion = &records[usize::try_from(companion_index).unwrap()];
    let head = bytes[companion.offset..companion.offset + companion.len]
        .windows(b"intcurve".len())
        .position(|window| window == b"intcurve")
        .map(|offset| companion.offset + offset)
        .expect("generated intcurve companion name");
    bytes[head..head + name.len()].copy_from_slice(name);
    bytes
}

pub(super) fn with_ref_pcurve_companion_reversed(mut bytes: Vec<u8>) -> Vec<u8> {
    let start = asm_header::record_stream_start(&bytes).unwrap();
    let limit = asm_header::solved_record_limit(&bytes).unwrap();
    let records = cadmpeg_asm::sab::frame(&bytes, start, limit, 8).unwrap();
    let pcurve = records
        .iter()
        .find(|record| record.head == "pcurve")
        .expect("generated pcurve record");
    let companion_index = pcurve.ref_at(4).expect("generated ref-form companion");
    let companion = &records[usize::try_from(companion_index).unwrap()];
    let offset = bytes[companion.offset..companion.offset + companion.len]
        .windows(b"\x0d\x04nubs".len())
        .position(|window| window == b"\x0d\x04nubs")
        .map(|offset| companion.offset + offset)
        .expect("generated intcurve cache marker");
    bytes.splice(offset..offset, [0x0a]);
    bytes
}

pub(super) fn synthetic_geometry_with_procedural_curve_smbh() -> Vec<u8> {
    let mut bytes = synthetic_geometry_smbh();
    let start = asm_header::record_stream_start(&bytes).unwrap();
    let limit = asm_header::solved_record_limit(&bytes).unwrap();
    let records = cadmpeg_asm::sab::frame(&bytes, start, limit, 8).unwrap();
    let edge = &records[10];
    let record = &mut bytes[edge.offset..edge.offset + edge.len];
    let curve_ref_tag = record.iter().rposition(|byte| *byte == 0x0c).unwrap();
    record[curve_ref_tag + 1..curve_ref_tag + 9].copy_from_slice(&19i64.to_le_bytes());

    let delta = bytes
        .windows(b"delta_state".len())
        .position(|window| window == b"delta_state")
        .unwrap()
        - 2;
    let mut curve = Vec::new();
    t_subident(&mut curve, "intcurve");
    t_ident(&mut curve, "curve");
    t_ref(&mut curve, -1);
    t_long(&mut curve, -1);
    t_ref(&mut curve, -1);
    curve.push(0x0f);
    t_ident(&mut curve, "surf_surf_int_cur");
    curve.extend_from_slice(&generated_curve_block());
    t_dbl(&mut curve, 0.0005);
    curve.push(0x10);
    t_end(&mut curve);
    bytes.splice(delta..delta, curve);
    bytes
}

pub(super) fn synthetic_geometry_with_helix_curve_smbh() -> Vec<u8> {
    let mut bytes = synthetic_geometry_smbh();
    let start = asm_header::record_stream_start(&bytes).unwrap();
    let limit = asm_header::solved_record_limit(&bytes).unwrap();
    let records = cadmpeg_asm::sab::frame(&bytes, start, limit, 8).unwrap();
    let edge = &records[10];
    let offsets = cadmpeg_asm::sab::payload_token_offsets(&bytes, edge, 8, 0x0c)
        .expect("generated edge reference offsets");
    bytes[offsets[5] + 1..offsets[5] + 9].copy_from_slice(&19i64.to_le_bytes());

    let delta = bytes
        .windows(b"delta_state".len())
        .position(|window| window == b"delta_state")
        .unwrap()
        - 2;
    let mut curve = Vec::new();
    t_subident(&mut curve, "intcurve");
    t_ident(&mut curve, "curve");
    t_ref(&mut curve, -1);
    t_long(&mut curve, -1);
    t_ref(&mut curve, -1);
    curve.push(0x0f);
    t_ident(&mut curve, "helix_int_cur");
    curve.push(0x0a);
    t_dbl(&mut curve, 0.0);
    curve.push(0x0a);
    t_dbl(&mut curve, std::f64::consts::TAU);
    t_pos(&mut curve, [1.0, 2.0, 3.0]);
    t_pos(&mut curve, [2.0, 0.0, 0.0]);
    t_pos(&mut curve, [0.0, 2.0, 0.0]);
    t_pos(&mut curve, [0.0, 0.0, 4.0]);
    t_dbl(&mut curve, 0.25);
    t_vec(&mut curve, [0.0, 0.0, 1.0]);
    curve.extend_from_slice(&generated_curve_block());
    t_dbl(&mut curve, 0.0005);
    curve.push(0x10);
    t_end(&mut curve);
    bytes.splice(delta..delta, curve);
    bytes
}

pub(super) fn synthetic_geometry_with_cacheless_helix_curve_smbh() -> Vec<u8> {
    let mut bytes = synthetic_geometry_with_helix_curve_smbh();
    let start = asm_header::record_stream_start(&bytes).unwrap();
    let limit = asm_header::solved_record_limit(&bytes).unwrap();
    let records = cadmpeg_asm::sab::frame(&bytes, start, limit, 8).unwrap();
    let helix = records.iter().find(|record| record.index == 19).unwrap();
    let block = generated_curve_block();
    let relative = bytes[helix.offset..helix.offset + helix.len]
        .windows(block.len())
        .position(|window| window == block)
        .unwrap();
    let cache = helix.offset + relative;
    bytes.drain(cache..cache + block.len() + 9);
    bytes
}

pub(super) fn synthetic_geometry_with_law_curve_smbh() -> Vec<u8> {
    let mut bytes = synthetic_geometry_smbh();
    let start = asm_header::record_stream_start(&bytes).unwrap();
    let limit = asm_header::solved_record_limit(&bytes).unwrap();
    let records = cadmpeg_asm::sab::frame(&bytes, start, limit, 8).unwrap();
    let edge = &records[10];
    let offsets = cadmpeg_asm::sab::payload_token_offsets(&bytes, edge, 8, 0x0c).unwrap();
    bytes[offsets[5] + 1..offsets[5] + 9].copy_from_slice(&19i64.to_le_bytes());
    let delta = bytes
        .windows(b"delta_state".len())
        .position(|window| window == b"delta_state")
        .unwrap()
        - 2;
    let mut curve = Vec::new();
    t_subident(&mut curve, "intcurve");
    t_ident(&mut curve, "curve");
    t_ref(&mut curve, -1);
    t_long(&mut curve, -1);
    t_ref(&mut curve, -1);
    curve.push(0x0f);
    t_ident(&mut curve, "law_int_cur");
    curve.extend_from_slice(&generated_curve_block());
    t_dbl(&mut curve, 0.0005);
    for origin in [[0.0, 0.0, 0.0], [0.0, 0.0, 1.0]] {
        t_ident(&mut curve, "plane");
        t_pos(&mut curve, origin);
        t_vec(&mut curve, [0.0, 0.0, 1.0]);
        t_vec(&mut curve, [1.0, 0.0, 0.0]);
        curve.push(0x0b);
    }
    curve.extend_from_slice(&generated_pcurve_block());
    curve.extend_from_slice(&generated_pcurve_block());
    t_dbl(&mut curve, -1.0);
    t_dbl(&mut curve, 2.0);
    for values in [&[0.25][..], &[][..], &[][..]] {
        append_generated_float_array(&mut curve, values);
    }
    t_long(&mut curve, 0);
    push_u8_string(&mut curve, "primary_law");
    t_long(&mut curve, 1);
    push_u8_string(&mut curve, "EDGE");
    curve.extend_from_slice(&generated_curve_block());
    t_dbl(&mut curve, -0.5);
    t_dbl(&mut curve, 1.5);
    t_long(&mut curve, 2);
    push_u8_string(&mut curve, "null_law");
    push_u8_string(&mut curve, "null_law");
    curve.push(0x10);
    t_end(&mut curve);
    bytes.splice(delta..delta, curve);
    bytes
}

/// Push a `0x15` enum token carrying the signed `int_width`-8 value.
pub(super) fn push_native_enum(bytes: &mut Vec<u8>, value: i64) {
    bytes.push(0x15);
    bytes.extend_from_slice(&value.to_le_bytes());
}

/// Append a vector-serialized `TRANS` law variable: the operator string, four
/// `0x14` vectors, a `0x06` scale, and three bare boolean flags.
pub(super) fn append_transform_vec_variable(bytes: &mut Vec<u8>) {
    push_u8_string(bytes, "TRANS");
    for vector in [
        [1.0, 0.0, 0.0],
        [0.0, 1.0, 0.0],
        [0.0, 0.0, 1.0],
        [0.0, 0.0, 0.0],
    ] {
        t_vec(bytes, vector);
    }
    t_dbl(bytes, 0.1);
    bytes.push(0x0b); // false
    bytes.push(0x0b); // false
    bytes.push(0x0a); // true
}

/// Build the version-stamped `law_int_cur` subtype span (opening `0x0f` through
/// the `0x10` terminator): a `04 <20900> 15 <0>` version prefix, solved cache,
/// two `null_surface` and two `nullbs` carriers, bare-`0b` unbounded interval
/// bounds, three empty discontinuity arrays, and primary/additional formulas —
/// the primary carrying a vector-form `TRANS`, the additional list the fixed
/// four-slot `[null_law, null_law, raw-law, TRANS-wrapped]` shape.
pub(super) fn stamped_law_curve_subtype(primary_name: &str, raw_name: &str) -> Vec<u8> {
    let mut c = Vec::new();
    c.push(0x0f);
    t_ident(&mut c, "law_int_cur");
    t_long(&mut c, 20900);
    push_native_enum(&mut c, 0);
    c.extend_from_slice(&generated_curve_block());
    t_dbl(&mut c, 0.0005);
    t_ident(&mut c, "null_surface");
    t_ident(&mut c, "null_surface");
    t_ident(&mut c, "nullbs");
    t_ident(&mut c, "nullbs");
    c.push(0x0b);
    c.push(0x0b);
    for _ in 0..3 {
        append_generated_float_array(&mut c, &[]);
    }
    t_long(&mut c, 0);
    t_u16_string(&mut c, primary_name);
    t_long(&mut c, 1);
    append_transform_vec_variable(&mut c);
    t_long(&mut c, 4);
    push_u8_string(&mut c, "null_law");
    push_u8_string(&mut c, "null_law");
    t_u16_string(&mut c, raw_name);
    t_long(&mut c, 0);
    push_u8_string(&mut c, "TRANS(VEC(X,X2,X3),TRANS1)");
    t_long(&mut c, 1);
    append_transform_vec_variable(&mut c);
    c.push(0x10);
    c
}

pub(super) fn synthetic_geometry_with_stamped_law_curve_smbh(subtype: &[u8]) -> Vec<u8> {
    let mut bytes = synthetic_geometry_smbh();
    let start = asm_header::record_stream_start(&bytes).unwrap();
    let limit = asm_header::solved_record_limit(&bytes).unwrap();
    let records = cadmpeg_asm::sab::frame(&bytes, start, limit, 8).unwrap();
    let edge = &records[10];
    let offsets = cadmpeg_asm::sab::payload_token_offsets(&bytes, edge, 8, 0x0c).unwrap();
    bytes[offsets[5] + 1..offsets[5] + 9].copy_from_slice(&19i64.to_le_bytes());
    let delta = bytes
        .windows(b"delta_state".len())
        .position(|window| window == b"delta_state")
        .unwrap()
        - 2;
    let mut curve = Vec::new();
    t_subident(&mut curve, "intcurve");
    t_ident(&mut curve, "curve");
    t_ref(&mut curve, -1);
    t_long(&mut curve, -1);
    t_ref(&mut curve, -1);
    curve.extend_from_slice(subtype);
    t_end(&mut curve);
    bytes.splice(delta..delta, curve);
    bytes
}

#[test]
fn stamped_law_intcurve_round_trips_byte_exactly() {
    use cadmpeg_ir::geometry::{CurveGeometry, LawExpression, ProceduralCurveDefinition};

    // Formula names exceed 255 bytes to exercise the u16 (`0x08`) length prefix
    // the serializer selects for long law text.
    let primary_name = format!("TRANS({},TRANS1)", "VEC(X,X2,X3)*COS(X)+".repeat(20));
    let raw_name = "VEC(X,X2,X3)*COS(X)+".repeat(20);
    assert!(primary_name.len() > 255 && raw_name.len() > 255);
    let subtype = stamped_law_curve_subtype(&primary_name, &raw_name);

    let decoded = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(
                &synthetic_geometry_with_stamped_law_curve_smbh(&subtype),
            )),
            &DecodeOptions::default(),
        )
        .expect("stamped law intcurve decode");
    let procedural = decoded
        .ir
        .model
        .procedural_curves
        .iter()
        .find(|curve| matches!(curve.definition, ProceduralCurveDefinition::Law { .. }))
        .expect("stamped law construction");
    let ProceduralCurveDefinition::Law {
        version,
        primary,
        additional,
        ..
    } = &procedural.definition
    else {
        unreachable!()
    };
    let version = version.as_ref().expect("version stamp");
    assert_eq!(version.stamp, 20900);
    assert_eq!(version.post_enum, 0);
    assert_eq!(version.parameter_range, [None, None]);
    assert_eq!(primary.name, primary_name);
    assert!(matches!(
        primary.variables[0],
        LawExpression::TransformVec { .. }
    ));
    assert_eq!(additional.len(), 4);
    assert_eq!(additional[0].name, "null_law");
    assert_eq!(additional[1].name, "null_law");
    assert_eq!(additional[2].name, raw_name);
    assert_eq!(additional[3].name, "TRANS(VEC(X,X2,X3),TRANS1)");
    assert!(matches!(
        additional[3].variables[0],
        LawExpression::TransformVec { .. }
    ));

    // Byte-exact re-emission of the subtype span. The solved cache uses
    // integer-valued control points so the cm->mm scaling round-trip is exact.
    let solved = decoded
        .ir
        .model
        .curves
        .iter()
        .find(|curve| curve.id == procedural.curve)
        .and_then(|curve| match &curve.geometry {
            CurveGeometry::Nurbs(nurbs) => Some(nurbs.clone()),
            _ => None,
        })
        .expect("solved cache");
    let mut regenerated = Vec::new();
    crate::writer::generate::native_geometry::native_procedural_curve(
        &mut regenerated,
        &decoded.ir,
        &procedural.curve,
        &solved,
    )
    .expect("regenerate stamped law curve");
    let inner = regenerated.iter().position(|&b| b == 0x0f).unwrap();
    let span = cadmpeg_asm::nurbs::subtypes::subtype_span(&regenerated, inner, 8).unwrap();
    assert_eq!(span, subtype.as_slice());
}

#[test]
fn legacy_law_intcurve_round_trips_byte_exactly() {
    use cadmpeg_ir::geometry::{CurveGeometry, ProceduralCurveDefinition};

    let smbh = synthetic_geometry_with_law_curve_smbh();
    let decoded = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(&smbh)),
            &DecodeOptions::default(),
        )
        .expect("legacy law intcurve decode");
    let procedural = decoded
        .ir
        .model
        .procedural_curves
        .iter()
        .find(|curve| matches!(curve.definition, ProceduralCurveDefinition::Law { .. }))
        .expect("legacy law construction");
    let ProceduralCurveDefinition::Law { version, .. } = &procedural.definition else {
        unreachable!()
    };
    assert!(version.is_none());

    let original = {
        let marker = smbh
            .windows(b"law_int_cur".len())
            .position(|window| window == b"law_int_cur")
            .unwrap()
            - 3;
        cadmpeg_asm::nurbs::subtypes::subtype_span(&smbh, marker, 8)
            .unwrap()
            .to_vec()
    };
    let solved = decoded
        .ir
        .model
        .curves
        .iter()
        .find(|curve| curve.id == procedural.curve)
        .and_then(|curve| match &curve.geometry {
            CurveGeometry::Nurbs(nurbs) => Some(nurbs.clone()),
            _ => None,
        })
        .expect("solved cache");
    let mut regenerated = Vec::new();
    crate::writer::generate::native_geometry::native_procedural_curve(
        &mut regenerated,
        &decoded.ir,
        &procedural.curve,
        &solved,
    )
    .expect("regenerate legacy law curve");
    let inner = regenerated.iter().position(|&b| b == 0x0f).unwrap();
    let span = cadmpeg_asm::nurbs::subtypes::subtype_span(&regenerated, inner, 8).unwrap();
    assert_eq!(span, original.as_slice());
}

pub(super) fn synthetic_geometry_with_vector_offset_curve_smbh() -> Vec<u8> {
    let mut bytes = synthetic_geometry_smbh();
    let start = asm_header::record_stream_start(&bytes).unwrap();
    let limit = asm_header::solved_record_limit(&bytes).unwrap();
    let records = cadmpeg_asm::sab::frame(&bytes, start, limit, 8).unwrap();
    let edge = &records[10];
    let offsets = cadmpeg_asm::sab::payload_token_offsets(&bytes, edge, 8, 0x0c)
        .expect("generated edge reference offsets");
    bytes[offsets[5] + 1..offsets[5] + 9].copy_from_slice(&19i64.to_le_bytes());

    let delta = bytes
        .windows(b"delta_state".len())
        .position(|window| window == b"delta_state")
        .unwrap()
        - 2;
    let mut curve = Vec::new();
    t_subident(&mut curve, "intcurve");
    t_ident(&mut curve, "curve");
    t_ref(&mut curve, -1);
    t_long(&mut curve, -1);
    t_ref(&mut curve, -1);
    curve.push(0x0f);
    t_ident(&mut curve, "offset_int_cur");
    curve.push(0x0b);
    curve.extend_from_slice(&generated_curve_block());
    t_dbl(&mut curve, -2.0);
    t_dbl(&mut curve, 5.0);
    t_vec(&mut curve, [0.5, -1.0, 2.0]);
    push_u8_string(&mut curve, "source");
    t_long(&mut curve, 7);
    push_u8_string(&mut curve, "offset");
    t_long(&mut curve, 9);
    curve.extend_from_slice(&generated_curve_block());
    t_dbl(&mut curve, 0.0008);
    curve.push(0x10);
    t_end(&mut curve);
    bytes.splice(delta..delta, curve);
    bytes
}

pub(super) fn synthetic_geometry_with_subset_curve_smbh() -> Vec<u8> {
    let mut bytes = synthetic_geometry_smbh();
    let start = asm_header::record_stream_start(&bytes).unwrap();
    let limit = asm_header::solved_record_limit(&bytes).unwrap();
    let records = cadmpeg_asm::sab::frame(&bytes, start, limit, 8).unwrap();
    let edge = &records[10];
    let offsets = cadmpeg_asm::sab::payload_token_offsets(&bytes, edge, 8, 0x0c)
        .expect("generated edge reference offsets");
    bytes[offsets[5] + 1..offsets[5] + 9].copy_from_slice(&19i64.to_le_bytes());
    let delta = bytes
        .windows(b"delta_state".len())
        .position(|window| window == b"delta_state")
        .unwrap()
        - 2;
    let mut curve = Vec::new();
    t_subident(&mut curve, "intcurve");
    t_ident(&mut curve, "curve");
    t_ref(&mut curve, -1);
    t_long(&mut curve, -1);
    t_ref(&mut curve, -1);
    curve.push(0x0f);
    t_ident(&mut curve, "subset_int_cur");
    curve.extend_from_slice(&generated_curve_block());
    t_dbl(&mut curve, -1.5);
    t_dbl(&mut curve, 3.5);
    curve.extend_from_slice(&generated_curve_block());
    t_dbl(&mut curve, 0.0006);
    curve.push(0x10);
    t_end(&mut curve);
    bytes.splice(delta..delta, curve);
    bytes
}

pub(super) fn synthetic_geometry_with_exact_curve_smbh() -> Vec<u8> {
    let mut bytes = synthetic_geometry_smbh();
    let start = asm_header::record_stream_start(&bytes).unwrap();
    let limit = asm_header::solved_record_limit(&bytes).unwrap();
    let records = cadmpeg_asm::sab::frame(&bytes, start, limit, 8).unwrap();
    let edge = &records[10];
    let offsets = cadmpeg_asm::sab::payload_token_offsets(&bytes, edge, 8, 0x0c)
        .expect("generated edge reference offsets");
    bytes[offsets[5] + 1..offsets[5] + 9].copy_from_slice(&19i64.to_le_bytes());
    let delta = bytes
        .windows(b"delta_state".len())
        .position(|window| window == b"delta_state")
        .unwrap()
        - 2;
    let mut curve = Vec::new();
    t_subident(&mut curve, "intcurve");
    t_ident(&mut curve, "curve");
    t_ref(&mut curve, -1);
    t_long(&mut curve, -1);
    t_ref(&mut curve, -1);
    curve.push(0x0f);
    t_ident(&mut curve, "exact_int_cur");
    curve.extend_from_slice(&generated_curve_block());
    t_dbl(&mut curve, 0.0004);
    curve.push(0x10);
    t_end(&mut curve);
    bytes.splice(delta..delta, curve);
    bytes
}

pub(super) fn synthetic_geometry_with_decoy_curve_sense_smbh() -> Vec<u8> {
    let mut bytes = synthetic_geometry_with_exact_curve_smbh();
    let marker = b"\x0f\x0d\x0dexact_int_cur";
    let subtype = bytes
        .windows(marker.len())
        .position(|window| window == marker)
        .expect("generated exact intcurve subtype");
    bytes.splice(subtype..subtype, [0x0a, 0x0b]);
    bytes
}

pub(super) fn with_legacy_subtype(mut bytes: Vec<u8>, modern: &str, legacy: &str) -> Vec<u8> {
    let position = bytes
        .windows(modern.len())
        .position(|window| window == modern.as_bytes())
        .expect("generated modern subtype");
    bytes[position - 1] = legacy.len() as u8;
    bytes.splice(
        position..position + modern.len(),
        legacy.as_bytes().iter().copied(),
    );
    bytes
}

pub(super) fn synthetic_geometry_with_compound_curve_smbh() -> Vec<u8> {
    let mut bytes = synthetic_geometry_smbh();
    let start = asm_header::record_stream_start(&bytes).unwrap();
    let limit = asm_header::solved_record_limit(&bytes).unwrap();
    let records = cadmpeg_asm::sab::frame(&bytes, start, limit, 8).unwrap();
    let edge = &records[10];
    let offsets = cadmpeg_asm::sab::payload_token_offsets(&bytes, edge, 8, 0x0c)
        .expect("generated edge reference offsets");
    bytes[offsets[5] + 1..offsets[5] + 9].copy_from_slice(&19i64.to_le_bytes());
    let delta = bytes
        .windows(b"delta_state".len())
        .position(|window| window == b"delta_state")
        .unwrap()
        - 2;
    let mut curve = Vec::new();
    t_subident(&mut curve, "intcurve");
    t_ident(&mut curve, "curve");
    t_ref(&mut curve, -1);
    t_long(&mut curve, -1);
    t_ref(&mut curve, -1);
    curve.push(0x0f);
    t_ident(&mut curve, "comp_int_cur");
    t_long(&mut curve, 3);
    for value in [0.0, 0.5, 1.0] {
        t_dbl(&mut curve, value);
    }
    t_long(&mut curve, 2);
    t_dbl(&mut curve, -2.0);
    t_dbl(&mut curve, 4.0);
    curve.push(0x0b);
    curve.extend_from_slice(&generated_curve_block());
    curve.extend_from_slice(&generated_curve_block());
    curve.extend_from_slice(&generated_curve_block());
    t_dbl(&mut curve, 0.0003);
    curve.push(0x10);
    t_end(&mut curve);
    bytes.splice(delta..delta, curve);
    bytes
}

pub(super) fn synthetic_geometry_with_two_sided_offset_curve_smbh() -> Vec<u8> {
    let mut bytes = synthetic_geometry_smbh();
    let start = asm_header::record_stream_start(&bytes).unwrap();
    let limit = asm_header::solved_record_limit(&bytes).unwrap();
    let records = cadmpeg_asm::sab::frame(&bytes, start, limit, 8).unwrap();
    let edge = &records[10];
    let offsets = cadmpeg_asm::sab::payload_token_offsets(&bytes, edge, 8, 0x0c)
        .expect("generated edge reference offsets");
    bytes[offsets[5] + 1..offsets[5] + 9].copy_from_slice(&19i64.to_le_bytes());
    let delta = bytes
        .windows(b"delta_state".len())
        .position(|window| window == b"delta_state")
        .unwrap()
        - 2;
    let mut curve = Vec::new();
    t_subident(&mut curve, "intcurve");
    t_ident(&mut curve, "curve");
    t_ref(&mut curve, -1);
    t_long(&mut curve, -1);
    t_ref(&mut curve, -1);
    curve.push(0x0f);
    t_ident(&mut curve, "off_int_cur");
    for name in ["null_surface", "null_surface", "nullbs", "nullbs"] {
        t_ident(&mut curve, name);
    }
    t_dbl(&mut curve, -1.0);
    t_dbl(&mut curve, 2.0);
    t_long(&mut curve, 2);
    t_dbl(&mut curve, 0.25);
    t_dbl(&mut curve, 0.75);
    t_long(&mut curve, 0);
    t_long(&mut curve, 1);
    t_dbl(&mut curve, 0.5);
    curve.push(0x0a);
    t_dbl(&mut curve, -0.2);
    t_dbl(&mut curve, 0.4);
    curve.extend_from_slice(&generated_curve_block());
    t_dbl(&mut curve, 0.0002);
    curve.push(0x10);
    t_end(&mut curve);
    bytes.splice(delta..delta, curve);
    bytes
}

pub(super) fn synthetic_geometry_with_embedded_offset_supports_smbh() -> Vec<u8> {
    let mut bytes = synthetic_geometry_smbh();
    let start = asm_header::record_stream_start(&bytes).unwrap();
    let limit = asm_header::solved_record_limit(&bytes).unwrap();
    let records = cadmpeg_asm::sab::frame(&bytes, start, limit, 8).unwrap();
    let edge = &records[10];
    let offsets = cadmpeg_asm::sab::payload_token_offsets(&bytes, edge, 8, 0x0c)
        .expect("generated edge reference offsets");
    bytes[offsets[5] + 1..offsets[5] + 9].copy_from_slice(&19i64.to_le_bytes());
    let delta = bytes
        .windows(b"delta_state".len())
        .position(|window| window == b"delta_state")
        .unwrap()
        - 2;
    let mut curve = Vec::new();
    t_subident(&mut curve, "intcurve");
    t_ident(&mut curve, "curve");
    t_ref(&mut curve, -1);
    t_long(&mut curve, -1);
    t_ref(&mut curve, -1);
    curve.push(0x0f);
    t_ident(&mut curve, "off_int_cur");
    for _ in 0..2 {
        t_ident(&mut curve, "spline");
        curve.extend_from_slice(&generated_surface_block());
    }
    curve.extend_from_slice(&generated_pcurve_block());
    curve.extend_from_slice(&generated_rational_pcurve_block());
    t_dbl(&mut curve, 0.0);
    t_dbl(&mut curve, 1.0);
    for _ in 0..3 {
        t_long(&mut curve, 0);
    }
    curve.push(0x0b);
    t_dbl(&mut curve, -0.1);
    t_dbl(&mut curve, 0.3);
    curve.extend_from_slice(&generated_curve_block());
    t_dbl(&mut curve, 0.0001);
    curve.push(0x10);
    t_end(&mut curve);
    bytes.splice(delta..delta, curve);
    bytes
}

pub(super) fn synthetic_geometry_with_analytic_offset_supports_smbh() -> Vec<u8> {
    let mut bytes = synthetic_geometry_smbh();
    let start = asm_header::record_stream_start(&bytes).unwrap();
    let limit = asm_header::solved_record_limit(&bytes).unwrap();
    let records = cadmpeg_asm::sab::frame(&bytes, start, limit, 8).unwrap();
    let edge = &records[10];
    let offsets = cadmpeg_asm::sab::payload_token_offsets(&bytes, edge, 8, 0x0c)
        .expect("generated edge reference offsets");
    bytes[offsets[5] + 1..offsets[5] + 9].copy_from_slice(&19i64.to_le_bytes());
    let delta = bytes
        .windows(b"delta_state".len())
        .position(|window| window == b"delta_state")
        .unwrap()
        - 2;
    let mut curve = Vec::new();
    t_subident(&mut curve, "intcurve");
    t_ident(&mut curve, "curve");
    t_ref(&mut curve, -1);
    t_long(&mut curve, -1);
    t_ref(&mut curve, -1);
    curve.push(0x0f);
    t_ident(&mut curve, "off_int_cur");
    t_ident(&mut curve, "cone");
    t_pos(&mut curve, [1.0, 2.0, 3.0]);
    t_vec(&mut curve, [0.0, 0.0, 1.0]);
    t_vec(&mut curve, [1.0, 0.0, 0.0]);
    t_dbl(&mut curve, 0.4);
    curve.extend_from_slice(&[0x0b; 2]);
    t_dbl(&mut curve, -0.5);
    t_dbl(&mut curve, 3.0_f64.sqrt() / 2.0);
    t_dbl(&mut curve, 1.25);
    curve.extend_from_slice(&[0x0b; 5]);
    t_ident(&mut curve, "torus");
    t_pos(&mut curve, [-1.0, 0.5, 2.0]);
    t_vec(&mut curve, [0.0, 1.0, 0.0]);
    t_dbl(&mut curve, 2.5);
    t_dbl(&mut curve, -0.75);
    t_vec(&mut curve, [1.0, 0.0, 0.0]);
    curve.extend_from_slice(&[0x0b; 5]);
    curve.extend_from_slice(&generated_pcurve_block());
    curve.extend_from_slice(&generated_pcurve_block());
    t_dbl(&mut curve, 0.0);
    t_dbl(&mut curve, 1.0);
    for _ in 0..3 {
        t_long(&mut curve, 0);
    }
    curve.push(0x0b);
    t_dbl(&mut curve, -0.15);
    t_dbl(&mut curve, 0.25);
    curve.extend_from_slice(&generated_curve_block());
    t_dbl(&mut curve, 0.0001);
    curve.push(0x10);
    t_end(&mut curve);
    bytes.splice(delta..delta, curve);
    bytes
}

pub(super) fn synthetic_geometry_with_surface_intersection_smbh() -> Vec<u8> {
    let mut bytes = synthetic_geometry_with_analytic_offset_supports_smbh();
    let subtype = bytes
        .windows(b"off_int_cur".len())
        .position(|window| window == b"off_int_cur")
        .expect("generated offset subtype");
    bytes[subtype..subtype + b"int_int_cur".len()].copy_from_slice(b"int_int_cur");
    let solved = bytes
        .windows(b"\x0d\x04nubs".len())
        .rposition(|window| window == b"\x0d\x04nubs")
        .expect("generated solved curve cache");
    bytes[solved - 19] = 0x0a;
    bytes.drain(solved - 18..solved);
    bytes
}

pub(super) fn synthetic_geometry_with_projection_smbh() -> Vec<u8> {
    let mut bytes = synthetic_geometry_with_analytic_offset_supports_smbh();
    let subtype = bytes
        .windows(b"off_int_cur".len())
        .position(|window| window == b"off_int_cur")
        .expect("generated offset subtype");
    bytes[subtype - 1] = b"proj_int_cur".len() as u8;
    bytes.splice(
        subtype..subtype + b"off_int_cur".len(),
        b"proj_int_cur".iter().copied(),
    );
    let solved = bytes
        .windows(b"\x0d\x04nubs".len())
        .rposition(|window| window == b"\x0d\x04nubs")
        .expect("generated solved curve cache");
    bytes[solved - 19] = 0x0a;
    let mut tail = generated_curve_block();
    tail.push(0x0a);
    t_dbl(&mut tail, -2.0);
    t_dbl(&mut tail, 3.0);
    push_u8_string(&mut tail, "surf2");
    bytes.splice(solved - 18..solved, tail);
    bytes
}

pub(super) fn synthetic_geometry_with_early_close_projection_smbh() -> Vec<u8> {
    let mut bytes = synthetic_geometry_with_projection_smbh();
    let solved = bytes
        .windows(b"\x0d\x04nubs".len())
        .rposition(|window| window == b"\x0d\x04nubs")
        .expect("generated solved curve cache");
    let source = bytes[..solved]
        .windows(b"\x0d\x04nubs".len())
        .rposition(|window| window == b"\x0d\x04nubs")
        .expect("generated projection source curve");
    let source_end = source + generated_curve_block().len();
    bytes.splice(source_end..solved, [0x0a, 0x10]);
    let solved = bytes
        .windows(b"\x0d\x04nubs".len())
        .rposition(|window| window == b"\x0d\x04nubs")
        .expect("shifted solved curve cache");
    let fit_end = solved + generated_curve_block().len() + 9;
    assert_eq!(bytes[fit_end], 0x10);
    bytes.remove(fit_end);
    bytes
}

pub(super) fn synthetic_geometry_with_three_surface_intersection_smbh() -> Vec<u8> {
    let mut bytes = synthetic_geometry_with_analytic_offset_supports_smbh();
    let subtype = bytes
        .windows(b"off_int_cur".len())
        .position(|window| window == b"off_int_cur")
        .expect("generated offset subtype");
    bytes[subtype..subtype + b"sss_int_cur".len()].copy_from_slice(b"sss_int_cur");
    let solved = bytes
        .windows(b"\x0d\x04nubs".len())
        .rposition(|window| window == b"\x0d\x04nubs")
        .expect("generated solved curve cache");
    let mut third = Vec::new();
    t_long(&mut third, 7);
    t_ident(&mut third, "sphere");
    t_pos(&mut third, [0.5, 1.0, -2.0]);
    t_dbl(&mut third, -1.25);
    t_vec(&mut third, [1.0, 0.0, 0.0]);
    t_vec(&mut third, [0.0, 0.0, 1.0]);
    third.extend_from_slice(&[0x0b; 5]);
    third.extend_from_slice(&generated_rational_pcurve_block());
    bytes.splice(solved - 19..solved, third);
    bytes
}

pub(super) fn synthetic_geometry_with_surface_curve_smbh(name: &str) -> Vec<u8> {
    let mut bytes = synthetic_geometry_with_surface_intersection_smbh();
    let subtype = bytes
        .windows(b"int_int_cur".len())
        .position(|window| window == b"int_int_cur")
        .expect("generated intersection subtype");
    bytes[subtype - 1] = name.len() as u8;
    bytes.splice(
        subtype..subtype + b"int_int_cur".len(),
        name.as_bytes().iter().copied(),
    );
    let solved = bytes
        .windows(b"\x0d\x04nubs".len())
        .rposition(|window| window == b"\x0d\x04nubs")
        .expect("generated solved curve cache");
    bytes.remove(solved - 1);
    bytes
}

pub(super) fn synthetic_geometry_with_silhouette_smbh(
    name: &str,
    draft_factor: Option<f64>,
) -> Vec<u8> {
    let mut bytes = synthetic_geometry_with_surface_intersection_smbh();
    let subtype = bytes
        .windows(b"int_int_cur".len())
        .position(|window| window == b"int_int_cur")
        .expect("generated intersection subtype");
    bytes[subtype - 1] = name.len() as u8;
    bytes.splice(
        subtype..subtype + b"int_int_cur".len(),
        name.as_bytes().iter().copied(),
    );
    let solved = bytes
        .windows(b"\x0d\x04nubs".len())
        .rposition(|window| window == b"\x0d\x04nubs")
        .expect("generated solved curve cache");
    let mut tail = Vec::new();
    t_ident(&mut tail, "sphere");
    t_pos(&mut tail, [0.0, 0.0, 0.0]);
    t_dbl(&mut tail, 1.5);
    t_vec(&mut tail, [1.0, 0.0, 0.0]);
    t_vec(&mut tail, [0.0, 0.0, 1.0]);
    tail.extend_from_slice(&[0x0b; 5]);
    t_vec(&mut tail, [0.0, -2.0, 0.0]);
    if let Some(draft_factor) = draft_factor {
        t_dbl(&mut tail, draft_factor);
    }
    bytes.splice(solved - 1..solved, tail);
    bytes
}

pub(super) fn synthetic_geometry_with_surface_offset_smbh() -> Vec<u8> {
    let mut bytes = synthetic_geometry_with_analytic_offset_supports_smbh();
    let subtype = bytes
        .windows(b"off_int_cur".len())
        .position(|window| window == b"off_int_cur")
        .expect("generated offset subtype");
    bytes[subtype - 1] = b"off_surf_int_cur".len() as u8;
    bytes.splice(
        subtype..subtype + b"off_int_cur".len(),
        b"off_surf_int_cur".iter().copied(),
    );
    let solved = bytes
        .windows(b"\x0d\x04nubs".len())
        .rposition(|window| window == b"\x0d\x04nubs")
        .expect("generated solved curve cache");
    let mut tail = vec![0x0a];
    for value in [-1.0, 2.0, -3.0, 4.0] {
        t_dbl(&mut tail, value);
    }
    tail.extend_from_slice(&generated_curve_block());
    t_dbl(&mut tail, -0.5);
    t_dbl(&mut tail, 1.5);
    t_dbl(&mut tail, -0.25);
    t_dbl(&mut tail, 0.75);
    t_dbl(&mut tail, 1.25);
    bytes.splice(solved - 19..solved, tail);
    bytes
}

pub(super) fn synthetic_geometry_with_spring_smbh() -> Vec<u8> {
    let mut bytes = synthetic_geometry_with_surface_intersection_smbh();
    let subtype = bytes
        .windows(b"int_int_cur".len())
        .position(|window| window == b"int_int_cur")
        .expect("generated intersection subtype");
    bytes[subtype - 1] = b"spring_int_cur".len() as u8;
    bytes.splice(
        subtype..subtype + b"int_int_cur".len(),
        b"spring_int_cur".iter().copied(),
    );
    let solved = bytes
        .windows(b"\x0d\x04nubs".len())
        .rposition(|window| window == b"\x0d\x04nubs")
        .expect("generated solved curve cache");
    let mut direction = Vec::new();
    direction.push(0x15);
    direction.extend_from_slice(&(-3i64).to_le_bytes());
    bytes.splice(solved..solved, direction);
    bytes
}

pub(super) fn synthetic_geometry_with_null_support_spring_smbh() -> Vec<u8> {
    let mut bytes = synthetic_geometry_smbh();
    let start = asm_header::record_stream_start(&bytes).unwrap();
    let limit = asm_header::solved_record_limit(&bytes).unwrap();
    let records = cadmpeg_asm::sab::frame(&bytes, start, limit, 8).unwrap();
    let edge = &records[10];
    let offsets = cadmpeg_asm::sab::payload_token_offsets(&bytes, edge, 8, 0x0c)
        .expect("generated edge reference offsets");
    bytes[offsets[5] + 1..offsets[5] + 9].copy_from_slice(&19i64.to_le_bytes());
    let delta = bytes
        .windows(b"delta_state".len())
        .position(|window| window == b"delta_state")
        .unwrap()
        - 2;
    let mut curve = Vec::new();
    t_subident(&mut curve, "intcurve");
    t_ident(&mut curve, "curve");
    t_ref(&mut curve, -1);
    t_long(&mut curve, -1);
    t_ref(&mut curve, -1);
    curve.push(0x0f);
    t_ident(&mut curve, "spring_int_cur");
    t_ident(&mut curve, "null_surface");
    for value in [-2.0, 3.0, -4.0, 5.0] {
        t_dbl(&mut curve, value);
    }
    t_ident(&mut curve, "null_surface");
    for value in [-6.0, 7.0, -8.0, 9.0] {
        t_dbl(&mut curve, value);
    }
    t_ident(&mut curve, "nullbs");
    t_dbl(&mut curve, -10.0);
    t_dbl(&mut curve, 11.0);
    t_ident(&mut curve, "nullbs");
    t_dbl(&mut curve, -1.0);
    t_dbl(&mut curve, 2.0);
    t_long(&mut curve, 1);
    t_dbl(&mut curve, 0.25);
    t_long(&mut curve, 0);
    t_long(&mut curve, 2);
    t_dbl(&mut curve, 0.5);
    t_dbl(&mut curve, 0.75);
    curve.push(0x0a);
    curve.push(0x15);
    curve.extend_from_slice(&4i64.to_le_bytes());
    curve.extend_from_slice(&generated_curve_block());
    t_dbl(&mut curve, 0.0004);
    curve.push(0x10);
    t_end(&mut curve);
    bytes.splice(delta..delta, curve);
    bytes
}

/// The cache form `0` head of the shared cache-first intcurve context: the
/// enum, the solved curve cache, and its fit tolerance.
pub(super) fn push_solved_cache_first_head(curve: &mut Vec<u8>) {
    curve.push(0x15);
    curve.extend_from_slice(&0i64.to_le_bytes());
    curve.extend_from_slice(&generated_curve_block());
    t_dbl(curve, 0.0004);
}

/// Splice one cache-first intcurve record built by `head` and `tail` into the
/// synthetic geometry stream and point edge 10 at it.
pub(super) fn synthetic_geometry_with_cache_first_curve_smbh(
    subtype: &str,
    head: fn(&mut Vec<u8>),
    tail: impl FnOnce(&mut Vec<u8>),
) -> Vec<u8> {
    let mut bytes = synthetic_geometry_smbh();
    let start = asm_header::record_stream_start(&bytes).unwrap();
    let limit = asm_header::solved_record_limit(&bytes).unwrap();
    let records = cadmpeg_asm::sab::frame(&bytes, start, limit, 8).unwrap();
    let edge = &records[10];
    let offsets = cadmpeg_asm::sab::payload_token_offsets(&bytes, edge, 8, 0x0c)
        .expect("generated edge reference offsets");
    bytes[offsets[5] + 1..offsets[5] + 9].copy_from_slice(&19i64.to_le_bytes());
    let delta = bytes
        .windows(b"delta_state".len())
        .position(|window| window == b"delta_state")
        .unwrap()
        - 2;
    let mut curve = Vec::new();
    t_subident(&mut curve, "intcurve");
    t_ident(&mut curve, "curve");
    t_ref(&mut curve, -1);
    t_long(&mut curve, -1);
    t_ref(&mut curve, -1);
    curve.push(0x0f);
    t_ident(&mut curve, subtype);
    t_long(&mut curve, 23100);
    head(&mut curve);
    t_ident(&mut curve, "null_surface");
    t_ident(&mut curve, "null_surface");
    t_ident(&mut curve, "nullbs");
    t_ident(&mut curve, "nullbs");
    curve.push(0x0a);
    t_dbl(&mut curve, -1.0);
    curve.push(0x0a);
    t_dbl(&mut curve, 2.0);
    t_long(&mut curve, 0);
    t_long(&mut curve, 0);
    t_long(&mut curve, 0);
    t_long(&mut curve, 7);
    tail(&mut curve);
    curve.push(0x10);
    t_end(&mut curve);
    bytes.splice(delta..delta, curve);
    bytes
}

#[test]
fn generated_cache_first_spring_decodes_and_writes_source_less() {
    use cadmpeg_ir::geometry::ProceduralCurveDefinition;

    let result = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(
                &synthetic_geometry_with_cache_first_curve_smbh(
                    "spring_int_cur",
                    push_solved_cache_first_head,
                    |curve| {
                        curve.push(0x15);
                        curve.extend_from_slice(&4i64.to_le_bytes());
                    },
                ),
            )),
            &DecodeOptions::default(),
        )
        .expect("cache-first spring decode");
    let ProceduralCurveDefinition::Spring {
        context,
        surface_parameter_ranges,
        first_pcurve_parameter_range,
        discontinuity_flag,
        cache_first,
        direction,
    } = &result.ir.model.procedural_curves[0].definition
    else {
        panic!("expected spring construction")
    };
    let form = cache_first.as_ref().expect("cache-first spring form");
    assert_eq!(form.revision, 23100);
    assert_eq!(form.solved_range, [Some(-1.0), Some(2.0)]);
    assert_eq!(form.extension, 7);
    assert_eq!(*direction, 4);
    assert!(!discontinuity_flag);
    assert_eq!(*surface_parameter_ranges, [None, None]);
    assert_eq!(*first_pcurve_parameter_range, None);
    assert_eq!(context.parameter_range, [-1.0, 2.0]);

    let mut source_less = result.ir;
    source_less.source = None;
    source_less.set_native_unknowns("f3d", &[]).unwrap();
    let mut encoded = Vec::new();
    F3dCodec
        .plan(cadmpeg_ir::codec::EncodeInput {
            ir: &source_less,
            fidelity: None,
        })
        .and_then(|plan| plan.write_to(&mut encoded))
        .expect("source-less cache-first spring encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less cache-first spring round trip");
    assert_eq!(
        round_trip.ir.model.procedural_curves[0].definition,
        source_less.model.procedural_curves[0].definition
    );
}

#[test]
fn generated_cache_first_parametric_curve_decodes_and_writes_source_less() {
    use cadmpeg_ir::geometry::{ProceduralCurveDefinition, SurfaceCurveFamily};

    let result = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(
                &synthetic_geometry_with_cache_first_curve_smbh(
                    "par_int_cur",
                    push_solved_cache_first_head,
                    |curve| {
                        curve.push(0x0a);
                        curve.push(0x0b);
                    },
                ),
            )),
            &DecodeOptions::default(),
        )
        .expect("cache-first parametric decode");
    let ProceduralCurveDefinition::SurfaceCurve {
        family,
        context,
        tail,
    } = &result.ir.model.procedural_curves[0].definition
    else {
        panic!("expected surface-curve construction")
    };
    assert_eq!(*family, SurfaceCurveFamily::Parametric);
    let tail = tail.as_ref().expect("cache-first parametric tail");
    assert_eq!(tail.revision, 23100);
    assert_eq!(tail.extension, 7);
    assert!(tail.flag);
    assert_eq!(tail.second_flag, Some(false));
    assert_eq!(tail.solved_range, [Some(-1.0), Some(2.0)]);
    assert_eq!(context.parameter_range, [-1.0, 2.0]);

    let mut source_less = result.ir;
    source_less.source = None;
    source_less.set_native_unknowns("f3d", &[]).unwrap();
    let mut encoded = Vec::new();
    F3dCodec
        .plan(cadmpeg_ir::codec::EncodeInput {
            ir: &source_less,
            fidelity: None,
        })
        .and_then(|plan| plan.write_to(&mut encoded))
        .expect("source-less cache-first parametric encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less cache-first parametric round trip");
    assert_eq!(
        round_trip.ir.model.procedural_curves[0].definition,
        source_less.model.procedural_curves[0].definition
    );
}

#[test]
fn generated_cache_first_surface_offset_decodes_and_writes_source_less() {
    use cadmpeg_ir::geometry::ProceduralCurveDefinition;

    let result = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(
                &synthetic_geometry_with_cache_first_curve_smbh(
                    "off_surf_int_cur",
                    push_solved_cache_first_head,
                    |curve| {
                        for value in [-1.0, 2.0, -3.0, 4.0] {
                            curve.push(0x0a);
                            t_dbl(curve, value);
                        }
                        curve.extend_from_slice(&generated_curve_block());
                        curve.push(0x0b);
                        curve.push(0x0b);
                        curve.push(0x0a);
                        t_dbl(curve, -0.5);
                        curve.push(0x0a);
                        t_dbl(curve, 1.5);
                        t_dbl(curve, -0.25);
                        t_dbl(curve, 0.75);
                        t_dbl(curve, 1.25);
                    },
                ),
            )),
            &DecodeOptions::default(),
        )
        .expect("cache-first surface-offset decode");
    let ProceduralCurveDefinition::SurfaceOffset {
        cache_first,
        base_u_range,
        base_v_range,
        base_endpoints,
        base_range,
        distance,
        shift,
        scale,
        ..
    } = &result.ir.model.procedural_curves[0].definition
    else {
        panic!("expected surface-offset construction")
    };
    let form = cache_first
        .as_ref()
        .expect("cache-first surface-offset form");
    assert_eq!(form.revision, 23100);
    assert_eq!(form.extension, 7);
    assert_eq!(*base_u_range, [-1.0, 2.0]);
    assert_eq!(*base_v_range, [-3.0, 4.0]);
    assert_eq!(*base_endpoints, [None, None]);
    assert_eq!(*base_range, [-0.5, 1.5]);
    assert_eq!(*distance, -2.5);
    assert_eq!(*shift, 0.75);
    assert_eq!(*scale, 1.25);

    let mut source_less = result.ir;
    source_less.source = None;
    source_less.set_native_unknowns("f3d", &[]).unwrap();
    let mut encoded = Vec::new();
    F3dCodec
        .plan(cadmpeg_ir::codec::EncodeInput {
            ir: &source_less,
            fidelity: None,
        })
        .and_then(|plan| plan.write_to(&mut encoded))
        .expect("source-less cache-first surface-offset encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less cache-first surface-offset round trip");
    let mut expected = source_less.model.procedural_curves[0].definition.clone();
    let mut actual = round_trip.ir.model.procedural_curves[0].definition.clone();
    let (
        ProceduralCurveDefinition::SurfaceOffset {
            base: expected_base,
            ..
        },
        ProceduralCurveDefinition::SurfaceOffset {
            base: actual_base, ..
        },
    ) = (&mut expected, &mut actual)
    else {
        panic!("expected surface-offset round trip")
    };
    let round_trip_base = actual_base.clone();
    *actual_base = expected_base.clone();
    assert_eq!(actual, expected);
    assert!(round_trip
        .ir
        .model
        .curves
        .iter()
        .any(|curve| curve.id == round_trip_base));
}

pub(super) fn t_str(b: &mut Vec<u8>, s: &str) {
    b.push(0x07);
    b.push(u8::try_from(s.len()).expect("short string"));
    b.extend_from_slice(s.as_bytes());
}

pub(super) fn push_revision_surface_tail(surface: &mut Vec<u8>) {
    surface.push(0x15);
    surface.extend_from_slice(&0i64.to_le_bytes());
    surface.extend_from_slice(&generated_surface_block());
    t_dbl(surface, 0.002);
    for _ in 0..6 {
        t_long(surface, 0);
    }
    surface.push(0x0b);
}

/// The shared revision-gated surface tail in cache form `2`: no solved cache
/// and no fit tolerance, the U parameter interval, the V parameter interval,
/// then the U closure, V closure, U singularity, and V singularity enums.
pub(super) fn push_parameterized_revision_surface_tail(surface: &mut Vec<u8>) {
    surface.push(0x15);
    surface.extend_from_slice(&2i64.to_le_bytes());
    // U interval: present lower bound, absent upper bound.
    surface.push(0x0a);
    t_dbl(surface, 0.25);
    surface.push(0x0b);
    // V interval: both bounds present.
    for value in [-1.5, 3.5] {
        surface.push(0x0a);
        t_dbl(surface, value);
    }
    for value in [1, 0, 2, 3] {
        surface.push(0x15);
        surface.extend_from_slice(&i64::from(value).to_le_bytes());
    }
    for _ in 0..6 {
        t_long(surface, 0);
    }
    surface.push(0x0b);
}

/// Replace record 9 of the mixed stream with a revision-gated spline-surface
/// record whose subtype body is built by `body`.
pub(super) fn synthetic_revision_surface_smbh(
    subtype: &str,
    body: impl FnOnce(&mut Vec<u8>),
) -> Vec<u8> {
    let mut bytes = synthetic_mixed_smbh();
    let start = asm_header::record_stream_start(&bytes).unwrap();
    let limit = asm_header::solved_record_limit(&bytes).unwrap();
    let records = cadmpeg_asm::sab::frame(&bytes, start, limit, 8).unwrap();
    let old = &records[9];
    let mut surface = Vec::new();
    t_subident(&mut surface, "spline");
    t_ident(&mut surface, "surface");
    t_ref(&mut surface, -1);
    t_long(&mut surface, -1);
    t_ref(&mut surface, -1);
    surface.push(0x0f);
    t_ident(&mut surface, subtype);
    t_long(&mut surface, 23100);
    body(&mut surface);
    surface.push(0x10);
    t_end(&mut surface);
    bytes.splice(old.offset..old.offset + old.len, surface);
    bytes
}

pub(super) fn scrubbed_definition(
    definition: &cadmpeg_ir::geometry::ProceduralSurfaceDefinition,
) -> String {
    let text = serde_json::to_string(definition).expect("definition JSON");
    let mut out = String::with_capacity(text.len());
    let mut in_index = false;
    for c in text.chars() {
        if in_index && c.is_ascii_digit() {
            continue;
        }
        in_index = c == '#';
        out.push(c);
    }
    out
}

pub(super) fn assert_revision_surface_round_trip(smbh: Vec<u8>, expected_kind: &str) {
    let result = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(&smbh)),
            &DecodeOptions::default(),
        )
        .expect("revision surface decode");
    let procedural = result
        .ir
        .model
        .procedural_surfaces
        .first()
        .expect("revision surface construction");
    let expected = scrubbed_definition(&procedural.definition);
    let kind = serde_json::to_value(&procedural.definition).expect("kind")["kind"]
        .as_str()
        .expect("kind string")
        .to_string();
    assert_eq!(kind, expected_kind);
    let mut source_less = result.ir;
    source_less.source = None;
    source_less.set_native_unknowns("f3d", &[]).unwrap();
    let mut encoded = Vec::new();
    F3dCodec
        .plan(cadmpeg_ir::codec::EncodeInput {
            ir: &source_less,
            fidelity: None,
        })
        .and_then(|plan| plan.write_to(&mut encoded))
        .expect("source-less revision surface encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less revision surface round trip");
    let actual = scrubbed_definition(
        &round_trip
            .ir
            .model
            .procedural_surfaces
            .first()
            .expect("round-trip construction")
            .definition,
    );
    assert_eq!(actual, expected);
}

#[test]
fn generated_revision_offset_surface_round_trips() {
    let smbh = synthetic_revision_surface_smbh("off_spl_sur", |surface| {
        t_ident(surface, "spline");
        surface.extend_from_slice(&generated_surface_block());
        surface.push(0x0a);
        t_dbl(surface, -1.0);
        surface.push(0x0b);
        surface.push(0x0a);
        t_dbl(surface, 2.0);
        surface.push(0x0b);
        t_dbl(surface, 0.3);
        for flag in [false, true, false, false] {
            surface.push(if flag { 0x0a } else { 0x0b });
        }
        push_revision_surface_tail(surface);
    });
    assert_revision_surface_round_trip(smbh.clone(), "offset");

    // The revision-gated layout shares byte positions with the pre-revision
    // U/V sense enums but no grammar, so its four-boolean carrier run travels
    // in the revision form and leaves the enum slots empty.
    let result = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(&smbh)),
            &DecodeOptions::default(),
        )
        .expect("revision offset decode");
    let ProceduralSurfaceDefinition::Offset {
        u_sense,
        v_sense,
        extension_flags,
        revision_form,
        ..
    } = &result.ir.model.procedural_surfaces[0].definition
    else {
        panic!("expected offset surface construction")
    };
    assert_eq!((*u_sense, *v_sense), (None, None));
    assert!(extension_flags.is_empty());
    assert_eq!(
        revision_form.as_ref().expect("revision form").flags,
        [false, true, false, false]
    );
}

#[test]
fn generated_parameterized_revision_offset_surface_round_trips() {
    let smbh = synthetic_revision_surface_smbh("off_spl_sur", |surface| {
        t_ident(surface, "spline");
        surface.extend_from_slice(&generated_surface_block());
        surface.push(0x0a);
        t_dbl(surface, -1.0);
        surface.push(0x0b);
        surface.push(0x0a);
        t_dbl(surface, 2.0);
        surface.push(0x0b);
        t_dbl(surface, 0.3);
        for flag in [false, true, false, false] {
            surface.push(if flag { 0x0a } else { 0x0b });
        }
        push_parameterized_revision_surface_tail(surface);
    });
    assert_revision_surface_round_trip(smbh.clone(), "offset");

    let subtype = synthetic_revision_surface_subtype_span(&smbh);
    let result = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(&smbh)),
            &DecodeOptions::default(),
        )
        .expect("parameterized revision offset decode");
    let procedural = &result.ir.model.procedural_surfaces[0];
    // Cache form 2 stores no fit tolerance.
    assert_eq!(procedural.cache_fit_tolerance, None);
    let cadmpeg_ir::geometry::ProceduralSurfaceDefinition::Offset { revision_form, .. } =
        &procedural.definition
    else {
        panic!("expected offset surface construction")
    };
    let form = revision_form.as_ref().expect("revision form");
    assert_eq!(form.tail_enum, 2);
    let parameterization = form
        .tail_parameterization
        .as_ref()
        .expect("tail parameterization");
    assert_eq!(parameterization.u_interval, [Some(0.25), None]);
    assert_eq!(parameterization.v_interval, [Some(-1.5), Some(3.5)]);
    assert_eq!(
        (parameterization.u_closure, parameterization.v_closure),
        (1, 0)
    );
    assert_eq!(
        (
            parameterization.u_singularity,
            parameterization.v_singularity
        ),
        (2, 3)
    );
    assert_eq!(regenerated_procedural_surface_span(&result.ir), subtype);
}

#[test]
fn generated_revision_orthogonal_taper_round_trips() {
    let smbh = synthetic_revision_surface_smbh("ortho_spl_sur", |surface| {
        t_ident(surface, "spline");
        surface.extend_from_slice(&generated_surface_block());
        surface.extend_from_slice(&[0x0b; 4]);
        surface.extend_from_slice(&generated_curve_block());
        surface.push(0x0a);
        t_dbl(surface, -1.0);
        surface.push(0x0a);
        t_dbl(surface, 2.0);
        surface.extend_from_slice(&generated_pcurve_block());
        t_dbl(surface, 0.5);
        push_revision_surface_tail(surface);
        surface.push(0x0a);
    });
    assert_revision_surface_round_trip(smbh, "taper");
}

#[test]
fn generated_revision_orthogonal_taper_decodes_sense_true() {
    let smbh = synthetic_revision_surface_smbh("ortho_spl_sur", |surface| {
        t_ident(surface, "spline");
        surface.extend_from_slice(&generated_surface_block());
        surface.extend_from_slice(&[0x0b; 4]);
        surface.extend_from_slice(&generated_curve_block());
        surface.push(0x0a);
        t_dbl(surface, -1.0);
        surface.push(0x0a);
        t_dbl(surface, 2.0);
        surface.extend_from_slice(&generated_pcurve_block());
        t_dbl(surface, 0.5);
        push_revision_surface_tail(surface);
        // Trailing orthogonal-sense logical set true.
        surface.push(0x0a);
    });
    let result = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(&smbh)),
            &DecodeOptions::default(),
        )
        .expect("ortho revision decode");
    let definition = &result
        .ir
        .model
        .procedural_surfaces
        .first()
        .expect("ortho construction")
        .definition;
    let cadmpeg_ir::geometry::ProceduralSurfaceDefinition::Taper { taper, .. } = definition else {
        panic!("expected taper definition, got {definition:?}");
    };
    assert_eq!(
        *taper,
        cadmpeg_ir::geometry::TaperSurfaceKind::Orthogonal { sense: true }
    );
}

#[test]
fn generated_revision_sweep_surface_round_trips() {
    let smbh = synthetic_revision_surface_smbh("sweep_sur", |surface| {
        surface.push(0x0b);
        t_long(surface, -1);
        surface.extend_from_slice(&generated_curve_block());
        surface.extend_from_slice(&[0x0b, 0x0b]);
        surface.push(0x0a);
        t_dbl(surface, 0.0);
        surface.push(0x0a);
        t_dbl(surface, 1.0);
        surface.push(0x0b);
        t_pos(surface, [1.0, 2.0, 3.0]);
        t_vec(surface, [0.0, 0.0, 1.0]);
        t_vec(surface, [1.0, 0.0, 0.0]);
        t_vec(surface, [0.0, 1.0, 0.0]);
        t_long(surface, 1);
        surface.push(0x0b);
        surface.extend_from_slice(&generated_curve_block());
        surface.extend_from_slice(&[0x0b, 0x0b]);
        surface.push(0x0a);
        t_dbl(surface, 0.0);
        surface.push(0x0a);
        t_dbl(surface, 0.5);
        t_dbl(surface, 0.0);
        surface.push(0x0b);
        t_str(surface, "MTRAIL(EDGE1)");
        t_long(surface, 1);
        t_str(surface, "EDGE");
        surface.extend_from_slice(&generated_curve_block());
        surface.extend_from_slice(&[0x0b, 0x0b]);
        t_dbl(surface, 0.0);
        t_dbl(surface, 1.0);
        surface.push(0x0b);
        push_revision_surface_tail(surface);
    });
    assert_revision_surface_round_trip(smbh, "sweep");
}

/// A revision-gated `loft_spl_sur` body holding one section entry with one
/// profile member. `type_code` selects the member payload: a nonzero type
/// stores the support surface, one nullable pcurve, and the first flag; a zero
/// type stores two nullable pcurve slots and no first flag. `asm_extension`
/// carries the ASM integer only when the stream save format stores it, and
/// `tail` writes the shared revision-gated surface tail in the cache form
/// under test.
pub(super) fn push_revision_loft_body(
    surface: &mut Vec<u8>,
    type_code: i64,
    asm_extension: Option<i64>,
    tail: fn(&mut Vec<u8>),
) {
    t_long(surface, 1);
    t_dbl(surface, 0.0);
    t_long(surface, 1);
    t_long(surface, type_code);
    surface.extend_from_slice(&generated_curve_block());
    surface.extend_from_slice(&[0x0b, 0x0b]);
    if type_code == 0 {
        surface.extend_from_slice(&generated_pcurve_block());
        t_ident(surface, "nullbs");
    } else {
        t_ident(surface, "null_surface");
        t_ident(surface, "nullbs");
        surface.push(0x0b);
    }
    if let Some(value) = asm_extension {
        t_long(surface, value);
    }
    t_long(surface, 213);
    t_long(surface, 1);
    t_long(surface, 1);
    for value in [0.0, 1.0, 0.25, 0.75, 0.5, 1.5] {
        t_dbl(surface, value);
    }
    surface.push(0x0b);
    t_ident(surface, "null_curve");
    t_long(surface, 0);
    t_long(surface, -1);
    t_long(surface, 0);
    for value in [0.0, 1.0, 0.0, 1.0] {
        surface.push(0x0a);
        t_dbl(surface, value);
    }
    surface.extend_from_slice(&[0x0b; 4]);
    t_long(surface, 0);
    t_long(surface, 0);
    tail(surface);
}

/// Rewrite a `BinaryFile8` stream's save-format version word.
pub(super) fn with_save_format(mut smbh: Vec<u8>, version: u32) -> Vec<u8> {
    smbh[15..19].copy_from_slice(&version.to_le_bytes());
    smbh
}

/// The single revision-gated profile member of a decoded loft construction.
pub(super) fn decoded_revision_loft_member(
    ir: &cadmpeg_ir::document::CadIr,
) -> &cadmpeg_ir::geometry::LoftProfileMember {
    let cadmpeg_ir::geometry::ProceduralSurfaceDefinition::Loft {
        sections,
        revision_form,
        ..
    } = &ir
        .model
        .procedural_surfaces
        .first()
        .expect("revision loft construction")
        .definition
    else {
        panic!("expected a loft construction")
    };
    assert!(revision_form.is_some());
    &sections[0].entries[0].profile[0]
}

/// Byte-exact re-emission of the decoded construction's subtype span.
pub(super) fn regenerated_procedural_surface_span(ir: &cadmpeg_ir::document::CadIr) -> Vec<u8> {
    let procedural = ir
        .model
        .procedural_surfaces
        .first()
        .expect("procedural construction");
    let surface = ir
        .model
        .surfaces
        .iter()
        .find(|surface| surface.id == procedural.surface)
        .expect("solved surface");
    let cadmpeg_ir::geometry::SurfaceGeometry::Nurbs(cache) = &surface.geometry else {
        panic!("expected a solved NURBS cache")
    };
    let mut bytes = Vec::new();
    crate::writer::generate::native_geometry::native_procedural_surface(
        &mut bytes, ir, surface, cache,
    )
    .expect("regenerate procedural surface");
    let inner = bytes
        .iter()
        .position(|&byte| byte == 0x0f)
        .expect("subtype opening");
    cadmpeg_asm::nurbs::subtypes::subtype_span(&bytes, inner, 8)
        .expect("subtype span")
        .to_vec()
}

/// The subtype span of the synthetic stream's revision-gated surface record.
pub(super) fn synthetic_revision_surface_subtype_span(smbh: &[u8]) -> Vec<u8> {
    let start = asm_header::record_stream_start(smbh).unwrap();
    let limit = asm_header::solved_record_limit(smbh).unwrap();
    let records = cadmpeg_asm::sab::frame(smbh, start, limit, 8).unwrap();
    let record = &records[9];
    let slice = &smbh[record.offset..record.offset + record.len];
    let inner = slice.iter().position(|&byte| byte == 0x0f).unwrap();
    cadmpeg_asm::nurbs::subtypes::subtype_span(slice, inner, 8)
        .unwrap()
        .to_vec()
}

#[test]
fn generated_revision_loft_surface_round_trips() {
    let smbh = synthetic_revision_surface_smbh("loft_spl_sur", |surface| {
        push_revision_loft_body(surface, 1, Some(-1), push_revision_surface_tail);
    });
    assert_revision_surface_round_trip(smbh, "loft");
}

/// The parameterization the shared form-`2` tail builder writes.
pub(super) fn assert_parameterized_tail(
    tail_enum: i64,
    parameterization: Option<&cadmpeg_ir::geometry::RevisionSurfaceParameterization>,
) {
    assert_eq!(tail_enum, 2);
    let parameterization = parameterization.expect("tail parameterization");
    assert_eq!(parameterization.u_interval, [Some(0.25), None]);
    assert_eq!(parameterization.v_interval, [Some(-1.5), Some(3.5)]);
    assert_eq!(
        (parameterization.u_closure, parameterization.v_closure),
        (1, 0)
    );
    assert_eq!(
        (
            parameterization.u_singularity,
            parameterization.v_singularity
        ),
        (2, 3)
    );
}

#[test]
fn generated_parameterized_revision_loft_surface_round_trips() {
    let smbh = synthetic_revision_surface_smbh("loft_spl_sur", |surface| {
        push_revision_loft_body(
            surface,
            1,
            Some(-1),
            push_parameterized_revision_surface_tail,
        );
    });
    assert_revision_surface_round_trip(smbh.clone(), "loft");

    let result = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(&smbh)),
            &DecodeOptions::default(),
        )
        .expect("parameterized revision loft decode");
    let procedural = &result.ir.model.procedural_surfaces[0];
    // Cache form 2 stores no fit tolerance.
    assert_eq!(procedural.cache_fit_tolerance, None);
    let cadmpeg_ir::geometry::ProceduralSurfaceDefinition::Loft { revision_form, .. } =
        &procedural.definition
    else {
        panic!("expected a loft construction")
    };
    let form = revision_form.as_ref().expect("revision form");
    assert_parameterized_tail(form.tail_enum, form.tail_parameterization.as_ref());
}

#[test]
fn revision_loft_member_omits_the_asm_integer_in_an_early_save_format_stream() {
    // Save format 22600: the constraint subdata follows the first flag with no
    // ASM integer between them.
    let smbh = with_save_format(
        synthetic_revision_surface_smbh("loft_spl_sur", |surface| {
            push_revision_loft_body(surface, 1, None, push_revision_surface_tail);
        }),
        22600,
    );
    let subtype = synthetic_revision_surface_subtype_span(&smbh);

    let decoded = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(&smbh)),
            &DecodeOptions::default(),
        )
        .expect("early-era revision loft decode");
    let member = decoded_revision_loft_member(&decoded.ir);
    assert_eq!(member.type_code, 1);
    assert_eq!(member.data.first_flag, Some(false));
    assert_eq!(member.data.asm_extension, None);
    assert_eq!(member.data.secondary_pcurve, None);
    assert_eq!(regenerated_procedural_surface_span(&decoded.ir), subtype);
}

#[test]
fn revision_loft_type_zero_member_stores_two_pcurve_slots() {
    let smbh = synthetic_revision_surface_smbh("loft_spl_sur", |surface| {
        push_revision_loft_body(surface, 0, Some(-1), push_revision_surface_tail);
    });
    let decoded = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(&smbh)),
            &DecodeOptions::default(),
        )
        .expect("type-zero revision loft decode");
    let member = decoded_revision_loft_member(&decoded.ir);
    assert_eq!(member.type_code, 0);
    assert_eq!(member.data.surface, None);
    assert!(member.data.pcurve.is_some());
    assert_eq!(member.data.secondary_pcurve, None);
    assert_eq!(member.data.first_flag, None);
    assert_eq!(member.data.asm_extension, Some(-1));
    assert_revision_surface_round_trip(smbh, "loft");
}

pub(super) fn synthetic_geometry_with_deformable_curve_smbh(mode: i64) -> Vec<u8> {
    synthetic_geometry_with_cache_first_curve_smbh(
        "defm_int_cur",
        push_solved_cache_first_head,
        |curve| {
            curve.extend_from_slice(&generated_curve_block());
            curve.push(0x0a);
            t_dbl(curve, 0.0);
            curve.push(0x0a);
            t_dbl(curve, 1.0);
            t_long(curve, mode);
            match mode {
                8 => {
                    for vector in [
                        [1.0, 2.0, 3.0],
                        [4.0, 5.0, 6.0],
                        [7.0, 8.0, 9.0],
                        [10.0, 11.0, 12.0],
                    ] {
                        t_vec(curve, vector);
                    }
                    t_long(curve, 2);
                    for value in [-1.0, 0.25, 2.0, 3.5] {
                        t_dbl(curve, value);
                    }
                }
                3 => {
                    for vector in [
                        [1.0, 2.0, 3.0],
                        [4.0, 5.0, 6.0],
                        [7.0, 8.0, 9.0],
                        [10.0, 11.0, 12.0],
                    ] {
                        t_vec(curve, vector);
                    }
                    t_dbl(curve, 0.5);
                    curve.extend_from_slice(&[0x0a, 0x0b, 0x0a]);
                    t_pos(curve, [13.0, 14.0, 15.0]);
                    for vector in [[16.0, 17.0, 18.0], [19.0, 20.0, 21.0]] {
                        t_vec(curve, vector);
                    }
                    t_dbl(curve, 1.5);
                    curve.extend_from_slice(&[0x0b, 0x0a]);
                    for value in [2.5, 3.5, 4.5] {
                        t_dbl(curve, value);
                    }
                    curve.extend_from_slice(&[0x0a, 0x0b, 0x0a, 0x0b, 0x0a]);
                    t_dbl(curve, 5.5);
                    t_long(curve, 6);
                }
                _ => unreachable!(),
            }
        },
    )
}

pub(super) fn synthetic_geometry_with_attribute_smbh() -> Vec<u8> {
    let mut bytes = synthetic_geometry_smbh();
    let start = asm_header::record_stream_start(&bytes).unwrap();
    let limit = asm_header::solved_record_limit(&bytes).unwrap();
    let records = cadmpeg_asm::sab::frame(&bytes, start, limit, 8).unwrap();
    let body = &records[1];
    let record = &mut bytes[body.offset..body.offset + body.len];
    let attribute_ref = record.iter().position(|byte| *byte == 0x0c).unwrap();
    record[attribute_ref + 1..attribute_ref + 9].copy_from_slice(&19i64.to_le_bytes());

    let delta = bytes
        .windows(b"delta_state".len())
        .position(|window| window == b"delta_state")
        .unwrap()
        - 2;
    let mut attribute = Vec::new();
    t_subident(&mut attribute, "ATTRIB_CUSTOM");
    t_ident(&mut attribute, "attrib");
    t_ref(&mut attribute, 20);
    push_u8_string(&mut attribute, "generic_tag_attrib_def");
    for value in [3, 3, -1] {
        t_long(&mut attribute, value);
    }
    push_u8_string(&mut attribute, "generic_tag_attrib_def ");
    t_long(&mut attribute, 3);
    for (kind, id, reference) in [(3, "311", 6), (4, "900", 42), (3, "322", 7)] {
        t_long(&mut attribute, kind);
        push_u8_string(&mut attribute, id);
        for value in [reference, 0, 0] {
            t_long(&mut attribute, value);
        }
    }
    t_end(&mut attribute);
    t_subident(&mut attribute, "ATTRIB_CUSTOM");
    t_ident(&mut attribute, "attrib");
    t_ref(&mut attribute, -1);
    push_u8_string(&mut attribute, "Timestamp_attrib_def");
    t_long(&mut attribute, 1);
    t_dbl(&mut attribute, 1_579_392_000_000_007.0);
    t_end(&mut attribute);
    bytes.splice(delta..delta, attribute);
    bytes
}

/// One `sketch_attrib_def` payload form: the form selector the third header
/// integer carries and the members that follow it.
pub(super) enum SketchLinkForm<'a> {
    /// Form `3`: the members as one tagged ASCII field.
    Tagged(&'a str),
    /// Form `2` or `0`: the members as integers.
    Integers(i64, &'a [i64]),
}

pub(super) fn synthetic_geometry_with_sketch_link_smbh(form: SketchLinkForm<'_>) -> Vec<u8> {
    let mut bytes = synthetic_geometry_smbh();
    let start = asm_header::record_stream_start(&bytes).unwrap();
    let limit = asm_header::solved_record_limit(&bytes).unwrap();
    let records = cadmpeg_asm::sab::frame(&bytes, start, limit, 8).unwrap();
    let coedge = &records[7];
    let record = &mut bytes[coedge.offset..coedge.offset + coedge.len];
    let attribute_ref = record.iter().position(|byte| *byte == 0x0c).unwrap();
    record[attribute_ref + 1..attribute_ref + 9].copy_from_slice(&19i64.to_le_bytes());

    let delta = bytes
        .windows(b"delta_state".len())
        .position(|window| window == b"delta_state")
        .unwrap()
        - 2;
    let mut attribute = Vec::new();
    t_subident(&mut attribute, "ATTRIB_CUSTOM");
    t_ident(&mut attribute, "attrib");
    t_ref(&mut attribute, -1);
    push_u8_string(&mut attribute, "sketch_attrib_def");
    let (selector, members) = match form {
        SketchLinkForm::Tagged(_) => (3, &[][..]),
        SketchLinkForm::Integers(selector, members) => (selector, members),
    };
    for value in [1, 1, selector] {
        t_long(&mut attribute, value);
    }
    match form {
        SketchLinkForm::Tagged(tuple) => push_u8_string(&mut attribute, tuple),
        SketchLinkForm::Integers(..) => {
            for value in members {
                t_long(&mut attribute, *value);
            }
        }
    }
    t_end(&mut attribute);
    bytes.splice(delta..delta, attribute);
    bytes
}

pub(super) fn synthetic_wire_body_smbh() -> Vec<u8> {
    let mut records = Vec::new();
    t_ident(&mut records, "asmheader");
    push_u8_string(&mut records, "231.6.3.65535");
    t_end(&mut records);

    t_ident(&mut records, "body");
    t_ref(&mut records, -1);
    t_long(&mut records, 1);
    t_ref(&mut records, -1);
    t_ref(&mut records, 2);
    t_ref(&mut records, -1);
    t_ref(&mut records, -1);
    t_end(&mut records);

    t_ident(&mut records, "region");
    for reference in [-1, -1, -1, -1, 3, 1] {
        t_ref(&mut records, reference);
    }
    t_end(&mut records);

    t_ident(&mut records, "shell");
    t_ref(&mut records, -1);
    t_long(&mut records, -1);
    for reference in [-1, -1, -1, -1, 4, 2] {
        t_ref(&mut records, reference);
    }
    t_end(&mut records);

    t_ident(&mut records, "wire");
    t_ref(&mut records, -1);
    t_long(&mut records, -1);
    for reference in [-1, -1, 5, 3, -1] {
        t_ref(&mut records, reference);
    }
    records.push(0x0b);
    t_end(&mut records);

    t_ident(&mut records, "coedge");
    t_ref(&mut records, -1);
    t_long(&mut records, -1);
    for reference in [-1, 5, 5, -1, 6] {
        t_ref(&mut records, reference);
    }
    records.push(0x0b);
    t_ref(&mut records, 4);
    t_long(&mut records, 0);
    t_ref(&mut records, -1);
    t_end(&mut records);

    t_ident(&mut records, "edge");
    t_ref(&mut records, -1);
    t_long(&mut records, -1);
    t_ref(&mut records, -1);
    t_ref(&mut records, 7);
    t_dbl(&mut records, 0.0);
    t_ref(&mut records, 8);
    t_dbl(&mut records, 2.0);
    t_ref(&mut records, 5);
    t_ref(&mut records, 11);
    records.push(0x0b);
    push_u8_string(&mut records, "unknown");
    t_end(&mut records);

    for (point, index_flag) in [(9, 0), (10, 1)] {
        t_ident(&mut records, "vertex");
        t_ref(&mut records, -1);
        t_long(&mut records, -1);
        t_ref(&mut records, -1);
        t_ref(&mut records, 6);
        t_long(&mut records, index_flag);
        t_ref(&mut records, point);
        t_end(&mut records);
    }
    for position in [[0.0, 0.0, 0.0], [2.0, 0.0, 0.0]] {
        t_ident(&mut records, "point");
        t_ref(&mut records, -1);
        t_long(&mut records, -1);
        t_ref(&mut records, -1);
        t_pos(&mut records, position);
        t_end(&mut records);
    }
    t_subident(&mut records, "straight");
    t_ident(&mut records, "curve");
    t_ref(&mut records, -1);
    t_long(&mut records, -1);
    t_ref(&mut records, -1);
    t_pos(&mut records, [0.0, 0.0, 0.0]);
    t_vec(&mut records, [1.0, 0.0, 0.0]);
    t_end(&mut records);
    t_ident(&mut records, "delta_state");

    let mut out = smbh_header_prefix();
    out.extend_from_slice(&records);
    out
}

pub(super) fn synthetic_free_vertex_body_smbh() -> Vec<u8> {
    let mut records = Vec::new();
    t_ident(&mut records, "asmheader");
    push_u8_string(&mut records, "231.6.3.65535");
    t_end(&mut records);

    t_ident(&mut records, "body");
    t_ref(&mut records, -1);
    t_long(&mut records, 1);
    for reference in [-1, 2, 4, -1] {
        t_ref(&mut records, reference);
    }
    t_end(&mut records);

    t_ident(&mut records, "region");
    for reference in [-1, -1, -1, -1, 3, 1] {
        t_ref(&mut records, reference);
    }
    t_end(&mut records);

    t_ident(&mut records, "shell");
    t_ref(&mut records, -1);
    t_long(&mut records, -1);
    for reference in [-1, -1, -1, -1, 4, 2] {
        t_ref(&mut records, reference);
    }
    t_end(&mut records);

    t_ident(&mut records, "wire");
    t_ref(&mut records, -1);
    t_long(&mut records, -1);
    for reference in [-1, -1, -1, 3, 5] {
        t_ref(&mut records, reference);
    }
    records.push(0x0b);
    t_end(&mut records);

    t_ident(&mut records, "vertex");
    t_ref(&mut records, -1);
    t_long(&mut records, -1);
    t_ref(&mut records, -1);
    t_ref(&mut records, 4);
    t_long(&mut records, -1);
    t_ref(&mut records, 6);
    t_end(&mut records);

    t_ident(&mut records, "point");
    t_ref(&mut records, -1);
    t_long(&mut records, -1);
    t_ref(&mut records, -1);
    t_pos(&mut records, [1.0, 2.0, 3.0]);
    t_end(&mut records);
    t_ident(&mut records, "delta_state");

    let mut out = smbh_header_prefix();
    out.extend_from_slice(&records);
    out
}

pub(super) fn synthetic_mixed_face_wire_body_smbh() -> Vec<u8> {
    let mut bytes = synthetic_geometry_smbh();
    let start = asm_header::record_stream_start(&bytes).unwrap();
    let limit = asm_header::solved_record_limit(&bytes).unwrap();
    let records = cadmpeg_asm::sab::frame(&bytes, start, limit, 8).unwrap();
    for (record_index, reference_ordinal) in [(1usize, 3usize), (3, 5)] {
        let record = &records[record_index];
        let offsets = cadmpeg_asm::sab::payload_token_offsets(&bytes, record, 8, 0x0c)
            .expect("generated reference offsets");
        let offset = offsets[reference_ordinal];
        bytes[offset + 1..offset + 9].copy_from_slice(&19i64.to_le_bytes());
    }
    let updated = cadmpeg_asm::sab::frame(&bytes, start, limit, 8).unwrap();
    assert_eq!(updated[1].ref_at(4), Some(19));
    assert_eq!(updated[3].ref_at(6), Some(19));

    let delta = bytes
        .windows(b"delta_state".len())
        .position(|window| window == b"delta_state")
        .unwrap()
        - 2;
    let mut appended = Vec::new();
    t_ident(&mut appended, "wire");
    t_ref(&mut appended, -1);
    t_long(&mut appended, -1);
    for reference in [-1, -1, 20, 3, -1] {
        t_ref(&mut appended, reference);
    }
    appended.push(0x0b);
    t_end(&mut appended);

    t_ident(&mut appended, "coedge");
    t_ref(&mut appended, -1);
    t_long(&mut appended, -1);
    for reference in [-1, 20, 20, -1, 21] {
        t_ref(&mut appended, reference);
    }
    appended.push(0x0b);
    t_ref(&mut appended, 19);
    t_long(&mut appended, 0);
    t_ref(&mut appended, -1);
    t_end(&mut appended);

    t_ident(&mut appended, "edge");
    t_ref(&mut appended, -1);
    t_long(&mut appended, -1);
    t_ref(&mut appended, -1);
    t_ref(&mut appended, 22);
    t_dbl(&mut appended, 0.0);
    t_ref(&mut appended, 23);
    t_dbl(&mut appended, 2.0);
    t_ref(&mut appended, 20);
    t_ref(&mut appended, 26);
    appended.push(0x0b);
    push_u8_string(&mut appended, "unknown");
    t_end(&mut appended);

    for (point, index_flag) in [(24, 0), (25, 1)] {
        t_ident(&mut appended, "vertex");
        t_ref(&mut appended, -1);
        t_long(&mut appended, -1);
        t_ref(&mut appended, -1);
        t_ref(&mut appended, 21);
        t_long(&mut appended, index_flag);
        t_ref(&mut appended, point);
        t_end(&mut appended);
    }
    for position in [[0.0, 0.0, 1.0], [2.0, 0.0, 1.0]] {
        t_ident(&mut appended, "point");
        t_ref(&mut appended, -1);
        t_long(&mut appended, -1);
        t_ref(&mut appended, -1);
        t_pos(&mut appended, position);
        t_end(&mut appended);
    }
    t_subident(&mut appended, "straight");
    t_ident(&mut appended, "curve");
    t_ref(&mut appended, -1);
    t_long(&mut appended, -1);
    t_ref(&mut appended, -1);
    t_pos(&mut appended, [0.0, 0.0, 1.0]);
    t_vec(&mut appended, [1.0, 0.0, 0.0]);
    t_end(&mut appended);
    bytes.splice(delta..delta, appended);
    bytes
}

pub(super) fn synthetic_geometry_with_degenerate_curve_smbh() -> Vec<u8> {
    let mut bytes = synthetic_geometry_smbh();
    let start = asm_header::record_stream_start(&bytes).unwrap();
    let limit = asm_header::solved_record_limit(&bytes).unwrap();
    let records = cadmpeg_asm::sab::frame(&bytes, start, limit, 8).unwrap();
    let edge = &records[10];
    let offsets = cadmpeg_asm::sab::payload_token_offsets(&bytes, edge, 8, 0x0c)
        .expect("generated edge reference offsets");
    bytes[offsets[3] + 1..offsets[3] + 9].copy_from_slice(&13i64.to_le_bytes());
    bytes[offsets[5] + 1..offsets[5] + 9].copy_from_slice(&19i64.to_le_bytes());
    let vertex = &records[14];
    let owner = cadmpeg_asm::sab::payload_token_offsets(&bytes, vertex, 8, 0x0c)
        .expect("generated vertex reference offsets")[2];
    bytes[owner + 1..owner + 9].copy_from_slice(&11i64.to_le_bytes());
    let endpoint = cadmpeg_asm::sab::payload_token_offsets(&bytes, vertex, 8, 0x04)
        .expect("generated vertex integer offsets")[1];
    bytes[endpoint + 1..endpoint + 9].copy_from_slice(&0i64.to_le_bytes());

    let delta = bytes
        .windows(b"delta_state".len())
        .position(|window| window == b"delta_state")
        .unwrap()
        - 2;
    let mut curve = Vec::new();
    t_subident(&mut curve, "degenerate_curve");
    t_ident(&mut curve, "curve");
    t_ref(&mut curve, -1);
    t_long(&mut curve, -1);
    t_ref(&mut curve, -1);
    t_pos(&mut curve, [0.0, 0.0, 0.0]);
    curve.extend_from_slice(&[0x0b, 0x0b]);
    t_end(&mut curve);
    bytes.splice(delta..delta, curve);
    bytes
}

pub(super) fn generated_pcurve_block() -> Vec<u8> {
    generated_pcurve_block_with_points([[0.25, 0.5], [0.75, 1.5]])
}
pub(super) fn generated_planar_pcurve_block() -> Vec<u8> {
    generated_pcurve_block_with_points([[0.025, -0.05], [0.075, -0.15]])
}
pub(super) fn generated_pcurve_block_with_points(points: [[f64; 2]; 2]) -> Vec<u8> {
    let mut b = Vec::new();
    b.extend_from_slice(b"\x0d\x04nubs");
    push_tagged_i64(&mut b, 0x04, 1);
    push_tagged_i64(&mut b, 0x15, 0);
    push_tagged_i64(&mut b, 0x04, 2);
    for (k, m) in [(0.0, 1i64), (1.0, 1)] {
        push_tagged_f64(&mut b, k);
        push_tagged_i64(&mut b, 0x04, m);
    }
    for [u, v] in points {
        push_tagged_f64(&mut b, u);
        push_tagged_f64(&mut b, v);
    }
    b
}
pub(super) fn generated_rational_pcurve_block() -> Vec<u8> {
    generated_rational_pcurve_block_with_points([[0.25, 0.5], [0.75, 1.5]])
}
pub(super) fn generated_planar_rational_pcurve_block() -> Vec<u8> {
    generated_rational_pcurve_block_with_points([[0.025, -0.05], [0.075, -0.15]])
}
pub(super) fn generated_rational_pcurve_block_with_points(points: [[f64; 2]; 2]) -> Vec<u8> {
    let mut b = Vec::new();
    b.extend_from_slice(b"\x0d\x05nurbs");
    push_tagged_i64(&mut b, 0x04, 1);
    push_tagged_i64(&mut b, 0x15, 0);
    push_tagged_i64(&mut b, 0x04, 2);
    for (k, m) in [(0.0, 1i64), (1.0, 1)] {
        push_tagged_f64(&mut b, k);
        push_tagged_i64(&mut b, 0x04, m);
    }
    for ([u, v], weight) in points.into_iter().zip([1.0, 0.5]) {
        push_tagged_f64(&mut b, u);
        push_tagged_f64(&mut b, v);
        push_tagged_f64(&mut b, weight);
    }
    b
}

pub(super) fn generated_curve_block() -> Vec<u8> {
    let mut b = Vec::new();
    b.extend_from_slice(b"\x0d\x04nubs");
    push_tagged_i64(&mut b, 0x04, 2);
    push_tagged_i64(&mut b, 0x15, 0);
    push_tagged_i64(&mut b, 0x04, 2);
    for (k, m) in [(0.0, 2i64), (1.0, 2)] {
        push_tagged_f64(&mut b, k);
        push_tagged_i64(&mut b, 0x04, m);
    }
    for point in [[0.0, 0.0, 0.0], [1.0, 2.0, 0.0], [2.0, 0.0, 0.0]] {
        for coordinate in point {
            push_tagged_f64(&mut b, coordinate);
        }
    }
    b
}

pub(super) fn generated_surface_block() -> Vec<u8> {
    let mut b = Vec::new();
    b.extend_from_slice(b"\x0d\x04nubs");
    push_tagged_i64(&mut b, 0x04, 1);
    push_tagged_i64(&mut b, 0x04, 1);
    for _ in 0..4 {
        push_tagged_i64(&mut b, 0x15, 0);
    }
    push_tagged_i64(&mut b, 0x04, 2);
    push_tagged_i64(&mut b, 0x04, 2);
    for _ in 0..2 {
        for (k, m) in [(0.0, 1i64), (1.0, 1)] {
            push_tagged_f64(&mut b, k);
            push_tagged_i64(&mut b, 0x04, m);
        }
    }
    for p in [
        [0.0, 0.0, 0.0],
        [1.0, 0.0, 0.0],
        [0.0, 1.0, 0.0],
        [1.0, 1.0, 0.0],
    ] {
        for c in p {
            push_tagged_f64(&mut b, c);
        }
    }
    b
}

pub(super) fn generated_rational_surface_block() -> Vec<u8> {
    let mut block = generated_surface_block();
    block.splice(0..6, b"\x0d\x05nurbs".iter().copied());
    let non_rational = generated_surface_block();
    let control_start = non_rational.len() - 4 * 3 * 9;
    let rational_control_start = control_start + 1;
    for pole in (0..4).rev() {
        let at = rational_control_start + pole * 3 * 9 + 3 * 9;
        let weight = [1.0f64, 0.8, 1.2, 1.0][pole];
        let mut tagged = vec![0x06];
        tagged.extend_from_slice(&weight.to_le_bytes());
        block.splice(at..at, tagged);
    }
    block
}

pub(super) fn synthetic_cyl_spl_sur_smbh() -> Vec<u8> {
    synthetic_cyl_spl_sur_with_cache_smbh(true)
}

/// Append the head of the shared revision-gated surface tail. Form `0` stores
/// the solved cache followed by its fit tolerance; form `2` stores the U
/// parameter interval and the V parameter interval in the optional bool-gated
/// encoding, then the U closure, V closure, U singularity, and V singularity
/// enums. Every slot carries a distinct value so a reordering fails loudly.
pub(super) fn append_revision_surface_tail_head(
    bytes: &mut Vec<u8>,
    form: i64,
    fit_tolerance: f64,
) {
    push_tagged_i64(bytes, 0x15, form);
    if form == 0 {
        bytes.extend_from_slice(&generated_surface_block());
        t_dbl(bytes, fit_tolerance);
        return;
    }
    for value in [0.25, 0.75, -1.5, 3.5] {
        bytes.push(0x0a);
        t_dbl(bytes, value);
    }
    for value in [1, 2, 3, 4] {
        push_tagged_i64(bytes, 0x15, value);
    }
}

/// Append the six counted discontinuity arrays and the boolean closing the
/// shared revision-gated surface tail.
pub(super) fn append_revision_surface_tail_discontinuities(bytes: &mut Vec<u8>) {
    for values in [
        &[0.25][..],
        &[][..],
        &[0.5, 0.75][..],
        &[1.5][..],
        &[][..],
        &[2.5, 3.5][..],
    ] {
        t_long(bytes, i64::try_from(values.len()).unwrap());
        for value in values {
            t_dbl(bytes, *value);
        }
    }
    bytes.push(0x0b);
}

/// The discontinuity arrays `append_revision_surface_tail_discontinuities`
/// writes.
pub(super) fn expected_revision_surface_tail_discontinuities() -> [Vec<f64>; 6] {
    [
        vec![0.25],
        vec![],
        vec![0.5, 0.75],
        vec![1.5],
        vec![],
        vec![2.5, 3.5],
    ]
}

/// The parameterization `append_revision_surface_tail_head` writes for form `2`.
pub(super) fn expected_revision_surface_tail_parameterization(
) -> cadmpeg_ir::geometry::RevisionSurfaceParameterization {
    cadmpeg_ir::geometry::RevisionSurfaceParameterization {
        u_interval: [Some(0.25), Some(0.75)],
        v_interval: [Some(-1.5), Some(3.5)],
        u_closure: 1,
        v_closure: 2,
        u_singularity: 3,
        v_singularity: 4,
    }
}

pub(super) fn synthetic_versioned_cyl_spl_sur_smbh() -> Vec<u8> {
    synthetic_versioned_cyl_spl_sur_with_tail_smbh(0)
}

/// A revision-gated `cyl_spl_sur` closing with the shared surface tail. Its
/// directrix scope carries a surface block and a trailing scalar of its own, so
/// a decoder that locates the face cache by scanning the scope rather than by
/// parsing the tail picks that block up and reads its trailing scalar as the
/// fit tolerance.
pub(super) fn synthetic_versioned_cyl_spl_sur_with_tail_smbh(tail_form: i64) -> Vec<u8> {
    let mut bytes = synthetic_mixed_smbh();
    let start = asm_header::record_stream_start(&bytes).unwrap();
    let limit = asm_header::solved_record_limit(&bytes).unwrap();
    let records = cadmpeg_asm::sab::frame(&bytes, start, limit, 8).unwrap();
    let old_offset = records[9].offset;
    let old_len = records[9].len;

    let mut surface = Vec::new();
    t_subident(&mut surface, "spline");
    t_ident(&mut surface, "surface");
    t_ref(&mut surface, -1);
    t_long(&mut surface, -1);
    t_ref(&mut surface, -1);
    surface.push(0x0f);
    t_ident(&mut surface, "cyl_spl_sur");
    t_long(&mut surface, 23100);
    t_ident(&mut surface, "intcurve");
    surface.push(0x0a);
    surface.push(0x0f);
    t_ident(&mut surface, "exact_int_cur");
    surface.extend_from_slice(&generated_curve_block());
    surface.extend_from_slice(&generated_surface_block());
    t_dbl(&mut surface, 0.009);
    surface.push(0x10);
    surface.push(0x0a);
    t_dbl(&mut surface, 0.25);
    surface.push(0x0a);
    t_dbl(&mut surface, 0.75);
    t_vec(&mut surface, [0.0, 0.0, 2.0]);
    t_pos(&mut surface, [4.0, 5.0, 6.0]);
    append_revision_surface_tail_head(&mut surface, tail_form, 0.002);
    append_revision_surface_tail_discontinuities(&mut surface);
    surface.push(0x10);
    t_end(&mut surface);
    bytes.splice(old_offset..old_offset + old_len, surface);
    bytes
}

pub(super) fn synthetic_cacheless_cyl_spl_sur_smbh() -> Vec<u8> {
    synthetic_cyl_spl_sur_with_cache_smbh(false)
}

pub(super) fn synthetic_cyl_spl_sur_with_cache_smbh(include_cache: bool) -> Vec<u8> {
    let mut bytes = synthetic_mixed_smbh();
    let start = asm_header::record_stream_start(&bytes).unwrap();
    let limit = asm_header::solved_record_limit(&bytes).unwrap();
    let records = cadmpeg_asm::sab::frame(&bytes, start, limit, 8).unwrap();
    let old_offset = records[9].offset;
    let old_len = records[9].len;

    let mut surface = Vec::new();
    t_subident(&mut surface, "spline");
    t_ident(&mut surface, "surface");
    t_ref(&mut surface, -1);
    t_long(&mut surface, -1);
    t_ref(&mut surface, -1);
    surface.push(0x0f);
    t_ident(&mut surface, "cyl_spl_sur");
    t_dbl(&mut surface, 0.25);
    t_dbl(&mut surface, 0.75);
    t_vec(&mut surface, [0.0, 0.0, 2.0]);
    t_pos(&mut surface, [4.0, 5.0, 6.0]);
    surface.extend_from_slice(&generated_curve_block());
    if include_cache {
        surface.extend_from_slice(&generated_surface_block());
        t_dbl(&mut surface, 0.002);
    }
    surface.push(0x10);
    t_end(&mut surface);
    bytes.splice(old_offset..old_offset + old_len, surface);
    bytes
}

pub(super) fn synthetic_exact_spl_sur_smbh(name: &str) -> Vec<u8> {
    let mut bytes = synthetic_mixed_smbh();
    let start = asm_header::record_stream_start(&bytes).unwrap();
    let limit = asm_header::solved_record_limit(&bytes).unwrap();
    let records = cadmpeg_asm::sab::frame(&bytes, start, limit, 8).unwrap();
    let old = &records[9];

    let mut surface = Vec::new();
    t_subident(&mut surface, "spline");
    t_ident(&mut surface, "surface");
    t_ref(&mut surface, -1);
    t_long(&mut surface, -1);
    t_ref(&mut surface, -1);
    surface.push(0x0f);
    t_ident(&mut surface, name);
    surface.extend_from_slice(&generated_surface_block());
    t_dbl(&mut surface, 0.0015);
    for value in [-2.0, 3.0, -4.0, 5.0] {
        t_dbl(&mut surface, value);
    }
    t_long(&mut surface, 7);
    surface.push(0x10);
    t_end(&mut surface);
    bytes.splice(old.offset..old.offset + old.len, surface);
    bytes
}

pub(super) fn synthetic_exact_spl_sur_with_decoy_sense_smbh() -> Vec<u8> {
    let mut bytes = synthetic_exact_spl_sur_smbh("exact_spl_sur");
    let marker = b"\x0f\x0d\x0dexact_spl_sur";
    let subtype = bytes
        .windows(marker.len())
        .position(|window| window == marker)
        .expect("generated exact spline-surface subtype");
    bytes.splice(subtype..subtype, [0x0a, 0x0b]);
    bytes
}

pub(super) fn synthetic_ruled_spl_sur_smbh(name: &str, include_cache: bool) -> Vec<u8> {
    let mut bytes = synthetic_mixed_smbh();
    let start = asm_header::record_stream_start(&bytes).unwrap();
    let limit = asm_header::solved_record_limit(&bytes).unwrap();
    let records = cadmpeg_asm::sab::frame(&bytes, start, limit, 8).unwrap();
    let old = &records[9];

    let mut surface = Vec::new();
    t_subident(&mut surface, "spline");
    t_ident(&mut surface, "surface");
    t_ref(&mut surface, -1);
    t_long(&mut surface, -1);
    t_ref(&mut surface, -1);
    surface.push(0x0f);
    t_ident(&mut surface, name);
    surface.extend_from_slice(&generated_curve_block());
    surface.extend_from_slice(&generated_curve_block());
    if include_cache {
        surface.extend_from_slice(&generated_surface_block());
        t_dbl(&mut surface, 0.0025);
    }
    surface.push(0x10);
    t_end(&mut surface);
    bytes.splice(old.offset..old.offset + old.len, surface);
    bytes
}

pub(super) fn synthetic_sum_spl_sur_smbh(name: &str, include_cache: bool) -> Vec<u8> {
    let mut bytes = synthetic_mixed_smbh();
    let start = asm_header::record_stream_start(&bytes).unwrap();
    let limit = asm_header::solved_record_limit(&bytes).unwrap();
    let records = cadmpeg_asm::sab::frame(&bytes, start, limit, 8).unwrap();
    let old = &records[9];

    let mut surface = Vec::new();
    t_subident(&mut surface, "spline");
    t_ident(&mut surface, "surface");
    t_ref(&mut surface, -1);
    t_long(&mut surface, -1);
    t_ref(&mut surface, -1);
    surface.push(0x0f);
    t_ident(&mut surface, name);
    surface.extend_from_slice(&generated_curve_block());
    surface.extend_from_slice(&generated_curve_block());
    t_pos(&mut surface, [1.0, -2.0, 3.0]);
    if include_cache {
        surface.extend_from_slice(&generated_surface_block());
        t_dbl(&mut surface, 0.0035);
    }
    surface.push(0x10);
    t_end(&mut surface);
    bytes.splice(old.offset..old.offset + old.len, surface);
    bytes
}

pub(super) fn synthetic_rot_spl_sur_smbh(name: &str) -> Vec<u8> {
    let mut bytes = synthetic_mixed_smbh();
    let start = asm_header::record_stream_start(&bytes).unwrap();
    let limit = asm_header::solved_record_limit(&bytes).unwrap();
    let records = cadmpeg_asm::sab::frame(&bytes, start, limit, 8).unwrap();
    let old = &records[9];

    let mut surface = Vec::new();
    t_subident(&mut surface, "spline");
    t_ident(&mut surface, "surface");
    t_ref(&mut surface, -1);
    t_long(&mut surface, -1);
    t_ref(&mut surface, -1);
    surface.push(0x0f);
    t_ident(&mut surface, name);
    surface.extend_from_slice(&generated_curve_block());
    t_pos(&mut surface, [1.0, -2.0, 3.0]);
    t_vec(&mut surface, [0.0, 0.0, 1.0]);
    surface.extend_from_slice(&generated_surface_block());
    t_dbl(&mut surface, 0.0045);
    surface.push(0x10);
    t_end(&mut surface);
    bytes.splice(old.offset..old.offset + old.len, surface);
    bytes
}

pub(super) fn synthetic_off_spl_sur_smbh(name: &str) -> Vec<u8> {
    let mut bytes = synthetic_mixed_smbh();
    let start = asm_header::record_stream_start(&bytes).unwrap();
    let limit = asm_header::solved_record_limit(&bytes).unwrap();
    let records = cadmpeg_asm::sab::frame(&bytes, start, limit, 8).unwrap();
    let old = &records[9];

    let mut surface = Vec::new();
    t_subident(&mut surface, "spline");
    t_ident(&mut surface, "surface");
    t_ref(&mut surface, -1);
    t_long(&mut surface, -1);
    t_ref(&mut surface, -1);
    surface.push(0x0f);
    t_ident(&mut surface, name);
    t_ident(&mut surface, "plane");
    t_pos(&mut surface, [1.0, -2.0, 3.0]);
    t_vec(&mut surface, [0.0, 0.0, 1.0]);
    t_vec(&mut surface, [1.0, 0.0, 0.0]);
    surface.push(0x0b);
    t_dbl(&mut surface, -1.25);
    surface.push(0x15);
    surface.extend_from_slice(&3i64.to_le_bytes());
    surface.push(0x15);
    surface.extend_from_slice(&(-4i64).to_le_bytes());
    if name == "off_spl_sur" {
        surface.extend_from_slice(&[0x0a, 0x0b, 0x0a]);
    }
    surface.extend_from_slice(&generated_surface_block());
    t_dbl(&mut surface, 0.0055);
    surface.push(0x10);
    t_end(&mut surface);
    bytes.splice(old.offset..old.offset + old.len, surface);
    bytes
}

pub(super) fn synthetic_comp_spl_sur_smbh() -> Vec<u8> {
    let mut bytes = synthetic_mixed_smbh();
    let start = asm_header::record_stream_start(&bytes).unwrap();
    let limit = asm_header::solved_record_limit(&bytes).unwrap();
    let records = cadmpeg_asm::sab::frame(&bytes, start, limit, 8).unwrap();
    let old = &records[9];

    let mut surface = Vec::new();
    t_subident(&mut surface, "spline");
    t_ident(&mut surface, "surface");
    t_ref(&mut surface, -1);
    t_long(&mut surface, -1);
    t_ref(&mut surface, -1);
    surface.push(0x0f);
    t_ident(&mut surface, "comp_spl_sur");
    surface.extend_from_slice(&generated_surface_block());
    t_dbl(&mut surface, 0.0065);
    t_long(&mut surface, 2);
    t_dbl(&mut surface, -0.5);
    t_dbl(&mut surface, 1.5);
    t_ident(&mut surface, "plane");
    t_pos(&mut surface, [1.0, -2.0, 3.0]);
    t_vec(&mut surface, [0.0, 0.0, 1.0]);
    t_vec(&mut surface, [1.0, 0.0, 0.0]);
    surface.push(0x0b);
    t_ident(&mut surface, "spline");
    surface.extend_from_slice(&generated_rational_surface_block());
    surface.push(0x10);
    t_end(&mut surface);
    bytes.splice(old.offset..old.offset + old.len, surface);
    bytes
}

pub(super) fn synthetic_taper_spl_sur_smbh(name: &str) -> Vec<u8> {
    let mut bytes = synthetic_mixed_smbh();
    let start = asm_header::record_stream_start(&bytes).unwrap();
    let limit = asm_header::solved_record_limit(&bytes).unwrap();
    let records = cadmpeg_asm::sab::frame(&bytes, start, limit, 8).unwrap();
    let old = &records[9];

    let mut surface = Vec::new();
    t_subident(&mut surface, "spline");
    t_ident(&mut surface, "surface");
    t_ref(&mut surface, -1);
    t_long(&mut surface, -1);
    t_ref(&mut surface, -1);
    surface.push(0x0f);
    t_ident(&mut surface, name);
    t_ident(&mut surface, "plane");
    t_pos(&mut surface, [1.0, -2.0, 3.0]);
    t_vec(&mut surface, [0.0, 0.0, 1.0]);
    t_vec(&mut surface, [1.0, 0.0, 0.0]);
    surface.push(0x0b);
    surface.extend_from_slice(&generated_curve_block());
    surface.extend_from_slice(&generated_pcurve_block());
    t_dbl(&mut surface, 0.35);
    surface.extend_from_slice(&generated_surface_block());
    t_dbl(&mut surface, 0.0075);
    match name {
        "ortho_spl_sur" | "orthosur" => surface.push(0x0a),
        "edge_tpr_spl_sur" => t_vec(&mut surface, [1.0, 2.0, 3.0]),
        "shadow_tpr_spl_sur" | "shadowtapersur" | "swept_tpr_spl_sur" | "swepttapersur" => {
            t_vec(&mut surface, [1.0, 2.0, 3.0]);
            t_dbl(&mut surface, 0.6);
            t_dbl(&mut surface, 0.8);
        }
        "ruled_tpr_spl_sur" | "ruledtapersur" => {
            t_vec(&mut surface, [1.0, 2.0, 3.0]);
            t_dbl(&mut surface, 0.6);
            t_dbl(&mut surface, 0.8);
            t_dbl(&mut surface, 1.25);
        }
        "taper_spl_sur" => {}
        _ => unreachable!(),
    }
    surface.push(0x10);
    t_end(&mut surface);
    bytes.splice(old.offset..old.offset + old.len, surface);
    bytes
}

pub(super) fn append_generated_loft_section(bytes: &mut Vec<u8>, parameter: f64, direction: bool) {
    t_long(bytes, 1);
    t_dbl(bytes, parameter);
    t_long(bytes, 1);
    t_long(bytes, 9);
    bytes.extend_from_slice(&generated_curve_block());
    t_ident(bytes, "plane");
    t_pos(bytes, [1.0, -2.0, 3.0]);
    t_vec(bytes, [0.0, 0.0, 1.0]);
    t_vec(bytes, [1.0, 0.0, 0.0]);
    bytes.push(0x0b);
    bytes.extend_from_slice(&generated_pcurve_block());
    bytes.push(0x0b);
    t_long(bytes, -1);
    t_long(bytes, 211);
    t_long(bytes, 4);
    t_long(bytes, 0);
    t_dbl(bytes, -0.25);
    t_dbl(bytes, 0.75);
    bytes.push(if direction { 0x0a } else { 0x0b });
    if direction {
        t_vec(bytes, [0.0, 1.0, 0.0]);
    }
    bytes.extend_from_slice(&generated_curve_block());
    t_long(bytes, 1);
    bytes.extend_from_slice(&generated_curve_block());
    t_long(bytes, 6);
}

pub(super) fn synthetic_loft_spl_sur_smbh(name: &str) -> Vec<u8> {
    let mut bytes = synthetic_mixed_smbh();
    let start = asm_header::record_stream_start(&bytes).unwrap();
    let limit = asm_header::solved_record_limit(&bytes).unwrap();
    let records = cadmpeg_asm::sab::frame(&bytes, start, limit, 8).unwrap();
    let old = &records[9];
    let mut surface = Vec::new();
    t_subident(&mut surface, "spline");
    t_ident(&mut surface, "surface");
    t_ref(&mut surface, -1);
    t_long(&mut surface, -1);
    t_ref(&mut surface, -1);
    surface.push(0x0f);
    t_ident(&mut surface, name);
    append_generated_loft_section(&mut surface, 0.0, true);
    append_generated_loft_section(&mut surface, 1.0, false);
    for value in [-1.0, 2.0, -3.0, 4.0] {
        t_dbl(&mut surface, value);
    }
    for value in [1i64, 2, 3, 4] {
        surface.push(0x15);
        surface.extend_from_slice(&value.to_le_bytes());
    }
    t_long(&mut surface, 2);
    surface.push(0x0a);
    t_long(&mut surface, 17);
    t_dbl(&mut surface, 0.125);
    push_u8_string(&mut surface, "bridge");
    surface.push(0x15);
    surface.extend_from_slice(&(-7i64).to_le_bytes());
    surface.extend_from_slice(&generated_surface_block());
    t_dbl(&mut surface, 0.0085);
    surface.push(0x10);
    t_end(&mut surface);
    bytes.splice(old.offset..old.offset + old.len, surface);
    bytes
}

pub(super) fn synthetic_net_spl_sur_smbh() -> Vec<u8> {
    let mut bytes = synthetic_mixed_smbh();
    let start = asm_header::record_stream_start(&bytes).unwrap();
    let limit = asm_header::solved_record_limit(&bytes).unwrap();
    let records = cadmpeg_asm::sab::frame(&bytes, start, limit, 8).unwrap();
    let old = &records[9];
    let mut surface = Vec::new();
    t_subident(&mut surface, "spline");
    t_ident(&mut surface, "surface");
    t_ref(&mut surface, -1);
    t_long(&mut surface, -1);
    t_ref(&mut surface, -1);
    surface.push(0x0f);
    t_ident(&mut surface, "net_spl_sur");
    append_generated_loft_section(&mut surface, 0.0, true);
    append_generated_loft_section(&mut surface, 1.0, false);
    for value in 0..12 {
        t_dbl(&mut surface, f64::from(value) / 10.0);
    }
    t_long(&mut surface, 17);
    for direction in [
        [1.0, 0.0, 0.0],
        [0.0, 1.0, 0.0],
        [0.0, 0.0, 1.0],
        [-1.0, 0.0, 0.0],
    ] {
        t_vec(&mut surface, direction);
    }
    for _ in 0..4 {
        push_u8_string(&mut surface, "null_law");
    }
    surface.extend_from_slice(&generated_surface_block());
    t_dbl(&mut surface, 0.005);
    for values in [&[0.25][..], &[][..], &[][..], &[][..], &[][..], &[][..]] {
        append_generated_float_array(&mut surface, values);
    }
    surface.push(0x0a);
    surface.push(0x10);
    t_end(&mut surface);
    bytes.splice(old.offset..old.offset + old.len, surface);
    bytes
}

pub(super) fn synthetic_profile_first_sweep_smbh() -> Vec<u8> {
    let mut bytes = synthetic_mixed_smbh();
    let start = asm_header::record_stream_start(&bytes).unwrap();
    let limit = asm_header::solved_record_limit(&bytes).unwrap();
    let records = cadmpeg_asm::sab::frame(&bytes, start, limit, 8).unwrap();
    let old = &records[9];
    let mut surface = Vec::new();
    t_subident(&mut surface, "spline");
    t_ident(&mut surface, "surface");
    t_ref(&mut surface, -1);
    t_long(&mut surface, -1);
    t_ref(&mut surface, -1);
    surface.push(0x0f);
    t_ident(&mut surface, "sweep_spl_sur");
    surface.push(0x15);
    surface.extend_from_slice(&3i64.to_le_bytes());
    surface.extend_from_slice(&generated_curve_block());
    surface.extend_from_slice(&generated_curve_block());
    surface.push(0x15);
    surface.extend_from_slice(&4i64.to_le_bytes());
    for direction in [
        [1.0, 0.0, 0.0],
        [0.0, 1.0, 0.0],
        [0.0, 0.0, 1.0],
        [-1.0, 0.0, 0.0],
        [0.0, -1.0, 0.0],
    ] {
        t_vec(&mut surface, direction);
    }
    t_pos(&mut surface, [1.0, 2.0, 3.0]);
    for value in [0.1, 0.2, 0.3, 0.4] {
        t_dbl(&mut surface, value);
    }
    for _ in 0..3 {
        push_u8_string(&mut surface, "null_law");
    }
    surface.extend_from_slice(&generated_surface_block());
    t_dbl(&mut surface, 0.005);
    for values in [&[0.25][..], &[][..], &[][..], &[][..], &[][..], &[][..]] {
        append_generated_float_array(&mut surface, values);
    }
    surface.push(0x0a);
    surface.push(0x10);
    t_end(&mut surface);
    bytes.splice(old.offset..old.offset + old.len, surface);
    bytes
}

pub(super) fn synthetic_t_spl_sur_smbh() -> Vec<u8> {
    let mut bytes = synthetic_mixed_smbh();
    let start = asm_header::record_stream_start(&bytes).unwrap();
    let limit = asm_header::solved_record_limit(&bytes).unwrap();
    let records = cadmpeg_asm::sab::frame(&bytes, start, limit, 8).unwrap();
    let old = &records[9];
    let mut surface = Vec::new();
    t_subident(&mut surface, "spline");
    t_ident(&mut surface, "surface");
    t_ref(&mut surface, -1);
    t_long(&mut surface, -1);
    t_ref(&mut surface, -1);
    surface.push(0x0f);
    t_ident(&mut surface, "t_spl_sur");
    surface.extend_from_slice(&generated_surface_block());
    t_dbl(&mut surface, 0.004);
    for values in [&[0.25][..], &[][..], &[][..], &[][..], &[][..], &[][..]] {
        append_generated_float_array(&mut surface, values);
    }
    surface.push(0x0a);
    for value in [-2.0, 3.0, -4.0, 5.0] {
        t_dbl(&mut surface, value);
    }
    t_long(&mut surface, 7);
    surface.push(0x0f);
    t_ident(&mut surface, "t_spl_subtrans_object");
    t_u16_string(
        &mut surface,
        "degree 3\nunits mm\nv 1 0 0 0\nv 2 1 0 0\ne 1 1 2\n",
    );
    surface.push(0x0b);
    t_u16_string(&mut surface, "100verts 1 2\n");
    surface.push(0x10);
    t_long(&mut surface, 9);
    surface.push(0x10);
    t_end(&mut surface);
    bytes.splice(old.offset..old.offset + old.len, surface);
    bytes
}

pub(super) fn synthetic_helix_surface_smbh(circular: bool) -> Vec<u8> {
    let mut bytes = synthetic_mixed_smbh();
    let start = asm_header::record_stream_start(&bytes).unwrap();
    let limit = asm_header::solved_record_limit(&bytes).unwrap();
    let records = cadmpeg_asm::sab::frame(&bytes, start, limit, 8).unwrap();
    let old = &records[9];
    let mut surface = Vec::new();
    t_subident(&mut surface, "spline");
    t_ident(&mut surface, "surface");
    t_ref(&mut surface, -1);
    t_long(&mut surface, -1);
    t_ref(&mut surface, -1);
    surface.push(0x0f);
    t_ident(
        &mut surface,
        if circular {
            "helix_spl_circ"
        } else {
            "helix_spl_line"
        },
    );
    t_dbl(&mut surface, -0.5);
    t_dbl(&mut surface, 0.5);
    t_dbl(&mut surface, -2.0);
    t_dbl(&mut surface, 3.0);
    if circular {
        t_dbl(&mut surface, 1.25);
    }
    t_dbl(&mut surface, 0.0);
    t_dbl(&mut surface, std::f64::consts::TAU);
    t_pos(&mut surface, [1.0, 2.0, 3.0]);
    t_pos(&mut surface, [2.0, 0.0, 0.0]);
    t_pos(&mut surface, [0.0, 2.0, 0.0]);
    t_pos(&mut surface, [0.0, 0.0, 4.0]);
    t_dbl(&mut surface, 0.25);
    t_vec(&mut surface, [0.0, 0.0, 1.0]);
    for sentinel in ["null_surface", "null_surface", "nullbs", "nullbs"] {
        t_ident(&mut surface, sentinel);
    }
    if circular {
        t_dbl(&mut surface, 0.75);
    } else {
        t_pos(&mut surface, [5.0, 6.0, 7.0]);
    }
    surface.push(0x10);
    t_end(&mut surface);
    bytes.splice(old.offset..old.offset + old.len, surface);
    bytes
}

pub(super) fn synthetic_minimal_deformable_surface_smbh() -> Vec<u8> {
    let mut bytes = synthetic_mixed_smbh();
    let start = asm_header::record_stream_start(&bytes).unwrap();
    let limit = asm_header::solved_record_limit(&bytes).unwrap();
    let records = cadmpeg_asm::sab::frame(&bytes, start, limit, 8).unwrap();
    let old = &records[9];
    let mut surface = Vec::new();
    t_subident(&mut surface, "spline");
    t_ident(&mut surface, "surface");
    t_ref(&mut surface, -1);
    t_long(&mut surface, -1);
    t_ref(&mut surface, -1);
    surface.push(0x0f);
    t_ident(&mut surface, "defm_spl_sur");
    t_ident(&mut surface, "plane");
    t_pos(&mut surface, [1.0, 2.0, 3.0]);
    t_vec(&mut surface, [0.0, 0.0, 1.0]);
    t_vec(&mut surface, [1.0, 0.0, 0.0]);
    surface.push(0x0b);
    t_long(&mut surface, 8);
    for vector in [
        [1.0, 0.0, 0.0],
        [0.0, 1.0, 0.0],
        [0.0, 0.0, 1.0],
        [-1.0, 0.0, 0.0],
    ] {
        t_vec(&mut surface, vector);
    }
    t_long(&mut surface, 0);
    surface.extend_from_slice(&generated_surface_block());
    t_dbl(&mut surface, 0.004);
    for values in [&[0.25][..], &[][..], &[][..], &[][..], &[][..], &[][..]] {
        append_generated_float_array(&mut surface, values);
    }
    surface.push(0x0a);
    surface.push(0x10);
    t_end(&mut surface);
    bytes.splice(old.offset..old.offset + old.len, surface);
    bytes
}

pub(super) fn synthetic_framed_deformable_surface_smbh(mode: i64) -> Vec<u8> {
    let mut bytes = synthetic_mixed_smbh();
    let start = asm_header::record_stream_start(&bytes).unwrap();
    let limit = asm_header::solved_record_limit(&bytes).unwrap();
    let records = cadmpeg_asm::sab::frame(&bytes, start, limit, 8).unwrap();
    let old = &records[9];
    let mut surface = Vec::new();
    t_subident(&mut surface, "spline");
    t_ident(&mut surface, "surface");
    t_ref(&mut surface, -1);
    t_long(&mut surface, -1);
    t_ref(&mut surface, -1);
    surface.push(0x0f);
    t_ident(&mut surface, "defm_spl_sur");
    t_ident(&mut surface, "plane");
    t_pos(&mut surface, [1.0, 2.0, 3.0]);
    t_vec(&mut surface, [0.0, 0.0, 1.0]);
    t_vec(&mut surface, [1.0, 0.0, 0.0]);
    surface.push(0x0b);
    t_long(&mut surface, mode);
    for vector in [
        [1.0, 0.0, 0.0],
        [0.0, 1.0, 0.0],
        [0.0, 0.0, 1.0],
        [-1.0, 0.0, 0.0],
    ] {
        t_vec(&mut surface, vector);
    }
    t_dbl(&mut surface, 0.5);
    surface.extend_from_slice(&[0x0a, 0x0b, 0x0a]);
    for vector in [[1.0, 1.0, 0.0], [0.0, 1.0, 1.0], [1.0, 0.0, 1.0]] {
        t_vec(&mut surface, vector);
    }
    t_dbl(&mut surface, 0.75);
    surface.extend_from_slice(&[0x0b, 0x0a]);
    t_pos(&mut surface, [4.0, 5.0, 6.0]);
    surface.extend_from_slice(&[0x0a, 0x0b, 0x0a, 0x0b, 0x0a]);
    if mode == 1 {
        t_long(&mut surface, 2);
        for value in [0.1, 0.2, 0.3, 0.4, 0.5, 0.6] {
            t_dbl(&mut surface, value);
        }
    } else {
        t_long(&mut surface, 1);
        t_dbl(&mut surface, 0.9);
    }
    surface.extend_from_slice(&generated_surface_block());
    t_dbl(&mut surface, 0.004);
    for values in [&[0.25][..], &[][..], &[][..], &[][..], &[][..], &[][..]] {
        append_generated_float_array(&mut surface, values);
    }
    surface.push(0x0a);
    surface.push(0x10);
    t_end(&mut surface);
    bytes.splice(old.offset..old.offset + old.len, surface);
    bytes
}

pub(super) fn synthetic_surface_curve_deformable_smbh() -> Vec<u8> {
    let mut bytes = synthetic_minimal_deformable_surface_smbh();
    let start = asm_header::record_stream_start(&bytes).unwrap();
    let limit = asm_header::solved_record_limit(&bytes).unwrap();
    let records = cadmpeg_asm::sab::frame(&bytes, start, limit, 8).unwrap();
    let old = &records[9];
    let mut surface = Vec::new();
    t_subident(&mut surface, "spline");
    t_ident(&mut surface, "surface");
    t_ref(&mut surface, -1);
    t_long(&mut surface, -1);
    t_ref(&mut surface, -1);
    surface.push(0x0f);
    t_ident(&mut surface, "defm_spl_sur");
    for z in [0.0, 1.0] {
        t_ident(&mut surface, "plane");
        t_pos(&mut surface, [0.0, 0.0, z]);
        t_vec(&mut surface, [0.0, 0.0, 1.0]);
        t_vec(&mut surface, [1.0, 0.0, 0.0]);
        surface.push(0x0b);
        if z == 0.0 {
            t_long(&mut surface, 5);
        }
    }
    t_long(&mut surface, 42);
    surface.push(0x0a);
    t_dbl(&mut surface, 0.2);
    t_long(&mut surface, 3);
    t_dbl(&mut surface, 0.4);
    surface.extend_from_slice(&generated_curve_block());
    for v in [
        [1.0, 0.0, 0.0],
        [0.0, 1.0, 0.0],
        [0.0, 0.0, 1.0],
        [-1.0, 0.0, 0.0],
    ] {
        t_vec(&mut surface, v);
    }
    t_dbl(&mut surface, 0.6);
    surface.extend_from_slice(&[0x0a, 0x0b, 0x0a]);
    t_long(&mut surface, 1);
    for v in [0.1, 0.2, 0.3] {
        t_dbl(&mut surface, v);
    }
    surface.extend_from_slice(&generated_surface_block());
    t_dbl(&mut surface, 0.004);
    for values in [&[0.25][..], &[][..], &[][..], &[][..], &[][..], &[][..]] {
        append_generated_float_array(&mut surface, values);
    }
    surface.push(0x0a);
    surface.push(0x10);
    t_end(&mut surface);
    bytes.splice(old.offset..old.offset + old.len, surface);
    bytes
}

pub(super) fn synthetic_full_deformable_surface_smbh(version_value: Option<i64>) -> Vec<u8> {
    let mut bytes = synthetic_minimal_deformable_surface_smbh();
    let start = asm_header::record_stream_start(&bytes).unwrap();
    let limit = asm_header::solved_record_limit(&bytes).unwrap();
    let records = cadmpeg_asm::sab::frame(&bytes, start, limit, 8).unwrap();
    let old = &records[9];
    let mut surface = Vec::new();
    t_subident(&mut surface, "spline");
    t_ident(&mut surface, "surface");
    t_ref(&mut surface, -1);
    t_long(&mut surface, -1);
    t_ref(&mut surface, -1);
    surface.push(0x0f);
    t_ident(&mut surface, "defm_spl_sur");
    t_ident(&mut surface, "plane");
    t_pos(&mut surface, [0.0, 0.0, 0.0]);
    t_vec(&mut surface, [0.0, 0.0, 1.0]);
    t_vec(&mut surface, [1.0, 0.0, 0.0]);
    surface.push(0x0b);
    t_long(&mut surface, 6);
    for v in [
        [1.0, 0.0, 0.0],
        [0.0, 1.0, 0.0],
        [0.0, 0.0, 1.0],
        [-1.0, 0.0, 0.0],
    ] {
        t_vec(&mut surface, v);
    }
    t_dbl(&mut surface, 0.1);
    surface.extend_from_slice(&[0x0a, 0x0b, 0x0a]);
    t_long(&mut surface, 7);
    t_ident(&mut surface, "plane");
    t_pos(&mut surface, [0.0, 0.0, 1.0]);
    t_vec(&mut surface, [0.0, 0.0, 1.0]);
    t_vec(&mut surface, [1.0, 0.0, 0.0]);
    surface.push(0x0b);
    t_long(&mut surface, 42);
    surface.push(0x0a);
    t_dbl(&mut surface, 0.2);
    if let Some(version_value) = version_value {
        t_long(&mut surface, version_value);
    }
    t_dbl(&mut surface, 0.3);
    surface.extend_from_slice(&generated_curve_block());
    for frame in 0..2 {
        for v in [
            [1.0, 1.0, 0.0],
            [0.0, 1.0, 1.0],
            [1.0, 0.0, 1.0],
            [-1.0, 1.0, 0.0],
        ] {
            t_vec(&mut surface, v);
        }
        t_dbl(&mut surface, 0.4 + f64::from(frame) * 0.1);
        surface.extend_from_slice(&[0x0b, 0x0a, 0x0b]);
    }
    t_long(&mut surface, 99);
    surface.extend_from_slice(&generated_surface_block());
    t_dbl(&mut surface, 0.004);
    for values in [&[0.25][..], &[][..], &[][..], &[][..], &[][..], &[][..]] {
        append_generated_float_array(&mut surface, values);
    }
    surface.push(0x0a);
    surface.push(0x10);
    t_end(&mut surface);
    bytes.splice(old.offset..old.offset + old.len, surface);
    bytes
}

pub(super) fn synthetic_referenced_t_spl_sur_smbh() -> Vec<u8> {
    let mut bytes = synthetic_mixed_smbh();
    let start = asm_header::record_stream_start(&bytes).unwrap();
    let limit = asm_header::solved_record_limit(&bytes).unwrap();
    let records = cadmpeg_asm::sab::frame(&bytes, start, limit, 8).unwrap();
    let old_offset = records[9].offset;
    let old_len = records[9].len;
    let mut surface = Vec::new();
    t_subident(&mut surface, "spline");
    t_ident(&mut surface, "surface");
    t_ref(&mut surface, -1);
    t_long(&mut surface, -1);
    t_ref(&mut surface, -1);
    let shared_offset = surface.len();
    surface.push(0x0f);
    t_ident(&mut surface, "t_spl_subtrans_object");
    t_u16_string(&mut surface, "degree 3\nv 1 0 0 0\n");
    t_u16_string(&mut surface, "100verts 1\n");
    surface.push(0x10);
    surface.push(0x0f);
    t_ident(&mut surface, "t_spl_sur");
    surface.extend_from_slice(&generated_surface_block());
    t_dbl(&mut surface, 0.004);
    for values in [&[0.25][..], &[][..], &[][..], &[][..], &[][..], &[][..]] {
        append_generated_float_array(&mut surface, values);
    }
    surface.push(0x0b);
    for value in [-2.0, 3.0, -4.0, 5.0] {
        t_dbl(&mut surface, value);
    }
    t_long(&mut surface, 7);
    surface.push(0x0f);
    t_ident(&mut surface, "ref");
    let reference_value_offset = surface.len() + 1;
    t_long(&mut surface, 0);
    surface.push(0x10);
    t_long(&mut surface, 9);
    surface.push(0x10);
    t_end(&mut surface);
    bytes.splice(old_offset..old_offset + old_len, surface);
    let records = cadmpeg_asm::sab::frame(
        &bytes,
        asm_header::record_stream_start(&bytes).unwrap(),
        asm_header::solved_record_limit(&bytes).unwrap(),
        8,
    )
    .unwrap();
    let tables = cadmpeg_asm::nurbs::subtypes::SubtypeTables::from_records(&records, &bytes);
    let index = tables
        .index_of_offset(8, old_offset + shared_offset)
        .expect("shared T-spline subtype index");
    bytes[old_offset + reference_value_offset..old_offset + reference_value_offset + 8]
        .copy_from_slice(&i64::try_from(index).unwrap().to_le_bytes());
    bytes
}

pub(super) fn synthetic_explicit_formula_sweep_smbh() -> Vec<u8> {
    let mut bytes = synthetic_mixed_smbh();
    let start = asm_header::record_stream_start(&bytes).unwrap();
    let limit = asm_header::solved_record_limit(&bytes).unwrap();
    let records = cadmpeg_asm::sab::frame(&bytes, start, limit, 8).unwrap();
    let old = &records[9];
    let mut surface = Vec::new();
    t_subident(&mut surface, "spline");
    t_ident(&mut surface, "surface");
    t_ref(&mut surface, -1);
    t_long(&mut surface, -1);
    t_ref(&mut surface, -1);
    surface.push(0x0f);
    t_ident(&mut surface, "sweep_spl_sur");
    surface.push(0x15);
    surface.extend_from_slice(&2i64.to_le_bytes());
    t_long(&mut surface, 7);
    surface.extend_from_slice(&generated_curve_block());
    t_dbl(&mut surface, -0.5);
    t_dbl(&mut surface, 1.5);
    surface.push(0x0a);
    t_pos(&mut surface, [1.0, 2.0, 3.0]);
    t_vec(&mut surface, [0.0, 0.0, 1.0]);
    t_pos(&mut surface, [4.0, 5.0, 6.0]);
    for direction in [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]] {
        t_vec(&mut surface, direction);
    }
    t_long(&mut surface, 1);
    surface.push(0x0a);
    surface.extend_from_slice(&generated_curve_block());
    t_dbl(&mut surface, -2.0);
    t_dbl(&mut surface, 3.0);
    t_dbl(&mut surface, 0.75);
    surface.push(0x0b);
    push_u8_string(&mut surface, "null_law");
    surface.push(0x0a);
    surface.extend_from_slice(&generated_surface_block());
    t_dbl(&mut surface, 0.005);
    for values in [&[0.25][..], &[][..], &[][..], &[][..], &[][..], &[][..]] {
        append_generated_float_array(&mut surface, values);
    }
    surface.push(0x0b);
    surface.push(0x10);
    t_end(&mut surface);
    bytes.splice(old.offset..old.offset + old.len, surface);
    bytes
}

pub(super) fn synthetic_explicit_guide_sweep_smbh() -> Vec<u8> {
    let mut bytes = synthetic_mixed_smbh();
    let start = asm_header::record_stream_start(&bytes).unwrap();
    let limit = asm_header::solved_record_limit(&bytes).unwrap();
    let records = cadmpeg_asm::sab::frame(&bytes, start, limit, 8).unwrap();
    let old = &records[9];
    let mut surface = Vec::new();
    t_subident(&mut surface, "spline");
    t_ident(&mut surface, "surface");
    t_ref(&mut surface, -1);
    t_long(&mut surface, -1);
    t_ref(&mut surface, -1);
    surface.push(0x0f);
    t_ident(&mut surface, "sweep_spl_sur");
    surface.push(0x15);
    surface.extend_from_slice(&2i64.to_le_bytes());
    t_long(&mut surface, 8);
    surface.extend_from_slice(&generated_curve_block());
    t_dbl(&mut surface, -0.25);
    t_dbl(&mut surface, 1.25);
    surface.push(0x0b);
    t_pos(&mut surface, [4.0, 5.0, 6.0]);
    for direction in [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]] {
        t_vec(&mut surface, direction);
    }
    t_long(&mut surface, 2);
    surface.push(0x0a);
    surface.extend_from_slice(&generated_curve_block());
    t_dbl(&mut surface, -2.0);
    t_dbl(&mut surface, 3.0);
    t_dbl(&mut surface, 0.5);
    surface.extend_from_slice(&[0x0a, 0x0b]);
    surface.extend_from_slice(&generated_curve_block());
    t_dbl(&mut surface, 0.0);
    t_dbl(&mut surface, 1.0);
    t_long(&mut surface, 11);
    t_long(&mut surface, 12);
    for value in [0.1, 0.2, 0.3, 0.4, 0.5, 0.6] {
        t_dbl(&mut surface, value);
    }
    surface.extend_from_slice(&[0x0a, 0x0b, 0x0a]);
    surface.extend_from_slice(&generated_surface_block());
    t_dbl(&mut surface, 0.005);
    for values in [&[0.25][..], &[][..], &[][..], &[][..], &[][..], &[][..]] {
        append_generated_float_array(&mut surface, values);
    }
    surface.push(0x0a);
    surface.push(0x10);
    t_end(&mut surface);
    bytes.splice(old.offset..old.offset + old.len, surface);
    bytes
}

pub(super) fn synthetic_explicit_surface_sweep_smbh() -> Vec<u8> {
    let mut bytes = synthetic_mixed_smbh();
    let start = asm_header::record_stream_start(&bytes).unwrap();
    let limit = asm_header::solved_record_limit(&bytes).unwrap();
    let records = cadmpeg_asm::sab::frame(&bytes, start, limit, 8).unwrap();
    let old = &records[9];
    let mut surface = Vec::new();
    t_subident(&mut surface, "spline");
    t_ident(&mut surface, "surface");
    t_ref(&mut surface, -1);
    t_long(&mut surface, -1);
    t_ref(&mut surface, -1);
    surface.push(0x0f);
    t_ident(&mut surface, "sweep_spl_sur");
    surface.push(0x15);
    surface.extend_from_slice(&2i64.to_le_bytes());
    t_long(&mut surface, 9);
    surface.extend_from_slice(&generated_curve_block());
    t_dbl(&mut surface, 0.0);
    t_dbl(&mut surface, 1.0);
    surface.push(0x0b);
    t_pos(&mut surface, [4.0, 5.0, 6.0]);
    for direction in [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]] {
        t_vec(&mut surface, direction);
    }
    t_long(&mut surface, 3);
    surface.push(0x0b);
    surface.extend_from_slice(&generated_curve_block());
    t_dbl(&mut surface, -2.0);
    t_dbl(&mut surface, 3.0);
    t_dbl(&mut surface, 0.25);
    surface.push(0x15);
    surface.extend_from_slice(&1i64.to_le_bytes());
    t_ident(&mut surface, "plane");
    t_pos(&mut surface, [1.0, 2.0, 3.0]);
    t_vec(&mut surface, [0.0, 0.0, 1.0]);
    t_vec(&mut surface, [1.0, 0.0, 0.0]);
    surface.push(0x0b);
    surface.push(0x0a);
    surface.extend_from_slice(&generated_curve_block());
    surface.push(0x0a);
    surface.push(0x0b);
    surface.extend_from_slice(&generated_surface_block());
    t_dbl(&mut surface, 0.005);
    for values in [&[0.25][..], &[][..], &[][..], &[][..], &[][..], &[][..]] {
        append_generated_float_array(&mut surface, values);
    }
    surface.push(0x0a);
    surface.push(0x10);
    t_end(&mut surface);
    bytes.splice(old.offset..old.offset + old.len, surface);
    bytes
}

pub(super) fn synthetic_law_driven_sweep_smbh() -> Vec<u8> {
    let mut bytes = synthetic_mixed_smbh();
    let start = asm_header::record_stream_start(&bytes).unwrap();
    let limit = asm_header::solved_record_limit(&bytes).unwrap();
    let records = cadmpeg_asm::sab::frame(&bytes, start, limit, 8).unwrap();
    let old = &records[9];
    let mut surface = Vec::new();
    t_subident(&mut surface, "spline");
    t_ident(&mut surface, "surface");
    t_ref(&mut surface, -1);
    t_long(&mut surface, -1);
    t_ref(&mut surface, -1);
    surface.push(0x0f);
    t_ident(&mut surface, "sweep_spl_sur");
    surface.push(0x15);
    surface.extend_from_slice(&5i64.to_le_bytes());
    t_long(&mut surface, 10);
    surface.extend_from_slice(&generated_curve_block());
    t_dbl(&mut surface, 0.0);
    t_dbl(&mut surface, 1.0);
    surface.push(0x0b);
    t_pos(&mut surface, [4.0, 5.0, 6.0]);
    for direction in [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]] {
        t_vec(&mut surface, direction);
    }
    t_dbl(&mut surface, 2.5);
    t_long(&mut surface, 21);
    t_dbl(&mut surface, -1.0);
    t_dbl(&mut surface, 1.0);
    t_vec(&mut surface, [0.0, 0.0, 1.0]);
    t_long(&mut surface, 22);
    surface.push(0x0a);
    surface.extend_from_slice(&generated_curve_block());
    t_dbl(&mut surface, -2.0);
    t_dbl(&mut surface, 3.0);
    t_dbl(&mut surface, 0.75);
    surface.push(0x0b);
    t_vec(&mut surface, [1.0, 2.0, 3.0]);
    t_long(&mut surface, 23);
    push_u8_string(&mut surface, "null_law");
    surface.push(0x0a);
    surface.extend_from_slice(&generated_surface_block());
    t_dbl(&mut surface, 0.005);
    for values in [&[0.25][..], &[][..], &[][..], &[][..], &[][..], &[][..]] {
        append_generated_float_array(&mut surface, values);
    }
    surface.push(0x0b);
    surface.push(0x10);
    t_end(&mut surface);
    bytes.splice(old.offset..old.offset + old.len, surface);
    bytes
}

pub(super) fn append_generated_compound_loft_scale(bytes: &mut Vec<u8>) {
    t_long(bytes, 1);
    t_long(bytes, 9);
    bytes.extend_from_slice(&generated_curve_block());
    t_ident(bytes, "plane");
    t_pos(bytes, [1.0, -2.0, 3.0]);
    t_vec(bytes, [0.0, 0.0, 1.0]);
    t_vec(bytes, [1.0, 0.0, 0.0]);
    bytes.push(0x0b);
    bytes.extend_from_slice(&generated_pcurve_block());
    bytes.push(0x0b);
    t_long(bytes, -1);
    t_long(bytes, 211);
    t_long(bytes, 4);
    t_long(bytes, 0);
    t_dbl(bytes, -0.25);
    t_dbl(bytes, 0.75);
    bytes.push(0x0a);
    t_vec(bytes, [0.0, 1.0, 0.0]);
    bytes.extend_from_slice(&generated_curve_block());
    t_long(bytes, 1);
    bytes.extend_from_slice(&generated_curve_block());
    t_long(bytes, 2);
    t_long(bytes, 3);
}

pub(super) fn synthetic_compound_loft_smbh() -> Vec<u8> {
    let mut bytes = synthetic_mixed_smbh();
    let start = asm_header::record_stream_start(&bytes).unwrap();
    let limit = asm_header::solved_record_limit(&bytes).unwrap();
    let records = cadmpeg_asm::sab::frame(&bytes, start, limit, 8).unwrap();
    let old = &records[9];
    let mut surface = Vec::new();
    t_subident(&mut surface, "spline");
    t_ident(&mut surface, "surface");
    t_ref(&mut surface, -1);
    t_long(&mut surface, -1);
    t_ref(&mut surface, -1);
    surface.push(0x0f);
    t_ident(&mut surface, "cl_loft_spl_sur");
    surface.extend_from_slice(&generated_surface_block());
    t_dbl(&mut surface, 0.004);
    append_generated_compound_loft_scale(&mut surface);
    surface.push(0x0a);
    surface.push(0x0b);
    t_long(&mut surface, 0);
    surface.push(0x0b);
    surface.push(0x0a);
    t_long(&mut surface, 0);
    t_vec(&mut surface, [0.0, 0.0, 1.0]);
    surface.push(0x0a);
    surface.push(0x0b);
    surface.push(0x10);
    t_end(&mut surface);
    bytes.splice(old.offset..old.offset + old.len, surface);
    bytes
}

pub(super) fn append_generated_float_array(bytes: &mut Vec<u8>, values: &[f64]) {
    t_long(bytes, i64::try_from(values.len()).unwrap());
    for value in values {
        t_dbl(bytes, *value);
    }
}

pub(super) fn synthetic_scaled_compound_loft_smbh(full: bool) -> Vec<u8> {
    let mut bytes = synthetic_mixed_smbh();
    let start = asm_header::record_stream_start(&bytes).unwrap();
    let limit = asm_header::solved_record_limit(&bytes).unwrap();
    let records = cadmpeg_asm::sab::frame(&bytes, start, limit, 8).unwrap();
    let old = &records[9];
    let mut surface = Vec::new();
    t_subident(&mut surface, "spline");
    t_ident(&mut surface, "surface");
    t_ref(&mut surface, -1);
    t_long(&mut surface, -1);
    t_ref(&mut surface, -1);
    surface.push(0x0f);
    t_ident(&mut surface, "scaled_cloft_spl_sur");
    surface.push(0x15);
    surface.extend_from_slice(&11i64.to_le_bytes());
    if full {
        surface.extend_from_slice(&generated_surface_block());
        t_dbl(&mut surface, 0.004);
    } else {
        for value in [-1.0, 2.0, -3.0, 4.0] {
            t_dbl(&mut surface, value);
        }
        append_generated_float_array(&mut surface, &[0.25]);
        append_generated_float_array(&mut surface, &[0.5, 0.75]);
    }
    for values in [&[0.25][..], &[][..], &[][..], &[][..], &[][..], &[][..]] {
        append_generated_float_array(&mut surface, values);
    }
    surface.push(0x0a);
    append_generated_compound_loft_scale(&mut surface);
    surface.push(0x0a);
    surface.push(0x0b);
    t_long(&mut surface, 0);
    surface.push(0x0b);
    surface.push(0x0a);
    t_long(&mut surface, 0);
    t_vec(&mut surface, [0.0, 0.0, 1.0]);
    surface.push(0x0b);
    surface.push(0x0a);
    t_long(&mut surface, 2);
    t_vec(&mut surface, [1.0, 0.0, 0.0]);
    t_vec(&mut surface, [0.0, 1.0, 0.0]);
    surface.push(0x15);
    surface.extend_from_slice(&12i64.to_le_bytes());
    surface.extend_from_slice(&generated_curve_block());
    surface.push(0x10);
    t_end(&mut surface);
    bytes.splice(old.offset..old.offset + old.len, surface);
    bytes
}

pub(super) fn synthetic_skin_spl_sur_smbh(law_case: u8, expanded: bool) -> Vec<u8> {
    let mut bytes = synthetic_mixed_smbh();
    let start = asm_header::record_stream_start(&bytes).unwrap();
    let limit = asm_header::solved_record_limit(&bytes).unwrap();
    let records = cadmpeg_asm::sab::frame(&bytes, start, limit, 8).unwrap();
    let old = &records[9];
    let mut surface = Vec::new();
    t_subident(&mut surface, "spline");
    t_ident(&mut surface, "surface");
    t_ref(&mut surface, -1);
    t_long(&mut surface, -1);
    t_ref(&mut surface, -1);
    surface.push(0x0f);
    t_ident(&mut surface, "skin_spl_sur");
    for value in [1i64, 2, 3] {
        surface.push(0x15);
        surface.extend_from_slice(&value.to_le_bytes());
    }
    t_long(&mut surface, 4);
    t_dbl(&mut surface, 0.25);
    t_long(&mut surface, 1);
    if expanded {
        t_long(&mut surface, 9);
        surface.extend_from_slice(&generated_curve_block());
        t_ident(&mut surface, "plane");
        t_pos(&mut surface, [1.0, -2.0, 3.0]);
        t_vec(&mut surface, [0.0, 0.0, 1.0]);
        t_vec(&mut surface, [1.0, 0.0, 0.0]);
        surface.push(0x0b);
        surface.extend_from_slice(&generated_pcurve_block());
        surface.push(0x0b);
        t_long(&mut surface, -1);
        t_long(&mut surface, 211);
        t_long(&mut surface, 4);
        t_long(&mut surface, 0);
        t_dbl(&mut surface, -0.5);
        t_dbl(&mut surface, 1.5);
        surface.push(0x0a);
        t_vec(&mut surface, [0.0, 1.0, 0.0]);
        surface.extend_from_slice(&generated_curve_block());
        t_long(&mut surface, -1);
        t_long(&mut surface, 7);
    } else {
        surface.extend_from_slice(&generated_curve_block());
        t_long(&mut surface, 211);
        t_long(&mut surface, 4);
        t_long(&mut surface, 0);
        t_dbl(&mut surface, -0.5);
        t_dbl(&mut surface, 1.5);
        t_long(&mut surface, -1);
        surface.extend_from_slice(&generated_curve_block());
        t_long(&mut surface, 7);
    }
    t_vec(&mut surface, [0.0, 0.0, 1.0]);
    t_dbl(&mut surface, 0.75);
    if law_case == 1 {
        push_u8_string(&mut surface, "structural-law");
        t_long(&mut surface, 3);
        push_u8_string(&mut surface, "null_law");
        push_u8_string(&mut surface, "TRANS");
        for value in 0..13 {
            t_dbl(&mut surface, f64::from(value) / 10.0);
        }
        for value in [4i64, 5, 6] {
            surface.push(0x15);
            surface.extend_from_slice(&value.to_le_bytes());
        }
        push_u8_string(&mut surface, "EDGE");
        surface.extend_from_slice(&generated_curve_block());
        t_dbl(&mut surface, -0.25);
        t_dbl(&mut surface, 1.25);
    } else if law_case == 2 {
        push_u8_string(&mut surface, "algebraic-law");
        t_long(&mut surface, 2);
        push_u8_string(&mut surface, "SIN");
        push_u8_string(&mut surface, "ABS");
        t_dbl(&mut surface, -2.5);
        push_u8_string(&mut surface, "DOT");
        t_vec(&mut surface, [1.0, 0.0, 0.0]);
        t_vec(&mut surface, [0.0, 1.0, 0.0]);
    } else {
        push_u8_string(&mut surface, "skin-law");
        t_long(&mut surface, 1);
        push_u8_string(&mut surface, "SPLINE_LAW");
        t_long(&mut surface, 5);
        append_generated_float_array(&mut surface, &[0.0, 0.5, 1.0]);
        append_generated_float_array(&mut surface, &[1.0, 2.0, 3.0]);
        t_pos(&mut surface, [1.0, 2.0, 3.0]);
    }
    surface.extend_from_slice(&generated_curve_block());
    surface.extend_from_slice(&generated_surface_block());
    t_dbl(&mut surface, 0.006);
    for values in [
        &[0.1][..],
        &[0.2, 0.3][..],
        &[][..],
        &[][..],
        &[][..],
        &[][..],
    ] {
        append_generated_float_array(&mut surface, values);
    }
    surface.push(0x0a);
    surface.push(0x10);
    t_end(&mut surface);
    bytes.splice(old.offset..old.offset + old.len, surface);
    bytes
}

pub(super) fn synthetic_law_spl_sur_smbh(
    name: &str,
    legacy_ranges: bool,
    tail_selector: i64,
) -> Vec<u8> {
    let mut bytes = synthetic_mixed_smbh();
    let start = asm_header::record_stream_start(&bytes).unwrap();
    let limit = asm_header::solved_record_limit(&bytes).unwrap();
    let records = cadmpeg_asm::sab::frame(&bytes, start, limit, 8).unwrap();
    let old = &records[9];
    let mut surface = Vec::new();
    t_subident(&mut surface, "spline");
    t_ident(&mut surface, "surface");
    t_ref(&mut surface, -1);
    t_long(&mut surface, -1);
    t_ref(&mut surface, -1);
    surface.push(0x0f);
    t_ident(&mut surface, name);
    if legacy_ranges {
        for value in [-1.0, 2.0, -3.0, 4.0] {
            t_dbl(&mut surface, value);
        }
    }
    push_u8_string(&mut surface, "primary-law");
    t_long(&mut surface, 1);
    push_u8_string(&mut surface, "SET");
    t_dbl(&mut surface, -2.5);
    t_long(&mut surface, 1);
    push_u8_string(&mut surface, "aux-law");
    t_long(&mut surface, 1);
    push_u8_string(&mut surface, "TERM");
    t_vec(&mut surface, [1.0, 2.0, 3.0]);
    t_long(&mut surface, 1);
    if !legacy_ranges {
        surface.push(0x15);
        surface.extend_from_slice(&tail_selector.to_le_bytes());
    } else {
        assert_eq!(tail_selector, 0);
    }
    match tail_selector {
        0 => {
            surface.extend_from_slice(&generated_surface_block());
            t_dbl(&mut surface, 0.007);
        }
        1 => {
            append_generated_float_array(&mut surface, &[0.0, 0.5, 1.0]);
            append_generated_float_array(&mut surface, &[-1.0, 1.0]);
            t_dbl(&mut surface, 0.008);
            for value in [0i64, 2, 1, 3] {
                surface.push(0x15);
                surface.extend_from_slice(&value.to_le_bytes());
            }
        }
        2 => {
            for value in [-0.5, 1.5, -2.0, 2.0] {
                t_dbl(&mut surface, value);
            }
            for value in [1i64, 2, 0, 4] {
                surface.push(0x15);
                surface.extend_from_slice(&value.to_le_bytes());
            }
        }
        3 | 4 => {}
        _ => panic!("invalid law tail selector"),
    }
    for values in [
        &[0.1][..],
        &[0.2, 0.3][..],
        &[][..],
        &[][..],
        &[][..],
        &[][..],
    ] {
        append_generated_float_array(&mut surface, values);
    }
    surface.push(0x10);
    t_end(&mut surface);
    bytes.splice(old.offset..old.offset + old.len, surface);
    bytes
}

pub(super) fn synthetic_sub_spl_sur_smbh(name: &str) -> Vec<u8> {
    let mut bytes = synthetic_mixed_smbh();
    let start = asm_header::record_stream_start(&bytes).unwrap();
    let limit = asm_header::solved_record_limit(&bytes).unwrap();
    let records = cadmpeg_asm::sab::frame(&bytes, start, limit, 8).unwrap();
    let old = &records[9];
    let mut surface = Vec::new();
    t_subident(&mut surface, "spline");
    t_ident(&mut surface, "surface");
    t_ref(&mut surface, -1);
    t_long(&mut surface, -1);
    t_ref(&mut surface, -1);
    surface.push(0x0f);
    t_ident(&mut surface, name);
    for value in [-1.0, 2.0, -3.0, 4.0] {
        t_dbl(&mut surface, value);
    }
    t_ident(&mut surface, "plane");
    t_pos(&mut surface, [0.1, -0.2, 0.3]);
    t_vec(&mut surface, [0.0, 0.0, 1.0]);
    t_vec(&mut surface, [1.0, 0.0, 0.0]);
    surface.push(0x0b);
    surface.push(0x10);
    t_end(&mut surface);
    bytes.splice(old.offset..old.offset + old.len, surface);
    bytes
}

pub(super) fn append_generated_g2_side(bytes: &mut Vec<u8>, label: &str) {
    push_u8_string(bytes, label);
    t_ident(bytes, "plane");
    t_pos(bytes, [1.0, -2.0, 3.0]);
    t_vec(bytes, [0.0, 0.0, 1.0]);
    t_vec(bytes, [1.0, 0.0, 0.0]);
    bytes.push(0x0b);
    bytes.extend_from_slice(&generated_curve_block());
    bytes.extend_from_slice(&generated_pcurve_block());
    t_vec(bytes, [0.0, 1.0, 0.0]);
    bytes.extend_from_slice(&generated_pcurve_block());
}

pub(super) fn synthetic_g2_blend_spl_sur_smbh(name: &str, full: bool) -> Vec<u8> {
    let mut bytes = synthetic_mixed_smbh();
    let start = asm_header::record_stream_start(&bytes).unwrap();
    let limit = asm_header::solved_record_limit(&bytes).unwrap();
    let records = cadmpeg_asm::sab::frame(&bytes, start, limit, 8).unwrap();
    let old = &records[9];
    let mut surface = Vec::new();
    t_subident(&mut surface, "spline");
    t_ident(&mut surface, "surface");
    t_ref(&mut surface, -1);
    t_long(&mut surface, -1);
    t_ref(&mut surface, -1);
    surface.push(0x0f);
    t_ident(&mut surface, name);
    append_generated_g2_side(&mut surface, "first");
    surface.push(0x15);
    surface.extend_from_slice(&(if full { 11i64 } else { 12i64 }).to_le_bytes());
    if full {
        surface.extend_from_slice(&generated_surface_block());
        t_dbl(&mut surface, 0.002);
    } else {
        for value in 1..=9 {
            t_dbl(&mut surface, f64::from(value));
        }
        t_dbl(&mut surface, 0.003);
        t_long(&mut surface, 44);
        surface.extend_from_slice(&generated_pcurve_block());
    }
    append_generated_g2_side(&mut surface, "second");
    surface.extend_from_slice(&generated_surface_block());
    surface.extend_from_slice(&generated_curve_block());
    t_dbl(&mut surface, -0.5);
    t_dbl(&mut surface, 1.5);
    t_long(&mut surface, 8);
    for value in [-1.0, 2.0, -3.0, 4.0, 0.1, 0.2, 0.3, 0.4] {
        t_dbl(&mut surface, value);
    }
    surface.extend_from_slice(&generated_surface_block());
    t_dbl(&mut surface, 0.0095);
    t_long(&mut surface, 1);
    t_dbl(&mut surface, 0.25);
    t_long(&mut surface, 0);
    t_long(&mut surface, 2);
    t_dbl(&mut surface, 0.5);
    t_dbl(&mut surface, 0.75);
    surface.push(0x10);
    t_end(&mut surface);
    bytes.splice(old.offset..old.offset + old.len, surface);
    bytes
}

pub(super) fn synthetic_rational_cyl_spl_sur_smbh() -> Vec<u8> {
    let mut bytes = synthetic_cyl_spl_sur_smbh();
    let old = generated_surface_block();
    let start = bytes
        .windows(old.len())
        .rposition(|window| window == old)
        .expect("generated solved surface cache");
    bytes.splice(start..start + old.len(), generated_rational_surface_block());
    bytes
}

pub(super) fn synthetic_ref_cyl_spl_sur_smbh() -> Vec<u8> {
    let mut bytes = synthetic_cyl_spl_sur_smbh();
    let start = asm_header::record_stream_start(&bytes).unwrap();
    let limit = asm_header::solved_record_limit(&bytes).unwrap();
    let records = cadmpeg_asm::sab::frame(&bytes, start, limit, 8).unwrap();
    let asmheader = &records[0];
    let surface = &records[9];
    let marker = b"\x0f\x0d\x0bcyl_spl_sur";
    let relative = bytes[surface.offset..surface.offset + surface.len]
        .windows(marker.len())
        .position(|window| window == marker)
        .unwrap();
    let target_start = surface.offset + relative;
    let target_end = surface.offset + surface.len - 1;
    let target = bytes[target_start..target_end].to_vec();

    let mut reference = Vec::new();
    reference.extend_from_slice(b"\x0f\x0d\x03ref\x04");
    reference.extend_from_slice(&0i64.to_le_bytes());
    reference.push(0x10);
    bytes.splice(target_start..target_end, reference);
    let asmheader_end = asmheader.offset + asmheader.len - 1;
    bytes.splice(asmheader_end..asmheader_end, target);
    bytes
}

pub(super) fn synthetic_revision_ref_directrix_cyl_spl_sur_smbh() -> Vec<u8> {
    let mut bytes = synthetic_versioned_cyl_spl_sur_smbh();
    let start = asm_header::record_stream_start(&bytes).unwrap();
    let limit = asm_header::solved_record_limit(&bytes).unwrap();
    let records = cadmpeg_asm::sab::frame(&bytes, start, limit, 8).unwrap();
    let asmheader = &records[0];

    let mut target = Vec::new();
    target.push(0x0f);
    t_ident(&mut target, "exact_int_cur");
    target.extend_from_slice(&generated_curve_block());
    target.extend_from_slice(&generated_surface_block());
    t_dbl(&mut target, 0.009);
    target.push(0x10);
    let target_start = bytes
        .windows(target.len())
        .position(|window| window == target)
        .expect("inline directrix definition");

    let mut reference = vec![0x0f, 0x04];
    reference.extend_from_slice(&0i64.to_le_bytes());
    reference.push(0x10);
    bytes.splice(target_start..target_start + target.len(), reference);
    let asmheader_end = asmheader.offset + asmheader.len - 1;
    bytes.splice(asmheader_end..asmheader_end, target);
    bytes
}

pub(super) fn synthetic_rb_blend_spl_sur_smbh() -> Vec<u8> {
    let mut bytes = synthetic_mixed_smbh();
    let start = asm_header::record_stream_start(&bytes).unwrap();
    let limit = asm_header::solved_record_limit(&bytes).unwrap();
    let records = cadmpeg_asm::sab::frame(&bytes, start, limit, 8).unwrap();
    let old = &records[9];

    let mut surface = Vec::new();
    t_subident(&mut surface, "spline");
    t_ident(&mut surface, "surface");
    t_ref(&mut surface, -1);
    t_long(&mut surface, -1);
    t_ref(&mut surface, -1);
    surface.push(0x0f);
    t_ident(&mut surface, "rb_blend_spl_sur");
    push_u8_string(&mut surface, "blend_support_surface");
    t_subident(&mut surface, "plane");
    surface.extend_from_slice(&generated_surface_block());
    push_u8_string(&mut surface, "blend_support_surface");
    t_subident(&mut surface, "sphere");
    surface.extend_from_slice(&generated_surface_block());
    surface.extend_from_slice(&generated_curve_block());
    t_dbl(&mut surface, -0.3);
    t_dbl(&mut surface, -0.3);
    push_tagged_i64(&mut surface, 0x15, -1);
    surface.extend_from_slice(&generated_surface_block());
    t_dbl(&mut surface, 0.001);
    surface.push(0x10);
    t_end(&mut surface);
    bytes.splice(old.offset..old.offset + old.len, surface);
    bytes
}

pub(super) fn append_generated_rolling_ball_side(bytes: &mut Vec<u8>, label: &str, x: f64) {
    push_u8_string(
        bytes,
        if label == "left" {
            "blend_support_surface"
        } else {
            "blend_support_curve"
        },
    );
    t_ident(bytes, "plane");
    t_pos(bytes, [x, 0.0, 0.0]);
    t_vec(bytes, [0.0, 0.0, 1.0]);
    t_vec(bytes, [1.0, 0.0, 0.0]);
    bytes.push(0x0b);
    bytes.extend_from_slice(&[0x0b; 4]);
    bytes.extend_from_slice(&generated_curve_block());
    bytes.extend_from_slice(&[0x0b, 0x0b]);
    bytes.extend_from_slice(&generated_pcurve_block());
    t_pos(bytes, [x, 2.0, 3.0]);
    t_ident(bytes, "nullbs");
    t_long(bytes, if label == "left" { 3 } else { 4 });
    t_ident(bytes, "nullbs");
}

pub(super) fn synthetic_full_rolling_ball_smbh(name: &str) -> Vec<u8> {
    synthetic_full_rolling_ball_with_tail_smbh(name, 0)
}

pub(super) fn synthetic_full_rolling_ball_with_tail_smbh(name: &str, tail_form: i64) -> Vec<u8> {
    let mut bytes = synthetic_mixed_smbh();
    let start = asm_header::record_stream_start(&bytes).unwrap();
    let limit = asm_header::solved_record_limit(&bytes).unwrap();
    let records = cadmpeg_asm::sab::frame(&bytes, start, limit, 8).unwrap();
    let old = &records[9];
    let mut surface = Vec::new();
    t_subident(&mut surface, "spline");
    t_ident(&mut surface, "surface");
    t_ref(&mut surface, -1);
    t_long(&mut surface, -1);
    t_ref(&mut surface, -1);
    surface.push(0x0f);
    t_ident(&mut surface, name);
    t_long(&mut surface, 22507);
    append_generated_rolling_ball_side(&mut surface, "left", 1.0);
    append_generated_rolling_ball_side(&mut surface, "right", 4.0);
    surface.extend_from_slice(&generated_curve_block());
    surface.extend_from_slice(&[0x0b, 0x0b]);
    for value in [-0.3, -0.6] {
        t_dbl(&mut surface, value);
    }
    surface.push(0x15);
    surface.extend_from_slice(&(-1i64).to_le_bytes());
    for value in [-1.0, 2.0] {
        surface.push(0x0a);
        t_dbl(&mut surface, value);
    }
    surface.push(0x0b);
    surface.push(0x0b);
    t_long(&mut surface, 1);
    for value in [0.1, 0.2] {
        t_dbl(&mut surface, value);
    }
    t_long(&mut surface, 17);
    append_revision_surface_tail_head(&mut surface, tail_form, 0.004);
    append_revision_surface_tail_discontinuities(&mut surface);
    if matches!(name, "sss_blend_spl_sur" | "sssblndsur") {
        push_u8_string(&mut surface, "third");
        t_ident(&mut surface, "plane");
        t_pos(&mut surface, [0.0, 0.0, 1.0]);
        t_vec(&mut surface, [0.0, 1.0, 0.0]);
        t_vec(&mut surface, [1.0, 0.0, 0.0]);
        surface.push(0x0b);
        surface.extend_from_slice(&generated_curve_block());
        t_ident(&mut surface, "nullbs");
        t_vec(&mut surface, [0.0, 1.0, 0.0]);
        surface.extend_from_slice(&generated_pcurve_block());
        t_long(&mut surface, 23);
        t_ident(&mut surface, "nullbs");
        surface.push(0x0b);
    }
    for value in [11, 12, 13] {
        t_long(&mut surface, value);
    }
    surface.push(0x10);
    t_end(&mut surface);
    bytes.splice(old.offset..old.offset + old.len, surface);
    bytes
}

pub(super) fn append_generated_variable_blend_side(bytes: &mut Vec<u8>, label: &str, x: f64) {
    push_u8_string(
        bytes,
        if label == "left" {
            "blend_support_surface"
        } else {
            "blendsupcur"
        },
    );
    t_ident(bytes, "plane");
    t_pos(bytes, [x, 0.0, 0.0]);
    t_vec(bytes, [0.0, 0.0, 1.0]);
    t_vec(bytes, [1.0, 0.0, 0.0]);
    bytes.push(0x0b);
    bytes.extend_from_slice(&[0x0b; 4]);
    bytes.extend_from_slice(&generated_curve_block());
    bytes.extend_from_slice(&[0x0b, 0x0b]);
    bytes.extend_from_slice(&generated_pcurve_block());
    t_pos(bytes, [x, 2.0, 3.0]);
    t_ident(bytes, "nullbs");
    t_long(bytes, if label == "left" { 0 } else { 5 });
    t_ident(bytes, "nullbs");
}

pub(super) fn append_generated_variable_blend_value(
    bytes: &mut Vec<u8>,
    parameters: [f64; 2],
    radii: [f64; 2],
) {
    push_u8_string(bytes, "two_ends");
    t_long(bytes, 7);
    bytes.push(0x15);
    bytes.extend_from_slice(&3i64.to_le_bytes());
    bytes.push(0x0a);
    for value in parameters.into_iter().chain(radii) {
        t_dbl(bytes, value);
    }
}

/// An `edge_offset` radius law with no leading sub-discriminator: the
/// law-domain parameter range and one offset length.
pub(super) fn append_generated_variable_blend_edge_offset_value(
    bytes: &mut Vec<u8>,
    parameters: [f64; 2],
    offset: f64,
) {
    push_u8_string(bytes, "edge_offset");
    push_tagged_i64(bytes, 0x15, 3);
    bytes.push(0x0a);
    for value in parameters.into_iter().chain([offset]) {
        t_dbl(bytes, value);
    }
}

/// An `interp` radius law: the law-domain parameter range, a `(u,radius)` BS2
/// function, the extension enum, the point count, and one radius point. The
/// payload ends at that point — nothing gates a trailing scalar pair.
pub(super) fn append_generated_variable_blend_interp_value(bytes: &mut Vec<u8>) {
    push_u8_string(bytes, "interp");
    push_tagged_i64(bytes, 0x15, 0);
    bytes.push(0x0a);
    t_dbl(bytes, 0.0);
    t_dbl(bytes, 1.0);
    bytes.extend_from_slice(&generated_pcurve_block());
    push_tagged_i64(bytes, 0x15, 2);
    push_tagged_i64(bytes, 0x04, 1);
    for value in [0.5, 1.5, 0.25, 0.75] {
        t_dbl(bytes, value);
    }
    bytes.push(0x13);
    for value in [1.0f64, 2.0, 3.0] {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    bytes.push(0x14);
    for value in [0.0f64, 0.0, 1.0] {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
}

/// Which radius law the synthetic stream stores as its first blend value.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum FirstRadiusLaw {
    TwoEnds,
    Interp,
    /// `edge_offset` with no leading sub-discriminator: two law-domain
    /// parameters and one offset length.
    EdgeOffset,
}

pub(super) fn synthetic_variable_blend_smbh(name: &str) -> Vec<u8> {
    synthetic_variable_blend_smbh_with_selector(name, false, None, [None, None])
}

pub(super) fn synthetic_variable_blend_smbh_with_branch(
    name: &str,
    rounded_chamfer: bool,
) -> Vec<u8> {
    synthetic_variable_blend_smbh_with_selector(
        name,
        rounded_chamfer,
        rounded_chamfer.then_some(3),
        [None, None],
    )
}

pub(super) fn synthetic_variable_blend_smbh_with_selector(
    name: &str,
    two_radii: bool,
    cross_section_selector: Option<i64>,
    v_range: [Option<f64>; 2],
) -> Vec<u8> {
    synthetic_variable_blend_smbh_inner(
        name,
        two_radii,
        cross_section_selector,
        v_range,
        FirstRadiusLaw::TwoEnds,
        0,
    )
}

/// The same stream whose shared revision-gated surface tail takes the given
/// form.
pub(super) fn synthetic_variable_blend_smbh_with_tail_form(name: &str, tail_form: i64) -> Vec<u8> {
    synthetic_variable_blend_smbh_inner(
        name,
        false,
        None,
        [None, None],
        FirstRadiusLaw::TwoEnds,
        tail_form,
    )
}

/// The same stream with an `interp` first radius law, which places a radius
/// point immediately before the cross-section enum.
pub(super) fn synthetic_variable_blend_smbh_with_interp_radius(
    name: &str,
    cross_section_selector: Option<i64>,
) -> Vec<u8> {
    synthetic_variable_blend_smbh_inner(
        name,
        false,
        cross_section_selector,
        [None, None],
        FirstRadiusLaw::Interp,
        0,
    )
}

/// The same stream with an `edge_offset` first radius law carrying no leading
/// sub-discriminator.
pub(super) fn synthetic_variable_blend_smbh_with_edge_offset_radius(name: &str) -> Vec<u8> {
    synthetic_variable_blend_smbh_inner(
        name,
        false,
        None,
        [None, None],
        FirstRadiusLaw::EdgeOffset,
        0,
    )
}

pub(super) fn synthetic_variable_blend_smbh_inner(
    name: &str,
    two_radii: bool,
    cross_section_selector: Option<i64>,
    v_range: [Option<f64>; 2],
    first_value: FirstRadiusLaw,
    tail_form: i64,
) -> Vec<u8> {
    let mut bytes = synthetic_mixed_smbh();
    let start = asm_header::record_stream_start(&bytes).unwrap();
    let limit = asm_header::solved_record_limit(&bytes).unwrap();
    let records = cadmpeg_asm::sab::frame(&bytes, start, limit, 8).unwrap();
    let old = &records[9];
    let mut surface = Vec::new();
    t_subident(&mut surface, "spline");
    t_ident(&mut surface, "surface");
    t_ref(&mut surface, -1);
    t_long(&mut surface, -1);
    t_ref(&mut surface, -1);
    surface.push(0x0f);
    t_ident(&mut surface, name);
    t_long(&mut surface, 23100);
    append_generated_variable_blend_side(&mut surface, "left", 1.0);
    append_generated_variable_blend_side(&mut surface, "right", 4.0);
    surface.extend_from_slice(&generated_curve_block());
    surface.extend_from_slice(&[0x0b, 0x0b]);
    t_dbl(&mut surface, -0.2);
    t_dbl(&mut surface, 0.4);
    surface.push(0x15);
    surface.extend_from_slice(&i64::from(two_radii).to_le_bytes());
    match first_value {
        FirstRadiusLaw::Interp => append_generated_variable_blend_interp_value(&mut surface),
        FirstRadiusLaw::EdgeOffset => {
            append_generated_variable_blend_edge_offset_value(&mut surface, [0.25, 0.75], 1.5);
        }
        FirstRadiusLaw::TwoEnds => {
            append_generated_variable_blend_value(&mut surface, [0.25, 0.75], [1.5, 2.5]);
        }
    }
    if !two_radii {
        if let Some(selector) = cross_section_selector {
            surface.push(0x15);
            surface.extend_from_slice(&selector.to_le_bytes());
            if matches!(selector, 1 | 7) {
                t_dbl(&mut surface, 2.0);
                t_dbl(&mut surface, 2.0);
            }
        }
    }
    if two_radii {
        append_generated_variable_blend_value(&mut surface, [0.1, 0.9], [3.5, 4.5]);
        if let Some(selector) = cross_section_selector {
            surface.push(0x15);
            surface.extend_from_slice(&selector.to_le_bytes());
            if selector == 3 {
                surface.push(0x0a);
                append_generated_variable_blend_value(&mut surface, [0.0, 1.0], [5.5, 6.5]);
            }
        }
    }
    for value in [-1.0, 2.0] {
        surface.push(0x0a);
        t_dbl(&mut surface, value);
    }
    // Second interval `(T lo, F)`: a lower bound with an unbounded-above
    // marker, or both bounds absent when `v_range` is `[None, None]`.
    for bound in v_range {
        match bound {
            Some(value) => {
                surface.push(0x0a);
                t_dbl(&mut surface, value);
            }
            None => surface.push(0x0b),
        }
    }
    t_long(&mut surface, 11);
    t_dbl(&mut surface, 0.125);
    t_dbl(&mut surface, 0.6);
    t_long(&mut surface, 12);
    append_revision_surface_tail_head(&mut surface, tail_form, 0.004);
    for values in [
        &[0.125][..],
        &[][..],
        &[0.25, 0.375][..],
        &[][..],
        &[0.5][..],
        &[][..],
    ] {
        t_long(&mut surface, i64::try_from(values.len()).unwrap());
        for value in values {
            t_dbl(&mut surface, *value);
        }
    }
    surface.push(0x0a);
    for value in [31, 32, 33] {
        t_long(&mut surface, value);
    }
    surface.extend_from_slice(&generated_curve_block());
    surface.extend_from_slice(&[0x0b, 0x0b]);
    surface.push(0x0a);
    surface.push(0x0b);
    surface.push(0x0a);
    t_dbl(&mut surface, 0.0);
    surface.push(0x0a);
    t_dbl(&mut surface, 1.0);
    surface.extend_from_slice(&generated_curve_block());
    t_ident(&mut surface, "nullbs");
    surface.push(0x10);
    t_end(&mut surface);
    bytes.splice(old.offset..old.offset + old.len, surface);
    bytes
}

pub(super) fn append_vertex_boundary_common(bytes: &mut Vec<u8>, kind: &str, x: f64) {
    push_u8_string(bytes, kind);
    bytes.push(0x0a);
    t_pos(bytes, [x, 0.0, 0.0]);
    bytes.push(0x0b);
    bytes.push(0x0a);
    t_dbl(bytes, x + 0.25);
}

pub(super) fn synthetic_vertex_blend_smbh(name: &str) -> Vec<u8> {
    let mut bytes = synthetic_mixed_smbh();
    let start = asm_header::record_stream_start(&bytes).unwrap();
    let limit = asm_header::solved_record_limit(&bytes).unwrap();
    let records = cadmpeg_asm::sab::frame(&bytes, start, limit, 8).unwrap();
    let old = &records[9];
    let mut surface = Vec::new();
    t_subident(&mut surface, "spline");
    t_ident(&mut surface, "surface");
    t_ref(&mut surface, -1);
    t_long(&mut surface, -1);
    t_ref(&mut surface, -1);
    surface.push(0x0f);
    t_ident(&mut surface, name);
    t_long(&mut surface, 4);

    append_vertex_boundary_common(&mut surface, "circle", 1.0);
    surface.extend_from_slice(&generated_curve_block());
    surface.push(0x15);
    surface.extend_from_slice(&1i64.to_le_bytes());
    t_pos(&mut surface, [2.0, 3.0, 4.0]);
    t_dbl(&mut surface, 0.1);
    t_dbl(&mut surface, 0.9);
    surface.push(0x0b);

    append_vertex_boundary_common(&mut surface, "deg", 2.0);
    t_pos(&mut surface, [5.0, 6.0, 7.0]);
    t_vec(&mut surface, [1.0, 0.0, 0.0]);
    t_vec(&mut surface, [0.0, 1.0, 0.0]);

    append_vertex_boundary_common(&mut surface, "pcurve", 3.0);
    t_ident(&mut surface, "plane");
    t_pos(&mut surface, [0.0, 0.0, 0.0]);
    t_vec(&mut surface, [0.0, 0.0, 1.0]);
    t_vec(&mut surface, [1.0, 0.0, 0.0]);
    surface.push(0x0b);
    surface.extend_from_slice(&generated_pcurve_block());
    surface.push(0x0a);
    t_dbl(&mut surface, 0.002);

    append_vertex_boundary_common(&mut surface, "plane", 4.0);
    t_vec(&mut surface, [0.0, 0.0, 1.0]);
    t_dbl(&mut surface, -0.5);
    t_dbl(&mut surface, 1.5);
    surface.extend_from_slice(&generated_curve_block());

    t_long(&mut surface, 17);
    t_dbl(&mut surface, 0.003);
    surface.extend_from_slice(&generated_surface_block());
    t_dbl(&mut surface, 0.004);
    surface.push(0x10);
    t_end(&mut surface);
    bytes.splice(old.offset..old.offset + old.len, surface);
    bytes
}

pub(super) fn synthetic_partial_rb_blend_spl_sur_smbh() -> Vec<u8> {
    let mut bytes = synthetic_rb_blend_spl_sur_smbh();
    let marker = b"\x0e\x06sphere";
    let start = bytes
        .windows(marker.len())
        .position(|window| window == marker)
        .unwrap();
    bytes.drain(start..start + marker.len());
    bytes
}

/// Two triangular faces sharing one edge: face 4 rests on a plane (analytic),
/// face 5 on a `spline-surface` (undecoded → unknown-geometry carrier). The
/// shared edge 16 is used by coedge 10 (face 4, forward) and coedge 13 (face 5,
/// reversed), which must decode as mutually-referencing partners.
pub(super) fn synthetic_mixed_smbh() -> Vec<u8> {
    let mut r = Vec::new();

    // 0: asmheader
    t_ident(&mut r, "asmheader");
    push_u8_string(&mut r, "231.6.3.65535");
    t_end(&mut r);

    // 1: body
    t_ident(&mut r, "body");
    t_ref(&mut r, -1); // 0 attrib
    t_long(&mut r, -1); // 1 history
    t_ref(&mut r, -1); // 2 null
    t_ref(&mut r, 2); // 3 first_region
    t_ref(&mut r, -1); // 4 wire
    t_ref(&mut r, -1); // 5 transform
    t_end(&mut r);

    // 2: region
    t_ident(&mut r, "region");
    t_ref(&mut r, -1);
    t_long(&mut r, -1);
    t_ref(&mut r, -1);
    t_ref(&mut r, -1);
    t_ref(&mut r, 3); // first_shell
    t_ref(&mut r, 1); // owner_body
    t_end(&mut r);

    // 3: shell (first_face = 4)
    t_ident(&mut r, "shell");
    t_ref(&mut r, -1);
    t_long(&mut r, -1);
    t_ref(&mut r, -1);
    t_ref(&mut r, -1);
    t_ref(&mut r, -1);
    t_ref(&mut r, 4); // first_face
    t_ref(&mut r, -1);
    t_ref(&mut r, 2); // owner_region
    t_end(&mut r);

    // Face builder: next_face, first_loop, surface.
    let face = |r: &mut Vec<u8>, next: i64, first_loop: i64, surface: i64| {
        t_ident(r, "face");
        t_ref(r, -1); // 0 attrib
        t_long(r, -1); // 1 history
        t_ref(r, -1); // 2 null
        t_ref(r, next); // 3 next_face
        t_ref(r, first_loop); // 4 first_loop
        t_ref(r, 3); // 5 owner_shell
        t_ref(r, -1); // 6 null
        t_ref(r, surface); // 7 surface
        r.push(0x0b); // 8 sense forward
        r.push(0x0b); // 9 sides single
        t_end(r);
    };
    face(&mut r, 5, 6, 8); // 4: plane face
    face(&mut r, -1, 7, 9); // 5: spline face

    // Loop builder: first_coedge, owner_face.
    let lp = |r: &mut Vec<u8>, first_coedge: i64, owner_face: i64| {
        t_ident(r, "loop");
        t_ref(r, -1);
        t_long(r, -1);
        t_ref(r, -1);
        t_ref(r, -1); // next_loop
        t_ref(r, first_coedge);
        t_ref(r, owner_face);
        t_end(r);
    };
    lp(&mut r, 10, 4); // 6: loop of face 4
    lp(&mut r, 13, 5); // 7: loop of face 5

    // 8: plane-surface
    t_subident(&mut r, "plane");
    t_ident(&mut r, "surface");
    t_ref(&mut r, -1);
    t_long(&mut r, -1);
    t_ref(&mut r, -1);
    t_pos(&mut r, [0.0, 0.0, 0.0]);
    t_vec(&mut r, [0.0, 0.0, 1.0]);
    t_vec(&mut r, [1.0, 0.0, 0.0]);
    r.push(0x0b);
    t_end(&mut r);

    // 9: spline-surface (undecoded carrier; only needs to frame cleanly)
    t_subident(&mut r, "spline");
    t_ident(&mut r, "surface");
    t_ref(&mut r, -1);
    t_long(&mut r, -1);
    t_ref(&mut r, -1);
    t_dbl(&mut r, 0.0);
    r.push(0x0b);
    t_end(&mut r);

    // Coedge builder: next, prev, partner, edge, sense_reversed, owner_loop.
    let ce =
        |r: &mut Vec<u8>, next: i64, prev: i64, partner: i64, edge: i64, rev: bool, owner: i64| {
            t_ident(r, "coedge");
            t_ref(r, -1); // 0 attrib
            t_long(r, -1); // 1 history
            t_ref(r, -1); // 2 null
            t_ref(r, next); // 3 next
            t_ref(r, prev); // 4 prev
            t_ref(r, partner); // 5 partner
            t_ref(r, edge); // 6 edge
            r.push(if rev { 0x0a } else { 0x0b }); // 7 sense
            t_ref(r, owner); // 8 owner_loop
            t_long(r, 0); // 9 reserved
            t_ref(r, -1); // 10 pcurve
            t_end(r);
        };
    // Loop of face 4: 10 -> 11 -> 12 -> 10; coedge 10 partners coedge 13.
    ce(&mut r, 11, 12, 13, 16, false, 6); // 10 (shared edge, forward)
    ce(&mut r, 12, 10, -1, 17, false, 6); // 11
    ce(&mut r, 10, 11, -1, 18, false, 6); // 12
                                          // Loop of face 5: 13 -> 14 -> 15 -> 13; coedge 13 partners coedge 10.
    ce(&mut r, 14, 15, 10, 16, true, 7); // 13 (shared edge, reversed)
    ce(&mut r, 15, 13, -1, 19, false, 7); // 14
    ce(&mut r, 13, 14, -1, 20, false, 7); // 15

    // Edge builder: start_vertex, end_vertex.
    let edge = |r: &mut Vec<u8>, start: i64, end: i64| {
        t_ident(r, "edge");
        t_ref(r, -1); // 0 attrib
        t_long(r, -1); // 1 history
        t_ref(r, -1); // 2 null
        t_ref(r, start); // 3 start_vertex
        t_dbl(r, 0.0); // 4 t_start
        t_ref(r, end); // 5 end_vertex
        t_dbl(r, 1.0); // 6 t_end
        t_ref(r, -1); // 7 owner_coedge
        t_ref(r, -1); // 8 curve (none)
        r.push(0x0b); // 9 sense
        push_u8_string(r, "unknown"); // 10 continuity
        t_end(r);
    };
    edge(&mut r, 21, 22); // 16 A->B (shared)
    edge(&mut r, 22, 23); // 17 B->C
    edge(&mut r, 23, 21); // 18 C->A
    edge(&mut r, 21, 24); // 19 A->D
    edge(&mut r, 24, 22); // 20 D->B

    // Vertex builder: owning_edge, point.
    let vert = |r: &mut Vec<u8>, owning_edge: i64, index_flag: i64, point: i64| {
        t_ident(r, "vertex");
        t_ref(r, -1);
        t_long(r, -1);
        t_ref(r, -1);
        t_ref(r, owning_edge);
        t_long(r, index_flag);
        t_ref(r, point);
        t_end(r);
    };
    vert(&mut r, 16, 0, 25); // 21 A
    vert(&mut r, 16, 1, 26); // 22 B
    vert(&mut r, 17, 1, 27); // 23 C
    vert(&mut r, 19, 1, 28); // 24 D

    // Points.
    for p in [
        [0.0, 0.0, 0.0],
        [1.0, 0.0, 0.0],
        [0.0, 1.0, 0.0],
        [0.0, -1.0, 0.0],
    ] {
        t_ident(&mut r, "point");
        t_ref(&mut r, -1);
        t_long(&mut r, -1);
        t_ref(&mut r, -1);
        t_pos(&mut r, p);
        t_end(&mut r);
    }

    // History boundary.
    t_ident(&mut r, "delta_state");

    let mut out = smbh_header_prefix();
    out.extend_from_slice(&r);
    out
}

/// Wrap an ASM stream byte blob into a `.f3d` ZIP as `Body1.smbh`.
pub(super) fn f3d_with_smbh(smbh: &[u8]) -> Vec<u8> {
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let stored = crate::zip_write::file_options(CompressionMethod::Stored);
    write_synthetic_manifests(&mut zip, stored);
    zip.start_file("FusionAssetName[Active]/Breps.BlobParts/Body1.smbh", stored)
        .unwrap();
    zip.write_all(smbh).unwrap();
    zip.finish().unwrap().into_inner()
}

#[test]
fn malformed_tspline_cage_degrades_to_a_loss_note() {
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let stored = crate::zip_write::file_options(CompressionMethod::Stored);
    write_synthetic_manifests(&mut zip, stored);
    zip.start_file("FusionAssetName[Active]/Breps.BlobParts/Body1.smbh", stored)
        .unwrap();
    zip.write_all(&synthetic_geometry_smbh()).unwrap();
    zip.start_file(
        "FusionAssetName[Active]/TSplines.BlobParts/Cage1.tsm",
        stored,
    )
    .unwrap();
    // An edge-root index far outside the half-edge range makes the cage
    // internally inconsistent while the entry itself stays well-formed.
    zip.write_all(b"tsm 1.0\ner 999\n").unwrap();
    let archive = zip.finish().unwrap().into_inner();

    let result = F3dCodec
        .decode(&mut Cursor::new(archive), &DecodeOptions::default())
        .expect("an inconsistent cage must not fail the document decode");
    assert!(result.ir.model.subds.is_empty());
    assert!(result
        .report
        .losses
        .iter()
        .any(|loss| loss.severity == cadmpeg_ir::report::Severity::Error
            && loss.message.contains("T-spline control cage not decoded")));
}

#[test]
fn malformed_paramesh_reports_its_entry_and_parser_failure() {
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let stored = crate::zip_write::file_options(CompressionMethod::Stored);
    write_synthetic_manifests(&mut zip, stored);
    let entry = "FusionAssetName[Active]/ParaMeshGeometry.BlobParts/broken.paramesh";
    zip.start_file(entry, stored).unwrap();
    zip.write_all(b"not a paramesh container").unwrap();
    let archive = zip.finish().unwrap().into_inner();

    let decoded = F3dCodec
        .decode(&mut Cursor::new(archive), &DecodeOptions::default())
        .expect("independent malformed mesh entry must not abort document decode");
    assert!(decoded.report.losses.iter().any(|loss| {
        loss.code == LossCode::shared(LossTaxonomy::DecodeDiagnostic)
            && loss.severity == Severity::Error
            && loss.message.contains(entry)
            && loss.message.contains("paramesh container has no magic")
    }));
}

pub(super) fn f3d_with_deflated_smbh(smbh: &[u8]) -> Vec<u8> {
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let stored = crate::zip_write::file_options(CompressionMethod::Stored);
    let deflated = crate::zip_write::file_options(CompressionMethod::Deflated);
    write_synthetic_manifests(&mut zip, stored);
    zip.start_file(
        "FusionAssetName[Active]/Breps.BlobParts/Body1.smbh",
        deflated,
    )
    .unwrap();
    zip.write_all(smbh).unwrap();
    zip.finish().unwrap().into_inner()
}

pub(super) fn set_zip_entry_uncompressed_size(archive: &mut [u8], target: &[u8], size: u32) {
    let central = archive
        .windows(4)
        .enumerate()
        .find_map(|(offset, signature)| {
            if signature != b"PK\x01\x02" || offset + 46 > archive.len() {
                return None;
            }
            let name_length = u16::from_le_bytes(
                archive[offset + 28..offset + 30]
                    .try_into()
                    .expect("central name-length field"),
            ) as usize;
            (archive.get(offset + 46..offset + 46 + name_length) == Some(target)).then_some(offset)
        })
        .expect("generated ZIP central-directory entry");
    archive[central + 24..central + 28].copy_from_slice(&size.to_le_bytes());
}

#[test]
fn oversized_zip_entry_declaration_is_rejected_before_allocation() {
    let mut archive = f3d_with_deflated_smbh(&synthetic_geometry_smbh());
    let target = b"FusionAssetName[Active]/Breps.BlobParts/Body1.smbh";
    set_zip_entry_uncompressed_size(&mut archive, target, u32::MAX);

    let error = F3dCodec
        .decode(&mut Cursor::new(archive), &DecodeOptions::default())
        .expect_err("oversized inflated entry must be rejected");
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(_)),
        "{error:?}"
    );
}

#[test]
fn write_path_protein_bounds_remain_local_constants() {
    // Decode nested Protein ZIPs charge through ArchiveSnapshot / begin_expand.
    // The write-path rewriter has no DecodeContext and keeps these local caps.
    assert_eq!(crate::container::MAX_ARCHIVE_BYTES, 256 * 1024 * 1024);
    assert_eq!(
        crate::container::MAX_INFLATED_ENTRY_BYTES,
        128 * 1024 * 1024
    );
}

#[test]
fn oversized_nested_protein_entry_is_rejected_before_allocation() {
    let target = b"AssetData/InstanceProperties.bin";
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let stored = crate::zip_write::file_options(CompressionMethod::Stored);
    zip.start_file(std::str::from_utf8(target).unwrap(), stored)
        .unwrap();
    zip.write_all(b"properties").unwrap();
    let mut protein = zip.finish().unwrap().into_inner();
    set_zip_entry_uncompressed_size(&mut protein, target, u32::MAX);

    let error =
        crate::materials::patch_protein_appearances(&protein, &std::collections::BTreeMap::new())
            .expect_err("oversized nested Protein entry must be rejected");
    assert!(error.to_string().contains("inflated bytes"));
}

#[test]
fn nested_protein_decode_charges_through_session_expand_ceilings() {
    use cadmpeg_core::decode::ResourceDimension;

    let target = b"AssetData/InstanceProperties.bin";
    let mut nested = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let deflated = crate::zip_write::file_options(CompressionMethod::Deflated);
    nested
        .start_file(std::str::from_utf8(target).unwrap(), deflated)
        .unwrap();
    nested.write_all(b"properties").unwrap();
    let mut protein = nested.finish().unwrap().into_inner();
    set_zip_entry_uncompressed_size(&mut protein, target, u32::MAX);

    let stored = crate::zip_write::file_options(CompressionMethod::Stored);
    let mut outer = zip::ZipWriter::new(Cursor::new(Vec::new()));
    write_synthetic_manifests(&mut outer, stored);
    outer
        .start_file(
            "FusionAssetName[Active]/Breps.BlobParts/BREP.synthetic.smbh",
            stored,
        )
        .unwrap();
    outer.write_all(&synthetic_geometry_smbh()).unwrap();
    outer
        .start_file(
            "FusionAssetName[Active]/ProteinAssets.BlobParts/ProteinAsset.0.protein",
            stored,
        )
        .unwrap();
    outer.write_all(&protein).unwrap();
    let archive = outer.finish().unwrap().into_inner();

    let error = F3dCodec
        .decode(&mut Cursor::new(archive), &DecodeOptions::default())
        .expect_err("nested Protein inflate must refuse session expand ceilings");
    assert!(
        matches!(
            error,
            cadmpeg_core::CodecError::ResourceLimit(limit)
                if limit.dimension == ResourceDimension::DecompressedBytes
        ),
        "{error:?}"
    );
}

#[test]
fn nested_protein_decode_honors_operator_per_expand_ceiling() {
    use cadmpeg_core::decode::ResourceDimension;

    let target = b"AssetData/InstanceProperties.bin";
    let payload = vec![b'x'; 64 * 1024];
    let mut nested = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let deflated = crate::zip_write::file_options(CompressionMethod::Deflated);
    nested
        .start_file(std::str::from_utf8(target).unwrap(), deflated)
        .unwrap();
    nested.write_all(&payload).unwrap();
    let protein = nested.finish().unwrap().into_inner();

    let stored = crate::zip_write::file_options(CompressionMethod::Stored);
    let mut outer = zip::ZipWriter::new(Cursor::new(Vec::new()));
    write_synthetic_manifests(&mut outer, stored);
    outer
        .start_file(
            "FusionAssetName[Active]/Breps.BlobParts/BREP.synthetic.smbh",
            stored,
        )
        .unwrap();
    outer.write_all(&synthetic_geometry_smbh()).unwrap();
    outer
        .start_file(
            "FusionAssetName[Active]/ProteinAssets.BlobParts/ProteinAsset.0.protein",
            stored,
        )
        .unwrap();
    outer.write_all(&protein).unwrap();
    let archive = outer.finish().unwrap().into_inner();

    let mut options = DecodeOptions::default();
    options.policy.limits.max_decompressed_bytes_per_expand = 1024;
    let error = F3dCodec
        .decode(&mut Cursor::new(archive), &options)
        .expect_err("operator per-expand ceiling must bind nested Protein inflate");
    assert!(
        matches!(
            error,
            cadmpeg_core::CodecError::ResourceLimit(limit)
                if limit.dimension == ResourceDimension::DecompressedBytes
        ),
        "{error:?}"
    );
}

pub(super) fn f3d_with_configuration(smbh: &[u8], name: &str, payload: &[u8]) -> Vec<u8> {
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let stored = crate::zip_write::file_options(CompressionMethod::Stored);
    write_synthetic_manifests(&mut zip, stored);
    zip.start_file("FusionAssetName[Active]/Breps.BlobParts/Body1.smbh", stored)
        .unwrap();
    zip.write_all(smbh).unwrap();
    zip.start_file(name, stored).unwrap();
    zip.write_all(payload).unwrap();
    zip.finish().unwrap().into_inner()
}

#[test]
fn form_dispatcher_binds_the_legacy_single_cage_gate() {
    let stream = "FusionAssetName[Active]/FusionDesignSegmentType1/BulkStream.dat";
    let mut bulk = Vec::new();
    let mut cage_list = vec![0; 100];
    cage_list[..4].copy_from_slice(&3u32.to_le_bytes());
    cage_list[4..7].copy_from_slice(b"355");
    cage_list[7..11].copy_from_slice(&205u32.to_le_bytes());
    cage_list[21] = 1;
    cage_list[22..30].copy_from_slice(&201u64.to_le_bytes());
    cage_list[32..36].copy_from_slice(&1u32.to_le_bytes());
    cage_list[36] = 1;
    cage_list[37..45].copy_from_slice(&971u64.to_le_bytes());
    cage_list[47..49].copy_from_slice(&[0xfc, 0]);
    bulk.extend_from_slice(&cage_list);

    let mut paired = vec![0; 15];
    paired[..4].copy_from_slice(&3u32.to_le_bytes());
    paired[4..7].copy_from_slice(b"262");
    paired[7..11].copy_from_slice(&205u32.to_le_bytes());
    bulk.extend_from_slice(&paired);

    let mut object = vec![0; 15];
    object[..4].copy_from_slice(&3u32.to_le_bytes());
    object[4..7].copy_from_slice(b"325");
    object[7..11].copy_from_slice(&971u32.to_le_bytes());
    bulk.extend_from_slice(&object);

    let mut archive = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let stored = crate::zip_write::file_options(CompressionMethod::Stored);
    write_synthetic_manifests(&mut archive, stored);
    archive.start_file(stream, stored).unwrap();
    archive.write_all(&bulk).unwrap();
    let archive = archive.finish().unwrap().into_inner();

    let mut scope = crate::records::DesignParameterScope::empty(
        &format!("f3d:{stream}:scope#201"),
        "Form",
        201,
    );
    scope.reference_members = vec![205];
    let feature_id = crate::ids::neutral_feature_id(&scope);
    let mut features = vec![cadmpeg_ir::features::Feature {
        id: feature_id,
        ordinal: 0,
        name: None,
        suppressed: None,
        parent: None,
        dependencies: Vec::new(),
        source_properties: Default::default(),
        source_tag: Some("Form".into()),
        source_text: None,
        source_content: Vec::new(),
        outputs: Vec::new(),
        definition: cadmpeg_ir::features::FeatureDefinition::Native {
            kind: "Form".into(),
            parameters: Default::default(),
            properties: Default::default(),
        },
        native_ref: Some(scope.id.clone()),
    }];
    let cages = [cadmpeg_ir::SubdSurface {
        id: cadmpeg_ir::ids::SubdId("f3d:model:subd#1".into()),
        scheme: cadmpeg_ir::subd::SubdScheme::CatmullClark,
        vertices: Vec::new(),
        edges: Vec::new(),
        faces: Vec::new(),
        source_object: None,
    }];

    crate::tests::with_scan(&archive, |scan| {
        crate::design::feature_project::bind_form_cages(
            scan,
            std::slice::from_ref(&scope),
            &mut features,
            &cages,
        )
    })
    .expect("legacy Form cage binding");
    assert_eq!(
        features[0].definition,
        cadmpeg_ir::features::FeatureDefinition::Form {
            cages: vec![cages[0].id.clone()],
        }
    );
}

#[test]
fn form_dispatcher_binds_a_unique_long_cage_list() {
    let stream = "FusionAssetName[Active]/FusionDesignSegmentType1/BulkStream.dat";
    let mut cage_list = vec![0; 99];
    cage_list[..4].copy_from_slice(&3u32.to_le_bytes());
    cage_list[4..7].copy_from_slice(b"415");
    cage_list[7..11].copy_from_slice(&205u32.to_le_bytes());
    cage_list[21] = 1;
    cage_list[22..30].copy_from_slice(&201u64.to_le_bytes());
    cage_list[32..36].copy_from_slice(&1u32.to_le_bytes());
    cage_list[36] = 1;
    cage_list[37..45].copy_from_slice(&971u64.to_le_bytes());
    let mut paired = vec![0; 15];
    paired[..4].copy_from_slice(&3u32.to_le_bytes());
    paired[4..7].copy_from_slice(b"258");
    paired[7..11].copy_from_slice(&205u32.to_le_bytes());
    let mut bulk = cage_list;
    bulk.extend_from_slice(&paired);

    let mut archive = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let stored = crate::zip_write::file_options(CompressionMethod::Stored);
    write_synthetic_manifests(&mut archive, stored);
    archive.start_file(stream, stored).unwrap();
    archive.write_all(&bulk).unwrap();
    let archive = archive.finish().unwrap().into_inner();

    let mut scope = crate::records::DesignParameterScope::empty(
        &format!("f3d:{stream}:scope#201"),
        "Form",
        201,
    );
    scope.reference_members = vec![205];
    let feature_id = crate::ids::neutral_feature_id(&scope);
    let mut features = vec![cadmpeg_ir::features::Feature {
        id: feature_id,
        ordinal: 0,
        name: None,
        suppressed: None,
        parent: None,
        dependencies: Vec::new(),
        source_properties: Default::default(),
        source_tag: Some("Form".into()),
        source_text: None,
        source_content: Vec::new(),
        outputs: Vec::new(),
        definition: cadmpeg_ir::features::FeatureDefinition::Native {
            kind: "Form".into(),
            parameters: Default::default(),
            properties: Default::default(),
        },
        native_ref: Some(scope.id.clone()),
    }];
    let cages = [cadmpeg_ir::SubdSurface {
        id: cadmpeg_ir::ids::SubdId("f3d:model:subd#1".into()),
        scheme: cadmpeg_ir::subd::SubdScheme::CatmullClark,
        vertices: Vec::new(),
        edges: Vec::new(),
        faces: Vec::new(),
        source_object: None,
    }];

    crate::tests::with_scan(&archive, |scan| {
        crate::design::feature_project::bind_form_cages(
            scan,
            std::slice::from_ref(&scope),
            &mut features,
            &cages,
        )
    })
    .expect("long Form cage binding");
    assert_eq!(
        features[0].definition,
        cadmpeg_ir::features::FeatureDefinition::Form {
            cages: vec![cages[0].id.clone()],
        }
    );
}
