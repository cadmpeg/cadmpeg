// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use cadmpeg_core::decode::DecodeMode;
use cadmpeg_ir::codec::DecodeOptions;

use crate::loss::IgesLossCode;
use crate::test_support::{fixed_ascii_with_global, point_file_with_global};

mod dialect;
mod inspection;
mod parsing;
mod resolution;
mod units;

fn valid_global_fields() -> Vec<String> {
    [
        "1H,",
        "1H;",
        "7Hproduct",
        "8Hpart.igs",
        "7Hcadmpeg",
        "3H0.1",
        "32",
        "38",
        "6",
        "308",
        "15",
        "0H",
        "1.0",
        "2",
        "2HMM",
        "1",
        "1.0",
        "15H20260714.000000",
        "0.001",
        "1000.0",
        "6Hauthor",
        "3Horg",
        "11",
        "0",
        "0H",
        "0H",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect()
}

type ParsedGlobal = (
    crate::global::ResolvedGlobal,
    Vec<cadmpeg_ir::report::LossNote>,
);

fn resolve_global_fields(fields: &[String]) -> ParsedGlobal {
    let mut global = fields.join(",");
    global.push(';');
    let bytes = fixed_ascii_with_global(global.as_bytes());
    crate::global::parse(&crate::card::scan(&bytes).unwrap()).unwrap()
}

fn code_count(losses: &[cadmpeg_ir::report::LossNote], code: IgesLossCode) -> usize {
    losses
        .iter()
        .filter(|loss| loss.code == code.kind())
        .count()
}

fn report_code_count(report: &cadmpeg_ir::report::DecodeReport, code: IgesLossCode) -> usize {
    code_count(&report.losses, code)
}

fn point_file_with_version_flag(flag: &str) -> Vec<u8> {
    point_file_with_global(
        format!(
            "1H,,1H;,7Hproduct,8Hpart.igs,7Hcadmpeg,3H0.1,32,38,6,308,15,0H,1.0,2,2HMM,1,1.0,15H20260714.000000,0.001,1000.0,6Hauthor,3Horg,{flag},0,0H,0H;"
        )
        .as_bytes(),
    )
}

fn point_file_with_field(index: usize, value: &str) -> Vec<u8> {
    let mut fields = valid_global_fields();
    fields[index] = value.to_owned();
    let mut global = fields.join(",");
    global.push(';');
    point_file_with_global(global.as_bytes())
}

fn strict_options(container_only: bool) -> DecodeOptions {
    let mut options = DecodeOptions {
        container_only,
        ..DecodeOptions::default()
    };
    options.policy.mode = DecodeMode::Strict;
    options
}
