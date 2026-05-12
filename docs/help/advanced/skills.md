# Skills (prompt templates)

A **skill** is a parameterized prompt template you can render and send
to the model on demand. Skills live as `.skill.md` files in
`<forge>/.forge/skills/` so they're versionable, editable, and
shareable.

## Built-in skills

A fresh forge ships with a starter set:

- `code-reviewer` — review a code diff or file
- `daily-journal` — generate a daily journal scaffold
- `meeting-notes` — turn raw notes into structured meeting minutes
- `commit-message` — propose a Conventional Commit message from a diff

Browse them in the **Skills** panel in the shell or:

```bash
nexus skill list
```

## Skill format

```markdown
---
name: code-reviewer
description: Review a code diff for clarity, bugs, and idiom.
parameters:
  - name: diff
    type: string
    required: true
  - name: focus
    type: string
    default: "all of the above"
---

You are reviewing a code change. Focus on {{focus}}.

```diff
{{diff}}
```

Return:
- **Bugs** (if any)
- **Style** notes
- **Suggestions**
```

The body is [Handlebars](https://handlebarsjs.com)-style; `{{name}}`
substitutes parameters.

## Render

```bash
nexus skill render code-reviewer --arg diff="$(git diff)" --arg focus="bugs"
```

This prints the rendered prompt. Feed it to `nexus ai ask` to send to
the model — `ai ask` takes the question as a positional argument, so
capture the rendered output and pass it directly:

```bash
prompt=$(nexus skill render code-reviewer --arg diff="$(git diff)")
nexus ai ask "$prompt"
```

## In the shell

The Skills panel shows every skill with a parameter form. Fill in the
arguments, click **Render**, and the rendered prompt drops into a new
chat session ready to send.

## Authoring your own

Just drop a `.skill.md` in `.forge/skills/`. Nexus picks it up on the
next file-watcher tick. There's no "register" step.

```bash
nexus skill list                        # confirm it's discovered
nexus skill render my-new-skill --arg x=1
```

## Importing skills

Skills are plain markdown — copy a `.skill.md` from any source into
`.forge/skills/` and it's installed. Plugins can also contribute
skills.

## Limitations

- Skills are templates, not agents. They render a single prompt, not a
  multi-step plan. For multi-step work, see [Agents](agents.md).
- The Handlebars dialect is intentionally limited (substitution +
  conditionals + iteration). No arbitrary code execution.
