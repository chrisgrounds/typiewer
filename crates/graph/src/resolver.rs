use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::Duration;

use lsp::{Location, LspClient, Position};

use crate::model::{
  EdgeId, FunctionId, FunctionInfo, GraphEdge, GraphResolution, TypeId, TypeRef,
  canonicalize_or_keep,
};

pub fn resolve_with_rust_analyzer(
  workspace_root: &Path,
  type_refs: &[TypeRef],
  functions: Vec<FunctionInfo>,
  ready_timeout: Duration,
) -> lsp::Result<GraphResolution> {
  let lsp = LspClient::start_rust_analyzer(workspace_root)?;
  lsp.wait_until_ready(ready_timeout)?;
  let resolution = resolve_edges(type_refs, functions, &lsp);
  let _ = lsp.shutdown();
  Ok(resolution)
}

pub fn resolve_edges(
  type_refs: &[TypeRef],
  mut functions: Vec<FunctionInfo>,
  lsp: &LspClient,
) -> GraphResolution {
  let mut type_index: HashMap<(PathBuf, Position), TypeId> = HashMap::with_capacity(type_refs.len());
  let mut opened_files: HashSet<PathBuf> = HashSet::new();

  for ty in type_refs {
    type_index.insert((ty.file_path.clone(), ty.position), ty.id);
    if opened_files.insert(ty.file_path.clone()) {
      let _ = lsp.open_file(&ty.file_path);
    }
  }

  for func in &functions {
    if opened_files.insert(func.file_path.clone()) {
      let _ = lsp.open_file(&func.file_path);
    }
  }

  let mut canonical_cache: HashMap<PathBuf, PathBuf> = HashMap::new();
  let mut edge_map: BTreeMap<(TypeId, TypeId), Vec<FunctionId>> = BTreeMap::new();

  for func in &mut functions {
    func.input_types = resolve_type_refs(
      &func.file_path,
      &func.param_type_refs,
      &type_index,
      &mut canonical_cache,
      lsp,
    );
    func.output_types = resolve_type_refs(
      &func.file_path,
      &func.return_type_refs,
      &type_index,
      &mut canonical_cache,
      lsp,
    );

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

  GraphResolution { edges, functions }
}

fn collect_matching_types(
  locations: Vec<Location>,
  type_index: &HashMap<(PathBuf, Position), TypeId>,
  canonical_cache: &mut HashMap<PathBuf, PathBuf>,
  seen: &mut HashSet<TypeId>,
  resolved: &mut Vec<TypeId>,
) -> bool {
  let mut matched = false;
  for loc in locations {
    let Some(raw_path) = loc.file_path() else {
      continue;
    };
    let target_path = canonical_cache
      .entry(raw_path)
      .or_insert_with_key(|raw| canonicalize_or_keep(raw))
      .clone();

    if let Some(&type_id) = type_index.get(&(target_path, loc.range.start)) {
      matched = true;
      if seen.insert(type_id) {
        resolved.push(type_id);
      }
    }
  }
  matched
}

fn resolve_type_refs(
  file_path: &Path,
  refs: &[Position],
  type_index: &HashMap<(PathBuf, Position), TypeId>,
  canonical_cache: &mut HashMap<PathBuf, PathBuf>,
  lsp: &LspClient,
) -> Vec<TypeId> {
  let mut resolved = Vec::new();
  let mut seen = HashSet::new();

  for &ref_pos in refs {
    let matched = lsp.definition(file_path, ref_pos).is_ok_and(|locations| {
      collect_matching_types(locations, type_index, canonical_cache, &mut seen, &mut resolved)
    });

    // Fall back to `typeDefinition` for `Self` (whose `definition` points to the
    // `impl` block) and `self` parameters (whose `definition` points to themselves).
    if !matched && let Ok(locations) = lsp.type_definition(file_path, ref_pos) {
      collect_matching_types(locations, type_index, canonical_cache, &mut seen, &mut resolved);
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

#[cfg(test)]
mod tests {
  use super::*;

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
