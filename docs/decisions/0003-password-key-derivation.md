# 0003 — Dẫn xuất khoá từ mật khẩu

- Trạng thái: thiết kế đã chốt; tham số Argon2id tạm thời, chờ đo trên Windows

## Quyết định
Client chạy Argon2id trên mật khẩu với salt của tài khoản, rồi dùng HKDF-SHA256 tách thành hai khoá 32 byte:
- **Khoá xác thực** gửi lên server để đăng nhập. Server chỉ lưu bản băm của nó.
- **Khoá mã hoá** ở lại máy, dùng để mở khoá gốc tài khoản.

Mật khẩu không rời máy. Biết khoá xác thực không suy ra được khoá mã hoá. Cài đặt ở `crates/ne-crypto/src/password.rs`.

Mật khẩu cấp 2 của vault dùng cùng hàm với salt riêng.

## Tham số
Mặc định tạm thời: bộ nhớ 256 MiB, 3 vòng, 4 làn. Tham số được lưu cạnh salt để nâng lên sau mà không khoá tài khoản cũ.

## Số đo
`cargo run --release -p ne-crypto --example argon2_bench`, Apple M4 Pro, 24 GB RAM, ngày 2026-11-21, trung vị của 3 lần:

| Bộ nhớ (MiB) | Vòng | Làn | Thời gian (ms) |
|---|---|---|---|
| 19 | 2 | 1 | 17 |
| 46 | 1 | 1 | 16 |
| 64 | 3 | 4 | 68 |
| 128 | 3 | 4 | 147 |
| 256 | 3 | 4 | 311 |

## Chưa kiểm chứng
- Chưa đo trên Windows và trên máy yếu. Mục tiêu là 0,5–1 giây trên máy yếu nhất được hỗ trợ; nếu 256 MiB vượt quá thì hạ xuống.
- Crate `argon2` chạy các làn tuần tự khi không bật tính năng song song, nên số làn hiện chỉ ảnh hưởng tới kết quả băm, không tăng tốc.
