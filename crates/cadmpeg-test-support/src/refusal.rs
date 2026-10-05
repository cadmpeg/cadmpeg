// SPDX-License-Identifier: Apache-2.0
//! Wire-key and resource-budget refusal oracles.
//!
//! A native wire type states the key it refuses. These helpers read one key
//! through the type's own `Deserialize` implementation and check the message.

/// The message `T` states when `key` is present and null.
///
/// # Panics
///
/// Panics when `T` admits the null value.
pub fn refusal<T: serde::de::DeserializeOwned>(key: &str) -> String {
    let mut wire = serde_json::json!({});
    wire[key] = serde_json::Value::Null;
    let Err(refused) = serde_json::from_value::<T>(wire) else {
        panic!("{key}: null was admitted")
    };
    refused.to_string()
}

/// Assert that a refusal message names its key and not the null value.
///
/// # Panics
///
/// Panics when `message` does not name `key`.
pub fn states_the_key(key: &str, message: &str) {
    assert!(
        message.starts_with(&format!("{key}: ")),
        "the refusal of a null {key} states {message}"
    );
    assert!(
        message.contains("it does not state null"),
        "the refusal of a null {key} states {message}"
    );
}

/// Admit preceding allocations, then refuse the named operation one unit below its need.
///
/// # Panics
///
/// Panics if the route has no named boundary, changes dimension, or fails for another reason.
pub fn resource_limit_at<T>(
    dimension: cadmpeg_core::decode::ResourceDimension,
    operation: &str,
    run: impl FnMut(u64) -> Result<T, cadmpeg_core::CodecError>,
) -> cadmpeg_core::CodecError {
    resource_limit_at_nth(dimension, operation, 0, run)
}

/// Refuse a named resource charge after `skip` reachable budget boundaries.
/// If the first run warms a setup cache, probe and replay once more to find
/// the stable boundary. Routes without cache changes need two runs.
///
/// # Panics
///
/// Panics if the route has no matching boundary or no stable normal policy replay.
pub fn resource_limit_at_nth<T>(
    dimension: cadmpeg_core::decode::ResourceDimension,
    operation: &str,
    skip: usize,
    mut run: impl FnMut(u64) -> Result<T, cadmpeg_core::CodecError>,
) -> cadmpeg_core::CodecError {
    use cadmpeg_core::CodecError;
    let mut mismatch = None;
    for _ in 0..2 {
        let probed =
            cadmpeg_core::decode::test_support::at_charge(dimension, operation, skip, || {
                run(u64::MAX)
            });
        let limit = match probed {
            Err(CodecError::ResourceLimit(limit)) => limit,
            Err(error) => panic!("unexpected refusal before {operation}: {error:?}"),
            Ok(_) => panic!("missing resource boundary: {operation}"),
        };
        assert_eq!(limit.dimension, dimension);
        assert_eq!(limit.operation, operation);
        let need = limit
            .used
            .checked_add(limit.additional)
            .expect("resource need fits");
        let error = cadmpeg_core::decode::test_support::without_probe(|| run(need - 1)).err();
        if matches!(&error, Some(CodecError::ResourceLimit(refusal))
            if refusal.dimension == dimension
                && refusal.operation == operation
                && refusal.used + refusal.additional == need)
        {
            return error.expect("matched normal policy refusal");
        }
        mismatch = Some((limit, error));
    }
    panic!("{operation}: no stable normal policy replay: {mismatch:?}")
}

#[cfg(test)]
mod tests {
    use super::resource_limit_at;
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

    #[test]
    fn normal_policy_replay_suspends_enclosing_probes() {
        let error = cadmpeg_core::decode::test_support::at_charge(
            ResourceDimension::WorkUnits,
            "before",
            0,
            || {
                resource_limit_at(ResourceDimension::WorkUnits, "target", |cap| {
                    let arena = DecodeArena::new();
                    let mut policy = DecodePolicy::desktop();
                    policy.limits.max_work_units = cap;
                    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)?;
                    ctx.charge_work(3, "before")?;
                    ctx.charge_work(7, "target")
                })
            },
        );
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.operation == "target" && limit.used == 3 && limit.additional == 7 && limit.limit == 9)
        );
    }

    #[test]
    fn resource_boundary_uses_one_probe_and_one_normal_policy_run() {
        let mut calls = 0;
        let error = resource_limit_at(ResourceDimension::WorkUnits, "target", |cap| {
            calls += 1;
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::desktop();
            policy.limits.max_work_units = cap;
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)?;
            ctx.charge_work(1, "before")?;
            ctx.charge_work(2, "before")?;
            ctx.charge_work(7, "target")
        });
        assert_eq!(calls, 2);
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::WorkUnits && limit.operation == "target"
                && limit.used == 3 && limit.additional == 7 && limit.limit == 9)
        );
    }

    #[test]
    fn cached_setup_replays_the_stable_normal_policy_boundary() {
        let mut cached = false;
        let mut calls = 0;
        let error = resource_limit_at(ResourceDimension::WorkUnits, "target", |cap| {
            calls += 1;
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::desktop();
            policy.limits.max_work_units = cap;
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)?;
            if !cached {
                ctx.charge_work(9, "prime cache")?;
                cached = true;
            }
            ctx.charge_work(3, "before")?;
            ctx.charge_work(20, "target")
        });
        assert_eq!(calls, 4);
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.operation == "target" && limit.used == 3 && limit.additional == 20 && limit.limit == 22)
        );
    }
}
