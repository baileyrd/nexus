// Tests live colocated next to the source they cover, but the
// `shell/package.json` test runner only scans `tests/*.test.ts`.
// Re-export so they actually run:
//
// `shell/src/plugins/nexus/editor/cm/replFence.test.ts`.

import '../src/plugins/nexus/editor/cm/replFence.test.ts'
