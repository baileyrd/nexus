/* Nexus Forge — Processes workspace (PRD-09 Terminal & Process Manager).
 * Switches in for the main editor area when activity rail = 'terminal'. */
const { useState: useStateP, useEffect: useEffectP, useRef: useRefP } = React;

const PROC_GROUPS = [
  { id: 'g_kernel', label: 'Kernel', items: [
    { id: 'p_kernel', name: 'nexus-kernel', kind: 'rust', status: 'run', proc: 1, mem: '48.2 MB', icon: 'k' },
    { id: 'p_storage', name: 'storage watcher', kind: 'sub', status: 'run', mem: '12.1 MB' },
    { id: 'p_tantivy', name: 'tantivy-indexer', kind: 'sub', status: 'run', mem: '82.4 MB' },
  ]},
  { id: 'g_app', label: 'App shell', items: [
    { id: 'p_tauri', name: 'nexus-app (tauri)', kind: 'rust', status: 'run', proc: 1, mem: '124.6 MB', icon: 't' },
    { id: 'p_vite', name: 'vite dev server', kind: 'node', status: 'run', mem: '78.0 MB' },
    { id: 'p_tsc', name: 'tsc --watch', kind: 'node', status: 'idle', mem: '41.3 MB' },
  ]},
  { id: 'g_mcp', label: 'MCP', items: [
    { id: 'p_mcp', name: 'serve_stdio', kind: 'rust', status: 'run', mem: '22.0 MB', icon: 'm' },
  ]},
  { id: 'g_plug', label: 'Plugins (hot)', items: [
    { id: 'p_hellojs', name: 'hello-js', kind: 'wasm', status: 'run', mem: '3.2 MB' },
    { id: 'p_hellonx', name: 'hello-nexus', kind: 'wasm', status: 'run', mem: '4.1 MB' },
    { id: 'p_theme',   name: 'com.nexus.theme', kind: 'wasm', status: 'run', mem: '2.8 MB' },
  ]},
  { id: 'g_build', label: 'Build', items: [
    { id: 'p_cargo', name: 'cargo check', kind: 'rust', status: 'stopped', mem: '0 MB' },
    { id: 'p_bench', name: 'bench_run.sh', kind: 'sh', status: 'stopped', mem: '0 MB' },
  ]},
];

const STATUS_COLOR = {
  run:     { fg: 'var(--ok)',   label: 'Running' },
  idle:    { fg: 'var(--cool)', label: 'Idle' },
  stopped: { fg: 'var(--fg-dim)', label: 'Stopped' },
  err:     { fg: 'var(--risk)', label: 'Crashed' },
};

// Demo log lines for the selected process (nexus-app / vite).
const LOG_LINES = [
  { t: 'sec', s: '⟳ Restored from previous session (stopped)' },
  { t: 'sec', s: '◐ Restarting… (restart #3)' },
  { t: 'sec', s: '▸ Running pre-command: cargo build -p nexus-app' },
  { t: 'sec', s: '   Directory: /Users/lap/code/nexus' },
  { t: 'sec', s: '⚑ Activated toolchain rust-stable 1.83.0' },
  { t: 'sec', s: '   Up to date.' },
  { t: 'ok',  s: '✓ Pre-command completed in 2.8s' },
  { t: 'sec', s: '▸ Starting: pnpm --filter @nexus/app dev' },
  { t: 'sec', s: '   Directory: /Users/lap/code/nexus/app' },
  { t: 'sec', s: '   Shell: zsh' },
  { t: 'sec', s: '' },
  { t: 'blank', s: ' ' },
  { t: 'log',  ts: '2026-04-17 14:25:13', lvl: 'INFO', mod: 'kernel::event_bus', msg: 'Event bus online · 12 subscribers' },
  { t: 'log',  ts: '2026-04-17 14:25:13', lvl: 'INFO', mod: 'storage::forge',     msg: 'Forge mounted: /Users/lap/notes (12,480 docs)' },
  { t: 'log',  ts: '2026-04-17 14:25:13', lvl: 'INFO', mod: 'storage::watcher',   msg: 'Watching 1,204 directories' },
  { t: 'log',  ts: '2026-04-17 14:25:13', lvl: 'INFO', mod: 'security::vault',    msg: 'Keyring unlocked for user lap@nexus (audit OK)' },
  { t: 'log',  ts: '2026-04-17 14:25:13', lvl: 'INFO', mod: 'plugins::loader',    msg: 'Loaded 3 wasm plugins (hello-js, hello-nexus, com.nexus.theme)' },
  { t: 'log',  ts: '2026-04-17 14:25:13', lvl: 'WARN', mod: 'ai::provider',       msg: 'No ANTHROPIC_API_KEY; falling back to ollama' },
  { t: 'log',  ts: '2026-04-17 14:25:14', lvl: 'INFO', mod: 'app::tauri',         msg: 'Tauri shell ready · WebView2 122.0.2365' },
  { t: 'sec',  s: '' },
  { t: 'sec',  s: '▸ vite dev server starting' },
  { t: 'log',  ts: '2026-04-17 14:25:14', lvl: 'INFO', mod: 'vite',               msg: 'VITE v5.4.8  ready in 612 ms' },
  { t: 'log',  ts: '2026-04-17 14:25:14', lvl: 'INFO', mod: 'vite',               msg: 'Local:   http://127.0.0.1:5173/' },
  { t: 'log',  ts: '2026-04-17 14:25:14', lvl: 'INFO', mod: 'vite',               msg: 'Network: use --host to expose' },
  { t: 'http', ts: '2026-04-17 14:25:17', method: 'GET', path: '/',               code: 200, size: '12.66 KB', ms: 94, ua: 'Nexus/0.1.0 (Tauri macOS 14.2)' },
  { t: 'http', ts: '2026-04-17 14:25:17', method: 'GET', path: '/src/main.tsx',   code: 200, size: '12.66 KB', ms: 6,  ua: 'Nexus/0.1.0' },
  { t: 'http', ts: '2026-04-17 14:25:55', method: 'GET', path: '/',               code: 200, size: '12.66 KB', ms: 6,  ua: 'Nexus/0.1.0' },
  { t: 'http', ts: '2026-04-17 14:25:56', method: 'GET', path: '/forge/graph.json', code: 200, size: '6.53 KB', ms: 80, ua: 'Nexus/0.1.0' },
  { t: 'log',  ts: '2026-04-17 14:26:02', lvl: 'INFO', mod: 'editor::liveMd',     msg: 'CM6 editor mounted, 48 extensions active' },
  { t: 'log',  ts: '2026-04-17 14:26:04', lvl: 'DEBUG', mod: 'plugins::runtime',  msg: 'hello-nexus subscribed to events: [note.created, note.updated]' },
  { t: 'http', ts: '2026-04-17 14:26:18', method: 'POST', path: '/ipc/editor/save', code: 200, size: '1.12 KB', ms: 11, ua: 'Nexus/0.1.0' },
  { t: 'log',  ts: '2026-04-17 14:26:18', lvl: 'INFO', mod: 'storage::writer',    msg: 'Wrote Nexus_Work/Implementation Status.md (14,139 chars)' },
  { t: 'log',  ts: '2026-04-17 14:26:19', lvl: 'INFO', mod: 'tantivy::index',     msg: 'Indexed 1 doc · commit in 18ms' },
];

function ProcSidebar({ selectedId, onSelect }) {
  return (
    <div className="proc-sidebar">
      <div className="panel-head" style={{ paddingLeft: 14 }}>
        <span>Processes · {countRun()} running</span>
        <div className="actions">
          <button className="icon-btn" title="New process"><Ic.plus /></button>
        </div>
      </div>

      <div className="proc-summary">
        <div className="ps-card">
          <div className="ps-k">procs</div>
          <div className="ps-v">{countRun()}<span className="ps-sub">/ {countAll()}</span></div>
        </div>
        <div className="ps-card">
          <div className="ps-k">memory</div>
          <div className="ps-v">418<span className="ps-sub">MB</span></div>
        </div>
        <div className="ps-card">
          <div className="ps-k">plugins</div>
          <div className="ps-v">3<span className="ps-sub">hot</span></div>
        </div>
      </div>

      <div className="proc-scroll">
        {PROC_GROUPS.map(g => (
          <div key={g.id} className="proc-group">
            <div className="pg-head">
              <span>{g.label}</span>
              <span className="kbd" style={{ fontSize: 9 }}>{g.items.length}</span>
            </div>
            {g.items.map(it => (
              <div
                key={it.id}
                className={'proc-row ' + (selectedId === it.id ? 'active' : '')}
                onClick={() => onSelect(it.id)}
              >
                <span className={'pr-dot ' + it.status} />
                <span className="pr-kind">{kindGlyph(it.kind)}</span>
                <span className="pr-name">{it.name}</span>
                <span className="pr-mem">{it.mem}</span>
              </div>
            ))}
          </div>
        ))}
      </div>

      <div className="proc-foot">
        <button className="pf-btn danger">
          <span className="sq" /> Stop All
        </button>
        <button className="pf-btn">
          <Ic.bolt style={{ width: 12, height: 12 }} />
          Run task…
        </button>
      </div>
    </div>
  );
}

function countRun() {
  return PROC_GROUPS.flatMap(g => g.items).filter(p => p.status === 'run').length;
}
function countAll() {
  return PROC_GROUPS.flatMap(g => g.items).length;
}
function kindGlyph(k) {
  return {
    rust: '🦀', node: '◆', wasm: '◈', sub: '↳', sh: '▸'
  }[k] || '•';
}

function StatusPill({ status }) {
  const s = STATUS_COLOR[status] || STATUS_COLOR.stopped;
  return (
    <span className="status-pill" style={{ color: s.fg, borderColor: s.fg }}>
      <span className="sp-dot" style={{ background: s.fg, boxShadow: status==='run' ? `0 0 8px ${s.fg}` : 'none' }} />
      {s.label}
    </span>
  );
}

function LogLine({ l }) {
  if (l.t === 'sec') {
    return <div className="ll-sec">{l.s}</div>;
  }
  if (l.t === 'ok') {
    return <div className="ll-ok">{l.s}</div>;
  }
  if (l.t === 'blank') return <div className="ll-blank">&nbsp;</div>;
  if (l.t === 'http') {
    const codeCls = l.code >= 500 ? 'c5' : l.code >= 400 ? 'c4' : l.code >= 300 ? 'c3' : 'c2';
    return (
      <div className="ll-http">
        <span className="ll-ms">{l.ms}ms</span>
        <span className={'ll-method m-' + l.method.toLowerCase()}>{l.method}</span>
        <span className={'ll-code ' + codeCls}>{l.code}</span>
        <span className="ll-path">{l.path}</span>
        <span className="ll-size">{l.size}</span>
        <span className="ll-ts">{l.ts.slice(11)}</span>
        <span className="ll-ua">{l.ua}</span>
      </div>
    );
  }
  // structured log
  const lvlCls = { INFO: 'lvl-info', WARN: 'lvl-warn', ERROR: 'lvl-err', DEBUG: 'lvl-dbg' }[l.lvl] || '';
  return (
    <div className="ll-log">
      <span className="ll-ts">{l.ts}</span>
      <span className="ll-bar">|</span>
      <span className={'ll-lvl ' + lvlCls}>{l.lvl.padEnd(5)}</span>
      <span className="ll-bar">|</span>
      <span className="ll-mod">{l.mod}</span>
      <span className="ll-col">-</span>
      <span className="ll-msg">{l.msg}</span>
    </div>
  );
}

function ProcDetail({ id }) {
  const proc = findProc(id) || PROC_GROUPS[1].items[0];
  const [running, setRunning] = useStateP(proc.status === 'run');
  const [follow, setFollow] = useStateP(true);
  const [filter, setFilter] = useStateP('all');
  const [query, setQuery] = useStateP('');
  const logRef = useRefP(null);

  useEffectP(() => { setRunning(proc.status === 'run'); }, [id]);

  // auto-scroll on new lines if follow is on
  useEffectP(() => {
    if (follow && logRef.current) logRef.current.scrollTop = logRef.current.scrollHeight;
  }, [follow, id]);

  const lines = LOG_LINES.filter(l => {
    if (filter === 'http' && l.t !== 'http') return false;
    if (filter === 'logs' && l.t !== 'log') return false;
    if (filter === 'warn' && !(l.t === 'log' && (l.lvl === 'WARN' || l.lvl === 'ERROR'))) return false;
    if (query) {
      const hay = JSON.stringify(l).toLowerCase();
      if (!hay.includes(query.toLowerCase())) return false;
    }
    return true;
  });

  return (
    <div className="proc-detail">
      {/* header */}
      <div className="pd-head">
        <div className="pd-title">
          <div className="pd-avatar"><Ic.terminal style={{ width: 16, height: 16 }} /></div>
          <div>
            <div className="pd-name">{proc.name}</div>
            <div className="pd-cmd"><code>{commandFor(proc)}</code></div>
          </div>
          <StatusPill status={running ? 'run' : 'stopped'} />
          {proc.name.includes('vite') || proc.name.includes('tauri') ? (
            <a className="pd-url">
              <span className="pd-dot" /> http://127.0.0.1:5173 <span className="pd-ext">↗</span>
            </a>
          ) : null}
        </div>

        <div className="pd-tabs">
          <div className="pd-tab active">Logs</div>
          <div className="pd-tab">Env</div>
          <div className="pd-tab">Routes <span className="n">4</span></div>
          <div className="pd-tab">History</div>
          <div className="pd-tab">Metrics</div>
        </div>
      </div>

      {/* filter bar */}
      <div className="pd-filter">
        <div className="seg compact">
          {['all','logs','http','warn'].map(k => (
            <button key={k} className={filter===k?'on':''} onClick={() => setFilter(k)}>{k.toUpperCase()}</button>
          ))}
        </div>
        <div className="pd-search">
          <Ic.search style={{ width: 12, height: 12, color: 'var(--fg-dim)' }} />
          <input
            value={query}
            onChange={e => setQuery(e.target.value)}
            placeholder="Filter logs (regex ok)…"
          />
          {query && <span className="kbd" style={{ fontSize: 9 }}>{lines.length}</span>}
        </div>
        <label className="pd-chk">
          <input type="checkbox" checked={follow} onChange={e => setFollow(e.target.checked)} />
          Follow
        </label>
        <button className="icon-btn" title="Clear"><Ic.x /></button>
        <button className="icon-btn" title="Wrap"><Ic.panel /></button>
      </div>

      {/* log surface */}
      <div className="pd-log" ref={logRef}>
        {lines.map((l,i) => <LogLine key={i} l={l} />)}
      </div>

      {/* footer */}
      <div className="pd-foot">
        <div className="pf-stats">
          <div className="pf-stat">
            <div className="pf-k">Started</div>
            <div className="pf-v">4/17/26, 2:25 PM</div>
          </div>
          <div className="pf-stat">
            <div className="pf-k">Uptime</div>
            <div className="pf-v">1:02:48</div>
          </div>
          <div className="pf-stat">
            <div className="pf-k">Memory</div>
            <div className="pf-v">124.6 MB <span className="pf-delta up">▲ 2.1</span></div>
          </div>
          <div className="pf-stat">
            <div className="pf-k">CPU</div>
            <div className="pf-v">3.4% <span className="pf-delta">—</span></div>
          </div>
          <div className="pf-stat">
            <div className="pf-k">Restarts</div>
            <div className="pf-v">3</div>
          </div>
          <div className="pf-spark">
            <Spark />
          </div>
        </div>
        <div className="pf-actions">
          <button className="pf-btn danger" onClick={() => setRunning(false)}>Force Stop</button>
          <button className="pf-btn">Restart</button>
          <button className="pf-btn primary" onClick={() => setRunning(r => !r)}>
            {running ? 'Stop' : 'Start'}
          </button>
        </div>
      </div>
    </div>
  );
}

function Spark() {
  // cpu-ish sparkline
  const pts = [3,4,3,6,9,5,4,3,3,5,8,12,7,4,3,3,2,4,6,4,3,3,5,9,11,8,4,3,3,4];
  const max = Math.max(...pts);
  const w = 160, h = 28;
  const step = w / (pts.length - 1);
  const d = pts.map((p, i) => `${i === 0 ? 'M' : 'L'} ${i*step} ${h - (p/max)*h}`).join(' ');
  return (
    <svg width={w} height={h} viewBox={`0 0 ${w} ${h}`}>
      <path d={d} stroke="var(--accent)" strokeWidth="1.4" fill="none"/>
      <path d={d + ` L ${w} ${h} L 0 ${h} Z`} fill="var(--accent-soft)"/>
    </svg>
  );
}

function findProc(id) {
  for (const g of PROC_GROUPS) {
    const p = g.items.find(i => i.id === id);
    if (p) return p;
  }
  return null;
}
function commandFor(p) {
  const map = {
    p_kernel: 'cargo run -p nexus-kernel -- --forge-path ~/notes',
    p_storage: '↳ spawned by nexus-kernel',
    p_tantivy: '↳ spawned by nexus-storage',
    p_tauri: 'cargo tauri dev',
    p_vite: 'pnpm --filter @nexus/app dev',
    p_tsc: 'tsc --watch --preserveWatchOutput',
    p_mcp: 'cargo run -p nexus-mcp -- serve --transport stdio',
    p_hellojs: 'wasm: plugins/hello-js.wasm',
    p_hellonx: 'wasm: plugins/hello-nexus.wasm',
    p_theme:   'wasm: core/com.nexus.theme.wasm',
    p_cargo:   'cargo check --workspace',
    p_bench:   './scripts/bench_run.sh',
  };
  return map[p.id] || p.name;
}

function Processes() {
  const [sel, setSel] = useStateP('p_tauri');
  return (
    <div className="proc-root">
      <ProcSidebar selectedId={sel} onSelect={setSel} />
      <ProcDetail id={sel} key={sel} />
    </div>
  );
}

Object.assign(window, { Processes });
