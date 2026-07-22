// SPDX-License-Identifier: Apache-2.0
//! Generate a small parametric `FCStd` document without a source archive.

use std::fs::File;

use cadmpeg_codec_freecad::{FcstdCodec, FcstdDocumentBuilder, FcstdPropertyValue};
use cadmpeg_ir::Encoder;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let output = std::env::args_os()
        .nth(1)
        .ok_or("usage: source_less OUTPUT.FCStd")?;
    let mut document = FcstdDocumentBuilder::new("cadmpeg source-less example");
    document
        .add_object("Box", "Part::Box")?
        .add_property(
            "Box",
            "Label",
            "App::PropertyString",
            vec![FcstdPropertyValue::attribute("String", "value", "Box")],
        )?
        .add_property(
            "Box",
            "Length",
            "App::PropertyLength",
            vec![FcstdPropertyValue::attribute("Float", "value", "12.5")],
        )?
        .add_property(
            "Box",
            "Width",
            "App::PropertyLength",
            vec![FcstdPropertyValue::attribute("Float", "value", "7")],
        )?
        .add_property(
            "Box",
            "Height",
            "App::PropertyLength",
            vec![FcstdPropertyValue::attribute("Float", "value", "3")],
        )?;
    let ir = document.build()?;
    FcstdCodec.encode(&ir, &mut File::create(output)?)?;
    Ok(())
}
