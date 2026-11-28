use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::Duration;

use gpui::{
  App, Bounds, Context, FocusHandle, KeyBinding, MouseButton, Point, Task, TitlebarOptions, Window,
  WindowBounds, WindowOptions, div, linear_color_stop, linear_gradient, prelude::*, px, rgb, size,
};
use gpui_platform::application;
use graph::{EdgeId, FunctionInfo, Graph, GraphEdge, GraphResolution, TypeId, TypeNode};
use lsp::LspClient;

use crate::camera::{Camera, ClearSelection, ResetZoom, ZoomIn, ZoomOut};
use crate::geometry::ViewportRect;
use crate::layout::{FileNode, build_file_nodes};
use crate::theme::{VIEWPORT_OVERSCAN_X_PX, VIEWPORT_OVERSCAN_Y_PX};
use crate::ui;

pub enum LspStatus {
  Initialising,
  Ready,
  Failed(String),
}

#[derive(Default)]
pub struct Selection {
  pub selected_type: Option<TypeId>,
  pub selected_edge: Option<EdgeId>,
  pub hovered_edge: Option<EdgeId>,
  pub connected_types: HashSet<TypeId>,
}

impl Selection {
  pub fn select_type(&mut self, id: TypeId, edges: &[GraphEdge]) {
    self.selected_edge = None;
    self.selected_type = Some(id);
    self.refresh_connected_types(edges);
  }

  pub fn toggle_type(&mut self, id: TypeId, edges: &[GraphEdge]) {
    self.selected_edge = None;
    self.selected_type = if self.selected_type == Some(id) {
      None
    } else {
      Some(id)
    };
    self.refresh_connected_types(edges);
  }

  pub fn toggle_edge(&mut self, id: EdgeId) {
    self.selected_type = None;
    self.connected_types.clear();
    self.selected_edge = if self.selected_edge == Some(id) {
      None
    } else {
      Some(id)
    };
  }

  pub fn clear(&mut self) -> bool {
    if self.selected_type.is_some() || self.selected_edge.is_some() {
      self.selected_type = None;
      self.selected_edge = None;
      self.connected_types.clear();
      true
    } else {
      false
    }
  }

  pub fn refresh_connected_types(&mut self, edges: &[GraphEdge]) {
    self.connected_types.clear();
    if let Some(sel) = self.selected_type {
      for edge in edges {
        if edge.from == sel {
          self.connected_types.insert(edge.to);
        } else if edge.to == sel {
          self.connected_types.insert(edge.from);
        }
      }
    }
  }
}

pub struct Application {
  pub(crate) canonical_root: PathBuf,
  pub(crate) nodes: Vec<FileNode>,
  pub(crate) types: Vec<TypeNode>,
  pub(crate) functions: Vec<FunctionInfo>,
  pub(crate) edges: Vec<GraphEdge>,
  pub(crate) directed_pairs: HashSet<(TypeId, TypeId)>,
  pub(crate) type_anchors: HashMap<TypeId, Point<f32>>,
  pub(crate) selection: Selection,
  pub(crate) lsp_status: LspStatus,
  _lsp_task: Task<()>,
  pub(crate) camera: Camera,
  pub(crate) focus_handle: FocusHandle,
}

impl Application {
  fn new(workspace_root: PathBuf, graph: Graph, cx: &mut Context<Self>) -> Self {
    let canonical_root =
      std::fs::canonicalize(&workspace_root).unwrap_or_else(|_| workspace_root.clone());
    let types_for_lsp = graph.types.clone();
    let functions_for_lsp = graph.functions.clone();
    let all_types = graph.types.clone();

    let (nodes, type_anchors) = build_file_nodes(&canonical_root, graph.types);

    let lsp_task = cx.spawn(async move |this, cx| {
      let result = cx
        .background_executor()
        .spawn(async move {
          let lsp = LspClient::start_rust_analyzer(&workspace_root)?;
          lsp.wait_until_ready(Duration::from_secs(60))?;
          let resolution = Graph::resolve_edges(&types_for_lsp, &functions_for_lsp, &lsp);
          let _ = lsp.shutdown();
          Ok::<GraphResolution, lsp::LspError>(resolution)
        })
        .await;

      let _ = this.update(cx, |app, cx| {
        match result {
          Ok(resolution) => {
            app.directed_pairs = resolution.edges.iter().map(|e| (e.from, e.to)).collect();
            app.edges = resolution.edges;
            app.functions = resolution.functions;
            app.selection.refresh_connected_types(&app.edges);
            app.lsp_status = LspStatus::Ready;
          }
          Err(err) => {
            app.lsp_status = LspStatus::Failed(err.to_string());
          }
        }
        cx.notify();
      });
    });

    let directed_pairs = graph.edges.iter().map(|e| (e.from, e.to)).collect();

    Self {
      canonical_root,
      nodes,
      types: all_types,
      functions: graph.functions,
      edges: graph.edges,
      directed_pairs,
      type_anchors,
      selection: Selection::default(),
      lsp_status: LspStatus::Initialising,
      _lsp_task: lsp_task,
      camera: Camera::default(),
      focus_handle: cx.focus_handle(),
    }
  }

  pub(crate) fn relative_display_path(&self, path: &Path) -> String {
    path
      .strip_prefix(&self.canonical_root)
      .unwrap_or(path)
      .display()
      .to_string()
  }
}

impl Render for Application {
  fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
    let viewport = window.viewport_size();
    let viewport_rect = ViewportRect::from_size(
      f32::from(viewport.width),
      f32::from(viewport.height),
      VIEWPORT_OVERSCAN_X_PX,
      VIEWPORT_OVERSCAN_Y_PX,
    );
    let is_dragging = self.camera.is_dragging();
    let is_hovering_edge = self.selection.hovered_edge.is_some();

    let dot_grid = ui::background::render_dot_grid(self.camera);
    let file_cards = self.render_visible_file_cards(viewport_rect, cx);
    let edges_layer = self.render_edges_layer(viewport_rect);
    let inspector_panel = self.render_inspector_panel(cx);
    let lsp_status_badge = ui::hud::render_lsp_status(&self.lsp_status);
    let zoom_badge = ui::hud::render_zoom_badge(self.camera.zoom);

    div()
      .id("canvas-root")
      .track_focus(&self.focus_handle)
      .on_action(cx.listener(Self::zoom_in))
      .on_action(cx.listener(Self::zoom_out))
      .on_action(cx.listener(Self::reset_zoom))
      .on_action(cx.listener(Self::clear_selection))
      .on_mouse_down(MouseButton::Left, cx.listener(Self::on_mouse_down))
      .on_mouse_down(MouseButton::Middle, cx.listener(Self::on_mouse_down))
      .on_mouse_move(cx.listener(Self::on_mouse_move))
      .on_mouse_up(MouseButton::Left, cx.listener(Self::on_mouse_up))
      .on_mouse_up(MouseButton::Middle, cx.listener(Self::on_mouse_up))
      .on_mouse_up_out(MouseButton::Left, cx.listener(Self::on_mouse_up))
      .on_scroll(cx.listener(Self::on_scroll))
      .relative()
      .size_full()
      .overflow_hidden()
      .when(is_dragging, |el| el.cursor_grabbing())
      .when(!is_dragging && is_hovering_edge, |el| el.cursor_pointer())
      .when(!is_dragging && !is_hovering_edge, |el| el.cursor_grab())
      .bg(linear_gradient(
        180.,
        linear_color_stop(rgb(0x0d0e14), 0.0),
        linear_color_stop(rgb(0x040406), 1.0),
      ))
      .text_color(rgb(0xf4f6fb))
      .child(dot_grid)
      .children(file_cards)
      .child(edges_layer)
      .when_some(inspector_panel, |el, panel| el.child(panel))
      .when_some(lsp_status_badge, |el, badge| el.child(badge))
      .child(zoom_badge)
  }
}

pub fn run(workspace_root: PathBuf, graph: Graph) {
  application().run(move |cx: &mut App| {
    cx.bind_keys([
      KeyBinding::new("cmd-=", ZoomIn, None),
      KeyBinding::new("cmd-+", ZoomIn, None),
      KeyBinding::new("cmd--", ZoomOut, None),
      KeyBinding::new("cmd-0", ResetZoom, None),
      KeyBinding::new("escape", ClearSelection, None),
    ]);

    cx.on_window_closed(|cx, _window_id| {
      if cx.windows().is_empty() {
        cx.quit();
      }
    })
    .detach();

    let bounds = Bounds::centered(None, size(px(1080.0), px(760.0)), cx);
    if let Err(err) = cx.open_window(
      WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(bounds)),
        titlebar: Some(TitlebarOptions {
          appears_transparent: true,
          ..Default::default()
        }),
        ..Default::default()
      },
      |window, cx| {
        let view = cx.new(|cx| Application::new(workspace_root, graph, cx));
        let focus = view.read(cx).focus_handle.clone();
        window.focus(&focus, cx);
        view
      },
    ) {
      eprintln!("Failed to open window: {err}");
    }

    cx.activate(true);
  });
}
