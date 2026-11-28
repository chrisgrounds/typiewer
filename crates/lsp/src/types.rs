use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use url::Url;

/// Zero-based line and UTF-16 character offset in a text document.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Position {
  pub line: u32,
  pub character: u32,
}

impl Position {
  pub const fn new(line: u32, character: u32) -> Self {
    Self { line, character }
  }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Range {
  pub start: Position,
  pub end: Position,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Location {
  pub uri: String,
  pub range: Range,
}

impl Location {
  pub fn file_path(&self) -> Option<PathBuf> {
    Url::parse(&self.uri).ok()?.to_file_path().ok()
  }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LocationLink {
  #[serde(default)]
  pub origin_selection_range: Option<Range>,
  pub target_uri: String,
  pub target_range: Range,
  pub target_selection_range: Range,
}

impl From<LocationLink> for Location {
  fn from(link: LocationLink) -> Self {
    Self {
      uri: link.target_uri,
      range: link.target_selection_range,
    }
  }
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub(crate) enum GotoDefinitionResponse {
  Scalar(Location),
  Array(Vec<Location>),
  Links(Vec<LocationLink>),
}

impl GotoDefinitionResponse {
  pub(crate) fn into_locations(self) -> Vec<Location> {
    match self {
      Self::Scalar(loc) => vec![loc],
      Self::Array(locs) => locs,
      Self::Links(links) => links.into_iter().map(Location::from).collect(),
    }
  }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ServerStatusNotification {
  pub quiescent: bool,
}
