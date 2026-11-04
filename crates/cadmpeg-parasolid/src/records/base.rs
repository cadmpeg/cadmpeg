// SPDX-License-Identifier: Apache-2.0

use super::{BaseVersion, BorrowedField, DataType};

pub(super) fn fields(version: BaseVersion, kind: u16) -> Option<&'static [BorrowedField<'static>]> {
    Some(match (version, kind) {
        (BaseVersion::V11004, 1) => &FIELDS_11004_1,
        (BaseVersion::V11004, 2) => &FIELDS_11004_2,
        (BaseVersion::V11004, 3) => &FIELDS_11004_3,
        (BaseVersion::V11004, 4) => &FIELDS_11004_4,
        (BaseVersion::V11004, 5) => &FIELDS_11004_5,
        (BaseVersion::V11004, 6) => &FIELDS_11004_6,
        (BaseVersion::V11004, 7) => &FIELDS_11004_7,
        (BaseVersion::V11004, 8) => &FIELDS_11004_8,
        (BaseVersion::V11004, 9) => &FIELDS_11004_9,
        (BaseVersion::V11004, 10) => &FIELDS_11004_10,
        (BaseVersion::V11004, 11) => &FIELDS_11004_11,
        (BaseVersion::V11004, 12) => &FIELDS_11004_12,
        (BaseVersion::V11004, 13) => &FIELDS_11004_13,
        (BaseVersion::V11004, 14) => &FIELDS_11004_14,
        (BaseVersion::V11004, 15) => &FIELDS_11004_15,
        (BaseVersion::V11004, 16) => &FIELDS_11004_16,
        (BaseVersion::V11004, 17) => &FIELDS_11004_17,
        (BaseVersion::V11004, 18) => &FIELDS_11004_18,
        (BaseVersion::V11004, 19) => &FIELDS_11004_19,
        (BaseVersion::V11004, 29) => &FIELDS_11004_29,
        (BaseVersion::V11004, 30) => &FIELDS_11004_30,
        (BaseVersion::V11004, 31) => &FIELDS_11004_31,
        (BaseVersion::V11004, 32) => &FIELDS_11004_32,
        (BaseVersion::V11004, 33) => &FIELDS_11004_33,
        (BaseVersion::V11004, 34) => &FIELDS_11004_34,
        (BaseVersion::V11004, 35) => &FIELDS_11004_35,
        (BaseVersion::V11004, 36) => &FIELDS_11004_36,
        (BaseVersion::V11004, 37) => &FIELDS_11004_37,
        (BaseVersion::V11004, 38) => &FIELDS_11004_38,
        (BaseVersion::V11004, 39) => &FIELDS_11004_39,
        (BaseVersion::V11004, 40) => &FIELDS_11004_40,
        (BaseVersion::V11004, 41) => &FIELDS_11004_41,
        (BaseVersion::V11004, 42) => &FIELDS_11004_42,
        (BaseVersion::V11004, 43) => &FIELDS_11004_43,
        (BaseVersion::V11004, 44) => &FIELDS_11004_44,
        (BaseVersion::V11004, 45) => &FIELDS_11004_45,
        (BaseVersion::V11004, 46) => &FIELDS_11004_46,
        (BaseVersion::V11004, 47) => &FIELDS_11004_47,
        (BaseVersion::V11004, 48) => &FIELDS_11004_48,
        (BaseVersion::V11004, 49) => &FIELDS_11004_49,
        (BaseVersion::V11004, 50) => &FIELDS_11004_50,
        (BaseVersion::V11004, 51) => &FIELDS_11004_51,
        (BaseVersion::V11004, 52) => &FIELDS_11004_52,
        (BaseVersion::V11004, 53) => &FIELDS_11004_53,
        (BaseVersion::V11004, 54) => &FIELDS_11004_54,
        (BaseVersion::V11004, 55) => &FIELDS_11004_55,
        (BaseVersion::V11004, 56) => &FIELDS_11004_56,
        (BaseVersion::V11004, 57) => &FIELDS_11004_57,
        (BaseVersion::V11004, 58) => &FIELDS_11004_58,
        (BaseVersion::V11004, 59) => &FIELDS_11004_59,
        (BaseVersion::V11004, 60) => &FIELDS_11004_60,
        (BaseVersion::V11004, 61) => &FIELDS_11004_61,
        (BaseVersion::V11004, 62) => &FIELDS_11004_62,
        (BaseVersion::V11004, 63) => &FIELDS_11004_63,
        (BaseVersion::V11004, 64) => &FIELDS_11004_64,
        (BaseVersion::V11004, 65) => &FIELDS_11004_65,
        (BaseVersion::V11004, 66) => &FIELDS_11004_66,
        (BaseVersion::V11004, 67) => &FIELDS_11004_67,
        (BaseVersion::V11004, 68) => &FIELDS_11004_68,
        (BaseVersion::V11004, 69) => &FIELDS_11004_69,
        (BaseVersion::V11004, 70) => &FIELDS_11004_70,
        (BaseVersion::V11004, 71) => &FIELDS_11004_71,
        (BaseVersion::V11004, 72) => &FIELDS_11004_72,
        (BaseVersion::V11004, 73) => &FIELDS_11004_73,
        (BaseVersion::V11004, 74) => &FIELDS_11004_74,
        (BaseVersion::V11004, 78) => &FIELDS_11004_78,
        (BaseVersion::V11004, 79) => &FIELDS_11004_79,
        (BaseVersion::V11004, 80) => &FIELDS_11004_80,
        (BaseVersion::V11004, 81) => &FIELDS_11004_81,
        (BaseVersion::V11004, 82) => &FIELDS_11004_82,
        (BaseVersion::V11004, 83) => &FIELDS_11004_83,
        (BaseVersion::V11004, 84) => &FIELDS_11004_84,
        (BaseVersion::V11004, 85) => &FIELDS_11004_85,
        (BaseVersion::V11004, 86) => &FIELDS_11004_85,
        (BaseVersion::V11004, 87) => &FIELDS_11004_85,
        (BaseVersion::V11004, 88) => &FIELDS_11004_88,
        (BaseVersion::V11004, 89) => &FIELDS_11004_85,
        (BaseVersion::V11004, 90) => &FIELDS_11004_90,
        (BaseVersion::V11004, 91) => &FIELDS_11004_91,
        (BaseVersion::V11004, 96) => &FIELDS_11004_96,
        (BaseVersion::V11004, 97) => &FIELDS_11004_97,
        (BaseVersion::V11004, 100) => &FIELDS_11004_100,
        (BaseVersion::V11004, 101) => &FIELDS_11004_101,
        (BaseVersion::V11004, 102) => &FIELDS_11004_79,
        (BaseVersion::V11004, 103) => &FIELDS_11004_103,
        (BaseVersion::V11004, 104) => &FIELDS_11004_104,
        (BaseVersion::V11004, 105) => &FIELDS_11004_105,
        (BaseVersion::V11004, 106) => &FIELDS_11004_106,
        (BaseVersion::V11004, 107) => &FIELDS_11004_107,
        (BaseVersion::V11004, 108) => &FIELDS_11004_108,
        (BaseVersion::V11004, 109) => &FIELDS_11004_109,
        (BaseVersion::V11004, 110) => &FIELDS_11004_110,
        (BaseVersion::V11004, 111) => &FIELDS_11004_111,
        (BaseVersion::V11004, 112) => &FIELDS_11004_112,
        (BaseVersion::V11004, 120) => &FIELDS_11004_120,
        (BaseVersion::V11004, 121) => &FIELDS_11004_121,
        (BaseVersion::V11004, 122) => &FIELDS_11004_122,
        (BaseVersion::V11004, 123) => &FIELDS_11004_123,
        (BaseVersion::V11004, 124) => &FIELDS_11004_124,
        (BaseVersion::V11004, 125) => &FIELDS_11004_125,
        (BaseVersion::V11004, 126) => &FIELDS_11004_126,
        (BaseVersion::V11004, 127) => &FIELDS_11004_127,
        (BaseVersion::V11004, 128) => &FIELDS_11004_128,
        (BaseVersion::V11004, 129) => &FIELDS_11004_127,
        (BaseVersion::V11004, 130) => &FIELDS_11004_130,
        (BaseVersion::V11004, 131) => &FIELDS_11004_131,
        (BaseVersion::V11004, 132) => &FIELDS_11004_132,
        (BaseVersion::V11004, 133) => &FIELDS_11004_133,
        (BaseVersion::V11004, 134) => &FIELDS_11004_134,
        (BaseVersion::V11004, 135) => &FIELDS_11004_135,
        (BaseVersion::V11004, 136) => &FIELDS_11004_136,
        (BaseVersion::V11004, 137) => &FIELDS_11004_137,
        (BaseVersion::V11004, 138) => &FIELDS_11004_138,
        (BaseVersion::V11004, 139) => &FIELDS_11004_139,
        (BaseVersion::V11004, 141) => &FIELDS_11004_141,
        (BaseVersion::V11004, 150) => &FIELDS_11004_150,
        (BaseVersion::V11004, 151) => &FIELDS_11004_151,
        (BaseVersion::V11004, 152) => &FIELDS_11004_152,
        (BaseVersion::V11004, 153) => &FIELDS_11004_153,
        (BaseVersion::V11004, 154) => &FIELDS_11004_154,
        (BaseVersion::V11004, 155) => &FIELDS_11004_155,
        (BaseVersion::V11004, 156) => &FIELDS_11004_156,
        (BaseVersion::V11004, 157) => &FIELDS_11004_157,
        (BaseVersion::V11004, 158) => &FIELDS_11004_158,
        (BaseVersion::V11004, 159) => &FIELDS_11004_159,
        (BaseVersion::V11004, 160) => &FIELDS_11004_160,
        (BaseVersion::V11004, 161) => &FIELDS_11004_161,
        (BaseVersion::V11004, 162) => &FIELDS_11004_162,
        (BaseVersion::V11004, 168) => &FIELDS_11004_168,
        (BaseVersion::V11004, 169) => &FIELDS_11004_169,
        (BaseVersion::V11004, 170) => &FIELDS_11004_170,
        (BaseVersion::V11004, 171) => &FIELDS_11004_171,
        (BaseVersion::V11004, 172) => &FIELDS_11004_172,
        (BaseVersion::V11004, 180) => &FIELDS_11004_180,
        (BaseVersion::V11004, 181) => &FIELDS_11004_181,
        (BaseVersion::V11004, 182) => &FIELDS_11004_182,
        (BaseVersion::V11004, 183) => &FIELDS_11004_183,
        (BaseVersion::V13006, 1) => &FIELDS_11004_1,
        (BaseVersion::V13006, 2) => &FIELDS_11004_2,
        (BaseVersion::V13006, 3) => &FIELDS_11004_3,
        (BaseVersion::V13006, 4) => &FIELDS_11004_4,
        (BaseVersion::V13006, 5) => &FIELDS_11004_5,
        (BaseVersion::V13006, 6) => &FIELDS_11004_6,
        (BaseVersion::V13006, 7) => &FIELDS_13006_7,
        (BaseVersion::V13006, 8) => &FIELDS_13006_8,
        (BaseVersion::V13006, 9) => &FIELDS_11004_9,
        (BaseVersion::V13006, 10) => &FIELDS_11004_10,
        (BaseVersion::V13006, 11) => &FIELDS_11004_11,
        (BaseVersion::V13006, 12) => &FIELDS_11004_12,
        (BaseVersion::V13006, 13) => &FIELDS_11004_13,
        (BaseVersion::V13006, 14) => &FIELDS_11004_14,
        (BaseVersion::V13006, 15) => &FIELDS_11004_15,
        (BaseVersion::V13006, 16) => &FIELDS_11004_16,
        (BaseVersion::V13006, 17) => &FIELDS_11004_17,
        (BaseVersion::V13006, 18) => &FIELDS_11004_18,
        (BaseVersion::V13006, 19) => &FIELDS_11004_19,
        (BaseVersion::V13006, 29) => &FIELDS_11004_29,
        (BaseVersion::V13006, 30) => &FIELDS_11004_30,
        (BaseVersion::V13006, 31) => &FIELDS_11004_31,
        (BaseVersion::V13006, 32) => &FIELDS_11004_32,
        (BaseVersion::V13006, 33) => &FIELDS_11004_33,
        (BaseVersion::V13006, 34) => &FIELDS_11004_34,
        (BaseVersion::V13006, 35) => &FIELDS_11004_35,
        (BaseVersion::V13006, 36) => &FIELDS_11004_36,
        (BaseVersion::V13006, 37) => &FIELDS_11004_37,
        (BaseVersion::V13006, 38) => &FIELDS_11004_38,
        (BaseVersion::V13006, 39) => &FIELDS_11004_39,
        (BaseVersion::V13006, 40) => &FIELDS_11004_40,
        (BaseVersion::V13006, 41) => &FIELDS_11004_41,
        (BaseVersion::V13006, 42) => &FIELDS_11004_42,
        (BaseVersion::V13006, 43) => &FIELDS_11004_43,
        (BaseVersion::V13006, 44) => &FIELDS_11004_44,
        (BaseVersion::V13006, 45) => &FIELDS_11004_45,
        (BaseVersion::V13006, 46) => &FIELDS_11004_46,
        (BaseVersion::V13006, 47) => &FIELDS_11004_47,
        (BaseVersion::V13006, 48) => &FIELDS_11004_48,
        (BaseVersion::V13006, 49) => &FIELDS_11004_49,
        (BaseVersion::V13006, 50) => &FIELDS_11004_50,
        (BaseVersion::V13006, 51) => &FIELDS_11004_51,
        (BaseVersion::V13006, 52) => &FIELDS_11004_52,
        (BaseVersion::V13006, 53) => &FIELDS_11004_53,
        (BaseVersion::V13006, 54) => &FIELDS_11004_54,
        (BaseVersion::V13006, 55) => &FIELDS_11004_55,
        (BaseVersion::V13006, 56) => &FIELDS_11004_56,
        (BaseVersion::V13006, 57) => &FIELDS_11004_57,
        (BaseVersion::V13006, 58) => &FIELDS_11004_58,
        (BaseVersion::V13006, 59) => &FIELDS_11004_59,
        (BaseVersion::V13006, 60) => &FIELDS_11004_60,
        (BaseVersion::V13006, 61) => &FIELDS_11004_61,
        (BaseVersion::V13006, 62) => &FIELDS_11004_62,
        (BaseVersion::V13006, 63) => &FIELDS_11004_63,
        (BaseVersion::V13006, 64) => &FIELDS_11004_64,
        (BaseVersion::V13006, 65) => &FIELDS_11004_65,
        (BaseVersion::V13006, 66) => &FIELDS_11004_66,
        (BaseVersion::V13006, 67) => &FIELDS_11004_67,
        (BaseVersion::V13006, 68) => &FIELDS_11004_68,
        (BaseVersion::V13006, 69) => &FIELDS_11004_69,
        (BaseVersion::V13006, 70) => &FIELDS_11004_70,
        (BaseVersion::V13006, 71) => &FIELDS_11004_71,
        (BaseVersion::V13006, 72) => &FIELDS_11004_72,
        (BaseVersion::V13006, 73) => &FIELDS_11004_73,
        (BaseVersion::V13006, 74) => &FIELDS_11004_74,
        (BaseVersion::V13006, 78) => &FIELDS_13006_78,
        (BaseVersion::V13006, 79) => &FIELDS_11004_79,
        (BaseVersion::V13006, 80) => &FIELDS_13006_80,
        (BaseVersion::V13006, 81) => &FIELDS_11004_81,
        (BaseVersion::V13006, 82) => &FIELDS_11004_82,
        (BaseVersion::V13006, 83) => &FIELDS_11004_83,
        (BaseVersion::V13006, 84) => &FIELDS_11004_84,
        (BaseVersion::V13006, 85) => &FIELDS_11004_85,
        (BaseVersion::V13006, 86) => &FIELDS_11004_85,
        (BaseVersion::V13006, 87) => &FIELDS_11004_85,
        (BaseVersion::V13006, 88) => &FIELDS_11004_88,
        (BaseVersion::V13006, 89) => &FIELDS_11004_85,
        (BaseVersion::V13006, 90) => &FIELDS_11004_90,
        (BaseVersion::V13006, 91) => &FIELDS_11004_91,
        (BaseVersion::V13006, 95) => &FIELDS_13006_95,
        (BaseVersion::V13006, 96) => &FIELDS_11004_96,
        (BaseVersion::V13006, 97) => &FIELDS_11004_97,
        (BaseVersion::V13006, 98) => &FIELDS_13006_98,
        (BaseVersion::V13006, 99) => &FIELDS_13006_99,
        (BaseVersion::V13006, 100) => &FIELDS_11004_100,
        (BaseVersion::V13006, 101) => &FIELDS_11004_101,
        (BaseVersion::V13006, 102) => &FIELDS_11004_79,
        (BaseVersion::V13006, 103) => &FIELDS_11004_103,
        (BaseVersion::V13006, 104) => &FIELDS_11004_104,
        (BaseVersion::V13006, 105) => &FIELDS_11004_105,
        (BaseVersion::V13006, 106) => &FIELDS_11004_106,
        (BaseVersion::V13006, 107) => &FIELDS_11004_107,
        (BaseVersion::V13006, 108) => &FIELDS_11004_108,
        (BaseVersion::V13006, 109) => &FIELDS_11004_109,
        (BaseVersion::V13006, 110) => &FIELDS_11004_110,
        (BaseVersion::V13006, 111) => &FIELDS_11004_111,
        (BaseVersion::V13006, 112) => &FIELDS_11004_112,
        (BaseVersion::V13006, 120) => &FIELDS_11004_120,
        (BaseVersion::V13006, 121) => &FIELDS_11004_121,
        (BaseVersion::V13006, 122) => &FIELDS_11004_122,
        (BaseVersion::V13006, 123) => &FIELDS_11004_123,
        (BaseVersion::V13006, 124) => &FIELDS_11004_124,
        (BaseVersion::V13006, 125) => &FIELDS_11004_125,
        (BaseVersion::V13006, 126) => &FIELDS_11004_126,
        (BaseVersion::V13006, 127) => &FIELDS_11004_127,
        (BaseVersion::V13006, 128) => &FIELDS_11004_128,
        (BaseVersion::V13006, 129) => &FIELDS_13006_129,
        (BaseVersion::V13006, 130) => &FIELDS_11004_130,
        (BaseVersion::V13006, 131) => &FIELDS_11004_131,
        (BaseVersion::V13006, 132) => &FIELDS_11004_132,
        (BaseVersion::V13006, 133) => &FIELDS_11004_133,
        (BaseVersion::V13006, 134) => &FIELDS_11004_134,
        (BaseVersion::V13006, 135) => &FIELDS_13006_135,
        (BaseVersion::V13006, 136) => &FIELDS_11004_136,
        (BaseVersion::V13006, 137) => &FIELDS_11004_137,
        (BaseVersion::V13006, 138) => &FIELDS_11004_138,
        (BaseVersion::V13006, 139) => &FIELDS_11004_139,
        (BaseVersion::V13006, 141) => &FIELDS_11004_141,
        (BaseVersion::V13006, 150) => &FIELDS_11004_150,
        (BaseVersion::V13006, 151) => &FIELDS_11004_151,
        (BaseVersion::V13006, 152) => &FIELDS_11004_152,
        (BaseVersion::V13006, 153) => &FIELDS_11004_153,
        (BaseVersion::V13006, 154) => &FIELDS_11004_154,
        (BaseVersion::V13006, 155) => &FIELDS_11004_155,
        (BaseVersion::V13006, 156) => &FIELDS_11004_156,
        (BaseVersion::V13006, 157) => &FIELDS_11004_157,
        (BaseVersion::V13006, 158) => &FIELDS_11004_158,
        (BaseVersion::V13006, 159) => &FIELDS_11004_159,
        (BaseVersion::V13006, 160) => &FIELDS_11004_160,
        (BaseVersion::V13006, 161) => &FIELDS_11004_161,
        (BaseVersion::V13006, 162) => &FIELDS_11004_162,
        (BaseVersion::V13006, 163) => &FIELDS_13006_163,
        (BaseVersion::V13006, 168) => &FIELDS_11004_168,
        (BaseVersion::V13006, 169) => &FIELDS_11004_169,
        (BaseVersion::V13006, 170) => &FIELDS_11004_170,
        (BaseVersion::V13006, 171) => &FIELDS_11004_171,
        (BaseVersion::V13006, 172) => &FIELDS_11004_172,
        (BaseVersion::V13006, 180) => &FIELDS_11004_180,
        (BaseVersion::V13006, 181) => &FIELDS_11004_181,
        (BaseVersion::V13006, 182) => &FIELDS_11004_182,
        (BaseVersion::V13006, 183) => &FIELDS_11004_183,
        (BaseVersion::V13006, 184) => &FIELDS_13006_184,
        _ => return None,
    })
}

pub(super) fn maximum(version: BaseVersion) -> u16 {
    match version {
        BaseVersion::V11004 => 183,
        BaseVersion::V13006 => 184,
    }
}

const FIELDS_11004_1: [BorrowedField<'static>; 0] = [];

const FIELDS_11004_2: [BorrowedField<'static>; 1] =
    [BorrowedField::new("ws", DataType::Char, 0, 1, true)];

const FIELDS_11004_3: [BorrowedField<'static>; 2] = [
    BorrowedField::new("current_pmark", DataType::Pointer, 4, 0, true),
    BorrowedField::new("highest_id", DataType::Integer, 0, 0, true),
];

const FIELDS_11004_4: [BorrowedField<'static>; 10] = [
    BorrowedField::new("preceding", DataType::Pointer, 4, 0, true),
    BorrowedField::new("first_following", DataType::Pointer, 4, 0, true),
    BorrowedField::new("next_sibling", DataType::Pointer, 4, 0, true),
    BorrowedField::new("prev_sibling", DataType::Pointer, 4, 0, true),
    BorrowedField::new("n_new_nodes", DataType::Integer, 0, 0, true),
    BorrowedField::new("n_del_nodes", DataType::Integer, 0, 0, true),
    BorrowedField::new("n_copy_mod_nodes", DataType::Integer, 0, 0, true),
    BorrowedField::new("delta_key", DataType::Integer, 0, 0, true),
    BorrowedField::new("delta_is_forward", DataType::Logical, 0, 0, true),
    BorrowedField::new("id", DataType::Integer, 0, 0, true),
];

const FIELDS_11004_5: [BorrowedField<'static>; 7] = [
    BorrowedField::new("first_partition", DataType::Pointer, 3, 0, true),
    BorrowedField::new("current_mark", DataType::Pointer, 9, 0, true),
    BorrowedField::new("pmarks_at_current", DataType::Pointer, 170, 0, true),
    BorrowedField::new("root_tag_table", DataType::Pointer, 8, 0, true),
    BorrowedField::new("session_data", DataType::Pointer, 172, 0, true),
    BorrowedField::new("new", DataType::Pointer, 168, 0, true),
    BorrowedField::new("del", DataType::Pointer, 168, 0, true),
];

const FIELDS_11004_6: [BorrowedField<'static>; 2] = [
    BorrowedField::new("entity", DataType::Pointer, 1001, 256, true),
    BorrowedField::new("n_live", DataType::Integer, 0, 0, true),
];

const FIELDS_11004_7: [BorrowedField<'static>; 1] =
    [BorrowedField::new("leaf", DataType::Pointer, 6, 1024, true)];

const FIELDS_11004_8: [BorrowedField<'static>; 6] = [
    BorrowedField::new("branch", DataType::Pointer, 7, 1024, true),
    BorrowedField::new("tag_limit", DataType::Integer, 0, 0, true),
    BorrowedField::new("null_branch", DataType::Pointer, 7, 0, true),
    BorrowedField::new("null_leaf", DataType::Pointer, 6, 0, true),
    BorrowedField::new("next_low_tag", DataType::Integer, 0, 0, true),
    BorrowedField::new("next_high_tag", DataType::Integer, 0, 0, true),
];

const FIELDS_11004_9: [BorrowedField<'static>; 4] = [
    BorrowedField::new("following", DataType::Pointer, 9, 0, true),
    BorrowedField::new("preceding", DataType::Pointer, 9, 0, true),
    BorrowedField::new("pmarks_from_preceding", DataType::Pointer, 170, 0, true),
    BorrowedField::new("pmarks_from_following", DataType::Pointer, 170, 0, true),
];

const FIELDS_11004_10: [BorrowedField<'static>; 17] = [
    BorrowedField::new("highest_node_id", DataType::Integer, 0, 0, true),
    BorrowedField::new("attributes_features", DataType::Pointer, 1019, 0, true),
    BorrowedField::new("attribute_chains", DataType::Pointer, 70, 0, true),
    BorrowedField::new("list", DataType::Pointer, 70, 0, true),
    BorrowedField::new("surface", DataType::Pointer, 1006, 0, true),
    BorrowedField::new("curve", DataType::Pointer, 1008, 0, true),
    BorrowedField::new("point", DataType::Pointer, 29, 0, true),
    BorrowedField::new("key", DataType::Pointer, 102, 0, true),
    BorrowedField::new("res_size", DataType::Real, 0, 0, true),
    BorrowedField::new("res_linear", DataType::Real, 0, 0, true),
    BorrowedField::new("ref_instance", DataType::Pointer, 11, 0, true),
    BorrowedField::new("next", DataType::Pointer, 10, 0, true),
    BorrowedField::new("previous", DataType::Pointer, 10, 0, true),
    BorrowedField::new("state", DataType::Byte, 0, 0, true),
    BorrowedField::new("owner", DataType::Pointer, 101, 0, true),
    BorrowedField::new("type", DataType::Byte, 0, 0, true),
    BorrowedField::new("sub_instance", DataType::Pointer, 11, 0, true),
];

const FIELDS_11004_11: [BorrowedField<'static>; 10] = [
    BorrowedField::new("node_id", DataType::Integer, 0, 0, true),
    BorrowedField::new("attributes_features", DataType::Pointer, 1019, 0, true),
    BorrowedField::new("type", DataType::Byte, 0, 0, true),
    BorrowedField::new("part", DataType::Pointer, 1005, 0, true),
    BorrowedField::new("transform", DataType::Pointer, 100, 0, true),
    BorrowedField::new("assembly", DataType::Pointer, 10, 0, true),
    BorrowedField::new("next_in_part", DataType::Pointer, 11, 0, true),
    BorrowedField::new("prev_in_part", DataType::Pointer, 11, 0, true),
    BorrowedField::new("next_of_part", DataType::Pointer, 11, 0, true),
    BorrowedField::new("prev_of_part", DataType::Pointer, 11, 0, true),
];

const FIELDS_11004_12: [BorrowedField<'static>; 23] = [
    BorrowedField::new("highest_node_id", DataType::Integer, 0, 0, true),
    BorrowedField::new("attributes_features", DataType::Pointer, 1019, 0, true),
    BorrowedField::new("attribute_chains", DataType::Pointer, 70, 0, true),
    BorrowedField::new("surface", DataType::Pointer, 1006, 0, true),
    BorrowedField::new("curve", DataType::Pointer, 1008, 0, true),
    BorrowedField::new("point", DataType::Pointer, 29, 0, true),
    BorrowedField::new("key", DataType::Pointer, 102, 0, true),
    BorrowedField::new("res_size", DataType::Real, 0, 0, true),
    BorrowedField::new("res_linear", DataType::Real, 0, 0, true),
    BorrowedField::new("ref_instance", DataType::Pointer, 11, 0, true),
    BorrowedField::new("next", DataType::Pointer, 12, 0, true),
    BorrowedField::new("previous", DataType::Pointer, 12, 0, true),
    BorrowedField::new("state", DataType::Byte, 0, 0, true),
    BorrowedField::new("owner", DataType::Pointer, 101, 0, true),
    BorrowedField::new("body_type", DataType::Byte, 0, 0, true),
    BorrowedField::new("nom_geom_state", DataType::Byte, 0, 0, true),
    BorrowedField::new("shell", DataType::Pointer, 13, 0, true),
    BorrowedField::new("boundary_surface", DataType::Pointer, 1006, 0, true),
    BorrowedField::new("boundary_curve", DataType::Pointer, 1008, 0, true),
    BorrowedField::new("boundary_point", DataType::Pointer, 29, 0, true),
    BorrowedField::new("region", DataType::Pointer, 19, 0, true),
    BorrowedField::new("edge", DataType::Pointer, 16, 0, true),
    BorrowedField::new("vertex", DataType::Pointer, 18, 0, true),
];

const FIELDS_11004_13: [BorrowedField<'static>; 9] = [
    BorrowedField::new("node_id", DataType::Integer, 0, 0, true),
    BorrowedField::new("attributes_features", DataType::Pointer, 1019, 0, true),
    BorrowedField::new("body", DataType::Pointer, 12, 0, true),
    BorrowedField::new("next", DataType::Pointer, 13, 0, true),
    BorrowedField::new("face", DataType::Pointer, 14, 0, true),
    BorrowedField::new("edge", DataType::Pointer, 16, 0, true),
    BorrowedField::new("vertex", DataType::Pointer, 18, 0, true),
    BorrowedField::new("region", DataType::Pointer, 19, 0, true),
    BorrowedField::new("front_face", DataType::Pointer, 14, 0, true),
];

const FIELDS_11004_14: [BorrowedField<'static>; 14] = [
    BorrowedField::new("node_id", DataType::Integer, 0, 0, true),
    BorrowedField::new("attributes_features", DataType::Pointer, 1019, 0, true),
    BorrowedField::new("tolerance", DataType::Real, 0, 0, true),
    BorrowedField::new("next", DataType::Pointer, 14, 0, true),
    BorrowedField::new("previous", DataType::Pointer, 14, 0, true),
    BorrowedField::new("loop", DataType::Pointer, 15, 0, true),
    BorrowedField::new("shell", DataType::Pointer, 13, 0, true),
    BorrowedField::new("surface", DataType::Pointer, 1006, 0, true),
    BorrowedField::new("sense", DataType::Char, 0, 0, true),
    BorrowedField::new("next_on_surface", DataType::Pointer, 14, 0, true),
    BorrowedField::new("previous_on_surface", DataType::Pointer, 14, 0, true),
    BorrowedField::new("next_front", DataType::Pointer, 14, 0, true),
    BorrowedField::new("previous_front", DataType::Pointer, 14, 0, true),
    BorrowedField::new("front_shell", DataType::Pointer, 13, 0, true),
];

const FIELDS_11004_15: [BorrowedField<'static>; 5] = [
    BorrowedField::new("node_id", DataType::Integer, 0, 0, true),
    BorrowedField::new("attributes_features", DataType::Pointer, 1019, 0, true),
    BorrowedField::new("halfedge", DataType::Pointer, 17, 0, true),
    BorrowedField::new("face", DataType::Pointer, 14, 0, true),
    BorrowedField::new("next", DataType::Pointer, 15, 0, true),
];

const FIELDS_11004_16: [BorrowedField<'static>; 10] = [
    BorrowedField::new("node_id", DataType::Integer, 0, 0, true),
    BorrowedField::new("attributes_features", DataType::Pointer, 1019, 0, true),
    BorrowedField::new("tolerance", DataType::Real, 0, 0, true),
    BorrowedField::new("halfedge", DataType::Pointer, 17, 0, true),
    BorrowedField::new("previous", DataType::Pointer, 16, 0, true),
    BorrowedField::new("next", DataType::Pointer, 16, 0, true),
    BorrowedField::new("curve", DataType::Pointer, 1008, 0, true),
    BorrowedField::new("next_on_curve", DataType::Pointer, 16, 0, true),
    BorrowedField::new("previous_on_curve", DataType::Pointer, 16, 0, true),
    BorrowedField::new("owner", DataType::Pointer, 1029, 0, true),
];

const FIELDS_11004_17: [BorrowedField<'static>; 10] = [
    BorrowedField::new("attributes_features", DataType::Pointer, 1019, 0, true),
    BorrowedField::new("loop", DataType::Pointer, 15, 0, true),
    BorrowedField::new("forward", DataType::Pointer, 17, 0, true),
    BorrowedField::new("backward", DataType::Pointer, 17, 0, true),
    BorrowedField::new("vertex", DataType::Pointer, 18, 0, true),
    BorrowedField::new("other", DataType::Pointer, 17, 0, true),
    BorrowedField::new("edge", DataType::Pointer, 16, 0, true),
    BorrowedField::new("curve", DataType::Pointer, 1008, 0, true),
    BorrowedField::new("next_at_vx", DataType::Pointer, 17, 0, true),
    BorrowedField::new("sense", DataType::Char, 0, 0, true),
];

const FIELDS_11004_18: [BorrowedField<'static>; 8] = [
    BorrowedField::new("node_id", DataType::Integer, 0, 0, true),
    BorrowedField::new("attributes_features", DataType::Pointer, 1019, 0, true),
    BorrowedField::new("halfedge", DataType::Pointer, 17, 0, true),
    BorrowedField::new("previous", DataType::Pointer, 18, 0, true),
    BorrowedField::new("next", DataType::Pointer, 18, 0, true),
    BorrowedField::new("point", DataType::Pointer, 29, 0, true),
    BorrowedField::new("tolerance", DataType::Real, 0, 0, true),
    BorrowedField::new("owner", DataType::Pointer, 1029, 0, true),
];

const FIELDS_11004_19: [BorrowedField<'static>; 7] = [
    BorrowedField::new("node_id", DataType::Integer, 0, 0, true),
    BorrowedField::new("attributes_features", DataType::Pointer, 1019, 0, true),
    BorrowedField::new("body", DataType::Pointer, 12, 0, true),
    BorrowedField::new("next", DataType::Pointer, 19, 0, true),
    BorrowedField::new("previous", DataType::Pointer, 19, 0, true),
    BorrowedField::new("shell", DataType::Pointer, 13, 0, true),
    BorrowedField::new("type", DataType::Char, 0, 0, true),
];

const FIELDS_11004_29: [BorrowedField<'static>; 6] = [
    BorrowedField::new("node_id", DataType::Integer, 0, 0, true),
    BorrowedField::new("attributes_features", DataType::Pointer, 1019, 0, true),
    BorrowedField::new("owner", DataType::Pointer, 1011, 0, true),
    BorrowedField::new("next", DataType::Pointer, 29, 0, true),
    BorrowedField::new("previous", DataType::Pointer, 29, 0, true),
    BorrowedField::new("pvec", DataType::Vector, 0, 0, true),
];

const FIELDS_11004_30: [BorrowedField<'static>; 9] = [
    BorrowedField::new("node_id", DataType::Integer, 0, 0, true),
    BorrowedField::new("attributes_features", DataType::Pointer, 1019, 0, true),
    BorrowedField::new("owner", DataType::Pointer, 1010, 0, true),
    BorrowedField::new("next", DataType::Pointer, 1008, 0, true),
    BorrowedField::new("previous", DataType::Pointer, 1008, 0, true),
    BorrowedField::new("geometric_owner", DataType::Pointer, 141, 0, true),
    BorrowedField::new("sense", DataType::Char, 0, 0, true),
    BorrowedField::new("pvec", DataType::Vector, 0, 0, true),
    BorrowedField::new("direction", DataType::Vector, 0, 0, true),
];

const FIELDS_11004_31: [BorrowedField<'static>; 11] = [
    BorrowedField::new("node_id", DataType::Integer, 0, 0, true),
    BorrowedField::new("attributes_features", DataType::Pointer, 1019, 0, true),
    BorrowedField::new("owner", DataType::Pointer, 1010, 0, true),
    BorrowedField::new("next", DataType::Pointer, 1008, 0, true),
    BorrowedField::new("previous", DataType::Pointer, 1008, 0, true),
    BorrowedField::new("geometric_owner", DataType::Pointer, 141, 0, true),
    BorrowedField::new("sense", DataType::Char, 0, 0, true),
    BorrowedField::new("centre", DataType::Vector, 0, 0, true),
    BorrowedField::new("normal", DataType::Vector, 0, 0, true),
    BorrowedField::new("x_axis", DataType::Vector, 0, 0, true),
    BorrowedField::new("radius", DataType::Real, 0, 0, true),
];

const FIELDS_11004_32: [BorrowedField<'static>; 12] = [
    BorrowedField::new("node_id", DataType::Integer, 0, 0, true),
    BorrowedField::new("attributes_features", DataType::Pointer, 1019, 0, true),
    BorrowedField::new("owner", DataType::Pointer, 1010, 0, true),
    BorrowedField::new("next", DataType::Pointer, 1008, 0, true),
    BorrowedField::new("previous", DataType::Pointer, 1008, 0, true),
    BorrowedField::new("geometric_owner", DataType::Pointer, 141, 0, true),
    BorrowedField::new("sense", DataType::Char, 0, 0, true),
    BorrowedField::new("centre", DataType::Vector, 0, 0, true),
    BorrowedField::new("normal", DataType::Vector, 0, 0, true),
    BorrowedField::new("x_axis", DataType::Vector, 0, 0, true),
    BorrowedField::new("major_radius", DataType::Real, 0, 0, true),
    BorrowedField::new("minor_radius", DataType::Real, 0, 0, true),
];

const FIELDS_11004_33: [BorrowedField<'static>; 11] = [
    BorrowedField::new("node_id", DataType::Integer, 0, 0, true),
    BorrowedField::new("attributes_features", DataType::Pointer, 1019, 0, true),
    BorrowedField::new("owner", DataType::Pointer, 1010, 0, true),
    BorrowedField::new("next", DataType::Pointer, 1008, 0, true),
    BorrowedField::new("previous", DataType::Pointer, 1008, 0, true),
    BorrowedField::new("geometric_owner", DataType::Pointer, 141, 0, true),
    BorrowedField::new("sense", DataType::Char, 0, 0, true),
    BorrowedField::new("origin", DataType::Vector, 0, 0, true),
    BorrowedField::new("normal", DataType::Vector, 0, 0, true),
    BorrowedField::new("x_axis", DataType::Vector, 0, 0, true),
    BorrowedField::new("focal_length", DataType::Real, 0, 0, true),
];

const FIELDS_11004_34: [BorrowedField<'static>; 12] = [
    BorrowedField::new("node_id", DataType::Integer, 0, 0, true),
    BorrowedField::new("attributes_features", DataType::Pointer, 1019, 0, true),
    BorrowedField::new("owner", DataType::Pointer, 1010, 0, true),
    BorrowedField::new("next", DataType::Pointer, 1008, 0, true),
    BorrowedField::new("previous", DataType::Pointer, 1008, 0, true),
    BorrowedField::new("geometric_owner", DataType::Pointer, 141, 0, true),
    BorrowedField::new("sense", DataType::Char, 0, 0, true),
    BorrowedField::new("origin", DataType::Vector, 0, 0, true),
    BorrowedField::new("normal", DataType::Vector, 0, 0, true),
    BorrowedField::new("x_axis", DataType::Vector, 0, 0, true),
    BorrowedField::new("transverse_radius", DataType::Real, 0, 0, true),
    BorrowedField::new("conjugate_radius", DataType::Real, 0, 0, true),
];

const FIELDS_11004_35: [BorrowedField<'static>; 9] = [
    BorrowedField::new("node_id", DataType::Integer, 0, 0, true),
    BorrowedField::new("attributes_features", DataType::Pointer, 1019, 0, true),
    BorrowedField::new("owner", DataType::Pointer, 1010, 0, true),
    BorrowedField::new("next", DataType::Pointer, 1008, 0, true),
    BorrowedField::new("previous", DataType::Pointer, 1008, 0, true),
    BorrowedField::new("geometric_owner", DataType::Pointer, 141, 0, true),
    BorrowedField::new("sense", DataType::Char, 0, 0, true),
    BorrowedField::new("seg", DataType::Integer, 0, 0, true),
    BorrowedField::new("cpc", DataType::Pointer, 48, 0, true),
];

const FIELDS_11004_36: [BorrowedField<'static>; 11] = [
    BorrowedField::new("node_id", DataType::Integer, 0, 0, true),
    BorrowedField::new("attributes_features", DataType::Pointer, 1019, 0, true),
    BorrowedField::new("owner", DataType::Pointer, 1010, 0, true),
    BorrowedField::new("next", DataType::Pointer, 1008, 0, true),
    BorrowedField::new("previous", DataType::Pointer, 1008, 0, true),
    BorrowedField::new("geometric_owner", DataType::Pointer, 141, 0, true),
    BorrowedField::new("sense", DataType::Char, 0, 0, true),
    BorrowedField::new("vertex_dimension", DataType::Short, 0, 0, true),
    BorrowedField::new("segment_count", DataType::Short, 0, 0, true),
    BorrowedField::new("order", DataType::Short, 0, 0, true),
    BorrowedField::new("segment", DataType::Pointer, 1025, 1, true),
];

const FIELDS_11004_37: [BorrowedField<'static>; 10] = [
    BorrowedField::new("node_id", DataType::Integer, 0, 0, true),
    BorrowedField::new("attributes_features", DataType::Pointer, 1019, 0, true),
    BorrowedField::new("owner", DataType::Pointer, 1010, 0, true),
    BorrowedField::new("next", DataType::Pointer, 1008, 0, true),
    BorrowedField::new("previous", DataType::Pointer, 1008, 0, true),
    BorrowedField::new("geometric_owner", DataType::Pointer, 141, 0, true),
    BorrowedField::new("sense", DataType::Char, 0, 0, true),
    BorrowedField::new("boundary", DataType::Char, 0, 0, true),
    BorrowedField::new("lh_patch", DataType::Pointer, 61, 0, true),
    BorrowedField::new("rh_patch", DataType::Pointer, 61, 0, true),
];

const FIELDS_11004_38: [BorrowedField<'static>; 11] = [
    BorrowedField::new("node_id", DataType::Integer, 0, 0, true),
    BorrowedField::new("attributes_features", DataType::Pointer, 1019, 0, true),
    BorrowedField::new("owner", DataType::Pointer, 1010, 0, true),
    BorrowedField::new("next", DataType::Pointer, 1008, 0, true),
    BorrowedField::new("previous", DataType::Pointer, 1008, 0, true),
    BorrowedField::new("geometric_owner", DataType::Pointer, 141, 0, true),
    BorrowedField::new("sense", DataType::Char, 0, 0, true),
    BorrowedField::new("surface", DataType::Pointer, 1006, 2, true),
    BorrowedField::new("chart", DataType::Pointer, 40, 0, true),
    BorrowedField::new("start", DataType::Pointer, 41, 0, true),
    BorrowedField::new("end", DataType::Pointer, 41, 0, true),
];

const FIELDS_11004_39: [BorrowedField<'static>; 13] = [
    BorrowedField::new("node_id", DataType::Integer, 0, 0, true),
    BorrowedField::new("attributes_features", DataType::Pointer, 1019, 0, true),
    BorrowedField::new("owner", DataType::Pointer, 1010, 0, true),
    BorrowedField::new("next", DataType::Pointer, 1008, 0, true),
    BorrowedField::new("previous", DataType::Pointer, 1008, 0, true),
    BorrowedField::new("geometric_owner", DataType::Pointer, 141, 0, true),
    BorrowedField::new("sense", DataType::Char, 0, 0, true),
    BorrowedField::new("analytic_root", DataType::Char, 0, 0, true),
    BorrowedField::new("from_infinity", DataType::Logical, 0, 0, true),
    BorrowedField::new("surface", DataType::Pointer, 1006, 0, true),
    BorrowedField::new("start", DataType::Pointer, 41, 0, true),
    BorrowedField::new("end", DataType::Pointer, 41, 0, true),
    BorrowedField::new("eye", DataType::Vector, 0, 0, true),
];

const FIELDS_11004_40: [BorrowedField<'static>; 7] = [
    BorrowedField::new("base_parameter", DataType::Real, 0, 0, true),
    BorrowedField::new("base_scale", DataType::Real, 0, 0, true),
    BorrowedField::new("chart_count", DataType::Integer, 0, 0, true),
    BorrowedField::new("chordal_error", DataType::Real, 0, 0, true),
    BorrowedField::new("angular_error", DataType::Real, 0, 0, true),
    BorrowedField::new("parameter_error", DataType::Real, 0, 2, true),
    BorrowedField::new("hvec", DataType::Hvec, 0, 1, true),
];

const FIELDS_11004_41: [BorrowedField<'static>; 2] = [
    BorrowedField::new("type", DataType::Char, 0, 0, true),
    BorrowedField::new("hvec", DataType::Hvec, 0, 1, true),
];

const FIELDS_11004_42: [BorrowedField<'static>; 2] = [
    BorrowedField::new("t_length", DataType::Real, 0, 0, true),
    BorrowedField::new("bezier_vertices", DataType::Real, 0, 1, true),
];

const FIELDS_11004_43: [BorrowedField<'static>; 5] = [
    BorrowedField::new("knot_vector", DataType::Pointer, 44, 0, true),
    BorrowedField::new("vertex_dimension", DataType::Short, 0, 0, true),
    BorrowedField::new("vertex_count", DataType::Integer, 0, 0, true),
    BorrowedField::new("order", DataType::Short, 0, 0, true),
    BorrowedField::new("bspline_vertices", DataType::Pointer, 45, 0, true),
];

const FIELDS_11004_44: [BorrowedField<'static>; 3] = [
    BorrowedField::new("periodic", DataType::Logical, 0, 0, true),
    BorrowedField::new("knot_count", DataType::Integer, 0, 0, true),
    BorrowedField::new("knots", DataType::Real, 0, 1, true),
];

const FIELDS_11004_45: [BorrowedField<'static>; 1] =
    [BorrowedField::new("vertices", DataType::Real, 0, 1, true)];

const FIELDS_11004_46: [BorrowedField<'static>; 10] = [
    BorrowedField::new("node_id", DataType::Integer, 0, 0, true),
    BorrowedField::new("attributes_features", DataType::Pointer, 1019, 0, true),
    BorrowedField::new("owner", DataType::Pointer, 1010, 0, true),
    BorrowedField::new("next", DataType::Pointer, 1008, 0, true),
    BorrowedField::new("previous", DataType::Pointer, 1008, 0, true),
    BorrowedField::new("geometric_owner", DataType::Pointer, 141, 0, true),
    BorrowedField::new("sense", DataType::Char, 0, 0, true),
    BorrowedField::new("surface", DataType::Pointer, 1006, 0, true),
    BorrowedField::new("curve", DataType::Pointer, 1008, 0, true),
    BorrowedField::new("offset", DataType::Real, 0, 0, true),
];

const FIELDS_11004_47: [BorrowedField<'static>; 4] = [
    BorrowedField::new("split", DataType::Real, 0, 0, true),
    BorrowedField::new("first", DataType::Pointer, 1025, 0, true),
    BorrowedField::new("last", DataType::Pointer, 1025, 0, true),
    BorrowedField::new("t_length", DataType::Real, 0, 0, true),
];

const FIELDS_11004_48: [BorrowedField<'static>; 9] = [
    BorrowedField::new("node_id", DataType::Integer, 0, 0, true),
    BorrowedField::new("attributes_features", DataType::Pointer, 1019, 0, true),
    BorrowedField::new("owner", DataType::Pointer, 1010, 0, true),
    BorrowedField::new("next", DataType::Pointer, 1008, 0, true),
    BorrowedField::new("previous", DataType::Pointer, 1008, 0, true),
    BorrowedField::new("geometric_owner", DataType::Pointer, 141, 0, true),
    BorrowedField::new("sense", DataType::Char, 0, 0, true),
    BorrowedField::new("bezier", DataType::Pointer, 103, 0, true),
    BorrowedField::new("bspline", DataType::Pointer, 43, 0, true),
];

const FIELDS_11004_49: [BorrowedField<'static>; 12] = [
    BorrowedField::new("node_id", DataType::Integer, 0, 0, true),
    BorrowedField::new("attributes_features", DataType::Pointer, 1019, 0, true),
    BorrowedField::new("owner", DataType::Pointer, 1010, 0, true),
    BorrowedField::new("next", DataType::Pointer, 1008, 0, true),
    BorrowedField::new("previous", DataType::Pointer, 1008, 0, true),
    BorrowedField::new("geometric_owner", DataType::Pointer, 141, 0, true),
    BorrowedField::new("sense", DataType::Char, 0, 0, true),
    BorrowedField::new("const_param", DataType::Logical, 0, 0, true),
    BorrowedField::new("segment_count", DataType::Short, 0, 0, true),
    BorrowedField::new("order", DataType::Short, 0, 0, true),
    BorrowedField::new("surface", DataType::Pointer, 1006, 0, true),
    BorrowedField::new("bezier_vertices", DataType::Real, 0, 1, true),
];

const FIELDS_11004_50: [BorrowedField<'static>; 10] = [
    BorrowedField::new("node_id", DataType::Integer, 0, 0, true),
    BorrowedField::new("attributes_features", DataType::Pointer, 1019, 0, true),
    BorrowedField::new("owner", DataType::Pointer, 1007, 0, true),
    BorrowedField::new("next", DataType::Pointer, 1006, 0, true),
    BorrowedField::new("previous", DataType::Pointer, 1006, 0, true),
    BorrowedField::new("geometric_owner", DataType::Pointer, 141, 0, true),
    BorrowedField::new("sense", DataType::Char, 0, 0, true),
    BorrowedField::new("pvec", DataType::Vector, 0, 0, true),
    BorrowedField::new("normal", DataType::Vector, 0, 0, true),
    BorrowedField::new("x_axis", DataType::Vector, 0, 0, true),
];

const FIELDS_11004_51: [BorrowedField<'static>; 11] = [
    BorrowedField::new("node_id", DataType::Integer, 0, 0, true),
    BorrowedField::new("attributes_features", DataType::Pointer, 1019, 0, true),
    BorrowedField::new("owner", DataType::Pointer, 1007, 0, true),
    BorrowedField::new("next", DataType::Pointer, 1006, 0, true),
    BorrowedField::new("previous", DataType::Pointer, 1006, 0, true),
    BorrowedField::new("geometric_owner", DataType::Pointer, 141, 0, true),
    BorrowedField::new("sense", DataType::Char, 0, 0, true),
    BorrowedField::new("pvec", DataType::Vector, 0, 0, true),
    BorrowedField::new("axis", DataType::Vector, 0, 0, true),
    BorrowedField::new("radius", DataType::Real, 0, 0, true),
    BorrowedField::new("x_axis", DataType::Vector, 0, 0, true),
];

const FIELDS_11004_52: [BorrowedField<'static>; 13] = [
    BorrowedField::new("node_id", DataType::Integer, 0, 0, true),
    BorrowedField::new("attributes_features", DataType::Pointer, 1019, 0, true),
    BorrowedField::new("owner", DataType::Pointer, 1007, 0, true),
    BorrowedField::new("next", DataType::Pointer, 1006, 0, true),
    BorrowedField::new("previous", DataType::Pointer, 1006, 0, true),
    BorrowedField::new("geometric_owner", DataType::Pointer, 141, 0, true),
    BorrowedField::new("sense", DataType::Char, 0, 0, true),
    BorrowedField::new("pvec", DataType::Vector, 0, 0, true),
    BorrowedField::new("axis", DataType::Vector, 0, 0, true),
    BorrowedField::new("radius", DataType::Real, 0, 0, true),
    BorrowedField::new("sin_half_angle", DataType::Real, 0, 0, true),
    BorrowedField::new("cos_half_angle", DataType::Real, 0, 0, true),
    BorrowedField::new("x_axis", DataType::Vector, 0, 0, true),
];

const FIELDS_11004_53: [BorrowedField<'static>; 11] = [
    BorrowedField::new("node_id", DataType::Integer, 0, 0, true),
    BorrowedField::new("attributes_features", DataType::Pointer, 1019, 0, true),
    BorrowedField::new("owner", DataType::Pointer, 1007, 0, true),
    BorrowedField::new("next", DataType::Pointer, 1006, 0, true),
    BorrowedField::new("previous", DataType::Pointer, 1006, 0, true),
    BorrowedField::new("geometric_owner", DataType::Pointer, 141, 0, true),
    BorrowedField::new("sense", DataType::Char, 0, 0, true),
    BorrowedField::new("centre", DataType::Vector, 0, 0, true),
    BorrowedField::new("radius", DataType::Real, 0, 0, true),
    BorrowedField::new("axis", DataType::Vector, 0, 0, true),
    BorrowedField::new("x_axis", DataType::Vector, 0, 0, true),
];

const FIELDS_11004_54: [BorrowedField<'static>; 12] = [
    BorrowedField::new("node_id", DataType::Integer, 0, 0, true),
    BorrowedField::new("attributes_features", DataType::Pointer, 1019, 0, true),
    BorrowedField::new("owner", DataType::Pointer, 1007, 0, true),
    BorrowedField::new("next", DataType::Pointer, 1006, 0, true),
    BorrowedField::new("previous", DataType::Pointer, 1006, 0, true),
    BorrowedField::new("geometric_owner", DataType::Pointer, 141, 0, true),
    BorrowedField::new("sense", DataType::Char, 0, 0, true),
    BorrowedField::new("centre", DataType::Vector, 0, 0, true),
    BorrowedField::new("axis", DataType::Vector, 0, 0, true),
    BorrowedField::new("major_radius", DataType::Real, 0, 0, true),
    BorrowedField::new("minor_radius", DataType::Real, 0, 0, true),
    BorrowedField::new("x_axis", DataType::Vector, 0, 0, true),
];

const FIELDS_11004_55: [BorrowedField<'static>; 9] = [
    BorrowedField::new("node_id", DataType::Integer, 0, 0, true),
    BorrowedField::new("attributes_features", DataType::Pointer, 1019, 0, true),
    BorrowedField::new("owner", DataType::Pointer, 1007, 0, true),
    BorrowedField::new("next", DataType::Pointer, 1006, 0, true),
    BorrowedField::new("previous", DataType::Pointer, 1006, 0, true),
    BorrowedField::new("geometric_owner", DataType::Pointer, 141, 0, true),
    BorrowedField::new("sense", DataType::Char, 0, 0, true),
    BorrowedField::new("spine", DataType::Pointer, 1008, 0, true),
    BorrowedField::new("radius", DataType::Real, 0, 0, true),
];

const FIELDS_11004_56: [BorrowedField<'static>; 15] = [
    BorrowedField::new("node_id", DataType::Integer, 0, 0, true),
    BorrowedField::new("attributes_features", DataType::Pointer, 1019, 0, true),
    BorrowedField::new("owner", DataType::Pointer, 1007, 0, true),
    BorrowedField::new("next", DataType::Pointer, 1006, 0, true),
    BorrowedField::new("previous", DataType::Pointer, 1006, 0, true),
    BorrowedField::new("geometric_owner", DataType::Pointer, 141, 0, true),
    BorrowedField::new("sense", DataType::Char, 0, 0, true),
    BorrowedField::new("blend_type", DataType::Char, 0, 0, true),
    BorrowedField::new("surface", DataType::Pointer, 1006, 2, true),
    BorrowedField::new("spine", DataType::Pointer, 1008, 0, true),
    BorrowedField::new("range", DataType::Real, 0, 2, true),
    BorrowedField::new("thumb_weight", DataType::Real, 0, 2, true),
    BorrowedField::new("boundary", DataType::Pointer, 1006, 2, true),
    BorrowedField::new("start", DataType::Pointer, 41, 0, true),
    BorrowedField::new("end", DataType::Pointer, 41, 0, true),
];

const FIELDS_11004_57: [BorrowedField<'static>; 14] = [
    BorrowedField::new("node_id", DataType::Integer, 0, 0, true),
    BorrowedField::new("attributes_features", DataType::Pointer, 1019, 0, true),
    BorrowedField::new("owner", DataType::Pointer, 1007, 0, true),
    BorrowedField::new("next", DataType::Pointer, 1006, 0, true),
    BorrowedField::new("previous", DataType::Pointer, 1006, 0, true),
    BorrowedField::new("geometric_owner", DataType::Pointer, 141, 0, true),
    BorrowedField::new("sense", DataType::Char, 0, 0, true),
    BorrowedField::new("blend_type", DataType::Char, 0, 0, true),
    BorrowedField::new("surface", DataType::Pointer, 1006, 3, true),
    BorrowedField::new("sub_surface", DataType::Pointer, 1006, 3, true),
    BorrowedField::new("boundary", DataType::Pointer, 1006, 3, true),
    BorrowedField::new("range", DataType::Real, 0, 3, true),
    BorrowedField::new("thumb_weight", DataType::Real, 0, 3, true),
    BorrowedField::new("centre", DataType::Vector, 0, 0, true),
];

const FIELDS_11004_58: [BorrowedField<'static>; 14] = [
    BorrowedField::new("node_id", DataType::Integer, 0, 0, true),
    BorrowedField::new("attributes_features", DataType::Pointer, 1019, 0, true),
    BorrowedField::new("owner", DataType::Pointer, 1007, 0, true),
    BorrowedField::new("next", DataType::Pointer, 1006, 0, true),
    BorrowedField::new("previous", DataType::Pointer, 1006, 0, true),
    BorrowedField::new("geometric_owner", DataType::Pointer, 141, 0, true),
    BorrowedField::new("sense", DataType::Char, 0, 0, true),
    BorrowedField::new("surface", DataType::Pointer, 1006, 2, true),
    BorrowedField::new("sub_surface", DataType::Pointer, 1006, 4, true),
    BorrowedField::new("range", DataType::Real, 0, 4, true),
    BorrowedField::new("thumb_weight", DataType::Real, 0, 4, true),
    BorrowedField::new("blend_type", DataType::Char, 0, 2, true),
    BorrowedField::new("overlap_type", DataType::Char, 0, 0, true),
    BorrowedField::new("swap_u_v", DataType::Logical, 0, 0, true),
];

const FIELDS_11004_59: [BorrowedField<'static>; 9] = [
    BorrowedField::new("node_id", DataType::Integer, 0, 0, true),
    BorrowedField::new("attributes_features", DataType::Pointer, 1019, 0, true),
    BorrowedField::new("owner", DataType::Pointer, 1007, 0, true),
    BorrowedField::new("next", DataType::Pointer, 1006, 0, true),
    BorrowedField::new("previous", DataType::Pointer, 1006, 0, true),
    BorrowedField::new("geometric_owner", DataType::Pointer, 141, 0, true),
    BorrowedField::new("sense", DataType::Char, 0, 0, true),
    BorrowedField::new("boundary", DataType::Short, 0, 0, true),
    BorrowedField::new("blend", DataType::Pointer, 1006, 0, true),
];

const FIELDS_11004_60: [BorrowedField<'static>; 12] = [
    BorrowedField::new("node_id", DataType::Integer, 0, 0, true),
    BorrowedField::new("attributes_features", DataType::Pointer, 1019, 0, true),
    BorrowedField::new("owner", DataType::Pointer, 1007, 0, true),
    BorrowedField::new("next", DataType::Pointer, 1006, 0, true),
    BorrowedField::new("previous", DataType::Pointer, 1006, 0, true),
    BorrowedField::new("geometric_owner", DataType::Pointer, 141, 0, true),
    BorrowedField::new("sense", DataType::Char, 0, 0, true),
    BorrowedField::new("check", DataType::Char, 0, 0, true),
    BorrowedField::new("true_offset", DataType::Logical, 0, 0, true),
    BorrowedField::new("surface", DataType::Pointer, 1006, 0, true),
    BorrowedField::new("offset", DataType::Real, 0, 0, true),
    BorrowedField::new("scale", DataType::Real, 0, 0, true),
];

const FIELDS_11004_61: [BorrowedField<'static>; 10] = [
    BorrowedField::new("node_id", DataType::Integer, 0, 0, true),
    BorrowedField::new("attributes_features", DataType::Pointer, 1019, 0, true),
    BorrowedField::new("owner", DataType::Pointer, 1007, 0, true),
    BorrowedField::new("next", DataType::Pointer, 1006, 0, true),
    BorrowedField::new("previous", DataType::Pointer, 1006, 0, true),
    BorrowedField::new("geometric_owner", DataType::Pointer, 141, 0, true),
    BorrowedField::new("sense", DataType::Char, 0, 0, true),
    BorrowedField::new("col", DataType::Integer, 0, 0, true),
    BorrowedField::new("row", DataType::Integer, 0, 0, true),
    BorrowedField::new("cps", DataType::Pointer, 69, 0, true),
];

const FIELDS_11004_62: [BorrowedField<'static>; 13] = [
    BorrowedField::new("node_id", DataType::Integer, 0, 0, true),
    BorrowedField::new("attributes_features", DataType::Pointer, 1019, 0, true),
    BorrowedField::new("owner", DataType::Pointer, 1007, 0, true),
    BorrowedField::new("next", DataType::Pointer, 1006, 0, true),
    BorrowedField::new("previous", DataType::Pointer, 1006, 0, true),
    BorrowedField::new("geometric_owner", DataType::Pointer, 141, 0, true),
    BorrowedField::new("sense", DataType::Char, 0, 0, true),
    BorrowedField::new("vertex_dimension", DataType::Short, 0, 0, true),
    BorrowedField::new("col_count", DataType::Short, 0, 0, true),
    BorrowedField::new("row_count", DataType::Short, 0, 0, true),
    BorrowedField::new("u_order", DataType::Short, 0, 0, true),
    BorrowedField::new("v_order", DataType::Short, 0, 0, true),
    BorrowedField::new("patch", DataType::Pointer, 1026, 1, true),
];

const FIELDS_11004_63: [BorrowedField<'static>; 10] = [
    BorrowedField::new("node_id", DataType::Integer, 0, 0, true),
    BorrowedField::new("attributes_features", DataType::Pointer, 1019, 0, true),
    BorrowedField::new("owner", DataType::Pointer, 1007, 0, true),
    BorrowedField::new("next", DataType::Pointer, 1006, 0, true),
    BorrowedField::new("previous", DataType::Pointer, 1006, 0, true),
    BorrowedField::new("geometric_owner", DataType::Pointer, 141, 0, true),
    BorrowedField::new("sense", DataType::Char, 0, 0, true),
    BorrowedField::new("from_infinity", DataType::Logical, 0, 0, true),
    BorrowedField::new("surface", DataType::Pointer, 1006, 0, true),
    BorrowedField::new("eye", DataType::Vector, 0, 0, true),
];

const FIELDS_11004_64: [BorrowedField<'static>; 3] = [
    BorrowedField::new("u_length", DataType::Real, 0, 0, true),
    BorrowedField::new("v_length", DataType::Real, 0, 0, true),
    BorrowedField::new("bezier_vertices", DataType::Real, 0, 1, true),
];

const FIELDS_11004_65: [BorrowedField<'static>; 4] = [
    BorrowedField::new("dimension", DataType::Short, 0, 0, true),
    BorrowedField::new("plane_count", DataType::Short, 0, 0, true),
    BorrowedField::new("corner_count", DataType::Short, 0, 0, true),
    BorrowedField::new("vecs", DataType::Vector, 0, 1, true),
];

const FIELDS_11004_66: [BorrowedField<'static>; 8] = [
    BorrowedField::new("row_knots", DataType::Pointer, 44, 0, true),
    BorrowedField::new("col_knots", DataType::Pointer, 44, 0, true),
    BorrowedField::new("vertex_dimension", DataType::Short, 0, 0, true),
    BorrowedField::new("col_count", DataType::Integer, 0, 0, true),
    BorrowedField::new("row_count", DataType::Integer, 0, 0, true),
    BorrowedField::new("u_order", DataType::Short, 0, 0, true),
    BorrowedField::new("v_order", DataType::Short, 0, 0, true),
    BorrowedField::new("bspline_vertices", DataType::Pointer, 45, 0, true),
];

const FIELDS_11004_67: [BorrowedField<'static>; 10] = [
    BorrowedField::new("node_id", DataType::Integer, 0, 0, true),
    BorrowedField::new("attributes_features", DataType::Pointer, 1019, 0, true),
    BorrowedField::new("owner", DataType::Pointer, 1007, 0, true),
    BorrowedField::new("next", DataType::Pointer, 1006, 0, true),
    BorrowedField::new("previous", DataType::Pointer, 1006, 0, true),
    BorrowedField::new("geometric_owner", DataType::Pointer, 141, 0, true),
    BorrowedField::new("sense", DataType::Char, 0, 0, true),
    BorrowedField::new("section", DataType::Pointer, 1008, 0, true),
    BorrowedField::new("sweep", DataType::Vector, 0, 0, true),
    BorrowedField::new("scale", DataType::Real, 0, 0, true),
];

const FIELDS_11004_68: [BorrowedField<'static>; 16] = [
    BorrowedField::new("node_id", DataType::Integer, 0, 0, true),
    BorrowedField::new("attributes_features", DataType::Pointer, 1019, 0, true),
    BorrowedField::new("owner", DataType::Pointer, 1007, 0, true),
    BorrowedField::new("next", DataType::Pointer, 1006, 0, true),
    BorrowedField::new("previous", DataType::Pointer, 1006, 0, true),
    BorrowedField::new("geometric_owner", DataType::Pointer, 141, 0, true),
    BorrowedField::new("sense", DataType::Char, 0, 0, true),
    BorrowedField::new("profile", DataType::Pointer, 1008, 0, true),
    BorrowedField::new("base", DataType::Vector, 0, 0, true),
    BorrowedField::new("axis", DataType::Vector, 0, 0, true),
    BorrowedField::new("start", DataType::Vector, 0, 0, true),
    BorrowedField::new("end", DataType::Vector, 0, 0, true),
    BorrowedField::new("start_param", DataType::Real, 0, 0, true),
    BorrowedField::new("end_param", DataType::Real, 0, 0, true),
    BorrowedField::new("x_axis", DataType::Vector, 0, 0, true),
    BorrowedField::new("scale", DataType::Real, 0, 0, true),
];

const FIELDS_11004_69: [BorrowedField<'static>; 9] = [
    BorrowedField::new("node_id", DataType::Integer, 0, 0, true),
    BorrowedField::new("attributes_features", DataType::Pointer, 1019, 0, true),
    BorrowedField::new("owner", DataType::Pointer, 1007, 0, true),
    BorrowedField::new("next", DataType::Pointer, 1006, 0, true),
    BorrowedField::new("previous", DataType::Pointer, 1006, 0, true),
    BorrowedField::new("geometric_owner", DataType::Pointer, 141, 0, true),
    BorrowedField::new("sense", DataType::Char, 0, 0, true),
    BorrowedField::new("bezier", DataType::Pointer, 104, 0, true),
    BorrowedField::new("bspline", DataType::Pointer, 66, 0, true),
];

const FIELDS_11004_70: [BorrowedField<'static>; 12] = [
    BorrowedField::new("node_id", DataType::Integer, 0, 0, true),
    BorrowedField::new("owner", DataType::Pointer, 1013, 0, true),
    BorrowedField::new("next", DataType::Pointer, 70, 0, true),
    BorrowedField::new("previous", DataType::Pointer, 70, 0, true),
    BorrowedField::new("list_type", DataType::Integer, 0, 0, true),
    BorrowedField::new("list_length", DataType::Integer, 0, 0, true),
    BorrowedField::new("block_length", DataType::Integer, 0, 0, true),
    BorrowedField::new("size_of_entry", DataType::Integer, 0, 0, true),
    BorrowedField::new("list_block", DataType::Pointer, 1012, 0, true),
    BorrowedField::new("finger_block", DataType::Pointer, 1012, 0, true),
    BorrowedField::new("finger_index", DataType::Integer, 0, 0, true),
    BorrowedField::new("notransmit", DataType::Logical, 0, 0, true),
];

const FIELDS_11004_71: [BorrowedField<'static>; 3] = [
    BorrowedField::new("n_entries", DataType::Integer, 0, 0, true),
    BorrowedField::new("next_block", DataType::Pointer, 71, 0, true),
    BorrowedField::new("entries", DataType::Real, 0, 1, true),
];

const FIELDS_11004_72: [BorrowedField<'static>; 3] = [
    BorrowedField::new("n_entries", DataType::Integer, 0, 0, true),
    BorrowedField::new("next_block", DataType::Pointer, 72, 0, true),
    BorrowedField::new("entries", DataType::Integer, 0, 1, true),
];

const FIELDS_11004_73: [BorrowedField<'static>; 3] = [
    BorrowedField::new("n_entries", DataType::Integer, 0, 0, true),
    BorrowedField::new("next_block", DataType::Pointer, 73, 0, true),
    BorrowedField::new("entries", DataType::Unestablished(b't'), 0, 1, true),
];

const FIELDS_11004_74: [BorrowedField<'static>; 3] = [
    BorrowedField::new("n_entries", DataType::Integer, 0, 0, true),
    BorrowedField::new("next_block", DataType::Pointer, 74, 0, true),
    BorrowedField::new("entries", DataType::Pointer, 1001, 1, true),
];

const FIELDS_11004_78: [BorrowedField<'static>; 12] = [
    BorrowedField::new("split_fn", DataType::Unestablished(b'q'), 0, 0, true),
    BorrowedField::new("merge_fn", DataType::Unestablished(b'q'), 0, 0, true),
    BorrowedField::new("delete_fn", DataType::Unestablished(b'q'), 0, 0, true),
    BorrowedField::new("copy_fn", DataType::Unestablished(b'q'), 0, 0, true),
    BorrowedField::new("transmit_fn", DataType::Unestablished(b'q'), 0, 0, true),
    BorrowedField::new("receive_fn", DataType::Unestablished(b'q'), 0, 0, true),
    BorrowedField::new("split_on", DataType::Logical, 0, 0, true),
    BorrowedField::new("merge_on", DataType::Logical, 0, 0, true),
    BorrowedField::new("delete_on", DataType::Logical, 0, 0, true),
    BorrowedField::new("copy_on", DataType::Logical, 0, 0, true),
    BorrowedField::new("transmit_on", DataType::Logical, 0, 0, true),
    BorrowedField::new("receive_on", DataType::Logical, 0, 0, true),
];

const FIELDS_11004_79: [BorrowedField<'static>; 1] =
    [BorrowedField::new("string", DataType::Char, 0, 1, true)];

const FIELDS_11004_80: [BorrowedField<'static>; 6] = [
    BorrowedField::new("next", DataType::Pointer, 80, 0, true),
    BorrowedField::new("identifier", DataType::Pointer, 79, 0, true),
    BorrowedField::new("type_id", DataType::Integer, 0, 0, true),
    BorrowedField::new("actions", DataType::Byte, 0, 8, true),
    BorrowedField::new("legal_owners", DataType::Logical, 0, 14, true),
    BorrowedField::new("fields", DataType::Byte, 0, 1, true),
];

const FIELDS_11004_81: [BorrowedField<'static>; 8] = [
    BorrowedField::new("node_id", DataType::Integer, 0, 0, true),
    BorrowedField::new("definition", DataType::Pointer, 80, 0, true),
    BorrowedField::new("owner", DataType::Pointer, 1015, 0, true),
    BorrowedField::new("next", DataType::Pointer, 1019, 0, true),
    BorrowedField::new("previous", DataType::Pointer, 1019, 0, true),
    BorrowedField::new("next_of_type", DataType::Pointer, 81, 0, true),
    BorrowedField::new("previous_of_type", DataType::Pointer, 81, 0, true),
    BorrowedField::new("fields", DataType::Pointer, 1018, 1, true),
];

const FIELDS_11004_82: [BorrowedField<'static>; 1] =
    [BorrowedField::new("values", DataType::Integer, 0, 1, true)];

const FIELDS_11004_83: [BorrowedField<'static>; 1] =
    [BorrowedField::new("values", DataType::Real, 0, 1, true)];

const FIELDS_11004_84: [BorrowedField<'static>; 1] =
    [BorrowedField::new("values", DataType::Char, 0, 1, true)];

const FIELDS_11004_85: [BorrowedField<'static>; 1] =
    [BorrowedField::new("values", DataType::Vector, 0, 1, true)];

const FIELDS_11004_88: [BorrowedField<'static>; 1] = [BorrowedField::new(
    "values",
    DataType::Unestablished(b't'),
    0,
    1,
    true,
)];

const FIELDS_11004_90: [BorrowedField<'static>; 7] = [
    BorrowedField::new("node_id", DataType::Integer, 0, 0, true),
    BorrowedField::new("attributes_features", DataType::Pointer, 1019, 0, true),
    BorrowedField::new("owner", DataType::Pointer, 1005, 0, true),
    BorrowedField::new("next", DataType::Pointer, 1019, 0, true),
    BorrowedField::new("previous", DataType::Pointer, 1019, 0, true),
    BorrowedField::new("type", DataType::Byte, 0, 0, true),
    BorrowedField::new("first_member", DataType::Pointer, 91, 0, true),
];

const FIELDS_11004_91: [BorrowedField<'static>; 7] = [
    BorrowedField::new("dummy_node_id", DataType::Integer, 0, 0, true),
    BorrowedField::new("owning_feature", DataType::Pointer, 90, 0, true),
    BorrowedField::new("owner", DataType::Pointer, 1017, 0, true),
    BorrowedField::new("next", DataType::Pointer, 1019, 0, true),
    BorrowedField::new("previous", DataType::Pointer, 1019, 0, true),
    BorrowedField::new("next_member", DataType::Pointer, 91, 0, true),
    BorrowedField::new("previous_member", DataType::Pointer, 91, 0, true),
];

const FIELDS_11004_96: [BorrowedField<'static>; 1] =
    [BorrowedField::new("values", DataType::Short, 0, 1, true)];

const FIELDS_11004_97: [BorrowedField<'static>; 1] =
    [BorrowedField::new("values", DataType::Box, 0, 1, true)];

const FIELDS_11004_100: [BorrowedField<'static>; 9] = [
    BorrowedField::new("node_id", DataType::Integer, 0, 0, true),
    BorrowedField::new("owner", DataType::Pointer, 1023, 0, true),
    BorrowedField::new("next", DataType::Pointer, 100, 0, true),
    BorrowedField::new("previous", DataType::Pointer, 100, 0, true),
    BorrowedField::new("rotation_matrix", DataType::Real, 0, 9, true),
    BorrowedField::new("translation_vector", DataType::Vector, 0, 0, true),
    BorrowedField::new("scale", DataType::Real, 0, 0, true),
    BorrowedField::new("flag", DataType::Integer, 0, 0, true),
    BorrowedField::new("perspective_vector", DataType::Vector, 0, 0, true),
];

const FIELDS_11004_101: [BorrowedField<'static>; 11] = [
    BorrowedField::new("assembly", DataType::Pointer, 10, 0, true),
    BorrowedField::new("attribute", DataType::Pointer, 81, 0, true),
    BorrowedField::new("body", DataType::Pointer, 12, 0, true),
    BorrowedField::new("transform", DataType::Pointer, 100, 0, true),
    BorrowedField::new("surface", DataType::Pointer, 1006, 0, true),
    BorrowedField::new("curve", DataType::Pointer, 1008, 0, true),
    BorrowedField::new("point", DataType::Pointer, 29, 0, true),
    BorrowedField::new("alive", DataType::Logical, 0, 0, true),
    BorrowedField::new("attrib_def", DataType::Pointer, 80, 0, true),
    BorrowedField::new("highest_id", DataType::Integer, 0, 0, true),
    BorrowedField::new("current_id", DataType::Integer, 0, 0, true),
];

const FIELDS_11004_103: [BorrowedField<'static>; 5] = [
    BorrowedField::new("vertex_dimension", DataType::Short, 0, 0, true),
    BorrowedField::new("segment_count", DataType::Integer, 0, 0, true),
    BorrowedField::new("order", DataType::Short, 0, 0, true),
    BorrowedField::new("check", DataType::Char, 0, 0, true),
    BorrowedField::new("segment", DataType::Pointer, 1025, 1, true),
];

const FIELDS_11004_104: [BorrowedField<'static>; 7] = [
    BorrowedField::new("vertex_dimension", DataType::Short, 0, 0, true),
    BorrowedField::new("col_count", DataType::Integer, 0, 0, true),
    BorrowedField::new("row_count", DataType::Integer, 0, 0, true),
    BorrowedField::new("u_order", DataType::Short, 0, 0, true),
    BorrowedField::new("v_order", DataType::Short, 0, 0, true),
    BorrowedField::new("check", DataType::Char, 0, 0, true),
    BorrowedField::new("patch", DataType::Pointer, 1026, 1, true),
];

const FIELDS_11004_105: [BorrowedField<'static>; 6] = [
    BorrowedField::new("split", DataType::Real, 0, 0, true),
    BorrowedField::new("split_dir", DataType::Char, 0, 0, true),
    BorrowedField::new("first", DataType::Pointer, 1026, 0, true),
    BorrowedField::new("last", DataType::Pointer, 1026, 0, true),
    BorrowedField::new("u_length", DataType::Real, 0, 0, true),
    BorrowedField::new("v_length", DataType::Real, 0, 0, true),
];

const FIELDS_11004_106: [BorrowedField<'static>; 15] = [
    BorrowedField::new("face", DataType::Logical, 0, 0, true),
    BorrowedField::new("edge", DataType::Logical, 0, 0, true),
    BorrowedField::new("vertex", DataType::Logical, 0, 0, true),
    BorrowedField::new("loop", DataType::Logical, 0, 0, true),
    BorrowedField::new("shell", DataType::Logical, 0, 0, true),
    BorrowedField::new("region", DataType::Logical, 0, 0, true),
    BorrowedField::new("instance", DataType::Logical, 0, 0, true),
    BorrowedField::new("body", DataType::Logical, 0, 0, true),
    BorrowedField::new("assembly", DataType::Logical, 0, 0, true),
    BorrowedField::new("point", DataType::Logical, 0, 0, true),
    BorrowedField::new("curve", DataType::Logical, 0, 0, true),
    BorrowedField::new("surface", DataType::Logical, 0, 0, true),
    BorrowedField::new("transform", DataType::Logical, 0, 0, true),
    BorrowedField::new("feature", DataType::Logical, 0, 0, true),
    BorrowedField::new("att_def", DataType::Logical, 0, 0, true),
];

const FIELDS_11004_107: [BorrowedField<'static>; 14] = [
    BorrowedField::new("create", DataType::Pointer, 106, 0, true),
    BorrowedField::new("delete", DataType::Pointer, 106, 0, true),
    BorrowedField::new("change", DataType::Pointer, 106, 0, true),
    BorrowedField::new("transform", DataType::Pointer, 106, 0, true),
    BorrowedField::new("split", DataType::Pointer, 106, 0, true),
    BorrowedField::new("merge", DataType::Pointer, 106, 0, true),
    BorrowedField::new("copy", DataType::Pointer, 106, 0, true),
    BorrowedField::new("transfer", DataType::Pointer, 106, 0, true),
    BorrowedField::new("change_attribute", DataType::Pointer, 106, 0, true),
    BorrowedField::new("event", DataType::Pointer, 70, 0, true),
    BorrowedField::new("entities", DataType::Pointer, 70, 0, true),
    BorrowedField::new("n_entities", DataType::Pointer, 70, 0, true),
    BorrowedField::new("entity_nt", DataType::Pointer, 70, 0, true),
    BorrowedField::new("entity_usfld", DataType::Pointer, 70, 0, true),
];

const FIELDS_11004_108: [BorrowedField<'static>; 1] = [BorrowedField::new(
    "bad_patch",
    DataType::Pointer,
    64,
    1,
    true,
)];

const FIELDS_11004_109: [BorrowedField<'static>; 4] = [
    BorrowedField::new("type", DataType::Char, 0, 0, true),
    BorrowedField::new("sense", DataType::Char, 0, 0, true),
    BorrowedField::new("u_int", DataType::Interval, 0, 0, true),
    BorrowedField::new("v_int", DataType::Interval, 0, 0, true),
];

const FIELDS_11004_110: [BorrowedField<'static>; 6] = [
    BorrowedField::new("next", DataType::Pointer, 110, 0, true),
    BorrowedField::new("forward", DataType::Pointer, 110, 0, true),
    BorrowedField::new("backward", DataType::Pointer, 110, 0, true),
    BorrowedField::new("class", DataType::Integer, 0, 0, true),
    BorrowedField::new("set", DataType::Pointer, 1021, 0, true),
    BorrowedField::new("node", DataType::Pointer, 1022, 0, true),
];

const FIELDS_11004_111: [BorrowedField<'static>; 5] = [
    BorrowedField::new("tag", DataType::Pointer, 110, 0, true),
    BorrowedField::new("next", DataType::Pointer, 111, 0, true),
    BorrowedField::new("class", DataType::Integer, 0, 0, true),
    BorrowedField::new("he_set", DataType::Pointer, 112, 0, true),
    BorrowedField::new("surfaces", DataType::Pointer, 1006, 0, true),
];

const FIELDS_11004_112: [BorrowedField<'static>; 6] = [
    BorrowedField::new("tag", DataType::Pointer, 110, 0, true),
    BorrowedField::new("next", DataType::Pointer, 112, 0, true),
    BorrowedField::new("previous", DataType::Pointer, 112, 0, true),
    BorrowedField::new("class", DataType::Integer, 0, 0, true),
    BorrowedField::new("fa_set", DataType::Pointer, 111, 0, true),
    BorrowedField::new("co_he_set", DataType::Pointer, 112, 0, true),
];

const FIELDS_11004_120: [BorrowedField<'static>; 11] = [
    BorrowedField::new("node_id", DataType::Integer, 0, 0, true),
    BorrowedField::new("attributes_features", DataType::Pointer, 1019, 0, true),
    BorrowedField::new("owner", DataType::Pointer, 1007, 0, true),
    BorrowedField::new("next", DataType::Pointer, 1006, 0, true),
    BorrowedField::new("previous", DataType::Pointer, 1006, 0, true),
    BorrowedField::new("geometric_owner", DataType::Pointer, 141, 0, true),
    BorrowedField::new("sense", DataType::Char, 0, 0, true),
    BorrowedField::new("type", DataType::Char, 0, 0, true),
    BorrowedField::new("data", DataType::Pointer, 1027, 0, true),
    BorrowedField::new("tf", DataType::Pointer, 100, 0, true),
    BorrowedField::new("internal_geom", DataType::Pointer, 1028, 1, true),
];

const FIELDS_11004_121: [BorrowedField<'static>; 3] = [
    BorrowedField::new("geom_type", DataType::Integer, 0, 0, true),
    BorrowedField::new("real_array", DataType::Pointer, 83, 0, true),
    BorrowedField::new("int_array", DataType::Pointer, 82, 0, true),
];

const FIELDS_11004_122: [BorrowedField<'static>; 4] = [
    BorrowedField::new("key", DataType::Pointer, 102, 0, true),
    BorrowedField::new("real_array", DataType::Pointer, 83, 0, true),
    BorrowedField::new("int_array", DataType::Pointer, 82, 0, true),
    BorrowedField::new("data", DataType::Real, 0, 1, false),
];

const FIELDS_11004_123: [BorrowedField<'static>; 6] = [
    BorrowedField::new("uint", DataType::Interval, 0, 0, true),
    BorrowedField::new("vint", DataType::Interval, 0, 0, true),
    BorrowedField::new("u_start", DataType::Char, 0, 0, true),
    BorrowedField::new("u_end", DataType::Char, 0, 0, true),
    BorrowedField::new("v_start", DataType::Char, 0, 0, true),
    BorrowedField::new("v_end", DataType::Char, 0, 0, true),
];

const FIELDS_11004_124: [BorrowedField<'static>; 9] = [
    BorrowedField::new("node_id", DataType::Integer, 0, 0, true),
    BorrowedField::new("attributes_features", DataType::Pointer, 1019, 0, true),
    BorrowedField::new("owner", DataType::Pointer, 1007, 0, true),
    BorrowedField::new("next", DataType::Pointer, 1006, 0, true),
    BorrowedField::new("previous", DataType::Pointer, 1006, 0, true),
    BorrowedField::new("geometric_owner", DataType::Pointer, 141, 0, true),
    BorrowedField::new("sense", DataType::Char, 0, 0, true),
    BorrowedField::new("nurbs", DataType::Pointer, 126, 0, true),
    BorrowedField::new("data", DataType::Pointer, 125, 0, true),
];

const FIELDS_11004_125: [BorrowedField<'static>; 21] = [
    BorrowedField::new("original_uint", DataType::Interval, 0, 0, true),
    BorrowedField::new("original_vint", DataType::Interval, 0, 0, true),
    BorrowedField::new("extended_uint", DataType::Interval, 0, 0, true),
    BorrowedField::new("extended_vint", DataType::Interval, 0, 0, true),
    BorrowedField::new("self_int", DataType::Byte, 0, 0, true),
    BorrowedField::new("original_u_start", DataType::Char, 0, 0, true),
    BorrowedField::new("original_u_end", DataType::Char, 0, 0, true),
    BorrowedField::new("original_v_start", DataType::Char, 0, 0, true),
    BorrowedField::new("original_v_end", DataType::Char, 0, 0, true),
    BorrowedField::new("extended_u_start", DataType::Char, 0, 0, true),
    BorrowedField::new("extended_u_end", DataType::Char, 0, 0, true),
    BorrowedField::new("extended_v_start", DataType::Char, 0, 0, true),
    BorrowedField::new("extended_v_end", DataType::Char, 0, 0, true),
    BorrowedField::new("analytic_form_type", DataType::Char, 0, 0, true),
    BorrowedField::new("swept_form_type", DataType::Char, 0, 0, true),
    BorrowedField::new("spun_form_type", DataType::Char, 0, 0, true),
    BorrowedField::new("blend_form_type", DataType::Char, 0, 0, true),
    BorrowedField::new("analytic_form", DataType::Pointer, 1030, 0, true),
    BorrowedField::new("swept_form", DataType::Pointer, 1031, 0, true),
    BorrowedField::new("spun_form", DataType::Pointer, 1032, 0, true),
    BorrowedField::new("blend_form", DataType::Pointer, 1033, 0, true),
];

const FIELDS_11004_126: [BorrowedField<'static>; 20] = [
    BorrowedField::new("u_periodic", DataType::Logical, 0, 0, true),
    BorrowedField::new("v_periodic", DataType::Logical, 0, 0, true),
    BorrowedField::new("u_degree", DataType::Short, 0, 0, true),
    BorrowedField::new("v_degree", DataType::Short, 0, 0, true),
    BorrowedField::new("n_u_vertices", DataType::Integer, 0, 0, true),
    BorrowedField::new("n_v_vertices", DataType::Integer, 0, 0, true),
    BorrowedField::new("u_knot_type", DataType::Byte, 0, 0, true),
    BorrowedField::new("v_knot_type", DataType::Byte, 0, 0, true),
    BorrowedField::new("n_u_knots", DataType::Integer, 0, 0, true),
    BorrowedField::new("n_v_knots", DataType::Integer, 0, 0, true),
    BorrowedField::new("rational", DataType::Logical, 0, 0, true),
    BorrowedField::new("u_closed", DataType::Logical, 0, 0, true),
    BorrowedField::new("v_closed", DataType::Logical, 0, 0, true),
    BorrowedField::new("surface_form", DataType::Byte, 0, 0, true),
    BorrowedField::new("vertex_dim", DataType::Short, 0, 0, true),
    BorrowedField::new("bspline_vertices", DataType::Pointer, 45, 0, true),
    BorrowedField::new("u_knot_mult", DataType::Pointer, 127, 0, true),
    BorrowedField::new("v_knot_mult", DataType::Pointer, 127, 0, true),
    BorrowedField::new("u_knots", DataType::Pointer, 128, 0, true),
    BorrowedField::new("v_knots", DataType::Pointer, 128, 0, true),
];

const FIELDS_11004_127: [BorrowedField<'static>; 1] =
    [BorrowedField::new("mult", DataType::Short, 0, 1, true)];

const FIELDS_11004_128: [BorrowedField<'static>; 1] =
    [BorrowedField::new("knots", DataType::Real, 0, 1, true)];

const FIELDS_11004_130: [BorrowedField<'static>; 11] = [
    BorrowedField::new("node_id", DataType::Integer, 0, 0, true),
    BorrowedField::new("attributes_features", DataType::Pointer, 1019, 0, true),
    BorrowedField::new("owner", DataType::Pointer, 1010, 0, true),
    BorrowedField::new("next", DataType::Pointer, 1008, 0, true),
    BorrowedField::new("previous", DataType::Pointer, 1008, 0, true),
    BorrowedField::new("geometric_owner", DataType::Pointer, 141, 0, true),
    BorrowedField::new("sense", DataType::Char, 0, 0, true),
    BorrowedField::new("type", DataType::Char, 0, 0, true),
    BorrowedField::new("data", DataType::Pointer, 1027, 0, true),
    BorrowedField::new("tf", DataType::Pointer, 100, 0, true),
    BorrowedField::new("internal_geom", DataType::Pointer, 1028, 1, true),
];

const FIELDS_11004_131: [BorrowedField<'static>; 3] = [
    BorrowedField::new("tint", DataType::Interval, 0, 0, true),
    BorrowedField::new("t_start", DataType::Char, 0, 0, true),
    BorrowedField::new("t_end", DataType::Char, 0, 0, true),
];

const FIELDS_11004_132: [BorrowedField<'static>; 10] = [
    BorrowedField::new("node_id", DataType::Integer, 0, 0, true),
    BorrowedField::new("attributes_features", DataType::Pointer, 1019, 0, true),
    BorrowedField::new("owner", DataType::Pointer, 1010, 0, true),
    BorrowedField::new("next", DataType::Pointer, 1008, 0, true),
    BorrowedField::new("previous", DataType::Pointer, 1008, 0, true),
    BorrowedField::new("geometric_owner", DataType::Pointer, 141, 0, true),
    BorrowedField::new("sense", DataType::Char, 0, 0, true),
    BorrowedField::new("bsp_parms", DataType::Logical, 0, 0, true),
    BorrowedField::new("surface", DataType::Pointer, 1006, 0, true),
    BorrowedField::new("bspline", DataType::Pointer, 43, 0, true),
];

const FIELDS_11004_133: [BorrowedField<'static>; 12] = [
    BorrowedField::new("node_id", DataType::Integer, 0, 0, true),
    BorrowedField::new("attributes_features", DataType::Pointer, 1019, 0, true),
    BorrowedField::new("owner", DataType::Pointer, 1010, 0, true),
    BorrowedField::new("next", DataType::Pointer, 1008, 0, true),
    BorrowedField::new("previous", DataType::Pointer, 1008, 0, true),
    BorrowedField::new("geometric_owner", DataType::Pointer, 141, 0, true),
    BorrowedField::new("sense", DataType::Char, 0, 0, true),
    BorrowedField::new("basis_curve", DataType::Pointer, 1008, 0, true),
    BorrowedField::new("point_1", DataType::Vector, 0, 0, true),
    BorrowedField::new("point_2", DataType::Vector, 0, 0, true),
    BorrowedField::new("parm_1", DataType::Real, 0, 0, true),
    BorrowedField::new("parm_2", DataType::Real, 0, 0, true),
];

const FIELDS_11004_134: [BorrowedField<'static>; 9] = [
    BorrowedField::new("node_id", DataType::Integer, 0, 0, true),
    BorrowedField::new("attributes_features", DataType::Pointer, 1019, 0, true),
    BorrowedField::new("owner", DataType::Pointer, 1010, 0, true),
    BorrowedField::new("next", DataType::Pointer, 1008, 0, true),
    BorrowedField::new("previous", DataType::Pointer, 1008, 0, true),
    BorrowedField::new("geometric_owner", DataType::Pointer, 141, 0, true),
    BorrowedField::new("sense", DataType::Char, 0, 0, true),
    BorrowedField::new("nurbs", DataType::Pointer, 136, 0, true),
    BorrowedField::new("data", DataType::Pointer, 135, 0, true),
];

const FIELDS_11004_135: [BorrowedField<'static>; 1] =
    [BorrowedField::new("self_int", DataType::Byte, 0, 0, true)];

const FIELDS_11004_136: [BorrowedField<'static>; 12] = [
    BorrowedField::new("degree", DataType::Short, 0, 0, true),
    BorrowedField::new("n_vertices", DataType::Integer, 0, 0, true),
    BorrowedField::new("vertex_dim", DataType::Short, 0, 0, true),
    BorrowedField::new("n_knots", DataType::Integer, 0, 0, true),
    BorrowedField::new("knot_type", DataType::Byte, 0, 0, true),
    BorrowedField::new("periodic", DataType::Logical, 0, 0, true),
    BorrowedField::new("closed", DataType::Logical, 0, 0, true),
    BorrowedField::new("rational", DataType::Logical, 0, 0, true),
    BorrowedField::new("curve_form", DataType::Byte, 0, 0, true),
    BorrowedField::new("bspline_vertices", DataType::Pointer, 45, 0, true),
    BorrowedField::new("knot_mult", DataType::Pointer, 127, 0, true),
    BorrowedField::new("knots", DataType::Pointer, 128, 0, true),
];

const FIELDS_11004_137: [BorrowedField<'static>; 11] = [
    BorrowedField::new("node_id", DataType::Integer, 0, 0, true),
    BorrowedField::new("attributes_features", DataType::Pointer, 1019, 0, true),
    BorrowedField::new("owner", DataType::Pointer, 1010, 0, true),
    BorrowedField::new("next", DataType::Pointer, 1008, 0, true),
    BorrowedField::new("previous", DataType::Pointer, 1008, 0, true),
    BorrowedField::new("geometric_owner", DataType::Pointer, 141, 0, true),
    BorrowedField::new("sense", DataType::Char, 0, 0, true),
    BorrowedField::new("surface", DataType::Pointer, 1006, 0, true),
    BorrowedField::new("b_curve", DataType::Pointer, 134, 0, true),
    BorrowedField::new("original", DataType::Pointer, 1008, 0, true),
    BorrowedField::new("tolerance_to_original", DataType::Real, 0, 0, true),
];

const FIELDS_11004_138: [BorrowedField<'static>; 4] = [
    BorrowedField::new("chart_count", DataType::Integer, 0, 0, true),
    BorrowedField::new("chordal_error", DataType::Real, 0, 0, true),
    BorrowedField::new("angular_error", DataType::Real, 0, 0, true),
    BorrowedField::new("hvec", DataType::Hvec, 0, 1, true),
];

const FIELDS_11004_139: [BorrowedField<'static>; 11] = [
    BorrowedField::new("n_pvecs", DataType::Integer, 0, 0, true),
    BorrowedField::new("n_branches", DataType::Integer, 0, 0, true),
    BorrowedField::new("n_leaves", DataType::Integer, 0, 0, true),
    BorrowedField::new("chordal_error", DataType::Real, 0, 0, true),
    BorrowedField::new("angular_error", DataType::Real, 0, 0, true),
    BorrowedField::new("regular_pvecs", DataType::Pointer, 85, 0, true),
    BorrowedField::new("split_parms", DataType::Pointer, 83, 0, true),
    BorrowedField::new("branch_parents", DataType::Pointer, 96, 0, true),
    BorrowedField::new("branch_lo_children", DataType::Pointer, 96, 0, true),
    BorrowedField::new("branch_hi_children", DataType::Pointer, 96, 0, true),
    BorrowedField::new("leaf_parents", DataType::Pointer, 96, 0, true),
];

const FIELDS_11004_141: [BorrowedField<'static>; 4] = [
    BorrowedField::new("owner", DataType::Pointer, 1003, 0, true),
    BorrowedField::new("next", DataType::Pointer, 141, 0, true),
    BorrowedField::new("previous", DataType::Pointer, 141, 0, true),
    BorrowedField::new("shared_geometry", DataType::Pointer, 1003, 0, true),
];

const FIELDS_11004_150: [BorrowedField<'static>; 2] = [
    BorrowedField::new("pvec", DataType::Vector, 0, 0, true),
    BorrowedField::new("normal", DataType::Vector, 0, 0, true),
];

const FIELDS_11004_151: [BorrowedField<'static>; 4] = [
    BorrowedField::new("pvec", DataType::Vector, 0, 0, true),
    BorrowedField::new("axis", DataType::Vector, 0, 0, true),
    BorrowedField::new("radius", DataType::Real, 0, 0, true),
    BorrowedField::new("sense", DataType::Char, 0, 0, true),
];

const FIELDS_11004_152: [BorrowedField<'static>; 6] = [
    BorrowedField::new("pvec", DataType::Vector, 0, 0, true),
    BorrowedField::new("axis", DataType::Vector, 0, 0, true),
    BorrowedField::new("radius", DataType::Real, 0, 0, true),
    BorrowedField::new("sin_half_angle", DataType::Real, 0, 0, true),
    BorrowedField::new("cos_half_angle", DataType::Real, 0, 0, true),
    BorrowedField::new("sense", DataType::Char, 0, 0, true),
];

const FIELDS_11004_153: [BorrowedField<'static>; 3] = [
    BorrowedField::new("centre", DataType::Vector, 0, 0, true),
    BorrowedField::new("radius", DataType::Real, 0, 0, true),
    BorrowedField::new("sense", DataType::Char, 0, 0, true),
];

const FIELDS_11004_154: [BorrowedField<'static>; 5] = [
    BorrowedField::new("centre", DataType::Vector, 0, 0, true),
    BorrowedField::new("axis", DataType::Vector, 0, 0, true),
    BorrowedField::new("major_radius", DataType::Real, 0, 0, true),
    BorrowedField::new("minor_radius", DataType::Real, 0, 0, true),
    BorrowedField::new("sense", DataType::Char, 0, 0, true),
];

const FIELDS_11004_155: [BorrowedField<'static>; 2] = [
    BorrowedField::new("sweep", DataType::Vector, 0, 0, true),
    BorrowedField::new("subtype", DataType::Char, 0, 0, true),
];

const FIELDS_11004_156: [BorrowedField<'static>; 4] = [
    BorrowedField::new("sweep_u", DataType::Vector, 0, 0, true),
    BorrowedField::new("sweep_v", DataType::Vector, 0, 0, true),
    BorrowedField::new("subtype_u", DataType::Char, 0, 0, true),
    BorrowedField::new("subtype_v", DataType::Char, 0, 0, true),
];

const FIELDS_11004_157: [BorrowedField<'static>; 3] = [
    BorrowedField::new("base", DataType::Vector, 0, 0, true),
    BorrowedField::new("axis", DataType::Vector, 0, 0, true),
    BorrowedField::new("subtype", DataType::Char, 0, 0, true),
];

const FIELDS_11004_158: [BorrowedField<'static>; 4] = [
    BorrowedField::new("max_radius", DataType::Real, 0, 0, true),
    BorrowedField::new("min_radius", DataType::Real, 0, 0, true),
    BorrowedField::new("max_radius_bdry", DataType::Char, 0, 0, true),
    BorrowedField::new("min_radius_bdry", DataType::Char, 0, 0, true),
];

const FIELDS_11004_159: [BorrowedField<'static>; 6] = [
    BorrowedField::new("pvec", DataType::Vector, 0, 0, true),
    BorrowedField::new("normal", DataType::Vector, 0, 0, true),
    BorrowedField::new("u_range", DataType::Interval, 0, 0, true),
    BorrowedField::new("v_range", DataType::Interval, 0, 0, true),
    BorrowedField::new("type", DataType::Integer, 0, 0, true),
    BorrowedField::new("next", DataType::Pointer, 159, 0, true),
];

const FIELDS_11004_160: [BorrowedField<'static>; 25] = [
    BorrowedField::new("n_u_pvecs", DataType::Integer, 0, 0, true),
    BorrowedField::new("n_v_pvecs", DataType::Integer, 0, 0, true),
    BorrowedField::new("n_branches", DataType::Integer, 0, 0, true),
    BorrowedField::new("n_leaves", DataType::Integer, 0, 0, true),
    BorrowedField::new("distance_error", DataType::Real, 0, 0, true),
    BorrowedField::new("angular_error", DataType::Real, 0, 0, true),
    BorrowedField::new("regular_pvecs", DataType::Pointer, 85, 0, true),
    BorrowedField::new("split_parms", DataType::Pointer, 83, 0, true),
    BorrowedField::new("split_dirs", DataType::Pointer, 84, 0, true),
    BorrowedField::new("branch_parents", DataType::Pointer, 96, 0, true),
    BorrowedField::new("branch_lo_children", DataType::Pointer, 96, 0, true),
    BorrowedField::new("branch_hi_children", DataType::Pointer, 96, 0, true),
    BorrowedField::new("leaf_parents", DataType::Pointer, 96, 0, true),
    BorrowedField::new("leaf_boxes", DataType::Pointer, 97, 0, true),
    BorrowedField::new("leaf_pvecs", DataType::Pointer, 85, 0, true),
    BorrowedField::new("leaf_u_lo", DataType::Pointer, 83, 0, true),
    BorrowedField::new("leaf_u_hi", DataType::Pointer, 83, 0, true),
    BorrowedField::new("leaf_v_lo", DataType::Pointer, 83, 0, true),
    BorrowedField::new("leaf_v_hi", DataType::Pointer, 83, 0, true),
    BorrowedField::new("bucket_chars", DataType::Pointer, 84, 0, true),
    BorrowedField::new("bucket_shorts", DataType::Pointer, 96, 0, true),
    BorrowedField::new("bucket_ints", DataType::Pointer, 82, 0, true),
    BorrowedField::new("bucket_reals", DataType::Pointer, 83, 0, true),
    BorrowedField::new("leaf_u_splits", DataType::Pointer, 96, 0, true),
    BorrowedField::new("leaf_v_splits", DataType::Pointer, 96, 0, true),
];

const FIELDS_11004_161: [BorrowedField<'static>; 2] = [
    BorrowedField::new("n_patches", DataType::Integer, 0, 0, true),
    BorrowedField::new("patch", DataType::Pointer, 162, 1, true),
];

const FIELDS_11004_162: [BorrowedField<'static>; 29] = [
    BorrowedField::new("last_used", DataType::Integer, 0, 0, true),
    BorrowedField::new("n_derivatives", DataType::Integer, 0, 0, true),
    BorrowedField::new("triangular", DataType::Logical, 0, 0, true),
    BorrowedField::new("u_knot_index", DataType::Integer, 0, 0, true),
    BorrowedField::new("v_knot_index", DataType::Integer, 0, 0, true),
    BorrowedField::new("uint", DataType::Interval, 0, 0, true),
    BorrowedField::new("vint", DataType::Interval, 0, 0, true),
    BorrowedField::new("u_scale", DataType::Real, 0, 0, true),
    BorrowedField::new("v_scale", DataType::Real, 0, 0, true),
    BorrowedField::new("u_on_right", DataType::Logical, 0, 0, true),
    BorrowedField::new("v_on_right", DataType::Logical, 0, 0, true),
    BorrowedField::new("C", DataType::Pointer, 86, 0, true),
    BorrowedField::new("Cdu", DataType::Pointer, 86, 0, true),
    BorrowedField::new("Cdv", DataType::Pointer, 86, 0, true),
    BorrowedField::new("Cd2u", DataType::Pointer, 86, 0, true),
    BorrowedField::new("Cdudv", DataType::Pointer, 86, 0, true),
    BorrowedField::new("Cd2v", DataType::Pointer, 86, 0, true),
    BorrowedField::new("Cd2udv", DataType::Pointer, 86, 0, true),
    BorrowedField::new("Cd2vdu", DataType::Pointer, 86, 0, true),
    BorrowedField::new("Cd2ud2v", DataType::Pointer, 86, 0, true),
    BorrowedField::new("W", DataType::Pointer, 83, 0, true),
    BorrowedField::new("Wdu", DataType::Pointer, 83, 0, true),
    BorrowedField::new("Wdv", DataType::Pointer, 83, 0, true),
    BorrowedField::new("Wd2u", DataType::Pointer, 83, 0, true),
    BorrowedField::new("Wdudv", DataType::Pointer, 83, 0, true),
    BorrowedField::new("Wd2v", DataType::Pointer, 83, 0, true),
    BorrowedField::new("Wd2udv", DataType::Pointer, 83, 0, true),
    BorrowedField::new("Wd2vdu", DataType::Pointer, 83, 0, true),
    BorrowedField::new("Wd2ud2v", DataType::Pointer, 83, 0, true),
];

const FIELDS_11004_168: [BorrowedField<'static>; 1] =
    [BorrowedField::new("dummy", DataType::Char, 0, 0, true)];

const FIELDS_11004_169: [BorrowedField<'static>; 1] =
    [BorrowedField::new("pmarks", DataType::Pointer, 4, 1, true)];

const FIELDS_11004_170: [BorrowedField<'static>; 3] = [
    BorrowedField::new("length", DataType::Integer, 0, 0, true),
    BorrowedField::new("size", DataType::Integer, 0, 0, true),
    BorrowedField::new("array", DataType::Pointer, 169, 0, true),
];

const FIELDS_11004_171: [BorrowedField<'static>; 15] = [
    BorrowedField::new("current_partition", DataType::Pointer, 3, 0, true),
    BorrowedField::new("bulletin_board", DataType::Pointer, 107, 0, true),
    BorrowedField::new("linear_prec", DataType::Real, 0, 0, true),
    BorrowedField::new("angular_prec", DataType::Real, 0, 0, true),
    BorrowedField::new("receive_user_fld", DataType::Logical, 0, 0, true),
    BorrowedField::new("local_check_on", DataType::Logical, 0, 0, true),
    BorrowedField::new("data_check_on", DataType::Logical, 0, 0, true),
    BorrowedField::new("self_int_check_on", DataType::Logical, 0, 0, true),
    BorrowedField::new("generated_surfs_on", DataType::Logical, 0, 0, true),
    BorrowedField::new("general_topology_on", DataType::Logical, 0, 0, true),
    BorrowedField::new("keep_edge_curve_on", DataType::Logical, 0, 0, true),
    BorrowedField::new("bulletin_board_on", DataType::Integer, 0, 0, true),
    BorrowedField::new("binary_text", DataType::Integer, 0, 0, true),
    BorrowedField::new("snapshot_binary_text", DataType::Integer, 0, 0, true),
    BorrowedField::new("continuity_check", DataType::Integer, 0, 0, true),
];

const FIELDS_11004_172: [BorrowedField<'static>; 7] = [
    BorrowedField::new("attrib_def", DataType::Pointer, 80, 0, true),
    BorrowedField::new("parameter_check_on", DataType::Logical, 0, 0, true),
    BorrowedField::new("journal_on", DataType::Logical, 0, 0, true),
    BorrowedField::new("roll_forward", DataType::Logical, 0, 0, true),
    BorrowedField::new("journal_open", DataType::Integer, 0, 0, true),
    BorrowedField::new("rollback_size", DataType::Integer, 0, 0, true),
    BorrowedField::new("tag_limit", DataType::Integer, 0, 0, true),
];

const FIELDS_11004_180: [BorrowedField<'static>; 2] = [
    BorrowedField::new("n_segments", DataType::Integer, 0, 0, true),
    BorrowedField::new("segment", DataType::Pointer, 181, 1, true),
];

const FIELDS_11004_181: [BorrowedField<'static>; 12] = [
    BorrowedField::new("last_used", DataType::Integer, 0, 0, true),
    BorrowedField::new("n_derivatives", DataType::Integer, 0, 0, true),
    BorrowedField::new("knot_index", DataType::Integer, 0, 0, true),
    BorrowedField::new("tint", DataType::Interval, 0, 0, true),
    BorrowedField::new("scale", DataType::Real, 0, 0, true),
    BorrowedField::new("on_right", DataType::Logical, 0, 0, true),
    BorrowedField::new("C", DataType::Pointer, 86, 0, true),
    BorrowedField::new("Cdt", DataType::Pointer, 86, 0, true),
    BorrowedField::new("Cd2t", DataType::Pointer, 86, 0, true),
    BorrowedField::new("W", DataType::Pointer, 83, 0, true),
    BorrowedField::new("Wdt", DataType::Pointer, 83, 0, true),
    BorrowedField::new("Wd2t", DataType::Pointer, 83, 0, true),
];

const FIELDS_11004_182: [BorrowedField<'static>; 1] =
    [BorrowedField::new("hvec", DataType::Hvec, 0, 1, true)];

const FIELDS_11004_183: [BorrowedField<'static>; 1] =
    [BorrowedField::new("order", DataType::Integer, 0, 1, true)];

const FIELDS_13006_7: [BorrowedField<'static>; 2] = [
    BorrowedField::new("leaf", DataType::Pointer, 6, 1024, true),
    BorrowedField::new("n_live", DataType::Integer, 0, 0, true),
];

const FIELDS_13006_8: [BorrowedField<'static>; 6] = [
    BorrowedField::new("tag_limit", DataType::Integer, 0, 0, true),
    BorrowedField::new("null_branch", DataType::Pointer, 7, 0, true),
    BorrowedField::new("null_leaf", DataType::Pointer, 6, 0, true),
    BorrowedField::new("next_low_tag", DataType::Integer, 0, 0, true),
    BorrowedField::new("next_high_tag", DataType::Integer, 0, 0, true),
    BorrowedField::new("branch", DataType::Pointer, 7, 4096, true),
];

const FIELDS_13006_78: [BorrowedField<'static>; 13] = [
    BorrowedField::new("split_fn", DataType::Unestablished(b'q'), 0, 0, true),
    BorrowedField::new("merge_fn", DataType::Unestablished(b'q'), 0, 0, true),
    BorrowedField::new("delete_fn", DataType::Unestablished(b'q'), 0, 0, true),
    BorrowedField::new("copy_fn", DataType::Unestablished(b'q'), 0, 0, true),
    BorrowedField::new("transmit_fn", DataType::Unestablished(b'q'), 0, 0, true),
    BorrowedField::new("receive_fn", DataType::Unestablished(b'q'), 0, 0, true),
    BorrowedField::new("split_on", DataType::Logical, 0, 0, true),
    BorrowedField::new("merge_on", DataType::Logical, 0, 0, true),
    BorrowedField::new("delete_on", DataType::Logical, 0, 0, true),
    BorrowedField::new("copy_on", DataType::Logical, 0, 0, true),
    BorrowedField::new("transmit_on", DataType::Logical, 0, 0, true),
    BorrowedField::new("receive_on", DataType::Logical, 0, 0, true),
    BorrowedField::new("callback_type", DataType::Char, 0, 0, true),
];

const FIELDS_13006_80: [BorrowedField<'static>; 7] = [
    BorrowedField::new("next", DataType::Pointer, 80, 0, true),
    BorrowedField::new("identifier", DataType::Pointer, 79, 0, true),
    BorrowedField::new("type_id", DataType::Integer, 0, 0, true),
    BorrowedField::new("actions", DataType::Byte, 0, 8, true),
    BorrowedField::new("field_names", DataType::Pointer, 99, 0, true),
    BorrowedField::new("legal_owners", DataType::Logical, 0, 14, true),
    BorrowedField::new("fields", DataType::Byte, 0, 1, true),
];

const FIELDS_13006_95: [BorrowedField<'static>; 1] = [BorrowedField::new(
    "values",
    DataType::Unestablished(b'q'),
    0,
    1,
    false,
)];

const FIELDS_13006_98: [BorrowedField<'static>; 1] =
    [BorrowedField::new("values", DataType::Word, 0, 1, true)];

const FIELDS_13006_99: [BorrowedField<'static>; 1] = [BorrowedField::new(
    "names",
    DataType::Pointer,
    1037,
    1,
    true,
)];

const FIELDS_13006_129: [BorrowedField<'static>; 1] =
    [BorrowedField::new("mult", DataType::Integer, 0, 1, true)];

const FIELDS_13006_135: [BorrowedField<'static>; 2] = [
    BorrowedField::new("self_int", DataType::Byte, 0, 0, true),
    BorrowedField::new("analytic_form", DataType::Pointer, 1036, 0, true),
];

const FIELDS_13006_163: [BorrowedField<'static>; 7] = [
    BorrowedField::new("axis_pt", DataType::Vector, 0, 0, true),
    BorrowedField::new("axis_dir", DataType::Vector, 0, 0, true),
    BorrowedField::new("hand", DataType::Char, 0, 0, true),
    BorrowedField::new("turns", DataType::Interval, 0, 0, true),
    BorrowedField::new("pitch", DataType::Real, 0, 0, true),
    BorrowedField::new("gap", DataType::Real, 0, 0, true),
    BorrowedField::new("tol", DataType::Real, 0, 0, true),
];

const FIELDS_13006_184: [BorrowedField<'static>; 7] = [
    BorrowedField::new("axis_pt", DataType::Vector, 0, 0, true),
    BorrowedField::new("axis_dir", DataType::Vector, 0, 0, true),
    BorrowedField::new("point", DataType::Vector, 0, 0, true),
    BorrowedField::new("hand", DataType::Char, 0, 0, true),
    BorrowedField::new("turns", DataType::Interval, 0, 0, true),
    BorrowedField::new("pitch", DataType::Real, 0, 0, true),
    BorrowedField::new("tol", DataType::Real, 0, 0, true),
];
