// SPDX-License-Identifier: Apache-2.0
//! Shared synthetic byte-fixture builders for the crate's `#[cfg(test)]` suites.
//!
//! Helpers hand-build `.prt` byte images and embedded-stream payloads. They
//! construct raw bytes only; no native record type crosses in here.
#![allow(clippy::unwrap_used)]

use std::io::Write;

use flate2::write::ZlibEncoder;
use flate2::Compression;

pub(crate) const MAGIC: &[u8; 8] = b"SPLMSSTR";

pub(crate) fn shifted_f64_bytes(value: f64) -> [u8; 8] {
    let mut bytes = value.to_be_bytes();
    bytes[0] -= 0x10;
    bytes
}

pub(crate) fn attach_test_body_surface(
    ir: &mut cadmpeg_ir::document::CadIr,
    body_id: &cadmpeg_ir::ids::BodyId,
    surface: cadmpeg_ir::ids::SurfaceId,
) {
    use cadmpeg_ir::ids::{FaceId, RegionId, ShellId};
    use cadmpeg_ir::topology::{Body, BodyKind, Face, Region, Sense, Shell};

    let region_id = RegionId::mint(format!("{}:region", body_id.0)).expect("identity grammar");
    let shell_id = ShellId::mint(format!("{}:shell", body_id.0)).expect("identity grammar");
    if !ir.model.bodies.iter().any(|body| body.id == *body_id) {
        ir.model.bodies.push(Body {
            id: body_id.clone(),
            kind: BodyKind::Solid,
            regions: vec![region_id.clone()],
            transform: None,
            name: None,
            color: None,
            visible: None,
        });
        ir.model.regions.push(Region {
            id: region_id.clone(),
            body: body_id.clone(),
            shells: vec![shell_id.clone()],
        });
        ir.model.shells.push(Shell {
            id: shell_id.clone(),
            region: region_id,
            faces: Vec::new(),
            wire_edges: Vec::new(),
            free_vertices: Vec::new(),
        });
    }
    let face_id = FaceId::mint(format!("{}:face#{}", body_id.0, ir.model.faces.len()))
        .expect("identity grammar");
    ir.model
        .shells
        .iter_mut()
        .find(|shell| shell.id == shell_id)
        .unwrap()
        .faces
        .push(face_id.clone());
    ir.model.faces.push(Face {
        id: face_id,
        shell: shell_id,
        surface,
        sense: Sense::Forward,
        loops: Vec::new().into(),
        name: None,
        color: None,
        tolerance: None,
    });
}

pub(crate) fn be_f64(v: f64) -> [u8; 8] {
    v.to_be_bytes()
}

/// Write three big-endian doubles into `rec` starting at `at`.
pub(crate) fn put_vec3(rec: &mut [u8], at: usize, xyz: [f64; 3]) {
    for (i, v) in xyz.iter().enumerate() {
        rec[at + 8 * i..at + 8 * i + 8].copy_from_slice(&be_f64(*v));
    }
}

pub(crate) fn put_f64(rec: &mut [u8], at: usize, v: f64) {
    rec[at..at + 8].copy_from_slice(&be_f64(v));
}

pub(crate) fn put_ref(rec: &mut [u8], at: usize, value: u16) {
    rec[at..at + 2].copy_from_slice(&value.to_be_bytes());
}

pub(crate) fn encoded_xmt(value: u32) -> Vec<u8> {
    if i16::try_from(value).is_ok() {
        return (value as u16).to_be_bytes().to_vec();
    }
    let quotient = value / 32_767;
    let remainder = value % 32_767;
    assert!(remainder > 0 && i16::try_from(remainder).is_ok());
    let mut out = (-(remainder as i16)).to_be_bytes().to_vec();
    out.extend_from_slice(&(quotient as u16).to_be_bytes());
    out
}

/// One fixed-length analytic record: a `00 <tag>` header then zeroed payload the
/// caller fills at the documented offsets.
pub(crate) fn record(tag: u8, len: usize) -> Vec<u8> {
    let mut r = vec![0u8; len];
    r[0] = 0x00;
    r[1] = tag;
    r
}

pub(crate) fn zlib_compress(raw: &[u8]) -> Vec<u8> {
    // Level 1 emits the `78 01` zlib header NX/Parasolid streams use.
    let mut e = ZlibEncoder::new(Vec::new(), Compression::new(1));
    e.write_all(raw).unwrap();
    e.finish().unwrap()
}

pub(crate) fn zlib_compress_at_level(raw: &[u8], level: u32) -> Vec<u8> {
    let mut e = ZlibEncoder::new(Vec::new(), Compression::new(level));
    e.write_all(raw).unwrap();
    e.finish().unwrap()
}
