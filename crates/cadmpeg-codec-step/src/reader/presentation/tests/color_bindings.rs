// SPDX-License-Identifier: Apache-2.0
//! Scalar conflict diagnostics retain physical source order.

use crate::loss::StepLossCode;
use crate::StepCodec;
use cadmpeg_ir::codec::{Codec, DecodeOptions};
use std::io::Cursor;

#[test]
fn scalar_conflict_losses_preserve_both_targets() {
    let source = String::from_utf8_lossy(include_bytes!("../../../../tests/fixtures/ap214_sheet.p21"))
        .replace("ENDSEC;\nEND-ISO-10303-21;", "#100=STYLED_ITEM('',(#66),#29);\n#101=STYLED_ITEM('',(#46),#31);\n#102=STYLED_ITEM('',(#66),#31);\nENDSEC;\nEND-ISO-10303-21;");
    let result = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("independent scalar conflicts");
    let conflicts = result
        .report()
        .losses
        .iter()
        .filter(|loss| loss.code == StepLossCode::ConflictingScalarColors.kind())
        .collect::<Vec<_>>();
    assert_eq!(conflicts.len(), 2);
    assert!(conflicts
        .iter()
        .any(|loss| loss.message.contains("face#29")));
    assert!(conflicts
        .iter()
        .any(|loss| loss.message.contains("body#31")));
}

#[test]
fn scalar_conflict_losses_follow_first_physical_style_encounter() {
    let base =
        String::from_utf8_lossy(include_bytes!("../../../../tests/fixtures/ap214_sheet.p21"))
            .lines()
            .filter(|line| {
                !line.starts_with("#47=") && !line.starts_with("#67=") && !line.starts_with("#68=")
            })
            .collect::<Vec<_>>()
            .join("\n");
    for (styles, expected) in [
        (
            "#900=STYLED_ITEM('',(#46),#31);\n#100=STYLED_ITEM('',(#46),#29);\n#101=STYLED_ITEM('',(#66),#29);\n#901=STYLED_ITEM('',(#66),#31);\n",
            ["body#31", "face#29"],
        ),
        (
            "#100=STYLED_ITEM('',(#46),#29);\n#900=STYLED_ITEM('',(#46),#31);\n#901=STYLED_ITEM('',(#66),#31);\n#101=STYLED_ITEM('',(#66),#29);\n",
            ["face#29", "body#31"],
        ),
        (
            "#90=GEOMETRIC_SET('',(#31,#29));\n#100=STYLED_ITEM('',(#46),#90);\n#101=STYLED_ITEM('',(#66),#90);\n",
            ["body#31", "face#29"],
        ),
        (
            "#90=GEOMETRIC_SET('',(#29,#31));\n#100=STYLED_ITEM('',(#46),#90);\n#101=STYLED_ITEM('',(#66),#90);\n",
            ["face#29", "body#31"],
        ),
    ] {
        let source = base.replace("ENDSEC;\nEND-ISO-10303-21;", &format!("{styles}ENDSEC;\nEND-ISO-10303-21;"));
        let result = StepCodec::default()
            .decode(&mut Cursor::new(source), &DecodeOptions::default())
            .expect("source-ordered scalar conflicts");
        let conflicts: Vec<_> = result.report().losses.iter()
            .filter(|loss| loss.code == StepLossCode::ConflictingScalarColors.kind())
            .collect();
        assert_eq!(conflicts.len(), 2);
        for (loss, target) in conflicts.iter().zip(expected) {
            assert!(loss.message.contains(target), "{}: {target}", loss.message);
        }
        assert_eq!(result.ir().model.appearance_bindings.len(), 4);
    }
}

#[test]
fn scalar_conflict_losses_merge_style_and_target_conflicts_in_source_order() {
    let base =
        String::from_utf8_lossy(include_bytes!("../../../../tests/fixtures/ap214_sheet.p21"))
            .lines()
            .filter(|line| {
                !line.starts_with("#47=") && !line.starts_with("#67=") && !line.starts_with("#68=")
            })
            .collect::<Vec<_>>()
            .join("\n");
    for (styles, expected) in [
        (
            "#900=STYLED_ITEM('',(#46),#31);\n#901=STYLED_ITEM('',(#66),#31);\n#100=STYLED_ITEM('',(#46,#66),#29);\n#99=STYLED_ITEM('',(#999),#29);\n#101=STYLED_ITEM('',(#998),#29);\n",
            ["body#31", "STYLED_ITEM #100"],
        ),
        (
            "#100=STYLED_ITEM('',(#46,#66),#29);\n#900=STYLED_ITEM('',(#46),#31);\n#901=STYLED_ITEM('',(#66),#31);\n#99=STYLED_ITEM('',(#999),#29);\n#101=STYLED_ITEM('',(#998),#29);\n",
            ["STYLED_ITEM #100", "body#31"],
        ),
    ] {
        let source = base.replace("ENDSEC;\nEND-ISO-10303-21;", &format!("{styles}#998=UNKNOWN_STYLE();\n#999=UNKNOWN_STYLE();\nENDSEC;\nEND-ISO-10303-21;"));
        let result = StepCodec::default()
            .decode(&mut Cursor::new(source), &DecodeOptions::default())
            .expect("mixed scalar conflicts");
        let losses: Vec<_> = result.report().losses.iter()
            .filter(|loss| loss.message.contains("STYLED_ITEM #99") || loss.message.contains("STYLED_ITEM #101") || loss.code == StepLossCode::ConflictingScalarColors.kind())
            .collect();
        assert_eq!(losses.len(), 4);
        assert_eq!(losses[0].message, "STYLED_ITEM #99 has no resolved surface color");
        assert_eq!(losses[2].message, "STYLED_ITEM #101 has no resolved surface color");
        for (loss, expected) in [losses[1], losses[3]].into_iter().zip(expected) {
            assert_eq!(loss.code, StepLossCode::ConflictingScalarColors.kind());
            assert!(loss.message.contains(expected), "{}: {expected}", loss.message);
        }
        assert_eq!(result.ir().model.appearance_bindings.len(), 2);
    }
}
