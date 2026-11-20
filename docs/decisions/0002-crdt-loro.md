# 0002 — CRDT: Loro

- Trạng thái: đã chốt cho nội dung trang theo [0006](./0006-page-document-structure.md) (2026-11-21), kèm điều kiện đo lại trong webview thật; cây trang vẫn dựa vào bằng chứng hội tụ bên dưới

## Quyết định
Dùng Loro (crate `loro` 1.18) cho cả nội dung trang (rich text) và cây trang/thư mục (movable tree).

## Bằng chứng đã có
`crates/ne-core/tests/crdt_convergence_spike.rs` chạy qua ngày 2026-11-21:
- Hai thiết bị sửa cùng một đoạn văn khi offline rồi trao đổi update: hai bên hội tụ và giữ đủ chỉnh sửa của cả hai.
- Hai thiết bị đồng thời kéo hai trang vào nhau: sau khi sync, hai bên giống nhau và đúng một lệnh di chuyển thắng, không tạo vòng lặp.

## Kết quả spike editor (2026-11-21)
Chi tiết, số đo và cấu trúc container ở [0006](./0006-page-document-structure.md).
- Đã kiểm chứng: ProseMirror gắn với Loro WASM bằng `loro-prosemirror` 0.4.4; bytes update và snapshot từ editor được crate `loro` 1.18 native đọc đúng (15 kiểu block, chữ có dấu, danh sách lồng, cờ to-do, mark); update do Rust ghi được editor hiển thị đúng, cả khi mở mới lẫn khi nhập lúc editor đang mở; undo chỉ hoàn tác sửa đổi của máy này.
- Độ trễ gõ trên trang 5.000 block: p95 8,65 ms, đạt ngưỡng 16 ms. Áp update từ máy khác ở cùng kích thước: p95 25,7 ms, không đạt; đạt từ khoảng 3.000 block trở xuống. Số đo là Node + jsdom trên Apple M4 Pro, chưa có layout, paint, IPC hay JavaScriptCore.
- Giới hạn của binding: block không di chuyển được như một đơn vị (xoá rồi tạo lại), đổi kiểu block mất thuộc tính của kiểu cũ, hai thiết bị không được cùng khởi tạo một trang. Cách xử lý nhỏ nhất nằm ở 0006.

## Chưa kiểm chứng
- Độ trễ trong webview thật của Tauri (WKWebView/WebView2) và trên máy Windows yếu; phải đo lại ở phase 4.
- CSP hiện tại của cửa sổ chính chặn WebAssembly; cần `script-src 'self' 'wasm-unsafe-eval'` trong `src-tauri`.
- So sánh với Yrs/`y-prosemirror` về kích thước update và độ chín của binding: chưa làm vì Loro đạt ngưỡng gõ phím; chỉ làm nếu phép đo trong webview thật thất bại.
