#[test]
fn test_full_table_debug() {
    let input = vec![
        "| 能力 | 说明 |".to_string(),
        "|------|------|".to_string(),
        "| 代码阅读与理解 | 读取文件、搜索代码（正则/grep）、浏览目录结构 |".to_string(),
        "| 代码编辑与修改 | 精确编辑文件（`edit_file`）、创建/覆写文件 |".to_string(),
        "| 命令执行与运行 | 运行构建、测试、linting 等各种 shell 命令 |".to_string(),
        "| 诊断检查与修复 | 自动检测 Rust、Python、JS/TS、Go 等语言的项目错误和警告 |".to_string(),
        "| 项目管理与规划 | 任务列表（todo）、计划（plan）、目标分解（goal） |".to_string(),
        "| 并行处理与加速 | 可派生子代理（sub-agent）并行处理多个独立任务 |".to_string(),
        "| 浏览器自动化 | 通过 Playwright 截图、抓取网页内容、模拟点击 |".to_string(),
        "| 持久记忆与上下文 | 跨会话的记忆系统（核心/次要/短期三层） |".to_string(),
        "| 系统信息与环境 | 查看操作系统、CPU、内存、磁盘等信息 |".to_string(),
    ];
    let blocks = crate::layout::measure_blocks(&input, 80);
    let block = &blocks[0];
    println!("measure height: {}", block.height);
    if let crate::layout::BlockKind::Table { rows: _, widths, sep_idx: _ } = &block.kind {
        println!("widths: {:?}", widths);
        let total = widths.iter().sum::<usize>() + widths.len() * 3 + 1;
        println!("total: {}", total);
    }
    let mut md = crate::markdown::MarkdownRenderer::new();
    let lines = block.render(80, 0, &mut md, false);
    println!("render lines: {}", lines.len());
    for (i, line) in lines.iter().enumerate() {
        println!("[{}] {}", i, line.spans.iter().map(|s| s.content.as_ref()).collect::<String>());
    }
}
