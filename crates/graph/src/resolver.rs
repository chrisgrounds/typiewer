use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::Duration;

use lsp::{Location, LspClient, PendingLocations, Position};

use crate::model::{
  EdgeId, FunctionId, FunctionInfo, GraphEdge, GraphResolution, TypeId, TypeRef, TypeRefSite,
  canonicalize_or_keep,
};

const PIPELINE_BATCH_SIZE: usize = 64;

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

#[derive(Clone, Copy)]
enum RefSlot {
  Input,
  Output,
}

struct PendingDef {
  func_idx: usize,
  slot: RefSlot,
  site: TypeRefSite,
  pending: PendingLocations,
}

struct PendingTypeDef {
  func_idx: usize,
  slot: RefSlot,
  pending: PendingLocations,
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
  let mut input_seen: Vec<HashSet<TypeId>> = vec![HashSet::new(); functions.len()];
  let mut output_seen: Vec<HashSet<TypeId>> = vec![HashSet::new(); functions.len()];

  let mut work: Vec<(usize, RefSlot, TypeRefSite)> = Vec::new();
  for (func_idx, func) in functions.iter().enumerate() {
    for &site in &func.param_type_refs {
      work.push((func_idx, RefSlot::Input, site));
    }
    for &site in &func.return_type_refs {
      work.push((func_idx, RefSlot::Output, site));
    }
  }

  // Pipeline definition requests in batches so rust-analyzer can evaluate them concurrently.
  for chunk in work.chunks(PIPELINE_BATCH_SIZE) {
    let mut in_flight: Vec<PendingDef> = Vec::with_capacity(chunk.len());
    for &(func_idx, slot, site) in chunk {
      let file_path = &functions[func_idx].file_path;
      if let Ok(pending) = lsp.definition_async(file_path, site.position) {
        in_flight.push(PendingDef {
          func_idx,
          slot,
          site,
          pending,
        });
      }
    }

    let mut fallback_in_flight: Vec<PendingTypeDef> = Vec::new();
    for item in in_flight {
      let matched = item.pending.wait().is_ok_and(|locations| {
        let (seen, target) = match item.slot {
          RefSlot::Input => (
            &mut input_seen[item.func_idx],
            &mut functions[item.func_idx].input_types,
          ),
          RefSlot::Output => (
            &mut output_seen[item.func_idx],
            &mut functions[item.func_idx].output_types,
          ),
        };
        collect_matching_types(locations, &type_index, &mut canonical_cache, seen, target)
      });

      // Only fall back to `typeDefinition` for `Self` / `self` references, skipping
      // external standard/third-party types that won't resolve to a workspace type.
      if !matched && item.site.needs_type_def_fallback {
        let file_path = &functions[item.func_idx].file_path;
        if let Ok(pending) = lsp.type_definition_async(file_path, item.site.position) {
          fallback_in_flight.push(PendingTypeDef {
            func_idx: item.func_idx,
            slot: item.slot,
            pending,
          });
        }
      }
    }

    for item in fallback_in_flight {
      if let Ok(locations) = item.pending.wait() {
        let (seen, target) = match item.slot {
          RefSlot::Input => (
            &mut input_seen[item.func_idx],
            &mut functions[item.func_idx].input_types,
          ),
          RefSlot::Output => (
            &mut output_seen[item.func_idx],
            &mut functions[item.func_idx].output_types,
          ),
        };
        collect_matching_types(locations, &type_index, &mut canonical_cache, seen, target);
      }
    }
  }

  let mut edge_map: BTreeMap<(TypeId, TypeId), Vec<FunctionId>> = BTreeMap::new();
  for func in &functions {
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
    let pairs = function_edge_pairs(&[TypeId(1), TypeId(2)], &[TypeId(3)]);
    assert_eq!(pairs, vec![(TypeId(1), TypeId(3)), (TypeId(2), TypeId(3))]);
  }

  #[test]
  fn function_edge_pairs_skips_self_loops() {
    let pairs = function_edge_pairs(&[TypeId(1), TypeId(2)], &[TypeId(1)]);
    assert_eq!(pairs, vec![(TypeId(2), TypeId(1))]);
  }

  #[test]
  fn function_edge_pairs_connects_params_when_no_output_type() {
    let pairs = function_edge_pairs(&[TypeId(0), TypeId(1), TypeId(2)], &[]);
    assert_eq!(pairs, vec![(TypeId(1), TypeId(0)), (TypeId(2), TypeId(0))]);
  }
}
