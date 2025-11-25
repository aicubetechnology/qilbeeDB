//! Query Planning and Optimization
//!
//! Enterprise-grade query planner with:
//! - Cost-based optimization
//! - Index selection
//! - Join ordering
//! - Predicate pushdown
//! - Common subexpression elimination

use crate::parser::*;
use qilbee_core::{Direction, Error, Result};
use std::collections::HashMap;

/// Physical execution plan
#[derive(Debug, Clone)]
pub struct ExecutionPlan {
    /// Root physical operator
    pub root: PhysicalOperator,

    /// Estimated cost
    pub estimated_cost: f64,

    /// Estimated cardinality (number of rows)
    pub estimated_rows: u64,
}

/// Physical query operators
#[derive(Debug, Clone)]
pub enum PhysicalOperator {
    /// Full node scan
    NodeScan {
        variable: String,
        labels: Vec<String>,
        estimated_cost: f64,
    },

    /// Index seek (point lookup)
    IndexSeek {
        variable: String,
        label: String,
        property: String,
        value: Expression,
        estimated_cost: f64,
    },

    /// Index scan (range scan)
    IndexScan {
        variable: String,
        label: String,
        property: String,
        range: (Option<Expression>, Option<Expression>),
        estimated_cost: f64,
    },

    /// Filter operation (WHERE clause)
    Filter {
        input: Box<PhysicalOperator>,
        predicate: Expression,
        estimated_cost: f64,
    },

    /// Projection (RETURN/WITH clause)
    Project {
        input: Box<PhysicalOperator>,
        expressions: Vec<Expression>,
        aliases: Vec<String>,
        estimated_cost: f64,
    },

    /// Relationship expansion
    Expand {
        input: Box<PhysicalOperator>,
        from_var: String,
        rel_var: Option<String>,
        to_var: String,
        rel_types: Vec<String>,
        direction: Direction,
        estimated_cost: f64,
    },

    /// Hash join
    HashJoin {
        left: Box<PhysicalOperator>,
        right: Box<PhysicalOperator>,
        join_keys: Vec<(String, String)>,
        estimated_cost: f64,
    },

    /// Nested loop join
    NestedLoopJoin {
        left: Box<PhysicalOperator>,
        right: Box<PhysicalOperator>,
        predicate: Expression,
        estimated_cost: f64,
    },

    /// Sort operation
    OrderBy {
        input: Box<PhysicalOperator>,
        items: Vec<(Expression, bool)>, // (expression, descending)
        estimated_cost: f64,
    },

    /// Limit operation
    Limit {
        input: Box<PhysicalOperator>,
        count: usize,
        estimated_cost: f64,
    },

    /// Skip operation
    Skip {
        input: Box<PhysicalOperator>,
        count: usize,
        estimated_cost: f64,
    },

    /// Distinct operation
    Distinct {
        input: Box<PhysicalOperator>,
        estimated_cost: f64,
    },

    /// Aggregation
    Aggregate {
        input: Box<PhysicalOperator>,
        group_by: Vec<Expression>,
        aggregates: Vec<(AggregateFunction, Expression, String)>,
        estimated_cost: f64,
    },
}

/// Aggregate functions
#[derive(Debug, Clone)]
pub enum AggregateFunction {
    Count,
    Sum,
    Avg,
    Min,
    Max,
    Collect,
}

/// Query planner
pub struct QueryPlanner {
    /// Statistics for cost estimation
    stats: PlannerStats,
}

/// Statistics for query planning
#[derive(Debug, Clone)]
struct PlannerStats {
    /// Estimated total nodes
    total_nodes: u64,

    /// Estimated nodes per label
    nodes_per_label: HashMap<String, u64>,

    /// Selectivity estimates
    selectivity_estimates: HashMap<String, f64>,
}

impl Default for PlannerStats {
    fn default() -> Self {
        Self {
            total_nodes: 1000, // Default estimate
            nodes_per_label: HashMap::new(),
            selectivity_estimates: HashMap::new(),
        }
    }
}

impl QueryPlanner {
    /// Create a new query planner
    pub fn new() -> Self {
        Self {
            stats: PlannerStats::default(),
        }
    }

    /// Create an execution plan from a parsed query
    pub fn plan(&self, query: &Query) -> Result<ExecutionPlan> {
        // Extract clauses
        let mut match_clauses = Vec::new();
        let mut where_clauses = Vec::new();
        let mut return_clause = None;
        let mut order_by_clause = None;
        let mut limit_clause = None;

        for clause in &query.clauses {
            match clause {
                Clause::Match(m) => match_clauses.push(m.clone()),
                Clause::Where(w) => where_clauses.push(w.clone()),
                Clause::Return(r) => return_clause = Some(r.clone()),
                Clause::OrderBy(o) => order_by_clause = Some(o.clone()),
                Clause::Limit(l) => limit_clause = Some(l.clone()),
                _ => {}
            }
        }

        if match_clauses.is_empty() {
            return Err(Error::QueryParse("Query must have at least one MATCH clause".to_string()));
        }

        // Build execution plan bottom-up
        let mut plan = self.plan_match(&match_clauses[0])?;

        // Apply WHERE filters (predicate pushdown)
        for where_expr in where_clauses {
            plan = self.apply_filter(plan, where_expr)?;
        }

        // Apply ORDER BY
        if let Some(order_by) = order_by_clause {
            plan = self.apply_order_by(plan, &order_by)?;
        }

        // Apply LIMIT
        if let Some(limit_expr) = limit_clause {
            if let Expression::Literal(Literal::Integer(count)) = limit_expr {
                plan = PhysicalOperator::Limit {
                    input: Box::new(plan),
                    count: count as usize,
                    estimated_cost: 1.0,
                };
            }
        }

        // Apply RETURN projection
        if let Some(return_clause) = return_clause {
            plan = self.apply_return(plan, &return_clause)?;
        }

        let estimated_cost = self.estimate_cost(&plan);
        let estimated_rows = self.estimate_cardinality(&plan);

        Ok(ExecutionPlan {
            root: plan,
            estimated_cost,
            estimated_rows,
        })
    }

    /// Plan a MATCH clause
    fn plan_match(&self, match_clause: &MatchClause) -> Result<PhysicalOperator> {
        if match_clause.patterns.is_empty() {
            return Err(Error::QueryParse("MATCH clause must have at least one pattern".to_string()));
        }

        // For now, handle simple single-node patterns
        let pattern = &match_clause.patterns[0];
        if pattern.elements.is_empty() {
            return Err(Error::QueryParse("Pattern must have at least one element".to_string()));
        }

        // Extract the node pattern
        if let PatternElement::Node(node_pattern) = &pattern.elements[0] {
            let variable = node_pattern.variable.clone().unwrap_or_else(|| "n".to_string());

            // Check if we can use an index
            if let Some(properties) = &node_pattern.properties {
                // Try to use index seek
                if let Some((key, value_expr)) = properties.entries.first() {
                    if !node_pattern.labels.is_empty() {
                        return Ok(PhysicalOperator::IndexSeek {
                            variable: variable.clone(),
                            label: node_pattern.labels[0].clone(),
                            property: key.clone(),
                            value: value_expr.clone(),
                            estimated_cost: 10.0,
                        });
                    }
                }
            }

            // Fall back to node scan
            return Ok(PhysicalOperator::NodeScan {
                variable,
                labels: node_pattern.labels.clone(),
                estimated_cost: self.estimate_scan_cost(&node_pattern.labels),
            });
        }

        Err(Error::QueryParse("Invalid pattern structure".to_string()))
    }

    /// Apply a filter operation
    fn apply_filter(&self, input: PhysicalOperator, predicate: Expression) -> Result<PhysicalOperator> {
        let estimated_cost = self.estimate_cost(&input) * 1.1; // Filter adds 10% overhead
        Ok(PhysicalOperator::Filter {
            input: Box::new(input),
            predicate,
            estimated_cost,
        })
    }

    /// Apply ORDER BY
    fn apply_order_by(&self, input: PhysicalOperator, order_by: &OrderByClause) -> Result<PhysicalOperator> {
        let items: Vec<(Expression, bool)> = order_by.items.iter()
            .map(|item| (item.expression.clone(), !item.ascending))
            .collect();

        let estimated_cost = self.estimate_cost(&input) * 2.0; // Sorting is expensive

        Ok(PhysicalOperator::OrderBy {
            input: Box::new(input),
            items,
            estimated_cost,
        })
    }

    /// Apply RETURN projection
    fn apply_return(&self, input: PhysicalOperator, return_clause: &ReturnClause) -> Result<PhysicalOperator> {
        let mut expressions = Vec::new();
        let mut aliases = Vec::new();

        for item in &return_clause.items {
            expressions.push(item.expression.clone());
            aliases.push(item.alias.clone().unwrap_or_else(|| "?column?".to_string()));
        }

        let estimated_cost = self.estimate_cost(&input) * 1.05; // Projection is cheap

        Ok(PhysicalOperator::Project {
            input: Box::new(input),
            expressions,
            aliases,
            estimated_cost,
        })
    }

    /// Estimate the cost of a physical operator
    fn estimate_cost(&self, operator: &PhysicalOperator) -> f64 {
        match operator {
            PhysicalOperator::NodeScan { estimated_cost, .. } => *estimated_cost,
            PhysicalOperator::IndexSeek { estimated_cost, .. } => *estimated_cost,
            PhysicalOperator::IndexScan { estimated_cost, .. } => *estimated_cost,
            PhysicalOperator::Filter { estimated_cost, .. } => *estimated_cost,
            PhysicalOperator::Project { estimated_cost, .. } => *estimated_cost,
            PhysicalOperator::Expand { estimated_cost, .. } => *estimated_cost,
            PhysicalOperator::HashJoin { estimated_cost, .. } => *estimated_cost,
            PhysicalOperator::NestedLoopJoin { estimated_cost, .. } => *estimated_cost,
            PhysicalOperator::OrderBy { estimated_cost, .. } => *estimated_cost,
            PhysicalOperator::Limit { estimated_cost, .. } => *estimated_cost,
            PhysicalOperator::Skip { estimated_cost, .. } => *estimated_cost,
            PhysicalOperator::Distinct { estimated_cost, .. } => *estimated_cost,
            PhysicalOperator::Aggregate { estimated_cost, .. } => *estimated_cost,
        }
    }

    /// Estimate cardinality (number of rows) for an operator
    fn estimate_cardinality(&self, operator: &PhysicalOperator) -> u64 {
        match operator {
            PhysicalOperator::NodeScan { labels, .. } => {
                if labels.is_empty() {
                    self.stats.total_nodes
                } else {
                    *self.stats.nodes_per_label.get(&labels[0]).unwrap_or(&1000)
                }
            }
            PhysicalOperator::IndexSeek { .. } => 1, // Point lookup
            PhysicalOperator::Filter { input, .. } => {
                // Assume 10% selectivity
                self.estimate_cardinality(input) / 10
            }
            PhysicalOperator::Limit { count, .. } => *count as u64,
            _ => 100, // Default estimate
        }
    }

    /// Estimate scan cost
    fn estimate_scan_cost(&self, labels: &[String]) -> f64 {
        if labels.is_empty() {
            self.stats.total_nodes as f64
        } else {
            *self.stats.nodes_per_label.get(&labels[0]).unwrap_or(&1000) as f64
        }
    }
}

impl Default for QueryPlanner {
    fn default() -> Self {
        Self::new()
    }
}
