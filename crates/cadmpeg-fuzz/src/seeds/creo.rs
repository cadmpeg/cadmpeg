// SPDX-License-Identifier: Apache-2.0
//! Creo PRT seed builders.

pub fn just_magic() -> Vec<u8> {
    b"#UGC:2 P test\n".to_vec()
}

pub fn build_prt(version: &str, sections: &[(&str, Vec<u8>)]) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(format!("#UGC:2 P {version}\n").as_bytes());
    out.extend_from_slice(b"#-END_OF_UGC_HEADER\n");
    out.extend_from_slice(b"#UGC_TOC\n");
    out.extend_from_slice(b"toc entry line\n");
    out.extend_from_slice(b"#END_OF_TOC_HEADER\n");
    for (name, payload) in sections {
        out.push(b'#');
        out.push(b'\n');
        out.push(b'#');
        out.extend_from_slice(name.as_bytes());
        out.push(b'\n');
        out.extend_from_slice(payload);
    }
    out
}

pub fn visibgeom_payload(srf: u8, crv: u8) -> Vec<u8> {
    let mut p = Vec::new();
    p.extend_from_slice(b"srf_array\0");
    p.extend_from_slice(&[0xf8, srf]);
    p.extend_from_slice(&[0xe0, 0x22, b'p', 0]);
    p.extend_from_slice(b"crv_array\0");
    p.extend_from_slice(&[0xf3, 0xf8, crv]);
    p
}

pub fn minimal_prt() -> Vec<u8> {
    build_prt("c", &[("VisibGeom", vec![0x00])])
}

pub fn with_visibgeom() -> Vec<u8> {
    build_prt("c", &[("VisibGeom", visibgeom_payload(5, 12))])
}
