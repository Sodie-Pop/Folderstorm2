//! Folderstorm: the line diff of `inventory_content_preview` and of the save
//! dialog (folderstorm/plans/mcp-content.md): the common first and last
//! lines are set aside, then the changed middle is compared line by line
//! (longest common subsequence). A middle too large to compare on the frame
//! thread is shown as one block removed and added; the counts then stay
//! true for that block, only the pairing is coarser.

use std::fmt::Write as _;

/// Line pairs compared at most (a 1,000 × 1,000 table of `u16`, 2 MB).
const PAIRS_MAX: usize = 1_000_000;
/// Lines of context around each hunk, as `diff -u`.
const CONTEXT: usize = 3;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Op {
    Same,
    Removed,
    Added,
}

/// What an edit changes.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Diff {
    pub added: usize,
    pub removed: usize,
    pub hunks: usize,
    /// The unified diff, `max` bytes at most.
    pub text: String,
    pub truncated: bool,
    /// The changed lines in order, `-` or `+` and the line without its end.
    pub changed: Vec<(char, String)>,
}

/// The lines of `text`, each with its end (so that a missing last newline
/// is a change too).
fn lines(text: &str) -> Vec<&str> {
    text.split_inclusive('\n').collect()
}

/// The operations turning `old` into `new`.
fn ops(old: &[&str], new: &[&str]) -> Vec<Op> {
    let prefix = old.iter().zip(new).take_while(|(a, b)| a == b).count();
    let rest_old = &old[prefix..];
    let rest_new = &new[prefix..];
    let suffix = rest_old.iter().rev().zip(rest_new.iter().rev()).take_while(|(a, b)| a == b).count();
    let a = &rest_old[..rest_old.len() - suffix];
    let b = &rest_new[..rest_new.len() - suffix];
    let mut out = vec![Op::Same; prefix];
    if a.len().saturating_mul(b.len()) > PAIRS_MAX || a.len() > usize::from(u16::MAX) || b.len() > usize::from(u16::MAX) {
        out.extend(std::iter::repeat_n(Op::Removed, a.len()));
        out.extend(std::iter::repeat_n(Op::Added, b.len()));
    } else {
        out.extend(middle(a, b));
    }
    out.extend(std::iter::repeat_n(Op::Same, suffix));
    out
}

/// The longest common subsequence of `a` and `b`, as operations.
fn middle(a: &[&str], b: &[&str]) -> Vec<Op> {
    let width = b.len() + 1;
    // table[i * width + j]: the LCS length of a[i..] and b[j..]
    let mut table = vec![0u16; (a.len() + 1) * width];
    for (i, line_a) in a.iter().enumerate().rev() {
        for (j, line_b) in b.iter().enumerate().rev() {
            table[i * width + j] = if line_a == line_b {
                table[(i + 1) * width + j + 1] + 1
            } else {
                table[(i + 1) * width + j].max(table[i * width + j + 1])
            };
        }
    }
    let mut out = Vec::with_capacity(a.len() + b.len());
    let (mut i, mut j) = (0, 0);
    while i < a.len() && j < b.len() {
        if a[i] == b[j] {
            out.push(Op::Same);
            i += 1;
            j += 1;
        } else if table[(i + 1) * width + j] >= table[i * width + j + 1] {
            out.push(Op::Removed);
            i += 1;
        } else {
            out.push(Op::Added);
            j += 1;
        }
    }
    out.extend(std::iter::repeat_n(Op::Removed, a.len() - i));
    out.extend(std::iter::repeat_n(Op::Added, b.len() - j));
    out
}

/// A line as shown: without its end.
fn shown(line: &str) -> String {
    let line = line.strip_suffix('\n').unwrap_or(line);
    line.strip_suffix('\r').unwrap_or(line).to_owned()
}

/// The diff of `old` and `new`, its text `max` bytes at most.
pub fn diff(old: &str, new: &str, max: usize) -> Diff {
    let old_lines = lines(old);
    let new_lines = lines(new);
    let ops = ops(&old_lines, &new_lines);
    let mut out = Diff::default();
    // each op's line numbers (0-based) in old and new
    let mut at = Vec::with_capacity(ops.len());
    let (mut i, mut j) = (0usize, 0usize);
    for op in &ops {
        at.push((i, j));
        match op {
            Op::Same => {
                i += 1;
                j += 1;
            }
            Op::Removed => {
                out.removed += 1;
                out.changed.push(('-', shown(old_lines[i])));
                i += 1;
            }
            Op::Added => {
                out.added += 1;
                out.changed.push(('+', shown(new_lines[j])));
                j += 1;
            }
        }
    }
    // hunks: runs of changes with their context, joined when close
    let changed: Vec<usize> = ops.iter().enumerate().filter(|(_, op)| **op != Op::Same).map(|(k, _)| k).collect();
    let mut groups: Vec<(usize, usize)> = Vec::new();
    for k in changed {
        let start = k.saturating_sub(CONTEXT);
        let end = (k + CONTEXT + 1).min(ops.len());
        match groups.last_mut() {
            Some(last) if start <= last.1 => last.1 = end,
            _ => groups.push((start, end)),
        }
    }
    out.hunks = groups.len();
    let mut text = String::new();
    for (start, end) in groups {
        let (old_start, new_start) = at[start];
        let old_count = ops[start..end].iter().filter(|op| **op != Op::Added).count();
        let new_count = ops[start..end].iter().filter(|op| **op != Op::Removed).count();
        let mut hunk = String::new();
        let _ = writeln!(
            hunk,
            "@@ -{},{} +{},{} @@",
            old_start + usize::from(old_count > 0),
            old_count,
            new_start + usize::from(new_count > 0),
            new_count
        );
        for (k, op) in ops.iter().enumerate().take(end).skip(start) {
            let (oi, nj) = at[k];
            let (mark, line) = match op {
                Op::Same => (' ', old_lines[oi]),
                Op::Removed => ('-', old_lines[oi]),
                Op::Added => ('+', new_lines[nj]),
            };
            let _ = writeln!(hunk, "{mark}{}", shown(line));
        }
        if text.len() + hunk.len() > max {
            out.truncated = true;
            break;
        }
        text.push_str(&hunk);
    }
    out.text = text;
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_lines_added_and_removed() {
        let d = diff("a\nb\nc\n", "a\nB\nc\nd\n", 4096);
        assert_eq!((d.added, d.removed, d.hunks), (2, 1, 1));
        assert_eq!(d.changed, vec![('-', "b".into()), ('+', "B".into()), ('+', "d".into())]);
        assert_eq!(d.text, "@@ -1,3 +1,4 @@\n a\n-b\n+B\n c\n+d\n");
        assert!(!d.truncated);
    }

    #[test]
    fn nothing_changed_is_an_empty_diff() {
        let d = diff("same\n", "same\n", 4096);
        assert_eq!(d, Diff::default());
        let d = diff("", "", 4096);
        assert_eq!((d.added, d.removed, d.hunks), (0, 0, 0));
    }

    #[test]
    fn a_missing_last_newline_is_a_change() {
        let d = diff("a\nb\n", "a\nb", 4096);
        assert_eq!((d.added, d.removed), (1, 1));
    }

    #[test]
    fn distant_changes_make_separate_hunks_with_context() {
        let old: String = (0..40).map(|n| format!("line {n}\n")).collect();
        let new = old.replace("line 2\n", "line two\n").replace("line 30\n", "");
        let d = diff(&old, &new, 4096);
        assert_eq!((d.added, d.removed, d.hunks), (1, 2, 2));
        assert!(d.text.starts_with("@@ -1,6 +1,6 @@\n line 0\n"), "{}", d.text);
        assert!(d.text.contains("@@ -28,7 +28,6 @@"), "{}", d.text);
    }

    #[test]
    fn the_text_is_cut_on_a_hunk_and_says_so() {
        let old: String = (0..200).map(|n| format!("{n}\n")).collect();
        let new: String = (0..200)
            .map(|n| if n % 10 == 0 { format!("x{n}\n") } else { format!("{n}\n") })
            .collect();
        let d = diff(&old, &new, 300);
        assert!(d.truncated);
        assert!(d.text.len() <= 300);
        assert!(d.text.ends_with('\n'));
        assert_eq!((d.added, d.removed), (20, 20), "the counts are whole");
    }

    #[test]
    fn a_huge_middle_is_one_replaced_block() {
        let old: String = (0..1500).map(|n| format!("a{n}\n")).collect();
        let new: String = (0..1500).map(|n| format!("b{n}\n")).collect();
        let d = diff(&old, &new, 1 << 20);
        assert_eq!((d.added, d.removed, d.hunks), (1500, 1500, 1));
    }
}
