# Giao thức đồng bộ

- Trạng thái: mô tả những gì `server/`, `crates/ne-sync/protocol` (định dạng dùng chung) và `crates/ne-sync` đã cài đặt, ngày 2026-11-21 (đã gồm khôi phục mật khẩu bằng recovery key, đối tượng lớn và `last_seen_at`). Đã kiểm thử cục bộ với `wrangler dev`; chưa chạy trên Cloudflare thật.
- Phiên bản giao thức: 1 (byte đầu của mọi frame WebSocket). Mọi thay đổi không tương thích phải tăng số này.
- Quyết định kiến trúc và số đo ở `docs/decisions/0004-sync-server-cloudflare-free.md`; mô hình đe doạ ở `docs/security-model.md`.

## 1. Mô hình

Một người, một server, nhiều thiết bị. Mỗi thiết bị giữ bản đầy đủ dữ liệu cục bộ. Server là một Worker phía trước và một Durable Object nền SQLite duy nhất; nó chỉ thấy bản mã. Mỗi update CRDT được mã hoá thành một phong bì (`ne-crypto::envelope`) bằng khoá workspace; id workspace, id tài liệu và id thiết bị nằm trong dữ liệu xác thực kèm theo (AAD), nên server không chuyển được update sang nơi khác mà vẫn mở được.

Log là một dãy dòng có số thứ tự toàn cục `seq` (số nguyên tăng dần, bắt đầu từ 1, không bao giờ dùng lại). Mỗi thiết bị giữ một **cursor**: vị trí mà mọi dòng có `seq` không lớn hơn nó đã được tính đến, tức đã áp dụng hoặc đã được một snapshot thay thế. Cursor lưu bền cùng với dữ liệu.

## 2. HTTP API

Mọi đường dẫn nằm dưới `/v1`. Thân yêu cầu và phản hồi là JSON (trừ file đính kèm), tối đa 64 KiB. Mọi yêu cầu `POST` và `PUT` phải có `Content-Length`.

Yêu cầu bị từ chối trước khi tới Durable Object (403) nếu: thiếu `User-Agent` bắt đầu bằng `NoteEverywhere/`, hoặc có header `Origin` (CORS đóng hẳn: trình duyệt luôn gửi `Origin`, client gốc thì không). Không có header CORS nào được trả về ở bất kỳ phản hồi nào.

Xác thực: `Authorization: Bearer <token>`. Token là 32 byte ngẫu nhiên mã hoá base64url không đệm (43 ký tự); server chỉ lưu SHA-256 của nó.

| Phương thức, đường dẫn | Xác thực | Mô tả |
|---|---|---|
| `GET /v1/health` | không | `{"status":"ok"}`; không gọi Durable Object |
| `POST /v1/account/salt` | không | `{username}` → `{kdf_salt, kdf_params}`. Tên không tồn tại nhận salt giả xác định và cùng tham số với tài khoản thật |
| `POST /v1/account/register` | mã thiết lập | `{setup_code, username, device_label, auth_key, recovery_auth_key, record}` → 201 phiên. Chỉ thành công một lần cho cả đời server |
| `POST /v1/account/recovery` | không | `{username}` → `{root_key_by_recovery, workspace_key}`: hai khoá đã bọc mà recovery key mở được. Tên không tồn tại nhận hai blob giả cùng hình dạng (mục 12) |
| `POST /v1/account/recover` | không | `{username, recovery_auth_key, new_auth_key, device_label, record}` → 200 phiên mới: đặt lại mật khẩu bằng recovery key, thu hồi mọi phiên (mục 12) |
| `POST /v1/session/login` | không | `{username, auth_key, device_label}` → 200 phiên |
| `POST /v1/session/logout` | phiên | 204; đóng WebSocket của phiên |
| `GET /v1/sessions` | phiên | danh sách `{id, label, created_at, last_seen_at, expires_at, current}` (mục 2.1) |
| `DELETE /v1/sessions/{id}` | phiên | thu hồi một phiên và đóng WebSocket của nó; 404 nếu không có |
| `POST /v1/account/password` | phiên | `{current_auth_key, new_auth_key, record}` → 204; thu hồi mọi phiên khác |
| `GET /v1/account/record` | phiên | `AccountRecord` |
| `PUT`, `GET /v1/vault` | phiên | `VaultRecord` đã bọc; `GET` trả 404 nếu chưa có |
| `PUT`, `GET /v1/workspace-key` | phiên | `{workspace_key}`: khoá workspace đã bọc bằng khoá gốc |
| `GET /v1/sync?device=<id>` | phiên | nâng cấp WebSocket (mục 3) |
| `PUT`, `GET`, `HEAD`, `DELETE /v1/blobs/{sha256}` | phiên | file đính kèm (mục 8) |
| `PUT`, `GET /v1/parts/{group}/{index}`, `GET /v1/parts/{group}` | phiên | phần của đối tượng lớn (mục 11) |

Phản hồi đăng ký và đăng nhập: `{token, session_id, expires_at (ms), workspace_id, record}`. `record` có dạng của `ne-crypto::account::AccountRecord` (mọi byte là base64url không đệm). Server tính lại `auth_verifier` từ `auth_key` nhận được và bỏ qua giá trị client gửi trong `record`; nó kiểm tra độ dài mọi trường, không bao giờ mở các khoá đã bọc.

### Lỗi HTTP

Thân lỗi luôn là `{"error": "<mã>"}` (thêm `retry_after` giây khi bị khoá), không bao giờ có chi tiết nội bộ.

| Trạng thái | Mã | Ý nghĩa |
|---|---|---|
| 400 | `bad_request` | JSON sai, trường sai độ dài hoặc có trường lạ |
| 401 | `invalid_credentials` | sai tên hoặc mật khẩu (không phân biệt hai trường hợp) |
| 401 | `unauthorized` | thiếu, hết hạn hoặc đã thu hồi token |
| 403 | `forbidden` | sai mã thiết lập, không phải client ứng dụng, có `Origin` |
| 403 | `registration_closed` | đã có tài khoản |
| 404 | `not_found` | không có đối tượng; cũng dùng cho đường dẫn lạ |
| 409 | `recovery_unavailable` | tài khoản tạo trước khi có xác minh recovery nên server không kiểm tra được recovery key (mục 12) |
| 411 | `length_required` | thiếu `Content-Length` |
| 413 | `payload_too_large` | thân quá giới hạn |
| 429 | `locked` | đang bị khoá do đăng nhập hoặc khôi phục sai; `Retry-After` và `retry_after` |
| 503 | `unavailable` | lỗi lưu trữ hoặc chạm hạn mức của Cloudflare; thử lại sau |
| 507 | `quota_exceeded` | hạn mức tổng của file đính kèm đã đầy |

### Giới hạn đăng nhập sai

Mỗi lần sai tính cho hai bộ đếm: tên tài khoản được thử (kể cả tên không tồn tại) và địa chỉ IP (`CF-Connecting-IP`). Bốn lần sai đầu không bị khoá. Từ lần sai thứ năm, khoá `30 s`, gấp đôi sau mỗi lần sai tiếp theo, tối đa một giờ. Trong lúc đang khoá, yêu cầu bị từ chối ngay với 429 và không tăng bộ đếm, nên spam không kéo dài khoá. Đăng nhập thành công xoá bộ đếm; bộ đếm tự quên sau 24 giờ không có lỗi mới. Số bộ đếm tối đa 2000 để dữ liệu không phình lên khi bị tấn công phun tên. Đăng ký sai mã thiết lập và đổi mật khẩu sai mật khẩu cũ dùng cùng cơ chế. Khôi phục bằng recovery key dùng cùng chính sách leo thang nhưng với **bộ đếm riêng** (theo tên và theo IP, tiền tố khác với đăng nhập): người quên mật khẩu thường đã đăng nhập sai nhiều lần và vẫn phải khôi phục được, còn đoán recovery key không được phép khoá chủ tài khoản khỏi đăng nhập bằng mật khẩu. Các hằng số có thể chỉnh bằng biến `LOCKOUT_FREE_ATTEMPTS` và `LOCKOUT_BASE_SECONDS`.

### 2.1 Hoạt động của phiên (`last_seen_at`)

Mỗi phiên có `last_seen_at` (ms). Phiên mới có `last_seen_at = created_at`. Server **không** ghi một dòng cho mỗi request: nó chỉ cập nhật trên các request đã vốn ghi dữ liệu (Push, Snapshot, Commit, tải phần lên) hoặc khi mở WebSocket, và chỉ khi giá trị đang lưu cũ hơn `SESSION_TOUCH_SECONDS` (mặc định 600 s). Câu lệnh là `UPDATE ... WHERE id = ? AND last_seen_at <= now - khoảng`, nên trong khoảng đó nó không ghi dòng nào (chỉ một lần đọc). Hệ quả: `last_seen_at` chính xác tới cỡ 10 phút, và một phiên bận cũng chỉ tốn một dòng ghi mỗi khoảng. Pull, danh sách phiên và các request chỉ đọc không đẩy giá trị này. Phiên cũ trước khi có cột trả `created_at`.

## 3. WebSocket

`GET /v1/sync?device=<id>` với `Upgrade: websocket` và header xác thực. `device` là id cài đặt của thiết bị (`ids::is_valid_id`: 1 đến 64 ký tự `A-Z a-z 0-9 . _ : -`), do client sinh ngẫu nhiên một lần và gắn với dữ liệu cục bộ của nó.

- Server nhận kết nối bằng `state.accept_web_socket` (có thẻ là id phiên và `d-<device>`) nên Durable Object ngủ được giữa các message. Trạng thái từng kết nối (`session`, `device`, hạn phiên, cờ `live`) nằm trong attachment của WebSocket để sống qua hibernation.
- Keepalive ở tầng ứng dụng: client gửi frame **văn bản** `ping` mỗi 30 giây; runtime tự trả `pong` bằng auto-response mà không đánh thức object. Không có alarm hay timer nào. Client coi kết nối là chết nếu không nhận được gì trong ba chu kỳ.
- Một thiết bị nối lại sẽ thay kết nối cũ cùng `device` (đóng với mã 4000). Tối đa 16 kết nối.
- Mã đóng do server đặt: `4000` bị thay, `4001` phiên đã kết thúc (client dừng hẳn, cần đăng nhập lại), `1008` vi phạm giao thức, `1009` frame quá lớn, `1003` không phải frame nhị phân.
- Client dùng frame **nhị phân** cho mọi thứ khác. Mỗi frame nhị phân tối đa 1 MiB theo cả hai chiều.

## 4. Frame nhị phân

Số nguyên big-endian. `id` = `len u8` rồi các byte UTF-8, hợp lệ theo `ids::is_valid_id`. `blob` = `len u32` rồi các byte. Mọi frame bắt đầu bằng `version u8 = 1` và `type u8`. Frame có byte thừa hoặc thiếu đều bị từ chối.

### Client gửi

| Kiểu | Tên | Thân |
|---|---|---|
| `0x01` | Push | `req_id u32`, `count u16` (1 đến 100), rồi `count` lần: `client_id [16]`, `doc id`, `payload blob` (1 đến 256 KiB) |
| `0x02` | Pull | `req_id u32`, `cursor u64` |
| `0x03` | Snapshot | `req_id u32`, `client_id [16]`, `doc id`, `upto u64`, `payload blob` (1 đến 960 KiB) |
| `0x04` | Commit | `req_id u32`, `client_id [16]`, `doc id`, `kind u8` (0 update, 1 snapshot), `upto u64` (0 với update), `parts u16` (1 đến 64): công bố đối tượng lớn (mục 11) |

`req_id` do client chọn, bắt đầu từ 1 (0 dành cho frame không được yêu cầu của server). Tổng payload của một Push tối đa 768 KiB.

### Server gửi

| Kiểu | Tên | Thân |
|---|---|---|
| `0x81` | Ack | `req_id u32`, `head u64`, `count u16`, rồi `count` số `seq u64` |
| `0x82` | Updates | `req_id u32`, `flags u8` (bit 0 = xong), `next_cursor u64`, `head u64`, `count u16`, rồi `count` lần: `seq u64`, `kind u8` (0 update, 1 snapshot, 2 update lớn, 3 snapshot lớn), `upto u64`, `doc id`, `device id`, `payload blob` |
| `0x83` | SnapshotAck | `req_id u32`, `status u8` (0 nhận, 1 cũ, 2 từ chối), `dropped u32` |
| `0x8f` | Error | `req_id u32`, `code u16` |

Mã lỗi: `1` sai định dạng, `2` quá lớn, `3` không khả dụng (lưu trữ hoặc hạn mức; thử lại), `4` hết phiên. Lỗi 1 và 2 do khung sai đi kèm đóng kết nối. Mã lạ được client coi là lỗi giao thức.

`head` là `seq` lớn nhất trong log tại thời điểm trả lời.

## 5. Thứ tự và tính bất biến khi gửi lại (idempotency)

- `client_id` là 16 byte ngẫu nhiên sinh khi update được đưa vào outbox và lưu cùng nó. Mọi lần gửi lại dùng đúng id đó (payload được mã hoá lại với nonce mới nhưng id không đổi).
- Server chèn `INSERT OR IGNORE` theo `client_id` (ràng buộc `UNIQUE`). Dòng mới nhận `seq` kế tiếp; dòng đã có không đổi gì. Ack trả `seq` của dòng mới, hoặc `0` cho dòng đã có từ trước (kể cả trùng ngay trong một Push). Client coi `0` cũng là đã nhận.
- Server xử lý message theo đúng thứ tự đến và gán `seq` theo thứ tự đó; Ack của hai Push gửi liền nhau đến cùng thứ tự. Client không dựa vào thứ tự giữa các Push khác nhau vì update CRDT giao hoán và áp dụng lặp lại là vô hại; thứ tự `seq` chỉ quyết định thứ tự phân phối.
- Một Push có thể bị dừng giữa chừng (lỗi lưu trữ, chạm hạn mức). Các dòng đã chèn vẫn được phát cho thiết bị khác, server trả Error 3, client giữ nguyên outbox và gửi lại cả lô sau khi chờ lùi; các dòng đã nhận trả `0`.
- Bia mộ (mục 7) giữ `client_id` của dòng đã bị nén trong 30 ngày, nên lần gửi lại muộn vẫn được nhận ra. Sau 30 ngày một lần gửi lại như vậy có thể tạo dòng trùng; áp dụng update CRDT hai lần không đổi nội dung.

## 6. Pull, cursor và phân phối trực tiếp

**Pull.** `Pull(cursor)` trả mọi dòng còn payload có `seq > cursor`, theo thứ tự `seq`, gồm cả snapshot. Mỗi trang tối đa 200 dòng và gần 1 MiB (luôn có ít nhất một dòng để tiến lên được). Nếu còn nữa, `done = 0` và `next_cursor` là `seq` của dòng cuối trang; client gửi `Pull(next_cursor)` tiếp. Trang cuối có `done = 1` và `next_cursor = head`, nhảy qua cả các bia mộ.

**Live.** Khi một Pull kết thúc (`done = 1`), kết nối trở thành **live** (cờ nằm trong attachment). Từ đó mọi Push mới của thiết bị khác được gửi ngay tới kết nối live dưới dạng frame Updates với `req_id = 0`, `done = 1`, `next_cursor = head = seq cuối`. Kết nối chưa live không nhận frame trực tiếp (nó sẽ thấy các dòng đó qua Pull); điều này loại trừ khoảng hở giữa lúc kết nối và lúc Pull được xử lý, vì Durable Object xử lý tuần tự. Snapshot không được phát trực tiếp: thiết bị live đã có mọi dòng nó bao phủ.

**Cursor ở client.**
- Trang Pull và frame live: áp dụng các dòng có `seq > cursor` rồi đặt cursor `= next_cursor`, trong **một** thao tác nguyên tử của kho lưu trữ. Cursor không bao giờ giảm.
- Ack: nếu kết nối đã live, đặt cursor `= head` của Ack. Hợp lệ vì các dòng của thiết bị khác có `seq < head` đã tới kết nối này, trên cùng một luồng có thứ tự, trước Ack.
- Dòng do chính thiết bị này ghi (`device_id` bằng id của nó) không bị áp dụng lại nhưng vẫn được tính vào cursor.
- Nếu `head` của server nhỏ hơn cursor cục bộ, server đã bị xoá hoặc khôi phục từ bản cũ; client dừng với trạng thái lỗi `ServerReset` thay vì lùi cursor.

**Nối lại.** Client mở kết nối mới, gửi `Pull(cursor)` từ cursor bền, sau đó gửi lại phần outbox chưa được Ack. Chờ lùi mũ có jitter: `min(60 s, 0,5 s x 2^lần)`, nhân với một số ngẫu nhiên đều trong `[0,5; 1]`. Đặt lại sau khi một Pull hoàn tất.

## 7. Nén bằng snapshot

Mục tiêu: giữ log không phình mãi mà thiết bị nào, kể cả thiết bị mới hoặc thiết bị ngoại tuyến lâu, cũng hội tụ về cùng nội dung.

**Snapshot** của tài liệu `D` tại `upto = N` là trạng thái đầy đủ của `D` bao gồm mọi update của `D` có `seq <= N`. Nó được mã hoá bằng cùng phong bì (AAD: workspace, `D`, id thiết bị gửi).

**Điều kiện client.** Client đọc `N` = cursor hiện tại **trước** khi xuất trạng thái tài liệu. Vì cursor chỉ tăng sau khi áp dụng, trạng thái xuất ra chứa ít nhất mọi update của `D` đến `N`; nếu có thêm (update mới đến trong lúc xuất, update cục bộ chưa gửi) thì vô hại vì áp dụng lặp lại không đổi gì.

**Quy tắc server.** Khi nhận `Snapshot(D, N, payload)`:
1. Nếu `client_id` đã tồn tại: trả `Accepted{dropped: 0}` (gửi lại).
2. Nếu `N > head`: trả `Rejected`. Không thể bao phủ các dòng chưa tồn tại.
3. Nếu `N <= max(upto)` của snapshot còn sống của `D`: trả `Stale`.
4. Ngược lại: chèn một dòng `kind = 1, upto = N` với `seq` mới (> `head`), rồi biến thành bia mộ (xoá `payload`, giữ `client_id`) mọi dòng còn sống của `D` thoả `seq <= N` hoặc là snapshot cũ. Trả `Accepted{dropped}` với `dropped` là số update bị thay. Dọn tối đa 200 bia mộ đã quá 30 ngày (không bao giờ dòng có `seq` lớn nhất).

Dòng giữ lại: mọi update của `D` có `seq > N`, mọi dòng của tài liệu khác.

**Vì sao thiết bị nào cũng hội tụ.** Gọi `c` là cursor của một thiết bị, `S` là `seq` của snapshot (S > N).
- Nếu `c < S`: Pull trả snapshot cùng mọi dòng còn sống có `seq > c`. Các update của `D` trong `(c, N]` đã bị thay, nhưng nằm trong snapshot. Các update của `D` có `seq <= c` thiết bị đã có. Các update của `D` có `seq > N` còn sống nên được gửi. Vậy thiết bị có mọi thứ.
- Nếu `c >= S`: thiết bị đã nhận snapshot qua Pull (hoặc đã có mọi dòng nó bao phủ vì đang live), vì `seq` được gán theo thứ tự và Pull `c` trả đủ mọi dòng `<= c`.
- Thay snapshot cũ bằng snapshot mới an toàn vì snapshot mới được xuất từ trạng thái đã chứa mọi update của `D` đến `N' > N` kể cả phần snapshot cũ bao phủ.
- Thiết bị live bỏ qua snapshot mà nó không nhận trực tiếp, nhưng nó đã nhận mọi dòng có `seq <= head` qua luồng có thứ tự, nên trạng thái của nó chứa nội dung snapshot.

**Thiết bị mới** (cursor 0) nhận trong các trang Pull: mỗi tài liệu có tối đa một snapshot còn sống cộng các update sau nó, cộng mọi dòng của tài liệu chưa nén. Áp dụng theo thứ tự bất kỳ (CRDT).

**Mã hoá hàng.** Plaintext được niêm phong có byte đầu cho biết loại: `0x00` rồi update; `0x01`, `upto u64`, rồi trạng thái. Client kiểm tra loại và `upto` khớp với dòng server báo, nên server không đổi nhãn update thành snapshot hay sửa `upto` mà không bị phát hiện. Một dòng không mở được hoặc sai loại làm client dừng (`CorruptData`) chứ không bỏ qua, vì bỏ qua sẽ mất dữ liệu âm thầm.

Snapshot nằm trong dòng SQLite nên bản mã tối đa 960 KiB; trạng thái lớn hơn đi theo cơ chế đối tượng lớn (mục 11), nên mọi tài liệu đều nén được tới giới hạn đối tượng lớn.

## 8. File đính kèm

Lưu trong R2 qua binding của Worker, khoá `blobs/<sha256 hex của bản mã>`. Server không cần credential S3.

- Client mã hoá luồng (`ne-crypto::stream`, khối 64 KiB, AAD gồm id workspace và id đính kèm) vào file tạm trong lúc tính SHA-256 của bản mã, rồi tải lên luồng từ file đó. Bộ nhớ không phụ thuộc kích thước.
- `PUT /v1/blobs/{hash}`: cần `Content-Length`, tối đa `BLOB_MAX_BYTES` (mặc định 64 MiB, trần cứng 90 MiB). Server đặt chỗ trong chỉ mục (tính vào hạn mức), rồi truyền thân vào R2 với checksum SHA-256 là `hash`; R2 từ chối nếu nội dung không khớp (400). Đối tượng đã có: 200 `{"stored":false}` mà không đọc thân. Mới lưu: 201.
- `HEAD`: 200 với header `X-Blob-Size`, hoặc 404. `GET`: luồng bản mã, hoặc 404. `DELETE`: 204 kể cả khi không có.
- Hạn mức tổng `BLOB_QUOTA_BYTES` (mặc định 8 GiB) tính theo chỉ mục, kể cả chỗ đang đặt; chỗ đặt không hoàn tất bị bỏ sau một giờ. Vượt: 507.
- Client xác minh khi tải xuống: băm bản mã và so với địa chỉ, rồi giải mã vào file tạm và chỉ đổi tên thành file đích khi mọi khối xác thực. Sai một trong hai: lỗi `Tampered`, không để lại gì. AAD khiến byte của đính kèm này không dùng được cho đính kèm khác.
- Tiếp tục tải: đơn vị là cả đối tượng. Tải lên hỏi `HEAD` trước và bỏ qua nếu đã có; gặp lỗi tạm thời thử lại tối đa bốn lần với chờ lùi.

## 9. Giới hạn tóm tắt

| Mục | Giá trị |
|---|---|
| Frame WebSocket | 1 MiB |
| Payload một update (trực tiếp) | 256 KiB (gồm 42 byte phong bì) |
| Payload một snapshot (trực tiếp) | 960 KiB |
| Một phần của đối tượng lớn | 1 MiB bản mã (1.047.552 byte bản rõ), tối đa 64 phần |
| Một update hoặc snapshot lớn | 48 MiB bản rõ (client từ chối trên mức này) |
| Update mỗi Push / tổng payload | 100 / 768 KiB |
| Dòng mỗi trang Pull | 200 |
| Id tài liệu, thiết bị, workspace | 64 ký tự |
| Thân JSON | 64 KiB |
| Đối tượng đính kèm | 64 MiB mặc định, 90 MiB trần |
| Kết nối đồng thời | 16 |
| Phiên đồng thời | 50 (phiên cũ nhất bị bỏ) |
| Cập nhật `last_seen_at` | tối đa một lần mỗi phiên mỗi 600 s (`SESSION_TOUCH_SECONDS`) |
| Phần chưa công bố | bị dọn sau 24 giờ |
| Hạn phiên | 30 ngày (`SESSION_TTL_DAYS`) |
| Bia mộ | 30 ngày |

## 10. Những gì server vẫn thấy và không làm được

Xem `docs/security-model.md` cho phân tích đầy đủ. Tóm tắt phần liên quan tới giao thức: server thấy id tài liệu, id thiết bị, kích thước, thời điểm và tần suất; nó có thể giữ lại hay xoá dòng, hoặc trả log cũ, và log không có xác thực về thứ tự hay tính đầy đủ. Nó không thể đọc nội dung, chuyển update sang tài liệu hay thiết bị khác, đổi loại dòng, hay làm client chấp nhận file đính kèm bị sửa. Lưu ý thêm của giao thức này: id thiết bị là nhãn được xác thực chứ không chứng minh tác giả, vì mọi thiết bị đều giữ khoá workspace.

## 11. Đối tượng lớn (update và snapshot vượt giới hạn một dòng)

Một update hơn khoảng 256 KiB (trang nhập lớn, snapshot đầu của trang dài) hoặc một snapshot hơn 960 KiB không nằm vừa một dòng log hay một frame. Chúng đi theo cơ chế **staging rồi công bố**, áp dụng đồng nhất cho cả update và snapshot, cho mọi kích thước tới 48 MiB bản rõ.

**Gửi.**
1. Client thêm byte loại như thường lệ (`0x00` rồi update; `0x01`, `upto`, rồi trạng thái), cắt chuỗi đó thành các khúc `PART_CHUNK` = 1.047.552 byte, và niêm phong **từng khúc riêng** bằng phong bì (AAD: workspace, tài liệu, thiết bị). Bản rõ của mỗi phần bắt đầu bằng `0x02`, `group [16]`, `index u16`, `count u16`, rồi khúc dữ liệu. Header nằm trong phần được xác thực nên server không đảo, bỏ, thay hay trộn phần của đối tượng khác mà không bị phát hiện.
2. `group` là `client_id` của dòng log sẽ được tạo. Với update, đó là id của mục outbox (không đổi giữa các lần gửi lại); với snapshot, một id ngẫu nhiên mới cho mỗi lần thử.
3. Client hỏi `GET /v1/parts/{group}` (trả `{"have":[chỉ số...]}`) rồi `PUT /v1/parts/{group}/{index}` từng phần còn thiếu (`Content-Length` bắt buộc, 1 byte đến 1 MiB, `index` < 64). Server lưu `INSERT OR IGNORE` theo `(group, index)`: 201 nếu mới, 200 nếu đã có. Một phần bất kỳ hợp lệ dù do lần thử nào tạo ra vì mọi phần của cùng một `group` chứa cùng bản rõ.
4. Khi đủ phần, client gửi **Commit** (frame `0x04`). Server kiểm tra các phần `0..parts-1` đều có (nếu thiếu trả Error 3 `Unavailable`, client tải lại phần thiếu), rồi chèn **một** dòng log với `kind` 2 (update lớn) hoặc 3 (snapshot lớn) và payload là manifest 18 byte: `group [16]`, `parts u16`. Dòng này nhận `seq` như mọi dòng khác, theo cùng quy tắc: update lớn trả `Ack`, snapshot lớn trả `SnapshotAck` và theo đúng luật snapshot ở mục 7 (`Stale`, `Rejected`, tombstone).

**Nhận.** Dòng kind 2 và 3 đến qua Pull hoặc phát trực tiếp như dòng thường nhưng chỉ mang manifest. Client tải `GET /v1/parts/{group}/{index}` từng phần theo thứ tự, mở từng phần, kiểm tra `group`, `index` và `count` khớp manifest, ghép lại thành chuỗi bản rõ rồi xử lý như một dòng thường (cùng kiểm tra byte loại và `upto`). Chỉ khi ghép xong mới gọi `apply_incoming`.

**Tính chất.**
- *Nguyên tử với bên nhận.* Dòng log chỉ xuất hiện sau khi mọi phần đã bền, và bên nhận không áp dụng gì cho tới khi có đủ phần. Không có trạng thái áp dụng một nửa. Một phần không mở được hoặc sai header làm client dừng với `CorruptData` như mọi dòng hỏng.
- *Lặp lại và mất kết nối giữa chừng.* Phần lưu theo khoá chính nên gửi lại không nhân đôi. Sau khi nối lại, client hỏi `have` và chỉ gửi phần thiếu. Commit lặp lại (cùng `client_id`) trả seq `0` như Push. Bên nhận tải lại từ đầu nếu kết nối đứt khi đang tải (mỗi phần thử tối đa ba lần trước khi bỏ kết nối).
- *Nén và thiết bị mới.* Dòng lớn là dòng log bình thường nên mọi quy tắc nén ở mục 7 áp dụng nguyên văn: snapshot lớn thay các dòng cũ của tài liệu, và thiết bị cursor 0 nhận đúng snapshot cùng các update sau nó. Khi dòng bị tombstone, server xoá luôn các phần của nó (`DELETE FROM parts WHERE grp IN (...)`). Nếu một phần biến mất giữa lúc client nhận trang và lúc tải (dòng bị nén ngay sau đó), client bỏ kết nối và lần Pull kế tiếp mang snapshot thay thế.
- *Dọn rác.* Phần chưa được công bố (client chết giữa chừng) bị xoá khi có một lần tải phần `index` 0 tới và chúng đã quá 24 giờ mà không có dòng log còn sống tham chiếu. Chưa có hạn mức tổng cho vùng staging: nó chỉ chứa dữ liệu của chính phiên đã xác thực của chủ tài khoản.
- *Bộ nhớ.* Server chỉ giữ tối đa một phần (1 MiB) mỗi request, không bao giờ ghép đối tượng. Client giữ bản rõ của một đối tượng cùng bản đã niêm phong một phần tại một thời điểm; trang Pull được áp dụng theo từng đợt khoảng 32 MiB bản rõ, mỗi đợt dời cursor tới dòng cuối của nó (hợp lệ vì dòng đến theo thứ tự).
- *Ngân sách dòng ghi của gói miễn phí.* Bảng `parts` là `WITHOUT ROWID` nên mỗi phần tốn **một** dòng ghi (không có chỉ mục phụ). Một update 30 MiB là 31 phần cộng khoảng ba dòng ghi cho dòng log (chèn và hai chỉ mục), tức chừng 34 dòng; khi bị nén thêm một dòng tombstone và 31 lần xoá phần. Cả vòng đời của một đối tượng 30 MiB dưới 100 dòng ghi, tức dưới 0,1% hạn mức ngày 100.000. Số request: mỗi phần là một request Worker, thêm một request cho `have`; mỗi thiết bị nhận tốn một request mỗi phần.
- *Giới hạn.* Client từ chối `SyncError::TooLarge` với update hoặc trạng thái snapshot trên 48 MiB bản rõ; server giới hạn 64 phần và 1 MiB mỗi phần (kiểm ở Worker phía trước theo `Content-Length` và lại trên số byte nhận được).

## 12. Khôi phục mật khẩu bằng recovery key

Recovery key chỉ là một chuỗi 200 bit; từ nó client dẫn xuất bằng HKDF hai giá trị độc lập (nhãn khác nhau, xem `docs/security-model.md`): khoá bọc khoá gốc (ở lại máy) và **recovery auth key** (gửi cho server). Khi đăng ký client gửi thêm `recovery_auth_key`; server lưu `auth_verifier(recovery_auth_key)` trong `recovery_verifier` của bản ghi tài khoản, đúng như với auth key của mật khẩu, và bỏ qua giá trị client tự điền trong `record`.

Luồng:
1. `POST /v1/account/recovery {username}` trả `root_key_by_recovery` và `workspace_key` (đã bọc). Tên không tồn tại nhận hai blob giả, xác định theo tên và hạt giống của server, cùng độ dài 73 byte và cùng byte phiên bản, nên không phân biệt được với tên thật qua nội dung. Không có throttle ở bước này (cũng như `/account/salt`).
2. Client thử mở khoá gốc bằng recovery key **cục bộ**. Sai khoá hoặc sai tên: lỗi `WrongSecret` ngay trên máy, không có gì bí mật rời thiết bị.
3. Client tạo salt mới, dẫn xuất khoá từ mật khẩu mới, bọc lại khoá gốc, rồi `POST /v1/account/recover {username, recovery_auth_key, new_auth_key, device_label, record}`. Server: kiểm tra độ dài và kiểm tra bộ đếm khoá riêng của khôi phục, so `recovery_auth_key` với `recovery_verifier` bằng so sánh hằng thời gian (tên không tồn tại so với một verifier giả không ai khớp được, nên thời gian và lỗi giống sai khoá). Sai: 401 `invalid_credentials` và tăng bộ đếm khôi phục (theo tên và theo IP). Đúng: xoá bộ đếm, thay `kdf_salt`, `kdf_params`, `root_key_by_password` bằng giá trị mới, đặt `auth_verifier = auth_verifier(new_auth_key)`, **giữ nguyên** `root_key_by_recovery`, `workspace_key` và `recovery_verifier` đã lưu (không tin giá trị trong yêu cầu), thu hồi **mọi** phiên (đóng WebSocket của chúng) và trả một phiên mới như đăng nhập.
4. Dữ liệu không bị mã hoá lại: khoá gốc và khoá workspace không đổi, nên mọi dữ liệu và file đính kèm vẫn mở được và thiết bị mới đồng bộ như thường.

**Recovery key không được xoay.** Sau khôi phục, cùng recovery key vẫn dùng được (và đặt lại được lần nữa). Xoay recovery key (sinh key mới, bọc lại khoá gốc, thay verifier) chưa cài đặt; ai có recovery key luôn đặt lại được mật khẩu và đá mọi thiết bị khác ra, nên phải giữ nó như giữ mật khẩu.

**Tài khoản tạo trước thay đổi này** không có `recovery_verifier` (bản ghi cũ không có trường này; client đọc nó như toàn số 0 và `supports_recovery()` là sai). Dự án đang trước phát hành nên chấp nhận phải tạo tài khoản mới. Server từ chối gọn gàng, không panic: `POST /v1/account/recover` với đúng tên trả 409 `recovery_unavailable` mà không đụng tới bộ đếm hay dữ liệu; tên khác vẫn nhận 401 như tên không tồn tại. Đăng nhập bằng mật khẩu và mọi thứ khác không đổi. Client hiển thị mã `recovery_unavailable` với lời khuyên đăng nhập bằng mật khẩu hoặc tạo tài khoản mới.

## 13. Hành vi phía thiết bị mà giao thức dựa vào

Các điểm dưới đây thuộc về ứng dụng desktop (`apps/desktop/src-tauri/src/app/`), nhưng định dạng và hệ quả của chúng nằm trên dây nên được ghi ở đây.

- **Id tài liệu trên dây.** Id nút của một trang có dạng `<số đếm>@<peer>`, ký tự `@` không hợp lệ theo `ids::is_valid_id`, nên trên dây `@` được thay bằng `:` (và đổi ngược khi nhận). Tài liệu cây có id `tree`, tài liệu vault là `vault`.
- **Chỉ mục đính kèm.** Server lưu file đính kèm theo SHA-256 của **bản mã**, mà thiết bị khác không tự tính được từ hash nội dung trong trang. Bảng ánh xạ hash nội dung sang địa chỉ trên server (`cipher_hash`, kích thước, tên file) đi trong một tài liệu nhỏ được đồng bộ có id `attachments`, bằng chính cơ chế update mã hoá như mọi tài liệu khác.
- **Đăng xuất.** Đăng xuất **giữ dữ liệu cục bộ** và đưa workspace về chế độ chỉ cục bộ: phiên bị thu hồi trên server (cố gắng hết sức), credential trong kho bảo mật của hệ điều hành bị xoá, **outbox, cursor và chỉ mục đính kèm bị xoá**. Lần đăng nhập sau (`queue_full_upload` trong `session.rs`) xếp lại **toàn bộ** dữ liệu cục bộ vào outbox từ cursor 0: snapshot cây, từng trang (kể cả trang trong thùng rác), vault, và mọi tài liệu đồng bộ khác mà `Workspace::synced_documents()` liệt kê (database, thư viện template, nhắc hẹn, nội dung block đồng bộ) — rồi tải lên lại; bên nhận áp dụng update đã có là không đổi gì (mục 5). Cùng một danh sách tài liệu (`workspace_documents()`) cũng là danh sách mà việc nén log phía server (mục 7, `compact_server_log`) duyệt qua.
- **`vault_conflict`.** Đăng nhập vào tài khoản mà bản ghi vault trên server **khác** bản ghi vault cục bộ bị từ chối với mã `vault_conflict` (hai vault có hai khoá riêng nên không gộp được). Chỉ khi một bên chưa có vault thì bên kia được nhận (hoặc được đưa lên).
- **`ServerReset`.** Nếu `head` do server trả nhỏ hơn cursor cục bộ (server bị xoá hoặc khôi phục từ bản cũ), engine dừng với trạng thái lỗi `ServerReset` thay vì lùi cursor, không gửi gì thêm và không ghi đè dữ liệu cục bộ. Thiết bị phải đăng nhập vào server mới (sau khi đăng xuất, dữ liệu cục bộ sẽ được tải lại).
