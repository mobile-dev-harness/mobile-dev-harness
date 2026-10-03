//! tree-sitter helpers shared by the Kotlin and Java extractors.

use tree_sitter::{Language, Node, Parser, Tree};

/// Kotlin modifiers that are also legal function names; the grammar misparses calls to them
/// (`open(id, screen)`). Not `suspend`: `suspend () -> T` is a valid function type.
const SOFT_KEYWORDS: &[&str] = &[
    "abstract",
    "actual",
    "annotation",
    "companion",
    "const",
    "crossinline",
    "data",
    "enum",
    "expect",
    "external",
    "final",
    "infix",
    "inline",
    "inner",
    "internal",
    "lateinit",
    "noinline",
    "open",
    "operator",
    "out",
    "override",
    "private",
    "protected",
    "public",
    "reified",
    "sealed",
    "tailrec",
    "vararg",
    "value",
];

/// Parses `src`; a Kotlin file with errors is parsed again with soft-keyword calls masked, and the
/// attempt with fewer errors wins. Masking keeps byte offsets, so nodes always index into `src`.
pub fn parse(language: &Language, src: &str, kotlin: bool) -> Option<(Tree, usize)> {
    // A fresh parser per file: some external scanners keep state between parses.
    let mut parser = Parser::new();
    parser.set_language(language).ok()?;
    let tree = parser.parse(src, None)?;
    let errors = error_count(tree.root_node());
    if errors == 0 || !kotlin {
        return Some((tree, errors));
    }
    let masked = mask_soft_keyword_calls(src);
    if masked == src {
        return Some((tree, errors));
    }
    let mut parser = Parser::new();
    parser.set_language(language).ok()?;
    match parser.parse(&masked, None) {
        Some(retry) if error_count(retry.root_node()) < errors => {
            let n = error_count(retry.root_node());
            Some((retry, n))
        }
        _ => Some((tree, errors)),
    }
}

pub fn error_count(node: Node) -> usize {
    if !node.has_error() {
        return 0;
    }
    let own = usize::from(node.is_error() || node.is_missing());
    let mut cursor = node.walk();
    own + node.children(&mut cursor).map(error_count).sum::<usize>()
}

/// Uppercases soft keywords directly followed by `(` (same length, so offsets are unchanged).
fn mask_soft_keyword_calls(src: &str) -> String {
    let bytes = src.as_bytes();
    let mut out = bytes.to_vec();
    let ident = |b: u8| b.is_ascii_alphanumeric() || b == b'_';
    let mut i = 0;
    while i < bytes.len() {
        if !bytes[i].is_ascii_lowercase()
            || (i > 0 && (ident(bytes[i - 1]) || bytes[i - 1] == b'.'))
        {
            i += 1;
            continue;
        }
        let end = (i..bytes.len())
            .find(|&j| !ident(bytes[j]))
            .unwrap_or(bytes.len());
        let word = &src[i..end];
        let mut next = end;
        while next < bytes.len() && (bytes[next] == b' ' || bytes[next] == b'\t') {
            next += 1;
        }
        if next < bytes.len() && bytes[next] == b'(' && SOFT_KEYWORDS.contains(&word) {
            out[i..end].make_ascii_uppercase();
        }
        i = end;
    }
    String::from_utf8(out).unwrap_or_else(|_| src.to_owned())
}

pub fn text<'s>(node: Node, src: &'s str) -> &'s str {
    &src[node.byte_range()]
}

pub fn line(node: Node) -> usize {
    node.start_position().row + 1
}

pub fn named_children<'t>(node: Node<'t>) -> Vec<Node<'t>> {
    let mut cursor = node.walk();
    node.named_children(&mut cursor).collect()
}

pub fn children<'t>(node: Node<'t>) -> Vec<Node<'t>> {
    let mut cursor = node.walk();
    node.children(&mut cursor).collect()
}

pub fn child_of_kind<'t>(node: Node<'t>, kind: &str) -> Option<Node<'t>> {
    let mut cursor = node.walk();
    node.named_children(&mut cursor).find(|c| c.kind() == kind)
}

pub fn is_comment(node: Node) -> bool {
    matches!(
        node.kind(),
        "line_comment" | "block_comment" | "comment" | "multiline_comment"
    )
}

/// Leaf tokens of `node` without comments, joined by single spaces: two versions that differ
/// only in comments or formatting normalize to the same string.
pub fn tokens(node: Node, src: &str, skip: &dyn Fn(Node) -> bool, out: &mut String) {
    if is_comment(node) || skip(node) {
        return;
    }
    if node.child_count() == 0 {
        if !out.is_empty() {
            out.push(' ');
        }
        out.push_str(text(node, src));
        return;
    }
    // Some grammars leave text between child tokens unnamed (XML attribute values): keep it too.
    let mut end = node.start_byte();
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        push_gap(&src[end..child.start_byte()], out);
        end = child.end_byte();
        tokens(child, src, skip, out);
    }
    push_gap(&src[end..node.end_byte()], out);
}

fn push_gap(gap: &str, out: &mut String) {
    for word in gap.split_whitespace() {
        if !out.is_empty() {
            out.push(' ');
        }
        out.push_str(word);
    }
}

pub fn token_hash(node: Node, src: &str, skip: &dyn Fn(Node) -> bool) -> u64 {
    let mut s = String::new();
    tokens(node, src, skip, &mut s);
    hash(&s)
}

/// FNV-1a: stable across runs and platforms.
pub fn hash(s: &str) -> u64 {
    hash_bytes(s.as_bytes())
}

pub fn hash_bytes(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for &b in bytes {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    h
}

/// Source text with runs of whitespace collapsed to one space.
pub fn collapsed(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Android API levels by `Build.VERSION_CODES` name.
const VERSION_CODES: &[(&str, u32)] = &[
    ("KITKAT", 19),
    ("KITKAT_WATCH", 20),
    ("LOLLIPOP", 21),
    ("LOLLIPOP_MR1", 22),
    ("M", 23),
    ("N", 24),
    ("N_MR1", 25),
    ("O", 26),
    ("O_MR1", 27),
    ("P", 28),
    ("Q", 29),
    ("R", 30),
    ("S", 31),
    ("S_V2", 32),
    ("TIRAMISU", 33),
    ("UPSIDE_DOWN_CAKE", 34),
    ("VANILLA_ICE_CREAM", 35),
    ("BAKLAVA", 36),
];

/// The API level an `SDK_INT` comparison or a `@RequiresApi` argument names, as the first level on
/// the newer side: `SDK_INT >= TIRAMISU` and `SDK_INT < 33` → 33, `SDK_INT > 32` → 33.
pub fn api_level(expr: &str) -> Option<u32> {
    let level = |token: &str| -> Option<u32> {
        let name = token.rsplit('.').next().unwrap_or(token);
        name.parse::<u32>()
            .ok()
            .filter(|n| (1..=99).contains(n))
            .or_else(|| {
                VERSION_CODES
                    .iter()
                    .find(|(c, _)| *c == name)
                    .map(|(_, n)| *n)
            })
    };
    let tokens: Vec<&str> = expr
        .split(|c: char| !(c.is_ascii_alphanumeric() || c == '_' || c == '.'))
        .filter(|t| !t.is_empty() && !t.ends_with("SDK_INT"))
        .collect();
    let n = tokens.iter().rev().find_map(|t| level(t))?;
    // `>` and `<=` put the boundary one level up.
    let strict_upper = expr.contains('>') && !expr.contains(">=") || expr.contains("<=");
    let sdk_left = expr.find("SDK_INT").unwrap_or(0) < expr.find(['<', '>']).unwrap_or(usize::MAX);
    // With SDK_INT on the right the operator reads the other way round.
    let bump = if sdk_left {
        strict_upper
    } else {
        expr.contains('<') && !expr.contains("<=") || expr.contains(">=")
    };
    Some(if bump { n + 1 } else { n })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn masks_only_calls_to_soft_keywords() {
        let src = "open(id, x)\nopen class A\nsuspend () -> T\nreopen(1)\nx.open(2)\nvalue (3)";
        let masked = mask_soft_keyword_calls(src);
        assert_eq!(
            masked,
            "OPEN(id, x)\nopen class A\nsuspend () -> T\nreopen(1)\nx.open(2)\nVALUE (3)"
        );
        assert_eq!(masked.len(), src.len());
    }

    #[test]
    fn api_levels_name_the_newer_side() {
        assert_eq!(
            api_level("Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU"),
            Some(33)
        );
        assert_eq!(api_level("SDK_INT < 33"), Some(33));
        assert_eq!(api_level("SDK_INT > 32"), Some(33));
        assert_eq!(api_level("SDK_INT <= 32"), Some(33));
        assert_eq!(api_level("33 <= SDK_INT"), Some(33));
        assert_eq!(api_level("32 < SDK_INT"), Some(33));
        assert_eq!(api_level("SDK_INT == Build.VERSION_CODES.S_V2"), Some(32));
        assert_eq!(
            api_level("@RequiresApi(api = Build.VERSION_CODES.UPSIDE_DOWN_CAKE)"),
            Some(34)
        );
        assert_eq!(api_level("SDK_INT >= minimum"), None);
    }
}
