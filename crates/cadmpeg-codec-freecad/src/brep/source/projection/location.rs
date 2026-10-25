// SPDX-License-Identifier: Apache-2.0
//! Original location factors and shape-use paths determine placed geometry.
use super::*;

enum LocationValue {
    Matrix(Transform),
    Factors(Vec<(usize, i64)>),
}
impl PayloadSource<'_> {
    fn location_value(
        &self,
        payload: &ShapePayloadRecord,
        dense: usize,
    ) -> Result<LocationValue, CodecError> {
        let index = payload.source_index(0, dense);
        let view = self.row(0, index)?;
        let (tokens, ranges) = text_tokens(view);
        let mut text = cursor(&tokens, &ranges, view);
        let mut binary = BinaryCursor {
            view,
            census_marks: vec![],
            representation_marks: vec![],
        };
        let kind = if self.binary {
            i64::from(binary.u8("location kind")?)
        } else {
            text.integer("location kind")?
        };
        match kind {
            1 => {
                let mut rows = [[FiniteReal::ZERO; 4]; 3];
                for row in &mut rows {
                    for value in row {
                        *value = if self.binary {
                            binary.finite_f64("location matrix")?
                        } else {
                            text.finite_real("location matrix")?
                        };
                    }
                }
                let matrix = Transform::from_finite_rows(rows);
                invert_affine(matrix)?;
                Ok(LocationValue::Matrix(matrix))
            }
            2 => {
                let mut factors = Vec::new();
                loop {
                    let reference = if self.binary {
                        i64::from(binary.i32("location factor")?)
                    } else {
                        text.integer("location factor")?
                    };
                    if reference == 0 {
                        break;
                    }
                    let reference = usize::try_from(reference)
                        .map_err(|_| CodecError::malformed("location factor is negative"))?;
                    if reference >= index {
                        return Err(CodecError::malformed(
                            "location factor is not prior to its owner",
                        ));
                    }
                    let facts = payload
                        .payload
                        .shape_set()
                        .ok_or_else(|| CodecError::malformed("location source has no tables"))?;
                    let target = (1..=facts.locations.len())
                        .find(|dense| payload.source_index(0, *dense) == reference)
                        .ok_or_else(|| {
                            CodecError::malformed("location factor has no accepted source row")
                        })?;
                    let power = if self.binary {
                        i64::from(binary.i32("location power")?)
                    } else {
                        text.integer("location power")?
                    };
                    if factors.len() >= 1_000_000 {
                        return Err(CodecError::malformed("location factor count exceeds limit"));
                    }
                    factors.push((target, power));
                }
                Ok(LocationValue::Factors(factors))
            }
            _ => Err(CodecError::malformed("location has an invalid selector")),
        }
    }
    pub(super) fn location(
        &self,
        payload: &ShapePayloadRecord,
        dense: usize,
    ) -> Result<Transform, CodecError> {
        if dense == 0 {
            return Ok(Transform::identity());
        }
        let mut pending = vec![(dense, false)];
        let mut values = BTreeMap::new();
        let mut solved = BTreeMap::new();
        while let Some((index, expanded)) = pending.pop() {
            if solved.contains_key(&index) {
                continue;
            }
            if !expanded {
                let value = self.location_value(payload, index)?;
                match value {
                    LocationValue::Matrix(matrix) => {
                        solved.insert(index, matrix);
                    }
                    LocationValue::Factors(factors) => {
                        pending.push((index, true));
                        pending.extend(factors.iter().map(|(factor, _)| (*factor, false)));
                        values.insert(index, factors);
                    }
                }
            } else {
                let factors = values
                    .remove(&index)
                    .ok_or_else(|| CodecError::malformed("location factor state is absent"))?;
                let mut matrix = Transform::identity();
                for (factor, power) in factors {
                    let value = solved
                        .get(&factor)
                        .copied()
                        .ok_or_else(|| CodecError::malformed("location factor was not resolved"))?;
                    matrix = transform_power(value, power)?
                        .compose(matrix)
                        .map_err(location_transform_error)?;
                }
                solved.insert(index, matrix);
            }
        }
        solved
            .remove(&dense)
            .ok_or_else(|| CodecError::malformed("location was not resolved"))
    }
    pub(super) fn root_values(
        &self,
        payload: &ShapePayloadRecord,
    ) -> Result<Vec<(TextShapeUse, Transform)>, CodecError> {
        let facts = payload
            .payload
            .shape_set()
            .ok_or_else(|| CodecError::malformed("root source has no typed tables"))?;
        let mut values = Vec::new();
        for mut root in self.roots()? {
            let Some(shape) = (1..=facts.tshapes.len())
                .find(|dense| payload.source_index(7, *dense) == root.shape)
            else {
                continue;
            };
            root.shape = shape;
            let location = root.location.index();
            if location != 0 {
                let Some(dense) = (1..=facts.locations.len())
                    .find(|dense| payload.source_index(0, *dense) == location)
                else {
                    continue;
                };
                root.location = dense.into();
            }
            let matrix = self.location(payload, root.location.index())?;
            values.push((root, matrix));
        }
        Ok(values)
    }
    pub(super) fn placement(
        &self,
        payload: &ShapePayloadRecord,
        target: usize,
        expected: Transform,
        ordinal: Option<usize>,
    ) -> Result<Transform, CodecError> {
        self.placement_local(payload, target, expected, ordinal, 0)
    }
    pub(super) fn placement_local(
        &self,
        payload: &ShapePayloadRecord,
        target: usize,
        expected: Transform,
        ordinal: Option<usize>,
        location: usize,
    ) -> Result<Transform, CodecError> {
        let local = self.location(payload, location)?;
        let roots = self.root_values(payload)?;
        let index = ordinal
            .unwrap_or(1)
            .checked_sub(1)
            .ok_or_else(|| CodecError::malformed("root ordinal is zero"))?;
        let (root, _) = roots
            .get(index)
            .ok_or_else(|| CodecError::malformed("placed source has no admitted root"))?;
        let mut pending = vec![(root.shape, Transform::identity())];
        let mut seen = std::collections::BTreeSet::new();
        while let Some((shape, matrix)) = pending.pop() {
            let key = matrix
                .rows()
                .into_iter()
                .flatten()
                .map(|value| if value == 0.0 { 0 } else { value.to_bits() })
                .collect::<Vec<_>>();
            if !seen.insert((shape, key)) {
                continue;
            }
            if shape == target {
                let placed = matrix.compose(local).map_err(location_transform_error)?;
                if placed == expected {
                    return Ok(placed);
                }
            }
            let value = self.shape(payload, shape)?;
            for child in value.children {
                pending.push((
                    child.shape,
                    matrix
                        .compose(self.location(payload, child.location.index())?)
                        .map_err(location_transform_error)?,
                ));
            }
        }
        Err(CodecError::malformed(
            "original shape-use path does not admit this placement",
        ))
    }
}
