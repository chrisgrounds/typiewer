use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::Duration;

use command_palette::{CommandItem, CommandPalette, CommandPaletteEvent, ToggleCommandPalette};
use finder::{FinderEvent, ToggleFinder, TypeFinder};
use gpui::{
  App, Bounds, Context, Entity, FocusHandle, KeyBinding, MouseButton, MouseDownEvent,
  MouseMoveEvent, MouseUpEvent, Pixels, Point, ScrollEvent, Subscription, Task, TitlebarOptions,
  Window, WindowBounds, WindowOptions, div, linear_color_stop, linear_gradient, point, prelude::*,
  px, rgb, size,
};
use gpui_platform::application;
use graph::{FunctionInfo, Graph, GraphEdge, GraphResolution, TypeId, TypeNode};
use layout::metrics::{BASE_REM_SIZE, VIEWPORT_OVERSCAN_X_PX, VIEWPORT_OVERSCAN_Y_PX};
use layout::{
  CollapsedTarget, CrateNode, Point2, ViewportRect, build_crate_nodes, hit_test_edge, point2,
  recompute_layout,
};
use lsp::LspClient;
use state::{Camera, InspectorState, LspStatus, Selection, ViewerSettings};
use ui::actions::{ClearSelection, ResetZoom, ZoomIn, ZoomOut};
use ui::file_card::FileCardEvent;
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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum AppCommand {
  ToggleDotGrid,
  ToggleEdges,
  ToggleZoomBadge,
  OpenTypeFinder,
  ExpandAll,
  CollapseAllFiles,
  CollapseAllCrates,
  ZoomIn,
  ZoomOut,
  ResetZoom,
  ClearSelection,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
enum ActiveOverlay {
  #[default]
  None,
  TypeFinder,
  CommandPalette,
}

fn on_off(value: bool) -> &'static str {
  if value { "On" } else { "Off" }
}

fn build_command_items(settings: ViewerSettings) -> Vec<CommandItem<AppCommand>> {
  vec![
    CommandItem::new(AppCommand::ToggleDotGrid, "Settings", "Toggle Dot Grid")
      .with_detail(on_off(settings.show_dot_grid))
      .with_keywords("background canvas dots grid"),
    CommandItem::new(
      AppCommand::ToggleEdges,
      "Settings",
      "Toggle Function Edges",
    )
    .with_detail(on_off(settings.show_edges))
    .with_keywords("arrows lines connections graph"),
    CommandItem::new(
      AppCommand::ToggleZoomBadge,
      "Settings",
      "Toggle Zoom Indicator",
    )
    .with_detail(on_off(settings.show_zoom_badge))
    .with_keywords("hud badge percentage"),
    CommandItem::new(AppCommand::OpenTypeFinder, "Navigation", "Go to Type...")
      .with_detail("⌘P")
      .with_keywords("find search jump struct enum trait"),
    CommandItem::new(
      AppCommand::ExpandAll,
      "Layout",
      "Expand All Crates & Files",
    )
    .with_keywords("unfold open show"),
    CommandItem::new(AppCommand::CollapseAllFiles, "Layout", "Collapse All Files")
      .with_keywords("fold hide"),
    CommandItem::new(
      AppCommand::CollapseAllCrates,
      "Layout",
      "Collapse All Crates",
    )
    .with_keywords("fold hide"),
    CommandItem::new(AppCommand::ZoomIn, "View", "Zoom In")
      .with_detail("⌘+")
      .with_keywords("scale magnify"),
    CommandItem::new(AppCommand::ZoomOut, "View", "Zoom Out")
      .with_detail("⌘-")
      .with_keywords("scale shrink"),
    CommandItem::new(AppCommand::ResetZoom, "View", "Reset Zoom & Pan")
      .with_detail("⌘0")
      .with_keywords("camera center default 100%"),
    CommandItem::new(AppCommand::ClearSelection, "Selection", "Clear Selection")
      .with_detail("Esc")
      .with_keywords("deselect close inspector"),
  ]
}

pub struct Application {
  canonical_root: PathBuf,
  crates: Vec<CrateNode>,
  types: Vec<TypeNode>,
  functions: Vec<FunctionInfo>,
  edges: Vec<GraphEdge>,
  directed_pairs: HashSet<(TypeId, TypeId)>,
  type_anchors: HashMap<TypeId, Point2>,
  collapsed_targets: HashMap<TypeId, CollapsedTarget>,
  selection: Selection,
  inspector: InspectorState,
  settings: ViewerSettings,
  top_bar_move_pending: bool,
  lsp_status: LspStatus,
  _lsp_task: Task<()>,
  camera: Camera,
  focus_handle: FocusHandle,
  finder: Entity<TypeFinder>,
  command_palette: Entity<CommandPalette<AppCommand>>,
  active_overlay: ActiveOverlay,
  _finder_subscription: Subscription,
  _command_palette_subscription: Subscription,
}

impl Application {
  fn new(
    workspace_root: PathBuf,
    graph: Graph,
    window: &mut Window,
    cx: &mut Context<Self>,
  ) -> Self {
    let canonical_root =
      std::fs::canonicalize(&workspace_root).unwrap_or_else(|_| workspace_root.clone());
    let types_for_lsp = graph.types.clone();
    let functions_for_lsp = graph.functions.clone();
    let all_types = graph.types.clone();

    let (crates, layout) = build_crate_nodes(&canonical_root, graph.types);

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
    let settings = ViewerSettings::default();
    let finder = cx.new(|cx| TypeFinder::new(&canonical_root, &all_types, cx));
    let finder_subscription = cx.subscribe_in(&finder, window, Self::on_finder_event);
    let command_palette = cx.new(|cx| CommandPalette::new(build_command_items(settings), cx));
    let command_palette_subscription =
      cx.subscribe_in(&command_palette, window, Self::on_command_palette_event);

    Self {
      canonical_root,
      crates,
      types: all_types,
      functions: graph.functions,
      edges: graph.edges,
      directed_pairs,
      type_anchors: layout.type_anchors,
      collapsed_targets: layout.collapsed_targets,
      selection: Selection::default(),
      inspector: InspectorState::default(),
      settings,
      top_bar_move_pending: false,
      lsp_status: LspStatus::Initialising,
      _lsp_task: lsp_task,
      camera: Camera::default(),
      focus_handle: cx.focus_handle(),
      finder,
      command_palette,
      active_overlay: ActiveOverlay::None,
      _finder_subscription: finder_subscription,
      _command_palette_subscription: command_palette_subscription,
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

  fn open_finder(&mut self, window: &mut Window, cx: &mut Context<Self>) {
    self.active_overlay = ActiveOverlay::TypeFinder;
    self.inspector.end_drag();
    self.camera.drag_last_pos = None;
    self
      .finder
      .update(cx, |finder, cx| finder.reset(window, cx));
  }

  fn toggle_finder(&mut self, _: &ToggleFinder, window: &mut Window, cx: &mut Context<Self>) {
    if self.active_overlay == ActiveOverlay::TypeFinder {
      self.active_overlay = ActiveOverlay::None;
      window.focus(&self.focus_handle, cx);
    } else {
      self.open_finder(window, cx);
    }
    cx.notify();
  }

  fn toggle_command_palette(
    &mut self,
    _: &ToggleCommandPalette,
    window: &mut Window,
    cx: &mut Context<Self>,
  ) {
    if self.active_overlay == ActiveOverlay::CommandPalette {
      self.active_overlay = ActiveOverlay::None;
      window.focus(&self.focus_handle, cx);
    } else {
      self.active_overlay = ActiveOverlay::CommandPalette;
      self.inspector.end_drag();
      self.camera.drag_last_pos = None;
      let items = build_command_items(self.settings);
      self.command_palette.update(cx, |palette, cx| {
        palette.set_commands(items, cx);
        palette.reset(window, cx);
      });
    }
    cx.notify();
  }

  fn on_finder_event(
    &mut self,
    _finder: &Entity<TypeFinder>,
    event: &FinderEvent,
    window: &mut Window,
    cx: &mut Context<Self>,
  ) {
    match *event {
      FinderEvent::SelectType(type_id) => {
        self.active_overlay = ActiveOverlay::None;
        self.inspector.end_drag();
        self.ensure_type_expanded(type_id);
        self.selection.select_type(type_id, &self.edges);
        if let Some(&anchor) = self.type_anchors.get(&type_id) {
          let viewport = window.viewport_size();
          self.camera.focus_on(anchor, f32::from(viewport.height));
        }
        window.focus(&self.focus_handle, cx);
        cx.notify();
      }
      FinderEvent::Close => {
        self.active_overlay = ActiveOverlay::None;
        window.focus(&self.focus_handle, cx);
        cx.notify();
      }
    }
  }

  fn on_command_palette_event(
    &mut self,
    _palette: &Entity<CommandPalette<AppCommand>>,
    event: &CommandPaletteEvent<AppCommand>,
    window: &mut Window,
    cx: &mut Context<Self>,
  ) {
    match *event {
      CommandPaletteEvent::RunCommand(cmd) => {
        self.active_overlay = ActiveOverlay::None;
        window.focus(&self.focus_handle, cx);
        match cmd {
          AppCommand::ToggleDotGrid => {
            self.settings.show_dot_grid = !self.settings.show_dot_grid;
            cx.notify();
          }
          AppCommand::ToggleEdges => {
            self.settings.show_edges = !self.settings.show_edges;
            if !self.settings.show_edges {
              self.selection.hovered_edge = None;
            }
            cx.notify();
          }
          AppCommand::ToggleZoomBadge => {
            self.settings.show_zoom_badge = !self.settings.show_zoom_badge;
            cx.notify();
          }
          AppCommand::OpenTypeFinder => {
            self.open_finder(window, cx);
            cx.notify();
          }
          AppCommand::ExpandAll => {
            for crate_node in &mut self.crates {
              crate_node.collapsed = false;
              for file_node in &mut crate_node.files {
                file_node.collapsed = false;
              }
            }
            self.relayout();
            cx.notify();
          }
          AppCommand::CollapseAllFiles => {
            for crate_node in &mut self.crates {
              for file_node in &mut crate_node.files {
                file_node.collapsed = true;
              }
            }
            self.relayout();
            cx.notify();
          }
          AppCommand::CollapseAllCrates => {
            for crate_node in &mut self.crates {
              crate_node.collapsed = true;
            }
            self.relayout();
            cx.notify();
          }
          AppCommand::ZoomIn => self.zoom_in(&ZoomIn, window, cx),
          AppCommand::ZoomOut => self.zoom_out(&ZoomOut, window, cx),
          AppCommand::ResetZoom => self.reset_zoom(&ResetZoom, window, cx),
          AppCommand::ClearSelection => self.clear_selection(&ClearSelection, window, cx),
        }
      }
      CommandPaletteEvent::Close => {
        self.active_overlay = ActiveOverlay::None;
        window.focus(&self.focus_handle, cx);
        cx.notify();
      }
    }
  }

  fn ensure_type_expanded(&mut self, type_id: TypeId) {
    let mut changed = false;
    for crate_node in &mut self.crates {
      for file_node in &mut crate_node.files {
        if file_node.types.iter().any(|ty| ty.id == type_id) {
          if crate_node.collapsed {
            crate_node.collapsed = false;
            changed = true;
          }
          if file_node.collapsed {
            file_node.collapsed = false;
            changed = true;
          }
          if changed {
            self.relayout();
          }
          return;
        }
      }
    }
  }

  fn toggle_crate_collapsed(&mut self, crate_idx: usize) {
    if let Some(crate_node) = self.crates.get_mut(crate_idx) {
      crate_node.collapsed = !crate_node.collapsed;
      self.relayout();
    }
  }

  fn toggle_file_collapsed(&mut self, crate_idx: usize, file_idx: usize) {
    if let Some(file_node) = self
      .crates
      .get_mut(crate_idx)
      .and_then(|c| c.files.get_mut(file_idx))
    {
      file_node.collapsed = !file_node.collapsed;
      self.relayout();
    }
  }

  fn relayout(&mut self) {
    let layout = recompute_layout(&mut self.crates);
    self.type_anchors = layout.type_anchors;
    self.collapsed_targets = layout.collapsed_targets;
    self.selection.hovered_edge = None;
  }

  fn on_file_card_event(
    &mut self,
    event: FileCardEvent,
    window: &mut Window,
    cx: &mut Context<Self>,
  ) {
    window.focus(&self.focus_handle, cx);
    match event {
      FileCardEvent::SelectType(id) => {
        self.selection.toggle_type(id, &self.edges);
      }
      FileCardEvent::ToggleCrate(crate_idx) => {
        self.toggle_crate_collapsed(crate_idx);
      }
      FileCardEvent::ToggleFile(crate_idx, file_idx) => {
        self.toggle_file_collapsed(crate_idx, file_idx);
      }
    }
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

    if self.settings.show_edges
      && event.button == MouseButton::Left
      && let Some(edge_id) = hit_test_edge(
        &self.edges,
        &self.type_anchors,
        &self.collapsed_targets,
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
    } else if self.settings.show_edges {
      let hovered = hit_test_edge(
        &self.edges,
        &self.type_anchors,
        &self.collapsed_targets,
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

    let dot_grid = self
      .settings
      .show_dot_grid
      .then(|| ui::background::render_dot_grid(self.camera));
    let file_cards = ui::file_card::render_visible_file_cards(
      &self.crates,
      &self.edges,
      self.camera,
      &self.selection,
      viewport_rect,
      cx,
      Self::on_file_card_event,
    );
    let edges_layer = self.settings.show_edges.then(|| {
      ui::edges::render_edges_layer(
        &self.edges,
        &self.type_anchors,
        &self.collapsed_targets,
        &self.directed_pairs,
        self.camera,
        &self.selection,
        viewport_rect,
      )
    });
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
    let zoom_badge = self
      .settings
      .show_zoom_badge
      .then(|| ui::hud::render_zoom_badge(self.camera.zoom));
    let top_bar = ui::top_bar::render_top_bar(cx, Self::on_top_bar_event);

    div()
      .id("canvas-root")
      .track_focus(&self.focus_handle)
      .on_action(cx.listener(Self::zoom_in))
      .on_action(cx.listener(Self::zoom_out))
      .on_action(cx.listener(Self::reset_zoom))
      .on_action(cx.listener(Self::clear_selection))
      .on_action(cx.listener(Self::toggle_finder))
      .on_action(cx.listener(Self::toggle_command_palette))
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
      .when_some(dot_grid, |el, grid| el.child(grid))
      .children(file_cards)
      .when_some(edges_layer, |el, edges| el.child(edges))
      .when_some(inspector_panel, |el, panel| el.child(panel))
      .when_some(lsp_status_badge, |el, badge| el.child(badge))
      .when_some(zoom_badge, |el, badge| el.child(badge))
      .child(top_bar)
      .when(self.active_overlay == ActiveOverlay::TypeFinder, |el| {
        el.child(self.finder.clone())
      })
      .when(self.active_overlay == ActiveOverlay::CommandPalette, |el| {
        el.child(self.command_palette.clone())
      })
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
    finder::bind_keys(cx);
    command_palette::bind_keys(cx);

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
        let view = cx.new(|cx| Application::new(workspace_root, graph, window, cx));
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
