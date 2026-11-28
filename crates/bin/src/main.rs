mod app;
mod palettes;

use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use graph::{Graph, TypeRef, parse_workspace, resolve_with_rust_analyzer};
use macros::performance;

fn dump_graph(graph: &Graph) {
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
    let Some(from) = graph.ty(edge.from) else {
      continue;
    };
    let Some(to) = graph.ty(edge.to) else {
      continue;
    };
    let fn_names: Vec<&str> = edge
      .functions
      .iter()
      .filter_map(|&fid| graph.func(fid).map(|f| f.name.as_str()))
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
}

fn main() {
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

  let parsed = match parse_workspace(Path::new(root), &gitignored_files) {
    Ok(parsed) => parsed,
    Err(err) => {
      eprintln!("{err}");
      return;
    }
  };

  let mut graph = Graph::from_parsed(parsed.types, parsed.functions);

  if std::env::args().any(|arg| arg == "--dump") {
    let type_refs: Vec<TypeRef> = graph.type_refs();
    let functions = std::mem::take(&mut graph.functions);
    match performance!(
      resolve_with_rust_analyzer(Path::new(root), &type_refs, functions, Duration::from_secs(60)),
      "lsp: resolve_with_rust_analyzer"
    ) {
      Ok(resolution) => graph.apply_resolution(resolution),
      Err(err) => eprintln!("Failed to resolve graph via LSP: {err}"),
    }

    dump_graph(&graph);
    return;
  }

  app::run(PathBuf::from(root), graph);
}
