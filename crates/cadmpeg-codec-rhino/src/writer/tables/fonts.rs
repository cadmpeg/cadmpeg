// SPDX-License-Identifier: Apache-2.0
//! Text styles authored from typed font characteristics.

use super::{
    anonymous_with_children, class_wrapper_with_direct, component, crc_chunk,
    crc_chunk_with_direct, integer, native_count_limit, records, utf16, uuid, wire_index, CadIr,
    CodecError, Map, Value,
};

const TEXT_STYLE_CLASS: [u8; 16] = [
    0xfb, 0x51, 0x0f, 0x4f, 0xd0, 0x35, 0x65, 0x48, 0x99, 0x98, 0x6d, 0x2c, 0x6a, 0x99, 0x72, 0x1d,
];

fn text<'a>(fields: &'a Map<String, Value>, name: &str) -> &'a str {
    fields.get(name).and_then(Value::as_str).unwrap_or("")
}

fn font(fields: &Map<String, Value>) -> Result<Vec<u8>, CodecError> {
    let mut payload = u32::try_from(
        fields
            .get("characteristics")
            .and_then(Value::as_u64)
            .unwrap_or(0),
    )
    .map_err(CodecError::malformed)?
    .to_le_bytes()
    .to_vec();
    let mut wide = vec![1];
    wide.extend(text(fields, "windows_logfont_name").as_bytes());
    let wide = crc_chunk(0x4000_8001, &wide)?;
    let wide_start = payload.len();
    payload.extend(wide);
    let wide_end = payload.len();
    payload.extend(utf16(text(fields, "postscript_name"))?);
    let has_weight = fields
        .get("windows_logfont_weight")
        .is_some_and(Value::is_number);
    if !has_weight && text(fields, "obsolete_description").is_empty() {
        return anonymous_with_children(
            1,
            0,
            &payload,
            std::slice::from_ref(&(wide_start..wide_end)),
        );
    }
    payload.extend(utf16(text(fields, "obsolete_description"))?);
    if !has_weight {
        return anonymous_with_children(
            1,
            1,
            &payload,
            std::slice::from_ref(&(wide_start..wide_end)),
        );
    }
    payload.extend(integer(Some(fields), "windows_logfont_weight", 0).to_le_bytes());
    payload.extend(
        fields
            .get("apple_weight_trait")
            .and_then(Value::as_f64)
            .unwrap_or(0.0)
            .to_le_bytes(),
    );
    let point_size = fields.get("point_size").and_then(Value::as_f64);
    if point_size.is_none() {
        return anonymous_with_children(
            1,
            2,
            &payload,
            std::slice::from_ref(&(wide_start..wide_end)),
        );
    }
    payload.extend(point_size.unwrap_or(0.0).to_le_bytes());
    payload.push(0); // No obsolete point-unit structure is retained in typed fonts.
    payload.extend(utf16(text(fields, "family_name"))?);
    let Some(panose) = fields.get("panose").and_then(Value::as_object) else {
        return anonymous_with_children(
            1,
            4,
            &payload,
            std::slice::from_ref(&(wide_start..wide_end)),
        );
    };
    for name in [
        "locale_name",
        "localized_postscript_name",
        "english_postscript_name",
        "localized_logfont_name",
        "english_logfont_name",
        "localized_family_name",
        "english_family_name",
        "localized_face_name",
        "english_face_name",
    ] {
        payload.extend(utf16(text(fields, name))?);
    }
    let mut traits = vec![0x10];
    for name in [
        "family_type",
        "serif_style",
        "weight",
        "proportion",
        "contrast",
        "stroke_variation",
        "arm_style",
        "letterform",
        "midline",
        "x_height",
    ] {
        traits.push(
            u8::try_from(panose.get(name).and_then(Value::as_u64).unwrap_or(0))
                .map_err(CodecError::malformed)?,
        );
    }
    let start = payload.len();
    payload.extend(crc_chunk(0x4000_8000, &traits)?);
    let end = payload.len();
    let quartet = fields.get("quartet_member").and_then(Value::as_u64);
    if let Some(value) = quartet {
        payload.push(u8::try_from(value).map_err(CodecError::malformed)?);
    }
    anonymous_with_children(
        1,
        if quartet.is_some() { 6 } else { 5 },
        &payload,
        &[wide_start..wide_end, start..end],
    )
}

fn legacy(
    fields: &Map<String, Value>,
    index: usize,
    identity: &str,
) -> Result<Vec<u8>, CodecError> {
    let font = fields
        .get("font")
        .and_then(Value::as_object)
        .ok_or_else(|| CodecError::malformed("Rhino text style has no typed font"))?;
    let mut payload = vec![0x12];
    payload.extend(wire_index(index)?.to_le_bytes());
    payload.extend(utf16(text(fields, "font_description"))?);
    let face = text(font, "windows_logfont_name")
        .encode_utf16()
        .collect::<Vec<_>>();
    if face.len() > 63 {
        // (O-capacity): a legacy LOGFONT face has 64 UTF-16 code units including NUL.
        return Err(native_count_limit(
            63,
            cadmpeg_core::decode::u64_from_index(face.len()),
        ));
    }
    for index in 0..64 {
        payload.extend(face.get(index).copied().unwrap_or(0).to_le_bytes());
    }
    payload.extend(integer(Some(font), "windows_logfont_weight", 0).to_le_bytes());
    payload.extend(
        i32::from(
            font.get("legacy_italic")
                .and_then(Value::as_bool)
                .unwrap_or(false),
        )
        .to_le_bytes(),
    );
    payload.extend(
        font.get("legacy_linefeed_ratio")
            .and_then(Value::as_f64)
            .unwrap_or(1.0)
            .to_le_bytes(),
    );
    payload.extend(uuid(fields.get("source_uuid"), identity));
    Ok(payload)
}

pub(in crate::writer) fn styles(ir: &CadIr) -> Result<Vec<Vec<u8>>, CodecError> {
    let mut output = Vec::new();
    for (index, record) in records(ir, "text_styles").iter().enumerate() {
        let fields = record.fields()?;
        let font_fields = fields
            .get("font")
            .and_then(Value::as_object)
            .ok_or_else(|| CodecError::malformed("Rhino text style has no typed font"))?;
        let (payload, direct) = if font_fields
            .get("legacy_italic")
            .and_then(Value::as_bool)
            .is_some()
        {
            let payload = legacy(&fields, index, record.id())?;
            let direct = payload.clone();
            (payload, direct)
        } else {
            let mut payload = component(
                record.id(),
                wire_index(index)?,
                text(&fields, "name"),
                fields.get("source_uuid"),
            )?;
            let component_end = payload.len();
            payload.push(1);
            payload.extend(utf16(text(&fields, "font_description"))?);
            payload.push(1);
            let font_start = payload.len();
            payload.extend(font(font_fields)?);
            let font_end = payload.len();
            payload.extend(uuid(fields.get("source_uuid"), record.id()));
            payload.extend(utf16(text(&fields, "name"))?);
            (
                anonymous_with_children(1, 1, &payload, &[0..component_end, font_start..font_end])?,
                Vec::new(),
            )
        };
        output.push(crc_chunk_with_direct(
            0x2000_8074,
            &class_wrapper_with_direct(TEXT_STYLE_CLASS, &payload, &direct)?,
            &[],
        )?);
    }
    Ok(output)
}

pub(in crate::writer) fn complete(record: &cadmpeg_ir::NativeRecord) -> bool {
    let Ok(fields) = record.fields() else {
        return false;
    };
    let Some(font) = fields.get("font").and_then(Value::as_object) else {
        return false;
    };
    if font.get("legacy_italic").and_then(Value::as_bool).is_some() {
        return text(&fields, "name") == text(&fields, "font_description");
    }
    let localized = [
        "locale_name",
        "localized_postscript_name",
        "english_postscript_name",
        "localized_logfont_name",
        "english_logfont_name",
        "localized_family_name",
        "english_family_name",
        "localized_face_name",
        "english_face_name",
    ];
    let no_later_values = localized.iter().all(|name| text(font, name).is_empty())
        && font.get("quartet_member").is_none_or(Value::is_null);
    if !font.get("point_size").is_some_and(Value::is_number) {
        return text(font, "family_name").is_empty()
            && no_later_values
            && font.get("panose").is_none_or(Value::is_null);
    }
    font.get("panose").is_some_and(Value::is_object) || no_later_values
}

#[cfg(test)]
mod tests;
