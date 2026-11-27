// SPDX-License-Identifier: Apache-2.0
//! The CADIR writer charges a record of an arena the wire omits by the
//! written `design` records that restate it (CADIR v11 §3.2).

use super::*;
use crate::design::projection::restated::Restater;

/// A CADIR write compares a record of an arena the wire omits with the
/// written `design` record of its id where no restatement is stated, and
/// names each field the design record does not hold; a write that omits the
/// design record too charges the record whole.
#[test]
fn a_cadir_write_compares_a_record_with_the_written_design_record_of_its_id() {
    let mut ir = CadIr::empty();
    ir.model.spreadsheets.push(v10_sheet());
    let cadir = crate::codec::write::coverage::cadir_dispositions();
    assert_eq!(cadir.omitted(&ir), ["spreadsheets"]);
    ir.design.spreadsheets.push(design_sheet("v10"));
    assert_eq!(
        sheet_charges(&ir, cadir),
        [
            "the cadir write does not represent model arena `spreadsheets`: \
          1 record(s) stated in part: test:design:spreadsheet#v10 (feature)"
        ]
    );
    assert_eq!(
        sheet_charges(&ir, uniform_dispositions(ArenaDisposition::Omitted)),
        [
            "the cadir write does not represent model arena `spreadsheets`: \
          1 record(s) omitted: test:design:spreadsheet#v10"
        ]
    );
}

/// The `arena.omitted` losses of `ir` a write with `dispositions` charges on
/// the model arena `spreadsheets`.
fn sheet_charges(ir: &CadIr, dispositions: ArenaDispositions) -> Vec<String> {
    coverage_losses(ir, ArenaCoverage::Declared(dispositions))
        .into_iter()
        .filter(|loss| loss.message.contains("model arena `spreadsheets`"))
        .map(|loss| loss.message)
        .collect()
}

/// A CADIR write carries a record of an arena the wire omits that a written
/// `design` record restates under another id, and still charges a record no
/// written record restates; a write that omits the restater charges both.
#[test]
fn a_cadir_write_carries_a_record_a_written_design_record_restates_under_another_id() {
    let mut ir = CadIr::empty();
    ir.model.spreadsheets.push(v10_sheet());
    ir.model.spreadsheets.push(model_sheet("unrestated"));
    ir.design.spreadsheets.push(design_sheet("v11"));
    ir.model.restated_mut().record(
        "test:design:spreadsheet#v10",
        Restater::Record("test:design:spreadsheet#v11".to_owned()),
        [],
    );
    let cadir = crate::codec::write::coverage::cadir_dispositions();
    assert_eq!(
        sheet_charges(&ir, cadir),
        [
            "the cadir write does not represent model arena `spreadsheets`: \
          1 record(s) omitted: test:design:spreadsheet#unrestated"
        ]
    );
    let mut no_design = cadir;
    no_design.design.spreadsheets = ArenaDisposition::Omitted;
    assert_eq!(
        sheet_charges(&ir, no_design),
        [
            "the cadir write does not represent model arena `spreadsheets`: \
          2 record(s) omitted: test:design:spreadsheet#unrestated, test:design:spreadsheet#v10"
        ]
    );
}

/// A CADIR write charges a record a written `design` record restates under
/// its id without some fields once, naming the record and the fields.
#[test]
fn a_cadir_write_charges_a_record_restated_in_part_once_naming_its_fields() {
    let mut ir = CadIr::empty();
    ir.model.spreadsheets.push(v10_sheet());
    ir.model.spreadsheets.push(model_sheet("unrestated"));
    ir.design.spreadsheets.push(design_sheet("v10"));
    let by = || Restater::Record("test:design:spreadsheet#v10".to_owned());
    ir.model
        .restated_mut()
        .record("test:design:spreadsheet#v10", by(), ["row_heights"]);
    ir.model
        .restated_mut()
        .record("test:design:spreadsheet#v10", by(), ["merged_ranges"]);
    let cadir = crate::codec::write::coverage::cadir_dispositions();
    assert_eq!(
        sheet_charges(&ir, cadir),
        [
            "the cadir write does not represent model arena `spreadsheets`: \
          1 record(s) omitted: test:design:spreadsheet#unrestated; \
          1 record(s) stated in part: test:design:spreadsheet#v10 (merged_ranges, row_heights)"
        ]
    );
    ir.model.spreadsheets.pop();
    assert_eq!(
        sheet_charges(&ir, cadir),
        [
            "the cadir write does not represent model arena `spreadsheets`: \
          1 record(s) stated in part: test:design:spreadsheet#v10 (merged_ranges, row_heights)"
        ]
    );
    assert_eq!(cadir.omitted(&ir), ["spreadsheets"]);
}

/// The name table restates a record only for a write of every `design`
/// arena, the one write that writes the table.
#[test]
fn a_record_the_name_table_restates_is_carried_only_by_a_write_of_the_whole_design() {
    let mut ir = CadIr::empty();
    ir.model.spreadsheets.push(v10_sheet());
    ir.model
        .restated_mut()
        .record("test:design:spreadsheet#v10", Restater::Names, []);
    let cadir = crate::codec::write::coverage::cadir_dispositions();
    assert!(sheet_charges(&ir, cadir).is_empty());
    let mut reported = cadir;
    reported.design.parameters = ArenaDisposition::Reported;
    assert_eq!(
        sheet_charges(&ir, reported),
        [
            "the cadir write does not represent model arena `spreadsheets`: \
          1 record(s) omitted: test:design:spreadsheet#v10"
        ]
    );
}
