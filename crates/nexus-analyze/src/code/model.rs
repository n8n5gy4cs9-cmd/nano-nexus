//! Shared types of the code index (ANALYZER §5.2).

use serde::{Deserialize, Serialize};

/// What a symbol is. Files and folders are symbols too (their "references" are imports and config entries).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum SymbolKind {
    Function,
    Method,
    Variable,
    Constant,
    Parameter,
    Field,
    Class,
    Interface,
    Enum,
    EnumMember,
    Module,
    File,
    Folder,
}

impl SymbolKind {
    /// Parse the part after `def.` in a tags capture name.
    pub fn from_capture(s: &str) -> Option<SymbolKind> {
        Some(match s {
            "function" => SymbolKind::Function,
            "method" => SymbolKind::Method,
            "variable" => SymbolKind::Variable,
            "constant" => SymbolKind::Constant,
            "parameter" => SymbolKind::Parameter,
            "field" => SymbolKind::Field,
            "class" => SymbolKind::Class,
            "interface" => SymbolKind::Interface,
            "enum" => SymbolKind::Enum,
            "enum_member" => SymbolKind::EnumMember,
            "module" => SymbolKind::Module,
            _ => return None,
        })
    }

    /// Lowercase label for prompts and the UI.
    pub fn label(self) -> &'static str {
        match self {
            SymbolKind::Function => "function",
            SymbolKind::Method => "method",
            SymbolKind::Variable => "variable",
            SymbolKind::Constant => "constant",
            SymbolKind::Parameter => "parameter",
            SymbolKind::Field => "field",
            SymbolKind::Class => "class",
            SymbolKind::Interface => "interface",
            SymbolKind::Enum => "enum",
            SymbolKind::EnumMember => "enum_member",
            SymbolKind::Module => "module",
            SymbolKind::File => "file",
            SymbolKind::Folder => "folder",
        }
    }
}
