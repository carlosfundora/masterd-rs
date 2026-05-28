use criterion::{Criterion, black_box, criterion_group, criterion_main};
use rusqlite::{Connection, params};

fn bench_tx_execute(c: &mut Criterion) {
    c.bench_function("tx_execute", |b| {
        b.iter_with_setup(
            || {
                let mut conn = Connection::open_in_memory().unwrap();
                conn.execute(
                    "CREATE TABLE test (id INTEGER PRIMARY KEY, provider TEXT, dim INTEGER, vector_json TEXT, vector_hash TEXT)",
                    [],
                ).unwrap();
                conn
            },
            |mut conn| {
                let tx = conn.transaction().unwrap();
                for i in 0..100 {
                    tx.execute(
                        "INSERT OR REPLACE INTO test(id, provider, dim, vector_json, vector_hash) VALUES (?1, ?2, ?3, ?4, ?5)",
                        params![i, "test_provider", 128, "[]", "hash"],
                    ).unwrap();
                }
                tx.commit().unwrap();
            },
        );
    });
}

fn bench_tx_prepare(c: &mut Criterion) {
    c.bench_function("tx_prepare", |b| {
        b.iter_with_setup(
            || {
                let mut conn = Connection::open_in_memory().unwrap();
                conn.execute(
                    "CREATE TABLE test (id INTEGER PRIMARY KEY, provider TEXT, dim INTEGER, vector_json TEXT, vector_hash TEXT)",
                    [],
                ).unwrap();
                conn
            },
            |mut conn| {
                let mut tx = conn.transaction().unwrap();
                {
                    let mut stmt = tx.prepare(
                        "INSERT OR REPLACE INTO test(id, provider, dim, vector_json, vector_hash) VALUES (?1, ?2, ?3, ?4, ?5)"
                    ).unwrap();
                    for i in 0..100 {
                        stmt.execute(params![i, "test_provider", 128, "[]", "hash"]).unwrap();
                    }
                }
                tx.commit().unwrap();
            },
        );
    });
}

criterion_group!(benches, bench_tx_execute, bench_tx_prepare);
criterion_main!(benches);
