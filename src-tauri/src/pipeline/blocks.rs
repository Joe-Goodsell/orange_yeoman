//! Block model: the mdast projection into flat Blocks with exact byte offsets,
//! heading chains, and exclusion flags. Parsing is delegated to the `markdown`
//! crate (CommonMark + GFM + frontmatter).

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum BlockKind {
    Heading,
    Paragraph,
    ListItem,
    BlockQuote,
    Table,
    FrontMatter,
    CodeFence,
    Html,
}

/// One Markdown block: the single type used by the parser, the incremental
/// diff, and the store. `id` is `Some(row_id)` for a block loaded from the
/// `blocks` table and `None` for a freshly parsed block that has no row yet.
/// `text` is the block's exact source text (`input[char_start..char_end]`);
/// it is empty for stored rows because the DB keeps only the hash.
/// `heading_path` is the heading chain joined with " > " (same separator the
/// prompt builders use). `char_start`/`char_end` are byte offsets into the
/// original input, inclusive and exclusive, named after the DB columns.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Block {
    pub(crate) id: Option<i64>,
    pub(crate) kind: BlockKind,
    pub(crate) text: String,
    pub(crate) block_hash: String,
    pub(crate) heading_path: String,
    pub(crate) char_start: usize,
    pub(crate) char_end: usize,
    pub(crate) excluded: bool, // true for FrontMatter, CodeFence, Html
}

/// Stable hash of a string, returned as a lowercase hex string.
/// FNV-1a 64-bit (offset basis 0xcbf29ce484222325, prime 0x100000001b3).
/// Deterministic across Rust releases and process restarts, so block hashes
/// survive the app being rebuilt or restarted and can anchor the concept store.
pub(crate) fn stable_hash(text: &str) -> String {
    const FNV_OFFSET_BASIS: u64 = 0xcbf29ce484222325;
    const FNV_PRIME: u64 = 0x100000001b3;
    let mut hash = FNV_OFFSET_BASIS;
    for byte in text.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(FNV_PRIME);
    }
    format!("{:016x}", hash)
}

/// Serialize a BlockKind to its stable text spelling for the `blocks.kind`
/// column; `block_kind_from_str` parses it back. Unknown spellings fall back
/// to Paragraph; the kind is stored for later stages and never used to drive
/// diff/apply logic.
pub(crate) fn block_kind_as_str(kind: &BlockKind) -> &'static str {
    match kind {
        BlockKind::Heading => "heading",
        BlockKind::Paragraph => "paragraph",
        BlockKind::ListItem => "list_item",
        BlockKind::BlockQuote => "block_quote",
        BlockKind::Table => "table",
        BlockKind::FrontMatter => "front_matter",
        BlockKind::CodeFence => "code_fence",
        BlockKind::Html => "html",
    }
}

/// Parse the `blocks.kind` column back into a BlockKind. Total: an unknown
/// spelling (e.g. written by a newer schema) falls back to Paragraph. The
/// kind is stored for later stages and never drives diff/apply logic, so the
/// fallback is safe.
pub(crate) fn block_kind_from_str(s: &str) -> BlockKind {
    match s {
        "heading" => BlockKind::Heading,
        "paragraph" => BlockKind::Paragraph,
        "list_item" => BlockKind::ListItem,
        "block_quote" => BlockKind::BlockQuote,
        "table" => BlockKind::Table,
        "front_matter" => BlockKind::FrontMatter,
        "code_fence" => BlockKind::CodeFence,
        "html" => BlockKind::Html,
        _ => BlockKind::Paragraph,
    }
}

/// Parse a Markdown string into blocks with exact byte offsets, heading chains,
/// and exclusion flags. Offsets are byte offsets into the input string.
pub(crate) fn parse_markdown_blocks(input: &str) -> Vec<Block> {
    let opts = markdown::ParseOptions {
        constructs: markdown::Constructs {
            frontmatter: true,
            ..markdown::Constructs::gfm()
        },
        ..markdown::ParseOptions::default()
    };
    let tree = markdown::to_mdast(input, &opts).expect("markdown never errors on normal input");
    let markdown::mdast::Node::Root(root) = tree else {
        return Vec::new();
    };
    let mut blocks = Vec::new();
    let mut chain: Vec<String> = Vec::new();
    for child in &root.children {
        emit_blocks(&mut blocks, child, input, &mut chain);
    }
    blocks
}

/// Map one top-level mdast node to one or more Blocks. Headings also
/// update the heading chain; lists flatten to one block per list item. Nodes
/// without a matching block kind (ThematicBreak, Definition,
/// FootnoteDefinition, Math, MDX) are skipped: they are not content we
/// fact-check.
fn emit_blocks(
    blocks: &mut Vec<Block>,
    node: &markdown::mdast::Node,
    input: &str,
    chain: &mut Vec<String>,
) {
    use markdown::mdast::Node;

    match node {
        Node::Heading(_) => {
            let pos = node.position().expect("mdast nodes have positions");
            push_block(
                blocks,
                BlockKind::Heading,
                input,
                pos.start.offset,
                pos.end.offset,
                chain,
            );
            chain.push(collect_text(node));
        }
        Node::Paragraph(_) => {
            let pos = node.position().expect("mdast nodes have positions");
            push_block(
                blocks,
                BlockKind::Paragraph,
                input,
                pos.start.offset,
                pos.end.offset,
                chain,
            );
        }
        Node::Blockquote(_) => {
            let pos = node.position().expect("mdast nodes have positions");
            push_block(
                blocks,
                BlockKind::BlockQuote,
                input,
                pos.start.offset,
                pos.end.offset,
                chain,
            );
        }
        Node::Table(_) => {
            let pos = node.position().expect("mdast nodes have positions");
            push_block(
                blocks,
                BlockKind::Table,
                input,
                pos.start.offset,
                pos.end.offset,
                chain,
            );
        }
        Node::Yaml(_) | Node::Toml(_) => {
            let pos = node.position().expect("mdast nodes have positions");
            push_block(
                blocks,
                BlockKind::FrontMatter,
                input,
                pos.start.offset,
                pos.end.offset,
                chain,
            );
        }
        Node::Code(_) => {
            let pos = node.position().expect("mdast nodes have positions");
            push_block(
                blocks,
                BlockKind::CodeFence,
                input,
                pos.start.offset,
                pos.end.offset,
                chain,
            );
        }
        Node::Html(_) => {
            let pos = node.position().expect("mdast nodes have positions");
            push_block(
                blocks,
                BlockKind::Html,
                input,
                pos.start.offset,
                pos.end.offset,
                chain,
            );
        }
        Node::List(list) => {
            // Each list item is its own block, using the list item's own
            // position (which includes its marker and content).
            for item in &list.children {
                let Node::ListItem(li) = item else {
                    continue;
                };
                let pos = li.position.as_ref().expect("mdast nodes have positions");
                push_block(
                    blocks,
                    BlockKind::ListItem,
                    input,
                    pos.start.offset,
                    pos.end.offset,
                    chain,
                );
            }
        }
        _ => {}
    }
}

/// Compute one block from a byte span of the input and push it. The block text
/// is exactly `input[start..end]`; offsets are byte offsets.
fn push_block(
    blocks: &mut Vec<Block>,
    kind: BlockKind,
    input: &str,
    start: usize,
    end: usize,
    chain: &[String],
) {
    let text = input[start..end].to_string();
    let excluded = matches!(
        kind,
        BlockKind::FrontMatter | BlockKind::CodeFence | BlockKind::Html
    );
    let block_hash = stable_hash(&text);
    blocks.push(Block {
        id: None,
        kind,
        text,
        block_hash,
        heading_path: chain.join(" > "),
        char_start: start,
        char_end: end,
        excluded,
    });
}

/// Rebuild the plain text of a node from its inline children, dropping
/// formatting markers (e.g. "## H2 **bold**" -> "H2 bold").
fn collect_text(node: &markdown::mdast::Node) -> String {
    use markdown::mdast::Node;
    match node {
        Node::Text(t) => t.value.clone(),
        other => other
            .children()
            .map(|kids| kids.iter().map(collect_text).collect::<String>())
            .unwrap_or_default(),
    }
}