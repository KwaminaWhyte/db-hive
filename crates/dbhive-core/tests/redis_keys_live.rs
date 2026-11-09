//! The Redis key browser's core calls against a real server on localhost.
//!
//!     cargo test -p dbhive-core --test redis_keys_live -- --ignored
//!
//! Uses logical database 15 and deletes only the keys it created.

use dbhive_core::drivers::redis::RedisDriver;
use dbhive_core::drivers::{ConnectionOptions, DatabaseDriver};

#[tokio::test]
#[ignore]
async fn scan_and_read_every_redis_type() {
    let driver = RedisDriver::connect(ConnectionOptions {
        host: "127.0.0.1".to_string(),
        port: 6379,
        username: String::new(),
        password: None,
        database: Some("15".to_string()),
        timeout: Some(3),
        require_tls: false,
    })
    .await
    .expect("connect to local Redis");

    for command in [
        "DEL dbhive-test:string dbhive-test:hash dbhive-test:list dbhive-test:set dbhive-test:zset",
        "SET dbhive-test:string hello",
        "HSET dbhive-test:hash a 1 b 2",
        "RPUSH dbhive-test:list one two three",
        "SADD dbhive-test:set red green",
        "ZADD dbhive-test:zset 2.5 beta 1 alpha",
        "EXPIRE dbhive-test:string 60",
    ] {
        driver.execute_query(command).await.expect(command);
    }

    let page = driver
        .scan_keys("dbhive-test:*", 0, 100)
        .await
        .expect("scan");
    let mut keys: Vec<(String, String)> = page
        .entries
        .iter()
        .map(|e| (e.key.clone(), e.kind.clone()))
        .collect();
    keys.sort();
    assert_eq!(
        keys,
        vec![
            ("dbhive-test:hash".to_string(), "hash".to_string()),
            ("dbhive-test:list".to_string(), "list".to_string()),
            ("dbhive-test:set".to_string(), "set".to_string()),
            ("dbhive-test:string".to_string(), "string".to_string()),
            ("dbhive-test:zset".to_string(), "zset".to_string()),
        ]
    );
    let string = page
        .entries
        .iter()
        .find(|e| e.key == "dbhive-test:string")
        .unwrap();
    assert!(string.ttl.is_some_and(|t| t > 0 && t <= 60));

    let hash = driver.key_detail("dbhive-test:hash").await.expect("hash");
    assert_eq!(hash.columns, ("field".to_string(), "value".to_string()));
    assert_eq!(
        hash.rows,
        vec![
            ("a".to_string(), "1".to_string()),
            ("b".to_string(), "2".to_string())
        ]
    );

    let list = driver.key_detail("dbhive-test:list").await.expect("list");
    assert_eq!(list.rows.len(), 3);
    assert_eq!(list.rows[2], ("2".to_string(), "three".to_string()));

    let zset = driver.key_detail("dbhive-test:zset").await.expect("zset");
    assert_eq!(zset.rows[0], ("alpha".to_string(), "1".to_string()));
    assert_eq!(zset.rows[1], ("beta".to_string(), "2.5".to_string()));

    let missing = driver.key_detail("dbhive-test:missing").await;
    assert!(missing.is_err(), "a missing key reports an error");

    driver
        .execute_query("DEL dbhive-test:string dbhive-test:hash dbhive-test:list dbhive-test:set dbhive-test:zset")
        .await
        .expect("cleanup");
}
