use std::path::{Path, PathBuf};
use std::time::Duration;

use gpui::{
  App, Bounds, Context, Entity, FocusHandle, KeyBinding, KeyDownEvent, MouseButton, MouseDownEvent,
  MouseMoveEvent, MouseUpEvent, Pixels, Point, ScrollEvent, Subscription, Task, TitlebarOptions,
  Window, WindowBounds, WindowOptions, div, linear_color_stop, linear_gradient, point, prelude::*,
  px, rgb, size,
};
use gpui_platform::application;
use graph::{Graph, TypeId, canonicalize_or_keep, resolve_with_rust_analyzer};
use layout::metrics::{BASE_REM_SIZE, VIEWPORT_OVERSCAN_X_PX, VIEWPORT_OVERSCAN_Y_PX};
use layout::{Point2, Rect, WorkspaceLayout, comment_section_extra_height, point2};
use palette::{FuzzyPalette, PaletteEvent, ToggleCommandPalette, ToggleFinder};
use state::{Camera, CommentsState, InspectorState, LspStatus, Selection, ViewerSettings};
use ui::actions::{ClearSelection, ResetZoom, ZoomIn, ZoomOut};
use ui::file_card::FileCardEvent;
use ui::inspector::InspectorEvent;
use ui::theme::to_point2;
use ui::top_bar::TopBarEvent;
use ui::type_context_menu::TypeContextMenuEvent;

use crate::palettes::{
  AppCommand, COMMAND_PALETTE_CONFIG, TYPE_FINDER_CONFIG, build_command_items,
  build_type_finder_items,
};

fn workspace_title(root: &Path) -> String {
  root
    .file_name()
    .and_then(|name| name.to_str())
    .filter(|name| !name.is_empty())
    .unwrap_or("typiewer")
    .to_owned()
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
enum ActiveOverlay {
  #[default]
  None,
  TypeFinder,
  CommandPalette,
}

pub struct Application {
  canonical_root: PathBuf,
  graph: Graph,
  layout: WorkspaceLayout,
  selection: Selection,
  comments: CommentsState,
  inspector: InspectorState,
  settings: ViewerSettings,
  top_bar_move_pending: bool,
  lsp_status: LspStatus,
  _lsp_task: Task<()>,
  camera: Camera,
  focus_handle: FocusHandle,
  finder: Entity<FuzzyPalette<TypeId>>,
  command_palette: Entity<FuzzyPalette<AppCommand>>,
  active_overlay: ActiveOverlay,
  _finder_subscription: Subscription,
  _command_palette_subscription: Subscription,
}

impl Application {
  fn new(
    canonical_root: PathBuf,
    graph: Graph,
    window: &mut Window,
    cx: &mut Context<Self>,
  ) -> Self {
    let layout = WorkspaceLayout::build(&canonical_root, &graph.types);

    let lsp_root = canonical_root.clone();
    let type_refs = graph.type_refs();
    let functions_for_lsp = graph.functions.clone();

    let lsp_task = cx.spawn(async move |this, cx| {
      let result = cx
        .background_executor()
        .spawn(async move {
          resolve_with_rust_analyzer(
            &lsp_root,
            &type_refs,
            functions_for_lsp,
            Duration::from_secs(60),
          )
        })
        .await;

      let _ = this.update(cx, |app, cx| {
        match result {
          Ok(resolution) => {
            app.graph.apply_resolution(resolution);
            app.selection.refresh_connected_types(&app.graph.edges);
            app.lsp_status = LspStatus::Ready;
          }
          Err(err) => {
            app.lsp_status = LspStatus::Failed(err.to_string());
          }
        }
        cx.notify();
      });
    });

    let settings = ViewerSettings::default();
    let finder_items = build_type_finder_items(&canonical_root, &graph.types);
    let finder = cx.new(|cx| FuzzyPalette::new(TYPE_FINDER_CONFIG, finder_items, cx));
    let finder_subscription = cx.subscribe_in(&finder, window, Self::on_finder_event);

    let command_palette = cx.new(|cx| {
      FuzzyPalette::new(COMMAND_PALETTE_CONFIG, build_command_items(settings), cx)
    });
    let command_palette_subscription =
      cx.subscribe_in(&command_palette, window, Self::on_command_palette_event);

    Self {
      canonical_root,
      graph,
      layout,
      selection: Selection::default(),
      comments: CommentsState::default(),
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

  fn zoom_by(&mut self, factor: f32, window: &mut Window, cx: &mut Context<Self>) {
    let viewport = window.viewport_size();
    let center = point2(
      f32::from(viewport.width) * 0.5,
      f32::from(viewport.height) * 0.5,
    );
    self.set_zoom_around(self.camera.zoom * factor, center, window, cx);
  }

  fn zoom_in(&mut self, _: &ZoomIn, window: &mut Window, cx: &mut Context<Self>) {
    self.zoom_by(1.15, window, cx);
  }

  fn zoom_out(&mut self, _: &ZoomOut, window: &mut Window, cx: &mut Context<Self>) {
    self.zoom_by(1.0 / 1.15, window, cx);
  }

  fn reset_zoom(&mut self, _: &ResetZoom, window: &mut Window, cx: &mut Context<Self>) {
    self.camera.reset();
    window.set_rem_size(px(BASE_REM_SIZE));
    cx.notify();
  }

  fn clear_selection(&mut self, _: &ClearSelection, _window: &mut Window, cx: &mut Context<Self>) {
    if self.cancel_active_comment_draft() {
      cx.notify();
      return;
    }
    let ended_drag = self.inspector.end_drag();
    if self.selection.clear() || ended_drag {
      cx.notify();
    }
  }

  fn sync_comment_layout(&mut self, type_id: TypeId) {
    let extra = comment_section_extra_height(
      self.comments.comments_for(type_id),
      self.comments.active_draft_for(type_id).is_some(),
    );
    if self.layout.set_type_extra_height(type_id, extra) {
      self.selection.hovered_edge = None;
    }
  }

  fn cancel_active_comment_draft(&mut self) -> bool {
    if let Some(type_id) = self.comments.cancel_draft() {
      self.sync_comment_layout(type_id);
      true
    } else {
      false
    }
  }

  fn submit_active_comment_draft(&mut self) -> bool {
    if let Some(type_id) = self.comments.submit_draft() {
      self.sync_comment_layout(type_id);
      true
    } else {
      false
    }
  }

  fn on_key_down(&mut self, event: &KeyDownEvent, _window: &mut Window, cx: &mut Context<Self>) {
    if self.active_overlay != ActiveOverlay::None || !self.comments.is_drafting() {
      return;
    }

    let key = event.keystroke.key.as_str();
    let mods = &event.keystroke.modifiers;

    // Allow command shortcuts (e.g. cmd-p, cmd-+, cmd-0) to propagate normally.
    if mods.platform && key != "backspace" {
      return;
    }

    match key {
      "enter" => {
        cx.stop_propagation();
        if self.submit_active_comment_draft() {
          cx.notify();
        }
      }
      "backspace" => {
        cx.stop_propagation();
        let changed = if mods.platform {
          self.comments.clear_draft_text()
        } else if mods.alt {
          self.comments.delete_last_word()
        } else {
          self.comments.pop_char()
        };
        if changed {
          cx.notify();
        }
      }
      "u" if mods.control => {
        cx.stop_propagation();
        if self.comments.clear_draft_text() {
          cx.notify();
        }
      }
      "w" if mods.control => {
        cx.stop_propagation();
        if self.comments.delete_last_word() {
          cx.notify();
        }
      }
      _ => {
        if !mods.platform
          && !mods.control
          && let Some(key_char) = event.keystroke.key_char.as_deref()
          && !key_char.is_empty()
          && !key_char.chars().any(char::is_control)
        {
          cx.stop_propagation();
          if self.comments.push_str(key_char) {
            cx.notify();
          }
        }
      }
    }
  }

  fn close_overlay(&mut self, window: &mut Window, cx: &mut Context<Self>) {
    self.active_overlay = ActiveOverlay::None;
    window.focus(&self.focus_handle, cx);
  }

  fn prepare_overlay(&mut self, overlay: ActiveOverlay) {
    self.active_overlay = overlay;
    self.inspector.end_drag();
    self.camera.drag_last_pos = None;
    self.selection.hovered_edge = None;
  }

  fn open_finder(&mut self, window: &mut Window, cx: &mut Context<Self>) {
    self.prepare_overlay(ActiveOverlay::TypeFinder);
    self.finder.update(cx, |finder, cx| finder.reset(window, cx));
  }

  fn toggle_finder(&mut self, _: &ToggleFinder, window: &mut Window, cx: &mut Context<Self>) {
    if self.active_overlay == ActiveOverlay::TypeFinder {
      self.close_overlay(window, cx);
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
      self.close_overlay(window, cx);
    } else {
      self.prepare_overlay(ActiveOverlay::CommandPalette);
      let items = build_command_items(self.settings);
      self.command_palette.update(cx, |palette, cx| {
        palette.set_items(items, cx);
        palette.reset(window, cx);
      });
    }
    cx.notify();
  }

  fn jump_to_type(&mut self, type_id: TypeId, window: &mut Window, cx: &mut Context<Self>) {
    self.inspector.end_drag();
    if self.layout.ensure_type_expanded(type_id) {
      self.selection.hovered_edge = None;
    }
    self.selection.select_type(type_id, &self.graph.edges);
    if let Some(anchor) = self.layout.type_anchor(type_id) {
      let viewport = window.viewport_size();
      self.camera.focus_on(anchor, f32::from(viewport.height));
    }
    window.focus(&self.focus_handle, cx);
  }

  fn on_finder_event(
    &mut self,
    _finder: &Entity<FuzzyPalette<TypeId>>,
    event: &PaletteEvent<TypeId>,
    window: &mut Window,
    cx: &mut Context<Self>,
  ) {
    match *event {
      PaletteEvent::Select(type_id) => {
        self.active_overlay = ActiveOverlay::None;
        self.jump_to_type(type_id, window, cx);
      }
      PaletteEvent::Close => self.close_overlay(window, cx),
    }
    cx.notify();
  }

  fn on_command_palette_event(
    &mut self,
    _palette: &Entity<FuzzyPalette<AppCommand>>,
    event: &PaletteEvent<AppCommand>,
    window: &mut Window,
    cx: &mut Context<Self>,
  ) {
    match *event {
      PaletteEvent::Select(cmd) => {
        self.close_overlay(window, cx);
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
            self.layout.expand_all();
            self.selection.hovered_edge = None;
            cx.notify();
          }
          AppCommand::CollapseAllFiles => {
            self.layout.collapse_all_files();
            self.selection.hovered_edge = None;
            cx.notify();
          }
          AppCommand::CollapseAllCrates => {
            self.layout.collapse_all_crates();
            self.selection.hovered_edge = None;
            cx.notify();
          }
          AppCommand::ZoomIn => self.zoom_in(&ZoomIn, window, cx),
          AppCommand::ZoomOut => self.zoom_out(&ZoomOut, window, cx),
          AppCommand::ResetZoom => self.reset_zoom(&ResetZoom, window, cx),
          AppCommand::ClearSelection => self.clear_selection(&ClearSelection, window, cx),
        }
      }
      PaletteEvent::Close => {
        self.close_overlay(window, cx);
        cx.notify();
      }
    }
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
        self.cancel_active_comment_draft();
        self.selection.toggle_type(id, &self.graph.edges);
      }
      FileCardEvent::ToggleCrate(crate_idx) => {
        if self.layout.toggle_crate(crate_idx) {
          self.selection.hovered_edge = None;
        }
      }
      FileCardEvent::ToggleFile(crate_idx, file_idx) => {
        if self.layout.toggle_file(crate_idx, file_idx) {
          self.selection.hovered_edge = None;
        }
      }
      FileCardEvent::SaveComment => {
        self.submit_active_comment_draft();
      }
      FileCardEvent::CancelComment => {
        self.cancel_active_comment_draft();
      }
    }
    cx.notify();
  }

  fn on_type_context_menu_event(
    &mut self,
    event: TypeContextMenuEvent,
    window: &mut Window,
    cx: &mut Context<Self>,
  ) {
    window.focus(&self.focus_handle, cx);
    match event {
      TypeContextMenuEvent::AddComment(type_id) => {
        if let Some(prev) = self.comments.start_draft(type_id) {
          self.sync_comment_layout(prev);
        }
        self.sync_comment_layout(type_id);
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
        self.jump_to_type(id, window, cx);
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
    let pos = to_point2(event.position);

    if self.settings.show_edges
      && event.button == MouseButton::Left
      && let Some(edge_id) = self
        .layout
        .hit_test_edge(&self.graph, self.camera.pan, self.camera.zoom, pos)
    {
      self.selection.toggle_edge(edge_id);
      cx.notify();
      return;
    }

    if event.button == MouseButton::Left || event.button == MouseButton::Middle {
      self.cancel_active_comment_draft();
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
    if self.active_overlay != ActiveOverlay::None {
      return;
    }

    let current = to_point2(event.position);

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
      let hovered = self
        .layout
        .hit_test_edge(&self.graph, self.camera.pan, self.camera.zoom, current);
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
      self.set_zoom_around(self.camera.zoom * factor, to_point2(event.position), window, cx);
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
    let viewport_rect = Rect::from_viewport(
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
      &self.graph,
      &self.layout,
      self.camera,
      &self.selection,
      &self.comments,
      viewport_rect,
      cx,
      Self::on_file_card_event,
    );
    let edges_layer = self.settings.show_edges.then(|| {
      ui::edges::render_edges_layer(
        &self.graph,
        &self.layout,
        self.camera,
        &self.selection,
        viewport_rect,
      )
    });
    let inspector_panel = ui::inspector::render_inspector_panel(
      &self.canonical_root,
      &self.graph,
      &self.selection,
      &self.inspector,
      viewport,
      cx,
      Self::on_inspector_event,
    );
    let type_context_menu = ui::type_context_menu::render_type_context_menu(
      &self.graph,
      &self.layout,
      self.camera,
      &self.selection,
      viewport,
      cx,
      Self::on_type_context_menu_event,
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
      .on_key_down(cx.listener(Self::on_key_down))
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
      .when_some(type_context_menu, |el, menu| el.child(menu))
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
    palette::bind_keys(cx);

    cx.on_window_closed(|cx, _window_id| {
      if cx.windows().is_empty() {
        cx.quit();
      }
    })
    .detach();

    let canonical_root = canonicalize_or_keep(&workspace_root);
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
        let view = cx.new(|cx| Application::new(canonical_root, graph, window, cx));
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
