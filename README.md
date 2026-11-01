# 久坐提醒

一个很轻的 Windows 久坐提醒小工具：坐满设定时间后全屏提醒你起来活动，平时安静地待在托盘里。

**[⬇ 下载 SitReminder.exe](https://github.com/hahq/sit-reminder/releases/latest/download/SitReminder.exe)**

单个 exe（约 300 KB），无需安装，双击即用；常驻内存约 3 MB。

## 功能

- 倒计时提醒，默认每 45 分钟提醒一次，休息 5 分钟
- 到点全屏提醒，附带活动建议；可“稍后提醒”（Esc）或“跳过本次”
- 离开电脑超过休息时长，自动重新计时
- 托盘图标：绿色计时中 / 灰色已暂停 / 橙色休息中，悬停显示剩余时间，右键菜单可暂停、重新计时、退出
- 记录今天完成的休息次数

## 使用

- 关闭主窗口会缩到托盘，后台继续计时；右键托盘图标 → 退出
- 设置保存在 exe 同目录的 `config.ini`
- `--hidden` 参数启动时直接缩在托盘

**开机自启**：按 `Win + R` 输入 `shell:startup` 回车，在打开的文件夹里新建 exe 的快捷方式，并在快捷方式“目标”末尾加上 ` --hidden`。

> 首次运行时 Windows SmartScreen 可能提示“未知发布者”，点“更多信息 → 仍要运行”即可（程序未做代码签名）。

## 从源码构建

需要 [Rust](https://rustup.rs/)：

```bash
cd rust
cargo build --release
```

产物在 `rust/target/release/sit_reminder.exe`。

推送 `v*` 标签（如 `v1.0.0`）会由 GitHub Actions 自动构建并发布到 Releases。
