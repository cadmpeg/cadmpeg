// SPDX-License-Identifier: Apache-2.0
//! Native string classification through registered record readers.
use cadmpeg_core::CodecError;
use serde::de::DeserializeOwned;
use serde_json::Value;
use std::{
    cell::RefCell,
    collections::{BTreeMap, BTreeSet},
};

/// Enum-valued JSON pointers, indexed by native record identity.
pub type EnumFields = BTreeMap<String, BTreeSet<String>>;

struct Capture {
    fields: EnumFields,
    probes: usize,
    limit: usize,
    exceeded: bool,
    work_bytes: usize,
}

thread_local! {
    static CAPTURE: RefCell<Option<Capture>> = const { RefCell::new(None) };
}

struct Restore(Option<Capture>);
impl Drop for Restore {
    fn drop(&mut self) {
        CAPTURE.with(|capture| {
            capture.replace(self.0.take());
        });
    }
}

/// Collect finite enum fields while codec-owned readers admit native records.
pub fn collect<T>(
    limit: usize,
    operation: impl FnOnce() -> Result<T, CodecError>,
) -> Result<EnumFields, CodecError> {
    let preceding = CAPTURE.with(|capture| {
        capture.replace(Some(Capture {
            fields: EnumFields::new(),
            probes: 0,
            limit,
            exceeded: false,
            work_bytes: 0,
        }))
    });
    let _restore = Restore(preceding);
    operation()?;
    let capture = CAPTURE.with(|capture| capture.take());
    let Some(capture) = capture else {
        return Err(CodecError::InvalidInput(
            "native privacy capture is absent".into(),
        ));
    };
    if capture.exceeded {
        return Err(CodecError::InvalidInput(
            "native privacy probe limit exceeded".into(),
        ));
    }
    Ok(capture.fields)
}

pub(super) fn observe<T: DeserializeOwned>(id: &str, json: &str) {
    if !CAPTURE.with(|capture| capture.borrow().is_some()) {
        return;
    }
    let Ok(value) = serde_json::from_str::<Value>(json) else {
        return;
    };
    let mut pending = vec![(&value, String::new())];
    while let Some((item, path)) = pending.pop() {
        match item {
            Value::Object(fields) => {
                for (field, value) in fields {
                    let field = field.replace('~', "~0").replace('/', "~1");
                    pending.push((value, format!("{path}/{field}")));
                }
            }
            Value::Array(values) => {
                for (index, value) in values.iter().enumerate() {
                    pending.push((value, format!("{path}/{index}")));
                }
            }
            Value::String(_) if path != "/id" => {
                if !take_probe(json.len()) {
                    return;
                }
                let mut probe = value.clone();
                let Some(target) = probe.pointer_mut(&path) else {
                    continue;
                };
                *target = Value::String("__cadmpeg_repro_probe__".into());
                if serde_json::from_value::<T>(probe)
                    .err()
                    .is_some_and(|error| {
                        error
                            .to_string()
                            .contains("unknown variant `__cadmpeg_repro_probe__`")
                            || error.to_string().contains("invalid base64 byte payload")
                            || error
                                .to_string()
                                .contains("native bytes must be an even-length hexadecimal string")
                    })
                {
                    CAPTURE.with(|capture| {
                        if let Some(capture) = capture.borrow_mut().as_mut() {
                            capture.fields.entry(id.into()).or_default().insert(path);
                        }
                    });
                }
            }
            _ => {}
        }
    }
}

fn take_probe(bytes: usize) -> bool {
    CAPTURE.with(|capture| {
        let mut capture = capture.borrow_mut();
        let Some(capture) = capture.as_mut() else {
            return false;
        };
        if capture.probes == capture.limit {
            capture.exceeded = true;
            return false;
        }
        let Some(work_bytes) = capture.work_bytes.checked_add(bytes) else {
            capture.exceeded = true;
            return false;
        };
        if work_bytes > 512 * 1024 * 1024 {
            capture.exceeded = true;
            return false;
        }
        capture.work_bytes = work_bytes;
        capture.probes += 1;
        true
    })
}

#[cfg(test)]
mod tests;
