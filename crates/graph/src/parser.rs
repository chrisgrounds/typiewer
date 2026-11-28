use std::fs;
use std::path::{Path, PathBuf};

use lsp::Position;
use macros::performance;
use tree_sitter::{Node, Parser};

use crate::model::{
  FunctionId, FunctionInfo, TypeId, TypeKind, TypeNode, TypeRefSite, canonicalize_or_keep,
};

#[derive(Debug)]
pub struct ParsedWorkspace {
  pub types: Vec<TypeNode>,
  pub functions: Vec<FunctionInfo>,
}

pub fn parse_workspace(root: &Path, ignore: &[String]) -> Result<ParsedWorkspace, String> {
  let mut parser = Parser::new();
  parser
    .set_language(&tree_sitter_rust::LANGUAGE.into())
    .map_err(|err| format!("Failed to load Rust grammar: {err}"))?;

  let normalized_ignore = normalize_ignore_entries(ignore);
  let files = performance!(
    collect_source_files(root, &normalized_ignore),
    "collect source files"
  );

  let trees: Vec<_> = performance!(
    files
      .into_iter()
      .filter_map(|(file_path, f)| parser.parse(&f, None).map(|tree| (file_path, tree, f)))
      .collect(),
    "tree-sitter: parse files into ASTs"
  );

  let mut types = Vec::new();
  let mut functions = Vec::new();

  performance!(
    for (file_path, tree, source) in &trees {
      walk_tree(
        tree.root_node(),
        source,
        file_path,
        None,
        &mut types,
        &mut functions,
      );
    },
    "tree-sitter: walk_tree"
  );

  Ok(ParsedWorkspace { types, functions })
}

fn normalize_ignore_entries(raw: &[String]) -> Vec<String> {
  raw
    .iter()
    .map(|line| line.trim())
    .filter(|line| !line.is_empty() && !line.starts_with('#'))
    .map(|line| line.trim_matches('/').to_owned())
    .filter(|line| !line.is_empty())
    .collect()
}

fn dedent_body(raw: &str, start_col: usize) -> String {
  if start_col == 0 {
    return raw.to_string();
  }

  raw
    .lines()
    .enumerate()
    .map(|(idx, line)| {
      if idx == 0 {
        line
      } else {
        let strip = line
          .bytes()
          .take(start_col)
          .take_while(|&b| b == b' ')
          .count();
        &line[strip..]
      }
    })
    .collect::<Vec<_>>()
    .join("\n")
}

fn node_text<'a>(node: Node, source: &'a str) -> &'a str {
  &source[node.byte_range()]
}

fn node_position(node: Node, source: &str) -> Position {
  let point = node.start_position();
  let start_byte = node.start_byte();
  let line_start = start_byte.saturating_sub(point.column);
  let utf16_col = source
    .get(line_start..start_byte)
    .map_or(point.column, |prefix| prefix.encode_utf16().count());
  Position::new(point.row as u32, utf16_col as u32)
}

fn collect_type_refs(node: Node, source: &str, acc: &mut Vec<TypeRefSite>) {
  match node.kind() {
    "type_identifier" => {
      acc.push(TypeRefSite {
        position: node_position(node, source),
        needs_type_def_fallback: node_text(node, source) == "Self",
      });
      return;
    }
    "self_type" | "self" => {
      acc.push(TypeRefSite {
        position: node_position(node, source),
        needs_type_def_fallback: true,
      });
      return;
    }
    // Don't descend into nested function/type declarations (they collect their own refs).
    "function_item" | "struct_item" | "enum_item" | "trait_item" | "type_item" => return,
    _ => {}
  }

  let mut cursor = node.walk();
  for child in node.children(&mut cursor) {
    collect_type_refs(child, source, acc);
  }
}

fn parse_type_kind(node_kind: &str) -> Option<TypeKind> {
  match node_kind {
    "struct_item" => Some(TypeKind::Struct),
    "enum_item" => Some(TypeKind::Enum),
    "trait_item" => Some(TypeKind::Trait),
    "type_item" => Some(TypeKind::Type),
    _ => None,
  }
}

fn format_self_parameter(raw: &str, self_type: &str) -> String {
  let trimmed = raw.trim();
  if let Some(prefix) = trimmed.strip_suffix("self") {
    let prefix = prefix.trim();
    if prefix.is_empty() {
      return format!("self: {self_type}");
    }
    if prefix == "mut" {
      return format!("mut self: {self_type}");
    }
    if prefix.starts_with('&') {
      let sep = if prefix == "&" { "" } else { " " };
      return format!("self: {prefix}{sep}{self_type}");
    }
  }
  trimmed.to_owned()
}

fn collect_self_replacements(
  node: Node,
  source: &str,
  self_type: &str,
  edits: &mut Vec<(usize, usize, String)>,
) {
  match node.kind() {
    "self_parameter" => {
      let raw = node_text(node, source);
      if !raw.contains(':') {
        edits.push((
          node.start_byte(),
          node.end_byte(),
          format_self_parameter(raw, self_type),
        ));
        return;
      }
    }
    "self_type" => {
      edits.push((node.start_byte(), node.end_byte(), self_type.to_owned()));
      return;
    }
    "type_identifier" if node_text(node, source) == "Self" => {
      edits.push((node.start_byte(), node.end_byte(), self_type.to_owned()));
      return;
    }
    _ => {}
  }

  let mut cursor = node.walk();
  for child in node.children(&mut cursor) {
    collect_self_replacements(child, source, self_type, edits);
  }
}

fn substitute_self(node: Node, source: &str, self_type: &str) -> String {
  let mut edits = Vec::new();
  collect_self_replacements(node, source, self_type, &mut edits);
  if edits.is_empty() {
    return node_text(node, source).to_owned();
  }

  edits.sort_by_key(|(start, _, _)| *start);
  let base = node.start_byte();
  let raw = node_text(node, source);
  let mut out = String::with_capacity(raw.len() + self_type.len() * edits.len());
  let mut cursor = 0;

  for (start, end, replacement) in edits {
    let rel_start = start.saturating_sub(base);
    let rel_end = end.saturating_sub(base);
    if rel_start >= cursor && rel_end <= raw.len() {
      out.push_str(&raw[cursor..rel_start]);
      out.push_str(&replacement);
      cursor = rel_end;
    }
  }
  out.push_str(&raw[cursor..]);
  out
}

fn walk_tree(
  node: Node,
  source: &str,
  file_path: &Path,
  enclosing_type: Option<&str>,
  types: &mut Vec<TypeNode>,
  functions: &mut Vec<FunctionInfo>,
) {
  if let Some(kind) = parse_type_kind(node.kind()) {
    if let Some(name) = node.child_by_field_name("name") {
      let id = TypeId(types.len());
      let body = dedent_body(node_text(node, source), node.start_position().column);
      types.push(TypeNode {
        id,
        name: node_text(name, source).to_string(),
        kind,
        body,
        file_path: file_path.to_path_buf(),
        position: node_position(name, source),
      });
    }
  } else if node.kind() == "function_item"
    && let Some(name) = node.child_by_field_name("name")
  {
    let mut param_type_refs = Vec::new();
    for field in ["type_parameters", "parameters", "where_clause"] {
      if let Some(child) = node.child_by_field_name(field) {
        collect_type_refs(child, source, &mut param_type_refs);
      }
    }

    let mut return_type_refs = Vec::new();
    if let Some(child) = node.child_by_field_name("return_type") {
      collect_type_refs(child, source, &mut return_type_refs);
    }

    let params = node
      .child_by_field_name("parameters")
      .map(|n| match enclosing_type {
        Some(self_ty) => substitute_self(n, source, self_ty),
        None => node_text(n, source).to_string(),
      })
      .unwrap_or_default();

    let return_type = node
      .child_by_field_name("return_type")
      .map(|n| match enclosing_type {
        Some(self_ty) => substitute_self(n, source, self_ty),
        None => node_text(n, source).to_string(),
      });

    let id = FunctionId(functions.len());
    functions.push(FunctionInfo {
      id,
      name: node_text(name, source).to_string(),
      file_path: file_path.to_path_buf(),
      position: node_position(name, source),
      params,
      return_type,
      param_type_refs,
      return_type_refs,
      input_types: Vec::new(),
      output_types: Vec::new(),
    });
  }

  let child_enclosing = match node.kind() {
    "impl_item" => node
      .child_by_field_name("type")
      .map(|n| node_text(n, source))
      .or(enclosing_type),
    "trait_item" => node
      .child_by_field_name("name")
      .map(|n| node_text(n, source))
      .or(enclosing_type),
    _ => enclosing_type,
  };

  let mut cursor = node.walk();
  for child in node.children(&mut cursor) {
    walk_tree(child, source, file_path, child_enclosing, types, functions);
  }
}

fn collect_source_files(root: &Path, ignore: &[String]) -> Vec<(PathBuf, String)> {
  let mut res = Vec::new();
  let Ok(read_dir_res) = fs::read_dir(root) else {
    return res;
  };

  for maybe_dir_entry in read_dir_res {
    let Ok(dir_entry) = maybe_dir_entry else {
      continue;
    };

    if dir_entry
      .file_name()
      .to_str()
      .is_some_and(|name| name == ".git" || ignore.iter().any(|i| i == name))
    {
      continue;
    }

    let path = dir_entry.path();

    if path.is_file() {
      if path.extension().and_then(|e| e.to_str()) != Some("rs") {
        continue;
      }
      if let Ok(source_file) = fs::read_to_string(&path) {
        res.push((canonicalize_or_keep(&path), source_file));
      }
    } else if path.is_dir() {
      let mut child_files = collect_source_files(&path, ignore);
      res.append(&mut child_files);
    }
  }
  res
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn dedent_body_aligns_indented_lines() {
    let raw = "enum EdgeHighlight {\n      Normal,\n      Dimmed,\n    }";
    let expected = "enum EdgeHighlight {\n  Normal,\n  Dimmed,\n}";
    assert_eq!(dedent_body(raw, 4), expected);
  }

  #[test]
  fn normalize_ignore_entries_trims_slashes_and_comments() {
    let raw = vec![
      "# build output".to_owned(),
      "".to_owned(),
      "/target/".to_owned(),
      "  dist/ ".to_owned(),
    ];
    assert_eq!(normalize_ignore_entries(&raw), vec!["target", "dist"]);
  }

  #[test]
  fn node_position_converts_non_ascii_prefix_to_utf16() {
    let mut parser = Parser::new();
    assert!(parser.set_language(&tree_sitter_rust::LANGUAGE.into()).is_ok());
    let source = "/* — */ struct Foo;";
    let tree = parser.parse(source, None);
    assert!(tree.is_some());
    if let Some(tree) = tree {
      let mut types = Vec::new();
      let mut functions = Vec::new();
      walk_tree(
        tree.root_node(),
        source,
        Path::new("lib.rs"),
        None,
        &mut types,
        &mut functions,
      );
      assert_eq!(types.len(), 1);
      assert_eq!(types[0].position, Position::new(0, 15));
      assert_eq!(types[0].kind, TypeKind::Struct);
    }
  }

  #[test]
  fn walk_tree_substitutes_self_and_self_type_in_impl_methods() {
    let mut parser = Parser::new();
    assert!(parser.set_language(&tree_sitter_rust::LANGUAGE.into()).is_ok());

    let source = r#"
struct Graph;
impl Graph {
  fn new() -> Self { Graph }
  fn func(&self, self_id: u32) -> Option<&Self> { None }
  fn mutate(&mut self, myself: &str) {}
  fn consume(self) -> Self { self }
  fn consume_mut(mut self) {}
  fn lifetime<'a>(&'a mut self) {}
  fn boxed(self: Box<Self>) {}
}
"#;
    let tree = parser.parse(source, None);
    assert!(tree.is_some());
    if let Some(tree) = tree {
      let mut types = Vec::new();
      let mut functions = Vec::new();
      walk_tree(
        tree.root_node(),
        source,
        Path::new("model.rs"),
        None,
        &mut types,
        &mut functions,
      );
      let sigs: Vec<String> = functions.iter().map(FunctionInfo::signature).collect();
      assert_eq!(
        sigs,
        vec![
          "fn new() -> Graph",
          "fn func(self: &Graph, self_id: u32) -> Option<&Graph>",
          "fn mutate(self: &mut Graph, myself: &str)",
          "fn consume(self: Graph) -> Graph",
          "fn consume_mut(mut self: Graph)",
          "fn lifetime(self: &'a mut Graph)",
          "fn boxed(self: Box<Graph>)",
        ]
      );
    }
  }
}
