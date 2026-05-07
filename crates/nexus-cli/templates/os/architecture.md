# Architecture

> Canonical domain → task → skill registry for this forge. This file is
> empty by design — the OS Setup skill (BL-054 Phase 5) interviews you
> and fills it in. Until then, the `nexus.osArchitecture` panel will
> show a placeholder state.

## Domains

_None registered yet. Each domain is an H2; tasks under a domain are
list items tagged with the four-attribute format._

```
example-task  [skill | foundation | raw | local cron 0700]
              └─────┘ └──────────┘ └─┘ └────────────────┘
              type    class        dest automation

type        — skill / agent / command / manual
class       — foundation (recurring) / capability (on-demand)
memory-dest — raw / wiki / project / output / none
automation  — local cron <HHMM> / webhook / none
```

## Tasks

_No tasks until the OS Setup skill runs. Run `nexus skill run os-setup`
once that skill ships._
