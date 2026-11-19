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

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn a_changed_line_is_one_hunk_with_its_context() {
        let diff = unified(
            "./Blastfile",
            "app \"shop\"\nview v \"/x\" { }\n",
            "app \"shop\"\n\nview v \"/x\" {\n}\n",
        );
        assert!(
            diff.starts_with("--- ./Blastfile\n+++ ./Blastfile\n@@ -1,2 +1,4 @@\n"),
            "{diff}"
        );
        assert!(diff.contains("\n app \"shop\"\n"), "{diff}");
        assert!(diff.contains("\n-view v \"/x\" { }\n"), "{diff}");
        assert!(diff.contains("\n+\n"), "{diff}");
        assert!(diff.ends_with("+}\n"), "{diff}");
    }

    #[test]
    fn identical_documents_render_nothing() {
        // Allowed: a canonical file prints no diff at all.
        assert!(unified("p", "a\nb\n", "a\nb\n").is_empty());
        // Denied twin: one added byte is a hunk, never silence.
        assert!(!unified("p", "a\nb\n", "a\nb\nc\n").is_empty());
    }

    #[test]
    fn far_apart_changes_are_two_hunks_with_the_right_line_numbers() {
        let old = "1\n2\n3\n4\n5\n6\n7\n8\n9\n10\n11\n12\n";
        let new = "one\n2\n3\n4\n5\n6\n7\n8\n9\n10\n11\ntwelve\n";
        let diff = unified("p", old, new);
        assert_eq!(diff.matches("@@ -").count(), 2, "{diff}");
        assert!(diff.contains("@@ -1,4 +1,4 @@"), "{diff}");
        assert!(diff.contains("@@ -9,4 +9,4 @@"), "{diff}");
    }

    #[test]
    fn an_insertion_at_the_end_names_both_line_ranges() {
        let diff = unified("p", "a\nb\n", "a\nb\nc\n");
        assert!(diff.contains("@@ -1,2 +1,3 @@"), "{diff}");
        assert!(diff.ends_with("+c\n"), "{diff}");
    }

    #[test]
    fn a_document_too_large_for_the_table_still_renders_a_hunk() {
        // Denied/edge twin: the fallback must still be a real hunk header, not
        // a count and not a truncated table.
        let old: Vec<String> = (0..3000).map(|index| format!("old {index}")).collect();
        let new: Vec<String> = (0..3000).map(|index| format!("new {index}")).collect();
        let diff = unified("p", &old.join("\n"), &new.join("\n"));
        assert!(
            diff.starts_with("--- p\n+++ p\n@@ -1,3000 +1,3000 @@\n"),
            "{}",
            diff.get(..80).unwrap_or_default()
        );
    }

    #[test]
    fn a_missing_final_newline_is_a_real_hunk() {
        // The hole this closes: `lines()` erased the trailing `\n`, so a file
        // whose only deviation was a missing final newline rendered no diff at
        // all and `fmt --check` printed the bare verdict.
        let diff = unified("p", "a\nb", "a\nb\n");
        assert!(
            diff.starts_with("--- p\n+++ p\n@@ -1,2 +1,2 @@\n"),
            "{diff}"
        );
        assert!(
            diff.contains("\n-b\n\\ No newline at end of file\n"),
            "{diff}"
        );
        assert!(diff.contains("\n+b\n"), "{diff}");
        // Denied twin: the canonical text already ends in a newline.
        assert!(unified("p", "a\nb\n", "a\nb\n").is_empty());
    }

    #[test]
    fn an_empty_file_is_one_hunk_of_added_lines() {
        let diff = unified("p", "", "a\nb\n");
        assert!(
            diff.starts_with("--- p\n+++ p\n@@ -0,0 +1,2 @@\n"),
            "{diff}"
        );
        // Two added lines, and no line removed (`+++` is a header, not a row).
        assert_eq!(
            diff.lines()
                .filter(|line| line.starts_with('+') && !line.starts_with("+++"))
                .count(),
            2,
            "{diff}"
        );
        // Both empty is no change at all.
        assert!(unified("p", "", "").is_empty());
    }

    #[test]
    fn crlf_input_shows_the_carriage_return_it_would_drop() {
        // `fmt` writes LF; a CRLF file must show the bytes it would lose.
        let diff = unified("p", "a\r\nb\r\n", "a\nb\n");
        assert_eq!(diff.matches("@@ -").count(), 1, "{diff}");
        assert!(diff.contains("\n-a\r\n"), "{diff}");
        assert!(diff.contains("\n+a\n"), "{diff}");
    }

    #[test]
    fn quotes_hide_no_line_and_a_braced_pattern_is_one_row() {
        // The diff is line-based; a pattern holding `{`/`}` and a statement
        // holding `'` must not confuse the row count.
        let old =
            "view v \"/orders/{id:int}\" {\n    postgres {\n        query 'SELECT 1'\n    }\n}\n";
        let new =
            "view v \"/orders/{id:int}\" {\n    postgres {\n        query 'SELECT 2'\n    }\n}\n";
        let diff = unified("p", old, new);
        assert_eq!(diff.matches("@@ -").count(), 1, "{diff}");
        assert!(diff.contains("@@ -1,5 +1,5 @@"), "{diff}");
        assert!(diff.contains("\n-        query 'SELECT 1'\n"), "{diff}");
        assert!(diff.contains("\n+        query 'SELECT 2'\n"), "{diff}");
    }
}
