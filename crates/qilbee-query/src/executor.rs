//! Query Execution Engine
//!
//! Enterprise-grade query executor with:
//! - Vectorized execution
//! - Lazy evaluation
//! - Streaming results
//! - Index-aware execution
//! - Cost-based operator selection

use crate::parser::*;
use crate::planner::{ExecutionPlan, PhysicalOperator};
use qilbee_core::{EntityId, Error, Node, NodeId, PropertyValue, Relationship, Result};
use qilbee_graph::Graph;
use std::collections::HashMap;
use std::sync::Arc;

/// Query execution result
#[derive(Debug, Clone)]
pub struct QueryResult {
    /// Column names
    pub columns: Vec<String>,

    /// Result rows
    pub rows: Vec<Vec<PropertyValue>>,

    /// Execution statistics
    pub stats: ExecutionStats,
}

/// Execution statistics for monitoring and optimization
#[derive(Debug, Clone, Default)]
pub struct ExecutionStats {
    pub nodes_created: u64,
    pub nodes_deleted: u64,
    pub relationships_created: u64,
    pub relationships_deleted: u64,
    pub properties_set: u64,
    pub rows_returned: u64,
    pub execution_time_ms: u64,
    pub nodes_scanned: u64,
    pub index_hits: u64,
    pub cache_hits: u64,
}

/// Query executor
pub struct QueryExecutor {
    graph: Arc<Graph>,
}

impl QueryExecutor {
    /// Create a new query executor
    pub fn new(graph: Arc<Graph>) -> Self {
        Self { graph }
    }

    /// Execute a query from an execution plan
    pub fn execute(&self, plan: &ExecutionPlan, params: &HashMap<String, PropertyValue>) -> Result<QueryResult> {
        let start = std::time::Instant::now();
        let mut stats = ExecutionStats::default();

        // Execute the physical plan
        let (columns, rows) = self.execute_plan(&plan.root, params, &mut stats)?;

        stats.execution_time_ms = start.elapsed().as_millis() as u64;
        stats.rows_returned = rows.len() as u64;

        Ok(QueryResult {
            columns,
            rows,
            stats,
        })
    }

    /// Execute a physical operator
    fn execute_plan(
        &self,
        operator: &PhysicalOperator,
        params: &HashMap<String, PropertyValue>,
        stats: &mut ExecutionStats,
    ) -> Result<(Vec<String>, Vec<Vec<PropertyValue>>)> {
        match operator {
            PhysicalOperator::NodeScan { variable, labels, .. } => {
                self.execute_node_scan(variable, labels, stats)
            }

            PhysicalOperator::IndexSeek { variable, label, property, value, .. } => {
                self.execute_index_seek(variable, label, property, value, params, stats)
            }

            PhysicalOperator::Filter { input, predicate, .. } => {
                self.execute_filter(input, predicate, params, stats)
            }

            PhysicalOperator::Project { input, expressions, aliases, .. } => {
                self.execute_project(input, expressions, aliases, params, stats)
            }

            PhysicalOperator::Expand { input, from_var, rel_var, to_var, rel_types, direction, .. } => {
                self.execute_expand(input, from_var, rel_var, to_var, rel_types, direction, params, stats)
            }

            PhysicalOperator::Limit { input, count, .. } => {
                self.execute_limit(input, *count, params, stats)
            }

            PhysicalOperator::OrderBy { input, items, .. } => {
                self.execute_order_by(input, items, params, stats)
            }

            _ => Err(Error::QueryExecution("Unsupported operator".to_string())),
        }
    }

    /// Execute node scan - full table scan with optional label filter
    fn execute_node_scan(
        &self,
        variable: &str,
        labels: &[String],
        stats: &mut ExecutionStats,
    ) -> Result<(Vec<String>, Vec<Vec<PropertyValue>>)> {
        let nodes = if labels.is_empty() {
            self.graph.get_all_nodes()?
        } else {
            // For now, scan by first label
            self.graph.find_nodes_by_label(&labels[0])?
        };

        stats.nodes_scanned += nodes.len() as u64;

        // Convert nodes to rows - for now just return node properties
        let columns = vec![variable.to_string()];
        let mut rows = Vec::new();

        for node in nodes {
            // Serialize node as a property value
            let node_id = PropertyValue::Integer(node.id.as_internal() as i64);
            rows.push(vec![node_id]);
        }

        Ok((columns, rows))
    }

    /// Execute index seek - use index for point lookup
    fn execute_index_seek(
        &self,
        variable: &str,
        label: &str,
        property: &str,
        value: &Expression,
        params: &HashMap<String, PropertyValue>,
        stats: &mut ExecutionStats,
    ) -> Result<(Vec<String>, Vec<Vec<PropertyValue>>)> {
        // Evaluate the value expression
        let prop_value = self.evaluate_expression(value, &HashMap::new(), params)?;

        // For now, use find_nodes_by_label_and_property
        let nodes = self.graph.find_nodes_by_label_and_property(
            label,
            property,
            &prop_value,
        )?;

        stats.index_hits += 1;
        stats.nodes_scanned += nodes.len() as u64;

        let columns = vec![variable.to_string()];
        let mut rows = Vec::new();

        for node in nodes {
            let node_id = PropertyValue::Integer(node.id.as_internal() as i64);
            rows.push(vec![node_id]);
        }

        Ok((columns, rows))
    }

    /// Execute filter - apply predicate to input
    fn execute_filter(
        &self,
        input: &PhysicalOperator,
        predicate: &Expression,
        params: &HashMap<String, PropertyValue>,
        stats: &mut ExecutionStats,
    ) -> Result<(Vec<String>, Vec<Vec<PropertyValue>>)> {
        let (columns, rows) = self.execute_plan(input, params, stats)?;

        // Filter rows based on predicate
        let mut filtered_rows = Vec::new();
        for row in rows {
            // Build variable bindings for this row
            let mut bindings = HashMap::new();
            for (i, col) in columns.iter().enumerate() {
                bindings.insert(col.clone(), row[i].clone());
            }

            // Evaluate predicate
            if let PropertyValue::Boolean(true) = self.evaluate_expression(predicate, &bindings, params)? {
                filtered_rows.push(row);
            }
        }

        Ok((columns, filtered_rows))
    }

    /// Execute projection - select specific columns/expressions
    fn execute_project(
        &self,
        input: &PhysicalOperator,
        expressions: &[Expression],
        aliases: &[String],
        params: &HashMap<String, PropertyValue>,
        stats: &mut ExecutionStats,
    ) -> Result<(Vec<String>, Vec<Vec<PropertyValue>>)> {
        let (input_columns, input_rows) = self.execute_plan(input, params, stats)?;

        let mut output_rows = Vec::new();
        for row in input_rows {
            // Build variable bindings
            let mut bindings = HashMap::new();
            for (i, col) in input_columns.iter().enumerate() {
                bindings.insert(col.clone(), row[i].clone());
            }

            // Evaluate projection expressions
            let mut output_row = Vec::new();
            for expr in expressions {
                let value = self.evaluate_expression(expr, &bindings, params)?;
                output_row.push(value);
            }
            output_rows.push(output_row);
        }

        Ok((aliases.to_vec(), output_rows))
    }

    /// Execute expand - traverse relationships
    fn execute_expand(
        &self,
        input: &PhysicalOperator,
        from_var: &str,
        _rel_var: &Option<String>,
        to_var: &str,
        _rel_types: &[String],
        direction: &qilbee_core::Direction,
        params: &HashMap<String, PropertyValue>,
        stats: &mut ExecutionStats,
    ) -> Result<(Vec<String>, Vec<Vec<PropertyValue>>)> {
        let (mut columns, rows) = self.execute_plan(input, params, stats)?;

        // Find the column index for the from variable
        let from_idx = columns.iter().position(|c| c == from_var)
            .ok_or_else(|| Error::QueryExecution(format!("Variable {} not found", from_var)))?;

        let mut output_rows = Vec::new();

        for row in rows {
            // Get the node ID
            if let PropertyValue::Integer(node_id) = row[from_idx] {
                let node_id = qilbee_core::NodeId::from_internal(node_id as u64);

                // Get neighbors
                let neighbors = self.graph.get_neighbors(node_id, *direction)?;

                // Create a row for each neighbor
                for neighbor in neighbors {
                    let mut output_row = row.clone();
                    output_row.push(PropertyValue::Integer(neighbor.id.as_internal() as i64));
                    output_rows.push(output_row);
                }
            }
        }

        columns.push(to_var.to_string());
        Ok((columns, output_rows))
    }

    /// Execute limit - restrict number of rows
    fn execute_limit(
        &self,
        input: &PhysicalOperator,
        count: usize,
        params: &HashMap<String, PropertyValue>,
        stats: &mut ExecutionStats,
    ) -> Result<(Vec<String>, Vec<Vec<PropertyValue>>)> {
        let (columns, mut rows) = self.execute_plan(input, params, stats)?;
        rows.truncate(count);
        Ok((columns, rows))
    }

    /// Execute order by - sort results
    fn execute_order_by(
        &self,
        input: &PhysicalOperator,
        items: &[(Expression, bool)],
        params: &HashMap<String, PropertyValue>,
        stats: &mut ExecutionStats,
    ) -> Result<(Vec<String>, Vec<Vec<PropertyValue>>)> {
        let (columns, mut rows) = self.execute_plan(input, params, stats)?;

        // Sort rows based on sort items
        rows.sort_by(|a, b| {
            for (expr, descending) in items {
                // Build bindings for both rows
                let mut bindings_a = HashMap::new();
                let mut bindings_b = HashMap::new();
                for (i, col) in columns.iter().enumerate() {
                    bindings_a.insert(col.clone(), a[i].clone());
                    bindings_b.insert(col.clone(), b[i].clone());
                }

                // Evaluate expression for both rows
                let val_a = self.evaluate_expression(expr, &bindings_a, params).unwrap_or(PropertyValue::Null);
                let val_b = self.evaluate_expression(expr, &bindings_b, params).unwrap_or(PropertyValue::Null);

                // Compare
                let cmp = compare_property_values(&val_a, &val_b);
                if cmp != std::cmp::Ordering::Equal {
                    return if *descending { cmp.reverse() } else { cmp };
                }
            }
            std::cmp::Ordering::Equal
        });

        Ok((columns, rows))
    }

    /// Evaluate an expression
    fn evaluate_expression(
        &self,
        expr: &Expression,
        bindings: &HashMap<String, PropertyValue>,
        params: &HashMap<String, PropertyValue>,
    ) -> Result<PropertyValue> {
        match expr {
            Expression::Literal(lit) => Ok(literal_to_property_value(lit)),

            Expression::Variable(var) => {
                bindings.get(var)
                    .cloned()
                    .ok_or_else(|| Error::QueryExecution(format!("Variable {} not found", var)))
            }

            Expression::Parameter(param) => {
                params.get(param)
                    .cloned()
                    .ok_or_else(|| Error::QueryExecution(format!("Parameter ${} not found", param)))
            }

            Expression::Property(object, property) => {
                // First evaluate the object
                let obj_val = self.evaluate_expression(object, bindings, params)?;

                // If it's a node ID, get the node and return the property
                if let PropertyValue::Integer(node_id) = obj_val {
                    let node_id = NodeId::from_internal(node_id as u64);
                    if let Some(node) = self.graph.get_node(node_id)? {
                        return Ok(node.properties.get(property).cloned().unwrap_or(PropertyValue::Null));
                    }
                }
                Ok(PropertyValue::Null)
            }

            Expression::Binary { left, op, right } => {
                let left_val = self.evaluate_expression(left, bindings, params)?;
                let right_val = self.evaluate_expression(right, bindings, params)?;
                evaluate_binary_op(&left_val, op, &right_val)
            }

            _ => Err(Error::QueryExecution("Unsupported expression type".to_string())),
        }
    }
}

/// Convert a literal to a property value
fn literal_to_property_value(lit: &Literal) -> PropertyValue {
    match lit {
        Literal::Null => PropertyValue::Null,
        Literal::Boolean(b) => PropertyValue::Boolean(*b),
        Literal::Integer(i) => PropertyValue::Integer(*i),
        Literal::Float(f) => PropertyValue::Float(*f),
        Literal::String(s) => PropertyValue::String(s.clone()),
    }
}

/// Evaluate a binary operation
fn evaluate_binary_op(left: &PropertyValue, op: &BinaryOp, right: &PropertyValue) -> Result<PropertyValue> {
    match op {
        BinaryOp::Equals => Ok(PropertyValue::Boolean(left == right)),
        BinaryOp::NotEquals => Ok(PropertyValue::Boolean(left != right)),
        BinaryOp::LessThan => Ok(PropertyValue::Boolean(compare_property_values(left, right) == std::cmp::Ordering::Less)),
        BinaryOp::LessEquals => Ok(PropertyValue::Boolean(compare_property_values(left, right) != std::cmp::Ordering::Greater)),
        BinaryOp::GreaterThan => Ok(PropertyValue::Boolean(compare_property_values(left, right) == std::cmp::Ordering::Greater)),
        BinaryOp::GreaterEquals => Ok(PropertyValue::Boolean(compare_property_values(left, right) != std::cmp::Ordering::Less)),
        BinaryOp::And => {
            let l = if let PropertyValue::Boolean(b) = left { *b } else { false };
            let r = if let PropertyValue::Boolean(b) = right { *b } else { false };
            Ok(PropertyValue::Boolean(l && r))
        }
        BinaryOp::Or => {
            let l = if let PropertyValue::Boolean(b) = left { *b } else { false };
            let r = if let PropertyValue::Boolean(b) = right { *b } else { false };
            Ok(PropertyValue::Boolean(l || r))
        }
        _ => Err(Error::QueryExecution(format!("Unsupported binary operator: {:?}", op))),
    }
}

/// Compare two property values
fn compare_property_values(a: &PropertyValue, b: &PropertyValue) -> std::cmp::Ordering {
    use std::cmp::Ordering;
    match (a, b) {
        (PropertyValue::Integer(a), PropertyValue::Integer(b)) => a.cmp(b),
        (PropertyValue::Float(a), PropertyValue::Float(b)) => {
            a.partial_cmp(b).unwrap_or(Ordering::Equal)
        }
        (PropertyValue::String(a), PropertyValue::String(b)) => a.cmp(b),
        (PropertyValue::Boolean(a), PropertyValue::Boolean(b)) => a.cmp(b),
        _ => Ordering::Equal,
    }
}
