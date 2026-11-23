// SPDX-License-Identifier: Apache-2.0
//! The losses of configurations whose state the file does not store.
#![allow(clippy::unwrap_used)]

use super::*;
use crate::loss::SldprtLossCode;

/// The losses of a part whose configuration `Long` has no partition, with
/// `swConfigurationNeedsUpdate` set to `needs_update`.
fn without_partition(needs_update: &str) -> (CadIr, Vec<cadmpeg_ir::report::loss::LossNote>) {
    with_damaged(needs_update, None)
}

/// The losses of the part [`without_partition`] gives, with `damaged` added
/// to the source and its last byte changed after the envelope is built.
fn with_damaged(
    needs_update: &str,
    damaged: Option<&[u8]>,
) -> (CadIr, Vec<cadmpeg_ir::report::loss::LossNote>) {
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
    source.extend(damaged.unwrap_or_default());
    let mut enveloped = native_envelope_fixture(&source);
    if let Some(block) = damaged {
        let at = enveloped
            .windows(block.len())
            .position(|window| window == block)
            .unwrap();
        enveloped[at + block.len() - 1] ^= 0xff;
    }
    let result = SldprtCodec
        .decode(&mut Cursor::new(enveloped), &DecodeOptions::default())
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

#[test]
fn a_configuration_whose_partition_is_given_up_is_not_reported_unstored() {
    let partition = make_block(
        0x20,
        "Contents/Config-1-Partition",
        &crate::test_support::parasolid::parasolid_with_body(
            "partition body",
            "SCH_SW_33103_11000",
            &triangle_body(),
        ),
    );
    let (_, losses) = with_damaged("NO", Some(&partition));
    assert!(
        losses.iter().any(
            |loss| loss.code == cadmpeg_ir::unknown::part::record_unreadable_kind()
                && loss.message.contains("\"Contents/Config-1-Partition\"")
        ),
        "{losses:#?}"
    );
    assert!(not_stored(&losses).is_empty(), "{losses:#?}");
    let unstated = losses
        .iter()
        .filter(|loss| loss.code == SldprtLossCode::ConfigStateUnstated.kind())
        .map(|loss| loss.message.as_str())
        .collect::<Vec<_>>();
    let [message] = unstated[..] else {
        panic!("one loss: {losses:#?}");
    };
    assert!(message.contains("\"Long\""), "{message}");
    assert!(
        message.contains("its stored partition is given up, so its state is not stated"),
        "{message}"
    );
}
