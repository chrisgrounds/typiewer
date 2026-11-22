use std::{fs, path::PathBuf};

use gpui::{
  App, Bounds, Context, SharedString, Window, WindowBounds, WindowOptions, div, prelude::*, px,
  rgb, size,
};
use gpui_platform::application;
use macros::performance;

use tree_sitter::{Node, Parser};

struct Application {
  text: SharedString,
  items: VecItems,
}

impl Render for Application {
  fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
    div()
      .flex()
      .flex_col()
      .gap_3()
      .bg(rgb(0x505050))
      .size(px(500.0))
      .text_color(rgb(0xffffff))
      .child(format!("Hello, {}!", self.text))
      .children(self.items.iter().map(|item| {
        let label = match item {
          Item::Type { name, kind } => format!("{kind} {name}"),
          Item::Function {
            name,
            params,
            return_type,
          } => match return_type {
            Some(ret) => format!("fn {name}{params} -> {ret}"),
            None => format!("fn {name}{params}"),
          },
        };
        div().child(label)
      }))
  }
}

type VecItems = Vec<Item>;

#[derive(Debug)]
pub enum Item {
  Type {
    name: String,
    kind: String,
  },
  Function {
    name: String,
    params: String,
    return_type: Option<String>,
  },
}

fn node_text<'a>(node: Node, source: &'a str) -> &'a str {
  &source[node.byte_range()]
}

fn walk_tree(node: Node, source: &str, acc: &mut VecItems) {
  match node.kind() {
    "struct_item" | "enum_item" | "trait_item" | "type_item" => {
      if let Some(name) = node.child_by_field_name("name") {
        acc.push(Item::Type {
          kind: node.kind().trim_end_matches("_item").to_string(),
          name: node_text(name, source).to_string(),
        });
      }
    }
    "function_item" => {
      if let Some(name) = node.child_by_field_name("name") {
        let params = node
          .child_by_field_name("parameters")
          .map(|n| node_text(n, source).to_string())
          .unwrap_or_default();

        let return_type = node
          .child_by_field_name("return_type")
          .map(|n| node_text(n, source).to_string());

        acc.push(Item::Function {
          name: node_text(name, source).to_string(),
          params,
          return_type,
        });
      }
    }
    _ => {}
  };

  let mut cursor = node.walk();
  for child in node.children(&mut cursor) {
    walk_tree(child, source, acc);
  }
}

fn collect_source_files(root: &PathBuf, ignore: Vec<String>) -> Vec<String> {
  let mut res = vec![];
  let read_dir_res = fs::read_dir(root).unwrap();
  for maybe_dir_entry in read_dir_res {
    let dir_entry = maybe_dir_entry.unwrap();

    if dir_entry
      .file_name()
      .to_str()
      .is_some_and(|name| ignore.iter().any(|i| i == name))
    {
      continue;
    }

    let path = dir_entry.path();

    if path.is_file() {
      match fs::read_to_string(&path) {
        Ok(source_file) => res.push(source_file),
        Err(_) => {
          // could not parse "../hyperactos/apps/etl-rs/target/debug/incremental/sales-1s038rym6x4gj/s-ho8rwb01sd-14otzvs-f33yv8dh5075gb83q1vn77uve/work-products.bin" because Error { kind: InvalidData, message: "stream did not contain valid UTF-8" }
          // println!("could not parse {:?} because {:?}", path, e);
        }
      }
    }

    if path.is_dir() {
      let mut c = collect_source_files(&path, ignore.clone());
      res.append(c.as_mut())
    }
  }
  res
}

fn main() {
  let mut parser = Parser::new();
  parser
    .set_language(&tree_sitter_rust::LANGUAGE.into())
    .expect("Failed to load Rust grammar");

  let root = ".";
  let gitignore = format!("./.gitignore");

  let gitignored_files: Vec<String> = fs::read_to_string(gitignore)
    .unwrap()
    .lines()
    .map(str::to_owned)
    .collect();

  let files = performance!(
    collect_source_files(&PathBuf::from(root), gitignored_files),
    "collect source files"
  );

  let trees: Vec<_> = performance!(
    files
      .into_iter()
      .map(|f| (parser.parse(&f, None).unwrap(), f))
      .collect(),
    "tree-sitter: parse files into ASTs"
  );

  let mut items = Vec::new();

  performance!(
    for (tree, source) in &trees {
      walk_tree(tree.root_node(), source, &mut items);
    },
    "tree-sitter: walk_tree"
  );

  println!("{:?}", items);

  application().run(|cx: &mut App| {
    cx.on_window_closed(|cx, _window_id| {
      if cx.windows().is_empty() {
        cx.quit();
      }
    })
    .detach();

    let bounds = Bounds::centered(None, size(px(500.), px(500.0)), cx);
    if let Err(err) = cx.open_window(
      WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(bounds)),
        ..Default::default()
      },
      |_, cx| {
        cx.new(|_| Application {
          text: "World".into(),
          items,
        })
      },
    ) {
      eprintln!("Failed to open window: {err}");
    }

    cx.activate(true);
  });
}
