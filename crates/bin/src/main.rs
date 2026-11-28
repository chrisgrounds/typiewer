mod app;
mod palettes;

use std::fs;
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Duration;

use graph::{
  Graph, TypeRef, canonicalize_or_keep, compute_workspace_fingerprint, load_cached_resolution,
  parse_workspace, resolve_with_rust_analyzer, save_cached_resolution,
};
use macros::performance;

const USAGE: &str = "Usage: typiewer [--project <PATH>] [--dump]";

#[derive(Debug, Eq, PartialEq)]
struct CliArgs {
  project: PathBuf,
  dump: bool,
  help: bool,
}

fn parse_args<I, S>(args: I) -> Result<CliArgs, String>
where
  I: IntoIterator<Item = S>,
  S: Into<String>,
{
  let mut project = PathBuf::from(".");
  let mut dump = false;
  let mut help = false;

  let mut iter = args.into_iter().map(Into::into);
  while let Some(arg) = iter.next() {
    match arg.as_str() {
      "--dump" => dump = true,
      "-h" | "--help" => help = true,
      "--project" => {
        let Some(value) = iter.next().filter(|v| !v.is_empty()) else {
          return Err("--project requires a path argument".to_owned());
        };
        project = PathBuf::from(value);
      }
      _ if arg.starts_with("--project=") => {
        let value = &arg["--project=".len()..];
        if value.is_empty() {
          return Err("--project requires a path argument".to_owned());
        }
        project = PathBuf::from(value);
      }
      _ => return Err(format!("Unknown argument: {arg}")),
    }
  }

  Ok(CliArgs {
    project,
    dump,
    help,
  })
}

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

fn main() -> ExitCode {
  let cli = match parse_args(std::env::args().skip(1)) {
    Ok(cli) => cli,
    Err(err) => {
      eprintln!("Error: {err}\n{USAGE}");
      return ExitCode::FAILURE;
    }
  };

  if cli.help {
    println!("{USAGE}");
    return ExitCode::SUCCESS;
  }

  let root = cli.project;
  if !root.is_dir() {
    eprintln!(
      "Error: project path '{}' does not exist or is not a directory",
      root.display()
    );
    return ExitCode::FAILURE;
  }

  let gitignore = root.join(".gitignore");

  let gitignored_files: Vec<String> = performance!(
    fs::read_to_string(&gitignore)
      .unwrap_or_default()
      .lines()
      .map(str::to_owned)
      .collect(),
    "parse gitignore"
  );

  let parsed = match parse_workspace(&root, &gitignored_files) {
    Ok(parsed) => parsed,
    Err(err) => {
      eprintln!("{err}");
      return ExitCode::FAILURE;
    }
  };

  let mut graph = Graph::from_parsed(parsed.types, parsed.functions);

  if cli.dump {
    let canonical_root = canonicalize_or_keep(&root);
    let fingerprint = compute_workspace_fingerprint(&canonical_root, &graph);
    if let Some(cached) = load_cached_resolution(
      &canonical_root,
      fingerprint,
      &graph.functions,
      graph.types.len(),
    ) {
      graph.apply_resolution(cached);
    } else {
      let type_refs: Vec<TypeRef> = graph.type_refs();
      let functions = std::mem::take(&mut graph.functions);
      match performance!(
        resolve_with_rust_analyzer(
          &canonical_root,
          &type_refs,
          functions,
          Duration::from_secs(60),
        ),
        "lsp: resolve_with_rust_analyzer"
      ) {
        Ok(resolution) => {
          save_cached_resolution(&canonical_root, fingerprint, &resolution);
          graph.apply_resolution(resolution);
        }
        Err(err) => eprintln!("Failed to resolve graph via LSP: {err}"),
      }
    }

    dump_graph(&graph);
    return ExitCode::SUCCESS;
  }

  app::run(root, graph);
  ExitCode::SUCCESS
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn parse_args_defaults_to_current_dir() {
    assert_eq!(
      parse_args(Vec::<String>::new()),
      Ok(CliArgs {
        project: PathBuf::from("."),
        dump: false,
        help: false,
      })
    );
  }

  #[test]
  fn parse_args_accepts_project_space_and_equals() {
    assert_eq!(
      parse_args(["--project", "/tmp/my-repo", "--dump"]),
      Ok(CliArgs {
        project: PathBuf::from("/tmp/my-repo"),
        dump: true,
        help: false,
      })
    );
    assert_eq!(
      parse_args(["--project=/tmp/other-repo"]),
      Ok(CliArgs {
        project: PathBuf::from("/tmp/other-repo"),
        dump: false,
        help: false,
      })
    );
  }

  #[test]
  fn parse_args_rejects_missing_project_value_and_unknown_args() {
    assert!(parse_args(["--project"]).is_err());
    assert!(parse_args(["--project="]).is_err());
    assert!(parse_args(["--unknown"]).is_err());
  }
}
