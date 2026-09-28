// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use cadmpeg_ir::ids::PcurveId;
use cadmpeg_ir::topology::PcurveUse;

use super::super::{one_coedge_pcurve_use, push_untransferred_pcurve_loss};

fn loss_result(collection_limit: u64, retained_limit: u64) -> Result<Vec<cadmpeg_ir::report::loss::LossNote>, CodecError> {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = collection_limit;
    policy.limits.max_retained_bytes = retained_limit;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("empty root admitted");
    let mut losses = Vec::new();
    push_untransferred_pcurve_loss(&ctx, &mut losses, 10, 5, "refused carrier")?;
    Ok(losses)
}

fn assert_refusal(error: CodecError, dimension: ResourceDimension, operation: &'static str) {
    assert!(matches!(error, CodecError::ResourceLimit(resource)
        if resource.dimension == dimension && resource.operation == operation), "{error:?}");
}

#[test]
fn brep_untransferred_pcurve_loss_text_refuses_retained_limit() {
    assert_refusal(loss_result(16, 0).err().expect("loss text refused"),
        ResourceDimension::RetainedBytes, "creo B-rep untransferred pcurve loss text");
}

#[test]
fn brep_untransferred_pcurve_losses_refuse_collection_limit() {
    assert_refusal(loss_result(0, u64::MAX).err().expect("loss row refused"),
        ResourceDimension::CollectionItems, "creo B-rep untransferred pcurve losses");
}

#[test]
fn brep_untransferred_pcurve_loss_preserves_service_message() {
    let losses = loss_result(16, u64::MAX).expect("service loss admitted");
    assert_eq!(losses.len(), 1);
    assert_eq!(losses[0].message,
        "VisibGeom curve row 10 on face 5 states no pcurve carrier: refused carrier");
}

fn pcurve_use_result(limit: u64, value: Option<PcurveUse>) -> Result<Vec<PcurveUse>, CodecError> {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = limit;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("empty root admitted");
    one_coedge_pcurve_use(&ctx, value)
}

fn pcurve_use() -> PcurveUse {
    PcurveUse {
        pcurve: PcurveId::compose(&crate::identity::VISIBGEOM_PCURVE,
            cadmpeg_ir::ids::IdentityKey::from(10).colon(5)),
        isoparametric: None,
        parameter_range: None,
    }
}

#[test]
fn brep_coedge_pcurve_uses_refuse_collection_limit() {
    assert_refusal(pcurve_use_result(0, Some(pcurve_use())).err().expect("pcurve use refused"),
        ResourceDimension::CollectionItems, "creo B-rep coedge pcurve uses");
}

#[test]
fn brep_coedge_pcurve_uses_preserve_service_value() {
    let value = pcurve_use();
    assert_eq!(pcurve_use_result(16, Some(value.clone())).expect("service use admitted"), vec![value]);
    assert!(pcurve_use_result(0, None).expect("empty use admitted").is_empty());
}
