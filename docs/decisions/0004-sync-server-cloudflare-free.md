# 0004 — Sync server: Cloudflare Workers gói miễn phí, một Durable Object

- Trạng thái: tạm chọn; đã cài đặt đầy đủ và kiểm thử cục bộ; còn hai rủi ro phải đo trên tài khoản Cloudflare thật (CPU 10 ms mỗi request và hibernation trên gói miễn phí)

## Quyết định
Mỗi người tự triển khai một Worker viết bằng Rust (`worker` 0.9) vào tài khoản Cloudflare miễn phí của mình. Một Durable Object nền SQLite giữ log append-only các update đã mã hoá và chuyển tiếp giữa các thiết bị qua WebSocket có hibernation. File đính kèm nằm ở R2. Không dùng D1.

## Quyết định bổ sung khi cài đặt
- **File đính kèm đi qua binding R2 của Worker, thay cho ý định dùng URL ký sẵn.** Lý do: URL ký sẵn đòi hỏi khoá S3 (access key và secret) do người triển khai tạo trong dashboard rồi đặt làm secret, nghĩa là thêm bước thủ công, thêm hai secret cần giữ, và thêm một chỗ lộ khoá. Với binding, Worker không cần credential nào và người dùng chỉ cần tạo bucket. Cái giá phải trả: mỗi lần tải lên hoặc tải xuống là một request Worker (kiểm tra tồn tại, tải lên, mỗi thiết bị tải xuống một lần), và byte đi qua Worker và Durable Object nên bị giới hạn bởi body request 100 MB của gói miễn phí. Vì vậy mỗi đối tượng tối đa 64 MiB theo mặc định và không bao giờ quá 90 MiB dù cấu hình thế nào. Chi phí CPU của việc băm không rơi vào Worker: server truyền SHA-256 của bản mã cho R2 làm checksum và R2 tự kiểm tra khi nhận luồng, nên địa chỉ nội dung được bảo đảm mà Worker không tốn CPU. Đo cục bộ: một ngày dùng điển hình (ba thiết bị, mười file đính kèm) tốn khoảng 40 request Worker cho file, xem bảng ngân sách bên dưới.
- **Một request, một lần gọi Durable Object.** Worker phía trước chỉ kiểm tra không cần trạng thái (User-Agent ứng dụng, không có `Origin`, độ dài body) rồi chuyển nguyên request sang Durable Object. Xác thực phiên, giới hạn đăng nhập, hạn mức file và ghi chỉ mục đều nằm trong object, nên số lần gọi không nhân lên.
- **Log là một bảng; snapshot là một dòng của chính bảng đó.** Mỗi dòng có số thứ tự toàn cục. Snapshot nhận một số thứ tự mới và, khi được nhận, các dòng cũ của tài liệu đó được thay bằng "bia mộ" (payload bị xoá, id giữ lại) để lần đẩy lại muộn vẫn bị nhận ra là trùng. Bia mộ được dọn sau 30 ngày. Quy tắc đầy đủ ở `docs/sync-protocol.md`.
- **Id cập nhật do client sinh ngẫu nhiên (16 byte) thay cho bộ đếm theo thiết bị.** Bộ đếm cho phép khử trùng không tốn chỉ mục, nhưng nếu thiết bị khôi phục outbox từ bản sao lưu cũ thì các update mới bị coi là trùng và mất âm thầm. Id ngẫu nhiên không có kiểu hỏng đó; cái giá là một chỉ mục `UNIQUE` (thêm một dòng ghi mỗi update).
- **Không dùng `AUTOINCREMENT`.** Nó ghi thêm một dòng vào `sqlite_sequence` mỗi lần chèn. Thay vào đó số thứ tự là `rowid`, và dòng có số lớn nhất không bao giờ bị xoá (việc dọn bia mộ loại nó ra), để số thứ tự không bao giờ được dùng lại.
- **Snapshot nằm trong dòng SQLite**, tối đa 960 KiB, vừa một frame WebSocket và dưới giới hạn 2 MB mỗi dòng. Tài liệu có trạng thái lớn hơn mức đó chưa nén được log; chuyển snapshot sang R2 là việc có thể làm sau mà không đổi giao thức.
- **Giới hạn đăng nhập sai nằm trong object**, theo tên tài khoản và theo địa chỉ IP (`CF-Connecting-IP`), khoá tạm thời tăng gấp đôi sau mỗi lần sai kể từ lần thứ 5, tối đa một giờ. Tên không tồn tại có bộ đếm và salt giả ổn định như tên thật. Đánh đổi: kẻ tấn công biết tên tài khoản có thể khoá chủ tài khoản tối đa một giờ; chấp nhận vì tài khoản duy nhất và thời gian khoá bị chặn trên.
- **`wrangler.toml` khai báo binding riêng cho từng môi trường** (`dev`, `production`), vì Wrangler không kế thừa binding giữa các môi trường. Tài liệu Cloudflare hiện mô tả trường `exports` sẽ thay mảng `migrations`; `migrations` vẫn chạy được với bản `wrangler@latest` dùng để kiểm thử, cần đối chiếu lại khi triển khai thật.

## Bằng chứng đã có
- `server/` biên dịch sang WebAssembly và chạy được dưới `wrangler dev` cục bộ ngày 2026-11-21.
- `node server/spikes/ws-relay-check.mjs`: hai kết nối, 200 update nhị phân 512 byte; bên gửi nhận đủ 200 ack có số thứ tự tăng liên tục từ 1 đến 200, bên nhận nhận đủ 200 update. Đây là số đo cục bộ, không phản ánh độ trễ hay CPU trên mạng Cloudflare.
- Hạn mức gói miễn phí đã đối chiếu với tài liệu chính thức, xem `docs/research/cloudflare-free-tier.md`. Với một người dùng ba thiết bị, không hạn mức đếm được nào bị vượt theo ước tính.

## Ràng buộc thiết kế rút ra
- Durable Object trên gói miễn phí chỉ có backend SQLite. Hạn mức mỗi ngày: 100.000 request, 13.000 GB-s, 100.000 dòng ghi; reset lúc 00:00 UTC.
- WebSocket phải được nhận qua `state.accept_web_socket` để object ngủ được; không dùng alarm hay timer làm heartbeat. Ping của ứng dụng phải dùng auto-response để không đánh thức object.
- Gom nhiều chỉnh sửa vào một lần đẩy, và giữ số lần gọi tới Durable Object cho mỗi lần đẩy ở mức một.
- File đính kèm đi qua binding R2 của Worker (không còn URL ký sẵn, xem mục quyết định bổ sung); mỗi đối tượng bị giới hạn dưới 100 MB.
- Client phải gửi User-Agent mô tả ứng dụng; `workers.dev` chặn User-Agent mặc định của thư viện.
- Cần `wrangler` bản mới. Bản 4.68.1 không đóng gói được đầu ra của `worker-build` 0.9 (lỗi "Source phase imports are not available"); `npx wrangler@latest` chạy được.

## Chưa kiểm chứng
- **CPU 10 ms mỗi request trên gói miễn phí.** Chưa đo cho đường đẩy update của Rust/WASM. Đây là rủi ro lớn nhất; cần đo trên một bản triển khai thật trước phase 5.
- **Hibernation trên gói miễn phí** chưa được tài liệu xác nhận tường minh. Nếu object không ngủ, thời lượng có thể tới 85% hạn mức ngày.
- **R2 có đòi thẻ thanh toán hay không** khi chỉ dùng trong hạn mức miễn phí. Người dùng đã chấp nhận trường hợp phải thêm thẻ.
- Hành vi chính xác khi chạm hạn mức ngày của Durable Object.

## Đo cục bộ ngày 2026-11-21
Các số dưới đây lấy từ `wrangler dev` trên máy phát triển bằng bài kiểm thử `request_budget` (`crates/ne-sync/tests/request_budget.rs`): ba thiết bị, mỗi thiết bị 300 lần sửa theo từng đợt 10 lần (mỗi đợt thành một lần đẩy), tám lần nối lại mỗi thiết bị, mười file đính kèm mỗi file hai lần tải xuống, mỗi thiết bị nén một tài liệu.

| Hạng mục | Dùng | Hạn mức Free | Tỉ lệ |
|---|---|---|---|
| Request Worker (HTTP và WebSocket upgrade) | 79 | 100.000 | 0,08% |
| Request Durable Object, đếm mọi message | 205 | 100.000 | 0,20% |
| Request Durable Object, tỉ lệ 20:1 cho message | 86 | 100.000 | 0,09% |
| Dòng ghi của Durable Object | 4.532 | 100.000 | 4,5% |
| Dòng đọc của Durable Object | 7.593 | 5.000.000 | 0,15% |

Đây là số đếm do chính server ghi (`cursor.rowsWritten`, số request và message), không phải số hoá đơn của Cloudflare. **CPU mỗi request không đo được cục bộ**: `wrangler dev` không áp giới hạn 10 ms và thời gian trên máy phát triển không đại diện cho Cloudflare. Mã giữ mỗi request ở mức nhỏ (không băm payload, không giải mã, không quét bảng lớn; một lần đẩy là các `INSERT` đơn lẻ), nhưng chưa có số đo thật.

## Giới hạn hiện tại của mã
Server đã có tài khoản, phiên, giới hạn đăng nhập, đồng bộ WebSocket, nén snapshot và file đính kèm; không còn đường không xác thực. Các tuyến `/v1/_test/*` (xoá sạch, đổ toàn bộ byte đang lưu, bộ đếm) chỉ tồn tại khi biến `TEST_INSPECT` bằng `1`, tệp cấu hình không bao giờ đặt biến này. Chưa triển khai lên Cloudflare thật; các mục "Chưa kiểm chứng" ở trên vẫn mở. Cần làm trên tài khoản thật: đo CPU của một lần đẩy 100 update và của một lần pull 200 dòng bằng `wrangler tail`, xác nhận object thật sự ngủ khi rảnh (thời lượng Durable Object trong dashboard không tăng khi ba thiết bị đang kết nối mà không sửa gì), và kiểm tra checkout R2 có đòi thẻ không.
