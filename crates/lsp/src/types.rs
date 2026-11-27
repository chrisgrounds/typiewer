use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use serde_json::Value;
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MarkupContent {
  pub kind: String,
  pub value: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum MarkedString {
  String(String),
  LanguageString { language: String, value: String },
}

impl MarkedString {
  pub fn as_str(&self) -> &str {
    match self {
      Self::String(s) => s,
      Self::LanguageString { value, .. } => value,
    }
  }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum HoverContents {
  Markup(MarkupContent),
  Array(Vec<MarkedString>),
  Scalar(MarkedString),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Hover {
  pub contents: HoverContents,
  #[serde(default)]
  pub range: Option<Range>,
}

impl Hover {
  /// Returns the hover text flattened into a single string.
  pub fn text(&self) -> String {
    match &self.contents {
      HoverContents::Markup(markup) => markup.value.clone(),
      HoverContents::Scalar(ms) => ms.as_str().to_owned(),
      HoverContents::Array(items) => items
        .iter()
        .map(MarkedString::as_str)
        .collect::<Vec<_>>()
        .join("\n\n"),
    }
  }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SymbolKind {
  File,
  Module,
  Namespace,
  Package,
  Class,
  Method,
  Property,
  Field,
  Constructor,
  Enum,
  Interface,
  Function,
  Variable,
  Constant,
  String,
  Number,
  Boolean,
  Array,
  Object,
  Key,
  Null,
  EnumMember,
  Struct,
  Event,
  Operator,
  TypeParameter,
  Unknown(u32),
}

impl SymbolKind {
  pub const fn from_u32(value: u32) -> Self {
    match value {
      1 => Self::File,
      2 => Self::Module,
      3 => Self::Namespace,
      4 => Self::Package,
      5 => Self::Class,
      6 => Self::Method,
      7 => Self::Property,
      8 => Self::Field,
      9 => Self::Constructor,
      10 => Self::Enum,
      11 => Self::Interface,
      12 => Self::Function,
      13 => Self::Variable,
      14 => Self::Constant,
      15 => Self::String,
      16 => Self::Number,
      17 => Self::Boolean,
      18 => Self::Array,
      19 => Self::Object,
      20 => Self::Key,
      21 => Self::Null,
      22 => Self::EnumMember,
      23 => Self::Struct,
      24 => Self::Event,
      25 => Self::Operator,
      26 => Self::TypeParameter,
      other => Self::Unknown(other),
    }
  }

  pub const fn as_u32(self) -> u32 {
    match self {
      Self::File => 1,
      Self::Module => 2,
      Self::Namespace => 3,
      Self::Package => 4,
      Self::Class => 5,
      Self::Method => 6,
      Self::Property => 7,
      Self::Field => 8,
      Self::Constructor => 9,
      Self::Enum => 10,
      Self::Interface => 11,
      Self::Function => 12,
      Self::Variable => 13,
      Self::Constant => 14,
      Self::String => 15,
      Self::Number => 16,
      Self::Boolean => 17,
      Self::Array => 18,
      Self::Object => 19,
      Self::Key => 20,
      Self::Null => 21,
      Self::EnumMember => 22,
      Self::Struct => 23,
      Self::Event => 24,
      Self::Operator => 25,
      Self::TypeParameter => 26,
      Self::Unknown(v) => v,
    }
  }
}

impl Serialize for SymbolKind {
  fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
    serializer.serialize_u32(self.as_u32())
  }
}

impl<'de> Deserialize<'de> for SymbolKind {
  fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
    let raw = u32::deserialize(deserializer)?;
    Ok(Self::from_u32(raw))
  }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DocumentSymbol {
  pub name: String,
  #[serde(default)]
  pub detail: Option<String>,
  pub kind: SymbolKind,
  pub range: Range,
  pub selection_range: Range,
  #[serde(default)]
  pub children: Vec<DocumentSymbol>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SymbolInformation {
  pub name: String,
  pub kind: SymbolKind,
  pub location: Location,
  #[serde(default)]
  pub container_name: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub(crate) enum DocumentSymbolResponse {
  Nested(Vec<DocumentSymbol>),
  Flat(Vec<SymbolInformation>),
}

impl DocumentSymbolResponse {
  pub(crate) fn into_document_symbols(self) -> Vec<DocumentSymbol> {
    match self {
      Self::Nested(symbols) => symbols,
      Self::Flat(infos) => infos
        .into_iter()
        .map(|info| DocumentSymbol {
          name: info.name,
          detail: info.container_name,
          kind: info.kind,
          range: info.location.range,
          selection_range: info.location.range,
          children: Vec::new(),
        })
        .collect(),
    }
  }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CallHierarchyItem {
  pub name: String,
  pub kind: SymbolKind,
  #[serde(default)]
  pub detail: Option<String>,
  pub uri: String,
  pub range: Range,
  pub selection_range: Range,
  #[serde(default)]
  pub data: Option<Value>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CallHierarchyIncomingCall {
  pub from: CallHierarchyItem,
  pub from_ranges: Vec<Range>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CallHierarchyOutgoingCall {
  pub to: CallHierarchyItem,
  pub from_ranges: Vec<Range>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServerInfo {
  pub name: String,
  #[serde(default)]
  pub version: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InitializeResult {
  #[serde(default)]
  pub capabilities: Value,
  #[serde(default)]
  pub server_info: Option<ServerInfo>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ServerHealth {
  Ok,
  Warning,
  Error,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServerStatus {
  pub health: ServerHealth,
  pub quiescent: bool,
  #[serde(default)]
  pub message: Option<String>,
}
