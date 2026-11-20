# 0002 — CRDT: Loro

- Trạng thái: tạm chọn, còn chờ spike editor

## Quyết định
Dùng Loro (crate `loro` 1.18) cho cả nội dung trang (rich text) và cây trang/thư mục (movable tree).

## Bằng chứng đã có
`crates/ne-core/tests/crdt_convergence_spike.rs` chạy qua ngày 2026-11-21:
- Hai thiết bị sửa cùng một đoạn văn khi offline rồi trao đổi update: hai bên hội tụ và giữ đủ chỉnh sửa của cả hai.
- Hai thiết bị đồng thời kéo hai trang vào nhau: sau khi sync, hai bên giống nhau và đúng một lệnh di chuyển thắng, không tạo vòng lặp.

## Chưa kiểm chứng
- Gắn Loro WASM với ProseMirror trong webview và gửi update qua IPC cho Loro native.
- Độ trễ gõ trên tài liệu 5.000 block (ngưỡng p95 dưới 16 ms).
- So sánh với Yrs/y-prosemirror về kích thước update và độ chín của binding editor.

Nếu spike editor không đạt ngưỡng độ trễ, xem lại quyết định này trước khi bắt đầu phase 3.
