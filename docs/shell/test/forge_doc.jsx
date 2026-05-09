/* Nexus Forge — the document body (Implementation Status demo content). */
const { useState: useStateD, useEffect: useEffectD, useRef: useRefD } = React;

function Task({ label, tag, initial=false }) {
  const [done, setDone] = useStateD(initial);
  return (
    <li className={'task ' + (done ? 'done' : '')} onClick={() => setDone(d => !d)}>
      <span className="check"><Ic.check style={{ width: 11, height: 11 }} /></span>
      <span className="label">{label}</span>
      {tag && <span className="tag">{tag}</span>}
    </li>
  );
}

function Tier({ emoji, label }) {
  return (
    <span style={{
      display: 'inline-flex', alignItems: 'center', gap: 6,
      padding: '1px 8px', borderRadius: 999,
      background: 'var(--bg-raised)', border: '1px solid var(--line-soft)',
      fontFamily: 'var(--f-ui)', fontSize: 11,
      color: 'var(--fg-muted)'
    }}>
      <span style={{ fontSize: 12 }}>{emoji}</span>{label}
    </span>
  );
}

function W({ children }) { return <span className="wikilink">{children}</span>; }

function Doc({ onHeading }) {
  // heading scroll-spy
  const root = useRefD(null);
  useEffectD(() => {
    const el = root.current; if (!el) return;
    const hs = el.querySelectorAll('[data-h]');
    const onScroll = () => {
      const top = el.scrollTop + 60;
      let cur = null;
      hs.forEach(h => { if (h.offsetTop - el.offsetTop <= top) cur = h.dataset.h; });
      if (cur) onHeading(cur);
    };
    el.addEventListener('scroll', onScroll);
    onScroll();
    return () => el.removeEventListener('scroll', onScroll);
  }, []);

  // Expose jump handler for outline clicks
  useEffectD(() => {
    window.__jumpHeading = (id) => {
      const el = root.current?.querySelector(`[data-h="${id}"]`);
      if (el) el.scrollIntoView({ behavior: 'smooth', block: 'start' });
    };
  }, []);

  return (
    <div className="surface" ref={root}>
      <div className="doc">
        <h1 className="title">Nexus PRD Implementation Status</h1>
        <div className="metaline">
          <span className="chip">Forge · Nexus_Work</span>
          <span className="chip">Updated 2026-04-17</span>
          <span className="tier"><Ic.clock style={{ width: 12, height: 12 }} /> Rolling tracking doc</span>
        </div>

        <p>
          Snapshot of where PRDs <code>01–17</code> stand, audited against the <code>crates/**</code> tree and
          <code> app/src/**</code>. This doc is the <em>state-of-the-build</em>, not a second copy of the spec —
          link to the source PRD for acceptance detail. Scope notes live in <W>[[BACKLOG.md]]</W> and
          <W>[[BACKLOG_COMPLETED.md]]</W>.
        </p>

        <div className="callout">
          <div className="i"><Ic.ember style={{ width: 14, height: 14 }} /></div>
          <div>
            <div className="h">Update cadence</div>
            <div className="b">Refresh whenever a PRD's status tier changes, or at minimum at every minor release. When a <code>BACKLOG.md</code> item moves to <code>BACKLOG_COMPLETED.md</code>, check whether its PRD's status should bump.</div>
          </div>
        </div>

        <h2 data-h="h-legend">Legend</h2>
        <table>
          <thead>
            <tr><th style={{ width: 90 }}>Tier</th><th>Meaning</th></tr>
          </thead>
          <tbody>
            <tr><td><Tier emoji="✅" label="Complete" /></td><td>Acceptance criteria met; no material gaps. Maintenance-only.</td></tr>
            <tr><td><Tier emoji="🟢" label="Substantial" /></td><td>Core shipped; remaining gaps tracked in <W>[[BACKLOG.md]]</W>.</td></tr>
            <tr><td><Tier emoji="🟡" label="Partial" /></td><td>Meaningful work shipped but major sections missing or unwired.</td></tr>
            <tr><td><Tier emoji="🟠" label="Scaffolded" /></td><td>Types/skeleton exist; little operational code.</td></tr>
            <tr><td><Tier emoji="🔴" label="Not started" /></td><td>No meaningful code in tree.</td></tr>
            <tr><td><Tier emoji="⚪" label="Deferred" /></td><td>Spec written; implementation phased out of current scope.</td></tr>
          </tbody>
        </table>

        <h2 data-h="h-summary">Summary</h2>
        <table>
          <thead>
            <tr>
              <th style={{ width: 54 }}>PRD</th>
              <th>Title</th>
              <th style={{ width: 96 }}>Status</th>
              <th>State</th>
            </tr>
          </thead>
          <tbody>
            {[
              ['01','Kernel & Event System','✅','Event bus, lifecycle, capability system all live'],
              ['02','Security Model','✅','WASM sandbox, capability gating, audit logging, consent shipped'],
              ['03','Storage Engine','✅','Forge layout + SQLite + Tantivy + graph + watcher + CRDT hooks'],
              ['04','Plugin System','✅','Manifest, WASM, hot-reload, activation events, core/community tiers'],
              ['05','CLI','🟢','12 subcommand groups live; agent/workflow blocked on their subsystems'],
              ['06','File Formats','✅','Markdown/MDX/Canvas/Bases/forge config all parse + serialize'],
              ['07','Theming & UI','✅','497-token CSS registry, theme core plugin, contribution registry'],
              ['08','Editor Engine','🟡','3.7k LoC core; §4 BlockPositionMap superseded by CM6-owns-text'],
              ['09','Terminal & Process Mgr','🟢','Phases A–V shipped; 239 tests; 10 dispatch handlers'],
              ['10','Database Engine','🟡','Bases parse + view engine; Board/List/Cal/Gallery renderers WIP'],
              ['11','Git Integration','🟢','1.1k-line GitEngine over git2; worker-thread wrapper pending'],
              ['12','AI Engine','🟡','Anthropic/OpenAI/Ollama providers + chunker; no chat UI yet'],
              ['13','Skills','⚪','Spec only; no parser, registry, or CLI'],
              ['14','MCP Integration','🟡','807-line serve_stdio; no WS/HTTP transports, no Host (client)'],
              ['15','Agent System','⚪','Spec only; no Agent trait or planner'],
              ['16','Workflow System','⚪','Spec only; no .workflow.toml parser or triggers'],
              ['17','Cross-Platform','🟢','Tauri desktop shipping; web OPFS + mobile UniFFI deferred'],
            ].map(([id, t, tier, state]) => (
              <tr key={id}>
                <td><code>{id}</code></td>
                <td>{t}</td>
                <td><span style={{ fontSize: 14 }}>{tier}</span></td>
                <td style={{ color: 'var(--fg-muted)' }}>{state}</td>
              </tr>
            ))}
          </tbody>
        </table>

        <h2 data-h="h-per">Per-PRD detail</h2>

        <h3 data-h="h-p01">PRD-01 · Kernel & Event System</h3>
        <p>
          Central <code>Kernel</code> in <W>[[crates/nexus-kernel]]</W> owns the event bus, plugin lifecycle
          and the capability system. All downstream crates call it through a narrow trait surface, so the
          microkernel boundary is preserved. No open acceptance items; finding-id
          <code> KA-2025-11</code> in <W>[[MICROKERNEL-AUDIT.md]]</W> was resolved in commit <code>3f1c8d2</code>.
        </p>

        <h3 data-h="h-p02">PRD-02 · Security Model</h3>
        <p>
          Credential vault lands in the OS keyring via <code>nexus-security::vault</code>; install-time consent
          is gated through the plugin manifest. Audit log is append-only JSONL at <code>.forge/audit/</code>.
        </p>

        <h3 data-h="h-p03">PRD-03 · Storage Engine</h3>
        <p>
          Files on disk are the source of truth; SQLite is rebuildable. Tantivy FTS runs in-process with
          incremental updates driven by the file watcher. See <W>[[Tantivy vs Meilisearch]]</W> for the
          original tradeoff write-up.
        </p>

        <h3 data-h="h-p08">PRD-08 · Editor Engine</h3>
        <p>
          CodeMirror 6 owns the text model; we provide a thin adapter that surfaces decorations, wikilink
          resolution and slash commands. The original <code>BlockPositionMap</code> design has been retired
          in favour of CM6-owns-text — see <W>[[UI-AUDIT.md]]</W> §4.
        </p>

        <h3 data-h="h-p09">PRD-09 · Terminal & Process Manager</h3>
        <p>
          Both the TUI pane and the Tauri React panel render live PTY output through kernel IPC. Remaining
          work: ANSI colour rendering via xterm.js, saved-commands sidebar, and FTS5 scrollback index.
        </p>

        <h3 data-h="h-p10">PRD-10 · Database Engine</h3>
        <p>
          <code>.bases</code> files parse, validate, and run filter/sort/group through the applied-view engine.
          Renderers are still WIP — Board and List land this milestone; Calendar and Gallery move to
          Phase-2 as <W>[[PRD-10b]]</W>.
        </p>

        <h3 data-h="h-p12">PRD-12 · AI Engine</h3>
        <p>
          Provider traits are in place for Anthropic, OpenAI, Ollama and llama.cpp. Embeddings + chunker ship;
          there is no chat UI, streaming transport or agent loop yet. Work unblocks after PRD-15 lands.
        </p>

        <h2 data-h="h-cross">Cross-cutting observations</h2>
        <ul>
          <li>Plugin system <Tier emoji="✅" label="Complete" /> but <em>community</em> marketplace remains first-party-only until <code>F-8.1.1</code> + <code>F-2.2.1</code> ship.</li>
          <li>Agent, skill, and workflow PRDs are spec-only — they cluster and should land together to avoid API churn.</li>
          <li>Theming engine is feature-complete; UI polish items live in <W>[[UI-AUDIT.md]]</W>.</li>
        </ul>

        <h2 data-h="h-risk">Risk hotspots</h2>
        <table>
          <thead>
            <tr><th>Risk</th><th>Why it matters</th><th>Mitigation</th></tr>
          </thead>
          <tbody>
            <tr>
              <td>PRD-08 §4 doc drift</td>
              <td>Plugin authors may be misled by stale PRD text</td>
              <td>Amend §4 before <code>1.0 GA</code></td>
            </tr>
            <tr>
              <td>MCP Host absence</td>
              <td>Positioned as "MCP-integrated" but cannot consume external MCP servers</td>
              <td>Add a minimal MCP client before any marketing claim</td>
            </tr>
            <tr>
              <td><code>Git !Send</code> constraint</td>
              <td>UI-driven git ops block the main thread</td>
              <td>Wrap <code>GitEngine</code> in a worker thread; specify the pattern once, reuse it</td>
            </tr>
            <tr>
              <td>F-8.1.1 iframe sandbox deferred</td>
              <td>Cannot ship the community JS plugin marketplace safely</td>
              <td>Policy recorded: script plugins first-party-only until F-8.1.1 + F-2.2.1 land</td>
            </tr>
            <tr>
              <td><code>.bases</code> views absent</td>
              <td>Files load but render nothing useful</td>
              <td>Scope views into Phase-2 PRD-10b rather than shipping 10 half-done</td>
            </tr>
          </tbody>
        </table>

        <h2 data-h="h-honest">How to keep this doc honest</h2>
        <ul className="tasks">
          <Task initial={true}  label={<>When a <code>BACKLOG.md</code> item moves to <code>BACKLOG_COMPLETED.md</code>, check the PRD's tier and bump if warranted.</>} tag="maintenance" />
          <Task initial={false} label={<>When a PRD's gaps list shrinks to zero, mark ✅ and note the commit that closed the last gap.</>} tag="bump" />
          <Task initial={false} label={<>When a new audit (<code>docs/UI-AUDIT.md</code>, <code>docs/MICROKERNEL-AUDIT.md</code>) surfaces a finding, add it to the affected PRD's Gaps line with the finding-id.</>} tag="audit" />
          <Task initial={false} label={<>Avoid re-describing the PRD here — link to it. This doc is the state-of-the-build, not a second copy of the spec.</>} tag="style" />
        </ul>

        <blockquote>
          Implementation status is a living artefact. When it goes stale, readers stop trusting it — and
          they stop bumping tiers, which is the whole point.
        </blockquote>
      </div>
    </div>
  );
}

Object.assign(window, { Doc });
