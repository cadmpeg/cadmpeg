// SPDX-License-Identifier: Apache-2.0
//! Closed support-envelope admission rules.

use crate::global::GlobalTable;

pub(crate) fn envelope_a_admits(entity_type: i64, form: i64, global_table: GlobalTable) -> bool {
    let admitted = if macro_instance_type(entity_type) {
        true
    } else {
        match entity_type {
            0 | 100 | 102 | 112 | 114 | 116 | 120 | 122 | 123 | 130 | 132 | 134 | 136 | 138
            | 140 | 141 | 142 | 143 | 144 | 150 | 152 | 154 | 156 | 158 | 160 | 164 | 168 | 182
            | 186 | 202 | 204 | 206 | 208 | 210 | 213 | 308 | 310 | 314 | 316 | 320 | 408 | 412
            | 414 | 420 => form == 0,
            306 => form == 0,
            146 | 148 => matches!(form, 0..=34),
            228 => matches!(form, 0..=3 | 5001..=9999),
            230 => matches!(form, 0..=1),
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
            418 => form == 0,
            502 | 504 | 508 | 510 => form == 1,
            514 => matches!(form, 1..=2),
            _ => false,
        }
    };
    admitted
        && match global_table {
            GlobalTable::V4_0 => envelope_a_v4_admits(entity_type, form),
            GlobalTable::V5_0 => envelope_a_v5_0_admits(entity_type, form),
            _ => true,
        }
}

/// User-assigned Macro Instance Entity type numbers are reserved by IGES for
/// the two ranges below. The macro definition supplies the instance schema,
/// so the fixed envelope does not impose a form-number table on those types.
pub(crate) const fn macro_instance_type(entity_type: i64) -> bool {
    matches!(entity_type, 600..=699 | 10_000..=99_999)
}

fn envelope_a_v4_admits(entity_type: i64, form: i64) -> bool {
    match entity_type {
        123 | 141 | 143 | 182 | 186 | 190 | 192 | 194 | 196 | 198 | 204 | 213 | 316 | 322 | 422
        | 502 | 504 | 508 | 510 | 514 => false,
        180 | 184 => form == 0,
        228 => matches!(form, 0..=3),
        230 => form == 0,
        110 => form == 0,
        118 => matches!(form, 0..=1),
        214 => matches!(form, 1..=11),
        216 => form == 0,
        218 => form == 0,
        402 => matches!(form, 1..=16 | 18),
        404 => form == 0,
        406 => matches!(form, 1..=18) || implementor_defined_form(form),
        410 => form == 0,
        416 => matches!(form, 0..=2),
        430 => form == 0,
        _ => true,
    }
}

/// IGES 5.0 keeps the 4.0 main entity table and adds the ECOs incorporated
/// into the 5.0 release.  The gray-page application forms are not part of
/// that release, and the B-rep entity family was held for 5.1.
fn envelope_a_v5_0_admits(entity_type: i64, form: i64) -> bool {
    if v4_appendix_i_compatibility_form(entity_type, form) {
        return false;
    }

    if envelope_a_v4_admits(entity_type, form) {
        return true;
    }

    match entity_type {
        141 | 143 | 182 | 204 | 213 | 316 => form == 0,
        214 => form == 12,
        216 => matches!(form, 1..=2),
        218 => form == 1,
        228 => implementor_defined_form(form),
        230 => form == 1,
        402 => form == 19,
        404 => form == 1,
        406 => matches!(form, 19..=26),
        410 => form == 1,
        416 => form == 3,
        322 => matches!(form, 0..=2),
        422 => matches!(form, 0..=1),
        _ => false,
    }
}

/// IGES 4.0 Appendix I compatibility forms are obsolete in the V5.0 table.
const fn v4_appendix_i_compatibility_form(entity_type: i64, form: i64) -> bool {
    matches!((entity_type, form), (402, 6 | 8 | 10 | 11) | (406, 4))
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
