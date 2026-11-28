mod app;
mod camera;
mod geometry;
mod layout;
mod theme;
mod ui;

use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use graph::{Graph, parse_workspace};
use lsp::LspClient;
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

  if std::env::args().any(|arg| arg == "--dump") {
    let graph = match LspClient::start_rust_analyzer(root) {
      Ok(lsp) => {
        let _ = performance!(
          lsp.wait_until_ready(Duration::from_secs(60)),
          "lsp: wait_until_ready"
        );
        let graph = match performance!(
          Graph::new(parsed.types, parsed.functions, &lsp),
          "lsp: Graph::new"
        ) {
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
        Graph::from_parsed(parsed.types, parsed.functions)
      }
    };

    dump_graph(&graph);
    return;
  }

  let graph = Graph::from_parsed(parsed.types, parsed.functions);
  app::run(PathBuf::from(root), graph);
}
