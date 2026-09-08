// SPDX-License-Identifier: Apache-2.0
//! Exact parameter spelling with its parsed index and qualifier boundary.

/// A name parsed once at construction. `u32` requires canonical parameter syntax;
/// `Option<u32>` also admits ordinary expression names.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ParameterName<S, I = Option<u32>> {
    spelling: S,
    index: I,
    qualifier_start: Option<usize>,
}

impl<S: AsRef<str>> ParameterName<S> {
    pub fn new(spelling: S) -> Self {
        let (index, qualifier_start) = canonical_parts(spelling.as_ref())
            .map_or((None, None), |(index, qualifier)| (Some(index), qualifier));
        Self {
            spelling,
            index,
            qualifier_start,
        }
    }
}

impl<S: AsRef<str>> ParameterName<S, u32> {
    pub fn parse(spelling: S) -> Option<Self> {
        let (index, qualifier_start) = canonical_parts(spelling.as_ref())?;
        Some(Self {
            spelling,
            index,
            qualifier_start,
        })
    }
}

impl<S: AsRef<str>, I: Copy> ParameterName<S, I> {
    pub fn as_str(&self) -> &str {
        self.spelling.as_ref()
    }

    pub fn index(&self) -> I {
        self.index
    }

    pub fn qualifier(&self) -> Option<&str> {
        self.qualifier_start
            .map(|start| &self.spelling.as_ref()[start..])
    }

    pub fn into_spelling(self) -> S {
        self.spelling
    }
}

impl<I> ParameterName<&str, I> {
    pub fn into_owned(self) -> ParameterName<String, I> {
        ParameterName {
            spelling: self.spelling.to_string(),
            index: self.index,
            qualifier_start: self.qualifier_start,
        }
    }
}

fn canonical_parts(name: &str) -> Option<(u32, Option<usize>)> {
    let tail = name.strip_prefix('p')?;
    let digit_count = tail.bytes().take_while(u8::is_ascii_digit).count();
    if digit_count == 0 {
        return None;
    }
    let index = tail[..digit_count].parse().ok()?;
    match &tail[digit_count..] {
        "" => Some((index, None)),
        suffix => {
            let qualifier = suffix.strip_prefix('_').filter(|qualifier| {
                !qualifier.is_empty()
                    && qualifier
                        .bytes()
                        .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
            })?;
            Some((index, Some(name.len() - qualifier.len())))
        }
    }
}
