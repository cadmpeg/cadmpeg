// SPDX-License-Identifier: Apache-2.0
//! Schema 32001/33103 and disc14/disc20 shell-partition decode tests.
#![allow(clippy::unwrap_used)]

use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeOptions};

use crate::test_support::*;
use crate::SldprtCodec;

#[test]
fn decode_binds_schema_32001_face_intervals_through_bridge_ids() {
    let mut body = Vec::new();
    body.extend(entity51(2, 500, 0x0017, &[0, 510, 600, 0, 0, 0]));
    body.extend(entity51(2, 510, 0x001b, &[520, 0, 0, 0, 0, 0]));
    body.extend(entity51(2, 520, 0x001f, &[530, 0, 0, 0, 0, 0]));
    body.extend(entity51(2, 530, 0x0021, &[540, 0, 0, 0, 0, 0]));
    body.extend(entity51(2, 540, 0x0023, &[0, 0, 0, 0, 0, 0]));
    body.extend(entity51(2, 600, 0x0015, &[0, 500, 0, 0, 0, 0]));
    body.extend(entity51(1, 700, 0x001f, &[10, 0, 0, 0, 0, 0]));
    body.extend(owned_triangle(0, 900, 0.0));

    let decoded = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_body(&body)),
            &DecodeOptions::default(),
        )
        .unwrap();

    assert!(decoded.report().geometry_transferred);
    assert_eq!(decoded.ir().model.bodies.len(), 1);
    assert_eq!(decoded.ir().model.faces.len(), 1);
    assert_eq!(
        decoded.ir().model.shells[0].faces[0].0,
        "sldprt:brep:face#10"
    );
}

#[test]
fn decode_partitions_interleaved_schema_33103_faces_by_adjacency() {
    let mut body = Vec::new();
    body.extend(entity51(2, 500, 0x0017, &[90, 510, 0, 0, 0, 0]));
    body.extend(entity51(2, 501, 0x0017, &[91, 511, 0, 0, 0, 0]));
    body.extend(entity51(2, 510, 0x0019, &[90, 520, 0, 0, 0, 0]));
    body.extend(entity51(2, 511, 0x0019, &[91, 521, 0, 0, 0, 0]));
    for (region, lump, shell_link, shell) in [(520, 530, 540, 550), (521, 531, 541, 551)] {
        body.extend(entity51(1, region, 0x001b, &[lump, 0, 0, 0, 0, 0]));
        body.extend(entity51(2, lump, 0x001f, &[shell_link, 0, 0, 0, 0, 0]));
        body.extend(entity51(2, shell_link, 0x0021, &[shell, 0, 0, 0, 0, 0]));
        body.extend(entity51(2, shell, 0x0023, &[0, 0, 0, 0, 0, 0]));
    }
    body.extend(entity51(2, 600, 0x0013, &[90, 500, 0, 0, 0, 0]));
    body.extend(entity51(1, 700, 0x0015, &[701, 0, 0, 0, 0, 0]));
    body.extend(entity51(2, 601, 0x0013, &[91, 501, 0, 0, 0, 0]));
    body.extend(entity51(1, 800, 0x0015, &[801, 0, 0, 0, 0, 0]));
    body.extend(entity51(1, 701, 0x0015, &[700, 0, 0, 0, 0, 0]));
    body.extend(entity51(1, 801, 0x0015, &[800, 0, 0, 0, 0, 0]));
    body.extend(owned_triangle(0, 700, 0.0));
    body.extend(owned_triangle(200, 701, 2.0));
    body.extend(owned_triangle(400, 800, 10.0));
    body.extend(owned_triangle(600, 801, 12.0));

    let decoded = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_body(&body)),
            &DecodeOptions::default(),
        )
        .unwrap();

    assert_eq!(decoded.ir().model.shells.len(), 4);
    assert!(decoded
        .ir()
        .model
        .shells
        .iter()
        .all(|shell| shell.faces.len() == 1));
    for (native_shell, face_suffixes) in [(550, ["#10", "#210"]), (551, ["#410", "#610"])] {
        let prefix = format!("sldprt:brep:shell#{native_shell}");
        let faces = decoded
            .ir()
            .model
            .shells
            .iter()
            .filter(|shell| shell.id.0.starts_with(&prefix))
            .flat_map(|shell| &shell.faces)
            .collect::<Vec<_>>();
        assert_eq!(faces.len(), 2);
        assert!(face_suffixes
            .iter()
            .all(|suffix| faces.iter().any(|face| face.0.ends_with(suffix))));
    }
}

#[test]
fn decode_partitions_disc14_faces_by_native_shell_rings() {
    let mut body = Vec::new();
    body.extend(entity51(1, 900, 0x001a, &[500, 501, 0, 0, 0, 0]));
    body.extend(entity51(1, 500, 0x0016, &[550, 0, 0, 0, 0, 0]));
    body.extend(entity51(1, 501, 0x0016, &[602, 0, 0, 0, 0, 0]));
    body.extend(entity51(1, 550, 0x0012, &[600, 0, 0, 0, 0, 0]));
    body.extend(entity51(1, 600, 0x0020, &[0, 0, 609, 601, 0, 0]));
    body.extend(entity51(1, 601, 0x0020, &[0, 0, 701, 600, 0, 0]));
    body.extend(entity51(1, 602, 0x0020, &[0, 0, 612, 603, 0, 0]));
    body.extend(entity51(1, 603, 0x0020, &[0, 0, 613, 602, 0, 0]));
    body.extend(entity51(1, 609, 0x001e, &[0, 0, 610, 0, 0, 0]));
    for (geometry, face) in [(610, 700), (611, 701), (612, 800), (613, 801)] {
        body.extend(entity51(1, geometry, 0x0018, &[0, 0, face, 0, 0, 0]));
        body.extend(entity51(1, face, 0x0014, &[0, 0, 0, 0, 0, 0]));
    }
    body.extend(owned_triangle(0, 700, 0.0));
    body.extend(owned_triangle(200, 701, 2.0));
    body.extend(owned_triangle(400, 800, 10.0));
    body.extend(owned_triangle(600, 801, 12.0));

    let mut decoded = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_body(&body)),
            &DecodeOptions::default(),
        )
        .unwrap();

    assert_eq!(decoded.ir().model.bodies.len(), 1);
    assert_eq!(decoded.ir().model.regions.len(), 1);
    assert_eq!(decoded.ir().model.shells.len(), 4);
    assert!(decoded
        .ir()
        .model
        .shells
        .iter()
        .all(|shell| shell.faces.len() == 1));

    decoded.ir_mut().model.points[0].position.z += 1.0;
    let mut encoded = Vec::new();
    SldprtCodec
        .write_preserved_with_source_fidelity(decoded.ir(), decoded.source_fidelity(), &mut encoded)
        .unwrap();
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();
    assert_eq!(regenerated.ir().model.regions.len(), 1);
    assert_eq!(regenerated.ir().model.shells.len(), 4);
    assert!(regenerated
        .ir()
        .model
        .shells
        .iter()
        .all(|shell| shell.faces.len() == 1));
}

#[test]
fn decode_keeps_multiple_disc14_regions_as_separate_bodies() {
    let mut body = Vec::new();
    body.extend(entity51(1, 900, 0x001a, &[500, 0, 0, 0, 0, 0]));
    body.extend(entity51(1, 901, 0x001a, &[501, 0, 0, 0, 0, 0]));
    body.extend(entity51(1, 500, 0x0016, &[550, 0, 0, 0, 0, 0]));
    body.extend(entity51(1, 501, 0x0016, &[602, 0, 0, 0, 0, 0]));
    body.extend(entity51(1, 550, 0x0012, &[600, 0, 0, 0, 0, 0]));
    body.extend(entity51(1, 600, 0x0020, &[0, 0, 609, 601, 0, 0]));
    body.extend(entity51(1, 601, 0x0020, &[0, 0, 701, 600, 0, 0]));
    body.extend(entity51(1, 602, 0x0020, &[0, 0, 612, 603, 0, 0]));
    body.extend(entity51(1, 603, 0x0020, &[0, 0, 613, 602, 0, 0]));
    body.extend(entity51(1, 609, 0x001e, &[0, 0, 610, 0, 0, 0]));
    for (geometry, face) in [(610, 700), (611, 701), (612, 800), (613, 801)] {
        body.extend(entity51(1, geometry, 0x0018, &[0, 0, face, 0, 0, 0]));
        body.extend(entity51(1, face, 0x0014, &[0, 0, 0, 0, 0, 0]));
    }
    body.extend(owned_triangle(0, 700, 0.0));
    body.extend(owned_triangle(200, 701, 2.0));
    body.extend(owned_triangle(400, 800, 10.0));
    body.extend(owned_triangle(600, 801, 12.0));

    let decoded = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_body(&body)),
            &DecodeOptions::default(),
        )
        .unwrap();

    assert_eq!(decoded.ir().model.bodies.len(), 2);
    assert_eq!(decoded.ir().model.regions.len(), 2);
    for (body_attr, shell_prefix) in [
        (900, "sldprt:brep:shell#500"),
        (901, "sldprt:brep:shell#501"),
    ] {
        let body_id = format!("sldprt:brep:body#{body_attr}");
        let body = decoded
            .ir()
            .model
            .bodies
            .iter()
            .find(|body| body.id.0 == body_id)
            .unwrap();
        assert_eq!(body.regions.len(), 1);
        let region_id = &body.regions[0].0;
        assert_eq!(region_id, &format!("sldprt:brep:region#{body_attr}"));
        let region = decoded
            .ir()
            .model
            .regions
            .iter()
            .find(|region| region.id.0 == *region_id)
            .unwrap();
        assert_eq!(region.body.0, body_id);
        assert!(!region.shells.is_empty());
        assert!(region
            .shells
            .iter()
            .all(|shell| shell.0.starts_with(shell_prefix)));
    }
}

#[test]
fn decode_partitions_disc20_faces_by_native_single_shell_lattice() {
    let mut body = Vec::new();
    body.extend(entity51(2, 900, 0x001a, &[500, 0, 0, 0, 0, 0]));
    body.extend(entity51(1, 500, 0x0016, &[0, 0, 0, 0, 0, 0]));
    body.extend(entity51(1, 700, 0x0020, &[0, 710, 0, 701, 701, 0]));
    body.extend(entity51(1, 701, 0x0020, &[0, 711, 0, 700, 700, 0]));
    body.extend(entity51(
        4,
        710,
        0x0024,
        &[0, 720, 700, 711, 711, 0, 0, 0, 0],
    ));
    body.extend(entity51(
        4,
        711,
        0x0024,
        &[0, 721, 701, 710, 710, 0, 0, 0, 0],
    ));
    body.extend(entity51(3, 720, 0x0026, &[0, 0, 710, 721, 721, 0]));
    body.extend(entity51(3, 721, 0x0026, &[0, 0, 711, 720, 720, 0]));
    body.extend(owned_triangle(0, 700, 0.0));
    body.extend(owned_triangle(200, 701, 2.0));

    let decoded = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_body(&body)),
            &DecodeOptions::default(),
        )
        .unwrap();

    assert_eq!(decoded.ir().model.bodies[0].id.0, "sldprt:brep:body#900");
    assert_eq!(decoded.ir().model.regions[0].id.0, "sldprt:brep:region#900");
    assert_eq!(decoded.ir().model.shells[0].id.0, "sldprt:brep:shell#500");
    assert_eq!(decoded.ir().model.shells.len(), 2);
    assert!(decoded
        .ir()
        .model
        .shells
        .iter()
        .all(|shell| shell.faces.len() == 1));
    assert_eq!(decoded.ir().model.regions[0].shells.len(), 2);
    assert!(!decoded
        .report()
        .losses
        .iter()
        .any(|loss| loss.message.contains("No body record")));
}
