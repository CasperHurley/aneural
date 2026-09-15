//! Default icon choices for node kinds, file extensions and package ecosystems.

use std::path::Path;

/// Default icon name for a node kind. For `File` nodes the path's extension
/// (or well-known file name) refines the choice.
pub fn default_icon(kind: &str, path: Option<&Path>) -> &'static str {
    match kind {
        "Directory" => "LuFolder",
        "Repo" => "VsRepo",
        "Manifest" => "VsPackage",
        "Package" => "LuPackage",
        "Symbol" => "VsSymbolMethod",
        "Comment" => "LuMessageSquare",
        "Plan" => "LuMap",
        "Idea" => "LuLightbulb",
        "Note" => "LuStickyNote",
        "File" => path.map(file_icon).unwrap_or("LuFile"),
        _ => crate::FALLBACK_ICON,
    }
}

/// Icon for a file by extension or well-known name.
///
/// Prefers Bootstrap's `BsFiletype*` set, which draws the extension on the
/// page itself, and falls back to a brand mark for languages Bootstrap has no
/// filetype glyph for (Rust, Go, TOML, ...).
pub fn file_icon(path: &Path) -> &'static str {
    let name = path.file_name().and_then(|s| s.to_str()).unwrap_or("");
    if name.eq_ignore_ascii_case("dockerfile")
        || name.to_ascii_lowercase().starts_with("dockerfile.")
    {
        return "SiDocker";
    }
    let ext = path
        .extension()
        .and_then(|s| s.to_str())
        .map(|s| s.to_ascii_lowercase());
    match ext.as_deref() {
        // code
        Some("tsx") => "BsFiletypeTsx",
        Some("ts" | "mts" | "cts") => "SiTypescript",
        Some("jsx") => "BsFiletypeJsx",
        Some("js" | "mjs" | "cjs") => "BsFiletypeJs",
        Some("py" | "pyi") => "BsFiletypePy",
        Some("rs") => "SiRust",
        Some("go") => "SiGo",
        Some("java") => "BsFiletypeJava",
        Some("cs") => "BsFiletypeCs",
        Some("php") => "BsFiletypePhp",
        Some("rb" | "rake" | "gemspec") => "BsFiletypeRb",
        Some("sh" | "zsh" | "bash") => "BsFiletypeSh",
        Some("sql") => "BsFiletypeSql",
        Some("db" | "sqlite" | "sqlite3") => "SiSqlite",
        // markup and data
        Some("mdx") => "BsFiletypeMdx",
        Some("md" | "markdown") => "BsFiletypeMd",
        Some("json" | "jsonc") => "BsFiletypeJson",
        Some("yaml" | "yml") => "BsFiletypeYml",
        Some("toml") => "SiToml",
        Some("xml") => "BsFiletypeXml",
        Some("csv" | "tsv") => "BsFiletypeCsv",
        Some("html" | "htm") => "BsFiletypeHtml",
        Some("scss") => "BsFiletypeScss",
        Some("sass") => "BsFiletypeSass",
        Some("css" | "less") => "BsFiletypeCss",
        Some("txt" | "text") => "BsFiletypeTxt",
        Some("pdf") => "BsFiletypePdf",
        // images
        Some("png") => "BsFiletypePng",
        Some("jpg" | "jpeg") => "BsFiletypeJpg",
        Some("gif") => "BsFiletypeGif",
        Some("svg") => "BsFiletypeSvg",
        Some("bmp") => "BsFiletypeBmp",
        Some("heic" | "heif") => "BsFiletypeHeic",
        Some("tiff" | "tif") => "BsFiletypeTiff",
        Some("raw" | "cr2" | "nef" | "arw") => "BsFiletypeRaw",
        Some("ai") => "BsFiletypeAi",
        Some("psd") => "BsFiletypePsd",
        Some("webp" | "ico" | "avif") => "LuImage",
        // fonts
        Some("ttf") => "BsFiletypeTtf",
        Some("otf") => "BsFiletypeOtf",
        Some("woff" | "woff2") => "BsFiletypeWoff",
        // media
        Some("mp3") => "BsFiletypeMp3",
        Some("wav") => "BsFiletypeWav",
        Some("aac") => "BsFiletypeAac",
        Some("m4p" | "m4a") => "BsFiletypeM4p",
        Some("mp4" | "m4v") => "BsFiletypeMp4",
        Some("mov") => "BsFiletypeMov",
        // documents and binaries
        Some("doc") => "BsFiletypeDoc",
        Some("docx") => "BsFiletypeDocx",
        Some("xls") => "BsFiletypeXls",
        Some("xlsx") => "BsFiletypeXlsx",
        Some("ppt") => "BsFiletypePpt",
        Some("pptx") => "BsFiletypePptx",
        Some("exe" | "dll" | "msi") => "BsFiletypeExe",
        Some("key" | "pem" | "crt") => "BsFiletypeKey",
        Some("lock") => "VsLock",
        _ => "LuFile",
    }
}

/// Badge icon for a package ecosystem.
pub fn ecosystem_icon(ecosystem: &str) -> &'static str {
    match ecosystem {
        "npm" => "SiNpm",
        "cargo" => "SiRust",
        "pypi" => "SiPypi",
        "go" => "SiGo",
        "maven" => "SiApachemaven",
        "packagist" => "SiComposer",
        "rubygems" => "SiRubygems",
        _ => "LuPackage",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::is_valid;

    #[test]
    fn kinds_and_extensions() {
        assert_eq!(
            default_icon("File", Some(Path::new("a.tsx"))),
            "BsFiletypeTsx"
        );
        assert_eq!(
            default_icon("File", Some(Path::new("src/main.RS"))),
            "SiRust"
        );
        assert_eq!(
            default_icon("File", Some(Path::new("Dockerfile"))),
            "SiDocker"
        );
        assert_eq!(
            default_icon("File", Some(Path::new("pnpm-lock.yaml"))),
            "BsFiletypeYml"
        );
        assert_eq!(
            default_icon("File", Some(Path::new("docs/README.MD"))),
            "BsFiletypeMd"
        );
        assert_eq!(
            default_icon("File", Some(Path::new("Cargo.lock"))),
            "VsLock"
        );
        assert_eq!(default_icon("File", Some(Path::new("weird.xyz"))), "LuFile");
        assert_eq!(default_icon("File", None), "LuFile");
        assert_eq!(default_icon("Directory", None), "LuFolder");
        assert_eq!(default_icon("Plan", None), "LuMap");
        assert_eq!(default_icon("Whatever", None), crate::FALLBACK_ICON);
    }

    #[test]
    fn every_default_is_registered() {
        for kind in [
            "Directory",
            "File",
            "Repo",
            "Manifest",
            "Package",
            "Symbol",
            "Comment",
            "Plan",
            "Idea",
            "Note",
            "Nope",
        ] {
            assert!(is_valid(default_icon(kind, None)), "{kind}");
        }
        for ext in [
            "ts", "tsx", "js", "jsx", "py", "rs", "go", "java", "cs", "php", "rb", "sh", "sql",
            "db", "md", "mdx", "json", "yaml", "toml", "xml", "csv", "html", "css", "scss", "sass",
            "txt", "pdf", "png", "jpg", "gif", "svg", "bmp", "heic", "tiff", "raw", "ai", "psd",
            "webp", "ttf", "otf", "woff", "mp3", "wav", "aac", "m4p", "mp4", "mov", "doc", "docx",
            "xls", "xlsx", "ppt", "pptx", "exe", "key", "lock", "bin",
        ] {
            let p = format!("x.{ext}");
            assert!(is_valid(file_icon(Path::new(&p))), "{ext}");
        }
        for eco in [
            "npm",
            "cargo",
            "pypi",
            "go",
            "maven",
            "packagist",
            "rubygems",
            "other",
        ] {
            assert!(is_valid(ecosystem_icon(eco)), "{eco}");
        }
    }
}
