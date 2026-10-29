//! Rule ownership and the finite packaged delivery boundary.

use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::Path,
};

const SHARED_RULES: [&str; 3] = [
    "perf.no-collect-iter-roundtrip",
    "rust.no-debug-prints",
    "rust.no-inherent-to-string",
];

fn rule_sources(directory: &Path) -> BTreeMap<String, String> {
    let mut rules = BTreeMap::new();
    for entry in fs::read_dir(directory).expect("read rule directory") {
        let path = entry.expect("read rule entry").path();
        if !matches!(
            path.extension().and_then(|extension| extension.to_str()),
            Some("yml" | "yaml")
        ) {
            continue;
        }
        let text = fs::read_to_string(&path).expect("read rule source");
        let ids: Vec<_> = text
            .lines()
            .filter_map(|line| line.strip_prefix("id:"))
            .map(str::trim)
            .collect();
        assert!(!ids.is_empty(), "{} declares no rule ID", path.display());
        for id in ids {
            assert!(
                rules
                    .insert(id.to_string(), path.display().to_string())
                    .is_none(),
                "duplicate maintained rule: {id}"
            );
        }
    }
    rules
}

#[test]
fn every_rule_has_one_ownership_and_delivery_classification() {
    let package = Path::new(env!("CARGO_MANIFEST_DIR"));
    let local = rule_sources(&package.join("../../.config/ast-grep"));
    let shared = rule_sources(&package.join("rules"));
    let inventory =
        fs::read_to_string(package.join("rules/README.md")).expect("read rule ownership inventory");
    let mut classified = BTreeMap::new();
    for row in inventory.lines() {
        let columns: Vec<_> = row.split('|').map(str::trim).collect();
        let Some(id) = columns
            .get(1)
            .and_then(|id| id.strip_prefix('`'))
            .and_then(|id| id.strip_suffix('`'))
        else {
            continue;
        };
        assert_eq!(columns.len(), 5, "invalid inventory row: {row}");
        assert!(
            matches!(
                columns[2],
                "generic candidate" | "optional primitive contract" | "Kithara project policy"
            ),
            "unknown ownership for {id}: {}",
            columns[2]
        );
        assert!(
            matches!(columns[3], "local" | "shared"),
            "unknown delivery for {id}: {}",
            columns[3]
        );
        assert!(
            classified.insert(id, columns[3]).is_none(),
            "rule has multiple classifications: {id}"
        );
    }
    assert!(
        local.keys().all(|id| !shared.contains_key(id)),
        "a shared rule retains a local maintained copy"
    );
    let sources: BTreeMap<_, _> = local
        .keys()
        .map(|id| (id.as_str(), "local"))
        .chain(shared.keys().map(|id| (id.as_str(), "shared")))
        .collect();
    assert_eq!(classified, sources);
}

#[test]
fn the_package_ships_only_the_three_selected_rule_sources() {
    let package = Path::new(env!("CARGO_MANIFEST_DIR"));
    let rules = rule_sources(&package.join("rules"));
    let expected: BTreeSet<_> = SHARED_RULES.into_iter().collect();
    assert_eq!(
        rules.keys().map(String::as_str).collect::<BTreeSet<_>>(),
        expected
    );
    let sources: BTreeSet<_> = rules.values().collect();
    assert_eq!(sources.len(), SHARED_RULES.len());
}
