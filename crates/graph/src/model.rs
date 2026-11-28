use std::collections::HashSet;
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};

use lsp::{LspClient, Position};

use crate::resolver;

#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash, Ord, PartialOrd)]
pub struct TypeId(pub usize);

#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash, Ord, PartialOrd)]
pub struct FunctionId(pub usize);

#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
pub struct EdgeId(pub usize);

#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
pub enum TypeKind {
  Struct,
  Enum,
  Trait,
  Type,
}

impl TypeKind {
  pub const fn as_str(self) -> &'static str {
    match self {
      Self::Struct => "struct",
      Self::Enum => "enum",
      Self::Trait => "trait",
      Self::Type => "type",
    }
  }
}

impl fmt::Display for TypeKind {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    f.write_str(self.as_str())
  }
}

#[derive(Debug, Clone)]
pub struct TypeNode {
  pub id: TypeId,
  pub name: String,
  pub kind: TypeKind,
  pub body: String,
  pub file_path: PathBuf,
  pub position: Position,
}

#[derive(Debug, Clone)]
pub struct TypeRef {
  pub id: TypeId,
  pub file_path: PathBuf,
  pub position: Position,
}

impl From<&TypeNode> for TypeRef {
  fn from(node: &TypeNode) -> Self {
    Self {
      id: node.id,
      file_path: node.file_path.clone(),
      position: node.position,
    }
  }
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
  pub directed_pairs: HashSet<(TypeId, TypeId)>,
}

impl Graph {
  pub fn from_parsed(types: Vec<TypeNode>, functions: Vec<FunctionInfo>) -> Self {
    Self {
      types,
      functions,
      edges: Vec::new(),
      directed_pairs: HashSet::new(),
    }
  }

  pub fn new(types: Vec<TypeNode>, functions: Vec<FunctionInfo>, lsp: &LspClient) -> Self {
    let type_refs: Vec<TypeRef> = types.iter().map(TypeRef::from).collect();
    let resolution = resolver::resolve_edges(&type_refs, functions, lsp);
    let mut graph = Self::from_parsed(types, Vec::new());
    graph.apply_resolution(resolution);
    graph
  }

  pub fn apply_resolution(&mut self, resolution: GraphResolution) {
    self.directed_pairs = resolution.edges.iter().map(|e| (e.from, e.to)).collect();
    self.edges = resolution.edges;
    self.functions = resolution.functions;
  }

  pub fn type_refs(&self) -> Vec<TypeRef> {
    self.types.iter().map(TypeRef::from).collect()
  }

  pub fn ty(&self, id: TypeId) -> Option<&TypeNode> {
    self.types.get(id.0)
  }

  pub fn func(&self, id: FunctionId) -> Option<&FunctionInfo> {
    self.functions.get(id.0)
  }

  pub fn edge(&self, id: EdgeId) -> Option<&GraphEdge> {
    self.edges.get(id.0)
  }
}

pub fn canonicalize_or_keep(path: &Path) -> PathBuf {
  fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

pub fn relative_display_path(canonical_root: &Path, path: &Path) -> String {
  path
    .strip_prefix(canonical_root)
    .unwrap_or(path)
    .display()
    .to_string()
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
