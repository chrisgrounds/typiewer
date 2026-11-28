use std::fs;
use std::path::{Path, PathBuf};

use lsp::Position;
use macros::performance;
use tree_sitter::{Node, Parser};

use crate::model::{FunctionId, FunctionInfo, TypeId, TypeNode};

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

  let files = performance!(collect_source_files(root, ignore), "collect source files");

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
        &mut types,
        &mut functions,
      );
    },
    "tree-sitter: walk_tree"
  );

  Ok(ParsedWorkspace { types, functions })
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

fn node_position(node: Node) -> Position {
  let point = node.start_position();
  Position::new(point.row as u32, point.column as u32)
}

fn collect_type_refs(node: Node, acc: &mut Vec<Position>) {
  match node.kind() {
    "type_identifier" | "self_type" | "self" => {
      acc.push(node_position(node));
      return;
    }
    // Don't descend into nested function/type declarations (they collect their own refs).
    "function_item" | "struct_item" | "enum_item" | "trait_item" | "type_item" => return,
    _ => {}
  }

  let mut cursor = node.walk();
  for child in node.children(&mut cursor) {
    collect_type_refs(child, acc);
  }
}

fn walk_tree(
  node: Node,
  source: &str,
  file_path: &Path,
  types: &mut Vec<TypeNode>,
  functions: &mut Vec<FunctionInfo>,
) {
  match node.kind() {
    "struct_item" | "enum_item" | "trait_item" | "type_item" => {
      if let Some(name) = node.child_by_field_name("name") {
        let id = TypeId(types.len());
        let body = dedent_body(node_text(node, source), node.start_position().column);
        types.push(TypeNode {
          id,
          name: node_text(name, source).to_string(),
          kind: node.kind().trim_end_matches("_item").to_string(),
          body,
          file_path: file_path.to_path_buf(),
          position: node_position(name),
        });
      }
    }
    "function_item" => {
      if let Some(name) = node.child_by_field_name("name") {
        let mut param_type_refs = Vec::new();
        for field in ["type_parameters", "parameters", "where_clause"] {
          if let Some(child) = node.child_by_field_name(field) {
            collect_type_refs(child, &mut param_type_refs);
          }
        }

        let mut return_type_refs = Vec::new();
        if let Some(child) = node.child_by_field_name("return_type") {
          collect_type_refs(child, &mut return_type_refs);
        }

        let params = node
          .child_by_field_name("parameters")
          .map(|n| node_text(n, source).to_string())
          .unwrap_or_default();

        let return_type = node
          .child_by_field_name("return_type")
          .map(|n| node_text(n, source).to_string());

        let id = FunctionId(functions.len());
        functions.push(FunctionInfo {
          id,
          name: node_text(name, source).to_string(),
          file_path: file_path.to_path_buf(),
          position: node_position(name),
          params,
          return_type,
          param_type_refs,
          return_type_refs,
          input_types: Vec::new(),
          output_types: Vec::new(),
        });
      }
    }
    _ => {}
  }

  let mut cursor = node.walk();
  for child in node.children(&mut cursor) {
    walk_tree(child, source, file_path, types, functions);
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
      .is_some_and(|name| ignore.iter().any(|i| i == name))
    {
      continue;
    }

    let path = dir_entry.path();

    if path.is_file() {
      if path.extension().and_then(|e| e.to_str()) != Some("rs") {
        continue;
      }
      if let Ok(source_file) = fs::read_to_string(&path) {
        let canonical = fs::canonicalize(&path).unwrap_or(path);
        res.push((canonical, source_file));
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
}
