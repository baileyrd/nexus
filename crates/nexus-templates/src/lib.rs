//! Page-template subsystem for Nexus.
//!
//! A **template** is a `.template.md` file with YAML frontmatter and a
//! markdown body. Frontmatter declares the template's name, parameters, and
//! a path pattern; the body is the page contents. Templates are stored in
//! `<forge>/.forge/templates/` (sub-directories supported) and a small
//! built-in set is available without any setup.
//!
//! ```text
//! ---
//! name: meeting-notes
//! description: A meeting-notes scaffold with attendees and action items.
//! target_path: meetings/{{date+7d}} - {{title}}.md
//! parameters:
//!   - name: title
//!     type: string
//!     required: true
//!   - name: attendees
//!     type: string
//!     default: ""
//! ---
//! # {{title}}
//!
//! - **Date**: {{date:MM/DD/YYYY}}
//! - **Attendees**: {{attendees}}
//!
//! ## Notes
//!
//! ## Action items
//!
//! - [ ]
//! ```
//!
//! Application:
//!
//! 1. Caller supplies key/value arguments.
//! 2. Built-in variables (`today`, `now`, `forge_path`, local-time-based) and
//!    any `{{date...}}` dynamic variables referenced in the template (custom
//!    format / offset — see [`date_vars`]) are added.
//! 3. Defaults fill in missing optional parameters; required ones with no
//!    default produce an error.
//! 4. Substitution runs on the body and the `target_path`.
//! 5. The rendered body is written to the resolved target path.

#![deny(missing_docs)]
#![warn(clippy::pedantic)]
#![allow(clippy::module_name_repetitions)]

pub mod builtins;
pub mod core_plugin;
pub mod date_vars;
pub mod registry;
pub mod substitute;
pub mod template;

pub use core_plugin::{
    TemplatesCorePlugin, HANDLER_APPLY, HANDLER_GET, HANDLER_LIST, HANDLER_RELOAD, HANDLER_RENDER,
    PLUGIN_ID,
};
pub use registry::{TemplateRegistry, TemplateRegistryError};
pub use substitute::{render, SubstitutionError};
pub use template::{
    parse_template_file, parse_template_text, ApplyError, ParameterType, Template, TemplateMeta,
    TemplateParameter, TemplateParseError,
};
