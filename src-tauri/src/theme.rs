use crate::error::{AppError, Result};
use serde::Serialize;
use serde_json::{json, Value};
use std::fs;
use std::io::Read;
use std::path::PathBuf;
use tauri::Manager;

/// VSCode theme files are JSONC: strip // and /* */ comments (outside strings)
/// and trailing commas so serde_json can parse them.
fn strip_jsonc(src: &str) -> String {
    let bytes = src.as_bytes();
    let mut out = String::with_capacity(src.len());
    let mut i = 0;
    let mut in_string = false;
    while i < bytes.len() {
        let c = bytes[i] as char;
        if in_string {
            out.push(c);
            if c == '\\' && i + 1 < bytes.len() {
                out.push(bytes[i + 1] as char);
                i += 1;
            } else if c == '"' {
                in_string = false;
            }
            i += 1;
        } else if c == '"' {
            in_string = true;
            out.push(c);
            i += 1;
        } else if c == '/' && bytes.get(i + 1) == Some(&b'/') {
            while i < bytes.len() && bytes[i] != b'\n' {
                i += 1;
            }
        } else if c == '/' && bytes.get(i + 1) == Some(&b'*') {
            i += 2;
            while i + 1 < bytes.len() && !(bytes[i] == b'*' && bytes[i + 1] == b'/') {
                i += 1;
            }
            i += 2;
        } else if c == ',' {
            // drop trailing comma before } or ]
            let mut j = i + 1;
            while j < bytes.len() && (bytes[j] as char).is_whitespace() {
                j += 1;
            }
            if bytes.get(j) == Some(&b'}') || bytes.get(j) == Some(&b']') {
                i += 1;
            } else {
                out.push(c);
                i += 1;
            }
        } else {
            out.push(c);
            i += 1;
        }
    }
    out
}

fn themes_dir(app: &tauri::AppHandle) -> Result<PathBuf> {
    let dir = app
        .path()
        .app_config_dir()
        .map_err(|e| AppError::Other(e.to_string()))?
        .join("themes");
    fs::create_dir_all(&dir)?;
    Ok(dir)
}

fn slugify(name: &str) -> String {
    let s: String = name
        .to_lowercase()
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { '-' })
        .collect();
    s.split('-').filter(|p| !p.is_empty()).collect::<Vec<_>>().join("-")
}

fn read_zip_file(zip: &mut zip::ZipArchive<fs::File>, path: &str) -> Result<String> {
    let mut f = zip
        .by_name(path)
        .map_err(|_| AppError::Other(format!("File not found in package: {path}")))?;
    let mut s = String::new();
    f.read_to_string(&mut s)?;
    Ok(s.trim_start_matches('\u{feff}').to_string())
}

fn parse_jsonc(src: &str) -> Result<Value> {
    serde_json::from_str(&strip_jsonc(src)).map_err(|e| AppError::Other(format!("Theme parse error: {e}")))
}

/// Join zip-internal paths, resolving `./` and `../`.
fn zip_join(base_dir: &str, rel: &str) -> String {
    let mut parts: Vec<&str> = base_dir.split('/').filter(|p| !p.is_empty()).collect();
    for seg in rel.split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            s => parts.push(s),
        }
    }
    parts.join("/")
}

/// Load a theme JSON from the zip, following its `include` chain.
/// Parent values override included ones.
fn load_theme_json(zip: &mut zip::ZipArchive<fs::File>, path: &str) -> Result<Value> {
    let theme = parse_jsonc(&read_zip_file(zip, path)?);
    let mut theme = theme?;
    if let Some(include) = theme["include"].as_str().map(String::from) {
        let dir = path.rsplit_once('/').map(|(d, _)| d).unwrap_or("");
        let base = load_theme_json(zip, &zip_join(dir, &include))?;
        theme = merge_theme(base, theme);
    }
    Ok(theme)
}

fn merge_theme(base: Value, over: Value) -> Value {
    let mut colors = base["colors"].as_object().cloned().unwrap_or_default();
    if let Some(oc) = over["colors"].as_object() {
        for (k, v) in oc {
            colors.insert(k.clone(), v.clone());
        }
    }
    let mut tokens = base["tokenColors"].as_array().cloned().unwrap_or_default();
    tokens.extend(over["tokenColors"].as_array().cloned().unwrap_or_default());
    json!({
        "name": over["name"].clone(),
        "type": if over["type"].is_string() { over["type"].clone() } else { base["type"].clone() },
        "colors": colors,
        "tokenColors": tokens,
    })
}

#[derive(Serialize)]
pub struct ThemeMeta {
    pub id: String,
    pub name: String,
    pub dark: bool,
}

/// Import all color themes from a VSCode .vsix extension package.
#[tauri::command]
pub fn import_vsix_theme(app: tauri::AppHandle, vsix_path: String) -> Result<Vec<ThemeMeta>> {
    let file = fs::File::open(&vsix_path)?;
    let mut zip = zip::ZipArchive::new(file).map_err(|_| AppError::Other("This file is not a valid .vsix package.".into()))?;
    let manifest = parse_jsonc(&read_zip_file(&mut zip, "extension/package.json")?)?;
    let Some(entries) = manifest["contributes"]["themes"].as_array().cloned() else {
        return Err(AppError::Other("This extension contains no color themes.".into()));
    };
    let dir = themes_dir(&app)?;
    let mut imported = Vec::new();
    for entry in entries {
        let Some(rel) = entry["path"].as_str() else { continue };
        let zip_path = zip_join("extension", rel);
        if !zip_path.ends_with(".json") {
            continue; // .tmTheme XML themes are out of scope
        }
        let Ok(theme) = load_theme_json(&mut zip, &zip_path) else { continue };
        let label = entry["label"]
            .as_str()
            .or(theme["name"].as_str())
            .unwrap_or("Imported theme")
            .to_string();
        let dark = entry["uiTheme"].as_str().map(|u| u != "vs") // "vs" = light, "vs-dark"/"hc-black" = dark
            .or(theme["type"].as_str().map(|t| t != "light"))
            .unwrap_or(true);
        let id = slugify(&label);
        let normalized = json!({
            "id": id,
            "name": label,
            "dark": dark,
            "colors": theme["colors"],
            "tokenColors": theme["tokenColors"],
        });
        fs::write(dir.join(format!("{id}.json")), serde_json::to_string(&normalized).unwrap())?;
        imported.push(ThemeMeta { id, name: label, dark });
    }
    if imported.is_empty() {
        return Err(AppError::Other("No importable color themes found in this package.".into()));
    }
    Ok(imported)
}

#[tauri::command]
pub fn list_imported_themes(app: tauri::AppHandle) -> Result<Vec<ThemeMeta>> {
    let dir = themes_dir(&app)?;
    let mut out = Vec::new();
    for e in fs::read_dir(dir)? {
        let path = e?.path();
        if path.extension().is_some_and(|x| x == "json") {
            if let Ok(v) = serde_json::from_str::<Value>(&fs::read_to_string(&path)?) {
                out.push(ThemeMeta {
                    id: v["id"].as_str().unwrap_or_default().into(),
                    name: v["name"].as_str().unwrap_or_default().into(),
                    dark: v["dark"].as_bool().unwrap_or(true),
                });
            }
        }
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(out)
}

#[tauri::command]
pub fn get_imported_theme(app: tauri::AppHandle, id: String) -> Result<Value> {
    let path = themes_dir(&app)?.join(format!("{}.json", slugify(&id)));
    Ok(serde_json::from_str(&fs::read_to_string(path)?)
        .map_err(|e| AppError::Other(e.to_string()))?)
}

#[tauri::command]
pub fn delete_imported_theme(app: tauri::AppHandle, id: String) -> Result<()> {
    let path = themes_dir(&app)?.join(format!("{}.json", slugify(&id)));
    if path.exists() {
        fs::remove_file(path)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn jsonc_strips_comments_and_trailing_commas() {
        let src = r#"{
            // line comment
            "a": "value // not a comment",
            /* block */ "b": [1, 2,],
        }"#;
        let v: Value = serde_json::from_str(&strip_jsonc(src)).unwrap();
        assert_eq!(v["a"], "value // not a comment");
        assert_eq!(v["b"][1], 2);
    }

    #[test]
    fn zip_paths_resolve() {
        assert_eq!(zip_join("extension", "./themes/x.json"), "extension/themes/x.json");
        assert_eq!(zip_join("extension/themes", "../base.json"), "extension/base.json");
    }

    #[test]
    fn slugs() {
        assert_eq!(slugify("Dracula (Official) Theme!"), "dracula-official-theme");
    }

    /// End-to-end parse of a real .vsix. Skips unless PUGDOCK_TEST_VSIX points
    /// at a downloaded package, so CI doesn't need network.
    #[test]
    fn parses_real_vsix_when_provided() {
        let Some(path) = std::env::var_os("PUGDOCK_TEST_VSIX") else { return };
        let file = fs::File::open(path).unwrap();
        let mut zip = zip::ZipArchive::new(file).unwrap();
        let manifest = parse_jsonc(&read_zip_file(&mut zip, "extension/package.json").unwrap()).unwrap();
        let themes = manifest["contributes"]["themes"].as_array().unwrap().clone();
        assert!(!themes.is_empty());
        for t in &themes {
            let p = zip_join("extension", t["path"].as_str().unwrap());
            let theme = load_theme_json(&mut zip, &p).unwrap();
            assert!(theme["colors"].is_object(), "colors missing in {p}");
            assert!(theme["tokenColors"].is_array(), "tokenColors missing in {p}");
        }
    }
}
