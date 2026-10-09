// SPDX-License-Identifier: Apache-2.0
//! Scoped parameter overrides with restoration before storage teardown.

use cadmpeg_core::decode::{DecodeContext, ScopedReservation};
use cadmpeg_core::CodecError;
use cadmpeg_ir::features::{DesignParameter, ParameterId, ParameterValue};
use std::collections::BTreeMap;

pub(super) struct ConfigurationParameterOverlay<'parameters, 'ctx> {
    parameters: &'parameters mut [DesignParameter],
    saved_values: Vec<(usize, Option<ParameterValue>)>,
    storage: ScopedReservation<'ctx>,
}

impl<'parameters, 'ctx> ConfigurationParameterOverlay<'parameters, 'ctx> {
    pub(super) fn new(
        ctx: &'ctx DecodeContext<'_>,
        parameters: &'parameters mut [DesignParameter],
        values: &BTreeMap<ParameterId, ParameterValue>,
    ) -> Result<Self, CodecError> {
        const OPERATION: &str = "overlay SLDPRT configuration parameter values";
        let storage = ctx.reserve_scoped(0, OPERATION)?;
        let mut overlay = Self { parameters, saved_values: Vec::new(), storage };
        for (index, parameter) in ctx.admit_iter(&mut *overlay.parameters, OPERATION)?.enumerate() {
            let Some(value) = ctx.get_btree_map(values, &parameter.id, "look up SLDPRT ordered key")? else {
                continue;
            };
            let copied = overlay.storage.with_storage(|| value.try_clone_for_decode(ctx, OPERATION))?;
            overlay.storage.with_storage(|| ctx.reserve_vec(&mut overlay.saved_values, 1, OPERATION))?;
            ctx.charge_work(1, "restore SLDPRT configuration parameter values")?;
            overlay.saved_values.push((index, parameter.value.replace(copied)));
        }
        Ok(overlay)
    }

    pub(super) fn as_slice(&self) -> &[DesignParameter] {
        self.parameters
    }
}

impl Drop for ConfigurationParameterOverlay<'_, '_> {
    fn drop(&mut self) {
        // Each restoration was admitted before the matching replacement.
        // Drop override children while their scoped storage is still live.
        for (index, value) in self.saved_values.drain(..) {
            self.parameters[index].value = value;
        }
    }
}

#[cfg(test)]
mod tests;
