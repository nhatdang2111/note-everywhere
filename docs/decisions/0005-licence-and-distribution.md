# 0005 — Giấy phép và phân phối

- Trạng thái: đã chốt (người dùng quyết định ngày 2026-11-21)

## Quyết định
- Giấy phép MIT.
- Dự án không có chi phí cố định: không Apple Developer Program, không chứng chỉ ký code Windows, không mua tên miền.
- Bản phát hành là bộ cài không ký trên GitHub Releases kèm checksum SHA-256, cùng hướng dẫn tự biên dịch từ mã nguồn.
- Không thu thập telemetry.

## Hệ quả
- macOS Gatekeeper và Windows SmartScreen sẽ cảnh báo ở lần mở đầu; README phải hướng dẫn cách cho phép.
- Không dùng được các API cần entitlement trả phí của Apple. Vì vậy đăng nhập dùng tài khoản và mật khẩu, vault dùng mật khẩu cấp 2, không có passkey hay sinh trắc học.
- Giấy phép của mọi phụ thuộc phải tương thích với MIT; `deny.toml` liệt kê các giấy phép được phép và CI chạy `cargo deny`.

## Chưa kiểm chứng
`cargo deny` chưa được chạy trên máy phát triển. Nó sẽ chạy lần đầu trong CI khi repo có remote; danh sách giấy phép trong `deny.toml` có thể cần bổ sung khi đó.
