// SPDX-License-Identifier: Apache-2.0
//! Closed support-envelope admission rules.

use crate::global::Dialect;

pub(crate) fn envelope_a_admits(entity_type: i64, form: i64, dialect: Dialect) -> bool {
    let admitted = match entity_type {
        0 | 100 | 102 | 112 | 114 | 116 | 120 | 122 | 123 | 130 | 132 | 140 | 141 | 142 | 143
        | 144 | 150 | 152 | 154 | 156 | 158 | 160 | 164 | 168 | 182 | 186 | 202 | 204 | 206
        | 208 | 210 | 213 | 228 | 230 | 308 | 310 | 314 | 316 | 320 | 408 | 412 | 414 | 420 => {
            form == 0
        }
        212 => general_note_form_admitted(form),
        104 => matches!(form, 0..=3),
        106 => matches!(form, 1..=3 | 11..=13 | 20..=21 | 31..=38 | 40 | 63),
        108 => matches!(form, -1..=1),
        110 => matches!(form, 0..=2),
        118 | 162 | 180 | 184 | 190 | 192 | 194 | 196 | 198 | 312 | 404 | 410 | 422 | 430 => {
            matches!(form, 0..=1)
        }
        125 => matches!(form, 0..=4),
        304 => matches!(form, 1..=2),
        124 => matches!(form, 0..=1 | 10..=12),
        126 => matches!(form, 0..=5),
        128 => matches!(form, 0..=9),
        216 => matches!(form, 0..=2),
        218 | 222 => matches!(form, 0..=1),
        220 => form == 0,
        214 => matches!(form, 1..=12),
        302 => matches!(form, 5001..=9999),
        322 => matches!(form, 0..=2),
        402 => matches!(form, 1 | 2 | 3..=8 | 9..=16 | 18..=21),
        406 => property_form_admitted(form),
        416 => matches!(form, 0..=4),
        502 | 504 | 508 | 510 => form == 1,
        514 => matches!(form, 1..=2),
        _ => false,
    };
    admitted && (!matches!(dialect, Dialect::V4_0) || envelope_a_v4_admits(entity_type, form))
}

fn envelope_a_v4_admits(entity_type: i64, form: i64) -> bool {
    match entity_type {
        123 | 141 | 143 | 182 | 186 | 190 | 192 | 194 | 196 | 198 | 204 | 213 | 316 | 502 | 504
        | 508 | 510 | 514 => false,
        110 => form == 0,
        118 => matches!(form, 0..=1),
        214 => matches!(form, 1..=11),
        216 => form == 0,
        218 => form == 0,
        402 => matches!(form, 1..=5 | 7 | 9 | 12..=16 | 18),
        404 => form == 0,
        406 => matches!(form, 1..=3 | 5..=18) || implementor_defined_form(form),
        410 => form == 0,
        416 => matches!(form, 0..=2),
        430 => form == 0,
        _ => true,
    }
}

fn property_form_admitted(form: i64) -> bool {
    matches!(form, 1..=36) || implementor_defined_form(form)
}

fn implementor_defined_form(form: i64) -> bool {
    matches!(form, 5001..=9999)
}

pub(crate) fn general_note_form_admitted(form: i64) -> bool {
    matches!(form, 0..=8 | 100..=102 | 105)
}

#[cfg(test)]
mod tests;
