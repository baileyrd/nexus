/**
 * AI defaults — mirror of `crates/nexus-formats/src/config/ai.rs`.
 *
 * Keep in sync with the Rust-side `DEFAULT_*` constants. Drift is
 * intentional only when the shell exposes a feature the backend has
 * not yet learned about; in every other case the shell should defer
 * (blank schema default) and let the kernel resolve via these values.
 */

/** Default provider name for new forges. */
export const DEFAULT_PROVIDER = 'anthropic'

/**
 * Default chat model when neither `ai.toml` nor the user-set
 * `ai.model` schema entry resolves a value.
 */
export const DEFAULT_MODEL = 'claude-sonnet-4-6'

/**
 * Default env-var pulled when `ai.toml` does not bind an `apiKey`.
 */
export const DEFAULT_API_KEY_ENV = 'ANTHROPIC_API_KEY'

/** Default `max_tokens` ceiling on generation responses. */
export const DEFAULT_MAX_TOKENS = 4096

/** Default sampling temperature. */
export const DEFAULT_TEMPERATURE = 0.7
