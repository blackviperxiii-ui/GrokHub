//! Tree-sitter repository map with cross-file symbol ranking and a token budget.
//!
//! Walks the workspace (respecting `.gitignore`), parses supported source files
//! with tree-sitter, ranks symbols using cross-file reference Personalized
//! PageRank plus prompt focus/query signals, and packs the top-ranked symbols
//! within a strict token budget for local coding turns and the `repo_map` tool.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::Path;

use ignore::WalkBuilder;
use tree_sitter::{Language, Node, Parser};

/// Switch file in the config directory (`enabled = false` turns auto-injection off).
pub const CONFIG_NAME: &str = "repo_map.toml";

/// Default token budget for automatic local coding turn injection.
pub const DEFAULT_TOKEN_BUDGET: usize = 1_024;
/// Tighter token budget for compact local context windows (`<= 8192`).
pub const COMPACT_TOKEN_BUDGET: usize = 512;
pub const MIN_TOKEN_BUDGET: usize = 64;
pub const MAX_TOKEN_BUDGET: usize = 8_192;
const DEFAULT_MAX_FILES: usize = 400;
const MAX_FILE_BYTES: u64 = 512 * 1024;
const MAX_SIG_CHARS: usize = 120;

/// Whether automatic repo-map injection is enabled in `config_dir`.
/// Defaults to `true` when `repo_map.toml` is absent.
pub fn enabled(config_dir: &Path) -> bool {
    let Ok(text) = std::fs::read_to_string(config_dir.join(CONFIG_NAME)) else {
        return true;
    };
    let file = crate::config_file::parse(CONFIG_NAME, &text, &["enabled"]);
    file.flag("enabled", true)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum SymbolKind {
    Function,
    Method,
    Struct,
    Enum,
    Trait,
    Impl,
    Class,
    Interface,
    Type,
    Const,
    Module,
}

impl SymbolKind {
    fn kind_bonus(self) -> f64 {
        match self {
            Self::Struct | Self::Trait | Self::Interface | Self::Class | Self::Enum => 0.35,
            Self::Function | Self::Method => 0.25,
            Self::Type => 0.15,
            Self::Const | Self::Module | Self::Impl => 0.10,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Symbol {
    pub name: String,
    pub kind: SymbolKind,
    pub signature: String,
    pub file: String,
    pub line: usize,
    pub score: f64,
    pub exported: bool,
}

#[derive(Debug, Clone)]
pub struct RepoMapConfig {
    pub token_budget: usize,
    pub focus_files: Vec<String>,
    pub query_terms: Vec<String>,
    pub max_files: usize,
}

impl Default for RepoMapConfig {
    fn default() -> Self {
        Self {
            token_budget: DEFAULT_TOKEN_BUDGET,
            focus_files: Vec::new(),
            query_terms: Vec::new(),
            max_files: DEFAULT_MAX_FILES,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct RepoMap {
    pub rendered: String,
    pub symbols: Vec<Symbol>,
    pub files_scanned: usize,
    pub symbols_total: usize,
    pub symbols_included: usize,
    pub estimated_tokens: usize,
    pub budget_tokens: usize,
    pub truncated: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Lang {
    Rust,
    TypeScript,
    Tsx,
    JavaScript,
    Python,
    Go,
}

impl Lang {
    fn from_path(path: &Path) -> Option<Self> {
        let ext = path.extension()?.to_str()?.to_ascii_lowercase();
        match ext.as_str() {
            "rs" => Some(Self::Rust),
            "ts" => Some(Self::TypeScript),
            "tsx" => Some(Self::Tsx),
            "js" | "jsx" | "mjs" | "cjs" => Some(Self::JavaScript),
            "py" => Some(Self::Python),
            "go" => Some(Self::Go),
            _ => None,
        }
    }

    fn ts_language(self) -> Language {
        match self {
            Self::Rust => tree_sitter_rust::LANGUAGE.into(),
            Self::TypeScript => tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
            Self::Tsx => tree_sitter_typescript::LANGUAGE_TSX.into(),
            Self::JavaScript => tree_sitter_javascript::LANGUAGE.into(),
            Self::Python => tree_sitter_python::LANGUAGE.into(),
            Self::Go => tree_sitter_go::LANGUAGE.into(),
        }
    }
}

#[derive(Debug, Clone)]
struct RawSymbol {
    name: String,
    kind: SymbolKind,
    signature: String,
    line: usize,
    exported: bool,
}

#[derive(Debug)]
struct ParsedFile {
    rel_path: String,
    defs: Vec<RawSymbol>,
    refs: HashSet<String>,
}

/// Deterministic byte-based token estimate (~4 UTF-8 bytes per token).
pub fn estimate_tokens(text: &str) -> usize {
    text.len().div_ceil(4)
}

/// Extract path-like `focus_files` and identifier `query_terms` from a prompt.
pub fn extract_prompt_hints(text: &str) -> (Vec<String>, Vec<String>) {
    let mut focus_files = Vec::new();
    let mut query_terms = Vec::new();
    let mut seen_files = HashSet::new();
    let mut seen_terms = HashSet::new();

    for raw in text.split(|c: char| {
        c.is_whitespace() || matches!(c, ',' | ';' | '(' | ')' | '[' | ']' | '{' | '}' | '"' | '\'' | '`')
    }) {
        let tok = raw.trim_matches(|c: char| matches!(c, '.' | ':' | '<' | '>' | '!'));
        if tok.is_empty() {
            continue;
        }
        let normalized = tok.replace('\\', "/");
        if (normalized.contains('/')
            || normalized.ends_with(".rs")
            || normalized.ends_with(".ts")
            || normalized.ends_with(".tsx")
            || normalized.ends_with(".js")
            || normalized.ends_with(".jsx")
            || normalized.ends_with(".py")
            || normalized.ends_with(".go"))
            && seen_files.insert(normalized.clone())
        {
            focus_files.push(normalized);
            continue;
        }
        for part in tok.split(|c: char| !c.is_ascii_alphanumeric() && c != '_') {
            if part.len() < 3 || is_stop_word(part) {
                continue;
            }
            if part.chars().all(|c| c.is_ascii_digit()) {
                continue;
            }
            let lower = part.to_ascii_lowercase();
            if seen_terms.insert(lower) {
                query_terms.push(part.to_string());
            }
        }
    }

    (focus_files, query_terms)
}

fn is_stop_word(word: &str) -> bool {
    matches!(
        word.to_ascii_lowercase().as_str(),
        "the"
            | "and"
            | "for"
            | "with"
            | "that"
            | "this"
            | "from"
            | "into"
            | "only"
            | "make"
            | "pass"
            | "test"
            | "tests"
            | "leave"
            | "change"
            | "file"
            | "files"
            | "code"
            | "fix"
            | "add"
            | "update"
            | "check"
            | "when"
            | "what"
            | "where"
            | "which"
            | "should"
            | "would"
            | "could"
            | "have"
            | "will"
    )
}

/// Build a ranked tree-sitter repository map for `root`.
pub fn build(root: &Path, config: &RepoMapConfig) -> RepoMap {
    let budget_tokens = config
        .token_budget
        .clamp(MIN_TOKEN_BUDGET, MAX_TOKEN_BUDGET);
    let max_files = config.max_files.max(1);
    let candidates = collect_source_files(root, max_files);

    let mut parsed_files = Vec::new();
    let mut parsers: HashMap<u8, Parser> = HashMap::new();

    for (rel_path, abs_path, lang) in candidates {
        let Ok(src) = std::fs::read_to_string(&abs_path) else {
            continue;
        };
        let parser = parsers.entry(lang as u8).or_insert_with(|| {
            let mut p = Parser::new();
            let _ = p.set_language(&lang.ts_language());
            p
        });
        if let Some(parsed) = parse_source(parser, &rel_path, &src, lang) {
            parsed_files.push(parsed);
        }
    }

    let files_scanned = parsed_files.len();
    let ranked = rank_symbols(&parsed_files, &config.focus_files, &config.query_terms);
    let symbols_total = ranked.len();

    if ranked.is_empty() {
        return RepoMap {
            rendered: String::new(),
            symbols: ranked,
            files_scanned,
            symbols_total: 0,
            symbols_included: 0,
            estimated_tokens: 0,
            budget_tokens,
            truncated: false,
        };
    }

    let (rendered, symbols_included) = render_within_budget(&ranked, budget_tokens);
    let estimated_tokens = estimate_tokens(&rendered);
    let truncated = symbols_included < symbols_total;

    RepoMap {
        rendered,
        symbols: ranked,
        files_scanned,
        symbols_total,
        symbols_included,
        estimated_tokens,
        budget_tokens,
        truncated,
    }
}

fn collect_source_files(root: &Path, max_files: usize) -> Vec<(String, std::path::PathBuf, Lang)> {
    let mut out = Vec::new();
    let mut builder = WalkBuilder::new(root);
    builder
        .hidden(true)
        .git_ignore(true)
        .git_global(true)
        .git_exclude(true)
        .filter_entry(|entry| {
            let name = entry.file_name().to_string_lossy();
            !matches!(
                name.as_ref(),
                "target" | "node_modules" | ".git" | "dist" | "build" | "__pycache__" | ".venv"
            )
        });

    for entry in builder.build().flatten() {
        if !entry.file_type().is_some_and(|ft| ft.is_file()) {
            continue;
        }
        let path = entry.path();
        let Some(lang) = Lang::from_path(path) else {
            continue;
        };
        if let Ok(meta) = entry.metadata()
            && meta.len() > MAX_FILE_BYTES
        {
            continue;
        }
        let rel = path
            .strip_prefix(root)
            .unwrap_or(path)
            .to_string_lossy()
            .replace('\\', "/");
        out.push((rel, path.to_path_buf(), lang));
    }

    out.sort_by(|a, b| a.0.cmp(&b.0));
    out.truncate(max_files);
    out
}

fn parse_source(parser: &mut Parser, rel_path: &str, src: &str, lang: Lang) -> Option<ParsedFile> {
    let tree = parser.parse(src, None)?;
    let root = tree.root_node();
    let bytes = src.as_bytes();

    let mut defs = Vec::new();
    let mut def_name_ranges: HashSet<(usize, usize)> = HashSet::new();
    let mut refs: HashSet<String> = HashSet::new();

    // First pass (iterative pre-order): collect definitions and identifier nodes.
    let mut ident_nodes: Vec<(usize, usize, String)> = Vec::new();
    let mut stack = vec![root];

    while let Some(node) = stack.pop() {
        if let Some((sym, name_range)) = extract_definition(node, bytes, lang) {
            if let Some(range) = name_range {
                def_name_ranges.insert(range);
            }
            defs.push(sym);
        }

        if is_ref_identifier_kind(node.kind())
            && let Ok(text) = node.utf8_text(bytes)
        {
            let trimmed = text.trim();
            if !trimmed.is_empty() && !is_primitive_or_keyword(trimmed) {
                ident_nodes.push((node.start_byte(), node.end_byte(), trimmed.to_string()));
            }
        }

        let count = node.child_count();
        for i in (0..count).rev() {
            if let Some(child) = node.child(i) {
                stack.push(child);
            }
        }
    }

    for (start, end, ident) in ident_nodes {
        if !def_name_ranges.contains(&(start, end)) {
            refs.insert(ident);
        }
    }

    defs.sort_by_key(|d| d.line);

    Some(ParsedFile {
        rel_path: rel_path.to_string(),
        defs,
        refs,
    })
}

fn is_ref_identifier_kind(kind: &str) -> bool {
    matches!(
        kind,
        "identifier"
            | "type_identifier"
            | "field_identifier"
            | "property_identifier"
            | "shorthand_field_identifier"
    )
}

fn is_primitive_or_keyword(ident: &str) -> bool {
    matches!(
        ident,
        "self"
            | "Self"
            | "super"
            | "crate"
            | "bool"
            | "char"
            | "str"
            | "String"
            | "Vec"
            | "Option"
            | "Result"
            | "Box"
            | "Arc"
            | "Rc"
            | "u8"
            | "u16"
            | "u32"
            | "u64"
            | "usize"
            | "i8"
            | "i16"
            | "i32"
            | "i64"
            | "isize"
            | "f32"
            | "f64"
            | "true"
            | "false"
            | "None"
            | "Some"
            | "Ok"
            | "Err"
            | "int"
            | "float"
            | "list"
            | "dict"
            | "set"
            | "tuple"
            | "any"
            | "void"
            | "number"
            | "string"
            | "boolean"
            | "object"
            | "error"
            | "nil"
    )
}

fn extract_definition(
    node: Node<'_>,
    bytes: &[u8],
    lang: Lang,
) -> Option<(RawSymbol, Option<(usize, usize)>)> {
    match lang {
        Lang::Rust => extract_rust_def(node, bytes),
        Lang::TypeScript | Lang::Tsx | Lang::JavaScript => extract_js_ts_def(node, bytes),
        Lang::Python => extract_python_def(node, bytes),
        Lang::Go => extract_go_def(node, bytes),
    }
}

fn extract_rust_def(node: Node<'_>, bytes: &[u8]) -> Option<(RawSymbol, Option<(usize, usize)>)> {
    let kind = match node.kind() {
        "function_item" => {
            if has_ancestor_kind(node, "impl_item") || has_ancestor_kind(node, "trait_item") {
                SymbolKind::Method
            } else {
                SymbolKind::Function
            }
        }
        "function_signature_item" => SymbolKind::Method,
        "struct_item" => SymbolKind::Struct,
        "enum_item" => SymbolKind::Enum,
        "trait_item" => SymbolKind::Trait,
        "impl_item" => SymbolKind::Impl,
        "type_item" => SymbolKind::Type,
        "const_item" | "static_item" => SymbolKind::Const,
        "mod_item" => SymbolKind::Module,
        _ => return None,
    };

    let (name, range) = if kind == SymbolKind::Impl {
        let ty = node
            .child_by_field_name("type")
            .and_then(|n| n.utf8_text(bytes).ok())
            .map(str::trim)
            .unwrap_or("impl");
        let name = if let Some(tr) = node
            .child_by_field_name("trait")
            .and_then(|n| n.utf8_text(bytes).ok())
            .map(str::trim)
        {
            format!("{tr} for {ty}")
        } else {
            ty.to_string()
        };
        (name, None)
    } else {
        let name_node = node.child_by_field_name("name")?;
        let name = name_node.utf8_text(bytes).ok()?.trim().to_string();
        (name, Some((name_node.start_byte(), name_node.end_byte())))
    };

    if name.is_empty() {
        return None;
    }

    let sig = signature_before_body(node, bytes, &["{", ";"]);
    let exported = has_child_kind(node, "visibility_modifier");
    let line = node.start_position().row + 1;

    Some((
        RawSymbol {
            name,
            kind,
            signature: sig,
            line,
            exported,
        },
        range,
    ))
}

fn extract_js_ts_def(node: Node<'_>, bytes: &[u8]) -> Option<(RawSymbol, Option<(usize, usize)>)> {
    let kind = match node.kind() {
        "function_declaration" | "generator_function_declaration" => SymbolKind::Function,
        "method_definition" | "method_signature" => SymbolKind::Method,
        "class_declaration" => SymbolKind::Class,
        "interface_declaration" => SymbolKind::Interface,
        "type_alias_declaration" => SymbolKind::Type,
        "enum_declaration" => SymbolKind::Enum,
        _ => return None,
    };

    let name_node = node.child_by_field_name("name")?;
    let name = name_node.utf8_text(bytes).ok()?.trim().to_string();
    if name.is_empty() {
        return None;
    }
    let exported = node
        .parent()
        .is_some_and(|p| p.kind() == "export_statement");
    let sig = signature_before_body(node, bytes, &["{", ";"]);
    let line = node.start_position().row + 1;

    Some((
        RawSymbol {
            name,
            kind,
            signature: sig,
            line,
            exported,
        },
        Some((name_node.start_byte(), name_node.end_byte())),
    ))
}

fn extract_python_def(node: Node<'_>, bytes: &[u8]) -> Option<(RawSymbol, Option<(usize, usize)>)> {
    let kind = match node.kind() {
        "function_definition" => {
            if has_ancestor_kind(node, "class_definition") {
                SymbolKind::Method
            } else {
                SymbolKind::Function
            }
        }
        "class_definition" => SymbolKind::Class,
        _ => return None,
    };

    let name_node = node.child_by_field_name("name")?;
    let name = name_node.utf8_text(bytes).ok()?.trim().to_string();
    if name.is_empty() {
        return None;
    }
    let exported = !name.starts_with('_');
    let sig = signature_before_body(node, bytes, &[":\n", ":\r\n"]);
    let sig = sig.trim_end_matches(':').trim().to_string();
    let line = node.start_position().row + 1;

    Some((
        RawSymbol {
            name,
            kind,
            signature: sig,
            line,
            exported,
        },
        Some((name_node.start_byte(), name_node.end_byte())),
    ))
}

fn extract_go_def(node: Node<'_>, bytes: &[u8]) -> Option<(RawSymbol, Option<(usize, usize)>)> {
    match node.kind() {
        "function_declaration" => {
            let name_node = node.child_by_field_name("name")?;
            let name = name_node.utf8_text(bytes).ok()?.trim().to_string();
            if name.is_empty() {
                return None;
            }
            let exported = name.chars().next().is_some_and(|c| c.is_ascii_uppercase());
            Some((
                RawSymbol {
                    name,
                    kind: SymbolKind::Function,
                    signature: signature_before_body(node, bytes, &["{"]),
                    line: node.start_position().row + 1,
                    exported,
                },
                Some((name_node.start_byte(), name_node.end_byte())),
            ))
        }
        "method_declaration" => {
            let name_node = node.child_by_field_name("name")?;
            let name = name_node.utf8_text(bytes).ok()?.trim().to_string();
            if name.is_empty() {
                return None;
            }
            let exported = name.chars().next().is_some_and(|c| c.is_ascii_uppercase());
            Some((
                RawSymbol {
                    name,
                    kind: SymbolKind::Method,
                    signature: signature_before_body(node, bytes, &["{"]),
                    line: node.start_position().row + 1,
                    exported,
                },
                Some((name_node.start_byte(), name_node.end_byte())),
            ))
        }
        "type_spec" => {
            let name_node = node.child_by_field_name("name")?;
            let name = name_node.utf8_text(bytes).ok()?.trim().to_string();
            if name.is_empty() {
                return None;
            }
            let kind = match node.child_by_field_name("type").map(|n| n.kind()) {
                Some("struct_type") => SymbolKind::Struct,
                Some("interface_type") => SymbolKind::Interface,
                _ => SymbolKind::Type,
            };
            let exported = name.chars().next().is_some_and(|c| c.is_ascii_uppercase());
            Some((
                RawSymbol {
                    name,
                    kind,
                    signature: format!("type {}", signature_before_body(node, bytes, &["{"])),
                    line: node.start_position().row + 1,
                    exported,
                },
                Some((name_node.start_byte(), name_node.end_byte())),
            ))
        }
        _ => None,
    }
}

fn has_ancestor_kind(node: Node<'_>, target: &str) -> bool {
    let mut cur = node.parent();
    while let Some(p) = cur {
        if p.kind() == target {
            return true;
        }
        cur = p.parent();
    }
    false
}

fn has_child_kind(node: Node<'_>, target: &str) -> bool {
    for i in 0..node.child_count() {
        if node.child(i).is_some_and(|c| c.kind() == target) {
            return true;
        }
    }
    false
}

fn signature_before_body(node: Node<'_>, bytes: &[u8], stops: &[&str]) -> String {
    let Ok(raw) = node.utf8_text(bytes) else {
        return String::new();
    };
    let mut cut = raw.len();
    for stop in stops {
        if let Some(pos) = raw.find(stop) {
            cut = cut.min(pos);
        }
    }
    let slice = raw[..cut].trim();
    let collapsed: String = slice.split_whitespace().collect::<Vec<_>>().join(" ");
    truncate_chars(&collapsed, MAX_SIG_CHARS)
}

fn truncate_chars(s: &str, max_chars: usize) -> String {
    match s.char_indices().nth(max_chars) {
        Some((idx, _)) => format!("{}...", s[..idx].trim_end()),
        None => s.to_string(),
    }
}

fn rank_symbols(
    files: &[ParsedFile],
    focus_files: &[String],
    query_terms: &[String],
) -> Vec<Symbol> {
    if files.is_empty() {
        return Vec::new();
    }

    let n = files.len();
    let mut symbol_to_files: HashMap<&str, Vec<usize>> = HashMap::new();
    for (idx, file) in files.iter().enumerate() {
        for sym in &file.defs {
            let entry = symbol_to_files.entry(sym.name.as_str()).or_default();
            if entry.last().copied() != Some(idx) {
                entry.push(idx);
            }
        }
    }

    // Cross-file reference edges: u -> v weighted by distinct symbols in v referenced by u.
    let mut edges: Vec<Vec<(usize, f64)>> = vec![Vec::new(); n];
    let mut symbol_ref_counts: HashMap<(usize, &str), usize> = HashMap::new();

    for (u, file) in files.iter().enumerate() {
        let mut to_counts: BTreeMap<usize, usize> = BTreeMap::new();
        for ident in &file.refs {
            let Some(def_files) = symbol_to_files.get(ident.as_str()) else {
                continue;
            };
            for &v in def_files {
                if v != u {
                    *to_counts.entry(v).or_insert(0) += 1;
                    *symbol_ref_counts.entry((v, ident.as_str())).or_insert(0) += 1;
                }
            }
        }
        edges[u] = to_counts.into_iter().map(|(v, c)| (v, c as f64)).collect();
    }

    // Personalization vector over files.
    let lower_queries: Vec<String> = query_terms
        .iter()
        .map(|q| q.to_ascii_lowercase())
        .filter(|q| !q.is_empty())
        .collect();

    let mut personalization = vec![1.0_f64; n];
    let mut file_is_focused = vec![false; n];
    for (idx, file) in files.iter().enumerate() {
        if matches_focus(&file.rel_path, focus_files) {
            personalization[idx] += 8.0;
            file_is_focused[idx] = true;
        }
        let lower_path = file.rel_path.to_ascii_lowercase();
        for q in &lower_queries {
            if lower_path.contains(q.as_str()) {
                personalization[idx] += 3.0;
            }
            if file
                .defs
                .iter()
                .any(|d| d.name.to_ascii_lowercase().contains(q.as_str()))
            {
                personalization[idx] += 3.0;
            }
        }
    }
    let p_sum: f64 = personalization.iter().sum();
    if p_sum > 0.0 {
        for p in &mut personalization {
            *p /= p_sum;
        }
    }

    // Deterministic Personalized PageRank (20 iterations, d = 0.85).
    let d = 0.85_f64;
    let mut rank = personalization.clone();
    let out_weights: Vec<f64> = edges
        .iter()
        .map(|adj| adj.iter().map(|&(_, w)| w).sum())
        .collect();

    for _ in 0..20 {
        let mut next = vec![0.0_f64; n];
        let mut dangling_mass = 0.0_f64;
        for u in 0..n {
            let total_w = out_weights[u];
            if total_w <= 0.0 {
                dangling_mass += rank[u];
            } else {
                let share = d * rank[u] / total_w;
                for &(v, w) in &edges[u] {
                    next[v] += share * w;
                }
            }
        }
        for v in 0..n {
            next[v] += (1.0 - d + d * dangling_mass) * personalization[v];
        }
        rank = next;
    }

    let mut out = Vec::new();
    for (file_idx, file) in files.iter().enumerate() {
        let base_rank = rank[file_idx] * 100.0;
        let focus_bonus = if file_is_focused[file_idx] { 10.0 } else { 0.0 };

        for sym in &file.defs {
            let ref_count = symbol_ref_counts
                .get(&(file_idx, sym.name.as_str()))
                .copied()
                .unwrap_or(0);
            let ref_bonus = (ref_count as f64) * 2.5;
            let export_bonus = if sym.exported { 0.15 } else { 0.0 };
            let lower_name = sym.name.to_ascii_lowercase();
            let mut query_bonus = 0.0;
            for q in &lower_queries {
                if lower_name == *q {
                    query_bonus += 9.0;
                } else if lower_name.contains(q.as_str()) {
                    query_bonus += 6.0;
                }
            }
            let score = base_rank
                + ref_bonus
                + sym.kind.kind_bonus()
                + export_bonus
                + focus_bonus
                + query_bonus;

            out.push(Symbol {
                name: sym.name.clone(),
                kind: sym.kind,
                signature: sym.signature.clone(),
                file: file.rel_path.clone(),
                line: sym.line,
                score,
                exported: sym.exported,
            });
        }
    }

    out.sort_by(|a, b| {
        b.score
            .total_cmp(&a.score)
            .then_with(|| a.file.cmp(&b.file))
            .then_with(|| a.line.cmp(&b.line))
            .then_with(|| a.name.cmp(&b.name))
    });

    out
}

fn matches_focus(rel_path: &str, focus_files: &[String]) -> bool {
    let norm = rel_path.trim_start_matches("./").to_ascii_lowercase();
    focus_files.iter().any(|f| {
        let f_norm = f
            .replace('\\', "/")
            .trim_start_matches("./")
            .to_ascii_lowercase();
        !f_norm.is_empty() && (norm == f_norm || norm.ends_with(&format!("/{f_norm}")))
    })
}

fn render_within_budget(ranked: &[Symbol], budget_tokens: usize) -> (String, usize) {
    let budget_bytes = budget_tokens.saturating_mul(4);
    let mut chosen: Vec<&Symbol> = Vec::new();
    let mut file_order: Vec<&str> = Vec::new();
    let mut by_file: HashMap<&str, Vec<&Symbol>> = HashMap::new();

    for sym in ranked {
        // Tentatively add this symbol and check rendered size against budget.
        let is_new_file = !by_file.contains_key(sym.file.as_str());
        if is_new_file {
            file_order.push(sym.file.as_str());
        }
        by_file.entry(sym.file.as_str()).or_default().push(sym);
        chosen.push(sym);

        let body_len = estimate_body_bytes(&file_order, &by_file);
        // Reserve ~88 bytes for `<repo_map ...>\n` and `</repo_map>`.
        if body_len + 88 > budget_bytes && chosen.len() > 1 {
            // Undo last addition.
            chosen.pop();
            if let Some(vec) = by_file.get_mut(sym.file.as_str()) {
                vec.pop();
                if vec.is_empty() {
                    by_file.remove(sym.file.as_str());
                    file_order.pop();
                }
            }
            break;
        }
    }

    // Final exact verification & trim if header digits pushed slightly over budget.
    loop {
        let rendered = format_map(&file_order, &by_file, chosen.len(), ranked.len(), budget_tokens);
        if estimate_tokens(&rendered) <= budget_tokens || chosen.len() <= 1 {
            return (rendered, chosen.len());
        }
        let Some(removed) = chosen.pop() else {
            return (String::new(), 0);
        };
        if let Some(vec) = by_file.get_mut(removed.file.as_str()) {
            vec.pop();
            if vec.is_empty() {
                by_file.remove(removed.file.as_str());
                file_order.retain(|&f| f != removed.file.as_str());
            }
        }
    }
}

fn estimate_body_bytes(file_order: &[&str], by_file: &HashMap<&str, Vec<&Symbol>>) -> usize {
    let mut bytes = 0usize;
    for file in file_order {
        // `<file>:\n`
        bytes += file.len() + 2;
        if let Some(syms) = by_file.get(file) {
            for s in syms {
                // `  <line>| <signature>\n`
                bytes += 4 + digits(s.line) + s.signature.len() + 1;
            }
        }
    }
    bytes
}

fn digits(mut n: usize) -> usize {
    let mut d = 1;
    while n >= 10 {
        n /= 10;
        d += 1;
    }
    d
}

fn format_map(
    file_order: &[&str],
    by_file: &HashMap<&str, Vec<&Symbol>>,
    included: usize,
    total: usize,
    budget_tokens: usize,
) -> String {
    let mut body = String::new();
    for file in file_order {
        let Some(syms) = by_file.get(file) else {
            continue;
        };
        let mut sorted = syms.clone();
        sorted.sort_by_key(|s| s.line);
        body.push_str(file);
        body.push_str(":\n");
        for sym in sorted {
            body.push_str("  ");
            body.push_str(&sym.line.to_string());
            body.push_str("| ");
            body.push_str(&sym.signature);
            body.push('\n');
        }
    }
    // Compute exact header with final token count.
    let approx_header = format!(
        "<repo_map tokens=\"{}/{}\" files=\"{}\" symbols=\"{}/{}\">\n",
        budget_tokens,
        budget_tokens,
        file_order.len(),
        included,
        total
    );
    let total_bytes = approx_header.len() + body.len() + "</repo_map>".len();
    let used_tokens = total_bytes.div_ceil(4);
    format!(
        "<repo_map tokens=\"{used_tokens}/{budget_tokens}\" files=\"{}\" symbols=\"{included}/{total}\">\n{body}</repo_map>",
        file_order.len()
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_ws(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "grokhub-repomap-{tag}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn write_file(root: &Path, rel: &str, content: &str) {
        let path = root.join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, content).unwrap();
    }

    #[test]
    fn extracts_symbols_across_rust_ts_python_and_go() {
        let ws = temp_ws("langs");
        write_file(
            &ws,
            "src/lib.rs",
            "pub struct Engine { x: i32 }\nimpl Engine {\n    pub fn run(&self) -> i32 { self.x }\n}\npub fn helper() {}\n",
        );
        write_file(
            &ws,
            "web/app.ts",
            "export interface User { id: string }\nexport function greet(u: User): string { return u.id; }\n",
        );
        write_file(
            &ws,
            "scripts/worker.py",
            "class JobRunner:\n    def execute(self):\n        pass\ndef standalone():\n    return 1\n",
        );
        write_file(
            &ws,
            "cmd/main.go",
            "package main\ntype Server struct { port int }\nfunc StartServer() {}\n",
        );

        let map = build(
            &ws,
            &RepoMapConfig {
                token_budget: 2_048,
                ..Default::default()
            },
        );
        assert_eq!(map.files_scanned, 4);
        assert!(!map.truncated);
        assert!(map.estimated_tokens <= 2_048);

        let pairs: Vec<(&str, SymbolKind, usize)> = map
            .symbols
            .iter()
            .map(|s| (s.name.as_str(), s.kind, s.line))
            .collect();
        assert!(pairs.contains(&("Engine", SymbolKind::Struct, 1)), "{pairs:?}");
        assert!(pairs.contains(&("run", SymbolKind::Method, 3)), "{pairs:?}");
        assert!(pairs.contains(&("helper", SymbolKind::Function, 5)), "{pairs:?}");
        assert!(pairs.contains(&("User", SymbolKind::Interface, 1)), "{pairs:?}");
        assert!(pairs.contains(&("greet", SymbolKind::Function, 2)), "{pairs:?}");
        assert!(pairs.contains(&("JobRunner", SymbolKind::Class, 1)), "{pairs:?}");
        assert!(pairs.contains(&("execute", SymbolKind::Method, 2)), "{pairs:?}");
        assert!(pairs.contains(&("standalone", SymbolKind::Function, 4)), "{pairs:?}");
        assert!(pairs.contains(&("Server", SymbolKind::Struct, 2)), "{pairs:?}");
        assert!(pairs.contains(&("StartServer", SymbolKind::Function, 3)), "{pairs:?}");

        let _ = std::fs::remove_dir_all(&ws);
    }

    #[test]
    fn literal_symbol_rankings_prefer_most_referenced_and_flip_on_focus() {
        let ws = temp_ws("rank");
        write_file(&ws, "src/core.rs", "pub fn hot_symbol() -> i32 { 1 }\n");
        write_file(&ws, "src/mid.rs", "pub fn warm_symbol() -> i32 { 2 }\n");
        write_file(&ws, "src/cold.rs", "pub fn unused_helper() -> i32 { 3 }\n");
        write_file(
            &ws,
            "src/a.rs",
            "use crate::core::hot_symbol;\nuse crate::mid::warm_symbol;\npub fn caller_a() -> i32 { hot_symbol() + warm_symbol() }\n",
        );
        write_file(
            &ws,
            "src/b.rs",
            "use crate::core::hot_symbol;\npub fn caller_b() -> i32 { hot_symbol() }\n",
        );
        write_file(
            &ws,
            "src/c.rs",
            "use crate::core::hot_symbol;\npub fn caller_c() -> i32 { hot_symbol() }\n",
        );

        let map = build(
            &ws,
            &RepoMapConfig {
                token_budget: 2_048,
                ..Default::default()
            },
        );
        let names: Vec<&str> = map.symbols.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(
            names,
            vec![
                "hot_symbol",
                "warm_symbol",
                "caller_a",
                "caller_b",
                "caller_c",
                "unused_helper",
            ]
        );

        // Focusing on cold.rs + querying unused_helper promotes it deterministically to #1.
        let focused = build(
            &ws,
            &RepoMapConfig {
                token_budget: 2_048,
                focus_files: vec!["src/cold.rs".into()],
                query_terms: vec!["unused_helper".into()],
                ..Default::default()
            },
        );
        let focused_names: Vec<&str> = focused.symbols.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(focused_names[0], "unused_helper", "{focused_names:?}");
        assert_eq!(focused_names[1], "hot_symbol", "{focused_names:?}");

        let _ = std::fs::remove_dir_all(&ws);
    }

    #[test]
    fn enforces_strict_token_budget_and_truncates_low_rank_symbols() {
        let ws = temp_ws("budget");
        write_file(
            &ws,
            "src/hub.rs",
            "pub fn critical_api_entrypoint(alpha: usize, beta: usize) -> usize { alpha + beta }\n",
        );
        write_file(
            &ws,
            "src/c1.rs",
            "use crate::hub::critical_api_entrypoint;\npub fn consumer_one() -> usize { critical_api_entrypoint(1, 2) }\n",
        );
        write_file(
            &ws,
            "src/c2.rs",
            "use crate::hub::critical_api_entrypoint;\npub fn consumer_two() -> usize { critical_api_entrypoint(3, 4) }\n",
        );
        write_file(
            &ws,
            "src/noise.rs",
            "pub fn obscure_one() {}\npub fn obscure_two() {}\npub fn obscure_three() {}\npub fn obscure_four() {}\n",
        );

        let map = build(
            &ws,
            &RepoMapConfig {
                token_budget: 64,
                ..Default::default()
            },
        );
        assert!(map.truncated, "expected tight budget to truncate");
        assert!(
            map.estimated_tokens <= 64,
            "budget exceeded: {}",
            map.estimated_tokens
        );
        assert!(map.symbols_included < map.symbols_total);
        assert!(
            map.rendered.contains("critical_api_entrypoint"),
            "{}",
            map.rendered
        );
        assert!(!map.rendered.contains("obscure_four"), "{}", map.rendered);

        let _ = std::fs::remove_dir_all(&ws);
    }

    #[test]
    fn extracts_prompt_hints_and_reads_config_flag() {
        let (files, terms) =
            extract_prompt_hints("Fix `calculate_total` in src/billing/tax.rs and crates\\core\\lib.rs");
        assert_eq!(files, vec!["src/billing/tax.rs", "crates/core/lib.rs"]);
        assert!(terms.contains(&"calculate_total".to_string()), "{terms:?}");

        let cfg = temp_ws("cfg");
        assert!(enabled(&cfg));
        std::fs::write(cfg.join(CONFIG_NAME), "enabled = false\n").unwrap();
        assert!(!enabled(&cfg));
        std::fs::write(cfg.join(CONFIG_NAME), "enabled = true\n").unwrap();
        assert!(enabled(&cfg));
        let _ = std::fs::remove_dir_all(&cfg);
    }
}
