/**
 * BL-096 follow-up — re-export wrapper so `pnpm test` picks up the
 * applyCapabilityChange unit tests living next to the implementation.
 */
import '../src/plugins/core/capabilityPrompt/applyCapabilityChange.test.ts'
