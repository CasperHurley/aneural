//! Tiny template language for spore manifests: `{var}`, `{fn(var)}` with
//! `hash`, `slug`, `upper`, `lower`, `trim`, and dotted keys like `{fm.status}`.

use regex::Regex;
use std::collections::BTreeMap;
use std::sync::OnceLock;

pub type Vars = BTreeMap<String, String>;

fn pattern() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"\{([A-Za-z_][A-Za-z0-9_.]*)(?:\(([A-Za-z_][A-Za-z0-9_.]*)\))?\}").unwrap()
    })
}

/// Render a template. Unknown variables render as empty strings.
pub fn render(template: &str, vars: &Vars) -> String {
    render_with(template, vars, |_, value| value.to_string())
}

/// Render, passing every substituted value through `f` along with the variable
/// name it came from. URLs use this to escape a value according to how much the
/// value is trusted: a key the user typed into their own config is not the same
/// as a string a remote API just handed back.
pub fn render_with(template: &str, vars: &Vars, f: impl Fn(&str, &str) -> String) -> String {
    pattern()
        .replace_all(template, |caps: &regex::Captures<'_>| {
            let name = &caps[1];
            match caps.get(2) {
                None => f(name, &vars.get(name).cloned().unwrap_or_default()),
                Some(arg) => {
                    let value = vars.get(arg.as_str()).cloned().unwrap_or_default();
                    f(arg.as_str(), &apply(name, &value))
                }
            }
        })
        .into_owned()
}

fn apply(func: &str, value: &str) -> String {
    match func {
        "hash" => aneural_core::short_hash(value.trim()),
        "slug" => aneural_core::slug(value),
        "upper" => value.to_uppercase(),
        "lower" => value.to_lowercase(),
        "trim" => value.trim().to_string(),
        "basename" => value.rsplit('/').next().unwrap_or(value).to_string(),
        "stem" => {
            let base = value.rsplit('/').next().unwrap_or(value);
            base.rsplit_once('.')
                .map(|(s, _)| s.to_string())
                .unwrap_or_else(|| base.to_string())
        }
        _ => value.to_string(),
    }
}

/// Coerce a rendered prop into JSON: integers become numbers, `true`/`false`
/// become booleans, everything else stays a string.
pub fn coerce(value: &str) -> serde_json::Value {
    if let Ok(i) = value.parse::<i64>() {
        return serde_json::Value::from(i);
    }
    match value {
        "true" => serde_json::Value::Bool(true),
        "false" => serde_json::Value::Bool(false),
        _ => serde_json::Value::String(value.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_vars_and_functions() {
        let mut v = Vars::new();
        v.insert("file".into(), "src/a.ts".into());
        v.insert("text".into(), " Fix Me ".into());
        v.insert("fm.status".into(), "open".into());
        assert_eq!(
            render("comment:{file}#{hash(text)}", &v),
            format!("comment:src/a.ts#{}", aneural_core::short_hash("Fix Me"))
        );
        assert_eq!(
            render("{slug(text)}/{upper(text)}/{fm.status}/{missing}", &v),
            "fix-me/ FIX ME /open/"
        );
        assert_eq!(render("{stem(file)}", &v), "a");
        assert_eq!(coerce("12"), serde_json::json!(12));
        assert_eq!(coerce("x"), serde_json::json!("x"));
    }
}
