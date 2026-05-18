# Settings schemas

A plugin declares its settings as a JSON schema. The shell auto-
generates a settings UI from the schema; values are persisted to
`<forge>/.forge/plugins/<id>/settings.json` and exposed to your code
through `ctx.config`.

## Declare in the manifest

```json
"contributes": {
  "settings": {
    "schema": [
      {
        "key": "hello.greeting",
        "title": "Greeting",
        "description": "Word used in the toast.",
        "type": "string",
        "default": "Hello"
      },
      {
        "key": "hello.exclamation",
        "title": "Add an exclamation mark",
        "type": "boolean",
        "default": true
      },
      {
        "key": "hello.repeat",
        "title": "Repeat count",
        "description": "How many times to greet.",
        "type": "number",
        "default": 1,
        "min": 1,
        "max": 10
      },
      {
        "key": "hello.style",
        "title": "Greeting style",
        "type": "string",
        "default": "casual",
        "options": ["casual", "formal", "shouted"]
      }
    ]
  }
}
```

The schema is an array of `ConfigSection` entries. Each entry
contributes one row in the auto-generated settings UI under
**Settings → Plugins → Hello**.

## Field reference

Every entry has these common fields:

| Field | Type | Required | Meaning |
|---|---|---|---|
| `key` | string | yes | Dotted key. Convention: `<plugin-namespace>.<setting>`. Used both as the storage key and the UI section path. |
| `title` | string | yes | Label shown in the UI. |
| `description` | string | no | Help text shown beneath the control. |
| `type` | string | yes | One of `string`, `number`, `boolean`, `password`, `multiline`. |
| `default` | matches type | yes | Initial value if the user has never edited it. |

Per-type extras:

| Type | Extras | Renders as |
|---|---|---|
| `string` | `options?: string[]`, `pattern?: string` | Text input; dropdown if `options`. |
| `number` | `min?: number`, `max?: number`, `step?: number` | Number input or slider. |
| `boolean` | — | Toggle switch. |
| `password` | — | Masked input; value redacted in logs. |
| `multiline` | `rows?: number` | `<textarea>`. |

Unknown fields on an entry are ignored (allowing forward-compatible
additions).

## Reading values

```ts
activate(ctx: PluginContext) {
  const greeting = ctx.config.get<string>('hello.greeting', 'Hello');
  const repeat   = ctx.config.get<number>('hello.repeat', 1);

  ctx.commands.register({
    id: 'hello.sayHi',
    run: async () => {
      for (let i = 0; i < repeat; i++) {
        ctx.ui.notify({ message: greeting });
      }
    },
  });
}
```

`ctx.config.get(key, fallback)` returns the current value (cached in
memory). Pass a fallback so you don't crash on a missing key during
upgrades.

## Reacting to changes

Subscribe for live updates:

```ts
const off = ctx.config.onChange((changed: string[]) => {
  if (changed.includes('hello.greeting')) {
    cachedGreeting = ctx.config.get<string>('hello.greeting', 'Hello');
  }
});
```

`onChange` fires once per save (the user clicks Apply, or another
plugin writes via `ctx.config.set`). Multiple changes coalesce into
one delivery; the array tells you which keys moved.

## Writing values

```ts
await ctx.config.set('hello.greeting', 'Howdy');
```

Writing fires `onChange` for subscribers (including yourself). Use
sparingly — settings are user-facing, and a plugin that overwrites
them surprises people.

## Secrets

Use `type: "password"` for API keys. Values are masked in the UI and
redacted from logs. They are **not** stored in the OS keyring (only
the AI plugin's keys are) — they live in `settings.json`. If a user
needs higher-grade secret storage, recommend an environment variable:

```json
{
  "key": "myplugin.apiKey",
  "title": "API key",
  "description": "Set via environment variable MYPLUGIN_API_KEY for keyring-less storage.",
  "type": "password",
  "default": ""
}
```

```ts
const key = ctx.config.get<string>('myplugin.apiKey', '')
         || ctx.env.get('MYPLUGIN_API_KEY')
         || '';
```

## Validation

Built-in validation runs on save:

- `pattern` (string): must match the regex.
- `min` / `max` (number): must be in range.
- `options` (string): must be one of the listed values.

Custom validation belongs in your `onChange` handler — if you decide
the new value is invalid, `ctx.config.set` it back to a sane default
and notify the user.

## Defaults vs. user values

The user only sees a value as "edited" once they explicitly change
it. Reading via `ctx.config.get(key, fallback)` returns the user
value if set, the schema default otherwise, and your fallback if
neither exists.

The `default` in the schema is the **canonical** default. Don't put
user-specific values there.

## Where they live

```
<forge>/.forge/plugins/com.example.hello/settings.json
```

Plain JSON, editable by hand if you want. The shell reloads it on
file-watcher tick.

## See also

- [Manifest](manifest.md) — how `contributes.settings` slots in.
- [`../../help/customize/settings.md`](../../help/customize/settings.md)
  — user-facing settings overview.
