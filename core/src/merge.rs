//! Line-based three-way merge.
//!
//! Both sides started from `base`. Changes to different lines are combined.
//! Where both sides changed the same lines differently, both versions are
//! kept, ours first, so nothing is ever lost: you delete the one you don't
//! want. (A line deleted on one side and edited on the other keeps the edit.)

use similar::{capture_diff_slices, Algorithm, DiffOp};

struct Hunk {
    /// Replaced base lines: base[start..end].
    start: usize,
    end: usize,
    lines: Vec<String>,
}

fn lines(s: &str) -> Vec<String> {
    s.split_inclusive('\n')
        .map(|l| if l.ends_with('\n') { l.to_string() } else { format!("{l}\n") })
        .collect()
}

fn hunks(base: &[String], side: &[String]) -> Vec<Hunk> {
    capture_diff_slices(Algorithm::Myers, base, side)
        .into_iter()
        .filter_map(|op| match op {
            DiffOp::Equal { .. } => None,
            DiffOp::Delete { old_index, old_len, .. } => Some(Hunk { start: old_index, end: old_index + old_len, lines: vec![] }),
            DiffOp::Insert { old_index, new_index, new_len } => {
                Some(Hunk { start: old_index, end: old_index, lines: side[new_index..new_index + new_len].to_vec() })
            }
            DiffOp::Replace { old_index, old_len, new_index, new_len } => Some(Hunk {
                start: old_index,
                end: old_index + old_len,
                lines: side[new_index..new_index + new_len].to_vec(),
            }),
        })
        .collect()
}

/// One side's version of base[start..end], given its hunks inside that range.
fn region(base: &[String], start: usize, end: usize, hs: &[&Hunk]) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = start;
    for h in hs {
        out.extend_from_slice(&base[cur..h.start]);
        out.extend(h.lines.iter().cloned());
        cur = h.end;
    }
    out.extend_from_slice(&base[cur..end]);
    out
}

/// Two hunks collide if their base ranges overlap, or both insert at the
/// same point. Merely adjacent edits (line 2 here, line 3 there) don't.
fn collides(s: usize, e: usize, h: &Hunk) -> bool {
    h.start < e || (h.start == s && (h.start == h.end || s == e))
}

pub fn merge3(base: &str, ours: &str, theirs: &str) -> String {
    if ours == theirs || theirs == base {
        return ours.to_string();
    }
    if ours == base {
        return theirs.to_string();
    }
    let (b, o, t) = (lines(base), lines(ours), lines(theirs));
    let (ho, ht) = (hunks(&b, &o), hunks(&b, &t));
    let (mut i, mut j, mut pos) = (0, 0, 0);
    let mut out: Vec<String> = Vec::new();

    loop {
        // The earliest remaining hunk opens a group.
        let first = match (ho.get(i), ht.get(j)) {
            (None, None) => break,
            (Some(a), None) => a,
            (None, Some(c)) => c,
            (Some(a), Some(c)) => if a.start <= c.start { a } else { c },
        };
        let (s, mut e) = (first.start, first.end);
        let (mut go, mut gt): (Vec<&Hunk>, Vec<&Hunk>) = (vec![], vec![]);
        // Pull in every hunk from either side that collides with the group.
        loop {
            if let Some(h) = ho.get(i).filter(|h| collides(s, e, h)) {
                e = e.max(h.end);
                go.push(h);
                i += 1;
            } else if let Some(h) = ht.get(j).filter(|h| collides(s, e, h)) {
                e = e.max(h.end);
                gt.push(h);
                j += 1;
            } else {
                break;
            }
        }

        out.extend_from_slice(&b[pos..s]);
        let ro = region(&b, s, e, &go);
        let rt = region(&b, s, e, &gt);
        if gt.is_empty() || ro == rt {
            out.extend(ro);
        } else if go.is_empty() {
            out.extend(rt);
        } else if ro.len() == e - s && rt.len() == e - s {
            // Both edited these lines in place (ticks, rewording): decide
            // line by line, so only lines changed on both sides are doubled.
            for k in 0..e - s {
                let (bl, ol, tl) = (&b[s + k], &ro[k], &rt[k]);
                if ol == bl || ol == tl {
                    out.push(tl.clone());
                } else if tl == bl {
                    out.push(ol.clone());
                } else {
                    out.push(ol.clone());
                    out.push(tl.clone());
                }
            }
        } else {
            // A real conflict: keep both, without repeating shared lines.
            let extra: Vec<String> = rt.into_iter().filter(|l| !ro.contains(l)).collect();
            out.extend(ro);
            out.extend(extra);
        }
        pos = e;
    }
    out.extend_from_slice(&b[pos..]);
    out.concat()
}

#[cfg(test)]
mod tests {
    use super::merge3;

    const BASE: &str = "- [ ] a\n- [ ] b\n- [ ] c\n- [ ] d\n";

    #[test]
    fn trivial_cases() {
        assert_eq!(merge3(BASE, BASE, BASE), BASE);
        assert_eq!(merge3(BASE, "x\n", BASE), "x\n");
        assert_eq!(merge3(BASE, BASE, "y\n"), "y\n");
        assert_eq!(merge3(BASE, "z\n", "z\n"), "z\n");
    }

    #[test]
    fn different_lines_combine() {
        let ours = "- [x] a\n- [ ] b\n- [ ] c\n- [ ] d\n";
        let theirs = "- [ ] a\n- [ ] b\n- [ ] c\n- [x] d\n";
        assert_eq!(merge3(BASE, ours, theirs), "- [x] a\n- [ ] b\n- [ ] c\n- [x] d\n");
    }

    #[test]
    fn adjacent_lines_combine() {
        let ours = "- [ ] a\n- [x] b\n- [ ] c\n- [ ] d\n";
        let theirs = "- [ ] a\n- [ ] b\n- [x] c\n- [ ] d\n";
        assert_eq!(merge3(BASE, ours, theirs), "- [ ] a\n- [x] b\n- [x] c\n- [ ] d\n");
    }

    #[test]
    fn both_append_keeps_both() {
        let ours = format!("{BASE}- [ ] from laptop\n");
        let theirs = format!("{BASE}- [ ] from mac\n");
        assert_eq!(merge3(BASE, &ours, &theirs), format!("{BASE}- [ ] from laptop\n- [ ] from mac\n"));
    }

    #[test]
    fn same_line_conflict_keeps_both_side_by_side() {
        let ours = "- [ ] a\n- [x] b\n- [ ] c\n- [ ] d\n";
        let theirs = "- [ ] a\n- [ ] b, renamed\n- [ ] c\n- [ ] d\n";
        assert_eq!(merge3(BASE, ours, theirs), "- [ ] a\n- [x] b\n- [ ] b, renamed\n- [ ] c\n- [ ] d\n");
    }

    #[test]
    fn overlapping_in_place_edits_merge_per_line() {
        let base = "- [ ] a\n- [ ] b\n- [ ] c\n";
        let ours = "- [ ] a\n- [ ] b (mac)\n- [x] c\n";
        let theirs = "- [x] a\n- [ ] b (laptop)\n- [ ] c\n";
        assert_eq!(merge3(base, ours, theirs), "- [x] a\n- [ ] b (mac)\n- [ ] b (laptop)\n- [x] c\n");
    }

    #[test]
    fn delete_vs_edit_keeps_the_edit() {
        let ours = "- [ ] a\n- [ ] c\n- [ ] d\n";
        let theirs = "- [ ] a\n- [ ] b!\n- [ ] c\n- [ ] d\n";
        assert_eq!(merge3(BASE, ours, theirs), "- [ ] a\n- [ ] b!\n- [ ] c\n- [ ] d\n");
    }

    #[test]
    fn missing_final_newline() {
        assert_eq!(merge3("a\nb", "a!\nb", "a\nb!"), "a!\nb!\n");
    }

    #[test]
    fn from_empty_both_sides() {
        assert_eq!(merge3("", "- [ ] x\n", "- [ ] y\n"), "- [ ] x\n- [ ] y\n");
    }
}
