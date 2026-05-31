//! BL-138 — TOML-driven per-handler capability matrix.
//!
//! The companion file `crates/nexus-bootstrap/cap_matrix.toml` is the
//! single source of truth for which capability(ies) each in-tree IPC
//! handler requires of its caller. Bootstrap embeds the file at
//! compile time via [`include_str!`], parses it once, validates every
//! cap string and policy name, and applies the resulting entries to
//! the [`SharedPluginLoader`] via `register_handler_caps` /
//! `register_handler_unrestricted`.
//!
//! Validation is fail-fast: an unknown capability name, an unknown
//! policy name, or a duplicate `(plugin, command)` row aborts the
//! bootstrap with a clear error. Production must never start a
//! runtime with a broken matrix file.
//!
//! Version-alias handling: a handler `cmd` registered alongside its
//! `cmd.v1` alias (the ADR 0021 deprecation-window shape) only needs
//! one row in the matrix. The applier discovers the alias by walking
//! [`SharedPluginLoader::list_ipc_commands_snapshot`] (via the
//! `loader.lock()` path) and auto-mirrors the classification onto
//! every `<cmd>.v<N>` form that resolves to the same target plugin.
//!
//! Doc cross-link: `docs/adr/0002-hierarchical-capability-strings.md`.

use std::collections::{BTreeMap, BTreeSet};

use anyhow::{anyhow, bail, Context, Result};
use nexus_kernel::Capability;
use nexus_plugins::SharedPluginLoader;
use serde::Deserialize;

use crate::cap_policies;

/// The embedded matrix TOML text. Baked at compile time so the
/// resulting binary is self-contained.
const MATRIX_TOML: &str = include_str!("../cap_matrix.toml");

#[derive(Debug, Deserialize)]
struct MatrixFile {
    #[serde(default, rename = "handler")]
    handlers: Vec<RawHandler>,
}

#[derive(Debug, Deserialize)]
struct RawHandler {
    plugin: String,
    command: String,
    #[serde(default)]
    caps: Option<Vec<String>>,
    #[serde(default)]
    unrestricted: Option<String>,
    #[serde(default)]
    policy: Option<String>,
    /// P1-02 — when `true`, the kernel rejects calls from contexts
    /// whose `caller_trust_level != Core` no matter what caps the
    /// caller holds. Stacks on top of `caps` or `unrestricted` (so
    /// `internal = true` alongside `caps = [...]` means
    /// "core-trust caller AND the listed caps").
    #[serde(default)]
    internal: Option<bool>,
    // `note` is human audit-trail text; the loader does not consume it
    // but accepting the field lets the matrix carry rationale without
    // tripping serde's deny-unknown-fields.
    #[serde(default)]
    #[allow(dead_code)]
    note: Option<String>,
}

/// Parse, validate, and apply the embedded cap matrix to `shared`.
///
/// On success every row in the matrix has been mirrored into the
/// loader's cap table (for `caps`-bearing rows), its classification
/// map, and — for rows that opt into a named policy — the args-aware
/// closure registry.
///
/// # Errors
/// - Malformed TOML
/// - A `[[handler]]` row with neither `caps` nor `unrestricted`,
///   or with both
/// - An unknown capability string
/// - An unknown policy name
/// - A duplicate `(plugin, command)` row
// The body is intentionally split into validation + apply passes so
// every authoring error surfaces before any side effect lands on the
// loader. Collapsing it would interleave validation with mutation.
#[allow(clippy::too_many_lines)]
pub fn apply(shared: &SharedPluginLoader) -> Result<()> {
    let parsed: MatrixFile =
        toml::from_str(MATRIX_TOML).context("failed to parse cap_matrix.toml")?;

    // Validation pass — surface every problem before any side effect
    // hits the loader. This keeps the failure mode "matrix invalid →
    // boot aborts, loader untouched" rather than "half the matrix
    // applied, then panic."
    let mut seen: BTreeSet<(String, String)> = BTreeSet::new();
    let mut prepared: Vec<PreparedRow> = Vec::with_capacity(parsed.handlers.len());

    for row in parsed.handlers {
        let key = (row.plugin.clone(), row.command.clone());
        if !seen.insert(key.clone()) {
            bail!(
                "cap_matrix.toml — duplicate handler row for `{}::{}`",
                row.plugin,
                row.command,
            );
        }

        let classification = match (row.caps.as_ref(), row.unrestricted.as_ref()) {
            (Some(_), Some(_)) => bail!(
                "cap_matrix.toml — `{}::{}` declares both `caps` and `unrestricted` (pick one)",
                row.plugin,
                row.command,
            ),
            (None, None) => bail!(
                "cap_matrix.toml — `{}::{}` declares neither `caps` nor `unrestricted` (pick one)",
                row.plugin,
                row.command,
            ),
            (Some(caps), None) => {
                let parsed_caps = caps
                    .iter()
                    .map(|s| {
                        Capability::from_str(s).map_err(|e| {
                            anyhow!(
                                "cap_matrix.toml — `{}::{}` references unknown capability `{}`: {}",
                                row.plugin,
                                row.command,
                                s,
                                e,
                            )
                        })
                    })
                    .collect::<Result<Vec<_>>>()?;
                PreparedClassification::Required(parsed_caps)
            }
            (None, Some(reason)) => {
                if row.policy.is_some() {
                    bail!(
                        "cap_matrix.toml — `{}::{}` declares `policy` on an `unrestricted` row \
                         (policies stack on top of `caps`, not `unrestricted`)",
                        row.plugin,
                        row.command,
                    );
                }
                PreparedClassification::Unrestricted(reason.clone())
            }
        };

        if let Some(policy_name) = row.policy.as_ref() {
            if !cap_policies::is_registered(policy_name) {
                bail!(
                    "cap_matrix.toml — `{}::{}` references unknown policy `{}` \
                     (register it in crates/nexus-bootstrap/src/cap_policies.rs)",
                    row.plugin,
                    row.command,
                    policy_name,
                );
            }
        }

        prepared.push(PreparedRow {
            plugin: row.plugin,
            command: row.command,
            classification,
            policy: row.policy,
            internal_only: row.internal.unwrap_or(false),
        });
    }

    // Build a `plugin -> {command, command.v1, …}` map of the live
    // registry so we can mirror each matrix row onto every version
    // alias that resolves to the same handler.
    let registered = shared.lock().list_ipc_commands();
    let mut by_plugin: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for (plugin, command) in registered {
        by_plugin.entry(plugin).or_default().insert(command);
    }

    // Apply pass — no further validation, every error is now an
    // unrecoverable bootstrap failure rather than a matrix-author
    // mistake we should have caught above.
    for row in prepared {
        let aliases = alias_set(&by_plugin, &row.plugin, &row.command);
        match &row.classification {
            PreparedClassification::Required(caps) => {
                for cmd in &aliases {
                    shared.register_handler_caps(row.plugin.clone(), cmd.clone(), caps.clone());
                    if let Some(policy_name) = row.policy.as_ref() {
                        let f = cap_policies::resolve(policy_name).ok_or_else(|| {
                            anyhow!(
                                "cap_policies::resolve disagrees with cap_policies::is_registered \
                                 for policy `{policy_name}`"
                            )
                        })?;
                        shared.add_cap_requirement_fn(row.plugin.clone(), cmd.clone(), f);
                    }
                }
            }
            PreparedClassification::Unrestricted(reason) => {
                for cmd in &aliases {
                    shared.register_handler_unrestricted(
                        row.plugin.clone(),
                        cmd.clone(),
                        reason.clone(),
                    );
                }
            }
        }
        if row.internal_only {
            for cmd in &aliases {
                shared.register_handler_internal_only(row.plugin.clone(), cmd.clone());
            }
        }
    }

    Ok(())
}

/// All command names that share a handler with `command` on `plugin`.
/// At minimum this is `{command}` itself; for an ADR 0021 versioned
/// handler the set will also include `{command}.v1`, etc.
///
/// If the live registry has no entry for the plugin at all (e.g. the
/// plugin failed to register — `or_lifecycle_skip` swallowed the
/// error), we still classify the bare `command` so a mismatched
/// matrix row surfaces during apply rather than as a silent omission
/// later. The completeness test runs against the live registry, so a
/// missing plugin caps out at "no IPC commands to assert against."
fn alias_set(
    by_plugin: &BTreeMap<String, BTreeSet<String>>,
    plugin: &str,
    command: &str,
) -> Vec<String> {
    let Some(commands) = by_plugin.get(plugin) else {
        return vec![command.to_string()];
    };

    let mut out = Vec::new();
    if commands.contains(command) {
        out.push(command.to_string());
    }
    let version_prefix = format!("{command}.v");
    for c in commands {
        if c.starts_with(&version_prefix)
            && c[version_prefix.len()..]
                .chars()
                .all(|ch| ch.is_ascii_digit())
        {
            out.push(c.clone());
        }
    }
    if out.is_empty() {
        // Matrix names a handler the live registry does not. Apply to
        // the bare name anyway — keeps the failure mode "unused
        // matrix row" rather than "silent skip"; surfaces on the next
        // run of the (eventual) "matrix references a handler that
        // does not exist" test.
        out.push(command.to_string());
    }
    out
}

struct PreparedRow {
    plugin: String,
    command: String,
    classification: PreparedClassification,
    policy: Option<String>,
    internal_only: bool,
}

enum PreparedClassification {
    Required(Vec<Capability>),
    Unrestricted(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_matrix_parses_and_validates() {
        // We can't run `apply` without a SharedPluginLoader; the
        // parse + validate happen in the same call, so a successful
        // `toml::from_str` plus per-row validation covers most of the
        // failure modes. The end-to-end apply is exercised in the
        // bootstrap integration test.
        let parsed: MatrixFile =
            toml::from_str(MATRIX_TOML).expect("embedded cap_matrix.toml must parse");
        assert!(
            !parsed.handlers.is_empty(),
            "cap_matrix.toml has zero rows — at minimum the 17 historical entries should ship"
        );

        let mut seen: BTreeSet<(String, String)> = BTreeSet::new();
        for row in parsed.handlers {
            let key = (row.plugin.clone(), row.command.clone());
            assert!(seen.insert(key.clone()), "duplicate row for {key:?}");
            assert!(
                row.caps.is_some() ^ row.unrestricted.is_some(),
                "{key:?}: must declare exactly one of `caps` / `unrestricted`",
            );
            if let Some(caps) = row.caps.as_ref() {
                for c in caps {
                    Capability::from_str(c)
                        .unwrap_or_else(|e| panic!("{key:?}: cap `{c}` is unknown: {e}"));
                }
            }
            if let Some(p) = row.policy.as_ref() {
                assert!(
                    cap_policies::is_registered(p),
                    "{key:?}: policy `{p}` is not registered in cap_policies",
                );
            }
        }
    }
}
