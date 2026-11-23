// SPDX-License-Identifier: Apache-2.0
//! The losses of configurations whose state the file does not store.
#![allow(clippy::unwrap_used)]

use super::*;
use crate::loss::SldprtLossCode;

/// The losses of a part whose configuration `Long` has no partition, with
/// `swConfigurationNeedsUpdate` set to `needs_update`.
fn without_partition(needs_update: &str) -> (CadIr, Vec<cadmpeg_ir::report::loss::LossNote>) {
    let mut source = with_history(
        sldprt_with_body(&triangle_body()),
        br#"<Keywords Name="Bracket">
            <Configuration Name="Default" SourceIndex="0"/>
            <Configuration Name="Long" SourceIndex="1"/>
            <Extrusion Name="Boss" Type="BossExtrude" id="7"/>
        </Keywords>"#,
        &[],
    );
    source.extend(make_block(
        0x43,
        "Contents/SolidWorks",
        format!(
            r#"<swSolidWorks><swModel swConfigurationName="Default"/><swConfigurationList><swConfiguration swName="Default" swID="0" swConfigurationNeedsUpdate="NO"/><swConfiguration swName="Long" swID="1" swConfigurationNeedsUpdate="{needs_update}"/></swConfigurationList></swSolidWorks>"#
        )
        .as_bytes(),
    ));
    let result = SldprtCodec
        .decode(
            &mut Cursor::new(native_envelope_fixture(&source)),
            &DecodeOptions::default(),
        )
        .unwrap();
    (result.ir().clone(), result.report().losses.clone())
}

fn not_stored(losses: &[cadmpeg_ir::report::loss::LossNote]) -> Vec<&str> {
    losses
        .iter()
        .filter(|loss| loss.code == SldprtLossCode::ConfigStateNotStored.kind())
        .map(|loss| loss.message.as_str())
        .collect()
}

#[test]
fn a_stale_configuration_without_a_partition_charges_a_loss_naming_it() {
    let (ir, losses) = without_partition("YES");
    let [evaluation] = ir.design.evaluations.as_slice() else {
        panic!("one evaluation: {:#?}", ir.design.evaluations);
    };
    assert!(evaluation.content.is_none());
    let [message] = not_stored(&losses)[..] else {
        panic!("one loss: {losses:#?}");
    };
    assert!(message.contains("\"Long\""), "{message}");
    assert!(message.contains("stale"), "{message}");
}

#[test]
fn a_configuration_without_a_partition_or_a_flag_charges_a_loss_naming_it() {
    let (ir, losses) = without_partition("NO");
    assert!(
        ir.design.evaluations.is_empty(),
        "{:#?}",
        ir.design.evaluations
    );
    let [message] = not_stored(&losses)[..] else {
        panic!("one loss: {losses:#?}");
    };
    assert!(message.contains("\"Long\""), "{message}");
    assert!(!message.contains("stale"), "{message}");
}
