// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::CodecError;
use cadmpeg_ir::ids::PcurveId;
use cadmpeg_ir::topology::PcurveUse;

use super::super::{one_coedge_pcurve_use, push_untransferred_pcurve_loss};

fn loss_result() -> Result<Vec<cadmpeg_ir::report::loss::LossNote>, CodecError> {
    let mut losses = Vec::new();
    push_untransferred_pcurve_loss(&mut losses, 10, 5, "refused carrier");
    Ok(losses)
}

#[test]
fn brep_untransferred_pcurve_loss_preserves_service_message() {
    let losses = loss_result().expect("service loss admitted");
    assert_eq!(losses.len(), 1);
    assert_eq!(
        losses[0].message,
        "VisibGeom curve row 10 on face 5 states no pcurve carrier: refused carrier"
    );
}

fn pcurve_use() -> PcurveUse {
    PcurveUse {
        pcurve: PcurveId::compose(
            &crate::identity::VISIBGEOM_PCURVE,
            cadmpeg_ir::ids::IdentityKey::from(10).colon(5),
        ),
        isoparametric: None,
        parameter_range: None,
    }
}

#[test]
fn brep_coedge_pcurve_uses_preserve_service_value() {
    let value = pcurve_use();
    assert_eq!(one_coedge_pcurve_use(Some(value.clone())), vec![value]);
    assert!(one_coedge_pcurve_use(None).is_empty());
}
