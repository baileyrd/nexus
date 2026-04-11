# PRD: Skills Subsystem
**Nexus** — Rust-based AI-native Developer Knowledge Environment

**Version:** 1.0  
**Status:** Ready for Implementation  
**Date:** April 2026  
**Author:** Product Team  

---

## Executive Summary

Skills are first-class, composable instruction sets that shape AI behavior for specific task domains in Nexus. This PRD resolves all open questions from v0.1 and provides implementation-ready specifications for:

- A `.skill.md` file format with frontmatter metadata and markdown instruction body
- Built-in skill library (code-review, write-spec, architecture-design, etc.)
- Skill registry, discovery, and activation mechanisms
- Composition rules and conflict resolution
- Integration with agents and inline AI
- Community skill distribution and marketplace

Skills are **NOT agents**. They are instruction layers that agents and inline AI consume to execute domain-specific tasks with precision and consistency.

---

## 1. Core Decision: Skills as First-Class Forge Files

### 1.1 Definition
Skills are first-class, shareable instruction sets stored as `.skill.md` files in the forge. A skill encodes domain expertise (e.g., "code review," "write specifications") as a reusable behavior template that shapes how the AI system handles specific task categories.

### 1.2 Scope
- **In Scope:** Forge file format, skill library, registry, activation, composition, parameters, authoring SDK
- **Out of Scope:** Community marketplace ratings/reviews, skill monetization, ML-based skill effectiveness tracking (design-level, not MVP)

### 1.3 Skill vs. Agent vs. Plugin
| Concept | Purpose | Scope | Persistence |
|---------|---------|-------|-------------|
| **Skill** | Instruction set that shapes AI behavior for a task domain | Behavior instructions, output format, preferences | Reusable, composable |
| **Agent** | Autonomous workflow orchestrator with goals, memory, and tool access | Long-running processes, multi-step autonomy | Deployed instances |
| **Plugin** | Extended capability container bundling skills, agents, commands, connectors | Feature pack for a domain (e.g., "Legal", "Sales") | Installed bundle |

Skills can be bundled in plugins, but are independently shareable and usable.

---

## 2. Skill File Format (`.skill.md`)

### 2.1 File Location and Naming
```
.forge/skills/
  ├── code-review.skill.md
  ├── write-spec.skill.md
  ├── debug.skill.md
  └── custom/
      └── my-security-audit.skill.md
```

Convention: lowercase, hyphens for multi-word skill names. File extension is `.skill.md`.

### 2.2 File Structure: Frontmatter + Markdown Body

```yaml
---
# REQUIRED
name: Code Review
id: code-review
description: >
  Conduct structured code reviews focusing on security, performance,
  and correctness. Evaluates changes for test coverage, logic flaws,
  and maintainability.
version: 1.0.0
author: Nexus Core Team
created: 2026-04-01

# TAGS (for discovery and filtering)
tags:
  - engineering
  - quality-assurance
  - peer-review

# APPLICABLE CONTEXTS (when this skill auto-activates)
# - pull-request, terminal, editor, ai-chat, agent
# - Optional; if omitted, skill is manual-only
applicable_contexts:
  - pull-request
  - ai-chat

# TRIGGERS (optional keywords or patterns that auto-activate the skill)
triggers:
  - "review this code"
  - "code review"
  - "check this for bugs"
  - "security review"

# INPUT PARAMETERS (optional; skill consumer can override defaults)
parameters:
  - name: strictness
    type: enum
    values: [low, medium, high]
    default: medium
    description: Depth of review rigor
  - name: focus_areas
    type: list
    items: string
    default: [security, performance, correctness]
    description: Domains to prioritize

# DEPENDENCIES (other skills this skill builds on or requires)
depends_on:
  - explain-code
  - write-tests
  (optional; empty if none)

# CAPABILITY RESTRICTIONS (optional; declare what this skill should NOT do)
restrictions:
  - modify_files: false
  - delete_content: false
  - execute_code: false
  allowed_tools:
    - read_file
    - list_files

# OUTPUT FORMAT (how AI should structure results for this skill)
output_format: structured  # structured, markdown, natural, or custom

# VISIBILITY (public for marketplace, private for personal skills)
visibility: public

---

# Code Review Skill

## Purpose
Guide the AI to conduct structured code reviews with focus on security, performance, and test coverage.

## When to Use
- Reviewing pull requests before merge
- Evaluating code snippets for potential issues
- Peer review sessions
- Security audits of critical code paths

## System Prompt Additions

You are an expert code reviewer. Conduct reviews with these principles:

1. **Security First**: Identify injection vulnerabilities, unsafe dependencies, privilege escalation risks.
2. **Performance**: Flag algorithmic inefficiencies, N+1 queries, memory leaks.
3. **Correctness**: Check logic, edge cases, error handling, race conditions.
4. **Maintainability**: Assess code clarity, complexity, naming conventions, architectural fit.
5. **Test Coverage**: Evaluate coverage gaps; suggest test cases for untested branches.

When reviewing code, structure your response as follows:

## Output Template

### Summary
[1-2 sentence high-level assessment]

### Security Findings
- [Finding 1]: [Context] → [Recommendation]
- [Finding 2]: ...

### Performance Findings
- [Finding 1]: [Impact] → [Fix]
- ...

### Correctness Issues
- [Issue 1]: [Scenario] → [Resolution]
- ...

### Test Coverage Gaps
- [Gap 1]: [Path not covered] → [Suggested test]
- ...

### Maintainability Observations
- [Observation 1]: [Why it matters] → [Suggestion]
- ...

### Approval
**Recommend:** [APPROVE | REQUEST_CHANGES | COMMENT]  
**Confidence:** [High | Medium | Low]

## Evaluation Criteria
- Review is actionable (not vague)
- Security issues are clearly explained with remediation
- Trade-offs between strictness and pragmatism (medium strictness = real-world concerns, not perfection)
- Code context understood (language idioms respected)

## Tool Preferences
- `read_file` to examine full context
- `list_files` to understand project structure
- DO NOT modify files; only report findings
```

### 2.3 Frontmatter Schema

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `name` | string | Yes | Human-readable skill name |
| `id` | string | Yes | Unique kebab-case identifier |
| `description` | string | Yes | 1-2 sentence purpose |
| `version` | semver | Yes | Follows semantic versioning |
| `author` | string | Yes | Author/organization |
| `created` | ISO 8601 | Yes | Creation date |
| `tags` | list[string] | Yes | Category tags for discovery |
| `applicable_contexts` | list[string] | No | Auto-activation contexts |
| `triggers` | list[string] | No | Keyword/pattern triggers |
| `parameters` | list[object] | No | Input parameters with types, defaults |
| `depends_on` | list[string] | No | Skill dependencies |
| `restrictions` | object | No | Capability restrictions and allowed tools |
| `output_format` | enum | Yes | structured, markdown, natural, custom |
| `visibility` | enum | No | public or private (default: private) |

### 2.4 Body Structure (Markdown)

The markdown body contains:
- **Purpose**: When and why to use the skill
- **System Prompt Additions**: Detailed instructions injected into the AI system prompt
- **Output Template**: Expected structure of AI output (example + explanation)
- **Evaluation Criteria**: How to assess if skill output is high-quality
- **Tool Preferences**: Which tools the AI should prioritize or avoid
- **Notes/Examples**: Contextual guidance and edge cases

---

## 3. Skill Registry and Discovery

### 3.1 Registry Structure
The Nexus forge maintains a skill registry in `.forge/skills/REGISTRY.json`:

```json
{
  "version": "1.0",
  "last_updated": "2026-04-11T12:00:00Z",
  "skills": [
    {
      "id": "code-review",
      "name": "Code Review",
      "path": ".forge/skills/code-review.skill.md",
      "version": "1.0.0",
      "tags": ["engineering", "quality-assurance"],
      "applicable_contexts": ["pull-request", "ai-chat"],
      "author": "Nexus Core Team",
      "visibility": "public"
    },
    ...
  ]
}
```

### 3.2 Discovery Mechanisms
1. **File-based:** Forge scans `.forge/skills/` for `.skill.md` files
2. **Indexing:** On startup and file change, registry is updated
3. **Search:** Users search by name, tag, or description via CLI: `nexus skills list --tag engineering`
4. **Auto-discovery:** By context and triggers (see Section 4)

### 3.3 Loading and Validation
- Skills are loaded into memory on first use (lazy loading)
- YAML frontmatter is validated against schema
- Circular dependencies detected and rejected
- File encoding must be UTF-8

---

## 4. Skill Activation

### 4.1 Activation Methods

#### Manual Selection
```
> /skill code-review
> Review the following PR...
```

User explicitly selects skill via `/skill <id>` command or UI skill picker.

#### Context-Based Auto-Activation
When user context matches `applicable_contexts`:
- `pull-request`: Skill auto-activates when AI is analyzing a PR
- `terminal`: Skill auto-activates in terminal-based interaction
- `editor`: Skill auto-activates when editing code files
- `ai-chat`: Skill can activate in chat based on intent
- `agent`: Skill auto-activates for assigned agents

#### Trigger-Based Auto-Activation
If user input matches any `triggers`, the skill activates:
- Input: "review this code for security issues"
- Matched trigger: "security review"
- Result: `code-review` skill auto-activates with `strictness: high`

#### Agent Assignment
Agents are initialized with a default skill set (e.g., a "code review agent" has `code-review`, `write-tests`, `explain-code`). Users can override:
```yaml
agent: code-review-bot
skills:
  - code-review:strictness=high
  - write-tests:coverage_target=90
```

### 4.2 Activation Priority (Conflict Resolution)
If multiple skills activate simultaneously:
1. **Explicit > Auto:** Manual selection overrides auto-activation
2. **Specific > General:** Trigger-based overrides context-based
3. **Most Recent > Default:** User's most recent skill choice wins if both are equally specific

---

## 5. Skill Composition and Layering

### 5.1 Composition Syntax
Skills can be composed using a chain or layer syntax:

#### Chaining (Sequential)
```
/skill code-review → write-tests → explain-code
```
Output of each skill becomes input to the next.

#### Layering (Parallel Instructions)
```
/skill [code-review:strictness=high, security-audit]
```
Both skills' system prompts are merged and injected together.

### 5.2 Composition Rules
- **Chains:** Up to 5 skills deep (prevents infinite loops)
- **Layers:** Up to 10 skills per layer
- **Dependency resolution:** If skill A depends_on skill B, B is auto-inserted before A
- **Conflict resolution:** If two layered skills contradict (e.g., output_format differs), error message guides user to resolve manually

### 5.3 Prompt Merging
When multiple skills are layered:
1. System prompt additions are concatenated in dependency order
2. Parameters are merged (later skill's parameter overrides earlier if keys match)
3. Output templates are merged hierarchically: first skill's template is primary; secondary skills' formats are noted as optional sections
4. Tool restrictions are unioned: if skill A allows tools [A, B] and skill B allows [B, C], the intersection [B] is enforced

---

## 6. Skill-Agent Relationship

### 6.1 Skills as Agent Instructions
- **Agent** = Autonomous workflow orchestrator (has goals, memory, tools, execution loop)
- **Skill** = Instruction layer that the agent's AI system consumes

An agent's behavior is shaped by:
1. Its archetype definition (e.g., "code-review-bot")
2. Its assigned skills (default + user overrides)
3. Its memory and history

### 6.2 Agent Skill Assignment
Example agent with skills:
```yaml
agent:
  id: security-auditor
  description: Autonomous security auditor
  skills:
    - code-review:strictness=high,focus_areas=[security]
    - security-audit:depth=deep
    - write-report:format=security
  tools:
    - read_file
    - grep
    - git_log
  execution_model: autonomous  # Can run without user interaction
```

### 6.3 Inline AI + Skills
When using inline AI (e.g., in chat), users select skills to shape the AI's behavior for that interaction:
```
User: /skill write-spec
User: How would you structure a new notification system?
AI: [Uses write-spec skill to respond with PRD-like output]
```

Skills do NOT make the AI autonomous — they guide behavior within a single interaction.

---

## 7. Built-in Skill Library (v1.0)

Nexus ships with 12 built-in skills covering core developer workflows:

| Skill ID | Name | Description | Primary Context |
|----------|------|-------------|-----------------|
| `code-review` | Code Review | Security, performance, correctness review | pull-request |
| `write-spec` | Write Specification | Structure ideas as PRD/specification | ai-chat |
| `architecture-design` | Architecture Design | System and service design | ai-chat, terminal |
| `debug` | Debug | Structured debugging (reproduce, isolate, diagnose, fix) | terminal, ai-chat |
| `refactor` | Refactor | Refactoring code for clarity, performance | editor, ai-chat |
| `explain-code` | Explain Code | Walkthrough code logic, design, intent | editor, ai-chat |
| `write-docs` | Write Documentation | Technical docs, READMEs, runbooks | editor, ai-chat |
| `write-tests` | Write Tests | Test case design and implementation | editor, terminal |
| `data-analysis` | Data Analysis | Exploratory data analysis and insights | ai-chat, terminal |
| `summarize` | Summarize | Distill text/code/discussions into summaries | ai-chat |
| `brainstorm` | Brainstorm | Creative ideation and problem-solving | ai-chat |
| `commit-message` | Commit Message | Generate clear, structured commit messages | terminal, editor |

Each built-in skill includes:
- Full `.skill.md` file in `.forge/skills/`
- Validation against schema
- Example usage documentation
- Tested system prompt additions

---

## 8. Skill Context Injection

### 8.1 Prompt Injection Strategy
When a skill is activated, its system prompt additions are injected into the AI prompt at a specific location:

```
[Base System Prompt]
↓
[Skill System Prompt Addition] ← Injected here
↓
[User Input / Context]
↓
[Optional: Assistant Priming]
```

### 8.2 Token Budget
- **Base system prompt:** 1,000 tokens (fixed)
- **Per-skill budget:** 500 tokens
- **Active skills limit:** 3 skills per interaction (to stay within budget)
- **Overflow handling:** If total exceeds budget, user receives warning and can reduce active skills

### 8.3 Prompt Injection Safety
- Skills are sandboxed: instructions are appended, not interpolated into critical system sections
- No skill can override core safety guardrails (no unauthorized file access, dangerous operations)
- Skill instructions are validated for prompt injection patterns on load

---

## 9. Skill Parameters

### 9.1 Parameter Definition
Skills can declare parameters that users customize:

```yaml
parameters:
  - name: strictness
    type: enum
    values: [low, medium, high]
    default: medium
    description: Depth of review rigor
    
  - name: timeout_seconds
    type: integer
    min: 1
    max: 300
    default: 60
    description: Max execution time
    
  - name: focus_areas
    type: list
    items: string
    enum_items: [security, performance, correctness, maintainability, coverage]
    default: [security, performance, correctness]
    description: Review focus areas
```

### 9.2 Parameter Types
- `enum`: Fixed set of values
- `integer`: Numeric with optional min/max
- `float`: Decimal with optional min/max
- `string`: Free-form text
- `boolean`: True/false toggle
- `list`: Array of items (with optional enum constraint)

### 9.3 Parameter Injection
Parameters are injected into the system prompt as variables:
```
System Prompt:
"Conduct code review with strictness level: {{strictness}}"
```

User selects:
```
/skill code-review:strictness=high
```

Result:
```
"Conduct code review with strictness level: high"
```

---

## 10. Skill Authoring SDK

### 10.1 Skill Creation Command
```bash
nexus skill create --name "My Custom Skill" --id my-custom-skill
```

Creates scaffold:
```
.forge/skills/custom/my-custom-skill.skill.md
```

With template:
```yaml
---
name: My Custom Skill
id: my-custom-skill
description: [Description]
version: 0.1.0
author: [Your Name]
created: 2026-04-11
tags: [domain, use-case]
applicable_contexts: []
triggers: []
parameters: []
depends_on: []
restrictions: {}
output_format: markdown
visibility: private
---

# My Custom Skill

## Purpose
...
```

### 10.2 Validation
```bash
nexus skill validate my-custom-skill.skill.md
```

Validates:
- YAML frontmatter structure
- Required fields present
- Parameter type consistency
- Circular dependency detection
- Prompt injection patterns

### 10.3 Testing/Preview Mode
```bash
nexus skill test my-custom-skill.skill.md --input "sample input"
```

Runs skill against sample input and shows:
- System prompt injection
- Merged parameters
- Output format applied
- Token usage estimate

### 10.4 Linting
```bash
nexus skill lint my-custom-skill.skill.md
```

Checks:
- Readability (clear instructions, examples)
- Completeness (output template, evaluation criteria)
- Quality (typos, consistency)

---

## 11. Distribution Model

### 11.1 Skill Packaging
Skills can be distributed in three ways:

#### 1. Standalone Skill File
Share `.skill.md` file directly (like Obsidian snippets):
```
Share: my-security-audit.skill.md
Install: Copy to .forge/skills/custom/
```

#### 2. Bundled in Plugin
Plugin packages include skills:
```
my-plugin/
  ├── PLUGIN.md
  ├── skills/
  │   ├── skill-1.skill.md
  │   ├── skill-2.skill.md
  └── agents/
      └── agent-1.agent.md
```

#### 3. Skill Marketplace
Nexus maintains a community skill marketplace (future; design-level):
- Users publish skills with metadata
- Community rates and reviews
- One-click install to `.forge/skills/community/`

### 11.2 Licensing
- Built-in skills: MIT (part of Nexus)
- Community skills: Author declares license (MIT, Apache 2.0, proprietary, etc.)
- Nexus provides SPDX license field in frontmatter:
  ```yaml
  license: MIT
  license_url: https://opensource.org/licenses/MIT
  ```

---

## 12. Skill Versioning

### 12.1 Semantic Versioning
Skills follow semver: `MAJOR.MINOR.PATCH`
- **MAJOR:** Breaking changes (e.g., output format changes, removed parameters)
- **MINOR:** New features (e.g., new parameters, new evaluation criteria)
- **PATCH:** Bug fixes (e.g., typo in system prompt, improved wording)

### 12.2 Dependency Constraints
Skills can specify version constraints on dependencies:
```yaml
depends_on:
  - explain-code: ">=1.0.0,<2.0.0"
  - write-tests: "^1.2.0"
```

Constraints follow npm semver syntax.

### 12.3 Breaking Change Policy
If skill changes break existing usage:
1. Document breaking changes in `CHANGELOG.md`
2. Increment MAJOR version
3. Provide migration guide for users

---

## 13. Skill Effectiveness Tracking (Design-Level, Future)

### 13.1 Tracking Mechanism (Optional in v1.0)
Optional telemetry to improve skills over time:
- When skill produces output, track: user acceptance (accept/reject/iterate)
- Aggregate feedback: which focus areas most commonly refined, parameter combinations most effective
- Share anonymized insights: skill page shows "90% of users accepted output on first iteration"

### 13.2 Opt-In
- Enabled by default, but users can disable: `nexus config set skills.tracking false`
- No personal data collected; only skill outcome metrics

### 13.3 Quality Metrics
- **Acceptance rate:** % of outputs accepted without iteration
- **Refinement depth:** Average number of iterations before acceptance
- **Parameter popularity:** Which parameter values users select most

---

## 14. Security and Sandboxing

### 14.1 Skill Restrictions
Skills declare what they can and cannot do:
```yaml
restrictions:
  modify_files: false
  delete_content: false
  execute_code: false
  allowed_tools: [read_file, list_files, grep]
```

### 14.2 Tool Whitelisting
Skills can only access tools they declare:
- Attempt to use unlisted tool → Error message
- Prevents skill from unexpectedly modifying files or executing dangerous operations

### 14.3 Community Skill Review
Community skills undergo review before marketplace approval:
- Code inspection: No prompt injection patterns, suspicious tool requests
- Capability audit: Restrictions are reasonable and documented
- Author verification: Author identity confirmed

### 14.4 Capabilities Denied to All Skills
No skill can override these guardrails:
- Modify security settings
- Delete user account
- Exfiltrate sensitive environment variables
- Execute arbitrary shell commands without explicit user approval per command

---

## 15. Skill Browser and Discovery UI

### 15.1 Skill Browser (`nexus skills` Command)
```bash
nexus skills list                                    # All available skills
nexus skills list --tag engineering                  # By category
nexus skills search "code review"                    # By name/description
nexus skills show code-review                        # Detailed view
```

### 15.2 In-App Skill Picker
In Nexus UI (AI chat context):
- Dropdown showing available skills with icons/tags
- Search by name or description
- Preview skill description and parameters on hover
- Quick-enable/disable toggle

### 15.3 Skill Details Page
Shows:
- Name, author, version
- Full description
- Parameters with defaults (interactive adjustment)
- Example usage
- Applicable contexts
- Community reviews/ratings (marketplace)

---

## 16. Skill Creation UX

### 16.1 In-App Skill Editor
Future UI feature (not MVP):
- Text editor for `.skill.md` with syntax highlighting
- Live preview: render YAML frontmatter as form, show markdown output
- Parameter tester: input sample data, see skill instructions injected
- Validation feedback: real-time error/warning display

### 16.2 Guided Scaffolding
```
nexus skill create --interactive
? Skill name: My Code Reviewer
? ID: my-code-reviewer
? Description: ...
? Primary context: [pull-request, terminal, ai-chat, editor]
? Parameters: strictness (enum: low/medium/high)
→ Created .forge/skills/custom/my-code-reviewer.skill.md
```

### 16.3 Testing Harness
```bash
nexus skill test my-code-reviewer.skill.md \
  --input "code snippet to review" \
  --param strictness=high \
  --show-prompt  # Display injected system prompt
```

Output shows:
- Full system prompt with skill injection
- Estimated token usage
- Parameter substitutions
- Ready for real test run

---

## 17. Skill Activation Indicator

### 17.1 Active Skill Display
In AI chat header:
```
🧠 Code Review | Write Spec | Debug  [✕]
```

Small pills showing:
- Active skills
- Click to view parameters
- Click [✕] to deactivate

### 17.2 Skill Indicator Colors
- **Green:** Skill matched automatically (context or trigger)
- **Blue:** Skill manually selected
- **Yellow:** Skill inherited from agent
- **Gray:** Skill available but inactive

### 17.3 Parameter Display
Click skill pill to see active parameters:
```
Code Review
├─ strictness: high
├─ focus_areas: [security, performance]
└─ [Edit] [Remove]
```

---

## 18. Skill Marketplace (Design-Level, Future)

### 18.1 Marketplace Structure
Community platform (separate from core Nexus):
- Centralized skill repository
- Search, filtering, ratings
- Author profiles
- Version history
- Community discussions

### 18.2 One-Click Install
```bash
nexus skill install @author/skill-name
```

Downloads skill from marketplace, validates, installs to `.forge/skills/community/`.

### 18.3 Quality Indicators
- Author reputation
- Community ratings (1-5 stars)
- Download count
- Last updated date
- Reported issues/bugs

---

## 19. Implementation Roadmap

### Phase 1 (Weeks 1-2): Core Infrastructure
- Skill file format and schema validation
- Skill registry and indexing
- Manual skill activation (`/skill` command)
- 12 built-in skills (templates + documentation)

### Phase 2 (Weeks 3-4): Activation and Composition
- Context-based auto-activation
- Trigger-based activation
- Skill composition (chaining, layering)
- Parameter system

### Phase 3 (Weeks 5-6): Authoring and UX
- Authoring SDK (`nexus skill create/validate/test`)
- Skill browser UI
- Activation indicator in chat header
- In-app skill picker

### Phase 4 (Week 7+, Future): Community and Analytics
- Community skill distribution (standalone files)
- Plugin bundling
- Optional: Marketplace
- Optional: Effectiveness tracking

---

## 20. Acceptance Criteria

### Functional
- [ ] `.skill.md` format implemented and validated
- [ ] All 12 built-in skills authored and tested
- [ ] Skill registry dynamically loads from `.forge/skills/`
- [ ] Manual activation via `/skill` command works
- [ ] Context and trigger-based auto-activation functional
- [ ] Skill composition (chains and layers) implemented
- [ ] Parameter system with defaults and overrides
- [ ] Skill authoring SDK: create, validate, test, lint commands
- [ ] Skill browser: list, search, show commands
- [ ] Agent skill assignment works
- [ ] Skill activation indicators visible in UI

### Quality
- [ ] All built-in skills pass linting and quality checks
- [ ] Example `.skill.md` file provided for custom authors
- [ ] Documentation: skill authoring guide, examples
- [ ] Circular dependency detection prevents infinite loops
- [ ] Prompt injection patterns rejected on skill load
- [ ] Token budget enforced (warn if over)

### Testing
- [ ] Unit tests: skill schema validation
- [ ] Integration tests: skill activation, composition, parameter injection
- [ ] End-to-end test: author custom skill, use in chat, verify output
- [ ] Performance: registry load < 100ms even with 100+ skills

---

## 21. Dependencies and Risks

### Dependencies
- **Forge subsystem:** Registry storage, file discovery
- **AI prompt system:** Injection mechanism, token counting
- **Agent system:** Skill assignment, execution
- **CLI:** `nexus` command infrastructure

### Risks and Mitigations
| Risk | Impact | Mitigation |
|------|--------|-----------|
| Prompt injection via skill instructions | Security | Validate all skill bodies for injection patterns; sandbox restrictions |
| Skill composition creates infinite loops | UX/Stability | Detect cycles in dependency graph; limit chain depth to 5 |
| Token budget exceeded | Performance | Warn user; auto-disable lowest-priority skills |
| Community skill quality varies | Trust | Marketplace review process; ratings/feedback |
| Skills contradict each other | UX | Conflict resolution rules; merging logic tested |

---

## 22. Glossary

- **Skill:** First-class instruction set (`.skill.md` file) that shapes AI behavior
- **Activation:** Process of enabling a skill for the current interaction
- **Context:** Environment where interaction occurs (pull-request, editor, terminal, ai-chat, agent)
- **Trigger:** Keyword or pattern that auto-activates a skill
- **Parameter:** User-customizable input variable in a skill
- **Composition:** Combining multiple skills (chaining or layering)
- **Restriction:** Limitation on tools a skill can use or actions it can take
- **Forge:** Nexus's file-based development knowledge environment
- **Marketplace:** (Future) Community platform for discovering and installing skills

---

## 23. References and Related Docs

- **Nexus v0.1 PRD:** Parent document with Skills open questions
- **Forge Subsystem PRD:** File structure, registry, discovery mechanisms
- **Agent Subsystem PRD:** Agent archetypes, skill assignment
- **AI Prompt System PRD:** System prompt injection, token budgeting
- **Example Skill Library:** `.forge/skills/` directory with all built-in skills

---

**Document prepared by:** Product Team  
**Last updated:** 2026-04-11  
**Next review:** May 2026 (post-implementation)
