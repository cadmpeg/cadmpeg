// SPDX-License-Identifier: Apache-2.0
//! Relation scalar membership and selected parameter/display roles.

use super::charged_clone::CloneCharged;
use super::{FeatureInputScalar, FeatureInputScalarRole};
use serde::{ser::SerializeMap, Deserialize, Deserializer, Serialize, Serializer};

#[derive(Debug, Clone, PartialEq, Eq)]
/// Nonempty, distinct, nonblank scalar identities with disjoint selected roles.
pub(crate) struct RelationScalars {
    refs: Vec<String>,
    parameter: Option<usize>,
    display: Option<usize>,
}

impl CloneCharged for RelationScalars {
    fn clone_charged(
        &self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        operation: &'static str,
    ) -> Result<Self, cadmpeg_core::CodecError> {
        Ok(Self {
            refs: self.refs.clone_charged(ctx, operation)?,
            parameter: self.parameter,
            display: self.display,
        })
    }
}

impl RelationScalars {
    pub(crate) fn from_scalars<'a, S, F>(
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        scalars: &'a S,
        scalar_ref: F,
    ) -> Result<Self, cadmpeg_core::CodecError>
    where
        S: cadmpeg_core::decode::iter_source::IterSource + ?Sized + 'a,
        F: Fn(<S::Iter<'a> as Iterator>::Item) -> &'a FeatureInputScalar,
    {
        let mut refs = Vec::new();
        let mut parameter = None;
        let mut display = None;
        let mut duplicate_parameter = false;
        let mut duplicate_display = false;
        for value in ctx.admit_iter(scalars, "select SLDPRT relation scalar roles")? {
            let scalar = scalar_ref(value);
            admit_member(ctx, &refs, &scalar.id)?;
            let index = refs.len();
            match scalar.role {
                FeatureInputScalarRole::Driving => {
                    duplicate_parameter |= parameter.is_some();
                    parameter = Some(index);
                }
                FeatureInputScalarRole::Display => {
                    duplicate_display |= display.is_some();
                    display = Some(index);
                }
                FeatureInputScalarRole::Native => {}
            }
            ctx.reserve_vec(&mut refs, 1, "collect SLDPRT relation scalar references")?;
            refs.push(ctx.format_retained(
                format_args!("{}", scalar.id),
                "retain SLDPRT relation scalar identity",
            )?);
        }
        if refs.is_empty() {
            return Err(cadmpeg_core::CodecError::malformed(
                "scalar_refs must be nonempty",
            ));
        }
        Ok(Self {
            parameter: if duplicate_parameter { None } else { parameter },
            display: if duplicate_display { None } else { display },
            refs,
        })
    }

    pub(crate) fn from_refs(
        refs: Vec<String>,
        parameter: Option<String>,
        display: Option<String>,
    ) -> Result<Self, String> {
        if refs.is_empty() {
            return Err("scalar_refs must be nonempty".into());
        }
        for (index, id) in refs.iter().enumerate() {
            check_member(&refs[..index], id).map_err(str::to_owned)?;
        }
        let index = |id: Option<String>, field| {
            id.map(|id| {
                refs.iter()
                    .position(|member| member == &id)
                    .ok_or_else(|| format!("{field} must be a member of scalar_refs"))
            })
            .transpose()
        };
        let parameter = index(parameter, "parameter_scalar_ref")?;
        let display = index(display, "display_scalar_ref")?;
        if parameter.is_some() && parameter == display {
            return Err("parameter_scalar_ref and display_scalar_ref must be distinct".into());
        }
        Ok(Self {
            refs,
            parameter,
            display,
        })
    }

    pub(super) fn refs(&self) -> &[String] {
        &self.refs
    }

    pub(super) fn parameter(&self) -> Option<&str> {
        self.parameter.map(|index| self.refs[index].as_str())
    }

    pub(super) fn display(&self) -> Option<&str> {
        self.display.map(|index| self.refs[index].as_str())
    }

    pub(crate) fn push(
        &mut self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        id: &str,
    ) -> Result<(), cadmpeg_core::CodecError> {
        admit_member(ctx, &self.refs, id)?;
        ctx.reserve_vec(
            &mut self.refs,
            1,
            "collect SLDPRT relation scalar references",
        )?;
        self.refs.push(ctx.format_retained(
            format_args!("{id}"),
            "retain SLDPRT relation scalar identity",
        )?);
        Ok(())
    }

    pub(crate) fn push_parameter(
        &mut self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        id: &str,
    ) -> Result<(), cadmpeg_core::CodecError> {
        if self.parameter.is_some() {
            return Err(cadmpeg_core::CodecError::malformed(
                "parameter_scalar_ref is already selected",
            ));
        }
        let index = self.refs.len();
        self.push(ctx, id)?;
        self.parameter = Some(index);
        Ok(())
    }

    #[cfg(test)]
    pub(crate) fn clear_parameter(&mut self) {
        self.parameter = None;
    }

    #[cfg(test)]
    pub(crate) fn clear_display(&mut self) {
        self.display = None;
    }
}

fn check_member(refs: &[String], id: &str) -> Result<(), &'static str> {
    if id.trim().is_empty() {
        return Err("scalar_refs identities must be nonblank");
    }
    if refs.iter().any(|member| member == id) {
        return Err("scalar_refs identities must be distinct");
    }
    Ok(())
}

fn admit_member(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    refs: &[String],
    id: &str,
) -> Result<(), cadmpeg_core::CodecError> {
    ctx.charge_work(
        cadmpeg_core::decode::u64_from_index(refs.len()),
        "measure SLDPRT relation scalar comparisons",
    )?;
    let length = cadmpeg_core::decode::u64_from_index(id.len());
    let work = refs
        .iter()
        .try_fold(length, |work, member| {
            work.checked_add(length)?
                .checked_add(cadmpeg_core::decode::u64_from_index(member.len()))?
                .checked_add(1)
        })
        .ok_or_else(|| {
            ctx.refuse_codec_limit("check SLDPRT relation scalar identity", u64::MAX, u64::MAX)
        })?;
    ctx.charge_work(work, "check SLDPRT relation scalar identity")?;
    check_member(refs, id).map_err(cadmpeg_core::CodecError::malformed)
}

#[derive(Deserialize)]
pub(crate) struct RelationScalarsWire {
    scalar_refs: Vec<String>,
    #[serde(default, deserialize_with = "deserialize_parameter_scalar_ref")]
    parameter_scalar_ref: Option<String>,
    #[serde(default, deserialize_with = "deserialize_display_scalar_ref")]
    display_scalar_ref: Option<String>,
}

impl RelationScalarsWire {
    pub(crate) fn into_checked(self) -> Result<RelationScalars, String> {
        RelationScalars::from_refs(
            self.scalar_refs,
            self.parameter_scalar_ref,
            self.display_scalar_ref,
        )
    }

    pub(crate) fn admit(
        self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    ) -> Result<RelationScalars, cadmpeg_core::CodecError> {
        const OPERATION: &str = "admit SLDPRT relation scalar membership";
        let count = cadmpeg_core::decode::u64_from_index(self.scalar_refs.len());
        ctx.charge_work(count, OPERATION)?;
        let work = (|| {
            let total = self.scalar_refs.iter().try_fold(0u64, |sum, id| {
                sum.checked_add(cadmpeg_core::decode::u64_from_index(id.len()))
            })?;
            let mut work = count.checked_mul(total)?;
            if count != 0 {
                work =
                    work.checked_add(count.checked_mul(count.checked_sub(1)?)?.checked_div(2)?)?;
            }
            for selected in [&self.parameter_scalar_ref, &self.display_scalar_ref]
                .into_iter()
                .flatten()
            {
                let length = cadmpeg_core::decode::u64_from_index(selected.len());
                work = work
                    .checked_add(total)?
                    .checked_add(count.checked_mul(length.checked_add(1)?)?)?;
            }
            work.checked_add(1)
        })()
        .ok_or_else(|| ctx.refuse_codec_limit(OPERATION, u64::MAX, u64::MAX))?;
        ctx.charge_work(work, OPERATION)?;
        self.into_checked().map_err(|error| {
            cadmpeg_core::CodecError::malformed(format_args!("relation instance: {error}"))
        })
    }
}

impl Serialize for RelationScalars {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(None)?;
        map.serialize_entry("scalar_refs", &self.refs)?;
        if let Some(id) = self.parameter() {
            map.serialize_entry("parameter_scalar_ref", id)?;
        }
        if let Some(id) = self.display() {
            map.serialize_entry("display_scalar_ref", id)?;
        }
        map.end()
    }
}

impl<'de> Deserialize<'de> for RelationScalars {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let wire = RelationScalarsWire::deserialize(deserializer)?;
        wire.into_checked().map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::RelationScalars;

    #[test]
    fn selected_scalars_are_members_on_the_flat_wire() {
        let wire = serde_json::json!({
            "scalar_refs": ["display", "driver"],
            "parameter_scalar_ref": "driver", "display_scalar_ref": "display"
        });
        let scalars: RelationScalars = serde_json::from_value(wire.clone()).unwrap();
        assert_eq!(scalars.parameter(), Some("driver"));
        assert_eq!(scalars.display(), Some("display"));
        assert_eq!(serde_json::to_value(scalars).unwrap(), wire);
        for field in ["parameter_scalar_ref", "display_scalar_ref"] {
            let mut invalid = wire.clone();
            invalid[field] = serde_json::json!("missing");
            let error = serde_json::from_value::<RelationScalars>(invalid).unwrap_err();
            assert!(error.to_string().contains(field));
        }
    }

    #[test]
    fn relation_scalars_wire_refuses_a_null_selected_scalar() {
        for key in ["parameter_scalar_ref", "display_scalar_ref"] {
            let mut wire = serde_json::json!({"scalar_refs": ["display", "driver"]});
            wire[key] = serde_json::Value::Null;
            assert!(
                serde_json::from_value::<RelationScalars>(wire.clone()).is_err(),
                "{wire}"
            );
        }
        assert!(
            serde_json::from_value::<RelationScalars>(serde_json::json!({"scalar_refs": []}))
                .is_err()
        );
        let absent: RelationScalars =
            serde_json::from_value(serde_json::json!({"scalar_refs": ["member"]})).unwrap();
        assert!(absent.parameter().is_none());
    }
    #[test]
    fn relation_membership_refuses_empty_blank_duplicate_and_conflicting_roles() {
        for wire in [
            serde_json::json!({"scalar_refs": []}),
            serde_json::json!({"scalar_refs": [" "]}),
            serde_json::json!({"scalar_refs": ["s", "s"]}),
            serde_json::json!({"scalar_refs": ["s"], "parameter_scalar_ref": "s", "display_scalar_ref": "s"}),
        ] {
            assert!(serde_json::from_value::<RelationScalars>(wire).is_err());
        }
        assert!(RelationScalars::from_scalars(
            &cadmpeg_test_support::service_decode_context(),
            &[] as &[&super::FeatureInputScalar],
            |scalar| *scalar,
        )
        .is_err());
    }

    #[test]
    fn relation_membership_mutation_refuses_invalid_changes_without_mutation() {
        let ctx = cadmpeg_test_support::service_decode_context();
        let mut members =
            RelationScalars::from_refs(vec!["s".into()], None, Some("s".into())).unwrap();
        let before = members.clone();
        assert!(members.push(&ctx, "s").is_err());
        assert!(members.push(&ctx, " ").is_err());
        assert!(members.push_parameter(&ctx, "s").is_err());
        assert_eq!(members, before);
        members.push_parameter(&ctx, "driver").unwrap();
        let before = members.clone();
        assert!(members.push_parameter(&ctx, "other").is_err());
        assert_eq!(members, before);
    }

    #[test]
    fn relation_membership_checks_refuse_unadmitted_comparison_work() {
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let mut policy = cadmpeg_core::decode::DecodePolicy::service();
        policy.limits.max_work_units = 0;
        let (ctx, _) =
            cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let mut members = RelationScalars::from_refs(vec!["s".into()], None, None).unwrap();
        let before = members.clone();
        assert!(matches!(
            members.push(&ctx, "other"),
            Err(cadmpeg_core::CodecError::ResourceLimit(_))
        ));
        assert_eq!(members, before);
    }
}

// Each optional key below names itself in whatever it refuses.
cadmpeg_core::named_optional_field!(
    deserialize_parameter_scalar_ref,
    String,
    "parameter_scalar_ref"
);
cadmpeg_core::named_optional_field!(deserialize_display_scalar_ref, String, "display_scalar_ref");
