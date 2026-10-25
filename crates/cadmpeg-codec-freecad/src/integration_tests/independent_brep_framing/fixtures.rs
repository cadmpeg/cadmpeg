// SPDX-License-Identifier: Apache-2.0
//! Hand-authored OCCT table records, independent of decoder and writer helpers.

enum Atom {
    Byte(u8),
    Int(i32),
    Real(f64),
    Float(f32),
    Text(&'static str),
}
use Atom::{Byte, Float, Int, Real, Text};

fn row(atoms: Vec<Atom>, binary: bool) -> Vec<u8> {
    let mut bytes = Vec::new();
    for atom in atoms {
        if binary {
            match atom {
                Byte(value) => bytes.push(value),
                Int(value) => bytes.extend_from_slice(&value.to_le_bytes()),
                Real(value) => bytes.extend_from_slice(&value.to_le_bytes()),
                Float(value) => bytes.extend_from_slice(&value.to_le_bytes()),
                Text(_) => {}
            }
        } else {
            let value = match atom {
                Byte(value) => value.to_string(),
                Int(value) => value.to_string(),
                Real(value) => value.to_string(),
                Float(value) => value.to_string(),
                Text(value) => value.to_owned(),
            };
            if !bytes.is_empty() {
                bytes.push(b' ');
            }
            bytes.extend_from_slice(value.as_bytes());
        }
    }
    bytes
}
fn reals(atoms: &mut Vec<Atom>, values: &[f64]) {
    atoms.extend(values.iter().copied().map(Real));
}
fn line(dimension: usize) -> Vec<Atom> {
    let mut atoms = vec![Byte(1)];
    if dimension == 2 {
        reals(&mut atoms, &[0.0, 0.0, 1.0, 0.0]);
    } else {
        reals(&mut atoms, &[1.0, 0.0, 0.0, 0.0, 0.0, 1.0]);
    }
    atoms
}
fn record(table: usize, index: usize, binary: bool) -> Vec<u8> {
    let mut atoms = match table {
        0 => {
            let mut atoms = vec![Byte(1)];
            reals(
                &mut atoms,
                &[1.0, 0.0, 0.0, 5.0, 0.0, 1.0, 0.0, 6.0, 0.0, 0.0, 1.0, 7.0],
            );
            atoms
        }
        1 | 2 => {
            let mut atoms = vec![Byte(8), Real(0.0), Real(1.0), Byte(9), Real(0.25)];
            if table == 2 {
                reals(&mut atoms, &[0.0, 1.0, 0.0]);
            }
            atoms.extend(line(if table == 1 { 2 } else { 3 }));
            atoms
        }
        3 => {
            let mut atoms = vec![Int(2), Byte(1), Real(0.01)];
            reals(&mut atoms, &[0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0]);
            atoms
        }
        4 => vec![
            Int(2),
            Int(1),
            Int(2),
            Text("p"),
            Real(0.02),
            Byte(1),
            Real(0.0),
            Real(1.0),
        ],
        5 => {
            let mut atoms = match index {
                0 => vec![Byte(6), Real(1.0), Real(0.0), Real(0.0)],
                1 => vec![
                    Byte(7),
                    Real(0.0),
                    Real(0.0),
                    Real(0.0),
                    Real(0.0),
                    Real(0.0),
                    Real(1.0),
                ],
                _ => vec![
                    Byte(10),
                    Real(0.0),
                    Real(1.0),
                    Real(0.0),
                    Real(1.0),
                    Byte(11),
                    Real(0.25),
                    Byte(1),
                ],
            };
            if index < 2 {
                atoms.extend(line(3));
            } else {
                reals(
                    &mut atoms,
                    &[0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0],
                );
            }
            atoms
        }
        6 => {
            let mut atoms = vec![Int(3), Int(1), Byte(1), Byte(1), Real(0.03)];
            reals(
                &mut atoms,
                &[
                    0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0,
                ],
            );
            atoms.extend([Int(1), Int(2), Int(3)]);
            for _ in 0..3 {
                atoms.extend([Float(0.0), Float(0.0), Float(1.0)]);
            }
            atoms
        }
        7 => {
            return if binary {
                vec![0, 1, 0, 0, 1, 0, 0, 0, b'*']
            } else {
                b"Co 1001000 *".to_vec()
            };
        }
        _ => unreachable!(),
    };
    row(std::mem::take(&mut atoms), binary)
}
pub(super) fn member(binary: bool) -> Vec<u8> {
    let mut bytes = if binary {
        b"Open CASCADE Topology V4, (c) Open Cascade\n".to_vec()
    } else {
        b"CASCADE Topology V3, (c) Open Cascade\n".to_vec()
    };
    for (table, name) in [
        "Locations",
        "Curve2ds",
        "Curves",
        "Polygon3D",
        "PolygonOnTriangulations",
        "Surfaces",
        "Triangulations",
        "TShapes",
    ]
    .into_iter()
    .enumerate()
    {
        bytes.extend_from_slice(format!("{name} 3\n").as_bytes());
        for index in 0..3 {
            bytes.extend(record(table, index, binary));
            if !binary {
                bytes.push(b'\n');
            }
        }
    }
    if binary {
        for _ in 0..3 {
            bytes.extend_from_slice(&(-1_i32).to_le_bytes());
        }
    } else {
        bytes.extend_from_slice(b"*");
    }
    bytes
}
pub(super) fn archive(binary: bool, member: &[u8]) -> Vec<u8> {
    let name = if binary { "Shape.BIN" } else { "Shape.brp" };
    let document = format!("<Document SchemaVersion='4' FileVersion='1'><Objects Count='1'><Object type='Part::Feature' name='Shape' id='1'/></Objects><ObjectData Count='1'><Object name='Shape'><Properties Count='2'><Property name='Shape' type='Part::PropertyPartShape'><Part file='{name}'/></Property><Property name='Peer' type='App::PropertyString'><String value='independent'/></Property></Properties></Object></ObjectData></Document>");
    cadmpeg_test_support::zip_fixture::archive(&[
        cadmpeg_test_support::zip_fixture::RawEntry::stored("Document.xml", document.as_bytes()),
        cadmpeg_test_support::zip_fixture::RawEntry::stored(name, member),
        cadmpeg_test_support::zip_fixture::RawEntry::stored(
            "peer.dat",
            b"independent source bytes",
        ),
    ])
}
