//! Integration tests for the session lifecycle.

use radiumical_core::session::{SessionItem, SessionPool, SessionMode};

#[test]
fn save_load_session_roundtrip() {
    let pool = SessionPool::for_workspace("__integ_test_session__");
    let _ = pool.delete("test-roundtrip");

    let items = vec![
        SessionItem::User {
            content: "hello".into(),
        },
        SessionItem::Assistant {
            content: "world".into(),
        },
    ];

    pool.save(
        "test-roundtrip",
        &items,
        "test-model",
        "test-provider",
        SessionMode::Auto,
        "max",
        Some("integration test"),
    )
    .unwrap();

    let (meta, loaded) = pool.load("test-roundtrip").unwrap().unwrap();
    assert_eq!(meta.model, "test-model");
    assert_eq!(meta.provider, "test-provider");
    assert_eq!(meta.description, "integration test");
    assert_eq!(loaded.len(), 2);

    // Cleanup
    pool.delete("test-roundtrip").unwrap();
}

#[test]
fn list_sessions_sorted() {
    let pool = SessionPool::for_workspace("__integ_test_list__");
    let _ = pool.delete("alpha");
    let _ = pool.delete("beta");

    pool.save("alpha", &[], "m", "p", SessionMode::Auto, "max", None)
        .unwrap();
    std::thread::sleep(std::time::Duration::from_millis(50));
    pool.save("beta", &[], "m", "p", SessionMode::Plan, "high", None)
        .unwrap();

    let sessions = pool.list().unwrap();
    let names: Vec<&str> = sessions.iter().map(|s| s.name.as_str()).collect();
    assert!(names.contains(&"alpha"));
    assert!(names.contains(&"beta"));

    // Cleanup
    pool.delete("alpha").unwrap();
    pool.delete("beta").unwrap();
}

#[test]
fn items_to_messages_preserves_content() {
    let items = vec![
        SessionItem::User {
            content: "What is 2+2?".into(),
        },
        SessionItem::Assistant {
            content: "4".into(),
        },
        SessionItem::Tool {
            id: "tc_1".into(),
            name: "calculator".into(),
            args: "{}".into(),
            result: Some("4".into()),
        },
    ];

    let messages = radiumical_core::session::items_to_messages(&items);
    assert_eq!(messages.len(), 3);
    assert_eq!(messages[0].role, radiumical_core::types::Role::User);
    assert_eq!(messages[1].role, radiumical_core::types::Role::Assistant);
    assert_eq!(messages[2].role, radiumical_core::types::Role::Tool);
}
