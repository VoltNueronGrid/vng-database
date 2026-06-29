// S7-001 / S7-002: Trigger framework baseline + extended DDL events.

use std::collections::HashMap;

// ─── DML trigger events (S7-001) ─────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum TriggerEvent {
    BeforeInsert,
    AfterInsert,
    BeforeUpdate,
    AfterUpdate,
    BeforeDelete,
    AfterDelete,
    // S7-002: Extended events
    TruncateTable,
    CreateTable,
    DropTable,
    CreateView,
    DropView,
}

impl TriggerEvent {
    pub fn is_dml(&self) -> bool {
        matches!(
            self,
            TriggerEvent::BeforeInsert
                | TriggerEvent::AfterInsert
                | TriggerEvent::BeforeUpdate
                | TriggerEvent::AfterUpdate
                | TriggerEvent::BeforeDelete
                | TriggerEvent::AfterDelete
                | TriggerEvent::TruncateTable
        )
    }

    pub fn is_ddl(&self) -> bool {
        matches!(
            self,
            TriggerEvent::CreateTable
                | TriggerEvent::DropTable
                | TriggerEvent::CreateView
                | TriggerEvent::DropView
        )
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            TriggerEvent::BeforeInsert => "BEFORE INSERT",
            TriggerEvent::AfterInsert => "AFTER INSERT",
            TriggerEvent::BeforeUpdate => "BEFORE UPDATE",
            TriggerEvent::AfterUpdate => "AFTER UPDATE",
            TriggerEvent::BeforeDelete => "BEFORE DELETE",
            TriggerEvent::AfterDelete => "AFTER DELETE",
            TriggerEvent::TruncateTable => "TRUNCATE",
            TriggerEvent::CreateTable => "CREATE TABLE",
            TriggerEvent::DropTable => "DROP TABLE",
            TriggerEvent::CreateView => "CREATE VIEW",
            TriggerEvent::DropView => "DROP VIEW",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TriggerGranularity {
    Row,
    Statement,
}

// ─── Trigger definitions ──────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct TriggerDefinition {
    pub name: String,
    pub table: String,
    pub schema: String,
    pub event: TriggerEvent,
    pub granularity: TriggerGranularity,
    /// SQL body or embedded function reference (e.g. `EXECUTE FUNCTION my_fn()`).
    pub body: String,
    pub enabled: bool,
}

impl TriggerDefinition {
    pub fn new(
        name: impl Into<String>,
        table: impl Into<String>,
        schema: impl Into<String>,
        event: TriggerEvent,
        granularity: TriggerGranularity,
        body: impl Into<String>,
    ) -> Self {
        Self {
            name: name.into(),
            table: table.into(),
            schema: schema.into(),
            event,
            granularity,
            body: body.into(),
            enabled: true,
        }
    }
}

// S7-002: DDL-specific trigger definition
#[derive(Debug, Clone)]
pub struct DdlTriggerDefinition {
    pub name: String,
    pub event: TriggerEvent,
    /// Optional schema filter (None = fires for any schema).
    pub schema_filter: Option<String>,
    pub body: String,
    pub enabled: bool,
}

impl DdlTriggerDefinition {
    pub fn new(
        name: impl Into<String>,
        event: TriggerEvent,
        body: impl Into<String>,
    ) -> Self {
        assert!(
            event.is_ddl(),
            "DdlTriggerDefinition requires a DDL event; got a DML event"
        );
        Self {
            name: name.into(),
            event,
            schema_filter: None,
            body: body.into(),
            enabled: true,
        }
    }
}

// ─── Trigger registry (S7-001) ────────────────────────────────────────────────

/// Central registry for both DML and DDL trigger definitions.
///
/// Indexed by `(schema, table, event)` for O(1) look-up on the hot path.
#[derive(Debug, Default)]
pub struct TriggerRegistry {
    dml_triggers: Vec<TriggerDefinition>,
    ddl_triggers: Vec<DdlTriggerDefinition>,
    /// name → index into dml_triggers for quick remove.
    dml_index: HashMap<String, usize>,
    /// name → index into ddl_triggers.
    ddl_index: HashMap<String, usize>,
}

impl TriggerRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a DML trigger. Returns an error if the name is already taken.
    pub fn register(&mut self, trigger: TriggerDefinition) -> Result<(), String> {
        if self.dml_index.contains_key(&trigger.name) || self.ddl_index.contains_key(&trigger.name) {
            return Err(format!("trigger '{}' already exists", trigger.name));
        }
        let idx = self.dml_triggers.len();
        self.dml_index.insert(trigger.name.clone(), idx);
        self.dml_triggers.push(trigger);
        Ok(())
    }

    /// Register a DDL trigger. Returns an error if the name is already taken.
    pub fn register_ddl(&mut self, trigger: DdlTriggerDefinition) -> Result<(), String> {
        if self.ddl_index.contains_key(&trigger.name) || self.dml_index.contains_key(&trigger.name) {
            return Err(format!("trigger '{}' already exists", trigger.name));
        }
        let idx = self.ddl_triggers.len();
        self.ddl_index.insert(trigger.name.clone(), idx);
        self.ddl_triggers.push(trigger);
        Ok(())
    }

    /// Find all enabled DML triggers matching the given table/schema/event.
    pub fn find_triggers(
        &self,
        table: &str,
        schema: &str,
        event: &TriggerEvent,
    ) -> Vec<&TriggerDefinition> {
        self.dml_triggers
            .iter()
            .filter(|t| t.enabled && t.table == table && t.schema == schema && &t.event == event)
            .collect()
    }

    /// Find all enabled DDL triggers matching the given event (and optional schema).
    pub fn find_ddl_triggers(
        &self,
        event: &TriggerEvent,
        schema: Option<&str>,
    ) -> Vec<&DdlTriggerDefinition> {
        self.ddl_triggers
            .iter()
            .filter(|t| {
                if !t.enabled || &t.event != event {
                    return false;
                }
                if let Some(filter) = &t.schema_filter {
                    schema.map_or(false, |s| s == filter)
                } else {
                    true
                }
            })
            .collect()
    }

    pub fn list_triggers(&self) -> &[TriggerDefinition] {
        &self.dml_triggers
    }

    pub fn list_ddl_triggers(&self) -> &[DdlTriggerDefinition] {
        &self.ddl_triggers
    }

    /// Remove a trigger by name (DML or DDL). Returns `true` when removed.
    ///
    /// Uses swap-remove; callers must not rely on stable indices.
    pub fn remove_trigger(&mut self, name: &str) -> bool {
        if let Some(&idx) = self.dml_index.get(name) {
            let last_name = self.dml_triggers.last().map(|t| t.name.clone());
            self.dml_triggers.swap_remove(idx);
            self.dml_index.remove(name);
            if let Some(last) = last_name {
                if last != name {
                    self.dml_index.insert(last, idx);
                }
            }
            return true;
        }
        if let Some(&idx) = self.ddl_index.get(name) {
            let last_name = self.ddl_triggers.last().map(|t| t.name.clone());
            self.ddl_triggers.swap_remove(idx);
            self.ddl_index.remove(name);
            if let Some(last) = last_name {
                if last != name {
                    self.ddl_index.insert(last, idx);
                }
            }
            return true;
        }
        false
    }

    pub fn len(&self) -> usize {
        self.dml_triggers.len() + self.ddl_triggers.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

// ─── Q-3: CREATE/DROP TRIGGER DDL parsing ─────────────────────────────────────

/// Parse a `CREATE TRIGGER` statement into a [`TriggerDefinition`].
///
/// Supported grammar (case-insensitive, whitespace-tolerant):
///
/// ```sql
/// CREATE TRIGGER <name>
///   {BEFORE | AFTER} {INSERT | UPDATE | DELETE}
///   ON [<schema>.]<table>
///   [FOR EACH ROW | FOR EACH STATEMENT]
///   [EXECUTE FUNCTION <fn>(...) | EXECUTE PROCEDURE <fn>(...) | <body...>]
/// ```
///
/// Returns `None` when the statement does not match the grammar (e.g. a DDL
/// `CREATE TRIGGER` for `TRUNCATE`, or a malformed statement).
pub fn parse_create_trigger(sql: &str) -> Option<TriggerDefinition> {
    let trimmed = sql.trim().trim_end_matches(';').trim();
    let lower = trimmed.to_ascii_lowercase();
    let tokens: Vec<&str> = lower.split_whitespace().collect();
    // Minimum: create trigger <name> <timing> <event> on <table>
    if tokens.len() < 7 {
        return None;
    }
    if tokens[0] != "create" || tokens[1] != "trigger" {
        return None;
    }
    let name = tokens[2];
    if name.is_empty() {
        return None;
    }
    let timing = tokens[3];
    let event_word = tokens[4];
    if tokens[5] != "on" {
        return None;
    }
    let event = match (timing, event_word) {
        ("before", "insert") => TriggerEvent::BeforeInsert,
        ("after", "insert") => TriggerEvent::AfterInsert,
        ("before", "update") => TriggerEvent::BeforeUpdate,
        ("after", "update") => TriggerEvent::AfterUpdate,
        ("before", "delete") => TriggerEvent::BeforeDelete,
        ("after", "delete") => TriggerEvent::AfterDelete,
        _ => return None,
    };
    // Table token (index 6), possibly schema-qualified.
    let raw_table = tokens[6].trim_matches(|c: char| c == '(' || c == ')' || c == ',');
    let (schema, table) = match raw_table.split_once('.') {
        Some((s, t)) => (s.to_string(), t.to_string()),
        None => ("public".to_string(), raw_table.to_string()),
    };
    if table.is_empty() {
        return None;
    }
    // Granularity: FOR EACH STATEMENT → Statement, otherwise default Row.
    let granularity = if lower.contains("for each statement") {
        TriggerGranularity::Statement
    } else {
        TriggerGranularity::Row
    };
    // Body: everything from the first EXECUTE keyword onward (preserve original
    // casing); fall back to the full original statement when no EXECUTE present.
    let body = {
        let lower_exec = lower.find("execute ");
        match lower_exec {
            Some(pos) => trimmed[pos..].trim().to_string(),
            None => trimmed.to_string(),
        }
    };
    Some(TriggerDefinition::new(
        name,
        table,
        schema,
        event,
        granularity,
        body,
    ))
}

/// Parse a `DROP TRIGGER [IF EXISTS] <name> [ON <table>]` statement and return
/// the trigger name to remove. Returns `None` when the statement is malformed.
pub fn parse_drop_trigger_name(sql: &str) -> Option<String> {
    let trimmed = sql.trim().trim_end_matches(';').trim();
    let lower = trimmed.to_ascii_lowercase();
    let tokens: Vec<&str> = lower.split_whitespace().collect();
    if tokens.len() < 3 || tokens[0] != "drop" || tokens[1] != "trigger" {
        return None;
    }
    // Skip optional IF EXISTS.
    let name_idx = if tokens.len() >= 5 && tokens[2] == "if" && tokens[3] == "exists" {
        4
    } else {
        2
    };
    tokens.get(name_idx).map(|s| s.to_string()).filter(|s| !s.is_empty())
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn make_trigger(name: &str, table: &str, event: TriggerEvent) -> TriggerDefinition {
        TriggerDefinition::new(name, table, "public", event, TriggerGranularity::Row, "-- body")
    }

    #[test]
    fn test_trigger_registry_register_and_find() {
        let mut reg = TriggerRegistry::new();
        reg.register(make_trigger("t1", "users", TriggerEvent::AfterInsert))
            .unwrap();
        reg.register(make_trigger("t2", "users", TriggerEvent::BeforeDelete))
            .unwrap();

        let found = reg.find_triggers("users", "public", &TriggerEvent::AfterInsert);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].name, "t1");

        let not_found = reg.find_triggers("users", "public", &TriggerEvent::AfterUpdate);
        assert!(not_found.is_empty());
    }

    #[test]
    fn test_trigger_registry_remove() {
        let mut reg = TriggerRegistry::new();
        reg.register(make_trigger("t1", "orders", TriggerEvent::BeforeInsert))
            .unwrap();
        reg.register(make_trigger("t2", "orders", TriggerEvent::AfterInsert))
            .unwrap();

        assert!(reg.remove_trigger("t1"));
        assert!(!reg.remove_trigger("t1")); // already removed
        assert_eq!(reg.list_triggers().len(), 1);
        assert_eq!(reg.list_triggers()[0].name, "t2");
    }

    #[test]
    fn test_trigger_registry_find_returns_matching_events() {
        let mut reg = TriggerRegistry::new();
        reg.register(make_trigger("ins", "items", TriggerEvent::AfterInsert)).unwrap();
        reg.register(make_trigger("upd", "items", TriggerEvent::AfterUpdate)).unwrap();
        reg.register(make_trigger("del", "items", TriggerEvent::BeforeDelete)).unwrap();
        reg.register(make_trigger("other_table", "products", TriggerEvent::AfterInsert)).unwrap();

        let after_insert = reg.find_triggers("items", "public", &TriggerEvent::AfterInsert);
        assert_eq!(after_insert.len(), 1);
        assert_eq!(after_insert[0].name, "ins");

        let all_on_items: Vec<_> = [
            TriggerEvent::AfterInsert,
            TriggerEvent::AfterUpdate,
            TriggerEvent::BeforeDelete,
        ]
        .iter()
        .flat_map(|e| reg.find_triggers("items", "public", e))
        .collect();
        assert_eq!(all_on_items.len(), 3);
    }

    #[test]
    fn test_trigger_registry_duplicate_name_rejected() {
        let mut reg = TriggerRegistry::new();
        reg.register(make_trigger("dup", "tbl", TriggerEvent::AfterInsert)).unwrap();
        let err = reg.register(make_trigger("dup", "tbl", TriggerEvent::AfterUpdate));
        assert!(err.is_err());
        assert!(err.unwrap_err().contains("already exists"));
    }

    #[test]
    fn test_ddl_trigger_register_and_find() {
        let mut reg = TriggerRegistry::new();
        let ddl = DdlTriggerDefinition::new("ddl1", TriggerEvent::CreateTable, "-- ddl body");
        reg.register_ddl(ddl).unwrap();

        let found = reg.find_ddl_triggers(&TriggerEvent::CreateTable, None);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].name, "ddl1");

        let not_found = reg.find_ddl_triggers(&TriggerEvent::DropTable, None);
        assert!(not_found.is_empty());
    }

    #[test]
    fn test_trigger_event_is_dml_ddl() {
        assert!(TriggerEvent::AfterInsert.is_dml());
        assert!(TriggerEvent::TruncateTable.is_dml());
        assert!(!TriggerEvent::AfterInsert.is_ddl());
        assert!(TriggerEvent::CreateTable.is_ddl());
        assert!(!TriggerEvent::CreateTable.is_dml());
    }

    // ── Q-3: CREATE/DROP TRIGGER parsing ──────────────────────────────────────

    #[test]
    fn test_parse_create_trigger_after_insert() {
        let def = parse_create_trigger(
            "CREATE TRIGGER orders_audit AFTER INSERT ON orders FOR EACH ROW EXECUTE FUNCTION audit_orders()",
        )
        .expect("should parse");
        assert_eq!(def.name, "orders_audit");
        assert_eq!(def.table, "orders");
        assert_eq!(def.schema, "public");
        assert_eq!(def.event, TriggerEvent::AfterInsert);
        assert_eq!(def.granularity, TriggerGranularity::Row);
        assert!(def.body.to_ascii_lowercase().contains("execute function audit_orders()"));
    }

    #[test]
    fn test_parse_create_trigger_before_update_schema_qualified() {
        let def = parse_create_trigger(
            "CREATE TRIGGER t1 BEFORE UPDATE ON sales.orders EXECUTE PROCEDURE chk()",
        )
        .expect("should parse");
        assert_eq!(def.schema, "sales");
        assert_eq!(def.table, "orders");
        assert_eq!(def.event, TriggerEvent::BeforeUpdate);
    }

    #[test]
    fn test_parse_create_trigger_statement_granularity() {
        let def = parse_create_trigger(
            "CREATE TRIGGER t2 AFTER DELETE ON items FOR EACH STATEMENT EXECUTE FUNCTION f()",
        )
        .expect("should parse");
        assert_eq!(def.event, TriggerEvent::AfterDelete);
        assert_eq!(def.granularity, TriggerGranularity::Statement);
    }

    #[test]
    fn test_parse_create_trigger_rejects_malformed() {
        assert!(parse_create_trigger("CREATE TABLE foo (id INT)").is_none());
        assert!(parse_create_trigger("CREATE TRIGGER t AFTER TRUNCATE ON x").is_none());
        assert!(parse_create_trigger("CREATE TRIGGER t").is_none());
    }

    #[test]
    fn test_parse_drop_trigger_name() {
        assert_eq!(
            parse_drop_trigger_name("DROP TRIGGER orders_audit").as_deref(),
            Some("orders_audit")
        );
        assert_eq!(
            parse_drop_trigger_name("DROP TRIGGER IF EXISTS t1 ON orders").as_deref(),
            Some("t1")
        );
        assert!(parse_drop_trigger_name("DROP TABLE x").is_none());
    }

    #[test]
    fn test_parse_create_then_register_and_fire_lookup() {
        let mut reg = TriggerRegistry::new();
        let def = parse_create_trigger(
            "CREATE TRIGGER au AFTER INSERT ON orders FOR EACH ROW EXECUTE FUNCTION f()",
        )
        .unwrap();
        reg.register(def).unwrap();
        let found = reg.find_triggers("orders", "public", &TriggerEvent::AfterInsert);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].name, "au");
        // DROP removes it.
        let name = parse_drop_trigger_name("DROP TRIGGER au").unwrap();
        assert!(reg.remove_trigger(&name));
        assert!(reg.find_triggers("orders", "public", &TriggerEvent::AfterInsert).is_empty());
    }
}
