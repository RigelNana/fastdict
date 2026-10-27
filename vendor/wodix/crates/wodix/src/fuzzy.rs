//! Unicode-scalar Levenshtein rows streamed over the FST's UTF-8 byte edges.
//! Rows are shared while a multibyte scalar is incomplete; no byte-distance
//! approximation or unbounded determinization is used.

use std::cell::{Cell, RefCell};
use std::sync::Arc;

use fst::Automaton;

use crate::{Error, Limits, Result};

pub(crate) struct Fuzzy {
    query: Vec<char>,
    initial: Arc<Vec<u32>>,
    distance: u32,
    used: Cell<usize>,
    maximum: usize,
    failure: RefCell<Option<Error>>,
}

#[derive(Clone)]
pub(crate) struct State {
    row: Arc<Vec<u32>>,
    bytes: [u8; 4],
    length: usize,
    width: usize,
    valid: bool,
}

impl Fuzzy {
    pub fn new(query: &str, distance: u32, limits: Limits) -> Result<Self> {
        let count = query.chars().count();
        let cells = count
            .checked_add(1)
            .ok_or_else(|| Error::Corrupt("fuzzy query length overflow".into()))?;
        Limits::check("fuzzy cells", cells as u64, limits.fuzzy_cells)?;
        let allocation = cells
            .checked_mul(12)
            .ok_or_else(|| Error::Corrupt("fuzzy state allocation overflow".into()))?;
        Limits::check("fuzzy metadata", allocation as u64, limits.metadata)?;
        let mut characters = Vec::new();
        characters
            .try_reserve_exact(count)
            .map_err(|error| Error::Corrupt(format!("fuzzy query allocation: {error}")))?;
        characters.extend(query.chars());
        let mut row = Vec::new();
        row.try_reserve_exact(cells)
            .map_err(|error| Error::Corrupt(format!("fuzzy row allocation: {error}")))?;
        for column in 0..cells {
            row.push(
                u32::try_from(column)
                    .unwrap_or(u32::MAX)
                    .min(distance.saturating_add(1)),
            );
        }
        Ok(Self {
            query: characters,
            initial: Arc::new(row),
            distance,
            used: Cell::new(cells),
            maximum: limits.fuzzy_cells,
            failure: RefCell::new(None),
        })
    }

    pub fn finish(&self) -> Result<()> {
        match self.failure.borrow_mut().take() {
            Some(error) => Err(error),
            None => Ok(()),
        }
    }

    fn scalar(&self, previous: &State, character: char) -> State {
        let used = self.used.get().saturating_add(previous.row.len());
        if used > self.maximum {
            *self.failure.borrow_mut() = Some(Error::Limit {
                kind: "fuzzy cells",
                actual: used as u64,
                limit: self.maximum as u64,
            });
            return State {
                valid: false,
                ..previous.clone()
            };
        }
        self.used.set(used);
        let mut row = Vec::new();
        if let Err(error) = row.try_reserve_exact(previous.row.len()) {
            *self.failure.borrow_mut() =
                Some(Error::Corrupt(format!("fuzzy row allocation: {error}")));
            return State {
                valid: false,
                ..previous.clone()
            };
        }
        let ceiling = self.distance.saturating_add(1);
        row.push(previous.row[0].saturating_add(1).min(ceiling));
        for (column, &expected) in self.query.iter().enumerate() {
            let insert = row[column].saturating_add(1);
            let delete = previous.row[column + 1].saturating_add(1);
            let substitute = previous.row[column].saturating_add(u32::from(expected != character));
            row.push(insert.min(delete).min(substitute).min(ceiling));
        }
        State {
            row: Arc::new(row),
            bytes: [0; 4],
            length: 0,
            width: 0,
            valid: true,
        }
    }
}

impl Automaton for Fuzzy {
    type State = State;

    fn start(&self) -> State {
        State {
            row: Arc::clone(&self.initial),
            bytes: [0; 4],
            length: 0,
            width: 0,
            valid: true,
        }
    }

    fn is_match(&self, state: &State) -> bool {
        state.valid
            && state.length == 0
            && self.failure.borrow().is_none()
            && state.row[self.query.len()] <= self.distance
    }

    fn can_match(&self, state: &State) -> bool {
        state.valid
            && self.failure.borrow().is_none()
            && state.row.iter().any(|&cost| cost <= self.distance)
    }

    fn accept(&self, state: &State, byte: u8) -> State {
        if !state.valid || self.failure.borrow().is_some() {
            return State {
                valid: false,
                ..state.clone()
            };
        }
        if state.length == 0 && byte < 0x80 {
            return self.scalar(state, char::from(byte));
        }
        let mut next = state.clone();
        if next.length == 0 {
            next.width = match byte {
                0xc2..=0xdf => 2,
                0xe0..=0xef => 3,
                0xf0..=0xf4 => 4,
                _ => {
                    return State {
                        valid: false,
                        ..next
                    };
                }
            };
        } else if !(0x80..=0xbf).contains(&byte) {
            return State {
                valid: false,
                ..next
            };
        }
        next.bytes[next.length] = byte;
        next.length += 1;
        if next.length < next.width {
            return next;
        }
        match std::str::from_utf8(&next.bytes[..next.width])
            .ok()
            .and_then(|text| text.chars().next())
        {
            Some(character) => self.scalar(state, character),
            None => State {
                valid: false,
                ..next
            },
        }
    }
}
