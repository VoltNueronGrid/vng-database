#![forbid(unsafe_code)]

use std::collections::{HashMap, HashSet};

/// The type of constraint enforced on a column.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConstraintKind {
    PrimaryKey,
    Unique,
    NotNull,
    ForeignKey,
}

/// Describes a single column-level constraint.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConstraintDescriptor {
    pub name: String,
    pub table: String,
    pub column: String,
    pub kind: ConstraintKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConstraintViolation {
    PrimaryKeyDuplicate {
        constraint: String,
        value: String,
    },
    UniqueDuplicate {
        constraint: String,
        value: String,
    },
    NotNullViolation {
        constraint: String,
        column: String,
    },
    ConstraintAlreadyExists(String),
    ConstraintNotFound(String),
}

impl std::fmt::Display for ConstraintViolation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::PrimaryKeyDuplicate { constraint, value } => {
                write!(f, "primary key '{constraint}' duplicate for value '{value}'")
            }
            Self::UniqueDuplicate { constraint, value } => {
                write!(f, "unique constraint '{constraint}' violation for value '{value}'")
            }
            Self::NotNullViolation { constraint, column } => {
                write!(f, "not-null constraint '{constraint}' on column '{column}'")
            }
            Self::ConstraintAlreadyExists(name) => {
                write!(f, "constraint '{name}' already exists")
            }
            Self::ConstraintNotFound(name) => write!(f, "constraint '{name}' not found"),
        }
    }
}

/// Manages table constraints and validates mutations against them.
#[derive(Debug, Default)]
pub struct ConstraintManager {
    constraints: HashMap<String, ConstraintDescriptor>,
    /// Tracks seen values for PK / UNIQUE constraints: constraint_name → set of values
    unique_sets: HashMap<String, HashSet<String>>,
}

impl ConstraintManager {
    pub fn new() -> Self {
        Self {
            constraints: HashMap::new(),
            unique_sets: HashMap::new(),
        }
    }

    pub fn add_constraint(
        &mut self,
        descriptor: ConstraintDescriptor,
    ) -> Result<(), ConstraintViolation> {
        if self.constraints.contains_key(&descriptor.name) {
            return Err(ConstraintViolation::ConstraintAlreadyExists(
                descriptor.name.clone(),
            ));
        }
        let name = descriptor.name.clone();
        if descriptor.kind == ConstraintKind::PrimaryKey
            || descriptor.kind == ConstraintKind::Unique
        {
            self.unique_sets.insert(name.clone(), HashSet::new());
        }
        self.constraints.insert(name, descriptor);
        Ok(())
    }

    pub fn drop_constraint(
        &mut self,
        name: &str,
    ) -> Result<ConstraintDescriptor, ConstraintViolation> {
        self.unique_sets.remove(name);
        self.constraints
            .remove(name)
            .ok_or_else(|| ConstraintViolation::ConstraintNotFound(name.to_string()))
    }

    /// Validate a proposed column value against all constraints for the given table+column.
    /// `value` is `None` when the column is absent (NULL).
    pub fn validate(
        &self,
        table: &str,
        column: &str,
        value: Option<&str>,
    ) -> Result<(), ConstraintViolation> {
        for constraint in self.constraints.values() {
            if constraint.table != table || constraint.column != column {
                continue;
            }
            match constraint.kind {
                ConstraintKind::NotNull => {
                    if value.is_none() {
                        return Err(ConstraintViolation::NotNullViolation {
                            constraint: constraint.name.clone(),
                            column: column.to_string(),
                        });
                    }
                }
                ConstraintKind::PrimaryKey => {
                    if let Some(val) = value {
                        if let Some(seen) = self.unique_sets.get(&constraint.name) {
                            if seen.contains(val) {
                                return Err(ConstraintViolation::PrimaryKeyDuplicate {
                                    constraint: constraint.name.clone(),
                                    value: val.to_string(),
                                });
                            }
                        }
                    } else {
                        return Err(ConstraintViolation::NotNullViolation {
                            constraint: constraint.name.clone(),
                            column: column.to_string(),
                        });
                    }
                }
                ConstraintKind::Unique => {
                    if let Some(val) = value {
                        if let Some(seen) = self.unique_sets.get(&constraint.name) {
                            if seen.contains(val) {
                                return Err(ConstraintViolation::UniqueDuplicate {
                                    constraint: constraint.name.clone(),
                                    value: val.to_string(),
                                });
                            }
                        }
                    }
                }
                ConstraintKind::ForeignKey => {
                    // FK validation is deferred to a future slice
                }
            }
        }
        Ok(())
    }

    /// Record a value as committed for uniqueness tracking.
    /// Must be called AFTER `validate` succeeds for PK/UNIQUE columns.
    pub fn record_committed_value(&mut self, table: &str, column: &str, value: &str) {
        for constraint in self.constraints.values() {
            if constraint.table != table || constraint.column != column {
                continue;
            }
            if constraint.kind == ConstraintKind::PrimaryKey
                || constraint.kind == ConstraintKind::Unique
            {
                if let Some(seen) = self.unique_sets.get_mut(&constraint.name) {
                    seen.insert(value.to_string());
                }
            }
        }
    }

    /// Remove a previously committed value (e.g. on row delete).
    pub fn remove_committed_value(&mut self, table: &str, column: &str, value: &str) {
        for constraint in self.constraints.values() {
            if constraint.table != table || constraint.column != column {
                continue;
            }
            if constraint.kind == ConstraintKind::PrimaryKey
                || constraint.kind == ConstraintKind::Unique
            {
                if let Some(seen) = self.unique_sets.get_mut(&constraint.name) {
                    seen.remove(value);
                }
            }
        }
    }

    pub fn list_constraints(&self) -> Vec<&ConstraintDescriptor> {
        self.constraints.values().collect()
    }

    pub fn constraint_count(&self) -> usize {
        self.constraints.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pk_descriptor(name: &str) -> ConstraintDescriptor {
        ConstraintDescriptor {
            name: name.to_string(),
            table: "users".to_string(),
            column: "id".to_string(),
            kind: ConstraintKind::PrimaryKey,
        }
    }

    fn unique_descriptor(name: &str) -> ConstraintDescriptor {
        ConstraintDescriptor {
            name: name.to_string(),
            table: "users".to_string(),
            column: "email".to_string(),
            kind: ConstraintKind::Unique,
        }
    }

    fn not_null_descriptor(name: &str) -> ConstraintDescriptor {
        ConstraintDescriptor {
            name: name.to_string(),
            table: "users".to_string(),
            column: "name".to_string(),
            kind: ConstraintKind::NotNull,
        }
    }

    #[test]
    fn primary_key_rejects_duplicate() {
        let mut mgr = ConstraintManager::new();
        mgr.add_constraint(pk_descriptor("pk_users")).unwrap();
        mgr.validate("users", "id", Some("1")).unwrap();
        mgr.record_committed_value("users", "id", "1");
        let err = mgr.validate("users", "id", Some("1")).unwrap_err();
        assert_eq!(
            err,
            ConstraintViolation::PrimaryKeyDuplicate {
                constraint: "pk_users".to_string(),
                value: "1".to_string()
            }
        );
    }

    #[test]
    fn primary_key_rejects_null() {
        let mut mgr = ConstraintManager::new();
        mgr.add_constraint(pk_descriptor("pk_users")).unwrap();
        let err = mgr.validate("users", "id", None).unwrap_err();
        assert!(matches!(err, ConstraintViolation::NotNullViolation { .. }));
    }

    #[test]
    fn unique_rejects_duplicate_but_allows_null() {
        let mut mgr = ConstraintManager::new();
        mgr.add_constraint(unique_descriptor("uq_email")).unwrap();
        mgr.validate("users", "email", Some("a@b.com")).unwrap();
        mgr.record_committed_value("users", "email", "a@b.com");

        let err = mgr.validate("users", "email", Some("a@b.com")).unwrap_err();
        assert!(matches!(err, ConstraintViolation::UniqueDuplicate { .. }));

        // NULL is allowed for UNIQUE
        mgr.validate("users", "email", None).unwrap();
    }

    #[test]
    fn not_null_rejects_absent_value() {
        let mut mgr = ConstraintManager::new();
        mgr.add_constraint(not_null_descriptor("nn_name")).unwrap();
        mgr.validate("users", "name", Some("Alice")).unwrap();
        let err = mgr.validate("users", "name", None).unwrap_err();
        assert!(matches!(err, ConstraintViolation::NotNullViolation { .. }));
    }

    #[test]
    fn remove_committed_value_allows_reuse() {
        let mut mgr = ConstraintManager::new();
        mgr.add_constraint(pk_descriptor("pk_users")).unwrap();
        mgr.validate("users", "id", Some("42")).unwrap();
        mgr.record_committed_value("users", "id", "42");

        mgr.remove_committed_value("users", "id", "42");
        // Now the value should be accepted again
        mgr.validate("users", "id", Some("42")).unwrap();
    }

    #[test]
    fn constraint_lifecycle_add_and_drop() {
        let mut mgr = ConstraintManager::new();
        mgr.add_constraint(pk_descriptor("pk_users")).unwrap();
        mgr.add_constraint(not_null_descriptor("nn_name")).unwrap();
        assert_eq!(mgr.constraint_count(), 2);

        let dropped = mgr.drop_constraint("pk_users").unwrap();
        assert_eq!(dropped.kind, ConstraintKind::PrimaryKey);
        assert_eq!(mgr.constraint_count(), 1);
    }

    #[test]
    fn duplicate_constraint_name_rejected() {
        let mut mgr = ConstraintManager::new();
        mgr.add_constraint(pk_descriptor("pk_users")).unwrap();
        let err = mgr.add_constraint(pk_descriptor("pk_users")).unwrap_err();
        assert!(matches!(err, ConstraintViolation::ConstraintAlreadyExists(_)));
    }
}
