use super::*;

pub(crate) fn new_general_note_file_with_character_metrics(
    character_width: &str,
    character_height: &str,
) -> Vec<u8> {
    new_general_note_file_with_fields("0", character_width, character_height, "", "0")
}

pub(crate) fn new_general_note_file_with_fields(
    fixed_or_variable: &str,
    character_width: &str,
    character_height: &str,
    character_spacing: &str,
    character_count: &str,
) -> Vec<u8> {
    new_general_note_file_with_font(
        fixed_or_variable,
        character_width,
        character_height,
        character_spacing,
        character_count,
        "1",
    )
}

pub(crate) fn new_general_note_file_with_font(
    fixed_or_variable: &str,
    character_width: &str,
    character_height: &str,
    character_spacing: &str,
    character_count: &str,
    font_style: &str,
) -> Vec<u8> {
    new_general_note_file_with_font_and_character_set(
        fixed_or_variable,
        character_width,
        character_height,
        character_spacing,
        character_count,
        font_style,
        "",
    )
}

pub(crate) fn new_general_note_file_with_font_and_character_set(
    fixed_or_variable: &str,
    character_width: &str,
    character_height: &str,
    character_spacing: &str,
    character_count: &str,
    font_style: &str,
    character_set: &str,
) -> Vec<u8> {
    owned_test_file(&[OwnedTestEntity {
        entity_type: 213,
        form: 0,
        label: "DEFAULTS".into(),
        status: "00000100",
        parameters: new_general_note_parameters(
            fixed_or_variable,
            character_width,
            character_height,
            character_spacing,
            character_count,
            font_style,
            character_set,
        ) + ";",
    }])
}

pub(crate) fn new_general_note_parameters(
    fixed_or_variable: &str,
    character_width: &str,
    character_height: &str,
    character_spacing: &str,
    character_count: &str,
    font_style: &str,
    character_set: &str,
) -> String {
    let mut fields = vec![String::from("213")];
    fields.extend((0..11).map(|_| String::new()));
    fields.push(String::from("1"));
    fields.extend(
        [
            fixed_or_variable,
            character_width,
            character_height,
            character_spacing,
            "", // LSPACE
            font_style,
            "", // CHRANG
            "", // CCTEXT
            character_count,
            "", // WT
            "", // HT
            character_set,
            "", // SL
            "", // A
            "", // M
            "", // VH
            "", // XS
            "", // YS
            "", // ZS
            "", // TEXT
        ]
        .into_iter()
        .map(str::to_owned),
    );
    fields.join(",")
}
