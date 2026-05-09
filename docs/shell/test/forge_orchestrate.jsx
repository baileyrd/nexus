/* Nexus Forge — AI Orchestration workspace (PRD-12 AI Engine + PRD-15 Agents).
 * Three layouts, switchable via tweaks: 'agents', 'dag', 'chat'.
 */
const { useState: useStateO, useEffect: useEffectO, useRef: useRefO } = React;

const AGENTS = [
  { id: 'a_plan',   name: 'Planner',      role: 'planner',  model: 'claude-3.7-sonnet', status: 'done',    cost: '$0.012', tok: '4.2k', t: '2.1s' },
  { id: 'a_rag',    name: 'Forge RAG',    role: 'retrieval',model: 'local/bge-large',   status: 'done',    cost: '$0.000', tok: '—',    t: '0.4s' },
  { id: 'a_search', name: 'Audit Finder', role: 'worker',   model: 'claude-3.5-haiku',  status: 'running', cost: '$0.004', tok: '1.8k', t: '…' },
  { id: 'a_author', name: 'Doc Author',   role: 'worker',   model: 'claude-3.7-sonnet', status: 'queued',  cost: '—',      tok: '—',    t: '—' },
  { id: 'a_crit',   name: 'Critic',       role: 'critic',   model: 'claude-3.7-sonnet', status: 'queued',  cost: '—',      tok: '—',    t: '—' },
];

const PLAN = {
  goal: 'Update Nexus PRD Implementation Status with findings from latest audits',
  nodes: [
    { id: 'n1', t: 'Parse goal',          agent: 'a_plan',   x: 80,  y: 70,  status: 'done' },
    { id: 'n2', t: 'Retrieve current doc',agent: 'a_rag',    x: 260, y: 40,  status: 'done' },
    { id: 'n3', t: 'Retrieve UI-AUDIT.md',agent: 'a_rag',    x: 260, y: 100, status: 'done' },
    { id: 'n4', t: 'Retrieve MICROKERNEL', agent: 'a_rag',   x: 260, y: 160, status: 'done' },
    { id: 'n5', t: 'Extract findings',    agent: 'a_search', x: 460, y: 100, status: 'running' },
    { id: 'n6', t: 'Map to PRDs',         agent: 'a_search', x: 460, y: 170, status: 'queued' },
    { id: 'n7', t: 'Draft updates',       agent: 'a_author', x: 660, y: 100, status: 'queued' },
    { id: 'n8', t: 'Review & critique',   agent: 'a_crit',   x: 860, y: 100, status: 'queued' },
  ],
  edges: [
    ['n1','n2'],['n1','n3'],['n1','n4'],
    ['n2','n5'],['n3','n5'],['n4','n5'],
    ['n5','n6'],['n6','n7'],['n7','n8'],
  ],
};

const TRACE = [
  { t: 0.00, kind: 'system', a: null,     m: 'Run #f4e8b2 started · budget $0.10 · timeout 120s' },
  { t: 0.04, kind: 'thought', a: 'a_plan', m: 'User wants the implementation-status doc refreshed against two audit files. I need to: (1) fetch the doc, (2) fetch both audits, (3) extract any finding-ids not yet reflected, (4) propose edits.' },
  { t: 0.35, kind: 'tool',   a: 'a_plan',  m: 'plan.commit', arg: 'decomposed into 8 steps (3 parallel retrievals).' },
  { t: 0.36, kind: 'hand',   a: 'a_plan',  m: '→ 3× Forge RAG (parallel)' },

  { t: 0.41, kind: 'tool',   a: 'a_rag',   m: 'forge.read', arg: 'Nexus_Work/Implementation Status.md' },
  { t: 0.48, kind: 'obs',    a: 'a_rag',   m: '14,139 chars · 17 headings · frontmatter tags=[#status,#prd]' },
  { t: 0.52, kind: 'tool',   a: 'a_rag',   m: 'forge.read', arg: 'docs/UI-AUDIT.md' },
  { t: 0.61, kind: 'obs',    a: 'a_rag',   m: '8,202 chars · 6 findings (UA-2026-01..06)' },
  { t: 0.62, kind: 'tool',   a: 'a_rag',   m: 'forge.read', arg: 'docs/MICROKERNEL-AUDIT.md' },
  { t: 0.71, kind: 'obs',    a: 'a_rag',   m: '3,901 chars · 2 findings (KA-2025-11 RESOLVED, KA-2026-02 OPEN)' },
  { t: 0.74, kind: 'hand',   a: 'a_rag',   m: '→ Audit Finder' },

  { t: 0.80, kind: 'thought',a: 'a_search',m: 'Cross-referencing findings against PRD rows. UA-2026-03 (Editor Engine drift) and KA-2026-02 (Kernel capability gaps) appear unreflected in the status doc.' },
  { t: 0.94, kind: 'tool',   a: 'a_search',m: 'graph.neighbors', arg: 'docs/UI-AUDIT.md depth=1' },
  { t: 1.08, kind: 'obs',    a: 'a_search',m: '12 linked notes · 4 tagged #prd' },
  { t: 1.16, kind: 'tool',   a: 'a_search',m: 'fts.search', arg: '"UA-2026-" AND "Implementation Status"' },
];

const TOOLS = [
  { id:'forge.read',      n: 23, ok: 23, group: 'Storage' },
  { id:'fts.search',      n: 8,  ok: 8,  group: 'Storage' },
  { id:'graph.neighbors', n: 4,  ok: 4,  group: 'Graph' },
  { id:'graph.backlinks', n: 2,  ok: 2,  group: 'Graph' },
  { id:'plan.commit',     n: 1,  ok: 1,  group: 'Meta' },
  { id:'mcp.call',        n: 0,  ok: 0,  group: 'MCP' },
  { id:'git.diff',        n: 0,  ok: 0,  group: 'Git' },
];

const CITATIONS = [
  { file: 'docs/UI-AUDIT.md',          line: 'UA-2026-03', snip: 'Editor engine §4 BlockPositionMap description is stale; CM6 owns text now.' },
  { file: 'docs/MICROKERNEL-AUDIT.md', line: 'KA-2026-02', snip: 'Kernel capability table missing three entries added in phase-V.' },
  { file: 'Nexus_Work/Implementation Status.md', line: 'PRD-08 row', snip: 'Status currently 🟡 Partial — should stay 🟡 but add finding-id UA-2026-03 to gaps line.' },
];

function Av({ role }) {
  const map = {
    planner:   { g: 'P', c: 'var(--accent)' },
    retrieval: { g: 'R', c: 'var(--cool)' },
    worker:    { g: 'W', c: 'oklch(0.82 0.12 140)' },
    critic:    { g: 'C', c: 'var(--warn)' },
  };
  const v = map[role] || map.worker;
  return <span className="ai-av" style={{ background: v.c }}>{v.g}</span>;
}

function StatusDot({ s }) {
  const c = s === 'done' ? 'var(--ok)' : s === 'running' ? 'var(--accent)' : s === 'err' ? 'var(--risk)' : 'var(--fg-dim)';
  const pulse = s === 'running';
  return <span className={'ai-sd ' + (pulse ? 'pulse' : '')} style={{ background: c }} />;
}

/* ---------------- Layout A: Agents list / Trace / Tools & Cite ---------------- */
function AgentsLayout() {
  const [selA, setSelA] = useStateO('a_search');
  return (
    <div className="ai-layout-agents">
      {/* agents column */}
      <div className="ai-col ai-agents">
        <div className="panel-head" style={{ paddingLeft: 14 }}>
          <span>Agents · run #f4e8b2</span>
          <div className="actions"><button className="icon-btn"><Ic.plus /></button></div>
        </div>
        <div className="ai-goal">
          <div className="ai-k">Goal</div>
          <div className="ai-g">{PLAN.goal}</div>
          <div className="ai-gbar">
            <div className="ai-gbar-fill" style={{ width: '42%' }} />
          </div>
          <div className="ai-g-meta">
            <span>42% · 3/8 steps</span>
            <span style={{ color: 'var(--fg-dim)' }}>· $0.016 of $0.10</span>
          </div>
        </div>
        <div className="ai-agents-list">
          {AGENTS.map(a => (
            <div key={a.id} className={'ai-agent ' + (selA===a.id?'active':'')} onClick={() => setSelA(a.id)}>
              <Av role={a.role} />
              <div className="ai-agent-b">
                <div className="ai-agent-n">
                  {a.name}
                  <StatusDot s={a.status} />
                </div>
                <div className="ai-agent-s">
                  <code>{a.model}</code>
                </div>
              </div>
              <div className="ai-agent-r">
                <div>{a.tok}</div>
                <div style={{ color: 'var(--fg-dim)' }}>{a.cost}</div>
              </div>
            </div>
          ))}
        </div>
        <div className="ai-foot">
          <button className="pf-btn danger"><span className="sq"/> Abort run</button>
          <button className="pf-btn">Pause</button>
        </div>
      </div>

      {/* trace column */}
      <div className="ai-col ai-trace-col">
        <div className="panel-head" style={{ paddingLeft: 20 }}>
          <span>Execution trace</span>
          <div className="actions">
            <button className="icon-btn" title="Autoscroll"><Ic.bolt /></button>
            <button className="icon-btn" title="Copy"><Ic.doc /></button>
          </div>
        </div>
        <div className="ai-trace">
          {TRACE.map((l,i) => <TraceLine key={i} l={l} />)}
          <div className="ai-streaming">
            <span className="ai-cursor" />
            <span className="ai-tstream">Scanning PRD-08 row for UA-2026-03 reference…</span>
          </div>
        </div>
      </div>

      {/* tools + citations column */}
      <div className="ai-col ai-right">
        <div className="panel-head" style={{ paddingLeft: 14 }}>
          <span>Tool calls · {TOOLS.reduce((a,b)=>a+b.n,0)}</span>
        </div>
        <div className="ai-tools">
          {TOOLS.map(t => (
            <div key={t.id} className="ai-tool">
              <div className="ai-tool-n">
                <span className="ai-tool-glyph">⟁</span>
                <code>{t.id}</code>
              </div>
              <div className="ai-tool-g">{t.group}</div>
              <div className="ai-tool-c">
                <span style={{ color: t.n ? 'var(--fg)' : 'var(--fg-dim)' }}>{t.n}</span>
                {t.n > 0 && <span style={{ color: 'var(--ok)', fontSize: 10 }}>✓ {t.ok}</span>}
              </div>
            </div>
          ))}
        </div>
        <div className="panel-head" style={{ paddingLeft: 14, borderTop: '1px solid var(--line-soft)' }}>
          <span>Citations</span>
        </div>
        <div className="ai-cites">
          {CITATIONS.map((c,i) => (
            <div className="ai-cite" key={i}>
              <div className="ai-cite-h">
                <Ic.doc style={{ width: 12, height: 12, color: 'var(--fg-dim)' }} />
                <span>{c.file}</span>
                <code>{c.line}</code>
              </div>
              <div className="ai-cite-b">{c.snip}</div>
            </div>
          ))}
        </div>
      </div>
    </div>
  );
}

function TraceLine({ l }) {
  const cls = 'ai-trline k-' + l.kind;
  const ag = l.a ? AGENTS.find(a => a.id === l.a) : null;
  const kindLabel = {
    system: 'SYSTEM', thought: 'THINK', tool: 'TOOL', obs: 'OBS', hand: 'HANDOFF', msg: 'MSG'
  }[l.kind] || l.kind.toUpperCase();
  return (
    <div className={cls}>
      <span className="ai-tl-t">+{l.t.toFixed(2)}s</span>
      {ag ? <Av role={ag.role} /> : <span className="ai-av empty">◦</span>}
      <span className="ai-tl-kind">{kindLabel}</span>
      <div className="ai-tl-body">
        {l.kind === 'tool' ? <>
          <code>{l.m}</code>{l.arg && <> · <span className="ai-tl-arg">{l.arg}</span></>}
        </> : l.m}
      </div>
    </div>
  );
}

/* ---------------- Layout B: DAG ---------------- */
function DagLayout() {
  const [sel, setSel] = useStateO('n5');
  const by = Object.fromEntries(PLAN.nodes.map(n => [n.id, n]));
  const selNode = by[sel];
  const selAgent = AGENTS.find(a => a.id === selNode?.agent);
  const selTrace = TRACE.filter(l => l.a === selNode?.agent).slice(-6);

  const colorFor = s => s === 'done' ? 'var(--ok)' : s === 'running' ? 'var(--accent)' : 'var(--fg-dim)';

  return (
    <div className="ai-layout-dag">
      <div className="ai-dag-head">
        <div>
          <div className="ai-k" style={{ marginBottom: 2 }}>Plan · run #f4e8b2</div>
          <div style={{ fontSize: 14, fontWeight: 500 }}>{PLAN.goal}</div>
        </div>
        <div className="ai-dag-legend">
          <span><span className="ai-sd" style={{ background: 'var(--ok)' }} /> done</span>
          <span><span className="ai-sd pulse" style={{ background: 'var(--accent)' }} /> running</span>
          <span><span className="ai-sd" style={{ background: 'var(--fg-dim)' }} /> queued</span>
        </div>
      </div>
      <div className="ai-dag-wrap">
        <svg viewBox="0 0 960 220" className="ai-dag">
          <defs>
            <marker id="ah" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="6" markerHeight="6" orient="auto">
              <path d="M0,0 L10,5 L0,10 z" fill="var(--line)" />
            </marker>
          </defs>
          {PLAN.edges.map(([a,b], i) => (
            <path key={i}
              d={`M ${by[a].x+54} ${by[a].y} C ${by[a].x+110} ${by[a].y}, ${by[b].x-40} ${by[b].y}, ${by[b].x-24} ${by[b].y}`}
              stroke={by[a].status==='done' && by[b].status!=='queued' ? 'var(--accent)' : 'var(--line)'}
              strokeWidth="1.2"
              fill="none" markerEnd="url(#ah)" />
          ))}
          {PLAN.nodes.map(n => {
            const ag = AGENTS.find(a => a.id === n.agent);
            return (
              <g key={n.id}
                className={'ai-dag-node ' + (sel===n.id?'sel':'') + ' s-' + n.status}
                onClick={() => setSel(n.id)}
                style={{ cursor: 'pointer' }}>
                <rect x={n.x-54} y={n.y-18} width="108" height="36" rx="8"
                  fill="var(--bg-raised)"
                  stroke={sel===n.id ? 'var(--accent)' : colorFor(n.status)}
                  strokeWidth={sel===n.id ? 2 : 1} />
                <circle cx={n.x-44} cy={n.y} r="4" fill={colorFor(n.status)} />
                {n.status === 'running' && (
                  <circle cx={n.x-44} cy={n.y} r="4" fill="none" stroke="var(--accent)" strokeWidth="1.2">
                    <animate attributeName="r" from="4" to="10" dur="1.2s" repeatCount="indefinite" />
                    <animate attributeName="opacity" from="1" to="0" dur="1.2s" repeatCount="indefinite" />
                  </circle>
                )}
                <text x={n.x-34} y={n.y-2} fontSize="10" fontFamily="Inter" fill="var(--fg)">{n.t}</text>
                <text x={n.x-34} y={n.y+10} fontSize="9" fontFamily="JetBrains Mono" fill="var(--fg-dim)">{ag?.name}</text>
              </g>
            );
          })}
        </svg>
      </div>
      <div className="ai-dag-detail">
        <div className="ai-dag-det-l">
          <div className="ai-k">Selected step</div>
          <div className="ai-det-t">{selNode?.t}</div>
          <div className="ai-det-sub">
            <Av role={selAgent?.role}/>
            <span>{selAgent?.name}</span>
            <code>{selAgent?.model}</code>
            <StatusDot s={selNode?.status}/>
          </div>
        </div>
        <div className="ai-dag-det-r">
          <div className="ai-k">Recent trace</div>
          {selTrace.length === 0 && <div style={{ color: 'var(--fg-dim)', fontSize: 12, padding: '6px 0' }}>No events yet.</div>}
          {selTrace.map((l,i) => <TraceLine key={i} l={l} />)}
        </div>
      </div>
    </div>
  );
}

/* ---------------- Layout C: Chat + live execution ---------------- */
function ChatLayout() {
  const [msg, setMsg] = useStateO('Refresh the implementation-status doc against the latest audits.');
  return (
    <div className="ai-layout-chat">
      <div className="ai-chat-col">
        <div className="panel-head" style={{ paddingLeft: 14 }}>
          <span>Thread · Implementation audit</span>
        </div>
        <div className="ai-chat">
          <div className="ai-msg u">
            <div className="ai-av" style={{ background: 'var(--cool)' }}>LW</div>
            <div className="ai-bub">
              <div className="ai-bub-h">You <span>2:43 PM</span></div>
              <div className="ai-bub-b">Refresh the implementation-status doc against the latest audits in docs/. Don't rewrite — just surface findings I haven't yet reflected, with finding-ids.</div>
            </div>
          </div>
          <div className="ai-msg a">
            <Av role="planner"/>
            <div className="ai-bub">
              <div className="ai-bub-h">Planner <span>2:43 PM · 2.1s · 4.2k tok</span></div>
              <div className="ai-bub-b">I'll run Forge RAG in parallel across the implementation-status doc and the two audit files, then have the Audit Finder extract unreflected finding-ids and propose minimal edits. Critic will review before I show you anything.</div>
              <div className="ai-bub-plan">
                <div className="ai-bub-pl">▸ 8-step plan committed · 3 parallel retrievals · budget $0.10</div>
              </div>
            </div>
          </div>
          <div className="ai-msg a">
            <Av role="retrieval"/>
            <div className="ai-bub">
              <div className="ai-bub-h">Forge RAG <span>2:43 PM · 0.4s</span></div>
              <div className="ai-bub-b">Retrieved 3 docs · 26,242 chars total.</div>
              <div className="ai-bub-cites">
                {CITATIONS.map((c,i) => (
                  <div key={i} className="ai-chip">
                    <Ic.doc style={{ width: 10, height: 10 }}/>
                    {c.file.split('/').pop()} · <code>{c.line}</code>
                  </div>
                ))}
              </div>
            </div>
          </div>
          <div className="ai-msg a">
            <Av role="worker"/>
            <div className="ai-bub">
              <div className="ai-bub-h">Audit Finder <span>running…</span> <StatusDot s="running"/></div>
              <div className="ai-bub-b">
                Cross-referencing 8 findings against the 17 PRD rows. So far:
                <ul className="ai-bub-ul">
                  <li><code>UA-2026-03</code> — PRD-08 gaps line needs update (editor §4)</li>
                  <li><code>KA-2026-02</code> — PRD-01 capability table entry missing</li>
                </ul>
                <span className="ai-cursor" />
              </div>
            </div>
          </div>
        </div>
        <div className="ai-composer">
          <textarea value={msg} onChange={e => setMsg(e.target.value)} rows={2} placeholder="Reply to the run, or start a new one…" />
          <div className="ai-composer-b">
            <div style={{ display:'flex', gap: 6, alignItems: 'center' }}>
              <button className="chip-btn"><Ic.plug style={{width:12,height:12}}/> claude-3.7-sonnet</button>
              <button className="chip-btn">temp 0.2</button>
              <button className="chip-btn"><Ic.bolt style={{width:12,height:12}}/> 3 tools</button>
            </div>
            <button className="send-btn"><Ic.bolt style={{width:12,height:12}}/> Run</button>
          </div>
        </div>
      </div>
      <div className="ai-exec-col">
        <div className="panel-head" style={{ paddingLeft: 14 }}>
          <span>Live execution</span>
          <div className="actions">
            <button className="icon-btn"><Ic.bolt/></button>
          </div>
        </div>
        <div className="ai-exec-plan">
          {PLAN.nodes.map(n => {
            const ag = AGENTS.find(a => a.id === n.agent);
            return (
              <div key={n.id} className={'ai-exec-node s-' + n.status}>
                <StatusDot s={n.status}/>
                <div className="ai-exec-n-b">
                  <div className="ai-exec-n-t">{n.t}</div>
                  <div className="ai-exec-n-s">
                    <span>{ag?.name}</span> · <code>{ag?.model}</code>
                  </div>
                </div>
                <span className="ai-exec-n-st">{n.status}</span>
              </div>
            );
          })}
        </div>
        <div className="ai-exec-trace">
          <div className="panel-head"><span>Recent events</span></div>
          {TRACE.slice(-8).map((l,i) => <TraceLine key={i} l={l}/>)}
        </div>
      </div>
    </div>
  );
}

/* -------------------- Root -------------------- */
function Orchestrate() {
  const [layout, setLayout] = useStateO(window.__aiLayout || 'agents');
  useEffectO(() => {
    window.__setAiLayout = (l) => setLayout(l);
  }, []);
  return (
    <div className="ai-root">
      <div className="ai-topbar">
        <div style={{ display:'flex', alignItems:'center', gap: 10 }}>
          <div className="ai-brand-chip">
            <Ic.bolt style={{ width: 14, height: 14 }}/>
            <span>Nexus Orchestrate</span>
            <span className="ai-run">run #f4e8b2</span>
          </div>
          <StatusDot s="running"/>
          <span style={{ color: 'var(--fg-muted)', fontSize: 12 }}>running · elapsed 01:24</span>
        </div>
        <div className="seg" style={{ padding: 2 }}>
          {[
            {k:'agents', n:'Agents'},
            {k:'dag',    n:'Plan DAG'},
            {k:'chat',   n:'Chat + exec'},
          ].map(o => (
            <button key={o.k} className={layout===o.k?'on':''} onClick={() => setLayout(o.k)}>{o.n}</button>
          ))}
        </div>
        <div style={{ display:'flex', gap: 6, alignItems:'center' }}>
          <span className="ai-stat"><code>$0.016</code> / $0.10</span>
          <span className="ai-stat"><code>5,982</code> tok</span>
          <button className="pf-btn" style={{ flex: 'none', padding: '0 12px', height: 26 }}>Export run</button>
        </div>
      </div>
      <div className="ai-body">
        {layout==='agents' && <AgentsLayout/>}
        {layout==='dag' && <DagLayout/>}
        {layout==='chat' && <ChatLayout/>}
      </div>
    </div>
  );
}

Object.assign(window, { Orchestrate });
