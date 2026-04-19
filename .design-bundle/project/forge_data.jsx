/* Nexus Forge — synthetic data: tree, files, outline, backlinks, palette. */

const TREE = [
  { id: 'clippings', name: 'Clippings', type: 'folder', children: [
    { id: 'c1', name: 'Microkernel essays.md', type: 'file' },
    { id: 'c2', name: 'CRDT landscape.md', type: 'file' },
    { id: 'c3', name: 'Tantivy vs Meilisearch.md', type: 'file', dot: 'cool' },
  ]},
  { id: 'dev_setup', name: 'Dev_setup', type: 'folder', children: [
    { id: 'd1', name: 'macOS toolchain.md', type: 'file' },
    { id: 'd2', name: 'Windows + WSL.md', type: 'file' },
  ]},
  { id: 'dev_tracker', name: 'Dev_tracker', type: 'folder', children: [
    { id: 't1', name: 'PRD-09 punch list.md', type: 'file', dot: 'warn' },
    { id: 't2', name: 'Plugin store roadmap.md', type: 'file' },
  ]},
  { id: 'dto_work', name: 'DTO_Work', type: 'folder', children: [
    { id: 'dto1', name: 'bindings/ audit.md', type: 'file' },
  ]},
  { id: 'home_lab', name: 'Home_lab', type: 'folder', children: [
    { id: 'h1', name: 'Server budget.md', type: 'file' },
  ]},
  { id: 'nexus_work', name: 'Nexus_Work', type: 'folder', open: true, children: [
    { id: 'backlog', name: 'Backlog', type: 'folder', open: true, children: [
      { id: 'b1', name: 'Backlog-Completed', type: 'file' },
      { id: 'b2', name: 'Backlog-Current', type: 'file', dot: 'warn' },
      { id: 'b3', name: 'Nexus Feature Backlog', type: 'file' },
    ]},
    { id: 'fullset', name: 'Fullset', type: 'file' },
    { id: 'impl', name: 'Nexus Implementation Status', type: 'file', active: true, dot: 'ember' },
    { id: 'trk', name: 'Nexus Tracking', type: 'file' },
    { id: 'recap', name: 'Recap', type: 'file' },
  ]},
  { id: 'repowork', name: 'RepoWork', type: 'folder', children: [
    { id: 'r1', name: 'migration notes.md', type: 'file' },
  ]},
  { id: 'claude_proxies', name: 'Claude Proxies', type: 'folder', children: [] },
  { id: 'iron_bank', name: 'Iron Bank', type: 'folder', children: [
    { id: 'ib1', name: 'wasm security review.md', type: 'file', dot: 'risk' },
  ]},
  { id: 'welcome', name: 'Welcome.md', type: 'file' },
];

const TABS = [
  { id: 'impl', file: 'Nexus Implementation Status', dirty: false, icon: 'doc' },
  { id: 'b2',   file: 'Backlog-Current', dirty: true, icon: 'doc' },
  { id: 'ib1',  file: 'wasm security review', dirty: false, icon: 'doc' },
];

const OUTLINE = [
  { id: 'h-legend',  lvl: 2, n: '01', t: 'Legend', size: '187w' },
  { id: 'h-summary', lvl: 2, n: '02', t: 'Summary', size: '948w' },
  { id: 'h-per',     lvl: 2, n: '03', t: 'Per-PRD detail', size: '6.1k' },
  { id: 'h-p01',  lvl: 3, n: '', t: 'PRD-01 · Kernel & Event System', size: '' },
  { id: 'h-p02',  lvl: 3, n: '', t: 'PRD-02 · Security Model', size: '' },
  { id: 'h-p03',  lvl: 3, n: '', t: 'PRD-03 · Storage Engine', size: '' },
  { id: 'h-p04',  lvl: 3, n: '', t: 'PRD-04 · Plugin System', size: '' },
  { id: 'h-p08',  lvl: 3, n: '', t: 'PRD-08 · Editor Engine', size: '' },
  { id: 'h-p09',  lvl: 3, n: '', t: 'PRD-09 · Terminal & Process Mgr', size: '' },
  { id: 'h-p10',  lvl: 3, n: '', t: 'PRD-10 · Database Engine', size: '' },
  { id: 'h-p12',  lvl: 3, n: '', t: 'PRD-12 · AI Engine', size: '' },
  { id: 'h-cross', lvl: 2, n: '04', t: 'Cross-cutting observations', size: '540w' },
  { id: 'h-risk', lvl: 2, n: '05', t: 'Risk hotspots', size: '310w' },
  { id: 'h-honest', lvl: 2, n: '06', t: 'How to keep this doc honest', size: '410w' },
];

const BACKLINKS = [
  { file: 'Backlog-Current.md', ctx: 'See <mark>[[Nexus Implementation Status]]</mark> for the current tier per PRD before opening a new task.', time: '2h' },
  { file: 'docs/UI-AUDIT.md',   ctx: 'The audit findings must be reflected in <mark>[[Nexus Implementation Status]]</mark> — specifically the PRD-07 and PRD-08 rows.', time: '1d' },
  { file: 'MICROKERNEL-AUDIT.md', ctx: 'Kernel tier should match what <mark>[[Nexus Implementation Status]]</mark> says for PRD-01; currently aligned.', time: '3d' },
  { file: 'Nexus Feature Backlog.md', ctx: 'Do not duplicate content — link to <mark>[[Nexus Implementation Status]]</mark>.', time: '5d' },
  { file: 'Recap.md', ctx: 'Weekly recap pulls freshness from the PRD tiers in <mark>[[Nexus Implementation Status]]</mark>.', time: '1w' },
];

const PROPS = {
  kind: 'md',
  tags: ['#status', '#prd', '#roadmap'],
  created: '2025-11-04',
  updated: '2026-04-17',
  words: '1,823',
  links: '48 out · 12 in',
  forge: 'Nexus_Work',
};

const PALETTE = [
  { sec: 'Files', kind: 'file', val: 'impl', t: 'Nexus Implementation Status', s: 'Nexus_Work/', icon: '📄' },
  { sec: 'Files', kind: 'file', val: 'b2',   t: 'Backlog-Current',             s: 'Backlog/',     icon: '📄' },
  { sec: 'Files', kind: 'file', val: 'ib1',  t: 'wasm security review',        s: 'Iron Bank/',   icon: '📄' },
  { sec: 'Files', kind: 'file', val: 'trk',  t: 'Nexus Tracking',              s: 'Nexus_Work/',  icon: '📄' },
  { sec: 'Commands', kind: 'cmd', val: 'toggleTheme', t: 'Toggle theme',      s: '⌘⇧T', icon: '◐' },
  { sec: 'Commands', kind: 'cmd', val: 'toggleBacklinks', t: 'Toggle backlinks drawer', s: '⌘B', icon: '⇅' },
  { sec: 'Commands', kind: 'cmd', val: 'tweaks', t: 'Open Tweaks',            s: '⌘,',  icon: '⚙' },
  { sec: 'Graph', kind: 'cmd', val: 'graph', t: 'Open local graph',           s: '⌘G',  icon: '◍' },
];

// Expose to other script files
Object.assign(window, { TREE, TABS, OUTLINE, BACKLINKS, PROPS, PALETTE });

// Palette data for plain-JS palette renderer
window.__paletteItems = PALETTE;
