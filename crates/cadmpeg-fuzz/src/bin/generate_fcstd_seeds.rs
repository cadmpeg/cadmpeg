// SPDX-License-Identifier: Apache-2.0
//! Generate focused FCStd fuzz seeds from the public CC0 corpus.

use std::fs;
use std::io::{Cursor, Read};
use std::path::{Path, PathBuf};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    for target in [
        "fcstd_container",
        "fcstd_decode",
        "fcstd_xml",
        "fcstd_gui",
        "fcstd_brep",
        "fcstd_element_map",
        "fcstd_auxiliary",
    ] {
        let directory = PathBuf::from("seeds").join(target);
        if directory.exists() {
            fs::remove_dir_all(directory)?;
        }
    }
    let fixture = fs::read("../../corpus/freecad_fcstd/fixtures/core_design_product.FCStd")?;
    write("fcstd_container", "core_design_product.FCStd", &fixture)?;
    write("fcstd_decode", "core_design_product.FCStd", &fixture)?;

    let document = archive_entry(&fixture, "Document.xml")?;
    write("fcstd_xml", "core_document.xml", &document)?;
    write(
        "fcstd_gui",
        "view_provider.xml",
        br#"<GuiDocument SchemaVersion="1"><ViewProviderData Count="1"><ViewProvider name="Box" expanded="1"><Property name="Visibility" type="App::PropertyBool"><Bool value="true"/></Property></ViewProvider></ViewProviderData></GuiDocument>"#,
    )?;
    write(
        "fcstd_brep",
        "text_shape.brp",
        &archive_entry(&fixture, "Box.Shape.brp")?,
    )?;
    write(
        "fcstd_element_map",
        "persistent_names.map",
        &archive_entry(&fixture, "Cut.Shape.Map.txt")?,
    )?;
    write(
        "fcstd_auxiliary",
        "embedded_payload.bin",
        &[1, 0, 0, 0, 2, 0, 0, 0, 3, 0, 0, 0, 4, 0, 0, 0],
    )?;
    Ok(())
}

fn archive_entry(bytes: &[u8], name: &str) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes))?;
    let mut entry = archive.by_name(name)?;
    let mut output = Vec::new();
    entry.read_to_end(&mut output)?;
    Ok(output)
}

fn write(target: &str, name: &str, bytes: &[u8]) -> std::io::Result<()> {
    let directory = PathBuf::from("seeds").join(target);
    fs::create_dir_all(&directory)?;
    fs::write(Path::new(&directory).join(name), bytes)
}
