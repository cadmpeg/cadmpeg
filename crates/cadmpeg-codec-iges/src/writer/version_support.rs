// SPDX-License-Identifier: Apache-2.0
//! IGES dialect admission for generated entities.

use super::Entity;

const WRITER_ENTITY_TYPES: &[u32] = &[
    100, 102, 104, 108, 110, 116, 120, 122, 123, 124, 126, 128, 134, 136, 138, 141, 142, 143, 144,
    146, 148, 186, 190, 192, 194, 196, 198, 314, 406, 418, 502, 504, 508, 510, 514,
];

impl crate::IgesVersion {
    pub(super) fn admits(self, entity: &Entity) -> bool {
        if matches!(
            entity.type_code,
            124 | 125
                | 106
                | 132
                | 150
                | 152
                | 154
                | 156
                | 158
                | 160
                | 162
                | 164
                | 168
                | 180
                | 182
                | 184
                | 202
                | 204
                | 206
                | 208
                | 210
                | 212
                | 213
                | 214
                | 216
                | 218
                | 220
                | 222
                | 228
                | 230
                | 302
                | 304
                | 306
                | 308
                | 310
                | 312
                | 316
                | 320
                | 322
                | 402
                | 404
                | 406
                | 408
                | 410
                | 412
                | 414
                | 416
                | 420
                | 422
                | 430
        ) || crate::profile::macro_instance_type(i64::from(entity.type_code))
        {
            return crate::profile::envelope_a_admits(
                i64::from(entity.type_code),
                entity.form,
                crate::version::VersionFlag::from_write_version(self).global_table(),
            );
        }
        if !WRITER_ENTITY_TYPES.contains(&entity.type_code) {
            return false;
        }
        match self {
            crate::IgesVersion::V4_0 => matches!(
                (entity.type_code, entity.form),
                (
                    100 | 102
                        | 108
                        | 110
                        | 116
                        | 120
                        | 122
                        | 124
                        | 126
                        | 128
                        | 134
                        | 136
                        | 138
                        | 142
                        | 144
                        | 314
                        | 418,
                    0
                ) | (104, 0 | 2 | 3)
                    | (146 | 148, 0..=34)
                    | (406, 15)
            ),
            crate::IgesVersion::V5_0 => matches!(
                (entity.type_code, entity.form),
                (
                    100 | 102
                        | 108
                        | 110
                        | 116
                        | 120
                        | 122
                        | 124
                        | 126
                        | 128
                        | 134
                        | 136
                        | 138
                        | 141
                        | 142
                        | 143
                        | 144
                        | 314
                        | 418,
                    0
                ) | (104, 1..=3)
                    | (146 | 148, 0..=34)
                    | (406, 15)
            ),
            crate::IgesVersion::V5_1 | crate::IgesVersion::V5_2 | crate::IgesVersion::V5_3 => {
                match entity.type_code {
                    100 | 102 | 108 | 110 | 116 | 120 | 122 | 123 | 124 | 126 | 128 | 141 | 142
                    | 143 | 144 | 186 | 314 => entity.form == 0,
                    104 => matches!(entity.form, 0 | 2 | 3),
                    190 | 192 | 194 | 196 | 198 => entity.form == 1,
                    502 | 504 | 508 | 510 => entity.form == 1,
                    406 => entity.form == 15,
                    134 | 136 | 138 | 418 => entity.form == 0,
                    146 | 148 => (0..=34).contains(&entity.form),
                    514 => {
                        entity.form == 1 || (entity.form == 2 && self == crate::IgesVersion::V5_3)
                    }
                    _ => false,
                }
            }
        }
    }
}
