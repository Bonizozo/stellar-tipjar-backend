use sqlx::PgPool;

#[derive(Debug)]
pub struct QueryPlan {
    pub raw: String,
    pub has_seq_scan: bool,
    pub estimated_cost: f64,
}

impl QueryPlan {
    fn parse(rows: Vec<String>) -> Self {
        let raw = rows.join("\n");
        let has_seq_scan = raw.contains("Seq Scan");
        let estimated_cost = raw
            .lines()
            .find(|l| l.contains("cost="))
            .and_then(|l| {
                let start = l.find("cost=")? + 5;
                let end = l[start..].find("..")?;
                l[start..start + end].parse().ok()
            })
            .unwrap_or(0.0);

        QueryPlan { raw, has_seq_scan, estimated_cost }
    }
}

pub struct QueryAnalyzer {
    pool: PgPool,
}

impl QueryAnalyzer {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub async fn analyze(&self, query: &str) -> Result<QueryPlan, sqlx::Error> {
        let rows: Vec<(String,)> = sqlx::query_as(&format!("EXPLAIN ANALYZE {query}"))
            .fetch_all(&self.pool)
            .await?;
        Ok(QueryPlan::parse(rows.into_iter().map(|(s,)| s).collect()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_plan_parse_seq_scan() {
        let rows = vec![
            "Seq Scan on creators  (cost=0.00..1.01 rows=1 width=100)".to_string(),
            "  Filter: (username = 'alice')".to_string(),
        ];
        let plan = QueryPlan::parse(rows);
        assert!(plan.has_seq_scan);
        assert_eq!(plan.estimated_cost, 0.0);
    }

    #[test]
    fn test_plan_parse_no_seq_scan() {
        let rows = vec![
            "Index Scan using creators_username_idx on creators  (cost=0.15..8.17 rows=1 width=100)".to_string(),
        ];
        let plan = QueryPlan::parse(rows);
        assert!(!plan.has_seq_scan);
        assert_eq!(plan.estimated_cost, 0.15);
    }
}
