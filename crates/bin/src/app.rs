use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::Duration;

use gpui::{
  App, Bounds, Context, FocusHandle, KeyBinding, MouseButton, MouseDownEvent, MouseMoveEvent,
  MouseUpEvent, Pixels, Point, ScrollEvent, Task, TitlebarOptions, Window, WindowBounds,
  WindowOptions, div, linear_color_stop, linear_gradient, point, prelude::*, px, rgb, size,
};
use gpui_platform::application;
use graph::{FunctionInfo, Graph, GraphEdge, GraphResolution, TypeId, TypeNode};
use layout::metrics::{BASE_REM_SIZE, VIEWPORT_OVERSCAN_X_PX, VIEWPORT_OVERSCAN_Y_PX};
use layout::{FileNode, Point2, ViewportRect, build_file_nodes, hit_test_edge, point2};
use lsp::LspClient;
use state::{Camera, InspectorState, LspStatus, Selection};
use ui::actions::{ClearSelection, ResetZoom, ZoomIn, ZoomOut};
use ui::inspector::InspectorEvent;
use ui::top_bar::TopBarEvent;

fn workspace_title(root: &Path) -> String {
  root
    .file_name()
    .and_then(|name| name.to_str())
    .filter(|name| !name.is_empty())
    .unwrap_or("typiewer")
    .to_owned()
}

pub struct Application {
  canonical_root: PathBuf,
  nodes: Vec<FileNode>,
  types: Vec<TypeNode>,
  functions: Vec<FunctionInfo>,
  edges: Vec<GraphEdge>,
  directed_pairs: HashSet<(TypeId, TypeId)>,
  type_anchors: HashMap<TypeId, Point2>,
  selection: Selection,
  inspector: InspectorState,
  top_bar_move_pending: bool,
  lsp_status: LspStatus,
  _lsp_task: Task<()>,
  camera: Camera,
  focus_handle: FocusHandle,
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
      inspector: InspectorState::default(),
      top_bar_move_pending: false,
      lsp_status: LspStatus::Initialising,
      _lsp_task: lsp_task,
      camera: Camera::default(),
      focus_handle: cx.focus_handle(),
    }
  }

  fn set_zoom_around(
    &mut self,
    new_zoom: f32,
    focal: Point2,
    window: &mut Window,
    cx: &mut Context<Self>,
  ) {
    if self.camera.zoom_around(new_zoom, focal) {
      window.set_rem_size(px(self.camera.rem_size()));
      cx.notify();
    }
  }

  fn zoom_in(&mut self, _: &ZoomIn, window: &mut Window, cx: &mut Context<Self>) {
    let viewport = window.viewport_size();
    let center = point2(
      f32::from(viewport.width) * 0.5,
      f32::from(viewport.height) * 0.5,
    );
    self.set_zoom_around(self.camera.zoom * 1.15, center, window, cx);
  }

  fn zoom_out(&mut self, _: &ZoomOut, window: &mut Window, cx: &mut Context<Self>) {
    let viewport = window.viewport_size();
    let center = point2(
      f32::from(viewport.width) * 0.5,
      f32::from(viewport.height) * 0.5,
    );
    self.set_zoom_around(self.camera.zoom / 1.15, center, window, cx);
  }

  fn reset_zoom(&mut self, _: &ResetZoom, window: &mut Window, cx: &mut Context<Self>) {
    self.camera.reset();
    window.set_rem_size(px(BASE_REM_SIZE));
    cx.notify();
  }

  fn clear_selection(&mut self, _: &ClearSelection, _window: &mut Window, cx: &mut Context<Self>) {
    let ended_drag = self.inspector.end_drag();
    if self.selection.clear() || ended_drag {
      cx.notify();
    }
  }

  fn on_type_click(&mut self, id: TypeId, window: &mut Window, cx: &mut Context<Self>) {
    window.focus(&self.focus_handle, cx);
    self.selection.toggle_type(id, &self.edges);
    cx.notify();
  }

  fn on_inspector_event(
    &mut self,
    event: InspectorEvent,
    window: &mut Window,
    cx: &mut Context<Self>,
  ) {
    match event {
      InspectorEvent::StartDrag(cursor) => {
        window.focus(&self.focus_handle, cx);
        let viewport = window.viewport_size();
        self.selection.hovered_edge = None;
        self
          .inspector
          .start_drag(cursor, f32::from(viewport.width), f32::from(viewport.height));
      }
      InspectorEvent::SelectType(id) => {
        window.focus(&self.focus_handle, cx);
        self.inspector.end_drag();
        self.selection.select_type(id, &self.edges);
      }
      InspectorEvent::Close => {
        self.inspector.end_drag();
        self.selection.selected_edge = None;
      }
    }
    cx.notify();
  }

  fn on_top_bar_event(
    &mut self,
    event: TopBarEvent,
    window: &mut Window,
    cx: &mut Context<Self>,
  ) {
    match event {
      TopBarEvent::MouseDown => {
        self.top_bar_move_pending = true;
        cx.stop_propagation();
      }
      TopBarEvent::MouseUp => {
        self.top_bar_move_pending = false;
      }
      TopBarEvent::MouseMove => {
        if self.top_bar_move_pending {
          self.top_bar_move_pending = false;
          window.start_window_move();
          cx.stop_propagation();
          return;
        }

        if !self.camera.is_dragging() && !self.inspector.is_dragging() {
          if self.selection.hovered_edge.take().is_some() {
            cx.notify();
          }
          cx.stop_propagation();
        }
      }
    }
  }

  fn on_mouse_down(
    &mut self,
    event: &MouseDownEvent,
    window: &mut Window,
    cx: &mut Context<Self>,
  ) {
    window.focus(&self.focus_handle, cx);
    let pos = point2(f32::from(event.position.x), f32::from(event.position.y));

    if event.button == MouseButton::Left
      && let Some(edge_id) = hit_test_edge(
        &self.edges,
        &self.type_anchors,
        &self.directed_pairs,
        self.camera.pan,
        self.camera.zoom,
        pos,
      )
    {
      self.selection.toggle_edge(edge_id);
      cx.notify();
      return;
    }

    if event.button == MouseButton::Left || event.button == MouseButton::Middle {
      self.selection.clear();
      self.camera.drag_last_pos = Some(pos);
      cx.notify();
    }
  }

  fn on_mouse_move(
    &mut self,
    event: &MouseMoveEvent,
    window: &mut Window,
    cx: &mut Context<Self>,
  ) {
    let current = point2(f32::from(event.position.x), f32::from(event.position.y));

    if self.inspector.is_dragging() {
      let viewport = window.viewport_size();
      if self.inspector.update_drag(
        current,
        f32::from(viewport.width),
        f32::from(viewport.height),
      ) {
        cx.notify();
      }
      return;
    }

    if let Some(last) = self.camera.drag_last_pos {
      self.camera.pan.x += current.x - last.x;
      self.camera.pan.y += current.y - last.y;
      self.camera.drag_last_pos = Some(current);
      cx.notify();
    } else {
      let hovered = hit_test_edge(
        &self.edges,
        &self.type_anchors,
        &self.directed_pairs,
        self.camera.pan,
        self.camera.zoom,
        current,
      );
      if hovered != self.selection.hovered_edge {
        self.selection.hovered_edge = hovered;
        cx.notify();
      }
    }
  }

  fn on_mouse_up(&mut self, _event: &MouseUpEvent, _window: &mut Window, cx: &mut Context<Self>) {
    let inspector_ended = self.inspector.end_drag();
    let camera_ended = self.camera.drag_last_pos.take().is_some();
    if inspector_ended || camera_ended {
      cx.notify();
    }
  }

  fn on_scroll(&mut self, event: &ScrollEvent, window: &mut Window, cx: &mut Context<Self>) {
    let delta: Point<Pixels> = event.delta.pixel_delta(px(20.0));
    let dx = f32::from(delta.x);
    let dy = f32::from(delta.y);

    if event.modifiers.platform || event.modifiers.control {
      let factor = (1.0 + dy * 0.004).clamp(0.75, 1.25);
      let cursor = point2(f32::from(event.position.x), f32::from(event.position.y));
      self.set_zoom_around(self.camera.zoom * factor, cursor, window, cx);
    } else {
      self.camera.pan.x += dx;
      self.camera.pan.y += dy;
      cx.notify();
    }
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
    let is_dragging = self.camera.is_dragging() || self.inspector.is_dragging();
    let is_hovering_edge = self.selection.hovered_edge.is_some();

    let dot_grid = ui::background::render_dot_grid(self.camera);
    let file_cards = ui::file_card::render_visible_file_cards(
      &self.nodes,
      &self.edges,
      self.camera,
      &self.selection,
      viewport_rect,
      cx,
      Self::on_type_click,
    );
    let edges_layer = ui::edges::render_edges_layer(
      &self.edges,
      &self.type_anchors,
      &self.directed_pairs,
      self.camera,
      &self.selection,
      viewport_rect,
    );
    let inspector_panel = ui::inspector::render_inspector_panel(
      &self.canonical_root,
      &self.types,
      &self.functions,
      &self.edges,
      &self.selection,
      &self.inspector,
      viewport,
      cx,
      Self::on_inspector_event,
    );
    let lsp_status_badge = ui::hud::render_lsp_status(&self.lsp_status);
    let zoom_badge = ui::hud::render_zoom_badge(self.camera.zoom);
    let top_bar = ui::top_bar::render_top_bar(cx, Self::on_top_bar_event);

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
      .child(top_bar)
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

    let canonical_root =
      std::fs::canonicalize(&workspace_root).unwrap_or_else(|_| workspace_root.clone());
    let window_title = workspace_title(&canonical_root);
    let bounds = Bounds::centered(None, size(px(1080.0), px(760.0)), cx);
    if let Err(err) = cx.open_window(
      WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(bounds)),
        titlebar: Some(TitlebarOptions {
          title: Some(window_title.into()),
          appears_transparent: true,
          traffic_light_position: Some(point(px(12.0), px(12.0))),
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
