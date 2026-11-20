use crate::layout::{
    sheet_metal_contour_expanded_prefix as expanded, sheet_metal_contour_short_prefix as short,
};
use crate::records::identity::Located;
use cadmpeg_ir::{
    features::FeatureDirection3,
    scalar::{FiniteReal, PositiveReal},
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct DesignContourFlangeOperation {
    pub(crate) operation: Located<u32>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_carrier"
    )]
    pub(crate) carrier: Option<Located<u32>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_settings"
    )]
    pub(crate) settings: Option<Located<u32>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_profile_control"
    )]
    pub(crate) profile_control: Option<Located<u32>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_inline_thickness"
    )]
    pub(crate) inline_thickness: Option<Located<PositiveReal>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_travel"
    )]
    pub(crate) travel: Option<DesignContourTravel>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_rule"
    )]
    pub(crate) rule: Option<DesignContourRule>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct DesignContourTravel {
    pub(crate) owner: Located<u32>,
    #[serde(default)]
    pub(crate) owner_role_assumed: bool,
    pub(crate) direction: Located<FeatureDirection3>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct DesignContourRule {
    pub(crate) record_index: u32,
    pub(crate) reference_assumed: bool,
    pub(crate) first_value: Located<FiniteReal>,
    pub(crate) thickness: Located<PositiveReal>,
    pub(crate) material_scalar: Located<FiniteReal>,
}

cadmpeg_core::named_optional_field!(deserialize_travel, DesignContourTravel, "travel");
cadmpeg_core::named_optional_field!(deserialize_rule, DesignContourRule, "rule");
rewrite_native_scalar!(DesignContourFlangeOperation);

impl DesignContourFlangeOperation {
    pub(crate) fn valid_for_scope(
        &self,
        scope: &super::super::scope::DesignParameterScope,
    ) -> bool {
        let has_reference = |index| {
            scope
                .reference_members()
                .values()
                .any(|value| *value == index)
        };
        let within = |offset: u64, width: u64| {
            offset >= scope.byte_offset()
                && offset
                    .checked_add(width)
                    .is_some_and(|end| end <= scope.reference_count_offset())
        };
        self.operation
            .offset
            .checked_sub(scope.byte_offset())
            .is_some_and(|offset| {
                [short::OPERATION, expanded::OPERATION]
                    .into_iter()
                    .any(|expected| cadmpeg_core::decode::u64_from_index(expected) == offset)
            })
            && within(self.operation.offset, 4)
            && self
                .carrier
                .as_ref()
                .is_none_or(|carrier| has_reference(carrier.value) && within(carrier.offset, 11))
            && self
                .settings
                .as_ref()
                .is_none_or(|settings| has_reference(settings.value) && within(settings.offset, 11))
            && self.profile_control.as_ref().is_none_or(|control| {
                within(control.offset, 4)
                    && self.carrier.as_ref().is_some_and(|carrier| {
                        control.offset.checked_add(5) == Some(carrier.offset)
                    })
            })
            && self.inline_thickness.as_ref().is_none_or(|thickness| {
                within(thickness.offset, 8)
                    && self.carrier.as_ref().is_some_and(|carrier| {
                        matches!(thickness.offset.checked_sub(carrier.offset), Some(11 | 51))
                    })
            })
            && self.travel.as_ref().is_none_or(|travel| {
                has_reference(travel.owner.value)
                    && within(travel.owner.offset, 11)
                    && within(travel.direction.offset, 24)
                    && travel.direction.offset.checked_add(25) == Some(travel.owner.offset)
            })
    }
}

cadmpeg_core::named_optional_field!(deserialize_carrier, Located<u32>, "carrier");

cadmpeg_core::named_optional_field!(deserialize_settings, Located<u32>, "settings");

cadmpeg_core::named_optional_field!(deserialize_profile_control, Located<u32>, "profile_control");

cadmpeg_core::named_optional_field!(
    deserialize_inline_thickness,
    Located<PositiveReal>,
    "inline_thickness"
);
