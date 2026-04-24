/**
 * OI-01 — re-export wrapper so `pnpm test` picks up the
 * SettingsTabRegistry tests that live as a sibling of the implementation.
 * Mirrors `tests/command-registry.test.ts`.
 */
import '../src/registry/SettingsTabRegistry.test.ts'
