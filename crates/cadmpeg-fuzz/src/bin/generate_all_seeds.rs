// SPDX-License-Identifier: Apache-2.0
//! Writes structural container and IR seeds, then derives deterministic
//! truncation, byte-flip, and oversized-length mutants.

use std::fs;
use std::io::{Cursor, Write};
use std::path::Path;

use cadmpeg_core::CodecError;
use cadmpeg_fuzz::seed_paths::seed_dir;
use cadmpeg_fuzz::seeds;
use flate2::write::DeflateEncoder;
use flate2::Compression;
use zip::write::SimpleFileOptions;
use zip::CompressionMethod;

fn main() -> Result<(), CodecError> {
    generate_f3d_seeds();
    generate_sldprt_seeds();
    generate_catia_seeds();
    generate_creo_seeds();
    generate_nx_seeds()?;
    generate_ir_seeds();
    generate_mutated_seeds();
    println!("All seeds generated.");
    Ok(())
}

// ============================================================================
// F3D seeds
// ============================================================================

fn generate_f3d_seeds() {
    let dir = seed_dir("seeds/f3d_container");
    fs::create_dir_all(&dir).unwrap();

    let seeds: Vec<(&str, Vec<u8>)> = vec![
        ("empty_zip", f3d::empty_zip()),
        ("bare_zip_with_txt", f3d::bare_zip_with_txt()),
        (
            "synthetic_smbh_header_only",
            f3d::f3d_with_smbh(&f3d::synthetic_smbh()),
        ),
        (
            "synthetic_geometry",
            f3d::f3d_with_smbh(&f3d::synthetic_geometry_smbh()),
        ),
        (
            "synthetic_mixed",
            f3d::f3d_with_smbh(&seeds::f3d::synthetic_mixed_smbh()),
        ),
        ("full_f3d_with_smbh", f3d::synthetic_f3d(true)),
        ("full_f3d_smb_only", f3d::synthetic_f3d(false)),
        ("corrupt_zip_magic", f3d::corrupt_zip_magic()),
        ("truncated_smbh", f3d::truncated_smbh()),
    ];

    for (name, data) in seeds {
        fs::write(dir.join(name), &data).unwrap();
        println!("  f3d/{} ({} bytes)", name, data.len());
    }
}

mod f3d {
    use super::*;
    use cadmpeg_fuzz::seeds::f3d::{
        push_tagged_f64, push_u8_string, smbh_header_prefix, t_end, t_ident, t_long, t_pos, t_ref,
        t_subident, t_vec,
    };

    pub fn empty_zip() -> Vec<u8> {
        zip::ZipWriter::new(Cursor::new(Vec::new()))
            .finish()
            .unwrap()
            .into_inner()
    }

    pub fn bare_zip_with_txt() -> Vec<u8> {
        let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
        let stored = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
        zip.start_file("readme.txt", stored).unwrap();
        zip.write_all(b"hello").unwrap();
        zip.finish().unwrap().into_inner()
    }

    pub fn corrupt_zip_magic() -> Vec<u8> {
        let mut data = empty_zip();
        data[0] = 0xFF;
        data[1] = 0xFF;
        data
    }

    pub fn truncated_smbh() -> Vec<u8> {
        let mut smbh = synthetic_smbh();
        smbh.truncate(60);
        f3d_with_smbh(&smbh)
    }

    pub fn synthetic_smbh() -> Vec<u8> {
        let mut b = smbh_header_prefix();
        b.extend_from_slice(&[0x0d, 0x04, b'b', b'o', b'd', b'y', 0x11]);
        let active_len = b.len();
        b.extend_from_slice(&[0x11, 0x0d, 0x0b]);
        b.extend_from_slice(b"delta_state");
        b.extend_from_slice(&[0u8; 16]);
        assert_eq!(&b[active_len + 3..active_len + 3 + 11], b"delta_state");
        b
    }

    pub fn synthetic_geometry_smbh() -> Vec<u8> {
        let mut r = Vec::new();
        t_ident(&mut r, "asmheader");
        push_u8_string(&mut r, "231.6.3.65535");
        t_end(&mut r);

        t_ident(&mut r, "body");
        t_ref(&mut r, -1);
        t_long(&mut r, -1);
        t_ref(&mut r, -1);
        t_ref(&mut r, 2);
        t_ref(&mut r, -1);
        t_ref(&mut r, -1);
        t_end(&mut r);

        t_ident(&mut r, "region");
        t_ref(&mut r, -1);
        t_long(&mut r, -1);
        t_ref(&mut r, -1);
        t_ref(&mut r, -1);
        t_ref(&mut r, 3);
        t_ref(&mut r, 1);
        t_end(&mut r);

        t_ident(&mut r, "shell");
        t_ref(&mut r, -1);
        t_long(&mut r, -1);
        t_ref(&mut r, -1);
        t_ref(&mut r, -1);
        t_ref(&mut r, -1);
        t_ref(&mut r, 4);
        t_ref(&mut r, -1);
        t_ref(&mut r, 2);
        t_end(&mut r);

        t_ident(&mut r, "face");
        t_ref(&mut r, -1);
        t_long(&mut r, -1);
        t_ref(&mut r, -1);
        t_ref(&mut r, -1);
        t_ref(&mut r, 5);
        t_ref(&mut r, 3);
        t_ref(&mut r, -1);
        t_ref(&mut r, 6);
        r.push(0x0b);
        r.push(0x0b);
        t_end(&mut r);

        t_ident(&mut r, "loop");
        t_ref(&mut r, -1);
        t_long(&mut r, -1);
        t_ref(&mut r, -1);
        t_ref(&mut r, -1);
        t_ref(&mut r, 7);
        t_ref(&mut r, 4);
        t_end(&mut r);

        t_subident(&mut r, "plane");
        t_ident(&mut r, "surface");
        t_ref(&mut r, -1);
        t_long(&mut r, -1);
        t_ref(&mut r, -1);
        t_pos(&mut r, [0.0, 0.0, 0.0]);
        t_vec(&mut r, [0.0, 0.0, 1.0]);
        t_pos(&mut r, [1.0, 0.0, 0.0]);
        r.push(0x0b);
        t_end(&mut r);

        let coedges = [(7i64, 8, 9, 10), (8, 9, 7, 11), (9, 7, 8, 12)];
        for (_id, next, prev, edge) in coedges {
            t_ident(&mut r, "coedge");
            t_ref(&mut r, -1);
            t_long(&mut r, -1);
            t_ref(&mut r, -1);
            t_ref(&mut r, next);
            t_ref(&mut r, prev);
            t_ref(&mut r, -1);
            t_ref(&mut r, edge);
            r.push(0x0b);
            t_ref(&mut r, 5);
            t_long(&mut r, 0);
            t_ref(&mut r, -1);
            t_end(&mut r);
        }

        let edges = [(10i64, 13, 14), (11, 14, 15), (12, 15, 13)];
        for (_id, start, end) in edges {
            t_ident(&mut r, "edge");
            t_ref(&mut r, -1);
            t_long(&mut r, -1);
            t_ref(&mut r, -1);
            t_ref(&mut r, start);
            push_tagged_f64(&mut r, 0.0);
            t_ref(&mut r, end);
            push_tagged_f64(&mut r, 1.0);
            t_ref(&mut r, -1);
            t_ref(&mut r, -1);
            r.push(0x0b);
            push_u8_string(&mut r, "unknown");
            t_end(&mut r);
        }

        let verts = [(13i64, 10, 16), (14, 11, 17), (15, 12, 18)];
        for (_id, edge, point) in verts {
            t_ident(&mut r, "vertex");
            t_ref(&mut r, -1);
            t_long(&mut r, -1);
            t_ref(&mut r, -1);
            t_ref(&mut r, edge);
            t_long(&mut r, 0);
            t_ref(&mut r, point);
            t_end(&mut r);
        }

        for p in [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]] {
            t_ident(&mut r, "point");
            t_ref(&mut r, -1);
            t_long(&mut r, -1);
            t_ref(&mut r, -1);
            t_pos(&mut r, p);
            t_long(&mut r, 1);
            t_end(&mut r);
        }

        t_ident(&mut r, "delta_state");
        let mut out = smbh_header_prefix();
        out.extend_from_slice(&r);
        out
    }

    pub fn f3d_with_smbh(smbh: &[u8]) -> Vec<u8> {
        let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
        let stored = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
        zip.start_file("Manifest.dat", stored).unwrap();
        zip.write_all(b"synthetic-manifest").unwrap();
        zip.start_file("FusionAssetName[Active]/Breps.BlobParts/Body1.smbh", stored)
            .unwrap();
        zip.write_all(smbh).unwrap();
        zip.finish().unwrap().into_inner()
    }

    pub fn synthetic_f3d(include_smbh: bool) -> Vec<u8> {
        let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
        let stored = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
        let deflated = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);

        let folder = "FusionAssetName[Active]";
        zip.start_file("Manifest.dat", stored).unwrap();
        zip.write_all(b"synthetic-manifest").unwrap();

        if include_smbh {
            zip.start_file(format!("{folder}/Breps.BlobParts/Body1.smbh"), deflated)
                .unwrap();
            zip.write_all(&synthetic_smbh()).unwrap();
        }

        let mut smb = synthetic_smbh();
        smb.truncate(60);
        zip.start_file(format!("{folder}/Breps.BlobParts/Body1.smb"), stored)
            .unwrap();
        zip.write_all(&smb).unwrap();

        zip.start_file(
            format!("{folder}/FusionDesignSegmentType1/BulkStream.dat"),
            stored,
        )
        .unwrap();
        zip.write_all(b"design-bulk").unwrap();

        zip.start_file(format!("{folder}/Previews/thumbnail.png"), stored)
            .unwrap();
        zip.write_all(b"\x89PNG").unwrap();

        zip.finish().unwrap().into_inner()
    }
}

// ============================================================================
// SLDPRT seeds
// ============================================================================

fn generate_sldprt_seeds() {
    let dir = seed_dir("seeds/sldprt_container");
    fs::create_dir_all(&dir).unwrap();

    let seeds: Vec<(&str, Vec<u8>)> = vec![
        ("empty", vec![]),
        ("just_header", seeds::sldprt::outer_header()),
        ("synthetic_sldprt", sldprt::synthetic_sldprt()),
        (
            "with_triangle_body",
            sldprt::sldprt_with_body(&seeds::sldprt::triangle_body()),
        ),
        (
            "with_cylinder",
            sldprt::sldprt_with_body(&seeds::sldprt::closed_cylinder_body()),
        ),
    ];

    for (name, data) in seeds {
        fs::write(dir.join(name), &data).unwrap();
        println!("  sldprt/{} ({} bytes)", name, data.len());
    }
}

mod sldprt {
    use super::*;
    use cadmpeg_fuzz::seeds::sldprt::{
        crc32, make_cache_cell, make_directory_entry, outer_header, parasolid_payload, swap_name,
        MARKER,
    };

    fn raw_deflate(data: &[u8]) -> Vec<u8> {
        let mut enc = DeflateEncoder::new(Vec::new(), Compression::default());
        enc.write_all(data).unwrap();
        enc.finish().unwrap()
    }

    fn make_block(type_id: u32, section: &str, payload: &[u8]) -> Vec<u8> {
        let comp = raw_deflate(payload);
        let preamble = swap_name(section);
        let mut b = Vec::new();
        b.extend_from_slice(&MARKER);
        b.extend_from_slice(&type_id.to_le_bytes());
        b.extend_from_slice(&crc32(payload).to_le_bytes());
        b.extend_from_slice(&(comp.len() as u32).to_le_bytes());
        b.extend_from_slice(&(payload.len() as u32).to_le_bytes());
        b.extend_from_slice(&(preamble.len() as u32).to_le_bytes());
        b.extend_from_slice(&preamble);
        b.extend_from_slice(&comp);
        b
    }

    pub fn synthetic_sldprt() -> Vec<u8> {
        let mut f = outer_header();
        f.extend_from_slice(&make_block(
            0x10,
            "PreviewPNG",
            &[0x89, b'P', b'N', b'G', 1, 2, 3, 4],
        ));
        f.extend_from_slice(&make_block(
            0x20,
            "Contents/Config-0-Partition",
            &parasolid_payload("partition body", "SCH_SW_33103_11000"),
        ));
        f.extend_from_slice(&make_cache_cell(90, "Contents/DisplayLists"));
        f.extend_from_slice(&make_directory_entry(0x30, 2, "[Content_Types].xml"));
        f
    }

    pub fn sldprt_with_body(body: &[u8]) -> Vec<u8> {
        let mut f = outer_header();
        f.extend_from_slice(&make_block(0x20, "Contents/Config-0-Partition", &{
            let mut p = parasolid_payload("partition body", "SCH_SW_33103_11000");
            p.extend_from_slice(body);
            p
        }));
        f
    }
}

// ============================================================================
// CATIA seeds
// ============================================================================

fn generate_catia_seeds() {
    let dir = seed_dir("seeds/catia_container");
    fs::create_dir_all(&dir).unwrap();

    let seeds: Vec<(&str, Vec<u8>)> = vec![
        ("empty", vec![]),
        ("just_magic", seeds::catia::outer_magic()),
        ("zero_entity", seeds::catia::zero_entity_catpart()),
        ("standard_nested", seeds::catia::standard_catpart()),
    ];

    for (name, data) in seeds {
        fs::write(dir.join(name), &data).unwrap();
        println!("  catia/{} ({} bytes)", name, data.len());
    }
}

// ============================================================================
// CREO seeds
// ============================================================================

fn generate_creo_seeds() {
    let dir = seed_dir("seeds/creo_container");
    fs::create_dir_all(&dir).unwrap();

    let seeds: Vec<(&str, Vec<u8>)> = vec![
        ("empty", vec![]),
        ("just_magic", seeds::creo::just_magic()),
        ("minimal_prt", seeds::creo::minimal_prt()),
        ("with_visibgeom", seeds::creo::with_visibgeom()),
    ];

    for (name, data) in seeds {
        fs::write(dir.join(name), &data).unwrap();
        println!("  creo/{} ({} bytes)", name, data.len());
    }
}

// ============================================================================
// NX seeds
// ============================================================================

fn generate_nx_seeds() -> Result<(), CodecError> {
    let dir = seed_dir("seeds/nx_container");
    fs::create_dir_all(&dir).unwrap();

    let seeds: Vec<(&str, Vec<u8>)> = vec![
        ("empty", vec![]),
        ("just_magic", seeds::nx::just_magic()),
        ("single_part", nx::single_part_prt()?),
        ("assembly", seeds::nx::assembly_prt()),
    ];

    for (name, data) in seeds {
        fs::write(dir.join(name), &data).unwrap();
        println!("  nx/{} ({} bytes)", name, data.len());
    }
    Ok(())
}

mod nx {
    use cadmpeg_core::CodecError;
    use cadmpeg_fuzz::seeds::nx::{partition_stream, MAGIC};
    use flate2::write::ZlibEncoder;
    use flate2::Compression;
    use std::io::Write;

    fn zlib_compress(raw: &[u8]) -> Vec<u8> {
        let mut e = ZlibEncoder::new(Vec::new(), Compression::new(1));
        e.write_all(raw).unwrap();
        e.finish().unwrap()
    }

    pub fn single_part_prt() -> Result<Vec<u8>, CodecError> {
        let mut f = Vec::new();
        f.extend_from_slice(MAGIC);
        f.push(0x06);
        f.extend_from_slice(&[0x11, 0x22, 0x33]);
        f.extend_from_slice(&[0, 0, 0, 0]);
        f.push(0x00);
        f.extend_from_slice(&[0, 0, 0, 0, 0, 0]);
        f.extend_from_slice(&[0, 0]);

        f.extend_from_slice(b"HEADER");
        let name = b"/Root/UG_PART/UG_PART";
        f.extend_from_slice(&(name.len() as u32).to_le_bytes());
        f.extend_from_slice(name);

        let blob = zlib_compress(&partition_stream()?);
        let dir_end = f.len() + 16;
        let blob_off = dir_end as u64;
        f.extend_from_slice(&blob_off.to_le_bytes());
        f.extend_from_slice(&(blob.len() as u64).to_le_bytes());
        f.extend_from_slice(&blob);
        Ok(f)
    }
}

// ============================================================================
// IR seeds
// ============================================================================

fn generate_ir_seeds() {
    let minimal = cadmpeg_ir::CadIr::empty().to_canonical_json().unwrap();
    let cube = cadmpeg_ir::examples::unit_cube()
        .expect("unit cube fixture is admitted")
        .to_canonical_json()
        .unwrap();
    let directed_subd_sum = cadmpeg_ir::examples::directed_subd_sum()
        .unwrap()
        .to_canonical_json()
        .unwrap();
    let canonical = [
        ("minimal.json", minimal.as_bytes()),
        ("unit_cube.json", cube.as_bytes()),
        ("directed_subd_sum.json", directed_subd_sum.as_bytes()),
    ];
    let current_version_field = format!(r#""ir_version": "{}""#, cadmpeg_ir::IR_VERSION);
    let valid_v0 = minimal.replacen(&current_version_field, r#""ir_version": "0""#, 1);
    assert_ne!(valid_v0, minimal, "current ir_version field must match");

    let from_json = seed_dir("seeds/ir_from_json");
    replace_seed_directory(&from_json);
    for (name, data) in &canonical {
        fs::write(from_json.join(name), data).unwrap();
        println!("  ir/{name} ({} bytes)", data.len());
    }
    fs::write(from_json.join("valid_v0_rejected.json"), valid_v0).unwrap();

    for target in ["ir_validate", "ir_canonical_roundtrip", "step_writer"] {
        let dir = seed_dir(target);
        replace_seed_directory(&dir);
        for (name, data) in &canonical {
            fs::write(dir.join(name), data).unwrap();
        }
    }

    let mutated = seed_dir("seeds/ir_validate_mutated");
    replace_seed_directory(&mutated);
    for (index, (name, data)) in canonical.iter().enumerate() {
        let mut input = vec![index as u8];
        input.extend_from_slice(data);
        fs::write(mutated.join(name), input).unwrap();
    }

    let custom = seed_dir("seeds/step_writer_custom");
    replace_seed_directory(&custom);
    for (index, (name, data)) in canonical.iter().enumerate() {
        let mut input = vec![index as u8; 8];
        input.extend_from_slice(data);
        fs::write(custom.join(name), input).unwrap();
    }

    let iges_writer = seed_dir("seeds/iges_writer");
    replace_seed_directory(&iges_writer);
    for (name, control, data) in [
        ("minimal_v5_1.json", 0_u8, minimal.as_bytes()),
        ("unit_cube_v5_2.json", 1_u8, cube.as_bytes()),
        (
            "directed_subd_v5_3.json",
            2_u8,
            directed_subd_sum.as_bytes(),
        ),
        ("unsupported_native.json", 0x80_u8, minimal.as_bytes()),
    ] {
        let mut input = vec![control];
        input.extend_from_slice(data);
        fs::write(iges_writer.join(name), input).unwrap();
    }

    let diff = seed_dir("seeds/ir_diff");
    replace_seed_directory(&diff);
    for (name, selector, left, right) in [
        (
            "minimal_vs_minimal",
            0_u8,
            minimal.as_bytes(),
            minimal.as_bytes(),
        ),
        ("minimal_vs_cube", 1_u8, minimal.as_bytes(), cube.as_bytes()),
        ("cube_vs_minimal", 2_u8, cube.as_bytes(), minimal.as_bytes()),
    ] {
        let mut input = vec![selector];
        input.extend_from_slice(left);
        input.push(0);
        input.extend_from_slice(right);
        fs::write(diff.join(name), input).unwrap();
    }
}

fn replace_seed_directory(directory: &Path) {
    fs::create_dir_all(directory).unwrap();
    for entry in fs::read_dir(directory).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            fs::remove_dir_all(path).unwrap();
        } else {
            fs::remove_file(path).unwrap();
        }
    }
}

// ============================================================================
// Mutated seeds: deterministic corruptions of every structurally valid seed
// ============================================================================

const MUTANT_SUFFIXES: [&str; 3] = [".mut_trunc", ".mut_flip", ".mut_lenmax"];

/// For each seed emitted above, write three deterministic corruptions:
/// - `.mut_trunc`: cut at 50% length (mid-record truncation)
/// - `.mut_flip`: invert the byte at 50% offset (payload corruption past the header)
/// - `.mut_lenmax`: saturate 4 bytes at 25% offset to 0xFF (oversized count/length fields)
///
/// Mutants are derived only from files this run just wrote, never from other
/// mutants, so regeneration is idempotent.
fn generate_mutated_seeds() {
    let container_dirs = [
        "seeds/f3d_container",
        "seeds/sldprt_container",
        "seeds/catia_container",
        "seeds/creo_container",
        "seeds/nx_container",
    ];
    for dir in container_dirs {
        for entry in fs::read_dir(seed_dir(dir)).unwrap() {
            let path = entry.unwrap().path();
            let name = path.file_name().unwrap().to_str().unwrap();
            if MUTANT_SUFFIXES.iter().any(|suffix| name.ends_with(suffix)) {
                fs::remove_file(path).unwrap();
            }
        }
    }

    for dir in ["seeds/ir_from_json"] {
        let dir = seed_dir(dir);
        for entry in fs::read_dir(&dir).unwrap() {
            let path = entry.unwrap().path();
            let name = path.file_name().unwrap().to_str().unwrap();
            if MUTANT_SUFFIXES.iter().any(|suffix| name.ends_with(suffix)) {
                fs::remove_file(path).unwrap();
            }
        }
        let mut entries: Vec<_> = fs::read_dir(&dir)
            .unwrap()
            .map(|e| e.unwrap().path())
            .filter(|p| {
                let name = p.file_name().unwrap().to_str().unwrap();
                p.is_file() && !MUTANT_SUFFIXES.iter().any(|s| name.ends_with(s))
            })
            .collect();
        entries.sort();
        for path in entries {
            let data = fs::read(&path).unwrap();
            // Too small to have structure past the magic; corruptions would
            // duplicate the existing bad-magic/truncation seeds.
            if data.len() < 32 {
                continue;
            }
            let name = path.file_name().unwrap().to_str().unwrap().to_string();

            let mut trunc = data.clone();
            trunc.truncate(data.len() / 2);

            let mut flip = data.clone();
            let mid = data.len() / 2;
            flip[mid] = !flip[mid];

            let mut lenmax = data.clone();
            let off = data.len() / 4;
            for b in &mut lenmax[off..(off + 4).min(data.len())] {
                *b = 0xFF;
            }

            for (suffix, mutant) in MUTANT_SUFFIXES.iter().zip([trunc, flip, lenmax]) {
                let out = path.with_file_name(format!("{name}{suffix}"));
                fs::write(&out, &mutant).unwrap();
                println!("  {} ({} bytes)", out.display(), mutant.len());
            }
        }
    }
}
