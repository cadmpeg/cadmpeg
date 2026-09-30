// SPDX-License-Identifier: Apache-2.0
//! Charged copies of native lane records and their nested collections.

use super::CloneCharged;
use crate::records::{
    FeatureInputBodySelection, FeatureInputClass, FeatureInputComponentPathEntry,
    FeatureInputEdgeSelection, FeatureInputGeneratedSurfaceIdentity, FeatureInputLane,
    FeatureInputName, FeatureInputOperand, FeatureInputReference, FeatureInputRelationBinding,
    FeatureInputRelationInstance, FeatureInputScalar, FeatureInputSurfaceSelection,
    SketchInputEntity, SketchInputLink, SketchInputLinks,
};
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;

impl CloneCharged for FeatureInputLane {
    fn clone_charged(
        &self,
        ctx: &DecodeContext<'_>,
        operation: &'static str,
    ) -> Result<Self, CodecError> {
        #[cfg(test)]
        crate::records::FEATURE_INPUT_LANE_CLONE_COUNT.with(|count| count.set(count.get() + 1));
        Ok(Self {
            id: self.id.clone_charged(ctx, operation)?,
            configuration: self.configuration.clone_charged(ctx, operation)?,
            native_payload: crate::byte_admission::copy_retained(
                ctx,
                &self.native_payload,
                operation,
            )?,
            classes: self.classes.clone_charged(ctx, operation)?,
            names: self.names.clone_charged(ctx, operation)?,
            scalars: self.scalars.clone_charged(ctx, operation)?,
            relation_bindings: self.relation_bindings.clone_charged(ctx, operation)?,
            relation_instances: self.relation_instances.clone_charged(ctx, operation)?,
            body_selections: self.body_selections.clone_charged(ctx, operation)?,
            edge_selections: self.edge_selections.clone_charged(ctx, operation)?,
            surface_selections: self.surface_selections.clone_charged(ctx, operation)?,
            generated_surface_identities: self
                .generated_surface_identities
                .clone_charged(ctx, operation)?,
            references: self.references.clone_charged(ctx, operation)?,
            sketch_entities: self.sketch_entities.clone_charged(ctx, operation)?,
        })
    }
}

impl CloneCharged for FeatureInputBodySelection {
    fn clone_charged(
        &self,
        ctx: &DecodeContext<'_>,
        operation: &'static str,
    ) -> Result<Self, CodecError> {
        Ok(Self {
            id: self.id.clone_charged(ctx, operation)?,
            parent: self.parent.clone_charged(ctx, operation)?,
            ordinal: self.ordinal,
            offset: self.offset,
            object_name_ref: self.object_name_ref.clone_charged(ctx, operation)?,
            feature_ref: self.feature_ref.clone_charged(ctx, operation)?,
            local_body_ids: self.local_body_ids.clone_charged(ctx, operation)?,
            body_state_ids: self.body_state_ids.clone_charged(ctx, operation)?,
            mode: self.mode,
        })
    }
}

impl CloneCharged for FeatureInputEdgeSelection {
    fn clone_charged(
        &self,
        ctx: &DecodeContext<'_>,
        operation: &'static str,
    ) -> Result<Self, CodecError> {
        Ok(Self {
            id: self.id.clone_charged(ctx, operation)?,
            parent: self.parent.clone_charged(ctx, operation)?,
            ordinal: self.ordinal,
            offset: self.offset,
            object_name_ref: self.object_name_ref.clone_charged(ctx, operation)?,
            feature_ref: self.feature_ref.clone_charged(ctx, operation)?,
            local_edge_ids: self.local_edge_ids.clone_charged(ctx, operation)?,
            components: self.components.clone_charged(ctx, operation)?,
            references: self.references.clone_charged(ctx, operation)?,
            producer_feature_refs: self.producer_feature_refs.clone_charged(ctx, operation)?,
            terminal_feature_ref: self.terminal_feature_ref.clone_charged(ctx, operation)?,
        })
    }
}

impl CloneCharged for FeatureInputSurfaceSelection {
    fn clone_charged(
        &self,
        ctx: &DecodeContext<'_>,
        operation: &'static str,
    ) -> Result<Self, CodecError> {
        Ok(Self {
            id: self.id.clone_charged(ctx, operation)?,
            parent: self.parent.clone_charged(ctx, operation)?,
            ordinal: self.ordinal,
            offset: self.offset,
            selector: self.selector,
            kind: self.kind,
            object_name_ref: self.object_name_ref.clone_charged(ctx, operation)?,
            feature_ref: self.feature_ref.clone_charged(ctx, operation)?,
            producer_feature_refs: self.producer_feature_refs.clone_charged(ctx, operation)?,
            terminal_feature_ref: self.terminal_feature_ref.clone_charged(ctx, operation)?,
            components: self.components.clone_charged(ctx, operation)?,
        })
    }
}

impl CloneCharged for FeatureInputGeneratedSurfaceIdentity {
    fn clone_charged(
        &self,
        ctx: &DecodeContext<'_>,
        operation: &'static str,
    ) -> Result<Self, CodecError> {
        Ok(Self {
            id: self.id.clone_charged(ctx, operation)?,
            parent: self.parent.clone_charged(ctx, operation)?,
            ordinal: self.ordinal,
            offset: self.offset,
            type_prefix: self.type_prefix,
            feature_source_id: self.feature_source_id,
            local_identity: self.local_identity,
            components: self.components.clone_charged(ctx, operation)?,
        })
    }
}

impl CloneCharged for FeatureInputComponentPathEntry {
    fn clone_charged(
        &self,
        _ctx: &DecodeContext<'_>,
        _operation: &'static str,
    ) -> Result<Self, CodecError> {
        Ok(Self {
            instance: self.instance,
            type_signature: self.type_signature,
            local_id: self.local_id,
        })
    }
}

impl CloneCharged for FeatureInputRelationBinding {
    fn clone_charged(
        &self,
        ctx: &DecodeContext<'_>,
        operation: &'static str,
    ) -> Result<Self, CodecError> {
        Ok(Self {
            id: self.id.clone_charged(ctx, operation)?,
            parent: self.parent.clone_charged(ctx, operation)?,
            ordinal: self.ordinal,
            offset: self.offset,
            class_ref: self.class_ref.clone_charged(ctx, operation)?,
            family: self.family,
            scalar_ref: self.scalar_ref.clone_charged(ctx, operation)?,
            feature_ref: self.feature_ref.clone_charged(ctx, operation)?,
        })
    }
}

impl CloneCharged for FeatureInputRelationInstance {
    fn clone_charged(
        &self,
        ctx: &DecodeContext<'_>,
        operation: &'static str,
    ) -> Result<Self, CodecError> {
        Ok(Self {
            id: self.id.clone_charged(ctx, operation)?,
            parent: self.parent.clone_charged(ctx, operation)?,
            ordinal: self.ordinal,
            offset: self.offset,
            family: self.family,
            class_ref: self.class_ref.clone_charged(ctx, operation)?,
            feature_ref: self.feature_ref.clone_charged(ctx, operation)?,
            scalars: self.scalars.clone_charged(ctx, operation)?,
            operands: self.operands.clone_charged(ctx, operation)?,
        })
    }
}

impl CloneCharged for FeatureInputReference {
    fn clone_charged(
        &self,
        ctx: &DecodeContext<'_>,
        operation: &'static str,
    ) -> Result<Self, CodecError> {
        Ok(Self {
            id: self.id.clone_charged(ctx, operation)?,
            parent: self.parent.clone_charged(ctx, operation)?,
            feature_ref: self.feature_ref.clone_charged(ctx, operation)?,
            ordinal: self.ordinal,
            offset: self.offset,
            kind: self.kind,
            class_ref: self.class_ref.clone_charged(ctx, operation)?,
            object_index: self.object_index,
        })
    }
}

impl CloneCharged for FeatureInputName {
    fn clone_charged(
        &self,
        ctx: &DecodeContext<'_>,
        operation: &'static str,
    ) -> Result<Self, CodecError> {
        Ok(Self {
            id: self.id.clone_charged(ctx, operation)?,
            parent: self.parent.clone_charged(ctx, operation)?,
            ordinal: self.ordinal,
            offset: self.offset,
            object_id: self.object_id,
            value: self.value.clone_charged(ctx, operation)?,
        })
    }
}

impl CloneCharged for FeatureInputScalar {
    fn clone_charged(
        &self,
        ctx: &DecodeContext<'_>,
        operation: &'static str,
    ) -> Result<Self, CodecError> {
        Ok(Self {
            id: self.id.clone_charged(ctx, operation)?,
            parent: self.parent.clone_charged(ctx, operation)?,
            feature_ref: self.feature_ref.clone_charged(ctx, operation)?,
            ordinal: self.ordinal,
            offset: self.offset,
            object_id: self.object_id,
            name: self.name.clone_charged(ctx, operation)?,
            value: self.value,
            role: self.role,
            operands: self.operands.clone_charged(ctx, operation)?,
        })
    }
}

impl CloneCharged for FeatureInputOperand {
    fn clone_charged(
        &self,
        ctx: &DecodeContext<'_>,
        operation: &'static str,
    ) -> Result<Self, CodecError> {
        Ok(Self {
            offset: self.offset,
            reference_ref: self.reference_ref.clone_charged(ctx, operation)?,
            kind: self.kind,
            entity_index: self.entity_index,
            entity_ref: self.entity_ref.clone_charged(ctx, operation)?,
        })
    }
}

impl CloneCharged for FeatureInputClass {
    fn clone_charged(
        &self,
        ctx: &DecodeContext<'_>,
        operation: &'static str,
    ) -> Result<Self, CodecError> {
        Ok(Self {
            id: self.id.clone_charged(ctx, operation)?,
            parent: self.parent.clone_charged(ctx, operation)?,
            ordinal: self.ordinal,
            offset: self.offset,
            name: self.name.clone_charged(ctx, operation)?,
        })
    }
}

impl CloneCharged for SketchInputEntity {
    fn clone_charged(
        &self,
        ctx: &DecodeContext<'_>,
        operation: &'static str,
    ) -> Result<Self, CodecError> {
        Ok(Self {
            id: self.id.clone_charged(ctx, operation)?,
            parent: self.parent.clone_charged(ctx, operation)?,
            feature_ref: self.feature_ref.clone_charged(ctx, operation)?,
            ordinal: self.ordinal,
            offset: self.offset,
            object_index: self.object_index,
            local_id: self.local_id,
            kind: self.kind,
            state_value: self.state_value,
            coordinates_m: self.coordinates_m,
            links: self.links.clone_charged(ctx, operation)?,
        })
    }
}

impl CloneCharged for SketchInputLinks {
    fn clone_charged(
        &self,
        ctx: &DecodeContext<'_>,
        operation: &'static str,
    ) -> Result<Self, CodecError> {
        Ok(Self {
            selector: self.selector,
            entries: self.entries.clone_charged(ctx, operation)?,
        })
    }
}

impl CloneCharged for SketchInputLink {
    fn clone_charged(
        &self,
        ctx: &DecodeContext<'_>,
        operation: &'static str,
    ) -> Result<Self, CodecError> {
        Ok(Self {
            local_id: self.local_id,
            entity_ref: self.entity_ref.clone_charged(ctx, operation)?,
        })
    }
}
