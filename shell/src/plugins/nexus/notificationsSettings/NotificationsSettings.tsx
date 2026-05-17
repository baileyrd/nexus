// BL-133 follow-up — Notifications settings tab.
//
// Surfaces per-channel credential entry (Discord webhook, Telegram bot
// token + chat id, SMTP host/port/user/password) backed by the
// nexus-security keyring. Each block has a "Send test" button that
// dispatches `com.nexus.notifications::send` with the channel routed
// directly so the user can verify the round-trip without waiting for
// a real producer event.
//
// Scope: this tab does not edit `<forge>/.forge/notifications.toml`
// directly. The credentials it manages live in the OS keyring; channel
// routing (which producer→which channel) stays a TOML concern.

import { useCallback, useEffect, useState } from 'react'
import { getNotificationsSettingsApi } from './notificationsSettingsRuntime'
import { clientLogger } from '../../../clientLogger'

const SECURITY_PLUGIN_ID = 'com.nexus.security'
const NOTIFICATIONS_PLUGIN_ID = 'com.nexus.notifications'

// Plugin id used as the keyring namespace. Every credential the panel
// writes shares this prefix so deleting the namespace cleans up cleanly.
const KEYRING_PLUGIN_ID = 'nexus.notificationsSettings'

const KEY_DISCORD_WEBHOOK = 'discord_webhook'
const KEY_TELEGRAM_BOT_TOKEN = 'telegram_bot_token'
const KEY_TELEGRAM_CHAT_ID = 'telegram_chat_id'
const KEY_SMTP_HOST = 'smtp_host'
const KEY_SMTP_PORT = 'smtp_port'
const KEY_SMTP_USERNAME = 'smtp_username'
const KEY_SMTP_PASSWORD = 'smtp_password'
const KEY_SMTP_TO = 'smtp_to'

const SECRET_KEYS = [
  KEY_DISCORD_WEBHOOK,
  KEY_TELEGRAM_BOT_TOKEN,
  KEY_TELEGRAM_CHAT_ID,
  KEY_SMTP_HOST,
  KEY_SMTP_PORT,
  KEY_SMTP_USERNAME,
  KEY_SMTP_PASSWORD,
  KEY_SMTP_TO,
] as const

interface ListNamesResult {
  names: string[]
}

interface SetSecretResult {
  ok: boolean
}

interface ChannelStatus {
  configured: boolean
  busy: boolean
  testResult?: 'ok' | 'error'
  testMessage?: string
}

type ChannelKey = 'discord' | 'telegram' | 'email'

const INITIAL_STATUS: Record<ChannelKey, ChannelStatus> = {
  discord: { configured: false, busy: false },
  telegram: { configured: false, busy: false },
  email: { configured: false, busy: false },
}

async function listConfiguredKeys(): Promise<Set<string>> {
  const api = getNotificationsSettingsApi()
  try {
    const result = await api.kernel.invoke<ListNamesResult>(
      SECURITY_PLUGIN_ID,
      'list_secret_names',
      { plugin_id: KEYRING_PLUGIN_ID },
    )
    return new Set(result.names ?? [])
  } catch (err) {
    clientLogger.warn('[nexus.notificationsSettings] list_secret_names failed:', err)
    return new Set()
  }
}

async function writeSecret(name: string, value: string): Promise<boolean> {
  const api = getNotificationsSettingsApi()
  const trimmed = value.trim()
  if (!trimmed) {
    // Empty string → delete the stored credential.
    try {
      await api.kernel.invoke(SECURITY_PLUGIN_ID, 'delete_secret', {
        plugin_id: KEYRING_PLUGIN_ID,
        name,
      })
      return true
    } catch (err) {
      clientLogger.warn(`[nexus.notificationsSettings] delete ${name} failed:`, err)
      return false
    }
  }
  try {
    const result = await api.kernel.invoke<SetSecretResult>(
      SECURITY_PLUGIN_ID,
      'set_secret',
      { plugin_id: KEYRING_PLUGIN_ID, name, value: trimmed },
    )
    return result.ok
  } catch (err) {
    clientLogger.warn(`[nexus.notificationsSettings] set ${name} failed:`, err)
    return false
  }
}

async function sendTest(channel: ChannelKey): Promise<ChannelStatus['testResult'] extends infer R ? R : never> {
  const api = getNotificationsSettingsApi()
  try {
    await api.kernel.invoke(NOTIFICATIONS_PLUGIN_ID, 'send', {
      source: 'settings',
      title: 'Notifications test',
      message: `Test from Settings → Notifications (channel: ${channel}).`,
      channel,
    })
    return 'ok'
  } catch (err) {
    clientLogger.warn(`[nexus.notificationsSettings] test ${channel} failed:`, err)
    return 'error'
  }
}

interface ChannelFieldProps {
  label: string
  fieldKey: string
  value: string
  onChange: (v: string) => void
  type?: 'text' | 'password'
  placeholder?: string
  configured: boolean
}

function ChannelField({
  label,
  fieldKey,
  value,
  onChange,
  type = 'text',
  placeholder,
  configured,
}: ChannelFieldProps) {
  return (
    <label
      key={fieldKey}
      style={{ display: 'flex', flexDirection: 'column', gap: 4, marginBottom: 8 }}
    >
      <span style={{ fontSize: '0.85em', color: 'var(--nexus-color-muted)' }}>
        {label}
        {configured ? (
          <em style={{ marginLeft: 8, color: 'var(--nexus-color-success)' }}>
            (saved)
          </em>
        ) : null}
      </span>
      <input
        type={type}
        value={value}
        placeholder={placeholder}
        onChange={(e) => onChange(e.currentTarget.value)}
        style={{
          padding: '0.3rem 0.5rem',
          border: '1px solid var(--nexus-color-border)',
          borderRadius: 4,
          background: 'var(--nexus-color-bg-elevated)',
          color: 'var(--nexus-color-fg)',
        }}
      />
    </label>
  )
}

interface ChannelBlockProps {
  title: string
  description: string
  status: ChannelStatus
  children: React.ReactNode
  onSave: () => void
  onTest: () => void
  saveDisabled?: boolean
  testDisabled?: boolean
}

function ChannelBlock({
  title,
  description,
  status,
  children,
  onSave,
  onTest,
  saveDisabled,
  testDisabled,
}: ChannelBlockProps) {
  return (
    <section
      style={{
        border: '1px solid var(--nexus-color-border)',
        borderRadius: 6,
        padding: '0.75rem 1rem',
        marginBottom: '1rem',
      }}
    >
      <h4 style={{ margin: '0 0 0.25rem' }}>{title}</h4>
      <p
        className="settings-help"
        style={{ marginTop: 0, marginBottom: '0.75rem', fontSize: '0.85em' }}
      >
        {description}
      </p>
      {children}
      <div style={{ display: 'flex', gap: 8, alignItems: 'center', marginTop: 8 }}>
        <button type="button" onClick={onSave} disabled={status.busy || saveDisabled}>
          Save
        </button>
        <button
          type="button"
          onClick={onTest}
          disabled={status.busy || testDisabled || !status.configured}
        >
          Send test
        </button>
        {status.testResult === 'ok' ? (
          <span style={{ color: 'var(--nexus-color-success)', fontSize: '0.85em' }}>
            Test dispatched.
          </span>
        ) : null}
        {status.testResult === 'error' ? (
          <span style={{ color: 'var(--nexus-color-danger)', fontSize: '0.85em' }}>
            {status.testMessage ?? 'Test failed.'}
          </span>
        ) : null}
      </div>
    </section>
  )
}

export function NotificationsSettings() {
  const [configuredKeys, setConfiguredKeys] = useState<Set<string>>(new Set())
  const [status, setStatus] = useState<Record<ChannelKey, ChannelStatus>>(INITIAL_STATUS)

  const [discordWebhook, setDiscordWebhook] = useState('')
  const [telegramBotToken, setTelegramBotToken] = useState('')
  const [telegramChatId, setTelegramChatId] = useState('')
  const [smtpHost, setSmtpHost] = useState('')
  const [smtpPort, setSmtpPort] = useState('')
  const [smtpUsername, setSmtpUsername] = useState('')
  const [smtpPassword, setSmtpPassword] = useState('')
  const [smtpTo, setSmtpTo] = useState('')

  const refresh = useCallback(async () => {
    const keys = await listConfiguredKeys()
    setConfiguredKeys(keys)
    setStatus((prev) => ({
      discord: {
        ...prev.discord,
        configured: keys.has(KEY_DISCORD_WEBHOOK),
      },
      telegram: {
        ...prev.telegram,
        configured:
          keys.has(KEY_TELEGRAM_BOT_TOKEN) && keys.has(KEY_TELEGRAM_CHAT_ID),
      },
      email: {
        ...prev.email,
        configured: keys.has(KEY_SMTP_HOST) && keys.has(KEY_SMTP_TO),
      },
    }))
  }, [])

  useEffect(() => {
    void refresh()
  }, [refresh])

  const saveDiscord = useCallback(async () => {
    setStatus((s) => ({ ...s, discord: { ...s.discord, busy: true } }))
    await writeSecret(KEY_DISCORD_WEBHOOK, discordWebhook)
    setDiscordWebhook('')
    await refresh()
    setStatus((s) => ({ ...s, discord: { ...s.discord, busy: false } }))
  }, [discordWebhook, refresh])

  const saveTelegram = useCallback(async () => {
    setStatus((s) => ({ ...s, telegram: { ...s.telegram, busy: true } }))
    await writeSecret(KEY_TELEGRAM_BOT_TOKEN, telegramBotToken)
    await writeSecret(KEY_TELEGRAM_CHAT_ID, telegramChatId)
    setTelegramBotToken('')
    setTelegramChatId('')
    await refresh()
    setStatus((s) => ({ ...s, telegram: { ...s.telegram, busy: false } }))
  }, [telegramBotToken, telegramChatId, refresh])

  const saveEmail = useCallback(async () => {
    setStatus((s) => ({ ...s, email: { ...s.email, busy: true } }))
    await writeSecret(KEY_SMTP_HOST, smtpHost)
    await writeSecret(KEY_SMTP_PORT, smtpPort)
    await writeSecret(KEY_SMTP_USERNAME, smtpUsername)
    await writeSecret(KEY_SMTP_PASSWORD, smtpPassword)
    await writeSecret(KEY_SMTP_TO, smtpTo)
    setSmtpHost('')
    setSmtpPort('')
    setSmtpUsername('')
    setSmtpPassword('')
    setSmtpTo('')
    await refresh()
    setStatus((s) => ({ ...s, email: { ...s.email, busy: false } }))
  }, [smtpHost, smtpPort, smtpUsername, smtpPassword, smtpTo, refresh])

  const runTest = useCallback(async (channel: ChannelKey) => {
    setStatus((s) => ({
      ...s,
      [channel]: { ...s[channel], busy: true, testResult: undefined },
    }))
    const result = await sendTest(channel)
    setStatus((s) => ({
      ...s,
      [channel]: {
        ...s[channel],
        busy: false,
        testResult: result,
        testMessage:
          result === 'error'
            ? `notifications::send rejected (check ${channel} config + creds)`
            : undefined,
      },
    }))
  }, [])

  return (
    <div className="notifications-settings">
      <h3 style={{ marginTop: 0 }}>Notifications</h3>
      <p className="settings-help" style={{ marginBottom: '1rem' }}>
        Credentials are stored in the OS keyring via{' '}
        <code>com.nexus.security</code>. Channel routing (which event source
        publishes to which channel) lives in{' '}
        <code>{`<forge>/.forge/notifications.toml`}</code> — edit it directly
        for now.
      </p>

      <ChannelBlock
        title="Discord"
        description="Webhook URL from a channel's Integrations → Webhooks panel."
        status={status.discord}
        onSave={saveDiscord}
        onTest={() => runTest('discord')}
        saveDisabled={!discordWebhook.trim()}
      >
        <ChannelField
          label="Webhook URL"
          fieldKey={KEY_DISCORD_WEBHOOK}
          value={discordWebhook}
          onChange={setDiscordWebhook}
          placeholder="https://discord.com/api/webhooks/..."
          type="password"
          configured={configuredKeys.has(KEY_DISCORD_WEBHOOK)}
        />
      </ChannelBlock>

      <ChannelBlock
        title="Telegram"
        description="Bot token from @BotFather and the chat id to deliver to."
        status={status.telegram}
        onSave={saveTelegram}
        onTest={() => runTest('telegram')}
        saveDisabled={
          !telegramBotToken.trim() && !telegramChatId.trim()
        }
      >
        <ChannelField
          label="Bot token"
          fieldKey={KEY_TELEGRAM_BOT_TOKEN}
          value={telegramBotToken}
          onChange={setTelegramBotToken}
          type="password"
          placeholder="123456:ABC-DEF..."
          configured={configuredKeys.has(KEY_TELEGRAM_BOT_TOKEN)}
        />
        <ChannelField
          label="Chat id"
          fieldKey={KEY_TELEGRAM_CHAT_ID}
          value={telegramChatId}
          onChange={setTelegramChatId}
          placeholder="@yourhandle or numeric chat id"
          configured={configuredKeys.has(KEY_TELEGRAM_CHAT_ID)}
        />
      </ChannelBlock>

      <ChannelBlock
        title="Email (SMTP)"
        description="SMTP server credentials plus the address to deliver alerts to."
        status={status.email}
        onSave={saveEmail}
        onTest={() => runTest('email')}
        saveDisabled={!smtpHost.trim() && !smtpTo.trim()}
      >
        <ChannelField
          label="SMTP host"
          fieldKey={KEY_SMTP_HOST}
          value={smtpHost}
          onChange={setSmtpHost}
          placeholder="smtp.example.com"
          configured={configuredKeys.has(KEY_SMTP_HOST)}
        />
        <ChannelField
          label="Port"
          fieldKey={KEY_SMTP_PORT}
          value={smtpPort}
          onChange={setSmtpPort}
          placeholder="587"
          configured={configuredKeys.has(KEY_SMTP_PORT)}
        />
        <ChannelField
          label="Username"
          fieldKey={KEY_SMTP_USERNAME}
          value={smtpUsername}
          onChange={setSmtpUsername}
          configured={configuredKeys.has(KEY_SMTP_USERNAME)}
        />
        <ChannelField
          label="Password"
          fieldKey={KEY_SMTP_PASSWORD}
          value={smtpPassword}
          onChange={setSmtpPassword}
          type="password"
          configured={configuredKeys.has(KEY_SMTP_PASSWORD)}
        />
        <ChannelField
          label="Deliver to"
          fieldKey={KEY_SMTP_TO}
          value={smtpTo}
          onChange={setSmtpTo}
          placeholder="you@example.com"
          configured={configuredKeys.has(KEY_SMTP_TO)}
        />
      </ChannelBlock>
    </div>
  )
}

// Re-export the key list for tests / consumers that need to clear
// credentials in bulk.
export const NOTIFICATIONS_SECRET_KEYS: readonly string[] = SECRET_KEYS
