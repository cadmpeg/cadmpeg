// SPDX-License-Identifier: Apache-2.0
//! Writes minimal structural inputs for focused parser fuzz targets.

use std::fs;
use std::io::Write as _;

include!("../seed_paths.rs");

use cadmpeg_core::decode::alloc_filled;
use cadmpeg_core::CodecError;

fn main() -> Result<(), CodecError> {
    generate_acis_header_seed();
    generate_f3d_submodule_seeds();
    generate_sldprt_submodule_seeds();
    generate_catia_submodule_seeds();
    generate_creo_submodule_seeds();
    generate_nx_submodule_seeds();
    generate_inventor_submodule_seeds()?;
    generate_rhino_submodule_seeds();
    println!("All sub-module seeds generated.");
    Ok(())
}

fn generate_acis_header_seed() {
    let mut header = b"ACIS BinaryFile".to_vec();
    for value in [21_800_u32, 0, 0, 0] {
        header.extend_from_slice(&value.to_le_bytes());
    }
    for value in ["Synthetic", "ACIS 218", "2000-01-01"] {
        header.push(0x07);
        header.push(u8::try_from(value.len()).expect("short synthetic string"));
        header.extend_from_slice(value.as_bytes());
    }
    for value in [1.0_f64, 1.0e-6, 1.0e-10] {
        header.push(0x06);
        header.extend_from_slice(&value.to_le_bytes());
    }
    write_seed("seeds/acis_header", "minimal", &header);
}

fn write_seed(dir: &str, name: &str, data: &[u8]) {
    let path = seed_dir(dir);
    fs::create_dir_all(&path).expect("required invariant");
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

    // PMISemanticDataDB MessagePack seeds for sldprt_pmi.
    write_seed(
        "seeds/sldprt_pmi",
        "minimal",
        &sldprt_pmi_seed(&[("Linear", 0.025)], false, false),
    );
    write_seed(
        "seeds/sldprt_pmi",
        "array16",
        &sldprt_pmi_seed(&[("Linear", 0.025); 16], false, false),
    );
    write_seed(
        "seeds/sldprt_pmi",
        "reordered",
        &sldprt_pmi_seed(&[("Linear", 0.025)], true, false),
    );
    write_seed(
        "seeds/sldprt_pmi",
        "malformed",
        &sldprt_pmi_seed(&[("Linear", 0.025)], false, true),
    );
}

fn sldprt_pmi_seed(items: &[(&str, f64)], reorder: bool, truncate: bool) -> Vec<u8> {
    fn fixstr(bytes: &mut Vec<u8>, value: &str) {
        bytes.push(0xa0 | value.len() as u8);
        bytes.extend_from_slice(value.as_bytes());
    }
    let mut payload = b"unqlite".to_vec();
    payload.extend_from_slice(&[0; 57]);
    payload.extend_from_slice(b"01234567-89ab-cdef-0123-456789abcdef");
    let outer = if reorder { 8 } else { 7 };
    payload.push(0x80 | outer);
    if reorder {
        fixstr(&mut payload, "cadText");
        fixstr(&mut payload, "D1@Sketch1");
        fixstr(&mut payload, "extraKey");
        fixstr(&mut payload, "ignored");
        fixstr(&mut payload, "annoType");
        payload.push(1);
    } else {
        fixstr(&mut payload, "annoType");
        payload.push(1);
        fixstr(&mut payload, "cadText");
        fixstr(&mut payload, "D1@Sketch1");
    }
    fixstr(&mut payload, "dimItems");
    if truncate {
        payload.push(0x91);
        return payload;
    }
    if items.len() < 16 {
        payload.push(0x90 | items.len() as u8);
    } else {
        payload.push(0xdc);
        payload.extend_from_slice(&(items.len() as u16).to_be_bytes());
    }
    for (subtype, value) in items {
        payload.push(0x87);
        fixstr(&mut payload, "class");
        fixstr(&mut payload, "DimSemData");
        fixstr(&mut payload, "dimSubType");
        fixstr(&mut payload, subtype);
        fixstr(&mut payload, "isBasic");
        payload.push(0xc3);
        fixstr(&mut payload, "isInspection");
        payload.push(0xc2);
        fixstr(&mut payload, "isReferenceOnly");
        payload.push(0xc3);
        fixstr(&mut payload, "valPrecision");
        payload.push(3);
        fixstr(&mut payload, "value");
        payload.push(0xcb);
        payload.extend_from_slice(&value.to_be_bytes());
    }
    fixstr(&mut payload, "dimText");
    fixstr(&mut payload, "25.000 mm");
    fixstr(&mut payload, "dimType");
    payload.push(0);
    fixstr(&mut payload, "iDString");
    fixstr(&mut payload, "native-id");
    fixstr(&mut payload, "reserved");
    payload.push(0xc0);
    payload
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

    let catalog_entries = ["CATCatalogManager", "catalogManager", "catalogLinks", ""];
    let mut catalog = vec![0x7c, 0x02, 0, 0, 0, 0];
    catalog.push(0x80 + u8::try_from(catalog_entries.len() + 1).expect("prefix count"));
    for entry in catalog_entries {
        catalog.push(u8::try_from(entry.len() + 1).expect("short catalog entry"));
        catalog.extend_from_slice(entry.as_bytes());
    }
    let catalog_len = u32::try_from(catalog.len()).expect("catalog length");
    catalog[2..6].copy_from_slice(&catalog_len.to_le_bytes());
    write_seed("seeds/catia_catalog", "minimal", &catalog);

    let mut value_block = vec![0x7c, 0x0b, 0, 0, 0, 0, 0x32, 1, 0, 0, 0];
    let value_len = u32::try_from(value_block.len()).expect("value-block length");
    value_block[2..6].copy_from_slice(&value_len.to_le_bytes());
    value_block.push(0xfe);
    write_seed("seeds/catia_value_block", "minimal", &value_block);

    let record_body = [0x04, 0x01, 0x82];
    let mut object_record = vec![0x7c, 0x09];
    object_record.extend_from_slice(&(6_u32 + record_body.len() as u32).to_le_bytes());
    object_record.extend_from_slice(&record_body);
    let mut object_graph = vec![0x7c, 0x08];
    object_graph.extend_from_slice(&(6_u32 + object_record.len() as u32).to_le_bytes());
    object_graph.extend_from_slice(&object_record);
    write_seed("seeds/catia_object_graph", "minimal", &object_graph);

    let mut topology = vec![0x01, 0x44, 0x01, 0xff, 10, 0, 0, 0, 10];
    for handle in [1u16, 10, 11, 12, 13, 14, 15, 16, 17, 10] {
        topology.extend_from_slice(&handle.to_be_bytes());
    }
    topology.extend_from_slice(&[0x30, 0x04, 0x04, 0xff, 0xd2, 0xd2, 0xd2, 0xd2]);
    write_seed("seeds/catia_topology", "minimal", &topology);

    write_seed("seeds/catia_e5_orientation", "minimal", &e5);
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

    write_seed("seeds/creo_datum", "minimal", &surface_rows);
    write_seed("seeds/creo_scalar", "minimal", &compact_int);
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

    let mut om = b"\x04\x01\x0eNX \x00hostglobalvariables".to_vec();
    om.extend_from_slice(&[0x00, 0x01, 0xff]);
    write_seed("seeds/nx_om", "minimal", &om);

    write_seed(
        "seeds/nx_deltas",
        "minimal",
        &[0x00, 0x1e, 0x01, 0x00, 0x00, 0x00],
    );
    write_seed("seeds/nx_topology", "minimal", &[0x00, 0x0c, 0x00, 0x00]);

    let mut intersection = vec![0x00, 0x28];
    intersection.extend_from_slice(&[0; 52]);
    write_seed("seeds/nx_intersection", "minimal", &intersection);
}

// ============================================================================
// Inventor and shared-container seeds
// ============================================================================

fn generate_inventor_submodule_seeds() -> Result<(), CodecError> {
    let cfb = synthetic_cfb_seed()?;
    write_seed("seeds/inventor_codec", "minimal", &cfb);
    write_seed("seeds/compound_snapshot", "minimal", &cfb);
    write_seed(
        "seeds/inventor_database",
        "minimal",
        &synthetic_database_seed(),
    );

    let metadata_body = synthetic_meta_table_body();
    let mut metadata = Vec::new();
    push_u32(&mut metadata, 24);
    metadata.extend_from_slice(b"RSe Meta Stream Version 8");
    push_u16(&mut metadata, 8);
    for value in [1_u16, 0, 2, 0, 3, 0, 4, 0] {
        push_u16(&mut metadata, value);
    }
    push_utf16(&mut metadata, "Synthetic PmBRep");
    metadata.extend_from_slice(&[0x5a; 16]);
    for value in [1_u32, 0, 0] {
        push_u32(&mut metadata, value);
    }
    push_utf8(&mut metadata, "2000-01-01");
    push_utf8(&mut metadata, "2000-01-02");
    metadata.push(0);
    let mut encoder = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
    encoder
        .write_all(&metadata_body)
        .expect("synthetic metadata fits the encoder");
    metadata.extend_from_slice(&encoder.finish().expect("synthetic metadata finishes"));
    write_seed("seeds/inventor_rse_meta", "minimal", &metadata);

    let mut records = metadata_body.clone();
    let mut bulk = Vec::new();
    push_u32(&mut bulk, 0);
    push_u32(&mut bulk, 0);
    push_u32(&mut bulk, u32::MAX);
    bulk.resize(metadata_body.len(), 0);
    records.extend_from_slice(&bulk);
    write_seed("seeds/inventor_rse_records", "minimal", &records);

    write_seed(
        "seeds/inventor_property_set",
        "minimal",
        &synthetic_property_set_seed(),
    );
    write_seed(
        "seeds/inventor_protein_envelope",
        "empty",
        &0_u32.to_le_bytes(),
    );
    write_seed("seeds/protein_decode", "malformed_page", &[0; 304]);
    Ok(())
}

fn generate_rhino_submodule_seeds() {
    let cage_body = {
        let mut body = Vec::new();
        for value in [1_i32, 0, 1, 0, 2, 2, 2, 2, 2, 2] {
            body.extend_from_slice(&value.to_le_bytes());
        }
        body
    };
    let mut cage = vec![0];
    cage.extend(rhino_crc_chunk(0x4000_0000, &cage_body));
    write_seed("seeds/rhino_cage", "minimal", &cage);

    let mut hatch_body = Vec::new();
    for _ in 0..12 {
        hatch_body.extend_from_slice(&0.0_f64.to_le_bytes());
    }
    let mut hatch = vec![0];
    hatch.extend(rhino_crc_chunk(0x4000_0000, &hatch_body));
    write_seed("seeds/rhino_hatch", "minimal", &hatch);

    let mut polyedge_body = vec![0x10];
    for value in [1_i32, 0, 0] {
        polyedge_body.extend_from_slice(&value.to_le_bytes());
    }
    polyedge_body.extend_from_slice(&[0; 48]);
    polyedge_body.extend_from_slice(&2_i32.to_le_bytes());
    polyedge_body.extend_from_slice(&0.0_f64.to_le_bytes());
    polyedge_body.extend_from_slice(&10.0_f64.to_le_bytes());
    let mut polyedge = vec![0];
    polyedge.extend(rhino_crc_chunk(0x4000_0000, &polyedge_body));
    write_seed("seeds/rhino_polyedge", "minimal", &polyedge);
}

fn rhino_crc_chunk(typecode: u32, body: &[u8]) -> Vec<u8> {
    let mut with_crc = body.to_vec();
    with_crc.extend(crc32fast::hash(body).to_le_bytes());
    let mut bytes = (typecode | 0x8000).to_le_bytes().to_vec();
    let len = i32::try_from(with_crc.len()).expect("rhino seed chunk fits i32");
    bytes.extend_from_slice(&len.to_le_bytes());
    bytes.extend(with_crc);
    bytes
}

fn synthetic_cfb_seed() -> Result<Vec<u8>, CodecError> {
    const SECTOR: usize = 512;
    const FREE: u32 = 0xffff_ffff;
    const END: u32 = 0xffff_fffe;
    const FAT: u32 = 0xffff_fffd;
    let mut file = alloc_filled(SECTOR * 13, 0_u8, "Inventor synthetic CFB seed")?;
    file[..8].copy_from_slice(&[0xd0, 0xcf, 0x11, 0xe0, 0xa1, 0xb1, 0x1a, 0xe1]);
    put_u16(&mut file, 24, 0x003e);
    put_u16(&mut file, 26, 3);
    put_u16(&mut file, 28, 0xfffe);
    put_u16(&mut file, 30, 9);
    put_u16(&mut file, 32, 6);
    put_u32(&mut file, 44, 1);
    put_u32(&mut file, 48, 0);
    put_u32(&mut file, 56, 4096);
    put_u32(&mut file, 60, 10);
    put_u32(&mut file, 64, 1);
    put_u32(&mut file, 68, END);
    put_u32(&mut file, 72, 0);
    for index in 0..109 {
        put_u32(&mut file, 76 + index * 4, FREE);
    }
    put_u32(&mut file, 76, 11);

    let directory = sector_mut(&mut file, SECTOR, 0);
    for entry in directory.chunks_exact_mut(128) {
        entry.fill(0);
        entry[68..80].fill(0xff);
    }
    directory_entry(directory, 0, "Root Entry", 5, FREE, FREE, 1, 1, 64);
    directory_entry(directory, 1, "RSeStorage", 1, FREE, FREE, 2, END, 0);
    directory_entry(directory, 2, "RSeSegInfo", 2, FREE, FREE, FREE, 0, 16);

    let root_mini = sector_mut(&mut file, SECTOR, 1);
    root_mini[..16].copy_from_slice(&synthetic_registry_seed());
    let mini_fat = sector_mut(&mut file, SECTOR, 10);
    mini_fat.fill(0xff);
    put_u32(mini_fat, 0, END);

    let fat = sector_mut(&mut file, SECTOR, 11);
    fat.fill(0xff);
    put_u32(fat, 0, END);
    put_u32(fat, 1 * 4, END);
    put_u32(fat, 10 * 4, END);
    put_u32(fat, 11 * 4, FAT);
    Ok(file)
}

fn synthetic_registry_seed() -> Vec<u8> {
    let mut bytes = Vec::new();
    push_u32(&mut bytes, 0);
    push_u16(&mut bytes, 0);
    push_u16(&mut bytes, 0);
    push_u32(&mut bytes, 0);
    push_u32(&mut bytes, 0);
    bytes
}

fn synthetic_database_seed() -> Vec<u8> {
    let mut bytes = vec![0x42; 16];
    push_u32(&mut bytes, 31);
    push_version(&mut bytes, 24);
    bytes.extend_from_slice(&17_u64.to_le_bytes());
    push_version(&mut bytes, 25);
    bytes.extend_from_slice(&18_u64.to_le_bytes());
    push_utf16(&mut bytes, "synthetic database");
    bytes
}

fn synthetic_meta_table_body() -> Vec<u8> {
    let mut body = Vec::new();
    for value in [3_u16, 0, 2, 1, 0, 4, 0] {
        push_u16(&mut body, value);
    }
    push_counted(&mut body, &[0x8000_0000], 4);
    push_counted(&mut body, &[], 10);
    push_counted(&mut body, &[], 28);
    push_u32(&mut body, 1);
    body.extend_from_slice(&[
        0x5c, 0x59, 0x45, 0xf6, 0xd5, 0x11, 0x33, 0x13, 0x10, 0x00, 0x60, 0xa6, 0xbb, 0xa6, 0x47,
        0xb5,
    ]);
    push_u16(&mut body, 1);
    push_u32(&mut body, 2);
    push_u16(&mut body, 3);
    push_u32(&mut body, 4);
    push_u32(&mut body, 32);
    let payloads = [0_usize, 0, 0, 0, 0, 0, 72];
    let counts = [u32::MAX, 0, 0, 0, 0, 0, 18];
    push_u32(&mut body, counts[0]);
    body.resize(body.len() + payloads[0], 0);
    for index in 1..payloads.len() {
        push_u32(&mut body, (payloads[index - 1] + 4) as u32);
        push_u32(&mut body, counts[index]);
        body.resize(body.len() + payloads[index], 0);
    }
    body.extend_from_slice(&[0x77; 16]);
    body
}

fn synthetic_property_set_seed() -> Vec<u8> {
    let mut bytes = Vec::new();
    push_u16(&mut bytes, 0xfffe);
    push_u16(&mut bytes, 0);
    push_u32(&mut bytes, 0);
    bytes.extend_from_slice(&[0; 16]);
    push_u32(&mut bytes, 1);
    bytes.extend_from_slice(&[0x11; 16]);
    push_u32(&mut bytes, 48);
    push_u32(&mut bytes, 8);
    push_u32(&mut bytes, 0);
    bytes
}

fn push_counted(bytes: &mut Vec<u8>, values: &[u32], item_size: usize) {
    push_u32(bytes, values.len() as u32);
    for value in values {
        push_u32(bytes, *value);
    }
    bytes.resize(bytes.len() + values.len() * (item_size - 4), 0);
    push_u32(bytes, (4 + values.len() * item_size) as u32);
}

fn push_u16(bytes: &mut Vec<u8>, value: u16) {
    bytes.extend_from_slice(&value.to_le_bytes());
}

fn push_u32(bytes: &mut Vec<u8>, value: u32) {
    bytes.extend_from_slice(&value.to_le_bytes());
}

fn put_u16(bytes: &mut [u8], offset: usize, value: u16) {
    bytes[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
}

fn put_u32(bytes: &mut [u8], offset: usize, value: u32) {
    bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}

fn push_utf8(bytes: &mut Vec<u8>, value: &str) {
    push_u32(bytes, value.len() as u32);
    bytes.extend_from_slice(value.as_bytes());
}

fn push_utf16(bytes: &mut Vec<u8>, value: &str) {
    let units = value.encode_utf16().collect::<Vec<_>>();
    push_u32(bytes, units.len() as u32);
    for unit in units {
        push_u16(bytes, unit);
    }
}

fn push_version(bytes: &mut Vec<u8>, major: u8) {
    bytes.extend_from_slice(&[1, 2, major, 4, 5, 6, 7, 8]);
}

fn sector_mut(file: &mut [u8], sector_size: usize, id: usize) -> &mut [u8] {
    let start = sector_size * (id + 1);
    &mut file[start..start + sector_size]
}

fn directory_entry(
    directory: &mut [u8],
    index: usize,
    name: &str,
    object_type: u8,
    left: u32,
    right: u32,
    child: u32,
    start_sector: u32,
    size: u64,
) {
    let offset = index * 128;
    let entry = &mut directory[offset..offset + 128];
    entry.fill(0);
    let mut name_offset = 0;
    for unit in name.encode_utf16() {
        entry[name_offset..name_offset + 2].copy_from_slice(&unit.to_le_bytes());
        name_offset += 2;
    }
    entry[name_offset..name_offset + 2].copy_from_slice(&0_u16.to_le_bytes());
    entry[64..66].copy_from_slice(
        &u16::try_from(name_offset + 2)
            .expect("short name")
            .to_le_bytes(),
    );
    entry[66] = object_type;
    entry[67] = 1;
    entry[68..72].copy_from_slice(&left.to_le_bytes());
    entry[72..76].copy_from_slice(&right.to_le_bytes());
    entry[76..80].copy_from_slice(&child.to_le_bytes());
    entry[116..120].copy_from_slice(&start_sector.to_le_bytes());
    entry[120..128].copy_from_slice(&size.to_le_bytes());
}
