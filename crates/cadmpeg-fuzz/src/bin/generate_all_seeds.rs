// SPDX-License-Identifier: Apache-2.0
//! Writes structural container and IR seeds, then derives deterministic
//! truncation, byte-flip, and oversized-length mutants.

use std::fs;
use std::path::Path;

use cadmpeg_fuzz::seed_paths::seed_dir;
use cadmpeg_fuzz::seeds;

type SeedError = Box<dyn std::error::Error>;

fn main() -> Result<(), SeedError> {
    generate_f3d_seeds()?;
    generate_sldprt_seeds()?;
    generate_catia_seeds()?;
    generate_creo_seeds()?;
    generate_nx_seeds()?;
    generate_ir_seeds()?;
    generate_mutated_seeds()?;
    println!("All seeds generated.");
    Ok(())
}

// ============================================================================
// F3D seeds
// ============================================================================

fn generate_f3d_seeds() -> Result<(), SeedError> {
    let dir = seed_dir("seeds/f3d_container");
    fs::create_dir_all(&dir)?;

    let seeds: Vec<(&str, Vec<u8>)> = vec![
        ("empty_zip", seeds::f3d::empty_zip()?),
        ("bare_zip_with_txt", seeds::f3d::bare_zip_with_txt()?),
        (
            "synthetic_smbh_header_only",
            seeds::f3d::f3d_with_smbh(&seeds::f3d::synthetic_smbh()?)?,
        ),
        (
            "synthetic_geometry",
            seeds::f3d::f3d_with_smbh(&f3d::synthetic_geometry_smbh()?)?,
        ),
        (
            "synthetic_mixed",
            seeds::f3d::f3d_with_smbh(&seeds::f3d::synthetic_mixed_smbh()?)?,
        ),
        ("full_f3d_with_smbh", seeds::f3d::synthetic_f3d(true)?),
        ("full_f3d_smb_only", seeds::f3d::synthetic_f3d(false)?),
        ("corrupt_zip_magic", seeds::f3d::corrupt_zip_magic()?),
        ("truncated_smbh", seeds::f3d::truncated_smbh()?),
    ];

    for (name, data) in seeds {
        fs::write(dir.join(name), &data)?;
        println!("  f3d/{} ({} bytes)", name, data.len());
    }
    Ok(())
}

mod f3d {
    use cadmpeg_fuzz::seeds::f3d::{
        push_tagged_f64, push_u8_string, smbh_header_prefix, t_end, t_ident, t_long, t_pos, t_ref,
        t_subident, t_vec,
    };

    pub(super) fn synthetic_geometry_smbh() -> std::io::Result<Vec<u8>> {
        let mut r = Vec::new();
        t_ident(&mut r, "asmheader")?;
        push_u8_string(&mut r, "231.6.3.65535")?;
        t_end(&mut r);

        t_ident(&mut r, "body")?;
        t_ref(&mut r, -1);
        t_long(&mut r, -1);
        t_ref(&mut r, -1);
        t_ref(&mut r, 2);
        t_ref(&mut r, -1);
        t_ref(&mut r, -1);
        t_end(&mut r);

        t_ident(&mut r, "region")?;
        t_ref(&mut r, -1);
        t_long(&mut r, -1);
        t_ref(&mut r, -1);
        t_ref(&mut r, -1);
        t_ref(&mut r, 3);
        t_ref(&mut r, 1);
        t_end(&mut r);

        t_ident(&mut r, "shell")?;
        t_ref(&mut r, -1);
        t_long(&mut r, -1);
        t_ref(&mut r, -1);
        t_ref(&mut r, -1);
        t_ref(&mut r, -1);
        t_ref(&mut r, 4);
        t_ref(&mut r, -1);
        t_ref(&mut r, 2);
        t_end(&mut r);

        t_ident(&mut r, "face")?;
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

        t_ident(&mut r, "loop")?;
        t_ref(&mut r, -1);
        t_long(&mut r, -1);
        t_ref(&mut r, -1);
        t_ref(&mut r, -1);
        t_ref(&mut r, 7);
        t_ref(&mut r, 4);
        t_end(&mut r);

        t_subident(&mut r, "plane")?;
        t_ident(&mut r, "surface")?;
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
            t_ident(&mut r, "coedge")?;
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
            t_ident(&mut r, "edge")?;
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
            push_u8_string(&mut r, "unknown")?;
            t_end(&mut r);
        }

        let verts = [(13i64, 10, 16), (14, 11, 17), (15, 12, 18)];
        for (_id, edge, point) in verts {
            t_ident(&mut r, "vertex")?;
            t_ref(&mut r, -1);
            t_long(&mut r, -1);
            t_ref(&mut r, -1);
            t_ref(&mut r, edge);
            t_long(&mut r, 0);
            t_ref(&mut r, point);
            t_end(&mut r);
        }

        for p in [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]] {
            t_ident(&mut r, "point")?;
            t_ref(&mut r, -1);
            t_long(&mut r, -1);
            t_ref(&mut r, -1);
            t_pos(&mut r, p);
            t_long(&mut r, 1);
            t_end(&mut r);
        }

        t_ident(&mut r, "delta_state")?;
        let mut out = smbh_header_prefix()?;
        out.extend_from_slice(&r);
        Ok(out)
    }
}

// ============================================================================
// SLDPRT seeds
// ============================================================================

fn generate_sldprt_seeds() -> Result<(), SeedError> {
    let dir = seed_dir("seeds/sldprt_container");
    fs::create_dir_all(&dir)?;

    let seeds: Vec<(&str, Vec<u8>)> = vec![
        ("empty", vec![]),
        ("just_header", seeds::sldprt::outer_header()),
        ("synthetic_sldprt", seeds::sldprt::synthetic_sldprt()?),
        (
            "with_triangle_body",
            seeds::sldprt::sldprt_with_body(&seeds::sldprt::triangle_body())?,
        ),
        (
            "with_cylinder",
            seeds::sldprt::sldprt_with_body(&seeds::sldprt::closed_cylinder_body())?,
        ),
    ];

    for (name, data) in seeds {
        fs::write(dir.join(name), &data)?;
        println!("  sldprt/{} ({} bytes)", name, data.len());
    }
    Ok(())
}

// ============================================================================
// CATIA seeds
// ============================================================================

fn generate_catia_seeds() -> Result<(), SeedError> {
    let dir = seed_dir("seeds/catia_container");
    fs::create_dir_all(&dir)?;

    let seeds: Vec<(&str, Vec<u8>)> = vec![
        ("empty", vec![]),
        ("just_magic", seeds::catia::outer_magic()),
        ("zero_entity", seeds::catia::zero_entity_catpart()),
        ("standard_nested", seeds::catia::standard_catpart()?),
    ];

    for (name, data) in seeds {
        fs::write(dir.join(name), &data)?;
        println!("  catia/{} ({} bytes)", name, data.len());
    }
    Ok(())
}

// ============================================================================
// CREO seeds
// ============================================================================

fn generate_creo_seeds() -> Result<(), SeedError> {
    let dir = seed_dir("seeds/creo_container");
    fs::create_dir_all(&dir)?;

    let seeds: Vec<(&str, Vec<u8>)> = vec![
        ("empty", vec![]),
        ("just_magic", seeds::creo::just_magic()),
        ("minimal_prt", seeds::creo::minimal_prt()),
        ("with_visibgeom", seeds::creo::with_visibgeom()),
    ];

    for (name, data) in seeds {
        fs::write(dir.join(name), &data)?;
        println!("  creo/{} ({} bytes)", name, data.len());
    }
    Ok(())
}

// ============================================================================
// NX seeds
// ============================================================================

fn generate_nx_seeds() -> Result<(), SeedError> {
    let dir = seed_dir("seeds/nx_container");
    fs::create_dir_all(&dir)?;

    let seeds: Vec<(&str, Vec<u8>)> = vec![
        ("empty", vec![]),
        ("just_magic", seeds::nx::just_magic()),
        ("single_part", seeds::nx::single_part_prt()?),
        ("assembly", seeds::nx::assembly_prt()?),
    ];

    for (name, data) in seeds {
        fs::write(dir.join(name), &data)?;
        println!("  nx/{} ({} bytes)", name, data.len());
    }
    Ok(())
}

// ============================================================================
// IR seeds
// ============================================================================

fn generate_ir_seeds() -> Result<(), SeedError> {
    let minimal = cadmpeg_ir::CadIr::empty().to_canonical_json()?;
    let cube = cadmpeg_ir::examples::unit_cube()?.to_canonical_json()?;
    let directed_subd_sum = cadmpeg_ir::examples::directed_subd_sum()?.to_canonical_json()?;
    let canonical = [
        ("minimal.json", minimal.as_bytes()),
        ("unit_cube.json", cube.as_bytes()),
        ("directed_subd_sum.json", directed_subd_sum.as_bytes()),
    ];
    let current_version_field = format!(r#""ir_version": "{}""#, cadmpeg_ir::IR_VERSION);
    let valid_v0 = minimal.replacen(&current_version_field, r#""ir_version": "0""#, 1);
    if valid_v0 == minimal {
        return Err(
            format!("no {current_version_field} field to rewrite in the minimal seed").into(),
        );
    }

    let from_json = seed_dir("seeds/ir_from_json");
    replace_seed_directory(&from_json)?;
    for (name, data) in &canonical {
        fs::write(from_json.join(name), data)?;
        println!("  ir/{name} ({} bytes)", data.len());
    }
    fs::write(from_json.join("valid_v0_rejected.json"), valid_v0)?;

    for target in ["ir_validate", "ir_canonical_roundtrip", "step_writer"] {
        let dir = seed_dir(target);
        replace_seed_directory(&dir)?;
        for (name, data) in &canonical {
            fs::write(dir.join(name), data)?;
        }
    }

    let mutated = seed_dir("seeds/ir_validate_mutated");
    replace_seed_directory(&mutated)?;
    for (index, (name, data)) in canonical.iter().enumerate() {
        let mut input = vec![index as u8];
        input.extend_from_slice(data);
        fs::write(mutated.join(name), input)?;
    }

    let custom = seed_dir("seeds/step_writer_custom");
    replace_seed_directory(&custom)?;
    for (index, (name, data)) in canonical.iter().enumerate() {
        let mut input = vec![index as u8; 8];
        input.extend_from_slice(data);
        fs::write(custom.join(name), input)?;
    }

    let iges_writer = seed_dir("seeds/iges_writer");
    replace_seed_directory(&iges_writer)?;
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
        fs::write(iges_writer.join(name), input)?;
    }

    let diff = seed_dir("seeds/ir_diff");
    replace_seed_directory(&diff)?;
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
        fs::write(diff.join(name), input)?;
    }
    Ok(())
}

fn replace_seed_directory(directory: &Path) -> Result<(), SeedError> {
    fs::create_dir_all(directory)?;
    for entry in fs::read_dir(directory)? {
        let path = entry?.path();
        if path.is_dir() {
            fs::remove_dir_all(path)?;
        } else {
            fs::remove_file(path)?;
        }
    }
    Ok(())
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
fn generate_mutated_seeds() -> Result<(), SeedError> {
    let container_dirs = [
        "seeds/f3d_container",
        "seeds/sldprt_container",
        "seeds/catia_container",
        "seeds/creo_container",
        "seeds/nx_container",
    ];
    for dir in container_dirs {
        remove_mutants(&seed_dir(dir))?;
    }

    for dir in ["seeds/ir_from_json"] {
        let dir = seed_dir(dir);
        remove_mutants(&dir)?;
        let mut entries = Vec::new();
        for entry in fs::read_dir(&dir)? {
            let path = entry?.path();
            if path.is_file() && !is_mutant(&path)? {
                entries.push(path);
            }
        }
        entries.sort();
        for path in entries {
            let data = fs::read(&path)?;
            // Too small to have structure past the magic; corruptions would
            // duplicate the existing bad-magic/truncation seeds.
            if data.len() < 32 {
                continue;
            }
            let name = seed_name(&path)?.to_string();

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
                fs::write(&out, &mutant)?;
                println!("  {} ({} bytes)", out.display(), mutant.len());
            }
        }
    }
    Ok(())
}

/// The UTF-8 file name of a seed path.
fn seed_name(path: &Path) -> Result<&str, SeedError> {
    path.file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| format!("seed path has no UTF-8 file name: {}", path.display()).into())
}

/// Whether a seed path carries one of the generated mutant suffixes.
fn is_mutant(path: &Path) -> Result<bool, SeedError> {
    let name = seed_name(path)?;
    Ok(MUTANT_SUFFIXES.iter().any(|suffix| name.ends_with(suffix)))
}

/// Delete every generated mutant in one seed directory.
fn remove_mutants(directory: &Path) -> Result<(), SeedError> {
    for entry in fs::read_dir(directory)? {
        let path = entry?.path();
        if is_mutant(&path)? {
            fs::remove_file(path)?;
        }
    }
    Ok(())
}
