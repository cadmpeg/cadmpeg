// SPDX-License-Identifier: Apache-2.0
//! Golden inspect and decode snapshots over synthetic packages.
//!
//! Three inputs are committed files under `tests/golden/fixtures`
//! (`synthetic_cube.eprt`, `synthetic_assembly.easm`,
//! `synthetic_sw_assembly.easm`); the others are built in code. A test checks
//! that the committed bytes equal what the writer produces.
//! `UPDATE_GOLDEN_FIXTURES=1` rewrites the three files and `UPDATE_GOLDEN=1`
//! rewrites the snapshots their comparison rejects.

use std::io::Cursor;
use std::path::PathBuf;

use cadmpeg_core::decode::InspectOptions;
use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_test_support::golden::{snapshot_text, Branch, Harness};
use cadmpeg_test_support::EditableDecodeResult;

use crate::test_support::hsf::{box_shells, matrix, BodySpec, Item, ModelSpec, NodeSpec};
use crate::test_support::models::{
    annotation_stream, assembly, assembly_fixture, cube_fixture, cube_part, solidworks_assembly,
    solidworks_fixture,
};
use crate::test_support::{assembly_package_of, part_package};
use crate::EdrawingsCodec;

const REGENERATE: &str = "UPDATE_GOLDEN=1 cargo test -p cadmpeg-codec-edrawings golden";
const CUBE_FILE: &str = "synthetic_cube.eprt";
const ASSEMBLY_FILE: &str = "synthetic_assembly.easm";
const SOLIDWORKS_FILE: &str = "synthetic_sw_assembly.easm";
/// The unit of the fixtures' length: metres per centimetre.
const CENTIMETRE: f32 = 0.01;

fn harness() -> Harness {
    Harness::new(env!("CARGO_MANIFEST_DIR"), "eprt", REGENERATE)
}

fn fixture_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/golden/fixtures")
        .join(name)
}

fn branches() -> [Branch; 2] {
    [
        Branch::named("inspect", inspect_snapshot),
        Branch::named("decode", decode_snapshot),
    ]
}

fn inspect_snapshot(bytes: &[u8]) -> String {
    let value = match EdrawingsCodec.inspect(&mut Cursor::new(bytes), &InspectOptions::default()) {
        Ok(summary) => serde_json::to_value(summary).expect("serialize inspect summary"),
        Err(error) => serde_json::json!({ "inspect_error": error.to_string() }),
    };
    snapshot_text(&value)
}

fn decode_snapshot(bytes: &[u8]) -> String {
    let value = match EdrawingsCodec.decode(&mut Cursor::new(bytes), &DecodeOptions::default()) {
        Ok(result) => {
            let result = EditableDecodeResult::from(result);
            serde_json::json!({
                "ir": result.ir(),
                "report": result.report(),
                "source_fidelity": result.source_fidelity(),
            })
        }
        Err(error) => serde_json::json!({ "decode_error": error.to_string() }),
    };
    snapshot_text(&value)
}

fn single(items: Vec<Item>, placement: Option<[f32; 12]>) -> Vec<u8> {
    part_package(
        &ModelSpec {
            document: "single.sldprt".into(),
            bodies: vec![BodySpec {
                colour: None,
                subs: vec![items],
            }],
            tree: vec![NodeSpec::Part {
                segment: "instance0".into(),
                name: "single".into(),
                body: 0,
                matrix: placement,
                user_name: false,
            }],
            state: None,
        }
        .stream(),
    )
}

fn committed(name: &str) -> Vec<u8> {
    std::fs::read(fixture_path(name)).unwrap_or_else(|error| panic!("read fixture {name}: {error}"))
}

fn inputs() -> Vec<(String, Vec<u8>)> {
    let moved = matrix(
        [[0.0, 1.0, 0.0], [-1.0, 0.0, 0.0], [0.0, 0.0, 1.0]],
        [CENTIMETRE, 0.0, 0.0],
    );
    let mut trailing = cube_part().stream();
    trailing.push(0);
    let mut bad_shell = box_shells([CENTIMETRE; 3]);
    bad_shell.push(Item::Shell {
        points: vec![[0.0; 3]; 3],
        strips: vec![vec![0, 1, 7]],
    });
    vec![
        ("synthetic_cube".to_owned(), committed(CUBE_FILE)),
        ("synthetic_assembly".to_owned(), committed(ASSEMBLY_FILE)),
        (
            "synthetic_sw_assembly".to_owned(),
            committed(SOLIDWORKS_FILE),
        ),
        (
            "part_with_matrix".to_owned(),
            single(box_shells([CENTIMETRE; 3]), Some(moved)),
        ),
        (
            "open_sheet".to_owned(),
            single(
                box_shells([CENTIMETRE; 3]).into_iter().take(2).collect(),
                None,
            ),
        ),
        ("part_with_bad_shell".to_owned(), single(bad_shell, None)),
        (
            "stream_with_trailing_byte".to_owned(),
            part_package(&trailing),
        ),
        ("stream_without_instances".to_owned(), part_package(b"z")),
        (
            "late_sw_assembly".to_owned(),
            assembly_package_of("19.00", &solidworks_assembly().stream()),
        ),
        (
            "annotation_records".to_owned(),
            part_package(&annotation_stream()),
        ),
    ]
}

#[test]
fn golden_snapshots_hold() {
    harness().check_inputs(&inputs(), &branches());
}

#[test]
fn golden_output_is_deterministic() {
    harness().check_determinism_inputs(&inputs(), &branches());
}

#[test]
fn committed_fixtures_equal_the_writer_output() {
    let expected = [
        (CUBE_FILE, cube_fixture()),
        (ASSEMBLY_FILE, assembly_fixture()),
        (SOLIDWORKS_FILE, solidworks_fixture()),
    ];
    if std::env::var_os("UPDATE_GOLDEN_FIXTURES").is_some() {
        for (name, bytes) in &expected {
            let path = fixture_path(name);
            std::fs::create_dir_all(path.parent().expect("fixture directory"))
                .expect("create fixture directory");
            std::fs::write(&path, bytes).expect("write fixture");
        }
    }
    for (name, bytes) in &expected {
        assert!(
            &committed(name) == bytes,
            "{name} differs from the writer output; regenerate with \
             UPDATE_GOLDEN_FIXTURES=1 cargo test -p cadmpeg-codec-edrawings golden"
        );
    }
}

/// The fuzz seeds are the raw streams of the three fixtures and the
/// annotation records stream.
#[test]
fn fuzz_seeds_equal_the_writer_output() {
    let root =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../cadmpeg-fuzz/seeds/edrawings_model");
    let expected = [
        ("synthetic_cube", cube_part().stream()),
        ("synthetic_assembly", assembly().stream()),
        ("synthetic_sw_assembly", solidworks_assembly().stream()),
        ("annotation_records", annotation_stream()),
    ];
    if std::env::var_os("UPDATE_GOLDEN_FIXTURES").is_some() {
        std::fs::create_dir_all(&root).expect("create seed directory");
        for (name, bytes) in &expected {
            std::fs::write(root.join(name), bytes).expect("write seed");
        }
    }
    for (name, bytes) in &expected {
        let seed = std::fs::read(root.join(name)).expect("read seed");
        assert!(
            &seed == bytes,
            "seed {name} differs; regenerate with UPDATE_GOLDEN_FIXTURES=1"
        );
    }
}
