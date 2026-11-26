// SPDX-License-Identifier: Apache-2.0
//! No encoder reads a history body: a document that holds one exports the
//! bytes and the report of the same document without it.

use cadmpeg_ir::codec::write::EncodeInput;
use cadmpeg_ir::CadIr;

use crate::{build_encoder, Format};

fn export(format: Format, ir: &CadIr) -> Result<(Vec<u8>, String), String> {
    let plan = build_encoder(format)
        .plan(EncodeInput::new(ir, None))
        .map_err(|error| error.to_string())?;
    let mut bytes = Vec::new();
    let report = plan
        .write_to(&mut bytes)
        .map_err(|error| error.to_string())?;
    Ok((bytes, format!("{:?}", report.losses)))
}

#[test]
fn every_encoder_exports_a_document_with_history_bodies_as_one_without() {
    let nested = cadmpeg_ir::examples::unit_cube_with_history_body().expect("fixture");
    assert_eq!(nested.model.history_bodies.len(), 1);
    let mut plain = nested.clone();
    plain.model.history_bodies.clear();
    let mut written = Vec::new();
    let mut deterministic = 0;
    for format in Format::all() {
        // CADIR serializes the whole document, history bodies included.
        if build_encoder(format).id().as_str() == "cadir" {
            continue;
        }
        // The nested export runs between the two plain exports, so a stamp
        // that both plain exports share also stamps the nested one.
        let baseline = export(format, &plain);
        let with = export(format, &nested);
        let again = export(format, &plain);
        if baseline.is_ok() && baseline == again {
            deterministic += 1;
        }
        match (&baseline, &again, &with) {
            (Ok(base), Ok(repeat), Ok(found)) => {
                // A writer that stamps its output varies between two runs of
                // the same document; its report must still agree.
                assert_eq!(base.1, found.1, "{format:?} losses");
                if base == repeat {
                    assert_eq!(base.0, found.0, "{format:?} bytes");
                    written.push(format);
                }
            }
            _ => assert_eq!(baseline, with, "{format:?}"),
        }
    }
    assert_eq!(
        written.len(),
        deterministic,
        "all deterministic writers must be compared"
    );
    let expected = [
        #[cfg(feature = "step")]
        Format::Step,
        #[cfg(feature = "f3d")]
        Format::F3d,
        #[cfg(feature = "usd")]
        Format::Usd,
    ];
    for format in expected {
        assert!(
            written.contains(&format),
            "deterministic writer {format:?} was not compared: {written:?}"
        );
    }
}

#[test]
fn a_nest_does_not_reach_the_census_of_a_document_without_one() {
    let nested = cadmpeg_ir::examples::unit_cube_with_history_body().expect("fixture");
    let mut plain = nested.clone();
    plain.model.history_bodies.clear();
    assert!(!plain.census().contains_key("history_bodies"));
    assert_eq!(nested.census()["history_bodies"], 1);
    assert_eq!(nested.census()["bodies"], plain.census()["bodies"]);
    assert_eq!(nested.census()["faces"], plain.census()["faces"]);
}
