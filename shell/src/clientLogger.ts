// Public re-export of clientLogger for use by plugin files.
// Plugins may not import from shell/src/host/* (see plugin-import-hygiene
// test), so this thin re-export lives outside that boundary.
export { clientLogger, type LogEntry, type LogLevel } from './host/clientLogger'
