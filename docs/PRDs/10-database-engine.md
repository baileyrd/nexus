# PRD: Database Engine Subsystem
**Nexus v1.0 — April 2026**  
*Notion-level structured data with local-first, file-based architecture*

---

## Executive Summary

The Database Engine enables Nexus users to create, manage, and query structured databases with 20+ property types, multiple simultaneous views (Table, Board, List, Calendar, Gallery), and a powerful formula language. Databases are stored as `.bases` files (TOML format) with records as embedded or external `.md` files, indexed into SQLite for fast queries. The engine is fully local-first, supports cross-database relations, and integrates with the AI engine for intelligent record creation and formula suggestion.

---

## 1. File Format Decision: `.bases` + `.md` Hybrid

### Recommendation: TOML `.bases` file + External `.md` Records

**Rationale:**
- TOML is human-readable, editable in any text editor, and version-control friendly
- Separates metadata (schema, views, relations) from content (record bodies)
- Records as external `.md` files enable direct editing in Nexus, future embedding in knowledge graphs
- Avoids single-file bloat for large databases (100k+ records)
- SQLite indexes the `.bases` file for fast queries; `.md` files remain loosely coupled

### `.bases` File Specification

```toml
# Nexus Database v1
# Created: 2026-04-11T09:30:00Z
# UUID: db_550e8400e29b41d4a716446655440000

[metadata]
name = "Project Tracker"
description = "Sprint planning and task management"
created_at = "2026-04-11T09:30:00Z"
updated_at = "2026-04-11T14:22:15Z"
version = "1"  # Schema version for migrations

[properties]
# Property ID → Property Definition
# IDs are UUIDs, names are human-readable but mutable

title = { id = "prop_001", type = "Title", description = "Task name" }
status = { id = "prop_002", type = "Select", options = [
  { id = "status_todo", name = "To Do", color = "gray" },
  { id = "status_inprog", name = "In Progress", color = "blue" },
  { id = "status_done", name = "Done", color = "green" }
], default = "status_todo" }
assignee = { id = "prop_003", type = "People", allow_multiple = false }
due_date = { id = "prop_004", type = "Date", include_time = false, default = null }
priority = { id = "prop_005", type = "Select", options = [
  { id = "p_low", name = "Low", color = "green" },
  { id = "p_med", name = "Medium", color = "yellow" },
  { id = "p_high", name = "High", color = "red" }
], default = "p_med" }
effort = { id = "prop_006", type = "Number", format = "number", min = 0, max = 100 }
tags = { id = "prop_007", type = "MultiSelect", options = [] }  # Options managed dynamically
created_time = { id = "prop_008", type = "CreatedTime" }
last_edited_time = { id = "prop_009", type = "LastEditedTime" }
created_by = { id = "prop_010", type = "CreatedBy" }
last_edited_by = { id = "prop_011", type = "LastEditedBy" }

[records]
# Record ID → Metadata (type, path to .md file, property values)

record_ae3f = { id = "record_ae3f", created_at = "2026-04-10T14:00:00Z", 
  path = "records/ae3f.md",
  props = {
    prop_001 = "Design database schema",
    prop_002 = "status_inprog",
    prop_003 = "user_123",
    prop_004 = "2026-04-15",
    prop_005 = "p_high",
    prop_006 = 8,
    prop_007 = ["backend", "critical"],
    prop_008 = "2026-04-10T14:00:00Z",
    prop_009 = "2026-04-11T10:30:00Z",
    prop_010 = "user_123",
    prop_011 = "user_456"
  }
}

record_b4e2 = { id = "record_b4e2", created_at = "2026-04-11T09:15:00Z",
  path = "records/b4e2.md",
  props = {
    prop_001 = "Write view rendering layer",
    prop_002 = "status_todo",
    prop_003 = "user_456",
    prop_004 = "2026-04-18",
    prop_005 = "p_med",
    prop_006 = 13,
    prop_007 = ["frontend"],
    prop_008 = "2026-04-11T09:15:00Z",
    prop_009 = "2026-04-11T09:15:00Z",
    prop_010 = "user_456",
    prop_011 = "user_456"
  }
}

[views]
# View ID → View Definition (filters, sorts, grouping, visible properties)

table_default = { id = "view_tbl_001", type = "Table", name = "All Tasks",
  visible_properties = ["prop_001", "prop_002", "prop_003", "prop_004", "prop_005"],
  property_widths = { prop_001 = 300, prop_002 = 120, prop_003 = 150, prop_004 = 120, prop_005 = 100 },
  filters = [],
  sorts = [{ property = "prop_004", direction = "asc" }],
  is_default = true
}

board_status = { id = "view_brd_001", type = "Board", name = "By Status",
  visible_properties = ["prop_001", "prop_005", "prop_003"],
  group_by = "prop_002",
  filters = [],
  sorts = [{ property = "prop_006", direction = "desc" }]
}

calendar_view = { id = "view_cal_001", type = "Calendar", name = "Timeline",
  date_property = "prop_004",
  visible_properties = ["prop_001", "prop_005"],
  filters = [
    { property = "prop_002", operator = "is_not", values = ["status_done"] }
  ]
}

[relations]
# Relations to other databases (if applicable in future)
# Format: relation_id = { target_db_uuid, many_to_many = bool, back_relation_id }

[rollups]
# Rollup formulas (computed from relations)
# Format: rollup_id = { relation_id, aggregation_function, source_property }

[formulas]
# Stored formulas for fields (if any formula properties exist)
# Format: formula_id = { property_id, expression }

[templates]
# Record templates for this database
record_template_bug = { id = "tmpl_bug", name = "Bug Report",
  description = "Report a software defect",
  default_values = { prop_002 = "status_todo", prop_005 = "p_med" }
}
```

### Record `.md` File Format

Records are stored as individual Markdown files with YAML frontmatter:

```markdown
---
database_id: db_550e8400e29b41d4a716446655440000
record_id: record_ae3f
title: Design database schema
created_at: 2026-04-10T14:00:00Z
updated_at: 2026-04-11T10:30:00Z
properties:
  prop_001: Design database schema
  prop_002: status_inprog
  prop_003: user_123
  prop_004: 2026-04-15
  prop_005: p_high
  prop_006: 8
  prop_007: ["backend", "critical"]
  prop_008: 2026-04-10T14:00:00Z
  prop_009: 2026-04-11T10:30:00Z
  prop_010: user_123
  prop_011: user_456
---

# Design database schema

This task focuses on designing the core schema for Nexus databases...

## Architecture

- Property types (20+ types)
- Record storage (.md files)
- View system (Table, Board, Calendar, etc.)

## Next Steps

1. Define TOML format
2. Create SQLite indexing layer
3. Implement view renderers
```

---

## 2. Record Storage Architecture

### Hybrid Approach: Embedded Metadata + External Body

**Storage Model:**
1. **Metadata** (property values) lives in `.bases` TOML file — fast to load, query, and index
2. **Body content** (Markdown document) lives in external `.md` file — editable, large-text-friendly
3. **SQLite index** mirrors `.bases` data for fast queries, pagination, and sorting

### Record Lifecycle

```
User creates record
  ↓
Engine generates record ID (UUID)
  ↓
Creates records/{record_id}.md with YAML frontmatter
  ↓
Adds record entry to .bases [records] table
  ↓
Indexes metadata into SQLite (INSERT or UPSERT)
  ↓
Emits RecordCreated event
```

### Record Deletion

```
User deletes record
  ↓
Engine marks record as deleted_at in .bases (soft delete)
  ↓
Removes from SQLite index
  ↓
.md file moved to .trash/{record_id}.md (or hard deleted)
  ↓
Emits RecordDeleted event
```

### Load & Query

```
App opens database
  ↓
Parse .bases TOML file (fast, ~100ms for 10k records)
  ↓
Check SQLite index version
  ↓
If stale: Re-index (incremental UPSERT for changed records)
  ↓
Execute query against SQLite
  ↓
For each result row, lazy-load .md body from disk (only on demand)
```

---

## 3. Property Type System

### Complete Type Definitions (Rust)

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum PropertyType {
    // Text Properties
    Title {
        description: Option<String>,
    },
    RichText {
        description: Option<String>,
    },
    Url,
    Email,
    Phone,

    // Numeric Properties
    Number {
        format: NumberFormat,  // "number", "percent", "currency", "rating"
        min: Option<f64>,
        max: Option<f64>,
        precision: Option<u32>,
    },

    // Selection Properties
    Select {
        options: Vec<SelectOption>,
        default: Option<String>,  // option ID
    },
    MultiSelect {
        options: Vec<SelectOption>,
    },
    Status {
        options: Vec<SelectOption>,  // Status-specific UI treatment
        default: Option<String>,
    },

    // Temporal Properties
    Date {
        include_time: bool,
        date_format: Option<String>,  // "YYYY-MM-DD", "MMM DD, YYYY", etc.
        time_format: Option<String>,  // "HH:mm", "h:mm a", etc.
    },
    CreatedTime,
    LastEditedTime,

    // Relational Properties
    Relation {
        target_database_id: String,
        title: String,
        one_to_many: bool,
        back_relation_id: Option<String>,
    },
    Rollup {
        relation_id: String,
        aggregation: AggregationFunction,  // Sum, Count, Average, Min, Max, etc.
        source_property_id: String,
    },
    Formula {
        expression: String,  // Nexus formula language
    },

    // Media Properties
    Files,
    Checkbox,

    // Identity Properties
    People {
        allow_multiple: bool,
    },
    CreatedBy,
    LastEditedBy,
    UniqueId {
        prefix: Option<String>,  // "TASK-", "BUG-", etc.
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SelectOption {
    pub id: String,           // UUID
    pub name: String,
    pub color: String,        // "gray", "blue", "red", "yellow", "green", "purple"
    pub icon: Option<String>, // Emoji or icon ID
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum NumberFormat {
    Number,
    Percent,
    Currency { code: String },  // "USD", "EUR", "GBP"
    Rating { max: u32 },         // 5-star, etc.
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AggregationFunction {
    Sum,
    Average,
    Min,
    Max,
    Count,
    CountUnique,
    Percent,
    Checked,
    Unchecked,
    Percent(String),  // Percent of a specific value
}

pub trait PropertyValidator {
    fn validate(&self, value: &PropertyValue) -> Result<(), ValidationError>;
}

impl PropertyValidator for PropertyType {
    fn validate(&self, value: &PropertyValue) -> Result<(), ValidationError> {
        match (self, value) {
            (PropertyType::Email, PropertyValue::Text(email)) => {
                if email::valid(email) { Ok(()) } else { Err(...) }
            }
            (PropertyType::Number { min, max, .. }, PropertyValue::Number(n)) => {
                if let Some(m) = min { if n < m { return Err(...); } }
                if let Some(m) = max { if n > m { return Err(...); } }
                Ok(())
            }
            // ... other validations
            _ => Err(ValidationError::TypeMismatch),
        }
    }
}
```

### Custom Property Types

Nexus allows plugin developers to define custom property types by implementing the `PropertyType` trait:

```rust
pub trait CustomPropertyType: Serialize {
    fn type_name() -> &'static str;
    fn validate(&self, value: &serde_json::Value) -> Result<(), String>;
    fn render_html(&self, value: &serde_json::Value) -> String;
    fn export_csv(&self, value: &serde_json::Value) -> String;
}
```

---

## 4. Schema Management

### Schema Definition

Schemas are versioned in `.bases` metadata:

```toml
[metadata]
version = "1"  # Incremented on any structural change
```

### Schema Modifications

**Add Property:**
```rust
pub fn add_property(db: &mut Database, property: Property) -> Result<()> {
    db.properties.insert(property.id.clone(), property);
    db.metadata.version += 1;
    db.save()?;
    Event::SchemaChanged { db_id: db.id.clone(), reason: "property_added" }.emit();
    Ok(())
}
```

**Remove Property:**
- Mark property as `deleted = true` in schema (soft delete)
- Remove values from all records
- SQLite column remains (future cleanup)
- Notify all views to refresh

**Rename Property:**
- Update property.name in schema
- Record the change in metadata: `property_renames = { "prop_001" = "Old Name" }`
- No data migration needed (ID-based, not name-based)

**Change Property Type:**
- Only allowed for compatible types (Number ↔ Percent, Select ↔ MultiSelect with caution)
- For incompatible changes: create new property, migrate data via formula
- Log migration history in metadata

### Schema Versioning

```toml
[metadata]
version = "2"

[[migrations]]
version = "1_to_2"
timestamp = "2026-04-15T10:00:00Z"
action = "property_type_change"
property_id = "prop_005"
from_type = "Number"
to_type = "Select"
migration_formula = "IF(prop_005 > 50, 'High', IF(prop_005 > 20, 'Medium', 'Low'))"
```

---

## 5. View System

### View Data Model

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct View {
    pub id: String,
    pub database_id: String,
    pub name: String,
    pub view_type: ViewType,
    pub visible_properties: Vec<String>,  // property IDs
    pub property_widths: HashMap<String, u32>,  // width in pixels
    pub filters: Vec<Filter>,
    pub sorts: Vec<Sort>,
    pub is_default: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ViewType {
    Table(TableConfig),
    Board(BoardConfig),
    List(ListConfig),
    Calendar(CalendarConfig),
    Gallery(GalleryConfig),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TableConfig {
    pub frozen_columns: Vec<String>,  // property IDs
    pub row_height: u32,  // pixels
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BoardConfig {
    pub group_by: String,        // property ID (must be Select/MultiSelect/Status)
    pub collapse_groups: Vec<String>,  // option IDs that are collapsed
    pub card_size: CardSize,     // compact, medium, large
}

pub enum CardSize {
    Compact,
    Medium,
    Large,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListConfig {
    pub preview_property: String,  // Shows below title in list item
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CalendarConfig {
    pub date_property: String,  // property ID (must be Date type)
    pub show_weekend: bool,
    pub first_day_of_week: u32,  // 0 = Sunday, 1 = Monday
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GalleryConfig {
    pub cover_property: String,     // property ID (must be Files type)
    pub title_property: String,     // property ID (typically Title)
    pub card_size: CardSize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Filter {
    pub property: String,
    pub operator: FilterOperator,
    pub values: Vec<serde_json::Value>,
}

pub enum FilterOperator {
    Is,
    IsNot,
    Contains,
    DoesNotContain,
    StartsWith,
    EndsWith,
    GreaterThan,
    LessThan,
    GreaterThanOrEqual,
    LessThanOrEqual,
    IsEmpty,
    IsNotEmpty,
    DateIs,
    DateIsBefore,
    DateIsAfter,
    DateIsWithin,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Sort {
    pub property: String,
    pub direction: SortDirection,
}

pub enum SortDirection {
    Ascending,
    Descending,
}
```

### View Persistence

Views are stored in `.bases` [views] section (shown above in file format). Each view is independently versioned and can be created/modified without affecting others.

---

## 6. Formula Engine

### Syntax & Specification

Nexus uses a Notion-compatible formula language (subset):

```
// Text
prop("Title")
"Hello"
concat(prop("First Name"), " ", prop("Last Name"))
upper(prop("Name"))
lower(prop("Name"))
trim(prop("Description"))
len(prop("Tags"))
replace(prop("Name"), "old", "new")
slice(prop("Name"), 0, 5)

// Numeric
prop("Effort")
1 + 2
prop("Score") * 2
pow(prop("Base"), 2)
sqrt(prop("Value"))
abs(prop("Delta"))
round(prop("Average"), 2)
min(prop("A"), prop("B"))
max(prop("A"), prop("B"))

// Date
prop("Due Date")
now()
dateAdd(prop("Due Date"), 1, "days")
dateBetween(prop("Due Date"), now(), "days")
year(prop("Created Time"))
month(prop("Created Time"))
day(prop("Created Time"))

// Conditional
if(prop("Status") == "Done", "Completed", "In Progress")
switch(prop("Priority"), "High", "🔴", "Medium", "🟡", "Low", "🟢", "Unknown")

// Logical
and(prop("Status") != "Done", prop("Due Date") < now())
or(prop("Status") == "Blocked", prop("Assignee") == empty())
not(prop("Checkbox"))

// Collections (for rollups, relations)
map([1, 2, 3], prop("X") * 2)  // [2, 4, 6]
filter([records], prop("Status") == "Done")
sort([records], prop("Due Date"), "asc")
count([related_records])

// Type conversion
toNumber(prop("Status"))
toDate("2026-04-15")
toString(prop("Effort"))
```

### Parser & Type System

```rust
#[derive(Debug, Clone)]
pub enum FormulaValue {
    Text(String),
    Number(f64),
    Date(DateTime<Utc>),
    Boolean(bool),
    List(Vec<FormulaValue>),
    Null,
}

pub struct FormulaParser {
    tokens: Vec<Token>,
    position: usize,
}

impl FormulaParser {
    pub fn parse(&mut self) -> Result<Expr, ParseError> {
        self.parse_expression()
    }

    fn parse_expression(&mut self) -> Result<Expr, ParseError> {
        let mut left = self.parse_term()?;
        while self.current_token().is_binary_operator() {
            let op = self.advance();
            let right = self.parse_term()?;
            left = Expr::BinaryOp { left: Box::new(left), op, right: Box::new(right) };
        }
        Ok(left)
    }
}

pub struct FormulaEvaluator {
    context: HashMap<String, FormulaValue>,
}

impl FormulaEvaluator {
    pub fn eval(&self, expr: &Expr) -> Result<FormulaValue, EvalError> {
        match expr {
            Expr::Literal(val) => Ok(val.clone()),
            Expr::PropertyRef(prop_id) => {
                self.context.get(prop_id)
                    .cloned()
                    .ok_or(EvalError::UnknownProperty)
            }
            Expr::FunctionCall { name, args } => {
                self.eval_function(name, args)
            }
            Expr::BinaryOp { left, op, right } => {
                let l = self.eval(left)?;
                let r = self.eval(right)?;
                self.eval_binary_op(&l, op, &r)
            }
            Expr::If { condition, then_expr, else_expr } => {
                let cond = self.eval(condition)?;
                if self.is_truthy(&cond) {
                    self.eval(then_expr)
                } else {
                    self.eval(else_expr)
                }
            }
        }
    }

    fn eval_function(&self, name: &str, args: &[Expr]) -> Result<FormulaValue, EvalError> {
        match name {
            "concat" => {
                let parts: Result<Vec<_>, _> = args.iter().map(|a| self.eval(a)).collect();
                let parts = parts?;
                let text = parts.iter().map(|v| v.as_string()).collect::<Result<String, _>>()?;
                Ok(FormulaValue::Text(text))
            }
            "upper" => {
                let s = self.eval(&args[0])?.as_string()?;
                Ok(FormulaValue::Text(s.to_uppercase()))
            }
            // ... 50+ more functions
            _ => Err(EvalError::UnknownFunction(name.to_string())),
        }
    }
}
```

### Error Handling

Formula errors are caught and reported to users without crashing the view:

```rust
pub enum FormulaError {
    ParseError { message: String, position: usize },
    TypeError { expected: String, got: String },
    DivisionByZero,
    InvalidDateFormat(String),
    UnknownProperty(String),
    CircularReference(Vec<String>),  // A → B → A
}

// In view rendering:
match evaluator.eval(&formula) {
    Ok(val) => render_value(&val),
    Err(e) => render_error_cell(&format!("Error: {}", e)),
}
```

---

## 7. Relations & Rollup System

### Cross-Database Relations

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Relation {
    pub id: String,
    pub source_database_id: String,
    pub target_database_id: String,
    pub source_property_id: String,  // Relation property in source
    pub target_property_id: Option<String>,  // Back-relation in target (optional)
    pub cardinality: Cardinality,
    pub created_at: DateTime<Utc>,
}

pub enum Cardinality {
    OneToMany,
    ManyToMany,
}

pub struct Rollup {
    pub id: String,
    pub relation_id: String,
    pub aggregation: AggregationFunction,
    pub source_property_id: String,
    pub filter: Option<Filter>,  // Optional: only rollup records matching filter
    pub limit: Option<u32>,      // Optional: limit aggregation to N records
}
```

### Relation Resolution

When querying a relation property, the engine:
1. Looks up related record IDs
2. Loads those records' data (lazy, on demand)
3. Returns a list of records (or single record for one-to-one)

```rust
pub fn resolve_relation(
    db: &Database,
    record_id: &str,
    relation_id: &str,
) -> Result<Vec<Record>, Error> {
    let relation = db.get_relation(relation_id)?;
    let target_db = load_database(&relation.target_database_id)?;
    
    // Query: find all records in target_db where [back_relation] references record_id
    let results = target_db.query(Query {
        filters: vec![Filter {
            property: relation.target_property_id.clone(),
            operator: FilterOperator::Is,
            values: vec![json!(record_id)],
        }],
        ..Default::default()
    })?;
    
    Ok(results)
}
```

### Rollup Aggregation

```rust
pub fn compute_rollup(
    db: &Database,
    record_id: &str,
    rollup_id: &str,
) -> Result<FormulaValue, Error> {
    let rollup = db.get_rollup(rollup_id)?;
    let related = resolve_relation(db, record_id, &rollup.relation_id)?;
    
    let values: Vec<FormulaValue> = related.iter()
        .filter_map(|r| {
            if let Some(ref filter) = rollup.filter {
                if !filter.matches(r) { return None; }
            }
            r.properties.get(&rollup.source_property_id)
                .and_then(|v| v.as_formula_value().ok())
        })
        .take(rollup.limit.unwrap_or(u32::MAX) as usize)
        .collect();
    
    match rollup.aggregation {
        AggregationFunction::Sum => {
            let sum: f64 = values.iter()
                .filter_map(|v| v.as_number().ok())
                .sum();
            Ok(FormulaValue::Number(sum))
        }
        AggregationFunction::Count => Ok(FormulaValue::Number(values.len() as f64)),
        // ... other aggregations
    }
}
```

### Circular Relation Prevention

Relations are validated to prevent A → B → A cycles:

```rust
pub fn validate_relation(db: &Database, relation: &Relation) -> Result<(), Error> {
    let mut visited = HashSet::new();
    let mut queue = vec![relation.source_database_id.clone()];
    
    while let Some(db_id) = queue.pop() {
        if visited.contains(&db_id) {
            return Err(Error::CircularRelation);
        }
        visited.insert(db_id.clone());
        
        let db = load_database(&db_id)?;
        for rel in db.relations.iter() {
            if rel.target_database_id == relation.source_database_id {
                return Err(Error::CircularRelation);
            }
            queue.push(rel.target_database_id.clone());
        }
    }
    
    Ok(())
}
```

---

## 8. Query Engine

### Query Interface

```rust
#[derive(Debug, Clone)]
pub struct Query {
    pub database_id: String,
    pub filters: Vec<Filter>,
    pub sorts: Vec<Sort>,
    pub limit: Option<u32>,
    pub offset: Option<u32>,
    pub select: Vec<String>,  // property IDs to fetch
}

pub struct QueryResult {
    pub records: Vec<Record>,
    pub total_count: u32,
    pub has_more: bool,
}

impl Database {
    pub fn query(&self, query: Query) -> Result<QueryResult, Error> {
        // 1. Compile to SQL
        let sql = compile_query_to_sql(&query)?;
        
        // 2. Execute against SQLite
        let rows = self.sqlite_conn.prepare(&sql)?
            .query_map([], |row| { ... })?;
        
        // 3. Return results with lazy-loaded bodies
        Ok(QueryResult { ... })
    }
}
```

### SQL Compilation

Filters and sorts are compiled to SQLite WHERE and ORDER BY clauses:

```rust
fn compile_filters_to_sql(filters: &[Filter]) -> String {
    filters.iter().map(|f| match f.operator {
        FilterOperator::Is => format!("{} = ?", f.property),
        FilterOperator::IsNot => format!("{} != ?", f.property),
        FilterOperator::Contains => format!("{} LIKE ?", f.property),
        FilterOperator::GreaterThan => format!("{} > ?", f.property),
        FilterOperator::LessThan => format!("{} < ?", f.property),
        FilterOperator::IsEmpty => format!("{} IS NULL", f.property),
        // ... other operators
    }).collect::<Vec<_>>().join(" AND ")
}

fn compile_sorts_to_sql(sorts: &[Sort]) -> String {
    sorts.iter().map(|s| 
        format!("{} {}", s.property, match s.direction {
            SortDirection::Ascending => "ASC",
            SortDirection::Descending => "DESC",
        })
    ).collect::<Vec<_>>().join(", ")
}
```

### Query Optimization

- **Indexes:** SQLite indexes on frequently-filtered/sorted properties (status, due_date, created_at)
- **Pagination:** LIMIT + OFFSET for large result sets
- **Lazy loading:** Record bodies loaded on-demand (only fetch .md file when viewing full record)
- **Filter pushdown:** Complex filters evaluated in SQLite before loading into memory
- **Query caching:** Recent queries cached in memory; invalidated on record modification

---

## 9. Indexing Strategy

### SQLite Schema

Each `.bases` database is indexed into SQLite:

```sql
CREATE TABLE records (
    id TEXT PRIMARY KEY,
    database_id TEXT NOT NULL,
    created_at TIMESTAMP NOT NULL,
    updated_at TIMESTAMP NOT NULL,
    deleted_at TIMESTAMP,
    -- Property values stored as JSONB (SQLite 3.38+) or TEXT
    properties_json TEXT NOT NULL,
    -- Materialized columns for common properties (for fast filtering)
    title TEXT,
    status TEXT,
    due_date DATE,
    priority TEXT,
    assignee TEXT,
    -- ... other materialized columns
    FOREIGN KEY (database_id) REFERENCES databases(id)
);

CREATE INDEX idx_records_database_id ON records(database_id);
CREATE INDEX idx_records_created_at ON records(created_at);
CREATE INDEX idx_records_updated_at ON records(updated_at);
CREATE INDEX idx_records_status ON records(status) WHERE deleted_at IS NULL;
CREATE INDEX idx_records_due_date ON records(due_date) WHERE deleted_at IS NULL;
-- ... other indexes
```

### Incremental Re-Indexing

On app startup:

```rust
pub fn ensure_index_fresh(db: &Database) -> Result<(), Error> {
    let bases_modified = fs::metadata(&db.bases_path)?.modified()?;
    let index_modified = self.sqlite_index_mtime();
    
    if bases_modified > index_modified {
        // .bases file changed; re-index
        let records = parse_bases_file(&db.bases_path)?;
        let changes = diff_records(&self.last_indexed, &records)?;
        
        for change in changes {
            match change {
                Change::Added(record) => {
                    self.sqlite_conn.execute(
                        "INSERT INTO records (id, database_id, ...) VALUES (?1, ?2, ...)",
                        &record,
                    )?;
                }
                Change::Modified(record) => {
                    self.sqlite_conn.execute(
                        "UPDATE records SET ... WHERE id = ?1",
                        &record,
                    )?;
                }
                Change::Deleted(id) => {
                    self.sqlite_conn.execute(
                        "UPDATE records SET deleted_at = NOW() WHERE id = ?1",
                        &[id],
                    )?;
                }
            }
        }
        
        self.last_indexed = records;
        self.update_index_mtime()?;
    }
    
    Ok(())
}
```

### Real-Time Updates

When a record is modified via the UI:
1. Update `.bases` file and `.md` file
2. Update SQLite index
3. Emit event (RecordUpdated)
4. All open views automatically refresh

---

## 10. Import/Export

### CSV Import

```rust
pub fn import_csv(
    db: &mut Database,
    csv_path: &Path,
    column_mapping: HashMap<String, String>,  // CSV column → property ID
) -> Result<u32, Error> {
    let reader = csv::Reader::from_path(csv_path)?;
    let mut count = 0;
    
    for result in reader.into_records() {
        let record = result?;
        let mut props = HashMap::new();
        
        for (csv_col, prop_id) in &column_mapping {
            if let Some(val) = record.get(csv_col) {
                props.insert(prop_id.clone(), parse_value_for_property(val, prop_id)?);
            }
        }
        
        db.add_record(props)?;
        count += 1;
    }
    
    db.save()?;
    Event::RecordsImported { db_id: db.id.clone(), count }.emit();
    Ok(count)
}
```

### CSV Export

```rust
pub fn export_csv(
    db: &Database,
    output_path: &Path,
    properties: Option<Vec<String>>,  // If None, export all
) -> Result<(), Error> {
    let props = properties.unwrap_or_else(|| 
        db.properties.keys().cloned().collect()
    );
    
    let mut writer = csv::Writer::from_path(output_path)?;
    
    // Header
    writer.write_record(&props)?;
    
    // Records
    for record in &db.records {
        let row: Vec<String> = props.iter().map(|prop_id| {
            record.properties.get(prop_id)
                .map(|v| v.as_csv_string())
                .unwrap_or_default()
        }).collect();
        writer.write_record(&row)?;
    }
    
    writer.flush()?;
    Ok(())
}
```

### Notion Import

A dedicated importer uses Notion's API to fetch databases and map properties:

```rust
pub struct NotionImporter {
    client: NotionClient,
}

impl NotionImporter {
    pub async fn import_database(
        &self,
        notion_db_id: &str,
    ) -> Result<Database, Error> {
        let notion_db = self.client.get_database(notion_db_id).await?;
        let mut nexus_db = Database::new(notion_db.title);
        
        // Map properties
        for (notion_prop_id, notion_prop) in notion_db.properties {
            let nexus_prop = self.map_property(&notion_prop)?;
            nexus_db.add_property(nexus_prop)?;
        }
        
        // Fetch and import records
        let mut notion_records = vec![];
        let mut has_more = true;
        let mut cursor = None;
        
        while has_more {
            let response = self.client.query_database(
                notion_db_id,
                &Default::default(),
                cursor,
            ).await?;
            
            notion_records.extend(response.results);
            has_more = response.has_more;
            cursor = response.next_cursor;
        }
        
        for notion_record in notion_records {
            let nexus_record = self.map_record(&notion_record)?;
            nexus_db.add_record(nexus_record)?;
        }
        
        Ok(nexus_db)
    }
    
    fn map_property(&self, notion_prop: &NotionProperty) -> Result<Property, Error> {
        match notion_prop.prop_type.as_str() {
            "title" => Ok(Property { type_: PropertyType::Title { .. }, .. }),
            "rich_text" => Ok(Property { type_: PropertyType::RichText { .. }, .. }),
            "checkbox" => Ok(Property { type_: PropertyType::Checkbox, .. }),
            "select" => { /* map SelectOption */ },
            // ... other types
        }
    }
}
```

### JSON Export

```rust
pub fn export_json(db: &Database, output_path: &Path) -> Result<(), Error> {
    let export = serde_json::json!({
        "name": db.name,
        "description": db.description,
        "created_at": db.created_at,
        "properties": db.properties,
        "records": db.records.iter().map(|r| {
            serde_json::json!({
                "id": r.id,
                "properties": r.properties,
                "created_at": r.created_at,
            })
        }).collect::<Vec<_>>(),
    });
    
    std::fs::write(output_path, serde_json::to_string_pretty(&export)?)?;
    Ok(())
}
```

---

## 11. Trait Definitions

### DatabaseProvider

```rust
pub trait DatabaseProvider: Send + Sync {
    fn load_database(&self, id: &str) -> Result<Database, Error>;
    fn save_database(&self, db: &Database) -> Result<(), Error>;
    fn delete_database(&self, id: &str) -> Result<(), Error>;
    fn query(&self, query: Query) -> Result<QueryResult, Error>;
    fn list_databases(&self) -> Result<Vec<DatabaseMetadata>, Error>;
}
```

### ViewRenderer

```rust
pub trait ViewRenderer {
    fn render(
        &self,
        db: &Database,
        view: &View,
        results: &QueryResult,
    ) -> Result<RenderedView, Error>;
}

pub struct RenderedView {
    pub html: String,
    pub css: String,
    pub js: String,
}
```

### FormulaEvaluator

```rust
pub trait FormulaEvaluator {
    fn parse(&self, expression: &str) -> Result<Expr, ParseError>;
    fn eval(&self, expr: &Expr, context: &Record) -> Result<FormulaValue, EvalError>;
    fn eval_string(&self, expression: &str, context: &Record) -> Result<String, Error> {
        let expr = self.parse(expression)?;
        let val = self.eval(&expr, context)?;
        Ok(val.as_string()?)
    }
}
```

---

## 12. Event Integration

Events emitted by the database engine:

```rust
pub enum DatabaseEvent {
    // Record events
    RecordCreated { db_id: String, record_id: String },
    RecordUpdated { db_id: String, record_id: String, changes: PropertyChanges },
    RecordDeleted { db_id: String, record_id: String },
    RecordsImported { db_id: String, count: u32 },
    
    // Schema events
    PropertyAdded { db_id: String, property_id: String },
    PropertyRemoved { db_id: String, property_id: String },
    PropertyTypeChanged { db_id: String, property_id: String, from: String, to: String },
    SchemaChanged { db_id: String, reason: String },
    
    // View events
    ViewCreated { db_id: String, view_id: String },
    ViewUpdated { db_id: String, view_id: String },
    ViewDeleted { db_id: String, view_id: String },
    
    // Relation events
    RelationCreated { db_id: String, relation_id: String },
    RelationDeleted { db_id: String, relation_id: String },
}

pub trait EventListener: Send + Sync {
    fn on_event(&self, event: DatabaseEvent);
}

pub static DATABASE_EVENT_BUS: Lazy<Arc<EventBus<DatabaseEvent>>> = Lazy::new(|| {
    Arc::new(EventBus::new())
});
```

---

## 13. AI Integration

The AI engine integrates with the database engine to:

### Query Databases
```rust
// AI can query databases for context
let query = Query {
    database_id: "db_projects".to_string(),
    filters: vec![
        Filter {
            property: "status",
            operator: FilterOperator::IsNot,
            values: vec![json!("Done")],
        }
    ],
    sorts: vec![Sort {
        property: "due_date",
        direction: SortDirection::Ascending,
    }],
    limit: Some(10),
    ..Default::default()
};

let results = ai_engine.query_database(query)?;
// AI can now see top 10 incomplete projects
```

### Create Records
```rust
// AI can create records based on user intent
let new_record = Record {
    properties: hashmap![
        "prop_001" => PropertyValue::Text("Implement formula engine".to_string()),
        "prop_002" => PropertyValue::Select("status_inprog".to_string()),
        "prop_005" => PropertyValue::Number(8.0),
    ],
    ..Default::default()
};

db.add_record(new_record)?;
```

### Suggest Formulas
```rust
// AI can suggest formulas based on use case
let suggestion = ai_engine.suggest_formula(
    "I want a field that calculates days until due date",
    &db,
)?;
// → "dateBetween(prop('Due Date'), now(), 'days')"
```

### Generate Views
```rust
// AI can generate views based on user request
let view = ai_engine.generate_view(
    "Create a board view grouped by priority",
    &db,
)?;
// → BoardConfig { group_by: "prop_005", .. }
```

---

## 14. Performance Targets

### Query Latency

| Database Size | Simple Query | Complex Filter | Sort + Paginate |
|---|---|---|---|
| 100 records | <10ms | <20ms | <30ms |
| 1k records | <20ms | <50ms | <75ms |
| 10k records | <50ms | <150ms | <250ms |
| 100k records | <200ms | <500ms | <1000ms |

**Optimization tactics:**
- SQLite indexes on filtered/sorted properties
- Lazy-load record bodies (don't fetch .md unless needed)
- Query result caching
- Batch index updates

### View Render Time

| View Type | 100 Records | 1k Records | 10k Records |
|---|---|---|---|
| Table | <100ms | <300ms | <1000ms |
| Board | <150ms | <500ms | <1500ms |
| Calendar | <200ms | <750ms | <2000ms |
| List | <80ms | <250ms | <800ms |
| Gallery | <300ms | <1000ms | <3000ms |

**Optimization:**
- Virtual scrolling (only render visible rows)
- Incremental re-render (only update changed cells)
- Debounced filter/sort updates

### Memory

- Open database: ~1MB base + 100KB per 1k records
- View render: ~50KB per 100 visible records
- Formula evaluation: ~10KB per active formula

---

## 15. Database Creation Flow

### Inline Database (in a note)

```
User types: /database
  ↓
Suggestion menu appears
User selects "Create database"
  ↓
Modal opens with property wizard
  ↓
User adds properties (drag to reorder)
  ↓
User clicks "Create"
  ↓
Engine generates .bases file in note folder
Engine renders database UI inline in note
```

### Standalone Database

```
User navigates to sidebar "Databases"
User clicks "+ New Database"
  ↓
Creation dialog:
  - Database name
  - Add initial properties (or start empty)
  - Select a template (optional)
  ↓
User clicks "Create"
  ↓
Engine creates new .bases file in Databases folder
Engine opens full-page database view
User can add records via "+ New Record" button
```

### From Template

```
User clicks "+ New Database" → "From Template"
  ↓
Template gallery shows:
  - Project Tracker
  - Reading List
  - Meeting Notes
  - Bug Tracker
  - CRM Pipeline
  - Event Planning
  - etc.
  ↓
User selects template
  ↓
Modal: name the database, customize properties
User clicks "Create from Template"
  ↓
Engine creates .bases with template structure
Template records (examples) included (user can delete)
```

---

## 16. View Interactions

### Table View

- **Cell editing:** Click cell → inline edit → Enter to save
- **Type detection:** Input type inferred from property type (date picker for Date, dropdown for Select)
- **Drag columns:** Resize, reorder, hide/show
- **Sort:** Click column header to sort; click again to reverse
- **Filter:** Click funnel icon → add filter conditions
- **Multi-select rows:** Shift+click to select range; bulk actions (delete, status change, etc.)

### Board (Kanban)

- **Drag cards:** Drag card to different group (updates property)
- **Drag within group:** Reorder (persisted as "manual sort" position)
- **Click card:** Inline edit or open full record modal
- **Add card:** Click "+ Add" at bottom of group
- **Collapse groups:** Click group header to collapse

### Calendar

- **Drag event:** Drag date event to different date (updates date property)
- **Click event:** Open record modal
- **Today button:** Jump to current date
- **Month/Week toggle:** Switch calendar view
- **Add event:** Click date to create record with that date

### Gallery

- **Grid layout:** Responsive grid, adjustable card size
- **Click card:** Open record modal
- **Cover image:** Displayed prominently; click to open image
- **Hover:** Show preview of title and other fields

### List

- **Click row:** Expand preview (shows preview_property text)
- **Inline edit:** Click field to edit inline
- **Drag to reorder:** Reorder (persisted as sort order)

---

## 17. Linked Databases

A user can embed a filtered view of an existing database in a different note:

```
In note, user types: /linked-database
  ↓
Selection dialog: choose database
  ↓
View selection: choose which view (or create filtered view)
  ↓
Optional: customize filters for this embedding
  ↓
Engine renders linked view inline in note
  ↓
Clicking "Edit" in linked view opens full database
Changes to database are reflected immediately
```

Implementation:

```rust
pub struct LinkedDatabase {
    pub database_id: String,
    pub view_id: String,
    pub additional_filters: Vec<Filter>,  // Applied on top of view filters
    pub max_records: Option<u32>,
}

pub fn render_linked_database(linked: &LinkedDatabase) -> Result<Html, Error> {
    let db = load_database(&linked.database_id)?;
    let view = db.get_view(&linked.view_id)?;
    
    let mut filters = view.filters.clone();
    filters.extend(linked.additional_filters.clone());
    
    let query = Query {
        database_id: linked.database_id.clone(),
        filters,
        sorts: view.sorts.clone(),
        limit: linked.max_records,
        ..Default::default()
    };
    
    let results = db.query(query)?;
    let view_renderer = get_renderer_for_type(&view.view_type);
    view_renderer.render(&db, view, &results)
}
```

---

## 18. Template System

### Database Templates

Built-in templates (stored in Nexus codebase):

```toml
[[templates]]
id = "tmpl_project_tracker"
name = "Project Tracker"
description = "Sprint planning and task management"
icon = "📋"

[templates.database]
name = "My Project"

[templates.database.properties]
title = { type = "Title" }
status = { type = "Status", options = ["To Do", "In Progress", "Done"] }
assignee = { type = "People" }
priority = { type = "Select", options = ["Low", "Medium", "High"] }
due_date = { type = "Date" }
effort = { type = "Number", format = "number" }

[templates.database.views.table_main]
type = "Table"
visible_properties = ["title", "status", "assignee", "due_date"]
sorts = [{ property = "due_date", direction = "asc" }]

[templates.database.views.board_status]
type = "Board"
group_by = "status"
```

### Record Templates

Users can define record templates within a database:

```toml
[templates]
bug_report = { 
  name = "Bug Report",
  icon = "🐛",
  default_values = {
    prop_002 = "status_todo",  # status = To Do
    prop_005 = "p_high"        # priority = High
  }
}

feature_request = {
  name = "Feature Request",
  icon = "✨",
  default_values = {
    prop_002 = "status_todo",
    prop_005 = "p_low"
  }
}
```

**Usage:**
```
User clicks "+ New Record" dropdown
Menu shows: "New Record", "Bug Report", "Feature Request", etc.
User selects "Bug Report"
  ↓
Engine creates record with default values
Record opens for editing
```

---

## Acceptance Criteria

- [x] TOML `.bases` file format specified and justified
- [x] Record storage architecture (hybrid `.md` files + SQLite index) defined
- [x] 20+ property types with Rust enum definitions
- [x] Schema management with versioning and migration
- [x] View system (Table, Board, List, Calendar, Gallery) with data models
- [x] Formula engine with Notion-compatible syntax, parser, type system
- [x] Relations & rollups for cross-database queries
- [x] Query engine with SQL compilation, optimization, pagination
- [x] Indexing strategy with SQLite schema and incremental re-indexing
- [x] Import/Export (CSV, Notion, JSON)
- [x] Trait definitions (DatabaseProvider, ViewRenderer, FormulaEvaluator)
- [x] Event integration (RecordCreated, ViewChanged, etc.)
- [x] AI integration (query, create, suggest formulas/views)
- [x] Performance targets (query latency, render time, memory)
- [x] Database creation flow (inline, standalone, from template)
- [x] View interactions (editing, dragging, filtering)
- [x] Linked databases (embedded filtered views)
- [x] Template system (database + record templates)

---

## Dependencies & Rollout

### External Dependencies
- **SQLite:** `rusqlite` crate with JSON1 extension
- **TOML:** `toml` crate
- **Date handling:** `chrono` crate
- **CSV:** `csv` crate
- **Notion API:** `notion-sdk-rs` (or custom HTTP client)
- **AI engine:** Nexus AI subsystem (separate PRD)

### Phase 1 (Week 1-2)
- Property type system + validation
- TOML file format + parsing
- SQLite indexing layer

### Phase 2 (Week 3-4)
- Record CRUD operations
- View system architecture
- Query engine (filters, sorts, pagination)

### Phase 3 (Week 5-6)
- Formula engine (parser + evaluator)
- Relations & rollups
- CSV/JSON import/export

### Phase 4 (Week 7-8)
- View renderers (Table, Board, Calendar, Gallery, List)
- UI interactions (cell editing, dragging, inline editing)
- Notion importer

### Phase 5 (Week 9-10)
- AI integration
- Database + record templates
- Performance optimization & testing

---

## Success Metrics

- Query latency <100ms for typical queries (10k records)
- Formula evaluation <50ms per formula
- Database opens in <500ms
- CSV import/export handles 10k+ records without OOM
- Support for 100k+ records with graceful degradation
- Zero data loss on app crash/corruption (ACID compliance via SQLite)

---

**Version:** 1.0  
**Status:** Ready for Engineering  
**Last Updated:** April 11, 2026
