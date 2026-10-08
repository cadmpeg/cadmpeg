// SPDX-License-Identifier: Apache-2.0
//! Work admission for typed semantic leaves.

use std::collections::{BTreeMap, BTreeSet};
use cadmpeg_core::decode::{DecodePolicy, ResourceDimension};
use crate::parse::Value;

fn omitted_wrapper() -> Value {
    Value::Typed("WRAP".into(), Box::new(Value::Omitted))
}

fn exchange() -> crate::parse::Exchange {
    crate::test_support::with_service_context(b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=ITEM();ENDSEC;END-ISO-10303-21;", crate::parse::parse_inner).expect("exchange").0
}

#[test]
fn typed_datum_modifier_descent_refuses_work_limit() {
    let exchange = exchange();
    let value = omitted_wrapper();
    cadmpeg_test_support::refusal::resource_limit_at(ResourceDimension::WorkUnits, "STEP typed datum modifier descent", |limit| {
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = limit;
        crate::test_support::with_policy_context(b"", &policy, |_, ctx| {
            let mut losses = Vec::new();
            let reports = std::cell::RefCell::new(ctx.reserve_scoped(0, "report fixture")?);
            let mut measurements = super::super::MeasureContext { length_scale: 1.0, angle_scale: 1.0, graph_limit: 64, losses: (&mut losses, &reports) };
            let mut claims = ctx.reserve_scoped(0, "claim fixture")?;
            let mut text = ctx.reserve_scoped(0, "text fixture")?;
            super::super::modifier_text(&value, &exchange, (&mut BTreeSet::new(), &mut claims), &mut measurements, &mut text, ctx).map(|_| ())
        })
    });
}

#[test]
fn typed_measure_id_descent_refuses_work_limit() {
    let exchange = exchange();
    let value = omitted_wrapper();
    cadmpeg_test_support::refusal::resource_limit_at(ResourceDimension::WorkUnits, "STEP typed measure ID descent", |limit| {
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = limit;
        crate::test_support::with_policy_context(b"", &policy, |_, ctx| {
            super::super::collect_measure_ids(&value, &exchange, &mut BTreeMap::new(), 0, 64, &mut BTreeSet::new(), ctx)
        })
    });
}
