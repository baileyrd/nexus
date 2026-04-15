import { type ChangeEvent } from "react";
import type { JsonSchema } from "../../ipc/pluginSettings";

/**
 * Minimal JSON-Schema form renderer for per-plugin settings.
 *
 * Supports a single-level object schema with these field types:
 *   - `string`                    → text input
 *   - `string` + `enum`           → select
 *   - `boolean`                   → checkbox
 *   - `integer` / `number`        → number input (with min/max)
 *
 * Nested objects, arrays, refs, format/pattern, oneOf/anyOf and most
 * other JSON Schema keywords are intentionally unsupported — the goal
 * is covering the 80% of plugin-settings cases, not reimplementing a
 * full schema validator on the frontend. Backend validation (via the
 * existing `SettingsManager`) is the source of truth.
 */

interface SettingsSchemaFormProps {
  schema: JsonSchema;
  values: Record<string, unknown>;
  onChange: (patch: Record<string, unknown>) => void;
}

interface PropertySchema {
  type?: string | string[];
  title?: string;
  description?: string;
  default?: unknown;
  enum?: unknown[];
  minimum?: number;
  maximum?: number;
}

export function SettingsSchemaForm({ schema, values, onChange }: SettingsSchemaFormProps) {
  const properties = (schema.properties as Record<string, PropertySchema> | undefined) ?? {};
  const required = (schema.required as string[] | undefined) ?? [];
  const entries = Object.entries(properties);

  if (entries.length === 0) {
    return (
      <p className="settings-empty">
        This plugin's settings schema declares no fields.
      </p>
    );
  }

  return (
    <div className="plugin-settings-form">
      {entries.map(([name, prop]) => (
        <SchemaField
          key={name}
          name={name}
          prop={prop}
          value={values[name]}
          isRequired={required.includes(name)}
          onChange={(next) => onChange({ ...values, [name]: next })}
        />
      ))}
    </div>
  );
}

interface SchemaFieldProps {
  name: string;
  prop: PropertySchema;
  value: unknown;
  isRequired: boolean;
  onChange: (next: unknown) => void;
}

function SchemaField({ name, prop, value, isRequired, onChange }: SchemaFieldProps) {
  const label = prop.title ?? name;
  const description = prop.description;
  const type = Array.isArray(prop.type) ? prop.type[0] : prop.type;

  return (
    <label className="plugin-settings-field">
      <div className="plugin-settings-field-head">
        <span className="plugin-settings-field-label">
          {label}
          {isRequired && <span className="plugin-settings-required"> *</span>}
        </span>
        {description && (
          <span className="plugin-settings-field-desc">{description}</span>
        )}
      </div>
      <FieldInput
        name={name}
        type={type}
        prop={prop}
        value={value}
        onChange={onChange}
      />
    </label>
  );
}

interface FieldInputProps {
  name: string;
  type: string | undefined;
  prop: PropertySchema;
  value: unknown;
  onChange: (next: unknown) => void;
}

function FieldInput({ name, type, prop, value, onChange }: FieldInputProps) {
  if (prop.enum && Array.isArray(prop.enum)) {
    return (
      <select
        className="plugin-settings-input"
        value={stringifyScalar(value, prop.default)}
        onChange={(e: ChangeEvent<HTMLSelectElement>) => onChange(e.target.value)}
      >
        {prop.enum.map((opt) => (
          <option key={String(opt)} value={String(opt)}>
            {String(opt)}
          </option>
        ))}
      </select>
    );
  }

  if (type === "boolean") {
    const checked = typeof value === "boolean" ? value : Boolean(prop.default);
    return (
      <input
        type="checkbox"
        className="plugin-settings-toggle"
        checked={checked}
        onChange={(e) => onChange(e.target.checked)}
      />
    );
  }

  if (type === "integer" || type === "number") {
    const current = typeof value === "number" ? value : Number(prop.default ?? 0);
    return (
      <input
        type="number"
        className="plugin-settings-input"
        value={Number.isFinite(current) ? current : ""}
        min={prop.minimum}
        max={prop.maximum}
        step={type === "integer" ? 1 : undefined}
        onChange={(e) => {
          const raw = e.target.value;
          if (raw === "") {
            onChange(undefined);
            return;
          }
          const parsed = type === "integer" ? parseInt(raw, 10) : parseFloat(raw);
          onChange(Number.isNaN(parsed) ? undefined : parsed);
        }}
      />
    );
  }

  // Default: string input.
  return (
    <input
      type="text"
      className="plugin-settings-input"
      value={stringifyScalar(value, prop.default)}
      name={name}
      onChange={(e) => onChange(e.target.value)}
    />
  );
}

function stringifyScalar(value: unknown, fallback: unknown): string {
  if (typeof value === "string") return value;
  if (typeof value === "number" || typeof value === "boolean") return String(value);
  if (typeof fallback === "string") return fallback;
  if (typeof fallback === "number" || typeof fallback === "boolean") return String(fallback);
  return "";
}
