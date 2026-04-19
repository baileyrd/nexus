/* Nexus Forge — root App */
const { useState: useStateA, useEffect: useEffectA } = React;

function TopBar({ onPalette, onTweaks, onBacklinks, backlinksOpen }) {
  return (
    <div className="topbar">
      <div className="cluster">
        <button className="icon-btn"><Ic.folder /></button>
        <button className="icon-btn" onClick={onPalette}><Ic.search /></button>
        <button className="icon-btn"><Ic.star /></button>
      </div>
      <div className="breadcrumb">
        <span className="sync" title="forge synced"></span>
        <span>Nexus_Work</span>
        <span style={{ color: 'var(--fg-dim)' }}>/</span>
        <b>Nexus PRD Implementation Status</b>
        <span style={{ marginLeft: 12, fontFamily: 'var(--f-mono)', color: 'var(--fg-dim)', fontSize: 10 }}>
          md · 1,823w
        </span>
      </div>
      <div className="win-controls">
        <button className="icon-btn" onClick={onTweaks} title="Tweaks"><Ic.sliders /></button>
        <button onClick={onBacklinks} className={'icon-btn ' + (backlinksOpen ? 'active' : '')} title="Backlinks drawer">
          <Ic.link />
        </button>
        <button className="icon-btn" title="Right panel"><Ic.panel /></button>
        <div style={{ width: 14 }} />
        <button className="icon-btn"><Ic.min /></button>
        <button className="icon-btn"><Ic.max /></button>
        <button className="icon-btn"><Ic.x /></button>
      </div>
    </div>
  );
}

function Tabs({ tabs, activeId, setActive, onClose }) {
  return (
    <div className="tabbar">
      {tabs.map(t => (
        <div key={t.id}
          className={'tab ' + (activeId === t.id ? 'active' : '')}
          onClick={() => setActive(t.id)}>
          <Ic.doc className="ficon" />
          <span className="tname">{t.file}</span>
          {t.dirty && <span className="dirty" />}
          <span className="x" onClick={(e) => { e.stopPropagation(); onClose(t.id); }}><Ic.x style={{ width: 10, height: 10 }} /></span>
        </div>
      ))}
      <div className="tab-plus" title="New tab"><Ic.plus style={{ width: 14, height: 14 }} /></div>
    </div>
  );
}

function StatusBar() {
  return (
    <div className="statusbar">
      <div className="left">
        <span className="sb"><span className="dot" /> Forge synced</span>
        <span className="sb">main · <code>3f1c8d2</code></span>
        <span className="sb">Tantivy · <code>12,480 docs</code></span>
        <span className="sb ember"><span className="dot" /> 3 plugins hot</span>
      </div>
      <div className="right">
        <span className="sb">ln 214, col 33</span>
        <span className="sb">MD · UTF-8</span>
        <span className="sb">1,823 words · 14,139 chars</span>
        <span className="sb">0 backlinks missing</span>
      </div>
    </div>
  );
}

function App() {
  const [tabs, setTabs] = useStateA(TABS);
  const [activeTab, setActiveTab] = useStateA('impl');
  const [pane, setPane] = useStateA('files');
  const [activeHeading, setActiveHeading] = useStateA('h-summary');

  // Cross-wire palette -> open file
  useEffectA(() => {
    window.__openFile = (id) => {
      const existing = tabs.find(t => t.id === id);
      if (existing) { setActiveTab(id); return; }
      const def = findNode(TREE, id);
      if (!def) return;
      setTabs(ts => [...ts, { id, file: def.name.replace('.md',''), dirty: false, icon: 'doc' }]);
      setActiveTab(id);
    };
  }, [tabs]);

  const onOpenNode = (n) => {
    if (n.type !== 'file') return;
    window.__openFile(n.id);
  };
  const closeTab = (id) => {
    setTabs(ts => {
      const idx = ts.findIndex(t => t.id === id);
      const next = ts.filter(t => t.id !== id);
      if (activeTab === id && next.length) {
        setActiveTab(next[Math.max(0, idx - 1)].id);
      }
      return next;
    });
  };

  const backlinksOpen = document.body.dataset.backlinks === 'open';

  return (
    <>
      <TopBar
        onPalette={() => window.openPalette(true)}
        onTweaks={() => window.toggleTweaks()}
        onBacklinks={() => {
          const now = document.body.dataset.backlinks === 'open' ? 'closed' : 'open';
          document.body.dataset.backlinks = now;
          if (parent) parent.postMessage({ type:'__edit_mode_set_keys', edits:{ backlinks: now } }, '*');
        }}
        backlinksOpen={backlinksOpen}
      />
      <div className="main" style={pane === 'terminal' || pane === 'ai' || pane === 'templates' ? { gridTemplateColumns: '44px 1fr' } : undefined}>
        <ActivityRail pane={pane} setPane={setPane} onTweaks={() => window.toggleTweaks()} />
        {pane === 'terminal' ? (
          <Processes />
        ) : pane === 'ai' ? (
          <Orchestrate />
        ) : pane === 'templates' ? (
          <Templates />
        ) : (<>
        <LeftPanel activeId={activeTab} onOpen={onOpenNode} />
        <div className="center">
          <Tabs tabs={tabs} activeId={activeTab} setActive={setActiveTab} onClose={closeTab} />
          <Doc onHeading={setActiveHeading} key={activeTab} />
          <div className="drawer">
            <div className="drawer-head">
              <span>Backlinks</span>
              <span style={{ color: 'var(--fg-dim)' }}>{BACKLINKS.length} linked · 0 unlinked</span>
              <button className="icon-btn close" onClick={() => {
                document.body.dataset.backlinks = 'closed';
                if (parent) parent.postMessage({ type:'__edit_mode_set_keys', edits:{ backlinks: 'closed' } }, '*');
              }}><Ic.x /></button>
            </div>
            <div style={{ overflowY: 'auto', padding: 8, display: 'grid', gap: 6 }}>
              {BACKLINKS.map((b,i) => (
                <div className="bl-item" key={i} style={{ background: 'var(--bg)' }}>
                  <div className="bl-file">
                    <Ic.doc />
                    <span>{b.file}</span>
                    <span style={{ marginLeft: 'auto', color: 'var(--fg-dim)', fontFamily: 'var(--f-mono)', fontSize: 10 }}>{b.time}</span>
                  </div>
                  <div className="bl-ctx" dangerouslySetInnerHTML={{ __html: b.ctx }} />
                </div>
              ))}
            </div>
          </div>
        </div>
        <RightPanel
          activeHeading={activeHeading}
          onJump={(id) => { setActiveHeading(id); window.__jumpHeading?.(id); }}
        />
        </>)}
      </div>
      <StatusBar />
    </>
  );
}

function findNode(list, id) {
  for (const n of list) {
    if (n.id === id) return n;
    if (n.children) {
      const r = findNode(n.children, id);
      if (r) return r;
    }
  }
  return null;
}

ReactDOM.createRoot(document.getElementById('root')).render(<App />);
