# 0004 — Sync server: Cloudflare Workers gói miễn phí, một Durable Object

- Trạng thái: tạm chọn; còn hai rủi ro phải đo trên tài khoản Cloudflare thật

## Quyết định
Mỗi người tự triển khai một Worker viết bằng Rust (`worker` 0.9) vào tài khoản Cloudflare miễn phí của mình. Một Durable Object nền SQLite giữ log append-only các update đã mã hoá và chuyển tiếp giữa các thiết bị qua WebSocket có hibernation. File đính kèm nằm ở R2. Không dùng D1.

## Bằng chứng đã có
- `server/` biên dịch sang WebAssembly và chạy được dưới `wrangler dev` cục bộ ngày 2026-11-21.
- `node server/spikes/ws-relay-check.mjs`: hai kết nối, 200 update nhị phân 512 byte; bên gửi nhận đủ 200 ack có số thứ tự tăng liên tục từ 1 đến 200, bên nhận nhận đủ 200 update. Đây là số đo cục bộ, không phản ánh độ trễ hay CPU trên mạng Cloudflare.
- Hạn mức gói miễn phí đã đối chiếu với tài liệu chính thức, xem `docs/research/cloudflare-free-tier.md`. Với một người dùng ba thiết bị, không hạn mức đếm được nào bị vượt theo ước tính.

## Ràng buộc thiết kế rút ra
- Durable Object trên gói miễn phí chỉ có backend SQLite. Hạn mức mỗi ngày: 100.000 request, 13.000 GB-s, 100.000 dòng ghi; reset lúc 00:00 UTC.
- WebSocket phải được nhận qua `state.accept_web_socket` để object ngủ được; không dùng alarm hay timer làm heartbeat. Ping của ứng dụng phải dùng auto-response để không đánh thức object.
- Gom nhiều chỉnh sửa vào một lần đẩy, và giữ số lần gọi tới Durable Object cho mỗi lần đẩy ở mức một.
- File đính kèm tải thẳng lên R2 bằng URL ký sẵn, không đi qua Worker.
- Client phải gửi User-Agent mô tả ứng dụng; `workers.dev` chặn User-Agent mặc định của thư viện.
- Cần `wrangler` bản mới. Bản 4.68.1 không đóng gói được đầu ra của `worker-build` 0.9 (lỗi "Source phase imports are not available"); `npx wrangler@latest` chạy được.

## Chưa kiểm chứng
- **CPU 10 ms mỗi request trên gói miễn phí.** Chưa đo cho đường đẩy update của Rust/WASM. Đây là rủi ro lớn nhất; cần đo trên một bản triển khai thật trước phase 5.
- **Hibernation trên gói miễn phí** chưa được tài liệu xác nhận tường minh. Nếu object không ngủ, thời lượng có thể tới 85% hạn mức ngày.
- **R2 có đòi thẻ thanh toán hay không** khi chỉ dùng trong hạn mức miễn phí. Người dùng đã chấp nhận trường hợp phải thêm thẻ.
- Hành vi chính xác khi chạm hạn mức ngày của Durable Object.

## Giới hạn hiện tại của mã
Server chưa có tài khoản và phiên. Đường `/sync` trả 503 trừ khi biến `ALLOW_UNAUTHENTICATED_SPIKE` được bật cho lần chạy cục bộ. Không triển khai lên Cloudflare trước khi phase 5 và 6 thêm xác thực.
