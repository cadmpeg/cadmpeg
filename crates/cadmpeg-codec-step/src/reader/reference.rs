// SPDX-License-Identifier: Apache-2.0
//! Allocation-free traversal of STEP entity references.

use crate::parse::Value;

// Parsing and anchor materialization each admit at most 256 nested values.
// One additional frame holds the root value.
const MAX_VALUE_FRAMES: usize = 513;

#[derive(Clone, Copy)]
struct Frame<'a> {
    value: &'a Value,
    next_child: usize,
}

pub(super) struct References<'a> {
    frames: [Frame<'a>; MAX_VALUE_FRAMES],
    active: usize,
}

pub(super) fn references(value: &Value) -> References<'_> {
    References {
        frames: [Frame {
            value,
            next_child: 0,
        }; MAX_VALUE_FRAMES],
        active: 1,
    }
}

impl Iterator for References<'_> {
    type Item = u64;

    fn next(&mut self) -> Option<Self::Item> {
        while self.active > 0 {
            let frame = &mut self.frames[self.active - 1];
            match frame.value {
                Value::Reference(id) => {
                    self.active -= 1;
                    return Some(*id);
                }
                Value::List(values) => {
                    if let Some(value) = values.get(frame.next_child) {
                        frame.next_child += 1;
                        if self.active == MAX_VALUE_FRAMES {
                            self.active = 0;
                            return None;
                        }
                        self.frames[self.active] = Frame {
                            value,
                            next_child: 0,
                        };
                        self.active += 1;
                    } else {
                        self.active -= 1;
                    }
                }
                Value::Typed(_, value) if frame.next_child == 0 => {
                    frame.next_child = 1;
                    if self.active == MAX_VALUE_FRAMES {
                        self.active = 0;
                        return None;
                    }
                    self.frames[self.active] = Frame {
                        value,
                        next_child: 0,
                    };
                    self.active += 1;
                }
                _ => self.active -= 1,
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use crate::parse::Value;

    use super::references;

    #[test]
    fn nested_references_keep_source_order_without_a_collection() {
        let value = Value::List(vec![
            Value::Reference(1),
            Value::Typed(
                "WRAPPER".into(),
                Box::new(Value::List(vec![Value::Reference(2), Value::Integer(3)])),
            ),
            Value::Reference(4),
        ]);
        assert_eq!(references(&value).collect::<Vec<_>>(), [1, 2, 4]);
    }

    #[test]
    fn admitted_nested_references_reach_the_leaf() {
        let mut value = Value::Reference(9);
        for _ in 0..256 {
            value = Value::Typed("WRAPPER".into(), Box::new(value));
        }
        assert_eq!(references(&value).collect::<Vec<_>>(), [9]);
    }
}

pub(super) fn visit(value: &Value, visitor: &mut impl FnMut(u64) -> bool) -> bool {
    match value {
        Value::Reference(id) => visitor(*id),
        Value::List(values) => values.iter().any(|value| visit(value, visitor)),
        Value::Typed(_, value) => visit(value, visitor),
        _ => false,
    }
}

pub(super) fn first_matching<'a>(
    values: impl IntoIterator<Item = &'a Value>,
    mut predicate: impl FnMut(u64) -> bool,
) -> Option<u64> {
    let mut matched = None;
    for value in values {
        if visit(value, &mut |id| {
            if predicate(id) {
                matched = Some(id);
                true
            } else {
                false
            }
        }) {
            break;
        }
    }
    matched
}
