//! Morph transition detection between two slides.
//!
//! PowerPoint's "Morph" transition animates every shape that exists on both
//! the outgoing and incoming slide from its old position/size to its new
//! one. Shapes that only appear on the incoming slide fade in; shapes that
//! only exist on the outgoing slide fade out.
//!
//! Detection pairs shapes across the two slides with a three-tier strategy:
//! 1. **Unique name** — a shape name occurring exactly once on each slide
//!    (case-insensitive, whitespace-trimmed) is a strong identity signal.
//! 2. **Shape id** — PowerPoint reuses the `cNvPr` id across a morph when the
//!    author drags/resizes a shape, so an id match is authoritative even when
//!    the name changed.
//! 3. **Nearest centroid, same kind** — remaining shapes of the same kind are
//!    paired by smallest centre-to-centre distance, a heuristic fallback for
//!    renamed or anonymous shapes.
//!
//! The input is the top-level shape tree from
//! [`crate::inspect::get_slide_shapes`]; groups are matched as a single unit
//! (their children are not recursed into, mirroring how a group morphs as one
//! object). Shapes without resolvable bounds cannot be interpolated, so they
//! are never paired and surface in `removed`/`added` instead.

use serde::Serialize;

use crate::inspect::{BoundingBox, ShapeInfo};

/// A single position/size box in slide EMU coordinates.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct MorphBox {
    pub x: i64,
    pub y: i64,
    pub w: i64,
    pub h: i64,
}

/// One paired shape that morphs from a `from` box to a `to` box.
#[derive(Debug, Clone, Serialize)]
pub struct MorphMatch {
    /// `cNvPr` id of the paired shape (the outgoing shape's id).
    pub id: u32,
    /// Display name for the timeline track (incoming name when present).
    pub name: String,
    /// Shape kind, e.g. `"autoshape"`, `"picture"`.
    pub kind: &'static str,
    /// Position/size on the outgoing slide.
    pub from: MorphBox,
    /// Position/size on the incoming slide.
    pub to: MorphBox,
}

/// Full morph result: paired shapes plus the shapes present on only one side
/// (these fade in/out rather than morph).
#[derive(Debug, Clone, Default, Serialize)]
pub struct MorphResult {
    /// Shapes present on both slides, paired. Ordered by outgoing position.
    pub matches: Vec<MorphMatch>,
    /// Ids of outgoing shapes with no pair (fade out).
    pub removed: Vec<u32>,
    /// Ids of incoming shapes with no pair (fade in).
    pub added: Vec<u32>,
}

/// Detect which shapes morph between `before` and `after`.
///
/// Both lists should be the top-level shapes of their respective slides. The
/// result is deterministic: matches are ordered by the outgoing shape's
/// position in `before`.
pub fn detect_morph(before: &[ShapeInfo], after: &[ShapeInfo]) -> MorphResult {
    let mut pairs: Vec<(usize, usize)> = Vec::new();
    let mut matched_before = vec![false; before.len()];
    let mut matched_after = vec![false; after.len()];

    pair_by_name(before, after, &mut pairs, &mut matched_before, &mut matched_after);
    pair_by_id(before, after, &mut pairs, &mut matched_before, &mut matched_after);
    pair_by_centroid(before, after, &mut pairs, &mut matched_before, &mut matched_after);

    let mut matches = Vec::with_capacity(pairs.len());
    let mut removed = Vec::new();
    let mut added = Vec::with_capacity(after.len());

    for (i, a) in before.iter().enumerate() {
        match pairs.iter().find(|&&(pi, _)| pi == i) {
            Some(&(_, j)) => matches.push(MorphMatch {
                id: a.id,
                name: after[j].name.clone(),
                kind: a.kind,
                from: box_of(&a.bounds.as_ref().unwrap()),
                to: box_of(&after[j].bounds.as_ref().unwrap()),
            }),
            None => removed.push(a.id),
        }
    }
    for (j, b) in after.iter().enumerate() {
        if !matched_after[j] {
            added.push(b.id);
        }
    }

    MorphResult {
        matches,
        removed,
        added,
    }
}

/// Pairable means the shape carries resolvable geometry on that slide.
fn pairable(s: &ShapeInfo) -> bool {
    s.bounds.is_some()
}

/// Tier 1: pair shapes whose trimmed, lower-cased name occurs exactly once on
/// each slide. Empty or duplicated names are skipped.
fn pair_by_name(
    before: &[ShapeInfo],
    after: &[ShapeInfo],
    pairs: &mut Vec<(usize, usize)>,
    matched_before: &mut Vec<bool>,
    matched_after: &mut Vec<bool>,
) {
    let key = |s: &ShapeInfo| {
        let t = s.name.trim().to_ascii_lowercase();
        if t.is_empty() { None } else { Some(t) }
    };
    let before_keys: Vec<Option<String>> = before.iter().map(key).collect();
    let after_keys: Vec<Option<String>> = after.iter().map(key).collect();

    for (i, k) in before_keys.iter().enumerate() {
        let Some(k) = k.as_ref() else { continue };
        if matched_before[i] || !pairable(&before[i]) {
            continue;
        }
        if before_keys.iter().filter(|x| x.as_deref() == Some(k.as_str())).count() != 1 {
            continue; // duplicated on the outgoing side
        }
        let hits: Vec<usize> = (0..after.len())
            .filter(|&j| !matched_after[j] && pairable(&after[j]) && after_keys[j].as_deref() == Some(k.as_str()))
            .collect();
        if hits.len() != 1 {
            continue; // not unique on the incoming side
        }
        let j = hits[0];
        matched_before[i] = true;
        matched_after[j] = true;
        pairs.push((i, j));
    }
}

/// Tier 2: pair shapes that share the same `cNvPr` id.
fn pair_by_id(
    before: &[ShapeInfo],
    after: &[ShapeInfo],
    pairs: &mut Vec<(usize, usize)>,
    matched_before: &mut Vec<bool>,
    matched_after: &mut Vec<bool>,
) {
    for (i, a) in before.iter().enumerate() {
        if matched_before[i] || !pairable(a) {
            continue;
        }
        if let Some(j) = (0..after.len())
            .find(|&j| !matched_after[j] && pairable(&after[j]) && after[j].id == a.id)
        {
            matched_before[i] = true;
            matched_after[j] = true;
            pairs.push((i, j));
        }
    }
}

/// Tier 3: greedily pair leftover same-kind shapes by nearest centroid.
fn pair_by_centroid(
    before: &[ShapeInfo],
    after: &[ShapeInfo],
    pairs: &mut Vec<(usize, usize)>,
    matched_before: &mut Vec<bool>,
    matched_after: &mut Vec<bool>,
) {
    let mut candidates: Vec<(f64, usize, usize)> = Vec::new();
    for (i, a) in before.iter().enumerate() {
        if matched_before[i] || !pairable(a) {
            continue;
        }
        for (j, b) in after.iter().enumerate() {
            if matched_after[j] || !pairable(b) || a.kind != b.kind {
                continue;
            }
            let (ax, ay) = centroid(&a.bounds.as_ref().unwrap());
            let (bx, by) = centroid(&b.bounds.as_ref().unwrap());
            let d = ((ax - bx).powi(2) + (ay - by).powi(2)).sqrt();
            candidates.push((d, i, j));
        }
    }
    candidates.sort_by(|x, y| x.0.partial_cmp(&y.0).unwrap_or(std::cmp::Ordering::Equal));
    for (_d, i, j) in candidates {
        if matched_before[i] || matched_after[j] {
            continue;
        }
        matched_before[i] = true;
        matched_after[j] = true;
        pairs.push((i, j));
    }
}

fn box_of(b: &BoundingBox) -> MorphBox {
    MorphBox {
        x: b.x_emu,
        y: b.y_emu,
        w: b.width_emu,
        h: b.height_emu,
    }
}

fn centroid(b: &BoundingBox) -> (f64, f64) {
    (
        (b.x_emu + b.width_emu / 2) as f64,
        (b.y_emu + b.height_emu / 2) as f64,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::inspect::BoundingBox;

    fn shape(id: u32, name: &str, kind: &'static str, x: i64, y: i64) -> ShapeInfo {
        ShapeInfo {
            id,
            name: name.to_string(),
            kind,
            placeholder: None,
            bounds: Some(BoundingBox {
                x_emu: x,
                y_emu: y,
                width_emu: 1_000_000,
                height_emu: 1_000_000,
            }),
            text: None,
            fill: None,
            line: None,
            runs: Vec::new(),
            pic: None,
            media: None,
            table: None,
            connector: None,
            children: None,
        }
    }

    #[test]
    fn matches_on_unique_name_case_insensitive() {
        let a = vec![shape(1, "Title Box", "autoshape", 0, 0)];
        let b = vec![shape(9, "title box", "autoshape", 100, 200)];
        let r = detect_morph(&a, &b);
        assert_eq!(r.matches.len(), 1);
        assert_eq!(r.matches[0].id, 1);
        assert_eq!(r.matches[0].name, "title box");
        assert_eq!(r.matches[0].from.x, 0);
        assert_eq!(r.matches[0].to.x, 100);
        assert!(r.removed.is_empty());
        assert!(r.added.is_empty());
    }

    #[test]
    fn id_match_beats_name_change() {
        // Same id, different name -> still paired by id.
        let a = vec![shape(42, "Alpha", "autoshape", 0, 0)];
        let b = vec![shape(42, "Beta", "picture", 0, 0)];
        let r = detect_morph(&a, &b);
        assert_eq!(r.matches.len(), 1);
        assert_eq!(r.matches[0].id, 42);
    }

    #[test]
    fn duplicate_names_skip_to_centroid() {
        // "dup" appears twice on the incoming side, so name pairing is
        // skipped; the nearest-centroid same-kind pair wins.
        let a = vec![shape(1, "dup", "autoshape", 0, 0)];
        let b = vec![
            shape(10, "dup", "autoshape", 0, 0),
            shape(11, "dup", "autoshape", 9_000_000, 9_000_000),
        ];
        let r = detect_morph(&a, &b);
        assert_eq!(r.matches.len(), 1);
        assert_eq!(r.matches[0].to.x, 0); // paired with the nearer one
        assert_eq!(r.added, vec![11]);
    }

    #[test]
    fn unmatched_shapes_report_removed_and_added() {
        let a = vec![shape(1, "Keep", "autoshape", 0, 0)];
        let b = vec![
            shape(2, "Keep", "autoshape", 0, 0),
            shape(3, "New", "picture", 5_000_000, 0),
        ];
        let r = detect_morph(&a, &b);
        assert_eq!(r.matches.len(), 1);
        assert!(r.removed.is_empty());
        assert_eq!(r.added, vec![3]);
    }

    #[test]
    fn different_kinds_do_not_centroid_pair() {
        let a = vec![shape(1, "", "autoshape", 0, 0)];
        let b = vec![shape(2, "", "picture", 0, 0)];
        let r = detect_morph(&a, &b);
        assert!(r.matches.is_empty());
        assert_eq!(r.removed, vec![1]);
        assert_eq!(r.added, vec![2]);
    }

    #[test]
    fn empty_names_do_not_name_pair() {
        let a = vec![shape(1, "   ", "autoshape", 0, 0)];
        let b = vec![shape(2, "   ", "autoshape", 0, 0)];
        let r = detect_morph(&a, &b);
        // No name, but same kind + same centroid -> still paired.
        assert_eq!(r.matches.len(), 1);
        assert_eq!(r.matches[0].id, 1);
    }

    #[test]
    fn shapes_without_bounds_never_pair() {
        let mut a = shape(1, "Ghost", "autoshape", 0, 0);
        a.bounds = None;
        let b = vec![shape(1, "Ghost", "autoshape", 0, 0)];
        let r = detect_morph(&[a], &b);
        assert!(r.matches.is_empty());
        assert_eq!(r.removed, vec![1]);
        assert_eq!(r.added, vec![1]);
    }
}
