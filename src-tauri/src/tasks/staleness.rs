//! Pure stale checks for whole-file and inline line-granular results.

/// Determine whether a result is stale by comparing the submitted source hash
/// with the current source hash. Returns true when:
/// - the file path is None (cannot verify; treat as not stale by default), OR
/// - the file does not exist, OR
/// - the file exists but its current content hash differs from submitted_source_hash.
/// Otherwise return false.
///
/// `read_file` is a function that takes a path and returns Ok(String) with the file content,
/// or Err(()) if the file cannot be read. This keeps the function pure and testable.
pub(crate) fn is_result_stale(
    file_path: Option<&str>,
    submitted_source_hash: &str,
    read_file: impl Fn(&str) -> Result<String, ()>,
) -> bool {
    let Some(path) = file_path else {
        return false;
    };
    let content = match read_file(path) {
        Ok(content) => content,
        Err(()) => return true,
    };
    crate::pipeline::stable_hash(&content) != submitted_source_hash
}

/// Determine whether an inline command result is stale by re-parsing the file
/// and locating the command line by the hash of its raw text. Staleness is
/// line-granular: an edit to the command line itself invalidates the result,
/// while edits to sibling lines in the same block or to other blocks do not.
/// Returns true when:
/// - the file path is None (cannot verify; treat as not stale by default), OR
/// - the file cannot be read, OR
/// - the file parses but no non-excluded block contains a slash command whose
///   raw command line hashes to the submitted line_text_hash (the command line
///   was edited or removed).
/// Otherwise return false.
///
/// `read_file` is a function that takes a path and returns Ok(String) with the
/// file content, or Err(()) if the file cannot be read. This keeps the
/// function pure and testable.
pub(crate) fn is_inline_result_stale(
    file_path: Option<&str>,
    line_text_hash: &str,
    read_file: impl Fn(&str) -> Result<String, ()>,
) -> bool {
    let Some(path) = file_path else {
        return false;
    };
    let content = match read_file(path) {
        Ok(content) => content,
        Err(()) => return true,
    };
    let blocks = crate::pipeline::parse_markdown_blocks(&content);
    for block in blocks.iter().filter(|b| !b.excluded) {
        if let Some(parsed) = crate::pipeline::parse_slash_command_in_block(block) {
            let raw_line = &block.text[parsed.line_start..parsed.line_end];
            if crate::pipeline::stable_hash(raw_line) == line_text_hash {
                return false;
            }
        }
    }
    true
}