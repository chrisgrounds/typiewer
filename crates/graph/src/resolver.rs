use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

use lsp::{LspClient, Position};

use crate::model::{
  EdgeId, FunctionId, FunctionInfo, GraphEdge, GraphResolution, TypeId, TypeNode,
};

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
    func.input_types = resolve_type_refs(&func.file_path, &func.param_type_refs, &type_index, lsp);
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
