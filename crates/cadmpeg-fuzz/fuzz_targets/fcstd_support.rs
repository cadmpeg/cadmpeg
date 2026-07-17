// SPDX-License-Identifier: Apache-2.0
#![allow(dead_code)] // Each focused harness uses a different subset.

use std::io::{Cursor, Write};

use zip::write::SimpleFileOptions;

pub const MAX_MUTATION_BYTES: usize = 1 << 20;

pub fn archive(entries: &[(&str, &[u8])]) -> Vec<u8> {
    let mut cursor = Cursor::new(Vec::new());
    let mut writer = zip::ZipWriter::new(&mut cursor);
    for (name, data) in entries {
        if writer
            .start_file(*name, SimpleFileOptions::default())
            .is_err()
            || writer.write_all(data).is_err()
        {
            return Vec::new();
        }
    }
    if writer.finish().is_err() {
        return Vec::new();
    }
    cursor.into_inner()
}

pub fn bounded(data: &[u8]) -> &[u8] {
    &data[..data.len().min(MAX_MUTATION_BYTES)]
}

pub const EMPTY_DOCUMENT: &[u8] = br#"<Document SchemaVersion="4" FileVersion="1"><Objects Count="0"/><ObjectData Count="0"/></Document>"#;

pub fn shape_document(entry: &str) -> Vec<u8> {
    format!(r#"<Document SchemaVersion="4" FileVersion="1"><Objects Count="1"><Object type="Part::Feature" name="Shape" id="1"/></Objects><ObjectData Count="1"><Object name="Shape"><Properties Count="1"><Property name="Shape" type="Part::PropertyPartShape"><Part file="{entry}"/></Property></Properties></Object></ObjectData></Document>"#).into_bytes()
}

pub fn decode(bytes: Vec<u8>) {
    use cadmpeg_codec_freecad::FcstdCodec;
    use cadmpeg_ir::codec::{Codec, DecodeOptions};

    let _ = FcstdCodec.decode(&mut Cursor::new(bytes), &DecodeOptions::default());
}
