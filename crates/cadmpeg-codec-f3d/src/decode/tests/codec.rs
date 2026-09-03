// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]
#![allow(
    clippy::cloned_ref_to_slice_refs,
    clippy::default_trait_access,
    clippy::if_not_else,
    clippy::needless_pass_by_value,
    clippy::range_plus_one,
    clippy::semicolon_if_nothing_returned,
    clippy::trivially_copy_pass_by_ref
)]

use std::io::{Cursor, Write};

use cadmpeg_asm::asm_header;
use cadmpeg_core::decode::InspectOptions;
use cadmpeg_ir::codec::{Codec, Confidence, DecodeOptions};
use zip::CompressionMethod;

use crate::container::{self, role};
use crate::loss::F3dLossCode;
use crate::test_support::*;
use crate::F3dCodec;

#[test]
fn asm_header_parses_documented_fields() {
    let bytes = synthetic_smbh();
    let h = asm_header::parse(&bytes).expect("magic present");
    assert_eq!(h.width, 8);
    assert_eq!(h.save_format_version, Some(23100));
    assert_eq!(h.entity_count, Some(7));
    assert_eq!(h.flags, Some(3));
    assert_eq!(h.save_format_major(), Some(231));
    assert_eq!(h.save_format_minor(), Some(0));
    assert!(h.has_history_partition());
    // Flags `3` is the history bit plus revision `1` in bits 1 to 7. Nothing is
    // left over, so no bit reaches the uninterpreted set.
    assert_eq!(h.format_revision(), Some(1));
    assert_eq!(h.unassigned_flags(), Some(0));
    assert_eq!(h.product_family.as_deref(), Some("Autodesk Neutron"));
    assert_eq!(h.product_version.as_deref(), Some("ASM 231.6.3.65535 OSX"));
    assert_eq!(h.save_date.as_deref(), Some("Tue Mar 31 16:16:19 2026"));
    assert_eq!(h.scale, Some(60.0));
    assert_eq!(h.linear, Some(1.0e-6));
    assert_eq!(h.angular, Some(1.0e-10));
}

/// Flag bits 1 to 7 hold the save format's revision number
/// ([spec §1](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/asm.md#1-asm-binary-header)):
/// save format 22300 carries revision 2 and 22500 carries revision 3. Those
/// bits are assigned, so they leave the uninterpreted set; bits 8 and above
/// stay in it.
#[test]
fn asm_header_flag_bits_one_to_seven_hold_the_format_revision() {
    let header = |flags: u64| cadmpeg_asm::kernel_header::KernelHeader {
        width: 8,
        save_format_version: Some(22500),
        record_count: None,
        entity_count: None,
        flags: Some(flags),
        product_family: None,
        product_version: None,
        save_date: None,
        scale: None,
        linear: None,
        angular: None,
    };

    assert_eq!(header(0b0000_0101).format_revision(), Some(2));
    assert_eq!(header(0b0000_0111).format_revision(), Some(3));
    assert_eq!(header(0b1111_1110).format_revision(), Some(0x7f));
    assert_eq!(header(0b0000_0001).format_revision(), Some(0));

    assert_eq!(header(0b0000_0111).unassigned_flags(), Some(0));
    assert_eq!(header(0b1111_1111).unassigned_flags(), Some(0));
    assert_eq!(header(0x1_00).unassigned_flags(), Some(0x1_00));
    assert!(header(0b0000_0101).has_history_partition());
    assert!(!header(0b0000_0100).has_history_partition());
}

#[test]
fn asm_header_absent_on_non_asm_bytes() {
    assert!(asm_header::parse(b"not an asm stream at all").is_none());
    assert!(!asm_header::has_asm_magic(b"PK\x03\x04"));
}

#[test]
fn asm_header_parses_binaryfile4_fields() {
    let bytes = bf4_header_prefix(5);
    assert!(asm_header::has_asm_magic(&bytes));
    let h = asm_header::parse(&bytes).expect("magic present");
    assert_eq!(h.width, 4);
    assert_eq!(h.save_format_version, Some(22700));
    assert_eq!(h.record_count, Some(0));
    assert_eq!(h.entity_count, Some(2));
    assert_eq!(h.flags, Some(5));
    assert_eq!(h.product_family.as_deref(), Some("Autodesk Neutron"));
    assert_eq!(h.product_version.as_deref(), Some("ASM 227.5.0.65535 NT"));
    assert_eq!(h.save_date.as_deref(), Some("Mon Aug  8 02:39:24 2022"));
    assert_eq!(h.scale, Some(50.0));
    assert_eq!(h.linear, Some(1.0e-6));
    assert_eq!(h.angular, Some(1.0e-10));
    // The record stream begins directly after the tolerance doubles.
    assert_eq!(asm_header::record_stream_start(&bytes), Some(bytes.len()));
}

#[test]
fn decodes_binaryfile4_geometry_with_lump_topology() {
    let f3d = f3d_with_smbh(&synthetic_geometry_bf4_smbh());
    let result = F3dCodec
        .decode(&mut Cursor::new(f3d), &DecodeOptions::default())
        .unwrap();

    assert!(result.report().geometry_transferred());
    assert_eq!(result.ir().model.bodies.len(), 1);
    // The ASM-227 `lump` head is emitted as the region record.
    assert_eq!(result.ir().model.regions.len(), 1);
    assert_eq!(result.ir().model.shells.len(), 1);
    assert_eq!(result.ir().model.faces.len(), 1);
    assert_eq!(result.ir().model.edges.len(), 3);
    assert_eq!(result.ir().model.points.len(), 3);

    // The circle arc's stored [-π, -π/2] range is wrapped into the canonical
    // [0, τ] domain with its sweep preserved.
    let arc = result
        .ir()
        .model
        .edges
        .iter()
        .find(|edge| edge.curve.is_some())
        .expect("edge on the ellipse carrier");
    let [start, end] = arc.param_range.expect("arc range");
    assert!((start - std::f64::consts::PI).abs() < 1.0e-9);
    assert!((end - 3.0 * std::f64::consts::FRAC_PI_2).abs() < 1.0e-9);
}

#[test]
fn generated_f3d_rewrites_binaryfile4_geometry() {
    let source = f3d_with_smbh(&synthetic_geometry_bf4_smbh());
    let decoded = F3dCodec
        .decode(&mut Cursor::new(&source), &DecodeOptions::default())
        .expect("generated BinaryFile4 decode");
    let (mut edited, _, fidelity) = decoded.into_parts();
    edited.model.points[0].position.x += 2.5;
    let expected = edited.model.points[0].position;
    let edge = edited
        .model
        .edges
        .iter_mut()
        .find(|edge| edge.curve.is_some())
        .expect("generated BinaryFile4 arc edge");
    let range = edge.param_range.as_mut().expect("generated arc range");
    range[0] += 0.125;
    range[1] -= 0.125;
    let expected_range = *range;
    edited.model.faces[0].sense = match edited.model.faces[0].sense {
        cadmpeg_ir::topology::Sense::Forward => cadmpeg_ir::topology::Sense::Reversed,
        cadmpeg_ir::topology::Sense::Reversed => cadmpeg_ir::topology::Sense::Forward,
    };
    let expected_face_sense = edited.model.faces[0].sense;

    let mut regenerated = Vec::new();
    crate::test_support::plan_inherited_write(&edited, &fidelity, &mut regenerated)
        .expect("generated BinaryFile4 regeneration");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(regenerated), &DecodeOptions::default())
        .expect("regenerated BinaryFile4 decode");
    assert_eq!(round_trip.ir().model.points[0].position, expected);
    assert_eq!(
        round_trip
            .ir()
            .model
            .edges
            .iter()
            .find(|edge| edge.curve.is_some())
            .and_then(|edge| edge.param_range),
        Some(expected_range)
    );
    assert_eq!(round_trip.ir().model.faces[0].sense, expected_face_sense);
}

#[test]
fn generated_f3d_rewrites_binaryfile4_nurbs_integer_fields() {
    let source = f3d_with_smbh(&synthetic_geometry_bf4_nurbs_smbh());
    let decoded = F3dCodec
        .decode(&mut Cursor::new(&source), &DecodeOptions::default())
        .expect("generated BinaryFile4 NURBS decode");
    let (mut edited, _, fidelity) = decoded.into_parts();
    let curve = edited
        .model
        .curves
        .iter_mut()
        .find(|curve| {
            matches!(
                curve.geometry,
                cadmpeg_ir::geometry::CurveGeometry::Nurbs(_)
            )
        })
        .expect("generated BinaryFile4 NURBS curve");
    let cadmpeg_ir::geometry::CurveGeometry::Nurbs(nurbs) = &mut curve.geometry else {
        unreachable!()
    };
    nurbs.degree = 1;
    nurbs.periodic = true;
    nurbs.knots = vec![-1.0, -1.0, 2.0, 2.0, 2.0];
    nurbs.control_points[1].z = 4.5;
    let expected = nurbs.clone();

    let mut regenerated = Vec::new();
    crate::test_support::plan_inherited_write(&edited, &fidelity, &mut regenerated)
        .expect("generated BinaryFile4 NURBS regeneration");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(regenerated), &DecodeOptions::default())
        .expect("regenerated BinaryFile4 NURBS decode");
    assert!(round_trip.ir().model.curves.iter().any(|curve| {
        matches!(&curve.geometry, cadmpeg_ir::geometry::CurveGeometry::Nurbs(nurbs) if nurbs == &expected)
    }));
}

#[test]
fn reversed_edge_sense_reverses_its_conic_carrier() {
    let f3d = f3d_with_smbh(&synthetic_geometry_bf4_smbh_with_arc_sense(0x0a));
    let result = F3dCodec
        .decode(&mut Cursor::new(f3d), &DecodeOptions::default())
        .unwrap();

    // A reversed edge runs `E(t) = C(-t)`; the IR keeps edges forward on
    // their curve, so the conic carrier is emitted with a negated plane
    // normal. The stored parameters already live on the reversed
    // parameterization and transform exactly like a forward edge's.
    let arc = result
        .ir()
        .model
        .edges
        .iter()
        .find(|edge| edge.curve.is_some())
        .expect("edge on the ellipse carrier");
    let [start, end] = arc.param_range.expect("arc range");
    assert!((start - std::f64::consts::PI).abs() < 1.0e-9);
    assert!((end - 3.0 * std::f64::consts::FRAC_PI_2).abs() < 1.0e-9);

    let curve_id = arc.curve.as_ref().expect("curve link");
    let carrier = result
        .ir()
        .model
        .curves
        .iter()
        .find(|curve| &curve.id == curve_id)
        .expect("conic carrier");
    let cadmpeg_ir::geometry::CurveGeometry::Circle { axis, .. } = &carrier.geometry else {
        panic!("expected the ratio-1 ellipse to decode as a circle");
    };
    assert!((axis.z - -1.0).abs() < 1.0e-12, "axis must be negated");
}

#[test]
fn delta_state_boundary_is_located_at_an_exact_identifier() {
    let bytes = synthetic_smbh();
    let off = asm_header::solved_record_limit(&bytes).expect("has a delta_state");
    assert_eq!(&bytes[off..off + 2], &[0x0d, 0x0b]);
    assert_eq!(&bytes[off + 2..off + 13], b"delta_state");

    // The header flag is part of the partition contract. A history-less
    // `.smb` with the same solved prefix has no partition boundary.
    let mut smb = bytes;
    smb[39..47].copy_from_slice(&2u64.to_le_bytes());
    smb.truncate(off);
    assert!(asm_header::solved_record_limit(&smb).is_none());
}

#[test]
fn history_preamble_record_is_the_modern_partition_boundary() {
    let direct = synthetic_smbh();
    let delta = asm_header::solved_record_limit(&direct).unwrap();
    let mut bytes = direct[..delta].to_vec();
    let expected = bytes.len();
    t_ident(&mut bytes, "Begin-of-ASM-History-Data");
    t_end(&mut bytes);
    bytes.extend_from_slice(&direct[delta..]);

    assert_eq!(asm_header::solved_record_limit(&bytes), Some(expected));
    let start = asm_header::record_stream_start(&bytes).unwrap();
    let solved = cadmpeg_asm::sab::frame(&bytes, start, expected, 8).unwrap();
    assert_eq!(
        solved.last().map(|record| record.name.as_str()),
        Some("body")
    );
}

#[test]
fn delta_state_text_inside_a_payload_cannot_cut_the_solved_stream() {
    let direct = synthetic_smbh();
    let delta = asm_header::solved_record_limit(&direct).unwrap();
    let start = asm_header::record_stream_start(&direct).unwrap();
    let mut bytes = direct[..start].to_vec();
    t_ident(&mut bytes, "metadata");
    push_u8_string(&mut bytes, "delta_state");
    t_end(&mut bytes);
    let expected = bytes.len();
    bytes.extend_from_slice(&direct[delta..]);

    assert_eq!(asm_header::solved_record_limit(&bytes), Some(expected));
}

#[test]
fn decode_retains_generated_asm_history_graph() {
    let f3d = f3d_with_smbh(&synthetic_geometry_with_history_smbh());
    let result = F3dCodec
        .decode(&mut Cursor::new(f3d), &DecodeOptions::default())
        .unwrap();

    assert_eq!(f3d_native(result.ir()).asm_histories.len(), 1);
    let history = &f3d_native(result.ir()).asm_histories[0];
    assert_eq!(history.stream_size, Some(2));
    assert_eq!(history.history_entry_count, Some(99));
    assert_eq!(history.states.len(), 2);
    assert_eq!(history.states[0].state_id, 2);
    assert_eq!(history.states[0].next_ref, Some(1));
    assert_eq!(history.states[0].bulletin_boards.len(), 1);
    assert_eq!(history.states[0].bulletin_boards[0].changes.len(), 2);
    assert_eq!(history.states[0].records.len(), 1);
    assert_eq!(history.states[0].records[0].name, "history_payload");
    assert_eq!(history.states[0].records[0].revision_id, Some(1830));
    assert_eq!(history.states[0].records[0].entity_references, [1830, -1]);
    assert!(!history.states[0].records[0].raw_bytes.is_empty());
    assert_eq!(
        history.states[0].bulletin_boards[0].changes[1].kind,
        crate::history_records::AsmEntityChangeKind::Insert
    );
    assert_eq!(history.states[1].previous_ref, Some(0));
    assert_eq!(history.states[1].next_ref, None);
    assert!(result.report().geometry_transferred());
}

#[test]
fn generated_f3d_rewrites_fixed_delta_state_header() {
    let source = f3d_with_smbh(&synthetic_geometry_with_history_smbh());
    let decoded = F3dCodec
        .decode(&mut Cursor::new(&source), &DecodeOptions::default())
        .expect("generated history decode");
    let (mut edited, _, fidelity) = decoded.into_parts();
    update_f3d_native(&mut edited, |native| {
        let history = &mut native.asm_histories[0];
        assert!(history.byte_offset > 0);
        assert!(history.states[0].byte_offset > 0);
        history.stream_size = Some(8);
        history.history_entry_count = Some(120);
        history.states[0].state_id = 8;
        history.states[0].version_flag = 4;
        history.states[0].state_flag = 6;
        history.states[0].previous_ref = Some(12);
        history.states[0].next_ref = Some(14);
        history.states[0].node_index = 16;
        history.states[0].partner_ref = Some(18);
        history.states[0].owner_ref = 20;
        let board = &mut history.states[0].bulletin_boards[0];
        assert!(board.byte_offset > 0);
        board.owner_ref = 22;
        board.number = 24;
        assert!(board.changes[0].byte_offset > 0);
        board.changes[0].kind = crate::history_records::AsmEntityChangeKind::Delete;
        board.changes[0].old_ref = Some(26);
        board.changes[0].new_ref = None;
        board.changes[1].new_ref = Some(28);
    });

    let mut regenerated = Vec::new();
    crate::test_support::plan_inherited_write(&edited, &fidelity, &mut regenerated)
        .expect("delta-state owner regeneration");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(regenerated), &DecodeOptions::default())
        .expect("regenerated history decode");
    let state = &f3d_native(round_trip.ir()).asm_histories[0].states[0];
    assert_eq!(
        f3d_native(round_trip.ir()).asm_histories[0].stream_size,
        Some(8)
    );
    assert_eq!(
        f3d_native(round_trip.ir()).asm_histories[0].history_entry_count,
        Some(120)
    );
    assert_eq!(state.state_id, 8);
    assert_eq!(state.version_flag, 4);
    assert_eq!(state.state_flag, 6);
    assert_eq!(state.previous_ref, Some(12));
    assert_eq!(state.next_ref, Some(14));
    assert_eq!(state.node_index, 16);
    assert_eq!(state.partner_ref, Some(18));
    assert_eq!(state.owner_ref, 20);
    let board = &state.bulletin_boards[0];
    assert_eq!(board.owner_ref, 22);
    assert_eq!(board.number, 24);
    assert_eq!(
        board.changes[0].kind,
        crate::history_records::AsmEntityChangeKind::Delete
    );
    assert_eq!(board.changes[0].old_ref, Some(26));
    assert_eq!(board.changes[0].new_ref, None);
    assert_eq!(board.changes[1].new_ref, Some(28));
}

#[test]
fn classify_matches_spec_families() {
    assert_eq!(classify("a/Breps.BlobParts/x.smbh"), role::BREP_SMBH);
    assert_eq!(classify("a/Breps.BlobParts/x.smb"), role::BREP_SMB);
    assert_eq!(
        classify("a/ProteinAssets.BlobParts/y.protein"),
        role::PROTEIN
    );
    assert_eq!(classify("a/Design1/BulkStream.dat"), role::BULKSTREAM);
    assert_eq!(classify("a/Design1/MetaStream.dat"), role::METASTREAM);
    assert_eq!(classify("Manifest.dat"), role::MANIFEST);
    assert_eq!(classify("a/Previews/thumb.png"), role::PREVIEW);
    assert_eq!(classify("a/x.paramesh"), role::PARAMESH);
    assert_eq!(classify("a/b/"), role::DIRECTORY);
}

use crate::container::classify;

#[test]
fn detect_high_on_f3d_zip_low_on_bare_zip() {
    let codec = F3dCodec;
    let f3d = synthetic_f3d(true);
    assert_eq!(codec.detect(&f3d), Confidence::High);

    // A ZIP whose visible prefix has no f3d markers.
    let mut bare = zip::ZipWriter::new(Cursor::new(Vec::new()));
    bare.start_file(
        "readme.txt",
        crate::zip_write::file_options(CompressionMethod::Stored),
    )
    .unwrap();
    bare.write_all(b"hello").unwrap();
    let bare = bare.finish().unwrap().into_inner();
    assert_eq!(codec.detect(&bare), Confidence::Low);

    assert_eq!(codec.detect(b"\x00\x01\x02\x03 not a zip"), Confidence::No);
}

#[test]
fn inspect_enumerates_and_reads_headers() {
    let codec = F3dCodec;
    let f3d = synthetic_f3d(true);
    let mut cur = Cursor::new(f3d);
    let summary = codec.inspect(&mut cur, &InspectOptions::default()).unwrap();

    assert_eq!(summary.format(), "f3d");
    assert_eq!(summary.container_kind, "zip");

    let smbh = summary
        .entries
        .iter()
        .find(|e| e.role == role::BREP_SMBH)
        .expect("smbh entry present");
    assert_eq!(smbh.compression, "deflate");
    assert_eq!(
        smbh.attributes.get("product_family").map(String::as_str),
        Some("Autodesk Neutron")
    );
    assert_eq!(smbh.attributes.get("scale").map(String::as_str), Some("60"));
    assert!(smbh.attributes.contains_key("history_partition_offset"));
    assert!(smbh.attributes.contains_key("sha256"));

    // The header identifies the unique history-bearing stream.
    assert!(summary
        .notes
        .iter()
        .any(|n| n.contains("history-bearing BREP")));
}

#[test]
fn decode_refuses_when_max_entities_is_zero_before_ir_build() {
    use cadmpeg_core::decode::ResourceDimension;

    let mut options = DecodeOptions::default();
    options.policy.limits.max_entities = 0;
    let error = F3dCodec
        .decode(&mut Cursor::new(synthetic_f3d(true)), &options)
        .expect_err("max_entities=0 must refuse at archive admission");
    assert!(
        matches!(
            error,
            cadmpeg_ir::DecodeFailure::Codec(cadmpeg_core::CodecError::ResourceLimit(limit))
                if limit.dimension == ResourceDimension::Entities
                    && limit.context.operation == "admit F3D archive entries"
        ),
        "{error:?}"
    );
}

#[test]
fn decode_refuses_when_max_entities_is_below_archive_entry_cardinality() {
    use cadmpeg_core::decode::ResourceDimension;

    let mut options = DecodeOptions::default();
    options.policy.limits.max_entities = 1;
    let error = F3dCodec
        .decode(&mut Cursor::new(synthetic_f3d(true)), &options)
        .expect_err("max_entities below archive entry count must refuse at admission");
    assert!(
        matches!(
            error,
            cadmpeg_ir::DecodeFailure::Codec(cadmpeg_core::CodecError::ResourceLimit(limit))
                if limit.dimension == ResourceDimension::Entities
        ),
        "{error:?}"
    );
}

#[test]
fn decode_yields_metadata_and_honest_report() {
    let codec = F3dCodec;
    let f3d = synthetic_f3d(true);
    let mut cur = Cursor::new(f3d);
    let result = codec.decode(&mut cur, &DecodeOptions::default()).unwrap();

    assert!(!result.report().geometry_transferred());
    assert!(result.ir().model.faces.is_empty());
    assert!(result.report().error_count() >= 1);
    assert!(result.report().losses.iter().any(|l| matches!(
        l.code.category(),
        cadmpeg_ir::report::LossCategory::Geometry
    )));

    let unknowns = result.ir().native_unknowns("f3d").unwrap();
    assert_eq!(unknowns.len(), 1);
    assert_eq!(result.source_fidelity().retained_records.len(), 2);
    assert!(result
        .source_fidelity()
        .retained_records
        .iter()
        .all(|record| record.sha256().len() == 64));
    assert!(result
        .source_fidelity()
        .retained_record("f3d:file:source-image#0")
        .is_some());
    let source = result.ir().source.as_ref().expect("source metadata");
    assert_eq!(source.format(), "f3d");
    assert_eq!(
        source.attributes.get("product_family").map(String::as_str),
        Some("Autodesk Neutron")
    );
    // resabs/resnor were carried into tolerances.
    assert_eq!(result.ir().tolerances.linear, 1.0e-6);
    assert_f3d_native_parity(result.ir());
    assert!(result
        .source_fidelity()
        .annotations
        .provenance
        .contains_key(&unknowns[0].id.0));
}

#[test]
fn smb_only_is_an_explicit_geometry_fallback_without_history() {
    let f3d = synthetic_f3d(false);
    with_scan(&f3d, |scan| {
        let fallback = container::select_fallback_brep(scan).unwrap();
        assert!(!fallback.is_smbh);
        assert!(container::select_history_brep(scan).is_none());
        assert!(container::legacy_design_model_breps(scan).is_none());
        let notes = container::summary_notes(scan, container::SummaryScope::FullDecode);
        assert!(notes
            .iter()
            .any(|note| note.contains("no BREP header declares a history partition")));
    });
}

#[test]
fn legacy_design_segment_selects_its_complete_brep_set() {
    let f3d = synthetic_legacy_multi_brep_f3d();
    with_scan(&f3d, |scan| {
        assert!(container::select_fallback_brep(scan).is_none());
        let selected = container::legacy_design_model_breps(scan).unwrap();
        assert_eq!(selected.len(), 2);
        assert!(selected[0].name.ends_with("BREP.first.smb"));
        assert!(selected[1].name.ends_with("BREP.second.smb"));
    });
}

#[test]
fn manifest_selects_design_asset_independently_of_brep_order() {
    let f3d = synthetic_multi_asset_f3d(true);
    with_scan(&f3d, |scan| {
        assert_eq!(scan.design_asset_folder(), Some("DesignAsset[Active]"));
        assert_eq!(scan.breps.len(), 2);
        let design_breps = container::design_breps(scan).collect::<Vec<_>>();
        assert_eq!(design_breps.len(), 1);
        assert!(design_breps[0].name.ends_with("BREP.design.smb"));
        assert!(container::select_history_brep(scan).is_none());
        assert_eq!(
            container::select_fallback_brep(scan).map(|brep| brep.name.as_str()),
            Some("DesignAsset[Active]/Breps.BlobParts/BREP.design.smb")
        );
        let streams = scan
            .entries
            .iter()
            .filter(|entry| scan.is_design_stream(entry, role::BULKSTREAM))
            .map(|entry| entry.name.as_str())
            .collect::<Vec<_>>();
        assert_eq!(
            streams,
            ["DesignAsset[Active]/FusionDesignSegmentType1/BulkStream.dat"]
        );
    });
}

#[test]
fn manifest_selects_brep_less_design_asset() {
    let f3d = synthetic_multi_asset_f3d(false);
    with_scan(&f3d, |scan| {
        assert_eq!(scan.design_asset_folder(), Some("DesignAsset[Active]"));
        assert_eq!(scan.breps.len(), 1);
        assert_eq!(container::design_breps(scan).count(), 0);
        assert!(container::select_fallback_brep(scan).is_none());
        assert!(container::select_history_brep(scan).is_none());
    });
}

#[test]
fn decode_uses_manifest_selected_geometry_not_the_first_brep_asset() {
    let decoded = F3dCodec
        .decode(
            &mut Cursor::new(synthetic_multi_asset_f3d(true)),
            &DecodeOptions::default(),
        )
        .unwrap();
    assert!(decoded.report().geometry_transferred());
    assert_eq!(decoded.ir().model.bodies.len(), 1);
    assert_eq!(
        decoded
            .ir()
            .source
            .as_ref()
            .and_then(|source| source.attributes.get("asset_folder"))
            .map(String::as_str),
        Some("DesignAsset[Active]")
    );
    assert!(decoded
        .ir()
        .model
        .bodies
        .iter()
        .all(|body| !body.id.0.contains("BREP.sibling")));
}

#[test]
fn decode_does_not_use_a_sibling_brep_for_a_brep_less_design_asset() {
    let decoded = F3dCodec
        .decode(
            &mut Cursor::new(synthetic_multi_asset_f3d(false)),
            &DecodeOptions::default(),
        )
        .unwrap();
    assert!(decoded.ir().model.bodies.is_empty());
    assert!(decoded.report().losses.iter().any(|loss| {
        loss.code == F3dLossCode::MissingGeometryStream.kind()
            && loss.message == "no ASM BREP stream (.smb/.smbh) was found in the container"
    }));
}

#[test]
fn smbh_header_string_region_starts_at_byte_47() {
    // Regression: the three product strings begin at byte 47, not 48 — the
    // schema word `7` at offset 40 puts its low byte 0x07 at offset 47, which
    // doubles as the first string's TAG_UTF8_U8 tag. A parser that starts the
    // string walk at 48 reads a length byte as a tag and desyncs the whole
    // header, so record_stream_start lands mid-header and framing fails.
    let prefix = smbh_header_prefix();
    assert_eq!(prefix[47], 0x07, "first string tag at offset 47");
    // The header parses all three strings and both tolerances despite the
    // overlap, and the record stream begins immediately after the last double.
    let h = asm_header::parse(&prefix).expect("magic present");
    assert_eq!(h.product_family.as_deref(), Some("Autodesk Neutron"));
    assert_eq!(h.flags, Some(3));
    assert_eq!(h.angular, Some(1.0e-10));
    assert_eq!(
        asm_header::record_stream_start(&prefix),
        Some(prefix.len()),
        "record stream starts right after the header"
    );
}

#[test]
fn sab_framer_indexes_records_from_asmheader() {
    let bytes = synthetic_geometry_smbh();
    let start = asm_header::record_stream_start(&bytes).expect("record stream start");
    let limit = asm_header::solved_record_limit(&bytes).unwrap_or(bytes.len());
    let records = cadmpeg_asm::sab::frame(&bytes, start, limit, 8).expect("framing succeeds");

    // asmheader occupies index 0; the topology records follow in order.
    assert_eq!(records[0].index, 0);
    assert_eq!(records[0].head, "asmheader");
    assert_eq!(records[1].head, "body");
    assert_eq!(records[4].head, "face");
    assert_eq!(records[4].name, "face");
    assert_eq!(records[6].name, "plane-surface");
    // The face's surface reference (chunk[7]) resolves to the plane at index 6.
    assert_eq!(records[4].ref_at(7), Some(6));
    assert!(records.iter().all(|r| r.head != "delta_state"));
}
