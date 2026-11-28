use std::collections::HashSet;

use graph::{EdgeId, GraphEdge, TypeId};

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
