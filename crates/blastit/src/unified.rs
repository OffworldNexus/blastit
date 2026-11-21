//! The one unified diff, for `blastit fmt --check` (§14.4, `AGENTS.md` §4).
//!
//! "not in canonical layout" tells a developer *that* the file would change and
//! nothing else. [`unified`] renders the change the way every editor, every
//! code forge and `patch` read it: `--- <path>`, `+++ <path>`, one `@@ -l,c
//! +l,c @@` header per group of changed lines, and three lines of context
//! around each group.
//!
//! The render is bounded. The longest-common-subsequence table stops at
//! [`MAX_CELLS`] cells; a pair of documents that would need more (a file
//! rewritten wholesale) falls back to one hunk that keeps the common prefix and
//! suffix as context and replaces the middle. Either way the output is a real
//! hunk, never a bare count.

/// How many unchanged lines a hunk keeps on each side (the unified default).
const CONTEXT: usize = 3;

/// How many LCS cells [`rows`] may allocate: 4 Mi cells, 16 MiB of `u32`.
const MAX_CELLS: usize = 4_194_304;

/// One diff row: `' '` unchanged, `'-'` only in `old`, `'+'` only in `new`.
type Row<'a> = (char, &'a str);

/// Renders `old` → `new` as a unified diff naming `path` on both headers.
///
/// Empty when the two documents are equal, so a caller can print the result
/// unconditionally.
pub fn unified(path: &str, old: &str, new: &str) -> String {
    if old == new {
        return String::new();
    }
    // `split_inclusive` keeps each line's own `\n`, so a document whose only
    // deviation is a missing final newline is still a difference (`-b` / `+b`
    // plus the `\ No newline at end of file` marker), never an empty diff.
    let old_lines: Vec<&str> = old.split_inclusive('\n').collect();
    let new_lines: Vec<&str> = new.split_inclusive('\n').collect();
    let diff = rows(&old_lines, &new_lines);

    // Every change, widened by CONTEXT and merged when the widened ranges
    // touch: one `@@` header per printed group of rows.
    let mut ranges: Vec<(usize, usize)> = Vec::new();
    for (index, row) in diff.iter().enumerate() {
        if row.0 == ' ' {
            continue;
        }
        let start = index.saturating_sub(CONTEXT);
        let end = (index + CONTEXT + 1).min(diff.len());
        match ranges.last_mut() {
            Some(last) if start <= last.1 => last.1 = last.1.max(end),
            _ => ranges.push((start, end)),
        }
    }
    if ranges.is_empty() {
        return String::new();
    }

    // How many lines of each side precede every row, so a hunk header can name
    // its 1-based start without walking the whole document again.
    let mut old_before = Vec::with_capacity(diff.len() + 1);
    let mut new_before = Vec::with_capacity(diff.len() + 1);
    let (mut old_line, mut new_line) = (0usize, 0usize);
    for row in &diff {
        old_before.push(old_line);
        new_before.push(new_line);
        if row.0 != '+' {
            old_line += 1;
        }
        if row.0 != '-' {
            new_line += 1;
        }
    }
    old_before.push(old_line);
    new_before.push(new_line);

    let mut out = format!("--- {path}\n+++ {path}\n");
    for (start, end) in ranges {
        let old_count = old_before[end] - old_before[start];
        let new_count = new_before[end] - new_before[start];
        // A side with no line is named by the line before the hunk, as diff
        // writes `@@ -3,0 +4,2 @@` for a pure insertion.
        let old_start = if old_count == 0 {
            old_before[start]
        } else {
            old_before[start] + 1
        };
        let new_start = if new_count == 0 {
            new_before[start]
        } else {
            new_before[start] + 1
        };
        out.push_str(&format!(
            "@@ -{old_start},{old_count} +{new_start},{new_count} @@\n"
        ));
        for row in &diff[start..end] {
            out.push(row.0);
            out.push_str(row.1);
            if !row.1.ends_with('\n') {
                // The last line of a document that does not end in a newline:
                // `diff -u` writes the line, then this marker.
                out.push('\n');
                out.push_str("\\ No newline at end of file\n");
            }
        }
    }
    out
}

/// The edit script of two line lists, in document order.
fn rows<'a>(old: &[&'a str], new: &[&'a str]) -> Vec<Row<'a>> {
    let (old_len, new_len) = (old.len(), new.len());
    if old_len == 0 {
        return new.iter().map(|line| ('+', *line)).collect();
    }
    if new_len == 0 {
        return old.iter().map(|line| ('-', *line)).collect();
    }
    if old_len.saturating_mul(new_len) > MAX_CELLS {
        return replacement(old, new);
    }

    // `table[i * width + j]` is the LCS length of `old[i..]` and `new[j..]`.
    let width = new_len + 1;
    let mut table = vec![0u32; (old_len + 1) * width];
    for i in (0..old_len).rev() {
        for j in (0..new_len).rev() {
            let at = i * width + j;
            table[at] = if old[i] == new[j] {
                table[(i + 1) * width + j + 1] + 1
            } else {
                table[(i + 1) * width + j].max(table[i * width + j + 1])
            };
        }
    }

    let mut script = Vec::with_capacity(old_len + new_len);
    let (mut i, mut j) = (0usize, 0usize);
    while i < old_len && j < new_len {
        if old[i] == new[j] {
            script.push((' ', old[i]));
            i += 1;
            j += 1;
        } else if table[(i + 1) * width + j] >= table[i * width + j + 1] {
            script.push(('-', old[i]));
            i += 1;
        } else {
            script.push(('+', new[j]));
            j += 1;
        }
    }
    script.extend(old[i..].iter().map(|line| ('-', *line)));
    script.extend(new[j..].iter().map(|line| ('+', *line)));
    script
}

/// The bounded fallback: keep the common prefix and suffix as context and
/// replace everything between them.
fn replacement<'a>(old: &[&'a str], new: &[&'a str]) -> Vec<Row<'a>> {
    let prefix = old.iter().zip(new).take_while(|(a, b)| a == b).count();
    let suffix = old[prefix..]
        .iter()
        .rev()
        .zip(new[prefix..].iter().rev())
        .take_while(|(a, b)| a == b)
        .count();
    let old_middle = (prefix..old.len() - suffix).map(|at| ('-', old[at]));
    let new_middle = (prefix..new.len() - suffix).map(|at| ('+', new[at]));
    old.iter()
        .take(prefix)
        .map(|line| (' ', *line))
        .chain(old_middle)
        .chain(new_middle)
        .chain(old[old.len() - suffix..].iter().map(|line| (' ', *line)))
        .collect()
}
