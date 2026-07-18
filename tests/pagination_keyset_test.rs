use chrono::{Duration, TimeZone, Utc};
use stellar_tipjar_backend::models::pagination::{KeysetCursor, PaginatedResponse};
use uuid::Uuid;

#[derive(Clone, serde::Serialize)]
struct Row {
    id: Uuid,
    created_at: chrono::DateTime<Utc>,
}

#[test]
fn signed_cursor_rejects_tampering() {
    std::env::set_var("PAGINATION_CURSOR_SECRET", "test-secret");
    let cursor = KeysetCursor::new(Utc.timestamp_opt(1_700_000_000, 0).unwrap(), Uuid::new_v4());
    let token = cursor.encode();
    assert_eq!(KeysetCursor::decode(&token).unwrap(), cursor);

    let mut tampered = token.clone().into_bytes();
    let last = tampered.len() - 1;
    tampered[last] = if tampered[last] == b'A' { b'B' } else { b'A' };
    let tampered = String::from_utf8(tampered).unwrap();
    assert!(KeysetCursor::decode(&tampered).is_err());
}

#[test]
fn concurrent_insert_during_keyset_traversal_has_no_duplicates_or_snapshot_skips() {
    let base = Utc.timestamp_opt(1_700_000_000, 0).unwrap();
    let mut rows: Vec<Row> = (0..100)
        .map(|i| Row {
            id: Uuid::new_v4(),
            created_at: base - Duration::seconds(i),
        })
        .collect();
    rows.sort_by_key(|r| (std::cmp::Reverse(r.created_at), std::cmp::Reverse(r.id)));
    let snapshot_ids: std::collections::BTreeSet<_> = rows.iter().map(|r| r.id).collect();

    let mut seen = std::collections::BTreeSet::new();
    let mut cursor = None;
    loop {
        let page: Vec<Row> = rows
            .iter()
            .filter(|r| match cursor {
                Some((ts, id)) => (r.created_at, r.id) < (ts, id),
                None => true,
            })
            .take(11)
            .cloned()
            .collect();
        let response =
            PaginatedResponse::keyset(page, 10, |r| KeysetCursor::new(r.created_at, r.id));
        for row in &response.items {
            assert!(seen.insert(row.id), "duplicate row surfaced");
        }
        if seen.len() == 20 {
            rows.push(Row {
                id: Uuid::new_v4(),
                created_at: base + Duration::seconds(10),
            });
            rows.sort_by_key(|r| (std::cmp::Reverse(r.created_at), std::cmp::Reverse(r.id)));
        }
        cursor = response.items.last().map(|r| (r.created_at, r.id));
        if !response.has_more {
            break;
        }
    }

    assert!(snapshot_ids.is_subset(&seen), "snapshot rows were skipped");
}
