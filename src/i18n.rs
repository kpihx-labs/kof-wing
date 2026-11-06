use std::{
    collections::{BTreeMap, BTreeSet},
    fs, io,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, bail, ensure};
use serde::{Deserialize, Serialize};

use crate::input::{atomic_write, atomic_write_new};

pub const LANGUAGE_SCHEMA_VERSION: u32 = 1;
pub const CATALOG_SCHEMA_VERSION: u32 = 1;
pub const EMBEDDED_TEXT_SYMBOLS: usize = 675;
pub const EMBEDDED_HAN_SYMBOLS: usize = 241;
const MAX_CATALOG_BYTES: u64 = 1024 * 1024;

const EN: &str = include_str!("../locales/en.json");
const FR: &str = include_str!("../locales/fr.json");
const REQUIRED_KEYS: &[&str] = &[
    "coverage.blocker",
    "coverage.embedded",
    "coverage.owned",
    "coverage.title",
    "menu.doctor",
    "menu.input",
    "menu.lan",
    "menu.language",
    "menu.play",
    "menu.quit",
    "menu.subtitle",
    "menu.title",
    "notice.embedded",
    "status.blocked",
    "status.ready",
];

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct LanguageConfig {
    pub schema_version: u32,
    pub id: String,
    pub catalog_version: u32,
}

impl Default for LanguageConfig {
    fn default() -> Self {
        Self {
            schema_version: LANGUAGE_SCHEMA_VERSION,
            id: "en".to_owned(),
            catalog_version: 1,
        }
    }
}

impl LanguageConfig {
    pub fn validate(&self) -> Result<()> {
        if self.schema_version != LANGUAGE_SCHEMA_VERSION {
            bail!(
                "unsupported language profile schema version {}; expected {}",
                self.schema_version,
                LANGUAGE_SCHEMA_VERSION
            );
        }
        let canonical = canonical_language_id(&self.id)?;
        if canonical != self.id {
            bail!(
                "language id '{}' is not canonical; use '{canonical}'",
                self.id
            );
        }
        if self.catalog_version == 0 {
            bail!("catalog_version must be greater than zero");
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Catalog {
    pub schema_version: u32,
    pub catalog_version: u32,
    pub id: String,
    pub name: String,
    pub fallback: Option<String>,
    pub messages: BTreeMap<String, String>,
}

impl Catalog {
    pub fn from_json(text: &str) -> Result<Self> {
        if text.len() > 1024 * 1024 {
            bail!("catalog exceeds the 1 MiB safety limit");
        }
        let catalog: Self = serde_json::from_str(text).context("invalid language catalog JSON")?;
        catalog.validate_structure()?;
        Ok(catalog)
    }

    pub fn to_json(&self) -> Result<String> {
        Ok(format!("{}\n", serde_json::to_string_pretty(self)?))
    }

    pub fn validate_structure(&self) -> Result<()> {
        if self.schema_version != CATALOG_SCHEMA_VERSION {
            bail!(
                "unsupported catalog schema version {}; expected {}",
                self.schema_version,
                CATALOG_SCHEMA_VERSION
            );
        }
        if self.catalog_version == 0 {
            bail!("catalog_version must be greater than zero");
        }
        let canonical = canonical_language_id(&self.id)?;
        if canonical != self.id {
            bail!(
                "catalog id '{}' is not canonical; use '{canonical}'",
                self.id
            );
        }
        if self.name.trim().is_empty() || self.name.contains(['\n', '\r']) {
            bail!("catalog name must be a non-empty single line");
        }
        if let Some(fallback) = &self.fallback {
            let canonical_fallback = canonical_language_id(fallback)?;
            if canonical_fallback != *fallback {
                bail!("fallback id '{fallback}' is not canonical; use '{canonical_fallback}'");
            }
            if fallback == &self.id {
                bail!("catalog cannot fall back to itself");
            }
        }
        for (key, value) in &self.messages {
            if !valid_message_key(key) {
                bail!("unsafe message id '{key}'");
            }
            if value.trim().is_empty() {
                bail!("message '{key}' is empty");
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct CoverageReport {
    pub language: String,
    pub catalog_version: u32,
    pub owned_total: usize,
    pub owned_direct: usize,
    pub owned_fallback: usize,
    pub fallback_chain: Vec<String>,
    pub missing: Vec<String>,
    pub embedded_total: usize,
    pub embedded_han: usize,
    pub embedded_verified_patched: usize,
    pub embedded_blockers: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct ResolvedCatalog {
    pub catalog: Catalog,
    pub messages: BTreeMap<String, String>,
    pub coverage: CoverageReport,
}

/// Normalize casing in our path-safe, 2-63 byte subset of BCP 47 syntax.
/// This is not IANA registry validation/canonicalization: grandfathered tags,
/// preferred-value aliases, extlang prefixes and extension semantics are not
/// resolved. Semantically equivalent registry aliases remain distinct IDs.
pub fn canonical_language_id(value: &str) -> Result<String> {
    if value.is_empty() || value.len() > 63 || value.starts_with('-') || value.ends_with('-') {
        bail!("language id must be a 2-63 character BCP 47 tag");
    }
    if !value.is_ascii() {
        bail!("language id must be ASCII BCP 47 text");
    }
    let parts: Vec<&str> = value.split('-').collect();
    if parts.iter().any(|part| part.is_empty()) {
        bail!("language id contains an empty BCP 47 subtag");
    }
    let mut canonical = Vec::with_capacity(parts.len());

    if parts[0].eq_ignore_ascii_case("x") {
        canonical.push("x".to_owned());
        let mut index = 1;
        ensure!(
            index < parts.len(),
            "private-use language ids must include at least one subtag"
        );
        while index < parts.len() {
            let part = parts[index];
            ensure!(
                is_alphanumeric_len(part, 1, 8),
                "private-use BCP 47 subtags must be 1-8 ASCII alphanumerics"
            );
            canonical.push(part.to_ascii_lowercase());
            index += 1;
        }
        return Ok(canonical.join("-"));
    }

    let language = parts[0];
    ensure!(
        is_primary_language(language),
        "language id must begin with a valid BCP 47 primary language subtag"
    );
    canonical.push(language.to_ascii_lowercase());
    let mut index = 1;

    if language.len() <= 3 {
        let mut extlangs = 0;
        while index < parts.len() && extlangs < 3 && is_alpha_len(parts[index], 3) {
            canonical.push(parts[index].to_ascii_lowercase());
            index += 1;
            extlangs += 1;
        }
    }

    if index < parts.len() && is_alpha_len(parts[index], 4) {
        canonical.push(titlecase(parts[index]));
        index += 1;
    }

    if index < parts.len() && (is_alpha_len(parts[index], 2) || is_digit_len(parts[index], 3)) {
        canonical.push(parts[index].to_ascii_uppercase());
        index += 1;
    }

    let mut seen_variants = BTreeSet::new();
    while index < parts.len() && is_variant(parts[index]) {
        ensure!(
            seen_variants.insert(parts[index].to_ascii_lowercase()),
            "language id repeats a variant subtag"
        );
        canonical.push(parts[index].to_ascii_lowercase());
        index += 1;
    }

    let mut seen_extensions = BTreeSet::new();
    while index < parts.len() {
        let part = parts[index];
        if part.eq_ignore_ascii_case("x") {
            canonical.push("x".to_owned());
            index += 1;
            ensure!(
                index < parts.len(),
                "private-use language ids must include at least one subtag"
            );
            while index < parts.len() {
                let subtag = parts[index];
                ensure!(
                    is_alphanumeric_len(subtag, 1, 8),
                    "private-use BCP 47 subtags must be 1-8 ASCII alphanumerics"
                );
                canonical.push(subtag.to_ascii_lowercase());
                index += 1;
            }
            break;
        }

        ensure!(
            is_singleton(part),
            "language id contains an invalid BCP 47 extension or private-use subtag"
        );
        let singleton = part.as_bytes()[0].to_ascii_lowercase();
        ensure!(
            seen_extensions.insert(singleton),
            "language id repeats the '{}' extension singleton",
            singleton as char
        );
        canonical.push(part.to_ascii_lowercase());
        index += 1;
        let start = index;
        while index < parts.len() && is_alphanumeric_len(parts[index], 2, 8) {
            canonical.push(parts[index].to_ascii_lowercase());
            index += 1;
        }
        ensure!(
            index > start,
            "extension '{}' must include at least one 2-8 character subtag",
            singleton as char
        );
    }
    Ok(canonical.join("-"))
}

pub fn builtin_catalogs() -> Result<Vec<Catalog>> {
    [EN, FR].into_iter().map(Catalog::from_json).collect()
}

pub fn list(catalog_dir: &Path) -> Result<Vec<Catalog>> {
    let mut catalogs: BTreeMap<String, Catalog> = builtin_catalogs()?
        .into_iter()
        .map(|catalog| (catalog.id.clone(), catalog))
        .collect();
    match fs::symlink_metadata(catalog_dir) {
        Ok(metadata) => {
            ensure!(
                metadata.is_dir() && !metadata.file_type().is_symlink(),
                "language catalog directory is not a real directory: {}",
                catalog_dir.display()
            );
            let mut paths = Vec::new();
            for entry in fs::read_dir(catalog_dir)
                .with_context(|| format!("cannot list {}", catalog_dir.display()))?
            {
                let entry = entry?;
                if entry.file_type()?.is_file()
                    && entry.path().extension().and_then(|value| value.to_str()) == Some("json")
                {
                    paths.push(entry.path());
                }
            }
            paths.sort();
            let mut local = BTreeMap::new();
            for path in paths {
                let catalog = read_catalog(&path)?;
                ensure!(
                    !local.contains_key(&catalog.id),
                    "duplicate local language catalog id '{}'",
                    catalog.id
                );
                local.insert(catalog.id.clone(), (path, catalog));
            }
            for (id, (path, catalog)) in local {
                ensure!(
                    path.file_stem().and_then(|stem| stem.to_str()) == Some(id.as_str()),
                    "language catalog filename stem must equal canonical id '{id}': {}",
                    path.display()
                );
                // Exactly one correctly named local override of a builtin is allowed.
                catalogs.insert(id, catalog);
            }
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => {
            return Err(error).with_context(|| format!("cannot inspect {}", catalog_dir.display()));
        }
    }
    Ok(catalogs.into_values().collect())
}

pub fn resolve(id: &str, catalog_dir: &Path) -> Result<ResolvedCatalog> {
    let id = canonical_language_id(id)?;
    let available: BTreeMap<String, Catalog> = list(catalog_dir)?
        .into_iter()
        .map(|catalog| (catalog.id.clone(), catalog))
        .collect();
    let catalog = available
        .get(&id)
        .cloned()
        .with_context(|| format!("language catalog '{id}' is not installed"))?;
    let mut chain = Vec::new();
    let mut seen = BTreeSet::from([id.clone()]);
    let mut cursor = catalog.fallback.clone();
    while let Some(fallback) = cursor {
        if !seen.insert(fallback.clone()) {
            bail!("language fallback cycle detected at '{fallback}'");
        }
        let next = available
            .get(&fallback)
            .with_context(|| format!("fallback catalog '{fallback}' is not installed"))?;
        chain.push(next.clone());
        cursor = next.fallback.clone();
    }

    let mut messages = BTreeMap::new();
    let mut direct = 0;
    let mut fallback_count = 0;
    let mut missing = Vec::new();
    for key in REQUIRED_KEYS {
        if let Some(value) = catalog.messages.get(*key) {
            messages.insert((*key).to_owned(), value.clone());
            direct += 1;
        } else if let Some(value) = chain.iter().find_map(|item| item.messages.get(*key)) {
            messages.insert((*key).to_owned(), value.clone());
            fallback_count += 1;
        } else {
            missing.push((*key).to_owned());
        }
    }
    let coverage = CoverageReport {
        language: catalog.id.clone(),
        catalog_version: catalog.catalog_version,
        owned_total: REQUIRED_KEYS.len(),
        owned_direct: direct,
        owned_fallback: fallback_count,
        fallback_chain: chain.iter().map(|item| item.id.clone()).collect(),
        missing,
        embedded_total: EMBEDDED_TEXT_SYMBOLS,
        embedded_han: EMBEDDED_HAN_SYMBOLS,
        embedded_verified_patched: 0,
        embedded_blockers: vec![
            "241 exported text symbols contain Han characters; complete translations are not yet audited".to_owned(),
            "FFDec text import is structurally reproducible, but Latin glyph coverage and in-game layout still require visual verification".to_owned(),
            "script-generated text and external skillList.swf text are outside the static 675-symbol export".to_owned(),
        ],
    };
    Ok(ResolvedCatalog {
        catalog,
        messages,
        coverage,
    })
}

pub fn resolve_selected(config: &LanguageConfig, catalog_dir: &Path) -> Result<ResolvedCatalog> {
    config.validate()?;
    let resolved = resolve(&config.id, catalog_dir)?;
    ensure!(
        resolved.catalog.catalog_version == config.catalog_version,
        "configured language '{}' expects catalog v{}, but installed catalog is v{}; rerun `kof-wing --profile <name> language set {}` after reviewing the replacement",
        config.id,
        config.catalog_version,
        resolved.catalog.catalog_version,
        config.id
    );
    Ok(resolved)
}

pub fn compatibility_contract(config: &LanguageConfig, catalog_dir: &Path) -> Result<String> {
    #[derive(Serialize)]
    struct CompatibilityContract<'a> {
        schema_version: u32,
        language: &'a str,
        catalog_version: u32,
        fallback_chain: &'a [String],
        messages: &'a BTreeMap<String, String>,
    }

    let resolved = resolve_selected(config, catalog_dir)?;
    let contract = CompatibilityContract {
        schema_version: config.schema_version,
        language: &resolved.catalog.id,
        catalog_version: resolved.catalog.catalog_version,
        fallback_chain: &resolved.coverage.fallback_chain,
        messages: &resolved.messages,
    };
    Ok(format!("{}\n", serde_json::to_string_pretty(&contract)?))
}

pub fn install(path: &Path, catalog_dir: &Path) -> Result<PathBuf> {
    let catalog = read_catalog(path)?;
    ensure_real_directory_tree(catalog_dir)?;
    let destination = catalog_dir.join(format!("{}.json", catalog.id));
    atomic_write(&destination, catalog.to_json()?.as_bytes())?;
    Ok(destination)
}

pub fn export(catalog: &Catalog, path: &Path) -> Result<()> {
    if path.extension().and_then(|value| value.to_str()) != Some("json") {
        bail!("language export path must end in .json");
    }
    ensure_real_parent_directory(path)?;
    if fs::symlink_metadata(path).is_ok() {
        bail!("refusing to overwrite existing export {}", path.display());
    }
    atomic_write_new(path, catalog.to_json()?.as_bytes())
}

pub fn write_wrapper(overlay: &Path, resolved: &ResolvedCatalog) -> Result<PathBuf> {
    if !resolved.coverage.missing.is_empty() {
        bail!(
            "language '{}' is missing wrapper messages: {}",
            resolved.catalog.id,
            resolved.coverage.missing.join(", ")
        );
    }
    let message = |key: &str| html_escape(&resolved.messages[key]);
    let html = format!(
        "<!doctype html>\n<html lang=\"{}\"><meta charset=\"utf-8\"><meta http-equiv=\"Content-Security-Policy\" content=\"default-src 'none'; style-src 'unsafe-inline'; base-uri 'none'; form-action 'none'; img-src 'none'; connect-src 'none'; frame-ancestors 'none'\"><title>{}</title>\n<style>body{{font:18px system-ui;background:#111;color:#eee;max-width:760px;margin:4rem auto;padding:2rem}}h1{{color:#f0b429}}nav{{display:grid;grid-template-columns:repeat(2,1fr);gap:.8rem}}button{{padding:1rem;background:#252525;color:#fff;border:1px solid #666}}.blocked{{color:#ffb4a2}}</style>\n<h1>{}</h1><p>{}</p><nav><button>{}</button><button>{}</button><button>{}</button><button>{}</button><button>{}</button><button>{}</button></nav><h2>{}</h2><p>{}: {}/{} ({})</p><p class=\"blocked\">{}: 0/{} ({})</p><p class=\"blocked\">{}</p><p>{}</p></html>\n",
        resolved.catalog.id,
        message("menu.title"),
        message("menu.title"),
        message("menu.subtitle"),
        message("menu.play"),
        message("menu.doctor"),
        message("menu.input"),
        message("menu.language"),
        message("menu.lan"),
        message("menu.quit"),
        message("coverage.title"),
        message("coverage.owned"),
        resolved.coverage.owned_direct,
        resolved.coverage.owned_total,
        message("status.ready"),
        message("coverage.embedded"),
        resolved.coverage.embedded_han,
        message("status.blocked"),
        message("coverage.blocker"),
        message("notice.embedded")
    );
    let destination = overlay.join("kof-wing-menu.html");
    atomic_write(&destination, html.as_bytes())?;
    Ok(destination)
}

fn read_catalog(path: &Path) -> Result<Catalog> {
    let metadata = fs::symlink_metadata(path)
        .with_context(|| format!("cannot inspect language catalog {}", path.display()))?;
    ensure!(
        metadata.is_file() && !metadata.file_type().is_symlink(),
        "language catalog is not a regular file: {}",
        path.display()
    );
    ensure!(
        metadata.len() <= MAX_CATALOG_BYTES,
        "catalog exceeds the 1 MiB safety limit"
    );
    let text = fs::read_to_string(path)
        .with_context(|| format!("cannot read language catalog {}", path.display()))?;
    Catalog::from_json(&text).with_context(|| format!("invalid catalog {}", path.display()))
}

fn ensure_real_parent_directory(path: &Path) -> Result<()> {
    ensure_real_directory_tree(destination_parent(path))
}

fn ensure_real_directory_tree(path: &Path) -> Result<()> {
    let mut directory = PathBuf::new();
    for component in path.components() {
        match component {
            std::path::Component::Prefix(prefix) => directory.push(prefix.as_os_str()),
            std::path::Component::RootDir => directory.push(component.as_os_str()),
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                bail!("unsafe parent directory {}", path.display());
            }
            std::path::Component::Normal(component) => {
                directory.push(component);
                ensure_real_directory(&directory)?;
            }
        }
    }
    Ok(())
}

fn ensure_real_directory(path: &Path) -> Result<()> {
    match fs::symlink_metadata(path) {
        Ok(metadata) => ensure!(
            metadata.is_dir() && !metadata.file_type().is_symlink(),
            "language directory is not a real directory: {}",
            path.display()
        ),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            fs::create_dir(path).with_context(|| format!("cannot create {}", path.display()))?;
            let metadata = fs::symlink_metadata(path)?;
            ensure!(
                metadata.is_dir() && !metadata.file_type().is_symlink(),
                "language directory is not a real directory: {}",
                path.display()
            );
        }
        Err(error) => {
            return Err(error).with_context(|| format!("cannot inspect {}", path.display()));
        }
    }
    Ok(())
}

fn destination_parent(path: &Path) -> &Path {
    path.parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."))
}

fn valid_message_key(key: &str) -> bool {
    !key.is_empty()
        && key.len() <= 96
        && key.split('.').all(|part| {
            !part.is_empty()
                && part.len() <= 32
                && part
                    .bytes()
                    .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
        })
}

fn html_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

fn is_primary_language(value: &str) -> bool {
    (2..=8).contains(&value.len()) && value.bytes().all(|byte| byte.is_ascii_alphabetic())
}

fn is_alpha_len(value: &str, len: usize) -> bool {
    value.len() == len && value.bytes().all(|byte| byte.is_ascii_alphabetic())
}

fn is_digit_len(value: &str, len: usize) -> bool {
    value.len() == len && value.bytes().all(|byte| byte.is_ascii_digit())
}

fn is_alphanumeric_len(value: &str, min: usize, max: usize) -> bool {
    (min..=max).contains(&value.len()) && value.bytes().all(|byte| byte.is_ascii_alphanumeric())
}

fn is_variant(value: &str) -> bool {
    is_alphanumeric_len(value, 5, 8)
        || (value.len() == 4
            && value.as_bytes()[0].is_ascii_digit()
            && value.bytes().all(|byte| byte.is_ascii_alphanumeric()))
}

fn is_singleton(value: &str) -> bool {
    value.len() == 1 && value.bytes().all(|byte| byte.is_ascii_alphanumeric())
}

fn titlecase(value: &str) -> String {
    let mut chars = value.to_ascii_lowercase().chars().collect::<Vec<_>>();
    chars[0] = chars[0].to_ascii_uppercase();
    chars.into_iter().collect()
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::tempdir;

    #[cfg(unix)]
    use super::export;
    use super::{
        Catalog, LanguageConfig, MAX_CATALOG_BYTES, canonical_language_id, compatibility_contract,
        install, list, resolve, resolve_selected, write_wrapper,
    };

    #[test]
    fn canonicalizes_safe_bcp47_ids() {
        assert_eq!(canonical_language_id("fr-FR").unwrap(), "fr-FR");
        assert_eq!(canonical_language_id("zh-hans-cn").unwrap(), "zh-Hans-CN");
        assert_eq!(canonical_language_id("de-ch-1901").unwrap(), "de-CH-1901");
        assert_eq!(
            canonical_language_id("en-x-build42").unwrap(),
            "en-x-build42"
        );
        assert_eq!(canonical_language_id("x-kof-wing").unwrap(), "x-kof-wing");
        for invalid in [
            "../fr",
            "f",
            "fr_FR",
            "fr--FR",
            "fr/FR",
            "en-a",
            "en-a-foo-a-bar",
            "de-1901-1901",
            "i-klingon",
        ] {
            assert!(
                canonical_language_id(invalid).is_err(),
                "accepted {invalid}"
            );
        }
    }

    #[test]
    fn builtins_are_complete_without_fallback() {
        let temp = tempdir().unwrap();
        for id in ["en", "fr"] {
            let resolved = resolve(id, temp.path()).unwrap();
            assert_eq!(
                resolved.coverage.owned_direct,
                resolved.coverage.owned_total
            );
            assert_eq!(resolved.coverage.owned_fallback, 0);
            assert!(resolved.coverage.missing.is_empty());
        }
    }

    #[test]
    fn ids_are_syntax_normalized_not_registry_canonicalized() {
        // Deprecated aliases and unregistered subtags are not resolved/validated.
        assert_eq!(canonical_language_id("iw-IL").unwrap(), "iw-IL");
        assert_eq!(canonical_language_id("zz-ZZ").unwrap(), "zz-ZZ");
        assert_eq!(
            canonical_language_id("en-b-foo-a-bar").unwrap(),
            "en-b-foo-a-bar"
        );
        assert!(canonical_language_id(&format!("en-x-{}", "a-".repeat(32))).is_err());
    }

    fn local_catalog(directory: &std::path::Path, file: &str, id: &str, fallback: Option<&str>) {
        let mut catalog = super::builtin_catalogs().unwrap().remove(0);
        catalog.id = id.to_owned();
        catalog.fallback = fallback.map(str::to_owned);
        fs::write(directory.join(file), catalog.to_json().unwrap()).unwrap();
    }

    #[test]
    fn fallback_cycles_across_multiple_catalogs_are_rejected_even_when_complete() {
        for chain in [vec!["de", "es"], vec!["de", "es", "it"]] {
            let temp = tempdir().unwrap();
            for (index, id) in chain.iter().enumerate() {
                local_catalog(
                    temp.path(),
                    &format!("{id}.json"),
                    id,
                    Some(chain[(index + 1) % chain.len()]),
                );
            }
            for id in &chain {
                let error = resolve(id, temp.path()).unwrap_err().to_string();
                assert!(error.contains("fallback cycle detected"), "{error}");
            }
        }
    }

    #[test]
    fn absent_fallback_catalog_is_rejected_including_transitive_fallback() {
        let temp = tempdir().unwrap();
        local_catalog(temp.path(), "de.json", "de", Some("es"));
        local_catalog(temp.path(), "it.json", "it", Some("de"));
        for id in ["de", "it"] {
            let error = resolve(id, temp.path()).unwrap_err().to_string();
            assert!(
                error.contains("fallback catalog 'es' is not installed"),
                "{error}"
            );
        }
    }

    #[test]
    fn local_catalog_filename_must_match_canonical_id() {
        for file in ["other.json", "FR-ca.json"] {
            let temp = tempdir().unwrap();
            local_catalog(temp.path(), file, "fr-CA", Some("fr"));
            assert!(
                list(temp.path())
                    .unwrap_err()
                    .to_string()
                    .contains("filename stem must equal canonical id 'fr-CA'")
            );
        }
    }

    #[test]
    fn duplicate_local_ids_fail_explicitly_regardless_of_creation_order() {
        for files in [["a.json", "fr.json"], ["fr.json", "a.json"]] {
            let temp = tempdir().unwrap();
            for file in files {
                local_catalog(temp.path(), file, "fr", None);
            }
            assert!(
                list(temp.path())
                    .unwrap_err()
                    .to_string()
                    .contains("duplicate local language catalog id 'fr'")
            );
            assert!(compatibility_contract(&LanguageConfig::default(), temp.path()).is_err());
        }
    }

    #[test]
    fn correctly_named_local_override_is_unique_and_listing_is_sorted() {
        let temp = tempdir().unwrap();
        local_catalog(temp.path(), "fr.json", "fr", None);
        local_catalog(temp.path(), "de.json", "de", None);
        let ids: Vec<_> = list(temp.path())
            .unwrap()
            .into_iter()
            .map(|catalog| catalog.id)
            .collect();
        assert_eq!(ids, ["de", "en", "fr"]);
    }

    #[test]
    fn fallback_is_explicitly_reported() {
        let temp = tempdir().unwrap();
        let catalog = Catalog::from_json(
            r#"{"schema_version":1,"catalog_version":1,"id":"fr-CA","name":"Français canadien","fallback":"fr","messages":{"menu.play":"Jouer au jeu"}}"#,
        )
        .unwrap();
        fs::write(temp.path().join("fr-CA.json"), catalog.to_json().unwrap()).unwrap();
        let resolved = resolve("fr-CA", temp.path()).unwrap();
        assert_eq!(resolved.coverage.fallback_chain, ["fr"]);
        assert!(resolved.coverage.owned_fallback > 0);
    }

    #[test]
    fn selected_catalog_version_must_match_installed_catalog() {
        let temp = tempdir().unwrap();
        let catalog = Catalog::from_json(
            r#"{"schema_version":1,"catalog_version":2,"id":"fr-CA","name":"Français canadien","fallback":"fr","messages":{"menu.play":"Jouer au jeu"}}"#,
        )
        .unwrap();
        fs::write(temp.path().join("fr-CA.json"), catalog.to_json().unwrap()).unwrap();
        let error = resolve_selected(
            &LanguageConfig {
                schema_version: 1,
                id: "fr-CA".to_owned(),
                catalog_version: 1,
            },
            temp.path(),
        )
        .unwrap_err();
        assert!(error.to_string().contains("expects catalog v1"));
    }

    #[test]
    fn compatibility_contract_hashes_effective_resolved_messages() {
        let first = tempdir().unwrap();
        let second = tempdir().unwrap();
        let first_catalog = Catalog::from_json(
            r#"{"schema_version":1,"catalog_version":1,"id":"fr-CA","name":"Français canadien","fallback":"fr","messages":{"menu.play":"Jouer au jeu"}}"#,
        )
        .unwrap();
        let second_catalog = Catalog::from_json(
            r#"{"schema_version":1,"catalog_version":1,"id":"fr-CA","name":"Français canadien","fallback":"fr","messages":{"menu.play":"Lancer le jeu"}}"#,
        )
        .unwrap();
        fs::write(
            first.path().join("fr-CA.json"),
            first_catalog.to_json().unwrap(),
        )
        .unwrap();
        fs::write(
            second.path().join("fr-CA.json"),
            second_catalog.to_json().unwrap(),
        )
        .unwrap();
        let config = LanguageConfig {
            schema_version: 1,
            id: "fr-CA".to_owned(),
            catalog_version: 1,
        };
        assert_ne!(
            compatibility_contract(&config, first.path()).unwrap(),
            compatibility_contract(&config, second.path()).unwrap()
        );
    }

    #[cfg(unix)]
    #[test]
    fn export_refuses_symlink_and_preserves_target() {
        use std::os::unix::fs::symlink;

        let temp = tempdir().unwrap();
        let catalog = resolve("en", temp.path()).unwrap().catalog;
        let target = temp.path().join("target.json");
        let link = temp.path().join("export.json");
        fs::write(&target, "keep").unwrap();
        symlink(&target, &link).unwrap();
        assert!(export(&catalog, &link).is_err());
        assert_eq!(fs::read_to_string(target).unwrap(), "keep");
    }

    #[cfg(unix)]
    #[test]
    fn import_replaces_catalog_symlink_without_touching_target() {
        use std::os::unix::fs::symlink;

        let temp = tempdir().unwrap();
        let source = temp.path().join("source.json");
        let catalog_dir = temp.path().join("catalogs");
        let target = temp.path().join("target.json");
        fs::create_dir(&catalog_dir).unwrap();
        fs::write(&target, "keep").unwrap();
        symlink(&target, catalog_dir.join("fr-CA.json")).unwrap();
        let catalog = Catalog::from_json(
            r#"{"schema_version":1,"catalog_version":1,"id":"fr-CA","name":"Français canadien","fallback":"fr","messages":{"menu.play":"Jouer"}}"#,
        )
        .unwrap();
        fs::write(&source, catalog.to_json().unwrap()).unwrap();

        install(&source, &catalog_dir).unwrap();

        assert_eq!(fs::read_to_string(target).unwrap(), "keep");
        assert!(
            !fs::symlink_metadata(catalog_dir.join("fr-CA.json"))
                .unwrap()
                .file_type()
                .is_symlink()
        );
    }

    #[cfg(unix)]
    #[test]
    fn export_rejects_symlinked_parent_directory() {
        use std::os::unix::fs::symlink;

        let temp = tempdir().unwrap();
        let outside = temp.path().join("outside");
        let link = temp.path().join("linked");
        fs::create_dir(&outside).unwrap();
        symlink(&outside, &link).unwrap();

        let catalog = resolve("en", temp.path()).unwrap().catalog;
        let error = export(&catalog, &link.join("export.json")).unwrap_err();
        assert!(error.to_string().contains("real directory"));
        assert!(!outside.join("export.json").exists());
    }

    #[cfg(unix)]
    #[test]
    fn import_rejects_symlinked_catalog_directory() {
        use std::os::unix::fs::symlink;

        let temp = tempdir().unwrap();
        let outside = temp.path().join("outside");
        let source = temp.path().join("source.json");
        let catalog_dir = temp.path().join("catalogs");
        fs::create_dir(&outside).unwrap();
        symlink(&outside, &catalog_dir).unwrap();
        let catalog = Catalog::from_json(
            r#"{"schema_version":1,"catalog_version":1,"id":"fr-CA","name":"Français canadien","fallback":"fr","messages":{"menu.play":"Jouer"}}"#,
        )
        .unwrap();
        fs::write(&source, catalog.to_json().unwrap()).unwrap();

        let error = install(&source, &catalog_dir).unwrap_err();
        assert!(error.to_string().contains("real directory"));
        assert!(!outside.join("fr-CA.json").exists());
    }

    #[test]
    fn wrapper_escapes_html_and_sets_csp() {
        let temp = tempdir().unwrap();
        let mut resolved = resolve("en", temp.path()).unwrap();
        resolved.catalog.id = "en-x-test".to_owned();
        resolved.messages.insert(
            "menu.subtitle".to_owned(),
            "\"<script>alert('x')</script>\"".to_owned(),
        );

        let wrapper = write_wrapper(temp.path(), &resolved).unwrap();
        let html = fs::read_to_string(wrapper).unwrap();

        assert!(html.contains("Content-Security-Policy"));
        assert!(html.contains("&lt;script&gt;alert(&#39;x&#39;)&lt;/script&gt;"));
        assert!(!html.contains("<script>alert('x')</script>"));
    }

    #[test]
    fn oversized_catalog_is_rejected() {
        let temp = tempdir().unwrap();
        let source = temp.path().join("huge.json");
        fs::write(&source, "x".repeat(MAX_CATALOG_BYTES as usize + 1)).unwrap();
        let error = install(&source, temp.path()).unwrap_err();
        assert!(error.to_string().contains("1 MiB"));
    }
}
