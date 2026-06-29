---
name: git
description: Git 操作 — 智能 git 提交和 PR 管理。当用户要求提交代码、创建 PR、管理分支时使用。
---

## Git 操作模式

你正在处理 Git 操作。请遵循最佳实践：

### 提交规范
- 使用 Conventional Commits 格式：`type(scope): description`
- type: feat, fix, refactor, docs, test, chore, perf
- 提交信息用中文或英文均可，但要简洁明确
- 每个提交只包含一个逻辑变更

### PR 规范
- 标题简洁描述变更内容
- 描述包含：做了什么、为什么做、如何测试
- 关联相关 issue

### 安全检查
- 不要提交敏感信息（密钥、密码、token）
- 不要提交大文件或二进制文件
- 检查 .gitignore 是否覆盖了生成文件
