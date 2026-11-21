use std::{fs, path::PathBuf};

use gpui::{
  App, Bounds, Context, SharedString, Window, WindowBounds, WindowOptions, div, prelude::*, px,
  rgb, size,
};
use gpui_platform::application;

use tree_sitter::{Node, Parser};

struct HelloWorld {
  text: SharedString,
}

impl Render for HelloWorld {
  fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
    div()
      .flex()
      .flex_col()
      .gap_3()
      .bg(rgb(0x505050))
      .size(px(500.0))
      .justify_center()
      .items_center()
      .shadow_lg()
      .border_1()
      .border_color(rgb(0x0000ff))
      .text_xl()
      .text_color(rgb(0xffffff))
      .child(format!("Hello, {}!", self.text))
      .child(
        div()
          .flex()
          .gap_2()
          .child(div().size_8().bg(gpui::red()))
          .child(div().size_8().bg(gpui::green()))
          .child(div().size_8().bg(gpui::blue()))
          .child(div().size_8().bg(gpui::yellow()))
          .child(div().size_8().bg(gpui::black()))
          .child(div().size_8().bg(gpui::white())),
      )
  }
}

macro_rules! performance {
  ($content:expr, $name:expr) => {{
    let start = std::time::Instant::now();
    let res = $content;
    println!("[{}] Time taken: {:?}", $name, start.elapsed());
    res
  }};
  ($content:expr) => {
    performance!($content, stringify!($content))
  };
}

fn walk_tree(node: Node, source: &str, depth: usize) {
  let indent = "  ".repeat(depth);
  // println!("{}{}: {:?}", indent, node.kind(), &source[node.byte_range()]);

  let mut cursor = node.walk();
  for child in node.children(&mut cursor) {
    walk_tree(child, source, depth + 1);
  }
}

fn collect_source_files(root: &PathBuf) -> Vec<String> {
  let mut res = vec![];
  let r = fs::read_dir(root).unwrap();
  for a in r {
    let dir_entry = a.unwrap();

    // we want to skip these as we don't want to parse them
    if dir_entry.file_name() == "target" {
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
      let mut c = collect_source_files(&path);
      res.append(c.as_mut())
    }
  }
  res
}

fn main() {
  // load all of the files from the workspace
  // use tree-sitter for working out what types and functions exist
  // build up directed graph
  // render

  let mut parser = Parser::new();
  parser
    .set_language(&tree_sitter_rust::LANGUAGE.into())
    .expect("Failed to load Rust grammar");

  let root = "../hyperactos/apps/etl-rs";

  let files = performance!(
    collect_source_files(&PathBuf::from(root)),
    "collect source files"
  );

  println!("{:?}", files);

  let trees: Vec<_> = performance!(
    files
      .into_iter()
      .map(|f| (parser.parse(&f, None).unwrap(), f))
      .collect(),
    "tree-sitter: parse files into ASTs"
  );

  let _res: Vec<_> = performance!(
    trees
      .into_iter()
      .map(|(tree, source)| walk_tree(tree.root_node(), &source, 0))
      .collect(),
    "tree-sitter: walk_tree"
  );

  application().run(|cx: &mut App| {
    let bounds = Bounds::centered(None, size(px(500.), px(500.0)), cx);
    if let Err(err) = cx.open_window(
      WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(bounds)),
        ..Default::default()
      },
      |_, cx| {
        cx.new(|_| HelloWorld {
          text: "World".into(),
        })
      },
    ) {
      eprintln!("Failed to open window: {err}");
    }
  });
}
