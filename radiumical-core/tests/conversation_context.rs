//! Integration tests for conversation context and compression.

use radiumical_core::conversation::Conversation;
use radiumical_core::types::{MessageContent, Role};

#[test]
fn conversation_push_and_build_context() {
    let mut conv = Conversation::new("You are a test assistant.".into(), None);
    conv.push_user("hello");
    conv.push_assistant("hi there", None, None);

    let msgs = conv.build_context("what's up?", None);
    // Should have: system prompt + workspace outline + previous user/assistant + new user
    assert!(msgs.len() >= 3);
    // Last message should be the new user message
    let last = msgs.last().unwrap();
    assert_eq!(last.role, Role::User);
    match &last.content {
        MessageContent::Text(s) => assert!(s.contains("what's up?")),
        _ => panic!("expected text"),
    }
}

#[test]
fn conversation_compress_range_replaces_messages() {
    let mut conv = Conversation::new("system".into(), None);
    conv.push_user("msg1");
    conv.push_assistant("reply1", None, None);
    conv.push_user("msg2");
    conv.push_assistant("reply2", None, None);
    conv.push_user("msg3");
    conv.push_assistant("reply3", None, None);

    let before = conv.messages().len();
    conv.compress_range(3, "[Compressed: 2 messages]".into());
    let after = conv.messages().len();
    assert!(after < before);
}

#[test]
fn conversation_estimate_tokens_grows_with_content() {
    let mut conv = Conversation::new("system".into(), None);
    let t0 = conv.estimate_tokens();
    conv.push_user(&"a".repeat(1000));
    let t1 = conv.estimate_tokens();
    assert!(t1 > t0);
}

#[tokio::test]
async fn conversation_zstd_jsonl_roundtrip() {
    use tempfile::tempdir;
    let dir = tempdir().unwrap();
    let path = dir.path().join("test_conv.jsonl.zst");

    // Write
    {
        let mut conv = Conversation::new("system".into(), Some(path.clone()));
        conv.push_user("hello");
        conv.push_assistant("world", None, None);
        let _handle = conv.spawn_flush_task();
        // Give flush task time to drain
        tokio::time::sleep(std::time::Duration::from_millis(600)).await;
    }

    // Read back
    let loaded = Conversation::load_jsonl(&path);
    assert!(loaded.is_some());
    let messages = loaded.unwrap();
    assert!(messages.len() >= 2);
}
