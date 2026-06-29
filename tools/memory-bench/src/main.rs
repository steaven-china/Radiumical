use std::process::Command;

fn main() {
    println!("=== Radiumical Main Program Memory Analysis ===\n");

    // ── 1. Binary size ──
    let exe = std::env::current_exe().unwrap();
    let meta = std::fs::metadata(&exe).unwrap();
    println!("Binary: {} ({:.1} MB)\n", exe.display(), meta.len() as f64 / 1_048_576.0);

    // ── 2. Simulate session load ──
    use radiumical_core::types::{Message, MessageContent, Role, FunctionCall, ToolCall};
    use std::time::Instant;

    let t0 = Instant::now();

    // Simulate a 200-message conversation with tool calls
    let mut messages: Vec<Message> = Vec::with_capacity(200);
    for i in 0..200 {
        match i % 5 {
            0 => messages.push(Message {
                role: Role::User,
                content: MessageContent::from_text(format!(
                    "Help me fix the bug in file {}.rs where the function returns wrong value",
                    ["main", "lib", "utils", "config", "types"][i % 5]
                )),
                tool_calls: None,
                tool_call_id: None,
                name: None,
                reasoning_content: None,
            }),
            1 => messages.push(Message {
                role: Role::Assistant,
                content: MessageContent::from_text(format!(
                    "Let me read the file first. I'll use the read_file tool.\n\n{}",
                    "Based on my analysis, the issue is in the error handling. The function \
                     doesn't properly handle the None case, which causes a panic downstream. \
                     Here's what I found:\n\n\
                     1. Line 42: `unwrap()` on Option that can be None\n\
                     2. Line 67: Missing error propagation with `?`\n\
                     3. Line 89: Inconsistent return type\n\n\
                     I'll fix these issues now.".repeat(1 + i % 3)
                )),
                tool_calls: Some(vec![
                    ToolCall {
                        id: format!("call_{i}"),
                        call_type: "function".into(),
                        function: FunctionCall {
                            name: "read_file".into(),
                            arguments: format!(r#"{{"path":"src/{}.rs","start":1,"end":100}}"#, 
                                ["main", "lib", "utils", "config", "types"][i % 5]),
                        },
                    },
                ]),
                tool_call_id: None,
                name: None,
                reasoning_content: Some(format!(
                    "The user wants to fix a bug. Let me analyze the code. \
                     I need to check error handling patterns and find the root cause. \
                     The most likely issue is improper Option/Result handling.",
                )),
            }),
            2 => messages.push(Message {
                role: Role::Tool,
                content: MessageContent::from_text(format!(
                    "File: src/{}.rs\nLines 1-100:\n\n{}\n\n// ... ({} more lines)",
                    ["main", "lib", "utils", "config", "types"][i % 5],
                    "use std::collections::HashMap;\n\
                     use anyhow::Result;\n\n\
                     pub fn process(data: &str) -> Result<String> {\n\
                         let parsed = parse(data)?;\n\
                         let validated = validate(parsed)?;\n\
                         let result = transform(validated)?;\n\
                         Ok(serde_json::to_string(&result)?)\n\
                     }\n\n\
                     fn parse(s: &str) -> Result<Parsed> {\n\
                         serde_json::from_str(s).map_err(|e| anyhow::anyhow!(\"parse error: {e}\"))\n\
                     }",
                    400 + i * 10
                )),
                tool_calls: None,
                tool_call_id: Some(format!("call_{i}")),
                name: Some("read_file".into()),
                reasoning_content: None,
            }),
            3 => messages.push(Message {
                role: Role::Assistant,
                content: MessageContent::from_text(format!(
                    "Found the issue. Here's the fix:\n\n\
                     ```rust\n\
                     // Before (line 42):\n\
                     let value = data.unwrap();\n\
                     \n\
                     // After:\n\
                     let value = data.ok_or_else(|| anyhow::anyhow!(\"missing data\"))?;\n\
                     ```\n\n\
                     I'll apply this change now."
                )),
                tool_calls: Some(vec![
                    ToolCall {
                        id: format!("call_{}", i + 1000),
                        call_type: "function".into(),
                        function: FunctionCall {
                            name: "edit_file".into(),
                            arguments: format!(
                                r#"{{"path":"src/{}.rs","old":"let value = data.unwrap();","new":"let value = data.ok_or_else(|| anyhow::anyhow!(\"missing data\"))?;"}}"#,
                                ["main", "lib", "utils", "config", "types"][i % 5]
                            ),
                        },
                    },
                ]),
                tool_call_id: None,
                name: None,
                reasoning_content: None,
            }),
            _ => messages.push(Message {
                role: Role::Tool,
                content: MessageContent::from_text(format!(
                    "Edit applied successfully to src/{}.rs",
                    ["main", "lib", "utils", "config", "types"][i % 5]
                )),
                tool_calls: None,
                tool_call_id: Some(format!("call_{}", i + 1000)),
                name: Some("edit_file".into()),
                reasoning_content: None,
            }),
        }
    }

    let build_time = t0.elapsed();

    // ── 3. Measure sizes ──
    let total_raw: usize = messages.iter().map(|m| m.content.raw_str().len()).sum();
    let total_json: usize = messages.iter().map(|m| serde_json::to_string(m).unwrap().len()).sum();
    let compressed_count = messages.iter().filter(|m| m.content.is_compressed()).count();

    // zstd of the full conversation
    let mut jsonl_buf = Vec::new();
    for msg in &messages {
        let json = serde_json::to_string(msg).unwrap();
        jsonl_buf.extend_from_slice(json.as_bytes());
        jsonl_buf.push(b'\n');
    }
    let zst_buf = zstd::encode_all(jsonl_buf.as_slice(), 3).unwrap();

    println!("=== Simulated Session: {} messages ===", messages.len());
    println!("  Build time:       {:.1}ms", build_time.as_secs_f64() * 1000.0);
    println!();
    println!("  lz4 compressed:   {compressed_count} / {} messages", messages.len());
    println!("  Raw text total:   {:.1} KB", total_raw as f64 / 1024.0);
    println!("  JSON serialized:  {:.1} KB", total_json as f64 / 1024.0);
    println!("  zstd JSONL:       {:.1} KB", zst_buf.len() as f64 / 1024.0);
    println!(
        "  zstd ratio:       {:.1}x smaller than plain JSONL",
        jsonl_buf.len() as f64 / zst_buf.len() as f64
    );
    println!();

    // ── 4. Process memory (Windows) ──
    #[cfg(windows)]
    {
        use std::mem;
        #[repr(C)]
        struct PROCESS_MEMORY_COUNTERS {
            cb: u32,
            page_fault_count: u32,
            peak_working_set_size: usize,
            working_set_size: usize,
            quota_peak_paged_pool_usage: usize,
            quota_paged_pool_usage: usize,
            quota_peak_nonpaged_pool_usage: usize,
            quota_nonpaged_pool_usage: usize,
            pagefile_usage: usize,
            peak_pagefile_usage: usize,
        }
        extern "system" {
            fn GetCurrentProcess() -> *mut core::ffi::c_void;
            fn GetProcessMemoryInfo(
                process: *mut core::ffi::c_void,
                ppsmemcounters: *mut PROCESS_MEMORY_COUNTERS,
                cb: u32,
            ) -> i32;
        }
        unsafe {
            let mut counters: PROCESS_MEMORY_COUNTERS = mem::zeroed();
            counters.cb = mem::size_of::<PROCESS_MEMORY_COUNTERS>() as u32;
            if GetProcessMemoryInfo(GetCurrentProcess(), &mut counters, counters.cb) != 0 {
                println!("=== Process Memory (Windows) ===");
                println!("  Working set:      {:.1} MB", counters.working_set_size as f64 / 1_048_576.0);
                println!("  Peak working set: {:.1} MB", counters.peak_working_set_size as f64 / 1_048_576.0);
                println!("  Pagefile usage:   {:.1} MB", counters.pagefile_usage as f64 / 1_048_576.0);
            }
        }
    }

    // ── 5. Estimate Vec<Message> heap ──
    let vec_cap = messages.capacity() * std::mem::size_of::<Message>();
    let msg_heap: usize = messages.iter().map(|m| {
        let content_heap = m.content.raw_str().len();
        let reasoning_heap = m.reasoning_content.as_ref().map(|s| s.len()).unwrap_or(0);
        let tool_calls_heap = m.tool_calls.as_ref().map(|calls| {
            calls.iter().map(|c| c.id.len() + c.call_type.len() + c.function.name.len() + c.function.arguments.len()).sum::<usize>()
        }).unwrap_or(0);
        let id_heap = m.tool_call_id.as_ref().map(|s| s.len()).unwrap_or(0);
        let name_heap = m.name.as_ref().map(|s| s.len()).unwrap_or(0);
        content_heap + reasoning_heap + tool_calls_heap + id_heap + name_heap
    }).sum::<usize>();

    println!();
    println!("=== Vec<Message> Heap Estimate ===");
    println!("  Vec overhead:     {:.1} KB ({} slots × {}B)", vec_cap as f64 / 1024.0, messages.capacity(), std::mem::size_of::<Message>());
    println!("  String data:      {:.1} KB", msg_heap as f64 / 1024.0);
    println!("  Total estimate:   {:.1} KB", (vec_cap + msg_heap) as f64 / 1024.0);
    println!();
    println!("  Note: lz4 compression reduces String heap by ~90% for >1KB texts.");
    println!("  Effective heap (with lz4): ~{:.1} KB", (vec_cap + msg_heap / 10) as f64 / 1024.0);
}
