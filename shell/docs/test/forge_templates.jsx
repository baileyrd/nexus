/* Nexus Forge — Templates Gallery (Notion-style) + Vibe Coding page.
 * Pane type: 'templates'. Tabs inside: Gallery, Vibe Coding, AI Orchestrate overview.
 */
const { useState: useStateT, useMemo: useMemoT, useEffect: useEffectT } = React;

const TPL_CATS = [
  { id: 'all',       label: 'All templates', count: 142 },
  { id: 'personal',  label: 'Personal', count: 38 },
  { id: 'work',      label: 'Work & projects', count: 44 },
  { id: 'eng',       label: 'Engineering', count: 27 },
  { id: 'research',  label: 'Research & notes', count: 18 },
  { id: 'ops',       label: 'Ops & dashboards', count: 15 },
];

const TPL_FILTERS = [
  { id: 'forge', label: 'Nexus + Forge ▾' },
  { id: 'free',  label: 'Free + Paid ▾' },
  { id: 'pop',   label: 'Popular ▾' },
];

const TPLS = [
  {
    id: 't1', cat: 'personal', cover: 'grid',
    name: 'Weekly Planner',
    author: 'James',
    price: 'Free',
    desc: '7-day grid with habit rows and a Friday retro.',
    pop: 4820,
    layout: 'calendar',
  },
  {
    id: 't2', cat: 'personal', cover: 'analytics',
    name: 'Habit Tracker + Analytics',
    author: 'Joe',
    price: 'Free',
    desc: 'Tracks streaks, ships a live chart via the Bases plugin.',
    pop: 6210,
    layout: 'stats',
  },
  {
    id: 't3', cat: 'work', cover: 'inbox',
    name: 'Simple To-Do Workflow',
    author: 'studio-kafka',
    price: 'Free',
    desc: 'Inbox → Today → Done. Keyboard-driven. Zero config.',
    pop: 9101,
    layout: 'kanban',
  },
  {
    id: 't4', cat: 'personal', cover: 'dots',
    name: 'Simple Habit Tracker',
    author: 'James',
    price: 'Free',
    desc: 'Monthly dot matrix, one per habit, one per day.',
    pop: 3320,
    layout: 'matrix',
  },
  {
    id: 't5', cat: 'research', cover: 'skyline',
    name: 'Research OS',
    author: 'Emiko Adachi',
    price: 'Free',
    desc: 'Lit review, claims ledger, evidence graph.',
    pop: 2140,
    layout: 'doc',
  },
  {
    id: 't6', cat: 'ops', cover: 'prayers',
    name: 'Ritual Tracker',
    author: 'Optimize with Osama',
    price: 'Free',
    desc: 'Daily rituals with timezone-aware reminders.',
    pop: 1840,
    layout: 'list',
  },
  {
    id: 't7', cat: 'work', cover: 'board',
    name: 'PRD Implementation Status',
    author: 'Nexus Core',
    price: 'Free',
    desc: 'The exact template this forge uses. Status rows, PRD table, audit links.',
    pop: 12400, featured: true,
    layout: 'table',
  },
  {
    id: 't8', cat: 'eng', cover: 'code',
    name: 'Vibe-Coded Plugin Starter',
    author: 'Nexus Core',
    price: 'Free',
    desc: 'Describe what you want. AI scaffolds a plugin + manifest + tests.',
    pop: 5120, featured: true,
    layout: 'code',
  },
  {
    id: 't9', cat: 'ops', cover: 'stars',
    name: 'Agent Run Log',
    author: 'Nexus Core',
    price: 'Free',
    desc: 'Every AI Orchestrate run, logged as a note with plan + cost.',
    pop: 2980,
    layout: 'log',
  },
  {
    id: 't10', cat: 'personal', cover: 'book',
    name: 'Reading List',
    author: 'James',
    price: 'Free',
    desc: 'Books → Highlights → Atomic notes. One click to outline.',
    pop: 4510,
    layout: 'list',
  },
  {
    id: 't11', cat: 'work', cover: 'meet',
    name: 'Meeting Notes',
    author: 'studio-kafka',
    price: 'Free',
    desc: 'Agenda, decisions, action items. Backlinked to attendees.',
    pop: 7820,
    layout: 'doc',
  },
  {
    id: 't12', cat: 'eng', cover: 'dots',
    name: 'Incident Retro',
    author: 'Nexus Core',
    price: 'Free',
    desc: 'Timeline + 5-whys + follow-up tasks, auto-filed under /incidents.',
    pop: 1630,
    layout: 'table',
  },
];

/* --- Cover renderers --- */
function Cover({ t }) {
  switch (t.layout) {
    case 'calendar':
      return (
        <div className="tpl-cover tc-cal">
          <div className="tc-h"><span>✓</span> <b>{t.name}</b></div>
          <div className="tc-cal-grid">
            {'Mon Tue Wed Thu Fri'.split(' ').map((d,i) => (
              <div key={i} className="tc-cal-col">
                <div className="tc-cal-d">{d}</div>
                {Array.from({length: 3 + (i%2)}).map((_,j) => <div key={j} className="tc-cal-row"/>)}
              </div>
            ))}
          </div>
        </div>
      );
    case 'stats':
      return (
        <div className="tpl-cover tc-stats">
          <div className="tc-h"><span style={{color:'var(--ok)'}}>▮</span> <b>{t.name}</b></div>
          <div className="tc-stats-row">
            <div className="tc-kpi"><div className="tc-kpi-n">24.3%</div><div className="tc-kpi-l">Active days</div></div>
            <div className="tc-donut"><svg viewBox="0 0 40 40"><circle cx="20" cy="20" r="15" fill="none" stroke="var(--bg-raised)" strokeWidth="5"/><circle cx="20" cy="20" r="15" fill="none" stroke="var(--accent)" strokeWidth="5" strokeDasharray="62 94" transform="rotate(-90 20 20)"/></svg></div>
            <div className="tc-kpi"><div className="tc-kpi-n">7</div><div className="tc-kpi-l">Streak</div></div>
          </div>
          <div className="tc-bars">
            {[0.3,0.6,0.8,0.5,0.9,0.7,0.4].map((v,i) => <div key={i} className="tc-bar" style={{height: `${v*100}%`}} />)}
          </div>
        </div>
      );
    case 'kanban':
      return (
        <div className="tpl-cover tc-kanban">
          <div className="tc-h"><span>▢</span> <b>{t.name}</b></div>
          <div className="tc-kanban-row">
            {['Inbox','Today','Done'].map(c => (
              <div className="tc-col" key={c}>
                <div className="tc-col-h">{c}</div>
                <div className="tc-card"/>
                <div className="tc-card"/>
                {c==='Today' && <div className="tc-card"/>}
              </div>
            ))}
          </div>
        </div>
      );
    case 'matrix':
      return (
        <div className="tpl-cover tc-matrix">
          <div className="tc-h"><span>◉</span> <b>{t.name}</b></div>
          <div className="tc-matrix-grid">
            {Array.from({length: 7*12}).map((_,i) => (
              <div key={i} className={'tc-dot ' + (Math.random() > 0.45 ? 'on' : '')} />
            ))}
          </div>
        </div>
      );
    case 'doc':
      return (
        <div className="tpl-cover tc-doc">
          <div className="tc-h"><span>¶</span> <b>{t.name}</b></div>
          <div className="tc-lines">
            <div className="tc-l tc-l-h"/>
            <div className="tc-l"/><div className="tc-l w80"/>
            <div className="tc-l-h2 tc-l-h"/>
            <div className="tc-l"/><div className="tc-l w60"/><div className="tc-l w70"/>
          </div>
        </div>
      );
    case 'list':
      return (
        <div className="tpl-cover tc-list">
          <div className="tc-h"><span>≡</span> <b>{t.name}</b></div>
          {Array.from({length: 6}).map((_,i) => (
            <div key={i} className="tc-li">
              <span className="tc-check"/>
              <span className="tc-l w80"/>
            </div>
          ))}
        </div>
      );
    case 'table':
      return (
        <div className="tpl-cover tc-table">
          <div className="tc-h"><span>▦</span> <b>{t.name}</b></div>
          <div className="tc-tbl">
            <div className="tc-tr tc-tr-h">
              <div>PRD</div><div>Status</div><div>Owner</div><div>Gaps</div>
            </div>
            {['PRD-01','PRD-04','PRD-08','PRD-12','PRD-15'].map((p,i) => (
              <div key={p} className="tc-tr">
                <div><code>{p}</code></div>
                <div><span className={'tc-pill s'+(i%3)}>{['Done','Partial','Todo'][i%3]}</span></div>
                <div>@{['riko','jun','sam','mei','lw'][i]}</div>
                <div>{i%3===1?'2':i%3===2?'3':'—'}</div>
              </div>
            ))}
          </div>
        </div>
      );
    case 'code':
      return (
        <div className="tpl-cover tc-code">
          <div className="tc-h"><span style={{color:'var(--accent)'}}>⟁</span> <b>{t.name}</b></div>
          <div className="tc-code-body">
            <div><span className="tok-k">plugin</span> <span className="tok-s">"my-plugin"</span> {'{'}</div>
            <div style={{paddingLeft: 12}}>  <span className="tok-k">manifest</span>: <span className="tok-s">"v2"</span>,</div>
            <div style={{paddingLeft: 12}}>  <span className="tok-k">caps</span>: [<span className="tok-s">"forge.read"</span>,</div>
            <div style={{paddingLeft: 24}}>         <span className="tok-s">"mcp.call"</span>],</div>
            <div style={{paddingLeft: 12}}>  <span className="tok-k">hooks</span>: <span className="ai-cursor" style={{width:6,height:10}}/></div>
            <div>{'}'}</div>
          </div>
        </div>
      );
    case 'log':
      return (
        <div className="tpl-cover tc-log">
          <div className="tc-h"><span style={{color:'var(--accent)'}}>✸</span> <b>{t.name}</b></div>
          {['#f4e8b2 · done · $0.02','#a91c3d · done · $0.04','#b2e8f4 · running','#e8b2f4 · error'].map((s,i) => (
            <div key={i} className="tc-log-row">
              <span className="ai-sd" style={{background: i===2?'var(--accent)':i===3?'var(--risk)':'var(--ok)'}}/>
              <code>{s}</code>
            </div>
          ))}
        </div>
      );
    default:
      return (
        <div className="tpl-cover tc-doc">
          <div className="tc-h"><b>{t.name}</b></div>
          <div className="tc-lines">
            <div className="tc-l"/><div className="tc-l w80"/><div className="tc-l w60"/>
          </div>
        </div>
      );
  }
}

function TplCard({ t, onOpen }) {
  return (
    <div className={'tpl-card ' + (t.featured?'featured':'')} onClick={() => onOpen(t)}>
      <div className="tpl-cover-wrap">
        <Cover t={t}/>
        {t.featured && <span className="tpl-feat">Featured</span>}
      </div>
      <div className="tpl-meta">
        <div className="tpl-author">
          <span className="tpl-av">{t.author.slice(0,1)}</span>
          <span className="tpl-name">{t.name}</span>
        </div>
        <span className="tpl-price">{t.price}</span>
      </div>
    </div>
  );
}

/* --- Gallery tab --- */
function TemplatesGallery({ onOpen }) {
  const [cat, setCat] = useStateT('all');
  const [q, setQ] = useStateT('');
  const list = useMemoT(() => {
    return TPLS.filter(t => (cat==='all' || t.cat===cat) && (q==='' || t.name.toLowerCase().includes(q.toLowerCase())));
  }, [cat, q]);
  return (
    <div className="tpl-wrap">
      <div className="tpl-hero">
        <div className="tpl-hero-sub">Nexus Forge · Templates</div>
        <h1 className="tpl-hero-h">Start from a template.<br/><span>Bend it with Vibe Coding.</span></h1>
        <p className="tpl-hero-p">
          {TPLS.length}+ community templates — pages, databases, dashboards. Open one and the forge copies it into your vault; the blocks are yours to edit, backlink, or hand to the AI orchestrator for a refactor.
        </p>
        <div className="tpl-hero-actions">
          <button className="send-btn">Browse all 4,944</button>
          <button className="pf-btn" style={{height: 32, padding: '0 14px'}}>Publish your own</button>
        </div>
      </div>

      <div className="tpl-subnav">
        <div className="tpl-cats">
          {TPL_CATS.map(c => (
            <button key={c.id} className={'tpl-cat ' + (cat===c.id?'on':'')} onClick={() => setCat(c.id)}>
              {c.label} <span className="tpl-cat-n">{c.count}</span>
            </button>
          ))}
        </div>
        <div className="tpl-tools">
          {TPL_FILTERS.map(f => <button key={f.id} className="chip-btn">{f.label}</button>)}
          <div className="tpl-search">
            <Ic.search style={{width:12,height:12}}/>
            <input placeholder="Search templates…" value={q} onChange={e => setQ(e.target.value)} />
          </div>
        </div>
      </div>

      <div className="tpl-count">{list.length.toLocaleString()} Templates</div>

      <div className="tpl-grid">
        {list.map(t => <TplCard key={t.id} t={t} onOpen={onOpen} />)}
      </div>

      <div className="tpl-foot">
        Can't find it? Describe what you want in <b>Vibe Coding</b> — the forge will scaffold a template in seconds.
      </div>
    </div>
  );
}

/* --- Vibe Coding tab --- */
const VIBE_EXAMPLES = [
  'A reading list that auto-extracts quotes into atomic notes',
  'A PRD template with status pills and an audit-log sidebar',
  'A meeting note that backlinks attendees and files actions as tasks',
  'A weekly OKR page with a progress ring per key result',
];

const VIBE_STEPS = [
  { k:'parse',  name:'Parse prompt',     agent:'Planner',    t:'0.2s', out:'Identified: book list + highlights + atomic notes' },
  { k:'scheme', name:'Draft schema',     agent:'Doc Author', t:'0.8s', out:'Books{title,author,status} · Highlights{book,quote,pg} · Notes{hl,body,tags}' },
  { k:'blocks', name:'Lay out blocks',   agent:'Doc Author', t:'1.3s', out:'Page: H1 · Now-reading card · Books table · "Pull quotes" view' },
  { k:'bind',   name:'Wire relations',   agent:'Doc Author', t:'0.4s', out:'Highlights.book → Books.id · Notes.hl → Highlights.id' },
  { k:'review', name:'Critic pass',      agent:'Critic',     t:'0.6s', out:'Looks good. Suggest adding a "last-read" rollup.' },
];

function VibeCoding() {
  const [prompt, setPrompt] = useStateT('A reading list that auto-extracts quotes into atomic notes');
  const [running, setRunning] = useStateT(false);
  const [stepIdx, setStepIdx] = useStateT(VIBE_STEPS.length);

  useEffectT(() => {
    if (!running) return;
    setStepIdx(0);
    const id = setInterval(() => {
      setStepIdx(i => {
        if (i >= VIBE_STEPS.length) { clearInterval(id); setRunning(false); return i; }
        return i + 1;
      });
    }, 650);
    return () => clearInterval(id);
  }, [running]);

  return (
    <div className="vibe-wrap">
      <div className="vibe-head">
        <div className="tpl-hero-sub" style={{color:'var(--accent)'}}>Nexus Forge · Vibe Coding</div>
        <h1 className="vibe-h">Describe the page. <span>Forge it.</span></h1>
        <p className="vibe-p">Vibe Coding turns natural-language prompts into real Nexus pages — schemas, blocks, relations, views — generated by a small orchestration run against your vault's own design vocabulary.</p>
      </div>

      <div className="vibe-body">
        <div className="vibe-left">
          <div className="vibe-composer">
            <div className="ai-k">Prompt</div>
            <textarea rows={3} value={prompt} onChange={e => setPrompt(e.target.value)} placeholder="Describe the page you want…"/>
            <div className="vibe-ex">
              {VIBE_EXAMPLES.map((e,i) => (
                <button key={i} className="chip-btn" onClick={() => setPrompt(e)}>{e}</button>
              ))}
            </div>
            <div className="vibe-actions">
              <div style={{display:'flex', gap:6}}>
                <button className="chip-btn"><Ic.sparkle style={{width:12,height:12}}/> claude-3.7-sonnet</button>
                <button className="chip-btn">Match vault style</button>
                <button className="chip-btn">Create as /Templates/Generated</button>
              </div>
              <button className="send-btn" onClick={() => setRunning(true)} disabled={running}>
                <Ic.sparkle style={{width:12,height:12}}/> {running ? 'Forging…' : 'Forge page'}
              </button>
            </div>
          </div>

          <div className="vibe-runlog">
            <div className="panel-head" style={{padding:'0 2px'}}><span>Run log</span><span style={{color:'var(--fg-dim)', fontSize:11}}>5 agents · $0.008</span></div>
            {VIBE_STEPS.map((s,i) => {
              const done = i < stepIdx;
              const active = i === stepIdx - 1 && running;
              return (
                <div key={s.k} className={'vibe-step ' + (done?'done ':'') + (active?'active':'')}>
                  <div className="vibe-step-l">
                    <StatusDot s={done ? 'done' : active ? 'running' : 'queued'}/>
                    <div>
                      <div className="vibe-step-n">{s.name}</div>
                      <div className="vibe-step-a">{s.agent} · {s.t}</div>
                    </div>
                  </div>
                  <div className="vibe-step-r">{done || active ? s.out : '—'}</div>
                </div>
              );
            })}
          </div>
        </div>

        <div className="vibe-right">
          <div className="panel-head" style={{padding:'0 2px'}}>
            <span>Preview · /Templates/Generated/Reading List.md</span>
            <span style={{color:'var(--fg-dim)', fontSize:11}}>live</span>
          </div>
          <div className="vibe-preview">
            <div className="vp-title">📚 Reading List</div>
            <div className="vp-sub">Auto-extracts quotes into atomic notes</div>
            <div className="vp-card">
              <div className="vp-card-k">Now reading</div>
              <div className="vp-card-t">The Design of Everyday Things</div>
              <div className="vp-card-m">Don Norman · 61% · last opened Tue</div>
            </div>
            <h3 className="vp-h">Books</h3>
            <div className="vp-tbl">
              <div className="vp-tr vp-tr-h">
                <div>Title</div><div>Author</div><div>Status</div><div>Highlights</div>
              </div>
              {[
                ['Thinking in Systems','D. Meadows','Done', '32'],
                ['The Design of Everyday Things','D. Norman','Reading','14'],
                ['Seeing Like a State','J. Scott','Queued','—'],
              ].map((r,i) => (
                <div key={i} className="vp-tr">
                  {r.map((c,j) => <div key={j}>{c}</div>)}
                </div>
              ))}
            </div>
            <h3 className="vp-h">Recent pull-quotes</h3>
            <div className="vp-q">"Design is really an act of communication" — Norman, p. 8</div>
            <div className="vp-q">"A system is a set of elements coherently organised" — Meadows, p. 11</div>
            <div className="vp-foot">
              <span className="ai-chip"><code>books.id</code> ← Highlights.book</span>
              <span className="ai-chip"><code>highlights.id</code> ← Notes.hl</span>
            </div>
          </div>
        </div>
      </div>
    </div>
  );
}

/* --- Workspace root --- */
function Templates() {
  const [sub, setSub] = useStateT('gallery');
  const [opened, setOpened] = useStateT(null);

  return (
    <div className="tpl-root">
      <div className="tpl-topbar">
        <div className="tpl-brand">
          <span className="tpl-logo">N</span>
          <span>Templates</span>
        </div>
        <div className="seg">
          {[
            {k:'gallery', n:'Gallery'},
            {k:'vibe',    n:'Vibe Coding'},
          ].map(o => (
            <button key={o.k} className={sub===o.k?'on':''} onClick={() => setSub(o.k)}>{o.n}</button>
          ))}
        </div>
        <div style={{display:'flex', gap: 8, alignItems:'center'}}>
          <button className="chip-btn"><Ic.sparkle style={{width:12,height:12}}/> Forge assist</button>
          <button className="send-btn" style={{height:28}}>Use template</button>
        </div>
      </div>
      <div className="tpl-body">
        {sub === 'gallery' && <TemplatesGallery onOpen={setOpened} />}
        {sub === 'vibe' && <VibeCoding/>}
      </div>

      {opened && (
        <div className="tpl-modal" onClick={() => setOpened(null)}>
          <div className="tpl-modal-inner" onClick={e => e.stopPropagation()}>
            <div className="tpl-modal-cover"><Cover t={opened}/></div>
            <div className="tpl-modal-meta">
              <div className="tpl-modal-h">
                <div>
                  <div className="tpl-hero-sub" style={{fontSize:10}}>by {opened.author} · {opened.pop.toLocaleString()} uses</div>
                  <div className="tpl-modal-t">{opened.name}</div>
                </div>
                <button className="icon-btn" onClick={() => setOpened(null)}><Ic.x/></button>
              </div>
              <div className="tpl-modal-d">{opened.desc}</div>
              <div className="tpl-modal-actions">
                <button className="send-btn">Use this template</button>
                <button className="pf-btn" style={{height:32, padding:'0 14px'}}>Preview in forge</button>
                <button className="chip-btn"><Ic.sparkle style={{width:12,height:12}}/> Remix with Vibe</button>
              </div>
              <div className="ai-k" style={{marginTop:14}}>Includes</div>
              <ul className="tpl-modal-incl">
                <li>1 page · 3 databases · 2 saved views</li>
                <li>Relations pre-wired</li>
                <li>MIT · editable</li>
              </ul>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}

Object.assign(window, { Templates });
