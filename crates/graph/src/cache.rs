use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use serde::{Deserialize, Serialize};

use crate::model::{EdgeId, FunctionId, FunctionInfo, Graph, GraphEdge, GraphResolution, TypeId};

const CACHE_VERSION: u32 = 1;

#[derive(Serialize, Deserialize)]
struct CachedEdge {
  from: usize,
  to: usize,
  functions: Vec<usize>,
}

#[derive(Serialize, Deserialize)]
struct CachedFunctionTypes {
  input_types: Vec<usize>,
  output_types: Vec<usize>,
}

#[derive(Serialize, Deserialize)]
struct CachedResolution {
  version: u32,
  fingerprint: u64,
  edges: Vec<CachedEdge>,
  function_types: Vec<CachedFunctionTypes>,
}

struct Fnv1a(u64);

impl Fnv1a {
  fn new() -> Self {
    Self(0xcbf2_9ce4_8422_2325)
  }

  fn update(&mut self, bytes: &[u8]) {
    for &b in bytes {
      self.0 ^= u64::from(b);
      self.0 = self.0.wrapping_mul(0x0100_0000_01b3);
    }
  }

  fn update_u64(&mut self, value: u64) {
    self.update(&value.to_le_bytes());
  }

  fn update_str(&mut self, value: &str) {
    self.update_u64(value.len() as u64);
    self.update(value.as_bytes());
  }

  fn finish(self) -> u64 {
    self.0
  }
}

fn cache_dir() -> PathBuf {
  #[cfg(target_os = "macos")]
  if let Some(home) = std::env::var_os("HOME") {
    return PathBuf::from(home)
      .join("Library")
      .join("Caches")
      .join("typiewer");
  }

  if let Some(xdg) = std::env::var_os("XDG_CACHE_HOME") {
    return PathBuf::from(xdg).join("typiewer");
  }

  if let Some(home) = std::env::var_os("HOME") {
    return PathBuf::from(home).join(".cache").join("typiewer");
  }

  std::env::temp_dir().join("typiewer")
}

fn cache_file_path(canonical_root: &Path) -> PathBuf {
  let mut hasher = Fnv1a::new();
  hasher.update_str(&canonical_root.to_string_lossy());
  let root_hash = hasher.finish();
  cache_dir().join(format!("graph-v{CACHE_VERSION}-{root_hash:016x}.json"))
}

fn hash_file_metadata(hasher: &mut Fnv1a, path: &Path) {
  hasher.update_str(&path.to_string_lossy());
  if let Ok(meta) = fs::metadata(path) {
    hasher.update_u64(meta.len());
    if let Ok(modified) = meta.modified()
      && let Ok(dur) = modified.duration_since(UNIX_EPOCH)
    {
      hasher.update_u64(dur.as_secs());
      hasher.update_u64(u64::from(dur.subsec_nanos()));
    }
  }
}

fn collect_manifest_files(root: &Path, acc: &mut BTreeSet<PathBuf>) {
  let Ok(entries) = fs::read_dir(root) else {
    return;
  };
  for entry in entries.flatten() {
    let name = entry.file_name();
    let Some(name_str) = name.to_str() else {
      continue;
    };
    if matches!(name_str, ".git" | "target" | "node_modules") {
      continue;
    }
    let path = entry.path();
    if path.is_file() {
      if matches!(name_str, "Cargo.toml" | "Cargo.lock") {
        acc.insert(path);
      }
    } else if path.is_dir() {
      collect_manifest_files(&path, acc);
    }
  }
}

/// Computes a deterministic fingerprint over the parsed types/functions and the metadata
/// (size + mtime) of all referenced `.rs` files and workspace `Cargo.toml`/`Cargo.lock` files.
pub fn compute_workspace_fingerprint(canonical_root: &Path, graph: &Graph) -> u64 {
  let mut hasher = Fnv1a::new();
  hasher.update_u64(u64::from(CACHE_VERSION));

  let mut tracked_files: BTreeSet<PathBuf> = BTreeSet::new();
  collect_manifest_files(canonical_root, &mut tracked_files);

  hasher.update_u64(graph.types.len() as u64);
  for ty in &graph.types {
    tracked_files.insert(ty.file_path.clone());
    hasher.update_u64(ty.id.0 as u64);
    hasher.update_str(&ty.name);
    hasher.update_str(ty.kind.as_str());
    hasher.update_str(&ty.file_path.to_string_lossy());
    hasher.update_u64(u64::from(ty.position.line));
    hasher.update_u64(u64::from(ty.position.character));
  }

  hasher.update_u64(graph.functions.len() as u64);
  for func in &graph.functions {
    tracked_files.insert(func.file_path.clone());
    hasher.update_u64(func.id.0 as u64);
    hasher.update_str(&func.name);
    hasher.update_str(&func.file_path.to_string_lossy());
    hasher.update_u64(u64::from(func.position.line));
    hasher.update_u64(u64::from(func.position.character));
    hasher.update_u64(func.param_type_refs.len() as u64);
    hasher.update_u64(func.return_type_refs.len() as u64);
  }

  hasher.update_u64(tracked_files.len() as u64);
  for path in &tracked_files {
    hash_file_metadata(&mut hasher, path);
  }

  hasher.finish()
}

pub fn load_cached_resolution(
  canonical_root: &Path,
  fingerprint: u64,
  functions: &[FunctionInfo],
  type_count: usize,
) -> Option<GraphResolution> {
  let path = cache_file_path(canonical_root);
  load_cached_resolution_from(&path, fingerprint, functions, type_count)
}

pub fn save_cached_resolution(
  canonical_root: &Path,
  fingerprint: u64,
  resolution: &GraphResolution,
) {
  let path = cache_file_path(canonical_root);
  let _ = save_cached_resolution_to(&path, fingerprint, resolution);
}

fn load_cached_resolution_from(
  cache_path: &Path,
  fingerprint: u64,
  functions: &[FunctionInfo],
  type_count: usize,
) -> Option<GraphResolution> {
  let raw = fs::read_to_string(cache_path).ok()?;
  let cached: CachedResolution = serde_json::from_str(&raw).ok()?;
  if cached.version != CACHE_VERSION
    || cached.fingerprint != fingerprint
    || cached.function_types.len() != functions.len()
  {
    return None;
  }

  let mut resolved_functions = functions.to_vec();
  for (func, cached_fn) in resolved_functions.iter_mut().zip(cached.function_types) {
    if cached_fn.input_types.iter().any(|&id| id >= type_count)
      || cached_fn.output_types.iter().any(|&id| id >= type_count)
    {
      return None;
    }
    func.input_types = cached_fn.input_types.into_iter().map(TypeId).collect();
    func.output_types = cached_fn.output_types.into_iter().map(TypeId).collect();
  }

  let mut edges = Vec::with_capacity(cached.edges.len());
  for (idx, edge) in cached.edges.into_iter().enumerate() {
    if edge.from >= type_count
      || edge.to >= type_count
      || edge.functions.iter().any(|&fid| fid >= functions.len())
    {
      return None;
    }
    edges.push(GraphEdge {
      id: EdgeId(idx),
      from: TypeId(edge.from),
      to: TypeId(edge.to),
      functions: edge.functions.into_iter().map(FunctionId).collect(),
    });
  }

  Some(GraphResolution {
    edges,
    functions: resolved_functions,
  })
}

fn save_cached_resolution_to(
  cache_path: &Path,
  fingerprint: u64,
  resolution: &GraphResolution,
) -> std::io::Result<()> {
  if let Some(parent) = cache_path.parent() {
    fs::create_dir_all(parent)?;
  }

  let payload = CachedResolution {
    version: CACHE_VERSION,
    fingerprint,
    edges: resolution
      .edges
      .iter()
      .map(|edge| CachedEdge {
        from: edge.from.0,
        to: edge.to.0,
        functions: edge.functions.iter().map(|fid| fid.0).collect(),
      })
      .collect(),
    function_types: resolution
      .functions
      .iter()
      .map(|func| CachedFunctionTypes {
        input_types: func.input_types.iter().map(|tid| tid.0).collect(),
        output_types: func.output_types.iter().map(|tid| tid.0).collect(),
      })
      .collect(),
  };

  let serialized = serde_json::to_string(&payload)
    .map_err(|err| std::io::Error::new(std::io::ErrorKind::InvalidData, err))?;

  let tmp_path = cache_path.with_extension("json.tmp");
  fs::write(&tmp_path, serialized)?;
  fs::rename(&tmp_path, cache_path)?;
  Ok(())
}

#[cfg(test)]
mod tests {
  use lsp::Position;

  use super::*;

  fn dummy_fn(id: usize) -> FunctionInfo {
    FunctionInfo {
      id: FunctionId(id),
      name: format!("f{id}"),
      file_path: PathBuf::from("/tmp/a.rs"),
      position: Position::new(id as u32, 0),
      params: "()".to_owned(),
      return_type: None,
      param_type_refs: Vec::new(),
      return_type_refs: Vec::new(),
      input_types: Vec::new(),
      output_types: Vec::new(),
    }
  }

  #[test]
  fn cache_round_trips_and_rejects_mismatched_fingerprint() {
    let dir = std::env::temp_dir().join(format!("typiewer-cache-test-{}", std::process::id()));
    let cache_path = dir.join("graph.json");

    let mut f0 = dummy_fn(0);
    f0.input_types = vec![TypeId(0)];
    f0.output_types = vec![TypeId(1)];
    let resolution = GraphResolution {
      edges: vec![GraphEdge {
        id: EdgeId(0),
        from: TypeId(0),
        to: TypeId(1),
        functions: vec![FunctionId(0)],
      }],
      functions: vec![f0],
    };

    assert!(save_cached_resolution_to(&cache_path, 42, &resolution).is_ok());

    let base_fns = vec![dummy_fn(0)];
    let loaded = load_cached_resolution_from(&cache_path, 42, &base_fns, 2);
    assert!(loaded.is_some());
    if let Some(loaded) = loaded {
      assert_eq!(loaded.edges.len(), 1);
      assert_eq!(loaded.edges[0].from, TypeId(0));
      assert_eq!(loaded.edges[0].to, TypeId(1));
      assert_eq!(loaded.functions[0].input_types, vec![TypeId(0)]);
      assert_eq!(loaded.functions[0].output_types, vec![TypeId(1)]);
    }

    // Wrong fingerprint invalidates the cache.
    assert!(load_cached_resolution_from(&cache_path, 99, &base_fns, 2).is_none());

    let _ = fs::remove_dir_all(&dir);
  }
}
