mod application;

use lsp::{LspClient, Position};
use macros::performance;
use std::{
  collections::{BTreeMap, HashMap, HashSet},
  fs,
  path::{Path, PathBuf},
  time::Duration,
};
use tree_sitter::{Node, Parser};

#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash, Ord, PartialOrd)]
pub struct TypeId(pub usize);

#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash, Ord, PartialOrd)]
pub struct FunctionId(pub usize);

#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
pub struct EdgeId(pub usize);

#[derive(Debug, Clone)]
pub struct TypeNode {
  pub id: TypeId,
  pub name: String,
  pub kind: String,
  pub body: String,
  pub file_path: PathBuf,
  pub position: Position,
}

#[derive(Debug, Clone)]
pub struct FunctionInfo {
  pub id: FunctionId,
  pub name: String,
  pub file_path: PathBuf,
  pub position: Position,
  pub params: String,
  pub return_type: Option<String>,
  pub param_type_refs: Vec<Position>,
  pub return_type_refs: Vec<Position>,
  pub input_types: Vec<TypeId>,
  pub output_types: Vec<TypeId>,
}

impl FunctionInfo {
  pub fn signature(&self) -> String {
    let params = collapse_whitespace(&self.params);
    match &self.return_type {
      Some(ret) => format!("fn {}{params} -> {}", self.name, collapse_whitespace(ret)),
      None => format!("fn {}{params}", self.name),
    }
  }
}

#[derive(Debug, Clone)]
pub struct GraphEdge {
  pub id: EdgeId,
  pub from: TypeId,
  pub to: TypeId,
  pub functions: Vec<FunctionId>,
}

#[derive(Debug, Clone)]
pub struct GraphResolution {
  pub edges: Vec<GraphEdge>,
  pub functions: Vec<FunctionInfo>,
}

#[derive(Debug)]
pub struct Graph {
  pub types: Vec<TypeNode>,
  pub functions: Vec<FunctionInfo>,
  pub edges: Vec<GraphEdge>,
}

impl Graph {
  pub fn from_parsed(types: Vec<TypeNode>, functions: Vec<FunctionInfo>) -> Self {
    Self {
      types,
      functions,
      edges: Vec::new(),
    }
  }

  pub fn resolve_edges(
    types: &[TypeNode],
    functions: &[FunctionInfo],
    lsp: &LspClient,
  ) -> GraphResolution {
    let mut type_index: HashMap<(PathBuf, Position), TypeId> = HashMap::with_capacity(types.len());
    let mut opened_files: HashSet<PathBuf> = HashSet::new();

    for ty in types {
      type_index.insert((ty.file_path.clone(), ty.position), ty.id);
      if opened_files.insert(ty.file_path.clone()) {
        let _ = lsp.open_file(&ty.file_path);
      }
    }

    for func in functions {
      if opened_files.insert(func.file_path.clone()) {
        let _ = lsp.open_file(&func.file_path);
      }
    }

    let mut resolved_functions = functions.to_vec();
    let mut edge_map: BTreeMap<(TypeId, TypeId), Vec<FunctionId>> = BTreeMap::new();

    for func in &mut resolved_functions {
      func.input_types =
        resolve_type_refs(&func.file_path, &func.param_type_refs, &type_index, lsp);
      func.output_types =
        resolve_type_refs(&func.file_path, &func.return_type_refs, &type_index, lsp);

      let pairs = function_edge_pairs(&func.input_types, &func.output_types);
      for (from, to) in pairs {
        let entry = edge_map.entry((from, to)).or_default();
        if !entry.contains(&func.id) {
          entry.push(func.id);
        }
      }
    }

    let edges = edge_map
      .into_iter()
      .enumerate()
      .map(|(idx, ((from, to), func_ids))| GraphEdge {
        id: EdgeId(idx),
        from,
        to,
        functions: func_ids,
      })
      .collect();

    GraphResolution {
      edges,
      functions: resolved_functions,
    }
  }

  pub fn new(
    types: Vec<TypeNode>,
    functions: Vec<FunctionInfo>,
    lsp: &LspClient,
  ) -> lsp::Result<Self> {
    let mut graph = Self::from_parsed(types, functions);
    let resolution = Self::resolve_edges(&graph.types, &graph.functions, lsp);
    graph.edges = resolution.edges;
    graph.functions = resolution.functions;
    Ok(graph)
  }
}

fn resolve_type_refs(
  file_path: &Path,
  refs: &[Position],
  type_index: &HashMap<(PathBuf, Position), TypeId>,
  lsp: &LspClient,
) -> Vec<TypeId> {
  let mut resolved = Vec::new();
  let mut seen = HashSet::new();

  for &ref_pos in refs {
    let mut matched = false;

    if let Ok(locations) = lsp.definition(file_path, ref_pos) {
      for loc in locations {
        let Some(raw_path) = loc.file_path() else {
          continue;
        };
        let target_path = fs::canonicalize(&raw_path).unwrap_or(raw_path);

        if let Some(&type_id) = type_index.get(&(target_path, loc.range.start)) {
          matched = true;
          if seen.insert(type_id) {
            resolved.push(type_id);
          }
        }
      }
    }

    // Fall back to `typeDefinition` for `Self` (whose `definition` points to the
    // `impl` block) and `self` parameters (whose `definition` points to themselves).
    if !matched && let Ok(locations) = lsp.type_definition(file_path, ref_pos) {
      for loc in locations {
        let Some(raw_path) = loc.file_path() else {
          continue;
        };
        let target_path = fs::canonicalize(&raw_path).unwrap_or(raw_path);

        if let Some(&type_id) = type_index.get(&(target_path, loc.range.start))
          && seen.insert(type_id)
        {
          resolved.push(type_id);
        }
      }
    }
  }

  resolved
}

fn function_edge_pairs(input_types: &[TypeId], output_types: &[TypeId]) -> Vec<(TypeId, TypeId)> {
  let mut pairs = Vec::new();
  let mut seen = HashSet::new();

  if !input_types.is_empty() && !output_types.is_empty() {
    for &from in input_types {
      for &to in output_types {
        if from != to && seen.insert((from, to)) {
          pairs.push((from, to));
        }
      }
    }
  } else if input_types.len() >= 2 {
    // Functions returning `()` or external types connect secondary parameter types into
    // the primary parameter (typically `self` for methods).
    let target = input_types[0];
    for &source in &input_types[1..] {
      if source != target && seen.insert((source, target)) {
        pairs.push((source, target));
      }
    }
  }

  pairs
}

fn collapse_whitespace(text: &str) -> String {
  let mut out = String::with_capacity(text.len());
  let mut prev_space = false;

  for ch in text.chars() {
    if ch.is_whitespace() {
      if !prev_space && !out.is_empty() {
        out.push(' ');
        prev_space = true;
      }
    } else {
      out.push(ch);
      prev_space = false;
    }
  }

  if out.ends_with(' ') {
    out.pop();
  }

  out
    .replace("( ", "(")
    .replace(", )", ")")
    .replace(" )", ")")
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

fn main() {
  let mut parser = Parser::new();
  if let Err(err) = parser.set_language(&tree_sitter_rust::LANGUAGE.into()) {
    eprintln!("Failed to load Rust grammar: {err}");
    return;
  }

  let root = ".";
  let gitignore = "./.gitignore";

  let gitignored_files: Vec<String> = performance!(
    fs::read_to_string(gitignore)
      .unwrap_or_default()
      .lines()
      .map(str::to_owned)
      .collect(),
    "parse gitignore"
  );

  let files = performance!(
    collect_source_files(Path::new(root), &gitignored_files),
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
        &mut types,
        &mut functions,
      );
    },
    "tree-sitter: walk_tree"
  );

  if std::env::args().any(|arg| arg == "--dump") {
    let graph = match LspClient::start_rust_analyzer(root) {
      Ok(lsp) => {
        let _ = performance!(
          lsp.wait_until_ready(Duration::from_secs(60)),
          "lsp: wait_until_ready"
        );
        let graph = match performance!(Graph::new(types, functions, &lsp), "lsp: Graph::new") {
          Ok(graph) => graph,
          Err(err) => {
            eprintln!("Failed to build graph via LSP: {err}");
            Graph {
              types: Vec::new(),
              functions: Vec::new(),
              edges: Vec::new(),
            }
          }
        };
        let _ = lsp.shutdown();
        graph
      }
      Err(err) => {
        eprintln!("Failed to start rust-analyzer: {err}");
        Graph::from_parsed(types, functions)
      }
    };

    println!("=== TYPES ({}) ===", graph.types.len());
    for ty in &graph.types {
      println!(
        "  [{}] {}:{}  {} {} ({} lines)",
        ty.id.0,
        ty.file_path.display(),
        ty.position.line + 1,
        ty.kind,
        ty.name,
        ty.body.lines().count()
      );
    }
    println!("\n=== FUNCTION EDGES ({}) ===", graph.edges.len());
    for edge in &graph.edges {
      let Some(from) = graph.types.get(edge.from.0) else {
        continue;
      };
      let Some(to) = graph.types.get(edge.to.0) else {
        continue;
      };
      let fn_names: Vec<&str> = edge
        .functions
        .iter()
        .filter_map(|fid| graph.functions.get(fid.0).map(|f| f.name.as_str()))
        .collect();
      println!(
        "  {} ({}) -> {} ({})  via [{}]",
        from.name,
        from
          .file_path
          .file_name()
          .and_then(|n| n.to_str())
          .unwrap_or(""),
        to.name,
        to.file_path
          .file_name()
          .and_then(|n| n.to_str())
          .unwrap_or(""),
        fn_names.join(", ")
      );
    }
    return;
  }

  let graph = Graph::from_parsed(types, functions);
  application::run(PathBuf::from(root), graph);
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
  fn collapse_whitespace_normalizes_multiline_signature() {
    let raw = "(\n  a: TypeA,\n  b: &TypeB,\n)";
    assert_eq!(collapse_whitespace(raw), "(a: TypeA, b: &TypeB)");
  }

  #[test]
  fn function_edge_pairs_connects_inputs_to_outputs() {
    let pairs = function_edge_pairs(&[TypeId(0), TypeId(1)], &[TypeId(2)]);
    assert_eq!(pairs, vec![(TypeId(0), TypeId(2)), (TypeId(1), TypeId(2))]);
  }

  #[test]
  fn function_edge_pairs_skips_self_loops() {
    let pairs = function_edge_pairs(&[TypeId(0), TypeId(1)], &[TypeId(0)]);
    assert_eq!(pairs, vec![(TypeId(1), TypeId(0))]);
  }

  #[test]
  fn function_edge_pairs_connects_params_when_no_output_type() {
    let pairs = function_edge_pairs(&[TypeId(0), TypeId(1), TypeId(2)], &[]);
    assert_eq!(pairs, vec![(TypeId(1), TypeId(0)), (TypeId(2), TypeId(0))]);
  }
}
