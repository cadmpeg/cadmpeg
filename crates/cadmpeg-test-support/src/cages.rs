// SPDX-License-Identifier: Apache-2.0
//! A subdivision cage held as a body of its own (§5.6), and what a writer
//! that writes no cage reports for it.

use cadmpeg_ir::ids::{BodyId, RegionId, ShellId, SubdId};
use cadmpeg_ir::report::export::ExportReport;
use cadmpeg_ir::CadIr;

/// The ids of a cage that [`push_cage_body`] added.
#[derive(Debug, Clone)]
pub struct CageBody {
    /// The cage.
    pub subd: SubdId,
    /// The body that holds only the cage.
    pub body: BodyId,
}

/// Adds to `ir` an empty Catmull-Clark cage held as a body of its own, with
/// the ids `test:<codec>:{subd,body,region,shell}#cage`. The definition that
/// owns the first body of `ir` owns the cage body too.
///
/// # Panics
///
/// Panics where `codec` makes an invalid identity.
pub fn push_cage_body(ir: &mut CadIr, codec: &str) -> CageBody {
    let id = |kind: &str| format!("test:{codec}:{kind}#cage");
    let subd = SubdId::mint(id("subd")).expect("identity grammar");
    let body = BodyId::mint(id("body")).expect("identity grammar");
    let owner = ir.geometry.bodies.first().map(|first| first.id.clone());
    ir.geometry.push_cage_object(
        cadmpeg_ir::SubdSurface {
            id: subd.clone(),
            scheme: cadmpeg_ir::SubdScheme::CatmullClark,
            source_object: None,
            cage: cadmpeg_ir::subd::SubdCage::default(),
            native_ref: None,
        },
        body.clone(),
        RegionId::mint(id("region")).expect("identity grammar"),
        ShellId::mint(id("shell")).expect("identity grammar"),
    );
    if let Some(definition) = owner.and_then(|owner| {
        ir.model
            .product_definitions
            .iter_mut()
            .find(|definition| definition.bodies.contains(&owner))
    }) {
        definition.bodies.push(body.clone());
    }
    CageBody { subd, body }
}

/// Asserts that one loss of `report` names the cage and its body, and that
/// the census counts `written` records under `key`.
///
/// # Panics
///
/// Panics where either does not hold.
pub fn assert_cage_body_dropped(report: &ExportReport, cage: &CageBody, key: &str, written: usize) {
    assert!(
        report.losses.iter().any(|loss| {
            loss.message.contains(cage.subd.as_str()) && loss.message.contains(cage.body.as_str())
        }),
        "a loss names {} and {}: {:#?}",
        cage.subd,
        cage.body,
        report.losses
    );
    assert_eq!(
        report.census.counts.get(key),
        Some(&written),
        "census {key}: {:#?}",
        report.census.counts
    );
}
