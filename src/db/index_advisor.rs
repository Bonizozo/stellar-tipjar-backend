use crate::db::query_analyzer::QueryPlan;

#[derive(Debug, PartialEq)]
pub struct IndexRecommendation {
    pub table: String,
    pub column: String,
    pub reason: String,
}

pub struct IndexAdvisor;

impl IndexAdvisor {
    /// Extracts table/column hints from sequential scan nodes in the plan.
    pub fn recommend(plan: &QueryPlan) -> Vec<IndexRecommendation> {
        if !plan.has_seq_scan {
            return vec![];
        }

        plan.raw
            .lines()
            .filter(|l| l.contains("Seq Scan on"))
            .filter_map(|l| {
                // "Seq Scan on <table>"
                let table = l.split("Seq Scan on").nth(1)?.split_whitespace().next()?.to_string();
                Some(IndexRecommendation {
                    table: table.clone(),
                    column: "—".to_string(),
                    reason: format!("Sequential scan detected on `{table}`; consider adding an index on the filtered column"),
                })
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::query_analyzer::QueryPlan;

    fn make_plan(raw: &str) -> QueryPlan {
        QueryPlan {
            raw: raw.to_string(),
            has_seq_scan: raw.contains("Seq Scan"),
            estimated_cost: 0.0,
        }
    }

    #[test]
    fn test_recommends_for_seq_scan() {
        let plan = make_plan("Seq Scan on tips  (cost=0.00..10.00 rows=100 width=50)\n  Filter: (creator_id = $1)");
        let recs = IndexAdvisor::recommend(&plan);
        assert_eq!(recs.len(), 1);
        assert_eq!(recs[0].table, "tips");
    }

    #[test]
    fn test_no_recommendation_for_index_scan() {
        let plan = make_plan("Index Scan using tips_pkey on tips  (cost=0.15..8.17 rows=1 width=50)");
        assert!(IndexAdvisor::recommend(&plan).is_empty());
    }
}
