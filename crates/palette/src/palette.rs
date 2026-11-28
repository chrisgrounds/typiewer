use gpui::{
  App, BoxShadow, Context, EventEmitter, FocusHandle, Focusable, FontWeight, KeyDownEvent,
  MouseButton, ScrollHandle, SharedString, Window, div, point, prelude::*, px, rgb, rgba,
};

use crate::ClosePalette;
use crate::fuzzy::fuzzy_match;

const MONO_FONT: &str = "Menlo";
const TITLE_MATCH_BONUS: i32 = 200;
const MAX_VISIBLE_RESULTS: usize = 64;

#[derive(Clone, Debug)]
pub enum PaletteEvent<T> {
  Select(T),
  Close,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BadgeTone {
  Sky,
  Emerald,
  Amber,
  Purple,
  Slate,
}

impl BadgeTone {
  pub const fn colors(self) -> (u32, u32, u32) {
    match self {
      Self::Sky => (0x38bdf822, 0x38bdf844, 0x7dd3fc),
      Self::Emerald => (0x34d39922, 0x34d39944, 0x6ee7b7),
      Self::Amber => (0xfbbf2422, 0xfbbf2444, 0xfde68a),
      Self::Purple => (0xc084fc22, 0xc084fc44, 0xd8b4fe),
      Self::Slate => (0x94a3b822, 0x94a3b844, 0xcbd5e1),
    }
  }
}

#[derive(Clone, Debug)]
pub struct PaletteBadge {
  pub label: SharedString,
  pub tone: BadgeTone,
}

impl PaletteBadge {
  pub fn new(label: impl Into<SharedString>, tone: BadgeTone) -> Self {
    Self {
      label: label.into(),
      tone,
    }
  }
}

#[derive(Clone, Debug)]
pub struct PaletteItem<T> {
  pub id: T,
  pub title: String,
  pub badge: Option<PaletteBadge>,
  pub detail: String,
  pub search_fallback: String,
}

#[derive(Clone, Copy, Debug)]
pub struct PaletteConfig {
  pub key_context: &'static str,
  pub prompt_badge: &'static str,
  pub placeholder: &'static str,
  pub empty_message: &'static str,
  pub item_singular: &'static str,
  pub item_plural: &'static str,
  pub footer_left: &'static str,
  pub footer_right: &'static str,
  pub accent_rgb: u32,
  pub accent_border_rgba: u32,
  pub selected_bg_rgba: u32,
  pub selected_border_rgba: u32,
  pub highlight_rgb: u32,
}

#[derive(Clone, Debug)]
struct MatchedItem {
  item_index: usize,
  score: i32,
  title_indices: Vec<usize>,
}

pub struct FuzzyPalette<T: Clone + 'static> {
  config: PaletteConfig,
  items: Vec<PaletteItem<T>>,
  query: String,
  matches: Vec<MatchedItem>,
  selected_index: usize,
  focus_handle: FocusHandle,
  scroll_handle: ScrollHandle,
}

impl<T: Clone + 'static> FuzzyPalette<T> {
  pub fn new(config: PaletteConfig, items: Vec<PaletteItem<T>>, cx: &mut Context<Self>) -> Self {
    let mut palette = Self {
      config,
      items,
      query: String::new(),
      matches: Vec::new(),
      selected_index: 0,
      focus_handle: cx.focus_handle(),
      scroll_handle: ScrollHandle::new(),
    };
    palette.recompute_matches();
    palette
  }

  pub fn set_items(&mut self, items: Vec<PaletteItem<T>>, cx: &mut Context<Self>) {
    self.items = items;
    self.refresh_results(cx);
  }

  pub fn reset(&mut self, window: &mut Window, cx: &mut Context<Self>) {
    self.query.clear();
    self.selected_index = 0;
    self.refresh_results(cx);
    window.focus(&self.focus_handle, cx);
  }

  fn refresh_results(&mut self, cx: &mut Context<Self>) {
    self.recompute_matches();
    self.scroll_handle.scroll_to_item(0);
    cx.notify();
  }

  fn recompute_matches(&mut self) {
    self.matches.clear();

    let trimmed = self.query.trim();
    if trimmed.is_empty() {
      self
        .matches
        .extend((0..self.items.len()).map(|item_index| MatchedItem {
          item_index,
          score: 0,
          title_indices: Vec::new(),
        }));
      self.selected_index = 0;
      return;
    }

    for (item_index, item) in self.items.iter().enumerate() {
      if let Some(m) = fuzzy_match(trimmed, &item.title) {
        self.matches.push(MatchedItem {
          item_index,
          score: m.score + TITLE_MATCH_BONUS,
          title_indices: m.indices,
        });
      } else if !item.search_fallback.is_empty()
        && let Some(m) = fuzzy_match(trimmed, &item.search_fallback)
      {
        self.matches.push(MatchedItem {
          item_index,
          score: m.score,
          title_indices: Vec::new(),
        });
      }
    }

    self.matches.sort_by(|a, b| {
      b.score.cmp(&a.score).then_with(|| {
        let title_a = self
          .items
          .get(a.item_index)
          .map(|i| i.title.as_str())
          .unwrap_or("");
        let title_b = self
          .items
          .get(b.item_index)
          .map(|i| i.title.as_str())
          .unwrap_or("");
        title_a.cmp(title_b)
      })
    });

    self.selected_index = 0;
  }

  fn visible_count(&self) -> usize {
    self.matches.len().min(MAX_VISIBLE_RESULTS)
  }

  fn move_selection(&mut self, delta: isize, cx: &mut Context<Self>) {
    let count = self.visible_count();
    if count == 0 {
      return;
    }
    let next = (self.selected_index as isize + delta).rem_euclid(count as isize) as usize;
    self.selected_index = next;
    self.scroll_handle.scroll_to_item(next);
    cx.notify();
  }

  fn confirm_selection(&mut self, cx: &mut Context<Self>) {
    let Some(matched) = self.matches.get(self.selected_index) else {
      return;
    };
    let Some(item) = self.items.get(matched.item_index) else {
      return;
    };
    cx.emit(PaletteEvent::Select(item.id.clone()));
  }

  fn select_item(&mut self, id: T, cx: &mut Context<Self>) {
    cx.emit(PaletteEvent::Select(id));
  }

  fn close(&mut self, _: &ClosePalette, _window: &mut Window, cx: &mut Context<Self>) {
    cx.emit(PaletteEvent::Close);
  }

  fn delete_last_word(&mut self) {
    let trimmed_end = self.query.trim_end().len();
    self.query.truncate(trimmed_end);
    while let Some(ch) = self.query.pop() {
      if ch.is_whitespace() || matches!(ch, '_' | '-' | '/' | ':' | '.') {
        break;
      }
    }
  }

  fn on_key_down(&mut self, event: &KeyDownEvent, _window: &mut Window, cx: &mut Context<Self>) {
    let key = event.keystroke.key.as_str();
    let mods = &event.keystroke.modifiers;

    // Allow cmd-p / cmd-shift-p to bubble up to global palette toggle bindings.
    if mods.platform && key == "p" {
      return;
    }

    match key {
      "down" => {
        cx.stop_propagation();
        self.move_selection(1, cx);
      }
      "up" => {
        cx.stop_propagation();
        self.move_selection(-1, cx);
      }
      "tab" => {
        cx.stop_propagation();
        self.move_selection(if mods.shift { -1 } else { 1 }, cx);
      }
      "n" if mods.control => {
        cx.stop_propagation();
        self.move_selection(1, cx);
      }
      "p" if mods.control => {
        cx.stop_propagation();
        self.move_selection(-1, cx);
      }
      "enter" => {
        cx.stop_propagation();
        self.confirm_selection(cx);
      }
      "backspace" => {
        cx.stop_propagation();
        if mods.platform {
          self.query.clear();
        } else if mods.alt {
          self.delete_last_word();
        } else {
          self.query.pop();
        }
        self.refresh_results(cx);
      }
      "u" if mods.control => {
        cx.stop_propagation();
        self.query.clear();
        self.refresh_results(cx);
      }
      "w" if mods.control => {
        cx.stop_propagation();
        self.delete_last_word();
        self.refresh_results(cx);
      }
      _ => {
        if !mods.platform
          && !mods.control
          && let Some(key_char) = event.keystroke.key_char.as_deref()
          && !key_char.is_empty()
          && !key_char.chars().any(char::is_control)
        {
          cx.stop_propagation();
          self.query.push_str(key_char);
          self.refresh_results(cx);
        }
      }
    }
  }
}

impl<T: Clone + 'static> EventEmitter<PaletteEvent<T>> for FuzzyPalette<T> {}

impl<T: Clone + 'static> Focusable for FuzzyPalette<T> {
  fn focus_handle(&self, _cx: &App) -> FocusHandle {
    self.focus_handle.clone()
  }
}

fn render_highlighted_title(
  title: &str,
  match_indices: &[usize],
  highlight_rgb: u32,
) -> impl IntoElement {
  if match_indices.is_empty() {
    return div()
      .flex()
      .items_center()
      .font_family(MONO_FONT)
      .text_size(px(13.0))
      .font_weight(FontWeight::MEDIUM)
      .text_color(rgb(0xf3f4f6))
      .child(title.to_owned());
  }

  let mut segments: Vec<(String, bool)> = Vec::new();
  let mut match_pos = 0;

  for (idx, ch) in title.chars().enumerate() {
    let is_match = match_indices.get(match_pos) == Some(&idx);
    if is_match {
      match_pos += 1;
    }

    if let Some((last_text, last_is_match)) = segments.last_mut()
      && *last_is_match == is_match
    {
      last_text.push(ch);
      continue;
    }

    segments.push((ch.to_string(), is_match));
  }

  div()
    .flex()
    .items_center()
    .font_family(MONO_FONT)
    .text_size(px(13.0))
    .children(segments.into_iter().map(move |(text, highlighted)| {
      div()
        .when(highlighted, |el| {
          el.font_weight(FontWeight::BOLD)
            .text_color(rgb(highlight_rgb))
        })
        .when(!highlighted, |el| {
          el.font_weight(FontWeight::MEDIUM).text_color(rgb(0xe2e8f0))
        })
        .child(text)
    }))
}

impl<T: Clone + 'static> Render for FuzzyPalette<T> {
  fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
    let config = self.config;
    let total_matches = self.matches.len();
    let visible_count = self.visible_count();
    let count_label = if total_matches == 1 {
      format!("1 {}", config.item_singular)
    } else if total_matches > MAX_VISIBLE_RESULTS {
      format!("{visible_count} of {total_matches} {}", config.item_plural)
    } else {
      format!("{total_matches} {}", config.item_plural)
    };

    let rows: Vec<_> = self
      .matches
      .iter()
      .take(MAX_VISIBLE_RESULTS)
      .enumerate()
      .filter_map(|(row_idx, matched)| {
        let item = self.items.get(matched.item_index)?;
        let is_selected = row_idx == self.selected_index;
        let item_id = item.id.clone();

        Some(
          div()
            .id(("palette-row", row_idx))
            .cursor_pointer()
            .px(px(14.0))
            .py(px(8.0))
            .mx(px(6.0))
            .rounded(px(7.0))
            .flex()
            .items_center()
            .justify_between()
            .gap(px(12.0))
            .border_1()
            .when(is_selected, |el| {
              el.bg(rgba(config.selected_bg_rgba))
                .border_color(rgba(config.selected_border_rgba))
            })
            .when(!is_selected, |el| {
              el.border_color(rgba(0x00000000))
                .hover(|h| h.bg(rgba(0xffffff08)))
            })
            .on_mouse_down(
              MouseButton::Left,
              cx.listener(move |this, _, _window, cx| {
                cx.stop_propagation();
                this.select_item(item_id.clone(), cx);
              }),
            )
            .child(
              div()
                .flex()
                .items_center()
                .gap(px(8.0))
                .min_w_0()
                .when_some(item.badge.as_ref(), |el, badge| {
                  let (bg_rgba, border_rgba, fg_rgb) = badge.tone.colors();
                  el.child(
                    div()
                      .px(px(6.0))
                      .py(px(1.5))
                      .rounded(px(4.0))
                      .bg(rgba(bg_rgba))
                      .border_1()
                      .border_color(rgba(border_rgba))
                      .font_family(MONO_FONT)
                      .text_size(px(10.0))
                      .text_color(rgb(fg_rgb))
                      .child(badge.label.clone()),
                  )
                })
                .child(render_highlighted_title(
                  &item.title,
                  &matched.title_indices,
                  config.highlight_rgb,
                )),
            )
            .when(!item.detail.is_empty(), |el| {
              el.child(
                div()
                  .font_family(MONO_FONT)
                  .text_size(px(11.0))
                  .text_color(if is_selected {
                    rgb(0x94a3b8)
                  } else {
                    rgb(0x64748b)
                  })
                  .child(item.detail.clone()),
              )
            }),
        )
      })
      .collect();

    div()
      .id("fuzzy-palette-backdrop")
      .key_context(config.key_context)
      .track_focus(&self.focus_handle)
      .on_action(cx.listener(Self::close))
      .on_key_down(cx.listener(Self::on_key_down))
      .on_mouse_down(
        MouseButton::Left,
        cx.listener(|_this, _, _window, cx| {
          cx.stop_propagation();
          cx.emit(PaletteEvent::Close);
        }),
      )
      .on_mouse_down(
        MouseButton::Middle,
        cx.listener(|_this, _, _window, cx| {
          cx.stop_propagation();
        }),
      )
      .on_scroll(cx.listener(|_this, _, _window, cx| {
        cx.stop_propagation();
      }))
      .absolute()
      .inset_0()
      .bg(rgba(0x04050999))
      .flex()
      .justify_center()
      .items_start()
      .pt(px(88.0))
      .cursor_default()
      .child(
        div()
          .id("fuzzy-palette-modal")
          .w(px(580.0))
          .max_w(px(640.0))
          .flex()
          .flex_col()
          .overflow_hidden()
          .rounded(px(12.0))
          .bg(rgba(0x11131af7))
          .border_1()
          .border_color(rgba(config.accent_border_rgba))
          .shadow(vec![BoxShadow {
            inset: false,
            color: rgba(0x000000bb).into(),
            offset: point(px(0.0), px(18.0)),
            blur_radius: px(40.0),
            spread_radius: px(-6.0),
          }])
          .on_mouse_down(
            MouseButton::Left,
            cx.listener(|_this, _, _window, cx| {
              cx.stop_propagation();
            }),
          )
          .child(
            div()
              .px(px(16.0))
              .py(px(12.0))
              .flex()
              .items_center()
              .justify_between()
              .gap(px(10.0))
              .bg(rgba(0xffffff05))
              .border_b_1()
              .border_color(rgba(0xffffff12))
              .child(
                div()
                  .flex()
                  .items_center()
                  .gap(px(8.0))
                  .flex_1()
                  .min_w_0()
                  .child(
                    div()
                      .font_family(MONO_FONT)
                      .text_size(px(14.0))
                      .text_color(rgb(config.accent_rgb))
                      .child(config.prompt_badge),
                  )
                  .child(
                    div()
                      .flex()
                      .items_center()
                      .flex_1()
                      .min_w_0()
                      .font_family(MONO_FONT)
                      .text_size(px(14.0))
                      .when(self.query.is_empty(), |el| {
                        el.child(
                          div()
                            .w(px(2.0))
                            .h(px(16.0))
                            .mr(px(2.0))
                            .rounded(px(1.0))
                            .bg(rgb(config.accent_rgb)),
                        )
                        .child(div().text_color(rgb(0x64748b)).child(config.placeholder))
                      })
                      .when(!self.query.is_empty(), |el| {
                        el.child(div().text_color(rgb(0xf8fafc)).child(self.query.clone()))
                          .child(
                            div()
                              .w(px(2.0))
                              .h(px(16.0))
                              .ml(px(1.0))
                              .rounded(px(1.0))
                              .bg(rgb(config.accent_rgb)),
                          )
                      }),
                  ),
              )
              .child(
                div()
                  .px(px(7.0))
                  .py(px(2.0))
                  .rounded(px(5.0))
                  .bg(rgba(0xffffff0a))
                  .text_size(px(11.0))
                  .text_color(rgb(0x94a3b8))
                  .child(count_label),
              ),
          )
          .child(
            div()
              .id("fuzzy-palette-results")
              .track_scroll(&self.scroll_handle)
              .max_h(px(360.0))
              .overflow_y_scroll()
              .py(px(6.0))
              .flex()
              .flex_col()
              .gap(px(2.0))
              .when(rows.is_empty(), |el| {
                el.child(
                  div()
                    .px(px(16.0))
                    .py(px(28.0))
                    .flex()
                    .justify_center()
                    .text_size(px(13.0))
                    .text_color(rgb(0x64748b))
                    .child(config.empty_message),
                )
              })
              .children(rows),
          )
          .child(
            div()
              .px(px(16.0))
              .py(px(8.0))
              .flex()
              .items_center()
              .justify_between()
              .bg(rgba(0xffffff04))
              .border_t_1()
              .border_color(rgba(0xffffff0e))
              .text_size(px(11.0))
              .text_color(rgb(0x64748b))
              .child(config.footer_left)
              .child(config.footer_right),
          ),
      )
  }
}
