// Skills browser panel (PRD-13).
//
// Two-pane read-only view over `com.nexus.skills::list` / `::get`:
// left column lists every loaded skill; right column shows the
// selected skill's metadata + raw markdown body. "Reload" re-scans
// the forge's `.forge/skills/` tree through the plugin.

import { useCallback, useEffect, useMemo, useState } from "react";

import { skillsList, skillsReload, type Skill } from "../../ipc/skills";

export default function SkillsPanel() {
  const [skills, setSkills] = useState<Skill[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);

  const load = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      const list = await skillsList();
      setSkills(list);
      if (list.length > 0 && !selectedId) {
        setSelectedId(list[0].id);
      }
    } catch (err) {
      setError(String(err));
    } finally {
      setLoading(false);
    }
  }, [selectedId]);

  useEffect(() => {
    void load();
  }, []); // eslint-disable-line react-hooks/exhaustive-deps

  const reload = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      await skillsReload();
      const list = await skillsList();
      setSkills(list);
    } catch (err) {
      setError(String(err));
    } finally {
      setLoading(false);
    }
  }, []);

  const selected = useMemo(
    () => skills.find((s) => s.id === selectedId) ?? null,
    [skills, selectedId],
  );

  return (
    <div
      style={{
        display: "flex",
        height: "100%",
        fontSize: 13,
        color: "var(--color-fg)",
      }}
    >
      <div
        style={{
          width: 240,
          minWidth: 180,
          borderRight: "1px solid var(--color-border)",
          display: "flex",
          flexDirection: "column",
        }}
      >
        <div
          style={{
            padding: "6px 10px",
            borderBottom: "1px solid var(--color-border)",
            display: "flex",
            gap: 6,
            alignItems: "center",
          }}
        >
          <span style={{ flex: 1, opacity: 0.75 }}>
            Skills ({skills.length})
          </span>
          <button
            type="button"
            onClick={reload}
            disabled={loading}
            style={chipButtonStyle}
          >
            Reload
          </button>
        </div>
        <div style={{ overflowY: "auto", flex: 1 }}>
          {skills.length === 0 && !loading ? (
            <div style={{ padding: 10, opacity: 0.6 }}>
              No skills in <code>.forge/skills/</code>.
            </div>
          ) : null}
          {skills.map((s) => (
            <button
              key={s.id}
              type="button"
              onClick={() => setSelectedId(s.id)}
              aria-pressed={s.id === selectedId}
              style={{
                display: "block",
                width: "100%",
                textAlign: "left",
                padding: "6px 10px",
                background:
                  s.id === selectedId
                    ? "var(--color-accent-bg, rgba(255,255,255,0.05))"
                    : "transparent",
                border: "none",
                borderBottom: "1px solid var(--color-border)",
                color: "inherit",
                cursor: "pointer",
                fontSize: 13,
              }}
            >
              <div style={{ fontWeight: 500 }}>{s.name}</div>
              <div style={{ opacity: 0.6, fontSize: 11 }}>{s.id}</div>
            </button>
          ))}
        </div>
      </div>
      <div style={{ flex: 1, display: "flex", flexDirection: "column" }}>
        {error && (
          <div
            role="alert"
            style={{
              padding: "6px 10px",
              background: "var(--color-error-bg, #4a1f1f)",
              color: "var(--color-error-fg, #ffd)",
              fontSize: 12,
            }}
          >
            {error}
          </div>
        )}
        {selected ? (
          <SkillDetail skill={selected} />
        ) : (
          <div style={{ padding: 20, opacity: 0.6 }}>
            Select a skill to view its body.
          </div>
        )}
      </div>
    </div>
  );
}

function SkillDetail({ skill }: { skill: Skill }) {
  return (
    <div style={{ overflowY: "auto", padding: "10px 14px" }}>
      <h2 style={{ margin: "0 0 4px 0", fontSize: 16 }}>{skill.name}</h2>
      <div style={{ opacity: 0.7, fontSize: 12, marginBottom: 8 }}>
        {skill.id} · v{skill.version} · by {skill.author}
      </div>
      {skill.description && (
        <p style={{ margin: "0 0 10px 0" }}>{skill.description}</p>
      )}
      <MetaRow label="Tags" values={skill.tags ?? []} />
      <MetaRow label="Contexts" values={skill.applicable_contexts ?? []} />
      <MetaRow label="Triggers" values={skill.triggers ?? []} />
      {skill.parameters && skill.parameters.length > 0 && (
        <div style={{ margin: "10px 0" }}>
          <div style={{ fontSize: 12, opacity: 0.7 }}>Parameters</div>
          <ul style={{ margin: "4px 0", paddingLeft: 18, fontSize: 12 }}>
            {skill.parameters.map((p) => (
              <li key={p.name}>
                <code>{p.name}</code>
                <span style={{ opacity: 0.7 }}> : {p.type}</span>
                {p.default !== undefined && p.default !== null ? (
                  <span style={{ opacity: 0.7 }}>
                    {" "}= {JSON.stringify(p.default)}
                  </span>
                ) : null}
              </li>
            ))}
          </ul>
        </div>
      )}
      <div
        style={{
          marginTop: 10,
          paddingTop: 8,
          borderTop: "1px solid var(--color-border)",
          fontSize: 11,
          opacity: 0.7,
        }}
      >
        Body
      </div>
      <pre
        style={{
          marginTop: 4,
          padding: 10,
          background: "var(--color-bg-subtle, rgba(255,255,255,0.03))",
          borderRadius: 4,
          whiteSpace: "pre-wrap",
          fontSize: 12,
        }}
      >
        {skill.body}
      </pre>
    </div>
  );
}

function MetaRow({ label, values }: { label: string; values: string[] }) {
  if (values.length === 0) return null;
  return (
    <div style={{ fontSize: 12, margin: "2px 0" }}>
      <span style={{ opacity: 0.7 }}>{label}: </span>
      {values.join(", ")}
    </div>
  );
}

const chipButtonStyle: React.CSSProperties = {
  padding: "2px 8px",
  borderRadius: 4,
  border: "1px solid var(--color-border)",
  background: "transparent",
  color: "inherit",
  fontSize: 11,
  cursor: "pointer",
};
