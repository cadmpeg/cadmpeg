// SPDX-License-Identifier: Apache-2.0
pub struct DecodeContext;
impl DecodeContext {
    pub fn try_reserve_retained_text(
        &self,
        _text: &mut String,
        _count: usize,
        _operation: &str,
    ) -> Result<(), ()> {
        Ok(())
    }
    pub fn charge_work(&self, _count: u64, _operation: &str) -> Result<(), ()> {
        Ok(())
    }
}
pub fn admitted(ctx: &DecodeContext, value: &mut String, suffix: &str) -> Result<(), ()> {
    ctx.try_reserve_retained_text(value, suffix.len(), "text")?;
    ctx.charge_work(suffix.len() as u64, "text")?;
    value.push_str(suffix);
    Ok(())
}
pub fn wrong_target(
    ctx: &DecodeContext,
    value: &mut String,
    other: &mut String,
    suffix: &str,
) -> Result<(), ()> {
    ctx.try_reserve_retained_text(value, suffix.len(), "text")?;
    ctx.charge_work(suffix.len() as u64, "text")?;
    other.push_str(suffix); // finding: unproven_decode_charge
    Ok(())
}
pub fn wrong_count(ctx: &DecodeContext, value: &mut String, suffix: &str) -> Result<(), ()> {
    ctx.try_reserve_retained_text(value, 1, "text")?;
    ctx.charge_work(suffix.len() as u64, "text")?;
    value.push_str(suffix); // finding: unproven_decode_charge
    Ok(())
}
pub fn reused(ctx: &DecodeContext, value: &mut String, suffix: &str) -> Result<(), ()> {
    ctx.try_reserve_retained_text(value, suffix.len(), "text")?;
    ctx.charge_work(suffix.len() as u64, "text")?;
    value.push_str(suffix);
    ctx.charge_work(suffix.len() as u64, "text")?;
    value.push_str(suffix); // finding: unproven_decode_charge
    Ok(())
}

pub fn admitted_char(
    ctx: &DecodeContext,
    output: &mut String,
    character: char,
) -> Result<(), ()> {
    ctx.charge_work(character.len_utf8() as u64, "retained char")?;
    ctx.try_reserve_retained_text(output, character.len_utf8(), "retained char")?;
    output.push(character);
    Ok(())
}

pub fn admitted_char_alias(
    ctx: &DecodeContext,
    output: &mut String,
    character: char,
) -> Result<(), ()> {
    let alias = character;
    ctx.charge_work(character.len_utf8() as u64, "retained char")?;
    ctx.try_reserve_retained_text(output, alias.len_utf8(), "retained char")?;
    output.push(alias);
    Ok(())
}

pub fn admitted_ascii_char(
    ctx: &DecodeContext,
    output: &mut String,
    choose: bool,
) -> Result<(), ()> {
    let character = if choose { 'a' } else { 'z' };
    ctx.charge_work(character.len_utf8() as u64, "retained char")?;
    ctx.try_reserve_retained_text(output, character.len_utf8(), "retained char")?;
    output.push(character);
    Ok(())
}

pub fn admitted_two_byte_char(
    ctx: &DecodeContext,
    output: &mut String,
    choose: bool,
) -> Result<(), ()> {
    let character = if choose { 'é' } else { 'ö' };
    ctx.charge_work(character.len_utf8() as u64, "retained char")?;
    ctx.try_reserve_retained_text(output, character.len_utf8(), "retained char")?;
    output.push(character);
    Ok(())
}

pub fn admitted_four_byte_char(
    ctx: &DecodeContext,
    output: &mut String,
    choose: bool,
) -> Result<(), ()> {
    let character = if choose { '🦀' } else { '𝅘𝅥𝅮' };
    ctx.charge_work(character.len_utf8() as u64, "retained char")?;
    ctx.try_reserve_retained_text(output, character.len_utf8(), "retained char")?;
    output.push(character);
    Ok(())
}

pub fn admitted_checked_char(
    ctx: &DecodeContext,
    output: &mut String,
    codepoint: u32,
) -> Result<(), ()> {
    let character = char::from_u32(codepoint).ok_or(())?;
    ctx.charge_work(character.len_utf8() as u64, "retained char")?;
    ctx.try_reserve_retained_text(output, character.len_utf8(), "retained char")?;
    output.push(character);
    Ok(())
}

pub fn missing_char_work(
    ctx: &DecodeContext,
    output: &mut String,
    character: char,
) -> Result<(), ()> {
    ctx.try_reserve_retained_text(output, character.len_utf8(), "retained char")?;
    output.push(character); // finding: uncharged_decode_work
    Ok(())
}

pub fn one_byte_storage(
    ctx: &DecodeContext,
    output: &mut String,
    choose: bool,
) -> Result<(), ()> {
    let character = if choose { 'é' } else { 'ö' };
    ctx.charge_work(character.len_utf8() as u64, "retained char")?;
    ctx.try_reserve_retained_text(output, 1, "retained char")?;
    output.push(character); // finding: unproven_decode_charge
    Ok(())
}

pub fn different_same_width_character(
    ctx: &DecodeContext,
    output: &mut String,
    choose: bool,
) -> Result<(), ()> {
    let charged = if choose { 'é' } else { 'ö' };
    let appended = if choose { 'ö' } else { 'é' };
    ctx.charge_work(charged.len_utf8() as u64, "retained char")?;
    ctx.try_reserve_retained_text(output, charged.len_utf8(), "retained char")?;
    output.push(appended); // finding: unproven_decode_charge
    Ok(())
}

pub fn wrong_output(
    ctx: &DecodeContext,
    reserved: &mut String,
    output: &mut String,
    character: char,
) -> Result<(), ()> {
    ctx.charge_work(character.len_utf8() as u64, "retained char")?;
    ctx.try_reserve_retained_text(reserved, character.len_utf8(), "retained char")?;
    output.push(character); // finding: unproven_decode_charge
    Ok(())
}

pub fn reused_char_receipts(
    ctx: &DecodeContext,
    output: &mut String,
    character: char,
) -> Result<(), ()> {
    ctx.charge_work(character.len_utf8() as u64, "retained char")?;
    ctx.try_reserve_retained_text(output, character.len_utf8(), "retained char")?;
    output.push(character);
    output.push(character); // finding: unproven_decode_charge, uncharged_decode_work
    Ok(())
}

pub fn stale_character(
    ctx: &DecodeContext,
    output: &mut String,
    input: char,
) -> Result<(), ()> {
    let mut character = input;
    ctx.charge_work(character.len_utf8() as u64, "retained char")?;
    ctx.try_reserve_retained_text(output, character.len_utf8(), "retained char")?;
    character = 'x';
    output.push(character); // finding: unproven_decode_charge, uncharged_decode_work
    Ok(())
}

pub fn stale_literal_character(
    ctx: &DecodeContext,
    output: &mut String,
) -> Result<(), ()> {
    let mut character = 'é';
    ctx.charge_work(character.len_utf8() as u64, "retained char")?;
    ctx.try_reserve_retained_text(output, character.len_utf8(), "retained char")?;
    character = 'ö';
    output.push(character); // finding: unproven_decode_charge, uncharged_decode_work
    Ok(())
}

pub struct CharacterField {
    pub value: char,
}

pub fn stale_character_field(
    ctx: &DecodeContext,
    output: &mut String,
    input: &mut CharacterField,
) -> Result<(), ()> {
    ctx.charge_work(input.value.len_utf8() as u64, "retained char")?;
    ctx.try_reserve_retained_text(output, input.value.len_utf8(), "retained char")?;
    input.value = 'x';
    output.push(input.value); // finding: unproven_decode_charge, uncharged_decode_work
    Ok(())
}

pub fn stale_box_character_field(
    ctx: &DecodeContext,
    output: &mut String,
    input: &mut Box<CharacterField>,
) -> Result<(), ()> {
    ctx.charge_work(input.value.len_utf8() as u64, "retained char")?;
    ctx.try_reserve_retained_text(output, input.value.len_utf8(), "retained char")?;
    input.value = 'x';
    output.push(input.value); // finding: unproven_decode_charge, uncharged_decode_work
    Ok(())
}

static ALTERNATING_FIELD_TARGETS: [CharacterField; 2] = [
    CharacterField { value: 'é' },
    CharacterField { value: 'ö' },
];

pub struct AlternatingField(std::cell::Cell<usize>);

impl std::ops::Deref for AlternatingField {
    type Target = CharacterField;

    fn deref(&self) -> &Self::Target {
        let selected = self.0.get();
        self.0.set(1 - selected);
        &ALTERNATING_FIELD_TARGETS[selected]
    }
}

pub fn custom_deref_field_reads_are_unproven(
    ctx: &DecodeContext,
    output: &mut String,
) -> Result<(), ()> {
    let proxy = AlternatingField(std::cell::Cell::new(0));
    ctx.charge_work(proxy.value.len_utf8() as u64, "retained char")?;
    ctx.try_reserve_retained_text(output, proxy.value.len_utf8(), "retained char")?;
    output.push(proxy.value); // finding: unproven_decode_charge, uncharged_decode_work
    Ok(())
}

pub fn admitted_copied_custom_deref_field(
    ctx: &DecodeContext,
    output: &mut String,
) -> Result<(), ()> {
    let proxy = AlternatingField(std::cell::Cell::new(0));
    let character = proxy.value;
    ctx.charge_work(character.len_utf8() as u64, "retained char")?;
    ctx.try_reserve_retained_text(output, character.len_utf8(), "retained char")?;
    output.push(character);
    Ok(())
}

pub struct MutableFieldProxy {
    target: CharacterField,
}

impl std::ops::Deref for MutableFieldProxy {
    type Target = CharacterField;

    fn deref(&self) -> &Self::Target {
        &self.target
    }
}

impl std::ops::DerefMut for MutableFieldProxy {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.target
    }
}

pub fn custom_deref_mut_write_invalidates_receipts(
    ctx: &DecodeContext,
    output: &mut String,
    input: &mut MutableFieldProxy,
) -> Result<(), ()> {
    ctx.charge_work(input.target.value.len_utf8() as u64, "retained char")?;
    ctx.try_reserve_retained_text(output, input.target.value.len_utf8(), "retained char")?;
    input.value = 'x';
    output.push(input.target.value); // finding: uncharged_decode_work, unproven_decode_charge
    Ok(())
}

pub struct MutableIndex<'a>(&'a mut [char; 2]);

impl std::ops::Index<usize> for MutableIndex<'_> {
    type Output = char;

    fn index(&self, index: usize) -> &Self::Output {
        &self.0[index]
    }
}

impl std::ops::IndexMut<usize> for MutableIndex<'_> {
    fn index_mut(&mut self, index: usize) -> &mut Self::Output {
        &mut self.0[index]
    }
}

pub fn custom_index_mut_write_invalidates_receipts(
    ctx: &DecodeContext,
    output: &mut String,
) -> Result<(), ()> {
    let mut characters = ['é', 'ö'];
    ctx.charge_work(characters[0].len_utf8() as u64, "retained char")?;
    ctx.try_reserve_retained_text(output, characters[0].len_utf8(), "retained char")?;
    {
        let mut proxy = MutableIndex(&mut characters);
        proxy[0] = '🦀';
    }
    output.push(characters[0]); // finding: uncharged_decode_work, unproven_decode_charge
    Ok(())
}

pub fn raw_pointer_write_invalidates_receipts(
    ctx: &DecodeContext,
    output: &mut String,
) -> Result<(), ()> {
    let mut characters = ['é', 'ö'];
    let pointer = characters.as_mut_ptr(); // finding: unproven_decode_charge
    ctx.charge_work(characters[0].len_utf8() as u64, "retained char")?;
    ctx.try_reserve_retained_text(output, characters[0].len_utf8(), "retained char")?;
    unsafe {
        *pointer = '🦀';
    }
    output.push(characters[0]); // finding: unproven_decode_charge
    Ok(())
}

pub fn admitted_indexed_character(
    ctx: &DecodeContext,
    output: &mut String,
    characters: &[char],
    index: usize,
) -> Result<(), ()> {
    ctx.charge_work(characters[index].len_utf8() as u64, "retained char")?;
    ctx.try_reserve_retained_text(output, characters[index].len_utf8(), "retained char")?;
    output.push(characters[index]);
    Ok(())
}

pub fn admitted_nested_indexed_character(
    ctx: &DecodeContext,
    output: &mut String,
) -> Result<(), ()> {
    let rows = [[IndexedCharacter { value: 'é' }], [IndexedCharacter { value: 'ö' }]];
    ctx.charge_work(rows[0][0].value.len_utf8() as u64, "retained char")?;
    ctx.try_reserve_retained_text(output, rows[0][0].value.len_utf8(), "retained char")?;
    output.push(rows[0][0].value);
    Ok(())
}

pub fn different_nested_indexed_character(
    ctx: &DecodeContext,
    output: &mut String,
) -> Result<(), ()> {
    let rows = [[IndexedCharacter { value: 'é' }], [IndexedCharacter { value: 'ö' }]];
    ctx.charge_work(rows[0][0].value.len_utf8() as u64, "retained char")?;
    ctx.try_reserve_retained_text(output, rows[0][0].value.len_utf8(), "retained char")?;
    output.push(rows[1][0].value); // finding: unproven_decode_charge, uncharged_decode_work
    Ok(())
}

pub fn changed_nested_indexed_character(
    ctx: &DecodeContext,
    output: &mut String,
) -> Result<(), ()> {
    let mut rows = [[IndexedCharacter { value: 'é' }], [IndexedCharacter { value: 'ö' }]];
    ctx.charge_work(rows[0][0].value.len_utf8() as u64, "retained char")?;
    ctx.try_reserve_retained_text(output, rows[0][0].value.len_utf8(), "retained char")?;
    rows[0][0].value = 'x';
    output.push(rows[0][0].value); // finding: unproven_decode_charge, uncharged_decode_work
    Ok(())
}

pub struct IndexedCharacter {
    pub value: char,
}

pub fn different_index_different_width(
    ctx: &DecodeContext,
    output: &mut String,
) -> Result<(), ()> {
    let characters = ['a', '🦀'];
    ctx.charge_work(characters[0].len_utf8() as u64, "retained char")?;
    ctx.try_reserve_retained_text(output, characters[0].len_utf8(), "retained char")?;
    output.push(characters[1]); // finding: unproven_decode_charge, uncharged_decode_work
    Ok(())
}

pub fn different_index_same_width(
    ctx: &DecodeContext,
    output: &mut String,
) -> Result<(), ()> {
    let characters = ['é', 'ö'];
    ctx.charge_work(characters[0].len_utf8() as u64, "retained char")?;
    ctx.try_reserve_retained_text(output, characters[0].len_utf8(), "retained char")?;
    output.push(characters[1]); // finding: unproven_decode_charge, uncharged_decode_work
    Ok(())
}

pub fn changed_character_index(
    ctx: &DecodeContext,
    output: &mut String,
    characters: &[char],
) -> Result<(), ()> {
    let mut index = 0;
    ctx.charge_work(characters[index].len_utf8() as u64, "retained char")?;
    ctx.try_reserve_retained_text(output, characters[index].len_utf8(), "retained char")?;
    index = 1;
    output.push(characters[index]); // finding: unproven_decode_charge, uncharged_decode_work
    Ok(())
}

pub fn changed_indexed_character(
    ctx: &DecodeContext,
    output: &mut String,
) -> Result<(), ()> {
    let mut characters = ['é', 'ö'];
    ctx.charge_work(characters[0].len_utf8() as u64, "retained char")?;
    ctx.try_reserve_retained_text(output, characters[0].len_utf8(), "retained char")?;
    characters[0] = 'x';
    output.push(characters[0]); // finding: uncharged_decode_work, unproven_decode_charge
    Ok(())
}

pub fn copied_character_snapshot_after_source_change(
    ctx: &DecodeContext,
    output: &mut String,
) -> Result<(), ()> {
    let mut source = 'é';
    let snapshot = source;
    ctx.charge_work(snapshot.len_utf8() as u64, "retained char")?;
    ctx.try_reserve_retained_text(output, snapshot.len_utf8(), "retained char")?;
    source = 'x';
    output.push(snapshot); // finding: unproven_decode_charge, uncharged_decode_work
    let _source_after_change = source;
    Ok(())
}

static ALTERNATING_CHARACTERS: [char; 2] = ['é', 'ö'];

pub struct AlternatingCharacters(std::cell::Cell<usize>);

impl std::ops::Index<usize> for AlternatingCharacters {
    type Output = char;

    fn index(&self, _index: usize) -> &Self::Output {
        let selected = self.0.get();
        self.0.set(1 - selected);
        &ALTERNATING_CHARACTERS[selected]
    }
}

pub fn custom_index_reads_are_unproven(
    ctx: &DecodeContext,
    output: &mut String,
) -> Result<(), ()> {
    let characters = AlternatingCharacters(std::cell::Cell::new(0));
    ctx.charge_work(characters[0].len_utf8() as u64, "retained char")?;
    ctx.try_reserve_retained_text(output, characters[0].len_utf8(), "retained char")?;
    output.push(characters[0]); // finding: unproven_decode_charge
    Ok(())
}

pub fn admitted_custom_index_snapshot(
    ctx: &DecodeContext,
    output: &mut String,
) -> Result<(), ()> {
    let characters = AlternatingCharacters(std::cell::Cell::new(0));
    let character = characters[0];
    ctx.charge_work(character.len_utf8() as u64, "retained char")?;
    ctx.try_reserve_retained_text(output, character.len_utf8(), "retained char")?;
    output.push(character);
    Ok(())
}

pub fn admitted_fmt_char(
    ctx: &DecodeContext,
    output: &mut String,
    character: char,
) -> Result<(), ()> {
    use std::fmt::Write as _;

    ctx.charge_work(character.len_utf8() as u64, "retained char")?;
    ctx.try_reserve_retained_text(output, character.len_utf8(), "retained char")?;
    output.write_char(character).map_err(|_| ())?;
    Ok(())
}

pub fn fmt_char_understorage(
    ctx: &DecodeContext,
    output: &mut String,
    choose: bool,
) -> Result<(), ()> {
    use std::fmt::Write as _;

    let character = if choose { 'é' } else { 'ö' };
    ctx.charge_work(character.len_utf8() as u64, "retained char")?;
    ctx.try_reserve_retained_text(output, 1, "retained char")?;
    output.write_char(character).map_err(|_| ())?; // finding: unproven_decode_charge
    Ok(())
}

pub fn wrong_fmt_char_output(
    ctx: &DecodeContext,
    reserved: &mut String,
    output: &mut String,
    character: char,
) -> Result<(), ()> {
    use std::fmt::Write as _;

    ctx.charge_work(character.len_utf8() as u64, "retained char")?;
    ctx.try_reserve_retained_text(reserved, character.len_utf8(), "retained char")?;
    output.write_char(character).map_err(|_| ())?; // finding: unproven_decode_charge
    Ok(())
}

pub fn admitted_fmt_str(
    ctx: &DecodeContext,
    output: &mut String,
    suffix: &str,
) -> Result<(), ()> {
    use std::fmt::Write as _;

    ctx.charge_work(suffix.len() as u64, "text")?;
    ctx.try_reserve_retained_text(output, suffix.len(), "text")?;
    output.write_str(suffix).map_err(|_| ())?;
    Ok(())
}

pub fn wrong_fmt_str_output(
    ctx: &DecodeContext,
    reserved: &mut String,
    output: &mut String,
    suffix: &str,
) -> Result<(), ()> {
    use std::fmt::Write as _;

    ctx.charge_work(suffix.len() as u64, "text")?;
    ctx.try_reserve_retained_text(reserved, suffix.len(), "text")?;
    output.write_str(suffix).map_err(|_| ())?; // finding: unproven_decode_charge
    Ok(())
}

pub fn formatter_keeps_non_string_costs(
    ctx: &DecodeContext,
    formatter: &mut std::fmt::Formatter<'_>,
    suffix: &str,
) -> std::fmt::Result {
    use std::fmt::Write as _;

    ctx.charge_work(suffix.len() as u64, "formatter")
        .map_err(|_| std::fmt::Error)?;
    formatter.write_char('é')?;
    formatter.write_str(suffix)
}

pub fn insert_keeps_shift_work_separate(
    ctx: &DecodeContext,
    output: &mut String,
    choose: bool,
) -> Result<(), ()> {
    let character = if choose { 'é' } else { 'ö' };
    ctx.try_reserve_retained_text(output, character.len_utf8(), "retained char")?;
    output.insert(0, character); // finding: uncharged_decode_work
    Ok(())
}

pub fn insert_rejects_under_storage(
    ctx: &DecodeContext,
    output: &mut String,
    choose: bool,
) -> Result<(), ()> {
    let character = if choose { 'é' } else { 'ö' };
    ctx.try_reserve_retained_text(output, 1, "retained char")?;
    output.insert(0, character); // finding: unproven_decode_charge, uncharged_decode_work
    Ok(())
}

pub fn generic_write_char<W: std::fmt::Write>(
    _ctx: &DecodeContext,
    output: &mut W,
    character: char,
) -> std::fmt::Result {
    output.write_char(character)
}

pub fn generic_string_write_char(
    ctx: &DecodeContext,
    output: &mut String,
    character: char,
) -> std::fmt::Result {
    generic_write_char(ctx, output, character)?; // finding: uncharged_decode_allocation, uncharged_decode_work
    Ok(())
}

pub fn generic_write_str<W: std::fmt::Write>(
    _ctx: &DecodeContext,
    output: &mut W,
    suffix: &str,
) -> std::fmt::Result {
    output.write_str(suffix)
}

pub fn generic_string_write_str(
    ctx: &DecodeContext,
    output: &mut String,
    suffix: &str,
) -> std::fmt::Result {
    generic_write_str(ctx, output, suffix)?; // finding: uncharged_decode_allocation, uncharged_decode_work
    Ok(())
}

pub fn generic_admitted_char<T>(
    ctx: &DecodeContext,
    output: &mut String,
    _tag: &T,
    character: char,
) -> Result<(), ()> {
    use std::fmt::Write as _;

    ctx.charge_work(character.len_utf8() as u64, "retained char")?;
    ctx.try_reserve_retained_text(output, character.len_utf8(), "retained char")?;
    output.write_char(character).map_err(|_| ())?;
    Ok(())
}

pub fn generic_char_storage_without_work<T>(
    ctx: &DecodeContext,
    output: &mut String,
    _tag: &T,
    character: char,
) -> Result<(), ()> {
    use std::fmt::Write as _;

    ctx.try_reserve_retained_text(output, character.len_utf8(), "retained char")?;
    output.write_char(character).map_err(|_| ())?; // finding: uncharged_decode_work
    Ok(())
}

pub fn instantiate_generic_char_storage_without_work(
    ctx: &DecodeContext,
    output: &mut String,
    character: char,
) -> Result<(), ()> {
    generic_char_storage_without_work(ctx, output, &(), character)
}

pub fn instantiate_generic_admitted_char(
    ctx: &DecodeContext,
    output: &mut String,
    character: char,
) -> Result<(), ()> {
    generic_admitted_char(ctx, output, &(), character)
}

pub fn generic_admitted_str<T>(
    ctx: &DecodeContext,
    output: &mut String,
    _tag: &T,
    suffix: &str,
) -> Result<(), ()> {
    use std::fmt::Write as _;

    ctx.charge_work(suffix.len() as u64, "text")?;
    ctx.try_reserve_retained_text(output, suffix.len(), "text")?;
    output.write_str(suffix).map_err(|_| ())?;
    Ok(())
}

pub fn instantiate_generic_admitted_str(
    ctx: &DecodeContext,
    output: &mut String,
    suffix: &str,
) -> Result<(), ()> {
    generic_admitted_str(ctx, output, &(), suffix)
}
