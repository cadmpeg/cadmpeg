// SPDX-License-Identifier: Apache-2.0
//! The target of the configuration tables on the generator path and the
//! patch path, on a held decode whose table names row `a` active: the Design
//! stream stores the state of the active row (f3d.md §1.2).

use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::features::parameters::ParameterValue as LegacyValue;
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::scalar::Length;

use super::super::{generated, patched, stated_rows};
use crate::design_v11::configurations::rebase::rebased;
use crate::design_v11::configurations::ActiveRows;
use crate::integration_tests::configuration_wire as wire;
use crate::test_support::native_test::{f3d_native, update_f3d_native};
use crate::writer::primitives::{configuration_targets, decoded_configurations};

/// `ir` with the configuration table documents the generator writes for
/// it, the configuration whose row the root record names, and the losses of
/// the tables.
fn generated_on(ir: &CadIr) -> (CadIr, Option<String>, Vec<LossNote>) {
    let rebased = rebased(ir);
    let target = rebased.as_ref().map_or(ir, |rebased| &rebased.ir);
    let native = f3d_native(target);
    let targets = configuration_targets(target, &native);
    let decoded = decoded_configurations(&native.design_configurations, &targets).unwrap();
    let mut losses = Vec::new();
    let stated = stated_rows(
        target,
        &decoded.configurations,
        &targets,
        &rebased
            .as_ref()
            .map(|rebased| rebased.parameters.keys().cloned().collect())
            .unwrap_or_default(),
        ActiveRows::Design,
        &mut losses,
    );
    let (written, plan) = generated(
        target,
        &native.design_configurations,
        &decoded,
        &stated,
        &targets,
        None,
        &mut losses,
    )
    .unwrap();
    let documents = written.documents.into_owned();
    let mut out = target.clone();
    update_f3d_native(&mut out, |native| native.design_configurations = documents);
    (out, plan.active().map(str::to_owned), losses)
}

/// `ir` with the configuration table documents the patch writes for it,
/// whether it writes them, and the losses of the tables.
fn patched_on(ir: &CadIr) -> (CadIr, bool, Vec<LossNote>) {
    let native = f3d_native(ir);
    let patched = patched(ir, &native).unwrap();
    let mut out = ir.clone();
    let written = matches!(&patched.destination.documents, std::borrow::Cow::Owned(_));
    if let std::borrow::Cow::Owned(documents) = patched.destination.documents {
        update_f3d_native(&mut out, |native| native.design_configurations = documents);
    }
    (out, written, patched.losses)
}

/// The held fixture with the active configuration `A` overriding `W` to
/// `9 mm`.
fn overridden() -> (CadIr, wire::Fixture) {
    let (decoded, fixture) = wire::fixture(true);
    let mut ir = decoded.ir().clone();
    wire::named_mut(&mut ir, "A")
        .overrides
        .parameters
        .insert(fixture.width.clone(), wire::length_override(9.0));
    (ir, fixture)
}

/// The generator takes the override of the active configuration into the
/// Design stream, so the active row states it and nothing is pinned.
#[test]
fn the_generator_writes_a_rebased_active_override_into_the_active_row() {
    let (ir, fixture) = overridden();
    let a = wire::named(&ir, "A").id.clone();
    let (out, active, losses) = generated_on(&ir);
    assert_eq!(active.as_deref(), Some(a.as_str()));
    let (text, value) = wire::parameter_cell(&out, &fixture.entry, "a", &fixture.width);
    assert_eq!(text, "9 mm", "{losses:#?}");
    assert!((value - 0.9).abs() < 1e-9, "{value}");
    assert!(
        wire::pinned(&losses, &a, &fixture.width).is_empty(),
        "{losses:#?}"
    );
}

/// The `testRowData` entry of a rebased parameter states the Design stream
/// value of the active row.
#[test]
fn the_generator_writes_the_test_row_of_a_rebased_active_override() {
    let (ir, fixture) = overridden();
    let (out, _, losses) = generated_on(&ir);
    let width = wire::test_row(&out, &fixture.entry, "W");
    assert_eq!(width["Expression"], "9 mm", "{width}: {losses:#?}");
    assert!(
        (width["Value"].as_f64().unwrap() - 0.9).abs() < 1e-9,
        "{width}"
    );
}

/// A row made active whose configuration no longer overrides `W` resolves
/// the Design stream value: its cell and the `testRowData` entry state it,
/// and nothing is charged for `W`.
#[test]
fn the_generator_writes_the_base_into_a_row_made_active() {
    let (decoded, fixture) = wire::fixture(true);
    let mut ir = decoded.ir().clone();
    let b = wire::named(&ir, "B").id.clone();
    ir.design
        .scopes
        .iter_mut()
        .find(|scope| scope.active_configuration.is_some())
        .unwrap()
        .active_configuration = Some(b.clone());
    wire::named_mut(&mut ir, "B")
        .overrides
        .parameters
        .remove(&fixture.width);
    let (expression, millimetres) = wire::own(&ir, &fixture.width);
    let (out, active, losses) = generated_on(&ir);
    assert_eq!(active.as_deref(), Some(b.as_str()));
    let (text, value) = wire::parameter_cell(&out, &fixture.entry, "b", &fixture.width);
    assert_eq!(text, expression, "{losses:#?}");
    assert!((value - millimetres / 10.0).abs() < 1e-9, "{value}");
    let width = wire::test_row(&out, &fixture.entry, "W");
    assert_eq!(width["Expression"], expression.as_str(), "{width}");
    assert!(
        (width["Value"].as_f64().unwrap() - millimetres / 10.0).abs() < 1e-9,
        "{width}"
    );
    assert!(
        losses
            .iter()
            .all(|loss| !loss.message.contains(fixture.width.as_str())),
        "{losses:#?}"
    );
}

/// An edit of the active cell in the v10 rows alone: the patch writes no
/// Design stream parameter, so the cell states the Design stream value and
/// the edit is charged once.
#[test]
fn the_patch_writes_the_design_stream_value_over_a_v10_edit_of_the_active_cell() {
    let (decoded, fixture) = wire::fixture(true);
    let mut ir = decoded.ir().clone();
    let a = wire::named(&ir, "A").id.clone();
    let row = ir
        .model
        .configurations
        .iter_mut()
        .find(|row| row.id == a)
        .expect("row a");
    row.parameter_overrides
        .insert(fixture.width.clone(), "3 mm".into());
    row.parameter_values.insert(
        fixture.width.clone(),
        LegacyValue::Length(Length::new(3.0).unwrap()),
    );
    let (expression, millimetres) = wire::own(&ir, &fixture.width);
    let (out, written, losses) = patched_on(&ir);
    assert!(written, "{losses:#?}");
    let (text, value) = wire::parameter_cell(&out, &fixture.entry, "a", &fixture.width);
    assert_eq!(text, expression, "{losses:#?}");
    assert!((value - millimetres / 10.0).abs() < 1e-9, "{value}");
    assert_eq!(
        wire::pinned(&losses, &a, &fixture.width).len(),
        1,
        "{losses:#?}"
    );
    assert!(
        losses
            .iter()
            .all(|loss| loss.code.to_string() != "export/edit.dropped"),
        "{losses:#?}"
    );
}

/// An active row moved in the design: the patch keeps the stored root record
/// and every row, and charges the move once.
#[test]
fn the_patch_keeps_the_stored_active_row_and_charges_the_move() {
    let (decoded, _) = wire::fixture(true);
    let mut ir = decoded.ir().clone();
    let b = wire::named(&ir, "B").id.clone();
    ir.design
        .scopes
        .iter_mut()
        .find(|scope| scope.active_configuration.is_some())
        .unwrap()
        .active_configuration = Some(b.clone());
    let (_, written, losses) = patched_on(&ir);
    assert!(!written, "{losses:#?}");
    let [loss] = losses.as_slice() else {
        panic!("one loss: {losses:#?}");
    };
    assert_eq!(loss.code.to_string(), "export/value.unwritable");
    assert!(
        loss.message.contains("DesignConfigurationRoot") && loss.message.contains(b.as_str()),
        "{loss:#?}"
    );
}

#[test]
fn the_patch_retains_a_removed_row_that_the_stored_root_references() {
    let (decoded, fixture) = wire::fixture(true);
    let mut ir = decoded.ir().clone();
    let a = wire::named(&ir, "A").id.clone();
    let b = wire::named(&ir, "B").id.clone();
    ir.design
        .scopes
        .iter_mut()
        .find(|scope| scope.active_configuration.is_some())
        .unwrap()
        .active_configuration = Some(b.clone());
    ir.design
        .configurations
        .retain(|configuration| configuration.id != a);
    ir.model.configurations.retain(|row| row.id != a);

    let (out, _, losses) = patched_on(&ir);
    assert_eq!(
        wire::parameter_cell(&out, &fixture.entry, "a", &fixture.width),
        wire::parameter_cell(decoded.ir(), &fixture.entry, "a", &fixture.width)
    );
    assert_eq!(
        wire::test_row(&out, &fixture.entry, "W"),
        wire::test_row(decoded.ir(), &fixture.entry, "W")
    );
    assert!(
        losses.iter().any(|loss| {
            loss.code.to_string() == "export/edit.dropped"
                && loss.message.contains(a.as_str())
                && loss.message.contains("still references this row")
        }),
        "{losses:#?}"
    );
    assert!(
        wire::pinned(&losses, &b, &fixture.width).is_empty(),
        "{losses:#?}"
    );
}
