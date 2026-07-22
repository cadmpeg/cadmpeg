// SPDX-License-Identifier: Apache-2.0
//! Writes minimal structural inputs for focused parser fuzz targets.

use std::fs;
use std::path::Path;

fn main() {
    generate_f3d_submodule_seeds();
    generate_sldprt_submodule_seeds();
    generate_catia_submodule_seeds();
    generate_creo_submodule_seeds();
    generate_nx_submodule_seeds();
    println!("All sub-module seeds generated.");
}

fn write_seed(dir: &str, name: &str, data: &[u8]) {
    let path = Path::new(dir);
    fs::create_dir_all(path).expect("required invariant");
    fs::write(path.join(name), data).expect("required invariant");
    println!("  {}/{} ({} bytes)", dir, name, data.len());
}

// ============================================================================
// F3D sub-module seeds
// ============================================================================

fn generate_f3d_submodule_seeds() {
    // ASM header seed
    let mut asm_header = Vec::new();
    asm_header.extend_from_slice(b"ASM BinaryFile");
    asm_header.extend_from_slice(&[0u8; 16]);
    write_seed("seeds/f3d_asm_header", "minimal", &asm_header);

    // SAB frame seed (minimal record stream)
    let sab_frame = vec![
        0x04, 0x00, 0x00, 0x00, // record length
        0x01, 0x00, 0x00, 0x00, // record type
        0x00, 0x00, 0x00, 0x00, // payload
    ];
    write_seed("seeds/f3d_sab_frame", "minimal", &sab_frame);

    // NURBS surface cache seed
    let nurbs_surface = vec![
        0x01, 0x00, 0x00, 0x00, // count
        0x02, 0x00, 0x00, 0x00, // surface type
        0x00, 0x00, 0x00, 0x00, // degree u
        0x00, 0x00, 0x00, 0x00, // degree v
    ];
    write_seed("seeds/f3d_nurbs_surfaces", "minimal", &nurbs_surface);

    // NURBS curve cache seed
    let nurbs_curve = vec![
        0x01, 0x00, 0x00, 0x00, // count
        0x01, 0x00, 0x00, 0x00, // curve type
        0x03, 0x00, 0x00, 0x00, // degree
    ];
    write_seed("seeds/f3d_nurbs_curves", "minimal", &nurbs_curve);

    // NURBS pcurve cache seed
    let nurbs_pcurve = vec![
        0x01, 0x00, 0x00, 0x00, // count
        0x00, 0x00, 0x00, 0x00, // surface ref
        0x00, 0x00, 0x00, 0x00, // curve ref
    ];
    write_seed("seeds/f3d_nurbs_pcurves", "minimal", &nurbs_pcurve);
}

// ============================================================================
// SolidWorks sub-module seeds
// ============================================================================

fn generate_sldprt_submodule_seeds() {
    // Parasolid stream seed (minimal valid stream)
    let parasolid = vec![
        0x00, 0x00, 0x00, 0x00, // padding
        b'P', b'a', b'r', b'a', b's', b'o', b'l', b'i', b'd', // magic
        0x00, 0x00, 0x00, 0x00, // version
    ];
    write_seed("seeds/sldprt_parasolid", "minimal", &parasolid);

    // Topology scan seed (minimal body with magic)
    let topology = vec![
        0x00, 0x00, 0x00, 0x00, // padding
        0x01, 0x00, 0x00, 0x00, // record count
        0x00, 0x00, 0x00, 0x00, // record type
    ];
    write_seed("seeds/sldprt_topology", "minimal", &topology);

    // Entity scan seed
    let entity = vec![
        0x00, 0x00, 0x00, 0x00, // padding
        0x01, 0x00, 0x00, 0x00, // entity count
        0x00, 0x00, 0x00, 0x00, // entity type
    ];
    write_seed("seeds/sldprt_entity", "minimal", &entity);

    // Spline curve carriers seed
    let spline_curves = vec![
        0x01, 0x00, 0x00, 0x00, // count
        0x00, 0x00, 0x00, 0x00, // carrier type
    ];
    write_seed("seeds/sldprt_spline_curves", "minimal", &spline_curves);

    // Spline surface carriers seed
    let spline_surfaces = vec![
        0x01, 0x00, 0x00, 0x00, // count
        0x00, 0x00, 0x00, 0x00, // carrier type
    ];
    write_seed("seeds/sldprt_spline_surfaces", "minimal", &spline_surfaces);

    // Container scan seed (reuse from main generator)
    let container = vec![
        0x9e, 0x14, 0x01, 0x00, // marker
        0x01, 0x00, 0x00, 0x00, // type
        0x00, 0x00, 0x00, 0x00, // crc
        0x00, 0x00, 0x00, 0x00, // comp len
        0x00, 0x00, 0x00, 0x00, // raw len
        0x00, 0x00, 0x00, 0x00, // name len
    ];
    write_seed("seeds/sldprt_container_scan", "minimal", &container);
}

// ============================================================================
// CATIA sub-module seeds
// ============================================================================

fn generate_catia_submodule_seeds() {
    // Geometry vertices seed
    let vertices = vec![
        0x01, 0x00, 0x00, 0x00, // vertex count
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, // x
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, // y
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, // z
    ];
    write_seed("seeds/catia_geometry_vertices", "minimal", &vertices);

    // Geometry surfaces seed
    let surfaces = vec![
        0x01, 0x00, 0x00, 0x00, // surface count
        0x00, 0x00, 0x00, 0x00, // surface type
    ];
    write_seed("seeds/catia_geometry_surfaces", "minimal", &surfaces);

    // A8 surfaces seed
    let a8_surfaces = vec![
        0x01, 0x00, 0x00, 0x00, // count
        0x02, 0x00, 0x00, 0x00, // type
        0x03, 0x00, 0x00, 0x00, // degree
    ];
    write_seed("seeds/catia_a8_surfaces", "minimal", &a8_surfaces);

    // A5 surfaces seed
    let a5_surfaces = vec![
        0x01, 0x00, 0x00, 0x00, // count
        0x05, 0x00, 0x00, 0x00, // type
    ];
    write_seed("seeds/catia_a5_surfaces", "minimal", &a5_surfaces);

    // B5 topology seed
    let b5 = vec![
        0x00, 0x00, 0x00, 0x00, // padding
        0x01, 0x00, 0x00, 0x00, // record count
    ];
    write_seed("seeds/catia_b5", "minimal", &b5);

    // E5 topology seed
    let e5 = vec![
        0x00, 0x00, 0x00, 0x00, // padding
        0x01, 0x00, 0x00, 0x00, // record count
    ];
    write_seed("seeds/catia_e5", "minimal", &e5);

    // Zero entity seed
    let zero_entity = vec![
        0x00, 0x00, 0x00, 0x00, // padding
        0x00, 0x00, 0x00, 0x00, // entity count
    ];
    write_seed("seeds/catia_zero_entity", "minimal", &zero_entity);

    // Container directory seed
    let container_dir = vec![
        0x00, 0x00, 0x00, 0x00, // padding
        0x01, 0x00, 0x00, 0x00, // directory count
    ];
    write_seed("seeds/catia_container_dir", "minimal", &container_dir);
}

// ============================================================================
// Creo sub-module seeds
// ============================================================================

fn generate_creo_submodule_seeds() {
    // PSB tokens seed
    let psb_tokens = vec![
        0x01, 0x00, 0x00, 0x00, // token count
        0x00, 0x00, 0x00, 0x00, // token type
    ];
    write_seed("seeds/creo_psb_tokens", "minimal", &psb_tokens);

    // Compact int seed
    let compact_int = vec![
        0x05, // value (encoded as (value * 4) + 1)
    ];
    write_seed("seeds/creo_compact_int", "minimal", &compact_int);

    // Short form float seed
    let short_float = vec![
        0x00, 0x00, 0x00, // 3-byte float
    ];
    write_seed("seeds/creo_short_form_float", "minimal", &short_float);

    // Container scan seed
    let container = vec![
        0x00, 0x00, 0x00, 0x00, // padding
        0x01, 0x00, 0x00, 0x00, // block count
    ];
    write_seed("seeds/creo_container_scan", "minimal", &container);

    // Surface rows seed
    let surface_rows = vec![
        0x01, 0x00, 0x00, 0x00, // row count
        0x00, 0x00, 0x00, 0x00, // row type
    ];
    write_seed("seeds/creo_surface_rows", "minimal", &surface_rows);

    // Curve prototypes seed
    let curve_protos = vec![
        0x01, 0x00, 0x00, 0x00, // prototype count
        0x00, 0x00, 0x00, 0x00, // prototype type
    ];
    write_seed("seeds/creo_curve_prototypes", "minimal", &curve_protos);
}

// ============================================================================
// NX sub-module seeds
// ============================================================================

fn generate_nx_submodule_seeds() {
    // Parasolid stream seed (with zlib header)
    let parasolid = vec![
        0x78, 0x9c, // zlib header
        0x00, 0x00, 0x00, 0x00, // compressed data
        0x00, 0x00, 0x00, 0x00, // checksum
    ];
    write_seed("seeds/nx_parasolid", "minimal", &parasolid);

    // Geometry points seed
    let points = vec![
        0x01, 0x00, 0x00, 0x00, // point count
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, // x
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, // y
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, // z
    ];
    write_seed("seeds/nx_geometry_points", "minimal", &points);

    // Geometry surfaces seed
    let surfaces = vec![
        0x01, 0x00, 0x00, 0x00, // surface count
        0x00, 0x00, 0x00, 0x00, // surface type
    ];
    write_seed("seeds/nx_geometry_surfaces", "minimal", &surfaces);

    // Geometry curves seed
    let curves = vec![
        0x01, 0x00, 0x00, 0x00, // curve count
        0x00, 0x00, 0x00, 0x00, // curve type
    ];
    write_seed("seeds/nx_geometry_curves", "minimal", &curves);

    // NURBS surfaces seed
    let nurbs_surfaces = vec![
        0x01, 0x00, 0x00, 0x00, // count
        0x02, 0x00, 0x00, 0x00, // type
        0x03, 0x00, 0x00, 0x00, // degree
    ];
    write_seed("seeds/nx_nurbs_surfaces", "minimal", &nurbs_surfaces);

    // NURBS curves seed
    let nurbs_curves = vec![
        0x01, 0x00, 0x00, 0x00, // count
        0x01, 0x00, 0x00, 0x00, // type
        0x03, 0x00, 0x00, 0x00, // degree
    ];
    write_seed("seeds/nx_nurbs_curves", "minimal", &nurbs_curves);
}
