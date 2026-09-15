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
        Some("ts" | "tsx" | "mts" | "cts") => "SiTypescript",
        Some("js" | "jsx" | "mjs" | "cjs") => "SiJavascript",
        Some("py" | "pyi") => "SiPython",
        Some("rs") => "SiRust",
        Some("go") => "SiGo",
        Some("java") => "SiOpenjdk",
        Some("php") => "SiPhp",
        Some("rb" | "rake" | "gemspec") => "SiRuby",
        Some("md" | "mdx" | "markdown") => "SiMarkdown",
        Some("json") => "VsJson",
        Some("yaml" | "yml") => "SiYaml",
        Some("toml") => "SiToml",
        Some("html" | "htm") => "SiHtml5",
        Some("css" | "scss" | "sass" | "less") => "SiCss",
        Some("sh" | "zsh" | "bash") => "SiGnubash",
        Some("sql" | "db" | "sqlite" | "sqlite3") => "SiSqlite",
        Some("png" | "jpg" | "jpeg" | "gif" | "webp" | "svg" | "ico" | "bmp") => "LuImage",
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
            "SiTypescript"
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
            "SiYaml"
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
            "ts", "js", "py", "rs", "go", "java", "php", "rb", "md", "json", "yaml", "toml",
            "html", "css", "sh", "sql", "png", "lock", "bin",
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
