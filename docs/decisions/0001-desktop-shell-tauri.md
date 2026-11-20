# 0001 — Shell desktop: Tauri 2 với UI TypeScript

- Trạng thái: đã chốt (người dùng quyết định ngày 2026-11-21)

## Quyết định
App desktop dùng Tauri 2. Lõi Rust lo lưu trữ, CRDT, mã hoá, sync và audio. Giao diện là TypeScript chạy trong webview của hệ điều hành, khởi tạo từ template `vanilla-ts` chính thức với Vite.

## Lý do
Block editor chất lượng cao hiện chỉ có sẵn trong hệ sinh thái web (ProseMirror). Viết lại bằng GUI thuần Rust sẽ kéo dài phần editor thêm nhiều tháng. Tauri dùng webview có sẵn nên bộ cài nhỏ và dùng ít RAM hơn Electron.

## Hệ quả
- UI nằm ở `apps/desktop/src`, vỏ Rust ở `apps/desktop/src-tauri` (theo bố cục của template, thay cho `apps/desktop/ui` ghi trong plan ban đầu).
- Chưa chọn framework UI. Quyết định này để tới phase 4, sau spike editor.
- CSP được bật ngay từ đầu; cửa sổ chính không tải nội dung từ xa.

## Bằng chứng
`cargo check -p note-everywhere-desktop` và `pnpm typecheck` qua trên macOS (Apple Silicon) ngày 2026-11-21. Chưa chạy trên Windows; CI sẽ kiểm khi repo có remote.
