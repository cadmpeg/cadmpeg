// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use cadmpeg_core::decode::DecodeMode;
use cadmpeg_core::CodecError;
use cadmpeg_ir::codec::DecodeOptions;

use crate::loss::IgesLossCode;
use crate::test_support::{
    card, directory_card, fixed_ascii_with_global, parameter_card, point_file_with_global,
};

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

fn parse_global_fields(fields: &[String]) -> Result<ParsedGlobal, CodecError> {
    let mut global = fields.join(",");
    global.push(';');
    let bytes = fixed_ascii_with_global(global.as_bytes());
    crate::global::parse(&crate::card::scan(&bytes)?)
}

fn resolve_global_fields(fields: &[String]) -> ParsedGlobal {
    parse_global_fields(fields).unwrap()
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

fn dialect_losses(report: &cadmpeg_ir::report::DecodeReport) -> usize {
    report_code_count(report, IgesLossCode::SourceDialectUnverified)
}

fn point_file_with_field(index: usize, value: &str) -> Vec<u8> {
    let mut fields = valid_global_fields();
    fields[index] = value.to_owned();
    let mut global = fields.join(",");
    global.push(';');
    point_file_with_global(global.as_bytes())
}

fn point_file_with_delimiters(parameter: char, record: char) -> Vec<u8> {
    let mut fields = valid_global_fields();
    fields[0] = format!("1H{parameter}");
    fields[1] = format!("1H{record}");
    let global = format!("{}{record}", fields.join(&parameter.to_string()));
    let mut bytes = fixed_ascii_with_global(global.as_bytes());
    bytes.truncate(bytes.len() - 81);
    bytes.extend(directory_card(
        ["116", "1", "0", "0", "0", "0", "0", "0", "00000000"],
        1,
    ));
    bytes.extend(directory_card(
        ["116", "0", "0", "1", "0", "", "", "POINT", "0"],
        2,
    ));
    bytes.extend(parameter_card(
        format!("116{parameter}1.0{parameter}2.0{parameter}3.0{record}").as_bytes(),
        1,
        1,
    ));
    let global_cards = global.len().div_ceil(72);
    bytes.extend(card(
        format!("S0000001G{global_cards:07}D0000002P0000001").as_bytes(),
        b'T',
        1,
    ));
    bytes
}

fn strict_options(container_only: bool) -> DecodeOptions {
    let mut options = DecodeOptions {
        container_only,
        ..DecodeOptions::default()
    };
    options.policy.mode = DecodeMode::Strict;
    options
}

fn fixed_ascii_with_global_chunks(chunks: &[&[u8]]) -> Vec<u8> {
    let mut bytes = card(b"original fixture", b'S', 1);
    let cards = chunks
        .iter()
        .flat_map(|chunk| chunk.chunks(72))
        .collect::<Vec<_>>();
    for (index, chunk) in cards.iter().enumerate() {
        bytes.extend(card(chunk, b'G', u32::try_from(index + 1).unwrap()));
    }
    bytes.extend(card(
        format!("S0000001G{:07}D0000000P0000000", cards.len()).as_bytes(),
        b'T',
        1,
    ));
    bytes
}
