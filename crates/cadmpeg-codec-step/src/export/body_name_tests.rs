use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeOptions};

use crate::export::write_step;
use crate::{StepCodec, StepSchema, StepWriteOptions};

fn written(ir: &cadmpeg_ir::document::CadIr) -> (String, cadmpeg_ir::report::export::ExportReport) {
    let mut output = Vec::new();
    let report = write_step(
        ir,
        &mut output,
        StepSchema::Ap242Edition3,
        &StepWriteOptions::default(),
    )
    .expect("write the cube");
    (String::from_utf8(output).unwrap(), report)
}

fn read_back(text: &str) -> cadmpeg_ir::document::CadIr {
    StepCodec::default()
        .decode(&mut Cursor::new(text.as_bytes()), &DecodeOptions::default())
        .expect("decode the written file")
        .into_parts()
        .0
}

#[test]
fn a_body_name_is_the_name_of_its_solid() {
    let mut ir = cadmpeg_ir::examples::unit_cube().unwrap();
    ir.geometry.bodies[0].name = Some("Bracket".to_owned());
    let (text, report) = written(&ir);
    assert!(text.contains("MANIFOLD_SOLID_BREP('Bracket',"), "{text}");
    assert!(
        report
            .losses
            .iter()
            .all(|loss| !loss.message.contains("Bracket")),
        "{:?}",
        report.losses
    );
    let decoded = read_back(&text);
    assert_eq!(decoded.geometry.bodies.len(), 1);
    assert_eq!(decoded.geometry.bodies[0].name.as_deref(), Some("Bracket"));
}

#[test]
fn a_solid_with_an_empty_name_reads_as_an_unnamed_body() {
    let mut ir = cadmpeg_ir::examples::unit_cube().unwrap();
    ir.geometry.bodies[0].name = None;
    let (text, _) = written(&ir);
    assert!(text.contains("MANIFOLD_SOLID_BREP('',"), "{text}");
    assert_eq!(read_back(&text).geometry.bodies[0].name, None);
}
