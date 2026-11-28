use std::path::PathBuf;

use lsp::{LspClient, Position};

use crate::resolver;

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
    resolver::resolve_edges(types, functions, lsp)
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

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn collapse_whitespace_normalizes_multiline_signature() {
    let raw = "(\n  a: TypeA,\n  b: &TypeB,\n)";
    assert_eq!(collapse_whitespace(raw), "(a: TypeA, b: &TypeB)");
  }
}
