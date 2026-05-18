# Publishing and distribution

A community plugin is a single artifact: the entry point (`index.js`
for iframe-JS, `plugin.wasm` for WASM) plus its `plugin.json`,
typically zipped together. Distribution today is BYO; a built-in
marketplace (work item WI-44) is on the roadmap.

## Build a release artifact

```bash
pnpm build              # produces dist/
```

The release layout:

```
hello-1.0.0.zip
├── plugin.json
├── index.js            # or plugin.wasm
├── README.md           # optional but recommended
├── icon.svg            # optional, shown in the plugins panel
└── LICENSE
```

A bare `dist/` directory works too — `nexus plugin install ./dist`
accepts either form.

## Versioning

The `version` field in `plugin.json` is informational, not enforced.
Use semver and bump it on every release; the loader uses the version
to decide whether to re-run capability prompts (only **new**
capabilities prompt; previously-granted ones carry over).

If a release adds a new capability, expect to lose users who
auto-update — they'll see the prompt again. Consider keeping risky
new features behind a setting toggle so the grant doesn't change.

## Distribution channels

Until a marketplace ships, pick what fits your audience:

- **GitHub releases** — recommended. Attach the zip to a release;
  users `nexus plugin install https://github.com/you/foo/releases/download/v1.0.0/foo-1.0.0.zip`.
- **Your own server** — same flow with a different URL. Use HTTPS
  with a current cert; the loader rejects bad certs.
- **Direct file** — for closed-source / internal plugins:
  `nexus plugin install ./hello-1.0.0.zip`.

## Signing

Manifests can be signed with Ed25519. A signed plugin shows a
verified badge in the plugins panel; an unsigned one shows
"unsigned". Signing is optional today; it's expected to become
required for marketplace listings.

To sign:

```bash
nexus plugin sign --key ~/keys/publishing-key.pem ./hello/
# writes plugin.json.sig alongside plugin.json
```

The user trusts your key by adding the public key to
`<forge>/.forge/trusted-publishers.json`. Document the key fingerprint
in your README so users can verify before trusting.

ADR / details: `shell/src-tauri/src/lib.rs` — `verify_plugin_signature`
and `TRUSTED_PUBLIC_KEYS`.

## A good release checklist

- [ ] `version` in `plugin.json` bumped (semver).
- [ ] CHANGELOG entry written.
- [ ] `capabilities` list reviewed — minimal set, every entry
  justified in description.
- [ ] README documents:
  - What the plugin does.
  - Required capabilities and *why*.
  - Settings and how to configure them.
  - Compatible Nexus version range.
  - Source code link and license.
- [ ] Built and smoke-tested in a fresh forge.
- [ ] Tagged in git, GitHub release created.
- [ ] (Optional) signed with your publishing key.

## Discoverability

Until the marketplace ships:

- Tag your repo `nexus-plugin` on GitHub for keyword search.
- Add yourself to the community plugin index (a curated list, link
  TBD).
- Post the release somewhere users can find it (the repo discussions
  area is a good first stop).

## Update strategy

There is no auto-update mechanism today (work item WI-41). Users
update by re-running `nexus plugin install <url>`. If your plugin
needs a migration on upgrade:

```ts
const SCHEMA = 2;

activate(ctx: PluginContext) {
  const stored = Number(ctx.kv.get('__schema') ?? 1);
  if (stored < SCHEMA) {
    migrate(ctx, stored, SCHEMA);
    ctx.kv.set('__schema', SCHEMA);
  }
}
```

Keep migrations idempotent — users may update by uninstalling and
reinstalling, which preserves KV.

## Yanking

If a release is broken and you need to pull it: post a notice in the
repo, leave the artifact in place (silent removal breaks pinned
installs), and ship a `1.0.1` immediately. Users who reinstall pick
up the fix.

## Compatibility

Pin a Nexus version range in your README. The `apiVersion` field in
the manifest declares the *major* API version you target; the loader
rejects mismatches at install time.

```json
"apiVersion": 1
```

Bumping `apiVersion` means the extension API changed in a backwards-
incompatible way. Announcements happen in the release notes for the
Nexus version that bumped it.

## When the marketplace ships

The plan (WI-44):

- One-click install from the **Plugins** panel.
- Capability grants on install (same flow as today's CLI install).
- `nexus plugin publish` command pushes to the registry.
- Signed releases required.

This page will be updated when it lands.
