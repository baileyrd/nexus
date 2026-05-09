/* Nexus Forge — left tree, right outline & props & graph. */
const { useState, useMemo, useEffect, useRef } = React;

function ActivityRail({ pane, setPane, onTweaks }) {
  const items = [
    { k: 'files', ic: Ic.doc,      label: 'Files' },
    { k: 'search', ic: Ic.search,  label: 'Search', pip: true },
    { k: 'graph', ic: Ic.graph,    label: 'Graph' },
    { k: 'tasks', ic: Ic.task,     label: 'Tasks' },
    { k: 'git', ic: Ic.git,        label: 'Git' },
    { k: 'db', ic: Ic.db,          label: 'Bases' },
    { k: 'templates', ic: Ic.star, label: 'Templates' },
    { k: 'ai', ic: Ic.sparkle,     label: 'AI Orchestrate' },
  ];
  return (
    <div className="rail">
      {items.map(it => (
        <button key={it.k}
          className={'rail-btn ' + (pane === it.k ? 'active' : '')}
          onClick={() => setPane(it.k)}
          title={it.label}>
          <it.ic />
          {it.pip && <span className="pip" />}
        </button>
      ))}
      <div className="spacer" />
      <button className={'rail-btn ' + (pane === 'terminal' ? 'active' : '')} onClick={() => setPane('terminal')} title="Terminal"><Ic.terminal /></button>
      <button className="rail-btn" title="Plugins"><Ic.plug /></button>
      <button className="rail-btn" onClick={onTweaks} title="Tweaks"><Ic.sliders /></button>
      <button className="rail-btn" title="Settings"><Ic.settings /></button>
    </div>
  );
}

function TreeNode({ node, depth, onOpen, activeId, filter }) {
  const [open, setOpen] = useState(!!node.open);
  const isFile = node.type === 'file';

  // filtering: if node or any descendant matches, show
  const matches = useMemo(() => {
    if (!filter) return true;
    const f = filter.toLowerCase();
    const self = node.name.toLowerCase().includes(f);
    const child = (node.children || []).some(c => deepMatch(c, f));
    return self || child;
  }, [filter, node]);
  useEffect(() => { if (filter && !isFile) setOpen(true); }, [filter]);
  if (!matches) return null;

  const Icon = isFile ? Ic.doc : (open ? Ic.folderOpen : Ic.folder);
  return (
    <>
      <div
        className={'row ' + (open ? 'open ' : '') + (activeId === node.id ? 'active' : '')}
        onClick={() => isFile ? onOpen(node) : setOpen(o => !o)}
        style={{ paddingLeft: 6 + depth * 14 }}
      >
        {!isFile
          ? <span className="caret"><Ic.chev style={{ width: 10, height: 10 }} /></span>
          : <span className="caret" />}
        <span className="icon"><Icon style={{ width: 13, height: 13 }} /></span>
        <span className="name">{node.name}</span>
        {node.dot && <span className={'dot ' + node.dot} />}
        {node.count && <span className="count">{node.count}</span>}
      </div>
      {!isFile && open && (
        <div className="children">
          {(node.children || []).map(c => (
            <TreeNode key={c.id} node={c} depth={depth + 1} onOpen={onOpen} activeId={activeId} filter={filter} />
          ))}
        </div>
      )}
    </>
  );
}
function deepMatch(n, f) {
  if (n.name.toLowerCase().includes(f)) return true;
  return (n.children || []).some(c => deepMatch(c, f));
}

function LeftPanel({ activeId, onOpen }) {
  const [filter, setFilter] = useState('');
  return (
    <div className="leftpanel">
      <div className="panel-head">
        <span>Nexus_Work</span>
        <div className="actions">
          <button className="icon-btn" title="New note"><Ic.plus /></button>
          <button className="icon-btn" title="New folder"><Ic.folder /></button>
          <button className="icon-btn" title="Collapse"><Ic.min /></button>
        </div>
      </div>
      <div className="filter">
        <Ic.search style={{ width: 12, height: 12, color: 'var(--fg-dim)' }} />
        <input value={filter} onChange={e => setFilter(e.target.value)} placeholder="Filter files…" />
        <span className="kbd">⌘P</span>
      </div>
      <div className="tree">
        {TREE.map(n => (
          <TreeNode key={n.id} node={n} depth={0} onOpen={onOpen} activeId={activeId} filter={filter} />
        ))}
      </div>
      <div className="leftfoot">
        <div className="u">
          <div className="av">LW</div>
          <span>lap-working</span>
        </div>
        <div style={{ display: 'flex', gap: 4 }}>
          <button className="icon-btn" title="Help"><span style={{fontSize:11}}>?</span></button>
          <button className="icon-btn" title="Settings"><Ic.settings /></button>
        </div>
      </div>
    </div>
  );
}

function Outline({ activeId, onJump }) {
  return (
    <div className="rpane">
      <div className="ol-section"><span>Document outline</span><span className="kbd" style={{fontSize:9}}>16 hdrs</span></div>
      {OUTLINE.map(o => (
        <div
          key={o.id}
          className={'ol-item lvl-' + o.lvl + (activeId === o.id ? ' active' : '')}
          onClick={() => onJump(o.id)}
        >
          {o.n && <span className="n">{o.n}</span>}
          <span className="t">{o.t}</span>
          <span className="s" style={{ color: 'var(--fg-dim)' }}>{o.size}</span>
        </div>
      ))}
    </div>
  );
}

function Backlinks() {
  return (
    <div className="rpane">
      <div className="ol-section"><span>Linked mentions · {BACKLINKS.length}</span><span className="kbd" style={{fontSize:9}}>12 in</span></div>
      {BACKLINKS.map((b, i) => (
        <div className="bl-item" key={i}>
          <div className="bl-file">
            <Ic.doc />
            <span>{b.file}</span>
            <span style={{ marginLeft: 'auto', color: 'var(--fg-dim)', fontFamily: 'var(--f-mono)', fontSize: 10 }}>{b.time}</span>
          </div>
          <div className="bl-ctx" dangerouslySetInnerHTML={{ __html: b.ctx }} />
        </div>
      ))}
    </div>
  );
}

function GraphPane() {
  // tiny synthetic local graph — the current file + neighbors.
  const nodes = [
    { id: 'impl', x: 140, y: 110, r: 22, label: 'Impl Status', cur: true },
    { id: 'bc',   x: 50,  y: 60,  r: 10, label: 'Backlog-Current' },
    { id: 'ui',   x: 60,  y: 170, r: 9,  label: 'UI-AUDIT' },
    { id: 'mk',   x: 230, y: 60,  r: 9,  label: 'Microkernel' },
    { id: 'rec',  x: 240, y: 175, r: 8,  label: 'Recap' },
    { id: 'feat', x: 145, y: 210, r: 7,  label: 'Feature Backlog' },
    { id: 'ka',   x: 30,  y: 120, r: 6,  label: 'kernel-audit' },
  ];
  const edges = [
    ['impl','bc'],['impl','ui'],['impl','mk'],['impl','rec'],['impl','feat'],['bc','ka'],['ui','ka']
  ];
  const by = Object.fromEntries(nodes.map(n => [n.id, n]));
  return (
    <div className="rpane">
      <div className="ol-section"><span>Local graph · depth 2</span><span className="kbd" style={{fontSize:9}}>7 nodes</span></div>
      <div className="graph">
        <svg viewBox="0 0 280 240">
          {edges.map(([a,b], i) => (
            <line key={i} x1={by[a].x} y1={by[a].y} x2={by[b].x} y2={by[b].y}
              stroke="var(--line)" strokeWidth="1" />
          ))}
          {nodes.map(n => (
            <g key={n.id}>
              <circle cx={n.x} cy={n.y} r={n.r}
                fill={n.cur ? 'var(--accent)' : 'var(--bg)'}
                stroke={n.cur ? 'var(--accent)' : 'var(--fg-dim)'}
                strokeWidth={n.cur ? 0 : 1.2}
              />
              <text x={n.x} y={n.y + n.r + 10}
                textAnchor="middle" fontSize="9"
                fontFamily="Inter"
                fill={n.cur ? 'var(--fg)' : 'var(--fg-muted)'}>
                {n.label}
              </text>
            </g>
          ))}
        </svg>
      </div>
      <div className="ol-section" style={{ marginTop: 4 }}><span>Properties</span></div>
      <div className="props">
        <div className="row2"><span className="k">kind</span><span className="v">{PROPS.kind}</span></div>
        <div className="row2"><span className="k">forge</span><span className="v">{PROPS.forge}</span></div>
        <div className="row2"><span className="k">updated</span><span className="v">{PROPS.updated}</span></div>
        <div className="row2"><span className="k">created</span><span className="v">{PROPS.created}</span></div>
        <div className="row2"><span className="k">words</span><span className="v">{PROPS.words}</span></div>
        <div className="row2"><span className="k">links</span><span className="v">{PROPS.links}</span></div>
        <div className="row2"><span className="k">tags</span>
          <span className="v tag">
            {PROPS.tags.map(t => <span className="tagpill" key={t}>{t}</span>)}
          </span>
        </div>
      </div>
    </div>
  );
}

function RightPanel({ activeHeading, onJump }) {
  const [tab, setTab] = useState('outline');
  return (
    <div className="rightpanel">
      <div className="panel-head">
        <span>Inspector</span>
        <div className="actions">
          <button className="icon-btn" title="Pin"><Ic.star /></button>
          <button className="icon-btn" title="Hide"><Ic.x /></button>
        </div>
      </div>
      <div className="rtabs">
        <div className={'rtab ' + (tab==='outline'?'active':'')} onClick={() => setTab('outline')}>
          Outline <span className="n">{OUTLINE.length}</span>
        </div>
        <div className={'rtab ' + (tab==='backlinks'?'active':'')} onClick={() => setTab('backlinks')}>
          Backlinks <span className="n">{BACKLINKS.length}</span>
        </div>
        <div className={'rtab ' + (tab==='graph'?'active':'')} onClick={() => setTab('graph')}>
          Graph
        </div>
      </div>
      <div className="rpanes">
        {tab==='outline' && <Outline activeId={activeHeading} onJump={onJump} />}
        {tab==='backlinks' && <Backlinks />}
        {tab==='graph' && <GraphPane />}
      </div>
    </div>
  );
}

Object.assign(window, { ActivityRail, LeftPanel, RightPanel });
