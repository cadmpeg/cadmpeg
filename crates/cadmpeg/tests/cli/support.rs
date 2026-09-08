// SPDX-License-Identifier: Apache-2.0
//! Fixture builders shared by the CLI test groups.

use std::fs;
use std::io::Write;

use cadmpeg_ir::examples::unit_cube;

pub fn fixture(dir: &std::path::Path, name: &str, ir: &cadmpeg_ir::CadIr) -> std::path::PathBuf {
    let path = dir.join(name);
    fs::write(&path, ir.to_canonical_json().unwrap()).unwrap();
    path
}

pub fn minimal_fcstd(dir: &std::path::Path, name: &str) -> std::path::PathBuf {
    let path = dir.join(name);
    let file = fs::File::create(&path).unwrap();
    let mut zip = zip::ZipWriter::new(file);
    zip.start_file(
        "Document.xml",
        zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored),
    )
    .unwrap();
    zip.write_all(
        b"<Document SchemaVersion=\"4\" FileVersion=\"1\" ProgramVersion=\"1.0\"><Object/></Document>",
    )
    .unwrap();
    zip.finish().unwrap();
    path
}

pub fn geometryless_creo(dir: &std::path::Path, name: &str) -> std::path::PathBuf {
    let path = dir.join(name);
    fs::write(
        &path,
        b"#UGC:2 P test\n#-END_OF_UGC_HEADER\n#UGC_TOC\n#END_OF_TOC_HEADER\n#\n#VisibGeom\n\0",
    )
    .unwrap();
    path
}

pub fn rhino_header(version: &str) -> Vec<u8> {
    let mut bytes = b"3D Geometry File Format ".to_vec();
    let mut version_field = [b' '; 8];
    let start = version_field.len() - version.len();
    version_field[start..].copy_from_slice(version.as_bytes());
    bytes.extend(version_field);
    assert_eq!(bytes.len(), 32);
    bytes
}

pub fn rhino_long_chunk(version: u64, typecode: u32, body: &[u8]) -> Vec<u8> {
    let mut bytes = typecode.to_le_bytes().to_vec();
    if version >= 50 {
        bytes.extend((body.len() as i64).to_le_bytes());
    } else {
        bytes.extend((body.len() as i32).to_le_bytes());
    }
    bytes.extend(body);
    bytes
}

pub fn rhino_short_chunk(version: u64, typecode: u32, value: i64) -> Vec<u8> {
    let mut bytes = typecode.to_le_bytes().to_vec();
    if version >= 50 {
        bytes.extend(value.to_le_bytes());
    } else {
        bytes.extend((value as i32).to_le_bytes());
    }
    bytes
}

pub fn rhino_crc_chunk(version: u64, typecode: u32, body: &[u8]) -> Vec<u8> {
    let mut payload = body.to_vec();
    payload.extend(crc32fast::hash(body).to_le_bytes());
    rhino_long_chunk(version, typecode, &payload)
}

pub fn rhino_table(version: u64, typecode: u32) -> Vec<u8> {
    let end = rhino_short_chunk(version, 0xffff_ffff, 0);
    rhino_long_chunk(version, typecode, &end)
}

pub fn rhino_object_record(version: u64, class_uuid: [u8; 16], payload: &[u8]) -> Vec<u8> {
    let object_type = rhino_short_chunk(version, 0x8200_0071, 1);
    let mut uuid_body = class_uuid.to_vec();
    uuid_body.extend(crc32fast::hash(&class_uuid).to_le_bytes());
    let uuid = rhino_long_chunk(version, 0x0002_fffb, &uuid_body);
    let class_data = rhino_crc_chunk(version, 0x0002_fffc, payload);
    let class_end = rhino_short_chunk(version, 0x8002_7fff, 0);
    let class = rhino_long_chunk(
        version,
        0x0002_7ffa,
        &[uuid, class_data, class_end].concat(),
    );
    let object_end = rhino_short_chunk(version, 0x8200_007f, 0);
    rhino_crc_chunk(
        version,
        0x2000_8070,
        &[object_type, class, object_end].concat(),
    )
}

pub fn synthetic_rhino_point(
    dir: &std::path::Path,
    name: &str,
    point: [f64; 3],
) -> std::path::PathBuf {
    let version = 50;
    let mut payload = vec![0x10];
    for coordinate in point {
        payload.extend(coordinate.to_le_bytes());
    }
    let point_class = [
        0x1d, 0x1a, 0x10, 0xc3, 0x57, 0xf1, 0xd3, 0x11, 0xbf, 0xe7, 0x00, 0x10, 0x83, 0x01, 0x22,
        0xf0,
    ];
    let object = rhino_object_record(version, point_class, &payload);
    let end = rhino_short_chunk(version, 0xffff_ffff, 0);
    let object_table = rhino_long_chunk(version, 0x1000_0013, &[object, end].concat());

    let mut units = 100_i32.to_le_bytes().to_vec();
    units.extend(2_i32.to_le_bytes());
    units.extend(0.01_f64.to_le_bytes());
    units.extend(0.1_f64.to_le_bytes());
    units.extend(0.001_f64.to_le_bytes());
    let units = rhino_crc_chunk(version, 0x2000_8031, &units);
    let settings_table = rhino_long_chunk(
        version,
        0x1000_0015,
        &[units, rhino_short_chunk(version, 0xffff_ffff, 0)].concat(),
    );

    let mut bytes = rhino_header("50");
    bytes.extend(rhino_long_chunk(version, 1, b"cadmpeg CLI geometry"));
    bytes.extend(rhino_table(version, 0x1000_0014));
    bytes.extend(settings_table);
    bytes.extend(object_table);
    let eof_offset = bytes.len();
    bytes.extend(rhino_long_chunk(version, 0x0000_7fff, &[0; 8]));
    let eof = rhino_long_chunk(version, 0x0000_7fff, &(bytes.len() as u64).to_le_bytes());
    bytes[eof_offset..].copy_from_slice(&eof);

    let path = dir.join(name);
    fs::write(&path, bytes).unwrap();
    path
}

pub fn minimal_rhino_archive(
    dir: &std::path::Path,
    name: &str,
    version_text: &str,
) -> std::path::PathBuf {
    minimal_rhino_archive_with_comment(dir, name, version_text, b"cadmpeg test")
}

pub fn minimal_rhino_archive_with_comment(
    dir: &std::path::Path,
    name: &str,
    version_text: &str,
    comment: &[u8],
) -> std::path::PathBuf {
    let version = version_text.parse::<u64>().unwrap();
    let mut bytes = rhino_header(version_text);
    bytes.extend(rhino_long_chunk(version, 0x0000_0001, comment));
    bytes.extend(rhino_table(version, 0x1000_0014));
    bytes.extend(rhino_table(version, 0x1000_0015));
    bytes.extend(rhino_table(version, 0x1000_0013));

    let eof_offset = bytes.len();
    let width = if version >= 50 { 8 } else { 4 };
    bytes.extend(rhino_long_chunk(version, 0x0000_7fff, &vec![0; width]));
    let file_size = bytes.len();
    let eof_body = if version >= 50 {
        (file_size as u64).to_le_bytes().to_vec()
    } else {
        (file_size as u32).to_le_bytes().to_vec()
    };
    let eof = rhino_long_chunk(version, 0x0000_7fff, &eof_body);
    bytes[eof_offset..].copy_from_slice(&eof);

    let path = dir.join(name);
    fs::write(&path, bytes).unwrap();
    path
}

pub fn sldprt_cube() -> cadmpeg_ir::CadIr {
    let mut ir = unit_cube();
    ir.model.bodies[0].name = None;
    ir.model.faces.iter_mut().for_each(|face| face.name = None);
    ir.model
        .edges
        .iter_mut()
        .for_each(|edge| edge.param_range = None);
    ir
}

/// A cube carrying source metadata with the given attributes.
pub fn cube_with_source(attributes: &[(&str, &str)]) -> cadmpeg_ir::CadIr {
    let mut ir = unit_cube();
    ir.source = Some(cadmpeg_ir::SourceMeta::classified(
        cadmpeg_core::dialect::DialectLayers::of(cadmpeg_core::dialect::DialectMatch::admitted(
            cadmpeg_core::dialect::DialectId::pinned("synthetic:test"),
        )),
        attributes
            .iter()
            .map(|(key, value)| ((*key).to_owned(), (*value).to_owned()))
            .collect(),
    ));
    ir
}
