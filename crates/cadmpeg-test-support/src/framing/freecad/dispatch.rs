// SPDX-License-Identifier: Apache-2.0
//! Exact tag and attribute source regions that dispatch an XML part grammar.

use cadmpeg_core::decode::{u64_from_index, DecodeContext, View, XmlElement};
use std::ops::Range;

pub(super) fn ranges(
    ctx: &DecodeContext<'_>,
    element: XmlElement<'_>,
    row: &str,
) -> Result<Vec<Range<u64>>, String> {
    let (tag, fields): (bool, &[&[u8]]) = match row {
        "freecad.document" => (
            false,
            &[
                b"SchemaVersion",
                b"FileVersion",
                b"ProgramVersion",
                b"StringHasher",
            ],
        ),
        "freecad.gui_document" => (false, &[b"SchemaVersion"]),
        "freecad.string_hasher" => (true, &[b"new", b"file", b"count", b"saveall", b"threshold"]),
        "freecad.objects" => (true, &[b"Dependencies"]),
        "freecad.object_declaration" | "freecad.extension" | "freecad.gui_extension" => {
            (false, &[b"type", b"name"])
        }
        "freecad.object_data_section" => (true, &[]),
        "freecad.object_data" | "freecad.view_provider" => (false, &[b"name"]),
        "freecad.property" => (true, &[b"type"]),
        "freecad.gui_property" => (false, &[b"type", b"name"]),
        "freecad.property_value" | "freecad.gui_property_value" => (true, &[b"file"]),
        "freecad.value_element" | "freecad.gui_value_element" => (true, &[b"type"]),
        "freecad.gui_state" => (true, &[b"settings"]),
        "freecad.gui_state_element" => (true, &[]),
        _ => (false, &[]),
    };
    let mut views = attributes(ctx, element, fields)?;
    if tag {
        admit(ctx, &mut views)?;
        views.push(element.name());
    }
    let mut ranges = views
        .into_iter()
        .map(|view| u64_from_index(view.start())..u64_from_index(view.end()))
        .collect::<Vec<_>>();
    ranges.sort_by_key(|range| range.start);
    Ok(ranges)
}

pub(super) fn attributes<'a>(
    ctx: &DecodeContext<'_>,
    element: XmlElement<'a>,
    selected: &[&[u8]],
) -> Result<Vec<View<'a>>, String> {
    let source = element.extent();
    let bytes = source.window();
    let name = element.name();
    let mut at = name
        .end()
        .checked_sub(source.start())
        .ok_or("XML attribute offset precedes its element")?;
    let mut values = Vec::new();
    while let Some(byte) = bytes.get(at) {
        if byte.is_ascii_whitespace() {
            at += 1;
            continue;
        }
        if matches!(*byte, b'>' | b'/') {
            break;
        }
        let start = at;
        while bytes
            .get(at)
            .is_some_and(|byte| !byte.is_ascii_whitespace() && !matches!(*byte, b'=' | b'>' | b'/'))
        {
            at += 1;
        }
        let end = at;
        while bytes.get(at).is_some_and(u8::is_ascii_whitespace) {
            at += 1;
        }
        if bytes.get(at) != Some(&b'=') {
            break;
        }
        at += 1;
        while bytes.get(at).is_some_and(u8::is_ascii_whitespace) {
            at += 1;
        }
        let Some(quote @ (b'\'' | b'"')) = bytes.get(at).copied() else {
            break;
        };
        at += 1;
        while bytes.get(at).is_some_and(|byte| *byte != quote) {
            at += 1;
        }
        if bytes.get(at) != Some(&quote) {
            break;
        }
        at += 1;
        if selected.contains(&&bytes[start..end]) {
            let absolute_start = source
                .start()
                .checked_add(start)
                .ok_or("XML attribute start overflows")?;
            let absolute_end = source
                .start()
                .checked_add(at)
                .ok_or("XML attribute end overflows")?;
            admit(ctx, &mut values)?;
            values.push(
                source
                    .child(absolute_start, absolute_end)
                    .ok_or("XML attribute escapes its element")?,
            );
        }
    }
    Ok(values)
}

fn admit(ctx: &DecodeContext<'_>, values: &mut Vec<View<'_>>) -> Result<(), String> {
    ctx.retain_bytes(
        u64_from_index(size_of::<View<'_>>() + size_of::<Range<u64>>()),
        "FreeCAD XML dispatch",
    )
    .map_err(|error| error.to_string())?;
    values.try_reserve(1).map_err(|error| error.to_string())
}
