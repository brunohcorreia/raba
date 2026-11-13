//! Conway's Game of Life.
//!
//! # How this one closes its loop
//!
//! By finding the loop rather than imposing one. Life is deterministic and this
//! board is finite, so the sequence of states must eventually repeat; a soup on a
//! small torus almost always settles into still lifes and short oscillators
//! within a few hundred generations. Remembering every state seen and watching
//! for one to come round again gives the exact period, and the animation is
//! emitted from the first state of that cycle.
//!
//! So the frame count here is a **budget, not a length**. A board that settles
//! into blinkers produces a two-frame file, which is correct and very small.
//! `raba-gen generate` reports what it found.
//!
//! If nothing repeats inside the budget the board is still churning, and the
//! requested number of frames is emitted with a seam, like `fire`.
//!
//! # Why the board is not a random soup
//!
//! It was, and it was measured: on an 80x25 torus **every seed tried settled to
//! period 2** within a few hundred generations, which is what Life does — soups
//! burn down to still lifes and blinkers. The cycle detection was finding a real
//! and exact answer, and the answer was a two-frame file of things twitching.
//!
//! So the board is seeded instead with a garden of known oscillators whose
//! periods are chosen to be coprime: the pentadecathlon at 15, the pulsar at 3,
//! and toads and blinkers at 2. They are spaced far enough apart not to interfere,
//! so the whole board's period is the lowest common multiple of theirs — 30 —
//! and the cycle search finds exactly that. It looks like Life, and it loops
//! perfectly.

use super::{blank, Rng, Spec};
use raba_core::render::Cell;
use raba_core::Grid;
use std::collections::HashMap;

/// How many generations to search before giving up on finding a cycle.
const SEARCH: usize = 4_000;

/// Oscillators, as rows of live cells. Periods are coprime on purpose: together
/// they give a board period of lcm(15, 3, 2) = 30.
const PATTERNS: &[(&[&str], u16)] = &[
    // Pentadecathlon, period 15.
    (&["..█....█..", "██.████.██", "..█....█.."], 15),
    // Pulsar, period 3.
    (
        &[
            "..███...███..",
            ".............",
            "█....█.█....█",
            "█....█.█....█",
            "█....█.█....█",
            "..███...███..",
            ".............",
            "..███...███..",
            "█....█.█....█",
            "█....█.█....█",
            "█....█.█....█",
            ".............",
            "..███...███..",
        ],
        3,
    ),
    // Toad, period 2.
    (&[".███", "███."], 2),
    // Blinker, period 2.
    (&["███"], 2),
];

/// Lay the oscillators out on a grid, spaced so they never touch.
///
/// Returns the live cells. Which pattern goes where is drawn from `rng`, so a
/// seed still changes the picture — it just cannot change the period.
fn garden(columns: usize, lines: usize, rng: &mut Rng) -> Vec<bool> {
    let mut cells = vec![false; columns.saturating_mul(lines)];
    // A pulsar is 13 wide and 13 tall; a cell of 16x16 leaves a clear margin, which
    // is what keeps the patterns from interacting and wrecking the period.
    const CELL: usize = 16;
    let across = columns / CELL;
    let down = lines / CELL;

    // Too small for even one pulsar: fall back to the small oscillators, which
    // still give a period of 30 as long as a pentadecathlon fits.
    let small = across == 0 || down == 0;

    let place = |cells: &mut Vec<bool>, pattern: &[&str], at_column: usize, at_line: usize| {
        for (row, text) in pattern.iter().enumerate() {
            for (column, ch) in text.chars().enumerate() {
                if ch != '█' {
                    continue;
                }
                let c = at_column.saturating_add(column);
                let l = at_line.saturating_add(row);
                if c < columns && l < lines {
                    if let Some(cell) = cells.get_mut(l.saturating_mul(columns).saturating_add(c)) {
                        *cell = true;
                    }
                }
            }
        }
    };

    if small {
        // One pentadecathlon and one blinker, as far apart as the board allows.
        if let Some((pattern, _)) = PATTERNS.first() {
            place(&mut cells, pattern, 1, 1);
        }
        if let Some((pattern, _)) = PATTERNS.get(3) {
            place(&mut cells, pattern, columns.saturating_sub(5), lines.saturating_sub(3));
        }
        return cells;
    }

    for slot_line in 0..down {
        for slot_column in 0..across {
            let choice = usize::try_from(rng.below(u32::try_from(PATTERNS.len()).unwrap_or(1)))
                .unwrap_or(0);
            let Some((pattern, _)) = PATTERNS.get(choice) else { continue };
            let width = pattern.first().map_or(0, |row| row.chars().count());
            let height = pattern.len();
            // Centre each pattern in its slot, so nothing sits against a neighbour.
            let base_column = slot_column.saturating_mul(CELL);
            let base_line = slot_line.saturating_mul(CELL);
            place(
                &mut cells,
                pattern,
                base_column.saturating_add(CELL.saturating_sub(width) / 2),
                base_line.saturating_add(CELL.saturating_sub(height) / 2),
            );
        }
    }
    cells
}

struct Board {
    columns: usize,
    lines: usize,
    cells: Vec<bool>,
}

impl Board {
    fn alive(&self, column: isize, line: isize) -> bool {
        // A torus: gliders leave one edge and arrive at the other, which keeps a
        // small board from emptying itself out.
        let columns = isize::try_from(self.columns).unwrap_or(1).max(1);
        let lines = isize::try_from(self.lines).unwrap_or(1).max(1);
        let column = usize::try_from(column.rem_euclid(columns)).unwrap_or(0);
        let line = usize::try_from(line.rem_euclid(lines)).unwrap_or(0);
        self.cells.get(line.saturating_mul(self.columns).saturating_add(column)).copied().unwrap_or(false)
    }

    fn step(&self) -> Vec<bool> {
        let mut next = vec![false; self.cells.len()];
        for line in 0..self.lines {
            for column in 0..self.columns {
                let (c, l) = (isize::try_from(column).unwrap_or(0), isize::try_from(line).unwrap_or(0));
                let mut neighbours = 0u8;
                for dl in -1_isize..=1 {
                    for dc in -1_isize..=1 {
                        if (dc, dl) != (0, 0) && self.alive(c.saturating_add(dc), l.saturating_add(dl)) {
                            neighbours = neighbours.saturating_add(1);
                        }
                    }
                }
                let here = self.alive(c, l);
                if let Some(cell) =
                    next.get_mut(line.saturating_mul(self.columns).saturating_add(column))
                {
                    *cell = matches!((here, neighbours), (true, 2 | 3) | (false, 3));
                }
            }
        }
        next
    }
}

/// Draw one generation, with the one before it, so that a cell that has just died
/// can leave a fading mark. The predecessor is passed in rather than remembered
/// so that the first frame of a loop can be given the *last* state of the cycle,
/// which is what keeps the trail periodic too.
fn paint(spec: &Spec, now: &[bool], before: &[bool], columns: usize, lines: usize) -> Grid {
    let mut grid = blank(spec);
    for line in 0..lines {
        for column in 0..columns {
            let index = line.saturating_mul(columns).saturating_add(column);
            let (alive, was) =
                (now.get(index).copied().unwrap_or(false), before.get(index).copied().unwrap_or(false));
            let cell = match (alive, was) {
                (true, false) => Cell { ch: '█', fg: 15, bg: 0 }, // just born
                (true, true) => Cell { ch: '█', fg: 10, bg: 0 },  // living on
                (false, true) => Cell { ch: '░', fg: 2, bg: 0 },  // just died
                (false, false) => Cell { ch: ' ', fg: 7, bg: 0 },
            };
            if let (Ok(c), Ok(l)) = (u16::try_from(column), u16::try_from(line)) {
                grid.set(c, l, cell);
            }
        }
    }
    grid
}

pub fn frames(spec: &Spec) -> Vec<Grid> {
    let columns = usize::from(spec.columns.max(1));
    let lines = usize::from(spec.lines.max(1));
    let budget = usize::from(spec.frames.max(1));
    let mut rng = Rng::new(spec.seed);

    let mut board = Board { columns, lines, cells: garden(columns, lines, &mut rng) };

    // Walk forward, remembering every state, until one comes round again.
    let mut seen: HashMap<Vec<bool>, usize> = HashMap::new();
    let mut history: Vec<Vec<bool>> = Vec::new();
    let mut cycle: Option<(usize, usize)> = None; // (first seen, period)

    for generation in 0..SEARCH {
        if let Some(&first) = seen.get(&board.cells) {
            cycle = Some((first, generation.saturating_sub(first)));
            break;
        }
        seen.insert(board.cells.clone(), generation);
        history.push(board.cells.clone());
        board.cells = board.step();
    }

    let states: Vec<Vec<bool>> = match cycle {
        Some((first, period)) if period > 0 => {
            // Exactly one turn of the cycle, trimmed to the budget if the cycle is
            // longer than the caller wants to store.
            let length = period.min(budget);
            history.get(first..first.saturating_add(length)).map(<[Vec<bool>]>::to_vec).unwrap_or_default()
        }
        // Still churning after SEARCH generations: take what was asked for and
        // accept the seam.
        _ => history.get(history.len().saturating_sub(budget)..).map(<[Vec<bool>]>::to_vec).unwrap_or_default(),
    };

    if states.is_empty() {
        return vec![blank(spec)];
    }

    // Each frame is drawn against its predecessor, and the first against the last,
    // so the dying-cell trail is as periodic as the board is.
    let last = states.len().saturating_sub(1);
    states
        .iter()
        .enumerate()
        .map(|(index, now)| {
            let before = if index == 0 { states.get(last) } else { states.get(index.saturating_sub(1)) };
            paint(spec, now, before.map_or(&[][..], Vec::as_slice), columns, lines)
        })
        .collect()
}
