//! File categories: by extension first, then by magic bytes (ANALYZER §1).

use std::path::Path;

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum Category {
    Image,
    Video,
    Audio,
    Document,
    Pdf,
    Spreadsheet,
    Presentation,
    Code,
    Data,
    Archive,
    Installer,
    Font,
    Model3d,
    GameAsset,
    Executable,
    System,
    Other,
}

impl Category {
    pub const ALL: [Category; 17] = [
        Self::Image,
        Self::Video,
        Self::Audio,
        Self::Document,
        Self::Pdf,
        Self::Spreadsheet,
        Self::Presentation,
        Self::Code,
        Self::Data,
        Self::Archive,
        Self::Installer,
        Self::Font,
        Self::Model3d,
        Self::GameAsset,
        Self::Executable,
        Self::System,
        Self::Other,
    ];

    /// Name for reports.
    pub fn label(self) -> &'static str {
        match self {
            Self::Image => "Images",
            Self::Video => "Video",
            Self::Audio => "Audio",
            Self::Document => "Documents",
            Self::Pdf => "PDFs",
            Self::Spreadsheet => "Spreadsheets",
            Self::Presentation => "Presentations",
            Self::Code => "Code",
            Self::Data => "Data",
            Self::Archive => "Archives",
            Self::Installer => "Installers",
            Self::Font => "Fonts",
            Self::Model3d => "3D models",
            Self::GameAsset => "Game assets",
            Self::Executable => "Programs",
            Self::System => "System files",
            Self::Other => "Other",
        }
    }
}

const TABLE: &[(Category, &[&str])] = &[
    (
        Category::Image,
        &[
            "jpg", "jpeg", "png", "gif", "bmp", "tif", "tiff", "webp", "heic", "heif", "avif", "svg", "ico",
            "icns", "raw", "cr2", "cr3", "nef", "arw", "dng", "orf", "rw2", "psd", "psb", "xcf", "ai", "eps",
            "jxl", "tga", "exr", "hdr", "pbm", "pgm", "ppm",
        ],
    ),
    (
        Category::Video,
        &[
            "mp4", "m4v", "mov", "avi", "mkv", "webm", "wmv", "flv", "mpg", "mpeg", "m2ts", "mts", "ts",
            "3gp", "ogv", "vob", "mxf", "prores", "braw",
        ],
    ),
    (
        Category::Audio,
        &[
            "mp3", "m4a", "aac", "wav", "flac", "ogg", "oga", "opus", "wma", "aiff", "aif", "alac", "mid",
            "midi", "amr", "caf", "ape", "wv", "als", "flp", "logicx", "band",
        ],
    ),
    (
        Category::Document,
        &[
            "doc", "docx", "dot", "dotx", "odt", "rtf", "txt", "md", "markdown", "rst", "tex", "pages",
            "wpd", "epub", "mobi", "azw", "azw3", "djvu", "xps", "log", "nfo", "org", "adoc", "textile",
            "one",
        ],
    ),
    (Category::Pdf, &["pdf"]),
    (
        Category::Spreadsheet,
        &[
            "xls", "xlsx", "xlsm", "xlsb", "xlt", "xltx", "ods", "numbers", "csv", "tsv",
        ],
    ),
    (
        Category::Presentation,
        &["ppt", "pptx", "pptm", "pps", "ppsx", "odp", "key", "potx"],
    ),
    (
        Category::Code,
        &[
            "rs",
            "py",
            "pyw",
            "pyi",
            "js",
            "mjs",
            "cjs",
            "jsx",
            "ts",
            "tsx",
            "go",
            "java",
            "kt",
            "kts",
            "scala",
            "groovy",
            "c",
            "h",
            "cc",
            "cpp",
            "cxx",
            "hpp",
            "hh",
            "hxx",
            "m",
            "mm",
            "swift",
            "cs",
            "fs",
            "fsx",
            "vb",
            "rb",
            "php",
            "pl",
            "pm",
            "lua",
            "r",
            "jl",
            "dart",
            "ex",
            "exs",
            "erl",
            "hrl",
            "hs",
            "ml",
            "mli",
            "clj",
            "cljs",
            "lisp",
            "el",
            "scm",
            "rkt",
            "nim",
            "zig",
            "v",
            "sv",
            "vhd",
            "vhdl",
            "asm",
            "s",
            "sh",
            "bash",
            "zsh",
            "fish",
            "ps1",
            "psm1",
            "bat",
            "cmd",
            "sql",
            "html",
            "htm",
            "css",
            "scss",
            "sass",
            "less",
            "vue",
            "svelte",
            "astro",
            "gd",
            "gdshader",
            "glsl",
            "hlsl",
            "wgsl",
            "shader",
            "cu",
            "proto",
            "thrift",
            "graphql",
            "gql",
            "tf",
            "hcl",
            "cmake",
            "gradle",
            "make",
            "mk",
            "dockerfile",
            "ipynb",
            "tcl",
            "pas",
            "ada",
            "cob",
            "f90",
            "f95",
            "for",
        ],
    ),
    (
        Category::Data,
        &[
            "json",
            "jsonl",
            "ndjson",
            "xml",
            "yaml",
            "yml",
            "toml",
            "ini",
            "cfg",
            "conf",
            "plist",
            "properties",
            "env",
            "parquet",
            "arrow",
            "feather",
            "avro",
            "orc",
            "sqlite",
            "sqlite3",
            "db",
            "mdb",
            "accdb",
            "sav",
            "dta",
            "rdata",
            "rds",
            "npy",
            "npz",
            "pkl",
            "pickle",
            "h5",
            "hdf5",
            "mat",
            "tfrecord",
            "safetensors",
            "gguf",
            "onnx",
            "pt",
            "pth",
            "ckpt",
            "bin",
            "dat",
            "lock",
            "sum",
            "geojson",
            "kml",
            "gpx",
            "shp",
            "srt",
            "vtt",
            "ass",
            "bib",
            "ics",
            "vcf",
        ],
    ),
    (
        Category::Archive,
        &[
            "zip", "tar", "gz", "tgz", "bz2", "tbz2", "xz", "txz", "zst", "7z", "rar", "lz", "lzma", "lz4",
            "cab", "jar", "war", "ear", "apk", "aab", "ipa", "whl", "egg", "gem", "nupkg", "crate", "cpio",
            "ar", "z", "sit", "sitx",
        ],
    ),
    (
        Category::Installer,
        &[
            "dmg",
            "pkg",
            "mpkg",
            "exe",
            "msi",
            "msix",
            "appx",
            "appxbundle",
            "deb",
            "rpm",
            "snap",
            "flatpak",
            "appimage",
            "iso",
            "img",
            "crdownload",
            "part",
        ],
    ),
    (
        Category::Font,
        &[
            "ttf", "otf", "woff", "woff2", "eot", "fon", "fnt", "pfb", "pfm", "ttc", "dfont",
        ],
    ),
    (
        Category::Model3d,
        &[
            "obj", "fbx", "stl", "dae", "3ds", "blend", "max", "ma", "mb", "c4d", "gltf", "glb", "usd",
            "usdz", "usda", "ply", "step", "stp", "iges", "igs", "dwg", "dxf", "skp", "x3d", "wrl", "3mf",
            "abc", "zpr", "ztl",
        ],
    ),
    (
        Category::GameAsset,
        &[
            "tscn",
            "tres",
            "godot",
            "import",
            "uasset",
            "umap",
            "unity",
            "unitypackage",
            "prefab",
            "asset",
            "mat",
            "pak",
            "wad",
            "bsp",
            "vpk",
            "tmx",
            "tsx",
            "ldtk",
            "anim",
            "controller",
            "meta",
            "atlas",
            "sprite",
            "spine",
            "dds",
            "ktx",
            "ktx2",
            "basis",
            "bank",
            "fsb",
        ],
    ),
    (
        Category::Executable,
        &[
            "app",
            "dll",
            "so",
            "dylib",
            "a",
            "lib",
            "o",
            "obj_",
            "class",
            "pyc",
            "pyo",
            "elf",
            "out",
            "com",
            "scr",
            "sys",
            "ocx",
            "wasm",
            "bundle",
            "framework",
            "kext",
            "vsix",
            "xpi",
            "crx",
        ],
    ),
    (
        Category::System,
        &[
            "ds_store",
            "localized",
            "thumbs",
            "ini_",
            "lnk",
            "url",
            "webloc",
            "desktop",
            "swp",
            "swo",
            "bak",
            "tmp",
            "temp",
            "cache",
            "trash",
            "icloud",
            "tmpl",
            "dmp",
            "mdmp",
            "pid",
            "lck",
            "lock_",
        ],
    ),
];

/// Category from the extension alone (`None` when unknown).
pub fn category_by_extension(ext: &str) -> Option<Category> {
    let ext = ext.trim_start_matches('.').to_ascii_lowercase();
    TABLE
        .iter()
        .find(|(_, exts)| exts.contains(&ext.as_str()))
        .map(|(c, _)| *c)
}

/// Number of extensions the table knows.
pub fn known_extensions() -> usize {
    TABLE.iter().map(|(_, e)| e.len()).sum()
}

/// Category from magic bytes (`head` = the first ~512 bytes).
pub fn category_by_magic(head: &[u8]) -> Option<Category> {
    use infer::MatcherType as M;
    let t = infer::get(head)?;
    Some(match t.matcher_type() {
        M::Image => Category::Image,
        M::Video => Category::Video,
        M::Audio => Category::Audio,
        M::Doc => match t.extension() {
            "pdf" => Category::Pdf,
            "xls" | "xlsx" => Category::Spreadsheet,
            "ppt" | "pptx" => Category::Presentation,
            _ => Category::Document,
        },
        M::Archive => Category::Archive,
        M::Font => Category::Font,
        M::Book => Category::Document,
        M::App => Category::Executable,
        M::Text | M::Custom => return None,
    })
}

/// Final category: a known extension wins; otherwise the magic bytes (when given); otherwise `Other`.
pub fn categorize(path: &Path, head: Option<&[u8]>) -> Category {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    if matches!(name.as_str(), ".ds_store" | "thumbs.db" | "desktop.ini") {
        return Category::System;
    }
    if matches!(
        name.as_str(),
        "dockerfile" | "makefile" | "cmakelists.txt" | "rakefile" | "gemfile" | "procfile"
    ) {
        return Category::Code;
    }
    path.extension()
        .and_then(|e| category_by_extension(&e.to_string_lossy()))
        .or_else(|| head.and_then(category_by_magic))
        .unwrap_or(Category::Other)
}

/// The real type disagrees with the extension (`holiday.jpg` that is a ZIP): returns the detected extension.
pub fn magic_extension_mismatch(path: &Path, head: &[u8]) -> Option<&'static str> {
    let ext = path.extension()?.to_string_lossy().to_ascii_lowercase();
    let detected = infer::get(head)?;
    let d = detected.extension();
    let same = d == ext
        || (d == "jpg" && ext == "jpeg")
        || (d == "tif" && ext == "tiff")
        || (d == "zip"
            && matches!(
                ext.as_str(),
                "docx"
                    | "xlsx"
                    | "pptx"
                    | "jar"
                    | "apk"
                    | "epub"
                    | "odt"
                    | "ods"
                    | "odp"
                    | "whl"
                    | "ipa"
                    | "xpi"
                    | "crx"
                    | "vsix"
                    | "aab"
                    | "war"
                    | "nupkg"
                    | "3mf"
                    | "usdz"
            ))
        || (d == "gz" && matches!(ext.as_str(), "tgz"))
        || (d == "mp4" && matches!(ext.as_str(), "m4v" | "m4a" | "mov" | "3gp"))
        || (d == "ole" || d == "doc") && matches!(ext.as_str(), "doc" | "xls" | "ppt" | "msi" | "msg");
    let comparable = matches!(
        detected.matcher_type(),
        infer::MatcherType::Image
            | infer::MatcherType::Video
            | infer::MatcherType::Audio
            | infer::MatcherType::Archive
    ) || d == "pdf";
    // Only flag when the file claims a *different media family* (an image that is really an archive, ...).
    let ext_cat = category_by_extension(&ext);
    let det_cat = category_by_magic(head);
    let claims_media = matches!(
        ext_cat,
        Some(Category::Image | Category::Video | Category::Audio | Category::Pdf | Category::Archive)
    );
    (comparable && !same && claims_media && ext_cat != det_cat).then_some(d)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_table_is_large_and_unambiguous() {
        assert!(known_extensions() >= 250, "{} extensions", known_extensions());
        // An extension may appear in only one category, except the few deliberate overlaps (kept first-wins).
        let mut seen = std::collections::HashMap::new();
        let mut dups = Vec::new();
        for (c, exts) in TABLE {
            for e in *exts {
                if let Some(prev) = seen.insert(*e, *c) {
                    dups.push((*e, prev, *c));
                }
            }
        }
        assert!(dups.len() <= 6, "too many overlapping extensions: {dups:?}");
    }

    #[test]
    fn extension_lookup_is_case_insensitive() {
        assert_eq!(category_by_extension("JPG"), Some(Category::Image));
        assert_eq!(category_by_extension(".rs"), Some(Category::Code));
        assert_eq!(category_by_extension("dmg"), Some(Category::Installer));
        assert_eq!(category_by_extension("pdf"), Some(Category::Pdf));
        assert_eq!(category_by_extension("nope"), None);
    }

    #[test]
    fn well_known_names_and_unknown_extensions() {
        assert_eq!(categorize(Path::new("/x/.DS_Store"), None), Category::System);
        assert_eq!(categorize(Path::new("/x/Dockerfile"), None), Category::Code);
        assert_eq!(categorize(Path::new("/x/mystery"), None), Category::Other);
    }

    #[test]
    fn magic_bytes_classify_extensionless_files() {
        let png = [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, 0, 0, 0, 0];
        assert_eq!(categorize(Path::new("/x/photo"), Some(&png)), Category::Image);
        let zip = [b'P', b'K', 3, 4, 0, 0, 0, 0, 0, 0];
        assert_eq!(categorize(Path::new("/x/bundle"), Some(&zip)), Category::Archive);
        assert_eq!(
            categorize(Path::new("/x/notes.txt"), Some(&png)),
            Category::Document,
            "the extension wins"
        );
    }

    #[test]
    fn extension_mismatch_is_flagged_for_media_only() {
        let zip = [b'P', b'K', 3, 4, 0, 0, 0, 0, 0, 0];
        assert_eq!(
            magic_extension_mismatch(Path::new("holiday.jpg"), &zip),
            Some("zip")
        );
        assert_eq!(magic_extension_mismatch(Path::new("report.docx"), &zip), None);
        assert_eq!(magic_extension_mismatch(Path::new("data.bin"), &zip), None);
        let png = [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, 0, 0, 0, 0];
        assert_eq!(magic_extension_mismatch(Path::new("a.png"), &png), None);
        assert_eq!(magic_extension_mismatch(Path::new("a.mp3"), &png), Some("png"));
    }
}
