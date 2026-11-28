// SPDX-License-Identifier: Apache-2.0
use std::cell::Cell;
use std::collections::BTreeMap;

use cadmpeg_ir::report::loss::LossNote;
use serde_json::Value;

use super::{features, rows};
use crate::loss::OnshapeLossCode;

#[derive(Clone, Copy, Debug)]
enum Disposition {
    Unread,
    Accounted,
    Partial(&'static str),
}

#[derive(Debug)]
pub(super) struct ParameterUsage<'a> {
    record: &'a Value,
    dispositions: BTreeMap<&'a str, Cell<Disposition>>,
}

impl<'a> ParameterUsage<'a> {
    pub(super) fn new(record: &'a Value) -> Self {
        let dispositions = Self::keys(record)
            .map(|key| (key, Cell::new(Disposition::Unread)))
            .collect();
        Self {
            record,
            dispositions,
        }
    }

    fn keys(record: &Value) -> impl Iterator<Item = &str> {
        rows(record, "parameters")
            .unwrap_or_default()
            .iter()
            .filter_map(|row| row["parameterId"].as_str())
    }

    pub(super) fn account(&self, keys: &[&str]) {
        for key in keys {
            if let Some(state) = self
                .dispositions
                .get(key)
                .filter(|state| matches!(state.get(), Disposition::Unread))
            {
                state.set(Disposition::Accounted);
            }
        }
    }

    pub(super) fn partial(&self, key: &str, reason: &'static str) {
        if let Some(state) = self.dispositions.get(key) {
            state.set(Disposition::Partial(reason));
        }
    }

    pub(super) fn ignore_false(&self, keys: &[&str]) {
        self.ignore_value(keys, &Value::Bool(false));
    }

    pub(super) fn ignore_value(&self, keys: &[&str], value: &Value) {
        for &key in keys {
            if features::parameter(self.record, key)
                .is_ok_and(|row| row.get("value") == Some(value))
            {
                self.account(&[key]);
            }
        }
    }

    pub(super) fn ignore_empty_queries(&self, keys: &[&str]) {
        for &key in keys {
            if features::parameter(self.record, key).is_ok_and(|row| {
                row.get("queries")
                    .is_none_or(|queries| queries.as_array().is_some_and(Vec::is_empty))
            }) {
                self.account(&[key]);
            }
        }
    }

    pub(super) fn report(&self, subject: &str, notes: &mut Vec<LossNote>) {
        for key in Self::keys(self.record) {
            let reason = match self.dispositions.get(key).map(Cell::get) {
                Some(Disposition::Accounted) => continue,
                Some(Disposition::Partial(reason)) => reason,
                Some(Disposition::Unread) | None => "has no projected design field",
            };
            notes.push(
                OnshapeLossCode::DesignParameterUnprojected
                    .note(format!("parameter {key} {reason}; subject: {subject}")),
            );
        }
    }
}
