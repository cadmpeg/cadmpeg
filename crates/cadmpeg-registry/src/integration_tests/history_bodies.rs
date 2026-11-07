// SPDX-License-Identifier: Apache-2.0
//! No encoder reads a history body: a document that holds one exports the
//! bytes and the report of the same document without it.

use cadmpeg_ir::codec::write::target::TargetRequest;
use cadmpeg_ir::codec::write::EncodeInput;
use cadmpeg_ir::CadIr;

use crate::{build_encoder, Format};

fn export(format: Format, ir: &CadIr) -> Result<(Vec<u8>, String), String> {
    let plan = build_encoder(format)
        .plan(EncodeInput::new(ir, None), TargetRequest::Inherit)
        .map_err(|error| error.to_string())?;
    let mut bytes = Vec::new();
    let report = plan
        .write_to(&mut bytes)
        .map_err(|error| error.to_string())?;
    Ok((bytes, format!("{:?}", report.losses)))
}

#[test]
fn every_encoder_exports_a_document_with_history_bodies_as_one_without() {
    let plain = cadmpeg_ir::examples::unit_cube().expect("fixture");
    let nested = cadmpeg_ir::examples::unit_cube_with_history_body().expect("fixture");
    assert_eq!(nested.model.history_bodies.len(), 1);
    assert!(plain.model.history_bodies.is_empty());
    let mut written = Vec::new();
    for format in Format::all() {
        // CADIR serializes the whole document, history bodies included.
        if build_encoder(format).id().as_str() == "cadir" {
            continue;
        }
        let baseline = export(format, &plain);
        let again = export(format, &plain);
        let with = export(format, &nested);
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
    assert!(
        written.len() >= 3,
        "too few deterministic writers compared: {written:?}"
    );
}

#[test]
fn a_nest_does_not_reach_the_census_of_a_document_without_one() {
    let plain = cadmpeg_ir::examples::unit_cube().expect("fixture");
    assert!(!plain.census().contains_key("history_bodies"));
    let nested = cadmpeg_ir::examples::unit_cube_with_history_body().expect("fixture");
    assert_eq!(nested.census()["history_bodies"], 1);
    assert_eq!(nested.census()["bodies"], plain.census()["bodies"]);
    assert_eq!(nested.census()["faces"], plain.census()["faces"]);
}
