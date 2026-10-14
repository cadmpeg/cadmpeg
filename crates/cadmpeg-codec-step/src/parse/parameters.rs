// SPDX-License-Identifier: Apache-2.0
//! Inline storage for short entity parameter lists.

use super::Value;

pub(super) trait ParameterValues: Default {
    fn is_empty(&self) -> bool;
    fn push_value(&mut self, value: Value);
}

impl ParameterValues for Vec<Value> {
    fn is_empty(&self) -> bool {
        self.as_slice().is_empty()
    }

    fn push_value(&mut self, value: Value) {
        self.push(value);
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct RecordParameters(Storage);

#[derive(Debug, Clone, Default, PartialEq)]
enum Storage {
    #[default]
    Empty,
    One(Value),
    Pair([Value; 2]),
    Many(Vec<Value>),
}

impl RecordParameters {
    pub(crate) fn as_slice(&self) -> &[Value] {
        self
    }

    pub(super) fn prepend(&mut self, value: Value) {
        match &mut self.0 {
            Storage::Empty => self.0 = Storage::One(value),
            Storage::One(first) => {
                let first = std::mem::replace(first, Value::Omitted);
                self.0 = Storage::Pair([value, first]);
            }
            Storage::Pair(pair) => {
                let mut values = Vec::new();
                let pair = std::mem::replace(pair, [Value::Omitted, Value::Omitted]);
                values.push(value);
                values.extend(pair);
                self.0 = Storage::Many(values);
            }
            Storage::Many(values) => {
                values.insert(0, value);
            }
        }
    }
}

impl ParameterValues for RecordParameters {
    fn is_empty(&self) -> bool {
        self.as_slice().is_empty()
    }

    fn push_value(&mut self, value: Value) {
        match &mut self.0 {
            Storage::Empty => self.0 = Storage::One(value),
            Storage::One(first) => {
                let first = std::mem::replace(first, Value::Omitted);
                self.0 = Storage::Pair([first, value]);
            }
            Storage::Pair(pair) => {
                let mut values = Vec::new();
                let pair = std::mem::replace(pair, [Value::Omitted, Value::Omitted]);
                values.extend(pair);
                values.push(value);
                self.0 = Storage::Many(values);
            }
            Storage::Many(values) => values.push(value),
        }
    }
}

impl std::ops::Deref for RecordParameters {
    type Target = [Value];

    fn deref(&self) -> &Self::Target {
        match &self.0 {
            Storage::Empty => &[],
            Storage::One(value) => std::slice::from_ref(value),
            Storage::Pair(pair) => pair,
            Storage::Many(values) => values,
        }
    }
}

impl std::ops::DerefMut for RecordParameters {
    fn deref_mut(&mut self) -> &mut Self::Target {
        match &mut self.0 {
            Storage::Empty => &mut [],
            Storage::One(value) => std::slice::from_mut(value),
            Storage::Pair(pair) => pair,
            Storage::Many(values) => values,
        }
    }
}

impl<'a> IntoIterator for &'a RecordParameters {
    type Item = &'a Value;
    type IntoIter = std::slice::Iter<'a, Value>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

impl<'a> IntoIterator for &'a mut RecordParameters {
    type Item = &'a mut Value;
    type IntoIter = std::slice::IterMut<'a, Value>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter_mut()
    }
}

#[cfg(test)]
mod tests {
    use super::{ParameterValues, RecordParameters, Value};

    #[test]
    fn short_parameters_are_inline_and_longer_lists_grow() {
        let mut values = RecordParameters::default();
        for value in 1..=4 {
            values.push_value(Value::Integer(value));
        }
        assert_eq!(
            values.as_slice(),
            [
                Value::Integer(1),
                Value::Integer(2),
                Value::Integer(3),
                Value::Integer(4)
            ]
        );
    }

    #[test]
    fn prepend_places_the_value_first() {
        let mut values = RecordParameters::default();
        values.push_value(Value::Integer(2));
        values.prepend(Value::Integer(1));
        assert_eq!(values.as_slice(), [Value::Integer(1), Value::Integer(2)]);
        values.prepend(Value::Integer(0));
        assert_eq!(
            values.as_slice(),
            [Value::Integer(0), Value::Integer(1), Value::Integer(2)]
        );
    }
}
