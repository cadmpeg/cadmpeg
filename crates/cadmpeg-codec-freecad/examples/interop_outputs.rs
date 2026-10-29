// SPDX-License-Identifier: Apache-2.0
//! Write native regression documents and checked leaf-value edits.

use std::fs::{self, File};
use std::path::Path;

use cadmpeg_codec_freecad::{
    FcstdCodec, FcstdDocumentBuilder, FcstdPropertyOwner, FcstdPropertyValue,
};
use cadmpeg_ir::codec::write::target::TargetRequest;
use cadmpeg_ir::codec::write::{EncodeInput, Encoder};
use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::CadIr;

fn write(ir: &CadIr, output: &Path) -> Result<(), Box<dyn std::error::Error>> {
    FcstdCodec
        .plan(EncodeInput::new(ir, None), TargetRequest::Inherit)?
        .write_to(&mut File::create(output)?)?;
    Ok(())
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut arguments = std::env::args_os().skip(1);
    let input = arguments.next().ok_or("missing input directory")?;
    let output = arguments.next().ok_or("missing output directory")?;
    if arguments.next().is_some() {
        return Err("usage: interop_outputs INPUT_DIRECTORY OUTPUT_DIRECTORY".into());
    }
    let output = Path::new(&output);
    fs::create_dir_all(output)?;
    let input = Path::new(&input);
    let mut paths = fs::read_dir(input)?
        .map(|entry| entry.map(|entry| entry.path()))
        .collect::<Result<Vec<_>, _>>()?;
    paths.sort();
    for path in paths {
        if path
            .extension()
            .is_none_or(|extension| extension != "FCStd")
        {
            continue;
        }
        let decoded = FcstdCodec.decode(&mut File::open(&path)?, &DecodeOptions::default())?;
        let name = path.file_name().ok_or("missing input filename")?;
        write(decoded.ir(), &output.join(name))?;
        let edit = match path.file_stem().and_then(|stem| stem.to_str()) {
            Some("parametric_box") => Some(("Box", "Length", "15")),
            Some("restoration_two_boxes") => Some(("ZFirst", "Label", "Edited first box")),
            _ => None,
        };
        if let Some((owner, property, value)) = edit {
            let mut edited = decoded.ir().clone();
            FcstdCodec.set_property_value_attribute(
                &mut edited,
                FcstdPropertyOwner::Object(owner),
                property,
                0,
                "value",
                value,
            )?;
            let stem = path
                .file_stem()
                .ok_or("missing input stem")?
                .to_string_lossy();
            write(&edited, &output.join(format!("{stem}_edit.FCStd")))?;
        }
    }
    let mut document = FcstdDocumentBuilder::new("Source-less shape restoration");
    for name in ["ZFirst", "ASecond"] {
        document.add_object(name, "Part::Feature")?.add_property(
            name,
            "Shape",
            "Part::PropertyPartShape",
            vec![FcstdPropertyValue::attribute(
                "Part",
                "file",
                format!("{name}.brp"),
            )],
        )?;
    }
    for name in ["ASecond", "ZFirst"] {
        document.add_side_entry(
            format!("{name}.brp"),
            fs::read(input.join(format!("{name}.brp")))?,
        )?;
    }
    write(
        &document.build()?,
        &output
            .parent()
            .ok_or("missing output parent")?
            .join("source_less_shapes.FCStd"),
    )?;
    Ok(())
}
