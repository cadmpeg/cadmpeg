// SPDX-License-Identifier: Apache-2.0
//! Closed support-envelope admission rules.

pub(crate) fn envelope_a_admits(entity_type: i64, form: i64) -> bool {
    match entity_type {
        0 | 100 | 102 | 112 | 114 | 116 | 120 | 122 | 123 | 130 | 132 | 140 | 141 | 142 | 143
        | 144 | 150 | 152 | 154 | 156 | 158 | 160 | 164 | 168 | 182 | 186 | 202 | 204 | 206
        | 208 | 210 | 212 | 213 | 228 | 230 | 308 | 310 | 314 | 316 | 320 | 408 | 412 | 414
        | 420 => form == 0,
        104 => matches!(form, 0..=3),
        106 => matches!(form, 1..=3 | 11..=13 | 20..=21 | 31..=38 | 40 | 63),
        108 => matches!(form, -1..=1),
        110 => matches!(form, 0..=2),
        118 | 162 | 180 | 184 | 190 | 192 | 194 | 196 | 198 | 312 | 404 | 410 | 422 | 430 => {
            matches!(form, 0..=1)
        }
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
        402 => matches!(form, 1 | 3..=7 | 9 | 12..=16 | 18..=21),
        406 => matches!(form, 1..=3 | 5..=36),
        416 => matches!(form, 0..=4),
        502 | 504 | 508 | 510 => form == 1,
        514 => matches!(form, 1..=2),
        _ => false,
    }
}
