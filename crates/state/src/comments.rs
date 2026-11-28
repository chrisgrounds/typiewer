use std::collections::HashMap;

use graph::TypeId;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CommentDraft {
  pub type_id: TypeId,
  pub text: String,
}

#[derive(Default)]
pub struct CommentsState {
  by_type: HashMap<TypeId, Vec<String>>,
  active_draft: Option<CommentDraft>,
}

impl CommentsState {
  pub fn comments_for(&self, type_id: TypeId) -> &[String] {
    self.by_type.get(&type_id).map_or(&[], Vec::as_slice)
  }

  pub fn active_draft(&self) -> Option<&CommentDraft> {
    self.active_draft.as_ref()
  }

  pub fn active_draft_for(&self, type_id: TypeId) -> Option<&str> {
    self
      .active_draft
      .as_ref()
      .filter(|draft| draft.type_id == type_id)
      .map(|draft| draft.text.as_str())
  }

  pub fn is_drafting(&self) -> bool {
    self.active_draft.is_some()
  }

  pub fn start_draft(&mut self, type_id: TypeId) -> Option<TypeId> {
    if let Some(existing) = &self.active_draft {
      if existing.type_id == type_id {
        return None;
      }
      let previous = existing.type_id;
      self.active_draft = Some(CommentDraft {
        type_id,
        text: String::new(),
      });
      return Some(previous);
    }

    self.active_draft = Some(CommentDraft {
      type_id,
      text: String::new(),
    });
    None
  }

  pub fn cancel_draft(&mut self) -> Option<TypeId> {
    self.active_draft.take().map(|draft| draft.type_id)
  }

  pub fn push_str(&mut self, text: &str) -> bool {
    let Some(draft) = &mut self.active_draft else {
      return false;
    };
    draft.text.push_str(text);
    true
  }

  pub fn pop_char(&mut self) -> bool {
    let Some(draft) = &mut self.active_draft else {
      return false;
    };
    draft.text.pop().is_some()
  }

  pub fn delete_last_word(&mut self) -> bool {
    let Some(draft) = &mut self.active_draft else {
      return false;
    };
    if draft.text.is_empty() {
      return false;
    }
    let trimmed_end = draft.text.trim_end().len();
    draft.text.truncate(trimmed_end);
    while let Some(ch) = draft.text.pop() {
      if ch.is_whitespace() {
        break;
      }
    }
    true
  }

  pub fn clear_draft_text(&mut self) -> bool {
    let Some(draft) = &mut self.active_draft else {
      return false;
    };
    if draft.text.is_empty() {
      return false;
    }
    draft.text.clear();
    true
  }

  pub fn submit_draft(&mut self) -> Option<TypeId> {
    let draft = self.active_draft.take()?;
    let trimmed = draft.text.trim();
    if !trimmed.is_empty() {
      self
        .by_type
        .entry(draft.type_id)
        .or_default()
        .push(trimmed.to_owned());
    }
    Some(draft.type_id)
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn comments_state_adds_multiple_comments_across_types() {
    let mut state = CommentsState::default();
    let t0 = TypeId(0);
    let t1 = TypeId(1);

    assert_eq!(state.start_draft(t0), None);
    assert!(state.push_str("First note"));
    assert_eq!(state.submit_draft(), Some(t0));

    assert_eq!(state.start_draft(t0), None);
    assert!(state.push_str("  Second note  "));
    assert_eq!(state.submit_draft(), Some(t0));

    assert_eq!(state.start_draft(t1), None);
    assert!(state.push_str("Other type note"));
    assert_eq!(state.submit_draft(), Some(t1));

    assert_eq!(state.comments_for(t0), &["First note", "Second note"]);
    assert_eq!(state.comments_for(t1), &["Other type note"]);
    assert!(!state.is_drafting());
  }

  #[test]
  fn empty_or_cancelled_drafts_do_not_save() {
    let mut state = CommentsState::default();
    let t0 = TypeId(0);

    state.start_draft(t0);
    state.push_str("   ");
    assert_eq!(state.submit_draft(), Some(t0));
    assert!(state.comments_for(t0).is_empty());

    state.start_draft(t0);
    state.push_str("temporary");
    assert_eq!(state.cancel_draft(), Some(t0));
    assert!(state.comments_for(t0).is_empty());
  }
}
