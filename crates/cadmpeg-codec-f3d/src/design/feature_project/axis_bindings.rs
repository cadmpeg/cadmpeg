use crate::ids::{self, native_stream};
use crate::records::feature::scope::{DesignFeatureKind, DesignParameterScope};
use crate::records::parameters::{DesignParameter, DesignParameterOwner};
use std::collections::{HashMap, HashSet};

pub(super) enum AxisBinding<'a> {
    OutsideAxis,
    Unresolved,
    Bound(&'a DesignParameterOwner),
}

pub(super) struct AxisBindings<'a> {
    owner_keys: HashSet<(&'a str, u32)>,
    owners: HashMap<(&'a str, u32), Option<&'a DesignParameterOwner>>,
    parameters: HashMap<(&'a str, u32), usize>,
    scopes: HashMap<(&'a str, u32), usize>,
}
impl<'a> AxisBindings<'a> {
    pub(super) fn new(
        native: &'a [DesignParameter],
        owners: &'a [DesignParameterOwner],
        scopes: &'a [DesignParameterScope],
    ) -> Self {
        let axes = scopes
            .iter()
            .filter(|s| s.kind() == DesignFeatureKind::WorkAxis)
            .filter_map(|s| native_stream(&s.id).map(|stream| (stream, s.record_index)))
            .collect::<HashSet<_>>();
        let mut scope_counts = HashMap::new();
        for scope in scopes {
            if let Some(stream) = native_stream(&scope.id) {
                *scope_counts
                    .entry((stream, scope.record_index))
                    .or_insert(0usize) += 1;
            }
        }
        let owner_keys = owners
            .iter()
            .filter_map(|owner| {
                let stream = native_stream(owner.id())?;
                axes.contains(&(stream, owner.scope_record_index()))
                    .then_some((stream, owner.record_index()))
            })
            .collect::<HashSet<_>>();
        let mut unique = HashMap::new();
        for owner in owners {
            if let Some(stream) = native_stream(owner.id()) {
                let key = (stream, owner.record_index());
                if owner_keys.contains(&key) {
                    unique
                        .entry(key)
                        .and_modify(|entry| *entry = None)
                        .or_insert(Some(owner));
                }
            }
        }
        let mut parameters = HashMap::new();
        for parameter in native {
            let stream = native_stream(&parameter.id).unwrap_or(ids::DEFAULT_STREAM);
            *parameters
                .entry((stream, parameter.record_index))
                .or_insert(0usize) += 1;
        }
        Self {
            owner_keys,
            owners: unique,
            parameters,
            scopes: scope_counts,
        }
    }
    pub(super) fn owner(&self, stream: &str, index: u32, parameter: u32) -> AxisBinding<'a> {
        let key = (stream, index);
        if !self.owner_keys.contains(&key) {
            return AxisBinding::OutsideAxis;
        }
        match self.owners.get(&key).copied().flatten().filter(|owner| {
            owner.parameter_record_index() == parameter
                && self.parameters.get(&(stream, parameter)) == Some(&1)
                && self.scopes.get(&(stream, owner.scope_record_index())) == Some(&1)
        }) {
            Some(owner) => AxisBinding::Bound(owner),
            None => AxisBinding::Unresolved,
        }
    }
}
