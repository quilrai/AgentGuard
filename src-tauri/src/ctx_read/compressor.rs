use similar::{ChangeTag, TextDiff};

pub fn diff_content(old_content: &str, new_content: &str) -> String {
    if old_content == new_content {
        return "∅ no changes".to_string();
    }

    let diff = TextDiff::from_lines(old_content, new_content);
    let mut changes = Vec::new();
    let mut additions = 0usize;
    let mut deletions = 0usize;

    for change in diff.iter_all_changes() {
        let line_no = change.new_index().or(change.old_index()).map(|i| i + 1);
        let text = change.value().trim_end_matches('\n');
        match change.tag() {
            ChangeTag::Insert => {
                additions += 1;
                if let Some(n) = line_no {
                    changes.push(format!("+{n}: {text}"));
                }
            }
            ChangeTag::Delete => {
                deletions += 1;
                if let Some(n) = line_no {
                    changes.push(format!("-{n}: {text}"));
                }
            }
            ChangeTag::Equal => {}
        }
    }

    if changes.is_empty() {
        return "∅ no changes".to_string();
    }

    changes.push(format!("\n∂ +{additions}/-{deletions} lines"));
    changes.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_diff_insertion() {
        let old = "line1\nline2\nline3";
        let new = "line1\nline2\nnew_line\nline3";
        let result = diff_content(old, new);
        assert!(result.contains('+'));
        assert!(result.contains("new_line"));
    }

    #[test]
    fn test_diff_deletion() {
        let old = "line1\nline2\nline3";
        let new = "line1\nline3";
        let result = diff_content(old, new);
        assert!(result.contains('-'));
        assert!(result.contains("line2"));
    }

    #[test]
    fn test_diff_no_changes() {
        let content = "same\ncontent";
        assert_eq!(diff_content(content, content), "∅ no changes");
    }
}
