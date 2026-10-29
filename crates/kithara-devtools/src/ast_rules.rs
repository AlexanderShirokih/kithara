//! Resolve one catalog of packaged and workspace-local ast-grep rules.

use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
};

use anyhow::{Context as _, Result, ensure};
use glob::Pattern;
use serde::Deserialize;
use serde_yaml_ng::{Deserializer, Mapping, Value};
use tempfile::{TempDir, tempdir};

use crate::{
    Ctx,
    common::{
        exclude::{cfg_test_module_globs, cfg_test_module_globs_for_targets},
        scope::Scope,
        walker::{matches_any, workspace_rs_files_scoped},
    },
};

const SHARED_RULES: [(&str, &str); 3] = [
    (
        "rust.no-debug-prints",
        include_str!("../rules/rust.no-debug-prints.yml"),
    ),
    (
        "rust.no-inherent-to-string",
        include_str!("../rules/rust.no-inherent-to-string.yml"),
    ),
    (
        "perf.no-collect-iter-roundtrip",
        include_str!("../rules/perf.no-collect-iter-roundtrip.yml"),
    ),
];

pub(crate) struct Rules {
    catalog: BTreeMap<String, BTreeSet<String>>,
    directory: TempDir,
}

impl Rules {
    pub(crate) fn config(&self) -> PathBuf {
        self.directory.path().join("sgconfig.yml")
    }

    pub(crate) fn load(ctx: &Ctx) -> Result<Self> {
        let directory = tempdir().context("create ast-grep rule directory")?;
        let mut catalog = BTreeMap::new();
        let mut identities = BTreeSet::new();
        let mut config = Mapping::new();
        let local_config = ctx.root.join("sgconfig.yml");
        let mut dirs = Vec::new();
        if local_config.exists() {
            config = serde_yaml_ng::from_str(&fs::read_to_string(&local_config)?)
                .with_context(|| format!("parse {}", local_config.display()))?;
            for key in ["ruleDirs", "utilDirs"] {
                if let Some(value) = config.get_mut(Value::String(key.to_owned())) {
                    let entries = value
                        .as_sequence_mut()
                        .with_context(|| format!("ast-grep {key} must be a list"))?;
                    for entry in entries {
                        let relative = entry
                            .as_str()
                            .with_context(|| format!("ast-grep {key} must contain paths"))?;
                        let path =
                            ctx.root.join(relative).canonicalize().with_context(|| {
                                format!("resolve ast-grep {key} path {relative}")
                            })?;
                        *entry = Value::String(path.to_string_lossy().into_owned());
                        if key == "ruleDirs" {
                            dirs.push(path);
                        }
                    }
                }
            }
            for dir in &dirs {
                index_directory(dir, &mut catalog, &mut identities)?;
            }
        }
        let shared_dir = directory.path().join("shared");
        fs::create_dir(&shared_dir)?;
        let mut selected = BTreeSet::new();
        for id in &ctx.config.ast_grep.shared_rules {
            ensure!(selected.insert(id), "duplicate shared ast-grep rule `{id}`");
            let (_, source) = SHARED_RULES
                .iter()
                .find(|(known, _)| *known == id)
                .with_context(|| format!("unknown shared ast-grep rule `{id}`"))?;
            let path = shared_dir.join(format!("{id}.yml"));
            let mut rule: Value = serde_yaml_ng::from_str(source)?;
            if let Some(paths) = ctx.config.ast_grep.shared_rule_paths.get(id) {
                ensure!(
                    !paths.is_empty(),
                    "shared ast-grep scope `{id}` must not be empty"
                );
                for pattern in paths {
                    ensure!(
                        !pattern.is_empty(),
                        "shared ast-grep scope `{id}` has an empty pattern"
                    );
                    Pattern::new(pattern)
                        .with_context(|| format!("invalid shared ast-grep scope `{id}`"))?;
                }
                rule["files"] = serde_yaml_ng::to_value(paths)?;
            }
            fs::write(&path, serde_yaml_ng::to_string(&rule)?)?;
            index_file(&path, &mut catalog, &mut identities)?;
        }
        for id in ctx.config.ast_grep.shared_rule_paths.keys() {
            ensure!(
                selected.contains(id),
                "scope names unselected shared ast-grep rule `{id}`"
            );
        }
        ensure!(
            local_config.exists() || !selected.is_empty(),
            "ast-grep requires sgconfig.yml or selected packaged rules",
        );
        if !selected.is_empty() {
            dirs.push(shared_dir);
        }
        config.insert(
            Value::String("ruleDirs".to_owned()),
            serde_yaml_ng::to_value(dirs)?,
        );
        fs::write(
            directory.path().join("sgconfig.yml"),
            serde_yaml_ng::to_string(&config)?,
        )?;
        for id in &ctx.config.lint_exclude.scan_all_rules {
            ensure!(
                catalog.contains_key(id),
                "unknown scan-all ast-grep rule `{id}`"
            );
        }
        Ok(Self { catalog, directory })
    }

    pub(crate) fn rule(&self, id: &str) -> Result<&BTreeSet<String>> {
        self.catalog
            .get(id)
            .with_context(|| format!("unknown ast-grep rule `{id}`"))
    }
}

fn index_directory(
    directory: &Path,
    catalog: &mut BTreeMap<String, BTreeSet<String>>,
    identities: &mut BTreeSet<String>,
) -> Result<()> {
    let mut entries = fs::read_dir(directory)?
        .map(|entry| entry.map(|entry| entry.path()))
        .collect::<std::io::Result<Vec<_>>>()?;
    entries.sort();
    for path in entries {
        let file_type = fs::symlink_metadata(&path)?.file_type();
        if file_type.is_symlink() {
            continue;
        }
        if file_type.is_dir() {
            index_directory(&path, catalog, identities)?;
        } else if path
            .extension()
            .is_some_and(|extension| extension == "yml" || extension == "yaml")
        {
            index_file(&path, catalog, identities)?;
        }
    }
    Ok(())
}

fn index_file(
    path: &Path,
    catalog: &mut BTreeMap<String, BTreeSet<String>>,
    identities: &mut BTreeSet<String>,
) -> Result<()> {
    let text = fs::read_to_string(path)?;
    for document in Deserializer::from_str(&text) {
        let rule = Value::deserialize(document)
            .with_context(|| format!("parse ast-grep rule {}", path.display()))?;
        let id = rule
            .get("id")
            .and_then(Value::as_str)
            .with_context(|| format!("ast-grep rule {} has no identity", path.display()))?;
        ensure!(
            identities.insert(id.to_owned()),
            "duplicate ast-grep rule `{id}`"
        );
        catalog
            .entry(id.to_owned())
            .or_default()
            .insert(id.to_owned());
        if let Some(category) = rule
            .get("metadata")
            .and_then(|metadata| metadata.get("category"))
            .and_then(Value::as_str)
            && category != id
        {
            catalog
                .entry(category.to_owned())
                .or_default()
                .insert(id.to_owned());
        }
    }
    Ok(())
}

pub(crate) fn source_paths(ctx: &Ctx, paths: &[String]) -> Result<Vec<PathBuf>> {
    if !paths.is_empty() {
        return Ok(paths.iter().map(PathBuf::from).collect());
    }
    if !ctx.config.ast_grep.default_roots.is_empty() {
        return Ok(ctx.config.ast_grep.default_roots.clone());
    }
    let Ok(metadata) = ctx.metadata() else {
        return Ok(vec![PathBuf::from(".")]);
    };
    let scope = Scope::default().with_metadata(metadata);
    Ok(workspace_rs_files_scoped(&ctx.root, &scope)?
        .into_iter()
        .map(|path| path.strip_prefix(&ctx.root).unwrap_or(&path).to_path_buf())
        .collect())
}

pub(crate) fn excluded_globs(ctx: &Ctx, include_tests: bool) -> Result<Vec<Pattern>> {
    let mut paths = ctx.config.workspace_scan.exclude.clone();
    if !include_tests {
        paths.extend(ctx.config.lint_exclude.runtime_paths());
        paths.extend(test_module_globs(ctx));
    }
    paths
        .iter()
        .map(|path| {
            Pattern::new(path).with_context(|| format!("invalid ast-grep exclusion `{path}`"))
        })
        .collect()
}

/// Cargo-backed contexts use declared targets; standalone contexts use their source graph.
pub(crate) fn test_module_globs(ctx: &Ctx) -> Vec<String> {
    ctx.metadata().map_or_else(
        |_| cfg_test_module_globs(&ctx.root),
        |metadata| {
            cfg_test_module_globs_for_targets(
                &ctx.root,
                metadata
                    .workspace_packages()
                    .into_iter()
                    .flat_map(|package| {
                        package
                            .targets
                            .iter()
                            .map(|target| target.src_path.as_std_path())
                    }),
            )
        },
    )
}

pub(crate) fn filter_file_paths(
    ctx: &Ctx,
    paths: &[PathBuf],
    include_tests: bool,
) -> Result<Vec<PathBuf>> {
    let excludes = excluded_globs(ctx, include_tests)?;
    Ok(paths
        .iter()
        .filter(|path| {
            let relative = path.strip_prefix(&ctx.root).unwrap_or(path);
            !ctx.root.join(path).is_file() || !matches_any(&excludes, relative)
        })
        .cloned()
        .collect())
}
