// Graph section for the Note Context accordion.
//
// Per the Phase 4.3 decision, `nexus.graph` stays as a standalone
// plugin (some users dock the graph in its own tab). This section is
// a thin reuse of its `GraphView` component: nexus.graph's activate
// hook owns the kernel subscription + `useGraphStore` writes, and
// `GraphView` renders from the store.
//
// Because noteContext declares `nexus.graph` in its `dependsOn`, the
// extension host activates the standalone graph plugin first; by the
// time this section mounts the store is already being populated by
// the active-file subscriber there. When the section is collapsed
// the React tree unmounts but the subscriber stays running — that's
// "soft" lazy for this one section, the price of zero duplication.
// If the cost of the standalone subscriber becomes meaningful, the
// fix is to lift the load function into a shared helper both surfaces
// import (a follow-up; the load logic is one screen of code).

import { GraphView } from '../../graph/GraphView'

export function GraphSection() {
  return (
    <div style={{ height: 280, position: 'relative' }}>
      <GraphView />
    </div>
  )
}
